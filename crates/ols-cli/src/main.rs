//! `olsc`: the OLS command line (§136).
//!
//! Every command is a `CoreCommand` sent over the local control channel to whoever owns
//! the core: the desktop app when it is open, otherwise a background `olsc daemon` that is
//! started on demand. The CLI never manages anything itself (architecture decision 1).

mod args;
mod commands;
mod ctx;
#[cfg(test)]
mod tests;

use std::process::ExitCode;
use clap::Parser;
use ols_core::control;
use ols_core::AppPaths;

use args::{Cli, Cmd};
use ctx::Ctx;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let paths = AppPaths::resolve();
    if let Err(e) = paths.ensure_dirs() {
        eprintln!("error: {e}");
        return ExitCode::FAILURE;
    }
    if let Cmd::Daemon { stop } = &cli.cmd {
        return daemon(&paths, *stop);
    }
    let ctx = Ctx {
        paths,
        json: cli.json,
    };
    match commands::run(&ctx, cli.cmd) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn daemon(paths: &AppPaths, stop: bool) -> ExitCode {
    if stop {
        match control::is_running(paths) {
            Some(i) if i.kind == "daemon" => {
                control::take_over_from_daemon(paths);
                println!("The background service stopped.");
            }
            Some(_) => {
                println!("The desktop app is running; there is no background service to stop.")
            }
            None => println!("Nothing is running."),
        }
        return ExitCode::SUCCESS;
    }
    if let Some(i) = control::is_running(paths) {
        eprintln!("OLS is already running ({}, process {}).", i.kind, i.pid);
        return ExitCode::FAILURE;
    }
    ols_core::logging::init(&paths.logs_dir());
    let settings = match ols_core::SettingsService::load(paths) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };
    let core = ols_core::Core::new(settings, paths.clone());
    let mut server = match control::serve(core.clone(), paths, "daemon") {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };
    tracing::info!("olsc daemon running");
    ols_core::scheduler::start_clock(core.inner());
    server.wait_for_shutdown();
    tracing::info!("olsc daemon handing over / stopping");
    let i = core.inner();
    i.stop_all_tunnels();
    i.stop_all_workers();
    i.terminals.close_all();
    i.web.stop();
    for s in core.services().list() {
        if s.running {
            if let Err(e) = core.services().stop(&s.id) {
                tracing::warn!(service = %s.id, %e, "service was still running when the daemon stopped");
            }
        }
    }
    control::close(paths);
    ExitCode::SUCCESS
}
