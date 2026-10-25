use crate::app::projects::skip::same_skip_entry;
use crate::app::*;
use crate::domain::{Domain, SiteKind};
use std::sync::Arc;

#[cfg(test)]
mod project_skip_tests {
    use super::*;

    fn inner() -> (Arc<Inner>, crate::test_support::IsolatedHome) {
        let home = crate::test_support::isolated_home();
        let settings = crate::settings::SettingsService::load(&home.paths).unwrap();
        (Inner::new(settings, home.paths.clone()).unwrap(), home)
    }

    /// A site added from the Explorer right-click menu is written by another process, so
    /// the running app has to notice the database moved under it. Without the re-read the
    /// new site stayed invisible until a restart, which is what made the menu look broken.
    #[test]
    fn a_site_added_by_another_process_shows_up_without_a_restart() {
        let (inner, home) = inner();
        let root = home.paths.data_dir().join("www");
        let app1 = project_dir(&root, "app1");
        inner
            .remember_projects_root(&root.display().to_string())
            .unwrap();
        inner.sync_auto_domains().unwrap();
        let before: Vec<String> = inner
            .domain_summaries()
            .into_iter()
            .map(|d| d.hostname)
            .collect();
        assert!(before.iter().any(|h| h.starts_with("app1")));

        // Stand in for the `ols project add` the right-click menu runs: a second process
        // writing straight to the database, with this app still holding the old rows.
        {
            let mut projects = ProjectStore::load(&home.paths).unwrap();
            projects.register(&app1.display().to_string()).unwrap();
        }
        project_dir(&root, "app2");
        crate::db::simulate_foreign_write(&home.paths);

        assert!(inner.reload_external_changes());
        let hosts: Vec<String> = inner
            .domain_summaries()
            .into_iter()
            .map(|d| d.hostname)
            .collect();
        assert!(
            hosts.iter().any(|h| h.starts_with("app2")),
            "the outside project's site is missing: {hosts:?}"
        );
    }

    /// The app's own writes must not read as somebody else's, or every save would throw
    /// the in-memory stores away and re-read them.
    #[test]
    fn the_apps_own_writes_are_not_mistaken_for_an_outside_change() {
        let (inner, home) = inner();
        let root = home.paths.data_dir().join("www");
        project_dir(&root, "app1");
        inner
            .remember_projects_root(&root.display().to_string())
            .unwrap();
        inner.sync_auto_domains().unwrap();
        for _ in 0..3 {
            assert!(!inner.reload_external_changes());
        }
    }

    /// A site whose name is not in the hosts file does not resolve, so the folder the
    /// user just dropped in is a row in the table and nothing else. The new names go into
    /// the hosts file as they are created, which is what makes the domain actually work.
    #[test]
    fn a_new_automatic_domain_is_added_to_the_hosts_file() {
        let home = crate::test_support::isolated_home();
        let hosts = home.paths.data_dir().join("hosts");
        std::fs::write(&hosts, "127.0.0.1 localhost\n").unwrap();
        // SAFETY: single-threaded test setup; the variable is process-global and no other
        // test in this binary reads it concurrently in a way that matters here.
        unsafe { std::env::set_var("OLS_HOSTS_FILE", &hosts) };
        let root = home.paths.data_dir().join("www");
        project_dir(&root, "app1");

        let settings = crate::settings::SettingsService::load(&home.paths).unwrap();
        let inner = Inner::new(settings, home.paths.clone()).unwrap();
        inner
            .remember_projects_root(&root.display().to_string())
            .unwrap();
        assert_eq!(inner.sync_auto_domains().unwrap(), 1);

        let written = std::fs::read_to_string(&hosts).unwrap();
        assert!(
            written.contains("app1.local"),
            "the new domain never reached the hosts file: {written}"
        );
        // The user's own entries are never touched (§75).
        assert!(written.contains("127.0.0.1 localhost"));
        unsafe { std::env::remove_var("OLS_HOSTS_FILE") };
    }

    fn project_dir(root: &std::path::Path, name: &str) -> std::path::PathBuf {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.html"), "hi").unwrap();
        dir
    }

