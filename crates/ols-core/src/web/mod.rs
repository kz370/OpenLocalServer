//! Web servers (§22–30, Stages 6, 9 and 10). Each server (Nginx, Apache, Caddy) implements
//! the [`WebServer`] trait — config rendering and the exact command lines for validate /
//! start / reload — so the apply pipeline in `manager.rs` is written once.

pub mod apache;
pub mod caddy;
pub mod manager;
pub mod nginx;

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::certs::CertPaths;
use crate::domain::SiteBlocks;
use crate::settings::SettingsService;

/// The ports one web server is stored with. The selected default never actually binds
/// these — it always takes 80/443 — but they are kept so a server that stops being the
/// default comes back on the port it was configured for instead of a surprise one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerPorts {
    pub http: u16,
    pub https: u16,
}

impl ServerPorts {
    /// What the default server binds, whatever is stored for it.
    pub const STANDARD: ServerPorts = ServerPorts {
        http: 80,
        https: 443,
    };
}

/// First port a server gets when it has never been configured.
pub fn default_ports_for(id: &str) -> ServerPorts {
    match id {
        "apache" => ServerPorts {
            http: 8080,
            https: 8443,
        },
        "caddy" => ServerPorts {
            http: 8081,
            https: 8444,
        },
        // nginx is the historical default, so its non-default port pair starts higher.
        "nginx" => ServerPorts {
            http: 8082,
            https: 8445,
        },
        _ => ServerPorts {
            http: 8080,
            https: 8443,
        },
    }
}

/// User-tunable web settings, read from the settings store on every call so a change in
/// the UI takes effect on the next apply without restarting the app.
///
/// Every server can run at the same time; they just can't share a port. The one named by
/// `default_server` binds 80/443 and the others bind their own stored ports.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebConfig {
    /// Server id that owns 80/443: "nginx" | "apache" | "caddy".
    pub default_server: String,
    /// Stored ports per server id. Never rewritten just because a server becomes default.
    pub servers: BTreeMap<String, ServerPorts>,
    /// `php-cgi` processes per PHP version. Windows `php-cgi` handles one request at a
    /// time per process, so a small pool keeps concurrent asset requests from queueing.
    pub php_workers: u16,
    /// Local DNS resolver port for wildcard domains. NRPT can only target port 53.
    pub dns_port: u16,
}

impl WebConfig {
    pub fn from_settings(settings: &SettingsService) -> Self {
        let num = |key: &str, default: u64| {
            settings
                .get(key)
                .and_then(|v| v.as_u64())
                .unwrap_or(default)
        };
        let port = |key: &str| settings.get(key).and_then(|v| v.as_u64()).map(|v| v as u16);

        // Pre-multi-server installs stored one server id and one port pair. They become
        // the default server's entry; every other server gets its own ports.
        let legacy_server = settings
            .get("web.server")
            .and_then(|v| v.as_str())
            .unwrap_or("nginx")
            .to_string();
        let default_server = settings
            .get("web.default_server")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| SERVER_IDS.contains(&s.as_str()))
            .unwrap_or(legacy_server.clone());
        if !SERVER_IDS.contains(&default_server.as_str()) {
            // A server that was uninstalled/renamed since the setting was written.
            return Self {
                default_server: "nginx".into(),
                servers: BTreeMap::new(),
                php_workers: num("web.php_workers", 3).clamp(1, 16) as u16,
                dns_port: num("web.dns_port", 53) as u16,
            };
        }

        let mut servers = BTreeMap::new();
        for id in SERVER_IDS {
            let is_default = *id == default_server;
            let fallback = default_ports_for(id);
            servers.insert(
                (*id).to_string(),
                ServerPorts {
                    http: port(&format!("web.servers.{id}.http_port")).unwrap_or(if is_default {
                        port("web.http_port").unwrap_or(fallback.http)
                    } else {
                        fallback.http
                    }),
                    https: port(&format!("web.servers.{id}.https_port")).unwrap_or(if is_default {
                        port("web.https_port").unwrap_or(fallback.https)
                    } else {
                        fallback.https
                    }),
                },
            );
        }
        Self {
            default_server,
            servers,
            php_workers: num("web.php_workers", 3).clamp(1, 16) as u16,
            dns_port: num("web.dns_port", 53) as u16,
        }
    }

    /// The stored ports for a server, never the ones it will actually bind.
    pub fn ports_for(&self, id: &str) -> ServerPorts {
        self.servers
            .get(id)
            .copied()
            .unwrap_or_else(|| default_ports_for(id))
    }

    /// The ports a server binds right now: the default always takes 80/443, every other
    /// server keeps its own stored pair. This is what binding, waiting and URLs use.
    pub fn effective_ports(&self, id: &str) -> ServerPorts {
        if id == self.default_server {
            ServerPorts::STANDARD
        } else {
            self.ports_for(id)
        }
    }

    /// Which server a site is served by: its own override, or the default. An id that is
    /// no longer a known server falls back to the default (never drops the site).
    pub fn resolve_server(&self, assigned: Option<&str>) -> String {
        match assigned.map(str::trim) {
            Some(s) if SERVER_IDS.contains(&s) => s.to_string(),
            _ => self.default_server.clone(),
        }
    }

    /// Deprecated single-server accessor: the default server. Kept so callers that only
    /// care about "the" server keep compiling.
    pub fn server(&self) -> &str {
        &self.default_server
    }

    /// Deprecated: the default server's effective HTTP port.
    pub fn http_port(&self) -> u16 {
        self.effective_ports(&self.default_server).http
    }

    /// Deprecated: the default server's effective HTTPS port.
    pub fn https_port(&self) -> u16 {
        self.effective_ports(&self.default_server).https
    }
}

