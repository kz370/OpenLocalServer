//! Process Supervisor + Command Runner (§107, §90–91 — Stage 2).
//!
//! Owns a dedicated background tokio runtime so it works identically from `cargo test`
//! and from the Tauri shell, without either caller needing to be async itself.
//!
//! Simplified vs. the full plan for this stage: tree-kill on Windows shells out to
//! `taskkill /T /F` rather than using Job Objects (windows-rs), and command history is an
//! in-memory ring buffer rather than the `command_history` SQLite table (Stage 1 hasn't
//! landed SQLite yet). Both are drop-in upgrades later — the public API doesn't change.

use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::broadcast;

/// Cap on how many recent output lines we keep per process in memory.
const OUTPUT_BUFFER_LINES: usize = 500;
/// Cap on how many finished one-shot commands we remember (§93 command history).
const HISTORY_CAPACITY: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProcessId(pub u64);

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// §107: the full state machine a managed process moves through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessState {
    Starting,
    Running,
    Stopping,
    Stopped,
    Crashed,
    Restarting,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessSpec {
    pub name: String,
    pub executable: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub env: Vec<(String, String)>,
    /// §108 crash recovery policy. `None` = never auto-restart.
    pub restart: Option<RestartPolicy>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RestartPolicy {
    pub max_retries: u32,
    pub delay_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub id: ProcessId,
    pub name: String,
    pub pid: Option<u32>,
    pub state: ProcessState,
    pub exit_code: Option<i32>,
    pub restarts: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProcessEvent {
    StateChanged { id: ProcessId, state: ProcessState },
    Output { id: ProcessId, stream: OutputStream, line: String },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputStream {
    Stdout,
    Stderr,
}

struct ProcessRecord {
    info: ProcessInfo,
    spec: ProcessSpec,
    output: VecDeque<String>,
    /// Set once the caller explicitly asked to stop — tells the exit-watcher not to
    /// treat this exit as a crash and not to apply the restart policy.
    stop_requested: bool,
}

/// §90: a finished one-shot command, kept for the Recent Commands view (§93).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandHistoryEntry {
    pub command: String,
    pub args: Vec<String>,
    pub exit_code: Option<i32>,
    pub duration_ms: u128,
    pub timed_out: bool,
}

type ProcessTable = Arc<Mutex<HashMap<u64, ProcessRecord>>>;

pub struct ProcessSupervisor {
    runtime: tokio::runtime::Runtime,
    processes: ProcessTable,
    events_tx: broadcast::Sender<ProcessEvent>,
    history: Arc<Mutex<VecDeque<CommandHistoryEntry>>>,
}

impl ProcessSupervisor {
    pub fn new() -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("failed to start process supervisor runtime");
        let (events_tx, _rx) = broadcast::channel(1024);
        Self {
            runtime,
            processes: Arc::new(Mutex::new(HashMap::new())),
            events_tx,
            history: Arc::new(Mutex::new(VecDeque::with_capacity(HISTORY_CAPACITY))),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ProcessEvent> {
        self.events_tx.subscribe()
    }

    /// §101 one-click "Start": spawn a long-running (or one-shot) managed process.
    /// Returns immediately once queued; state transitions and output arrive as events.
    pub fn start(&self, spec: ProcessSpec) -> ProcessId {
        let id = ProcessId(NEXT_ID.fetch_add(1, Ordering::SeqCst));
        let record = ProcessRecord {
            info: ProcessInfo {
                id,
                name: spec.name.clone(),
                pid: None,
                state: ProcessState::Starting,
                exit_code: None,
                restarts: 0,
            },
            spec: spec.clone(),
            output: VecDeque::with_capacity(OUTPUT_BUFFER_LINES),
            stop_requested: false,
        };
        self.processes.lock().unwrap().insert(id.0, record);
        let _ = self.events_tx.send(ProcessEvent::StateChanged { id, state: ProcessState::Starting });

        self.runtime.spawn(run_process_attempt(
            id,
            spec,
            self.processes.clone(),
            self.events_tx.clone(),
        ));
        id
    }

    /// §101 one-click "Stop". Marks the process so its exit isn't treated as a crash,
    /// then asks the OS to end it (tree-kill so child processes don't leak, §107).
    pub fn stop(&self, id: ProcessId) {
        let pid = {
            let mut guard = self.processes.lock().unwrap();
            let Some(rec) = guard.get_mut(&id.0) else { return };
            rec.stop_requested = true;
            rec.info.state = ProcessState::Stopping;
            rec.info.pid
        };
        let _ = self.events_tx.send(ProcessEvent::StateChanged { id, state: ProcessState::Stopping });

        let Some(pid) = pid else { return };
        self.runtime.spawn(async move {
            kill_tree(pid).await;
        });
    }

    pub fn snapshot(&self) -> Vec<ProcessInfo> {
        let guard = self.processes.lock().unwrap();
        let mut list: Vec<_> = guard.values().map(|r| r.info.clone()).collect();
        list.sort_by_key(|p| p.id.0);
        list
    }

    pub fn recent_output(&self, id: ProcessId) -> Vec<String> {
        let guard = self.processes.lock().unwrap();
        guard.get(&id.0).map(|r| r.output.iter().cloned().collect()).unwrap_or_default()
    }

    // -- Command Runner (§90–91): run a one-shot command to completion, with a timeout. --

    /// Blocks the calling thread until the command finishes, is killed by the timeout,
    /// or fails to spawn. Safe to call from a sync `CoreCommand` handler.
    pub fn run_to_completion(
        &self,
        executable: &str,
        args: &[String],
        cwd: Option<&str>,
        timeout: Duration,
    ) -> CommandHistoryEntry {
        let executable_owned = executable.to_string();
        let args_owned = args.to_vec();
        let cwd_owned = cwd.map(|s| s.to_string());

        let result = self.runtime.block_on(async move {
            let started = Instant::now();
            let mut cmd = Command::new(&executable_owned);
            cmd.args(&args_owned).stdout(Stdio::null()).stderr(Stdio::null());
            if let Some(cwd) = &cwd_owned {
                cmd.current_dir(cwd);
            }

            let (exit_code, timed_out) = match cmd.spawn() {
                Ok(mut child) => match tokio::time::timeout(timeout, child.wait()).await {
                    Ok(Ok(status)) => (status.code(), false),
                    Ok(Err(_)) => (None, false),
                    Err(_) => {
                        let _ = child.kill().await;
                        (None, true)
                    }
                },
                Err(_) => (None, false),
            };

            CommandHistoryEntry {
                command: executable_owned,
                args: args_owned,
                exit_code,
                duration_ms: started.elapsed().as_millis(),
                timed_out,
            }
        });

        let mut history = self.history.lock().unwrap();
        if history.len() >= HISTORY_CAPACITY {
            history.pop_front();
        }
        history.push_back(result.clone());
        result
    }

    pub fn history(&self) -> Vec<CommandHistoryEntry> {
        self.history.lock().unwrap().iter().cloned().collect()
    }
}

impl Default for ProcessSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

/// The spawn → stream-output → wait → (maybe restart) loop. Recursive: a restart is
/// just another call to this same function, so first-attempt and every retry share one
/// implementation. Boxed because an `async fn` can't otherwise recurse (infinite-sized future).
fn run_process_attempt(
    id: ProcessId,
    spec: ProcessSpec,
    processes: ProcessTable,
    events_tx: broadcast::Sender<ProcessEvent>,
) -> Pin<Box<dyn Future<Output = ()> + Send>> {
    Box::pin(async move {
        let mut cmd = Command::new(&spec.executable);
        cmd.args(&spec.args).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
        if let Some(cwd) = &spec.cwd {
            cmd.current_dir(cwd);
        }
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(process = %spec.name, error = %e, "failed to spawn process");
                set_state(&processes, id, ProcessState::Failed, None);
                let _ = events_tx.send(ProcessEvent::StateChanged { id, state: ProcessState::Failed });
                return;
            }
        };

        let pid = child.id();
        set_state(&processes, id, ProcessState::Running, pid);
        tracing::info!(process = %spec.name, pid = ?pid, "process running");
        let _ = events_tx.send(ProcessEvent::StateChanged { id, state: ProcessState::Running });

        if let Some(stdout) = child.stdout.take() {
            spawn_line_reader(id, OutputStream::Stdout, stdout, processes.clone(), events_tx.clone());
        }
        if let Some(stderr) = child.stderr.take() {
            spawn_line_reader(id, OutputStream::Stderr, stderr, processes.clone(), events_tx.clone());
        }

        let exit = child.wait().await;

        let (stop_requested, restart_policy) = {
            let guard = processes.lock().unwrap();
            let rec = guard.get(&id.0);
            (rec.map(|r| r.stop_requested).unwrap_or(true), rec.and_then(|r| r.spec.restart))
        };
        let exit_code = exit.ok().and_then(|s| s.code());
        let crashed = !stop_requested && exit_code != Some(0);
        let final_state = if crashed { ProcessState::Crashed } else { ProcessState::Stopped };

        {
            let mut guard = processes.lock().unwrap();
            if let Some(rec) = guard.get_mut(&id.0) {
                rec.info.state = final_state;
                rec.info.exit_code = exit_code;
            }
        }
        let _ = events_tx.send(ProcessEvent::StateChanged { id, state: final_state });
        tracing::info!(process = %spec.name, ?exit_code, ?final_state, "process exited");

        // §108 crash recovery: only for unexpected exits, only while retries remain.
        if crashed {
            if let Some(policy) = restart_policy {
                let attempts_so_far = {
                    let guard = processes.lock().unwrap();
                    guard.get(&id.0).map(|r| r.info.restarts).unwrap_or(0)
                };
                if attempts_so_far < policy.max_retries {
                    {
                        let mut guard = processes.lock().unwrap();
                        if let Some(rec) = guard.get_mut(&id.0) {
                            rec.info.restarts += 1;
                            rec.info.state = ProcessState::Restarting;
                        }
                    }
                    let _ = events_tx.send(ProcessEvent::StateChanged { id, state: ProcessState::Restarting });
                    tokio::time::sleep(Duration::from_millis(policy.delay_ms)).await;
                    run_process_attempt(id, spec, processes, events_tx).await;
                }
            }
        }
    })
}

fn set_state(processes: &ProcessTable, id: ProcessId, state: ProcessState, pid: Option<u32>) {
    let mut guard = processes.lock().unwrap();
    if let Some(rec) = guard.get_mut(&id.0) {
        rec.info.state = state;
        if pid.is_some() {
            rec.info.pid = pid;
        }
    }
}

fn spawn_line_reader<R>(
    id: ProcessId,
    stream: OutputStream,
    reader: R,
    processes: ProcessTable,
    events_tx: broadcast::Sender<ProcessEvent>,
) where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            {
                let mut guard = processes.lock().unwrap();
                if let Some(rec) = guard.get_mut(&id.0) {
                    if rec.output.len() >= OUTPUT_BUFFER_LINES {
                        rec.output.pop_front();
                    }
                    rec.output.push_back(line.clone());
                }
            }
            let _ = events_tx.send(ProcessEvent::Output { id, stream, line });
        }
    });
}

