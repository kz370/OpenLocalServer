//! Import sites from Laragon, XAMPP and WampServer (§172). `migrate.rs` brings their
//! databases; this brings their sites (a domain + a folder). **Read-only on the old tools**:
//! nothing in their folders or configs is written, renamed or deleted, and `Include`
//! directives are never followed.

use std::collections::HashSet;
use std::net::IpAddr;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::app::Inner;
use crate::domain::store::validate_hostname;
use crate::domain::{Domain, SiteKind};
use crate::error::CoreError;
use crate::tasks::TaskHandle;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SiteImportCandidate {
    /// `Laragon (C:\laragon)`.
    pub source: String,
    pub hostname: String,
    pub aliases: Vec<String>,
    pub document_root: String,
    pub project_root: String,
    /// The document root exists as a folder.
    pub exists: bool,
    pub already_in_ols: bool,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteImportChoice {
    pub hostname: String,
    pub document_root: String,
    #[serde(default)]
    pub new_hostname: Option<String>,
}

/// One virtual host as written in a config file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawVhost {
    /// `ServerName` first, then aliases.
    pub names: Vec<String>,
    pub document_root: Option<String>,
}

// ------------------------------------------------------------------ parsers (pure)

/// Splits a directive's value into words; `"quoted words"` and `'quoted'` stay together.
fn split_args(rest: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut has = false;
    for c in rest.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => cur.push(c),
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                has = true;
            }
            None if c.is_whitespace() => {
                if has || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            None => cur.push(c),
        }
    }
    if has || !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Apache `httpd-vhosts.conf` style. `Include` is not followed; unclosed blocks are dropped.
pub fn parse_apache_vhosts(text: &str) -> Vec<RawVhost> {
    let mut out = Vec::new();
    let mut cur: Option<RawVhost> = None;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let lower = line.to_lowercase();
        if lower.starts_with("<virtualhost") {
            cur = Some(RawVhost::default());
            continue;
        }
        if lower.starts_with("</virtualhost") {
            if let Some(v) = cur.take() {
                out.push(v);
            }
            continue;
        }
        let Some(v) = cur.as_mut() else { continue };
        let (directive, rest) = match line.split_once(char::is_whitespace) {
            Some((d, r)) => (d.to_lowercase(), r.trim()),
            None => continue,
        };
        let args = split_args(rest);
        match directive.as_str() {
            "servername" => {
                if let Some(n) = args.first() {
                    v.names.insert(0, n.clone());
                }
            }
            "serveralias" => v.names.extend(args),
            "documentroot" => {
                if let Some(p) = args.into_iter().next() {
                    v.document_root = Some(p);
                }
            }
            _ => {}
        }
    }
    out
}

#[derive(Debug, PartialEq, Eq)]
enum Tok {
    Word(String),
    Semi,
    Open,
    Close,
}

fn nginx_tokens(text: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut has = false;
    let mut chars = text.chars().peekable();
    let flush = |cur: &mut String, has: &mut bool, out: &mut Vec<Tok>| {
        if *has || !cur.is_empty() {
            out.push(Tok::Word(std::mem::take(cur)));
            *has = false;
        }
    };
    while let Some(c) = chars.next() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => cur.push(c),
            None => match c {
                '"' | '\'' => {
                    quote = Some(c);
                    has = true;
                }
                '#' => {
                    flush(&mut cur, &mut has, &mut out);
                    for n in chars.by_ref() {
                        if n == '\n' {
                            break;
                        }
                    }
                }
                ';' => {
                    flush(&mut cur, &mut has, &mut out);
                    out.push(Tok::Semi);
                }
                '{' => {
                    flush(&mut cur, &mut has, &mut out);
                    out.push(Tok::Open);
                }
                '}' => {
                    flush(&mut cur, &mut has, &mut out);
                    out.push(Tok::Close);
                }
                c if c.is_whitespace() => flush(&mut cur, &mut has, &mut out),
                c => cur.push(c),
            },
        }
    }
    flush(&mut cur, &mut has, &mut out);
    out
}

