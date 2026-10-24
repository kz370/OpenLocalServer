use std::path::PathBuf;
use super::super::*;
use crate::error::CoreError;

#[cfg(test)]
mod store_tests {
    use super::*;

    #[test]
    fn a_v6_page_the_user_edited_is_left_alone() {
        let home = crate::test_support::isolated_home();
        DomainStore::load(&home.paths).unwrap();
        let index = home.paths.data_dir().join("home").join("index.html");
        // No marker at all, so the page is not stock however it is spelled.
        std::fs::write(&index, "<html><body>my own page</body></html>").unwrap();
        DomainStore::load(&home.paths).unwrap();
        assert!(std::fs::read_to_string(&index)
            .unwrap()
            .contains("my own page"));
    }

    #[test]
    fn missing_home_site_is_recreated_without_touching_a_custom_page() {
        let home = crate::test_support::isolated_home();
        DomainStore::load(&home.paths).unwrap();
        // Simulate an install that lost the site entry but kept its files.
        let dir = home.paths.data_dir().join("home");
        std::fs::write(dir.join("index.html"), "<html><body>mine</body></html>").unwrap();
        db::save_docs::<Domain>(&home.paths, "domains", &[]).unwrap();
        let store = DomainStore::load(&home.paths).unwrap();
        assert!(store.get(HOME_HOSTNAME).is_some());
        assert!(std::fs::read_to_string(dir.join("index.html"))
            .unwrap()
            .contains("mine"));
    }

    #[test]
    fn home_test_can_neither_be_removed_nor_renamed() {
        let home = crate::test_support::isolated_home();
        let mut store = DomainStore::load(&home.paths).unwrap();
        assert!(store.remove(HOME_HOSTNAME).is_err());
        assert!(store.get(HOME_HOSTNAME).is_some());
        let reloaded = DomainStore::load(&home.paths).unwrap();
        assert!(reloaded.get(HOME_HOSTNAME).is_some());
    }

    /// A bulk edit is partial, not all-or-nothing: one stale hostname in the selection is
    /// reported and the rest of the batch still lands. That is the whole point of the
    /// outcome — the user sees which sites were left alone and why.
    #[test]
    fn a_bulk_enable_says_which_sites_it_refused_and_still_changes_the_rest() {
        let home = crate::test_support::isolated_home();
        let mut store = DomainStore::load(&home.paths).unwrap();
        store.add(domain("a.test")).unwrap();
        store.add(domain("b.test")).unwrap();

        let out = store
            .bulk_set_enabled(
                &["a.test".into(), "gone.test".into(), HOME_HOSTNAME.into()],
                false,
            )
            .unwrap();
        assert_eq!(out.changed, 1);
        assert_eq!(out.skipped.len(), 2);
        assert!(out.skipped.iter().any(|s| s.contains("gone.test")));
        assert!(out.skipped.iter().any(|s| s.contains(HOME_HOSTNAME)));

        assert!(!store.get("a.test").unwrap().enabled);
        // The home site is what the app is reachable on, so it stays on.
        assert!(store.get(HOME_HOSTNAME).unwrap().enabled);
        // And the change survived the write, because the batch was not read-only.
        let reloaded = DomainStore::load(&home.paths).unwrap();
        assert!(!reloaded.get("a.test").unwrap().enabled);
    }

    #[test]
    fn a_bulk_server_pin_writes_every_site_and_clears_back_to_the_default() {
        let home = crate::test_support::isolated_home();
        let mut store = DomainStore::load(&home.paths).unwrap();
        store.add(domain("a.test")).unwrap();
        store.add(domain("b.test")).unwrap();

        let out = store
            .bulk_set_server(&["a.test".into(), "b.test".into()], Some("apache"))
            .unwrap();
        assert_eq!(out.changed, 2);
        assert!(out.skipped.is_empty());
        assert_eq!(
            store.get("a.test").unwrap().server.as_deref(),
            Some("apache")
        );

        store
            .bulk_set_server(&["a.test".into(), "b.test".into()], None)
            .unwrap();
        let reloaded = DomainStore::load(&home.paths).unwrap();
        assert!(reloaded.get("a.test").unwrap().server.is_none());
        assert!(reloaded.get("b.test").unwrap().server.is_none());
    }

