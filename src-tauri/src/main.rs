// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::ExitCode;

/// `ols <command>` reaches the command line through this program, because the app
/// owns that name: it ships as `OLS.exe`, Windows does not tell two file names apart
/// by case, and the command line cannot also be called `ols`. A release build is a
/// GUI program, so it has no console of its own to print an answer into — which is
/// why the hand-over happens before anything else starts, and why
/// `forward_to_cli` attaches the caller's console first. Details and the measured
/// failure this replaces: `ols_core::cli_dispatch`.
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !ols_core::cli_dispatch::is_app_launch(&args) {
        return match ols_core::cli_dispatch::forward_to_cli(&args) {
            Ok(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
            Err(e) => {
                eprintln!("error: {}", e.problem);
                if !e.cause.is_empty() {
                    eprintln!("  {}", e.cause);
                }
                if let Some(fix) = &e.fix {
                    eprintln!("  → {fix}");
                }
                ExitCode::FAILURE
            }
        };
    }
    app_lib::run();
    ExitCode::SUCCESS
}
