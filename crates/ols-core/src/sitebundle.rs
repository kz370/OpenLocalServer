//! Site bundles (§165) — exporting and importing whole sites, in one file, on purpose.
//!
//! A **site bundle** is a zip that can hold any mix of four things for any number of sites:
//! their settings (the site record, the project manifest, workers, scheduled tasks, tunnels,
//! mode), their `.env` files, their database dumps, and their project files. The user picks
//! which, which is the whole point — settings travel in a few kilobytes, files and dumps do
//! not.
//!
//! Sites are grouped by the project behind them, so exporting `shop.test` and `blog.test` of
//! one project gives one entry with two sites, and importing it gives one project with two
//! sites rather than two projects. A site with no project behind it has nothing to describe
//! and is reported, never exported half-way.
//!
//! **Passwords.** A bundle may be exported encrypted, and then its `.env` files and database
//! dumps — the two places passwords actually live — are sealed under a key derived from the
//! export password (`crypto.rs`). Project files and settings are not sealed: they are source
//! code and configuration the user can read anyway, and sealing them would make the bundle
//! useless for review. The manifest is never sealed, so a reader can see what a bundle holds
//! without the password; what they cannot see is a single secret value.
//!
//! **Importing onto an existing site** takes a snapshot of the current state first, exactly
//! as restoring a snapshot does, and says which one it took — so "update" is reversible
//! rather than a leap of faith. Importing as a new site instead renames everything the
//! collision would have made ambiguous (hostname, database, port), the same way cloning does.
//!
//! Everything long runs on a [`TaskHandle`], so the Processes page shows which site is being
//! written and a twenty-site export is not one frozen bar.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::app::Inner;
use crate::crypto::{self, BundleCipher, CryptoError};
use crate::error::CoreError;
use crate::project::Project;
use crate::snapshots::{Adjust, DatabaseMeta, SnapshotContent, SnapshotOptions};
use crate::tasks::TaskHandle;

/// The manifest's name inside the zip.
pub const META: &str = "bundle.json";
const FORMAT: u32 = 1;
/// What a bundle file is called on disk, so the import picker can find them and the export
/// says what it produced.
pub const EXTENSION: &str = "olsbundle.zip";

fn err(msg: impl Into<String>) -> CoreError {
    CoreError::BundleError(msg.into())
}

/// A wrong password and a damaged file fail the same way — an AEAD cannot tell them apart —
/// so the message must not claim to know which it was.
fn crypto_err(e: CryptoError) -> CoreError {
    let detail = e.to_string();
    match e {
        CryptoError::Unseal => CoreError::Failed {
            problem: "That bundle could not be unlocked.".into(),
            cause: detail,
            fix: Some(
                "Check the password the bundle was exported with. If it is right, try another \
                 copy: a damaged file is refused exactly like a wrong password."
                    .into(),
            ),
        },
        _ => CoreError::BundleError(detail),
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn yes() -> bool {
    true
}

/// What goes into a bundle. Settings and `.env` files are on by default because they are
/// small and are what "the site's configuration" means to most people; data is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleOptions {
    #[serde(default = "yes")]
    pub settings: bool,
    #[serde(default = "yes")]
    pub env: bool,
    #[serde(default)]
    pub databases: bool,
    #[serde(default)]
    pub files: bool,
}

impl Default for BundleOptions {
    fn default() -> Self {
        BundleOptions {
            settings: true,
            env: true,
            databases: false,
            files: false,
        }
    }
}

impl BundleOptions {
    /// A bundle with nothing selected is a zip holding a manifest; say so rather than write
    /// one and call it a site.
    pub fn is_empty(&self) -> bool {
        !self.settings && !self.env && !self.databases && !self.files
    }
}

/// One `.env` file inside a bundle, and where its bytes are.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleEnvFile {
    /// The file's name in the project folder: `.env`, `.env.local`, ...
    pub name: String,
    /// Its path inside the zip, ending `.enc` when the bundle is encrypted.
    pub entry: String,
    #[serde(default)]
    pub sealed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleSite {
    /// This site's directory inside the zip.
    pub dir: String,
    pub project_id: String,
    pub project_name: String,
    pub project_path: String,
    /// The sites this entry covers.
    pub hostnames: Vec<String>,
    /// The site's configuration as a `SnapshotContent` document, with the `.env` contents and
    /// the dump names left out — they are listed here instead, so the manifest itself never
    /// holds a secret.
    pub settings: String,
    #[serde(default)]
    pub env_files: Vec<BundleEnvFile>,
    #[serde(default)]
    pub databases: Vec<DatabaseMeta>,
    #[serde(default)]
    pub file_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleManifest {
    pub format: u32,
    pub app_version: String,
    pub created_ms: u64,
    pub label: String,
    /// `.env` files and database dumps are sealed with the export password.
    pub encrypted: bool,
    /// Hex Argon2 salt, when encrypted.
    #[serde(default)]
    pub salt: Option<String>,
    pub options: BundleOptions,
    pub sites: Vec<BundleSite>,
    /// SHA-256 of the finished file, so a copy can be checked on the way back in.
    #[serde(default)]
    pub sha256: Option<String>,
}

/// What an export produced.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BundleSummary {
    pub path: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub sites: Vec<String>,
    pub databases: usize,
    pub env_files: usize,
    pub file_count: usize,
    pub encrypted: bool,
    pub problems: Vec<String>,
}

/// One site as the import preview shows it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundlePreviewSite {
    pub project_name: String,
    pub hostnames: Vec<String>,
    /// Hostnames that already exist — the ones the update / rename choice is about.
    pub existing: Vec<String>,
    /// A free hostname for each, which is what a rename import would use.
    pub suggested: Vec<String>,
    pub summary: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundlePreview {
    pub source: String,
    pub label: String,
    pub created_ms: u64,
    pub encrypted: bool,
    pub options: BundleOptions,
    pub sites: Vec<BundlePreviewSite>,
    /// Read once, up front, so a damaged bundle is refused before anything is created.
    pub problems: Vec<String>,
}

/// What one site of an import did.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleSiteResult {
    pub project_name: String,
    /// "created", "updated" or "skipped".
    pub action: String,
    pub hostnames: Vec<String>,
    /// The snapshot taken before an update, so it can be put back.
    pub safety_snapshot: Option<String>,
    pub changes: Vec<String>,
    pub problems: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BundleImportResult {
    pub sites: Vec<BundleSiteResult>,
    /// An encrypted bundle was unlocked. The password itself is never kept.
    pub unlocked: bool,
}

/// What to do about a hostname that already exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OnConflict {
    /// Put the bundle's version on the existing site, after a snapshot of what is there.
    Update,
    /// Create it as a new site under a different name.
    Rename,
}

