//! Pull a server database over SSH, optionally anonymized (§177).
//!
//! Limits for 2.12: a Linux/macOS server with `sh`, SSH key authentication only
//! (`BatchMode=yes`), MariaDB/MySQL and PostgreSQL. The database password goes to the remote
//! `sh` on stdin, never in a command line, a file or a log, and lives in the OS keyring.
//! The downloaded dump is deleted as soon as it is imported (or when the pull fails).

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::app::Inner;
use crate::error::CoreError;
use crate::service::is_safe_identifier;
use crate::tasks::TaskHandle;

pub const SETTING: &str = "projects.remote_dbs";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteDb {
    /// Slug of the name: `[a-z0-9-]{1,32}`.
    #[serde(default)]
    pub id: String,
    /// Shown to the user; never used in a command.
    pub name: String,
    /// `deploy@1.2.3.4` or an alias from `~/.ssh/config`.
    pub ssh_target: String,
    #[serde(default)]
    pub ssh_port: Option<u16>,
    /// `mariadb` | `postgres`.
    pub engine: String,
    pub database: String,
    pub db_user: String,
    /// As seen from the server. Default `127.0.0.1`.
    #[serde(default = "local_host")]
    pub db_host: String,
    #[serde(default)]
    pub db_port: Option<u16>,
    /// The password itself is in the keyring.
    #[serde(default)]
    pub has_password: bool,
}

fn local_host() -> String {
    "127.0.0.1".into()
}

// ------------------------------------------------------------------ validation

fn bad(field: &str, why: &str) -> CoreError {
    CoreError::failed_fix(
        format!("The {field} is not valid."),
        why.to_string(),
        "Fix it in the server profile and save again.",
    )
}

fn word_chars(s: &str, extra: &[char]) -> bool {
    s.chars()
        .all(|c| c.is_ascii_alphanumeric() || extra.contains(&c))
}

/// Checks every field. Runs before any process starts.
pub fn validate(r: &RemoteDb) -> Result<(), CoreError> {
    let t = &r.ssh_target;
    let (user, host) = match t.split_once('@') {
        Some((u, h)) => (Some(u), h),
        None => (None, t.as_str()),
    };
    if t.is_empty()
        || t.len() > 255
        || t.starts_with('-')
        || host.is_empty()
        || user.is_some_and(|u| u.is_empty())
        || !word_chars(host, &['.', '_', '-'])
        || user.is_some_and(|u| !word_chars(u, &['.', '_', '-']))
    {
        return Err(bad(
            "SSH address",
            "Use user@host or a name from your SSH config: letters, digits and . _ - only, not starting with -.",
        ));
    }
    if !matches!(r.engine.as_str(), "mariadb" | "postgres") {
        return Err(bad("database type", "Use MariaDB/MySQL or PostgreSQL."));
    }
    if !is_safe_identifier(&r.database) || r.database.starts_with(['.', '-']) {
        return Err(bad(
            "database name",
            "Letters, digits and _ - . only, at most 64 characters, not starting with . or -.",
        ));
    }
    if r.db_user.is_empty() || r.db_user.len() > 64 || !word_chars(&r.db_user, &['_', '.', '-']) {
        return Err(bad(
            "database user",
            "Letters, digits and _ . - only, at most 64 characters.",
        ));
    }
    if r.db_host.is_empty() || r.db_host.len() > 253 || !word_chars(&r.db_host, &['.', '-']) {
        return Err(bad("database host", "Letters, digits, . and - only."));
    }
    if r.ssh_port == Some(0) || r.db_port == Some(0) {
        return Err(bad("port", "Ports are 1 to 65535."));
    }
    let n = r.name.chars().count();
    if !(1..=40).contains(&n) || r.name.chars().any(|c| c.is_control()) {
        return Err(bad("name", "Use 1 to 40 printable characters."));
    }
    Ok(())
}

fn slug_id(name: &str) -> String {
    let mut out = String::new();
    for c in name.to_lowercase().chars() {
        let c = if c.is_ascii_alphanumeric() { c } else { '-' };
        if c == '-' && out.ends_with('-') {
            continue;
        }
        out.push(c);
    }
    let out = out.trim_matches('-');
    let out: String = out.chars().take(32).collect();
    if out.is_empty() {
        "server".into()
    } else {
        out
    }
}

// ------------------------------------------------------------------ the remote command