    #[test]
    fn removed_projects_stay_removed_across_rescans() {
        let (inner, home) = inner();
        let root = home.paths.data_dir().join("www");
        let app1 = project_dir(&root, "app1");
        project_dir(&root, "app2");
        inner
            .remember_projects_root(&root.display().to_string())
            .unwrap();

        inner.sync_auto_domains().unwrap();
        assert_eq!(inner.projects.lock().unwrap().list().len(), 2);

        let id1 = inner.projects.lock().unwrap().list()[0].id.clone();
        inner.remove_project(&id1).unwrap();
        assert_eq!(inner.projects.lock().unwrap().list().len(), 1);

        // Rescans (watcher, restart, Scan) must not bring it back.
        inner.sync_auto_domains().unwrap();
        inner.sync_auto_domains().unwrap();
        let ids: Vec<_> = inner
            .projects
            .lock()
            .unwrap()
            .list()
            .into_iter()
            .map(|p| p.id)
            .collect();
        assert!(!ids.contains(&id1), "removed project was re-registered");
        assert_eq!(ids.len(), 1);

        // Explicitly adding the folder again clears the removal.
        let p = inner
            .projects
            .lock()
            .unwrap()
            .register(&app1.display().to_string())
            .unwrap();
        inner.clear_project_skip(&p.path).unwrap();
        inner.sync_auto_domains().unwrap();
        assert_eq!(inner.projects.lock().unwrap().list().len(), 2);
    }

    #[test]
    fn deletion_history_lists_deleted_projects_and_sites_newest_first() {
        let (inner, home) = inner();
        let root = home.paths.data_dir().join("www");
        project_dir(&root, "app1");
        project_dir(&root, "app2");
        inner
            .remember_projects_root(&root.display().to_string())
            .unwrap();
        inner.sync_auto_domains().unwrap();

        let id = inner.projects.lock().unwrap().list()[0].id.clone();
        inner.remove_project(&id).unwrap();
        let hostname = inner.domains.lock().unwrap().list()[0].hostname.clone();
        inner.remove_domain(&hostname).unwrap();

        let items = inner.list_deleted_items();
        assert!(
            items.iter().any(|i| i.is_project()),
            "the removed project folder is missing from the history"
        );
        assert!(
            items
                .iter()
                .any(|i| i.kind == "domain" && i.value == hostname),
            "the deleted automatic site is missing from the history"
        );
        assert!(
            items.iter().all(|i| i.deleted_at > 0),
            "entries must carry when they were deleted"
        );
        let times: Vec<_> = items.iter().map(|i| i.deleted_at).collect();
        let mut sorted = times.clone();
        sorted.sort_unstable_by(|a, b| b.cmp(a));
        assert_eq!(times, sorted, "history must be newest first");
    }

    #[test]
    fn clearing_one_history_entry_lets_scans_register_the_folder_again() {
        let (inner, home) = inner();
        let root = home.paths.data_dir().join("www");
        let app = project_dir(&root, "app1");
        inner
            .remember_projects_root(&root.display().to_string())
            .unwrap();
        inner.sync_auto_domains().unwrap();
        let id = inner.projects.lock().unwrap().list()[0].id.clone();
        inner.remove_project(&id).unwrap();

        // This is the round trip the settings page exists for: delete, then take the
        // folder back out of the history and rescan.
        assert!(inner
            .forget_deleted_item(&app.display().to_string())
            .unwrap());
        assert!(inner.list_deleted_items().is_empty());
        inner.sync_auto_domains().unwrap();
        assert_eq!(inner.projects.lock().unwrap().list().len(), 1);
    }

    #[test]
    fn clear_deleted_items_empties_both_histories() {
        let (inner, home) = inner();
        let root = home.paths.data_dir().join("www");
        project_dir(&root, "app1");
        inner
            .remember_projects_root(&root.display().to_string())
            .unwrap();
        inner.sync_auto_domains().unwrap();
        let id = inner.projects.lock().unwrap().list()[0].id.clone();
        inner.remove_project(&id).unwrap();
        let hostname = inner.domains.lock().unwrap().list()[0].hostname.clone();
        inner.remove_domain(&hostname).unwrap();
        assert!(!inner.list_deleted_items().is_empty());

        assert!(inner.clear_deleted_items().unwrap() > 0);
        assert!(inner.list_deleted_items().is_empty());
        // Both histories really are empty, so a rescan sees the folder again.
        inner.sync_auto_domains().unwrap();
        assert_eq!(inner.projects.lock().unwrap().list().len(), 1);
        assert!(!inner.domains.lock().unwrap().list().is_empty());
    }

