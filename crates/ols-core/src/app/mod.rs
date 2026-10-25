//! The application core's shared state (`Inner`) and managers.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::certs::CertificateManager;
use crate::custom_install::CustomInstallStore;
use crate::dbtools::ExternalToolStore;
use crate::domain::{AppSpec, DomainStore, SiteBlocks, SiteKind};
use crate::error::CoreError;
use crate::paths::AppPaths;
use crate::php::PhpPools;
use crate::process::{ProcessId, ProcessSpec, ProcessSupervisor};
use crate::project::ProjectStore;
use crate::quickapp::commands::{CommandHistory, QuickCommandStore};
use crate::quickapp::plan::{self, PlanCtx, RunPlan};
use crate::quickapp::{QuickCatalog, RunManager};
use crate::runtime::{RuntimeEvent, RuntimeManager};
use crate::service::ServiceManager;
use crate::settings::SettingsService;
use crate::sqlite::SqliteStore;
use crate::web::manager::WebManager;
use crate::web::WebConfig;

pub mod domains;
pub mod projects;
pub mod quickapp;
pub mod system;
pub mod web;

#[cfg(test)]
mod tests;

pub use self::domains::*;
pub use self::projects::*;
pub use self::quickapp::*;
pub use self::system::*;

/// Recovers a mutex lock even if poisoned, preventing cascading panics.
pub(crate) fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub(crate) fn svc(msg: impl Into<String>) -> CoreError {
    CoreError::ServiceError(msg.into())
}

pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub const FORCE_MINIMIZED_ARG: &str = "--minimized";
pub const PROJECT_IGNORED_FOLDERS: &str = "projects.ignored_folders";
pub const OLS_EXCLUDED_FOLDER: &str = "ols_excluded_sites";
pub const DEFAULT_IGNORED_FOLDERS: &[&str] = &[OLS_EXCLUDED_FOLDER];
pub const PROJECT_ROOTS: &str = "projects.roots";
pub const AUTO_SKIP: &str = "domains.auto_skip";
pub const PROJECT_SKIP: &str = "projects.removed";
pub const REG_RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
pub const REG_VALUE: &str = "OpenLocalServer";

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
    /// §164: the background work the Processes page lists, with per-task progress.
    pub tasks: crate::tasks::TaskManager,
    /// Finding ids already attempted by automatic diagnostics during this app session.
    pub auto_fix_attempted: Mutex<HashSet<String>>,
    /// Last automatic-fix failures, shown in the corresponding diagnostic's details.
    pub auto_fix_failures: Mutex<HashMap<String, String>>,
    project_events: broadcast::Sender<()>,
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
    /// Also served at `localhost/<path_prefix>` (§55), when one is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_prefix: Option<String>,
    /// HTTP port of the server that renders this site, so the UI can build the localhost URL.
    pub http_port: u16,
    /// Optional Font Awesome icon identifier or class (e.g. "fa-wordpress" or "globe").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Data URL (`data:image/webp;base64,...`) when `ols_site_icon.webp` exists in `folder`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_image: Option<String>,
    /// SSL certificate provider: "local_ca", "lets_encrypt", "custom".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssl_provider: Option<String>,
    /// Whether automatic online broadcasting is active for this domain.
    #[serde(default)]
    pub auto_broadcast: bool,
    /// Live public broadcast URL if connected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub broadcast_url: Option<String>,
    /// Additional domain aliases pointing to this site.
    #[serde(default)]
    pub aliases: Vec<String>,
    /// Dynamic DNS provider: "cloudflare", "duckdns", "custom", or None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ddns_provider: Option<String>,
    /// Last detected public IP synced to DNS.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ddns_last_ip: Option<String>,
    /// For proxy sites: the target upstream URL, e.g. "http://127.0.0.1:3000".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy_target: Option<String>,
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
    /// Every service to start with the app, web servers included. A web server in this
    /// list is applied (config rendered and validated) before it starts, so it answers
    /// for the user's sites rather than only holding a port.
    pub autostart_services: Vec<String>,
    pub notifications: bool,
    pub close_to_tray: bool,
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

impl Inner {
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
            tasks: Default::default(),
            auto_fix_attempted: Mutex::new(HashSet::new()),
            auto_fix_failures: Mutex::new(HashMap::new()),
            project_events,
            paths,
        });
        core.start_projects_watcher();
        core.sync_php_external();
        core.services.set_limits(core.resource_limits());
        core.web.set_limits(core.resource_limits());
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

    pub(crate) fn default_projects_dir(&self) -> PathBuf {
        if let Some(dir) = self
            .settings
            .lock()
            .unwrap()
            .get("quickapps.projects_dir")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            // The value arrives through the generic `set_setting`, so a folder inside the
            // app may be stored relative (`sites/shop`) or, from an older build, as the
            // absolute path the install had at the time. Either way it resolves against
            // where the app lives now, so moving the install moves this default with it.
            return self.paths.decode_root(dir);
        }
        self.paths.sites_dir()
    }

    pub fn sites_dir(&self) -> PathBuf {
        self.paths.sites_dir()
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

    /// Reads a project and everything its page shows: detection, manifest and resolved
    /// runtimes.
    ///
    /// `build_detail` does blocking file I/O and spawns version probes, so the stores are
    /// cloned and their locks dropped first — holding `settings`/`custom_installs` across
    /// that work blocked every other command behind an unreachable network mount. A poisoned
    /// lock is recovered rather than unwrapped: one panic while a lock was held used to make
    /// every later read panic too, which surfaced as a page stuck loading for the whole
    /// session.
    /// Re-reads the stores when another process wrote to `app.db` under us, and gives the
    /// projects that arrived their automatic sites.
    ///
    /// The Explorer right-click menu and the `ols` command line run in their own process
    /// and write the same database the app has cached in memory, so a site added that way
    /// used to stay invisible until the app was restarted. The check is one single-row
    /// read of the writer marker, so every list of sites and projects can afford it.
    ///
    /// The re-read is followed by [`Self::sync_auto_domains`], because a project arriving
    /// from outside is exactly what that call turns into a `<folder>.<tld>` site — reading
    /// the rows back on its own would list a project with no domain and leave the user
    /// wondering why their new folder is not a site yet.
    pub fn reload_external_changes(&self) -> bool {
        if !crate::db::changed_externally(&self.paths) {
            return false;
        }
        // One store at a time, each released before the next is taken: the lock order at
        // the top of this module forbids holding settings and projects together.
        match ProjectStore::load(&self.paths) {
            Ok(store) => *lock(&self.projects) = store,
            Err(e) => tracing::warn!(%e, "projects could not be re-read after an outside change"),
        }
        match DomainStore::load(&self.paths) {
            Ok(store) => *lock(&self.domains) = store,
            // A store that will not load must not replace one that does: the list the user
            // is looking at stays as it was.
            Err(e) => tracing::warn!(%e, "domains could not be re-read after an outside change"),
        }
        if let Ok(store) = SettingsService::load(&self.paths) {
            *lock(&self.settings) = store;
        }
        // Every lock is released by now, so this may take them again in its own order.
        if let Err(e) = self.sync_auto_domains() {
            tracing::warn!(%e, "automatic domains could not be synced after an outside change");
        }
        true
    }
}
