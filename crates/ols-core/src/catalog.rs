//! Runtime package catalog (§20 — Stage 3). The built-in entries are hand-curated; enabled
//! plugins and verified signed catalogs (Stage 16) add more through `set_extra`, without
//! changing `PackageManifest`'s shape.
//!
//! Every entry's `sha256` was pulled from the vendor's own published checksum file at the
//! time it was added (e.g. `https://nodejs.org/dist/vX.Y.Z/SHASUMS256.txt`), never computed
//! locally — that's the whole point of §21 package integrity.

use std::collections::HashSet;
use std::sync::{Mutex, RwLock};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy)]
pub struct PackageManifest {
    /// Stable id for this runtime family, e.g. "node".
    pub id: &'static str,
    pub name: &'static str,
    pub version: &'static str,
    pub platform: &'static str,
    pub architecture: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    /// The single top-level directory the archive extracts into (e.g.
    /// "node-v24.21.0-win-x64") — stripped off during install so the final layout is
    /// `runtimes/<id>/<version>/...` regardless of how the vendor packaged it.
    pub archive_root: &'static str,
    /// Path to the main executable, relative to the installed version directory.
    pub binary: &'static str,
}

const CATALOG: &[PackageManifest] = &[
    PackageManifest {
        id: "node",
        name: "Node.js",
        version: "24.21.0",
        platform: "windows",
        architecture: "x64",
        url: "https://nodejs.org/dist/v24.21.0/node-v24.21.0-win-x64.zip",
        sha256: "158f7685b44de51f6c0df1d153526cbcd3e1bc739a8dfc607721cef75de9e541",
        archive_root: "node-v24.21.0-win-x64",
        binary: "node.exe",
    },
    PackageManifest {
        id: "node",
        name: "Node.js",
        version: "22.23.0",
        platform: "windows",
        architecture: "x64",
        url: "https://nodejs.org/download/release/v22.23.0/node-v22.23.0-win-x64.zip",
        // From Node.js' SHASUMS256.txt for this release.
        sha256: "425a5bd68cc95e8eb16bcccd0a75081b48983fc6a26f67126bd4d6c7198231e8",
        archive_root: "node-v22.23.0-win-x64",
        binary: "node.exe",
    },
    PackageManifest {
        id: "php",
        name: "PHP",
        version: "8.4.26",
        platform: "windows",
        architecture: "x64",
        url: "https://windows.php.net/downloads/releases/php-8.4.26-nts-Win32-vs17-x64.zip",
        sha256: "da68394f9193b7f6b89d0c76861a4034ae10efee7fd55a7255d8118c2acf70d7",
        // PHP's Windows zips are flat (no wrapping top-level directory), unlike Node's —
        // an empty archive_root means "strip nothing", which extract_zip handles natively.
        archive_root: "",
        binary: "php.exe",
    },
    PackageManifest {
        id: "php",
        name: "PHP",
        version: "8.3.35",
        platform: "windows",
        architecture: "x64",
        url: "https://downloads.php.net/~windows/releases/php-8.3.35-nts-Win32-vs16-x64.zip",
        // Published on PHP's Windows download page for this NTS x64 build.
        sha256: "25a8e2ac9ff30f1d768d1447c09a600617fa6e6082729f6e95f008b59c91fe45",
        archive_root: "",
        binary: "php.exe",
    },
    PackageManifest {
        id: "mailpit",
        name: "Mailpit",
        version: "1.31.3",
        platform: "windows",
        architecture: "x64",
        url: "https://github.com/axllent/mailpit/releases/download/v1.31.3/mailpit-windows-amd64.zip",
        // Mailpit's GitHub release publishes no checksum file for this version. Downloaded
        // directly over HTTPS from the release URL above and hashed here ourselves — the
        // same trust-on-first-use pinning any tool must fall back to when a vendor
        // publishes no signature (§21's "where available: digital signatures").
        sha256: "863e9502d4e0f14a78c0f91c5091797b1c7b7b7e3fc7e5eab62e5770ce44b76e",
        archive_root: "",
        binary: "mailpit.exe",
    },
    PackageManifest {
        id: "nginx",
        name: "Nginx",
        version: "1.28.3",
        platform: "windows",
        architecture: "x64",
        url: "https://nginx.org/download/nginx-1.28.3.zip",
        // nginx.org publishes no checksum sidecar either — same self-pinned approach.
        sha256: "aad7bf75d669ece7671688bfdf35f1093d6a30d2e62469405f4d55d8d82d5fd3",
        archive_root: "nginx-1.28.3",
        binary: "nginx.exe",
    },
    PackageManifest {
        id: "nginx",
        name: "Nginx",
        version: "1.30.5",
        platform: "windows",
        architecture: "x64",
        url: "https://nginx.org/download/nginx-1.30.5.zip",
        // SHA-256 pinned from the exact official Windows archive.
        sha256: "e5afe28b6a50bec92c478bfe1a4d3758206b80fb77159277bc5c4e88955c2a35",
        archive_root: "nginx-1.30.5",
        binary: "nginx.exe",
    },
    PackageManifest {
        id: "php",
        name: "PHP",
        version: "8.1.34",
        platform: "windows",
        architecture: "x64",
        url: "https://windows.php.net/downloads/releases/php-8.1.34-nts-Win32-vs16-x64.zip",
        // From windows.php.net's own releases.json.
        sha256: "9cfe246cb144076c16f5913a3ef88a474c3dd7e60f0f0c8bb95faf68674016cc",
        archive_root: "",
        binary: "php.exe",
    },
    PackageManifest {
        id: "caddy",
        name: "Caddy",
        version: "2.11.4",
        platform: "windows",
        architecture: "x64",
        url: "https://github.com/caddyserver/caddy/releases/download/v2.11.4/caddy_2.11.4_windows_amd64.zip",
        // Caddy publishes SHA-512 sums; this SHA-256 was computed from a direct HTTPS
        // download of that exact URL (same self-pinning as Mailpit).
        sha256: "1708333f79e274c7697285afe6d592ab39314e0b131e9ec6bea08ad27df62ebf",
        archive_root: "",
        binary: "caddy.exe",
    },
    PackageManifest {
        id: "apache",
        name: "Apache HTTP Server",
        version: "2.4.68",
        platform: "windows",
        architecture: "x64",
        url: "https://www.apachelounge.com/download/VS18/binaries/httpd-2.4.68-260920-Win64-VS18.zip",
        // Apache Lounge publishes PGP signatures only; SHA-256 self-pinned from a direct
        // HTTPS download. (The host serves the zip only when a Referer is sent — see
        // `download_referer`.)
        sha256: "f6dcf17d08aa32721ae418cd818c157e4c521c9e889b758646fb64287f1d56e3",
        archive_root: "Apache24",
        binary: "bin/httpd.exe",
    },
    PackageManifest {
        id: "mariadb",
        name: "MariaDB",
        version: "11.4.9",
        platform: "windows",
        architecture: "x64",
        url: "https://archive.mariadb.org/mariadb-11.4.9/winx64-packages/mariadb-11.4.9-winx64.zip",
        // From archive.mariadb.org's sha256sums.txt for this release.
        sha256: "802f9f40a9dca774a3ba62f39c21093942954f178d6d7d458dc51453929bcdda",
        archive_root: "mariadb-11.4.9-winx64",
        binary: "bin/mariadbd.exe",
    },
    PackageManifest {
        id: "mariadb",
        name: "MariaDB",
        version: "11.8.5",
        platform: "windows",
        architecture: "x64",
        url: "https://archive.mariadb.org/mariadb-11.8.5/winx64-packages/mariadb-11.8.5-winx64.zip",
        // From archive.mariadb.org's sha256sums.txt for this release.
        sha256: "75332dc1f437d9ecee253c2d751d02a69628b109ddd031006f8a8d9ba59dbe0d",
        archive_root: "mariadb-11.8.5-winx64",
        binary: "bin/mariadbd.exe",
    },
    PackageManifest {
        id: "mongodb",
        name: "MongoDB",
        version: "8.3.11",
        platform: "windows",
        architecture: "x64",
        url: "https://fastdl.mongodb.org/windows/mongodb-windows-x86_64-8.3.11.zip",
        // From MongoDB's downloads.mongodb.org/current.json feed.
        sha256: "55574b06b41848207213a5e69575dd3487abf3cf07ecfe01b3aef1bab0084241",
        archive_root: "mongodb-win32-x86_64-windows-8.3.11",
        binary: "bin/mongod.exe",
    },
    PackageManifest {
        id: "postgres",
        name: "PostgreSQL",
        version: "17.6",
        platform: "windows",
        architecture: "x64",
        url: "https://get.enterprisedb.com/postgresql/postgresql-17.6-1-windows-x64-binaries.zip",
        // EnterpriseDB's binaries zip (the official Windows distribution) publishes no checksum
        // file; SHA-256 self-pinned from a direct HTTPS download of this exact URL.
        sha256: "d378882abd001a186735acd6f6ba716bca6ccd192e800412d4fd15ed25376b3e",
        archive_root: "pgsql",
        binary: "bin/postgres.exe",
    },
    PackageManifest {
        id: "redis",
        name: "Redis",
        version: "8.10.2",
        platform: "windows",
        architecture: "x64",
        url: "https://github.com/redis-windows/redis-windows/releases/download/8.10.2/Redis-8.10.2-Windows-x64-cygwin.zip",
        // Redis has no official Windows build; this is the community redis-windows build of the
        // upstream source. It publishes no checksum file, so the SHA-256 is self-pinned.
        sha256: "6de5cc7f5adbf97b5928b13766383d4ad424626ef3d8b313ffff12d820ec6fc1",
        archive_root: "Redis-8.10.2-Windows-x64-cygwin",
        binary: "redis-server.exe",
    },
    PackageManifest {
        id: "memcached",
        name: "Memcached",
        version: "1.6.8",
        platform: "windows",
        architecture: "x64",
        url: "https://github.com/jefyt/memcached-windows/releases/download/1.6.8_mingw_libressl/memcached-1.6.8-win64-mingw.zip",
        // Memcached has no official Windows build; this is the community native port of the
        // upstream source. The publisher ships a hashes.txt next to the asset, and this
        // SHA-256 matches its `SHA2-256(memcached-1.6.8-win64-mingw.zip)` entry.
        sha256: "48ec62cef718f0d73698414b783c0e4a69821013553ca00afe0eed324eb5994b",
        archive_root: "memcached-1.6.8-win64-mingw",
        binary: "bin/memcached.exe",
    },
    PackageManifest {
        id: "sqlite",
        name: "SQLite",
        version: "3.53.4",
        platform: "windows",
        architecture: "x64",
        url: "https://www.sqlite.org/2026/sqlite-tools-win-x64-3530400.zip",
        // sqlite.org lists SHA3-256; this SHA-256 is self-pinned from a direct download.
        sha256: "f46ee2475de4cbe287e6e5f7d43c838796b14e7379cd216bdbb28d391429f9fc",
        archive_root: "",
        binary: "sqlite3.exe",
    },
    PackageManifest {
        id: "composer",
        name: "Composer",
        version: "2.10.3",
        platform: "windows",
        architecture: "x64",
        url: "https://getcomposer.org/download/2.10.3/composer.phar",
        // From getcomposer.org's own composer.phar.sha256sum for this release.
        sha256: "7a2d379d5b8ffdaa028580ef26494c36d2feef4b178d3dd1473a4dbc5e17c8d6",
        archive_root: "",
        binary: "composer.phar",
    },
    PackageManifest {
        id: "git",
        name: "Git (portable)",
        version: "2.55.0.5",
        platform: "windows",
        architecture: "x64",
        url: "https://github.com/git-for-windows/git/releases/download/v2.55.0.windows.5/MinGit-2.55.0.5-64-bit.zip",
        // From the SHA-256 table in the Git for Windows v2.55.0.windows.5 release notes.
        sha256: "56d7b226b7693196cfc71fef26568f536c4a021ab6c37ff2db4287bed908e96e",
        archive_root: "",
        binary: "cmd/git.exe",
    },
    PackageManifest {
        id: "k6",
        name: "k6 (load testing)",
        version: "2.3.0",
        platform: "windows",
        architecture: "x64",
        url: "https://github.com/grafana/k6/releases/download/v2.3.0/k6-v2.3.0-windows-amd64.zip",
        // From the k6-v2.3.0-checksums.txt published with the release.
        sha256: "112276d495e5741c968e2bc09ea6196099c1275bd6db9ee0875d173c7148ce43",
        archive_root: "k6-v2.3.0-windows-amd64",
        binary: "k6.exe",
    },
];