// ------------------------------------------------------------------------ grouping

/// The picked sites of one project.
struct Group {
    project: Project,
    hostnames: Vec<String>,
}

/// Turns picked hostnames into per-project groups, in the order the user picked them. A
/// hostname with no site, or a site with no project behind it, is reported and left out:
/// there is nothing to export for it, and half an entry would import as a mystery later.
fn groups(inner: &Inner, hostnames: &[String], problems: &mut Vec<String>) -> Vec<Group> {
    let domains: BTreeMap<String, _> = inner
        .domains
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .list()
        .into_iter()
        .map(|d| (d.hostname.clone(), d))
        .collect();
    let by_id: BTreeMap<String, Project> = inner
        .projects
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .list()
        .into_iter()
        .map(|p| (p.id.clone(), p))
        .collect();

    let mut out: Vec<Group> = Vec::new();
    for host in hostnames {
        let Some(domain) = domains.get(host) else {
            problems.push(format!(
                "site {host} was not found; it may have been deleted"
            ));
            continue;
        };
        let Some(pid) = domain.project_id.as_deref() else {
            problems.push(format!(
                "site {host} has no project behind it, so there is nothing to export for it"
            ));
            continue;
        };
        let Some(project) = by_id.get(pid) else {
            problems.push(format!(
                "site {host} points at a project that is not registered any more"
            ));
            continue;
        };
        match out.iter_mut().find(|g| g.project.id == project.id) {
            Some(g) => g.hostnames.push(host.clone()),
            None => out.push(Group {
                project: project.clone(),
                hostnames: vec![host.clone()],
            }),
        }
    }
    out
}

