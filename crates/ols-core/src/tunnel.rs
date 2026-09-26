//! Public tunnels (§56–60): a local site reachable from the internet for a while, for
//! webhooks, OAuth callbacks and showing work to someone.
//!
//! Safety first (§59, §140):
//! - a tunnel only ever starts when asked (never at app start; a manifest must say
//!   `autostart: true`, and even then the first exposure needs a confirmation);
//! - the first start of each tunnel asks for an explicit "yes, make this public";
//! - databases, caches, mail and debugger ports are refused as targets unless the tunnel
//!   explicitly allows internal targets;
//! - provider tokens live in the Secrets Manager, reach the provider through environment
//!   variables (never the command line), and are scrubbed from the tunnel's log;
//! - optional access control (a username and password) is enforced by our own inspector
//!   proxy, so it works with every provider.
//!
//! Each provider is a [`TunnelProvider`]: how to find its program, how to start it for a
//! local port, and how to spot the public URL in its output. Every tunnel points the
//! provider at its own [`crate::inspector::Inspector`], which forwards to the site and
//! records the traffic (§110).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::app::Inner;
use crate::error::CoreError;
use crate::inspector::{Inspector, RecordedRequest, Target};
use crate::paths::AppPaths;
use crate::process::{ProcessId, ProcessSpec, RestartPolicy};

/// Ports that are never a tunnel target by default: databases, caches, mail, debuggers.
const INTERNAL_PORTS: &[(u16, &str)] = &[
    (3306, "MariaDB"),
    (5432, "PostgreSQL"),
    (27017, "MongoDB"),
    (6379, "Redis"),
    (1025, "Mailpit SMTP"),
    (8025, "Mailpit"),
    (9003, "Xdebug"),
    (9000, "PHP-FPM"),
];
const PUBLIC_ADDRESS_TIMEOUT_MS: u64 = 60_000;
const PUBLIC_ADDRESS_TIMEOUT: &str = "The provider has not reported a public address after 60 seconds. Open Logs for details, then stop and try again.";

fn err(msg: impl Into<String>) -> CoreError {
    CoreError::TunnelError(msg.into())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TunnelConfig {
    pub id: String,
    #[serde(default)]
    pub project_id: Option<String>,
    pub name: String,
    /// cloudflare, ngrok, localtunnel, tailscale.
    pub provider: String,
    /// The local site: "https://shop.test" or "http://127.0.0.1:8000".
    pub target: String,
    /// Username for the optional access control; its password is a secret.
    #[serde(default)]
    pub auth_user: Option<String>,
    /// Allow a database / cache / mail / debugger port as the target (off by default).
    #[serde(default)]
    pub allow_internal: bool,
    /// For a named Cloudflare tunnel: the public hostname configured in its dashboard.
    #[serde(default)]
    pub public_hostname: Option<String>,
    /// Set once the user confirmed that this tunnel makes the site public.
    #[serde(default)]
    pub acknowledged: bool,
    /// Reconnect a named Cloudflare tunnel after the supervised process exits.
    #[serde(default)]
    pub autostart: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderInfo {
    pub id: String,
    pub name: String,
    /// Found on this computer (its path), or `None` with `install_hint`.
    pub path: Option<String>,
    pub install_hint: String,
    /// Takes an account token (ngrok, a named Cloudflare tunnel).
    pub uses_token: bool,
    pub token_saved: bool,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelStatus {
    pub config: TunnelConfig,
    /// stopped, needs_confirmation, starting, connected, failed.
    pub state: String,
    pub public_url: Option<String>,
    pub started_ms: Option<u64>,
    pub requests: usize,
    pub last_request_ms: Option<u64>,
    pub latency_ms: Option<u64>,
    pub error: Option<String>,
    pub inspector_port: Option<u16>,
    pub has_password: bool,
    /// A warning for the confirmation dialog (what exactly becomes public).
    pub exposure: String,
}

// ------------------------------------------------------------------------ providers

/// §56's interface. `authenticate` is the token in the Secrets Manager; `create` is the
/// saved [`TunnelConfig`]; start / stop / status / public URL / logs go through the
/// process the command starts.
pub trait TunnelProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn program(&self) -> &'static str;
    fn install_hint(&self) -> &'static str;
    fn uses_token(&self) -> bool {
        false
    }
    fn note(&self) -> &'static str {
        ""
    }
    /// Arguments and environment to expose `127.0.0.1:port`.
    fn command(
        &self,
        port: u16,
        token: Option<&str>,
        config: &TunnelConfig,
    ) -> (Vec<String>, Vec<(String, String)>);
    /// The public URL, found in the program's output.
    fn find_url(&self, line: &str) -> Option<String>;
    /// Common install folders to look in besides PATH.
    fn known_paths(&self) -> Vec<PathBuf> {
        Vec::new()
    }
}

fn url_matching(line: &str, suffix: &str) -> Option<String> {
    static URL: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = URL.get_or_init(|| regex::Regex::new(r"https://[A-Za-z0-9.-]+").unwrap());
    re.find_iter(line)
        .map(|m| m.as_str().to_string())
        .find(|u| u.ends_with(suffix) || u.contains(&format!("{suffix}/")))
}

struct Cloudflare;
impl TunnelProvider for Cloudflare {
    fn id(&self) -> &'static str {
        "cloudflare"
    }
    fn name(&self) -> &'static str {
        "Cloudflare Tunnel"
    }
    fn program(&self) -> &'static str {
        "cloudflared"
    }
    fn install_hint(&self) -> &'static str {
        "Install it with: winget install --id Cloudflare.cloudflared"
    }
    fn uses_token(&self) -> bool {
        true
    }
    fn note(&self) -> &'static str {
        "No account needed: a random trycloudflare.com address. Save a tunnel token for a named tunnel with your own hostname."
    }
    fn command(
        &self,
        port: u16,
        token: Option<&str>,
        config: &TunnelConfig,
    ) -> (Vec<String>, Vec<(String, String)>) {
        let url = format!("http://127.0.0.1:{port}");
        match token {
            // A named tunnel: its token goes in TUNNEL_TOKEN, never on the command line.
            Some(t)
                if config
                    .public_hostname
                    .as_deref()
                    .is_some_and(|h| !h.is_empty()) =>
            {
                (
                    vec!["tunnel".into(), "--no-autoupdate".into(), "run".into()],
                    vec![("TUNNEL_TOKEN".into(), t.into())],
                )
            }
            // Quick tunnels use `tunnel --url`; `run` is only for a named tunnel.
            _ => (
                vec![
                    "tunnel".into(),
                    "--no-autoupdate".into(),
                    "--url".into(),
                    url,
                ],
                vec![],
            ),
        }
    }
    fn find_url(&self, line: &str) -> Option<String> {
        url_matching(line, ".trycloudflare.com")
    }
    fn known_paths(&self) -> Vec<PathBuf> {
        vec![
            PathBuf::from(r"C:\Program Files (x86)\cloudflared\cloudflared.exe"),
            PathBuf::from(r"C:\Program Files\cloudflared\cloudflared.exe"),
        ]
    }
}

