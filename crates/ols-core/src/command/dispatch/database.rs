use super::super::types::*;
use super::super::Core;
use crate::app::Host;
use crate::error::CoreError;
use crate::quickapp::commands::quick_command_from_line;
use std::sync::Arc;

impl Core {
    pub(crate) fn dispatch_database(
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
                C::ListDatabaseTables { engine, database } => Ok(R::Names {
                    names: i
                        .services
                        .list_tables(&engine, &database)
                        .map_err(CoreError::ServiceError)?,
                }),
                C::ExecuteSql {
                    engine,
                    database,
                    sql,
                } => Ok(R::Text {
                    text: i
                        .services
                        .run_sql_query(&engine, database.as_deref(), &sql)
                        .map_err(CoreError::ServiceError)?,
                }),
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
                    let project = i.ensure_project_available(&project_id)?;
                    let found =
                        crate::sqlite::detect_in_project(std::path::Path::new(&project.path));
                    let mut store = i.sqlite.lock().unwrap();
                    let mut out = Vec::new();
                    for f in found {
                        out.push(
                            store.associate(&f.display().to_string(), Some(project_id.clone()))?,
                        );
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
                    let cmd = quick_command_from_line(
                        &command_id,
                        &name,
                        &entry.line,
                        entry.cwd.as_deref(),
                    )
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