/// `<slug>.olsbundle.zip`, never overwriting: two exports in the same millisecond are two
/// files, not one clobbered file.
fn bundle_path(dest: &str, label: &str) -> Result<PathBuf, CoreError> {
    let dir = PathBuf::from(dest);
    if !dir.is_absolute() {
        return Err(err("choose a full folder path to export into"));
    }
    std::fs::create_dir_all(&dir)?;
    let slug = crate::domain::slugify(label);
    let slug = if slug.is_empty() {
        "sites".into()
    } else {
        slug
    };
    let base = format!("{slug}-{}", now_ms());
    for seq in 0..u32::MAX {
        let name = if seq == 0 {
            format!("{base}.{EXTENSION}")
        } else {
            format!("{base}-{seq}.{EXTENSION}")
        };
        let candidate = dir.join(name);
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    unreachable!("a free file name is found long before u32::MAX")
}

// ------------------------------------------------------------------------ export

/// Writes one bundle for the picked sites, reporting every site into the task as it goes.
pub fn export(
    inner: &Inner,
    h: &TaskHandle,
    hostnames: &[String],
    options: BundleOptions,
    dest: &str,
    password: Option<&str>,
) -> Result<BundleSummary, CoreError> {
    if options.is_empty() {
        return Err(err(
            "nothing was selected to export: choose at least settings, environment files, \
             databases or project files",
        ));
    }
    if hostnames.is_empty() {
        return Err(err("no site was selected to export"));
    }
    // An empty password box means "no password", not "encrypt with nothing".
    let password = password.filter(|p| !p.trim().is_empty());
    let mut problems: Vec<String> = Vec::new();
    let selected = groups(inner, hostnames, &mut problems);
    if selected.is_empty() {
        return Err(err(format!(
            "no site could be exported: {}",
            problems.join("; ")
        )));
    }
    let (cipher, salt) = match password {
        Some(pw) => {
            let salt = crypto::random_salt().map_err(crypto_err)?;
            let cipher = BundleCipher::new(pw, &salt).map_err(crypto_err)?;
            (Some(cipher), Some(crypto::to_hex(&salt)))
        }
        None => (None, None),
    };

    let label = if selected.len() == 1 {
        selected[0].project.name.clone()
    } else {
        format!("{} sites", selected.len())
    };
    let file = bundle_path(dest, &label)?;
    let mut summary = BundleSummary {
        path: file.display().to_string(),
        encrypted: cipher.is_some(),
        ..Default::default()
    };
    for p in problems {
        h.problem(p.clone());
        summary.problems.push(p);
    }

    // Gathered first, written second: a dump that fails must not leave a half-written zip
    // whose manifest claims it is complete.
    h.begin_step("Reading site configuration");
    let mut prepared: Vec<PreparedSite> = Vec::new();
    for g in &selected {
        // Between sites, never mid-dump: the item in flight is finished, the rest is not.
        if h.cancel_requested() {
            return Err(h.check_cancelled().unwrap_err());
        }
        h.begin_step(&format!("Reading {}", g.project.name));
        match prepare(inner, g, options, cipher.as_ref()) {
            Ok(p) => prepared.push(p),
            Err(e) => {
                let line = format!("{}: {e}", g.project.name);
                h.problem(line.clone());
                summary.problems.push(line);
            }
        }
        h.advance(1);
    }
    if prepared.is_empty() {
        return Err(err(format!(
            "no site could be exported: {}",
            summary.problems.join("; ")
        )));
    }

    h.begin_step("Writing the bundle");
    let manifest = write_bundle(&file, &prepared, options, cipher.as_ref(), salt.as_deref())?;
    summary.size_bytes = std::fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
    summary.sha256 = sha256_of(&file);
    summary.sites = manifest
        .sites
        .iter()
        .map(|s| s.project_name.clone())
        .collect();
    summary.databases = manifest
        .sites
        .iter()
        .map(|s| s.databases.iter().filter(|d| d.dump.is_some()).count())
        .sum();
    summary.env_files = manifest.sites.iter().map(|s| s.env_files.len()).sum();
    summary.file_count = manifest.sites.iter().map(|s| s.file_count).sum();
    h.set_bytes(summary.size_bytes, None);
    Ok(summary)
}

/// One site's contribution, gathered as bytes rather than left as files on disk. A site
/// bundle is a configuration handover, not an archive of every dependency, so the practical
/// size is one dump and a couple of `.env` files.
struct PreparedSite {
    content: SnapshotContent,
    hostnames: Vec<String>,
    env: Vec<(String, Vec<u8>)>,
    databases: Vec<(String, Vec<u8>)>,
    files: Vec<(String, PathBuf)>,
}

impl PreparedSite {
    fn dir(&self) -> String {
        format!(
            "sites/{}",
            crate::domain::slugify(&self.content.project.name)
        )
    }
}

fn prepare(
    inner: &Inner,
    g: &Group,
    options: BundleOptions,
    cipher: Option<&BundleCipher>,
) -> Result<PreparedSite, CoreError> {
    let mut content = inner.snapshot_content(
        &g.project.id,
        "Site bundle",
        SnapshotOptions {
            env: options.env,
            databases: false,
            files: false,
        },
    )?;
    // The bundle carries the picked sites, not every site the project happens to have.
    let picked: BTreeSet<&str> = g.hostnames.iter().map(String::as_str).collect();
    content
        .domains
        .retain(|d| picked.contains(d.hostname.as_str()));
    let kept: Vec<String> = content.domains.iter().map(|d| d.hostname.clone()).collect();
    content.web_configs.retain(|h, _| kept.contains(h));

    let mut env = Vec::new();
    if options.env {
        for (name, text) in std::mem::take(&mut content.env_files) {
            env.push((name, seal_or_plain(cipher, text.into_bytes())?));
        }
    }
    let mut databases = Vec::new();
    if options.databases {
        for db in content.databases.clone() {
            if db.engine == "mongodb" {
                continue;
            }
            if !inner.services.is_running(&db.engine) {
                inner
                    .start_service_and_wait(&db.engine, &mut |_| {})
                    .map_err(err)?;
            }
            let dump = crate::dbbackup::backup(&inner.services, &inner.paths, &db.engine, &db.name)
                .map_err(err)?;
            let bytes = std::fs::read(&dump).map_err(|e| err(format!("{}.sql: {e}", db.name)))?;
            // The bundle is the copy; the working backup folder is not.
            let _ = std::fs::remove_file(&dump);
            databases.push((
                format!("{}-{}.sql", db.engine, db.name),
                seal_or_plain(cipher, bytes)?,
            ));
        }
    }
    let mut files = Vec::new();
    if options.files {
        let root = PathBuf::from(&g.project.path);
        for rel in crate::snapshots::project_files(&root) {
            files.push((
                rel.display().to_string().replace('\\', "/"),
                root.join(&rel),
            ));
        }
    }
    Ok(PreparedSite {
        content,
        hostnames: g.hostnames.clone(),
        env,
        databases,
        files,
    })
}

fn seal_or_plain(cipher: Option<&BundleCipher>, bytes: Vec<u8>) -> Result<Vec<u8>, CoreError> {
    match cipher {
        Some(c) => c.seal(&bytes).map_err(crypto_err),
        None => Ok(bytes),
    }
}

fn zip_options() -> zip::write::SimpleFileOptions {
    zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .large_file(true)
}

/// Writes the zip through a `.part` file, so an interrupted export never leaves a bundle
/// that claims to be complete. The manifest goes in last, the same rule the snapshot writer
/// follows.
fn write_bundle(
    file: &Path,
    sites: &[PreparedSite],
    options: BundleOptions,
    cipher: Option<&BundleCipher>,
    salt: Option<&str>,
) -> Result<BundleManifest, CoreError> {
    let tmp = file.with_extension("part");
    let mut manifest = BundleManifest {
        format: FORMAT,
        app_version: env!("CARGO_PKG_VERSION").into(),
        created_ms: now_ms(),
        label: file
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default(),
        encrypted: cipher.is_some(),
        salt: salt.map(str::to_string),
        options,
        sites: Vec::new(),
        sha256: None,
    };
    let result = (|| -> Result<(), CoreError> {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&tmp)?);
        for site in sites {
            let dir = site.dir();
            let mut meta = BundleSite {
                dir: dir.clone(),
                project_id: site.content.project.id.clone(),
                project_name: site.content.project.name.clone(),
                project_path: site.content.project.path.clone(),
                hostnames: site.hostnames.clone(),
                settings: String::new(),
                env_files: Vec::new(),
                databases: site.content.databases.clone(),
                file_count: site.files.len(),
            };

            for (name, bytes) in &site.env {
                let entry = format!("{dir}/env/{name}{}", sealed_suffix(cipher.is_some()));
                zip.start_file(entry.as_str(), zip_options())
                    .map_err(|e| err(e.to_string()))?;
                zip.write_all(bytes)?;
                meta.env_files.push(BundleEnvFile {
                    name: name.clone(),
                    entry,
                    sealed: cipher.is_some(),
                });
            }
            for (name, bytes) in &site.databases {
                let entry = format!("{dir}/databases/{name}{}", sealed_suffix(cipher.is_some()));
                zip.start_file(entry.as_str(), zip_options())
                    .map_err(|e| err(e.to_string()))?;
                zip.write_all(bytes)?;
                for d in meta.databases.iter_mut() {
                    if d.dump.is_none() {
                        d.dump = Some(entry.clone());
                    }
                }
            }
            for (rel, path) in &site.files {
                let entry = format!("{dir}/files/{rel}");
                zip.start_file(entry.as_str(), zip_options())
                    .map_err(|e| err(e.to_string()))?;
                std::io::copy(
                    &mut std::fs::File::open(path).map_err(|e| err(format!("{rel}: {e}")))?,
                    &mut zip,
                )?;
            }

            // The configuration document, with the bytes that now live in the zip removed.
            let mut content = site.content.clone();
            content.options.env = false;
            content.env_files.clear();
            content.file_count = site.files.len();
            for d in content.databases.iter_mut() {
                d.dump = None;
            }
            meta.settings = serde_json::to_string(&content)?;
            manifest.sites.push(meta);
        }
        zip.start_file(META, zip_options())
            .map_err(|e| err(e.to_string()))?;
        zip.write_all(serde_json::to_string_pretty(&manifest)?.as_bytes())?;
        zip.finish().map_err(|e| err(e.to_string()))?;
        Ok(())
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    std::fs::rename(&tmp, file)?;
    tracing::info!(file = %file.display(), sites = manifest.sites.len(), encrypted = manifest.encrypted, "site bundle written");
    Ok(manifest)
}

fn sealed_suffix(sealed: bool) -> &'static str {
    if sealed {
        ".enc"
    } else {
        ""
    }
}

