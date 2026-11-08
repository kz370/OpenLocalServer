//! Dump viewer (§175): `dump()` / `dd()` output from Laravel and Symfony shows up in OLS.
//!
//! Symfony's VarDumper can send dumps to a *dump server* instead of the page when
//! `$_SERVER['VAR_DUMPER_FORMAT']` is `server`. OLS runs the project's own
//! `var-dump-server` as a supervised process and shows its output; it never decodes PHP
//! serialized data. A small prepend file maps the request's host to the right server port.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::app::Inner;
use crate::error::CoreError;
use crate::paths::AppPaths;
use crate::process::ProcessId;

/// Settings key of the global switch (default off).
pub const SETTING: &str = "php.dump_capture";
const PREPEND: &str = include_str!("../catalog/php/ols-prepend.php");
const FIRST_PORT: u16 = 9912;
const LAST_PORT: u16 = 9999;
/// The server script inside a project, as installed by `symfony/var-dumper`.
const SERVER_SCRIPT: &str = "vendor/symfony/var-dumper/Resources/bin/var-dump-server";
const BIN_SCRIPT: &str = "vendor/bin/var-dump-server";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DumpStatus {
    /// The global switch.
    pub enabled: bool,
    /// The user's own php.ini already sets `auto_prepend_file`, so OLS does not add its own.
    pub conflict: bool,
    /// The project has `symfony/var-dumper`.
    pub supported: bool,
    #[serde(default)]
    pub running: Option<ProcessId>,
    #[serde(default)]
    pub port: Option<u16>,
}

fn php_dir(paths: &AppPaths) -> PathBuf {
    paths.services_dir().join("php")
}

/// Writes `text` to `path` through a temporary file, so PHP never reads half a file.
fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

/// Writes the prepend file when its content differs and returns the php.ini line for it.
pub fn prepend_ini_line(paths: &AppPaths) -> Option<String> {
    let path = php_dir(paths).join("ols-prepend.php");
    let current = std::fs::read_to_string(&path).ok();
    if current.as_deref() != Some(PREPEND) {
        write_atomic(&path, PREPEND).ok()?;
    }
    Some(format!(
        "auto_prepend_file = \"{}\"\n",
        path.display().to_string().replace('\\', "/")
    ))
}

/// `true` when the user's `overrides.ini` or any `conf.d/*.ini` already sets
/// `auto_prepend_file` (comments are ignored).
pub fn ini_sets_prepend(ini_dir: &Path) -> bool {
    let mut files = vec![ini_dir.join("overrides.ini")];
    if let Ok(entries) = std::fs::read_dir(ini_dir.join("conf.d")) {
        files.extend(entries.flatten().map(|e| e.path()).filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("ini"))
        }));
    }
    files.iter().any(|f| {
        std::fs::read_to_string(f).is_ok_and(|text| {
            text.lines().any(|line| {
                let line = line.trim_start();
                !line.starts_with(';')
                    && !line.starts_with('#')
                    && line.to_ascii_lowercase().starts_with("auto_prepend_file")
            })
        })
    })
}

/// The PHP file mapping each host to its dump server. Only hostnames that pass
/// `validate_hostname` are written, so no quote can ever reach the PHP source.
pub fn render_map(entries: &[(String, u16)]) -> String {
    let mut map: BTreeMap<String, u16> = BTreeMap::new();
    for (host, port) in entries {
        let host = host.to_ascii_lowercase();
        if crate::domain::validate_hostname(&host).is_ok() {
            map.insert(host, *port);
        }
    }
    let body: Vec<String> = map
        .iter()
        .map(|(host, port)| format!("'{host}' => '127.0.0.1:{port}'"))
        .collect();
    format!("<?php return [{}];\n", body.join(", "))
}

fn write_map(paths: &AppPaths, entries: &[(String, u16)]) -> std::io::Result<()> {
    write_atomic(&php_dir(paths).join("ols-dumps.php"), &render_map(entries))
}

/// The first free port from 9912 up that no other dump server uses.
fn pick_port(taken: &[u16]) -> Option<u16> {
    (FIRST_PORT..=LAST_PORT).find(|p| !taken.contains(p) && crate::port::port_is_free(*p))
}

fn supported(root: &Path) -> bool {
    root.join(SERVER_SCRIPT).is_file()
}

