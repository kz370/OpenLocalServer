use std::path::Path;

use super::types::*;
use crate::setup::plan_text;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{Core, CoreCommand, CoreResponse};
    use crate::manifest;

    fn core() -> (Core, crate::test_support::IsolatedHome) {
        let home = crate::test_support::isolated_home();
        let settings = crate::settings::SettingsService::load(&home.paths).unwrap();
        (Core::new(settings, home.paths.clone()), home)
    }

    fn register(core: &Core, dir: &Path) -> String {
        match core
            .dispatch(CoreCommand::RegisterProject {
                path: dir.display().to_string(),
            })
            .unwrap()
        {
            CoreResponse::Project { project } => project.id,
            _ => panic!(),
        }
    }

    #[test]
    fn db_names_are_safe_identifiers() {
        assert_eq!(db_name_for("My-Shop 2"), "my_shop_2");
        assert_eq!(db_name_for("--"), "app");
        assert!(crate::service::is_safe_identifier(&db_name_for(
            "ünïcode-thing"
        )));
    }

    #[test]
    fn a_plan_follows_the_manifest_and_a_dry_run_changes_nothing() {
        let (core, home) = core();
        let dir = home.paths.root().join("shop");
        std::fs::create_dir_all(dir.join("public")).unwrap();
        std::fs::create_dir_all(dir.join(".openlocalserver")).unwrap();
        std::fs::write(dir.join("public").join("index.php"), "<?php").unwrap();
        std::fs::write(dir.join(".openlocalserver").join("environment.yaml"), "name: shop\nruntime:\n  php: \"8.4\"\ndomain:\n  hostname: shop.test\n  https: true\n  root: public\ndatabase:\n  engine: mysql\nservices:\n  redis: true\n").unwrap();
        let id = register(&core, &dir);

        let CoreResponse::SetupPlan { plan } = core
            .dispatch(CoreCommand::PlanSetup {
                project_id: id.clone(),
            })
            .unwrap()
        else {
            panic!()
        };
        assert!(plan.manifest_found);
        let labels: Vec<&str> = plan.steps.iter().map(|s| s.label.as_str()).collect();
        let text = labels.join("\n");
        for want in [
            "PHP",
            "MariaDB database \"shop\"",
            "Redis",
            "shop.test",
            "environment.lock",
        ] {
            assert!(text.contains(want), "missing {want} in:\n{text}");
        }
        let pos = |needle: &str| labels.iter().position(|l| l.contains(needle)).unwrap();
        assert!(
            pos("PHP") < pos("database") && pos("database") < pos("shop.test"),
            "§74 order: runtimes, database, domain"
        );

        let CoreResponse::Setup { report } = core
            .dispatch(CoreCommand::ApplySetup {
                project_id: id.clone(),
                dry_run: true,
            })
            .unwrap()
        else {
            panic!()
        };
        assert!(report.dry_run && !report.running);
        assert!(
            report
                .steps
                .iter()
                .all(|s| matches!(s.status, StepStatus::Pending | StepStatus::Skipped)),
            "a dry run runs nothing"
        );
        assert!(!manifest::lock_path(&dir).exists());
    }

    #[test]
    fn a_domain_owned_by_another_project_is_a_blocking_conflict() {
        let (core, home) = core();
        let a = home.paths.root().join("a");
        let b = home.paths.root().join("b");
        for d in [&a, &b] {
            std::fs::create_dir_all(d.join(".openlocalserver")).unwrap();
            std::fs::write(d.join("index.html"), "hi").unwrap();
            std::fs::write(
                d.join(".openlocalserver").join("environment.yaml"),
                "domain:\n  hostname: same.test\n",
            )
            .unwrap();
        }
        let a_id = register(&core, &a);
        let b_id = register(&core, &b);
        let CoreResponse::SetupPlan { plan } = core
            .dispatch(CoreCommand::PlanSetup {
                project_id: a_id.clone(),
            })
            .unwrap()
        else {
            panic!()
        };
        let SetupAction::AddDomain { domain } = &plan
            .steps
            .iter()
            .find(|s| matches!(s.action, SetupAction::AddDomain { .. }))
            .unwrap()
            .action
        else {
            panic!()
        };
        core.inner().add_domain((**domain).clone()).unwrap();

        let CoreResponse::SetupPlan { plan } = core
            .dispatch(CoreCommand::PlanSetup {
                project_id: b_id.clone(),
            })
            .unwrap()
        else {
            panic!()
        };
        assert!(!plan.ok);
        assert!(
            plan.conflicts
                .iter()
                .any(|c| c.kind == "domain" && c.blocking && c.message.contains("a")),
            "{:?}",
            plan.conflicts
        );
        let CoreResponse::Setup { report } = core
            .dispatch(CoreCommand::ApplySetup {
                project_id: b_id,
                dry_run: false,
            })
            .unwrap()
        else {
            panic!()
        };
        assert!(
            !report.ok && report.error.is_some(),
            "a blocked plan is never applied"
        );
    }

    #[test]
    fn a_failed_step_rolls_back_the_safe_changes_before_it() {
        let (core, home) = core();
        let dir = home.paths.root().join("site");
        std::fs::create_dir_all(dir.join(".openlocalserver")).unwrap();
        std::fs::write(dir.join("index.html"), "hi").unwrap();
        // The site is added, then the unknown-extension-free failure: SQLite needs sqlite3,
        // which isn't installed in a test home, so creating the database fails.
        std::fs::write(
            dir.join(".openlocalserver").join("environment.yaml"),
            "domain:\n  hostname: roll.test\ndatabase:\n  engine: sqlite\n",
        )
        .unwrap();
        let id = register(&core, &dir);
        let plan = core.inner().plan_setup(&id).unwrap();
        // Put the site before the database so there is something to roll back.
        let mut steps = plan.steps.clone();
        let site = steps
            .iter()
            .position(|s| matches!(s.action, SetupAction::AddDomain { .. }))
            .unwrap();
        let s = steps.remove(site);
        steps.insert(0, s);
        let plan = EnvironmentPlan { steps, ..plan };
        let mut report = SetupReport {
            steps: plan
                .steps
                .iter()
                .map(|s| StepResult {
                    group: s.group.clone(),
                    label: s.label.clone(),
                    status: StepStatus::Pending,
                    detail: None,
                })
                .collect(),
            ..Default::default()
        };
        core.inner().run_plan(&plan, &mut report);
        assert!(
            report.error.is_some(),
            "creating SQLite without sqlite3 fails"
        );
        assert_eq!(report.steps[0].status, StepStatus::RolledBack);
        assert!(
            core.inner()
                .domains
                .lock()
                .unwrap()
                .get("roll.test")
                .is_none(),
            "the site added before the failure is gone"
        );
        assert!(report.rolled_back.iter().any(|r| r.contains("roll.test")));
    }

    #[test]
    fn without_a_manifest_the_plan_comes_from_detection_and_can_be_saved() {
        let (core, home) = core();
        let dir = home.paths.root().join("blog");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.html"), "hi").unwrap();
        let id = register(&core, &dir);
        let CoreResponse::SetupPlan { plan } = core
            .dispatch(CoreCommand::PlanSetup {
                project_id: id.clone(),
            })
            .unwrap()
        else {
            panic!()
        };
        assert!(!plan.manifest_found);
        assert_eq!(
            plan.manifest.domain.as_ref().unwrap().hostname,
            "blog.local"
        );
        core.dispatch(CoreCommand::SaveManifest {
            project_id: id.clone(),
            manifest: None,
        })
        .unwrap();
        assert!(manifest::read_manifest(&dir).unwrap().is_some());
        assert!(plan_text(&plan).contains("Environment Plan: blog"));
    }
}
