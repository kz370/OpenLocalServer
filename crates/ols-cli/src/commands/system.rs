use std::path::PathBuf;
use std::time::Duration;
use ols_core::command::{CoreCommand, CoreResponse};
use ols_core::control;
use crate::args::ShellMenuCmd;
use crate::ctx::{confirm, table, Ctx, R};

pub fn start(ctx: &Ctx) -> R<()> {
            let r = ctx.call(CoreCommand::ApplyWeb { overwrite: vec![] })?;
            if !ctx.print_json(&r) {
                println!("Web server started.");
            }
            if let CoreResponse::Startup { settings } = ctx.call(CoreCommand::GetStartupSettings)? {
                for id in settings.autostart_services {
                    match ctx.call(CoreCommand::StartService { id: id.clone() }) {
                        Ok(_) => println!("Started {id}."),
                        Err(e) if e.contains("already running") => {}
                        Err(e) => eprintln!("{id}: {e}"),
                    }
                }
            }
    Ok(())
}

pub fn restart(ctx: &Ctx) -> R<()> {
    stop_everything(ctx)?;
    std::thread::sleep(Duration::from_millis(800));
    start(ctx)
}

pub fn status(ctx: &Ctx) -> R<()> {
            let r = ctx.call(CoreCommand::GetDashboard)?;
            if ctx.print_json(&r) {
                return Ok(());
            }
            let CoreResponse::Dashboard { data } = r else {
                return Ok(());
            };
            println!(
                "Web server: {} ({})",
                if data.web.running {
                    "running"
                } else {
                    "stopped"
                },
                data.web.default_server
            );
            let mut rows = vec![vec!["SERVICE".into(), "STATE".into(), "PORT".into()]];
            for s in data.services.iter().filter(|s| s.installed) {
                rows.push(vec![
                    s.name.clone(),
                    if s.running {
                        "running".into()
                    } else {
                        "stopped".into()
                    },
                    s.port.map(|p| p.to_string()).unwrap_or_default(),
                ]);
            }
            table(rows);
            println!(
                "{} project(s), {} site(s)",
                data.project_count,
                data.domains.len()
            );
            if let CoreResponse::Tunnels { tunnels } = ctx.call(CoreCommand::ListTunnels)? {
                for t in tunnels
                    .iter()
                    .filter(|t| t.state == "connected" || t.state == "starting")
                {
                    println!(
                        "PUBLIC: {} → {}",
                        t.config.target,
                        t.public_url.clone().unwrap_or_else(|| "connecting…".into())
                    );
                }
            }
    Ok(())
}

pub fn doctor(ctx: &Ctx) -> R<()> {
            let r = ctx.call(CoreCommand::Doctor)?;
            if !ctx.print_json(&r) {
                if let CoreResponse::DoctorReport { report } = r {
                    print!("{}", ols_core::repair::doctor_text(&report));
                    if report.errors > 0 {
                        return Err(format!(
                            "{} error(s) found; `olsc repair` fixes what it safely can",
                            report.errors
                        ));
                    }
                }
            }
    Ok(())
}

