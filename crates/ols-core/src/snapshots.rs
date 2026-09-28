//! Snapshots (§131), backups (§130), environment import / export (§132) and environment
//! cloning (§158).
//!
//! A **snapshot** is one zip file holding a project's configuration: its manifest files,
//! runtime selection, sites (and hand-edited web configs), certificate and database
//! metadata, workers, scheduled tasks, tunnels, Quick Commands and mode. Its `.env` files,
//! database dumps and source files are optional (they can hold secrets or be large).
//! Snapshots live under `data/snapshots/<project>/`; exporting one is copying that file,
//! and importing one reviews it first, then creates a project from it (the same code
//! as cloning, which adjusts names, domains, databases and ports so two copies can run
//! side by side).
//!
//! A **settings backup** is a zip of the app's own configuration (not runtimes, caches or
//! logs). Restoring anything first saves what it's about to replace.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::app::Inner;
use crate::domain::{Domain, Ownership, SiteKind};
use crate::error::CoreError;
use crate::manifest;
use crate::project::Project;
use crate::quickapp::commands::QuickCommand;
use crate::web::manager::ConfigPart;

const FORMAT: u32 = 1;
const META: &str = "snapshot.json";

/// Folders never copied or zipped with a project's files: they are rebuilt by installs.
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "vendor",
    ".venv",
    "venv",
    "__pycache__",
    ".next",
    ".nuxt",
    "target",
];

