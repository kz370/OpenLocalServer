use notify::RecursiveMode;
use notify_debouncer_mini::new_debouncer;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;

use crate::app::*;
use crate::domain::Domain;
use crate::error::CoreError;
use crate::project::{build_detail, Project, ProjectDetail};

pub const CUSTOM_PROJECT_TYPES_SETTING: &str = "projects.custom_types";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectFolderInspection {
    pub exists: bool,
    pub is_dir: bool,
    pub path: String,
    pub suggested_name: String,
    pub duplicate_of: Option<Project>,
    pub detected_type: Option<String>,
}

impl Inner {
    pub fn subscribe_project_events(&self) -> broadcast::Receiver<()> {
        self.project_events.subscribe()
    }

    pub fn notify_project_events(&self) {
        let _ = self.project_events.send(());
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
                                if let Err(error) = core.sync_auto_domains() {
                                    tracing::warn!(%error, "automatic domains could not be synced after a project health change");
                                }
                                if let Err(error) = core.apply_web(&[]) {
                                    tracing::warn!(%error, "web configs could not be applied after project health change");
                                }
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
        let (path, name, standalone) = {
            let projects = self.projects.lock().unwrap();
            let p = projects.get(id);
            (
                p.as_ref().map(|pr| pr.path.clone()),
                p.as_ref().map(|pr| pr.name.clone()),
                p.as_ref().map(|pr| pr.standalone).unwrap_or(false),
            )
        };

        // Guard: A standalone project cannot be removed if it is connected to a site
        if standalone {
            if let Some(ref path_str) = path {
                let domains = self.domains.lock().unwrap();
                let connected: Vec<String> = domains
                    .list()
                    .into_iter()
                    .filter(|d| {
                        d.project_id.as_deref() == Some(id)
                            || (!d.root.is_empty() && crate::project::same_path(&d.root, path_str))
                    })
                    .map(|d| d.hostname)
                    .collect();

                if !connected.is_empty() {
                    let project_name = name.unwrap_or_else(|| id.to_string());
                    let site_list = connected.join(", ");
                    return Err(CoreError::failed_fix(
                        "Cannot remove project connected to a site.",
                        format!(
                            "Project '{project_name}' is currently connected to site(s): {site_list}."
                        ),
                        "Delete or unlink the connected site(s) before removing this project.",
                    ));
                }
            }
        }

        self.projects.lock().unwrap().remove(id)?;
        if let Some(path) = path {
            self.push_skip(PROJECT_SKIP, &path)?;
        }
        let _ = self.project_events.send(());
        Ok(())
    }

    pub fn ignore_project(&self, id: &str) -> Result<(), CoreError> {
        let path = {
            let projects = self.projects.lock().unwrap();
            projects.get(id).map(|pr| pr.path.clone())
        };
        if let Some(path) = path {
            // Disable any sites tied to this project so it stops working.
            let hostnames: Vec<String> = {
                let domains = self.domains.lock().unwrap();
                domains
                    .list()
                    .into_iter()
                    .filter(|d| {
                        d.project_id.as_deref() == Some(id)
                            || (!d.root.is_empty() && crate::project::same_path(&d.root, &path))
                    })
                    .map(|d| d.hostname)
                    .collect()
            };
            let is_discover_path = self.string_list(PROJECT_ROOTS).iter().any(|root| {
                Path::new(&path).starts_with(Path::new(root))
            });
            if !is_discover_path {
                for h in hostnames {
                    self.domains.lock().unwrap().remove(&h)?;
                    self.web.restart_app(&h);
                }
                self.projects.lock().unwrap().remove(id)?;
            } else {
                let ignore_file = std::path::Path::new(&path).join(".ols_ignore_me");
                let _ = std::fs::write(&ignore_file, "");
            }
        }
        let _ = self.project_events.send(());
        Ok(())
    }

