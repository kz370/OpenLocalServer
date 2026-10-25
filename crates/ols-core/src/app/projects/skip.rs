use crate::app::projects::*;
use crate::app::*;
use crate::error::CoreError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// One entry of the deletion history: a project folder or an automatic site the user
/// deleted, kept so scans and automatic domains leave it alone. Clearing an entry only
/// removes it from the history — it never touches the folder on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeletedItem {
    /// The skipped project path, or the skipped hostname.
    pub value: String,
    /// When the user deleted it, in milliseconds since the Unix epoch. Entries written
    /// before this field existed report `0` and sort last.
    pub deleted_at: u64,
    /// "project" or "domain" — which skip list the entry came from.
    pub kind: String,
}

impl DeletedItem {
    pub fn is_project(&self) -> bool {
        self.kind == "project"
    }
}

/// A skip-list entry as stored: the value plus when it was recorded. Plain strings are
/// still read (lists written by earlier versions), which is all they ever held.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct SkipEntry {
    pub(crate) value: String,
    pub(crate) deleted_at: u64,
}

impl SkipEntry {
    fn reads_as(entry: &serde_json::Value) -> Option<Self> {
        match entry {
            serde_json::Value::String(v) => Some(Self {
                value: v.clone(),
                deleted_at: 0,
            }),
            serde_json::Value::Object(_) => serde_json::from_value(entry.clone()).ok(),
            _ => None,
        }
    }
}

