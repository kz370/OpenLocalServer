//! Signed updates (§145, Stage 17). The app checks a small manifest (`latest.json`), published with a
//! minisign signature (`latest.json.minisig`), and only trusts it when the signature matches the
//! update public key. It then downloads the installer, checks its SHA-256 against the signed
//! manifest, and starts it only when the user says so. Nothing is checked or downloaded on its own:
//! every step starts from a button or a CLI command.
//!
//! A release build sets the key with `OLS_UPDATE_PUBKEY` at compile time; without a key (and
//! without one entered in Settings) an update can be found but never trusted, so it is refused.
//!
//! Manifest:
//! ```json
//! { "version": "0.4.0", "notes": "...", "pub_date": "2026-10-01",
//!   "platforms": { "windows-x86_64": { "url": "https://.../OpenLocalServer_0.4.0_x64-setup.exe", "sha256": "...", "size": 12345678 } } }
//! ```

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::app::Inner;
use crate::error::CoreError;

const KEY: &str = "updater";
const DEFAULT_ENDPOINT: &str =
    "https://github.com/openlocalserver/openlocalserver/releases/latest/download/latest.json";
const MAX_INSTALLER: usize = 600 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdaterSettings {
    pub endpoint: String,
    /// A minisign public key; blank uses the key this build was compiled with.
    pub public_key: String,
}

impl Default for UpdaterSettings {
    fn default() -> Self {
        Self {
            endpoint: DEFAULT_ENDPOINT.into(),
            public_key: String::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct Manifest {
    version: String,
    #[serde(default)]
    notes: String,
    #[serde(default)]
    pub_date: String,
    platforms: BTreeMap<String, Platform>,
}

#[derive(Debug, Clone, Deserialize)]
struct Platform {
    url: String,
    sha256: String,
    #[serde(default)]
    size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    pub available: bool,
    pub notes: String,
    pub date: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
    /// Where the verified installer is, once downloaded.
    pub downloaded: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdaterStatus {
    pub settings: UpdaterSettings,
    pub current: String,
    /// A public key is available, from this build or from Settings.
    pub key_configured: bool,
    pub last: Option<UpdateInfo>,
}

fn platform_key() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "windows-aarch64"
    } else {
        "windows-x86_64"
    }
}

fn fail(msg: impl Into<String>) -> CoreError {
    CoreError::failed_fix(
        "The update check didn't finish.",
        msg,
        "Try again later, or download the installer from the project's releases page.",
    )
}

/// `1.10.2` > `1.9.9`; missing parts count as 0 and a suffix (`-beta`) is ignored.
pub fn newer(latest: &str, current: &str) -> bool {
    let parts = |v: &str| -> Vec<u64> {
        v.trim_start_matches('v')
            .split(['-', '+'])
            .next()
            .unwrap_or("")
            .split('.')
            .map(|p| p.parse().unwrap_or(0))
            .collect()
    };
    let (a, b) = (parts(latest), parts(current));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (
            a.get(i).copied().unwrap_or(0),
            b.get(i).copied().unwrap_or(0),
        );
        if x != y {
            return x > y;
        }
    }
    false
}

/// Verifies a manifest and reads this platform's entry.
fn read_manifest(
    public_key: &str,
    data: &[u8],
    signature: &str,
    current: &str,
) -> Result<UpdateInfo, String> {
    if public_key.trim().is_empty() {
        return Err("no update public key is configured, so an update can't be trusted".into());
    }
    crate::catalogs::verify_signed(public_key, data, signature)?;
    let m: Manifest = serde_json::from_slice(data)
        .map_err(|e| format!("the update manifest isn't valid: {e}"))?;
    let p = m
        .platforms
        .get(platform_key())
        .ok_or_else(|| format!("this update has no installer for {}", platform_key()))?;
    if !p.url.starts_with("https://") {
        return Err("the installer address isn't HTTPS".into());
    }
    if p.sha256.len() != 64 || !p.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("the installer has no valid SHA-256 in the manifest".into());
    }
    Ok(UpdateInfo {
        current: current.into(),
        available: newer(&m.version, current),
        latest: m.version,
        notes: m.notes,
        date: m.pub_date,
        url: p.url.clone(),
        sha256: p.sha256.to_ascii_lowercase(),
        size: p.size,
        downloaded: None,
    })
}

