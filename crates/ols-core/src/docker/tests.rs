//! Docker manager tests against a fake engine on a local TCP port, so they run anywhere
//! without Docker installed.

use super::*;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
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
        ("GET", "/containers/a1/json") => (
            200,
            r#"{"Id":"a1","Name":"/shop-web-1","Config":{"Image":"nginx","Env":["SECRET_KEY=x"]},"HostConfig":{"RestartPolicy":{"Name":"no"}},"State":{"Status":"running"}}"#.into(),
        ),
        ("POST", "/containers/a1/update") => (200, r#"{"Warnings":[]}"#.into()),
        ("POST", "/containers/a1/rename?name=shop-web-2") => (204, String::new()),
        ("DELETE", "/containers/a1?force=true&v=false") => (204, String::new()),
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

/// "Connect to OLS" is stored per file and shows what it adds before it is used.
#[test]
fn connect_to_ols_round_trips_through_commands() {
    use crate::command::{CoreCommand, CoreResponse};
    let home = crate::test_support::isolated_home();
    let settings = crate::settings::SettingsService::load(&home.paths).unwrap();
    let core = crate::Core::new(settings, home.paths.clone());
    let dir = home.paths.data_dir().join("proj");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("compose.yaml");
    std::fs::write(
        &file,
        "services:\n  web:\n    image: nginx:1.28\n  db:\n    image: mariadb:11.4\n",
    )
    .unwrap();
    let path = file.display().to_string();

    let get = |cmd| match core.dispatch(cmd).unwrap() {
        CoreResponse::DockerComposeConnect { info } => info,
        other => panic!("unexpected {other:?}"),
    };
    let before = get(CoreCommand::GetDockerComposeConnect { path: path.clone() });
    assert!(!before.enabled);
    assert!(before.override_yaml.contains("\"web\":") && before.override_yaml.contains("\"db\":"));
    assert!(before.services.iter().all(|s| s
        .address
        .as_deref()
        .is_none_or(|a| a.starts_with("host.docker.internal:"))));

    let on = get(CoreCommand::SetDockerComposeConnect {
        path: path.clone(),
        enabled: true,
    });
    assert!(on.enabled);
    assert!(
        get(CoreCommand::GetDockerComposeConnect { path: path.clone() }).enabled,
        "stored"
    );
    let off = get(CoreCommand::SetDockerComposeConnect {
        path: path.clone(),
        enabled: false,
    });
    assert!(!off.enabled);
    // Only compose files are accepted.
    assert!(core
        .dispatch(CoreCommand::SetDockerComposeConnect {
            path: dir.join("notes.txt").display().to_string(),
            enabled: true
        })
        .is_err());
}

#[test]
fn container_settings_round_trip() {
    let fake = FakeEngine::start(engine);
    let m = DockerManager::new();
    let d = m.container_details(&fake.endpoint, "a1").unwrap();
    assert_eq!(
        (d.name.as_str(), d.restart_policy.as_str()),
        ("shop-web-1", "no")
    );
    assert!(d.env[0].masked && d.env[0].value.is_empty());
    m.update_container(
        &fake.endpoint,
        "a1",
        &details::ContainerUpdate {
            restart_policy: Some("unless-stopped".into()),
            ..Default::default()
        },
    )
    .unwrap();
    m.rename_container(&fake.endpoint, "a1", "shop-web-2")
        .unwrap();
    assert!(
        m.rename_container(&fake.endpoint, "a1", "bad name&x=1")
            .is_err(),
        "names are checked before the request"
    );
    m.remove_container(&fake.endpoint, "a1", true, false)
        .unwrap();
    let seen = fake.seen.lock().unwrap().clone();
    assert_eq!(
        seen,
        vec![
            "GET /containers/a1/json",
            "POST /containers/a1/update",
            "POST /containers/a1/rename?name=shop-web-2",
            "DELETE /containers/a1?force=true&v=false",
        ]
    );
}

const RECREATE_INSPECT: &str = r#"{"Id":"u1","Name":"/uptime","State":{"Running":true},
    "Config":{"Image":"louislam/uptime-kuma:1","Env":["TZ=UTC","ADMIN_PASSWORD=s3cret"],"Labels":{}},
    "HostConfig":{"NetworkMode":"bridge","Binds":[]},
    "Mounts":[{"Type":"volume","Name":"anonvol","Destination":"/app/data","RW":true}],
    "NetworkSettings":{"Networks":{"bridge":{"Aliases":null}}}}"#;