    /// Deletes a project without a domain by moving its folder into an `ols_excluded_sites` folder
    /// in the same directory instead of permanently deleting it.
    pub fn delete_project(&self, id: &str) -> Result<(), CoreError> {
        let (path_str, name) = {
            let projects = self.projects.lock().unwrap();
            let p = projects.get(id).ok_or_else(|| {
                CoreError::failed(
                    "Project not found",
                    format!("{id} is not a registered project"),
                )
            })?;
            (p.path.clone(), p.name.clone())
        };

        // Guard: A project cannot be deleted if it is connected to a site
        {
            let domains = self.domains.lock().unwrap();
            let connected: Vec<String> = domains
                .list()
                .into_iter()
                .filter(|d| {
                    d.project_id.as_deref() == Some(id)
                        || (!d.root.is_empty() && crate::project::same_path(&d.root, &path_str))
                })
                .map(|d| d.hostname)
                .collect();

            if !connected.is_empty() {
                let site_list = connected.join(", ");
                return Err(CoreError::failed_fix(
                    "Cannot delete project connected to a site.",
                    format!("Project '{name}' is currently connected to site(s): {site_list}."),
                    "Delete or unlink the connected site(s) before deleting this project.",
                ));
            }
        }

        self.projects.lock().unwrap().remove(id)?;
        let folder_path = PathBuf::from(&path_str);
        if folder_path.is_dir() {
            let excluded_folder = self.primary_excluded_folder();
            let _ = move_to_excluded_dir(&folder_path, &excluded_folder)?;
        }
        Ok(())
    }

    /// Relocates a registered project to a new directory path on disk, updating all associated domain roots.
    pub fn relocate_project(&self, id: &str, new_path: &str) -> Result<Project, CoreError> {
        let path_buf = PathBuf::from(new_path);
        if !path_buf.is_dir() {
            return Err(CoreError::InvalidProjectPath(new_path.to_string()));
        }
        let canonical = std::fs::canonicalize(&path_buf).unwrap_or(path_buf.clone());
        let canonical_str = crate::project::strip_verbatim_prefix(&canonical.display().to_string());

        let (project, old_path) = {
            let mut projects = lock(&self.projects);
            let old_p = projects.get(id).ok_or_else(|| {
                CoreError::failed(
                    "Project not found",
                    format!("{id} is not a registered project"),
                )
            })?;
            let old_path = old_p.path.clone();
            let updated = projects.relocate(id, &canonical_str)?;
            (updated, old_path)
        };

        // Clear any skip on the new path
        let _ = self.clear_project_skip(&canonical_str);

        // Update domain roots for any domains tied to this project
        {
            let mut domains = lock(&self.domains);
            let list = domains.list();
            for mut domain in list {
                if domain.project_id.as_deref() == Some(id) {
                    if let Ok(rel) = Path::new(&domain.root).strip_prefix(Path::new(&old_path)) {
                        let updated_root = Path::new(&canonical_str).join(rel);
                        domain.root = updated_root.display().to_string();
                    } else if domain.root.is_empty() || domain.root == old_path {
                        domain.root = canonical_str.clone();
                    }
                    let _ = domains.update(domain);
                }
            }
        }

        let _ = self.project_events.send(());

        if let Err(e) = self.sync_auto_domains() {
            tracing::warn!(error = %e, "automatic domains could not be synced after relocate");
        }

        if let Err(e) = self.apply_web(&[]) {
            tracing::warn!(error = %e, "web configs could not be applied after relocate");
        }

        Ok(project)
    }

    /// Updates metadata (name, icon spec, and project type) on a registered project.
    pub fn update_project(
        &self,
        id: &str,
        name: Option<String>,
        icon: Option<String>,
        clear_icon: Option<bool>,
        project_type: Option<String>,
        clear_project_type: Option<bool>,
    ) -> Result<Project, CoreError> {
        let mut projects = lock(&self.projects);
        let updated =
            projects.update_meta(id, name, icon, clear_icon, project_type, clear_project_type)?;
        let _ = self.project_events.send(());
        Ok(updated)
    }

    /// Creates a site domain for a project if possible, or returns needs_setup if manual configuration is required.
    pub fn create_site_for_project(
        &self,
        project_id: &str,
    ) -> Result<crate::command::CreateSiteResult, CoreError> {
        let project = self.ensure_project_available(project_id)?;
        let path = Path::new(&project.path);

        // 1. If an existing domain matches this project id or folder, return it.
        {
            let domains = lock(&self.domains);
            let projects = lock(&self.projects).list();
            let norm = |p: &str| {
                p.replace('/', "\\")
                    .trim_end_matches('\\')
                    .to_ascii_lowercase()
            };
            let proj_norm = norm(&project.path);
            if let Some(existing) = domains.list().into_iter().find(|d| {
                d.project_id.as_deref() == Some(project_id)
                    || norm(&d.root) == proj_norm
                    || norm(&crate::app::domains::site_folder(d, &projects)) == proj_norm
            }) {
                return Ok(crate::command::CreateSiteResult {
                    domain: Some(Box::new(existing.clone())),
                    needs_setup: false,
                    suggested_hostname: existing.hostname,
                });
            }
        }

        // 2. Derive suggested hostname
        let default_tld = self
            .settings
            .lock()
            .unwrap()
            .get("domains.default_tld")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().trim_start_matches('.').to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "local".to_string());
        let auto_template = format!("{{project}}.{default_tld}");
        let tld_suffix = format!(".{default_tld}");
        let effective_name = project
            .name
            .strip_suffix(&tld_suffix)
            .unwrap_or(&project.name);
        let base_hostname = crate::domain::apply_template(&auto_template, effective_name);

