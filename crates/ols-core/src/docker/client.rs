//! A small, blocking HTTP/1.1 client for the Docker Engine API (§166).
//!
//! The engine listens on a named pipe on Windows (`\\.\pipe\docker_engine`), a Unix socket
//! elsewhere, or a TCP port when `DOCKER_HOST` points at one. Every request opens its own
//! connection with `Connection: close`, so the whole response is "read until the engine
//! hangs up" — no keep-alive pool, no background connection left open.
//!
//! Every request runs on a short-lived worker thread and the caller waits with a hard
//! timeout. A Docker engine that hangs can therefore never freeze a `CoreCommand`; the
//! worker thread ends on its own when the engine finally answers or the pipe closes.

use std::io::{Read, Write};
use std::sync::mpsc;
use std::time::Duration;

/// Where the Docker engine listens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Endpoint {
    /// A Windows named pipe, e.g. `\\.\pipe\docker_engine`.
    NamedPipe(String),
    /// A Unix socket path, e.g. `/var/run/docker.sock`.
    Unix(String),
    /// `host:port` over plain TCP (`tcp://127.0.0.1:2375`).
    Tcp(String),
}

impl Endpoint {
    /// The platform default: Docker Desktop's pipe on Windows, the standard socket elsewhere.
    pub fn platform_default() -> Self {
        if cfg!(windows) {
            Endpoint::NamedPipe(r"\\.\pipe\docker_engine".into())
        } else {
            Endpoint::Unix("/var/run/docker.sock".into())
        }
    }

    /// Parses the forms Docker itself uses for `DOCKER_HOST`:
    /// `npipe:////./pipe/docker_engine`, `\\.\pipe\name`, `unix:///path`, `tcp://host:port`.
    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        if raw.is_empty() {
            return None;
        }
        if let Some(rest) = raw.strip_prefix("npipe://") {
            // `npipe:////./pipe/docker_engine` -> `\\.\pipe\docker_engine`
            let path = rest.trim_start_matches('/').replace('/', "\\");
            let name = path.strip_prefix(".\\").unwrap_or(&path);
            if name.is_empty() {
                return None;
            }
            return Some(Endpoint::NamedPipe(format!(r"\\.\{name}")));
        }
        if raw.starts_with(r"\\.\pipe\") {
            return Some(Endpoint::NamedPipe(raw.to_string()));
        }
        if let Some(path) = raw.strip_prefix("unix://") {
            return (!path.is_empty()).then(|| Endpoint::Unix(path.to_string()));
        }
        if let Some(addr) = raw.strip_prefix("tcp://") {
            let addr = addr.trim_end_matches('/');
            // A real `host:port`, nothing that could smuggle a path or a scheme.
            let ok = addr.rsplit_once(':').is_some_and(|(host, port)| {
                !host.is_empty()
                    && port.parse::<u16>().is_ok_and(|p| p > 0)
                    && host.chars().all(|c| {
                        c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '[' | ']' | ':')
                    })
            });
            return ok.then(|| Endpoint::Tcp(addr.to_string()));
        }
        None
    }

    /// How the endpoint is shown to the user (status bar, error causes).
    pub fn display(&self) -> String {
        match self {
            Endpoint::NamedPipe(p) => p.clone(),
            Endpoint::Unix(p) => format!("unix://{p}"),
            Endpoint::Tcp(a) => format!("tcp://{a}"),
        }
    }
}

/// What went wrong talking to the engine. Turned into a `Diagnostic` by `docker::error`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientError {
    /// Nothing is listening: Docker is stopped or not installed.
    NotRunning(String),
    /// The engine accepted the connection but did not answer within the timeout.
    Timeout(Duration),
    /// The engine answered with an HTTP error; `message` is its own explanation.
    Http { status: u16, message: String },
    /// The connection broke or the answer was not valid HTTP / JSON.
    Protocol(String),
}

/// A complete engine response.
#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

impl Response {
    pub fn json<T: serde::de::DeserializeOwned>(&self) -> Result<T, ClientError> {
        serde_json::from_slice(&self.body)
            .map_err(|e| ClientError::Protocol(format!("unexpected answer from Docker: {e}")))
    }
}

/// Responses bigger than this are refused (a container list is a few KB; this is a guard
/// against a misbehaving endpoint filling memory).
const MAX_RESPONSE: usize = 16 * 1024 * 1024;

