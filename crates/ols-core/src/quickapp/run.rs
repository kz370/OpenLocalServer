//! Quick App runner (§86, §90–92, §117): executes a [`RunPlan`] one step at a time on a
//! background thread, streaming every line into the run's own log (its own log source),
//! and building the ✓/✗ result list the user sees at the end (§155).
//!
//! The runner never decides *what* to do — the plan already fixed that and the user
//! reviewed it. Anything that needs the rest of the application (installing a runtime,
//! creating a domain, ...) goes through the [`QuickHost`] the Core implements.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::plan::{PlannedStep, RunPlan, StepBody};

const LOG_CAP: usize = 4000;
const STEP_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// A program the host resolved to something runnable.
#[derive(Debug, Clone, Default)]
pub struct ResolvedProgram {
    pub executable: PathBuf,
    /// Arguments that go before the step's own (`composer.phar` for `composer`, `/C script.cmd`).
    pub pre_args: Vec<String>,
    pub env: Vec<(String, String)>,
    /// Directories to put first on PATH.
    pub path_dirs: Vec<PathBuf>,
}

#[derive(Debug, Clone, Default)]
pub struct ActionOutcome {
    pub detail: Option<String>,
    /// Where to send the user when the run finishes ("Open: https://shop.test").
    pub open_url: Option<String>,
    pub project_id: Option<String>,
}

pub struct RunCtx<'a> {
    pub project_path: Option<PathBuf>,
    pub values: &'a BTreeMap<String, String>,
    pub cancel: &'a AtomicBool,
}

