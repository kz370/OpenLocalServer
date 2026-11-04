//! Explorer context menu (§124, Stage 17). Groups project actions under an Open Local
//! Server submenu for folders and folder backgrounds. Per user (`HKCU\Software\Classes`),
//! so no administrator prompt; removing it deletes the group and legacy keys added here.

use serde::{Deserialize, Serialize};

use std::path::Path;

use crate::app::Inner;
use crate::error::CoreError;

const ROOTS: [&str; 2] = [
    r"HKCU\Software\Classes\Directory\shell",
    r"HKCU\Software\Classes\Directory\Background\shell",
];
const MENU: &str = "OpenLocalServer";
const ADD: &str = "OpenLocalServer.01.Add";
const SETUP: &str = "OpenLocalServer.02.Setup";
const IGNORE_PROJECT: &str = "OpenLocalServer.03.IgnoreProject";
const IGNORE_SITE: &str = "OpenLocalServer.04.IgnoreSite";
const ACTIONS: [&str; 4] = [ADD, SETUP, IGNORE_PROJECT, IGNORE_SITE];
const LEGACY_ROOT_ACTIONS: [&str; 8] = [
    "OpenLocalServer.Add",
    "OpenLocalServer.Setup",
    "OpenLocalServer.01.Add",
    "OpenLocalServer.02.Setup",
    "OpenLocalServer.03.IgnoreProject",
    "OpenLocalServer.04.UnignoreProject",
    "OpenLocalServer.05.IgnoreSite",
    "OpenLocalServer.06.UnignoreSite",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellMenuStatus {
    pub installed: bool,
    /// The `ols` program the entries run, when it can be found.
    pub cli_path: Option<String>,
    pub supported: bool,
}

/// The CLI's file name. Not `ols.exe`: the app ships as `OLS.exe`, Windows does not
/// tell two names apart by case, and a lookup by that name finds the app.
const CLI: &str = if cfg!(windows) { "olsc.exe" } else { "olsc" };

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

/// Whether an executable name (without extension) is the desktop app: `OLS` when
/// shipped, `openlocalserver` when run by `cargo tauri dev`. Install and status must
/// agree on this, or a correct dev menu is reported as not installed.
fn is_app_stem(stem: &str) -> bool {
    stem.eq_ignore_ascii_case("OLS") || stem.eq_ignore_ascii_case("openlocalserver")
}

/// Where the main OLS desktop app is: the running exe, beside the CLI, or beside current exe.
///
/// The running exe counts as the app under its shipped name (`OLS`) and under its
/// Cargo name (`openlocalserver`, what `cargo tauri dev` runs). Without the second, a
/// dev build fell through to an installed `OLS.exe` of a different version.
fn find_app() -> Option<std::path::PathBuf> {
    let name = if cfg!(windows) { "OLS.exe" } else { "OLS" };
    if let Ok(exe) = std::env::current_exe() {
        if exe
            .file_stem()
            .and_then(|n| n.to_str())
            .is_some_and(is_app_stem)
        {
            return Some(exe);
        }
        let beside = exe.with_file_name(name);
        if beside.is_file() {
            return Some(beside);
        }
    }
    if let Some(cli) = find_cli() {
        let beside = cli.with_file_name(name);
        if beside.is_file() {
            return Some(beside);
        }
    }
    None
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
    let app = dir.join("OLS.exe");
    if app.is_file() {
        app.display().to_string()
    } else {
        let ico = dir.join("icon.ico");
        if ico.is_file() {
            ico.display().to_string()
        } else {
            // No OLS.exe or .ico beside the CLI: the CLI's own image is the fallback
            // (e.g. on dev builds where the CLI is not beside an install).
            cli.display().to_string()
        }
    }
}

/// The command each submenu entry runs; `%V` is the folder Explorer passes.
fn commands(app: &str, cli: &str) -> [(&'static str, &'static str, String); 4] {
    [
        (
            ADD,
            "Add folder to OLS projects",
            format!("\"{app}\" --add-project \"%V\""),
        ),
        (
            SETUP,
            "Set up with OLS",
            format!("cmd.exe /k \"\"{cli}\" setup --path \"%V\"\""),
        ),
        // The ignore actions run the windowed app, not the console CLI: a console
        // program always flashes a window, and no registry setting can hide it. The
        // app handles these flags before it starts anything (`run_menu_action`).
        (
            IGNORE_PROJECT,
            "Ignore this project",
            format!("\"{app}\" {IGNORE_PROJECT_FLAG} \"%V\""),
        ),
        (
            IGNORE_SITE,
            "Ignore this site",
            format!("\"{app}\" {IGNORE_SITE_FLAG} \"%V\""),
        ),
    ]
}

/// App flags the Explorer ignore actions pass.
pub const IGNORE_PROJECT_FLAG: &str = "--ignore-project";
pub const IGNORE_SITE_FLAG: &str = "--ignore-site";

/// The marker an ignore flag writes, if `flag` is one.
fn marker_for_flag(flag: &str) -> Option<&'static str> {
    match flag {
        IGNORE_PROJECT_FLAG => Some(crate::app::OLS_IGNORE_PROJECT_MARKER),
        IGNORE_SITE_FLAG => Some(crate::app::OLS_IGNORE_SITE_MARKER),
        _ => None,
    }
}