/// Sends one request and waits at most `timeout` for the full answer.
/// Statuses 200-299 and 304 ("already in that state") are success; anything else is
/// `ClientError::Http` carrying the engine's message.
pub fn request(
    endpoint: &Endpoint,
    method: &str,
    path: &str,
    body: Option<Vec<u8>>,
    timeout: Duration,
) -> Result<Response, ClientError> {
    let raw = build_request(method, path, body.as_deref());
    let endpoint = endpoint.clone();
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("ols-docker-request".into())
        .spawn(move || {
            let _ = tx.send(exchange(&endpoint, &raw, timeout));
        })
        .map_err(|e| ClientError::Protocol(format!("could not start a request thread: {e}")))?;
    let response = match rx.recv_timeout(timeout) {
        Ok(result) => result?,
        Err(mpsc::RecvTimeoutError::Timeout) => return Err(ClientError::Timeout(timeout)),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            return Err(ClientError::Protocol("the request thread stopped".into()))
        }
    };
    if (200..300).contains(&response.status) || response.status == 304 {
        Ok(response)
    } else {
        let message = serde_json::from_slice::<serde_json::Value>(&response.body)
            .ok()
            .and_then(|v| {
                v.get("message")
                    .and_then(|m| m.as_str())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| String::from_utf8_lossy(&response.body).trim().to_string());
        Err(ClientError::Http {
            status: response.status,
            message,
        })
    }
}

pub(crate) fn build_request(method: &str, path: &str, body: Option<&[u8]>) -> Vec<u8> {
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nHost: docker\r\nUser-Agent: OpenLocalServer\r\nAccept: application/json\r\nConnection: close\r\n"
    );
    match body {
        Some(b) => {
            head.push_str("Content-Type: application/json\r\n");
            head.push_str(&format!("Content-Length: {}\r\n\r\n", b.len()));
            let mut out = head.into_bytes();
            out.extend_from_slice(b);
            out
        }
        None => {
            head.push_str("Content-Length: 0\r\n\r\n");
            head.into_bytes()
        }
    }
}

fn exchange(endpoint: &Endpoint, raw: &[u8], timeout: Duration) -> Result<Response, ClientError> {
    let mut stream = connect(endpoint, timeout)?;
    stream
        .write_all(raw)
        .and_then(|_| stream.flush())
        .map_err(|e| ClientError::Protocol(format!("could not send the request: {e}")))?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 16 * 1024];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if buf.len() > MAX_RESPONSE {
                    return Err(ClientError::Protocol(
                        "Docker's answer was too large".into(),
                    ));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            // A named pipe reports the engine hanging up as "broken pipe" rather than EOF.
            Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe && !buf.is_empty() => break,
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                return Err(ClientError::Timeout(timeout))
            }
            Err(e) => {
                return Err(ClientError::Protocol(format!(
                    "reading Docker's answer: {e}"
                )))
            }
        }
    }
    parse_response(&buf)
}

trait Stream: Read + Write + Send {}
impl<T: Read + Write + Send> Stream for T {}

fn not_running(endpoint: &Endpoint, e: &std::io::Error) -> ClientError {
    ClientError::NotRunning(format!("{} ({e})", endpoint.display()))
}

fn connect(endpoint: &Endpoint, timeout: Duration) -> Result<Box<dyn Stream>, ClientError> {
    connect_with(endpoint, timeout, timeout)
}

