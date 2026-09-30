//! Central SQLite store (`app.db`). Replaces all `*.json` files in `data_dir`.
//! Fresh only: no JSON import, start clean. WAL mode, single writer, `busy_timeout`.
//!
//! Schema:
//! - `settings(key TEXT PK, value TEXT NOT NULL)` — JSON-encoded `serde_json::Value`.
//! - `docs(collection TEXT, id TEXT, data TEXT NOT NULL, PK(collection,id))` — every
//!   Vec-store persists each item as one JSON blob. Keeps struct evolution free
//!   without per-field migrations, still gives ACID + single file + transactions.

use std::path::PathBuf;

use rusqlite::{params, Connection};
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
/// to re-read its stores and stop asking until the next write.
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
}
