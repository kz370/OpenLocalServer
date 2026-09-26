//! Interactive terminal (§19): a real shell in a pseudo-terminal, opened in a project's folder
//! with that project's runtimes first on PATH, so `php`, `node`, `composer`, `python` and the
//! database clients are the ones the project resolves to. The UI draws it with xterm.js: output
//! goes out as `TerminalEvent`s, keystrokes and resizes come in as commands.

use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::app::Inner;
use crate::error::CoreError;

fn err(msg: impl Into<String>) -> CoreError {
    CoreError::ServiceError(msg.into())
}

/// Most sessions open at once; each one is a shell process.
const MAX_SESSIONS: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TerminalEvent {
    Output { id: u32, data: String },
    Exit { id: u32 },
}

/// What to start. `shell` is `powershell` (default) or `cmd`.
#[derive(Debug, Clone)]
pub struct TerminalSpec {
    pub shell: Option<String>,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(String, String)>,
    pub rows: u16,
    pub cols: u16,
}

struct Session {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
}

pub struct TerminalManager {
    next_id: AtomicU32,
    sessions: Arc<Mutex<HashMap<u32, Session>>>,
    events: broadcast::Sender<TerminalEvent>,
}

impl Default for TerminalManager {
    fn default() -> Self {
        Self::new()
    }
}

/// The next event, skipping over any that were missed while the receiver fell behind a burst
/// of output. `None` once the manager is gone.
pub async fn next_event(rx: &mut broadcast::Receiver<TerminalEvent>) -> Option<TerminalEvent> {
    loop {
        match rx.recv().await {
            Ok(event) => return Some(event),
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(broadcast::error::RecvError::Closed) => return None,
        }
    }
}

/// The shell's program and arguments for a shell name.
pub fn shell_command(shell: Option<&str>) -> Result<(&'static str, Vec<&'static str>), String> {
    match shell.unwrap_or("powershell") {
        "powershell" => Ok(("powershell.exe", vec!["-NoLogo"])),
        "cmd" => Ok(("cmd.exe", vec![])),
        other => Err(format!("unknown shell: {other}")),
    }
}

/// Turns raw bytes into text without cutting a multi-byte character in half: bytes that end
/// mid-character stay in `pending` for the next chunk, and invalid bytes become U+FFFD.
pub fn decode_chunk(pending: &mut Vec<u8>, incoming: &[u8]) -> String {
    pending.extend_from_slice(incoming);
    let mut out = String::new();
    loop {
        match std::str::from_utf8(pending) {
            Ok(text) => {
                out.push_str(text);
                pending.clear();
                return out;
            }
            Err(e) => {
                let valid = e.valid_up_to();
                out.push_str(std::str::from_utf8(&pending[..valid]).unwrap_or_default());
                match e.error_len() {
                    // Incomplete character at the end: wait for the rest.
                    None => {
                        pending.drain(..valid);
                        return out;
                    }
                    Some(bad) => {
                        out.push('\u{FFFD}');
                        pending.drain(..valid + bad);
                    }
                }
            }
        }
    }
}

