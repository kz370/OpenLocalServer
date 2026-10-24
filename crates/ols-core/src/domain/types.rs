use std::collections::BTreeMap;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use crate::error::CoreError;

pub(crate) fn yes() -> bool {
    true
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Ownership {
    /// OLS generates the whole file from the site's settings; hand edits are drift.
    #[default]
    Managed,
    /// OLS generates the file and includes a user-owned snippet inside the server block.
    Advanced,
    /// You own the whole file. OLS validates and reloads it, never rewrites it.
    Manual,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SiteKind {
    /// Served by a supervised `php-cgi` pool. `version: None` = the newest installed PHP.
    Php {
        version: Option<String>,
    },
    /// Reverse proxy (§30): a dev server on this machine (Node/Python apps), or anything
    /// else reachable — a Docker container, another computer — via `upstream_host`.
    Proxy {
        upstream_port: u16,
        /// Where the target runs; `None` = this machine (127.0.0.1).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        upstream_host: Option<String>,
        /// The target only speaks HTTPS.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        upstream_https: bool,
    },
    Static,
}

/// A proxy target is pasted into server configs: a real port, and a hostname or IPv4
/// address with nothing that could break out of a directive.
pub(crate) fn validate_kind(kind: &SiteKind) -> Result<(), CoreError> {
    if let SiteKind::Proxy {
        upstream_port,
        upstream_host,
        ..
    } = kind
    {
        if *upstream_port == 0 {
            return Err(CoreError::DomainError(
                "upstream port must be between 1 and 65535".into(),
            ));
        }
        if let Some(h) = upstream_host {
            if h.is_empty()
                || !h
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
            {
                return Err(CoreError::DomainError(format!(
                    "\"{h}\" is not a valid target host"
                )));
            }
        }
    }
    Ok(())
}

impl SiteKind {
    /// The full upstream URL of a proxy site, e.g. `http://127.0.0.1:3000`.
    pub fn upstream_url(&self) -> Option<String> {
        match self {
            SiteKind::Proxy {
                upstream_port,
                upstream_host,
                upstream_https,
            } => Some(format!(
                "{}://{}:{upstream_port}",
                if *upstream_https { "https" } else { "http" },
                upstream_host
                    .as_deref()
                    .filter(|h| !h.is_empty())
                    .unwrap_or("127.0.0.1")
            )),
            _ => None,
        }
    }
}

/// A supervised app dev-server behind a proxy site (`npm run dev`, `uvicorn`, ...).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppSpec {
    pub executable: String,
    pub args: Vec<String>,
    pub cwd: String,
    /// Runtime whose bin dir goes first on PATH ("node", "python", "php").
    pub runtime: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeaderRule {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedirectRule {
    pub from: String,
    pub to: String,
    pub code: u16,
}

/// §30: `path` on this site is forwarded to `upstream` (e.g. `/api` → `http://127.0.0.1:8000`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxyMapping {
    pub path: String,
    pub upstream: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpstreamGroup {
    pub name: String,
    pub servers: Vec<String>,
}

/// §23: the structured (form-editable) blocks common to every web server. Raw editing
/// stays available — these just cover the frequent cases without hand-writing config.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SiteBlocks {
    #[serde(default)]
    pub headers: Vec<HeaderRule>,
    #[serde(default)]
    pub redirects: Vec<RedirectRule>,
    #[serde(default)]
    pub mappings: Vec<ProxyMapping>,
    #[serde(default)]
    pub upstreams: Vec<UpstreamGroup>,
    #[serde(default)]
    pub includes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Domain {
    pub hostname: String,
    #[serde(default)]
    pub project_id: Option<String>,
    /// Document root (PHP/static) or project root (proxy).
    pub root: String,
    pub kind: SiteKind,
    #[serde(default)]
    pub https: bool,
    /// §52: send plain-HTTP requests to HTTPS. Only meaningful when `https` is on.
    #[serde(default)]
    pub redirect_https: bool,
    /// §46: also answer for `*.hostname` (needs the local DNS resolver, Stage 9).
    #[serde(default)]
    pub wildcard: bool,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub ownership: Ownership,
    #[serde(default)]
    pub app: Option<AppSpec>,
    #[serde(default)]
    pub blocks: SiteBlocks,
    /// SHA-256 of the config file OLS last wrote, per web server id — drift
    /// detection (§26). Keyed by server because each server has its own file.
    #[serde(default)]
    pub generated_hashes: BTreeMap<String, String>,
    /// Public hostname routed through a named Cloudflare tunnel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_domain: Option<String>,
    /// Saved tunnel configuration used to expose this site.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tunnel_id: Option<String>,
    /// Which web server renders this site ("nginx" | "apache" | "caddy"). `None` means
    /// the default server; a site is rendered on exactly one server, never both.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
    /// Also serve this site at `localhost/<path_prefix>` (§55), alongside its own domain.
    /// One leading segment, lowercase letters, digits and `-`/`_` — it becomes a URL path
    /// in a generated config, so nothing exotic may get through.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_prefix: Option<String>,
    /// Optional Font Awesome icon identifier or class (e.g. "fa-wordpress" or "globe").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// SSL certificate provider: Local CA, Let's Encrypt, or Custom files.
    #[serde(default)]
    pub ssl_provider: SslProvider,
    /// Path to custom SSL certificate file (PEM / CRT).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_cert_path: Option<String>,
    /// Path to custom SSL private key file (PEM / KEY).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_key_path: Option<String>,
    /// Contact email for Let's Encrypt account / expiry notifications.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lets_encrypt_email: Option<String>,
    /// Automatically broadcast this domain/site online using a tunnel.
    #[serde(default)]
    pub auto_broadcast: bool,
    /// Provider used for online broadcast ("cloudflare", "localtunnel", "tailscale").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub broadcast_provider: Option<String>,
    /// Additional domain names (aliases) mapped to this same site.
    #[serde(default)]
    pub aliases: Vec<String>,
    /// Dynamic DNS provider ("cloudflare", "duckdns", "custom", or None).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ddns_provider: Option<String>,
    /// API token or secret key used for dynamic DNS record updates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ddns_token: Option<String>,
    /// Cloudflare zone id (optional override; auto-discovered if not set).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ddns_zone_id: Option<String>,
    /// Cloudflare proxy flag (true = proxied via Cloudflare CDN).
    #[serde(default)]
    pub ddns_proxied: bool,
    /// Last public IP that was successfully synced to DNS.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ddns_last_ip: Option<String>,
    /// Unix timestamp (seconds) of the last successful DDNS update.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ddns_last_synced_at: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SslProvider {
    #[default]
    LocalCa,
    LetsEncrypt,
    Custom,
}

impl std::fmt::Display for SslProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LocalCa => write!(f, "local_ca"),
            Self::LetsEncrypt => write!(f, "lets_encrypt"),
            Self::Custom => write!(f, "custom"),
        }
    }
}

impl Default for Domain {
    fn default() -> Self {
        Self {
            hostname: String::new(),
            project_id: None,
            root: String::new(),
            kind: SiteKind::Static,
            https: false,
            redirect_https: false,
            wildcard: false,
            enabled: true,
            ownership: Ownership::Managed,
            app: None,
            blocks: SiteBlocks::default(),
            generated_hashes: BTreeMap::new(),
            public_domain: None,
            tunnel_id: None,
            server: None,
            path_prefix: None,
            icon: None,
            ssl_provider: SslProvider::LocalCa,
            custom_cert_path: None,
            custom_key_path: None,
            lets_encrypt_email: None,
            auto_broadcast: false,
            broadcast_provider: None,
            aliases: Vec::new(),
            ddns_provider: None,
            ddns_token: None,
            ddns_zone_id: None,
            ddns_proxied: false,
            ddns_last_ip: None,
            ddns_last_synced_at: None,
        }
    }
}



#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BulkOutcome {
    pub changed: usize,
    pub skipped: Vec<String>,
}

impl BulkOutcome {
    pub fn skip(&mut self, hostname: &str, reason: &str) {
        self.skipped.push(format!("{hostname} {reason}"));
    }
}
