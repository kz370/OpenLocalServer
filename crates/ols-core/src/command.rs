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
use crate::xdebug::{XdebugReport, XdebugSettings};

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
    /// Laragon-style: give every project in the remembered folders a `<name>.test` domain.
    SyncAutoDomains,
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
    /// Register every PHP install found under `dir` (see `php::scan_folder`).
    ScanPhpFolder { dir: String },
    /// Extensions for one installed PHP version (managed "8.4.26" or a custom label).
    ListPhpExtensions { version: String },
    SetPhpExtension { version: String, name: String, enabled: bool },
    /// Download a PECL extension build matching that PHP and enable it.
    InstallPhpExtension { version: String, name: String },
    ListPeclPackages,

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
    RenameDomain { hostname: String, new_hostname: String },
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
    ClearLog { source: String },
    GetStartupSettings,
    SetStartupSettings { settings: StartupSettings },
    OpenPath { path: String },
    OpenUrl { url: String },
    OpenInEditor { path: String },
    /// Known code editors and where each is installed (Settings picker).
    ListEditors,
    /// CPU / memory / disk for the machine and for every managed process tree.
    GetSystemStats,
    /// Old Laragon / XAMPP / WampServer database servers found on this PC.
    ListMigrationSources,
    ListForeignDatabases { source_id: String, password: String },
    /// Copy databases (all when `databases` is empty) into our `target` engine.
    MigrateDatabases { source_id: String, password: String, databases: Vec<String>, target: String },
    /// The admin helper service: status, install (one UAC prompt), remove.
    GetHelperService,
    InstallHelperService,
    UninstallHelperService,

    // ---- Stage 11: runtime depth + diagnostics -----------------------------------
    /// Xdebug state and settings for one PHP version (§13).
    GetXdebug { version: String },
    SetXdebug { version: String, settings: XdebugSettings },
    /// IDE setup text for a project: `ide` is "vscode", "phpstorm" or "other".
    XdebugIdeConfig { project_id: String, ide: String, version: String },
    /// composer.json / composer.lock read for a project (§14).
    GetComposerInfo { project_id: String },
    /// One of the named Composer actions (see `composer::command_args`).
    RunComposer { project_id: String, action: String, target: Option<String> },
    /// Which Node package managers a project uses and can run (§15).
    GetPackageManagers { project_id: String },
    /// Switch on pnpm or yarn through corepack.
    EnablePackageManager { project_id: String, manager: String },
    /// Python virtual environment of a project (§17).
    GetVenv { project_id: String },
    CreateVenv { project_id: String, recreate: bool },
    InstallVenvRequirements { project_id: String, what: String },
    /// DiagnosticEngine v1 (§112).
    RunDiagnostics,
    IgnoreDiagnostic { id: String, ignore: bool },
    RestartService { id: String },

    // ---- .env editor (§103) ------------------------------------------------------
    ListEnvFiles { project_id: String },
    ReadEnvFile { project_id: String, file: String },
    SaveEnvFile { project_id: String, file: String, content: String },
    SetEnvValue { project_id: String, file: String, key: String, value: String },
    DeleteEnvKey { project_id: String, file: String, key: String },
    CompareEnvFiles { project_id: String, a: String, b: String },
    /// `mode` is "merge" (keep this file's other keys) or "replace".
    ImportEnvFile { project_id: String, file: String, source: String, mode: String },
    ExportEnvFile { project_id: String, file: String, dest: String },
    /// A new env file, copied from `from` when given (e.g. `.env` from `.env.example`).
    CreateEnvFile { project_id: String, file: String, from: Option<String> },
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
    PhpScan { found: Vec<crate::php::ScannedPhp> },
    PhpExtensions { report: crate::php::PhpExtensions },

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
    Count { count: usize },
    HelperService { installed: bool },
    SystemStats { stats: Box<crate::monitor::SystemStats> },
    MigrationSources { sources: Vec<crate::migrate::MigrationSource> },
    Migrated { results: Vec<crate::migrate::MigratedDb> },
    Editors { editors: Vec<crate::editors::EditorInfo> },
    Xdebug { report: XdebugReport },
    Composer { info: Box<crate::composer::ComposerInfo> },
    PackageManagers { info: crate::nodepm::PackageManagerInfo },
    Venv { info: crate::venv::VenvInfo },
    Diagnostics { findings: Vec<crate::diagnostics::Finding> },
    EnvFiles { files: Vec<crate::envfile::EnvFileInfo> },
    EnvFile { view: Box<crate::envfile::EnvFileView> },
    EnvCompare { rows: Vec<crate::envfile::EnvDiffRow> },
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
                let project = i.projects.lock().unwrap().register(&path)?;
                if let Err(e) = i.sync_auto_domains() {
                    tracing::warn!(error = %e, "automatic domains could not be synced");
                }
                Ok(R::Project { project })
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
                drop(projects);
                i.remember_projects_root(&path)?;
                if let Err(e) = i.sync_auto_domains() {
                    tracing::warn!(error = %e, "automatic domains could not be synced");
                }
                Ok(R::Projects { projects: registered })
            }
            C::SyncAutoDomains => Ok(R::Count { count: i.sync_auto_domains()? }),
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
                i.sync_php_external();
                Ok(R::Ok)
            }
            C::RemoveCustomInstall { id, label } => {
                i.custom_installs.lock().unwrap().remove(&id, &label)?;
                i.sync_php_external();
                Ok(R::Ok)
            }
            C::ScanPhpFolder { dir } => {
                let found = crate::php::scan_folder(std::path::Path::new(&dir));
                {
                    let mut store = i.custom_installs.lock().unwrap();
                    for f in &found {
                        store.set("php", &f.version, &f.php_exe)?;
                    }
                }
                i.sync_php_external();
                Ok(R::PhpScan { found })
            }
            C::ListPhpExtensions { version } => Ok(R::PhpExtensions { report: i.php.extensions_report(&version) }),
            C::SetPhpExtension { version, name, enabled } => {
                tracing::info!(command = "set_php_extension", version = %version, name = %name, enabled);
                i.php.set_extension(&version, &name, enabled).map_err(CoreError::ServiceError)?;
                Ok(R::PhpExtensions { report: i.php.extensions_report(&version) })
            }
            C::InstallPhpExtension { version, name } => {
                tracing::info!(command = "install_php_extension", version = %version, name = %name);
                i.php.install_extension(&version, &name).map_err(CoreError::ServiceError)?;
                Ok(R::PhpExtensions { report: i.php.extensions_report(&version) })
            }
            C::ListPeclPackages => Ok(R::Names { names: i.php.pecl_packages().map_err(CoreError::ServiceError)? }),
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
            C::RenameDomain { hostname, new_hostname } => {
                tracing::info!(command = "rename_domain", from = %hostname, to = %new_hostname);
                Ok(R::Domain { domain: Box::new(i.rename_domain(&hostname, &new_hostname)?) })
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
                // Names our local DNS already answers for don't need a hosts entry.
                let hostnames: Vec<String> =
                    i.domains.lock().unwrap().list().into_iter().filter(|d| d.enabled && !i.web.dns_covers(&d.hostname)).map(|d| d.hostname).collect();
                let changed = crate::hosts::ensure(&hostnames).map_err(CoreError::WebError)?;
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
            C::ClearLog { source } => {
                i.clear_log(&source)?;
                Ok(R::Ok)
            }
            C::ListEditors => Ok(R::Editors { editors: crate::editors::detect() }),
            C::GetSystemStats => Ok(R::SystemStats { stats: Box::new(i.system_stats()) }),
            C::ListMigrationSources => Ok(R::MigrationSources { sources: crate::migrate::detect() }),
            C::ListForeignDatabases { source_id, password } => Ok(R::Names { names: i.foreign_databases(&source_id, &password)? }),
            C::MigrateDatabases { source_id, password, databases, target } => {
                tracing::info!(command = "migrate_databases", source = %source_id, target = %target);
                Ok(R::Migrated { results: i.migrate_databases(&source_id, &password, &databases, &target)? })
            }
            C::GetHelperService => Ok(R::HelperService { installed: crate::elevate::service_available() }),
            C::InstallHelperService => {
                crate::elevate::install_service().map_err(CoreError::ServiceError)?;
                Ok(R::HelperService { installed: crate::elevate::service_available() })
            }
            C::UninstallHelperService => {
                crate::elevate::uninstall_service().map_err(CoreError::ServiceError)?;
                Ok(R::HelperService { installed: crate::elevate::service_available() })
            }

            // ---- Stage 11
            C::GetXdebug { version } => Ok(R::Xdebug { report: i.xdebug_report(&version) }),
            C::SetXdebug { version, settings } => {
                tracing::info!(command = "set_xdebug", version = %version);
                i.php.set_xdebug_settings(&version, &settings).map_err(CoreError::ServiceError)?;
                Ok(R::Xdebug { report: i.xdebug_report(&version) })
            }
            C::XdebugIdeConfig { project_id, ide, version } => Ok(R::Text { text: i.xdebug_ide_config(&project_id, &ide, &version)? }),
            C::GetComposerInfo { project_id } => Ok(R::Composer { info: Box::new(i.composer_info(&project_id)?) }),
            C::RunComposer { project_id, action, target } => {
                Ok(R::ProcessStarted { id: i.run_composer(&project_id, &action, target.as_deref())? })
            }
            C::GetPackageManagers { project_id } => Ok(R::PackageManagers { info: i.package_managers(&project_id)? }),
            C::EnablePackageManager { project_id, manager } => {
                Ok(R::ProcessStarted { id: i.enable_package_manager(&project_id, &manager)? })
            }
            C::GetVenv { project_id } => Ok(R::Venv { info: i.venv_info(&project_id)? }),
            C::CreateVenv { project_id, recreate } => Ok(R::ProcessStarted { id: i.create_venv(&project_id, recreate)? }),
            C::InstallVenvRequirements { project_id, what } => {
                Ok(R::ProcessStarted { id: i.install_venv_requirements(&project_id, &what)? })
            }
            C::RunDiagnostics => Ok(R::Diagnostics { findings: i.diagnose() }),
            C::IgnoreDiagnostic { id, ignore } => {
                i.set_diagnostic_ignored(&id, ignore)?;
                Ok(R::Diagnostics { findings: i.diagnose() })
            }
            C::RestartService { id } => {
                i.services.stop(&id);
                // The old process needs a moment to release its port before the new one binds it.
                std::thread::sleep(Duration::from_millis(800));
                i.services.start(&id).map_err(CoreError::ServiceError)?;
                Ok(R::Ok)
            }

            // ---- .env editor
            C::ListEnvFiles { project_id } => Ok(R::EnvFiles { files: i.env_files(&project_id)? }),
            C::ReadEnvFile { project_id, file } => Ok(R::EnvFile { view: Box::new(i.env_read(&project_id, &file)?) }),
            C::SaveEnvFile { project_id, file, content } => {
                tracing::info!(command = "save_env_file", file = %file);
                Ok(R::EnvFile { view: Box::new(i.env_write(&project_id, &file, &content)?) })
            }
            C::SetEnvValue { project_id, file, key, value } => {
                // The value is never logged: env files hold secrets (§141).
                tracing::info!(command = "set_env_value", file = %file, key = %key);
                Ok(R::EnvFile { view: Box::new(i.env_set(&project_id, &file, &key, &value)?) })
            }
            C::DeleteEnvKey { project_id, file, key } => Ok(R::EnvFile { view: Box::new(i.env_delete(&project_id, &file, &key)?) }),
            C::CompareEnvFiles { project_id, a, b } => Ok(R::EnvCompare { rows: i.env_compare(&project_id, &a, &b)? }),
            C::ImportEnvFile { project_id, file, source, mode } => Ok(R::EnvFile { view: Box::new(i.env_import(&project_id, &file, &source, &mode)?) }),
            C::ExportEnvFile { project_id, file, dest } => {
                i.env_export(&project_id, &file, &dest)?;
                Ok(R::Ok)
            }
            C::CreateEnvFile { project_id, file, from } => Ok(R::EnvFile { view: Box::new(i.env_create(&project_id, &file, from.as_deref())?) }),

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
    fn sites_are_grouped_by_website_type() {
        let (core, home) = test_core();
        let root = home.paths.root().join("site");
        std::fs::create_dir_all(&root).unwrap();
        let make = |host: &str, kind: crate::domain::SiteKind, app: Option<(&str, Option<&str>)>| Domain {
            hostname: host.into(),
            project_id: None,
            root: root.display().to_string(),
            kind,
            https: false,
            redirect_https: false,
            wildcard: false,
            enabled: true,
            ownership: Ownership::Managed,
            app: app.map(|(exe, runtime)| crate::domain::AppSpec { executable: exe.into(), args: vec![], cwd: root.display().to_string(), runtime: runtime.map(str::to_string) }),
            blocks: Default::default(),
            generated_hashes: Default::default(),
        };
        let proxy = || crate::domain::SiteKind::Proxy { upstream_port: 3000, upstream_host: None, upstream_https: false };
        let sites = [
            make("a.test", crate::domain::SiteKind::Php { version: None }, None),
            make("b.test", crate::domain::SiteKind::Static, None),
            make("c.test", proxy(), Some(("npm", Some("node")))),
            make("d.test", proxy(), Some(("uvicorn", None))),
            make("e.test", proxy(), None),
            make("f.test", crate::domain::SiteKind::Proxy { upstream_port: 80, upstream_host: Some("10.0.0.5".into()), upstream_https: false }, None),
        ];
        for s in sites {
            core.dispatch(CoreCommand::AddDomain { domain: s }).unwrap();
        }
        let groups: std::collections::BTreeMap<String, String> = match core.dispatch(CoreCommand::ListDomains).unwrap() {
            CoreResponse::Domains { domains } => domains.into_iter().map(|d| (d.hostname, d.group)).collect(),
            _ => panic!("expected Domains"),
        };
        let expect = [("a.test", "php"), ("b.test", "static"), ("c.test", "nodejs"), ("d.test", "python"), ("e.test", "proxy"), ("f.test", "proxy")];
        for (host, group) in expect {
            assert_eq!(groups.get(host).map(String::as_str), Some(group), "{host}");
        }
    }

    #[test]
    fn env_files_round_trip_through_the_core_and_keep_a_backup() {
        let (core, home) = test_core();
        let dir = home.paths.root().join("laravel");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(".env"), "# keep me\nAPP_ENV=local\nDB_PASSWORD=old\n").unwrap();
        std::fs::write(dir.join(".env.example"), "APP_ENV=example\nNEW_KEY=1\n").unwrap();
        let pid = match core.dispatch(CoreCommand::RegisterProject { path: dir.display().to_string() }).unwrap() {
            CoreResponse::Project { project } => project.id,
            _ => panic!("expected Project"),
        };
        let view = |r: CoreResponse| match r {
            CoreResponse::EnvFile { view } => *view,
            _ => panic!("expected EnvFile"),
        };
        let v = view(core.dispatch(CoreCommand::SetEnvValue { project_id: pid.clone(), file: ".env".into(), key: "APP_ENV".into(), value: "production".into() }).unwrap());
        assert!(v.content.starts_with("# keep me\nAPP_ENV=production\n"));
        assert!(v.entries.iter().find(|e| e.key == "DB_PASSWORD").unwrap().secret);
        let backups = home.paths.data_dir().join("env_backups").join(&pid);
        assert_eq!(std::fs::read_dir(backups).unwrap().count(), 1, "the previous version is kept");

        // A file with a broken line is refused and the good one stays.
        assert!(core.dispatch(CoreCommand::SaveEnvFile { project_id: pid.clone(), file: ".env".into(), content: "oops no equals\n".into() }).is_err());
        assert!(std::fs::read_to_string(dir.join(".env")).unwrap().contains("APP_ENV=production"));

        // Names that could leave the project folder are refused.
        assert!(core.dispatch(CoreCommand::ReadEnvFile { project_id: pid.clone(), file: "../secrets.txt".into() }).is_err());

        match core.dispatch(CoreCommand::CompareEnvFiles { project_id: pid.clone(), a: ".env".into(), b: ".env.example".into() }).unwrap() {
            CoreResponse::EnvCompare { rows } => assert!(rows.iter().any(|r| r.key == "NEW_KEY" && r.status == "only_b")),
            _ => panic!("expected EnvCompare"),
        }
        let created = view(core.dispatch(CoreCommand::CreateEnvFile { project_id: pid.clone(), file: ".env.testing".into(), from: Some(".env.example".into()) }).unwrap());
        assert_eq!(created.entries.len(), 2);
        let v = view(core.dispatch(CoreCommand::DeleteEnvKey { project_id: pid, file: ".env.testing".into(), key: "NEW_KEY".into() }).unwrap());
        assert_eq!(v.content, "APP_ENV=example\n");
    }

    #[test]
    fn diagnostics_report_a_missing_site_folder_and_remember_ignores() {
        let (core, home) = test_core();
        let gone = home.paths.root().join("gone");
        let domain = Domain {
            hostname: "ghost.test".into(),
            project_id: None,
            root: gone.display().to_string(),
            kind: crate::domain::SiteKind::Static,
            https: false,
            redirect_https: false,
            wildcard: false,
            enabled: true,
            ownership: Ownership::Managed,
            app: None,
            blocks: Default::default(),
            generated_hashes: Default::default(),
        };
        // Written straight to the store: AddDomain would reject a folder that doesn't exist.
        core.inner().domains.lock().unwrap().add(domain).ok();
        let findings = |core: &Core| match core.dispatch(CoreCommand::RunDiagnostics).unwrap() {
            CoreResponse::Diagnostics { findings } => findings,
            _ => panic!("expected Diagnostics"),
        };
        let found = findings(&core);
        let f = found.iter().find(|f| f.id == "site_root_missing:ghost.test").expect("missing-folder finding");
        assert!(!f.ignored && !f.problem.is_empty() && !f.cause.is_empty() && !f.fix.is_empty());

        core.dispatch(CoreCommand::IgnoreDiagnostic { id: f.id.clone(), ignore: true }).unwrap();
        assert!(findings(&core).iter().find(|x| x.id == f.id).unwrap().ignored);
        core.dispatch(CoreCommand::IgnoreDiagnostic { id: f.id.clone(), ignore: false }).unwrap();
        assert!(!findings(&core).iter().find(|x| x.id == f.id).unwrap().ignored);
    }

    #[test]
    fn composer_and_venv_info_come_from_the_project_files() {
        let (core, home) = test_core();
        let dir = home.paths.root().join("app");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("composer.json"), r#"{"require":{"monolog/monolog":"^3.0"}}"#).unwrap();
        let project = match core.dispatch(CoreCommand::RegisterProject { path: dir.display().to_string() }).unwrap() {
            CoreResponse::Project { project } => project,
            _ => panic!("expected Project"),
        };
        match core.dispatch(CoreCommand::GetComposerInfo { project_id: project.id.clone() }).unwrap() {
            CoreResponse::Composer { info } => assert_eq!(info.packages.len(), 1),
            _ => panic!("expected Composer"),
        }
        match core.dispatch(CoreCommand::GetVenv { project_id: project.id.clone() }).unwrap() {
            CoreResponse::Venv { info } => assert!(!info.exists),
            _ => panic!("expected Venv"),
        }
        assert!(core.dispatch(CoreCommand::RunComposer { project_id: project.id, action: "require".into(), target: Some("--evil".into()) }).is_err());
    }
    #[test]
    fn renaming_a_domain_keeps_its_settings_and_refuses_taken_names() {
        let (core, home) = test_core();
        let root = home.paths.root().join("site");
        std::fs::create_dir_all(&root).unwrap();
        let domain = |host: &str| Domain {
            hostname: host.into(),
            project_id: None,
            root: root.display().to_string(),
            kind: crate::domain::SiteKind::Static,
            https: false,
            redirect_https: false,
            wildcard: false,
            enabled: true,
            ownership: Ownership::Managed,
            app: None,
            blocks: Default::default(),
            generated_hashes: Default::default(),
        };
        core.dispatch(CoreCommand::AddDomain { domain: domain("old.test") }).unwrap();
        core.dispatch(CoreCommand::AddDomain { domain: domain("other.test") }).unwrap();

        let renamed = core.dispatch(CoreCommand::RenameDomain { hostname: "old.test".into(), new_hostname: "My-Shop.Local".into() }).unwrap();
        assert!(matches!(renamed, CoreResponse::Domain { domain } if domain.hostname == "my-shop.local" && domain.root == root.display().to_string()));
        assert!(core.dispatch(CoreCommand::RenameDomain { hostname: "my-shop.local".into(), new_hostname: "other.test".into() }).is_err());
        assert!(
            matches!(core.dispatch(CoreCommand::GetDomain { hostname: "my-shop.local".into() }), Ok(CoreResponse::Domain { .. })),
            "a failed rename leaves the site as it was"
        );
    }

    /// Laragon-style: scanning a folder gives each servable project `<name>.test`, a
    /// deleted automatic domain stays deleted, and new folders are picked up on the next sync.
    #[test]
    fn scanned_projects_get_automatic_domains() {
        let (core, home) = test_core();
        let www = home.paths.root().join("www");
        for (dir, file) in [("Shop", "index.php"), ("site", "index.html"), ("api", "package.json")] {
            std::fs::create_dir_all(www.join(dir)).unwrap();
            std::fs::write(www.join(dir).join(file), "x").unwrap();
        }
        core.dispatch(CoreCommand::ScanAndRegisterProjects { path: www.display().to_string() }).unwrap();

        let kinds = |core: &Core| -> Vec<(String, String)> {
            match core.dispatch(CoreCommand::ListDomains).unwrap() {
                CoreResponse::Domains { domains } => domains.into_iter().map(|d| (d.hostname, d.kind)).collect(),
                _ => panic!("expected Domains"),
            }
        };
        let mut found = kinds(&core);
        found.sort();
        assert_eq!(found, vec![("shop.test".to_string(), "php".to_string()), ("site.test".to_string(), "static".to_string())], "a Node project needs a dev server, so no automatic domain");

        core.dispatch(CoreCommand::RemoveDomain { hostname: "shop.test".into() }).unwrap();
        std::fs::create_dir_all(www.join("blog")).unwrap();
        std::fs::write(www.join("blog").join("index.php"), "x").unwrap();
        core.dispatch(CoreCommand::SyncAutoDomains).unwrap();
        let hosts: Vec<String> = kinds(&core).into_iter().map(|(h, _)| h).collect();
        assert!(hosts.contains(&"blog.test".to_string()), "a new folder is picked up");
        assert!(!hosts.contains(&"shop.test".to_string()), "a deleted automatic domain is not recreated");
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

    /// The Reverse Proxy Quick App points a domain at something running elsewhere.
    #[test]
    fn reverse_proxy_quick_app_plans_a_domain_to_another_host() {
        let (core, _home) = test_core();
        let values = BTreeMap::from([
            ("domain".to_string(), "portainer.test".to_string()),
            ("target_host".to_string(), "192.168.1.20".to_string()),
            ("target_port".to_string(), "9443".to_string()),
            ("target_https".to_string(), "true".to_string()),
        ]);
        let CoreResponse::QuickPlan { result } = core.dispatch(CoreCommand::PlanQuickApp { id: "reverse-proxy".into(), values }).unwrap() else { panic!() };
        assert!(result.ok, "{:?}", result.errors);
        let text = serde_json::to_string(&result).unwrap();
        for want in ["create_domain", "portainer.test", "192.168.1.20", "9443"] {
            assert!(text.contains(want), "plan is missing {want}: {text}");
        }
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