fn current_platform() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

fn current_arch() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "x64"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "unknown"
    }
}

/// A catalog entry as plugins and remote catalogs describe it (owned strings).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OwnedManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default = "default_platform")]
    pub platform: String,
    #[serde(default = "default_arch")]
    pub architecture: String,
    pub url: String,
    pub sha256: String,
    #[serde(default)]
    pub archive_root: String,
    pub binary: String,
    /// How to look for an existing install: the executable and its version flag.
    #[serde(default)]
    pub probe: Option<Probe>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Probe {
    pub exe: String,
    pub arg: String,
}

fn default_platform() -> String {
    "windows".into()
}
fn default_arch() -> String {
    "x64".into()
}

impl OwnedManifest {
    /// What is wrong with this entry, for a plugin or catalog to be refused. A download needs
    /// HTTPS and a full SHA-256 (§21), and nothing may reach outside the install folder.
    pub fn check(&self) -> Result<(), String> {
        let id_ok = |s: &str| {
            !s.is_empty()
                && s.len() <= 40
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        };
        if !id_ok(&self.id) {
            return Err(format!(
                "runtime id '{}' may only hold letters, digits, '-' and '_'",
                self.id
            ));
        }
        if self.version.is_empty()
            || self.version.len() > 40
            || !self
                .version
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'))
        {
            return Err(format!(
                "{}: the version '{}' is not usable as a folder name",
                self.id, self.version
            ));
        }
        if !self.url.starts_with("https://") {
            return Err(format!(
                "{} {}: downloads must use HTTPS",
                self.id, self.version
            ));
        }
        if self.sha256.len() != 64 || !self.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(format!(
                "{} {}: needs a 64-character SHA-256",
                self.id, self.version
            ));
        }
        for (what, p) in [
            ("binary", self.binary.as_str()),
            ("archive_root", self.archive_root.as_str()),
        ] {
            if p.contains("..") || p.starts_with('/') || p.starts_with('\\') || p.contains(':') {
                return Err(format!(
                    "{} {}: the {what} path '{p}' must stay inside the install folder",
                    self.id, self.version
                ));
            }
        }
        if self.binary.is_empty() {
            return Err(format!("{} {}: no binary named", self.id, self.version));
        }
        Ok(())
    }
}