    /// A typo in the server id must not leave half the batch pinned: the id is checked
    /// once, up front, so the whole batch is refused before anything is written.
    #[test]
    fn a_bulk_server_pin_refuses_the_whole_batch_on_an_unknown_server() {
        let home = crate::test_support::isolated_home();
        let mut store = DomainStore::load(&home.paths).unwrap();
        store.add(domain("a.test")).unwrap();

        assert!(store
            .bulk_set_server(&["a.test".into(), "b.test".into()], Some("iis"))
            .is_err());
        assert!(store.get("a.test").unwrap().server.is_none());
    }

    #[test]
    fn a_bulk_delete_leaves_the_home_site_and_names_it() {
        let home = crate::test_support::isolated_home();
        let mut store = DomainStore::load(&home.paths).unwrap();
        store.add(domain("a.test")).unwrap();
        store.add(domain("b.test")).unwrap();

        let out = store
            .bulk_remove(&["a.test".into(), HOME_HOSTNAME.into(), "b.test".into()])
            .unwrap();
        assert_eq!(out.changed, 2);
        assert_eq!(out.skipped.len(), 1);
        assert!(out.skipped[0].contains(HOME_HOSTNAME));
        assert!(store.get("a.test").is_none());
        assert!(store.get(HOME_HOSTNAME).is_some());
    }

    fn web_config(default_server: &str) -> crate::web::WebConfig {
        crate::web::WebConfig {
            default_server: default_server.into(),
            servers: BTreeMap::new(),
            php_workers: 3,
            dns_port: 53,
        }
    }

    #[test]
    fn domain_server_none_means_default() {
        let cfg = web_config("nginx");
        assert_eq!(resolved_server(&domain("a.test"), &cfg), "nginx");
        let mut pinned = domain("b.test");
        pinned.server = Some("apache".into());
        assert_eq!(resolved_server(&pinned, &cfg), "apache");
    }

    #[test]
    fn domain_server_rejects_unknown_id_and_treats_blank_as_default() {
        assert!(validate_domain_server(Some("iis")).is_err());
        assert_eq!(validate_domain_server(None).unwrap(), None);
        assert_eq!(validate_domain_server(Some("")).unwrap(), None);
        assert_eq!(
            validate_domain_server(Some(" caddy ")).unwrap(),
            Some("caddy".into())
        );
    }

    #[test]
    fn an_unknown_override_survives_load_but_resolves_to_the_default() {
        // A site saved against a server that is no longer known must not be dropped:
        // it still resolves, it just falls back to the default server.
        let home = crate::test_support::isolated_home();
        let mut store = DomainStore::load(&home.paths).unwrap();
        let mut d = domain("legacy.test");
        d.server = Some("iis".into());
        store.domains.push(d);
        store.persist().unwrap();

        let reloaded = DomainStore::load(&home.paths).unwrap();
        let saved = reloaded.get("legacy.test").unwrap();
        assert_eq!(saved.server.as_deref(), Some("iis"));
        assert_eq!(resolved_server(&saved, &web_config("nginx")), "nginx");
    }

    #[test]
    fn add_rejects_an_unknown_server_instead_of_silently_dropping_it() {
        let home = crate::test_support::isolated_home();
        let mut store = DomainStore::load(&home.paths).unwrap();
        let mut d = domain("shop.test");
        d.server = Some("iis".into());
        assert!(store.add(d.clone()).is_err());
        d.server = Some("apache".into());
        assert_eq!(store.add(d).unwrap().server.as_deref(), Some("apache"));
    }

