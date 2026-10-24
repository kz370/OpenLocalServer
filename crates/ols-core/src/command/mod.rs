//! The `CoreCommand` dispatcher: routes front-door requests (Tauri IPC, CLI, API)
//! through a single enum and `dispatch` function.

use std::path::Path;
use std::sync::Arc;

use crate::app::Inner;
use crate::error::{CoreError, Diagnostic};
use crate::paths::AppPaths;
use crate::process::ProcessSupervisor;
use crate::runtime::RuntimeManager;
use crate::service::ServiceManager;
use crate::settings::SettingsService;

pub mod dispatch;
pub mod types;

#[cfg(test)]
mod tests;

pub use self::types::{CoreCommand, CoreResponse, DashboardData};

/// Reports a background task's completion status.
pub(crate) fn report<T>(h: &crate::tasks::TaskHandle, r: Result<T, CoreError>) {
    match r {
        Ok(_) => h.succeed(),
        Err(e) => {
            if h.cancel_requested() {
                h.end_step(Some("stopped".into()));
                h.finish_cancelled();
                return;
            }
            let d = Diagnostic::from(&e);
            h.problem(d.cause.clone());
            h.fail(d);
        }
    }
}

/// Returns the file name component of a path as a String.
pub(crate) fn file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

/// Returns the admin helper service's installation and update status.
pub(crate) fn helper_service_status() -> CoreResponse {
    let version = crate::elevate::service_version();
    let status = version
        .as_deref()
        .map(|v| crate::updater::update_status(v, env!("CARGO_PKG_VERSION")));
    CoreResponse::HelperService {
        installed: version.is_some(),
        status,
        version,
    }
}

/// Shared application core handle. Cloning shares the underlying state.
#[derive(Clone)]
pub struct Core {
    pub(crate) inner: Arc<Inner>,
}

impl Core {
    pub fn new(settings: SettingsService, paths: AppPaths) -> Self {
        let inner = Inner::new(settings, paths).expect("failed to start the application core");
        inner.apply_api();
        Self { inner }
    }

    /// Creates a Core sharing process supervisor and runtime manager parts.
    pub fn with_parts(
        settings: SettingsService,
        paths: AppPaths,
        supervisor: Arc<ProcessSupervisor>,
        runtimes: Arc<RuntimeManager>,
    ) -> Self {
        let inner = Inner::with_parts(settings, paths, supervisor, runtimes)
            .expect("failed to start the application core");
        inner.apply_api();
        Self { inner }
    }

    /// Wraps an existing shared Inner state handle.
    pub fn from_inner(inner: Arc<Inner>) -> Self {
        Self { inner }
    }

    pub fn inner(&self) -> &Arc<Inner> {
        &self.inner
    }
    pub fn supervisor(&self) -> Arc<ProcessSupervisor> {
        self.inner.supervisor.clone()
    }
    pub fn runtimes(&self) -> Arc<RuntimeManager> {
        self.inner.runtimes.clone()
    }
    pub fn services(&self) -> Arc<ServiceManager> {
        self.inner.services.clone()
    }

    /// Dispatches a `CoreCommand` and converts any error into a user diagnostic.
    pub fn dispatch(&self, command: CoreCommand) -> Result<CoreResponse, Diagnostic> {
        self.dispatch_inner(command)
            .map_err(|e| Diagnostic::from(&e))
    }
}
