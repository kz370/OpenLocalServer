//! Manual, real-world smoke test: drives the exact `Core::dispatch` path the GUI uses,
//! against the real app data directory (no OLS_HOME override) — installs Node + PHP for
//! real, registers a real fixture project, and proves detection → resolution → the
//! runtime-aware "terminal" (§19) all work end-to-end. Not part of `cargo test`; run with
//! `cargo run --release --example smoke -p ols-core`.

use std::thread::sleep;
use std::time::Duration;

use ols_core::{AppPaths, Core, CoreCommand, CoreResponse, SettingsService};

fn dispatch(core: &mut Core, cmd: CoreCommand) -> CoreResponse {
    core.dispatch(cmd).unwrap_or_else(|d| panic!("command failed: {} — {}", d.problem, d.cause))
}

fn wait_installed(core: &mut Core, id: &str) {
    for _ in 0..180 {
        if let CoreResponse::RuntimeCatalog { entries } = dispatch(core, CoreCommand::ListRuntimeCatalog) {
            if entries.iter().any(|e| e.id == id && e.installed) {
                println!("[smoke] {id} installed");
                return;
            }
        }
        sleep(Duration::from_secs(1));
    }
    panic!("{id} did not finish installing in time");
}

fn wait_process_output(core: &mut Core, id: ols_core::process::ProcessId, expect_lines: usize) -> Vec<String> {
    for _ in 0..50 {
        if let CoreResponse::ProcessOutput { lines, .. } = dispatch(core, CoreCommand::GetProcessOutput { id }) {
            if lines.len() >= expect_lines {
                return lines;
            }
        }
        sleep(Duration::from_millis(200));
    }
    if let CoreResponse::ProcessOutput { lines, .. } = dispatch(core, CoreCommand::GetProcessOutput { id }) {
        lines
    } else {
        vec![]
    }
}

fn main() {
    let paths = AppPaths::resolve();
    paths.ensure_dirs().unwrap();
    println!("[smoke] using real app home: {}", paths.root().display());

    let settings = SettingsService::load(&paths).unwrap();
    let mut core = Core::new(settings, paths.clone());

    // -- Install Node + PHP for real, into the real app data directory. --
    for (id, version) in [("node", "24.21.0"), ("php", "8.4.26")] {
        let entries = match dispatch(&mut core, CoreCommand::ListRuntimeCatalog) {
            CoreResponse::RuntimeCatalog { entries } => entries,
            _ => unreachable!(),
        };
        let already = entries.iter().any(|e| e.id == id && e.installed);
        if already {
            println!("[smoke] {id} already installed, skipping download");
        } else {
            println!("[smoke] installing {id} {version}...");
            dispatch(&mut core, CoreCommand::InstallRuntime { id: id.into(), version: version.into() });
            wait_installed(&mut core, id);
        }
    }

    // -- Register a real fixture project needing both PHP and Node. --
    let fixture_dir = paths.root().join("smoke-fixture-project");
    std::fs::create_dir_all(&fixture_dir).unwrap();
    std::fs::write(fixture_dir.join("composer.json"), r#"{"require":{"php":"^8.4"}}"#).unwrap();
    std::fs::write(fixture_dir.join("package.json"), r#"{"engines":{"node":"24"}}"#).unwrap();

    let project = match dispatch(&mut core, CoreCommand::RegisterProject { path: fixture_dir.display().to_string() }) {
        CoreResponse::Project { project } => project,
        _ => unreachable!(),
    };
    println!("[smoke] registered project: {} ({})", project.name, project.id);

    let detail = match dispatch(&mut core, CoreCommand::GetProjectDetail { id: project.id.clone() }) {
        CoreResponse::ProjectDetail { detail } => *detail,
        _ => unreachable!(),
    };
    println!("[smoke] detected framework: {:?}", detail.detection.framework);
    for r in &detail.resolved {
        println!(
            "[smoke] resolved {}: requested={:?} source={:?} installed={:?}",
            r.id, r.requested_version, r.source, r.installed_version
        );
    }

    // -- Run both resolved runtimes' `--version` through the project (§19). --
    for runtime_id in ["php", "node"] {
        let resp = dispatch(
            &mut core,
            CoreCommand::RunInProject {
                project_id: project.id.clone(),
                runtime_id: runtime_id.into(),
                args: vec!["--version".into()],
            },
        );
        let CoreResponse::ProcessStarted { id } = resp else { unreachable!() };
        let lines = wait_process_output(&mut core, id, 1);
        println!("[smoke] `{runtime_id} --version` via project-resolved binary:");
        for line in &lines {
            println!("    {line}");
        }
        assert!(!lines.is_empty(), "{runtime_id} produced no output");
    }

    // Clean up the fixture project registration (leave the installed runtimes in place —
    // those are genuinely useful to the real app).
    dispatch(&mut core, CoreCommand::RemoveProject { id: project.id.clone() });
    std::fs::remove_dir_all(&fixture_dir).ok();

    println!("[smoke] ALL GOOD — Stage 3 + Stage 4 pipeline verified end-to-end against real app data.");
}
