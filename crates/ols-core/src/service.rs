//! Service Manager (§22, §31–39, §61–68 — Stages 5 and 10): long-running background
//! services (Mailpit, MariaDB, PostgreSQL, MongoDB, Redis). Reuses the Runtime Manager's install
//! pipeline (same download → verify → extract, §20–21) and the Process Supervisor's
//! lifecycle (§107) — a service is just a process someone starts and expects to keep
//! running, not a new concept.
//!
//! Simplified vs. the full plan: dependency-ordered startup (§68) isn't implemented yet —
//! each service is started independently. Redis has no official Windows build, so the
//! community `redis-windows` build is used.

use std::collections::HashMap;
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::custom_service::{self, CustomService, CustomServiceStore};
use crate::exec::{run_capture, Captured};
use crate::paths::AppPaths;
use crate::port::port_is_free;
use crate::process::{ProcessId, ProcessSpec, ProcessSupervisor};
use crate::runtime::RuntimeManager;

/// In the order the pages list them. MariaDB is the MySQL-compatible server; MySQL itself
/// is not offered.
const KNOWN_SERVICES: &[&str] = &["mailpit", "mariadb", "postgres", "mongodb", "redis"];

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortStatusLite {
    Free,
    InUse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceStatus {
    pub id: String,
    pub name: String,
    pub installed: bool,
    pub running: bool,
    pub port: Option<u16>,
    pub port_status: Option<PortStatusLite>,
    /// "mail" | "sql" | "document" — what kind of thing this is.
    #[serde(default)]
    pub kind: String,
    /// Where a client connects (§62, §63): a DSN/URL, or the SMTP address for Mailpit.
    #[serde(default)]
    pub connection: Option<String>,
    /// A live TCP probe of the service's port — `None` while it isn't running (§37 health).
    #[serde(default)]
    pub healthy: Option<bool>,
    #[serde(default)]
    pub version: Option<String>,
}

/// Everything an external DB tool needs to open a connection (§102).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionInfo {
    pub engine: String,
    pub host: String,
    pub port: Option<u16>,
    pub user: Option<String>,
    pub database: Option<String>,
    /// SQLite file path.
    pub path: Option<String>,
    /// A ready-made URI for tools that take one (MongoDB Compass).
    pub uri: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbUser {
    pub user: String,
    pub host: String,
}

pub struct ServiceManager {
    paths: AppPaths,
    runtimes: Arc<RuntimeManager>,
    supervisor: Arc<ProcessSupervisor>,
    running: Mutex<HashMap<String, ProcessId>>,
    custom: Mutex<CustomServiceStore>,
    /// Memory limits from Settings → Resources (§129), applied at start.
    limits: Mutex<crate::resources::ResourceLimits>,
}

/// Names that go into SQL as identifiers can't be bound as parameters, so only plain
/// names are accepted at all.
pub fn is_safe_identifier(name: &str) -> bool {
    !name.is_empty() && name.len() <= 64 && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Single-quoted literal for PostgreSQL, where backslashes are ordinary characters.
fn pg_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// Escapes a value for a single-quoted SQL string literal.
fn sql_string(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "''"))
}

impl ServiceManager {
    pub fn new(paths: AppPaths, runtimes: Arc<RuntimeManager>, supervisor: Arc<ProcessSupervisor>) -> Self {
        let custom = Mutex::new(CustomServiceStore::load(&paths));
        Self { paths, runtimes, supervisor, running: Mutex::new(HashMap::new()), custom, limits: Mutex::new(Default::default()) }
    }

    pub fn set_limits(&self, limits: crate::resources::ResourceLimits) {
        *self.limits.lock().unwrap() = limits;
    }

    fn limit_args(&self, id: &str) -> Vec<String> {
        self.limits.lock().unwrap().service_args(id)
    }

