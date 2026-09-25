//! Per-project tooling actions (Stage 11): Composer, Node package managers, Python
//! venvs and the Xdebug IDE setup. Each one is a thin step over the pure helpers in
//! `composer.rs`, `nodepm.rs`, `venv.rs` and `xdebug.rs`, run through the same
//! supervised-process path as any other project command so output streams live.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::app::Inner;
use crate::composer::{self, ComposerInfo};
use crate::error::CoreError;
use crate::nodepm::{self, PackageManagerInfo};
use crate::process::ProcessId;
use crate::project::Project;
use crate::venv::{self, VenvInfo};
use crate::xdebug::{self, XdebugReport};

fn err(msg: impl Into<String>) -> CoreError {
    CoreError::ServiceError(msg.into())
}

impl Inner {
    fn project(&self, id: &str) -> Result<Project, CoreError> {
        self.projects.lock().unwrap().get(id).ok_or_else(|| CoreError::InvalidProjectPath(id.to_string()))
    }

    // -------------------------------------------------------------- composer (§14)

    pub fn composer_info(&self, project_id: &str) -> Result<ComposerInfo, CoreError> {
        Ok(composer::read(Path::new(&self.project(project_id)?.path)))
    }

    /// Runs one of the named Composer actions in the project. Returns the process to follow.
    pub fn run_composer(&self, project_id: &str, action: &str, target: Option<&str>) -> Result<ProcessId, CoreError> {
        let project = self.project(project_id)?;
        if !Path::new(&project.path).join("composer.json").is_file() && action != "diagnose" {
            return Err(err("This project has no composer.json."));
        }
        let args = composer::command_args(action, target).map_err(err)?;
        let line = format!("composer {}", args.join(" "));
        self.spawn_project_process("composer", &args, None, Some(project_id), &format!("{}: {line}", project.name), &line)
    }

    // ------------------------------------------------------ package managers (§15)

    pub fn package_managers(&self, project_id: &str) -> Result<PackageManagerInfo, CoreError> {
        let project = self.project(project_id)?;
        let node_dir = self.runtime_bin_for(Some(project_id), "node");
        Ok(nodepm::info(Path::new(&project.path), node_dir.as_deref()))
    }

    /// Switches on pnpm or yarn: `corepack enable` (writes the shims, quick) and then
    /// `corepack prepare ... --activate` as a followable process, since it downloads.
    pub fn enable_package_manager(&self, project_id: &str, manager: &str) -> Result<ProcessId, CoreError> {
        let project = self.project(project_id)?;
        let enable = nodepm::enable_args(manager).map_err(err)?;
        let node_dir = self.runtime_bin_for(Some(project_id), "node").ok_or_else(|| err("Node.js is not installed. Install it from the Runtimes page."))?;
        let corepack = crate::web::manager::find_executable(Some(&node_dir), "corepack")
            .filter(|p| p.starts_with(&node_dir))
            .ok_or_else(|| err("This Node.js has no corepack. Node 25 and newer dropped it; use the bundled Node 24, or run `npm install -g pnpm`."))?;

        let system_path = std::env::var("PATH").unwrap_or_default();
        let env = vec![("PATH".to_string(), format!("{};{system_path}", node_dir.display()))];
        let mut args = vec!["/C".to_string(), corepack.display().to_string()];
        args.extend(enable);
        let out = crate::exec::run_capture(Path::new("cmd.exe"), &args, Some(Path::new(&project.path)), &env, Duration::from_secs(60));
        if !out.success() {
            return Err(err(format!("corepack enable {manager} failed: {}", out.combined())));
        }

        let (detected, _, pinned) = nodepm::detect(Path::new(&project.path));
        let pinned = pinned.filter(|_| detected.as_deref() == Some(manager));
        let prepare = nodepm::prepare_args(manager, pinned.as_deref()).map_err(err)?;
        let line = format!("corepack {}", prepare.join(" "));
        self.spawn_project_process("corepack", &prepare, None, Some(project_id), &format!("{}: {line}", project.name), &line)
    }