/// nginx config: reads `server_name` and `root` directly inside `server { }`.
/// `location { }` blocks and `include` are ignored.
pub fn parse_nginx_servers(text: &str) -> Vec<RawVhost> {
    let mut out = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let mut cur: Option<RawVhost> = None;
    let mut directive: Vec<String> = Vec::new();
    for tok in nginx_tokens(text) {
        match tok {
            Tok::Word(w) => directive.push(w),
            Tok::Open => {
                let kind = directive.first().cloned().unwrap_or_default();
                if kind == "server" && stack.iter().all(|k| k != "server") {
                    cur = Some(RawVhost::default());
                }
                stack.push(kind);
                directive.clear();
            }
            Tok::Close => {
                directive.clear();
                if stack.pop().as_deref() == Some("server") && stack.iter().all(|k| k != "server") {
                    if let Some(v) = cur.take() {
                        out.push(v);
                    }
                }
            }
            Tok::Semi => {
                if stack.last().map(String::as_str) == Some("server") {
                    if let (Some(v), Some(name)) = (cur.as_mut(), directive.first()) {
                        match name.as_str() {
                            "server_name" => v.names.extend(directive[1..].iter().cloned()),
                            "root" => v.document_root = directive.get(1).cloned(),
                            _ => {}
                        }
                    }
                }
                directive.clear();
            }
        }
    }
    out
}

/// Expands the old tool's own root variable; any other variable means the path is unknown.
pub fn expand_vars(path: &str, tool_root: &Path) -> Result<String, String> {
    let root = tool_root.display().to_string();
    let mut p = path.to_string();
    for var in [
        "${LARAGON_ROOT}",
        "%LARAGON_ROOT%",
        "${SRVROOT}",
        "%SRVROOT%",
    ] {
        p = p.replace(var, &root);
    }
    if p.contains('$') || p.contains("%") {
        return Err("the folder uses a variable OLS cannot expand".into());
    }
    Ok(p.replace('/', "\\"))
}

/// Laragon's `HostnameFormat`: `{name}.test` by default. Only `{name}` is substituted;
/// anything else returns `None`.
pub fn parse_hostname_format(ini: &str) -> Option<String> {
    for line in ini.lines() {
        let line = line.trim();
        if let Some((k, v)) = line.split_once('=') {
            if k.trim().eq_ignore_ascii_case("HostnameFormat") {
                let v = v.trim().trim_matches('"').trim_matches('\'').to_string();
                let ok = v.matches("{name}").count() == 1
                    && !v.replace("{name}", "").contains(['{', '}']);
                return ok.then_some(v);
            }
        }
    }
    Some("{name}.test".into())
}

fn clean_hostname(name: &str) -> Option<String> {
    let mut n = name.trim().to_lowercase();
    // `shop.test:80`
    if let Some((h, port)) = n.rsplit_once(':') {
        if port.chars().all(|c| c.is_ascii_digit()) {
            n = h.to_string();
        }
    }
    if matches!(n.as_str(), "" | "localhost" | "_" | "*" | "_default_")
        || n.parse::<IpAddr>().is_ok()
    {
        return None;
    }
    validate_hostname(&n).ok().map(|_| n)
}

/// `…\public` → its parent; `htdocs\x\public` too. Otherwise the document root itself.
pub fn project_root_of(document_root: &str) -> String {
    let p = Path::new(document_root);
    let last = p
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if matches!(last.as_str(), "public" | "web" | "public_html") {
        if let Some(parent) = p.parent() {
            return parent.display().to_string();
        }
    }
    document_root.to_string()
}

// ------------------------------------------------------------------ discovery

/// Existing Laragon / XAMPP / WampServer folders on every drive: (tool, root).
pub(crate) fn tool_roots() -> Vec<(&'static str, PathBuf)> {
    let mut out = Vec::new();
    for drive in b'C'..=b'Z' {
        let base = PathBuf::from(format!("{}:\\", drive as char));
        if !base.exists() {
            continue;
        }
        for (tool, dir) in [
            ("Laragon", "laragon"),
            ("XAMPP", "xampp"),
            ("WampServer", "wamp64"),
            ("WampServer", "wamp"),
        ] {
            let root = base.join(dir);
            if root.is_dir() {
                out.push((tool, root));
            }
        }
    }
    out
}

fn read_confs(dir: &Path) -> Vec<String> {
    let mut texts = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        let mut files: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "conf"))
            .collect();
        files.sort();
        for f in files {
            if let Ok(t) = std::fs::read_to_string(&f) {
                texts.push(t);
            }
        }
    }
    texts
}

