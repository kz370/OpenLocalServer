//! Domain Manager (§44–48, §30 — Stages 6 and 9): the local domains OpenLocalServer serves, how
//! each is routed (PHP / reverse proxy / static), and who owns its web-server config.
//! Pure data + validation — turning a `Domain` into server config lives in `web/`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

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
    Php { version: Option<String> },
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
    if let SiteKind::Proxy { upstream_port, upstream_host, .. } = kind {
        if *upstream_port == 0 {
            return Err(CoreError::DomainError("upstream port must be between 1 and 65535".into()));
        }
        if let Some(h) = upstream_host {
            if h.is_empty() || !h.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-') {
                return Err(CoreError::DomainError(format!("\"{h}\" is not a valid target host")));
            }
        }
    }
    Ok(())
}

impl SiteKind {
    /// The full upstream URL of a proxy site, e.g. `http://127.0.0.1:3000`.
    pub fn upstream_url(&self) -> Option<String> {
        match self {
            SiteKind::Proxy { upstream_port, upstream_host, upstream_https } => Some(format!(
                "{}://{}:{upstream_port}",
                if *upstream_https { "https" } else { "http" },
                upstream_host.as_deref().filter(|h| !h.is_empty()).unwrap_or("127.0.0.1")
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
}

fn yes() -> bool {
    true
}

pub struct DomainStore {
    file: PathBuf,
    domains: Vec<Domain>,
}

impl DomainStore {
    pub fn load(paths: &AppPaths) -> Result<Self, CoreError> {
        paths.ensure_dirs()?;
        let file = paths.data_dir().join("domains.json");
        let domains = if file.exists() {
            let raw = std::fs::read_to_string(&file)?;
            serde_json::from_str(&raw).unwrap_or_default()
        } else {
            Vec::new()
        };
        Ok(Self { file, domains })
    }

    pub fn list(&self) -> Vec<Domain> {
        let mut list = self.domains.clone();
        list.sort_by(|a, b| a.hostname.cmp(&b.hostname));
        list
    }

    pub fn get(&self, hostname: &str) -> Option<Domain> {
        self.domains.iter().find(|d| d.hostname == hostname).cloned()
    }

    /// Adds a new domain. Rejects invalid names and any conflict (§44 conflict detection).
    pub fn add(&mut self, mut domain: Domain) -> Result<Domain, CoreError> {
        domain.hostname = domain.hostname.trim().to_ascii_lowercase();
        validate_hostname(&domain.hostname)?;
        if let Some(host) = domain.public_domain.as_mut() {
            *host = host.trim().to_ascii_lowercase();
            validate_hostname(host)?;
            validate_public_hostname(host)?;
        }
        if let Some(other) = self.domains.iter().find(|d| d.hostname == domain.hostname || d.public_domain.as_deref() == Some(domain.hostname.as_str()) || domain.public_domain.as_deref().is_some_and(|host| d.hostname == host || d.public_domain.as_deref() == Some(host))) {
            return Err(CoreError::DomainError(format!("{} overlaps the existing site or public hostname {}", domain.hostname, other.hostname)));
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
        if let Some(host) = domain.public_domain.as_mut() {
            *host = host.trim().to_ascii_lowercase();
            validate_hostname(host)?;
            validate_public_hostname(host)?;
        }
        if let Some(other) = self.domains.iter().filter(|d| d.hostname != domain.hostname).find(|d| d.hostname == domain.hostname || d.public_domain.as_deref() == Some(domain.hostname.as_str()) || domain.public_domain.as_deref().is_some_and(|host| d.hostname == host || d.public_domain.as_deref() == Some(host))) {
            return Err(CoreError::DomainError(format!("{} overlaps the existing site or public hostname {}", domain.hostname, other.hostname)));
        }
        let idx = self
            .domains
            .iter()
            .position(|d| d.hostname == domain.hostname)
            .ok_or_else(|| CoreError::DomainError(format!("{} is not a known domain", domain.hostname)))?;
        self.domains[idx] = domain.clone();
        self.persist()?;
        Ok(domain)
    }

    pub fn remove(&mut self, hostname: &str) -> Result<(), CoreError> {
        self.domains.retain(|d| d.hostname != hostname);
        self.persist()
    }

    /// Explains why `hostname` can't be added, if it can't. `ignoring` skips one existing
    /// domain (the one being edited).
    pub fn conflict(&self, hostname: &str, wildcard: bool, ignoring: Option<&str>) -> Option<String> {
        for existing in self.domains.iter().filter(|d| Some(d.hostname.as_str()) != ignoring) {
            if existing.hostname == hostname {
                return Some(format!("{hostname} already exists"));
            }
            // A wildcard on `shop.test` would swallow an existing `api.shop.test` (and the reverse).
            if existing.wildcard && hostname.ends_with(&format!(".{}", existing.hostname)) {
                return Some(format!("{hostname} is already covered by the wildcard on {}", existing.hostname));
            }
            if wildcard && existing.hostname.ends_with(&format!(".{hostname}")) {
                return Some(format!("a wildcard on {hostname} would overlap the existing domain {}", existing.hostname));
            }
        }
        None
    }

    fn persist(&self) -> Result<(), CoreError> {
        let raw = serde_json::to_string_pretty(&self.domains)?;
        let tmp = self.file.with_extension("json.tmp");
        std::fs::write(&tmp, raw)?;
        std::fs::rename(&tmp, &self.file)?;
        Ok(())
    }
}

fn validate_public_hostname(host: &str) -> Result<(), CoreError> {
    if ["test", "local", "localhost"].iter().any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}"))) {
        return Err(CoreError::DomainError("a public domain must use a real DNS hostname, not a local development suffix".into()));
    }
    Ok(())
}

/// RFC-1123 hostname with at least two labels. Strict on purpose: the name lands in a
/// hosts file, a certificate SAN, and a config file, so nothing exotic may get through.
pub fn validate_hostname(hostname: &str) -> Result<(), CoreError> {
    let bad = |why: &str| Err(CoreError::DomainError(format!("\"{hostname}\" is not a valid domain: {why}")));
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
        if !label.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
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
        assert!(validate_hostname("A.test").is_err(), "must be normalised to lowercase before validating");
        assert!(validate_hostname("a..test").is_err());
    }

    #[test]
    fn templates_expand_with_a_slugified_project_name() {
        assert_eq!(apply_template("{project}.test", "My Shop_2"), "my-shop-2.test");
        assert_eq!(apply_template("api.{project}.test", "shop"), "api.shop.test");
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
        assert!(store.add(domain("tenant.acme.test")).is_err(), "covered by *.acme.test");

        store.add(domain("api.shop.test")).unwrap();

        // Survives a reload.
        let reloaded = DomainStore::load(&home.paths).unwrap();
        assert_eq!(reloaded.list().len(), 3);
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
}
