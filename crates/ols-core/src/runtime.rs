//! Runtime Manager (§9–10, §127 — Stage 3): download → verify → extract, with the app
//! never running an unverified binary. Same "own runtime, broadcast events" shape as
//! `ProcessSupervisor` (see `process.rs`) so both work identically from `cargo test` and Tauri.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::broadcast;

use crate::catalog::{builtin_catalog, PackageManifest};
use crate::paths::AppPaths;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    /// A DevForge-managed copy is installed under `runtimes/<id>/<version>/`.
    pub installed: bool,
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
    static CACHE: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, (std::time::Instant, Option<SystemInstall>)>>> =
        std::sync::OnceLock::new();
    const TTL: std::time::Duration = std::time::Duration::from_secs(60);
    let cache = CACHE.get_or_init(Default::default);
    if let Some((at, found)) = cache.lock().unwrap().get(id) {
        if at.elapsed() < TTL {
            return found.clone();
        }
    }
    let found = probe_system_install(id);
    cache.lock().unwrap().insert(id.to_string(), (std::time::Instant::now(), found.clone()));
    found
}

fn probe_system_install(id: &str) -> Option<SystemInstall> {
    let (exe_name, version_flag) = crate::catalog::system_probe(id)?;
    let path_var = std::env::var_os("PATH")?;
    let exe_path = std::env::split_paths(&path_var).map(|dir| dir.join(exe_name)).find(|p| p.is_file())?;

    let output = std::process::Command::new(&exe_path).arg(version_flag).output().ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let text = if stdout.trim().is_empty() { stderr } else { stdout };
    let version = text.lines().next().unwrap_or("unknown version").trim().to_string();

    Some(SystemInstall { path: exe_path.display().to_string(), version })
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
    Progress { id: String, version: String, state: InstallState, downloaded: u64, total: Option<u64> },
    Installed { id: String, version: String, path: String },
    Failed { id: String, version: String, message: String },
}

pub struct RuntimeManager {
    runtime: tokio::runtime::Runtime,
    paths: AppPaths,
    http: reqwest::Client,
    events_tx: broadcast::Sender<RuntimeEvent>,
    state: Arc<Mutex<HashMap<String, InstallState>>>,
}

