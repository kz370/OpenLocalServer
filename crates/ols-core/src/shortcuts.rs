//! "Open with" and file shortcuts (§97, §100): the places a developer keeps jumping to in a
//! project (its folder, `public/`, `config/`, `.env`, logs, the site's server config) and the
//! ways to open any of them (code editor, Explorer, a terminal, the default app).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::app::Inner;
use crate::error::CoreError;

fn err(msg: impl Into<String>) -> CoreError {
    CoreError::ServiceError(msg.into())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shortcut {
    pub id: String,
    pub label: String,
    pub path: String,
    pub is_dir: bool,
}

/// The first of `candidates` (relative to `root`) that exists.
fn first_existing(root: &Path, candidates: &[&str]) -> Option<PathBuf> {
    candidates.iter().map(|c| root.join(c)).find(|p| p.exists())
}

/// Shortcuts to what exists in the project. `doc_root` is the served sub-folder the
/// detection found (`public` for Laravel-style layouts). Anything missing is left out.
pub fn project_shortcuts(root: &Path, doc_root: Option<&str>) -> Vec<Shortcut> {
    let mut out = Vec::new();
    let mut add = |id: &str, label: &str, path: Option<PathBuf>| {
        if let Some(p) = path {
            out.push(Shortcut { id: id.into(), label: label.into(), is_dir: p.is_dir(), path: p.display().to_string() });
        }
    };
    add("project", "Project folder", Some(root.to_path_buf()));

    let served = doc_root.filter(|d| !d.is_empty() && *d != ".").map(|d| root.join(d)).filter(|p| p.is_dir());
    add("public", "Public folder", served.or_else(|| first_existing(root, &["public", "web", "public_html", "wwwroot", "dist"]).filter(|p| p.is_dir())));
    add("config", "Config folder", first_existing(root, &["config"]).filter(|p| p.is_dir()));
    add("env", ".env", first_existing(root, &[".env"]));
    add("logs", "Logs", first_existing(root, &["storage/logs", "var/log", "logs", "wp-content/debug.log", "log"]));
    out
}

/// Files that are program code or scripts: opening them with the default app would run them.
pub fn is_runnable(path: &Path) -> bool {
    const RUNNABLE: &[&str] = &["exe", "com", "bat", "cmd", "ps1", "msi", "scr", "vbs", "vbe", "js", "jse", "wsf", "wsh", "lnk", "hta", "cpl", "reg", "jar"];
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| RUNNABLE.iter().any(|r| r.eq_ignore_ascii_case(e)))
}

impl Inner {
    /// The project's usual places, plus the server config of each site pointing at it.
    pub fn project_shortcuts(&self, project_id: &str) -> Result<Vec<Shortcut>, CoreError> {
        let detail = self.project_detail(project_id).ok_or_else(|| CoreError::InvalidProjectPath(project_id.to_string()))?;
        let mut shortcuts = project_shortcuts(Path::new(&detail.project.path), detail.detection.doc_root.as_deref());

        let domains = self.domains.lock().unwrap();
        let hostnames: Vec<String> = domains.list().into_iter().filter(|d| d.project_id.as_deref() == Some(project_id)).map(|d| d.hostname).collect();
        if !hostnames.is_empty() {
            if let Ok(files) = self.web.list_configs(&self.web_config(), &domains) {
                for f in files.into_iter().filter(|f| f.hostname.as_ref().is_some_and(|h| hostnames.contains(h))) {
                    if Path::new(&f.path).is_file() {
                        let host = f.hostname.unwrap_or_default();
                        shortcuts.push(Shortcut { id: format!("server:{host}"), label: format!("Server config · {host}"), path: f.path, is_dir: false });
                    }
                }
            }
        }
        Ok(shortcuts)
    }

