use std::path::Path;

use crate::app::*;
use crate::error::CoreError;

impl Inner {
    pub(crate) fn project_skips(&self) -> Vec<SkipEntry> {
        self.skip_entries(PROJECT_SKIP)
    }

    /// Explicitly adding a folder again clears a previous removal.
    pub(crate) fn clear_project_skip(&self, path: &str) -> Result<(), CoreError> {
        self.remove_skip(PROJECT_SKIP, path)
    }

    /// §53 health chain for one site.
    pub fn startup_settings(&self) -> StartupSettings {
        // Read first: `web_config` takes the same settings lock, so it cannot be called
        // while the guard below is held.
        let default_web = self.web_config().default_server.clone();
        let s = self.settings.lock().unwrap();
        let b = |k: &str, d: bool| s.get(k).and_then(|v| v.as_bool()).unwrap_or(d);
        let mut services: Vec<String> = s
            .get("startup.services")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        // The old "start the web server with the app" switch is gone (§121). A user who
        // had it on meant "my sites answer after a restart", so that intent is carried
        // over once: the default web server joins the autostart list, which now applies
        // the config before starting it.
        if b("startup.web", false)
            && !services
                .iter()
                .any(|id| crate::web::SERVER_IDS.contains(&id.as_str()))
        {
            services.push(default_web);
        }
        StartupSettings {
            with_windows: registry_run_value().is_some(),
            start_minimized: b("startup.minimized", true),
            autostart_services: services,
            notifications: b("notifications.enabled", true),
            close_to_tray: b("startup.close_to_tray", true),
        }
    }

    pub fn set_startup_settings(&self, new: StartupSettings) -> Result<(), CoreError> {
        {
            let mut s = self.settings.lock().unwrap();
            s.set("startup.minimized", serde_json::json!(new.start_minimized))?;
            s.set(
                "startup.services",
                serde_json::json!(new.autostart_services),
            )?;
            s.set(
                "notifications.enabled",
                serde_json::json!(new.notifications),
            )?;
            s.set(
                "startup.close_to_tray",
                serde_json::json!(new.close_to_tray),
            )?;
        }
        set_start_with_windows(new.with_windows).map_err(svc)
    }

    /// Runs what the user asked to have started with the app (§121). Failures are logged,
    /// never fatal — a service that won't start shouldn't stop the app opening.
    pub fn run_autostart(&self) {
        if let Err(e) = self.sync_auto_domains() {
            tracing::warn!(error = %e, "automatic domains could not be synced");
        }
        // Servers orphaned by a killed previous run would hold every port we need.
        let orphans = crate::process::kill_orphans(&self.paths.runtimes_dir());
        if orphans > 0 {
            tracing::info!(
                orphans,
                "stopped servers left running by a previous session"
            );
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        let startup = self.startup_settings();
        // A web server in the autostart list is applied, not just spawned: rendering and
        // validating its config is what makes the user's sites answer. `ServiceManager::start`
        // on a web server id only launches the process against whatever config is on disk,
        // which is what the removed "start the web server" switch used to do separately.
        let mut web_applied = false;
        for id in &startup.autostart_services {
            let result: Result<(), String> = if crate::web::SERVER_IDS.contains(&id.as_str()) {
                self.apply_web_server(id, &[])
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            } else {
                self.services.start(id).map(|_| ())
            };
            if let Err(e) = result {
                tracing::warn!(service = %id, error = %e, "autostart: service did not start");
            }
            if crate::web::SERVER_IDS.contains(&id.as_str()) {
                web_applied = true;
            }
        }
        let tunnels: Vec<_> = self
            .list_tunnels()
            .into_iter()
            .filter(|t| t.config.autostart && t.config.acknowledged)
            .collect();
        if !tunnels.is_empty() && !web_applied {
            if let Err(e) = self.apply_web(&[]) {
                tracing::warn!(error = %e, "autostart: web server for public domains did not start");
            }
        }
        for t in tunnels {
            if let Err(e) = self.start_tunnel(&t.config.id, true) {
                tracing::warn!(tunnel = %t.config.name, error = %e, "autostart: named Cloudflare tunnel did not start");
            }
        }
    }
}

// ------------------------------------------------------------------ Windows startup

#[cfg(windows)]
fn registry_run_value() -> Option<String> {
    let mut cmd = std::process::Command::new("reg");
    cmd.args(["query", REG_RUN_KEY, "/v", REG_VALUE]);
    crate::exec::hide_window(&mut cmd);
    let out = cmd.output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).to_string())
}