struct Ngrok;
impl TunnelProvider for Ngrok {
    fn id(&self) -> &'static str {
        "ngrok"
    }
    fn name(&self) -> &'static str {
        "ngrok"
    }
    fn program(&self) -> &'static str {
        "ngrok"
    }
    fn install_hint(&self) -> &'static str {
        "Install it with: winget install --id ngrok.ngrok, then save your authtoken here."
    }
    fn uses_token(&self) -> bool {
        true
    }
    fn note(&self) -> &'static str {
        "Needs a free ngrok account; its authtoken is kept in Windows Credential Manager."
    }
    fn command(
        &self,
        port: u16,
        token: Option<&str>,
        _config: &TunnelConfig,
    ) -> (Vec<String>, Vec<(String, String)>) {
        let env = token
            .map(|t| vec![("NGROK_AUTHTOKEN".to_string(), t.to_string())])
            .unwrap_or_default();
        (
            vec![
                "http".into(),
                format!("127.0.0.1:{port}"),
                "--log".into(),
                "stdout".into(),
                "--log-format".into(),
                "logfmt".into(),
            ],
            env,
        )
    }
    fn find_url(&self, line: &str) -> Option<String> {
        url_matching(line, ".ngrok-free.app")
            .or_else(|| url_matching(line, ".ngrok.app"))
            .or_else(|| url_matching(line, ".ngrok.io"))
            .or_else(|| url_matching(line, ".ngrok-free.dev"))
    }
    fn known_paths(&self) -> Vec<PathBuf> {
        std::env::var_os("LOCALAPPDATA")
            .map(|d| vec![PathBuf::from(d).join(r"Microsoft\WinGet\Links\ngrok.exe")])
            .unwrap_or_default()
    }
}

