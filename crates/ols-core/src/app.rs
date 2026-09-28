//! The application core's shared state (`Inner`) and everything the commands do with it.
//! `Core` (in `command.rs`) is a thin handle onto an `Arc<Inner>`, which is what lets a
//! Quick App run on a background thread keep using the same managers the UI does.
//!
//! Lock order, to keep this deadlock-free: `domains` is always taken first; `settings`,
//! `projects` and `custom_installs` are only ever held briefly and never while waiting on
//! anything slow (a process, the network, another lock).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::certs::CertificateManager;
use crate::custom_install::CustomInstallStore;
use crate::dbtools::{self, ExternalToolStore};
use crate::domain::{AppSpec, Domain, DomainStore, Ownership, SiteBlocks, SiteKind};
use crate::error::CoreError;
use crate::health::{self, HealthReport, HealthTarget};
use crate::paths::AppPaths;
use crate::php::PhpPools;
use crate::process::{ProcessId, ProcessSpec, ProcessSupervisor};
use crate::project::{build_detail, Project, ProjectDetail, ProjectStore};
use crate::quickapp::commands::{CommandHistory, QuickCommand, QuickCommandStore};
use crate::quickapp::plan::{self, PlanCtx, RunPlan};
use crate::quickapp::run::{ActionOutcome, ResolvedProgram, RunCtx};
use crate::quickapp::{QuickCatalog, QuickHost, RunManager};
use crate::runtime::{RuntimeEvent, RuntimeManager};
use crate::service::ServiceManager;
use crate::settings::SettingsService;
use crate::sqlite::SqliteStore;
use crate::web::manager::{ApplyContext, ApplyReport, WebManager};
use crate::web::WebConfig;
use notify::RecursiveMode;
use notify_debouncer_mini::new_debouncer;
use tokio::sync::broadcast;

pub struct Inner {
    pub paths: AppPaths,
    pub settings: Mutex<SettingsService>,
    pub projects: Mutex<ProjectStore>,
    pub custom_installs: Mutex<CustomInstallStore>,
    pub domains: Mutex<DomainStore>,
    pub sqlite: Mutex<SqliteStore>,
    pub ext_tools: Mutex<ExternalToolStore>,
    pub catalog: Mutex<QuickCatalog>,
    pub quick_commands: QuickCommandStore,
    pub history: Mutex<CommandHistory>,
    pub supervisor: Arc<ProcessSupervisor>,
    pub runtimes: Arc<RuntimeManager>,
    pub services: Arc<ServiceManager>,
    pub certs: Arc<CertificateManager>,
    pub php: Arc<PhpPools>,
    pub monitor: crate::monitor::Monitor,
    pub web: Arc<WebManager>,
    pub runs: RunManager,
    pub terminals: Arc<crate::terminal::TerminalManager>,
    pub journal: Mutex<crate::journal::Journal>,
    /// Where a database scan or import from Laragon/XAMPP/Wamp is up to.
    pub migration: crate::migrate::Progress,
    /// The running (or last) environment setup, for the UI to follow (§73).
    pub setup: Mutex<Option<crate::setup::SetupReport>>,
    pub workers: Mutex<crate::workers::WorkerStore>,
    pub worker_procs: crate::workers::WorkerProcesses,
    pub schedules: Mutex<crate::scheduler::ScheduleStore>,
    pub task_runs: crate::scheduler::TaskRuns,
    pub profiles: crate::profiles::ProfileStore,
    pub tunnels: crate::tunnel::TunnelManager,
    pub api: crate::api::ApiState,
    pub loadtests: crate::loadtest::LoadRuns,
    pub ai: crate::ai::AiJobs,
    /// Finding ids already attempted by automatic diagnostics during this app session.
    pub auto_fix_attempted: Mutex<HashSet<String>>,
    /// Last automatic-fix failures, shown in the corresponding diagnostic's details.
    pub auto_fix_failures: Mutex<HashMap<String, String>>,
    project_events: broadcast::Sender<()>,
}

