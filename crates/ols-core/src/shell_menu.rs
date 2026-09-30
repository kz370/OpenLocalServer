//! Explorer context menu (§124, Stage 17). Adds "Add to OLS" and "Set up with OLS"
//! to the right-click menu of a folder (and of its background). Per user (`HKCU\Software\Classes`), so no
//! administrator prompt; it only ever runs the `ols` command line, which does the work through the core.
//! Removing it deletes exactly the keys added here.

use serde::{Deserialize, Serialize};

use std::path::Path;

use crate::app::Inner;
use crate::error::CoreError;

const ROOTS: [&str; 2] = [
    r"HKCU\Software\Classes\Directory\shell",
    r"HKCU\Software\Classes\Directory\Background\shell",
];
const ADD: &str = "OpenLocalServer.Add";
const SETUP: &str = "OpenLocalServer.Setup";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellMenuStatus {
    pub installed: bool,
    /// The `ols` program the entries run, when it can be found.
    pub cli_path: Option<String>,
    pub supported: bool,
}

/// The CLI's file name. Not `ols.exe`: the app ships as `OLS.exe`, Windows does not
/// tell two names apart by case, and a lookup by that name finds the app.
const CLI: &str = if cfg!(windows) {
    "ols-cli.exe"
} else {
    "ols-cli"
};