/// How a site's requests are answered, with everything already resolved to concrete
/// ports and paths — `render_site` never has to look anything up.
#[derive(Debug, Clone)]
pub enum Backend {
    /// FastCGI workers for one PHP version; `pool` is the upstream's name-safe id ("php_81").
    Php {
        pool: String,
        ports: Vec<u16>,
    },
    /// `upstream` is a full URL: `http://127.0.0.1:3000`, `https://192.168.1.20:8443`.
    Proxy {
        upstream: String,
    },
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
    pub public_domain: Option<String>,
    pub forwarded_tls: bool,
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn settings(pairs: &[(&str, Value)]) -> (crate::test_support::IsolatedHome, SettingsService) {
        let home = crate::test_support::isolated_home();
        let mut svc = SettingsService::load(&home.paths).unwrap();
        for (k, v) in pairs {
            svc.set(*k, v.clone()).unwrap();
        }
        (home, svc)
    }

    #[test]
    fn migrates_legacy_single_server_keys() {
        let (_home, s) = settings(&[
            ("web.server", Value::String("apache".into())),
            ("web.http_port", Value::from(8080)),
            ("web.https_port", Value::from(8443)),
        ]);
        let cfg = WebConfig::from_settings(&s);
        assert_eq!(cfg.default_server, "apache");
        assert_eq!(
            cfg.ports_for("apache"),
            ServerPorts {
                http: 8080,
                https: 8443
            }
        );
        assert_eq!(cfg.ports_for("nginx"), default_ports_for("nginx"));
        assert_eq!(cfg.ports_for("caddy"), default_ports_for("caddy"));
    }

    #[test]
    fn a_fresh_install_defaults_to_nginx_on_80_443() {
        let (_home, s) = settings(&[]);
        let cfg = WebConfig::from_settings(&s);
        assert_eq!(cfg.default_server, "nginx");
        assert_eq!(cfg.effective_ports("nginx"), ServerPorts::STANDARD);
    }

    #[test]
    fn per_server_ports_round_trip_and_survive_a_default_switch() {
        let (_home, s) = settings(&[
            ("web.default_server", Value::String("nginx".into())),
            ("web.servers.apache.http_port", Value::from(9080)),
            ("web.servers.apache.https_port", Value::from(9443)),
        ]);
        let cfg = WebConfig::from_settings(&s);
        assert_eq!(cfg.ports_for("apache").http, 9080);
        assert_eq!(cfg.effective_ports("nginx"), ServerPorts::STANDARD);
        assert_eq!(
            cfg.effective_ports("apache"),
            ServerPorts {
                http: 9080,
                https: 9443
            }
        );

        let switched = WebConfig {
            default_server: "apache".into(),
            ..cfg
        };
        assert_eq!(switched.effective_ports("apache"), ServerPorts::STANDARD);
        assert_eq!(
            switched.effective_ports("nginx"),
            default_ports_for("nginx"),
            "switching the default must not rewrite the old default's stored ports"
        );
    }

    #[test]
    fn an_unknown_default_server_falls_back_to_nginx() {
        let (_home, s) = settings(&[("web.default_server", Value::String("iis".into()))]);
        assert_eq!(WebConfig::from_settings(&s).default_server, "nginx");
    }

    #[test]
    fn resolve_server_prefers_the_override_and_falls_back_for_junk() {
        let (_home, s) = settings(&[("web.default_server", Value::String("nginx".into()))]);
        let cfg = WebConfig::from_settings(&s);
        assert_eq!(cfg.resolve_server(None), "nginx");
        assert_eq!(cfg.resolve_server(Some("apache")), "apache");
        assert_eq!(cfg.resolve_server(Some("")), "nginx");
        assert_eq!(cfg.resolve_server(Some("iis")), "nginx");
    }
}