/// What the runner needs from the rest of the application.
pub trait QuickHost: Send + Sync {
    /// Makes sure runtime `id` is installed (downloading + verifying if not) and returns
    /// a human label for the result list ("PHP 8.4.26").
    fn ensure_runtime(&self, id: &str, version: Option<&str>, log: &mut dyn FnMut(&str)) -> Result<String, String>;
    fn resolve_program(&self, program: &str, values: &BTreeMap<String, String>) -> Result<ResolvedProgram, String>;
    fn action(
        &self,
        action: &str,
        with: &BTreeMap<String, String>,
        ctx: &RunCtx,
        log: &mut dyn FnMut(&str),
    ) -> Result<ActionOutcome, String>;
    /// Runs a step that needs administrator rights (after the user confirmed it).
    fn run_elevated(&self, executable: &Path, args: &[String]) -> Result<(), String>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    Running,
    Done,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepView {
    pub name: String,
    pub stage: String,
    pub display: String,
    pub status: StepStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResultItem {
    pub label: String,
    pub ok: bool,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunView {
    pub id: String,
    pub app_id: String,
    pub app_name: String,
    pub state: RunState,
    pub steps: Vec<StepView>,
    pub log: Vec<String>,
    pub results: Vec<ResultItem>,
    pub open_url: Option<String>,
    pub project_id: Option<String>,
    pub error: Option<String>,
    pub warnings: Vec<String>,
    pub started_ms: u64,
    pub finished_ms: Option<u64>,
}

struct Run {
    view: Mutex<RunView>,
    cancel: AtomicBool,
}

pub struct RunManager {
    runs: Mutex<HashMap<String, Arc<Run>>>,
    next: AtomicU64,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

impl Default for RunManager {
    fn default() -> Self {
        Self::new()
    }
}

impl RunManager {
    pub fn new() -> Self {
        Self { runs: Mutex::new(HashMap::new()), next: AtomicU64::new(1) }
    }

    /// Starts `plan` on a background thread and returns the run id immediately.
    /// `allow_elevated`: the user separately confirmed the plan's administrator steps (§92).
    pub fn start(&self, plan: RunPlan, host: Arc<dyn QuickHost>, allow_elevated: bool) -> String {
        let id = format!("run-{}", self.next.fetch_add(1, Ordering::SeqCst));
        let view = RunView {
            id: id.clone(),
            app_id: plan.app_id.clone(),
            app_name: plan.app_name.clone(),
            state: RunState::Running,
            steps: plan
                .steps
                .iter()
                .map(|s| StepView { name: s.name.clone(), stage: s.stage.clone(), display: s.display.clone(), status: StepStatus::Pending })
                .collect(),
            log: Vec::new(),
            results: Vec::new(),
            open_url: None,
            project_id: None,
            error: None,
            warnings: plan.warnings.clone(),
            started_ms: now_ms(),
            finished_ms: None,
        };
        let run = Arc::new(Run { view: Mutex::new(view), cancel: AtomicBool::new(false) });
        self.runs.lock().unwrap().insert(id.clone(), run.clone());

        std::thread::spawn(move || execute(&plan, host.as_ref(), &run, allow_elevated));
        id
    }

    pub fn get(&self, id: &str) -> Option<RunView> {
        self.runs.lock().unwrap().get(id).map(|r| r.view.lock().unwrap().clone())
    }

    pub fn clear_log(&self, id: &str) {
        if let Some(r) = self.runs.lock().unwrap().get(id) {
            r.view.lock().unwrap().log.clear();
        }
    }

    pub fn list(&self) -> Vec<RunView> {
        let mut all: Vec<RunView> = self.runs.lock().unwrap().values().map(|r| r.view.lock().unwrap().clone()).collect();
        all.sort_by(|a, b| b.started_ms.cmp(&a.started_ms));
        all
    }

    pub fn cancel(&self, id: &str) {
        if let Some(run) = self.runs.lock().unwrap().get(id) {
            run.cancel.store(true, Ordering::SeqCst);
        }
    }
}

/// Removes secret values from a line before it's stored or shown (§92, §141).
fn redact(line: &str, secrets: &[String]) -> String {
    let mut out = line.to_string();
    for s in secrets {
        if s.len() >= 4 {
            out = out.replace(s, "••••••••");
        }
    }
    out
}

fn secrets_of(plan: &RunPlan) -> Vec<String> {
    plan.display_values
        .iter()
        .filter(|(_, shown)| shown.as_str() == "••••••••")
        .filter_map(|(k, _)| plan.values.get(k).cloned())
        .collect()
}

fn execute(plan: &RunPlan, host: &dyn QuickHost, run: &Run, allow_elevated: bool) {
    let secrets = secrets_of(plan);
    let push_log = |line: &str| {
        let mut v = run.view.lock().unwrap();
        if v.log.len() >= LOG_CAP {
            v.log.remove(0);
        }
        v.log.push(redact(line, &secrets));
    };
    let set_status = |i: usize, status: StepStatus| {
        run.view.lock().unwrap().steps[i].status = status;
    };
    let add_result = |label: &str, ok: bool, detail: Option<String>| {
        run.view.lock().unwrap().results.push(ResultItem { label: label.to_string(), ok, detail });
    };

    push_log(&format!("Starting {}", plan.app_name));
    let project_path = plan.project_path.as_ref().map(PathBuf::from);

    for (i, step) in plan.steps.iter().enumerate() {
        if run.cancel.load(Ordering::SeqCst) {
            for j in i..plan.steps.len() {
                set_status(j, StepStatus::Skipped);
            }
            finish(run, RunState::Cancelled, Some("Cancelled".into()));
            return;
        }
        set_status(i, StepStatus::Running);
        push_log(&format!("▶ {}", step.name));

        let outcome = run_step(step, plan, host, run, project_path.as_deref(), allow_elevated, &push_log);
        match outcome {
            Ok(done) => {
                set_status(i, StepStatus::Done);
                add_result(&done.label.unwrap_or_else(|| step.name.clone()), true, done.detail);
                if let Some(url) = done.open_url {
                    run.view.lock().unwrap().open_url = Some(url);
                }
                if let Some(id) = done.project_id {
                    run.view.lock().unwrap().project_id = Some(id);
                }
            }
            Err(StepError::Cancelled) => {
                set_status(i, StepStatus::Failed);
                for j in i + 1..plan.steps.len() {
                    set_status(j, StepStatus::Skipped);
                }
                finish(run, RunState::Cancelled, Some("Cancelled".into()));
                return;
            }
            Err(StepError::Failed(msg)) => {
                push_log(&format!("✗ {}: {msg}", step.name));
                set_status(i, StepStatus::Failed);
                add_result(&step.name, false, Some(msg.clone()));
                if step.allow_failure {
                    continue;
                }
                for j in i + 1..plan.steps.len() {
                    set_status(j, StepStatus::Skipped);
                }
                finish(run, RunState::Failed, Some(format!("{}: {msg}", step.name)));
                return;
            }
        }
    }
    push_log("Done.");
    finish(run, RunState::Succeeded, None);
}

fn finish(run: &Run, state: RunState, error: Option<String>) {
    let mut v = run.view.lock().unwrap();
    v.state = state;
    v.error = error;
    v.finished_ms = Some(now_ms());
}

enum StepError {
    Failed(String),
    Cancelled,
}

impl From<String> for StepError {
    fn from(s: String) -> Self {
        StepError::Failed(s)
    }
}

#[derive(Default)]
struct StepDone {
    label: Option<String>,
    detail: Option<String>,
    open_url: Option<String>,
    project_id: Option<String>,
}

fn run_step(
    step: &PlannedStep,
    plan: &RunPlan,
    host: &dyn QuickHost,
    run: &Run,
    project_path: Option<&Path>,
    allow_elevated: bool,
    log: &dyn Fn(&str),
) -> Result<StepDone, StepError> {
    if step.elevated && !allow_elevated {
        return Err(StepError::Failed("this step needs administrator approval, which wasn't given".into()));
    }
    match &step.body {
        StepBody::EnsureRuntime { id, version } => {
            let mut sink = |l: &str| log(l);
            let label = host.ensure_runtime(id, version.as_deref(), &mut sink)?;
            Ok(StepDone { label: Some(label), ..Default::default() })
        }
        StepBody::WriteFile { path, content, overwrite } => {
            let path = Path::new(path);
            if path.exists() && !overwrite {
                log(&format!("{} already exists, left as is", path.display()));
                return Ok(StepDone::default());
            }
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("could not create {}: {e}", parent.display()))?;
            }
            std::fs::write(path, content).map_err(|e| format!("could not write {}: {e}", path.display()))?;
            Ok(StepDone::default())
        }
        StepBody::WriteEnv { file, key, value } => {
            let path = Path::new(file);
            let existing = std::fs::read_to_string(path).unwrap_or_default();
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            std::fs::write(path, upsert_env(&existing, key, value)).map_err(|e| format!("could not write {}: {e}", path.display()))?;
            Ok(StepDone::default())
        }
        StepBody::Action { action, with } => {
            let ctx = RunCtx { project_path: project_path.map(Path::to_path_buf), values: &plan.values, cancel: &run.cancel };
            let mut sink = |l: &str| log(l);
            let out = host.action(action, with, &ctx, &mut sink)?;
            Ok(StepDone { label: None, detail: out.detail, open_url: out.open_url, project_id: out.project_id })
        }
        StepBody::Run { program, args, cwd } => {
            let resolved = host.resolve_program(program, &plan.values)?;
            let cwd_path: Option<PathBuf> = match cwd {
                Some(c) if !c.is_empty() => Some(PathBuf::from(c)),
                // Before the project exists commands run in the parent folder.
                _ => match project_path {
                    Some(p) if p.is_dir() => Some(p.to_path_buf()),
                    _ => plan.values.get("parent_dir").map(PathBuf::from),
                },
            };
            if let Some(dir) = &cwd_path {
                std::fs::create_dir_all(dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
            }

            let mut full_args = resolved.pre_args.clone();
            full_args.extend(args.iter().cloned());

            let mut env = resolved.env.clone();
            if !resolved.path_dirs.is_empty() {
                let system_path = std::env::var("PATH").unwrap_or_default();
                let mut dirs: Vec<String> = resolved.path_dirs.iter().map(|d| d.display().to_string()).collect();
                dirs.push(system_path);
                env.push(("PATH".to_string(), dirs.join(";")));
            }

            if step.elevated {
                host.run_elevated(&resolved.executable, &full_args)?;
                return Ok(StepDone::default());
            }

            let out = crate::exec::run_streaming(
                &resolved.executable,
                &full_args,
                cwd_path.as_deref(),
                &env,
                STEP_TIMEOUT,
                &run.cancel,
                |line| log(line),
            );
            if run.cancel.load(Ordering::SeqCst) {
                return Err(StepError::Cancelled);
            }
            if out.timed_out {
                return Err(StepError::Failed(format!("timed out after {} minutes", STEP_TIMEOUT.as_secs() / 60)));
            }
            match out.exit_code {
                Some(0) => Ok(StepDone::default()),
                Some(code) => Err(StepError::Failed(format!("exited with code {code}"))),
                None => Err(StepError::Failed(format!("could not run {}: {}", program, out.stderr.trim()))),
            }
        }
    }
}

/// Sets `KEY=value` in dotenv text (§103): replaces an existing (or commented-out) line,
/// otherwise appends. Everything else in the file is preserved byte for byte.
pub fn upsert_env(existing: &str, key: &str, value: &str) -> String {
    let needs_quotes = value.chars().any(|c| c.is_whitespace() || c == '#' || c == '"' || c == '\'');
    let rendered = if needs_quotes { format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\"")) } else { value.to_string() };
    let new_line = format!("{key}={rendered}");
    let eol = if existing.contains("\r\n") { "\r\n" } else { "\n" };

    let mut lines: Vec<String> = existing.lines().map(str::to_string).collect();
    let is_active = |l: &str| l.trim_start().strip_prefix(key).is_some_and(|r| r.trim_start().starts_with('='));
    let is_commented = |l: &str| {
        l.trim_start().strip_prefix('#').is_some_and(|r| r.trim_start().strip_prefix(key).is_some_and(|r| r.trim_start().starts_with('=')))
    };

    if let Some(i) = lines.iter().position(|l| is_active(l)) {
        lines[i] = new_line;
    } else if let Some(i) = lines.iter().position(|l| is_commented(l)) {
        lines[i] = new_line;
    } else {
        lines.push(new_line);
    }
    let mut out = lines.join(eol);
    out.push_str(eol);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quickapp::plan::{Permission, PlannedStep};
    use std::sync::Mutex as StdMutex;

    #[test]
    fn env_upsert_replaces_uncomments_and_appends() {
        let env = "APP_NAME=Laravel\n# DB_HOST=127.0.0.1\nDB_PORT=3306\n";
        let out = upsert_env(env, "DB_PORT", "3307");
        assert!(out.contains("DB_PORT=3307") && !out.contains("DB_PORT=3306"));
        let out = upsert_env(&out, "DB_HOST", "127.0.0.1");
        assert!(out.contains("\nDB_HOST=127.0.0.1\n") && !out.contains("# DB_HOST"), "commented line is activated in place");
        let out = upsert_env(&out, "MAIL_HOST", "127.0.0.1");
        assert!(out.ends_with("MAIL_HOST=127.0.0.1\n"));
        assert!(out.starts_with("APP_NAME=Laravel\n"), "untouched lines survive");
        // Running twice changes nothing.
        assert_eq!(upsert_env(&out, "MAIL_HOST", "127.0.0.1"), out);
        // Values with spaces are quoted.
        assert!(upsert_env("", "APP_NAME", "My Shop").contains("APP_NAME=\"My Shop\""));
        // A key that merely starts with another key isn't confused with it.
        let out = upsert_env("DB_PORT_X=1\n", "DB_PORT", "5");
        assert!(out.contains("DB_PORT_X=1") && out.contains("DB_PORT=5"));
        // CRLF files stay CRLF.
        assert!(upsert_env("A=1\r\n", "B", "2").contains("\r\n"));
    }

    struct FakeHost {
        calls: StdMutex<Vec<String>>,
        fail_action: Option<&'static str>,
    }

    impl QuickHost for FakeHost {
        fn ensure_runtime(&self, id: &str, _v: Option<&str>, _log: &mut dyn FnMut(&str)) -> Result<String, String> {
            self.calls.lock().unwrap().push(format!("ensure {id}"));
            Ok(format!("{id} ready"))
        }
        fn resolve_program(&self, program: &str, _values: &BTreeMap<String, String>) -> Result<ResolvedProgram, String> {
            self.calls.lock().unwrap().push(format!("resolve {program}"));
            #[cfg(windows)]
            return Ok(ResolvedProgram { executable: "cmd".into(), pre_args: vec!["/C".into()], ..Default::default() });
            #[cfg(not(windows))]
            Ok(ResolvedProgram { executable: "sh".into(), pre_args: vec!["-c".into()], ..Default::default() })
        }
        fn action(&self, action: &str, _with: &BTreeMap<String, String>, _ctx: &RunCtx, _log: &mut dyn FnMut(&str)) -> Result<ActionOutcome, String> {
            self.calls.lock().unwrap().push(format!("action {action}"));
            if self.fail_action == Some(action) {
                return Err("boom".into());
            }
            Ok(ActionOutcome { open_url: Some("https://x.test".into()), ..Default::default() })
        }
        fn run_elevated(&self, _e: &Path, _a: &[String]) -> Result<(), String> {
            self.calls.lock().unwrap().push("elevated".into());
            Ok(())
        }
    }

    fn step(name: &str, body: StepBody) -> PlannedStep {
        PlannedStep { stage: "create".into(), name: name.into(), display: name.into(), body, elevated: false, allow_failure: false }
    }

    fn plan(steps: Vec<PlannedStep>, dir: &Path) -> RunPlan {
        let mut values = BTreeMap::new();
        values.insert("parent_dir".to_string(), dir.display().to_string());
        values.insert("db_pass".to_string(), "hunter2-secret".to_string());
        let mut display_values = values.clone();
        display_values.insert("db_pass".into(), "••••••••".into());
        RunPlan {
            app_id: "t".into(),
            app_name: "Test".into(),
            display_values,
            project_path: None,
            hostname: None,
            https: false,
            steps,
            permissions: Vec::<Permission>::new(),
            requirements: vec![],
            warnings: vec![],
            values,
        }
    }

    fn wait_done(mgr: &RunManager, id: &str) -> RunView {
        for _ in 0..400 {
            let v = mgr.get(id).unwrap();
            if v.state != RunState::Running {
                return v;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        panic!("run did not finish");
    }

    fn echo(text: &str) -> StepBody {
        StepBody::Run { program: "echo".into(), args: vec![format!("echo {text}")], cwd: None }
    }

    #[test]
    fn a_successful_run_executes_every_step_in_order_and_reports_results() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("sub").join("hello.txt");
        let host = Arc::new(FakeHost { calls: StdMutex::new(vec![]), fail_action: None });
        let mgr = RunManager::new();
        let id = mgr.start(
            plan(
                vec![
                    step("Runtime", StepBody::EnsureRuntime { id: "php".into(), version: None }),
                    step("Write file", StepBody::WriteFile { path: file.display().to_string(), content: "hi".into(), overwrite: true }),
                    step("Run it", echo("visible-output")),
                    step("Act", StepBody::Action { action: "create_domain".into(), with: BTreeMap::new() }),
                ],
                dir.path(),
            ),
            host.clone(),
            false,
        );
        let v = wait_done(&mgr, &id);
        assert_eq!(v.state, RunState::Succeeded, "{:?}", v.error);
        assert!(v.steps.iter().all(|s| s.status == StepStatus::Done));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "hi");
        assert!(v.log.iter().any(|l| l.contains("visible-output")), "command output must be streamed into the log");
        assert_eq!(v.open_url.as_deref(), Some("https://x.test"));
        assert_eq!(v.results.iter().map(|r| r.label.as_str()).collect::<Vec<_>>(), ["php ready", "Write file", "Run it", "Act"]);
        assert!(v.results.iter().all(|r| r.ok));
    }

    #[test]
    fn a_failing_step_stops_the_run_and_skips_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let host = Arc::new(FakeHost { calls: StdMutex::new(vec![]), fail_action: Some("create_database") });
        let mgr = RunManager::new();
        let id = mgr.start(
            plan(
                vec![
                    step("One", echo("one")),
                    step("Boom", StepBody::Action { action: "create_database".into(), with: BTreeMap::new() }),
                    step("Never", echo("never")),
                ],
                dir.path(),
            ),
            host,
            false,
        );
        let v = wait_done(&mgr, &id);
        assert_eq!(v.state, RunState::Failed);
        assert_eq!(v.steps[0].status, StepStatus::Done);
        assert_eq!(v.steps[1].status, StepStatus::Failed);
        assert_eq!(v.steps[2].status, StepStatus::Skipped);
        assert!(v.error.unwrap().contains("boom"));
        assert!(!v.log.iter().any(|l| l.contains("never")));
    }

