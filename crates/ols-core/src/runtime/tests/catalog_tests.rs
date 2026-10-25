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
use crate::runtime::archive::*;
use crate::runtime::catalog::*;
use crate::runtime::extract::*;
use crate::runtime::install::*;
use crate::runtime::manager::*;
use crate::runtime::resolve::*;
use crate::runtime::scrapers::*;
use crate::runtime::types::*;

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

/// The MariaDB rest-api is keyed by branch, and asking it for a full version returns
/// 200 with a different document — so this regressed into "13.1.1 is unavailable"
/// while every 13.x release was perfectly installable.
/// `cargo test -p ols-core --release -- --ignored mariadb_resolves`
#[test]
#[ignore]
fn mariadb_resolves_a_real_release_to_a_verified_windows_zip() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let http = reqwest::Client::builder()
        .user_agent("OLS")
        .build()
        .unwrap();
    let version = "13.1.1";
    let manifest = rt
        .block_on(resolve_online_manifest(&http, "mariadb", version))
        .unwrap_or_else(|e| panic!("MariaDB {version} must resolve: {e}"));
    assert!(manifest.url.ends_with(".zip"), "url: {}", manifest.url);
    assert!(
        manifest.url.starts_with("https://"),
        "a download must be HTTPS: {}",
        manifest.url
    );
    assert_eq!(
        manifest.sha256.len(),
        64,
        "MariaDB publishes a SHA-256 and it must be used to verify the download"
    );
    assert!(!manifest.sha256.is_empty());
    assert_eq!(manifest.binary, "bin/mariadbd.exe");
}

#[test]
fn linked_download_url_extracts_full_zip_url_and_skips_win32() {
    let html = r#"
            <pre><img src="/images/hot.gif" alt="[Apache VS18 Win64 Binary]"> <a 
            href="/download/VS18/binaries/httpd-2.4.69-261002-Win64-VS18.zip">httpd-2.4.69-261002-Win64-VS18.zip</a>               
                    2 Oct '26  14.156k
              <a href="/download/VS18/binaries/httpd-2.4.69-261002-Win64-VS18.zip.asc"><b>PGP</b></a> Signature, SHA1-SHA512 <a 
            href="/download/VS18/binaries/httpd-2.4.69-261002-Win64-VS18.zip.txt" target="_blank" ><b>Checksums</b></a></pre>
            <img src="/images/hot.gif" alt="[Apache vs18 Win32 Binary]"> <a 
            href="/download/VS18/binaries/httpd-2.4.69-261002-win32-vs18.zip">httpd-2.4.69-261002-win32-vs18.zip</a>
        "#;
    let url = linked_download_url(html, "httpd-", "2.4.69");
    assert_eq!(
            url.as_deref(),
            Some("https://www.apachelounge.com/download/VS18/binaries/httpd-2.4.69-261002-Win64-VS18.zip")
        );
}

#[test]
fn parse_apache_sha256_extracts_checksum() {
    let text = r#"
Checksums created with GPGHash by SmartJuwel

SHA1-Checksum for: httpd-2.4.69-261002-Win64-VS18.zip:
D7B557581787727CA222C6A5FF0E9E6FA6029893

SHA256-Checksum for: httpd-2.4.69-261002-Win64-VS18.zip:
CA272E0B75ABAD78B1E4029A0753B20FF2232D2D857E8EEBEEA5D469C5776977

SHA512-Checksum for: httpd-2.4.69-261002-Win64-VS18.zip:
B5198796E28FD985225DF36CB3E65226A423BB48DF5B111C91439C59D558FD9DB1868F838C927E79B7A30B7B0C1C6FA0E4BEA767A3908A5187745158E22E9269
        "#;
    assert_eq!(
        parse_apache_sha256(text).as_deref(),
        Some("ca272e0b75abad78b1e4029a0753b20ff2232d2d857e8eebeea5d469c5776977")
    );
}

#[test]
#[ignore]
fn apache_resolves_a_real_release_to_a_verified_windows_zip() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let http = reqwest::Client::builder()
        .user_agent("OLS")
        .build()
        .unwrap();
    let version = "2.4.69";
    let manifest = rt
        .block_on(resolve_online_manifest(&http, "apache", version))
        .unwrap_or_else(|e| panic!("Apache {version} must resolve: {e}"));
    assert!(manifest.url.ends_with(".zip"), "url: {}", manifest.url);
    assert!(
        manifest.url.starts_with("https://"),
        "a download must be HTTPS: {}",
        manifest.url
    );
    assert_eq!(
        manifest.sha256.len(),
        64,
        "Apache Lounge publishes a SHA-256 and it must be used to verify the download"
    );
    assert_eq!(
        manifest.sha256,
        "ca272e0b75abad78b1e4029a0753b20ff2232d2d857e8eebeea5d469c5776977"
    );
    assert_eq!(manifest.binary, "bin/httpd.exe");
}
