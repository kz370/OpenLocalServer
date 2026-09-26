//! Plugins (§133–135, Stage 16, phase A): declarative packages that add runtimes, Quick Apps,
//! project detections and health checks. A plugin is a folder (or zip) holding `plugin.yaml`.
//!
//! - **No code runs.** A declarative plugin is data checked against a schema. Sandboxed code
//!   plugins (phase B, WASM) are recognised by `kind: wasm` but refused: there is no sandbox yet.
//! - **Permissions are explicit** (§134). A plugin lists what it needs, and every contribution has
//!   to be covered by a listed permission. It stays off until the user approves exactly that list,
//!   and a changed manifest voids the approval.
//! - Runtimes go through the normal Package Manager: HTTPS, a pinned SHA-256, and the install
//!   folder. A plugin cannot reach outside it.

use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::app::{HealthItem, Inner};
use crate::catalog::OwnedManifest;
use crate::error::CoreError;

const STATE_KEY: &str = "plugins";
const MANIFEST_NAMES: [&str; 2] = ["plugin.yaml", "plugin.yml"];
const MAX_FILES: usize = 500;
const MAX_BYTES: u64 = 50 * 1024 * 1024;

const BUILTIN: &[(&str, &str)] = &[
    ("go", include_str!("../catalog/plugins/go.yaml")),
    ("bun", include_str!("../catalog/plugins/bun.yaml")),
    ("java", include_str!("../catalog/plugins/java.yaml")),
    ("dotnet", include_str!("../catalog/plugins/dotnet.yaml")),
];

/// Every permission a plugin may ask for, with the sentence the user is shown.
pub const PERMISSIONS: &[(&str, &str)] = &[
    ("download", "Add runtimes you can download and install (each download is checked against a fixed SHA-256)"),
    ("quick_apps", "Add Quick App recipes (each one still shows what it will run before it does)"),
    ("read_projects", "Look for marker files in your projects to recognise them (nothing is changed)"),
    ("network", "Check whether an address answers, for the health checks it adds"),
];

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub homepage: String,
    /// `declarative` (data only) or `wasm` (sandboxed code, not supported yet).
    #[serde(default = "declarative")]
    pub kind: String,
    /// For `wasm` plugins: the module file.
    #[serde(default)]
    pub entry: Option<String>,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub contributes: Contributes,
}