struct LocalTunnel;
impl TunnelProvider for LocalTunnel {
    fn id(&self) -> &'static str {
        "localtunnel"
    }
    fn name(&self) -> &'static str {
        "LocalTunnel"
    }
    fn program(&self) -> &'static str {
        "npx"
    }
    fn install_hint(&self) -> &'static str {
        "Runs through npx, so it needs Node.js (install it on the Runtimes page)."
    }
    fn note(&self) -> &'static str {
        "No account; visitors see a reminder page on first visit (loca.lt)."
    }
    fn command(
        &self,
        port: u16,
        _token: Option<&str>,
        _config: &TunnelConfig,
    ) -> (Vec<String>, Vec<(String, String)>) {
        (
            vec![
                "--yes".into(),
                "localtunnel".into(),
                "--port".into(),
                port.to_string(),
                "--local-host".into(),
                "127.0.0.1".into(),
            ],
            vec![],
        )
    }
    fn find_url(&self, line: &str) -> Option<String> {
        url_matching(line, ".loca.lt")
    }
}

struct Tailscale;
impl TunnelProvider for Tailscale {
    fn id(&self) -> &'static str {
        "tailscale"
    }
    fn name(&self) -> &'static str {
        "Tailscale Funnel"
    }
    fn program(&self) -> &'static str {
        "tailscale"
    }
    fn install_hint(&self) -> &'static str {
        "Install Tailscale, sign in, and turn on Funnel for your tailnet."
    }
    fn note(&self) -> &'static str {
        "Uses your machine's ts.net name; Funnel must be allowed in the tailnet's policy."
    }
    fn command(
        &self,
        port: u16,
        _token: Option<&str>,
        _config: &TunnelConfig,
    ) -> (Vec<String>, Vec<(String, String)>) {
        (vec!["funnel".into(), port.to_string()], vec![])
    }
    fn find_url(&self, line: &str) -> Option<String> {
        url_matching(line, ".ts.net")
    }
    fn known_paths(&self) -> Vec<PathBuf> {
        vec![PathBuf::from(r"C:\Program Files\Tailscale\tailscale.exe")]
    }
}

/// For tests: "connects" at once, with the inspector itself as the public URL.
struct Mock;
impl TunnelProvider for Mock {
    fn id(&self) -> &'static str {
        "mock"
    }
    fn name(&self) -> &'static str {
        "Test provider"
    }
    fn program(&self) -> &'static str {
        ""
    }
    fn install_hint(&self) -> &'static str {
        ""
    }
    fn command(
        &self,
        _port: u16,
        _token: Option<&str>,
        _config: &TunnelConfig,
    ) -> (Vec<String>, Vec<(String, String)>) {
        (vec![], vec![])
    }
    fn find_url(&self, _line: &str) -> Option<String> {
        None
    }
}

pub fn providers() -> Vec<Box<dyn TunnelProvider>> {
    vec![
        Box::new(Cloudflare),
        Box::new(Ngrok),
        Box::new(LocalTunnel),
        Box::new(Tailscale),
    ]
}

fn provider(id: &str) -> Option<Box<dyn TunnelProvider>> {
    if id == "mock" {
        return Some(Box::new(Mock));
    }
    providers().into_iter().find(|p| p.id() == id)
}

fn token_key(provider: &str) -> String {
    format!("tunnel.{provider}.token")
}

fn password_key(id: &str) -> String {
    format!("tunnel.{id}.password")
}

/// `Basic <base64(user:pass)>`, what the inspector expects.
fn basic_auth(user: &str, pass: &str) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let input = format!("{user}:{pass}").into_bytes();
    let mut out = String::new();
    for chunk in input.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    format!("Basic {out}")
}

/// Finds a program on PATH (with `.exe` / `.cmd`), then in its usual install folders.
fn find_program(name: &str, extra: &[PathBuf]) -> Option<PathBuf> {
    if name.is_empty() {
        return None;
    }
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for ext in ["exe", "cmd", "bat"] {
            let p = dir.join(format!("{name}.{ext}"));
            if p.is_file() {
                return Some(p);
            }
        }
    }
    extra.iter().find(|p| p.is_file()).cloned()
}

// ------------------------------------------------------------------------ store + manager

struct Live {
    inspector: Inspector,
    process: Option<ProcessId>,
    provider: String,
    started_ms: u64,
    public_url: Option<String>,
    latency_ms: Option<u64>,
    error: Option<String>,
    /// Secret values to scrub from the log.
    secrets: Vec<String>,
}

pub struct TunnelManager {
    file: PathBuf,
    configs: Mutex<Vec<TunnelConfig>>,
    live: Mutex<HashMap<String, Live>>,
    runtime: Arc<tokio::runtime::Runtime>,
}

impl TunnelManager {
    pub fn new(paths: &AppPaths) -> Self {
        let file = paths.data_dir().join("tunnels.json");
        let configs = std::fs::read_to_string(&file)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("ols-inspector")
            .enable_all()
            .build()
            .expect("failed to start the tunnel runtime");
        Self {
            file,
            configs: Mutex::new(configs),
            live: Mutex::new(HashMap::new()),
            runtime: Arc::new(runtime),
        }
    }

