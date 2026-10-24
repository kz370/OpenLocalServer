use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use super::*;
use crate::db;
use crate::domain::home::*;
use crate::error::CoreError;
use crate::paths::AppPaths;

    #[test]
    fn hostname_validation_accepts_normal_and_rejects_dangerous_names() {
        assert!(validate_hostname("shop.test").is_ok());
        assert!(validate_hostname("api.shop.test").is_ok());
        assert!(validate_hostname("localhost").is_err());
        assert!(validate_hostname("shop.test; rm -rf").is_err());
        assert!(validate_hostname("sh op.test").is_err());
        assert!(validate_hostname("-a.test").is_err());
        assert!(
            validate_hostname("A.test").is_err(),
            "must be normalised to lowercase before validating"
        );
        assert!(validate_hostname("a..test").is_err());
    }

    #[test]
    fn templates_expand_with_a_slugified_project_name() {
        assert_eq!(
            apply_template("{project}.test", "My Shop_2"),
            "my-shop-2.test"
        );
        assert_eq!(
            apply_template("api.{project}.test", "shop"),
            "api.shop.test"
        );
    }

    #[test]
    fn add_persists_and_rejects_duplicates_and_wildcard_overlaps() {
        let home = crate::test_support::isolated_home();
        let mut store = DomainStore::load(&home.paths).unwrap();
        store.add(domain("shop.test")).unwrap();
        assert!(store.add(domain("shop.test")).is_err());

        let mut wild = domain("acme.test");
        wild.wildcard = true;
        store.add(wild).unwrap();
        assert!(
            store.add(domain("tenant.acme.test")).is_err(),
            "covered by *.acme.test"
        );

        store.add(domain("api.shop.test")).unwrap();

        // Survives a reload (plus the seeded home.test).
        let reloaded = DomainStore::load(&home.paths).unwrap();
        assert_eq!(reloaded.list().len(), 4);
    }

    #[test]
    fn a_wildcard_may_not_shadow_an_existing_subdomain() {
        let home = crate::test_support::isolated_home();
        let mut store = DomainStore::load(&home.paths).unwrap();
        store.add(domain("api.shop.test")).unwrap();
        let mut wild = domain("shop.test");
        wild.wildcard = true;
        assert!(store.add(wild).is_err());
    }

    #[test]
    fn fresh_install_seeds_home_test_once_with_a_welcome_page() {
        let home = crate::test_support::isolated_home();
        let store = DomainStore::load(&home.paths).unwrap();
        let seeded = store.get(HOME_HOSTNAME).expect("home.test seeded");
        assert_eq!(seeded.kind, SiteKind::Static);
        assert!(seeded.enabled && seeded.https);
        let page = home.paths.data_dir().join("home").join("index.html");
        assert!(page.is_file());
        let html = std::fs::read_to_string(&page).unwrap();
        assert!(html.contains("OLS") && html.contains("Local Domains"));
        assert!(home
            .paths
            .data_dir()
            .join("home")
            .join("logo.svg")
            .is_file());
        // Both colourways, because the page picks one from its own theme toggle.
        assert!(home
            .paths
            .data_dir()
            .join("home")
            .join("logo-dark.svg")
            .is_file());

        // Second load: no duplicate, page kept.
        let again = DomainStore::load(&home.paths).unwrap();
        assert_eq!(
            again
                .list()
                .iter()
                .filter(|d| d.hostname == HOME_HOSTNAME)
                .count(),
            1
        );
    }

    #[test]
    fn legacy_home_test_is_renamed_on_load() {
        let home = crate::test_support::isolated_home();
        let mut legacy = home_domain(&home.paths.data_dir().join("home"));
        legacy.hostname = LEGACY_HOME_HOSTNAME.into();
        db::save_docs(
            &home.paths,
            "domains",
            &[(legacy.hostname.clone(), &legacy)],
        )
        .unwrap();
        let store = DomainStore::load(&home.paths).unwrap();
        assert!(store.get(HOME_HOSTNAME).is_some());
        assert!(store.get(LEGACY_HOME_HOSTNAME).is_none());
    }

    #[test]
    fn a_renamed_install_folder_reanchors_the_home_site_root() {
        let home = crate::test_support::isolated_home();
        DomainStore::load(&home.paths).unwrap();
        // The install folder moved: the stored absolute root no longer exists, and the
        // files now live under a different root.
        let moved = home.paths.root().join("moved");
        std::fs::create_dir_all(&moved).unwrap();
        std::fs::rename(home.paths.data_dir().join("home"), moved.join("home")).unwrap();
        let store = DomainStore::load(&home.paths).unwrap();
        let site = store.get(HOME_HOSTNAME).unwrap();
        assert_eq!(
            site.root,
            home.paths.data_dir().join("home").display().to_string()
        );
        assert!(
            std::path::Path::new(&site.root).is_dir(),
            "the re-anchored root must exist, or diagnostics keeps reporting it missing"
        );
        assert!(home
            .paths
            .data_dir()
            .join("home")
            .join("index.html")
            .is_file());
    }

    #[test]
    fn a_site_inside_the_sites_folder_is_stored_relative_to_the_app() {
        let home = crate::test_support::isolated_home();
        let mut store = DomainStore::load(&home.paths).unwrap();
        let dir = home.paths.sites_dir().join("shop");
        std::fs::create_dir_all(&dir).unwrap();
        let mut site = home_domain(&dir);
        site.hostname = "shop.test".into();
        store.add(site).unwrap();

        // On disk it is relative: move the app and the site still resolves.
        let rows: Vec<Domain> = db::load_docs(&home.paths, "domains").unwrap();
        let shop = rows.iter().find(|d| d.hostname == "shop.test").unwrap();
        assert_eq!(shop.root, "sites/shop");
        // In memory every consumer still sees a full path.
        assert_eq!(
            store.get("shop.test").unwrap().root,
            dir.display().to_string()
        );
    }

    #[test]
    fn a_site_outside_the_app_keeps_its_own_absolute_path() {
        let home = crate::test_support::isolated_home();
        let mut store = DomainStore::load(&home.paths).unwrap();
        let outside = std::env::temp_dir().join(format!("ols-site-{}", std::process::id()));
        std::fs::create_dir_all(&outside).unwrap();
        let mut site = home_domain(&outside);
        site.hostname = "outside.test".into();
        store.add(site).unwrap();
        let rows: Vec<Domain> = db::load_docs(&home.paths, "domains").unwrap();
        let stored = rows.iter().find(|d| d.hostname == "outside.test").unwrap();
        assert_eq!(stored.root, outside.display().to_string());
        let _ = std::fs::remove_dir_all(&outside);
    }

    #[test]
    fn an_app_relative_site_survives_the_app_moving_to_a_new_root() {
        let home = crate::test_support::isolated_home();
        {
            let mut store = DomainStore::load(&home.paths).unwrap();
            let dir = home.paths.sites_dir().join("shop");
            std::fs::create_dir_all(&dir).unwrap();
            let mut site = home_domain(&dir);
            site.hostname = "shop.test".into();
            store.add(site).unwrap();
        }
        // The install was moved: the database and the site files come along, the old
        // paths stop existing. `resolve_at` is a second AppPaths over a different root,
        // which is exactly that — no env var, so the isolation lock stays with the first
        // home.
        let moved_root = home.paths.root().with_extension("moved");
        let moved = AppPaths::resolve_at(&moved_root);
        moved.ensure_dirs().unwrap();
        std::fs::copy(home.paths.db_file(), moved.db_file()).unwrap();
        std::fs::create_dir_all(moved.sites_dir().join("shop")).unwrap();
        std::fs::rename(
            home.paths.sites_dir().join("shop"),
            moved.sites_dir().join("shop"),
        )
        .unwrap();

        let store = DomainStore::load(&moved).unwrap();
        assert_eq!(
            store.get("shop.test").unwrap().root,
            moved.sites_dir().join("shop").display().to_string()
        );
        assert!(Path::new(&store.get("shop.test").unwrap().root).is_dir());
    }

    #[test]
    fn stock_old_welcome_page_is_refreshed_but_hand_edits_survive() {
        let home = crate::test_support::isolated_home();
        DomainStore::load(&home.paths).unwrap();
        let index = home.paths.data_dir().join("home").join("index.html");
        // v1 stock page (carries a legacy marker) → refreshed to current.
        std::fs::write(
            &index,
            "<html><body style='max-width: 720px'>old</body></html>",
        )
        .unwrap();
        DomainStore::load(&home.paths).unwrap();
        let html = std::fs::read_to_string(&index).unwrap();
        assert!(html.contains(HOME_VERSION_MARKER));
        // Hand-edited page (no known marker) → kept.
        std::fs::write(&index, "<html><body>mine, do not touch</body></html>").unwrap();
        DomainStore::load(&home.paths).unwrap();
        assert!(std::fs::read_to_string(&index)
            .unwrap()
            .contains("do not touch"));
    }

    #[test]
    fn a_repaint_reaches_a_stock_welcome_page_but_not_a_hand_edited_one() {
        // The logo is a generated asset seeded from ui/public/favicon*.svg, so a
        // repaint has to overwrite the copy an install already has. Otherwise the
        // old mark survives forever on every existing install while a fresh one
        // gets the new mark, and nothing reports the difference. The guard is the
        // stock page marker: once the page is edited, the logo is the user's.
        let home = crate::test_support::isolated_home();
        DomainStore::load(&home.paths).unwrap();
        let dir = home.paths.data_dir().join("home");
        let index = dir.join("index.html");
        let logo = dir.join("logo.svg");

        // Stand in for the previous artwork: a stock page whose logo is not the
        // one this build ships.
        std::fs::write(&logo, b"<svg>previous mark</svg>").unwrap();
        std::fs::write(&index, HOME_PAGE).unwrap();
        DomainStore::load(&home.paths).unwrap();
        assert_eq!(
            std::fs::read(&logo).unwrap(),
            LOGO_SVG,
            "a stock page must pick up the current mark"
        );

        // Same stale logo, but the page was edited: the mark is left alone.
        std::fs::write(&logo, b"<svg>my mark</svg>").unwrap();
        std::fs::write(&index, "<html><body>mine, do not touch</body></html>").unwrap();
        DomainStore::load(&home.paths).unwrap();
        assert_eq!(std::fs::read(&logo).unwrap(), b"<svg>my mark</svg>");
    }

    #[test]
    fn a_pre_rebrand_welcome_page_is_refreshed_to_the_current_page() {
        // The page moved to v8, so an install still carrying the stock v6 page must
        // see the current one. The refresh only fires on a recognised marker, so
        // without `<!-- home v6 -->` in HOME_LEGACY_MARKERS this install would keep
        // the old page forever and nothing would say so.
        let home = crate::test_support::isolated_home();
        DomainStore::load(&home.paths).unwrap();
        let index = home.paths.data_dir().join("home").join("index.html");
        std::fs::write(
            &index,
            "<html><body><!-- home v6 --><h1>Open Local Server</h1></body></html>",
        )
        .unwrap();
        DomainStore::load(&home.paths).unwrap();
        let html = std::fs::read_to_string(&index).unwrap();
        assert!(html.contains(HOME_VERSION_MARKER));
        assert!(html.contains("OLS"));
    }

    #[test]
    fn a_v7_page_is_refreshed_so_the_tab_title_and_icon_reach_existing_installs() {
        // v7 shipped the acronym tab title and no icon at all. Without
        // `<!-- home v7 -->` in HOME_LEGACY_MARKERS, every install that already
        // opened the welcome page would keep that tab title forever.
        let home = crate::test_support::isolated_home();
        DomainStore::load(&home.paths).unwrap();
        let index = home.paths.data_dir().join("home").join("index.html");
        std::fs::write(
            &index,
            "<html><head><title>OLS — local development, simplified</title></head>\
             <body><!-- home v7 --></body></html>",
        )
        .unwrap();
        DomainStore::load(&home.paths).unwrap();
        let html = std::fs::read_to_string(&index).unwrap();
        assert!(html.contains(HOME_VERSION_MARKER));
        assert!(html.contains("<title>Open Local Server — local development, simplified</title>"));
    }

    #[test]
    fn a_v8_page_is_refreshed_so_the_expanded_brand_reaches_existing_installs() {
        // v8 fixed the tab title and left the header and footer reading a bare "OLS".
        // Without `<!-- home v8 -->` in HOME_LEGACY_MARKERS every install that already
        // opened the welcome page would keep that wordmark forever.
        let home = crate::test_support::isolated_home();
        DomainStore::load(&home.paths).unwrap();
        let index = home.paths.data_dir().join("home").join("index.html");
        std::fs::write(
            &index,
            "<html><body><!-- home v8 --><div class=\"brand\"><img src=\"logo.svg\" alt=\"OLS logo\">OLS</div></body></html>",
        )
        .unwrap();
        DomainStore::load(&home.paths).unwrap();
        let html = std::fs::read_to_string(&index).unwrap();
        assert!(html.contains(HOME_VERSION_MARKER));
        assert!(html.contains(r#"alt="OLS logo">Open Local Server</div>"#));
    }

    #[test]
    fn the_shipped_page_carries_a_themed_tab_icon() {
        // The page paints its own theme, so a `media`-scoped icon alone would go
        // stale the moment the toggle overrides the OS setting. Both the pre-paint
        // script and the toggle must drive the same `#favicon` href.
        let home = crate::test_support::isolated_home();
        DomainStore::load(&home.paths).unwrap();
        let html =
            std::fs::read_to_string(home.paths.data_dir().join("home").join("index.html")).unwrap();
        assert!(
            html.contains(r#"<link id="favicon" rel="icon" type="image/svg+xml" href="logo.svg">"#)
        );
        let pre_paint = html
            .find("document.getElementById('favicon').setAttribute('href'")
            .expect("pre-paint script");
        let toggle = html
            .find("if (fav) fav.setAttribute('href', src);")
            .expect("toggle");
        assert!(pre_paint < toggle, "pre-paint script must come first");
    }