fn recreate_engine(method: &str, path: &str) -> (u16, String) {
    match (method, path) {
        ("GET", "/containers/u1/json") => (200, RECREATE_INSPECT.into()),
        ("POST", p) if p.starts_with("/containers/u1/rename?name=") => (204, String::new()),
        ("POST", "/containers/create?name=uptime") => (201, r#"{"Id":"n1","Warnings":[]}"#.into()),
        ("POST", "/containers/u1/stop") | ("POST", "/containers/n1/start") => (204, String::new()),
        ("DELETE", "/containers/u1?v=false") => (204, String::new()),
        _ => (500, r#"{"message":"unexpected"}"#.into()),
    }
}

fn recreate_engine_start_fails(method: &str, path: &str) -> (u16, String) {
    match (method, path) {
        ("POST", "/containers/n1/start") => {
            (500, r#"{"message":"port is already allocated"}"#.into())
        }
        ("DELETE", "/containers/n1?force=true&v=false") => (204, String::new()),
        ("POST", "/containers/u1/start") => (204, String::new()),
        _ => recreate_engine(method, path),
    }
}

fn run_recreate(fake: &FakeEngine, edits: Vec<details::EnvEdit>) -> crate::tasks::TaskView {
    let tasks = crate::tasks::TaskManager::new();
    let m = std::sync::Arc::new(DockerManager::new());
    let ep = fake.endpoint.clone();
    let id = tasks.start("t", "t", "recreate", 5, move |h| {
        let r = m.recreate_with_env(&ep, "u1", &edits, &h);
        crate::command::report(&h, r);
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    while tasks.is_running(&id) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    tasks.get(&id).unwrap()
}

#[test]
fn recreate_swaps_containers_and_keeps_data() {
    let fake = FakeEngine::start(recreate_engine);
    let t = run_recreate(
        &fake,
        vec![
            details::EnvEdit {
                key: "TZ".into(),
                value: Some("Africa/Cairo".into()),
            },
            details::EnvEdit {
                key: "ADMIN_PASSWORD".into(),
                value: None,
            },
        ],
    );
    assert!(t.error.is_none(), "{:?}", t.error);
    let seen = fake.seen.lock().unwrap().clone();
    let order: Vec<&str> = seen.iter().map(|s| s.split('?').next().unwrap()).collect();
    assert_eq!(
        order,
        vec![
            "GET /containers/u1/json",
            "POST /containers/u1/rename",
            "POST /containers/create",
            "POST /containers/u1/stop",
            "POST /containers/n1/start",
            "DELETE /containers/u1",
        ]
    );
    assert!(seen[1].contains("name=uptime-ols-old-"), "{}", seen[1]);
}

#[test]
fn recreate_restores_the_old_container_when_the_new_one_fails() {
    let fake = FakeEngine::start(recreate_engine_start_fails);
    let t = run_recreate(
        &fake,
        vec![details::EnvEdit {
            key: "TZ".into(),
            value: Some("UTC".into()),
        }],
    );
    let err = t.error.expect("fails");
    assert!(err.cause.contains("port is already allocated"), "{err:?}");
    assert!(err.problem.contains("old one was restarted"), "{err:?}");
    let seen = fake.seen.lock().unwrap().clone();
    assert_eq!(
        &seen[seen.len() - 3..],
        &[
            "DELETE /containers/n1?force=true&v=false".to_string(),
            "POST /containers/u1/rename?name=uptime".to_string(),
            "POST /containers/u1/start".to_string(),
        ],
        "new removed, old renamed back and started"
    );
}

#[test]
fn container_shell_refuses_unsafe_names() {
    assert!(shell_program("web; rm -rf /").is_err());
    assert!(shell_program("../x").is_err());
    if let Ok((_, args)) = shell_program("shop-web-1") {
        assert_eq!(&args[..3], &["exec", "-it", "shop-web-1"]);
        assert!(
            args.last().unwrap().contains("exec bash") && args.last().unwrap().contains("exec sh")
        );
    }
}

fn storage_engine(method: &str, path: &str) -> (u16, String) {
    match (method, path) {
        ("GET", "/containers/json?all=1") => (
            200,
            r#"[{"Id":"c1","Names":["/web"],"Image":"nginx:1.28","ImageID":"sha256:aaa","State":"running","Status":"Up","Created":1,"Ports":[],"Labels":{},"Mounts":[{"Type":"volume","Name":"webdata"}]}]"#.into(),
        ),
        ("GET", "/images/json") => (200, r#"[{"Id":"sha256:aaa","RepoTags":["nginx:1.28"],"Size":10,"Created":5}]"#.into()),
        ("GET", "/volumes") => (200, r#"{"Volumes":[{"Name":"webdata","Driver":"local","Labels":{}}]}"#.into()),
        ("GET", "/system/df") => (
            200,
            r#"{"Images":[],"Containers":[],"Volumes":[{"Name":"webdata","UsageData":{"Size":2048,"RefCount":1}}],"BuildCache":[]}"#.into(),
        ),
        ("DELETE", "/images/nginx:1.28?force=false") | ("DELETE", "/volumes/webdata") => (204, String::new()),
        ("POST", "/images/prune") => (200, r#"{"SpaceReclaimed":1048576}"#.into()),
        ("POST", "/containers/create?name=my-web") => (201, r#"{"Id":"newc1"}"#.into()),
        ("POST", "/containers/newc1/start") => (204, String::new()),
        _ => (500, r#"{"message":"unexpected"}"#.into()),
    }
}

#[test]
fn images_volumes_and_disk_usage_through_the_engine() {
    let fake = FakeEngine::start(storage_engine);
    let m = DockerManager::new();
    let images = m.list_images(&fake.endpoint).unwrap();
    assert_eq!(images[0].used_by, vec!["web"]);
    let before = m.list_volumes(&fake.endpoint).unwrap();
    assert_eq!(
        (before[0].used_by.clone(), before[0].size),
        (vec!["web".to_string()], None),
        "no size until disk usage is read"
    );
    let du = m.disk_usage(&fake.endpoint, false).unwrap();
    assert_eq!(du.volumes, 2048);
    let df_calls = || {
        fake.seen
            .lock()
            .unwrap()
            .iter()
            .filter(|s| *s == "GET /system/df")
            .count()
    };
    m.disk_usage(&fake.endpoint, false).unwrap();
    assert_eq!(df_calls(), 1, "disk usage is cached");
    assert_eq!(m.list_volumes(&fake.endpoint).unwrap()[0].size, Some(2048));
    m.remove_image(&fake.endpoint, "nginx:1.28", false).unwrap();
    m.disk_usage(&fake.endpoint, false).unwrap();
    assert_eq!(df_calls(), 2, "a removal forgets the cached usage");
    m.remove_volume(&fake.endpoint, "webdata").unwrap();
    assert!(m.remove_volume(&fake.endpoint, "../etc").is_err());
    let id = m
        .run_image(
            &fake.endpoint,
            &storage::RunSpec {
                image: "nginx:1.28".into(),
                name: Some("my-web".into()),
                ports: vec![(8080, 80)],
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(id, "newc1");
}

#[test]
fn prune_runs_only_the_chosen_steps() {
    let fake = FakeEngine::start(storage_engine);
    let tasks = crate::tasks::TaskManager::new();
    let m = std::sync::Arc::new(DockerManager::new());
    let ep = fake.endpoint.clone();
    let id = tasks.start("t", "t", "prune", 1, move |h| {
        let r = m.prune(
            &ep,
            &storage::PruneTargets {
                dangling_images: true,
                ..Default::default()
            },
            &h,
        );
        crate::command::report(&h, r);
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    while tasks.is_running(&id) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let t = tasks.get(&id).unwrap();
    assert!(t.error.is_none(), "{:?}", t.error);
    assert_eq!(
        fake.seen.lock().unwrap().clone(),
        vec!["POST /images/prune"]
    );
    assert!(
        t.results.iter().any(|r| r.contains("Freed about 1 MB")),
        "{:?}",
        t.results
    );
}

/// Turning a Docker site off stops its container and on starts it, in a background task;
/// a site without a Docker link makes no Docker request at all.
#[test]
fn site_switch_drives_its_container() {
    use crate::command::CoreCommand;
    use crate::domain::{DockerLink, Domain, SiteKind};
    let fake = FakeEngine::start(engine);
    let home = crate::test_support::isolated_home();
    let mut settings = crate::settings::SettingsService::load(&home.paths).unwrap();
    let Endpoint::Tcp(addr) = &fake.endpoint else {
        unreachable!()
    };
    settings
        .set(
            SETTING_ENDPOINT,
            serde_json::Value::String(format!("tcp://{addr}")),
        )
        .unwrap();
    let core = crate::Core::new(settings, home.paths.clone());
    let root = home.paths.data_dir().join("shop");
    std::fs::create_dir_all(&root).unwrap();
    let site = |host: &str, port: u16, link: Option<DockerLink>| Domain {
        hostname: host.into(),
        root: root.display().to_string(),
        kind: SiteKind::Proxy {
            upstream_port: port,
            upstream_host: None,
            upstream_https: false,
        },
        docker: link,
        ..Default::default()
    };
    core.inner()
        .add_domain(site(
            "shop.test",
            18080,
            Some(DockerLink {
                container: "shop-web-1".into(),
                container_port: 80,
                compose_project: None,
                compose_service: None,
            }),
        ))
        .unwrap();
    core.inner()
        .add_domain(site("plain.test", 18081, None))
        .unwrap();
    let wait = || {
        let deadline = Instant::now() + Duration::from_secs(10);
        while core
            .inner()
            .tasks
            .list()
            .iter()
            .any(|t| t.kind == "docker-site-power" && t.state.is_running())
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(10));
        }
    };
    core.dispatch(CoreCommand::SetDomainEnabled {
        hostname: "plain.test".into(),
        enabled: false,
    })
    .unwrap();
    wait();
    assert_eq!(
        fake.hits(),
        0,
        "a site with no container makes no Docker request"
    );
    core.dispatch(CoreCommand::SetDomainEnabled {
        hostname: "shop.test".into(),
        enabled: false,
    })
    .unwrap();
    wait();
    core.dispatch(CoreCommand::SetDomainEnabled {
        hostname: "shop.test".into(),
        enabled: true,
    })
    .unwrap();
    wait();
    let posts: Vec<String> = fake
        .seen
        .lock()
        .unwrap()
        .iter()
        .filter(|s| s.starts_with("POST"))
        .cloned()
        .collect();
    assert_eq!(
        posts,
        vec!["POST /containers/a1/stop", "POST /containers/a1/start"]
    );
}

/// One container that refuses to stop must not keep the others running: every linked
/// container is tried, and the task ends failed, naming the one that did not stop.
#[test]
fn one_failing_container_does_not_stop_the_rest() {
    use crate::command::CoreCommand;
    use crate::domain::{DockerLink, Domain, SiteKind};
    fn engine_with_a_stuck_container(method: &str, path: &str) -> (u16, String) {
        match (method, path) {
            ("POST", "/containers/b1/stop") => {
                (500, r#"{"message":"cannot stop container: zeta"}"#.into())
            }
            ("POST", "/containers/a1/stop") => (204, String::new()),
            _ => engine(method, path),
        }
    }
    let fake = FakeEngine::start(engine_with_a_stuck_container);
    let home = crate::test_support::isolated_home();
    let mut settings = crate::settings::SettingsService::load(&home.paths).unwrap();
    let Endpoint::Tcp(addr) = &fake.endpoint else {
        unreachable!()
    };
    settings
        .set(
            SETTING_ENDPOINT,
            serde_json::Value::String(format!("tcp://{addr}")),
        )
        .unwrap();
    let core = crate::Core::new(settings, home.paths.clone());
    let root = home.paths.data_dir().join("shop");
    std::fs::create_dir_all(&root).unwrap();
    let site = |host: &str, port: u16, container: &str| Domain {
        hostname: host.into(),
        root: root.display().to_string(),
        kind: SiteKind::Proxy {
            upstream_port: port,
            upstream_host: None,
            upstream_https: false,
        },
        docker: Some(DockerLink {
            container: container.into(),
            container_port: 80,
            compose_project: None,
            compose_service: None,
        }),
        ..Default::default()
    };
    // The stuck container's site sorts first, so the loop meets the failure first.
    core.inner()
        .add_domain(site("a-cache.test", 18090, "zeta"))
        .unwrap();
    core.inner()
        .add_domain(site("b-shop.test", 18091, "shop-web-1"))
        .unwrap();
    core.dispatch(CoreCommand::BulkSetDomainsEnabled {
        hostnames: vec!["a-cache.test".into(), "b-shop.test".into()],
        enabled: false,
    })
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let task = loop {
        let t = core
            .inner()
            .tasks
            .list()
            .into_iter()
            .find(|t| t.kind == "docker-site-power");
        match t {
            Some(t) if !t.state.is_running() => break t,
            _ if Instant::now() > deadline => panic!("the power task never finished"),
            _ => std::thread::sleep(Duration::from_millis(10)),
        }
    };
    let posts: Vec<String> = fake
        .seen
        .lock()
        .unwrap()
        .iter()
        .filter(|s| s.starts_with("POST"))
        .cloned()
        .collect();
    assert!(
        posts.contains(&"POST /containers/a1/stop".to_string()),
        "the second container is still stopped after the first failed: {posts:?}"
    );
    let error = task.error.expect("the task reports the failure");
    assert!(error.problem.contains("a-cache.test"), "{error:?}");
    assert!(
        task.problems
            .iter()
            .any(|p| p.contains("cannot stop container")),
        "Docker's own message is kept: {:?}",
        task.problems
    );
}

/// The compose editor writes only to stacks OLS knows: a project folder's compose file,
/// a running stack's own file (from Docker's labels), or a stack made or connected in OLS.
/// Any other path is refused before a byte is written, and Docker's check (which writes a
/// temporary copy beside the file) is skipped for it while every local rule still runs.
#[test]
fn compose_saves_are_limited_to_known_stacks() {
    use crate::command::{CoreCommand, CoreResponse};
    fn engine_with_a_stack(method: &str, path: &str) -> (u16, String) {
        match (method, path) {
            ("GET", p) if p.starts_with("/containers/json") => {
                let file = std::env::var("OLS_TEST_ENGINE_STACK_FILE").unwrap_or_default();
                let body = serde_json::json!([{
                    "Id": "s1", "Names": ["/engine-web-1"], "Image": "nginx", "State": "running",
                    "Status": "Up", "Created": 1, "Ports": [],
                    "Labels": {
                        "com.docker.compose.project": "engine",
                        "com.docker.compose.service": "web",
                        "com.docker.compose.project.config_files": file,
                    }
                }]);
                (200, body.to_string())
            }
            _ => engine(method, path),
        }
    }
    let fake = FakeEngine::start(engine_with_a_stack);
    let home = crate::test_support::isolated_home();
    let mut settings = crate::settings::SettingsService::load(&home.paths).unwrap();
    let Endpoint::Tcp(addr) = &fake.endpoint else {
        unreachable!()
    };
    settings
        .set(
            SETTING_ENDPOINT,
            serde_json::Value::String(format!("tcp://{addr}")),
        )
        .unwrap();
    let core = crate::Core::new(settings, home.paths.clone());
    let yaml = "services:\n  web:\n    image: nginx:1.28\n";
    let edited = "services:\n  web:\n    image: nginx:1.29\n";
    let make = |dir: &std::path::Path| {
        std::fs::create_dir_all(dir).unwrap();
        let f = dir.join("compose.yaml");
        std::fs::write(&f, yaml).unwrap();
        f
    };
    let save = |f: &std::path::Path| {
        core.dispatch(CoreCommand::SaveDockerComposeFile {
            path: f.display().to_string(),
            content: edited.into(),
        })
    };

    // A project folder's compose file: saved as before.
    let project_dir = home.paths.root().join("shop");
    let in_project = make(&project_dir);
    core.dispatch(CoreCommand::RegisterProject {
        path: project_dir.display().to_string(),
    })
    .unwrap();
    assert!(save(&in_project).is_ok());
    assert!(std::fs::read_to_string(&in_project)
        .unwrap()
        .contains("1.29"));

    // A running stack's own file, outside any project: saved.
    let engine_file = make(&home.paths.root().join("elsewhere").join("engine"));
    std::env::set_var(
        "OLS_TEST_ENGINE_STACK_FILE",
        engine_file.display().to_string(),
    );
    assert!(save(&engine_file).is_ok());
    std::env::remove_var("OLS_TEST_ENGINE_STACK_FILE");

    // A stack connected in OLS (the builder connects every stack it makes): saved.
    let connected = make(&home.paths.root().join("built"));
    core.inner().set_docker_connected(&connected, true).unwrap();
    assert!(save(&connected).is_ok());

    // Anything else: refused, file untouched, nothing created.
    let stranger = make(&home.paths.root().join("Startup"));
    let err = save(&stranger).unwrap_err();
    assert!(err.cause.contains("not a stack OLS knows"), "{err:?}");
    assert_eq!(std::fs::read_to_string(&stranger).unwrap(), yaml);
    let missing = home.paths.root().join("nowhere").join("compose.yaml");
    std::fs::create_dir_all(missing.parent().unwrap()).unwrap();
    assert!(save(&missing).is_err());
    assert!(!missing.exists());
    // A path that climbs out of a project folder is not that project's file.
    let climbing = project_dir.join("..").join("Startup").join("compose.yaml");
    assert!(save(&climbing).is_err());
    assert_eq!(std::fs::read_to_string(&stranger).unwrap(), yaml);

    // Checking an unknown file still runs the local rules, without Docker's temp copy.
    match core
        .dispatch(CoreCommand::ValidateDockerCompose {
            path: stranger.display().to_string(),
            content: "services: [".into(),
        })
        .unwrap()
    {
        CoreResponse::DockerComposeCheck { check } => {
            assert!(!check.ok && !check.docker_checked, "{check:?}")
        }
        other => panic!("unexpected {other:?}"),
    }
}

// ---- §167 live updates ----

/// An engine whose `/events` answers with two chunked lines, then keeps the socket open
/// until the client hangs up. Everything else is an empty container list.
fn events_engine() -> (String, Arc<AtomicUsize>) {
    use std::io::Read;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let streams = Arc::new(AtomicUsize::new(0));
    let counter = streams.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let counter = counter.clone();
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                let _ = reader.read_line(&mut line);
                let mut header = String::new();
                while reader.read_line(&mut header).is_ok() && header != "\r\n" {
                    header.clear();
                }
                if line.contains("/events") {
                    counter.fetch_add(1, Ordering::SeqCst);
                    let _ = write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n11\r\n{{\"status\":\"die\"}}\n\r\n12\r\n{{\"status\":\"start\"}}\n\r\n"
                    );
                    let mut sink = [0u8; 64];
                    // Held open until the client closes (or the test ends).
                    while matches!(reader.get_mut().read(&mut sink), Ok(n) if n > 0) {}
                } else {
                    let _ = write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n[]"
                    );
                }
            });
        }
    });
    (addr, streams)
}

fn core_for(addr: &str) -> (crate::Core, crate::test_support::IsolatedHome) {
    let home = crate::test_support::isolated_home();
    let mut settings = crate::settings::SettingsService::load(&home.paths).unwrap();
    settings
        .set(
            SETTING_ENDPOINT,
            serde_json::Value::String(format!("tcp://{addr}")),
        )
        .unwrap();
    (crate::Core::new(settings, home.paths.clone()), home)
}

fn wait_for(mut cond: impl FnMut() -> bool) -> bool {
    let end = std::time::Instant::now() + Duration::from_secs(3);
    while std::time::Instant::now() < end {
        if cond() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    cond()
}

#[test]
fn watch_ticks_then_stops_cleanly() {
    let (addr, streams) = events_engine();
    let (core, _home) = core_for(&addr);
    let docker = &core.inner().docker;
    let mut rx = docker.subscribe();
    assert!(!docker.watching());

    core.inner().docker_watch(true);
    assert!(docker.watching());
    let started = std::time::Instant::now();
    assert!(wait_for(|| rx.try_recv().is_ok()), "no tick from events");
    assert!(started.elapsed() < Duration::from_secs(2));

    core.inner().docker_watch(false);
    assert!(!docker.watching());
    assert!(wait_for(|| events::watch_threads(docker) == 0));

    // A second watch session works the same.
    core.inner().docker_watch(true);
    assert!(wait_for(|| rx.try_recv().is_ok()), "no tick after restart");
    assert!(streams.load(Ordering::SeqCst) >= 2);
    core.inner().docker_watch(false);
    assert!(wait_for(|| events::watch_threads(docker) == 0));
}

#[test]
fn watchers_are_ref_counted() {
    let (addr, _streams) = events_engine();
    let (core, _home) = core_for(&addr);
    let docker = &core.inner().docker;
    core.inner().docker_watch(true);
    core.inner().docker_watch(true);
    core.inner().docker_watch(false);
    assert!(docker.watching());
    assert_eq!(events::watch_threads(docker), 1);
    core.inner().docker_watch(false);
    assert!(!docker.watching());
    assert!(wait_for(|| events::watch_threads(docker) == 0));
    // An extra stop never underflows.
    core.inner().docker_watch(false);
    assert!(!docker.watching());
}

#[test]
fn startup_starts_no_events_thread() {
    let fake = FakeEngine::start(engine);
    let Endpoint::Tcp(addr) = &fake.endpoint else {
        unreachable!()
    };
    let (core, _home) = core_for(addr);
    let _ = core.dispatch(crate::command::CoreCommand::GetDashboard);
    assert!(!core.inner().docker.watching());
    assert_eq!(events::watch_threads(&core.inner().docker), 0);
    assert_eq!(fake.hits(), 0);
}
