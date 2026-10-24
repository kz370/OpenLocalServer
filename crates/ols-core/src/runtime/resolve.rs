use crate::catalog::{builtin_catalog, OwnedManifest, PackageManifest};
use super::manager::runtime_name;
use super::scrapers::*;
use super::*;


pub(crate) fn owned_manifest(m: PackageManifest) -> OwnedManifest {
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
    let mut request = http.get(url);
    if let Some(referer) = crate::catalog::download_referer(url) {
        request = request.header(reqwest::header::REFERER, referer);
    }
    let response = request.send().await.map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("{url}: HTTP {}", response.status()));
    }
    response.text().await.map_err(|e| e.to_string())
}

pub(crate) async fn resolve_online_manifest(
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
            let sha = match fetch_text(http, &format!("{url}.txt")).await {
                Ok(sums) => parse_apache_sha256(&sums).unwrap_or_default(),
                Err(_) => String::new(),
            };
            (url, sha, "Apache24".into(), "bin/httpd.exe")
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
        "python" => {
            let mut matched = None;
            if let Ok(resp) = http
                .get("https://www.python.org/ftp/python/index-windows.json")
                .send()
                .await
            {
                if let Ok(bytes) = resp.bytes().await {
                    if let Ok(doc) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                        matched = doc
                            .get("versions")
                            .and_then(|v| v.as_array())
                            .into_iter()
                            .flatten()
                            .find_map(|item| {
                                let id_str = item.get("id").and_then(|v| v.as_str())?;
                                let sort_v = item.get("sort-version").and_then(|v| v.as_str())?;
                                if id_str.starts_with("pythonembed-") && id_str.ends_with("-64") && sort_v == version {
                                    let u = item.get("url").and_then(|v| v.as_str())?.to_string();
                                    let s = item.pointer("/hash/sha256").and_then(|v| v.as_str()).unwrap_or_default().to_string();
                                    Some((u, s))
                                } else {
                                    None
                                }
                            });
                    }
                }
            }
            let (url, sha) = match matched {
                Some((u, s)) => (u, s),
                None => {
                    let u1 = format!(
                        "https://www.python.org/ftp/python/{version}/python-{version}-embed-amd64.zip"
                    );
                    let u2 = format!(
                        "https://www.python.org/ftp/python/{version}/python-{version}-embeddable-amd64.zip"
                    );
                    let chosen = if let Ok(resp) = http.head(&u1).send().await {
                        if resp.status().is_success() {
                            u1
                        } else {
                            u2
                        }
                    } else {
                        u1
                    };
                    (chosen, String::new())
                }
            };
            (url, sha, String::new(), "python.exe")
        }
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