/// Create `marker` inside `folder`. An existing marker is success: the folder is
/// already ignored, which is what was asked for.
fn write_ignore_marker(folder: &Path, marker: &str) -> Result<(), String> {
    if !folder.is_dir() {
        return Err(format!(
            "'{}' is not an accessible folder.\n\nChoose an existing project folder and try again.",
            folder.display()
        ));
    }
    let marker_path = folder.join(marker);
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&marker_path)
    {
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(format!(
            "Could not create '{}': {error}.\n\nCheck that you can write to the project folder, then try again.",
            marker_path.display()
        )),
    }
}

/// Handle an Explorer ignore action passed to the app (`--ignore-project <folder>` or
/// `--ignore-site <folder>`). Returns the exit code when the arguments were one, so the
/// caller exits at once — before logging, the core, the daemon hand-over, or the
/// single-instance check, none of which a one-file write should touch. A running app
/// picks the marker up through its project-folder watch. Success is silent; a failure
/// is shown in a message box, since there is no console to print to.
pub fn run_menu_action(args: impl IntoIterator<Item = String>) -> Option<i32> {
    let mut args = args.into_iter().skip(1);
    let (marker, folder) = loop {
        let arg = args.next()?;
        if let Some(marker) = marker_for_flag(&arg) {
            break (marker, args.next());
        }
    };
    let result = match folder {
        Some(folder) => write_ignore_marker(Path::new(&folder), marker),
        None => Err("No folder was given.\n\nUse the Explorer menu on a folder.".into()),
    };
    match result {
        Ok(()) => Some(0),
        Err(message) => {
            show_error("Open Local Server could not ignore this folder", &message);
            Some(1)
        }
    }
}

#[cfg(windows)]
fn show_error(title: &str, message: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let (title, message) = (wide(title), wide(message));
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            message.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg(not(windows))]
fn show_error(title: &str, message: &str) {
    eprintln!("{title}: {message}");
}

/// The version out of `olsc --version` output (`olsc 2.9.0`).
#[cfg(any(windows, test))]
fn cli_version_from(out: &str) -> Option<&str> {
    out.split_whitespace().nth(1)
}

/// The CLI's version, if it runs and reports one.
#[cfg(windows)]
fn cli_version(cli: &Path) -> Option<String> {
    let mut cmd = std::process::Command::new(cli);
    cmd.arg("--version");
    crate::exec::hide_window(&mut cmd);
    let out = cmd.output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    cli_version_from(&text).map(str::to_string)
}

/// The program out of a stored command line, i.e. `C:\OLS\olsc.exe` out of
/// `"C:\OLS\olsc.exe" project add "%V"`. An unquoted program is the run up to
/// the first space, which is what the shell itself does.
#[cfg(any(windows, test))]
fn command_program(command: &str) -> Option<&str> {
    let t = command.trim();
    match t.strip_prefix('"') {
        Some(rest) => rest.split('"').next(),
        None => t.split(' ').next().filter(|p| !p.is_empty()),
    }
}

