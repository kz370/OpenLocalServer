//! Reaching the command line through the name the app already owns.
//!
//! `ols <command>` cannot be a separate program called `ols`, because the app ships
//! as `OLS.exe` and Windows does not tell two file names apart by case: a lookup for
//! `ols.exe` next to the app finds the app. So the command line is `ols-cli.exe`,
//! and `ols` itself resolves to `OLS.exe` — the app — because `.EXE` comes before
//! `.CMD` in `PATHEXT` and a shim in the same folder therefore never wins. That was
//! measured, not assumed: with `ols.cmd` and `OLS.exe` side by side, `ols --version`
//! printed nothing and exited 0, and with the shim moved into an earlier PATH
//! directory it answered `ols 1.1.0`.
//!
//! So the app answers for the command line instead. Before it starts anything, a
//! launch whose first argument is not one of its own is handed to `ols-cli.exe`
//! verbatim, and the app exits with the CLI's own exit code. One name, one command,
//! and nothing to uninstall from PATH.

use std::path::{Path, PathBuf};

use crate::app::FORCE_MINIMIZED_ARG;
use crate::Diagnostic;

/// The command line's file name. Not `ols.exe`: the app ships as `OLS.exe`, Windows
/// does not tell two names apart by case, and a lookup by that name finds the app.
pub const CLI_EXE: &str = if cfg!(windows) {
    "ols-cli.exe"
} else {
    "ols-cli"
};

/// The arguments that mean "start the app", out of everything after the program name.
///
/// The app takes `--minimized` and nothing else, so the whole argument list is
/// compared rather than its first word: `ols --minimized status` is a person who got
/// the order wrong, and answering it is better than ignoring the half that was meant.
///
/// Not a list of the CLI's subcommands, which is the other obvious shape and the
/// wrong one: it cannot be kept in step with the CLI without a second copy of every
/// command name, and the failure is silent and looks exactly like the bug above. A
/// misspelled subcommand would open the app. Here it goes to the CLI, which prints
/// `unrecognized subcommand` and exits non-zero — the same answer a person gets from
/// every other command line, and the truth when the name is not one of ours.
pub fn is_app_launch(args: &[String]) -> bool {
    args.is_empty() || (args.len() == 1 && args[0] == FORCE_MINIMIZED_ARG)
}

/// Where the CLI is: beside the app, or on PATH.
///
/// The running executable is never the answer, whatever it is called. The app and
/// the CLI are different programs that ship into the same folder, and Windows file
/// names differ only in case, so a lookup that accepted "the file next to me named
/// `ols.exe`" found the *app* and wrote it into the Explorer menu: "Add to OLS" then
/// launched OLS instead of registering the folder, with no error anywhere. The
/// comparison is on canonical paths so a differently-spelled path to the same file
/// is still caught — and it is the other half of this module's own safety, since
/// forwarding a `ols status` into the app would start the app instead of answering.
pub fn find_cli() -> Option<PathBuf> {
    let name = CLI_EXE;
    let me = std::env::current_exe()
        .ok()
        .and_then(|p| std::fs::canonicalize(p).ok());
    let is_me = |p: &Path| match std::fs::canonicalize(p) {
        Ok(c) => me.as_deref() != Some(c.as_path()),
        // Cannot resolve it, so it cannot be shown to be the running exe either.
        Err(_) => true,
    };
    if let Ok(exe) = std::env::current_exe() {
        let beside = exe.with_file_name(name);
        if beside.is_file() && is_me(&beside) {
            return Some(beside);
        }
    }
    std::env::var_os("PATH").and_then(|p| {
        std::env::split_paths(&p)
            .map(|d| d.join(name))
            .find(|f| f.is_file() && is_me(f))
    })
}

/// Hands the arguments to the command line and answers with its exit code.
///
/// The child's three standard handles are handed to it explicitly rather than left to
/// inheritance, which is what makes the answer visible: a release build is a
/// GUI-subsystem program, so it has no console of its own, and a console program
/// started from one with nothing but inheritance writes into a window that flashes up
/// and closes. That was measured — the child ran, exited 0 and printed nothing, and
/// `ols --version > out.txt` left the file empty. See [`console_handles`].
pub fn forward_to_cli(args: &[String]) -> Result<i32, Diagnostic> {
    let cli = find_cli().ok_or_else(|| Diagnostic {
        problem: format!(
            "`ols {command}` needs the command-line program, and it is not there.",
            command = args.first().map(String::as_str).unwrap_or("")
        ),
        cause: format!("Neither next to this one nor on PATH is there a file called {CLI_EXE}."),
        fix: Some(
            "Reinstall OLS, or run the command with its full path (for example C:\\OpenLocalServer\\ols-cli.exe status)."
                .into(),
        ),
    })?;
    let handles = console_handles();
    let mut child = std::process::Command::new(&cli);
    child.args(args);
    if let Some(h) = &handles {
        child
            .stdin(ConsoleHandles::stdio(&h.input))
            .stdout(ConsoleHandles::stdio(&h.output))
            .stderr(ConsoleHandles::stdio(&h.error));
    }
    child
        .status()
        .map(|s| s.code().unwrap_or(1))
        .map_err(|e| Diagnostic {
            problem: format!("{} could not be started.", cli.display()),
            cause: e.to_string(),
            fix: Some(
                "Check that the file is not blocked or in use, then run the command again.".into(),
            ),
        })
}

