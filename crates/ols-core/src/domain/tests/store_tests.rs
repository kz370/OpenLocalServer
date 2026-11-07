use super::*;
use crate::db;
use std::collections::BTreeMap;

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

#[test]
fn two_proxy_sites_cannot_claim_the_same_local_port() {
    let home = crate::test_support::isolated_home();
    let mut store = DomainStore::load(&home.paths).unwrap();

    let mut first = domain("node-app.test");
    first.kind = SiteKind::Proxy {
        upstream_port: 3000,
        upstream_host: None,
        upstream_https: false,
    };
    store.add(first).unwrap();

    let mut second = domain("second-app.test");
    second.kind = SiteKind::Proxy {
        upstream_port: 3000,
        upstream_host: Some("127.0.0.1".into()),
        upstream_https: false,
    };
    let err = store.add(second).unwrap_err().to_string();
    assert!(err.contains("Port 3000"), "{err}");
    assert!(err.contains("node-app.test"), "{err}");
}

#[test]
fn a_proxy_site_may_keep_its_own_port_when_edited() {
    let home = crate::test_support::isolated_home();
    let mut store = DomainStore::load(&home.paths).unwrap();

    let mut d = domain("node-app.test");
    d.kind = SiteKind::Proxy {
        upstream_port: 5000,
        upstream_host: None,
        upstream_https: false,
    };
    let saved = store.add(d).unwrap();

    let mut edited = saved;
    edited.https = false;
    assert!(store.update(edited).is_ok());
}

#[test]
fn proxy_sites_on_different_remote_hosts_may_share_a_port() {
    let home = crate::test_support::isolated_home();
    let mut store = DomainStore::load(&home.paths).unwrap();

    let mut first = domain("remote1.test");
    first.kind = SiteKind::Proxy {
        upstream_port: 8080,
        upstream_host: Some("192.168.1.10".into()),
        upstream_https: false,
    };
    store.add(first).unwrap();

    let mut second = domain("remote2.test");
    second.kind = SiteKind::Proxy {
        upstream_port: 8080,
        upstream_host: Some("192.168.1.20".into()),
        upstream_https: false,
    };
    assert!(store.add(second).is_ok());
}

#[test]
fn proxy_sites_cannot_use_reserved_app_ports() {
    let home = crate::test_support::isolated_home();
    let mut store = DomainStore::load(&home.paths).unwrap();

    let reserved = [
        (80, "Web server HTTP"),
        (443, "Web server HTTPS"),
        (3306, "MariaDB / MySQL"),
        (5432, "PostgreSQL"),
        (6379, "Redis"),
        (8025, "Mailpit"),
    ];

    for (port, label) in reserved {
        let mut d = domain(&format!("app-{port}.test"));
        d.kind = SiteKind::Proxy {
            upstream_port: port,
            upstream_host: None,
            upstream_https: false,
        };
        let err = store.add(d).unwrap_err().to_string();
        assert!(
            err.contains(&format!("Port {port} is reserved for {label}")),
            "expected reserved port error for {port}, got: {err}"
        );
    }
}

// ---------------------------------------------------------------- DDNS tokens (keyring)

fn ddns_site(host: &str, token: Option<&str>) -> Domain {
    Domain {
        root: "C:/sites/ddns".into(),
        ddns_provider: Some("duckdns".into()),
        ddns_token: token.map(str::to_string),
        ..super::domain(host)
    }
}

/// The token as written in the database row, not as the store shows it.
fn token_in_row(paths: &crate::paths::AppPaths, host: &str) -> Option<String> {
    db::load_docs::<serde_json::Value>(paths, "domains")
        .unwrap()
        .into_iter()
        .find(|v| v["hostname"] == host)
        .expect("row exists")["ddns_token"]
        .as_str()
        .map(str::to_string)
}

fn keyring_token(paths: &crate::paths::AppPaths, host: &str) -> Option<String> {
    let ns = crate::domain::ddns_keyring::namespace(paths).unwrap();
    crate::secrets::get_secret(&crate::domain::ddns_keyring::key(&ns, host)).unwrap()
}

fn clean_keyring(paths: &crate::paths::AppPaths, hosts: &[&str]) {
    let ns = crate::domain::ddns_keyring::namespace(paths).unwrap();
    for h in hosts {
        let _ = crate::secrets::delete_secret(&crate::domain::ddns_keyring::key(&ns, h));
    }
}

