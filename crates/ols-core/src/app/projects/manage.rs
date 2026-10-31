use notify::RecursiveMode;
use notify_debouncer_mini::new_debouncer;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;

use crate::app::*;
use crate::error::CoreError;
use crate::project::{build_detail, Project, ProjectDetail};

impl Inner {
    pub fn subscribe_project_events(&self) -> broadcast::Receiver<()> {
        self.project_events.subscribe()
    }

    pub(crate) fn start_projects_watcher(self: &Arc<Self>) {
        if !self.setting_bool("projects.watch", true) {
            return;
        }
        let mut roots = self.string_list(PROJECT_ROOTS);
        let default_root = self.paths.sites_dir().display().to_string();
        if !roots
            .iter()
            .any(|root| root.eq_ignore_ascii_case(&default_root))
        {
            roots.push(default_root);
        }
        let core = Arc::clone(self);
        std::thread::spawn(move || {
            let (events_tx, events_rx) = std::sync::mpsc::channel();
            let Ok(mut debouncer) =
                new_debouncer(std::time::Duration::from_millis(1500), events_tx)
            else {
                return;
            };
            let mut watched = std::collections::HashSet::new();
            for root in roots {
                let path = PathBuf::from(root);
                if path.is_dir() {
                    if debouncer
                        .watcher()
                        .watch(&path, RecursiveMode::NonRecursive)
                        .is_ok()
                    {
                        watched.insert(path);
                    }
                }
            }
            let mut tick_count: u32 = 0;
            loop {
                match events_rx.recv_timeout(Duration::from_millis(500)) {
                    Ok(_) => {
                        let _ = lock(&core.projects).sweep_health();
                        if let Err(error) = core.sync_auto_domains() {
                            tracing::warn!(%error, "automatic domains could not be synced after a project change");
                        }
                        let _ = core.project_events.send(());
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        tick_count = tick_count.wrapping_add(1);
                        // Every ~3 seconds (6 * 500ms), sweep health to detect unmounted drives / external deletions
                        if tick_count % 6 == 0 {
                            let health_changed = lock(&core.projects).sweep_health();
                            if health_changed {
                                let _ = core.project_events.send(());
                            }
                        }
                    }
                }
                let desired: std::collections::HashSet<PathBuf> = core
                    .string_list(PROJECT_ROOTS)
                    .into_iter()
                    .map(PathBuf::from)
                    .collect();
                for path in watched.clone() {
                    if !desired.contains(&path) {
                        let _ = debouncer.watcher().unwatch(&path);
                        watched.remove(&path);
                    }
                }
                for path in desired {
                    if path.is_dir()
                        && !watched.contains(&path)
                        && debouncer
                            .watcher()
                            .watch(&path, RecursiveMode::NonRecursive)
                            .is_ok()
                    {
                        watched.insert(path);
                    }
                }
            }
        });
    }

    pub fn project_detail(&self, id: &str) -> Option<ProjectDetail> {
        let project = lock(&self.projects).get(id)?;
        let settings = lock(&self.settings).clone();
        let custom = lock(&self.custom_installs).clone();
        Some(build_detail(&project, &self.runtimes, &settings, &custom))
    }

    /// Verifies that a registered project exists in the store and that its directory is available on disk.
    pub fn ensure_project_available(&self, project_id: &str) -> Result<Project, CoreError> {
        let mut project = lock(&self.projects).get(project_id).ok_or_else(|| {
            CoreError::failed(
                "Project not found",
                format!("{project_id} is not a registered project"),
            )
        })?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let next_health =
            crate::project::evaluate_health(Path::new(&project.path), &project.health, now);
        if next_health != project.health {
            project.health = next_health;
            lock(&self.projects).update_health(project_id, project.health.clone());
            let _ = self.project_events.send(());
        }

        crate::project::ensure_project_available(&project)?;
        Ok(project)
    }

    pub fn project_by_path(&self, path: &str) -> Option<Project> {
        let norm = |p: &str| {
            p.replace('/', "\\")
                .trim_end_matches('\\')
                .to_ascii_lowercase()
        };
        self.projects
            .lock()
            .unwrap()
            .list()
            .into_iter()
            .find(|p| norm(&p.path) == norm(path))
    }

