//! Python virtual environments (§17, Stage 11): find a project's venv, and build the
//! commands that create, recreate and fill one. Python itself is not managed yet, so
//! `python -m venv` runs with the Python already on this PC (or a custom pin).
//!
//! "Activating" a venv here means project commands use it: `python` and `pip` run from
//! the venv's `Scripts` folder with `VIRTUAL_ENV` set (see `Inner::quick_resolve_program`),
//! so nothing has to be activated by hand in a shell.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Folder names checked for an existing venv, in order. New ones are created as `.venv`.
const NAMES: &[&str] = &[".venv", "venv", "env"];
/// Files `pip install -r` can read, in the order they're offered.
const REQUIREMENT_FILES: &[&str] = &[
    "requirements.txt",
    "requirements-dev.txt",
    "requirements/base.txt",
    "requirements/dev.txt",
];

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VenvInfo {
    pub exists: bool,
    /// Folder name inside the project, e.g. ".venv".
    pub dir_name: Option<String>,
    /// Python version the venv was made with, from pyvenv.cfg.
    pub python_version: Option<String>,
    /// The base interpreter's folder, from pyvenv.cfg. When that folder is gone the venv is broken.
    pub base_home: Option<String>,
    pub base_missing: bool,
    /// Requirement files found in the project.
    pub requirements: Vec<String>,
    pub has_pyproject: bool,
    /// What to type in a terminal to activate it by hand.
    pub activate_command: Option<String>,
}

/// The venv folder inside `project`, if there is one (a folder with `pyvenv.cfg`).
pub fn find_dir(project: &Path) -> Option<PathBuf> {
    NAMES
        .iter()
        .map(|n| project.join(n))
        .find(|d| d.join("pyvenv.cfg").is_file())
}

pub fn python_exe(venv_dir: &Path) -> PathBuf {
    venv_dir.join("Scripts").join("python.exe")
}

pub fn scripts_dir(venv_dir: &Path) -> PathBuf {
    venv_dir.join("Scripts")
}

/// `key = value` lines of pyvenv.cfg.
fn config_value(cfg: &str, key: &str) -> Option<String> {
    cfg.lines().find_map(|line| {
        let (k, v) = line.split_once('=')?;
        (k.trim().eq_ignore_ascii_case(key)).then(|| v.trim().to_string())
    })
}

pub fn detect(project: &Path) -> VenvInfo {
    let requirements: Vec<String> = REQUIREMENT_FILES
        .iter()
        .filter(|f| project.join(f).is_file())
        .map(|f| f.to_string())
        .collect();
    let has_pyproject = project.join("pyproject.toml").is_file();
    let Some(dir) = find_dir(project) else {
        return VenvInfo {
            requirements,
            has_pyproject,
            ..Default::default()
        };
    };
    let cfg = std::fs::read_to_string(dir.join("pyvenv.cfg")).unwrap_or_default();
    let base_home = config_value(&cfg, "home");
    let dir_name = dir.file_name().and_then(|n| n.to_str()).map(str::to_string);
    VenvInfo {
        exists: true,
        python_version: config_value(&cfg, "version")
            .or_else(|| config_value(&cfg, "version_info")),
        base_missing: base_home.as_deref().is_some_and(|h| !Path::new(h).is_dir())
            || !python_exe(&dir).is_file(),
        base_home,
        activate_command: dir_name.as_ref().map(|n| format!("{n}\\Scripts\\activate")),
        dir_name,
        requirements,
        has_pyproject,
    }
}

pub fn create_args(dir_name: &str) -> Vec<String> {
    vec!["-m".into(), "venv".into(), dir_name.into()]
}

/// Deleting is only ever done to a folder that `find_dir` recognised as a venv, so a
/// wrong path can't wipe project files.
pub fn remove(project: &Path) -> Result<Option<String>, String> {
    let Some(dir) = find_dir(project) else {
        return Ok(None);
    };
    let name = dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(".venv")
        .to_string();
    std::fs::remove_dir_all(&dir).map_err(|e| {
        format!(
            "could not remove {}: {e} (close anything using the venv and retry)",
            dir.display()
        )
    })?;
    Ok(Some(name))
}