#[test]
fn an_existing_install_moves_plaintext_ddns_tokens_to_the_keyring() {
    let home = crate::test_support::isolated_home();
    DomainStore::load(&home.paths).unwrap();
    // A row as an older version wrote it: the token in plain text.
    let legacy = ddns_site("legacy-ddns.test", Some("legacy-token-0001"));
    let mut rows: Vec<(String, serde_json::Value)> =
        db::load_docs::<serde_json::Value>(&home.paths, "domains")
            .unwrap()
            .into_iter()
            .map(|v| (v["hostname"].as_str().unwrap().to_string(), v))
            .collect();
    rows.push((
        legacy.hostname.clone(),
        serde_json::to_value(&legacy).unwrap(),
    ));
    db::save_values(&home.paths, "domains", &rows).unwrap();
    assert_eq!(
        token_in_row(&home.paths, "legacy-ddns.test").as_deref(),
        Some("legacy-token-0001")
    );

    let store = DomainStore::load(&home.paths).unwrap();
    assert_eq!(
        store.get("legacy-ddns.test").unwrap().ddns_token.as_deref(),
        Some("legacy-token-0001"),
        "DDNS keeps working with the same token"
    );
    assert_eq!(
        token_in_row(&home.paths, "legacy-ddns.test"),
        None,
        "the plain-text copy is gone from the database"
    );
    assert_eq!(
        keyring_token(&home.paths, "legacy-ddns.test").as_deref(),
        Some("legacy-token-0001")
    );
    // The next start reads it back from the keyring.
    let again = DomainStore::load(&home.paths).unwrap();
    assert_eq!(
        again.get("legacy-ddns.test").unwrap().ddns_token.as_deref(),
        Some("legacy-token-0001")
    );
    clean_keyring(&home.paths, &["legacy-ddns.test"]);
}

#[test]
fn a_fresh_install_never_writes_a_ddns_token_into_the_database() {
    let home = crate::test_support::isolated_home();
    let mut store = DomainStore::load(&home.paths).unwrap();
    store
        .add(ddns_site("fresh-ddns.test", Some("fresh-token-0002")))
        .unwrap();
    assert_eq!(token_in_row(&home.paths, "fresh-ddns.test"), None);
    assert_eq!(
        keyring_token(&home.paths, "fresh-ddns.test").as_deref(),
        Some("fresh-token-0002")
    );
    // Changing the token replaces the keyring value.
    store
        .update(ddns_site("fresh-ddns.test", Some("fresh-token-0003")))
        .unwrap();
    assert_eq!(
        keyring_token(&home.paths, "fresh-ddns.test").as_deref(),
        Some("fresh-token-0003")
    );
    // Clearing it removes it; removing the site leaves nothing behind.
    store.update(ddns_site("fresh-ddns.test", None)).unwrap();
    assert_eq!(keyring_token(&home.paths, "fresh-ddns.test"), None);
    store
        .update(ddns_site("fresh-ddns.test", Some("fresh-token-0004")))
        .unwrap();
    store.remove("fresh-ddns.test").unwrap();
    assert_eq!(keyring_token(&home.paths, "fresh-ddns.test"), None);
    // The install's namespace is random (so two installs never share a key) and stable.
    let ns = crate::domain::ddns_keyring::namespace(&home.paths).unwrap();
    assert_eq!(ns.len(), 32);
    assert!(ns.chars().all(|c| c.is_ascii_hexdigit()));
    assert_eq!(
        crate::domain::ddns_keyring::namespace(&home.paths).as_deref(),
        Some(ns.as_str())
    );
    clean_keyring(&home.paths, &["fresh-ddns.test"]);
}

#[test]
fn a_credential_store_that_refuses_keeps_the_token_in_the_database() {
    let home = crate::test_support::isolated_home();
    let mut store = DomainStore::load(&home.paths).unwrap();
    crate::domain::ddns_keyring::REFUSE_WRITES.with(|r| r.set(true));
    store
        .add(ddns_site("stuck-ddns.test", Some("stuck-token-0005")))
        .unwrap();
    crate::domain::ddns_keyring::REFUSE_WRITES.with(|r| r.set(false));
    assert_eq!(
        token_in_row(&home.paths, "stuck-ddns.test").as_deref(),
        Some("stuck-token-0005"),
        "never lost: it stays where it was"
    );
    assert_eq!(keyring_token(&home.paths, "stuck-ddns.test"), None);
    // Once the store works again, the next start finishes the move.
    let store = DomainStore::load(&home.paths).unwrap();
    assert_eq!(
        store.get("stuck-ddns.test").unwrap().ddns_token.as_deref(),
        Some("stuck-token-0005")
    );
    assert_eq!(token_in_row(&home.paths, "stuck-ddns.test"), None);
    assert_eq!(
        keyring_token(&home.paths, "stuck-ddns.test").as_deref(),
        Some("stuck-token-0005")
    );
    clean_keyring(&home.paths, &["stuck-ddns.test"]);
}