fn push_candidate(
    out: &mut Vec<SiteImportCandidate>,
    seen: &mut HashSet<String>,
    existing: &HashSet<String>,
    source: &str,
    tool_root: &Path,
    vhost: &RawVhost,
) {
    let Some(first) = vhost.names.iter().find_map(|n| clean_hostname(n)) else {
        return;
    };
    if !seen.insert(first.clone()) {
        return;
    }
    let aliases: Vec<String> = vhost
        .names
        .iter()
        .filter_map(|n| clean_hostname(n))
        .filter(|n| *n != first)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let (doc, note) = match vhost.document_root.as_deref() {
        Some(raw) => match expand_vars(raw, tool_root) {
            Ok(p) => (p, None),
            Err(why) => (raw.to_string(), Some(why)),
        },
        None => (
            String::new(),
            Some("the config has no DocumentRoot / root".into()),
        ),
    };
    let is_abs = Path::new(&doc).is_absolute();
    let exists = note.is_none() && is_abs && Path::new(&doc).is_dir();
    let note = note.or_else(|| (!exists).then(|| "folder missing".to_string()));
    out.push(SiteImportCandidate {
        source: source.to_string(),
        already_in_ols: existing.contains(&first),
        project_root: if exists {
            project_root_of(&doc)
        } else {
            doc.clone()
        },
        hostname: first,
        aliases,
        document_root: doc,
        exists,
        note,
    });
}

