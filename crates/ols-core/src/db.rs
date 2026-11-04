//! Central SQLite store (`app.db`). Replaces all `*.json` files in `data_dir`.
//! Fresh only: no JSON import, start clean. WAL mode, single writer, `busy_timeout`.
//!
//! Schema:
//! - `settings(key TEXT PK, value TEXT NOT NULL)` — JSON-encoded `serde_json::Value`.
//! - `docs(collection TEXT, id TEXT, data TEXT NOT NULL, PK(collection,id))` — every
//!   Vec-store persists each item as one JSON blob. Keeps struct evolution free
//!   without per-field migrations, still gives ACID + single file + transactions.

use std::path::PathBuf;

use rusqlite::{params, Connection, TransactionBehavior};
use serde_json::Value;

use crate::error::CoreError;
use crate::paths::AppPaths;

fn map_err(e: rusqlite::Error) -> CoreError {
    CoreError::Db(e.to_string())
}

/// Settings key holding the process id of whoever wrote `app.db` last.
///
/// The app caches its stores in memory and reloads them on start, but the Explorer
/// right-click menu and the `ols` command line write the same database from a *separate*
/// process. Without this marker the running app cannot tell its own writes from someone
/// else's, and a site added from the right-click menu stayed invisible until a restart.
///
/// The writer is identified by process id rather than by file time on purpose: under WAL
/// a commit lands in `app.db-wal` and a later checkpoint moves `app.db` again, so the
/// timestamps of the files do not line up with the writes and comparing them reported the
/// app's own saves as outside changes.
const LAST_WRITER_KEY: &str = "db.last_writer";

/// True when another process has written to `app.db` since this one last looked.
///
/// One single-row read, which is cheap enough to sit in front of every list of sites and
/// projects. Returns true only when a write came from elsewhere; the caller is expected
/// to re-read its stores and then adopt the ownership below, so the same foreign write is
/// not reported again on every subsequent list.
pub fn changed_externally(paths: &AppPaths) -> bool {
    let Ok(Some(writer)) = read_setting(paths, LAST_WRITER_KEY) else {
        // No marker yet: nothing has ever recorded a writer, so nothing changed.
        return false;
    };
    writer
        .as_str()
        .is_some_and(|pid| pid != std::process::id().to_string())
}

/// One settings row, or `None` when it was never written. The targeted read
/// [`changed_externally`] needs; loading the whole table for a single key is waste.
fn read_setting(paths: &AppPaths, key: &str) -> Result<Option<Value>, CoreError> {
    let conn = connect(paths)?;
    let mut stmt = conn
        .prepare("SELECT value FROM settings WHERE key=?1")
        .map_err(map_err)?;
    let mut rows = stmt
        .query_map(params![key], |row| row.get::<_, String>(0))
        .map_err(map_err)?;
    match rows.next() {
        None => Ok(None),
        Some(row) => {
            let raw = row.map_err(map_err)?;
            Ok(Some(serde_json::from_str(&raw).unwrap_or(Value::Null)))
        }
    }
}

/// Stamps the database as written by this process. Called on every commit: a single
/// indexed upsert against a table that already exists, next to the write that caused it.
fn note_write(paths: &AppPaths) {
    if let Err(e) = save_setting_raw(
        paths,
        LAST_WRITER_KEY,
        &Value::from(std::process::id().to_string()),
    ) {
        tracing::warn!(%e, "could not record which process wrote the database");
    }
}

/// Adopts the writer marker after an outside change has been consumed, so
/// [`changed_externally`] stops reporting the same foreign write on every list command.
/// Without this, one stale marker (written by a CLI or Explorer run that has since exited)
/// made every `list_domains`/`list_projects` re-read all stores and re-scan the project
/// roots, which was the multi-second cost on every site read.
pub fn adopt_ownership(paths: &AppPaths) {
    note_write(paths);
}