impl Inner {
    pub fn updater_settings(&self) -> UpdaterSettings {
        self.settings
            .lock()
            .unwrap()
            .get(KEY)
            .and_then(|v| serde_json::from_value(v.get("settings")?.clone()).ok())
            .unwrap_or_default()
    }

    fn update_key(&self) -> String {
        let s = self.updater_settings();
        if s.public_key.trim().is_empty() {
            option_env!("OLS_UPDATE_PUBKEY").unwrap_or("").to_string()
        } else {
            s.public_key
        }
    }

    fn last_update(&self) -> Option<UpdateInfo> {
        self.settings
            .lock()
            .unwrap()
            .get(KEY)
            .and_then(|v| serde_json::from_value(v.get("last")?.clone()).ok())
    }

    fn save_updater(
        &self,
        settings: &UpdaterSettings,
        last: Option<&UpdateInfo>,
    ) -> Result<(), CoreError> {
        self.settings.lock().unwrap().set(
            KEY.to_string(),
            serde_json::json!({ "settings": settings, "last": last }),
        )
    }

    pub fn updater_status(&self) -> UpdaterStatus {
        UpdaterStatus {
            settings: self.updater_settings(),
            current: env!("CARGO_PKG_VERSION").into(),
            key_configured: !self.update_key().trim().is_empty(),
            last: self.last_update(),
        }
    }

    pub fn set_updater_settings(
        &self,
        endpoint: &str,
        public_key: &str,
    ) -> Result<UpdaterStatus, CoreError> {
        let endpoint = endpoint.trim();
        if !endpoint.starts_with("https://") {
            return Err(fail("the update address must be https://"));
        }
        if !public_key.trim().is_empty() {
            let line = public_key
                .lines()
                .map(str::trim)
                .rev()
                .find(|l| !l.is_empty() && !l.starts_with("untrusted comment"))
                .unwrap_or("");
            minisign_verify::PublicKey::from_base64(line)
                .map_err(|e| fail(format!("the public key isn't valid: {e}")))?;
        }
        self.save_updater(
            &UpdaterSettings {
                endpoint: endpoint.into(),
                public_key: public_key.trim().into(),
            },
            None,
        )?;
        Ok(self.updater_status())
    }

    /// Fetches and verifies the manifest. Returns what was found, whether or not it is newer.
    pub fn check_update(&self) -> Result<UpdateInfo, CoreError> {
        let settings = self.updater_settings();
        let data = self.runtimes.fetch(&settings.endpoint).map_err(fail)?;
        let sig = String::from_utf8(
            self.runtimes
                .fetch(&format!("{}.minisig", settings.endpoint))
                .map_err(|e| fail(format!("no signature was published ({e})")))?,
        )
        .map_err(|_| fail("the signature isn't text"))?;
        let info = read_manifest(&self.update_key(), &data, &sig, env!("CARGO_PKG_VERSION"))
            .map_err(fail)?;
        self.save_updater(&settings, Some(&info))?;
        Ok(info)
    }