fn declarative() -> String {
    "declarative".into()
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Contributes {
    #[serde(default)]
    pub runtimes: Vec<OwnedManifest>,
    /// A folder inside the plugin holding Quick App recipes (`*.yaml`).
    #[serde(default)]
    pub quick_apps: Option<String>,
    #[serde(default)]
    pub detections: Vec<Detection>,
    #[serde(default)]
    pub health_checks: Vec<HealthCheck>,
}

/// "This folder is a <name> project": any of `markers` (or all of `all`) exist in it. A marker
/// may be a file or folder name, or `*.ext`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Detection {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub markers: Vec<String>,
    #[serde(default)]
    pub all: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HealthCheck {
    pub id: String,
    pub name: String,
    /// `tcp` (target `host:port`) or `http` (target `http://host[:port]/path`, any 2xx–3xx is healthy).
    pub kind: String,
    pub target: String,
    #[serde(default)]
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionInfo {
    pub id: String,
    pub description: String,
    pub used: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInfo {
    pub manifest: PluginManifest,
    pub builtin: bool,
    pub enabled: bool,
    /// The user approved this exact manifest's permissions.
    pub approved: bool,
    pub permissions: Vec<PermissionInfo>,
    /// Set when the manifest is invalid, or the plugin can't run here.
    pub problem: Option<String>,
    pub runtimes: usize,
    pub quick_apps: usize,
    pub detections: usize,
    pub health_checks: usize,
    pub folder: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginDetection {
    pub plugin: String,
    pub id: String,
    pub name: String,
    pub matched: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct PluginState {
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    approved: Vec<String>,
    /// SHA-256 of the manifest text that was approved.
    #[serde(default)]
    approved_hash: String,
}

fn fail(msg: impl Into<String>) -> CoreError {
    CoreError::failed_fix(
        "That plugin couldn't be used.",
        msg,
        "Check the plugin's plugin.yaml against docs/PLUGINS.md.",
    )
}

fn slug_ok(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 40
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// A relative path that stays inside its folder.
fn relative_ok(p: &str) -> bool {
    !p.is_empty()
        && Path::new(p)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
        && !p.contains(':')
}

fn hash_text(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

impl PluginManifest {
    pub fn parse(text: &str) -> Result<Self, String> {
        let m: PluginManifest =
            serde_yaml_ng::from_str(text).map_err(|e| format!("plugin.yaml: {e}"))?;
        m.validate()?;
        Ok(m)
    }

    /// Every rule a plugin has to meet, before it may be listed as usable.
    pub fn validate(&self) -> Result<(), String> {
        if !slug_ok(&self.id) {
            return Err(format!(
                "the id '{}' may only hold lowercase letters, digits, '-' and '_'",
                self.id
            ));
        }
        if self.name.trim().is_empty() || self.name.len() > 80 {
            return Err("the plugin needs a name of up to 80 characters".into());
        }
        if self.version.trim().is_empty() || self.version.len() > 40 {
            return Err("the plugin needs a version".into());
        }
        match self.kind.as_str() {
            "declarative" => {}
            "wasm" => {
                if !self.entry.as_deref().is_some_and(relative_ok) {
                    return Err("a wasm plugin needs an `entry` module inside its folder".into());
                }
            }
            other => {
                return Err(format!(
                    "unknown plugin kind '{other}' (declarative or wasm)"
                ))
            }
        }
        for p in &self.permissions {
            if !PERMISSIONS.iter().any(|(id, _)| id == p) {
                return Err(format!("unknown permission '{p}'"));
            }
        }
        let has = |p: &str| self.permissions.iter().any(|x| x == p);
        let c = &self.contributes;
        if !c.runtimes.is_empty() && !has("download") {
            return Err("it adds runtimes but doesn't declare the `download` permission".into());
        }
        if c.quick_apps.is_some() && !has("quick_apps") {
            return Err(
                "it adds Quick Apps but doesn't declare the `quick_apps` permission".into(),
            );
        }
        if !c.detections.is_empty() && !has("read_projects") {
            return Err(
                "it adds detections but doesn't declare the `read_projects` permission".into(),
            );
        }
        if !c.health_checks.is_empty() && !has("network") {
            return Err(
                "it adds health checks but doesn't declare the `network` permission".into(),
            );
        }
        for r in &c.runtimes {
            r.check()?;
        }
        if let Some(dir) = &c.quick_apps {
            if !relative_ok(dir) {
                return Err(format!(
                    "the quick_apps folder '{dir}' must be a relative path inside the plugin"
                ));
            }
        }
        let mut seen = std::collections::HashSet::new();
        for d in &c.detections {
            if !slug_ok(&d.id) || !seen.insert(format!("d:{}", d.id)) {
                return Err(format!("detection id '{}' is invalid or repeated", d.id));
            }
            if d.markers.is_empty() && d.all.is_empty() {
                return Err(format!("detection '{}' has no markers", d.id));
            }
            for m in d.markers.iter().chain(&d.all) {
                let name = m.strip_prefix("*.").unwrap_or(m);
                if !relative_ok(name) {
                    return Err(format!(
                        "detection '{}': marker '{m}' must be a relative path",
                        d.id
                    ));
                }
            }
        }
        for h in &c.health_checks {
            if !slug_ok(&h.id) || !seen.insert(format!("h:{}", h.id)) {
                return Err(format!("health check id '{}' is invalid or repeated", h.id));
            }
            match h.kind.as_str() {
                "tcp" => {
                    if split_host_port(&h.target).is_none() {
                        return Err(format!("health check '{}': target must be host:port", h.id));
                    }
                }
                "http" => {
                    if parse_http(&h.target).is_none() {
                        return Err(format!(
                            "health check '{}': target must be an http:// URL",
                            h.id
                        ));
                    }
                }
                other => {
                    return Err(format!(
                        "health check '{}': unknown kind '{other}' (tcp or http)",
                        h.id
                    ))
                }
            }
        }
        Ok(())
    }
}

fn split_host_port(target: &str) -> Option<(String, u16)> {
    let (host, port) = target.rsplit_once(':')?;
    let ok = !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
    ok.then(|| port.parse().ok().map(|p| (host.to_string(), p)))?
}

/// `http://host[:port][/path]` → (host, port, path).
fn parse_http(url: &str) -> Option<(String, u16, String)> {
    let rest = url.strip_prefix("http://")?;
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (h, p.parse().ok()?),
        None => (authority, 80),
    };
    let ok = !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
    ok.then(|| (host.to_string(), port, path.to_string()))
}

/// One check's outcome, without touching anything.
fn run_check(h: &HealthCheck) -> Result<(), String> {
    use std::io::Write;
    use std::net::ToSocketAddrs;
    let timeout = Duration::from_millis(800);
    let (host, port, path) = match h.kind.as_str() {
        "tcp" => split_host_port(&h.target)
            .map(|(h, p)| (h, p, String::new()))
            .ok_or("bad target")?,
        _ => parse_http(&h.target).ok_or("bad target")?,
    };
    let addr = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|e| format!("{host} didn't resolve: {e}"))?
        .next()
        .ok_or("no address")?;
    let mut stream = std::net::TcpStream::connect_timeout(&addr, timeout)
        .map_err(|_| format!("nothing answers on {host}:{port}"))?;
    if h.kind == "tcp" {
        return Ok(());
    }
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));
    write!(stream, "GET {path} HTTP/1.0\r\nHost: {host}\r\nUser-Agent: OpenLocalServer\r\nConnection: close\r\n\r\n").map_err(|e| e.to_string())?;
    let mut head = [0u8; 32];
    let n = stream
        .read(&mut head)
        .map_err(|e| format!("no reply from {host}:{port}: {e}"))?;
    let line = String::from_utf8_lossy(&head[..n]);
    let status: u16 = line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or("not an HTTP reply")?;
    if (200..400).contains(&status) {
        Ok(())
    } else {
        Err(format!("answered HTTP {status}"))
    }
}

fn marker_present(root: &Path, marker: &str) -> bool {
    if let Some(ext) = marker.strip_prefix("*.") {
        let suffix = format!(".{}", ext.to_ascii_lowercase());
        return std::fs::read_dir(root)
            .map(|d| {
                d.flatten().any(|e| {
                    e.file_name()
                        .to_string_lossy()
                        .to_ascii_lowercase()
                        .ends_with(&suffix)
                })
            })
            .unwrap_or(false);
    }
    root.join(marker).exists()
}

/// What `detections` recognise in a folder.
pub fn detect_in(root: &Path, plugin: &str, detections: &[Detection]) -> Vec<PluginDetection> {
    detections
        .iter()
        .filter_map(|d| {
            let any: Vec<String> = d
                .markers
                .iter()
                .filter(|m| marker_present(root, m))
                .cloned()
                .collect();
            let all_ok = d.all.iter().all(|m| marker_present(root, m));
            let hit = (d.markers.is_empty() || !any.is_empty()) && all_ok;
            hit.then(|| PluginDetection {
                plugin: plugin.to_string(),
                id: d.id.clone(),
                name: d.name.clone(),
                matched: any.into_iter().chain(d.all.iter().cloned()).collect(),
            })
        })
        .collect()
}

/// Unpacks a plugin zip without trusting its paths (no `..`, no absolute paths, a size cap).
pub fn extract_plugin_zip(zip_path: &Path, dest: &Path) -> Result<(), String> {
    let file = std::fs::File::open(zip_path).map_err(|e| format!("{}: {e}", zip_path.display()))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("not a zip file: {e}"))?;
    if archive.len() > MAX_FILES {
        return Err(format!("the archive holds more than {MAX_FILES} files"));
    }
    let mut total = 0u64;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let Some(rel) = entry.enclosed_name() else {
            return Err(format!(
                "'{}' would be written outside the plugin folder",
                entry.name()
            ));
        };
        let out = dest.join(&rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            continue;
        }
        total += entry.size();
        if total > MAX_BYTES {
            return Err("the archive is larger than 50 MB when unpacked".into());
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut file = std::fs::File::create(&out).map_err(|e| e.to_string())?;
        std::io::copy(&mut (&mut entry).take(MAX_BYTES), &mut file).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn copy_dir(from: &Path, to: &Path, files: &mut usize, bytes: &mut u64) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
    for e in std::fs::read_dir(from)
        .map_err(|e| e.to_string())?
        .flatten()
    {
        let meta = e.metadata().map_err(|e| e.to_string())?;
        // Links could point anywhere; a plugin is plain files.
        if meta.file_type().is_symlink() {
            continue;
        }
        let target = to.join(e.file_name());
        if meta.is_dir() {
            copy_dir(&e.path(), &target, files, bytes)?;
        } else {
            *files += 1;
            *bytes += meta.len();
            if *files > MAX_FILES || *bytes > MAX_BYTES {
                return Err("the plugin is larger than 500 files or 50 MB".into());
            }
            std::fs::copy(e.path(), &target).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// The folder holding `plugin.yaml`: `dir` itself, or its only sub-folder (a zip's wrapper).
fn find_root(dir: &Path) -> Option<PathBuf> {
    let has = |d: &Path| MANIFEST_NAMES.iter().any(|n| d.join(n).is_file());
    if has(dir) {
        return Some(dir.to_path_buf());
    }
    let subs: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    (subs.len() == 1 && has(&subs[0])).then(|| subs[0].clone())
}

fn read_manifest_text(dir: &Path) -> Option<String> {
    MANIFEST_NAMES
        .iter()
        .find_map(|n| std::fs::read_to_string(dir.join(n)).ok())
}

impl Inner {
    fn plugins_dir(&self) -> PathBuf {
        self.paths.data_dir().join("plugins")
    }

    fn plugin_states(&self) -> std::collections::BTreeMap<String, PluginState> {
        self.settings
            .lock()
            .unwrap()
            .get(STATE_KEY)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default()
    }

    fn save_plugin_states(
        &self,
        states: &std::collections::BTreeMap<String, PluginState>,
    ) -> Result<(), CoreError> {
        self.settings
            .lock()
            .unwrap()
            .set(STATE_KEY.to_string(), serde_json::to_value(states)?)
    }

    /// Every plugin's manifest text, the folder it lives in (none for built-ins), and whether it is built in.
    fn plugin_sources(&self) -> Vec<(String, Option<PathBuf>, bool)> {
        let mut out: Vec<(String, Option<PathBuf>, bool)> = BUILTIN
            .iter()
            .map(|(_, text)| (text.to_string(), None, true))
            .collect();
        if let Ok(dirs) = std::fs::read_dir(self.plugins_dir()) {
            let mut dirs: Vec<PathBuf> = dirs
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect();
            dirs.sort();
            for d in dirs {
                if let Some(text) = read_manifest_text(&d) {
                    out.push((text, Some(d), false));
                }
            }
        }
        out
    }

    fn quick_app_count(dir: &Path, sub: &str) -> usize {
        std::fs::read_dir(dir.join(sub))
            .map(|d| {
                d.flatten()
                    .filter(|e| {
                        matches!(
                            e.path().extension().and_then(|x| x.to_str()),
                            Some("yaml" | "yml")
                        )
                    })
                    .count()
            })
            .unwrap_or(0)
    }

    fn plugin_info(
        &self,
        text: &str,
        folder: Option<&Path>,
        builtin: bool,
        states: &std::collections::BTreeMap<String, PluginState>,
    ) -> Option<PluginInfo> {
        let (manifest, mut problem) = match PluginManifest::parse(text) {
            Ok(m) => (m, None),
            Err(e) => {
                // An invalid plugin is still listed (named by its folder), so it can be removed.
                let id = folder
                    .and_then(|f| f.file_name())
                    .map(|n| n.to_string_lossy().to_string())?;
                (
                    PluginManifest {
                        id: id.clone(),
                        name: id,
                        version: "?".into(),
                        ..Default::default()
                    },
                    Some(e),
                )
            }
        };
        if problem.is_none() && manifest.kind == "wasm" {
            problem = Some("This plugin runs code in a sandbox, which this version of OpenLocalServer doesn't have yet. It can't be turned on.".into());
        }
        let state = states.get(&manifest.id).cloned().unwrap_or_default();
        let approved = problem.is_none()
            && state.approved_hash == hash_text(text)
            && same_set(&state.approved, &manifest.permissions);
        let quick_apps = match (&manifest.contributes.quick_apps, folder) {
            (Some(sub), Some(dir)) if relative_ok(sub) => Self::quick_app_count(dir, sub),
            _ => 0,
        };
        Some(PluginInfo {
            enabled: state.enabled && approved,
            approved,
            permissions: PERMISSIONS
                .iter()
                .filter(|(id, _)| manifest.permissions.iter().any(|p| p == id))
                .map(|(id, d)| PermissionInfo {
                    id: id.to_string(),
                    description: d.to_string(),
                    used: true,
                })
                .collect(),
            problem,
            runtimes: manifest.contributes.runtimes.len(),
            quick_apps,
            detections: manifest.contributes.detections.len(),
            health_checks: manifest.contributes.health_checks.len(),
            folder: folder.map(|f| f.display().to_string()),
            builtin,
            manifest,
        })
    }

    pub fn list_plugins(&self) -> Vec<PluginInfo> {
        let states = self.plugin_states();
        self.plugin_sources()
            .iter()
            .filter_map(|(text, folder, builtin)| {
                self.plugin_info(text, folder.as_deref(), *builtin, &states)
            })
            .collect()
    }

    fn get_plugin(&self, id: &str) -> Result<PluginInfo, CoreError> {
        self.list_plugins()
            .into_iter()
            .find(|p| p.manifest.id == id)
            .ok_or_else(|| fail(format!("no plugin '{id}' is installed")))
    }

    /// Installs a plugin from a folder or a `.zip`. It arrives switched off, with nothing approved.
    pub fn install_plugin(&self, source: &str) -> Result<PluginInfo, CoreError> {
        let src = PathBuf::from(source.trim());
        let staging = self
            .paths
            .cache_dir()
            .join(format!("plugin-stage-{}", crate::ca::unix_now()));
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(&staging)?;
        let result = (|| -> Result<PluginInfo, CoreError> {
            if src.is_file() {
                extract_plugin_zip(&src, &staging).map_err(fail)?;
            } else if src.is_dir() {
                let (mut files, mut bytes) = (0, 0);
                copy_dir(&src, &staging, &mut files, &mut bytes).map_err(fail)?;
            } else {
                return Err(fail(format!("{source} is not a folder or a .zip file")));
            }
            let root = find_root(&staging).ok_or_else(|| fail("there is no plugin.yaml in it"))?;
            let text =
                read_manifest_text(&root).ok_or_else(|| fail("plugin.yaml can't be read"))?;
            let manifest = PluginManifest::parse(&text).map_err(fail)?;
            if manifest.kind == "wasm" {
                return Err(fail(
                    "code plugins need a sandbox that isn't part of this version",
                ));
            }
            if BUILTIN.iter().any(|(id, _)| *id == manifest.id) {
                return Err(fail(format!("'{}' is a built-in plugin's id", manifest.id)));
            }
            if let Some(sub) = &manifest.contributes.quick_apps {
                if Self::quick_app_count(&root, sub) == 0 {
                    return Err(fail(format!(
                        "the quick_apps folder '{sub}' holds no .yaml recipes"
                    )));
                }
            }
            let dest = self.plugins_dir().join(&manifest.id);
            let was_enabled = self
                .plugin_states()
                .get(&manifest.id)
                .is_some_and(|s| s.enabled);
            let _ = std::fs::remove_dir_all(&dest);
            std::fs::create_dir_all(&dest)?;
            let (mut files, mut bytes) = (0, 0);
            copy_dir(&root, &dest, &mut files, &mut bytes).map_err(fail)?;
            // A replaced plugin starts over: switched off, permissions asked for again.
            let mut states = self.plugin_states();
            states.remove(&manifest.id);
            self.save_plugin_states(&states)?;
            if was_enabled {
                self.apply_plugins();
            }
            self.get_plugin(&manifest.id)
        })();
        let _ = std::fs::remove_dir_all(&staging);
        result
    }

    /// Turns a plugin on or off. Turning on needs `approve` to list exactly the permissions it declares.
    pub fn set_plugin_enabled(
        &self,
        id: &str,
        enabled: bool,
        approve: &[String],
    ) -> Result<PluginInfo, CoreError> {
        let sources = self.plugin_sources();
        let (text, _, _) = sources
            .iter()
            .find(|(t, _, _)| {
                PluginManifest::parse(t)
                    .map(|m| m.id == id)
                    .unwrap_or(false)
            })
            .ok_or_else(|| fail(format!("no plugin '{id}' is installed")))?;
        let manifest = PluginManifest::parse(text).map_err(fail)?;
        let mut states = self.plugin_states();
        if enabled {
            if manifest.kind == "wasm" {
                return Err(fail("code plugins can't be turned on in this version"));
            }
            if !same_set(approve, &manifest.permissions) {
                return Err(CoreError::failed_fix(
                    "The plugin wasn't turned on.",
                    "Its permissions have to be approved as listed.",
                    "Review the permissions and approve them.",
                ));
            }
            states.insert(
                id.to_string(),
                PluginState {
                    enabled: true,
                    approved: manifest.permissions.clone(),
                    approved_hash: hash_text(text),
                },
            );
        } else if let Some(s) = states.get_mut(id) {
            s.enabled = false;
        }
        self.save_plugin_states(&states)?;
        self.apply_plugins();
        self.get_plugin(id)
    }

    pub fn remove_plugin(&self, id: &str) -> Result<(), CoreError> {
        if BUILTIN.iter().any(|(b, _)| *b == id) {
            return Err(fail("a built-in plugin can be turned off, not removed"));
        }
        if !slug_ok(id) {
            return Err(fail("not a plugin id"));
        }
        let dir = self.plugins_dir().join(id);
        if dir.is_dir() {
            std::fs::remove_dir_all(&dir)?;
        }
        let mut states = self.plugin_states();
        states.remove(id);
        self.save_plugin_states(&states)?;
        self.apply_plugins();
        Ok(())
    }

    /// The manifests of plugins that are on: approved as they stand today.
    fn enabled_plugins(&self) -> Vec<(PluginManifest, Option<PathBuf>)> {
        let states = self.plugin_states();
        self.plugin_sources()
            .into_iter()
            .filter_map(|(text, folder, _)| {
                let m = PluginManifest::parse(&text).ok()?;
                let s = states.get(&m.id)?;
                (s.enabled
                    && m.kind == "declarative"
                    && s.approved_hash == hash_text(&text)
                    && same_set(&s.approved, &m.permissions))
                .then_some((m, folder))
            })
            .collect()
    }

    /// Feeds enabled plugins' runtimes, verified catalog runtimes and Quick Apps into the app.
    /// Call at startup and after anything that changes them.
    pub fn apply_plugins(&self) {
        let enabled = self.enabled_plugins();
        let mut runtimes: Vec<OwnedManifest> = enabled
            .iter()
            .flat_map(|(m, _)| m.contributes.runtimes.clone())
            .collect();
        runtimes.extend(self.catalog_runtimes());
        crate::catalog::set_extra(&runtimes);

        let mut quick = self.catalog.lock().unwrap();
        quick.clear_plugin_sources();
        for (m, folder) in &enabled {
            if let (Some(sub), Some(dir)) = (&m.contributes.quick_apps, folder) {
                let _ = quick.import_plugin(&m.id, &dir.join(sub));
            }
        }
    }

    /// What enabled plugins recognise in a project.
    pub fn plugin_detect(&self, project_id: &str) -> Result<Vec<PluginDetection>, CoreError> {
        let project = self
            .projects
            .lock()
            .unwrap()
            .get(project_id)
            .ok_or_else(|| fail(format!("no project '{project_id}'")))?;
        let root = PathBuf::from(&project.path);
        Ok(self
            .enabled_plugins()
            .iter()
            .flat_map(|(m, _)| detect_in(&root, &m.id, &m.contributes.detections))
            .collect())
    }

    /// The health checks of enabled plugins, as environment-health items. Runs each with a short timeout.
    pub fn plugin_health(&self) -> Vec<HealthItem> {
        let mut items = Vec::new();
        for (m, _) in self.enabled_plugins() {
            for h in m.contributes.health_checks.iter().take(8) {
                let label = format!("{} — {}", m.name, h.name);
                items.push(match run_check(h) {
                    Ok(()) => HealthItem {
                        id: format!("plugin_{}_{}", m.id, h.id),
                        label,
                        status: "ok".into(),
                        detail: format!("{} answers", h.target),
                        fix: None,
                    },
                    Err(e) => HealthItem {
                        id: format!("plugin_{}_{}", m.id, h.id),
                        label,
                        status: "warn".into(),
                        detail: e,
                        fix: h.hint.clone(),
                    },
                });
            }
        }
        items
    }
}

fn same_set(a: &[String], b: &[String]) -> bool {
    let sa: std::collections::BTreeSet<&String> = a.iter().collect();
    let sb: std::collections::BTreeSet<&String> = b.iter().collect();
    sa == sb
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"
id: demo
name: Demo
version: 1.2.0
permissions: [download, read_projects, network]
contributes:
  runtimes:
    - id: demo
      name: Demo
      version: "1.0"
      url: https://example.com/demo.zip
      sha256: 0000000000000000000000000000000000000000000000000000000000000000
      binary: demo.exe
  detections:
    - id: demo-project
      name: Demo
      markers: [demo.toml, "*.demo"]
  health_checks:
    - id: db
      name: Local database
      kind: tcp
      target: 127.0.0.1:5432
"#;

    #[test]
    fn a_good_manifest_parses() {
        let m = PluginManifest::parse(GOOD).unwrap();
        assert_eq!(m.contributes.runtimes[0].id, "demo");
        assert_eq!(m.kind, "declarative");
    }

    #[test]
    fn every_builtin_plugin_is_valid() {
        for (id, text) in BUILTIN {
            let m = PluginManifest::parse(text).unwrap_or_else(|e| panic!("{id}: {e}"));
            assert_eq!(&m.id, id);
        }
    }

    #[test]
    fn contributions_need_their_permission() {
        let text = GOOD.replace(
            "[download, read_projects, network]",
            "[read_projects, network]",
        );
        assert!(PluginManifest::parse(&text)
            .unwrap_err()
            .contains("download"));
        let text = GOOD.replace(
            "[download, read_projects, network]",
            "[download, read_projects]",
        );
        assert!(PluginManifest::parse(&text)
            .unwrap_err()
            .contains("network"));
    }

    #[test]
    fn unsafe_manifests_are_refused() {
        assert!(
            PluginManifest::parse(&GOOD.replace("https://example.com", "http://example.com"))
                .unwrap_err()
                .contains("HTTPS")
        );
        assert!(
            PluginManifest::parse(&GOOD.replace("binary: demo.exe", "binary: ../demo.exe"))
                .is_err()
        );
        assert!(PluginManifest::parse(&GOOD.replace("id: demo\n", "id: ../demo\n")).is_err());
        assert!(PluginManifest::parse(
            &GOOD.replace("permissions: [download", "permissions: [root, download")
        )
        .unwrap_err()
        .contains("unknown permission"));
        assert!(PluginManifest::parse(&GOOD.replace(
            "0000000000000000000000000000000000000000000000000000000000000000",
            "abc"
        ))
        .is_err());
    }

    #[test]
    fn detections_match_files_and_globs() {
        let dir = tempfile::tempdir().unwrap();
        let d = vec![Detection {
            id: "demo".into(),
            name: "Demo".into(),
            markers: vec!["demo.toml".into(), "*.demo".into()],
            all: vec![],
        }];
        assert!(detect_in(dir.path(), "p", &d).is_empty());
        std::fs::write(dir.path().join("app.DEMO"), "").unwrap();
        assert_eq!(detect_in(dir.path(), "p", &d)[0].name, "Demo");
    }

    #[test]
    fn a_zip_cannot_write_outside_its_folder() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("evil.zip");
        {
            let mut w = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
            let opts = zip::write::SimpleFileOptions::default();
            w.start_file("../escaped.txt", opts).unwrap();
            w.write_all(b"x").unwrap();
            w.finish().unwrap();
        }
        let dest = dir.path().join("out");
        std::fs::create_dir_all(&dest).unwrap();
        assert!(extract_plugin_zip(&zip_path, &dest).is_err());
        assert!(!dir.path().join("escaped.txt").exists());
    }

    #[test]
    fn health_checks_see_a_listening_port() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let up = HealthCheck {
            id: "a".into(),
            name: "a".into(),
            kind: "tcp".into(),
            target: format!("127.0.0.1:{port}"),
            hint: None,
        };
        assert!(run_check(&up).is_ok());
        drop(listener);
        assert!(run_check(&up).is_err());
    }
}
