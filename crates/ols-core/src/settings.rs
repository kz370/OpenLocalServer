//! Settings service (§1 core skeleton). Backed by a JSON file for now; will move onto the
//! SQLite `settings` table once the storage layer lands.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::Value;

use crate::error::CoreError;
use crate::paths::AppPaths;

#[derive(Debug)]
pub struct SettingsService {
    file: PathBuf,
    values: BTreeMap<String, Value>,
}

impl SettingsService {
    pub fn load(paths: &AppPaths) -> Result<Self, CoreError> {
        paths.ensure_dirs()?;
        let file = paths.settings_file();
        let values = if file.exists() {
            let raw = std::fs::read_to_string(&file)?;
            serde_json::from_str(&raw).unwrap_or_default()
        } else {
            BTreeMap::new()
        };
        Ok(Self { file, values })
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.values.get(key)
    }

    pub fn set(&mut self, key: impl Into<String>, value: Value) -> Result<(), CoreError> {
        self.values.insert(key.into(), value);
        self.persist()
    }

    fn persist(&self) -> Result<(), CoreError> {
        let raw = serde_json::to_string_pretty(&self.values)?;
        // Atomic-ish write: write to a temp file then rename (§163 reliability).
        let tmp = self.file.with_extension("json.tmp");
        std::fs::write(&tmp, raw)?;
        std::fs::rename(&tmp, &self.file)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_then_get_round_trips() {
        let home = crate::test_support::isolated_home();
        let mut svc = SettingsService::load(&home.paths).unwrap();
        svc.set("theme", Value::String("dark".into())).unwrap();
        assert_eq!(svc.get("theme"), Some(&Value::String("dark".into())));

        // Reload from disk to prove persistence.
        let svc2 = SettingsService::load(&home.paths).unwrap();
        assert_eq!(svc2.get("theme"), Some(&Value::String("dark".into())));
    }
}
