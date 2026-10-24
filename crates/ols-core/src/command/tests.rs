use std::collections::BTreeMap;
use serde_json::Value;

use super::types::*;
use super::*;
use crate::domain::{Domain, Ownership};
use crate::project::Project;
use crate::settings::SettingsService;
use crate::web::manager::ConfigPart;

    /// Keeps the isolated-home guard alive for the test's duration (it unsets `OLS_HOME` on drop).
    fn test_core() -> (Core, crate::test_support::IsolatedHome) {
        let home = crate::test_support::isolated_home();
        let settings = SettingsService::load(&home.paths).unwrap();
        let core = Core::new(settings, home.paths.clone());
        (core, home)
    }

    /// `StopAll` has to leave nothing running, whatever the owner. The status mark is
    /// red only when no supervised process is alive, so anything this misses shows the
    /// user a green icon immediately after they asked for everything to stop — which is
    /// exactly the bug: the tray's Stop all never covered tunnels and workers, and the
    /// Dashboard's never covered anything outside the auto-startup set.
    #[cfg(windows)]
    #[test]
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

    #[test]
    fn diagnostics_report_a_missing_site_folder_and_remember_ignores() {
        let (core, home) = test_core();
        let gone = home.paths.root().join("gone");
        let domain = Domain {
            hostname: "ghost.test".into(),
            project_id: None,
            root: gone.display().to_string(),
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
        };
        // Written straight to the store: AddDomain would reject a folder that doesn't exist.
        core.inner().domains.lock().unwrap().add(domain).ok();
        let findings = |core: &Core| match core.dispatch(CoreCommand::RunDiagnostics).unwrap() {
            CoreResponse::Diagnostics { findings } => findings,
            _ => panic!("expected Diagnostics"),
        };
        let found = findings(&core);
        let f = found
            .iter()
            .find(|f| f.id == "site_root_missing:ghost.test")
            .expect("missing-folder finding");
        assert!(!f.ignored && !f.problem.is_empty() && !f.cause.is_empty() && !f.fix.is_empty());

        core.dispatch(CoreCommand::IgnoreDiagnostic {
            id: f.id.clone(),
            ignore: true,
        })
        .unwrap();
        assert!(
            findings(&core)
                .iter()
                .find(|x| x.id == f.id)
                .unwrap()
                .ignored
        );
        core.dispatch(CoreCommand::IgnoreDiagnostic {
            id: f.id.clone(),
            ignore: false,
        })
        .unwrap();
        assert!(
            !findings(&core)
                .iter()
                .find(|x| x.id == f.id)
                .unwrap()
                .ignored
        );
    }

    #[test]
    fn composer_and_venv_info_come_from_the_project_files() {
        let (core, home) = test_core();
        let dir = home.paths.root().join("app");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("composer.json"),
            r#"{"require":{"monolog/monolog":"^3.0"}}"#,
        )
        .unwrap();
        let project = match core
            .dispatch(CoreCommand::RegisterProject {
                path: dir.display().to_string(),
            })
            .unwrap()
        {
            CoreResponse::Project { project } => project,
            _ => panic!("expected Project"),
        };
        match core
            .dispatch(CoreCommand::GetComposerInfo {
                project_id: project.id.clone(),
            })
            .unwrap()
        {
            CoreResponse::Composer { info } => assert_eq!(info.packages.len(), 1),
            _ => panic!("expected Composer"),
        }
        match core
            .dispatch(CoreCommand::GetVenv {
                project_id: project.id.clone(),
            })
            .unwrap()
        {
            CoreResponse::Venv { info } => assert!(!info.exists),
            _ => panic!("expected Venv"),
        }
        assert!(core
            .dispatch(CoreCommand::RunComposer {
                project_id: project.id,
                action: "require".into(),
                target: Some("--evil".into())
            })
            .is_err());
    }

    #[test]
    fn discovered_commands_include_package_scripts_with_the_projects_manager() {
        let (core, home) = test_core();
        let dir = home.paths.root().join("web");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("package.json"),
            r#"{"scripts":{"dev":"vite","build":"vite build"}}"#,
        )
        .unwrap();
        std::fs::write(dir.join("pnpm-lock.yaml"), "").unwrap();
        let project = match core
            .dispatch(CoreCommand::RegisterProject {
                path: dir.display().to_string(),
            })
            .unwrap()
        {
            CoreResponse::Project { project } => project,
            _ => panic!("expected Project"),
        };
        match core
            .dispatch(CoreCommand::DiscoverCommands {
                project_id: project.id,
            })
            .unwrap()
        {
            CoreResponse::CommandSources { sources } => {
                assert_eq!(sources.len(), 1, "no artisan / composer / manage.py here");
                assert_eq!(sources[0].prefix, ["pnpm", "run"]);
                assert_eq!(sources[0].commands.len(), 2);
            }
            _ => panic!("expected CommandSources"),
        }
    }

    #[test]
    fn renaming_a_domain_keeps_its_settings_and_refuses_taken_names() {
        let (core, home) = test_core();
        let root = home.paths.root().join("site");
        std::fs::create_dir_all(&root).unwrap();
        let domain = |host: &str| Domain {
            hostname: host.into(),
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
        };
        core.dispatch(CoreCommand::AddDomain {
            domain: domain("old.test"),
        })
        .unwrap();
        core.dispatch(CoreCommand::AddDomain {
            domain: domain("other.test"),
        })
        .unwrap();

        let renamed = core
            .dispatch(CoreCommand::RenameDomain {
                hostname: "old.test".into(),
                new_hostname: "My-Shop.Local".into(),
            })
            .unwrap();
        assert!(
            matches!(renamed, CoreResponse::Domain { domain } if domain.hostname == "my-shop.local" && domain.root == root.display().to_string())
        );
        assert!(core
            .dispatch(CoreCommand::RenameDomain {
                hostname: "my-shop.local".into(),
                new_hostname: "other.test".into()
            })
            .is_err());
        assert!(
            matches!(
                core.dispatch(CoreCommand::GetDomain {
                    hostname: "my-shop.local".into()
                }),
                Ok(CoreResponse::Domain { .. })
            ),
            "a failed rename leaves the site as it was"
        );
        assert!(
            core.dispatch(CoreCommand::RenameDomain {
                hostname: "openlocalserver.test".into(),
                new_hostname: "gone.test".into()
            })
            .is_err(),
            "the built-in home site can't be renamed away"
        );
        assert!(
            core.dispatch(CoreCommand::RemoveDomain {
                hostname: "openlocalserver.test".into()
            })
            .is_err(),
            "the built-in home site can't be deleted"
        );
    }

    /// Laragon-style: scanning a folder gives each servable project `<name>.test`, a
    /// deleted automatic domain stays deleted, and new folders are picked up on the next sync.
    #[test]
    fn scanned_projects_get_automatic_domains() {
        let (core, home) = test_core();
        let www = home.paths.root().join("www");
        for (dir, file) in [
            ("Shop", "index.php"),
            ("site", "index.html"),
            ("api", "package.json"),
        ] {
            std::fs::create_dir_all(www.join(dir)).unwrap();
            std::fs::write(www.join(dir).join(file), "x").unwrap();
        }
        core.dispatch(CoreCommand::ScanAndRegisterProjects {
            path: www.display().to_string(),
        })
        .unwrap();

        let kinds = |core: &Core| -> Vec<(String, String)> {
            match core.dispatch(CoreCommand::ListDomains).unwrap() {
                CoreResponse::Domains { domains } => {
                    domains.into_iter().map(|d| (d.hostname, d.kind)).collect()
                }
                _ => panic!("expected Domains"),
            }
        };
        let mut found = kinds(&core);
        found.sort();
        assert_eq!(
            found,
            vec![
                ("openlocalserver.test".to_string(), "static".to_string()),
                ("shop.local".to_string(), "php".to_string()),
                ("site.local".to_string(), "static".to_string())
            ],
            "a Node project needs a dev server, so no automatic domain"
        );

        core.dispatch(CoreCommand::RemoveDomain {
            hostname: "shop.local".into(),
        })
        .unwrap();
        std::fs::create_dir_all(www.join("blog")).unwrap();
        std::fs::write(www.join("blog").join("index.php"), "x").unwrap();
        core.dispatch(CoreCommand::SyncAutoDomains).unwrap();
        let hosts: Vec<String> = kinds(&core).into_iter().map(|(h, _)| h).collect();
        assert!(
            hosts.contains(&"blog.local".to_string()),
            "a new folder is picked up"
        );
        assert!(
            !hosts.contains(&"shop.local".to_string()),
            "a deleted automatic domain is not recreated"
        );
    }

    /// The round trip the Settings history exists for: delete a project, see it listed,
    /// drop it from the history, and a scan registers the folder again.
    #[test]
    fn a_deleted_project_can_be_taken_back_out_of_the_history() {
        let (core, home) = test_core();
        let www = home.paths.root().join("www");
        std::fs::create_dir_all(www.join("Shop")).unwrap();
        std::fs::write(www.join("Shop").join("index.php"), "x").unwrap();
        core.dispatch(CoreCommand::ScanAndRegisterProjects {
            path: www.display().to_string(),
        })
        .unwrap();

        let projects = |core: &Core| -> Vec<Project> {
            match core.dispatch(CoreCommand::ListProjects).unwrap() {
                CoreResponse::Projects { projects } => projects,
                _ => panic!("expected Projects"),
            }
        };
        let shop = projects(&core)
            .into_iter()
            .find(|p| p.name == "Shop")
            .expect("the scanned folder is registered");
        core.dispatch(CoreCommand::RemoveProject { id: shop.id })
            .unwrap();

        let items = |core: &Core| -> Vec<crate::app::DeletedItem> {
            match core.dispatch(CoreCommand::ListDeletedItems).unwrap() {
                CoreResponse::DeletedItems { items } => items,
                _ => panic!("expected DeletedItems"),
            }
        };
        let deleted = items(&core);
        assert_eq!(deleted.len(), 1, "the removed folder is on the list");
        assert_eq!(deleted[0].value, shop.path);
        assert_eq!(deleted[0].kind, "project");

        // Scanning again must not bring it back while the history still names it.
        core.dispatch(CoreCommand::ScanAndRegisterProjects {
            path: www.display().to_string(),
        })
        .unwrap();
        assert!(
            projects(&core).is_empty(),
            "a deleted folder was re-registered"
        );

        // Clearing that one entry is what unblocks it.
        match core
            .dispatch(CoreCommand::ForgetDeletedItem {
                value: shop.path.clone(),
            })
            .unwrap()
        {
            CoreResponse::DeletedItems { items } => assert!(items.is_empty()),
            _ => panic!("expected DeletedItems"),
        }
        core.dispatch(CoreCommand::ScanAndRegisterProjects {
            path: www.display().to_string(),
        })
        .unwrap();
        assert_eq!(projects(&core).len(), 1, "the folder came back");
    }

    #[test]
    fn clearing_the_history_returns_the_deleted_items_to_a_scan() {
        let (core, home) = test_core();
        let www = home.paths.root().join("www");
        for dir in ["Shop", "Blog"] {
            std::fs::create_dir_all(www.join(dir)).unwrap();
            std::fs::write(www.join(dir).join("index.php"), "x").unwrap();
        }
        core.dispatch(CoreCommand::ScanAndRegisterProjects {
            path: www.display().to_string(),
        })
        .unwrap();
        let ids: Vec<String> = match core.dispatch(CoreCommand::ListProjects).unwrap() {
            CoreResponse::Projects { projects } => projects.into_iter().map(|p| p.id).collect(),
            _ => panic!("expected Projects"),
        };
        for id in ids {
            core.dispatch(CoreCommand::RemoveProject { id }).unwrap();
        }

        match core.dispatch(CoreCommand::ClearDeletedItems).unwrap() {
            CoreResponse::DeletedItems { items } => assert!(items.is_empty()),
            _ => panic!("expected DeletedItems"),
        }
        core.dispatch(CoreCommand::ScanAndRegisterProjects {
            path: www.display().to_string(),
        })
        .unwrap();
        match core.dispatch(CoreCommand::ListProjects).unwrap() {
            CoreResponse::Projects { projects } => assert_eq!(projects.len(), 2),
            _ => panic!("expected Projects"),
        }
    }

    #[test]
    fn forgetting_nothing_says_so_instead_of_doing_nothing_quietly() {
        let (core, _home) = test_core();
        let err = core
            .dispatch(CoreCommand::ForgetDeletedItem {
                value: "   ".into(),
            })
            .unwrap_err();
        assert!(
            err.problem.contains("could not be cleared") && err.cause.contains("deletion history"),
            "an empty value must be reported, not ignored: {err:?}"
        );
    }

    #[test]
    fn ping_returns_pong_with_version() {
        let (core, _home) = test_core();
        match core.dispatch(CoreCommand::Ping).unwrap() {
            CoreResponse::Pong { version } => assert!(!version.is_empty()),
            _ => panic!("expected Pong"),
        }
    }

    #[test]
    fn set_then_get_setting_round_trips_through_dispatch() {
        let (core, _home) = test_core();
        core.dispatch(CoreCommand::SetSetting {
            key: "editor".into(),
            value: Value::String("vscode".into()),
        })
        .unwrap();
        match core
            .dispatch(CoreCommand::GetSetting {
                key: "editor".into(),
            })
            .unwrap()
        {
            CoreResponse::Setting { value, .. } => {
                assert_eq!(value, Some(Value::String("vscode".into())))
            }
            _ => panic!("expected Setting"),
        }
    }

    #[test]
    fn get_missing_setting_returns_none_not_error() {
        let (core, _home) = test_core();
        match core
            .dispatch(CoreCommand::GetSetting {
                key: "does-not-exist".into(),
            })
            .unwrap()
        {
            CoreResponse::Setting { value, .. } => assert_eq!(value, None),
            _ => panic!("expected Setting"),
        }
    }

    #[test]
    fn required_ignored_folder_is_default_and_cannot_be_removed_or_renamed() {
        let (core, _home) = test_core();
        let get_ignored = || {
            let CoreResponse::Setting { value, .. } = core
                .dispatch(CoreCommand::GetSetting {
                    key: crate::app::PROJECT_IGNORED_FOLDERS.into(),
                })
                .unwrap()
            else {
                panic!("expected a setting response");
            };
            value.unwrap()
        };

        assert_eq!(get_ignored(), serde_json::json!(["ols_excluded_sites"]));

        core.dispatch(CoreCommand::SetSetting {
            key: crate::app::PROJECT_IGNORED_FOLDERS.into(),
            value: serde_json::json!(["ols_excluded_sites", "archive"]),
        })
        .unwrap();

        assert!(core
            .dispatch(CoreCommand::SetSetting {
                key: crate::app::PROJECT_IGNORED_FOLDERS.into(),
                value: serde_json::json!(["archive"]),
            })
            .is_err());
        assert!(core
            .dispatch(CoreCommand::RenameIgnoredFolder {
                old_name: "ols_excluded_sites".into(),
                new_name: "archive".into(),
            })
            .is_err());
        assert_eq!(
            get_ignored(),
            serde_json::json!(["ols_excluded_sites", "archive"])
        );
    }

    #[test]
    fn every_command_variant_round_trips_through_json_with_its_snake_case_tag() {
        // The UI builds these as JSON — a renamed field would silently break it.
        let cmd = CoreCommand::StartQuickApp {
            id: "laravel".into(),
            values: BTreeMap::from([("project_name".to_string(), "shop".to_string())]),
            approval: Some("once".into()),
            allow_elevated: false,
        };
        let json = serde_json::to_value(&cmd).unwrap();
        assert_eq!(json["type"], "start_quick_app");
        assert_eq!(json["values"]["project_name"], "shop");
        let back: CoreCommand = serde_json::from_value(json).unwrap();
        assert!(matches!(back, CoreCommand::StartQuickApp { .. }));

        let json = serde_json::json!({"type": "read_web_config", "hostname": null, "part": "main"});
        assert!(matches!(
            serde_json::from_value::<CoreCommand>(json).unwrap(),
            CoreCommand::ReadWebConfig {
                part: ConfigPart::Main,
                ..
            }
        ));
    }

    /// The default projects folder is the one setting that points inside the app, so it is
    /// stored relative. The UI must still only ever see a real path: it renders this value
    /// in a text input and offers it as the parent folder for a new project.
    #[test]
    fn a_projects_dir_inside_the_app_is_stored_relative_and_answered_absolute() {
        let (core, home) = test_core();
        let wanted = home.paths.sites_dir().join("work");
        std::fs::create_dir_all(&wanted).unwrap();

        core.dispatch(CoreCommand::SetSetting {
            key: "quickapps.projects_dir".into(),
            value: serde_json::Value::String(wanted.display().to_string()),
        })
        .unwrap();

        // On disk, relative. Read past the service, since the command layer decodes it.
        let stored = crate::db::load_settings(&home.paths)
            .unwrap()
            .get("quickapps.projects_dir")
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default();
        assert_eq!(stored, "sites/work");

        // Through the command the UI uses, absolute.
        let CoreResponse::Setting { value, .. } = core
            .dispatch(CoreCommand::GetSetting {
                key: "quickapps.projects_dir".into(),
            })
            .unwrap()
        else {
            panic!("expected a setting response");
        };
        assert_eq!(
            value.and_then(|v| v.as_str().map(str::to_string)),
            Some(wanted.display().to_string())
        );
    }

    /// A blank value must keep meaning "unset, use the sites folder" — it is the default,
    /// and the field is a text input the user clears.
    #[test]
    fn a_blank_projects_dir_still_falls_back_to_the_sites_folder() {
        let (core, home) = test_core();
        core.dispatch(CoreCommand::SetSetting {
            key: "quickapps.projects_dir".into(),
            value: serde_json::Value::String("   ".into()),
        })
        .unwrap();
        let CoreResponse::Setting { value, .. } = core
            .dispatch(CoreCommand::GetSetting {
                key: "quickapps.projects_dir".into(),
            })
            .unwrap()
        else {
            panic!("expected a setting response");
        };
        assert_eq!(
            value.and_then(|v| v.as_str().map(str::to_string)),
            Some(home.paths.sites_dir().display().to_string())
        );
    }

    #[test]
    fn domains_reject_injection_in_structured_blocks_and_relative_roots() {
        use crate::domain::{HeaderRule, SiteBlocks, SiteKind};
        let (core, home) = test_core();
        let root = home.paths.root().join("site").display().to_string();
        let mut domain = Domain {
            hostname: "shop.test".into(),
            project_id: None,
            root: root.clone(),
            kind: SiteKind::Static,
            https: false,
            redirect_https: false,
            wildcard: false,
            enabled: true,
            ownership: Ownership::Managed,
            app: None,
            blocks: SiteBlocks::default(),
            generated_hashes: Default::default(),
            public_domain: None,
            tunnel_id: None,
            server: None,
            path_prefix: None,
            icon: None,
            ..Default::default()
        };
        domain.blocks.headers.push(HeaderRule {
            name: "X-Test".into(),
            value: "a\"; } server { listen 1; ".into(),
        });
        assert!(
            core.dispatch(CoreCommand::AddDomain {
                domain: domain.clone()
            })
            .is_err(),
            "a header value must not break out of its directive"
        );

        domain.blocks.headers.clear();
        domain.root = "relative/path".into();
        assert!(core
            .dispatch(CoreCommand::AddDomain {
                domain: domain.clone()
            })
            .is_err());

        domain.root = root;
        assert!(core.dispatch(CoreCommand::AddDomain { domain }).is_ok());
        match core.dispatch(CoreCommand::ListDomains).unwrap() {
            CoreResponse::Domains { domains } => {
                // The seeded openlocalserver.test plus the added shop.test.
                assert_eq!(domains.len(), 2);
                let shop = domains.iter().find(|d| d.hostname == "shop.test").unwrap();
                assert_eq!(shop.url, "http://shop.test/");
            }
            _ => panic!(),
        }
    }

    #[test]
    fn untrusted_quick_apps_refuse_to_run_without_approval() {
        let (core, home) = test_core();
        let file = home.paths.root().join("x.yaml");
        std::fs::write(&file, "id: theirs\nname: Theirs\nvariables:\n  - { name: project_name, required: true }\ncommands:\n  - echo hi\n").unwrap();
        core.dispatch(CoreCommand::ImportQuickApp {
            source: file.display().to_string(),
        })
        .unwrap();

        let values = BTreeMap::from([("project_name".to_string(), "demo".to_string())]);
        let err = core
            .dispatch(CoreCommand::StartQuickApp {
                id: "theirs".into(),
                values,
                approval: None,
                allow_elevated: false,
            })
            .unwrap_err();
        assert!(err.cause.contains("untrusted"), "{err:?}");
    }

    /// The Reverse Proxy Quick App points a domain at something running elsewhere.
    #[test]
    fn reverse_proxy_quick_app_plans_a_domain_to_another_host() {
        let (core, _home) = test_core();
        let values = BTreeMap::from([
            ("domain".to_string(), "portainer.test".to_string()),
            ("target_host".to_string(), "192.168.1.20".to_string()),
            ("target_port".to_string(), "9443".to_string()),
            ("target_https".to_string(), "true".to_string()),
        ]);
        let CoreResponse::QuickPlan { result } = core
            .dispatch(CoreCommand::PlanQuickApp {
                id: "reverse-proxy".into(),
                values,
            })
            .unwrap()
        else {
            panic!()
        };
        assert!(result.ok, "{:?}", result.errors);
        let text = serde_json::to_string(&result).unwrap();
        for want in ["create_domain", "portainer.test", "192.168.1.20", "9443"] {
            assert!(text.contains(want), "plan is missing {want}: {text}");
        }
    }

    #[test]
    fn plan_quick_app_reports_field_errors_instead_of_failing() {
        let (core, _home) = test_core();
        match core
            .dispatch(CoreCommand::PlanQuickApp {
                id: "laravel".into(),
                values: BTreeMap::new(),
            })
            .unwrap()
        {
            CoreResponse::QuickPlan { result } => {
                assert!(!result.ok);
                assert!(result.errors.iter().any(|e| e.field == "project_name"));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn delete_site_moves_folder_to_ols_excluded_sites() {
        let (core, _home) = test_core();
        let tmp = tempfile::tempdir().unwrap();
        let project_dir = tmp.path().join("my-site");
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::write(project_dir.join("index.html"), "<h1>Hello</h1>").unwrap();

        let project = match core
            .dispatch(CoreCommand::RegisterProject {
                path: project_dir.display().to_string(),
            })
            .unwrap()
        {
            CoreResponse::Project { project } => project,
            _ => panic!("expected Project"),
        };

        let hostname = "test-site.local".to_string();
        core.dispatch(CoreCommand::AddDomain {
            domain: crate::domain::Domain {
                hostname: hostname.clone(),
                project_id: Some(project.id.clone()),
                root: project_dir.display().to_string(),
                kind: crate::domain::SiteKind::Static,
                https: false,
                redirect_https: false,
                wildcard: false,
                enabled: true,
                ownership: Default::default(),
                app: None,
                blocks: Default::default(),
                generated_hashes: Default::default(),
                public_domain: None,
                tunnel_id: None,
                server: None,
                path_prefix: None,
                icon: None,
                ..Default::default()
            },
        })
        .unwrap();

        assert!(project_dir.exists());
        core.dispatch(CoreCommand::DeleteSite {
            hostname: hostname.clone(),
        })
        .unwrap();

        // Project folder was moved into ols_excluded_sites in the same directory
        assert!(!project_dir.exists());
        let excluded_dir = tmp.path().join("ols_excluded_sites").join("my-site");
        assert!(excluded_dir.exists());
        assert!(excluded_dir.join("index.html").exists());
    }
