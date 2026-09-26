//! The local control channel (architecture decision 2, §136): how the `ols` command line
//! talks to the running app.
//!
//! One process owns the core at a time: the desktop app, or `ols daemon` when the app
//! isn't open. The owner listens on a per-user named pipe and writes `control.json`
//! (pipe name, a random per-session token, its process id) into the data folder. Each
//! request is one JSON line `{"token": ..., "command": <CoreCommand>}` and gets one JSON
//! line back: `{"ok": <CoreResponse>}` or `{"err": <Diagnostic>}`. A request without the
//! session's token is refused, so only something that can read the user's data folder
//! can drive the app. The pipe only takes local connections.
//!
//! When the desktop app starts while a daemon is running, it asks the daemon to hand over
//! (`{"token", "op": "shutdown"}`): the daemon stops what it started and exits, and the
//! app takes the pipe.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::command::{Core, CoreCommand, CoreResponse};
use crate::error::Diagnostic;
use crate::paths::AppPaths;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlInfo {
    pub pipe: String,
    pub token: String,
    pub pid: u32,
    /// "app" or "daemon".
    pub kind: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Request {
    token: String,
    #[serde(default)]
    command: Option<CoreCommand>,
    /// "shutdown" asks a daemon to stop and hand over.
    #[serde(default)]
    op: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reply {
    Ok(Box<CoreResponse>),
    Err(Diagnostic),
}

pub fn info_file(paths: &AppPaths) -> PathBuf {
    paths.data_dir().join("control.json")
}

pub fn read_info(paths: &AppPaths) -> Option<ControlInfo> {
    std::fs::read_to_string(info_file(paths)).ok().and_then(|t| serde_json::from_str(&t).ok())
}

/// The pipe for this user and data folder: one owner per install, and a second install
/// (or a test home) never collides with the first.
pub fn pipe_name(paths: &AppPaths) -> String {
    use sha2::Digest;
    let user: String = std::env::var("USERNAME").unwrap_or_else(|_| "user".into()).chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
    let root = paths.root().display().to_string().to_lowercase();
    let hash = format!("{:x}", sha2::Sha256::digest(root.as_bytes()));
    format!(r"\\.\pipe\OpenLocalServer-control-{user}-{}", &hash[..12])
}

fn new_token() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("the OS random generator failed");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Constant-time comparison, so the token can't be guessed byte by byte from timings.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

// ------------------------------------------------------------------------ client

#[derive(Debug)]
pub enum ClientError {
    /// Nothing is listening.
    NotRunning,
    Io(String),
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::NotRunning => write!(f, "OpenLocalServer is not running"),
            ClientError::Io(e) => write!(f, "{e}"),
        }
    }
}

fn exchange(info: &ControlInfo, request: &Request) -> Result<Vec<u8>, ClientError> {
    use std::io::{Read, Write};
    let mut pipe = None;
    // The server may be between connections for a moment (ERROR_PIPE_BUSY).
    for _ in 0..50 {
        match std::fs::OpenOptions::new().read(true).write(true).open(&info.pipe) {
            Ok(p) => {
                pipe = Some(p);
                break;
            }
            Err(e) if e.raw_os_error() == Some(231) => std::thread::sleep(std::time::Duration::from_millis(100)),
            Err(_) => return Err(ClientError::NotRunning),
        }
    }
    let mut pipe = pipe.ok_or(ClientError::NotRunning)?;
    let mut line = serde_json::to_vec(request).map_err(|e| ClientError::Io(e.to_string()))?;
    line.push(b'\n');
    pipe.write_all(&line).map_err(|e| ClientError::Io(e.to_string()))?;
    let mut reply = Vec::new();
    let mut buf = [0u8; 64 * 1024];
    while !reply.contains(&b'\n') {
        match pipe.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => reply.extend_from_slice(&buf[..n]),
        }
    }
    Ok(reply)
}

/// Sends one command to whoever owns the core.
pub fn send(paths: &AppPaths, command: CoreCommand) -> Result<Result<CoreResponse, Diagnostic>, ClientError> {
    let info = read_info(paths).ok_or(ClientError::NotRunning)?;
    let reply = exchange(&info, &Request { token: info.token.clone(), command: Some(command), op: None })?;
    let line = reply.split(|b| *b == b'\n').next().unwrap_or_default();
    match serde_json::from_slice::<Reply>(line) {
        Ok(Reply::Ok(r)) => Ok(Ok(*r)),
        Ok(Reply::Err(d)) => Ok(Err(d)),
        Err(_) if line.is_empty() => Err(ClientError::NotRunning),
        Err(e) => Err(ClientError::Io(format!("unreadable reply: {e}"))),
    }
}

/// Whether an owner is answering right now.
pub fn is_running(paths: &AppPaths) -> Option<ControlInfo> {
    let info = read_info(paths)?;
    matches!(send(paths, CoreCommand::Ping), Ok(Ok(_))).then_some(info)
}

