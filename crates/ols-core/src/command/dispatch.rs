use std::sync::Arc;
use std::time::Duration;

use super::types::*;
use super::{file_name, helper_service_status, report, Core};
use crate::app::Host;
use crate::error::CoreError;
use crate::port;
use crate::process::ProcessSpec;
use crate::quickapp::commands::quick_command_from_line;
use crate::resolver::ResolutionSource;

impl Core {
    pub(crate) fn dispatch_inner(&self, command: CoreCommand) -> Result<CoreResponse, CoreError> {
        use CoreCommand as C;
        use CoreResponse as R;
        let i = &self.inner;
        match command {
            C::Ping => Ok(R::Pong {
                version: env!("CARGO_PKG_VERSION").to_string(),
            }),
            C::GetSetting { key } => {
                let value = if key == "paths.sites_dir" {
                    Some(serde_json::Value::String(
                        i.sites_dir().display().to_string(),
                    ))
                } else if key == "paths.backups_dir" {
                    // The folder exports land in by default, resolved rather than
                    // concatenated in the UI: it follows a moved or portable install, and
                    // the answer is the one `backups_dir` actually uses.
                    Some(serde_json::Value::String(
                        i.paths.backups_dir().display().to_string(),
                    ))
                } else if key == "quickapps.projects_dir" {
                    // Answer with the folder it resolves to, never the stored form: the
                    // UI shows this value in a text input and offers it as the parent for
                    // a new project, so a stored `sites/shop` would be shown to the user
                    // as if it were a path on disk.
                    Some(serde_json::Value::String(
                        i.default_projects_dir().display().to_string(),
                    ))
                } else if key == crate::app::PROJECT_IGNORED_FOLDERS {
                    Some(serde_json::json!(i.ignored_folders()))
                } else {
                    i.settings.lock().unwrap().get(&key).cloned()
                };
                Ok(R::Setting { value, key })
            }
            C::SetSetting { key, value } => {
                // Redact before it ever reaches the log, per §141 — settings can hold secrets.
                let logged_value = crate::logging::redact_value(&key, &value.to_string());
                tracing::info!(command = "set_setting", key = %key, value = %logged_value);
                if key == crate::app::PROJECT_IGNORED_FOLDERS {
                    let has_valid = match value.as_array() {
                        Some(arr) => arr
                            .iter()
                            .any(|v| v.as_str().is_some_and(|folder| !folder.trim().is_empty())),
                        None => false,
                    };
                    if !has_valid {
                        return Err(CoreError::failed_fix(
                            "Cannot empty ignored folders.",
                            "At least one ignored folder must remain so deleted sites and excluded projects have a designated directory.",
                            format!(
                                "Keep '{}' and at least one valid folder name in the ignored folders list.",
                                crate::app::OLS_EXCLUDED_FOLDER
                            ),
                        ));
                    }
                    let includes_required = value.as_array().is_some_and(|arr| {
                        arr.iter().any(|v| {
                            v.as_str().is_some_and(|folder| {
                                folder
                                    .trim()
                                    .eq_ignore_ascii_case(crate::app::OLS_EXCLUDED_FOLDER)
                            })
                        })
                    });
                    if !includes_required {
                        return Err(CoreError::failed_fix(
                            "Cannot remove the required ignored folder.",
                            format!(
                                "'{}' is needed to keep deleted sites and projects out of scans.",
                                crate::app::OLS_EXCLUDED_FOLDER
                            ),
                            format!(
                                "Keep '{}' in the ignored folders list.",
                                crate::app::OLS_EXCLUDED_FOLDER
                            ),
                        ));
                    }
                }
                if let Some(id) = key
                    .strip_prefix("runtime.")
                    .and_then(|k| k.strip_suffix(".global"))
                {
                    if matches!(id, "nginx" | "apache" | "caddy") && i.web.is_running() {
                        return Err(CoreError::ServiceError(
                            "Stop the web server before changing its runtime version.".into(),
                        ));
                    }
                    if matches!(
                        id,
                        "mariadb" | "postgres" | "mongodb" | "redis" | "memcached" | "mailpit"
                    ) && i.services.is_running(id)
                    {
                        return Err(CoreError::ServiceError(format!(
                            "Stop {id} before changing its runtime version."
                        )));
                    }
                }
                // A default projects folder inside the app is stored relative, so a
                // renamed or moved install keeps the same default. A blank value still
                // means "unset" and falls back to the sites folder, so it is left alone.
                let value = if key == "quickapps.projects_dir" {
                    match value.as_str() {
                        Some(dir) if !dir.trim().is_empty() => serde_json::Value::String(
                            i.paths.encode_root(std::path::Path::new(dir.trim())),
                        ),
                        _ => value,
                    }
                } else {
                    value
                };
                i.settings.lock().unwrap().set(key.clone(), value.clone())?;
                if let (Some(id), Some(version)) = (
                    key.strip_prefix("runtime.")
                        .and_then(|k| k.strip_suffix(".global")),
                    value.as_str(),
                ) {
                    // Keep unmanaged/custom PHP and Node preferences in settings, while only
                    // applying managed catalog versions to the runtime manager.
                    let _ = i.runtimes.set_preferred(id, version);
                }
                Ok(R::Ok)
            }

            C::StartProcess { spec } => {
                tracing::info!(command = "start_process", name = %spec.name);
                Ok(R::ProcessStarted {
                    id: i.supervisor.start(spec),
                })
            }
            C::StopProcess { id } => {
                i.supervisor.stop(id);
                Ok(R::Ok)
            }
            C::ListProcesses => Ok(R::Processes {
                processes: i.supervisor.snapshot(),
            }),
            C::GetProcessOutput { id } => Ok(R::ProcessOutput {
                id,
                lines: i.supervisor.recent_output(id),
            }),

            C::RunCommand {
                executable,
                args,
                cwd,
                timeout_ms,
            } => {
                tracing::info!(command = "run_command", executable = %executable);
                let entry = i.supervisor.run_to_completion(
                    &executable,
                    &args,
                    cwd.as_deref(),
                    Duration::from_millis(timeout_ms),
                );
                Ok(R::CommandResult { entry })
            }
            C::ListCommandHistory => Ok(R::CommandHistory {
                entries: i.supervisor.history(),
            }),

            C::CheckPort { port: p } => Ok(R::Port {
                port: p,
                status: port::check_port(p),
            }),

            C::ListRuntimeCatalog => Ok(R::RuntimeCatalog {
                entries: i.runtimes.catalog(),
            }),
            C::RefreshRuntimeCatalog { id } => {
                i.runtimes
                    .refresh_online_catalog(&id)
                    .map_err(CoreError::ServiceError)?;
                Ok(R::RuntimeCatalog {
                    entries: i.runtimes.catalog(),
                })
            }
            C::InstallRuntime { id, version } => {
                tracing::info!(command = "install_runtime", id = %id, version = %version);
                i.runtimes.install(&id, &version);
                Ok(R::Ok)
            }
            C::PauseRuntime {
                id,
                version,
                paused,
            } => {
                tracing::info!(command = "pause_runtime", id = %id, version = %version, paused);
                i.runtimes
                    .pause_install(&id, &version, paused)
                    .map_err(CoreError::ServiceError)?;
                Ok(R::Ok)
            }
            C::CancelRuntime { id, version } => {
                tracing::info!(command = "cancel_runtime", id = %id, version = %version);
                i.runtimes
                    .cancel_install(&id, &version)
                    .map_err(CoreError::ServiceError)?;
                Ok(R::Ok)
            }
            C::RemoveRuntime { id, version } => {
                if i.services.is_running(&id) {
                    return Err(CoreError::ServiceError(format!(
                        "Stop {id} before removing a version."
                    )));
                }
                if matches!(id.as_str(), "nginx" | "apache" | "caddy") && i.web.is_running() {
                    return Err(CoreError::ServiceError(
                        "Stop the web server before removing a server version.".into(),
                    ));
                }
                if id == "php" && i.web.is_running() {
                    return Err(CoreError::ServiceError(
                        "Stop the web server before removing a PHP version.".into(),
                    ));
                }
                let was_default =
                    i.runtimes.catalog().iter().any(|entry| {
                        entry.id == id && entry.version == version && entry.is_default
                    });
                let last_version = i.runtimes.installed_versions(&id).len() == 1;
                let pref_key = format!("runtime.{id}.global");
                let old_pref = if was_default && last_version {
                    let mut settings = i.settings.lock().unwrap();
                    let old = settings.get(&pref_key).cloned();
                    settings.set(pref_key.clone(), serde_json::Value::Null)?;
                    old
                } else {
                    None
                };
                if let Err(error) = i.runtimes.remove(&id, &version) {
                    if was_default && last_version {
                        let mut settings = i.settings.lock().unwrap();
                        if let Some(old) = old_pref {
                            let _ = settings.set(pref_key, old);
                        } else {
                            let _ = settings.set(pref_key, serde_json::Value::Null);
                        }
                    }
                    return Err(CoreError::ServiceError(error));
                }
                Ok(R::Ok)
            }

            C::RegisterProject { path } => {
                tracing::info!(command = "register_project", path = %path);
                let project = i.projects.lock().unwrap().register(&path)?;
                let _ = i.clear_project_skip(&project.path);
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
                let ignored = i.ignored_folders();
                let candidates = crate::detection::scan_for_projects_with_ignored(&root, &ignored);
                // Read the skips before taking the project store, then judge every candidate
                // the same way the watcher does: a folder the user deleted stays deleted
                // whichever path spelling this scan happened to walk.
                let skipped = i.project_skips();
                let mut projects = i.projects.lock().unwrap();
                let mut registered = Vec::with_capacity(candidates.len());
                for candidate in candidates {
                    if let Some(s) = candidate.to_str() {
                        // Bulk scans don't resurrect folders removed from the list.
                        if crate::app::skip_matches(&skipped, s) {
                            continue;
                        }
                        registered.push(projects.register(s)?);
                    }
                }
                drop(projects);
                i.remember_projects_root(&path)?;
                if let Err(e) = i.sync_auto_domains() {
                    tracing::warn!(error = %e, "automatic domains could not be synced");
                }
                Ok(R::Projects {
                    projects: registered,
                })
            }
            C::SyncAutoDomains => Ok(R::Count {
                count: i.sync_auto_domains()?,
            }),
            C::ListProjects => {
                // Same outside-change re-read as `ListDomains`: a folder registered from
                // the right-click menu is written by another process.
                i.reload_external_changes();
                let mut projects = i.projects.lock().unwrap().list();
                for p in &mut projects {
                    p.icon_image = crate::app::read_site_icon_image(&p.path);
                }
                Ok(R::Projects { projects })
            }
            C::RemoveProject { id } => {
                i.remove_project(&id)?;
                Ok(R::Ok)
            }
            C::DeleteProject { id } => {
                i.delete_project(&id)?;
                Ok(R::Ok)
            }
            C::RenameIgnoredFolder { old_name, new_name } => {
                i.rename_ignored_folder(&old_name, &new_name)?;
                Ok(R::Ok)
            }
            C::ListDeletedItems => Ok(R::DeletedItems {
                items: i.list_deleted_items(),
            }),
            C::ForgetDeletedItem { value } => {
                let value = value.trim();
                if value.is_empty() {
                    return Err(CoreError::failed_fix(
                        "That deleted item could not be cleared.",
                        "No folder or site name was given to remove from the deletion history.",
                        "Reload Settings and try again.",
                    ));
                }
                i.forget_deleted_item(value)?;
                Ok(R::DeletedItems {
                    items: i.list_deleted_items(),
                })
            }
            C::ClearDeletedItems => {
                i.clear_deleted_items()?;
                Ok(R::DeletedItems {
                    items: i.list_deleted_items(),
                })
            }
            C::GetProjectDetail { id } => {
                // The store has no such id, which in practice means the site still points at a
                // project that was removed. `InvalidProjectPath` claimed the id "is not a
                // directory on disk", which is a different problem and left the user guessing.
                let detail = i.project_detail(&id).ok_or_else(|| {
                    CoreError::failed_fix(
                        "This site has no project any more.",
                        format!("No project is registered under the id {id}, so its settings cannot be read."),
                        "Register the folder again on the Projects page, or pick another project in this site's settings.",
                    )
                })?;
                Ok(R::ProjectDetail {
                    detail: Box::new(detail),
                })
            }
            C::InspectSiteFolder { path } => {
                let p = std::path::Path::new(&path);
                Ok(R::SiteFolderInspection {
                    inspection: crate::detection::inspect_site_folder(p),
                })
            }
            C::ListWpProjects => Ok(R::WpProjects {
                projects: i.wp_projects(),
            }),
            C::WpSignIn {
                project_id,
                hostname,
                theme,
            } => {
                let signin = i.wp_signin(&project_id, hostname.as_deref(), theme.as_deref())?;
                Ok(R::WpSignIn {
                    url: signin.url,
                    expires_at: signin.expires_at,
                })
            }
            C::WpSignInRevoke { project_id } => {
                i.wp_signin_revoke(&project_id)?;
                Ok(R::Ok)
            }
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
            C::GetWebStatus => Ok(R::WebStatus {
                status: Box::new(
                    i.web
                        .status(&i.web_config(), &i.domains.lock().unwrap().list()),
                ),
            }),
            C::GetWebConfig => Ok(R::WebConfig {
                config: i.web_config(),
            }),
            C::ListDomains => {
                // Picks up a site the Explorer right-click menu or the `ols` command line
                // added while the app was running: that write came from another process,
                // so the in-memory store has to be re-read before it is reported.
                if i.reload_external_changes() {
                    tracing::info!("re-read the stores after an outside change");
                }
                Ok(R::Domains {
                    domains: i.domain_summaries(),
                })
            }
            C::GetDomain { hostname } => {
                let d = i.domains.lock().unwrap().get(&hostname).ok_or_else(|| {
                    CoreError::DomainError(format!("{hostname} is not a known domain"))
                })?;
                Ok(R::Domain {
                    domain: Box::new(d),
                })
            }
            C::AddDomain { domain } => {
                tracing::info!(command = "add_domain", hostname = %domain.hostname);
                Ok(R::Domain {
                    domain: Box::new(i.add_domain(domain)?),
                })
            }
            C::UpdateDomain { domain } => Ok(R::Domain {
                domain: Box::new(i.update_domain(domain)?),
            }),
            C::RemoveDomain { hostname } => {
                i.remove_domain(&hostname)?;
                Ok(R::Ok)
            }
            C::DeleteSite { hostname } => {
                i.delete_site(&hostname)?;
                Ok(R::Ok)
            }
            C::SetDomainEnabled { hostname, enabled } => {
                i.set_domain_enabled(&hostname, enabled)?;
                Ok(R::Ok)
            }
            C::BulkSetDomainsEnabled { hostnames, enabled } => {
                tracing::info!(
                    command = "bulk_set_domains_enabled",
                    sites = hostnames.len(),
                    enabled
                );
                let out = i.set_domains_enabled(&hostnames, enabled)?;
                Ok(R::Bulk {
                    changed: out.changed,
                    skipped: out.skipped,
                })
            }
            C::BulkRemoveDomains { hostnames } => {
                tracing::info!(command = "bulk_remove_domains", sites = hostnames.len());
                let out = i.remove_domains(&hostnames)?;
                Ok(R::Bulk {
                    changed: out.changed,
                    skipped: out.skipped,
                })
            }
            C::BulkDeleteSites { hostnames } => {
                tracing::info!(command = "bulk_delete_sites", sites = hostnames.len());
                let out = i.delete_sites(&hostnames)?;
                Ok(R::Bulk {
                    changed: out.changed,
                    skipped: out.skipped,
                })
            }
            C::BulkSetDomainServer { hostnames, server } => {
                tracing::info!(command = "bulk_set_domain_server", sites = hostnames.len(), server = ?server);
                let out = i.set_domain_servers(&hostnames, server.as_deref())?;
                Ok(R::Bulk {
                    changed: out.changed,
                    skipped: out.skipped,
                })
            }
            C::RenameDomain {
                hostname,
                new_hostname,
            } => {
                tracing::info!(command = "rename_domain", from = %hostname, to = %new_hostname);
                Ok(R::Domain {
                    domain: Box::new(i.rename_domain(&hostname, &new_hostname)?),
                })
            }
            C::SetDomainIcon { hostname, icon } => {
                let mut domains = i.domains.lock().unwrap();
                let mut d = domains.get(&hostname).ok_or_else(|| {
                    CoreError::DomainError(format!("{hostname} is not a known domain"))
                })?;
                let clean_icon = icon.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
                d.icon = clean_icon;
                domains.update(d)?;
                Ok(R::Ok)
            }
            C::SetSiteIconImage {
                hostname,
                folder,
                webp_base64,
            } => {
                let target_dir = if let Some(f) = folder.filter(|s| !s.trim().is_empty()) {
                    std::path::PathBuf::from(f)
                } else if let Some(h) = hostname {
                    let d = i.domains.lock().unwrap().get(&h).ok_or_else(|| {
                        CoreError::DomainError(format!("{h} is not a known domain"))
                    })?;
                    let projects = i.projects.lock().unwrap().list();
                    std::path::PathBuf::from(crate::app::site_folder(&d, &projects))
                } else {
                    return Err(CoreError::DomainError(
                        "Neither hostname nor folder was provided".into(),
                    ));
                };

                if !target_dir.is_dir() {
                    return Err(CoreError::DomainError(format!(
                        "Folder does not exist: {}",
                        target_dir.display()
                    )));
                }

                let raw_b64 = if let Some(idx) = webp_base64.find(',') {
                    &webp_base64[idx + 1..]
                } else {
                    &webp_base64
                };
                let bytes = crate::app::from_base64(raw_b64.trim()).map_err(|e| {
                    CoreError::DomainError(format!("Invalid base64 image data: {e}"))
                })?;

                if bytes.len() > 10 * 1024 * 1024 {
                    return Err(CoreError::DomainError(
                        "Image data exceeds 10MB limit".into(),
                    ));
                }

                let icon_path = target_dir.join("ols_site_icon.webp");
                std::fs::write(&icon_path, &bytes).map_err(|e| {
                    CoreError::DomainError(format!("Failed to write ols_site_icon.webp: {e}"))
                })?;
                Ok(R::Ok)
            }
            C::RemoveSiteIconImage { hostname, folder } => {
                let target_dir = if let Some(f) = folder.filter(|s| !s.trim().is_empty()) {
                    std::path::PathBuf::from(f)
                } else if let Some(h) = hostname {
                    let d = i.domains.lock().unwrap().get(&h).ok_or_else(|| {
                        CoreError::DomainError(format!("{h} is not a known domain"))
                    })?;
                    let projects = i.projects.lock().unwrap().list();
                    std::path::PathBuf::from(crate::app::site_folder(&d, &projects))
                } else {
                    return Err(CoreError::DomainError(
                        "Neither hostname nor folder was provided".into(),
                    ));
                };

                let icon_path = target_dir.join("ols_site_icon.webp");
                if icon_path.exists() {
                    let _ = std::fs::remove_file(icon_path);
                }
                Ok(R::Ok)
            }
            C::ReadImageFile { path } => {
                let p = std::path::Path::new(&path);
                if !p.is_file() {
                    return Err(CoreError::DomainError(format!(
                        "Image file does not exist: {path}"
                    )));
                }
                let bytes = std::fs::read(p)
                    .map_err(|e| CoreError::DomainError(format!("Failed to read file: {e}")))?;
                if bytes.len() > 20 * 1024 * 1024 {
                    return Err(CoreError::DomainError(
                        "Image file exceeds 20MB limit".into(),
                    ));
                }
                let ext = p
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                let mime = match ext.as_str() {
                    "webp" => "image/webp",
                    "png" => "image/png",
                    "jpg" | "jpeg" => "image/jpeg",
                    "svg" => "image/svg+xml",
                    "gif" => "image/gif",
                    "bmp" => "image/bmp",
                    "ico" => "image/x-icon",
                    _ => "application/octet-stream",
                };
                let data_url = format!("data:{mime};base64,{}", crate::app::to_base64(&bytes));
                Ok(R::ImageFile { data_url })
            }
            C::DuplicateDomain {
                hostname,
                new_hostname,
            } => Ok(R::Domain {
                domain: Box::new(i.duplicate_domain(&hostname, &new_hostname)?),
            }),
            C::SuggestDomain {
                project_id,
                template,
            } => {
                let project = i
                    .projects
                    .lock()
                    .unwrap()
                    .get(&project_id)
                    .ok_or_else(|| CoreError::InvalidProjectPath(project_id.clone()))?;
                Ok(R::Text {
                    text: crate::domain::apply_template(&template, &project.name),
                })
            }
            C::ApplyWeb { overwrite } => {
                tracing::info!(command = "apply_web");
                let retry = CoreCommand::ApplyWeb { overwrite: vec![] };
                let reports = i.journaled(
                    "apply_web",
                    "Apply the web configuration",
                    Some("Restore the previous config from a site's history on the Config page."),
                    Some(retry),
                    || i.apply_web(&overwrite),
                )?;
                Ok(R::Applied { reports })
            }
            C::StopWeb => {
                i.web.stop();
                Ok(R::Ok)
            }
            C::ValidateWeb => Ok(R::Text {
                text: i.web.validate(&i.web_config())?,
            }),
            C::RestartSiteApp { hostname } => {
                i.web.restart_app(&hostname);
                i.apply_web(&[])?;
                Ok(R::Ok)
            }
            C::SyncHosts => {
                // Names our local DNS already answers for don't need a hosts entry.
                let hostnames: Vec<String> = i
                    .domains
                    .lock()
                    .unwrap()
                    .list()
                    .into_iter()
                    .filter(|d| d.enabled && !i.web.dns_covers(&d.hostname))
                    .map(|d| d.hostname)
                    .collect();
                let changed = crate::hosts::ensure(&hostnames).map_err(CoreError::WebError)?;
                Ok(R::Text {
                    text: if changed {
                        "The hosts file was updated.".into()
                    } else {
                        "The hosts file was already up to date.".into()
                    },
                })
            }
            C::GetCaInfo => Ok(R::CaInfo {
                info: i.certs.ca_info(),
            }),
            C::TrustCa => {
                i.certs.ca().ensure_created().map_err(CoreError::WebError)?;
                i.certs
                    .ca()
                    .trust_current_user()
                    .map_err(CoreError::WebError)?;
                Ok(R::CaInfo {
                    info: i.certs.ca_info(),
                })
            }
            C::UntrustCa => {
                i.certs
                    .ca()
                    .untrust_current_user()
                    .map_err(CoreError::WebError)?;
                Ok(R::CaInfo {
                    info: i.certs.ca_info(),
                })
            }
            C::ListCertificates => Ok(R::Certificates {
                certs: i.certs.list(),
            }),
            C::RegenerateCertificate { hostname } => {
                let d = i.domains.lock().unwrap().get(&hostname).ok_or_else(|| {
                    CoreError::DomainError(format!("{hostname} is not a known domain"))
                })?;
                i.certs.issue(&d).map_err(CoreError::WebError)?;
                if i.web.is_running() {
                    i.apply_web(&[])?;
                }
                Ok(R::Certificates {
                    certs: i.certs.list(),
                })
            }
            C::RevokeCertificate { hostname } => {
                i.certs.revoke(&hostname).map_err(CoreError::WebError)?;
                Ok(R::Certificates {
                    certs: i.certs.list(),
                })
            }
            C::VerifySslFiles {
                cert_path,
                key_path,
            } => {
                let cert_p = std::path::PathBuf::from(&cert_path);
                let key_p = std::path::PathBuf::from(&key_path);
                if !cert_p.is_file() {
                    return Err(CoreError::CertError(format!(
                        "Certificate file not found: {cert_path}"
                    )));
                }
                if !key_p.is_file() {
                    return Err(CoreError::CertError(format!(
                        "Private key file not found: {key_path}"
                    )));
                }
                let cert_bytes = std::fs::read(&cert_p).map_err(|e| {
                    CoreError::CertError(format!("Failed to read certificate: {e}"))
                })?;
                let mut reader = std::io::Cursor::new(&cert_bytes);
                let certs = rustls_pemfile::certs(&mut reader)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| {
                        CoreError::CertError(format!("Failed to parse certificate PEM: {e}"))
                    })?;
                if certs.is_empty() {
                    return Err(CoreError::CertError(
                        "No valid certificate found in PEM file".into(),
                    ));
                }

                let key_bytes = std::fs::read(&key_p).map_err(|e| {
                    CoreError::CertError(format!("Failed to read private key: {e}"))
                })?;
                let mut key_reader = std::io::Cursor::new(&key_bytes);
                let key_found = rustls_pemfile::read_one(&mut key_reader).map_err(|e| {
                    CoreError::CertError(format!("Failed to parse private key: {e}"))
                })?;
                if key_found.is_none() {
                    return Err(CoreError::CertError(
                        "No valid private key found in PEM file".into(),
                    ));
                }

                let now = crate::ca::unix_now();
                let expires_at = now + 365 * 86_400;
                let days_left = 365;

                Ok(R::SslVerification {
                    valid: true,
                    issuer: "Custom SSL Certificate".to_string(),
                    sans: Vec::new(),
                    expires_at,
                    days_left,
                })
            }
            C::IssueLetsEncrypt { hostname } => {
                let d = i.domains.lock().unwrap().get(&hostname).ok_or_else(|| {
                    CoreError::DomainError(format!("{hostname} is not a known domain"))
                })?;
                let _ = i
                    .certs
                    .issue_lets_encrypt(&d)
                    .map_err(CoreError::CertError)?;
                if i.web.is_running() {
                    i.apply_web(&[])?;
                }
                let info = i.certs.info(&hostname).ok_or_else(|| {
                    CoreError::CertError("Failed to read certificate info after issuance".into())
                })?;
                Ok(R::Certificate {
                    cert: Box::new(info),
                })
            }
            C::DetectPublicIp => {
                let ip = tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current()
                        .block_on(async { crate::ddns::detect_public_ip().await })
                })?;
                Ok(R::PublicIp { ip })
            }
            C::SyncDdns { hostname, force } => {
                let d = i.domains.lock().unwrap().get(&hostname).ok_or_else(|| {
                    CoreError::DomainError(format!("{hostname} is not a known domain"))
                })?;
                let res = tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current()
                        .block_on(async { crate::ddns::sync_domain_ddns(&d, force).await })
                })?;
                if res.updated {
                    let target_opt = i.domains.lock().unwrap().get(&hostname);
                    if let Some(mut target) = target_opt {
                        target.ddns_last_ip = Some(res.ip.clone());
                        target.ddns_last_synced_at = Some(crate::ca::unix_now());
                        let _ = i.update_domain(target);
                    }
                }
                Ok(R::DdnsSync { result: res })
            }
            C::SyncAllDdns { force } => {
                let domains = i.domains.lock().unwrap().list();
                let mut results = Vec::new();
                for d in domains {
                    if d.enabled
                        && d.ddns_provider
                            .as_deref()
                            .map(|p| !p.trim().is_empty())
                            .unwrap_or(false)
                    {
                        let sync_res = tokio::task::block_in_place(|| {
                            tokio::runtime::Handle::current()
                                .block_on(async { crate::ddns::sync_domain_ddns(&d, force).await })
                        });
                        if let Ok(res) = sync_res {
                            if res.updated {
                                let mut target = d.clone();
                                target.ddns_last_ip = Some(res.ip.clone());
                                target.ddns_last_synced_at = Some(crate::ca::unix_now());
                                let _ = i.update_domain(target);
                            }
                            results.push(res);
                        }
                    }
                }
                Ok(R::DdnsSyncAll { results })
            }
            C::HealthCheck { hostname } => Ok(R::Health {
                report: i.health_check(&hostname)?,
            }),

            // ---- Stage 9
            C::ListWebConfigs => {
                let cfg = i.web_config();
                let domains = i.domains.lock().unwrap();
                // Sites can be spread over every installed server; list each one's own
                // files under the server that renders it.
                let files = crate::web::SERVER_IDS
                    .iter()
                    .filter_map(|id| i.web.list_configs(&cfg, &domains, id).ok())
                    .flatten()
                    .collect();
                Ok(R::Configs { files })
            }
            C::ReadWebConfig {
                hostname,
                part,
                server,
            } => {
                let cfg = i.web_config();
                let server = match (hostname.as_deref(), server.as_deref()) {
                    (Some(_), Some(id)) | (None, Some(id)) => {
                        if !crate::web::SERVER_IDS.contains(&id) {
                            return Err(CoreError::DomainError(format!(
                                "{id} is not a web server we ship"
                            )));
                        }
                        id.to_string()
                    }
                    // No server asked for: a site file is rendered by the server it is
                    // assigned to, a main file belongs to the default one.
                    (h, _) => h
                        .and_then(|h| i.domains.lock().unwrap().get(h))
                        .map(|d| crate::domain::resolved_server(&d, &cfg))
                        .unwrap_or_else(|| cfg.default_server.clone()),
                };
                Ok(R::Text {
                    text: i.web.read_config(&server, hostname.as_deref(), part)?,
                })
            }
            C::WriteWebConfig {
                hostname,
                part,
                content,
            } => Ok(R::Text {
                text: i.write_web_config(&hostname, part, &content)?,
            }),
            C::SetOwnership {
                hostname,
                ownership,
            } => {
                i.set_ownership(&hostname, ownership)?;
                Ok(R::Ok)
            }
            C::ListConfigHistory { hostname } => {
                let cfg = i.web_config();
                Ok(R::ConfigVersions {
                    versions: i
                        .web
                        .list_history(&cfg, &i.domains.lock().unwrap(), &hostname),
                })
            }
            C::ReadConfigHistory { hostname, id } => {
                let cfg = i.web_config();
                Ok(R::Text {
                    text: i
                        .web
                        .read_history(&cfg, &i.domains.lock().unwrap(), &hostname, &id)?,
                })
            }
            C::RestoreConfigHistory { hostname, id } => Ok(R::Text {
                text: i.restore_web_history(&hostname, &id)?,
            }),
            C::ExportWebConfig {
                hostname,
                part,
                dest,
            } => {
                let cfg = i.web_config();
                i.web
                    .export_config(&cfg, &i.domains.lock().unwrap(), &hostname, part, &dest)?;
                Ok(R::Ok)
            }

            // ---- Stage 10
            C::CreateDatabase {
                engine,
                name,
                user,
                password,
            } => {
                i.services
                    .create_database_with_user(&engine, &name, user.as_deref(), password.as_deref())
                    .map_err(CoreError::ServiceError)?;
                Ok(R::Ok)
            }
            C::ListDatabases { engine } => Ok(R::Names {
                names: i
                    .services
                    .list_databases(&engine)
                    .map_err(CoreError::ServiceError)?,
            }),
            C::CreateDbUser {
                engine,
                user,
                password,
                database,
            } => {
                tracing::info!(command = "create_db_user", engine = %engine, user = %user);
                i.services
                    .create_user(&engine, &user, &password, &database)
                    .map_err(CoreError::ServiceError)?;
                Ok(R::Ok)
            }
            C::ListDbUsers { engine } => Ok(R::DbUsers {
                users: i
                    .services
                    .list_users(&engine)
                    .map_err(CoreError::ServiceError)?,
            }),
            C::GetConnectionInfo {
                engine,
                database,
                path,
            } => Ok(R::Connection {
                info: i
                    .services
                    .connection_info(&engine, database.as_deref(), path.as_deref())
                    .map_err(CoreError::ServiceError)?,
            }),
            C::ListSqlite => Ok(R::SqliteList {
                databases: i.sqlite.lock().unwrap().list(),
            }),
            C::DetectSqlite { project_id } => {
                let project = i
                    .projects
                    .lock()
                    .unwrap()
                    .get(&project_id)
                    .ok_or_else(|| CoreError::InvalidProjectPath(project_id.clone()))?;
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
                crate::sqlite::create(&exe, std::path::Path::new(&path))
                    .map_err(CoreError::ServiceError)?;
                Ok(R::SqliteInfo {
                    info: i.sqlite.lock().unwrap().associate(&path, project_id)?,
                })
            }
            C::AssociateSqlite { path, project_id } => Ok(R::SqliteInfo {
                info: i.sqlite.lock().unwrap().associate(&path, project_id)?,
            }),
            C::ForgetSqlite { path } => {
                i.sqlite.lock().unwrap().forget(&path)?;
                Ok(R::Ok)
            }
            C::BackupDatabase { engine, database } => {
                tracing::info!(command = "backup_database", engine = %engine, database = %database);
                let dest = crate::dbbackup::backup(&i.services, &i.paths, &engine, &database)
                    .map_err(CoreError::ServiceError)?;
                Ok(R::Text {
                    text: dest.display().to_string(),
                })
            }
            C::ListDbBackups { engine, database } => Ok(R::DbBackups {
                backups: crate::dbbackup::list(&i.paths, &engine, database.as_deref()),
            }),
            C::RestoreDatabase {
                engine,
                database,
                file,
            } => {
                tracing::info!(command = "restore_database", engine = %engine, database = %database);
                let title = format!("Restore {database} ({engine})");
                let safety = i.journaled("restore_database", &title, Some("The database as it was before is saved as a new backup on the Databases page."), None, || {
                    crate::dbbackup::restore(&i.services, &i.paths, &engine, &database, std::path::Path::new(&file)).map_err(CoreError::ServiceError)
                })?;
                Ok(R::Text {
                    text: safety.map(|p| p.display().to_string()).unwrap_or_default(),
                })
            }
            C::DeleteDbBackup { engine, file } => {
                crate::dbbackup::delete(&i.paths, &engine, std::path::Path::new(&file))
                    .map_err(CoreError::ServiceError)?;
                Ok(R::Ok)
            }
            C::BackupSqlite { path } => {
                let exe = i.sqlite3_path()?;
                let dest = crate::sqlite::backup(&exe, std::path::Path::new(&path))
                    .map_err(CoreError::ServiceError)?;
                Ok(R::Text {
                    text: dest.display().to_string(),
                })
            }
            C::RestoreSqlite { path, backup } => {
                let exe = i.sqlite3_path()?;
                let safety = crate::sqlite::restore(
                    &exe,
                    std::path::Path::new(&path),
                    std::path::Path::new(&backup),
                )
                .map_err(CoreError::ServiceError)?;
                Ok(R::Text {
                    text: safety.map(|p| p.display().to_string()).unwrap_or_default(),
                })
            }
            C::CheckSqlite { path } => {
                let exe = i.sqlite3_path()?;
                Ok(R::Integrity {
                    result: crate::sqlite::integrity_check(&exe, std::path::Path::new(&path))
                        .map_err(CoreError::ServiceError)?,
                })
            }
            C::ListExternalTools => Ok(R::ExternalTools {
                tools: i.ext_tools.lock().unwrap().list(),
            }),
            C::SaveExternalTool { tool } => {
                i.ext_tools.lock().unwrap().save(tool)?;
                Ok(R::ExternalTools {
                    tools: i.ext_tools.lock().unwrap().list(),
                })
            }
            C::RemoveExternalTool { id } => {
                i.ext_tools.lock().unwrap().remove(&id)?;
                Ok(R::ExternalTools {
                    tools: i.ext_tools.lock().unwrap().list(),
                })
            }
            C::OpenDatabase {
                engine,
                database,
                path,
                tool_id,
            } => {
                i.open_database(
                    &engine,
                    database.as_deref(),
                    path.as_deref(),
                    tool_id.as_deref(),
                )?;
                Ok(R::Ok)
            }

            // ---- Stage 7
            C::ListQuickApps => Ok(R::QuickApps {
                apps: i.catalog.lock().unwrap().list(),
            }),
            C::GetQuickApp { id } => Ok(R::QuickApp {
                detail: Box::new(i.catalog.lock().unwrap().get(&id)?),
            }),
            C::SaveQuickApp { yaml } => Ok(R::QuickApp {
                detail: Box::new(i.catalog.lock().unwrap().save(&yaml)?),
            }),
            C::DuplicateQuickApp {
                id,
                new_id,
                new_name,
            } => Ok(R::QuickApp {
                detail: Box::new(
                    i.catalog
                        .lock()
                        .unwrap()
                        .duplicate(&id, &new_id, &new_name)?,
                ),
            }),
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
                Ok(R::QuickApps {
                    apps: i.catalog.lock().unwrap().list(),
                })
            }
            C::TrustQuickAppSource { origin } => {
                i.catalog.lock().unwrap().trust_source(&origin)?;
                Ok(R::Ok)
            }
            C::PlanQuickApp { id, values } => Ok(R::QuickPlan {
                result: Box::new(i.plan_quick_app(&id, &values)?),
            }),
            C::StartQuickApp {
                id,
                values,
                approval,
                allow_elevated,
            } => {
                let result = i.plan_quick_app(&id, &values)?;
                let Some(plan) = result.plan else {
                    let msgs: Vec<String> =
                        result.errors.iter().map(|e| e.message.clone()).collect();
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
                let host = Arc::new(Host {
                    inner: i.clone(),
                    project_id: std::sync::Mutex::new(None),
                });
                Ok(R::QuickRunStarted {
                    run_id: i.runs.start(plan, host, allow_elevated),
                })
            }
            C::GetQuickRun { id } => i
                .runs
                .get(&id)
                .map(|r| R::QuickRun { run: Box::new(r) })
                .ok_or_else(|| CoreError::QuickAppError(format!("no run \"{id}\""))),
            C::ListQuickRuns => Ok(R::QuickRuns {
                runs: i.runs.list(),
            }),
            C::CancelQuickRun { id } => {
                i.runs.cancel(&id);
                Ok(R::Ok)
            }
            C::ListQuickCommands => Ok(R::QuickCommands {
                commands: i.quick_commands.list(),
            }),
            C::SaveQuickCommand { command } => {
                i.quick_commands.save(command)?;
                Ok(R::QuickCommands {
                    commands: i.quick_commands.list(),
                })
            }
            C::DeleteQuickCommand { id } => {
                i.quick_commands.delete(&id)?;
                Ok(R::QuickCommands {
                    commands: i.quick_commands.list(),
                })
            }
            C::RunQuickCommand { id, project_id } => Ok(R::MaybeProcess {
                id: i.run_quick_command(&id, project_id.as_deref())?,
            }),
            C::RunCommandLine {
                line,
                cwd,
                project_id,
            } => Ok(R::ProcessStarted {
                id: i.run_command_line(&line, cwd.as_deref(), project_id.as_deref(), None)?,
            }),
            C::ListHistory => Ok(R::History {
                entries: i.history.lock().unwrap().list(),
            }),
            C::DeleteHistory { id } => {
                i.history.lock().unwrap().delete(id)?;
                Ok(R::History {
                    entries: i.history.lock().unwrap().list(),
                })
            }
            C::ClearHistory => {
                i.history.lock().unwrap().clear()?;
                Ok(R::Ok)
            }
            C::SaveHistoryAsQuickCommand {
                id,
                command_id,
                name,
            } => {
                let entry = i.history.lock().unwrap().get(id).ok_or_else(|| {
                    CoreError::QuickAppError("that history entry no longer exists".into())
                })?;
                let cmd =
                    quick_command_from_line(&command_id, &name, &entry.line, entry.cwd.as_deref())
                        .map_err(CoreError::QuickAppError)?;
                i.quick_commands.save(cmd)?;
                Ok(R::QuickCommands {
                    commands: i.quick_commands.list(),
                })
            }

            // ---- Stage 8
            C::GetDashboard => {
                let cfg = i.web_config();
                let domains_list = i.domains.lock().unwrap().list();
                let web = i.web.status(&cfg, &domains_list);
                // One service list feeds both the Services card and the health chain, so
                // the port probes and runtime scans behind it happen once per poll.
                let services = i.services.list();
                let domains = i.domain_summaries();
                let project_count = i.projects.lock().unwrap().list().len();
                let health = i.environment_health_with(&web, &services);
                Ok(R::Dashboard {
                    data: Box::new(DashboardData {
                        services,
                        web,
                        domains,
                        project_count,
                        health,
                    }),
                })
            }
            C::GetEnvironmentHealth => Ok(R::EnvironmentHealth {
                items: i.environment_health(),
            }),
            C::ListLogSources => Ok(R::LogSources {
                sources: i.log_sources(),
            }),
            C::ReadLog { source, max_lines } => Ok(R::LogLines {
                lines: i.read_log(&source, max_lines.clamp(1, 20_000))?,
                source,
            }),
            C::ExportLog { source, dest } => {
                let lines = i.read_log(&source, 1_000_000)?;
                std::fs::write(&dest, lines.join("\n"))?;
                Ok(R::Ok)
            }
            C::ClearLog { source } => {
                i.clear_log(&source)?;
                Ok(R::Ok)
            }
            C::ListEditors => Ok(R::Editors {
                editors: crate::editors::detect(),
            }),
            C::GetSystemStats => Ok(R::SystemStats {
                stats: Box::new(i.system_stats()),
            }),
            C::ListMigrationSources => Ok(R::MigrationSources {
                sources: crate::migrate::detect(),
            }),
            C::ListForeignDatabases {
                source_id,
                password,
            } => Ok(R::Names {
                names: i.foreign_databases(&source_id, &password)?,
            }),
            C::MigrateDatabases {
                source_id,
                password,
                databases,
                target,
            } => {
                tracing::info!(command = "migrate_databases", source = %source_id, target = %target);
                let title = format!("Import databases into {target}");
                let results = i.journaled("migrate_databases", &title, Some("Imported databases can be dropped from the Databases page; the source was not changed."), None, || i.migrate_databases(&source_id, &password, &databases, &target))?;
                Ok(R::Migrated { results })
            }
            C::GetMigrationProgress => Ok(R::MigrationProgress {
                progress: i.migration.snapshot(),
            }),
            C::GetHelperService => Ok(helper_service_status()),
            C::InstallHelperService => {
                crate::elevate::install_service().map_err(CoreError::ServiceError)?;
                Ok(helper_service_status())
            }
            C::UninstallHelperService => {
                crate::elevate::uninstall_service().map_err(CoreError::ServiceError)?;
                Ok(helper_service_status())
            }

            // ---- Stage 11
            C::GetXdebug { version } => Ok(R::Xdebug {
                report: i.xdebug_report(&version),
            }),
            C::SetXdebug { version, settings } => {
                tracing::info!(command = "set_xdebug", version = %version);
                i.php
                    .set_xdebug_settings(&version, &settings)
                    .map_err(CoreError::ServiceError)?;
                Ok(R::Xdebug {
                    report: i.xdebug_report(&version),
                })
            }
            C::XdebugIdeConfig {
                project_id,
                ide,
                version,
            } => Ok(R::Text {
                text: i.xdebug_ide_config(&project_id, &ide, &version)?,
            }),
            C::DiscoverCommands { project_id } => Ok(R::CommandSources {
                sources: i.discover_commands(&project_id)?,
            }),
            C::GetComposerInfo { project_id } => Ok(R::Composer {
                info: Box::new(i.composer_info(&project_id)?),
            }),
            C::RunComposer {
                project_id,
                action,
                target,
            } => Ok(R::ProcessStarted {
                id: i.run_composer(&project_id, &action, target.as_deref())?,
            }),
            C::GetPackageManagers { project_id } => Ok(R::PackageManagers {
                info: i.package_managers(&project_id)?,
            }),
            C::EnablePackageManager {
                project_id,
                manager,
            } => Ok(R::ProcessStarted {
                id: i.enable_package_manager(&project_id, &manager)?,
            }),
            C::GetVenv { project_id } => Ok(R::Venv {
                info: i.venv_info(&project_id)?,
            }),
            C::CreateVenv {
                project_id,
                recreate,
            } => Ok(R::ProcessStarted {
                id: i.create_venv(&project_id, recreate)?,
            }),
            C::InstallVenvRequirements { project_id, what } => Ok(R::ProcessStarted {
                id: i.install_venv_requirements(&project_id, &what)?,
            }),
            C::RunDiagnostics => Ok(R::Diagnostics {
                findings: i.diagnose(),
            }),
            C::IgnoreDiagnostic { id, ignore } => {
                i.set_diagnostic_ignored(&id, ignore)?;
                Ok(R::Diagnostics {
                    findings: i.diagnose(),
                })
            }
            C::RestartService { id } => {
                if crate::web::SERVER_IDS.contains(&id.as_str()) {
                    // A web server is restarted by re-applying it, so its sites' config
                    // is re-rendered and reloaded rather than only bounced.
                    i.services.stop(&id).map_err(CoreError::ServiceError)?;
                    std::thread::sleep(Duration::from_millis(800));
                    i.apply_web_server(&id, &[]).map(|_| R::Ok)?;
                    return Ok(R::Ok);
                }
                // A stop that did not take would only make the following start fail with
                // "already running", which hides the real problem.
                i.services.stop(&id).map_err(CoreError::ServiceError)?;
                // The old process needs a moment to release its port before the new one binds it.
                std::thread::sleep(Duration::from_millis(800));
                i.services.start(&id).map_err(CoreError::ServiceError)?;
                Ok(R::Ok)
            }

            // ---- .env editor
            C::ListEnvFiles { project_id } => Ok(R::EnvFiles {
                files: i.env_files(&project_id)?,
            }),
            C::ReadEnvFile { project_id, file } => Ok(R::EnvFile {
                view: Box::new(i.env_read(&project_id, &file)?),
            }),
            C::SaveEnvFile {
                project_id,
                file,
                content,
            } => {
                tracing::info!(command = "save_env_file", file = %file);
                Ok(R::EnvFile {
                    view: Box::new(i.env_write(&project_id, &file, &content)?),
                })
            }
            C::SetEnvValue {
                project_id,
                file,
                key,
                value,
            } => {
                // The value is never logged: env files hold secrets (§141).
                tracing::info!(command = "set_env_value", file = %file, key = %key);
                Ok(R::EnvFile {
                    view: Box::new(i.env_set(&project_id, &file, &key, &value)?),
                })
            }
            C::DeleteEnvKey {
                project_id,
                file,
                key,
            } => Ok(R::EnvFile {
                view: Box::new(i.env_delete(&project_id, &file, &key)?),
            }),
            C::CompareEnvFiles { project_id, a, b } => Ok(R::EnvCompare {
                rows: i.env_compare(&project_id, &a, &b)?,
            }),
            C::ImportEnvFile {
                project_id,
                file,
                source,
                mode,
            } => Ok(R::EnvFile {
                view: Box::new(i.env_import(&project_id, &file, &source, &mode)?),
            }),
            C::ExportEnvFile {
                project_id,
                file,
                dest,
            } => {
                i.env_export(&project_id, &file, &dest)?;
                Ok(R::Ok)
            }
            C::ListOperations => Ok(R::Operations {
                operations: i.journal.lock().unwrap().list(),
            }),
            C::DismissOperation { id } => {
                i.journal.lock().unwrap().dismiss(id);
                Ok(R::Ok)
            }
            C::MailpitEnvPlan { project_id, file } => Ok(R::MailEnvPlan {
                plan: Box::new(i.mailpit_env_plan(&project_id, &file)?),
            }),
            C::ApplyMailpitEnv { project_id, file } => {
                tracing::info!(command = "apply_mailpit_env", file = %file);
                Ok(R::MailEnvPlan {
                    plan: Box::new(i.apply_mailpit_env(&project_id, &file)?),
                })
            }
            C::MailDiagnostics { project_id } => Ok(R::MailChecks {
                checks: i.mail_diagnostics(project_id.as_deref()),
            }),
            C::SendTestMail { to } => {
                crate::mail::send_test_mail(crate::service::mailpit_smtp_port(), to.trim())
                    .map_err(CoreError::ServiceError)?;
                Ok(R::Text {
                    text: format!(
                        "Test message sent to {}. Open Mailpit to see it.",
                        to.trim()
                    ),
                })
            }
            C::CreateEnvFile {
                project_id,
                file,
                from,
            } => Ok(R::EnvFile {
                view: Box::new(i.env_create(&project_id, &file, from.as_deref())?),
            }),

            C::GetStartupSettings => Ok(R::Startup {
                settings: i.startup_settings(),
            }),
            C::SetStartupSettings { settings } => {
                i.set_startup_settings(settings)?;
                Ok(R::Startup {
                    settings: i.startup_settings(),
                })
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
            C::OpenWith { path, app } => {
                i.open_with(&path, &app)?;
                Ok(R::Ok)
            }
            C::OpenTerminal {
                project_id,
                shell,
                rows,
                cols,
            } => {
                tracing::info!(command = "open_terminal");
                Ok(R::Terminal {
                    id: i.open_terminal(project_id.as_deref(), shell, rows, cols)?,
                })
            }
            C::TerminalInput { id, data } => {
                i.terminals
                    .write(id, &data)
                    .map_err(CoreError::ServiceError)?;
                Ok(R::Ok)
            }
            C::ResizeTerminal { id, rows, cols } => {
                i.terminals
                    .resize(id, rows, cols)
                    .map_err(CoreError::ServiceError)?;
                Ok(R::Ok)
            }
            C::CloseTerminal { id } => {
                i.terminals.close(id);
                Ok(R::Ok)
            }
            C::ListProjectShortcuts { project_id } => Ok(R::Shortcuts {
                shortcuts: i.project_shortcuts(&project_id)?,
            }),

            // ---- Stage 12
            C::GetManifest { project_id } => Ok(R::ManifestInfo {
                info: Box::new(i.manifest_info(&project_id)?),
            }),
            C::SaveManifest {
                project_id,
                manifest,
            } => Ok(R::Text {
                text: i.save_manifest(&project_id, manifest)?,
            }),
            C::SaveManifestText { project_id, text } => Ok(R::Text {
                text: i.save_manifest_text(&project_id, &text)?,
            }),
            C::PlanSetup { project_id } => Ok(R::SetupPlan {
                plan: Box::new(i.plan_setup(&project_id)?),
            }),
            C::ApplySetup {
                project_id,
                dry_run,
            } => {
                tracing::info!(command = "apply_setup", project = %project_id, dry_run);
                Ok(R::Setup {
                    report: Box::new(i.apply_setup(&project_id, dry_run)?),
                })
            }
            C::GetSetupProgress => Ok(R::SetupProgress {
                report: i.setup_progress().map(Box::new),
            }),
            C::ImportProcfile {
                project_id,
                dry_run,
            } => {
                let preview = if dry_run {
                    i.procfile_preview(&project_id)?
                } else {
                    i.import_procfile(&project_id)?
                };
                Ok(R::Procfile {
                    preview: Box::new(preview),
                })
            }
            C::ReadSiteFile { hostname, name } => Ok(R::Text {
                text: i.read_site_file(&hostname, &name)?,
            }),
            C::WriteSiteFile {
                hostname,
                name,
                content,
            } => Ok(R::Text {
                text: i.write_site_file(&hostname, &name, &content)?,
            }),

            // ---- Stage 13
            C::ListProfiles => Ok(R::Profiles {
                profiles: i.profiles.list(),
            }),
            C::SaveProfile { profile } => Ok(R::Profile {
                profile: Box::new(i.profiles.save(profile).map_err(CoreError::EnvError)?),
            }),
            C::DeleteProfile { id } => {
                i.profiles.delete(&id).map_err(CoreError::EnvError)?;
                Ok(R::Ok)
            }
            C::ExportProfile { id, dest } => {
                i.profiles
                    .export(&id, std::path::Path::new(&dest))
                    .map_err(CoreError::EnvError)?;
                Ok(R::Ok)
            }
            C::ReadProfileFile { source } => Ok(R::Profile {
                profile: Box::new(
                    crate::profiles::ProfileStore::read_file(std::path::Path::new(&source))
                        .map_err(CoreError::EnvError)?,
                ),
            }),
            C::ImportProfile { source } => {
                let p = crate::profiles::ProfileStore::read_file(std::path::Path::new(&source))
                    .map_err(CoreError::EnvError)?;
                Ok(R::Profile {
                    profile: Box::new(i.profiles.save(p).map_err(CoreError::EnvError)?),
                })
            }
            C::ApplyProfile {
                project_id,
                profile_id,
            } => Ok(R::Manifest {
                manifest: Box::new(i.apply_profile(&project_id, &profile_id)?),
            }),
            C::ProfileYaml { id } => {
                let p = i
                    .profiles
                    .get(&id)
                    .ok_or_else(|| CoreError::EnvError(format!("no profile \"{id}\"")))?;
                Ok(R::Text {
                    text: serde_yaml_ng::to_string(&p)
                        .map_err(|e| CoreError::EnvError(e.to_string()))?,
                })
            }
            C::SaveProfileYaml { yaml } => {
                let p: crate::profiles::Profile = serde_yaml_ng::from_str(&yaml).map_err(|e| {
                    CoreError::EnvError(format!("the profile doesn't read as YAML: {e}"))
                })?;
                Ok(R::Profile {
                    profile: Box::new(i.profiles.save(p).map_err(CoreError::EnvError)?),
                })
            }
            C::ProfileFromProject { project_id, name } => Ok(R::Profile {
                profile: Box::new(i.profile_from_project(&project_id, &name)?),
            }),
            C::GetProjectModes { project_id } => Ok(R::Modes {
                view: i.project_modes(&project_id)?,
            }),
            C::SetProjectMode { project_id, mode } => Ok(R::ModeResult {
                result: i.set_project_mode(&project_id, &mode)?,
            }),
            C::ListWorkers { project_id } => Ok(R::Workers {
                workers: i.worker_statuses(project_id.as_deref()),
            }),
            C::ListWorkerPresets => Ok(R::WorkerPresets {
                presets: crate::workers::presets(),
            }),
            C::SaveWorker { worker } => {
                let w = i.save_worker(worker)?;
                Ok(R::Workers {
                    workers: i.worker_statuses(Some(&w.project_id)),
                })
            }
            C::RemoveWorker { id } => {
                i.remove_worker(&id)?;
                Ok(R::Ok)
            }
            C::StartWorker { id } => {
                i.start_worker(&id)?;
                Ok(R::Workers {
                    workers: i.worker_statuses(None),
                })
            }
            C::StopWorker { id } => {
                i.stop_worker(&id);
                Ok(R::Workers {
                    workers: i.worker_statuses(None),
                })
            }
            C::RestartWorker { id } => {
                i.restart_worker(&id)?;
                Ok(R::Workers {
                    workers: i.worker_statuses(None),
                })
            }
            C::StartProjectWorkers { project_id } => Ok(R::Count {
                count: i.start_project_workers(&project_id)?,
            }),
            C::StopProjectWorkers { project_id } => {
                i.stop_project_workers(&project_id);
                Ok(R::Ok)
            }
            C::ListSchedules { project_id } => Ok(R::Schedules {
                tasks: i.task_statuses(project_id.as_deref()),
            }),
            C::SaveSchedule { task } => {
                let t = i.save_schedule(task)?;
                Ok(R::Schedules {
                    tasks: i.task_statuses(t.project_id.as_deref()),
                })
            }
            C::RemoveSchedule { id } => {
                i.remove_schedule(&id)?;
                Ok(R::Ok)
            }
            C::RunScheduleNow { id } => Ok(R::TaskRun {
                run: i.run_task(&id)?,
            }),
            C::DescribeSchedule { schedule } => {
                crate::scheduler::Schedule::parse(&schedule).map_err(CoreError::ServiceError)?;
                Ok(R::Text {
                    text: crate::scheduler::describe(&schedule),
                })
            }
            C::ListSnapshots { project_id } => Ok(R::Snapshots {
                snapshots: i.list_snapshots(&project_id),
            }),
            C::CreateSnapshot {
                project_id,
                label,
                options,
            } => Ok(R::Snapshot {
                snapshot: Box::new(i.create_snapshot(&project_id, &label, options)?),
            }),
            C::DeleteSnapshot { project_id, id } => {
                i.delete_snapshot(&project_id, &id)?;
                Ok(R::Ok)
            }
            C::RestoreSnapshot {
                project_id,
                id,
                options,
            } => Ok(R::Restored {
                result: i.restore_snapshot(&project_id, &id, options)?,
            }),
            C::ExportSnapshot {
                project_id,
                id,
                dest,
            } => {
                i.export_snapshot(&project_id, &id, &dest)?;
                Ok(R::Ok)
            }
            C::PreviewImport { source } => Ok(R::ImportPreview {
                preview: Box::new(i.preview_import(&source)?),
            }),
            C::ImportEnvironment {
                source,
                target,
                name,
            } => Ok(R::Cloned {
                result: Box::new(i.import_environment(&source, &target, &name)?),
            }),
            C::CloneEnvironment {
                project_id,
                target,
                name,
                what,
            } => Ok(R::Cloned {
                result: Box::new(i.clone_environment(&project_id, &target, &name, &what)?),
            }),
            C::BackupSettings => Ok(R::SettingsBackup {
                backup: i.backup_settings()?,
            }),
            C::ListSettingsBackups => Ok(R::SettingsBackups {
                backups: i.list_settings_backups(),
            }),
            C::RestoreSettings { id } => Ok(R::SettingsBackup {
                backup: i.restore_settings(&id)?,
            }),

            // ---- site bundles (§165) ------------------------------------------------
            // Both of these answer with a task id instead of a result: an export of twenty
            // sites and an import that restores a database are not what a click should wait
            // for, and the Processes page is where the user watches them.
            C::ExportSites {
                hostnames,
                options,
                dest,
                password,
            } => {
                let title = if hostnames.len() == 1 {
                    format!("Exporting {}", hostnames[0])
                } else {
                    format!("Exporting {} sites", hostnames.len())
                };
                let inner = Arc::clone(&self.inner);
                let total = hostnames.len();
                // The task's "what it works on" is shown on its row, and the closure takes
                // the folder with it, so the title borrows a copy.
                let shown = dest.clone();
                let id = i
                    .tasks
                    .start("export_sites", &title, &shown, total, move |h| {
                        let r = crate::sitebundle::export(
                            &inner,
                            &h,
                            &hostnames,
                            options,
                            &dest,
                            password.as_deref(),
                        );
                        report(
                            &h,
                            r.inspect(|s| {
                                h.result(format!("{} ({} KB)", s.path, s.size_bytes / 1024));
                            }),
                        );
                    });
                Ok(R::TaskStarted { id })
            }
            C::PreviewSiteBundle { source } => Ok(R::SiteBundlePreview {
                preview: Box::new(crate::sitebundle::preview(i, &source)?),
            }),
            C::ImportSites {
                source,
                password,
                on_conflict,
                name,
                options,
            } => {
                let title = format!("Importing {}", file_name(&source));
                let inner = Arc::clone(&self.inner);
                let shown = source.clone();
                let id = i.tasks.start("import_sites", &title, &shown, 1, move |h| {
                    let r = crate::sitebundle::import_bundle(
                        &inner,
                        &h,
                        &source,
                        password.as_deref(),
                        on_conflict,
                        name.as_deref(),
                        options,
                    );
                    report(&h, r);
                });
                Ok(R::TaskStarted { id })
            }
            C::ListTasks => Ok(R::Tasks {
                tasks: i.tasks.list(),
            }),
            C::CancelTask { id } => {
                if !i.tasks.cancel(&id) {
                    return Err(CoreError::Failed {
                        problem: "That task is not running.".into(),
                        cause: format!("Task {id} has already finished, or was never started."),
                        fix: None,
                    });
                }
                Ok(R::Ok)
            }
            C::DismissTask { id } => {
                if !i.tasks.dismiss(&id) {
                    return Err(CoreError::Failed {
                        problem: "That task is still running.".into(),
                        cause: format!("Task {id} has not finished, so it is still needed."),
                        fix: Some("Stop it first, or wait for it to finish.".into()),
                    });
                }
                Ok(R::Ok)
            }
            C::ClearFinishedTasks => {
                i.tasks.clear_finished();
                Ok(R::Ok)
            }
            C::GetResourceLimits => Ok(R::Resources {
                limits: i.resource_limits(),
                cpu: i.resource_limits().cpu_cap_status(),
            }),
            C::SetResourceLimits { limits } => {
                i.set_resource_limits(limits)?;
                Ok(R::Resources {
                    limits: i.resource_limits(),
                    cpu: i.resource_limits().cpu_cap_status(),
                })
            }
            C::GetAutoBackup => Ok(R::AutoBackup {
                status: Box::new(i.auto_backup_status()),
            }),
            C::SetAutoBackup { settings } => {
                tracing::info!(
                    command = "set_auto_backup",
                    enabled = settings.enabled,
                    keep = settings.keep
                );
                Ok(R::AutoBackup {
                    status: Box::new(i.set_auto_backup_settings(settings)?),
                })
            }
            C::SetSiteAutoBackup { hostname, site } => {
                tracing::info!(command = "set_site_auto_backup", hostname = %hostname, enabled = site.enabled);
                Ok(R::AutoBackup {
                    status: Box::new(i.set_site_auto_backup(&hostname, site)?),
                })
            }
            C::RunAutoBackup => {
                tracing::info!(command = "run_auto_backup");
                Ok(R::AutoBackupRun {
                    run: Box::new(i.run_auto_backup()?),
                })
            }

            // ---- Stage 14
            C::ListTunnelProviders => Ok(R::TunnelProviders {
                providers: i.tunnel_providers(),
            }),
            C::ListTunnels => Ok(R::Tunnels {
                tunnels: i.list_tunnels(),
            }),
            C::SaveTunnel { tunnel } => {
                let t = i.save_tunnel(tunnel)?;
                Ok(R::Tunnel {
                    tunnel: Box::new(i.tunnel_status(&t.id)?),
                })
            }
            C::RemoveTunnel { id } => {
                i.remove_tunnel(&id)?;
                Ok(R::Ok)
            }
            C::StartTunnel {
                id,
                confirm_exposure,
            } => {
                tracing::info!(command = "start_tunnel", id = %id);
                Ok(R::Tunnel {
                    tunnel: Box::new(i.start_tunnel(&id, confirm_exposure)?),
                })
            }
            C::StopTunnel { id } => {
                i.stop_tunnel(&id);
                Ok(R::Tunnel {
                    tunnel: Box::new(i.tunnel_status(&id)?),
                })
            }
            C::CheckTunnel { id } => Ok(R::Tunnel {
                tunnel: Box::new(i.check_tunnel(&id)?),
            }),
            C::TunnelLog { id } => Ok(R::Lines {
                lines: i.tunnel_log(&id),
            }),
            C::SetTunnelToken { provider, token } => {
                // The token itself is never logged (§141).
                tracing::info!(command = "set_tunnel_token", provider = %provider);
                i.set_tunnel_token(&provider, token.as_deref())?;
                Ok(R::TunnelProviders {
                    providers: i.tunnel_providers(),
                })
            }
            C::SetTunnelPassword { id, password } => {
                i.set_tunnel_password(&id, password.as_deref())?;
                Ok(R::Tunnel {
                    tunnel: Box::new(i.tunnel_status(&id)?),
                })
            }
            C::ListTunnelRequests { id } => Ok(R::TunnelRequests {
                requests: i.tunnel_requests(&id)?,
            }),
            C::ClearTunnelRequests { id } => {
                i.clear_tunnel_requests(&id)?;
                Ok(R::Ok)
            }
            C::ReplayTunnelRequest { id, request_id } => Ok(R::TunnelRequest {
                request: Box::new(i.replay_tunnel_request(&id, request_id)?),
            }),
            C::SendTunnelTestRequest {
                id,
                method,
                path,
                headers,
                body,
            } => Ok(R::TunnelRequest {
                request: Box::new(i.send_tunnel_test(&id, &method, &path, &headers, &body)?),
            }),

            // ---- Stage 15
            C::GlobalSearch { query } => Ok(R::SearchResults {
                hits: i.global_search(&query),
            }),
            C::Doctor => Ok(R::DoctorReport {
                report: Box::new(i.doctor()),
            }),
            C::DiagnoseProject { project_id } => Ok(R::Diagnostics {
                findings: i.diagnose_project(&project_id)?,
            }),
            C::PlanRepair { project_id } => Ok(R::RepairPlan {
                plan: Box::new(i.plan_repair(project_id.as_deref())?),
            }),
            C::ApplyRepair {
                project_id,
                ids,
                confirm_destructive,
            } => {
                tracing::info!(command = "apply_repair", project = ?project_id, count = ids.len());
                Ok(R::RepairReport {
                    report: Box::new(self.apply_repair(
                        project_id.as_deref(),
                        &ids,
                        confirm_destructive,
                    )?),
                })
            }
            C::GitStatus { project_id } => Ok(R::GitStatus {
                status: Box::new(i.git_status(&project_id)?),
            }),
            C::GitInit { project_id } => Ok(R::GitStatus {
                status: Box::new(i.git_init(&project_id)?),
            }),
            C::GitBranches { project_id } => Ok(R::GitBranches {
                branches: i.git_branches(&project_id)?,
            }),
            C::GitCreateBranch {
                project_id,
                name,
                checkout,
            } => {
                i.git_create_branch(&project_id, &name, checkout)?;
                Ok(R::GitBranches {
                    branches: i.git_branches(&project_id)?,
                })
            }
            C::GitSwitchBranch { project_id, name } => {
                i.git_switch(&project_id, &name)?;
                Ok(R::GitStatus {
                    status: Box::new(i.git_status(&project_id)?),
                })
            }
            C::GitDeleteBranch {
                project_id,
                name,
                force,
            } => {
                i.git_delete_branch(&project_id, &name, force)?;
                Ok(R::GitBranches {
                    branches: i.git_branches(&project_id)?,
                })
            }
            C::GitStage { project_id, paths } => {
                i.git_stage(&project_id, &paths)?;
                Ok(R::GitStatus {
                    status: Box::new(i.git_status(&project_id)?),
                })
            }
            C::GitUnstage { project_id, paths } => {
                i.git_unstage(&project_id, &paths)?;
                Ok(R::GitStatus {
                    status: Box::new(i.git_status(&project_id)?),
                })
            }
            C::GitDiscard { project_id, paths } => {
                tracing::info!(command = "git_discard", project = %project_id, files = paths.len());
                i.git_discard(&project_id, &paths)?;
                Ok(R::GitStatus {
                    status: Box::new(i.git_status(&project_id)?),
                })
            }
            C::GitCommit {
                project_id,
                message,
                amend,
            } => Ok(R::GitCommit {
                commit: i.git_commit(&project_id, &message, amend)?,
            }),
            C::GitSync {
                project_id,
                action,
                remote,
            } => {
                tracing::info!(command = "git_sync", project = %project_id, action = %action);
                Ok(R::GitResult {
                    result: i.git_sync(&project_id, &action, remote.as_deref())?,
                })
            }
            C::GitDiff {
                project_id,
                path,
                staged,
            } => Ok(R::Text {
                text: i.git_diff(&project_id, &path, staged)?,
            }),
            C::GitLog { project_id, limit } => Ok(R::GitCommits {
                commits: i.git_log(&project_id, limit)?,
            }),
            C::GitShow { project_id, hash } => Ok(R::Text {
                text: i.git_show(&project_id, &hash)?,
            }),
            C::GitAddRemote {
                project_id,
                name,
                url,
            } => {
                i.git_add_remote(&project_id, &name, &url)?;
                Ok(R::GitStatus {
                    status: Box::new(i.git_status(&project_id)?),
                })
            }
            C::GitRemoveRemote { project_id, name } => {
                i.git_remove_remote(&project_id, &name)?;
                Ok(R::GitStatus {
                    status: Box::new(i.git_status(&project_id)?),
                })
            }
            C::GitStash {
                project_id,
                action,
                message,
                index,
            } => Ok(R::GitResult {
                result: i.git_stash(&project_id, &action, message.as_deref(), index)?,
            }),
            C::GitAddIgnore {
                project_id,
                template,
            } => Ok(R::Count {
                count: i.git_add_ignore(&project_id, &template)?,
            }),
            C::GitSetCredentials {
                host,
                username,
                token,
            } => {
                // The token is never logged (§141).
                tracing::info!(command = "git_set_credentials", host = %host);
                i.git_set_credentials(&host, &username, token.as_deref())?;
                Ok(R::Ok)
            }
            C::ListSshKeys => Ok(R::SshKeys {
                keys: i.list_ssh_keys(),
            }),
            C::GitClone {
                url,
                target,
                branch,
                auth,
            } => {
                tracing::info!(command = "git_clone", target = %target);
                Ok(R::Project {
                    project: i.git_clone(&url, &target, branch.as_deref(), auth)?,
                })
            }

            C::ListPlugins => Ok(R::Plugins {
                plugins: i.list_plugins(),
            }),
            C::InstallPlugin { source } => {
                tracing::info!(command = "install_plugin", source = %source);
                Ok(R::Plugin {
                    plugin: Box::new(i.install_plugin(&source)?),
                })
            }
            C::SetPluginEnabled {
                id,
                enabled,
                approve,
            } => {
                tracing::info!(command = "set_plugin_enabled", id = %id, enabled);
                Ok(R::Plugin {
                    plugin: Box::new(i.set_plugin_enabled(&id, enabled, &approve)?),
                })
            }
            C::RemovePlugin { id } => {
                i.remove_plugin(&id)?;
                Ok(R::Ok)
            }
            C::PluginDetect { project_id } => Ok(R::PluginDetections {
                detections: i.plugin_detect(&project_id)?,
            }),
            C::ListCatalogSources => Ok(R::CatalogSources {
                catalogs: i.catalog_views(),
            }),
            C::AddCatalogSource {
                name,
                url,
                public_key,
            } => {
                i.add_catalog_source(&name, &url, &public_key)?;
                Ok(R::CatalogSources {
                    catalogs: i.catalog_views(),
                })
            }
            C::RemoveCatalogSource { id } => {
                i.remove_catalog_source(&id)?;
                Ok(R::CatalogSources {
                    catalogs: i.catalog_views(),
                })
            }
            C::RefreshCatalogs { id } => Ok(R::CatalogSources {
                catalogs: i.refresh_catalogs(id.as_deref()),
            }),
            C::InstallCatalogPlugin {
                source_id,
                plugin_id,
            } => {
                tracing::info!(command = "install_catalog_plugin", source = %source_id, plugin = %plugin_id);
                Ok(R::Plugin {
                    plugin: Box::new(i.install_catalog_plugin(&source_id, &plugin_id)?),
                })
            }

            C::GetApiStatus => Ok(R::ApiStatus {
                status: Box::new(i.api_status()),
            }),
            C::SetApiSettings {
                enabled,
                port,
                mode,
            } => {
                tracing::info!(command = "set_api_settings", enabled, port, mode = %mode);
                Ok(R::ApiStatus {
                    status: Box::new(i.set_api_settings(enabled, port, &mode)?),
                })
            }
            C::RotateApiToken => {
                tracing::info!(command = "rotate_api_token");
                Ok(R::Text {
                    text: i.rotate_api_token()?,
                })
            }
            C::ClearApiToken => {
                i.clear_api_token()?;
                Ok(R::ApiStatus {
                    status: Box::new(i.api_status()),
                })
            }
            C::GetUpdaterStatus => Ok(R::UpdaterStatus {
                status: Box::new(i.updater_status()),
            }),
            C::SetUpdaterSettings {
                endpoint,
                public_key,
            } => Ok(R::UpdaterStatus {
                status: Box::new(i.set_updater_settings(&endpoint, &public_key)?),
            }),
            C::CheckUpdate => Ok(R::Update {
                update: Box::new(i.check_update()?),
            }),
            C::DownloadUpdate => Ok(R::Update {
                update: Box::new(i.download_update()?),
            }),
            C::InstallUpdate => {
                tracing::info!(command = "install_update");
                i.install_update()?;
                Ok(R::Ok)
            }
            C::GetShellMenu => Ok(R::ShellMenu {
                status: i.shell_menu_status(),
            }),
            C::InstallShellMenu => Ok(R::ShellMenu {
                status: i.install_shell_menu()?,
            }),
            C::RemoveShellMenu => Ok(R::ShellMenu {
                status: i.remove_shell_menu()?,
            }),
            C::CheckNetwork { force } => Ok(R::Network {
                status: Box::new(i.network_status(force)),
            }),
            C::ExportSupportBundle { dest } => Ok(R::Lines {
                lines: i.export_support_bundle(&dest)?,
            }),

            C::LoadOverview { project_id } => Ok(R::LoadOverview {
                overview: Box::new(i.load_overview(&project_id)?),
            }),
            C::LoadReadScript { project_id, name } => Ok(R::Text {
                text: i.load_read_script(&project_id, &name)?,
            }),
            C::LoadSaveScript {
                project_id,
                name,
                content,
            } => {
                i.load_save_script(&project_id, &name, &content)?;
                Ok(R::Ok)
            }
            C::LoadDeleteScript { project_id, name } => {
                i.load_delete_script(&project_id, &name)?;
                Ok(R::Ok)
            }
            C::LoadListProfiles => Ok(R::LoadProfiles {
                profiles: i.load_profiles(),
            }),
            C::LoadSaveProfile { profile } => Ok(R::LoadProfiles {
                profiles: i.load_save_profile(profile)?,
            }),
            C::LoadDeleteProfile { id } => Ok(R::LoadProfiles {
                profiles: i.load_delete_profile(&id)?,
            }),
            C::LoadGenerate {
                project_id,
                profile,
                name,
            } => Ok(R::Text {
                text: i.load_generate(&project_id, &profile, name.as_deref())?,
            }),
            C::LoadRun {
                project_id,
                script,
                target,
                confirm_public,
                env,
            } => {
                tracing::info!(command = "load_run", project = %project_id, script = %script, public = confirm_public);
                Ok(R::LoadRun {
                    run: Box::new(i.load_run(
                        &project_id,
                        &script,
                        target.as_deref(),
                        confirm_public,
                        &env,
                    )?),
                })
            }
            C::LoadStatus { run_id } => Ok(R::LoadRun {
                run: Box::new(i.load_status(&run_id)?),
            }),
            C::LoadStop { run_id } => Ok(R::LoadRun {
                run: Box::new(i.load_stop(&run_id)?),
            }),
            C::LoadRuns { project_id } => Ok(R::LoadRuns {
                runs: i.load_runs(&project_id),
            }),
            C::LoadDeleteRun { project_id, run_id } => {
                i.load_delete_run(&project_id, &run_id)?;
                Ok(R::Ok)
            }

            C::AiGetState => Ok(R::AiState {
                state: Box::new(i.ai_state()),
            }),
            C::AiSaveSettings { enabled, features } => Ok(R::AiState {
                state: Box::new(i.ai_save_settings(enabled, features)?),
            }),
            C::AiSaveProvider { provider, api_key } => {
                // Only that a provider was saved, never its key (§141).
                tracing::info!(command = "ai_save_provider", provider = %provider.name, key_changed = api_key.is_some());
                Ok(R::AiState {
                    state: Box::new(i.ai_save_provider(provider, api_key)?),
                })
            }
            C::AiRemoveProvider { id } => Ok(R::AiState {
                state: Box::new(i.ai_remove_provider(&id)?),
            }),
            C::AiDetectLocal => Ok(R::AiDetected {
                servers: i.ai_detect_local(),
            }),
            C::AiTest { provider_id } => Ok(R::AiTest {
                result: i.ai_test(&provider_id)?,
            }),
            C::AiProbe { provider, api_key } => Ok(R::AiTest {
                result: i.ai_probe(provider, api_key)?,
            }),
            C::AiModels { provider_id } => Ok(R::AiModels {
                models: i.ai_models(&provider_id)?,
            }),
            C::AiPreview { request } => Ok(R::AiPrompt {
                prompt: Box::new(i.ai_prompt(&request)?),
            }),
            C::AiStart {
                request,
                confirm_remote,
            } => {
                tracing::info!(command = "ai_start", feature = %request.feature, confirm_remote);
                Ok(R::AiJob {
                    job: Box::new(i.ai_start(request, confirm_remote)?),
                })
            }
            C::AiJob { job_id } => Ok(R::AiJob {
                job: Box::new(i.ai_job(&job_id)?),
            }),
            C::AiCancel { job_id } => Ok(R::AiJob {
                job: Box::new(i.ai_cancel(&job_id)?),
            }),
            C::AiApply {
                actions,
                confirm_destructive,
            } => Ok(R::AiApplied {
                steps: self.ai_apply(actions, confirm_destructive),
            }),
            C::GetAppGuide { version: _ } => Ok(R::AppGuide {
                markdown: i.app_guide(),
                version: "2.3.0".to_string(),
            }),
            // @@arms-end
        }
    }
}

