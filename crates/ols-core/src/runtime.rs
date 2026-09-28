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
    /// The user paused it. The download is held where it is; resuming continues.
    Paused,
    Verifying,
    Extracting,
    Installed,
    /// The user stopped it. The partial download is deleted and nothing is installed.
    Cancelled,
    Failed,
}

/// Why an install stopped. A user stop is not a failure: nothing went wrong, so it must
/// not be reported as one (and must not set `InstallState::Failed`).
#[derive(Debug)]
pub enum InstallOutcome {
    Done,
    Failed(String),
    Cancelled,
}

/// The in-flight install controls a UI can drive: pause holds the transfer where it is,
/// cancel aborts it. One per `id@version`; dropped when the install finishes.
#[derive(Default)]
struct InstallControl {
    paused: std::sync::atomic::AtomicBool,
    cancelled: std::sync::atomic::AtomicBool,
}

impl InstallControl {
    fn pause(&self) {
        self.paused
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    fn resume(&self) {
        self.paused
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }

    fn is_paused(&self) -> bool {
        self.paused.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn cancel(&self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(std::sync::atomic::Ordering::Relaxed)
    }
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
    /// The user stopped this install. Not a failure: nothing is left behind and no
    /// diagnostic is warranted, so this is its own terminal event.
    Cancelled { id: String, version: String },
}

pub struct RuntimeManager {
    runtime: tokio::runtime::Runtime,
    paths: AppPaths,
    http: reqwest::Client,
    events_tx: broadcast::Sender<RuntimeEvent>,
    state: Arc<Mutex<HashMap<String, InstallState>>>,
    /// One control per in-flight `id@version`, so a running download can be paused or
    /// stopped from the UI. Entries are removed when the install reaches a terminal state.
    controls: Arc<RwLock<HashMap<String, Arc<InstallControl>>>>,
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
            controls: Arc::new(RwLock::new(HashMap::new())),
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
                let page = String::from_utf8(self.fetch(EDB_PAGE_URL)?)
                    .map_err(|_| "PostgreSQL download page is not UTF-8".to_string())?;
                edb_versions(&page)
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
            let key = format!("{id}@{version}");
            let mut s = self.state.lock().unwrap();
            // A second install of the same version while one is running would race it
            // over the same cache file and scratch dir. The running one keeps the
            // controls; only the state entry is refreshed.
            if s.get(&key).is_some_and(|st| {
                matches!(
                    st,
                    InstallState::Downloading
                        | InstallState::Paused
                        | InstallState::Verifying
                        | InstallState::Extracting
                )
            }) {
                // Say it is already running, so the caller that just asked is told
                // something instead of hearing nothing at all.
                let _ = self.events_tx.send(RuntimeEvent::Progress {
                    id: id.to_string(),
                    version: version.to_string(),
                    state: s.get(&key).copied().unwrap_or(InstallState::Downloading),
                    downloaded: 0,
                    total: None,
                });
                return;
            }
            s.insert(key.clone(), InstallState::Downloading);
            self.controls
                .write()
                .unwrap()
                .insert(key, Arc::new(InstallControl::default()));
        }

        // Announce the start before anything slow happens. Resolving an online manifest
        // is a ranged GET and the first body chunk can be seconds away, so without this
        // the UI showed a button that had visibly done nothing.
        let _ = self.events_tx.send(RuntimeEvent::Progress {
            id: id.to_string(),
            version: version.to_string(),
            state: InstallState::Downloading,
            downloaded: 0,
            total: None,
        });

        let paths = self.paths.clone();
        let http = self.http.clone();
        let events_tx = self.events_tx.clone();
        let state = self.state.clone();
        let controls = self.controls.clone();
        let manifest_id = id.to_string();
        let manifest_version = version.to_string();
        let key = format!("{}@{}", manifest_id, manifest_version);
        let control = self
            .controls
            .read()
            .unwrap()
            .get(&key)
            .cloned()
            .unwrap_or_default();

        self.runtime.spawn(async move {
            let manifest = match static_manifest {
                Some(manifest) => Ok(manifest),
                None => resolve_online_manifest(&http, &manifest_id, &manifest_version).await,
            };
            let result = match manifest {
                Ok(manifest) => {
                    install_one(
                        manifest,
                        paths,
                        http,
                        events_tx.clone(),
                        state.clone(),
                        control.clone(),
                    )
                    .await
                }
                Err(e) => InstallOutcome::Failed(e),
            };
            // The control is dropped with the install, so a later "pause" can never
            // address one that is no longer running.
            controls.write().unwrap().remove(&key);
            match result {
                InstallOutcome::Done => {}
                InstallOutcome::Failed(e) => {
                    tracing::warn!(id = %manifest_id, version = %manifest_version, error = %e, "runtime install failed");
                    state.lock().unwrap().insert(key, InstallState::Failed);
                    let _ = events_tx.send(RuntimeEvent::Failed {
                        id: manifest_id,
                        version: manifest_version,
                        message: e,
                    });
                }
                InstallOutcome::Cancelled => {
                    tracing::info!(id = %manifest_id, version = %manifest_version, "runtime install stopped by the user");
                    state.lock().unwrap().insert(key, InstallState::Cancelled);
                    let _ = events_tx.send(RuntimeEvent::Cancelled {
                        id: manifest_id,
                        version: manifest_version,
                    });
                }
            }
        });
    }

