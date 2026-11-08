//! Live updates from Docker (§167): while the Docker page is open, one thread follows the
//! engine's `/events` stream and tells the UI (debounced) when a container changed.
//!
//! Rules (plan §2.5, binding):
//! - Nothing starts with the app: the thread exists only between `docker_watch(true)` and
//!   the matching `docker_watch(false)` (ref-counted, one thread however many watchers).
//! - No polling: the thread blocks on the stream; it wakes 4 times a second only to check
//!   its stop flag and the debounce timer.
//! - An engine that is down is retried with a growing pause (2, 5, 15, 60 s) and logged once.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::client::{self, ClientError};
use crate::app::Inner;

/// Container events worth a refresh.
const FILTERS: &str =
    r#"{"type":["container"],"event":["start","die","destroy","rename","health_status"]}"#;
/// How often the stream wakes to check the stop flag and the debounce timer.
const POLL: Duration = Duration::from_millis(250);
/// A burst of events becomes one refresh.
const DEBOUNCE: Duration = Duration::from_millis(250);
const BACKOFF: [u64; 4] = [2, 5, 15, 60];

/// Who is watching and how to stop the thread.
#[derive(Default)]
pub(crate) struct WatchState {
    count: usize,
    stop: Option<Arc<AtomicBool>>,
    /// Live events threads (0 or 1; briefly 2 while an old one winds down).
    alive: Arc<AtomicUsize>,
}

pub(crate) fn watching(docker: &super::DockerManager) -> bool {
    docker.watch.lock().map(|w| w.count > 0).unwrap_or(false)
}

/// Events threads still running (tests assert this returns to 0).
pub fn watch_threads(docker: &super::DockerManager) -> usize {
    docker
        .watch
        .lock()
        .map(|w| w.alive.load(Ordering::SeqCst))
        .unwrap_or(0)
}

struct AliveGuard(Arc<AtomicUsize>);
impl Drop for AliveGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Inner {
    /// `on = true` registers a watcher (the first one starts the events thread);
    /// `on = false` removes one (the last one stops it).
    pub fn docker_watch(self: &Arc<Self>, on: bool) {
        let start = {
            let mut w = self.docker.watch.lock().unwrap();
            if on {
                w.count += 1;
                if w.count == 1 {
                    let stop = Arc::new(AtomicBool::new(false));
                    w.stop = Some(stop.clone());
                    w.alive.fetch_add(1, Ordering::SeqCst);
                    Some((stop, AliveGuard(w.alive.clone())))
                } else {
                    None
                }
            } else {
                w.count = w.count.saturating_sub(1);
                if w.count == 0 {
                    if let Some(stop) = w.stop.take() {
                        stop.store(true, Ordering::SeqCst);
                    }
                }
                None
            }
        };
        if let Some((stop, guard)) = start {
            let inner = self.clone();
            let spawned = std::thread::Builder::new()
                .name("ols-docker-events".into())
                .spawn(move || {
                    let _guard = guard;
                    inner.watch_loop(&stop);
                });
            if let Err(e) = spawned {
                tracing::warn!("could not start the Docker events thread: {e}");
                // The guard moved into the failed closure and dropped; undo the count.
                let mut w = self.docker.watch.lock().unwrap();
                w.count = 0;
                w.stop = None;
            }
        }
    }

    fn watch_loop(self: &Arc<Self>, stop: &Arc<AtomicBool>) {
        let path = format!("/events?filters={}", client::url_encode(FILTERS));
        let mut attempt = 0usize;
        let mut warned = false;
        while !stop.load(Ordering::Relaxed) {
            let endpoint = self.docker_endpoint();
            let started = Instant::now();
            // After an outage something may have changed unseen: refresh once connected.
            let mut pending: Option<Instant> = (attempt > 0).then(Instant::now);
            let result = client::stream_lines(&endpoint, &path, stop, POLL, |event| {
                if event.is_some() && pending.is_none() {
                    pending = Some(Instant::now());
                }
                if pending.is_some_and(|t| t.elapsed() >= DEBOUNCE) {
                    pending = None;
                    self.docker_refresh_after_event();
                }
            });
            match result {
                Ok(()) => break,
                Err(e) => {
                    if !warned {
                        warned = true;
                        tracing::info!("Docker events stream ended: {}", describe(&e));
                    }
                    if started.elapsed() > Duration::from_secs(5) {
                        attempt = 0;
                        warned = false;
                    }
                    let wait = BACKOFF[attempt.min(BACKOFF.len() - 1)];
                    attempt += 1;
                    let until = Instant::now() + Duration::from_secs(wait);
                    while Instant::now() < until && !stop.load(Ordering::Relaxed) {
                        std::thread::sleep(Duration::from_millis(200));
                    }
                }
            }
        }
    }

    /// Re-reads containers, moves linked sites to new ports, then tells the UI.
    fn docker_refresh_after_event(self: &Arc<Self>) {
        self.docker.forget_list();
        let endpoint = self.docker_endpoint();
        if let Ok(containers) = self.docker.list_containers(&endpoint, true) {
            let moved = self.sync_docker_sites(&containers);
            self.docker_apply_site_moves(moved);
        }
        let _ = self.docker.events.send(());
    }
}

fn describe(e: &ClientError) -> String {
    match e {
        ClientError::NotRunning(w) => format!("Docker is not running ({w})"),
        ClientError::Timeout(_) => "timed out".into(),
        ClientError::Http { status, message } => format!("HTTP {status}: {message}"),
        ClientError::Protocol(m) => m.clone(),
    }
}