    /// Opens `path` with `app`: `editor` (the one chosen in Settings), an editor id such as
    /// `vscode`, `explorer`, `terminal` or `default` (the file's registered app).
    pub fn open_with(&self, path: &str, app: &str) -> Result<(), CoreError> {
        let p = Path::new(path);
        if !p.exists() {
            return Err(err(format!("{path} does not exist")));
        }
        match app {
            "editor" => self.open_in_editor(path),
            "explorer" => {
                let mut cmd = std::process::Command::new("explorer.exe");
                if p.is_file() {
                    // One argument, or paths with spaces or commas would be split.
                    cmd.arg(format!("/select,{path}"));
                } else {
                    cmd.arg(p);
                }
                cmd.spawn().map(|_| ()).map_err(|e| err(e.to_string()))
            }
            "terminal" => {
                let dir = if p.is_dir() { p.to_path_buf() } else { p.parent().map(Path::to_path_buf).unwrap_or_else(|| p.to_path_buf()) };
                let mut wt = std::process::Command::new("wt.exe");
                wt.arg("-d").arg(&dir);
                if wt.spawn().is_ok() {
                    return Ok(());
                }
                // No Windows Terminal: a plain PowerShell window in that folder.
                std::process::Command::new("powershell.exe")
                    .arg("-NoExit")
                    .current_dir(&dir)
                    .spawn()
                    .map(|_| ())
                    .map_err(|e| err(format!("could not open a terminal: {e}")))
            }
            "default" => {
                if p.is_file() && is_runnable(p) {
                    return Err(err("That file is a program or script, so it is not opened with the default app. Open it in your editor instead."));
                }
                let mut cmd = std::process::Command::new("cmd.exe");
                cmd.args(["/C", "start", ""]).arg(p);
                crate::exec::hide_window(&mut cmd);
                cmd.spawn().map(|_| ()).map_err(|e| err(e.to_string()))
            }
            editor_id => {
                let found = crate::editors::detect().into_iter().find(|e| e.id == editor_id).ok_or_else(|| err(format!("unknown app: {editor_id}")))?;
                let exe = found.path.ok_or_else(|| err(format!("{} is not installed", found.name)))?;
                self.launch_editor(Path::new(&exe), path)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(shortcuts: &[Shortcut]) -> Vec<&str> {
        shortcuts.iter().map(|s| s.id.as_str()).collect()
    }

    #[test]
    fn laravel_layout_gets_every_shortcut() {
        let dir = tempfile::tempdir().unwrap();
        for d in ["public", "config", "storage/logs"] {
            std::fs::create_dir_all(dir.path().join(d)).unwrap();
        }
        std::fs::write(dir.path().join(".env"), "A=1").unwrap();
        let s = project_shortcuts(dir.path(), Some("public"));
        assert_eq!(labels(&s), vec!["project", "public", "config", "env", "logs"]);
        assert!(s.iter().find(|x| x.id == "env").is_some_and(|x| !x.is_dir));
        assert!(s.iter().find(|x| x.id == "logs").is_some_and(|x| x.is_dir && x.path.ends_with("logs")));
    }

    #[test]
    fn missing_places_are_left_out_and_the_doc_root_wins() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("web")).unwrap();
        std::fs::create_dir_all(dir.path().join("public")).unwrap();
        let s = project_shortcuts(dir.path(), Some("web"));
        assert_eq!(labels(&s), vec!["project", "public"]);
        assert!(s[1].path.ends_with("web"), "the served folder is the one shown: {}", s[1].path);
        // A doc root that isn't there falls back to the usual names.
        let s = project_shortcuts(dir.path(), Some("nope"));
        assert!(s[1].path.ends_with("web") || s[1].path.ends_with("public"));
    }

    #[test]
    fn programs_and_scripts_are_never_opened_with_the_default_app() {
        for name in ["a.exe", "b.BAT", "c.ps1", "d.lnk", "e.js", "f.jar"] {
            assert!(is_runnable(Path::new(name)), "{name}");
        }
        for name in ["readme.md", "app.log", "index.php", "data.json", "noextension"] {
            assert!(!is_runnable(Path::new(name)), "{name}");
        }
    }
}