/// Like `connect`, with a separate socket read timeout (named pipes have none).
fn connect_with(
    endpoint: &Endpoint,
    timeout: Duration,
    read_timeout: Duration,
) -> Result<Box<dyn Stream>, ClientError> {
    use std::io::ErrorKind as K;
    let is_absent = |e: &std::io::Error| {
        matches!(
            e.kind(),
            K::NotFound | K::ConnectionRefused | K::AddrNotAvailable | K::PermissionDenied
        )
    };
    match endpoint {
        Endpoint::Tcp(addr) => {
            use std::net::ToSocketAddrs;
            let sock = addr
                .to_socket_addrs()
                .map_err(|e| not_running(endpoint, &e))?
                .next()
                .ok_or_else(|| ClientError::NotRunning(endpoint.display()))?;
            let s = std::net::TcpStream::connect_timeout(&sock, timeout).map_err(|e| {
                if e.kind() == K::TimedOut {
                    ClientError::Timeout(timeout)
                } else {
                    not_running(endpoint, &e)
                }
            })?;
            let _ = s.set_read_timeout(Some(read_timeout));
            let _ = s.set_write_timeout(Some(timeout));
            Ok(Box::new(s))
        }
        #[cfg(unix)]
        Endpoint::Unix(path) => {
            let s = std::os::unix::net::UnixStream::connect(path).map_err(|e| {
                if is_absent(&e) {
                    not_running(endpoint, &e)
                } else {
                    ClientError::Protocol(format!("connecting to {}: {e}", endpoint.display()))
                }
            })?;
            let _ = s.set_read_timeout(Some(read_timeout));
            let _ = s.set_write_timeout(Some(timeout));
            Ok(Box::new(s))
        }
        #[cfg(not(unix))]
        Endpoint::Unix(_) => Err(ClientError::NotRunning(format!(
            "{} (Unix sockets are not available on this system)",
            endpoint.display()
        ))),
        Endpoint::NamedPipe(path) => {
            // ERROR_PIPE_BUSY (231): every pipe instance is in use; Docker frees one quickly.
            let mut tries = 0;
            loop {
                match std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(path)
                {
                    Ok(f) => return Ok(Box::new(f)),
                    Err(e) if e.raw_os_error() == Some(231) && tries < 10 => {
                        tries += 1;
                        std::thread::sleep(Duration::from_millis(50));
                    }
                    Err(e) if is_absent(&e) => return Err(not_running(endpoint, &e)),
                    Err(e) => {
                        return Err(ClientError::Protocol(format!(
                            "connecting to {}: {e}",
                            endpoint.display()
                        )))
                    }
                }
            }
        }
    }
}

/// Parses a full HTTP/1.1 response read until the connection closed.
pub(crate) fn parse_response(buf: &[u8]) -> Result<Response, ClientError> {
    let bad = |why: &str| ClientError::Protocol(format!("Docker sent an invalid answer ({why})"));
    let split = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| bad("no header end"))?;
    let head = std::str::from_utf8(&buf[..split]).map_err(|_| bad("header is not text"))?;
    let mut lines = head.split("\r\n");
    let status_line = lines.next().ok_or_else(|| bad("empty"))?;
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| bad("no status code"))?;
    let mut chunked = false;
    let mut length: Option<usize> = None;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let (name, value) = (name.trim(), value.trim());
        if name.eq_ignore_ascii_case("transfer-encoding") && value.eq_ignore_ascii_case("chunked") {
            chunked = true;
        } else if name.eq_ignore_ascii_case("content-length") {
            length = value.parse().ok();
        }
    }
    let raw_body = &buf[split + 4..];
    let body = if chunked {
        decode_chunked(raw_body).ok_or_else(|| bad("broken chunked body"))?
    } else if let Some(n) = length {
        raw_body
            .get(..n)
            .ok_or_else(|| bad("body shorter than announced"))?
            .to_vec()
    } else {
        raw_body.to_vec()
    };
    Ok(Response { status, body })
}

fn decode_chunked(mut data: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let line_end = data.windows(2).position(|w| w == b"\r\n")?;
        let size_text = std::str::from_utf8(&data[..line_end]).ok()?;
        let size_text = size_text.split(';').next()?.trim();
        let size = usize::from_str_radix(size_text, 16).ok()?;
        data = &data[line_end + 2..];
        if size == 0 {
            return Some(out);
        }
        out.extend_from_slice(data.get(..size)?);
        data = data.get(size + 2..)?;
    }
}