/// Open (creating dirs + file) and init schema. Sets WAL, FK, busy timeout.
pub fn connect(paths: &AppPaths) -> Result<Connection, CoreError> {
    paths.ensure_dirs()?;
    let file: PathBuf = paths.db_file();
    let conn = Connection::open(&file).map_err(map_err)?;
    conn.busy_timeout(std::time::Duration::from_millis(5000))
        .map_err(map_err)?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA foreign_keys=ON;
         CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS docs(collection TEXT NOT NULL, id TEXT NOT NULL, data TEXT NOT NULL, PRIMARY KEY(collection, id));",
    )
    .map_err(map_err)?;
    Ok(conn)
}

/// Apply additive, versioned database migrations. Called before the app loads its stores.
/// A failure is returned as a structured diagnostic; startup callers may continue with the
/// original JSON tables because all migration work is transactional and additive.
pub fn migrate(paths: &AppPaths) -> Result<(), CoreError> {
    for (target, apply) in [
        (
            1,
            migrate_read_views as fn(&rusqlite::Transaction<'_>) -> rusqlite::Result<()>,
        ),
        (2, migrate_operation_retention),
    ] {
        loop {
            let current_version: i64 = {
                let conn =
                    connect(paths).map_err(|error| migration_error(target, error.to_string()))?;
                conn.pragma_query_value(None, "user_version", |row| row.get(0))
                    .map_err(|error| migration_error(target, error.to_string()))?
            };
            if current_version >= target {
                break;
            }
            // SQLite checkpoints on last close. Copy the database and any remaining sidecars
            // without an open handle, which is required on Windows where SQLite locks SHM.
            backup_database(paths, current_version, target)
                .map_err(|error| migration_error(target, error.to_string()))?;
            let mut conn =
                connect(paths).map_err(|error| migration_error(target, error.to_string()))?;
            let tx = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|error| migration_error(target, error.to_string()))?;
            let locked_version: i64 =
                match tx.pragma_query_value(None, "user_version", |row| row.get(0)) {
                    Ok(version) => version,
                    Err(error) => {
                        let _ = tx.rollback();
                        return Err(migration_error(target, error.to_string()));
                    }
                };
            if locked_version != current_version {
                tx.rollback()
                    .map_err(|error| migration_error(target, error.to_string()))?;
                continue;
            }
            let result = apply(&tx).and_then(|()| {
                tx.pragma_update(None, "user_version", target)?;
                Ok(())
            });
            match result {
                Ok(()) => tx
                    .commit()
                    .map_err(|error| migration_error(target, error.to_string()))?,
                Err(error) => {
                    let _ = tx.rollback();
                    return Err(migration_error(target, error.to_string()));
                }
            }
            break;
        }
    }
    Ok(())
}

fn migration_error(target: i64, cause: String) -> CoreError {
    CoreError::failed_fix(
        format!("Database migration {target} could not be applied."),
        cause,
        "The original database was kept. Check that the database is writable, then restart OLS to retry.",
    )
}