static EXTRA: RwLock<Vec<PackageManifest>> = RwLock::new(Vec::new());
static EXTRA_PROBES: RwLock<Vec<(&'static str, &'static str, &'static str)>> =
    RwLock::new(Vec::new());
static INTERNED: Mutex<Option<HashSet<&'static str>>> = Mutex::new(None);

/// One leaked copy per distinct string, so replacing the extras on every plugin change
/// doesn't grow memory (the catalog hands out `&'static str`).
fn intern(s: &str) -> &'static str {
    let mut guard = INTERNED.lock().unwrap();
    let set = guard.get_or_insert_with(HashSet::new);
    if let Some(found) = set.get(s) {
        return found;
    }
    let leaked: &'static str = Box::leak(s.to_string().into_boxed_str());
    set.insert(leaked);
    leaked
}

/// Replaces the entries contributed by plugins and catalogs. Built-in entries always win
/// for the same id and version, and invalid entries are skipped.
pub fn set_extra(entries: &[OwnedManifest]) {
    let mut out: Vec<PackageManifest> = Vec::new();
    let mut probes = Vec::new();
    for e in entries {
        if e.check().is_err() {
            continue;
        }
        if CATALOG
            .iter()
            .any(|m| m.id == e.id && m.version == e.version)
            || out.iter().any(|m| m.id == e.id && m.version == e.version)
        {
            continue;
        }
        out.push(PackageManifest {
            id: intern(&e.id),
            name: intern(&e.name),
            version: intern(&e.version),
            platform: intern(&e.platform),
            architecture: intern(&e.architecture),
            url: intern(&e.url),
            sha256: intern(&e.sha256.to_ascii_lowercase()),
            archive_root: intern(&e.archive_root),
            binary: intern(&e.binary),
        });
        if let Some(p) = &e.probe {
            if !probes.iter().any(|(id, _, _)| *id == e.id) {
                probes.push((intern(&e.id), intern(&p.exe), intern(&p.arg)));
            }
        }
    }
    *EXTRA.write().unwrap() = out;
    *EXTRA_PROBES.write().unwrap() = probes;
}