    fn persist(&self, configs: &[TunnelConfig]) -> Result<(), CoreError> {
        let tmp = self.file.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(configs)?)?;
        std::fs::rename(&tmp, &self.file)?;
        Ok(())
    }

    pub fn any_running(&self) -> bool {
        !self.live.lock().unwrap().is_empty()
    }
}

/// Host and port of a target URL, for the safety checks.
fn parse_target(target: &str) -> Result<reqwest::Url, CoreError> {
    let url = reqwest::Url::parse(target.trim()).map_err(|_| {
        err(format!(
            "\"{target}\" is not a URL like https://shop.test or http://127.0.0.1:8000"
        ))
    })?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(err("the target must be an http:// or https:// address"));
    }
    Ok(url)
}

impl Inner {
    pub fn tunnel_providers(&self) -> Vec<ProviderInfo> {
        providers()
            .into_iter()
            .map(|p| ProviderInfo {
                id: p.id().into(),
                name: p.name().into(),
                path: self
                    .provider_program(p.as_ref())
                    .map(|x| x.display().to_string()),
                install_hint: p.install_hint().into(),
                uses_token: p.uses_token(),
                token_saved: p.uses_token()
                    && crate::secrets::get_secret(&token_key(p.id()))
                        .ok()
                        .flatten()
                        .is_some(),
                note: p.note().into(),
            })
            .collect()
    }

    fn provider_program(&self, p: &dyn TunnelProvider) -> Option<PathBuf> {
        if let Some(c) = self
            .custom_installs
            .lock()
            .unwrap()
            .resolve(p.program(), None)
        {
            return Some(PathBuf::from(&c.path));
        }
        find_program(p.program(), &p.known_paths())
    }

    pub fn tunnels_for(&self, project_id: &str) -> Vec<TunnelConfig> {
        self.tunnels
            .configs
            .lock()
            .unwrap()
            .iter()
            .filter(|t| t.project_id.as_deref() == Some(project_id))
            .cloned()
            .collect()
    }

    /// What starting this tunnel would expose, in words, for the confirmation dialog.
    fn exposure(&self, c: &TunnelConfig) -> String {
        let project = c
            .project_id
            .as_ref()
            .and_then(|p| self.projects.lock().unwrap().get(p))
            .map(|p| format!(" ({})", p.name))
            .unwrap_or_default();
        format!("Anyone with the public address will be able to open {}{project}. Only start it while you need it, and stop it when you are done.", c.target)
    }

    fn check_target(&self, c: &TunnelConfig) -> Result<(), CoreError> {
        let url = parse_target(&c.target)?;
        let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
        let port = url.port_or_known_default().unwrap_or(80);
        let local = matches!(host.as_str(), "127.0.0.1" | "localhost" | "::1" | "[::1]");
        if local && !c.allow_internal {
            if let Some((_, what)) = INTERNAL_PORTS.iter().find(|(p, _)| *p == port) {
                return Err(err(format!("port {port} is {what}. Databases, caches, mail and debuggers are never made public unless the tunnel allows internal targets.")));
            }
        }
        if !local && self.domains.lock().unwrap().get(&host).is_none() {
            return Err(err(format!(
                "{host} is not one of your sites. Tunnels point at your own sites or 127.0.0.1."
            )));
        }
        Ok(())
    }

    pub fn save_tunnel(&self, mut c: TunnelConfig) -> Result<TunnelConfig, CoreError> {
        if provider(&c.provider).is_none() {
            return Err(err(format!("unknown tunnel provider \"{}\"", c.provider)));
        }
        self.check_target(&c)?;
        if c.name.trim().is_empty() {
            c.name = parse_target(&c.target)?
                .host_str()
                .unwrap_or("tunnel")
                .to_string();
        }
        if c.id.is_empty() {
            c.id = format!(
                "{}-{}",
                crate::domain::slugify(&c.name),
                &format!("{:x}", now_ms())[6..]
            );
        }
        let mut configs = self.tunnels.configs.lock().unwrap();
        match configs.iter_mut().find(|x| x.id == c.id) {
            Some(x) => {
                // Changing where a tunnel points needs the confirmation again.
                if x.target != c.target || x.provider != c.provider {
                    c.acknowledged = false;
                } else {
                    c.acknowledged = c.acknowledged || x.acknowledged;
                }
                *x = c.clone();
            }
            None => {
                c.acknowledged = false;
                configs.push(c.clone());
            }
        }
        self.tunnels.persist(&configs)?;
        Ok(c)
    }