/// Whether a stored command line runs the app with `flag`.
#[cfg(any(windows, test))]
fn command_runs_app_with(command: &str, flag: &str) -> bool {
    let Some(program) = command_program(command) else {
        return false;
    };
    let is_app = std::path::Path::new(program)
        .file_stem()
        .and_then(|n| n.to_str())
        .is_some_and(is_app_stem);
    is_app && command.contains(flag)
}

/// Whether a stored command line runs the app with --add-project.
#[cfg(any(windows, test))]
fn command_is_add_project(command: &str) -> bool {
    command_runs_app_with(command, "--add-project")
}

/// Whether the stored command for `action` is the one this build writes. The setup
/// action runs the CLI in a kept-open console on purpose, so only its presence counts.
#[cfg(any(windows, test))]
fn action_command_is_current(action: &str, command: &str) -> bool {
    match action {
        ADD => command_is_add_project(command),
        IGNORE_PROJECT => command_runs_app_with(command, IGNORE_PROJECT_FLAG),
        IGNORE_SITE => command_runs_app_with(command, IGNORE_SITE_FLAG),
        _ => true,
    }
}

/// The `(Default)` value out of `reg query ... /ve` output.
///
/// Only the first line is read. reg.exe wraps a long `REG_SZ` across lines, but what
/// is compared is the program at the front of the value and the wrap lands on a word
/// boundary, so the leading `"C:\...\olsc.exe"` is always in the first line — and
/// guessing at the rest of a wrapped line is a worse way to answer this question than
/// not answering it.
#[cfg(any(windows, test))]
fn reg_value(out: &str) -> Option<String> {
    out.lines()
        .find(|l| l.contains("(Default)") && l.contains("REG_SZ"))
        .and_then(|l| l.split_once("REG_SZ"))
        .map(|(_, v)| v.trim().to_string())
        .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("(value not set)"))
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
fn remove_legacy_menu_keys() -> Result<(), String> {
    for root in ROOTS {
        let menu = format!("{root}\\{MENU}");
        if reg(&["query", &menu]).is_ok() {
            reg(&["delete", &menu, "/f"])?;
        }
        for key in LEGACY_ROOT_ACTIONS {
            let base = format!("{root}\\{key}");
            if reg(&["query", &base]).is_ok() {
                reg(&["delete", &base, "/f"])?;
            }
        }
    }
    Ok(())
}

#[cfg(windows)]
fn notify_shell_menu_changed() {
    unsafe {
        windows_sys::Win32::UI::Shell::SHChangeNotify(
            windows_sys::Win32::UI::Shell::SHCNE_ASSOCCHANGED as i32,
            windows_sys::Win32::UI::Shell::SHCNF_IDLIST,
            std::ptr::null(),
            std::ptr::null(),
        );
    }
}

