//! Free up space (§170): find `node_modules`, `vendor`, build caches and similar folders in
//! registered projects that have not been touched for a while, show their sizes, and delete
//! the ones the user picks. These folders can always be rebuilt (`npm install`, `composer
//! install`, a build).
//!
//! Safety rules, all re-checked at delete time (nothing from the UI is trusted):
//! - only a **closed list** of folder shapes inside a **registered project** is ever a candidate;
//! - links (symlinks, Windows junctions) are never followed, counted or entered;
//! - a project with a running process inside it is never touched;
//! - deletion is permanent (these folders are huge and rebuildable).

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::app::Inner;
use crate::error::CoreError;
use crate::snapshots::is_plain_file_name;

const SIZE_TIME_LIMIT: Duration = Duration::from_secs(15);
const SIZE_ENTRY_LIMIT: u64 = 2_000_000;

/// Folders in a project root that count as "rebuildable" (also excluded from "last used").
const ROOT_CANDIDATES: &[&str] = &[
    "node_modules",
    "vendor",
    ".next",
    ".nuxt",
    ".svelte-kit",
    ".turbo",
    ".parcel-cache",
    ".angular/cache",
    "target",
    ".venv",
    "venv",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReclaimReport {
    pub scanned_ms: u64,
    pub projects: Vec<ReclaimProject>,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReclaimProject {
    pub project_id: String,
    pub name: String,
    pub path: String,
    pub idle_days: u64,
    pub busy: bool,
    pub items: Vec<ReclaimCandidate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReclaimCandidate {
    pub relative: String,
    /// `node_modules`, `vendor`, `build-cache`, `rust-target` or `venv`.
    pub kind: String,
    pub bytes: u64,
    pub size_is_minimum: bool,
    pub rebuild: String,
    pub default_selected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReclaimItem {
    pub project_id: String,
    pub relative: String,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ------------------------------------------------------------------ links

#[cfg(windows)]
fn is_reparse(md: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    // FILE_ATTRIBUTE_REPARSE_POINT: junctions, symlinks, mount points.
    md.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
fn is_reparse(_md: &std::fs::Metadata) -> bool {
    false
}

/// A symlink or junction (anything that points somewhere else).
fn is_link(md: &std::fs::Metadata) -> bool {
    md.file_type().is_symlink() || is_reparse(md)
}

fn is_real_dir(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir() && !is_link(&m))
}

// ------------------------------------------------------------------ candidates

/// `git ls-files --error-unmatch <file>` succeeds when Git tracks the file.
pub type GitCheck<'a> = &'a dyn Fn(&Path, &str) -> bool;

fn tracked_by_git(git: Option<&Path>, root: &Path, file: &str) -> bool {
    let Some(git) = git else { return false };
    crate::exec::run_capture(
        git,
        &[
            "ls-files".into(),
            "--error-unmatch".into(),
            file.to_string(),
        ],
        Some(root),
        &[],
        Duration::from_secs(10),
    )
    .success()
}

fn rebuild_command(dir: &Path) -> &'static str {
    if dir.join("pnpm-lock.yaml").is_file() {
        "pnpm install"
    } else if dir.join("yarn.lock").is_file() {
        "yarn"
    } else {
        "npm install"
    }
}

/// What kind of candidate `relative` is for this project, if the closed list and its
/// condition both allow it. `relative` uses `/`. Pure over the file system (and `tracked`).
fn classify(
    root: &Path,
    relative: &str,
    tracked: GitCheck,
) -> Option<(&'static str, String, bool)> {
    let parts: Vec<&str> = relative.split('/').collect();
    // packages/<x>/node_modules and apps/<x>/node_modules (one level).
    if parts.len() == 3
        && matches!(parts[0], "packages" | "apps")
        && is_plain_file_name(parts[1])
        && parts[2] == "node_modules"
    {
        let pkg = root.join(parts[0]).join(parts[1]);
        return pkg
            .join("package.json")
            .is_file()
            .then(|| ("node_modules", rebuild_command(&pkg).to_string(), true));
    }
    if !ROOT_CANDIDATES.contains(&relative) {
        return None;
    }
    match relative {
        "node_modules" => root
            .join("package.json")
            .is_file()
            .then(|| ("node_modules", rebuild_command(root).to_string(), true)),
        "vendor" => {
            if !root.join("composer.json").is_file() || !root.join("vendor/autoload.php").is_file()
            {
                return None;
            }
            let ok = if root.join(".git").exists() {
                !tracked(root, "vendor/autoload.php")
            } else {
                root.join("composer.lock").is_file()
            };
            ok.then(|| ("vendor", "composer install".to_string(), true))
        }
        ".next" | ".nuxt" | ".svelte-kit" | ".turbo" | ".parcel-cache" | ".angular/cache" => root
            .join("package.json")
            .is_file()
            .then(|| ("build-cache", "the next build or dev run".to_string(), true)),
        "target" => root
            .join("Cargo.toml")
            .is_file()
            .then(|| ("rust-target", "cargo build".to_string(), true)),
        ".venv" | "venv" => (root.join("requirements.txt").is_file()
            || root.join("pyproject.toml").is_file()
            || root.join("Pipfile").is_file())
        .then(|| {
            (
                "venv",
                "Repair \u{2192} recreate the virtual environment".to_string(),
                false,
            )
        }),
        _ => None,
    }
}

/// Every candidate relative path that exists in the project root as a real folder.
fn existing_candidates(
    root: &Path,
    tracked: GitCheck,
) -> Vec<(String, &'static str, String, bool)> {
    let mut found = Vec::new();
    let mut rels: Vec<String> = ROOT_CANDIDATES.iter().map(|s| s.to_string()).collect();
    for group in ["packages", "apps"] {
        if let Ok(rd) = std::fs::read_dir(root.join(group)) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if is_plain_file_name(&name) && is_real_dir(&e.path()) {
                    rels.push(format!("{group}/{name}/node_modules"));
                }
            }
        }
    }
    for rel in rels {
        let full = rel.split('/').fold(root.to_path_buf(), |p, c| p.join(c));
        // A link is listed too (it costs nothing and deleting it removes only the link), but
        // it has size 0 and is not a real folder.
        let Ok(md) = std::fs::symlink_metadata(&full) else {
            continue;
        };
        if !md.is_dir() && !is_link(&md) {
            continue;
        }
        if let Some((kind, rebuild, default)) = classify(root, &rel, tracked) {
            found.push((rel, kind, rebuild, default));
        }
    }
    found
}

// ------------------------------------------------------------------ size and age

/// Sum of file sizes under `path`, never entering a link. Stops at the time or entry limit and
/// says so (`true` = the number is a minimum).
pub fn dir_size(path: &Path, time_limit: Duration, entry_limit: u64) -> (u64, bool) {
    let start = Instant::now();
    let Ok(md) = std::fs::symlink_metadata(path) else {
        return (0, false);
    };
    if is_link(&md) {
        return (0, false);
    }
    let mut total = 0u64;
    let mut entries = 0u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            entries += 1;
            if entries > entry_limit || start.elapsed() > time_limit {
                return (total, true);
            }
            let Ok(md) = std::fs::symlink_metadata(e.path()) else {
                continue;
            };
            if is_link(&md) {
                continue;
            }
            if md.is_dir() {
                stack.push(e.path());
            } else {
                total += md.len();
            }
        }
    }
    (total, false)
}

