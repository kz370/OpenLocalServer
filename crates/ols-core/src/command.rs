//! The `CoreCommand` dispatcher (architecture decision 1): every front door — Tauri IPC,
//! the CLI, and later the local HTTP API — routes through this single enum + `dispatch` fn.
//! The UI/CLI never talks to individual managers directly.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::app::{
    DomainSummary, HealthItem, Host, Inner, LogSource, QuickPlanResult, StartupSettings,
};
use crate::certs::{CaInfo, CertInfo};
use crate::custom_install::CustomInstall;
use crate::dbtools::{DbTool, ExternalTool};
use crate::domain::{Domain, Ownership};
use crate::error::{CoreError, Diagnostic};
use crate::health::HealthReport;
use crate::paths::AppPaths;
use crate::port::{self, PortStatus};
use crate::process::{CommandHistoryEntry, ProcessId, ProcessInfo, ProcessSpec, ProcessSupervisor};
use crate::project::{Project, ProjectDetail};
use crate::quickapp::commands::{quick_command_from_line, HistoryEntry, QuickCommand};
use crate::quickapp::{EntryDetail, EntryView, RunView};
use crate::resolver::ResolutionSource;
use crate::runtime::{CatalogEntry, RuntimeManager};
use crate::service::{ConnectionInfo, DbUser, ServiceManager, ServiceStatus};
use crate::settings::SettingsService;
use crate::sqlite::{IntegrityResult, SqliteInfo};
use crate::web::manager::{ApplyReport, ConfigFile, ConfigPart, ConfigVersion, WebStatus};
use crate::web::WebConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CoreCommand {
    /// Trivial liveness check the UI calls on startup.
    Ping,
    GetSetting { key: String },
    SetSetting { key: String, value: Value },

    // Process Supervisor (§107, Stage 2)
    StartProcess { spec: ProcessSpec },
    StopProcess { id: ProcessId },
    ListProcesses,
    GetProcessOutput { id: ProcessId },

    // Command Runner (§90–91, Stage 2)
    RunCommand { executable: String, args: Vec<String>, cwd: Option<String>, timeout_ms: u64 },
    ListCommandHistory,

    // Port Manager (§109, Stage 2)
    CheckPort { port: u16 },

    // Runtime Manager (§9–10, §20–21, Stage 3)
    ListRuntimeCatalog,
    InstallRuntime { id: String, version: String },

    // Project Manager + Environment Resolver (§18, §40, §42–43, Stage 4)
    RegisterProject { path: String },
    /// Registers every project folder found as an immediate child of `path` (not
    /// recursive) — for a workspace directory holding several unrelated projects,
    /// which don't need to share a parent beyond that one scan point.
    ScanAndRegisterProjects { path: String },
    ListProjects,
    RemoveProject { id: String },
    GetProjectDetail { id: String },
    /// Runtime-aware terminal (§19): runs `runtime_id`'s resolved binary for this
    /// project, with its bin dir first on PATH, and streams output like any other
    /// managed process (reuses the Process Supervisor — same live-output UI).
    RunInProject { project_id: String, runtime_id: String, args: Vec<String> },

    // Service Manager (§22, §31, §61–68, Stage 5)
    ListServices,
    StartService { id: String },
    StopService { id: String },
    CreateMysqlDatabase { name: String },

    // Secrets Manager (§104, §141, Stage 5)
    SetSecret { key: String, value: String },
    GetSecret { key: String },
    DeleteSecret { key: String },

    // External DB GUI tools (§102, Stage 5)
    ListDbTools,
    OpenDbTool { id: String },

    // Custom install locations (Stage 5, user-requested): point DevForge at a tool or
    // runtime version it didn't find/install itself, instead of only ever offering a
    // download. `label` is a version string for runtimes ("8.1"), empty for single-path
    // tools (heidisql/pgadmin).
    SetCustomInstall { id: String, label: String, path: String },
    RemoveCustomInstall { id: String, label: String },
    ListCustomInstalls,

    // ---- Stage 6: domains, HTTPS, web server -------------------------------------
    GetWebStatus,
    GetWebConfig,
    ListDomains,
    GetDomain { hostname: String },
    AddDomain { domain: Domain },
    UpdateDomain { domain: Domain },
    RemoveDomain { hostname: String },
    SetDomainEnabled { hostname: String, enabled: bool },
    DuplicateDomain { hostname: String, new_hostname: String },
    /// §48 templates: `{project}.test`, `api.{project}.test`, ...
    SuggestDomain { project_id: String, template: String },
    /// §28 apply pipeline. `overwrite` lists hosts whose hand-edited file may be replaced (§27).
    ApplyWeb { overwrite: Vec<String> },
    StopWeb,
    ValidateWeb,
    RestartSiteApp { hostname: String },
    SyncHosts,
    GetCaInfo,
    TrustCa,
    UntrustCa,
    ListCertificates,
    RegenerateCertificate { hostname: String },
    RevokeCertificate { hostname: String },
    HealthCheck { hostname: String },

    // ---- Stage 9: config management ---------------------------------------------
    ListWebConfigs,
    ReadWebConfig { hostname: Option<String>, part: ConfigPart },
    WriteWebConfig { hostname: String, part: ConfigPart, content: String },
    SetOwnership { hostname: String, ownership: Ownership },
    ListConfigHistory { hostname: String },
    ReadConfigHistory { hostname: String, id: String },
    RestoreConfigHistory { hostname: String, id: String },
    ExportWebConfig { hostname: String, part: ConfigPart, dest: String },

    // ---- Stage 10: more databases ------------------------------------------------
    CreateDatabase { engine: String, name: String },
    ListDatabases { engine: String },
    CreateDbUser { engine: String, user: String, password: String, database: String },
    ListDbUsers { engine: String },
    GetConnectionInfo { engine: String, database: Option<String>, path: Option<String> },
    ListSqlite,
    DetectSqlite { project_id: String },
    CreateSqlite { path: String, project_id: Option<String> },
    AssociateSqlite { path: String, project_id: Option<String> },
    ForgetSqlite { path: String },
    BackupSqlite { path: String },
    RestoreSqlite { path: String, backup: String },
    CheckSqlite { path: String },
    ListExternalTools,
    SaveExternalTool { tool: ExternalTool },
    RemoveExternalTool { id: String },
    OpenDatabase { engine: String, database: Option<String>, path: Option<String>, tool_id: Option<String> },

    // ---- Stage 7: Quick Apps + Quick Commands -----------------------------------
    ListQuickApps,
    GetQuickApp { id: String },
    SaveQuickApp { yaml: String },
    DuplicateQuickApp { id: String, new_id: String, new_name: String },
    DeleteQuickApp { id: String },
    FavoriteQuickApp { id: String, favorite: bool },
    ExportQuickApp { id: String, dest: String },
    ImportQuickApp { source: String },
    TrustQuickAppSource { origin: String },
    PlanQuickApp { id: String, values: BTreeMap<String, String> },
    /// `approval`: "once" or "source" — required for untrusted (imported) apps (§139).
    /// `allow_elevated`: the separate confirmation for administrator steps (§92).
    StartQuickApp { id: String, values: BTreeMap<String, String>, approval: Option<String>, allow_elevated: bool },
    GetQuickRun { id: String },
    ListQuickRuns,
    CancelQuickRun { id: String },
    ListQuickCommands,
    SaveQuickCommand { command: QuickCommand },
    DeleteQuickCommand { id: String },
    RunQuickCommand { id: String, project_id: Option<String> },
    RunCommandLine { line: String, cwd: Option<String>, project_id: Option<String> },
    ListHistory,
    DeleteHistory { id: u64 },
    ClearHistory,
    SaveHistoryAsQuickCommand { id: u64, command_id: String, name: String },

    // ---- Stage 8: dashboard, logs, startup, project actions ----------------------
    GetDashboard,
    GetEnvironmentHealth,
    ListLogSources,
    ReadLog { source: String, max_lines: usize },
    ExportLog { source: String, dest: String },
    GetStartupSettings,
    SetStartupSettings { settings: StartupSettings },
    OpenPath { path: String },
    OpenUrl { url: String },
    OpenInEditor { path: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardData {
    pub services: Vec<ServiceStatus>,
    pub web: WebStatus,
    pub domains: Vec<DomainSummary>,
    pub project_count: usize,
    pub health: Vec<HealthItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CoreResponse {
    Pong { version: String },
    Setting { key: String, value: Option<Value> },
    Ok,
    ProcessStarted { id: ProcessId },
    Processes { processes: Vec<ProcessInfo> },
    ProcessOutput { id: ProcessId, lines: Vec<String> },
    CommandResult { entry: CommandHistoryEntry },
    CommandHistory { entries: Vec<CommandHistoryEntry> },
    Port { port: u16, status: PortStatus },
    RuntimeCatalog { entries: Vec<CatalogEntry> },
    Project { project: Project },
    Projects { projects: Vec<Project> },
    ProjectDetail { detail: Box<ProjectDetail> },
    Services { services: Vec<ServiceStatus> },
    Secret { key: String, value: Option<String> },
    DbTools { tools: Vec<DbTool> },
    CustomInstalls { entries: Vec<CustomInstall> },

    WebStatus { status: Box<WebStatus> },
    WebConfig { config: WebConfig },
    Domains { domains: Vec<DomainSummary> },
    Domain { domain: Box<Domain> },
    Text { text: String },
    Applied { report: Box<ApplyReport> },
    CaInfo { info: CaInfo },
    Certificates { certs: Vec<CertInfo> },
    Health { report: HealthReport },
    Configs { files: Vec<ConfigFile> },
    ConfigVersions { versions: Vec<ConfigVersion> },

    Names { names: Vec<String> },
    DbUsers { users: Vec<DbUser> },
    Connection { info: ConnectionInfo },
    SqliteList { databases: Vec<SqliteInfo> },
    SqliteInfo { info: SqliteInfo },
    Integrity { result: IntegrityResult },
    ExternalTools { tools: Vec<ExternalTool> },

    QuickApps { apps: Vec<EntryView> },
    QuickApp { detail: Box<EntryDetail> },
    QuickPlan { result: Box<QuickPlanResult> },
    QuickRunStarted { run_id: String },
    QuickRun { run: Box<RunView> },
    QuickRuns { runs: Vec<RunView> },
    QuickCommands { commands: Vec<QuickCommand> },
    History { entries: Vec<HistoryEntry> },
    MaybeProcess { id: Option<ProcessId> },

    Dashboard { data: Box<DashboardData> },
    EnvironmentHealth { items: Vec<HealthItem> },
    LogSources { sources: Vec<LogSource> },
    LogLines { source: String, lines: Vec<String> },
    Startup { settings: StartupSettings },
}

/// A cheap handle onto the shared application state. Cloning shares everything.
#[derive(Clone)]
pub struct Core {
    inner: Arc<Inner>,
}

impl Core {
    pub fn new(settings: SettingsService, paths: AppPaths) -> Self {
        Self { inner: Inner::new(settings, paths).expect("failed to start the application core") }
    }

    /// Used by the Tauri shell so it can share the supervisor/runtime managers it forwards
    /// events from with the core.
    pub fn with_parts(
        settings: SettingsService,
        paths: AppPaths,
        supervisor: Arc<ProcessSupervisor>,
        runtimes: Arc<RuntimeManager>,
    ) -> Self {
        Self { inner: Inner::with_parts(settings, paths, supervisor, runtimes).expect("failed to start the application core") }
    }

    pub fn inner(&self) -> &Arc<Inner> {
        &self.inner
    }
    pub fn supervisor(&self) -> Arc<ProcessSupervisor> {
        self.inner.supervisor.clone()
    }
    pub fn runtimes(&self) -> Arc<RuntimeManager> {
        self.inner.runtimes.clone()
    }
    pub fn services(&self) -> Arc<ServiceManager> {
        self.inner.services.clone()
    }

    /// Route a `CoreCommand` to the right manager and return its result.
    /// Every failure is converted to a `Diagnostic` before it reaches the caller.
    pub fn dispatch(&self, command: CoreCommand) -> Result<CoreResponse, Diagnostic> {
        self.dispatch_inner(command).map_err(|e| Diagnostic::from(&e))
    }

    fn dispatch_inner(&self, command: CoreCommand) -> Result<CoreResponse, CoreError> {
        use CoreCommand as C;
        use CoreResponse as R;
        let i = &self.inner;
        match command {
            C::Ping => Ok(R::Pong { version: env!("CARGO_PKG_VERSION").to_string() }),
            C::GetSetting { key } => {
                let value = i.settings.lock().unwrap().get(&key).cloned();
                Ok(R::Setting { value, key })
            }
            C::SetSetting { key, value } => {
                // Redact before it ever reaches the log, per §141 — settings can hold secrets.
                let logged_value = crate::logging::redact_value(&key, &value.to_string());
                tracing::info!(command = "set_setting", key = %key, value = %logged_value);
                i.settings.lock().unwrap().set(key, value)?;
                Ok(R::Ok)
            }

            C::StartProcess { spec } => {
                tracing::info!(command = "start_process", name = %spec.name);
                Ok(R::ProcessStarted { id: i.supervisor.start(spec) })
            }
            C::StopProcess { id } => {
                i.supervisor.stop(id);
                Ok(R::Ok)
            }
            C::ListProcesses => Ok(R::Processes { processes: i.supervisor.snapshot() }),
            C::GetProcessOutput { id } => Ok(R::ProcessOutput { id, lines: i.supervisor.recent_output(id) }),

            C::RunCommand { executable, args, cwd, timeout_ms } => {
                tracing::info!(command = "run_command", executable = %executable);
                let entry = i.supervisor.run_to_completion(&executable, &args, cwd.as_deref(), Duration::from_millis(timeout_ms));
                Ok(R::CommandResult { entry })
            }
            C::ListCommandHistory => Ok(R::CommandHistory { entries: i.supervisor.history() }),

            C::CheckPort { port: p } => Ok(R::Port { port: p, status: port::check_port(p) }),

            C::ListRuntimeCatalog => Ok(R::RuntimeCatalog { entries: i.runtimes.catalog() }),
            C::InstallRuntime { id, version } => {
                tracing::info!(command = "install_runtime", id = %id, version = %version);
                i.runtimes.install(&id, &version);
                Ok(R::Ok)
            }

            C::RegisterProject { path } => {
                tracing::info!(command = "register_project", path = %path);
                Ok(R::Project { project: i.projects.lock().unwrap().register(&path)? })
            }
            C::ScanAndRegisterProjects { path } => {
                tracing::info!(command = "scan_and_register_projects", path = %path);
                let root = std::path::PathBuf::from(&path);
                if !root.is_dir() {
                    return Err(CoreError::InvalidProjectPath(path));
                }
                let candidates = crate::detection::scan_for_projects(&root);
                let mut projects = i.projects.lock().unwrap();
                let mut registered = Vec::with_capacity(candidates.len());
                for candidate in candidates {
                    if let Some(s) = candidate.to_str() {
                        registered.push(projects.register(s)?);
                    }
                }
                Ok(R::Projects { projects: registered })
            }
            C::ListProjects => Ok(R::Projects { projects: i.projects.lock().unwrap().list() }),
            C::RemoveProject { id } => {
                i.projects.lock().unwrap().remove(&id)?;
                Ok(R::Ok)
            }
            C::GetProjectDetail { id } => {
                let detail = i.project_detail(&id).ok_or_else(|| CoreError::InvalidProjectPath(id.clone()))?;
                Ok(R::ProjectDetail { detail: Box::new(detail) })
            }
            C::RunInProject { project_id, runtime_id, args } => {
                let project = i.projects.lock().unwrap().get(&project_id).ok_or_else(|| CoreError::InvalidProjectPath(project_id.clone()))?;
                let detail = i.project_detail(&project_id).ok_or_else(|| CoreError::InvalidProjectPath(project_id.clone()))?;
                let resolved = detail.resolved.iter().find(|r| r.id == runtime_id);

                let Some(r) = resolved.filter(|r| r.installed_version.is_some()) else {
                    return Err(CoreError::InvalidProjectPath(format!("{runtime_id} is not resolved to an installed version for this project")));
                };
                let bin_dir = r.bin_dir.clone();

                let binary = if r.source == ResolutionSource::Custom {
                    // A custom install's own path IS the binary — bin_dir above is just
                    // its parent directory (for PATH), not something to re-derive from.
                    i.custom_installs
                        .lock()
                        .unwrap()
                        .resolve(&runtime_id, r.requested_version.as_deref())
                        .map(|c| std::path::PathBuf::from(&c.path))
                        .ok_or_else(|| CoreError::InvalidProjectPath(format!("{runtime_id} custom install vanished")))?
                } else {
                    let version = r.installed_version.clone().unwrap();
                    i.runtimes.binary_path(&runtime_id, &version).ok_or_else(|| CoreError::InvalidProjectPath(format!("{runtime_id} {version} binary missing on disk")))?
                };

                // Prepend the resolved runtime's own directory to PATH so a command this
                // process shells out to (e.g. `npm` calling back into `node`) also finds
                // the project-selected version, not whatever's on the system PATH (§19).
                let system_path = std::env::var("PATH").unwrap_or_default();
                let new_path = match bin_dir {
                    Some(dir) => format!("{dir};{system_path}"),
                    None => system_path,
                };
                let mut env = vec![("PATH".to_string(), new_path)];
                if runtime_id == "php" {
                    if let Some(v) = &r.installed_version {
                        if let Ok(ini) = i.php.write_ini(v) {
                            env.push(("PHPRC".into(), ini.display().to_string()));
                        }
                    }
                }
                let version_for_log = r.installed_version.clone().unwrap_or_default();
                tracing::info!(command = "run_in_project", project = %project.name, runtime = %runtime_id, version = %version_for_log);
                let id = i.supervisor.start(ProcessSpec {
                    name: format!("{}: {} {}", project.name, runtime_id, args.join(" ")),
                    executable: binary.display().to_string(),
                    args,
                    cwd: Some(project.path.clone()),
                    env,
                    restart: None,
                });
                Ok(R::ProcessStarted { id })
            }

            C::ListServices => Ok(R::Services { services: i.services.list() }),
            C::StartService { id } => {
                tracing::info!(command = "start_service", id = %id);
                i.services.start(&id).map_err(CoreError::ServiceError)?;
                Ok(R::Ok)
            }
            C::StopService { id } => {
                i.services.stop(&id);
                Ok(R::Ok)
            }
            C::CreateMysqlDatabase { name } => {
                i.services.create_database("mysql", &name).map_err(CoreError::ServiceError)?;
                Ok(R::Ok)
            }

            C::SetSecret { key, value } => {
                // Never log the value itself, only that a secret was set (§141).
                tracing::info!(command = "set_secret", key = %key);
                crate::secrets::set_secret(&key, &value).map_err(CoreError::ServiceError)?;
                Ok(R::Ok)
            }
            C::GetSecret { key } => {
                let value = crate::secrets::get_secret(&key).map_err(CoreError::ServiceError)?;
                Ok(R::Secret { key, value })
            }
            C::DeleteSecret { key } => {
                crate::secrets::delete_secret(&key).map_err(CoreError::ServiceError)?;
                Ok(R::Ok)
            }

            C::ListDbTools => {
                // A user-pinned custom path always wins over auto-detection (§126).
                let mut tools = crate::dbtools::detect_db_tools();
                let custom = i.custom_installs.lock().unwrap();
                for tool in &mut tools {
                    if let Some(c) = custom.resolve(&tool.id, None) {
                        tool.found_path = Some(c.path.clone());
                    }
                }
                Ok(R::DbTools { tools })
            }
            C::OpenDbTool { id } => {
                let path = i
                    .custom_installs
                    .lock()
                    .unwrap()
                    .resolve(&id, None)
                    .map(|c| c.path.clone())
                    .or_else(|| crate::dbtools::detect_db_tools().into_iter().find(|t| t.id == id).and_then(|t| t.found_path))
                    .ok_or_else(|| CoreError::ServiceError(format!("{id} was not found on this system")))?;
                // A standalone GUI app the user drives themselves — not managed/supervised.
                crate::dbtools::launch(&path, &[], false).map_err(CoreError::ServiceError)?;
                Ok(R::Ok)
            }

            C::SetCustomInstall { id, label, path } => {
                i.custom_installs.lock().unwrap().set(&id, &label, &path)?;
                Ok(R::Ok)
            }
            C::RemoveCustomInstall { id, label } => {
                i.custom_installs.lock().unwrap().remove(&id, &label)?;
                Ok(R::Ok)
            }
            C::ListCustomInstalls => Ok(R::CustomInstalls { entries: i.custom_installs.lock().unwrap().list() }),

            // ---- Stage 6
            C::GetWebStatus => Ok(R::WebStatus { status: Box::new(i.web.status(&i.web_config())) }),
            C::GetWebConfig => Ok(R::WebConfig { config: i.web_config() }),
            C::ListDomains => Ok(R::Domains { domains: i.domain_summaries() }),
            C::GetDomain { hostname } => {
                let d = i.domains.lock().unwrap().get(&hostname).ok_or_else(|| CoreError::DomainError(format!("{hostname} is not a known domain")))?;
                Ok(R::Domain { domain: Box::new(d) })
            }
            C::AddDomain { domain } => {
                tracing::info!(command = "add_domain", hostname = %domain.hostname);
                Ok(R::Domain { domain: Box::new(i.add_domain(domain)?) })
            }
            C::UpdateDomain { domain } => Ok(R::Domain { domain: Box::new(i.update_domain(domain)?) }),
            C::RemoveDomain { hostname } => {
                i.remove_domain(&hostname)?;
                Ok(R::Ok)
            }
            C::SetDomainEnabled { hostname, enabled } => {
                i.set_domain_enabled(&hostname, enabled)?;
                Ok(R::Ok)
            }
            C::DuplicateDomain { hostname, new_hostname } => Ok(R::Domain { domain: Box::new(i.duplicate_domain(&hostname, &new_hostname)?) }),
            C::SuggestDomain { project_id, template } => {
                let project = i.projects.lock().unwrap().get(&project_id).ok_or_else(|| CoreError::InvalidProjectPath(project_id.clone()))?;
                Ok(R::Text { text: crate::domain::apply_template(&template, &project.name) })
            }
            C::ApplyWeb { overwrite } => {
                tracing::info!(command = "apply_web");
                Ok(R::Applied { report: Box::new(i.apply_web(&overwrite)?) })
            }
            C::StopWeb => {
                i.web.stop();
                Ok(R::Ok)
            }
            C::ValidateWeb => Ok(R::Text { text: i.web.validate(&i.web_config())? }),
            C::RestartSiteApp { hostname } => {
                i.web.restart_app(&hostname);
                i.apply_web(&[])?;
                Ok(R::Ok)
            }
            C::SyncHosts => {
                let hostnames: Vec<String> = i.domains.lock().unwrap().list().into_iter().filter(|d| d.enabled).map(|d| d.hostname).collect();
                let changed = crate::hosts::sync(&hostnames).map_err(CoreError::WebError)?;
                Ok(R::Text { text: if changed { "The hosts file was updated.".into() } else { "The hosts file was already up to date.".into() } })
            }
            C::GetCaInfo => Ok(R::CaInfo { info: i.certs.ca_info() }),
            C::TrustCa => {
                i.certs.ca().ensure_created().map_err(CoreError::WebError)?;
                i.certs.ca().trust_current_user().map_err(CoreError::WebError)?;
                Ok(R::CaInfo { info: i.certs.ca_info() })
            }
            C::UntrustCa => {
                i.certs.ca().untrust_current_user().map_err(CoreError::WebError)?;
                Ok(R::CaInfo { info: i.certs.ca_info() })
            }
            C::ListCertificates => Ok(R::Certificates { certs: i.certs.list() }),
            C::RegenerateCertificate { hostname } => {
                let d = i.domains.lock().unwrap().get(&hostname).ok_or_else(|| CoreError::DomainError(format!("{hostname} is not a known domain")))?;
                i.certs.issue(&d).map_err(CoreError::WebError)?;
                if i.web.is_running() {
                    i.apply_web(&[])?;
                }
                Ok(R::Certificates { certs: i.certs.list() })
            }
            C::RevokeCertificate { hostname } => {
                i.certs.revoke(&hostname).map_err(CoreError::WebError)?;
                Ok(R::Certificates { certs: i.certs.list() })
            }
            C::HealthCheck { hostname } => Ok(R::Health { report: i.health_check(&hostname)? }),

            // ---- Stage 9
            C::ListWebConfigs => {
                let domains = i.domains.lock().unwrap();
                Ok(R::Configs { files: i.web.list_configs(&i.web_config(), &domains)? })
            }
            C::ReadWebConfig { hostname, part } => Ok(R::Text { text: i.web.read_config(&i.web_config(), hostname.as_deref(), part)? }),
            C::WriteWebConfig { hostname, part, content } => Ok(R::Text { text: i.write_web_config(&hostname, part, &content)? }),
            C::SetOwnership { hostname, ownership } => {
                i.set_ownership(&hostname, ownership)?;
                Ok(R::Ok)
            }
            C::ListConfigHistory { hostname } => Ok(R::ConfigVersions { versions: i.web.list_history(&i.web_config(), &hostname) }),
            C::ReadConfigHistory { hostname, id } => Ok(R::Text { text: i.web.read_history(&i.web_config(), &hostname, &id)? }),
            C::RestoreConfigHistory { hostname, id } => Ok(R::Text { text: i.restore_web_history(&hostname, &id)? }),
            C::ExportWebConfig { hostname, part, dest } => {
                i.web.export_config(&i.web_config(), &hostname, part, &dest)?;
                Ok(R::Ok)
            }

            // ---- Stage 10
            C::CreateDatabase { engine, name } => {
                i.services.create_database(&engine, &name).map_err(CoreError::ServiceError)?;
                Ok(R::Ok)
            }
            C::ListDatabases { engine } => Ok(R::Names { names: i.services.list_databases(&engine).map_err(CoreError::ServiceError)? }),
            C::CreateDbUser { engine, user, password, database } => {
                tracing::info!(command = "create_db_user", engine = %engine, user = %user);
                i.services.create_user(&engine, &user, &password, &database).map_err(CoreError::ServiceError)?;
                Ok(R::Ok)
            }
            C::ListDbUsers { engine } => Ok(R::DbUsers { users: i.services.list_users(&engine).map_err(CoreError::ServiceError)? }),
            C::GetConnectionInfo { engine, database, path } => {
                Ok(R::Connection { info: i.services.connection_info(&engine, database.as_deref(), path.as_deref()).map_err(CoreError::ServiceError)? })
            }
            C::ListSqlite => Ok(R::SqliteList { databases: i.sqlite.lock().unwrap().list() }),
            C::DetectSqlite { project_id } => {
                let project = i.projects.lock().unwrap().get(&project_id).ok_or_else(|| CoreError::InvalidProjectPath(project_id.clone()))?;
                let found = crate::sqlite::detect_in_project(std::path::Path::new(&project.path));
                let mut store = i.sqlite.lock().unwrap();
                let mut out = Vec::new();
                for f in found {
                    out.push(store.associate(&f.display().to_string(), Some(project_id.clone()))?);
                }
                Ok(R::SqliteList { databases: out })
            }
            C::CreateSqlite { path, project_id } => {
                let exe = i.sqlite3_path()?;
                crate::sqlite::create(&exe, std::path::Path::new(&path)).map_err(CoreError::ServiceError)?;
                Ok(R::SqliteInfo { info: i.sqlite.lock().unwrap().associate(&path, project_id)? })
            }
            C::AssociateSqlite { path, project_id } => Ok(R::SqliteInfo { info: i.sqlite.lock().unwrap().associate(&path, project_id)? }),
            C::ForgetSqlite { path } => {
                i.sqlite.lock().unwrap().forget(&path)?;
                Ok(R::Ok)
            }
            C::BackupSqlite { path } => {
                let exe = i.sqlite3_path()?;
                let dest = crate::sqlite::backup(&exe, std::path::Path::new(&path)).map_err(CoreError::ServiceError)?;
                Ok(R::Text { text: dest.display().to_string() })
            }
            C::RestoreSqlite { path, backup } => {
                let exe = i.sqlite3_path()?;
                let safety = crate::sqlite::restore(&exe, std::path::Path::new(&path), std::path::Path::new(&backup)).map_err(CoreError::ServiceError)?;
                Ok(R::Text { text: safety.map(|p| p.display().to_string()).unwrap_or_default() })
            }
            C::CheckSqlite { path } => {
                let exe = i.sqlite3_path()?;
                Ok(R::Integrity { result: crate::sqlite::integrity_check(&exe, std::path::Path::new(&path)).map_err(CoreError::ServiceError)? })
            }
            C::ListExternalTools => Ok(R::ExternalTools { tools: i.ext_tools.lock().unwrap().list() }),
            C::SaveExternalTool { tool } => {
                i.ext_tools.lock().unwrap().save(tool)?;
                Ok(R::ExternalTools { tools: i.ext_tools.lock().unwrap().list() })
            }
            C::RemoveExternalTool { id } => {
                i.ext_tools.lock().unwrap().remove(&id)?;
                Ok(R::ExternalTools { tools: i.ext_tools.lock().unwrap().list() })
            }
            C::OpenDatabase { engine, database, path, tool_id } => {
                i.open_database(&engine, database.as_deref(), path.as_deref(), tool_id.as_deref())?;
                Ok(R::Ok)
            }

            // ---- Stage 7
            C::ListQuickApps => Ok(R::QuickApps { apps: i.catalog.lock().unwrap().list() }),
            C::GetQuickApp { id } => Ok(R::QuickApp { detail: Box::new(i.catalog.lock().unwrap().get(&id)?) }),
            C::SaveQuickApp { yaml } => Ok(R::QuickApp { detail: Box::new(i.catalog.lock().unwrap().save(&yaml)?) }),
            C::DuplicateQuickApp { id, new_id, new_name } => Ok(R::QuickApp { detail: Box::new(i.catalog.lock().unwrap().duplicate(&id, &new_id, &new_name)?) }),
            C::DeleteQuickApp { id } => {
                i.catalog.lock().unwrap().delete(&id)?;
                Ok(R::Ok)
            }
            C::FavoriteQuickApp { id, favorite } => {
                i.catalog.lock().unwrap().set_favorite(&id, favorite)?;
                Ok(R::Ok)
            }
            C::ExportQuickApp { id, dest } => {
                i.catalog.lock().unwrap().export(&id, &dest)?;
                Ok(R::Ok)
            }
            C::ImportQuickApp { source } => {
                tracing::info!(command = "import_quick_app", source = %source);
                i.catalog.lock().unwrap().import(&source)?;
                Ok(R::QuickApps { apps: i.catalog.lock().unwrap().list() })
            }
            C::TrustQuickAppSource { origin } => {
                i.catalog.lock().unwrap().trust_source(&origin)?;
                Ok(R::Ok)
            }
            C::PlanQuickApp { id, values } => Ok(R::QuickPlan { result: Box::new(i.plan_quick_app(&id, &values)?) }),
            C::StartQuickApp { id, values, approval, allow_elevated } => {
                let result = i.plan_quick_app(&id, &values)?;
                let Some(plan) = result.plan else {
                    let msgs: Vec<String> = result.errors.iter().map(|e| e.message.clone()).collect();
                    return Err(CoreError::QuickAppError(msgs.join("; ")));
                };
                if !result.trusted {
                    // §139: imported recipes never run without the user's explicit approval.
                    match approval.as_deref() {
                        Some("source") => {
                            let detail = i.catalog.lock().unwrap().get(&id)?;
                            if let Some(origin) = detail.view.origin {
                                i.catalog.lock().unwrap().trust_source(&origin)?;
                            }
                        }
                        Some("once") => {}
                        _ => return Err(CoreError::QuickAppError("this Quick App comes from an untrusted source. Review its commands and approve it first.".into())),
                    }
                }
                if plan.steps.iter().any(|s| s.elevated) && !allow_elevated {
                    return Err(CoreError::QuickAppError("this Quick App has steps that need administrator rights. Confirm them separately first.".into()));
                }
                tracing::info!(command = "start_quick_app", app = %id);
                let host = Arc::new(Host { inner: i.clone(), project_id: std::sync::Mutex::new(None) });
                Ok(R::QuickRunStarted { run_id: i.runs.start(plan, host, allow_elevated) })
            }
            C::GetQuickRun { id } => i.runs.get(&id).map(|r| R::QuickRun { run: Box::new(r) }).ok_or_else(|| CoreError::QuickAppError(format!("no run \"{id}\""))),
            C::ListQuickRuns => Ok(R::QuickRuns { runs: i.runs.list() }),
            C::CancelQuickRun { id } => {
                i.runs.cancel(&id);
                Ok(R::Ok)
            }
            C::ListQuickCommands => Ok(R::QuickCommands { commands: i.quick_commands.list() }),
            C::SaveQuickCommand { command } => {
                i.quick_commands.save(command)?;
                Ok(R::QuickCommands { commands: i.quick_commands.list() })
            }
            C::DeleteQuickCommand { id } => {
                i.quick_commands.delete(&id)?;
                Ok(R::QuickCommands { commands: i.quick_commands.list() })
            }
            C::RunQuickCommand { id, project_id } => Ok(R::MaybeProcess { id: i.run_quick_command(&id, project_id.as_deref())? }),
            C::RunCommandLine { line, cwd, project_id } => Ok(R::ProcessStarted { id: i.run_command_line(&line, cwd.as_deref(), project_id.as_deref(), None)? }),
            C::ListHistory => Ok(R::History { entries: i.history.lock().unwrap().list() }),
            C::DeleteHistory { id } => {
                i.history.lock().unwrap().delete(id)?;
                Ok(R::History { entries: i.history.lock().unwrap().list() })
            }
            C::ClearHistory => {
                i.history.lock().unwrap().clear()?;
                Ok(R::Ok)
            }
            C::SaveHistoryAsQuickCommand { id, command_id, name } => {
                let entry = i.history.lock().unwrap().get(id).ok_or_else(|| CoreError::QuickAppError("that history entry no longer exists".into()))?;
                let cmd = quick_command_from_line(&command_id, &name, &entry.line, entry.cwd.as_deref()).map_err(CoreError::QuickAppError)?;
                i.quick_commands.save(cmd)?;
                Ok(R::QuickCommands { commands: i.quick_commands.list() })
            }

            // ---- Stage 8
            C::GetDashboard => {
                let cfg = i.web_config();
                Ok(R::Dashboard {
                    data: Box::new(DashboardData {
                        services: i.services.list(),
                        web: i.web.status(&cfg),
                        domains: i.domain_summaries(),
                        project_count: i.projects.lock().unwrap().list().len(),
                        health: i.environment_health(),
                    }),
                })
            }
            C::GetEnvironmentHealth => Ok(R::EnvironmentHealth { items: i.environment_health() }),
            C::ListLogSources => Ok(R::LogSources { sources: i.log_sources() }),
            C::ReadLog { source, max_lines } => Ok(R::LogLines { lines: i.read_log(&source, max_lines.clamp(1, 20_000))?, source }),
            C::ExportLog { source, dest } => {
                let lines = i.read_log(&source, 1_000_000)?;
                std::fs::write(&dest, lines.join("\n"))?;
                Ok(R::Ok)
            }
            C::GetStartupSettings => Ok(R::Startup { settings: i.startup_settings() }),
            C::SetStartupSettings { settings } => {
                i.set_startup_settings(settings)?;
                Ok(R::Startup { settings: i.startup_settings() })
            }
            C::OpenPath { path } => {
                i.open_path(&path)?;
                Ok(R::Ok)
            }
            C::OpenUrl { url } => {
                i.open_url(&url)?;
                Ok(R::Ok)
            }
            C::OpenInEditor { path } => {
                i.open_in_editor(&path)?;
                Ok(R::Ok)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Keeps the isolated-home guard alive for the test's duration (it unsets `OLS_HOME` on drop).
    fn test_core() -> (Core, crate::test_support::IsolatedHome) {
        let home = crate::test_support::isolated_home();
        let settings = SettingsService::load(&home.paths).unwrap();
        let core = Core::new(settings, home.paths.clone());
        (core, home)
    }

    #[test]
    fn ping_returns_pong_with_version() {
        let (core, _home) = test_core();
        match core.dispatch(CoreCommand::Ping).unwrap() {
            CoreResponse::Pong { version } => assert!(!version.is_empty()),
            _ => panic!("expected Pong"),
        }
    }

    #[test]
    fn set_then_get_setting_round_trips_through_dispatch() {
        let (core, _home) = test_core();
        core.dispatch(CoreCommand::SetSetting { key: "editor".into(), value: Value::String("vscode".into()) }).unwrap();
        match core.dispatch(CoreCommand::GetSetting { key: "editor".into() }).unwrap() {
            CoreResponse::Setting { value, .. } => assert_eq!(value, Some(Value::String("vscode".into()))),
            _ => panic!("expected Setting"),
        }
    }

    #[test]
    fn get_missing_setting_returns_none_not_error() {
        let (core, _home) = test_core();
        match core.dispatch(CoreCommand::GetSetting { key: "does-not-exist".into() }).unwrap() {
            CoreResponse::Setting { value, .. } => assert_eq!(value, None),
            _ => panic!("expected Setting"),
        }
    }

    #[test]
    fn every_command_variant_round_trips_through_json_with_its_snake_case_tag() {
        // The UI builds these as JSON — a renamed field would silently break it.
        let cmd = CoreCommand::StartQuickApp {
            id: "laravel".into(),
            values: BTreeMap::from([("project_name".to_string(), "shop".to_string())]),
            approval: Some("once".into()),
            allow_elevated: false,
        };
        let json = serde_json::to_value(&cmd).unwrap();
        assert_eq!(json["type"], "start_quick_app");
        assert_eq!(json["values"]["project_name"], "shop");
        let back: CoreCommand = serde_json::from_value(json).unwrap();
        assert!(matches!(back, CoreCommand::StartQuickApp { .. }));

        let json = serde_json::json!({"type": "read_web_config", "hostname": null, "part": "main"});
        assert!(matches!(serde_json::from_value::<CoreCommand>(json).unwrap(), CoreCommand::ReadWebConfig { part: ConfigPart::Main, .. }));
    }

    #[test]
    fn domains_reject_injection_in_structured_blocks_and_relative_roots() {
        use crate::domain::{HeaderRule, SiteBlocks, SiteKind};
        let (core, home) = test_core();
        let root = home.paths.root().join("site").display().to_string();
        let mut domain = Domain {
            hostname: "shop.test".into(),
            project_id: None,
            root: root.clone(),
            kind: SiteKind::Static,
            https: false,
            redirect_https: false,
            wildcard: false,
            enabled: true,
            ownership: Ownership::Managed,
            app: None,
            blocks: SiteBlocks::default(),
            generated_hashes: Default::default(),
        };
        domain.blocks.headers.push(HeaderRule { name: "X-Test".into(), value: "a\"; } server { listen 1; ".into() });
        assert!(core.dispatch(CoreCommand::AddDomain { domain: domain.clone() }).is_err(), "a header value must not break out of its directive");

        domain.blocks.headers.clear();
        domain.root = "relative/path".into();
        assert!(core.dispatch(CoreCommand::AddDomain { domain: domain.clone() }).is_err());

        domain.root = root;
        assert!(core.dispatch(CoreCommand::AddDomain { domain }).is_ok());
        match core.dispatch(CoreCommand::ListDomains).unwrap() {
            CoreResponse::Domains { domains } => {
                assert_eq!(domains.len(), 1);
                assert_eq!(domains[0].url, "http://shop.test/");
            }
            _ => panic!(),
        }
    }

    #[test]
    fn untrusted_quick_apps_refuse_to_run_without_approval() {
        let (core, home) = test_core();
        let file = home.paths.root().join("x.yaml");
        std::fs::write(&file, "id: theirs\nname: Theirs\nvariables:\n  - { name: project_name, required: true }\ncommands:\n  - echo hi\n").unwrap();
        core.dispatch(CoreCommand::ImportQuickApp { source: file.display().to_string() }).unwrap();

        let values = BTreeMap::from([("project_name".to_string(), "demo".to_string())]);
        let err = core.dispatch(CoreCommand::StartQuickApp { id: "theirs".into(), values, approval: None, allow_elevated: false }).unwrap_err();
        assert!(err.cause.contains("untrusted"), "{err:?}");
    }

    #[test]
    fn plan_quick_app_reports_field_errors_instead_of_failing() {
        let (core, _home) = test_core();
        match core.dispatch(CoreCommand::PlanQuickApp { id: "laravel".into(), values: BTreeMap::new() }).unwrap() {
            CoreResponse::QuickPlan { result } => {
                assert!(!result.ok);
                assert!(result.errors.iter().any(|e| e.field == "project_name"));
            }
            _ => panic!(),
        }
    }
}
