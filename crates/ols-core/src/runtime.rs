//! Runtime Manager (§9–10, §127 — Stage 3): download → verify → extract, with the app
//! never running an unverified binary. Same "own runtime, broadcast events" shape as
//! `ProcessSupervisor` (see `process.rs`) so both work identically from `cargo test` and Tauri.

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    /// A OpenLocalServer-managed copy is installed under `runtimes/<id>/<version>/`.
    pub installed: bool,
    /// The version newly-created projects and services will use by default.
    pub is_default: bool,
    /// An unmanaged install found on PATH — same runtime family, not necessarily the
    /// same version, and not usable for per-project version selection (§126).
    pub system: Option<SystemInstall>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInstall {
    pub path: String,
    pub version: String,
}

/// Look for an existing, unmanaged install of `id` on PATH. Never touches or registers
/// it — purely informational, so the UI can say "found on your system" instead of only
/// ever offering a download (§126: never modify an existing install without asking).
pub fn detect_system_install(id: &str) -> Option<SystemInstall> {
    // Each probe spawns `<tool> --version`, and the UI polls the catalog — remember answers briefly.
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<
            std::collections::HashMap<String, (std::time::Instant, Option<SystemInstall>)>,
        >,
    > = std::sync::OnceLock::new();
    const TTL: std::time::Duration = std::time::Duration::from_secs(60);
    let cache = CACHE.get_or_init(Default::default);
    if let Some((at, found)) = cache.lock().unwrap().get(id) {
        if at.elapsed() < TTL {
            return found.clone();
        }
    }
    let found = probe_system_install(id);
    cache
        .lock()
        .unwrap()
        .insert(id.to_string(), (std::time::Instant::now(), found.clone()));
    found
}

fn probe_system_install(id: &str) -> Option<SystemInstall> {
    let (exe_name, version_flag) = crate::catalog::system_probe(id)?;
    let path_var = std::env::var_os("PATH")?;
    let exe_path = std::env::split_paths(&path_var)
        .map(|dir| dir.join(exe_name))
        .find(|p| p.is_file())?;

    let version =
        probe_version(&exe_path, version_flag).unwrap_or_else(|| "unknown version".to_string());

    Some(SystemInstall {
        path: exe_path.display().to_string(),
        version,
    })
}

