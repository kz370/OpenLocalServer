//! Operation journal (§78, §163). Operations that change a lot at once (applying the web
//! config, restoring a database, importing databases) write a line before they start and
//! another when they end. If the app dies in between, the next start finds an operation
//! still marked as running, marks it interrupted and reports it, instead of leaving the
//! user to wonder what state things are in.

use serde::{Deserialize, Serialize};

use crate::command::CoreCommand;
use crate::paths::AppPaths;

/// Entries kept; the oldest finished ones go first.
const KEEP: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpStatus {
    Running,
    Done,
    Failed,
    /// Was running when the app stopped.
    Interrupted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Operation {
    pub id: u64,
    /// "apply_web", "restore_database", ...
    pub kind: String,
    pub title: String,
    pub started_ms: u64,
    pub finished_ms: Option<u64>,
    pub status: OpStatus,
    /// The error, when it failed.
    pub detail: Option<String>,
    /// How to undo it, in words, when there is a way.
    pub undo: Option<String>,
    /// Running this again is the safe way to finish the job.
    pub retry: Option<CoreCommand>,
}

pub struct Journal {
    paths: AppPaths,
    entries: Vec<Operation>,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl Journal {
    /// Loads the journal and marks whatever was still running as interrupted. A missing or
    /// unreadable DB is an empty journal; the app must still start.
    pub fn load(paths: &AppPaths) -> Self {
        let mut entries: Vec<Operation> =
            crate::db::load_docs(paths, "operations").unwrap_or_default();
        let mut changed = false;
        for e in entries.iter_mut().filter(|e| e.status == OpStatus::Running) {
            e.status = OpStatus::Interrupted;
            changed = true;
        }
        let journal = Self {
            paths: paths.clone(),
            entries,
        };
        if changed {
            journal.persist();
        }
        journal
    }

    pub fn list(&self) -> Vec<Operation> {
        let mut all = self.entries.clone();
        all.reverse();
        all
    }

    pub fn interrupted(&self) -> Vec<Operation> {
        self.entries
            .iter()
            .filter(|e| e.status == OpStatus::Interrupted)
            .cloned()
            .collect()
    }

    /// Records the start of an operation and returns its id. An older interrupted run of the
    /// same operation is dropped: starting it again is the answer to it.
    pub fn begin(
        &mut self,
        kind: &str,
        title: &str,
        undo: Option<&str>,
        retry: Option<CoreCommand>,
    ) -> u64 {
        self.entries
            .retain(|e| !(e.status == OpStatus::Interrupted && e.kind == kind && e.title == title));
        let id = self.entries.iter().map(|e| e.id).max().unwrap_or(0) + 1;
        self.entries.push(Operation {
            id,
            kind: kind.to_string(),
            title: title.to_string(),
            started_ms: now_ms(),
            finished_ms: None,
            status: OpStatus::Running,
            detail: None,
            undo: undo.map(str::to_string),
            retry,
        });
        while self.entries.len() > KEEP {
            match self
                .entries
                .iter()
                .position(|e| matches!(e.status, OpStatus::Done | OpStatus::Failed))
            {
                Some(i) => {
                    self.entries.remove(i);
                }
                None => break,
            }
        }
        self.persist();
        id
    }

    pub fn finish(&mut self, id: u64, result: &Result<(), String>) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
            e.finished_ms = Some(now_ms());
            match result {
                Ok(()) => e.status = OpStatus::Done,
                Err(msg) => {
                    e.status = OpStatus::Failed;
                    e.detail = Some(msg.clone());
                }
            }
            self.persist();
        }
    }

    /// Forgets an interrupted operation the user has dealt with.
    pub fn dismiss(&mut self, id: u64) {
        self.entries
            .retain(|e| !(e.id == id && e.status == OpStatus::Interrupted));
        self.persist();
    }

    fn persist(&self) {
        let refs: Vec<(String, &Operation)> =
            self.entries.iter().map(|e| (e.id.to_string(), e)).collect();
        let _ = crate::db::save_docs(&self.paths, "operations", &refs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finished_operations_are_recorded_with_their_result() {
        let home = crate::test_support::isolated_home();
        let mut j = Journal::load(&home.paths);
        let a = j.begin("apply_web", "Apply the web config", None, None);
        let b = j.begin(
            "restore_database",
            "Restore shop",
            Some("Restore the safety backup"),
            None,
        );
        j.finish(a, &Ok(()));
        j.finish(b, &Err("port busy".into()));

        let list = j.list();
        assert_eq!(list[0].id, b, "newest first");
        assert_eq!(list[0].status, OpStatus::Failed);
        assert_eq!(list[0].detail.as_deref(), Some("port busy"));
        assert_eq!(list[1].status, OpStatus::Done);
        assert!(j.interrupted().is_empty());
    }

    #[test]
    fn an_operation_left_running_is_found_interrupted_on_the_next_start() {
        let home = crate::test_support::isolated_home();
        let id = {
            let mut j = Journal::load(&home.paths);
            let done = j.begin("apply_web", "Apply the web config", None, None);
            j.finish(done, &Ok(()));
            j.begin(
                "apply_web",
                "Apply again",
                None,
                Some(CoreCommand::ApplyWeb { overwrite: vec![] }),
            )
            // The app "dies" here: no finish.
        };

        let reloaded = Journal::load(&home.paths);
        let stuck = reloaded.interrupted();
        assert_eq!(stuck.len(), 1);
        assert_eq!(stuck[0].id, id);
        assert!(stuck[0].retry.is_some());
        assert_eq!(
            reloaded
                .list()
                .iter()
                .filter(|e| e.status == OpStatus::Done)
                .count(),
            1
        );
    }

    #[test]
    fn starting_the_same_operation_again_clears_its_interrupted_entry() {
        let home = crate::test_support::isolated_home();
        {
            let mut j = Journal::load(&home.paths);
            j.begin("apply_web", "Apply the web config", None, None);
        }
        let mut j = Journal::load(&home.paths);
        assert_eq!(j.interrupted().len(), 1);
        let again = j.begin("apply_web", "Apply the web config", None, None);
        assert!(j.interrupted().is_empty());
        j.finish(again, &Ok(()));
        assert_eq!(j.list().len(), 1);
    }

    #[test]
    fn dismiss_only_removes_interrupted_entries() {
        let home = crate::test_support::isolated_home();
        let mut j = Journal::load(&home.paths);
        let done = j.begin("a", "A", None, None);
        j.finish(done, &Ok(()));
        j.dismiss(done);
        assert_eq!(
            j.list().len(),
            1,
            "a finished operation stays in the history"
        );
    }

    #[test]
    fn the_journal_is_capped_but_never_drops_a_running_operation() {
        let home = crate::test_support::isolated_home();
        let mut j = Journal::load(&home.paths);
        let running = j.begin("long", "Still going", None, None);
        for n in 0..(KEEP + 20) {
            let id = j.begin("k", &format!("op {n}"), None, None);
            j.finish(id, &Ok(()));
        }
        assert!(j.list().len() <= KEEP + 1);
        assert!(j.list().iter().any(|e| e.id == running));
    }
}