    #[test]
    fn a_path_prefix_is_normalized_trimmed_and_survives_a_reload() {
        let home = crate::test_support::isolated_home();
        let mut store = DomainStore::load(&home.paths).unwrap();
        let mut d = domain("shop.test");
        d.path_prefix = Some("  shop_2  ".into());
        assert_eq!(
            store.add(d.clone()).unwrap().path_prefix.as_deref(),
            Some("shop_2")
        );
        // A blank value means "no path route" and is stored as such, so the form and
        // the config agree instead of the file carrying an empty prefix.
        let mut blank = domain("api.test");
        blank.path_prefix = Some("   ".into());
        assert_eq!(store.add(blank).unwrap().path_prefix, None);
        assert_eq!(
            store.get("shop.test").unwrap().path_prefix.as_deref(),
            Some("shop_2")
        );
    }

    #[test]
    fn a_path_prefix_that_could_escape_or_change_the_request_is_refused() {
        assert!(validate_path_prefix(Some("")).unwrap().is_none());
        assert!(validate_path_prefix(None).unwrap().is_none());
        assert!(validate_path_prefix(Some("shop")).is_ok());
        assert!(validate_path_prefix(Some("shop-2_app")).is_ok());
        // A `/` would make it a path tree, and the rest would land in a generated config
        // as a URL path where they could mean something other than a folder name.
        assert!(validate_path_prefix(Some("a/b")).is_err());
        assert!(validate_path_prefix(Some("a\\b")).is_err());
        assert!(validate_path_prefix(Some("..")).is_err());
        assert!(validate_path_prefix(Some(".")).is_err());
        assert!(validate_path_prefix(Some("Shop")).is_err());
        assert!(validate_path_prefix(Some("a b")).is_err());
        assert!(validate_path_prefix(Some("a;b")).is_err());
    }

    #[test]
    fn two_sites_cannot_claim_the_same_localhost_path() {
        // They would answer on one path and only one could win, so the second is refused
        // with the name of the site already holding it.
        let home = crate::test_support::isolated_home();
        let mut store = DomainStore::load(&home.paths).unwrap();
        let mut first = domain("shop.test");
        first.path_prefix = Some("site".into());
        store.add(first).unwrap();

        let mut second = domain("api.test");
        second.path_prefix = Some("site".into());
        let err = store.add(second).unwrap_err().to_string();
        assert!(err.contains("localhost/site"), "{err}");
        assert!(err.contains("shop.test"), "{err}");

        // Renaming the first site frees the path again.
        let mut renamed = store.get("shop.test").unwrap();
        renamed.path_prefix = None;
        store.update(renamed).unwrap();
        let mut third = domain("api.test");
        third.path_prefix = Some("site".into());
        assert!(store.add(third).is_ok());
    }

    #[test]
    fn a_site_may_keep_its_own_path_when_edited() {
        // `check_path_prefix_taken` skips the site being edited, so re-saving a site
        // without changing anything must not report a conflict with itself.
        let home = crate::test_support::isolated_home();
        let mut store = DomainStore::load(&home.paths).unwrap();
        let mut d = domain("shop.test");
        d.path_prefix = Some("site".into());
        let saved = store.add(d).unwrap();
        let mut edited = saved.clone();
        edited.https = false;
        assert!(store.update(edited).is_ok());
    }

    #[test]
    fn update_persists_a_pinned_server_across_a_reload() {
        // `add` was covered, `update` was not — and `update` is the path the site
        // settings dialog uses, so a pin set from the UI had no round-trip proof.
        let home = crate::test_support::isolated_home();
        let mut store = DomainStore::load(&home.paths).unwrap();
        let mut d = domain("shop.test");
        store.add(d.clone()).unwrap();

        d.server = Some("caddy".into());
        assert_eq!(store.update(d).unwrap().server.as_deref(), Some("caddy"));

        let reloaded = DomainStore::load(&home.paths).unwrap();
        assert_eq!(
            reloaded.get("shop.test").unwrap().server.as_deref(),
            Some("caddy")
        );
        assert_eq!(
            resolved_server(&reloaded.get("shop.test").unwrap(), &web_config("nginx")),
            "caddy"
        );

        // Back to automatic: `None` must survive the write too, not just the pin.
        let mut auto = reloaded.get("shop.test").unwrap().clone();
        auto.server = None;
        assert!(store.update(auto).unwrap().server.is_none());
        assert!(DomainStore::load(&home.paths)
            .unwrap()
            .get("shop.test")
            .unwrap()
            .server
            .is_none());
    }