    /// The built-in services, then the user's own (§67).
    pub fn list(&self) -> Vec<ServiceStatus> {
        let custom: Vec<String> = self.custom.lock().unwrap().list().into_iter().map(|s| s.id).collect();
        KNOWN_SERVICES.iter().map(|s| s.to_string()).chain(custom).map(|id| self.status(&id)).collect()
    }

    // ------------------------------------------------------------ custom services (§67)

    pub fn list_custom(&self) -> Vec<CustomService> {
        self.custom.lock().unwrap().list()
    }

    pub fn save_custom(&self, service: CustomService) -> Result<CustomService, String> {
        self.custom.lock().unwrap().save(service)
    }

    /// Stops the service if it is running, then forgets its definition.
    pub fn remove_custom(&self, id: &str) -> Result<(), String> {
        self.stop(id);
        self.custom.lock().unwrap().remove(id)
    }

    fn custom_status(&self, def: CustomService) -> ServiceStatus {
        let running = self.is_running(&def.id);
        ServiceStatus {
            installed: std::path::Path::new(&def.executable).is_file(),
            running,
            port: def.port,
            port_status: def.port.map(|p| if port_is_free(p) { PortStatusLite::Free } else { PortStatusLite::InUse }),
            kind: "custom".into(),
            connection: def.port.map(|p| format!("127.0.0.1:{p}")),
            healthy: if running { def.port.and_then(|p| custom_service::probe(p, &def.health)) } else { None },
            version: None,
            id: def.id,
            name: def.name,
        }
    }

    fn start_custom(&self, def: &CustomService) -> Result<ProcessId, String> {
        if !std::path::Path::new(&def.executable).is_file() {
            return Err(format!("{} no longer exists", def.executable));
        }
        if let Some(port) = def.port.filter(|p| !port_is_free(*p)) {
            return Err(format!("port {port} is already in use, so {} can't start", def.name));
        }
        Ok(self.supervisor.start(ProcessSpec {
            name: def.name.clone(),
            executable: def.executable.clone(),
            args: def.args.clone(),
            cwd: def.cwd.clone().filter(|c| !c.is_empty()),
            env: def.env.clone(),
            restart: def.restart_on_crash.then_some(crate::process::RestartPolicy { max_retries: 3, delay_ms: 2000 }),
        }))
    }

    pub fn is_running(&self, id: &str) -> bool {
        let map = self.running.lock().unwrap();
        map.get(id).is_some_and(|p| self.supervisor.is_alive(*p))
    }

    pub fn status(&self, id: &str) -> ServiceStatus {
        if custom_service::is_custom_id(id) {
            let def = self.custom.lock().unwrap().get(id);
            return match def {
                Some(def) => self.custom_status(def),
                None => ServiceStatus {
                    id: id.to_string(),
                    name: id.to_string(),
                    installed: false,
                    running: false,
                    port: None,
                    port_status: None,
                    kind: "custom".into(),
                    connection: None,
                    healthy: None,
                    version: None,
                },
            };
        }
        let versions = self.runtimes.installed_versions(id);
        let installed = !versions.is_empty();
        let running = self.is_running(id);
        let port = primary_port(id);
        ServiceStatus {
            id: id.to_string(),
            name: self.runtimes.display_name(id).unwrap_or_else(|| id.to_string()),
            installed,
            running,
            port,
            // Only free/in-use matters here, so skip the netstat/tasklist owner lookup.
            port_status: port.map(|p| if port_is_free(p) { PortStatusLite::Free } else { PortStatusLite::InUse }),
            kind: kind_of(id).to_string(),
            connection: installed.then(|| connection_string(id)).flatten(),
            healthy: running.then(|| {
                port.is_some_and(|p| TcpStream::connect_timeout(&([127, 0, 0, 1], p).into(), Duration::from_millis(300)).is_ok())
            }),
            version: versions.into_iter().next(),
        }
    }