    #[test]
    fn allow_failure_steps_are_reported_but_do_not_stop_the_run() {
        let dir = tempfile::tempdir().unwrap();
        let host = Arc::new(FakeHost { calls: StdMutex::new(vec![]), fail_action: Some("health_check") });
        let mgr = RunManager::new();
        let mut soft = step("Health", StepBody::Action { action: "health_check".into(), with: BTreeMap::new() });
        soft.allow_failure = true;
        let id = mgr.start(plan(vec![soft, step("After", echo("after"))], dir.path()), host, false);
        let v = wait_done(&mgr, &id);
        assert_eq!(v.state, RunState::Succeeded);
        assert!(!v.results[0].ok, "the soft failure is still shown as failed");
        assert!(v.results[1].ok);
    }

    #[test]
    fn nonzero_exit_codes_fail_the_step() {
        let dir = tempfile::tempdir().unwrap();
        let host = Arc::new(FakeHost { calls: StdMutex::new(vec![]), fail_action: None });
        let mgr = RunManager::new();
        let id = mgr.start(plan(vec![step("Bad", StepBody::Run { program: "x".into(), args: vec!["exit 7".into()], cwd: None })], dir.path()), host, false);
        let v = wait_done(&mgr, &id);
        assert_eq!(v.state, RunState::Failed);
        assert!(v.error.unwrap().contains("code 7"));
    }

