//! `.devforge/environment.yaml` reader (§71 — Stage 4 slice). A project's own manifest
//! always wins over detected requirements (§11 — explicit beats guessed).

use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuntimeManifest {
    pub php: Option<String>,
    pub node: Option<String>,
    pub python: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EnvironmentManifest {
    pub name: Option<String>,
    #[serde(default)]
    pub runtime: RuntimeManifest,
}

/// Reads `<project>/.devforge/environment.yaml`. Returns `None` if the project has no
/// manifest (most won't yet) or it fails to parse — a malformed manifest should surface
/// as a diagnostic at the call site, not silently fall back, so this stays a `Result`.
pub fn read_manifest(project_path: &Path) -> Result<Option<EnvironmentManifest>, String> {
    let manifest_path = project_path.join(".devforge").join("environment.yaml");
    if !manifest_path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&manifest_path).map_err(|e| e.to_string())?;
    let manifest: EnvironmentManifest = serde_yaml_ng::from_str(&text).map_err(|e| e.to_string())?;
    Ok(Some(manifest))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_runtime_versions_from_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let devforge_dir = tmp.path().join(".devforge");
        std::fs::create_dir_all(&devforge_dir).unwrap();
        std::fs::write(
            devforge_dir.join("environment.yaml"),
            "name: shop\nruntime:\n  php: \"8.1\"\n  node: \"20\"\n",
        )
        .unwrap();

        let manifest = read_manifest(tmp.path()).unwrap().expect("manifest present");
        assert_eq!(manifest.name.as_deref(), Some("shop"));
        assert_eq!(manifest.runtime.php.as_deref(), Some("8.1"));
        assert_eq!(manifest.runtime.node.as_deref(), Some("20"));
    }

    #[test]
    fn returns_none_when_no_manifest_file() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(read_manifest(tmp.path()).unwrap().is_none());
    }

    #[test]
    fn malformed_manifest_is_an_error_not_a_silent_fallback() {
        let tmp = tempfile::tempdir().unwrap();
        let devforge_dir = tmp.path().join(".devforge");
        std::fs::create_dir_all(&devforge_dir).unwrap();
        std::fs::write(devforge_dir.join("environment.yaml"), "runtime: [this, is, not, a, map]").unwrap();

        assert!(read_manifest(tmp.path()).is_err());
    }
}
