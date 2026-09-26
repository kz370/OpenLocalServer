//! Small synchronous "run it and capture everything" helper. The supervisor's
//! `run_to_completion` deliberately discards output (it feeds command history); config
//! validators (`nginx -t`, `httpd -t`, `caddy validate`) and DB clients need the text.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct Captured {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

impl Captured {
    pub fn success(&self) -> bool {
        self.exit_code == Some(0) && !self.timed_out
    }

    /// stdout and stderr joined — validators disagree about which stream carries errors.
    pub fn combined(&self) -> String {
        match (self.stdout.trim().is_empty(), self.stderr.trim().is_empty()) {
            (true, _) => self.stderr.trim().to_string(),
            (_, true) => self.stdout.trim().to_string(),
            _ => format!("{}\n{}", self.stdout.trim(), self.stderr.trim()),
        }
    }
}

/// Stops a console window from flashing up when the (GUI) app spawns a child.
pub fn hide_window(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd
}

pub fn run_capture(
    executable: &Path,
    args: &[String],
    cwd: Option<&Path>,
    env: &[(String, String)],
    timeout: Duration,
) -> Captured {
    let mut cmd = Command::new(executable);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    for (k, v) in env {
        cmd.env(k, v);
    }
    hide_window(&mut cmd);

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return Captured {
                exit_code: None,
                stdout: String::new(),
                stderr: e.to_string(),
                timed_out: false,
            };
        }
    };

    // Drain both pipes on their own threads so a chatty child can't fill a pipe buffer
    // and deadlock against our wait.
    let mut out_pipe = child.stdout.take();
    let mut err_pipe = child.stderr.take();
    let out_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(p) = out_pipe.as_mut() {
            let _ = p.read_to_end(&mut buf);
        }
        buf
    });
    let err_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(p) = err_pipe.as_mut() {
            let _ = p.read_to_end(&mut buf);
        }
        buf
    });

    let started = Instant::now();
    let (exit_code, timed_out) = loop {
        match child.try_wait() {
            Ok(Some(status)) => break (status.code(), false),
            Ok(None) => {
                if started.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    break (None, true);
                }
                std::thread::sleep(Duration::from_millis(15));
            }
            Err(_) => break (None, false),
        }
    };

    Captured {
        exit_code,
        stdout: String::from_utf8_lossy(&out_thread.join().unwrap_or_default()).to_string(),
        stderr: String::from_utf8_lossy(&err_thread.join().unwrap_or_default()).to_string(),
        timed_out,
    }
}

/// Runs a command to completion, handing every output line (stdout and stderr interleaved
/// in arrival order) to `on_line` as it appears. Used by Quick App runs so the user watches
/// `composer create-project` progress live instead of waiting for it to finish. `cancel`
/// is polled so a run can be aborted mid-step.
pub fn run_streaming(
    executable: &Path,
    args: &[String],
    cwd: Option<&Path>,
    env: &[(String, String)],
    timeout: Duration,
    cancel: &std::sync::atomic::AtomicBool,
    mut on_line: impl FnMut(&str),
) -> Captured {
    use std::io::BufRead;
    use std::sync::mpsc;

    let mut cmd = Command::new(executable);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    for (k, v) in env {
        cmd.env(k, v);
    }
    hide_window(&mut cmd);

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return Captured {
                exit_code: None,
                stdout: String::new(),
                stderr: e.to_string(),
                timed_out: false,
            };
        }
    };

    let (tx, rx) = mpsc::channel::<String>();
    let mut readers = Vec::new();
    if let Some(out) = child.stdout.take() {
        let tx = tx.clone();
        readers.push(std::thread::spawn(move || {
            for line in std::io::BufReader::new(out).split(b'\n').flatten() {
                let _ = tx.send(
                    String::from_utf8_lossy(&line)
                        .trim_end_matches('\r')
                        .to_string(),
                );
            }
        }));
    }
    if let Some(err) = child.stderr.take() {
        let tx = tx.clone();
        readers.push(std::thread::spawn(move || {
            for line in std::io::BufReader::new(err).split(b'\n').flatten() {
                let _ = tx.send(
                    String::from_utf8_lossy(&line)
                        .trim_end_matches('\r')
                        .to_string(),
                );
            }
        }));
    }
    drop(tx);

    let started = Instant::now();
    let mut collected = String::new();
    let mut timed_out = false;
    let mut cancelled = false;
    let exit_code = loop {
        // Drain whatever output has arrived.
        while let Ok(line) = rx.try_recv() {
            on_line(&line);
            collected.push_str(&line);
            collected.push('\n');
        }
        match child.try_wait() {
            Ok(Some(status)) => break status.code(),
            Ok(None) => {
                if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    cancelled = true;
                } else if started.elapsed() > timeout {
                    timed_out = true;
                }
                if cancelled || timed_out {
                    kill_tree_blocking(child.id());
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                std::thread::sleep(Duration::from_millis(30));
            }
            Err(_) => break None,
        }
    };
    for r in readers {
        let _ = r.join();
    }
    while let Ok(line) = rx.try_recv() {
        on_line(&line);
        collected.push_str(&line);
        collected.push('\n');
    }
    Captured {
        exit_code,
        stdout: collected,
        stderr: if cancelled {
            "cancelled".into()
        } else {
            String::new()
        },
        timed_out,
    }
}

/// Kills a process and everything it started (`npm` spawns `node`, `composer` spawns `php`).
pub fn kill_tree_blocking(pid: u32) {
    #[cfg(windows)]
    {
        let mut cmd = Command::new("taskkill");
        cmd.args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        hide_window(&mut cmd);
        let _ = cmd.status();
    }
    #[cfg(not(windows))]
    {
        let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn captures_stdout_and_exit_code() {
        let r = run_capture(
            Path::new("cmd"),
            &["/C".into(), "echo hello & exit 3".into()],
            None,
            &[],
            Duration::from_secs(10),
        );
        assert_eq!(r.exit_code, Some(3));
        assert!(r.stdout.contains("hello"));
        assert!(!r.success());
    }

    #[cfg(windows)]
    #[test]
    fn streaming_delivers_lines_and_honours_cancel() {
        use std::sync::atomic::AtomicBool;
        let cancel = AtomicBool::new(false);
        let mut lines = Vec::new();
        let r = run_streaming(
            Path::new("cmd"),
            &["/C".into(), "echo one & echo two".into()],
            None,
            &[],
            Duration::from_secs(10),
            &cancel,
            |l| lines.push(l.to_string()),
        );
        assert!(r.success());
        assert_eq!(lines, ["one ", "two"]);

        // A long-running command is stopped as soon as cancel flips.
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
        });
        let started = Instant::now();
        let r = run_streaming(
            Path::new("cmd"),
            &["/C".into(), "ping -n 30 127.0.0.1 > nul".into()],
            None,
            &[],
            Duration::from_secs(60),
            &cancel,
            |_| {},
        );
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "cancel must not wait for the command"
        );
        assert!(!r.success());
    }

    #[test]
    fn missing_executable_reports_error_not_panic() {
        let r = run_capture(
            Path::new("definitely-not-a-real-binary-xyz"),
            &[],
            None,
            &[],
            Duration::from_secs(2),
        );
        assert!(r.exit_code.is_none());
        assert!(!r.stderr.is_empty());
    }
}
