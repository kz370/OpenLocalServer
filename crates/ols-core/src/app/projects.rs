use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use notify::RecursiveMode;
use notify_debouncer_mini::new_debouncer;
use tokio::sync::broadcast;

use super::*;
use crate::error::CoreError;
use crate::project::{build_detail, Project, ProjectDetail};

/// One entry of the deletion history: a project folder or an automatic site the user
/// deleted, kept so scans and automatic domains leave it alone. Clearing an entry only
/// removes it from the history — it never touches the folder on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeletedItem {
    /// The skipped project path, or the skipped hostname.
    pub value: String,
    /// When the user deleted it, in milliseconds since the Unix epoch. Entries written
    /// before this field existed report `0` and sort last.
    pub deleted_at: u64,
    /// "project" or "domain" — which skip list the entry came from.
    pub kind: String,
}

impl DeletedItem {
    pub fn is_project(&self) -> bool {
        self.kind == "project"
    }
}

/// A skip-list entry as stored: the value plus when it was recorded. Plain strings are
/// still read (lists written by earlier versions), which is all they ever held.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct SkipEntry {
    pub(crate) value: String,
    pub(crate) deleted_at: u64,
}

impl SkipEntry {
    fn reads_as(entry: &serde_json::Value) -> Option<Self> {
        match entry {
            serde_json::Value::String(v) => Some(Self {
                value: v.clone(),
                deleted_at: 0,
            }),
            serde_json::Value::Object(_) => serde_json::from_value(entry.clone()).ok(),
            _ => None,
        }
    }
}