/// Reads a long-lived response (the engine's `/events` stream) and calls `on_event` with
/// `Some(line)` for every JSON line and with `None` roughly every `poll` while idle.
///
/// Returns `Ok(())` when `stop` becomes true (checked at least every `poll`) and an error
/// when the engine hangs up or breaks. A helper thread owns the blocking read so a stream
/// without read timeouts (a Windows named pipe) can never block the caller; on a pipe
/// that helper ends at the next event or when the engine closes.
pub fn stream_lines(
    endpoint: &Endpoint,
    path: &str,
    stop: &std::sync::Arc<std::sync::atomic::AtomicBool>,
    poll: Duration,
    mut on_event: impl FnMut(Option<&str>),
) -> Result<(), ClientError> {
    use std::sync::atomic::Ordering;
    let raw = build_request("GET", path, None);
    let mut stream = connect_with(endpoint, Duration::from_secs(3), poll)?;
    stream
        .write_all(&raw)
        .and_then(|_| stream.flush())
        .map_err(|e| ClientError::Protocol(format!("could not send the request: {e}")))?;
    let (tx, rx) = mpsc::channel::<Result<Vec<u8>, ClientError>>();
    let reader_stop = stop.clone();
    std::thread::Builder::new()
        .name("ols-docker-events-read".into())
        .spawn(move || {
            let mut chunk = [0u8; 8 * 1024];
            while !reader_stop.load(Ordering::Relaxed) {
                match stream.read(&mut chunk) {
                    Ok(0) => {
                        let _ = tx.send(Err(ClientError::Protocol(
                            "Docker closed the stream".into(),
                        )));
                        return;
                    }
                    Ok(n) => {
                        if tx.send(Ok(chunk[..n].to_vec())).is_err() {
                            return;
                        }
                    }
                    Err(e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock
                                | std::io::ErrorKind::TimedOut
                                | std::io::ErrorKind::Interrupted
                        ) => {}
                    Err(e) => {
                        let _ = tx.send(Err(ClientError::Protocol(format!(
                            "reading Docker's events: {e}"
                        ))));
                        return;
                    }
                }
            }
        })
        .map_err(|e| ClientError::Protocol(format!("could not start a reader thread: {e}")))?;
    let mut decoder = StreamDecoder::default();
    loop {
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        match rx.recv_timeout(poll) {
            Ok(Ok(bytes)) => {
                for line in decoder.feed(&bytes)? {
                    on_event(Some(&line));
                }
                if decoder.finished {
                    return Err(ClientError::Protocol("Docker ended the stream".into()));
                }
            }
            Ok(Err(e)) => return Err(e),
            Err(mpsc::RecvTimeoutError::Timeout) => on_event(None),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return if stop.load(Ordering::Relaxed) {
                    Ok(())
                } else {
                    Err(ClientError::Protocol("the events reader stopped".into()))
                }
            }
        }
    }
}

/// Incremental decoder for an HTTP/1.1 response whose body never ends: strips the
/// headers, undoes chunked framing and yields complete text lines.
#[derive(Default)]
pub(crate) struct StreamDecoder {
    buf: Vec<u8>,
    headers_done: bool,
    chunked: bool,
    error_status: Option<u16>,
    text: Vec<u8>,
    pub(crate) finished: bool,
}

impl StreamDecoder {
    pub(crate) fn feed(&mut self, bytes: &[u8]) -> Result<Vec<String>, ClientError> {
        self.buf.extend_from_slice(bytes);
        if self.buf.len() > MAX_RESPONSE {
            return Err(ClientError::Protocol(
                "Docker's stream was too large".into(),
            ));
        }
        if !self.headers_done {
            let Some(split) = self.buf.windows(4).position(|w| w == b"\r\n\r\n") else {
                return Ok(Vec::new());
            };
            let head = String::from_utf8_lossy(&self.buf[..split]).to_string();
            let mut lines = head.split("\r\n");
            let status: u16 = lines
                .next()
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|s| s.parse().ok())
                .ok_or_else(|| ClientError::Protocol("Docker sent an invalid answer".into()))?;
            self.chunked = lines.any(|l| {
                l.split_once(':').is_some_and(|(n, v)| {
                    n.trim().eq_ignore_ascii_case("transfer-encoding")
                        && v.trim().eq_ignore_ascii_case("chunked")
                })
            });
            self.buf.drain(..split + 4);
            self.headers_done = true;
            if !(200..300).contains(&status) {
                self.error_status = Some(status);
            }
        }
        if let Some(status) = self.error_status {
            return Err(ClientError::Http {
                status,
                message: String::from_utf8_lossy(&self.buf).trim().to_string(),
            });
        }
        if self.chunked {
            self.finished = decode_chunk_stream(&mut self.buf, &mut self.text)?;
        } else {
            self.text.append(&mut self.buf);
        }
        let mut out = Vec::new();
        while let Some(end) = self.text.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.text.drain(..=end).collect();
            let line = String::from_utf8_lossy(&line).trim().to_string();
            if !line.is_empty() {
                out.push(line);
            }
        }
        Ok(out)
    }
}

