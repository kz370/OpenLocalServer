use super::super::types::*;
use super::super::{helper_service_status, Core};
use crate::error::CoreError;
use std::time::Duration;

impl Core {
    pub(crate) fn dispatch_maintenance(
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
                    let p: crate::profiles::Profile =
                        serde_yaml_ng::from_str(&yaml).map_err(|e| {
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