/// One POSIX single-quoted word. Safe for any text: a quote becomes `'"'"'`.
pub fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\"'\"'"))
}

/// The command sent to the server: `sh -c '<script>'`. Built only from validated values.
pub fn remote_command(r: &RemoteDb, schema_only: bool) -> Result<String, CoreError> {
    validate(r)?;
    let host = sh_quote(&r.db_host);
    let user = sh_quote(&r.db_user);
    let db = sh_quote(&r.database);
    let script = if r.engine == "postgres" {
        let port = sh_quote(&r.db_port.unwrap_or(5432).to_string());
        let extra = if schema_only { " --schema-only" } else { "" };
        format!(
            "IFS= read -r P; export PGPASSWORD=\"$P\"; exec pg_dump --no-owner --no-privileges --clean --if-exists{extra} -h {host} -p {port} -U {user} {db}"
        )
    } else {
        let port = sh_quote(&r.db_port.unwrap_or(3306).to_string());
        let extra = if schema_only { " --no-data" } else { "" };
        format!(
            "IFS= read -r P; export MYSQL_PWD=\"$P\"; if command -v mariadb-dump >/dev/null 2>&1; then D=mariadb-dump; else D=mysqldump; fi; exec \"$D\" --single-transaction --quick --routines --triggers --no-tablespaces --default-character-set=utf8mb4{extra} -h {host} -P {port} -u {user} {db}"
        )
    };
    Ok(format!("sh -c {}", sh_quote(&script)))
}

