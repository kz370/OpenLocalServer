use std::path::{Path, PathBuf};

use crate::app::*;
use crate::dbtools;
use crate::error::CoreError;

impl Inner {
    pub fn open_database(
        &self,
        engine: &str,
        database: Option<&str>,
        path: Option<&str>,
        tool_id: Option<&str>,
    ) -> Result<(), CoreError> {
        let info = self
            .services
            .connection_info(engine, database, path)
            .map_err(svc)?;

        let detected = dbtools::detect_db_tools();
        let custom_path = |id: &str| {
            self.custom_installs
                .lock()
                .unwrap()
                .resolve(id, None)
                .map(|c| c.path.clone())
        };
        let find = |id: &str| {
            custom_path(id).or_else(|| {
                detected
                    .iter()
                    .find(|t| t.id == id)
                    .and_then(|t| t.found_path.clone())
            })
        };

        if let Some(id) = tool_id {
            if id == "heidisql" {
                let exe = find(id).ok_or_else(|| {
                    svc("HeidiSQL was not found. Install it or locate it from Services.")
                })?;
                let args = dbtools::heidisql_args_for_executable(&info, &exe).ok_or_else(|| svc("HeidiSQL can't open that database or its PostgreSQL libpq library was not found"))?;
                return dbtools::launch(&exe, &args, true).map_err(svc);
            }
            if id == "pgadmin" {
                let exe = find(id).ok_or_else(|| {
                    svc("pgAdmin 4 was not found. Install it or locate it from Services.")
                })?;
                return dbtools::launch(&exe, &[], false).map_err(svc);
            }
            if id == "nosqlbooster" {
                if engine != "mongodb" {
                    return Err(svc("NoSQLBooster only supports MongoDB"));
                }
                let exe = find(id).ok_or_else(|| {
                    svc("NoSQLBooster was not found. Install it or locate it from Services.")
                })?;
                return dbtools::launch(&exe, &[], false).map_err(svc);
            }
            if id == "tinyrdm" {
                if engine != "redis" {
                    return Err(svc("Tiny RDM only supports Redis"));
                }
                let exe = find(id).ok_or_else(|| {
                    svc("Tiny RDM was not found. Install it or register it under External tools.")
                })?;
                return dbtools::launch(&exe, &[], false).map_err(svc);
            }
            if id == "dbbrowser" {
                if engine != "sqlite" {
                    return Err(svc("DB Browser for SQLite only supports SQLite"));
                }
                let exe = find(id).ok_or_else(|| {
                    svc("DB Browser for SQLite was not found. Install it or locate it from Services.")
                })?;
                let args = dbtools::dbbrowser_args(&info)
                    .ok_or_else(|| svc("DB Browser for SQLite needs a database file path"))?;
                return dbtools::launch(&exe, &args, false).map_err(svc);
            }
        }

        // 1) an explicitly chosen registered tool, 2) the first registered for the engine,
        // 3) HeidiSQL (mariadb/sqlite) or pgAdmin if detected.
        let registered = {
            let store = self.ext_tools.lock().unwrap();
            match tool_id {
                Some(id) => store.get(id).cloned(),
                None => store.for_engine(engine).cloned(),
            }
        };
        if let Some(tool) = registered {
            let args = dbtools::expand_args(&tool.args, &info);
            return dbtools::launch(&tool.executable, &args, false).map_err(svc);
        }
        match engine {
            "mariadb" => {
                let exe = find("heidisql").ok_or_else(|| svc("HeidiSQL was not found. Install it, locate it, or register another tool for this engine."))?;
                let args = dbtools::heidisql_args_for_executable(&info, &exe)
                    .ok_or_else(|| svc("HeidiSQL can't open that database"))?;
                dbtools::launch(&exe, &args, true).map_err(svc)
            }
            "sqlite" => {
                if let Some(exe) = find("dbbrowser") {
                    if let Some(args) = dbtools::dbbrowser_args(&info) {
                        return dbtools::launch(&exe, &args, false).map_err(svc);
                    }
                }
                if let Some(exe) = find("heidisql") {
                    if let Some(args) = dbtools::heidisql_args_for_executable(&info, &exe) {
                        return dbtools::launch(&exe, &args, true).map_err(svc);
                    }
                }
                Err(svc("Neither DB Browser for SQLite nor HeidiSQL was found. Install one, locate it from Services, or register another tool for this engine."))
            }
            "postgres" => {
                if let Some(exe) = find("heidisql") {
                    if let Some(args) = dbtools::heidisql_args_for_executable(&info, &exe) {
                        return dbtools::launch(&exe, &args, true).map_err(svc);
                    }
                }
                let exe = find("pgadmin").ok_or_else(|| svc("Neither HeidiSQL nor pgAdmin was found. Install one, locate it, or register another tool for this engine."))?;
                dbtools::launch(&exe, &[], false).map_err(svc)
            }
            "mongodb" => {
                let exe = find("nosqlbooster").ok_or_else(|| {
                    svc("NoSQLBooster was not found. Install it or locate it from Services.")
                })?;
                dbtools::launch(&exe, &[], false).map_err(svc)
            }
            "redis" => {
                let exe = find("tinyrdm").ok_or_else(|| {
                    svc("Tiny RDM was not found. Install it or register another tool for this engine.")
                })?;
                dbtools::launch(&exe, &[], false).map_err(svc)
            }
            other => Err(svc(format!(
                "No tool is registered for {other}. Add one under External tools."
            ))),
        }
    }

