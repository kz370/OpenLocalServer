//! Migration guard (§168): before OLS runs a command that changes a database's structure
//! (`php artisan migrate`, `manage.py migrate`, `prisma migrate dev`, …) it saves a copy of
//! the project's database. **Rewind** puts the copy back.
//!
//! Copies live under `backups/guard/<engine>/<database>.<unix-secs>.sql` (or `.sqlite`) with a
//! `.json` sidecar describing them. Only guard copies are ever pruned; the normal
//! `backups/<engine>/` files are never touched.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::app::Inner;
use crate::error::CoreError;
use crate::paths::AppPaths;
use crate::snapshots::is_plain_file_name;

pub const SETTING_ENABLED: &str = "db.migration_guard";
pub const SETTING_MAX_MB: &str = "db.migration_guard.max_mb";
pub const SETTING_KEEP: &str = "db.migration_guard.keep";
const DEFAULT_MAX_MB: u64 = 1024;
const DEFAULT_KEEP: u64 = 10;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardSnapshot {
    /// File name without extension, e.g. `shop.1762600000`.
    pub id: String,
    pub project_id: String,
    /// `mariadb`, `postgres` or `sqlite`.
    pub engine: String,
    /// The database name, or the SQLite path relative to the project.
    pub database: String,
    /// Absolute path of the copy.
    pub file: String,
    /// Unix seconds.
    pub created: u64,
    pub size: u64,
    /// The command line, with secrets removed.
    pub command: String,
    /// Label from [`migration_kind`].
    pub kind: String,
}

// ------------------------------------------------------------------ detection

/// `C:\tools\PHP.EXE` → `php`.
fn normalize_program(program: &str) -> String {
    let name = program
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(program)
        .to_lowercase();
    for ext in [".exe", ".cmd", ".bat"] {
        if let Some(stem) = name.strip_suffix(ext) {
            return stem.to_string();
        }
    }
    name
}

fn is_one_of(value: Option<&&str>, list: &[&str]) -> bool {
    value.is_some_and(|v| list.contains(v))
}

/// Which kind of schema-changing command `program args` is, if any. Only the listed verbs
/// match: `migrate:status`, `--pretend` and the like never do.
pub fn migration_kind(program: &str, args: &[String], project_root: &Path) -> Option<&'static str> {
    kind_at_depth(program, args, project_root, true)
}

/// A whole command line (a `package.json` script, say) run through [`migration_kind`].
/// Scripts are not looked up again from here: recursion depth is 1.
pub fn migration_in_line(line: &str, project_root: &Path) -> Option<&'static str> {
    let tokens = crate::quickapp::plan::split_command_line(line);
    let (program, args) = tokens.split_first()?;
    kind_at_depth(program, args, project_root, false)
}

fn kind_at_depth(
    program: &str,
    args: &[String],
    root: &Path,
    allow_script: bool,
) -> Option<&'static str> {
    let p = normalize_program(program);
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    match p.as_str() {
        "php" => match a.first().copied() {
            Some("artisan") => is_one_of(
                a.get(1),
                &[
                    "migrate",
                    "migrate:fresh",
                    "migrate:refresh",
                    "migrate:reset",
                    "migrate:rollback",
                    "db:wipe",
                    "db:seed",
                ],
            )
            .then_some("laravel"),
            Some("bin/console") | Some("bin\\console") => is_one_of(
                a.get(1),
                &[
                    "doctrine:migrations:migrate",
                    "doctrine:schema:update",
                    "doctrine:database:drop",
                    "doctrine:fixtures:load",
                ],
            )
            .then_some("symfony"),
            Some(phar) if phar.to_lowercase().ends_with("wp-cli.phar") => {
                wp_cli(&a[1..]).then_some("wp-cli")
            }
            _ => None,
        },
        "python" | "python3" | "py" => (a.first() == Some(&"manage.py")
            && is_one_of(a.get(1), &["migrate", "flush", "loaddata", "reset_db"]))
        .then_some("django"),
        // Inside a package script, `node_modules/.bin` is on PATH, so the tool stands alone.
        "prisma" | "sequelize" | "sequelize-cli" => {
            let mut full = vec![p.as_str()];
            full.extend(a.iter().copied());
            node_tool(&full)
        }
        "alembic" => is_one_of(a.first(), &["upgrade", "downgrade"]).then_some("alembic"),
        "wp" => wp_cli(&a).then_some("wp-cli"),
        "npx" | "pnpm" | "bunx" | "yarn" | "npm" => {
            let mut rest: &[&str] = &a;
            if matches!(p.as_str(), "pnpm" | "yarn") && is_one_of(rest.first(), &["exec", "dlx"]) {
                rest = &rest[1..];
            }
            if p != "npm" {
                if let Some(k) = node_tool(rest) {
                    return Some(k);
                }
            }
            if allow_script && matches!(p.as_str(), "npm" | "pnpm" | "yarn") {
                let script = match rest.first().copied() {
                    Some("run") | Some("run-script") => rest.get(1).copied(),
                    Some(s) if p != "npm" && !s.starts_with('-') => Some(s),
                    _ => None,
                }?;
                return script_kind(root, script);
            }
            None
        }
        _ => None,
    }
}