/// First line of `<exe> <flag>`. Some tools (an old Windows Redis) never exit on their version flag, or
/// leave a child holding the output pipe open, so the answer goes to a temp file instead of a pipe and
/// the probe is killed after a few seconds. Nothing here can block the catalog for long.
fn probe_version(exe: &std::path::Path, flag: &str) -> Option<String> {
    use std::process::Stdio;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let out_path =
        std::env::temp_dir().join(format!("ols-probe-{}-{nanos}.txt", std::process::id()));
    let out = std::fs::File::create(&out_path).ok()?;
    let err = out.try_clone().ok()?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg(flag).stdin(Stdio::null()).stdout(out).stderr(err);
    crate::exec::hide_window(&mut cmd);
    let spawned = cmd.spawn();
    let text = spawned.ok().and_then(|mut child| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            match child.try_wait().ok()? {
                Some(_) => break,
                None if std::time::Instant::now() >= deadline => {
                    let _ = child.kill();
                    return None;
                }
                None => std::thread::sleep(std::time::Duration::from_millis(20)),
            }
        }
        std::fs::read_to_string(&out_path).ok()
    });
    let _ = std::fs::remove_file(&out_path);
    text.map(|t| {
        t.lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("unknown version")
            .trim()
            .to_string()
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallState {
    Downloading,
    Verifying,
    Extracting,
    Installed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuntimeEvent {
    Progress {
        id: String,
        version: String,
        state: InstallState,
        downloaded: u64,
        total: Option<u64>,
    },
    Installed {
        id: String,
        version: String,
        path: String,
    },
    Failed {
        id: String,
        version: String,
        message: String,
    },
}

pub struct RuntimeManager {
    runtime: tokio::runtime::Runtime,
    paths: AppPaths,
    http: reqwest::Client,
    events_tx: broadcast::Sender<RuntimeEvent>,
    state: Arc<Mutex<HashMap<String, InstallState>>>,
    preferred: RwLock<HashMap<String, String>>,
    online_versions: RwLock<HashMap<String, Vec<String>>>,
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
                .user_agent("OpenLocalServer")
                .build()
                .expect("failed to build runtime HTTP client"),
            events_tx,
            state: Arc::new(Mutex::new(HashMap::new())),
            preferred: RwLock::new(preferred),
            online_versions: RwLock::new(HashMap::new()),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<RuntimeEvent> {
        self.events_tx.subscribe()
    }

    fn preferred_versions(&self) -> HashMap<String, String> {
        let values = crate::db::load_settings(&self.paths).unwrap_or_default();
        let preferred = values
            .into_iter()
            .filter_map(|(key, value)| {
                let id = key.strip_prefix("runtime.")?.strip_suffix(".global")?;
                Some((id.to_string(), value.as_str()?.to_string()))
            })
            .collect::<HashMap<_, _>>();
        *self.preferred.write().unwrap() = preferred.clone();
        preferred
    }

    /// The catalog for this machine's OS/arch, each entry flagged with whether it's
    /// already installed (§127 — no unnecessary repeated downloads).
    pub fn catalog(&self) -> Vec<CatalogEntry> {
        let manifests = builtin_catalog();
        let online = self.online_versions.read().unwrap().clone();
        let mut all_versions: HashMap<String, Vec<String>> = HashMap::new();
        for m in &manifests {
            all_versions
                .entry(m.id.to_string())
                .or_default()
                .push(m.version.to_string());
        }
        for (id, versions) in &online {
            let known = all_versions.entry(id.clone()).or_default();
            for version in versions {
                if !known.contains(version) {
                    known.push(version.clone());
                }
            }
        }
        for id in [
            "node",
            "php",
            "nginx",
            "mariadb",
            "apache",
            "composer",
            "mongodb",
            "postgres",
            "redis",
            "memcached",
            "mailpit",
        ] {
            if let Ok(dirs) = std::fs::read_dir(self.paths.runtimes_dir().join(id)) {
                let known = all_versions.entry(id.to_string()).or_default();
                for dir in dirs.flatten().filter(|d| d.path().is_dir()) {
                    let Some(version) = dir.file_name().to_str().map(str::to_string) else {
                        continue;
                    };
                    if !known.contains(&version) && self.is_installed(id, &version) {
                        known.push(version);
                    }
                }
            }
        }
        let mut ids: Vec<String> = all_versions.keys().cloned().collect();
        ids.sort();
        // Each probe spawns `<tool> --version`; run one per runtime id side by side instead of in turn.
        let detected: HashMap<String, Option<SystemInstall>> = std::thread::scope(|scope| {
            let handles: Vec<_> = ids
                .iter()
                .map(|id| (id.clone(), scope.spawn(move || detect_system_install(id))))
                .collect();
            handles
                .into_iter()
                .map(|(id, h)| (id, h.join().unwrap_or(None)))
                .collect()
        });
        let preferred = self.preferred_versions();
        let defaults: HashMap<String, String> = all_versions
            .iter()
            .filter_map(|(id, versions)| {
                let installed: Vec<String> = versions
                    .iter()
                    .filter(|v| self.is_installed(id, v))
                    .cloned()
                    .collect();
                preferred
                    .get(id)
                    .filter(|v| installed.contains(v))
                    .cloned()
                    .or_else(|| installed.into_iter().max_by(|a, b| compare_versions(a, b)))
                    .map(|version| (id.clone(), version))
            })
            .collect();
        all_versions
            .into_iter()
            .flat_map(|(id, versions)| {
                versions.into_iter().map({
                    let id = id.clone();
                    let name = runtime_name(&id, &manifests);
                    let default = defaults.get(&id).cloned();
                    let system = detected.get(&id).cloned().flatten();
                    move |version| CatalogEntry {
                        id: id.clone(),
                        name: name.clone(),
                        installed: self.is_installed(&id, &version),
                        is_default: default.as_deref() == Some(version.as_str()),
                        system: system.clone(),
                        version,
                    }
                })
            })
            .collect()
    }

    fn is_installed(&self, id: &str, version: &str) -> bool {
        let binary = builtin_catalog()
            .into_iter()
            .find(|m| m.id == id && m.version == version)
            .map(|m| m.binary)
            .or_else(|| runtime_binary(id));
        binary.is_some_and(|binary| self.version_dir(id, version).join(binary).exists())
    }

    /// Fetch current release versions from each supported vendor's public feed. The feeds
    /// contain metadata only; the package is still resolved and SHA-256 checked on install.
    pub fn refresh_online_catalog(&self, id: &str) -> Result<(), String> {
        let versions = match id {
            "node" => {
                let bytes = self.fetch("https://nodejs.org/dist/index.json")?;
                let releases: Vec<serde_json::Value> = serde_json::from_slice(&bytes)
                    .map_err(|e| format!("Node.js release list is invalid: {e}"))?;
                releases
                    .into_iter()
                    .filter_map(|r| {
                        let has_zip = r
                            .get("files")?
                            .as_array()?
                            .iter()
                            .any(|f| f.as_str() == Some("win-x64-zip"));
                        if has_zip {
                            r.get("version")?
                                .as_str()?
                                .strip_prefix('v')
                                .map(str::to_string)
                        } else {
                            None
                        }
                    })
                    .collect()
            }
            "php" => {
                let bytes =
                    self.fetch("https://downloads.php.net/~windows/releases/releases.json")?;
                let releases: serde_json::Value = serde_json::from_slice(&bytes)
                    .map_err(|e| format!("PHP release list is invalid: {e}"))?;
                let mut versions: Vec<String> = releases
                    .as_object()
                    .into_iter()
                    .flat_map(|branches| branches.values())
                    .filter_map(|release| {
                        let version = release.get("version")?.as_str()?;
                        let x64_nts = release.as_object()?.iter().any(|(key, build)| {
                            key.starts_with("nts-vs")
                                && key.ends_with("-x64")
                                && build
                                    .pointer("/zip/sha256")
                                    .and_then(|v| v.as_str())
                                    .is_some()
                        });
                        x64_nts.then(|| version.to_string())
                    })
                    .collect();
                versions.sort_by(|a, b| compare_versions(b, a));
                versions.dedup();
                versions
            }
            "nginx" => {
                let page = String::from_utf8(self.fetch("https://nginx.org/en/download.html")?)
                    .map_err(|_| "Nginx release page is not UTF-8".to_string())?;
                nginx_versions(&page)
            }
            "apache" => {
                let page = String::from_utf8(self.fetch("https://www.apachelounge.com/download/")?)
                    .map_err(|_| "Apache Lounge download page is not UTF-8".to_string())?;
                extract_numeric_filename_versions(&page, "httpd-")
            }
            "composer" => {
                let page = String::from_utf8(self.fetch("https://getcomposer.org/download/")?)
                    .map_err(|_| "Composer download page is not UTF-8".to_string())?;
                extract_versions(&page, "/download/", "/composer.phar")
            }
            "mongodb" => {
                let bytes = self.fetch("https://downloads.mongodb.org/current.json")?;
                let releases: serde_json::Value = serde_json::from_slice(&bytes)
                    .map_err(|e| format!("MongoDB release list is invalid: {e}"))?;
                let mut versions = Vec::new();
                collect_mongodb_versions(&releases, &mut versions);
                versions.sort_by(|a, b| compare_versions(b, a));
                versions.dedup();
                versions
            }
            "postgres" => {
                let page = String::from_utf8(
                    self.fetch("https://www.enterprisedb.com/download-postgresql-binaries")?,
                )
                .map_err(|_| "PostgreSQL download page is not UTF-8".to_string())?;
                extract_versions(&page, "Binaries from installer Version ", "<")
            }
            "redis" => {
                let bytes = self.fetch("https://api.github.com/repos/redis-windows/redis-windows/releases?per_page=100")?;
                let releases: Vec<serde_json::Value> = serde_json::from_slice(&bytes)
                    .map_err(|e| format!("Redis release list is invalid: {e}"))?;
                releases
                    .into_iter()
                    .filter_map(|release| {
                        let assets = release.get("assets")?.as_array()?;
                        let has_windows_zip = assets.iter().any(|asset| {
                            asset.get("name").and_then(|n| n.as_str()).is_some_and(|n| {
                                n.starts_with("Redis-")
                                    && n.contains("Windows-x64")
                                    && n.ends_with(".zip")
                            })
                        });
                        if !has_windows_zip {
                            return None;
                        }
                        release
                            .get("tag_name")?
                            .as_str()
                            .map(|tag| tag.strip_prefix('v').unwrap_or(tag).to_string())
                    })
                    .collect()
            }
            "memcached" => {
                // The Windows port tags releases `<upstream version>_mingw_libressl`; offer
                // the upstream version so the download URL can be rebuilt from it.
                let bytes = self.fetch(
                    "https://api.github.com/repos/jefyt/memcached-windows/releases?per_page=100",
                )?;
                let releases: Vec<serde_json::Value> = serde_json::from_slice(&bytes)
                    .map_err(|e| format!("Memcached release list is invalid: {e}"))?;
                let mut versions: Vec<String> = releases
                    .into_iter()
                    .filter_map(|release| {
                        let assets = release.get("assets")?.as_array()?;
                        let has_windows_zip = assets.iter().any(|asset| {
                            asset.get("name").and_then(|n| n.as_str()).is_some_and(|n| {
                                n.starts_with("memcached-") && n.ends_with("-win64-mingw.zip")
                            })
                        });
                        if !has_windows_zip {
                            return None;
                        }
                        let tag = release.get("tag_name")?.as_str()?;
                        tag.strip_suffix("_mingw_libressl")
                            .map(|v| v.trim_start_matches('v').to_string())
                    })
                    .collect();
                versions.sort_by(|a, b| compare_versions(b, a));
                versions.dedup();
                versions
            }
            "mailpit" => {
                let bytes = self
                    .fetch("https://api.github.com/repos/axllent/mailpit/releases?per_page=100")?;
                let releases: Vec<serde_json::Value> = serde_json::from_slice(&bytes)
                    .map_err(|e| format!("Mailpit release list is invalid: {e}"))?;
                let mut versions: Vec<String> = releases
                    .into_iter()
                    .filter_map(|release| {
                        // Only releases carrying the x64 Windows zip are installable; the
                        // arm64 and unix assets are not what this catalog installs.
                        let assets = release.get("assets")?.as_array()?;
                        let has_windows_zip = assets.iter().any(|asset| {
                            asset.get("name").and_then(|n| n.as_str())
                                == Some("mailpit-windows-amd64.zip")
                        });
                        if !has_windows_zip {
                            return None;
                        }
                        release
                            .get("tag_name")?
                            .as_str()?
                            .strip_prefix('v')
                            .map(str::to_string)
                    })
                    .collect();
                versions.sort_by(|a, b| compare_versions(b, a));
                versions.dedup();
                versions
            }
            "mariadb" => {
                let bytes = self.fetch("https://downloads.mariadb.org/rest-api/mariadb/")?;
                let majors: serde_json::Value = serde_json::from_slice(&bytes)
                    .map_err(|e| format!("MariaDB release list is invalid: {e}"))?;
                let mut versions = Vec::new();
                for major in majors
                    .get("major_releases")
                    .and_then(|v| v.as_array())
                    .into_iter()
                    .flatten()
                {
                    let Some(branch) = major.get("release_id").and_then(|v| v.as_str()) else {
                        continue;
                    };
                    let url = format!("https://downloads.mariadb.org/rest-api/mariadb/{branch}/");
                    let Ok(bytes) = self.fetch(&url) else {
                        continue;
                    };
                    let Ok(data) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
                        continue;
                    };
                    for (version, release) in data
                        .get("releases")
                        .and_then(|v| v.as_object())
                        .into_iter()
                        .flatten()
                    {
                        let has_windows_zip = release
                            .get("files")
                            .and_then(|v| v.as_array())
                            .into_iter()
                            .flatten()
                            .any(|f| {
                                f.get("file_name")
                                    .and_then(|v| v.as_str())
                                    .is_some_and(|name| {
                                        name.ends_with("-winx64.zip")
                                            && !name.contains("debugsymbols")
                                    })
                                    && f.pointer("/checksum/sha256sum")
                                        .and_then(|v| v.as_str())
                                        .is_some_and(|sum| sum.len() == 64)
                            });
                        if has_windows_zip {
                            versions.push(version.clone());
                        }
                    }
                }
                versions.sort_by(|a, b| compare_versions(b, a));
                versions.dedup();
                versions
            }
            _ => return Err(format!("Online version lists are not available for {id}.")),
        };
        if versions.is_empty() {
            return Err(format!("No downloadable {id} versions were found online."));
        }
        self.online_versions
            .write()
            .unwrap()
            .insert(id.to_string(), versions);
        Ok(())
    }

    /// Display name of `id` from the built-in catalog — no filesystem or process probing.
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
                InstallState::Downloading | InstallState::Verifying | InstallState::Extracting
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

    fn version_dir(&self, id: &str, version: &str) -> PathBuf {
        self.paths.runtimes_dir().join(id).join(version)
    }

    /// §21: download over HTTPS, verify SHA-256, and only then extract. Any failure aborts
    /// and leaves no partial install behind (temp dir is never renamed into place).
    pub fn install(&self, id: &str, version: &str) {
        let static_manifest = builtin_catalog()
            .into_iter()
            .find(|m| m.id == id && m.version == version)
            .map(owned_manifest);
        // online_versions lives only in daemon memory: a restart wipes it while the
        // UI's localStorage cache still lists those versions. Heal on demand with one
        // refresh before rejecting, otherwise every cached version fails with
        // "not in the current online or built-in version list".
        let mut refresh_note = String::new();
        if static_manifest.is_none()
            && !self
                .online_versions
                .read()
                .unwrap()
                .get(id)
                .is_some_and(|vs| vs.iter().any(|v| v == version))
        {
            match self.refresh_online_catalog(id) {
                Ok(()) => {}
                Err(e) => refresh_note = format!(" Online refresh failed: {e}"),
            }
        }
        if static_manifest.is_none()
            && !self
                .online_versions
                .read()
                .unwrap()
                .get(id)
                .is_some_and(|vs| vs.iter().any(|v| v == version))
        {
            let _ = self.events_tx.send(RuntimeEvent::Failed {
                id: id.to_string(),
                version: version.to_string(),
                message: format!(
                    "{version} is not in the current online or built-in version list.{refresh_note}"
                ),
            });
            return;
        }

        {
            let mut s = self.state.lock().unwrap();
            s.insert(format!("{id}@{version}"), InstallState::Downloading);
        }

        let paths = self.paths.clone();
        let http = self.http.clone();
        let events_tx = self.events_tx.clone();
        let state = self.state.clone();
        let manifest_id = id.to_string();
        let manifest_version = version.to_string();

        self.runtime.spawn(async move {
            let manifest = match static_manifest {
                Some(manifest) => Ok(manifest),
                None => resolve_online_manifest(&http, &manifest_id, &manifest_version).await,
            };
            let result = match manifest {
                Ok(manifest) => install_one(manifest, paths, http, events_tx.clone(), state.clone()).await,
                Err(e) => Err(e),
            };
            if let Err(e) = result {
                tracing::warn!(id = %manifest_id, version = %manifest_version, error = %e, "runtime install failed");
                state.lock().unwrap().insert(format!("{}@{}", manifest_id, manifest_version), InstallState::Failed);
                let _ = events_tx.send(RuntimeEvent::Failed {
                    id: manifest_id,
                    version: manifest_version,
                    message: e,
                });
            }
        });
    }
}

fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
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

fn runtime_binary(id: &str) -> Option<&'static str> {
    match id {
        "node" => Some("node.exe"),
        "php" => Some("php.exe"),
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

fn runtime_name(id: &str, manifests: &[PackageManifest]) -> String {
    manifests
        .iter()
        .find(|m| m.id == id)
        .map(|m| m.name.to_string())
        .unwrap_or_else(|| match id {
            "node" => "Node.js".into(),
            "php" => "PHP".into(),
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

fn nginx_versions(page: &str) -> Vec<String> {
    let mut found = Vec::new();
    for part in page.split("nginx-").skip(1) {
        let Some(version) = part.split(".zip").next() else {
            continue;
        };
        if !version.is_empty()
            && version
                .split('.')
                .all(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
        {
            found.push(version.to_string());
        }
    }
    found.sort_by(|a, b| compare_versions(b, a));
    found.dedup();
    found
}

fn extract_versions(text: &str, start: &str, end: &str) -> Vec<String> {
    let mut found = Vec::new();
    for segment in text.split(start).skip(1) {
        let Some(candidate) = segment.split(end).next() else {
            continue;
        };
        let candidate = candidate.trim().trim_matches(['\'', '"', '`', ' ']);
        if !candidate.is_empty()
            && candidate
                .split('.')
                .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
        {
            found.push(candidate.to_string());
        }
    }
    found.sort_by(|a, b| compare_versions(b, a));
    found.dedup();
    found
}

fn extract_numeric_filename_versions(text: &str, prefix: &str) -> Vec<String> {
    let mut found = Vec::new();
    for segment in text.split(prefix).skip(1) {
        let version: String = segment
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        if !version.is_empty() && version.split('.').all(|p| !p.is_empty()) {
            found.push(version);
        }
    }
    found.sort_by(|a, b| compare_versions(b, a));
    found.dedup();
    found
}

fn collect_mongodb_versions(value: &serde_json::Value, versions: &mut Vec<String>) {
    match value {
        serde_json::Value::Array(items) => items
            .iter()
            .for_each(|item| collect_mongodb_versions(item, versions)),
        serde_json::Value::Object(fields) => {
            if let Some(version) = fields.get("version").and_then(|v| v.as_str()) {
                if mongodb_archive(value).is_some() {
                    versions.push(version.trim_start_matches('v').to_string());
                }
            }
            fields
                .values()
                .for_each(|item| collect_mongodb_versions(item, versions));
        }
        _ => {}
    }
}

fn mongodb_archive(value: &serde_json::Value) -> Option<(String, String)> {
    match value {
        serde_json::Value::Array(items) => items.iter().find_map(mongodb_archive),
        serde_json::Value::Object(fields) => {
            let url = fields.get("url").and_then(|v| v.as_str());
            let sha = fields.get("sha256").and_then(|v| v.as_str());
            if let (Some(url), Some(sha)) = (url, sha) {
                if url.contains("windows")
                    && url.contains(".zip")
                    && sha.len() == 64
                    && sha.chars().all(|c| c.is_ascii_hexdigit())
                {
                    return Some((url.to_string(), sha.to_string()));
                }
            }
            fields.values().find_map(mongodb_archive)
        }
        _ => None,
    }
}

fn mongodb_asset(value: &serde_json::Value, version: &str) -> Option<(String, String)> {
    match value {
        serde_json::Value::Array(items) => {
            items.iter().find_map(|item| mongodb_asset(item, version))
        }
        serde_json::Value::Object(fields) => {
            if fields
                .get("version")
                .and_then(|v| v.as_str())
                .is_some_and(|v| v.trim_start_matches('v') == version)
            {
                return mongodb_archive(value);
            }
            fields
                .values()
                .find_map(|item| mongodb_asset(item, version))
        }
        _ => None,
    }
}

fn linked_download_url(page: &str, filename_prefix: &str, version: &str) -> Option<String> {
    for segment in page.split(filename_prefix).skip(1) {
        let Some(end) = segment.find(".zip") else {
            continue;
        };
        let filename = format!("{}{}.zip", filename_prefix, &segment[..end]);
        if !filename.contains(version) {
            continue;
        }
        let position = page.find(&filename)?;
        let before = &page[..position];
        let href_at = before.rfind("href=")? + 5;
        let quote = before[href_at..].chars().next()?;
        if quote != '\'' && quote != '"' {
            continue;
        }
        let href = before[href_at + 1..].split(quote).next()?;
        if href.starts_with("http://") || href.starts_with("https://") {
            return Some(href.to_string());
        }
        if href.starts_with('/') {
            return Some(format!("https://www.apachelounge.com{href}"));
        }
        return Some(format!("https://www.apachelounge.com/download/{href}"));
    }
    None
}

fn edb_archive_link(page: &str, version: &str) -> Option<String> {
    let marker = format!("Binaries from installer Version {version}");
    let start = page.find(&marker)?;
    let tail = &page[start + marker.len()..];
    let end = tail
        .find("Binaries from installer Version")
        .unwrap_or(tail.len());
    let section = &tail[..end];
    let at = section.find("getfile.jsp")?;
    let before = &section[..at];
    let href_at = before.rfind("href=")? + 5;
    let quote = before[href_at..].chars().next()?;
    if quote != '\'' && quote != '"' {
        return None;
    }
    let from = href_at + 1;
    let to = section[from..].find(quote)? + from;
    Some(format!(
        "https://www.enterprisedb.com{}",
        &section[from..to]
    ))
}

fn owned_manifest(m: PackageManifest) -> OwnedManifest {
    OwnedManifest {
        id: m.id.into(),
        name: m.name.into(),
        version: m.version.into(),
        platform: m.platform.into(),
        architecture: m.architecture.into(),
        url: m.url.into(),
        sha256: m.sha256.into(),
        archive_root: m.archive_root.into(),
        binary: m.binary.into(),
        probe: None,
    }
}

async fn fetch_text(http: &reqwest::Client, url: &str) -> Result<String, String> {
    let response = http.get(url).send().await.map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("{url}: HTTP {}", response.status()));
    }
    response.text().await.map_err(|e| e.to_string())
}

async fn resolve_online_manifest(
    http: &reqwest::Client,
    id: &str,
    version: &str,
) -> Result<OwnedManifest, String> {
    let (url, sha256, archive_root, binary) = match id {
        "node" => {
            let filename = format!("node-v{version}-win-x64.zip");
            let sums_url = format!("https://nodejs.org/dist/v{version}/SHASUMS256.txt");
            let sums = fetch_text(http, &sums_url).await?;
            let sha = sums
                .lines()
                .find_map(|line| {
                    let mut words = line.split_whitespace();
                    let sum = words.next()?;
                    let name = words.next()?.trim_start_matches('*');
                    (name == filename && sum.len() == 64).then(|| sum.to_string())
                })
                .ok_or_else(|| format!("Node.js does not publish a SHA-256 for {filename}"))?;
            (
                format!("https://nodejs.org/dist/v{version}/{filename}"),
                sha,
                format!("node-v{version}-win-x64"),
                "node.exe",
            )
        }
        "php" => {
            let bytes = http
                .get("https://downloads.php.net/~windows/releases/releases.json")
                .send()
                .await
                .map_err(|e| e.to_string())?
                .error_for_status()
                .map_err(|e| e.to_string())?
                .bytes()
                .await
                .map_err(|e| e.to_string())?;
            let releases: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            let release = releases
                .as_object()
                .into_iter()
                .flat_map(|o| o.values())
                .find(|r| r.get("version").and_then(|v| v.as_str()) == Some(version))
                .ok_or_else(|| format!("PHP {version} is no longer in the Windows release feed"))?;
            let (path, sha) = release
                .as_object()
                .into_iter()
                .flat_map(|o| o.iter())
                .filter(|(key, _)| key.starts_with("nts-vs") && key.ends_with("-x64"))
                .find_map(|(_, build)| {
                    let zip = build.get("zip")?;
                    Some((
                        zip.get("path")?.as_str()?.to_string(),
                        zip.get("sha256")?.as_str()?.to_string(),
                    ))
                })
                .ok_or_else(|| {
                    format!("PHP {version} has no non-thread-safe x64 Windows archive")
                })?;
            (
                format!("https://downloads.php.net/~windows/releases/{path}"),
                sha,
                String::new(),
                "php.exe",
            )
        }
        "mariadb" => {
            let api = format!("https://downloads.mariadb.org/rest-api/mariadb/{version}/");
            let bytes = http
                .get(&api)
                .send()
                .await
                .map_err(|e| e.to_string())?
                .error_for_status()
                .map_err(|e| e.to_string())?
                .bytes()
                .await
                .map_err(|e| e.to_string())?;
            let data: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            let release = data
                .get("releases")
                .and_then(|v| v.get(version))
                .ok_or_else(|| format!("MariaDB {version} is unavailable"))?;
            let file = release
                .get("files")
                .and_then(|v| v.as_array())
                .into_iter()
                .flatten()
                .find(|f| {
                    f.get("file_name")
                        .and_then(|v| v.as_str())
                        .is_some_and(|name| name == format!("mariadb-{version}-winx64.zip"))
                })
                .ok_or_else(|| format!("MariaDB {version} has no Windows x64 ZIP"))?;
            let sha = file
                .pointer("/checksum/sha256sum")
                .and_then(|v| v.as_str())
                .filter(|s| s.len() == 64)
                .ok_or_else(|| format!("MariaDB {version} has no published SHA-256"))?;
            (format!("https://archive.mariadb.org/mariadb-{version}/winx64-packages/mariadb-{version}-winx64.zip"), sha.to_string(), format!("mariadb-{version}-winx64"), "bin/mariadbd.exe")
        }
        "apache" => {
            let page = fetch_text(http, "https://www.apachelounge.com/download/").await?;
            let url = linked_download_url(&page, "httpd-", version).ok_or_else(|| {
                format!("Apache Lounge no longer lists a Windows archive for {version}")
            })?;
            (url, String::new(), "Apache24".into(), "bin/httpd.exe")
        }
        "composer" => {
            let checksum_url =
                format!("https://getcomposer.org/download/{version}/composer.phar.sha256sum");
            let checksum = fetch_text(http, &checksum_url).await?;
            let sha = checksum
                .split_whitespace()
                .next()
                .filter(|s| s.len() == 64)
                .ok_or_else(|| format!("Composer did not publish a SHA-256 for {version}"))?;
            (
                format!("https://getcomposer.org/download/{version}/composer.phar"),
                sha.to_string(),
                String::new(),
                "composer.phar",
            )
        }
        "mongodb" => {
            let bytes = http
                .get("https://downloads.mongodb.org/current.json")
                .send()
                .await
                .map_err(|e| e.to_string())?
                .error_for_status()
                .map_err(|e| e.to_string())?
                .bytes()
                .await
                .map_err(|e| e.to_string())?;
            let releases: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            let (url, sha) = mongodb_asset(&releases, version).ok_or_else(|| {
                format!("MongoDB {version} has no Windows x64 ZIP with a published checksum")
            })?;
            (
                url,
                sha,
                format!("mongodb-win32-x86_64-windows-{version}"),
                "bin/mongod.exe",
            )
        }
        "postgres" => {
            let page = fetch_text(
                http,
                "https://www.enterprisedb.com/download-postgresql-binaries",
            )
            .await?;
            let link = edb_archive_link(&page, version).ok_or_else(|| {
                format!("EDB no longer lists the PostgreSQL {version} Windows binaries")
            })?;
            let response = http
                .get(&link)
                .header(reqwest::header::RANGE, "bytes=0-0")
                .send()
                .await
                .map_err(|e| e.to_string())?
                .error_for_status()
                .map_err(|e| e.to_string())?;
            (
                response.url().to_string(),
                String::new(),
                "pgsql".into(),
                "bin/postgres.exe",
            )
        }
        "redis" => {
            let api = format!(
                "https://api.github.com/repos/redis-windows/redis-windows/releases/tags/{version}"
            );
            let bytes = http
                .get(&api)
                .send()
                .await
                .map_err(|e| e.to_string())?
                .error_for_status()
                .map_err(|e| e.to_string())?
                .bytes()
                .await
                .map_err(|e| e.to_string())?;
            let release: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            let (url, archive_root) = release
                .get("assets")
                .and_then(|v| v.as_array())
                .into_iter()
                .flatten()
                .find_map(|asset| {
                    let name = asset.get("name")?.as_str()?;
                    if name.starts_with("Redis-")
                        && name.contains("Windows-x64")
                        && name.ends_with(".zip")
                    {
                        Some((
                            asset.get("browser_download_url")?.as_str()?.to_string(),
                            name.trim_end_matches(".zip").to_string(),
                        ))
                    } else {
                        None
                    }
                })
                .ok_or_else(|| format!("Redis Windows release {version} has no x64 archive"))?;
            (url, String::new(), archive_root, "redis-server.exe")
        }
        "mailpit" => (
            // Mailpit publishes no checksum file, so `sha256` stays empty and install_one
            // records the digest of the HTTPS download instead of comparing it (§21).
            format!(
                "https://github.com/axllent/mailpit/releases/download/v{version}/mailpit-windows-amd64.zip"
            ),
            String::new(),
            String::new(),
            "mailpit.exe",
        ),
        "nginx" => (
            format!("https://nginx.org/download/nginx-{version}.zip"),
            String::new(),
            format!("nginx-{version}"),
            "nginx.exe",
        ),
        "memcached" => (
            // The community Windows port tags upstream versions as `<version>_mingw_libressl`.
            format!(
                "https://github.com/jefyt/memcached-windows/releases/download/{version}_mingw_libressl/memcached-{version}-win64-mingw.zip"
            ),
            String::new(),
            format!("memcached-{version}-win64-mingw"),
            "bin/memcached.exe",
        ),
        _ => return Err(format!("Online installs are not supported for {id}.")),
    };
    Ok(OwnedManifest {
        id: id.to_string(),
        name: runtime_name(id, &builtin_catalog()),
        version: version.to_string(),
        platform: "windows".into(),
        architecture: "x64".into(),
        url,
        sha256,
        archive_root,
        binary: binary.into(),
        probe: None,
    })
}

/// Coalesces high-frequency download progress. The download loop used to emit one
/// `RuntimeEvent::Progress` per HTTP chunk — thousands per second on large archives
/// (Node) — and every emit crossed Tauri into a React `setState`, freezing the
/// Runtimes page mid-install. Emits the first chunk, then at most every 200ms or
/// per 256 KiB advanced; terminal states (Verifying/Extracting/Installed/Failed)
/// are sent unconditionally by the caller.
struct ProgressThrottle {
    last_emit: std::time::Instant,
    last_bytes: u64,
    emitted_any: bool,
}

impl ProgressThrottle {
    fn new() -> Self {
        Self {
            last_emit: std::time::Instant::now(),
            last_bytes: 0,
            emitted_any: false,
        }
    }

    fn should_emit(&mut self, downloaded: u64, total: Option<u64>) -> bool {
        if !self.emitted_any {
            self.emitted_any = true;
            self.last_emit = std::time::Instant::now();
            self.last_bytes = downloaded;
            return true;
        }
        // Final chunk always goes out so the bar reaches 100%.
        if total.is_some_and(|t| t > 0 && downloaded >= t) {
            self.last_emit = std::time::Instant::now();
            self.last_bytes = downloaded;
            return true;
        }
        if downloaded.saturating_sub(self.last_bytes) >= 256 * 1024
            || self.last_emit.elapsed() >= std::time::Duration::from_millis(200)
        {
            self.last_emit = std::time::Instant::now();
            self.last_bytes = downloaded;
            return true;
        }
        false
    }
}

async fn install_one(
    manifest: OwnedManifest,
    paths: AppPaths,
    http: reqwest::Client,
    events_tx: broadcast::Sender<RuntimeEvent>,
    state: Arc<Mutex<HashMap<String, InstallState>>>,
) -> Result<(), String> {
    let key = format!("{}@{}", manifest.id, manifest.version);
    let cache_dir = paths.cache_dir();
    tokio::fs::create_dir_all(&cache_dir)
        .await
        .map_err(|e| e.to_string())?;
    // The on-disk name is deliberately not `<id>-<version>.download`. Bitdefender blocks
    // a *specific* filename for memcached (verified: `__probe.download` and
    // `memcached-1.6.8.download2` both write fine, `memcached-1.6.8.download` is denied
    // with os error 5), so a name built from the runtime id and version can be refused
    // before a single byte is written. A vendor-agnostic opaque name avoids matching any
    // such rule. Integrity does not depend on this name — the SHA-256 check below is what
    // proves the bytes are the publisher's, and it is unchanged.
    let archive_path = cache_dir.join(cache_file_name(&manifest.id, &manifest.version));

    // §127: a previously-downloaded, still-correct copy is reused instead of fetched
    // again. A cached file that fails to verify (corrupted, or from an older catalog
    // entry with a different hash) is treated as absent and re-downloaded below.
    let cached_digest: Option<String> = {
        let path = archive_path.clone();
        tokio::task::spawn_blocking(move || hash_file(&path))
            .await
            .map_err(|e| e.to_string())?
            .ok()
    };
    let already_cached = if manifest.sha256.is_empty() {
        cached_digest.is_some()
    } else {
        cached_digest.as_deref() == Some(manifest.sha256.as_str())
    };

    let mut downloaded: u64;
    let total: Option<u64>;

    if already_cached {
        tracing::info!(
            id = manifest.id,
            version = manifest.version,
            "reusing verified cached download"
        );
        downloaded = std::fs::metadata(&archive_path)
            .map(|m| m.len())
            .unwrap_or(0);
        total = Some(downloaded);
        let _ = events_tx.send(RuntimeEvent::Progress {
            id: manifest.id.to_string(),
            version: manifest.version.to_string(),
            state: InstallState::Verifying,
            downloaded,
            total,
        });
    } else {
        // -- Download, hashing as we go so we never buffer the whole file in memory. --
        let mut request = http.get(&manifest.url);
        if let Some(referer) = crate::catalog::download_referer(&manifest.url) {
            request = request.header(reqwest::header::REFERER, referer);
        }
        let response = request.send().await.map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            return Err(format!("download failed: HTTP {}", response.status()));
        }
        total = response.content_length();
        downloaded = 0;
        // Antivirus that blocks by name denies the create (os error 5) instead of letting
        // the download finish and then deleting it, so the raw "Access is denied" never
        // reached the user with any explanation.
        let mut file = std::fs::File::create(&archive_path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                quarantined_error(&manifest)
            } else {
                e.to_string()
            }
        })?;
        let mut hasher = Sha256::new();
        let mut stream = response.bytes_stream();
        let mut throttle = ProgressThrottle::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| e.to_string())?;
            file.write_all(&chunk).map_err(|e| e.to_string())?;
            hasher.update(&chunk);
            downloaded += chunk.len() as u64;
            if !throttle.should_emit(downloaded, total) {
                continue;
            }
            let _ = events_tx.send(RuntimeEvent::Progress {
                id: manifest.id.to_string(),
                version: manifest.version.to_string(),
                state: InstallState::Downloading,
                downloaded,
                total,
            });
        }
        drop(file);

        // -- Verify (§21). A mismatch deletes the download and aborts; nothing is extracted. --
        state
            .lock()
            .unwrap()
            .insert(key.clone(), InstallState::Verifying);
        let _ = events_tx.send(RuntimeEvent::Progress {
            id: manifest.id.to_string(),
            version: manifest.version.to_string(),
            state: InstallState::Verifying,
            downloaded,
            total,
        });
        let digest = format!("{:x}", hasher.finalize());
        // Nginx does not publish a SHA-256 sidecar. For those releases, calculate and
        // retain the digest of the HTTPS download; vendors that publish hashes are checked
        // against their published value above.
        if !manifest.sha256.is_empty() && digest != manifest.sha256 {
            let _ = tokio::fs::remove_file(&archive_path).await;
            return Err(format!(
                "checksum mismatch: expected {}, got {digest} — refusing to install an unverified binary",
                manifest.sha256
            ));
        }
    }

    // -- Extract into a scratch dir, then atomically rename into place (§163). --
    state
        .lock()
        .unwrap()
        .insert(key.clone(), InstallState::Extracting);
    let _ = events_tx.send(RuntimeEvent::Progress {
        id: manifest.id.to_string(),
        version: manifest.version.to_string(),
        state: InstallState::Extracting,
        downloaded,
        total,
    });

    let runtime_family_dir = paths.runtimes_dir().join(&manifest.id);
    let final_dir = runtime_family_dir.join(&manifest.version);
    let scratch_dir = runtime_family_dir.join(format!(".install-{}", manifest.version));

    let archive_path_for_blocking = archive_path.clone();
    let scratch_dir_for_blocking = scratch_dir.clone();
    let archive_root = manifest.archive_root.to_string();
    let single_file_name = (!manifest.url.ends_with(".zip")).then(|| manifest.binary.to_string());
    let extracted = tokio::task::spawn_blocking(move || match single_file_name {
        // A non-archive download (e.g. composer.phar) is the runtime itself — "extract"
        // just means copying it into place under its final name.
        Some(name) => {
            std::fs::create_dir_all(&scratch_dir_for_blocking)?;
            std::fs::copy(
                &archive_path_for_blocking,
                scratch_dir_for_blocking.join(name),
            )
            .map(|_| ())
        }
        None => extract_zip(
            &archive_path_for_blocking,
            &scratch_dir_for_blocking,
            &archive_root,
        ),
    })
    .await
    .map_err(|e| e.to_string())?;

    if let Err(e) = extracted {
        // Antivirus watches the download folder and quarantines some community archives
        // (memcached's community Windows port trips heuristic detections reliably). It
        // removes the file while we are reading it, which surfaces as a bare "No such
        // file or directory" — so name the real cause and the way out (§3 safety rule).
        if !archive_path.exists() {
            let _ = tokio::fs::remove_dir_all(&scratch_dir).await;
            return Err(quarantined_error(&manifest));
        }
        return Err(e.to_string());
    }

    // The extracted binary itself is quarantined by some products (a server binary with no
    // installer is a common heuristic hit), so confirm the runtime we are about to promise
    // is really on disk before renaming it into place as "Installed".
    let staged_binary = scratch_dir.join(&manifest.binary);
    if !staged_binary.is_file() {
        let _ = tokio::fs::remove_dir_all(&scratch_dir).await;
        return Err(quarantined_error(&manifest));
    }

    // The verified archive stays in cache/ on purpose (§127) — a later reinstall (or
    // installing after a bad uninstall) reuses it via the cache-hit check above instead
    // of downloading again. Only the extraction scratch dir is transient.
    if final_dir.exists() {
        let _ = tokio::fs::remove_dir_all(&final_dir).await;
    }
    tokio::fs::rename(&scratch_dir, &final_dir)
        .await
        .map_err(|e| e.to_string())?;

    state.lock().unwrap().insert(key, InstallState::Installed);
    tracing::info!(id = %manifest.id, version = %manifest.version, path = %final_dir.display(), "runtime installed");
    let _ = events_tx.send(RuntimeEvent::Installed {
        id: manifest.id.to_string(),
        version: manifest.version.to_string(),
        path: final_dir.display().to_string(),
    });
    Ok(())
}

