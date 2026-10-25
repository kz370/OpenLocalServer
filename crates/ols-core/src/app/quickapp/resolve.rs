use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::app::quickapp::*;
use crate::app::*;
use crate::domain::{Domain, Ownership};
use crate::error::CoreError;
use crate::quickapp::commands::QuickCommand;
use crate::quickapp::plan::RunPlan;
use crate::quickapp::run::{ActionOutcome, ResolvedProgram, RunCtx};
use crate::quickapp::QuickHost;

impl Inner {
    pub fn requirement_views(&self, plan: &RunPlan) -> Vec<RequirementView> {
        let catalog = crate::catalog::builtin_catalog();
        plan.requirements
            .iter()
            .map(|r| {
                let label_of = |id: &str| {
                    catalog
                        .iter()
                        .find(|m| m.id == id)
                        .map(|m| m.name.to_string())
                        .unwrap_or_else(|| id.to_string())
                };
                let label = match r.id.as_str() {
                    "python" => "Python".to_string(),
                    id => label_of(id),
                };
                if r.id == "python" {
                    return match crate::runtime::detect_system_install("python") {
                        Some(sys) => RequirementView {
                            id: r.id.clone(),
                            label,
                            wanted: r.wanted.clone(),
                            status: "installed".into(),
                            detail: Some(sys.version),
                        },
                        None => RequirementView {
                            id: r.id.clone(),
                            label,
                            wanted: r.wanted.clone(),
                            status: "unavailable".into(),
                            detail: Some(
                                "Python was not found on PATH. Install it from python.org.".into(),
                            ),
                        },
                    };
                }
                let installed = self.runtimes.installed_versions(&r.id);
                if let Some(v) = crate::php::pick_version(&installed, r.wanted.as_deref()) {
                    return RequirementView {
                        id: r.id.clone(),
                        label,
                        wanted: r.wanted.clone(),
                        status: "installed".into(),
                        detail: Some(v),
                    };
                }
                let available: Vec<String> = catalog
                    .iter()
                    .filter(|m| m.id == r.id)
                    .map(|m| m.version.to_string())
                    .collect();
                match crate::php::pick_version(&available, r.wanted.as_deref()) {
                    Some(v) => RequirementView {
                        id: r.id.clone(),
                        label,
                        wanted: r.wanted.clone(),
                        status: "installable".into(),
                        detail: Some(format!("will download {v}")),
                    },
                    None => RequirementView {
                        id: r.id.clone(),
                        label,
                        wanted: r.wanted.clone(),
                        status: "unavailable".into(),
                        detail: Some(match &r.wanted {
                            Some(w) => format!("version {w} isn't available for Windows"),
                            None => "not available for Windows".into(),
                        }),
                    },
                }
            })
            .collect()
    }

    /// Installs a catalog runtime and waits for it (Quick App runs are sequential).
    fn install_blocking(
        &self,
        id: &str,
        version: &str,
        log: &mut dyn FnMut(&str),
    ) -> Result<(), String> {
        let mut events = self.runtimes.subscribe();
        self.runtimes.install(id, version);
        let started = Instant::now();
        let mut last_pct = 0u64;
        loop {
            if started.elapsed() > Duration::from_secs(45 * 60) {
                return Err(format!("installing {id} {version} timed out"));
            }
            match events.blocking_recv() {
                Ok(RuntimeEvent::Installed {
                    id: i, version: v, ..
                }) if i == id && v == version => return Ok(()),
                Ok(RuntimeEvent::Failed {
                    id: i,
                    version: v,
                    message,
                }) if i == id && v == version => {
                    return Err(format!("installing {id} {version} failed: {message}"));
                }
                // A stop is a deliberate outcome, not a failure: the caller is told what
                // happened instead of being left waiting for an install that will never
                // finish.
                Ok(RuntimeEvent::Cancelled { id: i, version: v }) if i == id && v == version => {
                    return Err(format!(
                        "installing {id} {version} was stopped before it finished"
                    ));
                }
                Ok(RuntimeEvent::Progress {
                    id: i,
                    version: v,
                    state,
                    downloaded,
                    total,
                }) if i == id && v == version => {
                    if let Some(t) = total.filter(|t| *t > 0) {
                        let pct = downloaded * 100 / t;
                        if pct >= last_pct + 10 {
                            last_pct = pct;
                            log(&format!("{id} {version}: {state:?} {pct}%"));
                        }
                    }
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    return Err("the install channel closed".into())
                }
            }
        }
    }

