//! Xdebug settings (§13, Stage 11). The extension itself is downloaded and switched on like
//! any other PHP extension (`php.rs`); this module owns what goes into its `[xdebug]` ini
//! block and the IDE configuration that matches it.
//!
//! Settings are per PHP version, because one set of FastCGI workers serves every site on
//! that version. Debugging one project at a time works through `start_with_request=trigger`:
//! only requests carrying the `XDEBUG_TRIGGER` cookie or query parameter connect to the IDE.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Xdebug 3 modes; `off` is the only one that can't be combined with the others.
pub const MODES: &[&str] = &["off", "develop", "coverage", "debug", "gcstats", "profile", "trace"];
const START_MODES: &[&str] = &["yes", "trigger", "default", "no"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct XdebugSettings {
    pub modes: Vec<String>,
    /// "yes" (every request), "trigger" (only with XDEBUG_TRIGGER), "default", "no".
    pub start_with_request: String,
    pub client_host: String,
    pub client_port: u16,
    pub idekey: String,
}

impl Default for XdebugSettings {
    fn default() -> Self {
        Self {
            modes: vec!["debug".into()],
            start_with_request: "trigger".into(),
            client_host: "127.0.0.1".into(),
            client_port: 9003,
            idekey: "VSCODE".into(),
        }
    }
}

impl XdebugSettings {
    /// Checks the values before they reach php.ini, where a stray newline or quote would
    /// let a setting smuggle in other directives.
    pub fn validate(&self) -> Result<(), String> {
        if self.modes.is_empty() {
            return Err("Pick at least one Xdebug mode (or \"off\").".into());
        }
        for m in &self.modes {
            if !MODES.contains(&m.as_str()) {
                return Err(format!("\"{m}\" is not an Xdebug mode."));
            }
        }
        if self.modes.len() > 1 && self.modes.iter().any(|m| m == "off") {
            return Err("\"off\" can't be combined with other modes.".into());
        }
        if !START_MODES.contains(&self.start_with_request.as_str()) {
            return Err(format!("\"{}\" is not a valid start_with_request value.", self.start_with_request));
        }
        if self.client_port == 0 {
            return Err("The client port must be between 1 and 65535.".into());
        }
        let plain = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':'));
        if !plain(&self.client_host) {
            return Err("The client host can only contain letters, digits and . - _ :".into());
        }
        if !plain(&self.idekey) {
            return Err("The IDE key can only contain letters, digits and . - _ :".into());
        }
        Ok(())
    }

    /// The `[xdebug]` block for php.ini. `output_dir` receives profiler and trace files.
    pub fn ini_block(&self, output_dir: &Path) -> String {
        let mut modes = self.modes.clone();
        modes.sort_by_key(|m| MODES.iter().position(|x| x == m));
        modes.dedup();
        format!(
            "[xdebug]\nxdebug.mode={}\nxdebug.start_with_request={}\nxdebug.client_host={}\nxdebug.client_port={}\nxdebug.idekey={}\nxdebug.output_dir=\"{}\"\n",
            modes.join(","),
            self.start_with_request,
            self.client_host,
            self.client_port,
            self.idekey,
            output_dir.display().to_string().replace('\\', "/"),
        )
    }
}

/// What the UI shows for one PHP version.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XdebugReport {
    pub version: String,
    /// `php_xdebug.dll` is present (shipped or downloaded).
    pub installed: bool,
    /// php.ini loads it.
    pub enabled: bool,
    pub settings: XdebugSettings,
}

/// IDE setup text for one project. `ide` is "vscode", "phpstorm" or anything else for the
/// generic checklist.
pub fn ide_config(ide: &str, project_path: &str, site_root: &str, settings: &XdebugSettings) -> String {
    let port = settings.client_port;
    match ide {
        "vscode" => format!(
            r#"// .vscode/launch.json (needs the "PHP Debug" extension by xdebug.org)
{{
  "version": "0.2.0",
  "configurations": [
    {{
      "name": "Listen for Xdebug",
      "type": "php",
      "request": "launch",
      "port": {port},
      "pathMappings": {{
        "{}": "${{workspaceFolder}}"
      }}
    }}
  ]
}}
"#,
            project_path.replace('\\', "/")
        ),
        "phpstorm" => format!(
            "PhpStorm setup\n\
             1. Settings > PHP > Debug: set the Xdebug debug port to {port} and tick \"Can accept external connections\".\n\
             2. Settings > PHP > Servers: add a server for this site with host = its domain, port 80 or 443,\n\
                debugger = Xdebug, and tick \"Use path mappings\": map {} to the folder the site serves\n\
                ({}) on this computer (the same path).\n\
             3. Click \"Start Listening for PHP Debug Connections\" (the phone icon), set a breakpoint, and\n\
                open the site with ?XDEBUG_TRIGGER=1 (or use the Xdebug helper browser extension with IDE key {}).\n",
            project_path,
            site_root,
            settings.idekey
        ),
        _ => format!(
            "Xdebug connects back to your IDE, so:\n\
             - Listen on port {port} on this computer ({}).\n\
             - The IDE key is {}.\n\
             - Map the site folder ({site_root}) to the same path in the IDE; the project is at {project_path}.\n\
             - Start a debug session by opening the site with ?XDEBUG_TRIGGER=1{}.\n",
            settings.client_host,
            settings.idekey,
            if settings.start_with_request == "yes" { " (not needed while start_with_request is \"yes\")" } else { "" }
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid_and_render_in_a_stable_order() {
        let s = XdebugSettings { modes: vec!["trace".into(), "debug".into(), "develop".into()], ..Default::default() };
        assert!(s.validate().is_ok());
        let block = s.ini_block(Path::new(r"C:\data\xdebug"));
        assert!(block.contains("xdebug.mode=develop,debug,trace\n"), "{block}");
        assert!(block.contains("xdebug.client_port=9003\n"));
        assert!(block.contains("xdebug.output_dir=\"C:/data/xdebug\"\n"));
    }

    #[test]
    fn rejects_values_that_could_inject_ini_lines() {
        let mut s = XdebugSettings::default();
        s.client_host = "127.0.0.1\nzend_extension=evil.dll".into();
        assert!(s.validate().is_err());
        let mut s = XdebugSettings::default();
        s.idekey = "a\"b".into();
        assert!(s.validate().is_err());
        let mut s = XdebugSettings::default();
        s.modes = vec!["off".into(), "debug".into()];
        assert!(s.validate().is_err());
        s.modes = vec!["bogus".into()];
        assert!(s.validate().is_err());
        let mut s = XdebugSettings::default();
        s.client_port = 0;
        assert!(s.validate().is_err());
    }

    #[test]
    fn ide_snippets_carry_the_port_and_project_path() {
        let s = XdebugSettings { client_port: 9100, ..Default::default() };
        let code = ide_config("vscode", r"C:\Sites\shop", r"C:\Sites\shop\public", &s);
        assert!(code.contains("\"port\": 9100"));
        assert!(code.contains("\"C:/Sites/shop\": \"${workspaceFolder}\""));
        assert!(ide_config("phpstorm", r"C:\Sites\shop", r"C:\Sites\shop\public", &s).contains("9100"));
        assert!(ide_config("other", r"C:\Sites\shop", r"C:\Sites\shop\public", &s).contains("XDEBUG_TRIGGER"));
    }
}
