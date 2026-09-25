//! The `CoreCommand` dispatcher (architecture decision 1): every front door — Tauri IPC,
//! the CLI, and later the local HTTP API — routes through this single enum + `dispatch` fn.
//! The UI/CLI never talks to individual managers directly.

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::custom_install::{CustomInstall, CustomInstallStore};
use crate::error::{CoreError, Diagnostic};
use crate::paths::AppPaths;
use crate::port::{self, PortStatus};
use crate::process::{CommandHistoryEntry, ProcessId, ProcessInfo, ProcessSpec, ProcessSupervisor};
use crate::project::{Project, ProjectDetail, ProjectStore};
use crate::resolver::ResolutionSource;
use crate::runtime::{CatalogEntry, RuntimeManager};
use crate::service::{ServiceManager, ServiceStatus};
use crate::settings::SettingsService;

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
    DbTools { tools: Vec<crate::dbtools::DbTool> },
    CustomInstalls { entries: Vec<CustomInstall> },
}

pub struct Core {
    settings: SettingsService,
    supervisor: Arc<ProcessSupervisor>,
    runtimes: Arc<RuntimeManager>,
    services: Arc<ServiceManager>,
    projects: ProjectStore,
    custom_installs: CustomInstallStore,
}

impl Core {
    pub fn new(settings: SettingsService, paths: AppPaths) -> Self {
        let projects = ProjectStore::load(&paths).expect("failed to load project store");
        let custom_installs = CustomInstallStore::load(&paths).expect("failed to load custom install store");
        let supervisor = Arc::new(ProcessSupervisor::new());
        let runtimes = Arc::new(RuntimeManager::new(paths.clone()));
        let services = Arc::new(ServiceManager::new(paths, runtimes.clone(), supervisor.clone()));
        Self::with_managers(settings, supervisor, runtimes, services, projects, custom_installs)
    }

    /// Used by the Tauri shell so it can hold its own clones of the managers and forward
    /// their events to the frontend, independent of the `Core` mutex.
    pub fn with_managers(
        settings: SettingsService,
        supervisor: Arc<ProcessSupervisor>,
        runtimes: Arc<RuntimeManager>,
        services: Arc<ServiceManager>,
        projects: ProjectStore,
        custom_installs: CustomInstallStore,
    ) -> Self {
        Self { settings, supervisor, runtimes, services, projects, custom_installs }
    }

    /// Route a `CoreCommand` to the right manager and return its result.
    /// Every failure is converted to a `Diagnostic` before it reaches the caller.
    pub fn dispatch(&mut self, command: CoreCommand) -> Result<CoreResponse, Diagnostic> {
        self.dispatch_inner(command).map_err(|e| Diagnostic::from(&e))
    }

