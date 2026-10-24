use std::path::{Path, PathBuf};

use super::*;
use crate::dbtools;
use crate::domain::Domain;
use crate::error::CoreError;
use crate::health::{HealthReport, HealthTarget};

impl Inner {
    /// Every registered project that is a WordPress install, with the site that serves it
    /// and whether a one-time sign-in link is still waiting to be used.
    ///
    /// This is what decides where the "WP Admin" action appears at all: a folder is checked
    /// on disk, never guessed from its name, and a project with no domain yet is listed
    /// with no URL so the UI can say why the action will refuse.
    pub fn wp_projects(&self) -> Vec<crate::wordpress::WpProject> {
        let projects = self.projects.lock().unwrap().list();
        let cfg = self.web_config();
        let domains = self.domains.lock().unwrap().list();
        projects
            .into_iter()
            .filter_map(|p| {
                let mut wp = crate::wordpress::describe(&p.id, &p.name, Path::new(&p.path))?;
                if let Some(d) = domains
                    .iter()
                    .filter(|d| d.project_id.as_deref() == Some(p.id.as_str()) && d.enabled)
                    .min_by_key(|d| d.hostname.clone())
                {
                    wp.hostname = Some(d.hostname.clone());
                    wp.url = Some(self.site_url(d, &cfg));
                }
                Some(wp)
            })
            .collect()
    }

    /// The site a WordPress project's sign-in link should point at: the hostname asked
    /// for when it belongs to this project, else the project's first enabled site.
    fn wp_site_for(
        &self,
        project: &crate::project::Project,
        hostname: Option<&str>,
    ) -> Result<Domain, CoreError> {
        let domains = self.domains.lock().unwrap().list();
        let mut mine: Vec<&Domain> = domains
            .iter()
            .filter(|d| d.project_id.as_deref() == Some(project.id.as_str()))
            .collect();
        mine.sort_by_key(|d| d.hostname.clone());
        let site = match hostname {
            Some(host) => mine.iter().copied().find(|d| d.hostname == host),
            None => mine.iter().copied().find(|d| d.enabled),
        };
        site.cloned().ok_or_else(|| match hostname {
            Some(host) => CoreError::failed_fix(
                format!("{host} is not a site of this project."),
                format!("{} has no domain called {host} behind it.", project.name),
                "Open the site's own menu to sign in to WordPress, or add a domain for this project first.",
            ),
            None => CoreError::failed_fix(
                format!("{} has no site to open yet.", project.name),
                "A WordPress sign-in link needs a URL, and this project has no domain pointing at it.",
                "Give the project a domain first (Add auto domain in its menu), then sign in.",
            ),
        })
    }

    /// Writes a one-time admin sign-in bridge and returns the link to open (§40's project
    /// menu). No password is read, asked for, or changed: the link carries a random token
    /// that WordPress itself exchanges for a session cookie, once, from this machine.
    pub fn wp_signin(
        &self,
        project_id: &str,
        hostname: Option<&str>,
        theme: Option<&str>,
    ) -> Result<crate::wordpress::WpSignIn, CoreError> {
        let project = self
            .projects
            .lock()
            .unwrap()
            .get(project_id)
            .ok_or_else(|| {
                CoreError::failed_fix(
                    "This project is not on the list any more.",
                    format!("No project is registered under the id {project_id}."),
                    "Reload the Sites page and pick the project again.",
                )
            })?;
        if !self.web.is_running() {
            return Err(CoreError::failed_fix(
                "The web server is not running.",
                "A WordPress sign-in link is opened in the browser, and the browser needs the site to answer first.",
                "Start the web server on the Web server page, then sign in again.",
            ));
        }
        let site = self.wp_site_for(&project, hostname)?;
        let cfg = self.web_config();
        let signin = crate::wordpress::issue(
            Path::new(&project.path),
            &self.site_url(&site, &cfg),
            None,
            theme,
        )?;
        // The URL carries the token, so it is deliberately not logged.
        tracing::info!(
            project = %project.name,
            hostname = %site.hostname,
            expires_at = %signin.expires_at,
            "issued a one-time WordPress admin sign-in"
        );
        Ok(signin)
    }

    /// Cancels a pending one-time sign-in link. The file is the whole capability, so
    /// removing it is the whole cancellation.
    pub fn wp_signin_revoke(&self, project_id: &str) -> Result<usize, CoreError> {
        let project = self
            .projects
            .lock()
            .unwrap()
            .get(project_id)
            .ok_or_else(|| CoreError::InvalidProjectPath(project_id.to_string()))?;
        crate::wordpress::revoke(Path::new(&project.path))
    }

    /// The skipped project folders, read once so a scan can compare many paths without
    /// re-locking settings for each one.
    pub(crate) fn project_skips(&self) -> Vec<SkipEntry> {
        self.skip_entries(PROJECT_SKIP)
    }

    /// Explicitly adding a folder again clears a previous removal.
    pub(crate) fn clear_project_skip(&self, path: &str) -> Result<(), CoreError> {
        self.remove_skip(PROJECT_SKIP, path)
    }