/// SHA-256 of a finished file, so a bundle copied to another machine can be checked there.
pub fn sha256_of(file: &Path) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    if let Ok(mut f) = std::fs::File::open(file) {
        let _ = std::io::copy(&mut f, &mut h);
    }
    crypto::to_hex(&h.finalize())
}

// ------------------------------------------------------------------------ reading

/// Opens a bundle's manifest, refusing a format from a newer OLS before anything is created.
fn read_manifest(source: &str) -> Result<BundleManifest, CoreError> {
    let path = PathBuf::from(source);
    let file = std::fs::File::open(&path).map_err(|e| err(format!("{}: {e}", path.display())))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|_| err(format!("{} is not a site bundle", path.display())))?;
    let mut text = String::new();
    zip.by_name(META)
        .map_err(|_| err(format!("{} is not an OLS site bundle", path.display())))?
        .read_to_string(&mut text)
        .map_err(|e| err(e.to_string()))?;
    let m: BundleManifest = serde_json::from_str(&text)
        .map_err(|e| err(format!("the bundle's description is damaged: {e}")))?;
    if m.format > FORMAT {
        return Err(err(format!(
            "this bundle was made by a newer OLS (format {})",
            m.format
        )));
    }
    Ok(m)
}

impl BundleManifest {
    /// The key, when the bundle is encrypted. The salt is in the manifest, so this works
    /// before a single entry is opened.
    fn cipher(&self, password: Option<&str>) -> Result<Option<BundleCipher>, CoreError> {
        if !self.encrypted {
            return Ok(None);
        }
        let Some(password) = password.filter(|p| !p.is_empty()) else {
            return Err(err(
                "this bundle is encrypted: enter the password it was exported with",
            ));
        };
        let salt = self
            .salt
            .as_deref()
            .and_then(crypto::from_hex)
            .ok_or_else(|| err("this bundle's password salt is missing or damaged"))?;
        Ok(Some(
            BundleCipher::new(password, &salt).map_err(crypto_err)?,
        ))
    }