pub fn repair(
    ctx: &Ctx,
    project: Option<String>,
    dry_run: bool,
    yes: bool,
) -> R<()> {
            let pid = project.map(|p| ctx.project_id(&p)).transpose()?;
            let CoreResponse::RepairPlan { plan } = ctx.call(CoreCommand::PlanRepair {
                project_id: pid.clone(),
            })?
            else {
                return Ok(());
            };
            if ctx.json && dry_run {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&plan).unwrap_or_default()
                );
                return Ok(());
            }
            if plan.findings.is_empty() {
                println!("Nothing to repair.");
                return Ok(());
            }
            for f in &plan.findings {
                println!(
                    "• {}\n    Cause: {}\n    Fix: {}{}",
                    f.problem,
                    f.cause,
                    f.fix,
                    if f.fix_command.is_some() {
                        " (automatic)"
                    } else {
                        ""
                    }
                );
            }
            let safe = plan.actions.iter().filter(|a| !a.destructive).count();
            let destructive = plan.actions.len() - safe;
            if dry_run {
                println!(
                    "\nDry run: nothing was changed. {safe} safe fix(es) and {} destructive one(s) would run; {} issue(s) need a manual fix.",
                    destructive,
                    plan.manual.len()
                );
                return Ok(());
            }
            if safe == 0 {
                println!("\nNothing here can be fixed automatically.");
                return Ok(());
            }
            if !yes && !confirm(&format!("\nApply {safe} safe fix(es)?")) {
                println!("Nothing changed. Run with --yes to apply.");
                return Ok(());
            }
            let r = ctx.call(CoreCommand::ApplyRepair {
                project_id: pid,
                ids: vec![],
                confirm_destructive: false,
            })?;
            if !ctx.print_json(&r) {
                if let CoreResponse::RepairReport { report } = r {
                    for s in report.steps {
                        println!(
                            "{} {}{}",
                            if s.ok { "✓" } else { "✗" },
                            s.label,
                            if s.ok {
                                String::new()
                            } else {
                                format!(" ({})", s.detail)
                            }
                        );
                    }
                    println!("{} fixed, {} left.", report.fixed, report.after.len());
                }
            }
    Ok(())
}

pub fn search(ctx: &Ctx, query: Vec<String>) -> R<()> {
            let r = ctx.call(CoreCommand::GlobalSearch {
                query: query.join(" "),
            })?;
            if !ctx.print_json(&r) {
                if let CoreResponse::SearchResults { hits } = r {
                    if hits.is_empty() {
                        println!("Nothing found.");
                    }
                    table(
                        hits.into_iter()
                            .map(|h| vec![h.kind, h.title, h.excerpt.unwrap_or(h.subtitle)])
                            .collect(),
                    );
                }
            }
    Ok(())
}

pub fn network(ctx: &Ctx) -> R<()> {
            let r = ctx.call(CoreCommand::CheckNetwork { force: true })?;
            if !ctx.print_json(&r) {
                if let CoreResponse::Network { status } = r {
                    println!(
                        "{}",
                        if status.online {
                            "Online."
                        } else {
                            "Offline: downloads, tunnels and updates won't work."
                        }
                    );
                    for p in status.probes {
                        println!("  {} {}", if p.ok { "✓" } else { "✗" }, p.name);
                    }
                }
            }
    Ok(())
}

pub fn support_bundle(ctx: &Ctx, dest: PathBuf) -> R<()> {
            let r = ctx.call(CoreCommand::ExportSupportBundle {
                dest: dest.display().to_string(),
            })?;
            if !ctx.print_json(&r) {
                if let CoreResponse::Lines { lines } = r {
                    println!("Wrote {} with: {}", dest.display(), lines.join(", "));
                    println!("Secrets are redacted, but read it before you share it.");
                }
            }
    Ok(())
}

pub fn shell_menu(ctx: &Ctx, m: ShellMenuCmd) -> R<()> {
            let r = ctx.call(match m {
                ShellMenuCmd::Status => CoreCommand::GetShellMenu,
                ShellMenuCmd::Install => CoreCommand::InstallShellMenu,
                ShellMenuCmd::Remove => CoreCommand::RemoveShellMenu,
            })?;
            if !ctx.print_json(&r) {
                if let CoreResponse::ShellMenu { status } = r {
                    println!(
                        "The Explorer menu is {}.",
                        if status.installed {
                            "installed"
                        } else {
                            "not installed"
                        }
                    );
                }
            }
    Ok(())
}

pub fn guide(ctx: &Ctx, version: Option<String>) -> R<()> {
            let res = ctx.call(CoreCommand::GetAppGuide { version })?;
            let CoreResponse::AppGuide { markdown, .. } = &res else {
                return Err("unexpected reply".into());
            };
            if !ctx.print_json(&res) {
                println!("{markdown}");
            }
    Ok(())
}

