//! Code editors the user can open a site or file in (§94–99). VS Code is the default;
//! any other known editor found on this machine can be picked in Settings, or a custom
//! executable path given instead. Detection runs on every `ListEditors`, so Settings'
//! "Refresh" re-checks the machine.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditorInfo {
    pub id: String,
    pub name: String,
    /// Where it was found; `None` when it isn't installed.
    pub path: Option<String>,
    #[serde(default)]
    pub custom: bool,
    #[serde(default)]
    pub hidden: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomEditor {
    pub id: String,
    pub name: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
}

pub const CUSTOM_EDITORS_SETTING: &str = "editors.custom";
pub const HIDDEN_EDITORS_SETTING: &str = "editors.hidden";
const LEGACY_EDITOR_MIGRATED_SETTING: &str = "editors.legacy_migrated";

/// Migrate the removed default-editor pair exactly once, retaining the legacy values.
pub fn migrate_legacy_settings(settings: &mut crate::settings::SettingsService) {
    if settings
        .get(LEGACY_EDITOR_MIGRATED_SETTING)
        .and_then(|v| v.as_bool())
        == Some(true)
    {
        return;
    }
    let old_id = settings
        .get("editor")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let old_command = settings
        .get("editor.command")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    let mut list = settings
        .get(CUSTOM_EDITORS_SETTING)
        .and_then(|v| serde_json::from_value::<Vec<CustomEditor>>(v.clone()).ok())
        .unwrap_or_default();
    // VS Code is already part of the detected editor list, so retaining it as a custom
    // editor would show a duplicate entry. The legacy choice now resolves to that entry.
    let _migrated_vscode_choice = old_id == "vscode";
    if !old_command.is_empty() && !list.iter().any(|e| e.id == "legacy:custom") {
        let name = Path::new(old_command)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Custom Editor")
            .to_string();
        list.push(CustomEditor {
            id: "legacy:custom".into(),
            name,
            path: old_command.to_string(),
            args: None,
        });
    }
    let _ = save_custom(settings, &list);
    let _ = settings.set(
        LEGACY_EDITOR_MIGRATED_SETTING,
        serde_json::Value::Bool(true),
    );
}

/// Reads user-configured hidden editor IDs from settings.
pub fn list_hidden(settings: &crate::settings::SettingsService) -> Vec<String> {
    if let Some(val) = settings.get(HIDDEN_EDITORS_SETTING) {
        if let Ok(list) = serde_json::from_value::<Vec<String>>(val.clone()) {
            return list;
        }
    }
    Vec::new()
}

/// Persists hidden editor IDs to settings.
pub fn save_hidden(
    settings: &mut crate::settings::SettingsService,
    list: &[String],
) -> Result<(), crate::error::CoreError> {
    let val = serde_json::to_value(list)
        .map_err(|e| crate::error::CoreError::ServiceError(e.to_string()))?;
    settings.set(HIDDEN_EDITORS_SETTING, val)
}

/// Reads user-configured custom editors from settings.
pub fn list_custom(settings: &crate::settings::SettingsService) -> Vec<CustomEditor> {
    if let Some(val) = settings.get(CUSTOM_EDITORS_SETTING) {
        if let Ok(list) = serde_json::from_value::<Vec<CustomEditor>>(val.clone()) {
            return list;
        }
    }
    Vec::new()
}

/// Persists custom editors list to settings.
pub fn save_custom(
    settings: &mut crate::settings::SettingsService,
    list: &[CustomEditor],
) -> Result<(), crate::error::CoreError> {
    let val = serde_json::to_value(list)
        .map_err(|e| crate::error::CoreError::ServiceError(e.to_string()))?;
    settings.set(CUSTOM_EDITORS_SETTING, val)
}

