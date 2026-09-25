//! Application paths, honoring OS conventions with an `OLS_HOME` override for tests (§161 test isolation).

use std::path::{Path, PathBuf};

use directories::ProjectDirs;

/// Environment variable that overrides every OpenLocalServer path.
/// Used by tests and CI so nothing touches a real machine's actual environment.
pub const HOME_ENV_VAR: &str = "OLS_HOME";

#[derive(Debug, Clone)]
pub struct AppPaths {
    root: PathBuf,
}

impl AppPaths {
    /// Resolve paths. If `OLS_HOME` is set, everything lives under that directory
    /// (flat layout, convenient for tests). Otherwise use OS-convention dirs.
    pub fn resolve() -> Self {
        if let Ok(override_home) = std::env::var(HOME_ENV_VAR) {
            return Self {
                root: PathBuf::from(override_home),
            };
        }

        let dirs = ProjectDirs::from("dev", "OpenLocalServer", "OpenLocalServer")
            .expect("could not determine a home directory for the current user");
        Self {
            root: dirs.data_dir().to_path_buf(),
        }
    }

    /// Paths rooted at an explicit directory — for tests that need a second, separate home.
    pub fn resolve_at(root: &Path) -> Self {
        Self { root: root.to_path_buf() }
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

    pub fn settings_file(&self) -> PathBuf {
        self.data_dir().join("settings.json")
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
        ] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_env_var_wins() {
        let home = crate::test_support::isolated_home();
        assert_eq!(std::env::var(HOME_ENV_VAR).unwrap(), home.paths.root().to_str().unwrap());
    }

    #[test]
    fn ensure_dirs_creates_tree() {
        let home = crate::test_support::isolated_home();
        home.paths.ensure_dirs().unwrap();
        assert!(home.paths.logs_dir().is_dir());
        assert!(home.paths.data_dir().is_dir());
    }
}
