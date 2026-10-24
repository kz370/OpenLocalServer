use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use serde::{Deserialize, Serialize};

use super::types::*;
use crate::app::Inner;
use crate::error::CoreError;

use std::time::Duration;
use crate::journal::Journal;
use crate::manifest::LockFile;
use crate::process::ProcessSpec;

impl Inner {
    pub fn setup_progress(&self) -> Option<SetupReport> {
        self.setup.lock().unwrap().clone()
    }

    /// §73: runs the plan. With `dry_run` nothing changes and the report lists what would.
    pub fn apply_setup(&self, project_id: &str, dry_run: bool) -> Result<SetupReport, CoreError> {
        let plan = self.plan_setup(project_id)?;
        let mut report = SetupReport {
            project_id: project_id.to_string(),
            running: !dry_run,
            dry_run,
            ok: plan.ok,
            conflicts: plan.conflicts.clone(),
            steps: plan
                .steps
                .iter()
                .map(|s| StepResult {
                    group: s.group.clone(),
                    label: s.label.clone(),
                    status: if s.done {
                        StepStatus::Skipped
                    } else {
                        StepStatus::Pending
                    },
                    detail: s.note.clone(),
                })
                .collect(),
            ..Default::default()
        };
        if dry_run {
            report.running = false;
            return Ok(report);
        }
        if !plan.ok {
            report.running = false;
            report.error = Some("The plan has blocking conflicts. Resolve them first.".into());
            return Ok(report);
        }
        *self.setup.lock().unwrap() = Some(report.clone());

        let title = format!("Set up {}", plan.project_name);
        let retry = crate::command::CoreCommand::ApplySetup {
            project_id: project_id.to_string(),
            dry_run: false,
        };
        let result = self.journaled(
            "setup",
            &title,
            Some("Failed setups undo their safe changes automatically."),
            Some(retry),
            || {
                self.run_plan(&plan, &mut report);
                match &report.error {
                    Some(e) => Err(CoreError::EnvError(e.clone())),
                    None => Ok(()),
                }
            },
        );
        report.running = false;
        report.ok = result.is_ok();
        *self.setup.lock().unwrap() = Some(report.clone());
        Ok(report)
    }

    pub(crate) fn run_plan(&self, plan: &EnvironmentPlan, report: &mut SetupReport) {
        let mut undo: Vec<Undo> = Vec::new();
        let mut installed: LockFile = LockFile::new();
        let path = PathBuf::from(&plan.project_path);
        for (i, step) in plan.steps.iter().enumerate() {
            if step.done {
                continue;
            }
            self.set_step(report, i, StepStatus::Running, None);
            let mut log_lines: Vec<String> = Vec::new();
            let mut log = |l: &str| log_lines.push(l.to_string());
            let outcome = self.run_action(
                plan,
                &step.action,
                &mut undo,
                &mut installed,
                &path,
                report,
                &mut log,
            );
            match outcome {
                Ok(detail) => self.set_step(
                    report,
                    i,
                    StepStatus::Done,
                    detail.or_else(|| log_lines.last().cloned()),
                ),
                Err(e) => {
                    tracing::warn!(step = %step.label, error = %e, "setup step failed");
                    self.set_step(report, i, StepStatus::Failed, Some(e.clone()));
                    for later in report.steps.iter_mut().skip(i + 1) {
                        if later.status == StepStatus::Pending {
                            later.status = StepStatus::NotRun;
                        }
                    }
                    report.rolled_back = self.roll_back(undo);
                    if !report.rolled_back.is_empty() {
                        for (j, s) in plan.steps.iter().enumerate().take(i) {
                            if !s.done && Self::reversible(&s.action) {
                                report.steps[j].status = StepStatus::RolledBack;
                            }
                        }
                    }
                    report.error = Some(format!("{}: {e}", step.label));
                    *self.setup.lock().unwrap() = Some(report.clone());
                    return;
                }
            }
        }
    }

    fn reversible(action: &SetupAction) -> bool {
        matches!(
            action,
            SetupAction::AddDomain { .. }
                | SetupAction::UpdateDomain { .. }
                | SetupAction::StartService { .. }
                | SetupAction::EnableExtension { .. }
                | SetupAction::ImportCommands { .. }
                | SetupAction::AddWorker { .. }
                | SetupAction::AddSchedule { .. }
                | SetupAction::StartWorkers
        )
    }

    fn set_step(
        &self,
        report: &mut SetupReport,
        i: usize,
        status: StepStatus,
        detail: Option<String>,
    ) {
        report.steps[i].status = status;
        if detail.is_some() {
            report.steps[i].detail = detail;
        }
        *self.setup.lock().unwrap() = Some(report.clone());
    }

