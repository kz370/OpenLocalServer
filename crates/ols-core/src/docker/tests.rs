//! Docker manager tests against a fake engine on a local TCP port, so they run anywhere
//! without Docker installed.

use super::*;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;

/// A fake engine: answers each request with `respond(method, path)` and counts them.
pub(crate) struct FakeEngine {
    pub endpoint: Endpoint,
    pub hits: Arc<AtomicUsize>,
    pub seen: Arc<Mutex<Vec<String>>>,
}

impl FakeEngine {
    pub fn start(respond: fn(&str, &str) -> (u16, String)) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let hits = Arc::new(AtomicUsize::new(0));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let (h, s) = (hits.clone(), seen.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    continue;
                }
                // Drain headers.
                let mut header = String::new();
                while reader.read_line(&mut header).is_ok() && header != "\r\n" {
                    header.clear();
                }
                let mut parts = line.split_whitespace();
                let method = parts.next().unwrap_or("").to_string();
                let path = parts.next().unwrap_or("").to_string();
                h.fetch_add(1, Ordering::SeqCst);
                s.lock().unwrap().push(format!("{method} {path}"));
                let (status, body) = respond(&method, &path);
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        FakeEngine {
            endpoint: Endpoint::Tcp(addr.to_string()),
            hits,
            seen,
        }
    }

    pub fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }
}

