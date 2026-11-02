use crate::command::*;
use crate::domain::{Domain, Ownership};
use crate::project::Project;
use crate::Core;

fn test_core() -> (Core, crate::test_support::IsolatedHome) {
    let home = crate::test_support::isolated_home();
    let settings = crate::settings::SettingsService::load(&home.paths).unwrap();
    let core = Core::new(settings, home.paths.clone());
    (core, home)
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
fn standalone_projects_do_not_get_auto_domains_and_can_create_site() {
    let (core, home) = test_core();
    let p_dir = home.paths.root().join("my-standalone-project");
    std::fs::create_dir_all(&p_dir).unwrap();
    std::fs::write(p_dir.join("index.html"), "<h1>Hello</h1>").unwrap();

    let project = match core
        .dispatch(CoreCommand::AddProject {
            path: p_dir.display().to_string(),
        })
        .unwrap()
    {
        CoreResponse::Project { project } => project,
        other => panic!("expected Project, got {other:?}"),
    };
    assert!(
        project.standalone,
        "added project must have standalone = true"
    );

    // SyncAutoDomains should not create a domain for standalone project
    core.dispatch(CoreCommand::SyncAutoDomains).unwrap();
    match core.dispatch(CoreCommand::ListDomains).unwrap() {
        CoreResponse::Domains { domains } => {
            assert!(
                !domains
                    .iter()
                    .any(|d| d.project_id.as_deref() == Some(&project.id)),
                "standalone project must not receive automatic domain"
            );
        }
        other => panic!("expected Domains, got {other:?}"),
    }

    // Now call CreateSiteForProject
    let res = match core
        .dispatch(CoreCommand::CreateSiteForProject {
            project_id: project.id.clone(),
        })
        .unwrap()
    {
        CoreResponse::CreateSiteForProject {
            domain,
            needs_setup,
            suggested_hostname,
        } => (domain, needs_setup, suggested_hostname),
        other => panic!("expected CreateSiteForProject, got {other:?}"),
    };

    assert!(!res.1, "should not need setup for static index.html site");
    let domain = res.0.expect("should have created domain");
    assert_eq!(domain.project_id.as_deref(), Some(project.id.as_str()));

    // Idempotent: calling again returns the existing domain
    let res2 = match core
        .dispatch(CoreCommand::CreateSiteForProject {
            project_id: project.id.clone(),
        })
        .unwrap()
    {
        CoreResponse::CreateSiteForProject {
            domain,
            needs_setup,
            ..
        } => (domain, needs_setup),
        other => panic!("expected CreateSiteForProject, got {other:?}"),
    };
    assert!(!res2.1);
    assert_eq!(res2.0.unwrap().hostname, domain.hostname);
}

#[test]
fn project_metadata_update_persists_icon_and_type() {
    let (core, home) = test_core();
    let p_dir = home.paths.root().join("test-proj-meta");
    std::fs::create_dir_all(&p_dir).unwrap();

    let project = match core
        .dispatch(CoreCommand::AddProject {
            path: p_dir.display().to_string(),
        })
        .unwrap()
    {
        CoreResponse::Project { project } => project,
        other => panic!("expected Project, got {other:?}"),
    };

    let updated = match core
        .dispatch(CoreCommand::UpdateProject {
            id: project.id.clone(),
            name: Some("Renamed Proj".into()),
            icon: Some("fa:rocket:#38bdf8".into()),
            clear_icon: None,
            project_type: Some("python".into()),
            clear_project_type: None,
        })
        .unwrap()
    {
        CoreResponse::Project { project } => project,
        other => panic!("expected Project, got {other:?}"),
    };

    assert_eq!(updated.name, "Renamed Proj");
    assert_eq!(updated.icon.as_deref(), Some("fa:rocket:#38bdf8"));
    assert_eq!(updated.project_type.as_deref(), Some("python"));

    // Reload projects to verify persistence
    match core.dispatch(CoreCommand::ListProjects).unwrap() {
        CoreResponse::Projects { projects } => {
            let found = projects.iter().find(|p| p.id == project.id).unwrap();
            assert_eq!(found.name, "Renamed Proj");
            assert_eq!(found.icon.as_deref(), Some("fa:rocket:#38bdf8"));
            assert_eq!(found.project_type.as_deref(), Some("python"));
        }
        other => panic!("expected Projects, got {other:?}"),
    }

    // Clear icon
    let cleared = match core
        .dispatch(CoreCommand::UpdateProject {
            id: project.id.clone(),
            name: None,
            icon: None,
            clear_icon: Some(true),
            project_type: None,
            clear_project_type: None,
        })
        .unwrap()
    {
        CoreResponse::Project { project } => project,
        other => panic!("expected Project, got {other:?}"),
    };
    assert!(cleared.icon.is_none());
    assert_eq!(cleared.project_type.as_deref(), Some("python"));
}

#[test]
fn custom_editors_crud_and_project_types() {
    let (core, home) = test_core();
    let fake_exe = home.paths.root().join("my-custom-editor.exe");
    std::fs::write(&fake_exe, "mock exe").unwrap();

    // 1. Add editor
    let editors = match core
        .dispatch(CoreCommand::AddEditor {
            name: "Awesome Editor".into(),
            path: fake_exe.display().to_string(),
        })
        .unwrap()
    {
        CoreResponse::Editors { editors } => editors,
        other => panic!("expected Editors, got {other:?}"),
    };

    let custom = editors.iter().find(|e| e.name == "Awesome Editor").unwrap();
    assert!(custom.custom);
    assert_eq!(custom.path.as_deref(), Some(fake_exe.to_str().unwrap()));
    let custom_id = custom.id.clone();

    // 2. Update editor
    let updated_editors = match core
        .dispatch(CoreCommand::UpdateEditor {
            id: custom_id.clone(),
            name: "Awesome Editor Pro".into(),
            path: fake_exe.display().to_string(),
        })
        .unwrap()
    {
        CoreResponse::Editors { editors } => editors,
        other => panic!("expected Editors, got {other:?}"),
    };
    assert!(updated_editors
        .iter()
        .any(|e| e.id == custom_id && e.name == "Awesome Editor Pro"));

    // 3. Remove editor
    let remaining = match core
        .dispatch(CoreCommand::RemoveEditor {
            id: custom_id.clone(),
        })
        .unwrap()
    {
        CoreResponse::Editors { editors } => editors,
        other => panic!("expected Editors, got {other:?}"),
    };
    assert!(!remaining.iter().any(|e| e.id == custom_id));

    // 4. Project types
    let types = match core.dispatch(CoreCommand::ListProjectTypes).unwrap() {
        CoreResponse::ProjectTypes { types } => types,
        other => panic!("expected ProjectTypes, got {other:?}"),
    };
    assert!(!types.is_empty());
    assert!(types.iter().any(|t| t.id == "laravel"));
    assert!(types.iter().any(|t| t.id == "python"));
}

#[test]
fn custom_project_types_crud_and_project_updates() {
    let core = test_core();
    let dir = core.inner().paths.root().join("test_custom_type_app");
    std::fs::create_dir_all(&dir).unwrap();

    // 1. Add a project with custom type "custom:Rust"
    let p = match core
        .dispatch(CoreCommand::AddProject {
            path: dir.to_str().unwrap().to_string(),
            name: Some("Rust App".into()),
            project_type: Some("custom:Rust".into()),
        })
        .unwrap()
    {
        CoreResponse::Project { project } => project,
        other => panic!("expected Project, got {other:?}"),
    };
    assert_eq!(p.project_type.as_deref(), Some("custom:Rust"));

    // 2. Add custom type via command
    let types = match core
        .dispatch(CoreCommand::AddCustomProjectType {
            name: "Go Service".into(),
        })
        .unwrap()
    {
        CoreResponse::CustomProjectTypes { types } => types,
        other => panic!("expected CustomProjectTypes, got {other:?}"),
    };
    assert!(types.iter().any(|t| t.name == "Go Service"));
    assert!(types.iter().any(|t| t.name == "Rust")); // Automatically recognized from existing project
    assert!(types.iter().any(|t| t.name == "Tauri")); // Pre-seeded custom type

    // 3. Rename "Rust" to "Rust Backend" -> must update the project's type
    let renamed_types = match core
        .dispatch(CoreCommand::RenameCustomProjectType {
            old_name: "Rust".into(),
            new_name: "Rust Backend".into(),
        })
        .unwrap()
    {
        CoreResponse::CustomProjectTypes { types } => types,
        other => panic!("expected CustomProjectTypes, got {other:?}"),
    };
    assert!(renamed_types.iter().any(|t| t.name == "Rust Backend"));
    assert!(!renamed_types.iter().any(|t| t.name == "Rust"));

    let projects = match core.dispatch(CoreCommand::ListProjects).unwrap() {
        CoreResponse::Projects { projects } => projects,
        other => panic!("expected Projects, got {other:?}"),
    };
    let updated_p = projects.iter().find(|pr| pr.id == p.id).unwrap();
    assert_eq!(
        updated_p.project_type.as_deref(),
        Some("custom:Rust Backend")
    );

    // 4. Remove "Rust Backend" -> project must reset to None ("Auto")
    let after_remove = match core
        .dispatch(CoreCommand::RemoveCustomProjectType {
            name: "Rust Backend".into(),
        })
        .unwrap()
    {
        CoreResponse::CustomProjectTypes { types } => types,
        other => panic!("expected CustomProjectTypes, got {other:?}"),
    };
    assert!(!after_remove.iter().any(|t| t.name == "Rust Backend"));

    let projects_after = match core.dispatch(CoreCommand::ListProjects).unwrap() {
        CoreResponse::Projects { projects } => projects,
        other => panic!("expected Projects, got {other:?}"),
    };
    let p_after = projects_after.iter().find(|pr| pr.id == p.id).unwrap();
    assert_eq!(p_after.project_type, None);
}

#[test]
fn inspect_project_folder_detects_tauri_and_auto_adds_project() {
    let (core, home) = test_core();
    let dir = home.paths.root().join("my-desktop-app");
    let src_tauri = dir.join("src-tauri");
    std::fs::create_dir_all(&src_tauri).unwrap();
    std::fs::write(src_tauri.join("tauri.conf.json"), "{}").unwrap();

    let inspection = match core
        .dispatch(CoreCommand::InspectProjectFolder {
            path: dir.display().to_string(),
        })
        .unwrap()
    {
        CoreResponse::ProjectFolderInspection { inspection } => inspection,
        other => panic!("expected ProjectFolderInspection, got {other:?}"),
    };
    assert_eq!(inspection.detected_type.as_deref(), Some("custom:Tauri"));

    // AddProject with project_type = None should auto-detect "custom:Tauri"
    let p = match core
        .dispatch(CoreCommand::AddProject {
            path: dir.display().to_string(),
            name: None,
            project_type: None,
        })
        .unwrap()
    {
        CoreResponse::Project { project } => project,
        other => panic!("expected Project, got {other:?}"),
    };
    assert_eq!(p.project_type.as_deref(), Some("custom:Tauri"));
}