    /// One site's configuration, with its dump names and `.env` contents put back, so every
    /// import path downstream is the same one a snapshot restore already uses.
    fn content_of(
        &self,
        site: &BundleSite,
        cipher: Option<&BundleCipher>,
        source: &Path,
    ) -> Result<SnapshotContent, CoreError> {
        let mut content: SnapshotContent = serde_json::from_str(&site.settings).map_err(|e| {
            err(format!(
                "{}'s configuration is damaged: {e}",
                site.project_name
            ))
        })?;
        content.options.env = !site.env_files.is_empty();
        content.databases = site.databases.clone();
        content.file_count = site.file_count;
        if !site.env_files.is_empty() {
            let mut zip = zip::ZipArchive::new(std::fs::File::open(source)?)
                .map_err(|e| err(e.to_string()))?;
            for f in &site.env_files {
                let mut bytes = Vec::new();
                zip.by_name(&f.entry)
                    .map_err(|_| {
                        err(format!(
                            "{}: {} is missing from the bundle",
                            site.project_name, f.name
                        ))
                    })?
                    .read_to_end(&mut bytes)?;
                if f.sealed {
                    let c = cipher.ok_or_else(|| {
                        err("this bundle's .env files are sealed, but it is not marked encrypted")
                    })?;
                    bytes = c.open(&bytes).map_err(crypto_err)?;
                }
                content
                    .env_files
                    .insert(f.name.clone(), String::from_utf8_lossy(&bytes).to_string());
            }
        }
        Ok(content)
    }
}

