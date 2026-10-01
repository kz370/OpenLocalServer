//! Backup and restore for the SQL servers we run (§32, §34): MariaDB and PostgreSQL.
//! Backups are plain SQL dumps made with the engine's own dump tool, kept under
//! `backups/<engine>/<database>.<unix-secs>.sql`. Restoring first takes a safety backup of
//! the current database so a restore can itself be undone (the same rule as SQLite, §36).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::exec::{hide_window, run_capture};
use crate::paths::AppPaths;
use crate::service::ServiceManager;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbBackup {
    pub file: String,
    pub database: String,
    /// Unix seconds, from the file name.
    pub created: u64,
    pub size: u64,
}

fn check_engine(engine: &str) -> Result<(), String> {
    match engine {
        "mariadb" | "postgres" => Ok(()),
        other => Err(format!(
            "{other} has no backup support here (SQL servers only)"
        )),
    }
}

pub fn backups_dir(paths: &AppPaths, engine: &str) -> PathBuf {
    paths.backups_dir().join(engine)
}

/// The dump tool that sits beside the engine's command-line client.
fn dump_tool(client: &Path, engine: &str) -> PathBuf {
    client.with_file_name(match engine {
        "mariadb" => "mariadb-dump.exe",
        _ => "pg_dump.exe",
    })
}

/// `<database>.<secs>.sql` → (database, secs).
fn parse_name(name: &str) -> Option<(String, u64)> {
    let stem = name.strip_suffix(".sql")?;
    let (database, secs) = stem.rsplit_once('.')?;
    Some((database.to_string(), secs.parse().ok()?))
}

/// Backups of `engine`, newest first; only one database's when `database` is given.
pub fn list(paths: &AppPaths, engine: &str, database: Option<&str>) -> Vec<DbBackup> {
    let dir = backups_dir(paths, engine);
    let mut found: Vec<DbBackup> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_string();
            let (db, created) = parse_name(&name)?;
            if database.is_some_and(|wanted| wanted != db) {
                return None;
            }
            Some(DbBackup {
                file: e.path().display().to_string(),
                database: db,
                created,
                size: e.metadata().map(|m| m.len()).unwrap_or(0),
            })
        })
        .collect();
    found.sort_by(|a, b| b.created.cmp(&a.created).then_with(|| b.file.cmp(&a.file)));
    found
}

/// Windows will not put these in a file name, and a `/` or `..` in a database name would let
/// a backup file climb out of this engine's backup folder. Everything else is a legal database
/// name and a legal file name (`-`, `.`, spaces, accents), so backups allow it: the name reaches
/// the dump tool as a process argument, never as SQL text.
fn file_stem(database: &str) -> Result<String, String> {
    let name = database.trim();
    let bad = |c: char| {
        matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control()
    };
    if name.is_empty() {
        return Err("no database was named. Click Back up next to a database in the list.".into());
    }
    if name.len() > 120 || name.starts_with('.') || name.chars().any(bad) {
        return Err(format!(
            "\"{database}\" cannot be backed up: a backup is a file named after the database, and that name holds characters a Windows file name will not accept (/ \\ : * ? \" < > |). Rename the database to letters, digits, - or _, then back up again."
        ));
    }
    Ok(name.to_string())
}

fn new_backup_path(paths: &AppPaths, engine: &str, database: &str) -> Result<PathBuf, String> {
    let dir = backups_dir(paths, engine);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut secs = crate::ca::unix_now();
    // Two backups in the same second must not overwrite each other.
    loop {
        let path = dir.join(format!("{database}.{secs}.sql"));
        if !path.exists() {
            return Ok(path);
        }
        secs += 1;
    }
}

