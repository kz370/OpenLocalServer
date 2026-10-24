use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::*;
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

    fn quick_ensure_runtime(
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

    fn quick_action(
        &self,
        action: &str,
        with: &BTreeMap<String, String>,
        ctx: &RunCtx,
        log: &mut dyn FnMut(&str),
    ) -> Result<ActionOutcome, String> {
        let get = |k: &str| {
            with.get(k)
                .map(String::as_str)
                .ok_or_else(|| format!("the \"{action}\" action needs \"{k}\""))
        };
        match action {
            "make_dir" => {
                let path = get("path")?;
                std::fs::create_dir_all(path)
                    .map_err(|e| format!("could not create {path}: {e}"))?;
                Ok(ActionOutcome::default())
            }
            "replace_in_file" => {
                let root = ctx.project_path.as_ref().ok_or("no project folder")?;
                let file = plan::safe_join(&root.display().to_string(), get("file")?)?;
                let text = std::fs::read_to_string(&file)
                    .map_err(|e| format!("could not read {file}: {e}"))?;
                let find = get("find")?;
                if !text.contains(find) {
                    return Err(format!("could not find \"{find}\" in {file}"));
                }
                std::fs::write(&file, text.replacen(find, get("replace")?, 1))
                    .map_err(|e| e.to_string())?;
                Ok(ActionOutcome::default())
            }
            "start_service" => {
                self.start_service_and_wait(get("id")?, log)?;
                Ok(ActionOutcome::default())
            }
            "create_database" => {
                let (engine, name) = (get("engine")?, get("name")?);
                let started = Instant::now();
                loop {
                    match self.services.create_database(engine, name) {
                        Ok(()) => {
                            return Ok(ActionOutcome {
                                detail: Some(format!("database {name}")),
                                ..Default::default()
                            })
                        }
                        // The server may still be finishing startup right after its port opened.
                        Err(e)
                            if started.elapsed() < Duration::from_secs(30)
                                && e.to_lowercase().contains("connect") =>
                        {
                            std::thread::sleep(Duration::from_millis(700));
                        }
                        Err(e) => return Err(e),
                    }
                }
            }
            "register_project" => {
                let path = get("path")?;
                let project = self
                    .projects
                    .lock()
                    .unwrap()
                    .register(path)
                    .map_err(|e| e.to_string())?;
                Ok(ActionOutcome {
                    project_id: Some(project.id),
                    ..Default::default()
                })
            }
            "create_domain" => {
                let hostname = get("hostname")?.to_string();
                let kind = match get("kind")? {
                    "php" => SiteKind::Php {
                        version: with.get("php_version").cloned(),
                    },
                    "static" => SiteKind::Static,
                    "proxy" => SiteKind::Proxy {
                        upstream_port: with
                            .get("port")
                            .and_then(|p| p.parse().ok())
                            .ok_or("a proxy site needs a valid port")?,
                        upstream_host: with
                            .get("upstream_host")
                            .map(|h| h.trim().to_string())
                            .filter(|h| !h.is_empty() && h != "127.0.0.1" && h != "localhost"),
                        upstream_https: with.get("upstream_https").is_some_and(|v| v == "true"),
                    },
                    other => return Err(format!("unknown site kind {other}")),
                };
                let app = with.get("app_program").map(|program| AppSpec {
                    executable: program.clone(),
                    args: with
                        .get("app_args")
                        .and_then(|a| serde_json::from_str(a).ok())
                        .unwrap_or_default(),
                    cwd: with.get("app_cwd").cloned().unwrap_or_default(),
                    runtime: with.get("app_runtime").cloned(),
                });
                let root = get("root")?.replace('/', "\\");
                let project_id = ctx
                    .project_path
                    .as_ref()
                    .and_then(|p| self.project_by_path(&p.display().to_string()))
                    .map(|p| p.id);
                let https = with.get("https").is_some_and(|v| v == "true");
                let domain = Domain {
                    hostname: hostname.clone(),
                    project_id,
                    root,
                    kind,
                    https,
                    redirect_https: https,
                    wildcard: with.get("wildcard").is_some_and(|v| v == "true"),
                    enabled: true,
                    ownership: Ownership::Managed,
                    app,
                    blocks: SiteBlocks::default(),
                    generated_hashes: BTreeMap::new(),
                    public_domain: None,
                    tunnel_id: None,
                    server: None,
                    path_prefix: None,
                    icon: None,
                    ..Default::default()
                };
                let cfg = self.web_config();
                let url = self.site_url(&domain, &cfg);
                let mut domains = self.domains.lock().unwrap();
                match domains.get(&hostname) {
                    // Re-running a recipe reuses the domain instead of failing on the duplicate.
                    Some(existing) => {
                        let mut updated = domain;
                        updated.generated_hashes = existing.generated_hashes;
                        domains.update(updated).map_err(|e| e.to_string())?;
                    }
                    None => {
                        domains.add(domain).map_err(|e| e.to_string())?;
                    }
                }
                Ok(ActionOutcome {
                    detail: Some(url.clone()),
                    open_url: Some(url),
                    ..Default::default()
                })
            }
            "trust_ca" => {
                self.certs.ca().ensure_created()?;
                if self.certs.ca().is_trusted() {
                    return Ok(ActionOutcome {
                        detail: Some("already trusted".into()),
                        ..Default::default()
                    });
                }
                self.certs
                    .ca()
                    .trust_current_user()
                    .map_err(|e| format!("Windows did not trust the certificate authority: {e}"))?;
                Ok(ActionOutcome::default())
            }
            "apply_web" => {
                let reports = self.apply_web(&[]).map_err(|e| e.to_string())?;
                for r in &reports {
                    for w in &r.warnings {
                        log(&format!("warning: {w}"));
                    }
                }
                Ok(ActionOutcome::default())
            }
            "health_check" => {
                let report = self
                    .health_check(get("hostname")?)
                    .map_err(|e| e.to_string())?;
                for s in &report.steps {
                    log(&format!(
                        "{} {}: {}",
                        if s.ok { "✓" } else { "✗" },
                        s.name,
                        s.detail
                    ));
                }
                if report.ok {
                    Ok(ActionOutcome::default())
                } else {
                    let failed: Vec<String> = report
                        .steps
                        .iter()
                        .filter(|s| !s.ok)
                        .map(|s| format!("{}: {}", s.name, s.detail))
                        .collect();
                    Err(failed.join("; "))
                }
            }
            "download_extract" => {
                self.download_extract(
                    get("url")?,
                    with.get("sha1_url").map(String::as_str),
                    with.get("sha256").map(String::as_str),
                    get("dest")?,
                    with.get("strip").map(String::as_str),
                    log,
                )?;
                Ok(ActionOutcome::default())
            }
            other => Err(format!("unknown action \"{other}\"")),
        }
    }

    /// Downloads a zip over HTTPS, verifies its checksum, and extracts it (stripping a
    /// leading folder). Used by recipes like WordPress that ship as one archive.
    fn download_extract(
        &self,
        url: &str,
        sha1_url: Option<&str>,
        sha256: Option<&str>,
        dest: &str,
        strip: Option<&str>,
        log: &mut dyn FnMut(&str),
    ) -> Result<(), String> {
        use sha1::Digest as _;
        if !url.starts_with("https://") {
            return Err("downloads must use HTTPS".into());
        }
        if sha1_url.is_none() && sha256.is_none() {
            return Err("a download needs a checksum (sha1_url or sha256)".into());
        }
        log(&format!("Downloading {url}"));
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        let client = reqwest::Client::new();
        let (bytes, expected) = rt.block_on(async {
            let bytes = client
                .get(url)
                .send()
                .await
                .and_then(|r| r.error_for_status())
                .map_err(|e| e.to_string())?
                .bytes()
                .await
                .map_err(|e| e.to_string())?;
            let expected = match (sha1_url, sha256) {
                (Some(u), _) => {
                    let text = client
                        .get(u)
                        .send()
                        .await
                        .and_then(|r| r.error_for_status())
                        .map_err(|e| e.to_string())?
                        .text()
                        .await
                        .map_err(|e| e.to_string())?;
                    (
                        "sha1",
                        text.split_whitespace().next().unwrap_or("").to_lowercase(),
                    )
                }
                (None, Some(h)) => ("sha256", h.to_lowercase()),
                _ => unreachable!(),
            };
            Ok::<_, String>((bytes, expected))
        })?;
        let actual = if expected.0 == "sha1" {
            format!("{:x}", sha1::Sha1::digest(&bytes))
        } else {
            format!("{:x}", sha2::Sha256::digest(&bytes))
        };
        if actual != expected.1 {
            return Err(format!(
                "checksum mismatch: expected {}, got {actual}. Nothing was extracted.",
                expected.1
            ));
        }
        log("Checksum verified");
        let cache = self
            .paths
            .cache_dir()
            .join(format!("download-{}.zip", &actual[..16]));
        std::fs::create_dir_all(self.paths.cache_dir()).map_err(|e| e.to_string())?;
        std::fs::write(&cache, &bytes).map_err(|e| e.to_string())?;
        let result = crate::runtime::extract_zip(&cache, Path::new(dest), strip.unwrap_or(""))
            .map_err(|e| e.to_string());
        let _ = std::fs::remove_file(&cache);
        result
    }

    // --------------------------------------------------------------- quick commands

    /// Values available to a Quick Command's `{{placeholders}}`.
    fn command_values(&self, project_id: Option<&str>) -> BTreeMap<String, String> {
        let mut values = BTreeMap::new();
        if let Some(p) = project_id.and_then(|id| self.projects.lock().unwrap().get(id)) {
            values.insert("project_path".into(), p.path.clone());
            values.insert("project_name".into(), p.name.clone());
            if let Some(d) = self
                .domains
                .lock()
                .unwrap()
                .list()
                .into_iter()
                .find(|d| d.project_id.as_deref() == Some(p.id.as_str()))
            {
                values.insert("domain".into(), d.hostname);
            }
        }
        values
    }

    pub fn run_command_line(
        &self,
        line: &str,
        cwd: Option<&str>,
        project_id: Option<&str>,
        name: Option<&str>,
    ) -> Result<ProcessId, CoreError> {
        let tokens = plan::split_command_line(line);
        let (program, args) = tokens
            .split_first()
            .ok_or_else(|| CoreError::QuickAppError("that command line is empty".into()))?;
        self.spawn_project_process(program, args, cwd, project_id, name.unwrap_or(line), line)
    }

    pub(crate) fn spawn_project_process(
        &self,
        program: &str,
        args: &[String],
        cwd: Option<&str>,
        project_id: Option<&str>,
        name: &str,
        history_line: &str,
    ) -> Result<ProcessId, CoreError> {
        let (executable, full_args, env) = self.project_program(program, args, project_id)?;
        let cwd_owned = cwd.map(str::to_string).or_else(|| {
            project_id
                .and_then(|id| self.projects.lock().unwrap().get(id))
                .map(|p| p.path)
        });
        let id = self.supervisor.start(ProcessSpec {
            name: name.to_string(),
            executable: executable.display().to_string(),
            args: full_args,
            cwd: cwd_owned.clone(),
            env,
            restart: None,
        });
        let _ = self
            .history
            .lock()
            .unwrap()
            .record(history_line, cwd_owned.as_deref(), project_id);
        Ok(id)
    }

    /// `program args` resolved the way a project's commands run: its own PHP / Node /
    /// Python, with their folders first on PATH. Returns the executable, all arguments and
    /// the extra environment.
    pub(crate) fn project_program(
        &self,
        program: &str,
        args: &[String],
        project_id: Option<&str>,
    ) -> Result<ProjectProgram, CoreError> {
        let mut values = BTreeMap::new();
        // Pin the resolved versions so `php`/`node` follow the project (§18–19).
        if let Some(detail) = project_id.and_then(|id| self.project_detail(id)) {
            for r in &detail.resolved {
                if let Some(v) = &r.installed_version {
                    values.insert(format!("{}_version", r.id), v.clone());
                }
            }
        }
        let resolved = self
            .quick_resolve_program(program, &values, project_id)
            .map_err(CoreError::QuickAppError)?;
        let mut full_args = resolved.pre_args.clone();
        full_args.extend(args.iter().cloned());
        let mut env = resolved.env.clone();
        // §129: Node's heap limit for every project command (Node ignores it otherwise).
        if let Some(mb) = self.resource_limits().node_max_old_space_mb {
            if !env.iter().any(|(k, _)| k == "NODE_OPTIONS") {
                env.push(("NODE_OPTIONS".into(), format!("--max-old-space-size={mb}")));
            }
        }
        if !resolved.path_dirs.is_empty() {
            let mut dirs: Vec<String> = resolved
                .path_dirs
                .iter()
                .map(|d| d.display().to_string())
                .collect();
            dirs.push(std::env::var("PATH").unwrap_or_default());
            env.push(("PATH".into(), dirs.join(";")));
        }
        Ok((resolved.executable, full_args, env))
    }

    pub fn run_quick_command(
        &self,
        id: &str,
        project_id: Option<&str>,
    ) -> Result<Option<ProcessId>, CoreError> {
        let cmd: QuickCommand = self
            .quick_commands
            .get(id)
            .ok_or_else(|| CoreError::QuickAppError(format!("no Quick Command \"{id}\"")))?;
        let values = self.command_values(project_id);
        let render = |t: &str| plan::render(t, &values, &[]).map_err(CoreError::QuickAppError);

        if let Some(action) = &cmd.action {
            match action.as_str() {
                "open_url" => {
                    let url = cmd
                        .with
                        .get("url")
                        .map(|s| s.as_string())
                        .unwrap_or_default();
                    self.open_url(&render(&url)?)?;
                }
                "open_site" => {
                    let pid = project_id
                        .ok_or_else(|| CoreError::QuickAppError("pick a project first".into()))?;
                    let cfg = self.web_config();
                    let domain = self
                        .domains
                        .lock()
                        .unwrap()
                        .list()
                        .into_iter()
                        .find(|d| d.project_id.as_deref() == Some(pid))
                        .ok_or_else(|| {
                            CoreError::QuickAppError("this project has no domain yet".into())
                        })?;
                    self.open_url(&self.site_url(&domain, &cfg))?;
                }
                "open_web_config" => {
                    let cfg = self.web_config();
                    let dir = self.paths.web_dir().join(cfg.server());
                    std::fs::create_dir_all(&dir)?;
                    self.open_path(&dir.display().to_string())?;
                }
                "restart_project" => {
                    let pid = project_id
                        .ok_or_else(|| CoreError::QuickAppError("pick a project first".into()))?;
                    let hosts: Vec<String> = self
                        .domains
                        .lock()
                        .unwrap()
                        .list()
                        .into_iter()
                        .filter(|d| d.project_id.as_deref() == Some(pid))
                        .map(|d| d.hostname)
                        .collect();
                    for h in hosts {
                        self.web.restart_app(&h);
                    }
                    self.apply_web(&[])?;
                }
                other => {
                    return Err(CoreError::QuickAppError(format!(
                        "unknown action \"{other}\""
                    )))
                }
            }
            return Ok(None);
        }

        let spec = cmd.command.as_ref().expect("validated");
        let cwd = cmd.working_directory.as_deref().map(render).transpose()?;
        let args = spec
            .arguments
            .iter()
            .map(|a| render(a))
            .collect::<Result<Vec<_>, _>>()?;
        let mut line_parts = vec![spec.executable.clone()];
        line_parts.extend(args.iter().map(|a| {
            if a.contains(' ') {
                format!("\"{a}\"")
            } else {
                a.clone()
            }
        }));
        let pid = self.spawn_project_process(
            &spec.executable,
            &args,
            cwd.as_deref(),
            project_id,
            &format!("Quick Command: {}", cmd.name),
            &line_parts.join(" "),
        )?;
        Ok(Some(pid))
    }

    pub fn plan_quick_app(
        &self,
        id: &str,
        values: &BTreeMap<String, String>,
    ) -> Result<QuickPlanResult, CoreError> {
        let detail = self.catalog.lock().unwrap().get(id)?;
        let ctx = self.plan_ctx();
        let (resolved, errors) = plan::resolve_lenient(&detail.app, values, &ctx);
        // Secrets never leave the core — the wizard only ever sees them masked.
        let masked = |vals: &BTreeMap<String, String>| -> BTreeMap<String, String> {
            vals.iter()
                .map(|(k, v)| {
                    let secret = detail
                        .app
                        .variables
                        .iter()
                        .any(|x| &x.name == k && x.is_secret());
                    (
                        k.clone(),
                        if secret && !v.is_empty() {
                            "••••••••".to_string()
                        } else {
                            v.clone()
                        },
                    )
                })
                .collect()
        };
        let display = masked(&resolved);
        if !errors.is_empty() {
            return Ok(QuickPlanResult {
                ok: false,
                errors,
                plan: None,
                requirements: vec![],
                trusted: detail.view.trusted,
                source: detail.view.source_label(),
                values: display,
            });
        }
        let plan =
            plan::build_plan(&detail.app, resolved, &ctx).map_err(CoreError::QuickAppError)?;
        let requirements = self.requirement_views(&plan);
        Ok(QuickPlanResult {
            ok: true,
            errors: vec![],
            plan: Some(plan),
            requirements,
            trusted: detail.view.trusted,
            source: detail.view.source_label(),
            values: display,
        })
    }
}