pub fn stop_everything(ctx: &Ctx) -> R<()> {
    if let CoreResponse::Tunnels { tunnels } = ctx.call(CoreCommand::ListTunnels)? {
        for t in tunnels.into_iter().filter(|t| t.state != "stopped") {
            ctx.call(CoreCommand::StopTunnel { id: t.config.id })?;
        }
    }
    if let CoreResponse::Projects { projects } = ctx.call(CoreCommand::ListProjects)? {
        for p in projects {
            ctx.call(CoreCommand::StopProjectWorkers { project_id: p.id })?;
        }
    }
    ctx.call(CoreCommand::StopWeb)?;
    if let CoreResponse::Services { services } = ctx.call(CoreCommand::ListServices)? {
        for s in services.into_iter().filter(|s| s.running) {
            ctx.call(CoreCommand::StopService { id: s.id.clone() })?;
            println!("Stopped {}.", s.name);
        }
    }
    println!("Everything is stopped.");
    Ok(())
}



pub fn setup(ctx: &Ctx, path: PathBuf, dry_run: bool, yes: bool) -> R<()> {
    let (id, _) = ctx.project_for_path(&path)?;
    let r = ctx.call(CoreCommand::PlanSetup {
        project_id: id.clone(),
    })?;
    let CoreResponse::SetupPlan { plan } = r else {
        return Err("unexpected reply".into());
    };
    if ctx.json && dry_run {
        println!(
            "{}",
            serde_json::to_string_pretty(&plan).unwrap_or_default()
        );
        return Ok(());
    }
    print!("{}", ols_core::setup::plan_text(&plan));
    if !plan.ok {
        return Err("the plan has blocking conflicts (✗ above); nothing was changed".into());
    }
    let todo = plan.steps.iter().filter(|s| !s.done).count();
    if dry_run {
        println!("\nDry run: nothing was changed. {todo} step(s) would run.");
        return Ok(());
    }
    if todo == 0 {
        println!("\nEverything is already in place.");
        return Ok(());
    }
    if !yes && !confirm(&format!("\nApply these {todo} step(s)?")) {
        println!("Nothing changed. Run with --yes to apply.");
        return Ok(());
    }

    // Follow the steps while the (blocking) setup runs.
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let watcher = {
        let done = done.clone();
        let paths = ctx.paths.clone();
        std::thread::spawn(move || {
            let mut shown = std::collections::HashSet::new();
            while !done.load(std::sync::atomic::Ordering::Relaxed) {
                if let Ok(Ok(CoreResponse::SetupProgress { report: Some(r) })) =
                    control::send(&paths, CoreCommand::GetSetupProgress)
                {
                    for s in r
                        .steps
                        .iter()
                        .filter(|s| matches!(s.status, ols_core::setup::StepStatus::Running))
                    {
                        if shown.insert(s.label.clone()) {
                            println!("  … {}", s.label);
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(600));
            }
        })
    };
    let result = ctx.call(CoreCommand::ApplySetup {
        project_id: id,
        dry_run: false,
    });
    done.store(true, std::sync::atomic::Ordering::Relaxed);
    let _ = watcher.join();
    let r = result?;
    if ctx.print_json(&r) {
        return Ok(());
    }
    let CoreResponse::Setup { report } = r else {
        return Ok(());
    };
    println!();
    for s in &report.steps {
        use ols_core::setup::StepStatus as S;
        let mark = match s.status {
            S::Done | S::Skipped => "✓",
            S::Failed => "✗",
            S::RolledBack => "↺",
            _ => "·",
        };
        println!(
            "{mark} {}{}",
            s.label,
            s.detail
                .as_ref()
                .map(|d| format!(" ({d})"))
                .unwrap_or_default()
        );
    }
    for r in &report.rolled_back {
        println!("undone: {r}");
    }
    match report.error {
        Some(e) => Err(e),
        None => {
            println!(
                "\nThe environment is set up.{}",
                report
                    .lock_written
                    .map(|l| format!(" Wrote {l}."))
                    .unwrap_or_default()
            );
            Ok(())
        }
    }
}