    /// Starts a service (when it isn't running) and waits until its port answers.
    pub(crate) fn start_service_and_wait(
        &self,
        id: &str,
        log: &mut dyn FnMut(&str),
    ) -> Result<(), String> {
        if !self.services.is_running(id) {
            log(&format!("Starting {id}"));
            self.services.start(id)?;
        }
        let Some(port) = self.services.status(id).port else {
            return Ok(());
        };
        // MariaDB initialises its data directory on first start — give them time.
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(120) {
            if !self.services.is_running(id) {
                return Err(format!(
                    "{id} stopped right after starting. See its output on the Processes page."
                ));
            }
            if std::net::TcpStream::connect_timeout(
                &([127, 0, 0, 1], port).into(),
                Duration::from_millis(300),
            )
            .is_ok()
            {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(400));
        }
        Err(format!("{id} did not open port {port} in time"))
    }

    pub(crate) fn install_runtime_blocking(
        &self,
        id: &str,
        version: &str,
        log: &mut dyn FnMut(&str),
    ) -> Result<(), String> {
        self.install_blocking(id, version, log)
    }

    pub(crate) fn quick_ensure_runtime(
        &self,
        id: &str,
        wanted: Option<&str>,
        log: &mut dyn FnMut(&str),
    ) -> Result<String, String> {
        if id == "python" {
            return crate::runtime::detect_system_install("python")
                .map(|s| s.version)
                .ok_or_else(|| {
                    "Python was not found on PATH. Install it from python.org, then run this again."
                        .to_string()
                });
        }
        let catalog = crate::catalog::builtin_catalog();
        let name = catalog
            .iter()
            .find(|m| m.id == id)
            .map(|m| m.name)
            .ok_or_else(|| format!("{id} isn't available for this platform"))?;

        let installed = self.runtimes.installed_versions(id);
        if let Some(v) = crate::php::pick_version(&installed, wanted) {
            return Ok(format!("{name} {v}"));
        }
        let available: Vec<String> = catalog
            .iter()
            .filter(|m| m.id == id)
            .map(|m| m.version.to_string())
            .collect();
        let version = crate::php::pick_version(&available, wanted).ok_or_else(|| match wanted {
            Some(w) => format!(
                "{name} {w} isn't available. Available: {}",
                available.join(", ")
            ),
            None => format!("{name} isn't available"),
        })?;
        log(&format!("Installing {name} {version}"));
        self.install_blocking(id, &version, log)?;
        Ok(format!("{name} {version}"))
    }