fn secs_since(t: SystemTime) -> u64 {
    SystemTime::now()
        .duration_since(t)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Days since anything in the project (outside the rebuildable folders and `.git`) changed;
/// Git's `HEAD` and `index` count as activity.
pub fn idle_days(root: &Path) -> u64 {
    let skip: HashSet<&str> = ROOT_CANDIDATES
        .iter()
        .map(|s| s.split('/').next().unwrap_or(s))
        .chain([".git"])
        .collect();
    let mut newest: Option<SystemTime> = None;
    let mut see = |t: Option<SystemTime>| {
        if let Some(t) = t {
            if newest.is_none_or(|n| t > n) {
                newest = Some(t);
            }
        }
    };
    if let Ok(rd) = std::fs::read_dir(root) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if skip.contains(name.as_str()) {
                continue;
            }
            see(e.metadata().and_then(|m| m.modified()).ok());
        }
    }
    for f in ["HEAD", "index"] {
        see(std::fs::metadata(root.join(".git").join(f))
            .and_then(|m| m.modified())
            .ok());
    }
    newest.map(|t| secs_since(t) / 86_400).unwrap_or(0)
}

// ------------------------------------------------------------------ scan

impl Inner {
    fn git_for_reclaim(&self) -> Option<PathBuf> {
        self.git_path()
    }

