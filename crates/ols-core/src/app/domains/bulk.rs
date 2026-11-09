use crate::app::*;
use crate::domain::{BulkOutcome, Domain, Ownership, SiteKind};
use crate::error::CoreError;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// How long a folder probe (favicon search, framework detection, WordPress check) is reused.
/// The site and project lists are read every few seconds and every read probed each folder
/// again: a dozen stat calls per favicon name per root, a JSON parse per project, a base64
/// encode per icon. On a slow or network drive that alone took seconds per list.
const PROBE_TTL: Duration = Duration::from_secs(20);

type ProbeCache = Mutex<HashMap<String, (Instant, Option<String>)>>;

static PROBE_CACHE: OnceLock<ProbeCache> = OnceLock::new();

fn probe_cached(key: String, compute: impl FnOnce() -> Option<String>) -> Option<String> {
    let cache = PROBE_CACHE.get_or_init(Default::default);
    if let Ok(map) = cache.lock() {
        if let Some((at, value)) = map.get(&key) {
            if at.elapsed() < PROBE_TTL {
                return value.clone();
            }
        }
    }
    let value = compute();
    if let Ok(mut map) = cache.lock() {
        if map.len() > 4000 {
            map.clear();
        }
        map.insert(key, (Instant::now(), value.clone()));
    }
    value
}

/// Forget every cached folder probe, so an icon that was just set or removed shows at once.
pub fn clear_probe_cache() {
    if let Some(cache) = PROBE_CACHE.get() {
        if let Ok(mut map) = cache.lock() {
            map.clear();
        }
    }
}

impl Inner {
    pub fn rename_domain(&self, hostname: &str, new_hostname: &str) -> Result<Domain, CoreError> {
        if hostname == crate::domain::HOME_HOSTNAME {
            return Err(CoreError::DomainError(format!(
                "{} is built in and can't be renamed",
                crate::domain::HOME_HOSTNAME
            )));
        }
        let new_hostname = new_hostname.trim().to_ascii_lowercase();
        if new_hostname == hostname {
            return self.domains.lock().unwrap().get(hostname).ok_or_else(|| {
                CoreError::DomainError(format!("{hostname} is not a known domain"))
            });
        }
        let mut domains = self.domains.lock().unwrap();
        let original = domains
            .get(hostname)
            .ok_or_else(|| CoreError::DomainError(format!("{hostname} is not a known domain")))?;
        let mut renamed = original.clone();
        renamed.hostname = new_hostname.clone();
        renamed.generated_hashes.clear();
        domains.remove(hostname)?;
        let renamed = match domains.add(renamed) {
            Ok(d) => d,
            Err(e) => {
                // Name taken or invalid: put the original back untouched.
                domains.add(original)?;
                return Err(e);
            }
        };
        drop(domains);
        self.web.rename_site_files(hostname, &renamed.hostname);
        let _ = self.certs.revoke(hostname);
        self.web.restart_app(hostname);
        self.remove_skip(AUTO_SKIP, &renamed.hostname)?;
        Ok(renamed)
    }

    pub fn duplicate_domain(
        &self,
        hostname: &str,
        new_hostname: &str,
    ) -> Result<Domain, CoreError> {
        let mut domains = self.domains.lock().unwrap();
        let mut copy = domains
            .get(hostname)
            .ok_or_else(|| CoreError::DomainError(format!("{hostname} is not a known domain")))?;
        copy.hostname = new_hostname.trim().to_ascii_lowercase();
        copy.generated_hashes.clear();
        // Two sites can't share one dev-server port or process: the copy needs its own.
        copy.app = None;
        domains.add(copy)
    }

    /// Applies every running web server. One report per server; a failure in one never
    /// rolls back or stops another.
    ///
    /// The domain store is locked only long enough to take a snapshot: rendering files,
    /// running each server's validator and (re)starting it can take seconds, and holding
    /// the lock that long blocked reads (`list_domains` runs on a UI poll) for the whole
    /// apply. Generated-config hashes are committed afterwards, under the lock again.
    pub fn set_domain_enabled(&self, hostname: &str, enabled: bool) -> Result<(), CoreError> {
        let mut domains = self.domains.lock().unwrap();
        let mut d = domains
            .get(hostname)
            .ok_or_else(|| CoreError::DomainError(format!("{hostname} is not a known domain")))?;
        d.enabled = enabled;
        domains.update(d)?;
        drop(domains);
        if !enabled {
            crate::lan::reconcile(self);
        }
        Ok(())
    }

