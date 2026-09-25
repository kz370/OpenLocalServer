//! Service Manager (§22, §31, §61–68 — Stage 5): long-running background services
//! (Mailpit, MySQL). Reuses the Runtime Manager's install pipeline (same download →
//! verify → extract, §20–21) and the Process Supervisor's lifecycle (§107) — a service
//! is just a process someone starts and expects to keep running, not a new concept.
//!
//! Simplified vs. the full plan for this stage: only Mailpit and MySQL are wired up
//! (PostgreSQL/MariaDB/Redis follow the exact same `start_*` pattern once added to the
//! catalog); dependency-ordered startup (§68) isn't implemented yet — each service is
//! started independently.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::paths::AppPaths;
use crate::port::{check_port, PortStatus};
use crate::process::{ProcessId, ProcessSpec, ProcessSupervisor};
use crate::runtime::RuntimeManager;

const KNOWN_SERVICES: &[&str] = &["mailpit", "mysql"];

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
}

pub struct ServiceManager {
    paths: AppPaths,
    runtimes: Arc<RuntimeManager>,
    supervisor: Arc<ProcessSupervisor>,
    running: Mutex<HashMap<String, ProcessId>>,
}

impl ServiceManager {
    pub fn new(paths: AppPaths, runtimes: Arc<RuntimeManager>, supervisor: Arc<ProcessSupervisor>) -> Self {
        Self { paths, runtimes, supervisor, running: Mutex::new(HashMap::new()) }
    }

    pub fn list(&self) -> Vec<ServiceStatus> {
        KNOWN_SERVICES.iter().map(|&id| self.status(id)).collect()
    }

    pub fn status(&self, id: &str) -> ServiceStatus {
        let entry = self.runtimes.catalog().into_iter().find(|e| e.id == id);
        let installed = entry.as_ref().map(|e| e.installed).unwrap_or(false);
        let running = self.running.lock().unwrap().contains_key(id);
        let port = primary_port(id);
        ServiceStatus {
            id: id.to_string(),
            name: entry.map(|e| e.name).unwrap_or_else(|| id.to_string()),
            installed,
            running,
            port,
            port_status: port.map(|p| match check_port(p) {
                PortStatus::Free => PortStatusLite::Free,
                PortStatus::InUse { .. } => PortStatusLite::InUse,
            }),
        }
    }

    /// §61–68 / §101 one-click "Start". Mailpit and MySQL each need a different command
    /// line, so this dispatches to a per-service starter — the shared part (recording the
    /// resulting `ProcessId`, refusing a double-start) lives here once.
    pub fn start(&self, id: &str) -> Result<ProcessId, String> {
        if self.running.lock().unwrap().contains_key(id) {
            return Err(format!("{id} is already running"));
        }
        let version = self
            .runtimes
            .installed_versions(id)
            .into_iter()
            .next()
            .ok_or_else(|| format!("{id} is not installed — install it from the Runtimes page first"))?;

        let process_id = match id {
            "mailpit" => self.start_mailpit(&version)?,
            "mysql" => self.start_mysql(&version)?,
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

    fn start_mysql(&self, version: &str) -> Result<ProcessId, String> {
        let install_dir = self.runtimes.install_dir("mysql", version);
        let mysqld = install_dir.join("bin").join("mysqld.exe");
        if !mysqld.is_file() {
            return Err("mysqld.exe missing on disk".into());
        }
        let data_dir = self.paths.services_dir().join("mysql").join("data");

        // First run only: `mysqld --initialize-insecure` creates the data directory and a
        // passwordless root user (fine for a local dev tool; never exposed by default —
        // §138 requires database ports stay unexposed, which is a Stage 6 networking
        // concern, not this command's).
        if !data_dir.exists() {
            std::fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;
            let init = self.supervisor.run_to_completion(
                &mysqld.display().to_string(),
                &[
                    "--initialize-insecure".to_string(),
                    format!("--datadir={}", data_dir.display()),
                    format!("--basedir={}", install_dir.display()),
                ],
                None,
                Duration::from_secs(120),
            );
            if init.exit_code != Some(0) {
                let _ = std::fs::remove_dir_all(&data_dir);
                return Err(format!(
                    "mysqld --initialize-insecure failed (exit code {:?}, timed_out={})",
                    init.exit_code, init.timed_out
                ));
            }
        }

        let id = self.supervisor.start(ProcessSpec {
            name: "MySQL".into(),
            executable: mysqld.display().to_string(),
            args: vec![
                format!("--datadir={}", data_dir.display()),
                format!("--basedir={}", install_dir.display()),
                format!("--port={}", primary_port("mysql").unwrap()),
                // Bound to loopback only — never exposed on the network by default (§138).
                "--bind-address=127.0.0.1".to_string(),
            ],
            cwd: None,
            env: vec![],
            restart: None,
        });
        Ok(id)
    }

    /// Runs `mysql -u root -e <sql>` against the running instance (§32 database creation).
    pub fn run_mysql_client(&self, sql: &str) -> Result<String, String> {
        let version = self.runtimes.installed_versions("mysql").into_iter().next().ok_or("mysql is not installed")?;
        let install_dir = self.runtimes.install_dir("mysql", &version);
        let client = install_dir.join("bin").join("mysql.exe");
        if !client.is_file() {
            return Err("mysql.exe missing on disk".into());
        }
        let port = primary_port("mysql").unwrap();
        let entry = self.supervisor.run_to_completion(
            &client.display().to_string(),
            &["-u".into(), "root".into(), "-P".into(), port.to_string(), "-h".into(), "127.0.0.1".into(), "-e".into(), sql.into()],
            None,
            Duration::from_secs(20),
        );
        if entry.exit_code == Some(0) {
            Ok("ok".to_string())
        } else {
            Err(format!("mysql client exited with {:?} (timed_out={})", entry.exit_code, entry.timed_out))
        }
    }
}

fn primary_port(id: &str) -> Option<u16> {
    match id {
        "mailpit" => Some(8025),
        "mysql" => Some(3306),
        _ => None,
    }
}

/// Mailpit's SMTP port — separate from its web UI port (`primary_port`), which is what a
/// project's `.env` gets pointed at (§63).
pub fn mailpit_smtp_port() -> u16 {
    1025
}