fn wp_cli(a: &[&str]) -> bool {
    a.first() == Some(&"db") && is_one_of(a.get(1), &["reset", "import"])
}

fn node_tool(a: &[&str]) -> Option<&'static str> {
    match a.first().copied() {
        Some("prisma") => {
            let ok = (a.get(1) == Some(&"migrate")
                && is_one_of(a.get(2), &["dev", "reset", "deploy"]))
                || (a.get(1) == Some(&"db") && a.get(2) == Some(&"push"));
            ok.then_some("prisma")
        }
        Some("sequelize") | Some("sequelize-cli") => is_one_of(
            a.get(1),
            &[
                "db:migrate",
                "db:migrate:undo",
                "db:migrate:undo:all",
                "db:seed:all",
            ],
        )
        .then_some("sequelize"),
        _ => None,
    }
}

/// `package.json` `scripts[name]`, run through [`migration_in_line`] (no further lookups).
fn script_kind(root: &Path, name: &str) -> Option<&'static str> {
    let text = std::fs::read_to_string(root.join("package.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    let line = json.get("scripts")?.get(name)?.as_str()?;
    migration_in_line(line, root).map(|_| "script")
}

// ------------------------------------------------------------------ storage

fn guard_dir(paths: &AppPaths) -> PathBuf {
    paths.backups_dir().join("guard")
}

fn engine_dir(paths: &AppPaths, engine: &str) -> PathBuf {
    guard_dir(paths).join(engine)
}

fn now_secs() -> u64 {
    crate::ca::unix_now()
}

/// Every guard copy, newest first; one project's when `project_id` is given.
pub fn list(paths: &AppPaths, project_id: Option<&str>) -> Vec<GuardSnapshot> {
    let mut out = Vec::new();
    for engine in ["mariadb", "postgres", "sqlite"] {
        let Ok(rd) = std::fs::read_dir(engine_dir(paths, engine)) else {
            continue;
        };
        for e in rd.flatten() {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) != Some("json") {
                continue;
            }
            let Some(snap) = std::fs::read_to_string(&path)
                .ok()
                .and_then(|t| serde_json::from_str::<GuardSnapshot>(&t).ok())
            else {
                continue;
            };
            // The copy itself must still be there and must sit inside this engine's folder.
            if !Path::new(&snap.file).is_file() {
                continue;
            }
            if project_id.is_some_and(|p| p != snap.project_id) {
                continue;
            }
            out.push(snap);
        }
    }
    out.sort_by(|a, b| b.created.cmp(&a.created).then_with(|| b.id.cmp(&a.id)));
    out
}

/// The copy called `id`. The id is checked as a plain file name and then looked up in the
/// list, so no path is ever built from user text.
pub fn find(paths: &AppPaths, id: &str) -> Result<GuardSnapshot, CoreError> {
    if !is_plain_file_name(id) {
        return Err(CoreError::failed_fix(
            "That saved copy name is not valid.",
            format!("\"{id}\" is not the name of a copy OLS saved."),
            "Pick a copy from the list in the project's Backups tab.",
        ));
    }
    list(paths, None)
        .into_iter()
        .find(|s| s.id == id)
        .ok_or_else(|| {
            CoreError::failed_fix(
                "That saved copy no longer exists.",
                format!("No copy called \"{id}\" was found. It may have been deleted or replaced by newer ones."),
                "Refresh the list and pick another copy.",
            )
        })
}

