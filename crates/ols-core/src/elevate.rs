//! Running `ols-helper` (§138). Order of preference:
//! 1. The `OpenLocalServerHelper` service, over its local pipe: no prompt at all.
//! 2. The helper un-elevated: works when the change needs no rights (or the file is writable).
//! 3. On "access denied": one UAC prompt that installs the service, then step 1 again, so
//!    that prompt is the last one. If the service can't be installed, the single command
//!    runs elevated instead.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const ACCESS_DENIED: i32 = 5;

/// Where the helper binary lives: `OLS_HELPER_PATH`, then a freshly built copy in the
/// workspace `target/release` or `target/debug` (so a dev build that rebuilt the helper
/// is found before any stale copy beside an older app), then beside the running executable
/// (how the installer lays it out), then beside the cargo `target/<profile>` dir tests run from.
pub fn helper_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("OLS_HELPER_PATH") {
        return Some(p.into());
    }
    let name = if cfg!(windows) {
        "ols-helper.exe"
    } else {
        "ols-helper"
    };
    // Workspace target directories first: a helper rebuilt alongside the app (e.g. after
    // `cargo build -p ols-helper`) should win over a stale copy shipped with an older
    // installed release beside the exe. `CARGO_MANIFEST_DIR` is compile-time, so on a
    // user's machine these paths don't exist and the lookup falls through to beside-exe.
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if let Some(workspace_root) = manifest_dir.parent().and_then(|p| p.parent()) {
        for profile in ["release", "debug"] {
            let candidate = workspace_root.join("target").join(profile).join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    // Beside the running executable: installed builds ship ols-helper.exe next to OLS.exe.
    let exe = std::env::current_exe().ok()?;
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

/// What is missing, why, and what to do about it. A bare "not found" is the one message a
/// user cannot act on: an installed build ships the helper beside the app, and only a
/// development build has to build it first.
fn helper_missing() -> String {
    let name = if cfg!(windows) {
        "ols-helper.exe"
    } else {
        "ols-helper"
    };
    format!(
        "The administrator helper ({name}) was not found next to the application, so the \
         changes that need administrator rights cannot run. An installed build ships it \
         beside the app; in a development build run `cargo build -p ols-helper` in the \
         project folder and try again."
    )
}

pub fn run_helper(args: &[String]) -> Result<(), String> {
    if let Some(result) = via_service(args) {
        return result;
    }
    let helper = helper_path().ok_or_else(helper_missing)?;

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
            tracing::warn!(
                "helper service installed but not answering; running this change elevated"
            );
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
///
/// Before copying anything, the candidate helper's own `--version` is checked against
/// the app's `CARGO_PKG_VERSION`. A stale copy beside an older install (for example a
/// `target\debug\ols-helper.exe` left over from 2.2.0 while the app is 2.3.0) would
/// otherwise be re-installed as the service and nothing would change. A mismatch is
/// reported with an actionable message instead of silently installing the wrong version.
pub fn install_service() -> Result<(), String> {
    let helper = helper_path().ok_or_else(helper_missing)?;
    let expected = env!("CARGO_PKG_VERSION");
    if let Some(actual) = helper_version(&helper) {
        if actual != expected {
            return Err(format!(
                "the helper at {} is version {actual}, but this app is {expected}; \
                 rebuild ols-helper first (`cargo build -p ols-helper`)",
                helper.display()
            ));
        }
    }
    elevated(&helper, &["install-service".to_string()])
}

/// Reads an `ols-helper` binary's version by running it with `--version` (un-elevated,
/// so no UAC prompt). `None` when the binary can't be executed or doesn't answer.
fn helper_version(helper: &Path) -> Option<String> {
    let out = crate::exec::run_capture(
        helper,
        &["version".to_string()],
        None,
        &[],
        Duration::from_secs(10),
    );
    (out.exit_code == Some(0) && !out.combined().is_empty()).then_some(out.combined())
}

/// Removes the helper service (one UAC prompt); changes then prompt each time again.
pub fn uninstall_service() -> Result<(), String> {
    let helper = helper_path().ok_or_else(helper_missing)?;
    elevated(&helper, &["uninstall-service".to_string()])
}

/// Whether the helper service is installed and answering.
pub fn service_available() -> bool {
    via_service(&["version".to_string()]).is_some_and(|r| r.is_ok())
}

/// The version the resident helper service is running.
///
/// Installing the service *copies* the helper into an admin-only folder, and nothing
/// re-copies it afterwards. So an app update — or an install moved to a different folder —
/// leaves the service running whatever version was current when it was first installed, and
/// the app goes on talking to an elevated half it no longer matches. The service answers
/// `version` over the pipe for exactly this question.
///
/// Retries a few times because the service needs a moment after `install-service` starts it
/// before the named pipe answers.
pub fn service_version() -> Option<String> {
    for _ in 0..25 {
        if let Some(reply) = service_reply(&["version".to_string()]) {
            if reply.code != 0 {
                return None;
            }
            if !reply.message.is_empty() {
                return Some(reply.message);
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    None
}

/// Whether the installed service is out of date with the app it is serving.
///
/// `None` when the service is not answering at all: that is a missing service, which the
/// UI already offers to install, and reporting it as "out of date" would send the user
/// looking for a version difference that is not the problem.
pub fn service_is_stale() -> Option<bool> {
    service_version().map(|v| v != env!("CARGO_PKG_VERSION"))
}

/// One request to the service pipe and the reply it parsed, with the same deadline
/// `via_service` uses. Split out because the version query needs the message, which
/// `via_service` deliberately drops.
fn service_reply(args: &[String]) -> Option<ServiceReply> {
    use std::io::{Read, Write};
    const REPLY_TIMEOUT: Duration = Duration::from_secs(10);
    let mut pipe = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(SERVICE_PIPE)
        .ok()?;
    let mut request = serde_json::to_vec(args).ok()?;
    request.push(b'\n');
    pipe.write_all(&request).ok()?;
    // Read up to the newline: the service disconnects right after replying, which Windows
    // reports as an error rather than a clean end of stream.
    let deadline = Instant::now() + REPLY_TIMEOUT;
    let mut reply = Vec::new();
    let mut buf = [0u8; 4096];
    while !reply.contains(&b'\n') {
        if Instant::now() >= deadline {
            tracing::warn!(
                args = ?args,
                "the helper service did not reply in time; continuing without it"
            );
            return None;
        }
        match pipe.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => reply.extend_from_slice(&buf[..n]),
        }
    }
    let v: serde_json::Value = serde_json::from_slice(reply.split(|b| *b == b'\n').next()?).ok()?;
    Some(ServiceReply {
        code: v.get("code").and_then(|c| c.as_i64()).unwrap_or(1),
        message: v
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or_default()
            .to_string(),
    })
}

/// What the service answered: `0` is success, and `message` carries either the version
/// (for `version`) or the reason (for a real command).
struct ServiceReply {
    code: i64,
    message: String,
}

/// Sends one command to the helper service. `None` when the service isn't there.
///
/// The reply is read with a deadline: a helper that accepted the connection and then
/// wedged (or was killed mid-request) used to block here forever. That froze the caller —
/// `apply_web` holds the web state lock across this — so every later command queued behind
/// it and the whole app went blank. A helper that cannot answer in time is treated as
/// unavailable, and the caller falls back to running the change itself.
fn via_service(args: &[String]) -> Option<Result<(), String>> {
    let reply = service_reply(args)?;
    Some(if reply.code == 0 {
        Ok(())
    } else {
        Err(reply.message)
    })
}

#[cfg(windows)]
fn run_elevated_win32(
    executable: &std::path::Path,
    args: &[String],
    show_window: bool,
    timeout: Duration,
) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_CANCELLED, WAIT_OBJECT_0, WAIT_TIMEOUT,
    };
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, TerminateProcess, WaitForSingleObject,
    };
    use windows_sys::Win32::UI::Shell::{
        ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{SW_HIDE, SW_SHOWNORMAL};

    let exe_wide: Vec<u16> = executable
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let verb_wide: Vec<u16> = "runas\0".encode_utf16().collect();

    let formatted_args = crate::exec::format_windows_args(args);
    let args_wide: Vec<u16> = formatted_args
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    let mut exec_info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
        hwnd: std::ptr::null_mut(),
        lpVerb: verb_wide.as_ptr(),
        lpFile: exe_wide.as_ptr(),
        lpParameters: if formatted_args.is_empty() {
            std::ptr::null()
        } else {
            args_wide.as_ptr()
        },
        lpDirectory: std::ptr::null(),
        nShow: if show_window { SW_SHOWNORMAL } else { SW_HIDE },
        hInstApp: std::ptr::null_mut(),
        lpIDList: std::ptr::null_mut(),
        lpClass: std::ptr::null(),
        hkeyClass: std::ptr::null_mut(),
        dwHotKey: 0,
        Anonymous: unsafe { std::mem::zeroed() },
        hProcess: std::ptr::null_mut(),
    };

    let success = unsafe { ShellExecuteExW(&mut exec_info) };
    if success == 0 {
        let err = unsafe { GetLastError() };
        if err == ERROR_CANCELLED {
            return Err("the administrator prompt was cancelled".into());
        }
        let io_err = std::io::Error::from_raw_os_error(err as i32);
        return Err(format!(
            "could not start {} elevated: {io_err}",
            executable.display()
        ));
    }

    let h_process = exec_info.hProcess;
    if h_process.is_null() {
        return Ok(());
    }

    let timeout_ms = timeout.as_millis().min(u32::MAX as u128) as u32;
    let wait_res = unsafe { WaitForSingleObject(h_process, timeout_ms) };

    let result = match wait_res {
        WAIT_OBJECT_0 => {
            let mut exit_code: u32 = 0;
            if unsafe { GetExitCodeProcess(h_process, &mut exit_code) } != 0 {
                if exit_code == 0 {
                    Ok(())
                } else {
                    Err(format!(
                        "the elevated process failed (exit code {exit_code})"
                    ))
                }
            } else {
                Err(format!(
                    "could not read exit code from elevated process: {}",
                    std::io::Error::last_os_error()
                ))
            }
        }
        WAIT_TIMEOUT => {
            let _ = unsafe { TerminateProcess(h_process, 1) };
            Err("the elevated process timed out".into())
        }
        _ => Err(format!(
            "failed waiting for elevated process: {}",
            std::io::Error::last_os_error()
        )),
    };

    unsafe { CloseHandle(h_process) };
    result
}

#[cfg(windows)]
fn elevated(helper: &std::path::Path, args: &[String]) -> Result<(), String> {
    run_elevated_win32(helper, args, false, Duration::from_secs(120)).map_err(|e| {
        if e.contains("cancelled") {
            e
        } else if e.contains("failed (exit code") {
            format!(
                "the elevated helper {}",
                e.trim_start_matches("the elevated process ")
            )
        } else {
            format!("could not start the elevated helper: {e}")
        }
    })
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
    run_elevated_win32(executable, args, true, Duration::from_secs(1800)).map_err(|e| {
        if e.contains("cancelled") {
            "the administrator prompt was cancelled or failed".into()
        } else {
            format!("the elevated command {e}")
        }
    })
}

#[cfg(not(windows))]
pub fn run_elevated_command(_executable: &std::path::Path, _args: &[String]) -> Result<(), String> {
    Err("elevation is only implemented on Windows so far".into())
}