    #[allow(clippy::too_many_arguments)]
    fn run_action(
        &self,
        plan: &EnvironmentPlan,
        action: &SetupAction,
        undo: &mut Vec<Undo>,
        installed: &mut LockFile,
        path: &Path,
        report: &mut SetupReport,
        log: &mut dyn FnMut(&str),
    ) -> Result<Option<String>, String> {
        let pid = plan.project_id.as_str();
        match action {
            SetupAction::InstallRuntime { id, version } => {
                self.install_runtime_blocking(id, version, log)?;
                installed.insert(id.clone(), version.clone());
                if id == "php" {
                    self.sync_php_external();
                }
                Ok(Some(format!("{id} {version} installed")))
            }
            SetupAction::EnableExtension { php_version, name } => {
                self.php.set_extension(php_version, name, true)?;
                undo.push(Undo::DisableExtension(php_version.clone(), name.clone()));
                Ok(None)
            }
            SetupAction::EnablePackageManager { manager } => {
                let process = self
                    .enable_package_manager(pid, manager)
                    .map_err(|e| e.to_string())?;
                self.wait_process(process, std::time::Duration::from_secs(300))
            }
            SetupAction::StartService { id } => {
                let was_running = self.services.is_running(id);
                self.start_service_and_wait(id, log)?;
                if !was_running {
                    undo.push(Undo::StopService(id.clone()));
                }
                Ok(None)
            }
            SetupAction::CreateDatabase { engine, name } => {
                if engine == "mongodb" {
                    return Ok(None);
                }
                let started = std::time::Instant::now();
                loop {
                    match self.services.create_database(engine, name) {
                        Ok(()) => return Ok(None),
                        Err(e)
                            if started.elapsed() < std::time::Duration::from_secs(30)
                                && e.to_lowercase().contains("connect") =>
                        {
                            std::thread::sleep(std::time::Duration::from_millis(700))
                        }
                        Err(e) => return Err(e),
                    }
                }
            }
            SetupAction::CreateSqlite { path: file } => {
                let exe = self.sqlite3_path().map_err(|e| e.to_string())?;
                if let Some(parent) = Path::new(file).parent() {
                    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                crate::sqlite::create(&exe, Path::new(file))?;
                self.sqlite
                    .lock()
                    .unwrap()
                    .associate(file, Some(pid.to_string()))
                    .map_err(|e| e.to_string())?;
                Ok(None)
            }
            SetupAction::AddDomain { domain } => {
                self.add_domain((**domain).clone())
                    .map_err(|e| e.to_string())?;
                undo.push(Undo::RemoveDomain(domain.hostname.clone()));
                Ok(None)
            }
            SetupAction::UpdateDomain { domain } => {
                let before = self
                    .domains
                    .lock()
                    .unwrap()
                    .get(&domain.hostname)
                    .ok_or("the site disappeared")?;
                self.update_domain((**domain).clone())
                    .map_err(|e| e.to_string())?;
                undo.push(Undo::RestoreDomain(Box::new(before)));
                Ok(None)
            }
            SetupAction::SyncHosts => {
                let hostnames: Vec<String> = self
                    .domains
                    .lock()
                    .unwrap()
                    .list()
                    .into_iter()
                    .filter(|d| d.enabled && !self.web.dns_covers(&d.hostname))
                    .map(|d| d.hostname)
                    .collect();
                crate::hosts::ensure(&hostnames)?;
                Ok(None)
            }
            SetupAction::TrustCa => {
                self.certs.ca().ensure_created()?;
                self.certs.ca().trust_current_user()?;
                Ok(None)
            }
            SetupAction::ConfigureMail { file } => {
                let plan = self
                    .apply_mailpit_env(pid, file)
                    .map_err(|e| e.to_string())?;
                Ok(Some(format!(
                    "{} value(s) changed; the previous .env is kept as a backup",
                    plan.changes.len()
                )))
            }
            SetupAction::ImportCommands { commands } => {
                for c in commands {
                    let existed = self.quick_commands.list().iter().any(|k| k.id == c.id);
                    self.quick_commands
                        .save(c.clone())
                        .map_err(|e| e.to_string())?;
                    if !existed {
                        undo.push(Undo::DeleteCommand(c.id.clone()));
                    }
                }
                Ok(None)
            }
            SetupAction::AddWorker { worker } => {
                self.save_worker((**worker).clone())
                    .map_err(|e| e.to_string())?;
                undo.push(Undo::RemoveWorker(worker.id.clone()));
                Ok(None)
            }
            SetupAction::AddSchedule { task } => {
                self.save_schedule((**task).clone())
                    .map_err(|e| e.to_string())?;
                undo.push(Undo::RemoveSchedule(task.id.clone()));
                Ok(None)
            }
            SetupAction::ApplyWeb => {
                let reports = self.apply_web(&[]).map_err(|e| e.to_string())?;
                let mut msg = reports
                    .iter()
                    .map(|r| {
                        format!(
                            "{}: {} file(s) written{}",
                            r.server,
                            r.written.len(),
                            if r.started {
                                ", started"
                            } else if r.reloaded {
                                ", reloaded"
                            } else {
                                ""
                            }
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("; ");
                let warnings: Vec<String> =
                    reports.iter().flat_map(|r| r.warnings.clone()).collect();
                if !warnings.is_empty() {
                    msg.push_str(&format!(" ({})", warnings.join("; ")));
                }
                Ok(Some(msg))
            }
            SetupAction::StartWorkers => {
                let started = self.start_project_workers(pid).map_err(|e| e.to_string())?;
                undo.push(Undo::StopWorkers(pid.to_string()));
                Ok(Some(format!("{started} worker process(es) running")))
            }
            SetupAction::StartTunnel { provider, target } => {
                let t = self
                    .start_tunnel_for(Some(pid), provider, target, false)
                    .map_err(|e| e.to_string())?;
                Ok(Some(format!(
                    "public URL: {}",
                    t.public_url
                        .unwrap_or_else(|| "waiting for the provider".into())
                )))
            }
            SetupAction::HealthCheck { hostname } => {
                // A health check failure is reported, not rolled back: the setup itself worked.
                let h = self.health_check(hostname).map_err(|e| e.to_string())?;
                let msg = if h.ok {
                    "the site answers".to_string()
                } else {
                    h.steps
                        .iter()
                        .find(|s| !s.ok && !s.skipped)
                        .map(|s| format!("{}: {}", s.name, s.detail))
                        .unwrap_or_default()
                };
                report.health = Some(h);
                Ok(Some(msg))
            }
            SetupAction::WriteLock => {
                let lock = self.current_lock(pid, installed);
                let file = manifest::write_lock(path, &lock)?;
                report.lock_written = Some(file.display().to_string());
                Ok(Some(
                    lock.iter()
                        .map(|(k, v)| format!("{k} {v}"))
                        .collect::<Vec<_>>()
                        .join(", "),
                ))
            }
        }
    }

    /// Waits for a supervised process to exit; its exit code decides success.
    pub(crate) fn wait_process(
        &self,
        id: crate::process::ProcessId,
        timeout: std::time::Duration,
    ) -> Result<Option<String>, String> {
        let started = std::time::Instant::now();
        while self.supervisor.is_alive(id) {
            if started.elapsed() > timeout {
                self.supervisor.stop(id);
                return Err("it took too long and was stopped".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
        let info = self.supervisor.snapshot().into_iter().find(|p| p.id == id);
        match info.and_then(|p| p.exit_code) {
            Some(0) | None => Ok(None),
            Some(code) => Err(format!(
                "exited with code {code}: {}",
                self.supervisor
                    .recent_output(id)
                    .last()
                    .cloned()
                    .unwrap_or_default()
            )),
        }
    }

    fn roll_back(&self, undo: Vec<Undo>) -> Vec<String> {
        let mut done = Vec::new();
        for u in undo.into_iter().rev() {
            let (what, result): (String, Result<(), String>) = match u {
                Undo::RemoveDomain(h) => (
                    format!("removed site {h}"),
                    self.remove_domain(&h).map_err(|e| e.to_string()),
                ),
                Undo::RestoreDomain(d) => (
                    format!("restored site {}", d.hostname),
                    self.update_domain(*d)
                        .map(|_| ())
                        .map_err(|e| e.to_string()),
                ),
                Undo::StopService(id) => {
                    let stopped = self.services.stop(&id).map_err(|e| e.to_string());
                    (format!("stopped {id}"), stopped)
                }
                Undo::DisableExtension(v, n) => (
                    format!("switched PHP {v} extension {n} off again"),
                    self.php.set_extension(&v, &n, false),
                ),
                Undo::DeleteCommand(id) => (
                    format!("removed Quick Command {id}"),
                    self.quick_commands.delete(&id).map_err(|e| e.to_string()),
                ),
                Undo::RemoveWorker(id) => (
                    format!("removed worker {id}"),
                    self.remove_worker(&id).map_err(|e| e.to_string()),
                ),
                Undo::RemoveSchedule(id) => (
                    format!("removed scheduled task {id}"),
                    self.remove_schedule(&id).map_err(|e| e.to_string()),
                ),
                Undo::StopWorkers(pid) => {
                    self.stop_project_workers(&pid);
                    ("stopped the project's workers".to_string(), Ok(()))
                }
            };
            match result {
                Ok(()) => done.push(what),
                Err(e) => done.push(format!("could not undo ({what}): {e}")),
            }
        }
        done
    }

    /// Exact versions in use now, for the lock file.
    fn current_lock(&self, project_id: &str, installed: &LockFile) -> LockFile {
        let mut lock = installed.clone();
        if let Some(detail) = self.project_detail(project_id) {
            for r in detail.resolved {
                if let Some(v) = r.installed_version {
                    lock.insert(r.id, v);
                }
            }
        }
        for s in self.services.list() {
            if s.running && s.kind != "custom" {
                if let Some(v) = s.version {
                    lock.entry(s.id).or_insert(v);
                }
            }
        }
        let cfg = self.web_config();
        if let Some(v) = self
            .runtimes
            .installed_versions(cfg.server())
            .into_iter()
            .next()
        {
            lock.insert(cfg.default_server, v);
        }
        lock
    }
}
