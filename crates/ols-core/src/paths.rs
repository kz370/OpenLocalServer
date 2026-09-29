//! Application paths, honoring OS conventions with an `OLS_HOME` override for tests (§161 test isolation).

use std::path::{Path, PathBuf};

use directories::ProjectDirs;

/// Environment variable that overrides every OLS path.
/// Used by tests and CI so nothing touches a real machine's actual environment.
pub const HOME_ENV_VAR: &str = "OLS_HOME";

#[derive(Debug, Clone)]
pub struct AppPaths {
    root: PathBuf,
}

impl AppPaths {
    /// Resolve paths. Order:
    /// 1. `OLS_HOME` (tests, CI): everything lives under that directory.
    /// 2. Debug builds: `<repo>/data`, so `cargo clean` never wipes it.
    /// 3. A `data` folder beside the executable (portable, like Laragon: reinstalling
    ///    Windows loses nothing when the install lives on another drive).
    /// 4. The OS app-data directory, only when the install folder isn't writable
    ///    (for example under Program Files).
    pub fn resolve() -> Self {
        if let Ok(override_home) = std::env::var(HOME_ENV_VAR) {
            return Self {
                root: PathBuf::from(override_home),
            };
        }
        #[cfg(debug_assertions)]
        {
            let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("..")
                .join("data");
            if let Some(root) = writable_dir(&repo) {
                return Self { root };
            }
        }
        let beside_exe = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|d| d.join("data")));
        if let Some(root) = beside_exe.and_then(|d| writable_dir(&d)) {
            return Self { root };
        }
        Self {
            root: Self::legacy_root()
                .expect("could not determine a home directory for the current user"),
        }
    }

    /// Where earlier versions kept everything (the OS app-data directory).
    pub fn legacy_root() -> Option<PathBuf> {
        ProjectDirs::from("dev", "OpenLocalServer", "OpenLocalServer")
            .map(|d| d.data_dir().to_path_buf())
    }

    /// One-time move of data left in the old app-data location into the current root, so
    /// nothing has to be reinstalled. Only for the default (non-`OLS_HOME`) layout and only
    /// while the new location is still empty. Downloaded archives (`cache`) stay behind,
    /// since they can be fetched again. Returns notes for the log.
    pub fn migrate_legacy(&self) -> Vec<String> {
        let mut notes = Vec::new();
        if std::env::var(HOME_ENV_VAR).is_ok() {
            return notes;
        }
        let Some(legacy) = Self::legacy_root() else {
            return notes;
        };
        if !legacy.is_dir() || same_path(&legacy, &self.root) {
            return notes;
        }
        if ["app.db", "runtimes"]
            .iter()
            .any(|n| self.root.join(n).exists())
        {
            return notes;
        }
        let entries: Vec<_> = match std::fs::read_dir(&legacy) {
            Ok(rd) => rd.flatten().filter(|e| e.file_name() != "cache").collect(),
            Err(_) => return notes,
        };
        if entries.is_empty() {
            return notes;
        }
        let _ = std::fs::create_dir_all(&self.root);
        notes.push(format!(
            "moving data from {} to {}",
            legacy.display(),
            self.root.display()
        ));
        for entry in entries {
            let from = entry.path();
            let to = self.root.join(entry.file_name());
            if std::fs::rename(&from, &to).is_ok() {
                continue;
            }
            // Different drive: copy, and delete the original only once the copy is complete.
            match copy_recursive(&from, &to) {
                Ok(()) => {
                    let _ = if from.is_dir() {
                        std::fs::remove_dir_all(&from)
                    } else {
                        std::fs::remove_file(&from)
                    };
                }
                Err(e) => notes.push(format!(
                    "could not move {}: {e} (left in place)",
                    from.display()
                )),
            }
        }
        notes.push("data move finished".into());
        notes
    }

    /// Paths rooted at an explicit directory — for tests that need a second, separate home.
    pub fn resolve_at(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }

    pub fn data_dir(&self) -> PathBuf {
        // `root` already resolves to the OS-convention app-data directory (§146); it IS the
        // data dir, not a parent that needs a "data" child.
        self.root.clone()
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.root.join("cache")
    }

    pub fn runtimes_dir(&self) -> PathBuf {
        self.root.join("runtimes")
    }

    pub fn services_dir(&self) -> PathBuf {
        self.root.join("services")
    }

    /// Root of everything the local CA and per-site leaf certs live in (§146).
    pub fn certs_dir(&self) -> PathBuf {
        self.root.join("certificates")
    }

    /// Generated + user-edited web-server config, its history, and per-server prefixes.
    pub fn web_dir(&self) -> PathBuf {
        self.root.join("web")
    }

    pub fn quick_apps_dir(&self) -> PathBuf {
        self.root.join("quick-apps")
    }

    pub fn sites_dir(&self) -> PathBuf {
        if std::env::var(HOME_ENV_VAR).is_ok() {
            return self.root.join("sites");
        }
        #[cfg(debug_assertions)]
        {
            return self.root.join("sites");
        }
        #[cfg(not(debug_assertions))]
        {
            let beside_exe = std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(|d| d.join("sites")));
            beside_exe
                .and_then(|dir| writable_dir(&dir))
                .unwrap_or_else(|| {
                    ProjectDirs::from("dev", "OpenLocalServer", "OpenLocalServer")
                        .map(|d| d.data_dir().join("Sites"))
                        .unwrap_or_else(|| self.root.join("Sites"))
                })
        }
    }

    /// Database dumps, one folder per engine (§32, §34).
    pub fn backups_dir(&self) -> PathBuf {
        self.root.join("backups")
    }

    pub fn settings_file(&self) -> PathBuf {
        self.data_dir().join("settings.json")
    }

    /// Single SQLite file replacing all `*.json` stores (fresh only, no JSON import).
    pub fn db_file(&self) -> PathBuf {
        self.data_dir().join("app.db")
    }

    /// Create every directory this struct points at. Idempotent.
    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        for dir in [
            self.root.clone(),
            self.logs_dir(),
            self.data_dir(),
            self.cache_dir(),
            self.runtimes_dir(),
            self.services_dir(),
            self.certs_dir(),
            self.web_dir(),
            self.quick_apps_dir(),
            self.backups_dir(),
            self.sites_dir(),
        ] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }

    // ------------------------------------------------- portable site roots (§ portability)
    //
    // A site the user made inside the app's own `sites` folder belongs to the app, not to
    // the drive it happens to sit on: storing its absolute root meant that renaming the
    // install folder, or moving it to another drive, turned every such site into
    // "folder does not exist" even though its files moved along with the app. Those roots
    // are stored relative to a fixed prefix instead, and expanded again on load. A root
    // outside the app (a real project folder) stays absolute — that path is the user's to
    // own, and the app has no business rewriting it.

    /// Prefix marking a stored root relative to the sites folder.
    const SITES_PREFIX: &'static str = "sites/";
    /// Prefix marking a stored root relative to the data folder.
    const DATA_PREFIX: &'static str = "data/";

    /// How a site root is written to the database: relative when it lives inside the app,
    /// absolute when it lives anywhere else.
    pub fn encode_root(&self, path: &Path) -> String {
        if let Some(rel) = strip_prefix_ci(path, &self.sites_dir()) {
            return format!("{}{}", Self::SITES_PREFIX, rel);
        }
        if let Some(rel) = strip_prefix_ci(path, &self.data_dir()) {
            return format!("{}{}", Self::DATA_PREFIX, rel);
        }
        path.display().to_string()
    }

    /// The folder a stored root points at, with an app-relative one re-anchored to where
    /// the app lives now.
    pub fn decode_root(&self, stored: &str) -> PathBuf {
        // The stored form uses forward slashes so it is readable and identical on every
        // platform; joining one onto a Windows base would otherwise leave a path spelled
        // `C:\app/data/runtimes/php\php.exe` that still works but reads as two different
        // folders to a user and to anything comparing the string.
        let rest = stored
            .strip_prefix(Self::SITES_PREFIX)
            .map(|r| join_relative(&self.sites_dir(), r))
            .or_else(|| {
                stored
                    .strip_prefix(Self::DATA_PREFIX)
                    .map(|r| join_relative(&self.root, r))
            });
        match rest {
            Some(p) => p,
            // Absolute: it was written by this app, so it is the app's own folder that
            // moved. Re-anchor it to the same relative spot under the current install, and
            // keep the stored value when nothing there exists — an install whose data was
            // deleted has to keep reporting the site as missing rather than silently
            // pointing somewhere else.
            None => self.reanchor_root(stored),
        }
    }

    /// Finds the site root that moved with the app. The old absolute path still ends in
    /// `sites/<name>` or `data/<name>`, so that tail is looked for under the current
    /// install; an unrecognizable path is returned unchanged.
    fn reanchor_root(&self, stored: &str) -> PathBuf {
        let original = PathBuf::from(stored);
        if original.is_dir() {
            return original;
        }
        let components: Vec<_> = original
            .components()
            .filter_map(|c| c.as_os_str().to_str())
            .collect();
        let tail = components
            .iter()
            .rposition(|c| {
                c.eq_ignore_ascii_case("sites")
                    || c.eq_ignore_ascii_case("data")
                    || c.eq_ignore_ascii_case("home")
            })
            .map(|i| components[i..].join("/"));
        let Some(tail) = tail else {
            return original;
        };
        for base in [self.sites_dir(), self.data_dir()] {
            let candidate = join_relative(&base, &tail);
            if candidate.is_dir() {
                return candidate;
            }
        }
        original
    }
}