    // ------------------------------------------------------------ python venv (§17)

    pub fn venv_info(&self, project_id: &str) -> Result<VenvInfo, CoreError> {
        Ok(venv::detect(Path::new(&self.project(project_id)?.path)))
    }

    /// Creates the project's `.venv`, first deleting the existing venv when `recreate`.
    pub fn create_venv(&self, project_id: &str, recreate: bool) -> Result<ProcessId, CoreError> {
        let project = self.project(project_id)?;
        let root = Path::new(&project.path);
        let existing = venv::find_dir(root);
        if existing.is_some() && !recreate {
            return Err(err("This project already has a virtual environment. Use Recreate to rebuild it."));
        }
        let name = if recreate { venv::remove(root).map_err(err)? } else { None };
        let dir_name = name.unwrap_or_else(|| ".venv".to_string());
        // The system Python: `python` resolves outside any venv, since none exists at this moment.
        let args = venv::create_args(&dir_name);
        let line = format!("python {}", args.join(" "));
        self.spawn_project_process("python", &args, None, Some(project_id), &format!("{}: {line}", project.name), &line)
    }

    /// `pip install` into the venv from a requirements file, or the project's pyproject.
    pub fn install_venv_requirements(&self, project_id: &str, what: &str) -> Result<ProcessId, CoreError> {
        let project = self.project(project_id)?;
        let root = Path::new(&project.path);
        let dir = venv::find_dir(root).ok_or_else(|| err("Create the virtual environment first."))?;
        let python = venv::python_exe(&dir);
        if !python.is_file() {
            return Err(err("The virtual environment's Python is missing. Recreate it."));
        }
        let args = venv::install_args(root, what).map_err(err)?;
        let line = format!("{}\\Scripts\\python.exe {}", dir.file_name().and_then(|n| n.to_str()).unwrap_or(".venv"), args.join(" "));
        self.spawn_project_process(&python.display().to_string(), &args, None, Some(project_id), &format!("{}: pip install ({what})", project.name), &line)
    }

    /// The venv's Python and Scripts folder when the project has one — what `python` and
    /// `pip` resolve to for that project's commands.
    pub(crate) fn project_venv(&self, project_id: Option<&str>) -> Option<(PathBuf, PathBuf)> {
        let project = self.projects.lock().unwrap().get(project_id?)?;
        let dir = venv::find_dir(Path::new(&project.path))?;
        venv::python_exe(&dir).is_file().then(|| (dir.clone(), venv::scripts_dir(&dir)))
    }

    // ---------------------------------------------------------------- xdebug (§13)

    pub fn xdebug_ide_config(&self, project_id: &str, ide: &str, version: &str) -> Result<String, CoreError> {
        let project = self.project(project_id)?;
        let site_root = self
            .domains
            .lock()
            .unwrap()
            .list()
            .into_iter()
            .find(|d| d.project_id.as_deref() == Some(project_id))
            .map(|d| d.root)
            .unwrap_or_else(|| project.path.clone());
        let settings = self.php.xdebug_settings(version);
        Ok(xdebug::ide_config(ide, &project.path, &site_root, &settings))
    }

    pub fn xdebug_report(&self, version: &str) -> XdebugReport {
        self.php.xdebug_report(version)
    }
}

// ------------------------------------------------------------------ .env editor (§103)

use crate::envfile::{self, EnvDiffRow, EnvFileInfo, EnvFileView};

/// Older versions of an env file kept before each save, per project.
const ENV_BACKUPS_KEPT: usize = 20;

impl Inner {
    fn env_path(&self, project_id: &str, file: &str) -> Result<PathBuf, CoreError> {
        let project = self.project(project_id)?;
        envfile::path_of(Path::new(&project.path), file).map_err(err)
    }

    fn env_content(&self, project_id: &str, file: &str) -> Result<String, CoreError> {
        let path = self.env_path(project_id, file)?;
        match std::fs::read_to_string(&path) {
            Ok(c) => Ok(c),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
            Err(e) => Err(err(format!("could not read {}: {e}", path.display()))),
        }
    }