    #[test]
    fn update_rejects_an_unknown_server_instead_of_clearing_the_pin() {
        let home = crate::test_support::isolated_home();
        let mut store = DomainStore::load(&home.paths).unwrap();
        let mut d = domain("shop.test");
        d.server = Some("apache".into());
        store.add(d.clone()).unwrap();

        d.server = Some("iis".into());
        assert!(store.update(d.clone()).is_err());
        // The failed write must leave the stored pin alone, not reset it to automatic.
        assert_eq!(
            DomainStore::load(&home.paths)
                .unwrap()
                .get("shop.test")
                .unwrap()
                .server
                .as_deref(),
            Some("apache")
        );
    }

    #[test]
    fn ensure_site_dir_and_welcome_file_creates_static_welcome_page() {
        let home = crate::test_support::isolated_home();
        let site_dir = home.paths.data_dir().join("new-static-site");
        let mut d = domain("my-static.local");
        d.root = site_dir.display().to_string();
        d.kind = SiteKind::Static;

        assert!(!site_dir.exists());
        ensure_site_dir_and_welcome_file(&d).unwrap();

        assert!(site_dir.is_dir());
        let index_file = site_dir.join("index.html");
        assert!(index_file.is_file());
        let content = std::fs::read_to_string(&index_file).unwrap();
        assert!(content.contains("my-static.local"));
        assert!(content.contains("Static Site Ready"));
        assert!(content.contains("Open Local Server"));
    }

    #[test]
    fn ensure_site_dir_and_welcome_file_creates_php_welcome_page() {
        let home = crate::test_support::isolated_home();
        let site_dir = home.paths.data_dir().join("new-php-site");
        let mut d = domain("my-php.local");
        d.root = site_dir.display().to_string();
        d.kind = SiteKind::Php { version: None };

        assert!(!site_dir.exists());
        ensure_site_dir_and_welcome_file(&d).unwrap();

        assert!(site_dir.is_dir());
        let index_file = site_dir.join("index.php");
        assert!(index_file.is_file());
        let content = std::fs::read_to_string(&index_file).unwrap();
        assert!(content.contains("my-php.local"));
        assert!(content.contains("PHP Site Ready"));
        assert!(content.contains("phpinfo()"));
    }

    #[test]
    fn ensure_site_dir_and_welcome_file_ignores_proxy_site() {
        let home = crate::test_support::isolated_home();
        let site_dir = home.paths.data_dir().join("proxy-site-dir");
        let mut d = domain("my-proxy.local");
        d.root = site_dir.display().to_string();
        d.kind = SiteKind::Proxy {
            upstream_port: 3000,
            upstream_host: None,
            upstream_https: false,
        };

        ensure_site_dir_and_welcome_file(&d).unwrap();
        assert!(!site_dir.exists());
    }

    #[test]
    fn ensure_site_dir_does_not_overwrite_existing_index_file() {
        let home = crate::test_support::isolated_home();
        let site_dir = home.paths.data_dir().join("existing-site");
        std::fs::create_dir_all(&site_dir).unwrap();
        let index_file = site_dir.join("index.html");
        std::fs::write(&index_file, "<h1>custom user content</h1>").unwrap();

        let mut d = domain("existing.local");
        d.root = site_dir.display().to_string();
        d.kind = SiteKind::Static;

        ensure_site_dir_and_welcome_file(&d).unwrap();
        let content = std::fs::read_to_string(&index_file).unwrap();
        assert_eq!(content, "<h1>custom user content</h1>");
    }
}