/// Appends a stored, forward-slash-separated relative path to `base`, one segment at a
/// time. Joining the whole string at once would leave `C:\app/data/runtimes/php\php.exe`
/// on Windows: it still resolves, but a path spelled with two different separators reads
/// as two folders to a user and to anything comparing the string.
fn join_relative(base: &Path, rel: &str) -> PathBuf {
    rel.split('/')
        .filter(|s| !s.is_empty())
        .fold(base.to_path_buf(), |p, seg| p.join(seg))
}

/// `path` relative to `base`, as forward slashes, when it really is inside it.
/// Comparison is case-insensitive: Windows paths are, and two spellings of one folder
/// must not read as "outside the app" and get frozen into the database.
fn strip_prefix_ci(path: &Path, base: &Path) -> Option<String> {
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let base = std::fs::canonicalize(base).unwrap_or_else(|_| base.to_path_buf());
    let rel = path.strip_prefix(&base).ok()?;
    let rel = rel.to_str()?.replace('\\', "/");
    if rel.is_empty() || rel == "." {
        return None;
    }
    Some(rel)
}

/// `dir` (created if needed) when files can actually be written there.
fn writable_dir(dir: &Path) -> Option<PathBuf> {
    std::fs::create_dir_all(dir).ok()?;
    let probe = dir.join(".write-test");
    std::fs::write(&probe, b"").ok()?;
    let _ = std::fs::remove_file(probe);
    std::fs::canonicalize(dir).ok().map(strip_verbatim)
}

/// Drops the `\\?\` prefix canonicalize adds on Windows; nginx, Apache and PHP mishandle it.
fn strip_verbatim(p: PathBuf) -> PathBuf {
    match p.to_str().and_then(|s| s.strip_prefix(r"\\?\")) {
        Some(rest) => PathBuf::from(rest),
        None => p,
    }
}

fn same_path(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn copy_recursive(from: &Path, to: &Path) -> std::io::Result<()> {
    if from.is_dir() {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(from, to).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_env_var_wins() {
        let home = crate::test_support::isolated_home();
        assert_eq!(
            std::env::var(HOME_ENV_VAR).unwrap(),
            home.paths.root().to_str().unwrap()
        );
    }

    #[test]
    fn ensure_dirs_creates_tree() {
        let home = crate::test_support::isolated_home();
        home.paths.ensure_dirs().unwrap();
        assert!(home.paths.logs_dir().is_dir());
        assert!(home.paths.data_dir().is_dir());
        assert!(home.paths.sites_dir().is_dir());
    }
}