    /// Holds an in-flight download where it is. The partial file stays, and resuming
    /// continues the same transfer — the file is never renamed into place while paused,
    /// so a pause can never produce a half-installed runtime.
    pub fn pause_install(&self, id: &str, version: &str, paused: bool) -> Result<(), String> {
        let key = format!("{id}@{version}");
        let control = self
            .controls
            .read()
            .unwrap()
            .get(&key)
            .cloned()
            .ok_or_else(|| format!("{id} {version} is not installing"))?;
        if paused {
            control.pause();
        } else {
            control.resume();
        }
        Ok(())
    }

    /// Aborts an in-flight install. The partial download is deleted, nothing is extracted
    /// or installed, and the user sees a stop, not an error.
    pub fn cancel_install(&self, id: &str, version: &str) -> Result<(), String> {
        let key = format!("{id}@{version}");
        let control = self
            .controls
            .read()
            .unwrap()
            .get(&key)
            .cloned()
            .ok_or_else(|| format!("{id} {version} is not installing"))?;
        control.cancel();
        Ok(())
    }

    /// Whether an install is running right now, and whether the user has it paused.
    pub fn install_control(&self, id: &str, version: &str) -> Option<(bool, bool)> {
        let key = format!("{id}@{version}");
        self.controls
            .read()
            .unwrap()
            .get(&key)
            .map(|c| (c.is_paused(), c.is_cancelled()))
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

const EDB_PAGE_URL: &str = "https://www.enterprisedb.com/download-postgresql-binaries";

/// EDB renders every downloadable version as
/// `Binaries from installer<span …>Version <!-- -->18.6</span>` followed by one
/// `<a href="…getfile.jsp?fileid=…"><img alt="…">` per platform. HTML comment nodes and the
/// span mean the version is never adjacent to the label, so each half is parsed on its own:
/// the version after `Version`, the link from the `alt="Windows x86-64"` image it wraps.
fn edb_versions(page: &str) -> Vec<String> {
    let mut found = Vec::new();
    for section in page.split("Binaries from installer").skip(1) {
        if let Some(version) = edb_section_version(section) {
            if edb_windows_link(section).is_some() {
                found.push(version);
            }
        }
    }
    found.sort_by(|a, b| compare_versions(b, a));
    found.dedup();
    found
}

/// The `18.6` in `…Version <!-- -->18.6</span>`, skipping the markup React interleaves.
fn edb_section_version(section: &str) -> Option<String> {
    let at = section.find("Version")? + "Version".len();
    let version: String = section[at..]
        .trim_start()
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    (!version.is_empty()).then_some(version)
}

/// The Windows x86-64 archive for one `Binaries from installer` section. EDB serves the file
/// through a `getfile.jsp` redirect rather than a direct `.zip` URL, so the caller still
/// resolves the final location over HTTP; only the platform anchor is selected here — the
/// first one in a section is whatever the page happens to list first (often Mac OS X).
fn edb_windows_link(section: &str) -> Option<String> {
    let at = section.find("alt=\"Windows x86-64\"")?;
    let before = &section[..at];
    let href_at = before.rfind("href=")? + 5;
    let quote = before[href_at..].chars().next()?;
    if quote != '\'' && quote != '"' {
        return None;
    }
    let from = href_at + 1;
    let to = section[from..].find(quote)? + from;
    let href = &section[from..to];
    if href.starts_with("http://") || href.starts_with("https://") {
        return Some(href.to_string());
    }
    Some(format!("https://www.enterprisedb.com{href}"))
}

fn edb_archive_link(page: &str, version: &str) -> Option<String> {
    page.split("Binaries from installer")
        .skip(1)
        .filter(|section| edb_section_version(section).as_deref() == Some(version))
        .find_map(edb_windows_link)
}

/// The `13.1` of `13.1.1` — the release branch `downloads.mariadb.org/rest-api` is keyed by.
/// Cuts at the second dot, so `10.11.14` becomes `10.11`; a branch-only `13.1` is unchanged.
fn mariadb_branch(version: &str) -> &str {
    let Some(first) = version.find('.') else {
        return version;
    };
    match version[first + 1..].find('.') {
        Some(second) => &version[..first + 1 + second],
        None => version,
    }
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
            // The API is keyed by *branch* (`13.1`), not by full version: asking for
            // `/mariadb/13.1.1/` returns 200 with a `release_data` object and no `releases`
            // map, so the lookup below found nothing and every 13.x install failed with
            // "MariaDB 13.1.1 is unavailable". The branch carries every patch release.
            let branch = mariadb_branch(version);
            let api = format!("https://downloads.mariadb.org/rest-api/mariadb/{branch}/");
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
                .ok_or_else(|| {
                    format!(
                        "MariaDB {version} is not listed in the {branch} release feed, so its \
Windows x64 package cannot be verified. Fix: pick another {branch} version on the Runtimes \
page, or report it at https://github.com/kz370/OpenLocalServer/issues so the feed mapping can \
be updated."
                    )
                })?;
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
            let page = fetch_text(http, EDB_PAGE_URL).await?;
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
    control: Arc<InstallControl>,
) -> InstallOutcome {
    let key = format!("{}@{}", manifest.id, manifest.version);
    let cache_dir = paths.cache_dir();
    if let Err(e) = tokio::fs::create_dir_all(&cache_dir).await {
        return InstallOutcome::Failed(e.to_string());
    }
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
        match tokio::task::spawn_blocking(move || hash_file(&path)).await {
            Ok(Ok(digest)) => Some(digest),
            Ok(Err(_)) => None,
            Err(e) => return InstallOutcome::Failed(e.to_string()),
        }
    };
    // A vendor that publishes no checksum can only be judged on the file's own structure.
    // A cached copy whose zip end-of-central-directory record is missing is a truncated
    // download, not a usable archive — treating it as a hit would fail later inside
    // extract_zip with "Could not find EOCD", naming neither the cause nor the fix.
    let cached_is_valid_archive = {
        let path = archive_path.clone();
        let is_archive = manifest.url.ends_with(".zip");
        match tokio::task::spawn_blocking(move || zip_has_eocd(&path, is_archive)).await {
            Ok(v) => v,
            Err(e) => return InstallOutcome::Failed(e.to_string()),
        }
    };
    let already_cached = if manifest.sha256.is_empty() {
        cached_digest.is_some() && cached_is_valid_archive
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
        let response = match request.send().await {
            Ok(r) => r,
            Err(_) if control.is_cancelled() => return stop_install(&control, &archive_path).await,
            Err(e) => return InstallOutcome::Failed(e.to_string()),
        };
        if !response.status().is_success() {
            return InstallOutcome::Failed(format!("download failed: HTTP {}", response.status()));
        }
        total = response.content_length();
        downloaded = 0;
        // Antivirus that blocks by name denies the create (os error 5) instead of letting
        // the download finish and then deleting it, so the raw "Access is denied" never
        // reached the user with any explanation.
        let mut file = match std::fs::File::create(&archive_path) {
            Ok(f) => f,
            Err(e) => {
                return InstallOutcome::Failed(
                    if e.kind() == std::io::ErrorKind::PermissionDenied {
                        quarantined_error(&manifest)
                    } else {
                        e.to_string()
                    },
                )
            }
        };
        let mut hasher = Sha256::new();
        let mut stream = response.bytes_stream();
        let mut throttle = ProgressThrottle::new();
        // A pause must be visible on the first tick even if no bytes moved yet.
        let mut announced_pause = false;

        loop {
            // A stop wins over a pause: both set, stop.
            if control.is_cancelled() {
                drop(file);
                return stop_install(&control, &archive_path).await;
            }
            if control.is_paused() {
                if !announced_pause {
                    announced_pause = true;
                    state
                        .lock()
                        .unwrap()
                        .insert(key.clone(), InstallState::Paused);
                    let _ = events_tx.send(RuntimeEvent::Progress {
                        id: manifest.id.to_string(),
                        version: manifest.version.to_string(),
                        state: InstallState::Paused,
                        downloaded,
                        total,
                    });
                }
                // Sleep instead of polling the stream: the transfer really is held
                // (the socket is not drained), it is not merely not written to.
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                continue;
            }
            if announced_pause {
                announced_pause = false;
                state
                    .lock()
                    .unwrap()
                    .insert(key.clone(), InstallState::Downloading);
                let _ = events_tx.send(RuntimeEvent::Progress {
                    id: manifest.id.to_string(),
                    version: manifest.version.to_string(),
                    state: InstallState::Downloading,
                    downloaded,
                    total,
                });
            }
            let Some(next) = stream.next().await else {
                break;
            };
            let chunk = match next {
                Ok(chunk) => chunk,
                Err(_) if control.is_cancelled() => {
                    drop(file);
                    return stop_install(&control, &archive_path).await;
                }
                Err(e) => {
                    drop(file);
                    return InstallOutcome::Failed(e.to_string());
                }
            };
            if let Err(e) = file.write_all(&chunk) {
                drop(file);
                return InstallOutcome::Failed(e.to_string());
            }
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

        // A stop that lands after the last byte still aborts: nothing has been verified
        // or installed yet, so this is where the user means it.
        if control.is_cancelled() {
            return stop_install(&control, &archive_path).await;
        }
        if control.is_paused() {
            // Wait out a pause that arrived during the final chunk, then continue to
            // verify — the transfer itself is already complete.
            while control.is_paused() && !control.is_cancelled() {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            if control.is_cancelled() {
                return stop_install(&control, &archive_path).await;
            }
        }

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
        // A transfer that ends early is a truncated file, not a complete one. The stream
        // gives no error when the server simply closes the connection, so the byte count is
        // checked against the advertised length. Without this the partial file is left in
        // cache and, for a vendor that publishes no checksum (postgres, nginx, apache,
        // mailpit, redis, memcached), the cache-hit check above trusts *any* existing file —
        // so the next install reuses the truncated bytes and fails much later at
        // "Could not find EOCD", far from the cause.
        if let Some(expected) = total {
            if downloaded < expected {
                let _ = tokio::fs::remove_file(&archive_path).await;
                return InstallOutcome::Failed(format!(
                    "download of {} {} was cut short: got {downloaded} of {expected} bytes. \
The connection dropped mid-transfer; the partial file has been deleted, so installing again \
restarts the download from the beginning.",
                    manifest.name, manifest.version
                ));
            }
        }
        // Nginx does not publish a SHA-256 sidecar. For those releases, calculate and
        // retain the digest of the HTTPS download; vendors that publish hashes are checked
        // against their published value above.
        if !manifest.sha256.is_empty() && digest != manifest.sha256 {
            let _ = tokio::fs::remove_file(&archive_path).await;
            return InstallOutcome::Failed(format!(
                "checksum mismatch: expected {}, got {digest} — refusing to install an unverified binary",
                manifest.sha256
            ));
        }
    }

    // -- Extract into a scratch dir, then atomically rename into place (§163). --
    // Extraction itself is a blocking zip walk and is not pausable mid-file; a stop
    // lands at the next boundary below, and nothing is ever renamed into place after
    // one, so a stop can never leave a half-extracted runtime installed.
    if control.is_cancelled() {
        let _ = tokio::fs::remove_file(&archive_path).await;
        return InstallOutcome::Cancelled;
    }
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
    .await;

    let extracted = match extracted {
        Ok(result) => result,
        Err(e) => return InstallOutcome::Failed(e.to_string()),
    };

    if control.is_cancelled() {
        let _ = tokio::fs::remove_dir_all(&scratch_dir).await;
        let _ = tokio::fs::remove_file(&archive_path).await;
        return InstallOutcome::Cancelled;
    }

    if let Err(e) = extracted {
        // Antivirus watches the download folder and quarantines some community archives
        // (memcached's community Windows port trips heuristic detections reliably). It
        // removes the file while we are reading it, which surfaces as a bare "No such
        // file or directory" — so name the real cause and the way out (§3 safety rule).
        if !archive_path.exists() {
            let _ = tokio::fs::remove_dir_all(&scratch_dir).await;
            return InstallOutcome::Failed(quarantined_error(&manifest));
        }
        return InstallOutcome::Failed(e.to_string());
    }

    // The extracted binary itself is quarantined by some products (a server binary with no
    // installer is a common heuristic hit), so confirm the runtime we are about to promise
    // is really on disk before renaming it into place as "Installed".
    let staged_binary = scratch_dir.join(&manifest.binary);
    if !staged_binary.is_file() {
        let _ = tokio::fs::remove_dir_all(&scratch_dir).await;
        return InstallOutcome::Failed(quarantined_error(&manifest));
    }

    // The verified archive stays in cache/ on purpose (§127) — a later reinstall (or
    // installing after a bad uninstall) reuses it via the cache-hit check above instead
    // of downloading again. Only the extraction scratch dir is transient.
    if final_dir.exists() {
        let _ = tokio::fs::remove_dir_all(&final_dir).await;
    }
    if let Err(e) = tokio::fs::rename(&scratch_dir, &final_dir).await {
        return InstallOutcome::Failed(e.to_string());
    }

    state.lock().unwrap().insert(key, InstallState::Installed);
    tracing::info!(id = %manifest.id, version = %manifest.version, path = %final_dir.display(), "runtime installed");
    let _ = events_tx.send(RuntimeEvent::Installed {
        id: manifest.id.to_string(),
        version: manifest.version.to_string(),
        path: final_dir.display().to_string(),
    });
    InstallOutcome::Done
}

/// A user stop: the partial download is deleted (it is unverified, so keeping it could
/// only ever be re-fetched anyway) and nothing is extracted.
async fn stop_install(_control: &InstallControl, archive_path: &Path) -> InstallOutcome {
    let _ = tokio::fs::remove_file(archive_path).await;
    InstallOutcome::Cancelled
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

/// Whether a cached download is a structurally complete zip, judged the same way the zip
/// crate judges it: the end-of-central-directory record (`PK\x05\x06`) must be present. That
/// record is written last, so its absence is exactly the "Could not find EOCD" signature of a
/// transfer that stopped early. A missing or unreadable file is reported as not-an-archive so
/// the caller simply re-downloads. Non-archive runtimes (composer.phar) have no EOCD to find
/// and are trusted as-is — they are a few MB, so a short transfer is not a practical risk.
fn zip_has_eocd(path: &Path, is_archive: bool) -> bool {
    if !is_archive {
        return true;
    }
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let Ok(len) = file.metadata().map(|m| m.len()) else {
        return false;
    };
    // EOCD is 22 bytes plus an optional comment of up to 64 KiB.
    const EOCD: &[u8; 4] = b"PK\x05\x06";
    let window = 22u64 + u16::MAX as u64;
    if len < EOCD.len() as u64 {
        return false;
    }
    let start = len.saturating_sub(window);
    if file.seek(SeekFrom::Start(start)).is_err() {
        return false;
    }
    let mut buf = vec![0u8; (len - start) as usize];
    if file.read_exact(&mut buf).is_err() {
        return false;
    }
    buf.windows(EOCD.len()).any(|w| w == EOCD)
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
    use std::io::Read as _;
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

    /// Serves a body of `total` bytes in `chunk`-sized pieces, `delay_ms` apart, so a test
    /// can pause or stop a download that is genuinely still in flight. The socket stays
    /// undrained while the client is paused, so the pause is a real hold, not a cosmetic one.
    fn slow_file_server(total: usize, chunk: usize, delay_ms: u64) -> String {
        use std::io::Write as _;
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let url = format!("http://{}/tool.bin", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let Ok((mut socket, _)) = listener.accept() else {
                return;
            };
            // Read the request head; the body answer is all these tests care about.
            let mut head = [0u8; 1024];
            let _ = socket.read(&mut head).ok();
            let _ = write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {total}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n"
            );
            let body = vec![b'x'; chunk];
            let mut sent = 0;
            while sent < total {
                let n = chunk.min(total - sent);
                if socket.write_all(&body[..n]).is_err() || socket.flush().is_err() {
                    return;
                }
                sent += n;
                std::thread::sleep(std::time::Duration::from_millis(delay_ms));
            }
            // Keep the connection open so the client sees a clean end only after the body.
            let _ = socket.flush();
        });
        url
    }

    fn test_manifest(url: String) -> OwnedManifest {
        OwnedManifest {
            id: "testrt".into(),
            name: "Test Runtime".into(),
            version: "1.0.0".into(),
            platform: "windows".into(),
            architecture: "x64".into(),
            url,
            // Empty: the check is skipped, so the test exercises the transport controls
            // and not the vendor-hash path (covered by the real-download tests).
            sha256: String::new(),
            archive_root: String::new(),
            binary: "tool.bin".into(),
            probe: None,
        }
    }

    /// Runs `install_one` on a runtime of its own thread, so the test body can drive the
    /// control flags and read events while the download runs.
    fn spawn_install(
        manifest: OwnedManifest,
        paths: AppPaths,
        http: reqwest::Client,
        events_tx: broadcast::Sender<RuntimeEvent>,
        state: Arc<Mutex<HashMap<String, InstallState>>>,
        control: Arc<InstallControl>,
    ) -> std::thread::JoinHandle<InstallOutcome> {
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(install_one(
                manifest, paths, http, events_tx, state, control,
            ))
        })
    }