    /// Downloads the installer from the last check and keeps it only if its SHA-256 matches the signed manifest.
    pub fn download_update(&self) -> Result<UpdateInfo, CoreError> {
        let mut info = self
            .last_update()
            .filter(|i| i.available)
            .ok_or_else(|| fail("check for an update first"))?;
        let bytes = self.runtimes.fetch(&info.url).map_err(fail)?;
        if bytes.len() > MAX_INSTALLER {
            return Err(fail("the installer is larger than expected"));
        }
        let got: String = Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if got != info.sha256 {
            return Err(CoreError::failed_fix(
                "The update was thrown away.",
                format!(
                    "Its SHA-256 is {got}, not the {} the signed manifest lists.",
                    info.sha256
                ),
                "Try again; if it repeats, don't install it.",
            ));
        }
        let dir = self.paths.cache_dir().join("updates");
        std::fs::create_dir_all(&dir)?;
        let name = info
            .url
            .rsplit('/')
            .next()
            .unwrap_or("update.exe")
            .split('?')
            .next()
            .unwrap_or("update.exe");
        let name: String = name
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
            .collect();
        let name = if name.is_empty() {
            "update.exe".to_string()
        } else {
            name
        };
        let path = dir.join(&name);
        std::fs::write(&path, &bytes)?;
        info.downloaded = Some(path.display().to_string());
        self.save_updater(&self.updater_settings(), Some(&info))?;
        Ok(info)
    }

    /// Starts the verified installer. It replaces the app, so the caller should expect to be closed.
    pub fn install_update(&self) -> Result<(), CoreError> {
        let info = self
            .last_update()
            .ok_or_else(|| fail("check for an update first"))?;
        let path = info
            .downloaded
            .ok_or_else(|| fail("download the update first"))?;
        let path = std::path::PathBuf::from(path);
        // Only ever run a file this module put in its own folder.
        if !path.starts_with(self.paths.cache_dir().join("updates")) || !path.is_file() {
            return Err(fail("the downloaded installer is missing"));
        }
        let mut cmd = if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("msi"))
        {
            let mut c = std::process::Command::new("msiexec");
            c.arg("/i").arg(&path);
            c
        } else {
            std::process::Command::new(&path)
        };
        cmd.spawn()
            .map_err(|e| fail(format!("the installer didn't start: {e}")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalogs::tests::sign;

    fn manifest(version: &str) -> String {
        format!(
            r#"{{"version":"{version}","notes":"Fixes","platforms":{{"{}":{{"url":"https://example.com/setup.exe","sha256":"{}","size":10}}}}}}"#,
            platform_key(),
            "ab".repeat(32)
        )
    }

    #[test]
    fn versions_compare_numerically() {
        assert!(newer("0.10.0", "0.9.9"));
        assert!(newer("1.0", "0.99.99"));
        assert!(!newer("0.3.0", "0.3.0"));
        assert!(!newer("0.2.9", "0.3.0"));
        assert!(newer("v1.2.1-beta", "1.2"));
    }

    #[test]
    fn a_signed_newer_manifest_is_offered() {
        let m = manifest("9.9.9");
        let (key, sig) = sign(m.as_bytes(), "release");
        let info = read_manifest(&key, m.as_bytes(), &sig, "0.3.0").unwrap();
        assert!(info.available);
        assert_eq!(info.latest, "9.9.9");
        assert_eq!(info.sha256, "ab".repeat(32));
    }

    #[test]
    fn unsigned_tampered_or_keyless_manifests_are_refused() {
        let m = manifest("9.9.9");
        let (key, sig) = sign(m.as_bytes(), "release");
        assert!(
            read_manifest(&key, manifest("9.9.8").as_bytes(), &sig, "0.3.0").is_err(),
            "tampered"
        );
        assert!(
            read_manifest("", m.as_bytes(), &sig, "0.3.0")
                .unwrap_err()
                .contains("public key"),
            "no key"
        );
        assert!(
            read_manifest(&key, m.as_bytes(), "not a signature", "0.3.0").is_err(),
            "no signature"
        );
        let http = m.replace("https://example.com", "http://example.com");
        let (key2, sig2) = sign(http.as_bytes(), "release");
        assert!(read_manifest(&key2, http.as_bytes(), &sig2, "0.3.0")
            .unwrap_err()
            .contains("HTTPS"));
    }
}