#[cfg(windows)]
async fn kill_tree(pid: u32) {
    // Pragmatic Stage 2 approach: shell out to `taskkill /T /F` to kill the whole process
    // tree. A proper Windows Job Object (so children are killed even if `taskkill` itself
    // can't enumerate them) is planned but not yet wired in (see module docs).
    let _ = tokio::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;
}

#[cfg(not(windows))]
async fn kill_tree(pid: u32) {
    let _ = tokio::process::Command::new("kill").args(["-9", &pid.to_string()]).status().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait_for_state(sup: &ProcessSupervisor, id: ProcessId, want: ProcessState, timeout: Duration) -> bool {
        let start = Instant::now();
        loop {
            if sup.snapshot().iter().any(|p| p.id == id && p.state == want) {
                return true;
            }
            if start.elapsed() > timeout {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[cfg(windows)]
    fn echo_spec(name: &str, message: &str) -> ProcessSpec {
        ProcessSpec {
            name: name.into(),
            executable: "cmd".into(),
            args: vec!["/C".into(), format!("echo {message}")],
            cwd: None,
            env: vec![],
            restart: None,
        }
    }

    #[cfg(windows)]
    #[test]
    fn spawns_runs_and_captures_output() {
        let sup = ProcessSupervisor::new();
        let id = sup.start(echo_spec("echo-test", "hello-supervisor"));

        assert!(wait_for_state(&sup, id, ProcessState::Stopped, Duration::from_secs(5)));

        let output = sup.recent_output(id);
        assert!(output.iter().any(|l| l.contains("hello-supervisor")));
    }

    #[cfg(windows)]
    #[test]
    fn stop_marks_process_stopped_not_crashed() {
        let sup = ProcessSupervisor::new();
        // `ping` genuinely blocks under redirected stdio (unlike `timeout.exe`, see the
        // command-runner timeout test), so this really is still Running when we call stop().
        let spec = ProcessSpec {
            name: "sleep-test".into(),
            executable: "ping".into(),
            args: vec!["127.0.0.1".into(), "-n".into(), "6".into()],
            cwd: None,
            env: vec![],
            restart: None,
        };
        let id = sup.start(spec);
        assert!(wait_for_state(&sup, id, ProcessState::Running, Duration::from_secs(3)));

        sup.stop(id);
        assert!(wait_for_state(&sup, id, ProcessState::Stopped, Duration::from_secs(5)));
    }

    #[cfg(windows)]
    #[test]
    fn crash_triggers_restart_policy() {
        let sup = ProcessSupervisor::new();
        let spec = ProcessSpec {
            name: "crash-test".into(),
            executable: "cmd".into(),
            args: vec!["/C".into(), "exit 1".into()],
            cwd: None,
            env: vec![],
            restart: Some(RestartPolicy { max_retries: 2, delay_ms: 50 }),
        };
        let id = sup.start(spec);

        let start = Instant::now();
        let mut saw_restart = false;
        while start.elapsed() < Duration::from_secs(5) {
            if sup.snapshot().iter().any(|p| p.id == id && p.restarts >= 1) {
                saw_restart = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(saw_restart, "expected at least one restart after a crash");
    }

    #[cfg(windows)]
    #[test]
    fn command_runner_runs_to_completion() {
        let sup = ProcessSupervisor::new();
        let entry = sup.run_to_completion("cmd", &["/C".into(), "exit 0".into()], None, Duration::from_secs(5));
        assert_eq!(entry.exit_code, Some(0));
        assert!(!entry.timed_out);
        assert_eq!(sup.history().len(), 1);
    }

    #[cfg(windows)]
    #[test]
    fn command_runner_enforces_timeout() {
        // `timeout.exe` refuses to run at all without an attached console (it errors out
        // instantly under redirected stdio, which is what CommandRunner always uses) —
        // `ping` sleeps for real regardless of stdio, so it actually exercises the timeout.
        let sup = ProcessSupervisor::new();
        let entry = sup.run_to_completion(
            "ping",
            &["127.0.0.1".into(), "-n".into(), "11".into()],
            None,
            Duration::from_millis(300),
        );
        assert!(entry.timed_out);
        assert_eq!(entry.exit_code, None);
    }
}
