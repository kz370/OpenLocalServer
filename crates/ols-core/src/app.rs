//! The application core's shared state (`Inner`) and everything the commands do with it.
//! `Core` (in `command.rs`) is a thin handle onto an `Arc<Inner>`, which is what lets a
//! Quick App run on a background thread keep using the same managers the UI does.
//!
//! Lock order, to keep this deadlock-free: `domains` is always taken first; `settings`,
//! `projects` and `custom_installs` are only ever held briefly and never while waiting on
//! anything slow (a process, the network, another lock).

use std::collections::BTreeMap;
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
        let services = Arc::new(ServiceManager::new(paths.clone(), runtimes.clone(), supervisor.clone()));
        let certs = Arc::new(CertificateManager::new(&paths));
        let php = Arc::new(PhpPools::new(paths.clone(), runtimes.clone(), supervisor.clone()));
        let web = Arc::new(WebManager::new(paths.clone(), runtimes.clone(), supervisor.clone(), certs.clone(), php.clone()));
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
            paths,
        });
        core.sync_php_external();
        Ok(core)
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
        self.settings.lock().unwrap().get(key).and_then(|v| v.as_bool()).unwrap_or(default)
    }

    fn default_projects_dir(&self) -> PathBuf {
        if let Some(dir) = self.settings.lock().unwrap().get("quickapps.projects_dir").and_then(|v| v.as_str()) {
            return PathBuf::from(dir);
        }
        directories::UserDirs::new().map(|d| d.home_dir().join("Sites")).unwrap_or_else(|| self.paths.root().join("Sites"))
    }

    pub fn plan_ctx(&self) -> PlanCtx {
        let cfg = self.web_config();
        PlanCtx {
            projects_dir: self.default_projects_dir(),
            web_server: cfg.server.clone(),
            http_port: cfg.http_port,
            https_port: cfg.https_port,
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
        let norm = |p: &str| p.replace('/', "\\").trim_end_matches('\\').to_ascii_lowercase();
        self.projects.lock().unwrap().list().into_iter().find(|p| norm(&p.path) == norm(path))
    }

    /// The runtime's bin dir a project resolves to (custom pin, then managed), else the
    /// newest managed install.
    fn runtime_bin_for(&self, project_id: Option<&str>, id: &str) -> Option<PathBuf> {
        if let Some(pid) = project_id {
            if let Some(detail) = self.project_detail(pid) {
                if let Some(dir) = detail.resolved.iter().find(|r| r.id == id).and_then(|r| r.bin_dir.clone()) {
                    return Some(PathBuf::from(dir));
                }
            }
        }
        let versions = self.runtimes.installed_versions(id);
        let newest = crate::php::pick_version(&versions, None)?;
        self.runtimes.bin_dir(id, &newest)
    }

    // -------------------------------------------------------------------- domains

    pub fn site_url(&self, d: &Domain, cfg: &WebConfig) -> String {
        let (scheme, port, default) = if d.https { ("https", cfg.https_port, 443) } else { ("http", cfg.http_port, 80) };
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
                folder: site_folder(&d, &projects),
            })
            .collect()
    }

    /// Validates the structured blocks before they can reach a config file: these strings
    /// are pasted into Nginx/Apache/Caddy syntax, so nothing that could break out of a
    /// directive (newlines, quotes, braces, semicolons) is allowed through.
    pub fn validate_blocks(blocks: &SiteBlocks) -> Result<(), CoreError> {
        let bad = |s: &str| s.chars().any(|c| matches!(c, '\n' | '\r' | '"' | ';' | '{' | '}' | '\0'));
        let name_ok = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        let d = |m: String| CoreError::DomainError(m);
        for h in &blocks.headers {
            if !name_ok(&h.name) || bad(&h.value) {
                return Err(d(format!("header \"{}\" has characters that aren't allowed", h.name)));
            }
        }
        for r in &blocks.redirects {
            if !r.from.starts_with('/') || bad(&r.from) || bad(&r.to) || r.from.contains(' ') || r.to.contains(' ') || !matches!(r.code, 301 | 302 | 303 | 307 | 308) {
                return Err(d(format!("redirect \"{}\" is invalid (paths must start with / and the code must be 301, 302, 303, 307 or 308)", r.from)));
            }
        }
        for m in &blocks.mappings {
            let up_ok = m.upstream.starts_with("http://") || m.upstream.starts_with("https://");
            if !m.path.starts_with('/') || bad(&m.path) || m.path.contains(' ') || bad(&m.upstream) || m.upstream.contains(' ') || !up_ok {
                return Err(d(format!("proxy mapping \"{}\" is invalid (the upstream must be an http:// or https:// URL)", m.path)));
            }
        }
        for u in &blocks.upstreams {
            if !name_ok(&u.name) || u.servers.is_empty() || u.servers.iter().any(|s| bad(s) || s.contains(' ')) {
                return Err(d(format!("upstream \"{}\" is invalid", u.name)));
            }
        }
        for i in &blocks.includes {
            if bad(i) {
                return Err(d("an include path has characters that aren't allowed".to_string()));
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
            return Err(CoreError::DomainError("the document root must be a full folder path".into()));
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
        let was_auto = domains.get(hostname).is_some_and(|d| d.project_id.is_some());
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
        s.get(key).and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default()
    }

    fn edit_string_list(&self, key: &str, edit: impl FnOnce(&mut Vec<String>)) -> Result<(), CoreError> {
        let mut list = self.string_list(key);
        let before = list.clone();
        edit(&mut list);
        list.dedup();
        if list != before {
            self.settings.lock().unwrap().set(key, serde_json::json!(list))?;
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

    /// Laragon-style automatic domains (setting `domains.auto`, on by default): every
    /// folder in a remembered projects folder becomes a project, and every project that
    /// can be served (PHP or plain HTML) gets `<folder>.test` over HTTPS. Domains the user
    /// deleted stay deleted. Applies the web config when something was added and the
    /// server is running. Returns how many domains were created.
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
            let roots: Vec<String> = parents.into_iter().filter(|(_, n)| *n >= 2).map(|(p, _)| p).collect();
            self.settings.lock().unwrap().set(PROJECT_ROOTS, serde_json::json!(roots))?;
        }
        for root in self.string_list(PROJECT_ROOTS) {
            let Ok(entries) = std::fs::read_dir(&root) else { continue };
            let mut projects = self.projects.lock().unwrap();
            for e in entries.flatten() {
                let dir = e.path();
                let hidden = e.file_name().to_string_lossy().starts_with('.');
                if dir.is_dir() && !hidden && (crate::detection::looks_like_a_project(&dir) || has_index(&dir)) {
                    let _ = projects.register(&dir.display().to_string());
                }
            }
        }

        let skip = self.string_list(AUTO_SKIP);
        let projects = self.projects.lock().unwrap().list();
        let mut created = 0;
        for p in projects {
            let path = Path::new(&p.path);
            let (taken, linked) = {
                let domains = self.domains.lock().unwrap();
                let hostname = crate::domain::apply_template("{project}.test", &p.name);
                (domains.get(&hostname).is_some() || skip.contains(&hostname), domains.list().iter().any(|d| d.project_id.as_deref() == Some(&p.id)))
            };
            if taken || linked || !path.is_dir() {
                continue;
            }
            let Some((kind, root)) = auto_site(path) else { continue };
            let domain = Domain {
                hostname: crate::domain::apply_template("{project}.test", &p.name),
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
        let new_hostname = new_hostname.trim().to_ascii_lowercase();
        if new_hostname == hostname {
            return self.domains.lock().unwrap().get(hostname).ok_or_else(|| CoreError::DomainError(format!("{hostname} is not a known domain")));
        }
        let mut domains = self.domains.lock().unwrap();
        let original = domains.get(hostname).ok_or_else(|| CoreError::DomainError(format!("{hostname} is not a known domain")))?;
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

    pub fn duplicate_domain(&self, hostname: &str, new_hostname: &str) -> Result<Domain, CoreError> {
        let mut domains = self.domains.lock().unwrap();
        let mut copy = domains.get(hostname).ok_or_else(|| CoreError::DomainError(format!("{hostname} is not a known domain")))?;
        copy.hostname = new_hostname.trim().to_ascii_lowercase();
        copy.generated_hashes.clear();
        // Two sites can't share one dev-server port or process: the copy needs its own.
        copy.app = None;
        domains.add(copy)
    }

    pub fn apply_web(&self, overwrite: &[String]) -> Result<ApplyReport, CoreError> {
        let cfg = self.web_config();
        let php_for = |d: &Domain| -> Option<String> {
            let detail = self.project_detail(d.project_id.as_deref()?)?;
            detail.resolved.into_iter().find(|r| r.id == "php").and_then(|r| r.installed_version)
        };
        let runtime_bin = |d: &Domain, rt: &str| self.runtime_bin_for(d.project_id.as_deref(), rt);
        let ctx = ApplyContext { cfg: &cfg, php_for: &php_for, runtime_bin: &runtime_bin, overwrite };
        let mut domains = self.domains.lock().unwrap();
        self.web.apply(&ctx, &mut domains)
    }

    pub fn set_domain_enabled(&self, hostname: &str, enabled: bool) -> Result<(), CoreError> {
        let mut domains = self.domains.lock().unwrap();
        let mut d = domains.get(hostname).ok_or_else(|| CoreError::DomainError(format!("{hostname} is not a known domain")))?;
        d.enabled = enabled;
        domains.update(d)?;
        Ok(())
    }

    pub fn write_web_config(&self, hostname: &str, part: crate::web::manager::ConfigPart, content: &str) -> Result<String, CoreError> {
        let cfg = self.web_config();
        let mut domains = self.domains.lock().unwrap();
        self.web.write_config(&cfg, &mut domains, hostname, part, content)
    }

    pub fn set_ownership(&self, hostname: &str, ownership: Ownership) -> Result<(), CoreError> {
        let cfg = self.web_config();
        let mut domains = self.domains.lock().unwrap();
        self.web.set_ownership(&cfg, &mut domains, hostname, ownership)
    }

    pub fn restore_web_history(&self, hostname: &str, id: &str) -> Result<String, CoreError> {
        let cfg = self.web_config();
        let mut domains = self.domains.lock().unwrap();
        self.web.restore_history(&cfg, &mut domains, hostname, id)
    }

    /// §53 health chain for one site.
    pub fn health_check(&self, hostname: &str) -> Result<HealthReport, CoreError> {
        let cfg = self.web_config();
        let d = self.domains.lock().unwrap().get(hostname).ok_or_else(|| CoreError::DomainError(format!("{hostname} is not a known domain")))?;
        let cert = self.certs.info(hostname);
        let ca_info = self.certs.ca_info();
        let ca_path = PathBuf::from(&ca_info.cert_path);
        Ok(health::check_site(&HealthTarget {
            hostname,
            https: d.https,
            http_port: cfg.http_port,
            https_port: cfg.https_port,
            ca_pem: &ca_path,
            ca_trusted: ca_info.trusted,
            cert: cert.as_ref(),
        }))
    }

    // ------------------------------------------------------------------ databases

    pub fn open_database(&self, engine: &str, database: Option<&str>, path: Option<&str>, tool_id: Option<&str>) -> Result<(), CoreError> {
        let info = self.services.connection_info(engine, database, path).map_err(svc)?;

        // 1) an explicitly chosen registered tool, 2) the first registered for the engine,
        // 3) HeidiSQL (mysql/mariadb/sqlite) or pgAdmin if detected.
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
        let custom_path = |id: &str| self.custom_installs.lock().unwrap().resolve(id, None).map(|c| c.path.clone());
        let detected = dbtools::detect_db_tools();
        let find = |id: &str| custom_path(id).or_else(|| detected.iter().find(|t| t.id == id).and_then(|t| t.found_path.clone()));

        match engine {
            "mysql" | "mariadb" | "sqlite" => {
                let exe = find("heidisql").ok_or_else(|| svc("HeidiSQL was not found. Install it, locate it, or register another tool for this engine."))?;
                let args = dbtools::heidisql_args(&info).ok_or_else(|| svc("HeidiSQL can't open that database"))?;
                dbtools::launch(&exe, &args, true).map_err(svc)
            }
            "postgres" => {
                let exe = find("pgadmin").ok_or_else(|| svc("pgAdmin was not found. Install it or locate it."))?;
                dbtools::launch(&exe, &[], false).map_err(svc)
            }
            other => Err(svc(format!("No tool is registered for {other}. Add one under External tools."))),
        }
    }

    /// The `sqlite3` binary: a pinned custom install, the managed one, or PATH.
    pub fn sqlite3_path(&self) -> Result<PathBuf, CoreError> {
        if let Some(c) = self.custom_installs.lock().unwrap().resolve("sqlite", None) {
            return Ok(PathBuf::from(&c.path));
        }
        if let Some(v) = self.runtimes.installed_versions("sqlite").into_iter().next() {
            if let Some(p) = self.runtimes.binary_path("sqlite", &v) {
                return Ok(p);
            }
        }
        crate::web::manager::find_executable(None, "sqlite3").ok_or_else(|| svc("SQLite is not installed. Install it from the Runtimes page."))
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
        let mut cmd = if crate::editors::is_shim(&exe) {
            let mut c = std::process::Command::new("cmd.exe");
            c.arg("/C").arg(&exe).arg(path);
            c
        } else {
            let mut c = std::process::Command::new(&exe);
            c.arg(path);
            c
        };
        crate::exec::hide_window(&mut cmd);
        cmd.spawn().map(|_| ()).map_err(|e| svc(format!("could not start {}: {e}", exe.display())))
    }

    // ---------------------------------------------------------- startup (§121)

    pub fn startup_settings(&self) -> StartupSettings {
        let s = self.settings.lock().unwrap();
        let b = |k: &str, d: bool| s.get(k).and_then(|v| v.as_bool()).unwrap_or(d);
        let services = s
            .get("startup.services")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
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
            s.set("startup.services", serde_json::json!(new.autostart_services))?;
            s.set("notifications.enabled", serde_json::json!(new.notifications))?;
            s.set("startup.close_to_tray", serde_json::json!(new.close_to_tray))?;
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
            tracing::info!(orphans, "stopped servers left running by a previous session");
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
    }

    // ----------------------------------------------------------- environment health

    /// §116: one list answering "is my setup healthy, and if not, what do I do?".
    pub fn environment_health(&self) -> Vec<HealthItem> {
        let cfg = self.web_config();
        let mut items = Vec::new();
        let item = |id: &str, label: &str, status: &str, detail: String, fix: Option<&str>| HealthItem {
            id: id.into(),
            label: label.into(),
            status: status.into(),
            detail,
            fix: fix.map(str::to_string),
        };

        let server_name = crate::web::server_by_id(&cfg.server).map(|s| s.name()).unwrap_or("Web server");
        let server_installed = !self.runtimes.installed_versions(&cfg.server).is_empty();
        items.push(if server_installed {
            item("web_installed", server_name, "ok", format!("{server_name} is installed"), None)
        } else {
            item("web_installed", server_name, "error", format!("{server_name} is not installed"), Some("Install it from the Runtimes page."))
        });

        let web = self.web.status(&cfg);
        let domain_count = self.domains.lock().unwrap().list().iter().filter(|d| d.enabled).count();
        if web.running {
            items.push(item("web_running", "Web server", "ok", format!("running on ports {} / {}", cfg.http_port, cfg.https_port), None));
        } else if domain_count > 0 {
            items.push(item("web_running", "Web server", "warn", "stopped, so your sites are offline".into(), Some("Apply the web config from the Domains page.")));
        }
        for conflict in &web.port_conflicts {
            items.push(item("port_conflict", "Ports", "warn", conflict.clone(), Some("Stop that program or choose other ports in the web settings.")));
        }

        let ca = self.certs.ca_info();
        let any_https = self.domains.lock().unwrap().list().iter().any(|d| d.https);
        if any_https || ca.exists {
            items.push(if ca.trusted {
                item("ca_trusted", "Local certificate authority", "ok", "trusted by Windows".into(), None)
            } else {
                item("ca_trusted", "Local certificate authority", "warn", "not trusted, browsers will warn about HTTPS sites".into(), Some("Trust it from the Certificates page."))
            });
        }
        for cert in self.certs.list() {
            match cert.status {
                crate::certs::CertStatus::Expired => items.push(item("cert_expired", &format!("Certificate {}", cert.hostname), "error", "expired".into(), Some("Regenerate it from the Certificates page."))),
                crate::certs::CertStatus::Expiring => items.push(item("cert_expiring", &format!("Certificate {}", cert.hostname), "warn", format!("expires in {} days", cert.days_left), Some("It renews on the next apply."))),
                crate::certs::CertStatus::Valid => {}
            }
        }

        let enabled: Vec<String> = self.domains.lock().unwrap().list().into_iter().filter(|d| d.enabled).map(|d| d.hostname).collect();
        if !enabled.is_empty() {
            let existing = std::fs::read_to_string(crate::hosts::hosts_path()).unwrap_or_default();
            let missing: Vec<&String> = enabled.iter().filter(|h| !self.web.dns_covers(h) && !crate::hosts::lists(&existing, h)).collect();
            items.push(if missing.is_empty() {
                item("hosts", "Domain names", "ok", "every domain resolves to this computer".into(), None)
            } else {
                item("hosts", "Domain names", "warn", format!("not resolving yet: {}", missing.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")), Some("Apply the web config."))
            });
        }

        if self.php.all_versions().is_empty() {
            items.push(item("php", "PHP", "warn", "no PHP version is installed".into(), Some("Install one from the Runtimes page.")));
        }
        for s in self.services.list().into_iter().filter(|s| s.installed) {
            items.push(match (s.running, s.healthy) {
                (true, Some(false)) => item(&format!("svc_{}", s.id), &s.name, "warn", "running but not answering on its port".into(), Some("Restart the service.")),
                (true, _) => item(&format!("svc_{}", s.id), &s.name, "ok", "running".into(), None),
                (false, _) => item(&format!("svc_{}", s.id), &s.name, "ok", "stopped".into(), None),
            });
        }
        items
    }

    // ------------------------------------------------------------- monitoring

    /// Machine and process usage, plus what each enabled site costs. Shared processes (a
    /// PHP version's workers, the web server) are reported whole, with how many sites
    /// share them, rather than split by guesswork.
    pub fn system_stats(&self) -> crate::monitor::SystemStats {
        let procs = self.supervisor.snapshot();
        let pid_of = |id: ProcessId| procs.iter().find(|p| p.id == id).and_then(|p| p.pid);
        let mut stats = self.monitor.stats(&procs.iter().filter_map(|p| p.pid).collect::<Vec<_>>());
        let plan = self.web.usage_plan();
        let sum = |ids: &[ProcessId]| {
            ids.iter().filter_map(|id| pid_of(*id)).filter_map(|pid| stats.processes.get(&pid)).fold((0.0f32, 0u64), |(c, m), s| (c + s.cpu_percent, m + s.memory))
        };
        let enabled: Vec<Domain> = self.domains.lock().unwrap().list().into_iter().filter(|d| d.enabled).collect();
        let server = plan.server.map(|s| sum(&[s])).unwrap_or_default();
        let static_sites = enabled.iter().filter(|d| !plan.apps.contains_key(&d.hostname) && !plan.site_php.contains_key(&d.hostname)).count();
        let mut sites = Vec::new();
        for d in &enabled {
            let (via, (cpu, memory), shared_by, measured) = if let Some(app) = plan.apps.get(&d.hostname) {
                ("App process".to_string(), sum(&[*app]), 1, true)
            } else if let Some(version) = plan.site_php.get(&d.hostname) {
                let workers = plan.pools.get(version).cloned().unwrap_or_default();
                let sharing = plan.site_php.values().filter(|v| *v == version).count();
                (format!("PHP {version} workers"), sum(&workers), sharing, true)
            } else if let SiteKind::Proxy { upstream_host: Some(host), upstream_port, .. } = &d.kind {
                (format!("Forwarded to {host}:{upstream_port}"), (0.0, 0), 1, false)
            } else {
                ("Web server".to_string(), server, static_sites.max(1), true)
            };
            sites.push(crate::monitor::SiteUsage { hostname: d.hostname.clone(), via, cpu_percent: cpu, memory, shared_by, measured });
        }
        stats.sites = sites;

        let mut places = vec![("OpenLocalServer data".to_string(), self.paths.root().to_path_buf())];
        places.extend(self.string_list(PROJECT_ROOTS).into_iter().map(|r| ("Projects".to_string(), PathBuf::from(r))));
        places.push(("Projects".to_string(), self.default_projects_dir()));
        stats.disks = self.monitor.disks(&places);
        stats
    }

    // --------------------------------------------------------- database migration

    fn migration_source(&self, id: &str) -> Result<crate::migrate::MigrationSource, CoreError> {
        crate::migrate::detect().into_iter().find(|s| s.id == id).ok_or_else(|| svc(format!("{id} is no longer there")))
    }

    /// Databases in an old Laragon/XAMPP/Wamp server (started on a copy if it isn't running).
    pub fn foreign_databases(&self, source_id: &str, password: &str) -> Result<Vec<String>, CoreError> {
        let source = self.migration_source(source_id)?;
        let session = crate::migrate::Session::open(source, password, &self.paths.cache_dir()).map_err(svc)?;
        session.databases().map_err(svc)
    }

    /// Copies databases from an old server into ours (`target`: "mysql" | "mariadb").
    /// An empty `databases` list means all of them. Each database reports on its own, so
    /// one failure doesn't stop the rest.
    pub fn migrate_databases(&self, source_id: &str, password: &str, databases: &[String], target: &str) -> Result<Vec<crate::migrate::MigratedDb>, CoreError> {
        let source = self.migration_source(source_id)?;
        let (client, port) = self.services.sql_client(target).map_err(svc)?;
        if source.running_port == Some(port) {
            return Err(svc(format!(
                "{} is running on port {port}, which our {target} needs. Stop it (in Laragon/XAMPP) and try again; its data is copied, not moved.",
                source.label
            )));
        }
        if !self.services.is_running(target) {
            self.services.start(target).map_err(svc)?;
        }
        let started = std::time::Instant::now();
        while std::net::TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), std::time::Duration::from_millis(300)).is_err() {
            if started.elapsed() > std::time::Duration::from_secs(60) {
                return Err(svc(format!("our {target} did not start")));
            }
            std::thread::sleep(std::time::Duration::from_millis(300));
        }

        let work = self.paths.cache_dir();
        std::fs::create_dir_all(&work)?;
        let session = crate::migrate::Session::open(source, password, &work).map_err(svc)?;
        let names = if databases.is_empty() { session.databases().map_err(svc)? } else { databases.to_vec() };
        let mut results = Vec::new();
        for db in names {
            let file = work.join(format!("migrate-{}.sql", crate::domain::slugify(&db)));
            let outcome = session.dump(&db, &file).and_then(|()| crate::migrate::import(&client, port, &file));
            let _ = std::fs::remove_file(&file);
            tracing::info!(database = %db, ok = outcome.is_ok(), "database migration");
            results.push(match outcome {
                Ok(()) => crate::migrate::MigratedDb { name: db, ok: true, detail: "copied".into() },
                Err(e) => crate::migrate::MigratedDb { name: db, ok: false, detail: e },
            });
        }
        Ok(results)
    }

    // --------------------------------------------------------------------- logs

    pub fn log_sources(&self) -> Vec<LogSource> {
        let mut out = vec![LogSource { id: "app".into(), name: "OpenLocalServer".into(), kind: "app".into() }];
        let cfg = self.web_config();
        if let Some(s) = crate::web::server_by_id(&cfg.server) {
            out.push(LogSource { id: "web:error".into(), name: format!("{} error log", s.name()), kind: "web".into() });
            out.push(LogSource { id: "web:access".into(), name: format!("{} access log", s.name()), kind: "web".into() });
        }
        for p in self.supervisor.snapshot() {
            out.push(LogSource { id: format!("process:{}", p.id.0), name: p.name.clone(), kind: "process".into() });
        }
        for r in self.runs.list() {
            out.push(LogSource { id: format!("run:{}", r.id), name: format!("Quick App: {}", r.app_name), kind: "run".into() });
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
        let file_lines = |p: &Path| std::fs::read_to_string(p).map(|t| t.lines().map(str::to_string).collect::<Vec<_>>()).unwrap_or_default();
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
                Ok(tail(newest.map(|(_, p)| file_lines(&p)).unwrap_or_default()))
            }
            "web:error" | "web:access" => {
                let cfg = self.web_config();
                let server = crate::web::server_by_id(&cfg.server).ok_or_else(|| svc("unknown web server"))?;
                let logs = self.paths.web_dir().join(server.id()).join("logs");
                let file = if source == "web:error" { "error.log" } else { "access.log" };
                Ok(tail(file_lines(&logs.join(file))))
            }
            s if s.starts_with("process:") => {
                let id: u64 = s[8..].parse().map_err(|_| svc("bad process id"))?;
                Ok(tail(self.supervisor.recent_output(ProcessId(id))))
            }
            s if s.starts_with("run:") => {
                Ok(tail(self.runs.get(&s[4..]).map(|r| r.log).unwrap_or_default()))
            }
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
                let server = crate::web::server_by_id(&cfg.server).ok_or_else(|| svc("unknown web server"))?;
                let file = if source == "web:error" { "error.log" } else { "access.log" };
                truncate(&self.paths.web_dir().join(server.id()).join("logs").join(file))
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
                let label_of = |id: &str| catalog.iter().find(|m| m.id == id).map(|m| m.name.to_string()).unwrap_or_else(|| id.to_string());
                let label = match r.id.as_str() {
                    "python" => "Python".to_string(),
                    id => label_of(id),
                };
                if r.id == "python" {
                    return match crate::runtime::detect_system_install("python") {
                        Some(sys) => RequirementView { id: r.id.clone(), label, wanted: r.wanted.clone(), status: "installed".into(), detail: Some(sys.version) },
                        None => RequirementView {
                            id: r.id.clone(),
                            label,
                            wanted: r.wanted.clone(),
                            status: "unavailable".into(),
                            detail: Some("Python was not found on PATH. Install it from python.org.".into()),
                        },
                    };
                }
                let installed = self.runtimes.installed_versions(&r.id);
                if let Some(v) = crate::php::pick_version(&installed, r.wanted.as_deref()) {
                    return RequirementView { id: r.id.clone(), label, wanted: r.wanted.clone(), status: "installed".into(), detail: Some(v) };
                }
                let available: Vec<String> = catalog.iter().filter(|m| m.id == r.id).map(|m| m.version.to_string()).collect();
                match crate::php::pick_version(&available, r.wanted.as_deref()) {
                    Some(v) => RequirementView { id: r.id.clone(), label, wanted: r.wanted.clone(), status: "installable".into(), detail: Some(format!("will download {v}")) },
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
    fn install_blocking(&self, id: &str, version: &str, log: &mut dyn FnMut(&str)) -> Result<(), String> {
        let mut events = self.runtimes.subscribe();
        self.runtimes.install(id, version);
        let started = Instant::now();
        let mut last_pct = 0u64;
        loop {
            if started.elapsed() > Duration::from_secs(45 * 60) {
                return Err(format!("installing {id} {version} timed out"));
            }
            match events.blocking_recv() {
                Ok(RuntimeEvent::Installed { id: i, version: v, .. }) if i == id && v == version => return Ok(()),
                Ok(RuntimeEvent::Failed { id: i, version: v, message }) if i == id && v == version => {
                    return Err(format!("installing {id} {version} failed: {message}"));
                }
                Ok(RuntimeEvent::Progress { id: i, version: v, state, downloaded, total }) if i == id && v == version => {
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
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return Err("the install channel closed".into()),
            }
        }
    }

    fn quick_ensure_runtime(&self, id: &str, wanted: Option<&str>, log: &mut dyn FnMut(&str)) -> Result<String, String> {
        if id == "python" {
            return crate::runtime::detect_system_install("python")
                .map(|s| s.version)
                .ok_or_else(|| "Python was not found on PATH. Install it from python.org, then run this again.".to_string());
        }
        let catalog = crate::catalog::builtin_catalog();
        let name = catalog.iter().find(|m| m.id == id).map(|m| m.name).ok_or_else(|| format!("{id} isn't available for this platform"))?;

        let installed = self.runtimes.installed_versions(id);
        if let Some(v) = crate::php::pick_version(&installed, wanted) {
            return Ok(format!("{name} {v}"));
        }
        let available: Vec<String> = catalog.iter().filter(|m| m.id == id).map(|m| m.version.to_string()).collect();
        let version = crate::php::pick_version(&available, wanted).ok_or_else(|| match wanted {
            Some(w) => format!("{name} {w} isn't available. Available: {}", available.join(", ")),
            None => format!("{name} isn't available"),
        })?;
        log(&format!("Installing {name} {version}"));
        self.install_blocking(id, &version, log)?;
        Ok(format!("{name} {version}"))
    }

    fn quick_resolve_program(&self, program: &str, values: &BTreeMap<String, String>, project_id: Option<&str>) -> Result<ResolvedProgram, String> {
        let is_path = program.contains('/') || program.contains('\\') || Path::new(program).is_absolute();
        if is_path {
            return Ok(ResolvedProgram { executable: PathBuf::from(program), ..Default::default() });
        }
        let pick_php = || -> Result<(PathBuf, PathBuf), String> {
            // A project's own resolution (custom pin or manifest) wins, then the wizard's choice.
            if let Some(dir) = project_id.and_then(|p| self.project_detail(p)).and_then(|d| d.resolved.into_iter().find(|r| r.id == "php").and_then(|r| r.bin_dir)) {
                let dir = PathBuf::from(dir);
                let exe = dir.join("php.exe");
                if exe.is_file() {
                    return Ok((exe, dir));
                }
            }
            let wanted = values.get("php_version").filter(|v| !v.is_empty()).map(String::as_str);
            let v = self.php.pick_version(wanted).ok_or("PHP is not installed. Install it from the Runtimes page.")?;
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

        match program {
            "php" => {
                let (exe, dir) = pick_php()?;
                Ok(ResolvedProgram { env: php_env(&dir), executable: exe, pre_args: vec![], path_dirs: vec![dir] })
            }
            "composer" => {
                let (exe, dir) = pick_php()?;
                let v = self.runtimes.installed_versions("composer").into_iter().next().ok_or("Composer is not installed. Install it from the Runtimes page.")?;
                let phar = self.runtimes.binary_path("composer", &v).ok_or("composer.phar is missing")?;
                let home = self.paths.services_dir().join("composer");
                let _ = std::fs::create_dir_all(&home);
                let mut env = php_env(&dir);
                env.push(("COMPOSER_HOME".into(), home.display().to_string()));
                env.push(("COMPOSER_NO_INTERACTION".into(), "1".into()));
                Ok(ResolvedProgram { executable: exe, pre_args: vec![phar.display().to_string()], env, path_dirs: vec![dir] })
            }
            "node" | "npm" | "npx" => {
                let dir = project_id
                    .and_then(|p| self.project_detail(p))
                    .and_then(|d| d.resolved.into_iter().find(|r| r.id == "node").and_then(|r| r.bin_dir))
                    .map(PathBuf::from)
                    .or_else(|| {
                        let wanted = values.get("node_version").filter(|v| !v.is_empty()).map(String::as_str);
                        let installed = self.runtimes.installed_versions("node");
                        crate::php::pick_version(&installed, wanted).and_then(|v| self.runtimes.bin_dir("node", &v))
                    })
                    .ok_or("Node.js is not installed. Install it from the Runtimes page.")?;
                let file = crate::web::manager::find_executable(Some(&dir), program).ok_or_else(|| format!("{program} was not found in {}", dir.display()))?;
                Ok(shim(file, vec![dir]))
            }
            other => {
                let file = crate::web::manager::find_executable(None, other).ok_or_else(|| format!("\"{other}\" was not found. Install it or add it to PATH."))?;
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
        let get = |k: &str| with.get(k).map(String::as_str).ok_or_else(|| format!("the \"{action}\" action needs \"{k}\""));
        match action {
            "make_dir" => {
                let path = get("path")?;
                std::fs::create_dir_all(path).map_err(|e| format!("could not create {path}: {e}"))?;
                Ok(ActionOutcome::default())
            }
            "replace_in_file" => {
                let root = ctx.project_path.as_ref().ok_or("no project folder")?;
                let file = plan::safe_join(&root.display().to_string(), get("file")?)?;
                let text = std::fs::read_to_string(&file).map_err(|e| format!("could not read {file}: {e}"))?;
                let find = get("find")?;
                if !text.contains(find) {
                    return Err(format!("could not find \"{find}\" in {file}"));
                }
                std::fs::write(&file, text.replacen(find, get("replace")?, 1)).map_err(|e| e.to_string())?;
                Ok(ActionOutcome::default())
            }
            "start_service" => {
                let id = get("id")?;
                if !self.services.is_running(id) {
                    log(&format!("Starting {id}"));
                    self.services.start(id)?;
                }
                let port = self.services.status(id).port.ok_or("service has no port")?;
                // MySQL/MariaDB initialise their data directory on first start — give them time.
                let started = Instant::now();
                while started.elapsed() < Duration::from_secs(120) {
                    if !self.services.is_running(id) {
                        return Err(format!("{id} stopped right after starting. See its output on the Processes page."));
                    }
                    if std::net::TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(300)).is_ok() {
                        return Ok(ActionOutcome::default());
                    }
                    std::thread::sleep(Duration::from_millis(400));
                }
                Err(format!("{id} did not open port {port} in time"))
            }
            "create_database" => {
                let (engine, name) = (get("engine")?, get("name")?);
                let started = Instant::now();
                loop {
                    match self.services.create_database(engine, name) {
                        Ok(()) => return Ok(ActionOutcome { detail: Some(format!("database {name}")), ..Default::default() }),
                        // The server may still be finishing startup right after its port opened.
                        Err(e) if started.elapsed() < Duration::from_secs(30) && e.to_lowercase().contains("connect") => {
                            std::thread::sleep(Duration::from_millis(700));
                        }
                        Err(e) => return Err(e),
                    }
                }
            }
            "register_project" => {
                let path = get("path")?;
                let project = self.projects.lock().unwrap().register(path).map_err(|e| e.to_string())?;
                Ok(ActionOutcome { project_id: Some(project.id), ..Default::default() })
            }
            "create_domain" => {
                let hostname = get("hostname")?.to_string();
                let kind = match get("kind")? {
                    "php" => SiteKind::Php { version: with.get("php_version").cloned() },
                    "static" => SiteKind::Static,
                    "proxy" => SiteKind::Proxy {
                        upstream_port: with.get("port").and_then(|p| p.parse().ok()).ok_or("a proxy site needs a valid port")?,
                        upstream_host: with.get("upstream_host").map(|h| h.trim().to_string()).filter(|h| !h.is_empty() && h != "127.0.0.1" && h != "localhost"),
                        upstream_https: with.get("upstream_https").is_some_and(|v| v == "true"),
                    },
                    other => return Err(format!("unknown site kind {other}")),
                };
                let app = with.get("app_program").map(|program| AppSpec {
                    executable: program.clone(),
                    args: with.get("app_args").and_then(|a| serde_json::from_str(a).ok()).unwrap_or_default(),
                    cwd: with.get("app_cwd").cloned().unwrap_or_default(),
                    runtime: with.get("app_runtime").cloned(),
                });
                let root = get("root")?.replace('/', "\\");
                let project_id = ctx.project_path.as_ref().and_then(|p| self.project_by_path(&p.display().to_string())).map(|p| p.id);
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
                Ok(ActionOutcome { detail: Some(url.clone()), open_url: Some(url), ..Default::default() })
            }
            "trust_ca" => {
                self.certs.ca().ensure_created()?;
                if self.certs.ca().is_trusted() {
                    return Ok(ActionOutcome { detail: Some("already trusted".into()), ..Default::default() });
                }
                self.certs.ca().trust_current_user().map_err(|e| format!("Windows did not trust the certificate authority: {e}"))?;
                Ok(ActionOutcome::default())
            }
            "apply_web" => {
                let report = self.apply_web(&[]).map_err(|e| e.to_string())?;
                for w in &report.warnings {
                    log(&format!("warning: {w}"));
                }
                Ok(ActionOutcome::default())
            }
            "health_check" => {
                let report = self.health_check(get("hostname")?).map_err(|e| e.to_string())?;
                for s in &report.steps {
                    log(&format!("{} {}: {}", if s.ok { "✓" } else { "✗" }, s.name, s.detail));
                }
                if report.ok {
                    Ok(ActionOutcome::default())
                } else {
                    let failed: Vec<String> = report.steps.iter().filter(|s| !s.ok).map(|s| format!("{}: {}", s.name, s.detail)).collect();
                    Err(failed.join("; "))
                }
            }
            "download_extract" => {
                self.download_extract(get("url")?, with.get("sha1_url").map(String::as_str), with.get("sha256").map(String::as_str), get("dest")?, with.get("strip").map(String::as_str), log)?;
                Ok(ActionOutcome::default())
            }
            other => Err(format!("unknown action \"{other}\"")),
        }
    }

    /// Downloads a zip over HTTPS, verifies its checksum, and extracts it (stripping a
    /// leading folder). Used by recipes like WordPress that ship as one archive.
    fn download_extract(&self, url: &str, sha1_url: Option<&str>, sha256: Option<&str>, dest: &str, strip: Option<&str>, log: &mut dyn FnMut(&str)) -> Result<(), String> {
        use sha1::Digest as _;
        if !url.starts_with("https://") {
            return Err("downloads must use HTTPS".into());
        }
        if sha1_url.is_none() && sha256.is_none() {
            return Err("a download needs a checksum (sha1_url or sha256)".into());
        }
        log(&format!("Downloading {url}"));
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?;
        let client = reqwest::Client::new();
        let (bytes, expected) = rt.block_on(async {
            let bytes = client.get(url).send().await.and_then(|r| r.error_for_status()).map_err(|e| e.to_string())?.bytes().await.map_err(|e| e.to_string())?;
            let expected = match (sha1_url, sha256) {
                (Some(u), _) => {
                    let text = client.get(u).send().await.and_then(|r| r.error_for_status()).map_err(|e| e.to_string())?.text().await.map_err(|e| e.to_string())?;
                    ("sha1", text.split_whitespace().next().unwrap_or("").to_lowercase())
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
            return Err(format!("checksum mismatch: expected {}, got {actual}. Nothing was extracted.", expected.1));
        }
        log("Checksum verified");
        let cache = self.paths.cache_dir().join(format!("download-{}.zip", &actual[..16]));
        std::fs::create_dir_all(self.paths.cache_dir()).map_err(|e| e.to_string())?;
        std::fs::write(&cache, &bytes).map_err(|e| e.to_string())?;
        let result = crate::runtime::extract_zip(&cache, Path::new(dest), strip.unwrap_or("")).map_err(|e| e.to_string());
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
            if let Some(d) = self.domains.lock().unwrap().list().into_iter().find(|d| d.project_id.as_deref() == Some(p.id.as_str())) {
                values.insert("domain".into(), d.hostname);
            }
        }
        values
    }

    pub fn run_command_line(&self, line: &str, cwd: Option<&str>, project_id: Option<&str>, name: Option<&str>) -> Result<ProcessId, CoreError> {
        let tokens = plan::split_command_line(line);
        let (program, args) = tokens.split_first().ok_or_else(|| CoreError::QuickAppError("that command line is empty".into()))?;
        self.spawn_project_process(program, args, cwd, project_id, name.unwrap_or(line), line)
    }

    fn spawn_project_process(&self, program: &str, args: &[String], cwd: Option<&str>, project_id: Option<&str>, name: &str, history_line: &str) -> Result<ProcessId, CoreError> {
        let mut values = BTreeMap::new();
        // Pin the resolved versions so `php`/`node` follow the project (§18–19).
        if let Some(detail) = project_id.and_then(|id| self.project_detail(id)) {
            for r in &detail.resolved {
                if let Some(v) = &r.installed_version {
                    values.insert(format!("{}_version", r.id), v.clone());
                }
            }
        }
        let resolved = self.quick_resolve_program(program, &values, project_id).map_err(CoreError::QuickAppError)?;
        let mut full_args = resolved.pre_args.clone();
        full_args.extend(args.iter().cloned());
        let mut env = resolved.env.clone();
        if !resolved.path_dirs.is_empty() {
            let mut dirs: Vec<String> = resolved.path_dirs.iter().map(|d| d.display().to_string()).collect();
            dirs.push(std::env::var("PATH").unwrap_or_default());
            env.push(("PATH".into(), dirs.join(";")));
        }
        let cwd_owned = cwd.map(str::to_string).or_else(|| project_id.and_then(|id| self.projects.lock().unwrap().get(id)).map(|p| p.path));
        let id = self.supervisor.start(ProcessSpec {
            name: name.to_string(),
            executable: resolved.executable.display().to_string(),
            args: full_args,
            cwd: cwd_owned.clone(),
            env,
            restart: None,
        });
        let _ = self.history.lock().unwrap().record(history_line, cwd_owned.as_deref(), project_id);
        Ok(id)
    }

    pub fn run_quick_command(&self, id: &str, project_id: Option<&str>) -> Result<Option<ProcessId>, CoreError> {
        let cmd: QuickCommand = self.quick_commands.get(id).ok_or_else(|| CoreError::QuickAppError(format!("no Quick Command \"{id}\"")))?;
        let values = self.command_values(project_id);
        let render = |t: &str| plan::render(t, &values, &[]).map_err(CoreError::QuickAppError);

        if let Some(action) = &cmd.action {
            match action.as_str() {
                "open_url" => {
                    let url = cmd.with.get("url").map(|s| s.as_string()).unwrap_or_default();
                    self.open_url(&render(&url)?)?;
                }
                "open_site" => {
                    let pid = project_id.ok_or_else(|| CoreError::QuickAppError("pick a project first".into()))?;
                    let cfg = self.web_config();
                    let domain = self.domains.lock().unwrap().list().into_iter().find(|d| d.project_id.as_deref() == Some(pid)).ok_or_else(|| CoreError::QuickAppError("this project has no domain yet".into()))?;
                    self.open_url(&self.site_url(&domain, &cfg))?;
                }
                "open_web_config" => {
                    let cfg = self.web_config();
                    let dir = self.paths.web_dir().join(&cfg.server);
                    std::fs::create_dir_all(&dir)?;
                    self.open_path(&dir.display().to_string())?;
                }
                "restart_project" => {
                    let pid = project_id.ok_or_else(|| CoreError::QuickAppError("pick a project first".into()))?;
                    let hosts: Vec<String> = self.domains.lock().unwrap().list().into_iter().filter(|d| d.project_id.as_deref() == Some(pid)).map(|d| d.hostname).collect();
                    for h in hosts {
                        self.web.restart_app(&h);
                    }
                    self.apply_web(&[])?;
                }
                other => return Err(CoreError::QuickAppError(format!("unknown action \"{other}\""))),
            }
            return Ok(None);
        }

        let spec = cmd.command.as_ref().expect("validated");
        let cwd = cmd.working_directory.as_deref().map(render).transpose()?;
        let args = spec.arguments.iter().map(|a| render(a)).collect::<Result<Vec<_>, _>>()?;
        let mut line_parts = vec![spec.executable.clone()];
        line_parts.extend(args.iter().map(|a| if a.contains(' ') { format!("\"{a}\"") } else { a.clone() }));
        let pid = self.spawn_project_process(&spec.executable, &args, cwd.as_deref(), project_id, &format!("Quick Command: {}", cmd.name), &line_parts.join(" "))?;
        Ok(Some(pid))
    }

    pub fn plan_quick_app(&self, id: &str, values: &BTreeMap<String, String>) -> Result<QuickPlanResult, CoreError> {
        let detail = self.catalog.lock().unwrap().get(id)?;
        let ctx = self.plan_ctx();
        let (resolved, errors) = plan::resolve_lenient(&detail.app, values, &ctx);
        // Secrets never leave the core — the wizard only ever sees them masked.
        let masked = |vals: &BTreeMap<String, String>| -> BTreeMap<String, String> {
            vals.iter()
                .map(|(k, v)| {
                    let secret = detail.app.variables.iter().any(|x| &x.name == k && x.is_secret());
                    (k.clone(), if secret && !v.is_empty() { "••••••••".to_string() } else { v.clone() })
                })
                .collect()
        };
        let display = masked(&resolved);
        if !errors.is_empty() {
            return Ok(QuickPlanResult { ok: false, errors, plan: None, requirements: vec![], trusted: detail.view.trusted, source: detail.view.source_label(), values: display });
        }
        let plan = plan::build_plan(&detail.app, resolved, &ctx).map_err(CoreError::QuickAppError)?;
        let requirements = self.requirement_views(&plan);
        Ok(QuickPlanResult { ok: true, errors: vec![], plan: Some(plan), requirements, trusted: detail.view.trusted, source: detail.view.source_label(), values: display })
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
            crate::quickapp::EntrySource::Imported => self.origin.clone().unwrap_or_else(|| "imported".into()),
        }
    }
}

/// `.cmd` / `.bat` shims (npm, npx) can't be launched directly — run them through cmd.exe.
fn shim(file: PathBuf, path_dirs: Vec<PathBuf>) -> ResolvedProgram {
    let is_shim = file.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"));
    if is_shim {
        ResolvedProgram { executable: PathBuf::from("cmd.exe"), pre_args: vec!["/C".into(), file.display().to_string()], env: vec![], path_dirs }
    } else {
        ResolvedProgram { executable: file, pre_args: vec![], env: vec![], path_dirs }
    }
}

/// The Quick App runner's window onto the core.
pub struct Host {
    pub inner: Arc<Inner>,
    /// Set when the run belongs to a known project, so runtimes resolve the way the project does.
    pub project_id: Mutex<Option<String>>,
}

impl QuickHost for Host {
    fn ensure_runtime(&self, id: &str, version: Option<&str>, log: &mut dyn FnMut(&str)) -> Result<String, String> {
        self.inner.quick_ensure_runtime(id, version, log)
    }
    fn resolve_program(&self, program: &str, values: &BTreeMap<String, String>) -> Result<ResolvedProgram, String> {
        let pid = self.project_id.lock().unwrap().clone();
        self.inner.quick_resolve_program(program, values, pid.as_deref())
    }
    fn action(&self, action: &str, with: &BTreeMap<String, String>, ctx: &RunCtx, log: &mut dyn FnMut(&str)) -> Result<ActionOutcome, String> {
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
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).to_string())
}

#[cfg(windows)]
fn set_start_with_windows(enabled: bool) -> Result<(), String> {
    let mut cmd = std::process::Command::new("reg");
    if enabled {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        cmd.args(["add", REG_RUN_KEY, "/v", REG_VALUE, "/t", "REG_SZ", "/d", &format!("\"{}\" --minimized", exe.display()), "/f"]);
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

#[cfg(not(windows))]
fn set_start_with_windows(_enabled: bool) -> Result<(), String> {
    Err("start with the system is only implemented on Windows so far".into())
}

fn site_folder(d: &Domain, projects: &[crate::project::Project]) -> String {
    if let Some(p) = d.project_id.as_ref().and_then(|id| projects.iter().find(|p| &p.id == id)) {
        return p.path.clone();
    }
    let root = Path::new(&d.root);
    let docroot = root.file_name().and_then(|n| n.to_str()).is_some_and(|n| matches!(n.to_lowercase().as_str(), "public" | "web" | "public_html"));
    match root.parent() {
        Some(parent) if docroot => parent.display().to_string(),
        _ => d.root.clone(),
    }
}

const PROJECT_ROOTS: &str = "projects.roots";
const AUTO_SKIP: &str = "domains.auto_skip";

fn has_index(dir: &Path) -> bool {
    ["index.php", "index.html", "index.htm"].iter().any(|f| dir.join(f).is_file())
}

/// How an automatic domain serves a project, or `None` when it needs a dev server
/// (Node/Python) that a plain domain can't start.
fn auto_site(path: &Path) -> Option<(SiteKind, PathBuf)> {
    use crate::detection::Framework as F;
    let detection = crate::detection::detect(path);
    let root = detection.doc_root.as_ref().map(|d| path.join(d)).unwrap_or_else(|| path.to_path_buf());
    match detection.framework {
        F::Laravel | F::Symfony | F::WordPress | F::GenericPhp => Some((SiteKind::Php { version: None }, root)),
        _ if has_index(&root) => Some((SiteKind::Static, root)),
        _ => None,
    }
}