/// `ProjectStore::register` stores paths without the Windows `\\?\` prefix, but a caller can
/// hand us one that still has it and canonicalizing adds it back. Normalize both sides so
/// one folder is spelled one way on every comparison.
fn normalize_skip_path(path: &str) -> String {
    path.strip_prefix(r"\\?\").unwrap_or(path).to_string()
}

/// True when `value` can only be a filesystem path: a rooted path, a drive, or anything
/// with a separator. A bare automatic-domain name (`app1.local`) is not one, and resolving
/// a bare name against the working directory could match a folder that happens to share it.
fn is_path_like(value: &str) -> bool {
    Path::new(value).is_absolute() || value.contains(['\\', '/', ':'])
}

/// Resolves a path to a comparable form, falling back to canonicalizing its *parent* when
/// the leaf itself is gone.
///
/// The fallback is the case that matters for clearing: a folder deleted from disk before its
/// history entry is cleared cannot be canonicalized at all, so a whole-path resolve on both
/// sides fails and the two spellings are compared raw again — the clear silently does nothing.
/// The parent usually outlives the leaf, and a short name is expanded at every level, so
/// canonicalizing the parent and reattaching the file name still collapses the two spellings.
fn comparable_path(value: &str) -> Option<String> {
    if let Ok(c) = std::fs::canonicalize(value) {
        return Some(normalize_skip_path(&c.display().to_string()));
    }
    let path = Path::new(value);
    let parent = path.parent()?;
    let name = path.file_name()?;
    let parent = std::fs::canonicalize(parent).ok()?;
    Some(normalize_skip_path(
        &parent.join(name).display().to_string(),
    ))
}

/// One folder, whatever either side spells it: the canonicalized path `ProjectStore::register`
/// wrote, and the raw spelling a scan or a caller holds.
///
/// Same rule as `skip_matches`, applied the other way round. `skip_matches` compares an
/// entry against a scan's path; this compares a caller-supplied value against an entry, so
/// clearing an entry has to resolve both sides too. On Windows a short name
/// (`C:\Users\RUNNER~1\...`) is a different string from the long name `canonicalize`
/// returns, so comparing the strings alone left the entry in place: the settings page said
/// it had cleared it, the history still listed it, and the folder stayed skipped.
pub(crate) fn same_skip_entry(entry: &SkipEntry, value: &str) -> bool {
    let stored = normalize_skip_path(&entry.value);
    let asked = normalize_skip_path(value);
    if stored.eq_ignore_ascii_case(&asked) {
        return true;
    }
    if !is_path_like(&stored) || !is_path_like(&asked) {
        return false;
    }
    match (comparable_path(&stored), comparable_path(&asked)) {
        (Some(a), Some(b)) => a.eq_ignore_ascii_case(&b),
        _ => false,
    }
}

/// True when `path` is one of the skipped project folders, in either form.
///
/// Skips are stored the way `ProjectStore::register` writes paths — canonicalized — while a
/// scan only has the path `read_dir` handed it, which is raw. A folder reached through a
/// short name, a junction, a mapped drive or a `..` segment only matches when both are
/// compared; comparing the raw spelling alone let a scan re-add what the user deleted.
pub(crate) fn skip_matches(entries: &[SkipEntry], path: &str) -> bool {
    let raw = normalize_skip_path(path);
    let canonical = std::fs::canonicalize(Path::new(&raw))
        .map(|c| normalize_skip_path(&c.display().to_string()))
        .unwrap_or_else(|_| raw.clone());
    entries.iter().any(|e| {
        let stored = normalize_skip_path(&e.value);
        stored.eq_ignore_ascii_case(&raw) || stored.eq_ignore_ascii_case(&canonical)
    })
}


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
            loop {
                match events_rx.recv_timeout(Duration::from_millis(500)) {
                    Ok(_) => {
                        if let Err(error) = core.sync_auto_domains() {
                            tracing::warn!(%error, "automatic domains could not be synced after a project change");
                        }
                        let _ = core.project_events.send(());
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
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

    /// The URL a site is reachable at, on the server that actually renders it: a site
    /// pinned to a non-default server always carries that server's port.

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
    pub(crate) fn skip_entries(&self, key: &str) -> Vec<SkipEntry> {
        let s = self.settings.lock().unwrap();
        s.get(key)
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(SkipEntry::reads_as).collect())
            .unwrap_or_default()
    }

    fn edit_skip_entries(
        &self,
        key: &str,
        edit: impl FnOnce(&mut Vec<SkipEntry>),
    ) -> Result<(), CoreError> {
        let mut list = self.skip_entries(key);
        let before = list.clone();
        edit(&mut list);
        // Same value twice keeps the newest timestamp, so a re-delete moves the entry up
        // the history instead of duplicating it.
        let mut merged: Vec<SkipEntry> = Vec::with_capacity(list.len());
        for entry in list {
            match merged.iter_mut().find(|e| same_skip_entry(e, &entry.value)) {
                Some(existing) => existing.deleted_at = existing.deleted_at.max(entry.deleted_at),
                None => merged.push(entry),
            }
        }
        if merged != before {
            self.settings
                .lock()
                .unwrap()
                .set(key, serde_json::json!(merged))?;
        }
        Ok(())
    }

    /// Records `value` in `key`'s skip list unless it is already there.
    pub(crate) fn push_skip(&self, key: &str, value: &str) -> Result<(), CoreError> {
        let value = value.to_string();
        let stamp = now_ms();
        self.edit_skip_entries(key, move |list| {
            match list.iter_mut().find(|e| same_skip_entry(e, &value)) {
                Some(existing) => existing.deleted_at = stamp,
                None => list.push(SkipEntry {
                    value,
                    deleted_at: stamp,
                }),
            }
        })
    }

    pub(crate) fn remove_skip(&self, key: &str, value: &str) -> Result<(), CoreError> {
        let value = value.to_string();
        self.edit_skip_entries(key, move |list| {
            list.retain(|e| !same_skip_entry(e, &value))
        })
    }

    /// The whole deletion history, newest first: deleted project folders and deleted
    /// automatic sites, both of which stay hidden from scans until cleared.
    pub fn list_deleted_items(&self) -> Vec<DeletedItem> {
        let mut items: Vec<DeletedItem> = [PROJECT_SKIP, AUTO_SKIP]
            .into_iter()
            .flat_map(|key| {
                let kind = if key == PROJECT_SKIP {
                    "project"
                } else {
                    "domain"
                };
                self.skip_entries(key)
                    .into_iter()
                    .map(move |e| DeletedItem {
                        value: e.value,
                        deleted_at: e.deleted_at,
                        kind: kind.to_string(),
                    })
            })
            .collect();
        items.sort_by_key(|i| std::cmp::Reverse(i.deleted_at));
        items
    }

    /// Drops one entry from the history. The folder it names is not touched: the next scan
    /// or automatic-domain sync picks it up again if it still exists on disk.
    pub fn forget_deleted_item(&self, value: &str) -> Result<bool, CoreError> {
        let before = self.list_deleted_items().len();
        // Both lists can name the same value (a folder and the site it served), so clear
        // it from both and report whether anything went.
        for key in [PROJECT_SKIP, AUTO_SKIP] {
            self.remove_skip(key, value)?;
        }
        Ok(self.list_deleted_items().len() < before)
    }

    /// Empties the deletion history. Nothing on disk changes; the next scan or sync simply
    /// sees those folders again. Returns how many entries were dropped.
    pub fn clear_deleted_items(&self) -> Result<usize, CoreError> {
        let dropped = self.list_deleted_items().len();
        for key in [PROJECT_SKIP, AUTO_SKIP] {
            self.edit_skip_entries(key, |list| list.clear())?;
        }
        Ok(dropped)
    }

    /// Removes a project from the list for good: its folder joins a skip list so
    /// folder rescans (watcher, restart, Scan) don't re-register it. Explicitly
    /// adding the folder again clears the skip.
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
                CoreError::InvalidProjectPath(format!("{id} is not a registered project"))
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

pub(crate) fn move_to_excluded_dir(path: &Path, excluded_folder_name: &str) -> Result<PathBuf, CoreError> {
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

