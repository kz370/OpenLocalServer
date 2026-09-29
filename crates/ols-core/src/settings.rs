//! Settings service (§1 core skeleton). Backed by SQLite `settings` table in `app.db`.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::db;
use crate::error::CoreError;
use crate::paths::AppPaths;

#[derive(Debug, Clone)]
pub struct SettingsService {
    paths: AppPaths,
    values: BTreeMap<String, Value>,
}

impl SettingsService {
    pub fn load(paths: &AppPaths) -> Result<Self, CoreError> {
        let values = db::load_settings(paths)?;
        Ok(Self {
            paths: paths.clone(),
            values,
        })
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.values.get(key)
    }

    /// Every setting, for the support bundle.
    pub fn snapshot(&self) -> BTreeMap<String, Value> {
        self.values.clone()
    }

    pub fn set(&mut self, key: impl Into<String>, value: Value) -> Result<(), CoreError> {
        let key = key.into();
        db::save_setting(&self.paths, &key, &value)?;
        self.values.insert(key, value);
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
