//! Request and traffic inspector (§110) and webhook tester (§111).
//!
//! Every tunnel points its provider at a small local proxy instead of at the site itself:
//!
//! ```text
//! public URL → provider → 127.0.0.1:<inspector> → https://shop.test (the real site)
//! ```
//!
//! The proxy forwards each request unchanged (Host set to the site's name, trusting the
//! local CA) and records method, path, status, timing, sizes and headers. Recorded values
//! are **redacted** before anything can read them (cookies, authorization, tokens,
//! passwords, signatures); only a replay uses the original request, and that never leaves
//! this process. Bodies are buffered, so streaming responses and WebSockets aren't
//! supported through a tunnel yet.

use std::collections::VecDeque;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{Request, Response};
use serde::{Deserialize, Serialize};

/// Requests kept per tunnel.
const KEEP: usize = 500;
/// Larger request bodies are refused (webhooks are small).
const MAX_BODY: usize = 50 * 1024 * 1024;
/// Shown in the inspector; the rest is summarised.
const PREVIEW: usize = 16 * 1024;

const HOP_BY_HOP: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
    "host",
    "content-length",
];
const SECRET_HEADERS: &[&str] = &[
    "authorization",
    "proxy-authorization",
    "cookie",
    "set-cookie",
    "x-api-key",
    "x-auth-token",
    "x-csrf-token",
    "x-xsrf-token",
];
const SECRET_WORDS: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "token",
    "apikey",
    "api_key",
    "signature",
    "auth",
    "session",
    "key",
    "otp",
    "credential",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordedRequest {
    pub id: u64,
    pub time_ms: u64,
    pub method: String,
    /// Path and query, secrets in the query redacted.
    pub path: String,
    pub status: u16,
    pub duration_ms: u64,
    pub request_headers: Vec<(String, String)>,
    pub response_headers: Vec<(String, String)>,
    pub request_size: u64,
    pub response_size: u64,
    /// Text bodies (redacted), cut to a preview.
    pub request_body: Option<String>,
    pub response_body: Option<String>,
    /// Where it came from, per the provider's X-Forwarded-For.
    pub client: Option<String>,
    /// Sent from the webhook tester or replayed, not from the internet.
    pub replay: bool,
    pub error: Option<String>,
}

/// The unredacted request, for replays only (never serialized).
#[derive(Clone)]
struct Original {
    method: hyper::Method,
    path: String,
    headers: hyper::HeaderMap,
    body: Bytes,
}

pub fn is_secret_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    SECRET_HEADERS.contains(&n.as_str()) || SECRET_WORDS.iter().any(|w| n.contains(w))
}

pub fn redact_query(path: &str) -> String {
    let Some((base, query)) = path.split_once('?') else {
        return path.to_string();
    };
    let parts: Vec<String> = query
        .split('&')
        .map(|kv| match kv.split_once('=') {
            Some((k, _)) if is_secret_name(k) => format!("{k}=[redacted]"),
            _ => kv.to_string(),
        })
        .collect();
    format!("{base}?{}", parts.join("&"))
}

