//! Automatic project requirement detection (§42–43 — Stage 4). Reads marker files a
//! project already has; never guesses, never modifies anything.

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Framework {
    Laravel,
    Symfony,
    WordPress,
    GenericPhp,
    Node,
    Django,
    Flask,
    FastApi,
    GenericPython,
    Unknown,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuntimeRequirement {
    pub php: Option<String>,
    pub node: Option<String>,
    pub python: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionResult {
    pub framework: Framework,
    /// Marker filenames that led to the detection, for the UI to show its work.
    pub markers: Vec<String>,
    pub requirements: RuntimeRequirement,
    /// Sub-folder to serve when the entry point isn't in the project root (`public` for
    /// Laravel-style layouts). `None` means serve the project folder itself.
    pub doc_root: Option<String>,
}

fn exists(root: &Path, name: &str) -> bool {
    root.join(name).is_file()
}

/// Marker files/dirs whose presence means "this folder is a project root" — used by the
/// folder scanner (§40's "register" workflow extended to a whole workspace at once).
/// Deliberately broader than `detect()`'s framework markers: `.git` alone counts here,
/// since a repo with no recognized framework is still very much a real project.
const PROJECT_MARKERS: &[&str] = &[
    "composer.json",
    "package.json",
    "artisan",
    "manage.py",
    "pyproject.toml",
    "requirements.txt",
    "go.mod",
    "wp-config.php",
];

pub fn looks_like_a_project(dir: &Path) -> bool {
    dir.join(".git").exists() || PROJECT_MARKERS.iter().any(|m| exists(dir, m))
}

/// Scans the immediate children of `root` (not recursive — a workspace folder full of
/// separate repos, not a search through every nested node_modules) for folders that look
/// like projects. If `root` itself looks like a project, it's included too.
pub fn scan_for_projects(root: &Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    if looks_like_a_project(root) {
        found.push(root.to_path_buf());
    }
    let Ok(entries) = std::fs::read_dir(root) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && looks_like_a_project(&path) {
            found.push(path);
        }
    }
    found
}

fn read_json(root: &Path, name: &str) -> Option<Value> {
    let text = std::fs::read_to_string(root.join(name)).ok()?;
    serde_json::from_str(&text).ok()
}

fn read_text(root: &Path, name: &str) -> Option<String> {
    std::fs::read_to_string(root.join(name)).ok()
}

/// Strips a leading version-constraint operator (`^`, `~`, `>=`, ...) so `"^8.2"` reads
/// as `"8.2"`. Deliberately not a full semver-range parser — good enough to show the
/// user "this project wants roughly PHP 8.2", which they can always override manually.
fn strip_constraint_prefix(raw: &str) -> String {
    raw.trim_start_matches(['^', '~', '>', '<', '=', ' ']).trim().to_string()
}

pub fn detect(project_path: &Path) -> DetectionResult {
    let mut markers = Vec::new();
    let mut requirements = RuntimeRequirement::default();

    // -- PHP family (§42): artisan → Laravel, symfony.lock → Symfony, wp-config.php →
    // WordPress, else a bare composer.json → generic PHP. Requirement extraction below is
    // deliberately NOT nested inside this chain: a project can have composer.json AND
    // package.json at once (Laravel + Vite is the common case), and both must still be
    // read even though only one framework label "wins" for display.
    let composer = read_json(project_path, "composer.json");
    let package_json = read_json(project_path, "package.json");
    if composer.is_some() {
        markers.push("composer.json".to_string());
    }
    if package_json.is_some() {
        markers.push("package.json".to_string());
    }
    if let Some(json) = &composer {
        if let Some(php_constraint) = json.pointer("/require/php").and_then(Value::as_str) {
            requirements.php = Some(strip_constraint_prefix(php_constraint));
        }
    }
    if let Some(pkg) = &package_json {
        if let Some(node_constraint) = pkg.pointer("/engines/node").and_then(Value::as_str) {
            requirements.node = Some(strip_constraint_prefix(node_constraint));
        }
    }

    let framework = if exists(project_path, "artisan") {
        markers.push("artisan".to_string());
        Framework::Laravel
    } else if exists(project_path, "symfony.lock") {
        markers.push("symfony.lock".to_string());
        Framework::Symfony
    } else if exists(project_path, "wp-config.php") {
        markers.push("wp-config.php".to_string());
        Framework::WordPress
    } else if composer.is_some() || exists(project_path, "index.php") || exists(project_path, "public/index.php") {
        Framework::GenericPhp
    } else if package_json.is_some() {
        Framework::Node
    } else if exists(project_path, "manage.py") {
        markers.push("manage.py".to_string());
        Framework::Django
    } else if exists(project_path, "pyproject.toml") || exists(project_path, "requirements.txt") {
        let marker = if exists(project_path, "pyproject.toml") { "pyproject.toml" } else { "requirements.txt" };
        markers.push(marker.to_string());
        let text = read_text(project_path, "pyproject.toml")
            .or_else(|| read_text(project_path, "requirements.txt"))
            .unwrap_or_default()
            .to_lowercase();
        if text.contains("fastapi") {
            Framework::FastApi
        } else if text.contains("flask") {
            Framework::Flask
        } else {
            Framework::GenericPython
        }
    } else {
        Framework::Unknown
    };

    // -- Python version hint, independent of which Python framework was matched. --
    if let Some(pyproject) = read_text(project_path, "pyproject.toml") {
        if let Some(line) = pyproject.lines().find(|l| l.trim_start().starts_with("requires-python")) {
            if let Some(value) = line.split('=').nth(1) {
                requirements.python = Some(strip_constraint_prefix(value.trim().trim_matches('"')));
            }
        }
    }

    let php_family = matches!(framework, Framework::Laravel | Framework::Symfony | Framework::GenericPhp);
    let doc_root = (php_family && !exists(project_path, "index.php") && exists(project_path, "public/index.php")).then(|| "public".to_string());

    DetectionResult { framework, markers, requirements, doc_root }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_laravel_via_artisan() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("artisan"), "#!/usr/bin/env php").unwrap();
        std::fs::write(tmp.path().join("composer.json"), r#"{"require":{"php":"^8.2"}}"#).unwrap();

        let result = detect(tmp.path());
        assert_eq!(result.framework, Framework::Laravel);
        assert_eq!(result.requirements.php.as_deref(), Some("8.2"));
        assert!(result.markers.contains(&"artisan".to_string()));
    }

    #[test]
    fn detects_node_and_its_engine_constraint() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("package.json"), r#"{"engines":{"node":">=22"}}"#).unwrap();

        let result = detect(tmp.path());
        assert_eq!(result.framework, Framework::Node);
        assert_eq!(result.requirements.node.as_deref(), Some("22"));
    }

    #[test]
    fn detects_django_via_managepy() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("manage.py"), "#!/usr/bin/env python").unwrap();

        let result = detect(tmp.path());
        assert_eq!(result.framework, Framework::Django);
    }

    /// Regression test: a project with both composer.json and package.json (Laravel +
    /// Vite is the common real-world case) must have BOTH requirements detected, even
    /// though only PHP's framework label "wins" for display.
    #[test]
    fn detects_both_php_and_node_requirements_when_both_marker_files_present() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("composer.json"), r#"{"require":{"php":"^8.4"}}"#).unwrap();
        std::fs::write(tmp.path().join("package.json"), r#"{"engines":{"node":"24"}}"#).unwrap();

        let result = detect(tmp.path());
        assert_eq!(result.framework, Framework::GenericPhp);
        assert_eq!(result.requirements.php.as_deref(), Some("8.4"));
        assert_eq!(result.requirements.node.as_deref(), Some("24"));
    }

    #[test]
    fn plain_php_folder_without_composer_is_generic_php() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("index.php"), "<?php echo 1;").unwrap();
        assert_eq!(detect(tmp.path()).framework, Framework::GenericPhp);

        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("public")).unwrap();
        std::fs::write(tmp.path().join("public").join("index.php"), "<?php echo 1;").unwrap();
        let found = detect(tmp.path());
        assert_eq!(found.framework, Framework::GenericPhp);
        assert_eq!(found.doc_root.as_deref(), Some("public"));
    }

    #[test]
    fn unknown_when_no_markers_present() {
        let tmp = tempfile::tempdir().unwrap();
        let result = detect(tmp.path());
        assert_eq!(result.framework, Framework::Unknown);
        assert!(result.markers.is_empty());
    }

    #[test]
    fn scan_finds_multiple_separate_project_folders_under_one_root() {
        let tmp = tempfile::tempdir().unwrap();
        let laravel = tmp.path().join("laravel-app");
        let node = tmp.path().join("node-app");
        let empty = tmp.path().join("just-some-folder");
        std::fs::create_dir_all(&laravel).unwrap();
        std::fs::create_dir_all(&node).unwrap();
        std::fs::create_dir_all(&empty).unwrap();
        std::fs::write(laravel.join("composer.json"), "{}").unwrap();
        std::fs::write(node.join("package.json"), "{}").unwrap();

        let found = scan_for_projects(tmp.path());
        assert_eq!(found.len(), 2);
        assert!(found.contains(&laravel));
        assert!(found.contains(&node));
        assert!(!found.contains(&empty));
    }

    #[test]
    fn scan_is_not_recursive_into_nested_project_folders() {
        let tmp = tempfile::tempdir().unwrap();
        let outer = tmp.path().join("workspace");
        let nested = outer.join("deep").join("inner-app");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("package.json"), "{}").unwrap();

        // `outer` itself has no markers and its only project is two levels deep — a
        // scan of `outer` should find nothing, not recurse and find `inner-app`.
        let found = scan_for_projects(&outer);
        assert!(found.is_empty());
    }
}