    /// One batch, one store write. Sites that cannot change are named in the outcome
    /// instead of failing the whole edit — see `DomainStore::bulk_set_enabled`.
    pub fn set_domains_enabled(
        &self,
        hostnames: &[String],
        enabled: bool,
    ) -> Result<BulkOutcome, CoreError> {
        let outcome = self
            .domains
            .lock()
            .unwrap()
            .bulk_set_enabled(hostnames, enabled)?;
        if !enabled {
            crate::lan::reconcile(self);
        }
        Ok(outcome)
    }

    /// Pins a group of sites to one web server, or back to the default when `server` is
    /// `None`. One store write for the batch; the caller still runs a single `ApplyWeb`
    /// afterwards, so a forty-site repaint re-renders the configs once, not forty times.
    pub fn set_domain_servers(
        &self,
        hostnames: &[String],
        server: Option<&str>,
    ) -> Result<BulkOutcome, CoreError> {
        self.domains
            .lock()
            .unwrap()
            .bulk_set_server(hostnames, server)
    }

    /// Deletes a group of sites. Each one goes through [`Self::remove_domain`], so the
    /// certificate revocation, skip-list entry and project-orphaning rules are the same
    /// ones a single delete follows. A site that cannot go is reported and the rest are
    /// still deleted: half a batch the user asked for beats none of it.
    pub fn remove_domains(&self, hostnames: &[String]) -> Result<BulkOutcome, CoreError> {
        let mut outcome = BulkOutcome::default();
        for hostname in hostnames {
            match self.remove_domain(hostname) {
                Ok(()) => outcome.changed += 1,
                Err(e) => outcome.skip(hostname, &e.to_string()),
            }
        }
        Ok(outcome)
    }

    /// Deletes a group of sites by moving their folders into `ols_excluded_sites` in their respective directories.
    pub fn delete_sites(&self, hostnames: &[String]) -> Result<BulkOutcome, CoreError> {
        let mut outcome = BulkOutcome::default();
        for hostname in hostnames {
            if hostname == "openlocalserver.test" {
                outcome.skip(
                    hostname,
                    "openlocalserver.test is the built-in home site, which cannot be deleted.",
                );
                continue;
            }
            match self.delete_site(hostname) {
                Ok(()) => outcome.changed += 1,
                Err(e) => outcome.skip(hostname, &e.to_string()),
            }
        }
        Ok(outcome)
    }

    pub fn set_ownership(&self, hostname: &str, ownership: Ownership) -> Result<(), CoreError> {
        let cfg = self.web_config();
        let mut domains = self.domains.lock().unwrap();
        self.web
            .set_ownership(&cfg, &mut domains, hostname, ownership)
    }

    pub fn restore_web_history(&self, hostname: &str, id: &str) -> Result<String, CoreError> {
        let cfg = self.web_config();
        let mut domains = self.domains.lock().unwrap();
        self.web.restore_history(&cfg, &mut domains, hostname, id)
    }
}

/// The "type of website" a site is listed under: "php", "nodejs", "python", "static", or
/// "proxy" for anything else that is forwarded (Docker, another computer, unknown apps).
/// A dev-server site is told apart by its start command, else by the linked project.
pub(crate) fn site_group(d: &Domain, projects: &[crate::project::Project]) -> &'static str {
    use crate::detection::Framework;
    match &d.kind {
        SiteKind::Php { .. } => "php",
        SiteKind::Static => "static",
        SiteKind::Proxy { upstream_host, .. } => {
            if let Some(app) = &d.app {
                let exe = Path::new(&app.executable)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or(&app.executable)
                    .to_ascii_lowercase();
                let runtime = app.runtime.as_deref().unwrap_or_default();
                if runtime == "node"
                    || matches!(
                        exe.as_str(),
                        "node" | "npm" | "npx" | "pnpm" | "yarn" | "bun" | "deno"
                    )
                {
                    return "nodejs";
                }
                if runtime == "python"
                    || matches!(
                        exe.as_str(),
                        "python"
                            | "python3"
                            | "py"
                            | "uvicorn"
                            | "gunicorn"
                            | "flask"
                            | "django-admin"
                            | "hypercorn"
                            | "poetry"
                            | "pipenv"
                    )
                {
                    return "python";
                }
                if runtime == "php" || exe == "php" {
                    return "php";
                }
            }
            // No start command, and it is this computer (not Docker / another machine):
            // the linked project's framework says what is behind the port.
            if upstream_host.is_none() {
                let path = d
                    .project_id
                    .as_ref()
                    .and_then(|id| projects.iter().find(|p| &p.id == id))
                    .map(|p| p.path.clone());
                if let Some(path) = path {
                    return match crate::detection::detect(Path::new(&path)).framework {
                        Framework::Node => "nodejs",
                        Framework::Django
                        | Framework::Flask
                        | Framework::FastApi
                        | Framework::GenericPython => "python",
                        _ => "proxy",
                    };
                }
            }
            "proxy"
        }
    }
}