/// Reads a bundle for review before anything is created.
pub fn preview(inner: &Inner, source: &str) -> Result<BundlePreview, CoreError> {
    let m = read_manifest(source)?;
    let known: BTreeSet<String> = inner
        .domains
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .list()
        .into_iter()
        .map(|d| d.hostname)
        .collect();
    let mut problems = Vec::new();
    let mut sites = Vec::new();
    for site in &m.sites {
        let existing: Vec<String> = site
            .hostnames
            .iter()
            .filter(|h| known.contains(*h))
            .cloned()
            .collect();
        let mut summary: Vec<String> = site
            .env_files
            .iter()
            .map(|f| {
                format!(
                    "{} ({})",
                    f.name,
                    if f.sealed { "encrypted" } else { "plain" }
                )
            })
            .chain(
                site.databases
                    .iter()
                    .filter(|d| d.dump.is_some())
                    .map(|d| format!("database {} ({})", d.name, d.engine)),
            )
            .collect();
        if site.file_count > 0 {
            summary.push(format!("{} project file(s)", site.file_count));
        }

        // The suggestions depend on the configuration document, which a damaged bundle may
        // not have. That is reported here rather than turning a preview into a dead end:
        // the import refuses it anyway, and says which site is damaged.
        let free = inner.free_project_name(&site.project_name);
        let suggested = match serde_json::from_str::<SnapshotContent>(&site.settings) {
            Ok(content) => {
                let adjust = Adjust::new(
                    &content,
                    &free,
                    &PathBuf::from(&site.project_path).with_file_name(&free),
                    inner,
                );
                site.hostnames.iter().map(|h| adjust.host(h)).collect()
            }
            Err(_) => {
                problems.push(format!(
                    "{}'s configuration is damaged; the import will refuse it",
                    site.project_name
                ));
                summary.push("its configuration is damaged".to_string());
                site.hostnames.clone()
            }
        };
        sites.push(BundlePreviewSite {
            project_name: site.project_name.clone(),
            hostnames: site.hostnames.clone(),
            existing,
            suggested,
            summary,
        });
    }
    if m.encrypted {
        problems.push(
            "this bundle is encrypted: its .env files and database dumps need the export \
             password"
                .into(),
        );
    }
    if m.sites.is_empty() {
        problems.push("this bundle holds no sites".into());
    }
    Ok(BundlePreview {
        source: source.into(),
        label: m.label.clone(),
        created_ms: m.created_ms,
        encrypted: m.encrypted,
        options: m.options,
        sites,
        problems,
    })
}

// ------------------------------------------------------------------------ import

/// Imports a bundle. `name` is the base name for a renamed import; blank means "pick a free
/// one". Every site is reported, and one that fails never stops the others.
pub fn import_bundle(
    inner: &Inner,
    h: &TaskHandle,
    source: &str,
    password: Option<&str>,
    on_conflict: OnConflict,
    name: Option<&str>,
    options: crate::snapshots::RestoreOptions,
) -> Result<BundleImportResult, CoreError> {
    let manifest = read_manifest(source)?;
    let cipher = manifest.cipher(password)?;
    let src = PathBuf::from(source);
    // The bar counts sites, and only the manifest knows how many there are.
    h.set_total(manifest.sites.len());
    let mut result = BundleImportResult {
        unlocked: cipher.is_some(),
        sites: Vec::new(),
    };
    for site in &manifest.sites {
        // Between sites: a site already being written is finished, so stopping never leaves
        // half a project registered.
        if h.cancel_requested() {
            return Err(h.check_cancelled().unwrap_err());
        }
        h.begin_step(&format!("Importing {}", site.project_name));
        let outcome = import_site(
            inner,
            h,
            &manifest,
            &src,
            site,
            cipher.as_ref(),
            on_conflict,
            name,
            &options,
        );
        let r = match outcome {
            Ok(r) => r,
            Err(e) => BundleSiteResult {
                project_name: site.project_name.clone(),
                action: "skipped".into(),
                hostnames: site.hostnames.clone(),
                safety_snapshot: None,
                changes: Vec::new(),
                problems: vec![e.to_string()],
            },
        };
        for c in r.changes.clone() {
            h.result(format!("{}: {c}", r.project_name));
        }
        for p in r.problems.clone() {
            h.problem(format!("{}: {p}", r.project_name));
        }
        result.sites.push(r);
        h.advance(1);
    }
    Ok(result)
}

