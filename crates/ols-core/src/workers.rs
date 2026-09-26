//! Queue workers (§105): long-running project processes such as `php artisan queue:work`,
//! Celery or a BullMQ script, run through the Process Supervisor with the project's own
//! runtimes. Each worker runs `count` copies, restarts on a crash up to `max_retries`
//! times, and can pass a job timeout and a memory limit to the frameworks that take one.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::app::Inner;
use crate::detection::Framework;
use crate::error::CoreError;
use crate::paths::AppPaths;
use crate::process::{ProcessId, ProcessSpec, RestartPolicy};

/// More copies than this is almost certainly a typo, and each one is a whole process.
pub const MAX_COUNT: u32 = 16;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Worker {
    /// `<project id>-<name>`, stable so a manifest re-run finds it again.
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub command: String,
    #[serde(default = "one")]
    pub count: u32,
    /// Seconds one job may take (passed as the framework's own flag).
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    #[serde(default)]
    pub memory_mb: Option<u32>,
    /// Restarts after a crash before giving up.
    #[serde(default = "five")]
    pub max_retries: u32,
    #[serde(default = "yes")]
    pub restart: bool,
    /// Started with the project (Start project / setup / a mode that turns workers on).
    #[serde(default = "yes")]
    pub autostart: bool,
}

fn one() -> u32 {
    1
}
fn five() -> u32 {
    5
}
fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerStatus {
    pub worker: Worker,
    pub running: usize,
    pub processes: Vec<ProcessId>,
    /// The full command line each copy runs, flags included.
    pub command_line: String,
}

/// A ready-made worker for the UI's "Add worker" menu.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerPreset {
    pub id: String,
    pub label: String,
    pub command: String,
}

pub fn presets() -> Vec<WorkerPreset> {
    [
        ("laravel", "Laravel queue", "php artisan queue:work"),
        ("symfony", "Symfony Messenger", "php bin/console messenger:consume async"),
        ("celery", "Celery", "celery -A app worker --loglevel=info"),
        ("bullmq", "BullMQ (Node script)", "node worker.js"),
        ("custom", "Custom worker", ""),
    ]
    .into_iter()
    .map(|(id, label, command)| WorkerPreset { id: id.into(), label: label.into(), command: command.into() })
    .collect()
}

/// The usual worker for a framework (`workers: { queue: true }` in a manifest).
pub fn default_command(framework: &Framework) -> Option<&'static str> {
    match framework {
        Framework::Laravel => Some("php artisan queue:work"),
        Framework::Symfony => Some("php bin/console messenger:consume async"),
        Framework::Django | Framework::Flask | Framework::FastApi => Some("celery -A app worker --loglevel=info"),
        _ => None,
    }
}

pub fn worker_id(project_id: &str, name: &str) -> String {
    format!("{project_id}-{}", crate::domain::slugify(name))
}

/// Framework flags for the timeout and memory limit, added only when the command doesn't
/// set them itself.
pub fn command_line(w: &Worker) -> String {
    let mut line = w.command.trim().to_string();
    let original = line.clone();
    let has = |flag: &str| original.contains(flag);
    if line.contains("queue:work") || line.contains("queue:listen") {
        if let Some(t) = w.timeout_secs.filter(|_| !has("--timeout")) {
            line.push_str(&format!(" --timeout={t}"));
        }
        if let Some(m) = w.memory_mb.filter(|_| !has("--memory")) {
            line.push_str(&format!(" --memory={m}"));
        }
    } else if line.contains("messenger:consume") {
        if let Some(t) = w.timeout_secs.filter(|_| !has("--time-limit")) {
            line.push_str(&format!(" --time-limit={t}"));
        }
        if let Some(m) = w.memory_mb.filter(|_| !has("--memory-limit")) {
            line.push_str(&format!(" --memory-limit={m}M"));
        }
    } else if line.starts_with("celery") {
        if let Some(t) = w.timeout_secs.filter(|_| !has("--time-limit")) {
            line.push_str(&format!(" --time-limit={t}"));
        }
        if let Some(m) = w.memory_mb.filter(|_| !has("--max-memory-per-child")) {
            line.push_str(&format!(" --max-memory-per-child={}", m * 1024));
        }
    }
    line
}