    pub fn remove_tunnel(&self, id: &str) -> Result<(), CoreError> {
        self.stop_tunnel(id);
        let mut configs = self.tunnels.configs.lock().unwrap();
        configs.retain(|t| t.id != id);
        self.tunnels.persist(&configs)?;
        let _ = crate::secrets::delete_secret(&password_key(id));
        Ok(())
    }

    pub fn set_tunnel_password(&self, id: &str, password: Option<&str>) -> Result<(), CoreError> {
        match password.filter(|p| !p.is_empty()) {
            Some(p) => crate::secrets::set_secret(&password_key(id), p).map_err(err),
            None => crate::secrets::delete_secret(&password_key(id)).map_err(err),
        }
    }

    pub fn set_tunnel_token(
        &self,
        provider_id: &str,
        token: Option<&str>,
    ) -> Result<(), CoreError> {
        match token.filter(|t| !t.trim().is_empty()) {
            Some(t) => crate::secrets::set_secret(&token_key(provider_id), t.trim()).map_err(err),
            None => crate::secrets::delete_secret(&token_key(provider_id)).map_err(err),
        }
    }

    /// Starts a tunnel. The first time, `confirm_exposure` must be true (§59: warn before
    /// first exposure); otherwise the status comes back as `needs_confirmation`.
    pub fn start_tunnel(
        &self,
        id: &str,
        confirm_exposure: bool,
    ) -> Result<TunnelStatus, CoreError> {
        let mut c = self
            .tunnels
            .configs
            .lock()
            .unwrap()
            .iter()
            .find(|t| t.id == id)
            .cloned()
            .ok_or_else(|| err(format!("no tunnel \"{id}\"")))?;
        if self.tunnels.live.lock().unwrap().contains_key(id) {
            return self.tunnel_status(id);
        }
        if !c.acknowledged {
            if !confirm_exposure {
                let mut s = self.tunnel_status(id)?;
                s.state = "needs_confirmation".into();
                return Ok(s);
            }
            c.acknowledged = true;
            let mut configs = self.tunnels.configs.lock().unwrap();
            if let Some(x) = configs.iter_mut().find(|x| x.id == id) {
                x.acknowledged = true;
            }
            self.tunnels.persist(&configs)?;
        }
        self.check_target(&c)?;
        let p = provider(&c.provider)
            .ok_or_else(|| err(format!("unknown tunnel provider \"{}\"", c.provider)))?;

        // The inspector sits between the provider and the site.
        let url = parse_target(&c.target)?;
        let host = url.host_str().unwrap_or_default().to_string();
        let cfg = self.web_config();
        let ours = self.domains.lock().unwrap().get(&host).is_some();
        let resolve = ours.then(|| {
            std::net::SocketAddr::from((
                [127, 0, 0, 1],
                if url.scheme() == "https" {
                    cfg.https_port
                } else {
                    cfg.http_port
                },
            ))
        });
        let routed_host = c
            .public_hostname
            .as_deref()
            .filter(|h| !h.is_empty())
            .unwrap_or(&host);
        let base = if ours {
            format!("{}://{routed_host}", url.scheme())
        } else {
            c.target.trim_end_matches('/').to_string()
        };
        let ca = std::fs::read(self.certs.ca_info().cert_path).ok();
        let mut inspector = Inspector::start(
            self.tunnels.runtime.clone(),
            Target {
                base,
                host: routed_host.to_string(),
            },
            resolve,
            ca,
        )
        .map_err(err)?;
        let password = crate::secrets::get_secret(&password_key(id)).ok().flatten();
        let mut secrets = Vec::new();
        if let (Some(user), Some(pass)) = (
            c.auth_user.as_deref().filter(|u| !u.is_empty()),
            password.as_deref(),
        ) {
            inspector.require_auth(basic_auth(user, pass));
            secrets.push(pass.to_string());
        }

        let token = if p.uses_token() {
            crate::secrets::get_secret(&token_key(p.id())).map_err(err)?
        } else {
            None
        };
        if c.provider == "cloudflare"
            && c.public_hostname.as_deref().is_some_and(|h| !h.is_empty())
            && token.is_none()
        {
            return Err(err("a named Cloudflare tunnel needs its saved tunnel token. Add it in Tunnels → Providers."));
        }
        if c.provider == "ngrok" && token.is_none() {
            return Err(err(
                "ngrok needs your authtoken. Save it in the tunnel's provider settings first.",
            ));
        }
        if let Some(t) = &token {
            secrets.push(t.clone());
        }
        let (args, env) = p.command(inspector.port, token.as_deref(), &c);
        let process = if c.provider == "mock" {
            None
        } else {
            let (exe, args, mut env_all) = if c.provider == "localtunnel" {
                let (exe, full, env2) =
                    self.project_program("npx", &args, c.project_id.as_deref())?;
                (exe, full, env2)
            } else {
                let exe = self.provider_program(p.as_ref()).ok_or_else(|| {
                    err(format!("{} was not found. {}", p.name(), p.install_hint()))
                })?;
                (exe, args, Vec::new())
            };
            env_all.extend(env);
            Some(
                self.supervisor.start(ProcessSpec {
                    name: format!("Tunnel: {} ({})", c.name, p.name()),
                    executable: exe.display().to_string(),
                    args,
                    cwd: None,
                    env: env_all,
                    restart: (c.autostart
                        && c.provider == "cloudflare"
                        && c.public_hostname.is_some())
                    .then_some(RestartPolicy {
                        max_retries: 10,
                        delay_ms: 5000,
                    }),
                }),
            )
        };
        let public_url = if c.provider == "mock" {
            Some(format!("http://127.0.0.1:{}", inspector.port))
        } else {
            c.public_hostname
                .clone()
                .filter(|h| !h.is_empty() && token.is_some())
                .map(|h| format!("https://{h}"))
        };
        tracing::info!(tunnel = %c.name, provider = %c.provider, target = %c.target, "tunnel started");
        self.tunnels.live.lock().unwrap().insert(
            id.to_string(),
            Live {
                inspector,
                process,
                provider: c.provider.clone(),
                started_ms: now_ms(),
                public_url,
                latency_ms: None,
                error: None,
                secrets,
            },
        );
        self.tunnel_status(id)
    }