    pub fn env_files(&self, project_id: &str) -> Result<Vec<EnvFileInfo>, CoreError> {
        Ok(envfile::list_files(Path::new(&self.project(project_id)?.path)))
    }

    pub fn env_read(&self, project_id: &str, file: &str) -> Result<EnvFileView, CoreError> {
        let path = self.env_path(project_id, file)?;
        if !path.is_file() {
            return Err(err(format!("{file} does not exist in this project.")));
        }
        Ok(envfile::view(file, &self.env_content(project_id, file)?))
    }

    /// Validates, keeps a copy of what was there, then writes. Errors in the text (not
    /// warnings) stop the save so a broken file never replaces a working one.
    pub fn env_write(&self, project_id: &str, file: &str, content: &str) -> Result<EnvFileView, CoreError> {
        let path = self.env_path(project_id, file)?;
        if let Some(problem) = envfile::validate(content).into_iter().find(|i| i.severity == "error") {
            return Err(err(format!("Line {}: {}", problem.line, problem.message)));
        }
        if let Ok(previous) = std::fs::read_to_string(&path) {
            if previous != content {
                self.env_backup(project_id, file, &previous);
            }
        }
        std::fs::write(&path, content).map_err(|e| err(format!("could not write {}: {e}", path.display())))?;
        Ok(envfile::view(file, content))
    }

    fn env_backup(&self, project_id: &str, file: &str, previous: &str) {
        let dir = self.paths.data_dir().join("env_backups").join(project_id);
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        let _ = std::fs::write(dir.join(format!("{file}.{stamp}")), previous);
        // Keep only the newest few.
        let mut all: Vec<PathBuf> = std::fs::read_dir(&dir).into_iter().flatten().flatten().map(|e| e.path()).collect();
        all.sort();
        while all.len() > ENV_BACKUPS_KEPT {
            let _ = std::fs::remove_file(all.remove(0));
        }
    }

    pub fn env_set(&self, project_id: &str, file: &str, key: &str, value: &str) -> Result<EnvFileView, CoreError> {
        let updated = envfile::set(&self.env_content(project_id, file)?, key, value).map_err(err)?;
        self.env_write(project_id, file, &updated)
    }

    pub fn env_delete(&self, project_id: &str, file: &str, key: &str) -> Result<EnvFileView, CoreError> {
        let updated = envfile::remove(&self.env_content(project_id, file)?, key);
        self.env_write(project_id, file, &updated)
    }

    pub fn env_compare(&self, project_id: &str, a: &str, b: &str) -> Result<Vec<EnvDiffRow>, CoreError> {
        Ok(envfile::compare(&self.env_content(project_id, a)?, &self.env_content(project_id, b)?))
    }

    /// Brings variables in from any file on disk: `merge` keeps this file's other keys.
    pub fn env_import(&self, project_id: &str, file: &str, source: &str, mode: &str) -> Result<EnvFileView, CoreError> {
        let incoming = std::fs::read_to_string(source).map_err(|e| err(format!("could not read {source}: {e}")))?;
        let merged = envfile::import(&self.env_content(project_id, file)?, &incoming, mode).map_err(err)?;
        self.env_write(project_id, file, &merged)
    }

    pub fn env_export(&self, project_id: &str, file: &str, dest: &str) -> Result<(), CoreError> {
        std::fs::write(dest, self.env_content(project_id, file)?).map_err(|e| err(format!("could not write {dest}: {e}")))
    }

    /// A new env file, empty or copied from another one (typically `.env` from `.env.example`).
    pub fn env_create(&self, project_id: &str, file: &str, from: Option<&str>) -> Result<EnvFileView, CoreError> {
        let path = self.env_path(project_id, file)?;
        if path.exists() {
            return Err(err(format!("{file} already exists.")));
        }
        let content = match from {
            Some(src) => {
                let source = self.env_path(project_id, src)?;
                std::fs::read_to_string(&source).map_err(|e| err(format!("could not read {src}: {e}")))?
            }
            None => String::new(),
        };
        self.env_write(project_id, file, &content)
    }
}