fn svc(msg: impl Into<String>) -> CoreError {
    CoreError::ServiceError(msg.into())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequirementView {
    pub id: String,
    pub label: String,
    pub wanted: Option<String>,
    /// "installed" | "installable" | "unavailable"
    pub status: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainSummary {
    pub hostname: String,
    pub url: String,
    pub https: bool,
    pub kind: String,
    pub enabled: bool,
    pub project_id: Option<String>,
    pub has_app: bool,
    /// The folder to open in an editor: the linked project, else the site root without a
    /// trailing `public`/`web` docroot.
    pub folder: String,
    /// Website type it is listed under: "php", "nodejs", "python", "static" or "proxy".
    pub group: String,
    pub public_domain: Option<String>,
    /// Web server that renders this site: its own override, or the default.
    pub server: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthItem {
    pub id: String,
    pub label: String,
    /// "ok" | "warn" | "error"
    pub status: String,
    pub detail: String,
    pub fix: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogSource {
    pub id: String,
    pub name: String,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartupSettings {
    pub with_windows: bool,
    pub start_minimized: bool,
    pub autostart_web: bool,
    pub autostart_services: Vec<String>,
    pub notifications: bool,
    pub close_to_tray: bool,
}

const REG_RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const REG_VALUE: &str = "OpenLocalServer";

impl Inner {
    /// Site files are intentionally allowlisted; names never become arbitrary paths.
    pub fn read_site_file(&self, hostname: &str, name: &str) -> Result<String, CoreError> {
        if name != ".htaccess" {
            return Err(CoreError::DomainError(
                "only .htaccess can be edited here".into(),
            ));
        }
        let domain = self
            .domains
            .lock()
            .unwrap()
            .get(hostname)
            .ok_or_else(|| CoreError::DomainError(format!("{hostname} is not a known site")))?;
        if !matches!(domain.kind, crate::domain::SiteKind::Php { .. }) {
            return Err(CoreError::DomainError(
                ".htaccess editing is available for PHP sites".into(),
            ));
        }
        let root = std::fs::canonicalize(&domain.root).map_err(|e| {
            CoreError::DomainError(format!("site document root is unavailable: {e}"))
        })?;
        let path = root.join(name);
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(String::new()),
            Err(e) => return Err(CoreError::DomainError(e.to_string())),
        };
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(CoreError::DomainError(
                ".htaccess must be a regular file in this site's document root".into(),
            ));
        }
        let actual =
            std::fs::canonicalize(&path).map_err(|e| CoreError::DomainError(e.to_string()))?;
        if actual.parent() != Some(root.as_path()) {
            return Err(CoreError::DomainError(
                ".htaccess must be in this site's document root".into(),
            ));
        }
        std::fs::read_to_string(actual)
            .map_err(|e| CoreError::DomainError(format!("could not read .htaccess: {e}")))
    }

    pub fn write_site_file(
        &self,
        hostname: &str,
        name: &str,
        content: &str,
    ) -> Result<String, CoreError> {
        if name != ".htaccess" {
            return Err(CoreError::DomainError(
                "only .htaccess can be edited here".into(),
            ));
        }
        if content.len() > 256 * 1024 {
            return Err(CoreError::DomainError(
                ".htaccess is limited to 256 KB".into(),
            ));
        }
        let domain = self
            .domains
            .lock()
            .unwrap()
            .get(hostname)
            .ok_or_else(|| CoreError::DomainError(format!("{hostname} is not a known site")))?;
        if !matches!(domain.kind, crate::domain::SiteKind::Php { .. }) {
            return Err(CoreError::DomainError(
                ".htaccess editing is available for PHP sites".into(),
            ));
        }
        let root = std::fs::canonicalize(&domain.root).map_err(|e| {
            CoreError::DomainError(format!("site document root is unavailable: {e}"))
        })?;
        let path = root.join(".htaccess");
        let old = match std::fs::symlink_metadata(&path) {
            Ok(meta) => {
                if !meta.is_file() || meta.file_type().is_symlink() {
                    return Err(CoreError::DomainError(
                        ".htaccess must be a regular file in this site's document root".into(),
                    ));
                }
                let actual = std::fs::canonicalize(&path)
                    .map_err(|e| CoreError::DomainError(e.to_string()))?;
                if actual.parent() != Some(root.as_path()) {
                    return Err(CoreError::DomainError(
                        ".htaccess must be in this site's document root".into(),
                    ));
                }
                Some(std::fs::read(&actual)?)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(CoreError::DomainError(e.to_string())),
        };
        if let Some(old) = &old {
            let history = self.paths.web_dir().join("site-files").join(hostname);
            std::fs::create_dir_all(&history)?;
            let mut stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            while history.join(format!("{stamp}.htaccess")).exists() {
                stamp += 1;
            }
            std::fs::write(history.join(format!("{stamp}.htaccess")), old)?;
        }
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let temp = root.join(format!(".htaccess-{}-{stamp}.tmp", std::process::id()));
        std::fs::write(&temp, content)?;
        if old.is_some() {
            std::fs::remove_file(&path)?;
        }
        if let Err(e) = std::fs::rename(&temp, &path) {
            let _ = std::fs::remove_file(&temp);
            if let Some(old) = old {
                let _ = std::fs::write(&path, old);
            }
            return Err(CoreError::DomainError(format!(
                "could not save .htaccess: {e}"
            )));
        }
        Ok(path.display().to_string())
    }

    pub fn new(settings: SettingsService, paths: AppPaths) -> Result<Arc<Self>, CoreError> {
        paths.ensure_dirs()?;
        let supervisor = Arc::new(ProcessSupervisor::new());
        let runtimes = Arc::new(RuntimeManager::new(paths.clone()));
        Self::with_parts(settings, paths, supervisor, runtimes)
    }

    pub fn with_parts(
        settings: SettingsService,
        paths: AppPaths,
        supervisor: Arc<ProcessSupervisor>,
        runtimes: Arc<RuntimeManager>,
    ) -> Result<Arc<Self>, CoreError> {
        let services = Arc::new(ServiceManager::new(
            paths.clone(),
            runtimes.clone(),
            supervisor.clone(),
        ));
        let certs = Arc::new(CertificateManager::new(&paths));
        let php = Arc::new(PhpPools::new(
            paths.clone(),
            runtimes.clone(),
            supervisor.clone(),
        ));
        let web = Arc::new(WebManager::new(
            paths.clone(),
            runtimes.clone(),
            supervisor.clone(),
            certs.clone(),
            php.clone(),
        ));
        let (project_events, _) = broadcast::channel(16);
        let core = Arc::new(Self {
            settings: Mutex::new(settings),
            projects: Mutex::new(ProjectStore::load(&paths)?),
            custom_installs: Mutex::new(CustomInstallStore::load(&paths)?),
            domains: Mutex::new(DomainStore::load(&paths)?),
            sqlite: Mutex::new(SqliteStore::load(&paths)?),
            ext_tools: Mutex::new(ExternalToolStore::load(&paths)?),
            catalog: Mutex::new(QuickCatalog::load(&paths)?),
            quick_commands: QuickCommandStore::load(&paths)?,
            history: Mutex::new(CommandHistory::load(&paths)?),
            supervisor,
            runtimes,
            services,
            certs,
            php,
            monitor: Default::default(),
            web,
            runs: RunManager::new(),
            terminals: Arc::new(crate::terminal::TerminalManager::new()),
            journal: Mutex::new(crate::journal::Journal::load(&paths)),
            migration: Default::default(),
            setup: Mutex::new(None),
            workers: Mutex::new(crate::workers::WorkerStore::load(&paths)),
            worker_procs: Default::default(),
            schedules: Mutex::new(crate::scheduler::ScheduleStore::load(&paths)),
            task_runs: Default::default(),
            profiles: crate::profiles::ProfileStore::new(&paths),
            tunnels: crate::tunnel::TunnelManager::new(&paths),
            api: Default::default(),
            loadtests: Default::default(),
            ai: Default::default(),
            auto_fix_attempted: Mutex::new(HashSet::new()),
            auto_fix_failures: Mutex::new(HashMap::new()),
            project_events,
            paths,
        });
        core.start_projects_watcher();
        core.sync_php_external();
        core.services.set_limits(core.resource_limits());
        {
            // The Services page shows the web servers too, but they keep running through
            // `WebManager` — this only hands it over for listing and probing.
            let for_config = Arc::clone(&core);
            let for_sites = Arc::clone(&core);
            core.services.attach_web(
                Arc::clone(&core.web),
                Arc::new(move || for_config.web_config()),
                // `apply_web` holds the domains mutex for the full apply pipeline, which
                // can block for several seconds (helper-service reply timeout).  Using
                // `lock().unwrap()` here would deadlock any concurrent `list_services` call
                // that arrives while an apply is in flight.  `try_lock` lets that call
                // return immediately with an empty site-count instead of hanging forever.
                Arc::new(move || {
                    for_sites
                        .domains
                        .try_lock()
                        .map(|g| g.list())
                        .unwrap_or_default()
                }),
            );
        }
        core.apply_plugins();
        Ok(core)
    }

    /// Runs `f` between a "started" and an "ended" line in the operation journal (§78, §163), so an
    /// operation the app dies in the middle of is found and reported on the next start.
    pub fn journaled<T>(
        &self,
        kind: &str,
        title: &str,
        undo: Option<&str>,
        retry: Option<crate::command::CoreCommand>,
        f: impl FnOnce() -> Result<T, CoreError>,
    ) -> Result<T, CoreError> {
        let id = self.journal.lock().unwrap().begin(kind, title, undo, retry);
        let result = f();
        let outcome = match &result {
            Ok(_) => Ok(()),
            Err(e) => Err(e.to_string()),
        };
        self.journal.lock().unwrap().finish(id, &outcome);
        result
    }

    /// Feeds the user-registered PHP installs (custom installs with id "php") to the pool
    /// manager. Call after loading and after any custom-install change.
    pub fn sync_php_external(&self) {
        let entries = self
            .custom_installs
            .lock()
            .unwrap()
            .list()
            .into_iter()
            .filter(|c| c.id == "php" && !c.label.is_empty())
            .map(|c| (c.label, PathBuf::from(c.path)))
            .collect();
        self.php.set_external(entries);
    }

    // ------------------------------------------------------------------- settings

    pub fn web_config(&self) -> WebConfig {
        WebConfig::from_settings(&self.settings.lock().unwrap())
    }

    pub fn setting_bool(&self, key: &str, default: bool) -> bool {
        self.settings
            .lock()
            .unwrap()
            .get(key)
            .and_then(|v| v.as_bool())
            .unwrap_or(default)
    }

    pub fn setting_string(&self, key: &str, default: &str) -> String {
        self.settings
            .lock()
            .unwrap()
            .get(key)
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| default.to_string())
    }

    fn default_projects_dir(&self) -> PathBuf {
        if let Some(dir) = self
            .settings
            .lock()
            .unwrap()
            .get("quickapps.projects_dir")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return PathBuf::from(dir);
        }
        self.paths.sites_dir()
    }

    pub fn sites_dir(&self) -> PathBuf {
        self.paths.sites_dir()
    }

    pub fn subscribe_project_events(&self) -> broadcast::Receiver<()> {
        self.project_events.subscribe()
    }

    fn start_projects_watcher(self: &Arc<Self>) {
        if !self.setting_bool("projects.watch", true) {
            return;
        }
        let mut roots = self.string_list(PROJECT_ROOTS);
        let default_root = self.paths.sites_dir().display().to_string();
        if !roots
            .iter()
            .any(|root| root.eq_ignore_ascii_case(&default_root))
        {
            roots.push(default_root);
        }
        let core = Arc::clone(self);
        std::thread::spawn(move || {
            let (events_tx, events_rx) = std::sync::mpsc::channel();
            let Ok(mut debouncer) =
                new_debouncer(std::time::Duration::from_millis(1500), events_tx)
            else {
                return;
            };
            let mut watched = std::collections::HashSet::new();
            for root in roots {
                let path = PathBuf::from(root);
                if path.is_dir() {
                    if debouncer
                        .watcher()
                        .watch(&path, RecursiveMode::NonRecursive)
                        .is_ok()
                    {
                        watched.insert(path);
                    }
                }
            }
            loop {
                match events_rx.recv_timeout(Duration::from_millis(500)) {
                    Ok(_) => {
                        if let Err(error) = core.sync_auto_domains() {
                            tracing::warn!(%error, "automatic domains could not be synced after a project change");
                        }
                        let _ = core.project_events.send(());
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                }
                let desired: std::collections::HashSet<PathBuf> = core
                    .string_list(PROJECT_ROOTS)
                    .into_iter()
                    .map(PathBuf::from)
                    .collect();
                for path in watched.clone() {
                    if !desired.contains(&path) {
                        let _ = debouncer.watcher().unwatch(&path);
                        watched.remove(&path);
                    }
                }
                for path in desired {
                    if path.is_dir()
                        && !watched.contains(&path)
                        && debouncer
                            .watcher()
                            .watch(&path, RecursiveMode::NonRecursive)
                            .is_ok()
                    {
                        watched.insert(path);
                    }
                }
            }
        });
    }

    pub fn plan_ctx(&self) -> PlanCtx {
        let cfg = self.web_config();
        PlanCtx {
            projects_dir: self.default_projects_dir(),
            web_server: cfg.default_server.clone(),
            http_port: cfg.http_port(),
            https_port: cfg.https_port(),
        }
    }

    // ------------------------------------------------------------------- projects

    pub fn project_detail(&self, id: &str) -> Option<ProjectDetail> {
        let project = self.projects.lock().unwrap().get(id)?;
        let settings = self.settings.lock().unwrap();
        let custom = self.custom_installs.lock().unwrap();
        Some(build_detail(&project, &self.runtimes, &settings, &custom))
    }

    pub fn project_by_path(&self, path: &str) -> Option<Project> {
        let norm = |p: &str| {
            p.replace('/', "\\")
                .trim_end_matches('\\')
                .to_ascii_lowercase()
        };
        self.projects
            .lock()
            .unwrap()
            .list()
            .into_iter()
            .find(|p| norm(&p.path) == norm(path))
    }

    /// The runtime's bin dir a project resolves to (custom pin, then managed), else the
    /// newest managed install.
    pub(crate) fn runtime_bin_for(&self, project_id: Option<&str>, id: &str) -> Option<PathBuf> {
        if let Some(pid) = project_id {
            if let Some(detail) = self.project_detail(pid) {
                if let Some(dir) = detail
                    .resolved
                    .iter()
                    .find(|r| r.id == id)
                    .and_then(|r| r.bin_dir.clone())
                {
                    return Some(PathBuf::from(dir));
                }
            }
        }
        let versions = self.runtimes.installed_versions(id);
        let newest = crate::php::pick_version(&versions, None)?;
        self.runtimes.bin_dir(id, &newest)
    }

    // -------------------------------------------------------------------- domains

    /// The URL a site is reachable at, on the server that actually renders it: a site
    /// pinned to a non-default server always carries that server's port.
    pub fn site_url(&self, d: &Domain, cfg: &WebConfig) -> String {
        let ports = cfg.effective_ports(&crate::domain::resolved_server(d, cfg));
        let (scheme, port, default) = if d.https {
            ("https", ports.https, 443)
        } else {
            ("http", ports.http, 80)
        };
        if port == default {
            format!("{scheme}://{}/", d.hostname)
        } else {
            format!("{scheme}://{}:{port}/", d.hostname)
        }
    }

    pub fn domain_summaries(&self) -> Vec<DomainSummary> {
        let cfg = self.web_config();
        let projects = self.projects.lock().unwrap().list();
        self.domains
            .lock()
            .unwrap()
            .list()
            .into_iter()
            .map(|d| DomainSummary {
                url: self.site_url(&d, &cfg),
                hostname: d.hostname.clone(),
                https: d.https,
                kind: match d.kind {
                    SiteKind::Php { .. } => "php",
                    SiteKind::Proxy { .. } => "proxy",
                    SiteKind::Static => "static",
                }
                .into(),
                enabled: d.enabled,
                project_id: d.project_id.clone(),
                has_app: d.app.is_some(),
                group: site_group(&d, &projects).into(),
                folder: site_folder(&d, &projects),
                public_domain: d.public_domain.clone(),
                server: crate::domain::resolved_server(&d, &cfg),
            })
            .collect()
    }

    /// Validates the structured blocks before they can reach a config file: these strings
    /// are pasted into Nginx/Apache/Caddy syntax, so nothing that could break out of a
    /// directive (newlines, quotes, braces, semicolons) is allowed through.
    pub fn validate_blocks(blocks: &SiteBlocks) -> Result<(), CoreError> {
        let bad = |s: &str| {
            s.chars()
                .any(|c| matches!(c, '\n' | '\r' | '"' | ';' | '{' | '}' | '\0'))
        };
        let name_ok = |s: &str| {
            !s.is_empty()
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        };
        let d = |m: String| CoreError::DomainError(m);
        for h in &blocks.headers {
            if !name_ok(&h.name) || bad(&h.value) {
                return Err(d(format!(
                    "header \"{}\" has characters that aren't allowed",
                    h.name
                )));
            }
        }
        for r in &blocks.redirects {
            if !r.from.starts_with('/')
                || bad(&r.from)
                || bad(&r.to)
                || r.from.contains(' ')
                || r.to.contains(' ')
                || !matches!(r.code, 301 | 302 | 303 | 307 | 308)
            {
                return Err(d(format!("redirect \"{}\" is invalid (paths must start with / and the code must be 301, 302, 303, 307 or 308)", r.from)));
            }
        }
        for m in &blocks.mappings {
            let up_ok = m.upstream.starts_with("http://") || m.upstream.starts_with("https://");
            if !m.path.starts_with('/')
                || bad(&m.path)
                || m.path.contains(' ')
                || bad(&m.upstream)
                || m.upstream.contains(' ')
                || !up_ok
            {
                return Err(d(format!("proxy mapping \"{}\" is invalid (the upstream must be an http:// or https:// URL)", m.path)));
            }
        }
        for u in &blocks.upstreams {
            if !name_ok(&u.name)
                || u.servers.is_empty()
                || u.servers.iter().any(|s| bad(s) || s.contains(' '))
            {
                return Err(d(format!("upstream \"{}\" is invalid", u.name)));
            }
        }
        for i in &blocks.includes {
            if bad(i) {
                return Err(d(
                    "an include path has characters that aren't allowed".to_string()
                ));
            }
        }
        Ok(())
    }

    pub fn add_domain(&self, mut domain: Domain) -> Result<Domain, CoreError> {
        Self::validate_blocks(&domain.blocks)?;
        if domain.https && !domain.redirect_https && domain.generated_hashes.is_empty() {
            // Sensible default for a brand-new HTTPS site (§52); the toggle stays user-controlled.
            domain.redirect_https = true;
        }
        if !Path::new(&domain.root).is_absolute() {
            return Err(CoreError::DomainError(
                "the document root must be a full folder path".into(),
            ));
        }
        let hostname = domain.hostname.clone();
        let added = self.domains.lock().unwrap().add(domain)?;
        self.edit_string_list(AUTO_SKIP, |list| list.retain(|h| h != &hostname))?;
        Ok(added)
    }

    pub fn update_domain(&self, domain: Domain) -> Result<Domain, CoreError> {
        Self::validate_blocks(&domain.blocks)?;
        self.domains.lock().unwrap().update(domain)
    }

    pub fn remove_domain(&self, hostname: &str) -> Result<(), CoreError> {
        let mut domains = self.domains.lock().unwrap();
        let was_auto = domains
            .get(hostname)
            .is_some_and(|d| d.project_id.is_some());
        domains.remove(hostname)?;
        if was_auto {
            // Deleted on purpose: automatic domains must not bring it back.
            self.edit_string_list(AUTO_SKIP, |list| list.push(hostname.to_string()))?;
        }
        let _ = self.certs.revoke(hostname);
        self.web.restart_app(hostname);
        Ok(())
    }

    // ------------------------------------------------------- automatic domains

    fn string_list(&self, key: &str) -> Vec<String> {
        let s = self.settings.lock().unwrap();
        let mut roots: Vec<String> = s
            .get(key)
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        if key == PROJECT_ROOTS {
            let default_root = self.paths.sites_dir().display().to_string();
            if !roots
                .iter()
                .any(|root| root.eq_ignore_ascii_case(&default_root))
            {
                roots.push(default_root);
            }
        }
        roots
    }

    fn edit_string_list(
        &self,
        key: &str,
        edit: impl FnOnce(&mut Vec<String>),
    ) -> Result<(), CoreError> {
        let mut list = self.string_list(key);
        let before = list.clone();
        edit(&mut list);
        list.dedup();
        if list != before {
            self.settings
                .lock()
                .unwrap()
                .set(key, serde_json::json!(list))?;
        }
        Ok(())
    }

    /// Remembers a folder of projects (like Laragon's `www`) so new folders in it are
    /// picked up automatically.
    pub fn remember_projects_root(&self, root: &str) -> Result<(), CoreError> {
        self.edit_string_list(PROJECT_ROOTS, |list| {
            if !list.iter().any(|r| r.eq_ignore_ascii_case(root)) {
                list.push(root.to_string());
            }
        })
    }

    /// Removes a project from the list for good: its folder joins a skip list so
    /// folder rescans (watcher, restart, Scan) don't re-register it. Explicitly
    /// adding the folder again clears the skip.
    pub fn remove_project(&self, id: &str) -> Result<(), CoreError> {
        let path = self
            .projects
            .lock()
            .unwrap()
            .get(id)
            .map(|p| p.path.clone());
        self.projects.lock().unwrap().remove(id)?;
        if let Some(path) = path {
            self.edit_string_list(PROJECT_SKIP, |list| {
                if !list.iter().any(|p| p.eq_ignore_ascii_case(&path)) {
                    list.push(path.clone());
                }
            })?;
        }
        Ok(())
    }

    /// True when the user removed this folder from the project list.
    pub(crate) fn is_project_skipped(&self, path: &str) -> bool {
        self.string_list(PROJECT_SKIP)
            .iter()
            .any(|p| p.eq_ignore_ascii_case(path))
    }

    /// Explicitly adding a folder again clears a previous removal.
    pub(crate) fn clear_project_skip(&self, path: &str) -> Result<(), CoreError> {
        self.edit_string_list(PROJECT_SKIP, |list| {
            list.retain(|p| !p.eq_ignore_ascii_case(path))
        })
    }

    /// Laragon-style automatic domains (setting `domains.auto`, on by default): every
    /// folder in a remembered projects folder becomes a project, and every project that
    /// can be served (PHP or plain HTML) gets `<folder>.<tld>` over HTTPS where `<tld>` is
    /// read from `domains.default_tld` (default: `local`). Domains the user deleted stay
    /// deleted. Applies the web config when something was added and the server is running.
    /// Returns how many domains were created.
    pub fn sync_auto_domains(&self) -> Result<usize, CoreError> {
        if !self.setting_bool("domains.auto", true) {
            return Ok(0);
        }
        // Folders scanned before roots were remembered: adopt any parent holding 2+ projects.
        if self.settings.lock().unwrap().get(PROJECT_ROOTS).is_none() {
            let mut parents: std::collections::HashMap<String, usize> = Default::default();
            for p in self.projects.lock().unwrap().list() {
                if let Some(parent) = Path::new(&p.path).parent() {
                    *parents.entry(parent.display().to_string()).or_default() += 1;
                }
            }
            let roots: Vec<String> = parents
                .into_iter()
                .filter(|(_, n)| *n >= 2)
                .map(|(p, _)| p)
                .collect();
            self.settings
                .lock()
                .unwrap()
                .set(PROJECT_ROOTS, serde_json::json!(roots))?;
        }
        for root in self.string_list(PROJECT_ROOTS) {
            let Ok(entries) = std::fs::read_dir(&root) else {
                continue;
            };
            let skipped = self.string_list(PROJECT_SKIP);
            let is_skipped = |dir: &std::path::Path| {
                let raw = dir.display().to_string();
                // Registered paths are stored canonicalized; compare both forms.
                let canonical = std::fs::canonicalize(dir)
                    .map(|c| {
                        let s = c.display().to_string();
                        s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
                    })
                    .unwrap_or_else(|_| raw.clone());
                skipped
                    .iter()
                    .any(|s| s.eq_ignore_ascii_case(&raw) || s.eq_ignore_ascii_case(&canonical))
            };
            let mut projects = self.projects.lock().unwrap();
            for e in entries.flatten() {
                let dir = e.path();
                let hidden = e.file_name().to_string_lossy().starts_with('.');
                if dir.is_dir()
                    && !hidden
                    && !is_skipped(&dir)
                    && (crate::detection::looks_like_a_project(&dir) || has_index(&dir))
                {
                    let _ = projects.register(&dir.display().to_string());
                }
            }
        }

        let skip = self.string_list(AUTO_SKIP);
        let default_tld = self
            .settings
            .lock()
            .unwrap()
            .get("domains.default_tld")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().trim_start_matches('.').to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "local".to_string());
        let auto_template = format!("{{project}}.{default_tld}");
        let projects = self.projects.lock().unwrap().list();
        let mut created = 0;
        for p in projects {
            let path = Path::new(&p.path);
            let (taken, linked) = {
                let domains = self.domains.lock().unwrap();
                let hostname = crate::domain::apply_template(&auto_template, &p.name);
                (
                    domains.get(&hostname).is_some() || skip.contains(&hostname),
                    domains
                        .list()
                        .iter()
                        .any(|d| d.project_id.as_deref() == Some(&p.id)),
                )
            };
            if taken || linked || !path.is_dir() {
                continue;
            }
            let Some((kind, root)) = auto_site(path) else {
                continue;
            };
            let domain = Domain {
                hostname: crate::domain::apply_template(&auto_template, &p.name),
                project_id: Some(p.id.clone()),
                root: root.display().to_string(),
                kind,
                https: true,
                redirect_https: true,
                wildcard: false,
                enabled: true,
                ownership: Default::default(),
                app: None,
                blocks: Default::default(),
                generated_hashes: Default::default(),
                public_domain: None,
                tunnel_id: None,
                server: None,
            };
            match self.add_domain(domain) {
                Ok(d) => {
                    tracing::info!(hostname = %d.hostname, project = %p.name, "created an automatic domain");
                    created += 1;
                }
                Err(e) => tracing::warn!(project = %p.name, error = %e, "automatic domain skipped"),
            }
        }
        if created > 0 && self.web.is_running() {
            self.apply_web(&[])?;
        }
        Ok(created)
    }

    /// Gives a site a different name (any valid hostname, any ending). Its certificate is
    /// reissued for the new name on the next apply, and its config files move with it.
    pub fn rename_domain(&self, hostname: &str, new_hostname: &str) -> Result<Domain, CoreError> {
        if hostname == crate::domain::HOME_HOSTNAME {
            return Err(CoreError::DomainError(format!(
                "{} is built in and can't be renamed",
                crate::domain::HOME_HOSTNAME
            )));
        }
        let new_hostname = new_hostname.trim().to_ascii_lowercase();
        if new_hostname == hostname {
            return self.domains.lock().unwrap().get(hostname).ok_or_else(|| {
                CoreError::DomainError(format!("{hostname} is not a known domain"))
            });
        }
        let mut domains = self.domains.lock().unwrap();
        let original = domains
            .get(hostname)
            .ok_or_else(|| CoreError::DomainError(format!("{hostname} is not a known domain")))?;
        let mut renamed = original.clone();
        renamed.hostname = new_hostname.clone();
        renamed.generated_hashes.clear();
        domains.remove(hostname)?;
        let renamed = match domains.add(renamed) {
            Ok(d) => d,
            Err(e) => {
                // Name taken or invalid: put the original back untouched.
                domains.add(original)?;
                return Err(e);
            }
        };
        drop(domains);
        self.web.rename_site_files(hostname, &renamed.hostname);
        let _ = self.certs.revoke(hostname);
        self.web.restart_app(hostname);
        self.edit_string_list(AUTO_SKIP, |list| list.retain(|h| h != &renamed.hostname))?;
        Ok(renamed)
    }

    pub fn duplicate_domain(
        &self,
        hostname: &str,
        new_hostname: &str,
    ) -> Result<Domain, CoreError> {
        let mut domains = self.domains.lock().unwrap();
        let mut copy = domains
            .get(hostname)
            .ok_or_else(|| CoreError::DomainError(format!("{hostname} is not a known domain")))?;
        copy.hostname = new_hostname.trim().to_ascii_lowercase();
        copy.generated_hashes.clear();
        // Two sites can't share one dev-server port or process: the copy needs its own.
        copy.app = None;
        domains.add(copy)
    }

    /// Applies every running web server. One report per server; a failure in one never
    /// rolls back or stops another.
    pub fn apply_web(&self, overwrite: &[String]) -> Result<Vec<ApplyReport>, CoreError> {
        let cfg = self.web_config();
        Self::require_distinct_ports(&cfg)?;
        let php_for = |d: &Domain| -> Option<String> {
            let detail = self.project_detail(d.project_id.as_deref()?)?;
            detail
                .resolved
                .into_iter()
                .find(|r| r.id == "php")
                .and_then(|r| r.installed_version)
        };
        let runtime_bin = |d: &Domain, rt: &str| self.runtime_bin_for(d.project_id.as_deref(), rt);
        let ctx = ApplyContext {
            cfg: &cfg,
            php_for: &php_for,
            runtime_bin: &runtime_bin,
            overwrite,
        };
        let mut domains = self.domains.lock().unwrap();
        self.web.apply(&ctx, &mut domains)
    }

    /// Applies a single web server, for "Start" on one row of the Services page.
    pub fn apply_web_server(
        &self,
        server_id: &str,
        overwrite: &[String],
    ) -> Result<ApplyReport, CoreError> {
        let cfg = self.web_config();
        Self::require_distinct_ports(&cfg)?;
        let php_for = |d: &Domain| -> Option<String> {
            let detail = self.project_detail(d.project_id.as_deref()?)?;
            detail
                .resolved
                .into_iter()
                .find(|r| r.id == "php")
                .and_then(|r| r.installed_version)
        };
        let runtime_bin = |d: &Domain, rt: &str| self.runtime_bin_for(d.project_id.as_deref(), rt);
        let ctx = ApplyContext {
            cfg: &cfg,
            php_for: &php_for,
            runtime_bin: &runtime_bin,
            overwrite,
        };
        let mut domains = self.domains.lock().unwrap();
        self.web.apply_one(&ctx, &mut domains, server_id)
    }

    /// Two servers pointed at one port can never both start, so the clash is refused
    /// up front with the fix, rather than surfacing as a bind error halfway through.
    fn require_distinct_ports(cfg: &WebConfig) -> Result<(), CoreError> {
        let clashes = cfg.port_conflicts();
        if clashes.is_empty() {
            return Ok(());
        }
        Err(CoreError::WebError(clashes.join("\n")))
    }

    pub fn set_domain_enabled(&self, hostname: &str, enabled: bool) -> Result<(), CoreError> {
        let mut domains = self.domains.lock().unwrap();
        let mut d = domains
            .get(hostname)
            .ok_or_else(|| CoreError::DomainError(format!("{hostname} is not a known domain")))?;
        d.enabled = enabled;
        domains.update(d)?;
        Ok(())
    }

    pub fn write_web_config(
        &self,
        hostname: &str,
        part: crate::web::manager::ConfigPart,
        content: &str,
    ) -> Result<String, CoreError> {
        let cfg = self.web_config();
        let mut domains = self.domains.lock().unwrap();
        self.web
            .write_config(&cfg, &mut domains, hostname, part, content)
    }

    pub fn set_ownership(&self, hostname: &str, ownership: Ownership) -> Result<(), CoreError> {
        let cfg = self.web_config();
        let mut domains = self.domains.lock().unwrap();
        self.web
            .set_ownership(&cfg, &mut domains, hostname, ownership)
    }

    pub fn restore_web_history(&self, hostname: &str, id: &str) -> Result<String, CoreError> {
        let cfg = self.web_config();
        let mut domains = self.domains.lock().unwrap();
        self.web.restore_history(&cfg, &mut domains, hostname, id)
    }

    /// §53 health chain for one site.
    pub fn health_check(&self, hostname: &str) -> Result<HealthReport, CoreError> {
        let cfg = self.web_config();
        let d =
            self.domains.lock().unwrap().get(hostname).ok_or_else(|| {
                CoreError::DomainError(format!("{hostname} is not a known domain"))
            })?;
        let cert = self.certs.info(hostname);
        let ca_info = self.certs.ca_info();
        let ca_path = PathBuf::from(&ca_info.cert_path);
        Ok(health::check_site(&HealthTarget {
            hostname,
            https: d.https,
            http_port: cfg.http_port(),
            https_port: cfg.https_port(),
            ca_pem: &ca_path,
            ca_trusted: ca_info.trusted,
            cert: cert.as_ref(),
        }))
    }

    // ------------------------------------------------------------------ databases

    pub fn open_database(
        &self,
        engine: &str,
        database: Option<&str>,
        path: Option<&str>,
        tool_id: Option<&str>,
    ) -> Result<(), CoreError> {
        let info = self
            .services
            .connection_info(engine, database, path)
            .map_err(svc)?;

        let detected = dbtools::detect_db_tools();
        let custom_path = |id: &str| {
            self.custom_installs
                .lock()
                .unwrap()
                .resolve(id, None)
                .map(|c| c.path.clone())
        };
        let find = |id: &str| {
            custom_path(id).or_else(|| {
                detected
                    .iter()
                    .find(|t| t.id == id)
                    .and_then(|t| t.found_path.clone())
            })
        };

        if let Some(id) = tool_id {
            if id == "heidisql" {
                let exe = find(id).ok_or_else(|| {
                    svc("HeidiSQL was not found. Install it or locate it from Services.")
                })?;
                let args = dbtools::heidisql_args_for_executable(&info, &exe).ok_or_else(|| svc("HeidiSQL can't open that database or its PostgreSQL libpq library was not found"))?;
                return dbtools::launch(&exe, &args, true).map_err(svc);
            }
            if id == "pgadmin" {
                let exe = find(id).ok_or_else(|| {
                    svc("pgAdmin 4 was not found. Install it or locate it from Services.")
                })?;
                return dbtools::launch(&exe, &[], false).map_err(svc);
            }
            if id == "nosqlbooster" {
                if engine != "mongodb" {
                    return Err(svc("NoSQLBooster only supports MongoDB"));
                }
                let exe = find(id).ok_or_else(|| {
                    svc("NoSQLBooster was not found. Install it or locate it from Services.")
                })?;
                return dbtools::launch(&exe, &[], false).map_err(svc);
            }
            if id == "tinyrdm" {
                if engine != "redis" {
                    return Err(svc("Tiny RDM only supports Redis"));
                }
                let exe = find(id).ok_or_else(|| {
                    svc("Tiny RDM was not found. Install it or register it under External tools.")
                })?;
                return dbtools::launch(&exe, &[], false).map_err(svc);
            }
            if id == "dbbrowser" {
                if engine != "sqlite" {
                    return Err(svc("DB Browser for SQLite only supports SQLite"));
                }
                let exe = find(id).ok_or_else(|| {
                    svc("DB Browser for SQLite was not found. Install it or locate it from Services.")
                })?;
                let args = dbtools::dbbrowser_args(&info)
                    .ok_or_else(|| svc("DB Browser for SQLite needs a database file path"))?;
                return dbtools::launch(&exe, &args, false).map_err(svc);
            }
        }

        // 1) an explicitly chosen registered tool, 2) the first registered for the engine,
        // 3) HeidiSQL (mariadb/sqlite) or pgAdmin if detected.
        let registered = {
            let store = self.ext_tools.lock().unwrap();
            match tool_id {
                Some(id) => store.get(id).cloned(),
                None => store.for_engine(engine).cloned(),
            }
        };
        if let Some(tool) = registered {
            let args = dbtools::expand_args(&tool.args, &info);
            return dbtools::launch(&tool.executable, &args, false).map_err(svc);
        }
        match engine {
            "mariadb" => {
                let exe = find("heidisql").ok_or_else(|| svc("HeidiSQL was not found. Install it, locate it, or register another tool for this engine."))?;
                let args = dbtools::heidisql_args_for_executable(&info, &exe)
                    .ok_or_else(|| svc("HeidiSQL can't open that database"))?;
                dbtools::launch(&exe, &args, true).map_err(svc)
            }
            "sqlite" => {
                if let Some(exe) = find("dbbrowser") {
                    if let Some(args) = dbtools::dbbrowser_args(&info) {
                        return dbtools::launch(&exe, &args, false).map_err(svc);
                    }
                }
                if let Some(exe) = find("heidisql") {
                    if let Some(args) = dbtools::heidisql_args_for_executable(&info, &exe) {
                        return dbtools::launch(&exe, &args, true).map_err(svc);
                    }
                }
                Err(svc("Neither DB Browser for SQLite nor HeidiSQL was found. Install one, locate it from Services, or register another tool for this engine."))
            }
            "postgres" => {
                if let Some(exe) = find("heidisql") {
                    if let Some(args) = dbtools::heidisql_args_for_executable(&info, &exe) {
                        return dbtools::launch(&exe, &args, true).map_err(svc);
                    }
                }
                let exe = find("pgadmin").ok_or_else(|| svc("Neither HeidiSQL nor pgAdmin was found. Install one, locate it, or register another tool for this engine."))?;
                dbtools::launch(&exe, &[], false).map_err(svc)
            }
            "mongodb" => {
                let exe = find("nosqlbooster").ok_or_else(|| {
                    svc("NoSQLBooster was not found. Install it or locate it from Services.")
                })?;
                dbtools::launch(&exe, &[], false).map_err(svc)
            }
            "redis" => {
                let exe = find("tinyrdm").ok_or_else(|| {
                    svc("Tiny RDM was not found. Install it or register another tool for this engine.")
                })?;
                dbtools::launch(&exe, &[], false).map_err(svc)
            }
            other => Err(svc(format!(
                "No tool is registered for {other}. Add one under External tools."
            ))),
        }
    }

    /// The `sqlite3` binary: a pinned custom install, the managed one, or PATH.
    pub fn sqlite3_path(&self) -> Result<PathBuf, CoreError> {
        if let Some(c) = self.custom_installs.lock().unwrap().resolve("sqlite", None) {
            return Ok(PathBuf::from(&c.path));
        }
        if let Some(v) = self
            .runtimes
            .installed_versions("sqlite")
            .into_iter()
            .next()
        {
            if let Some(p) = self.runtimes.binary_path("sqlite", &v) {
                return Ok(p);
            }
        }
        crate::web::manager::find_executable(None, "sqlite3")
            .ok_or_else(|| svc("SQLite is not installed. Install it from the Runtimes page."))
    }

    // --------------------------------------------------------------- OS integration

    pub fn open_path(&self, path: &str) -> Result<(), CoreError> {
        let p = Path::new(path);
        if !p.exists() {
            return Err(svc(format!("{path} does not exist")));
        }
        let mut cmd = std::process::Command::new("explorer.exe");
        cmd.arg(p);
        cmd.spawn().map(|_| ()).map_err(|e| svc(e.to_string()))
    }

    pub fn open_url(&self, url: &str) -> Result<(), CoreError> {
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(svc("only http:// and https:// links can be opened"));
        }
        let mut cmd = std::process::Command::new("rundll32.exe");
        cmd.args(["url.dll,FileProtocolHandler", url]);
        crate::exec::hide_window(&mut cmd);
        cmd.spawn().map(|_| ()).map_err(|e| svc(e.to_string()))
    }

    /// §94–99: the user's editor (setting `editor.command`), else VS Code if present, else Notepad.
    pub fn open_in_editor(&self, path: &str) -> Result<(), CoreError> {
        let (chosen, custom) = {
            let s = self.settings.lock().unwrap();
            let text = |k: &str| s.get(k).and_then(|v| v.as_str()).map(str::to_string);
            (text("editor"), text("editor.command"))
        };
        let exe: PathBuf = match crate::editors::resolve(chosen.as_deref(), custom.as_deref()) {
            Some(exe) => exe,
            // No code editor at all: a folder opens in Explorer, a file in Notepad.
            None if Path::new(path).is_dir() => PathBuf::from("explorer.exe"),
            None => PathBuf::from("notepad.exe"),
        };
        self.launch_editor(&exe, path)
    }

    /// Starts `exe` on `path`, detached. Editor shims (`code.cmd`) go through `cmd.exe`.
    pub(crate) fn launch_editor(&self, exe: &Path, path: &str) -> Result<(), CoreError> {
        let mut cmd = if crate::editors::is_shim(exe) {
            let mut c = std::process::Command::new("cmd.exe");
            c.arg("/C")
                .arg(exe)
                .args(crate::editors::open_args(exe, path));
            c
        } else {
            let mut c = std::process::Command::new(exe);
            c.args(crate::editors::open_args(exe, path));
            c
        };
        crate::exec::hide_window(&mut cmd);
        cmd.spawn()
            .map(|_| ())
            .map_err(|e| svc(format!("could not start {}: {e}", exe.display())))
    }

    // ---------------------------------------------------------- startup (§121)

    pub fn startup_settings(&self) -> StartupSettings {
        let s = self.settings.lock().unwrap();
        let b = |k: &str, d: bool| s.get(k).and_then(|v| v.as_bool()).unwrap_or(d);
        let services = s
            .get("startup.services")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        StartupSettings {
            with_windows: registry_run_value().is_some(),
            start_minimized: b("startup.minimized", true),
            autostart_web: b("startup.web", false),
            autostart_services: services,
            notifications: b("notifications.enabled", true),
            close_to_tray: b("startup.close_to_tray", true),
        }
    }

    pub fn set_startup_settings(&self, new: StartupSettings) -> Result<(), CoreError> {
        {
            let mut s = self.settings.lock().unwrap();
            s.set("startup.minimized", serde_json::json!(new.start_minimized))?;
            s.set("startup.web", serde_json::json!(new.autostart_web))?;
            s.set(
                "startup.services",
                serde_json::json!(new.autostart_services),
            )?;
            s.set(
                "notifications.enabled",
                serde_json::json!(new.notifications),
            )?;
            s.set(
                "startup.close_to_tray",
                serde_json::json!(new.close_to_tray),
            )?;
        }
        set_start_with_windows(new.with_windows).map_err(svc)
    }

    /// Runs what the user asked to have started with the app (§121). Failures are logged,
    /// never fatal — a service that won't start shouldn't stop the app opening.
    pub fn run_autostart(&self) {
        if let Err(e) = self.sync_auto_domains() {
            tracing::warn!(error = %e, "automatic domains could not be synced");
        }
        // Servers orphaned by a killed previous run would hold every port we need.
        let orphans = crate::process::kill_orphans(&self.paths.runtimes_dir());
        if orphans > 0 {
            tracing::info!(
                orphans,
                "stopped servers left running by a previous session"
            );
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        let startup = self.startup_settings();
        for id in &startup.autostart_services {
            if let Err(e) = self.services.start(id) {
                tracing::warn!(service = %id, error = %e, "autostart: service did not start");
            }
        }
        if startup.autostart_web {
            if let Err(e) = self.apply_web(&[]) {
                tracing::warn!(error = %e, "autostart: web server did not start");
            }
        }
        let tunnels: Vec<_> = self
            .list_tunnels()
            .into_iter()
            .filter(|t| {
                t.config.autostart
                    && t.config.acknowledged
                    && t.config.provider == "cloudflare"
                    && t.config.public_hostname.is_some()
            })
            .collect();
        if !tunnels.is_empty() && !startup.autostart_web {
            if let Err(e) = self.apply_web(&[]) {
                tracing::warn!(error = %e, "autostart: web server for public domains did not start");
            }
        }
        for t in tunnels {
            if let Err(e) = self.start_tunnel(&t.config.id, true) {
                tracing::warn!(tunnel = %t.config.name, error = %e, "autostart: named Cloudflare tunnel did not start");
            }
        }
    }

    // ----------------------------------------------------------- environment health

    /// §116: one list answering "is my setup healthy, and if not, what do I do?".
    pub fn environment_health(&self) -> Vec<HealthItem> {
        let cfg = self.web_config();
        let web = self.web.status(&cfg, &self.domains.lock().unwrap().list());
        let services = self.services.list();
        self.environment_health_with(&web, &services)
    }

    /// [`Self::environment_health`] over an already-computed web status and service list.
    /// The dashboard needs both for its own cards, so recomputing them here doubled the
    /// runtime-folder scans, PHP-pool reads and TCP probes on a page polled every 3s.
    pub fn environment_health_with(
        &self,
        web: &crate::web::manager::WebStatus,
        services: &[crate::service::ServiceStatus],
    ) -> Vec<HealthItem> {
        let cfg = self.web_config();
        let mut items = Vec::new();
        let item =
            |id: &str, label: &str, status: &str, detail: String, fix: Option<&str>| HealthItem {
                id: id.into(),
                label: label.into(),
                status: status.into(),
                detail,
                fix: fix.map(str::to_string),
            };

        let server_name = crate::web::server_by_id(cfg.server())
            .map(|s| s.name())
            .unwrap_or("Web server");
        let server_installed = !self.runtimes.installed_versions(cfg.server()).is_empty();
        items.push(if server_installed {
            item(
                "web_installed",
                server_name,
                "ok",
                format!("{server_name} is installed"),
                None,
            )
        } else {
            item(
                "web_installed",
                server_name,
                "error",
                format!("{server_name} is not installed"),
                Some("Install it from the Runtimes page."),
            )
        });

        let domain_count = self
            .domains
            .lock()
            .unwrap()
            .list()
            .iter()
            .filter(|d| d.enabled)
            .count();
        if web.running {
            items.push(item(
                "web_running",
                "Web server",
                "ok",
                format!(
                    "running on ports {} / {}",
                    cfg.http_port(),
                    cfg.https_port()
                ),
                None,
            ));
        } else if domain_count > 0 {
            items.push(item(
                "web_running",
                "Web server",
                "warn",
                "stopped, so your sites are offline".into(),
                Some("Apply the web config from the Domains page."),
            ));
        }
        for conflict in &web.port_conflicts {
            items.push(item(
                "port_conflict",
                "Ports",
                "warn",
                conflict.clone(),
                Some("Stop that program or choose other ports in the web settings."),
            ));
        }

        let ca = self.certs.ca_info();
        let any_https = self.domains.lock().unwrap().list().iter().any(|d| d.https);
        if any_https || ca.exists {
            items.push(if ca.trusted {
                item(
                    "ca_trusted",
                    "Local certificate authority",
                    "ok",
                    "trusted by Windows".into(),
                    None,
                )
            } else {
                item(
                    "ca_trusted",
                    "Local certificate authority",
                    "warn",
                    "not trusted, browsers will warn about HTTPS sites".into(),
                    Some("Trust it from the Certificates page."),
                )
            });
        }
        for cert in self.certs.list() {
            match cert.status {
                crate::certs::CertStatus::Expired => items.push(item(
                    "cert_expired",
                    &format!("Certificate {}", cert.hostname),
                    "error",
                    "expired".into(),
                    Some("Regenerate it from the Certificates page."),
                )),
                crate::certs::CertStatus::Expiring => items.push(item(
                    "cert_expiring",
                    &format!("Certificate {}", cert.hostname),
                    "warn",
                    format!("expires in {} days", cert.days_left),
                    Some("It renews on the next apply."),
                )),
                crate::certs::CertStatus::Valid => {}
            }
        }

        let enabled: Vec<String> = self
            .domains
            .lock()
            .unwrap()
            .list()
            .into_iter()
            .filter(|d| d.enabled)
            .map(|d| d.hostname)
            .collect();
        if !enabled.is_empty() {
            let existing = std::fs::read_to_string(crate::hosts::hosts_path()).unwrap_or_default();
            let missing: Vec<&String> = enabled
                .iter()
                .filter(|h| !self.web.dns_covers(h) && !crate::hosts::lists(&existing, h))
                .collect();
            items.push(if missing.is_empty() {
                item(
                    "hosts",
                    "Domain names",
                    "ok",
                    "every domain resolves to this computer".into(),
                    None,
                )
            } else {
                item(
                    "hosts",
                    "Domain names",
                    "warn",
                    format!(
                        "not resolving yet: {}",
                        missing
                            .iter()
                            .map(|s| s.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    Some("Apply the web config."),
                )
            });
        }

        if self.php.all_versions().is_empty() {
            items.push(item(
                "php",
                "PHP",
                "warn",
                "no PHP version is installed".into(),
                Some("Install one from the Runtimes page."),
            ));
        }
        for s in services.iter().filter(|s| s.installed) {
            items.push(match (s.running, s.healthy) {
                (true, Some(false)) => item(
                    &format!("svc_{}", s.id),
                    &s.name,
                    "warn",
                    "running but not answering on its port".into(),
                    Some("Restart the service."),
                ),
                (true, _) => item(
                    &format!("svc_{}", s.id),
                    &s.name,
                    "ok",
                    "running".into(),
                    None,
                ),
                (false, _) => item(
                    &format!("svc_{}", s.id),
                    &s.name,
                    "ok",
                    "stopped".into(),
                    None,
                ),
            });
        }
        items.extend(self.plugin_health());
        items
    }

    // ------------------------------------------------------------- monitoring

    /// Machine and process usage, plus what each enabled site costs. Shared processes (a
    /// PHP version's workers, the web server) are reported whole, with how many sites
    /// share them, rather than split by guesswork.
    pub fn system_stats(&self) -> crate::monitor::SystemStats {
        let procs = self.supervisor.snapshot();
        let pid_of = |id: ProcessId| procs.iter().find(|p| p.id == id).and_then(|p| p.pid);
        let mut stats = self
            .monitor
            .stats(&procs.iter().filter_map(|p| p.pid).collect::<Vec<_>>());
        let plan = self.web.usage_plan();
        let sum = |ids: &[ProcessId]| {
            ids.iter()
                .filter_map(|id| pid_of(*id))
                .filter_map(|pid| stats.processes.get(&pid))
                .fold((0.0f32, 0u64), |(c, m), s| {
                    (c + s.cpu_percent, m + s.memory)
                })
        };
        let enabled: Vec<Domain> = self
            .domains
            .lock()
            .unwrap()
            .list()
            .into_iter()
            .filter(|d| d.enabled)
            .collect();
        let server = sum(&plan.servers);
        let static_sites = enabled
            .iter()
            .filter(|d| {
                !plan.apps.contains_key(&d.hostname) && !plan.site_php.contains_key(&d.hostname)
            })
            .count();
        let mut sites = Vec::new();
        for d in &enabled {
            let (via, (cpu, memory), shared_by, measured) =
                if let Some(app) = plan.apps.get(&d.hostname) {
                    ("App process".to_string(), sum(&[*app]), 1, true)
                } else if let Some(version) = plan.site_php.get(&d.hostname) {
                    let workers = plan.pools.get(version).cloned().unwrap_or_default();
                    let sharing = plan.site_php.values().filter(|v| *v == version).count();
                    (
                        format!("PHP {version} workers"),
                        sum(&workers),
                        sharing,
                        true,
                    )
                } else if let SiteKind::Proxy {
                    upstream_host: Some(host),
                    upstream_port,
                    ..
                } = &d.kind
                {
                    (
                        format!("Forwarded to {host}:{upstream_port}"),
                        (0.0, 0),
                        1,
                        false,
                    )
                } else {
                    ("Web server".to_string(), server, static_sites.max(1), true)
                };
            sites.push(crate::monitor::SiteUsage {
                hostname: d.hostname.clone(),
                via,
                cpu_percent: cpu,
                memory,
                shared_by,
                measured,
                disk: None,
            });
        }
        let projects = self.projects.lock().unwrap().list();
        let folders: Vec<(String, PathBuf)> = enabled
            .iter()
            .filter(|d| {
                !matches!(
                    d.kind,
                    SiteKind::Proxy {
                        upstream_host: Some(_),
                        ..
                    }
                )
            })
            .map(|d| (d.hostname.clone(), PathBuf::from(site_folder(d, &projects))))
            .collect();
        let sizes = self
            .monitor
            .folder_sizes(&folders.iter().map(|(_, f)| f.clone()).collect::<Vec<_>>());
        for site in &mut sites {
            if let Some((_, folder)) = folders.iter().find(|(h, _)| *h == site.hostname) {
                site.disk = sizes.get(folder).copied();
            }
        }
        stats.sites = sites;

        let mut places = vec![(
            "OpenLocalServer data".to_string(),
            self.paths.root().to_path_buf(),
        )];
        places.extend(
            self.string_list(PROJECT_ROOTS)
                .into_iter()
                .map(|r| ("Projects".to_string(), PathBuf::from(r))),
        );
        places.push(("Projects".to_string(), self.default_projects_dir()));
        stats.disks = self.monitor.disks(&places);
        stats
    }

    // --------------------------------------------------------- database migration

    fn migration_source(&self, id: &str) -> Result<crate::migrate::MigrationSource, CoreError> {
        crate::migrate::detect()
            .into_iter()
            .find(|s| s.id == id)
            .ok_or_else(|| svc(format!("{id} is no longer there")))
    }

    /// Databases in an old Laragon/XAMPP/Wamp server (started on a copy if it isn't running).
    pub fn foreign_databases(
        &self,
        source_id: &str,
        password: &str,
    ) -> Result<Vec<String>, CoreError> {
        let source = self.migration_source(source_id)?;
        self.migration.begin("scan");
        let result = crate::migrate::Session::open(
            source,
            password,
            &self.paths.cache_dir(),
            &self.migration,
        )
        .and_then(|session| {
            self.migration.step("Reading the list of databases", None);
            session.databases()
        });
        self.migration.end();
        result.map_err(svc)
    }

    /// Copies databases from an old server (MySQL or MariaDB) into our MariaDB (`target`).
    /// An empty `databases` list means all of them. Each database reports on its own, so
    /// one failure doesn't stop the rest.
    pub fn migrate_databases(
        &self,
        source_id: &str,
        password: &str,
        databases: &[String],
        target: &str,
    ) -> Result<Vec<crate::migrate::MigratedDb>, CoreError> {
        self.migration.begin("import");
        let result = self.migrate_databases_inner(source_id, password, databases, target);
        self.migration.end();
        result
    }

    fn migrate_databases_inner(
        &self,
        source_id: &str,
        password: &str,
        databases: &[String],
        target: &str,
    ) -> Result<Vec<crate::migrate::MigratedDb>, CoreError> {
        let progress = &self.migration;
        let source = self.migration_source(source_id)?;
        let (client, port) = self.services.sql_client(target).map_err(svc)?;
        if source.running_port == Some(port) {
            return Err(svc(format!(
                "{} is running on port {port}, which our {target} needs. Stop it (in Laragon/XAMPP) and try again; its data is copied, not moved.",
                source.label
            )));
        }
        progress.step(format!("Starting our {target}"), None);
        if !self.services.is_running(target) {
            self.services.start(target).map_err(svc)?;
        }
        let started = std::time::Instant::now();
        while std::net::TcpStream::connect_timeout(
            &([127, 0, 0, 1], port).into(),
            std::time::Duration::from_millis(300),
        )
        .is_err()
        {
            if started.elapsed() > std::time::Duration::from_secs(60) {
                return Err(svc(format!("our {target} did not start")));
            }
            std::thread::sleep(std::time::Duration::from_millis(300));
        }

        let work = self.paths.cache_dir();
        std::fs::create_dir_all(&work)?;
        let session =
            crate::migrate::Session::open(source, password, &work, progress).map_err(svc)?;
        let names = if databases.is_empty() {
            session.databases().map_err(svc)?
        } else {
            databases.to_vec()
        };
        progress.step("Measuring the databases", None);
        let sizes = session.sizes();
        let mut results = Vec::new();
        let total = names.len();
        for (index, db) in names.into_iter().enumerate() {
            progress.database(index + 1, total, &db);
            let file = work.join(format!("migrate-{}.sql", crate::domain::slugify(&db)));
            progress.step(
                format!("Exporting {db}"),
                sizes.get(&db).copied().filter(|s| *s > 0),
            );
            let outcome = session.dump(&db, &file, progress).and_then(|()| {
                progress.step(
                    format!("Importing {db} into {target}"),
                    std::fs::metadata(&file).ok().map(|m| m.len()),
                );
                crate::migrate::import(&client, port, &file, progress)
            });
            let _ = std::fs::remove_file(&file);
            tracing::info!(database = %db, ok = outcome.is_ok(), "database migration");
            let result = match outcome {
                Ok(()) => crate::migrate::MigratedDb {
                    name: db,
                    ok: true,
                    detail: "copied".into(),
                },
                Err(e) => crate::migrate::MigratedDb {
                    name: db,
                    ok: false,
                    detail: e,
                },
            };
            progress.finished(result.clone());
            results.push(result);
        }
        progress.step("Cleaning up the temporary copy", None);
        drop(session);
        Ok(results)
    }

    // --------------------------------------------------------------------- logs

    pub fn log_sources(&self) -> Vec<LogSource> {
        let mut out = vec![LogSource {
            id: "app".into(),
            name: "OpenLocalServer".into(),
            kind: "app".into(),
        }];
        // Every web server writes its own logs, not just the configured default,
        // so all three are listed by id. The bare `web:error` / `web:access` ids
        // still read the default server, but the picker shows the per-server ones
        // so opening logs for a service row never shows another server's file.
        for id in crate::web::SERVER_IDS {
            let Some(s) = crate::web::server_by_id(id) else {
                continue;
            };
            out.push(LogSource {
                id: format!("web:{id}:error"),
                name: format!("{} error log", s.name()),
                kind: "web".into(),
            });
            out.push(LogSource {
                id: format!("web:{id}:access"),
                name: format!("{} access log", s.name()),
                kind: "web".into(),
            });
        }
        // A service's own stdout/stderr, so "View logs" on a service row lands on
        // that service instead of the app log. Only while it runs: the output
        // buffer dies with the process.
        for s in self.services.list().into_iter().filter(|s| s.kind != "web") {
            if let Some(source) = s.log_source {
                out.push(LogSource {
                    id: source,
                    name: format!("{} log", s.name),
                    kind: "service".into(),
                });
            }
        }
        for p in self.supervisor.snapshot() {
            out.push(LogSource {
                id: format!("process:{}", p.id.0),
                name: p.name.clone(),
                kind: "process".into(),
            });
        }
        for r in self.runs.list() {
            out.push(LogSource {
                id: format!("run:{}", r.id),
                name: format!("Quick App: {}", r.app_name),
                kind: "run".into(),
            });
        }
        out
    }

    pub fn read_log(&self, source: &str, max_lines: usize) -> Result<Vec<String>, CoreError> {
        let tail = |mut lines: Vec<String>| {
            if lines.len() > max_lines {
                lines.drain(..lines.len() - max_lines);
            }
            lines
        };
        let file_lines = |p: &Path| {
            std::fs::read_to_string(p)
                .map(|t| t.lines().map(str::to_string).collect::<Vec<_>>())
                .unwrap_or_default()
        };
        match source {
            "app" => {
                let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
                if let Ok(entries) = std::fs::read_dir(self.paths.logs_dir()) {
                    for e in entries.flatten() {
                        if let Ok(m) = e.metadata().and_then(|m| m.modified()) {
                            if newest.as_ref().is_none_or(|(t, _)| m > *t) {
                                newest = Some((m, e.path()));
                            }
                        }
                    }
                }
                Ok(tail(
                    newest.map(|(_, p)| file_lines(&p)).unwrap_or_default(),
                ))
            }
            "web:error" | "web:access" => {
                let cfg = self.web_config();
                let server = crate::web::server_by_id(cfg.server())
                    .ok_or_else(|| svc("unknown web server"))?;
                let logs = self.paths.web_dir().join(server.id()).join("logs");
                let file = if source == "web:error" {
                    "error.log"
                } else {
                    "access.log"
                };
                Ok(tail(file_lines(&logs.join(file))))
            }
            // Per-server form (`web:nginx:error`) so a service row for a server
            // that isn't the configured default still reads its own file.
            s if s.starts_with("web:") => {
                let (id, file) = match s[4..].rsplit_once(':') {
                    Some((id, "error")) => (id, "error.log"),
                    Some((id, "access")) => (id, "access.log"),
                    _ => return Err(svc(format!("unknown log source {s}"))),
                };
                crate::web::server_by_id(id)
                    .ok_or_else(|| svc(format!("unknown web server {id}")))?;
                Ok(tail(file_lines(
                    &self.paths.web_dir().join(id).join("logs").join(file),
                )))
            }
            // A service's own output. Empty while it is stopped: the buffer went
            // with the process, and an error here would look like a broken page.
            s if s.starts_with("service:") => {
                let id = &s[8..];
                Ok(self
                    .services
                    .process_id(id)
                    .map(|process_id| tail(self.supervisor.recent_output(process_id)))
                    .unwrap_or_default())
            }
            s if s.starts_with("process:") => {
                let id: u64 = s[8..].parse().map_err(|_| svc("bad process id"))?;
                Ok(tail(self.supervisor.recent_output(ProcessId(id))))
            }
            s if s.starts_with("run:") => Ok(tail(
                self.runs.get(&s[4..]).map(|r| r.log).unwrap_or_default(),
            )),
            other => Err(svc(format!("unknown log source {other}"))),
        }
    }

    /// Empties a log. Files are truncated in place rather than deleted: the server still
    /// holds them open and keeps appending.
    pub fn clear_log(&self, source: &str) -> Result<(), CoreError> {
        let truncate = |p: &Path| -> Result<(), CoreError> {
            if !p.exists() {
                return Ok(());
            }
            std::fs::OpenOptions::new()
                .write(true)
                .open(p)
                .and_then(|f| f.set_len(0))
                .map_err(|e| svc(format!("could not clear {}: {e}", p.display())))
        };
        match source {
            "app" => {
                if let Ok(entries) = std::fs::read_dir(self.paths.logs_dir()) {
                    for e in entries.flatten().filter(|e| e.path().is_file()) {
                        truncate(&e.path())?;
                    }
                }
                Ok(())
            }
            "web:error" | "web:access" => {
                let cfg = self.web_config();
                let server = crate::web::server_by_id(cfg.server())
                    .ok_or_else(|| svc("unknown web server"))?;
                let file = if source == "web:error" {
                    "error.log"
                } else {
                    "access.log"
                };
                truncate(
                    &self
                        .paths
                        .web_dir()
                        .join(server.id())
                        .join("logs")
                        .join(file),
                )
            }
            s if s.starts_with("web:") => {
                let (id, file) = match s[4..].rsplit_once(':') {
                    Some((id, "error")) => (id, "error.log"),
                    Some((id, "access")) => (id, "access.log"),
                    _ => return Err(svc(format!("unknown log source {s}"))),
                };
                crate::web::server_by_id(id)
                    .ok_or_else(|| svc(format!("unknown web server {id}")))?;
                truncate(&self.paths.web_dir().join(id).join("logs").join(file))
            }
            s if s.starts_with("service:") => {
                // Nothing to clear while the service is stopped: the buffer that
                // held its output went away with the process.
                if let Some(process_id) = self.services.process_id(&s[8..]) {
                    self.supervisor.clear_output(process_id);
                }
                Ok(())
            }
            s if s.starts_with("process:") => {
                let id: u64 = s[8..].parse().map_err(|_| svc("bad process id"))?;
                self.supervisor.clear_output(ProcessId(id));
                Ok(())
            }
            s if s.starts_with("run:") => {
                self.runs.clear_log(&s[4..]);
                Ok(())
            }
            other => Err(svc(format!("unknown log source {other}"))),
        }
    }

    // ------------------------------------------------------------ quick apps (§79–93)

    pub fn requirement_views(&self, plan: &RunPlan) -> Vec<RequirementView> {
        let catalog = crate::catalog::builtin_catalog();
        plan.requirements
            .iter()
            .map(|r| {
                let label_of = |id: &str| {
                    catalog
                        .iter()
                        .find(|m| m.id == id)
                        .map(|m| m.name.to_string())
                        .unwrap_or_else(|| id.to_string())
                };
                let label = match r.id.as_str() {
                    "python" => "Python".to_string(),
                    id => label_of(id),
                };
                if r.id == "python" {
                    return match crate::runtime::detect_system_install("python") {
                        Some(sys) => RequirementView {
                            id: r.id.clone(),
                            label,
                            wanted: r.wanted.clone(),
                            status: "installed".into(),
                            detail: Some(sys.version),
                        },
                        None => RequirementView {
                            id: r.id.clone(),
                            label,
                            wanted: r.wanted.clone(),
                            status: "unavailable".into(),
                            detail: Some(
                                "Python was not found on PATH. Install it from python.org.".into(),
                            ),
                        },
                    };
                }
                let installed = self.runtimes.installed_versions(&r.id);
                if let Some(v) = crate::php::pick_version(&installed, r.wanted.as_deref()) {
                    return RequirementView {
                        id: r.id.clone(),
                        label,
                        wanted: r.wanted.clone(),
                        status: "installed".into(),
                        detail: Some(v),
                    };
                }
                let available: Vec<String> = catalog
                    .iter()
                    .filter(|m| m.id == r.id)
                    .map(|m| m.version.to_string())
                    .collect();
                match crate::php::pick_version(&available, r.wanted.as_deref()) {
                    Some(v) => RequirementView {
                        id: r.id.clone(),
                        label,
                        wanted: r.wanted.clone(),
                        status: "installable".into(),
                        detail: Some(format!("will download {v}")),
                    },
                    None => RequirementView {
                        id: r.id.clone(),
                        label,
                        wanted: r.wanted.clone(),
                        status: "unavailable".into(),
                        detail: Some(match &r.wanted {
                            Some(w) => format!("version {w} isn't available for Windows"),
                            None => "not available for Windows".into(),
                        }),
                    },
                }
            })
            .collect()
    }

    /// Installs a catalog runtime and waits for it (Quick App runs are sequential).
    fn install_blocking(
        &self,
        id: &str,
        version: &str,
        log: &mut dyn FnMut(&str),
    ) -> Result<(), String> {
        let mut events = self.runtimes.subscribe();
        self.runtimes.install(id, version);
        let started = Instant::now();
        let mut last_pct = 0u64;
        loop {
            if started.elapsed() > Duration::from_secs(45 * 60) {
                return Err(format!("installing {id} {version} timed out"));
            }
            match events.blocking_recv() {
                Ok(RuntimeEvent::Installed {
                    id: i, version: v, ..
                }) if i == id && v == version => return Ok(()),
                Ok(RuntimeEvent::Failed {
                    id: i,
                    version: v,
                    message,
                }) if i == id && v == version => {
                    return Err(format!("installing {id} {version} failed: {message}"));
                }
                // A stop is a deliberate outcome, not a failure: the caller is told what
                // happened instead of being left waiting for an install that will never
                // finish.
                Ok(RuntimeEvent::Cancelled { id: i, version: v }) if i == id && v == version => {
                    return Err(format!(
                        "installing {id} {version} was stopped before it finished"
                    ));
                }
                Ok(RuntimeEvent::Progress {
                    id: i,
                    version: v,
                    state,
                    downloaded,
                    total,
                }) if i == id && v == version => {
                    if let Some(t) = total.filter(|t| *t > 0) {
                        let pct = downloaded * 100 / t;
                        if pct >= last_pct + 10 {
                            last_pct = pct;
                            log(&format!("{id} {version}: {state:?} {pct}%"));
                        }
                    }
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    return Err("the install channel closed".into())
                }
            }
        }
    }

    /// Starts a service (when it isn't running) and waits until its port answers.
    pub(crate) fn start_service_and_wait(
        &self,
        id: &str,
        log: &mut dyn FnMut(&str),
    ) -> Result<(), String> {
        if !self.services.is_running(id) {
            log(&format!("Starting {id}"));
            self.services.start(id)?;
        }
        let Some(port) = self.services.status(id).port else {
            return Ok(());
        };
        // MariaDB initialises its data directory on first start — give them time.
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(120) {
            if !self.services.is_running(id) {
                return Err(format!(
                    "{id} stopped right after starting. See its output on the Processes page."
                ));
            }
            if std::net::TcpStream::connect_timeout(
                &([127, 0, 0, 1], port).into(),
                Duration::from_millis(300),
            )
            .is_ok()
            {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(400));
        }
        Err(format!("{id} did not open port {port} in time"))
    }

    pub(crate) fn install_runtime_blocking(
        &self,
        id: &str,
        version: &str,
        log: &mut dyn FnMut(&str),
    ) -> Result<(), String> {
        self.install_blocking(id, version, log)
    }

    fn quick_ensure_runtime(
        &self,
        id: &str,
        wanted: Option<&str>,
        log: &mut dyn FnMut(&str),
    ) -> Result<String, String> {
        if id == "python" {
            return crate::runtime::detect_system_install("python")
                .map(|s| s.version)
                .ok_or_else(|| {
                    "Python was not found on PATH. Install it from python.org, then run this again."
                        .to_string()
                });
        }
        let catalog = crate::catalog::builtin_catalog();
        let name = catalog
            .iter()
            .find(|m| m.id == id)
            .map(|m| m.name)
            .ok_or_else(|| format!("{id} isn't available for this platform"))?;

        let installed = self.runtimes.installed_versions(id);
        if let Some(v) = crate::php::pick_version(&installed, wanted) {
            return Ok(format!("{name} {v}"));
        }
        let available: Vec<String> = catalog
            .iter()
            .filter(|m| m.id == id)
            .map(|m| m.version.to_string())
            .collect();
        let version = crate::php::pick_version(&available, wanted).ok_or_else(|| match wanted {
            Some(w) => format!(
                "{name} {w} isn't available. Available: {}",
                available.join(", ")
            ),
            None => format!("{name} isn't available"),
        })?;
        log(&format!("Installing {name} {version}"));
        self.install_blocking(id, &version, log)?;
        Ok(format!("{name} {version}"))
    }

    pub(crate) fn quick_resolve_program(
        &self,
        program: &str,
        values: &BTreeMap<String, String>,
        project_id: Option<&str>,
    ) -> Result<ResolvedProgram, String> {
        let is_path =
            program.contains('/') || program.contains('\\') || Path::new(program).is_absolute();
        if is_path {
            return Ok(ResolvedProgram {
                executable: PathBuf::from(program),
                ..Default::default()
            });
        }
        let pick_php = || -> Result<(PathBuf, PathBuf), String> {
            // A project's own resolution (custom pin or manifest) wins, then the wizard's choice.
            if let Some(dir) = project_id
                .and_then(|p| self.project_detail(p))
                .and_then(|d| {
                    d.resolved
                        .into_iter()
                        .find(|r| r.id == "php")
                        .and_then(|r| r.bin_dir)
                })
            {
                let dir = PathBuf::from(dir);
                let exe = dir.join("php.exe");
                if exe.is_file() {
                    return Ok((exe, dir));
                }
            }
            let wanted = values
                .get("php_version")
                .filter(|v| !v.is_empty())
                .map(String::as_str);
            let v = self
                .php
                .pick_version(wanted)
                .ok_or("PHP is not installed. Install it from the Runtimes page.")?;
            let dir = self.runtimes.install_dir("php", &v);
            Ok((dir.join("php.exe"), dir))
        };
        let php_env = |dir: &Path| -> Vec<(String, String)> {
            let mut env = Vec::new();
            // The generated php.ini (extensions on) is only for managed installs.
            if let Some(version) = dir.file_name().and_then(|n| n.to_str()) {
                if dir.starts_with(self.paths.runtimes_dir()) {
                    if let Ok(ini) = self.php.write_ini(version) {
                        env.push(("PHPRC".to_string(), ini.display().to_string()));
                    }
                }
            }
            env
        };

        // A project's own virtual environment "activates" for its commands: python and pip
        // run from its Scripts folder with VIRTUAL_ENV set (§17).
        if matches!(program, "python" | "python3" | "pip" | "pip3") {
            if let Some((venv_dir, scripts)) = self.project_venv(project_id) {
                let name = if program.starts_with("pip") {
                    "pip"
                } else {
                    "python"
                };
                let exe = scripts.join(format!("{name}.exe"));
                if exe.is_file() {
                    return Ok(ResolvedProgram {
                        executable: exe,
                        pre_args: vec![],
                        env: vec![("VIRTUAL_ENV".into(), venv_dir.display().to_string())],
                        path_dirs: vec![scripts],
                    });
                }
            }
        }

        match program {
            "php" => {
                let (exe, dir) = pick_php()?;
                Ok(ResolvedProgram {
                    env: php_env(&dir),
                    executable: exe,
                    pre_args: vec![],
                    path_dirs: vec![dir],
                })
            }
            "composer" => {
                let (exe, dir) = pick_php()?;
                let v = self
                    .runtimes
                    .installed_versions("composer")
                    .into_iter()
                    .next()
                    .ok_or("Composer is not installed. Install it from the Runtimes page.")?;
                let phar = self
                    .runtimes
                    .binary_path("composer", &v)
                    .ok_or("composer.phar is missing")?;
                let home = self.paths.services_dir().join("composer");
                let _ = std::fs::create_dir_all(&home);
                let mut env = php_env(&dir);
                env.push(("COMPOSER_HOME".into(), home.display().to_string()));
                env.push(("COMPOSER_NO_INTERACTION".into(), "1".into()));
                Ok(ResolvedProgram {
                    executable: exe,
                    pre_args: vec![phar.display().to_string()],
                    env,
                    path_dirs: vec![dir],
                })
            }
            "node" | "npm" | "npx" | "corepack" | "pnpm" | "pnpx" | "yarn" => {
                let dir = project_id
                    .and_then(|p| self.project_detail(p))
                    .and_then(|d| {
                        d.resolved
                            .into_iter()
                            .find(|r| r.id == "node")
                            .and_then(|r| r.bin_dir)
                    })
                    .map(PathBuf::from)
                    .or_else(|| {
                        let wanted = values
                            .get("node_version")
                            .filter(|v| !v.is_empty())
                            .map(String::as_str);
                        let installed = self.runtimes.installed_versions("node");
                        crate::php::pick_version(&installed, wanted)
                            .and_then(|v| self.runtimes.bin_dir("node", &v))
                    })
                    .ok_or("Node.js is not installed. Install it from the Runtimes page.")?;
                let file = crate::web::manager::find_executable(Some(&dir), program).ok_or_else(|| {
                    if matches!(program, "pnpm" | "pnpx" | "yarn") {
                        format!("{program} is not switched on for this Node.js. Enable it from the project's Node tools (corepack).")
                    } else {
                        format!("{program} was not found in {}", dir.display())
                    }
                })?;
                Ok(shim(file, vec![dir]))
            }
            other => {
                let file = crate::web::manager::find_executable(None, other).ok_or_else(|| {
                    format!("\"{other}\" was not found. Install it or add it to PATH.")
                })?;
                Ok(shim(file, vec![]))
            }
        }
    }

    fn quick_action(
        &self,
        action: &str,
        with: &BTreeMap<String, String>,
        ctx: &RunCtx,
        log: &mut dyn FnMut(&str),
    ) -> Result<ActionOutcome, String> {
        let get = |k: &str| {
            with.get(k)
                .map(String::as_str)
                .ok_or_else(|| format!("the \"{action}\" action needs \"{k}\""))
        };
        match action {
            "make_dir" => {
                let path = get("path")?;
                std::fs::create_dir_all(path)
                    .map_err(|e| format!("could not create {path}: {e}"))?;
                Ok(ActionOutcome::default())
            }
            "replace_in_file" => {
                let root = ctx.project_path.as_ref().ok_or("no project folder")?;
                let file = plan::safe_join(&root.display().to_string(), get("file")?)?;
                let text = std::fs::read_to_string(&file)
                    .map_err(|e| format!("could not read {file}: {e}"))?;
                let find = get("find")?;
                if !text.contains(find) {
                    return Err(format!("could not find \"{find}\" in {file}"));
                }
                std::fs::write(&file, text.replacen(find, get("replace")?, 1))
                    .map_err(|e| e.to_string())?;
                Ok(ActionOutcome::default())
            }
            "start_service" => {
                self.start_service_and_wait(get("id")?, log)?;
                Ok(ActionOutcome::default())
            }
            "create_database" => {
                let (engine, name) = (get("engine")?, get("name")?);
                let started = Instant::now();
                loop {
                    match self.services.create_database(engine, name) {
                        Ok(()) => {
                            return Ok(ActionOutcome {
                                detail: Some(format!("database {name}")),
                                ..Default::default()
                            })
                        }
                        // The server may still be finishing startup right after its port opened.
                        Err(e)
                            if started.elapsed() < Duration::from_secs(30)
                                && e.to_lowercase().contains("connect") =>
                        {
                            std::thread::sleep(Duration::from_millis(700));
                        }
                        Err(e) => return Err(e),
                    }
                }
            }
            "register_project" => {
                let path = get("path")?;
                let project = self
                    .projects
                    .lock()
                    .unwrap()
                    .register(path)
                    .map_err(|e| e.to_string())?;
                Ok(ActionOutcome {
                    project_id: Some(project.id),
                    ..Default::default()
                })
            }
            "create_domain" => {
                let hostname = get("hostname")?.to_string();
                let kind = match get("kind")? {
                    "php" => SiteKind::Php {
                        version: with.get("php_version").cloned(),
                    },
                    "static" => SiteKind::Static,
                    "proxy" => SiteKind::Proxy {
                        upstream_port: with
                            .get("port")
                            .and_then(|p| p.parse().ok())
                            .ok_or("a proxy site needs a valid port")?,
                        upstream_host: with
                            .get("upstream_host")
                            .map(|h| h.trim().to_string())
                            .filter(|h| !h.is_empty() && h != "127.0.0.1" && h != "localhost"),
                        upstream_https: with.get("upstream_https").is_some_and(|v| v == "true"),
                    },
                    other => return Err(format!("unknown site kind {other}")),
                };
                let app = with.get("app_program").map(|program| AppSpec {
                    executable: program.clone(),
                    args: with
                        .get("app_args")
                        .and_then(|a| serde_json::from_str(a).ok())
                        .unwrap_or_default(),
                    cwd: with.get("app_cwd").cloned().unwrap_or_default(),
                    runtime: with.get("app_runtime").cloned(),
                });
                let root = get("root")?.replace('/', "\\");
                let project_id = ctx
                    .project_path
                    .as_ref()
                    .and_then(|p| self.project_by_path(&p.display().to_string()))
                    .map(|p| p.id);
                let https = with.get("https").is_some_and(|v| v == "true");
                let domain = Domain {
                    hostname: hostname.clone(),
                    project_id,
                    root,
                    kind,
                    https,
                    redirect_https: https,
                    wildcard: with.get("wildcard").is_some_and(|v| v == "true"),
                    enabled: true,
                    ownership: Ownership::Managed,
                    app,
                    blocks: SiteBlocks::default(),
                    generated_hashes: BTreeMap::new(),
                    public_domain: None,
                    tunnel_id: None,
                    server: None,
                };
                let cfg = self.web_config();
                let url = self.site_url(&domain, &cfg);
                let mut domains = self.domains.lock().unwrap();
                match domains.get(&hostname) {
                    // Re-running a recipe reuses the domain instead of failing on the duplicate.
                    Some(existing) => {
                        let mut updated = domain;
                        updated.generated_hashes = existing.generated_hashes;
                        domains.update(updated).map_err(|e| e.to_string())?;
                    }
                    None => {
                        domains.add(domain).map_err(|e| e.to_string())?;
                    }
                }
                Ok(ActionOutcome {
                    detail: Some(url.clone()),
                    open_url: Some(url),
                    ..Default::default()
                })
            }
            "trust_ca" => {
                self.certs.ca().ensure_created()?;
                if self.certs.ca().is_trusted() {
                    return Ok(ActionOutcome {
                        detail: Some("already trusted".into()),
                        ..Default::default()
                    });
                }
                self.certs
                    .ca()
                    .trust_current_user()
                    .map_err(|e| format!("Windows did not trust the certificate authority: {e}"))?;
                Ok(ActionOutcome::default())
            }
            "apply_web" => {
                let reports = self.apply_web(&[]).map_err(|e| e.to_string())?;
                for r in &reports {
                    for w in &r.warnings {
                        log(&format!("warning: {w}"));
                    }
                }
                Ok(ActionOutcome::default())
            }
            "health_check" => {
                let report = self
                    .health_check(get("hostname")?)
                    .map_err(|e| e.to_string())?;
                for s in &report.steps {
                    log(&format!(
                        "{} {}: {}",
                        if s.ok { "✓" } else { "✗" },
                        s.name,
                        s.detail
                    ));
                }
                if report.ok {
                    Ok(ActionOutcome::default())
                } else {
                    let failed: Vec<String> = report
                        .steps
                        .iter()
                        .filter(|s| !s.ok)
                        .map(|s| format!("{}: {}", s.name, s.detail))
                        .collect();
                    Err(failed.join("; "))
                }
            }
            "download_extract" => {
                self.download_extract(
                    get("url")?,
                    with.get("sha1_url").map(String::as_str),
                    with.get("sha256").map(String::as_str),
                    get("dest")?,
                    with.get("strip").map(String::as_str),
                    log,
                )?;
                Ok(ActionOutcome::default())
            }
            other => Err(format!("unknown action \"{other}\"")),
        }
    }

    /// Downloads a zip over HTTPS, verifies its checksum, and extracts it (stripping a
    /// leading folder). Used by recipes like WordPress that ship as one archive.
    fn download_extract(
        &self,
        url: &str,
        sha1_url: Option<&str>,
        sha256: Option<&str>,
        dest: &str,
        strip: Option<&str>,
        log: &mut dyn FnMut(&str),
    ) -> Result<(), String> {
        use sha1::Digest as _;
        if !url.starts_with("https://") {
            return Err("downloads must use HTTPS".into());
        }
        if sha1_url.is_none() && sha256.is_none() {
            return Err("a download needs a checksum (sha1_url or sha256)".into());
        }
        log(&format!("Downloading {url}"));
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        let client = reqwest::Client::new();
        let (bytes, expected) = rt.block_on(async {
            let bytes = client
                .get(url)
                .send()
                .await
                .and_then(|r| r.error_for_status())
                .map_err(|e| e.to_string())?
                .bytes()
                .await
                .map_err(|e| e.to_string())?;
            let expected = match (sha1_url, sha256) {
                (Some(u), _) => {
                    let text = client
                        .get(u)
                        .send()
                        .await
                        .and_then(|r| r.error_for_status())
                        .map_err(|e| e.to_string())?
                        .text()
                        .await
                        .map_err(|e| e.to_string())?;
                    (
                        "sha1",
                        text.split_whitespace().next().unwrap_or("").to_lowercase(),
                    )
                }
                (None, Some(h)) => ("sha256", h.to_lowercase()),
                _ => unreachable!(),
            };
            Ok::<_, String>((bytes, expected))
        })?;
        let actual = if expected.0 == "sha1" {
            format!("{:x}", sha1::Sha1::digest(&bytes))
        } else {
            format!("{:x}", sha2::Sha256::digest(&bytes))
        };
        if actual != expected.1 {
            return Err(format!(
                "checksum mismatch: expected {}, got {actual}. Nothing was extracted.",
                expected.1
            ));
        }
        log("Checksum verified");
        let cache = self
            .paths
            .cache_dir()
            .join(format!("download-{}.zip", &actual[..16]));
        std::fs::create_dir_all(self.paths.cache_dir()).map_err(|e| e.to_string())?;
        std::fs::write(&cache, &bytes).map_err(|e| e.to_string())?;
        let result = crate::runtime::extract_zip(&cache, Path::new(dest), strip.unwrap_or(""))
            .map_err(|e| e.to_string());
        let _ = std::fs::remove_file(&cache);
        result
    }

    // --------------------------------------------------------------- quick commands

    /// Values available to a Quick Command's `{{placeholders}}`.
    fn command_values(&self, project_id: Option<&str>) -> BTreeMap<String, String> {
        let mut values = BTreeMap::new();
        if let Some(p) = project_id.and_then(|id| self.projects.lock().unwrap().get(id)) {
            values.insert("project_path".into(), p.path.clone());
            values.insert("project_name".into(), p.name.clone());
            if let Some(d) = self
                .domains
                .lock()
                .unwrap()
                .list()
                .into_iter()
                .find(|d| d.project_id.as_deref() == Some(p.id.as_str()))
            {
                values.insert("domain".into(), d.hostname);
            }
        }
        values
    }

    pub fn run_command_line(
        &self,
        line: &str,
        cwd: Option<&str>,
        project_id: Option<&str>,
        name: Option<&str>,
    ) -> Result<ProcessId, CoreError> {
        let tokens = plan::split_command_line(line);
        let (program, args) = tokens
            .split_first()
            .ok_or_else(|| CoreError::QuickAppError("that command line is empty".into()))?;
        self.spawn_project_process(program, args, cwd, project_id, name.unwrap_or(line), line)
    }

    pub(crate) fn spawn_project_process(
        &self,
        program: &str,
        args: &[String],
        cwd: Option<&str>,
        project_id: Option<&str>,
        name: &str,
        history_line: &str,
    ) -> Result<ProcessId, CoreError> {
        let (executable, full_args, env) = self.project_program(program, args, project_id)?;
        let cwd_owned = cwd.map(str::to_string).or_else(|| {
            project_id
                .and_then(|id| self.projects.lock().unwrap().get(id))
                .map(|p| p.path)
        });
        let id = self.supervisor.start(ProcessSpec {
            name: name.to_string(),
            executable: executable.display().to_string(),
            args: full_args,
            cwd: cwd_owned.clone(),
            env,
            restart: None,
        });
        let _ = self
            .history
            .lock()
            .unwrap()
            .record(history_line, cwd_owned.as_deref(), project_id);
        Ok(id)
    }

    /// `program args` resolved the way a project's commands run: its own PHP / Node /
    /// Python, with their folders first on PATH. Returns the executable, all arguments and
    /// the extra environment.
    pub(crate) fn project_program(
        &self,
        program: &str,
        args: &[String],
        project_id: Option<&str>,
    ) -> Result<ProjectProgram, CoreError> {
        let mut values = BTreeMap::new();
        // Pin the resolved versions so `php`/`node` follow the project (§18–19).
        if let Some(detail) = project_id.and_then(|id| self.project_detail(id)) {
            for r in &detail.resolved {
                if let Some(v) = &r.installed_version {
                    values.insert(format!("{}_version", r.id), v.clone());
                }
            }
        }
        let resolved = self
            .quick_resolve_program(program, &values, project_id)
            .map_err(CoreError::QuickAppError)?;
        let mut full_args = resolved.pre_args.clone();
        full_args.extend(args.iter().cloned());
        let mut env = resolved.env.clone();
        // §129: Node's heap limit for every project command (Node ignores it otherwise).
        if let Some(mb) = self.resource_limits().node_max_old_space_mb {
            if !env.iter().any(|(k, _)| k == "NODE_OPTIONS") {
                env.push(("NODE_OPTIONS".into(), format!("--max-old-space-size={mb}")));
            }
        }
        if !resolved.path_dirs.is_empty() {
            let mut dirs: Vec<String> = resolved
                .path_dirs
                .iter()
                .map(|d| d.display().to_string())
                .collect();
            dirs.push(std::env::var("PATH").unwrap_or_default());
            env.push(("PATH".into(), dirs.join(";")));
        }
        Ok((resolved.executable, full_args, env))
    }

    pub fn run_quick_command(
        &self,
        id: &str,
        project_id: Option<&str>,
    ) -> Result<Option<ProcessId>, CoreError> {
        let cmd: QuickCommand = self
            .quick_commands
            .get(id)
            .ok_or_else(|| CoreError::QuickAppError(format!("no Quick Command \"{id}\"")))?;
        let values = self.command_values(project_id);
        let render = |t: &str| plan::render(t, &values, &[]).map_err(CoreError::QuickAppError);

        if let Some(action) = &cmd.action {
            match action.as_str() {
                "open_url" => {
                    let url = cmd
                        .with
                        .get("url")
                        .map(|s| s.as_string())
                        .unwrap_or_default();
                    self.open_url(&render(&url)?)?;
                }
                "open_site" => {
                    let pid = project_id
                        .ok_or_else(|| CoreError::QuickAppError("pick a project first".into()))?;
                    let cfg = self.web_config();
                    let domain = self
                        .domains
                        .lock()
                        .unwrap()
                        .list()
                        .into_iter()
                        .find(|d| d.project_id.as_deref() == Some(pid))
                        .ok_or_else(|| {
                            CoreError::QuickAppError("this project has no domain yet".into())
                        })?;
                    self.open_url(&self.site_url(&domain, &cfg))?;
                }
                "open_web_config" => {
                    let cfg = self.web_config();
                    let dir = self.paths.web_dir().join(cfg.server());
                    std::fs::create_dir_all(&dir)?;
                    self.open_path(&dir.display().to_string())?;
                }
                "restart_project" => {
                    let pid = project_id
                        .ok_or_else(|| CoreError::QuickAppError("pick a project first".into()))?;
                    let hosts: Vec<String> = self
                        .domains
                        .lock()
                        .unwrap()
                        .list()
                        .into_iter()
                        .filter(|d| d.project_id.as_deref() == Some(pid))
                        .map(|d| d.hostname)
                        .collect();
                    for h in hosts {
                        self.web.restart_app(&h);
                    }
                    self.apply_web(&[])?;
                }
                other => {
                    return Err(CoreError::QuickAppError(format!(
                        "unknown action \"{other}\""
                    )))
                }
            }
            return Ok(None);
        }

        let spec = cmd.command.as_ref().expect("validated");
        let cwd = cmd.working_directory.as_deref().map(render).transpose()?;
        let args = spec
            .arguments
            .iter()
            .map(|a| render(a))
            .collect::<Result<Vec<_>, _>>()?;
        let mut line_parts = vec![spec.executable.clone()];
        line_parts.extend(args.iter().map(|a| {
            if a.contains(' ') {
                format!("\"{a}\"")
            } else {
                a.clone()
            }
        }));
        let pid = self.spawn_project_process(
            &spec.executable,
            &args,
            cwd.as_deref(),
            project_id,
            &format!("Quick Command: {}", cmd.name),
            &line_parts.join(" "),
        )?;
        Ok(Some(pid))
    }

    pub fn plan_quick_app(
        &self,
        id: &str,
        values: &BTreeMap<String, String>,
    ) -> Result<QuickPlanResult, CoreError> {
        let detail = self.catalog.lock().unwrap().get(id)?;
        let ctx = self.plan_ctx();
        let (resolved, errors) = plan::resolve_lenient(&detail.app, values, &ctx);
        // Secrets never leave the core — the wizard only ever sees them masked.
        let masked = |vals: &BTreeMap<String, String>| -> BTreeMap<String, String> {
            vals.iter()
                .map(|(k, v)| {
                    let secret = detail
                        .app
                        .variables
                        .iter()
                        .any(|x| &x.name == k && x.is_secret());
                    (
                        k.clone(),
                        if secret && !v.is_empty() {
                            "••••••••".to_string()
                        } else {
                            v.clone()
                        },
                    )
                })
                .collect()
        };
        let display = masked(&resolved);
        if !errors.is_empty() {
            return Ok(QuickPlanResult {
                ok: false,
                errors,
                plan: None,
                requirements: vec![],
                trusted: detail.view.trusted,
                source: detail.view.source_label(),
                values: display,
            });
        }
        let plan =
            plan::build_plan(&detail.app, resolved, &ctx).map_err(CoreError::QuickAppError)?;
        let requirements = self.requirement_views(&plan);
        Ok(QuickPlanResult {
            ok: true,
            errors: vec![],
            plan: Some(plan),
            requirements,
            trusted: detail.view.trusted,
            source: detail.view.source_label(),
            values: display,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuickPlanResult {
    pub ok: bool,
    pub errors: Vec<plan::FieldError>,
    pub plan: Option<RunPlan>,
    pub requirements: Vec<RequirementView>,
    pub trusted: bool,
    pub source: String,
    /// The answers as resolved so far (defaults filled in, secrets masked).
    pub values: BTreeMap<String, String>,
}

impl crate::quickapp::EntryView {
    pub fn source_label(&self) -> String {
        match self.source {
            crate::quickapp::EntrySource::Builtin => "built-in".into(),
            crate::quickapp::EntrySource::Local => "your own".into(),
            crate::quickapp::EntrySource::Imported => {
                self.origin.clone().unwrap_or_else(|| "imported".into())
            }
        }
    }
}

/// `.cmd` / `.bat` shims (npm, npx) can't be launched directly — run them through cmd.exe.
/// Executable, full arguments and extra environment of a resolved project command.
pub(crate) type ProjectProgram = (PathBuf, Vec<String>, Vec<(String, String)>);

fn shim(file: PathBuf, path_dirs: Vec<PathBuf>) -> ResolvedProgram {
    let is_shim = file
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"));
    if is_shim {
        ResolvedProgram {
            executable: PathBuf::from("cmd.exe"),
            pre_args: vec!["/C".into(), file.display().to_string()],
            env: vec![],
            path_dirs,
        }
    } else {
        ResolvedProgram {
            executable: file,
            pre_args: vec![],
            env: vec![],
            path_dirs,
        }
    }
}

/// The Quick App runner's window onto the core.
pub struct Host {
    pub inner: Arc<Inner>,
    /// Set when the run belongs to a known project, so runtimes resolve the way the project does.
    pub project_id: Mutex<Option<String>>,
}

impl QuickHost for Host {
    fn ensure_runtime(
        &self,
        id: &str,
        version: Option<&str>,
        log: &mut dyn FnMut(&str),
    ) -> Result<String, String> {
        self.inner.quick_ensure_runtime(id, version, log)
    }
    fn resolve_program(
        &self,
        program: &str,
        values: &BTreeMap<String, String>,
    ) -> Result<ResolvedProgram, String> {
        let pid = self.project_id.lock().unwrap().clone();
        self.inner
            .quick_resolve_program(program, values, pid.as_deref())
    }
    fn action(
        &self,
        action: &str,
        with: &BTreeMap<String, String>,
        ctx: &RunCtx,
        log: &mut dyn FnMut(&str),
    ) -> Result<ActionOutcome, String> {
        let out = self.inner.quick_action(action, with, ctx, log)?;
        if let Some(pid) = &out.project_id {
            *self.project_id.lock().unwrap() = Some(pid.clone());
        }
        Ok(out)
    }
    fn run_elevated(&self, executable: &Path, args: &[String]) -> Result<(), String> {
        crate::elevate::run_elevated_command(executable, args)
    }
}

// ------------------------------------------------------------------ Windows startup

#[cfg(windows)]
fn registry_run_value() -> Option<String> {
    let mut cmd = std::process::Command::new("reg");
    cmd.args(["query", REG_RUN_KEY, "/v", REG_VALUE]);
    crate::exec::hide_window(&mut cmd);
    let out = cmd.output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).to_string())
}

#[cfg(windows)]
fn set_start_with_windows(enabled: bool) -> Result<(), String> {
    let mut cmd = std::process::Command::new("reg");
    if enabled {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        cmd.args([
            "add",
            REG_RUN_KEY,
            "/v",
            REG_VALUE,
            "/t",
            "REG_SZ",
            // No `--minimized`: the startup entry and a manual launch now behave
            // the same way, because `startup.minimized` decides on its own. Passing
            // the flag here would hide the window even with the preference off.
            "/d",
            &format!("\"{}\"", exe.display()),
            "/f",
        ]);
    } else {
        if registry_run_value().is_none() {
            return Ok(());
        }
        cmd.args(["delete", REG_RUN_KEY, "/v", REG_VALUE, "/f"]);
    }
    crate::exec::hide_window(&mut cmd);
    let out = cmd.output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

#[cfg(not(windows))]
fn registry_run_value() -> Option<String> {
    None
}

/// The CLI flag that forces a hidden start regardless of the preference. Rare, but
/// a script or a hand-made shortcut still needs a way to say it.
pub const FORCE_MINIMIZED_ARG: &str = "--minimized";

/// Whether this launch starts in the tray.
///
/// The preference is the whole answer: it applies to a manual launch exactly as it
/// does to the startup entry, so the toggle means what it says. The flag only adds
/// to that — it forces a hidden start when the preference is off, and changes
/// nothing when it is on. Anything else opens the window.
pub fn should_start_hidden(start_minimized: bool, args: impl IntoIterator<Item = String>) -> bool {
    start_minimized || args.into_iter().any(|a| a == FORCE_MINIMIZED_ARG)
}

/// Rewrites the startup entry when it is stale. An entry written by an earlier
/// build still carries `--minimized`, which would hide the window even with the
/// preference off — the opposite of what the user then chose. The entry itself is
/// left alone when it is missing, so a build without autostart is not changed
/// behind the user's back. Failures are logged, never fatal: a bad registry value
/// must not stop the app from starting.
pub fn migrate_startup_entry() {
    #[cfg(windows)]
    {
        let Some(current) = registry_run_value() else {
            return;
        };
        if !current.contains("--minimized") {
            return;
        }
        match set_start_with_windows(true) {
            Ok(()) => tracing::info!("rewrote the startup entry without the old --minimized flag"),
            Err(e) => tracing::warn!(error = %e, "could not rewrite the startup entry"),
        }
    }
}

#[cfg(not(windows))]
fn set_start_with_windows(_enabled: bool) -> Result<(), String> {
    Err("start with the system is only implemented on Windows so far".into())
}

/// The "type of website" a site is listed under: "php", "nodejs", "python", "static", or
/// "proxy" for anything else that is forwarded (Docker, another computer, unknown apps).
/// A dev-server site is told apart by its start command, else by the linked project.
fn site_group(d: &Domain, projects: &[crate::project::Project]) -> &'static str {
    use crate::detection::Framework;
    match &d.kind {
        SiteKind::Php { .. } => "php",
        SiteKind::Static => "static",
        SiteKind::Proxy { upstream_host, .. } => {
            if let Some(app) = &d.app {
                let exe = Path::new(&app.executable)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or(&app.executable)
                    .to_ascii_lowercase();
                let runtime = app.runtime.as_deref().unwrap_or_default();
                if runtime == "node"
                    || matches!(
                        exe.as_str(),
                        "node" | "npm" | "npx" | "pnpm" | "yarn" | "bun" | "deno"
                    )
                {
                    return "nodejs";
                }
                if runtime == "python"
                    || matches!(
                        exe.as_str(),
                        "python"
                            | "python3"
                            | "py"
                            | "uvicorn"
                            | "gunicorn"
                            | "flask"
                            | "django-admin"
                            | "hypercorn"
                            | "poetry"
                            | "pipenv"
                    )
                {
                    return "python";
                }
                if runtime == "php" || exe == "php" {
                    return "php";
                }
            }
            // No start command, and it is this computer (not Docker / another machine):
            // the linked project's framework says what is behind the port.
            if upstream_host.is_none() {
                let path = d
                    .project_id
                    .as_ref()
                    .and_then(|id| projects.iter().find(|p| &p.id == id))
                    .map(|p| p.path.clone());
                if let Some(path) = path {
                    return match crate::detection::detect(Path::new(&path)).framework {
                        Framework::Node => "nodejs",
                        Framework::Django
                        | Framework::Flask
                        | Framework::FastApi
                        | Framework::GenericPython => "python",
                        _ => "proxy",
                    };
                }
            }
            "proxy"
        }
    }
}

fn site_folder(d: &Domain, projects: &[crate::project::Project]) -> String {
    if let Some(p) = d
        .project_id
        .as_ref()
        .and_then(|id| projects.iter().find(|p| &p.id == id))
    {
        return p.path.clone();
    }
    let root = Path::new(&d.root);
    let docroot = root
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| matches!(n.to_lowercase().as_str(), "public" | "web" | "public_html"));
    match root.parent() {
        Some(parent) if docroot => parent.display().to_string(),
        _ => d.root.clone(),
    }
}

const PROJECT_ROOTS: &str = "projects.roots";
const AUTO_SKIP: &str = "domains.auto_skip";
/// Folders the user removed from the project list. Folder rescans must not
/// re-register them; only explicitly adding the folder brings one back.
const PROJECT_SKIP: &str = "projects.removed";

fn has_index(dir: &Path) -> bool {
    ["index.php", "index.html", "index.htm"]
        .iter()
        .any(|f| dir.join(f).is_file())
}

/// How an automatic domain serves a project, or `None` when it needs a dev server
/// (Node/Python) that a plain domain can't start.
fn auto_site(path: &Path) -> Option<(SiteKind, PathBuf)> {
    use crate::detection::Framework as F;
    let detection = crate::detection::detect(path);
    let root = detection
        .doc_root
        .as_ref()
        .map(|d| path.join(d))
        .unwrap_or_else(|| path.to_path_buf());
    match detection.framework {
        F::Laravel | F::Symfony | F::WordPress | F::GenericPhp => {
            Some((SiteKind::Php { version: None }, root))
        }
        _ if has_index(&root) => Some((SiteKind::Static, root)),
        _ => None,
    }
}

#[cfg(test)]
mod project_skip_tests {
    use super::*;

    fn inner() -> (Arc<Inner>, crate::test_support::IsolatedHome) {
        let home = crate::test_support::isolated_home();
        let settings = crate::settings::SettingsService::load(&home.paths).unwrap();
        (Inner::new(settings, home.paths.clone()).unwrap(), home)
    }

    fn project_dir(root: &std::path::Path, name: &str) -> std::path::PathBuf {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.html"), "hi").unwrap();
        dir
    }

    #[test]
    fn removed_projects_stay_removed_across_rescans() {
        let (inner, home) = inner();
        let root = home.paths.data_dir().join("www");
        let app1 = project_dir(&root, "app1");
        project_dir(&root, "app2");
        inner
            .remember_projects_root(&root.display().to_string())
            .unwrap();

        inner.sync_auto_domains().unwrap();
        assert_eq!(inner.projects.lock().unwrap().list().len(), 2);

        let id1 = inner.projects.lock().unwrap().list()[0].id.clone();
        inner.remove_project(&id1).unwrap();
        assert_eq!(inner.projects.lock().unwrap().list().len(), 1);

        // Rescans (watcher, restart, Scan) must not bring it back.
        inner.sync_auto_domains().unwrap();
        inner.sync_auto_domains().unwrap();
        let ids: Vec<_> = inner
            .projects
            .lock()
            .unwrap()
            .list()
            .into_iter()
            .map(|p| p.id)
            .collect();
        assert!(!ids.contains(&id1), "removed project was re-registered");
        assert_eq!(ids.len(), 1);

        // Explicitly adding the folder again clears the removal.
        let p = inner
            .projects
            .lock()
            .unwrap()
            .register(&app1.display().to_string())
            .unwrap();
        inner.clear_project_skip(&p.path).unwrap();
        inner.sync_auto_domains().unwrap();
        assert_eq!(inner.projects.lock().unwrap().list().len(), 2);
    }
}

#[cfg(test)]
mod open_database_tests {
    use super::*;

    #[test]
    fn redis_tinyrdm_routes_to_detected_tool() {
        let home = crate::test_support::isolated_home();
        let settings = crate::settings::SettingsService::load(&home.paths).unwrap();
        let inner = Inner::new(settings, home.paths.clone()).unwrap();
        let err = inner
            .open_database("redis", None, None, Some("tinyrdm"))
            .unwrap_err()
            .to_string();
        assert!(
            !err.contains("No tool is registered"),
            "tinyrdm fell through routing: {err}"
        );
    }
}

#[cfg(test)]
mod log_source_tests {
    use super::*;

    fn inner() -> (Arc<Inner>, crate::test_support::IsolatedHome) {
        let home = crate::test_support::isolated_home();
        let settings = crate::settings::SettingsService::load(&home.paths).unwrap();
        (Inner::new(settings, home.paths.clone()).unwrap(), home)
    }

    /// A service names the log source holding its own output, so "View logs" on
    /// a service row opens that service rather than the app log.
    #[test]
    fn every_service_names_its_own_log_source() {
        let (inner, _home) = inner();
        for s in inner.services.list() {
            let source = s
                .log_source
                .unwrap_or_else(|| panic!("{} names no log source", s.id));
            if s.kind == "web" {
                assert!(source.starts_with("web:"), "{}: {source}", s.id);
            } else {
                assert_eq!(source, format!("service:{}", s.id), "{}", s.id);
            }
        }
    }

    #[test]
    fn a_stopped_service_reads_empty_rather_than_failing() {
        let (inner, _home) = inner();
        // The output buffer dies with the process, so a stopped service has no
        // lines — an error here would look like a broken Logs page.
        assert!(inner.read_log("service:redis", 100).unwrap().is_empty());
        inner.clear_log("service:redis").unwrap();
    }

    #[test]
    fn each_web_server_reads_its_own_error_log() {
        let (inner, home) = inner();
        for id in crate::web::SERVER_IDS {
            let file = home.paths.web_dir().join(id).join("logs").join("error.log");
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, format!("{id} failed to bind\n")).unwrap();

            let lines = inner.read_log(&format!("web:{id}:error"), 100).unwrap();
            assert_eq!(lines, vec![format!("{id} failed to bind")], "{id}");

            inner.clear_log(&format!("web:{id}:error")).unwrap();
            assert!(inner
                .read_log(&format!("web:{id}:error"), 100)
                .unwrap()
                .is_empty());
        }
    }

    #[test]
    fn an_unknown_web_server_is_reported_not_silently_empty() {
        let (inner, _home) = inner();
        let err = inner
            .read_log("web:traefik:error", 100)
            .unwrap_err()
            .to_string();
        assert!(err.contains("traefik"), "{err}");
    }
}

#[cfg(test)]
mod startup_visibility_tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    /// The bug this replaced: the preference was ANDed with `--minimized`, so a user
    /// who turned "Start minimized to the tray" on still got a window every time
    /// they opened the app themselves. Only a launch at login hid it.
    #[test]
    fn the_preference_alone_hides_a_manual_launch() {
        assert!(should_start_hidden(
            true,
            args(&["C:\\app\\Open Local Server.exe"])
        ));
    }

    #[test]
    fn turning_the_preference_off_opens_the_window() {
        assert!(!should_start_hidden(
            false,
            args(&["C:\\app\\Open Local Server.exe"])
        ));
    }

    #[test]
    fn the_flag_forces_a_hidden_start_with_the_preference_off() {
        assert!(should_start_hidden(
            false,
            args(&["C:\\app\\Open Local Server.exe", FORCE_MINIMIZED_ARG])
        ));
    }

    #[test]
    fn the_flag_changes_nothing_when_the_preference_is_on() {
        assert!(should_start_hidden(
            true,
            args(&["C:\\app\\Open Local Server.exe", FORCE_MINIMIZED_ARG])
        ));
    }

    /// Every argument the app is actually launched with must be tolerated; only the
    /// exact flag may hide the window. A near-miss like `--minimized=true` is a
    /// different argument and must not silently start hidden.
    #[test]
    fn only_the_exact_flag_counts() {
        for other in [
            "--minimized=true",
            "--hidden",
            "--tray",
            "--MINIMIZED",
            "-minimized",
        ] {
            assert!(
                !should_start_hidden(false, args(&["app.exe", other])),
                "{other} hid the window"
            );
        }
    }

    /// The stored preference is what reaches the decision, so a value that is not a
    /// bool must not quietly flip a launch either way. `setting_bool` answers false
    /// for a non-bool instead of inventing the default.
    #[test]
    fn a_non_bool_stored_value_does_not_hide_the_window() {
        let home = crate::test_support::isolated_home();
        let settings = crate::settings::SettingsService::load(&home.paths).unwrap();
        let inner = Inner::new(settings, home.paths.clone()).unwrap();

        // Never written: the documented default is to start minimized.
        assert!(inner.setting_bool("startup.minimized", true));
        assert!(should_start_hidden(
            inner.setting_bool("startup.minimized", true),
            args(&["app.exe"])
        ));

        // Written as a string, not a bool: the read falls back to the default the
        // caller passed, so the launch follows the default rather than a guess.
        let mut settings = crate::settings::SettingsService::load(&home.paths).unwrap();
        settings
            .set("startup.minimized", serde_json::json!("true"))
            .unwrap();
        let inner = Inner::new(settings, home.paths.clone()).unwrap();
        assert!(inner.setting_bool("startup.minimized", true));
        assert!(!inner.setting_bool("startup.minimized", false));
    }

    /// The preference survives a restart, because the decision is made on a later
    /// launch than the one where it was toggled.
    #[test]
    fn turning_the_preference_off_survives_a_reload() {
        let home = crate::test_support::isolated_home();
        let mut settings = crate::settings::SettingsService::load(&home.paths).unwrap();
        settings
            .set("startup.minimized", serde_json::json!(false))
            .unwrap();
        drop(settings);

        let reloaded = crate::settings::SettingsService::load(&home.paths).unwrap();
        let inner = Inner::new(reloaded, home.paths.clone()).unwrap();
        assert!(!should_start_hidden(
            inner.setting_bool("startup.minimized", true),
            args(&["app.exe"])
        ));
    }
}