pub struct WorkerStore {
    file: PathBuf,
    workers: Vec<Worker>,
}

impl WorkerStore {
    pub fn load(paths: &AppPaths) -> Self {
        let file = paths.data_dir().join("workers.json");
        let workers = std::fs::read_to_string(&file).ok().and_then(|raw| serde_json::from_str(&raw).ok()).unwrap_or_default();
        Self { file, workers }
    }

    pub fn list(&self) -> Vec<Worker> {
        self.workers.clone()
    }

    pub fn get(&self, id: &str) -> Option<Worker> {
        self.workers.iter().find(|w| w.id == id).cloned()
    }

    pub fn save(&mut self, w: Worker) -> Result<(), CoreError> {
        match self.workers.iter_mut().find(|x| x.id == w.id) {
            Some(x) => *x = w,
            None => self.workers.push(w),
        }
        self.persist()
    }

    pub fn remove(&mut self, id: &str) -> Result<(), CoreError> {
        self.workers.retain(|w| w.id != id);
        self.persist()
    }

    fn persist(&self) -> Result<(), CoreError> {
        let tmp = self.file.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(&self.workers)?)?;
        std::fs::rename(&tmp, &self.file)?;
        Ok(())
    }
}

/// Worker id → the supervised processes running its copies.
#[derive(Default)]
pub struct WorkerProcesses(std::sync::Mutex<HashMap<String, Vec<ProcessId>>>);

impl Inner {
    pub fn workers_for(&self, project_id: &str) -> Vec<Worker> {
        self.workers.lock().unwrap().list().into_iter().filter(|w| w.project_id == project_id).collect()
    }

    pub fn worker_statuses(&self, project_id: Option<&str>) -> Vec<WorkerStatus> {
        let procs = self.worker_procs.0.lock().unwrap();
        self.workers
            .lock()
            .unwrap()
            .list()
            .into_iter()
            .filter(|w| project_id.is_none_or(|p| w.project_id == p))
            .map(|w| {
                let processes: Vec<ProcessId> = procs.get(&w.id).cloned().unwrap_or_default().into_iter().filter(|p| self.supervisor.is_alive(*p)).collect();
                WorkerStatus { running: processes.len(), processes, command_line: command_line(&w), worker: w }
            })
            .collect()
    }

    pub fn save_worker(&self, mut w: Worker) -> Result<Worker, CoreError> {
        if w.command.trim().is_empty() {
            return Err(CoreError::ServiceError("a worker needs a command".into()));
        }
        if self.projects.lock().unwrap().get(&w.project_id).is_none() {
            return Err(CoreError::InvalidProjectPath(w.project_id.clone()));
        }
        if w.name.trim().is_empty() {
            w.name = "worker".into();
        }
        if w.id.is_empty() {
            w.id = worker_id(&w.project_id, &w.name);
        }
        w.count = w.count.clamp(1, MAX_COUNT.min(self.resource_limits().max_worker_count.unwrap_or(MAX_COUNT)));
        self.workers.lock().unwrap().save(w.clone())?;
        Ok(w)
    }

    pub fn remove_worker(&self, id: &str) -> Result<(), CoreError> {
        self.stop_worker(id);
        self.workers.lock().unwrap().remove(id)
    }