    pub fn stop_tunnel(&self, id: &str) {
        if let Some(live) = self.tunnels.live.lock().unwrap().remove(id) {
            if let Some(p) = live.process {
                self.supervisor.stop(p);
            }
            tracing::info!(tunnel = %id, "tunnel stopped");
        }
    }

    pub fn stop_all_tunnels(&self) {
        let ids: Vec<String> = self.tunnels.live.lock().unwrap().keys().cloned().collect();
        for id in ids {
            self.stop_tunnel(&id);
        }
    }

    /// Reads the provider's output for the public URL and notices a provider that quit.
    fn refresh_live(&self, id: &str) {
        let mut live = self.tunnels.live.lock().unwrap();
        let Some(l) = live.get_mut(id) else { return };
        let Some(pid) = l.process else { return };
        let Some(p) = provider(&l.provider) else {
            return;
        };
        if l.public_url.is_none() {
            l.public_url = self
                .supervisor
                .recent_output(pid)
                .iter()
                .rev()
                .find_map(|line| p.find_url(line));
            if l.public_url.is_some() && l.error.as_deref() == Some(PUBLIC_ADDRESS_TIMEOUT) {
                l.error = None;
            } else if l.public_url.is_none()
                && l.error.is_none()
                && now_ms().saturating_sub(l.started_ms) >= PUBLIC_ADDRESS_TIMEOUT_MS
            {
                l.error = Some(PUBLIC_ADDRESS_TIMEOUT.into());
            }
        }
        if !self.supervisor.is_alive(pid) && l.error.is_none() {
            let last = self
                .supervisor
                .recent_output(pid)
                .into_iter()
                .rev()
                .find(|x| !x.trim().is_empty())
                .unwrap_or_default();
            let mut msg = format!("{} stopped: {last}", p.name());
            for s in &l.secrets {
                msg = msg.replace(s.as_str(), "[redacted]");
            }
            l.error = Some(msg);
        }
    }

    pub fn tunnel_status(&self, id: &str) -> Result<TunnelStatus, CoreError> {
        let c = self
            .tunnels
            .configs
            .lock()
            .unwrap()
            .iter()
            .find(|t| t.id == id)
            .cloned()
            .ok_or_else(|| err(format!("no tunnel \"{id}\"")))?;
        self.refresh_live(id);
        let has_password = c.auth_user.as_deref().is_some_and(|u| !u.is_empty())
            && crate::secrets::get_secret(&password_key(id))
                .ok()
                .flatten()
                .is_some();
        let exposure = self.exposure(&c);
        let live = self.tunnels.live.lock().unwrap();
        Ok(match live.get(id) {
            Some(l) => TunnelStatus {
                state: if l.error.is_some() {
                    "failed"
                } else if l.public_url.is_some() {
                    "connected"
                } else {
                    "starting"
                }
                .into(),
                public_url: l.public_url.clone(),
                started_ms: Some(l.started_ms),
                requests: l.inspector.count(),
                last_request_ms: l.inspector.last_request_ms(),
                latency_ms: l.latency_ms,
                error: l.error.clone(),
                inspector_port: Some(l.inspector.port),
                has_password,
                exposure,
                config: c,
            },
            None => TunnelStatus {
                state: "stopped".into(),
                public_url: None,
                started_ms: None,
                requests: 0,
                last_request_ms: None,
                latency_ms: None,
                error: None,
                inspector_port: None,
                has_password,
                exposure,
                config: c,
            },
        })
    }