fn secret_in_keyring(paths: &crate::paths::AppPaths, host: &str) -> Option<String> {
    let ns = crate::domain::ddns_keyring::namespace(paths).unwrap();
    crate::secrets::get_secret(&crate::domain::ddns_keyring::secret_key(&ns, host)).unwrap()
}

fn zone_in_row(paths: &crate::paths::AppPaths, host: &str) -> Option<String> {
    db::load_docs::<serde_json::Value>(paths, "domains")
        .unwrap()
        .into_iter()
        .find(|v| v["hostname"] == host)
        .expect("row exists")["ddns_zone_id"]
        .as_str()
        .map(str::to_string)
}

#[test]
fn a_second_ddns_credential_moves_too_but_a_cloudflare_zone_id_stays() {
    let home = crate::test_support::isolated_home();
    let mut store = DomainStore::load(&home.paths).unwrap();
    // GoDaddy: the second field is the API secret.
    store
        .add(Domain {
            ddns_provider: Some("godaddy".into()),
            ddns_token: Some("gd-key-000001".into()),
            ddns_zone_id: Some("gd-secret-000001".into()),
            ..ddns_site("gd-ddns.test", None)
        })
        .unwrap();
    // Cloudflare: the second field is a zone ID, not a secret.
    store
        .add(Domain {
            ddns_provider: Some("cloudflare".into()),
            ddns_token: Some("cf-token-000001".into()),
            ddns_zone_id: Some("cf-zone-0000001".into()),
            ..ddns_site("cf-ddns.test", None)
        })
        .unwrap();
    assert_eq!(zone_in_row(&home.paths, "gd-ddns.test"), None);
    assert_eq!(
        secret_in_keyring(&home.paths, "gd-ddns.test").as_deref(),
        Some("gd-secret-000001")
    );
    assert_eq!(
        zone_in_row(&home.paths, "cf-ddns.test").as_deref(),
        Some("cf-zone-0000001")
    );
    assert_eq!(secret_in_keyring(&home.paths, "cf-ddns.test"), None);
    assert_eq!(token_in_row(&home.paths, "cf-ddns.test"), None);

    let again = DomainStore::load(&home.paths).unwrap();
    let gd = again.get("gd-ddns.test").unwrap();
    assert_eq!(
        (gd.ddns_token.as_deref(), gd.ddns_zone_id.as_deref()),
        (Some("gd-key-000001"), Some("gd-secret-000001"))
    );
    assert_eq!(
        again.get("cf-ddns.test").unwrap().ddns_zone_id.as_deref(),
        Some("cf-zone-0000001")
    );

    // A site that switches to Cloudflare keeps its second field in the row from then on,
    // and the old secret leaves the keyring.
    store
        .update(Domain {
            ddns_provider: Some("cloudflare".into()),
            ddns_token: Some("gd-key-000001".into()),
            ddns_zone_id: Some("now-a-zone-id".into()),
            ..ddns_site("gd-ddns.test", None)
        })
        .unwrap();
    assert_eq!(secret_in_keyring(&home.paths, "gd-ddns.test"), None);
    assert_eq!(
        zone_in_row(&home.paths, "gd-ddns.test").as_deref(),
        Some("now-a-zone-id")
    );

    // Exported copies never carry the credentials.
    let out = crate::domain::ddns_keyring::without_credentials(Domain {
        ddns_provider: Some("porkbun".into()),
        ddns_token: Some("pk".into()),
        ddns_zone_id: Some("sk".into()),
        ..ddns_site("x.test", None)
    });
    assert_eq!((out.ddns_token, out.ddns_zone_id), (None, None));
    let cf = crate::domain::ddns_keyring::without_credentials(Domain {
        ddns_provider: Some("cloudflare".into()),
        ddns_token: Some("t".into()),
        ddns_zone_id: Some("zone".into()),
        ..ddns_site("y.test", None)
    });
    assert_eq!(
        (cf.ddns_token, cf.ddns_zone_id.as_deref()),
        (None, Some("zone"))
    );
    store.remove("gd-ddns.test").unwrap();
    store.remove("cf-ddns.test").unwrap();
    assert_eq!(keyring_token(&home.paths, "gd-ddns.test"), None);
    assert_eq!(keyring_token(&home.paths, "cf-ddns.test"), None);
}
