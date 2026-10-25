use super::super::types::*;
use super::super::{file_name, report, Core};
use crate::error::CoreError;

impl Core {
    pub(crate) fn dispatch_ops(
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
                    crate::scheduler::Schedule::parse(&schedule)
                        .map_err(CoreError::ServiceError)?;
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