    pub fn list_tunnels(&self) -> Vec<TunnelStatus> {
        let ids: Vec<String> = self
            .tunnels
            .configs
            .lock()
            .unwrap()
            .iter()
            .map(|t| t.id.clone())
            .collect();
        ids.iter()
            .filter_map(|id| self.tunnel_status(id).ok())
            .collect()
    }

    /// §60 latency: one request through the public URL, timed.
    pub fn check_tunnel(&self, id: &str) -> Result<TunnelStatus, CoreError> {
        let url = self
            .tunnel_status(id)?
            .public_url
            .ok_or_else(|| err("the tunnel has no public address yet"))?;
        let started = std::time::Instant::now();
        let ok = self.runtimes.fetch(&url).is_ok();
        let ms = started.elapsed().as_millis() as u64;
        if let Some(l) = self.tunnels.live.lock().unwrap().get_mut(id) {
            l.latency_ms = ok.then_some(ms);
        }
        if !ok {
            return Err(err(format!("{url} did not answer through the provider")));
        }
        self.tunnel_status(id)
    }

    /// The provider's output, with tokens and passwords scrubbed (§59, §141).
    pub fn tunnel_log(&self, id: &str) -> Vec<String> {
        let live = self.tunnels.live.lock().unwrap();
        let Some(l) = live.get(id) else {
            return vec!["The tunnel is not running.".into()];
        };
        let Some(p) = l.process else { return vec![] };
        self.supervisor
            .recent_output(p)
            .into_iter()
            .map(|line| {
                let mut line = crate::logging::redact_value("line", &line);
                for s in &l.secrets {
                    line = line.replace(s.as_str(), "[redacted]");
                }
                line
            })
            .collect()
    }

    fn with_inspector<T>(
        &self,
        id: &str,
        f: impl FnOnce(&Inspector) -> Result<T, String>,
    ) -> Result<T, CoreError> {
        let live = self.tunnels.live.lock().unwrap();
        let l = live
            .get(id)
            .ok_or_else(|| err("the tunnel is not running"))?;
        f(&l.inspector).map_err(err)
    }

    pub fn tunnel_requests(&self, id: &str) -> Result<Vec<RecordedRequest>, CoreError> {
        self.with_inspector(id, |i| Ok(i.requests()))
    }

    pub fn clear_tunnel_requests(&self, id: &str) -> Result<(), CoreError> {
        self.with_inspector(id, |i| {
            i.clear();
            Ok(())
        })
    }

    pub fn replay_tunnel_request(
        &self,
        id: &str,
        request_id: u64,
    ) -> Result<RecordedRequest, CoreError> {
        // The inspector is used outside the lock: a replay can take a while.
        let inspector_port = self.with_inspector(id, |i| Ok(i.port))?;
        let _ = inspector_port;
        let live = self.tunnels.live.lock().unwrap();
        let l = live
            .get(id)
            .ok_or_else(|| err("the tunnel is not running"))?;
        l.inspector.replay(request_id).map_err(err)
    }

    pub fn send_tunnel_test(
        &self,
        id: &str,
        method: &str,
        path: &str,
        headers: &[(String, String)],
        body: &str,
    ) -> Result<RecordedRequest, CoreError> {
        let live = self.tunnels.live.lock().unwrap();
        let l = live
            .get(id)
            .ok_or_else(|| err("start the tunnel first; test requests go through its inspector"))?;
        l.inspector
            .send_test(method, path, headers, body)
            .map_err(err)
    }

