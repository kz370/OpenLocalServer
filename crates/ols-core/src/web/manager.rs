//! Web Manager (§22–30, Stages 6, 9, 10): turns the domain list into a running web server.
//!
//! The apply pipeline (§28) is the heart of it: render → write (archiving what it
//! replaces) → validate with the server's own checker → start or reload. If validation
//! fails everything is rolled back to exactly what was on disk before, so a bad edit can
//! never leave the server unable to start.

use std::collections::{BTreeMap, HashMap};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
    server_by_id, Backend, Invocation, PoolSpec, Ports, ServerLayout, SiteSpec, WebConfig, WebServer, SERVER_IDS,
};
use crate::certs::CertificateManager;
use crate::dns::DnsServer;
use crate::domain::{AppSpec, Domain, DomainStore, Ownership, SiteKind};
use crate::error::CoreError;
use crate::exec::run_capture;
use crate::paths::AppPaths;
use crate::php::{PhpPools, PoolStatus};
use crate::port::{check_port, PortStatus};
use crate::process::{ProcessId, ProcessSpec, ProcessSupervisor};
use crate::runtime::RuntimeManager;

/// How a caller (the Core) resolves things only it knows: which PHP a project wants, and
/// where a project's runtime lives. Keeps `WebManager` free of project/settings state.
pub struct ApplyContext<'a> {
    pub cfg: &'a WebConfig,
    /// The PHP version (any prefix, e.g. "8.1") a domain should run, if it has an opinion.
    pub php_for: &'a dyn Fn(&Domain) -> Option<String>,
    /// The bin dir of runtime `id` ("node", "python") to put first on PATH for a domain's app.
    pub runtime_bin: &'a dyn Fn(&Domain, &str) -> Option<PathBuf>,
    /// Hostnames whose drifted (hand-edited) generated file the user agreed to overwrite (§27).
    pub overwrite: &'a [String],
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ApplyReport {
    pub server: String,
    pub written: Vec<String>,
    pub unchanged: Vec<String>,
    /// Hand-edited generated files that were left alone (§26–27).
    pub drifted: Vec<String>,
    pub started: bool,
    pub reloaded: bool,
    pub hosts_updated: bool,
    pub validator_output: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigPart {
    Main,
    /// The generated (or, for Manual sites, user-owned) per-site file.
    Site,
    /// The user-owned snippet of an Advanced site.
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigFile {
    pub hostname: Option<String>,
    pub part: ConfigPart,
    pub path: String,
    pub ownership: Option<Ownership>,
    /// The file on disk no longer matches what DevForge last wrote (§26).
    pub drifted: bool,
    pub editable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigVersion {
    pub id: String,
    pub part: ConfigPart,
    pub timestamp_ms: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerAvailability {
    pub id: String,
    pub name: String,
    pub installed: bool,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppStatus {
    pub hostname: String,
    pub running: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebStatus {
    pub server: String,
    pub servers: Vec<ServerAvailability>,
    pub running: bool,
    pub http_port: u16,
    pub https_port: u16,
    /// Human-readable reasons a port can't be used (owned by something else).
    pub port_conflicts: Vec<String>,
    pub php_pools: Vec<PoolStatus>,
    pub apps: Vec<AppStatus>,
    pub dns_running: bool,
    pub dns_port: u16,
    pub error_log: Option<String>,
}

struct Running {
    id: String,
    process: ProcessId,
}

struct State {
    server: Option<Running>,
    apps: HashMap<String, ProcessId>,
    dns: Option<DnsServer>,
}

pub struct WebManager {
    paths: AppPaths,
    runtimes: Arc<RuntimeManager>,
    supervisor: Arc<ProcessSupervisor>,
    certs: Arc<CertificateManager>,
    php: Arc<PhpPools>,
    state: Mutex<State>,
}

fn sha_hex(text: &str) -> String {
    let mut h = Sha256::new();
    h.update(text.as_bytes());
    format!("{:x}", h.finalize())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn werr(msg: impl Into<String>) -> CoreError {
    CoreError::WebError(msg.into())
}

impl WebManager {
    pub fn new(
        paths: AppPaths,
        runtimes: Arc<RuntimeManager>,
        supervisor: Arc<ProcessSupervisor>,
        certs: Arc<CertificateManager>,
        php: Arc<PhpPools>,
    ) -> Self {
        Self {
            paths,
            runtimes,
            supervisor,
            certs,
            php,
            state: Mutex::new(State { server: None, apps: HashMap::new(), dns: None }),
        }
    }

    pub fn certs(&self) -> &Arc<CertificateManager> {
        &self.certs
    }

    pub fn php(&self) -> &Arc<PhpPools> {
        &self.php
    }

    // ---------------------------------------------------------------- layout

    fn layout(&self, server: &dyn WebServer) -> Result<ServerLayout, CoreError> {
        let id = server.id();
        let version = self
            .runtimes
            .installed_versions(id)
            .into_iter()
            .next()
            .ok_or_else(|| werr(format!("{} is not installed. Install it from the Runtimes page first.", server.name())))?;
        let binary = self
            .runtimes
            .binary_path(id, &version)
            .ok_or_else(|| werr(format!("{} {version} is missing its executable", server.name())))?;
        let prefix = self.paths.web_dir().join(id);
        Ok(ServerLayout {
            install_dir: self.runtimes.install_dir(id, &version),
            sites_dir: prefix.join("sites"),
            custom_dir: prefix.join("custom"),
            logs_dir: prefix.join("logs"),
            prefix,
            binary,
        })
    }

    fn history_dir(&self, server_id: &str, hostname: &str) -> PathBuf {
        self.paths.web_dir().join(server_id).join("history").join(hostname)
    }

    /// Where a config file lives on disk.
    fn file_path(&self, server: &dyn WebServer, layout: &ServerLayout, hostname: Option<&str>, part: ConfigPart) -> Result<PathBuf, CoreError> {
        match (part, hostname) {
            (ConfigPart::Main, _) => Ok(server.main_config(layout)),
            (ConfigPart::Site, Some(h)) => Ok(layout.site_file(server.config_ext(), h)),
            (ConfigPart::Custom, Some(h)) => Ok(layout.custom_file(server.config_ext(), h)),
            _ => Err(werr("a hostname is required for site config files")),
        }
    }

    // ---------------------------------------------------------------- status

    pub fn status(&self, cfg: &WebConfig) -> WebStatus {
        let servers = SERVER_IDS
            .iter()
            .filter_map(|id| server_by_id(id))
            .map(|s| ServerAvailability {
                id: s.id().to_string(),
                name: s.name().to_string(),
                installed: !self.runtimes.installed_versions(s.id()).is_empty(),
                active: s.id() == cfg.server,
            })
            .collect();

        let st = self.state.lock().unwrap();
        let running = st.server.as_ref().is_some_and(|r| self.supervisor.is_alive(r.process));
        let mut port_conflicts = Vec::new();
        if !running {
            for (label, port) in [("HTTP", cfg.http_port), ("HTTPS", cfg.https_port)] {
                if let PortStatus::InUse { pid, process_name } = check_port(port) {
                    let who = match (process_name, pid) {
                        (Some(n), Some(p)) => format!("{n} (PID {p})"),
                        (None, Some(p)) => format!("PID {p}"),
                        _ => "another program".to_string(),
                    };
                    port_conflicts.push(format!("{label} port {port} is in use by {who}"));
                }
            }
        }
        let apps = st
            .apps
            .iter()
            .map(|(h, id)| AppStatus { hostname: h.clone(), running: self.supervisor.is_alive(*id) })
            .collect();
        let error_log = server_by_id(&cfg.server)
            .and_then(|s| self.layout(s.as_ref()).ok().map(|l| s.error_log(&l).display().to_string()));

        WebStatus {
            server: cfg.server.clone(),
            servers,
            running,
            http_port: cfg.http_port,
            https_port: cfg.https_port,
            port_conflicts,
            php_pools: self.php.status(),
            apps,
            dns_running: st.dns.is_some(),
            dns_port: cfg.dns_port,
            error_log,
        }
    }

    pub fn is_running(&self) -> bool {
        let st = self.state.lock().unwrap();
        st.server.as_ref().is_some_and(|r| self.supervisor.is_alive(r.process))
    }

    // ---------------------------------------------------------------- apply (§28)

    pub fn apply(&self, ctx: &ApplyContext, domains: &mut DomainStore) -> Result<ApplyReport, CoreError> {
        let cfg = ctx.cfg;
        let server = server_by_id(&cfg.server).ok_or_else(|| werr(format!("unknown web server \"{}\"", cfg.server)))?;
        let layout = self.layout(server.as_ref())?;
        server.prepare(&layout).map_err(|e| werr(format!("could not prepare {}: {e}", server.name())))?;
        let ports = Ports { http: cfg.http_port, https: cfg.https_port };
        let mut report = ApplyReport { server: server.id().to_string(), ..Default::default() };

        // Only one server may own ports 80/443 — stop a different one before switching.
        self.stop_other_server(server.id());

        let all = domains.list();
        let enabled: Vec<Domain> = all.iter().filter(|d| d.enabled).cloned().collect();

        // 1. PHP pools for every version a PHP site needs.
        let mut pool_specs: BTreeMap<String, PoolSpec> = BTreeMap::new();
        let mut site_pools: HashMap<String, (String, Vec<u16>)> = HashMap::new();
        let mut versions_in_use = Vec::new();
        for d in &enabled {
            let SiteKind::Php { version } = &d.kind else { continue };
            let wanted = version.clone().or_else(|| (ctx.php_for)(d));
            let picked = self.php.pick_version(wanted.as_deref()).ok_or_else(|| {
                werr(match &wanted {
                    Some(v) => format!("{} needs PHP {v}, which is not installed. Install it from the Runtimes page.", d.hostname),
                    None => format!("{} is a PHP site but no PHP is installed. Install one from the Runtimes page.", d.hostname),
                })
            })?;
            let ports_for_version = self.php.ensure(&picked, cfg.php_workers).map_err(werr)?;
            let pool_id = PhpPools::pool_id(&picked);
            pool_specs.entry(pool_id.clone()).or_insert(PoolSpec { id: pool_id.clone(), ports: ports_for_version.clone() });
            site_pools.insert(d.hostname.clone(), (pool_id, ports_for_version));
            versions_in_use.push(picked);
        }
        self.php.retain(&versions_in_use);

        // 2. Certificates, then the rendered site files.
        let mut rendered: Vec<(Domain, String)> = Vec::new();
        for d in &enabled {
            let tls = if d.https { Some(self.certs.ensure_for(d).map_err(werr)?) } else { None };
            let backend = match &d.kind {
                SiteKind::Php { .. } => {
                    let (pool, ports) = site_pools[&d.hostname].clone();
                    Backend::Php { pool, ports }
                }
                SiteKind::Proxy { upstream_port } => Backend::Proxy { upstream_port: *upstream_port },
                SiteKind::Static => Backend::Static,
            };
            let custom_snippet = (d.ownership == Ownership::Advanced).then(|| layout.custom_file(server.config_ext(), &d.hostname));
            let spec = SiteSpec {
                hostname: d.hostname.clone(),
                wildcard: d.wildcard,
                root: d.root.clone(),
                backend,
                tls,
                redirect_https: d.https && d.redirect_https,
                blocks: d.blocks.clone(),
                custom_snippet,
            };
            rendered.push((d.clone(), server.render_site(&spec, ports)));
        }
        let pools_vec: Vec<PoolSpec> = pool_specs.into_values().collect();
        let main_text = server.render_main(&layout, ports, &pools_vec);

        // 3. Snapshot what's on disk (for rollback), then write.
        let ext = server.config_ext();
        let mut snapshot: HashMap<PathBuf, Option<String>> = HashMap::new();
        let mut remember = |p: &Path| {
            snapshot.entry(p.to_path_buf()).or_insert_with(|| std::fs::read_to_string(p).ok());
        };

        let main_path = server.main_config(&layout);
        remember(&main_path);
        let mut new_hashes: Vec<(String, String)> = Vec::new();
        let mut plan: Vec<(PathBuf, String, Option<String>)> = vec![(main_path.clone(), main_text, None)];

        for (d, text) in &rendered {
            let path = layout.site_file(ext, &d.hostname);
            remember(&path);
            let on_disk = std::fs::read_to_string(&path).ok();
            let known_hash = d.generated_hashes.get(server.id());

            let content = match (d.ownership, &on_disk) {
                // The user owns this file: never rewrite it once it exists.
                (Ownership::Manual, Some(existing)) => {
                    report.unchanged.push(d.hostname.clone());
                    new_hashes.push((d.hostname.clone(), sha_hex(existing)));
                    continue;
                }
                _ => text.clone(),
            };
            if let (Some(existing), Some(known)) = (&on_disk, known_hash) {
                let drifted = sha_hex(existing) != *known;
                if drifted && !ctx.overwrite.contains(&d.hostname) {
                    report.drifted.push(d.hostname.clone());
                    continue;
                }
            }
            if on_disk.as_deref() == Some(content.as_str()) {
                report.unchanged.push(d.hostname.clone());
                new_hashes.push((d.hostname.clone(), sha_hex(&content)));
                continue;
            }
            new_hashes.push((d.hostname.clone(), sha_hex(&content)));
            plan.push((path, content, Some(d.hostname.clone())));

            if d.ownership == Ownership::Advanced {
                let custom = layout.custom_file(ext, &d.hostname);
                remember(&custom);
                if !custom.exists() {
                    plan.push((
                        custom,
                        format!("# Your own {} directives for {}. OpenLocalServer never overwrites this file.\n", server.name(), d.hostname),
                        None,
                    ));
                }
            }
        }
        // Advanced sites whose site file didn't change still need their snippet to exist.
        for (d, _) in &rendered {
            if d.ownership == Ownership::Advanced {
                let custom = layout.custom_file(ext, &d.hostname);
                if !custom.exists() && !plan.iter().any(|(p, _, _)| *p == custom) {
                    remember(&custom);
                    plan.push((custom, format!("# Your own {} directives for {}.\n", server.name(), d.hostname), None));
                }
            }
        }

        // Stale site files (deleted or disabled domains) are removed, archived first.
        let keep: Vec<String> = enabled.iter().map(|d| format!("{}.{ext}", d.hostname)).collect();
        let mut stale: Vec<PathBuf> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&layout.sites_dir) {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name.ends_with(&format!(".{ext}")) && !keep.contains(&name) {
                    remember(&e.path());
                    stale.push(e.path());
                }
            }
        }

        let write_all = || -> std::io::Result<()> {
            for (path, content, host) in &plan {
                if let Some(host) = host {
                    if let Some(old) = std::fs::read_to_string(path).ok() {
                        self.archive(server.id(), host, ConfigPart::Site, ext, &old);
                    }
                }
                write_atomic(path, content)?;
            }
            for path in &stale {
                if let (Some(stem), Ok(old)) = (path.file_name().and_then(|n| n.to_str()), std::fs::read_to_string(path)) {
                    let host = stem.trim_end_matches(&format!(".{ext}")).to_string();
                    self.archive(server.id(), &host, ConfigPart::Site, ext, &old);
                }
                let _ = std::fs::remove_file(path);
            }
            Ok(())
        };
        if let Err(e) = write_all() {
            restore(&snapshot);
            return Err(werr(format!("could not write the web-server config: {e}")));
        }
        report.written = plan.iter().filter_map(|(_, _, h)| h.clone()).collect();

        // 4. Validate with the server's own checker — roll back on failure (§28).
        let check = self.run_invocation(&layout, &server.validate(&layout));
        report.validator_output = check.combined();
        if !check.success() {
            restore(&snapshot);
            return Err(werr(format!(
                "{} rejected the generated config, so nothing was changed:\n{}",
                server.name(),
                check.combined()
            )));
        }

        // Config is good: record the hashes so future hand-edits show up as drift.
        for (host, hash) in new_hashes {
            if let Some(mut d) = domains.get(&host) {
                d.generated_hashes.insert(server.id().to_string(), hash);
                let _ = domains.update(d);
            }
        }

        // 5. Start (or gracefully reload).
        self.start_or_reload(server.as_ref(), &layout, cfg, &mut report)?;

        // 6. Site app processes (npm run dev, uvicorn, ...).
        self.sync_apps(ctx, &enabled, &mut report);

        // 7. Hosts file + wildcard DNS. Failures here don't invalidate the server config.
        let mut hostnames: Vec<String> = enabled.iter().map(|d| d.hostname.clone()).collect();
        hostnames.sort();
        match crate::hosts::sync(&hostnames) {
            Ok(changed) => report.hosts_updated = changed,
            Err(e) => report.warnings.push(format!("The hosts file was not updated: {e}")),
        }
        self.sync_dns(cfg, &enabled, &mut report);

        Ok(report)
    }

    fn run_invocation(&self, layout: &ServerLayout, inv: &Invocation) -> crate::exec::Captured {
        run_capture(&layout.binary, &inv.args, Some(&inv.cwd), &[], Duration::from_secs(30))
    }

    fn stop_other_server(&self, keep_id: &str) {
        let mut st = self.state.lock().unwrap();
        if st.server.as_ref().is_some_and(|r| r.id != keep_id) {
            if let Some(r) = st.server.take() {
                self.supervisor.stop(r.process);
                drop(st);
                wait_port_free_all(Duration::from_secs(3));
            }
        }
    }

    fn start_or_reload(
        &self,
        server: &dyn WebServer,
        layout: &ServerLayout,
        cfg: &WebConfig,
        report: &mut ApplyReport,
    ) -> Result<(), CoreError> {
        let alive = {
            let st = self.state.lock().unwrap();
            st.server.as_ref().filter(|r| r.id == server.id() && self.supervisor.is_alive(r.process)).map(|r| r.process)
        };

        if let Some(process) = alive {
            if let Some(inv) = server.reload(layout) {
                let out = self.run_invocation(layout, &inv);
                if out.success() {
                    report.reloaded = true;
                    return Ok(());
                }
                report.warnings.push(format!("Reload failed, restarting instead: {}", out.combined()));
            }
            // No graceful reload (Apache in foreground mode) or it failed: restart.
            self.supervisor.stop(process);
            self.state.lock().unwrap().server = None;
            wait_port_free_all(Duration::from_secs(4));
        }

        // Ports must be ours to take.
        for (label, port) in [("HTTP", cfg.http_port), ("HTTPS", cfg.https_port)] {
            if let PortStatus::InUse { pid, process_name } = check_port(port) {
                let who = match (process_name, pid) {
                    (Some(n), Some(p)) => format!("{n} (PID {p})"),
                    (None, Some(p)) => format!("PID {p}"),
                    _ => "another program".to_string(),
                };
                return Err(werr(format!(
                    "{label} port {port} is already in use by {who}. Stop it, or choose different ports in the web settings."
                )));
            }
        }

        let inv = server.start(layout);
        let process = self.supervisor.start(ProcessSpec {
            name: server.name().to_string(),
            executable: layout.binary.display().to_string(),
            args: inv.args,
            cwd: Some(inv.cwd.display().to_string()),
            env: vec![],
            restart: None,
        });
        if let Err(msg) = self.wait_ready(process, cfg.http_port, Duration::from_secs(10)) {
            self.supervisor.stop(process);
            let log_tail = tail(&server.error_log(layout), 8);
            return Err(werr(format!("{} did not start: {msg}{log_tail}", server.name())));
        }
        self.state.lock().unwrap().server = Some(Running { id: server.id().to_string(), process });
        report.started = true;
        Ok(())
    }

    fn wait_ready(&self, process: ProcessId, port: u16, timeout: Duration) -> Result<(), String> {
        let started = Instant::now();
        loop {
            if !self.supervisor.is_alive(process) {
                let out = self.supervisor.recent_output(process);
                let text = out.iter().rev().take(6).rev().cloned().collect::<Vec<_>>().join("\n");
                return Err(if text.is_empty() { "the process exited immediately".into() } else { text });
            }
            if TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(200)).is_ok() {
                return Ok(());
            }
            if started.elapsed() > timeout {
                return Err(format!("nothing answered on port {port} within {}s", timeout.as_secs()));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    pub fn stop(&self) {
        let mut st = self.state.lock().unwrap();
        if let Some(r) = st.server.take() {
            self.supervisor.stop(r.process);
        }
        for (_, id) in st.apps.drain() {
            self.supervisor.stop(id);
        }
        st.dns = None;
        drop(st);
        self.php.stop_all();
    }

    /// Re-runs the server's config check against what's on disk, without changing anything.
    pub fn validate(&self, cfg: &WebConfig) -> Result<String, CoreError> {
        let server = server_by_id(&cfg.server).ok_or_else(|| werr("unknown web server"))?;
        let layout = self.layout(server.as_ref())?;
        let out = self.run_invocation(&layout, &server.validate(&layout));
        if out.success() {
            Ok(out.combined())
        } else {
            Err(werr(out.combined()))
        }
    }

    // ---------------------------------------------------------------- apps

    fn sync_apps(&self, ctx: &ApplyContext, enabled: &[Domain], report: &mut ApplyReport) {
        let mut st = self.state.lock().unwrap();
        let wanted: Vec<&Domain> = enabled.iter().filter(|d| d.app.is_some()).collect();

        // Stop apps whose site was removed/disabled or lost its app command.
        let stale: Vec<String> =
            st.apps.keys().filter(|h| !wanted.iter().any(|d| &d.hostname == *h)).cloned().collect();
        for host in stale {
            if let Some(id) = st.apps.remove(&host) {
                self.supervisor.stop(id);
            }
        }

        for d in wanted {
            if st.apps.get(&d.hostname).is_some_and(|id| self.supervisor.is_alive(*id)) {
                continue;
            }
            let app = d.app.as_ref().expect("filtered above");
            let port = match d.kind {
                SiteKind::Proxy { upstream_port } => Some(upstream_port),
                _ => None,
            };
            let bin_dir = app.runtime.as_deref().and_then(|rt| (ctx.runtime_bin)(d, rt));
            match build_app_spec(&d.hostname, app, port, bin_dir.as_deref()) {
                Ok(spec) => {
                    let id = self.supervisor.start(spec);
                    st.apps.insert(d.hostname.clone(), id);
                }
                Err(e) => report.warnings.push(format!("{}: could not start the app: {e}", d.hostname)),
            }
        }
    }

    pub fn restart_app(&self, hostname: &str) {
        let mut st = self.state.lock().unwrap();
        if let Some(id) = st.apps.remove(hostname) {
            self.supervisor.stop(id);
        }
    }

    // ---------------------------------------------------------------- wildcard DNS (§46–47)

    fn sync_dns(&self, cfg: &WebConfig, enabled: &[Domain], report: &mut ApplyReport) {
        let suffixes: Vec<String> = enabled.iter().filter(|d| d.wildcard).map(|d| d.hostname.clone()).collect();
        let mut st = self.state.lock().unwrap();

        if suffixes.is_empty() {
            st.dns = None;
        } else if let Some(dns) = &st.dns {
            dns.set_suffixes(suffixes.clone());
        } else {
            match DnsServer::start(cfg.dns_port, suffixes.clone()) {
                Ok(dns) => st.dns = Some(dns),
                Err(e) => {
                    report.warnings.push(format!("Wildcard DNS is not running: {e}"));
                    return;
                }
            }
        }
        drop(st);

        // Windows only sends a suffix to our resolver if an NRPT rule says so. Track which
        // rules are installed so a rule costs one UAC prompt, once, not one per apply.
        let file = self.paths.web_dir().join("nrpt.json");
        let mut installed: Vec<String> =
            std::fs::read_to_string(&file).ok().and_then(|r| serde_json::from_str(&r).ok()).unwrap_or_default();
        let mut changed = false;
        // The helper's NRPT rule can only target port 53; skip when a test/alternate port is in use.
        if cfg.dns_port == 53 && std::env::var("OLS_HOSTS_FILE").is_err() {
            for suffix in &suffixes {
                if !installed.contains(suffix) {
                    match crate::elevate::run_helper(&["nrpt-add".into(), format!(".{suffix}"), "127.0.0.1".into()]) {
                        Ok(()) => {
                            installed.push(suffix.clone());
                            changed = true;
                        }
                        Err(e) => report.warnings.push(format!("Windows DNS rule for *.{suffix} was not added: {e}")),
                    }
                }
            }
            let gone: Vec<String> = installed.iter().filter(|s| !suffixes.contains(s)).cloned().collect();
            for suffix in gone {
                if crate::elevate::run_helper(&["nrpt-remove".into(), format!(".{suffix}")]).is_ok() {
                    installed.retain(|s| *s != suffix);
                    changed = true;
                }
            }
        }
        if changed {
            let _ = std::fs::write(&file, serde_json::to_string_pretty(&installed).unwrap_or_default());
        }
    }

    // ---------------------------------------------------------------- config files (§25–29)

    pub fn list_configs(&self, cfg: &WebConfig, domains: &DomainStore) -> Result<Vec<ConfigFile>, CoreError> {
        let server = server_by_id(&cfg.server).ok_or_else(|| werr("unknown web server"))?;
        let layout = self.layout(server.as_ref())?;
        let mut files = vec![ConfigFile {
            hostname: None,
            part: ConfigPart::Main,
            path: server.main_config(&layout).display().to_string(),
            ownership: None,
            drifted: false,
            editable: false,
        }];
        for d in domains.list() {
            let path = layout.site_file(server.config_ext(), &d.hostname);
            let drifted = std::fs::read_to_string(&path).ok().zip(d.generated_hashes.get(server.id())).is_some_and(|(text, h)| {
                d.ownership != Ownership::Manual && sha_hex(&text) != *h
            });
            files.push(ConfigFile {
                hostname: Some(d.hostname.clone()),
                part: ConfigPart::Site,
                path: path.display().to_string(),
                ownership: Some(d.ownership),
                drifted,
                editable: d.ownership == Ownership::Manual,
            });
            if d.ownership == Ownership::Advanced {
                files.push(ConfigFile {
                    hostname: Some(d.hostname.clone()),
                    part: ConfigPart::Custom,
                    path: layout.custom_file(server.config_ext(), &d.hostname).display().to_string(),
                    ownership: Some(d.ownership),
                    drifted: false,
                    editable: true,
                });
            }
        }
        Ok(files)
    }

    pub fn read_config(&self, cfg: &WebConfig, hostname: Option<&str>, part: ConfigPart) -> Result<String, CoreError> {
        let server = server_by_id(&cfg.server).ok_or_else(|| werr("unknown web server"))?;
        let layout = self.layout(server.as_ref())?;
        let path = self.file_path(server.as_ref(), &layout, hostname, part)?;
        std::fs::read_to_string(&path).map_err(|e| werr(format!("could not read {}: {e}", path.display())))
    }

    /// Saves an edit through the same pipeline as a full apply (§28): archive, write,
    /// validate, and roll back on failure; reload on success.
    pub fn write_config(
        &self,
        cfg: &WebConfig,
        domains: &mut DomainStore,
        hostname: &str,
        part: ConfigPart,
        content: &str,
    ) -> Result<String, CoreError> {
        let server = server_by_id(&cfg.server).ok_or_else(|| werr("unknown web server"))?;
        let layout = self.layout(server.as_ref())?;
        let domain = domains.get(hostname).ok_or_else(|| werr(format!("{hostname} is not a known domain")))?;

        match (part, domain.ownership) {
            (ConfigPart::Main, _) => return Err(werr("The main config is generated by OpenLocalServer and can't be edited here.")),
            (ConfigPart::Site, Ownership::Manual) | (ConfigPart::Custom, Ownership::Advanced) => {}
            (ConfigPart::Site, own) => {
                return Err(werr(format!(
                    "This site's config is {} by OpenLocalServer. Switch it to Manual to edit the whole file, or Advanced to add your own directives.",
                    if own == Ownership::Managed { "managed" } else { "generated" }
                )))
            }
            (ConfigPart::Custom, _) => return Err(werr("Only Advanced sites have a custom snippet.")),
        }

        let ext = server.config_ext();
        let path = self.file_path(server.as_ref(), &layout, Some(hostname), part)?;
        let previous = std::fs::read_to_string(&path).ok();
        if let Some(old) = &previous {
            self.archive(server.id(), hostname, part, ext, old);
        }
        std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| werr(e.to_string()))?;
        write_atomic(&path, content).map_err(|e| werr(format!("could not write {}: {e}", path.display())))?;

        let check = self.run_invocation(&layout, &server.validate(&layout));
        if !check.success() {
            match &previous {
                Some(old) => {
                    let _ = write_atomic(&path, old);
                }
                None => {
                    let _ = std::fs::remove_file(&path);
                }
            }
            return Err(werr(format!(
                "{} rejected that config, so your previous version was restored:\n{}",
                server.name(),
                check.combined()
            )));
        }

        if part == ConfigPart::Site {
            let mut d = domain;
            d.generated_hashes.insert(server.id().to_string(), sha_hex(content));
            let _ = domains.update(d);
        }

        let mut report = ApplyReport::default();
        if self.is_running() {
            self.start_or_reload(server.as_ref(), &layout, cfg, &mut report)?;
        }
        Ok(check.combined())
    }

    /// §26–27: change who owns a site's config.
    pub fn set_ownership(
        &self,
        cfg: &WebConfig,
        domains: &mut DomainStore,
        hostname: &str,
        ownership: Ownership,
    ) -> Result<(), CoreError> {
        let mut d = domains.get(hostname).ok_or_else(|| werr(format!("{hostname} is not a known domain")))?;
        if d.ownership == ownership {
            return Ok(());
        }
        if let Some(server) = server_by_id(&cfg.server) {
            if let Ok(layout) = self.layout(server.as_ref()) {
                let ext = server.config_ext();
                let site = layout.site_file(ext, hostname);
                // Whatever the file held is about to stop being DevForge's to regenerate —
                // or is about to be regenerated over. Either way keep a copy.
                if let Ok(old) = std::fs::read_to_string(&site) {
                    self.archive(server.id(), hostname, ConfigPart::Site, ext, &old);
                    if ownership == Ownership::Manual {
                        // The current generated content becomes the user's starting point.
                        d.generated_hashes.insert(server.id().to_string(), sha_hex(&old));
                    }
                }
                if ownership == Ownership::Advanced {
                    let custom = layout.custom_file(ext, hostname);
                    if !custom.exists() {
                        std::fs::create_dir_all(&layout.custom_dir).map_err(|e| werr(e.to_string()))?;
                        write_atomic(
                            &custom,
                            &format!("# Your own {} directives for {hostname}. OpenLocalServer never overwrites this file.\n", server.name()),
                        )
                        .map_err(|e| werr(e.to_string()))?;
                    }
                }
            }
        }
        d.ownership = ownership;
        domains.update(d)?;
        Ok(())
    }

    // ---------------------------------------------------------------- history (§29)

    fn archive(&self, server_id: &str, hostname: &str, part: ConfigPart, ext: &str, content: &str) {
        let dir = self.history_dir(server_id, hostname);
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        // Skip a byte-identical duplicate of the newest entry.
        if let Some(latest) = self.list_history_raw(server_id, hostname).into_iter().max_by_key(|v| v.timestamp_ms) {
            let latest_path = dir.join(format!("{}.{ext}", latest.id));
            if latest.part == part && std::fs::read_to_string(latest_path).ok().as_deref() == Some(content) {
                return;
            }
        }
        let tag = if part == ConfigPart::Custom { "custom" } else { "site" };
        let mut ts = now_ms();
        // Two archives within one millisecond must not collide.
        while dir.join(format!("{ts}-{tag}.{ext}")).exists() {
            ts += 1;
        }
        let _ = std::fs::write(dir.join(format!("{ts}-{tag}.{ext}")), content);
    }

    fn list_history_raw(&self, server_id: &str, hostname: &str) -> Vec<ConfigVersion> {
        let Ok(entries) = std::fs::read_dir(self.history_dir(server_id, hostname)) else { return Vec::new() };
        let mut out = Vec::new();
        for e in entries.flatten() {
            let path = e.path();
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
            let Some((ts, tag)) = stem.split_once('-') else { continue };
            let Ok(timestamp_ms) = ts.parse::<u64>() else { continue };
            let part = if tag == "custom" { ConfigPart::Custom } else { ConfigPart::Site };
            out.push(ConfigVersion {
                id: stem.to_string(),
                part,
                timestamp_ms,
                bytes: e.metadata().map(|m| m.len()).unwrap_or(0),
            });
        }
        out
    }

    pub fn list_history(&self, cfg: &WebConfig, hostname: &str) -> Vec<ConfigVersion> {
        let mut v = self.list_history_raw(&cfg.server, hostname);
        v.sort_by(|a, b| b.timestamp_ms.cmp(&a.timestamp_ms));
        v
    }

    pub fn read_history(&self, cfg: &WebConfig, hostname: &str, id: &str) -> Result<String, CoreError> {
        let server = server_by_id(&cfg.server).ok_or_else(|| werr("unknown web server"))?;
        if id.contains(['/', '\\', '.']) {
            return Err(werr("invalid history id"));
        }
        let path = self.history_dir(server.id(), hostname).join(format!("{id}.{}", server.config_ext()));
        std::fs::read_to_string(&path).map_err(|e| werr(format!("could not read that version: {e}")))
    }

    /// Restores an old version through the normal validated write path.
    pub fn restore_history(&self, cfg: &WebConfig, domains: &mut DomainStore, hostname: &str, id: &str) -> Result<String, CoreError> {
        let content = self.read_history(cfg, hostname, id)?;
        let part = if id.ends_with("-custom") { ConfigPart::Custom } else { ConfigPart::Site };
        self.write_config(cfg, domains, hostname, part, &content)
    }

    /// Copies a config file (or a history version) to `dest` so it can be shared or diffed elsewhere.
    pub fn export_config(&self, cfg: &WebConfig, hostname: &str, part: ConfigPart, dest: &str) -> Result<(), CoreError> {
        let text = self.read_config(cfg, Some(hostname), part)?;
        std::fs::write(dest, text).map_err(|e| werr(format!("could not export: {e}")))
    }

    /// `log`'s last lines, for the Logs page.
    pub fn error_log_tail(&self, cfg: &WebConfig, lines: usize) -> Vec<String> {
        let Some(server) = server_by_id(&cfg.server) else { return Vec::new() };
        let Ok(layout) = self.layout(server.as_ref()) else { return Vec::new() };
        read_tail(&server.error_log(&layout), lines)
    }
}

fn read_tail(path: &Path, lines: usize) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(path) else { return Vec::new() };
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].iter().map(|s| s.to_string()).collect()
}

fn tail(path: &Path, lines: usize) -> String {
    let t = read_tail(path, lines);
    if t.is_empty() {
        String::new()
    } else {
        format!("\nLast lines of the error log:\n{}", t.join("\n"))
    }
}

fn write_atomic(path: &Path, content: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp-write");
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path)
}

/// Puts every touched file back exactly as it was (or removes it if it didn't exist).
fn restore(snapshot: &HashMap<PathBuf, Option<String>>) {
    for (path, old) in snapshot {
        match old {
            Some(text) => {
                let _ = write_atomic(path, text);
            }
            None => {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

fn wait_port_free_all(timeout: Duration) {
    // Best effort: give the OS a moment to release listening sockets after a kill.
    std::thread::sleep(timeout.min(Duration::from_millis(600)));
}

/// Finds `exe` (with common Windows extensions) in `dir`, or on PATH when `dir` is None.
pub fn find_executable(dir: Option<&Path>, exe: &str) -> Option<PathBuf> {
    let candidates = [exe.to_string(), format!("{exe}.exe"), format!("{exe}.cmd"), format!("{exe}.bat")];
    let search = |d: &Path| candidates.iter().map(|c| d.join(c)).find(|p| p.is_file());
    if let Some(dir) = dir {
        if let Some(found) = search(dir) {
            return Some(found);
        }
    }
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var).find_map(|d| search(&d))
}

/// Builds the supervised process for a site's app: the runtime's bin dir goes first on
/// PATH, `PORT` tells the dev server where to listen, and `.cmd` shims (npm) run via cmd.exe.
pub fn build_app_spec(hostname: &str, app: &AppSpec, port: Option<u16>, bin_dir: Option<&Path>) -> Result<ProcessSpec, String> {
    let found = find_executable(bin_dir, &app.executable).ok_or_else(|| {
        format!("\"{}\" was not found. Install the {} runtime or add it to PATH.", app.executable, app.runtime.as_deref().unwrap_or("required"))
    })?;
    let is_shim = found.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"));
    let (executable, args) = if is_shim {
        let mut args = vec!["/C".to_string(), found.display().to_string()];
        args.extend(app.args.iter().cloned());
        ("cmd.exe".to_string(), args)
    } else {
        (found.display().to_string(), app.args.clone())
    };

    let system_path = std::env::var("PATH").unwrap_or_default();
    let path = match bin_dir {
        Some(d) => format!("{};{system_path}", d.display()),
        None => system_path,
    };
    let mut env = vec![("PATH".to_string(), path)];
    if let Some(p) = port {
        env.push(("PORT".to_string(), p.to_string()));
    }
    Ok(ProcessSpec {
        name: format!("{hostname} app: {} {}", app.executable, app.args.join(" ")),
        executable,
        args,
        cwd: Some(app.cwd.clone()),
        env,
        restart: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_spec_puts_the_runtime_first_on_path_and_sets_port() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("npm.cmd"), "@echo off").unwrap();
        let app = AppSpec { executable: "npm".into(), args: vec!["run".into(), "dev".into()], cwd: "C:/app".into(), runtime: Some("node".into()) };
        let spec = build_app_spec("c.test", &app, Some(5173), Some(dir.path())).unwrap();

        assert_eq!(spec.executable, "cmd.exe", ".cmd shims must run through cmd.exe");
        assert_eq!(spec.args[0], "/C");
        assert!(spec.args.iter().any(|a| a.ends_with("npm.cmd")));
        assert_eq!(&spec.args[spec.args.len() - 2..], ["run", "dev"]);
        assert!(spec.env.iter().any(|(k, v)| k == "PORT" && v == "5173"));
        let path = &spec.env.iter().find(|(k, _)| k == "PATH").unwrap().1;
        assert!(path.starts_with(&dir.path().display().to_string()));
    }

    #[test]
    fn missing_executable_gives_a_helpful_error() {
        let app = AppSpec { executable: "definitely-not-installed-xyz".into(), args: vec![], cwd: ".".into(), runtime: Some("node".into()) };
        let err = build_app_spec("c.test", &app, None, None).unwrap_err();
        assert!(err.contains("was not found"));
    }

    #[test]
    fn restore_puts_files_back_and_removes_new_ones() {
        let dir = tempfile::tempdir().unwrap();
        let existing = dir.path().join("a.conf");
        let created = dir.path().join("b.conf");
        std::fs::write(&existing, "old").unwrap();
        let mut snap = HashMap::new();
        snap.insert(existing.clone(), Some("old".to_string()));
        snap.insert(created.clone(), None);

        std::fs::write(&existing, "new").unwrap();
        std::fs::write(&created, "new").unwrap();
        restore(&snap);

        assert_eq!(std::fs::read_to_string(&existing).unwrap(), "old");
        assert!(!created.exists());
    }
}
