//! Quick Commands (§89, §153) and command history (§93): reusable developer commands that
//! don't create a project — "Run migrations", "Clear cache", "Build frontend", ... — plus
//! the list of what was recently run, with Run Again / Edit / Save as Quick Command / Delete.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::schema::{valid_id, Scalar};
use crate::error::CoreError;
use crate::paths::AppPaths;

const BUILTIN_YAML: &str = include_str!("../../catalog/quick-commands.yaml");

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandSpec {
    pub executable: String,
    #[serde(default)]
    pub arguments: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandEnvironment {
    /// Run with the project's resolved PHP/Node/Python first on PATH (§18, §153).
    #[serde(default = "yes")]
    pub use_project_runtime: bool,
}

fn yes() -> bool {
    true
}

impl Default for CommandEnvironment {
    fn default() -> Self {
        Self {
            use_project_runtime: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuickCommand {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub category: String,
    /// Frameworks this makes sense for ("laravel", "node", ...). Empty = any project.
    #[serde(default)]
    pub applies_to: Vec<String>,
    #[serde(default)]
    pub working_directory: Option<String>,
    #[serde(default)]
    pub command: Option<CommandSpec>,
    #[serde(default)]
    pub environment: CommandEnvironment,
    /// Instead of a command: a built-in action (`open_url`, `open_web_config`, `restart_project`).
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub with: BTreeMap<String, Scalar>,
    /// Filled in by the store: built-ins can't be deleted.
    #[serde(default, skip_deserializing)]
    pub builtin: bool,
}

impl QuickCommand {
    pub fn validate(&self) -> Result<(), String> {
        if !valid_id(&self.id) {
            return Err(format!(
                "id \"{}\" must be lowercase letters, digits and dashes",
                self.id
            ));
        }
        if self.name.trim().is_empty() {
            return Err("name is required".into());
        }
        match (&self.command, &self.action) {
            (Some(c), None) if !c.executable.trim().is_empty() => Ok(()),
            (None, Some(_)) => Ok(()),
            _ => Err(
                "a Quick Command needs exactly one of `command` (with an executable) or `action`"
                    .into(),
            ),
        }
    }

    pub fn applies_to_framework(&self, framework: &str) -> bool {
        self.applies_to.is_empty() || self.applies_to.iter().any(|f| f == framework)
    }
}

pub struct QuickCommandStore {
    dir: PathBuf,
}

impl QuickCommandStore {
    pub fn load(paths: &AppPaths) -> Result<Self, CoreError> {
        let dir = paths.data_dir().join("quick-commands");
        std::fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    fn builtin() -> Vec<QuickCommand> {
        let mut list: Vec<QuickCommand> =
            serde_yaml_ng::from_str(BUILTIN_YAML).expect("built-in quick-commands.yaml is valid");
        for c in &mut list {
            c.builtin = true;
        }
        list
    }

    pub fn list(&self) -> Vec<QuickCommand> {
        let mut by_id: BTreeMap<String, QuickCommand> = Self::builtin()
            .into_iter()
            .map(|c| (c.id.clone(), c))
            .collect();
        if let Ok(entries) = std::fs::read_dir(&self.dir) {
            for e in entries.flatten() {
                let path = e.path();
                if path.extension().and_then(|x| x.to_str()) != Some("yaml") {
                    continue;
                }
                let Ok(raw) = std::fs::read_to_string(&path) else {
                    continue;
                };
                if let Ok(cmd) = serde_yaml_ng::from_str::<QuickCommand>(&raw) {
                    if cmd.validate().is_ok() {
                        by_id.insert(cmd.id.clone(), cmd);
                    }
                }
            }
        }
        by_id.into_values().collect()
    }

    pub fn get(&self, id: &str) -> Option<QuickCommand> {
        self.list().into_iter().find(|c| c.id == id)
    }

    pub fn save(&self, cmd: QuickCommand) -> Result<QuickCommand, CoreError> {
        cmd.validate().map_err(CoreError::QuickAppError)?;
        let raw =
            serde_yaml_ng::to_string(&cmd).map_err(|e| CoreError::QuickAppError(e.to_string()))?;
        std::fs::write(self.dir.join(format!("{}.yaml", cmd.id)), raw)?;
        Ok(self.get(&cmd.id).unwrap_or(cmd))
    }

    pub fn delete(&self, id: &str) -> Result<(), CoreError> {
        let path = self.dir.join(format!("{id}.yaml"));
        if !path.is_file() {
            return Err(CoreError::QuickAppError(
                "only your own Quick Commands can be deleted".into(),
            ));
        }
        std::fs::remove_file(path)?;
        Ok(())
    }
}

// ------------------------------------------------------------------------------- history

const HISTORY_CAP: usize = 200;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: u64,
    /// The command line exactly as the user (or a Quick Command) ran it.
    pub line: String,
    pub cwd: Option<String>,
    pub project_id: Option<String>,
    pub timestamp_ms: u64,
}

pub struct CommandHistory {
    file: PathBuf,
    entries: Vec<HistoryEntry>,
}

impl CommandHistory {
    pub fn load(paths: &AppPaths) -> Result<Self, CoreError> {
        paths.ensure_dirs()?;
        let file = paths.data_dir().join("command_history.json");
        let entries = std::fs::read_to_string(&file)
            .ok()
            .and_then(|r| serde_json::from_str(&r).ok())
            .unwrap_or_default();
        Ok(Self { file, entries })
    }

    /// Newest first.
    pub fn list(&self) -> Vec<HistoryEntry> {
        self.entries.iter().rev().cloned().collect()
    }

    pub fn get(&self, id: u64) -> Option<HistoryEntry> {
        self.entries.iter().find(|e| e.id == id).cloned()
    }

    /// Records a run. Running the identical command again in the same place moves it to the
    /// top instead of piling up duplicates.
    pub fn record(
        &mut self,
        line: &str,
        cwd: Option<&str>,
        project_id: Option<&str>,
    ) -> Result<HistoryEntry, CoreError> {
        self.entries.retain(|e| {
            !(e.line == line && e.cwd.as_deref() == cwd && e.project_id.as_deref() == project_id)
        });
        let mut id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        while self.entries.iter().any(|e| e.id == id) {
            id += 1;
        }
        let entry = HistoryEntry {
            id,
            line: line.to_string(),
            cwd: cwd.map(str::to_string),
            project_id: project_id.map(str::to_string),
            timestamp_ms: id,
        };
        self.entries.push(entry.clone());
        if self.entries.len() > HISTORY_CAP {
            let excess = self.entries.len() - HISTORY_CAP;
            self.entries.drain(..excess);
        }
        self.persist()?;
        Ok(entry)
    }

    pub fn delete(&mut self, id: u64) -> Result<(), CoreError> {
        self.entries.retain(|e| e.id != id);
        self.persist()
    }

    pub fn clear(&mut self) -> Result<(), CoreError> {
        self.entries.clear();
        self.persist()
    }

    fn persist(&self) -> Result<(), CoreError> {
        let raw = serde_json::to_string_pretty(&self.entries)?;
        let tmp = self.file.with_extension("json.tmp");
        std::fs::write(&tmp, raw)?;
        std::fs::rename(&tmp, &self.file)?;
        Ok(())
    }
}

/// §93 "Save as Quick Command": a history line becomes a reusable command.
pub fn quick_command_from_line(
    id: &str,
    name: &str,
    line: &str,
    cwd: Option<&str>,
) -> Result<QuickCommand, String> {
    let tokens = super::plan::split_command_line(line);
    let (exe, args) = tokens.split_first().ok_or("that command line is empty")?;
    let cmd = QuickCommand {
        id: id.to_string(),
        name: name.to_string(),
        description: String::new(),
        category: "custom".into(),
        applies_to: vec![],
        working_directory: cwd.map(str::to_string),
        command: Some(CommandSpec {
            executable: exe.clone(),
            arguments: args.to_vec(),
        }),
        environment: CommandEnvironment::default(),
        action: None,
        with: BTreeMap::new(),
        builtin: false,
    };
    cmd.validate()?;
    Ok(cmd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_commands_are_valid_and_include_the_srs_examples() {
        let home = crate::test_support::isolated_home();
        let store = QuickCommandStore::load(&home.paths).unwrap();
        let list = store.list();
        assert!(list.iter().all(|c| c.builtin && c.validate().is_ok()));
        let names: Vec<&str> = list.iter().map(|c| c.name.as_str()).collect();
        for want in [
            "Run Laravel Migrations",
            "Clear Laravel Cache",
            "Install Dependencies (Composer)",
            "Run Tests",
            "Build Frontend",
            "Open Mailpit",
            "Restart Project",
        ] {
            assert!(names.contains(&want), "missing {want}");
        }
        let migrate = store.get("laravel-migrate").unwrap();
        assert_eq!(
            migrate.command.as_ref().unwrap().arguments,
            ["artisan", "migrate"]
        );
        assert!(migrate.applies_to_framework("laravel") && !migrate.applies_to_framework("node"));
    }

    #[test]
    fn user_commands_save_list_and_delete_but_builtins_cannot_be_deleted() {
        let home = crate::test_support::isolated_home();
        let store = QuickCommandStore::load(&home.paths).unwrap();
        let cmd = quick_command_from_line("say-hi", "Say hi", "echo \"hello world\"", Some("C:/x"))
            .unwrap();
        store.save(cmd).unwrap();
        let got = store.get("say-hi").unwrap();
        assert_eq!(got.command.unwrap().arguments, ["hello world"]);
        assert!(!got.builtin);

        store.delete("say-hi").unwrap();
        assert!(store.get("say-hi").is_none());
        assert!(store.delete("laravel-migrate").is_err());
    }

    #[test]
    fn invalid_commands_are_rejected() {
        assert!(quick_command_from_line("Bad Id", "x", "echo", None).is_err());
        assert!(quick_command_from_line("ok", "x", "   ", None).is_err());
    }

    #[test]
    fn history_dedupes_moves_to_top_caps_and_deletes() {
        let home = crate::test_support::isolated_home();
        let mut h = CommandHistory::load(&home.paths).unwrap();
        h.record("php artisan migrate", Some("C:/a"), Some("p1"))
            .unwrap();
        h.record("npm install", Some("C:/a"), Some("p1")).unwrap();
        h.record("php artisan migrate", Some("C:/a"), Some("p1"))
            .unwrap();

        let list = h.list();
        assert_eq!(list.len(), 2, "same command in the same place is one entry");
        assert_eq!(list[0].line, "php artisan migrate", "newest first");

        // Different folder = different entry.
        h.record("php artisan migrate", Some("C:/b"), Some("p2"))
            .unwrap();
        assert_eq!(h.list().len(), 3);

        let id = h.list()[1].id;
        h.delete(id).unwrap();
        assert_eq!(h.list().len(), 2);
        assert!(h.get(id).is_none());

        // Persisted.
        assert_eq!(CommandHistory::load(&home.paths).unwrap().list().len(), 2);

        for i in 0..250 {
            h.record(&format!("cmd {i}"), None, None).unwrap();
        }
        assert_eq!(h.list().len(), HISTORY_CAP);
        assert_eq!(h.list()[0].line, "cmd 249");
        h.clear().unwrap();
        assert!(h.list().is_empty());
    }
}
