//! Runtime package catalog (§20 — Stage 3). Hand-curated for now; a signed remote catalog
//! (Stage 17) will replace `builtin_catalog` without changing `PackageManifest`'s shape.
//!
//! Every entry's `sha256` was pulled from the vendor's own published checksum file at the
//! time it was added (e.g. `https://nodejs.org/dist/vX.Y.Z/SHASUMS256.txt`), never computed
//! locally — that's the whole point of §21 package integrity.

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
        id: "mailpit",
        name: "Mailpit",
        version: "1.31.2",
        platform: "windows",
        architecture: "x64",
        url: "https://github.com/axllent/mailpit/releases/download/v1.31.2/mailpit-windows-amd64.zip",
        // Mailpit's GitHub release publishes no checksum file for this version. Downloaded
        // directly over HTTPS from the release URL above and hashed here ourselves — the
        // same trust-on-first-use pinning any tool must fall back to when a vendor
        // publishes no signature (§21's "where available: digital signatures").
        sha256: "42c20e5c3254125ea7489847811f10d70e39de573fe41d03a61412c87913e995",
        archive_root: "",
        binary: "mailpit.exe",
    },
    PackageManifest {
        id: "mysql",
        name: "MySQL",
        version: "26.7.0",
        platform: "windows",
        architecture: "x64",
        url: "https://cdn.mysql.com//Downloads/MySQL-26.7/mysql-26.7.0-winx64.zip",
        // MySQL's download page shows no static checksum file either (MD5/SHA256 are
        // rendered client-side via JS on the download page) — same self-pinned approach
        // as Mailpit above, hashed from a direct HTTPS download of this exact URL.
        sha256: "e8d5b08f0d430555497679fa713a9649953ea067f952708a74cdcdf34f8e4b7d",
        archive_root: "mysql-26.7.0-winx64",
        binary: "bin/mysqld.exe",
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

/// Entries matching the machine DevForge is actually running on.
pub fn builtin_catalog() -> Vec<PackageManifest> {
    CATALOG
        .iter()
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
        "mysql" => Some(("mysql.exe", "--version")),
        "nginx" => Some(("nginx.exe", "-v")),
        "caddy" => Some(("caddy.exe", "version")),
        "apache" => Some(("httpd.exe", "-v")),
        "mariadb" => Some(("mariadbd.exe", "--version")),
        "mongodb" => Some(("mongod.exe", "--version")),
        "postgres" => Some(("postgres.exe", "--version")),
        "redis" => Some(("redis-server.exe", "--version")),
        "sqlite" => Some(("sqlite3.exe", "--version")),
        _ => None,
    }
}

/// Hosts that only serve a download when the request carries a Referer from their own
/// site (Apache Lounge). Returns the Referer to send, if any.
pub fn download_referer(url: &str) -> Option<&'static str> {
    url.contains("apachelounge.com").then_some("https://www.apachelounge.com/download/")
}
