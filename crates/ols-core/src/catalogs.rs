//! Signed remote catalogs (§21, §87–88, Stage 16). A catalog is one JSON file listing runtimes,
//! plugins and Quick App sources, published with a minisign signature (`<url>.minisig`).
//!
//! - The user adds a source together with its **public key**; that key is what says who may
//!   publish to it. A file without a valid signature from that key is never read.
//! - The verified file and signature are cached, and verified **again** each time they're loaded,
//!   so a file edited on disk afterwards is ignored.
//! - A catalog runtime carries its SHA-256 like any other entry (§21). Catalog plugins are
//!   downloaded, checked against their listed SHA-256 and installed switched off, like a local one.
//! - Quick App sources listed in a catalog are only pointers: importing one goes through the
//!   normal untrusted-until-approved flow (§88).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::app::Inner;
use crate::catalog::OwnedManifest;
use crate::error::CoreError;
use crate::plugin::PluginInfo;

const KEY: &str = "catalog_sources";
const MAX_CATALOG_BYTES: usize = 4 * 1024 * 1024;

static ERRORS: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogSource {
    pub id: String,
    pub name: String,
    /// An `https://` address, or a path to a file (a company mirror on a share).
    pub url: String,
    /// The publisher's minisign public key (base64 line).
    pub public_key: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CatalogDoc {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub runtimes: Vec<OwnedManifest>,
    #[serde(default)]
    pub plugins: Vec<CatalogPlugin>,
    #[serde(default)]
    pub quick_app_sources: Vec<QuickAppSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogPlugin {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    pub url: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuickAppSource {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogView {
    pub source: CatalogSource,
    /// The cached copy still matches its signature.
    pub verified: bool,
    /// The signed comment (publisher's note, usually a timestamp).
    pub note: Option<String>,
    pub refreshed_ms: Option<u64>,
    pub doc: Option<CatalogDoc>,
    pub error: Option<String>,
}

fn fail(msg: impl Into<String>) -> CoreError {
    CoreError::failed_fix("That catalog couldn't be used.", msg, "Check the address and the publisher's public key.")
}

/// Checks `signature` (a `.minisig` file's text) over `data` against `public_key`, returning the
/// signed trusted comment. The key may be the bare base64 line or a whole `.pub` file.
pub fn verify_signed(public_key: &str, data: &[u8], signature: &str) -> Result<String, String> {
    let key = normalize_key(public_key)?;
    let pk = minisign_verify::PublicKey::from_base64(&key).map_err(|e| format!("the public key isn't valid: {e}"))?;
    let sig = minisign_verify::Signature::decode(signature).map_err(|e| format!("the signature file isn't valid: {e}"))?;
    pk.verify(data, &sig, false).map_err(|e| format!("the signature doesn't match: {e}"))?;
    Ok(sig.trusted_comment().to_string())
}

fn normalize_key(text: &str) -> Result<String, String> {
    text.lines().map(str::trim).rev().find(|l| !l.is_empty() && !l.starts_with("untrusted comment")).map(str::to_string).ok_or_else(|| "the public key is empty".to_string())
}

fn cache_dir(inner: &Inner) -> PathBuf {
    inner.paths.data_dir().join("catalogs")
}

fn set_error(id: &str, message: Option<String>) {
    let mut guard = ERRORS.lock().unwrap();
    let map = guard.get_or_insert_with(HashMap::new);
    match message {
        Some(m) => {
            map.insert(id.to_string(), m);
        }
        None => {
            map.remove(id);
        }
    }
}

fn get_error(id: &str) -> Option<String> {
    ERRORS.lock().unwrap().as_ref().and_then(|m| m.get(id).cloned())
}

impl Inner {
    fn catalog_sources(&self) -> Vec<CatalogSource> {
        self.settings.lock().unwrap().get(KEY).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default()
    }

    fn save_catalog_sources(&self, sources: &[CatalogSource]) -> Result<(), CoreError> {
        self.settings.lock().unwrap().set(KEY.to_string(), serde_json::to_value(sources)?)
    }

    pub fn add_catalog_source(&self, name: &str, url: &str, public_key: &str) -> Result<(), CoreError> {
        let url = url.trim();
        if !(url.starts_with("https://") || std::path::Path::new(url).is_file()) {
            return Err(fail("the address must be https:// or a file on this computer"));
        }
        let public_key = normalize_key(public_key).map_err(fail)?;
        minisign_verify::PublicKey::from_base64(&public_key).map_err(|e| fail(format!("the public key isn't valid: {e}")))?;
        let id = crate::domain::slugify(name);
        if id.is_empty() {
            return Err(fail("give the catalog a name"));
        }
        let mut sources = self.catalog_sources();
        if sources.iter().any(|s| s.id == id) {
            return Err(fail(format!("a catalog named '{name}' already exists")));
        }
        sources.push(CatalogSource { id, name: name.trim().to_string(), url: url.to_string(), public_key });
        self.save_catalog_sources(&sources)
    }

    pub fn remove_catalog_source(&self, id: &str) -> Result<(), CoreError> {
        let mut sources = self.catalog_sources();
        sources.retain(|s| s.id != id);
        self.save_catalog_sources(&sources)?;
        let _ = std::fs::remove_file(cache_dir(self).join(format!("{id}.json")));
        let _ = std::fs::remove_file(cache_dir(self).join(format!("{id}.json.minisig")));
        set_error(id, None);
        self.apply_plugins();
        Ok(())
    }

    /// The cached catalog for a source, only if it still verifies.
    fn load_catalog(&self, s: &CatalogSource) -> Result<(CatalogDoc, String, u64), String> {
        let dir = cache_dir(self);
        let json_path = dir.join(format!("{}.json", s.id));
        let data = std::fs::read(&json_path).map_err(|_| "not downloaded yet".to_string())?;
        let sig = std::fs::read_to_string(dir.join(format!("{}.json.minisig", s.id))).map_err(|_| "the saved signature is missing".to_string())?;
        let note = verify_signed(&s.public_key, &data, &sig).map_err(|e| format!("the saved copy no longer matches its signature ({e})"))?;
        let doc: CatalogDoc = serde_json::from_slice(&data).map_err(|e| format!("the catalog isn't valid: {e}"))?;
        let modified = std::fs::metadata(&json_path).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64).unwrap_or(0);
        Ok((doc, note, modified))
    }

    pub fn catalog_views(&self) -> Vec<CatalogView> {
        self.catalog_sources()
            .into_iter()
            .map(|source| match self.load_catalog(&source) {
                Ok((doc, note, at)) => CatalogView { error: get_error(&source.id), source, verified: true, note: Some(note), refreshed_ms: Some(at), doc: Some(doc) },
                Err(e) => CatalogView { error: Some(get_error(&source.id).unwrap_or(e)), source, verified: false, note: None, refreshed_ms: None, doc: None },
            })
            .collect()
    }

    /// Runtimes listed by verified catalogs (nothing from a catalog that fails its check).
    pub(crate) fn catalog_runtimes(&self) -> Vec<OwnedManifest> {
        self.catalog_sources().iter().filter_map(|s| self.load_catalog(s).ok()).flat_map(|(doc, _, _)| doc.runtimes).collect()
    }

    fn fetch_bytes(&self, url: &str) -> Result<Vec<u8>, String> {
        let data = if url.starts_with("https://") {
            self.runtimes.fetch(url)?
        } else {
            std::fs::read(url).map_err(|e| format!("{url}: {e}"))?
        };
        if data.len() > MAX_CATALOG_BYTES {
            return Err("the catalog is larger than 4 MB".into());
        }
        Ok(data)
    }

    /// Downloads a catalog and its signature, verifies them, and only then replaces the cached copy.
    fn refresh_one(&self, s: &CatalogSource) -> Result<(), String> {
        let data = self.fetch_bytes(&s.url)?;
        let sig_url = format!("{}.minisig", s.url);
        let sig = String::from_utf8(self.fetch_bytes(&sig_url).map_err(|e| format!("no signature found ({e})"))?).map_err(|_| "the signature isn't text".to_string())?;
        verify_signed(&s.public_key, &data, &sig)?;
        let doc: CatalogDoc = serde_json::from_slice(&data).map_err(|e| format!("the catalog isn't valid JSON: {e}"))?;
        for r in &doc.runtimes {
            r.check()?;
        }
        let dir = cache_dir(self);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        std::fs::write(dir.join(format!("{}.json", s.id)), &data).map_err(|e| e.to_string())?;
        std::fs::write(dir.join(format!("{}.json.minisig", s.id)), sig).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Refreshes one catalog, or all. A failure keeps the previous copy and is shown on the catalog.
    pub fn refresh_catalogs(&self, id: Option<&str>) -> Vec<CatalogView> {
        for s in self.catalog_sources().iter().filter(|s| id.is_none_or(|i| i == s.id)) {
            set_error(&s.id, self.refresh_one(s).err());
        }
        self.apply_plugins();
        self.catalog_views()
    }

    /// Installs a plugin a verified catalog lists: downloads it, checks its SHA-256, installs it switched off.
    pub fn install_catalog_plugin(&self, source_id: &str, plugin_id: &str) -> Result<PluginInfo, CoreError> {
        let source = self.catalog_sources().into_iter().find(|s| s.id == source_id).ok_or_else(|| fail("that catalog isn't added"))?;
        let (doc, _, _) = self.load_catalog(&source).map_err(fail)?;
        let entry = doc.plugins.iter().find(|p| p.id == plugin_id).ok_or_else(|| fail(format!("the catalog doesn't list '{plugin_id}'")))?;
        if !entry.url.starts_with("https://") {
            return Err(fail("plugin downloads must use HTTPS"));
        }
        let bytes = self.runtimes.fetch(&entry.url).map_err(fail)?;
        let got: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
        if !got.eq_ignore_ascii_case(&entry.sha256) {
            return Err(CoreError::failed_fix("The plugin wasn't installed.", format!("Its SHA-256 is {got}, not the {} the catalog lists.", entry.sha256), "Refresh the catalog, or tell the publisher."));
        }
        let tmp = self.paths.cache_dir().join(format!("catalog-plugin-{}.zip", crate::ca::unix_now()));
        std::fs::write(&tmp, &bytes)?;
        let result = self.install_plugin(&tmp.display().to_string());
        let _ = std::fs::remove_file(&tmp);
        let info = result?;
        if info.manifest.id != plugin_id {
            let _ = self.remove_plugin(&info.manifest.id);
            return Err(fail(format!("the package holds '{}', not '{plugin_id}'", info.manifest.id)));
        }
        Ok(info)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use base64::Engine;
    use ed25519_dalek::{Signer, SigningKey};

    fn b64(bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    /// A key pair and signature made here in minisign's default "ED" format (Blake2b-512 of the data, signed).
    pub(crate) fn sign(data: &[u8], comment: &str) -> (String, String) {
        let key = SigningKey::from_bytes(&[7u8; 32]);
        let key_id = [1u8, 2, 3, 4, 5, 6, 7, 8];
        let public = [b"Ed".as_slice(), &key_id, key.verifying_key().as_bytes()].concat();
        use blake2::Digest;
        let digest = blake2::Blake2b512::digest(data);
        let sig = key.sign(&digest).to_bytes();
        let sig_line = [b"ED".as_slice(), &key_id, &sig].concat();
        let global = key.sign(&[sig.as_slice(), comment.as_bytes()].concat()).to_bytes();
        let file = format!("untrusted comment: signature\n{}\ntrusted comment: {comment}\n{}\n", b64(&sig_line), b64(&global));
        (b64(&public), file)
    }

    #[test]
    fn a_good_signature_verifies_and_returns_its_comment() {
        let (key, sig) = sign(b"{\"runtimes\":[]}", "published 2026-09-26");
        assert_eq!(verify_signed(&key, b"{\"runtimes\":[]}", &sig).unwrap(), "published 2026-09-26");
        // The key can also come as a whole .pub file.
        assert!(verify_signed(&format!("untrusted comment: minisign public key\n{key}\n"), b"{\"runtimes\":[]}", &sig).is_ok());
    }

    #[test]
    fn a_changed_file_or_another_key_is_refused() {
        let (key, sig) = sign(b"original", "c");
        assert!(verify_signed(&key, b"tampered", &sig).is_err());
        let other = b64(&[b"Ed".as_slice(), &[1u8, 2, 3, 4, 5, 6, 7, 8], SigningKey::from_bytes(&[9u8; 32]).verifying_key().as_bytes()].concat());
        assert!(verify_signed(&other, b"original", &sig).is_err());
        assert!(verify_signed("not a key", b"original", &sig).is_err());
    }

    #[test]
    fn a_verified_catalog_feeds_runtimes_and_a_tampered_cache_is_ignored() {
        let home = crate::test_support::isolated_home();
        let core = crate::app::Inner::new(crate::settings::SettingsService::load(&home.paths).unwrap(), home.paths.clone()).unwrap();
        let doc = r#"{"name":"Acme","runtimes":[{"id":"acme-tool","name":"Acme tool","version":"1.0.0","url":"https://example.com/acme.zip","sha256":"1111111111111111111111111111111111111111111111111111111111111111","binary":"acme.exe"}]}"#;
        let (key, sig) = sign(doc.as_bytes(), "t");
        let file = home.paths.root().join("acme.json");
        std::fs::write(&file, doc).unwrap();
        std::fs::write(home.paths.root().join("acme.json.minisig"), &sig).unwrap();
        core.add_catalog_source("Acme", &file.display().to_string(), &key).unwrap();

        let views = core.refresh_catalogs(None);
        assert!(views[0].verified, "{:?}", views[0].error);
        assert!(crate::catalog::builtin_catalog().iter().any(|m| m.id == "acme-tool"));

        // Edit the cached copy: it no longer verifies, so its runtimes disappear.
        std::fs::write(cache_dir(&core).join("acme.json"), doc.replace("1.0.0", "9.9.9")).unwrap();
        core.apply_plugins();
        assert!(!core.catalog_views()[0].verified);
        assert!(!crate::catalog::builtin_catalog().iter().any(|m| m.id == "acme-tool"));

        // An unsigned or wrongly signed update is refused and the last good copy is kept.
        std::fs::write(&file, doc).unwrap();
        std::fs::write(home.paths.root().join("acme.json.minisig"), sign(b"other", "t").1).unwrap();
        let views = core.refresh_catalogs(Some("acme"));
        assert!(views[0].error.as_deref().unwrap_or("").contains("signature"));
        crate::catalog::set_extra(&[]);
    }
}
