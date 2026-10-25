use crate::app::*;
use crate::domain::{BulkOutcome, Domain, Ownership, SiteKind};
use crate::error::CoreError;
use std::path::Path;

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
        Ok(())
    }

    /// One batch, one store write. Sites that cannot change are named in the outcome
    /// instead of failing the whole edit — see `DomainStore::bulk_set_enabled`.
    pub fn set_domains_enabled(
        &self,
        hostnames: &[String],
        enabled: bool,
    ) -> Result<BulkOutcome, CoreError> {
        self.domains
            .lock()
            .unwrap()
            .bulk_set_enabled(hostnames, enabled)
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

/// Reads `<folder>/ols_site_icon.webp` if present and returns a data URL.
pub fn read_site_icon_image(folder: &str) -> Option<String> {
    let path = Path::new(folder).join("ols_site_icon.webp");
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