#[cfg(windows)]
impl Inner {
    pub fn shell_menu_status(&self) -> ShellMenuStatus {
        let installed = ROOTS.iter().all(|root| {
            let menu = format!("{root}\\{MENU}");
            // A cascade is a parent with an empty `SubCommands` value and no command of
            // its own; Explorer renders a parent that has a command as a plain action.
            let cascade = reg(&["query", &menu, "/v", "SubCommands"]).is_ok();
            let direct_command = reg(&["query", &format!("{menu}\\command")]).is_ok()
                || reg(&["query", &menu, "/ve"])
                    .ok()
                    .and_then(|out| reg_value(&out))
                    .is_some();
            let legacy_flat_items = LEGACY_ROOT_ACTIONS
                .iter()
                .any(|action| reg(&["query", &format!("{root}\\{action}")]).is_ok());
            cascade
                && !direct_command
                && !legacy_flat_items
                && ACTIONS.iter().all(|action| {
                    reg(&["query", &format!("{menu}\\shell\\{action}\\command"), "/ve"])
                        .ok()
                        .as_deref()
                        .and_then(reg_value)
                        .is_some_and(|command| action_command_is_current(action, &command))
                })
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
                "Reinstall OLS, or add the folder holding olsc.exe to PATH.",
            )
        })?;
        // An older olsc (e.g. an installed release found on PATH by a dev build) lacks
        // the `project ignore` / `ignore-site` subcommands, and every action then fails
        // inside a console that closes at once. Refuse instead of writing a dead menu.
        let expected = env!("CARGO_PKG_VERSION");
        match cli_version(&cli) {
            Some(found) if found == expected => {}
            found => {
                return Err(CoreError::failed_fix(
                    "The Explorer menu wasn't added.",
                    format!(
                        "{} is version {}, but this OLS is {expected}. Its menu commands would not match.",
                        cli.display(),
                        found.as_deref().unwrap_or("unknown"),
                    ),
                    format!(
                        "Install OLS {expected}, or build the CLI (`cargo build -p ols-cli`) so olsc.exe sits beside the running app, then click Add to Explorer again."
                    ),
                ));
            }
        }
        let app = find_app()
            .unwrap_or_else(|| cli.with_file_name(if cfg!(windows) { "OLS.exe" } else { "OLS" }));
        let app_str = app.display().to_string();
        let cli_str = cli.display().to_string();
        let icon = menu_icon(&app);
        let result = (|| -> Result<(), String> {
            // Start clean: the old parent (with a stray command or the misplaced
            // `ExtendedSubCommandsKey` subkey) and every former flat verb go first, so
            // nothing renders beside or instead of the cascade.
            remove_legacy_menu_keys()?;
            for root in ROOTS {
                let menu = format!("{root}\\{MENU}");
                reg(&[
                    "add",
                    &menu,
                    "/v",
                    "MUIVerb",
                    "/d",
                    "Open Local Server",
                    "/f",
                ])?;
                reg(&["add", &menu, "/v", "Icon", "/d", &icon, "/f"])?;
                // Empty `SubCommands` tells Explorer the children live in `<menu>\shell`.
                reg(&["add", &menu, "/v", "SubCommands", "/d", "", "/f"])?;
                for (key, label, command) in commands(&app_str, &cli_str) {
                    let item = format!("{menu}\\shell\\{key}");
                    reg(&["add", &item, "/v", "MUIVerb", "/d", label, "/f"])?;
                    reg(&["add", &item, "/v", "Icon", "/d", &icon, "/f"])?;
                    reg(&[
                        "add",
                        &format!("{item}\\command"),
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
            let _ = remove_legacy_menu_keys();
            return Err(CoreError::failed_fix(
                "The Explorer menu wasn't added.",
                e,
                "Check that OLS can write to HKCU\\Software\\Classes, then click Add to Explorer again.",
            ));
        }
        notify_shell_menu_changed();
        Ok(self.shell_menu_status())
    }

    pub fn remove_shell_menu(&self) -> Result<ShellMenuStatus, CoreError> {
        remove_legacy_menu_keys().map_err(|error| {
            CoreError::failed_fix(
                "The Explorer menu wasn't removed.",
                error,
                "Check that OLS can write to HKCU\\Software\\Classes, then click Remove again.",
            )
        })?;
        notify_shell_menu_changed();
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
    fn entries_run_correct_commands_with_the_chosen_folder() {
        let c = commands(
            r"C:\Program Files\OLS\OLS.exe",
            r"C:\Program Files\OLS\olsc.exe",
        );
        assert_eq!(
            c[0].2,
            r#""C:\Program Files\OLS\OLS.exe" --add-project "%V""#
        );
        assert_eq!(c[0].1, "Add folder to OLS projects");
        assert!(c[1].2.contains("setup --path \"%V\""));
        // Ignore actions run the windowed app, so no console flashes.
        assert_eq!(
            c[2].2,
            r#""C:\Program Files\OLS\OLS.exe" --ignore-project "%V""#
        );
        assert_eq!(
            c[3].2,
            r#""C:\Program Files\OLS\OLS.exe" --ignore-site "%V""#
        );
        assert!(action_command_is_current(IGNORE_PROJECT, &c[2].2));
        assert!(action_command_is_current(IGNORE_SITE, &c[3].2));
        // The former console form must read as stale so the card offers Repair.
        assert!(!action_command_is_current(
            IGNORE_PROJECT,
            r#"cmd.exe /c ""C:\OLS\olsc.exe" project ignore "%V" || pause""#
        ));
        assert!(c.iter().all(|(k, _, _)| k.starts_with("OpenLocalServer.")));
    }

    #[test]
    fn menu_action_writes_the_marker_and_ignores_other_launches() {
        let home = crate::test_support::isolated_home();
        let folder = home.paths.root().join("site");
        std::fs::create_dir_all(&folder).unwrap();
        let args = |flag: &str| {
            vec![
                "OLS.exe".to_string(),
                flag.to_string(),
                folder.display().to_string(),
            ]
        };
        assert_eq!(run_menu_action(args(IGNORE_PROJECT_FLAG)), Some(0));
        assert!(folder.join(crate::app::OLS_IGNORE_PROJECT_MARKER).is_file());
        // Already ignored is still success.
        assert_eq!(run_menu_action(args(IGNORE_PROJECT_FLAG)), Some(0));
        assert_eq!(run_menu_action(args(IGNORE_SITE_FLAG)), Some(0));
        assert!(folder.join(crate::app::OLS_IGNORE_SITE_MARKER).is_file());
        // Normal launches are not menu actions.
        assert_eq!(run_menu_action(vec!["OLS.exe".to_string()]), None);
        assert_eq!(run_menu_action(args("--add-project")), None);
    }

    #[test]
    fn the_cli_version_is_read_out_of_version_output() {
        assert_eq!(cli_version_from("olsc 2.9.0\r\n"), Some("2.9.0"));
        assert_eq!(cli_version_from(""), None);
    }

    /// The entries run the CLI, so the icon cannot come from it.
    #[test]
    fn the_menu_icon_is_the_apps_ico_not_the_console_exe() {
        let home = crate::test_support::isolated_home();
        let dir = home.paths.root().join("install");
        std::fs::create_dir_all(&dir).unwrap();
        let cli = dir.join(CLI);
        // No .ico or OLS.exe yet: the CLI is the fallback, which is what a dev build has.
        assert_eq!(menu_icon(&cli), cli.display().to_string());
        std::fs::write(dir.join("icon.ico"), b"x").unwrap();
        assert_eq!(menu_icon(&cli), dir.join("icon.ico").display().to_string());
        std::fs::write(dir.join("OLS.exe"), b"x").unwrap();
        assert_eq!(menu_icon(&cli), dir.join("OLS.exe").display().to_string());
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
        assert!(!command_is_add_project(
            r#""I:\OLS\ols.exe" project add "%V""#
        ));
        assert!(!command_is_add_project(
            r#""C:\Program Files\OLS\Open Local Server.exe" project add "%V""#
        ));
        assert!(command_is_add_project(
            r#""I:\OLS\OLS.exe" --add-project "%V""#
        ));
        // Case is the whole point: Windows does not distinguish the two names.
        assert!(command_is_add_project(
            r#""I:\OLS\ols.EXE" --add-project "%V""#
        ));
        // The second entry runs cmd.exe, which is correct and must not be read as a
        // broken CLI; the first entry is the one that decides.
        assert!(!command_is_add_project(
            r#"cmd.exe /k ""C:\OLS\olsc.exe" setup --path "%V"""#
        ));
        assert!(!command_is_add_project(""));
        // A dev build registers its Cargo-named exe; status must accept it.
        assert!(command_is_add_project(
            r#""I:\repo\target\debug\openlocalserver.exe" --add-project "%V""#
        ));
    }

    #[test]
    fn the_registry_value_is_read_out_of_reg_query_output() {
        let out = "\r\nHKEY_CURRENT_USER\\Software\\Classes\\Directory\\shell\\OpenLocalServer.Add\\command\r\n    (Default)    REG_SZ    \"I:\\OLS\\OLS.exe\" --add-project \"%V\"\r\n\r\n";
        let v = reg_value(out).expect("value");
        assert_eq!(v, r#""I:\OLS\OLS.exe" --add-project "%V""#);
        assert!(command_is_add_project(&v));
        // A key with no (Default) line is not a command.
        assert!(reg_value("HKEY_CURRENT_USER\\x\n    (Default)    REG_DWORD    0x1\n").is_none());
    }
}
