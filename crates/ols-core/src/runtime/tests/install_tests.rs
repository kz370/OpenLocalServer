use std::collections::HashMap;
use std::io::{Read as _, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::broadcast;

use crate::catalog::{builtin_catalog, OwnedManifest, PackageManifest};
use crate::paths::AppPaths;
use crate::runtime::types::*;
use crate::runtime::manager::*;
use crate::runtime::install::*;
use crate::runtime::extract::*;
use crate::runtime::archive::*;
use crate::runtime::catalog::*;
use crate::runtime::scrapers::*;
use crate::runtime::resolve::*;

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

