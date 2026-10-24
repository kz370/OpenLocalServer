use std::sync::Arc;

use super::*;
use crate::domain::{Domain, SiteKind};

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

#[cfg(test)]
mod project_detail_tests {
    use super::*;

    fn inner() -> (Arc<Inner>, crate::test_support::IsolatedHome) {
        let home = crate::test_support::isolated_home();
        let settings = crate::settings::SettingsService::load(&home.paths).unwrap();
        (Inner::new(settings, home.paths.clone()).unwrap(), home)
    }

    /// A panic while a core lock is held poisons it, and the old `.lock().unwrap()` then made
    /// every later read panic as well — `get_project_detail` never answered, so the site's
    /// settings stayed on "Reading the project…" for the rest of the session.
    #[test]
    fn project_detail_survives_a_poisoned_projects_lock() {
        let (inner, home) = inner();
        let dir = home.paths.data_dir().join("shop");
        std::fs::create_dir_all(&dir).unwrap();
        let project = inner
            .projects
            .lock()
            .unwrap()
            .register(&dir.display().to_string())
            .unwrap();

        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _held = inner.projects.lock().unwrap();
            panic!("a command panicked while holding the projects lock");
        }));
        assert!(
            inner.projects.lock().is_err(),
            "the lock should be poisoned"
        );

        let detail = inner.project_detail(&project.id);
        assert!(
            detail.is_some(),
            "a poisoned lock must not hide the project"
        );
    }

    /// The locks must not be held across `build_detail`'s file I/O: a project on an
    /// unreachable mount blocks there for minutes, and every other command waits behind it.
    #[test]
    fn project_detail_does_not_hold_the_settings_lock_while_reading_the_project() {
        let (inner, home) = inner();
        let dir = home.paths.data_dir().join("shop");
        std::fs::create_dir_all(&dir).unwrap();
        let project = inner
            .projects
            .lock()
            .unwrap()
            .register(&dir.display().to_string())
            .unwrap();

        // Prove the guard is dropped before the read by taking every lock from this thread
        // while the read is in flight on another.
        let reader = {
            let inner = Arc::clone(&inner);
            let id = project.id.clone();
            std::thread::spawn(move || inner.project_detail(&id).is_some())
        };
        for _ in 0..200 {
            if reader.is_finished() {
                break;
            }
            drop(lock(&inner.settings));
            drop(lock(&inner.custom_installs));
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(
            reader.join().unwrap(),
            "project_detail should have answered"
        );
    }
}

#[cfg(test)]
mod open_database_tests {
    use super::*;

    #[test]
    fn redis_tinyrdm_routes_to_detected_tool() {
        let home = crate::test_support::isolated_home();
        let settings = crate::settings::SettingsService::load(&home.paths).unwrap();
        let inner = Inner::new(settings, home.paths.clone()).unwrap();
        let err = inner
            .open_database("redis", None, None, Some("tinyrdm"))
            .unwrap_err()
            .to_string();
        assert!(
            !err.contains("No tool is registered"),
            "tinyrdm fell through routing: {err}"
        );
    }
}

#[cfg(test)]
mod log_source_tests {
    use super::*;

    fn inner() -> (Arc<Inner>, crate::test_support::IsolatedHome) {
        let home = crate::test_support::isolated_home();
        let settings = crate::settings::SettingsService::load(&home.paths).unwrap();
        (Inner::new(settings, home.paths.clone()).unwrap(), home)
    }

    /// A service names the log source holding its own output, so "View logs" on
    /// a service row opens that service rather than the app log.
    #[test]
    fn every_service_names_its_own_log_source() {
        let (inner, _home) = inner();
        for s in inner.services.list() {
            let source = s
                .log_source
                .unwrap_or_else(|| panic!("{} names no log source", s.id));
            if s.kind == "web" {
                assert!(source.starts_with("web:"), "{}: {source}", s.id);
            } else {
                assert_eq!(source, format!("service:{}", s.id), "{}", s.id);
            }
        }
    }

    #[test]
    fn a_stopped_service_reads_empty_rather_than_failing() {
        let (inner, _home) = inner();
        // The output buffer dies with the process, so a stopped service has no
        // lines — an error here would look like a broken Logs page.
        assert!(inner.read_log("service:redis", 100).unwrap().is_empty());
        inner.clear_log("service:redis").unwrap();
    }

    #[test]
    fn each_web_server_reads_its_own_error_log() {
        let (inner, home) = inner();
        for id in crate::web::SERVER_IDS {
            let file = home.paths.web_dir().join(id).join("logs").join("error.log");
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, format!("{id} failed to bind\n")).unwrap();

            let lines = inner.read_log(&format!("web:{id}:error"), 100).unwrap();
            assert_eq!(lines, vec![format!("{id} failed to bind")], "{id}");

            inner.clear_log(&format!("web:{id}:error")).unwrap();
            assert!(inner
                .read_log(&format!("web:{id}:error"), 100)
                .unwrap()
                .is_empty());
        }
    }

    #[test]
    fn an_unknown_web_server_is_reported_not_silently_empty() {
        let (inner, _home) = inner();
        let err = inner
            .read_log("web:traefik:error", 100)
            .unwrap_err()
            .to_string();
        assert!(err.contains("traefik"), "{err}");
    }
}

