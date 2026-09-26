//! Offline indicators (§128, Stage 17). A quick reachability probe, so the UI can say "you're offline" and
//! name what won't work, instead of letting a download fail with a raw error. It only opens TCP connections
//! to a few well-known hosts (port 443) and sends nothing.

use std::net::ToSocketAddrs;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::app::Inner;

const PROBES: &[(&str, &str, u16)] = &[("Cloudflare DNS", "1.1.1.1", 443), ("GitHub", "github.com", 443), ("Google DNS", "8.8.8.8", 443)];
const CACHE_FOR: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeResult {
    pub name: String,
    pub ok: bool,
    pub ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkStatus {
    pub online: bool,
    pub probes: Vec<ProbeResult>,
    /// What needs the internet, for the offline notice.
    pub needs_internet: Vec<String>,
}

const NEEDS_INTERNET: &[&str] = &[
    "Installing or updating runtimes, services and tools",
    "Plugin and catalog downloads",
    "Checking for a new version of OpenLocalServer",
    "Public tunnels",
    "AI providers that run outside this computer (a local model still works)",
];

static CACHE: Mutex<Option<(Instant, NetworkStatus)>> = Mutex::new(None);

fn probe(name: &str, host: &str, port: u16) -> ProbeResult {
    let start = Instant::now();
    let ok = (host, port).to_socket_addrs().ok().and_then(|mut a| a.next()).is_some_and(|addr| std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(1500)).is_ok());
    ProbeResult { name: name.into(), ok, ms: ok.then(|| start.elapsed().as_millis() as u64) }
}

/// Probes in parallel; online when any host answers.
pub fn check(force: bool) -> NetworkStatus {
    if !force {
        if let Some((at, status)) = CACHE.lock().unwrap().as_ref() {
            if at.elapsed() < CACHE_FOR {
                return status.clone();
            }
        }
    }
    let handles: Vec<_> = PROBES.iter().map(|(n, h, p)| std::thread::spawn(move || probe(n, h, *p))).collect();
    let probes: Vec<ProbeResult> = handles.into_iter().filter_map(|h| h.join().ok()).collect();
    let status = NetworkStatus { online: probes.iter().any(|p| p.ok), probes, needs_internet: NEEDS_INTERNET.iter().map(|s| s.to_string()).collect() };
    *CACHE.lock().unwrap() = Some((Instant::now(), status.clone()));
    status
}

impl Inner {
    pub fn network_status(&self, force: bool) -> NetworkStatus {
        check(force)
    }
}