    /// §61–68 / §101 one-click "Start". Each service needs a different command line, so
    /// this dispatches to a per-service starter — the shared part (recording the
    /// resulting `ProcessId`, refusing a double-start) lives here once.
    pub fn start(&self, id: &str) -> Result<ProcessId, String> {
        if self.is_running(id) {
            return Err(format!("{id} is already running"));
        }
        if custom_service::is_custom_id(id) {
            let def = self.custom.lock().unwrap().get(id).ok_or_else(|| format!("unknown service: {id}"))?;
            let process_id = self.start_custom(&def)?;
            self.running.lock().unwrap().insert(id.to_string(), process_id);
            return Ok(process_id);
        }
        let version = self
            .runtimes
            .installed_versions(id)
            .into_iter()
            .next()
            .ok_or_else(|| format!("{id} is not installed — install it from the Runtimes page first"))?;

        let process_id = match id {
            "mailpit" => self.start_mailpit(&version)?,
            "mariadb" => self.start_mariadb(&version)?,
            "postgres" => self.start_postgres(&version)?,
            "mongodb" => self.start_mongodb(&version)?,
            "redis" => self.start_redis(&version)?,
            other => return Err(format!("unknown service: {other}")),
        };
        self.running.lock().unwrap().insert(id.to_string(), process_id);
        Ok(process_id)
    }

    pub fn stop(&self, id: &str) {
        if let Some(process_id) = self.running.lock().unwrap().remove(id) {
            self.supervisor.stop(process_id);
        }
    }

    /// Called when the caller already knows the process exited (e.g. after seeing a
    /// `ProcessEvent::StateChanged` to a terminal state) — keeps `running` accurate
    /// without this manager needing its own event subscription.
    pub fn mark_stopped(&self, id: &str) {
        self.running.lock().unwrap().remove(id);
    }

    fn start_mailpit(&self, version: &str) -> Result<ProcessId, String> {
        let binary = self.runtimes.binary_path("mailpit", version).ok_or("mailpit.exe missing on disk")?;
        let db_file = self.paths.services_dir().join("mailpit").join("mailpit.db");
        std::fs::create_dir_all(db_file.parent().unwrap()).map_err(|e| e.to_string())?;

        let id = self.supervisor.start(ProcessSpec {
            name: "Mailpit".into(),
            executable: binary.display().to_string(),
            args: vec!["--db-file".into(), db_file.display().to_string()],
            cwd: None,
            env: vec![],
            restart: None,
        });
        Ok(id)
    }

    /// MariaDB (§31, Stage 10): the MySQL-compatible server, on the standard 3306.
    fn start_mariadb(&self, version: &str) -> Result<ProcessId, String> {
        let install_dir = self.runtimes.install_dir("mariadb", version);
        let mariadbd = install_dir.join("bin").join("mariadbd.exe");
        if !mariadbd.is_file() {
            return Err("mariadbd.exe missing on disk".into());
        }
        let data_dir = self.paths.services_dir().join("mariadb").join("data");

        if !data_dir.exists() {
            // The installer makes the data folder itself but not the folders above it.
            std::fs::create_dir_all(data_dir.parent().unwrap()).map_err(|e| e.to_string())?;
            let installer = install_dir.join("bin").join("mariadb-install-db.exe");
            let init = run_capture(
                &installer,
                &[format!("--datadir={}", data_dir.display())],
                Some(&install_dir),
                &[],
                Duration::from_secs(120),
            );
            if !init.success() {
                let _ = std::fs::remove_dir_all(&data_dir);
                return Err(format!("mariadb-install-db failed: {}", init.combined()));
            }
        }

        Ok(self.supervisor.start(ProcessSpec {
            name: "MariaDB".into(),
            executable: mariadbd.display().to_string(),
            args: vec![
                format!("--datadir={}", data_dir.display()),
                format!("--port={}", primary_port("mariadb").unwrap()),
                "--bind-address=127.0.0.1".to_string(),
                "--console".to_string(),
            ]
            .into_iter()
            .chain(self.limit_args("mariadb"))
            .collect(),
            cwd: Some(install_dir.display().to_string()),
            env: vec![],
            restart: None,
        }))
    }

