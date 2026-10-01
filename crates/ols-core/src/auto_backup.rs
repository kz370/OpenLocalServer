//! Automatic backups (§130).
//!
//! Snapshots (§131) and database dumps (§32, §34, §36) are only taken when the user asks
//! for them. This module takes them on a timetable instead, so a project is not lost
//! because nobody remembered to press the button.
//!
//! - **What is covered** is either the whole app or a chosen set of sites. A site is
//!   backed up through the project that owns it, because that is what a snapshot records.
//! - **What is copied** is chosen: each project gets a snapshot (its `.env` files and its
//!   own files are optional — they can hold secrets or be large), and every database in
//!   scope gets a dump: SQL dumps for MariaDB and PostgreSQL, `.bak` copies for the
//!   registered SQLite files.
//! - **How often** is a schedule — the same cron expressions and names the scheduler uses
//!   (`daily`, `0 3 * * *`, `every 12 hours` → no, names only: `every_minute`, `hourly`,
//!   `daily`, `weekly`, `monthly`, …). It is checked by the clock the app already runs
//!   (§106), so a backup only happens while the app (or `ols daemon`) is open.
//! - **How many to keep** is a number per thing being backed up. Retention only ever
//!   removes files this module wrote: every artifact it creates is recorded in the state
//!   below, and only recorded paths are ever deleted. A backup the user took by hand is
//!   never touched, however old it is.
//!
//! Backups land where the manual ones already do — `data/snapshots/<project>/` for
//! snapshots, `data/backups/<engine>/` for SQL, beside the file for SQLite — so the
//! existing lists, restore dialogs and delete buttons find them with no extra wiring.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use chrono::Local;
use serde::{Deserialize, Serialize};

use crate::app::Inner;
use crate::error::CoreError;
use crate::manifest;
use crate::project::Project;
use crate::scheduler::{describe, Schedule};
use crate::snapshots::SnapshotOptions;

const KEY: &str = "auto_backup";
const STATE_KEY: &str = "auto_backup_state";
/// Which sites asked to be backed up, as `{hostname: true}`. The choice is made per site in
/// the site's own settings dialog, where the site is; this map is what that leaves behind.
/// A hostname is the key rather than a project id because the user is choosing a site, and
/// because a project can gain or lose a site without the choice moving.
const SITES_KEY: &str = "auto_backup.sites";
/// The label every automatic snapshot carries, so it is obvious in the list.
const LABEL: &str = "auto";
/// The SQL servers a dump can be taken from. MongoDB has no dump tool here.
const SQL_ENGINES: &[&str] = &["mariadb", "postgres"];
/// A run already going must never be doubled by the clock or by the button.
static RUNNING: AtomicBool = AtomicBool::new(false);

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn svc(msg: impl Into<String>) -> CoreError {
    CoreError::ServiceError(msg.into())
}

// ------------------------------------------------------------------------ settings

/// Which projects an automatic backup covers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoBackupScope {
    /// Every registered project and every database the app knows about.
    #[default]
    App,
    /// Only the listed projects, and the databases they declare.
    Site,
}

/// Everything the user chooses about automatic backups, in the `auto_backup` setting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutoBackupSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub scope: AutoBackupScope,
    /// Cron expression, or a name the scheduler understands (`daily`).
    #[serde(default = "default_schedule")]
    pub schedule: String,
    /// How many backups of each thing to keep. Old automatic ones are deleted.
    #[serde(default = "default_keep")]
    pub keep: u32,
    /// Take a snapshot of every project in scope.
    #[serde(default = "yes")]
    pub snapshots: bool,
    /// Put the project's `.env*` files in its snapshot.
    #[serde(default = "yes")]
    pub include_env: bool,
    /// Put the project's own files in its snapshot (without node_modules, vendor, …).
    #[serde(default = "yes")]
    pub include_files: bool,
    /// Dump every database in scope (MariaDB, PostgreSQL, and SQLite files).
    #[serde(default = "yes")]
    pub databases: bool,
}

/// What one site records, kept in the site's own Backups page. `schedule` and `keep` are
/// `None` while the site follows the app-wide plan, and set once the plan is scoped to
/// chosen sites — a busy site wants an hourly backup and a forgotten one wants a monthly
/// one, and one number for both is a number that is wrong for at least one of them.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AutoBackupSiteSettings {
    pub enabled: bool,
    /// Cron expression or a scheduler name, for this site only.
    #[serde(default)]
    pub schedule: Option<String>,
    /// How many of this site's backups to keep.
    #[serde(default)]
    pub keep: Option<u32>,
}

impl AutoBackupSiteSettings {
    /// Refuses a per-site plan that could not run, naming the site and the way out.
    fn validate(&self, hostname: &str) -> Result<(), String> {
        if let Some(schedule) = &self.schedule {
            Schedule::parse(schedule).map_err(|e| {
                format!("Automatic backups cannot run for {hostname} on \"{schedule}\": {e}")
            })?;
        }
        if self.keep.is_some_and(|k| !(1..=200).contains(&k)) {
            return Err(format!(
                "Keep {} backup(s) for {hostname} is not usable: the number must be between 1 and 200.",
                self.keep.unwrap_or(0)
            ));
        }
        Ok(())
    }

    /// The site's own number, or the plan's.
    fn keep_or(&self, fallback: u32) -> u32 {
        self.keep.unwrap_or(fallback).clamp(1, 200)
    }

    /// The site's own period, or the plan's.
    fn schedule_or<'a>(&'a self, fallback: &'a str) -> &'a str {
        self.schedule.as_deref().unwrap_or(fallback)
    }
}

fn default_schedule() -> String {
    "daily".into()
}

fn default_keep() -> u32 {
    7
}

fn yes() -> bool {
    true
}