impl TerminalManager {
    pub fn new() -> Self {
        let (events, _) = broadcast::channel(1024);
        Self { next_id: AtomicU32::new(1), sessions: Arc::new(Mutex::new(HashMap::new())), events }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<TerminalEvent> {
        self.events.subscribe()
    }

    pub fn open(&self, spec: TerminalSpec) -> Result<u32, String> {
        if self.sessions.lock().unwrap().len() >= MAX_SESSIONS {
            return Err(format!("{MAX_SESSIONS} terminals are already open. Close one first."));
        }
        let (program, args) = shell_command(spec.shell.as_deref())?;
        let pair = native_pty_system()
            .openpty(PtySize { rows: spec.rows.max(2), cols: spec.cols.max(10), pixel_width: 0, pixel_height: 0 })
            .map_err(|e| format!("could not open a terminal: {e}"))?;

        let mut cmd = CommandBuilder::new(program);
        cmd.args(args);
        if let Some(cwd) = spec.cwd.as_ref().filter(|c| c.is_dir()) {
            cmd.cwd(cwd);
        }
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }
        cmd.env("TERM", "xterm-256color");

        let child = pair.slave.spawn_command(cmd).map_err(|e| format!("could not start {program}: {e}"))?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;

        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        self.sessions.lock().unwrap().insert(id, Session { master: pair.master, writer, child });

        let events = self.events.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            let mut pending = Vec::new();
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let data = decode_chunk(&mut pending, &buf[..n]);
                        if !data.is_empty() {
                            let _ = events.send(TerminalEvent::Output { id, data });
                        }
                    }
                }
            }
        });

        // A shell that exits doesn't always end the read on Windows, so exit is watched
        // separately. Whichever side notices first removes the session; the event is sent once.
        let sessions = self.sessions.clone();
        let events = self.events.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_millis(300));
            let mut map = sessions.lock().unwrap();
            let done = match map.get_mut(&id) {
                None => return,
                Some(s) => !matches!(s.child.try_wait(), Ok(None)),
            };
            if done {
                map.remove(&id);
                drop(map);
                let _ = events.send(TerminalEvent::Exit { id });
                return;
            }
        });
        Ok(id)
    }

    pub fn write(&self, id: u32, data: &str) -> Result<(), String> {
        let mut map = self.sessions.lock().unwrap();
        let s = map.get_mut(&id).ok_or("that terminal is closed")?;
        s.writer.write_all(data.as_bytes()).and_then(|_| s.writer.flush()).map_err(|e| e.to_string())
    }

    pub fn resize(&self, id: u32, rows: u16, cols: u16) -> Result<(), String> {
        let map = self.sessions.lock().unwrap();
        let s = map.get(&id).ok_or("that terminal is closed")?;
        s.master.resize(PtySize { rows: rows.max(2), cols: cols.max(10), pixel_width: 0, pixel_height: 0 }).map_err(|e| e.to_string())
    }

    /// Ends the shell and everything it started in its console.
    pub fn close(&self, id: u32) {
        let removed = self.sessions.lock().unwrap().remove(&id);
        if let Some(mut s) = removed {
            let _ = s.child.kill();
            let _ = self.events.send(TerminalEvent::Exit { id });
        }
    }

    pub fn close_all(&self) {
        let ids: Vec<u32> = self.sessions.lock().unwrap().keys().copied().collect();
        for id in ids {
            self.close(id);
        }
    }

    pub fn open_count(&self) -> usize {
        self.sessions.lock().unwrap().len()
    }
}

/// `composer.cmd`: runs Composer's phar with whichever `php` is first on the terminal's PATH.
pub fn composer_shim(phar: &Path) -> String {
    format!("@echo off\r\nphp \"{}\" %*\r\n", phar.display())
}

