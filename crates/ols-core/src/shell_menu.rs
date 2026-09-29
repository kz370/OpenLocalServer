//! Explorer context menu (§124, Stage 17). Adds "Add to OLS" and "Set up with OLS"
//! to the right-click menu of a folder (and of its background). Per user (`HKCU\Software\Classes`), so no
//! administrator prompt; it only ever runs the `ols` command line, which does the work through the core.
//! Removing it deletes exactly the keys added here.

use serde::{Deserialize, Serialize};

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

/// Where `ols.exe` is: beside the app, or on PATH.
fn find_cli() -> Option<std::path::PathBuf> {
    let name = if cfg!(windows) { "ols.exe" } else { "ols" };
    if let Ok(exe) = std::env::current_exe() {
        let beside = exe.with_file_name(name);
        if beside.is_file() {
            return Some(beside);
        }
    }
    std::env::var_os("PATH").and_then(|p| {
        std::env::split_paths(&p)
            .map(|d| d.join(name))
            .find(|f| f.is_file())
    })
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
        let installed = ROOTS
            .iter()
            .all(|r| reg(&["query", &format!("{r}\\{ADD}")]).is_ok());
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
                "Reinstall OLS, or add the folder holding ols.exe to PATH.",
            )
        })?;
        let cli = cli.display().to_string();
        let result = (|| -> Result<(), String> {
            for root in ROOTS {
                for (key, label, command) in commands(&cli) {
                    let base = format!("{root}\\{key}");
                    reg(&["add", &base, "/ve", "/d", label, "/f"])?;
                    reg(&["add", &base, "/v", "Icon", "/d", &cli, "/f"])?;
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
        let c = commands(r"C:\Program Files\OLS\ols.exe");
        assert_eq!(c[0].2, r#""C:\Program Files\OLS\ols.exe" project add "%V""#);
        assert!(c[1].2.contains("setup --path \"%V\""));
        assert!(c.iter().all(|(k, _, _)| k.starts_with("OpenLocalServer.")));
    }
}
