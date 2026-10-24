use std::path::{Path, PathBuf};
use super::types::*;
use crate::error::CoreError;

pub fn ensure_site_dir_and_welcome_file(domain: &Domain) -> std::io::Result<()> {
    match &domain.kind {
        SiteKind::Proxy { .. } => Ok(()),
        SiteKind::Static => {
            let path = Path::new(&domain.root);
            std::fs::create_dir_all(path)?;
            let index_html = path.join("index.html");
            let index_php = path.join("index.php");
            let index_htm = path.join("index.htm");
            if !index_html.exists() && !index_php.exists() && !index_htm.exists() {
                let content = static_welcome_page(&domain.hostname, &domain.root);
                std::fs::write(index_html, content)?;
            }
            Ok(())
        }
        SiteKind::Php { version: _ } => {
            let path = Path::new(&domain.root);
            std::fs::create_dir_all(path)?;
            let index_php = path.join("index.php");
            let index_html = path.join("index.html");
            let index_htm = path.join("index.htm");
            if !index_php.exists() && !index_html.exists() && !index_htm.exists() {
                let content = php_welcome_page(&domain.hostname, &domain.root);
                std::fs::write(index_php, content)?;
            }
            Ok(())
        }
    }
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

/// Generates a self-contained, responsive welcome page for a newly created static site.
fn static_welcome_page(hostname: &str, root: &str) -> String {
    let host = escape_html(hostname);
    let doc_root = escape_html(root);
    format!(
        r##"<!doctype html>
<html lang="en" data-theme="light">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{host} — Open Local Server</title>
<link rel="icon" type="image/svg+xml" href="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 32 32' fill='none'%3E%3Crect width='32' height='32' rx='8' fill='%230d9488'/%3E%3Crect x='6' y='7' width='20' height='5' rx='1.5' fill='white' fill-opacity='0.95'/%3E%3Crect x='6' y='14' width='20' height='5' rx='1.5' fill='white' fill-opacity='0.95'/%3E%3Crect x='6' y='21' width='20' height='5' rx='1.5' fill='white' fill-opacity='0.95'/%3E%3Ccircle cx='9.5' cy='9.5' r='1' fill='%230d9488'/%3E%3Ccircle cx='9.5' cy='16.5' r='1' fill='%230d9488'/%3E%3Ccircle cx='9.5' cy='23.5' r='1' fill='%230d9488'/%3E%3C/svg%3E">
<style>
  :root {{
    --teal: #0d9488; --teal-dark: #0b6e64; --ink: #0f2e2b; --muted: #5b6f6c;
    --bg: #f4faf8; --card: #ffffff; --line: #dcebe7; --soft: #e6f5f1;
  }}
  [data-theme="dark"] {{
    --ink: #d7efeb; --muted: #93a8a4;
    --bg: #0b1514; --card: #12201e; --line: #223836; --soft: #142625;
  }}
  * {{ box-sizing: border-box; }}
  body {{ margin: 0; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; background: var(--bg); color: var(--ink); transition: background .25s, color .25s; min-height: 100vh; display: flex; flex-direction: column; }}
  a {{ color: var(--teal); }}
  .wrap {{ max-width: 920px; margin: 0 auto; padding: 0 24px; width: 100%; }}
  header.top {{ display: flex; align-items: center; justify-content: space-between; padding: 18px 0; border-bottom: 1px solid var(--line); }}
  .brand {{ display: flex; align-items: center; gap: 10px; font-weight: 700; font-size: 16px; text-decoration: none; color: var(--ink); }}
  .brand svg {{ width: 28px; height: 28px; border-radius: 7px; flex-shrink: 0; }}
  nav.top {{ display: flex; align-items: center; gap: 4px; }}
  nav.top a, nav.top button {{ font-size: 13.5px; color: var(--muted); text-decoration: none; background: none; border: 0; cursor: pointer; padding: 8px 12px; border-radius: 8px; font-family: inherit; }}
  nav.top a:hover, nav.top button:hover {{ background: var(--soft); color: var(--ink); }}
  main {{ flex: 1; padding: 48px 0 24px; }}
  .hero {{ text-align: center; }}
  .badge {{ display: inline-flex; align-items: center; gap: 6px; background: var(--soft); color: var(--teal-dark); font-size: 13px; font-weight: 600; padding: 6px 14px; border-radius: 999px; border: 1px solid var(--line); margin-bottom: 16px; }}
  [data-theme="dark"] .badge {{ color: #5eead4; }}
  .hero h1 {{ font-size: clamp(32px, 5.5vw, 52px); font-weight: 800; letter-spacing: -1.5px; margin: 0 0 12px; }}
  .hero h1 .os {{ color: var(--ink); }} .hero h1 .ls {{ color: var(--teal); }}
  .hero p.sub {{ max-width: 580px; margin: 0 auto 20px; color: var(--muted); font-size: 16px; line-height: 1.6; }}
  .status {{ display: inline-flex; align-items: center; gap: 8px; font-size: 13px; font-weight: 500; color: var(--muted); padding: 5px 14px; background: var(--card); border: 1px solid var(--line); border-radius: 999px; }}
  .status .dot {{ color: #22c55e; font-size: 14px; }}
  .cards {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 14px; margin: 36px 0; }}
  .card {{ background: var(--card); border: 1px solid var(--line); border-radius: 12px; padding: 18px 20px; text-align: left; }}
  .card .lbl {{ font-size: 11.5px; font-weight: 600; text-transform: uppercase; letter-spacing: 0.5px; color: var(--muted); margin-bottom: 6px; }}
  .card .val {{ font-size: 14px; font-weight: 600; color: var(--ink); word-break: break-all; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; }}
  .cta {{ display: flex; gap: 12px; justify-content: center; flex-wrap: wrap; margin: 28px 0; }}
  .btn {{ display: inline-flex; align-items: center; gap: 8px; font-size: 14px; font-weight: 600; padding: 10px 22px; border-radius: 10px; text-decoration: none; border: 1px solid transparent; cursor: pointer; }}
  .btn.primary {{ background: var(--teal); color: #fff; box-shadow: 0 2px 10px rgba(13,148,136,.3); }}
  .btn.primary:hover {{ background: var(--teal-dark); }}
  .btn.ghost {{ border-color: var(--line); color: var(--ink); background: var(--card); }}
  .btn.ghost:hover {{ background: var(--soft); }}
  .next-steps {{ background: var(--card); border: 1px solid var(--line); border-radius: 14px; padding: 28px; margin: 28px 0; text-align: left; }}
  .next-steps h3 {{ margin: 0 0 6px; font-size: 18px; }}
  .next-steps p.lead {{ margin: 0 0 20px; color: var(--muted); font-size: 14px; }}
  .steps-grid {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); gap: 18px; }}
  .step-item {{ padding: 4px; }}
  .step-item .num {{ font-size: 12px; font-weight: 800; color: var(--teal); letter-spacing: 1px; }}
  .step-item h4 {{ margin: 6px 0 4px; font-size: 15px; }}
  .step-item p {{ margin: 0; color: var(--muted); font-size: 13.5px; line-height: 1.5; }}
  .step-item code {{ font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 12px; background: var(--soft); padding: 2px 6px; border-radius: 4px; }}
  footer {{ border-top: 1px solid var(--line); padding: 20px 0 32px; font-size: 13px; color: var(--muted); }}
  footer .wrap {{ display: flex; align-items: center; justify-content: space-between; gap: 12px; flex-wrap: wrap; }}
  footer nav {{ display: flex; gap: 4px; }}
  footer nav a {{ color: var(--muted); text-decoration: none; padding: 4px 8px; border-radius: 6px; }}
  footer nav a:hover {{ background: var(--soft); color: var(--ink); }}
</style>
</head>
<body>
<div class="wrap">
  <header class="top">
    <a class="brand" href="http://openlocalserver.test">
      <svg viewBox="0 0 32 32" fill="none" xmlns="http://www.w3.org/2000/svg">
        <rect width="32" height="32" rx="8" fill="#0d9488"/>
        <rect x="6" y="7" width="20" height="5" rx="1.5" fill="white" fill-opacity="0.95"/>
        <rect x="6" y="14" width="20" height="5" rx="1.5" fill="white" fill-opacity="0.95"/>
        <rect x="6" y="21" width="20" height="5" rx="1.5" fill="white" fill-opacity="0.95"/>
        <circle cx="9.5" cy="9.5" r="1" fill="#0d9488"/>
        <circle cx="9.5" cy="16.5" r="1" fill="#0d9488"/>
        <circle cx="9.5" cy="23.5" r="1" fill="#0d9488"/>
      </svg>
      Open Local Server
    </a>
    <nav class="top">
      <a href="http://openlocalserver.test">Local Server</a>
      <a href="https://github.com/kz370/OpenLocalServer">Documentation</a>
      <button id="theme" aria-label="Toggle theme">◐</button>
    </nav>
  </header>
</div>
<main class="wrap">
  <div class="hero">
    <span class="badge">⚡ Static Site Ready · Open Local Server</span>
    <h1><span class="os">Welcome to</span> <span class="ls">{host}</span></h1>
    <p class="sub">Your local static website is up and running on Open Local Server.</p>
    <div class="status"><span class="dot">●</span> Local Environment Active</div>
  </div>

  <div class="cards">
    <div class="card">
      <div class="lbl">Domain</div>
      <div class="val">{host}</div>
    </div>
    <div class="card">
      <div class="lbl">Site Type</div>
      <div class="val">Static (HTML / CSS / JS)</div>
    </div>
    <div class="card">
      <div class="lbl">Document Root</div>
      <div class="val">{doc_root}</div>
    </div>
    <div class="card">
      <div class="lbl">Web Server</div>
      <div class="val">Open Local Server</div>
    </div>
  </div>

  <div class="cta">
    <a class="btn primary" href="http://openlocalserver.test">Open Local Server</a>
    <a class="btn ghost" href="https://github.com/kz370/OpenLocalServer">Documentation</a>
  </div>

  <section class="next-steps">
    <h3>Getting Started</h3>
    <p class="lead">Next steps for developing your static website.</p>
    <div class="steps-grid">
      <div class="step-item">
        <div class="num">01</div>
        <h4>Add your files</h4>
        <p>Edit or replace <code>index.html</code> in your document root with your project's HTML, stylesheets, and scripts.</p>
      </div>
      <div class="step-item">
        <div class="num">02</div>
        <h4>Configure in OLS</h4>
        <p>Switch between Nginx, Apache, or Caddy web servers, set up SSL certificates, or add domain aliases.</p>
      </div>
      <div class="step-item">
        <div class="num">03</div>
        <h4>Share &amp; Broadcast</h4>
        <p>Use built-in Cloudflare tunnels to securely broadcast your local site to the internet with one click.</p>
      </div>
    </div>
  </section>
</main>
<footer>
  <div class="wrap">
    <div class="brand">Open Local Server</div>
    <nav>
      <a href="http://openlocalserver.test">Local Server</a>
      <a href="https://github.com/kz370/OpenLocalServer">GitHub</a>
      <a href="https://github.com/kz370/OpenLocalServer/issues">Support</a>
    </nav>
    <span>Local development, simplified.</span>
  </div>
</footer>
<script>
(function () {{
  var key = 'ols-site-theme';
  function paint(t) {{
    document.documentElement.setAttribute('data-theme', t);
    try {{ localStorage.setItem(key, t); }} catch (e) {{}}
  }}
  var t;
  try {{ t = localStorage.getItem(key); }} catch (e) {{}}
  if (t !== 'dark' && t !== 'light') {{
    t = (window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches) ? 'dark' : 'light';
  }}
  paint(t);
  var b = document.getElementById('theme');
  if (b) {{
    b.addEventListener('click', function () {{
      var cur = document.documentElement.getAttribute('data-theme') || 'light';
      paint(cur === 'dark' ? 'light' : 'dark');
    }});
  }}
}})();
</script>
</body>
</html>
"##
    )
}

/// Generates a self-contained, responsive welcome page for a newly created PHP site.
fn php_welcome_page(hostname: &str, root: &str) -> String {
    let host = escape_html(hostname);
    let _doc_root = escape_html(root);
    format!(
        r##"<?php
if (isset($_GET['phpinfo'])) {{
    phpinfo();
    exit;
}}
$host = htmlspecialchars($_SERVER['HTTP_HOST'] ?? '{host}');
$php_ver = htmlspecialchars(PHP_VERSION);
$sapi = htmlspecialchars(php_sapi_name());
$server_sw = htmlspecialchars($_SERVER['SERVER_SOFTWARE'] ?? 'Open Local Server');
$doc_root = htmlspecialchars(__DIR__);
?>
<!doctype html>
<html lang="en" data-theme="light">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title><?php echo $host; ?> — Open Local Server</title>
<link rel="icon" type="image/svg+xml" href="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 32 32' fill='none'%3E%3Crect width='32' height='32' rx='8' fill='%230d9488'/%3E%3Crect x='6' y='7' width='20' height='5' rx='1.5' fill='white' fill-opacity='0.95'/%3E%3Crect x='6' y='14' width='20' height='5' rx='1.5' fill='white' fill-opacity='0.95'/%3E%3Crect x='6' y='21' width='20' height='5' rx='1.5' fill='white' fill-opacity='0.95'/%3E%3Ccircle cx='9.5' cy='9.5' r='1' fill='%230d9488'/%3E%3Ccircle cx='9.5' cy='16.5' r='1' fill='%230d9488'/%3E%3Ccircle cx='9.5' cy='23.5' r='1' fill='%230d9488'/%3E%3C/svg%3E">
<style>
  :root {{
    --teal: #0d9488; --teal-dark: #0b6e64; --ink: #0f2e2b; --muted: #5b6f6c;
    --bg: #f4faf8; --card: #ffffff; --line: #dcebe7; --soft: #e6f5f1;
  }}
  [data-theme="dark"] {{
    --ink: #d7efeb; --muted: #93a8a4;
    --bg: #0b1514; --card: #12201e; --line: #223836; --soft: #142625;
  }}
  * {{ box-sizing: border-box; }}
  body {{ margin: 0; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; background: var(--bg); color: var(--ink); transition: background .25s, color .25s; min-height: 100vh; display: flex; flex-direction: column; }}
  a {{ color: var(--teal); }}
  .wrap {{ max-width: 920px; margin: 0 auto; padding: 0 24px; width: 100%; }}
  header.top {{ display: flex; align-items: center; justify-content: space-between; padding: 18px 0; border-bottom: 1px solid var(--line); }}
  .brand {{ display: flex; align-items: center; gap: 10px; font-weight: 700; font-size: 16px; text-decoration: none; color: var(--ink); }}
  .brand svg {{ width: 28px; height: 28px; border-radius: 7px; flex-shrink: 0; }}
  nav.top {{ display: flex; align-items: center; gap: 4px; }}
  nav.top a, nav.top button {{ font-size: 13.5px; color: var(--muted); text-decoration: none; background: none; border: 0; cursor: pointer; padding: 8px 12px; border-radius: 8px; font-family: inherit; }}
  nav.top a:hover, nav.top button:hover {{ background: var(--soft); color: var(--ink); }}
  main {{ flex: 1; padding: 48px 0 24px; }}
  .hero {{ text-align: center; }}
  .badge {{ display: inline-flex; align-items: center; gap: 6px; background: var(--soft); color: var(--teal-dark); font-size: 13px; font-weight: 600; padding: 6px 14px; border-radius: 999px; border: 1px solid var(--line); margin-bottom: 16px; }}
  [data-theme="dark"] .badge {{ color: #5eead4; }}
  .hero h1 {{ font-size: clamp(32px, 5.5vw, 52px); font-weight: 800; letter-spacing: -1.5px; margin: 0 0 12px; }}
  .hero h1 .os {{ color: var(--ink); }} .hero h1 .ls {{ color: var(--teal); }}
  .hero p.sub {{ max-width: 580px; margin: 0 auto 20px; color: var(--muted); font-size: 16px; line-height: 1.6; }}
  .status {{ display: inline-flex; align-items: center; gap: 8px; font-size: 13px; font-weight: 500; color: var(--muted); padding: 5px 14px; background: var(--card); border: 1px solid var(--line); border-radius: 999px; }}
  .status .dot {{ color: #22c55e; font-size: 14px; }}
  .cards {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 14px; margin: 36px 0; }}
  .card {{ background: var(--card); border: 1px solid var(--line); border-radius: 12px; padding: 18px 20px; text-align: left; }}
  .card .lbl {{ font-size: 11.5px; font-weight: 600; text-transform: uppercase; letter-spacing: 0.5px; color: var(--muted); margin-bottom: 6px; }}
  .card .val {{ font-size: 14px; font-weight: 600; color: var(--ink); word-break: break-all; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; }}
  .cta {{ display: flex; gap: 12px; justify-content: center; flex-wrap: wrap; margin: 28px 0; }}
  .btn {{ display: inline-flex; align-items: center; gap: 8px; font-size: 14px; font-weight: 600; padding: 10px 22px; border-radius: 10px; text-decoration: none; border: 1px solid transparent; cursor: pointer; }}
  .btn.primary {{ background: var(--teal); color: #fff; box-shadow: 0 2px 10px rgba(13,148,136,.3); }}
  .btn.primary:hover {{ background: var(--teal-dark); }}
  .btn.ghost {{ border-color: var(--line); color: var(--ink); background: var(--card); }}
  .btn.ghost:hover {{ background: var(--soft); }}
  .next-steps {{ background: var(--card); border: 1px solid var(--line); border-radius: 14px; padding: 28px; margin: 28px 0; text-align: left; }}
  .next-steps h3 {{ margin: 0 0 6px; font-size: 18px; }}
  .next-steps p.lead {{ margin: 0 0 20px; color: var(--muted); font-size: 14px; }}
  .steps-grid {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); gap: 18px; }}
  .step-item {{ padding: 4px; }}
  .step-item .num {{ font-size: 12px; font-weight: 800; color: var(--teal); letter-spacing: 1px; }}
  .step-item h4 {{ margin: 6px 0 4px; font-size: 15px; }}
  .step-item p {{ margin: 0; color: var(--muted); font-size: 13.5px; line-height: 1.5; }}
  .step-item code {{ font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 12px; background: var(--soft); padding: 2px 6px; border-radius: 4px; }}
  footer {{ border-top: 1px solid var(--line); padding: 20px 0 32px; font-size: 13px; color: var(--muted); }}
  footer .wrap {{ display: flex; align-items: center; justify-content: space-between; gap: 12px; flex-wrap: wrap; }}
  footer nav {{ display: flex; gap: 4px; }}
  footer nav a {{ color: var(--muted); text-decoration: none; padding: 4px 8px; border-radius: 6px; }}
  footer nav a:hover {{ background: var(--soft); color: var(--ink); }}
</style>
</head>
<body>
<div class="wrap">
  <header class="top">
    <a class="brand" href="http://openlocalserver.test">
      <svg viewBox="0 0 32 32" fill="none" xmlns="http://www.w3.org/2000/svg">
        <rect width="32" height="32" rx="8" fill="#0d9488"/>
        <rect x="6" y="7" width="20" height="5" rx="1.5" fill="white" fill-opacity="0.95"/>
        <rect x="6" y="14" width="20" height="5" rx="1.5" fill="white" fill-opacity="0.95"/>
        <rect x="6" y="21" width="20" height="5" rx="1.5" fill="white" fill-opacity="0.95"/>
        <circle cx="9.5" cy="9.5" r="1" fill="#0d9488"/>
        <circle cx="9.5" cy="16.5" r="1" fill="#0d9488"/>
        <circle cx="9.5" cy="23.5" r="1" fill="#0d9488"/>
      </svg>
      Open Local Server
    </a>
    <nav class="top">
      <a href="http://openlocalserver.test">Local Server</a>
      <a href="https://github.com/kz370/OpenLocalServer">Documentation</a>
      <button id="theme" aria-label="Toggle theme">◐</button>
    </nav>
  </header>
</div>
<main class="wrap">
  <div class="hero">
    <span class="badge">🐘 PHP Site Ready · Open Local Server</span>
    <h1><span class="os">Welcome to</span> <span class="ls"><?php echo $host; ?></span></h1>
    <p class="sub">Your local PHP website is up and running on Open Local Server.</p>
    <div class="status"><span class="dot">●</span> Local Environment Active</div>
  </div>

  <div class="cards">
    <div class="card">
      <div class="lbl">Domain</div>
      <div class="val"><?php echo $host; ?></div>
    </div>
    <div class="card">
      <div class="lbl">PHP Version</div>
      <div class="val"><?php echo $php_ver; ?> (<?php echo $sapi; ?>)</div>
    </div>
    <div class="card">
      <div class="lbl">Document Root</div>
      <div class="val"><?php echo $doc_root; ?></div>
    </div>
    <div class="card">
      <div class="lbl">Web Server</div>
      <div class="val"><?php echo $server_sw; ?></div>
    </div>
  </div>

  <div class="cta">
    <a class="btn primary" href="?phpinfo=1">View PHP Info</a>
    <a class="btn ghost" href="http://openlocalserver.test">Open Local Server</a>
  </div>

  <section class="next-steps">
    <h3>Getting Started</h3>
    <p class="lead">Next steps for developing your PHP application.</p>
    <div class="steps-grid">
      <div class="step-item">
        <div class="num">01</div>
        <h4>Write your PHP code</h4>
        <p>Open <code>index.php</code> in this folder to start coding or replace it with your application framework files.</p>
      </div>
      <div class="step-item">
        <div class="num">02</div>
        <h4>Manage PHP in OLS</h4>
        <p>Switch PHP versions per site, enable extensions (Xdebug, curl, gd, etc.), and adjust FastCGI pool settings.</p>
      </div>
      <div class="step-item">
        <div class="num">03</div>
        <h4>Databases &amp; Tools</h4>
        <p>Create MySQL, MariaDB, PostgreSQL or SQLite databases in OLS and open them with HeidiSQL or phpMyAdmin.</p>
      </div>
    </div>
  </section>
</main>
<footer>
  <div class="wrap">
    <div class="brand">Open Local Server</div>
    <nav>
      <a href="http://openlocalserver.test">Local Server</a>
      <a href="https://github.com/kz370/OpenLocalServer">GitHub</a>
      <a href="https://github.com/kz370/OpenLocalServer/issues">Support</a>
    </nav>
    <span>Local development, simplified.</span>
  </div>
</footer>
<script>
(function () {{
  var key = 'ols-site-theme';
  function paint(t) {{
    document.documentElement.setAttribute('data-theme', t);
    try {{ localStorage.setItem(key, t); }} catch (e) {{}}
  }}
  var t;
  try {{ t = localStorage.getItem(key); }} catch (e) {{}}
  if (t !== 'dark' && t !== 'light') {{
    t = (window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches) ? 'dark' : 'light';
  }}
  paint(t);
  var b = document.getElementById('theme');
  if (b) {{
    b.addEventListener('click', function () {{
      var cur = document.documentElement.getAttribute('data-theme') || 'light';
      paint(cur === 'dark' ? 'light' : 'dark');
    }});
  }}
}})();
</script>
</body>
</html>
"##
    )
}
