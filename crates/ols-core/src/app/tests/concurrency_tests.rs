use crate::app::*;
use crate::domain::{Domain, SiteKind};
use std::sync::Arc;

mod apply_concurrency_tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    fn inner() -> (Arc<Inner>, crate::test_support::IsolatedHome) {
        let home = crate::test_support::isolated_home();
        let settings = crate::settings::SettingsService::load(&home.paths).unwrap();
        (Inner::new(settings, home.paths.clone()).unwrap(), home)
    }

    /// A PHP site bound to a project, so applying it resolves the project's runtime —
    /// which reads the project store, and is therefore where a test can park an apply.
    fn php_site_on_a_project(inner: &Inner, home: &crate::test_support::IsolatedHome) {
        let dir = home.paths.data_dir().join("www").join("app");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.php"), "<?php echo 1;").unwrap();
        let project = inner
            .projects
            .lock()
            .unwrap()
            .register(&dir.display().to_string())
            .unwrap();
        let mut domains = inner.domains.lock().unwrap();
        let mut site = domains
            .get(crate::domain::HOME_HOSTNAME)
            .expect("the built-in home site is seeded");
        site.hostname = "app.test".into();
        site.project_id = Some(project.id.clone());
        site.root = dir.display().to_string();
        site.kind = crate::domain::SiteKind::Php { version: None };
        domains.add(site).unwrap();
    }

    /// Deleting a site takes it off the list, and the project behind it goes with it once
    /// it has no sites left — otherwise the row the user just deleted reappears as
    /// "project only". Nothing on disk is touched.
    #[test]
    fn deleting_a_site_takes_its_last_project_off_the_list() {
        let (inner, home) = inner();
        let dir = home.paths.data_dir().join("www").join("app");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.php"), "<?php echo 1;").unwrap();
        let project = inner
            .projects
            .lock()
            .unwrap()
            .register(&dir.display().to_string())
            .unwrap();
        let site = |host: &str| Domain {
            hostname: host.into(),
            project_id: Some(project.id.clone()),
            root: dir.display().to_string(),
            kind: crate::domain::SiteKind::Php { version: None },
            https: false,
            redirect_https: false,
            wildcard: false,
            enabled: true,
            ownership: crate::domain::Ownership::Managed,
            app: None,
            blocks: crate::domain::SiteBlocks::default(),
            generated_hashes: Default::default(),
            public_domain: None,
            tunnel_id: None,
            server: None,
            path_prefix: None,
            icon: None,
            ..Default::default()
        };
        {
            let mut domains = inner.domains.lock().unwrap();
            domains.add(site("one.test")).unwrap();
            domains.add(site("two.test")).unwrap();
        }

        // One of two sites left: the project stays.
        inner.remove_domain("one.test").unwrap();
        assert!(inner.projects.lock().unwrap().get(&project.id).is_some());
        assert!(inner.domains.lock().unwrap().get("two.test").is_some());

        // The last one: both the site and the project leave the list.
        inner.remove_domain("two.test").unwrap();
        assert!(inner.projects.lock().unwrap().get(&project.id).is_none());
        assert!(inner.domains.lock().unwrap().get("two.test").is_none());
        assert!(
            dir.exists(),
            "deleting a site must not touch the project folder"
        );
    }

    /// A running apply renders files, runs each server's validator and restarts it — seconds of
    /// work. It must not hold the domain store for that time: `list_domains` runs on a UI
    /// poll, and a page waiting seconds on every status refresh reads as a hung app.
    #[test]
    fn a_running_apply_leaves_the_domain_store_readable() {
        let (inner, home) = inner();
        php_site_on_a_project(&inner, &home);

        // Hold the project store so the apply parks in `project_detail`, mid-apply.
        let projects = inner.projects.lock().unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let worker = {
            let inner = Arc::clone(&inner);
            std::thread::spawn(move || {
                started_tx.send(()).unwrap();
                let result = inner.apply_web(&[]);
                done_tx.send(result.is_ok()).unwrap();
            })
        };
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        // Give the apply time to get past its snapshot and park on the project store,
        // so the probe below cannot win the race by arriving too early.
        std::thread::sleep(Duration::from_millis(500));

        // The apply cannot finish while we hold the project store, so anything it is
        // holding right now is held for the whole apply, not for a moment.
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut free = false;
        while Instant::now() < deadline {
            if let Ok(guard) = inner.domains.try_lock() {
                drop(guard);
                free = true;
                break;
            }
            assert!(
                done_rx.try_recv().is_err(),
                "the apply finished, so it never really blocked"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        drop(projects);
        worker.join().unwrap();
        assert!(free, "the domain store stayed locked for the whole apply");
    }

    /// An apply no longer writes generated-config hashes itself — it hands them back and
    /// the store is updated afterwards, without the apply still holding it. The hashes
    /// have to land all the same, keyed per server, and a hostname the store no longer
    /// knows is skipped rather than resurrected.
    #[test]
    fn generated_hashes_are_committed_after_the_apply() {
        let (inner, _home) = inner();
        {
            let mut domains = inner.domains.lock().unwrap();
            let mut site = domains.get(crate::domain::HOME_HOSTNAME).unwrap();
            site.hostname = "static.test".into();
            domains.add(site).unwrap();
        }
        inner
            .record_generated_hashes(&[
                (
                    "nginx".to_string(),
                    "static.test".to_string(),
                    "aaa".to_string(),
                ),
                (
                    "apache".to_string(),
                    "static.test".to_string(),
                    "bbb".to_string(),
                ),
                (
                    "nginx".to_string(),
                    "gone.test".to_string(),
                    "ccc".to_string(),
                ),
            ])
            .unwrap();

        let hashes = inner
            .domains
            .lock()
            .unwrap()
            .get("static.test")
            .unwrap()
            .generated_hashes
            .clone();
        assert_eq!(hashes.get("nginx").map(String::as_str), Some("aaa"));
        assert_eq!(hashes.get("apache").map(String::as_str), Some("bbb"));
        assert!(inner.domains.lock().unwrap().get("gone.test").is_none());
    }

    /// Nothing to commit is the common case for a no-op apply, and must not take the
    /// store lock at all.
    #[test]
    fn committing_no_hashes_does_not_touch_the_store() {
        let (inner, _home) = inner();
        let guard = inner.domains.try_lock().expect("the store is free");
        inner.record_generated_hashes(&[]).unwrap();
        drop(guard);
    }
}