    pub(crate) fn quick_resolve_program(
        &self,
        program: &str,
        values: &BTreeMap<String, String>,
        project_id: Option<&str>,
    ) -> Result<ResolvedProgram, String> {
        let is_path =
            program.contains('/') || program.contains('\\') || Path::new(program).is_absolute();
        if is_path {
            return Ok(ResolvedProgram {
                executable: PathBuf::from(program),
                ..Default::default()
            });
        }
        let pick_php = || -> Result<(PathBuf, PathBuf), String> {
            // A project's own resolution (custom pin or manifest) wins, then the wizard's choice.
            if let Some(dir) = project_id
                .and_then(|p| self.project_detail(p))
                .and_then(|d| {
                    d.resolved
                        .into_iter()
                        .find(|r| r.id == "php")
                        .and_then(|r| r.bin_dir)
                })
            {
                let dir = PathBuf::from(dir);
                let exe = dir.join("php.exe");
                if exe.is_file() {
                    return Ok((exe, dir));
                }
            }
            let wanted = values
                .get("php_version")
                .filter(|v| !v.is_empty())
                .map(String::as_str);
            let v = self
                .php
                .pick_version(wanted)
                .ok_or("PHP is not installed. Install it from the Runtimes page.")?;
            let dir = self.runtimes.install_dir("php", &v);
            Ok((dir.join("php.exe"), dir))
        };
        let php_env = |dir: &Path| -> Vec<(String, String)> {
            let mut env = Vec::new();
            // The generated php.ini (extensions on) is only for managed installs.
            if let Some(version) = dir.file_name().and_then(|n| n.to_str()) {
                if dir.starts_with(self.paths.runtimes_dir()) {
                    if let Ok(ini) = self.php.write_ini(version) {
                        env.push(("PHPRC".to_string(), ini.display().to_string()));
                    }
                }
            }
            env
        };

        // A project's own virtual environment "activates" for its commands: python and pip
        // run from its Scripts folder with VIRTUAL_ENV set (§17).
        if matches!(program, "python" | "python3" | "pip" | "pip3") {
            if let Some((venv_dir, scripts)) = self.project_venv(project_id) {
                let name = if program.starts_with("pip") {
                    "pip"
                } else {
                    "python"
                };
                let exe = scripts.join(format!("{name}.exe"));
                if exe.is_file() {
                    return Ok(ResolvedProgram {
                        executable: exe,
                        pre_args: vec![],
                        env: vec![("VIRTUAL_ENV".into(), venv_dir.display().to_string())],
                        path_dirs: vec![scripts],
                    });
                }
            }
        }

        match program {
            "php" => {
                let (exe, dir) = pick_php()?;
                Ok(ResolvedProgram {
                    env: php_env(&dir),
                    executable: exe,
                    pre_args: vec![],
                    path_dirs: vec![dir],
                })
            }
            "composer" => {
                let (exe, dir) = pick_php()?;
                let v = self
                    .runtimes
                    .installed_versions("composer")
                    .into_iter()
                    .next()
                    .ok_or("Composer is not installed. Install it from the Runtimes page.")?;
                let phar = self
                    .runtimes
                    .binary_path("composer", &v)
                    .ok_or("composer.phar is missing")?;
                let home = self.paths.services_dir().join("composer");
                let _ = std::fs::create_dir_all(&home);
                let mut env = php_env(&dir);
                env.push(("COMPOSER_HOME".into(), home.display().to_string()));
                env.push(("COMPOSER_NO_INTERACTION".into(), "1".into()));
                Ok(ResolvedProgram {
                    executable: exe,
                    pre_args: vec![phar.display().to_string()],
                    env,
                    path_dirs: vec![dir],
                })
            }
            "node" | "npm" | "npx" | "corepack" | "pnpm" | "pnpx" | "yarn" => {
                let dir = project_id
                    .and_then(|p| self.project_detail(p))
                    .and_then(|d| {
                        d.resolved
                            .into_iter()
                            .find(|r| r.id == "node")
                            .and_then(|r| r.bin_dir)
                    })
                    .map(PathBuf::from)
                    .or_else(|| {
                        let wanted = values
                            .get("node_version")
                            .filter(|v| !v.is_empty())
                            .map(String::as_str);
                        let installed = self.runtimes.installed_versions("node");
                        crate::php::pick_version(&installed, wanted)
                            .and_then(|v| self.runtimes.bin_dir("node", &v))
                    })
                    .ok_or("Node.js is not installed. Install it from the Runtimes page.")?;
                let file = crate::web::manager::find_executable(Some(&dir), program).ok_or_else(|| {
                    if matches!(program, "pnpm" | "pnpx" | "yarn") {
                        format!("{program} is not switched on for this Node.js. Enable it from the project's Node tools (corepack).")
                    } else {
                        format!("{program} was not found in {}", dir.display())
                    }
                })?;
                Ok(shim(file, vec![dir]))
            }
            other => {
                let file = crate::web::manager::find_executable(None, other).ok_or_else(|| {
                    format!("\"{other}\" was not found. Install it or add it to PATH.")
                })?;
                Ok(shim(file, vec![]))
            }
        }
    }
}
