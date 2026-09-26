//! Port Manager (§109 — Stage 2). Reports what owns a port instead of guessing; never
//! kills anything (§75 — conflict detection must propose, not act).

use std::net::{SocketAddr, TcpListener};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PortStatus {
    Free,
    InUse {
        pid: Option<u32>,
        process_name: Option<String>,
    },
}

/// Cheap free/in-use test: a bind attempt, no process lookup.
pub fn port_is_free(port: u16) -> bool {
    TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port))).is_ok()
}

/// Check whether `port` is free on 127.0.0.1. If it's busy, best-effort identify the
/// owning process (Windows: parse `netstat -ano` + `tasklist`) so the UI can show
/// "Port 3306 is in use by mysqld.exe (PID 4821)" instead of a bare failure.
pub fn check_port(port: u16) -> PortStatus {
    let addr: SocketAddr = ([127, 0, 0, 1], port).into();
    match TcpListener::bind(addr) {
        Ok(listener) => {
            drop(listener);
            PortStatus::Free
        }
        Err(_) => {
            // netstat + tasklist cost ~0.5s and the UI polls this; owners rarely change, so cache.
            static CACHE: std::sync::OnceLock<
                std::sync::Mutex<
                    std::collections::HashMap<
                        u16,
                        (std::time::Instant, Option<u32>, Option<String>),
                    >,
                >,
            > = std::sync::OnceLock::new();
            let cache = CACHE.get_or_init(Default::default);
            if let Some((at, pid, name)) = cache.lock().unwrap().get(&port) {
                if at.elapsed() < std::time::Duration::from_secs(5) {
                    return PortStatus::InUse {
                        pid: *pid,
                        process_name: name.clone(),
                    };
                }
            }
            let pid = find_owning_pid(port);
            let process_name = pid
                .and_then(find_process_name)
                .or_else(|| excluded_range_reason(port));
            cache
                .lock()
                .unwrap()
                .insert(port, (std::time::Instant::now(), pid, process_name.clone()));
            PortStatus::InUse { pid, process_name }
        }
    }
}

#[cfg(windows)]
fn find_owning_pid(port: u16) -> Option<u32> {
    let mut cmd = std::process::Command::new("netstat");
    cmd.args(["-ano"]);
    crate::exec::hide_window(&mut cmd);
    let output = cmd.output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let needle = format!(":{port}");

    let mut fallback: Option<u32> = None;
    for line in text.lines() {
        let cols: Vec<&str> = line.split_whitespace().collect();
        // Expected shape: Proto  Local Address  Foreign Address  State  PID.
        // State text is localized ("LISTENING" vs "ABHÖREN"), so prefer a
        // LISTENING row but accept any 5-column TCP row: TIME_WAIT rows have
        // no PID column and are skipped by the length check below.
        if cols.len() < 5 {
            continue;
        }
        if cols[0] != "TCP" {
            continue;
        }
        if !cols[1].ends_with(&needle) {
            continue;
        }
        let state_listening = cols[3] == "LISTENING";
        if let Ok(pid) = cols[cols.len() - 1].parse::<u32>() {
            if state_listening {
                return Some(pid);
            }
            // Localized state or non-LISTENING holder: remember, keep looking
            // for an explicit LISTENING row first.
            fallback = Some(pid);
        }
    }
    fallback
}

/// Port binds fail with nobody listening when Windows reserves the range
/// (Hyper-V / Docker / WSL exclude blocks via `netsh interface ipv4 show
/// excludedportrange`). Name that so the UI proposes the real fix instead of
/// a ghost holder. Best-effort: any parse failure means "not excluded".
#[cfg(windows)]
fn excluded_range_reason(port: u16) -> Option<String> {
    let mut cmd = std::process::Command::new("netsh");
    cmd.args([
        "interface",
        "ipv4",
        "show",
        "excludedportrange",
        "protocol=tcp",
    ]);
    crate::exec::hide_window(&mut cmd);
    let output = cmd.output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    // Header lines are localized; only rows with two integers matter.
    for line in text.lines() {
        let nums: Vec<u16> = line
            .split(|c: char| !(c.is_ascii_digit()))
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse().ok())
            .collect();
        if nums.len() == 2 && port >= nums[0] && port <= nums[1] {
            return Some("Windows excluded port range (Hyper-V/Docker)".to_string());
        }
    }
    None
}

#[cfg(not(windows))]
fn excluded_range_reason(_port: u16) -> Option<String> {
    None
}

#[cfg(windows)]
fn find_process_name(pid: u32) -> Option<String> {
    let mut cmd = std::process::Command::new("tasklist");
    cmd.args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"]);
    crate::exec::hide_window(&mut cmd);
    let output = cmd.output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let trimmed = text.trim();
    // Race: netstat saw a PID that exited before tasklist ran. tasklist then
    // prints "INFO: No tasks are running..." — never surface that as a name.
    if trimmed.is_empty() || trimmed.starts_with("INFO:") {
        return None;
    }
    // CSV line: "name.exe","1234","Console","1","12,345 K"
    let first_field = trimmed.split(',').next()?;
    let name = first_field.trim_matches('"').trim();
    if name.is_empty() || name.starts_with("INFO:") {
        None
    } else {
        Some(name.to_string())
    }
}

#[cfg(not(windows))]
fn find_owning_pid(_port: u16) -> Option<u32> {
    None
}

#[cfg(not(windows))]
fn find_process_name(_pid: u32) -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_free_on_an_unused_high_port() {
        // 127.0.0.1:0 asks the OS for any free port so this test can't collide with a
        // real service; we then release it and check *that exact* port back as Free.
        let probe = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);

        match check_port(port) {
            PortStatus::Free => {}
            other => panic!("expected Free, got {other:?}"),
        }
    }

    #[test]
    fn reports_in_use_while_a_listener_holds_the_port() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();

        match check_port(port) {
            PortStatus::InUse { .. } => {}
            other => panic!("expected InUse, got {other:?}"),
        }
        drop(listener);
    }

    #[cfg(windows)]
    #[test]
    fn identifies_the_current_process_as_the_owner() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();

        let status = check_port(port);
        drop(listener);

        match status {
            PortStatus::InUse { pid: Some(pid), .. } => {
                assert_eq!(pid, std::process::id());
            }
            other => panic!("expected InUse with our own PID, got {other:?}"),
        }
    }
}