impl RuntimeManager {
    pub fn new(paths: AppPaths) -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("failed to start runtime manager runtime");
        let (events_tx, _rx) = broadcast::channel(256);
        Self {
            runtime,
            paths,
            http: reqwest::Client::new(),
            events_tx,
            state: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<RuntimeEvent> {
        self.events_tx.subscribe()
    }

    /// The catalog for this machine's OS/arch, each entry flagged with whether it's
    /// already installed (§127 — no unnecessary repeated downloads).
    pub fn catalog(&self) -> Vec<CatalogEntry> {
        builtin_catalog()
            .into_iter()
            .map(|m| CatalogEntry {
                id: m.id.to_string(),
                name: m.name.to_string(),
                version: m.version.to_string(),
                installed: self.version_dir(m.id, m.version).join(m.binary).exists(),
                system: detect_system_install(m.id),
            })
            .collect()
    }

    /// Display name of `id` from the built-in catalog — no filesystem or process probing.
    pub fn display_name(&self, id: &str) -> Option<String> {
        builtin_catalog().into_iter().find(|m| m.id == id).map(|m| m.name.to_string())
    }

    pub fn binary_path(&self, id: &str, version: &str) -> Option<PathBuf> {
        let manifest = builtin_catalog().into_iter().find(|m| m.id == id && m.version == version)?;
        let path = self.version_dir(id, version).join(manifest.binary);
        path.exists().then_some(path)
    }

    /// The directory holding the runtime's executable — what a runtime-aware terminal
    /// prepends to PATH (§19), and what the Environment Resolver checks before offering
    /// "this project needs PHP 8.1, install it?" (§18, §74).
    pub fn bin_dir(&self, id: &str, version: &str) -> Option<PathBuf> {
        self.binary_path(id, version).and_then(|p| p.parent().map(|d| d.to_path_buf()))
    }

    /// Every DevForge-managed version of `id` that's actually installed (has its binary
    /// on disk), newest-looking first isn't guaranteed — callers sort if order matters.
    pub fn installed_versions(&self, id: &str) -> Vec<String> {
        builtin_catalog()
            .into_iter()
            .filter(|m| m.id == id)
            .filter(|m| self.version_dir(m.id, m.version).join(m.binary).exists())
            .map(|m| m.version.to_string())
            .collect()
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
        let http = self.http.clone();
        let url = url.to_string();
        let (tx, rx) = std::sync::mpsc::channel();
        self.runtime.spawn(async move {
            let get = async {
                let response = http.get(&url).send().await.map_err(|e| e.to_string())?;
                if !response.status().is_success() {
                    return Err(format!("{url}: HTTP {}", response.status()));
                }
                response.bytes().await.map(|b| b.to_vec()).map_err(|e| e.to_string())
            };
            let result = tokio::time::timeout(std::time::Duration::from_secs(120), get)
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
        let Some(manifest) = builtin_catalog().into_iter().find(|m| m.id == id && m.version == version) else {
            let _ = self.events_tx.send(RuntimeEvent::Failed {
                id: id.to_string(),
                version: version.to_string(),
                message: "not in the catalog for this platform".into(),
            });
            return;
        };

        {
            let mut s = self.state.lock().unwrap();
            s.insert(format!("{id}@{version}"), InstallState::Downloading);
        }

        let paths = self.paths.clone();
        let http = self.http.clone();
        let events_tx = self.events_tx.clone();
        let state = self.state.clone();

        self.runtime.spawn(async move {
            if let Err(e) = install_one(manifest, paths, http, events_tx.clone(), state.clone()).await {
                tracing::warn!(id = manifest.id, version = manifest.version, error = %e, "runtime install failed");
                state.lock().unwrap().insert(format!("{}@{}", manifest.id, manifest.version), InstallState::Failed);
                let _ = events_tx.send(RuntimeEvent::Failed {
                    id: manifest.id.to_string(),
                    version: manifest.version.to_string(),
                    message: e,
                });
            }
        });
    }
}

async fn install_one(
    manifest: PackageManifest,
    paths: AppPaths,
    http: reqwest::Client,
    events_tx: broadcast::Sender<RuntimeEvent>,
    state: Arc<Mutex<HashMap<String, InstallState>>>,
) -> Result<(), String> {
    let key = format!("{}@{}", manifest.id, manifest.version);
    let cache_dir = paths.cache_dir();
    tokio::fs::create_dir_all(&cache_dir).await.map_err(|e| e.to_string())?;
    let archive_path = cache_dir.join(format!("{}-{}.download", manifest.id, manifest.version));

    // §127: a previously-downloaded, still-correct copy is reused instead of fetched
    // again. A cached file that fails to verify (corrupted, or from an older catalog
    // entry with a different hash) is treated as absent and re-downloaded below.
    let cached_digest: Option<String> = {
        let path = archive_path.clone();
        tokio::task::spawn_blocking(move || hash_file(&path)).await.map_err(|e| e.to_string())?.ok()
    };
    let already_cached = cached_digest.as_deref() == Some(manifest.sha256);

    let mut downloaded: u64;
    let total: Option<u64>;

    if already_cached {
        tracing::info!(id = manifest.id, version = manifest.version, "reusing verified cached download");
        downloaded = std::fs::metadata(&archive_path).map(|m| m.len()).unwrap_or(0);
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
        let mut request = http.get(manifest.url);
        if let Some(referer) = crate::catalog::download_referer(manifest.url) {
            request = request.header(reqwest::header::REFERER, referer);
        }
        let response = request.send().await.map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            return Err(format!("download failed: HTTP {}", response.status()));
        }
        total = response.content_length();
        downloaded = 0;
        let mut file = std::fs::File::create(&archive_path).map_err(|e| e.to_string())?;
        let mut hasher = Sha256::new();
        let mut stream = response.bytes_stream();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| e.to_string())?;
            file.write_all(&chunk).map_err(|e| e.to_string())?;
            hasher.update(&chunk);
            downloaded += chunk.len() as u64;
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
        state.lock().unwrap().insert(key.clone(), InstallState::Verifying);
        let _ = events_tx.send(RuntimeEvent::Progress {
            id: manifest.id.to_string(),
            version: manifest.version.to_string(),
            state: InstallState::Verifying,
            downloaded,
            total,
        });
        let digest = format!("{:x}", hasher.finalize());
        if digest != manifest.sha256 {
            let _ = tokio::fs::remove_file(&archive_path).await;
            return Err(format!(
                "checksum mismatch: expected {}, got {digest} — refusing to install an unverified binary",
                manifest.sha256
            ));
        }
    }

    // -- Extract into a scratch dir, then atomically rename into place (§163). --
    state.lock().unwrap().insert(key.clone(), InstallState::Extracting);
    let _ = events_tx.send(RuntimeEvent::Progress {
        id: manifest.id.to_string(),
        version: manifest.version.to_string(),
        state: InstallState::Extracting,
        downloaded,
        total,
    });

    let runtime_family_dir = paths.runtimes_dir().join(manifest.id);
    let final_dir = runtime_family_dir.join(manifest.version);
    let scratch_dir = runtime_family_dir.join(format!(".install-{}", manifest.version));

    let archive_path_for_blocking = archive_path.clone();
    let scratch_dir_for_blocking = scratch_dir.clone();
    let archive_root = manifest.archive_root.to_string();
    let single_file_name = (!manifest.url.ends_with(".zip"))
        .then(|| manifest.binary.to_string());
    tokio::task::spawn_blocking(move || match single_file_name {
        // A non-archive download (e.g. composer.phar) is the runtime itself — "extract"
        // just means copying it into place under its final name.
        Some(name) => {
            std::fs::create_dir_all(&scratch_dir_for_blocking)?;
            std::fs::copy(&archive_path_for_blocking, scratch_dir_for_blocking.join(name)).map(|_| ())
        }
        None => extract_zip(&archive_path_for_blocking, &scratch_dir_for_blocking, &archive_root),
    })
    .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;

    // The verified archive stays in cache/ on purpose (§127) — a later reinstall (or
    // installing after a bad uninstall) reuses it via the cache-hit check above instead
    // of downloading again. Only the extraction scratch dir is transient.
    if final_dir.exists() {
        let _ = tokio::fs::remove_dir_all(&final_dir).await;
    }
    tokio::fs::rename(&scratch_dir, &final_dir).await.map_err(|e| e.to_string())?;

    state.lock().unwrap().insert(key, InstallState::Installed);
    tracing::info!(id = manifest.id, version = manifest.version, path = %final_dir.display(), "runtime installed");
    let _ = events_tx.send(RuntimeEvent::Installed {
        id: manifest.id.to_string(),
        version: manifest.version.to_string(),
        path: final_dir.display().to_string(),
    });
    Ok(())
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
pub(crate) fn extract_zip(archive_path: &Path, dest_dir: &Path, archive_root: &str) -> std::io::Result<()> {
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
        if relative.split('/').any(|c| c == "..") || relative.contains(':') || relative.starts_with('/') {
            return Err(std::io::Error::other(format!("refusing unsafe path in archive: {relative}")));
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

        let entry = manager.catalog().into_iter().find(|e| e.id == id).unwrap_or_else(|| panic!("{id} in catalog"));
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

        let bin = manager.binary_path(&entry.id, &entry.version).expect("binary_path after install");
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
