//! Web servers (§22–30, Stages 6, 9 and 10). Each server (Nginx, Apache, Caddy) implements
//! the [`WebServer`] trait — config rendering and the exact command lines for validate /
//! start / reload — so the apply pipeline in `manager.rs` is written once.

pub mod apache;
pub mod caddy;
pub mod manager;
pub mod nginx;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::certs::CertPaths;
use crate::domain::SiteBlocks;
use crate::settings::SettingsService;

/// User-tunable web settings, read from the settings store on every call so a change in
/// the UI takes effect on the next apply without restarting the app.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebConfig {
    /// Active server id: "nginx" | "apache" | "caddy". Only one runs at a time (they share 80/443).
    pub server: String,
    pub http_port: u16,
    pub https_port: u16,
    /// `php-cgi` processes per PHP version. Windows `php-cgi` handles one request at a
    /// time per process, so a small pool keeps concurrent asset requests from queueing.
    pub php_workers: u16,
    /// Local DNS resolver port for wildcard domains. NRPT can only target port 53.
    pub dns_port: u16,
}

impl WebConfig {
    pub fn from_settings(settings: &SettingsService) -> Self {
        let num = |key: &str, default: u64| settings.get(key).and_then(|v| v.as_u64()).unwrap_or(default);
        Self {
            server: settings.get("web.server").and_then(|v| v.as_str()).unwrap_or("nginx").to_string(),
            http_port: num("web.http_port", 80) as u16,
            https_port: num("web.https_port", 443) as u16,
            php_workers: num("web.php_workers", 3).clamp(1, 16) as u16,
            dns_port: num("web.dns_port", 53) as u16,
        }
    }
}

/// How a site's requests are answered, with everything already resolved to concrete
/// ports and paths — `render_site` never has to look anything up.
#[derive(Debug, Clone)]
pub enum Backend {
    /// FastCGI workers for one PHP version; `pool` is the upstream's name-safe id ("php_81").
    Php { pool: String, ports: Vec<u16> },
    /// `upstream` is a full URL: `http://127.0.0.1:3000`, `https://192.168.1.20:8443`.
    Proxy { upstream: String },
    Static,
}

/// One site, ready to render.
#[derive(Debug, Clone)]
pub struct SiteSpec {
    pub hostname: String,
    pub wildcard: bool,
    pub root: String,
    pub backend: Backend,
    /// Present when HTTPS is on and a certificate exists.
    pub tls: Option<CertPaths>,
    pub redirect_https: bool,
    pub blocks: SiteBlocks,
    /// Advanced ownership: a user-owned snippet included inside the site.
    pub custom_snippet: Option<PathBuf>,
}

impl SiteSpec {
    /// Names the site answers to: the host, plus `*.host` when wildcard is on.
    pub fn server_names(&self) -> Vec<String> {
        let mut names = vec![self.hostname.clone()];
        if self.wildcard {
            names.push(format!("*.{}", self.hostname));
        }
        names
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Ports {
    pub http: u16,
    pub https: u16,
}

/// A PHP FastCGI pool the main config may need to declare (nginx upstreams, apache balancers).
#[derive(Debug, Clone)]
pub struct PoolSpec {
    pub id: String,
    pub ports: Vec<u16>,
}

/// Where one server keeps its files.
#[derive(Debug, Clone)]
pub struct ServerLayout {
    /// The server's install dir (has bin/, conf/, modules/, ...).
    pub install_dir: PathBuf,
    /// Our working dir for this server: generated config, logs, temp.
    pub prefix: PathBuf,
    pub sites_dir: PathBuf,
    pub custom_dir: PathBuf,
    pub logs_dir: PathBuf,
    pub binary: PathBuf,
}

impl ServerLayout {
    pub fn site_file(&self, ext: &str, hostname: &str) -> PathBuf {
        self.sites_dir.join(format!("{hostname}.{ext}"))
    }
    pub fn custom_file(&self, ext: &str, hostname: &str) -> PathBuf {
        self.custom_dir.join(format!("{hostname}.{ext}"))
    }
}

/// One line-per-command description of how to drive a server binary.
pub struct Invocation {
    pub args: Vec<String>,
    pub cwd: PathBuf,
}

pub trait WebServer: Send + Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    /// File extension for per-site config ("conf", or "caddy" for Caddyfile snippets).
    fn config_ext(&self) -> &'static str;
    /// The main config file the binary is started with.
    fn main_config(&self, layout: &ServerLayout) -> PathBuf;
    /// Renders the top-level config that pulls in every site file.
    fn render_main(&self, layout: &ServerLayout, ports: Ports, pools: &[PoolSpec]) -> String;
    fn render_site(&self, site: &SiteSpec, ports: Ports) -> String;
    /// One-time filesystem preparation (create dirs, copy mime.types, ...).
    fn prepare(&self, layout: &ServerLayout) -> std::io::Result<()>;
    fn validate(&self, layout: &ServerLayout) -> Invocation;
    fn start(&self, layout: &ServerLayout) -> Invocation;
    /// Graceful in-place reload; `None` means "stop and start again".
    fn reload(&self, layout: &ServerLayout) -> Option<Invocation>;
    fn stop(&self, layout: &ServerLayout) -> Option<Invocation>;
    /// Where the server writes its error log (shown on the Logs page).
    fn error_log(&self, layout: &ServerLayout) -> PathBuf;
}

pub fn server_by_id(id: &str) -> Option<Box<dyn WebServer>> {
    match id {
        "nginx" => Some(Box::new(nginx::Nginx)),
        "apache" => Some(Box::new(apache::Apache)),
        "caddy" => Some(Box::new(caddy::Caddy)),
        _ => None,
    }
}

pub const SERVER_IDS: &[&str] = &["nginx", "apache", "caddy"];

/// Forward-slash path for config files — every one of these servers accepts `/` on
/// Windows, and `\` would be read as an escape.
pub fn cfg_path(path: &std::path::Path) -> String {
    path.display().to_string().replace('\\', "/")
}

/// Redirect target for "http → https": the port is omitted only when it's the default 443.
pub fn https_redirect_port_suffix(ports: Ports) -> String {
    if ports.https == 443 {
        String::new()
    } else {
        format!(":{}", ports.https)
    }
}

pub const MANAGED_HEADER: &str = "# Managed by OpenLocalServer. Changes made here are reported as drift;\n# switch this site to Advanced or Manual ownership to customise it.\n";