fn migrate_read_views(tx: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    tx.execute_batch(
        r#"
        CREATE VIEW IF NOT EXISTS sites_view AS
        SELECT json_extract(data,'$.hostname') AS hostname,
               json_extract(data,'$.kind.type') AS kind,
               json_extract(data,'$.kind.type') AS kind_type,
               json_extract(data,'$.kind.upstream_port') AS upstream_port,
               json_extract(data,'$.root') AS root,
               json_extract(data,'$.https') AS https,
               json_extract(data,'$.ssl_provider') AS ssl_provider,
               json_extract(data,'$.enabled') AS enabled,
               json_extract(data,'$.wildcard') AS wildcard,
               json_extract(data,'$.project_id') AS project_id,
               json_extract(data,'$.ownership') AS ownership
          FROM docs WHERE collection='domains';
        CREATE VIEW IF NOT EXISTS projects_view AS
        SELECT json_extract(p.data,'$.id') AS id,
               json_extract(p.data,'$.name') AS name,
               json_extract(p.data,'$.path') AS path,
               json_extract(p.data,'$.standalone') AS standalone,
               json_extract(p.data,'$.health.status') AS health_status,
               (SELECT s.hostname FROM sites_view s
                 WHERE s.project_id=json_extract(p.data,'$.id') LIMIT 1) AS site_hostname
          FROM docs p WHERE p.collection='projects';
        CREATE VIEW IF NOT EXISTS operations_view AS
        SELECT json_extract(data,'$.id') AS id, json_extract(data,'$.kind') AS kind,
               json_extract(data,'$.title') AS title, json_extract(data,'$.status') AS status,
               json_extract(data,'$.started_ms') AS started_ms,
               json_extract(data,'$.finished_ms') AS finished_ms,
               json_extract(data,'$.detail') AS detail
          FROM docs WHERE collection='operations';
        CREATE VIEW IF NOT EXISTS workers_view AS
        SELECT id, json_extract(data,'$.project_id') AS project_id,
               json_extract(data,'$.name') AS name, json_extract(data,'$.command') AS command,
               json_extract(data,'$.count') AS count, json_extract(data,'$.autostart') AS autostart
          FROM docs WHERE collection='workers';
        CREATE VIEW IF NOT EXISTS schedules_view AS
        SELECT id, json_extract(data,'$.project_id') AS project_id,
               json_extract(data,'$.name') AS name, json_extract(data,'$.schedule') AS schedule,
               json_extract(data,'$.command') AS command, json_extract(data,'$.enabled') AS enabled
          FROM docs WHERE collection='schedules';
        CREATE VIEW IF NOT EXISTS command_history_view AS
        SELECT id, json_extract(data,'$.line') AS line, json_extract(data,'$.cwd') AS cwd,
               json_extract(data,'$.project_id') AS project_id,
               json_extract(data,'$.timestamp_ms') AS timestamp_ms
          FROM docs WHERE collection='command_history';
        CREATE INDEX IF NOT EXISTS docs_domains_project_id
          ON docs(json_extract(data,'$.project_id')) WHERE collection='domains';
        CREATE INDEX IF NOT EXISTS docs_operations_status_started
          ON docs(json_extract(data,'$.status'), json_extract(data,'$.started_ms'))
          WHERE collection='operations';
        CREATE INDEX IF NOT EXISTS docs_projects_path
          ON docs(json_extract(data,'$.path')) WHERE collection='projects';
        "#,
    )
}

fn migrate_operation_retention(tx: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    prune_operations_tx(tx)
}

/// Read-only connection helper for the UI's project/site picker. `None` means the view is
/// unavailable and lets callers retain their existing in-memory fallback.
pub fn unconnected_project_ids(
    paths: &AppPaths,
) -> Result<Option<std::collections::HashSet<String>>, CoreError> {
    let conn = connect(paths)?;
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='view' AND name='projects_view')",
            [],
            |row| row.get(0),
        )
        .map_err(map_err)?;
    if !exists {
        return Ok(None);
    }
    let mut stmt = conn
        .prepare("SELECT id FROM projects_view WHERE site_hostname IS NULL")
        .map_err(map_err)?;
    let rows = stmt
        .query_map([], |row| row.get::<_, Option<String>>(0))
        .map_err(map_err)?;
    let mut ids = std::collections::HashSet::new();
    for row in rows {
        if let Some(id) = row.map_err(map_err)? {
            ids.insert(id);
        }
    }
    Ok(Some(ids))
}