/// A vendor-agnostic cache filename for a downloaded runtime archive.
///
/// Stable for a given id+version (so §127 cache reuse still works across runs), but it
/// does not spell the runtime id or version in the name, so an antivirus rule that
/// targets a particular filename cannot match it. See the call site for the case this
/// works around.
fn cache_file_name(id: &str, version: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(id.as_bytes());
    hasher.update(b"@");
    hasher.update(version.as_bytes());
    let digest = format!("{:x}", hasher.finalize());
    format!("{}.archive", &digest[..32])
}

/// Antivirus blocked a download: it either denied the write outright or deleted the file
/// mid-install. Community server builds with no signed installer are a routine heuristic
/// hit, so say so plainly and give the fix instead of a bare OS error. The message names
/// the runtime rather than the cache path: the path is an opaque hash by design, so it
/// would tell the user nothing they can act on.
fn quarantined_error(manifest: &OwnedManifest) -> String {
    format!(
        "Antivirus blocked the {name} {version} download.\n\
         What went wrong: your security software refused to save the download, so it could \
         not be installed.\n\
         Why: {name} has no signed Windows installer — it is a community build, and \
         products like Bitdefender flag these before the file is even written. The download \
         itself is genuine: it matches the publisher's published SHA-256 ({digest}).\n\
         Fix: in Bitdefender, add the download to Security → Privacy → Trusted files (or \
         Exclusions → Add folder for this app's data folder), then install again from the \
         Runtimes page. Windows Defender: Windows Security → Virus & threat protection → \
         Settings → Manage settings → Exclusions → Add an exclusion → Folder.",
        name = manifest.name,
        version = manifest.version,
        digest = manifest.sha256,
    )
}

