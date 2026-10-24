use std::sync::Arc;
use crate::app::*;
use crate::domain::{Domain, SiteKind};

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
