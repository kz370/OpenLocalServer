//! WordPress projects: detection, and a one-time admin sign-in that needs no password.
//!
//! The sign-in is a *bridge file*, not a stored credential. Clicking "WP Admin" writes one
//! small must-use plugin into `wp-content/mu-plugins`, carrying a random 128-bit token and a
//! short expiry, and hands the browser a URL that contains that token. The plugin signs the
//! chosen administrator in through WordPress' own `wp_set_auth_cookie()`, deletes itself,
//! and the token stops working immediately.
//!
//! What this module deliberately never does:
//!
//! * It never reads, logs, returns or stores a WordPress password or hash. Nothing here
//!   opens `wp-config.php`, and the generated PHP never touches `user_pass`.
//! * It never writes a credential that outlives the click. The file is the whole
//!   capability, it is removed the moment it is used, expired or revoked, and it only ever
//!   answers on the loopback interface.
//!
//! A sign-in bridge left behind by a crash is swept on the next listing (§40's project
//! list is where every WordPress project is visited anyway), and an unused bridge can be
//! revoked by hand from the same menu.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::CoreError;

/// File name prefix of a sign-in bridge. Also the marker used to find and remove them.
pub const BRIDGE_PREFIX: &str = "ols-wp-login-";

/// How long a bridge stays usable. Long enough to click a link and pick an account, short
/// enough that an abandoned link is dead minutes later.
pub const DEFAULT_TTL_SECS: u64 = 300;

/// Upper bound on a caller-supplied TTL. Nobody needs a sign-in link that lives for hours.
pub const MAX_TTL_SECS: u64 = 900;

/// Slack added to the expiry before a sweep reclaims the file, so a link clicked a second
/// before it expires is not pulled out from under the browser.
const SWEEP_GRACE_SECS: u64 = 60;

/// A registered project that is a WordPress install, with the site it is reachable at.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WpProject {
    pub project_id: String,
    pub name: String,
    /// The site that serves this project, when it has one. `None` for a project with no
    /// domain yet: a bridge needs a URL to hand the browser.
    pub hostname: Option<String>,
    pub url: Option<String>,
    /// When a bridge is still waiting to be used, as a Unix timestamp in seconds. The UI
    /// offers to cancel it; the value is `None` when there is nothing pending.
    pub pending_until: Option<u64>,
}

/// The one-time link to hand the browser.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WpSignIn {
    /// Full URL, token included. Never logged.
    pub url: String,
    /// Unix seconds after which the token is refused.
    pub expires_at: u64,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// A token with 128 bits of entropy, hex-encoded. Same shape as the API token: only the
/// value handed to the browser is ever the plain text.
fn new_token() -> Result<String, CoreError> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes)
        .map_err(|e| CoreError::failed("No sign-in link was made.", e.to_string()))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// True when `dir` holds a WordPress install.
///
/// `wp-config.php` is the marker the rest of OLS already detects a WordPress project by
/// (§42); `wp-admin/index.php` and `wp-includes` are accepted too, because a folder
/// extracted from an archive or cloned without its (git-ignored) config file is still a
/// WordPress site the user wants to sign in to.
pub fn is_wordpress(dir: &Path) -> bool {
    dir.join("wp-config.php").is_file()
        || dir.join("wp-admin").join("index.php").is_file()
        || (dir.join("wp-includes").is_dir() && dir.join("wp-content").is_dir())
}

fn mu_plugins_dir(dir: &Path) -> Result<PathBuf, CoreError> {
    let wp_content = dir.join("wp-content");
    if !wp_content.is_dir() {
        return Err(CoreError::failed_fix(
            "This WordPress project has no wp-content folder.",
            format!(
                "{} is missing, so there is nowhere to put a temporary sign-in helper.",
                wp_content.display()
            ),
            "Restore the project's files (git pull, or re-extract the archive), then try again.",
        ));
    }
    let mu = wp_content.join("mu-plugins");
    // `mu-plugins` is not part of a WordPress zip, so a fresh install has no such folder
    // until one is created. Creating it is ordinary WordPress practice and is the only
    // thing OLS writes into a project folder.
    if !mu.is_dir() {
        std::fs::create_dir_all(&mu).map_err(|e| {
            CoreError::failed_fix(
                "The sign-in helper folder could not be created.",
                format!("{} could not be created: {e}", mu.display()),
                format!(
                    "Grant write access to {} and try again.",
                    wp_content.display()
                ),
            )
        })?;
    }
    Ok(mu)
}