fn remove_copy(snap: &GuardSnapshot) {
    let file = PathBuf::from(&snap.file);
    let _ = std::fs::remove_file(file.with_extension("json"));
    let _ = std::fs::remove_file(&file);
}

pub fn delete(paths: &AppPaths, id: &str) -> Result<(), CoreError> {
    let snap = find(paths, id)?;
    remove_copy(&snap);
    Ok(())
}

/// Keeps the newest `keep` copies of one (engine, database); deletes the rest.
pub fn prune(paths: &AppPaths, engine: &str, database: &str, keep: usize) {
    let mine: Vec<GuardSnapshot> = list(paths, None)
        .into_iter()
        .filter(|s| s.engine == engine && s.database == database)
        .collect();
    for old in mine.into_iter().skip(keep.max(1)) {
        remove_copy(&old);
    }
}

fn unique_stem(paths: &AppPaths, name: &str) -> (String, u64) {
    let mut secs = now_secs();
    loop {
        let stem = format!("{name}.{secs}");
        let taken = ["mariadb", "postgres", "sqlite"]
            .iter()
            .any(|e| engine_dir(paths, e).join(format!("{stem}.json")).exists());
        if !taken {
            return (stem, secs);
        }
        secs += 1;
    }
}

// ------------------------------------------------------------------ SQLite

/// Relative path (as configured) of a project's SQLite file: `DB_DATABASE`, or Laravel's
/// default `database/database.sqlite`.
fn sqlite_relative(root: &Path) -> String {
    crate::setup::env_value(root, "DB_DATABASE")
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "database/database.sqlite".into())
}

pub(crate) fn sqlite_full(root: &Path, configured: &str) -> PathBuf {
    let p = Path::new(configured);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        root.join(p)
    }
}

fn side_file(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(suffix);
    PathBuf::from(s)
}

/// Copies a SQLite file (and its `-wal` / `-shm` when present) to `dest`.
pub(crate) fn copy_sqlite(src: &Path, dest: &Path) -> std::io::Result<()> {
    std::fs::copy(src, dest)?;
    for suffix in ["-wal", "-shm"] {
        let side = side_file(src, suffix);
        if side.is_file() {
            std::fs::copy(&side, side_file(dest, suffix))?;
        }
    }
    Ok(())
}

