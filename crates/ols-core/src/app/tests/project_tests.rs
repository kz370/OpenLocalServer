use std::sync::Arc;
use crate::app::*;
use crate::domain::{Domain, SiteKind};

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