    /// MongoDB (§31, Stage 10): connection info, logs (process output), health (port probe).
    fn start_mongodb(&self, version: &str) -> Result<ProcessId, String> {
        let mongod = self.runtimes.binary_path("mongodb", version).ok_or("mongod.exe missing on disk")?;
        let data_dir = self.paths.services_dir().join("mongodb").join("data");
        std::fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;

        Ok(self.supervisor.start(ProcessSpec {
            name: "MongoDB".into(),
            executable: mongod.display().to_string(),
            args: vec![
                "--dbpath".into(),
                data_dir.display().to_string(),
                "--bind_ip".into(),
                "127.0.0.1".into(),
                "--port".into(),
                primary_port("mongodb").unwrap().to_string(),
            ]
            .into_iter()
            .chain(self.limit_args("mongodb"))
            .collect(),
            cwd: None,
            env: vec![],
            restart: None,
        }))
    }

    /// PostgreSQL (§31, Stage 5). Trust authentication on loopback only: fine for a local dev
    /// tool, and the server never listens on another interface (§138).
    fn start_postgres(&self, version: &str) -> Result<ProcessId, String> {
        let install_dir = self.runtimes.install_dir("postgres", version);
        let bin = install_dir.join("bin");
        let postgres = bin.join("postgres.exe");
        if !postgres.is_file() {
            return Err("postgres.exe missing on disk".into());
        }
        let data_dir = self.paths.services_dir().join("postgres").join("data");

        // First run only: `initdb` creates the cluster and a `postgres` superuser.
        if !data_dir.join("PG_VERSION").is_file() {
            std::fs::create_dir_all(data_dir.parent().unwrap()).map_err(|e| e.to_string())?;
            let init = run_capture(
                &bin.join("initdb.exe"),
                &[
                    "-D".to_string(),
                    data_dir.display().to_string(),
                    "-U".to_string(),
                    "postgres".to_string(),
                    "-A".to_string(),
                    "trust".to_string(),
                    "-E".to_string(),
                    "UTF8".to_string(),
                ],
                Some(&install_dir),
                &[],
                Duration::from_secs(180),
            );
            if !init.success() {
                let _ = std::fs::remove_dir_all(&data_dir);
                return Err(format!("initdb failed: {}", init.combined()));
            }
        }

        Ok(self.supervisor.start(ProcessSpec {
            name: "PostgreSQL".into(),
            executable: postgres.display().to_string(),
            args: vec![
                "-D".into(),
                data_dir.display().to_string(),
                "-p".into(),
                primary_port("postgres").unwrap().to_string(),
                "-c".into(),
                "listen_addresses=127.0.0.1".into(),
            ]
            .into_iter()
            .chain(self.limit_args("postgres"))
            .collect(),
            cwd: Some(install_dir.display().to_string()),
            env: vec![],
            restart: None,
        }))
    }

    /// Redis (§31, Stage 5), from the community Windows build. Loopback only, snapshots go to
    /// the service's own data directory (the process's working directory).
    fn start_redis(&self, version: &str) -> Result<ProcessId, String> {
        let server = self.runtimes.binary_path("redis", version).ok_or("redis-server.exe missing on disk")?;
        let data_dir = self.paths.services_dir().join("redis").join("data");
        std::fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;

        Ok(self.supervisor.start(ProcessSpec {
            name: "Redis".into(),
            executable: server.display().to_string(),
            args: vec![
                "--port".into(),
                primary_port("redis").unwrap().to_string(),
                "--bind".into(),
                "127.0.0.1".into(),
            ]
            .into_iter()
            .chain(self.limit_args("redis"))
            .collect(),
            cwd: Some(data_dir.display().to_string()),
            env: vec![],
            restart: None,
        }))
    }

    // ------------------------------------------------------------- SQL clients (§32–33)