/// Prune only the operations collection: age first, then oldest-first above the cap.
pub(crate) fn prune_operations_tx(tx: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    let cutoff = crate::app::now_ms().saturating_sub(30 * 24 * 60 * 60 * 1000);
    let mut stmt = tx.prepare("SELECT id, data FROM docs WHERE collection='operations'")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut entries = Vec::<(String, u64, Value)>::new();
    for row in rows {
        let (id, raw) = row?;
        if let Ok(value) = serde_json::from_str::<Value>(&raw) {
            let started = value.get("started_ms").and_then(Value::as_u64).unwrap_or(0);
            if started >= cutoff {
                entries.push((id, started, value));
            }
        }
    }
    drop(stmt);
    entries.sort_by_key(|entry| entry.1);
    let mut collapsed = Vec::<(String, u64, Value)>::new();
    for entry in entries {
        let repeated = collapsed.last().is_some_and(|(_, _, previous)| {
            previous.get("status").and_then(Value::as_str) == Some("failed")
                && entry.2.get("status").and_then(Value::as_str) == Some("failed")
                && previous.get("kind") == entry.2.get("kind")
                && previous.get("detail") == entry.2.get("detail")
        });
        if repeated {
            let Some((_, _, previous)) = collapsed.pop() else {
                unreachable!()
            };
            let count = previous
                .get("repeat_count")
                .and_then(Value::as_u64)
                .unwrap_or(1)
                .saturating_add(1);
            let mut latest = entry;
            if let Some(object) = latest.2.as_object_mut() {
                object.insert("repeat_count".into(), Value::from(count));
            }
            collapsed.push(latest);
        } else {
            collapsed.push(entry);
        }
    }
    if collapsed.len() > 100 {
        let remove = collapsed.len() - 100;
        collapsed.drain(..remove);
    }
    tx.execute("DELETE FROM docs WHERE collection='operations'", [])?;
    for (id, _, value) in collapsed {
        tx.execute(
            "INSERT INTO docs(collection,id,data) VALUES('operations',?1,?2)",
            params![id, value.to_string()],
        )?;
    }
    Ok(())
}

