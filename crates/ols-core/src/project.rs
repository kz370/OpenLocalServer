//! Project Manager (§40 — Stage 4). A project is a folder OpenLocalServer knows about, plus
//! whatever OpenLocalServer can read from it. Registering a project never writes into it.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::custom_install::CustomInstallStore;
use crate::detection::{self, DetectionResult};
use crate::error::CoreError;
use crate::manifest::{self, EnvironmentManifest};
use crate::paths::AppPaths;
use crate::resolver::{self, ResolutionSource, ResolvedRuntime};
use crate::runtime::RuntimeManager;
use crate::settings::SettingsService;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectDetail {
    pub project: Project,
    pub detection: DetectionResult,
    pub manifest: Option<EnvironmentManifest>,
    pub resolved: Vec<ResolvedRuntime>,
}

/// Strips Windows' `\\?\` extended-length path prefix, if present. A no-op on any other
/// shape of path (including non-Windows paths, which never have this prefix).
fn strip_verbatim_prefix(path: &str) -> String {
    path.strip_prefix(r"\\?\").unwrap_or(path).to_string()
}

pub struct ProjectStore {
    file: PathBuf,
    projects: Vec<Project>,
}

impl ProjectStore {
    pub fn load(paths: &AppPaths) -> Result<Self, CoreError> {
        paths.ensure_dirs()?;
        let file = paths.data_dir().join("projects.json");
        let projects = if file.exists() {
            let raw = std::fs::read_to_string(&file)?;
            serde_json::from_str(&raw).unwrap_or_default()
        } else {
            Vec::new()
        };
        Ok(Self { file, projects })
    }

    pub fn list(&self) -> Vec<Project> {
        self.projects.clone()
    }

    pub fn get(&self, id: &str) -> Option<Project> {
        self.projects.iter().find(|p| p.id == id).cloned()
    }

    /// Registers an existing folder. Idempotent — registering the same path twice
    /// returns the existing project rather than creating a duplicate.
    pub fn register(&mut self, path: &str) -> Result<Project, CoreError> {
        let path_buf = PathBuf::from(path);
        if !path_buf.is_dir() {
            return Err(CoreError::InvalidProjectPath(path.to_string()));
        }
        let canonical = std::fs::canonicalize(&path_buf).unwrap_or(path_buf.clone());
        // `canonicalize()` on Windows returns a `\\?\`-prefixed "verbatim" path — correct
        // for the Win32 API, but ugly (and confusing, showing up as stray "?" characters)
        // anywhere it's displayed or fed back into a normal command line. Strip it.
        let canonical_str = strip_verbatim_prefix(&canonical.display().to_string());

        if let Some(existing) = self.projects.iter().find(|p| p.path == canonical_str) {
            return Ok(existing.clone());
        }

        let name = canonical
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("project")
            .to_string();
        let mut hasher = Sha256::new();
        hasher.update(canonical_str.as_bytes());
        let id = format!("{:x}", hasher.finalize())[..16].to_string();

        let project = Project { id, name, path: canonical_str };
        self.projects.push(project.clone());
        self.persist()?;
        Ok(project)
    }

    /// A display name other than the folder's (clones and imports choose their own).
    pub fn rename(&mut self, id: &str, name: &str) -> Result<Project, CoreError> {
        let p = self.projects.iter_mut().find(|p| p.id == id).ok_or_else(|| CoreError::InvalidProjectPath(id.to_string()))?;
        p.name = name.to_string();
        let out = p.clone();
        self.persist()?;
        Ok(out)
    }

    pub fn remove(&mut self, id: &str) -> Result<(), CoreError> {
        self.projects.retain(|p| p.id != id);
        self.persist()
    }

    fn persist(&self) -> Result<(), CoreError> {
        let raw = serde_json::to_string_pretty(&self.projects)?;
        let tmp = self.file.with_extension("json.tmp");
        std::fs::write(&tmp, raw)?;
        std::fs::rename(&tmp, &self.file)?;
        Ok(())
    }
}

/// Combines detection, the project's manifest (if any), and runtime resolution into the
/// one payload the UI needs for a project's page (§18 resolution order, §42–43 detection).
pub fn build_detail(
    project: &Project,
    runtimes: &RuntimeManager,
    settings: &SettingsService,
    custom_installs: &CustomInstallStore,
) -> ProjectDetail {
    let path = PathBuf::from(&project.path);
    let detection = detection::detect(&path);
    let manifest = manifest::read_manifest(&path).unwrap_or(None);

    let global = |id: &str| -> Option<String> {
        settings
            .get(&format!("runtime.{id}.global"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    };

    let mut resolved = Vec::new();
    for id in ["php", "node", "python"] {
        let manifest_version = manifest.as_ref().and_then(|m| match id {
            "php" => m.runtime.php.as_deref(),
            "node" => m.runtime.node.as_deref(),
            "python" => m.runtime.python.as_deref(),
            _ => None,
        });
        let detected_version = match id {
            "php" => detection.requirements.php.as_deref(),
            "node" => detection.requirements.node.as_deref(),
            "python" => detection.requirements.python.as_deref(),
            _ => None,
        };
        let global_version = global(id);
        let mut r = resolver::resolve(id, manifest_version, detected_version, global_version.as_deref(), runtimes);

        // A user-pinned custom install always wins (§126).
        if let Some(custom) = custom_installs.resolve(id, r.requested_version.as_deref()) {
            r.source = ResolutionSource::Custom;
            r.requested_version = Some(if custom.label.is_empty() { "custom".to_string() } else { custom.label.clone() });
            r.installed_version = Some(r.requested_version.clone().unwrap());
            r.bin_dir = PathBuf::from(&custom.path).parent().map(|p| p.display().to_string());
        }

        resolved.push(r);
    }

    ProjectDetail { project: project.clone(), detection, manifest, resolved }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_rejects_a_path_that_is_not_a_directory() {
        let home = crate::test_support::isolated_home();
        let mut store = ProjectStore::load(&home.paths).unwrap();
        let bogus = home.paths.root().join("does-not-exist");
        let err = store.register(bogus.to_str().unwrap()).unwrap_err();
        assert!(matches!(err, CoreError::InvalidProjectPath(_)));
    }

    /// Regression test: Windows' `canonicalize()` returns a `\\?\`-prefixed verbatim
    /// path; that prefix must never leak into a stored project path or its displayed name.
    #[test]
    fn register_strips_windows_verbatim_path_prefix() {
        let home = crate::test_support::isolated_home();
        let mut store = ProjectStore::load(&home.paths).unwrap();
        let project_dir = home.paths.root().join("shop");
        std::fs::create_dir_all(&project_dir).unwrap();

        let project = store.register(project_dir.to_str().unwrap()).unwrap();
        assert!(!project.path.contains(r"\\?\"), "path leaked verbatim prefix: {}", project.path);
        assert!(!project.name.contains('?'), "name leaked verbatim prefix: {}", project.name);
        assert_eq!(project.name, "shop");
    }

    #[test]
    fn register_is_idempotent_for_the_same_path() {
        let home = crate::test_support::isolated_home();
        let mut store = ProjectStore::load(&home.paths).unwrap();
        let project_dir = home.paths.root().join("shop");
        std::fs::create_dir_all(&project_dir).unwrap();

        let first = store.register(project_dir.to_str().unwrap()).unwrap();
        let second = store.register(project_dir.to_str().unwrap()).unwrap();
        assert_eq!(first.id, second.id);
        assert_eq!(store.list().len(), 1);
    }

    #[test]
    fn register_persists_across_reloads() {
        let home = crate::test_support::isolated_home();
        let project_dir = home.paths.root().join("shop");
        std::fs::create_dir_all(&project_dir).unwrap();

        {
            let mut store = ProjectStore::load(&home.paths).unwrap();
            store.register(project_dir.to_str().unwrap()).unwrap();
        }
        let reloaded = ProjectStore::load(&home.paths).unwrap();
        assert_eq!(reloaded.list().len(), 1);
        assert_eq!(reloaded.list()[0].name, "shop");
    }

    #[test]
    fn remove_deletes_the_project() {
        let home = crate::test_support::isolated_home();
        let project_dir = home.paths.root().join("shop");
        std::fs::create_dir_all(&project_dir).unwrap();

        let mut store = ProjectStore::load(&home.paths).unwrap();
        let project = store.register(project_dir.to_str().unwrap()).unwrap();
        store.remove(&project.id).unwrap();
        assert!(store.list().is_empty());
    }

    #[test]
    fn build_detail_resolves_manifest_version_over_detected_over_global() {
        let home = crate::test_support::isolated_home();
        let project_dir = home.paths.root().join("shop");
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::write(project_dir.join("composer.json"), r#"{"require":{"php":"^8.1"}}"#).unwrap();
        let manifest_dir = project_dir.join(".openlocalserver");
        std::fs::create_dir_all(&manifest_dir).unwrap();
        std::fs::write(manifest_dir.join("environment.yaml"), "runtime:\n  php: \"8.3\"\n").unwrap();

        let mut store = ProjectStore::load(&home.paths).unwrap();
        let project = store.register(project_dir.to_str().unwrap()).unwrap();

        let runtimes = RuntimeManager::new(home.paths.clone());
        let settings = SettingsService::load(&home.paths).unwrap();
        let custom_installs = CustomInstallStore::load(&home.paths).unwrap();
        let detail = build_detail(&project, &runtimes, &settings, &custom_installs);

        let php = detail.resolved.iter().find(|r| r.id == "php").unwrap();
        // The manifest says 8.3; composer.json's detected 8.1 must lose.
        assert_eq!(php.requested_version.as_deref(), Some("8.3"));
        assert_eq!(php.source, resolver::ResolutionSource::Manifest);
    }

    /// A user-pinned custom install must win even over an explicit manifest version —
    /// it's the most explicit choice possible (§126).
    #[test]
    fn build_detail_prefers_a_matching_custom_install_over_the_manifest() {
        let home = crate::test_support::isolated_home();
        let project_dir = home.paths.root().join("shop");
        std::fs::create_dir_all(&project_dir).unwrap();
        let manifest_dir = project_dir.join(".openlocalserver");
        std::fs::create_dir_all(&manifest_dir).unwrap();
        std::fs::write(manifest_dir.join("environment.yaml"), "runtime:\n  php: \"8.1\"\n").unwrap();

        let custom_php = home.paths.root().join("custom-php.exe");
        std::fs::write(&custom_php, b"fake").unwrap();

        let mut store = ProjectStore::load(&home.paths).unwrap();
        let project = store.register(project_dir.to_str().unwrap()).unwrap();
        let runtimes = RuntimeManager::new(home.paths.clone());
        let settings = SettingsService::load(&home.paths).unwrap();
        let mut custom_installs = CustomInstallStore::load(&home.paths).unwrap();
        custom_installs.set("php", "8.1", custom_php.to_str().unwrap()).unwrap();

        let detail = build_detail(&project, &runtimes, &settings, &custom_installs);
        let php = detail.resolved.iter().find(|r| r.id == "php").unwrap();
        assert_eq!(php.source, resolver::ResolutionSource::Custom);
        assert_eq!(php.bin_dir.as_deref(), Some(home.paths.root().display().to_string().as_str()));
    }
}