    /// Path + port of the command-line client for a SQL engine ("mariadb" | "postgres").
    /// The engine's own command-line client and the port it listens on.
    pub fn sql_client(&self, engine: &str) -> Result<(PathBuf, u16), String> {
        let (exe, port) = match engine {
            "mariadb" => ("mariadb.exe", primary_port("mariadb")),
            "postgres" => ("psql.exe", primary_port("postgres")),
            other => return Err(format!("{other} is not a SQL engine OpenLocalServer manages")),
        };
        let version = self
            .runtimes
            .installed_versions(engine)
            .into_iter()
            .next()
            .ok_or_else(|| format!("{engine} is not installed"))?;
        let client = self.runtimes.install_dir(engine, &version).join("bin").join(exe);
        if !client.is_file() {
            return Err(format!("{exe} missing on disk"));
        }
        Ok((client, port.unwrap()))
    }

    /// Runs SQL as root against the local engine and returns its tab-separated output.
    pub fn run_sql(&self, engine: &str, sql: &str) -> Result<String, String> {
        let (client, port) = self.sql_client(engine)?;
        let port_s = port.to_string();
        let args: Vec<String> = if engine == "postgres" {
            // Unaligned, tuples-only, tab-separated: the same shape the MySQL client gives.
            ["-U", "postgres", "-h", "127.0.0.1", "-p", port_s.as_str(), "-X", "-A", "-t", "-F", "\t", "-v", "ON_ERROR_STOP=1", "-c", sql]
                .iter()
                .map(|a| a.to_string())
                .collect()
        } else {
            ["-u", "root", "-h", "127.0.0.1", "-P", port_s.as_str(), "--batch", "--skip-column-names", "-e", sql]
                .iter()
                .map(|a| a.to_string())
                .collect()
        };
        let out: Captured = run_capture(&client, &args, None, &[], Duration::from_secs(20));
        if out.success() {
            Ok(out.stdout)
        } else if out.exit_code.is_none() && !out.timed_out {
            Err(out.stderr)
        } else {
            Err(format!("{engine} client failed: {}", out.combined()))
        }
    }

    pub fn create_database(&self, engine: &str, name: &str) -> Result<(), String> {
        if !is_safe_identifier(name) {
            return Err("database name must be alphanumeric/underscore only".into());
        }
        if engine == "postgres" {
            // PostgreSQL has no `IF NOT EXISTS` for databases.
            if self.list_databases(engine)?.iter().any(|d| d == name) {
                return Ok(());
            }
            return self.run_sql(engine, &format!("CREATE DATABASE \"{name}\"")).map(|_| ());
        }
        self.run_sql(engine, &format!("CREATE DATABASE IF NOT EXISTS `{name}`")).map(|_| ())
    }

    pub fn list_databases(&self, engine: &str) -> Result<Vec<String>, String> {
        let text = if engine == "postgres" {
            self.run_sql(engine, "SELECT datname FROM pg_database WHERE NOT datistemplate ORDER BY datname")?
        } else {
            self.run_sql(engine, "SHOW DATABASES")?
        };
        const SYSTEM: &[&str] = &["information_schema", "mysql", "performance_schema", "sys", "postgres"];
        Ok(text.lines().map(str::trim).filter(|l| !l.is_empty() && !SYSTEM.contains(l)).map(str::to_string).collect())
    }