    /// Scans every registered project. Run it as a background task.
    pub fn scan_reclaim(&self, h: &crate::tasks::TaskHandle) -> ReclaimReport {
        let projects = self.projects.lock().unwrap().list();
        let git = self.git_for_reclaim();
        let tracked = |root: &Path, file: &str| tracked_by_git(git.as_deref(), root, file);
        h.begin_step(&format!("Scanning {} projects", projects.len()));
        let mut report = ReclaimReport {
            scanned_ms: now_ms(),
            projects: Vec::new(),
            total_bytes: 0,
        };
        for p in projects {
            if h.cancel_requested() {
                break;
            }
            h.advance(1);
            let root = PathBuf::from(&p.path);
            if !is_real_dir(&root) {
                continue;
            }
            let mut items = Vec::new();
            for (relative, kind, rebuild, default_selected) in existing_candidates(&root, &tracked)
            {
                let full = relative.split('/').fold(root.clone(), |a, c| a.join(c));
                let (bytes, size_is_minimum) = dir_size(&full, SIZE_TIME_LIMIT, SIZE_ENTRY_LIMIT);
                report.total_bytes += bytes;
                items.push(ReclaimCandidate {
                    relative,
                    kind: kind.to_string(),
                    bytes,
                    size_is_minimum,
                    rebuild,
                    default_selected,
                });
            }
            if items.is_empty() {
                continue;
            }
            report.projects.push(ReclaimProject {
                busy: self.supervisor.any_alive_in_dir(&root),
                idle_days: idle_days(&root),
                project_id: p.id,
                name: p.name,
                path: p.path,
                items,
            });
        }
        report
            .projects
            .sort_by(|a, b| b.idle_days.cmp(&a.idle_days).then(a.name.cmp(&b.name)));
        h.end_step(Some(format!(
            "{} projects have folders to clear",
            report.projects.len()
        )));
        report
    }

    /// Deletes the chosen folders. Everything is checked again here. Returns bytes freed.
    pub fn reclaim_folders(&self, items: &[ReclaimItem], h: &crate::tasks::TaskHandle) -> u64 {
        let git = self.git_for_reclaim();
        let tracked = |root: &Path, file: &str| tracked_by_git(git.as_deref(), root, file);
        h.begin_step(&format!("Deleting {} folders", items.len()));
        let mut freed = 0u64;
        for item in items {
            if h.cancel_requested() {
                break;
            }
            h.advance(1);
            let label = format!("{} / {}", item.project_id, item.relative);
            let root = match self.projects.lock().unwrap().get(&item.project_id) {
                Some(p) => PathBuf::from(p.path),
                None => {
                    h.problem(format!("{label}: that project is not registered anymore."));
                    continue;
                }
            };
            if self.supervisor.any_alive_in_dir(&root) {
                h.problem(format!(
                    "{label}: the project has running processes. Stop them first."
                ));
                continue;
            }
            let size = {
                let full = item
                    .relative
                    .split('/')
                    .fold(root.clone(), |a, c| a.join(c));
                dir_size(&full, SIZE_TIME_LIMIT, SIZE_ENTRY_LIMIT).0
            };
            match delete_candidate(&root, &item.relative, &tracked) {
                Ok(()) => {
                    freed += size;
                    h.result(format!("Deleted {}", item.relative));
                }
                Err(e) => h.problem(format!("{}: {e}", item.relative)),
            }
        }
        h.end_step(Some(format!("Freed {} bytes", freed)));
        freed
    }
}

