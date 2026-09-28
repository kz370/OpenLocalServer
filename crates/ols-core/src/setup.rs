//! Reproducible environments (§73–78, §159): `ols setup` and the Environment tab.
//!
//! 1. **Resolve** (§74): the project's manifest (or, without one, what detection finds) is
//!    walked in the SRS order: runtimes → extensions → package manager → database →
//!    services → domain → DNS → SSL → mail → workers → scheduler → tunnel.
//! 2. **Plan** (§76): each requirement becomes a step, already-satisfied ones marked done.
//!    Anything that can't be done safely is a [`Conflict`] (§75) with a proposed resolution;
//!    blocking conflicts stop the plan from being applied. Nothing unrelated is ever killed.
//! 3. **Dry run** (§77) is the plan alone.
//! 4. **Apply** (§78) runs the steps in order inside the operation journal. On a failure the
//!    safe changes made so far are undone in reverse (sites added, services started,
//!    extensions switched on, workers registered); installs and databases are kept, since
//!    removing them could lose data or cost a long download for nothing.
//! 5. A successful apply writes `.openlocalserver/environment.lock` (§72) with the exact versions.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::app::Inner;
use crate::detection::Framework;
use crate::domain::{AppSpec, Domain, Ownership, SiteKind};
use crate::error::CoreError;
use crate::manifest::{
    self, DatabaseManifest, DomainManifest, EnvironmentManifest, LockFile, SchedulerEntry,
    ServiceToggle, WorkerEntry,
};
use crate::quickapp::commands::QuickCommand;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SetupAction {
    InstallRuntime {
        id: String,
        version: String,
    },
    EnableExtension {
        php_version: String,
        name: String,
    },
    EnablePackageManager {
        manager: String,
    },
    StartService {
        id: String,
    },
    CreateDatabase {
        engine: String,
        name: String,
    },
    CreateSqlite {
        path: String,
    },
    AddDomain {
        domain: Box<Domain>,
    },
    UpdateDomain {
        domain: Box<Domain>,
    },
    SyncHosts,
    TrustCa,
    ConfigureMail {
        file: String,
    },
    ImportCommands {
        commands: Vec<QuickCommand>,
    },
    AddWorker {
        worker: Box<crate::workers::Worker>,
    },
    AddSchedule {
        task: Box<crate::scheduler::ScheduledTask>,
    },
    ApplyWeb,
    StartWorkers,
    /// Only planned when the manifest asks for `tunnel.autostart` (§73.12).
    StartTunnel {
        provider: String,
        target: String,
    },
    HealthCheck {
        hostname: String,
    },
    WriteLock,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    /// The §76 heading: install, create, configure, start, tunnel, check.
    pub group: String,
    pub label: String,
    pub action: SetupAction,
    /// Already in place: shown, not run.
    pub done: bool,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conflict {
    /// port, runtime, database, domain, certificate, service, file_ownership, web_server, manifest.
    pub kind: String,
    /// A blocking conflict stops the plan from being applied.
    pub blocking: bool,
    pub message: String,
    /// The safe way out, in words (§75: never "kill whatever is on the port").
    pub resolution: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentPlan {
    pub project_id: String,
    pub project_name: String,
    pub project_path: String,
    /// `.openlocalserver/environment.yaml` exists; otherwise the manifest below was derived from
    /// what detection found and can be saved as a starting point.
    pub manifest_found: bool,
    pub manifest: EnvironmentManifest,
    pub lock_found: bool,
    pub steps: Vec<PlanStep>,
    pub conflicts: Vec<Conflict>,
    /// No blocking conflicts.
    pub ok: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    Running,
    Done,
    /// Already in place.
    Skipped,
    Failed,
    RolledBack,
    /// Never reached because an earlier step failed.
    NotRun,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    pub group: String,
    pub label: String,
    pub status: StepStatus,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SetupReport {
    pub project_id: String,
    pub running: bool,
    pub dry_run: bool,
    pub ok: bool,
    pub steps: Vec<StepResult>,
    pub conflicts: Vec<Conflict>,
    /// What was undone after a failure.
    pub rolled_back: Vec<String>,
    pub lock_written: Option<String>,
    pub health: Option<crate::health::HealthReport>,
    pub error: Option<String>,
}

/// What a successful step changed, so it can be undone if a later one fails.
enum Undo {
    RemoveDomain(String),
    RestoreDomain(Box<Domain>),
    StopService(String),
    DisableExtension(String, String),
    DeleteCommand(String),
    RemoveWorker(String),
    RemoveSchedule(String),
    StopWorkers(String),
}

const DB_SERVICES: &[(&str, &str)] = &[
    ("mariadb", "mariadb"),
    ("mysql", "mariadb"),
    ("postgres", "postgres"),
    ("postgresql", "postgres"),
    ("mongodb", "mongodb"),
    ("mongo", "mongodb"),
];

fn db_service(engine: &str) -> Option<&'static str> {
    DB_SERVICES
        .iter()
        .find(|(e, _)| e.eq_ignore_ascii_case(engine))
        .map(|(_, s)| *s)
}

fn is_php(f: &Framework) -> bool {
    matches!(
        f,
        Framework::Laravel | Framework::Symfony | Framework::WordPress | Framework::GenericPhp
    )
}

/// Lowercase letters, digits and underscores: what a database name may be.
pub fn db_name_for(name: &str) -> String {
    let mut out: String = name
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    while out.contains("__") {
        out = out.replace("__", "_");
    }
    let out = out.trim_matches('_').to_string();
    if out.is_empty() {
        "app".into()
    } else {
        out.chars().take(64).collect()
    }
}

/// Reads one `KEY=value` from a project's `.env`, if it has one.
fn env_value(project: &Path, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(project.join(".env")).ok()?;
    text.lines().find_map(|l| {
        let (k, v) = l.trim().split_once('=')?;
        (k.trim() == key).then(|| v.trim().trim_matches('"').trim_matches('\'').to_string())
    })
}

/// The Environment tab's view of a project's manifest files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestInfo {
    pub path: String,
    pub found: bool,
    /// The raw YAML, for the editor.
    pub text: Option<String>,
    pub manifest: Option<EnvironmentManifest>,
    /// Why the file doesn't parse, if it doesn't.
    pub error: Option<String>,
    /// What detection suggests; the starting point when there is no manifest.
    pub derived: EnvironmentManifest,
    pub lock: Option<LockFile>,
    pub has_commands: bool,
    pub has_services: bool,
}

impl Inner {
    pub fn manifest_info(&self, project_id: &str) -> Result<ManifestInfo, CoreError> {
        let project = self
            .projects
            .lock()
            .unwrap()
            .get(project_id)
            .ok_or_else(|| CoreError::InvalidProjectPath(project_id.to_string()))?;
        let root = PathBuf::from(&project.path);
        let path = manifest::manifest_path(&root);
        let text = std::fs::read_to_string(&path).ok();
        let (manifest, error) = match manifest::read_manifest(&root) {
            Ok(m) => (m, None),
            Err(e) => (None, Some(e)),
        };
        Ok(ManifestInfo {
            path: path.display().to_string(),
            found: text.is_some(),
            text,
            manifest,
            error,
            derived: self.derive_manifest(project_id)?,
            lock: manifest::read_lock(&root).ok().flatten(),
            has_commands: manifest::dir(&root).join("commands.yaml").is_file(),
            has_services: manifest::dir(&root).join("services.yaml").is_file(),
        })
    }

    pub fn save_manifest_text(&self, project_id: &str, text: &str) -> Result<String, CoreError> {
        serde_yaml_ng::from_str::<EnvironmentManifest>(text)
            .map_err(|e| CoreError::EnvError(format!("the manifest doesn't read as YAML: {e}")))?;
        let project = self
            .projects
            .lock()
            .unwrap()
            .get(project_id)
            .ok_or_else(|| CoreError::InvalidProjectPath(project_id.to_string()))?;
        let path = manifest::manifest_path(Path::new(&project.path));
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(&path, text)?;
        Ok(path.display().to_string())
    }

    /// Without a manifest: what detection says the project needs, as a manifest the user
    /// can save and edit.
    pub fn derive_manifest(&self, project_id: &str) -> Result<EnvironmentManifest, CoreError> {
        let detail = self
            .project_detail(project_id)
            .ok_or_else(|| CoreError::InvalidProjectPath(project_id.to_string()))?;
        let path = PathBuf::from(&detail.project.path);
        let det = &detail.detection;
        let mut m = EnvironmentManifest {
            name: Some(detail.project.name.clone()),
            ..Default::default()
        };
        m.runtime.php = det.requirements.php.clone();
        m.runtime.node = det.requirements.node.clone();
        m.runtime.python = det.requirements.python.clone();
        if is_php(&det.framework) && m.runtime.php.is_none() {
            m.runtime.php = self
                .php
                .pick_version(None)
                .map(|v| v.split('.').take(2).collect::<Vec<_>>().join("."));
        }

        // A site: PHP and static projects are served directly; others need their own port.
        let existing = self
            .domains
            .lock()
            .unwrap()
            .list()
            .into_iter()
            .find(|d| d.project_id.as_deref() == Some(project_id));
        let servable = is_php(&det.framework) || path.join("index.html").is_file();
        if let Some(d) = existing {
            m.domain = Some(DomainManifest {
                hostname: d.hostname.clone(),
                https: d.https,
                wildcard: d.wildcard,
                root: Path::new(&d.root)
                    .strip_prefix(&path)
                    .ok()
                    .map(|r| r.display().to_string().replace('\\', "/"))
                    .filter(|r| !r.is_empty()),
                port: match d.kind {
                    SiteKind::Proxy {
                        upstream_port,
                        upstream_host: None,
                        ..
                    } => Some(upstream_port),
                    _ => None,
                },
            });
        } else if servable {
            let default_tld = self.setting_string("domains.default_tld", "local");
            m.domain = Some(DomainManifest {
                hostname: format!(
                    "{}.{default_tld}",
                    crate::domain::slugify(&detail.project.name)
                ),
                https: true,
                wildcard: false,
                root: det.doc_root.clone(),
                port: None,
            });
        }

        // The database the project's .env already points at.
        if let Some(conn) = env_value(&path, "DB_CONNECTION") {
            let engine = match conn.as_str() {
                "mysql" | "mariadb" => Some("mariadb"),
                "pgsql" | "postgres" | "postgresql" => Some("postgres"),
                "sqlite" => Some("sqlite"),
                "mongodb" => Some("mongodb"),
                _ => None,
            };
            if let Some(engine) = engine {
                let name = env_value(&path, "DB_DATABASE").filter(|n| !n.is_empty());
                m.database = Some(DatabaseManifest {
                    engine: engine.into(),
                    version: None,
                    name,
                });
            }
            if env_value(&path, "REDIS_HOST").is_some()
                && ["redis"].iter().any(|k| {
                    env_value(&path, "CACHE_STORE")
                        .or_else(|| env_value(&path, "CACHE_DRIVER"))
                        .as_deref()
                        == Some(k)
                        || env_value(&path, "QUEUE_CONNECTION").as_deref() == Some(k)
                })
            {
                m.services.insert("redis".into(), ServiceToggle::On(true));
            }
            if env_value(&path, "MAIL_MAILER").is_some() {
                m.services.insert("mailpit".into(), ServiceToggle::On(true));
            }
        }
        if det.framework == Framework::Laravel {
            if env_value(&path, "QUEUE_CONNECTION").is_some_and(|q| q != "sync") {
                m.workers.insert("queue".into(), WorkerEntry::On(true));
            }
            m.scheduler = Some(SchedulerEntry::On(true));
        }
        if let Some(pm) = crate::nodepm::info(&path, None).detected {
            if pm != "npm" {
                m.package_manager = Some(pm);
            }
        }
        Ok(m)
    }

    /// Writes a manifest (the derived one when `manifest` is `None`) into the project.
    pub fn save_manifest(
        &self,
        project_id: &str,
        manifest: Option<EnvironmentManifest>,
    ) -> Result<String, CoreError> {
        let project = self
            .projects
            .lock()
            .unwrap()
            .get(project_id)
            .ok_or_else(|| CoreError::InvalidProjectPath(project_id.to_string()))?;
        let m = match manifest {
            Some(m) => m,
            None => self.derive_manifest(project_id)?,
        };
        let path =
            manifest::write_manifest(Path::new(&project.path), &m).map_err(CoreError::EnvError)?;
        Ok(path.display().to_string())
    }

    /// §74–76: the plan for bringing this project's environment up.
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
                            Some(e) if e.enabled => p.done("configure", format!("PHP extension {name}"), SetupAction::EnableExtension { php_version: v.clone(), name: name.clone() }),
                            Some(_) => p.step("configure", format!("Enable PHP extension {name}"), SetupAction::EnableExtension { php_version: v.clone(), name: name.clone() }),
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
                p.conflict("service", true, format!("Unknown service \"{id}\"."), "Use redis, mailpit, mariadb, postgres or mongodb, or add it as a custom service.");
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

    pub fn setup_progress(&self) -> Option<SetupReport> {
        self.setup.lock().unwrap().clone()
    }

    /// §73: runs the plan. With `dry_run` nothing changes and the report lists what would.
    pub fn apply_setup(&self, project_id: &str, dry_run: bool) -> Result<SetupReport, CoreError> {
        let plan = self.plan_setup(project_id)?;
        let mut report = SetupReport {
            project_id: project_id.to_string(),
            running: !dry_run,
            dry_run,
            ok: plan.ok,
            conflicts: plan.conflicts.clone(),
            steps: plan
                .steps
                .iter()
                .map(|s| StepResult {
                    group: s.group.clone(),
                    label: s.label.clone(),
                    status: if s.done {
                        StepStatus::Skipped
                    } else {
                        StepStatus::Pending
                    },
                    detail: s.note.clone(),
                })
                .collect(),
            ..Default::default()
        };
        if dry_run {
            report.running = false;
            return Ok(report);
        }
        if !plan.ok {
            report.running = false;
            report.error = Some("The plan has blocking conflicts. Resolve them first.".into());
            return Ok(report);
        }
        *self.setup.lock().unwrap() = Some(report.clone());

        let title = format!("Set up {}", plan.project_name);
        let retry = crate::command::CoreCommand::ApplySetup {
            project_id: project_id.to_string(),
            dry_run: false,
        };
        let result = self.journaled(
            "setup",
            &title,
            Some("Failed setups undo their safe changes automatically."),
            Some(retry),
            || {
                self.run_plan(&plan, &mut report);
                match &report.error {
                    Some(e) => Err(CoreError::EnvError(e.clone())),
                    None => Ok(()),
                }
            },
        );
        report.running = false;
        report.ok = result.is_ok();
        *self.setup.lock().unwrap() = Some(report.clone());
        Ok(report)
    }

    fn run_plan(&self, plan: &EnvironmentPlan, report: &mut SetupReport) {
        let mut undo: Vec<Undo> = Vec::new();
        let mut installed: LockFile = LockFile::new();
        let path = PathBuf::from(&plan.project_path);
        for (i, step) in plan.steps.iter().enumerate() {
            if step.done {
                continue;
            }
            self.set_step(report, i, StepStatus::Running, None);
            let mut log_lines: Vec<String> = Vec::new();
            let mut log = |l: &str| log_lines.push(l.to_string());
            let outcome = self.run_action(
                plan,
                &step.action,
                &mut undo,
                &mut installed,
                &path,
                report,
                &mut log,
            );
            match outcome {
                Ok(detail) => self.set_step(
                    report,
                    i,
                    StepStatus::Done,
                    detail.or_else(|| log_lines.last().cloned()),
                ),
                Err(e) => {
                    tracing::warn!(step = %step.label, error = %e, "setup step failed");
                    self.set_step(report, i, StepStatus::Failed, Some(e.clone()));
                    for later in report.steps.iter_mut().skip(i + 1) {
                        if later.status == StepStatus::Pending {
                            later.status = StepStatus::NotRun;
                        }
                    }
                    report.rolled_back = self.roll_back(undo);
                    if !report.rolled_back.is_empty() {
                        for (j, s) in plan.steps.iter().enumerate().take(i) {
                            if !s.done && Self::reversible(&s.action) {
                                report.steps[j].status = StepStatus::RolledBack;
                            }
                        }
                    }
                    report.error = Some(format!("{}: {e}", step.label));
                    *self.setup.lock().unwrap() = Some(report.clone());
                    return;
                }
            }
        }
    }

    fn reversible(action: &SetupAction) -> bool {
        matches!(
            action,
            SetupAction::AddDomain { .. }
                | SetupAction::UpdateDomain { .. }
                | SetupAction::StartService { .. }
                | SetupAction::EnableExtension { .. }
                | SetupAction::ImportCommands { .. }
                | SetupAction::AddWorker { .. }
                | SetupAction::AddSchedule { .. }
                | SetupAction::StartWorkers
        )
    }

    fn set_step(
        &self,
        report: &mut SetupReport,
        i: usize,
        status: StepStatus,
        detail: Option<String>,
    ) {
        report.steps[i].status = status;
        if detail.is_some() {
            report.steps[i].detail = detail;
        }
        *self.setup.lock().unwrap() = Some(report.clone());
    }

    #[allow(clippy::too_many_arguments)]
    fn run_action(
        &self,
        plan: &EnvironmentPlan,
        action: &SetupAction,
        undo: &mut Vec<Undo>,
        installed: &mut LockFile,
        path: &Path,
        report: &mut SetupReport,
        log: &mut dyn FnMut(&str),
    ) -> Result<Option<String>, String> {
        let pid = plan.project_id.as_str();
        match action {
            SetupAction::InstallRuntime { id, version } => {
                self.install_runtime_blocking(id, version, log)?;
                installed.insert(id.clone(), version.clone());
                if id == "php" {
                    self.sync_php_external();
                }
                Ok(Some(format!("{id} {version} installed")))
            }
            SetupAction::EnableExtension { php_version, name } => {
                self.php.set_extension(php_version, name, true)?;
                undo.push(Undo::DisableExtension(php_version.clone(), name.clone()));
                Ok(None)
            }
            SetupAction::EnablePackageManager { manager } => {
                let process = self
                    .enable_package_manager(pid, manager)
                    .map_err(|e| e.to_string())?;
                self.wait_process(process, std::time::Duration::from_secs(300))
            }
            SetupAction::StartService { id } => {
                let was_running = self.services.is_running(id);
                self.start_service_and_wait(id, log)?;
                if !was_running {
                    undo.push(Undo::StopService(id.clone()));
                }
                Ok(None)
            }
            SetupAction::CreateDatabase { engine, name } => {
                if engine == "mongodb" {
                    return Ok(None);
                }
                let started = std::time::Instant::now();
                loop {
                    match self.services.create_database(engine, name) {
                        Ok(()) => return Ok(None),
                        Err(e)
                            if started.elapsed() < std::time::Duration::from_secs(30)
                                && e.to_lowercase().contains("connect") =>
                        {
                            std::thread::sleep(std::time::Duration::from_millis(700))
                        }
                        Err(e) => return Err(e),
                    }
                }
            }
            SetupAction::CreateSqlite { path: file } => {
                let exe = self.sqlite3_path().map_err(|e| e.to_string())?;
                if let Some(parent) = Path::new(file).parent() {
                    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                crate::sqlite::create(&exe, Path::new(file))?;
                self.sqlite
                    .lock()
                    .unwrap()
                    .associate(file, Some(pid.to_string()))
                    .map_err(|e| e.to_string())?;
                Ok(None)
            }
            SetupAction::AddDomain { domain } => {
                self.add_domain((**domain).clone())
                    .map_err(|e| e.to_string())?;
                undo.push(Undo::RemoveDomain(domain.hostname.clone()));
                Ok(None)
            }
            SetupAction::UpdateDomain { domain } => {
                let before = self
                    .domains
                    .lock()
                    .unwrap()
                    .get(&domain.hostname)
                    .ok_or("the site disappeared")?;
                self.update_domain((**domain).clone())
                    .map_err(|e| e.to_string())?;
                undo.push(Undo::RestoreDomain(Box::new(before)));
                Ok(None)
            }
            SetupAction::SyncHosts => {
                let hostnames: Vec<String> = self
                    .domains
                    .lock()
                    .unwrap()
                    .list()
                    .into_iter()
                    .filter(|d| d.enabled && !self.web.dns_covers(&d.hostname))
                    .map(|d| d.hostname)
                    .collect();
                crate::hosts::ensure(&hostnames)?;
                Ok(None)
            }
            SetupAction::TrustCa => {
                self.certs.ca().ensure_created()?;
                self.certs.ca().trust_current_user()?;
                Ok(None)
            }
            SetupAction::ConfigureMail { file } => {
                let plan = self
                    .apply_mailpit_env(pid, file)
                    .map_err(|e| e.to_string())?;
                Ok(Some(format!(
                    "{} value(s) changed; the previous .env is kept as a backup",
                    plan.changes.len()
                )))
            }
            SetupAction::ImportCommands { commands } => {
                for c in commands {
                    let existed = self.quick_commands.list().iter().any(|k| k.id == c.id);
                    self.quick_commands
                        .save(c.clone())
                        .map_err(|e| e.to_string())?;
                    if !existed {
                        undo.push(Undo::DeleteCommand(c.id.clone()));
                    }
                }
                Ok(None)
            }
            SetupAction::AddWorker { worker } => {
                self.save_worker((**worker).clone())
                    .map_err(|e| e.to_string())?;
                undo.push(Undo::RemoveWorker(worker.id.clone()));
                Ok(None)
            }
            SetupAction::AddSchedule { task } => {
                self.save_schedule((**task).clone())
                    .map_err(|e| e.to_string())?;
                undo.push(Undo::RemoveSchedule(task.id.clone()));
                Ok(None)
            }
            SetupAction::ApplyWeb => {
                let reports = self.apply_web(&[]).map_err(|e| e.to_string())?;
                let mut msg = reports
                    .iter()
                    .map(|r| {
                        format!(
                            "{}: {} file(s) written{}",
                            r.server,
                            r.written.len(),
                            if r.started {
                                ", started"
                            } else if r.reloaded {
                                ", reloaded"
                            } else {
                                ""
                            }
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("; ");
                let warnings: Vec<String> =
                    reports.iter().flat_map(|r| r.warnings.clone()).collect();
                if !warnings.is_empty() {
                    msg.push_str(&format!(" ({})", warnings.join("; ")));
                }
                Ok(Some(msg))
            }
            SetupAction::StartWorkers => {
                let started = self.start_project_workers(pid).map_err(|e| e.to_string())?;
                undo.push(Undo::StopWorkers(pid.to_string()));
                Ok(Some(format!("{started} worker process(es) running")))
            }
            SetupAction::StartTunnel { provider, target } => {
                let t = self
                    .start_tunnel_for(Some(pid), provider, target, false)
                    .map_err(|e| e.to_string())?;
                Ok(Some(format!(
                    "public URL: {}",
                    t.public_url
                        .unwrap_or_else(|| "waiting for the provider".into())
                )))
            }
            SetupAction::HealthCheck { hostname } => {
                // A health check failure is reported, not rolled back: the setup itself worked.
                let h = self.health_check(hostname).map_err(|e| e.to_string())?;
                let msg = if h.ok {
                    "the site answers".to_string()
                } else {
                    h.steps
                        .iter()
                        .find(|s| !s.ok && !s.skipped)
                        .map(|s| format!("{}: {}", s.name, s.detail))
                        .unwrap_or_default()
                };
                report.health = Some(h);
                Ok(Some(msg))
            }
            SetupAction::WriteLock => {
                let lock = self.current_lock(pid, installed);
                let file = manifest::write_lock(path, &lock)?;
                report.lock_written = Some(file.display().to_string());
                Ok(Some(
                    lock.iter()
                        .map(|(k, v)| format!("{k} {v}"))
                        .collect::<Vec<_>>()
                        .join(", "),
                ))
            }
        }
    }

    /// Waits for a supervised process to exit; its exit code decides success.
    pub(crate) fn wait_process(
        &self,
        id: crate::process::ProcessId,
        timeout: std::time::Duration,
    ) -> Result<Option<String>, String> {
        let started = std::time::Instant::now();
        while self.supervisor.is_alive(id) {
            if started.elapsed() > timeout {
                self.supervisor.stop(id);
                return Err("it took too long and was stopped".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
        let info = self.supervisor.snapshot().into_iter().find(|p| p.id == id);
        match info.and_then(|p| p.exit_code) {
            Some(0) | None => Ok(None),
            Some(code) => Err(format!(
                "exited with code {code}: {}",
                self.supervisor
                    .recent_output(id)
                    .last()
                    .cloned()
                    .unwrap_or_default()
            )),
        }
    }

    fn roll_back(&self, undo: Vec<Undo>) -> Vec<String> {
        let mut done = Vec::new();
        for u in undo.into_iter().rev() {
            let (what, result): (String, Result<(), String>) = match u {
                Undo::RemoveDomain(h) => (
                    format!("removed site {h}"),
                    self.remove_domain(&h).map_err(|e| e.to_string()),
                ),
                Undo::RestoreDomain(d) => (
                    format!("restored site {}", d.hostname),
                    self.update_domain(*d)
                        .map(|_| ())
                        .map_err(|e| e.to_string()),
                ),
                Undo::StopService(id) => {
                    self.services.stop(&id);
                    (format!("stopped {id}"), Ok(()))
                }
                Undo::DisableExtension(v, n) => (
                    format!("switched PHP {v} extension {n} off again"),
                    self.php.set_extension(&v, &n, false),
                ),
                Undo::DeleteCommand(id) => (
                    format!("removed Quick Command {id}"),
                    self.quick_commands.delete(&id).map_err(|e| e.to_string()),
                ),
                Undo::RemoveWorker(id) => (
                    format!("removed worker {id}"),
                    self.remove_worker(&id).map_err(|e| e.to_string()),
                ),
                Undo::RemoveSchedule(id) => (
                    format!("removed scheduled task {id}"),
                    self.remove_schedule(&id).map_err(|e| e.to_string()),
                ),
                Undo::StopWorkers(pid) => {
                    self.stop_project_workers(&pid);
                    ("stopped the project's workers".to_string(), Ok(()))
                }
            };
            match result {
                Ok(()) => done.push(what),
                Err(e) => done.push(format!("could not undo ({what}): {e}")),
            }
        }
        done
    }

    /// Exact versions in use now, for the lock file.
    fn current_lock(&self, project_id: &str, installed: &LockFile) -> LockFile {
        let mut lock = installed.clone();
        if let Some(detail) = self.project_detail(project_id) {
            for r in detail.resolved {
                if let Some(v) = r.installed_version {
                    lock.insert(r.id, v);
                }
            }
        }
        for s in self.services.list() {
            if s.running && s.kind != "custom" {
                if let Some(v) = s.version {
                    lock.entry(s.id).or_insert(v);
                }
            }
        }
        let cfg = self.web_config();
        if let Some(v) = self
            .runtimes
            .installed_versions(cfg.server())
            .into_iter()
            .next()
        {
            lock.insert(cfg.default_server, v);
        }
        lock
    }
}

struct Planner<'a> {
    inner: &'a Inner,
    steps: Vec<PlanStep>,
    conflicts: Vec<Conflict>,
    lock: &'a LockFile,
}

impl Planner<'_> {
    fn step(&mut self, group: &str, label: String, action: SetupAction) {
        self.steps.push(PlanStep {
            group: group.into(),
            label,
            action,
            done: false,
            note: None,
        });
    }

    fn done(&mut self, group: &str, label: String, action: SetupAction) {
        self.steps.push(PlanStep {
            group: group.into(),
            label,
            action,
            done: true,
            note: None,
        });
    }

    fn note(&mut self, text: String) {
        if let Some(last) = self.steps.last_mut() {
            last.note = Some(text);
        } else {
            self.conflicts.push(Conflict {
                kind: "info".into(),
                blocking: false,
                message: text,
                resolution: String::new(),
            });
        }
    }

    fn conflict(&mut self, kind: &str, blocking: bool, message: String, resolution: &str) {
        self.conflicts.push(Conflict {
            kind: kind.into(),
            blocking,
            message,
            resolution: resolution.into(),
        });
    }

    /// Plans a managed runtime. Returns the version that will be used.
    fn runtime(&mut self, id: &str, wanted: Option<&str>) -> Option<String> {
        let wanted = wanted?;
        let name = self
            .inner
            .runtimes
            .display_name(id)
            .unwrap_or_else(|| id.to_string());
        // A lock pins the exact version a working setup used (§72).
        let locked = self.lock.get(id).cloned();
        let installed = self.inner.runtimes.installed_versions(id);
        let mut custom: Vec<String> = self
            .inner
            .custom_installs
            .lock()
            .unwrap()
            .list()
            .into_iter()
            .filter(|c| c.id == id)
            .map(|c| c.label)
            .collect();
        custom.sort();
        let want = locked.as_deref().unwrap_or(wanted);
        if let Some(v) = crate::php::pick_version(&installed, Some(want))
            .or_else(|| crate::php::pick_version(&custom, Some(want)))
        {
            self.done(
                "install",
                format!("{name} {v}"),
                SetupAction::InstallRuntime {
                    id: id.into(),
                    version: v.clone(),
                },
            );
            return Some(v);
        }
        let available: Vec<String> = crate::catalog::builtin_catalog()
            .iter()
            .filter(|m| m.id == id)
            .map(|m| m.version.to_string())
            .collect();
        if let Some(locked) = &locked {
            if let Some(v) = crate::php::pick_version(&installed, Some(wanted))
                .or_else(|| crate::php::pick_version(&available, Some(wanted)))
            {
                self.conflict("runtime", false, format!("The lock file pins {name} {locked}, which isn't available; {v} matches the manifest's \"{wanted}\"."), "Use it; the lock file is updated after setup.");
                return self.install_or_use(id, &name, &installed, v);
            }
        }
        match crate::php::pick_version(&available, Some(wanted)) {
            Some(v) => self.install_or_use(id, &name, &installed, v),
            None => {
                self.conflict("runtime", true, format!("{name} {wanted} is needed, and it isn't available to install (available: {})", available.join(", ")), "Point OpenLocalServer at your own install on the Runtimes page, or change the manifest's version.");
                None
            }
        }
    }

    fn install_or_use(
        &mut self,
        id: &str,
        name: &str,
        installed: &[String],
        v: String,
    ) -> Option<String> {
        if installed.contains(&v) {
            self.done(
                "install",
                format!("{name} {v}"),
                SetupAction::InstallRuntime {
                    id: id.into(),
                    version: v.clone(),
                },
            );
        } else {
            self.step(
                "install",
                format!("Install {name} {v}"),
                SetupAction::InstallRuntime {
                    id: id.into(),
                    version: v.clone(),
                },
            );
        }
        Some(v)
    }

    /// Plans installing (if needed) and starting a service, with port conflict checks.
    fn service(&mut self, id: &str) {
        if self
            .steps
            .iter()
            .any(|s| matches!(&s.action, SetupAction::StartService { id: x } if x == id))
        {
            return;
        }
        let status = self.inner.services.status(id);
        if !crate::custom_service::is_custom_id(id) && !status.installed {
            if let Some(m) = crate::catalog::builtin_catalog()
                .into_iter()
                .find(|m| m.id == id)
            {
                self.step(
                    "install",
                    format!("Install {} {}", m.name, m.version),
                    SetupAction::InstallRuntime {
                        id: id.into(),
                        version: m.version.into(),
                    },
                );
            }
        }
        if status.running {
            self.done(
                "start",
                format!("{} is running", status.name),
                SetupAction::StartService { id: id.into() },
            );
            return;
        }
        if let Some(port) = status.port {
            if let crate::port::PortStatus::InUse { process_name, pid } =
                crate::port::check_port(port)
            {
                let who = process_name
                    .map(|n| {
                        format!(
                            "{n}{}",
                            pid.map(|p| format!(" (PID {p})")).unwrap_or_default()
                        )
                    })
                    .unwrap_or_else(|| "another program".into());
                self.conflict("port", true, format!("{} needs port {port}, which {who} is using.", status.name), "Stop that program yourself (OpenLocalServer never stops programs it didn't start), then plan again.");
            }
        }
        self.step(
            "start",
            format!("Start {}", status.name),
            SetupAction::StartService { id: id.into() },
        );
    }

    fn domain(
        &mut self,
        project_id: &str,
        path: &Path,
        det: &crate::detection::DetectionResult,
        dm: &DomainManifest,
        php: Option<&str>,
    ) -> Option<String> {
        let host = dm.hostname.trim().to_ascii_lowercase();
        if host.is_empty() || !host.contains('.') {
            self.conflict(
                "domain",
                true,
                format!("\"{}\" is not a usable site name.", dm.hostname),
                "Use a name like shop.test.",
            );
            return None;
        }
        let root_rel = dm.root.clone().or_else(|| det.doc_root.clone());
        let root = match &root_rel {
            Some(r) => match crate::quickapp::plan::safe_join(&path.display().to_string(), r) {
                Ok(p) => p,
                Err(e) => {
                    self.conflict(
                        "domain",
                        true,
                        format!("The site root {r} is not allowed: {e}"),
                        "Use a folder inside the project.",
                    );
                    return None;
                }
            },
            None => path.display().to_string(),
        };
        let kind = match dm.port {
            Some(port) => SiteKind::Proxy {
                upstream_port: port,
                upstream_host: None,
                upstream_https: false,
            },
            None if is_php(&det.framework) => SiteKind::Php {
                version: php.map(|v| v.split('.').take(2).collect::<Vec<_>>().join(".")),
            },
            None => SiteKind::Static,
        };
        let existing = self.inner.domains.lock().unwrap().get(&host);
        let wanted = Domain {
            hostname: host.clone(),
            project_id: Some(project_id.to_string()),
            root: root.clone(),
            kind: kind.clone(),
            https: dm.https,
            redirect_https: dm.https,
            wildcard: dm.wildcard,
            enabled: true,
            ownership: Ownership::Managed,
            app: None::<AppSpec>,
            blocks: Default::default(),
            generated_hashes: Default::default(),
            public_domain: None,
            tunnel_id: None,
            server: None,
        };
        match existing {
            Some(d) if d.project_id.as_deref().is_some_and(|p| p != project_id) => {
                let other = d
                    .project_id
                    .and_then(|p| self.inner.projects.lock().unwrap().get(&p))
                    .map(|p| p.name)
                    .unwrap_or_else(|| "another project".into());
                self.conflict(
                    "domain",
                    true,
                    format!("{host} already belongs to {other}."),
                    "Choose another domain.hostname, or remove that site first.",
                );
                return None;
            }
            Some(d) => {
                if d.ownership != Ownership::Managed {
                    self.conflict("file_ownership", false, format!("{host}'s web config is edited by hand ({:?}); setup leaves that file as it is.", d.ownership).to_lowercase(), "Switch the site back to Managed on the Web config page if you want setup to rewrite it.");
                }
                if d.https != dm.https || d.wildcard != dm.wildcard || !d.enabled {
                    let mut updated = d.clone();
                    updated.https = dm.https;
                    updated.redirect_https = dm.https && (d.redirect_https || !d.https);
                    updated.wildcard = dm.wildcard;
                    updated.enabled = true;
                    updated.project_id = Some(project_id.to_string());
                    self.step(
                        "configure",
                        format!(
                            "Update {host} (HTTPS {}, wildcard {})",
                            on_off(dm.https),
                            on_off(dm.wildcard)
                        ),
                        SetupAction::UpdateDomain {
                            domain: Box::new(updated),
                        },
                    );
                } else {
                    self.done(
                        "configure",
                        format!("Site {host}"),
                        SetupAction::AddDomain {
                            domain: Box::new(wanted),
                        },
                    );
                }
            }
            None => {
                let label = format!(
                    "Add site {host}{}{} → {}",
                    if dm.https { " with HTTPS" } else { "" },
                    if dm.wildcard { " and *." } else { "" },
                    match &kind {
                        SiteKind::Proxy { upstream_port, .. } => format!("port {upstream_port}"),
                        _ => root_rel.clone().unwrap_or_else(|| "project folder".into()),
                    }
                );
                let label = if dm.wildcard {
                    label.replace(" and *.", &format!(" and *.{host}"))
                } else {
                    label
                };
                self.step(
                    "configure",
                    label,
                    SetupAction::AddDomain {
                        domain: Box::new(wanted),
                    },
                );
            }
        }

        // DNS: .test / .localhost / .internal resolve through the built-in DNS; other names need the hosts file.
        let tld = host.rsplit('.').next().unwrap_or_default();
        if !["test", "localhost", "internal", "example", "invalid"].contains(&tld) {
            let hosts = std::fs::read_to_string(crate::hosts::hosts_path()).unwrap_or_default();
            if crate::hosts::lists(&hosts, &host) {
                self.done(
                    "configure",
                    format!("{host} is in the hosts file"),
                    SetupAction::SyncHosts,
                );
            } else {
                self.step(
                    "configure",
                    format!(
                        "Add {host} to the Windows hosts file (asks for administrator approval)"
                    ),
                    SetupAction::SyncHosts,
                );
            }
            if dm.wildcard {
                self.conflict(
                    "domain",
                    false,
                    format!("*.{host} can't be resolved through the hosts file."),
                    "Use a .test name for wildcard sites; they resolve through the built-in DNS.",
                );
            }
        }

        // SSL.
        if dm.https {
            let ca = self.inner.certs.ca_info();
            if !ca.trusted {
                self.step(
                    "configure",
                    "Trust the local certificate authority (one Windows confirmation)".to_string(),
                    SetupAction::TrustCa,
                );
            }
            if let Some(cert) = self.inner.certs.info(&host) {
                if cert.status == crate::certs::CertStatus::Expired {
                    self.conflict(
                        "certificate",
                        false,
                        format!("The certificate for {host} has expired."),
                        "It is renewed when the web config is applied.",
                    );
                }
            }
        }
        Some(host)
    }

    fn workers(&mut self, project_id: &str, framework: &Framework, manifest: &EnvironmentManifest) {
        let existing = self.inner.workers_for(project_id);
        for (name, entry) in &manifest.workers {
            let def = match entry {
                WorkerEntry::On(false) => continue,
                WorkerEntry::On(true) => match crate::workers::default_command(framework) {
                    Some(cmd) => crate::manifest::WorkerManifest {
                        command: cmd.into(),
                        count: 1,
                        timeout_secs: None,
                        memory_mb: None,
                    },
                    None => {
                        self.conflict("manifest", true, format!("The \"{name}\" worker has no command, and this kind of project has no usual one."), "Give it one: workers: { name: { command: \"node worker.js\" } }.");
                        continue;
                    }
                },
                WorkerEntry::Custom(w) => w.clone(),
            };
            let id = crate::workers::worker_id(project_id, name);
            if existing
                .iter()
                .any(|w| w.id == id && w.command == def.command && w.count == def.count.max(1))
            {
                self.done(
                    "configure",
                    format!("Worker \"{name}\""),
                    SetupAction::StartWorkers,
                );
                continue;
            }
            let worker = crate::workers::Worker {
                id,
                project_id: project_id.to_string(),
                name: name.clone(),
                command: def.command.clone(),
                count: def.count.max(1),
                timeout_secs: def.timeout_secs,
                memory_mb: def.memory_mb,
                max_retries: 5,
                restart: true,
                autostart: true,
            };
            self.step(
                "configure",
                format!(
                    "Add worker \"{name}\": {} × {}",
                    worker.count, worker.command
                ),
                SetupAction::AddWorker {
                    worker: Box::new(worker),
                },
            );
        }

        let tasks: Vec<crate::manifest::ScheduledTaskManifest> = match &manifest.scheduler {
            Some(SchedulerEntry::On(true)) => {
                match crate::scheduler::default_task(framework) {
                    Some(t) => vec![t],
                    None => {
                        self.conflict("manifest", false, "scheduler: true, but this kind of project has no usual scheduler command.".to_string(), "List the tasks instead: scheduler: [{ name, schedule, command }].");
                        vec![]
                    }
                }
            }
            Some(SchedulerEntry::Tasks(list)) => list.clone(),
            _ => vec![],
        };
        let existing = self.inner.schedules_for(project_id);
        for t in tasks {
            if let Err(e) = crate::scheduler::Schedule::parse(&t.schedule) {
                self.conflict("manifest", true, format!("Scheduled task \"{}\": {e}", t.name), "Use a cron expression (\"*/5 * * * *\") or every_minute, hourly, daily, weekly.");
                continue;
            }
            let id = crate::workers::worker_id(project_id, &t.name);
            if existing
                .iter()
                .any(|x| x.id == id && x.command == t.command && x.schedule == t.schedule)
            {
                self.done(
                    "configure",
                    format!("Scheduled task \"{}\"", t.name),
                    SetupAction::WriteLock,
                );
                continue;
            }
            let task = crate::scheduler::ScheduledTask {
                id,
                project_id: Some(project_id.to_string()),
                name: t.name.clone(),
                schedule: t.schedule.clone(),
                command: t.command.clone(),
                enabled: true,
            };
            self.step(
                "configure",
                format!(
                    "Schedule \"{}\" ({}): {}",
                    t.name,
                    crate::scheduler::describe(&t.schedule),
                    t.command
                ),
                SetupAction::AddSchedule {
                    task: Box::new(task),
                },
            );
        }
    }
}

fn on_off(b: bool) -> &'static str {
    if b {
        "on"
    } else {
        "off"
    }
}

/// For the CLI and the UI: the plan as the §76 text block.
pub fn plan_text(plan: &EnvironmentPlan) -> String {
    let mut groups: BTreeMap<usize, (String, Vec<String>)> = BTreeMap::new();
    let order = ["install", "create", "configure", "start", "tunnel", "check"];
    for s in &plan.steps {
        let idx = order
            .iter()
            .position(|g| *g == s.group)
            .unwrap_or(order.len());
        let title = match s.group.as_str() {
            "install" => "Install",
            "create" => "Create",
            "configure" => "Configure",
            "start" => "Start",
            "tunnel" => "Tunnel",
            _ => "Check",
        };
        let mark = if s.done { "✓" } else { "•" };
        groups
            .entry(idx)
            .or_insert_with(|| (title.to_string(), Vec::new()))
            .1
            .push(format!(
                "  {mark} {}{}",
                s.label,
                s.note
                    .as_ref()
                    .map(|n| format!("\n      {n}"))
                    .unwrap_or_default()
            ));
    }
    let mut out = format!("Environment Plan: {}\n", plan.project_name);
    if !plan.manifest_found {
        out.push_str("(no .openlocalserver/environment.yaml; planned from what was detected)\n");
    }
    for (_, (title, lines)) in groups {
        out.push_str(&format!("\n{title}:\n{}\n", lines.join("\n")));
    }
    if !plan.conflicts.is_empty() {
        out.push_str("\nConflicts:\n");
        for c in &plan.conflicts {
            out.push_str(&format!(
                "  {} [{}] {}\n",
                if c.blocking { "✗" } else { "⚠" },
                c.kind,
                c.message
            ));
            if !c.resolution.is_empty() {
                out.push_str(&format!("      → {}\n", c.resolution));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{Core, CoreCommand, CoreResponse};

    fn core() -> (Core, crate::test_support::IsolatedHome) {
        let home = crate::test_support::isolated_home();
        let settings = crate::settings::SettingsService::load(&home.paths).unwrap();
        (Core::new(settings, home.paths.clone()), home)
    }

    fn register(core: &Core, dir: &Path) -> String {
        match core
            .dispatch(CoreCommand::RegisterProject {
                path: dir.display().to_string(),
            })
            .unwrap()
        {
            CoreResponse::Project { project } => project.id,
            _ => panic!(),
        }
    }

    #[test]
    fn db_names_are_safe_identifiers() {
        assert_eq!(db_name_for("My-Shop 2"), "my_shop_2");
        assert_eq!(db_name_for("--"), "app");
        assert!(crate::service::is_safe_identifier(&db_name_for(
            "ünïcode-thing"
        )));
    }

    #[test]
    fn a_plan_follows_the_manifest_and_a_dry_run_changes_nothing() {
        let (core, home) = core();
        let dir = home.paths.root().join("shop");
        std::fs::create_dir_all(dir.join("public")).unwrap();
        std::fs::create_dir_all(dir.join(".openlocalserver")).unwrap();
        std::fs::write(dir.join("public").join("index.php"), "<?php").unwrap();
        std::fs::write(dir.join(".openlocalserver").join("environment.yaml"), "name: shop\nruntime:\n  php: \"8.4\"\ndomain:\n  hostname: shop.test\n  https: true\n  root: public\ndatabase:\n  engine: mysql\nservices:\n  redis: true\n").unwrap();
        let id = register(&core, &dir);

        let CoreResponse::SetupPlan { plan } = core
            .dispatch(CoreCommand::PlanSetup {
                project_id: id.clone(),
            })
            .unwrap()
        else {
            panic!()
        };
        assert!(plan.manifest_found);
        let labels: Vec<&str> = plan.steps.iter().map(|s| s.label.as_str()).collect();
        let text = labels.join("\n");
        for want in [
            "PHP",
            "MariaDB database \"shop\"",
            "Redis",
            "shop.test",
            "environment.lock",
        ] {
            assert!(text.contains(want), "missing {want} in:\n{text}");
        }
        let pos = |needle: &str| labels.iter().position(|l| l.contains(needle)).unwrap();
        assert!(
            pos("PHP") < pos("database") && pos("database") < pos("shop.test"),
            "§74 order: runtimes, database, domain"
        );

        let CoreResponse::Setup { report } = core
            .dispatch(CoreCommand::ApplySetup {
                project_id: id.clone(),
                dry_run: true,
            })
            .unwrap()
        else {
            panic!()
        };
        assert!(report.dry_run && !report.running);
        assert!(
            report
                .steps
                .iter()
                .all(|s| matches!(s.status, StepStatus::Pending | StepStatus::Skipped)),
            "a dry run runs nothing"
        );
        assert!(!manifest::lock_path(&dir).exists());
    }

    #[test]
    fn a_domain_owned_by_another_project_is_a_blocking_conflict() {
        let (core, home) = core();
        let a = home.paths.root().join("a");
        let b = home.paths.root().join("b");
        for d in [&a, &b] {
            std::fs::create_dir_all(d.join(".openlocalserver")).unwrap();
            std::fs::write(d.join("index.html"), "hi").unwrap();
            std::fs::write(
                d.join(".openlocalserver").join("environment.yaml"),
                "domain:\n  hostname: same.test\n",
            )
            .unwrap();
        }
        let a_id = register(&core, &a);
        let b_id = register(&core, &b);
        let CoreResponse::SetupPlan { plan } = core
            .dispatch(CoreCommand::PlanSetup {
                project_id: a_id.clone(),
            })
            .unwrap()
        else {
            panic!()
        };
        let SetupAction::AddDomain { domain } = &plan
            .steps
            .iter()
            .find(|s| matches!(s.action, SetupAction::AddDomain { .. }))
            .unwrap()
            .action
        else {
            panic!()
        };
        core.inner().add_domain((**domain).clone()).unwrap();

        let CoreResponse::SetupPlan { plan } = core
            .dispatch(CoreCommand::PlanSetup {
                project_id: b_id.clone(),
            })
            .unwrap()
        else {
            panic!()
        };
        assert!(!plan.ok);
        assert!(
            plan.conflicts
                .iter()
                .any(|c| c.kind == "domain" && c.blocking && c.message.contains("a")),
            "{:?}",
            plan.conflicts
        );
        let CoreResponse::Setup { report } = core
            .dispatch(CoreCommand::ApplySetup {
                project_id: b_id,
                dry_run: false,
            })
            .unwrap()
        else {
            panic!()
        };
        assert!(
            !report.ok && report.error.is_some(),
            "a blocked plan is never applied"
        );
    }

    #[test]
    fn a_failed_step_rolls_back_the_safe_changes_before_it() {
        let (core, home) = core();
        let dir = home.paths.root().join("site");
        std::fs::create_dir_all(dir.join(".openlocalserver")).unwrap();
        std::fs::write(dir.join("index.html"), "hi").unwrap();
        // The site is added, then the unknown-extension-free failure: SQLite needs sqlite3,
        // which isn't installed in a test home, so creating the database fails.
        std::fs::write(
            dir.join(".openlocalserver").join("environment.yaml"),
            "domain:\n  hostname: roll.test\ndatabase:\n  engine: sqlite\n",
        )
        .unwrap();
        let id = register(&core, &dir);
        let plan = core.inner().plan_setup(&id).unwrap();
        // Put the site before the database so there is something to roll back.
        let mut steps = plan.steps.clone();
        let site = steps
            .iter()
            .position(|s| matches!(s.action, SetupAction::AddDomain { .. }))
            .unwrap();
        let s = steps.remove(site);
        steps.insert(0, s);
        let plan = EnvironmentPlan { steps, ..plan };
        let mut report = SetupReport {
            steps: plan
                .steps
                .iter()
                .map(|s| StepResult {
                    group: s.group.clone(),
                    label: s.label.clone(),
                    status: StepStatus::Pending,
                    detail: None,
                })
                .collect(),
            ..Default::default()
        };
        core.inner().run_plan(&plan, &mut report);
        assert!(
            report.error.is_some(),
            "creating SQLite without sqlite3 fails"
        );
        assert_eq!(report.steps[0].status, StepStatus::RolledBack);
        assert!(
            core.inner()
                .domains
                .lock()
                .unwrap()
                .get("roll.test")
                .is_none(),
            "the site added before the failure is gone"
        );
        assert!(report.rolled_back.iter().any(|r| r.contains("roll.test")));
    }

    #[test]
    fn without_a_manifest_the_plan_comes_from_detection_and_can_be_saved() {
        let (core, home) = core();
        let dir = home.paths.root().join("blog");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.html"), "hi").unwrap();
        let id = register(&core, &dir);
        let CoreResponse::SetupPlan { plan } = core
            .dispatch(CoreCommand::PlanSetup {
                project_id: id.clone(),
            })
            .unwrap()
        else {
            panic!()
        };
        assert!(!plan.manifest_found);
        assert_eq!(
            plan.manifest.domain.as_ref().unwrap().hostname,
            "blog.local"
        );
        core.dispatch(CoreCommand::SaveManifest {
            project_id: id.clone(),
            manifest: None,
        })
        .unwrap();
        assert!(manifest::read_manifest(&dir).unwrap().is_some());
        assert!(plan_text(&plan).contains("Environment Plan: blog"));
    }
}