impl Inner {
    /// Startup work, only when the switch is on: turn the prepend on and empty the map.
    pub fn init_dump_capture(&self) {
        if !self.setting_bool(SETTING, false) {
            return;
        }
        self.php.set_dump_prepend(true);
        let _ = prepend_ini_line(&self.paths);
        let _ = write_map(&self.paths, &[]);
    }

    /// The global switch. Restarts the running PHP pools once so they pick up the ini.
    pub fn set_dump_capture(&self, on: bool) -> Result<(), CoreError> {
        self.settings
            .lock()
            .unwrap()
            .set(SETTING, serde_json::json!(on))?;
        self.php.set_dump_prepend(on);
        if on {
            let _ = prepend_ini_line(&self.paths);
            self.rewrite_dump_map();
        } else {
            let running: Vec<String> = self.dump_servers.lock().unwrap().keys().cloned().collect();
            for id in running {
                let _ = self.stop_dump_server(&id);
            }
        }
        self.php.restart_running();
        Ok(())
    }

    fn dump_conflict(&self) -> bool {
        self.php
            .all_versions()
            .iter()
            .any(|v| ini_sets_prepend(&self.php.ini_dir(v)))
    }

    /// Hostnames (and aliases) of every site that belongs to `project_id`.
    fn project_hostnames(&self, project_id: &str) -> Vec<String> {
        self.domains
            .lock()
            .unwrap()
            .list()
            .into_iter()
            .filter(|d| d.project_id.as_deref() == Some(project_id))
            .flat_map(|d| std::iter::once(d.hostname.clone()).chain(d.aliases.clone()))
            .collect()
    }

    /// Rewrites the host -> server map from the running dump servers.
    pub(crate) fn rewrite_dump_map(&self) {
        let servers: Vec<(String, u16)> = self
            .dump_servers
            .lock()
            .unwrap()
            .iter()
            .map(|(p, (_, port))| (p.clone(), *port))
            .collect();
        let mut entries = Vec::new();
        for (project, port) in servers {
            for host in self.project_hostnames(&project) {
                entries.push((host, port));
            }
        }
        let _ = write_map(&self.paths, &entries);
    }

    /// Drops entries whose process has ended, then reports the project's state.
    pub fn dump_status(&self, project_id: &str) -> Result<DumpStatus, CoreError> {
        let project = self.ensure_project_available(project_id)?;
        let dead = {
            let mut servers = self.dump_servers.lock().unwrap();
            let before = servers.len();
            servers.retain(|_, (id, _)| self.supervisor.is_alive(*id));
            servers.len() != before
        };
        if dead {
            self.rewrite_dump_map();
        }
        let running = self.dump_servers.lock().unwrap().get(project_id).cloned();
        Ok(DumpStatus {
            enabled: self.setting_bool(SETTING, false),
            conflict: self.dump_conflict(),
            supported: supported(Path::new(&project.path)),
            running: running.map(|(id, _)| id),
            port: running.map(|(_, port)| port),
        })
    }

    pub fn start_dump_server(&self, project_id: &str) -> Result<ProcessId, CoreError> {
        let status = self.dump_status(project_id)?;
        let project = self.ensure_project_available(project_id)?;
        if let Some(id) = status.running {
            return Ok(id);
        }
        if !status.enabled {
            return Err(CoreError::failed_fix(
                "The dump viewer is off.",
                "It sends dump() output to OLS instead of the page, so it has to be turned on first.",
                "Turn on Dump viewer in Settings, then start it again.",
            ));
        }
        if status.conflict {
            return Err(CoreError::failed_fix(
                "OLS cannot add its dump hook to PHP.",
                "Your php.ini overrides already set auto_prepend_file, and OLS will not replace it.",
                "Remove auto_prepend_file from the PHP overrides (Runtimes > PHP), or leave the dump viewer off.",
            ));
        }
        if !status.supported {
            return Err(CoreError::failed_fix(
                "This project cannot send dumps to OLS yet.",
                "symfony/var-dumper is not installed in the project's vendor folder.",
                "Run `composer require --dev symfony/var-dumper` in the project (Laravel and Symfony already have it).",
            ));
        }
        let taken: Vec<u16> = self
            .dump_servers
            .lock()
            .unwrap()
            .values()
            .map(|(_, p)| *p)
            .collect();
        let port = pick_port(&taken).ok_or_else(|| {
            CoreError::failed_fix(
                "OLS found no free port for the dump viewer.",
                format!("Ports {FIRST_PORT} to {LAST_PORT} are all in use."),
                "Stop a dump viewer you no longer need, then try again.",
            )
        })?;
        let script = if Path::new(&project.path).join(BIN_SCRIPT).is_file() {
            BIN_SCRIPT
        } else {
            SERVER_SCRIPT
        };
        let args = vec![script.to_string(), format!("--host=127.0.0.1:{port}")];
        let id = self.spawn_project_process(
            "php",
            &args,
            None,
            Some(project_id),
            &format!("Dumps: {}", project.name),
            "php vendor/bin/var-dump-server",
        )?;
        self.dump_servers
            .lock()
            .unwrap()
            .insert(project_id.to_string(), (id, port));
        self.rewrite_dump_map();
        Ok(id)
    }