    /// Starts the copies that aren't running yet. Returns how many are running afterwards.
    pub fn start_worker(&self, id: &str) -> Result<usize, CoreError> {
        let w = self.workers.lock().unwrap().get(id).ok_or_else(|| CoreError::ServiceError(format!("no worker \"{id}\"")))?;
        let line = command_line(&w);
        let tokens = crate::quickapp::plan::split_command_line(&line);
        let (program, args) = tokens.split_first().ok_or_else(|| CoreError::ServiceError("the worker's command is empty".into()))?;
        let (executable, mut full_args, mut env) = self.project_program(program, args, Some(&w.project_id))?;
        if let Some(mb) = w.memory_mb {
            // Node takes its heap limit from NODE_OPTIONS; PHP from -d memory_limit.
            if program == "node" || program.ends_with("node.exe") {
                env.retain(|(k, _)| k != "NODE_OPTIONS");
                env.push(("NODE_OPTIONS".into(), format!("--max-old-space-size={mb}")));
            } else if program == "php" && !line.contains("--memory") {
                full_args.insert(0, format!("memory_limit={mb}M"));
                full_args.insert(0, "-d".into());
            }
        }
        let project = self.projects.lock().unwrap().get(&w.project_id).ok_or_else(|| CoreError::InvalidProjectPath(w.project_id.clone()))?;

        let mut procs = self.worker_procs.0.lock().unwrap();
        let list = procs.entry(w.id.clone()).or_default();
        list.retain(|p| self.supervisor.is_alive(*p));
        while (list.len() as u32) < w.count {
            if !self.process_slot_free() {
                return Err(CoreError::ServiceError("the process limit in Settings → Resources is reached".into()));
            }
            let n = list.len() + 1;
            let id = self.supervisor.start(ProcessSpec {
                name: format!("{} worker: {}{}", project.name, w.name, if w.count > 1 { format!(" #{n}") } else { String::new() }),
                executable: executable.display().to_string(),
                args: full_args.clone(),
                cwd: Some(project.path.clone()),
                env: env.clone(),
                restart: w.restart.then_some(RestartPolicy { max_retries: w.max_retries, delay_ms: 3000 }),
            });
            list.push(id);
        }
        Ok(list.len())
    }

    pub fn stop_worker(&self, id: &str) {
        if let Some(list) = self.worker_procs.0.lock().unwrap().remove(id) {
            for p in list {
                self.supervisor.stop(p);
            }
        }
    }

    pub fn restart_worker(&self, id: &str) -> Result<usize, CoreError> {
        self.stop_worker(id);
        std::thread::sleep(std::time::Duration::from_millis(300));
        self.start_worker(id)
    }

    /// Starts every autostart worker of a project. Returns the number of running copies.
    pub fn start_project_workers(&self, project_id: &str) -> Result<usize, CoreError> {
        let mut total = 0;
        for w in self.workers_for(project_id).into_iter().filter(|w| w.autostart) {
            total += self.start_worker(&w.id)?;
        }
        Ok(total)
    }

    pub fn stop_project_workers(&self, project_id: &str) {
        for w in self.workers_for(project_id) {
            self.stop_worker(&w.id);
        }
    }

    pub fn stop_all_workers(&self) {
        let ids: Vec<String> = self.worker_procs.0.lock().unwrap().keys().cloned().collect();
        for id in ids {
            self.stop_worker(&id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(command: &str) -> Worker {
        Worker { id: "x".into(), project_id: "p".into(), name: "q".into(), command: command.into(), count: 1, timeout_secs: Some(90), memory_mb: Some(256), max_retries: 5, restart: true, autostart: true }
    }

    #[test]
    fn timeout_and_memory_become_the_frameworks_own_flags() {
        assert_eq!(command_line(&w("php artisan queue:work")), "php artisan queue:work --timeout=90 --memory=256");
        assert_eq!(command_line(&w("php artisan queue:work --timeout=5")), "php artisan queue:work --timeout=5 --memory=256", "the user's own flag wins");
        assert_eq!(command_line(&w("php bin/console messenger:consume async")), "php bin/console messenger:consume async --time-limit=90 --memory-limit=256M");
        assert!(command_line(&w("celery -A app worker")).ends_with("--time-limit=90 --max-memory-per-child=262144"));
        assert_eq!(command_line(&w("node worker.js")), "node worker.js", "unknown workers are run as written");
    }

    #[test]
    fn ids_are_stable_per_project_and_name() {
        assert_eq!(worker_id("abc", "Queue Main"), "abc-queue-main");
    }
}
