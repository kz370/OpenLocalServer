//! The `CoreCommand` dispatcher (architecture decision 1): every front door — Tauri IPC,
//! the CLI, and later the local HTTP API — routes through this single enum + `dispatch` fn.
//! The UI/CLI never talks to individual managers directly.

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{CoreError, Diagnostic};
use crate::paths::AppPaths;
use crate::port::{self, PortStatus};
use crate::process::{CommandHistoryEntry, ProcessId, ProcessInfo, ProcessSpec, ProcessSupervisor};
use crate::project::{Project, ProjectDetail, ProjectStore};
use crate::runtime::{CatalogEntry, RuntimeManager};
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
}

pub struct Core {
    settings: SettingsService,
    supervisor: Arc<ProcessSupervisor>,
    runtimes: Arc<RuntimeManager>,
    projects: ProjectStore,
}

impl Core {
    pub fn new(settings: SettingsService, paths: AppPaths) -> Self {
        let projects = ProjectStore::load(&paths).expect("failed to load project store");
        Self::with_managers(settings, Arc::new(ProcessSupervisor::new()), Arc::new(RuntimeManager::new(paths)), projects)
    }

    /// Used by the Tauri shell so it can hold its own clones of the managers and forward
    /// their events to the frontend, independent of the `Core` mutex.
    pub fn with_managers(
        settings: SettingsService,
        supervisor: Arc<ProcessSupervisor>,
        runtimes: Arc<RuntimeManager>,
        projects: ProjectStore,
    ) -> Self {
        Self { settings, supervisor, runtimes, projects }
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
                let detail = crate::project::build_detail(&project, &self.runtimes, &self.settings);
                Ok(CoreResponse::ProjectDetail { detail: Box::new(detail) })
            }
            CoreCommand::RunInProject { project_id, runtime_id, args } => {
                let project = self
                    .projects
                    .get(&project_id)
                    .ok_or_else(|| CoreError::InvalidProjectPath(project_id.clone()))?;
                let detail = crate::project::build_detail(&project, &self.runtimes, &self.settings);
                let resolved = detail
                    .resolved
                    .iter()
                    .find(|r| r.id == runtime_id)
                    .and_then(|r| r.installed_version.as_ref().map(|v| (v.clone(), r.bin_dir.clone())));

                let Some((version, bin_dir)) = resolved else {
                    return Err(CoreError::InvalidProjectPath(format!(
                        "{runtime_id} is not resolved to an installed version for this project"
                    )));
                };
                let Some(binary) = self.runtimes.binary_path(&runtime_id, &version) else {
                    return Err(CoreError::InvalidProjectPath(format!("{runtime_id} {version} binary missing on disk")));
                };

                // Prepend the resolved runtime's own directory to PATH so a command this
                // process shells out to (e.g. `npm` calling back into `node`) also finds
                // the project-selected version, not whatever's on the system PATH (§19).
                let system_path = std::env::var("PATH").unwrap_or_default();
                let new_path = match bin_dir {
                    Some(dir) => format!("{dir};{system_path}"),
                    None => system_path,
                };

                tracing::info!(command = "run_in_project", project = %project.name, runtime = %runtime_id, version = %version);
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