impl Default for AutoBackupSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            scope: AutoBackupScope::default(),
            schedule: default_schedule(),
            keep: default_keep(),
            snapshots: true,
            include_env: true,
            include_files: true,
            databases: true,
        }
    }
}

impl AutoBackupSettings {
    /// Refuses a plan that could not run, saying what is wrong and how to fix it.
    pub fn validate(&self) -> Result<(), String> {
        Schedule::parse(&self.schedule)
            .map_err(|e| format!("Automatic backups cannot run on \"{}\": {e}", self.schedule))?;
        if !(1..=200).contains(&self.keep) {
            return Err(format!(
                "Keep {} backup(s) per project and database is not usable: the number must be between 1 and 200. Set it to 7 to keep a week of daily backups.",
                self.keep
            ));
        }
        if !self.snapshots && !self.databases {
            return Err(
                "Automatic backups are switched on but nothing is selected to back up. Turn on sites, databases, or both — or switch automatic backups off."
                    .into(),
            );
        }
        Ok(())
    }
}

// ------------------------------------------------------------------------ state

/// One file this module wrote, so only these are ever deleted by retention.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutoArtifact {
    pub path: String,
    pub created_ms: u64,
}

/// What automatic backups have written, newest first inside each bucket, and how the last
/// run went. A bucket is one project, one SQL database or one SQLite file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AutoBackupState {
    #[serde(default)]
    pub buckets: BTreeMap<String, Vec<AutoArtifact>>,
    #[serde(default)]
    pub last_run: Option<AutoBackupRun>,
}

/// The outcome of one pass, in words the UI can show without re-reading the file system.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AutoBackupRun {
    pub started_ms: u64,
    pub finished_ms: u64,
    /// "snapshot shop", "mariadb/shop", "sqlite blog.db" — what was written.
    pub created: Vec<String>,
    /// Automatic backups deleted to honour "keep this many".
    pub removed: Vec<String>,
    /// Why something was skipped or failed. Never empty on its own: a partial pass is
    /// still a pass, and the reason belongs in words, not in a log the user never reads.
    pub problems: Vec<String>,
}

impl AutoBackupRun {
    pub fn ok(&self) -> bool {
        self.problems.is_empty()
    }
}

/// One site's own backup settings, as its Backups page leaves them. The choice lives with
/// the site, and the app-wide card only reports the tally.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutoBackupSite {
    pub hostname: String,
    pub project_id: Option<String>,
    /// The project a snapshot would record, when the site belongs to one. `None` for a
    /// site with no project: there is nothing a snapshot can hold yet.
    pub project_name: Option<String>,
    pub enabled: bool,
    /// This site's own period, when it has one; `None` means it follows the app-wide plan.
    pub schedule: Option<String>,
    /// This site's own keep count, when it has one.
    pub keep: Option<u32>,
    /// What the site runs at *now*: its own value, or the plan's. The UI shows this even
    /// when the site has no value of its own, so the page never hides the effective period.
    pub effective_schedule: String,
    pub effective_keep: u32,
    /// The plan covers the whole app, so this site's own period and keep are not in force
    /// and the UI must not offer to edit them.
    pub managed_by_plan: bool,
}

/// What the Settings card renders: the plan, when it next runs, and the last result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoBackupStatus {
    pub settings: AutoBackupSettings,
    /// "daily at 00:00", "every 15 minutes", …
    pub description: String,
    pub next_run_ms: Option<u64>,
    /// A pass is in progress right now.
    pub running: bool,
    pub last_run: Option<AutoBackupRun>,
    /// How many automatic backups are kept per bucket, so the card can show the real count
    /// rather than the setting.
    pub kept: BTreeMap<String, usize>,
    /// Every known site and whether it asked to be backed up, so the card can say how many
    /// there are without owning the list.
    pub sites: Vec<AutoBackupSite>,
}

// ------------------------------------------------------------------------ buckets

/// One project to back up, at the period and keep count that apply to it. Under the `app`
/// scope these are the plan's; under the `site` scope they are the covered site's own.
struct BackupTarget {
    project: Project,
    schedule: String,
    keep: u32,
}

fn snapshot_bucket(project_id: &str) -> String {
    format!("snapshot:{project_id}")
}

fn sql_bucket(engine: &str, database: &str) -> String {
    format!("sql:{engine}/{database}")
}

fn sqlite_bucket(path: &str) -> String {
    format!("sqlite:{}", path.to_lowercase())
}

impl AutoBackupState {
    /// Records a new artifact and deletes whatever now sits past `keep`. Returns the paths
    /// deleted. Artifacts whose file is already gone are simply dropped from the list.
    fn record(&mut self, bucket: String, path: String, keep: u32) -> Vec<String> {
        let created_ms = now_ms();
        let list = self.buckets.entry(bucket).or_default();
        list.retain(|a| a.path != path);
        list.push(AutoArtifact { path, created_ms });
        list.sort_by_key(|a| std::cmp::Reverse(a.created_ms));
        let keep = keep.max(1) as usize;
        let mut removed = Vec::new();
        while list.len() > keep {
            let old = list.remove(list.len() - 1);
            if Path::new(&old.path).is_file() {
                let _ = std::fs::remove_file(&old.path);
                removed.push(old.path);
            }
        }
        removed
    }
}

// ------------------------------------------------------------------------ targets

/// The manifest's database as `(engine, name)`, with MySQL served by MariaDB. `None` for
/// SQLite (a file, backed up separately) and for a project that declares no database.
fn project_database(project: &Project) -> Option<(String, String)> {
    let manifest = manifest::read_manifest(&PathBuf::from(&project.path))
        .ok()
        .flatten()?;
    let db = manifest.database?;
    let engine = match db.engine.trim().to_ascii_lowercase().as_str() {
        "" | "sqlite" => return None,
        "mysql" => "mariadb".to_string(),
        e => e.to_string(),
    };
    let name = db
        .name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| crate::setup::db_name_for(&project.name));
    Some((engine, name))
}

