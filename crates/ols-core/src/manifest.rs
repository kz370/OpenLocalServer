//! Project manifests (§71–72, §159). A project may carry a `.openlocalserver/` folder:
//!
//! ```text
//! .openlocalserver/
//! ├── environment.yaml   what the project needs (runtimes, site, database, services, ...)
//! ├── environment.lock   the exact versions a working setup used (§72)
//! ├── services.yaml      extra services, merged into environment.yaml's `services`
//! └── commands.yaml      the project's own Quick Commands
//! ```
//!
//! A project's own manifest always wins over detected requirements (§11: explicit beats
//! guessed). Every file is optional, and a malformed one is an error, never a silent
//! fallback, so the user finds out why their manifest "does nothing".

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::quickapp::commands::QuickCommand;

pub const DIR: &str = ".openlocalserver";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RuntimeManifest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub php: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub python: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WebManifest {
    /// nginx, apache or caddy. Informational when it differs from the app-wide server,
    /// which serves every site (reported as a conflict, never switched silently).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DomainManifest {
    pub hostname: String,
    #[serde(default)]
    pub https: bool,
    #[serde(default)]
    pub wildcard: bool,
    /// Document root relative to the project ("public"); detected when missing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
    /// For apps that run their own server (Node, Python): the port the site proxies to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DatabaseManifest {
    /// mariadb, mysql (served by MariaDB), postgres, mongodb or sqlite.
    pub engine: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Database name; the project name when missing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// `redis: true` or `redis: { enabled: true }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ServiceToggle {
    On(bool),
    Detailed {
        #[serde(default = "yes")]
        enabled: bool,
    },
}

impl ServiceToggle {
    pub fn enabled(&self) -> bool {
        match self {
            ServiceToggle::On(b) => *b,
            ServiceToggle::Detailed { enabled } => *enabled,
        }
    }
}

fn yes() -> bool {
    true
}

/// A queue worker (§105). `queue: true` means the framework's usual worker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WorkerEntry {
    On(bool),
    Custom(WorkerManifest),
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WorkerManifest {
    /// The command line, run with the project's runtimes ("php artisan queue:work").
    pub command: String,
    #[serde(default = "one")]
    pub count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_mb: Option<u32>,
}

fn one() -> u32 {
    1
}

/// `scheduler: true` (the framework's scheduler) or a list of tasks (§106).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SchedulerEntry {
    On(bool),
    Tasks(Vec<ScheduledTaskManifest>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScheduledTaskManifest {
    pub name: String,
    /// Cron expression or a shortcut: "every_minute", "hourly", "daily", ...
    pub schedule: String,
    pub command: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TunnelManifest {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Never on by default (§59): a public tunnel only starts when asked for.
    #[serde(default)]
    pub autostart: bool,
}

/// A project mode (§70): what switching to it turns on or off.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModeManifest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xdebug: Option<bool>,
    /// Services to start in this mode.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub services: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workers: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheduler: Option<bool>,
    /// `.env` values to set ("APP_DEBUG: true", "LOG_LEVEL: debug").
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EnvironmentManifest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The profile this was made from (§69), for information.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(default)]
    pub runtime: RuntimeManifest,
    /// PHP extensions the project needs switched on (§12, §74 "Extensions").
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extensions: Vec<String>,
    /// npm, pnpm or yarn (§15).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_manager: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web: Option<WebManifest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<DomainManifest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database: Option<DatabaseManifest>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub services: BTreeMap<String, ServiceToggle>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub workers: BTreeMap<String, WorkerEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheduler: Option<SchedulerEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tunnel: Option<TunnelManifest>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub modes: BTreeMap<String, ModeManifest>,
}

impl EnvironmentManifest {
    /// Services switched on, in a stable order.
    pub fn enabled_services(&self) -> Vec<String> {
        self.services
            .iter()
            .filter(|(_, t)| t.enabled())
            .map(|(k, _)| k.clone())
            .collect()
    }
}

/// `.openlocalserver/commands.yaml`: `commands: [...]` or a bare list.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum CommandsFile {
    Wrapped { commands: Vec<QuickCommand> },
    List(Vec<QuickCommand>),
}

/// `.openlocalserver/environment.lock` (§72): runtime / service id → exact version.
pub type LockFile = BTreeMap<String, String>;

pub fn dir(project_path: &Path) -> PathBuf {
    project_path.join(DIR)
}

pub fn manifest_path(project_path: &Path) -> PathBuf {
    dir(project_path).join("environment.yaml")
}

pub fn lock_path(project_path: &Path) -> PathBuf {
    dir(project_path).join("environment.lock")
}