    /// Lists written before the history existed held bare strings. They must keep working
    /// instead of silently turning into an empty history.
    #[test]
    fn skip_lists_written_as_plain_strings_still_skip() {
        let (inner, home) = inner();
        let root = home.paths.data_dir().join("www");
        let app = project_dir(&root, "app1");
        inner
            .settings
            .lock()
            .unwrap()
            .set(PROJECT_SKIP, serde_json::json!([app.display().to_string()]))
            .unwrap();
        inner
            .remember_projects_root(&root.display().to_string())
            .unwrap();

        inner.sync_auto_domains().unwrap();
        assert!(
            inner.projects.lock().unwrap().list().is_empty(),
            "a folder skipped by an older list was re-registered"
        );
        let items = inner.list_deleted_items();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].value, app.display().to_string());
    }

    /// The settings page clears a history entry with the spelling it has on screen, while the
    /// stored entry is the canonicalized path `register` wrote. On Windows those are different
    /// strings whenever the path has more than one — a short name, a `..` segment, a junction —
    /// so the entry survived the clear, the history still listed it, and the folder below stayed
    /// skipped: the scan registered nothing and the user was told the restore had worked.
    #[test]
    fn clearing_a_history_entry_by_another_spelling_of_the_path_works() {
        let (inner, home) = inner();
        let root = home.paths.data_dir().join("www");
        let app = project_dir(&root, "app1");
        inner
            .remember_projects_root(&root.display().to_string())
            .unwrap();
        inner.sync_auto_domains().unwrap();
        let id = inner.projects.lock().unwrap().list()[0].id.clone();
        inner.remove_project(&id).unwrap();

        // A second spelling of the same folder that still resolves, so this fails on any
        // machine rather than only where the temp path happens to be an 8.3 short name.
        let detour = root.join("app1").join("..").join("app1");
        let detour = detour.display().to_string();
        assert_ne!(detour, app.display().to_string());
        assert!(std::fs::canonicalize(&detour).is_ok());
        assert!(
            inner.forget_deleted_item(&detour).unwrap(),
            "clearing the entry by another spelling of its path left it in the history"
        );
        assert!(inner.list_deleted_items().is_empty());
        inner.sync_auto_domains().unwrap();
        assert_eq!(
            inner.projects.lock().unwrap().list().len(),
            1,
            "the folder was still skipped after the history said it was cleared"
        );
    }

    /// `same_skip_entry` is the compare the clear, the record and the merge all use, so the
    /// rule is pinned directly instead of only through one round trip. The last half is the
    /// one that would otherwise be a new bug: a bare domain name must not be resolved against
    /// the working directory, where a folder of the same name would make it match.
    #[test]
    fn a_history_entry_matches_another_spelling_of_its_own_path_and_nothing_else() {
        let (inner, home) = inner();
        let root = home.paths.data_dir().join("www");
        let app = project_dir(&root, "app1");
        let canonical = std::fs::canonicalize(&app).unwrap();
        inner
            .push_skip(PROJECT_SKIP, &canonical.display().to_string())
            .unwrap();
        let entries = inner.skip_entries(PROJECT_SKIP);
        assert_eq!(entries.len(), 1, "one folder must not become two entries");

        let detour = root.join("app1").join("..").join("app1");
        assert!(same_skip_entry(&entries[0], &detour.display().to_string()));
        // Case-insensitive, on the full path: that compare predates this fix.
        assert!(same_skip_entry(
            &entries[0],
            &canonical.display().to_string().to_uppercase()
        ));
        assert!(!same_skip_entry(&entries[0], "app2"));
        assert!(!same_skip_entry(&entries[0], "app1"));

        // Re-deleting the same folder through the other spelling updates the entry rather
        // than appending a duplicate, so the history cannot grow every time it is used.
        inner
            .push_skip(PROJECT_SKIP, &detour.display().to_string())
            .unwrap();
        assert_eq!(inner.skip_entries(PROJECT_SKIP).len(), 1);
    }

    /// The common order of operations: the user removes the project, then deletes the folder,
    /// then clears it from the history. Nothing is on disk to canonicalize by the third step,
    /// so a resolve that needs the whole path fails and the entry survives — the same silent
    /// no-op as before, one step later. The parent directory is still there to resolve.
    #[test]
    fn clearing_an_entry_whose_folder_is_already_deleted_works() {
        let (inner, home) = inner();
        let root = home.paths.data_dir().join("www");
        let app = project_dir(&root, "app1");
        inner
            .remember_projects_root(&root.display().to_string())
            .unwrap();
        inner.sync_auto_domains().unwrap();
        let id = inner.projects.lock().unwrap().list()[0].id.clone();
        inner.remove_project(&id).unwrap();
        std::fs::remove_dir_all(&app).unwrap();
        assert!(!app.exists());

        // Spelled through `..`, so this still depends on the two sides being resolved rather
        // than compared: the exact stored string would match on the plain compare.
        let detour = root.join("..").join("www").join("app1");
        let detour = detour.display().to_string();
        assert_ne!(detour, app.display().to_string());
        assert!(
            inner.forget_deleted_item(&detour).unwrap(),
            "clearing an entry for a folder that no longer exists left it in the history"
        );
        assert!(inner.list_deleted_items().is_empty());
    }

    /// A folder reached through a different spelling of the same path — a short name, a
    /// junction, or a path with `..` in it — used to dodge the skip list, so a scan
    /// re-added what the user deleted. The entry is stored the way `register` stores it,
    /// canonicalized; a scan only ever holds the raw `read_dir` spelling.
    #[test]
    fn a_deleted_folder_is_skipped_however_its_path_is_spelled() {
        let (inner, home) = inner();
        let root = home.paths.data_dir().join("www");
        let app = project_dir(&root, "app1");
        let detour = root.join("app1").join("..").join("app1");

        // Stored canonicalized, the way removing a project records it.
        let canonical = std::fs::canonicalize(&app).unwrap();
        inner
            .push_skip(PROJECT_SKIP, &canonical.display().to_string())
            .unwrap();
        let skipped = inner.project_skips();

        let raw = app.display().to_string();
        let detour = detour.display().to_string();
        assert_ne!(raw, detour, "the test needs two spellings of one folder");
        assert!(!skipped.iter().any(|e| e.value == detour));
        assert!(
            skip_matches(&skipped, &detour),
            "a scan that reached the deleted folder by another route must still skip it"
        );
        assert!(skip_matches(&skipped, &raw));
    }

    #[test]
    fn sync_auto_domains_does_not_duplicate_existing_domain_for_folder() {
        let (inner, home) = inner();
        let root = home.paths.data_dir().join("www");
        let site_dir = project_dir(&root, "test366.local");
        inner
            .remember_projects_root(&root.display().to_string())
            .unwrap();

        // Add domain explicitly (e.g. via MCP ols_add_domain) without project_id
        let d = Domain {
            hostname: "test366.local".into(),
            project_id: None,
            root: site_dir.display().to_string(),
            kind: SiteKind::Static,
            enabled: true,
            https: true,
            redirect_https: true,
            server: Some("caddy".into()),
            ..Default::default()
        };
        inner.add_domain(d).unwrap();

        // sync_auto_domains runs (as happens on watcher or refresh)
        let created = inner.sync_auto_domains().unwrap();
        assert_eq!(
            created, 0,
            "must not create a duplicate auto domain for existing folder"
        );

        let custom_domains: Vec<_> = inner
            .domains
            .lock()
            .unwrap()
            .list()
            .into_iter()
            .filter(|d| d.hostname != "openlocalserver.test")
            .collect();
        assert_eq!(custom_domains.len(), 1);
        assert_eq!(custom_domains[0].hostname, "test366.local");
        assert!(
            custom_domains[0].project_id.is_some(),
            "should link project_id to existing domain"
        );
    }
}