/// The plugin body. Placeholders are substituted with `.replace` rather than `format!`
/// because PHP is written almost entirely out of braces.
///
/// The PHP half of the same guarantee: it reads no password, it refuses any request that
/// did not come from loopback, it refuses an expired token, and it refuses a second use.
const BRIDGE_TEMPLATE: &str = r#"<?php
/**
 * Plugin Name: OpenLocalServer one-time admin sign-in
 * Description: Temporary single-use sign-in written by OpenLocalServer. Removes itself.
 */

defined('ABSPATH') || exit;

const OLS_SIGNIN_TOKEN  = '__TOKEN__';
const OLS_SIGNIN_ISSUED = __ISSUED__;
const OLS_SIGNIN_TTL    = __TTL__;
const OLS_SIGNIN_SITE   = '__HOST__';

add_action('init', 'ols_signin_bridge');

// The app's own theme tokens (ui/src/index.css), copied rather than linked: this page is
// served by WordPress out of the project folder and has no access to the app's stylesheet,
// and a sign-in screen in a different design language than the tool that opened it reads
// as a phishing page.
//
// The scheme is not left to the browser alone. The UI owns the theme (it is a UI-local
// setting), so the click tells the core which way the app is showing and the page is told
// outright — otherwise an app in light mode on a dark-mode machine opens a dark page, which
// is both wrong and the more suspicious-looking of the two. `data-theme` is only written
// when that answer is known; with no answer the media query decides.
const OLS_SIGNIN_THEME = "
:root{color-scheme:light;--radius:8px;--background:oklch(1 0 0);--foreground:oklch(0.18 0.01 200);--card:oklch(1 0 0);--muted-foreground:oklch(0.5 0.015 200);--primary:oklch(0.6 0.135 168);--primary-foreground:oklch(0.99 0.005 168);--border:oklch(0.9 0.008 200);--danger:oklch(0.55 0.2 25)}
@media (prefers-color-scheme:dark){html:not([data-theme=light]){color-scheme:dark;--background:oklch(0.15 0.008 200);--foreground:oklch(0.95 0.005 200);--card:oklch(0.19 0.009 200);--muted-foreground:oklch(0.65 0.015 200);--primary:oklch(0.75 0.14 168);--primary-foreground:oklch(0.15 0.03 168);--border:oklch(1 0 0/.1);--danger:oklch(0.7 0.18 25)}}
html[data-theme=dark]{color-scheme:dark;--background:oklch(0.15 0.008 200);--foreground:oklch(0.95 0.005 200);--card:oklch(0.19 0.009 200);--muted-foreground:oklch(0.65 0.015 200);--primary:oklch(0.75 0.14 168);--primary-foreground:oklch(0.15 0.03 168);--border:oklch(1 0 0/.1);--danger:oklch(0.7 0.18 25)}
*{box-sizing:border-box}
html,body{margin:0;padding:0}
body{min-height:100vh;display:flex;align-items:center;justify-content:center;padding:40px 16px;background:var(--background);color:var(--foreground);font:15px/1.6 Inter,-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif}
.card{width:100%;max-width:440px;background:var(--card);border:1px solid var(--border);border-radius:var(--radius);padding:24px}
h1{margin:0 0 8px;font-size:20px;font-weight:600;letter-spacing:-.01em}
.sub{margin:0 0 20px;font-size:14px;color:var(--muted-foreground)}
.sub strong{color:var(--foreground);font-weight:600}
ul{list-style:none;margin:0;padding:0}
li+li{margin-top:8px}
button{width:100%;padding:10px 14px;border:0;border-radius:6px;background:var(--primary);color:var(--primary-foreground);font:inherit;font-size:14px;font-weight:500;cursor:pointer;transition:filter .15s ease}
button:hover{filter:brightness(1.08)}
button:focus-visible{outline:2px solid var(--primary);outline-offset:2px}
.err{margin:0 0 16px;padding:10px 12px;border-left:3px solid var(--danger);background:color-mix(in oklab,var(--danger) 10%,transparent);font-size:14px}
";

function ols_signin_is_loopback() {
    $remote = isset($_SERVER['REMOTE_ADDR']) ? $_SERVER['REMOTE_ADDR'] : '';
    return in_array($remote, array('127.0.0.1', '::1'), true);
}

