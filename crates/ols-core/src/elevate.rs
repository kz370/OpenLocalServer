//! Running `ols-helper` (§138). Order of preference:
//! 1. The `OpenLocalServerHelper` service, over its local pipe: no prompt at all.
//! 2. The helper un-elevated: works when the change needs no rights (or the file is writable).
//! 3. On "access denied": one UAC prompt that installs the service, then step 1 again, so
//!    that prompt is the last one. If the service can't be installed, the single command
//!    runs elevated instead.

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
    if let Some(result) = via_service(args) {
        return result;
    }
    let helper = helper_path().ok_or("ols-helper was not found next to the application")?;

    let direct = crate::exec::run_capture(&helper, args, None, &[], Duration::from_secs(30));
    match direct.exit_code {
        Some(0) => return Ok(()),
        Some(ACCESS_DENIED) => {}
        _ => return Err(direct.combined()),
    }

    // One prompt to install the service; every later change goes through it silently.
    match elevated(&helper, &["install-service".to_string()]) {
        Ok(()) => {
            for _ in 0..25 {
                if let Some(result) = via_service(args) {
                    return result;
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            tracing::warn!("helper service installed but not answering; running this change elevated");
            elevated(&helper, args)
        }
        Err(e) if e.contains("cancelled") => Err(e),
        Err(e) => {
            tracing::warn!(error = %e, "could not install the helper service; running this change elevated");
            elevated(&helper, args)
        }
    }
}

pub const SERVICE_PIPE: &str = r"\\.\pipe\OpenLocalServerHelper";

/// Installs the helper service (one UAC prompt). Afterwards nothing prompts again.
pub fn install_service() -> Result<(), String> {
    let helper = helper_path().ok_or("ols-helper was not found next to the application")?;
    elevated(&helper, &["install-service".to_string()])
}

/// Removes the helper service (one UAC prompt); changes then prompt each time again.
pub fn uninstall_service() -> Result<(), String> {
    let helper = helper_path().ok_or("ols-helper was not found next to the application")?;
    elevated(&helper, &["uninstall-service".to_string()])
}

/// Whether the helper service is installed and answering.
pub fn service_available() -> bool {
    via_service(&["version".to_string()]).is_some_and(|r| r.is_ok())
}

/// Sends one command to the helper service. `None` when the service isn't there.
fn via_service(args: &[String]) -> Option<Result<(), String>> {
    use std::io::{Read, Write};
    let mut pipe = std::fs::OpenOptions::new().read(true).write(true).open(SERVICE_PIPE).ok()?;
    let mut request = serde_json::to_vec(args).ok()?;
    request.push(b'\n');
    pipe.write_all(&request).ok()?;
    // Read up to the newline: the service disconnects right after replying, which Windows
    // reports as an error rather than a clean end of stream.
    let mut reply = Vec::new();
    let mut buf = [0u8; 4096];
    while !reply.contains(&b'\n') {
        match pipe.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => reply.extend_from_slice(&buf[..n]),
        }
    }
    let v: serde_json::Value = serde_json::from_slice(reply.split(|b| *b == b'\n').next()?).ok()?;
    let code = v.get("code").and_then(|c| c.as_i64()).unwrap_or(1);
    let message = v.get("message").and_then(|m| m.as_str()).unwrap_or_default().to_string();
    Some(if code == 0 { Ok(()) } else { Err(message) })
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
