//! The support bundle (Stage 17, advanced diagnostics): one zip a person can attach to a bug report. It holds
//! the doctor's report, the current findings and environment health, the settings, the versions of what is
//! installed, and the tail of the main logs. Everything is passed through `redact_text` first and the home
//! folder's name is masked. It never holds secrets, `.env` files, certificate keys or project files, and it is
//! written where the user chooses; nothing is sent anywhere.

use std::io::Write;

use crate::app::Inner;
use crate::error::CoreError;
use crate::redact::redact_text;

const LOG_LINES: usize = 500;

pub(crate) fn mask_home(text: &str) -> String {
    let mut out = text.to_string();
    for var in ["USERPROFILE", "HOME"] {
        if let Ok(home) = std::env::var(var) {
            if home.len() > 3 {
                out = out
                    .replace(&home, "%USERPROFILE%")
                    .replace(&home.replace('\\', "/"), "%USERPROFILE%");
            }
        }
    }
    out
}

fn clean(text: &str) -> String {
    mask_home(&redact_text(text))
}

impl Inner {
    /// Writes the bundle to `dest` (a `.zip` path) and returns what it holds.
    pub fn export_support_bundle(&self, dest: &str) -> Result<Vec<String>, CoreError> {
        if !dest.to_ascii_lowercase().ends_with(".zip") {
            return Err(CoreError::failed(
                "The bundle wasn't written.",
                "Choose a file name ending in .zip.",
            ));
        }
        let mut entries: Vec<(String, String)> = Vec::new();

        let mut summary = format!(
            "OpenLocalServer {}\nOS: {} {}\nData folder: {}\n\nInstalled runtimes:\n",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH,
            self.paths.root().display()
        );
        for m in crate::catalog::builtin_catalog() {
            let installed = self.runtimes.installed_versions(m.id);
            if installed.contains(&m.version.to_string()) {
                summary.push_str(&format!("  {} {}\n", m.name, m.version));
            }
        }
        summary.push_str("\nPlugins:\n");
        for p in self.list_plugins() {
            summary.push_str(&format!(
                "  {} {} ({})\n",
                p.manifest.name,
                p.manifest.version,
                if p.enabled { "on" } else { "off" }
            ));
        }
        let net = self.network_status(true);
        summary.push_str(&format!(
            "\nInternet: {}\n",
            if net.online {
                "reachable"
            } else {
                "not reachable"
            }
        ));
        entries.push(("summary.txt".into(), clean(&summary)));

        entries.push((
            "doctor.txt".into(),
            clean(&crate::repair::doctor_text(&self.doctor())),
        ));
        entries.push((
            "findings.json".into(),
            clean(&serde_json::to_string_pretty(&self.diagnose())?),
        ));
        entries.push((
            "health.json".into(),
            clean(&serde_json::to_string_pretty(&self.environment_health())?),
        ));
        let settings: std::collections::BTreeMap<String, serde_json::Value> =
            self.settings.lock().unwrap().snapshot();
        let settings: std::collections::BTreeMap<String, String> = settings
            .into_iter()
            .map(|(k, v)| (k.clone(), crate::logging::redact_value(&k, &v.to_string())))
            .collect();
        entries.push((
            "settings.json".into(),
            clean(&serde_json::to_string_pretty(&settings)?),
        ));
        for source in self
            .log_sources()
            .into_iter()
            .filter(|s| s.kind == "app" || s.kind == "web")
        {
            if let Ok(lines) = self.read_log(&source.id, LOG_LINES) {
                if !lines.is_empty() {
                    entries.push((
                        format!("logs/{}.log", source.id.replace(':', "-")),
                        clean(&lines.join("\n")),
                    ));
                }
            }
        }

        let file = std::fs::File::create(dest)
            .map_err(|e| CoreError::failed("The bundle wasn't written.", format!("{dest}: {e}")))?;
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, body) in &entries {
            zip.start_file(name.as_str(), options)
                .map_err(|e| CoreError::failed("The bundle wasn't written.", e.to_string()))?;
            zip.write_all(body.as_bytes())?;
        }
        zip.finish()
            .map_err(|e| CoreError::failed("The bundle wasn't written.", e.to_string()))?;
        Ok(entries.into_iter().map(|(n, _)| n).collect())
    }
}