/// The caller's three console streams, once this process has attached to them.
///
/// Owned handles rather than borrowed ones: they are handed to a child process, and
/// `Stdio` takes ownership, so the process that spawned it has to let go of them.
#[cfg(windows)]
pub struct ConsoleHandles {
    input: std::os::windows::io::OwnedHandle,
    output: std::os::windows::io::OwnedHandle,
    error: std::os::windows::io::OwnedHandle,
}

#[cfg(windows)]
impl ConsoleHandles {
    /// A stream for the child, cloned from the handle kept here.
    ///
    /// The clone is `DuplicateHandle` underneath, and it is what the child is given —
    /// a `Stdio` built from the handle itself would take it away from the field it
    /// came from, and the second call would have nothing left to hand over.
    fn stdio(handle: &std::os::windows::io::OwnedHandle) -> std::process::Stdio {
        std::process::Stdio::from(
            handle
                .try_clone()
                .expect("a console handle can always be duplicated"),
        )
    }
}

/// The caller's console, attached to this process, as handles to pass on.
///
/// `AttachConsole` is what joins a GUI-subsystem process to the console of whoever
/// started it, and `SetStdHandle` points this process's own standard handles at the
/// newly available console — which is what lets the caller print a `Diagnostic` here
/// if the command line cannot be found at all. The returned handles are the third
/// part: they are what the child is given, which is the only reliable way to get its
/// output to the terminal, and the only way a redirect like `ols status > out.txt`
/// works at all, since the handle it needs is the shell's, not the console's.
///
/// `None` unless all three are there. A console that answers for one stream and not
/// the others is not a case worth guessing at, and a half-set-up hand-over is how a
/// command ends up printing nothing — the child simply inherits instead. `AttachConsole`
/// also fails with `ERROR_ACCESS_DENIED` when the process already has a console, and
/// that is not a failure here: the handles are still the right ones.
#[cfg(windows)]
fn console_handles() -> Option<ConsoleHandles> {
    use std::os::windows::io::{FromRawHandle, OwnedHandle};

    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Console::{
        AttachConsole, GetStdHandle, SetStdHandle, ATTACH_PARENT_PROCESS, STD_ERROR_HANDLE,
        STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    };
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
        // A handle of 0 means "no console", and INVALID_HANDLE_VALUE means the
        // request failed; SetStdHandle with either would replace a working handle
        // with nothing.
        let take = |which| {
            let handle = GetStdHandle(which);
            if handle.is_null() || std::ptr::eq(handle, INVALID_HANDLE_VALUE) {
                return None;
            }
            SetStdHandle(which, handle);
            Some(OwnedHandle::from_raw_handle(handle.cast()))
        };
        match (
            take(STD_INPUT_HANDLE),
            take(STD_OUTPUT_HANDLE),
            take(STD_ERROR_HANDLE),
        ) {
            (Some(input), Some(output), Some(error)) => Some(ConsoleHandles {
                input,
                output,
                error,
            }),
            _ => None,
        }
    }
}

#[cfg(not(windows))]
fn console_handles() -> Option<()> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rule that decides whether a launch is the app or the command line. The
    /// second case is the bug: anything the app does not own belongs to the CLI, so a
    /// name it does not recognise is the CLI's to reject rather than the app's to
    /// silently swallow.
    #[test]
    fn only_a_bare_launch_or_the_minimized_flag_is_the_app() {
        assert!(is_app_launch(&[]));
        assert!(is_app_launch(&[FORCE_MINIMIZED_ARG.to_string()]));
        assert!(!is_app_launch(&[
            FORCE_MINIMIZED_ARG.to_string(),
            "status".to_string()
        ]));
        assert!(!is_app_launch(&["status".to_string()]));
        assert!(!is_app_launch(&["project".to_string(), "add".to_string()]));
        assert!(!is_app_launch(&["--help".to_string()]));
        assert!(!is_app_launch(&["--version".to_string()]));
        assert!(!is_app_launch(&["json".to_string()]));
        // A misspelled command has to reach the CLI to be reported, not open a window.
        assert!(!is_app_launch(&["statuss".to_string()]));
    }

    /// The app and the CLI ship into the same folder and differ only in case, so a
    /// name that Windows cannot separate would make a lookup find the app.
    #[test]
    fn the_cli_name_cannot_be_the_app_image() {
        let cli = CLI_EXE.to_ascii_lowercase();
        assert_ne!(cli, "ols.exe", "the CLI cannot be named like the app");
        assert_ne!(cli, "openlocalserver.exe");
    }

    /// Forwarding a command into the app would start the app instead of answering, so
    /// the lookup this module depends on must still refuse the running executable.
    #[test]
    fn find_cli_never_returns_the_running_executable() {
        let me = std::env::current_exe().expect("test binary path");
        if let Some(found) = find_cli() {
            let a = std::fs::canonicalize(&found).unwrap_or_else(|_| found.clone());
            let b = std::fs::canonicalize(&me).unwrap_or_else(|_| me.clone());
            assert_ne!(a, b, "find_cli returned the running executable");
        }
    }
}
