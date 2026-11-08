//! Database per Git branch (§176): switching to `feature/login` points the project's `.env`
//! at its own copy of the database, so migrations on one branch never break another.
//!
//! Opt-in per project (setting `projects.branch_db`). One thread, `ols-branch-db`, watches
//! the `.git` folder (non-recursive) of enabled projects for `HEAD` changes and only runs
//! while at least one project is enabled. `.env` is edited losslessly with `envfile::set`.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::app::Inner;
use crate::error::CoreError;
use crate::service::is_safe_identifier;

pub const SETTING: &str = "projects.branch_db";
const MAX_NAME: usize = 64;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BranchDbConfig {
    #[serde(default)]
    pub enabled: bool,
    /// `mariadb` | `postgres` | `sqlite`.
    #[serde(default)]
    pub engine: String,
    /// The branch that was active when it was turned on, e.g. `main`.
    #[serde(default)]
    pub base_branch: String,
    /// Database name, or the SQLite path as written in `.env`.
    #[serde(default)]
    pub base_database: String,
    /// `base` (default) or `current`.
    #[serde(default)]
    pub clone_from: String,
    /// branch -> database. Never contains `base_branch`.
    #[serde(default)]
    pub map: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchDbInfo {
    pub config: Option<BranchDbConfig>,
    pub current_branch: Option<String>,
    pub current_database: Option<String>,
    /// Mapped branches that no longer exist in the repository.
    pub orphaned: Vec<String>,
    /// A branch OLS could not switch yet (its database server was off).
    pub pending: Option<String>,
    /// Laravel's config cache exists and still points at the old database.
    pub config_cache: bool,
}

// ------------------------------------------------------------------ pure helpers

/// `ref: refs/heads/<name>` -> `<name>`; a detached HEAD (a hash) -> `None`.
pub fn parse_head(text: &str) -> Option<String> {
    let line = text.lines().next()?.trim();
    let name = line
        .strip_prefix("ref:")?
        .trim()
        .strip_prefix("refs/heads/")?;
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_string())
}

fn slug(branch: &str) -> String {
    let mut out = String::new();
    for c in branch.to_lowercase().chars() {
        let c = if c.is_ascii_alphanumeric() { c } else { '_' };
        if c == '_' && out.ends_with('_') {
            continue;
        }
        out.push(c);
    }
    out.trim_matches('_').to_string()
}

