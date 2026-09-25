//! SQLite (§36, Stage 10): no server process — just files. Create, detect, associate with
//! a project, show the path, back up, restore, integrity-check. All work is done by the
//! real `sqlite3` command-line shell, so a backup of a live WAL-mode database is safe
//! (`.backup`), not a naive file copy.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::CoreError;
use crate::exec::run_capture;
use crate::paths::AppPaths;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SqliteEntry {
    pub path: String,
    pub project_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqliteInfo {
    pub path: String,
    pub name: String,
    pub project_id: Option<String>,
    pub exists: bool,
    pub size_bytes: u64,
    pub backups: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrityResult {
    pub ok: bool,
    pub detail: String,
}

pub struct SqliteStore {
    file: PathBuf,
    entries: Vec<SqliteEntry>,
}

impl SqliteStore {
    pub fn load(paths: &AppPaths) -> Result<Self, CoreError> {
        paths.ensure_dirs()?;
        let file = paths.data_dir().join("sqlite_databases.json");
        let entries = if file.exists() {
            serde_json::from_str(&std::fs::read_to_string(&file)?).unwrap_or_default()
        } else {
            Vec::new()
        };
        Ok(Self { file, entries })
    }

    pub fn list(&self) -> Vec<SqliteInfo> {
        self.entries.iter().map(info_for).collect()
    }

    /// Registers (or re-associates) a database file with an optional project (§36 associate).
    pub fn associate(&mut self, path: &str, project_id: Option<String>) -> Result<SqliteInfo, CoreError> {
        let normalized = normalize(path);
        match self.entries.iter_mut().find(|e| e.path == normalized) {
            Some(existing) => existing.project_id = project_id,
            None => self.entries.push(SqliteEntry { path: normalized.clone(), project_id }),
        }
        self.persist()?;
        Ok(info_for(self.entries.iter().find(|e| e.path == normalized).expect("just inserted")))
    }

    pub fn forget(&mut self, path: &str) -> Result<(), CoreError> {
        let normalized = normalize(path);
        self.entries.retain(|e| e.path != normalized);
        self.persist()
    }

    fn persist(&self) -> Result<(), CoreError> {
        let raw = serde_json::to_string_pretty(&self.entries)?;
        let tmp = self.file.with_extension("json.tmp");
        std::fs::write(&tmp, raw)?;
        std::fs::rename(&tmp, &self.file)?;
        Ok(())
    }
}

fn normalize(path: &str) -> String {
    path.replace('/', "\\")
}

fn info_for(entry: &SqliteEntry) -> SqliteInfo {
    let path = PathBuf::from(&entry.path);
    let meta = std::fs::metadata(&path).ok();
    SqliteInfo {
        name: path.file_name().and_then(|n| n.to_str()).unwrap_or("database").to_string(),
        path: entry.path.clone(),
        project_id: entry.project_id.clone(),
        exists: meta.is_some(),
        size_bytes: meta.map(|m| m.len()).unwrap_or(0),
        backups: list_backups(&path),
    }
}

/// §36 detect: database files inside a project. Skips dependency and VCS folders and never
/// descends deeper than 3 levels, so scanning a big project stays instant.
pub fn detect_in_project(project: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for e in entries.flatten() {
            let path = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if path.is_dir() {
                if depth < 3 && !matches!(name.as_str(), "node_modules" | "vendor" | ".git" | "target" | "dist" | "build" | "storage") {
                    walk(&path, depth + 1, out);
                }
            } else if is_sqlite_name(&name) && has_sqlite_header(&path) {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(project, 0, &mut out);
    // Laravel keeps its file under database/ — `storage` is skipped above, `database` is not.
    out.sort();
    out
}

fn is_sqlite_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [".sqlite", ".sqlite3", ".db", ".db3", ".s3db"].iter().any(|ext| lower.ends_with(ext))
}

/// Every SQLite file starts with this 16-byte magic; checking it keeps unrelated `.db`
/// files (thumbs.db, Access, ...) out of the list.
fn has_sqlite_header(path: &Path) -> bool {
    use std::io::Read;
    let mut buf = [0u8; 16];
    std::fs::File::open(path).and_then(|mut f| f.read_exact(&mut buf)).is_ok() && &buf == b"SQLite format 3\0"
}

fn timeout() -> Duration {
    Duration::from_secs(60)
}

/// §36 create: makes a new, valid, empty database at `path`.
pub fn create(sqlite3: &Path, path: &Path) -> Result<(), String> {
    if path.exists() {
        return Err(format!("{} already exists", path.display()));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    // Setting a non-default pragma forces SQLite to write the file header.
    let out = run_capture(
        sqlite3,
        &[path.display().to_string(), "PRAGMA user_version = 1; PRAGMA user_version = 0;".into()],
        None,
        &[],
        timeout(),
    );
    if !out.success() {
        return Err(format!("sqlite3 failed: {}", out.combined()));
    }
    if !path.exists() {
        return Err("sqlite3 ran but did not create the file".into());
    }
    Ok(())
}

/// §36 integrity check: `PRAGMA integrity_check` — a healthy file answers exactly "ok".
pub fn integrity_check(sqlite3: &Path, path: &Path) -> Result<IntegrityResult, String> {
    if !path.is_file() {
        return Err(format!("{} does not exist", path.display()));
    }
    let out = run_capture(sqlite3, &[path.display().to_string(), "PRAGMA integrity_check;".into()], None, &[], timeout());
    if out.exit_code.is_none() {
        return Err(out.combined());
    }
    let text = out.stdout.trim().to_string();
    Ok(IntegrityResult { ok: out.success() && text == "ok", detail: if text.is_empty() { out.combined() } else { text } })
}

/// Backups sit beside the database as `name.<unix-secs>.bak`.
fn backup_path(path: &Path) -> PathBuf {
    let secs = crate::ca::unix_now();
    let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(format!(".{secs}.bak"));
    path.with_file_name(name)
}

/// §36 backup, via SQLite's online backup API so it's consistent even while the app has
/// the database open.
pub fn backup(sqlite3: &Path, path: &Path) -> Result<PathBuf, String> {
    if !path.is_file() {
        return Err(format!("{} does not exist", path.display()));
    }
    let mut dest = backup_path(path);
    while dest.exists() {
        // Two backups in the same second must not overwrite each other.
        let mut n = dest.file_name().unwrap().to_os_string();
        n.push("x");
        dest = dest.with_file_name(n);
    }
    let cmd = format!(".backup '{}'", dest.display().to_string().replace('\'', "''"));
    let out = run_capture(sqlite3, &[path.display().to_string(), cmd], None, &[], timeout());
    if !out.success() || !dest.is_file() {
        return Err(format!("backup failed: {}", out.combined()));
    }
    Ok(dest)
}

pub fn list_backups(path: &Path) -> Vec<String> {
    let Some(dir) = path.parent() else { return Vec::new() };
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else { return Vec::new() };
    let prefix = format!("{name}.");
    let mut found: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .filter(|n| n.starts_with(&prefix) && n.ends_with(".bak"))
        .map(|n| dir.join(n).display().to_string())
        .collect();
    found.sort();
    found.reverse();
    found
}

/// §36 restore. The current file is backed up first, so a restore can itself be undone.
pub fn restore(sqlite3: &Path, path: &Path, backup_file: &Path) -> Result<Option<PathBuf>, String> {
    if !backup_file.is_file() {
        return Err(format!("{} does not exist", backup_file.display()));
    }
    // A backup that isn't a healthy database must never replace a working one.
    let check = integrity_check(sqlite3, backup_file)?;
    if !check.ok {
        return Err(format!("that backup is damaged ({}), so it was not restored", check.detail));
    }
    let safety = if path.is_file() { Some(backup(sqlite3, path)?) } else { None };
    // Stale WAL/SHM sidecars would corrupt the restored file.
    for ext in ["-wal", "-shm"] {
        let mut sidecar = path.as_os_str().to_os_string();
        sidecar.push(ext);
        let _ = std::fs::remove_file(sidecar);
    }
    std::fs::copy(backup_file, path).map_err(|e| format!("could not restore: {e}"))?;
    Ok(safety)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real sqlite3.exe — installed by the Stage 10 smoke test, or found on PATH.
    fn sqlite3() -> Option<PathBuf> {
        // The real per-user install, not OLS_HOME (other tests set that concurrently).
        let dirs = directories::ProjectDirs::from("dev", "OpenLocalServer", "OpenLocalServer")?;
        let managed = dirs.data_dir().join("runtimes").join("sqlite");
        if let Ok(dirs) = std::fs::read_dir(managed) {
            for d in dirs.flatten() {
                let exe = d.path().join("sqlite3.exe");
                if exe.is_file() {
                    return Some(exe);
                }
            }
        }
        let path = std::env::var_os("PATH")?;
        std::env::split_paths(&path).map(|d| d.join("sqlite3.exe")).find(|p| p.is_file())
    }

    #[test]
    fn detect_finds_real_sqlite_files_and_skips_impostors_and_dependency_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let db_dir = dir.path().join("database");
        std::fs::create_dir_all(&db_dir).unwrap();
        let mut header = b"SQLite format 3\0".to_vec();
        header.extend_from_slice(&[0u8; 100]);
        std::fs::write(db_dir.join("database.sqlite"), &header).unwrap();
        std::fs::write(dir.path().join("thumbs.db"), b"not a database at all").unwrap();
        let vendor = dir.path().join("node_modules").join("x");
        std::fs::create_dir_all(&vendor).unwrap();
        std::fs::write(vendor.join("cache.sqlite"), &header).unwrap();

        let found = detect_in_project(dir.path());
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].ends_with("database.sqlite"));
    }

    #[test]
    fn store_associates_persists_and_forgets() {
        let home = crate::test_support::isolated_home();
        let mut store = SqliteStore::load(&home.paths).unwrap();
        store.associate("C:/proj/db.sqlite", Some("p1".into())).unwrap();
        store.associate("C:\\proj\\db.sqlite", Some("p2".into())).unwrap(); // same file, re-associated
        assert_eq!(store.list().len(), 1);
        assert_eq!(store.list()[0].project_id.as_deref(), Some("p2"));

        let reloaded = SqliteStore::load(&home.paths).unwrap();
        assert_eq!(reloaded.list().len(), 1);
        let mut reloaded = reloaded;
        reloaded.forget("C:/proj/db.sqlite").unwrap();
        assert!(reloaded.list().is_empty());
    }

    #[test]
    fn create_backup_integrity_and_restore_round_trip_with_real_sqlite3() {
        let Some(exe) = sqlite3() else {
            eprintln!("skipping: sqlite3.exe is not installed (run the stage-10 smoke example first)");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("app.sqlite");

        create(&exe, &db).unwrap();
        assert!(has_sqlite_header(&db), "create must produce a real SQLite file");
        assert!(create(&exe, &db).is_err(), "must not overwrite");

        // Put real data in, back it up, wreck the original, restore.
        let out = run_capture(&exe, &[db.display().to_string(), "CREATE TABLE t(x); INSERT INTO t VALUES (42);".into()], None, &[], timeout());
        assert!(out.success(), "{}", out.combined());
        assert!(integrity_check(&exe, &db).unwrap().ok);

        let bak = backup(&exe, &db).unwrap();
        assert_eq!(list_backups(&db).len(), 1);
        run_capture(&exe, &[db.display().to_string(), "DELETE FROM t;".into()], None, &[], timeout());

        let safety = restore(&exe, &db, &bak).unwrap();
        assert!(safety.is_some(), "restore must keep a safety copy of what it replaced");
        let out = run_capture(&exe, &[db.display().to_string(), "SELECT x FROM t;".into()], None, &[], timeout());
        assert_eq!(out.stdout.trim(), "42");
    }

    #[test]
    fn a_damaged_backup_is_never_restored() {
        let Some(exe) = sqlite3() else { return };
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("app.sqlite");
        create(&exe, &db).unwrap();
        let bad = dir.path().join("app.sqlite.1.bak");
        std::fs::write(&bad, b"garbage that is not a database, definitely not, padding padding padding").unwrap();
        assert!(restore(&exe, &db, &bad).is_err());
    }
}
