//! User-pinned custom install locations (Stage 5 addition, user-requested): when
//! OpenLocalServer can't find or doesn't manage a tool/runtime, the user points straight at
//! where it already lives instead of only ever being offered a download. Always wins
//! over auto-detection — it's an explicit choice (same precedent as manifest > detected
//! in the Environment Resolver, §18).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::CoreError;
use crate::paths::AppPaths;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CustomInstall {
    /// "heidisql" | "pgadmin" | "php" | "node" | "python" | ...
    pub id: String,
    /// A version label for versioned runtimes ("8.1"); empty for single-path tools.
    pub label: String,
    /// Full path to the executable itself.
    pub path: String,
}

#[derive(Debug, Clone)]
pub struct CustomInstallStore {
    paths: AppPaths,
    entries: Vec<CustomInstall>,
}

impl CustomInstallStore {
    pub fn load(paths: &AppPaths) -> Result<Self, CoreError> {
        let entries = crate::db::load_docs(paths, "custom_installs")?;
        Ok(Self {
            paths: paths.clone(),
            entries,
        })
    }

    pub fn list(&self) -> Vec<CustomInstall> {
        self.entries.clone()
    }

    pub fn find(&self, id: &str, label: &str) -> Option<&CustomInstall> {
        self.entries.iter().find(|e| e.id == id && e.label == label)
    }

    /// The resolution rule used wherever a custom install can satisfy a runtime request:
    /// an exact label match wins; then the newest label the request is a prefix of
    /// ("8.3" is satisfied by "8.3.30"); otherwise, if nothing specific was requested and
    /// exactly one custom install exists for `id`, that one is used (the common
    /// single-version-override case). Centralized here so every caller (Environment
    /// Resolver, the runtime-aware "terminal") agrees on the same rule.
    pub fn resolve(&self, id: &str, requested: Option<&str>) -> Option<&CustomInstall> {
        if let Some(v) = requested {
            if let Some(found) = self.find(id, v) {
                return Some(found);
            }
            let labels: Vec<String> = self
                .entries
                .iter()
                .filter(|e| e.id == id)
                .map(|e| e.label.clone())
                .collect();
            if let Some(best) = crate::php::pick_version(&labels, Some(v)) {
                return self.find(id, &best);
            }
        }
        let mut matches = self.entries.iter().filter(|e| e.id == id);
        let first = matches.next()?;
        if requested.is_none() && matches.next().is_none() {
            Some(first)
        } else {
            None
        }
    }

    /// Set (or replace) the custom path for `id`/`label`. Rejects a path that doesn't
    /// exist on disk — pointing at nothing helps no one.
    pub fn set(&mut self, id: &str, label: &str, path: &str) -> Result<(), CoreError> {
        if !PathBuf::from(path).is_file() {
            return Err(CoreError::InvalidProjectPath(path.to_string()));
        }
        self.entries.retain(|e| !(e.id == id && e.label == label));
        self.entries.push(CustomInstall {
            id: id.to_string(),
            label: label.to_string(),
            path: path.to_string(),
        });
        self.persist()
    }

    pub fn remove(&mut self, id: &str, label: &str) -> Result<(), CoreError> {
        self.entries.retain(|e| !(e.id == id && e.label == label));
        self.persist()
    }