/// Moves every complete chunk from `buf` into `out`; leaves a partial chunk in `buf`.
/// Returns `true` once the terminating zero-size chunk was seen.
pub(crate) fn decode_chunk_stream(
    buf: &mut Vec<u8>,
    out: &mut Vec<u8>,
) -> Result<bool, ClientError> {
    loop {
        let Some(line_end) = buf.windows(2).position(|w| w == b"\r\n") else {
            return Ok(false);
        };
        let size = std::str::from_utf8(&buf[..line_end])
            .ok()
            .and_then(|t| t.split(';').next())
            .and_then(|t| usize::from_str_radix(t.trim(), 16).ok())
            .ok_or_else(|| ClientError::Protocol("Docker sent a broken chunk".into()))?;
        if size == 0 {
            buf.clear();
            return Ok(true);
        }
        let total = line_end + 2 + size + 2;
        if buf.len() < total {
            return Ok(false);
        }
        out.extend_from_slice(&buf[line_end + 2..line_end + 2 + size]);
        buf.drain(..total);
    }
}

/// Percent-encodes everything but unreserved characters (for a query value).
pub fn url_encode(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_docker_host_forms() {
        assert_eq!(
            Endpoint::parse("npipe:////./pipe/docker_engine"),
            Some(Endpoint::NamedPipe(r"\\.\pipe\docker_engine".into()))
        );
        assert_eq!(
            Endpoint::parse(r"\\.\pipe\dockerDesktopLinuxEngine"),
            Some(Endpoint::NamedPipe(
                r"\\.\pipe\dockerDesktopLinuxEngine".into()
            ))
        );
        assert_eq!(
            Endpoint::parse("unix:///var/run/docker.sock"),
            Some(Endpoint::Unix("/var/run/docker.sock".into()))
        );
        assert_eq!(
            Endpoint::parse("tcp://127.0.0.1:2375"),
            Some(Endpoint::Tcp("127.0.0.1:2375".into()))
        );
        assert_eq!(Endpoint::parse("tcp://127.0.0.1"), None);
        assert_eq!(Endpoint::parse("tcp://evil/x:1"), None);
        assert_eq!(Endpoint::parse("http://127.0.0.1:2375"), None);
        assert_eq!(Endpoint::parse(""), None);
    }

    #[test]
    fn request_has_close_and_length() {
        let raw = String::from_utf8(build_request("POST", "/containers/x/start", None)).unwrap();
        assert!(raw.starts_with("POST /containers/x/start HTTP/1.1\r\n"));
        assert!(raw.contains("Connection: close\r\n"));
        assert!(raw.ends_with("Content-Length: 0\r\n\r\n"));
    }

    #[test]
    fn parses_content_length_and_chunked_bodies() {
        let r = parse_response(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n[]").unwrap();
        assert_eq!((r.status, r.body.as_slice()), (200, b"[]".as_slice()));

        let r = parse_response(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\n[{}\r\n1\r\n]\r\n0\r\n\r\n",
        )
        .unwrap();
        assert_eq!(r.body, b"[{}]");

        assert!(parse_response(b"garbage").is_err());
    }

    #[test]
    fn stream_decoder_handles_split_chunks() {
        let mut d = StreamDecoder::default();
        let all = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n8\r\n{\"a\":1}\n\r\n4\r\n{\"b\"\r\n";
        let mut lines = Vec::new();
        // One byte at a time: every split point must work.
        for b in all.iter() {
            lines.extend(d.feed(&[*b]).unwrap());
        }
        assert_eq!(lines, vec!["{\"a\":1}".to_string()]);
        lines.extend(d.feed(b"4\r\n:2}\n\r\n0\r\n\r\n").unwrap());
        assert_eq!(lines.last().unwrap(), "{\"b\":2}");
        assert!(d.finished);
    }

    #[test]
    fn stream_decoder_reports_http_errors() {
        let mut d = StreamDecoder::default();
        let e = d.feed(b"HTTP/1.1 500 X\r\n\r\nboom").unwrap_err();
        assert!(matches!(e, ClientError::Http { status: 500, .. }));
    }

    #[test]
    fn stream_stops_on_flag_within_a_poll() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        std::thread::spawn(move || {
            // Accept and say nothing, like an engine with no events.
            let _keep = listener.accept();
            std::thread::sleep(Duration::from_secs(5));
        });
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            flag.store(true, Ordering::Relaxed);
        });
        let started = std::time::Instant::now();
        let result = stream_lines(
            &Endpoint::Tcp(addr),
            "/events",
            &stop,
            Duration::from_millis(250),
            |_| {},
        );
        assert!(result.is_ok());
        assert!(started.elapsed() < Duration::from_millis(1200));
    }

    #[test]
    fn url_encode_escapes_json() {
        assert_eq!(
            url_encode(r#"{"a":["b"]}"#),
            "%7B%22a%22%3A%5B%22b%22%5D%7D"
        );
    }
}
