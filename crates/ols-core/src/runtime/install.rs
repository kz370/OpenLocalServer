use std::sync::Arc;

use super::extract::*;
use super::manager::*;
use super::resolve::{owned_manifest, resolve_online_manifest};
use super::types::*;
use crate::catalog::builtin_catalog;

impl RuntimeManager {
    pub(crate) fn is_installed(&self, id: &str, version: &str) -> bool {
        let binary = builtin_catalog()
            .into_iter()
            .find(|m| m.id == id && m.version == version)
            .map(|m| m.binary)
            .or_else(|| runtime_binary(id));
        binary.is_some_and(|binary| self.version_dir(id, version).join(binary).exists())
    }

    /// Fetch current release versions from each supported vendor's public feed. The feeds
    /// contain metadata only; the package is still resolved and SHA-256 checked on install.
    pub fn install(&self, id: &str, version: &str) {
        let static_manifest = builtin_catalog()
            .into_iter()
            .find(|m| m.id == id && m.version == version)
            .map(owned_manifest);
        // online_versions lives only in daemon memory: a restart wipes it while the
        // UI's localStorage cache still lists those versions. Heal on demand with one
        // refresh before rejecting, otherwise every cached version fails with
        // "not in the current online or built-in version list".
        let mut refresh_note = String::new();
        if static_manifest.is_none()
            && !self
                .online_versions
                .read()
                .unwrap()
                .get(id)
                .is_some_and(|vs| vs.iter().any(|v| v == version))
        {
            match self.refresh_online_catalog(id) {
                Ok(()) => {}
                Err(e) => refresh_note = format!(" Online refresh failed: {e}"),
            }
        }
        if static_manifest.is_none()
            && !self
                .online_versions
                .read()
                .unwrap()
                .get(id)
                .is_some_and(|vs| vs.iter().any(|v| v == version))
        {
            let _ = self.events_tx.send(RuntimeEvent::Failed {
                id: id.to_string(),
                version: version.to_string(),
                message: format!(
                    "{version} is not in the current online or built-in version list.{refresh_note}"
                ),
            });
            return;
        }

        {
            let key = format!("{id}@{version}");
            let mut s = self.state.lock().unwrap();
            // A second install of the same version while one is running would race it
            // over the same cache file and scratch dir. The running one keeps the
            // controls; only the state entry is refreshed.
            if s.get(&key).is_some_and(|st| {
                matches!(
                    st,
                    InstallState::Downloading
                        | InstallState::Paused
                        | InstallState::Verifying
                        | InstallState::Extracting
                )
            }) {
                // Say it is already running, so the caller that just asked is told
                // something instead of hearing nothing at all.
                let _ = self.events_tx.send(RuntimeEvent::Progress {
                    id: id.to_string(),
                    version: version.to_string(),
                    state: s.get(&key).copied().unwrap_or(InstallState::Downloading),
                    downloaded: 0,
                    total: None,
                });
                return;
            }
            s.insert(key.clone(), InstallState::Downloading);
            self.controls
                .write()
                .unwrap()
                .insert(key, Arc::new(InstallControl::default()));
        }

        // Announce the start before anything slow happens. Resolving an online manifest
        // is a ranged GET and the first body chunk can be seconds away, so without this
        // the UI showed a button that had visibly done nothing.
        let _ = self.events_tx.send(RuntimeEvent::Progress {
            id: id.to_string(),
            version: version.to_string(),
            state: InstallState::Downloading,
            downloaded: 0,
            total: None,
        });

        let paths = self.paths.clone();
        let http = self.http.clone();
        let events_tx = self.events_tx.clone();
        let state = self.state.clone();
        let controls = self.controls.clone();
        let manifest_id = id.to_string();
        let manifest_version = version.to_string();
        let key = format!("{}@{}", manifest_id, manifest_version);
        let control = self
            .controls
            .read()
            .unwrap()
            .get(&key)
            .cloned()
            .unwrap_or_default();

        self.runtime.spawn(async move {
            let manifest = match static_manifest {
                Some(manifest) => Ok(manifest),
                None => resolve_online_manifest(&http, &manifest_id, &manifest_version).await,
            };
            let result = match manifest {
                Ok(manifest) => {
                    install_one(
                        manifest,
                        paths,
                        http,
                        events_tx.clone(),
                        state.clone(),
                        control.clone(),
                    )
                    .await
                }
                Err(e) => InstallOutcome::Failed(e),
            };
            // The control is dropped with the install, so a later "pause" can never
            // address one that is no longer running.
            controls.write().unwrap().remove(&key);
            match result {
                InstallOutcome::Done => {}
                InstallOutcome::Failed(e) => {
                    tracing::warn!(id = %manifest_id, version = %manifest_version, error = %e, "runtime install failed");
                    state.lock().unwrap().insert(key, InstallState::Failed);
                    let _ = events_tx.send(RuntimeEvent::Failed {
                        id: manifest_id,
                        version: manifest_version,
                        message: e,
                    });
                }
                InstallOutcome::Cancelled => {
                    tracing::info!(id = %manifest_id, version = %manifest_version, "runtime install stopped by the user");
                    state.lock().unwrap().insert(key, InstallState::Cancelled);
                    let _ = events_tx.send(RuntimeEvent::Cancelled {
                        id: manifest_id,
                        version: manifest_version,
                    });
                }
            }
        });
    }

    /// Holds an in-flight download where it is. The partial file stays, and resuming
    /// continues the same transfer — the file is never renamed into place while paused,
    /// so a pause can never produce a half-installed runtime.
    pub fn pause_install(&self, id: &str, version: &str, paused: bool) -> Result<(), String> {
        let key = format!("{id}@{version}");
        let control = self
            .controls
            .read()
            .unwrap()
            .get(&key)
            .cloned()
            .ok_or_else(|| format!("{id} {version} is not installing"))?;
        if paused {
            control.pause();
        } else {
            control.resume();
        }
        Ok(())
    }

    /// Aborts an in-flight install. The partial download is deleted, nothing is extracted
    /// or installed, and the user sees a stop, not an error.
    pub fn cancel_install(&self, id: &str, version: &str) -> Result<(), String> {
        let key = format!("{id}@{version}");
        let control = self
            .controls
            .read()
            .unwrap()
            .get(&key)
            .cloned()
            .ok_or_else(|| format!("{id} {version} is not installing"))?;
        control.cancel();
        Ok(())
    }

    /// Whether an install is running right now, and whether the user has it paused.
    pub fn install_control(&self, id: &str, version: &str) -> Option<(bool, bool)> {
        let key = format!("{id}@{version}");
        self.controls
            .read()
            .unwrap()
            .get(&key)
            .map(|c| (c.is_paused(), c.is_cancelled()))
    }
}