/// Arguments for `ssh`. The password is not among them.
pub fn ssh_args(r: &RemoteDb, remote: &str) -> Vec<String> {
    let mut a: Vec<String> = [
        "-o",
        "BatchMode=yes",
        "-o",
        "ConnectTimeout=15",
        "-o",
        "ServerAliveInterval=30",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    if let Some(p) = r.ssh_port {
        a.push("-p".into());
        a.push(p.to_string());
    }
    a.push("--".into());
    a.push(r.ssh_target.clone());
    a.push(remote.to_string());
    a
}

fn ssh_path(inner: &Inner) -> Result<PathBuf, CoreError> {
    let system = PathBuf::from(r"C:\Windows\System32\OpenSSH\ssh.exe");
    if system.is_file() {
        return Ok(system);
    }
    if let Some(path) = std::env::var_os("PATH") {
        if let Some(p) = std::env::split_paths(&path)
            .map(|d| d.join("ssh.exe"))
            .find(|p| p.is_file())
        {
            return Ok(p);
        }
    }
    if let Some(git) = inner.git_path() {
        for dir in git.ancestors().skip(1).take(3) {
            let p = dir.join("usr").join("bin").join("ssh.exe");
            if p.is_file() {
                return Ok(p);
            }
        }
    }
    Err(CoreError::failed_fix(
        "OLS cannot find an SSH program.",
        "Pulling a server database needs the OpenSSH client.",
        "Install the OpenSSH Client in Windows Settings > System > Optional features.",
    ))
}

// ------------------------------------------------------------------ profiles

fn load_all(inner: &Inner) -> std::collections::BTreeMap<String, Vec<RemoteDb>> {
    inner
        .settings
        .lock()
        .unwrap()
        .get(SETTING)
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

fn save_all(
    inner: &Inner,
    all: &std::collections::BTreeMap<String, Vec<RemoteDb>>,
) -> Result<(), CoreError> {
    let value = serde_json::to_value(all)?;
    inner.settings.lock().unwrap().set(SETTING, value)
}

fn secret_key(project_id: &str, id: &str) -> String {
    format!("remote_db:{project_id}:{id}")
}

pub fn list(inner: &Inner, project_id: &str) -> Vec<RemoteDb> {
    load_all(inner).remove(project_id).unwrap_or_default()
}

fn find(inner: &Inner, project_id: &str, id: &str) -> Result<RemoteDb, CoreError> {
    list(inner, project_id)
        .into_iter()
        .find(|r| r.id == id)
        .ok_or_else(|| {
            CoreError::failed_fix(
                "That server profile does not exist.",
                format!("No profile called \"{id}\" is saved for this project."),
                "Pick one from the list, or add it first.",
            )
        })
}

/// Saves a profile. `password`: `None` keeps the stored one, `Some("")` removes it.
pub fn save(
    inner: &Inner,
    project_id: &str,
    mut remote: RemoteDb,
    password: Option<String>,
) -> Result<Vec<RemoteDb>, CoreError> {
    inner.ensure_project_available(project_id)?;
    if remote.id.is_empty() {
        remote.id = slug_id(&remote.name);
    }
    if remote.db_host.trim().is_empty() {
        remote.db_host = local_host();
    }
    validate(&remote)?;
    let mut all = load_all(inner);
    let profiles = all.entry(project_id.to_string()).or_default();
    let previous = profiles.iter().find(|r| r.id == remote.id).cloned();
    remote.has_password = previous.is_some_and(|p| p.has_password);
    let key = secret_key(project_id, &remote.id);
    let store_err = |e: String| {
        CoreError::failed_fix(
            "OLS could not use the password store.",
            e,
            "Check that the Windows credential store is available, then save again.",
        )
    };
    match password.as_deref() {
        Some("") => {
            crate::secrets::delete_secret(&key).map_err(store_err)?;
            remote.has_password = false;
        }
        Some(p) => {
            crate::secrets::set_secret(&key, p).map_err(store_err)?;
            remote.has_password = true;
        }
        None => {}
    }
    match profiles.iter_mut().find(|r| r.id == remote.id) {
        Some(slot) => *slot = remote,
        None => profiles.push(remote),
    }
    let out = profiles.clone();
    save_all(inner, &all)?;
    Ok(out)
}

pub fn delete(inner: &Inner, project_id: &str, id: &str) -> Result<Vec<RemoteDb>, CoreError> {
    let mut all = load_all(inner);
    if let Some(list) = all.get_mut(project_id) {
        list.retain(|r| r.id != id);
    }
    let _ = crate::secrets::delete_secret(&secret_key(project_id, id));
    let out = all.get(project_id).cloned().unwrap_or_default();
    save_all(inner, &all)?;
    Ok(out)
}

// ------------------------------------------------------------------ the download

/// Deletes the dump file whenever it goes out of scope: success, failure or cancel.
pub struct DumpFile(pub PathBuf);

impl Drop for DumpFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Removes dumps a crashed pull left behind (one `read_dir`).
pub fn cleanup_leftovers(cache_dir: &Path) {
    let Ok(rd) = std::fs::read_dir(cache_dir) else {
        return;
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with("pull-") && (name.ends_with(".sql") || name.ends_with(".sql.part")) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

fn friendly(stderr: &str) -> (String, String) {
    let tail: Vec<&str> = stderr
        .lines()
        .rev()
        .take(20)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let text = crate::redact::redact_text(&tail.join("\n"));
    let lower = text.to_lowercase();
    let fix = if lower.contains("permission denied") && lower.contains("publickey") {
        "Add your SSH key to the server (ssh-copy-id), then try again."
    } else if lower.contains("command not found") || lower.contains("not found") {
        "mysqldump or pg_dump is not installed on the server. Install the database client tools there."
    } else if lower.contains("access denied") || lower.contains("authentication failed") {
        "Check the database user and password in the server profile."
    } else if lower.contains("timed out") || lower.contains("could not resolve") {
        "Check the server address and that you can reach it from this computer."
    } else {
        "Run the same ssh command in a terminal to see the full error, then fix the profile."
    };
    (text, fix.to_string())
}

/// Streams the dump into `dest`. Returns the bytes received.
fn download(
    inner: &Inner,
    task: Option<&TaskHandle>,
    r: &RemoteDb,
    password: &str,
    dest: &Path,
    schema_only: bool,
    timeout: Duration,
) -> Result<u64, CoreError> {
    let ssh = ssh_path(inner)?;
    let args = ssh_args(r, &remote_command(r, schema_only)?);
    let file = std::fs::File::create(dest)?;
    let mut cmd = Command::new(ssh);
    cmd.args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::from(file))
        .stderr(Stdio::piped());
    crate::exec::hide_window(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| {
        CoreError::failed_fix(
            "OLS could not start SSH.",
            e.to_string(),
            "Install the OpenSSH Client in Windows Settings > System > Optional features.",
        )
    })?;
    if let Some(mut stdin) = child.stdin.take() {
        // Password then newline, then stdin closes. Errors here show up as the exit code.
        let _ = stdin.write_all(password.as_bytes());
        let _ = stdin.write_all(b"\n");
    }
    let mut stderr = child.stderr.take();
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(s) = stderr.as_mut() {
            let mut buf = Vec::new();
            let _ = s.take(64 * 1024).read_to_end(&mut buf);
            text = String::from_utf8_lossy(&buf).to_string();
        }
        text
    });
    let started = Instant::now();
    let mut last = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => {}
            Err(e) => return Err(CoreError::from(e)),
        }
        if task.is_some_and(|h| h.cancel_requested()) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(CoreError::failed_fix(
                "The pull was cancelled.",
                "You stopped it before it finished.",
                "Start it again when you are ready.",
            ));
        }
        if started.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(CoreError::failed_fix(
                "The server took too long.",
                format!("No finished dump after {} seconds.", timeout.as_secs()),
                "Check the connection and try again.",
            ));
        }
        if last.elapsed() >= Duration::from_secs(1) {
            last = Instant::now();
            if let Some(h) = task {
                h.set_bytes(std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0), None);
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let err_text = reader.join().unwrap_or_default();
    if !status.success() {
        let (text, fix) = friendly(&err_text);
        return Err(CoreError::failed_fix(
            "The server did not give a database dump.",
            if text.is_empty() {
                format!("SSH ended with {status}.")
            } else {
                text
            },
            fix,
        ));
    }
    Ok(std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0))
}

