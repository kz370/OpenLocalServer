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
    Ok(())
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