fn hash6(branch: &str) -> String {
    Sha256::digest(branch.as_bytes())
        .iter()
        .take(3)
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// `shop` + `feature/Login-Page` -> `shop__feature_login_page`. Over 64 characters the slug
/// is cut and a short hash of the branch name keeps it unique and stable.
pub fn branch_db_name(base: &str, branch: &str) -> String {
    let s = slug(branch);
    let full = format!("{base}__{s}");
    if full.len() <= MAX_NAME && !s.is_empty() {
        return full;
    }
    let h = hash6(branch);
    let base_cut: String = base.chars().take(40).collect();
    let room = MAX_NAME.saturating_sub(base_cut.len() + 2 + 1 + h.len());
    let cut: String = s.chars().take(room).collect();
    let cut = cut.trim_matches('_');
    if cut.is_empty() {
        format!("{base_cut}__{h}")
    } else {
        format!("{base_cut}__{cut}_{h}")
    }
}

/// The SQLite file for a branch, next to the base file and in the same relative/absolute form.
pub fn sqlite_branch_path(base: &str, branch: &str) -> String {
    let s = slug(branch);
    let h = hash6(branch);
    let s: String = s.chars().take(40).collect();
    let tag = if s.is_empty() { h } else { s };
    let (dir, file) = match base.rfind(['/', '\\']) {
        Some(i) => (&base[..=i], &base[i + 1..]),
        None => ("", base),
    };
    match file.rfind('.') {
        Some(i) if i > 0 => format!("{dir}{}__{tag}{}", &file[..i], &file[i..]),
        _ => format!("{dir}{file}__{tag}"),
    }
}

// ------------------------------------------------------------------ config store

fn load_all(inner: &Inner) -> BTreeMap<String, BranchDbConfig> {
    inner
        .settings
        .lock()
        .unwrap()
        .get(SETTING)
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

fn save_all(inner: &Inner, all: &BTreeMap<String, BranchDbConfig>) -> Result<(), CoreError> {
    let value = serde_json::to_value(all)?;
    inner.settings.lock().unwrap().set(SETTING, value)
}

fn update_config(
    inner: &Inner,
    project_id: &str,
    f: impl FnOnce(&mut BranchDbConfig),
) -> Result<(), CoreError> {
    let mut all = load_all(inner);
    if let Some(c) = all.get_mut(project_id) {
        f(c);
        save_all(inner, &all)?;
    }
    Ok(())
}

fn pending() -> &'static Mutex<HashMap<String, String>> {
    static P: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    P.get_or_init(|| Mutex::new(HashMap::new()))
}

fn refuse(problem: &str, cause: &str, fix: &str) -> CoreError {
    CoreError::failed_fix(problem, cause, fix)
}

// ------------------------------------------------------------------ project facts

struct Facts {
    root: PathBuf,
    git_dir: PathBuf,
}

fn facts(inner: &Inner, project_id: &str) -> Result<Facts, CoreError> {
    let project = inner.ensure_project_available(project_id)?;
    let root = PathBuf::from(project.path);
    let git_dir = root.join(".git");
    Ok(Facts { root, git_dir })
}

fn current_branch(git_dir: &Path) -> Option<String> {
    parse_head(&std::fs::read_to_string(git_dir.join("HEAD")).ok()?)
}

fn env_db(root: &Path) -> Option<String> {
    crate::setup::env_value(root, "DB_DATABASE").filter(|v| !v.is_empty())
}

fn set_env_db(root: &Path, value: &str) -> Result<(), CoreError> {
    let path = root.join(".env");
    let text = std::fs::read_to_string(&path)?;
    let next = crate::envfile::set(&text, "DB_DATABASE", value).map_err(|e| {
        refuse(
            "OLS could not update .env.",
            &e,
            "Check that .env is a valid file.",
        )
    })?;
    std::fs::write(path, next)?;
    Ok(())
}

fn git_succeeds(inner: &Inner, root: &Path, args: &[&str]) -> Result<bool, CoreError> {
    let git = inner.git_path().ok_or_else(|| {
        refuse(
            "Git was not found.",
            "Database per branch needs Git to check your files.",
            "Install Git for Windows, or the portable Git on the Runtimes page.",
        )
    })?;
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    let out = crate::exec::run_capture(&git, &args, Some(root), &[], Duration::from_secs(20));
    Ok(out.success())
}

fn branches(inner: &Inner, root: &Path) -> Vec<String> {
    let Some(git) = inner.git_path() else {
        return Vec::new();
    };
    let args = [
        "branch".to_string(),
        "--format=%(refname:short)".to_string(),
    ];
    let out = crate::exec::run_capture(&git, &args, Some(root), &[], Duration::from_secs(20));
    if !out.success() {
        return Vec::new();
    }
    out.stdout.lines().map(|l| l.trim().to_string()).collect()
}

fn db_exists(inner: &Inner, root: &Path, engine: &str, name: &str) -> bool {
    if engine == "sqlite" {
        return crate::migration_guard::sqlite_full(root, name).is_file();
    }
    inner
        .services
        .list_databases(engine)
        .map(|l| l.iter().any(|d| d == name))
        .unwrap_or(false)
}

// ------------------------------------------------------------------ commands

pub fn get(inner: &Inner, project_id: &str) -> Result<BranchDbInfo, CoreError> {
    let f = facts(inner, project_id)?;
    let config = load_all(inner).remove(project_id);
    let existing = branches(inner, &f.root);
    let orphaned = match &config {
        Some(c) if !existing.is_empty() => c
            .map
            .keys()
            .filter(|b| !existing.contains(b))
            .cloned()
            .collect(),
        _ => Vec::new(),
    };
    let cache = f
        .root
        .join("bootstrap")
        .join("cache")
        .join("config.php")
        .is_file();
    Ok(BranchDbInfo {
        config,
        current_branch: current_branch(&f.git_dir),
        current_database: env_db(&f.root),
        orphaned,
        pending: pending().lock().unwrap().get(project_id).cloned(),
        config_cache: cache,
    })
}

/// Turns the feature on or off for one project.
pub fn set(
    inner: &Arc<Inner>,
    project_id: &str,
    enabled: bool,
    clone_from: Option<String>,
) -> Result<BranchDbInfo, CoreError> {
    let f = facts(inner, project_id)?;
    let mut all = load_all(inner);
    if !enabled {
        if let Some(c) = all.get_mut(project_id) {
            if c.enabled && !c.base_database.is_empty() && f.root.join(".env").is_file() {
                set_env_db(&f.root, &c.base_database)?;
            }
            c.enabled = false;
            save_all(inner, &all)?;
        }
        pending().lock().unwrap().remove(project_id);
        return get(inner, project_id);
    }
    if !f.git_dir.is_dir() {
        return Err(refuse(
            "Database per branch cannot be turned on for this project.",
            "The project has no .git folder (worktrees and submodules are not supported yet).",
            "Use a normal Git repository, or leave this off.",
        ));
    }
    let Some(branch) = current_branch(&f.git_dir) else {
        return Err(refuse(
            "Database per branch cannot be turned on right now.",
            "Git is on a detached HEAD, so there is no branch to start from.",
            "Switch to a branch, then try again.",
        ));
    };
    if !f.root.join(".env").is_file() {
        return Err(refuse(
            "Database per branch needs a .env file.",
            "The project has no .env, so OLS has nothing to point at a branch database.",
            "Create the .env with DB_DATABASE set, then try again.",
        ));
    }
    let Some(base) = env_db(&f.root) else {
        return Err(refuse(
            "Database per branch needs DB_DATABASE in .env.",
            "DB_DATABASE is empty or missing.",
            "Set DB_DATABASE in .env, then try again.",
        ));
    };
    let engine = match inner.project_database(project_id).map(|d| d.engine) {
        Some(e) if matches!(e.as_str(), "mariadb" | "postgres" | "sqlite") => e,
        _ => {
            return Err(refuse(
                "Database per branch does not support this database.",
                "It works with MariaDB/MySQL, PostgreSQL and SQLite.",
                "Point the project's .env at one of those, or leave this off.",
            ))
        }
    };
    if engine != "sqlite" && !is_safe_identifier(&base) {
        return Err(refuse(
            "The database name cannot be used.",
            "Only letters, digits, _ - . and at most 64 characters are allowed.",
            "Rename the database, then try again.",
        ));
    }
    if git_succeeds(inner, &f.root, &["ls-files", "--error-unmatch", ".env"])? {
        return Err(refuse(
            "OLS would change a file you commit.",
            ".env is tracked by Git, and switching branches edits it.",
            "Stop tracking .env (add it to .gitignore and run `git rm --cached .env`), then try again.",
        ));
    }
    if engine != "sqlite" && !inner.services.is_running(&engine) {
        return Err(refuse(
            "The database server is not running.",
            &format!("{engine} must run so OLS can check the database."),
            &format!("Start {engine} on the Services page, then try again."),
        ));
    }
    if !db_exists(inner, &f.root, &engine, &base) {
        return Err(refuse(
            "The database does not exist yet.",
            &format!("{base} was not found."),
            "Create or import the database first.",
        ));
    }
    let keep = all
        .get(project_id)
        .map(|c| c.map.clone())
        .unwrap_or_default();
    all.insert(
        project_id.to_string(),
        BranchDbConfig {
            enabled: true,
            engine,
            base_branch: branch,
            base_database: base,
            clone_from: match clone_from.as_deref() {
                Some("current") => "current".into(),
                _ => "base".into(),
            },
            map: keep,
        },
    );
    save_all(inner, &all)?;
    #[cfg(not(test))]
    inner.ensure_branch_watcher();
    get(inner, project_id)
}

/// Handles a changed HEAD for one project. Returns a message for the user when something
/// changed. Safe to call repeatedly: it does nothing when `.env` already has the target.
pub fn handle_head_change(
    inner: &Arc<Inner>,
    project_id: &str,
) -> Result<Option<String>, CoreError> {
    let Some(cfg) = load_all(inner).remove(project_id).filter(|c| c.enabled) else {
        return Ok(None);
    };
    let f = facts(inner, project_id)?;
    let Some(branch) = current_branch(&f.git_dir) else {
        return Ok(None);
    };
    let current = env_db(&f.root);
    let sqlite = cfg.engine == "sqlite";
    let mut target = if branch == cfg.base_branch {
        cfg.base_database.clone()
    } else if let Some(t) = cfg.map.get(&branch) {
        t.clone()
    } else if sqlite {
        sqlite_branch_path(&cfg.base_database, &branch)
    } else {
        branch_db_name(&cfg.base_database, &branch)
    };
    if current.as_deref() == Some(target.as_str()) {
        pending().lock().unwrap().remove(project_id);
        return Ok(None);
    }
    let mut copied_from = None;
    if !db_exists(inner, &f.root, &cfg.engine, &target) {
        if !sqlite && !inner.services.is_running(&cfg.engine) {
            pending()
                .lock()
                .unwrap()
                .insert(project_id.to_string(), branch.clone());
            return Ok(Some(format!(
                "Start {} so OLS can switch the database for branch {branch}.",
                cfg.engine
            )));
        }
        let from = if cfg.clone_from == "current" {
            current.clone().unwrap_or_else(|| cfg.base_database.clone())
        } else {
            cfg.base_database.clone()
        };
        if !sqlite && !cfg.map.contains_key(&branch) {
            // A database with this name that OLS did not make is never reused.
            target = inner.services.free_database_name(&cfg.engine, &target);
        }
        if sqlite {
            let src = crate::migration_guard::sqlite_full(&f.root, &from);
            let dest = crate::migration_guard::sqlite_full(&f.root, &target);
            crate::migration_guard::copy_sqlite(&src, &dest).map_err(|e| {
                refuse(
                    "OLS could not copy the SQLite database.",
                    &e.to_string(),
                    "Close anything that has the file open, then switch branches again.",
                )
            })?;
        } else {
            inner
                .services
                .clone_database(&cfg.engine, &from, &target)
                .map_err(|e| {
                    refuse(
                        "OLS could not copy the database for this branch.",
                        &e,
                        "Check that the database server is running, then switch branches again.",
                    )
                })?;
        }
        copied_from = Some(from);
    }
    set_env_db(&f.root, &target)?;
    if branch != cfg.base_branch {
        let (b, t) = (branch.clone(), target.clone());
        update_config(inner, project_id, move |c| {
            c.map.insert(b, t);
        })?;
    }
    pending().lock().unwrap().remove(project_id);
    let workers: Vec<String> = inner
        .workers
        .lock()
        .unwrap()
        .list()
        .into_iter()
        .filter(|w| w.project_id == project_id)
        .map(|w| w.id)
        .filter(|id| inner.worker_procs.is_running(id))
        .collect();
    for id in workers {
        let _ = inner.restart_worker(&id);
    }
    let mut msg = format!("Branch {branch} now uses database {target}");
    if let Some(from) = copied_from {
        msg.push_str(&format!(" (copied from {from})"));
    }
    msg.push('.');
    if f.root
        .join("bootstrap")
        .join("cache")
        .join("config.php")
        .is_file()
    {
        msg.push_str(" Laravel's config cache still points to the old database. Run `php artisan config:clear`.");
    }
    Ok(Some(msg))
}

/// Deletes one branch database after saving a normal backup. Never automatic.
pub fn delete(inner: &Arc<Inner>, project_id: &str, database: &str) -> Result<String, CoreError> {
    let f = facts(inner, project_id)?;
    let cfg = load_all(inner).remove(project_id).ok_or_else(|| {
        refuse(
            "Database per branch is off.",
            "This project has no branch databases.",
            "Turn it on first.",
        )
    })?;
    if !cfg.map.values().any(|d| d == database) {
        return Err(refuse(
            "OLS only deletes databases it made for a branch.",
            &format!("{database} is not in this project's branch list."),
            "Pick one from the list.",
        ));
    }
    if database == cfg.base_database || env_db(&f.root).as_deref() == Some(database) {
        return Err(refuse(
            "That database is in use.",
            "It is the main database or the one .env uses right now.",
            "Switch to another branch first.",
        ));
    }
    if cfg.engine == "sqlite" {
        let file = crate::migration_guard::sqlite_full(&f.root, database);
        let dir = inner.paths.backups_dir().join("sqlite");
        std::fs::create_dir_all(&dir)?;
        let name = file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("branch.sqlite");
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if file.is_file() {
            std::fs::copy(&file, dir.join(format!("{name}.{secs}")))?;
            std::fs::remove_file(&file)?;
        }
    } else {
        if !inner.services.is_running(&cfg.engine) {
            return Err(refuse(
                "The database server is not running.",
                "OLS saves a backup before deleting.",
                &format!("Start {} on the Services page.", cfg.engine),
            ));
        }
        crate::dbbackup::backup(&inner.services, &inner.paths, &cfg.engine, database).map_err(
            |e| {
                refuse(
                    "OLS could not back up the database first.",
                    &e,
                    "Nothing was deleted.",
                )
            },
        )?;
        inner
            .services
            .drop_database(&cfg.engine, database)
            .map_err(|e| refuse("OLS could not delete the database.", &e, "Try again."))?;
    }
    let db = database.to_string();
    update_config(inner, project_id, move |c| c.map.retain(|_, d| *d != db))?;
    Ok(format!("Deleted {database}. A backup was saved first."))
}

// ------------------------------------------------------------------ watcher

fn watcher_needed(inner: &Inner) -> bool {
    load_all(inner).values().any(|c| c.enabled)
}

static WATCHER: AtomicBool = AtomicBool::new(false);

pub fn branch_watcher_running() -> bool {
    WATCHER.load(Ordering::SeqCst)
}

impl Inner {
    /// Starts the HEAD watcher when at least one project is enabled. Cheap when none is.
    pub fn ensure_branch_watcher(self: &Arc<Self>) {
        if !watcher_needed(self) {
            return;
        }
        if WATCHER.swap(true, Ordering::SeqCst) {
            return;
        }
        let inner = Arc::clone(self);
        let spawned = std::thread::Builder::new()
            .name("ols-branch-db".into())
            .spawn(move || {
                watch(&inner);
                WATCHER.store(false, Ordering::SeqCst);
            });
        if spawned.is_err() {
            WATCHER.store(false, Ordering::SeqCst);
        }
    }
}

fn watch(inner: &Arc<Inner>) {
    use notify::RecursiveMode;
    let (tx, rx) = std::sync::mpsc::channel();
    let Ok(mut debouncer) = notify_debouncer_mini::new_debouncer(Duration::from_millis(1500), tx)
    else {
        return;
    };
    let mut watched: HashMap<String, PathBuf> = HashMap::new();
    let mut ticks = 0u32;
    loop {
        if ticks.is_multiple_of(4) {
            let enabled: Vec<String> = load_all(inner)
                .into_iter()
                .filter(|(_, c)| c.enabled)
                .map(|(id, _)| id)
                .collect();
            if enabled.is_empty() {
                return;
            }
            for id in &enabled {
                if watched.contains_key(id) {
                    continue;
                }
                if let Ok(f) = facts(inner, id) {
                    if f.git_dir.is_dir()
                        && debouncer
                            .watcher()
                            .watch(&f.git_dir, RecursiveMode::NonRecursive)
                            .is_ok()
                    {
                        watched.insert(id.clone(), f.git_dir);
                    }
                }
            }
            let gone: Vec<String> = watched
                .keys()
                .filter(|k| !enabled.contains(k))
                .cloned()
                .collect();
            for id in gone {
                if let Some(dir) = watched.remove(&id) {
                    let _ = debouncer.watcher().unwatch(&dir);
                }
            }
        }
        ticks = ticks.wrapping_add(1);
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(Ok(events)) => {
                let heads: Vec<PathBuf> = events
                    .iter()
                    .filter(|e| e.path.file_name().is_some_and(|n| n == "HEAD"))
                    .filter_map(|e| e.path.parent().map(Path::to_path_buf))
                    .collect();
                for (id, dir) in &watched {
                    if heads.contains(dir) {
                        run_switch(inner, id);
                    }
                }
            }
            Ok(Err(_)) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// Runs one switch as a background task so the UI shows its progress and result.
fn run_switch(inner: &Arc<Inner>, project_id: &str) {
    let worker = Arc::clone(inner);
    let id = project_id.to_string();
    inner.tasks.start(
        "branch-db",
        "Switching the branch database",
        project_id,
        1,
        move |h| {
            h.begin_step("Pointing .env at the branch database");
            let r = handle_head_change(&worker, &id);
            if let Ok(Some(text)) = &r {
                h.result(text.clone());
            }
            crate::command::report(&h, r);
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_the_rule_and_stay_valid() {
        assert_eq!(
            branch_db_name("shop", "feature/Login-Page"),
            "shop__feature_login_page"
        );
        let long = "x".repeat(200);
        let a = branch_db_name("shop", &long);
        assert!(a.len() <= 64, "{a}");
        assert_eq!(a, branch_db_name("shop", &long), "stable");
        assert!(is_safe_identifier(&a));
        let uni = branch_db_name("shop", "feature/üñí-ログイン");
        assert!(is_safe_identifier(&uni), "{uni}");
        assert!(is_safe_identifier(&branch_db_name("shop", "///")));
        assert_ne!(
            branch_db_name("shop", &"a".repeat(100)),
            branch_db_name("shop", &"b".repeat(100))
        );
    }

    #[test]
    fn head_parsing() {
        assert_eq!(
            parse_head("ref: refs/heads/main\n").as_deref(),
            Some("main")
        );
        assert_eq!(
            parse_head("ref: refs/heads/feature/x\r\n").as_deref(),
            Some("feature/x")
        );
        assert_eq!(parse_head("ref:   refs/heads/a  \n").as_deref(), Some("a"));
        assert_eq!(
            parse_head("9fceb02d0ae598e95dc970b74767f19372d61af8\n"),
            None
        );
        assert_eq!(parse_head(""), None);
    }

    #[test]
    fn sqlite_names_keep_the_form_of_the_base() {
        assert_eq!(
            sqlite_branch_path("database/database.sqlite", "feature/x"),
            "database/database__feature_x.sqlite"
        );
        assert_eq!(sqlite_branch_path("app.db", "dev"), "app__dev.db");
    }

    fn git(dir: &Path, args: &[&str]) -> bool {
        std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn setup() -> (
        Arc<Inner>,
        crate::test_support::IsolatedHome,
        tempfile::TempDir,
        String,
    ) {
        let home = crate::test_support::isolated_home();
        let settings = crate::settings::SettingsService::load(&home.paths).unwrap();
        let inner = Inner::new(settings, home.paths.clone()).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("shop");
        std::fs::create_dir_all(root.join("database")).unwrap();
        std::fs::write(root.join("database/database.sqlite"), "base-data").unwrap();
        let env = "# keep me\r\nAPP_NAME=Shop\r\nDB_CONNECTION=sqlite\r\nDB_DATABASE=database/database.sqlite\r\n# end\r\n";
        std::fs::write(root.join(".env"), env).unwrap();
        std::fs::write(root.join(".gitignore"), ".env\n").unwrap();
        let id = {
            let mut store = inner.projects.lock().unwrap();
            store.register(root.to_str().unwrap()).unwrap().id
        };
        (inner, home, dir, id)
    }

    #[test]
    fn sqlite_end_to_end_switches_and_restores_env_byte_for_byte() {
        if !git(Path::new("."), &["--version"]) {
            eprintln!("git not found; skipping");
            return;
        }
        let (inner, _home, dir, id) = setup();
        let root = dir.path().join("shop");
        assert!(git(&root, &["init", "-q", "-b", "main"]));
        let original = std::fs::read_to_string(root.join(".env")).unwrap();

        let info = set(&inner, &id, true, None).unwrap();
        assert_eq!(info.config.as_ref().unwrap().base_branch, "main");

        // Simulate `git switch -c feature/login` by changing HEAD.
        std::fs::write(root.join(".git/HEAD"), "ref: refs/heads/feature/login\n").unwrap();
        let msg = handle_head_change(&inner, &id).unwrap().unwrap();
        assert!(msg.contains("feature/login"), "{msg}");
        assert!(root
            .join("database/database__feature_login.sqlite")
            .is_file());
        let env = std::fs::read_to_string(root.join(".env")).unwrap();
        assert!(env.contains("DB_DATABASE=database/database__feature_login.sqlite\r\n"));
        // Running it again changes nothing.
        assert!(handle_head_change(&inner, &id).unwrap().is_none());

        std::fs::write(root.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        handle_head_change(&inner, &id).unwrap().unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join(".env")).unwrap(),
            original
        );

        // Delete refuses the base and the one in use, then removes an unused branch copy.
        assert!(delete(&inner, &id, "database/database.sqlite").is_err());
        assert!(delete(&inner, &id, "database/database__feature_login.sqlite").is_ok());
        assert!(!root
            .join("database/database__feature_login.sqlite")
            .exists());
    }

    #[test]
    fn refusals() {
        if !git(Path::new("."), &["--version"]) {
            eprintln!("git not found; skipping");
            return;
        }
        // .env tracked.
        let (inner, _home, dir, id) = setup();
        let root = dir.path().join("shop");
        assert!(git(&root, &["init", "-q", "-b", "main"]));
        std::fs::write(root.join(".gitignore"), "").unwrap();
        assert!(git(&root, &["add", ".env"]));
        let e = format!("{:?}", set(&inner, &id, true, None).unwrap_err());
        assert!(e.contains("commit"), "{e}");
        // Detached HEAD.
        assert!(git(&root, &["rm", "-q", "--cached", ".env"]));
        std::fs::write(
            root.join(".git/HEAD"),
            "9fceb02d0ae598e95dc970b74767f19372d61af8\n",
        )
        .unwrap();
        assert!(set(&inner, &id, true, None).is_err());
        // .git is a file.
        std::fs::remove_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join(".git"), "gitdir: ../elsewhere\n").unwrap();
        assert!(set(&inner, &id, true, None).is_err());
        assert!(!load_all(&inner).get(&id).is_some_and(|c| c.enabled));
    }

    #[test]
    fn the_watcher_is_not_started_when_nothing_is_enabled() {
        let (inner, _home, _dir, _id) = setup();
        assert!(!watcher_needed(&inner));
        inner.ensure_branch_watcher();
    }
}