function ols_signin_forget() {
    if (is_file(__FILE__)) {
        @unlink(__FILE__);
    }
}

function ols_signin_stop($message, $status) {
    ols_signin_forget();
    wp_die(esc_html($message), 'WordPress admin sign-in', array('response' => $status));
}

function ols_signin_prompt($token, $error) {
    $users = get_users(array('role' => 'administrator', 'orderby' => 'ID', 'order' => 'ASC'));
    $action = esc_url(isset($_SERVER['REQUEST_URI']) ? $_SERVER['REQUEST_URI'] : home_url('/'));
    nocache_headers();
    header('Content-Type: text/html; charset=utf-8');
    echo '<!DOCTYPE html><html lang="en"__THEME_ATTR__><head><meta charset="utf-8">';
    echo '<meta name="viewport" content="width=device-width,initial-scale=1">';
    echo '<title>WordPress admin sign-in</title>';
    echo '<style>' . OLS_SIGNIN_THEME . '</style>';
    echo '</head><body><main class="card">';
    echo '<h1>Sign in as administrator</h1>';
    echo '<p class="sub">OpenLocalServer opened this link for <strong>' . esc_html(OLS_SIGNIN_SITE) . '</strong>. '
        . 'It works once, and only on this computer. No password was read or changed.</p>';
    if ($error !== '') {
        echo '<p class="err">' . esc_html($error) . '</p>';
    }
    if (!$users) {
        echo '<p class="sub">This site has no administrator account yet. Create one in WordPress, then make a new link.</p>';
        echo '</main></body></html>';
        exit;
    }
    echo '<form method="post" action="' . $action . '">';
    echo '<input type="hidden" name="ols_signin" value="' . esc_attr($token) . '">';
    echo '<ul>';
    foreach ($users as $user) {
        $label = $user->user_login;
        if ($user->display_name !== '') {
            $label .= ' (' . $user->display_name . ')';
        }
        echo '<li><button type="submit" name="ols_signin_user" value="' . esc_attr($user->user_login) . '">'
            . 'Sign in as ' . esc_html($label) . '</button></li>';
    }
    echo '</ul></form></main></body></html>';
    exit;
}

function ols_signin_bridge() {
    if (!isset($_REQUEST['ols_signin'])) {
        return;
    }
    $token = sanitize_text_field(wp_unslash($_REQUEST['ols_signin']));
    if (!hash_equals(OLS_SIGNIN_TOKEN, $token)) {
        wp_die('This sign-in link is not valid.', 'WordPress admin sign-in', array('response' => 403));
    }
    if (!ols_signin_is_loopback()) {
        ols_signin_stop('A sign-in link only works from the computer running this site.', 403);
    }
    if (time() > OLS_SIGNIN_ISSUED + OLS_SIGNIN_TTL) {
        ols_signin_stop('This sign-in link has expired. Make a new one from OpenLocalServer.', 410);
    }
    $used = 'ols_signin_used_' . $token;
    if (get_transient($used)) {
        ols_signin_stop('This sign-in link has already been used. Make a new one from OpenLocalServer.', 410);
    }

    $login = isset($_POST['ols_signin_user']) ? sanitize_user(wp_unslash($_POST['ols_signin_user'])) : '';
    if ($login === '') {
        ols_signin_prompt($token, '');
    }
    $user = get_user_by('login', $login);
    if (!$user || !user_can($user, 'manage_options')) {
        ols_signin_prompt($token, 'That account is not a WordPress administrator.');
    }

    set_transient($used, time(), OLS_SIGNIN_TTL + MINUTE_IN_SECONDS);
    ols_signin_forget();
    wp_set_current_user($user->ID);
    wp_set_auth_cookie($user->ID, false);
    wp_redirect(admin_url());
    exit;
}
"#;

/// Renders the plugin body for one token, expiry, site name and theme.
///
/// `theme` is what the app was showing when the link was made (`"light"`, `"dark"`, or
/// `None` when the caller does not know). `None` writes no `data-theme`, which leaves the
/// page to the browser's own preference rather than guessing.
fn bridge_source(token: &str, issued_at: u64, ttl: u64, site: &str, theme: Option<&str>) -> String {
    let theme_attr = match theme.map(str::trim) {
        Some("dark") => " data-theme=\"dark\"",
        Some("light") => " data-theme=\"light\"",
        _ => "",
    };
    BRIDGE_TEMPLATE
        .replace("__TOKEN__", token)
        .replace("__ISSUED__", &issued_at.to_string())
        .replace("__TTL__", &ttl.to_string())
        .replace("__HOST__", site)
        .replace("__THEME_ATTR__", theme_attr)
}

