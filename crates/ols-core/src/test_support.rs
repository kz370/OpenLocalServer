//! Test-only helpers. `OLS_HOME` is process-wide state, so any test that sets it must hold
//! this lock for its duration or parallel `cargo test` threads race each other's env var.

#![cfg(test)]

use std::sync::{Mutex, MutexGuard, OnceLock};

use tempfile::TempDir;

use crate::paths::{AppPaths, HOME_ENV_VAR};

static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

/// Holds the env-var lock and a temp dir for its lifetime; `OLS_HOME` is unset and the
/// lock released on drop, so tests can just let this fall out of scope.
pub struct IsolatedHome {
    _guard: MutexGuard<'static, ()>,
    _tmp: TempDir,
    pub paths: AppPaths,
}

impl Drop for IsolatedHome {
    fn drop(&mut self) {
        std::env::remove_var(HOME_ENV_VAR);
    }
}

pub fn isolated_home() -> IsolatedHome {
    let lock = ENV_LOCK.get_or_init(|| Mutex::new(()));
    let guard = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var(HOME_ENV_VAR, tmp.path());
    let paths = AppPaths::resolve();
    IsolatedHome {
        _guard: guard,
        _tmp: tmp,
        paths,
    }
}