    fn persist(&self) -> Result<(), CoreError> {
        let refs: Vec<(String, &CustomInstall)> = self
            .entries
            .iter()
            .map(|e| (format!("{}|{}", e.id, e.label), e))
            .collect();
        crate::db::save_docs(&self.paths, "custom_installs", &refs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_rejects_a_path_that_does_not_exist() {
        let home = crate::test_support::isolated_home();
        let mut store = CustomInstallStore::load(&home.paths).unwrap();
        let bogus = home.paths.root().join("nope.exe");
        let err = store
            .set("heidisql", "", bogus.to_str().unwrap())
            .unwrap_err();
        assert!(matches!(err, CoreError::InvalidProjectPath(_)));
    }

    #[test]
    fn set_then_find_round_trips_and_persists() {
        let home = crate::test_support::isolated_home();
        let exe = home.paths.root().join("php.exe");
        std::fs::write(&exe, b"fake").unwrap();

        {
            let mut store = CustomInstallStore::load(&home.paths).unwrap();
            store.set("php", "8.1", exe.to_str().unwrap()).unwrap();
        }
        let reloaded = CustomInstallStore::load(&home.paths).unwrap();
        let found = reloaded.find("php", "8.1").unwrap();
        assert_eq!(found.path, exe.to_str().unwrap());
    }

    #[test]
    fn resolve_matches_by_exact_label() {
        let home = crate::test_support::isolated_home();
        let exe = home.paths.root().join("php.exe");
        std::fs::write(&exe, b"fake").unwrap();
        let mut store = CustomInstallStore::load(&home.paths).unwrap();
        store.set("php", "8.1", exe.to_str().unwrap()).unwrap();

        assert_eq!(
            store.resolve("php", Some("8.1")).unwrap().path,
            exe.to_str().unwrap()
        );
        assert!(store.resolve("php", Some("8.3")).is_none());
    }

    /// A project asking for "8.3" must find a PHP registered under its full version.
    #[test]
    fn resolve_matches_a_minor_request_against_full_version_labels() {
        let home = crate::test_support::isolated_home();
        let old = home.paths.root().join("old.exe");
        let new = home.paths.root().join("new.exe");
        std::fs::write(&old, b"fake").unwrap();
        std::fs::write(&new, b"fake").unwrap();
        let mut store = CustomInstallStore::load(&home.paths).unwrap();
        store.set("php", "8.3.9", old.to_str().unwrap()).unwrap();
        store.set("php", "8.3.30", new.to_str().unwrap()).unwrap();

        assert_eq!(
            store.resolve("php", Some("8.3")).unwrap().label,
            "8.3.30",
            "newest 8.3.x wins"
        );
        assert_eq!(store.resolve("php", Some("8.3.9")).unwrap().label, "8.3.9");
        assert!(store.resolve("php", Some("8.4")).is_none());
    }

    #[test]
    fn resolve_falls_back_to_the_sole_entry_when_nothing_specific_requested() {
        let home = crate::test_support::isolated_home();
        let exe = home.paths.root().join("heidisql.exe");
        std::fs::write(&exe, b"fake").unwrap();
        let mut store = CustomInstallStore::load(&home.paths).unwrap();
        store.set("heidisql", "", exe.to_str().unwrap()).unwrap();

        assert_eq!(
            store.resolve("heidisql", None).unwrap().path,
            exe.to_str().unwrap()
        );
    }

    #[test]
    fn resolve_refuses_to_guess_between_multiple_candidates() {
        let home = crate::test_support::isolated_home();
        let exe_a = home.paths.root().join("a.exe");
        let exe_b = home.paths.root().join("b.exe");
        std::fs::write(&exe_a, b"fake").unwrap();
        std::fs::write(&exe_b, b"fake").unwrap();
        let mut store = CustomInstallStore::load(&home.paths).unwrap();
        store.set("php", "8.1", exe_a.to_str().unwrap()).unwrap();
        store.set("php", "8.3", exe_b.to_str().unwrap()).unwrap();

        assert!(
            store.resolve("php", None).is_none(),
            "ambiguous — must not silently pick one"
        );
    }

    #[test]
    fn setting_again_replaces_rather_than_duplicates() {
        let home = crate::test_support::isolated_home();
        let exe_a = home.paths.root().join("a.exe");
        let exe_b = home.paths.root().join("b.exe");
        std::fs::write(&exe_a, b"fake").unwrap();
        std::fs::write(&exe_b, b"fake").unwrap();

        let mut store = CustomInstallStore::load(&home.paths).unwrap();
        store.set("php", "8.1", exe_a.to_str().unwrap()).unwrap();
        store.set("php", "8.1", exe_b.to_str().unwrap()).unwrap();

        assert_eq!(store.list().len(), 1);
        assert_eq!(
            store.find("php", "8.1").unwrap().path,
            exe_b.to_str().unwrap()
        );
    }
}