    /// Laragon-style automatic domains (setting `domains.auto`, on by default): every
    /// folder in a remembered projects folder becomes a project, and every project that
    /// can be served (PHP or plain HTML) gets `<folder>.<tld>` over HTTPS where `<tld>` is
    /// read from `domains.default_tld` (default: `local`). Domains the user deleted stay
    /// deleted. Applies the web config when something was added and the server is running.
    /// Returns how many domains were created.

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
        // Read first: `web_config` takes the same settings lock, so it cannot be called
        // while the guard below is held.
        let default_web = self.web_config().default_server.clone();
        let s = self.settings.lock().unwrap();
        let b = |k: &str, d: bool| s.get(k).and_then(|v| v.as_bool()).unwrap_or(d);
        let mut services: Vec<String> = s
            .get("startup.services")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        // The old "start the web server with the app" switch is gone (§121). A user who
        // had it on meant "my sites answer after a restart", so that intent is carried
        // over once: the default web server joins the autostart list, which now applies
        // the config before starting it.
        if b("startup.web", false)
            && !services
                .iter()
                .any(|id| crate::web::SERVER_IDS.contains(&id.as_str()))
        {
            services.push(default_web);
        }
        StartupSettings {
            with_windows: registry_run_value().is_some(),
            start_minimized: b("startup.minimized", true),
            autostart_services: services,
            notifications: b("notifications.enabled", true),
            close_to_tray: b("startup.close_to_tray", true),
        }
    }

    pub fn set_startup_settings(&self, new: StartupSettings) -> Result<(), CoreError> {
        {
            let mut s = self.settings.lock().unwrap();
            s.set("startup.minimized", serde_json::json!(new.start_minimized))?;
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
        // A web server in the autostart list is applied, not just spawned: rendering and
        // validating its config is what makes the user's sites answer. `ServiceManager::start`
        // on a web server id only launches the process against whatever config is on disk,
        // which is what the removed "start the web server" switch used to do separately.
        let mut web_applied = false;
        for id in &startup.autostart_services {
            let result: Result<(), String> = if crate::web::SERVER_IDS.contains(&id.as_str()) {
                self.apply_web_server(id, &[])
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            } else {
                self.services.start(id).map(|_| ())
            };
            if let Err(e) = result {
                tracing::warn!(service = %id, error = %e, "autostart: service did not start");
            }
            if crate::web::SERVER_IDS.contains(&id.as_str()) {
                web_applied = true;
            }
        }
        let tunnels: Vec<_> = self
            .list_tunnels()
            .into_iter()
            .filter(|t| t.config.autostart && t.config.acknowledged)
            .collect();
        if !tunnels.is_empty() && !web_applied {
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

        let mut places = vec![("OLS data".to_string(), self.paths.root().to_path_buf())];
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
            name: "OLS".into(),
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
            // with the process, and an error here would look like a broken page. A
            // Start that never spawned is the exception — there is no buffer to read,
            // so the recorded reason is the whole story.
            s if s.starts_with("service:") => {
                let id = &s[8..];
                let lines = self
                    .services
                    .process_id(id)
                    .map(|process_id| tail(self.supervisor.recent_output(process_id)))
                    .unwrap_or_default();
                if !lines.is_empty() {
                    return Ok(lines);
                }
                Ok(self
                    .services
                    .last_start_failure(id)
                    .map(|reason| vec![format!("last start attempt failed: {reason}")])
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

/// The executable the startup entry names, or `None` when there is no entry.
///
/// `reg query` prints a table, not a bare value, so the path is the tail of the value
/// row: everything after the type column. Anything that does not look like a path
/// returns `None` rather than a guess — a startup entry this app did not write is not
/// ours to interpret, and a wrong guess here would rewrite a working entry.
#[cfg(windows)]
fn registry_run_target() -> Option<String> {
    parse_run_target(&registry_run_value()?)
}

/// Pulls the executable path out of a `reg query` value row.
///
/// Windows paths contain spaces, so the value is written quoted; but whether the quote
/// characters survive into the stored value depends on how `reg add` was given the
/// argument, so both spellings are accepted. Splitting on whitespace instead would
/// truncate `C:\Program Files\Open Local Server\…` to `C:\Program`.
pub(crate) fn parse_run_target(blob: &str) -> Option<String> {
    let line = blob.lines().find(|l| l.contains(REG_VALUE))?;
    let row = line.split_once(REG_VALUE)?.1.trim();
    if let Some(quoted) = row
        .split_once('"')
        .and_then(|(_, rest)| rest.split_once('"'))
    {
        return (!quoted.0.is_empty()).then(|| quoted.0.to_string());
    }
    // Unquoted: everything after the type column is the value, spaces included.
    let value = row.strip_prefix("REG_SZ")?.trim();
    (!value.is_empty()).then(|| value.to_string())
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

/// Rewrites the startup entry when it is stale. Two kinds of staleness, both silent
/// failures the user would otherwise only notice as "it never starts by itself".
///
/// 1. An entry written by an earlier build carries `--minimized`, which would hide the
///    window even with the preference off — the opposite of what the user then chose.
/// 2. An entry whose executable no longer exists, because the install folder was renamed
///    or moved. `set_start_with_windows` bakes the absolute exe path into the value, so
///    the entry survives a move as a valid-looking command pointing at nothing. The
///    rewrite points it at the copy that is actually running, which is the only one the
///    user can have meant.
///
/// A missing entry is left alone, so a build without autostart is not turned on behind
/// the user's back. Failures are logged, never fatal: a bad registry value must not stop
/// the app from starting.
pub fn migrate_startup_entry() {
    #[cfg(windows)]
    {
        let Some(current) = registry_run_value() else {
            return;
        };
        let dead = registry_run_target().filter(|target| !Path::new(target).exists());
        if !current.contains("--minimized") && dead.is_none() {
            return;
        }
        if let Some(target) = dead.as_deref() {
            tracing::info!(
                target,
                "the startup entry points at an executable that is gone; repointing it"
            );
        }
        match set_start_with_windows(true) {
            Ok(()) => tracing::info!("rewrote the startup entry to the running executable"),
            Err(e) => tracing::warn!(error = %e, "could not rewrite the startup entry"),
        }
    }
}

#[cfg(not(windows))]
fn set_start_with_windows(_enabled: bool) -> Result<(), String> {
    Err("start with the system is only implemented on Windows so far".into())
}