fn engine(method: &str, path: &str) -> (u16, String) {
    match (method, path) {
        ("GET", "/version") => (
            200,
            r#"{"Version":"27.3.1","ApiVersion":"1.47","Os":"linux"}"#.into(),
        ),
        ("GET", p) if p.starts_with("/containers/json") => (
            200,
            r#"[{"Id":"b1","Names":["/zeta"],"Image":"redis","State":"running","Status":"Up","Created":1,"Ports":[],"Labels":{}},
                {"Id":"a1","Names":["/shop-web-1"],"Image":"nginx","State":"exited","Status":"Exited (0)","Created":2,
                 "Ports":[{"IP":"0.0.0.0","PrivatePort":80,"PublicPort":8080,"Type":"tcp"}],
                 "Labels":{"com.docker.compose.project":"shop"}}]"#
                .into(),
        ),
        ("POST", "/containers/a1/start") => (204, String::new()),
        ("POST", "/containers/a1/stop") => (304, String::new()),
        ("POST", "/containers/gone/start") => (404, r#"{"message":"No such container: gone"}"#.into()),
        _ => (500, r#"{"message":"unexpected"}"#.into()),
    }
}

#[test]
fn status_reports_running_engine() {
    let fake = FakeEngine::start(engine);
    let s = DockerManager::new().status(&fake.endpoint);
    assert!(s.running && s.installed);
    assert_eq!(s.version.as_deref(), Some("27.3.1"));
    assert_eq!(s.os_type.as_deref(), Some("linux"));
    assert!(s.message.is_none());
}

#[test]
fn status_reports_stopped_engine_without_error() {
    // A port nothing listens on.
    let free = TcpListener::bind("127.0.0.1:0").unwrap();
    let ep = Endpoint::Tcp(free.local_addr().unwrap().to_string());
    drop(free);
    let s = DockerManager::new().status(&ep);
    assert!(!s.running);
    assert!(s.message.is_some());
}

#[test]
fn list_groups_compose_first_and_is_cached() {
    let fake = FakeEngine::start(engine);
    let m = DockerManager::new();
    let list = m.list_containers(&fake.endpoint, true).unwrap();
    assert_eq!(list[0].name, "shop-web-1", "compose stacks sort first");
    assert_eq!(list[1].name, "zeta");
    assert_eq!(list[0].ports[0].host_port, Some(8080));
    // Performance rule: a second list inside the cache window makes no engine request.
    let again = m.list_containers(&fake.endpoint, true).unwrap();
    assert_eq!(again, list);
    assert_eq!(fake.hits(), 1);
    // A different filter is a different question.
    m.list_containers(&fake.endpoint, false).unwrap();
    assert_eq!(fake.hits(), 2);
}

#[test]
fn actions_hit_the_right_path_and_clear_the_cache() {
    let fake = FakeEngine::start(engine);
    let m = DockerManager::new();
    m.list_containers(&fake.endpoint, true).unwrap();
    m.container_action(&fake.endpoint, "a1", ContainerAction::Start)
        .unwrap();
    // 304 "already stopped" is success.
    m.container_action(&fake.endpoint, "a1", ContainerAction::Stop)
        .unwrap();
    m.list_containers(&fake.endpoint, true).unwrap();
    let seen = fake.seen.lock().unwrap().clone();
    assert_eq!(
        seen,
        vec![
            "GET /containers/json?all=1",
            "POST /containers/a1/start",
            "POST /containers/a1/stop",
            "GET /containers/json?all=1",
        ]
    );
}

#[test]
fn missing_container_is_a_clear_diagnostic() {
    let fake = FakeEngine::start(engine);
    let err = DockerManager::new()
        .container_action(&fake.endpoint, "gone", ContainerAction::Start)
        .unwrap_err();
    let d = crate::error::Diagnostic::from(&err);
    assert!(d.problem.contains("no longer exists"), "{d:?}");
    assert!(d.cause.contains("No such container"), "{d:?}");
    assert!(d.fix.is_some());
}

#[test]
fn unsafe_container_ref_never_reaches_the_engine() {
    let fake = FakeEngine::start(engine);
    let err = DockerManager::new()
        .container_action(&fake.endpoint, "../../images/x", ContainerAction::Start)
        .unwrap_err();
    assert!(crate::error::Diagnostic::from(&err).fix.is_some());
    assert_eq!(fake.hits(), 0);
}

#[test]
fn stopped_engine_maps_to_start_docker_fix() {
    let free = TcpListener::bind("127.0.0.1:0").unwrap();
    let ep = Endpoint::Tcp(free.local_addr().unwrap().to_string());
    drop(free);
    let err = DockerManager::new().list_containers(&ep, true).unwrap_err();
    let d = crate::error::Diagnostic::from(&err);
    assert_eq!(d.problem, "Docker is not running.");
    assert!(d.fix.unwrap().contains("Start Docker"));
}

#[test]
fn hung_engine_times_out_instead_of_blocking() {
    // Accepts the connection, never answers.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let ep = Endpoint::Tcp(listener.local_addr().unwrap().to_string());
    std::thread::spawn(move || {
        let _held: Vec<_> = listener.incoming().take(1).collect();
        std::thread::sleep(Duration::from_secs(10));
    });
    let started = Instant::now();
    let r = client::request(&ep, "GET", "/version", None, Duration::from_millis(300));
    assert!(matches!(r, Err(ClientError::Timeout(_))), "{r:?}");
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn endpoint_setting_wins_and_bad_values_fall_through() {
    assert_eq!(
        resolve_endpoint(Some("tcp://127.0.0.1:2375")),
        Endpoint::Tcp("127.0.0.1:2375".into())
    );
    // Garbage in the setting is ignored rather than breaking Docker entirely.
    let fallback = resolve_endpoint(Some("not an endpoint"));
    assert!(matches!(
        fallback,
        Endpoint::NamedPipe(_) | Endpoint::Unix(_) | Endpoint::Tcp(_)
    ));
}

/// Performance rule (plan §2.5): opening the app — building the core and loading the
/// Dashboard and Services list the UI shows first — makes no Docker engine request.
/// Only a Docker command does.
#[test]
fn app_startup_makes_no_docker_requests() {
    use crate::command::{CoreCommand, CoreResponse};
    let fake = FakeEngine::start(engine);
    let home = crate::test_support::isolated_home();
    let mut settings = crate::settings::SettingsService::load(&home.paths).unwrap();
    settings
        .set(
            SETTING_ENDPOINT,
            serde_json::Value::String(format!(
                "tcp://{}",
                match &fake.endpoint {
                    Endpoint::Tcp(a) => a.clone(),
                    _ => unreachable!(),
                }
            )),
        )
        .unwrap();
    let core = crate::Core::new(settings, home.paths.clone());
    let _ = core.dispatch(CoreCommand::GetDashboard);
    let _ = core.dispatch(CoreCommand::ListServices);
    assert_eq!(fake.hits(), 0, "startup must not talk to Docker");
    assert_eq!(core.inner().docker.request_count(), 0);

    let CoreResponse::DockerStatus { status } =
        core.dispatch(CoreCommand::GetDockerStatus).unwrap()
    else {
        panic!("expected a Docker status")
    };
    assert!(status.running);
    assert_eq!(fake.hits(), 1);
}

#[test]
fn framed_and_plain_logs_become_lines() {
    let mut framed = vec![1, 0, 0, 0, 0, 0, 0, 6];
    framed.extend_from_slice(b"hello\n");
    framed.extend_from_slice(&[2, 0, 0, 0, 0, 0, 0, 12]);
    framed.extend_from_slice(b"oops\r\nworld\n");
    assert_eq!(split_log_stream(&framed), vec!["hello", "oops", "world"]);
    assert_eq!(
        split_log_stream(b"plain one\nplain two\n"),
        vec!["plain one", "plain two"]
    );
}
