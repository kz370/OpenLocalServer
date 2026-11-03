use crate::app::*;
use crate::domain::{Domain, SiteKind};

fn inner() -> (std::sync::Arc<Inner>, crate::test_support::IsolatedHome) {
    let home = crate::test_support::isolated_home();
    let settings = crate::settings::SettingsService::load(&home.paths).unwrap();
    (Inner::new(settings, home.paths.clone()).unwrap(), home)
}

fn reopen(home: &crate::test_support::IsolatedHome) -> std::sync::Arc<Inner> {
    let settings = crate::settings::SettingsService::load(&home.paths).unwrap();
    Inner::new(settings, home.paths.clone()).unwrap()
}

fn add_site(inner: &Inner, path: &std::path::Path, hostname: &str) -> String {
    std::fs::create_dir_all(path).unwrap();
    std::fs::write(path.join("index.html"), "site").unwrap();
    let project = inner
        .projects
        .lock()
        .unwrap()
        .register(&path.display().to_string())
        .unwrap();
    inner
        .domains
        .lock()
        .unwrap()
        .add(Domain {
            hostname: hostname.into(),
            project_id: Some(project.id.clone()),
            root: path.display().to_string(),
            kind: SiteKind::Static,
            enabled: true,
            ..Default::default()
        })
        .unwrap();
    project.id
}

#[test]
fn hide_keeps_the_database_record_and_settings_restore_it() {
    let (inner, home) = inner();
    let folder = home.paths.data_dir().join("www").join("kept-site");
    add_site(&inner, &folder, "kept-site.test");

    inner.remove_domain("kept-site.test").unwrap();
    assert_eq!(
        inner.reconcile().unwrap(),
        1,
        "only the built-in site stays active"
    );
    assert!(inner
        .domains
        .lock()
        .unwrap()
        .get("kept-site.test")
        .is_some());
    assert!(!inner.site_is_active(&inner.domains.lock().unwrap().get("kept-site.test").unwrap()));

    drop(inner);
    let inner = reopen(&home);
    inner.reconcile().unwrap();
    let domain = inner.domains.lock().unwrap().get("kept-site.test").unwrap();
    assert!(
        !inner.site_is_active(&domain),
        "Hide must survive reopening the core"
    );

    assert!(inner.forget_deleted_item("kept-site.test").unwrap());
    assert_eq!(inner.reconcile().unwrap(), 2);
    assert!(inner.site_is_active(&inner.domains.lock().unwrap().get("kept-site.test").unwrap()));
    assert!(
        folder.exists(),
        "restoring a hidden site must preserve its files"
    );
}

#[test]
fn marker_makes_a_manual_site_inactive_without_deleting_its_record_and_removal_restores_it() {
    let (inner, home) = inner();
    let folder = home.paths.data_dir().join("www").join("marked-site");
    add_site(&inner, &folder, "marked-site.test");
    let domain = inner
        .domains
        .lock()
        .unwrap()
        .get("marked-site.test")
        .unwrap();

    std::fs::write(folder.join(".ols_ignore_me"), "").unwrap();
    assert_eq!(inner.reconcile().unwrap(), 1);
    assert!(inner
        .domains
        .lock()
        .unwrap()
        .get("marked-site.test")
        .is_some());
    assert!(!inner.site_is_active(&domain));

    drop(inner);
    let inner = reopen(&home);
    inner.reconcile().unwrap();
    let domain = inner
        .domains
        .lock()
        .unwrap()
        .get("marked-site.test")
        .unwrap();
    assert!(
        !inner.site_is_active(&domain),
        "the marker remains effective after reopening the core"
    );

    std::fs::remove_file(folder.join(".ols_ignore_me")).unwrap();
    assert_eq!(inner.reconcile().unwrap(), 2);
    assert!(inner.site_is_active(&domain));
}

#[test]
fn remove_from_list_deletes_manual_records_but_marks_discovered_sites() {
    let (inner, home) = inner();
    let manual = home.paths.data_dir().join("manual-site");
    let manual_id = add_site(&inner, &manual, "manual-site.test");
    inner.remove_site_from_list("manual-site.test").unwrap();
    assert!(inner.projects.lock().unwrap().get(&manual_id).is_none());
    assert!(inner
        .domains
        .lock()
        .unwrap()
        .get("manual-site.test")
        .is_none());
    assert!(
        manual.exists(),
        "Remove from list must preserve project files"
    );

    drop(inner);
    let inner = reopen(&home);
    inner.reconcile().unwrap();
    assert!(inner
        .domains
        .lock()
        .unwrap()
        .get("manual-site.test")
        .is_none());
    assert!(inner.projects.lock().unwrap().get(&manual_id).is_none());

    let root = home.paths.data_dir().join("discover");
    let discovered = root.join("discovered-site");
    let discovered_id = add_site(&inner, &discovered, "discovered-site.test");
    inner
        .remember_projects_root(&root.display().to_string())
        .unwrap();
    inner.remove_site_from_list("discovered-site.test").unwrap();
    assert!(inner.projects.lock().unwrap().get(&discovered_id).is_some());
    assert!(inner
        .domains
        .lock()
        .unwrap()
        .get("discovered-site.test")
        .is_some());
    assert!(discovered.join(".ols_ignore_me").is_file());
    assert!(!inner.site_is_active(
        &inner
            .domains
            .lock()
            .unwrap()
            .get("discovered-site.test")
            .unwrap()
    ));

    drop(inner);
    let inner = reopen(&home);
    inner.reconcile().unwrap();
    let domain = inner
        .domains
        .lock()
        .unwrap()
        .get("discovered-site.test")
        .unwrap();
    assert!(
        !inner.site_is_active(&domain),
        "the ignored discovered site stays inactive after reopening"
    );

    std::fs::remove_file(discovered.join(".ols_ignore_me")).unwrap();
    inner.reconcile().unwrap();
    assert!(inner.site_is_active(
        &inner
            .domains
            .lock()
            .unwrap()
            .get("discovered-site.test")
            .unwrap()
    ));
}

#[test]
fn reconcile_removes_and_restores_managed_hosts_entries_for_hidden_sites() {
    let (inner, home) = inner();
    let hosts = home.paths.data_dir().join("hosts");
    std::fs::write(&hosts, "127.0.0.1 localhost\n").unwrap();
    // SAFETY: isolated_home serializes tests that alter process-wide test environment.
    unsafe { std::env::set_var("OLS_HOSTS_FILE", &hosts) };
    let _restore_hosts_path = HostsPathGuard;
    let folder = home.paths.data_dir().join("www").join("hosts-site");
    add_site(&inner, &folder, "hosts-site.local");

    inner.reconcile().unwrap();
    assert!(std::fs::read_to_string(&hosts)
        .unwrap()
        .contains("127.0.0.1 hosts-site.local"));

    inner.remove_domain("hosts-site.local").unwrap();
    inner.reconcile().unwrap();
    assert!(!std::fs::read_to_string(&hosts)
        .unwrap()
        .contains("hosts-site.local"));

    inner.forget_deleted_item("hosts-site.local").unwrap();
    inner.reconcile().unwrap();
    assert!(std::fs::read_to_string(&hosts)
        .unwrap()
        .contains("127.0.0.1 hosts-site.local"));
}

struct HostsPathGuard;

impl Drop for HostsPathGuard {
    fn drop(&mut self) {
        // SAFETY: paired with the test's scoped OLS_HOSTS_FILE override.
        unsafe { std::env::remove_var("OLS_HOSTS_FILE") };
    }
}
