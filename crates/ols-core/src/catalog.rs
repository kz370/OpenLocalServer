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
        _ => None,
    }
}