    /// §31 "MariaDB (users)": creates `user@localhost` with a password and full rights on
    /// one database. The password is kept in the OS credential store, never on disk (§141).
    pub fn create_user(&self, engine: &str, user: &str, password: &str, database: &str) -> Result<(), String> {
        if !is_safe_identifier(user) {
            return Err("user name must be alphanumeric/underscore only".into());
        }
        if !is_safe_identifier(database) {
            return Err("database name must be alphanumeric/underscore only".into());
        }
        if password.is_empty() {
            return Err("a password is required".into());
        }
        let u = sql_string(user);
        if engine == "postgres" {
            let exists = !self.run_sql(engine, &format!("SELECT 1 FROM pg_roles WHERE rolname = {}", pg_string(user)))?.trim().is_empty();
            let verb = if exists { "ALTER" } else { "CREATE" };
            self.run_sql(engine, &format!("{verb} ROLE \"{user}\" LOGIN PASSWORD {}", pg_string(password)))?;
            self.create_database(engine, database)?;
            self.run_sql(engine, &format!("GRANT ALL PRIVILEGES ON DATABASE \"{database}\" TO \"{user}\""))?;
            // PostgreSQL 15+ no longer lets ordinary users create tables in `public`; owning the
            // database fixes that.
            self.run_sql(engine, &format!("ALTER DATABASE \"{database}\" OWNER TO \"{user}\""))?;
            let _ = crate::secrets::set_secret(&format!("db.{engine}.{user}"), password);
            return Ok(());
        }
        let sql = format!(
            "CREATE DATABASE IF NOT EXISTS `{database}`; \
             CREATE USER IF NOT EXISTS {u}@'localhost' IDENTIFIED BY {p}; \
             ALTER USER {u}@'localhost' IDENTIFIED BY {p}; \
             GRANT ALL PRIVILEGES ON `{database}`.* TO {u}@'localhost'; \
             FLUSH PRIVILEGES",
            p = sql_string(password)
        );
        self.run_sql(engine, &sql)?;
        // Best effort: not being able to remember it doesn't undo the user that now exists.
        let _ = crate::secrets::set_secret(&format!("db.{engine}.{user}"), password);
        Ok(())
    }

    pub fn list_users(&self, engine: &str) -> Result<Vec<DbUser>, String> {
        let text = if engine == "postgres" {
            self.run_sql(engine, "SELECT rolname, 'localhost' FROM pg_roles WHERE rolname NOT LIKE 'pg\\_%' ORDER BY rolname")?
        } else {
            self.run_sql(engine, "SELECT user, host FROM mysql.user ORDER BY user")?
        };
        Ok(text
            .lines()
            .filter_map(|l| {
                let (user, host) = l.split_once('\t')?;
                Some(DbUser { user: user.trim().to_string(), host: host.trim().to_string() })
            })
            .collect())
    }

    /// What an external tool needs to connect to `engine` (§102). `database` is optional.
    pub fn connection_info(&self, engine: &str, database: Option<&str>, sqlite_path: Option<&str>) -> Result<ConnectionInfo, String> {
        match engine {
            "mariadb" => {
                let port = primary_port(engine).unwrap();
                Ok(ConnectionInfo {
                    engine: engine.into(),
                    host: "127.0.0.1".into(),
                    port: Some(port),
                    user: Some("root".into()),
                    database: database.map(str::to_string),
                    path: None,
                    uri: format!("mysql://root@127.0.0.1:{port}/{}", database.unwrap_or("")),
                })
            }
            "postgres" => {
                let port = primary_port("postgres").unwrap();
                Ok(ConnectionInfo {
                    engine: "postgres".into(),
                    host: "127.0.0.1".into(),
                    port: Some(port),
                    user: Some("postgres".into()),
                    database: database.map(str::to_string),
                    path: None,
                    uri: format!("postgresql://postgres@127.0.0.1:{port}/{}", database.unwrap_or("postgres")),
                })
            }
            "redis" => Ok(ConnectionInfo {
                engine: "redis".into(),
                host: "127.0.0.1".into(),
                port: primary_port("redis"),
                user: None,
                database: None,
                path: None,
                uri: format!("redis://127.0.0.1:{}", primary_port("redis").unwrap()),
            }),
            "mongodb" => Ok(ConnectionInfo {
                engine: "mongodb".into(),
                host: "127.0.0.1".into(),
                port: primary_port("mongodb"),
                user: None,
                database: database.map(str::to_string),
                path: None,
                uri: format!("mongodb://127.0.0.1:{}", primary_port("mongodb").unwrap()),
            }),
            "sqlite" => {
                let path = sqlite_path.ok_or("a database file path is required")?;
                Ok(ConnectionInfo {
                    engine: "sqlite".into(),
                    host: "".into(),
                    port: None,
                    user: None,
                    database: None,
                    path: Some(path.to_string()),
                    uri: format!("sqlite:///{}", path.replace('\\', "/")),
                })
            }
            other => Err(format!("unknown database engine: {other}")),
        }
    }
}

