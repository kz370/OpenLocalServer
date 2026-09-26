//! Core error type. Wraps every failure into a user-facing `Diagnostic { problem, cause, fix }`
//! shape (§112 / §115) instead of leaking raw error strings to the UI.

use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("could not read or write settings: {0}")]
    Json(#[from] serde_json::Error),

    #[error("unknown setting key: {0}")]
    UnknownKey(String),

    #[error("not a directory: {0}")]
    InvalidProjectPath(String),

    #[error("{0}")]
    ServiceError(String),

    #[error("{0}")]
    DomainError(String),

    #[error("{0}")]
    WebError(String),

    #[error("{0}")]
    QuickAppError(String),

    /// Manifests, setup, profiles, snapshots (§69–78, §130–132).
    #[error("{0}")]
    EnvError(String),

    #[error("{0}")]
    GitError(String),

    #[error("{0}")]
    TunnelError(String),

    /// Plugins, catalogs, updates, load tests and the AI assistant: says what was being done.
    #[error("{problem}: {cause}")]
    Failed { problem: String, cause: String, fix: Option<String> },
}

impl CoreError {
    pub fn failed(problem: impl Into<String>, cause: impl Into<String>) -> Self {
        CoreError::Failed { problem: problem.into(), cause: cause.into(), fix: None }
    }
    pub fn failed_fix(problem: impl Into<String>, cause: impl Into<String>, fix: impl Into<String>) -> Self {
        CoreError::Failed { problem: problem.into(), cause: cause.into(), fix: Some(fix.into()) }
    }
}

/// The shape every error crosses the IPC boundary as, so the UI can render
/// Problem / Cause / Fix instead of a raw Rust error string (§112, §115).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    pub problem: String,
    pub cause: String,
    pub fix: Option<String>,
}

impl From<&CoreError> for Diagnostic {
    fn from(err: &CoreError) -> Self {
        match err {
            CoreError::Io(e) => Diagnostic {
                problem: "A file operation failed.".into(),
                cause: e.to_string(),
                fix: Some("Check that OpenLocalServer has permission to write to its data directory.".into()),
            },
            CoreError::Json(e) => Diagnostic {
                problem: "Settings could not be read or written.".into(),
                cause: e.to_string(),
                fix: Some("The settings file may be corrupted. Consider restoring a backup.".into()),
            },
            CoreError::UnknownKey(k) => Diagnostic {
                problem: format!("Setting \"{k}\" does not exist."),
                cause: "The requested key was never set.".into(),
                fix: None,
            },
            CoreError::InvalidProjectPath(p) => Diagnostic {
                problem: "That folder can't be added as a project.".into(),
                cause: format!("\"{p}\" is not a directory on disk."),
                fix: Some("Check the path and try again.".into()),
            },
            CoreError::ServiceError(msg) => Diagnostic {
                problem: "That service operation failed.".into(),
                cause: msg.clone(),
                fix: None,
            },
            CoreError::DomainError(msg) => Diagnostic {
                problem: "That domain change was rejected.".into(),
                cause: msg.clone(),
                fix: Some("Pick a different name, or edit the existing domain instead.".into()),
            },
            CoreError::WebError(msg) => Diagnostic {
                problem: "The web server operation failed.".into(),
                cause: msg.clone(),
                fix: Some("Check the web-server log on the Logs page, fix the config, and apply again.".into()),
            },
            CoreError::QuickAppError(msg) => Diagnostic {
                problem: "That Quick App or Quick Command couldn't be used.".into(),
                cause: msg.clone(),
                fix: None,
            },
            CoreError::EnvError(msg) => Diagnostic {
                problem: "The project environment couldn't be set up.".into(),
                cause: msg.clone(),
                fix: Some("Check the project's .openlocalserver files, or review the plan's conflicts.".into()),
            },
            CoreError::GitError(msg) => Diagnostic { problem: "Git reported a problem.".into(), cause: msg.clone(), fix: None },
            CoreError::TunnelError(msg) => Diagnostic {
                problem: "The tunnel operation failed.".into(),
                cause: msg.clone(),
                fix: Some("Check the tunnel's log on the Tunnels page.".into()),
            },
            CoreError::Failed { problem, cause, fix } => Diagnostic { problem: problem.clone(), cause: cause.clone(), fix: fix.clone() },
        }
    }
}