    pub fn stop_dump_server(&self, project_id: &str) -> Result<(), CoreError> {
        let entry = self.dump_servers.lock().unwrap().remove(project_id);
        if let Some((id, _)) = entry {
            self.supervisor.stop(id);
        }
        self.rewrite_dump_map();
        Ok(())
    }

    /// Environment that sends a CLI process's dumps (tinker, queue workers, tests) to the
    /// project's running dump server. Empty when none is running.
    pub(crate) fn dump_env(&self, project_id: &str) -> Vec<(String, String)> {
        match self.dump_servers.lock().unwrap().get(project_id) {
            Some((_, port)) => vec![
                ("VAR_DUMPER_FORMAT".to_string(), "server".to_string()),
                ("VAR_DUMPER_SERVER".to_string(), format!("127.0.0.1:{port}")),
            ],
            None => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_contains_only_valid_lowercase_hosts() {
        let text = render_map(&[
            ("Shop.test".to_string(), 9913),
            ("www.shop.test".to_string(), 9913),
            ("evil'.test".to_string(), 9913),
            ("a.test'] ; system('calc'); //".to_string(), 9913),
            ("nodots".to_string(), 9913),
        ]);
        assert_eq!(
            text,
            "<?php return ['shop.test' => '127.0.0.1:9913', 'www.shop.test' => '127.0.0.1:9913'];\n"
        );
        assert_eq!(render_map(&[]), "<?php return [];\n");
    }

    #[test]
    fn detects_a_user_prepend_but_ignores_comments() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!ini_sets_prepend(tmp.path()));
        std::fs::write(
            tmp.path().join("overrides.ini"),
            "; auto_prepend_file = x\nmemory_limit = 1G\n",
        )
        .unwrap();
        assert!(!ini_sets_prepend(tmp.path()));
        std::fs::create_dir_all(tmp.path().join("conf.d")).unwrap();
        std::fs::write(
            tmp.path().join("conf.d").join("a.ini"),
            "auto_prepend_file=\"C:/x.php\"\n",
        )
        .unwrap();
        assert!(ini_sets_prepend(tmp.path()));
    }

    #[test]
    fn prepend_ini_line_writes_the_file_once_with_forward_slashes() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::resolve_at(tmp.path());
        let line = prepend_ini_line(&paths).unwrap();
        assert!(line.starts_with("auto_prepend_file = \""));
        assert!(!line.contains('\\'));
        let file = php_dir(&paths).join("ols-prepend.php");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), PREPEND);
    }

    #[test]
    fn the_prepend_file_has_valid_php_syntax() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("ols-prepend.php");
        std::fs::write(&file, PREPEND).unwrap();
        let Ok(out) = std::process::Command::new("php")
            .arg("-l")
            .arg(&file)
            .output()
        else {
            eprintln!("php not found; skipping the prepend lint");
            return;
        };
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stdout)
        );
    }

    #[test]
    fn open_in_editor_at_refuses_files_outside_registered_projects() {
        let home = crate::test_support::isolated_home();
        let settings = crate::settings::SettingsService::load(&home.paths).unwrap();
        let inner = Inner::new(settings, home.paths.clone()).unwrap();
        let outside = tempfile::tempdir().unwrap();
        let file = outside.path().join("secret.php");
        std::fs::write(&file, "<?php").unwrap();
        for path in [
            file.display().to_string(),
            r"C:\Windows\win.ini".to_string(),
        ] {
            let err = inner.open_in_editor_at(&path, 3).unwrap_err();
            let text = format!("{err:?}");
            assert!(!text.is_empty(), "{path} must be refused");
        }
    }

    #[test]
    fn ports_start_at_9912_and_skip_taken_ones() {
        let p = pick_port(&[9912]).unwrap();
        assert!(p > 9912 && p <= LAST_PORT);
    }
}