    /// For setup (§73.12): finds or creates the project's tunnel and starts it. A tunnel
    /// that was never confirmed is left for the user to confirm on the Tunnels page.
    pub fn start_tunnel_for(
        &self,
        project_id: Option<&str>,
        provider_id: &str,
        target: &str,
        confirm: bool,
    ) -> Result<TunnelStatus, CoreError> {
        let existing = self
            .tunnels
            .configs
            .lock()
            .unwrap()
            .iter()
            .find(|t| {
                t.project_id.as_deref() == project_id
                    && t.provider == provider_id
                    && t.target == target
            })
            .cloned();
        let c = match existing {
            Some(c) => c,
            None => self.save_tunnel(TunnelConfig {
                id: String::new(),
                project_id: project_id.map(str::to_string),
                name: String::new(),
                provider: provider_id.into(),
                target: target.into(),
                auth_user: None,
                allow_internal: false,
                public_hostname: None,
                acknowledged: false,
                autostart: false,
            })?,
        };
        self.start_tunnel(&c.id, confirm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Core;

    fn core() -> (Core, crate::test_support::IsolatedHome) {
        let home = crate::test_support::isolated_home();
        let settings = crate::settings::SettingsService::load(&home.paths).unwrap();
        (Core::new(settings, home.paths.clone()), home)
    }

    fn config(target: &str) -> TunnelConfig {
        TunnelConfig {
            id: String::new(),
            project_id: None,
            name: "t".into(),
            provider: "mock".into(),
            target: target.into(),
            auth_user: None,
            allow_internal: false,
            public_hostname: None,
            acknowledged: false,
            autostart: false,
        }
    }

    #[test]
    fn basic_auth_matches_the_standard_encoding() {
        assert_eq!(
            basic_auth("Aladdin", "open sesame"),
            "Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ=="
        );
        assert_eq!(basic_auth("a", "b"), "Basic YTpi");
    }

    #[test]
    fn providers_find_their_public_urls() {
        assert_eq!(
            Cloudflare.find_url("INF |  https://calm-sea-12.trycloudflare.com  |"),
            Some("https://calm-sea-12.trycloudflare.com".into())
        );
        assert_eq!(
            Cloudflare.find_url("see https://www.cloudflare.com/website-terms/"),
            None
        );
        assert_eq!(
            Ngrok.find_url("t=1 lvl=info msg=\"started tunnel\" url=https://ab12.ngrok-free.app"),
            Some("https://ab12.ngrok-free.app".into())
        );
        assert_eq!(
            LocalTunnel.find_url("your url is: https://silly-cat.loca.lt"),
            Some("https://silly-cat.loca.lt".into())
        );
        assert_eq!(
            Tailscale.find_url("Available on the internet:\nhttps://pc.tail1234.ts.net/"),
            Some("https://pc.tail1234.ts.net".into())
        );
    }

    #[test]
    fn provider_tokens_never_reach_the_command_line() {
        let (args, env) = Cloudflare.command(5000, Some("sekrit"), &config("http://127.0.0.1:1"));
        assert!(!args.iter().any(|a| a.contains("sekrit")));
        assert_eq!(
            env,
            vec![("TUNNEL_TOKEN".to_string(), "sekrit".to_string())]
        );
        let (args, env) = Ngrok.command(5000, Some("sekrit"), &config("http://127.0.0.1:1"));
        assert!(!args.iter().any(|a| a.contains("sekrit")) && env[0].0 == "NGROK_AUTHTOKEN");
    }

    #[test]
    fn databases_and_unknown_hosts_are_refused_as_targets() {
        let (core, _home) = core();
        let i = core.inner();
        assert!(i
            .save_tunnel(config("http://127.0.0.1:3306"))
            .unwrap_err()
            .to_string()
            .contains("MariaDB"));
        assert!(
            i.save_tunnel(config("https://example.com")).is_err(),
            "only our own sites or loopback"
        );
        let mut ok = config("http://127.0.0.1:3306");
        ok.allow_internal = true;
        assert!(i.save_tunnel(ok).is_ok(), "an explicit opt-in allows it");
    }

    #[test]
    fn a_tunnel_needs_confirmation_before_its_first_exposure_and_stays_stoppable() {
        let (core, _home) = core();
        let i = core.inner();
        let c = i.save_tunnel(config("http://127.0.0.1:8123")).unwrap();
        assert!(!c.acknowledged);
        let s = i.start_tunnel(&c.id, false).unwrap();
        assert_eq!(s.state, "needs_confirmation");
        assert!(!i.tunnels.any_running(), "nothing started silently");

        let s = i.start_tunnel(&c.id, true).unwrap();
        assert_eq!(s.state, "connected");
        assert!(s.public_url.is_some());
        i.stop_tunnel(&c.id);
        assert_eq!(i.tunnel_status(&c.id).unwrap().state, "stopped");
        // Confirmed once: the next start needs no second confirmation.
        assert_eq!(i.start_tunnel(&c.id, false).unwrap().state, "connected");
        i.stop_all_tunnels();

        // Pointing it elsewhere asks again.
        let mut moved = i.tunnel_status(&c.id).unwrap().config;
        moved.target = "http://127.0.0.1:8124".into();
        assert!(!i.save_tunnel(moved).unwrap().acknowledged);
    }
}
