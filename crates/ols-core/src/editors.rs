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

/// Every known editor, with where it's installed (if it is).
pub fn detect() -> Vec<EditorInfo> {
    let roots: Vec<PathBuf> = ["LOCALAPPDATA", "ProgramFiles", "ProgramFiles(x86)"]
        .iter()
        .filter_map(|v| std::env::var_os(v).map(PathBuf::from))
        .flat_map(|r| [r.join("Programs"), r])
        .collect();
    KNOWN
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
            }
        })
        .collect()
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
}