/// Validates `relative` against the closed list, checks it is inside `root`, and removes it.
pub fn delete_candidate(root: &Path, relative: &str, tracked: GitCheck) -> Result<(), String> {
    // Plain components only: no `..`, no absolute paths, no drive letters, no backslash tricks.
    if relative.is_empty() || relative.contains(['\\', ':', '\0']) || relative.starts_with('/') {
        return Err("that folder is not on the list of folders OLS can remove.".into());
    }
    let comps: Vec<&str> = relative.split('/').collect();
    if comps
        .iter()
        .any(|c| c.is_empty() || *c == "." || *c == "..")
    {
        return Err("that folder is not on the list of folders OLS can remove.".into());
    }
    if classify(root, relative, tracked).is_none() {
        return Err("that folder is not on the list of folders OLS can remove, or it no longer meets the conditions (for example Git tracks it).".into());
    }
    let full = comps.iter().fold(root.to_path_buf(), |p, c| p.join(c));
    let root_canon = std::fs::canonicalize(root).map_err(|e| e.to_string())?;
    let md = match std::fs::symlink_metadata(&full) {
        Ok(md) => md,
        Err(_) => return Ok(()), // already gone
    };
    // The folder's parent must be inside the project even when the folder itself is a link.
    let parent = full.parent().ok_or("no parent folder")?;
    let parent_canon = std::fs::canonicalize(parent).map_err(|e| e.to_string())?;
    if !parent_canon.starts_with(&root_canon) {
        return Err("that folder is outside the project.".into());
    }
    if is_link(&md) {
        // Remove only the link, never what it points at.
        return std::fs::remove_dir(&full)
            .or_else(|_| std::fs::remove_file(&full))
            .map_err(|e| format!("could not remove the link: {e}"));
    }
    if !md.is_dir() {
        return Err("that is not a folder.".into());
    }
    let full_canon = std::fs::canonicalize(&full).map_err(|e| e.to_string())?;
    if !full_canon.starts_with(&root_canon) {
        return Err("that folder is outside the project.".into());
    }
    // `remove_dir_all` (Rust 1.58+) does not follow symlinks or junctions inside the folder:
    // it removes the link itself and leaves its target alone.
    std::fs::remove_dir_all(&full)
        .map_err(|e| format!("some files could not be removed (a file may be in use): {e}"))
}