#[cfg(test)]
mod startup_visibility_tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    /// The bug this replaced: the preference was ANDed with `--minimized`, so a user
    /// who turned "Start minimized to the tray" on still got a window every time
    /// they opened the app themselves. Only a launch at login hid it.
    #[test]
    fn the_preference_alone_hides_a_manual_launch() {
        assert!(should_start_hidden(
            true,
            args(&["C:\\app\\Open Local Server.exe"])
        ));
    }

    #[test]
    fn turning_the_preference_off_opens_the_window() {
        assert!(!should_start_hidden(
            false,
            args(&["C:\\app\\Open Local Server.exe"])
        ));
    }

    #[test]
    fn the_flag_forces_a_hidden_start_with_the_preference_off() {
        assert!(should_start_hidden(
            false,
            args(&["C:\\app\\Open Local Server.exe", FORCE_MINIMIZED_ARG])
        ));
    }

    #[test]
    fn the_flag_changes_nothing_when_the_preference_is_on() {
        assert!(should_start_hidden(
            true,
            args(&["C:\\app\\Open Local Server.exe", FORCE_MINIMIZED_ARG])
        ));
    }

    /// Every argument the app is actually launched with must be tolerated; only the
    /// exact flag may hide the window. A near-miss like `--minimized=true` is a
    /// different argument and must not silently start hidden.
    #[test]
    fn only_the_exact_flag_counts() {
        for other in [
            "--minimized=true",
            "--hidden",
            "--tray",
            "--MINIMIZED",
            "-minimized",
        ] {
            assert!(
                !should_start_hidden(false, args(&["app.exe", other])),
                "{other} hid the window"
            );
        }
    }

    /// The startup entry's path has to survive the spaces in a Windows install folder,
    /// because that folder is exactly what a user renames or moves. Splitting the row on
    /// whitespace would truncate `C:\Program Files\…` to `C:\Program` and the move check
    /// would then test a path that never existed — always "gone", rewriting the entry on
    /// every launch.
    #[test]
    fn the_startup_entry_path_keeps_its_spaces() {
        let blob = "HKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Run\r\n\
                    OpenLocalServer    REG_SZ    \"C:\\Program Files\\Open Local Server\\Open Local Server.exe\"\r\n";
        assert_eq!(
            parse_run_target(blob).as_deref(),
            Some(r"C:\Program Files\Open Local Server\Open Local Server.exe")
        );
    }

    /// `reg add` may or may not keep the quote characters in the stored value depending
    /// on how it was invoked, so both spellings have to read back the same path.
    #[test]
    fn an_unquoted_startup_entry_is_read_too() {
        let blob = "HKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Run\r\n\
                    OpenLocalServer    REG_SZ    C:\\Tools\\OLS\\Open Local Server.exe\r\n";
        assert_eq!(
            parse_run_target(blob).as_deref(),
            Some(r"C:\Tools\OLS\Open Local Server.exe")
        );
    }

    /// An entry this app did not write must be left alone. Guessing a path here would
    /// rewrite somebody else's working startup entry, so anything unrecognisable is None.
    #[test]
    fn an_unrecognisable_startup_entry_yields_nothing() {
        for blob in [
            "",
            "ERROR: The system was unable to find the specified registry key or value.\r\n",
            "HKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Run\r\n",
            "HKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Run\r\n\
             OpenLocalServer    REG_SZ\r\n",
        ] {
            assert_eq!(parse_run_target(blob), None, "{blob:?}");
        }
    }

    /// The stored preference is what reaches the decision, so a value that is not a
    /// bool must not quietly flip a launch either way. `setting_bool` answers false
    /// for a non-bool instead of inventing the default.
    #[test]
    fn a_non_bool_stored_value_does_not_hide_the_window() {
        let home = crate::test_support::isolated_home();
        let settings = crate::settings::SettingsService::load(&home.paths).unwrap();
        let inner = Inner::new(settings, home.paths.clone()).unwrap();

        // Never written: the documented default is to start minimized.
        assert!(inner.setting_bool("startup.minimized", true));
        assert!(should_start_hidden(
            inner.setting_bool("startup.minimized", true),
            args(&["app.exe"])
        ));

        // Written as a string, not a bool: the read falls back to the default the
        // caller passed, so the launch follows the default rather than a guess.
        let mut settings = crate::settings::SettingsService::load(&home.paths).unwrap();
        settings
            .set("startup.minimized", serde_json::json!("true"))
            .unwrap();
        let inner = Inner::new(settings, home.paths.clone()).unwrap();
        assert!(inner.setting_bool("startup.minimized", true));
        assert!(!inner.setting_bool("startup.minimized", false));
    }

    /// The preference survives a restart, because the decision is made on a later
    /// launch than the one where it was toggled.
    #[test]
    fn turning_the_preference_off_survives_a_reload() {
        let home = crate::test_support::isolated_home();
        let mut settings = crate::settings::SettingsService::load(&home.paths).unwrap();
        settings
            .set("startup.minimized", serde_json::json!(false))
            .unwrap();
        drop(settings);

        let reloaded = crate::settings::SettingsService::load(&home.paths).unwrap();
        let inner = Inner::new(reloaded, home.paths.clone()).unwrap();
        assert!(!should_start_hidden(
            inner.setting_bool("startup.minimized", true),
            args(&["app.exe"])
        ));
    }
}

#[cfg(test)]
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