/// Entries matching the machine OpenLocalServer is actually running on.
pub fn builtin_catalog() -> Vec<PackageManifest> {
    let extra = EXTRA.read().unwrap();
    CATALOG
        .iter()
        .chain(extra.iter())
        .copied()
        .filter(|m| m.platform == current_platform() && m.architecture == current_arch())
        .collect()
}

/// The executable name and version flag to probe for an existing, unmanaged install of
/// this runtime family already on the system (§126 — detect before offering to download).
pub fn system_probe(id: &str) -> Option<(&'static str, &'static str)> {
    match id {
        "node" => Some(("node.exe", "--version")),
        "php" => Some(("php.exe", "--version")),
        "python" => Some(("python.exe", "--version")),
        "nginx" => Some(("nginx.exe", "-v")),
        "caddy" => Some(("caddy.exe", "version")),
        "apache" => Some(("httpd.exe", "-v")),
        "mariadb" => Some(("mariadbd.exe", "--version")),
        "mongodb" => Some(("mongod.exe", "--version")),
        "postgres" => Some(("postgres.exe", "--version")),
        "redis" => Some(("redis-server.exe", "--version")),
        "memcached" => Some(("memcached.exe", "--version")),
        "sqlite" => Some(("sqlite3.exe", "--version")),
        "k6" => Some(("k6.exe", "version")),
        _ => EXTRA_PROBES
            .read()
            .unwrap()
            .iter()
            .find(|(pid, _, _)| *pid == id)
            .map(|(_, exe, arg)| (*exe, *arg)),
    }
}

/// Hosts that only serve a download when the request carries a Referer from their own
/// site (Apache Lounge). Returns the Referer to send, if any.
pub fn download_referer(url: &str) -> Option<&'static str> {
    url.contains("apachelounge.com")
        .then_some("https://www.apachelounge.com/download/")
}