/// Reads the given tool roots and returns every site found. Pure over the file system.
pub fn scan_roots(
    roots: &[(&str, PathBuf)],
    existing: &HashSet<String>,
) -> Vec<SiteImportCandidate> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for (tool, root) in roots {
        let source = format!("{tool} ({})", root.display());
        match *tool {
            "Laragon" => {
                for t in read_confs(&root.join("etc/apache2/sites-enabled")) {
                    for v in parse_apache_vhosts(&t) {
                        push_candidate(&mut out, &mut seen, existing, &source, root, &v);
                    }
                }
                for t in read_confs(&root.join("etc/nginx/sites-enabled")) {
                    for v in parse_nginx_servers(&t) {
                        push_candidate(&mut out, &mut seen, existing, &source, root, &v);
                    }
                }
                // Auto virtual hosts: every folder in www becomes <name>.<tld>.
                let ini = std::fs::read_to_string(root.join("usr/laragon.ini")).unwrap_or_default();
                let format = parse_hostname_format(&ini);
                if let (Some(format), Ok(rd)) = (format, std::fs::read_dir(root.join("www"))) {
                    let mut dirs: Vec<PathBuf> = rd
                        .flatten()
                        .map(|e| e.path())
                        .filter(|p| p.is_dir())
                        .collect();
                    dirs.sort();
                    for dir in dirs {
                        let name = dir
                            .file_name()
                            .map(|n| n.to_string_lossy().to_lowercase())
                            .unwrap_or_default();
                        let host = format.replace("{name}", &name);
                        let v = RawVhost {
                            names: vec![host],
                            document_root: Some(dir.display().to_string()),
                        };
                        push_candidate(&mut out, &mut seen, existing, &source, root, &v);
                    }
                }
            }
            "XAMPP" => {
                let conf = root.join("apache/conf/extra/httpd-vhosts.conf");
                if let Ok(t) = std::fs::read_to_string(conf) {
                    for v in parse_apache_vhosts(&t) {
                        push_candidate(&mut out, &mut seen, existing, &source, root, &v);
                    }
                }
            }
            "WampServer" => {
                if let Ok(rd) = std::fs::read_dir(root.join("bin/apache")) {
                    let mut dirs: Vec<PathBuf> = rd
                        .flatten()
                        .map(|e| e.path())
                        .filter(|p| {
                            p.is_dir()
                                && p.file_name()
                                    .is_some_and(|n| n.to_string_lossy().starts_with("apache"))
                        })
                        .collect();
                    dirs.sort();
                    for d in dirs {
                        if let Ok(t) =
                            std::fs::read_to_string(d.join("conf/extra/httpd-vhosts.conf"))
                        {
                            for v in parse_apache_vhosts(&t) {
                                push_candidate(&mut out, &mut seen, existing, &source, root, &v);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn same(a: &str, b: &str) -> bool {
    a.replace('/', "\\").trim_end_matches('\\').to_lowercase()
        == b.replace('/', "\\").trim_end_matches('\\').to_lowercase()
}

impl Inner {
    fn existing_hostnames(&self) -> HashSet<String> {
        self.domains
            .lock()
            .unwrap()
            .list()
            .into_iter()
            .map(|d| d.hostname)
            .collect()
    }

    /// Sites found in installed Laragon / XAMPP / WampServer.
    pub fn list_site_import_candidates(&self) -> Vec<SiteImportCandidate> {
        scan_roots(&tool_roots(), &self.existing_hostnames())
    }

    /// Adds the chosen sites. Each item must match a site found right now (same hostname
    /// and document root), so nothing the caller invents can be added.
    pub fn import_site_candidates(
        &self,
        items: &[SiteImportChoice],
        h: &TaskHandle,
    ) -> Result<usize, CoreError> {
        let current = self.list_site_import_candidates();
        h.begin_step(&format!("Adding {} sites", items.len()));
        let mut added = 0;
        for item in items {
            h.advance(1);
            let Some(c) = current.iter().find(|c| {
                c.hostname == item.hostname && same(&c.document_root, &item.document_root)
            }) else {
                h.problem(format!(
                    "{}: that site is no longer in the old tool's config.",
                    item.hostname
                ));
                continue;
            };
            if !c.exists {
                h.problem(format!("{}: its folder is missing.", c.hostname));
                continue;
            }
            let hostname = match item.new_hostname.as_deref().map(str::trim) {
                Some(n) if !n.is_empty() => {
                    let n = n.to_lowercase();
                    if let Err(e) = validate_hostname(&n) {
                        h.problem(e.to_string());
                        continue;
                    }
                    n
                }
                _ => c.hostname.clone(),
            };
            if self.domains.lock().unwrap().get(&hostname).is_some() {
                h.problem(format!("{hostname}: already in OLS, skipped."));
                continue;
            }
            match self.add_imported_site(c, hostname.clone()) {
                Ok(()) => {
                    added += 1;
                    h.result(format!("Added {hostname}"));
                }
                Err(e) => h.problem(format!("{hostname}: {e}")),
            }
        }
        if added > 0 {
            // One web configuration pass for all the sites.
            self.apply_web(&[])?;
        }
        h.end_step(Some(format!("{added} added")));
        Ok(added)
    }

    fn add_imported_site(
        &self,
        c: &SiteImportCandidate,
        hostname: String,
    ) -> Result<(), CoreError> {
        let doc = Path::new(&c.document_root);
        let project_dir = if Path::new(&c.project_root).is_dir() {
            c.project_root.clone()
        } else {
            c.document_root.clone()
        };
        let project = self
            .projects
            .lock()
            .unwrap()
            .register_with_standalone(&project_dir, true)?;
        let pdir = Path::new(&project_dir);
        let php = doc.join("index.php").is_file()
            || pdir.join("composer.json").is_file()
            || pdir.join("artisan").is_file()
            || pdir.join("wp-config.php").is_file()
            || doc.join("wp-config.php").is_file();
        let domain = Domain {
            hostname,
            project_id: Some(project.id),
            root: c.document_root.clone(),
            kind: if php {
                SiteKind::Php { version: None }
            } else {
                SiteKind::Static
            },
            https: true,
            redirect_https: true,
            aliases: c.aliases.clone(),
            ..Default::default()
        };
        self.add_domain(domain)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apache_vhosts_with_comments_crlf_quotes_and_aliases() {
        let text = "# <VirtualHost *:80>\r\n#   ServerName ghost.test\r\n# </VirtualHost>\r\n\r\n<VirtualHost *:80>\r\n    servername Shop.test\r\n    ServerAlias a.test b.test   c.test\r\n    DocumentRoot \"C:/xampp/htdocs/my shop/public\"\r\n    Include conf/extra/other.conf\r\n</VirtualHost>\r\n<VirtualHost *:80>\r\n    ServerName unclosed.test\r\n";
        let v = parse_apache_vhosts(text);
        assert_eq!(v.len(), 1, "commented-out and unclosed blocks are dropped");
        assert_eq!(v[0].names, vec!["Shop.test", "a.test", "b.test", "c.test"]);
        assert_eq!(
            v[0].document_root.as_deref(),
            Some("C:/xampp/htdocs/my shop/public")
        );
    }

    #[test]
    fn nginx_servers_with_nested_locations_and_hash_in_quotes() {
        let text = r#"
http {
  include mime.types;
  server {
    listen 80;
    server_name shop.test www.shop.test; # main
    root "C:/laragon/www/shop # not a comment/public";
    location / { root /elsewhere; try_files $uri /index.php; }
    location ~ \.php$ { fastcgi_pass 127.0.0.1:9000; }
  }
  server { server_name two.test; root C:/laragon/www/two; }
}
"#;
        let v = parse_nginx_servers(text);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].names, vec!["shop.test", "www.shop.test"]);
        assert_eq!(
            v[0].document_root.as_deref(),
            Some("C:/laragon/www/shop # not a comment/public"),
            "location roots are ignored"
        );
        assert_eq!(v[1].names, vec!["two.test"]);
    }

    #[test]
    fn variables_and_hostile_names_are_handled() {
        let root = Path::new(r"C:\laragon");
        assert_eq!(
            expand_vars("${LARAGON_ROOT}/www/x", root).unwrap(),
            r"C:\laragon\www\x"
        );
        assert_eq!(
            expand_vars("%LARAGON_ROOT%\\www\\y", root).unwrap(),
            r"C:\laragon\www\y"
        );
        assert!(expand_vars("${OTHER}/x", root).is_err());
        assert!(expand_vars("C:/x/$host", root).is_err());
        for bad in [
            "localhost",
            "_",
            "*",
            "_default_",
            "127.0.0.1",
            "evil;rm",
            "a b.test",
            "single",
        ] {
            assert!(clean_hostname(bad).is_none(), "{bad}");
        }
        assert_eq!(
            clean_hostname("Shop.TEST:8080").as_deref(),
            Some("shop.test")
        );
    }

    #[test]
    fn hostname_format_supports_only_name() {
        assert_eq!(
            parse_hostname_format("[laragon]\nHostnameFormat={name}.dev.test\n").as_deref(),
            Some("{name}.dev.test")
        );
        assert_eq!(parse_hostname_format("").as_deref(), Some("{name}.test"));
        assert_eq!(
            parse_hostname_format("HostnameFormat={name}.{tld}\n"),
            None,
            "any other template is unsupported"
        );
    }

    #[test]
    fn project_root_strips_public_like_folders() {
        assert_eq!(project_root_of(r"C:\w\shop\public"), r"C:\w\shop");
        assert_eq!(project_root_of(r"C:\w\shop\Web"), r"C:\w\shop");
        assert_eq!(project_root_of(r"C:\w\shop"), r"C:\w\shop");
    }

    fn write(p: &Path, text: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    #[test]
    fn laragon_auto_hosts_and_conf_files_become_candidates() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("laragon");
        std::fs::create_dir_all(root.join("www/blog/public")).unwrap();
        std::fs::create_dir_all(root.join("www/shop")).unwrap();
        std::fs::create_dir_all(root.join("www/Bad_Name")).unwrap();
        write(
            &root.join("usr/laragon.ini"),
            "HostnameFormat={name}.dev.test\n",
        );
        write(
            &root.join("etc/apache2/sites-enabled/auto.shop.test.conf"),
            &format!(
                "<VirtualHost *:80>\n  ServerName shop.test\n  DocumentRoot \"{}\"\n</VirtualHost>\n<VirtualHost *:80>\n  ServerName gone.test\n  DocumentRoot \"C:/definitely/missing/folder\"\n</VirtualHost>\n",
                root.join("www/shop").display().to_string().replace('\\', "/")
            ),
        );
        let existing: HashSet<String> = ["shop.test".to_string()].into();
        let found = scan_roots(&[("Laragon", root.clone())], &existing);
        let by = |h: &str| found.iter().find(|c| c.hostname == h);
        let shop = by("shop.test").expect("conf site");
        assert!(shop.exists && shop.already_in_ols);
        let gone = by("gone.test").unwrap();
        assert!(!gone.exists);
        assert_eq!(gone.note.as_deref(), Some("folder missing"));
        let blog = by("blog.dev.test").expect("auto host with HostnameFormat");
        assert!(blog.project_root.ends_with("blog"), "{blog:?}");
        assert!(by("shop.dev.test").is_some());
        assert!(
            found.iter().all(|c| !c.hostname.contains('_')),
            "invalid folder names dropped"
        );
    }

    #[test]
    fn a_hostile_document_root_is_only_a_candidate() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("xampp");
        write(
            &root.join("apache/conf/extra/httpd-vhosts.conf"),
            "<VirtualHost *:80>\n ServerName evil.test\n DocumentRoot \"C:\\..\\..\\Windows\"\n</VirtualHost>\n",
        );
        let found = scan_roots(&[("XAMPP", root)], &HashSet::new());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].hostname, "evil.test");
        // It is listed; importing still needs the user's pick and an existing folder.
        assert!(!found[0].already_in_ols);
    }
}
