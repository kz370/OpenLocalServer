//! Services sleep when unused, wake on connect (§179).
//!
//! Opt-in per service (`services.idle`), for MariaDB, PostgreSQL, Redis, MongoDB and
//! Memcached only. Once a minute (in the scheduler tick, and only when a service has it
//! turned on) one `netstat` run counts established connections per service port. A service
//! with none for N minutes is stopped; a small listener on `127.0.0.1:<port>` then starts it
//! again when the first app connects and forwards that connection. A service the user
//! stopped or started by hand is never touched: those commands call [`IdleState::release`].

use std::collections::{BTreeMap, HashMap};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::app::Inner;
use crate::error::CoreError;

pub const SETTING: &str = "services.idle";
/// The only services this applies to.
pub const SCOPE: &[&str] = &["mariadb", "postgres", "redis", "mongodb", "memcached"];
const DEFAULT_MINUTES: u32 = 30;

fn yes() -> bool {
    true
}
fn thirty() -> u32 {
    DEFAULT_MINUTES
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdleConfig {
    #[serde(default)]
    pub enabled: bool,
    /// 5 to 240.
    #[serde(default = "thirty")]
    pub minutes: u32,
    #[serde(default = "yes")]
    pub wake_on_connect: bool,
}

pub type IdleSettings = BTreeMap<String, IdleConfig>;

// ------------------------------------------------------------------ pure helpers

/// Established TCP connections whose *local* port is one of `ports`, from `netstat -ano -p TCP`.
pub fn established_on_ports(output: &str, ports: &[u16]) -> HashMap<u16, u32> {
    let mut counts: HashMap<u16, u32> = HashMap::new();
    for line in output.lines() {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 5 || !cols[0].eq_ignore_ascii_case("TCP") {
            continue;
        }
        if !cols[3].eq_ignore_ascii_case("ESTABLISHED") {
            continue;
        }
        let Some(port) = cols[1]
            .rsplit_once(':')
            .and_then(|(_, p)| p.parse::<u16>().ok())
        else {
            continue;
        };
        if ports.contains(&port) {
            *counts.entry(port).or_insert(0) += 1;
        }
    }
    counts
}

/// `true` when the service has been quiet for `minutes` or longer.
pub fn should_sleep(last_active: Instant, now: Instant, minutes: u32) -> bool {
    now.saturating_duration_since(last_active) >= Duration::from_secs(u64::from(minutes) * 60)
}

/// A line parsed at all? `netstat` words are localized on some systems; `TCP` and
/// `ESTABLISHED` are not, but an output with no TCP line is not trusted.
fn looks_like_netstat(output: &str) -> bool {
    output.lines().any(|l| {
        l.split_whitespace()
            .next()
            .is_some_and(|w| w.eq_ignore_ascii_case("TCP"))
    })
}

pub fn validate(settings: &IdleSettings) -> Result<(), CoreError> {
    for (id, c) in settings {
        if !SCOPE.contains(&id.as_str()) {
            return Err(CoreError::failed_fix(
                "Sleep when unused is not available for that service.",
                format!("\"{id}\" is not one of MariaDB, PostgreSQL, Redis, MongoDB or Memcached."),
                "Choose one of those services.",
            ));
        }
        if !(5..=240).contains(&c.minutes) {
            return Err(CoreError::failed_fix(
                "The time is out of range.",
                format!("{} minutes for {id}; it must be 5 to 240.", c.minutes),
                "Pick a time between 5 minutes and 4 hours.",
            ));
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ the wake listener

/// One listener thread's handle.
struct Waker {
    release: Arc<AtomicBool>,
    /// `true` while the thread still holds the port.
    holding: Arc<AtomicBool>,
}

/// Binds `127.0.0.1:port`, waits for the first connection, frees the port, runs `start` (which
/// must bring the real service up on that port) and then forwards the waiting clients to it.
/// Returns once the thread is running; the work happens on that thread.
fn spawn_waker(
    name: String,
    port: u16,
    start: Arc<dyn Fn() -> bool + Send + Sync>,
    done: Arc<dyn Fn(bool) + Send + Sync>,
) -> Waker {
    let release = Arc::new(AtomicBool::new(false));
    let holding = Arc::new(AtomicBool::new(true));
    let (rel, hold) = (release.clone(), holding.clone());
    let spawned = std::thread::Builder::new().name(format!("ols-wake-{name}")).spawn(move || {
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        // The service has just stopped; its port can take a moment to free.
        let mut listener = None;
        for _ in 0..40 {
            if rel.load(Ordering::SeqCst) {
                break;
            }
            match TcpListener::bind(addr) {
                Ok(l) => {
                    listener = Some(l);
                    break;
                }
                Err(_) => std::thread::sleep(Duration::from_millis(250)),
            }
        }
        let Some(listener) = listener else {
            tracing::warn!(service = %name, port, "wake listener could not bind; the service stays stopped");
            hold.store(false, Ordering::SeqCst);
            return;
        };
        let _ = listener.set_nonblocking(true);
        let mut clients: Vec<TcpStream> = Vec::new();
        loop {
            if rel.load(Ordering::SeqCst) {
                drop(listener);
                hold.store(false, Ordering::SeqCst);
                return;
            }
            match listener.accept() {
                Ok((s, _)) => {
                    clients.push(s);
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(_) => std::thread::sleep(Duration::from_millis(200)),
            }
        }
        // More clients may be waiting right behind the first.
        let until = Instant::now() + Duration::from_millis(200);
        while Instant::now() < until {
            match listener.accept() {
                Ok((s, _)) => clients.push(s),
                Err(_) => std::thread::sleep(Duration::from_millis(20)),
            }
        }
        drop(listener);
        hold.store(false, Ordering::SeqCst);

        let ok = start();
        if ok {
            for client in clients {
                forward(client, addr);
            }
        }
        // On failure the clients are simply dropped (closed).
        done(ok);
    });
    if spawned.is_err() {
        holding.store(false, Ordering::SeqCst);
    }
    Waker { release, holding }
}

/// Pipes `client` to the real service in both directions.
fn forward(client: TcpStream, to: SocketAddr) {
    let Ok(upstream) = TcpStream::connect_timeout(&to, Duration::from_secs(5)) else {
        return;
    };
    let _ = client.set_nonblocking(false);
    let (Ok(mut c_read), Ok(mut c_write), Ok(mut u_read), Ok(mut u_write)) = (
        client.try_clone(),
        client.try_clone(),
        upstream.try_clone(),
        upstream.try_clone(),
    ) else {
        return;
    };
    std::thread::spawn(move || {
        let _ = std::io::copy(&mut c_read, &mut u_write);
        let _ = u_write.shutdown(Shutdown::Both);
    });
    std::thread::spawn(move || {
        let _ = std::io::copy(&mut u_read, &mut c_write);
        let _ = c_write.shutdown(Shutdown::Both);
    });
}

// ------------------------------------------------------------------ state

struct Entry {
    last_active: Instant,
    slept: bool,
    waker: Option<Waker>,
}

/// Per-service idle bookkeeping, kept in memory.
#[derive(Default)]
pub struct IdleState {
    entries: Mutex<HashMap<String, Entry>>,
    /// Test hook: how many times `netstat` was run.
    netstat_runs: AtomicUsize,
    warned: AtomicBool,
}

impl IdleState {
    pub fn is_sleeping(&self, id: &str) -> bool {
        self.entries
            .lock()
            .unwrap()
            .get(id)
            .is_some_and(|e| e.slept)
    }

    pub fn netstat_runs(&self) -> usize {
        self.netstat_runs.load(Ordering::SeqCst)
    }

    /// A manual start, stop or restart wins: stop the listener (waiting until the port is
    /// free) and forget that the service was asleep.
    pub fn release(&self, id: &str) {
        let waker = {
            let mut entries = self.entries.lock().unwrap();
            match entries.get_mut(id) {
                Some(e) => {
                    e.slept = false;
                    e.last_active = Instant::now();
                    e.waker.take()
                }
                None => None,
            }
        };
        if let Some(w) = waker {
            w.release.store(true, Ordering::SeqCst);
            let until = Instant::now() + Duration::from_secs(2);
            while w.holding.load(Ordering::SeqCst) && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }

    pub fn release_all(&self) {
        let ids: Vec<String> = self.entries.lock().unwrap().keys().cloned().collect();
        for id in ids {
            self.release(&id);
        }
    }
}

pub fn load(inner: &Inner) -> IdleSettings {
    inner
        .settings
        .lock()
        .unwrap()
        .get(SETTING)
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

pub fn save(inner: &Inner, settings: IdleSettings) -> Result<IdleSettings, CoreError> {
    validate(&settings)?;
    let value = serde_json::to_value(&settings)?;
    inner.settings.lock().unwrap().set(SETTING, value)?;
    // A service that is no longer enabled must not keep a listener on its port.
    for (id, c) in &settings {
        if !c.enabled || !c.wake_on_connect {
            inner.idle.release(id);
        }
    }
    Ok(settings)
}

/// Runs once a minute from the scheduler thread. Returns at once when nothing is enabled.
pub fn tick(inner: &Arc<Inner>) {
    let settings: IdleSettings = load(inner).into_iter().filter(|(_, c)| c.enabled).collect();
    if settings.is_empty() {
        return;
    }
    let now = Instant::now();
    // Which enabled services run, and on which port.
    let mut running: Vec<(String, u16, IdleConfig)> = Vec::new();
    for (id, cfg) in &settings {
        if !inner.services.is_running(id) {
            continue;
        }
        let Some(port) = inner.services.status(id).port else {
            continue;
        };
        running.push((id.clone(), port, cfg.clone()));
    }
    if running.is_empty() {
        return;
    }
    let ports: Vec<u16> = running.iter().map(|(_, p, _)| *p).collect();
    inner.idle.netstat_runs.fetch_add(1, Ordering::SeqCst);
    let out = crate::exec::run_capture(
        std::path::Path::new("netstat"),
        &["-ano".to_string(), "-p".to_string(), "TCP".to_string()],
        None,
        &[],
        Duration::from_secs(5),
    );
    if !out.success() || !looks_like_netstat(&out.stdout) {
        if !inner.idle.warned.swap(true, Ordering::SeqCst) {
            tracing::warn!("netstat gave no readable answer; services are not put to sleep");
        }
        return;
    }
    let counts = established_on_ports(&out.stdout, &ports);
    for (id, port, cfg) in running {
        let connections = counts.get(&port).copied().unwrap_or(0);
        let sleep_now = {
            let mut entries = inner.idle.entries.lock().unwrap();
            let e = entries.entry(id.clone()).or_insert(Entry {
                last_active: now,
                slept: false,
                waker: None,
            });
            e.slept = false;
            if connections > 0 {
                e.last_active = now;
                false
            } else {
                should_sleep(e.last_active, now, cfg.minutes)
            }
        };
        if sleep_now {
            put_to_sleep(inner, &id, port, &cfg);
        }
    }
}

fn put_to_sleep(inner: &Arc<Inner>, id: &str, port: u16, cfg: &IdleConfig) {
    if inner.services.stop(id).is_err() {
        return;
    }
    let name = inner.services.status(id).name;
    let wake = cfg.wake_on_connect;
    let waker = if wake {
        let starter = Arc::clone(inner);
        let sid = id.to_string();
        let start: Arc<dyn Fn() -> bool + Send + Sync> =
            Arc::new(move || starter.start_service_and_wait(&sid, &mut |_| {}).is_ok());
        let finisher = Arc::clone(inner);
        let fid = id.to_string();
        let done: Arc<dyn Fn(bool) + Send + Sync> = Arc::new(move |_ok| {
            if let Some(e) = finisher.idle.entries.lock().unwrap().get_mut(&fid) {
                e.slept = false;
                e.last_active = Instant::now();
                e.waker = None;
            }
        });
        Some(spawn_waker(id.to_string(), port, start, done))
    } else {
        None
    };
    {
        let mut entries = inner.idle.entries.lock().unwrap();
        let e = entries.entry(id.to_string()).or_insert(Entry {
            last_active: Instant::now(),
            slept: true,
            waker: None,
        });
        e.slept = true;
        e.waker = waker;
    }
    let message = if wake {
        format!("{name} was stopped after {} minutes without use. It starts again when an app connects.", cfg.minutes)
    } else {
        format!(
            "{name} was stopped after {} minutes without use. Start it on the Services page.",
            cfg.minutes
        )
    };
    let target = id.to_string();
    inner.tasks.start(
        "idle-sleep",
        &format!("{name} is sleeping"),
        &target,
        1,
        move |h| {
            h.result(message);
            h.succeed();
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    const NETSTAT: &str = "\r\nActive Connections\r\n\r\n  Proto  Local Address          Foreign Address        State           PID\r\n  TCP    0.0.0.0:3306           0.0.0.0:0              LISTENING       4120\r\n  TCP    127.0.0.1:3306         127.0.0.1:50211        ESTABLISHED     4120\r\n  TCP    127.0.0.1:50211        127.0.0.1:3306         ESTABLISHED     9988\r\n  TCP    127.0.0.1:3306         127.0.0.1:50300        ESTABLISHED     4120\r\n  TCP    [::1]:6379             [::1]:50400            ESTABLISHED     5000\r\n  TCP    [::]:5432              [::]:0                 LISTENING       777\r\n  TCP    192.168.1.5:3306       192.168.1.9:61000      TIME_WAIT       0\r\n";

    #[test]
    fn netstat_counts_established_connections_by_local_port() {
        let c = established_on_ports(NETSTAT, &[3306, 6379, 5432]);
        assert_eq!(
            c.get(&3306),
            Some(&2),
            "only local-port matches, not the client side"
        );
        assert_eq!(c.get(&6379), Some(&1), "IPv6 loopback");
        assert_eq!(c.get(&5432), None, "LISTENING is ignored");
        assert!(looks_like_netstat(NETSTAT));
        assert!(!looks_like_netstat("garbage\nlines"));
    }

    #[test]
    fn should_sleep_at_the_boundary() {
        let start = Instant::now();
        let m = |n: u64| start + Duration::from_secs(n * 60);
        assert!(!should_sleep(start, m(29), 30));
        assert!(should_sleep(start, m(30), 30));
        assert!(should_sleep(start, m(31), 30));
        assert!(!should_sleep(start, start, 5));
    }

    #[test]
    fn settings_are_validated() {
        let mut s = IdleSettings::new();
        s.insert(
            "mariadb".into(),
            IdleConfig {
                enabled: true,
                minutes: 30,
                wake_on_connect: true,
            },
        );
        assert!(validate(&s).is_ok());
        s.insert(
            "nginx".into(),
            IdleConfig {
                enabled: true,
                minutes: 30,
                wake_on_connect: true,
            },
        );
        assert!(validate(&s).is_err());
        s.remove("nginx");
        s.get_mut("mariadb").unwrap().minutes = 4;
        assert!(validate(&s).is_err());
        s.get_mut("mariadb").unwrap().minutes = 241;
        assert!(validate(&s).is_err());
    }

    fn free_port() -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    }

    #[test]
    fn a_connection_wakes_the_service_and_gets_its_answer() {
        let port = free_port();
        // The fake service: starting it binds the same port and greets every client with "G".
        let start: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(move || {
            let l = TcpListener::bind(("127.0.0.1", port)).unwrap();
            std::thread::spawn(move || {
                while let Ok((mut s, _)) = l.accept() {
                    let _ = std::io::Write::write_all(&mut s, b"G");
                }
            });
            true
        });
        let finished = Arc::new(AtomicBool::new(false));
        let f2 = finished.clone();
        let done: Arc<dyn Fn(bool) + Send + Sync> =
            Arc::new(move |ok| f2.store(ok, Ordering::SeqCst));
        let _waker = spawn_waker("test".into(), port, start, done);
        // Wait for the wake listener to hold the port, then connect like an app would.
        let mut client = loop {
            if let Ok(c) = TcpStream::connect(("127.0.0.1", port)) {
                break c;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        client
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut byte = [0u8; 1];
        client.read_exact(&mut byte).unwrap();
        assert_eq!(&byte, b"G");
        assert!(finished.load(Ordering::SeqCst));
    }

    #[test]
    fn release_frees_the_port_quickly_and_the_thread_ends() {
        let port = free_port();
        let start: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(|| true);
        let done: Arc<dyn Fn(bool) + Send + Sync> = Arc::new(|_| {});
        let state = IdleState::default();
        let waker = spawn_waker("rel".into(), port, start, done);
        // Wait until it holds the port.
        let t0 = Instant::now();
        while TcpListener::bind(("127.0.0.1", port)).is_ok()
            && t0.elapsed() < Duration::from_secs(3)
        {
            std::thread::sleep(Duration::from_millis(20));
        }
        state.entries.lock().unwrap().insert(
            "rel".into(),
            Entry {
                last_active: Instant::now(),
                slept: true,
                waker: Some(waker),
            },
        );
        let t = Instant::now();
        state.release("rel");
        assert!(
            t.elapsed() < Duration::from_secs(1),
            "release took {:?}",
            t.elapsed()
        );
        assert!(!state.is_sleeping("rel"));
        assert!(
            TcpListener::bind(("127.0.0.1", port)).is_ok(),
            "the port is free again"
        );
    }

    #[test]
    fn tick_runs_no_process_when_nothing_is_enabled() {
        let home = crate::test_support::isolated_home();
        let settings = crate::settings::SettingsService::load(&home.paths).unwrap();
        let inner = Inner::new(settings, home.paths.clone()).unwrap();
        tick(&inner);
        assert_eq!(inner.idle.netstat_runs(), 0);
        // Enabled, but the service is not running: still no process.
        let mut s = IdleSettings::new();
        s.insert(
            "redis".into(),
            IdleConfig {
                enabled: true,
                minutes: 5,
                wake_on_connect: true,
            },
        );
        save(&inner, s).unwrap();
        tick(&inner);
        assert_eq!(inner.idle.netstat_runs(), 0);
        // Reading what was saved.
        assert!(load(&inner)["redis"].enabled);
    }
}