/// Redacts `"password": "..."` in JSON and `password=...` in forms.
pub fn redact_body(text: &str) -> String {
    static JSON: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    static FORM: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let json = JSON.get_or_init(|| regex::Regex::new(r#"(?i)"([^"]*(?:password|passwd|secret|token|api_?key|signature|auth|session|otp|credential)[^"]*)"\s*:\s*"(?:[^"\\]|\\.)*""#).unwrap());
    let form = FORM.get_or_init(|| regex::Regex::new(r"(?i)(^|&)([^=&]*(?:password|passwd|secret|token|api_?key|signature|auth|session|otp|credential)[^=&]*)=[^&]*").unwrap());
    let out = json.replace_all(text, r#""$1": "[redacted]""#);
    form.replace_all(&out, "$1$2=[redacted]").to_string()
}

fn headers_view(h: &hyper::HeaderMap) -> Vec<(String, String)> {
    h.iter()
        .map(|(k, v)| {
            let name = k.as_str().to_string();
            let value = if is_secret_name(&name) {
                "[redacted]".to_string()
            } else {
                String::from_utf8_lossy(v.as_bytes()).to_string()
            };
            (name, value)
        })
        .collect()
}

fn body_preview(h: &hyper::HeaderMap, body: &[u8]) -> Option<String> {
    if body.is_empty() {
        return None;
    }
    let ct = h
        .get(hyper::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    let texty = ct.is_empty()
        || ct.contains("json")
        || ct.contains("text")
        || ct.contains("xml")
        || ct.contains("x-www-form-urlencoded")
        || ct.contains("javascript");
    if !texty {
        return Some(format!("[{} bytes of {ct}]", body.len()));
    }
    let cut = &body[..body.len().min(PREVIEW)];
    let mut text = redact_body(&String::from_utf8_lossy(cut));
    if body.len() > PREVIEW {
        text.push_str(&format!("\n… {} more bytes", body.len() - PREVIEW));
    }
    Some(text)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Where the proxy sends requests.
#[derive(Clone, Debug)]
pub struct Target {
    /// "https://shop.test" or "http://127.0.0.1:8000".
    pub base: String,
    /// The site's name, sent as Host.
    pub host: String,
}

struct State {
    target: Target,
    client: reqwest::Client,
    log: Mutex<VecDeque<(RecordedRequest, Original)>>,
    next: Mutex<u64>,
    last_ms: Mutex<Option<u64>>,
    /// `Basic ...` a visitor must send (the tunnel's optional access control).
    auth: Mutex<Option<String>>,
}

/// One running inspector proxy.
pub struct Inspector {
    pub port: u16,
    state: Arc<State>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    runtime: Arc<tokio::runtime::Runtime>,
}

impl Inspector {
    /// Starts the proxy on a free loopback port. `resolve` maps the site's name to our own
    /// web server (so `.test` names work without the system resolver); `ca_pem` is the
    /// local CA, trusted for the forwarded HTTPS request.
    pub fn start(
        runtime: Arc<tokio::runtime::Runtime>,
        target: Target,
        resolve: Option<SocketAddr>,
        ca_pem: Option<Vec<u8>>,
    ) -> Result<Self, String> {
        let mut builder = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(120));
        if let Some(addr) = resolve {
            builder = builder.resolve(&target.host, addr);
        }
        if let Some(pem) = ca_pem {
            if let Ok(cert) = reqwest::Certificate::from_pem(&pem) {
                builder = builder.add_root_certificate(cert);
            }
        }
        let client = builder.build().map_err(|e| e.to_string())?;
        let state = Arc::new(State {
            target,
            client,
            log: Mutex::new(VecDeque::new()),
            next: Mutex::new(1),
            last_ms: Mutex::new(None),
            auth: Mutex::new(None),
        });
        // Bound here (not with `block_on`): callers may already be inside another runtime.
        let std_listener = std::net::TcpListener::bind(("127.0.0.1", 0))
            .map_err(|e| format!("the inspector could not open a port: {e}"))?;
        std_listener
            .set_nonblocking(true)
            .map_err(|e| e.to_string())?;
        let port = std_listener.local_addr().map_err(|e| e.to_string())?.port();
        let (tx, mut rx) = tokio::sync::oneshot::channel::<()>();
        let serve_state = state.clone();
        runtime.spawn(async move {
            let Ok(listener) = tokio::net::TcpListener::from_std(std_listener) else { return };
            loop {
                tokio::select! {
                    _ = &mut rx => break,
                    accepted = listener.accept() => {
                        let Ok((stream, _)) = accepted else { continue };
                        let st = serve_state.clone();
                        tokio::spawn(async move {
                            let svc = hyper::service::service_fn(move |req| handle(req, st.clone(), false));
                            let _ = hyper::server::conn::http1::Builder::new().serve_connection(hyper_util::rt::TokioIo::new(stream), svc).await;
                        });
                    }
                }
            }
        });
        Ok(Self {
            port,
            state,
            shutdown: Some(tx),
            runtime,
        })
    }

    /// Visitors must send this `Authorization` value; others get 401 (never recorded).
    pub fn require_auth(&mut self, basic: String) {
        *self.state.auth.lock().unwrap() = Some(basic);
    }

    pub fn requests(&self) -> Vec<RecordedRequest> {
        self.state
            .log
            .lock()
            .unwrap()
            .iter()
            .rev()
            .map(|(r, _)| r.clone())
            .collect()
    }

    pub fn clear(&self) {
        self.state.log.lock().unwrap().clear();
    }

    pub fn last_request_ms(&self) -> Option<u64> {
        *self.state.last_ms.lock().unwrap()
    }

    pub fn count(&self) -> usize {
        self.state.log.lock().unwrap().len()
    }

    /// §111: sends a recorded request again, exactly as it first arrived.
    pub fn replay(&self, id: u64) -> Result<RecordedRequest, String> {
        let original = self
            .state
            .log
            .lock()
            .unwrap()
            .iter()
            .find(|(r, _)| r.id == id)
            .map(|(_, o)| o.clone())
            .ok_or("that request is no longer in the log")?;
        self.send(original)
    }

    /// §111: a hand-made request (the webhook tester), recorded like real traffic.
    pub fn send_test(
        &self,
        method: &str,
        path: &str,
        headers: &[(String, String)],
        body: &str,
    ) -> Result<RecordedRequest, String> {
        let method = hyper::Method::from_bytes(method.to_ascii_uppercase().as_bytes())
            .map_err(|_| format!("\"{method}\" is not an HTTP method"))?;
        let path = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        };
        let mut map = hyper::HeaderMap::new();
        for (k, v) in headers {
            let name = hyper::header::HeaderName::from_bytes(k.trim().as_bytes())
                .map_err(|_| format!("\"{k}\" is not a header name"))?;
            let value = hyper::header::HeaderValue::from_str(v.trim())
                .map_err(|_| format!("the value of {k} is not allowed in a header"))?;
            map.append(name, value);
        }
        self.send(Original {
            method,
            path,
            headers: map,
            body: Bytes::from(body.to_string()),
        })
    }

    fn send(&self, o: Original) -> Result<RecordedRequest, String> {
        let mut req = Request::builder().method(o.method).uri(o.path);
        for (k, v) in o.headers.iter() {
            req = req.header(k, v);
        }
        let req = req.body(Full::new(o.body)).map_err(|e| e.to_string())?;
        let state = self.state.clone();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        self.runtime.spawn(async move {
            let (parts, body) = req.into_parts();
            let bytes = body
                .collect()
                .await
                .map(|b| b.to_bytes())
                .unwrap_or_default();
            let _ = forward(Request::from_parts(parts, bytes), state, true).await;
            let _ = done_tx.send(());
        });
        done_rx
            .recv_timeout(std::time::Duration::from_secs(130))
            .map_err(|_| "the local site did not answer in time".to_string())?;
        self.state
            .log
            .lock()
            .unwrap()
            .back()
            .map(|(r, _)| r.clone())
            .ok_or_else(|| "nothing was recorded".to_string())
    }
}

impl Drop for Inspector {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

async fn handle(
    req: Request<hyper::body::Incoming>,
    state: Arc<State>,
    replay: bool,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let wanted = state.auth.lock().unwrap().clone();
    if let Some(wanted) = wanted {
        let given = req
            .headers()
            .get(hyper::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok());
        if given != Some(wanted.as_str()) {
            let r = Response::builder()
                .status(401)
                .header("www-authenticate", "Basic realm=\"OpenLocalServer tunnel\"")
                .body(Full::new(Bytes::from_static(
                    b"This tunnel needs a username and password.",
                )))
                .unwrap();
            return Ok(r);
        }
    }
    let (parts, body) = req.into_parts();
    let bytes = match body.collect().await {
        Ok(b) => b.to_bytes(),
        Err(e) => return Ok(simple(400, &format!("could not read the request: {e}"))),
    };
    if bytes.len() > MAX_BODY {
        return Ok(simple(413, "request body too large for the inspector"));
    }
    Ok(forward(Request::from_parts(parts, bytes), state, replay).await)
}

fn simple(status: u16, text: &str) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header("content-type", "text/plain; charset=utf-8")
        .body(Full::new(Bytes::from(text.to_string())))
        .unwrap()
}

async fn forward(req: Request<Bytes>, state: Arc<State>, replay: bool) -> Response<Full<Bytes>> {
    let started = Instant::now();
    let path = req
        .uri()
        .path_and_query()
        .map(|p| p.as_str().to_string())
        .unwrap_or_else(|| "/".into());
    let original = Original {
        method: req.method().clone(),
        path: path.clone(),
        headers: req.headers().clone(),
        body: req.body().clone(),
    };
    let client_ip = req
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(',').next().unwrap_or(s).trim().to_string());

    let url = format!("{}{}", state.target.base.trim_end_matches('/'), path);
    let method = reqwest::Method::from_bytes(req.method().as_str().as_bytes())
        .unwrap_or(reqwest::Method::GET);
    let mut out = state.client.request(method, &url);
    for (k, v) in req.headers() {
        if !HOP_BY_HOP.contains(&k.as_str()) {
            out = out.header(k.as_str(), v.as_bytes());
        }
    }
    let public_host = req
        .headers()
        .get("x-forwarded-host")
        .or_else(|| req.headers().get(hyper::header::HOST))
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    out = out
        .header("host", state.target.host.as_str())
        .header("x-forwarded-proto", "https");
    if let Some(h) = &public_host {
        out = out.header("x-forwarded-host", h.as_str());
    }
    let result = out.body(req.body().clone()).send().await;

    let id = {
        let mut n = state.next.lock().unwrap();
        let id = *n;
        *n += 1;
        id
    };
    let mut record = RecordedRequest {
        id,
        time_ms: now_ms(),
        method: req.method().to_string(),
        path: redact_query(&path),
        status: 0,
        duration_ms: 0,
        request_headers: headers_view(req.headers()),
        response_headers: Vec::new(),
        request_size: req.body().len() as u64,
        response_size: 0,
        request_body: body_preview(req.headers(), req.body()),
        response_body: None,
        client: client_ip,
        replay,
        error: None,
    };

    let response = match result {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let mut headers = hyper::HeaderMap::new();
            for (k, v) in resp.headers() {
                if !HOP_BY_HOP.contains(&k.as_str()) {
                    if let (Ok(name), Ok(value)) = (
                        hyper::header::HeaderName::from_bytes(k.as_str().as_bytes()),
                        hyper::header::HeaderValue::from_bytes(v.as_bytes()),
                    ) {
                        headers.append(name, value);
                    }
                }
            }
            let body = resp.bytes().await.unwrap_or_default();
            record.status = status;
            record.response_headers = headers_view(&headers);
            record.response_size = body.len() as u64;
            record.response_body = body_preview(&headers, &body);
            let mut r = Response::builder().status(status);
            if let Some(h) = r.headers_mut() {
                *h = headers;
            }
            r.body(Full::new(body))
                .unwrap_or_else(|_| simple(502, "bad response from the site"))
        }
        Err(e) => {
            let msg = format!("the local site did not answer: {e}");
            record.status = 502;
            record.error = Some(msg.clone());
            simple(502, &msg)
        }
    };
    record.duration_ms = started.elapsed().as_millis() as u64;
    *state.last_ms.lock().unwrap() = Some(record.time_ms);
    let mut log = state.log.lock().unwrap();
    log.push_back((record, original));
    while log.len() > KEEP {
        log.pop_front();
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn secrets_are_redacted_everywhere_they_show() {
        assert_eq!(
            redact_query("/cb?code=1&access_token=abc&state=x"),
            "/cb?code=1&access_token=[redacted]&state=x"
        );
        let json = redact_body(r#"{"user":"a","password":"hunter2","nested":{"api_key":"k"}}"#);
        assert!(
            !json.contains("hunter2") && !json.contains("\"k\"") && json.contains("\"user\":\"a\""),
            "{json}"
        );
        assert_eq!(
            redact_body("user=a&password=b&x=1"),
            "user=a&password=[redacted]&x=1"
        );
        let mut h = hyper::HeaderMap::new();
        h.insert("cookie", "sid=1".parse().unwrap());
        h.insert("stripe-signature", "t=1,v1=abc".parse().unwrap());
        h.insert("accept", "*/*".parse().unwrap());
        let v = headers_view(&h);
        assert!(v.iter().filter(|(_, v)| v == "[redacted]").count() == 2);
        assert!(v.iter().any(|(k, v)| k == "accept" && v == "*/*"));
    }

    /// A real round trip: a tiny HTTP server stands in for the site.
    #[test]
    fn requests_are_forwarded_recorded_and_replayable() {
        let site = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let site_port = site.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in site.incoming().flatten().take(3) {
                let mut s = stream;
                let mut buf = [0u8; 4096];
                let n = s.read(&mut buf).unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).to_string();
                let host_ok = head.to_ascii_lowercase().contains("host: shop.test");
                let body = if host_ok { "hello" } else { "wrong host" };
                let _ = write!(s, "HTTP/1.1 201 Created\r\ncontent-type: text/plain\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len());
            }
        });
        let rt = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .enable_all()
                .build()
                .unwrap(),
        );
        let inspector = Inspector::start(
            rt,
            Target {
                base: format!("http://127.0.0.1:{site_port}"),
                host: "shop.test".into(),
            },
            None,
            None,
        )
        .unwrap();

        let mut c = std::net::TcpStream::connect(("127.0.0.1", inspector.port)).unwrap();
        write!(c, "POST /hook?token=secret HTTP/1.1\r\nhost: abc.trycloudflare.com\r\ncontent-type: application/json\r\ncontent-length: 20\r\nconnection: close\r\n\r\n{{\"password\":\"x1234\"}}").unwrap();
        let mut resp = String::new();
        c.read_to_string(&mut resp).unwrap();
        assert!(
            resp.starts_with("HTTP/1.1 201") && resp.ends_with("hello"),
            "{resp}"
        );

        let reqs = inspector.requests();
        assert_eq!(reqs.len(), 1);
        let r = &reqs[0];
        assert_eq!((r.method.as_str(), r.status), ("POST", 201));
        assert_eq!(r.path, "/hook?token=[redacted]");
        assert!(!r.request_body.as_ref().unwrap().contains("x1234"));
        assert_eq!(r.response_body.as_deref(), Some("hello"));

        let again = inspector.replay(r.id).unwrap();
        assert!(again.replay && again.status == 201);
        let test = inspector.send_test("get", "ping", &[], "").unwrap();
        assert_eq!(test.path, "/ping");
        assert_eq!(inspector.count(), 3);
    }
}