fn read_yaml<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>, String> {
    if !path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if text.trim().is_empty() {
        return Ok(None);
    }
    serde_yaml_ng::from_str(&text)
        .map(Some)
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// Reads `environment.yaml` with `services.yaml` merged in. `None` when the project has no
/// manifest; an error when a file doesn't parse.
pub fn read_manifest(project_path: &Path) -> Result<Option<EnvironmentManifest>, String> {
    let base: Option<EnvironmentManifest> = read_yaml(&manifest_path(project_path))?;
    let extra: Option<BTreeMap<String, ServiceToggle>> =
        read_yaml(&dir(project_path).join("services.yaml"))?;
    Ok(match (base, extra) {
        (None, None) => None,
        (base, extra) => {
            let mut m = base.unwrap_or_default();
            for (k, v) in extra.unwrap_or_default() {
                m.services.insert(k, v);
            }
            Some(m)
        }
    })
}

/// The project's own Quick Commands from `commands.yaml` (empty when there is none).
pub fn read_commands(project_path: &Path) -> Result<Vec<QuickCommand>, String> {
    Ok(
        match read_yaml::<CommandsFile>(&dir(project_path).join("commands.yaml"))? {
            Some(CommandsFile::Wrapped { commands }) | Some(CommandsFile::List(commands)) => {
                commands
            }
            None => Vec::new(),
        },
    )
}

pub fn read_lock(project_path: &Path) -> Result<Option<LockFile>, String> {
    read_yaml(&lock_path(project_path))
}

const MANIFEST_HEADER: &str = "# OpenLocalServer project environment (see docs: .openlocalserver/environment.yaml).\n# Commit this folder so `ols setup` rebuilds the same environment on another machine.\n";
const LOCK_HEADER: &str = "# Written by OpenLocalServer after a successful setup. Exact versions, for reproducible setups.\n";

pub fn write_manifest(
    project_path: &Path,
    manifest: &EnvironmentManifest,
) -> Result<PathBuf, String> {
    let path = manifest_path(project_path);
    let yaml = serde_yaml_ng::to_string(manifest).map_err(|e| e.to_string())?;
    write_atomic(&path, &format!("{MANIFEST_HEADER}{yaml}"))?;
    Ok(path)
}

pub fn write_lock(project_path: &Path, lock: &LockFile) -> Result<PathBuf, String> {
    let path = lock_path(project_path);
    let yaml = serde_yaml_ng::to_string(lock).map_err(|e| e.to_string())?;
    write_atomic(&path, &format!("{LOCK_HEADER}{yaml}"))?;
    Ok(path)
}

fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project_with(files: &[(&str, &str)]) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(DIR)).unwrap();
        for (name, text) in files {
            std::fs::write(tmp.path().join(DIR).join(name), text).unwrap();
        }
        tmp
    }

    #[test]
    fn reads_runtime_versions_from_manifest() {
        let tmp = project_with(&[(
            "environment.yaml",
            "name: shop\nruntime:\n  php: \"8.1\"\n  node: \"20\"\n",
        )]);
        let manifest = read_manifest(tmp.path())
            .unwrap()
            .expect("manifest present");
        assert_eq!(manifest.name.as_deref(), Some("shop"));
        assert_eq!(manifest.runtime.php.as_deref(), Some("8.1"));
        assert_eq!(manifest.runtime.node.as_deref(), Some("20"));
    }

    #[test]
    fn reads_the_full_srs_example() {
        let yaml = r#"
name: shop
runtime:
  php: "8.4"
  node: "22"
web:
  server: nginx
domain:
  hostname: shop.test
  https: true
  wildcard: true
database:
  engine: mysql
  version: "8.4"
services:
  redis: true
  mailpit: true
workers:
  queue: true
scheduler: true
tunnel:
  enabled: false
"#;
        let tmp = project_with(&[
            ("environment.yaml", yaml),
            (
                "services.yaml",
                "mailpit: false\npostgres: { enabled: true }\n",
            ),
        ]);
        let m = read_manifest(tmp.path()).unwrap().unwrap();
        let d = m.domain.as_ref().unwrap();
        assert!(d.https && d.wildcard && d.hostname == "shop.test");
        assert_eq!(m.database.as_ref().unwrap().engine, "mysql");
        assert_eq!(
            m.enabled_services(),
            vec!["postgres".to_string(), "redis".to_string()],
            "services.yaml overrides and extends"
        );
        assert_eq!(m.workers.get("queue"), Some(&WorkerEntry::On(true)));
        assert_eq!(m.scheduler, Some(SchedulerEntry::On(true)));
        assert!(
            !m.tunnel.unwrap().autostart,
            "a tunnel never starts by default"
        );
    }

    #[test]
    fn manifest_and_lock_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let mut m = EnvironmentManifest {
            name: Some("api".into()),
            ..Default::default()
        };
        m.runtime.node = Some("24".into());
        m.services.insert("redis".into(), ServiceToggle::On(true));
        write_manifest(tmp.path(), &m).unwrap();
        assert_eq!(read_manifest(tmp.path()).unwrap().unwrap(), m);

        let lock = LockFile::from([("node".to_string(), "24.21.0".to_string())]);
        write_lock(tmp.path(), &lock).unwrap();
        assert_eq!(read_lock(tmp.path()).unwrap().unwrap(), lock);
    }

    #[test]
    fn commands_file_accepts_a_wrapped_or_bare_list() {
        let item = "- id: migrate\n  name: Migrate\n  command: { executable: php, arguments: [artisan, migrate] }\n";
        let tmp = project_with(&[("commands.yaml", item)]);
        assert_eq!(read_commands(tmp.path()).unwrap()[0].id, "migrate");
        let tmp = project_with(&[(
            "commands.yaml",
            &format!(
                "commands:\n{}",
                item.lines().map(|l| format!("  {l}\n")).collect::<String>()
            ),
        )]);
        assert_eq!(read_commands(tmp.path()).unwrap().len(), 1);
    }

    #[test]
    fn returns_none_when_no_manifest_file() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(read_manifest(tmp.path()).unwrap().is_none());
    }

    #[test]
    fn malformed_manifest_is_an_error_not_a_silent_fallback() {
        let tmp = project_with(&[("environment.yaml", "runtime: [this, is, not, a, map]")]);
        assert!(read_manifest(tmp.path()).is_err());
    }
}
