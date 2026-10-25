use super::types::*;
use crate::paths::AppPaths;
use std::collections::BTreeMap;
use std::path::Path;

/// The site every fresh install starts with: a static welcome page.
pub const HOME_HOSTNAME: &str = "openlocalserver.test";
/// Hostname seeded by older installs; renamed to [`HOME_HOSTNAME`] on load.
pub(crate) const LEGACY_HOME_HOSTNAME: &str = "home.test";

/// A fresh install (no `domains.json` yet) gets `openlocalserver.test`: a static,
/// managed, HTTPS site serving a welcome page from the data dir. A missing
/// file is the only trigger — deleting the site afterwards is respected.
pub(crate) fn home_domain(dir: &Path) -> Domain {
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
        path_prefix: None,
        icon: None,
        ssl_provider: SslProvider::LocalCa,
        custom_cert_path: None,
        custom_key_path: None,
        lets_encrypt_email: None,
        auto_broadcast: false,
        broadcast_provider: None,
        ..Default::default()
    }
}

/// Writes the welcome page. Never overwrites: hand edits survive updates.
pub(crate) fn write_home_page(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let index = dir.join("index.html");
    if !index.exists() {
        std::fs::write(&index, HOME_PAGE)?;
    }
    // The exact official logo bytes (ui/public/favicon*.svg, also the in-app mark)
    // so the page reuses the real brand, never a redraw. Both colourways are
    // written because the page has its own theme toggle and no way to be told
    // which one is active.
    //
    // A missing logo is always written. An existing one is rewritten only while
    // the page is still the stock design *this* build ships — the logo is a
    // generated asset, so a repaint has to reach an install that already has one,
    // or every existing user keeps the old mark forever while a fresh install
    // gets the new one. Once someone edits the page the marker is gone, and from
    // then on the logo is theirs and is left alone, same as the page itself.
    let stock_page = std::fs::read_to_string(&index)
        .map(|html| html.contains(HOME_VERSION_MARKER))
        .unwrap_or(false);
    for (name, bytes) in [("logo.svg", LOGO_SVG), ("logo-dark.svg", LOGO_SVG_DARK)] {
        let path = dir.join(name);
        let current = std::fs::read(&path).ok();
        if current.as_deref() != Some(bytes) && (current.is_none() || stock_page) {
            std::fs::write(path, bytes)?;
        }
    }
    Ok(())
}

/// Exact bytes of the official application logo, per theme. The welcome page is a
/// plain static page with its own theme toggle, so it carries both colourways
/// rather than being told which one is active.
pub(crate) const LOGO_SVG: &[u8] = include_bytes!("../../../../ui/public/favicon.svg");
pub(crate) const LOGO_SVG_DARK: &[u8] = include_bytes!("../../../../ui/public/favicon-dark.svg");

/// Marker of the currently shipped welcome page.
pub(crate) const HOME_VERSION_MARKER: &str = "<!-- home v9 -->";

/// Markers of older shipped pages (v1 teal cards, v1.5 centered hero, v2 brand,
/// v3 brand, v4 brand, v5 brand, v6 pre-rebrand brand, v7 acronym tab title with
/// no tab icon, v8 expanded title and themed tab icon). A page carrying one of
/// these was never hand-edited, so it is safe to refresh — v9 is what names the
/// product in full in the header and footer instead of the bare acronym. v6 is
/// listed because the rebrand changed only the wording, so a v6 page is still
/// stock: an install that never opened its welcome site should see the new
/// design, and a page a user edited should be left alone.
const HOME_LEGACY_MARKERS: &[&str] = &[
    "max-width: 720px",
    "OPENLOCALSERVER",
    "<h1>home.test</h1>",
    "created once, on first install",
    "<!-- home v3 -->",
    "<!-- home v4 -->",
    "<!-- home v5 -->",
    "<!-- home v6 -->",
    "<!-- home v7 -->",
    "<!-- home v8 -->",
];

/// Replaces a stock older welcome page with the current one. Returns true when
/// it wrote. Hand-edited pages and io failures are silently kept as-is: this
/// must never break startup over a cosmetic file.
pub(crate) fn upgrade_home_page(paths: &AppPaths) -> bool {
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
        .and_then(|_| std::fs::write(dir.join("logo-dark.svg"), LOGO_SVG_DARK))
        .and_then(|_| std::fs::write(&index, HOME_PAGE))
        .is_ok()
}

/// Static welcome page for `openlocalserver.test`. Self-contained except the
/// seeded `logo.svg` (exact official brand bytes); works offline.
pub(crate) const HOME_PAGE: &str = r##"<!doctype html>
<html lang="en" data-theme="light">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Open Local Server — local development, simplified</title>
<!-- The tab icon follows the page theme, the same swap the marks do, so a dark
     window never shows a tab the browser has to invert. A `media` query cannot
     do it on its own: the toggle overrides the OS setting, so the href is set in
     the head before first paint, from the stored choice when there is one. -->
<link id="favicon" rel="icon" type="image/svg+xml" href="logo.svg">
<script>
(function () {
  var t;
  try { t = localStorage.getItem('ols-home-theme'); } catch (e) {}
  if (t !== 'dark' && t !== 'light' && window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches) t = 'dark';
  document.getElementById('favicon').setAttribute('href', t === 'dark' ? 'logo-dark.svg' : 'logo.svg');
})();
</script>
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
<!-- home v9 -->
<div class="wrap">
  <header class="top">
    <div class="brand"><img data-mark src="logo.svg" alt="OLS logo">Open Local Server</div>
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
    <div class="core"><img data-mark src="logo.svg" alt=""></div>
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
    <p class="lead">Three steps, all inside OLS.</p>
    <div class="steps">
      <div class="step"><div class="num">01</div><h4>Add your project</h4><p>Choose a project folder and let OLS configure it.</p></div>
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
    <div class="brand"><img data-mark src="logo.svg" alt="OLS logo">Open Local Server</div>
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
  function paint(t) {
    document.documentElement.setAttribute('data-theme', t);
    // The mark ships in both colourways so it stays readable on the theme the page
    // is actually painted in — the same swap the app, tray and taskbar do.
    var src = t === 'dark' ? 'logo-dark.svg' : 'logo.svg';
    var marks = document.querySelectorAll('img[data-mark]');
    for (var i = 0; i < marks.length; i++) marks[i].setAttribute('src', src);
    var fav = document.getElementById('favicon');
    if (fav) fav.setAttribute('href', src);
  }
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
