//! Domain Manager (§44–48, §30 — Stages 6 and 9): the local domains OpenLocalServer serves, how
//! each is routed (PHP / reverse proxy / static), and who owns its web-server config.
//! Pure data + validation — turning a `Domain` into server config lives in `web/`.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::db;
use crate::error::CoreError;
use crate::paths::AppPaths;

/// §26: who is allowed to change a site's web-server config file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Ownership {
    /// OpenLocalServer generates the whole file from the site's settings; hand edits are drift.
    #[default]
    Managed,
    /// OpenLocalServer generates the file and includes a user-owned snippet inside the server block.
    Advanced,
    /// The user owns the whole file. OpenLocalServer validates and reloads it, never rewrites it.
    Manual,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SiteKind {
    /// Served by a supervised `php-cgi` pool. `version: None` = the newest installed PHP.
    Php {
        version: Option<String>,
    },
    /// Reverse proxy (§30): a dev server on this machine (Node/Python apps), or anything
    /// else reachable — a Docker container, another computer — via `upstream_host`.
    Proxy {
        upstream_port: u16,
        /// Where the target runs; `None` = this machine (127.0.0.1).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        upstream_host: Option<String>,
        /// The target only speaks HTTPS.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        upstream_https: bool,
    },
    Static,
}

/// A proxy target is pasted into server configs: a real port, and a hostname or IPv4
/// address with nothing that could break out of a directive.
fn validate_kind(kind: &SiteKind) -> Result<(), CoreError> {
    if let SiteKind::Proxy {
        upstream_port,
        upstream_host,
        ..
    } = kind
    {
        if *upstream_port == 0 {
            return Err(CoreError::DomainError(
                "upstream port must be between 1 and 65535".into(),
            ));
        }
        if let Some(h) = upstream_host {
            if h.is_empty()
                || !h
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
            {
                return Err(CoreError::DomainError(format!(
                    "\"{h}\" is not a valid target host"
                )));
            }
        }
    }
    Ok(())
}

impl SiteKind {
    /// The full upstream URL of a proxy site, e.g. `http://127.0.0.1:3000`.
    pub fn upstream_url(&self) -> Option<String> {
        match self {
            SiteKind::Proxy {
                upstream_port,
                upstream_host,
                upstream_https,
            } => Some(format!(
                "{}://{}:{upstream_port}",
                if *upstream_https { "https" } else { "http" },
                upstream_host
                    .as_deref()
                    .filter(|h| !h.is_empty())
                    .unwrap_or("127.0.0.1")
            )),
            _ => None,
        }
    }
}

