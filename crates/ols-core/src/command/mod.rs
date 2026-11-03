//! The `CoreCommand` dispatcher: routes front-door requests (Tauri IPC, CLI, API)
//! through a single enum and `dispatch` function.

use std::path::Path;
use std::sync::Arc;

use crate::app::Inner;
use crate::error::{CoreError, Diagnostic};
use crate::health::HealthProgress;
use crate::paths::AppPaths;
use crate::process::ProcessSupervisor;
use crate::runtime::RuntimeManager;
use crate::service::{ServiceAggregate, ServiceManager, ServiceStatus};
use crate::settings::SettingsService;

pub mod dispatch;
pub mod types;

#[cfg(test)]
mod tests;

pub use self::types::{CoreCommand, CoreResponse, CreateSiteResult, DashboardData};

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

/// Returns the status after installation, waiting briefly for the service named pipe to answer.
pub(crate) fn helper_service_status_wait() -> CoreResponse {
    let version = crate::elevate::service_version_retries(25);
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

    pub fn service_aggregate(&self) -> ServiceAggregate {
        self.service_aggregate_from(self.services().list())
    }

    pub fn service_aggregate_from(&self, services: Vec<ServiceStatus>) -> ServiceAggregate {
        let autostart_ids = self.inner.startup_settings().autostart_services;
        let default_web = self.inner.web_config().default_server;
        ServiceAggregate::from_services(services, &autostart_ids, &default_web)
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

    pub fn set_pending_add_project(&self, path: String) {
        self.inner.set_pending_add_project(path);
    }

    pub fn take_pending_add_project(&self) -> Option<String> {
        self.inner.take_pending_add_project()
    }

    /// Dispatches a `CoreCommand` and converts any error into a user diagnostic.
    pub fn dispatch(&self, command: CoreCommand) -> Result<CoreResponse, Diagnostic> {
        use crate::service::ServiceState;

        let services = self.services();
        let transitions = match &command {
            CoreCommand::StartService { id } => {
                if services.is_running_and_ready(id) {
                    Vec::new()
                } else {
                    let ids = vec![id.clone()];
                    services.begin_transitions(&ids, ServiceState::Starting);
                    ids
                }
            }
            CoreCommand::StopService { id } => {
                let ids = vec![id.clone()];
                services.begin_transitions(&ids, ServiceState::Stopping);
                ids
            }
            CoreCommand::RestartService { id } => {
                let ids = vec![id.clone()];
                services.begin_transitions(&ids, ServiceState::Restarting);
                ids
            }
            CoreCommand::StopAll => {
                let ids = services
                    .list()
                    .into_iter()
                    .filter(|service| {
                        matches!(
                            service.state,
                            ServiceState::Starting
                                | ServiceState::Stopping
                                | ServiceState::Restarting
                                | ServiceState::Running
                        )
                    })
                    .map(|service| service.id)
                    .collect::<Vec<_>>();
                services.begin_transitions(&ids, ServiceState::Stopping);
                ids
            }
            CoreCommand::StopWeb => {
                let ids = services
                    .list()
                    .into_iter()
                    .filter(|service| {
                        service.kind == "web"
                            && matches!(
                                service.state,
                                ServiceState::Starting
                                    | ServiceState::Stopping
                                    | ServiceState::Restarting
                                    | ServiceState::Running
                            )
                    })
                    .map(|service| service.id)
                    .collect::<Vec<_>>();
                services.begin_transitions(&ids, ServiceState::Stopping);
                ids
            }
            CoreCommand::ApplyWeb { .. } => {
                let default_server = self.inner().web_config().default_server;
                let ids = services
                    .list()
                    .into_iter()
                    .filter(|service| {
                        service.kind == "web" && (service.running || service.id == default_server)
                    })
                    .map(|service| service.id)
                    .collect::<Vec<_>>();
                services.begin_transitions(&ids, ServiceState::Restarting);
                ids
            }
            _ => Vec::new(),
        };
        let result = self
            .dispatch_inner(command)
            .map_err(|e| Diagnostic::from(&e));
        services.finish_transitions(&transitions);
        result
    }

    /// Runs a site health check while reporting each step's start and result.
    /// The regular `HealthCheck` dispatch remains available to non-UI callers.
    pub fn health_check_with_progress(
        &self,
        hostname: &str,
        progress: impl FnMut(HealthProgress),
    ) -> Result<CoreResponse, Diagnostic> {
        self.inner
            .health_check_with_progress(hostname, progress)
            .map(|report| CoreResponse::Health { report })
            .map_err(|error| Diagnostic::from(&error))
    }
}