fn password_for(project_id: &str, r: &RemoteDb) -> Result<String, CoreError> {
    if !r.has_password {
        return Ok(String::new());
    }
    match crate::secrets::get_secret(&secret_key(project_id, &r.id)) {
        Ok(Some(p)) => Ok(p),
        Ok(None) => Err(CoreError::failed_fix(
            "The saved password is missing.",
            "The Windows credential store has no password for this profile.",
            "Edit the profile and enter the password again.",
        )),
        Err(e) => Err(CoreError::failed_fix(
            "OLS could not read the password.",
            e,
            "Check that the Windows credential store is available.",
        )),
    }
}

/// A quick check that the SSH login and the dump tool work (schema only, 30 s).
pub fn test(inner: &Inner, project_id: &str, id: &str) -> Result<String, CoreError> {
    let r = find(inner, project_id, id)?;
    let pw = password_for(project_id, &r)?;
    std::fs::create_dir_all(inner.paths.cache_dir())?;
    let guard = DumpFile(
        inner
            .paths
            .cache_dir()
            .join(format!("pull-test-{}.sql.part", std::process::id())),
    );
    let bytes = download(
        inner,
        None,
        &r,
        &pw,
        &guard.0,
        true,
        Duration::from_secs(30),
    )?;
    Ok(format!(
        "Connected to {}. The server answered ({bytes} bytes of structure).",
        r.name
    ))
}

// ------------------------------------------------------------------ anonymizing rules file

pub fn rules_path(root: &Path) -> PathBuf {
    crate::manifest::dir(root).join("anonymize.yaml")
}

pub fn get_rules(inner: &Inner, project_id: &str) -> Result<Option<String>, CoreError> {
    let p = inner.ensure_project_available(project_id)?;
    Ok(std::fs::read_to_string(rules_path(Path::new(&p.path))).ok())
}

pub fn save_rules(inner: &Inner, project_id: &str, yaml: &str) -> Result<(), CoreError> {
    let p = inner.ensure_project_available(project_id)?;
    crate::anonymize::parse_rules(yaml).map_err(|e| {
        CoreError::failed_fix(
            "The anonymizing rules are not valid.",
            e,
            "Fix the rules and save again. Nothing was written.",
        )
    })?;
    let path = rules_path(Path::new(&p.path));
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, yaml)?;
    Ok(())
}

fn columns_of(
    inner: &Inner,
    engine: &str,
    database: &str,
) -> Result<Vec<(String, String)>, String> {
    let sql = if engine == "postgres" {
        "SELECT table_name, column_name FROM information_schema.columns WHERE table_schema = 'public' ORDER BY table_name, ordinal_position"
    } else {
        "SELECT table_name, column_name FROM information_schema.columns WHERE table_schema = DATABASE() ORDER BY table_name, ordinal_position"
    };
    let out = inner
        .services
        .run_sql_in(engine, database, sql, Duration::from_secs(60))?;
    Ok(out
        .lines()
        .filter_map(|l| {
            let (t, c) = l.split_once('\t')?;
            Some((t.trim().to_string(), c.trim().to_string()))
        })
        .collect())
}

