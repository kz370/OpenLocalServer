//! ols-core: the application core (§8.1). GUI, CLI, and the future local API are all
//! front doors onto this crate — none of them talk to a manager directly.

pub mod ca;
pub mod catalog;
pub mod command;
pub mod custom_install;
pub mod dbtools;
pub mod detection;
pub mod hosts;
pub mod error;
pub mod logging;
pub mod manifest;
pub mod paths;
pub mod port;
pub mod process;
pub mod project;
pub mod resolver;
pub mod runtime;
pub mod secrets;
pub mod service;
pub mod settings;
#[cfg(test)]
mod test_support;

pub use command::{Core, CoreCommand, CoreResponse};
pub use error::{CoreError, Diagnostic};
pub use paths::AppPaths;
pub use process::ProcessSupervisor;
pub use project::ProjectStore;
pub use runtime::RuntimeManager;
pub use service::ServiceManager;
pub use settings::SettingsService;