#[allow(clippy::too_many_arguments)]
fn import_site(
    inner: &Inner,
    h: &TaskHandle,
    manifest: &BundleManifest,
    src: &Path,
    site: &BundleSite,
    cipher: Option<&BundleCipher>,
    on_conflict: OnConflict,
    name: Option<&str>,
    options: &crate::snapshots::RestoreOptions,
) -> Result<BundleSiteResult, CoreError> {
    let content = manifest.content_of(site, cipher, src)?;
    let existing: Vec<String> = inner
        .domains
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .list()
        .into_iter()
        .filter(|d| site.hostnames.contains(&d.hostname))
        .map(|d| d.hostname)
        .collect();
    if options.config && on_conflict == OnConflict::Update && !existing.is_empty() {
        update_existing(inner, content, cipher, src, &existing, options)
    } else {
        create_new(inner, h, src, site, content, cipher, name, options)
    }
}

/// Update mode: a snapshot of what is there now, then the bundle's version on top.
fn update_existing(
    inner: &Inner,
    content: SnapshotContent,
    cipher: Option<&BundleCipher>,
    src: &Path,
    existing: &[String],
    options: &crate::snapshots::RestoreOptions,
) -> Result<BundleSiteResult, CoreError> {
    // The project that owns the first colliding site is the one being updated.
    let owner = inner
        .domains
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&existing[0])
        .and_then(|d| d.project_id.clone());
    let Some(project_id) = owner else {
        return Err(err(format!(
            "{} already exists but belongs to no project, so it cannot be updated",
            existing[0]
        )));
    };
    // §165: the current configuration, snapshotted before anything is replaced.
    let safety = inner.create_snapshot(
        &project_id,
        "Before import",
        SnapshotOptions {
            env: true,
            databases: false,
            files: false,
        },
    )?;
    let project = inner
        .projects
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&project_id)
        .ok_or_else(|| err("the project behind that site is not registered"))?;

    let mut changes = vec![format!("snapshot {} taken first", safety.id)];
    let mut problems = Vec::new();
    if options.config {
        inner.apply_config(
            &content,
            &project,
            &Adjust::none(),
            &mut changes,
            &mut problems,
        );
    }
    if options.env {
        for (name, text) in &content.env_files {
            match inner.env_write(&project_id, name, text) {
                Ok(_) => changes.push(format!("{name} written")),
                Err(e) => problems.push(format!("{name}: {e}")),
            }
        }
    }
    if options.databases {
        let dumps: Vec<DatabaseMeta> = content
            .databases
            .iter()
            .filter(|d| d.dump.is_some())
            .cloned()
            .collect();
        inner.restore_dumps(src, &dumps, cipher, None, &mut changes, &mut problems);
    }
    Ok(BundleSiteResult {
        project_name: project.name.clone(),
        action: "updated".into(),
        hostnames: existing.to_vec(),
        safety_snapshot: Some(safety.id),
        changes,
        problems,
    })
}