    #[test]
    fn elevated_steps_are_refused_without_separate_approval() {
        let dir = tempfile::tempdir().unwrap();
        let host = Arc::new(FakeHost { calls: StdMutex::new(vec![]), fail_action: None });
        let mgr = RunManager::new();
        let mut admin = step("Admin thing", echo("x"));
        admin.elevated = true;

        let id = mgr.start(plan(vec![admin.clone()], dir.path()), host.clone(), false);
        let v = wait_done(&mgr, &id);
        assert_eq!(v.state, RunState::Failed);
        assert!(v.error.unwrap().contains("administrator"));
        assert!(!host.calls.lock().unwrap().iter().any(|c| c == "elevated"), "must not elevate without approval");

        let id = mgr.start(plan(vec![admin], dir.path()), host.clone(), true);
        assert_eq!(wait_done(&mgr, &id).state, RunState::Succeeded);
        assert!(host.calls.lock().unwrap().iter().any(|c| c == "elevated"));
    }

    #[test]
    fn secrets_never_reach_the_log() {
        let dir = tempfile::tempdir().unwrap();
        let host = Arc::new(FakeHost { calls: StdMutex::new(vec![]), fail_action: None });
        let mgr = RunManager::new();
        let id = mgr.start(plan(vec![step("Leak", echo("password is hunter2-secret ok"))], dir.path()), host, false);
        let v = wait_done(&mgr, &id);
        let log = v.log.join("\n");
        assert!(!log.contains("hunter2-secret"), "secret leaked into log: {log}");
        assert!(log.contains("••••••••"));
    }