/// Dumps one database to a chosen path. The server has to be running.
///
/// Split out of [`backup`] so a *move* can reuse the same dump shape without leaving a file in
/// the user's backups list — the file is a temporary, not something they asked to keep.
pub fn dump_to(
    services: &ServiceManager,
    engine: &str,
    database: &str,
    dest: &Path,
) -> Result<(), String> {
    check_engine(engine)?;
    let _stem = file_stem(database)?;
    if !services.is_running(engine) {
        return Err(format!("{engine} is not running. Start it first."));
    }
    let (client, port) = services.sql_client(engine)?;
    let tool = dump_tool(&client, engine);
    if !tool.is_file() {
        return Err(format!(
            "{} is missing from the install",
            tool.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("the dump tool")
        ));
    }
    let dest_arg = dest.display().to_string();
    let port_s = port.to_string();
    let args: Vec<String> = if engine == "postgres" {
        // `--clean --if-exists` makes the dump replace what's there; `--no-owner` lets any user restore it.
        [
            "-U",
            "postgres",
            "-h",
            "127.0.0.1",
            "-p",
            port_s.as_str(),
            "--clean",
            "--if-exists",
            "--no-owner",
            "-f",
            dest_arg.as_str(),
            database,
        ]
        .iter()
        .map(|a| a.to_string())
        .collect()
    } else {
        let result_file = format!("--result-file={dest_arg}");
        [
            "-u",
            "root",
            "-h",
            "127.0.0.1",
            "-P",
            port_s.as_str(),
            "--single-transaction",
            "--routines",
            "--triggers",
            "--events",
            "--default-character-set=utf8mb4",
            result_file.as_str(),
            database,
        ]
        .iter()
        .map(|a| a.to_string())
        .collect()
    };
    let out = run_capture(&tool, &args, None, &[], Duration::from_secs(60 * 30));
    if !out.success() {
        let _ = std::fs::remove_file(dest);
        return Err(format!("backup failed: {}", out.combined()));
    }
    Ok(())
}

/// Dumps one database. The server has to be running.
pub fn backup(
    services: &ServiceManager,
    paths: &AppPaths,
    engine: &str,
    database: &str,
) -> Result<PathBuf, String> {
    let dest = new_backup_path(paths, engine, &file_stem(database)?)?;
    dump_to(services, engine, database, &dest)?;
    Ok(dest)
}

/// Loads a dump file into a database that already exists, without touching anything else —
/// no safety copy, because the caller owns the database's state (a *move* has just emptied
/// it on purpose, and a backup of an empty database is noise).
pub fn load_into(
    services: &ServiceManager,
    engine: &str,
    database: &str,
    file: &Path,
) -> Result<(), String> {
    check_engine(engine)?;
    if !file.is_file() {
        return Err(format!("{} does not exist", file.display()));
    }
    if !services.is_running(engine) {
        return Err(format!("{engine} is not running. Start it first."));
    }
    let (client, port) = services.sql_client(engine)?;
    let file_arg = file.display().to_string();
    let port_s = port.to_string();
    let mut cmd = Command::new(&client);
    if engine == "postgres" {
        cmd.args([
            "-U",
            "postgres",
            "-h",
            "127.0.0.1",
            "-p",
            port_s.as_str(),
            "-X",
            "-q",
            "-v",
            "ON_ERROR_STOP=1",
            "-d",
            database,
            "-f",
            file_arg.as_str(),
        ]);
        cmd.stdin(Stdio::null());
    } else {
        cmd.args([
            "-u",
            "root",
            "-h",
            "127.0.0.1",
            "-P",
            port_s.as_str(),
            "--default-character-set=utf8mb4",
            database,
        ]);
        cmd.stdin(Stdio::from(
            std::fs::File::open(file).map_err(|e| e.to_string())?,
        ));
    }
    hide_window(&mut cmd);
    let out = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("could not run {}: {e}", client.display()))?;
    if !out.status.success() {
        let detail = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(format!("load failed: {detail}"));
    }
    Ok(())
}