    /// The runtime's bin dir a project resolves to (custom pin, then managed), else the
    /// newest managed install.
    pub(crate) fn runtime_bin_for(&self, project_id: Option<&str>, id: &str) -> Option<PathBuf> {
        let (runtime_id, ver_pin) = id
            .split_once(':')
            .map(|(r, v)| (r, Some(v)))
            .unwrap_or((id, None));
        if let Some(ver) = ver_pin {
            return self.runtimes.bin_dir(runtime_id, ver);
        }
        if let Some(pid) = project_id {
            if let Some(detail) = self.project_detail(pid) {
                if let Some(dir) = detail
                    .resolved
                    .iter()
                    .find(|r| r.id == runtime_id)
                    .and_then(|r| r.bin_dir.clone())
                {
                    return Some(PathBuf::from(dir));
                }
            }
        }
        let versions = self.runtimes.installed_versions(runtime_id);
        let newest = crate::php::pick_version(&versions, None)?;
        self.runtimes.bin_dir(runtime_id, &newest)
    }

    // -------------------------------------------------------------------- domains

    pub(crate) fn string_list(&self, key: &str) -> Vec<String> {
        let s = self.settings.lock().unwrap();
        let mut roots: Vec<String> = s
            .get(key)
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        if key == PROJECT_ROOTS {
            let default_root = self.paths.sites_dir().display().to_string();
            if !roots
                .iter()
                .any(|root| root.eq_ignore_ascii_case(&default_root))
            {
                roots.push(default_root);
            }
        }
        roots
    }

    fn edit_string_list(
        &self,
        key: &str,
        edit: impl FnOnce(&mut Vec<String>),
    ) -> Result<(), CoreError> {
        let mut list = self.string_list(key);
        let before = list.clone();
        edit(&mut list);
        list.dedup();
        if list != before {
            self.settings
                .lock()
                .unwrap()
                .set(key, serde_json::json!(list))?;
        }
        Ok(())
    }

    /// Remembers a folder of projects (like Laragon's `www`) so new folders in it are
    /// picked up automatically.
    pub fn remember_projects_root(&self, root: &str) -> Result<(), CoreError> {
        self.edit_string_list(PROJECT_ROOTS, |list| {
            if !list.iter().any(|r| r.eq_ignore_ascii_case(root)) {
                list.push(root.to_string());
            }
        })
    }

    /// Folders the app must never scan or read. Always includes `ols_excluded_sites`.
    /// Guaranteed to return at least one valid folder name.
    pub fn ignored_folders(&self) -> Vec<String> {
        let s = self.settings.lock().unwrap();
        let mut folders = match s.get(PROJECT_IGNORED_FOLDERS).and_then(|v| v.as_array()) {
            Some(a) => {
                let filtered: Vec<String> = a
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                if filtered.is_empty() {
                    DEFAULT_IGNORED_FOLDERS
                        .iter()
                        .map(|name| (*name).to_string())
                        .collect()
                } else {
                    filtered
                }
            }
            None => DEFAULT_IGNORED_FOLDERS
                .iter()
                .map(|name| (*name).to_string())
                .collect(),
        };
        if !folders
            .iter()
            .any(|folder| folder.eq_ignore_ascii_case(OLS_EXCLUDED_FOLDER))
        {
            folders.push(OLS_EXCLUDED_FOLDER.to_string());
        }
        folders
    }

    /// Primary folder name to use when moving deleted sites/projects.
    pub fn primary_excluded_folder(&self) -> String {
        OLS_EXCLUDED_FOLDER.to_string()
    }

    /// Renames a folder in the ignored list and also renames any matching folder
    /// on disk inside registered root folders.
    pub fn rename_ignored_folder(&self, old_name: &str, new_name: &str) -> Result<(), CoreError> {
        let old_name = old_name.trim();
        let new_name = new_name.trim();
        if old_name.eq_ignore_ascii_case(OLS_EXCLUDED_FOLDER) {
            return Err(CoreError::failed_fix(
                "The required ignored folder cannot be renamed.",
                format!(
                    "'{OLS_EXCLUDED_FOLDER}' is reserved for safely deleted sites and projects."
                ),
                "Keep this folder name unchanged and rename a different ignored folder instead.",
            ));
        }
        if new_name.is_empty() {
            return Err(CoreError::failed_fix(
                "Folder name cannot be blank.",
                "An ignored folder name must contain at least one character.",
                "Enter a non-empty folder name.",
            ));
        }
        if new_name.contains('/') || new_name.contains('\\') {
            return Err(CoreError::failed_fix(
                "Invalid folder name.",
                format!("'{new_name}' contains path separators."),
                "Enter a single folder name without slashes or backslashes.",
            ));
        }
        let mut list = self.ignored_folders();
        if list
            .iter()
            .any(|f| f.eq_ignore_ascii_case(new_name) && !f.eq_ignore_ascii_case(old_name))
        {
            return Err(CoreError::failed_fix(
                "Folder name already in ignored list.",
                format!("'{new_name}' is already being ignored."),
                "Choose a unique folder name.",
            ));
        }
        if let Some(pos) = list.iter().position(|f| f.eq_ignore_ascii_case(old_name)) {
            list[pos] = new_name.to_string();
        } else {
            list.push(new_name.to_string());
        }
        self.settings
            .lock()
            .unwrap()
            .set(PROJECT_IGNORED_FOLDERS, serde_json::json!(list))?;

        // If the folder exists on disk in any root folder, rename it too
        for root in self.string_list(PROJECT_ROOTS) {
            let root_path = PathBuf::from(root);
            let old_dir = root_path.join(old_name);
            let new_dir = root_path.join(new_name);
            if old_dir.is_dir() && !new_dir.exists() {
                let _ = std::fs::rename(&old_dir, &new_dir);
            }
        }
        Ok(())
    }