/// Where the CLI is: beside the app, or on PATH.
///
/// The running executable is never the answer, whatever it is called. The app and
/// the CLI are different programs that ship into the same folder, and Windows file
/// names differ only in case, so a lookup that accepted "the file next to me named
/// `ols.exe`" found the *app* and wrote it into the menu: "Add to OLS" then launched
/// OLS instead of registering the folder, with no error anywhere. The comparison is
/// on canonical paths so a differently-spelled path to the same file is still caught.
fn find_cli() -> Option<std::path::PathBuf> {
    let name = CLI;
    let me = std::env::current_exe()
        .ok()
        .and_then(|p| std::fs::canonicalize(p).ok());
    let is_me = |p: &std::path::Path| match std::fs::canonicalize(p) {
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

/// The icon the menu entries show.
///
/// The app's own `icon.ico`, which the installer puts in the same folder as the CLI.
/// The entries run the CLI, and the CLI carries no icon resource of its own -- a Rust
/// binary is a console program as far as Explorer is concerned -- so pointing `Icon`
/// at it drew the generic console picture next to both entries, which is how the menu
/// ended up with a broken-looking icon next to a name that is not a program anyone
/// recognises. The .ico is what the app itself is drawn from, and it is beside the CLI
/// because that is the only folder both are guaranteed to share.
fn menu_icon(cli: &Path) -> String {
    let dir = cli.parent().unwrap_or(cli);
    let ico = dir.join("icon.ico");
    if ico.is_file() {
        ico.display().to_string()
    } else {
        // No .ico beside the CLI: the app's own image is the next best thing, and on
        // a dev build (where the CLI is not installed next to anything) it is the only
        // one there is.
        cli.display().to_string()
    }
}

/// The command each entry runs; `%V` is the folder Explorer passes.
fn commands(cli: &str) -> [(&'static str, &'static str, String); 2] {
    [
        (ADD, "Add to OLS", format!("\"{cli}\" project add \"%V\"")),
        (
            SETUP,
            "Set up with OLS",
            format!("cmd.exe /k \"\"{cli}\" setup --path \"%V\"\""),
        ),
    ]
}

/// The program out of a stored command line, i.e. `C:\OLS\ols-cli.exe` out of
/// `"C:\OLS\ols-cli.exe" project add "%V"`. An unquoted program is the run up to
/// the first space, which is what the shell itself does.
#[cfg(any(windows, test))]
fn command_program(command: &str) -> Option<&str> {
    let t = command.trim();
    match t.strip_prefix('"') {
        Some(rest) => rest.split('"').next(),
        None => t.split(' ').next().filter(|p| !p.is_empty()),
    }
}

/// Whether a stored command line still runs the CLI.
///
/// Keys alone are not enough. An entry written before the rename, or by any build
/// whose lookup found the app, launches OLS instead of the CLI, and the keys read as
/// installed — so the card shows "installed" and offers only Remove, and the menu
/// stays broken with nothing to click. The name is what distinguishes them, and it
/// is compared without case because Windows does.
#[cfg(any(windows, test))]
fn command_runs_the_cli(command: &str) -> bool {
    let Some(program) = command_program(command) else {
        return false;
    };
    std::path::Path::new(program)
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.eq_ignore_ascii_case(CLI))
}

/// The `(Default)` value out of `reg query ... /ve` output.
///
/// Only the first line is read. reg.exe wraps a long `REG_SZ` across lines, but what
/// is compared is the program at the front of the value and the wrap lands on a word
/// boundary, so the leading `"C:\...\ols-cli.exe"` is always in the first line — and
/// guessing at the rest of a wrapped line is a worse way to answer this question than
/// not answering it.
#[cfg(any(windows, test))]
fn reg_value(out: &str) -> Option<String> {
    out.lines()
        .find(|l| l.contains("(Default)") && l.contains("REG_SZ"))
        .and_then(|l| l.split_once("REG_SZ"))
        .map(|(_, v)| v.trim().to_string())
}

#[cfg(windows)]
fn reg(args: &[&str]) -> Result<String, String> {
    let mut cmd = std::process::Command::new("reg");
    cmd.args(args);
    crate::exec::hide_window(&mut cmd);
    let out = cmd.output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

#[cfg(windows)]
impl Inner {
    pub fn shell_menu_status(&self) -> ShellMenuStatus {
        let keys = ROOTS
            .iter()
            .all(|r| reg(&["query", &format!("{r}\\{ADD}")]).is_ok());
        // A menu whose entry runs something other than the CLI is not installed for
        // any purpose the user cares about, so the card says so and offers Add again.
        let installed = keys
            && ROOTS.iter().all(|r| {
                reg(&["query", &format!("{r}\\{ADD}\\command"), "/ve"])
                    .ok()
                    .as_deref()
                    .and_then(reg_value)
                    .is_some_and(|v| command_runs_the_cli(&v))
            });
        ShellMenuStatus {
            installed,
            cli_path: find_cli().map(|p| p.display().to_string()),
            supported: true,
        }
    }

    pub fn install_shell_menu(&self) -> Result<ShellMenuStatus, CoreError> {
        let cli = find_cli().ok_or_else(|| {
            CoreError::failed_fix(
                "The Explorer menu wasn't added.",
                "The `ols` command line program wasn't found next to the app or on PATH.",
                "Reinstall OLS, or add the folder holding ols-cli.exe to PATH.",
            )
        })?;
        let cli = cli.display().to_string();
        let icon = menu_icon(Path::new(&cli));
        let result = (|| -> Result<(), String> {
            for root in ROOTS {
                for (key, label, command) in commands(&cli) {
                    let base = format!("{root}\\{key}");
                    reg(&["add", &base, "/ve", "/d", label, "/f"])?;
                    reg(&["add", &base, "/v", "Icon", "/d", &icon, "/f"])?;
                    reg(&[
                        "add",
                        &format!("{base}\\command"),
                        "/ve",
                        "/d",
                        &command,
                        "/f",
                    ])?;
                }
            }
            Ok(())
        })();
        if let Err(e) = result {
            let _ = self.remove_shell_menu();
            return Err(CoreError::failed("The Explorer menu wasn't added.", e));
        }
        Ok(self.shell_menu_status())
    }

    pub fn remove_shell_menu(&self) -> Result<ShellMenuStatus, CoreError> {
        for root in ROOTS {
            for key in [ADD, SETUP] {
                let base = format!("{root}\\{key}");
                if reg(&["query", &base]).is_ok() {
                    reg(&["delete", &base, "/f"])
                        .map_err(|e| CoreError::failed("The Explorer menu wasn't removed.", e))?;
                }
            }
        }
        Ok(self.shell_menu_status())
    }
}

#[cfg(not(windows))]
impl Inner {
    pub fn shell_menu_status(&self) -> ShellMenuStatus {
        ShellMenuStatus {
            installed: false,
            cli_path: find_cli().map(|p| p.display().to_string()),
            supported: false,
        }
    }
    pub fn install_shell_menu(&self) -> Result<ShellMenuStatus, CoreError> {
        Err(CoreError::failed(
            "The Explorer menu wasn't added.",
            "It is only available on Windows.",
        ))
    }
    pub fn remove_shell_menu(&self) -> Result<ShellMenuStatus, CoreError> {
        Ok(self.shell_menu_status())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_run_only_the_cli_with_the_chosen_folder() {
        let c = commands(r"C:\Program Files\OLS\ols-cli.exe");
        assert_eq!(
            c[0].2,
            r#""C:\Program Files\OLS\ols-cli.exe" project add "%V""#
        );
        assert!(c[1].2.contains("setup --path \"%V\""));
        assert!(c.iter().all(|(k, _, _)| k.starts_with("OpenLocalServer.")));
    }

    /// The entries run the CLI, so the icon cannot come from it.
    #[test]
    fn the_menu_icon_is_the_apps_ico_not_the_console_exe() {
        let home = crate::test_support::isolated_home();
        let dir = home.paths.root().join("install");
        std::fs::create_dir_all(&dir).unwrap();
        let cli = dir.join(CLI);
        // No .ico yet: the CLI is the fallback, which is what a dev build has.
        assert_eq!(menu_icon(&cli), cli.display().to_string());
        std::fs::write(dir.join("icon.ico"), b"x").unwrap();
        assert_eq!(menu_icon(&cli), dir.join("icon.ico").display().to_string());
    }

    /// The lookup that caused it. Two independent mistakes, so two checks: the name
    /// must not be one Windows can confuse with the app's, and the lookup must not
    /// return the running executable even if something *is* sitting there under it.
    #[test]
    fn the_cli_name_cannot_be_the_app_image() {
        let cli = CLI.to_ascii_lowercase();
        assert_ne!(cli, "ols.exe", "the CLI cannot be named like the app");
        assert_ne!(cli, "openlocalserver.exe");
    }

    #[test]
    fn find_cli_never_returns_the_running_executable() {
        let me = std::env::current_exe().expect("test binary path");
        if let Some(found) = find_cli() {
            let a = std::fs::canonicalize(&found).unwrap_or_else(|_| found.clone());
            let b = std::fs::canonicalize(&me).unwrap_or_else(|_| me.clone());
            assert_ne!(a, b, "find_cli returned the running executable");
        }
    }

    /// The card showed "installed" over a menu that ran the app, because the status
    /// asked the registry whether the keys existed and never what they pointed at.
    /// The two lines below are exactly what a broken install holds.
    #[test]
    fn a_menu_pointing_at_the_app_is_not_reported_as_installed() {
        assert!(!command_runs_the_cli(
            r#""I:\OLS\ols.exe" project add "%V""#
        ));
        assert!(!command_runs_the_cli(
            r#""C:\Program Files\OLS\Open Local Server.exe" project add "%V""#
        ));
        assert!(command_runs_the_cli(
            r#""I:\OLS\ols-cli.exe" project add "%V""#
        ));
        // Case is the whole point: Windows does not distinguish the two names.
        assert!(command_runs_the_cli(
            r#""I:\OLS\OLS-CLI.EXE" project add "%V""#
        ));
        // The second entry runs cmd.exe, which is correct and must not be read as a
        // broken CLI; the first entry is the one that decides.
        assert!(!command_runs_the_cli(
            r#"cmd.exe /k ""C:\OLS\ols-cli.exe" setup --path "%V"""#
        ));
        assert!(!command_runs_the_cli(""));
    }

    #[test]
    fn the_registry_value_is_read_out_of_reg_query_output() {
        let out = "\r\nHKEY_CURRENT_USER\\Software\\Classes\\Directory\\shell\\OpenLocalServer.Add\\command\r\n    (Default)    REG_SZ    \"I:\\OLS\\ols-cli.exe\" project add \"%V\"\r\n\r\n";
        let v = reg_value(out).expect("value");
        assert_eq!(v, r#""I:\OLS\ols-cli.exe" project add "%V""#);
        assert!(command_runs_the_cli(&v));
        // A key with no (Default) line is not a command.
        assert!(reg_value("HKEY_CURRENT_USER\\x\n    (Default)    REG_DWORD    0x1\n").is_none());
    }
}
