use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use serde_json::Value;

use crate::command::*;
use crate::Core;
use crate::web::manager::ConfigPart;
use crate::domain::{Domain, Ownership};
use crate::error::CoreError;
use crate::project::Project;

fn test_core() -> (Core, crate::test_support::IsolatedHome) {
    let home = crate::test_support::isolated_home();
    let settings = crate::settings::SettingsService::load(&home.paths).unwrap();
    let core = Core::new(settings, home.paths.clone());
    (core, home)
}

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