/// The host part of a site URL, for the sign-in page's own line of text. Reduced to
/// characters a hostname can hold so nothing from a caller lands inside the PHP source
/// unescaped; anything unexpected falls back to a neutral word.
fn host_label(site_url: &str) -> String {
    let after_scheme = site_url
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(site_url);
    let host: String = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .chars()
        .take(80)
        .collect();
    if host.is_empty()
        || !host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || ".-:".contains(c))
    {
        "this site".to_string()
    } else {
        host
    }
}

/// Reads back the `const` values OLS wrote, used to age a file out. A file it cannot parse
/// is a file it must not keep, so this is the `err`-as-`None` case by design.
fn bridge_constants(source: &str) -> Option<(u64, u64)> {
    let read = |key: &str| {
        let marker = format!("const {key}");
        let start = source.find(&marker)? + marker.len();
        let rest = &source[start..];
        let end = rest.find(';')?;
        rest[..end]
            .trim()
            .trim_start_matches('=')
            .trim()
            .parse::<u64>()
            .ok()
    };
    Some((read("OLS_SIGNIN_ISSUED")?, read("OLS_SIGNIN_TTL")?))
}

fn is_bridge(name: &str) -> bool {
    name.starts_with(BRIDGE_PREFIX) && name.ends_with(".php")
}

fn bridge_files(mu: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(mu) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(is_bridge)
        })
        .collect()
}