    /// The `sqlite3` binary: a pinned custom install, the managed one, or PATH.
    pub fn sqlite3_path(&self) -> Result<PathBuf, CoreError> {
        if let Some(c) = self.custom_installs.lock().unwrap().resolve("sqlite", None) {
            return Ok(PathBuf::from(&c.path));
        }
        if let Some(v) = self
            .runtimes
            .installed_versions("sqlite")
            .into_iter()
            .next()
        {
            if let Some(p) = self.runtimes.binary_path("sqlite", &v) {
                return Ok(p);
            }
        }
        crate::web::manager::find_executable(None, "sqlite3")
            .ok_or_else(|| svc("SQLite is not installed. Install it from the Runtimes page."))
    }

    // --------------------------------------------------------------- OS integration

    pub fn open_path(&self, path: &str) -> Result<(), CoreError> {
        let p = Path::new(path);
        if !p.exists() {
            return Err(svc(format!("{path} does not exist")));
        }
        let mut cmd = std::process::Command::new("explorer.exe");
        cmd.arg(p);
        cmd.spawn().map(|_| ()).map_err(|e| svc(e.to_string()))
    }

    pub fn open_url(&self, url: &str) -> Result<(), CoreError> {
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(svc("only http:// and https:// links can be opened"));
        }
        let mut cmd = std::process::Command::new("rundll32.exe");
        cmd.args(["url.dll,FileProtocolHandler", url]);
        crate::exec::hide_window(&mut cmd);
        cmd.spawn().map(|_| ()).map_err(|e| svc(e.to_string()))
    }

    /// §94–99: the user's editor (setting `editor.command`), else VS Code if present, else Notepad.
    pub fn open_in_editor(&self, path: &str) -> Result<(), CoreError> {
        let (chosen, custom) = {
            let s = self.settings.lock().unwrap();
            let text = |k: &str| s.get(k).and_then(|v| v.as_str()).map(str::to_string);
            (text("editor"), text("editor.command"))
        };
        let exe: PathBuf = match crate::editors::resolve(chosen.as_deref(), custom.as_deref()) {
            Some(exe) => exe,
            // No code editor at all: a folder opens in Explorer, a file in Notepad.
            None if Path::new(path).is_dir() => PathBuf::from("explorer.exe"),
            None => PathBuf::from("notepad.exe"),
        };
        self.launch_editor(&exe, path)
    }

    /// Starts `exe` on `path`, detached. Editor shims (`code.cmd`) go through `cmd.exe`.
    pub(crate) fn launch_editor(&self, exe: &Path, path: &str) -> Result<(), CoreError> {
        let mut cmd = if crate::editors::is_shim(exe) {
            let mut c = std::process::Command::new("cmd.exe");
            c.arg("/C")
                .arg(exe)
                .args(crate::editors::open_args(exe, path));
            c
        } else {
            let mut c = std::process::Command::new(exe);
            c.args(crate::editors::open_args(exe, path));
            c
        };
        crate::exec::hide_window(&mut cmd);
        cmd.spawn()
            .map(|_| ())
            .map_err(|e| svc(format!("could not start {}: {e}", exe.display())))
    }

    // ---------------------------------------------------------- startup (§121)
}
