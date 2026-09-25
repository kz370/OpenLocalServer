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
        }
    }
}
