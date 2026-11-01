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

/// Cheap free/in-use test: tests both wildcard (0.0.0.0) and loopback (127.0.0.1).
/// On Windows, if a service (e.g. Nginx, Apache) binds to 0.0.0.0:port, Winsock
/// still allows another socket to bind to 127.0.0.1:port. Conversely, if a service
/// (e.g. MariaDB) binds specifically to 127.0.0.1:port, binding to 0.0.0.0:port can
/// succeed while 127.0.0.1 fails. Both addresses must be bindable for the port to be free.
pub fn port_is_free(port: u16) -> bool {
    let any_free = TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], port))).is_ok();
    let loop_free = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port))).is_ok();
    any_free && loop_free
}

/// Check whether `port` is free. If it's busy, best-effort identify the
/// owning process (Windows: parse `netstat -ano` / `Get-NetTCPConnection` + `tasklist`)
/// so the UI can show the process name and PID (e.g. "nginx.exe (PID 27272)")
/// instead of a bare failure or false "Free".
pub fn check_port(port: u16) -> PortStatus {
    if port_is_free(port) {
        return PortStatus::Free;
    }

    // netstat + tasklist cost ~0.5s and the UI polls this; owners rarely change, so cache.
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<
            std::collections::HashMap<u16, (std::time::Instant, Option<u32>, Option<String>)>,
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
    let (pid, process_name) = resolve_in_use_owner(port);
    let process_name = process_name
        .or_else(|| excluded_range_reason(port))
        .or_else(|| known_port_hint(port));
    cache
        .lock()
        .unwrap()
        .insert(port, (std::time::Instant::now(), pid, process_name.clone()));
    PortStatus::InUse { pid, process_name }
}

#[cfg(windows)]
fn resolve_in_use_owner(port: u16) -> (Option<u32>, Option<String>) {
    // 1) Fast path: netstat
    if let Some(pid) = find_owning_pid_netstat(port) {
        if pid > 0 {
            if let Some(name) = find_process_name(pid) {
                if !name.eq_ignore_ascii_case("System Idle Process") {
                    return (Some(pid), Some(name));
                }
            }
        }
    }

    // 2) PowerShell Get-NetTCPConnection fallback: handles state 0, dual-stack, or sockets where netstat misses/reports PID 0
    if let Some((pid, name)) = find_owner_powershell(port) {
        if pid > 0 && !name.eq_ignore_ascii_case("System Idle Process") {
            let exe_name = if name.ends_with(".exe") {
                name
            } else {
                format!("{name}.exe")
            };
            return (Some(pid), Some(exe_name));
        }
    }

    (None, None)
}

#[cfg(not(windows))]
fn resolve_in_use_owner(_port: u16) -> (Option<u32>, Option<String>) {
    (None, None)
}

#[cfg(windows)]
fn find_owning_pid_netstat(port: u16) -> Option<u32> {
    let mut cmd = std::process::Command::new(crate::exec::windows_system_dir().join("netstat.exe"));
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
            // PID 0 is System Idle Process (assigned by Windows to closed/TIME_WAIT entries).
            // Never treat PID 0 as the owning process of a port.
            if pid == 0 {
                continue;
            }
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

#[cfg(windows)]
fn find_owner_powershell(port: u16) -> Option<(u32, String)> {
    let mut cmd = std::process::Command::new(crate::exec::powershell_path());
    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        &format!(
            "Get-NetTCPConnection -LocalPort {port} -ErrorAction SilentlyContinue | Where-Object {{ $_.OwningProcess -gt 0 }} | ForEach-Object {{ $p = Get-Process -Id $_.OwningProcess -ErrorAction SilentlyContinue; if ($p) {{ \"$($p.Id)|$($p.ProcessName)\" }} }} | Select-Object -First 1"
        ),
    ]);
    crate::exec::hide_window(&mut cmd);
    let output = cmd.output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut parts = trimmed.splitn(2, '|');
    let pid = parts
        .next()?
        .trim()
        .parse::<u32>()
        .ok()
        .filter(|&p| p > 0)?;
    let name = parts.next()?.trim().to_string();
    if name.is_empty() || name.eq_ignore_ascii_case("System Idle Process") {
        None
    } else {
        Some((pid, name))
    }
}

fn known_port_hint(port: u16) -> Option<String> {
    match port {
        80 => Some("HTTP Web Server (port 80)".to_string()),
        443 => Some("HTTPS Web Server (port 443)".to_string()),
        3306 => Some("MySQL / MariaDB (standard port 3306)".to_string()),
        5432 => Some("PostgreSQL (standard port 5432)".to_string()),
        6379 => Some("Redis (standard port 6379)".to_string()),
        8025 => Some("Mailpit Web UI (port 8025)".to_string()),
        1025 => Some("Mailpit SMTP (port 1025)".to_string()),
        11211 => Some("Memcached (standard port 11211)".to_string()),
        27017 => Some("MongoDB (standard port 27017)".to_string()),
        _ => None,
    }
}

/// Port binds fail with nobody listening when Windows reserves the range
/// (Hyper-V / Docker / WSL exclude blocks via `netsh interface ipv4 show
/// excludedportrange`). Name that so the UI proposes the real fix instead of
/// a ghost holder. Best-effort: any parse failure means "not excluded".
#[cfg(windows)]
fn excluded_range_reason(port: u16) -> Option<String> {
    let mut cmd = std::process::Command::new(crate::exec::windows_system_dir().join("netsh.exe"));
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
    if pid == 0 {
        return None;
    }
    let mut cmd =
        std::process::Command::new(crate::exec::windows_system_dir().join("tasklist.exe"));
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
    if name.is_empty()
        || name.starts_with("INFO:")
        || name.eq_ignore_ascii_case("System Idle Process")
    {
        None
    } else {
        Some(name.to_string())
    }
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
        let _conn = std::net::TcpStream::connect(("127.0.0.1", port)).ok();

        let status = check_port(port);
        drop(_conn);
        drop(listener);

        match status {
            PortStatus::InUse { pid: Some(pid), .. } => {
                assert_eq!(pid, std::process::id());
            }
            other => panic!("expected InUse with our own PID, got {other:?}"),
        }
    }
}
