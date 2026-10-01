//! Background tasks (§164) — work the user starts and watches instead of waiting on.
//!
//! Exporting several sites, importing a bundle or taking a safety snapshot all outlive the
//! click that started them, and all of them want the same three things: a list of what is
//! running, how far along each item is, and what went wrong without losing the items that
//! did work. A task is therefore a record plus a thread, not a future the UI has to poll a
//! closure for.
//!
//! A task is **linear in its steps**: `begin_step` / `end_step` move a cursor forward, so a
//! caller describes what it is doing by naming the step it starts rather than by maintaining
//! a list of states. Every mutation publishes the whole view, so a UI can render straight
//! from the event and never has to reconcile. Progress ticks are throttled: copying a
//! database reports bytes, and a report per chunk froze the Runtimes page before
//! (`runtime.rs`, `ProgressThrottle`).
//!
//! Tasks are in memory only. A task that was running when the app stopped is reported by the
//! operation journal (`journal.rs`) instead — that is the record that survives a restart.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::error::Diagnostic;

/// Finished tasks kept for the history. The oldest *finished* ones go first; a running task
/// is never dropped.
const KEEP: usize = 200;

/// How long between two progress reports of the same task.
const THROTTLE: Duration = Duration::from_millis(200);

/// How many events a slow reader may fall behind before it misses one. The UI re-reads the
/// whole list after a `Lagged`, so a dropped event is a redraw, not a lost task.
const CHANNEL: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Running,
    Succeeded,
    Failed,
    /// Stopped on request. Deliberate, so it is never dressed up as a failure.
    Cancelled,
}

impl TaskState {
    pub fn is_running(self) -> bool {
        self == TaskState::Running
    }

