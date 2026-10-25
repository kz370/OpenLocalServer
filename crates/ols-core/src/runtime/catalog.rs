use std::collections::HashMap;

use super::manager::*;
use super::scrapers::*;
use super::types::*;
use crate::catalog::builtin_catalog;

impl RuntimeManager {
    pub(crate) fn preferred_versions(&self) -> HashMap<String, String> {
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
            let list = all_versions.entry(m.id.to_string()).or_default();
            let v = m.version.to_string();
            if !list.contains(&v) {
                list.push(v);
            }
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
            "python",
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
            "python" => {
                let mut versions = Vec::new();
                // 1. Fetch release list from python.org directory (contains all released 3.x versions)
                if let Ok(bytes) = self.fetch("https://www.python.org/ftp/python/") {
                    if let Ok(page) = String::from_utf8(bytes) {
                        for v in extract_versions(&page, "href=\"", "/\"") {
                            if v.starts_with("3.") && is_valid_released_python(&v) {
                                versions.push(v);
                            }
                        }
                    }
                }
                // 2. Also incorporate index-windows.json if available
                if let Ok(bytes) =
                    self.fetch("https://www.python.org/ftp/python/index-windows.json")
                {
                    if let Ok(doc) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                        for item in doc
                            .get("versions")
                            .and_then(|v| v.as_array())
                            .into_iter()
                            .flatten()
                        {
                            let Some(id_str) = item.get("id").and_then(|v| v.as_str()) else {
                                continue;
                            };
                            if id_str.starts_with("pythonembed-") && id_str.ends_with("-64") {
                                if let Some(v) = item.get("sort-version").and_then(|v| v.as_str()) {
                                    if !v.contains('a')
                                        && !v.contains('b')
                                        && !v.contains("rc")
                                        && is_valid_released_python(v)
                                    {
                                        versions.push(v.to_string());
                                    }
                                }
                            }
                        }
                    }
                }
                // 3. Fallback to built-in catalog if network returned no entries
                if versions.is_empty() {
                    for m in builtin_catalog() {
                        if m.id == "python" {
                            versions.push(m.version.to_string());
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
}