/// Returns all editors: detected system editors plus user-configured custom editors.
pub fn list_all(settings: &crate::settings::SettingsService) -> Vec<EditorInfo> {
    let hidden = list_hidden(settings);
    let mut all = detect();
    let custom = list_custom(settings);
    for c in custom {
        if all.iter().any(|e| e.id == c.id) {
            continue;
        }
        all.push(EditorInfo {
            id: c.id,
            name: c.name,
            path: Some(c.path),
            custom: true,
            hidden: false,
        });
    }
    for e in &mut all {
        e.hidden = hidden.contains(&e.id);
    }
    all
}

/// One editor the app knows how to find.
struct Known {
    id: &'static str,
    name: &'static str,
    /// The executable inside an install folder, preferred first. May include a subfolder
    /// (`bin/studio64.exe`).
    exes: &'static [&'static str],
    /// Install folders under the usual roots (`%LOCALAPPDATA%\Programs`, `Program Files`, …).
    folders: &'static [&'static str],
    /// Scoop package names (`scoop\apps\<name>\current`).
    scoop: &'static [&'static str],
    /// Command-line launchers looked up on `PATH`.
    shims: &'static [&'static str],
    /// Built on VS Code. A real install ships `resources\app\product.json`; this keeps an
    /// unrelated program that happens to share the executable name out of the list.
    vscode_based: bool,
}

const KNOWN: &[Known] = &[
    Known {
        id: "vscode",
        name: "VS Code",
        exes: &["Code.exe"],
        folders: &["Microsoft VS Code"],
        scoop: &["vscode"],
        shims: &["code"],
        vscode_based: true,
    },
    Known {
        id: "vscode-insiders",
        name: "VS Code Insiders",
        exes: &["Code - Insiders.exe"],
        folders: &["Microsoft VS Code Insiders"],
        scoop: &["vscode-insiders"],
        shims: &["code-insiders"],
        vscode_based: true,
    },
    Known {
        id: "cursor",
        name: "Cursor",
        exes: &["Cursor.exe"],
        folders: &["cursor", "Cursor"],
        scoop: &["cursor"],
        shims: &["cursor"],
        vscode_based: true,
    },
    Known {
        id: "windsurf",
        name: "Windsurf",
        exes: &["Windsurf.exe"],
        folders: &["Windsurf", "windsurf"],
        scoop: &["windsurf"],
        shims: &["windsurf"],
        vscode_based: true,
    },
    Known {
        id: "trae",
        name: "Trae",
        exes: &["Trae.exe"],
        folders: &["Trae", "trae"],
        scoop: &[],
        shims: &["trae"],
        vscode_based: true,
    },
    Known {
        id: "kiro",
        name: "Kiro",
        exes: &["Kiro.exe"],
        folders: &["Kiro", "kiro"],
        scoop: &[],
        shims: &["kiro"],
        vscode_based: true,
    },
    Known {
        id: "antigravity",
        name: "Antigravity",
        exes: &["Antigravity IDE.exe", "Antigravity.exe"],
        folders: &["Antigravity IDE", "Antigravity", "antigravity"],
        scoop: &[],
        shims: &["antigravity-ide", "antigravity"],
        vscode_based: true,
    },
    Known {
        id: "zed",
        name: "Zed",
        exes: &["Zed.exe"],
        folders: &["Zed", "zed"],
        scoop: &["zed"],
        shims: &["zed"],
        vscode_based: false,
    },
    Known {
        id: "vscodium",
        name: "VSCodium",
        exes: &["VSCodium.exe"],
        folders: &["VSCodium"],
        scoop: &["vscodium"],
        shims: &["codium"],
        vscode_based: true,
    },
    Known {
        id: "sublime",
        name: "Sublime Text",
        exes: &["sublime_text.exe"],
        folders: &["Sublime Text", "Sublime Text 3"],
        scoop: &["sublime-text"],
        shims: &["subl"],
        vscode_based: false,
    },
    Known {
        id: "notepadpp",
        name: "Notepad++",
        exes: &["notepad++.exe"],
        folders: &["Notepad++"],
        scoop: &["notepadplusplus"],
        shims: &["notepad++"],
        vscode_based: false,
    },
    Known {
        id: "androidstudio",
        name: "Android Studio",
        exes: &["bin/studio64.exe"],
        folders: &["Android/Android Studio", "Android Studio"],
        scoop: &["android-studio"],
        shims: &[],
        vscode_based: false,
    },
];