        let candidate_hostname = {
            let domains = lock(&self.domains);
            if domains.get(&base_hostname).is_none() {
                base_hostname
            } else {
                let mut idx = 1;
                loop {
                    let next = format!("{effective_name}-{idx}.{default_tld}");
                    if domains.get(&next).is_none() {
                        break next;
                    }
                    idx += 1;
                }
            }
        };

        // 3. Check if auto_site can serve it
        let Some((kind, root)) = crate::app::domains::sync::auto_site(path) else {
            return Ok(crate::command::CreateSiteResult {
                domain: None,
                needs_setup: true,
                suggested_hostname: candidate_hostname,
            });
        };

        let domain = Domain {
            hostname: candidate_hostname.clone(),
            project_id: Some(project.id.clone()),
            root: root.display().to_string(),
            kind,
            https: true,
            redirect_https: true,
            wildcard: false,
            enabled: true,
            icon: project.icon.clone(),
            ..Default::default()
        };

        let created = self.add_domain(domain)?;
        if created.enabled && !self.web.dns_covers(&created.hostname) {
            let _ = crate::hosts::ensure(&[created.hostname.clone()]);
        }
        if self.web.is_running() {
            let _ = self.apply_web(&[]);
        }

        Ok(crate::command::CreateSiteResult {
            domain: Some(Box::new(created)),
            needs_setup: false,
            suggested_hostname: candidate_hostname,
        })
    }

    /// Opens an external Windows Terminal window in the project's directory, falling back to PowerShell.
    pub fn open_project_terminal(&self, project_id: &str) -> Result<(), CoreError> {
        let project = self.ensure_project_available(project_id)?;
        let dir = Path::new(&project.path);
        let mut wt = std::process::Command::new("wt.exe");
        wt.arg("-w").arg("new").arg("-d").arg(dir);
        if wt.spawn().is_ok() {
            return Ok(());
        }
        std::process::Command::new(crate::exec::powershell_path())
            .arg("-NoExit")
            .current_dir(dir)
            .spawn()
            .map(|_| ())
            .map_err(|e| {
                CoreError::failed_fix(
                    "Could not open external terminal window.",
                    e.to_string(),
                    "Ensure Windows Terminal or PowerShell is available on this system.",
                )
            })
    }

    /// Adds a custom editor to the configuration and returns the updated editor list.
    pub fn add_editor(
        &self,
        name: &str,
        path: &str,
    ) -> Result<Vec<crate::editors::EditorInfo>, CoreError> {
        let name = name.trim();
        let path = path.trim();
        if name.is_empty() {
            return Err(CoreError::failed_fix(
                "Editor name cannot be blank.",
                "An editor name is required.",
                "Enter a display name for the editor.",
            ));
        }
        if !Path::new(path).is_file() {
            return Err(CoreError::failed_fix(
                "Editor executable not found.",
                format!("'{path}' is not a valid executable file."),
                "Select a valid .exe, .cmd, or .bat file on your system.",
            ));
        }
        let mut settings = lock(&self.settings);
        let mut list = crate::editors::list_custom(&settings);
        let slug = name
            .to_lowercase()
            .replace(|c: char| !c.is_alphanumeric(), "-");
        let id = format!("custom:{}", slug.trim_matches('-'));
        if let Some(existing) = list
            .iter_mut()
            .find(|e| e.id == id || e.path.eq_ignore_ascii_case(path))
        {
            existing.name = name.to_string();
            existing.path = path.to_string();
        } else {
            list.push(crate::editors::CustomEditor {
                id,
                name: name.to_string(),
                path: path.to_string(),
                args: None,
            });
        }
        crate::editors::save_custom(&mut settings, &list)?;
        let _ = self.project_events.send(());
        Ok(crate::editors::list_all(&settings))
    }

    /// Updates an existing custom editor's display name or executable path.
    pub fn update_editor(
        &self,
        id: &str,
        name: &str,
        path: &str,
    ) -> Result<Vec<crate::editors::EditorInfo>, CoreError> {
        let name = name.trim();
        let path = path.trim();
        if name.is_empty() {
            return Err(CoreError::failed_fix(
                "Editor name cannot be blank.",
                "An editor name is required.",
                "Enter a display name for the editor.",
            ));
        }
        if !Path::new(path).is_file() {
            return Err(CoreError::failed_fix(
                "Editor executable not found.",
                format!("'{path}' is not a valid executable file."),
                "Select a valid .exe, .cmd, or .bat file on your system.",
            ));
        }
        let mut settings = lock(&self.settings);
        let mut list = crate::editors::list_custom(&settings);
        let item = list.iter_mut().find(|e| e.id == id).ok_or_else(|| {
            CoreError::failed_fix(
                "Editor not found.",
                format!("No custom editor with id '{id}' was found."),
                "Check the editor ID and try again.",
            )
        })?;
        item.name = name.to_string();
        item.path = path.to_string();
        crate::editors::save_custom(&mut settings, &list)?;
        let _ = self.project_events.send(());
        Ok(crate::editors::list_all(&settings))
    }

    /// Removes a custom editor by id and returns the updated editor list.
    pub fn remove_editor(&self, id: &str) -> Result<Vec<crate::editors::EditorInfo>, CoreError> {
        let mut settings = lock(&self.settings);
        let mut list = crate::editors::list_custom(&settings);
        list.retain(|e| e.id != id);
        crate::editors::save_custom(&mut settings, &list)?;
        let _ = self.project_events.send(());
        Ok(crate::editors::list_all(&settings))
    }

    /// Sets whether an editor is hidden from menus and pickers, returning the updated list.
    pub fn set_editor_hidden(
        &self,
        id: &str,
        hidden: bool,
    ) -> Result<Vec<crate::editors::EditorInfo>, CoreError> {
        let mut settings = lock(&self.settings);
        let mut list = crate::editors::list_hidden(&settings);
        if hidden {
            if !list.iter().any(|h| h == id) {
                list.push(id.to_string());
            }
        } else {
            list.retain(|h| h != id);
        }
        crate::editors::save_hidden(&mut settings, &list)?;
        let _ = self.project_events.send(());
        Ok(crate::editors::list_all(&settings))
    }

    pub fn set_pending_add_project(&self, path: String) {
        *self.pending_add_project.lock().unwrap() = Some(path);
    }

    pub fn take_pending_add_project(&self) -> Option<String> {
        self.pending_add_project.lock().unwrap().take()
    }

    pub fn inspect_project_folder(&self, path: &str) -> ProjectFolderInspection {
        let p = Path::new(path);
        if !p.exists() {
            return ProjectFolderInspection {
                exists: false,
                is_dir: false,
                path: path.to_string(),
                suggested_name: p
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("project")
                    .to_string(),
                duplicate_of: None,
                detected_type: None,
            };
        }
        if !p.is_dir() {
            return ProjectFolderInspection {
                exists: true,
                is_dir: false,
                path: path.to_string(),
                suggested_name: p
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("project")
                    .to_string(),
                duplicate_of: None,
                detected_type: None,
            };
        }

        let canonical = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        let canonical_str = crate::project::strip_verbatim_prefix(&canonical.display().to_string());
        let name = canonical
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("project")
            .to_string();

        let duplicate_of = {
            let projects = lock(&self.projects);
            projects
                .list()
                .into_iter()
                .find(|pr| crate::project::same_path(&pr.path, &canonical_str))
        };

        let is_tauri = canonical
            .join("src-tauri")
            .join("tauri.conf.json")
            .is_file()
            || canonical.join("src-tauri").join("Cargo.toml").is_file()
            || canonical.join("tauri.conf.json").is_file()
            || (canonical.join("package.json").is_file() && {
                std::fs::read_to_string(canonical.join("package.json"))
                    .map(|s| s.contains("@tauri-apps/api") || s.contains("@tauri-apps/cli"))
                    .unwrap_or(false)
            });

        let detected_type = if is_tauri {
            Some("custom:Tauri".to_string())
        } else {
            let detection = crate::detection::detect(&canonical);
            let mut dt = match detection.framework {
                crate::detection::Framework::Laravel => Some("laravel".to_string()),
                crate::detection::Framework::Symfony => Some("symfony".to_string()),
                crate::detection::Framework::WordPress => Some("wordpress".to_string()),
                crate::detection::Framework::GenericPhp => Some("php".to_string()),
                crate::detection::Framework::Node => Some("node".to_string()),
                crate::detection::Framework::Django => Some("django".to_string()),
                crate::detection::Framework::Flask => Some("flask".to_string()),
                crate::detection::Framework::FastApi => Some("fastapi".to_string()),
                crate::detection::Framework::GenericPython => Some("python".to_string()),
                crate::detection::Framework::Unknown => None,
            };

            // Refine generic Node or Unknown if popular presets exist
            if dt.as_deref() == Some("node") || dt.is_none() {
                if let Ok(pkg) = std::fs::read_to_string(canonical.join("package.json")) {
                    if pkg.contains("\"next\"")
                        || canonical.join("next.config.js").is_file()
                        || canonical.join("next.config.mjs").is_file()
                        || canonical.join("next.config.ts").is_file()
                    {
                        dt = Some("nextjs".to_string());
                    } else if pkg.contains("\"vue\"") || canonical.join("vue.config.js").is_file() {
                        dt = Some("vue".to_string());
                    } else if pkg.contains("\"react\"") {
                        dt = Some("react".to_string());
                    }
                }
            }

            // Other language presets if still undetected
            if dt.is_none() {
                if canonical.join("go.mod").is_file() {
                    dt = Some("go".to_string());
                } else if canonical.join("Cargo.toml").is_file() {
                    dt = Some("custom:Rust".to_string());
                } else if canonical.join("pom.xml").is_file()
                    || canonical.join("build.gradle").is_file()
                {
                    dt = Some("java".to_string());
                } else if std::fs::read_dir(&canonical)
                    .ok()
                    .map(|mut r| {
                        r.any(|e| {
                            e.ok()
                                .map(|entry| {
                                    let name = entry.file_name().to_string_lossy().to_lowercase();
                                    name.ends_with(".csproj") || name.ends_with(".sln")
                                })
                                .unwrap_or(false)
                        })
                    })
                    .unwrap_or(false)
                {
                    dt = Some("dotnet".to_string());
                } else if canonical.join("index.html").is_file() {
                    dt = Some("static".to_string());
                }
            }
            dt
        };

        ProjectFolderInspection {
            exists: true,
            is_dir: true,
            path: canonical_str,
            suggested_name: name,
            duplicate_of,
            detected_type,
        }
    }

    /// Reads custom project types from settings and existing registered projects.
    pub fn list_custom_project_types(&self) -> Vec<crate::project::CustomProjectType> {
        let settings = lock(&self.settings);
        let mut names: Vec<String> = settings
            .get(CUSTOM_PROJECT_TYPES_SETTING)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        // Always ensure "Tauri" is offered in the custom project type list
        if !names.iter().any(|n| n.eq_ignore_ascii_case("tauri")) {
            names.push("Tauri".to_string());
        }

        // Also check if any existing projects use a custom:<name> not yet in settings
        let projects = lock(&self.projects).list();
        for p in projects {
            if let Some(ref pt) = p.project_type {
                if let Some(custom_name) = pt.strip_prefix("custom:") {
                    let trimmed = custom_name.trim();
                    if !trimmed.is_empty() && !names.iter().any(|n| n.eq_ignore_ascii_case(trimmed))
                    {
                        names.push(trimmed.to_string());
                    }
                }
            }
        }

        names.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
        names
            .into_iter()
            .map(|name| crate::project::CustomProjectType {
                id: format!("custom:{name}"),
                name,
            })
            .collect()
    }

    pub fn add_custom_project_type(
        &self,
        name: &str,
    ) -> Result<Vec<crate::project::CustomProjectType>, CoreError> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(CoreError::failed_fix(
                "Type name cannot be blank.",
                "A project type name is required.",
                "Enter a name for the custom project type (e.g. Rust, Go API).",
            ));
        }
        if trimmed.len() > 40 {
            return Err(CoreError::failed_fix(
                "Type name is too long.",
                "Project type names cannot exceed 40 characters.",
                "Choose a shorter name for the custom project type.",
            ));
        }
        if trimmed.eq_ignore_ascii_case("all") || trimmed.eq_ignore_ascii_case("auto") {
            return Err(CoreError::failed_fix(
                "Reserved project type name.",
                format!("'{trimmed}' is a reserved type name in OpenLocalServer."),
                "Choose a different custom type name.",
            ));
        }

        let presets = crate::project::list_project_types();
        if presets
            .iter()
            .any(|p| p.name.eq_ignore_ascii_case(trimmed) || p.id.eq_ignore_ascii_case(trimmed))
        {
            return Err(CoreError::failed_fix(
                "Type name already exists.",
                format!("'{trimmed}' matches a built-in framework preset."),
                "Choose a different custom name or select the existing preset from the list.",
            ));
        }

        let mut settings = lock(&self.settings);
        let mut list: Vec<String> = settings
            .get(CUSTOM_PROJECT_TYPES_SETTING)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        if list.iter().any(|n| n.eq_ignore_ascii_case(trimmed)) {
            return Err(CoreError::failed_fix(
                "Type name already exists.",
                format!("A custom project type named '{trimmed}' is already configured."),
                "Enter a unique name or edit the existing custom type.",
            ));
        }

        list.push(trimmed.to_string());
        settings.set(CUSTOM_PROJECT_TYPES_SETTING, serde_json::to_value(&list)?)?;

        drop(settings);
        let _ = self.project_events.send(());
        Ok(self.list_custom_project_types())
    }

    pub fn rename_custom_project_type(
        &self,
        old_name: &str,
        new_name: &str,
    ) -> Result<Vec<crate::project::CustomProjectType>, CoreError> {
        let old_trimmed = old_name.trim();
        let new_trimmed = new_name.trim();
        if new_trimmed.is_empty() {
            return Err(CoreError::failed_fix(
                "Type name cannot be blank.",
                "A project type name is required.",
                "Enter a new name for the custom project type.",
            ));
        }
        if new_trimmed.len() > 40 {
            return Err(CoreError::failed_fix(
                "Type name is too long.",
                "Project type names cannot exceed 40 characters.",
                "Choose a shorter name for the custom project type.",
            ));
        }
        if new_trimmed.eq_ignore_ascii_case("all") || new_trimmed.eq_ignore_ascii_case("auto") {
            return Err(CoreError::failed_fix(
                "Reserved project type name.",
                format!("'{new_trimmed}' is a reserved type name in OpenLocalServer."),
                "Choose a different custom type name.",
            ));
        }

        let presets = crate::project::list_project_types();
        if presets.iter().any(|p| {
            p.name.eq_ignore_ascii_case(new_trimmed) || p.id.eq_ignore_ascii_case(new_trimmed)
        }) {
            return Err(CoreError::failed_fix(
                "Type name already exists.",
                format!("'{new_trimmed}' matches a built-in framework preset."),
                "Choose a different custom name.",
            ));
        }

        let mut settings = lock(&self.settings);
        let mut list: Vec<String> = settings
            .get(CUSTOM_PROJECT_TYPES_SETTING)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        if let Some(pos) = list
            .iter()
            .position(|n| n.eq_ignore_ascii_case(old_trimmed))
        {
            list[pos] = new_trimmed.to_string();
        } else {
            list.push(new_trimmed.to_string());
        }
        settings.set(CUSTOM_PROJECT_TYPES_SETTING, serde_json::to_value(&list)?)?;
        drop(settings);

        // Update all projects using old_name to new_name
        let old_target = format!("custom:{old_trimmed}").to_lowercase();
        let new_type = format!("custom:{new_trimmed}");
        {
            let mut projects = lock(&self.projects);
            let list = projects.list();
            for p in list {
                if let Some(ref pt) = p.project_type {
                    if pt.to_lowercase() == old_target {
                        let _ = projects.update_meta(
                            &p.id,
                            None,
                            None,
                            None,
                            Some(new_type.clone()),
                            None,
                        );
                    }
                }
            }
        }

        let _ = self.project_events.send(());
        Ok(self.list_custom_project_types())
    }

    pub fn remove_custom_project_type(
        &self,
        name: &str,
    ) -> Result<Vec<crate::project::CustomProjectType>, CoreError> {
        let trimmed = name.trim();
        let mut settings = lock(&self.settings);
        let mut list: Vec<String> = settings
            .get(CUSTOM_PROJECT_TYPES_SETTING)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        list.retain(|n| !n.eq_ignore_ascii_case(trimmed));
        settings.set(CUSTOM_PROJECT_TYPES_SETTING, serde_json::to_value(&list)?)?;
        drop(settings);

        // Reset projects using this type to Auto (None)
        let target = format!("custom:{trimmed}").to_lowercase();
        {
            let mut projects = lock(&self.projects);
            let list = projects.list();
            for p in list {
                if let Some(ref pt) = p.project_type {
                    if pt.to_lowercase() == target {
                        let _ = projects.update_meta(&p.id, None, None, None, None, Some(true));
                    }
                }
            }
        }

        let _ = self.project_events.send(());
        Ok(self.list_custom_project_types())
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
