use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::broadcast;

use crate::catalog::{builtin_catalog, OwnedManifest, PackageManifest};
use crate::paths::AppPaths;
use super::types::*;
use super::*;

pub struct RuntimeManager {
    pub(crate) runtime: tokio::runtime::Runtime,
    pub(crate) paths: AppPaths,
    pub(crate) http: reqwest::Client,
    pub(crate) events_tx: broadcast::Sender<RuntimeEvent>,
    pub(crate) state: Arc<Mutex<HashMap<String, InstallState>>>,
    /// One control per in-flight `id@version`, so a running download can be paused or
    /// stopped from the UI. Entries are removed when the install reaches a terminal state.
    pub(crate) controls: Arc<RwLock<HashMap<String, Arc<InstallControl>>>>,
    pub(crate) preferred: RwLock<HashMap<String, String>>,
    pub(crate) online_versions: RwLock<HashMap<String, Vec<String>>>,
}

impl RuntimeManager {
    pub fn new(paths: AppPaths) -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("failed to start runtime manager runtime");
        let (events_tx, _rx) = broadcast::channel(256);
        let preferred = std::fs::read_to_string(paths.settings_file())
            .ok()
            .and_then(|raw| serde_json::from_str::<HashMap<String, serde_json::Value>>(&raw).ok())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(key, value)| {
                let id = key.strip_prefix("runtime.")?.strip_suffix(".global")?;
                Some((id.to_string(), value.as_str()?.to_string()))
            })
            .collect();
        Self {
            runtime,
            paths,
            http: reqwest::Client::builder()
                .user_agent("OLS")
                .build()
                .expect("failed to build runtime HTTP client"),
            events_tx,
            state: Arc::new(Mutex::new(HashMap::new())),
            controls: Arc::new(RwLock::new(HashMap::new())),
            preferred: RwLock::new(preferred),
            online_versions: RwLock::new(HashMap::new()),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<RuntimeEvent> {
        self.events_tx.subscribe()
    }

    pub fn display_name(&self, id: &str) -> Option<String> {
        builtin_catalog()
            .into_iter()
            .find(|m| m.id == id)
            .map(|m| m.name.to_string())
    }

    pub fn binary_path(&self, id: &str, version: &str) -> Option<PathBuf> {
        let binary = builtin_catalog()
            .into_iter()
            .find(|m| m.id == id && m.version == version)
            .map(|m| m.binary)
            .or_else(|| runtime_binary(id))?;
        let path = self.version_dir(id, version).join(binary);
        path.exists().then_some(path)
    }

    /// The directory holding the runtime's executable — what a runtime-aware terminal
    /// prepends to PATH (§19), and what the Environment Resolver checks before offering
    /// "this project needs PHP 8.1, install it?" (§18, §74).
    pub fn bin_dir(&self, id: &str, version: &str) -> Option<PathBuf> {
        self.binary_path(id, version)
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
    }

    /// Installed versions for `id`, with a configured default first and the remaining versions
    /// in catalog order.
    pub fn installed_versions(&self, id: &str) -> Vec<String> {
        let mut versions = Vec::new();
        if let Some(binary) = runtime_binary(id) {
            if let Ok(dirs) = std::fs::read_dir(self.paths.runtimes_dir().join(id)) {
                for dir in dirs.flatten().filter(|d| d.path().is_dir()) {
                    if let (Some(version), true) = (
                        dir.file_name().to_str().map(str::to_string),
                        dir.path().join(binary).exists(),
                    ) {
                        if !versions.contains(&version) {
                            versions.push(version);
                        }
                    }
                }
            }
        }
        for m in builtin_catalog().into_iter().filter(|m| m.id == id) {
            if self.version_dir(m.id, m.version).join(m.binary).exists()
                && !versions.iter().any(|v| v == m.version)
            {
                versions.push(m.version.to_string());
            }
        }
        versions.sort_by(|a, b| compare_versions(b, a));
        if let Some(preferred) = self.preferred_versions().get(id) {
            if let Some(index) = versions.iter().position(|v| v == preferred) {
                versions.swap(0, index);
            }
        }
        versions
    }

    /// Record an installed managed version as the family default. Project pins still take
    /// precedence over this preference.
    pub fn set_preferred(&self, id: &str, version: &str) -> Result<(), String> {
        if self.binary_path(id, version).is_none() {
            return Err(format!("{id} {version} is not installed"));
        }
        self.preferred
            .write()
            .unwrap()
            .insert(id.to_string(), version.to_string());
        Ok(())
    }

    /// Remove only the managed program files. Data directories (for example MariaDB's
    /// databases) live outside the runtime tree and are never touched here.
    pub fn remove(&self, id: &str, version: &str) -> Result<(), String> {
        if !self
            .catalog()
            .iter()
            .any(|e| e.id == id && e.version == version)
        {
            return Err(format!("{id} {version} is not in the runtime catalog"));
        }
        let versions = self.installed_versions(id);
        let configured_default = self.preferred_versions().get(id).cloned();
        let selected = configured_default
            .as_deref()
            .map(|v| v == version)
            .unwrap_or_else(|| versions.first().is_some_and(|v| v == version));
        if selected && versions.len() > 1 {
            return Err(
                "Choose another installed version as the default before removing this one.".into(),
            );
        }
        let key = format!("{id}@{version}");
        if self.state.lock().unwrap().get(&key).is_some_and(|s| {
            matches!(
                s,
                InstallState::Downloading
                    | InstallState::Paused
                    | InstallState::Verifying
                    | InstallState::Extracting
            )
        }) {
            return Err(format!("{id} {version} is currently being installed"));
        }
        let dir = self.version_dir(id, version);
        let binary = builtin_catalog()
            .into_iter()
            .find(|m| m.id == id && m.version == version)
            .map(|m| m.binary)
            .or_else(|| runtime_binary(id))
            .ok_or_else(|| format!("{id} {version} is not in the runtime catalog"))?;
        if !dir.join(binary).exists() {
            return Err(format!("{id} {version} is not installed"));
        }
        std::fs::remove_dir_all(&dir)
            .map_err(|e| format!("could not remove {id} {version}: {e}"))?;
        if selected {
            self.preferred.write().unwrap().remove(id);
        }
        Ok(())
    }

    /// The root of an installed version — e.g. MariaDB's `--basedir`, which needs the whole
    /// install tree (bin/, share/, ...), not just the directory holding the executable.
    pub fn install_dir(&self, id: &str, version: &str) -> PathBuf {
        self.version_dir(id, version)
    }

    /// Blocking HTTPS GET of a whole (small) body, for callers outside any async context.
    /// Runs on this manager's own runtime, so it's safe from inside another runtime's
    /// blocking thread too (where `block_on` would panic).
    pub fn fetch(&self, url: &str) -> Result<Vec<u8>, String> {
        self.fetch_with_timeout(url, 120)
    }

    /// Same GET with a caller-chosen ceiling. The update manifest uses 20s so a
    /// stalled check fails fast instead of spinning the button for two minutes;
    /// big installer downloads keep the long timeout.
    pub fn fetch_with_timeout(&self, url: &str, secs: u64) -> Result<Vec<u8>, String> {
        let http = self.http.clone();
        let url = url.to_string();
        let (tx, rx) = std::sync::mpsc::channel();
        self.runtime.spawn(async move {
            let get = async {
                let response = http.get(&url).send().await.map_err(|e| e.to_string())?;
                if !response.status().is_success() {
                    return Err(format!("{url}: HTTP {}", response.status()));
                }
                response
                    .bytes()
                    .await
                    .map(|b| b.to_vec())
                    .map_err(|e| e.to_string())
            };
            let result = tokio::time::timeout(std::time::Duration::from_secs(secs), get)
                .await
                .unwrap_or_else(|_| Err(format!("{url}: timed out")));
            let _ = tx.send(result);
        });
        rx.recv().map_err(|e| e.to_string())?
    }

    pub(crate) fn version_dir(&self, id: &str, version: &str) -> PathBuf {
        self.paths.runtimes_dir().join(id).join(version)
    }
}