fn backup_database(paths: &AppPaths, from: i64, to: i64) -> Result<(), CoreError> {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};
    let db = paths.db_file();
    let parent = db.parent().unwrap_or_else(|| std::path::Path::new("."));
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let mut base = db.with_file_name(format!("app.db.v{from}-v{to}.{stamp}.bak"));
    let mut suffix = 1;
    while base.exists() {
        base = db.with_file_name(format!("app.db.v{from}-v{to}.{stamp}-{suffix}.bak"));
        suffix += 1;
    }
    fs::copy(&db, &base).map_err(CoreError::Io)?;
    for suffix in ["-wal", "-shm"] {
        let sidecar = std::path::PathBuf::from(format!("{}{suffix}", db.display()));
        if sidecar.exists() {
            let target = std::path::PathBuf::from(format!("{}{suffix}", base.display()));
            fs::copy(sidecar, target).map_err(CoreError::Io)?;
        }
    }
    let mut backups: Vec<_> = fs::read_dir(parent)
        .map_err(CoreError::Io)?
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_name().to_string_lossy().starts_with("app.db.v")
                && entry.path().extension().is_some_and(|ext| ext == "bak")
        })
        .collect();
    backups.sort_by_key(|entry| entry.metadata().and_then(|m| m.modified()).ok());
    while backups.len() > 3 {
        let old = backups.remove(0).path();
        let _ = fs::remove_file(&old);
        for suffix in ["-wal", "-shm"] {
            let _ = fs::remove_file(format!("{}{suffix}", old.display()));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- settings

pub fn load_settings(
    paths: &AppPaths,
) -> Result<std::collections::BTreeMap<String, Value>, CoreError> {
    let conn = connect(paths)?;
    let mut stmt = conn
        .prepare("SELECT key, value FROM settings")
        .map_err(map_err)?;
    let rows = stmt
        .query_map([], |row| {
            let k: String = row.get(0)?;
            let v: String = row.get(1)?;
            Ok((k, v))
        })
        .map_err(map_err)?;
    let mut out = std::collections::BTreeMap::new();
    for r in rows {
        let (k, v) = r.map_err(map_err)?;
        let value: Value = serde_json::from_str(&v).unwrap_or(Value::Null);
        out.insert(k, value);
    }
    Ok(out)
}

pub fn save_setting(paths: &AppPaths, key: &str, value: &Value) -> Result<(), CoreError> {
    save_setting_raw(paths, key, value)?;
    note_write(paths);
    Ok(())
}

/// The upsert itself, without the writer stamp. Split out so [`note_write`] can record
/// its own row without the two writing over each other.
fn save_setting_raw(paths: &AppPaths, key: &str, value: &Value) -> Result<(), CoreError> {
    let conn = connect(paths)?;
    let raw = serde_json::to_string(value)?;
    conn.execute(
        "INSERT INTO settings(key, value) VALUES(?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, raw],
    )
    .map_err(map_err)?;
    Ok(())
}

// ---------------------------------------------------------------- generic docs

/// Load all blobs for a collection. Corrupt rows skipped, missing table = empty.
pub fn load_docs<T>(paths: &AppPaths, collection: &str) -> Result<Vec<T>, CoreError>
where
    T: serde::de::DeserializeOwned,
{
    let conn = connect(paths)?;
    let mut stmt = conn
        .prepare("SELECT data FROM docs WHERE collection=?1")
        .map_err(map_err)?;
    let rows = stmt
        .query_map(params![collection], |row| {
            let d: String = row.get(0)?;
            Ok(d)
        })
        .map_err(map_err)?;
    let mut out = Vec::new();
    for r in rows {
        let raw = r.map_err(map_err)?;
        if let Ok(item) = serde_json::from_str::<T>(&raw) {
            out.push(item);
        }
    }
    Ok(out)
}

/// Replace whole collection atomically (delete + insert in txn).
pub fn save_docs<T>(
    paths: &AppPaths,
    collection: &str,
    items: &[(String, &T)],
) -> Result<(), CoreError>
where
    T: serde::Serialize,
{
    let mut conn = connect(paths)?;
    let tx = conn.transaction().map_err(map_err)?;
    tx.execute("DELETE FROM docs WHERE collection=?1", params![collection])
        .map_err(map_err)?;
    for (id, item) in items {
        let raw = serde_json::to_string(item)?;
        tx.execute(
            "INSERT INTO docs(collection, id, data) VALUES(?1, ?2, ?3)",
            params![collection, id, raw],
        )
        .map_err(map_err)?;
    }
    tx.commit().map_err(map_err)?;
    note_write(paths);
    Ok(())
}

/// [`save_docs`] for rows already serialized, so a caller can rewrite one field per row
/// on its way to the database without cloning every document twice.
pub fn save_values(
    paths: &AppPaths,
    collection: &str,
    items: &[(String, serde_json::Value)],
) -> Result<(), CoreError> {
    let mut conn = connect(paths)?;
    let tx = conn.transaction().map_err(map_err)?;
    tx.execute("DELETE FROM docs WHERE collection=?1", params![collection])
        .map_err(map_err)?;
    for (id, item) in items {
        tx.execute(
            "INSERT INTO docs(collection, id, data) VALUES(?1, ?2, ?3)",
            params![collection, id, item.to_string()],
        )
        .map_err(map_err)?;
    }
    tx.commit().map_err(map_err)?;
    note_write(paths);
    Ok(())
}

/// Stamps the database as written by a *different* process, so the outside-change path
/// can be exercised without spawning one. `0` is never a real process id.
#[cfg(test)]
pub fn simulate_foreign_write(paths: &AppPaths) {
    save_setting_raw(paths, LAST_WRITER_KEY, &Value::from("0")).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn settings_round_trip() {
        let home = crate::test_support::isolated_home();
        save_setting(&home.paths, "theme", &json!("dark")).unwrap();
        let all = load_settings(&home.paths).unwrap();
        assert_eq!(all.get("theme"), Some(&json!("dark")));
    }

    #[test]
    fn docs_round_trip() {
        let home = crate::test_support::isolated_home();
        let owned: Vec<(String, serde_json::Value)> = vec![("a".to_string(), json!({"x":1}))];
        let refs: Vec<(String, &serde_json::Value)> =
            owned.iter().map(|(k, v)| (k.clone(), v)).collect();
        save_docs(&home.paths, "test", &refs).unwrap();
        let back: Vec<serde_json::Value> = load_docs(&home.paths, "test").unwrap();
        assert_eq!(back.len(), 1);
    }

    #[test]
    fn attached_old_database_migrates_twice_without_changing_legacy_reads() {
        let home = crate::test_support::isolated_home();
        std::fs::write(home.paths.db_file(), include_bytes!("../../../data/app.db")).unwrap();
        // The checked-in database advances with the app. Rewind only its migration marker
        // and views so this test always exercises both migrations against the current data.
        let conn = Connection::open(home.paths.db_file()).unwrap();
        conn.execute_batch(
            "DROP VIEW IF EXISTS sites_view;
             DROP VIEW IF EXISTS projects_view;
             DROP VIEW IF EXISTS operations_view;
             DROP VIEW IF EXISTS workers_view;
             DROP VIEW IF EXISTS schedules_view;
             DROP VIEW IF EXISTS command_history_view;
             PRAGMA user_version=0;",
        )
        .unwrap();
        drop(conn);
        let domains_before: Vec<crate::domain::Domain> = load_docs(&home.paths, "domains").unwrap();
        let projects_before: Vec<crate::project::Project> =
            load_docs(&home.paths, "projects").unwrap();
        let settings_before = load_settings(&home.paths).unwrap();
        let conn = Connection::open(home.paths.db_file()).unwrap();
        let docs_schema: String = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name='docs'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let settings_schema: String = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name='settings'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        drop(conn);

        migrate(&home.paths).unwrap();
        migrate(&home.paths).unwrap();

        assert_eq!(
            serde_json::to_value(
                load_docs::<crate::domain::Domain>(&home.paths, "domains").unwrap()
            )
            .unwrap(),
            serde_json::to_value(&domains_before).unwrap()
        );
        assert_eq!(
            serde_json::to_value(
                load_docs::<crate::project::Project>(&home.paths, "projects").unwrap()
            )
            .unwrap(),
            serde_json::to_value(&projects_before).unwrap()
        );
        assert_eq!(load_settings(&home.paths).unwrap(), settings_before);
        let expected_unconnected: std::collections::HashSet<String> = projects_before
            .iter()
            .filter(|project| {
                !domains_before
                    .iter()
                    .any(|domain| domain.project_id.as_deref() == Some(project.id.as_str()))
            })
            .map(|project| project.id.clone())
            .collect();
        assert_eq!(
            unconnected_project_ids(&home.paths).unwrap().unwrap(),
            expected_unconnected
        );
        let conn = connect(&home.paths).unwrap();
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 2);
        for view in [
            "sites_view",
            "projects_view",
            "operations_view",
            "workers_view",
            "schedules_view",
            "command_history_view",
        ] {
            let exists: bool = conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='view' AND name=?1)",
                    [view],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(exists, "missing view {view}");
        }
        let after_docs_schema: String = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name='docs'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let after_settings_schema: String = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name='settings'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(after_docs_schema, docs_schema);
        assert_eq!(after_settings_schema, settings_schema);
        let sites_view_count: usize = conn
            .query_row("SELECT COUNT(*) FROM sites_view", [], |row| row.get(0))
            .unwrap();
        let projects_view_count: usize = conn
            .query_row("SELECT COUNT(*) FROM projects_view", [], |row| row.get(0))
            .unwrap();
        assert_eq!(sites_view_count, domains_before.len());
        assert_eq!(projects_view_count, projects_before.len());
        let backup_names: Vec<String> = std::fs::read_dir(home.paths.db_file().parent().unwrap())
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with("app.db.v") && name.ends_with(".bak"))
            .collect();
        assert_eq!(
            backup_names.len(),
            2,
            "a second migration run must not make new backups: {backup_names:?}"
        );
        drop(conn);
        for _ in 0..4 {
            backup_database(&home.paths, 2, 3).unwrap();
        }
        let retained_backups = std::fs::read_dir(home.paths.db_file().parent().unwrap())
            .unwrap()
            .flatten()
            .filter(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                name.starts_with("app.db.v") && name.ends_with(".bak")
            })
            .count();
        assert_eq!(retained_backups, 3, "only the newest three backups remain");
    }
}
