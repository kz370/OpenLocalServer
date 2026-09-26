//! The localhost HTTP API (§137, Stage 17). Off by default. When on, it listens on `127.0.0.1` only
//! and answers `POST /v1/command` with the same `CoreCommand` JSON the CLI and the app use, so
//! scripts and CI can drive OpenLocalServer without the desktop window.
//!
//! - **A bearer token is required.** Only its SHA-256 is stored; the token itself is shown once, when
//!   it is generated. Generating a new one revokes the old one immediately.
//! - **Browsers can't use it.** A request with an `Origin` header, or a `Host` that isn't loopback
//!   plus this port (DNS rebinding), is refused.
//! - **Read-only by default.** `read_only` mode allows queries only; `operate` adds a short list of
//!   operational commands (start/stop services, workers and the web server, apply the web config,
//!   install runtimes, snapshots). Everything else is refused: nothing that runs an arbitrary program,
//!   changes settings (this API's own included), reads secrets, or makes anything public is on either list.
//! - Bodies are limited to 1 MB, and failed sign-ins are slowed down.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Full, Limited};
use hyper::{Method, Request, Response, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::app::Inner;
use crate::command::{Core, CoreCommand};
use crate::error::CoreError;

const KEY: &str = "api";
pub const DEFAULT_PORT: u16 = 7420;
const MAX_BODY: usize = 1024 * 1024;

/// Commands the API never runs, whatever the mode.
const NEVER: &[&str] = &[
    "get_secret",
    "set_secret",
    "delete_secret",
    "run_command",
    "start_process",
    "set_setting",
    "get_setting",
    "get_connection_info",
    "get_process_output",
    "set_api_settings",
    "rotate_api_token",
    "clear_api_token",
    "set_custom_install",
    "remove_custom_install",
    "git_set_credentials",
    "set_tunnel_token",
    "set_tunnel_password",
    "save_custom_service",
    "remove_custom_service",
    "install_plugin",
    "set_plugin_enabled",
    "add_catalog_source",
    "install_catalog_plugin",
    "ai_save_provider",
    "ai_set_key",
    "ai_save_settings",
    "ai_remove_provider",
    "ai_start",
    "ai_apply",
    "set_updater_settings",
    "install_update",
    "install_shell_menu",
    "remove_shell_menu",
];

/// Prefixes of commands that only look.
const READ_PREFIXES: &[&str] = &["list_", "get_", "check_", "diagnose", "plan_", "health", "search", "global_search", "doctor", "ping", "git_status", "git_log", "git_diff", "git_show", "git_branches", "plugin_detect", "tunnel_log", "network_"];

/// What `operate` adds to the read-only set. A list, not a deny list: a new command is unreachable until added here.
const OPERATE: &[&str] = &[
    "start_service",
    "stop_service",
    "restart_service",
    "apply_web",
    "stop_web",
    "validate_web",
    "restart_site_app",
    "install_runtime",
    "register_project",
    "start_worker",
    "stop_worker",
    "restart_worker",
    "start_project_workers",
    "stop_project_workers",
    "run_schedule_now",
    "create_snapshot",
    "backup_database",
    "run_diagnostics",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiSettings {
    pub enabled: bool,
    pub port: u16,
    /// `read_only` or `operate`.
    pub mode: String,
}

impl Default for ApiSettings {
    fn default() -> Self {
        Self { enabled: false, port: DEFAULT_PORT, mode: "read_only".into() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiStatus {
    pub settings: ApiSettings,
    pub token_set: bool,
    pub running: bool,
    pub error: Option<String>,
    pub url: String,
}

struct Handle {
    _stop: tokio::sync::oneshot::Sender<()>,
}

#[derive(Default)]
pub struct ApiState {
    handle: Mutex<Option<Handle>>,
    error: Mutex<Option<String>>,
}

/// Whether `command` (its `type` tag) may run in `mode`.
pub fn allowed(tag: &str, mode: &str) -> bool {
    if NEVER.contains(&tag) {
        return false;
    }
    let reads = READ_PREFIXES.iter().any(|p| tag.starts_with(p)) || tag == "read_log";
    reads || (mode == "operate" && OPERATE.contains(&tag))
}

fn hash_token(token: &str) -> String {
    Sha256::digest(token.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn reply(status: StatusCode, body: Value) -> Response<Full<Bytes>> {
    let mut r = Response::new(Full::new(Bytes::from(body.to_string())));
    *r.status_mut() = status;
    r.headers_mut().insert(hyper::header::CONTENT_TYPE, "application/json".parse().unwrap());
    r.headers_mut().insert("cache-control", "no-store".parse().unwrap());
    r
}

fn error(status: StatusCode, problem: &str, cause: &str) -> Response<Full<Bytes>> {
    reply(status, json!({ "ok": false, "error": { "problem": problem, "cause": cause, "fix": null } }))
}

async fn handle(req: Request<hyper::body::Incoming>, core: Core, port: u16) -> Result<Response<Full<Bytes>>, Infallible> {
    // Browsers send Origin; scripts don't. Refusing it keeps web pages out.
    if req.headers().contains_key(hyper::header::ORIGIN) {
        return Ok(error(StatusCode::FORBIDDEN, "Not allowed.", "Requests from web pages are refused."));
    }
    let host_ok = req.headers().get(hyper::header::HOST).and_then(|h| h.to_str().ok()).is_some_and(|h| {
        let h = h.to_ascii_lowercase();
        [format!("127.0.0.1:{port}"), format!("localhost:{port}"), format!("[::1]:{port}")].contains(&h)
    });
    if !host_ok {
        return Ok(error(StatusCode::FORBIDDEN, "Not allowed.", "The Host header must be 127.0.0.1 or localhost with this port."));
    }
    let inner = core.inner().clone();
    let settings = inner.api_settings();
    let expected = inner.api_token_hash();
    let given = req.headers().get(hyper::header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer ")).map(str::trim);
    let authorised = match (given, expected) {
        (Some(t), Some(h)) => same(&hash_token(t), &h),
        _ => false,
    };
    if !authorised {
        tokio::time::sleep(Duration::from_millis(300)).await;
        return Ok(error(StatusCode::UNAUTHORIZED, "Not signed in.", "Send the API token as `Authorization: Bearer <token>`."));
    }
    let path = req.uri().path().to_string();
    match (req.method().clone(), path.as_str()) {
        (Method::GET, "/v1/ping") => Ok(reply(StatusCode::OK, json!({ "ok": true, "result": { "type": "pong", "version": env!("CARGO_PKG_VERSION") }, "mode": settings.mode }))),
        (Method::POST, "/v1/command") => {
            let content_ok = req.headers().get(hyper::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).is_some_and(|v| v.starts_with("application/json"));
            if !content_ok {
                return Ok(error(StatusCode::UNSUPPORTED_MEDIA_TYPE, "That request couldn't be read.", "Send Content-Type: application/json."));
            }
            let body = match Limited::new(req.into_body(), MAX_BODY).collect().await {
                Ok(b) => b.to_bytes(),
                Err(_) => return Ok(error(StatusCode::PAYLOAD_TOO_LARGE, "That request couldn't be read.", "The body is larger than 1 MB.")),
            };
            let value: Value = match serde_json::from_slice(&body) {
                Ok(v) => v,
                Err(e) => return Ok(error(StatusCode::BAD_REQUEST, "That request couldn't be read.", &e.to_string())),
            };
            let tag = value.get("type").and_then(|t| t.as_str()).unwrap_or("").to_string();
            if !allowed(&tag, &settings.mode) {
                let why = if NEVER.contains(&tag.as_str()) { "This command isn't available over the API." } else { "The API is read-only. Switch it to operate mode in Settings to allow this." };
                return Ok(error(StatusCode::FORBIDDEN, "Not allowed.", why));
            }
            let command: CoreCommand = match serde_json::from_value(value) {
                Ok(c) => c,
                Err(e) => return Ok(error(StatusCode::BAD_REQUEST, "That command isn't valid.", &e.to_string())),
            };
            tracing::info!(command = %tag, "api command");
            let result = tokio::task::spawn_blocking(move || core.dispatch(command)).await;
            Ok(match result {
                Ok(Ok(r)) => reply(StatusCode::OK, json!({ "ok": true, "result": r })),
                Ok(Err(d)) => reply(StatusCode::UNPROCESSABLE_ENTITY, json!({ "ok": false, "error": d })),
                Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, "The command crashed.", &e.to_string()),
            })
        }
        _ => Ok(error(StatusCode::NOT_FOUND, "Not found.", "Use GET /v1/ping or POST /v1/command.")),
    }
}

/// Binds and serves on a thread of its own; returns the port and a handle whose drop stops it.
fn serve(core: Core, addr: SocketAddr) -> Result<(u16, Handle), String> {
    let listener = std::net::TcpListener::bind(addr).map_err(|e| format!("port {} isn't available: {e}", addr.port()))?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let (tx, mut rx) = tokio::sync::oneshot::channel::<()>();
    std::thread::Builder::new()
        .name("ols-api".into())
        .spawn(move || {
            let Ok(rt) = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build() else { return };
            rt.block_on(async move {
                let Ok(listener) = tokio::net::TcpListener::from_std(listener) else { return };
                loop {
                    tokio::select! {
                        _ = &mut rx => break,
                        accepted = listener.accept() => {
                            let Ok((stream, _)) = accepted else { continue };
                            let core = core.clone();
                            tokio::spawn(async move {
                                let svc = hyper::service::service_fn(move |req| handle(req, core.clone(), port));
                                let conn = hyper::server::conn::http1::Builder::new().serve_connection(hyper_util::rt::TokioIo::new(stream), svc);
                                let _ = tokio::time::timeout(Duration::from_secs(120), conn).await;
                            });
                        }
                    }
                }
            });
        })
        .map_err(|e| e.to_string())?;
    Ok((port, Handle { _stop: tx }))
}

impl Inner {
    pub fn api_settings(&self) -> ApiSettings {
        self.settings.lock().unwrap().get(KEY).and_then(|v| serde_json::from_value(v.get("settings")?.clone()).ok()).unwrap_or_default()
    }

    fn api_token_hash(&self) -> Option<String> {
        self.settings.lock().unwrap().get(KEY).and_then(|v| v.get("token_hash")?.as_str().map(str::to_string)).filter(|h| !h.is_empty())
    }

    fn save_api(&self, settings: &ApiSettings, token_hash: Option<String>) -> Result<(), CoreError> {
        let mut s = self.settings.lock().unwrap();
        s.set(KEY.to_string(), json!({ "settings": settings, "token_hash": token_hash.unwrap_or_default() }))
    }

    pub fn api_status(&self) -> ApiStatus {
        let settings = self.api_settings();
        ApiStatus {
            url: format!("http://127.0.0.1:{}", settings.port),
            token_set: self.api_token_hash().is_some(),
            running: self.api.handle.lock().unwrap().is_some(),
            error: self.api.error.lock().unwrap().clone(),
            settings,
        }
    }

    /// Starts or stops the server to match the settings. Call at startup and after changing them.
    pub fn apply_api(self: &Arc<Self>) {
        *self.api.handle.lock().unwrap() = None;
        *self.api.error.lock().unwrap() = None;
        let settings = self.api_settings();
        if !settings.enabled {
            return;
        }
        if self.api_token_hash().is_none() {
            *self.api.error.lock().unwrap() = Some("Generate an API token first.".into());
            return;
        }
        match serve(Core::from_inner(self.clone()), SocketAddr::from(([127, 0, 0, 1], settings.port))) {
            Ok((_, handle)) => {
                *self.api.handle.lock().unwrap() = Some(handle);
                tracing::info!(port = settings.port, mode = %settings.mode, "local API listening");
            }
            Err(e) => *self.api.error.lock().unwrap() = Some(e),
        }
    }

    pub fn set_api_settings(self: &Arc<Self>, enabled: bool, port: u16, mode: &str) -> Result<ApiStatus, CoreError> {
        if port < 1024 {
            return Err(CoreError::failed("The API port wasn't changed.", "Use a port from 1024 up."));
        }
        if !matches!(mode, "read_only" | "operate") {
            return Err(CoreError::failed("The API mode wasn't changed.", "The mode is read_only or operate."));
        }
        self.save_api(&ApiSettings { enabled, port, mode: mode.into() }, self.api_token_hash())?;
        self.apply_api();
        Ok(self.api_status())
    }

    /// A new token. Only its hash is kept, so this is the one time it can be read.
    pub fn rotate_api_token(self: &Arc<Self>) -> Result<String, CoreError> {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).map_err(|e| CoreError::failed("No token was made.", e.to_string()))?;
        let token = format!("ols_{}", bytes.iter().map(|b| format!("{b:02x}")).collect::<String>());
        self.save_api(&self.api_settings(), Some(hash_token(&token)))?;
        self.apply_api();
        Ok(token)
    }

    pub fn clear_api_token(self: &Arc<Self>) -> Result<(), CoreError> {
        self.save_api(&self.api_settings(), None)?;
        self.apply_api();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    fn request(port: u16, head: &str, body: &str) -> (u16, String) {
        let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        write!(s, "{head}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        let status = out.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
        (status, out)
    }

    #[test]
    fn modes_and_the_never_list() {
        assert!(allowed("list_services", "read_only"));
        assert!(!allowed("stop_web", "read_only"));
        assert!(allowed("stop_web", "operate"));
        // Anything that runs code, changes settings or reads secrets stays out, even in operate mode.
        for tag in ["get_secret", "run_command", "set_setting", "rotate_api_token", "run_in_project", "open_terminal", "save_schedule", "run_quick_command", "apply_setup", "start_tunnel", "get_connection_info", "read_env_file"] {
            assert!(!allowed(tag, "operate"), "{tag}");
        }
    }

    #[test]
    fn the_server_checks_token_host_origin_and_mode() {
        let home = crate::test_support::isolated_home();
        let core = Core::new(crate::settings::SettingsService::load(&home.paths).unwrap(), home.paths.clone());
        let inner = core.inner().clone();
        let token = inner.rotate_api_token().unwrap();
        let (port, _handle) = serve(core.clone(), SocketAddr::from(([127, 0, 0, 1], 0))).unwrap();
        let auth = format!("Authorization: Bearer {token}");
        let host = format!("Host: 127.0.0.1:{port}");
        let json = "Content-Type: application/json";

        let (status, _) = request(port, &format!("GET /v1/ping HTTP/1.1\r\n{host}"), "");
        assert_eq!(status, 401, "no token");
        let (status, _) = request(port, &format!("GET /v1/ping HTTP/1.1\r\n{host}\r\nAuthorization: Bearer wrong"), "");
        assert_eq!(status, 401, "wrong token");
        let (status, body) = request(port, &format!("GET /v1/ping HTTP/1.1\r\n{host}\r\n{auth}"), "");
        assert_eq!(status, 200);
        assert!(body.contains("pong"));
        let (status, _) = request(port, &format!("GET /v1/ping HTTP/1.1\r\nHost: evil.example:{port}\r\n{auth}"), "");
        assert_eq!(status, 403, "rebinding");
        let (status, _) = request(port, &format!("GET /v1/ping HTTP/1.1\r\n{host}\r\n{auth}\r\nOrigin: https://evil.example"), "");
        assert_eq!(status, 403, "browser");

        let (status, body) = request(port, &format!("POST /v1/command HTTP/1.1\r\n{host}\r\n{auth}\r\n{json}"), r#"{"type":"list_projects"}"#);
        assert_eq!(status, 200, "{body}");
        let (status, _) = request(port, &format!("POST /v1/command HTTP/1.1\r\n{host}\r\n{auth}\r\n{json}"), r#"{"type":"stop_web"}"#);
        assert_eq!(status, 403, "read-only");
        let (status, _) = request(port, &format!("POST /v1/command HTTP/1.1\r\n{host}\r\n{auth}\r\n{json}"), r#"{"type":"get_secret","key":"x"}"#);
        assert_eq!(status, 403, "never");

        // A new token revokes the old one at once.
        let _new = inner.rotate_api_token().unwrap();
        let (status, _) = request(port, &format!("GET /v1/ping HTTP/1.1\r\n{host}\r\n{auth}"), "");
        assert_eq!(status, 401);
    }
}