const JETBRAINS: &[(&str, &str, &str, &str)] = &[
    ("phpstorm", "PhpStorm", "PhpStorm", "phpstorm64.exe"),
    ("webstorm", "WebStorm", "WebStorm", "webstorm64.exe"),
    ("pycharm", "PyCharm", "PyCharm", "pycharm64.exe"),
    ("intellij", "IntelliJ IDEA", "IntelliJ IDEA", "idea64.exe"),
    ("goland", "GoLand", "GoLand", "goland64.exe"),
    ("rustrover", "RustRover", "RustRover", "rustrover64.exe"),
    ("rider", "Rider", "Rider", "rider64.exe"),
    ("clion", "CLion", "CLion", "clion64.exe"),
    ("rubymine", "RubyMine", "RubyMine", "rubymine64.exe"),
];

/// Folders under `dir` whose name starts with `prefix` (case-insensitive).
fn subdirs_with_prefix(dir: &Path, prefix: &str) -> Vec<PathBuf> {
    let prefix = prefix.to_lowercase();
    std::fs::read_dir(dir)
        .map(|it| {
            it.filter_map(Result::ok)
                .filter(|e| {
                    e.file_name()
                        .to_string_lossy()
                        .to_lowercase()
                        .starts_with(&prefix)
                })
                .map(|e| e.path())
                .collect()
        })
        .unwrap_or_default()
}

fn find_jetbrains(
    roots: &[PathBuf],
    installed: &[PathBuf],
    product_prefix: &str,
    exe_name: &str,
) -> Option<PathBuf> {
    let bin = |dir: &Path| Some(dir.join("bin").join(exe_name)).filter(|p| p.is_file());
    for r in roots {
        // Standalone installer (`JetBrains\PhpStorm 2024.2`) and Toolbox 2.x, which installs
        // straight into `%LOCALAPPDATA%\Programs\PhpStorm`.
        for dir in [r.join("JetBrains"), r.clone()] {
            if let Some(p) = subdirs_with_prefix(&dir, product_prefix)
                .iter()
                .find_map(|d| bin(d))
            {
                return Some(p);
            }
        }
        // Toolbox 1.x: `JetBrains\Toolbox\apps\<product>\<channel>\<build>\bin`.
        let tb = r
            .join("JetBrains")
            .join("Toolbox")
            .join("apps")
            .join(product_prefix);
        if let Ok(channels) = std::fs::read_dir(&tb) {
            for ch in channels.filter_map(Result::ok) {
                if let Ok(builds) = std::fs::read_dir(ch.path()) {
                    for build in builds.filter_map(Result::ok) {
                        if let Some(p) = bin(&build.path()) {
                            return Some(p);
                        }
                    }
                }
            }
        }
    }
    installed.iter().find_map(|d| bin(d))
}

/// Visual Studio: `Microsoft Visual Studio\<version>\<edition>\Common7\IDE\devenv.exe`.
/// The newest version wins.
fn find_visual_studio(roots: &[PathBuf], installed: &[PathBuf]) -> Option<PathBuf> {
    let devenv = |dir: &Path| {
        Some(dir.join("Common7").join("IDE").join("devenv.exe")).filter(|p| p.is_file())
    };
    let mut versions: Vec<PathBuf> = roots
        .iter()
        .flat_map(|r| subdirs_with_prefix(&r.join("Microsoft Visual Studio"), ""))
        .collect();
    // Version folders are years (`2022`) or majors (`18`); compare them as numbers.
    versions.sort_by_key(|v| {
        std::cmp::Reverse(
            v.file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.parse::<u32>().ok())
                .map(|n| if n < 100 { n + 2008 } else { n })
                .unwrap_or(0),
        )
    });
    versions
        .iter()
        .flat_map(|v| subdirs_with_prefix(v, ""))
        .find_map(|edition| devenv(&edition))
        .or_else(|| installed.iter().find_map(|d| devenv(d)))
}