/// Removes every bridge for `dir`. How many were removed is returned so a revoke can tell
/// the user whether there was anything to cancel.
pub fn revoke(dir: &Path) -> Result<usize, CoreError> {
    let mu = match mu_plugins_dir(dir) {
        Ok(mu) => mu,
        Err(_) => return Ok(0),
    };
    let mut removed = 0;
    for file in bridge_files(&mu) {
        if std::fs::remove_file(&file).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

/// Removes bridges whose window has closed, and reports the expiry of the newest one that
/// is still live. Called on every listing, so a crash between issue and use cannot leave a
/// usable link behind for longer than its own TTL plus the grace window.
pub fn sweep(dir: &Path) -> Option<u64> {
    let mu = mu_plugins_dir(dir).ok()?;
    let now = now_secs();
    let mut live: Option<u64> = None;
    for file in bridge_files(&mu) {
        let expires = std::fs::read_to_string(&file)
            .ok()
            .as_deref()
            .and_then(bridge_constants)
            .map(|(issued, ttl)| {
                if now >= issued + ttl + SWEEP_GRACE_SECS {
                    None
                } else {
                    Some(issued + ttl)
                }
            })
            // `None` covers "cannot read" and "cannot parse" as well as "expired": a bridge
            // OLS cannot age out is a bridge it removes.
            .unwrap_or(None);
        match expires {
            Some(at) => live = Some(live.map_or(at, |current: u64| current.max(at))),
            None => {
                let _ = std::fs::remove_file(&file);
            }
        }
    }
    live
}

/// Issues a one-time sign-in bridge for `dir` and returns the link to open.
///
/// `site_url` is only used to build the link's base; the WordPress install is expected to
/// answer there, which the caller has already checked by resolving the project's domain.
pub fn issue(
    dir: &Path,
    site_url: &str,
    ttl_secs: Option<u64>,
    theme: Option<&str>,
) -> Result<WpSignIn, CoreError> {
    if !is_wordpress(dir) {
        return Err(CoreError::failed_fix(
            "That folder is not a WordPress project.",
            format!("{} has no wp-config.php, wp-admin or wp-includes, so there is no WordPress install to sign in to.", dir.display()),
            "Open the project's folder and check the WordPress files are all there.",
        ));
    }
    let mu = mu_plugins_dir(dir)?;
    // An earlier link is superseded, never accumulated: one live bridge per project keeps
    // the window for a leaked file as short as the code allows.
    let _ = revoke(dir);
    let ttl = ttl_secs.unwrap_or(DEFAULT_TTL_SECS).clamp(1, MAX_TTL_SECS);
    let issued_at = now_secs();
    let token = new_token()?;
    let file = mu.join(format!("{BRIDGE_PREFIX}{token}.php"));
    let source = bridge_source(&token, issued_at, ttl, &host_label(site_url), theme);
    if let Err(e) = std::fs::write(&file, source) {
        return Err(CoreError::failed_fix(
            "The sign-in helper could not be written.",
            format!("{} could not be written: {e}", file.display()),
            format!("Grant write access to {} and try again.", mu.display()),
        ));
    }
    // Everything older than this token is gone by the call above: one live bridge per
    // project keeps the window for a leaked file as short as the code allows.
    let base = site_url.trim_end_matches('/');
    Ok(WpSignIn {
        url: format!("{base}/?ols_signin={token}"),
        expires_at: issued_at + ttl,
    })
}

/// Describes `dir` for the project list: whether it is WordPress, and whether a bridge is
/// still waiting to be used.
pub fn describe(project_id: &str, name: &str, dir: &Path) -> Option<WpProject> {
    if !is_wordpress(dir) {
        return None;
    }
    Some(WpProject {
        project_id: project_id.to_string(),
        name: name.to_string(),
        hostname: None,
        url: None,
        pending_until: sweep(dir),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Diagnostic;

    fn wp_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("wp-content")).unwrap();
        std::fs::create_dir_all(dir.path().join("wp-admin")).unwrap();
        std::fs::write(dir.path().join("wp-admin/index.php"), "<?php").unwrap();
        std::fs::write(dir.path().join("wp-config.php"), "<?php").unwrap();
        dir
    }

    #[test]
    fn detects_wordpress_by_config_or_admin_folder() {
        let dir = wp_dir();
        assert!(is_wordpress(dir.path()));
        std::fs::remove_file(dir.path().join("wp-config.php")).unwrap();
        assert!(
            is_wordpress(dir.path()),
            "wp-admin alone still means WordPress"
        );
        std::fs::remove_dir_all(dir.path().join("wp-admin")).unwrap();
        assert!(!is_wordpress(dir.path()));
    }

    #[test]
    fn plain_php_project_is_not_wordpress() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("index.php"), "<?php").unwrap();
        std::fs::write(dir.path().join("composer.json"), "{}").unwrap();
        assert!(!is_wordpress(dir.path()));
    }

    #[test]
    fn issued_link_carries_a_fresh_token_and_writes_one_bridge() {
        let dir = wp_dir();
        let sign_in = issue(dir.path(), "http://blog.test/", None, None).unwrap();
        assert!(sign_in.url.starts_with("http://blog.test/?ols_signin="));
        assert!(sign_in.expires_at > now_secs());
        let token = sign_in.url.rsplit('=').next().unwrap();
        assert_eq!(token.len(), 32, "a 128-bit token, hex-encoded");
        let written = bridge_files(&dir.path().join("wp-content/mu-plugins"));
        assert_eq!(written.len(), 1);
        assert!(written[0].to_string_lossy().contains(token));
    }

    /// The guarantee the whole feature rests on: the helper handed to the site must not
    /// carry any way to read a password out of WordPress.
    #[test]
    fn bridge_source_never_reads_a_password() {
        let source = bridge_source("deadbeef", 1, 300, "blog.test", None);
        for forbidden in [
            "user_pass",
            "wp-config",
            "DB_PASSWORD",
            "$wpdb",
            "wp_hash_password",
            "wp_set_password",
        ] {
            assert!(
                !source.contains(forbidden),
                "the sign-in helper must not mention {forbidden}"
            );
        }
    }

    /// The sign-in screen is the one surface the user meets outside the app, and it has no
    /// access to the app's stylesheet. It therefore carries the app's own tokens, both
    /// themes: a page in a foreign design language is what a phishing page looks like.
    #[test]
    fn sign_in_page_uses_the_app_theme_in_both_schemes() {
        let source = bridge_source("deadbeef", 1, 300, "blog.test", None);
        assert!(source.contains("prefers-color-scheme:dark"));
        assert!(source.contains("--primary:oklch(0.6 0.135 168)"));
        assert!(source.contains("--primary:oklch(0.75 0.14 168)"));
        assert!(source.contains("var(--background)"));
        assert!(source.contains("= 'blog.test'"));
    }

    #[test]
    fn site_label_is_a_hostname_or_a_neutral_fallback() {
        assert_eq!(host_label("https://blog.test/"), "blog.test");
        assert_eq!(host_label("http://localhost:8080/"), "localhost:8080");
        assert_eq!(host_label("http://'/--<'"), "this site");
    }

    /// An app in light mode on a dark-mode machine must not open a dark page. The UI owns
    /// the theme, so it says which way it is showing and the page is told outright; with no
    /// answer the page carries no `data-theme` at all and the browser decides.
    #[test]
    fn sign_in_page_follows_the_app_theme_not_just_the_browser() {
        let light = bridge_source("a", 1, 300, "blog.test", Some("light"));
        assert!(light.contains("<html lang=\"en\" data-theme=\"light\">"));
        let dark = bridge_source("a", 1, 300, "blog.test", Some("dark"));
        assert!(dark.contains("<html lang=\"en\" data-theme=\"dark\">"));
        let unknown = bridge_source("a", 1, 300, "blog.test", None);
        assert!(unknown.contains("<html lang=\"en\">"));
        for source in [light, dark, unknown] {
            assert!(source.contains("prefers-color-scheme:dark"));
            assert!(source.contains("html:not([data-theme=light])"));
            assert!(source.contains("html[data-theme=dark]"));
        }
    }

    #[test]
    fn expiring_bridge_is_swept_and_reports_nothing_pending() {
        let dir = wp_dir();
        // Written by hand with an expiry in the past, which is what a crash between issue
        // and use leaves behind. `mu-plugins` is created through the same call the sweep
        // uses, so this writes where a real install would put it -- a WordPress zip has no
        // such folder, and creating it is the code's job, not the test's.
        let file = mu_plugins_dir(dir.path())
            .unwrap()
            .join(format!("{BRIDGE_PREFIX}cafe.php"));
        let issued = now_secs() - 10_000;
        std::fs::write(&file, bridge_source("cafe", issued, 300, "blog.test", None)).unwrap();
        assert_eq!(sweep(dir.path()), None);
        assert!(
            !file.exists(),
            "an expired bridge is removed, not left usable"
        );
    }

    #[test]
    fn unreadable_bridge_is_removed_rather_than_kept() {
        let dir = wp_dir();
        let file = mu_plugins_dir(dir.path())
            .unwrap()
            .join(format!("{BRIDGE_PREFIX}0123.php"));
        std::fs::write(&file, "<?php // not written by us in the expected shape").unwrap();
        assert_eq!(sweep(dir.path()), None);
        assert!(!file.exists());
    }

    #[test]
    fn live_bridge_is_reported_and_revoke_removes_it() {
        let dir = wp_dir();
        let sign_in = issue(dir.path(), "http://blog.test/", None, None).unwrap();
        assert_eq!(sweep(dir.path()), Some(sign_in.expires_at));
        assert_eq!(revoke(dir.path()).unwrap(), 1);
        assert!(bridge_files(&dir.path().join("wp-content/mu-plugins")).is_empty());
        assert_eq!(revoke(dir.path()).unwrap(), 0);
    }

    /// A second click must not leave the first link working: one live bridge per project.
    #[test]
    fn a_new_bridge_supersedes_the_previous_one() {
        let dir = wp_dir();
        let first = issue(dir.path(), "http://blog.test/", None, None).unwrap();
        let second = issue(dir.path(), "http://blog.test/", None, None).unwrap();
        assert_ne!(first.url, second.url);
        let mu = dir.path().join("wp-content/mu-plugins");
        assert_eq!(bridge_files(&mu).len(), 1);
    }

    #[test]
    fn ttl_is_clamped_so_no_link_outlives_its_window() {
        let dir = wp_dir();
        let short = issue(dir.path(), "http://blog.test/", Some(60_000), None).unwrap();
        assert!(short.expires_at <= now_secs() + MAX_TTL_SECS);
    }

    #[test]
    fn missing_wp_content_explains_itself() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("wp-config.php"), "<?php").unwrap();
        let err = issue(dir.path(), "http://blog.test/", None, None).unwrap_err();
        let diagnostic = Diagnostic::from(&err);
        assert!(diagnostic.problem.contains("wp-content"));
        assert!(
            diagnostic.fix.is_some(),
            "every refusal says how to get unblocked"
        );
    }

    #[test]
    fn non_wordpress_folder_is_refused_with_a_fix() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("index.php"), "<?php").unwrap();
        let err = issue(dir.path(), "http://blog.test/", None, None).unwrap_err();
        let diagnostic = Diagnostic::from(&err);
        assert!(diagnostic.problem.contains("not a WordPress project"));
        assert!(diagnostic.fix.is_some());
    }
}