/// Rename mode, and everything with no collision: a new project folder, with every name the
/// collision would have made ambiguous adjusted to something free.
#[allow(clippy::too_many_arguments)]
fn create_new(
    inner: &Inner,
    h: &TaskHandle,
    src: &Path,
    site: &BundleSite,
    content: SnapshotContent,
    cipher: Option<&BundleCipher>,
    name: Option<&str>,
    options: &crate::snapshots::RestoreOptions,
) -> Result<BundleSiteResult, CoreError> {
    let base = name
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .unwrap_or(site.project_name.as_str())
        .to_string();
    let project_name = inner.free_project_name(&base);
    let parent = inner.paths.sites_dir();
    let slug = crate::domain::slugify(&project_name);
    if slug.is_empty() {
        return Err(err(format!(
            "\"{project_name}\" cannot be a folder name; choose another name for this import"
        )));
    }
    let target = parent.join(&slug);
    std::fs::create_dir_all(&parent)?;

    let mut changes = Vec::new();
    let mut problems = Vec::new();
    if options.config && site.file_count > 0 {
        if target.exists()
            && std::fs::read_dir(&target)
                .map(|mut d| d.next().is_some())
                .unwrap_or(false)
        {
            return Err(err(format!(
                "{} is not empty; choose another name for this import",
                target.display()
            )));
        }
        match inner.extract_files(src, &target, &format!("{}/files", site.dir)) {
            Ok(n) => {
                changes.push(format!("{n} project file(s) unpacked"));
                h.result(format!("{n} file(s) unpacked into {}", target.display()));
            }
            Err(e) => problems.push(format!("project files: {e}")),
        }
    } else {
        std::fs::create_dir_all(&target)?;
    }

    // Registered from the folder, then renamed: the same order a clone uses, so the project's
    // id is derived from where it actually lives.
    let project = inner
        .projects
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .register(&target.display().to_string())?;
    let project = if project.name == project_name {
        project
    } else {
        inner
            .projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .rename(&project.id, &project_name)?
    };
    let adjust = Adjust::new(&content, &project.name, &target, inner);
    changes.extend(adjust.describe());

    // Only what the bundle actually holds is applied.
    let mut wanted = content.clone();
    if !options.config {
        wanted.domains.clear();
        wanted.web_configs.clear();
        wanted.workers.clear();
        wanted.schedules.clear();
        wanted.tunnels.clear();
        wanted.manifest_files.clear();
        wanted.quick_commands.clear();
        wanted.mode = None;
    }
    if !options.env {
        wanted.env_files.clear();
    }
    if !options.databases {
        wanted.databases.clear();
    }
    let mut done = Vec::new();
    inner.apply_config(&wanted, &project, &adjust, &mut done, &mut problems);
    changes.extend(done);
    if options.env {
        for (file, text) in &wanted.env_files {
            match std::fs::write(target.join(file), adjust.env(text)) {
                Ok(()) => changes.push(format!("{file} written")),
                Err(e) => problems.push(format!("{file}: {e}")),
            }
        }
    }
    if options.databases {
        let rename: BTreeMap<String, String> = wanted
            .databases
            .iter()
            .map(|d| (d.name.clone(), adjust.db(&d.name)))
            .collect();
        inner.restore_dumps(
            src,
            &wanted.databases,
            cipher,
            Some(&rename),
            &mut changes,
            &mut problems,
        );
    }
    let _ = inner.sync_auto_domains();
    let hostnames: Vec<String> = content
        .domains
        .iter()
        .map(|d| adjust.host(&d.hostname))
        .collect();
    Ok(BundleSiteResult {
        project_name,
        action: "created".into(),
        hostnames,
        safety_snapshot: None,
        changes,
        problems,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_selection_is_configuration_without_data() {
        let d = BundleOptions::default();
        assert!(
            d.settings && d.env,
            "settings and .env files are the small part"
        );
        assert!(!d.databases && !d.files, "data is asked for, not assumed");
        assert!(!d.is_empty());
        assert!(BundleOptions {
            settings: false,
            env: false,
            databases: false,
            files: false
        }
        .is_empty());
    }

    #[test]
    fn a_bundle_name_never_overwrites_an_existing_file() {
        let dir = std::env::temp_dir().join(format!("ols-bundle-{}", now_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = bundle_path(&dir.display().to_string(), "shop").unwrap();
        std::fs::write(&a, b"x").unwrap();
        let b = bundle_path(&dir.display().to_string(), "shop").unwrap();
        assert_ne!(a, b, "a second export must not clobber the first");
        assert!(a
            .file_name()
            .unwrap()
            .to_string_lossy()
            .ends_with(EXTENSION));
        // A label that slugifies to nothing still produces a usable name.
        let c = bundle_path(&dir.display().to_string(), "///").unwrap();
        assert!(c
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .starts_with("sites-"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_relative_destination_is_refused() {
        let e = bundle_path("out", "shop").unwrap_err();
        assert!(
            e.to_string().contains("full folder path"),
            "the message says what to do: {e}"
        );
    }

    #[test]
    fn a_bundle_needs_at_least_one_thing_in_it() {
        let home = crate::test_support::isolated_home();
        let core = crate::command::Core::new(
            crate::settings::SettingsService::load(&home.paths).unwrap(),
            home.paths.clone(),
        );
        let inner = core.inner().clone();
        let manager = crate::tasks::TaskManager::new();
        manager.start("export_sites", "Nothing", "", 1, move |h| {
            let e = export(
                &inner,
                &h,
                &["shop.test".to_string()],
                BundleOptions {
                    settings: false,
                    env: false,
                    databases: false,
                    files: false,
                },
                "C:/out",
                None,
            )
            .unwrap_err()
            .to_string();
            assert!(e.contains("nothing was selected"), "{e}");
            assert!(e.contains("settings"), "the message lists the choices: {e}");
            h.succeed();
        });
        let _ = home;
    }

    #[test]
    fn the_sealed_suffix_marks_only_sealed_entries() {
        assert_eq!(sealed_suffix(false), "");
        assert_eq!(sealed_suffix(true), ".enc");
    }
}