pub(crate) fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let parts = |v: &str| {
        v.split('.')
            .map(|p| p.parse::<u64>().unwrap_or(0))
            .collect::<Vec<_>>()
    };
    let (a, b) = (parts(a), parts(b));
    for i in 0..a.len().max(b.len()) {
        match a
            .get(i)
            .copied()
            .unwrap_or(0)
            .cmp(&b.get(i).copied().unwrap_or(0))
        {
            std::cmp::Ordering::Equal => {}
            other => return other,
        }
    }
    std::cmp::Ordering::Equal
}

pub(crate) fn runtime_binary(id: &str) -> Option<&'static str> {
    match id {
        "node" => Some("node.exe"),
        "php" => Some("php.exe"),
        "python" => Some("python.exe"),
        "nginx" => Some("nginx.exe"),
        "mariadb" => Some("bin/mariadbd.exe"),
        "apache" => Some("bin/httpd.exe"),
        "composer" => Some("composer.phar"),
        "mongodb" => Some("bin/mongod.exe"),
        "postgres" => Some("bin/postgres.exe"),
        "redis" => Some("redis-server.exe"),
        "memcached" => Some("bin/memcached.exe"),
        _ => None,
    }
}

pub(crate) fn runtime_name(id: &str, manifests: &[PackageManifest]) -> String {
    manifests
        .iter()
        .find(|m| m.id == id)
        .map(|m| m.name.to_string())
        .unwrap_or_else(|| match id {
            "node" => "Node.js".into(),
            "php" => "PHP".into(),
            "python" => "Python".into(),
            "nginx" => "Nginx".into(),
            "mariadb" => "MariaDB".into(),
            "apache" => "Apache HTTP Server".into(),
            "composer" => "Composer".into(),
            "mongodb" => "MongoDB".into(),
            "postgres" => "PostgreSQL".into(),
            "redis" => "Redis".into(),
            "memcached" => "Memcached".into(),
            _ => id.into(),
        })
}

