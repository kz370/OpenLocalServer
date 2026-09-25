//! What a project's Composer files say (§14, Stage 11): the packages it asks for and the
//! versions locked, read straight from `composer.json` and `composer.lock` without running
//! PHP. The commands themselves (install, update, require, ...) run through the ordinary
//! project command runner, which resolves `composer` to the project's PHP.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComposerPackage {
    pub name: String,
    /// The constraint from composer.json, e.g. "^11.0".
    pub constraint: String,
    /// The version in composer.lock; `None` until `composer install` has run.
    pub locked: Option<String>,
    pub dev: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ComposerInfo {
    pub has_composer_json: bool,
    pub has_lock: bool,
    pub vendor_installed: bool,
    pub name: Option<String>,
    pub packages: Vec<ComposerPackage>,
    /// Names under `scripts`, runnable with `composer run-script <name>`.
    pub scripts: Vec<String>,
}

/// Platform requirements (`php`, `ext-*`, `lib-*`) aren't packages you install.
fn is_platform(name: &str) -> bool {
    name == "php" || name == "php-64bit" || name.starts_with("ext-") || name.starts_with("lib-") || name.starts_with("composer-")
}

pub fn read(project: &Path) -> ComposerInfo {
    let json: Option<Value> = std::fs::read_to_string(project.join("composer.json")).ok().and_then(|raw| serde_json::from_str(&raw).ok());
    let Some(json) = json else { return ComposerInfo::default() };

    let lock: Option<Value> = std::fs::read_to_string(project.join("composer.lock")).ok().and_then(|raw| serde_json::from_str(&raw).ok());
    let mut locked: BTreeMap<String, String> = BTreeMap::new();
    if let Some(lock) = &lock {
        for key in ["packages", "packages-dev"] {
            for p in lock.get(key).and_then(Value::as_array).into_iter().flatten() {
                if let (Some(name), Some(version)) = (p.get("name").and_then(Value::as_str), p.get("version").and_then(Value::as_str)) {
                    locked.insert(name.to_string(), version.to_string());
                }
            }
        }
    }

    let mut packages = Vec::new();
    for (key, dev) in [("require", false), ("require-dev", true)] {
        let Some(map) = json.get(key).and_then(Value::as_object) else { continue };
        for (name, constraint) in map {
            if is_platform(name) {
                continue;
            }
            packages.push(ComposerPackage {
                name: name.clone(),
                constraint: constraint.as_str().unwrap_or_default().to_string(),
                locked: locked.get(name).cloned(),
                dev,
            });
        }
    }
    packages.sort_by(|a, b| a.dev.cmp(&b.dev).then_with(|| a.name.cmp(&b.name)));

    let scripts = json.get("scripts").and_then(Value::as_object).map(|m| m.keys().cloned().collect()).unwrap_or_default();
    ComposerInfo {
        has_composer_json: true,
        has_lock: lock.is_some(),
        vendor_installed: project.join("vendor").join("autoload.php").is_file(),
        name: json.get("name").and_then(Value::as_str).map(str::to_string),
        packages,
        scripts,
    }
}

/// A package name as Composer accepts it: `vendor/name`, optionally with a `:constraint`.
/// Checked before it goes on a command line, so it can't smuggle in extra flags.
pub fn valid_requirement(spec: &str) -> bool {
    let (name, constraint) = match spec.split_once(':') {
        Some((n, c)) => (n, Some(c)),
        None => (spec, None),
    };
    let name_ok = name.split_once('/').is_some_and(|(vendor, pkg)| {
        let part = |s: &str| !s.is_empty() && !s.starts_with('-') && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
        part(vendor) && part(pkg)
    });
    let constraint_ok = constraint.is_none_or(|c| !c.is_empty() && c.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '*' | '^' | '~' | '-' | '@' | '>' | '<' | '=' | '|' | ',')));
    name_ok && constraint_ok
}