/// A supervised app dev-server behind a proxy site (`npm run dev`, `uvicorn`, ...).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppSpec {
    pub executable: String,
    pub args: Vec<String>,
    pub cwd: String,
    /// Runtime whose bin dir goes first on PATH ("node", "python", "php").
    pub runtime: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeaderRule {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedirectRule {
    pub from: String,
    pub to: String,
    pub code: u16,
}

/// §30: `path` on this site is forwarded to `upstream` (e.g. `/api` → `http://127.0.0.1:8000`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxyMapping {
    pub path: String,
    pub upstream: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpstreamGroup {
    pub name: String,
    pub servers: Vec<String>,
}

/// §23: the structured (form-editable) blocks common to every web server. Raw editing
/// stays available — these just cover the frequent cases without hand-writing config.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SiteBlocks {
    #[serde(default)]
    pub headers: Vec<HeaderRule>,
    #[serde(default)]
    pub redirects: Vec<RedirectRule>,
    #[serde(default)]
    pub mappings: Vec<ProxyMapping>,
    #[serde(default)]
    pub upstreams: Vec<UpstreamGroup>,
    #[serde(default)]
    pub includes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Domain {
    pub hostname: String,
    #[serde(default)]
    pub project_id: Option<String>,
    /// Document root (PHP/static) or project root (proxy).
    pub root: String,
    pub kind: SiteKind,
    #[serde(default)]
    pub https: bool,
    /// §52: send plain-HTTP requests to HTTPS. Only meaningful when `https` is on.
    #[serde(default)]
    pub redirect_https: bool,
    /// §46: also answer for `*.hostname` (needs the local DNS resolver, Stage 9).
    #[serde(default)]
    pub wildcard: bool,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub ownership: Ownership,
    #[serde(default)]
    pub app: Option<AppSpec>,
    #[serde(default)]
    pub blocks: SiteBlocks,
    /// SHA-256 of the config file OpenLocalServer last wrote, per web server id — drift
    /// detection (§26). Keyed by server because each server has its own file.
    #[serde(default)]
    pub generated_hashes: BTreeMap<String, String>,
    /// Public hostname routed through a named Cloudflare tunnel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_domain: Option<String>,
    /// Saved tunnel configuration used to expose this site.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tunnel_id: Option<String>,
    /// Which web server renders this site ("nginx" | "apache" | "caddy"). `None` means
    /// the default server; a site is rendered on exactly one server, never both.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
}

/// Rejects a per-site server id that isn't one we ship. A blank value means "use the
/// default" and is stored as such, so the form and the file agree.
pub fn validate_domain_server(server: Option<&str>) -> Result<Option<String>, CoreError> {
    match server.map(str::trim) {
        None | Some("") => Ok(None),
        Some(s) if crate::web::SERVER_IDS.contains(&s) => Ok(Some(s.to_string())),
        Some(s) => Err(CoreError::DomainError(format!(
            "\"{s}\" is not a web server OpenLocalServer ships. \
             Pick Default, Nginx, Apache or Caddy."
        ))),
    }
}

/// The server that actually renders this site: its own override, or the default one.
pub fn resolved_server(d: &Domain, cfg: &crate::web::WebConfig) -> String {
    cfg.resolve_server(d.server.as_deref())
}

fn yes() -> bool {
    true
}

/// The site every fresh install starts with: a static welcome page.
pub const HOME_HOSTNAME: &str = "openlocalserver.test";
/// Hostname seeded by older installs; renamed to [`HOME_HOSTNAME`] on load.
const LEGACY_HOME_HOSTNAME: &str = "home.test";

/// A fresh install (no `domains.json` yet) gets `openlocalserver.test`: a static,
/// managed, HTTPS site serving a welcome page from the data dir. A missing
/// file is the only trigger — deleting the site afterwards is respected.
fn home_domain(dir: &Path) -> Domain {
    Domain {
        hostname: HOME_HOSTNAME.into(),
        project_id: None,
        root: dir.display().to_string(),
        kind: SiteKind::Static,
        https: true,
        redirect_https: true,
        wildcard: false,
        enabled: true,
        ownership: Ownership::Managed,
        app: None,
        blocks: SiteBlocks::default(),
        generated_hashes: BTreeMap::new(),
        public_domain: None,
        tunnel_id: None,
        server: None,
    }
}

/// Writes the welcome page. Never overwrites: hand edits survive updates.
fn write_home_page(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    // The exact official logo bytes (ui/public/favicon.svg, also the sidebar
    // header mark) so the page reuses the real brand, never a redraw.
    let logo = dir.join("logo.svg");
    if !logo.exists() {
        std::fs::write(logo, LOGO_SVG)?;
    }
    let index = dir.join("index.html");
    if !index.exists() {
        std::fs::write(index, HOME_PAGE)?;
    }
    Ok(())
}

/// Exact bytes of the official application logo.
const LOGO_SVG: &[u8] = include_bytes!("../../../ui/public/favicon.svg");

/// Marker of the currently shipped welcome page.
const HOME_VERSION_MARKER: &str = "<!-- home v4 -->";

/// Markers of older shipped pages (v1 teal cards, v1.5 centered hero, v2 brand,
/// v3 brand). A page carrying one of these was never hand-edited, so it is safe
/// to refresh — v4 is what re-seeds `logo.svg` with the new green app icon.
const HOME_LEGACY_MARKERS: &[&str] = &[
    "max-width: 720px",
    "OPENLOCALSERVER",
    "<h1>home.test</h1>",
    "created once, on first install",
    "<!-- home v3 -->",
];

/// Replaces a stock older welcome page with the current one. Returns true when
/// it wrote. Hand-edited pages and io failures are silently kept as-is: this
/// must never break startup over a cosmetic file.
fn upgrade_home_page(paths: &AppPaths) -> bool {
    let dir = paths.data_dir().join("home");
    let index = dir.join("index.html");
    let current = std::fs::read_to_string(&index).unwrap_or_default();
    if current.contains(HOME_VERSION_MARKER) {
        return false;
    }
    let stock = HOME_LEGACY_MARKERS.iter().any(|m| current.contains(m));
    if !stock && index.exists() {
        return false;
    }
    std::fs::create_dir_all(&dir)
        .and_then(|_| std::fs::write(dir.join("logo.svg"), LOGO_SVG))
        .and_then(|_| std::fs::write(&index, HOME_PAGE))
        .is_ok()
}

/// Static welcome page for `openlocalserver.test`. Self-contained except the
/// seeded `logo.svg` (exact official brand bytes); works offline.
const HOME_PAGE: &str = r##"<!doctype html>
<html lang="en" data-theme="light">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Open Local Server — local development, simplified</title>
<style>
  :root {
    --teal: #0d9488; --teal-dark: #0b6e64; --ink: #0f2e2b; --muted: #5b6f6c;
    --bg: #f4faf8; --card: #ffffff; --line: #dcebe7; --soft: #e6f5f1;
  }
  [data-theme="dark"] {
    --ink: #d7efeb; --muted: #93a8a4;
    --bg: #0b1514; --card: #12201e; --line: #223836; --soft: #142625;
  }
  * { box-sizing: border-box; }
  body { margin: 0; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; background: var(--bg); color: var(--ink); transition: background .25s, color .25s; }
  a { color: var(--teal); }
  .wrap { max-width: 1020px; margin: 0 auto; padding: 0 24px; }
  header.top { display: flex; align-items: center; justify-content: space-between; padding: 18px 0; }
  .brand { display: flex; align-items: center; gap: 10px; font-weight: 700; font-size: 16px; }
  .brand img { width: 30px; height: 30px; border-radius: 8px; }
  nav.top { display: flex; align-items: center; gap: 4px; }
  nav.top a, nav.top button { font-size: 14px; color: var(--muted); text-decoration: none; background: none; border: 0; cursor: pointer; padding: 8px 12px; border-radius: 8px; font-family: inherit; }
  nav.top a:hover, nav.top button:hover { background: var(--soft); color: var(--ink); }
  .hero { text-align: center; padding: 64px 0 16px; }
  .badge { display: inline-block; background: var(--soft); color: var(--teal-dark); font-size: 13.5px; font-weight: 600; padding: 7px 16px; border-radius: 999px; border: 1px solid var(--line); }
  [data-theme="dark"] .badge { color: #5eead4; }
  .hero h1 { font-size: clamp(44px, 7vw, 76px); font-weight: 800; letter-spacing: -2px; margin: 22px 0 8px; }
  .hero h1 .os { color: var(--ink); } .hero h1 .ls { color: var(--teal); }
  .hero h2 { font-size: clamp(18px, 2.6vw, 24px); font-weight: 600; color: var(--muted); margin: 0 0 12px; }
  .hero p.sub { max-width: 620px; margin: 0 auto; color: var(--muted); font-size: 16px; line-height: 1.6; }
  .cta { display: flex; gap: 12px; justify-content: center; flex-wrap: wrap; margin: 30px 0 18px; }
  .btn { display: inline-flex; align-items: center; gap: 8px; font-size: 15px; font-weight: 600; padding: 13px 26px; border-radius: 12px; text-decoration: none; border: 1px solid transparent; }
  .btn.primary { background: var(--teal); color: #fff; box-shadow: 0 4px 14px rgba(13,148,136,.35); }
  .btn.primary:hover { background: var(--teal-dark); }
  .btn.ghost { border-color: var(--line); color: var(--ink); background: var(--card); }
  .status { color: var(--muted); font-size: 14px; }
  .status .dot { color: #22c55e; }
  .visual { position: relative; max-width: 760px; margin: 56px auto 8px; min-height: 300px; }
  .visual .core { position: absolute; left: 50%; top: 50%; transform: translate(-50%,-50%); width: 120px; height: 120px; }
  .visual .core img { width: 100%; height: 100%; border-radius: 28px; box-shadow: 0 12px 40px rgba(13,148,136,.35); animation: float 5s ease-in-out infinite; }
  .node { position: absolute; background: var(--card); border: 1px solid var(--line); border-radius: 12px; padding: 10px 14px; font-size: 13.5px; box-shadow: 0 2px 10px rgba(0,0,0,.06); animation: float 6s ease-in-out infinite; }
  .node b { display: block; font-size: 13.5px; } .node span { color: var(--muted); font-size: 12px; }
  .node .tick { display: inline-block; width: 8px; height: 8px; border-radius: 50%; background: var(--teal); margin-right: 6px; }
  .n1 { left: 0; top: 0; } .n2 { left: 2%; top: 42%; animation-delay: -2s; } .n3 { left: 6%; bottom: 0; animation-delay: -4s; }
  .n4 { right: 0; top: 0; animation-delay: -1s; } .n5 { right: 2%; top: 42%; animation-delay: -3s; } .n6 { right: 6%; bottom: 0; animation-delay: -5s; }
  .visual svg.wires { position: absolute; inset: 0; width: 100%; height: 100%; }
  .visual svg.wires path { fill: none; stroke: var(--teal); stroke-width: 1.5; stroke-dasharray: 4 5; opacity: .45; }
  @keyframes float { 0%,100% { translate: 0 0; } 50% { translate: 0 -8px; } }
  section.block { padding: 56px 0 8px; }
  section.block h3 { text-align: center; font-size: clamp(24px, 3.4vw, 32px); letter-spacing: -.5px; margin: 0 0 8px; }
  section.block p.lead { text-align: center; color: var(--muted); margin: 0 0 28px; }
  .cards { display: grid; grid-template-columns: repeat(auto-fit, minmax(170px, 1fr)); gap: 14px; }
  .card { background: var(--card); border: 1px solid var(--line); border-radius: 14px; padding: 20px; }
  .card h4 { margin: 0 0 6px; font-size: 15px; } .card p { margin: 0; color: var(--muted); font-size: 13.5px; line-height: 1.6; }
  .steps { display: grid; grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); gap: 14px; }
  .step { padding: 8px 4px; }
  .step .num { font-size: 13px; font-weight: 700; color: var(--teal); letter-spacing: 1px; }
  .step h4 { margin: 6px 0; font-size: 16px; } .step p { margin: 0; color: var(--muted); font-size: 14px; line-height: 1.6; }
  .local { text-align: center; padding: 72px 0 16px; }
  .local h3 { font-size: clamp(26px, 3.6vw, 34px); letter-spacing: -.5px; margin: 0 0 10px; }
  .local p { color: var(--muted); max-width: 560px; margin: 0 auto 18px; line-height: 1.65; }
  .local .lock { font-size: 40px; }
  footer { border-top: 1px solid var(--line); margin-top: 56px; padding: 22px 0 40px; font-size: 13.5px; color: var(--muted); }
  footer .wrap { display: flex; align-items: center; justify-content: space-between; gap: 12px; flex-wrap: wrap; }
  footer .brand { font-size: 14px; } footer .brand img { width: 22px; height: 22px; border-radius: 6px; }
  footer nav { display: flex; gap: 4px; } footer nav a { color: var(--muted); text-decoration: none; padding: 6px 10px; border-radius: 8px; }
  footer nav a:hover { background: var(--soft); color: var(--ink); }
  @media (max-width: 640px) {
    .node { position: static; margin: 6px auto; width: fit-content; animation: none; }
    .visual { min-height: 0; display: flex; flex-direction: column; align-items: center; gap: 4px; }
    .visual .core { position: static; transform: none; margin-bottom: 10px; }
    .visual svg.wires { display: none; }
  }
  @media (prefers-reduced-motion: reduce) { .node, .visual .core img { animation: none; } }
</style>
</head>
<body>
<!-- home v4 -->
<div class="wrap">
  <header class="top">
    <div class="brand"><img src="logo.svg" alt="Open Local Server logo">Open Local Server</div>
    <nav class="top" aria-label="Primary">
      <a href="https://github.com/kz370/OpenLocalServer">Documentation</a>
      <a href="https://github.com/kz370/OpenLocalServer">GitHub</a>
      <a href="https://github.com/kz370/OpenLocalServer/issues">Support</a>
      <button id="theme" aria-label="Toggle theme">◐</button>
    </nav>
  </header>
</div>
<main class="wrap">
  <div class="hero">
    <span class="badge">🚀 Local Development, Simplified</span>
    <h1><span class="os">Open</span> <span class="ls">Local Server</span></h1>
    <h2>Everything you need for local development.</h2>
    <p class="sub">Run PHP, Node.js, Python and more — with automatic local domains, trusted HTTPS, databases, and zero cloud setup.</p>
    <div class="cta">
      <a class="btn primary" href="#start">Add Your First Project</a>
      <a class="btn ghost" href="#tools">Open Terminal</a>
    </div>
    <div class="status"><span class="dot">●</span> Local Environment Ready</div>
  </div>

  <div class="visual" aria-hidden="true">
    <svg class="wires" viewBox="0 0 760 300" preserveAspectRatio="none">
      <path d="M170 40 C 260 40, 300 130, 330 150"/><path d="M170 150 C 250 150, 300 150, 330 150"/><path d="M170 260 C 260 260, 300 170, 330 160"/>
      <path d="M430 150 C 460 130, 500 40, 590 40"/><path d="M430 150 C 460 150, 510 150, 590 150"/><path d="M430 160 C 460 170, 500 260, 590 260"/>
    </svg>
    <div class="node n1"><b><span class="tick"></span>PHP</b><span>8.x</span></div>
    <div class="node n2"><b><span class="tick"></span>Node.js</b><span>20.x+</span></div>
    <div class="node n3"><b><span class="tick"></span>Python</b><span>3.x</span></div>
    <div class="core"><img src="logo.svg" alt=""></div>
    <div class="node n4"><b><span class="tick"></span>Local Domains</b><span>*.test, *.localhost</span></div>
    <div class="node n5"><b><span class="tick"></span>Trusted HTTPS</b><span>Automatic SSL</span></div>
    <div class="node n6"><b><span class="tick"></span>Databases</b><span>MySQL, MariaDB, PostgreSQL, Redis</span></div>
  </div>

  <section class="block">
    <div class="cards">
      <div class="card"><h4>Local Domains</h4><p>Use *.test, *.localhost and *.internal without editing your hosts file.</p></div>
      <div class="card"><h4>Trusted HTTPS</h4><p>Automatic local certificates that your browser can trust.</p></div>
      <div class="card"><h4>Multiple Runtimes</h4><p>Run PHP, Node.js, Python and more with project-specific versions.</p></div>
      <div class="card"><h4>Built-in Databases</h4><p>MariaDB, MySQL, PostgreSQL and Redis — ready for local development.</p></div>
      <div class="card" id="tools"><h4>Developer Tools</h4><p>Built-in terminal, Git tools, snapshots, workers and more — in the app's Sites pages.</p></div>
    </div>
  </section>

  <section class="block" id="start">
    <h3>From project folder to local site.</h3>
    <p class="lead">Three steps, all inside the Open Local Server app.</p>
    <div class="steps">
      <div class="step"><div class="num">01</div><h4>Add your project</h4><p>Choose a project folder and let Open Local Server configure it.</p></div>
      <div class="step"><div class="num">02</div><h4>Choose your environment</h4><p>Select your runtime, database and web configuration.</p></div>
      <div class="step"><div class="num">03</div><h4>Start building</h4><p>Open your .test domain and start developing.</p></div>
    </div>
  </section>

  <section class="local">
    <div class="lock">🔒</div>
    <h3>Everything runs on your machine.</h3>
    <p>No cloud. No account. No remote development environment. Your projects, services and data stay local.</p>
  </section>
</main>
<footer>
  <div class="wrap">
    <div class="brand"><img src="logo.svg" alt="Open Local Server logo">Open Local Server</div>
    <nav aria-label="Footer">
      <a href="https://github.com/kz370/OpenLocalServer">Documentation</a>
      <a href="https://github.com/kz370/OpenLocalServer">GitHub</a>
      <a href="https://github.com/kz370/OpenLocalServer/issues">Support</a>
    </nav>
    <span>Open source &amp; free · Made for local development.</span>
  </div>
</footer>
<script>
(function () {
  var key = 'ols-home-theme';
  function paint(t) { document.documentElement.setAttribute('data-theme', t); }
  try {
    var saved = localStorage.getItem(key);
    if (saved === 'dark' || saved === 'light') paint(saved);
    else if (window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches) paint('dark');
  } catch (e) {}
  document.getElementById('theme').addEventListener('click', function () {
    var next = document.documentElement.getAttribute('data-theme') === 'dark' ? 'light' : 'dark';
    paint(next);
    try { localStorage.setItem(key, next); } catch (e) {}
  });
})();
</script>
</body>
</html>
"##;

pub struct DomainStore {
    paths: AppPaths,
    domains: Vec<Domain>,
}

impl DomainStore {
    pub fn load(paths: &AppPaths) -> Result<Self, CoreError> {
        paths.ensure_dirs()?;
        let mut domains: Vec<Domain> = db::load_docs(paths, "domains").unwrap_or_default();
        let mut seeded = false;
        if !domains.iter().any(|d| d.hostname == HOME_HOSTNAME) {
            // The home site is built in and undeletable, so its absence only
            // means an older install: adopt legacy "home.test" or seed fresh.
            // Existing page files are kept; stock old pages refresh below.
            if let Some(old) = domains
                .iter_mut()
                .find(|d| d.hostname == LEGACY_HOME_HOSTNAME)
            {
                // Older installs seeded "home.test": adopt the new name, keep root and edits.
                old.hostname = HOME_HOSTNAME.into();
                old.generated_hashes.clear();
            } else {
                let home_dir = paths.data_dir().join("home");
                write_home_page(&home_dir)?;
                domains.push(home_domain(&home_dir));
            }
            seeded = true;
        }
        let store = Self {
            paths: paths.clone(),
            domains,
        };
        if seeded {
            store.persist()?;
        }
        // Roll out new welcome-page designs to installs seeded by older
        // versions — but only when the file is still a known stock page.
        // Hand-edited pages (no known marker) are left alone.
        if store.domains.iter().any(|d| d.hostname == HOME_HOSTNAME) {
            let _ = upgrade_home_page(paths);
        }
        Ok(store)
    }

    pub fn list(&self) -> Vec<Domain> {
        let mut list = self.domains.clone();
        list.sort_by(|a, b| a.hostname.cmp(&b.hostname));
        list
    }

    pub fn get(&self, hostname: &str) -> Option<Domain> {
        self.domains
            .iter()
            .find(|d| d.hostname == hostname)
            .cloned()
    }

    /// Adds a new domain. Rejects invalid names and any conflict (§44 conflict detection).
    pub fn add(&mut self, mut domain: Domain) -> Result<Domain, CoreError> {
        domain.hostname = domain.hostname.trim().to_ascii_lowercase();
        validate_hostname(&domain.hostname)?;
        domain.server = validate_domain_server(domain.server.as_deref())?;
        if let Some(host) = domain.public_domain.as_mut() {
            *host = host.trim().to_ascii_lowercase();
            validate_hostname(host)?;
            validate_public_hostname(host)?;
        }
        if let Some(other) = self.domains.iter().find(|d| {
            d.hostname == domain.hostname
                || d.public_domain.as_deref() == Some(domain.hostname.as_str())
                || domain.public_domain.as_deref().is_some_and(|host| {
                    d.hostname == host || d.public_domain.as_deref() == Some(host)
                })
        }) {
            return Err(CoreError::DomainError(format!(
                "{} overlaps the existing site or public hostname {}",
                domain.hostname, other.hostname
            )));
        }
        if let Some(reason) = self.conflict(&domain.hostname, domain.wildcard, None) {
            return Err(CoreError::DomainError(reason));
        }
        validate_kind(&domain.kind)?;
        self.domains.push(domain.clone());
        self.persist()?;
        Ok(domain)
    }

    /// Replaces an existing domain (same hostname) with edited settings.
    pub fn update(&mut self, mut domain: Domain) -> Result<Domain, CoreError> {
        validate_kind(&domain.kind)?;
        domain.server = validate_domain_server(domain.server.as_deref())?;
        if let Some(host) = domain.public_domain.as_mut() {
            *host = host.trim().to_ascii_lowercase();
            validate_hostname(host)?;
            validate_public_hostname(host)?;
        }
        if let Some(other) = self
            .domains
            .iter()
            .filter(|d| d.hostname != domain.hostname)
            .find(|d| {
                d.hostname == domain.hostname
                    || d.public_domain.as_deref() == Some(domain.hostname.as_str())
                    || domain.public_domain.as_deref().is_some_and(|host| {
                        d.hostname == host || d.public_domain.as_deref() == Some(host)
                    })
            })
        {
            return Err(CoreError::DomainError(format!(
                "{} overlaps the existing site or public hostname {}",
                domain.hostname, other.hostname
            )));
        }
        let idx = self
            .domains
            .iter()
            .position(|d| d.hostname == domain.hostname)
            .ok_or_else(|| {
                CoreError::DomainError(format!("{} is not a known domain", domain.hostname))
            })?;
        self.domains[idx] = domain.clone();
        self.persist()?;
        Ok(domain)
    }

    pub fn remove(&mut self, hostname: &str) -> Result<(), CoreError> {
        if hostname == HOME_HOSTNAME {
            return Err(CoreError::DomainError(format!(
                "{HOME_HOSTNAME} is built in and can't be deleted"
            )));
        }
        self.domains.retain(|d| d.hostname != hostname);
        self.persist()
    }

    /// Explains why `hostname` can't be added, if it can't. `ignoring` skips one existing
    /// domain (the one being edited).
    pub fn conflict(
        &self,
        hostname: &str,
        wildcard: bool,
        ignoring: Option<&str>,
    ) -> Option<String> {
        for existing in self
            .domains
            .iter()
            .filter(|d| Some(d.hostname.as_str()) != ignoring)
        {
            if existing.hostname == hostname {
                return Some(format!("{hostname} already exists"));
            }
            // A wildcard on `shop.test` would swallow an existing `api.shop.test` (and the reverse).
            if existing.wildcard && hostname.ends_with(&format!(".{}", existing.hostname)) {
                return Some(format!(
                    "{hostname} is already covered by the wildcard on {}",
                    existing.hostname
                ));
            }
            if wildcard && existing.hostname.ends_with(&format!(".{hostname}")) {
                return Some(format!(
                    "a wildcard on {hostname} would overlap the existing domain {}",
                    existing.hostname
                ));
            }
        }
        None
    }

    fn persist(&self) -> Result<(), CoreError> {
        let items: Vec<(String, &Domain)> = self
            .domains
            .iter()
            .map(|d| (d.hostname.clone(), d))
            .collect();
        db::save_docs(&self.paths, "domains", &items)
    }
}

fn validate_public_hostname(host: &str) -> Result<(), CoreError> {
    if ["test", "local", "localhost"]
        .iter()
        .any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}")))
    {
        return Err(CoreError::DomainError(
            "a public domain must use a real DNS hostname, not a local development suffix".into(),
        ));
    }
    Ok(())
}

/// RFC-1123 hostname with at least two labels. Strict on purpose: the name lands in a
/// hosts file, a certificate SAN, and a config file, so nothing exotic may get through.
pub fn validate_hostname(hostname: &str) -> Result<(), CoreError> {
    let bad = |why: &str| {
        Err(CoreError::DomainError(format!(
            "\"{hostname}\" is not a valid domain: {why}"
        )))
    };
    if hostname.len() > 253 {
        return bad("too long");
    }
    let labels: Vec<&str> = hostname.split('.').collect();
    if labels.len() < 2 {
        return bad("needs at least two parts, like shop.test");
    }
    for label in labels {
        if label.is_empty() || label.len() > 63 {
            return bad("empty or over-long label");
        }
        if label.starts_with('-') || label.ends_with('-') {
            return bad("labels can't start or end with '-'");
        }
        if !label
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return bad("use lowercase letters, digits and '-' only");
        }
    }
    Ok(())
}

/// §48: expands a template like `api.{project}.test` for a project name.
pub fn apply_template(template: &str, project_name: &str) -> String {
    template.replace("{project}", &slugify(project_name))
}

/// Lowercase, alphanumerics and single dashes — a folder name like "My Shop_2" → "my-shop-2".
pub fn slugify(name: &str) -> String {
    let mut out = String::new();
    let mut last_dash = true;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    out.trim_end_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn domain(host: &str) -> Domain {
        Domain {
            hostname: host.into(),
            project_id: None,
            root: "C:/sites/x".into(),
            kind: SiteKind::Static,
            https: true,
            redirect_https: true,
            wildcard: false,
            enabled: true,
            ownership: Ownership::Managed,
            app: None,
            blocks: SiteBlocks::default(),
            generated_hashes: BTreeMap::new(),
            public_domain: None,
            tunnel_id: None,
            server: None,
        }
    }

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
        assert!(html.contains("Open Local Server") && html.contains("Local Domains"));
        assert!(home
            .paths
            .data_dir()
            .join("home")
            .join("logo.svg")
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
}
