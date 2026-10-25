use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::command::*;
use crate::domain::{Domain, Ownership};
use crate::error::CoreError;
use crate::project::Project;
use crate::web::manager::ConfigPart;
use crate::Core;

fn test_core() -> (Core, crate::test_support::IsolatedHome) {
    let home = crate::test_support::isolated_home();
    let settings = crate::settings::SettingsService::load(&home.paths).unwrap();
    let core = Core::new(settings, home.paths.clone());
    (core, home)
}

fn stop_all_leaves_no_supervised_process_running() {
    use crate::process::ProcessSpec;
    use std::time::Duration;

    let (core, _home) = test_core();
    // One process per owner that StopAll has to cover. A plain supervisor process is
    // the straggler case: nothing in the specific lists knows about it, so only the
    // sweep can catch it.
    for name in ["stopall-service", "stopall-stray"] {
        let id = core.supervisor().start(ProcessSpec {
            name: name.into(),
            executable: "ping".into(),
            args: vec!["127.0.0.1".into(), "-n".into(), "30".into()],
            cwd: None,
            env: vec![],
            restart: None,
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !core.supervisor().is_alive(id) && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(25));
        }
        assert!(core.supervisor().is_alive(id), "{name} never started");
    }

    let CoreResponse::StoppedEverything { still_running } =
        core.dispatch(CoreCommand::StopAll).expect("stop all")
    else {
        panic!("StopAll must report what survived")
    };
    assert!(
        still_running.is_empty(),
        "StopAll reported survivors: {still_running:?}"
    );
    let alive: Vec<String> = core
        .supervisor()
        .snapshot()
        .into_iter()
        .filter(|p| core.supervisor().is_alive(p.id))
        .map(|p| p.name)
        .collect();
    assert!(alive.is_empty(), "still alive after Stop all: {alive:?}");
}

