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
    server_by_id, Backend, Invocation, PoolSpec, Ports, ServerLayout, SiteSpec, WebConfig,
    WebServer, SERVER_IDS,
};
use crate::certs::CertificateManager;
use crate::dns::DnsServer;
use crate::domain::{resolved_server, AppSpec, Domain, DomainStore, Ownership, SiteKind};
use crate::error::CoreError;
use crate::exec::run_capture;
use crate::paths::AppPaths;
use crate::php::{PhpPools, PoolStatus};
use crate::port::{check_port, PortStatus};
use crate::process::{ProcessId, ProcessSpec, ProcessSupervisor, RestartPolicy};
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
    /// The web server whose layout this file lives in. Every server has its own main
    /// config, so a file is only identified by hostname + part + server.
    pub server: String,
    pub path: String,
    pub ownership: Option<Ownership>,
    /// The file on disk no longer matches what OpenLocalServer last wrote (§26).
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
    /// The server that owns 80/443 and serves every site with no override.
    pub active: bool,
    /// Whether this server's process is alive right now. Each one runs independently.
    pub running: bool,
    /// Enabled sites this server renders. A running server with none of its own opens
    /// no listener at all, so "running" is all that can be true of it.
    pub sites: usize,
    /// Ports it will actually bind: 80/443 for the default, its own stored pair otherwise.
    pub http_port: u16,
    pub https_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppStatus {
    pub hostname: String,
    pub running: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebStatus {
    /// Server that owns 80/443. Every site without its own override is served by it.
    pub default_server: String,
    pub servers: Vec<ServerAvailability>,
    /// Any web server alive.
    pub running: bool,
    pub http_port: u16,
    pub https_port: u16,
    /// Human-readable reasons a port can't be used (owned by something else).
    pub port_conflicts: Vec<String>,
    pub php_pools: Vec<PoolStatus>,
    pub apps: Vec<AppStatus>,
    pub dns_running: bool,
    pub dns_port: u16,
    /// Error log of the default server; each server's own log is reachable per id.
    pub error_log: Option<String>,
}

struct Running {
    process: ProcessId,
}

struct State {
    /// One entry per server that is running. They hold different ports, so more than one
    /// can be alive at the same time (§3.1).
    servers: HashMap<String, Running>,
    apps: HashMap<String, ProcessId>,
    dns: Option<DnsServer>,
    /// hostname -> the PHP version its requests go to (from the last apply).
    site_php: HashMap<String, String>,
}

/// Which supervised processes do each site's work, for per-site resource usage.
pub struct UsagePlan {
    pub servers: Vec<ProcessId>,
    pub apps: HashMap<String, ProcessId>,
    pub site_php: HashMap<String, String>,
    pub pools: HashMap<String, Vec<ProcessId>>,
}

pub struct WebManager {
    paths: AppPaths,
    runtimes: Arc<RuntimeManager>,
    supervisor: Arc<ProcessSupervisor>,
    certs: Arc<CertificateManager>,
    php: Arc<PhpPools>,
    state: Mutex<State>,
    /// Settings → Resources (§129). Memory limits are per-server program flags; the CPU
    /// limit is applied by wrapping the server in the `cpulimit` utility, which is why it
    /// has to happen here at spawn time rather than being another command-line flag.
    limits: Mutex<crate::resources::ResourceLimits>,
}

fn sha_hex(text: &str) -> String {
    let mut h = Sha256::new();
    h.update(text.as_bytes());
    format!("{:x}", h.finalize())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn werr(msg: impl Into<String>) -> CoreError {
    CoreError::WebError(msg.into())
}

/// Who is sitting on a port, in words a user can act on.
fn port_owner(process_name: Option<String>, pid: Option<u32>) -> String {
    match (process_name, pid) {
        (Some(n), Some(p)) => format!("{n} (PID {p})"),
        (None, Some(p)) => format!("PID {p}"),
        _ => "another program".to_string(),
    }
}

/// A generated-config hash to record once an apply has written and validated the files:
/// `(server id, hostname, sha-256 of what is now on disk)`. The caller commits these, so
/// the apply itself never needs the domain store.
pub type HashUpdate = (String, String, String);

/// The enabled sites one server renders. A site is served by exactly one server: its own
/// `server` override, or the default when it has none.
pub fn sites_for_server(cfg: &WebConfig, domains: &[Domain], server_id: &str) -> Vec<Domain> {
    domains
        .iter()
        .filter(|d| d.enabled && resolved_server(d, cfg) == server_id)
        .cloned()
        .collect()
}

/// Every PHP version any enabled site needs, across *all* servers, not just one.
///
/// `apply` renders the servers one after another and they share a single set of FastCGI
/// pools, so a per-server keep set is wrong: Apache's pass would retain only Apache's
/// versions and stop the pool nginx's sites were still pointing at, which left those sites
/// answering `connect() failed (10061) ... while connecting to upstream` against a port
/// nothing was listening on any more.
fn php_versions_across_servers(
    ctx: &ApplyContext,
    domains: &[Domain],
    pick: &dyn Fn(Option<&str>) -> Option<String>,
) -> Vec<String> {
    let mut versions: Vec<String> = Vec::new();
    for d in domains.iter().filter(|d| d.enabled) {
        let SiteKind::Php { version } = effective_kind(d) else {
            continue;
        };
        let wanted = version.or_else(|| (ctx.php_for)(d));
        if let Some(picked) = pick(wanted.as_deref()) {
            if !versions.contains(&picked) {
                versions.push(picked);
            }
        }
    }
    versions
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
            state: Mutex::new(State {
                servers: HashMap::new(),
                apps: HashMap::new(),
                dns: None,
                site_php: HashMap::new(),
                pools: HashMap::new(),
            }),
            limits: Mutex::new(Default::default()),
        }
    }

    /// The CPU cap from Settings → Resources, applied to the next server start.
    pub fn set_limits(&self, limits: crate::resources::ResourceLimits) {
        *self.limits.lock().unwrap() = limits;
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
            .ok_or_else(|| {
                werr(format!(
                    "{} is not installed. Install it from the Runtimes page first.",
                    server.name()
                ))
            })?;
        let binary = self.runtimes.binary_path(id, &version).ok_or_else(|| {
            werr(format!(
                "{} {version} is missing its executable",
                server.name()
            ))
        })?;
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
        self.paths
            .web_dir()
            .join(server_id)
            .join("history")
            .join(hostname)
    }

    /// Where a config file lives on disk.
    fn file_path(
        &self,
        server: &dyn WebServer,
        layout: &ServerLayout,
        hostname: Option<&str>,
        part: ConfigPart,
    ) -> Result<PathBuf, CoreError> {
        match (part, hostname) {
            (ConfigPart::Main, _) => Ok(server.main_config(layout)),
            (ConfigPart::Site, Some(h)) => Ok(layout.site_file(server.config_ext(), h)),
            (ConfigPart::Custom, Some(h)) => Ok(layout.custom_file(server.config_ext(), h)),
            _ => Err(werr("a hostname is required for site config files")),
        }
    }

    // ---------------------------------------------------------------- status

    pub fn status(&self, cfg: &WebConfig, domains: &[Domain]) -> WebStatus {
        let st = self.state.lock().unwrap();
        let servers = SERVER_IDS
            .iter()
            .filter_map(|id| server_by_id(id))
            .map(|s| {
                let ports = cfg.effective_ports(s.id());
                ServerAvailability {
                    id: s.id().to_string(),
                    name: s.name().to_string(),
                    installed: !self.runtimes.installed_versions(s.id()).is_empty(),
                    active: s.id() == cfg.default_server,
                    running: st
                        .servers
                        .get(s.id())
                        .is_some_and(|r| self.supervisor.is_alive(r.process)),
                    sites: sites_for_server(cfg, domains, s.id()).len(),
                    http_port: ports.http,
                    https_port: ports.https,
                }
            })
            .collect();

        let running = st
            .servers
            .values()
            .any(|r| self.supervisor.is_alive(r.process));
        let mut port_conflicts = Vec::new();
        // Two servers pointed at the same port can never both come up: say so before
        // either is asked to start.
        port_conflicts.extend(cfg.port_conflicts());
        for (label, port) in [("HTTP", cfg.http_port()), ("HTTPS", cfg.https_port())] {
            // A port the default server is already using isn't a conflict with itself.
            if st
                .servers
                .get(&cfg.default_server)
                .is_some_and(|r| self.supervisor.is_alive(r.process))
            {
                continue;
            }
            if let PortStatus::InUse { pid, process_name } = check_port(port) {
                port_conflicts.push(format!(
                    "{label} port {port} is in use by {}",
                    port_owner(process_name, pid)
                ));
            }
        }
        let apps = st
            .apps
            .iter()
            .map(|(h, id)| AppStatus {
                hostname: h.clone(),
                running: self.supervisor.is_alive(*id),
            })
            .collect();
        drop(st);
        let error_log = self.error_log_path(&cfg.default_server);

        WebStatus {
            default_server: cfg.default_server.clone(),
            servers,
            running,
            http_port: cfg.http_port(),
            https_port: cfg.https_port(),
            port_conflicts,
            php_pools: self.php.status(),
            apps,
            dns_running: self.state.lock().unwrap().dns.is_some(),
            dns_port: cfg.dns_port,
            error_log,
        }
    }

    /// Where a server writes its error log, if it is installed.
    pub fn error_log_path(&self, id: &str) -> Option<String> {
        let server = server_by_id(id)?;
        let layout = self.layout(server.as_ref()).ok()?;
        Some(server.error_log(&layout).display().to_string())
    }

    pub fn usage_plan(&self) -> UsagePlan {
        let st = self.state.lock().unwrap();
        UsagePlan {
            servers: st.servers.values().map(|r| r.process).collect(),
            apps: st.apps.clone(),
            site_php: st.site_php.clone(),
            pools: self.php.pool_processes(),
        }
    }

    /// Is any web server alive?
    pub fn is_running(&self) -> bool {
        let st = self.state.lock().unwrap();
        st.servers
            .values()
            .any(|r| self.supervisor.is_alive(r.process))
    }

    /// Is this one server alive? Each starts and stops on its own (§3.2).
    pub fn is_running_id(&self, id: &str) -> bool {
        let st = self.state.lock().unwrap();
        st.servers
            .get(id)
            .is_some_and(|r| self.supervisor.is_alive(r.process))
    }

    /// Stops one web server, leaving every other one running.
    pub fn stop_server(&self, id: &str) {
        let running = self.state.lock().unwrap().servers.remove(id);
        if let Some(r) = running {
            self.supervisor.stop(r.process);
        }
    }

    /// The supervised process of a running server, for callers that need its id.
    pub fn running_pid(&self, id: &str) -> Option<ProcessId> {
        self.state
            .lock()
            .unwrap()
            .servers
            .get(id)
            .map(|r| r.process)
    }

    // ---------------------------------------------------------------- apply (§28)

    /// Which servers an apply touches: the default one always, plus every installed
    /// server the user has explicitly started. A server that was never started renders
    /// nothing, so its config on disk is left exactly as it was.
    fn apply_targets(&self, cfg: &WebConfig) -> Vec<String> {
        let mut targets = vec![cfg.default_server.clone()];
        for id in SERVER_IDS {
            if *id != cfg.default_server
                && self.is_running_id(id)
                && !self.runtimes.installed_versions(id).is_empty()
            {
                targets.push((*id).to_string());
            }
        }
        targets
    }

    /// Renders, validates and starts every server that should be serving, one after the
    /// other. Each server's rollback is its own, so a bad config for one never touches
    /// the others.
    ///
    /// Works from a snapshot of the domains rather than the live store: writing files,
    /// running the validator and (re)starting a server can take seconds, and holding the
    /// store lock that long would block every read — including the UI's status poll. The
    /// hashes to record come back in `hashes` for the caller to commit once the lock is
    /// free again. A change made while an apply runs is picked up by the next one.
    pub fn apply(
        &self,
        ctx: &ApplyContext,
        domains: &[Domain],
        hashes: &mut Vec<HashUpdate>,
    ) -> Result<Vec<ApplyReport>, CoreError> {
        let mut out = Vec::new();
        for id in self.apply_targets(ctx.cfg) {
            out.push(self.apply_one(ctx, domains, &id, hashes)?);
        }
        self.finish_global(ctx, domains, &mut out);
        Ok(out)
    }

    /// §28 for one server, rendering only the sites assigned to it.
    pub fn apply_one(
        &self,
        ctx: &ApplyContext,
        domains: &[Domain],
        server_id: &str,
        hashes: &mut Vec<HashUpdate>,
    ) -> Result<ApplyReport, CoreError> {
        let cfg = ctx.cfg;
        let server = server_by_id(server_id)
            .ok_or_else(|| werr(format!("unknown web server \"{server_id}\"")))?;
        let layout = self.layout(server.as_ref())?;
        server
            .prepare(&layout)
            .map_err(|e| werr(format!("could not prepare {}: {e}", server.name())))?;
        let effective = cfg.effective_ports(server.id());
        let ports = Ports {
            http: effective.http,
            https: effective.https,
        };
        let mut report = ApplyReport {
            server: server.id().to_string(),
            ..Default::default()
        };

        let all = domains;
        // Exclusive binding: a site is rendered on its assigned server and no other, so
        // the files of the other servers for that host are removed below.
        let enabled = sites_for_server(cfg, all, server.id());
        for d in &enabled {
            if let Some(want) = d.server.as_deref() {
                if !SERVER_IDS.contains(&want) {
                    report.warnings.push(format!(
                        "{} asks for web server \"{want}\", which is not one we ship; \
                         it is being served by {} instead.",
                        d.hostname,
                        server.id()
                    ));
                }
            }
        }

        // 1. PHP pools for every version a PHP site needs.
        let mut pool_specs: BTreeMap<String, PoolSpec> = BTreeMap::new();
        let mut site_pools: HashMap<String, (String, Vec<u16>)> = HashMap::new();
        let mut site_php: HashMap<String, String> = HashMap::new();
        for d in &enabled {
            let SiteKind::Php { version } = effective_kind(d) else {
                continue;
            };
            let wanted = version.or_else(|| (ctx.php_for)(d));
            let picked = self.php.pick_version(wanted.as_deref()).ok_or_else(|| {
                werr(match &wanted {
                    Some(v) => format!("{} needs PHP {v}, which is not installed. Install it from the Runtimes page.", d.hostname),
                    None => format!("{} is a PHP site but no PHP is installed. Install one from the Runtimes page.", d.hostname),
                })
            })?;
            let ports_for_version = self.php.ensure(&picked, cfg.php_workers).map_err(werr)?;
            let pool_id = PhpPools::pool_id(&picked);
            pool_specs.entry(pool_id.clone()).or_insert(PoolSpec {
                id: pool_id.clone(),
                ports: ports_for_version.clone(),
            });
            site_pools.insert(d.hostname.clone(), (pool_id, ports_for_version));
            site_php.insert(d.hostname.clone(), picked.clone());
        }
        self.php
            .retain(&php_versions_across_servers(ctx, &all, &|v| {
                self.php.pick_version(v)
            }));
        self.state.lock().unwrap().site_php = site_php;

        // 2. Certificates, then the rendered site files.
        let mut rendered: Vec<(Domain, String)> = Vec::new();
        for d in &enabled {
            let tls = if d.https {
                Some(self.certs.ensure_for(d).map_err(werr)?)
            } else {
                None
            };
            let backend = match &effective_kind(d) {
                SiteKind::Php { .. } => {
                    let (pool, ports) = site_pools[&d.hostname].clone();
                    Backend::Php { pool, ports }
                }
                kind @ SiteKind::Proxy { .. } => Backend::Proxy {
                    upstream: kind.upstream_url().unwrap_or_default(),
                },
                SiteKind::Static => Backend::Static,
            };
            let custom_snippet = (d.ownership == Ownership::Advanced)
                .then(|| layout.custom_file(server.config_ext(), &d.hostname));
            let spec = SiteSpec {
                hostname: d.hostname.clone(),
                wildcard: d.wildcard,
                root: d.root.clone(),
                backend,
                tls,
                redirect_https: d.https && d.redirect_https,
                blocks: d.blocks.clone(),
                custom_snippet,
                public_domain: d.public_domain.clone(),
                forwarded_tls: false,
            };
            rendered.push((d.clone(), server.render_site(&spec, ports)));
        }
        let pools_vec: Vec<PoolSpec> = pool_specs.into_values().collect();
        let main_text = server.render_main(&layout, ports, &pools_vec);

        // 3. Snapshot what's on disk (for rollback), then write.
        let ext = server.config_ext();
        let mut snapshot: HashMap<PathBuf, Option<String>> = HashMap::new();
        let mut remember = |p: &Path| {
            snapshot
                .entry(p.to_path_buf())
                .or_insert_with(|| std::fs::read_to_string(p).ok());
        };

        let main_path = server.main_config(&layout);
        remember(&main_path);
        let mut new_hashes: Vec<(String, String)> = Vec::new();
        let mut plan: Vec<(PathBuf, String, Option<String>)> =
            vec![(main_path.clone(), main_text, None)];

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
                    plan.push((
                        custom,
                        format!(
                            "# Your own {} directives for {}.\n",
                            server.name(),
                            d.hostname
                        ),
                        None,
                    ));
                }
            }
        }

        // Stale site files (deleted or disabled domains) are removed, archived first.
        let keep: Vec<String> = enabled
            .iter()
            .map(|d| format!("{}.{ext}", d.hostname))
            .collect();
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
                if let (Some(stem), Ok(old)) = (
                    path.file_name().and_then(|n| n.to_str()),
                    std::fs::read_to_string(path),
                ) {
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

        // Config is good: hand back the hashes so future hand-edits show up as drift.
        for (host, hash) in new_hashes {
            hashes.push((server.id().to_string(), host, hash));
        }

        // 5. Start (or gracefully reload). A server with no sites of its own opens no
        // listener, so it is not waited on by port — only checked for staying alive.
        self.start_or_reload(
            server.as_ref(),
            &layout,
            ports,
            !enabled.is_empty(),
            &mut report,
        )?;

        Ok(report)
    }

    /// Steps that are about the machine, not one server: site app processes and name
    /// resolution run once for every enabled site, whichever server renders it (§3.2).
    fn finish_global(&self, ctx: &ApplyContext, domains: &[Domain], reports: &mut [ApplyReport]) {
        let enabled: Vec<Domain> = domains.iter().filter(|d| d.enabled).cloned().collect();
        let Some(report) = reports.last_mut() else {
            return;
        };
        self.sync_apps(ctx, &enabled, report);

        // Name resolution. Failures here don't invalidate any server config. Our local
        // DNS answers for whole reserved TLDs (.test), so those names never need the
        // admin-only hosts file; only names outside them are written there.
        let covered = self.sync_dns(ctx.cfg, &enabled, report);
        let mut hostnames: Vec<String> = enabled
            .iter()
            .map(|d| d.hostname.clone())
            .filter(|h| !covered.contains(&tld_of(h)))
            .collect();
        hostnames.sort();
        if !hostnames.is_empty() {
            match crate::hosts::ensure(&hostnames) {
                Ok(changed) => report.hosts_updated = changed,
                Err(e) => report
                    .warnings
                    .push(format!("The hosts file was not updated: {e}")),
            }
        }
    }

    fn run_invocation(&self, layout: &ServerLayout, inv: &Invocation) -> crate::exec::Captured {
        run_capture(
            &layout.binary,
            &inv.args,
            Some(&inv.cwd),
            &[],
            Duration::from_secs(30),
        )
    }

    fn start_or_reload(
        &self,
        server: &dyn WebServer,
        layout: &ServerLayout,
        ports: Ports,
        expect_listener: bool,
        report: &mut ApplyReport,
    ) -> Result<(), CoreError> {
        let alive = {
            let st = self.state.lock().unwrap();
            st.servers
                .get(server.id())
                .filter(|r| self.supervisor.is_alive(r.process))
                .map(|r| r.process)
        };

        if let Some(process) = alive {
            if let Some(inv) = server.reload(layout) {
                let out = self.run_invocation(layout, &inv);
                if out.success() {
                    report.reloaded = true;
                    return Ok(());
                }
                report.warnings.push(format!(
                    "Reload failed, restarting instead: {}",
                    out.combined()
                ));
            }
            // No graceful reload (Apache in foreground mode) or it failed: restart.
            self.supervisor.stop(process);
            self.state.lock().unwrap().servers.remove(server.id());
            wait_port_free_all(Duration::from_secs(4));
        }

        // Ports must be ours to take.
        for (label, port) in [("HTTP", ports.http), ("HTTPS", ports.https)] {
            if let PortStatus::InUse { pid, process_name } = check_port(port) {
                return Err(werr(format!(
                    "{label} port {port} is already in use by {}. \
                     Stop it, or give {} different ports in the web settings.",
                    port_owner(process_name, pid),
                    server.name()
                )));
            }
        }

        let inv = server.start(layout);
        let executable = layout.binary.display().to_string();
        // A wanted CPU cap whose utility is missing must not turn into an uncapped server
        // that still reads as limited, so this returns the same refusal ServiceManager does.
        let capped = self
            .limits
            .lock()
            .unwrap()
            .cap_process(&executable, &inv.args)
            .map_err(werr)?;
        let (executable, args) = capped.unwrap_or((executable, inv.args));
        let process = self.supervisor.start(ProcessSpec {
            name: server.name().to_string(),
            executable,
            args,
            cwd: Some(inv.cwd.display().to_string()),
            env: vec![],
            restart: None,
        });
        if let Err(msg) = self.await_start(process, ports.http, expect_listener) {
            self.supervisor.stop(process);
            // Give the OS the port back before reporting, or the next start would see
            // our own dying process as the owner of it.
            wait_port_free_all(Duration::from_secs(3));
            let log_tail = tail(&server.error_log(layout), 8);
            return Err(werr(format!(
                "{} did not start: {msg}{log_tail}",
                server.name()
            )));
        }
        self.state
            .lock()
            .unwrap()
            .servers
            .insert(server.id().to_string(), Running { process });
        report.started = true;
        Ok(())
    }

    /// A freshly started server is up when its HTTP port answers — unless it was given
    /// no sites at all, in which case it opens no listener and all we can check is that
    /// it did not fall over straight away (§3.2: servers run side by side).
    fn await_start(
        &self,
        process: ProcessId,
        port: u16,
        expect_listener: bool,
    ) -> Result<(), String> {
        if expect_listener {
            return self.wait_ready(process, port, Duration::from_secs(10));
        }
        let deadline = Instant::now() + Duration::from_millis(1500);
        while Instant::now() < deadline {
            if !self.supervisor.is_alive(process) {
                return Err(self.exit_reason(process));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Ok(())
    }

    /// The tail of a process's output, for "it died on the way up".
    fn exit_reason(&self, process: ProcessId) -> String {
        let out = self.supervisor.recent_output(process);
        let text = out
            .iter()
            .rev()
            .take(6)
            .rev()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        if text.is_empty() {
            "the process exited immediately".into()
        } else {
            text
        }
    }

    fn wait_ready(&self, process: ProcessId, port: u16, timeout: Duration) -> Result<(), String> {
        let started = Instant::now();
        loop {
            if !self.supervisor.is_alive(process) {
                return Err(self.exit_reason(process));
            }
            if TcpStream::connect_timeout(
                &([127, 0, 0, 1], port).into(),
                Duration::from_millis(200),
            )
            .is_ok()
            {
                return Ok(());
            }
            if started.elapsed() > timeout {
                return Err(format!(
                    "nothing answered on port {port} within {}s",
                    timeout.as_secs()
                ));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// Stops every web server at once (shutdown).
    pub fn stop(&self) {
        let mut st = self.state.lock().unwrap();
        for (_, r) in st.servers.drain() {
            self.supervisor.stop(r.process);
        }
        for (_, id) in st.apps.drain() {
            self.supervisor.stop(id);
        }
        st.dns = None;
        drop(st);
        self.php.stop_all();
    }

    /// Starts one server on its own, without touching the others. The config for the
    /// sites assigned to it is rendered by the caller's apply.
    pub fn start_server(&self, id: &str, cfg: &WebConfig) -> Result<(), CoreError> {
        let server =
            server_by_id(id).ok_or_else(|| werr(format!("unknown web server \"{id}\"")))?;
        let layout = self.layout(server.as_ref())?;
        server
            .prepare(&layout)
            .map_err(|e| werr(format!("could not prepare {}: {e}", server.name())))?;
        let effective = cfg.effective_ports(id);
        let mut report = ApplyReport {
            server: id.to_string(),
            ..Default::default()
        };
        // Starting on its own: the port pre-check still guards the bind, but there is
        // no way to know from here whether this server has sites yet, so liveness is
        // judged by the process rather than by a listener.
        self.start_or_reload(
            server.as_ref(),
            &layout,
            Ports {
                http: effective.http,
                https: effective.https,
            },
            false,
            &mut report,
        )
    }

    /// Re-runs one server's config check against what's on disk, without changing anything.
    pub fn validate(&self, cfg: &WebConfig) -> Result<String, CoreError> {
        self.validate_server(&cfg.default_server)
    }

    pub fn validate_server(&self, id: &str) -> Result<String, CoreError> {
        let server = server_by_id(id).ok_or_else(|| werr("unknown web server"))?;
        let layout = self.layout(server.as_ref())?;
        let out = self.run_invocation(&layout, &server.validate(&layout));
        if out.success() {
            Ok(out.combined())
        } else {
            Err(werr(format!(
                "{} rejected the config on disk:\n{}",
                server.name(),
                out.combined()
            )))
        }
    }

    // ---------------------------------------------------------------- apps

    fn sync_apps(&self, ctx: &ApplyContext, enabled: &[Domain], report: &mut ApplyReport) {
        let mut st = self.state.lock().unwrap();
        let wanted: Vec<&Domain> = enabled.iter().filter(|d| d.app.is_some()).collect();

        // Stop apps whose site was removed/disabled or lost its app command.
        let stale: Vec<String> = st
            .apps
            .keys()
            .filter(|h| !wanted.iter().any(|d| &d.hostname == *h))
            .cloned()
            .collect();
        for host in stale {
            if let Some(id) = st.apps.remove(&host) {
                self.supervisor.stop(id);
            }
        }

        for d in wanted {
            if st
                .apps
                .get(&d.hostname)
                .is_some_and(|id| self.supervisor.is_alive(*id))
            {
                continue;
            }
            let app = d.app.as_ref().expect("filtered above");
            let port = match d.kind {
                SiteKind::Proxy { upstream_port, .. } => Some(upstream_port),
                _ => None,
            };
            let bin_dir = app
                .runtime
                .as_deref()
                .and_then(|rt| (ctx.runtime_bin)(d, rt));
            match build_app_spec(&d.hostname, app, port, bin_dir.as_deref()) {
                Ok(spec) => {
                    let id = self.supervisor.start(spec);
                    st.apps.insert(d.hostname.clone(), id);
                }
                Err(e) => report
                    .warnings
                    .push(format!("{}: could not start the app: {e}", d.hostname)),
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

    /// Local DNS (§46). The resolver answers every name under the reserved TLDs our domains
    /// use (`.test`, ...) plus wildcard subdomains, and one Windows NRPT rule per suffix
    /// routes those lookups to it. A TLD rule costs a single UAC prompt, ever: after that,
    /// adding or deleting `.test` domains needs no administrator rights at all. TLD rules
    /// are never removed automatically (that would prompt again, and routing a reserved TLD
    /// to localhost is harmless). Returns the TLDs whose names are fully handled here.
    fn sync_dns(
        &self,
        cfg: &WebConfig,
        enabled: &[Domain],
        report: &mut ApplyReport,
    ) -> Vec<String> {
        let mut tlds: Vec<String> = enabled
            .iter()
            .map(|d| tld_of(&d.hostname))
            .filter(|t| SAFE_TLDS.contains(&t.as_str()))
            .collect();
        tlds.sort();
        tlds.dedup();
        // Wildcards under a TLD we already answer for need nothing extra.
        let wildcards: Vec<String> = enabled
            .iter()
            .filter(|d| d.wildcard && !tlds.contains(&tld_of(&d.hostname)))
            .map(|d| d.hostname.clone())
            .collect();
        let suffixes: Vec<String> = tlds.iter().chain(&wildcards).cloned().collect();

        let mut st = self.state.lock().unwrap();
        if suffixes.is_empty() {
            st.dns = None;
        } else if let Some(dns) = &st.dns {
            dns.set_suffixes(suffixes.clone());
        } else {
            match DnsServer::start(cfg.dns_port, suffixes.clone()) {
                Ok(dns) => st.dns = Some(dns),
                Err(e) => {
                    report.warnings.push(format!(
                        "Local DNS is not running ({e}); using the hosts file instead."
                    ));
                    return Vec::new();
                }
            }
        }
        drop(st);

        // NRPT rules can only target port 53, and tests use a redirected hosts file.
        if cfg.dns_port != 53 || std::env::var("OLS_HOSTS_FILE").is_ok() {
            return Vec::new();
        }
        let mut installed = self.nrpt_rules();
        let before = installed.clone();
        for suffix in &suffixes {
            if !installed.contains(suffix) {
                match crate::elevate::run_helper(&[
                    "nrpt-add".into(),
                    format!(".{suffix}"),
                    "127.0.0.1".into(),
                ]) {
                    Ok(()) => installed.push(suffix.clone()),
                    Err(e) => report
                        .warnings
                        .push(format!("Windows DNS rule for .{suffix} was not added: {e}")),
                }
            }
        }
        // Rules for single wildcard sites outside our TLDs point real-world names at this
        // machine, so those are removed when their site goes.
        let stale: Vec<String> = installed
            .iter()
            .filter(|s| !suffixes.contains(s) && !SAFE_TLDS.contains(&s.as_str()))
            .cloned()
            .collect();
        for suffix in stale {
            if crate::elevate::run_helper(&["nrpt-remove".into(), format!(".{suffix}")]).is_ok() {
                installed.retain(|s| *s != suffix);
            }
        }
        if installed != before {
            let _ = std::fs::write(
                self.nrpt_file(),
                serde_json::to_string_pretty(&installed).unwrap_or_default(),
            );
        }
        tlds.into_iter().filter(|t| installed.contains(t)).collect()
    }

    /// Moves a site's own files (hand-edited Manual file, Advanced snippet, history) to its
    /// new name, for every server. Generated files are simply rewritten on the next apply.
    pub fn rename_site_files(&self, old: &str, new: &str) {
        for id in SERVER_IDS {
            let Some(server) = server_by_id(id) else {
                continue;
            };
            let root = self.paths.web_dir().join(id);
            let ext = server.config_ext();
            for dir in ["sites", "custom"] {
                let from = root.join(dir).join(format!("{old}.{ext}"));
                if from.is_file() {
                    let _ = std::fs::rename(&from, root.join(dir).join(format!("{new}.{ext}")));
                }
            }
            let history = self.history_dir(id, old);
            if history.is_dir() {
                let _ = std::fs::rename(&history, self.history_dir(id, new));
            }
        }
    }

    fn nrpt_file(&self) -> PathBuf {
        self.paths.web_dir().join("nrpt.json")
    }

    fn nrpt_rules(&self) -> Vec<String> {
        std::fs::read_to_string(self.nrpt_file())
            .ok()
            .and_then(|r| serde_json::from_str(&r).ok())
            .unwrap_or_default()
    }

    /// Whether `hostname` resolves through our local DNS right now (no hosts entry needed).
    pub fn dns_covers(&self, hostname: &str) -> bool {
        let tld = tld_of(hostname);
        self.state.lock().unwrap().dns.is_some()
            && SAFE_TLDS.contains(&tld.as_str())
            && self.nrpt_rules().contains(&tld)
    }

    // ---------------------------------------------------------------- config files (§25–29)

    /// Config files for one server. `server_id` is resolved from the site's own override,
    /// so a site's files are listed under the server that actually renders it.
    pub fn list_configs(
        &self,
        cfg: &WebConfig,
        domains: &DomainStore,
        server_id: &str,
    ) -> Result<Vec<ConfigFile>, CoreError> {
        let server = server_by_id(server_id)
            .ok_or_else(|| werr(format!("unknown web server \"{server_id}\"")))?;
        let layout = self.layout(server.as_ref())?;
        let mut files = vec![ConfigFile {
            hostname: None,
            part: ConfigPart::Main,
            server: server.id().into(),
            path: server.main_config(&layout).display().to_string(),
            ownership: None,
            drifted: false,
            editable: false,
        }];
        for d in domains.list() {
            if resolved_server(&d, cfg) != server.id() {
                continue;
            }
            let path = layout.site_file(server.config_ext(), &d.hostname);
            let drifted = std::fs::read_to_string(&path)
                .ok()
                .zip(d.generated_hashes.get(server.id()))
                .is_some_and(|(text, h)| d.ownership != Ownership::Manual && sha_hex(&text) != *h);
            files.push(ConfigFile {
                hostname: Some(d.hostname.clone()),
                part: ConfigPart::Site,
                server: server.id().into(),
                path: path.display().to_string(),
                ownership: Some(d.ownership),
                drifted,
                editable: d.ownership == Ownership::Manual,
            });
            if d.ownership == Ownership::Advanced {
                files.push(ConfigFile {
                    hostname: Some(d.hostname.clone()),
                    part: ConfigPart::Custom,
                    server: server.id().into(),
                    path: layout
                        .custom_file(server.config_ext(), &d.hostname)
                        .display()
                        .to_string(),
                    ownership: Some(d.ownership),
                    drifted: false,
                    editable: true,
                });
            }
        }
        Ok(files)
    }

    pub fn read_config(
        &self,
        server_id: &str,
        hostname: Option<&str>,
        part: ConfigPart,
    ) -> Result<String, CoreError> {
        let server = server_by_id(server_id)
            .ok_or_else(|| werr(format!("unknown web server \"{server_id}\"")))?;
        let layout = self.layout(server.as_ref())?;
        let path = self.file_path(server.as_ref(), &layout, hostname, part)?;
        std::fs::read_to_string(&path)
            .map_err(|e| werr(format!("could not read {}: {e}", path.display())))
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
        let domain = domains
            .get(hostname)
            .ok_or_else(|| werr(format!("{hostname} is not a known domain")))?;
        let server_id = resolved_server(&domain, cfg);
        let server = server_by_id(&server_id)
            .ok_or_else(|| werr(format!("unknown web server \"{server_id}\"")))?;
        let layout = self.layout(server.as_ref())?;

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
        write_atomic(&path, content)
            .map_err(|e| werr(format!("could not write {}: {e}", path.display())))?;

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
            d.generated_hashes
                .insert(server.id().to_string(), sha_hex(content));
            let _ = domains.update(d);
        }

        let mut report = ApplyReport::default();
        if self.is_running_id(server.id()) {
            let effective = cfg.effective_ports(server.id());
            // An edited file with no site of its own still has no listener; the reload
            // is the whole check there.
            let has_sites = domains
                .list()
                .iter()
                .any(|d| d.enabled && resolved_server(d, cfg) == server.id());
            self.start_or_reload(
                server.as_ref(),
                &layout,
                Ports {
                    http: effective.http,
                    https: effective.https,
                },
                has_sites,
                &mut report,
            )?;
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
        let mut d = domains
            .get(hostname)
            .ok_or_else(|| werr(format!("{hostname} is not a known domain")))?;
        if d.ownership == ownership {
            return Ok(());
        }
        let server_id = resolved_server(&d, cfg);
        if let Some(server) = server_by_id(&server_id) {
            if let Ok(layout) = self.layout(server.as_ref()) {
                let ext = server.config_ext();
                let site = layout.site_file(ext, hostname);
                // Whatever the file held is about to stop being OpenLocalServer's to regenerate —
                // or is about to be regenerated over. Either way keep a copy.
                if let Ok(old) = std::fs::read_to_string(&site) {
                    self.archive(server.id(), hostname, ConfigPart::Site, ext, &old);
                    if ownership == Ownership::Manual {
                        // The current generated content becomes the user's starting point.
                        d.generated_hashes
                            .insert(server.id().to_string(), sha_hex(&old));
                    }
                }
                if ownership == Ownership::Advanced {
                    let custom = layout.custom_file(ext, hostname);
                    if !custom.exists() {
                        std::fs::create_dir_all(&layout.custom_dir)
                            .map_err(|e| werr(e.to_string()))?;
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
        if let Some(latest) = self
            .list_history_raw(server_id, hostname)
            .into_iter()
            .max_by_key(|v| v.timestamp_ms)
        {
            let latest_path = dir.join(format!("{}.{ext}", latest.id));
            if latest.part == part
                && std::fs::read_to_string(latest_path).ok().as_deref() == Some(content)
            {
                return;
            }
        }
        let tag = if part == ConfigPart::Custom {
            "custom"
        } else {
            "site"
        };
        let mut ts = now_ms();
        // Two archives within one millisecond must not collide.
        while dir.join(format!("{ts}-{tag}.{ext}")).exists() {
            ts += 1;
        }
        let _ = std::fs::write(dir.join(format!("{ts}-{tag}.{ext}")), content);
    }

    fn list_history_raw(&self, server_id: &str, hostname: &str) -> Vec<ConfigVersion> {
        let Ok(entries) = std::fs::read_dir(self.history_dir(server_id, hostname)) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for e in entries.flatten() {
            let path = e.path();
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let Some((ts, tag)) = stem.split_once('-') else {
                continue;
            };
            let Ok(timestamp_ms) = ts.parse::<u64>() else {
                continue;
            };
            let part = if tag == "custom" {
                ConfigPart::Custom
            } else {
                ConfigPart::Site
            };
            out.push(ConfigVersion {
                id: stem.to_string(),
                part,
                timestamp_ms,
                bytes: e.metadata().map(|m| m.len()).unwrap_or(0),
            });
        }
        out
    }

    /// Saved versions of a site's config, under the server that renders the site.
    pub fn list_history(
        &self,
        cfg: &WebConfig,
        domains: &DomainStore,
        hostname: &str,
    ) -> Vec<ConfigVersion> {
        let server_id = self.server_of(cfg, domains, hostname);
        let mut v = self.list_history_raw(&server_id, hostname);
        v.sort_by(|a, b| b.timestamp_ms.cmp(&a.timestamp_ms));
        v
    }

    /// Which server a hostname's config lives under: its own override, or the default.
    fn server_of(&self, cfg: &WebConfig, domains: &DomainStore, hostname: &str) -> String {
        domains
            .get(hostname)
            .map(|d| resolved_server(&d, cfg))
            .unwrap_or_else(|| cfg.default_server.clone())
    }

    pub fn read_history(
        &self,
        cfg: &WebConfig,
        domains: &DomainStore,
        hostname: &str,
        id: &str,
    ) -> Result<String, CoreError> {
        let server_id = self.server_of(cfg, domains, hostname);
        let server = server_by_id(&server_id)
            .ok_or_else(|| werr(format!("unknown web server \"{server_id}\"")))?;
        if id.contains(['/', '\\', '.']) {
            return Err(werr("invalid history id"));
        }
        let path = self
            .history_dir(server.id(), hostname)
            .join(format!("{id}.{}", server.config_ext()));
        std::fs::read_to_string(&path)
            .map_err(|e| werr(format!("could not read that version: {e}")))
    }

    /// Restores an old version through the normal validated write path.
    pub fn restore_history(
        &self,
        cfg: &WebConfig,
        domains: &mut DomainStore,
        hostname: &str,
        id: &str,
    ) -> Result<String, CoreError> {
        let content = self.read_history(cfg, domains, hostname, id)?;
        let part = if id.ends_with("-custom") {
            ConfigPart::Custom
        } else {
            ConfigPart::Site
        };
        self.write_config(cfg, domains, hostname, part, &content)
    }

    /// Copies a config file (or a history version) to `dest` so it can be shared or diffed elsewhere.
    pub fn export_config(
        &self,
        cfg: &WebConfig,
        domains: &DomainStore,
        hostname: &str,
        part: ConfigPart,
        dest: &str,
    ) -> Result<(), CoreError> {
        let server_id = self.server_of(cfg, domains, hostname);
        let text = self.read_config(&server_id, Some(hostname), part)?;
        std::fs::write(dest, text).map_err(|e| werr(format!("could not export: {e}")))
    }

    /// A server's error log, last `lines` lines, for the Logs page. `server_id` of `None`
    /// means the default server.
    pub fn error_log_tail(
        &self,
        cfg: &WebConfig,
        server_id: Option<&str>,
        lines: usize,
    ) -> Vec<String> {
        let id = server_id.unwrap_or(&cfg.default_server);
        let Some(server) = server_by_id(id) else {
            return Vec::new();
        };
        let Ok(layout) = self.layout(server.as_ref()) else {
            return Vec::new();
        };
        read_tail(&server.error_log(&layout), lines)
    }
}

fn read_tail(path: &Path, lines: usize) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..]
        .iter()
        .map(|s| s.to_string())
        .collect()
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
    // Windows cannot spawn a bare Unix shell script (e.g. Node's extensionless `npx`/`npm`)
    // via CreateProcess — that surfaces as os error 193. Prefer native launchers first so
    // `shim()` routes `.cmd`/`.bat` through `cmd.exe /C`. Bare names stay last as fallback.
    #[cfg(windows)]
    let candidates = [
        format!("{exe}.exe"),
        format!("{exe}.cmd"),
        format!("{exe}.bat"),
        exe.to_string(),
    ];
    #[cfg(not(windows))]
    let candidates = [
        exe.to_string(),
        format!("{exe}.exe"),
        format!("{exe}.cmd"),
        format!("{exe}.bat"),
    ];
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
pub fn build_app_spec(
    hostname: &str,
    app: &AppSpec,
    port: Option<u16>,
    bin_dir: Option<&Path>,
) -> Result<ProcessSpec, String> {
    let found = find_executable(bin_dir, &app.executable).ok_or_else(|| {
        format!(
            "\"{}\" was not found. Install the {} runtime or add it to PATH.",
            app.executable,
            app.runtime.as_deref().unwrap_or("required")
        )
    })?;
    let is_shim = found
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"));
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
        // A dev server that dies takes its site down with it: the web server keeps serving
        // the `proxy_pass` upstream and answers every request with 502 until something
        // respawns it. `sync_apps` only runs on a config apply, so without a restart policy
        // a single crash (bad build, port taken, OOM) meant a dead site until the user
        // noticed and pressed restart. Bounded like every other managed process.
        restart: Some(RestartPolicy {
            max_retries: 5,
            delay_ms: 1000,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web::ServerPorts;

    fn static_domain(root: &Path) -> Domain {
        Domain {
            hostname: "x.test".into(),
            project_id: None,
            root: root.display().to_string(),
            kind: SiteKind::Static,
            https: false,
            redirect_https: false,
            wildcard: false,
            enabled: true,
            ownership: Ownership::Managed,
            app: None,
            blocks: Default::default(),
            generated_hashes: Default::default(),
            public_domain: None,
            tunnel_id: None,
            server: None,
        }
    }

    #[test]
    fn static_site_with_an_index_php_is_served_as_php_not_downloaded() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            effective_kind(&static_domain(dir.path())),
            SiteKind::Static
        ));
        std::fs::write(dir.path().join("index.html"), "hi").unwrap();
        assert!(
            matches!(effective_kind(&static_domain(dir.path())), SiteKind::Static),
            "plain HTML stays static"
        );
        std::fs::create_dir_all(dir.path().join("node_modules").join("pkg")).unwrap();
        std::fs::write(
            dir.path().join("node_modules").join("pkg").join("x.php"),
            "<?php",
        )
        .unwrap();
        assert!(
            matches!(effective_kind(&static_domain(dir.path())), SiteKind::Static),
            "dependency folders don't count"
        );
        std::fs::create_dir_all(dir.path().join("contact")).unwrap();
        std::fs::write(dir.path().join("contact").join("Send.PHP"), "<?php").unwrap();
        assert!(
            matches!(
                effective_kind(&static_domain(dir.path())),
                SiteKind::Php { version: None }
            ),
            "php in a subfolder needs the PHP handler too"
        );
    }

    #[test]
    fn retaining_pools_spans_every_server_so_one_server_cannot_stop_anothers_pool() {
        let dir = tempfile::tempdir().unwrap();
        let mut on_nginx = static_domain(dir.path());
        on_nginx.hostname = "a.test".into();
        on_nginx.kind = SiteKind::Php {
            version: Some("8.1".into()),
        };
        let mut on_apache = static_domain(dir.path());
        on_apache.hostname = "b.test".into();
        on_apache.kind = SiteKind::Php {
            version: Some("8.4".into()),
        };
        on_apache.server = Some("apache".into());

        let installed: Vec<String> = vec!["8.1.34".into(), "8.4.26".into()];
        let pick = |wanted: Option<&str>| crate::php::pick_version(&installed, wanted);
        let cfg = test_config("nginx");
        let php_for = |_: &Domain| None;
        let ctx = ApplyContext {
            cfg: &cfg,
            php_for: &php_for,
            runtime_bin: &|_, _| None,
            overwrite: &[],
        };

        // The bug: rendering Apache alone must still keep nginx's 8.1 pool alive, because
        // nginx's sites keep a `fastcgi_pass` pointing at it.
        let all = vec![on_nginx, on_apache];
        assert_eq!(sites_for_server(&cfg, &all, "nginx").len(), 1);
        assert_eq!(sites_for_server(&cfg, &all, "apache").len(), 1);

        let mut kept = php_versions_across_servers(&ctx, &all, &pick);
        kept.sort();
        assert_eq!(
            kept,
            vec!["8.1.34".to_string(), "8.4.26".to_string()],
            "every enabled site's PHP version must survive one server's apply"
        );
    }

    #[test]
    fn a_disabled_site_does_not_pin_a_php_pool_open() {
        let mut d = static_domain(std::path::Path::new("."));
        d.hostname = "off.test".into();
        d.kind = SiteKind::Php {
            version: Some("8.1".into()),
        };
        d.enabled = false;
        let installed: Vec<String> = vec!["8.1.34".into(), "8.4.26".into()];
        let pick = |wanted: Option<&str>| crate::php::pick_version(&installed, wanted);
        let cfg = test_config("nginx");
        let php_for = |_: &Domain| None;
        let ctx = ApplyContext {
            cfg: &cfg,
            php_for: &php_for,
            runtime_bin: &|_, _| None,
            overwrite: &[],
        };
        assert!(
            php_versions_across_servers(&ctx, std::slice::from_ref(&d), &pick).is_empty(),
            "a disabled site must not pin a pool open"
        );
    }

    #[test]
    fn app_spec_puts_the_runtime_first_on_path_and_sets_port() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("npm.cmd"), "@echo off").unwrap();
        let app = AppSpec {
            executable: "npm".into(),
            args: vec!["run".into(), "dev".into()],
            cwd: "C:/app".into(),
            runtime: Some("node".into()),
        };
        let spec = build_app_spec("c.test", &app, Some(5173), Some(dir.path())).unwrap();

        assert_eq!(
            spec.executable, "cmd.exe",
            ".cmd shims must run through cmd.exe"
        );
        assert_eq!(spec.args[0], "/C");
        assert!(spec.args.iter().any(|a| a.ends_with("npm.cmd")));
        assert_eq!(&spec.args[spec.args.len() - 2..], ["run", "dev"]);
        assert!(spec.env.iter().any(|(k, v)| k == "PORT" && v == "5173"));
        let path = &spec.env.iter().find(|(k, _)| k == "PATH").unwrap().1;
        assert!(path.starts_with(&dir.path().display().to_string()));
    }

    #[test]
    fn app_spec_restarts_on_crash_so_a_dead_app_does_not_502_forever() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("npm.cmd"), "@echo off").unwrap();
        let app = AppSpec {
            executable: "npm".into(),
            args: vec!["run".into(), "dev".into()],
            cwd: "C:/app".into(),
            runtime: Some("node".into()),
        };
        let spec = build_app_spec("c.test", &app, Some(5173), Some(dir.path())).unwrap();

        let policy = spec
            .restart
            .expect("a crashed dev server must be brought back, not left 502");
        assert!(policy.max_retries > 0);
        assert!(
            policy.delay_ms > 0,
            "retries need a backoff, not a hot loop"
        );
    }

    #[test]
    fn missing_executable_gives_a_helpful_error() {
        let app = AppSpec {
            executable: "definitely-not-installed-xyz".into(),
            args: vec![],
            cwd: ".".into(),
            runtime: Some("node".into()),
        };
        let err = build_app_spec("c.test", &app, None, None).unwrap_err();
        assert!(err.contains("was not found"));
    }

    #[test]
    fn resolver_prefers_native_launcher_over_bare_unix_script() {
        // Node ships extensionless `npx`/`npm` shell scripts next to `npx.cmd`.
        // On Windows the bare script cannot spawn (os error 193); the `.cmd` must win.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("npx"), "#!/bin/sh\nnode \"$@\"").unwrap();
        std::fs::write(dir.path().join("npx.cmd"), "@echo off").unwrap();
        let found = find_executable(Some(dir.path()), "npx").unwrap();
        #[cfg(windows)]
        assert_eq!(
            found.file_name().and_then(|n| n.to_str()),
            Some("npx.cmd"),
            "bare Unix script must not shadow the .cmd shim on Windows"
        );
        #[cfg(not(windows))]
        assert_eq!(
            found.file_name().and_then(|n| n.to_str()),
            Some("npx"),
            "Unix keeps preferring the bare executable script"
        );
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

    fn test_config(default_server: &str) -> WebConfig {
        WebConfig {
            default_server: default_server.into(),
            servers: BTreeMap::new(),
            php_workers: 1,
            dns_port: 5354,
        }
    }

    fn test_manager(home: &crate::test_support::IsolatedHome) -> WebManager {
        let runtimes = Arc::new(RuntimeManager::new(home.paths.clone()));
        WebManager::new(
            home.paths.clone(),
            runtimes,
            Arc::new(ProcessSupervisor::new()),
            Arc::new(crate::certs::CertificateManager::new(&home.paths)),
            Arc::new(PhpPools::new(
                home.paths.clone(),
                Arc::new(RuntimeManager::new(home.paths.clone())),
                Arc::new(ProcessSupervisor::new()),
            )),
        )
    }

    #[test]
    fn every_server_lists_its_own_main_config() {
        let home = crate::test_support::isolated_home();
        // list_configs resolves a layout per server, which needs that runtime installed.
        // Installed means "the catalog's binary exists under runtimes/<id>/<version>", so
        // the runtime is faked from the catalog itself rather than from a made-up version.
        for m in crate::catalog::builtin_catalog()
            .into_iter()
            .filter(|m| SERVER_IDS.contains(&m.id))
        {
            let dir = home.paths.runtimes_dir().join(m.id).join(m.version);
            std::fs::create_dir_all(dir.join(m.binary).parent().unwrap()).unwrap();
            std::fs::write(dir.join(m.binary), b"").unwrap();
        }
        let mgr = test_manager(&home);
        let cfg = test_config("nginx");
        let domains = DomainStore::load(&home.paths).unwrap();

        // One Main per server, each naming its server and pointing at its own file. The
        // page keys a file by (server, hostname, part), so without the server field the
        // three Main entries collapse to one key and the sidebar shows "Main config" three
        // times with a single selection between them.
        let listed: Vec<ConfigFile> = SERVER_IDS
            .iter()
            .filter_map(|id| mgr.list_configs(&cfg, &domains, id).ok())
            .flatten()
            .filter(|f| f.part == ConfigPart::Main)
            .collect();
        assert_eq!(listed.len(), SERVER_IDS.len());
        for id in SERVER_IDS {
            let f = listed
                .iter()
                .find(|f| f.server == *id)
                .unwrap_or_else(|| panic!("no Main config listed for {id}"));
            assert_eq!(f.hostname, None);
            assert!(
                f.path.contains(id),
                "{id}'s Main config points at another server's file: {}",
                f.path
            );
        }
    }

    #[test]
    fn two_servers_track_running_independently() {
        let home = crate::test_support::isolated_home();
        let mgr = test_manager(&home);
        assert!(!mgr.is_running_id("nginx") && !mgr.is_running_id("apache"));

        // Bookkeeping is per id: dropping one entry must not touch the other.
        mgr.state.lock().unwrap().servers.insert(
            "nginx".into(),
            Running {
                process: ProcessId(1),
            },
        );
        mgr.state.lock().unwrap().servers.insert(
            "apache".into(),
            Running {
                process: ProcessId(2),
            },
        );
        assert_eq!(mgr.state.lock().unwrap().servers.len(), 2);
        assert!(mgr.usage_plan().servers.contains(&ProcessId(2)));

        mgr.stop_server("nginx");
        let st = mgr.state.lock().unwrap();
        assert!(!st.servers.contains_key("nginx"));
        assert!(
            st.servers.contains_key("apache"),
            "stopping one stops only one"
        );
    }

    #[test]
    fn status_reports_each_servers_own_effective_ports() {
        let home = crate::test_support::isolated_home();
        let mgr = test_manager(&home);
        let mut cfg = test_config("nginx");
        cfg.servers.insert(
            "apache".into(),
            ServerPorts {
                http: 9080,
                https: 9443,
            },
        );
        let st = mgr.status(&cfg, &DomainStore::load(&home.paths).unwrap().list());
        let by_id = |id: &str| st.servers.iter().find(|s| s.id == id).unwrap().clone();
        assert_eq!(st.default_server, "nginx");
        assert_eq!(by_id("nginx").http_port, 80);
        assert!(by_id("nginx").active);
        assert_eq!(by_id("apache").http_port, 9080);
        assert!(!by_id("apache").active);
        assert!(!by_id("apache").running);
    }

    #[test]
    fn status_counts_the_sites_each_server_actually_renders() {
        let home = crate::test_support::isolated_home();
        let mgr = test_manager(&home);
        let cfg = test_config("nginx");
        let mut domains = DomainStore::load(&home.paths).unwrap();
        let mut a = static_domain(Path::new("C:/sites/a"));
        a.hostname = "a.test".into();
        let mut pinned = static_domain(Path::new("C:/sites/b"));
        pinned.hostname = "b.test".into();
        pinned.server = Some("apache".into());
        domains.add(pinned).unwrap();
        domains.add(a).unwrap();
        let mut off = static_domain(Path::new("C:/sites/c"));
        off.hostname = "c.test".into();
        off.enabled = false;
        domains.add(off).unwrap();

        let st = mgr.status(&cfg, &domains.list());
        let count = |id: &str| st.servers.iter().find(|s| s.id == id).unwrap().sites;
        // The store's own home.test plus a.test, since neither picked a server.
        assert_eq!(count("nginx"), 2, "home.test and a.test");
        assert_eq!(count("apache"), 1, "b.test only");
        assert_eq!(count("caddy"), 0);
    }

    #[test]
    fn apply_groups_sites_onto_their_assigned_server_only() {
        let cfg = test_config("nginx");
        let mut a = static_domain(Path::new("C:/sites/a"));
        a.hostname = "a.test".into();
        let mut b = static_domain(Path::new("C:/sites/b"));
        b.hostname = "b.test".into();
        b.server = Some("apache".into());
        let mut off = static_domain(Path::new("C:/sites/c"));
        off.hostname = "c.test".into();
        off.enabled = false;
        let domains = vec![a.clone(), b.clone(), off.clone()];

        let names = |id: &str| {
            sites_for_server(&cfg, &domains, id)
                .into_iter()
                .map(|d| d.hostname)
                .collect::<Vec<_>>()
        };
        assert_eq!(names("nginx"), vec!["a.test".to_string()]);
        assert_eq!(names("apache"), vec!["b.test".to_string()]);
        assert!(names("caddy").is_empty());

        // Turning a site's override off moves it back to the default server.
        let mut moved = b.clone();
        moved.server = None;
        let all = vec![a, moved, off];
        assert_eq!(
            sites_for_server(&cfg, &all, "nginx")
                .into_iter()
                .map(|d| d.hostname)
                .collect::<Vec<_>>(),
            vec!["a.test".to_string(), "b.test".to_string()]
        );
    }

    #[test]
    fn a_server_that_was_never_started_is_not_an_apply_target() {
        let home = crate::test_support::isolated_home();
        let mgr = test_manager(&home);
        let cfg = test_config("nginx");
        assert_eq!(mgr.apply_targets(&cfg), vec!["nginx".to_string()]);
    }
}

/// A site saved as static whose folder holds PHP files would hand their source to the
/// browser as downloads. Serve it through PHP instead (PHP serves plain files fine too);
/// the stored kind is left alone so the user's choice is never rewritten behind their back.
fn effective_kind(d: &Domain) -> SiteKind {
    if matches!(d.kind, SiteKind::Static) && has_php(std::path::Path::new(&d.root), 3) {
        return SiteKind::Php { version: None };
    }
    d.kind.clone()
}

/// Any `.php` file in `dir` or up to `depth` folders below it (`/contact/index.php`
/// downloads just the same as a top-level one). Dependency and VCS folders are skipped:
/// a `node_modules` package shipping a stray PHP file doesn't make the site PHP.
fn has_php(dir: &Path, depth: u8) -> bool {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return false;
    };
    let mut subdirs = Vec::new();
    for e in rd.flatten() {
        let path = e.path();
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_file()
            && path
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("php"))
        {
            return true;
        }
        if ft.is_dir() && depth > 0 {
            let name = e.file_name().to_string_lossy().to_lowercase();
            if !name.starts_with('.') && name != "node_modules" && name != "vendor" {
                subdirs.push(path);
            }
        }
    }
    subdirs.iter().any(|d| has_php(d, depth - 1))
}

/// Top-level domains reserved for local use (RFC 2606 / 6761 / ICANN `.internal`): routing
/// every name under them to this machine can't hijack a real website. `.local` is left
/// out on purpose (mDNS and some company networks use it).
const SAFE_TLDS: &[&str] = &["test", "localhost", "example", "invalid", "internal"];

fn tld_of(hostname: &str) -> String {
    hostname
        .trim_end_matches('.')
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}