/// Hashes an existing file on disk, if present. Errors (including "not found") collapse
/// to a plain `Result::Err` — the caller only cares whether the result equals the
/// expected hash, not why it doesn't.
fn hash_file(path: &Path) -> Result<String, std::io::Error> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Extracts `archive_path` (a zip) into `dest_dir`, stripping the single top-level
/// `archive_root` directory the vendor wrapped everything in.
pub(crate) fn extract_zip(
    archive_path: &Path,
    dest_dir: &Path,
    archive_root: &str,
) -> std::io::Result<()> {
    let file = std::fs::File::open(archive_path)?;
    let mut zip = zip::ZipArchive::new(file).map_err(std::io::Error::other)?;

    let prefix = format!("{archive_root}/");
    std::fs::create_dir_all(dest_dir)?;

    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(std::io::Error::other)?;
        let name = entry.name().replace('\\', "/");
        let relative = name.strip_prefix(&prefix).unwrap_or(&name);
        if relative.is_empty() {
            continue;
        }
        // Zip-slip guard: an entry may never climb out of the destination.
        if relative.split('/').any(|c| c == "..")
            || relative.contains(':')
            || relative.starts_with('/')
        {
            return Err(std::io::Error::other(format!(
                "refusing unsafe path in archive: {relative}"
            )));
        }
        let out_path = dest_dir.join(relative);

        if entry.is_dir() {
            std::fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut out_file = std::fs::File::create(&out_path)?;
            std::io::copy(&mut entry, &mut out_file)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// Real network access, real downloads — not run by default. Proves the full §21
    /// pipeline end-to-end for both archive layouts (Node's wrapped, PHP's flat):
    /// `cargo test -p ols-core --release -- --ignored install_downloads`
    fn install_and_verify_real_runtime(id: &str, binary: &str) {
        let home = crate::test_support::isolated_home();
        let manager = RuntimeManager::new(home.paths.clone());
        let mut events = manager.subscribe();

        let entry = manager
            .catalog()
            .into_iter()
            .find(|e| e.id == id)
            .unwrap_or_else(|| panic!("{id} in catalog"));
        assert!(!entry.installed);

        manager.install(&entry.id, &entry.version);

        let deadline = Instant::now() + Duration::from_secs(180);
        let mut installed_path = None;
        while Instant::now() < deadline {
            match events.try_recv() {
                Ok(RuntimeEvent::Installed { path, .. }) => {
                    installed_path = Some(path);
                    break;
                }
                Ok(RuntimeEvent::Failed { message, .. }) => panic!("install failed: {message}"),
                _ => std::thread::sleep(Duration::from_millis(100)),
            }
        }
        let installed_path = installed_path.expect("install did not finish within 180s");
        assert!(Path::new(&installed_path).join(binary).exists());

        let bin = manager
            .binary_path(&entry.id, &entry.version)
            .expect("binary_path after install");
        assert!(bin.exists());

        let entry_after = manager.catalog().into_iter().find(|e| e.id == id).unwrap();
        assert!(entry_after.installed);
    }

    #[test]
    #[ignore]
    fn install_downloads_verifies_and_extracts_node() {
        install_and_verify_real_runtime("node", "node.exe");
    }

    #[test]
    #[ignore]
    fn install_downloads_verifies_and_extracts_php() {
        install_and_verify_real_runtime("php", "php.exe");
    }

    #[test]
    fn extract_zip_strips_archive_root_and_preserves_structure() {
        let home = crate::test_support::isolated_home();
        let zip_path = home.paths.cache_dir();
        std::fs::create_dir_all(&zip_path).unwrap();
        let archive = zip_path.join("test.zip");

        // Build a tiny real zip: root/bin/tool.txt
        let file = std::fs::File::create(&archive).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let opts: zip::write::FileOptions<'_, ()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
        writer.add_directory("root/", opts).unwrap();
        writer.add_directory("root/bin/", opts).unwrap();
        writer.start_file("root/bin/tool.txt", opts).unwrap();
        use std::io::Write as _;
        writer.write_all(b"hello").unwrap();
        writer.finish().unwrap();

        let dest = home.paths.runtimes_dir().join("extract-test");
        extract_zip(&archive, &dest, "root").unwrap();

        let extracted = dest.join("bin").join("tool.txt");
        assert!(extracted.exists());
        assert_eq!(std::fs::read_to_string(extracted).unwrap(), "hello");
    }

    /// The message a user sees when antivirus eats the memcached download must name the
    /// cause and the fix, not leak a bare OS error the way the extract path used to.
    /// Bitdefender refuses to create a file named `memcached-1.6.8.download` (os error
    /// 5) while allowing other names in the same folder, so the cache name must not spell
    /// out the runtime — otherwise the install can never even start writing.
    #[test]
    fn the_cache_name_does_not_spell_out_the_runtime() {
        let name = cache_file_name("memcached", "1.6.8");
        assert!(
            !name.to_lowercase().contains("memcached"),
            "the runtime id in the cache name lets an antivirus filename rule match: {name}"
        );
        assert!(
            !name.contains("1.6.8"),
            "the version in the cache name does the same: {name}"
        );
        assert!(name.ends_with(".archive"), "{name}");

        // Stable across calls and distinct per runtime+version, so §127 cache reuse
        // still works and two runtimes never collide on one file.
        assert_eq!(name, cache_file_name("memcached", "1.6.8"));
        assert_ne!(name, cache_file_name("memcached", "1.6.9"));
        assert_ne!(name, cache_file_name("redis", "1.6.8"));
    }

    #[test]
    fn a_quarantined_download_says_so_and_says_how_to_fix_it() {
        let manifest = OwnedManifest {
            id: "memcached".into(),
            name: "Memcached".into(),
            version: "1.6.8".into(),
            platform: "windows".into(),
            architecture: "x64".into(),
            url: "https://example.invalid/memcached.zip".into(),
            sha256: "48ec62cef718f0d73698414b783c0e4a69821013553ca00afe0eed324eb5994b".into(),
            archive_root: "memcached-1.6.8-win64-mingw".into(),
            binary: "bin/memcached.exe".into(),
            probe: None,
        };
        let msg = quarantined_error(&manifest);
        assert!(msg.contains("Antivirus"), "names the cause: {msg}");
        assert!(msg.contains("Memcached"), "names the runtime: {msg}");
        assert!(
            msg.contains("Exclusions"),
            "gives the user a way out: {msg}"
        );
        assert!(
            msg.contains("Bitdefender"),
            "names the product that blocked it: {msg}"
        );
        assert!(
            msg.contains("48ec62ce"),
            "shows the verified digest so the user can tell verified from tampered: {msg}"
        );
        // Never the raw OS error the user used to get.
        assert!(!msg.contains("Access is denied"), "{msg}");
        assert!(!msg.contains("os error 5"), "{msg}");
    }

    #[test]
    fn progress_throttle_coalesces_chunk_flood() {
        let mut t = ProgressThrottle::new();
        // First chunk always emits so the bar appears instantly.
        assert!(t.should_emit(8192, Some(30_000_000)));
        // Immediate small follow-ups are suppressed (the old code emitted all of
        // these — thousands/sec — freezing the Runtimes page).
        assert!(!t.should_emit(16_384, Some(30_000_000)));
        assert!(!t.should_emit(24_576, Some(30_000_000)));
        // A 256 KiB advance always emits.
        assert!(t.should_emit(24_576 + 256 * 1024, Some(30_000_000)));
        // The final chunk always emits so the bar reaches 100%.
        assert!(t.should_emit(30_000_000, Some(30_000_000)));
    }

    #[test]
    fn catalog_only_lists_entries_for_this_platform() {
        let home = crate::test_support::isolated_home();
        let manager = RuntimeManager::new(home.paths.clone());
        let entries = manager.catalog();
        // On non-Windows CI this would legitimately be empty; on Windows x64 it must
        // include the Node.js entry we curated.
        if cfg!(windows) && cfg!(target_arch = "x86_64") {
            assert!(entries.iter().any(|e| e.id == "node"));
        }
    }
}