/// Loads a backup into `database` (created when missing), replacing its tables. Returns the
/// safety backup of what was there before, when there was anything.
pub fn restore(
    services: &ServiceManager,
    paths: &AppPaths,
    engine: &str,
    database: &str,
    file: &Path,
) -> Result<Option<PathBuf>, String> {
    check_engine(engine)?;
    let _stem = file_stem(database)?;
    if !file.is_file() {
        return Err(format!("{} does not exist", file.display()));
    }
    if file
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase)
        .as_deref()
        != Some("sql")
    {
        return Err("only .sql backups can be restored".into());
    }
    if !services.is_running(engine) {
        return Err(format!("{engine} is not running. Start it first."));
    }

    let existing = services.list_databases(engine)?;
    let safety = if existing.iter().any(|d| d == database) {
        Some(backup(services, paths, engine, database).map_err(|e| {
            format!("could not take a safety backup first, so nothing was restored ({e})")
        })?)
    } else {
        services.create_database(engine, database)?;
        None
    };

    let (client, port) = services.sql_client(engine)?;
    let file_arg = file.display().to_string();
    let port_s = port.to_string();
    let mut cmd = Command::new(&client);
    if engine == "postgres" {
        cmd.args([
            "-U",
            "postgres",
            "-h",
            "127.0.0.1",
            "-p",
            port_s.as_str(),
            "-X",
            "-q",
            "-v",
            "ON_ERROR_STOP=1",
            "-d",
            database,
            "-f",
            file_arg.as_str(),
        ]);
        cmd.stdin(Stdio::null());
    } else {
        cmd.args([
            "-u",
            "root",
            "-h",
            "127.0.0.1",
            "-P",
            port_s.as_str(),
            "--default-character-set=utf8mb4",
            database,
        ]);
        cmd.stdin(Stdio::from(
            std::fs::File::open(file).map_err(|e| e.to_string())?,
        ));
    }
    hide_window(&mut cmd);
    let out = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("could not run {}: {e}", client.display()))?;
    if !out.status.success() {
        let detail = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let undo = safety
            .as_ref()
            .map(|p| {
                format!(
                    " The database as it was before is saved at {}.",
                    p.display()
                )
            })
            .unwrap_or_default();
        return Err(format!("restore failed: {detail}.{undo}"));
    }
    Ok(safety)
}

/// Deletes a backup file, but only one that sits in this engine's backup folder.
pub fn delete(paths: &AppPaths, engine: &str, file: &Path) -> Result<(), String> {
    check_engine(engine)?;
    let dir = backups_dir(paths, engine);
    let inside = match (file.canonicalize(), dir.canonicalize()) {
        (Ok(f), Ok(d)) => f.starts_with(d),
        _ => false,
    };
    if !inside || file.extension().and_then(|e| e.to_str()) != Some("sql") {
        return Err("that file is not one of this engine's backups".into());
    }
    std::fs::remove_file(file).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_stem_allows_real_database_names() {
        for name in ["shop", "shop_test", "code-nova", "shop 2", "متجري", "a.b"] {
            assert_eq!(file_stem(name).unwrap(), name, "{name} should back up");
        }
    }

    #[test]
    fn file_stem_rejects_names_a_file_cannot_hold() {
        assert!(file_stem("").unwrap_err().contains("no database was named"));
        assert!(file_stem("   ")
            .unwrap_err()
            .contains("no database was named"));
        for name in [
            "../escape",
            "a/b",
            "a\\b",
            "c:name",
            ".hidden",
            "a*b?",
            "a\u{0}b",
            &"x".repeat(200),
        ] {
            let err = file_stem(name).unwrap_err();
            assert!(err.contains("Rename"), "{name} -> {err}");
        }
    }

    #[test]
    fn names_round_trip_and_reject_other_files() {
        assert_eq!(
            parse_name("shop.1700000000.sql"),
            Some(("shop".into(), 1_700_000_000))
        );
        assert_eq!(parse_name("shop.sql"), None);
        assert_eq!(parse_name("shop.abc.sql"), None);
        assert_eq!(parse_name("shop.1700000000.txt"), None);
    }

    #[test]
    fn lists_newest_first_and_filters_by_database() {
        let home = crate::test_support::isolated_home();
        let dir = backups_dir(&home.paths, "mariadb");
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["a.100.sql", "a.300.sql", "b.200.sql", "notes.txt"] {
            std::fs::write(dir.join(name), "x").unwrap();
        }
        let all = list(&home.paths, "mariadb", None);
        assert_eq!(
            all.iter().map(|b| b.created).collect::<Vec<_>>(),
            vec![300, 200, 100]
        );
        let only_a = list(&home.paths, "mariadb", Some("a"));
        assert_eq!(only_a.len(), 2);
        assert!(list(&home.paths, "postgres", None).is_empty());
    }

    #[test]
    fn delete_only_touches_the_engines_backup_folder() {
        let home = crate::test_support::isolated_home();
        let dir = backups_dir(&home.paths, "mariadb");
        std::fs::create_dir_all(&dir).unwrap();
        let inside = dir.join("a.1.sql");
        std::fs::write(&inside, "x").unwrap();
        let outside = home.paths.root().join("other.sql");
        std::fs::write(&outside, "x").unwrap();

        assert!(delete(&home.paths, "mariadb", &outside).is_err());
        assert!(outside.is_file());
        assert!(delete(&home.paths, "mariadb", &inside).is_ok());
        assert!(!inside.exists());
        assert!(delete(&home.paths, "mongodb", &inside).is_err());
    }
}