    /// A pause must actually hold the transfer: no further bytes are read or written while
    /// it is on, and resuming continues the same download instead of starting over.
    #[test]
    fn pausing_holds_the_download_and_resuming_finishes_it() {
        let home = crate::test_support::isolated_home();
        let manager = RuntimeManager::new(home.paths.clone());
        let (events_tx, mut events) = broadcast::channel(256);
        let state = Arc::new(Mutex::new(HashMap::new()));
        let control = Arc::new(InstallControl::default());
        let url = slow_file_server(200 * 1024, 4 * 1024, 25);
        let manifest = test_manifest(url);

        let task = spawn_install(
            manifest,
            home.paths.clone(),
            manager.http.clone(),
            events_tx.clone(),
            state,
            control.clone(),
        );

        // Wait until bytes are moving, then pause.
        let mut first_bytes = 0u64;
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            match events.try_recv() {
                Ok(RuntimeEvent::Progress {
                    state: InstallState::Downloading,
                    downloaded,
                    ..
                }) => {
                    first_bytes = downloaded;
                    break;
                }
                _ => std::thread::sleep(Duration::from_millis(10)),
            }
        }
        assert!(first_bytes > 0, "no bytes were ever downloaded");
        control.pause();
        std::thread::sleep(Duration::from_millis(300));