    fn dispatch_inner(&mut self, command: CoreCommand) -> Result<CoreResponse, CoreError> {
        match command {
            CoreCommand::Ping => {
                tracing::info!(command = "ping");
                Ok(CoreResponse::Pong {
                    version: env!("CARGO_PKG_VERSION").to_string(),
                })
            }
            CoreCommand::GetSetting { key } => {
                tracing::info!(command = "get_setting", key = %key);
                Ok(CoreResponse::Setting {
                    value: self.settings.get(&key).cloned(),
                    key,
                })
            }
            CoreCommand::SetSetting { key, value } => {
                // Redact before it ever reaches the log, per §141 — settings can hold secrets.
                let logged_value = crate::logging::redact_value(&key, &value.to_string());
                tracing::info!(command = "set_setting", key = %key, value = %logged_value);
                self.settings.set(key, value)?;
                Ok(CoreResponse::Ok)
            }

            CoreCommand::StartProcess { spec } => {
                tracing::info!(command = "start_process", name = %spec.name);
                let id = self.supervisor.start(spec);
                Ok(CoreResponse::ProcessStarted { id })
            }
            CoreCommand::StopProcess { id } => {
                tracing::info!(command = "stop_process", id = id.0);
                self.supervisor.stop(id);
                Ok(CoreResponse::Ok)
            }
            CoreCommand::ListProcesses => Ok(CoreResponse::Processes {
                processes: self.supervisor.snapshot(),
            }),
            CoreCommand::GetProcessOutput { id } => Ok(CoreResponse::ProcessOutput {
                id,
                lines: self.supervisor.recent_output(id),
            }),

            CoreCommand::RunCommand { executable, args, cwd, timeout_ms } => {
                tracing::info!(command = "run_command", executable = %executable);
                let entry = self.supervisor.run_to_completion(
                    &executable,
                    &args,
                    cwd.as_deref(),
                    Duration::from_millis(timeout_ms),
                );
                Ok(CoreResponse::CommandResult { entry })
            }
            CoreCommand::ListCommandHistory => Ok(CoreResponse::CommandHistory {
                entries: self.supervisor.history(),
            }),

            CoreCommand::CheckPort { port: p } => {
                let status = port::check_port(p);
                Ok(CoreResponse::Port { port: p, status })
            }

            CoreCommand::ListRuntimeCatalog => Ok(CoreResponse::RuntimeCatalog {
                entries: self.runtimes.catalog(),
            }),
            CoreCommand::InstallRuntime { id, version } => {
                tracing::info!(command = "install_runtime", id = %id, version = %version);
                self.runtimes.install(&id, &version);
                Ok(CoreResponse::Ok)
            }

            CoreCommand::RegisterProject { path } => {
                tracing::info!(command = "register_project", path = %path);
                let project = self.projects.register(&path)?;
                Ok(CoreResponse::Project { project })
            }
            CoreCommand::ScanAndRegisterProjects { path } => {
                tracing::info!(command = "scan_and_register_projects", path = %path);
                let root = std::path::PathBuf::from(&path);
                if !root.is_dir() {
                    return Err(CoreError::InvalidProjectPath(path));
                }
                let candidates = crate::detection::scan_for_projects(&root);
                let mut registered = Vec::with_capacity(candidates.len());
                for candidate in candidates {
                    if let Some(candidate_str) = candidate.to_str() {
                        registered.push(self.projects.register(candidate_str)?);
                    }
                }
                Ok(CoreResponse::Projects { projects: registered })
            }
            CoreCommand::ListProjects => Ok(CoreResponse::Projects { projects: self.projects.list() }),
            CoreCommand::RemoveProject { id } => {
                self.projects.remove(&id)?;
                Ok(CoreResponse::Ok)
            }
            CoreCommand::GetProjectDetail { id } => {
                let project = self.projects.get(&id).ok_or_else(|| CoreError::InvalidProjectPath(id.clone()))?;
                let detail = crate::project::build_detail(&project, &self.runtimes, &self.settings, &self.custom_installs);
                Ok(CoreResponse::ProjectDetail { detail: Box::new(detail) })
            }
            CoreCommand::RunInProject { project_id, runtime_id, args } => {
                let project = self
                    .projects
                    .get(&project_id)
                    .ok_or_else(|| CoreError::InvalidProjectPath(project_id.clone()))?;
                let detail = crate::project::build_detail(&project, &self.runtimes, &self.settings, &self.custom_installs);
                let resolved = detail.resolved.iter().find(|r| r.id == runtime_id);

                let Some(r) = resolved.filter(|r| r.installed_version.is_some()) else {
                    return Err(CoreError::InvalidProjectPath(format!(
                        "{runtime_id} is not resolved to an installed version for this project"
                    )));
                };
                let bin_dir = r.bin_dir.clone();

                let binary = if r.source == ResolutionSource::Custom {
                    // A custom install's own path IS the binary — bin_dir above is just
                    // its parent directory (for PATH), not something to re-derive from.
                    self.custom_installs
                        .resolve(&runtime_id, r.requested_version.as_deref())
                        .map(|c| std::path::PathBuf::from(&c.path))
                        .ok_or_else(|| CoreError::InvalidProjectPath(format!("{runtime_id} custom install vanished")))?
                } else {
                    let version = r.installed_version.clone().unwrap();
                    self.runtimes.binary_path(&runtime_id, &version).ok_or_else(|| {
                        CoreError::InvalidProjectPath(format!("{runtime_id} {version} binary missing on disk"))
                    })?
                };

                // Prepend the resolved runtime's own directory to PATH so a command this
                // process shells out to (e.g. `npm` calling back into `node`) also finds
                // the project-selected version, not whatever's on the system PATH (§19).
                let system_path = std::env::var("PATH").unwrap_or_default();
                let new_path = match bin_dir {
                    Some(dir) => format!("{dir};{system_path}"),
                    None => system_path,
                };

                let version_for_log = r.installed_version.clone().unwrap_or_default();
                tracing::info!(command = "run_in_project", project = %project.name, runtime = %runtime_id, version = %version_for_log);
                let id = self.supervisor.start(ProcessSpec {
                    name: format!("{}: {} {}", project.name, runtime_id, args.join(" ")),
                    executable: binary.display().to_string(),
                    args,
                    cwd: Some(project.path.clone()),
                    env: vec![("PATH".to_string(), new_path)],
                    restart: None,
                });
                Ok(CoreResponse::ProcessStarted { id })
            }

            CoreCommand::ListServices => Ok(CoreResponse::Services { services: self.services.list() }),
            CoreCommand::StartService { id } => {
                tracing::info!(command = "start_service", id = %id);
                self.services.start(&id).map_err(CoreError::ServiceError)?;
                Ok(CoreResponse::Ok)
            }
            CoreCommand::StopService { id } => {
                tracing::info!(command = "stop_service", id = %id);
                self.services.stop(&id);
                Ok(CoreResponse::Ok)
            }
            CoreCommand::CreateMysqlDatabase { name } => {
                tracing::info!(command = "create_mysql_database", name = %name);
                // Identifiers can't be parameterized in SQL — reject anything that isn't a
                // plain name instead of building a query string from raw user input.
                if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') || name.is_empty() {
                    return Err(CoreError::ServiceError(
                        "database name must be alphanumeric/underscore only".into(),
                    ));
                }
                self.services
                    .run_mysql_client(&format!("CREATE DATABASE IF NOT EXISTS `{name}`"))
                    .map_err(CoreError::ServiceError)?;
                Ok(CoreResponse::Ok)
            }

            CoreCommand::SetSecret { key, value } => {
                // Never log the value itself, only that a secret was set (§141).
                tracing::info!(command = "set_secret", key = %key);
                crate::secrets::set_secret(&key, &value).map_err(CoreError::ServiceError)?;
                Ok(CoreResponse::Ok)
            }
            CoreCommand::GetSecret { key } => {
                let value = crate::secrets::get_secret(&key).map_err(CoreError::ServiceError)?;
                Ok(CoreResponse::Secret { key, value })
            }
            CoreCommand::DeleteSecret { key } => {
                crate::secrets::delete_secret(&key).map_err(CoreError::ServiceError)?;
                Ok(CoreResponse::Ok)
            }

            CoreCommand::ListDbTools => {
                // A user-pinned custom path always wins over auto-detection (§126).
                let mut tools = crate::dbtools::detect_db_tools();
                for tool in &mut tools {
                    if let Some(custom) = self.custom_installs.resolve(&tool.id, None) {
                        tool.found_path = Some(custom.path.clone());
                    }
                }
                Ok(CoreResponse::DbTools { tools })
            }
            CoreCommand::OpenDbTool { id } => {
                let path = self
                    .custom_installs
                    .resolve(&id, None)
                    .map(|c| c.path.clone())
                    .or_else(|| crate::dbtools::detect_db_tools().into_iter().find(|t| t.id == id).and_then(|t| t.found_path))
                    .ok_or_else(|| CoreError::ServiceError(format!("{id} was not found on this system")))?;
                // A standalone GUI app the user drives themselves — not managed/supervised
                // (no output capture, no kill-on-drop tie to DevForge's own lifecycle).
                std::process::Command::new(&path).spawn().map_err(|e| CoreError::ServiceError(e.to_string()))?;
                Ok(CoreResponse::Ok)
            }

            CoreCommand::SetCustomInstall { id, label, path } => {
                tracing::info!(command = "set_custom_install", id = %id, label = %label);
                self.custom_installs.set(&id, &label, &path)?;
                Ok(CoreResponse::Ok)
            }
            CoreCommand::RemoveCustomInstall { id, label } => {
                self.custom_installs.remove(&id, &label)?;
                Ok(CoreResponse::Ok)
            }
            CoreCommand::ListCustomInstalls => {
                Ok(CoreResponse::CustomInstalls { entries: self.custom_installs.list() })
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
        let (mut core, _home) = test_core();
        let resp = core.dispatch(CoreCommand::Ping).unwrap();
        match resp {
            CoreResponse::Pong { version } => assert!(!version.is_empty()),
            _ => panic!("expected Pong"),
        }
    }

    #[test]
    fn set_then_get_setting_round_trips_through_dispatch() {
        let (mut core, _home) = test_core();
        core.dispatch(CoreCommand::SetSetting {
            key: "editor".into(),
            value: Value::String("vscode".into()),
        })
        .unwrap();

        let resp = core
            .dispatch(CoreCommand::GetSetting {
                key: "editor".into(),
            })
            .unwrap();
        match resp {
            CoreResponse::Setting { value, .. } => {
                assert_eq!(value, Some(Value::String("vscode".into())))
            }
            _ => panic!("expected Setting"),
        }
    }

    #[test]
    fn get_missing_setting_returns_none_not_error() {
        let (mut core, _home) = test_core();
        let resp = core
            .dispatch(CoreCommand::GetSetting {
                key: "does-not-exist".into(),
            })
            .unwrap();
        match resp {
            CoreResponse::Setting { value, .. } => assert_eq!(value, None),
            _ => panic!("expected Setting"),
        }
    }
}