fn err(msg: impl Into<String>) -> CoreError {
    CoreError::EnvError(msg.into())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotOptions {
    /// `.env` files (they usually hold passwords and keys).
    #[serde(default)]
    pub env: bool,
    /// SQL dumps of the project's MariaDB / PostgreSQL database.
    #[serde(default)]
    pub databases: bool,
    /// The project's own files (without node_modules, vendor and the like).
    #[serde(default)]
    pub files: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DatabaseMeta {
    pub engine: String,
    pub name: String,
    /// The dump's name inside the snapshot, when data was included.
    #[serde(default)]
    pub dump: Option<String>,
}

/// Everything a snapshot records, stored as `snapshot.json` inside the zip.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotContent {
    pub format: u32,
    pub app_version: String,
    pub created_ms: u64,
    pub label: String,
    pub options: SnapshotOptions,
    pub project: Project,
    /// Every file in `.openlocalserver/`, by name.
    pub manifest_files: BTreeMap<String, String>,
    /// The runtime versions the project resolved to.
    pub runtimes: BTreeMap<String, String>,
    pub domains: Vec<Domain>,
    /// Hand-owned web config text per site: (site file, custom snippet).
    #[serde(default)]
    pub web_configs: BTreeMap<String, (Option<String>, Option<String>)>,
    pub certificates: Vec<crate::certs::CertInfo>,
    pub databases: Vec<DatabaseMeta>,
    #[serde(default)]
    pub sqlite: Vec<String>,
    pub workers: Vec<crate::workers::Worker>,
    pub schedules: Vec<crate::scheduler::ScheduledTask>,
    #[serde(default)]
    pub tunnels: Vec<crate::tunnel::TunnelConfig>,
    /// The user's own (non built-in) Quick Commands.
    pub quick_commands: Vec<QuickCommand>,
    pub mode: Option<String>,
    /// `.env*` file name → content, when included.
    #[serde(default)]
    pub env_files: BTreeMap<String, String>,
    /// Number of project files included.
    #[serde(default)]
    pub file_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotInfo {
    /// The file name, which is also its id.
    pub id: String,
    pub project_id: String,
    pub project_name: String,
    pub label: String,
    pub created_ms: u64,
    pub size_bytes: u64,
    pub options: SnapshotOptions,
    pub path: String,
    pub summary: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct RestoreOptions {
    #[serde(default = "yes")]
    pub config: bool,
    #[serde(default)]
    pub env: bool,
    #[serde(default)]
    pub databases: bool,
    #[serde(default)]
    pub files: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RestoreResult {
    /// The snapshot taken of the current state first.
    pub safety_snapshot: Option<String>,
    pub restored: Vec<String>,
    pub problems: Vec<String>,
}

/// What an environment file (or clone) would create, shown before anything is done (§132).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportPreview {
    pub source: String,
    pub content: Box<SnapshotContent>,
    pub summary: Vec<String>,
    /// Suggested new name and the adjustments it implies.
    pub suggested_name: String,
    pub adjustments: Vec<String>,
    pub conflicts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloneResult {
    pub project: Project,
    pub changes: Vec<String>,
    pub problems: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsBackup {
    pub id: String,
    pub path: String,
    pub created_ms: u64,
    pub size_bytes: u64,
}

// ------------------------------------------------------------------------ helpers

fn summary(c: &SnapshotContent) -> Vec<String> {
    let mut s = Vec::new();
    if !c.manifest_files.is_empty() {
        s.push(format!(
            "manifest ({})",
            c.manifest_files
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !c.runtimes.is_empty() {
        s.push(
            c.runtimes
                .iter()
                .map(|(k, v)| format!("{k} {v}"))
                .collect::<Vec<_>>()
                .join(", "),
        );
    }
    if !c.domains.is_empty() {
        s.push(format!(
            "sites: {}",
            c.domains
                .iter()
                .map(|d| d.hostname.clone())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !c.databases.is_empty() {
        s.push(format!(
            "databases: {}",
            c.databases
                .iter()
                .map(|d| format!(
                    "{} ({}{})",
                    d.name,
                    d.engine,
                    if d.dump.is_some() { ", with data" } else { "" }
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !c.workers.is_empty() {
        s.push(format!("{} worker(s)", c.workers.len()));
    }
    if !c.schedules.is_empty() {
        s.push(format!("{} scheduled task(s)", c.schedules.len()));
    }
    if !c.tunnels.is_empty() {
        s.push(format!("{} tunnel(s)", c.tunnels.len()));
    }
    if !c.env_files.is_empty() {
        s.push(format!(
            "env files: {}",
            c.env_files.keys().cloned().collect::<Vec<_>>().join(", ")
        ));
    }
    if c.file_count > 0 {
        s.push(format!("{} project file(s)", c.file_count));
    }
    s
}

fn zip_options() -> zip::write::SimpleFileOptions {
    zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .large_file(true)
}

/// Project files relative to `root`, skipping rebuildable folders and `.git`.
fn project_files(root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                if name == ".git" || SKIP_DIRS.contains(&name.as_str()) {
                    continue;
                }
                walk(&e.path(), root, out);
            } else if ft.is_file() {
                if let Ok(rel) = e.path().strip_prefix(root) {
                    out.push(rel.to_path_buf());
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out
}

/// Copies a project folder for a full clone (keeps `.git`, skips rebuildable folders).
fn copy_project(from: &Path, to: &Path) -> std::io::Result<usize> {
    let mut n = 0;
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)?.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let target = to.join(e.file_name());
        if e.file_type()?.is_dir() {
            if SKIP_DIRS.contains(&name.as_str()) {
                continue;
            }
            n += copy_project(&e.path(), &target)?;
        } else {
            std::fs::copy(e.path(), target)?;
            n += 1;
        }
    }
    Ok(n)
}

fn read_zip_meta(path: &Path) -> Result<SnapshotContent, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| format!("{} is not a snapshot: {e}", path.display()))?;
    let mut entry = zip
        .by_name(META)
        .map_err(|_| format!("{} is not an OpenLocalServer snapshot", path.display()))?;
    let mut text = String::new();
    entry.read_to_string(&mut text).map_err(|e| e.to_string())?;
    let c: SnapshotContent = serde_json::from_str(&text)
        .map_err(|e| format!("the snapshot's description is damaged: {e}"))?;
    if c.format > FORMAT {
        return Err(format!(
            "this snapshot was made by a newer OpenLocalServer (format {})",
            c.format
        ));
    }
    Ok(c)
}

/// Replaces whole-word occurrences of a slug in a hostname ("shop.test" → "shop-copy.test").
fn rename_host(host: &str, old: &str, new: &str) -> String {
    let labels: Vec<String> = host
        .split('.')
        .map(|l| {
            if l == old {
                new.to_string()
            } else {
                l.to_string()
            }
        })
        .collect();
    let renamed = labels.join(".");
    if renamed == host {
        format!("{new}.{host}")
    } else {
        renamed
    }
}

impl Inner {
    fn snapshots_dir(&self, project_id: &str) -> PathBuf {
        self.paths.data_dir().join("snapshots").join(project_id)
    }

    /// Reads a project's current configuration (no data) into a snapshot description.
    pub fn snapshot_content(
        &self,
        project_id: &str,
        label: &str,
        options: SnapshotOptions,
    ) -> Result<SnapshotContent, CoreError> {
        let project = self
            .projects
            .lock()
            .unwrap()
            .get(project_id)
            .ok_or_else(|| CoreError::InvalidProjectPath(project_id.to_string()))?;
        let root = PathBuf::from(&project.path);
        let mut manifest_files = BTreeMap::new();
        if let Ok(rd) = std::fs::read_dir(manifest::dir(&root)) {
            for e in rd.flatten().filter(|e| e.path().is_file()) {
                if let Ok(text) = std::fs::read_to_string(e.path()) {
                    manifest_files.insert(e.file_name().to_string_lossy().to_string(), text);
                }
            }
        }
        let runtimes = self
            .project_detail(project_id)
            .map(|d| {
                d.resolved
                    .into_iter()
                    .filter_map(|r| r.installed_version.map(|v| (r.id, v)))
                    .collect()
            })
            .unwrap_or_default();
        let domains: Vec<Domain> = self
            .domains
            .lock()
            .unwrap()
            .list()
            .into_iter()
            .filter(|d| d.project_id.as_deref() == Some(project_id))
            .collect();
        let cfg = self.web_config();
        let mut web_configs = BTreeMap::new();
        for d in domains.iter().filter(|d| d.ownership != Ownership::Managed) {
            let server = crate::domain::resolved_server(d, &cfg);
            let site = self
                .web
                .read_config(&server, Some(&d.hostname), ConfigPart::Site)
                .ok();
            let custom = (d.ownership == Ownership::Advanced)
                .then(|| {
                    self.web
                        .read_config(&server, Some(&d.hostname), ConfigPart::Custom)
                        .ok()
                })
                .flatten();
            web_configs.insert(d.hostname.clone(), (site, custom));
        }
        let certificates = domains
            .iter()
            .filter_map(|d| self.certs.info(&d.hostname))
            .collect();
        let mut databases = Vec::new();
        if let Ok(Some(m)) = manifest::read_manifest(&root) {
            if let Some(db) = m.database {
                let engine = match db.engine.as_str() {
                    "mysql" | "mariadb" => "mariadb",
                    "postgresql" => "postgres",
                    e => e,
                };
                if engine != "sqlite" {
                    databases.push(DatabaseMeta {
                        engine: engine.into(),
                        name: db
                            .name
                            .unwrap_or_else(|| crate::setup::db_name_for(&project.name)),
                        dump: None,
                    });
                }
            }
        }
        let sqlite = self
            .sqlite
            .lock()
            .unwrap()
            .list()
            .into_iter()
            .filter(|s| s.project_id.as_deref() == Some(project_id))
            .map(|s| s.path)
            .collect();
        let mut env_files = BTreeMap::new();
        if options.env {
            for f in self.env_files(project_id).unwrap_or_default() {
                if let Ok(text) = std::fs::read_to_string(root.join(&f.name)) {
                    env_files.insert(f.name, text);
                }
            }
        }
        Ok(SnapshotContent {
            format: FORMAT,
            app_version: env!("CARGO_PKG_VERSION").into(),
            created_ms: now_ms(),
            label: label.to_string(),
            options,
            manifest_files,
            runtimes,
            domains,
            web_configs,
            certificates,
            databases,
            sqlite,
            workers: self.workers_for(project_id),
            schedules: self.schedules_for(project_id),
            tunnels: self.tunnels_for(project_id),
            quick_commands: self
                .quick_commands
                .list()
                .into_iter()
                .filter(|c| !c.builtin)
                .collect(),
            mode: self
                .settings
                .lock()
                .unwrap()
                .get(&format!("project.{project_id}.mode"))
                .and_then(|v| v.as_str().map(str::to_string)),
            env_files,
            file_count: 0,
            project,
        })
    }

    /// §131: takes a snapshot and stores it with the project's others.
    pub fn create_snapshot(
        &self,
        project_id: &str,
        label: &str,
        options: SnapshotOptions,
    ) -> Result<SnapshotInfo, CoreError> {
        let mut content = self.snapshot_content(project_id, label, options)?;
        let dir = self.snapshots_dir(project_id);
        std::fs::create_dir_all(&dir)?;
        let slug = crate::domain::slugify(if label.is_empty() { "snapshot" } else { label });
        let file = dir.join(format!(
            "{}-{}.zip",
            content.created_ms,
            if slug.is_empty() {
                "snapshot".into()
            } else {
                slug
            }
        ));
        let tmp = file.with_extension("zip.part");
        let result = (|| -> Result<(), CoreError> {
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&tmp)?);
            if options.databases {
                for db in content.databases.iter_mut() {
                    if db.engine == "mongodb" {
                        continue;
                    }
                    if !self.services.is_running(&db.engine) {
                        self.start_service_and_wait(&db.engine, &mut |_| {})
                            .map_err(err)?;
                    }
                    let dump =
                        crate::dbbackup::backup(&self.services, &self.paths, &db.engine, &db.name)
                            .map_err(err)?;
                    let name = format!("databases/{}-{}.sql", db.engine, db.name);
                    zip.start_file(name.as_str(), zip_options())
                        .map_err(|e| err(e.to_string()))?;
                    std::io::copy(&mut std::fs::File::open(&dump)?, &mut zip)?;
                    db.dump = Some(name);
                }
            }
            if options.files {
                let root = PathBuf::from(&content.project.path);
                for rel in project_files(&root) {
                    let name = format!("files/{}", rel.display().to_string().replace('\\', "/"));
                    zip.start_file(name.as_str(), zip_options())
                        .map_err(|e| err(e.to_string()))?;
                    std::io::copy(&mut std::fs::File::open(root.join(&rel))?, &mut zip)?;
                    content.file_count += 1;
                }
            }
            zip.start_file(META, zip_options())
                .map_err(|e| err(e.to_string()))?;
            zip.write_all(serde_json::to_string_pretty(&content)?.as_bytes())?;
            zip.finish().map_err(|e| err(e.to_string()))?;
            Ok(())
        })();
        if let Err(e) = result {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
        std::fs::rename(&tmp, &file)?;
        tracing::info!(project = %project_id, file = %file.display(), "snapshot taken");
        self.snapshot_info(&file)
    }

    fn snapshot_info(&self, file: &Path) -> Result<SnapshotInfo, CoreError> {
        let c = read_zip_meta(file).map_err(err)?;
        Ok(SnapshotInfo {
            id: file
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            project_id: c.project.id.clone(),
            project_name: c.project.name.clone(),
            label: c.label.clone(),
            created_ms: c.created_ms,
            size_bytes: std::fs::metadata(file).map(|m| m.len()).unwrap_or(0),
            options: c.options,
            path: file.display().to_string(),
            summary: summary(&c),
        })
    }

    pub fn list_snapshots(&self, project_id: &str) -> Vec<SnapshotInfo> {
        let mut out: Vec<SnapshotInfo> = std::fs::read_dir(self.snapshots_dir(project_id))
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "zip"))
            .filter_map(|e| self.snapshot_info(&e.path()).ok())
            .collect();
        out.sort_by_key(|s| std::cmp::Reverse(s.created_ms));
        out
    }

    fn snapshot_file(&self, project_id: &str, id: &str) -> Result<PathBuf, CoreError> {
        if id.contains(['/', '\\']) || id.contains("..") {
            return Err(err("that is not a snapshot name"));
        }
        let file = self.snapshots_dir(project_id).join(id);
        if !file.is_file() {
            return Err(err(format!("snapshot {id} was not found")));
        }
        Ok(file)
    }

    pub fn delete_snapshot(&self, project_id: &str, id: &str) -> Result<(), CoreError> {
        std::fs::remove_file(self.snapshot_file(project_id, id)?)?;
        Ok(())
    }

    /// §132 export: the snapshot file is the environment file.
    pub fn export_snapshot(&self, project_id: &str, id: &str, dest: &str) -> Result<(), CoreError> {
        std::fs::copy(self.snapshot_file(project_id, id)?, dest)?;
        Ok(())
    }

    /// Puts a project back the way a snapshot recorded it. A safety snapshot of the current
    /// state (config and env files) is taken first.
    pub fn restore_snapshot(
        &self,
        project_id: &str,
        id: &str,
        opts: RestoreOptions,
    ) -> Result<RestoreResult, CoreError> {
        let file = self.snapshot_file(project_id, id)?;
        let content = read_zip_meta(&file).map_err(err)?;
        let safety = self.create_snapshot(
            project_id,
            "Before restore",
            SnapshotOptions {
                env: true,
                ..Default::default()
            },
        )?;
        let mut r = RestoreResult {
            safety_snapshot: Some(safety.id),
            ..Default::default()
        };
        let title = format!("Restore a snapshot of {}", content.project.name);
        self.journaled("restore_snapshot", &title, Some("A \"Before restore\" snapshot of the previous state was taken; restore it to undo."), None, || {
            let project = self.projects.lock().unwrap().get(project_id).ok_or_else(|| CoreError::InvalidProjectPath(project_id.to_string()))?;
            let root = PathBuf::from(&project.path);
            if opts.files && content.file_count > 0 {
                match self.extract_files(&file, &root) {
                    Ok(n) => r.restored.push(format!("{n} project file(s)")),
                    Err(e) => r.problems.push(format!("files: {e}")),
                }
            }
            if opts.config {
                self.apply_config(&content, &project, &Adjust::none(), &mut r.restored, &mut r.problems);
            }
            if opts.env {
                for (name, text) in &content.env_files {
                    match self.env_write(project_id, name, text) {
                        Ok(_) => r.restored.push(name.clone()),
                        Err(e) => r.problems.push(format!("{name}: {e}")),
                    }
                }
            }
            if opts.databases {
                self.restore_dumps(&file, &content, None, &mut r.restored, &mut r.problems);
            }
            Ok(())
        })?;
        Ok(r)
    }

    fn extract_files(&self, zip_path: &Path, root: &Path) -> Result<usize, String> {
        let mut zip =
            zip::ZipArchive::new(std::fs::File::open(zip_path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        let mut n = 0;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
            let Some(rel) = entry.enclosed_name() else {
                continue;
            };
            let Ok(rel) = rel.strip_prefix("files") else {
                continue;
            };
            if rel.as_os_str().is_empty() || entry.is_dir() {
                continue;
            }
            let target = root.join(rel);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut out =
                std::fs::File::create(&target).map_err(|e| format!("{}: {e}", target.display()))?;
            std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
            n += 1;
        }
        Ok(n)
    }

    /// Loads each included dump; `rename` maps old database names to new ones (clones).
    fn restore_dumps(
        &self,
        zip_path: &Path,
        content: &SnapshotContent,
        rename: Option<&BTreeMap<String, String>>,
        done: &mut Vec<String>,
        problems: &mut Vec<String>,
    ) {
        let Ok(f) = std::fs::File::open(zip_path) else {
            return;
        };
        let Ok(mut zip) = zip::ZipArchive::new(f) else {
            return;
        };
        for db in &content.databases {
            let Some(dump) = &db.dump else { continue };
            let target = rename
                .and_then(|m| m.get(&db.name))
                .cloned()
                .unwrap_or_else(|| db.name.clone());
            let result = (|| -> Result<(), String> {
                let mut entry = zip.by_name(dump).map_err(|e| e.to_string())?;
                let tmp = self
                    .paths
                    .cache_dir()
                    .join(format!("restore-{}-{}.sql", db.engine, target));
                std::fs::create_dir_all(self.paths.cache_dir()).map_err(|e| e.to_string())?;
                std::io::copy(
                    &mut entry,
                    &mut std::fs::File::create(&tmp).map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
                if !self.services.is_running(&db.engine) {
                    self.start_service_and_wait(&db.engine, &mut |_| {})?;
                }
                self.services.create_database(&db.engine, &target)?;
                let r = crate::dbbackup::restore(
                    &self.services,
                    &self.paths,
                    &db.engine,
                    &target,
                    &tmp,
                )
                .map(|_| ());
                let _ = std::fs::remove_file(&tmp);
                r
            })();
            match result {
                Ok(()) => done.push(format!("database {target} ({})", db.engine)),
                Err(e) => problems.push(format!("database {target}: {e}")),
            }
        }
    }

    /// Writes a snapshot's configuration onto `project`, adjusted for a clone when asked.
    fn apply_config(
        &self,
        c: &SnapshotContent,
        project: &Project,
        adjust: &Adjust,
        done: &mut Vec<String>,
        problems: &mut Vec<String>,
    ) {
        let root = PathBuf::from(&project.path);
        // Manifest files, with the manifest's name / site / database adjusted.
        for (name, text) in &c.manifest_files {
            let text = if name == "environment.yaml" && !adjust.is_none() {
                match serde_yaml_ng::from_str::<manifest::EnvironmentManifest>(text) {
                    Ok(mut m) => {
                        m.name = Some(project.name.clone());
                        if let Some(d) = m.domain.as_mut() {
                            d.hostname = adjust.host(&d.hostname);
                            if let Some(p) = d.port {
                                d.port = Some(adjust.port(p));
                            }
                        }
                        if let Some(db) = m.database.as_mut() {
                            db.name = db
                                .name
                                .as_ref()
                                .map(|n| adjust.db(n))
                                .or_else(|| Some(crate::setup::db_name_for(&project.name)));
                        }
                        serde_yaml_ng::to_string(&m).unwrap_or_else(|_| text.clone())
                    }
                    Err(_) => text.clone(),
                }
            } else {
                text.clone()
            };
            let path = manifest::dir(&root).join(name);
            if let Err(e) = std::fs::create_dir_all(manifest::dir(&root))
                .and_then(|_| std::fs::write(&path, text))
            {
                problems.push(format!("{name}: {e}"));
            }
        }
        if !c.manifest_files.is_empty() {
            done.push(format!("{} manifest file(s)", c.manifest_files.len()));
        }

        // Sites: the snapshot's set replaces the project's (restores) or is added (clones).
        if adjust.is_none() {
            let keep: Vec<&str> = c.domains.iter().map(|d| d.hostname.as_str()).collect();
            let extra: Vec<String> = self
                .domains
                .lock()
                .unwrap()
                .list()
                .into_iter()
                .filter(|d| {
                    d.project_id.as_deref() == Some(project.id.as_str())
                        && !keep.contains(&d.hostname.as_str())
                })
                .map(|d| d.hostname)
                .collect();
            for h in extra {
                match self.remove_domain(&h) {
                    Ok(()) => done.push(format!("removed site {h} (not in the snapshot)")),
                    Err(e) => problems.push(format!("{h}: {e}")),
                }
            }
        }
        for d in &c.domains {
            let mut d = d.clone();
            d.project_id = Some(project.id.clone());
            if !adjust.is_none() {
                d.hostname = adjust.host(&d.hostname);
                d.root = adjust.path(&d.root);
                d.generated_hashes.clear();
                if let SiteKind::Proxy {
                    upstream_port,
                    upstream_host: None,
                    ..
                } = &mut d.kind
                {
                    *upstream_port = adjust.port(*upstream_port);
                }
                if let Some(app) = d.app.as_mut() {
                    app.cwd = adjust.path(&app.cwd);
                }
            }
            let exists = self.domains.lock().unwrap().get(&d.hostname);
            let host = d.hostname.clone();
            let r = match exists {
                Some(e) if e.project_id.as_deref().is_some_and(|p| p != project.id) => {
                    Err(format!("{host} belongs to another project"))
                }
                Some(_) => self.update_domain(d).map(|_| ()).map_err(|e| e.to_string()),
                None => self.add_domain(d).map(|_| ()).map_err(|e| e.to_string()),
            };
            match r {
                Ok(()) => done.push(format!("site {host}")),
                Err(e) => problems.push(e),
            }
            if let Some((site, custom)) = c.web_configs.get(&adjust_back(adjust, &host, c)) {
                if let Some(text) = site {
                    if let Err(e) = self.write_web_config(&host, ConfigPart::Site, text) {
                        problems.push(format!("{host} web config: {e}"));
                    }
                }
                if let Some(text) = custom {
                    if let Err(e) = self.write_web_config(&host, ConfigPart::Custom, text) {
                        problems.push(format!("{host} custom config: {e}"));
                    }
                }
            }
        }

        // Workers and scheduled tasks: replace the project's.
        for w in self.workers_for(&project.id) {
            let _ = self.remove_worker(&w.id);
        }
        for mut w in c.workers.clone() {
            w.id = crate::workers::worker_id(&project.id, &w.name);
            w.project_id = project.id.clone();
            match self.save_worker(w) {
                Ok(w) => done.push(format!("worker {}", w.name)),
                Err(e) => problems.push(e.to_string()),
            }
        }
        for t in self.schedules_for(&project.id) {
            let _ = self.remove_schedule(&t.id);
        }
        for mut t in c.schedules.clone() {
            t.id = crate::workers::worker_id(&project.id, &t.name);
            t.project_id = Some(project.id.clone());
            match self.save_schedule(t) {
                Ok(t) => done.push(format!("scheduled task {}", t.name)),
                Err(e) => problems.push(e.to_string()),
            }
        }
        for t in &c.tunnels {
            let mut t = t.clone();
            t.project_id = Some(project.id.clone());
            if !adjust.is_none() {
                t.id = String::new();
                t.target = adjust.host_in_url(&t.target);
            }
            match self.save_tunnel(t) {
                Ok(t) => done.push(format!("tunnel {}", t.name)),
                Err(e) => problems.push(e.to_string()),
            }
        }
        let known = self.quick_commands.list();
        for q in &c.quick_commands {
            if !known.iter().any(|k| k.id == q.id) {
                if let Err(e) = self.quick_commands.save(q.clone()) {
                    problems.push(e.to_string());
                }
            }
        }
        if let Some(mode) = &c.mode {
            let _ = self.settings.lock().unwrap().set(
                format!("project.{}.mode", project.id),
                serde_json::json!(mode),
            );
        }
    }

    // ------------------------------------------------------------- import / clone

    /// Reads an environment file for review before anything is created (§132).
    pub fn preview_import(&self, source: &str) -> Result<ImportPreview, CoreError> {
        let content = read_zip_meta(Path::new(source)).map_err(err)?;
        let suggested = self.free_project_name(&content.project.name);
        let adjust = Adjust::new(
            &content,
            &suggested,
            &PathBuf::from(&content.project.path).with_file_name(&suggested),
            self,
        );
        let mut conflicts = Vec::new();
        for d in &content.domains {
            let h = adjust.host(&d.hostname);
            if self.domains.lock().unwrap().get(&h).is_some() {
                conflicts.push(format!("the site {h} already exists"));
            }
        }
        if content.databases.iter().any(|d| d.dump.is_some()) {
            conflicts
                .push("database data is included and will be loaded into new databases".into());
        }
        if content.options.env {
            conflicts.push(".env files are included; they may hold passwords and keys".into());
        }
        Ok(ImportPreview {
            source: source.into(),
            summary: summary(&content),
            adjustments: adjust.describe(),
            suggested_name: suggested,
            conflicts,
            content: Box::new(content),
        })
    }

    fn free_project_name(&self, base: &str) -> String {
        let projects = self.projects.lock().unwrap().list();
        let taken = |n: &str| {
            projects.iter().any(|p| p.name.eq_ignore_ascii_case(n))
                || self
                    .domains
                    .lock()
                    .unwrap()
                    .get(&format!("{}.test", crate::domain::slugify(n)))
                    .is_some()
        };
        if !taken(base) {
            return base.to_string();
        }
        (2..)
            .map(|i| format!("{base}-{i}"))
            .find(|n| !taken(n))
            .unwrap()
    }

    /// §132 import: creates a project from an environment file at `target`.
    pub fn import_environment(
        &self,
        source: &str,
        target: &str,
        name: &str,
    ) -> Result<CloneResult, CoreError> {
        let zip_path = PathBuf::from(source);
        let content = read_zip_meta(&zip_path).map_err(err)?;
        let target = PathBuf::from(target);
        if !target.is_absolute() {
            return Err(err("choose a full folder path for the project"));
        }
        let mut changes = Vec::new();
        let mut problems = Vec::new();
        if content.file_count > 0 {
            if target.exists()
                && std::fs::read_dir(&target)
                    .map(|mut d| d.next().is_some())
                    .unwrap_or(false)
            {
                return Err(err(format!(
                    "{} is not empty; choose an empty or new folder",
                    target.display()
                )));
            }
            let n = self.extract_files(&zip_path, &target).map_err(err)?;
            changes.push(format!("{n} project file(s) unpacked"));
        } else {
            std::fs::create_dir_all(&target)?;
        }
        self.finish_clone(
            &content,
            Some(&zip_path),
            &target,
            name,
            true,
            changes,
            &mut problems,
        )
    }

    /// §158: another copy of a project's environment. `what` is `full` (files, config,
    /// database copies), `infrastructure` (sites, services, workers, empty databases for an
    /// existing folder) or `configuration` (manifest and .env files only).
    pub fn clone_environment(
        &self,
        project_id: &str,
        target: &str,
        name: &str,
        what: &str,
    ) -> Result<CloneResult, CoreError> {
        let source = self
            .projects
            .lock()
            .unwrap()
            .get(project_id)
            .ok_or_else(|| CoreError::InvalidProjectPath(project_id.to_string()))?;
        let target = PathBuf::from(target);
        if !target.is_absolute() {
            return Err(err("choose a full folder path for the copy"));
        }
        if target == Path::new(&source.path) {
            return Err(err("the copy needs its own folder"));
        }
        // Database data is copied only when its server is installed; otherwise the clone
        // still goes ahead and says which databases it couldn't copy.
        let dbs = self
            .snapshot_content(project_id, "", SnapshotOptions::default())?
            .databases;
        let missing: Vec<String> = dbs
            .iter()
            .filter(|d| d.engine != "mongodb" && !self.services.status(&d.engine).installed)
            .map(|d| format!("{} ({})", d.name, d.engine))
            .collect();
        let with_db = what == "full" && missing.is_empty() && !dbs.is_empty();
        let title = format!("Clone {} to {name}", source.name);
        self.journaled("clone_environment", &title, Some("Remove the new project and its sites to undo; the original is not changed."), None, || {
            let mut changes = Vec::new();
            let mut problems = Vec::new();
            match what {
                "full" => {
                    if target.exists() && std::fs::read_dir(&target).map(|mut d| d.next().is_some()).unwrap_or(false) {
                        return Err(err(format!("{} is not empty", target.display())));
                    }
                    let n = copy_project(Path::new(&source.path), &target)?;
                    changes.push(format!("{n} file(s) copied (node_modules, vendor and virtual environments are left out: install them again)"));
                }
                "infrastructure" | "configuration" => std::fs::create_dir_all(&target)?,
                other => return Err(err(format!("unknown clone type \"{other}\""))),
            }
            // A snapshot of the source in memory (and on disk, for its database dumps).
            let snap = self.create_snapshot(project_id, &format!("Clone to {name}"), SnapshotOptions { env: true, databases: with_db, files: false })?;
            let zip_path = PathBuf::from(&snap.path);
            let mut content = read_zip_meta(&zip_path).map_err(err)?;
            if what == "configuration" {
                content.domains.clear();
                content.workers.clear();
                content.schedules.clear();
                content.tunnels.clear();
                content.databases.clear();
            }
            if what == "full" && !missing.is_empty() {
                problems.push(format!("database data was not copied because its server isn't installed: {}", missing.join(", ")));
            }
            let r = self.finish_clone(&content, with_db.then_some(zip_path.as_path()), &target, name, what != "configuration", changes, &mut problems);
            r.map(|mut res| {
                if what == "infrastructure" {
                    // Empty databases for the new copy.
                    for db in &content.databases {
                        let new = crate::setup::db_name_for(&res.project.name);
                        if !self.services.is_running(&db.engine) {
                            res.problems.push(format!("database {new} was not created: {} isn't running (ols setup creates it later)", db.engine));
                        } else if let Err(e) = self.services.create_database(&db.engine, &new) {
                            res.problems.push(format!("database {new}: {e}"));
                        } else {
                            res.changes.push(format!("empty database {new} created"));
                        }
                    }
                }
                res
            })
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn finish_clone(
        &self,
        content: &SnapshotContent,
        zip_with_dumps: Option<&Path>,
        target: &Path,
        name: &str,
        infra: bool,
        mut changes: Vec<String>,
        problems: &mut Vec<String>,
    ) -> Result<CloneResult, CoreError> {
        let mut project = self
            .projects
            .lock()
            .unwrap()
            .register(&target.display().to_string())?;
        if !name.trim().is_empty() && project.name != name.trim() {
            project = self
                .projects
                .lock()
                .unwrap()
                .rename(&project.id, name.trim())?;
        }
        let adjust = Adjust::new(content, &project.name, target, self);
        changes.extend(adjust.describe());

        // .env files, adjusted for the new name, site, database and port.
        for (file, text) in &content.env_files {
            let text = adjust.env(text);
            match std::fs::write(target.join(file), text) {
                Ok(()) => changes.push(format!("{file} written")),
                Err(e) => problems.push(format!("{file}: {e}")),
            }
        }
        let mut c = content.clone();
        if !infra {
            c.domains.clear();
        }
        let mut done = Vec::new();
        self.apply_config(&c, &project, &adjust, &mut done, problems);
        changes.extend(done);
        if let Some(zip) = zip_with_dumps {
            let rename: BTreeMap<String, String> = content
                .databases
                .iter()
                .map(|d| (d.name.clone(), adjust.db(&d.name)))
                .collect();
            self.restore_dumps(zip, content, Some(&rename), &mut changes, problems);
        }
        let _ = self.sync_auto_domains();
        Ok(CloneResult {
            project,
            changes,
            problems: std::mem::take(problems),
        })
    }

    // ------------------------------------------------------------- settings backups

    fn settings_backups_dir(&self) -> PathBuf {
        self.paths.backups_dir().join("settings")
    }

    /// §130: a zip of the app's own configuration. Certificates keep only their metadata
    /// (the CA's private key never leaves its folder in a backup).
    pub fn backup_settings(&self) -> Result<SettingsBackup, CoreError> {
        let dir = self.settings_backups_dir();
        std::fs::create_dir_all(&dir)?;
        let created = now_ms();
        // Same-ms backups must not overwrite each other (restore takes a safety
        // backup first; fast tests hit the same millisecond).
        let mut seq = 0u32;
        let file = loop {
            let candidate = if seq == 0 {
                dir.join(format!("settings-{created}.zip"))
            } else {
                dir.join(format!("settings-{created}-{seq}.zip"))
            };
            if !candidate.exists() {
                break candidate;
            }
            seq += 1;
        };
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&file)?);
        let root = self.paths.data_dir();
        // Checkpoint so app.db is complete on disk before zipping.
        if let Ok(conn) = rusqlite::Connection::open(root.join("app.db")) {
            let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
        }
        for e in std::fs::read_dir(&root)?.flatten() {
            let path = e.path();
            let is_cfg =
                path.is_file() && path.extension().is_some_and(|x| x == "json" || x == "db");
            if is_cfg {
                zip.start_file(e.file_name().to_string_lossy().as_ref(), zip_options())
                    .map_err(|e| err(e.to_string()))?;
                std::io::copy(&mut std::fs::File::open(&path)?, &mut zip)?;
            }
        }
        for sub in ["profiles", "quick-commands", "quick-apps"] {
            let base = root.join(sub);
            for rel in project_files(&base) {
                let name = format!("{sub}/{}", rel.display().to_string().replace('\\', "/"));
                zip.start_file(name.as_str(), zip_options())
                    .map_err(|e| err(e.to_string()))?;
                std::io::copy(&mut std::fs::File::open(base.join(&rel))?, &mut zip)?;
            }
        }
        zip.start_file("certificates.json", zip_options())
            .map_err(|e| err(e.to_string()))?;
        zip.write_all(serde_json::to_string_pretty(&self.certs.list())?.as_bytes())?;
        zip.finish().map_err(|e| err(e.to_string()))?;
        Ok(SettingsBackup {
            id: file.file_name().unwrap().to_string_lossy().to_string(),
            path: file.display().to_string(),
            created_ms: created,
            size_bytes: std::fs::metadata(&file).map(|m| m.len()).unwrap_or(0),
        })
    }

    pub fn list_settings_backups(&self) -> Vec<SettingsBackup> {
        let mut out: Vec<SettingsBackup> = std::fs::read_dir(self.settings_backups_dir())
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "zip"))
            .map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                let created_ms = name
                    .trim_start_matches("settings-")
                    .trim_end_matches(".zip")
                    .parse()
                    .unwrap_or(0);
                SettingsBackup {
                    path: e.path().display().to_string(),
                    size_bytes: e.metadata().map(|m| m.len()).unwrap_or(0),
                    id: name,
                    created_ms,
                }
            })
            .collect();
        out.sort_by_key(|b| std::cmp::Reverse(b.created_ms));
        out
    }

    /// Puts a settings backup back (after backing up the current settings). The app must
    /// be restarted to load it, since every store read its file at start.
    pub fn restore_settings(&self, id: &str) -> Result<SettingsBackup, CoreError> {
        if id.contains(['/', '\\']) || id.contains("..") {
            return Err(err("that is not a backup name"));
        }
        let file = self.settings_backups_dir().join(id);
        let mut zip =
            zip::ZipArchive::new(std::fs::File::open(&file)?).map_err(|e| err(e.to_string()))?;
        let safety = self.backup_settings()?;
        let root = self.paths.data_dir();
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(|e| err(e.to_string()))?;
            let Some(rel) = entry.enclosed_name() else {
                continue;
            };
            if entry.is_dir() || rel == Path::new("certificates.json") {
                continue;
            }
            let target = root.join(rel);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::io::copy(&mut entry, &mut std::fs::File::create(&target)?)?;
        }
        // Drop WAL sidecars so reopened app.db recovers cleanly from restored main file.
        let _ = std::fs::remove_file(root.join("app.db-wal"));
        let _ = std::fs::remove_file(root.join("app.db-shm"));
        Ok(safety)
    }
}

/// Maps a web-config key from the clone's new hostname back to the snapshot's old one.
fn adjust_back(adjust: &Adjust, new_host: &str, c: &SnapshotContent) -> String {
    c.domains
        .iter()
        .map(|d| d.hostname.clone())
        .find(|h| adjust.host(h) == new_host)
        .unwrap_or_else(|| new_host.to_string())
}

/// How a clone differs from its source (§158): name, paths, sites, databases, ports.
struct Adjust {
    old_slug: String,
    new_slug: String,
    old_path: String,
    new_path: String,
    dbs: BTreeMap<String, String>,
    ports: BTreeMap<u16, u16>,
    hosts: BTreeMap<String, String>,
}

impl Adjust {
    fn none() -> Self {
        Self {
            old_slug: String::new(),
            new_slug: String::new(),
            old_path: String::new(),
            new_path: String::new(),
            dbs: BTreeMap::new(),
            ports: BTreeMap::new(),
            hosts: BTreeMap::new(),
        }
    }

    fn is_none(&self) -> bool {
        self.new_slug.is_empty()
    }

    fn new(c: &SnapshotContent, new_name: &str, new_path: &Path, inner: &Inner) -> Self {
        let old_slug = crate::domain::slugify(&c.project.name);
        let new_slug = crate::domain::slugify(new_name);
        let dbs = c
            .databases
            .iter()
            .map(|d| {
                (
                    d.name.clone(),
                    crate::setup::db_name_for(&if d.name
                        == crate::setup::db_name_for(&c.project.name)
                    {
                        new_name.to_string()
                    } else {
                        format!("{}_{}", d.name, new_slug)
                    }),
                )
            })
            .collect();
        // A proxied app gets a port nothing else uses.
        let mut used: Vec<u16> = inner
            .domains
            .lock()
            .unwrap()
            .list()
            .into_iter()
            .filter_map(|d| match d.kind {
                SiteKind::Proxy {
                    upstream_port,
                    upstream_host: None,
                    ..
                } => Some(upstream_port),
                _ => None,
            })
            .collect();
        let mut ports = BTreeMap::new();
        for d in &c.domains {
            if let SiteKind::Proxy {
                upstream_port,
                upstream_host: None,
                ..
            } = d.kind
            {
                let mut p = upstream_port + 1;
                while used.contains(&p) || !crate::port::port_is_free(p) {
                    p += 1;
                }
                used.push(p);
                ports.insert(upstream_port, p);
            }
        }
        let domains_taken = |h: &str| inner.domains.lock().unwrap().get(h).is_some();
        let mut hosts = BTreeMap::new();
        for d in &c.domains {
            let mut h = rename_host(&d.hostname, &old_slug, &new_slug);
            let mut i = 2;
            while domains_taken(&h) {
                h = rename_host(&d.hostname, &old_slug, &format!("{new_slug}-{i}"));
                i += 1;
            }
            hosts.insert(d.hostname.clone(), h);
        }
        Self {
            old_slug,
            new_slug,
            old_path: c.project.path.clone(),
            new_path: new_path.display().to_string(),
            dbs,
            ports,
            hosts,
        }
    }

    fn host(&self, h: &str) -> String {
        if self.is_none() {
            return h.to_string();
        }
        self.hosts
            .get(h)
            .cloned()
            .unwrap_or_else(|| rename_host(h, &self.old_slug, &self.new_slug))
    }

    fn host_in_url(&self, url: &str) -> String {
        let mut out = url.to_string();
        for (old, new) in &self.hosts {
            out = out.replace(old.as_str(), new);
        }
        for (old, new) in &self.ports {
            out = out.replace(&format!(":{old}"), &format!(":{new}"));
        }
        out
    }

    fn db(&self, name: &str) -> String {
        self.dbs
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.to_string())
    }

    fn port(&self, p: u16) -> u16 {
        self.ports.get(&p).copied().unwrap_or(p)
    }

    fn path(&self, p: &str) -> String {
        if self.is_none() || self.old_path.is_empty() {
            return p.to_string();
        }
        match Path::new(p).strip_prefix(&self.old_path) {
            Ok(rel) if rel.as_os_str().is_empty() => self.new_path.clone(),
            Ok(rel) => Path::new(&self.new_path).join(rel).display().to_string(),
            Err(_) => p.to_string(),
        }
    }

    /// `.env` values that name the old project.
    fn env(&self, text: &str) -> String {
        let mut out = text.to_string();
        let get = |key: &str| {
            text.lines().find_map(|l| {
                let (k, v) = l.split_once('=')?;
                (k.trim() == key).then(|| v.trim().trim_matches('"').to_string())
            })
        };
        if let Some(db) = get("DB_DATABASE") {
            if let Some(new) = self.dbs.get(&db) {
                out = crate::quickapp::run::upsert_env(&out, "DB_DATABASE", new);
            }
        }
        if let Some(url) = get("APP_URL") {
            out = crate::quickapp::run::upsert_env(&out, "APP_URL", &self.host_in_url(&url));
        }
        if let Some(port) = get("PORT").and_then(|p| p.parse::<u16>().ok()) {
            out = crate::quickapp::run::upsert_env(&out, "PORT", &self.port(port).to_string());
        }
        out
    }

    fn describe(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (a, b) in &self.hosts {
            if a != b {
                out.push(format!("site {a} → {b}"));
            }
        }
        for (a, b) in &self.dbs {
            if a != b {
                out.push(format!("database {a} → {b}"));
            }
        }
        for (a, b) in &self.ports {
            out.push(format!("app port {a} → {b}"));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{Core, CoreCommand, CoreResponse};

    fn core() -> (Core, crate::test_support::IsolatedHome) {
        let home = crate::test_support::isolated_home();
        let settings = crate::settings::SettingsService::load(&home.paths).unwrap();
        (Core::new(settings, home.paths.clone()), home)
    }

    fn project(core: &Core, dir: &Path) -> String {
        std::fs::create_dir_all(dir.join(".openlocalserver")).unwrap();
        std::fs::write(dir.join("index.html"), "hi").unwrap();
        std::fs::write(
            dir.join(".env"),
            "APP_URL=https://shop.test\nDB_DATABASE=shop\nPORT=3000\n",
        )
        .unwrap();
        std::fs::write(dir.join(".openlocalserver").join("environment.yaml"), "name: shop\ndomain:\n  hostname: shop.test\ndatabase:\n  engine: mariadb\n  name: shop\n").unwrap();
        std::fs::create_dir_all(dir.join("node_modules").join("x")).unwrap();
        std::fs::write(dir.join("node_modules").join("x").join("big.js"), "x").unwrap();
        let CoreResponse::Project { project } = core
            .dispatch(CoreCommand::RegisterProject {
                path: dir.display().to_string(),
            })
            .unwrap()
        else {
            panic!()
        };
        let d = Domain {
            hostname: "shop.test".into(),
            project_id: Some(project.id.clone()),
            root: dir.display().to_string(),
            kind: SiteKind::Static,
            https: false,
            redirect_https: false,
            wildcard: false,
            enabled: true,
            ownership: Ownership::Managed,
            app: None,
            blocks: Default::default(),
            generated_hashes: Default::default(),
            public_domain: None,
            tunnel_id: None,
             server: None,
        };
        // Registering already gave it shop.test (automatic domains); make it ours either way.
        if core
            .inner()
            .domains
            .lock()
            .unwrap()
            .get("shop.test")
            .is_some()
        {
            core.inner().update_domain(d).unwrap();
        } else {
            core.inner().add_domain(d).unwrap();
        }
        project.id
    }

    #[test]
    fn hostnames_are_renamed_by_label() {
        assert_eq!(
            rename_host("shop.test", "shop", "shop-copy"),
            "shop-copy.test"
        );
        assert_eq!(rename_host("api.shop.test", "shop", "b"), "api.b.test");
        assert_eq!(
            rename_host("other.test", "shop", "b"),
            "b.other.test",
            "a name without the old one gets a prefix"
        );
    }

    #[test]
    fn a_snapshot_round_trips_and_restores_removed_config() {
        let (core, home) = core();
        let dir = home.paths.root().join("shop");
        let id = project(&core, &dir);
        let snap = core
            .inner()
            .create_snapshot(
                &id,
                "Before upgrade",
                SnapshotOptions {
                    env: true,
                    files: true,
                    databases: false,
                },
            )
            .unwrap();
        assert!(
            snap.summary.iter().any(|s| s.contains("shop.test")),
            "{:?}",
            snap.summary
        );
        let content = read_zip_meta(Path::new(&snap.path)).unwrap();
        assert_eq!(
            content.file_count, 3,
            "index.html, .env, environment.yaml; node_modules left out"
        );

        core.inner().remove_domain("shop.test").unwrap();
        std::fs::write(dir.join(".env"), "BROKEN=1\n").unwrap();
        let r = core
            .inner()
            .restore_snapshot(
                &id,
                &snap.id,
                RestoreOptions {
                    config: true,
                    env: true,
                    databases: false,
                    files: false,
                },
            )
            .unwrap();
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        assert!(core
            .inner()
            .domains
            .lock()
            .unwrap()
            .get("shop.test")
            .is_some());
        assert!(std::fs::read_to_string(dir.join(".env"))
            .unwrap()
            .contains("DB_DATABASE=shop"));
        assert!(r.safety_snapshot.is_some());
        assert_eq!(
            core.inner().list_snapshots(&id).len(),
            2,
            "the snapshot and the safety one"
        );
    }

    #[test]
    fn a_clone_gets_its_own_name_site_database_and_env() {
        let (core, home) = core();
        let dir = home.paths.root().join("shop");
        let id = project(&core, &dir);
        let target = home.paths.root().join("shop-copy");
        let r = core
            .inner()
            .clone_environment(&id, &target.display().to_string(), "shop-copy", "full")
            .unwrap();
        assert!(target.join("index.html").is_file());
        assert!(!target.join("node_modules").exists());
        assert!(
            core.inner()
                .domains
                .lock()
                .unwrap()
                .get("shop-copy.test")
                .is_some(),
            "{:?}",
            r
        );
        assert!(
            core.inner()
                .domains
                .lock()
                .unwrap()
                .get("shop.test")
                .is_some(),
            "the original keeps its site"
        );
        let env = std::fs::read_to_string(target.join(".env")).unwrap();
        assert!(
            env.contains("DB_DATABASE=shop_copy") && env.contains("APP_URL=https://shop-copy.test"),
            "{env}"
        );
        let m = manifest::read_manifest(&target).unwrap().unwrap();
        assert_eq!(m.domain.unwrap().hostname, "shop-copy.test");
        assert_eq!(m.database.unwrap().name.as_deref(), Some("shop_copy"));
    }

    #[test]
    fn an_exported_environment_imports_as_a_new_project_after_review() {
        let (core, home) = core();
        let dir = home.paths.root().join("shop");
        let id = project(&core, &dir);
        let snap = core
            .inner()
            .create_snapshot(
                &id,
                "export",
                SnapshotOptions {
                    env: false,
                    files: true,
                    databases: false,
                },
            )
            .unwrap();
        let exported = home.paths.root().join("shop-env.zip");
        core.inner()
            .export_snapshot(&id, &snap.id, &exported.display().to_string())
            .unwrap();

        let preview = core
            .inner()
            .preview_import(&exported.display().to_string())
            .unwrap();
        assert_eq!(preview.suggested_name, "shop-2", "shop is taken");
        let target = home.paths.root().join("imported");
        let r = core
            .inner()
            .import_environment(
                &exported.display().to_string(),
                &target.display().to_string(),
                "imported",
            )
            .unwrap();
        assert!(target.join("index.html").is_file());
        assert_eq!(
            r.project.path.to_lowercase(),
            std::fs::canonicalize(&target)
                .unwrap()
                .display()
                .to_string()
                .trim_start_matches(r"\\?\")
                .to_lowercase()
        );
    }

    #[test]
    fn settings_backups_restore_after_saving_the_current_state() {
        let (core, _home) = core();
        core.dispatch(CoreCommand::SetSetting {
            key: "editor".into(),
            value: serde_json::json!("vscode"),
        })
        .unwrap();
        let b = core.inner().backup_settings().unwrap();
        core.dispatch(CoreCommand::SetSetting {
            key: "editor".into(),
            value: serde_json::json!("zed"),
        })
        .unwrap();
        core.inner().restore_settings(&b.id).unwrap();
        let settings = crate::db::load_settings(&core.inner().paths).unwrap();
        let text = serde_json::to_string(&settings).unwrap();
        assert!(text.contains("vscode"));
        assert_eq!(core.inner().list_settings_backups().len(), 2);
    }
}
