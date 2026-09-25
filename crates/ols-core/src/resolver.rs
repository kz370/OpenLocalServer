//! Environment Resolver — runtime piece (§18, §74 — Stage 4). Order: manifest (explicit,
//! §11 — never silently overridden) → detected requirement (a hint) → global default.

use serde::{Deserialize, Serialize};

use crate::runtime::RuntimeManager;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionSource {
    /// From `.devforge/environment.yaml` — an explicit choice, never overridden (§11).
    Manifest,
    /// Guessed from a marker file (composer.json, package.json, ...) — a hint, not a promise.
    Detected,
    Global,
    None,
    /// A user-pinned custom install location — wins over everything (§126 principle:
    /// an explicit choice is never silently overridden).
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedRuntime {
    pub id: String,
    pub requested_version: Option<String>,
    pub source: ResolutionSource,
    /// An installed version that satisfies the request (exact or prefix match), if any.
    pub installed_version: Option<String>,
    pub bin_dir: Option<String>,
}

pub fn resolve(
    id: &str,
    manifest_version: Option<&str>,
    detected_version: Option<&str>,
    global_version: Option<&str>,
    runtimes: &RuntimeManager,
) -> ResolvedRuntime {
    let (requested, source) = match (manifest_version, detected_version, global_version) {
        (Some(v), _, _) => (Some(v.to_string()), ResolutionSource::Manifest),
        (None, Some(v), _) => (Some(v.to_string()), ResolutionSource::Detected),
        (None, None, Some(v)) => (Some(v.to_string()), ResolutionSource::Global),
        (None, None, None) => (None, ResolutionSource::None),
    };

    let installed_version = requested.as_ref().and_then(|v| {
        let installed = runtimes.installed_versions(id);
        // Exact match first, then "requested 8.2 satisfied by installed 8.2.26".
        installed.iter().find(|iv| *iv == v).or_else(|| installed.iter().find(|iv| iv.starts_with(v.as_str()))).cloned()
    });
    let bin_dir = installed_version.as_ref().and_then(|v| runtimes.bin_dir(id, v)).map(|p| p.display().to_string());

    ResolvedRuntime { id: id.to_string(), requested_version: requested, source, installed_version, bin_dir }
}