impl crate::quickapp::EntryView {
    pub fn source_label(&self) -> String {
        match self.source {
            crate::quickapp::EntrySource::Builtin => "built-in".into(),
            crate::quickapp::EntrySource::Local => "your own".into(),
            crate::quickapp::EntrySource::Imported => {
                self.origin.clone().unwrap_or_else(|| "imported".into())
            }
        }
    }
}

/// `.cmd` / `.bat` shims (npm, npx) can't be launched directly — run them through cmd.exe.
/// Executable, full arguments and extra environment of a resolved project command.
pub(crate) type ProjectProgram = (PathBuf, Vec<String>, Vec<(String, String)>);

fn shim(file: PathBuf, path_dirs: Vec<PathBuf>) -> ResolvedProgram {
    let is_shim = file
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"));
    if is_shim {
        ResolvedProgram {
            executable: PathBuf::from("cmd.exe"),
            pre_args: vec!["/C".into(), file.display().to_string()],
            env: vec![],
            path_dirs,
        }
    } else {
        ResolvedProgram {
            executable: file,
            pre_args: vec![],
            env: vec![],
            path_dirs,
        }
    }
}

/// The Quick App runner's window onto the core.
pub struct Host {
    pub inner: Arc<Inner>,
    /// Set when the run belongs to a known project, so runtimes resolve the way the project does.
    pub project_id: Mutex<Option<String>>,
}

impl QuickHost for Host {
    fn ensure_runtime(
        &self,
        id: &str,
        version: Option<&str>,
        log: &mut dyn FnMut(&str),
    ) -> Result<String, String> {
        self.inner.quick_ensure_runtime(id, version, log)
    }
    fn resolve_program(
        &self,
        program: &str,
        values: &BTreeMap<String, String>,
    ) -> Result<ResolvedProgram, String> {
        let pid = self.project_id.lock().unwrap().clone();
        self.inner
            .quick_resolve_program(program, values, pid.as_deref())
    }
    fn action(
        &self,
        action: &str,
        with: &BTreeMap<String, String>,
        ctx: &RunCtx,
        log: &mut dyn FnMut(&str),
    ) -> Result<ActionOutcome, String> {
        let out = self.inner.quick_action(action, with, ctx, log)?;
        if let Some(pid) = &out.project_id {
            *self.project_id.lock().unwrap() = Some(pid.clone());
        }
        Ok(out)
    }
    fn run_elevated(&self, executable: &Path, args: &[String]) -> Result<(), String> {
        crate::elevate::run_elevated_command(executable, args)
    }
}