pub fn site_folder(d: &Domain, projects: &[crate::project::Project]) -> String {
    if let Some(p) = d
        .project_id
        .as_ref()
        .and_then(|id| projects.iter().find(|p| &p.id == id))
    {
        return p.path.clone();
    }
    let root = Path::new(&d.root);
    let docroot = root
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| matches!(n.to_lowercase().as_str(), "public" | "web" | "public_html"));
    match root.parent() {
        Some(parent) if docroot => parent.display().to_string(),
        _ => d.root.clone(),
    }
}

const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn to_base64(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        out.push(BASE64_ALPHABET[(b0 >> 2) as usize] as char);
        out.push(BASE64_ALPHABET[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(BASE64_ALPHABET[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(BASE64_ALPHABET[(b2 & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

pub fn from_base64(input: &str) -> Result<Vec<u8>, String> {
    fn decode_char(c: u8) -> Result<u8, String> {
        match c {
            b'A'..=b'Z' => Ok(c - b'A'),
            b'a'..=b'z' => Ok(c - b'a' + 26),
            b'0'..=b'9' => Ok(c - b'0' + 52),
            b'+' => Ok(62),
            b'/' => Ok(63),
            _ => Err(format!("Invalid base64 character: {}", c as char)),
        }
    }

    let input = input.trim().as_bytes();
    if input.is_empty() {
        return Ok(Vec::new());
    }
    if !input.len().is_multiple_of(4) {
        return Err("Base64 string length must be a multiple of 4".into());
    }

    let mut out = Vec::with_capacity(input.len() / 4 * 3);
    for chunk in input.chunks(4) {
        let b0 = decode_char(chunk[0])?;
        let b1 = decode_char(chunk[1])?;
        out.push((b0 << 2) | (b1 >> 4));
        if chunk[2] != b'=' {
            let b2 = decode_char(chunk[2])?;
            out.push(((b1 & 0x0f) << 4) | (b2 >> 2));
            if chunk[3] != b'=' {
                let b3 = decode_char(chunk[3])?;
                out.push(((b2 & 0x03) << 6) | b3);
            }
        }
    }
    Ok(out)
}

const SITE_ICON_LIMIT: u64 = 2 * 1024 * 1024;
const FAVICON_NAMES: &[(&str, &str)] = &[
    ("favicon.svg", "image/svg+xml"),
    ("favicon.ico", "image/x-icon"),
    ("favicon.png", "image/png"),
    ("favicon-32x32.png", "image/png"),
    ("favicon-16x16.png", "image/png"),
    ("favicon.webp", "image/webp"),
    ("favicon.jpg", "image/jpeg"),
    ("favicon.jpeg", "image/jpeg"),
    ("apple-touch-icon.png", "image/png"),
    ("apple-touch-icon-precomposed.png", "image/png"),
    ("site-icon.svg", "image/svg+xml"),
    ("site-icon.png", "image/png"),
];

fn image_data_url(path: &Path, mime: &str) -> Option<String> {
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > SITE_ICON_LIMIT {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    Some(format!("data:{mime};base64,{}", to_base64(&bytes)))
}

fn custom_site_icon_image(folder: &Path) -> Option<String> {
    image_data_url(&folder.join("ols_site_icon.webp"), "image/webp")
}

fn discovered_site_icon_image(folder: &Path) -> Option<String> {
    let mut roots = vec![folder.to_path_buf()];
    for child in ["public", "web", "public_html"] {
        roots.push(folder.join(child));
    }
    // Common NestJS/static front-end layouts. Only inspect these shallow, conventional
    // locations; do not recursively walk a potentially large node_modules tree.
    roots.extend(["client/public", "client", "static", "assets"].map(|path| folder.join(path)));
    if let Ok(apps) = std::fs::read_dir(folder.join("apps")) {
        roots.extend(apps.flatten().filter_map(|entry| {
            entry
                .file_type()
                .ok()
                .filter(|kind| kind.is_dir())
                .map(|_| entry.path().join("public"))
        }));
    }
    for root in roots {
        for (name, mime) in FAVICON_NAMES {
            if let Some(data_url) = image_data_url(&root.join(name), mime) {
                return Some(data_url);
            }
        }
    }
    None
}

/// Reads a manually selected site icon first, then a favicon from the project root or
/// common document-root folders. Files are bounded to 2 MiB before being returned.
pub fn read_site_icon_image(folder: &str) -> Option<String> {
    probe_cached(format!("icon|{folder}"), || {
        let folder = Path::new(folder);
        custom_site_icon_image(folder).or_else(|| discovered_site_icon_image(folder))
    })
}

/// Domain document roots can differ from their linked project folder. Keep uploaded
/// images ahead of discovered favicons across both locations.
pub fn read_domain_icon_image(document_root: &str, project_folder: &str) -> Option<String> {
    probe_cached(
        format!("domain-icon|{document_root}|{project_folder}"),
        || {
            let document_root = Path::new(document_root);
            let project_folder = Path::new(project_folder);
            custom_site_icon_image(project_folder)
                .or_else(|| custom_site_icon_image(document_root))
                .or_else(|| discovered_site_icon_image(document_root))
                .or_else(|| discovered_site_icon_image(project_folder))
        },
    )
}

/// Recognizes WordPress from its selected type or its conventional core files. This is a
/// presentation fallback only; it does not change or persist the project's type.
pub fn is_wordpress_project(folder: &str, project_type: Option<&str>) -> bool {
    probe_cached(format!("wp|{folder}|{project_type:?}"), || {
        is_wordpress_project_uncached(folder, project_type).then(|| "1".to_string())
    })
    .is_some()
}

fn is_wordpress_project_uncached(folder: &str, project_type: Option<&str>) -> bool {
    if project_type
        .and_then(|kind| kind.rsplit(':').next())
        .is_some_and(|kind| kind.eq_ignore_ascii_case("wordpress"))
    {
        return true;
    }
    let folder = Path::new(folder);
    [
        folder.to_path_buf(),
        folder.join("public"),
        folder.join("web"),
    ]
    .iter()
    .any(|root| {
        root.join("wp-config.php").is_file()
            || root.join("wp-includes/version.php").is_file()
            || root.join("wp-admin").is_dir()
    })
}

/// Resolves an icon type from the stored project type or the framework detector. The
/// result is a transient read-model value; it never changes the user's project settings.
pub fn project_type_for_icon(folder: &str, stored_type: Option<&str>) -> Option<String> {
    if let Some(kind) = stored_type.filter(|kind| !kind.eq_ignore_ascii_case("auto")) {
        return Some(kind.to_string());
    }
    probe_cached(format!("type|{folder}"), || {
        project_type_for_icon_uncached(folder, stored_type)
    })
}

fn project_type_for_icon_uncached(folder: &str, stored_type: Option<&str>) -> Option<String> {
    if let Some(kind) = stored_type.filter(|kind| !kind.eq_ignore_ascii_case("auto")) {
        return Some(kind.to_string());
    }
    let detection = crate::detection::detect(Path::new(folder));
    match detection.framework {
        crate::detection::Framework::Laravel => Some("laravel"),
        crate::detection::Framework::Symfony => Some("symfony"),
        crate::detection::Framework::WordPress => Some("wordpress"),
        crate::detection::Framework::GenericPhp => Some("php"),
        crate::detection::Framework::Node => Some("node"),
        crate::detection::Framework::Django => Some("django"),
        crate::detection::Framework::Flask => Some("flask"),
        crate::detection::Framework::FastApi => Some("fastapi"),
        crate::detection::Framework::GenericPython => Some("python"),
        crate::detection::Framework::Unknown => None,
    }
    .map(str::to_string)
}

/// Reads a per-project-type fallback named `<type>_fallback.<ext>` from `data/icons`
/// or one category folder. Examples: `laravel_fallback.webp` and `Web/nestjs_fallback.ico`.
pub fn read_project_fallback_icon(icons_dir: &Path, project_type: Option<&str>) -> Option<String> {
    probe_cached(
        format!("fallback|{}|{project_type:?}", icons_dir.display()),
        || read_project_fallback_icon_uncached(icons_dir, project_type),
    )
}

fn read_project_fallback_icon_uncached(
    icons_dir: &Path,
    project_type: Option<&str>,
) -> Option<String> {
    let project_type = project_type?
        .rsplit(':')
        .next()?
        .trim()
        .to_ascii_lowercase();
    let slug = project_type
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect::<String>();
    if slug.is_empty() {
        return None;
    }

    const FORMATS: &[(&str, &str)] = &[
        ("webp", "image/webp"),
        ("png", "image/png"),
        ("svg", "image/svg+xml"),
        ("ico", "image/x-icon"),
        ("jpg", "image/jpeg"),
        ("jpeg", "image/jpeg"),
        ("gif", "image/gif"),
        ("bmp", "image/bmp"),
    ];
    let mut folders = vec![icons_dir.to_path_buf()];
    if let Ok(entries) = std::fs::read_dir(icons_dir) {
        folders.extend(entries.flatten().filter_map(|entry| {
            entry
                .file_type()
                .ok()
                .filter(|kind| kind.is_dir())
                .map(|_| entry.path())
        }));
    }
    for folder in folders {
        for (extension, mime) in FORMATS {
            let path = folder.join(format!("{slug}_fallback.{extension}"));
            if let Some(data_url) = image_data_url(&path, mime) {
                return Some(data_url);
            }
        }
    }
    None
}

/// Lists image files in `data/icons` and one level of category folders.
pub fn list_site_icon_library(icons_dir: &Path) -> Vec<SiteIconLibraryEntry> {
    fn image_entries(dir: &Path, category: &str, output: &mut Vec<SiteIconLibraryEntry>) {
        const MAX_ICON_LIBRARY_ENTRIES: usize = 100;
        const MAX_ICON_LIBRARY_FILE_BYTES: u64 = 256 * 1024;
        if output.len() >= MAX_ICON_LIBRARY_ENTRIES {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut paths = entries
            .flatten()
            .filter_map(|entry| {
                let file_type = entry.file_type().ok()?;
                if !file_type.is_file() {
                    return None;
                }
                let path = entry.path();
                let ext = path.extension()?.to_str()?.to_ascii_lowercase();
                let mime = match ext.as_str() {
                    "webp" => "image/webp",
                    "png" => "image/png",
                    "jpg" | "jpeg" => "image/jpeg",
                    "svg" => "image/svg+xml",
                    "gif" => "image/gif",
                    "bmp" => "image/bmp",
                    "ico" => "image/x-icon",
                    _ => return None,
                };
                Some((path, mime.to_string()))
            })
            .collect::<Vec<_>>();
        paths.sort_by(|a, b| a.0.file_name().cmp(&b.0.file_name()));
        for (path, mime) in paths {
            if output.len() >= MAX_ICON_LIBRARY_ENTRIES
                || std::fs::metadata(&path)
                    .map(|metadata| metadata.len() > MAX_ICON_LIBRARY_FILE_BYTES)
                    .unwrap_or(true)
            {
                continue;
            }
            let Some(data_url) = image_data_url(&path, &mime) else {
                continue;
            };
            let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            output.push(SiteIconLibraryEntry {
                name: name.to_string(),
                category: category.to_string(),
                data_url,
            });
        }
    }

    let mut output = Vec::new();
    image_entries(icons_dir, "", &mut output);
    let Ok(categories) = std::fs::read_dir(icons_dir) else {
        return output;
    };
    let mut categories = categories
        .flatten()
        .filter_map(|entry| {
            let file_type = entry.file_type().ok()?;
            file_type.is_dir().then(|| entry.path())
        })
        .collect::<Vec<_>>();
    categories.sort();
    for category_dir in categories {
        let category = category_dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Other");
        image_entries(&category_dir, category, &mut output);
    }
    output
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SiteIconLibraryEntry {
    pub name: String,
    pub category: String,
    pub data_url: String,
}

/// Generates a safe filename for a proxy site icon (e.g. "test.local" -> "test_local.webp").
pub fn proxy_icon_filename(hostname: &str) -> String {
    let sanitized: String = hostname
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let name = if sanitized.trim_matches('_').is_empty() {
        "proxy_site"
    } else {
        sanitized.as_str()
    };
    format!("{name}.webp")
}

/// Reads `<proxy_dir>/<hostname>.webp` if present and returns a data URL.
pub fn read_proxy_site_icon_image(proxy_dir: &Path, hostname: &str) -> Option<String> {
    let path = proxy_dir.join(proxy_icon_filename(hostname));
    if path.is_file() {
        match std::fs::read(&path) {
            Ok(bytes) if !bytes.is_empty() && bytes.len() <= 2 * 1024 * 1024 => {
                Some(format!("data:image/webp;base64,{}", to_base64(&bytes)))
            }
            _ => None,
        }
    } else {
        None
    }
}