    #[test]
    fn cancel_stops_a_long_running_step() {
        let dir = tempfile::tempdir().unwrap();
        let host = Arc::new(FakeHost { calls: StdMutex::new(vec![]), fail_action: None });
        let mgr = RunManager::new();
        #[cfg(windows)]
        let sleeper = StepBody::Run { program: "x".into(), args: vec!["ping -n 60 127.0.0.1 > nul".into()], cwd: None };
        #[cfg(not(windows))]
        let sleeper = StepBody::Run { program: "x".into(), args: vec!["sleep 60".into()], cwd: None };
        let id = mgr.start(plan(vec![step("Slow", sleeper), step("After", echo("after"))], dir.path()), host, false);
        std::thread::sleep(Duration::from_millis(400));
        let started = std::time::Instant::now();
        mgr.cancel(&id);
        let v = wait_done(&mgr, &id);
        assert!(started.elapsed() < Duration::from_secs(10));
        assert_eq!(v.state, RunState::Cancelled);
        assert_eq!(v.steps[1].status, StepStatus::Skipped);
    }

    #[test]
    fn writefile_respects_overwrite_false() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("keep.txt");
        std::fs::write(&file, "mine").unwrap();
        let host = Arc::new(FakeHost { calls: StdMutex::new(vec![]), fail_action: None });
        let mgr = RunManager::new();
        let id = mgr.start(
            plan(vec![step("W", StepBody::WriteFile { path: file.display().to_string(), content: "theirs".into(), overwrite: false })], dir.path()),
            host,
            false,
        );
        assert_eq!(wait_done(&mgr, &id).state, RunState::Succeeded);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "mine");
    }
}