/// Every database in scope, as `(engine, name, owning project)`, deduplicated. The owner
/// is what decides how many of its dumps to keep — a project's own count, or the plan's.
/// Anything in scope that cannot be dumped is named in `run.problems` rather than dropped.
fn sql_targets(
    inner: &Inner,
    settings: &AutoBackupSettings,
    projects: &[Project],
    run: &mut AutoBackupRun,
) -> Vec<(String, String, Option<String>)> {
    let mut out: Vec<(String, String, Option<String>)> = Vec::new();
    if settings.scope == AutoBackupScope::App {
        for engine in SQL_ENGINES {
            out.extend(
                inner
                    .services
                    .list_databases(engine)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|name| ((*engine).to_string(), name, None)),
            );
        }
        // "All databases" has to say what it left out, or it is a smaller promise than the
        // label on the toggle.
        if inner.services.status("mongodb").installed {
            run.problems.push(
                "MongoDB databases were not dumped: OLS has no mongodump in this build, so a document database is not covered by an automatic backup. Export it from mongosh or Compass."
                    .into(),
            );
        }
    } else {
        for project in projects {
            match project_database(project) {
                Some((engine, name)) if SQL_ENGINES.contains(&engine.as_str()) => {
                    out.push((engine, name, Some(project.id.clone())))
                }
                Some((engine, name)) => run.problems.push(format!(
                    "{name} ({engine}) was not dumped: automatic database backups cover MariaDB, PostgreSQL and SQLite files. Back up {engine} by hand."
                )),
                None => {}
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// The registered SQLite files in scope, as `(path, owning project)`: all of them for the
/// whole app, or only those belonging to a project behind a site that asked to be backed
/// up. A file with no project is in scope for the app and out of scope for a site list —
/// it belongs to nothing the user picked.
fn sqlite_targets(
    inner: &Inner,
    settings: &AutoBackupSettings,
    projects: &[Project],
) -> Vec<(String, Option<String>)> {
    let wanted: BTreeSet<&str> = projects.iter().map(|p| p.id.as_str()).collect();
    inner
        .sqlite
        .lock()
        .unwrap()
        .list()
        .into_iter()
        .filter(|s| s.exists)
        .filter(|s| match settings.scope {
            AutoBackupScope::App => true,
            AutoBackupScope::Site => s
                .project_id
                .as_deref()
                .is_some_and(|id| wanted.contains(id)),
        })
        .map(|s| (s.path, s.project_id))
        .collect()
}

// ------------------------------------------------------------------------ the run

impl Inner {
    pub fn auto_backup_settings(&self) -> AutoBackupSettings {
        self.settings
            .lock()
            .unwrap()
            .get(KEY)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default()
    }

    /// Stores the plan after checking it.
    pub fn set_auto_backup_settings(
        &self,
        settings: AutoBackupSettings,
    ) -> Result<AutoBackupStatus, CoreError> {
        settings.validate().map_err(svc)?;
        self.settings
            .lock()
            .unwrap()
            .set(KEY.to_string(), serde_json::to_value(&settings)?)?;
        Ok(self.auto_backup_status())
    }

    /// What each site recorded, keyed by hostname. An entry left by an older build was a
    /// plain `true`, which reads as "covered, following the plan".
    fn auto_backup_opt_ins(&self) -> BTreeMap<String, AutoBackupSiteSettings> {
        let raw = self
            .settings
            .lock()
            .unwrap()
            .get(SITES_KEY)
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        if raw.is_null() {
            return BTreeMap::new();
        }
        let whole: BTreeMap<String, AutoBackupSiteSettings> =
            serde_json::from_value(raw.clone()).unwrap_or_default();
        if !whole.is_empty() {
            return whole;
        }
        raw.as_object()
            .map(|m| {
                m.iter()
                    .map(|(host, on)| {
                        (
                            host.clone(),
                            AutoBackupSiteSettings {
                                enabled: on.as_bool().unwrap_or(false),
                                ..Default::default()
                            },
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Every known site with its own backup settings and what they mean right now, so the
    /// site's Backups page and the app-wide card read one list from one place.
    pub fn auto_backup_sites(&self) -> Vec<AutoBackupSite> {
        let opt_ins = self.auto_backup_opt_ins();
        let settings = self.auto_backup_settings();
        let managed_by_plan = settings.scope == AutoBackupScope::App;
        let names: BTreeMap<String, String> = self
            .projects
            .lock()
            .unwrap()
            .list()
            .into_iter()
            .map(|p| (p.id.clone(), p.name.clone()))
            .collect();
        let mut sites: Vec<AutoBackupSite> = self
            .domains
            .lock()
            .unwrap()
            .list()
            .into_iter()
            .map(|d| {
                let project_name = d
                    .project_id
                    .as_deref()
                    .and_then(|id| names.get(id))
                    .cloned();
                let own = opt_ins.get(&d.hostname).cloned().unwrap_or_default();
                AutoBackupSite {
                    effective_schedule: own.schedule_or(&settings.schedule).to_string(),
                    effective_keep: own.keep_or(settings.keep),
                    enabled: own.enabled,
                    schedule: own.schedule.clone(),
                    keep: own.keep,
                    managed_by_plan,
                    hostname: d.hostname,
                    project_id: d.project_id.clone(),
                    project_name,
                }
            })
            .collect();
        sites.sort_by(|a, b| a.hostname.cmp(&b.hostname));
        sites
    }

    /// Records one site's own settings, made in that site's Backups page. A hostname no
    /// site owns is refused, so the map cannot accumulate entries nothing reads.
    pub fn set_site_auto_backup(
        &self,
        hostname: &str,
        site: AutoBackupSiteSettings,
    ) -> Result<AutoBackupStatus, CoreError> {
        let hostname = hostname.trim().to_lowercase();
        if self.domains.lock().unwrap().get(&hostname).is_none() {
            return Err(svc(format!(
                "\"{hostname}\" is not a site any more, so its automatic backup cannot be changed. Reopen the site and try again."
            )));
        }
        let mut site = site;
        // A period is only the site's own business when the plan is scoped to chosen sites;
        // with the whole app covered, the plan's numbers are what run, and a per-site
        // number kept here would be a second answer to the same question.
        if self.auto_backup_settings().scope == AutoBackupScope::App {
            site.schedule = None;
            site.keep = None;
        }
        site.validate(&hostname).map_err(svc)?;
        let mut opt_ins = self.auto_backup_opt_ins();
        if site.enabled {
            opt_ins.insert(hostname, site);
        } else {
            // Nothing of its own to keep once it is off.
            opt_ins.remove(&hostname);
        }
        self.settings
            .lock()
            .unwrap()
            .set(SITES_KEY.to_string(), serde_json::to_value(&opt_ins)?)?;
        Ok(self.auto_backup_status())
    }

    fn auto_backup_state(&self) -> AutoBackupState {
        self.settings
            .lock()
            .unwrap()
            .get(STATE_KEY)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default()
    }

    fn save_auto_backup_state(&self, state: AutoBackupState) -> Result<(), CoreError> {
        self.settings
            .lock()
            .unwrap()
            .set(STATE_KEY.to_string(), serde_json::to_value(&state)?)
    }

    pub fn auto_backup_status(&self) -> AutoBackupStatus {
        let settings = self.auto_backup_settings();
        let state = self.auto_backup_state();
        let now = Local::now();
        // Under the `site` scope every site has its own period, so the next run is the
        // soonest of them rather than the plan's. The plan's own number is still what the
        // app-wide scope runs at, and is what `description` says.
        let next_run_ms = if settings.enabled {
            let mut earliest: Option<chrono::DateTime<Local>> = None;
            let mut run = AutoBackupRun::default();
            for target in self.auto_backup_targets(&settings, &mut run) {
                if let Some(next) = Schedule::parse(&target.schedule)
                    .ok()
                    .and_then(|s| s.next_after(now))
                {
                    earliest = Some(earliest.map_or(next, |e| e.min(next)));
                }
            }
            earliest.map(|d| d.timestamp_millis() as u64)
        } else {
            None
        };
        AutoBackupStatus {
            description: if settings.scope == AutoBackupScope::Site {
                "each site's own period".to_string()
            } else {
                describe(&settings.schedule)
            },
            next_run_ms,
            running: RUNNING.load(Ordering::SeqCst),
            last_run: state.last_run,
            kept: state
                .buckets
                .iter()
                .map(|(k, v)| (k.clone(), v.len()))
                .collect(),
            settings,
            sites: self.auto_backup_sites(),
        }
    }

    /// The projects a plan covers, each with the period and keep count it runs at. Under
    /// the `app` scope every project runs at the plan's numbers. Under the `site` scope it
    /// is every project behind a site that asked, each at its own site's numbers — a
    /// snapshot is a project's, so when two sites of one project disagree the first
    /// (alphabetically, so the answer is stable) is used and the disagreement is said out
    /// loud rather than resolved silently. A site with no project cannot be snapshotted,
    /// and says so rather than being quietly counted.
    fn auto_backup_targets(
        &self,
        settings: &AutoBackupSettings,
        run: &mut AutoBackupRun,
    ) -> Vec<BackupTarget> {
        let all = self.projects.lock().unwrap().list();
        if settings.scope == AutoBackupScope::App {
            return all
                .into_iter()
                .map(|project| BackupTarget {
                    schedule: settings.schedule.clone(),
                    keep: settings.keep,
                    project,
                })
                .collect();
        }
        let covered: Vec<AutoBackupSite> = self
            .auto_backup_sites()
            .into_iter()
            .filter(|s| s.enabled)
            .collect();
        for site in covered.iter().filter(|s| s.project_id.is_none()) {
            run.problems.push(format!(
                "{} was not backed up: it belongs to no project yet, and a snapshot records a project. Point it at a project on its settings tab first.",
                site.hostname
            ));
        }
        let mut out: Vec<BackupTarget> = Vec::new();
        for project in all {
            let sites: Vec<&AutoBackupSite> = covered
                .iter()
                .filter(|s| s.project_id.as_deref() == Some(project.id.as_str()))
                .collect();
            if sites.is_empty() {
                continue;
            }
            let first = sites[0];
            if let Some(other) = sites
                .iter()
                .find(|s| s.schedule != first.schedule || s.keep != first.keep)
            {
                run.problems.push(format!(
                    "{} and {} are sites of the same project, and a snapshot holds the whole project, so one period runs for both ({}). Give them the same period and keep count on their Backups pages.",
                    first.hostname,
                    other.hostname,
                    describe(first.effective_schedule.as_str())
                ));
            }
            out.push(BackupTarget {
                schedule: first.effective_schedule.clone(),
                keep: first.effective_keep,
                project,
            });
        }
        out
    }

    /// One pass: snapshot every project in scope, dump every database in scope, then
    /// delete what "keep" no longer allows. Never returns an error for a per-item
    /// problem — those go into `run.problems` so one bad database does not cost the rest.
    /// `only` limits the pass to the named projects, which is how the clock hands over only
    /// the ones whose own period matched the minute.
    pub fn run_auto_backup(&self) -> Result<AutoBackupRun, CoreError> {
        self.start_pass(None)
    }

    /// The clock's entry point: the same pass, restricted to the projects whose period
    /// matches now.
    pub fn run_auto_backup_due(&self, due: &BTreeSet<String>) -> Result<AutoBackupRun, CoreError> {
        self.start_pass(Some(due))
    }

    fn start_pass(&self, only: Option<&BTreeSet<String>>) -> Result<AutoBackupRun, CoreError> {
        if RUNNING
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(svc(
                "An automatic backup is already running. Wait for it to finish, or check the backups already written on the Projects and Databases pages.",
            ));
        }
        let result = self.auto_backup_pass(only);
        RUNNING.store(false, Ordering::SeqCst);
        result
    }

    fn auto_backup_pass(
        &self,
        only: Option<&BTreeSet<String>>,
    ) -> Result<AutoBackupRun, CoreError> {
        let settings = self.auto_backup_settings();
        let mut run = AutoBackupRun {
            started_ms: now_ms(),
            ..Default::default()
        };
        if let Err(e) = settings.validate() {
            return Err(svc(format!(
                "Automatic backups are switched on but cannot run: {e}"
            )));
        }
        let mut state = self.auto_backup_state();
        let mut targets = self.auto_backup_targets(&settings, &mut run);
        if let Some(due) = only {
            targets.retain(|t| due.contains(&t.project.id));
            if targets.is_empty() {
                return Ok(AutoBackupRun {
                    started_ms: now_ms(),
                    finished_ms: now_ms(),
                    ..Default::default()
                });
            }
        }
        let projects: Vec<Project> = targets.iter().map(|t| t.project.clone()).collect();

        if settings.snapshots {
            let options = SnapshotOptions {
                env: settings.include_env,
                // The data lives in the dumps below, so it is not repeated inside the zip.
                databases: false,
                files: settings.include_files,
            };
            for target in &targets {
                let id = target.project.id.clone();
                let name = target.project.name.clone();
                match self.create_snapshot(&id, LABEL, options) {
                    Ok(info) => {
                        run.created.push(format!("snapshot {name}"));
                        run.removed.extend(state.record(
                            snapshot_bucket(&id),
                            info.path,
                            target.keep,
                        ));
                    }
                    Err(e) => run.problems.push(format!("Snapshot of {name} failed: {e}")),
                }
            }
        }

        if settings.databases {
            // A database belongs to the project that declares it, so its dumps are capped
            // by that project's own keep count; with the whole app covered they follow the
            // plan's.
            for (engine, database, owner) in sql_targets(self, &settings, &projects, &mut run) {
                let bucket = sql_bucket(&engine, &database);
                let keep = owner
                    .as_deref()
                    .and_then(|id| targets.iter().find(|t| t.project.id == id))
                    .map(|t| t.keep)
                    .unwrap_or(settings.keep);
                match self.dump_sql(&engine, &database) {
                    Ok(path) => {
                        run.created.push(format!("{engine}/{database}"));
                        run.removed.extend(state.record(bucket, path, keep));
                    }
                    Err(e) => run.problems.push(format!(
                        "Backup of {engine} database {database} failed: {e}"
                    )),
                }
            }
            if let Ok(exe) = self.sqlite3_path() {
                for (path, owner) in sqlite_targets(self, &settings, &projects) {
                    let bucket = sqlite_bucket(&path);
                    let name = Path::new(&path)
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| path.clone());
                    let keep = owner
                        .as_deref()
                        .and_then(|id| targets.iter().find(|t| t.project.id == id))
                        .map(|t| t.keep)
                        .unwrap_or(settings.keep);
                    match crate::sqlite::backup(&exe, Path::new(&path)) {
                        Ok(dest) => {
                            run.created.push(format!("sqlite {name}"));
                            run.removed.extend(state.record(
                                bucket,
                                dest.display().to_string(),
                                keep,
                            ));
                        }
                        Err(e) => run.problems.push(format!("Backup of {name} failed: {e}")),
                    }
                }
            } else {
                run.problems.push(
                    "SQLite files were left out: the sqlite3 command-line shell is not in this install. Reinstall OLS, or back these files up by hand."
                        .into(),
                );
            }
        }

        if run.created.is_empty() && run.problems.is_empty() {
            run.problems.push(match settings.scope {
                AutoBackupScope::App => format!(
                    "Nothing was backed up: {} in scope. Register a project, or check the folders on the Projects page.",
                    match projects.len() {
                        0 => "there is no project".to_string(),
                        1 => "the one project has no files to copy".to_string(),
                        n => format!("the {n} projects hold nothing to copy"),
                    }
                ),
                AutoBackupScope::Site =>
                    "No site has automatic backups switched on yet, so there was nothing to back up. Open a site's Backups page and turn on its automatic backup."
                        .to_string(),
            });
        }

        run.finished_ms = now_ms();
        state.last_run = Some(run.clone());
        self.save_auto_backup_state(state)?;
        tracing::info!(
            created = run.created.len(),
            removed = run.removed.len(),
            problems = run.problems.len(),
            "automatic backup pass finished"
        );
        Ok(run)
    }

    /// Dumps one SQL database, starting the server first when it is not up — a dump needs
    /// one, and the user asked for a backup, not for a skipped one.
    fn dump_sql(&self, engine: &str, database: &str) -> Result<String, String> {
        if !self.services.status(engine).installed {
            return Err(format!(
                "{engine} is not installed, so there is nothing to dump. Install it on the Services page to back its databases up automatically."
            ));
        }
        if !self.services.is_running(engine) {
            self.start_service_and_wait(engine, &mut |line| {
                tracing::info!(engine = %engine, "automatic backup: {line}");
            })?;
        }
        crate::dbbackup::backup(&self.services, &self.paths, engine, database)
            .map(|p| p.display().to_string())
    }
}

/// The clock hook: the same minute-by-minute wake-up the scheduler uses (§106). Every
/// project is checked against **its own** period, because under the `site` scope a busy
/// site can be hourly while a forgotten one is monthly; only the ones due this minute are
/// handed to the pass. A backup that takes a while must not hold the clock, so the work
/// goes to its own thread and the `RUNNING` flag keeps a slow pass from being doubled.
pub fn clock_tick(inner: &std::sync::Arc<Inner>) {
    let settings = inner.auto_backup_settings();
    if !settings.enabled {
        return;
    }
    if RUNNING.load(Ordering::SeqCst) {
        return;
    }
    let now = Local::now();
    let due: BTreeSet<String> = {
        let mut run = AutoBackupRun::default();
        inner
            .auto_backup_targets(&settings, &mut run)
            .into_iter()
            .filter(|t| Schedule::parse(&t.schedule).is_ok_and(|s| s.matches(&now)))
            .map(|t| t.project.id)
            .collect()
    };
    if due.is_empty() {
        return;
    }
    let core = std::sync::Arc::clone(inner);
    let _ = std::thread::Builder::new()
        .name("ols-auto-backup".into())
        .spawn(move || {
            if let Err(e) = core.run_auto_backup_due(&due) {
                tracing::warn!(error = %e, "automatic backup failed");
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{Core, CoreCommand, CoreResponse};
    use crate::domain::{Domain, Ownership, SiteKind};

    fn core() -> (Core, crate::test_support::IsolatedHome) {
        let home = crate::test_support::isolated_home();
        let settings = crate::settings::SettingsService::load(&home.paths).unwrap();
        (Core::new(settings, home.paths.clone()), home)
    }

    /// A project with a site of its own, the way a user meets it: the folder is registered
    /// and the site points at it.
    fn project(core: &Core, dir: &Path) -> (Project, String) {
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "project".into());
        let hostname = format!("{name}.test");
        std::fs::create_dir_all(dir.join(".openlocalserver")).unwrap();
        std::fs::write(dir.join("index.html"), "hi").unwrap();
        std::fs::write(
            dir.join(".openlocalserver").join("environment.yaml"),
            format!("name: {name}\ndatabase:\n  engine: mariadb\n  name: {name}\n"),
        )
        .unwrap();
        let CoreResponse::Project { project } = core
            .dispatch(CoreCommand::RegisterProject {
                path: dir.display().to_string(),
            })
            .unwrap()
        else {
            panic!()
        };
        core.inner()
            .add_domain(Domain {
                hostname: hostname.clone(),
                project_id: Some(project.id.clone()),
                root: dir.display().to_string(),
                kind: SiteKind::Static,
                https: false,
                redirect_https: false,
                wildcard: false,
                enabled: true,
                ownership: Ownership::Managed,
                app: None,
                blocks: Default::default(),
                generated_hashes: Default::default(),
                public_domain: None,
                tunnel_id: None,
                server: None,
                path_prefix: None,
            })
            .unwrap();
        (project, hostname)
    }

    #[test]
    fn defaults_are_a_daily_weekly_plan_that_is_off() {
        let s = AutoBackupSettings::default();
        assert!(!s.enabled);
        assert_eq!(s.schedule, "daily");
        assert_eq!(s.keep, 7);
        assert_eq!(s.scope, AutoBackupScope::App);
        assert!(s.validate().is_ok());
    }

    #[test]
    fn unusable_plans_say_what_and_how() {
        let bad_schedule = AutoBackupSettings {
            schedule: "every other tuesday".into(),
            ..Default::default()
        };
        assert!(bad_schedule
            .validate()
            .unwrap_err()
            .contains("Automatic backups cannot run"));

        let none_kept = AutoBackupSettings {
            keep: 0,
            ..Default::default()
        };
        assert!(none_kept
            .validate()
            .unwrap_err()
            .contains("between 1 and 200"));

        let nothing = AutoBackupSettings {
            snapshots: false,
            databases: false,
            ..Default::default()
        };
        assert!(nothing
            .validate()
            .unwrap_err()
            .contains("nothing is selected"));

        // A site scope with no site switched on yet is a plan waiting for a site, not a
        // broken one — the site decides in its own dialog, at whatever time the user gets to it.
        let no_site_yet = AutoBackupSettings {
            scope: AutoBackupScope::Site,
            ..Default::default()
        };
        assert!(no_site_yet.validate().is_ok());
    }

    #[test]
    fn a_site_carries_its_own_period_and_keep_count() {
        let (core, home) = core();
        let (shop, hostname) = project(&core, &home.paths.data_dir().join("shop"));
        core.dispatch(CoreCommand::SetAutoBackup {
            settings: AutoBackupSettings {
                enabled: true,
                scope: AutoBackupScope::Site,
                ..Default::default()
            },
        })
        .unwrap();

        // Off until the site asks for it, and the card can see every site's answer.
        let CoreResponse::AutoBackup { status } =
            core.dispatch(CoreCommand::GetAutoBackup).unwrap()
        else {
            panic!()
        };
        let site = status
            .sites
            .iter()
            .find(|s| s.hostname == hostname)
            .unwrap();
        assert!(!site.enabled);
        assert_eq!(site.project_id.as_deref(), Some(shop.id.as_str()));
        assert_eq!(site.project_name.as_deref(), Some("shop"));
        // It follows the plan until it says otherwise.
        assert_eq!(site.schedule, None);
        assert_eq!(site.effective_schedule, "daily");
        assert_eq!(site.effective_keep, 7);
        assert!(!site.managed_by_plan, "the plan is scoped to chosen sites");

        // The site's own Backups page: on, hourly, three kept.
        let CoreResponse::AutoBackup { status } = core
            .dispatch(CoreCommand::SetSiteAutoBackup {
                hostname: hostname.clone(),
                site: AutoBackupSiteSettings {
                    enabled: true,
                    schedule: Some("hourly".into()),
                    keep: Some(3),
                },
            })
            .unwrap()
        else {
            panic!()
        };
        let site = status
            .sites
            .iter()
            .find(|s| s.hostname == hostname)
            .unwrap();
        assert!(site.enabled);
        assert_eq!(site.schedule.as_deref(), Some("hourly"));
        assert_eq!(site.keep, Some(3));
        assert_eq!(site.effective_schedule, "hourly");
        assert_eq!(site.effective_keep, 3);
        assert!(status.next_run_ms.is_some());
        assert_eq!(status.description, "each site's own period");

        // And off again, which removes the entry rather than storing a false.
        let CoreResponse::AutoBackup { status } = core
            .dispatch(CoreCommand::SetSiteAutoBackup {
                hostname: hostname.clone(),
                site: AutoBackupSiteSettings::default(),
            })
            .unwrap()
        else {
            panic!()
        };
        assert!(
            !status
                .sites
                .iter()
                .find(|s| s.hostname == hostname)
                .unwrap()
                .enabled
        );
    }

    #[test]
    fn a_hostname_no_site_owns_is_refused_with_a_way_out() {
        let (core, _home) = core();
        let err = core
            .dispatch(CoreCommand::SetSiteAutoBackup {
                hostname: "gone.test".into(),
                site: AutoBackupSiteSettings {
                    enabled: true,
                    ..Default::default()
                },
            })
            .unwrap_err()
            .cause;
        assert!(err.contains("is not a site any more"), "{err}");
    }

    #[test]
    fn a_sites_own_period_and_keep_are_refused_when_they_cannot_run() {
        let (core, home) = core();
        let (_, hostname) = project(&core, &home.paths.data_dir().join("shop"));
        core.dispatch(CoreCommand::SetAutoBackup {
            settings: AutoBackupSettings {
                enabled: true,
                scope: AutoBackupScope::Site,
                ..Default::default()
            },
        })
        .unwrap();

        for site in [
            AutoBackupSiteSettings {
                enabled: true,
                schedule: Some("every other tuesday".into()),
                keep: None,
            },
            AutoBackupSiteSettings {
                enabled: true,
                schedule: None,
                keep: Some(0),
            },
        ] {
            let err = core
                .dispatch(CoreCommand::SetSiteAutoBackup {
                    hostname: hostname.clone(),
                    site,
                })
                .unwrap_err()
                .cause;
            assert!(err.contains(&hostname), "the message names the site: {err}");
        }
    }

    #[test]
    fn the_app_wide_plan_owns_the_period_when_it_covers_everything() {
        let (core, home) = core();
        let (_, hostname) = project(&core, &home.paths.data_dir().join("shop"));
        // The plan covers the whole app, so a per-site period is not stored: two answers to
        // the same question, one of which silently does nothing, is worse than one.
        core.dispatch(CoreCommand::SetAutoBackup {
            settings: AutoBackupSettings {
                enabled: true,
                scope: AutoBackupScope::App,
                schedule: "hourly".into(),
                keep: 4,
                ..Default::default()
            },
        })
        .unwrap();
        let CoreResponse::AutoBackup { status } = core
            .dispatch(CoreCommand::SetSiteAutoBackup {
                hostname: hostname.clone(),
                site: AutoBackupSiteSettings {
                    enabled: true,
                    schedule: Some("monthly".into()),
                    keep: Some(1),
                },
            })
            .unwrap()
        else {
            panic!()
        };
        let site = status
            .sites
            .iter()
            .find(|s| s.hostname == hostname)
            .unwrap();
        assert!(site.enabled);
        assert_eq!(site.schedule, None, "the plan's period is the one in force");
        assert_eq!(site.effective_schedule, "hourly");
        assert_eq!(site.effective_keep, 4);
        assert!(site.managed_by_plan, "the page must not offer to edit it");
        assert_eq!(status.description, "hourly");
    }

    #[test]
    fn a_projects_sites_that_disagree_are_named_not_resolved_in_silence() {
        let (core, home) = core();
        let (shop, first) = project(&core, &home.paths.data_dir().join("shop"));
        // A second site on the same project, asking for a different period.
        let second = "shop-two.test";
        core.inner()
            .add_domain(Domain {
                hostname: second.into(),
                project_id: Some(shop.id.clone()),
                root: home.paths.data_dir().join("shop").display().to_string(),
                kind: SiteKind::Static,
                https: false,
                redirect_https: false,
                wildcard: false,
                enabled: true,
                ownership: Ownership::Managed,
                app: None,
                blocks: Default::default(),
                generated_hashes: Default::default(),
                public_domain: None,
                tunnel_id: None,
                server: None,
                path_prefix: None,
            })
            .unwrap();
        core.dispatch(CoreCommand::SetAutoBackup {
            settings: AutoBackupSettings {
                enabled: true,
                scope: AutoBackupScope::Site,
                databases: false,
                include_files: false,
                ..Default::default()
            },
        })
        .unwrap();
        for (host, schedule) in [
            (first.clone(), Some("hourly")),
            (second.into(), Some("monthly")),
        ] {
            core.dispatch(CoreCommand::SetSiteAutoBackup {
                hostname: host,
                site: AutoBackupSiteSettings {
                    enabled: true,
                    schedule: schedule.map(str::to_string),
                    keep: None,
                },
            })
            .unwrap();
        }

        let CoreResponse::AutoBackupRun { run } =
            core.dispatch(CoreCommand::RunAutoBackup).unwrap()
        else {
            panic!()
        };
        let warning = run
            .problems
            .iter()
            .find(|p| p.contains("same project"))
            .expect("the disagreement is said out loud");
        assert!(
            warning.contains("shop-two.test") || warning.contains(&first),
            "{warning}"
        );
        assert_eq!(run.created, vec!["snapshot shop"], "one snapshot, not two");
    }

    #[test]
    fn retention_keeps_the_newest_and_only_deletes_what_it_wrote() {
        let home = crate::test_support::isolated_home();
        let mut state = AutoBackupState::default();
        let bucket = snapshot_bucket("p1");
        // A user's own backup, by hand, in the same folder.
        let manual = home
            .paths
            .data_dir()
            .join("snapshots/p1/1700000000-before-release.zip");
        std::fs::create_dir_all(manual.parent().unwrap()).unwrap();
        std::fs::write(&manual, "x").unwrap();

        let mut removed = Vec::new();
        for i in 0..4 {
            let path = home
                .paths
                .data_dir()
                .join(format!("snapshots/p1/170000000{i}-auto.zip"));
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "x").unwrap();
            removed.extend(state.record(bucket.clone(), path.display().to_string(), 2));
        }
        assert_eq!(
            removed.len(),
            2,
            "two of four automatic backups are past the limit"
        );
        for path in &removed {
            assert!(!Path::new(path).exists(), "{path} should be gone");
        }
        assert!(
            manual.is_file(),
            "a backup the user took by hand is never deleted"
        );

        let kept = state.buckets.get(&bucket).unwrap();
        assert_eq!(kept.len(), 2);
        assert!(kept.iter().all(|a| a.path.ends_with("auto.zip")));
    }

    #[test]
    fn a_pass_snapshots_every_project_in_scope_and_caps_the_list() {
        let (core, home) = core();
        let (shop, _) = project(&core, &home.paths.data_dir().join("shop"));
        let (blog, _) = project(&core, &home.paths.data_dir().join("blog"));

        for _ in 0..3 {
            let CoreResponse::AutoBackupRun { run } = core
                .dispatch(CoreCommand::SetAutoBackup {
                    settings: AutoBackupSettings {
                        enabled: true,
                        keep: 2,
                        databases: false,
                        ..Default::default()
                    },
                })
                .and_then(|_| core.dispatch(CoreCommand::RunAutoBackup))
                .unwrap()
            else {
                panic!()
            };
            assert_eq!(run.created.len(), 2, "both projects are in scope");
        }

        for id in [&shop.id, &blog.id] {
            let listed = core
                .inner()
                .list_snapshots(id)
                .into_iter()
                .map(|s| s.label)
                .collect::<Vec<_>>();
            assert_eq!(listed, vec!["auto", "auto"], "keep = 2 for {id}");
        }

        let CoreResponse::AutoBackup { status } =
            core.dispatch(CoreCommand::GetAutoBackup).unwrap()
        else {
            panic!()
        };
        assert_eq!(status.kept.get(&snapshot_bucket(&shop.id)), Some(&2));
        assert!(!status.running);
    }

    #[test]
    fn a_site_scope_covers_only_the_sites_that_asked() {
        let (core, home) = core();
        let (shop, shop_host) = project(&core, &home.paths.data_dir().join("shop"));
        let (blog, _) = project(&core, &home.paths.data_dir().join("blog"));
        let site_plan = CoreCommand::SetAutoBackup {
            settings: AutoBackupSettings {
                enabled: true,
                scope: AutoBackupScope::Site,
                keep: 2,
                databases: false,
                include_files: false,
                ..Default::default()
            },
        };

        core.dispatch(site_plan).unwrap();
        core.dispatch(CoreCommand::SetSiteAutoBackup {
            hostname: shop_host,
            site: AutoBackupSiteSettings {
                enabled: true,
                ..Default::default()
            },
        })
        .unwrap();
        let CoreResponse::AutoBackupRun { run } =
            core.dispatch(CoreCommand::RunAutoBackup).unwrap()
        else {
            panic!()
        };
        assert_eq!(run.created, vec!["snapshot shop"]);

        assert_eq!(core.inner().list_snapshots(&shop.id).len(), 1);
        assert!(
            core.inner().list_snapshots(&blog.id).is_empty(),
            "a site that never asked is not in scope"
        );
    }

    #[test]
    fn a_site_with_no_project_is_reported_not_hidden() {
        let (core, home) = core();
        project(&core, &home.paths.data_dir().join("shop"));
        // A site that belongs to nothing: nothing a snapshot can hold, and it must say so.
        let CoreResponse::Domain { .. } = core
            .dispatch(CoreCommand::AddDomain {
                domain: Domain {
                    hostname: "loose.test".into(),
                    project_id: None,
                    root: home.paths.data_dir().join("loose").display().to_string(),
                    kind: SiteKind::Static,
                    https: false,
                    redirect_https: false,
                    wildcard: false,
                    enabled: true,
                    ownership: Ownership::Managed,
                    app: None,
                    blocks: Default::default(),
                    generated_hashes: Default::default(),
                    public_domain: None,
                    tunnel_id: None,
                    server: None,
                    path_prefix: None,
                },
            })
            .unwrap()
        else {
            panic!()
        };
        core.dispatch(CoreCommand::SetAutoBackup {
            settings: AutoBackupSettings {
                enabled: true,
                scope: AutoBackupScope::Site,
                databases: false,
                include_files: false,
                ..Default::default()
            },
        })
        .unwrap();
        core.dispatch(CoreCommand::SetSiteAutoBackup {
            hostname: "loose.test".into(),
            site: AutoBackupSiteSettings {
                enabled: true,
                ..Default::default()
            },
        })
        .unwrap();

        let CoreResponse::AutoBackupRun { run } =
            core.dispatch(CoreCommand::RunAutoBackup).unwrap()
        else {
            panic!()
        };
        assert!(
            run.problems
                .iter()
                .any(|p| p.contains("belongs to no project")),
            "{:?}",
            run.problems
        );
        assert!(run.created.is_empty());
    }
}
