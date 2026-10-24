use std::sync::Arc;
use std::time::Duration;

use super::super::types::*;
use super::super::{file_name, helper_service_status, report, Core};
use crate::app::Host;
use crate::error::CoreError;
use crate::port;
use crate::process::ProcessSpec;
use crate::quickapp::commands::quick_command_from_line;
use crate::resolver::ResolutionSource;

impl Core {
    pub(crate) fn dispatch_services(&self, command: CoreCommand) -> Result<Result<CoreResponse, CoreError>, CoreCommand> {
        use CoreCommand as C;
        use CoreResponse as R;
        let i = &self.inner;
        #[allow(unreachable_patterns)]
        let mut unhandled = None;
        let outcome: Result<CoreResponse, CoreError> = (|| {
            let res = match command {
            C::RunInProject {
                project_id,
                runtime_id,
                args,
            } => {
                let project = i
                    .projects
                    .lock()
                    .unwrap()
                    .get(&project_id)
                    .ok_or_else(|| CoreError::InvalidProjectPath(project_id.clone()))?;
                let detail = i
                    .project_detail(&project_id)
                    .ok_or_else(|| CoreError::InvalidProjectPath(project_id.clone()))?;
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
                    i.custom_installs
                        .lock()
                        .unwrap()
                        .resolve(&runtime_id, r.requested_version.as_deref())
                        .map(|c| std::path::PathBuf::from(&c.path))
                        .ok_or_else(|| {
                            CoreError::InvalidProjectPath(format!(
                                "{runtime_id} custom install vanished"
                            ))
                        })?
                } else {
                    let version = r.installed_version.clone().unwrap();
                    i.runtimes
                        .binary_path(&runtime_id, &version)
                        .ok_or_else(|| {
                            CoreError::InvalidProjectPath(format!(
                                "{runtime_id} {version} binary missing on disk"
                            ))
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

            C::ListServices => Ok(R::Services {
                services: i.services.list(),
            }),
            C::ListCustomServices => Ok(R::CustomServices {
                services: i.services.list_custom(),
            }),
            C::SaveCustomService { service } => {
                tracing::info!(command = "save_custom_service", name = %service.name);
                Ok(R::CustomService {
                    service: Box::new(
                        i.services
                            .save_custom(service)
                            .map_err(CoreError::ServiceError)?,
                    ),
                })
            }
            C::RemoveCustomService { id } => {
                i.services
                    .remove_custom(&id)
                    .map_err(CoreError::ServiceError)?;
                Ok(R::Ok)
            }
            C::StartService { id } => {
                tracing::info!(command = "start_service", id = %id);
                if crate::web::SERVER_IDS.contains(&id.as_str()) {
                    // A web server renders the sites assigned to it, then binds its ports.
                    i.apply_web_server(&id, &[]).map(|_| R::Ok)?;
                } else {
                    i.services.start(&id).map_err(CoreError::ServiceError)?;
                }
                Ok(R::Ok)
            }
            C::StopService { id } => {
                if crate::web::SERVER_IDS.contains(&id.as_str()) {
                    i.web.stop_server(&id);
                } else {
                    i.services.stop(&id).map_err(CoreError::ServiceError)?;
                }
                Ok(R::Ok)
            }

            C::StopAll => {
                tracing::info!(command = "stop_all");
                // Order matters: the specific owners go first so the sweep below only
                // has to catch strays (a scheduled job re-spawned mid-loop, a project
                // command with no owner left to stop it).
                i.stop_all_tunnels();
                i.stop_all_workers();
                i.web.stop();
                let mut failed = Vec::new();
                for service in i.services.list() {
                    if !service.running {
                        continue;
                    }
                    if let Err(e) = i.services.stop(&service.id) {
                        // A stop that fails leaves the service running, which is
                        // exactly why the mark would stay green — so it is collected
                        // and reported instead of being dropped.
                        tracing::warn!(id = %service.id, error = %e, "stop all: did not stop");
                        failed.push(format!("{}: {e}", service.id));
                    }
                }
                // The sweep is what makes "stop all" mean all. A process the OS
                // refuses to kill is the one case where the mark staying green is
                // correct, so it is named rather than swallowed.
                if !i.supervisor.stop_all_and_wait(Duration::from_secs(10)) {
                    for p in i.supervisor.snapshot() {
                        if matches!(
                            p.state,
                            crate::process::ProcessState::Starting
                                | crate::process::ProcessState::Running
                                | crate::process::ProcessState::Restarting
                        ) {
                            failed.push(p.name);
                        }
                    }
                }
                Ok(R::StoppedEverything {
                    still_running: failed,
                })
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
                    .or_else(|| {
                        crate::dbtools::detect_db_tools()
                            .into_iter()
                            .find(|t| t.id == id)
                            .and_then(|t| t.found_path)
                    })
                    .ok_or_else(|| {
                        CoreError::ServiceError(format!("{id} was not found on this system"))
                    })?;
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
            C::ListPhpExtensions { version } => Ok(R::PhpExtensions {
                report: i.php.extensions_report(&version),
            }),
            C::SetPhpExtension {
                version,
                name,
                enabled,
            } => {
                tracing::info!(command = "set_php_extension", version = %version, name = %name, enabled);
                i.php
                    .set_extension(&version, &name, enabled)
                    .map_err(CoreError::ServiceError)?;
                Ok(R::PhpExtensions {
                    report: i.php.extensions_report(&version),
                })
            }
            C::InstallPhpExtension { version, name } => {
                tracing::info!(command = "install_php_extension", version = %version, name = %name);
                i.php
                    .install_extension(&version, &name)
                    .map_err(CoreError::ServiceError)?;
                Ok(R::PhpExtensions {
                    report: i.php.extensions_report(&version),
                })
            }
            C::ListPeclPackages => Ok(R::Names {
                names: i.php.pecl_packages().map_err(CoreError::ServiceError)?,
            }),
            C::ListCustomInstalls => Ok(R::CustomInstalls {
                entries: i.custom_installs.lock().unwrap().list(),
            }),

            // ---- Stage 6
            other => {
                    unhandled = Some(other);
                    return Ok(R::Ok);
                }
            };
            res
        })();
        if let Some(cmd) = unhandled {
            Err(cmd)
        } else {
            Ok(outcome)
        }
    }
}
