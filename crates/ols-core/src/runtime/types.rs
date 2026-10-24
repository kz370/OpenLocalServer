use std::collections::HashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    /// An OLS-managed copy is installed under `runtimes/<id>/<version>/`.
    pub installed: bool,
    /// The version newly-created projects and services will use by default.
    pub is_default: bool,
    /// An unmanaged install found on PATH — same runtime family, not necessarily the
    /// same version, and not usable for per-project version selection (§126).
    pub system: Option<SystemInstall>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInstall {
    pub path: String,
    pub version: String,
}


#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallState {
    Downloading,
    /// The user paused it. The download is held where it is; resuming continues.
    Paused,
    Verifying,
    Extracting,
    Installed,
    /// The user stopped it. The partial download is deleted and nothing is installed.
    Cancelled,
    Failed,
}

/// Why an install stopped. A user stop is not a failure: nothing went wrong, so it must
/// not be reported as one (and must not set `InstallState::Failed`).
#[derive(Debug)]
pub enum InstallOutcome {
    Done,
    Failed(String),
    Cancelled,
}

/// The in-flight install controls a UI can drive: pause holds the transfer where it is,
/// cancel aborts it. One per `id@version`; dropped when the install finishes.
#[derive(Default)]
pub(crate) struct InstallControl {
    paused: std::sync::atomic::AtomicBool,
    cancelled: std::sync::atomic::AtomicBool,
}

impl InstallControl {
    pub(crate) fn pause(&self) {
        self.paused
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    pub(crate) fn resume(&self) {
        self.paused
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }

    pub(crate) fn is_paused(&self) -> bool {
        self.paused.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub(crate) fn cancel(&self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancelled.load(std::sync::atomic::Ordering::Relaxed)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuntimeEvent {
    Progress {
        id: String,
        version: String,
        state: InstallState,
        downloaded: u64,
        total: Option<u64>,
    },
    Installed {
        id: String,
        version: String,
        path: String,
    },
    Failed {
        id: String,
        version: String,
        message: String,
    },
    /// The user stopped this install. Not a failure: nothing is left behind and no
    /// diagnostic is warranted, so this is its own terminal event.
    Cancelled { id: String, version: String },
}

