//! Scheduler (§106): commands that run on a timetable, like `php artisan schedule:run`
//! every minute. Schedules are cron expressions (`*/5 * * * *`) or shortcuts
//! (`every_minute`, `every_5_minutes`, `hourly`, `daily`, `weekly`). A task never
//! overlaps itself: if the previous run is still going, that minute is skipped.
//!
//! The clock runs while the app (or `ols daemon`) is open, checking once a minute in
//! local time.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::time::Duration;

use chrono::{Datelike, Local, Timelike};
use serde::{Deserialize, Serialize};

use crate::app::Inner;
use crate::detection::Framework;
use crate::error::CoreError;
use crate::manifest::ScheduledTaskManifest;
use crate::paths::AppPaths;
use crate::process::ProcessId;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScheduledTask {
    pub id: String,
    /// Runs with this project's runtimes and in its folder; `None` for a global task.
    pub project_id: Option<String>,
    pub name: String,
    pub schedule: String,
    pub command: String,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskRun {
    pub started_ms: u64,
    pub process: Option<ProcessId>,
    pub exit_code: Option<i32>,
    /// Skipped because the previous run was still going.
    pub skipped: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStatus {
    pub task: ScheduledTask,
    /// "Every 5 minutes", "Daily at 00:00", ...
    pub description: String,
    pub next_run_ms: Option<u64>,
    pub last_run: Option<TaskRun>,
    pub running: bool,
}

/// The usual scheduler for a framework (`scheduler: true`).
pub fn default_task(framework: &Framework) -> Option<ScheduledTaskManifest> {
    match framework {
        Framework::Laravel => Some(ScheduledTaskManifest {
            name: "Laravel scheduler".into(),
            schedule: "every_minute".into(),
            command: "php artisan schedule:run".into(),
        }),
        _ => None,
    }
}

// ------------------------------------------------------------------------ cron parsing

#[derive(Debug, Clone, PartialEq)]
pub struct Schedule {
    minutes: BTreeSet<u32>,
    hours: BTreeSet<u32>,
    days: BTreeSet<u32>,
    months: BTreeSet<u32>,
    weekdays: BTreeSet<u32>,
    /// Day-of-month and day-of-week both restricted: cron runs when either matches.
    day_or: bool,
}

fn shortcut(s: &str) -> Option<&'static str> {
    Some(
        match s
            .trim()
            .trim_start_matches('@')
            .to_ascii_lowercase()
            .replace([' ', '-'], "_")
            .as_str()
        {
            "every_minute" | "minutely" => "* * * * *",
            "every_2_minutes" => "*/2 * * * *",
            "every_5_minutes" => "*/5 * * * *",
            "every_10_minutes" => "*/10 * * * *",
            "every_15_minutes" => "*/15 * * * *",
            "every_30_minutes" => "*/30 * * * *",
            "hourly" => "0 * * * *",
            "daily" | "midnight" => "0 0 * * *",
            "weekly" => "0 0 * * 0",
            "monthly" => "0 0 1 * *",
            "yearly" | "annually" => "0 0 1 1 *",
            _ => return None,
        },
    )
}

fn field(text: &str, min: u32, max: u32, name: &str) -> Result<BTreeSet<u32>, String> {
    let mut out = BTreeSet::new();
    for part in text.split(',') {
        let (range, step) = match part.split_once('/') {
            Some((r, s)) => (
                r,
                s.parse::<u32>()
                    .map_err(|_| format!("bad step \"{s}\" in the {name} field"))?,
            ),
            None => (part, 1),
        };
        if step == 0 {
            return Err(format!("a step of 0 in the {name} field"));
        }
        let (lo, hi) = if range == "*" {
            (min, max)
        } else if let Some((a, b)) = range.split_once('-') {
            (
                a.parse()
                    .map_err(|_| format!("bad value \"{a}\" in the {name} field"))?,
                b.parse()
                    .map_err(|_| format!("bad value \"{b}\" in the {name} field"))?,
            )
        } else {
            let v: u32 = range
                .parse()
                .map_err(|_| format!("bad value \"{range}\" in the {name} field"))?;
            (v, if part.contains('/') { max } else { v })
        };
        // Sunday is 0 or 7.
        let max_ok = if name == "weekday" { 7 } else { max };
        if lo < min || hi > max_ok || lo > hi {
            return Err(format!(
                "{range} is outside {min}-{max} in the {name} field"
            ));
        }
        out.extend((lo..=hi).step_by(step as usize).map(|v| {
            if name == "weekday" && v == 7 {
                0
            } else {
                v
            }
        }));
    }
    Ok(out)
}

impl Schedule {
    pub fn parse(text: &str) -> Result<Self, String> {
        let expr = shortcut(text).unwrap_or(text.trim());
        let parts: Vec<&str> = expr.split_whitespace().collect();
        if parts.len() != 5 {
            return Err(format!("\"{text}\" needs five fields (minute hour day month weekday) or a name like hourly"));
        }
        Ok(Self {
            minutes: field(parts[0], 0, 59, "minute")?,
            hours: field(parts[1], 0, 23, "hour")?,
            days: field(parts[2], 1, 31, "day")?,
            months: field(parts[3], 1, 12, "month")?,
            weekdays: field(parts[4], 0, 6, "weekday")?,
            day_or: parts[2] != "*" && parts[4] != "*",
        })
    }

    pub fn matches<T: Datelike + Timelike>(&self, t: &T) -> bool {
        let day_ok = self.days.contains(&t.day());
        let weekday_ok = self.weekdays.contains(&t.weekday().num_days_from_sunday());
        let date_ok = if self.day_or {
            day_ok || weekday_ok
        } else {
            day_ok && weekday_ok
        };
        self.minutes.contains(&t.minute())
            && self.hours.contains(&t.hour())
            && self.months.contains(&t.month())
            && date_ok
    }

    /// The next matching minute after `from`, looking up to a year ahead.
    pub fn next_after(&self, from: chrono::DateTime<Local>) -> Option<chrono::DateTime<Local>> {
        let mut t = from.with_second(0)?.with_nanosecond(0)? + chrono::Duration::minutes(1);
        for _ in 0..(366 * 24 * 60) {
            if self.matches(&t) {
                return Some(t);
            }
            t += chrono::Duration::minutes(1);
        }
        None
    }
}

/// Words for a schedule, for lists and plans.
pub fn describe(text: &str) -> String {
    let expr = shortcut(text).unwrap_or(text.trim());
    match expr {
        "* * * * *" => "every minute".into(),
        "0 * * * *" => "hourly".into(),
        "0 0 * * *" => "daily at 00:00".into(),
        "0 0 * * 0" => "weekly on Sunday".into(),
        "0 0 1 * *" => "monthly".into(),
        _ => {
            let p: Vec<&str> = expr.split_whitespace().collect();
            match p.as_slice() {
                [m, "*", "*", "*", "*"] if m.starts_with("*/") => {
                    format!("every {} minutes", &m[2..])
                }
                [m, h, "*", "*", "*"] if m.parse::<u32>().is_ok() && h.parse::<u32>().is_ok() => {
                    format!("daily at {:0>2}:{:0>2}", h, m)
                }
                _ => format!("cron {expr}"),
            }
        }
    }
}

// ------------------------------------------------------------------------ store

pub struct ScheduleStore {
    paths: AppPaths,
    tasks: Vec<ScheduledTask>,
}

impl ScheduleStore {
    pub fn load(paths: &AppPaths) -> Self {
        let tasks = crate::db::load_docs(paths, "schedules").unwrap_or_default();
        Self {
            paths: paths.clone(),
            tasks,
        }
    }

    pub fn list(&self) -> Vec<ScheduledTask> {
        self.tasks.clone()
    }

    pub fn get(&self, id: &str) -> Option<ScheduledTask> {
        self.tasks.iter().find(|t| t.id == id).cloned()
    }

    pub fn save(&mut self, t: ScheduledTask) -> Result<(), CoreError> {
        match self.tasks.iter_mut().find(|x| x.id == t.id) {
            Some(x) => *x = t,
            None => self.tasks.push(t),
        }
        self.persist()
    }

    pub fn remove(&mut self, id: &str) -> Result<(), CoreError> {
        self.tasks.retain(|t| t.id != id);
        self.persist()
    }

    fn persist(&self) -> Result<(), CoreError> {
        let refs: Vec<(String, &ScheduledTask)> =
            self.tasks.iter().map(|t| (t.id.clone(), t)).collect();
        crate::db::save_docs(&self.paths, "schedules", &refs)
    }
}

/// Last run per task id (kept in memory; the Processes page has the output).
#[derive(Default)]
pub struct TaskRuns(pub std::sync::Mutex<HashMap<String, TaskRun>>);

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl Inner {
    pub fn schedules_for(&self, project_id: &str) -> Vec<ScheduledTask> {
        self.schedules
            .lock()
            .unwrap()
            .list()
            .into_iter()
            .filter(|t| t.project_id.as_deref() == Some(project_id))
            .collect()
    }

    pub fn task_statuses(&self, project_id: Option<&str>) -> Vec<TaskStatus> {
        let runs = self.task_runs.0.lock().unwrap();
        let now = Local::now();
        self.schedules
            .lock()
            .unwrap()
            .list()
            .into_iter()
            .filter(|t| project_id.is_none_or(|p| t.project_id.as_deref() == Some(p)))
            .map(|t| {
                let last = runs.get(&t.id).cloned();
                let running = last
                    .as_ref()
                    .and_then(|r| r.process)
                    .is_some_and(|p| self.supervisor.is_alive(p));
                let next = if t.enabled {
                    Schedule::parse(&t.schedule)
                        .ok()
                        .and_then(|s| s.next_after(now))
                        .map(|d| d.timestamp_millis() as u64)
                } else {
                    None
                };
                TaskStatus {
                    description: describe(&t.schedule),
                    next_run_ms: next,
                    last_run: last,
                    running,
                    task: t,
                }
            })
            .collect()
    }

    pub fn save_schedule(&self, mut t: ScheduledTask) -> Result<ScheduledTask, CoreError> {
        Schedule::parse(&t.schedule).map_err(CoreError::ServiceError)?;
        if t.command.trim().is_empty() {
            return Err(CoreError::ServiceError(
                "a scheduled task needs a command".into(),
            ));
        }
        if let Some(p) = &t.project_id {
            if self.projects.lock().unwrap().get(p).is_none() {
                return Err(CoreError::InvalidProjectPath(p.clone()));
            }
        }
        if t.id.is_empty() {
            t.id = crate::workers::worker_id(t.project_id.as_deref().unwrap_or("global"), &t.name);
        }
        self.schedules.lock().unwrap().save(t.clone())?;
        Ok(t)
    }

    pub fn remove_schedule(&self, id: &str) -> Result<(), CoreError> {
        self.schedules.lock().unwrap().remove(id)?;
        self.task_runs.0.lock().unwrap().remove(id);
        Ok(())
    }

    /// Runs a task now (also what the clock calls). A run still going is never doubled.
    pub fn run_task(&self, id: &str) -> Result<TaskRun, CoreError> {
        let t = self
            .schedules
            .lock()
            .unwrap()
            .get(id)
            .ok_or_else(|| CoreError::ServiceError(format!("no scheduled task \"{id}\"")))?;
        let previous = self.task_runs.0.lock().unwrap().get(id).cloned();
        if previous
            .as_ref()
            .and_then(|r| r.process)
            .is_some_and(|p| self.supervisor.is_alive(p))
        {
            let run = TaskRun {
                started_ms: now_ms(),
                skipped: true,
                ..Default::default()
            };
            tracing::info!(task = %t.name, "scheduled task skipped: previous run still going");
            return Ok(run);
        }
        let run = if !self.process_slot_free() {
            TaskRun {
                started_ms: now_ms(),
                error: Some("the process limit in Settings → Resources is reached".into()),
                ..Default::default()
            }
        } else {
            match self.run_command_line(
                &t.command,
                None,
                t.project_id.as_deref(),
                Some(&format!("Scheduled: {}", t.name)),
            ) {
                Ok(p) => TaskRun {
                    started_ms: now_ms(),
                    process: Some(p),
                    ..Default::default()
                },
                Err(e) => TaskRun {
                    started_ms: now_ms(),
                    error: Some(e.to_string()),
                    ..Default::default()
                },
            }
        };
        self.task_runs
            .0
            .lock()
            .unwrap()
            .insert(id.to_string(), run.clone());
        Ok(run)
    }

    /// One clock tick: runs every enabled task due this minute. Also fills in exit codes.
    pub fn scheduler_tick(&self) {
        let now = Local::now();
        {
            let mut runs = self.task_runs.0.lock().unwrap();
            for run in runs.values_mut() {
                if let (Some(p), None) = (run.process, run.exit_code) {
                    if !self.supervisor.is_alive(p) {
                        run.exit_code = self
                            .supervisor
                            .snapshot()
                            .into_iter()
                            .find(|x| x.id == p)
                            .and_then(|x| x.exit_code);
                    }
                }
            }
        }
        let due: Vec<String> = self
            .schedules
            .lock()
            .unwrap()
            .list()
            .into_iter()
            .filter(|t| t.enabled && Schedule::parse(&t.schedule).is_ok_and(|s| s.matches(&now)))
            .map(|t| t.id)
            .collect();
        for id in due {
            if let Err(e) = self.run_task(&id) {
                tracing::warn!(task = %id, error = %e, "scheduled task failed to start");
            }
        }
    }
}

/// Starts the clock thread: wakes at each minute boundary. Stops when the core is dropped.
pub fn start_clock(inner: &Arc<Inner>) {
    let weak = Arc::downgrade(inner);
    std::thread::Builder::new()
        .name("ols-scheduler".into())
        .spawn(move || loop {
            let secs = Local::now().second() as u64;
            std::thread::sleep(Duration::from_secs(60 - secs.min(59)) + Duration::from_millis(200));
            match weak.upgrade() {
                Some(inner) => {
                    inner.scheduler_tick();
                    crate::auto_backup::clock_tick(&inner);
                }
                None => return,
            }
        })
        .expect("failed to start the scheduler thread");
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> chrono::DateTime<Local> {
        Local.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()
    }

    #[test]
    fn shortcuts_and_cron_expressions_parse() {
        assert!(Schedule::parse("every_minute")
            .unwrap()
            .matches(&at(2026, 1, 5, 13, 7)));
        let five = Schedule::parse("every 5 minutes").unwrap();
        assert!(five.matches(&at(2026, 1, 5, 13, 10)) && !five.matches(&at(2026, 1, 5, 13, 11)));
        let daily = Schedule::parse("30 2 * * *").unwrap();
        assert!(daily.matches(&at(2026, 3, 1, 2, 30)) && !daily.matches(&at(2026, 3, 1, 3, 30)));
        let weekdays = Schedule::parse("0 9 * * 1-5").unwrap();
        assert!(weekdays.matches(&at(2026, 9, 28, 9, 0)), "a Monday");
        assert!(!weekdays.matches(&at(2026, 9, 27, 9, 0)), "a Sunday");
        assert!(
            Schedule::parse("0 0 * * 7")
                .unwrap()
                .matches(&at(2026, 9, 27, 0, 0)),
            "7 is Sunday too"
        );
    }

    #[test]
    fn bad_expressions_are_explained() {
        assert!(Schedule::parse("61 * * * *")
            .unwrap_err()
            .contains("minute"));
        assert!(Schedule::parse("* * *")
            .unwrap_err()
            .contains("five fields"));
        assert!(Schedule::parse("*/0 * * * *").is_err());
    }

    #[test]
    fn next_run_and_descriptions() {
        let s = Schedule::parse("hourly").unwrap();
        assert_eq!(
            s.next_after(at(2026, 1, 1, 10, 15)).unwrap(),
            at(2026, 1, 1, 11, 0)
        );
        assert_eq!(describe("*/15 * * * *"), "every 15 minutes");
        assert_eq!(describe("30 2 * * *"), "daily at 02:30");
        assert_eq!(describe("every_minute"), "every minute");
    }
}