impl Inner {
    /// Where the terminal starts and what it sees: the project's folder, its runtimes first on
    /// PATH, its virtual environment active, and `composer` available as a command.
    pub fn terminal_setup(&self, project_id: Option<&str>) -> Result<(Option<PathBuf>, Vec<(String, String)>), CoreError> {
        let project = match project_id {
            Some(id) => Some(self.projects.lock().unwrap().get(id).ok_or_else(|| CoreError::InvalidProjectPath(id.to_string()))?),
            None => None,
        };

        // Pin the project's resolved versions so `php` and `node` follow it (§18–19).
        let mut values = BTreeMap::new();
        if let Some(detail) = project_id.and_then(|id| self.project_detail(id)) {
            for r in &detail.resolved {
                if let Some(v) = &r.installed_version {
                    values.insert(format!("{}_version", r.id), v.clone());
                }
            }
        }

        let mut dirs: Vec<PathBuf> = Vec::new();
        let mut env: Vec<(String, String)> = Vec::new();
        let add_dir = |dirs: &mut Vec<PathBuf>, d: PathBuf| {
            if !dirs.contains(&d) {
                dirs.push(d);
            }
        };
        let add_env = |env: &mut Vec<(String, String)>, k: String, v: String| {
            if !env.iter().any(|(existing, _)| existing.eq_ignore_ascii_case(&k)) {
                env.push((k, v));
            }
        };

        for program in ["php", "node", "python"] {
            if let Ok(r) = self.quick_resolve_program(program, &values, project_id) {
                for d in r.path_dirs {
                    add_dir(&mut dirs, d);
                }
                for (k, v) in r.env {
                    add_env(&mut env, k, v);
                }
            }
        }

        if let Ok(r) = self.quick_resolve_program("composer", &values, project_id) {
            if let Some(phar) = r.pre_args.first() {
                let shims = self.paths.root().join("shims");
                if std::fs::create_dir_all(&shims).is_ok() && std::fs::write(shims.join("composer.cmd"), composer_shim(Path::new(phar))).is_ok() {
                    add_dir(&mut dirs, shims);
                }
            }
            for (k, v) in r.env {
                // The Quick App runner turns prompts off; a person at a terminal wants them.
                if k != "COMPOSER_NO_INTERACTION" {
                    add_env(&mut env, k, v);
                }
            }
        }

        // Command-line clients of the databases we manage.
        for id in ["mariadb", "postgres", "redis", "mongodb", "sqlite"] {
            let versions = self.runtimes.installed_versions(id);
            if let Some(dir) = crate::php::pick_version(&versions, None).and_then(|v| self.runtimes.bin_dir(id, &v)) {
                add_dir(&mut dirs, dir);
            }
        }

        let system_path = std::env::var("PATH").unwrap_or_default();
        let mut path: Vec<String> = dirs.iter().map(|d| d.display().to_string()).collect();
        path.push(system_path);
        env.push(("PATH".to_string(), path.join(";")));

        Ok((project.map(|p| PathBuf::from(p.path)).or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from)), env))
    }

    pub fn open_terminal(&self, project_id: Option<&str>, shell: Option<String>, rows: u16, cols: u16) -> Result<u32, CoreError> {
        let (cwd, env) = self.terminal_setup(project_id)?;
        self.terminals.open(TerminalSpec { shell, cwd, env, rows, cols }).map_err(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_passes_straight_through() {
        let mut pending = Vec::new();
        assert_eq!(decode_chunk(&mut pending, b"hello\r\n"), "hello\r\n");
        assert!(pending.is_empty());
    }

    #[test]
    fn a_character_split_across_reads_is_not_broken() {
        // "é" is 0xC3 0xA9; "€" is 0xE2 0x82 0xAC.
        let mut pending = Vec::new();
        assert_eq!(decode_chunk(&mut pending, &[b'a', 0xC3]), "a");
        assert_eq!(pending, vec![0xC3]);
        assert_eq!(decode_chunk(&mut pending, &[0xA9, b'b']), "éb");
        assert!(pending.is_empty());

        assert_eq!(decode_chunk(&mut pending, &[0xE2, 0x82]), "");
        assert_eq!(decode_chunk(&mut pending, &[0xAC]), "€");
    }

    #[test]
    fn invalid_bytes_become_replacement_characters_without_losing_the_rest() {
        let mut pending = Vec::new();
        assert_eq!(decode_chunk(&mut pending, &[b'a', 0xFF, b'b']), "a\u{FFFD}b");
        assert!(pending.is_empty());
    }

    #[test]
    fn only_known_shells_start() {
        assert_eq!(shell_command(None).unwrap().0, "powershell.exe");
        assert_eq!(shell_command(Some("cmd")).unwrap().0, "cmd.exe");
        assert!(shell_command(Some("bash; rm -rf /")).is_err());
    }

    #[test]
    fn the_composer_shim_calls_the_phar_through_php() {
        let shim = composer_shim(Path::new(r"C:\ols\composer.phar"));
        assert!(shim.contains("php \"C:\\ols\\composer.phar\" %*"));
        assert!(shim.starts_with("@echo off"));
    }

    #[test]
    fn writing_to_a_closed_terminal_is_an_error_not_a_panic() {
        let mgr = TerminalManager::new();
        assert!(mgr.write(99, "x").is_err());
        assert!(mgr.resize(99, 24, 80).is_err());
        mgr.close(99);
        assert_eq!(mgr.open_count(), 0);
    }
}
