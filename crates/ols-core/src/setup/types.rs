use serde::{Deserialize, Serialize};
use std::path::Path;

pub use crate::detection::Framework;
pub use crate::domain::{AppSpec, Domain, Ownership, SiteKind};
pub use crate::manifest::{
    self, DatabaseManifest, DomainManifest, EnvironmentManifest, LockFile, SchedulerEntry,
    ServiceToggle, WorkerEntry,
};
pub use crate::quickapp::commands::QuickCommand;
pub use crate::resolver::ResolutionSource;

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
pub(crate) enum Undo {
    RemoveDomain(String),
    RestoreDomain(Box<Domain>),
    StopService(String),
    DisableExtension(String, String),
    DeleteCommand(String),
    RemoveWorker(String),
    RemoveSchedule(String),
    StopWorkers(String),
}

pub(crate) const DB_SERVICES: &[(&str, &str)] = &[
    ("mariadb", "mariadb"),
    ("mysql", "mariadb"),
    ("postgres", "postgres"),
    ("postgresql", "postgres"),
    ("mongodb", "mongodb"),
    ("mongo", "mongodb"),
];

pub(crate) fn db_service(engine: &str) -> Option<&'static str> {
    DB_SERVICES
        .iter()
        .find(|(e, _)| e.eq_ignore_ascii_case(engine))
        .map(|(_, s)| *s)
}

pub(crate) fn is_php(f: &Framework) -> bool {
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
pub fn env_value(project: &Path, key: &str) -> Option<String> {
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