    /// Reads one skip list, oldest entries last. Values written by earlier versions were
    /// plain strings and are read as entries with no timestamp.
    pub fn remove_project(&self, id: &str) -> Result<(), CoreError> {
        let path = self
            .projects
            .lock()
            .unwrap()
            .get(id)
            .map(|p| p.path.clone());
        self.projects.lock().unwrap().remove(id)?;
        if let Some(path) = path {
            self.push_skip(PROJECT_SKIP, &path)?;
        }
        Ok(())
    }

    /// Deletes a project without a domain by moving its folder into an `ols_excluded_sites` folder
    /// in the same directory instead of permanently deleting it.
    pub fn delete_project(&self, id: &str) -> Result<(), CoreError> {
        let path_str = {
            let mut projects = self.projects.lock().unwrap();
            let p = projects.get(id).ok_or_else(|| {
                CoreError::failed(
                    "Project not found",
                    format!("{id} is not a registered project"),
                )
            })?;
            let path = p.path.clone();
            projects.remove(id)?;
            path
        };
        let folder_path = PathBuf::from(&path_str);
        if folder_path.is_dir() {
            let excluded_folder = self.primary_excluded_folder();
            let _ = move_to_excluded_dir(&folder_path, &excluded_folder)?;
        }
        Ok(())
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let entry_path = entry.path();
        let dest_path = dst.join(entry.file_name());
        if entry_path.is_dir() {
            copy_dir_recursive(&entry_path, &dest_path)?;
        } else {
            std::fs::copy(&entry_path, &dest_path)?;
        }
    }
    Ok(())
}

pub(crate) fn move_to_excluded_dir(
    path: &Path,
    excluded_folder_name: &str,
) -> Result<PathBuf, CoreError> {
    if !path.is_dir() {
        return Err(CoreError::failed_fix(
            format!("Folder does not exist: {}", path.display()),
            "The site project directory could not be found on disk.",
            "Verify the folder exists or remove the broken site from the list.",
        ));
    }
    let parent = path.parent().ok_or_else(|| {
        CoreError::failed_fix(
            format!("Cannot exclude root directory: {}", path.display()),
            "A filesystem root has no parent directory to create an excluded folder in.",
            "Projects must be located inside a subdirectory.",
        )
    })?;
    let file_name = path.file_name().ok_or_else(|| {
        CoreError::failed_fix(
            format!("Invalid folder name: {}", path.display()),
            "Could not determine the folder name to move.",
            "Verify the project path is valid.",
        )
    })?;
    let file_name_str = file_name.to_string_lossy().to_string();
    let excluded_dir = parent.join(excluded_folder_name);
    if !excluded_dir.exists() {
        std::fs::create_dir_all(&excluded_dir).map_err(|e| {
            CoreError::failed_fix(
                format!("Could not create '{excluded_folder_name}' folder."),
                format!("Failed creating directory {}: {e}", excluded_dir.display()),
                "Check folder permissions on the parent directory.",
            )
        })?;
    }
    let mut dest = excluded_dir.join(&file_name_str);
    if dest.exists() {
        let mut counter = 1;
        loop {
            let candidate = excluded_dir.join(format!("{file_name_str}-{counter}"));
            if !candidate.exists() {
                dest = candidate;
                break;
            }
            counter += 1;
        }
    }

    if let Err(e) = std::fs::rename(path, &dest) {
        if let Err(copy_err) =
            copy_dir_recursive(path, &dest).and_then(|_| std::fs::remove_dir_all(path))
        {
            return Err(CoreError::failed_fix(
                format!("Could not move project folder to {excluded_folder_name}."),
                format!(
                    "Moving {} to {} failed: rename error ({e}), copy error ({copy_err})",
                    path.display(),
                    dest.display()
                ),
                "Ensure no running process or editor is holding a lock on the project files.",
            ));
        }
    }
    Ok(dest)
}