/// `true` when `dir` (or one versioned subfolder of it, as newer VS Code installs use)
/// holds `resources\app\product.json`.
fn ships_vscode_product(dir: &Path) -> bool {
    let direct = |d: &Path| {
        d.join("resources")
            .join("app")
            .join("product.json")
            .is_file()
    };
    direct(dir)
        || std::fs::read_dir(dir)
            .map(|it| {
                it.filter_map(Result::ok)
                    .any(|e| e.path().is_dir() && direct(&e.path()))
            })
            .unwrap_or(false)
}

/// `true` when `exe` belongs to a VS Code-based install: the program itself, or the
/// command-line launcher in its `bin\` folder (`<install>\bin\code.cmd`, or Cursor's
/// `resources\app\bin\cursor.cmd`).
fn is_vscode_install(exe: &Path) -> bool {
    let Some(dir) = exe.parent() else {
        return false;
    };
    if ships_vscode_product(dir) {
        return true;
    }
    let in_bin = dir
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.eq_ignore_ascii_case("bin"));
    match dir.parent().filter(|_| in_bin) {
        Some(up) => {
            ships_vscode_product(up)
                || (up.ends_with(Path::new("resources").join("app"))
                    && up.join("product.json").is_file())
        }
        None => false,
    }
}

fn find_known(
    k: &Known,
    roots: &[PathBuf],
    scoop: &[PathBuf],
    installed: &[PathBuf],
) -> Option<PathBuf> {
    let accept = |p: &Path| !k.vscode_based || is_vscode_install(p);
    let dirs = roots
        .iter()
        .flat_map(|r| k.folders.iter().map(move |f| r.join(f)))
        .chain(
            scoop
                .iter()
                .flat_map(|s| k.scoop.iter().map(move |n| s.join(n).join("current"))),
        )
        .chain(installed.iter().cloned());
    dirs.flat_map(|d| k.exes.iter().map(move |e| d.join(e)))
        .find(|p| p.is_file() && accept(p))
        .or_else(|| {
            k.shims
                .iter()
                .filter_map(|s| crate::web::manager::find_executable(None, s))
                .find(|p| accept(p))
        })
}

/// Where programs are usually installed: per-user and machine-wide.
fn install_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    for v in [
        "LOCALAPPDATA",
        "ProgramFiles",
        "ProgramW6432",
        "ProgramFiles(x86)",
        "APPDATA",
    ] {
        if let Some(r) = std::env::var_os(v).map(PathBuf::from) {
            for p in [r.join("Programs"), r] {
                if !roots.contains(&p) {
                    roots.push(p);
                }
            }
        }
    }
    roots
}

/// Scoop's `apps` folders: per-user (`%SCOOP%` or `~\scoop`) and global.
fn scoop_roots() -> Vec<PathBuf> {
    let user = std::env::var_os("SCOOP")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join("scoop")));
    let global = std::env::var_os("SCOOP_GLOBAL")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("ProgramData").map(|d| PathBuf::from(d).join("scoop")));
    [user, global]
        .into_iter()
        .flatten()
        .map(|r| r.join("apps"))
        .collect()
}