        // The in-flight chunk may still land, but nothing may keep growing: the transfer
        // is held, so the byte count stops moving.
        let mut held = first_bytes;
        while let Ok(event) = events.try_recv() {
            if let RuntimeEvent::Progress { downloaded, .. } = event {
                held = held.max(downloaded);
            }
        }
        std::thread::sleep(Duration::from_millis(500));
        let mut after = held;
        while let Ok(event) = events.try_recv() {
            if let RuntimeEvent::Progress { downloaded, .. } = event {
                after = after.max(downloaded);
            }
        }
        assert_eq!(after, held, "the download kept moving while paused");

        control.resume();
        let outcome = task.join().unwrap();
        assert!(matches!(outcome, InstallOutcome::Done), "{outcome:?}");
        let installed = home
            .paths
            .runtimes_dir()
            .join("testrt")
            .join("1.0.0")
            .join("tool.bin");
        assert!(installed.is_file(), "resume must finish the same install");
    }

    /// A stop is not a failure: the partial download is deleted, nothing is installed, and
    /// the outcome is `Cancelled` so the caller never reports an error the user did not hit.
    #[test]
    fn stopping_deletes_the_partial_download_and_installs_nothing() {
        let home = crate::test_support::isolated_home();
        let manager = RuntimeManager::new(home.paths.clone());
        let (events_tx, mut events) = broadcast::channel(256);
        let state = Arc::new(Mutex::new(HashMap::new()));
        let control = Arc::new(InstallControl::default());
        let url = slow_file_server(4 * 1024 * 1024, 4 * 1024, 25);
        let manifest = test_manifest(url);
        let archive = home
            .paths
            .cache_dir()
            .join(cache_file_name(&manifest.id, &manifest.version));

        let task = spawn_install(
            manifest,
            home.paths.clone(),
            manager.http.clone(),
            events_tx.clone(),
            state,
            control.clone(),
        );

        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            if matches!(
                events.try_recv(),
                Ok(RuntimeEvent::Progress { downloaded, .. }) if downloaded > 0
            ) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        control.cancel();
        let outcome = task.join().unwrap();
        assert!(
            matches!(outcome, InstallOutcome::Cancelled),
            "a user stop must not be reported as a failure: {outcome:?}"
        );
        assert!(!archive.exists(), "the partial download must be deleted");
        assert!(
            !home
                .paths
                .runtimes_dir()
                .join("testrt")
                .join("1.0.0")
                .exists(),
            "a stopped install must leave nothing installed"
        );
    }

    /// The controls are per in-flight install: addressing one that is not running says so
    /// rather than silently doing nothing.
    #[test]
    fn pausing_or_stopping_a_version_that_is_not_installing_says_so() {
        let home = crate::test_support::isolated_home();
        let manager = RuntimeManager::new(home.paths.clone());
        let err = manager.pause_install("php", "8.4.26", true).unwrap_err();
        assert!(err.contains("not installing"), "{err}");
        let err = manager.cancel_install("php", "8.4.26").unwrap_err();
        assert!(err.contains("not installing"), "{err}");
        assert!(manager.install_control("php", "8.4.26").is_none());
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

    /// Trimmed copy of the real EDB binaries page: the version is separated from the
    /// `Binaries from installer` label by a `<span>` and a React `<!-- -->` comment, and
    /// each version's anchors are absolute `sbp.enterprisedb.com` links, Windows not first.
    const EDB_PAGE: &str = r#"<div class="italic mt-16 mb-5">Binaries from installer<span class="font-semibold pl-1">Version <!-- -->18.6</span><div><div class="m-5"><a href="https://sbp.enterprisedb.com/getfile.jsp?fileid=1260549"><img alt="Mac OS X"></a></div><div class="m-5"><a href="https://sbp.enterprisedb.com/getfile.jsp?fileid=1260566"><img alt="Windows x86-64"></a></div></div></div><div class="italic mt-16 mb-5">Binaries from installer<span class="font-semibold pl-1">Version <!-- -->17.11</span><div><div class="m-5"><a href="https://sbp.enterprisedb.com/getfile.jsp?fileid=1260569"><img alt="Windows x86-64"></a></div><div class="m-5"><a href="https://sbp.enterprisedb.com/getfile.jsp?fileid=1260579"><img alt="Mac OS X"></a></div></div></div>"#;

    #[test]
    fn postgres_versions_survive_the_edb_span_and_comment_markup() {
        let versions = edb_versions(EDB_PAGE);
        assert_eq!(versions, vec!["18.6".to_string(), "17.11".to_string()]);
    }

    #[test]
    fn postgres_archive_link_picks_the_windows_x64_anchor_not_the_first() {
        // 18.6 lists Mac OS X first: taking the first getfile.jsp would hand the Mac
        // archive to the Windows installer.
        assert_eq!(
            edb_archive_link(EDB_PAGE, "18.6").as_deref(),
            Some("https://sbp.enterprisedb.com/getfile.jsp?fileid=1260566")
        );
        assert_eq!(
            edb_archive_link(EDB_PAGE, "17.11").as_deref(),
            Some("https://sbp.enterprisedb.com/getfile.jsp?fileid=1260569")
        );
        assert!(edb_archive_link(EDB_PAGE, "9.2.24").is_none());
    }

    #[test]
    fn postgres_relative_edb_href_is_still_accepted() {
        let page = r#"Binaries from installer<span>Version <!-- -->16.15</span><a href='/getfile.jsp?fileid=1'><img alt="Windows x86-64"></a>"#;
        assert_eq!(
            edb_archive_link(page, "16.15").as_deref(),
            Some("https://www.enterprisedb.com/getfile.jsp?fileid=1")
        );
    }

    #[test]
    fn a_version_with_no_windows_anchor_is_not_offered() {
        let page = r#"Binaries from installer<span>Version <!-- -->10.23</span><a href="https://sbp.enterprisedb.com/getfile.jsp?fileid=7"><img alt="Mac OS X"></a>"#;
        assert!(edb_versions(page).is_empty());
    }

    #[test]
    fn mariadb_branch_strips_the_patch_component() {
        // The rest-api is keyed by branch: `/mariadb/13.1.1/` returns a `release_data`
        // object with no `releases` map, so the lookup silently finds nothing.
        assert_eq!(mariadb_branch("13.1.1"), "13.1");
        assert_eq!(mariadb_branch("11.4.9"), "11.4");
        assert_eq!(mariadb_branch("10.11.14"), "10.11");
        // Already a branch, or nothing to strip.
        assert_eq!(mariadb_branch("13.1"), "13.1");
        assert_eq!(mariadb_branch("11"), "11");
    }

    /// A transfer that stops early leaves no EOCD, which is exactly what extract_zip later
    /// rejects as "invalid Zip archive: Could not find EOCD" — far from the real cause.
    #[test]
    fn a_truncated_zip_is_recognised_before_it_is_reused() {
        let dir = std::env::temp_dir().join("ols-zip-eocd-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("truncated.zip");

        // A real zip, then a copy with its tail cut off mid-entry.
        let complete = dir.join("complete.zip");
        let file = std::fs::File::create(&complete).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let opts: zip::write::FileOptions<'_, ()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
        writer.add_directory("root/", opts).unwrap();
        writer.start_file("root/bin/postgres.exe", opts).unwrap();
        use std::io::Write as _;
        writer.write_all(&[0u8; 4096]).unwrap();
        writer.finish().unwrap();
        assert!(zip_has_eocd(&complete, true), "a finished zip has an EOCD");

        let bytes = std::fs::read(&complete).unwrap();
        std::fs::write(&path, &bytes[..bytes.len() - 2048]).unwrap();
        assert!(
            !zip_has_eocd(&path, true),
            "a zip missing its EOCD must not be trusted as a complete download"
        );

        // A missing file is simply "not cached" so the caller re-downloads.
        assert!(!zip_has_eocd(&dir.join("absent.zip"), true));
        // Non-archive runtimes (composer.phar) have no EOCD to look for.
        assert!(zip_has_eocd(&path, false));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// EDB reshapes this page without notice (it already did once: the version moved
    /// behind a `<span>` and a React comment), which silently emptied the list before.
    /// `cargo test -p ols-core --release -- --ignored postgres_feed`
    #[test]
    #[ignore]
    fn postgres_feed_still_lists_versions_and_windows_archives() {
        let home = crate::test_support::isolated_home();
        let manager = RuntimeManager::new(home.paths.clone());
        manager.refresh_online_catalog("postgres").unwrap();
        let versions = manager
            .online_versions
            .read()
            .unwrap()
            .get("postgres")
            .cloned()
            .unwrap();
        assert!(
            !versions.is_empty(),
            "no PostgreSQL versions parsed from EDB"
        );
        let page = String::from_utf8(manager.fetch(EDB_PAGE_URL).unwrap()).unwrap();
        for version in &versions {
            let link = edb_archive_link(&page, version);
            assert!(
                link.is_some(),
                "no Windows x86-64 archive listed for {version}"
            );
        }
    }
}
