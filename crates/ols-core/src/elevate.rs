//! Running `ols-helper` (§138). The helper is tried un-elevated first — if the hosts file
//! is already writable (or the change is a no-op) nothing prompts. Only an "access
//! denied" exit re-launches it through a single UAC prompt. Per-operation elevation, not a
//! resident service, keeps the privileged surface as small as it can be.

use std::path::PathBuf;
use std::time::Duration;

const ACCESS_DENIED: i32 = 5;

/// Where the helper binary lives: `OLS_HELPER_PATH`, else beside the running executable
/// (how the installer lays it out), else beside the cargo `target/<profile>` dir tests run from.
pub fn helper_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("OLS_HELPER_PATH") {
        return Some(p.into());
    }
    let exe = std::env::current_exe().ok()?;
    let name = if cfg!(windows) { "ols-helper.exe" } else { "ols-helper" };
    let mut dir = exe.parent()?.to_path_buf();
    // `cargo test` binaries live in target/<profile>/deps.
    for _ in 0..2 {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        dir = dir.parent()?.to_path_buf();
    }
    None
}

pub fn run_helper(args: &[String]) -> Result<(), String> {
    let helper = helper_path().ok_or("ols-helper was not found next to the application")?;

    let direct = crate::exec::run_capture(&helper, args, None, &[], Duration::from_secs(30));
    match direct.exit_code {
        Some(0) => return Ok(()),
        Some(ACCESS_DENIED) => {}
        _ => return Err(direct.combined()),
    }

    elevated(&helper, args)
}

#[cfg(windows)]
fn elevated(helper: &std::path::Path, args: &[String]) -> Result<(), String> {
    // Args are validated by the helper itself, but quote defensively anyway.
    let arg_list = args.iter().map(|a| format!("\"{}\"", a.replace('"', ""))).collect::<Vec<_>>().join(" ");
    let script = format!(
        "$p = Start-Process -FilePath '{}' -ArgumentList '{}' -Verb RunAs -Wait -PassThru -WindowStyle Hidden; exit $p.ExitCode",
        helper.display().to_string().replace('\'', "''"),
        arg_list.replace('\'', "''")
    );
    let out = crate::exec::run_capture(
        std::path::Path::new("powershell"),
        &["-NoProfile".into(), "-NonInteractive".into(), "-Command".into(), script],
        None,
        &[],
        // The user has to notice and click through UAC.
        Duration::from_secs(120),
    );
    match out.exit_code {
        Some(0) => Ok(()),
        _ if out.stderr.to_lowercase().contains("canceled") || out.stderr.to_lowercase().contains("cancelled") => {
            Err("the administrator prompt was cancelled".into())
        }
        Some(code) => Err(format!("the elevated helper failed (exit code {code}) {}", out.combined())),
        None => Err(format!("could not start the elevated helper: {}", out.combined())),
    }
}

#[cfg(not(windows))]
fn elevated(_helper: &std::path::Path, _args: &[String]) -> Result<(), String> {
    Err("elevation is only implemented on Windows so far".into())
}

/// Runs an arbitrary command with administrator rights (a Quick App step marked
/// `elevated`, after the user confirmed it separately, §92). Output isn't captured — UAC
/// launches it in a separate session — so only success or failure comes back.
#[cfg(windows)]
pub fn run_elevated_command(executable: &std::path::Path, args: &[String]) -> Result<(), String> {
    let arg_list = args.iter().map(|a| format!("\"{}\"", a.replace('"', "\\\""))).collect::<Vec<_>>().join(" ");
    let script = format!(
        "$p = Start-Process -FilePath '{}' -ArgumentList '{}' -Verb RunAs -Wait -PassThru; exit $p.ExitCode",
        executable.display().to_string().replace('\'', "''"),
        arg_list.replace('\'', "''")
    );
    let out = crate::exec::run_capture(
        std::path::Path::new("powershell"),
        &["-NoProfile".into(), "-NonInteractive".into(), "-Command".into(), script],
        None,
        &[],
        Duration::from_secs(1800),
    );
    match out.exit_code {
        Some(0) => Ok(()),
        Some(code) => Err(format!("the elevated command failed (exit code {code}) {}", out.combined())),
        None => Err(format!("the administrator prompt was cancelled or failed: {}", out.combined())),
    }
}

#[cfg(not(windows))]
pub fn run_elevated_command(_executable: &std::path::Path, _args: &[String]) -> Result<(), String> {
    Err("elevation is only implemented on Windows so far".into())
}