/// Every known editor, with where it's installed (if it is). Looks in the usual install
/// folders, Scoop, the folders installers register with Windows (so an editor installed
/// on another drive or in a custom folder is still found), and `PATH`.
pub fn detect() -> Vec<EditorInfo> {
    let roots = install_roots();
    let scoop = scoop_roots();
    let installed = registered_install_dirs();
    let info = |id: &str, name: &str, path: Option<PathBuf>| EditorInfo {
        id: id.to_string(),
        name: name.to_string(),
        path: path.map(|p| p.display().to_string()),
        custom: false,
        hidden: false,
    };

    let mut out: Vec<EditorInfo> = KNOWN
        .iter()
        .map(|k| info(k.id, k.name, find_known(k, &roots, &scoop, &installed)))
        .collect();

    out.push(info(
        "visualstudio",
        "Visual Studio",
        find_visual_studio(&roots, &installed),
    ));

    for (id, name, prefix, exe) in JETBRAINS {
        let path = find_jetbrains(&roots, &installed, prefix, exe)
            .or_else(|| crate::web::manager::find_executable(None, id));
        out.push(info(id, name, path));
    }

    out
}

/// The folder a registry `InstallLocation` or `DisplayIcon` value points at. `DisplayIcon`
/// names a file, optionally quoted and followed by an icon index (`"C:\x\a.exe",0`).
fn registry_dir(value: &str, is_icon: bool) -> Option<PathBuf> {
    let mut v = value.trim();
    if is_icon {
        if let Some((path, index)) = v.rsplit_once(',') {
            if index
                .trim()
                .trim_start_matches('-')
                .chars()
                .all(|c| c.is_ascii_digit())
            {
                v = path.trim();
            }
        }
    }
    let v = v.trim_matches('"').trim();
    if v.is_empty() {
        return None;
    }
    let p = PathBuf::from(v);
    let dir = if is_icon {
        p.parent()?.to_path_buf()
    } else {
        p
    };
    dir.is_absolute().then_some(dir)
}