/// `ProjectStore::register` stores paths without the Windows `\\?\` prefix, but a caller can
/// hand us one that still has it and canonicalizing adds it back. Normalize both sides so
/// one folder is spelled one way on every comparison.
fn normalize_skip_path(path: &str) -> String {
    path.strip_prefix(r"\\?\").unwrap_or(path).to_string()
}

/// True when `value` can only be a filesystem path: a rooted path, a drive, or anything
/// with a separator. A bare automatic-domain name (`app1.local`) is not one, and resolving
/// a bare name against the working directory could match a folder that happens to share it.
fn is_path_like(value: &str) -> bool {
    Path::new(value).is_absolute() || value.contains(['\\', '/', ':'])
}

/// Resolves a path to a comparable form, falling back to canonicalizing its *parent* when
/// the leaf itself is gone.
///
/// The fallback is the case that matters for clearing: a folder deleted from disk before its
/// history entry is cleared cannot be canonicalized at all, so a whole-path resolve on both
/// sides fails and the two spellings are compared raw again — the clear silently does nothing.
/// The parent usually outlives the leaf, and a short name is expanded at every level, so
/// canonicalizing the parent and reattaching the file name still collapses the two spellings.
fn comparable_path(value: &str) -> Option<String> {
    if let Ok(c) = std::fs::canonicalize(value) {
        return Some(normalize_skip_path(&c.display().to_string()));
    }
    let path = Path::new(value);
    let parent = path.parent()?;
    let name = path.file_name()?;
    let parent = std::fs::canonicalize(parent).ok()?;
    Some(normalize_skip_path(
        &parent.join(name).display().to_string(),
    ))
}

/// One folder, whatever either side spells it: the canonicalized path `ProjectStore::register`
/// wrote, and the raw spelling a scan or a caller holds.
///
/// Same rule as `skip_matches`, applied the other way round. `skip_matches` compares an
/// entry against a scan's path; this compares a caller-supplied value against an entry, so
/// clearing an entry has to resolve both sides too. On Windows a short name
/// (`C:\Users\RUNNER~1\...`) is a different string from the long name `canonicalize`
/// returns, so comparing the strings alone left the entry in place: the settings page said
/// it had cleared it, the history still listed it, and the folder stayed skipped.
pub(crate) fn same_skip_entry(entry: &SkipEntry, value: &str) -> bool {
    let stored = normalize_skip_path(&entry.value);
    let asked = normalize_skip_path(value);
    if stored.eq_ignore_ascii_case(&asked) {
        return true;
    }
    if !is_path_like(&stored) || !is_path_like(&asked) {
        return false;
    }
    match (comparable_path(&stored), comparable_path(&asked)) {
        (Some(a), Some(b)) => a.eq_ignore_ascii_case(&b),
        _ => false,
    }
}

/// True when `path` is one of the skipped project folders, in either form.
///
/// Skips are stored the way `ProjectStore::register` writes paths — canonicalized — while a
/// scan only has the path `read_dir` handed it, which is raw. A folder reached through a
/// short name, a junction, a mapped drive or a `..` segment only matches when both are
/// compared; comparing the raw spelling alone let a scan re-add what the user deleted.
pub(crate) fn skip_matches(entries: &[SkipEntry], path: &str) -> bool {
    let raw = normalize_skip_path(path);
    let canonical = std::fs::canonicalize(Path::new(&raw))
        .map(|c| normalize_skip_path(&c.display().to_string()))
        .unwrap_or_else(|_| raw.clone());
    entries.iter().any(|e| {
        let stored = normalize_skip_path(&e.value);
        stored.eq_ignore_ascii_case(&raw) || stored.eq_ignore_ascii_case(&canonical)
    })
}

impl Inner {
    pub(crate) fn skip_entries(&self, key: &str) -> Vec<SkipEntry> {
        let s = self.settings.lock().unwrap();
        s.get(key)
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(SkipEntry::reads_as).collect())
            .unwrap_or_default()
    }

    fn edit_skip_entries(
        &self,
        key: &str,
        edit: impl FnOnce(&mut Vec<SkipEntry>),
    ) -> Result<(), CoreError> {
        let mut list = self.skip_entries(key);
        let before = list.clone();
        edit(&mut list);
        // Same value twice keeps the newest timestamp, so a re-delete moves the entry up
        // the history instead of duplicating it.
        let mut merged: Vec<SkipEntry> = Vec::with_capacity(list.len());
        for entry in list {
            match merged.iter_mut().find(|e| same_skip_entry(e, &entry.value)) {
                Some(existing) => existing.deleted_at = existing.deleted_at.max(entry.deleted_at),
                None => merged.push(entry),
            }
        }
        if merged != before {
            self.settings
                .lock()
                .unwrap()
                .set(key, serde_json::json!(merged))?;
        }
        Ok(())
    }

    /// Records `value` in `key`'s skip list unless it is already there.
    pub(crate) fn push_skip(&self, key: &str, value: &str) -> Result<(), CoreError> {
        let value = value.to_string();
        let stamp = now_ms();
        self.edit_skip_entries(key, move |list| {
            match list.iter_mut().find(|e| same_skip_entry(e, &value)) {
                Some(existing) => existing.deleted_at = stamp,
                None => list.push(SkipEntry {
                    value,
                    deleted_at: stamp,
                }),
            }
        })
    }

    pub(crate) fn remove_skip(&self, key: &str, value: &str) -> Result<(), CoreError> {
        let value = value.to_string();
        self.edit_skip_entries(key, move |list| {
            list.retain(|e| !same_skip_entry(e, &value))
        })
    }

    /// The whole deletion history, newest first: deleted project folders and deleted
    /// automatic sites, both of which stay hidden from scans until cleared.
    pub fn list_deleted_items(&self) -> Vec<DeletedItem> {
        let mut items: Vec<DeletedItem> = [PROJECT_SKIP, AUTO_SKIP]
            .into_iter()
            .flat_map(|key| {
                let kind = if key == PROJECT_SKIP {
                    "project"
                } else {
                    "domain"
                };
                self.skip_entries(key)
                    .into_iter()
                    .map(move |e| DeletedItem {
                        value: e.value,
                        deleted_at: e.deleted_at,
                        kind: kind.to_string(),
                    })
            })
            .collect();
        items.sort_by_key(|i| std::cmp::Reverse(i.deleted_at));
        items
    }

    /// Drops one entry from the history. The folder it names is not touched: the next scan
    /// or automatic-domain sync picks it up again if it still exists on disk.
    pub fn forget_deleted_item(&self, value: &str) -> Result<bool, CoreError> {
        let before = self.list_deleted_items().len();
        // Both lists can name the same value (a folder and the site it served), so clear
        // it from both and report whether anything went.
        for key in [PROJECT_SKIP, AUTO_SKIP] {
            self.remove_skip(key, value)?;
        }
        Ok(self.list_deleted_items().len() < before)
    }

    /// Empties the deletion history. Nothing on disk changes; the next scan or sync simply
    /// sees those folders again. Returns how many entries were dropped.
    pub fn clear_deleted_items(&self) -> Result<usize, CoreError> {
        let dropped = self.list_deleted_items().len();
        for key in [PROJECT_SKIP, AUTO_SKIP] {
            self.edit_skip_entries(key, |list| list.clear())?;
        }
        Ok(dropped)
    }
}