/// Puts a guard copy over the live SQLite file, keeping the live one as
/// `<file>.before-rewind-<secs>` first.
fn rewind_sqlite(guard_file: &Path, live: &Path) -> std::io::Result<()> {
    if live.is_file() {
        let keep = side_file(live, &format!(".before-rewind-{}", now_secs()));
        copy_sqlite(live, &keep)?;
    }
    copy_sqlite(guard_file, live)?;
    // A stale journal from the replaced file would be replayed over the restored one.
    for suffix in ["-wal", "-shm"] {
        if !side_file(guard_file, suffix).is_file() {
            let _ = std::fs::remove_file(side_file(live, suffix));
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ the copy

fn refuse(cause: impl Into<String>, engine: &str) -> CoreError {
    CoreError::failed_fix(
        "OLS did not run this migration.",
        cause,
        format!("Start {engine} on the Services page and run it again, or turn off Migration guard in Settings → Databases."),
    )
}

/// Size of a SQL database in bytes, when the server can say.
fn sql_size(inner: &Inner, engine: &str, db: &str) -> Option<u64> {
    if !crate::service::is_safe_identifier(db) {
        return None;
    }
    let sql = if engine == "postgres" {
        format!("SELECT pg_database_size('{db}')")
    } else {
        format!("SELECT COALESCE(SUM(data_length+index_length),0) FROM information_schema.tables WHERE table_schema='{db}'")
    };
    let out = inner.services.run_sql_query(engine, None, &sql).ok()?;
    out.lines()
        .rev()
        .find_map(|l| l.split_whitespace().last()?.parse::<u64>().ok())
}

/// Saves the project's database before a migration. `Ok(None)`: nothing to save (no database
/// is configured, or it is bigger than the limit). `Err`: the copy could not be made and the
/// command must not start.
pub fn take_guard_copy(
    inner: &Inner,
    project_id: &str,
    kind: &str,
    line: &str,
) -> Result<Option<GuardSnapshot>, CoreError> {
    let Some(db) = inner.project_database(project_id) else {
        return Ok(None);
    };
    if db.engine == "mongodb" {
        return Ok(None);
    }
    let project = inner.ensure_project_available(project_id)?;
    let root = PathBuf::from(&project.path);
    let keep = inner
        .settings
        .lock()
        .unwrap()
        .get(SETTING_KEEP)
        .and_then(|v| v.as_u64())
        .unwrap_or(DEFAULT_KEEP)
        .clamp(1, 50) as usize;
    let max_mb = inner
        .settings
        .lock()
        .unwrap()
        .get(SETTING_MAX_MB)
        .and_then(|v| v.as_u64())
        .unwrap_or(DEFAULT_MAX_MB);
    let command = crate::redact::redact_text(line);
    let dir = engine_dir(&inner.paths, &db.engine);

    let (database, name, extension) = if db.engine == "sqlite" {
        let relative = sqlite_relative(&root);
        let live = sqlite_full(&root, &relative);
        if !live.is_file() {
            return Ok(None);
        }
        let stem = live
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("database")
            .to_string();
        (relative, stem, "sqlite")
    } else {
        (db.name.clone(), db.name.clone(), "sql")
    };
    if !is_plain_file_name(&name) {
        return Err(refuse(
            format!(
                "The database name \"{name}\" cannot be used as a file name for the saved copy."
            ),
            &db.engine,
        ));
    }
    std::fs::create_dir_all(&dir)?;
    let (stem, created) = unique_stem(&inner.paths, &name);
    let file = dir.join(format!("{stem}.{extension}"));

    if db.engine == "sqlite" {
        copy_sqlite(&sqlite_full(&root, &database), &file).map_err(|e| {
            refuse(
                format!("Copying the SQLite file failed: {e}"),
                "the database",
            )
        })?;
    } else {
        if !inner.services.is_running(&db.engine) {
            return Err(refuse(
                format!(
                    "{} is not running, so the database could not be saved first.",
                    db.engine
                ),
                &db.engine,
            ));
        }
        if let Some(bytes) = sql_size(inner, &db.engine, &db.name) {
            if bytes > max_mb.saturating_mul(1024 * 1024) {
                tracing::info!(database = %db.name, bytes, "migration guard skipped: database over the size limit");
                return Ok(None);
            }
        }
        let part = file.with_extension("sql.part");
        if let Err(e) = crate::dbbackup::dump_to(&inner.services, &db.engine, &db.name, &part) {
            let _ = std::fs::remove_file(&part);
            return Err(refuse(e, &db.engine));
        }
        if let Err(e) = std::fs::rename(&part, &file) {
            let _ = std::fs::remove_file(&part);
            return Err(refuse(
                format!("Could not finish the saved copy: {e}"),
                &db.engine,
            ));
        }
    }

    let snap = GuardSnapshot {
        id: stem,
        project_id: project_id.to_string(),
        engine: db.engine.clone(),
        database,
        file: file.display().to_string(),
        created,
        size: std::fs::metadata(&file).map(|m| m.len()).unwrap_or(0),
        command,
        kind: kind.to_string(),
    };
    let sidecar = file.with_extension("json");
    let written = serde_json::to_string_pretty(&snap)
        .map_err(CoreError::from)
        .and_then(|t| std::fs::write(&sidecar, t).map_err(CoreError::from));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&file);
        return Err(e);
    }
    prune(&inner.paths, &snap.engine, &snap.database, keep);
    Ok(Some(snap))
}

/// Puts a guard copy back. SQL databases go through the normal restore (which saves the
/// current state first); SQLite keeps the current file as `<file>.before-rewind-<secs>`.
pub fn rewind(inner: &Inner, id: &str) -> Result<String, CoreError> {
    let snap = find(&inner.paths, id)?;
    if snap.engine == "sqlite" {
        let project = inner.ensure_project_available(&snap.project_id)?;
        let live = sqlite_full(Path::new(&project.path), &snap.database);
        rewind_sqlite(Path::new(&snap.file), &live).map_err(|e| {
            CoreError::failed_fix(
                "OLS could not put the database back.",
                format!("Copying the saved file failed: {e}"),
                "Close anything that has the database file open, then try again.",
            )
        })?;
        return Ok(format!("Put back {} as it was.", snap.database));
    }
    if !inner.services.is_running(&snap.engine) {
        inner
            .start_service_and_wait(&snap.engine, &mut |_| {})
            .map_err(|e| {
                CoreError::failed_fix(
                    "OLS could not put the database back.",
                    format!(
                        "{} is not running and could not be started: {e}",
                        snap.engine
                    ),
                    format!("Start {} on the Services page and try again.", snap.engine),
                )
            })?;
    }
    let safety = crate::dbbackup::restore(
        &inner.services,
        &inner.paths,
        &snap.engine,
        &snap.database,
        Path::new(&snap.file),
    )
    .map_err(|e| {
        CoreError::failed_fix(
            "OLS could not put the database back.",
            e,
            "Check that the database server is running, then try again.",
        )
    })?;
    Ok(match safety {
        Some(p) => format!(
            "Put back {}. The state before is saved at {}.",
            snap.database,
            p.display()
        ),
        None => format!("Put back {}.", snap.database),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(program: &str, args: &[&str]) -> Option<&'static str> {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        migration_kind(program, &args, Path::new("."))
    }

    #[test]
    fn every_listed_command_matches() {
        for v in [
            "migrate",
            "migrate:fresh",
            "migrate:refresh",
            "migrate:reset",
            "migrate:rollback",
            "db:wipe",
            "db:seed",
        ] {
            assert_eq!(kind("php", &["artisan", v]), Some("laravel"), "{v}");
        }
        assert_eq!(
            kind("php", &["bin/console", "doctrine:migrations:migrate"]),
            Some("symfony")
        );
        assert_eq!(
            kind("php", &["bin\\console", "doctrine:fixtures:load"]),
            Some("symfony")
        );
        for v in ["migrate", "flush", "loaddata", "reset_db"] {
            assert_eq!(kind("python", &["manage.py", v]), Some("django"));
        }
        assert_eq!(kind("py", &["manage.py", "migrate"]), Some("django"));
        assert_eq!(kind("npx", &["prisma", "migrate", "dev"]), Some("prisma"));
        assert_eq!(
            kind("pnpm", &["dlx", "prisma", "db", "push"]),
            Some("prisma")
        );
        assert_eq!(
            kind("yarn", &["exec", "prisma", "migrate", "deploy"]),
            Some("prisma")
        );
        assert_eq!(
            kind("npx", &["sequelize-cli", "db:migrate:undo:all"]),
            Some("sequelize")
        );
        assert_eq!(kind("alembic", &["upgrade", "head"]), Some("alembic"));
        assert_eq!(kind("wp", &["db", "reset"]), Some("wp-cli"));
        assert_eq!(
            kind("php", &["wp-cli.phar", "db", "import"]),
            Some("wp-cli")
        );
    }

    #[test]
    fn read_only_commands_never_match() {
        assert_eq!(kind("php", &["artisan", "migrate:status"]), None);
        assert_eq!(kind("php", &["artisan", "migrate:install"]), None);
        assert_eq!(kind("php", &["artisan", "route:list"]), None);
        assert_eq!(kind("python", &["manage.py", "showmigrations"]), None);
        assert_eq!(kind("npx", &["prisma", "migrate", "status"]), None);
        assert_eq!(kind("wp", &["db", "check"]), None);
        assert_eq!(kind("alembic", &["current"]), None);
    }

    #[test]
    fn program_names_are_normalized() {
        assert_eq!(kind("PHP.EXE", &["artisan", "migrate"]), Some("laravel"));
        assert_eq!(kind("php.cmd", &["artisan", "migrate"]), Some("laravel"));
        assert_eq!(
            kind("C:\\tools\\php\\php.exe", &["artisan", "migrate"]),
            Some("laravel")
        );
    }

    #[test]
    fn package_scripts_match_one_level_deep() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"scripts":{"db":"prisma migrate dev","other":"npm run db","plain":"vite"}}"#,
        )
        .unwrap();
        let k = |p: &str, a: &[&str]| {
            let a: Vec<String> = a.iter().map(|s| s.to_string()).collect();
            migration_kind(p, &a, dir.path())
        };
        assert_eq!(k("npm", &["run", "plain"]), None);
        assert_eq!(k("npm", &["run", "other"]), None, "no recursion");
        assert_eq!(k("npm", &["run", "db"]), Some("script"));
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"scripts":{"db":"npx prisma migrate dev","php":"php artisan migrate"}}"#,
        )
        .unwrap();
        assert_eq!(k("npm", &["run", "db"]), Some("script"));
        assert_eq!(k("yarn", &["php"]), Some("script"));
        assert_eq!(k("pnpm", &["run", "php"]), Some("script"));
    }

    fn snap(paths: &AppPaths, engine: &str, db: &str, secs: u64) -> GuardSnapshot {
        let dir = engine_dir(paths, engine);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join(format!("{db}.{secs}.sql"));
        std::fs::write(&file, "x").unwrap();
        let s = GuardSnapshot {
            id: format!("{db}.{secs}"),
            project_id: "p1".into(),
            engine: engine.into(),
            database: db.into(),
            file: file.display().to_string(),
            created: secs,
            size: 1,
            command: "php artisan migrate".into(),
            kind: "laravel".into(),
        };
        std::fs::write(
            file.with_extension("json"),
            serde_json::to_string(&s).unwrap(),
        )
        .unwrap();
        s
    }

    #[test]
    fn prune_keeps_the_newest_and_never_touches_normal_backups() {
        let home = crate::test_support::isolated_home();
        let normal = crate::dbbackup::backups_dir(&home.paths, "mariadb");
        std::fs::create_dir_all(&normal).unwrap();
        std::fs::write(normal.join("shop.1.sql"), "x").unwrap();
        for secs in 100..105 {
            snap(&home.paths, "mariadb", "shop", secs);
        }
        snap(&home.paths, "mariadb", "other", 50);
        prune(&home.paths, "mariadb", "shop", 2);
        let left: Vec<u64> = list(&home.paths, None)
            .iter()
            .filter(|s| s.database == "shop")
            .map(|s| s.created)
            .collect();
        assert_eq!(left, vec![104, 103]);
        assert_eq!(list(&home.paths, None).len(), 3, "other db untouched");
        assert!(normal.join("shop.1.sql").is_file());
    }

    #[test]
    fn find_rejects_ids_that_climb_out() {
        let home = crate::test_support::isolated_home();
        snap(&home.paths, "mariadb", "shop", 100);
        assert!(find(&home.paths, "..\\..\\x").is_err());
        assert!(find(&home.paths, "../x").is_err());
        assert!(find(&home.paths, "").is_err());
        assert!(find(&home.paths, "shop.999").is_err());
        assert_eq!(find(&home.paths, "shop.100").unwrap().database, "shop");
        assert!(delete(&home.paths, "shop.100").is_ok());
        assert!(find(&home.paths, "shop.100").is_err());
    }

    #[test]
    fn sqlite_copy_and_rewind_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let live = dir.path().join("db.sqlite");
        std::fs::write(&live, "before").unwrap();
        let copy = dir.path().join("copy.sqlite");
        copy_sqlite(&live, &copy).unwrap();
        std::fs::write(&live, "after migration").unwrap();
        std::fs::write(side_file(&live, "-wal"), "stale").unwrap();
        rewind_sqlite(&copy, &live).unwrap();
        assert_eq!(std::fs::read_to_string(&live).unwrap(), "before");
        assert!(!side_file(&live, "-wal").exists(), "stale journal removed");
        let kept = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .find(|e| e.file_name().to_string_lossy().contains(".before-rewind-"))
            .expect("current state kept");
        assert_eq!(
            std::fs::read_to_string(kept.path()).unwrap(),
            "after migration"
        );
    }
}