/// Install folders registered under the Windows "Uninstall" keys (per-user, 64-bit and
/// 32-bit machine-wide).
#[cfg(windows)]
fn registered_install_dirs() -> Vec<PathBuf> {
    use windows_sys::Win32::Foundation::{ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER,
        HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY, RRF_RT_REG_SZ,
    };

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// A string value of subkey `sub` (NUL-terminated) of `key`.
    fn read_string(key: HKEY, sub: &[u16], value: &str) -> Option<String> {
        let name = wide(value);
        let mut buf = vec![0u16; 512];
        for _ in 0..2 {
            let mut bytes = (buf.len() * 2) as u32;
            // SAFETY: `sub` and `name` are NUL-terminated UTF-16; `buf` is `bytes` long.
            let rc = unsafe {
                RegGetValueW(
                    key,
                    sub.as_ptr(),
                    name.as_ptr(),
                    RRF_RT_REG_SZ,
                    std::ptr::null_mut(),
                    buf.as_mut_ptr().cast(),
                    &mut bytes,
                )
            };
            if rc == ERROR_MORE_DATA && bytes as usize <= 64 * 1024 {
                buf = vec![0u16; (bytes as usize).div_ceil(2)];
                continue;
            }
            if rc != ERROR_SUCCESS {
                return None;
            }
            let len = (bytes as usize / 2).min(buf.len());
            let s = String::from_utf16_lossy(&buf[..len]);
            let s = s.trim_end_matches('\0').trim();
            return (!s.is_empty()).then(|| s.to_string());
        }
        None
    }

    let uninstall = wide(r"Software\Microsoft\Windows\CurrentVersion\Uninstall");
    let mut out: Vec<PathBuf> = Vec::new();
    for (hive, view) in [
        (HKEY_CURRENT_USER, 0),
        (HKEY_LOCAL_MACHINE, KEY_WOW64_64KEY),
        (HKEY_LOCAL_MACHINE, KEY_WOW64_32KEY),
    ] {
        let mut key: HKEY = std::ptr::null_mut();
        // SAFETY: `uninstall` is NUL-terminated; `key` receives the opened handle.
        if unsafe { RegOpenKeyExW(hive, uninstall.as_ptr(), 0, KEY_READ | view, &mut key) }
            != ERROR_SUCCESS
        {
            continue;
        }
        for index in 0..4096u32 {
            let mut name = [0u16; 256];
            let mut len = name.len() as u32;
            // SAFETY: `name` holds `len` UTF-16 units; the optional out-params are null.
            let rc = unsafe {
                RegEnumKeyExW(
                    key,
                    index,
                    name.as_mut_ptr(),
                    &mut len,
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            if rc == ERROR_NO_MORE_ITEMS {
                break;
            }
            if rc != ERROR_SUCCESS {
                continue;
            }
            let mut sub = name[..len as usize].to_vec();
            sub.push(0);
            for (value, is_icon) in [("InstallLocation", false), ("DisplayIcon", true)] {
                if let Some(dir) =
                    read_string(key, &sub, value).and_then(|v| registry_dir(&v, is_icon))
                {
                    if !out.contains(&dir) {
                        out.push(dir);
                    }
                }
            }
        }
        // SAFETY: `key` was opened above and is closed once.
        unsafe { RegCloseKey(key) };
    }
    out
}

#[cfg(not(windows))]
fn registered_install_dirs() -> Vec<PathBuf> {
    Vec::new()
}

/// The executable to open things with: a custom path wins, then the chosen editor, then
/// VS Code, then any installed editor. `None` when nothing usable is installed.
pub fn resolve(chosen: Option<&str>, custom: Option<&str>) -> Option<PathBuf> {
    if let Some(c) = custom.map(str::trim).filter(|c| !c.is_empty()) {
        return Some(PathBuf::from(c));
    }
    let all = detect();
    let find = |id: &str| all.iter().find(|e| e.id == id).and_then(|e| e.path.clone());
    chosen
        .and_then(find)
        .or_else(|| find("vscode"))
        .or_else(|| all.iter().find_map(|e| e.path.clone()))
        .map(PathBuf::from)
}

/// The arguments that open `path`. Notepad++ opens every file inside a plain folder
/// argument, so a folder goes in as a workspace (its folder panel) instead.
pub fn open_args(exe: &Path, path: &str) -> Vec<String> {
    let notepadpp = exe
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.eq_ignore_ascii_case("notepad++.exe"));
    if notepadpp && Path::new(path).is_dir() {
        vec!["-openFoldersAsWorkspace".to_string(), path.to_string()]
    } else {
        vec![path.to_string()]
    }
}

/// The arguments that open `path` at `line` (§175). Editors that cannot jump to a line fall
/// back to [`open_args`].
pub fn open_args_at(exe: &Path, path: &str, line: u32) -> Vec<String> {
    let stem = exe
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match stem.as_str() {
        "code" | "code-insiders" | "code - insiders" | "codium" | "vscodium" | "cursor"
        | "windsurf" | "trae" | "kiro" | "antigravity" | "antigravity ide" | "antigravity-ide" => {
            vec!["-g".to_string(), format!("{path}:{line}")]
        }
        "phpstorm64" | "phpstorm" | "webstorm64" | "webstorm" | "idea64" | "idea" | "pycharm64"
        | "pycharm" | "rider64" | "rider" | "goland64" | "goland" | "rustrover64" | "rustrover"
        | "clion64" | "clion" | "rubymine64" | "rubymine" | "studio64" | "studio" => {
            vec!["--line".to_string(), line.to_string(), path.to_string()]
        }
        "sublime_text" | "subl" | "zed" => vec![format!("{path}:{line}")],
        "notepad++" => vec![format!("-n{line}"), path.to_string()],
        _ => open_args(exe, path),
    }
}

/// `true` for `.cmd`/`.bat` shims, which must run through `cmd.exe /C`.
pub fn is_shim(exe: &Path) -> bool {
    exe.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_args_at_jumps_to_the_line_for_each_editor_family() {
        let p = r"C:\sites\shop\app\Foo.php";
        let at = |exe: &str| open_args_at(Path::new(exe), p, 42);
        let g = vec!["-g".to_string(), format!("{p}:42")];
        for exe in [
            r"C:\x\Code.exe",
            r"C:\x\code.cmd",
            r"C:\x\Cursor.exe",
            r"C:\x\windsurf.exe",
            r"C:\x\codium.exe",
        ] {
            assert_eq!(at(exe), g, "{exe}");
        }
        let jb = vec!["--line".to_string(), "42".to_string(), p.to_string()];
        assert_eq!(at(r"C:\x\phpstorm64.exe"), jb);
        assert_eq!(at(r"C:\x\rider.exe"), jb);
        assert_eq!(at(r"C:\x\clion64.exe"), jb);
        assert_eq!(at(r"C:\x\bin\studio64.exe"), jb);
        for exe in [
            r"C:\x\Code - Insiders.exe",
            r"C:\x\Kiro.exe",
            r"C:\x\Antigravity IDE.exe",
            r"C:\x\bin\antigravity-ide.cmd",
        ] {
            assert_eq!(at(exe), g, "{exe}");
        }
        assert_eq!(at(r"C:\x\devenv.exe"), vec![p.to_string()]);
        assert_eq!(at(r"C:\x\sublime_text.exe"), vec![format!("{p}:42")]);
        assert_eq!(at(r"C:\x\zed.exe"), vec![format!("{p}:42")]);
        assert_eq!(
            at(r"C:\x\notepad++.exe"),
            vec!["-n42".to_string(), p.to_string()]
        );
        assert_eq!(at(r"C:\x\unknown.exe"), vec![p.to_string()]);
    }

    #[test]
    fn notepadpp_gets_folders_as_a_workspace_and_files_as_plain_paths() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().display().to_string();
        let exe = Path::new(r"C:\Program Files\Notepad++\notepad++.exe");
        assert_eq!(
            open_args(exe, &folder),
            vec!["-openFoldersAsWorkspace".to_string(), folder.clone()]
        );
        let file = dir.path().join("a.php");
        std::fs::write(&file, "").unwrap();
        assert_eq!(
            open_args(exe, &file.display().to_string()),
            vec![file.display().to_string()]
        );
        assert_eq!(
            open_args(Path::new(r"C:\Code\Code.exe"), &folder),
            vec![folder]
        );
    }

    #[test]
    fn antigravity_is_offered_once() {
        assert_eq!(detect().iter().filter(|e| e.id == "antigravity").count(), 1);
    }

    #[test]
    fn detected_ids_are_unique() {
        let all = detect();
        for e in &all {
            assert_eq!(all.iter().filter(|o| o.id == e.id).count(), 1, "{}", e.id);
        }
    }

    fn touch(p: &Path) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, "").unwrap();
    }

    #[test]
    fn only_real_vscode_based_installs_count() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // A VS Code fork: `resources\app\product.json` next to the program.
        let fork = root.join("Antigravity IDE");
        touch(&fork.join("Antigravity IDE.exe"));
        touch(&fork.join("resources/app/product.json"));
        touch(&fork.join("bin/antigravity-ide.cmd"));
        assert!(is_vscode_install(&fork.join("Antigravity IDE.exe")));
        assert!(is_vscode_install(&fork.join("bin/antigravity-ide.cmd")));

        // An unrelated Electron app with the same name: no product.json.
        let other = root.join("antigravity");
        touch(&other.join("Antigravity.exe"));
        touch(&other.join("resources/app.asar"));
        assert!(!is_vscode_install(&other.join("Antigravity.exe")));

        // Newer VS Code: the app lives in a versioned subfolder.
        let code = root.join("Microsoft VS Code");
        touch(&code.join("Code.exe"));
        touch(&code.join("c4453f8ea4/resources/app/product.json"));
        touch(&code.join("bin/code.cmd"));
        assert!(is_vscode_install(&code.join("Code.exe")));
        assert!(is_vscode_install(&code.join("bin/code.cmd")));

        // Cursor keeps its launcher inside `resources\app\bin`.
        let cursor = root.join("cursor");
        touch(&cursor.join("resources/app/product.json"));
        touch(&cursor.join("resources/app/bin/cursor.cmd"));
        assert!(is_vscode_install(
            &cursor.join("resources/app/bin/cursor.cmd")
        ));
    }

    #[test]
    fn find_known_skips_a_same_named_program_and_finds_the_fork() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let k = KNOWN.iter().find(|k| k.id == "antigravity").unwrap();

        touch(&root.join("antigravity/Antigravity.exe"));
        let only_other = find_known(
            &Known { shims: &[], ..*k },
            std::slice::from_ref(&root),
            &[],
            &[],
        );
        assert_eq!(only_other, None);

        touch(&root.join("Antigravity IDE/Antigravity IDE.exe"));
        touch(&root.join("Antigravity IDE/resources/app/product.json"));
        let found = find_known(
            &Known { shims: &[], ..*k },
            std::slice::from_ref(&root),
            &[],
            &[],
        );
        assert_eq!(
            found,
            Some(root.join("Antigravity IDE/Antigravity IDE.exe"))
        );
    }

    #[test]
    fn find_known_uses_registered_install_folders() {
        let dir = tempfile::tempdir().unwrap();
        let custom = dir.path().join("D-drive/Tools/Notepad++");
        touch(&custom.join("notepad++.exe"));
        let k = KNOWN.iter().find(|k| k.id == "notepadpp").unwrap();
        let found = find_known(
            &Known { shims: &[], ..*k },
            &[],
            &[],
            std::slice::from_ref(&custom),
        );
        assert_eq!(found, Some(custom.join("notepad++.exe")));
    }

    #[test]
    fn jetbrains_toolbox_2_and_registered_folders_are_found() {
        let dir = tempfile::tempdir().unwrap();
        let programs = dir.path().join("Programs");
        touch(&programs.join("PhpStorm/bin/phpstorm64.exe"));
        assert_eq!(
            find_jetbrains(
                std::slice::from_ref(&programs),
                &[],
                "PhpStorm",
                "phpstorm64.exe"
            ),
            Some(programs.join("PhpStorm/bin/phpstorm64.exe"))
        );
        let elsewhere = dir.path().join("E/IDEs/CLion 2025.1");
        touch(&elsewhere.join("bin/clion64.exe"));
        assert_eq!(
            find_jetbrains(
                &[],
                std::slice::from_ref(&elsewhere),
                "CLion",
                "clion64.exe"
            ),
            Some(elsewhere.join("bin/clion64.exe"))
        );
    }

    #[test]
    fn visual_studio_newest_version_wins() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let vs = root.join("Microsoft Visual Studio");
        touch(&vs.join("2019/Community/Common7/IDE/devenv.exe"));
        touch(&vs.join("2022/Professional/Common7/IDE/devenv.exe"));
        touch(&vs.join("Installer/vswhere.exe"));
        assert_eq!(
            find_visual_studio(std::slice::from_ref(&root), &[]),
            Some(vs.join("2022/Professional/Common7/IDE/devenv.exe"))
        );
    }

    #[cfg(windows)]
    #[test]
    fn registry_dir_reads_install_location_and_display_icon() {
        assert_eq!(
            registry_dir(r"C:\Tools\Zed\", false),
            Some(PathBuf::from(r"C:\Tools\Zed\"))
        );
        assert_eq!(
            registry_dir(r#""D:\Apps\Kiro\Kiro.exe",0"#, true),
            Some(PathBuf::from(r"D:\Apps\Kiro"))
        );
        assert_eq!(
            registry_dir(r"D:\Apps\Kiro\Kiro.exe", true),
            Some(PathBuf::from(r"D:\Apps\Kiro"))
        );
        assert_eq!(registry_dir("", false), None);
        assert_eq!(registry_dir("relative\\dir", false), None);
    }

    #[test]
    fn notepad_is_not_offered() {
        assert!(detect().iter().all(|e| e.id != "notepad"));
    }
}