    pub fn is_finished(self) -> bool {
        !self.is_running()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepState {
    Running,
    Done,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStep {
    pub label: String,
    pub state: StepState,
    pub detail: Option<String>,
}

impl TaskStep {
    fn new(label: impl Into<String>) -> Self {
        TaskStep {
            label: label.into(),
            state: StepState::Running,
            detail: None,
        }
    }
}

/// How far along a task is: items finished of items known, and bytes when the step reports
/// them. `total: 0` means "not counted", so a bar is shown as indeterminate rather than as
/// a full or an empty one.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct TaskProgress {
    pub done: usize,
    pub total: usize,
    pub bytes: u64,
    #[serde(default)]
    pub bytes_total: Option<u64>,
}

impl TaskProgress {
    /// 0..=1 when the total is known, `None` when it is not.
    pub fn fraction(&self) -> Option<f32> {
        (self.total > 0).then(|| (self.done as f32 / self.total as f32).clamp(0.0, 1.0))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskView {
    pub id: String,
    /// "export_sites", "import_sites", ...
    pub kind: String,
    /// "Exporting 3 sites" — the one line the list shows.
    pub title: String,
    /// What it works on: a destination folder, a bundle path.
    pub target: String,
    pub state: TaskState,
    pub steps: Vec<TaskStep>,
    pub progress: TaskProgress,
    /// What each item contributed, in order.
    pub results: Vec<String>,
    /// What could not be done. A problem never cancels the rest of the task.
    pub problems: Vec<String>,
    pub error: Option<Diagnostic>,
    pub started_ms: u64,
    pub finished_ms: Option<u64>,
    pub cancel_requested: bool,
}

impl TaskView {
    /// The step in flight, if any: the label to show next to the progress bar.
    pub fn current_step(&self) -> Option<&str> {
        self.steps
            .iter()
            .rev()
            .find(|s| s.state == StepState::Running)
            .map(|s| s.label.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TaskEvent {
    Started { task: Box<TaskView> },
    Progress { task: Box<TaskView> },
    Finished { task: Box<TaskView> },
}

struct Task {
    view: Mutex<TaskView>,
    cancel: AtomicBool,
    last_tick: Mutex<Instant>,
    events: broadcast::Sender<TaskEvent>,
}

impl Task {
    fn lock(&self) -> std::sync::MutexGuard<'_, TaskView> {
        self.view.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Publishes the whole view. A UI renders straight from this and never has to reconcile
    /// a delta against what it already had.
    fn emit(&self, kind: TaskEventKind) {
        if kind == TaskEventKind::Throttled {
            let mut last = self.last_tick.lock().unwrap_or_else(|e| e.into_inner());
            if last.elapsed() < THROTTLE {
                return;
            }
            *last = Instant::now();
        }
        let view = Box::new(self.lock().clone());
        let event = match kind {
            TaskEventKind::Started => TaskEvent::Started { task: view },
            TaskEventKind::Finished => TaskEvent::Finished { task: view },
            _ => TaskEvent::Progress { task: view },
        };
        // No subscribers is normal: the CLI runs the same core with no UI listening.
        let _ = self.events.send(event);
    }
}

/// What a running task uses to report. Dropping it without a terminal call fails the task, so
/// a worker that panics cannot leave a spinner on the page forever.
pub struct TaskHandle {
    task: Arc<Task>,
}

impl TaskHandle {
    /// A second handle to the same task, for the drop guard. Both are cheap — one `Arc` —
    /// and the guard needs its own because the worker is given the other one by value and a
    /// worker that unwinds never returns it.
    fn sharing(&self) -> Self {
        TaskHandle {
            task: Arc::clone(&self.task),
        }
    }

    pub fn id(&self) -> String {
        self.task.lock().id.clone()
    }

    /// True once the user asked to stop. Long steps check this between items.
    pub fn cancel_requested(&self) -> bool {
        self.task.cancel.load(Ordering::SeqCst)
    }

    /// `Err` when the task was cancelled — the shape a `?` in a worker can propagate.
    pub fn check_cancelled(&self) -> Result<(), crate::error::CoreError> {
        if self.cancel_requested() {
            Err(crate::error::CoreError::Failed {
                problem: "The task was stopped.".into(),
                cause: "You asked for it to stop, so the remaining work was left undone.".into(),
                fix: Some("Start it again when you are ready.".into()),
            })
        } else {
            Ok(())
        }
    }

    /// Starts a step, closing the one in flight (if any) as done.
    pub fn begin_step(&self, label: &str) {
        let mut view = self.task.lock();
        if let Some(last) = view.steps.last_mut() {
            if last.state == StepState::Running {
                last.state = StepState::Done;
            }
        }
        view.steps.push(TaskStep::new(label));
        drop(view);
        self.emit(TaskEventKind::Progress);
    }

    /// Closes the step in flight with a note, e.g. "2 database(s) dumped".
    pub fn end_step(&self, detail: Option<String>) {
        {
            let mut view = self.task.lock();
            match view.steps.last_mut() {
                Some(s) if s.state == StepState::Running => {
                    s.state = StepState::Done;
                    s.detail = detail;
                }
                _ => {}
            }
        }
        self.emit(TaskEventKind::Progress);
    }

    /// Closes the step in flight as failed; the task itself may still go on.
    pub fn fail_step(&self, detail: impl Into<String>) {
        {
            let mut view = self.task.lock();
            match view.steps.last_mut() {
                Some(s) if s.state == StepState::Running => {
                    s.state = StepState::Failed;
                    s.detail = Some(detail.into());
                }
                _ => {}
            }
        }
        self.emit(TaskEventKind::Progress);
    }

    pub fn skip_step(&self, detail: impl Into<String>) {
        self.end_step(Some(detail.into()));
    }

    pub fn set_total(&self, total: usize) {
        self.task.lock().progress.total = total;
        self.emit(TaskEventKind::Progress);
    }

    /// A new phase of the same task, with its own denominator: the step changes and the count
    /// starts at zero again. An export reads one item per site and then writes one item per
    /// file, so one `total` for both would either stall at the site count or finish long
    /// before the writing is done — which is exactly a bar that sits still and then jumps.
    pub fn set_phase(&self, step: &str, total: usize) {
        if self.cancel_requested() {
            return;
        }
        {
            let mut view = self.task.lock();
            view.progress.step = step.to_string();
            view.progress.total = total as u64;
            view.progress.done = 0;
            view.progress.bytes = 0;
            view.progress.bytes_total = None;
        }
        self.emit(TaskEventKind::Progress);
    }

    /// One item done. Throttled, so a 400-file copy does not publish 400 events.
    pub fn advance(&self, by: usize) {
        self.task.lock().progress.done += by;
        self.emit(TaskEventKind::Throttled);
    }

    /// Bytes read so far, when the step can report them.
    pub fn set_bytes(&self, bytes: u64, total: Option<u64>) {
        {
            let mut view = self.task.lock();
            view.progress.bytes = bytes;
            view.progress.bytes_total = total;
        }
        self.emit(TaskEventKind::Throttled);
    }

    /// One line of result: what an item contributed.
    pub fn result(&self, line: impl Into<String>) {
        self.task.lock().results.push(line.into());
        self.emit(TaskEventKind::Progress);
    }

    /// One thing that could not be done. Collected, never raised: a single bad database
    /// must not cost the user the rest of the export.
    pub fn problem(&self, line: impl Into<String>) {
        self.task.lock().problems.push(line.into());
        self.emit(TaskEventKind::Progress);
    }

    pub fn succeed(&self) {
        self.finish(TaskState::Succeeded, None);
    }

    pub fn fail(&self, error: Diagnostic) {
        self.finish(TaskState::Failed, Some(error));
    }

    pub fn finish_cancelled(&self) {
        self.finish(TaskState::Cancelled, None);
    }

    /// Used by the worker's drop guard: a task still running when its thread ends failed.
    fn finish_unless_done(&self) {
        self.finish(
            TaskState::Failed,
            Some(Diagnostic {
                problem: "The task stopped unexpectedly.".into(),
                cause: "It did not finish, so some of its work may be missing.".into(),
                fix: Some("Start it again; anything already written is reported below.".into()),
            }),
        );
    }

    fn finish(&self, state: TaskState, error: Option<Diagnostic>) {
        {
            let mut view = self.task.lock();
            if view.state.is_finished() {
                return;
            }
            view.state = state;
            view.finished_ms = Some(now_ms());
            view.error = error;
            if let Some(last) = view.steps.last_mut() {
                if last.state == StepState::Running {
                    last.state = match state {
                        TaskState::Succeeded => StepState::Done,
                        TaskState::Failed => StepState::Failed,
                        // `Running` cannot reach here — `finish` returns early on a task
                        // that already ended — but leaving a step reading "running" on a
                        // finished task would be a lie, so it closes as skipped.
                        TaskState::Cancelled | TaskState::Running => StepState::Skipped,
                    };
                }
            }
        }
        self.emit(TaskEventKind::Finished);
    }

    fn emit(&self, kind: TaskEventKind) {
        self.task.emit(kind);
    }
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum TaskEventKind {
    Started,
    Progress,
    Finished,
    Throttled,
}

struct FinishGuard {
    handle: Option<TaskHandle>,
}

impl Drop for FinishGuard {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.finish_unless_done();
        }
    }
}

impl Default for TaskManager {
    fn default() -> Self {
        Self::new()
    }
}

pub struct TaskManager {
    tasks: Mutex<Vec<Arc<Task>>>,
    next: AtomicU64,
    events: broadcast::Sender<TaskEvent>,
}

impl TaskManager {
    pub fn new() -> Self {
        let (events, _) = broadcast::channel(CHANNEL);
        TaskManager {
            tasks: Mutex::new(Vec::new()),
            next: AtomicU64::new(0),
            events,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<TaskEvent> {
        self.events.subscribe()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Arc<Task>>> {
        self.tasks.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Newest first, the order the Processes page shows.
    pub fn list(&self) -> Vec<TaskView> {
        self.lock().iter().rev().map(|t| t.lock().clone()).collect()
    }

    pub fn get(&self, id: &str) -> Option<TaskView> {
        self.lock()
            .iter()
            .find(|t| t.lock().id == id)
            .map(|t| t.lock().clone())
    }

    pub fn is_running(&self, id: &str) -> bool {
        self.get(id).is_some_and(|t| t.state.is_running())
    }

    /// Asks a running task to stop. It stops between items, so a task already mid-write
    /// finishes that write first. Says whether there was a running task to stop.
    pub fn cancel(&self, id: &str) -> bool {
        let Some(task) = self
            .lock()
            .iter()
            .find(|t| t.lock().id == id)
            .map(Arc::clone)
        else {
            return false;
        };
        if !task.lock().state.is_running() {
            return false;
        }
        task.cancel.store(true, Ordering::SeqCst);
        task.lock().cancel_requested = true;
        task.emit(TaskEventKind::Progress);
        true
    }

    /// Drops a finished task from the list. A running one is left alone.
    pub fn dismiss(&self, id: &str) -> bool {
        let mut tasks = self.lock();
        let before = tasks.len();
        tasks.retain(|t| {
            let view = t.lock();
            !(view.id == id && view.state.is_finished())
        });
        tasks.len() != before
    }

    /// Empties the history, keeping whatever is still running.
    pub fn clear_finished(&self) -> usize {
        let mut tasks = self.lock();
        let before = tasks.len();
        tasks.retain(|t| t.lock().state.is_running());
        before - tasks.len()
    }

    /// Starts `work` on its own thread and returns the id to watch it by. `total` is how many
    /// items the progress bar counts; `0` when it is not countable up front.
    pub fn start<F>(&self, kind: &str, title: &str, target: &str, total: usize, work: F) -> String
    where
        F: FnOnce(TaskHandle) + Send + 'static,
    {
        let id = format!("task-{}", self.next.fetch_add(1, Ordering::SeqCst));
        let task = Arc::new(Task {
            view: Mutex::new(TaskView {
                id: id.clone(),
                kind: kind.into(),
                title: title.into(),
                target: target.into(),
                state: TaskState::Running,
                steps: Vec::new(),
                progress: TaskProgress {
                    total,
                    ..Default::default()
                },
                results: Vec::new(),
                problems: Vec::new(),
                error: None,
                started_ms: now_ms(),
                finished_ms: None,
                cancel_requested: false,
            }),
            cancel: AtomicBool::new(false),
            // A new task reports its first progress immediately, not a tick later.
            last_tick: Mutex::new(
                Instant::now()
                    .checked_sub(THROTTLE)
                    .unwrap_or_else(Instant::now),
            ),
            events: self.events.clone(),
        });
        self.lock().push(Arc::clone(&task));
        self.trim();

        let handle = TaskHandle {
            task: Arc::clone(&task),
        };
        handle.emit(TaskEventKind::Started);
        std::thread::Builder::new()
            .name(format!("ols-{kind}"))
            .spawn(move || {
                // The guard keeps its own handle rather than taking the worker's: `work` is
                // given the handle by value, so on a panic or an early return it never comes
                // back, and a guard holding nothing has nothing to fail the task with. That
                // left a panicking worker running forever — a spinner with nothing behind it.
                let _guard = FinishGuard {
                    handle: Some(handle.sharing()),
                };
                work(handle);
            })
            .map(|_| ())
            .unwrap_or_else(|e| {
                // A thread that cannot even start must not leave the task running.
                let mut view = task.lock();
                view.state = TaskState::Failed;
                view.error = Some(Diagnostic {
                    problem: "The task could not start.".into(),
                    cause: e.to_string(),
                    fix: Some("Free some memory or close other work, then try again.".into()),
                });
                view.finished_ms = Some(now_ms());
            });
        id
    }

    /// Keeps the history bounded, oldest finished first.
    fn trim(&self) {
        let mut tasks = self.lock();
        while tasks.len() > KEEP {
            let Some(i) = tasks.iter().position(|t| t.lock().state.is_finished()) else {
                break;
            };
            tasks.remove(i);
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn wait_for<F: Fn(&TaskView) -> bool>(manager: &TaskManager, id: &str, done: F) -> TaskView {
        for _ in 0..600 {
            if let Some(view) = manager.get(id) {
                if done(&view) {
                    return view;
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the task never reached the expected state");
    }

    #[test]
    fn a_task_runs_on_its_own_thread_and_reports_progress() {
        let manager = TaskManager::new();
        let other = manager.start("noop", "Another", "", 0, |h| h.succeed());
        let id = manager.start("export_sites", "Exporting 2 sites", "C:/out", 2, |h| {
            h.begin_step("Collecting");
            h.result("shop.test");
            h.result("blog.test");
            h.advance(1);
            h.advance(1);
            h.end_step(Some("2 site(s)".into()));
            h.succeed();
        });
        assert_ne!(id, other, "every task gets its own id");

        let view = wait_for(&manager, &id, |t| t.state.is_finished());
        assert_eq!(view.state, TaskState::Succeeded);
        assert_eq!(view.progress.done, 2);
        assert_eq!(view.results, ["shop.test", "blog.test"]);
        assert_eq!(view.steps.len(), 1);
        assert_eq!(view.steps[0].state, StepState::Done);
        assert_eq!(view.steps[0].detail.as_deref(), Some("2 site(s)"));
        assert!(view.finished_ms.is_some());
    }

    #[test]
    fn a_finished_task_stays_in_the_list_and_can_be_dismissed() {
        let manager = TaskManager::new();
        let id = manager.start("t", "One", "", 0, |h| h.succeed());
        wait_for(&manager, &id, |t| t.state.is_finished());
        assert!(manager.get(&id).is_some());
        assert!(manager.dismiss(&id));
        assert!(manager.get(&id).is_none());
        assert!(!manager.dismiss(&id), "dismissing twice is not an error");
    }

    #[test]
    fn a_panicking_task_fails_instead_of_running_forever() {
        let manager = TaskManager::new();
        let id = manager.start("t", "Boom", "", 0, |_h| {
            panic!("the worker exploded");
        });
        let view = wait_for(&manager, &id, |t| t.state.is_finished());
        assert_eq!(view.state, TaskState::Failed);
        let error = view.error.expect("a failure says what happened");
        assert!(error.problem.contains("stopped unexpectedly"));
        assert!(error.fix.is_some(), "a failure says how to recover");
    }

    #[test]
    fn a_worker_that_never_finishes_is_reported_as_failed() {
        let manager = TaskManager::new();
        let id = manager.start("t", "Forgotten", "", 0, |_h| {});
        let view = wait_for(&manager, &id, |t| t.state.is_finished());
        assert_eq!(view.state, TaskState::Failed);
    }

    #[test]
    fn cancelling_stops_the_task_at_the_next_item() {
        let manager = TaskManager::new();
        // The worker says when it is between items; the test cancels in that gap, which is
        // exactly when a long task can honour a cancel without losing the item in flight.
        let (tx_ready, rx_ready) = mpsc::channel::<()>();
        let (tx_resume, rx_resume) = mpsc::channel::<()>();
        let id = manager.start("import_sites", "Importing", "", 4, move |h| {
            for i in 0..4 {
                // Where every long task checks: between items, never mid-write.
                if h.check_cancelled().is_err() {
                    h.finish_cancelled();
                    return;
                }
                h.begin_step(&format!("site {i}"));
                h.advance(1);
                h.end_step(Some(format!("site {i} written")));
                if i == 0 {
                    let _ = tx_ready.send(());
                    let _ = rx_resume.recv();
                }
            }
            h.succeed();
        });

        rx_ready
            .recv_timeout(Duration::from_secs(5))
            .expect("the first item finished");
        assert!(manager.cancel(&id), "a running task can be stopped");
        let _ = tx_resume.send(());

        let done = wait_for(&manager, &id, |t| t.state.is_finished());
        assert_eq!(done.state, TaskState::Cancelled);
        assert!(done.cancel_requested);
        assert_eq!(done.progress.done, 1, "the item in flight still counts");
        assert!(!manager.cancel(&id), "a finished task cannot be cancelled");
    }

    #[test]
    fn a_worker_that_checks_for_a_stop_reports_the_recovery() {
        let manager = TaskManager::new();
        // The check has to happen *after* a stop was asked for, so the worker parks until the
        // test has cancelled. Reading it on a task nobody cancelled asserts that a task which
        // was never asked to stop reports that it was.
        let (tx_ready, rx_ready) = mpsc::channel::<()>();
        let (tx_resume, rx_resume) = mpsc::channel::<()>();
        let id = manager.start("t", "Stops", "", 1, move |h| {
            let _ = tx_ready.send(());
            let _ = rx_resume.recv();
            let e = h.check_cancelled().unwrap_err();
            assert!(e.to_string().contains("stopped"));
            h.finish_cancelled();
        });

        rx_ready
            .recv_timeout(Duration::from_secs(5))
            .expect("the worker reached the check");
        assert!(manager.cancel(&id), "a running task can be stopped");
        let _ = tx_resume.send(());

        let view = wait_for(&manager, &id, |t| t.state.is_finished());
        assert_eq!(view.state, TaskState::Cancelled);
        assert!(
            view.error.is_none(),
            "a stop the user asked for is not a failure to explain"
        );
    }

    #[test]
    fn progress_is_reported_with_the_current_step() {
        let manager = TaskManager::new();
        // The worker parks on its second step instead of returning from it. Returning left the
        // task's fate to the `FinishGuard`, which closes a running step as `Failed` — so the
        // predicate below was a race, and it only ever passed because a *broken* guard left the
        // task `Running` forever. A test that needs a task to still be running has to keep it
        // running, rather than rely on nothing stopping it.
        let (tx_resume, rx_resume) = mpsc::channel::<()>();
        let id = manager.start("t", "Two steps", "", 2, move |h| {
            h.begin_step("Dumping databases");
            h.advance(1);
            h.end_step(Some("done".into()));
            h.begin_step("Writing the bundle");
            let _ = rx_resume.recv();
            h.succeed();
        });
        let view = wait_for(&manager, &id, |t| {
            t.steps.len() == 2 && t.steps[1].state == StepState::Running
        });
        assert_eq!(view.current_step(), Some("Writing the bundle"));
        assert_eq!(view.progress.fraction(), Some(0.5));
        assert_eq!(view.state, TaskState::Running, "and it is still going");
        let _ = tx_resume.send(());
        wait_for(&manager, &id, |t| t.state.is_finished());
    }

    #[test]
    fn an_uncountable_task_reports_no_fraction_rather_than_a_full_bar() {
        assert_eq!(TaskProgress::default().fraction(), None);
        assert_eq!(
            TaskProgress {
                done: 3,
                total: 0,
                ..Default::default()
            }
            .fraction(),
            None
        );
        assert_eq!(
            TaskProgress {
                done: 1,
                total: 4,
                ..Default::default()
            }
            .fraction(),
            Some(0.25)
        );
    }

    #[test]
    fn every_event_carries_the_whole_view() {
        let manager = TaskManager::new();
        let mut events = manager.subscribe();
        let id = manager.start("export_sites", "Exporting", "C:/out", 1, |h| {
            h.begin_step("Writing");
            h.result("shop.test.zip");
            h.succeed();
        });
        let mut seen = Vec::new();
        for _ in 0..3 {
            // A broadcast receiver only blocks; the loop below is the bounded form, so a
            // publisher that stops mid-way fails the test instead of hanging it.
            let mut waited = Duration::ZERO;
            loop {
                match events.try_recv() {
                    Ok(TaskEvent::Started { task })
                    | Ok(TaskEvent::Progress { task })
                    | Ok(TaskEvent::Finished { task }) => {
                        seen.push(task.id.clone());
                        break;
                    }
                    // A slow reader falls behind and then re-reads the whole list; a missed
                    // event is a redraw, not a lost task.
                    Err(broadcast::error::TryRecvError::Lagged(_)) => break,
                    Err(broadcast::error::TryRecvError::Empty)
                        if waited < Duration::from_secs(5) =>
                    {
                        std::thread::sleep(Duration::from_millis(10));
                        waited += Duration::from_millis(10);
                    }
                    Err(e) => panic!("expected three events, got {e}"),
                }
            }
        }
        assert_eq!(seen, [id.clone(), id.clone(), id]);
    }

    #[test]
    fn clearing_the_history_keeps_what_is_still_running() {
        let manager = TaskManager::new();
        let done = manager.start("t", "Done", "", 0, |h| h.succeed());
        wait_for(&manager, &done, |t| t.state.is_finished());
        let (tx, rx) = mpsc::channel::<()>();
        let running = manager.start("t", "Running", "", 0, move |_h| {
            let _ = rx.recv();
        });
        // The first event is published before the thread body runs, so the task is known.
        assert!(manager.clear_finished() >= 1);
        assert!(manager.get(&running).is_some());
        assert!(manager.get(&done).is_none());
        let _ = tx.send(());
        wait_for(&manager, &running, |t| t.state.is_finished());
    }
}
