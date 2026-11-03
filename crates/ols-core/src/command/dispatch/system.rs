use super::super::types::*;
use super::super::Core;
use crate::error::CoreError;
use crate::port;
use std::time::Duration;

impl Core {
    pub(crate) fn dispatch_system(
        &self,
        command: CoreCommand,
    ) -> Result<Result<CoreResponse, CoreError>, CoreCommand> {
        use CoreCommand as C;
        use CoreResponse as R;
        let i = &self.inner;
        #[allow(unreachable_patterns)]
        let mut unhandled = None;
        let outcome: Result<CoreResponse, CoreError> = (|| {
            let res = match command {
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
                            Some(arr) => arr.iter().any(|v| {
                                v.as_str().is_some_and(|folder| !folder.trim().is_empty())
                            }),
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
                    let version_dir = i.runtimes.version_dir(&id, &version);
                    if i.supervisor.any_alive_in_dir(&version_dir) {
                        let msg = if id == "php" {
                            format!("Stop sites using PHP {version} before removing it.")
                        } else {
                            format!("Stop {id} before removing {id} {version}.")
                        };
                        return Err(CoreError::ServiceError(msg));
                    }
                    let was_default = i.runtimes.catalog().iter().any(|entry| {
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
                    let project = {
                        let mut projects = i.projects.lock().unwrap();
                        let project = projects.register(&path)?;
                        projects.set_standalone(&project.id, false)?;
                        projects.get(&project.id).unwrap_or(project)
                    };
                    let _ = i.clear_project_skip(&project.path);
                    if let Err(e) = i.sync_auto_domains() {
                        tracing::warn!(error = %e, "automatic domains could not be synced");
                    }
                    Ok(R::Project { project })
                }
                C::AddProject {
                    path,
                    name,
                    project_type,
                    auto_site,
                } => {
                    tracing::info!(command = "add_project", path = %path);
                    let effective_type = match project_type.as_deref() {
                        None | Some("auto") => {
                            let inspection = i.inspect_project_folder(&path);
                            inspection.detected_type
                        }
                        Some(t) if t.trim().is_empty() => {
                            let inspection = i.inspect_project_folder(&path);
                            inspection.detected_type
                        }
                        Some(t) => Some(t.to_string()),
                    };
                    let project = {
                        let mut projects = i.projects.lock().unwrap();
                        let existing = projects
                            .list()
                            .into_iter()
                            .find(|project| crate::project::same_path(&project.path, &path));
                        if let Some(existing) = existing {
                            projects.set_standalone(&existing.id, !auto_site)?;
                            projects.get(&existing.id).unwrap_or(existing)
                        } else {
                            projects.add_new(&path, name, effective_type, !auto_site)?
                        }
                    };
                    let _ = i.clear_project_skip(&project.path);
                    if auto_site {
                        match i.create_site_for_project(&project.id) {
                            Ok(result) if result.needs_setup => {
                                tracing::info!(project = %project.name, "automatic site creation needs project setup");
                            }
                            Ok(_) => {}
                            Err(error) => {
                                tracing::warn!(project = %project.name, %error, "automatic site creation failed after project registration");
                            }
                        }
                    }
                    i.notify_project_events();
                    Ok(R::Project { project })
                }
                C::UpdateProject {
                    id,
                    name,
                    icon,
                    clear_icon,
                    project_type,
                    clear_project_type,
                } => {
                    tracing::info!(command = "update_project", id = %id);
                    let project = i.update_project(
                        &id,
                        name,
                        icon,
                        clear_icon,
                        project_type,
                        clear_project_type,
                    )?;
                    Ok(R::Project { project })
                }
                C::CreateSiteForProject { project_id } => {
                    tracing::info!(command = "create_site_for_project", project_id = %project_id);
                    let res = i.create_site_for_project(&project_id)?;
                    Ok(R::CreateSiteForProject {
                        domain: res.domain,
                        needs_setup: res.needs_setup,
                        suggested_hostname: res.suggested_hostname,
                    })
                }
                C::OpenProjectTerminal { project_id } => {
                    tracing::info!(command = "open_project_terminal", project_id = %project_id);
                    i.open_project_terminal(&project_id)?;
                    Ok(R::Ok)
                }
                C::ListProjectTypes => Ok(R::ProjectTypes {
                    types: crate::project::list_project_types(),
                }),
                C::InspectProjectFolder { path } => {
                    tracing::info!(command = "inspect_project_folder", path = %path);
                    Ok(R::ProjectFolderInspection {
                        inspection: i.inspect_project_folder(&path),
                    })
                }
                C::ListCustomProjectTypes => Ok(R::CustomProjectTypes {
                    types: i.list_custom_project_types(),
                }),
                C::AddCustomProjectType { name } => {
                    tracing::info!(command = "add_custom_project_type", name = %name);
                    let types = i.add_custom_project_type(&name)?;
                    Ok(R::CustomProjectTypes { types })
                }
                C::RenameCustomProjectType { old_name, new_name } => {
                    tracing::info!(command = "rename_custom_project_type", old_name = %old_name, new_name = %new_name);
                    let types = i.rename_custom_project_type(&old_name, &new_name)?;
                    Ok(R::CustomProjectTypes { types })
                }
                C::RemoveCustomProjectType { name } => {
                    tracing::info!(command = "remove_custom_project_type", name = %name);
                    let types = i.remove_custom_project_type(&name)?;
                    Ok(R::CustomProjectTypes { types })
                }
                C::TakePendingAddProject => Ok(R::PendingAddProject {
                    path: i.take_pending_add_project(),
                }),
                C::ScanAndRegisterProjects { path } => {
                    tracing::info!(command = "scan_and_register_projects", path = %path);
                    let root = std::path::PathBuf::from(&path);
                    if !root.is_dir() {
                        return Err(CoreError::InvalidProjectPath(path));
                    }
                    let ignored = i.ignored_folders();
                    let candidates =
                        crate::detection::scan_for_projects_with_ignored(&root, &ignored);
                    // Read the skips before taking the project store, then judge every candidate
                    // the same way the watcher does: a folder the user deleted stays deleted
                    // whichever path spelling this scan happened to walk.
                    let skipped = i.project_skips();
                    let hidden_domains = i.skip_entries(crate::app::AUTO_SKIP);
                    let existing_projects = i.projects.lock().unwrap().list();
                    let hidden_paths: Vec<_> = i.domains.lock().unwrap().list().into_iter()
                        .filter(|d| hidden_domains.iter().any(|entry| entry.value.eq_ignore_ascii_case(&d.hostname)))
                        .map(|d| crate::app::site_folder(&d, &existing_projects))
                        .collect();
                    let mut projects = i.projects.lock().unwrap();
                    let mut registered = Vec::with_capacity(candidates.len());
                    for candidate in candidates {
                        if let Some(s) = candidate.to_str() {
                            if candidate.join(".ols_ignore_me").exists() {
                                continue;
                            }
                            if hidden_paths.iter().any(|path| crate::project::same_path(path, s)) {
                                continue;
                            }
                            // Bulk scans don't resurrect folders removed from the list.
                            if crate::app::skip_matches(&skipped, s) {
                                continue;
                            }
                            let project = projects.register(s)?;
                            projects.set_standalone(&project.id, false)?;
                            registered.push(projects.get(&project.id).unwrap_or(project));
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
                    let records = i.domains.lock().unwrap().list();
                    let hidden_hosts: std::collections::HashSet<_> = records.into_iter()
                        .filter(|d| !i.site_is_active(d))
                        .map(|d| d.hostname)
                        .collect();
                    let records = i.domains.lock().unwrap().list();
                    let project_snapshot = i.projects.lock().unwrap().list();
                    let hidden_paths: Vec<_> = records.iter()
                        .filter(|d| hidden_hosts.contains(&d.hostname))
                        .map(|d| crate::app::site_folder(d, &project_snapshot))
                        .collect();
                    let mut projects = i.projects.lock().unwrap().list();
                    projects.retain(|p| !std::path::Path::new(&p.path).join(".ols_ignore_me").exists()
                        && !hidden_paths.iter().any(|path| crate::project::same_path(path, &p.path)));
                    let roots = i.string_list(crate::app::PROJECT_ROOTS);
                    for p in &mut projects {
                        p.auto_discovered = Some(roots.iter().any(|root| {
                            std::path::Path::new(&p.path).starts_with(std::path::Path::new(root))
                        }));
                        p.icon_image = crate::app::read_site_icon_image(&p.path);
                    }
                    Ok(R::Projects { projects })
                }
                C::RemoveProject { id } => {
                    i.remove_project(&id)?;
                    Ok(R::Ok)
                }
                C::IgnoreProject { id } => {
                    i.ignore_project(&id)?;
                    i.reconcile()?;
                    Ok(R::Ok)
                }
                C::RemoveSiteFromList { hostname } => {
                    i.remove_site_from_list(&hostname)?;
                    Ok(R::Ok)
                }
                C::RelocateProject { id, path } => {
                    tracing::info!(command = "relocate_project", id = %id, path = %path);
                    let project = i.relocate_project(&id, &path)?;
                    Ok(R::Project { project })
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
                    i.reconcile()?;
                    Ok(R::DeletedItems {
                        items: i.list_deleted_items(),
                    })
                }
                C::ClearDeletedItems => {
                    i.clear_deleted_items()?;
                    i.reconcile()?;
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
