//! Code editors the user can open a site or file in (§94–99). VS Code is the default;
//! any other known editor found on this machine can be picked in Settings, or a custom
//! executable path given instead.

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
    // Backward compatibility: if no custom editors yet, but `editor.command` exists:
    if let Some(cmd) = settings
        .get("editor.command")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let name = Path::new(cmd)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Custom Editor")
            .to_string();
        return vec![CustomEditor {
            id: "custom:default".to_string(),
            name,
            path: cmd.to_string(),
            args: None,
        }];
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

/// (id, display name, exe name, install folders relative to the usual roots, PATH shim).
const KNOWN: &[(&str, &str, &str, &[&str], &str)] = &[
    (
        "vscode",
        "VS Code",
        "Code.exe",
        &["Microsoft VS Code"],
        "code",
    ),
    (
        "cursor",
        "Cursor",
        "Cursor.exe",
        &["cursor", "Cursor"],
        "cursor",
    ),
    (
        "windsurf",
        "Windsurf",
        "Windsurf.exe",
        &["Windsurf", "windsurf"],
        "windsurf",
    ),
    ("trae", "Trae", "Trae.exe", &["Trae", "trae"], "trae"),
    ("zed", "Zed", "Zed.exe", &["Zed", "zed"], "zed"),
    (
        "vscodium",
        "VSCodium",
        "VSCodium.exe",
        &["VSCodium"],
        "codium",
    ),
    (
        "sublime",
        "Sublime Text",
        "sublime_text.exe",
        &["Sublime Text", "Sublime Text 3"],
        "subl",
    ),
    (
        "notepadpp",
        "Notepad++",
        "notepad++.exe",
        &["Notepad++"],
        "notepad++",
    ),
];

const JETBRAINS: &[(&str, &str, &str, &str)] = &[
    ("phpstorm", "PhpStorm", "PhpStorm", "phpstorm64.exe"),
    ("webstorm", "WebStorm", "WebStorm", "webstorm64.exe"),
    ("pycharm", "PyCharm", "PyCharm", "pycharm64.exe"),
    ("intellij", "IntelliJ IDEA", "IntelliJ IDEA", "idea64.exe"),
    ("goland", "GoLand", "GoLand", "goland64.exe"),
    ("rustrover", "RustRover", "RustRover", "rustrover64.exe"),
    ("rider", "Rider", "Rider", "rider64.exe"),
];

fn find_jetbrains(roots: &[PathBuf], product_prefix: &str, exe_name: &str) -> Option<PathBuf> {
    for r in roots {
        let jb = r.join("JetBrains");
        if let Ok(entries) = std::fs::read_dir(&jb) {
            for entry in entries.filter_map(Result::ok) {
                let name = entry.file_name().to_string_lossy().to_string();
                if name
                    .to_lowercase()
                    .starts_with(&product_prefix.to_lowercase())
                {
                    let bin_exe = entry.path().join("bin").join(exe_name);
                    if bin_exe.is_file() {
                        return Some(bin_exe);
                    }
                }
            }
        }
        let tb = r
            .join("JetBrains")
            .join("Toolbox")
            .join("apps")
            .join(product_prefix);
        if let Ok(channels) = std::fs::read_dir(&tb) {
            for ch in channels.filter_map(Result::ok) {
                if let Ok(builds) = std::fs::read_dir(ch.path()) {
                    for build in builds.filter_map(Result::ok) {
                        let bin_exe = build.path().join("bin").join(exe_name);
                        if bin_exe.is_file() {
                            return Some(bin_exe);
                        }
                    }
                }
            }
        }
    }
    None
}

/// Every known editor, with where it's installed (if it is).
pub fn detect() -> Vec<EditorInfo> {
    let roots: Vec<PathBuf> = ["LOCALAPPDATA", "ProgramFiles", "ProgramFiles(x86)"]
        .iter()
        .filter_map(|v| std::env::var_os(v).map(PathBuf::from))
        .flat_map(|r| [r.join("Programs"), r])
        .collect();

    let mut out: Vec<EditorInfo> = KNOWN
        .iter()
        .map(|(id, name, exe, folders, shim)| {
            let installed = roots
                .iter()
                .flat_map(|r| folders.iter().map(move |f| r.join(f).join(exe)))
                .find(|p| p.is_file());
            let path = installed.or_else(|| crate::web::manager::find_executable(None, shim));
            EditorInfo {
                id: id.to_string(),
                name: name.to_string(),
                path: path.map(|p| p.display().to_string()),
                custom: false,
                hidden: false,
            }
        })
        .collect();

    for (id, name, prefix, exe) in JETBRAINS {
        let path = find_jetbrains(&roots, prefix, exe)
            .or_else(|| crate::web::manager::find_executable(None, id));
        out.push(EditorInfo {
            id: id.to_string(),
            name: name.to_string(),
            path: path.map(|p| p.display().to_string()),
            custom: false,
            hidden: false,
        });
    }

    out
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
    fn antigravity_is_not_offered() {
        assert!(detect().iter().all(|e| e.id != "antigravity"));
    }

    #[test]
    fn notepad_is_not_offered() {
        assert!(detect().iter().all(|e| e.id != "notepad"));
    }
}