/// The executable the startup entry names, or `None` when there is no entry.
///
/// `reg query` prints a table, not a bare value, so the path is the tail of the value
/// row: everything after the type column. Anything that does not look like a path
/// returns `None` rather than a guess — a startup entry this app did not write is not
/// ours to interpret, and a wrong guess here would rewrite a working entry.
#[cfg(windows)]
fn registry_run_target() -> Option<String> {
    parse_run_target(&registry_run_value()?)
}

/// Pulls the executable path out of a `reg query` value row.
///
/// Windows paths contain spaces, so the value is written quoted; but whether the quote
/// characters survive into the stored value depends on how `reg add` was given the
/// argument, so both spellings are accepted. Splitting on whitespace instead would
/// truncate `C:\Program Files\Open Local Server\…` to `C:\Program`.
pub fn parse_run_target(blob: &str) -> Option<String> {
    let line = blob.lines().find(|l| l.contains(REG_VALUE))?;
    let row = line.split_once(REG_VALUE)?.1.trim();
    if let Some(quoted) = row
        .split_once('"')
        .and_then(|(_, rest)| rest.split_once('"'))
    {
        return (!quoted.0.is_empty()).then(|| quoted.0.to_string());
    }
    // Unquoted: everything after the type column is the value, spaces included.
    let value = row.strip_prefix("REG_SZ")?.trim();
    (!value.is_empty()).then(|| value.to_string())
}

#[cfg(windows)]
fn set_start_with_windows(enabled: bool) -> Result<(), String> {
    let mut cmd = std::process::Command::new("reg");
    if enabled {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        cmd.args([
            "add",
            REG_RUN_KEY,
            "/v",
            REG_VALUE,
            "/t",
            "REG_SZ",
            // No `--minimized`: the startup entry and a manual launch now behave
            // the same way, because `startup.minimized` decides on its own. Passing
            // the flag here would hide the window even with the preference off.
            "/d",
            &format!("\"{}\"", exe.display()),
            "/f",
        ]);
    } else {
        if registry_run_value().is_none() {
            return Ok(());
        }
        cmd.args(["delete", REG_RUN_KEY, "/v", REG_VALUE, "/f"]);
    }
    crate::exec::hide_window(&mut cmd);
    let out = cmd.output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

#[cfg(not(windows))]
fn registry_run_value() -> Option<String> {
    None
}

/// The CLI flag that forces a hidden start regardless of the preference. Rare, but
/// a script or a hand-made shortcut still needs a way to say it.
pub const FORCE_MINIMIZED_ARG: &str = "--minimized";

/// Whether this launch starts in the tray.
///
/// The preference is the whole answer: it applies to a manual launch exactly as it
/// does to the startup entry, so the toggle means what it says. The flag only adds
/// to that — it forces a hidden start when the preference is off, and changes
/// nothing when it is on. Anything else opens the window.
pub fn should_start_hidden(start_minimized: bool, args: impl IntoIterator<Item = String>) -> bool {
    start_minimized || args.into_iter().any(|a| a == FORCE_MINIMIZED_ARG)
}

/// Rewrites the startup entry when it is stale. Two kinds of staleness, both silent
/// failures the user would otherwise only notice as "it never starts by itself".
///
/// 1. An entry written by an earlier build carries `--minimized`, which would hide the
///    window even with the preference off — the opposite of what the user then chose.
/// 2. An entry whose executable no longer exists, because the install folder was renamed
///    or moved. `set_start_with_windows` bakes the absolute exe path into the value, so
///    the entry survives a move as a valid-looking command pointing at nothing. The
///    rewrite points it at the copy that is actually running, which is the only one the
///    user can have meant.
///
/// A missing entry is left alone, so a build without autostart is not turned on behind
/// the user's back. Failures are logged, never fatal: a bad registry value must not stop
/// the app from starting.
pub fn migrate_startup_entry() {
    #[cfg(windows)]
    {
        let Some(current) = registry_run_value() else {
            return;
        };
        let dead = registry_run_target().filter(|target| !Path::new(target).exists());
        if !current.contains("--minimized") && dead.is_none() {
            return;
        }
        if let Some(target) = dead.as_deref() {
            tracing::info!(
                target,
                "the startup entry points at an executable that is gone; repointing it"
            );
        }
        match set_start_with_windows(true) {
            Ok(()) => tracing::info!("rewrote the startup entry to the running executable"),
            Err(e) => tracing::warn!(error = %e, "could not rewrite the startup entry"),
        }
    }
}

#[cfg(not(windows))]
fn set_start_with_windows(_enabled: bool) -> Result<(), String> {
    Err("start with the system is only implemented on Windows so far".into())
}