fn kind_of(id: &str) -> &'static str {
    match id {
        "mailpit" => "mail",
        "mongodb" => "document",
        "redis" => "cache",
        _ => "sql",
    }
}

fn primary_port(id: &str) -> Option<u16> {
    match id {
        "mailpit" => Some(8025),
        "mariadb" => Some(3306),
        "mongodb" => Some(27017),
        "postgres" => Some(5432),
        "redis" => Some(6379),
        _ => None,
    }
}

fn connection_string(id: &str) -> Option<String> {
    match id {
        "mailpit" => Some(format!("SMTP 127.0.0.1:{} · UI http://127.0.0.1:8025", mailpit_smtp_port())),
        "mariadb" => Some("mysql://root@127.0.0.1:3306".into()),
        "mongodb" => Some("mongodb://127.0.0.1:27017".into()),
        "postgres" => Some("postgresql://postgres@127.0.0.1:5432".into()),
        "redis" => Some("redis://127.0.0.1:6379".into()),
        _ => None,
    }
}

/// Mailpit's SMTP port — separate from its web UI port (`primary_port`), which is what a
/// project's `.env` gets pointed at (§63).
pub fn mailpit_smtp_port() -> u16 {
    1025
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_reject_anything_that_could_break_out_of_sql() {
        assert!(is_safe_identifier("shop_db1"));
        assert!(!is_safe_identifier(""));
        assert!(!is_safe_identifier("a`b"));
        assert!(!is_safe_identifier("a b"));
        assert!(!is_safe_identifier("x; DROP DATABASE y"));
        assert!(!is_safe_identifier(&"a".repeat(65)));
    }

    #[test]
    fn sql_string_escapes_quotes_and_backslashes() {
        assert_eq!(sql_string("pa'ss"), "'pa''ss'");
        assert_eq!(sql_string("a\\b"), "'a\\\\b'");
        assert_eq!(sql_string("'; DROP TABLE x; --"), "'''; DROP TABLE x; --'");
    }

    #[test]
    fn connection_info_covers_each_engine() {
        let home = crate::test_support::isolated_home();
        let runtimes = Arc::new(RuntimeManager::new(home.paths.clone()));
        let sup = Arc::new(ProcessSupervisor::new());
        let mgr = ServiceManager::new(home.paths.clone(), runtimes, sup);

        let m = mgr.connection_info("mariadb", Some("shop"), None).unwrap();
        assert_eq!(m.port, Some(3306));
        assert_eq!(m.uri, "mysql://root@127.0.0.1:3306/shop");
        assert!(mgr.connection_info("mysql", None, None).is_err(), "MySQL is not offered");
        assert_eq!(mgr.connection_info("mongodb", None, None).unwrap().uri, "mongodb://127.0.0.1:27017");
        assert!(mgr.connection_info("sqlite", None, None).is_err());
        assert_eq!(mgr.connection_info("sqlite", None, Some("C:\\db\\a.sqlite")).unwrap().uri, "sqlite:///C:/db/a.sqlite");
        assert_eq!(mgr.connection_info("postgres", Some("shop"), None).unwrap().uri, "postgresql://postgres@127.0.0.1:5432/shop");
        assert_eq!(mgr.connection_info("redis", None, None).unwrap().uri, "redis://127.0.0.1:6379");
        assert!(mgr.connection_info("oracle", None, None).is_err());
    }
}