/// The `pip install` arguments for `what`: one of the requirement files found in the
/// project, or "pyproject" for an editable install of the project itself.
pub fn install_args(project: &Path, what: &str) -> Result<Vec<String>, String> {
    let mut args: Vec<String> = vec!["-m".into(), "pip".into(), "install".into()];
    if what == "pyproject" {
        if !project.join("pyproject.toml").is_file() {
            return Err("This project has no pyproject.toml.".into());
        }
        args.extend(["-e".into(), ".".into()]);
    } else if REQUIREMENT_FILES.contains(&what) && project.join(what).is_file() {
        args.extend(["-r".into(), what.into()]);
    } else {
        return Err(format!(
            "\"{what}\" is not a requirements file in this project."
        ));
    }
    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_venv(project: &Path, name: &str, home: &str) {
        let dir = project.join(name);
        std::fs::create_dir_all(dir.join("Scripts")).unwrap();
        std::fs::write(
            dir.join("pyvenv.cfg"),
            format!("home = {home}\nversion = 3.12.4\n"),
        )
        .unwrap();
    }

    #[test]
    fn finds_the_venv_and_reads_its_python_version() {
        let p = tempfile::tempdir().unwrap();
        std::fs::write(p.path().join("requirements.txt"), "flask\n").unwrap();
        make_venv(p.path(), "venv", &p.path().display().to_string());
        std::fs::write(python_exe(&p.path().join("venv")), "").unwrap();
        let info = detect(p.path());
        assert!(info.exists);
        assert_eq!(info.dir_name.as_deref(), Some("venv"));
        assert_eq!(info.python_version.as_deref(), Some("3.12.4"));
        assert_eq!(info.requirements, ["requirements.txt"]);
        assert!(!info.base_missing);
        assert_eq!(
            info.activate_command.as_deref(),
            Some("venv\\Scripts\\activate")
        );
    }

    #[test]
    fn a_venv_whose_base_python_is_gone_is_reported_broken() {
        let p = tempfile::tempdir().unwrap();
        make_venv(p.path(), ".venv", r"C:\definitely\not\here");
        assert!(detect(p.path()).base_missing);
    }

    #[test]
    fn a_folder_without_pyvenv_cfg_is_not_a_venv_and_is_never_removed() {
        let p = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(p.path().join("env")).unwrap();
        std::fs::write(p.path().join("env").join("keep.txt"), "x").unwrap();
        assert!(!detect(p.path()).exists);
        assert_eq!(remove(p.path()).unwrap(), None);
        assert!(p.path().join("env").join("keep.txt").is_file());
    }

    #[test]
    fn remove_deletes_only_the_venv() {
        let p = tempfile::tempdir().unwrap();
        make_venv(p.path(), ".venv", "C:\\x");
        std::fs::write(p.path().join("app.py"), "").unwrap();
        assert_eq!(remove(p.path()).unwrap().as_deref(), Some(".venv"));
        assert!(!p.path().join(".venv").exists());
        assert!(p.path().join("app.py").is_file());
    }

    #[test]
    fn install_arguments_accept_only_known_requirement_files() {
        let p = tempfile::tempdir().unwrap();
        std::fs::write(p.path().join("requirements.txt"), "").unwrap();
        assert_eq!(
            install_args(p.path(), "requirements.txt").unwrap(),
            ["-m", "pip", "install", "-r", "requirements.txt"]
        );
        assert!(install_args(p.path(), "..\\evil.txt").is_err());
        assert!(
            install_args(p.path(), "requirements-dev.txt").is_err(),
            "listed but missing"
        );
        assert!(install_args(p.path(), "pyproject").is_err());
        std::fs::write(p.path().join("pyproject.toml"), "").unwrap();
        assert_eq!(
            install_args(p.path(), "pyproject").unwrap(),
            ["-m", "pip", "install", "-e", "."]
        );
    }
}