/// Used by the dispatcher to build the closed-list error for a bad path early.
pub fn validate_relative(relative: &str) -> Result<(), CoreError> {
    let ok = !relative.is_empty()
        && !relative.contains(['\\', ':', '\0'])
        && Path::new(relative)
            .components()
            .all(|c| matches!(c, Component::Normal(_)));
    if ok {
        Ok(())
    } else {
        Err(CoreError::failed_fix(
            "OLS cannot remove that folder.",
            format!("\"{relative}\" is not one of the folders OLS knows how to rebuild."),
            "Pick folders from the list shown by Free up space.",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_git(_: &Path, _: &str) -> bool {
        false
    }
    fn all_tracked(_: &Path, _: &str) -> bool {
        true
    }

    fn project(dir: &Path) {
        std::fs::write(dir.join("package.json"), "{}").unwrap();
        std::fs::create_dir_all(dir.join("node_modules/x")).unwrap();
        std::fs::write(dir.join("node_modules/x/a.js"), "1234").unwrap();
    }

    #[test]
    fn node_modules_needs_package_json() {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(t.path().join("node_modules")).unwrap();
        assert!(classify(t.path(), "node_modules", &no_git).is_none());
        std::fs::write(t.path().join("package.json"), "{}").unwrap();
        assert!(classify(t.path(), "node_modules", &no_git).is_some());
    }

    #[test]
    fn vendor_needs_autoload_and_must_not_be_tracked() {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join("composer.json"), "{}").unwrap();
        std::fs::write(t.path().join("composer.lock"), "{}").unwrap();
        std::fs::create_dir_all(t.path().join("vendor")).unwrap();
        assert!(
            classify(t.path(), "vendor", &no_git).is_none(),
            "no autoload.php"
        );
        std::fs::write(t.path().join("vendor/autoload.php"), "<?php").unwrap();
        assert!(classify(t.path(), "vendor", &no_git).is_some());
        std::fs::create_dir_all(t.path().join(".git")).unwrap();
        assert!(
            classify(t.path(), "vendor", &all_tracked).is_none(),
            "tracked by Git"
        );
        assert!(classify(t.path(), "vendor", &no_git).is_some());
    }

    #[test]
    fn bad_paths_are_rejected() {
        let t = tempfile::tempdir().unwrap();
        project(t.path());
        for bad in [
            "..\\x",
            "C:\\x",
            "../x",
            "/node_modules",
            "packages/a/b/node_modules",
            "src",
            "",
            "node_modules/x",
            "packages/../node_modules",
        ] {
            assert!(
                delete_candidate(t.path(), bad, &no_git).is_err(),
                "{bad} must be refused"
            );
        }
        assert!(t.path().join("node_modules/x/a.js").is_file());
    }

    #[test]
    fn workspace_package_node_modules_is_found_one_level_deep() {
        let t = tempfile::tempdir().unwrap();
        let pkg = t.path().join("packages/a");
        std::fs::create_dir_all(pkg.join("node_modules")).unwrap();
        std::fs::write(pkg.join("package.json"), "{}").unwrap();
        let found = existing_candidates(t.path(), &no_git);
        assert!(found.iter().any(|f| f.0 == "packages/a/node_modules"));
    }

    #[test]
    fn size_counts_files_and_delete_removes_the_folder() {
        let t = tempfile::tempdir().unwrap();
        project(t.path());
        let (n, min) = dir_size(
            &t.path().join("node_modules"),
            SIZE_TIME_LIMIT,
            SIZE_ENTRY_LIMIT,
        );
        assert_eq!((n, min), (4, false));
        let (_, min) = dir_size(&t.path().join("node_modules"), SIZE_TIME_LIMIT, 1);
        assert!(min, "entry limit reports a minimum");
        delete_candidate(t.path(), "node_modules", &no_git).unwrap();
        assert!(!t.path().join("node_modules").exists());
        assert!(t.path().join("package.json").is_file());
    }

    #[cfg(windows)]
    fn junction(link: &Path, target: &Path) {
        let out = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .unwrap();
        assert!(out.status.success(), "mklink failed: {out:?}");
    }

    #[cfg(windows)]
    #[test]
    fn a_junction_inside_node_modules_is_never_followed() {
        let t = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("precious.txt"), "keep me").unwrap();
        let proj = t.path().join("proj");
        std::fs::create_dir_all(proj.join("node_modules")).unwrap();
        std::fs::write(proj.join("package.json"), "{}").unwrap();
        junction(&proj.join("node_modules").join("linked"), outside.path());
        let (bytes, _) = dir_size(
            &proj.join("node_modules"),
            SIZE_TIME_LIMIT,
            SIZE_ENTRY_LIMIT,
        );
        assert_eq!(bytes, 0, "the outside file must not be counted");
        delete_candidate(&proj, "node_modules", &no_git).unwrap();
        assert!(!proj.join("node_modules").exists());
        assert!(
            outside.path().join("precious.txt").is_file(),
            "outside file survives"
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_node_modules_junction_removes_only_the_link() {
        let t = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("precious.txt"), "keep me").unwrap();
        let proj = t.path().join("proj");
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(proj.join("package.json"), "{}").unwrap();
        junction(&proj.join("node_modules"), outside.path());
        delete_candidate(&proj, "node_modules", &no_git).unwrap();
        assert!(!proj.join("node_modules").exists());
        assert!(outside.path().join("precious.txt").is_file());
    }

    #[test]
    fn idle_days_ignores_rebuildable_folders() {
        let t = tempfile::tempdir().unwrap();
        project(t.path());
        // A just-written package.json means "used today".
        assert_eq!(idle_days(t.path()), 0);
    }
}