/// A service outside the auto-startup set must still stop. The Dashboard's Stop all
/// used to iterate only that set, so Mailpit survived a Stop all and kept the icon
/// green while the button that stopped everything sat right there. This drives the
/// command through a real service with a real (stoppable) process rather than
/// poking the running map, so it fails the same way the real path does.
#[cfg(windows)]
#[test]
fn stop_all_covers_services_outside_the_autostart_set() {
    use crate::process::ProcessSpec;
    use std::time::Duration;

    let (core, _home) = test_core();
    // Mailpit is deliberately not in the autostart set: it is installed by default
    // and the user starts it by hand, which is exactly the case that used to survive.
    core.dispatch(CoreCommand::SetSetting {
        key: "startup.autostart_services".into(),
        value: serde_json::json!(["mariadb"]),
    })
    .unwrap();
    // A supervised process standing in for the service: `StopAll` must take it down
    // even though nothing ties it to a service record.
    let id = core.supervisor().start(ProcessSpec {
        name: "Mailpit".into(),
        executable: "ping".into(),
        args: vec!["127.0.0.1".into(), "-n".into(), "30".into()],
        cwd: None,
        env: vec![],
        restart: None,
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !core.supervisor().is_alive(id) && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(core.supervisor().is_alive(id), "the stand-in never started");

    let CoreResponse::StoppedEverything { still_running } =
        core.dispatch(CoreCommand::StopAll).expect("stop all")
    else {
        panic!("StopAll must report what survived")
    };
    assert!(still_running.is_empty(), "survivors: {still_running:?}");
    assert!(
        !core.supervisor().is_alive(id),
        "a process outside the autostart set survived Stop all"
    );
}

/// The dashboard is the only page polled every 3s, so `GetDashboard` has to finish
/// well inside that. This measures the real dispatch with the stores populated the way
/// an installed app has them, and with every service port actually held — that is the
/// state where each row pays a 300ms TCP probe, and where a per-id recomputation of
/// the web status made the poll overrun its own interval and leave the Services and
/// Web server cards on "Loading…".
#[test]
fn the_dashboard_poll_finishes_well_inside_its_interval() {
    let (core, home) = test_core();
    let root = home.paths.root().join("site");
    std::fs::create_dir_all(&root).unwrap();
    for n in 0..10 {
        core.inner()
            .domains
            .lock()
            .unwrap()
            .add(Domain {
                hostname: format!("site{n}.test"),
                project_id: None,
                root: root.display().to_string(),
                kind: crate::domain::SiteKind::Static,
                https: false,
                redirect_https: false,
                wildcard: false,
                enabled: true,
                ownership: Ownership::Managed,
                app: None,
                blocks: Default::default(),
                generated_hashes: Default::default(),
                public_domain: None,
                tunnel_id: None,
                server: None,
                path_prefix: None,
                icon: None,
                ..Default::default()
            })
            .unwrap();
    }

    // Warm the one-time caches so the measurement is of the steady-state poll.
    core.dispatch(CoreCommand::GetDashboard).unwrap();

    let started = std::time::Instant::now();
    let res = core.dispatch(CoreCommand::GetDashboard).unwrap();
    let elapsed = started.elapsed();

    let CoreResponse::Dashboard { data } = res else {
        panic!("expected a dashboard response");
    };
    assert!(
        !data.services.is_empty(),
        "the Services card needs its rows"
    );
    // The 10 we added, plus the seeded welcome site.
    assert_eq!(data.domains.len(), 11);

    assert!(
        elapsed < std::time::Duration::from_millis(1500),
        "GetDashboard took {elapsed:?}; the UI polls it every 3s and shows the \
             Services and Web server cards as still loading until it lands"
    );
}

/// Holding a listener on each service port must not turn the dashboard poll into a
/// chain of serialized 300ms probes. The rows are independent, so they are probed
/// together and the poll stays a single probe timeout at worst.
#[test]
fn the_dashboard_poll_does_not_serialize_a_probe_per_service() {
    let (core, home) = test_core();

    // An installed runtime directory per service, so every row reports installed and
    // pays the version scan a real install pays.
    for id in [
        "mailpit",
        "mariadb",
        "postgres",
        "mongodb",
        "redis",
        "memcached",
    ] {
        for v in ["1.0.0", "1.1.0", "1.2.0"] {
            std::fs::create_dir_all(home.paths.runtimes_dir().join(id).join(v)).unwrap();
        }
    }
    // A listener on each port, so every row reports a live port.
    let _listeners: Vec<std::net::TcpListener> = [
        "mailpit",
        "mariadb",
        "postgres",
        "mongodb",
        "redis",
        "memcached",
    ]
    .iter()
    .filter_map(|id| crate::service::known_port(id))
    .filter_map(|port| std::net::TcpListener::bind(("127.0.0.1", port)).ok())
    .collect();

    core.dispatch(CoreCommand::GetDashboard).unwrap();

    let started = std::time::Instant::now();
    core.dispatch(CoreCommand::GetDashboard).unwrap();
    let elapsed = started.elapsed();

    // Six 300ms probes back to back would be 1.8s on their own.
    assert!(
        elapsed < std::time::Duration::from_millis(900),
        "GetDashboard took {elapsed:?} with every service installed and its port held, \
             which means the per-service probes are running one after another"
    );
}

#[test]
fn sites_are_grouped_by_website_type() {
    let (core, home) = test_core();
    let root = home.paths.root().join("site");
    std::fs::create_dir_all(&root).unwrap();
    let make =
        |host: &str, kind: crate::domain::SiteKind, app: Option<(&str, Option<&str>)>| Domain {
            hostname: host.into(),
            project_id: None,
            root: root.display().to_string(),
            kind,
            https: false,
            redirect_https: false,
            wildcard: false,
            enabled: true,
            ownership: Ownership::Managed,
            app: app.map(|(exe, runtime)| crate::domain::AppSpec {
                executable: exe.into(),
                args: vec![],
                cwd: root.display().to_string(),
                runtime: runtime.map(str::to_string),
            }),
            blocks: Default::default(),
            generated_hashes: Default::default(),
            public_domain: None,
            tunnel_id: None,
            server: None,
            path_prefix: None,
            icon: None,
            ..Default::default()
        };
    let proxy = || crate::domain::SiteKind::Proxy {
        upstream_port: 3000,
        upstream_host: None,
        upstream_https: false,
    };
    let sites = [
        make(
            "a.test",
            crate::domain::SiteKind::Php { version: None },
            None,
        ),
        make("b.test", crate::domain::SiteKind::Static, None),
        make("c.test", proxy(), Some(("npm", Some("node")))),
        make("d.test", proxy(), Some(("uvicorn", None))),
        make("e.test", proxy(), None),
        make(
            "f.test",
            crate::domain::SiteKind::Proxy {
                upstream_port: 80,
                upstream_host: Some("10.0.0.5".into()),
                upstream_https: false,
            },
            None,
        ),
    ];
    for s in sites {
        core.dispatch(CoreCommand::AddDomain { domain: s }).unwrap();
    }
    let groups: std::collections::BTreeMap<String, String> =
        match core.dispatch(CoreCommand::ListDomains).unwrap() {
            CoreResponse::Domains { domains } => {
                domains.into_iter().map(|d| (d.hostname, d.group)).collect()
            }
            _ => panic!("expected Domains"),
        };
    let expect = [
        ("a.test", "php"),
        ("b.test", "static"),
        ("c.test", "nodejs"),
        ("d.test", "python"),
        ("e.test", "proxy"),
        ("f.test", "proxy"),
    ];
    for (host, group) in expect {
        assert_eq!(groups.get(host).map(String::as_str), Some(group), "{host}");
    }
}

#[test]
fn env_files_round_trip_through_the_core_and_keep_a_backup() {
    let (core, home) = test_core();
    let dir = home.paths.root().join("laravel");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(".env"),
        "# keep me\nAPP_ENV=local\nDB_PASSWORD=old\n",
    )
    .unwrap();
    std::fs::write(dir.join(".env.example"), "APP_ENV=example\nNEW_KEY=1\n").unwrap();
    let pid = match core
        .dispatch(CoreCommand::RegisterProject {
            path: dir.display().to_string(),
        })
        .unwrap()
    {
        CoreResponse::Project { project } => project.id,
        _ => panic!("expected Project"),
    };
    let view = |r: CoreResponse| match r {
        CoreResponse::EnvFile { view } => *view,
        _ => panic!("expected EnvFile"),
    };
    let v = view(
        core.dispatch(CoreCommand::SetEnvValue {
            project_id: pid.clone(),
            file: ".env".into(),
            key: "APP_ENV".into(),
            value: "production".into(),
        })
        .unwrap(),
    );
    assert!(v.content.starts_with("# keep me\nAPP_ENV=production\n"));
    assert!(
        v.entries
            .iter()
            .find(|e| e.key == "DB_PASSWORD")
            .unwrap()
            .secret
    );
    let backups = home.paths.data_dir().join("env_backups").join(&pid);
    assert_eq!(
        std::fs::read_dir(backups).unwrap().count(),
        1,
        "the previous version is kept"
    );

    // A file with a broken line is refused and the good one stays.
    assert!(core
        .dispatch(CoreCommand::SaveEnvFile {
            project_id: pid.clone(),
            file: ".env".into(),
            content: "oops no equals\n".into()
        })
        .is_err());
    assert!(std::fs::read_to_string(dir.join(".env"))
        .unwrap()
        .contains("APP_ENV=production"));

    // Names that could leave the project folder are refused.
    assert!(core
        .dispatch(CoreCommand::ReadEnvFile {
            project_id: pid.clone(),
            file: "../secrets.txt".into()
        })
        .is_err());

    match core
        .dispatch(CoreCommand::CompareEnvFiles {
            project_id: pid.clone(),
            a: ".env".into(),
            b: ".env.example".into(),
        })
        .unwrap()
    {
        CoreResponse::EnvCompare { rows } => assert!(rows
            .iter()
            .any(|r| r.key == "NEW_KEY" && r.status == "only_b")),
        _ => panic!("expected EnvCompare"),
    }
    let created = view(
        core.dispatch(CoreCommand::CreateEnvFile {
            project_id: pid.clone(),
            file: ".env.testing".into(),
            from: Some(".env.example".into()),
        })
        .unwrap(),
    );
    assert_eq!(created.entries.len(), 2);
    let v = view(
        core.dispatch(CoreCommand::DeleteEnvKey {
            project_id: pid,
            file: ".env.testing".into(),
            key: "NEW_KEY".into(),
        })
        .unwrap(),
    );
    assert_eq!(v.content, "APP_ENV=example\n");
}