/// Asks a running daemon to stop and hand over; waits until it has. Does nothing to a
/// running app (only daemons hand over).
pub fn take_over_from_daemon(paths: &AppPaths) {
    let Some(info) = is_running(paths) else { return };
    if info.kind != "daemon" {
        return;
    }
    tracing::info!(pid = info.pid, "asking the ols daemon to hand over");
    let _ = exchange(&info, &Request { token: info.token.clone(), command: None, op: Some("shutdown".into()) });
    for _ in 0..100 {
        if is_running(paths).is_none() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

// ------------------------------------------------------------------------ server

/// Keeps the control channel open; dropping it doesn't stop it (it lives with the process).
pub struct ControlServer {
    pub info: ControlInfo,
    shutdown: tokio::sync::watch::Receiver<bool>,
}

impl ControlServer {
    /// Blocks until a client asks this process to shut down (daemons only).
    pub fn wait_for_shutdown(&mut self) {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
        rt.block_on(async {
            while !*self.shutdown.borrow() {
                if self.shutdown.changed().await.is_err() {
                    break;
                }
            }
        });
    }
}

/// Opens the pipe and serves `core` on it. `kind` is "app" or "daemon"; only a daemon
/// accepts the shutdown request.
#[cfg(windows)]
pub fn serve(core: Core, paths: &AppPaths, kind: &str) -> Result<ControlServer, String> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::windows::named_pipe::ServerOptions;

    let info = ControlInfo { pipe: pipe_name(paths), token: new_token(), pid: std::process::id(), kind: kind.into() };
    let (tx, rx) = tokio::sync::watch::channel(false);
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), String>>();
    let server_info = info.clone();
    let accept_shutdown = kind == "daemon";
    std::thread::Builder::new()
        .name("ols-control".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().expect("control runtime");
            rt.block_on(async move {
                // `first_pipe_instance` fails if another process already owns the name.
                let mut server = match ServerOptions::new().first_pipe_instance(true).reject_remote_clients(true).create(&server_info.pipe) {
                    Ok(s) => {
                        let _ = ready_tx.send(Ok(()));
                        s
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(format!("another OpenLocalServer already owns the control pipe ({e})")));
                        return;
                    }
                };
                loop {
                    if server.connect().await.is_err() {
                        continue;
                    }
                    let connected = server;
                    server = match ServerOptions::new().reject_remote_clients(true).create(&server_info.pipe) {
                        Ok(s) => s,
                        Err(e) => {
                            tracing::error!(error = %e, "control pipe could not accept more clients");
                            return;
                        }
                    };
                    let core = core.clone();
                    let token = server_info.token.clone();
                    let tx = tx.clone();
                    tokio::spawn(async move {
                        let (read, mut write) = tokio::io::split(connected);
                        let mut line = String::new();
                        if BufReader::new(read).read_line(&mut line).await.is_err() {
                            return;
                        }
                        let reply = match serde_json::from_str::<Request>(&line) {
                            Err(e) => Reply::Err(Diagnostic { problem: "That request could not be read.".into(), cause: e.to_string(), fix: None }),
                            Ok(r) if !same(&r.token, &token) => Reply::Err(Diagnostic { problem: "Not allowed.".into(), cause: "The request did not carry this session's token.".into(), fix: None }),
                            Ok(Request { op: Some(op), .. }) if op == "shutdown" && accept_shutdown => {
                                let _ = tx.send(true);
                                Reply::Ok(Box::new(CoreResponse::Ok))
                            }
                            Ok(Request { command: Some(cmd), .. }) => {
                                let c = core.clone();
                                match tokio::task::spawn_blocking(move || c.dispatch(cmd)).await {
                                    Ok(Ok(r)) => Reply::Ok(Box::new(r)),
                                    Ok(Err(d)) => Reply::Err(d),
                                    Err(e) => Reply::Err(Diagnostic { problem: "The command crashed.".into(), cause: e.to_string(), fix: None }),
                                }
                            }
                            Ok(_) => Reply::Err(Diagnostic { problem: "Nothing to do.".into(), cause: "The request had no command.".into(), fix: None }),
                        };
                        let mut out = serde_json::to_vec(&reply).unwrap_or_default();
                        out.push(b'\n');
                        let _ = write.write_all(&out).await;
                        let _ = write.flush().await;
                    });
                }
            });
        })
        .map_err(|e| e.to_string())?;
    ready_rx.recv().map_err(|e| e.to_string())??;
    let file = info_file(paths);
    std::fs::write(&file, serde_json::to_string_pretty(&info).map_err(|e| e.to_string())?).map_err(|e| format!("{}: {e}", file.display()))?;
    tracing::info!(pipe = %info.pipe, kind, "control channel open");
    Ok(ControlServer { info, shutdown: rx })
}

#[cfg(not(windows))]
pub fn serve(_core: Core, _paths: &AppPaths, _kind: &str) -> Result<ControlServer, String> {
    Err("the control channel is Windows-only until the macOS / Linux release".into())
}

/// Removes `control.json` when it still describes this process.
pub fn close(paths: &AppPaths) {
    if read_info(paths).is_some_and(|i| i.pid == std::process::id()) {
        let _ = std::fs::remove_file(info_file(paths));
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_checked_and_commands_round_trip() {
        let home = crate::test_support::isolated_home();
        let settings = crate::settings::SettingsService::load(&home.paths).unwrap();
        let core = Core::new(settings, home.paths.clone());
        // The pipe name follows the (temporary) data folder, so a running app isn't disturbed.
        let server = serve(core, &home.paths, "daemon").unwrap();
        let info = server.info.clone();
        assert_ne!(info.pipe, pipe_name(&AppPaths::resolve_at(std::path::Path::new("C:/elsewhere"))));

        match send(&home.paths, CoreCommand::Ping).unwrap() {
            Ok(CoreResponse::Pong { .. }) => {}
            other => panic!("{other:?}"),
        }
        let bad = ControlInfo { token: "wrong".into(), ..info.clone() };
        let reply = exchange(&bad, &Request { token: bad.token.clone(), command: Some(CoreCommand::Ping), op: None }).unwrap();
        assert!(String::from_utf8_lossy(&reply).contains("Not allowed"));
        assert!(same("abc", "abc") && !same("abc", "abd") && !same("abc", "ab"));
    }
}
