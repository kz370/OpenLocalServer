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

use std::io::Write;
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
/// The child's output is piped and written out again here, rather than handed a
/// console and trusted to find it. That is not a stylistic choice; it is the only
/// arrangement that works from a release build, and the measurements are what forced
/// it. A release build is a GUI-subsystem program, so it has no console of its own.
/// Every version of the "just give it the console" approach — plain inheritance,
/// `AttachConsole` + `SetStdHandle`, then explicit `Stdio::from(OwnedHandle)` of the
/// console handles — produced a child that ran, printed nothing and exited with a
/// correct code: `ols /c echo HELLO` printed nothing, `ols /c exit 3` returned 3, and
/// `ols --version > out.txt` left the file empty. A debug build of the same binary,
/// which is a console-subsystem program because the subsystem attribute is only set
/// for release, printed it correctly — so the code was never the problem, the
/// subsystem was.
///
/// Piping also means redirection works, which inheritance cannot do here: `ols status >
/// out.txt` writes the file because the writing is done by this process, on handles it
/// has already been given.
///
/// Streaming, not a single read: the CLI follows running output (`ols service logs`),
/// and a command that prints for ten minutes must not look hung.
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
    // A prompt is answered on this process's stdin, and a command that needs one
    // (`ols setup` with no --yes) reads from the terminal it was typed into. The
    // child is given this process's standard input rather than null, or every
    // confirmation would be unanswerable and the command would abort.
    let mut child = std::process::Command::new(&cli);
    child
        .args(args)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = child.spawn().map_err(|e| Diagnostic {
        problem: format!("{} could not be started.", cli.display()),
        cause: e.to_string(),
        fix: Some(
            "Check that the file is not blocked or in use, then run the command again.".into(),
        ),
    })?;
    // Two threads, one per pipe, both writing as lines arrive. A single reader on
    // stdout would deadlock the moment the CLI filled the stderr buffer while waiting
    // to be read, which is the normal state of a command that warns about something.
    let out = child.stdout.take().map(|r| {
        let mut r = std::io::BufReader::new(r);
        std::thread::spawn(move || {
            let mut line = String::new();
            while read_line_lossy(&mut r, &mut line) {
                print!("{line}");
                let _ = std::io::stdout().flush();
            }
        })
    });
    let err = child.stderr.take().map(|r| {
        let mut r = std::io::BufReader::new(r);
        std::thread::spawn(move || {
            let mut line = String::new();
            while read_line_lossy(&mut r, &mut line) {
                eprint!("{line}");
                let _ = std::io::stderr().flush();
            }
        })
    });
    let status = child.wait().map_err(|e| Diagnostic {
        problem: format!("{} stopped answering.", cli.display()),
        cause: e.to_string(),
        fix: Some("Run the command again.".into()),
    })?;
    // The joins come after the wait on purpose. A thread still draining a pipe when
    // this returns would write into a process that is about to exit, and the tail of
    // the output — the error that explains the non-zero code, typically — is exactly
    // the part that would be lost.
    if let Some(t) = out {
        let _ = t.join();
    }
    if let Some(t) = err {
        let _ = t.join();
    }
    Ok(status.code().unwrap_or(1))
}

/// Reads one line, appending it (newline included) to `into`. `BufRead::read_line`
/// needs valid UTF-8 and fails outright on anything else, and this stream is whatever
/// a service chose to print; a non-UTF-8 byte must not end the output half-way
/// through with nothing said about it. Returns false at end of input.
fn read_line_lossy(r: &mut impl std::io::BufRead, into: &mut String) -> bool {
    let mut raw: Vec<u8> = Vec::new();
    match r.read_until(b'\n', &mut raw) {
        Ok(0) | Err(_) => false,
        Ok(_) => {
            into.push_str(&String::from_utf8_lossy(&raw));
            true
        }
    }
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

    /// The output is what the whole fix is for, so the reader that carries it is
    /// pinned on the two things that lose it: a byte sequence that is not UTF-8 (a
    /// service is free to print one) and a last line with no newline on it, which is
    /// what a command killed mid-print leaves behind.
    #[test]
    fn the_output_reader_keeps_bad_bytes_and_an_unterminated_last_line() {
        let raw: &[u8] = b"first\n\xff\xfe not utf-8\nlast without newline";
        let mut r = std::io::BufReader::new(raw);
        let mut got = String::new();
        let mut line = String::new();
        let mut lines = Vec::new();
        while read_line_lossy(&mut r, &mut line) {
            lines.push(std::mem::take(&mut line));
        }
        for l in &lines {
            got.push_str(l);
        }
        assert_eq!(lines.len(), 3, "every line survives");
        assert_eq!(lines[0], "first\n");
        assert!(
            lines[1].contains('\u{fffd}'),
            "a bad byte is replaced, not dropped"
        );
        // No trailing newline and still returned — read_line would have done the same,
        // but only because the input happened to be valid UTF-8 up to that point.
        assert_eq!(lines[2], "last without newline");
        assert!(got.contains("first"));
    }
}
