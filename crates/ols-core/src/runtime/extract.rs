use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, Mutex};

use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use tokio::sync::broadcast;

use super::archive::*;
use super::types::*;
use crate::catalog::OwnedManifest;
use crate::paths::AppPaths;

pub(crate) struct ProgressThrottle {
    last_emit: std::time::Instant,
    last_bytes: u64,
    emitted_any: bool,
}

impl ProgressThrottle {
    pub(crate) fn new() -> Self {
        Self {
            last_emit: std::time::Instant::now(),
            last_bytes: 0,
            emitted_any: false,
        }
    }

    pub(crate) fn should_emit(&mut self, downloaded: u64, total: Option<u64>) -> bool {
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

pub(crate) async fn install_one(
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
    let is_zip = manifest
        .url
        .split('?')
        .next()
        .unwrap_or(&manifest.url)
        .ends_with(".zip");
    let single_file_name = (!is_zip).then(|| manifest.binary.to_string());
    let extracted = tokio::task::spawn_blocking(move || match single_file_name {
        // A non-archive download (e.g. composer.phar) is the runtime itself — "extract"
        // just means copying it into place under its final name.
        Some(name) => {
            let target = scratch_dir_for_blocking.join(name);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            } else {
                std::fs::create_dir_all(&scratch_dir_for_blocking)?;
            }
            std::fs::copy(&archive_path_for_blocking, target).map(|_| ())
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
