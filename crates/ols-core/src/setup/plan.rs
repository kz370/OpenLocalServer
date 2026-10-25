use std::path::{Path, PathBuf};

use super::*;
use crate::app::Inner;
use crate::error::CoreError;
use crate::manifest;

impl Inner {
    pub fn plan_setup(&self, project_id: &str) -> Result<EnvironmentPlan, CoreError> {
        let detail = self
            .project_detail(project_id)
            .ok_or_else(|| CoreError::InvalidProjectPath(project_id.to_string()))?;
        let path = PathBuf::from(&detail.project.path);
        let mut conflicts = Vec::new();
        let (mut manifest, manifest_found) = match manifest::read_manifest(&path) {
            Ok(Some(m)) => (m, true),
            Ok(None) => (self.derive_manifest(project_id)?, false),
            Err(e) => {
                return Err(CoreError::EnvError(format!(
                    "the manifest could not be read: {e}"
                )))
            }
        };
        if manifest.workers.is_empty() {
            if let Some((_, text)) = crate::procfile::project_file(&path) {
                match crate::procfile::parse(&text) {
                    Ok(entries) => {
                        for entry in entries.into_iter().filter(|e| e.name != "web") {
                            manifest.workers.insert(
                                entry.name,
                                WorkerEntry::Custom(crate::manifest::WorkerManifest {
                                    command: entry.command,
                                    count: 1,
                                    timeout_secs: None,
                                    memory_mb: None,
                                }),
                            );
                        }
                    }
                    Err(e) => conflicts.push(Conflict {
                        kind: "manifest".into(),
                        blocking: false,
                        message: format!("Procfile was not imported: {e}"),
                        resolution: "Correct its process lines, then plan setup again.".into(),
                    }),
                }
            }
        }
        let (lock, lock_found) = match manifest::read_lock(&path) {
            Ok(Some(l)) => (l, true),
            Ok(None) => (LockFile::new(), false),
            Err(e) => {
                conflicts.push(Conflict {
                    kind: "manifest".into(),
                    blocking: false,
                    message: format!("environment.lock could not be read: {e}"),
                    resolution: "It is ignored and rewritten after a successful setup.".into(),
                });
                (LockFile::new(), false)
            }
        };
        let commands = manifest::read_commands(&path).unwrap_or_else(|e| {
            conflicts.push(Conflict {
                kind: "manifest".into(),
                blocking: true,
                message: format!("commands.yaml could not be read: {e}"),
                resolution: "Fix the file, then plan again.".into(),
            });
            Vec::new()
        });

        let mut p = Planner {
            inner: self,
            steps: Vec::new(),
            conflicts,
            lock: &lock,
        };
        let project_name = manifest
            .name
            .clone()
            .unwrap_or_else(|| detail.project.name.clone());

        // Runtimes (§74: runtime requirements).
        let mut php_version = None;
        for (id, wanted) in [
            ("php", manifest.runtime.php.clone()),
            ("node", manifest.runtime.node.clone()),
        ] {
            if let Some(v) = p.runtime(id, wanted.as_deref()) {
                if id == "php" {
                    php_version = Some(v);
                }
            }
        }
        if let Some(wanted) = &manifest.runtime.python {
            match crate::runtime::detect_system_install("python") {
                Some(sys) if sys.version.starts_with(wanted.as_str()) => p.done("install", format!("Python {}", sys.version), SetupAction::WriteLock),
                Some(sys) => p.conflict("runtime", false, format!("Python {wanted} is requested; this computer has Python {}.", sys.version), "Install that Python from python.org, or change runtime.python in the manifest."),
                None => p.conflict("runtime", true, format!("Python {wanted} is needed and Python was not found on PATH."), "Install Python from python.org (tick \"Add to PATH\"), then plan again."),
            }
        }

        // Extensions.
        if !manifest.extensions.is_empty() {
            match &php_version {
                Some(v) if self.runtimes.installed_versions("php").contains(v) => {
                    let report = self.php.extensions_report(v);
                    for name in &manifest.extensions {
                        match report.extensions.iter().find(|e| e.name.eq_ignore_ascii_case(name)) {
                            Some(e) if e.enabled => p.done("configure", format!("PHP extension {name}"), SetupAction::EnableExtension { php_version: v.clone(), name: name.to_string() }),
                            Some(_) => p.step("configure", format!("Enable PHP extension {name}"), SetupAction::EnableExtension { php_version: v.clone(), name: name.to_string() }),
                            None => p.conflict("runtime", false, format!("PHP {v} has no {name} extension."), "Install it from the Runtimes page (PHP extensions), then plan again."),
                        }
                    }
                }
                Some(v) => {
                    for name in &manifest.extensions {
                        p.step(
                            "configure",
                            format!("Enable PHP extension {name}"),
                            SetupAction::EnableExtension {
                                php_version: v.clone(),
                                name: name.clone(),
                            },
                        );
                    }
                }
                None => p.conflict(
                    "runtime",
                    false,
                    "PHP extensions are listed but the project has no PHP.".to_string(),
                    "Add runtime.php to the manifest.",
                ),
            }
        }

        // Package manager.
        if let Some(pm) = manifest
            .package_manager
            .as_deref()
            .filter(|pm| *pm != "npm")
        {
            let node_dir = detail
                .resolved
                .iter()
                .find(|r| r.id == "node")
                .and_then(|r| r.bin_dir.clone())
                .map(PathBuf::from);
            let info = crate::nodepm::info(&path, node_dir.as_deref());
            let ready = match pm {
                "pnpm" => info.pnpm,
                "yarn" => info.yarn,
                _ => false,
            };
            if ready {
                p.done(
                    "configure",
                    format!("{pm} package manager"),
                    SetupAction::EnablePackageManager { manager: pm.into() },
                );
            } else {
                p.step(
                    "configure",
                    format!("Switch on {pm} through corepack"),
                    SetupAction::EnablePackageManager { manager: pm.into() },
                );
            }
        }

        // Web server (every site shares the one the app is set to).
        let cfg = self.web_config();
        if let Some(want) = manifest.web.as_ref().and_then(|w| w.server.clone()) {
            if want != cfg.default_server {
                p.conflict("web_server", false, format!("The manifest asks for {want}; this site is served by {}.", cfg.default_server), "Pin the site's web server in its settings, or switch the default in the Sites settings.");
            }
        }

        // Database (§74: database requirements).
        let db_name = manifest
            .database
            .as_ref()
            .map(|d| d.name.clone().unwrap_or_else(|| db_name_for(&project_name)));
        if let (Some(db), Some(name)) = (&manifest.database, &db_name) {
            if db.engine.eq_ignore_ascii_case("sqlite") {
                let file = if name.ends_with(".sqlite") || name.contains('/') {
                    name.clone()
                } else {
                    "database/database.sqlite".into()
                };
                match crate::quickapp::plan::safe_join(&detail.project.path, &file) {
                    Ok(full) if Path::new(&full).is_file() => p.done(
                        "create",
                        format!("SQLite database {file}"),
                        SetupAction::CreateSqlite { path: full },
                    ),
                    Ok(full) => p.step(
                        "create",
                        format!("Create SQLite database {file}"),
                        SetupAction::CreateSqlite { path: full },
                    ),
                    Err(e) => p.conflict(
                        "database",
                        true,
                        format!("The SQLite path {file} is not allowed: {e}"),
                        "Use a path inside the project.",
                    ),
                }
            } else if let Some(service) = db_service(&db.engine) {
                if db.engine.eq_ignore_ascii_case("mysql") {
                    p.note(format!(
                        "MySQL databases are served by MariaDB here (compatible for {} use).",
                        project_name
                    ));
                }
                if let Some(v) = &db.version {
                    let have: Vec<String> = crate::catalog::builtin_catalog()
                        .iter()
                        .filter(|m| m.id == service)
                        .map(|m| m.version.to_string())
                        .collect();
                    if db.engine.eq_ignore_ascii_case(service)
                        && !have.iter().any(|h| h.starts_with(v.as_str()))
                    {
                        p.conflict("database", false, format!("{} {v} is requested; the available version is {}.", db.engine, have.join(", ")), "The available version is used. Change database.version if that's fine.");
                    }
                }
                p.service(service);
                if service == "mongodb" {
                    p.done(
                        "create",
                        format!("MongoDB database {name} (created on first write)"),
                        SetupAction::CreateDatabase {
                            engine: service.into(),
                            name: name.clone(),
                        },
                    );
                } else if !crate::service::is_safe_identifier(name) {
                    p.conflict(
                        "database",
                        true,
                        format!("\"{name}\" is not a usable database name."),
                        "Use letters, digits and underscores in database.name.",
                    );
                } else {
                    let exists = self.services.is_running(service)
                        && self
                            .services
                            .list_databases(service)
                            .map(|l| l.iter().any(|d| d == name))
                            .unwrap_or(false);
                    let action = SetupAction::CreateDatabase {
                        engine: service.into(),
                        name: name.clone(),
                    };
                    if exists {
                        p.done(
                            "create",
                            format!(
                                "{} database \"{name}\"",
                                self.runtimes.display_name(service).unwrap_or_default()
                            ),
                            action,
                        );
                    } else {
                        p.step(
                            "create",
                            format!(
                                "Create {} database \"{name}\"",
                                self.runtimes.display_name(service).unwrap_or_default()
                            ),
                            action,
                        );
                    }
                }
            } else {
                p.conflict(
                    "database",
                    true,
                    format!("Unknown database engine \"{}\".", db.engine),
                    "Use mariadb, mysql, postgres, mongodb or sqlite.",
                );
            }
        }

        // Services.
        for id in manifest.enabled_services() {
            let id = db_service(&id).unwrap_or(id.as_str()).to_string();
            if crate::custom_service::is_custom_id(&id)
                || self.services.list().iter().any(|s| s.id == id)
            {
                p.service(&id);
            } else {
                p.conflict("service", true, format!("Unknown service \"{id}\"."), "Use redis, memcached, mailpit, mariadb, postgres or mongodb, or add it as a custom service.");
            }
        }

        // Domain → DNS → SSL.
        let mut hostname = None;
        if let Some(dm) = &manifest.domain {
            hostname = p.domain(
                project_id,
                &path,
                &detail.detection,
                dm,
                php_version.as_deref(),
            );
        }

        // Mail: point the project's .env at Mailpit when it uses mail and Mailpit is wanted.
        if manifest.enabled_services().iter().any(|s| s == "mailpit") && path.join(".env").is_file()
        {
            match self.mailpit_env_plan(project_id, ".env") {
                Ok(plan) if plan.up_to_date || plan.changes.is_empty() => p.done(
                    "configure",
                    "Mail goes to Mailpit".to_string(),
                    SetupAction::ConfigureMail {
                        file: ".env".into(),
                    },
                ),
                Ok(plan) => p.step(
                    "configure",
                    format!(
                        "Point .env mail settings at Mailpit ({} value(s))",
                        plan.changes.len()
                    ),
                    SetupAction::ConfigureMail {
                        file: ".env".into(),
                    },
                ),
                Err(_) => {}
            }
        }

        // Project Quick Commands.
        if !commands.is_empty() {
            let known = self.quick_commands.list();
            let new: Vec<QuickCommand> = commands
                .into_iter()
                .filter(|c| {
                    !known
                        .iter()
                        .any(|k| k.id == c.id && k.command == c.command && k.action == c.action)
                })
                .collect();
            let clashes: Vec<String> = new
                .iter()
                .filter(|c| known.iter().any(|k| k.id == c.id && k.builtin))
                .map(|c| c.id.clone())
                .collect();
            if !clashes.is_empty() {
                p.conflict(
                    "file_ownership",
                    true,
                    format!(
                        "commands.yaml redefines built-in Quick Commands: {}.",
                        clashes.join(", ")
                    ),
                    "Give those commands other ids.",
                );
            } else if new.is_empty() {
                p.done(
                    "configure",
                    "Project Quick Commands".to_string(),
                    SetupAction::ImportCommands { commands: vec![] },
                );
            } else {
                let label = format!("Add {} Quick Command(s) from commands.yaml", new.len());
                p.step(
                    "configure",
                    label,
                    SetupAction::ImportCommands { commands: new },
                );
            }
        }

        // Workers (§105) and the scheduler (§106).
        p.workers(project_id, &detail.detection.framework, &manifest);

        // Web server config, then the things that need it.
        let web_running = self.web.is_running();
        p.step(
            "start",
            format!(
                "Apply the web config and start {}",
                crate::web::server_by_id(cfg.server())
                    .map(|s| s.name())
                    .unwrap_or("the web server")
            ),
            SetupAction::ApplyWeb,
        );
        if !web_running && hostname.is_none() {
            p.steps.pop();
        }
        if p.steps
            .iter()
            .any(|s| matches!(s.action, SetupAction::AddWorker { .. }))
            || !self.workers_for(project_id).is_empty()
        {
            p.step(
                "start",
                "Start the project's queue workers".to_string(),
                SetupAction::StartWorkers,
            );
        }

        // Tunnel (§73.12: only when explicitly configured).
        if let Some(t) = manifest.tunnel.as_ref().filter(|t| t.enabled) {
            let provider = t.provider.clone().unwrap_or_else(|| "cloudflare".into());
            let target = t.target.clone().or_else(|| {
                hostname.as_ref().map(|h| {
                    format!(
                        "{}://{h}",
                        if manifest.domain.as_ref().is_some_and(|d| d.https) {
                            "https"
                        } else {
                            "http"
                        }
                    )
                })
            });
            match (target, t.autostart) {
                (Some(target), true) => p.step("tunnel", format!("Start a public {provider} tunnel to {target}"), SetupAction::StartTunnel { provider, target }),
                (Some(target), false) => p.note(format!("A {provider} tunnel to {target} is configured. It is not started automatically; start it from the Tunnels page.")),
                (None, _) => p.conflict("manifest", false, "A tunnel is enabled but has no target and the project has no site.".to_string(), "Add tunnel.target."),
            }
        }

        if let Some(h) = &hostname {
            let scheme = if manifest.domain.as_ref().is_some_and(|d| d.https) {
                "https"
            } else {
                "http"
            };
            p.step(
                "check",
                format!("Check that {scheme}://{h} answers"),
                SetupAction::HealthCheck {
                    hostname: h.clone(),
                },
            );
        }
        p.step(
            "check",
            "Write .openlocalserver/environment.lock".to_string(),
            SetupAction::WriteLock,
        );

        let ok = !p.conflicts.iter().any(|c| c.blocking);
        let Planner {
            steps, conflicts, ..
        } = p;
        // Placeholder rows (used only to show Python as present) aren't real steps.
        let steps = steps
            .into_iter()
            .filter(|s| !(s.done && matches!(s.action, SetupAction::WriteLock)))
            .collect();
        Ok(EnvironmentPlan {
            project_id: project_id.to_string(),
            project_name,
            project_path: detail.project.path.clone(),
            manifest_found,
            manifest,
            lock_found,
            steps,
            conflicts,
            ok,
        })
    }
}