/// The Composer arguments for one of the app's named actions. `target` is a package
/// (`vendor/name[:constraint]`) or, for `run_script`, a script name.
pub fn command_args(action: &str, target: Option<&str>) -> Result<Vec<String>, String> {
    let target = target.map(str::trim).filter(|t| !t.is_empty());
    let package = || match target {
        Some(t) if valid_requirement(t) => Ok(t.to_string()),
        Some(t) => Err(format!("\"{t}\" is not a package name like vendor/name or vendor/name:^1.0")),
        None => Err(format!("The {action} action needs a package name.")),
    };
    let args: Vec<String> = match action {
        "install" => vec!["install".into()],
        "update" => match target {
            Some(_) => vec!["update".into(), package()?],
            None => vec!["update".into()],
        },
        "require" => vec!["require".into(), package()?],
        "require_dev" => vec!["require".into(), "--dev".into(), package()?],
        "remove" => vec!["remove".into(), package()?.split(':').next().unwrap_or_default().to_string()],
        "dump_autoload" => vec!["dump-autoload".into()],
        "dump_autoload_optimized" => vec!["dump-autoload".into(), "--optimize".into()],
        "outdated" => vec!["outdated".into(), "--direct".into()],
        "validate" => vec!["validate".into()],
        "audit" => vec!["audit".into()],
        "diagnose" => vec!["diagnose".into()],
        "show" => vec!["show".into()],
        "clear_cache" => vec!["clear-cache".into()],
        "run_script" => {
            let name = target.ok_or("Pick a script to run.")?;
            if !name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':' | '.')) || name.starts_with('-') {
                return Err(format!("\"{name}\" is not a script name."));
            }
            vec!["run-script".into(), name.to_string()]
        }
        other => return Err(format!("Unknown Composer action \"{other}\".")),
    };
    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_map_to_composer_arguments_and_reject_bad_targets() {
        assert_eq!(command_args("install", None).unwrap(), ["install"]);
        assert_eq!(command_args("update", None).unwrap(), ["update"]);
        assert_eq!(command_args("update", Some("a/b")).unwrap(), ["update", "a/b"]);
        assert_eq!(command_args("require_dev", Some("a/b:^2")).unwrap(), ["require", "--dev", "a/b:^2"]);
        assert_eq!(command_args("remove", Some("a/b:^2")).unwrap(), ["remove", "a/b"]);
        assert_eq!(command_args("run_script", Some("test")).unwrap(), ["run-script", "test"]);
        assert!(command_args("require", None).is_err());
        assert!(command_args("require", Some("--no-scripts")).is_err());
        assert!(command_args("run_script", Some("--help")).is_err());
        assert!(command_args("self-update", None).is_err());
    }

    #[test]
    fn reads_requirements_and_locked_versions_without_platform_entries() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("composer.json"),
            r#"{"name":"acme/shop","require":{"php":"^8.2","ext-mbstring":"*","laravel/framework":"^11.0"},
                "require-dev":{"phpunit/phpunit":"^11"},"scripts":{"test":"phpunit","post-install-cmd":[]}}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("composer.lock"),
            r#"{"packages":[{"name":"laravel/framework","version":"v11.9.2"}],"packages-dev":[{"name":"phpunit/phpunit","version":"11.1.0"}]}"#,
        )
        .unwrap();
        let info = read(dir.path());
        assert!(info.has_composer_json && info.has_lock && !info.vendor_installed);
        assert_eq!(info.name.as_deref(), Some("acme/shop"));
        assert_eq!(
            info.packages,
            vec![
                ComposerPackage { name: "laravel/framework".into(), constraint: "^11.0".into(), locked: Some("v11.9.2".into()), dev: false },
                ComposerPackage { name: "phpunit/phpunit".into(), constraint: "^11".into(), locked: Some("11.1.0".into()), dev: true },
            ]
        );
        assert_eq!(info.scripts.len(), 2);
    }

    #[test]
    fn a_folder_without_composer_json_reports_nothing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!read(dir.path()).has_composer_json);
    }

    #[test]
    fn requirement_names_cannot_carry_flags() {
        assert!(valid_requirement("monolog/monolog"));
        assert!(valid_requirement("monolog/monolog:^3.0"));
        assert!(valid_requirement("symfony/console:>=6.0,<7"));
        assert!(!valid_requirement("--dev"));
        assert!(!valid_requirement("-x/y"));
        assert!(!valid_requirement("monolog"));
        assert!(!valid_requirement("a/b c"));
        assert!(!valid_requirement("a/b:"));
        assert!(!valid_requirement("a/b:^1; rm"));
    }
}
