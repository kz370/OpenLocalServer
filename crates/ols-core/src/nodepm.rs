//! Node package managers (§15, Stage 11): which one a project uses, and switching on
//! pnpm / yarn through Node's bundled `corepack`. `npm` always ships with Node.

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageManagerInfo {
    /// "npm", "pnpm", "yarn" or "bun", when the project says which it uses.
    pub detected: Option<String>,
    /// What told us: "the packageManager field" or the lock file's name.
    pub detected_from: Option<String>,
    /// Version pinned by `packageManager` ("pnpm@9.1.0" gives "9.1.0").
    pub pinned_version: Option<String>,
    /// Whether each manager can be run for this project right now.
    pub npm: bool,
    pub pnpm: bool,
    pub yarn: bool,
    /// The Node install has `corepack`, so pnpm / yarn can be switched on.
    pub corepack: bool,
}

/// Reads the project's own choice: `packageManager` in package.json wins, then a lock file.
pub fn detect(project: &Path) -> (Option<String>, Option<String>, Option<String>) {
    let json: Option<Value> = std::fs::read_to_string(project.join("package.json")).ok().and_then(|raw| serde_json::from_str(&raw).ok());
    if let Some(field) = json.as_ref().and_then(|j| j.get("packageManager")).and_then(Value::as_str) {
        // "pnpm@9.1.0+sha512.abc" -> ("pnpm", "9.1.0")
        let (name, rest) = field.split_once('@').unwrap_or((field, ""));
        let version = rest.split('+').next().filter(|v| !v.is_empty()).map(str::to_string);
        if ["npm", "pnpm", "yarn", "bun"].contains(&name) {
            return (Some(name.to_string()), Some("the packageManager field".to_string()), version);
        }
    }
    for (file, name) in [("pnpm-lock.yaml", "pnpm"), ("yarn.lock", "yarn"), ("package-lock.json", "npm"), ("bun.lockb", "bun"), ("bun.lock", "bun")] {
        if project.join(file).is_file() {
            return (Some(name.to_string()), Some(file.to_string()), None);
        }
    }
    (None, None, None)
}

/// Full status for a project whose Node lives in `node_dir` (None when Node isn't resolved).
pub fn info(project: &Path, node_dir: Option<&Path>) -> PackageManagerInfo {
    let (detected, detected_from, pinned_version) = detect(project);
    let has = |name: &str| node_dir.is_some_and(|dir| crate::web::manager::find_executable(Some(dir), name).is_some());
    PackageManagerInfo {
        detected,
        detected_from,
        pinned_version,
        npm: has("npm"),
        pnpm: has("pnpm"),
        yarn: has("yarn"),
        corepack: has("corepack"),
    }
}

/// Only these two are enabled through corepack; npm needs nothing and bun isn't a Node tool.
pub fn valid_manager(name: &str) -> bool {
    matches!(name, "pnpm" | "yarn")
}

/// `corepack enable <manager>` writes its `pnpm`/`yarn` shims next to node.
pub fn enable_args(manager: &str) -> Result<Vec<String>, String> {
    if !valid_manager(manager) {
        return Err(format!("\"{manager}\" can't be switched on through corepack (only pnpm and yarn)."));
    }
    Ok(vec!["enable".into(), manager.into()])
}

/// `corepack prepare <manager>@<version> --activate` downloads that release. Follows the
/// project's pinned version when it has one, else the latest.
pub fn prepare_args(manager: &str, pinned: Option<&str>) -> Result<Vec<String>, String> {
    if !valid_manager(manager) {
        return Err(format!("\"{manager}\" can't be switched on through corepack (only pnpm and yarn)."));
    }
    let version = pinned.filter(|v| v.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))).unwrap_or("latest");
    Ok(vec!["prepare".into(), format!("{manager}@{version}"), "--activate".into()])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (name, body) in files {
            std::fs::write(dir.path().join(name), body).unwrap();
        }
        dir
    }

    #[test]
    fn package_manager_field_beats_lock_files_and_keeps_the_version() {
        let p = project(&[("package.json", r#"{"packageManager":"pnpm@9.1.0+sha512.abc"}"#), ("yarn.lock", "")]);
        assert_eq!(detect(p.path()), (Some("pnpm".into()), Some("the packageManager field".into()), Some("9.1.0".into())));
    }

    #[test]
    fn falls_back_to_the_lock_file() {
        assert_eq!(detect(project(&[("pnpm-lock.yaml", "")]).path()).0.as_deref(), Some("pnpm"));
        assert_eq!(detect(project(&[("yarn.lock", "")]).path()).0.as_deref(), Some("yarn"));
        assert_eq!(detect(project(&[("package-lock.json", "{}")]).path()).0.as_deref(), Some("npm"));
        assert_eq!(detect(project(&[("package.json", "{}")]).path()).0, None);
    }

    #[test]
    fn corepack_arguments_are_limited_to_pnpm_and_yarn() {
        assert_eq!(enable_args("pnpm").unwrap(), ["enable", "pnpm"]);
        assert!(enable_args("npm").is_err());
        assert!(enable_args("pnpm --evil").is_err());
        assert_eq!(prepare_args("yarn", Some("4.1.0")).unwrap(), ["prepare", "yarn@4.1.0", "--activate"]);
        assert_eq!(prepare_args("yarn", None).unwrap(), ["prepare", "yarn@latest", "--activate"]);
        assert_eq!(prepare_args("yarn", Some("1.0 && calc")).unwrap(), ["prepare", "yarn@latest", "--activate"]);
    }
}