/// Draft rules from the local database's columns, as YAML for the user to review.
pub fn suggest_rules(inner: &Inner, project_id: &str) -> Result<String, CoreError> {
    let db = inner.project_database(project_id).ok_or_else(|| {
        CoreError::failed_fix(
            "OLS cannot find this project's database.",
            "No database is set in the project's .env or manifest.",
            "Set DB_DATABASE in .env, then try again.",
        )
    })?;
    if !matches!(db.engine.as_str(), "mariadb" | "postgres")
        || !inner.services.is_running(&db.engine)
    {
        return Err(CoreError::failed_fix(
            "OLS cannot read the local database's columns.",
            format!("{} must be MariaDB or PostgreSQL, and running.", db.engine),
            "Start the database on the Services page, then try again.",
        ));
    }
    let cols = columns_of(inner, &db.engine, &db.name).map_err(|e| {
        CoreError::failed_fix(
            "OLS could not read the columns.",
            e,
            "Check the local database exists.",
        )
    })?;
    Ok(crate::anonymize::suggest(&cols))
}

// ------------------------------------------------------------------ the pull

/// Pulls `remote_id` into the project's local database. Runs inside a background task.
pub fn pull(
    inner: &Arc<Inner>,
    h: &TaskHandle,
    project_id: &str,
    remote_id: &str,
    mode: &str,
    anonymize: bool,
    confirm_real_data: bool,
) -> Result<String, CoreError> {
    // 1. Preflight.
    if !anonymize && !confirm_real_data {
        return Err(CoreError::failed_fix(
            "OLS did not pull the database.",
            "Without anonymizing, this copies real personal data onto your computer, and you did not confirm that.",
            "Turn on Anonymize, or confirm that you understand and are allowed to hold the data.",
        ));
    }
    if !matches!(mode, "replace" | "new") {
        return Err(bad("pull mode", "Use replace or new."));
    }
    h.begin_step("Checking the profile");
    let r = find(inner, project_id, remote_id)?;
    validate(&r)?;
    let pw = password_for(project_id, &r)?;
    let local = inner.project_database(project_id).ok_or_else(|| {
        CoreError::failed_fix(
            "OLS cannot find this project's database.",
            "No database is set in the project's .env or manifest.",
            "Set DB_DATABASE in .env, then try again.",
        )
    })?;
    if local.engine != r.engine {
        return Err(CoreError::failed_fix(
            "The database types do not match.",
            format!(
                "The project uses {}, the server profile is {}.",
                local.engine, r.engine
            ),
            "Use a profile of the same type as the project's database.",
        ));
    }
    if !inner.services.is_running(&r.engine) {
        return Err(CoreError::failed_fix(
            "The local database server is not running.",
            format!("{} must run to import the dump.", r.engine),
            format!("Start {} on the Services page, then try again.", r.engine),
        ));
    }
    if !crate::network::check(false).online {
        return Err(CoreError::failed_fix(
            "You are offline.",
            "Pulling a database needs a network connection to the server.",
            "Connect to the network, then try again.",
        ));
    }
    let rules = if anonymize {
        let project = inner.ensure_project_available(project_id)?;
        let text = std::fs::read_to_string(rules_path(Path::new(&project.path))).map_err(|_| {
            CoreError::failed_fix(
                "There are no anonymizing rules for this project.",
                "The file .openlocalserver/anonymize.yaml does not exist.",
                "Create the rules first (Create rules), then pull again.",
            )
        })?;
        Some(crate::anonymize::parse_rules(&text).map_err(|e| {
            CoreError::failed_fix(
                "The anonymizing rules are not valid.",
                e,
                "Fix the rules, then pull again.",
            )
        })?)
    } else {
        None
    };

    // 2. Download.
    h.begin_step("Downloading the database");
    std::fs::create_dir_all(inner.paths.cache_dir())?;
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let dump = DumpFile(
        inner
            .paths
            .cache_dir()
            .join(format!("pull-{project_id}-{secs}.sql.part")),
    );
    download(
        inner,
        Some(h),
        &r,
        &pw,
        &dump.0,
        false,
        Duration::from_secs(4 * 3600),
    )?;
    h.check_cancelled()?;

    // 3. Import into a temporary local database.
    h.begin_step("Importing into a temporary database");
    let temp = inner
        .services
        .free_database_name(&r.engine, &format!("{}__pull", local.name));
    inner
        .services
        .create_database(&r.engine, &temp)
        .map_err(|e| {
            CoreError::failed_fix(
                "OLS could not create the temporary database.",
                e,
                "Check the local database server.",
            )
        })?;
    let drop_temp = |why: CoreError| {
        let _ = inner.services.drop_database(&r.engine, &temp);
        why
    };
    crate::dbbackup::load_into(&inner.services, &r.engine, &temp, &dump.0).map_err(|e| {
        drop_temp(CoreError::failed_fix(
            "OLS could not import the dump.",
            e,
            "Your local database was not changed.",
        ))
    })?;
    drop(dump);

    // 4. Anonymize the temporary copy.
    if let Some(rules) = rules {
        h.begin_step("Replacing personal data with fake values");
        let existing = columns_of(inner, &r.engine, &temp).map_err(|e| {
            drop_temp(CoreError::failed_fix(
                "OLS could not read the imported columns.",
                e,
                "Your local database was not changed.",
            ))
        })?;
        crate::anonymize::check_columns(&rules, &existing).map_err(|e| {
            drop_temp(CoreError::failed_fix(
                "The rules do not match the server's database.",
                e,
                "Fix the rules (or the table names), then pull again. Your local database was not changed.",
            ))
        })?;
        let statements = crate::anonymize::build_sql(&rules, &r.engine).map_err(|e| {
            drop_temp(CoreError::failed_fix(
                "The rules cannot be run.",
                e,
                "Fix the rules.",
            ))
        })?;
        let script = crate::anonymize::script(&statements, &r.engine);
        inner
            .services
            .run_sql_in(&r.engine, &temp, &script, Duration::from_secs(1800))
            .map_err(|e| {
                drop_temp(CoreError::failed_fix(
                    "Anonymizing failed.",
                    e,
                    "Your local database was not changed.",
                ))
            })?;
    }

    // 5. Put it in place.
    h.begin_step("Putting the database in place");
    if mode == "replace" {
        let exists = inner
            .services
            .list_databases(&r.engine)
            .map(|l| l.contains(&local.name))
            .unwrap_or(false);
        if exists {
            crate::dbbackup::backup(&inner.services, &inner.paths, &r.engine, &local.name)
                .map_err(|e| {
                    drop_temp(CoreError::failed_fix(
                        "OLS could not back up your local database first.",
                        e,
                        "Nothing was changed.",
                    ))
                })?;
            inner
                .services
                .drop_database(&r.engine, &local.name)
                .map_err(|e| {
                    drop_temp(CoreError::failed_fix(
                        "OLS could not remove the old local database.",
                        e,
                        "Nothing was changed.",
                    ))
                })?;
        }
        inner
            .services
            .rename_database(&r.engine, &temp, &local.name)
            .map_err(|e| {
                CoreError::failed_fix(
                    "OLS could not finish the replacement.",
                    e,
                    format!("The pulled copy is still in the database {temp}; the old one is in Backups."),
                )
            })?;
        Ok(format!(
            "Replaced {} with {} from {}.",
            local.name,
            if anonymize {
                "anonymized data"
            } else {
                "real data"
            },
            r.name
        ))
    } else {
        let name = inner
            .services
            .free_database_name(&r.engine, &format!("{}_{}", local.name, r.id));
        inner
            .services
            .rename_database(&r.engine, &temp, &name)
            .map_err(|e| {
                drop_temp(CoreError::failed_fix(
                    "OLS could not name the new database.",
                    e,
                    "Nothing was changed.",
                ))
            })?;
        Ok(format!(
            "Created the database {name} from {}. Use it by setting DB_DATABASE to {name} in .env.",
            r.name
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> RemoteDb {
        RemoteDb {
            id: "staging".into(),
            name: "Staging".into(),
            ssh_target: "deploy@1.2.3.4".into(),
            ssh_port: Some(2222),
            engine: "mariadb".into(),
            database: "shop".into(),
            db_user: "app".into(),
            db_host: "127.0.0.1".into(),
            db_port: None,
            has_password: true,
        }
    }

    /// A tiny POSIX word splitter: single quotes, double quotes and bare words.
    fn split(line: &str) -> Vec<String> {
        let (mut words, mut cur, mut have) = (Vec::new(), String::new(), false);
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\'' => {
                    have = true;
                    for q in chars.by_ref() {
                        if q == '\'' {
                            break;
                        }
                        cur.push(q);
                    }
                }
                '"' => {
                    have = true;
                    for q in chars.by_ref() {
                        if q == '"' {
                            break;
                        }
                        cur.push(q);
                    }
                }
                c if c.is_whitespace() => {
                    if have {
                        words.push(std::mem::take(&mut cur));
                        have = false;
                    }
                }
                c => {
                    have = true;
                    cur.push(c);
                }
            }
        }
        if have {
            words.push(cur);
        }
        words
    }

    #[test]
    fn dangerous_fields_are_rejected_and_normal_ones_pass() {
        assert!(validate(&profile()).is_ok());
        let mut r = profile();
        r.ssh_target = "-oProxyCommand=calc".into();
        assert!(validate(&r).is_err());
        let mut r = profile();
        r.database = "db'; drop".into();
        assert!(validate(&r).is_err());
        let mut r = profile();
        r.ssh_target = "a@b@c".into();
        assert!(validate(&r).is_err());
        let mut r = profile();
        r.db_user = "app;ls".into();
        assert!(validate(&r).is_err());
        let mut r = profile();
        r.db_host = "10.0.0.1 -x".into();
        assert!(validate(&r).is_err());
        let mut r = profile();
        r.db_port = Some(0);
        assert!(validate(&r).is_err());
        let mut r = profile();
        r.database = "-x".into();
        assert!(validate(&r).is_err());
        let mut r = profile();
        r.ssh_target = "my-alias".into();
        assert!(validate(&r).is_ok());
    }

    #[test]
    fn the_remote_command_is_exact_and_round_trips_through_a_shell_parser() {
        let cmd = remote_command(&profile(), false).unwrap();
        let script = "IFS= read -r P; export MYSQL_PWD=\"$P\"; if command -v mariadb-dump >/dev/null 2>&1; then D=mariadb-dump; else D=mysqldump; fi; exec \"$D\" --single-transaction --quick --routines --triggers --no-tablespaces --default-character-set=utf8mb4 -h '127.0.0.1' -P '3306' -u 'app' 'shop'";
        assert_eq!(cmd, format!("sh -c {}", sh_quote(script)));
        // The outer layer parses back to `sh`, `-c` and the exact script.
        let words = split(&cmd);
        assert_eq!(words, vec!["sh", "-c", script]);
        let pg = RemoteDb {
            engine: "postgres".into(),
            ..profile()
        };
        let cmd = remote_command(&pg, true).unwrap();
        assert!(cmd.contains("PGPASSWORD"));
        let script = &split(&cmd)[2];
        assert!(
            script.contains("--schema-only")
                && script.ends_with("-h '127.0.0.1' -p '5432' -U 'app' 'shop'")
        );
        assert_eq!(split(&sh_quote("it's")), vec!["it's"]);
    }

    #[test]
    fn the_password_is_never_in_the_arguments() {
        let r = profile();
        let remote = remote_command(&r, false).unwrap();
        let args = ssh_args(&r, &remote);
        for a in &args {
            assert!(!a.contains("hunter2"));
        }
        assert_eq!(&args[..2], ["-o", "BatchMode=yes"]);
        assert!(args.windows(2).any(|w| w == ["-p", "2222"]));
        let dd = args.iter().position(|a| a == "--").unwrap();
        assert_eq!(args[dd + 1], "deploy@1.2.3.4");
    }

    #[test]
    fn the_dump_file_is_deleted_on_drop_and_leftovers_are_cleaned() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("pull-a-1.sql.part");
        std::fs::write(&f, "x").unwrap();
        {
            let _g = DumpFile(f.clone());
        }
        assert!(!f.exists());
        std::fs::write(dir.path().join("pull-a-1.sql"), "x").unwrap();
        std::fs::write(dir.path().join("pull-b-2.sql.part"), "x").unwrap();
        std::fs::write(dir.path().join("keep.txt"), "x").unwrap();
        cleanup_leftovers(dir.path());
        assert!(dir.path().join("keep.txt").exists());
        assert!(!dir.path().join("pull-a-1.sql").exists());
        assert!(!dir.path().join("pull-b-2.sql.part").exists());
    }

    #[test]
    fn ids_are_slugs() {
        assert_eq!(slug_id("My Staging!"), "my-staging");
        assert_eq!(slug_id("!!!"), "server");
        assert!(slug_id(&"a".repeat(80)).len() <= 32);
    }
}
