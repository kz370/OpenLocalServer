//! Project diagnostics (§112), the environment doctor (§113), automatic repair (§114) and
//! explained diagnostics (§115).
//!
//! **Doctor** is one report of everything the app depends on: OS, runtimes, web server,
//! databases, caches, mail, DNS, the local CA and Git, as ✓ / ⚠ / ✗ lines, plus every
//! finding from the DiagnosticEngine. `ols doctor` prints it.
//!
//! **Repair** follows §114 step by step: diagnose (the app-wide findings plus checks for
//! one project), explain each issue, show the fixes it proposes, ask before anything
//! destructive, apply the safe fixes, then diagnose again. Fixes are only ever the
//! `CoreCommand` a finding carries, run through the normal dispatcher.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::app::Inner;
use crate::command::{Core, CoreCommand};
use crate::diagnostics::{Finding, Severity};
use crate::error::CoreError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorCheck {
    pub label: String,
    /// ok, warning, error, info.
    pub status: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorReport {
    pub checks: Vec<DoctorCheck>,
    pub findings: Vec<Finding>,
    pub warnings: usize,
    pub errors: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairAction {
    pub finding_id: String,
    pub label: String,
    pub command: CoreCommand,
    /// Replaces or removes something; needs a separate confirmation.
    pub destructive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairPlan {
    pub project_id: Option<String>,
    pub findings: Vec<Finding>,
    pub actions: Vec<RepairAction>,
    /// Issues only the user can fix (no automatic fix exists).
    pub manual: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairStep {
    pub label: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairReport {
    pub steps: Vec<RepairStep>,
    /// Findings after the repair (§114.6: health checks re-run).
    pub after: Vec<Finding>,
    pub fixed: usize,
}

/// Fixes that overwrite or delete something the user may want to keep.
pub fn is_destructive(cmd: &CoreCommand) -> bool {
    match cmd {
        CoreCommand::CreateVenv { recreate, .. } => *recreate,
        CoreCommand::ApplyWeb { overwrite } => !overwrite.is_empty(),
        CoreCommand::RestoreDatabase { .. }
        | CoreCommand::RemoveDomain { .. }
        | CoreCommand::BulkRemoveDomains { .. }
        | CoreCommand::RestoreSnapshot { .. }
        | CoreCommand::RevokeCertificate { .. } => true,
        // An import onto an existing site replaces its configuration; a new site adds one.
        CoreCommand::ImportSites { on_conflict, .. } => {
            *on_conflict == crate::sitebundle::OnConflict::Update
        }
        _ => false,
    }
}

fn finding(
    id: String,
    severity: Severity,
    problem: String,
    cause: &str,
    fix: &str,
    fix_command: Option<CoreCommand>,
    details: Vec<String>,
) -> Finding {
    let auto_fixable = fix_command.as_ref().is_some_and(|cmd| !is_destructive(cmd));
    Finding {
        id,
        severity,
        problem,
        cause: cause.into(),
        fix: fix.into(),
        fix_command,
        auto_fixable,
        details,
        ignored: false,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoFixResult {
    pub id: String,
    pub problem: String,
    pub ok: bool,
    pub detail: String,
}

fn env_value(project: &Path, file: &str, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(project.join(file)).ok()?;
    text.lines().find_map(|l| {
        let (k, v) = l.trim().split_once('=')?;
        (k.trim() == key).then(|| v.trim().trim_matches('"').to_string())
    })
}

impl Inner {
    /// §112 for one project: runtimes, .env, APP_KEY, dependencies, its services and site.
    pub fn diagnose_project(&self, project_id: &str) -> Result<Vec<Finding>, CoreError> {
        let detail = self
            .project_detail(project_id)
            .ok_or_else(|| CoreError::InvalidProjectPath(project_id.to_string()))?;
        let root = Path::new(&detail.project.path);
        let name = detail.project.name.clone();
        let mut out = Vec::new();

        for r in &detail.resolved {
            if let (Some(wanted), None) = (&r.requested_version, &r.installed_version) {
                let available = crate::catalog::builtin_catalog()
                    .into_iter()
                    .filter(|m| m.id == r.id)
                    .map(|m| m.version.to_string())
                    .collect::<Vec<_>>();
                let fix = crate::php::pick_version(&available, Some(wanted)).map(|version| {
                    CoreCommand::InstallRuntime {
                        id: r.id.clone(),
                        version,
                    }
                });
                out.push(finding(
                    format!("project_runtime_missing:{}:{}", project_id, r.id),
                    Severity::Error,
                    format!("{name} needs {} {wanted}, which is not installed", r.id.to_uppercase()),
                    "The project asks for this version (in its manifest or its own files), and no installed version matches.",
                    if fix.is_some() { "Install it." } else { "Install it yourself and point OLS at it on the Runtimes page." },
                    fix,
                    vec![format!("Requested by: {:?}", r.source).to_lowercase()],
                ));
            }
        }

        let has_example = root.join(".env.example").is_file();
        if has_example && !root.join(".env").is_file() {
            out.push(finding(
                format!("env_missing:{project_id}"),
                Severity::Error,
                format!("{name} has no .env file"),
                "The project comes with .env.example, but its own .env was never created, so it has no settings to run with.",
                "Create .env from .env.example.",
                Some(CoreCommand::CreateEnvFile { project_id: project_id.into(), file: ".env".into(), from: Some(".env.example".into()) }),
                vec![],
            ));
        }
        if detail.detection.framework == crate::detection::Framework::Laravel
            && root.join(".env").is_file()
            && env_value(root, ".env", "APP_KEY").is_none_or(|k| k.is_empty())
        {
            out.push(finding(
                format!("app_key_missing:{project_id}"),
                Severity::Error,
                "APP_KEY missing".to_string(),
                "Laravel encrypts sessions and cookies with APP_KEY; without it every page fails with \"No application encryption key\".",
                "Generate one with php artisan key:generate.",
                Some(CoreCommand::RunCommandLine { line: "php artisan key:generate".into(), cwd: None, project_id: Some(project_id.into()) }),
                vec![],
            ));
        }
        if root.join("package.json").is_file() && !root.join("node_modules").is_dir() {
            let pm = crate::nodepm::info(root, None)
                .detected
                .unwrap_or_else(|| "npm".into());
            out.push(finding(
                format!("node_modules_missing:{project_id}"),
                Severity::Info,
                format!("{name}'s Node packages are not installed"),
                "package.json lists packages but node_modules is missing, so builds and dev servers will fail.",
                &format!("Run {pm} install."),
                Some(CoreCommand::RunCommandLine { line: format!("{pm} install"), cwd: None, project_id: Some(project_id.into()) }),
                vec![],
            ));
        }
        let composer = crate::composer::read(root);
        if composer.has_composer_json && !composer.vendor_installed && !composer.packages.is_empty()
        {
            out.push(finding(
                format!("composer_not_installed:{project_id}"),
                Severity::Error,
                format!("{name} has no vendor folder"),
                "composer.json lists packages, but they were never installed, so the site can't load them.",
                "Run composer install.",
                Some(CoreCommand::RunComposer { project_id: project_id.into(), action: "install".into(), target: None }),
                vec![],
            ));
        }

        // What its manifest (or .env) says it needs running.
        let m = crate::manifest::read_manifest(root)
            .ok()
            .flatten()
            .unwrap_or_else(|| self.derive_manifest(project_id).unwrap_or_default());
        let mut services = m.enabled_services();
        if let Some(db) = &m.database {
            if let Some(s) = match db.engine.as_str() {
                "mysql" | "mariadb" => Some("mariadb"),
                "postgres" | "postgresql" => Some("postgres"),
                "mongodb" => Some("mongodb"),
                _ => None,
            } {
                services.push(s.into());
            }
        }
        services.sort();
        services.dedup();
        for s in services {
            let st = self.services.status(&s);
            if !st.installed {
                let version = crate::catalog::builtin_catalog()
                    .into_iter()
                    .find(|x| x.id == s)
                    .map(|x| x.version.to_string());
                out.push(finding(
                    format!("project_service_missing:{project_id}:{s}"),
                    Severity::Error,
                    format!("{} is not installed", st.name),
                    "This project uses it (its manifest or .env says so).",
                    "Install it.",
                    version.map(|version| CoreCommand::InstallRuntime {
                        id: s.clone(),
                        version,
                    }),
                    vec![],
                ));
            } else if !st.running {
                out.push(finding(
                    format!("project_service_stopped:{project_id}:{s}"),
                    Severity::Warning,
                    format!("{} is stopped", st.name),
                    "This project uses it, so parts of it fail until it runs.",
                    "Start it.",
                    Some(CoreCommand::StartService { id: s.clone() }),
                    vec![],
                ));
            }
        }
        if let Some(d) = &m.domain {
            if !d.hostname.is_empty() && self.domains.lock().unwrap().get(&d.hostname).is_none() {
                out.push(finding(
                    format!("project_site_missing:{project_id}"),
                    Severity::Warning,
                    format!("{} is not set up yet", d.hostname),
                    "The manifest names this site, but it doesn't exist here.",
                    "Run the environment setup for the project.",
                    Some(CoreCommand::ApplySetup {
                        project_id: project_id.into(),
                        dry_run: false,
                    }),
                    vec![],
                ));
            }
        }
        let ignored = self.ignored_diagnostics();
        for f in &mut out {
            f.ignored = ignored.contains(&f.id);
        }
        Ok(out)
    }

    /// §113.
    pub fn doctor(&self) -> DoctorReport {
        let mut checks = Vec::new();
        let mut add = |label: &str, status: &str, detail: String| {
            checks.push(DoctorCheck {
                label: label.into(),
                status: status.into(),
                detail,
            })
        };

        add(
            "Operating system supported",
            if cfg!(windows) { "ok" } else { "warning" },
            std::env::consts::OS.to_string(),
        );
        for (id, label) in [("php", "PHP available"), ("node", "Node available")] {
            let mut v = self.runtimes.installed_versions(id);
            v.extend(
                self.custom_installs
                    .lock()
                    .unwrap()
                    .list()
                    .into_iter()
                    .filter(|c| c.id == id)
                    .map(|c| c.label),
            );
            match v.is_empty() {
                false => add(label, "ok", v.join(", ")),
                true => add(
                    label,
                    "info",
                    "not installed (install it on the Runtimes page when a project needs it)"
                        .into(),
                ),
            }
        }
        match crate::runtime::detect_system_install("python") {
            Some(p) => add("Python available", "ok", p.version),
            None => add("Python available", "info", "not found on PATH".into()),
        }

        let cfg = self.web_config();
        let server = crate::web::server_by_id(cfg.server())
            .map(|s| s.name())
            .unwrap_or("Web server")
            .to_string();
        if self.runtimes.installed_versions(cfg.server()).is_empty() {
            add(&format!("{server} valid"), "error", "not installed".into());
        } else {
            match self.web.validate(&cfg) {
                Ok(text) => add(
                    &format!("{server} valid"),
                    "ok",
                    text.lines()
                        .last()
                        .unwrap_or("configuration is valid")
                        .to_string(),
                ),
                Err(e) => add(&format!("{server} valid"), "error", e.to_string()),
            }
        }
        for s in self
            .services
            .list()
            .into_iter()
            .filter(|s| s.kind != "custom")
        {
            let label = format!("{} valid", s.name);
            match (s.installed, s.running, s.healthy) {
                (false, _, _) => add(&label, "info", "not installed".into()),
                (true, false, _) => add(&label, "info", "installed, stopped".into()),
                (true, true, Some(false)) => add(
                    &label,
                    "error",
                    format!("{} unreachable on port {}", s.name, s.port.unwrap_or(0)),
                ),
                (true, true, _) => add(
                    &label,
                    "ok",
                    s.port
                        .map(|p| format!("running on port {p}"))
                        .unwrap_or_else(|| "running".into()),
                ),
            }
        }
        let web = self.web.status(&cfg, &self.domains.lock().unwrap().list());
        add(
            "DNS valid",
            if web.dns_running { "ok" } else { "info" },
            if web.dns_running {
                format!("local DNS on port {}", web.dns_port)
            } else {
                "local DNS starts with the web server".into()
            },
        );
        let ca = self.certs.ca_info();
        add(
            "Local CA trusted",
            if ca.trusted {
                "ok"
            } else if ca.exists {
                "warning"
            } else {
                "info"
            },
            if ca.trusted {
                "browsers trust local HTTPS sites".into()
            } else if ca.exists {
                "not trusted yet".into()
            } else {
                "not created yet (made with the first HTTPS site)".into()
            },
        );
        for v in self.php.all_versions() {
            let x = self.php.xdebug_report(&v);
            if !x.enabled {
                add(
                    &format!("Xdebug (PHP {v})"),
                    "warning",
                    "Xdebug disabled".into(),
                );
            }
        }
        match self.git_path() {
            Some(p) => add("Git available", "ok", p.display().to_string()),
            None => add(
                "Git available",
                "info",
                "not found (install portable Git on the Runtimes page)".into(),
            ),
        }
        if self.tunnels.any_running() {
            add(
                "Public tunnels",
                "warning",
                "at least one project is public right now".into(),
            );
        }

        let findings = self
            .diagnose()
            .into_iter()
            .filter(|f| !f.ignored)
            .collect::<Vec<_>>();
        let warnings = checks.iter().filter(|c| c.status == "warning").count()
            + findings
                .iter()
                .filter(|f| f.severity == Severity::Warning)
                .count();
        let errors = checks.iter().filter(|c| c.status == "error").count()
            + findings
                .iter()
                .filter(|f| f.severity == Severity::Error)
                .count();
        DoctorReport {
            checks,
            findings,
            warnings,
            errors,
        }
    }

    /// §114 steps 1–3: diagnose, explain, propose.
    pub fn plan_repair(&self, project_id: Option<&str>) -> Result<RepairPlan, CoreError> {
        let mut findings: Vec<Finding> =
            self.diagnose().into_iter().filter(|f| !f.ignored).collect();
        if let Some(p) = project_id {
            let own = self.diagnose_project(p)?;
            // A project's own check replaces the app-wide one about the same thing.
            findings.retain(|f| !own.iter().any(|o| o.id == f.id));
            findings.extend(own.into_iter().filter(|f| !f.ignored));
        }
        let mut actions = Vec::new();
        let mut manual = Vec::new();
        for f in &findings {
            match &f.fix_command {
                Some(cmd) => actions.push(RepairAction {
                    finding_id: f.id.clone(),
                    label: f.fix.clone(),
                    destructive: is_destructive(cmd),
                    command: cmd.clone(),
                }),
                None => manual.push(format!("{}: {}", f.problem, f.fix)),
            }
        }
        Ok(RepairPlan {
            project_id: project_id.map(str::to_string),
            findings,
            actions,
            manual,
        })
    }
}

impl Core {
    /// Apply newly discovered, safe diagnostic fixes at most once per finding per session.
    /// A failed attempt is retained on the finding so periodic scans do not retry in a loop.
    pub fn auto_fix_diagnostics(&self) -> Vec<AutoFixResult> {
        if !self.inner().setting_bool("diagnostics.auto_fix", true) {
            return Vec::new();
        }
        let findings = auto_fix_findings(self.inner());
        let mut results = Vec::new();
        for f in findings
            .into_iter()
            .filter(|f| !f.ignored && f.auto_fixable)
        {
            {
                let mut attempted = self.inner().auto_fix_attempted.lock().unwrap();
                if !attempted.insert(f.id.clone()) {
                    continue;
                }
            }
            let Some(command) = f.fix_command.as_ref() else {
                continue;
            };
            match self.run_fix(command) {
                Ok(detail) => {
                    self.inner().auto_fix_failures.lock().unwrap().remove(&f.id);
                    results.push(AutoFixResult {
                        id: f.id,
                        problem: f.problem,
                        ok: true,
                        detail,
                    });
                }
                Err(detail) => {
                    self.inner()
                        .auto_fix_failures
                        .lock()
                        .unwrap()
                        .insert(f.id.clone(), detail.clone());
                    results.push(AutoFixResult {
                        id: f.id,
                        problem: f.problem,
                        ok: false,
                        detail,
                    });
                }
            }
        }
        results
    }

    /// Runs one fix command through the dispatcher and waits for what it starts, so the caller can
    /// diagnose again straight after. Shared by repairs and by the AI assistant's approved plans.
    pub(crate) fn run_fix(&self, command: &CoreCommand) -> Result<String, String> {
        let i = self.inner();
        let outcome = self.dispatch(command.clone());
        // Fixes that start a process (composer install, key:generate) are waited for.
        let result = match outcome {
            Ok(crate::command::CoreResponse::ProcessStarted { id }) => i
                .wait_process(id, std::time::Duration::from_secs(900))
                .map(|d| d.unwrap_or_else(|| "done".into()))
                .map_err(|e| e.to_string()),
            Ok(crate::command::CoreResponse::Setup { report }) if !report.ok => Err(report
                .error
                .clone()
                .unwrap_or_else(|| "setup failed".into())),
            Ok(_) => Ok("done".to_string()),
            Err(d) => Err(format!("{} {}", d.problem, d.cause)),
        };
        if let (CoreCommand::InstallRuntime { id, version }, true) = (command, result.is_ok()) {
            // Installs run in the background; wait for them so the re-check sees them.
            let started = std::time::Instant::now();
            while !i.runtimes.installed_versions(id).contains(version)
                && started.elapsed() < std::time::Duration::from_secs(1800)
            {
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        }
        result
    }

    /// §114 steps 4–6: applies the chosen fixes (destructive ones only with `confirm_destructive`),
    /// then diagnoses again.
    pub fn apply_repair(
        &self,
        project_id: Option<&str>,
        ids: &[String],
        confirm_destructive: bool,
    ) -> Result<RepairReport, CoreError> {
        let i = self.inner();
        let plan = i.plan_repair(project_id)?;
        let mut steps = Vec::new();
        let chosen: Vec<&RepairAction> = plan
            .actions
            .iter()
            .filter(|a| ids.is_empty() || ids.contains(&a.finding_id))
            .collect();
        for a in chosen {
            if a.destructive && !confirm_destructive {
                steps.push(RepairStep {
                    label: a.label.clone(),
                    ok: false,
                    detail:
                        "skipped: this fix replaces or removes something; confirm it separately"
                            .into(),
                });
                continue;
            }
            let result = self.run_fix(&a.command);
            match result {
                Ok(d) => {
                    i.auto_fix_failures.lock().unwrap().remove(&a.finding_id);
                    steps.push(RepairStep {
                        label: a.label.clone(),
                        ok: true,
                        detail: d,
                    });
                }
                Err(e) => steps.push(RepairStep {
                    label: a.label.clone(),
                    ok: false,
                    detail: e,
                }),
            }
        }
        let after = i.plan_repair(project_id)?.findings;
        let fixed = plan
            .findings
            .iter()
            .filter(|f| !after.iter().any(|a| a.id == f.id))
            .count();
        tracing::info!(project = ?project_id, fixed, "repair");
        Ok(RepairReport {
            steps,
            after,
            fixed,
        })
    }
}

fn auto_fix_findings(inner: &Inner) -> Vec<Finding> {
    let mut findings = inner.diagnose();
    let projects = inner.projects.lock().unwrap().list();
    for project in projects {
        match inner.diagnose_project(&project.id) {
            Ok(project_findings) => findings.extend(project_findings),
            Err(error) => {
                tracing::warn!(project = %project.id, %error, "could not diagnose project for automatic fixes")
            }
        }
    }
    findings
}

/// `ols doctor` as text.
pub fn doctor_text(r: &DoctorReport) -> String {
    let mut out = String::from("OLS Doctor\n\n");
    for c in &r.checks {
        let mark = match c.status.as_str() {
            "ok" => "✓",
            "warning" => "⚠",
            "error" => "✗",
            _ => "·",
        };
        out.push_str(&format!(
            "{mark} {}{}\n",
            c.label,
            if c.detail.is_empty() {
                String::new()
            } else {
                format!(" ({})", c.detail)
            }
        ));
    }
    let warn: Vec<&Finding> = r
        .findings
        .iter()
        .filter(|f| f.severity != Severity::Error)
        .collect();
    let errs: Vec<&Finding> = r
        .findings
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .collect();
    if !warn.is_empty() {
        out.push_str("\nWarnings:\n");
        for f in warn {
            out.push_str(&format!(
                "⚠ {}\n    Cause: {}\n    Fix: {}\n",
                f.problem, f.cause, f.fix
            ));
        }
    }
    if !errs.is_empty() {
        out.push_str("\nErrors:\n");
        for f in errs {
            out.push_str(&format!(
                "✗ {}\n    Cause: {}\n    Fix: {}\n",
                f.problem, f.cause, f.fix
            ));
        }
    }
    out.push_str(&format!(
        "\n{} warning(s), {} error(s)\n",
        r.warnings, r.errors
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::CoreResponse;

    fn core() -> (Core, crate::test_support::IsolatedHome) {
        let home = crate::test_support::isolated_home();
        let settings = crate::settings::SettingsService::load(&home.paths).unwrap();
        (Core::new(settings, home.paths.clone()), home)
    }

    #[test]
    fn destructive_fixes_are_recognised() {
        assert!(is_destructive(&CoreCommand::CreateVenv {
            project_id: "p".into(),
            recreate: true
        }));
        assert!(!is_destructive(&CoreCommand::StartService {
            id: "redis".into()
        }));
        assert!(!is_destructive(&CoreCommand::ApplyWeb {
            overwrite: vec![]
        }));
        assert!(is_destructive(&CoreCommand::ApplyWeb {
            overwrite: vec!["a.test".into()]
        }));
    }

    #[test]
    fn repair_explains_fixes_and_re_checks_after_applying() {
        let (core, home) = core();
        let dir = home.paths.root().join("shop");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(".env.example"), "APP_NAME=shop\n").unwrap();
        let CoreResponse::Project { project } = core
            .dispatch(CoreCommand::RegisterProject {
                path: dir.display().to_string(),
            })
            .unwrap()
        else {
            panic!()
        };

        let plan = core.inner().plan_repair(Some(&project.id)).unwrap();
        let env = plan
            .actions
            .iter()
            .find(|a| a.finding_id.starts_with("env_missing"))
            .expect("a missing .env is found with a fix");
        assert!(!env.destructive);
        let f = plan
            .findings
            .iter()
            .find(|f| f.id == env.finding_id)
            .unwrap();
        assert!(
            !f.problem.is_empty() && !f.cause.is_empty(),
            "§115: explained"
        );

        let report = core
            .apply_repair(
                Some(&project.id),
                std::slice::from_ref(&env.finding_id),
                false,
            )
            .unwrap();
        assert!(report.steps.iter().all(|s| s.ok), "{:?}", report.steps);
        assert!(dir.join(".env").is_file());
        assert!(
            !report.after.iter().any(|f| f.id == env.finding_id),
            "the re-check no longer finds it"
        );
        assert!(report.fixed >= 1);
    }

    #[test]
    fn automatic_fix_candidates_include_registered_project_findings() {
        let (core, home) = core();
        let dir = home.paths.root().join("shop");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(".env.example"), "APP_NAME=shop\n").unwrap();
        let CoreResponse::Project { project } = core
            .dispatch(CoreCommand::RegisterProject {
                path: dir.display().to_string(),
            })
            .unwrap()
        else {
            panic!()
        };

        let findings = auto_fix_findings(core.inner());
        let missing_env = findings
            .iter()
            .find(|f| f.id == format!("env_missing:{}", project.id))
            .expect("project .env finding is scanned");
        assert!(missing_env.auto_fixable);
    }

    #[test]
    fn doctor_reports_every_dependency() {
        let (core, _home) = core();
        let r = core.inner().doctor();
        let text = doctor_text(&r);
        for want in [
            "Operating system supported",
            "PHP available",
            "Local CA trusted",
            "Git available",
        ] {
            assert!(text.contains(want), "{want} missing:\n{text}");
        }
    }
}
