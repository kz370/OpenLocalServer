use std::path::{Path, PathBuf};

use super::*;
use crate::domain::{BulkOutcome, Domain, Ownership, SiteBlocks, SiteKind};
use crate::error::CoreError;
use crate::web::WebConfig;

impl Inner {
    pub fn read_site_file(&self, hostname: &str, name: &str) -> Result<String, CoreError> {
        if name != ".htaccess" {
            return Err(CoreError::DomainError(
                "only .htaccess can be edited here".into(),
            ));
        }
        let domain = self
            .domains
            .lock()
            .unwrap()
            .get(hostname)
            .ok_or_else(|| CoreError::DomainError(format!("{hostname} is not a known site")))?;
        if !matches!(domain.kind, crate::domain::SiteKind::Php { .. }) {
            return Err(CoreError::DomainError(
                ".htaccess editing is available for PHP sites".into(),
            ));
        }
        let root = std::fs::canonicalize(&domain.root).map_err(|e| {
            CoreError::DomainError(format!("site document root is unavailable: {e}"))
        })?;
        let path = root.join(name);
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(String::new()),
            Err(e) => return Err(CoreError::DomainError(e.to_string())),
        };
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(CoreError::DomainError(
                ".htaccess must be a regular file in this site's document root".into(),
            ));
        }
        let actual =
            std::fs::canonicalize(&path).map_err(|e| CoreError::DomainError(e.to_string()))?;
        if actual.parent() != Some(root.as_path()) {
            return Err(CoreError::DomainError(
                ".htaccess must be in this site's document root".into(),
            ));
        }
        std::fs::read_to_string(actual)
            .map_err(|e| CoreError::DomainError(format!("could not read .htaccess: {e}")))
    }

    pub fn write_site_file(
        &self,
        hostname: &str,
        name: &str,
        content: &str,
    ) -> Result<String, CoreError> {
        if name != ".htaccess" {
            return Err(CoreError::DomainError(
                "only .htaccess can be edited here".into(),
            ));
        }
        if content.len() > 256 * 1024 {
            return Err(CoreError::DomainError(
                ".htaccess is limited to 256 KB".into(),
            ));
        }
        let domain = self
            .domains
            .lock()
            .unwrap()
            .get(hostname)
            .ok_or_else(|| CoreError::DomainError(format!("{hostname} is not a known site")))?;
        if !matches!(domain.kind, crate::domain::SiteKind::Php { .. }) {
            return Err(CoreError::DomainError(
                ".htaccess editing is available for PHP sites".into(),
            ));
        }
        let root = std::fs::canonicalize(&domain.root).map_err(|e| {
            CoreError::DomainError(format!("site document root is unavailable: {e}"))
        })?;
        let path = root.join(".htaccess");
        let old = match std::fs::symlink_metadata(&path) {
            Ok(meta) => {
                if !meta.is_file() || meta.file_type().is_symlink() {
                    return Err(CoreError::DomainError(
                        ".htaccess must be a regular file in this site's document root".into(),
                    ));
                }
                let actual = std::fs::canonicalize(&path)
                    .map_err(|e| CoreError::DomainError(e.to_string()))?;
                if actual.parent() != Some(root.as_path()) {
                    return Err(CoreError::DomainError(
                        ".htaccess must be in this site's document root".into(),
                    ));
                }
                Some(std::fs::read(&actual)?)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(CoreError::DomainError(e.to_string())),
        };
        if let Some(old) = &old {
            let history = self.paths.web_dir().join("site-files").join(hostname);
            std::fs::create_dir_all(&history)?;
            let mut stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            while history.join(format!("{stamp}.htaccess")).exists() {
                stamp += 1;
            }
            std::fs::write(history.join(format!("{stamp}.htaccess")), old)?;
        }
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let temp = root.join(format!(".htaccess-{}-{stamp}.tmp", std::process::id()));
        std::fs::write(&temp, content)?;
        if old.is_some() {
            std::fs::remove_file(&path)?;
        }
        if let Err(e) = std::fs::rename(&temp, &path) {
            let _ = std::fs::remove_file(&temp);
            if let Some(old) = old {
                let _ = std::fs::write(&path, old);
            }
            return Err(CoreError::DomainError(format!(
                "could not save .htaccess: {e}"
            )));
        }
        Ok(path.display().to_string())
    }


    pub fn site_url(&self, d: &Domain, cfg: &WebConfig) -> String {
        let ports = cfg.effective_ports(&crate::domain::resolved_server(d, cfg));
        let (scheme, port, default) = if d.https {
            ("https", ports.https, 443)
        } else {
            ("http", ports.http, 80)
        };
        if port == default {
            format!("{scheme}://{}/", d.hostname)
        } else {
            format!("{scheme}://{}:{port}/", d.hostname)
        }
    }

    pub fn domain_summaries(&self) -> Vec<DomainSummary> {
        let cfg = self.web_config();
        let projects = self.projects.lock().unwrap().list();
        let tunnels = self.list_tunnels();
        self.domains
            .lock()
            .unwrap()
            .list()
            .into_iter()
            .map(|d| {
                let folder = site_folder(&d, &projects);
                let icon_image = read_site_icon_image(&folder);
                let broadcast_tunnel = tunnels.iter().find(|t| {
                    (d.tunnel_id.as_deref() == Some(&t.config.id)
                        || (d.project_id.is_some() && d.project_id == t.config.project_id))
                        && (t.state == "connected" || t.state == "starting")
                });
                let broadcast_url = broadcast_tunnel.and_then(|t| t.public_url.clone());
                DomainSummary {
                    url: self.site_url(&d, &cfg),
                    hostname: d.hostname.clone(),
                    https: d.https,
                    kind: match d.kind {
                        SiteKind::Php { .. } => "php",
                        SiteKind::Proxy { .. } => "proxy",
                        SiteKind::Static => "static",
                    }
                    .into(),
                    enabled: d.enabled,
                    project_id: d.project_id.clone(),
                    has_app: d.app.is_some(),
                    group: site_group(&d, &projects).into(),
                    folder,
                    public_domain: d.public_domain.clone(),
                    server: crate::domain::resolved_server(&d, &cfg),
                    path_prefix: d.path_prefix.clone(),
                    http_port: cfg
                        .effective_ports(&crate::domain::resolved_server(&d, &cfg))
                        .http,
                    icon: d.icon.clone(),
                    icon_image,
                    ssl_provider: Some(d.ssl_provider.to_string()),
                    auto_broadcast: d.auto_broadcast,
                    broadcast_url,
                    aliases: d.aliases.clone(),
                    ddns_provider: d.ddns_provider.clone(),
                    ddns_last_ip: d.ddns_last_ip.clone(),
                }
            })
            .collect()
    }

    /// Validates the structured blocks before they can reach a config file: these strings
    /// are pasted into Nginx/Apache/Caddy syntax, so nothing that could break out of a
    /// directive (newlines, quotes, braces, semicolons) is allowed through.
    pub fn validate_blocks(blocks: &SiteBlocks) -> Result<(), CoreError> {
        let bad = |s: &str| {
            s.chars()
                .any(|c| matches!(c, '\n' | '\r' | '"' | ';' | '{' | '}' | '\0'))
        };
        let name_ok = |s: &str| {
            !s.is_empty()
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        };
        let d = |m: String| CoreError::DomainError(m);
        for h in &blocks.headers {
            if !name_ok(&h.name) || bad(&h.value) {
                return Err(d(format!(
                    "header \"{}\" has characters that aren't allowed",
                    h.name
                )));
            }
        }
        for r in &blocks.redirects {
            if !r.from.starts_with('/')
                || bad(&r.from)
                || bad(&r.to)
                || r.from.contains(' ')
                || r.to.contains(' ')
                || !matches!(r.code, 301 | 302 | 303 | 307 | 308)
            {
                return Err(d(format!("redirect \"{}\" is invalid (paths must start with / and the code must be 301, 302, 303, 307 or 308)", r.from)));
            }
        }
        for m in &blocks.mappings {
            let up_ok = m.upstream.starts_with("http://") || m.upstream.starts_with("https://");
            if !m.path.starts_with('/')
                || bad(&m.path)
                || m.path.contains(' ')
                || bad(&m.upstream)
                || m.upstream.contains(' ')
                || !up_ok
            {
                return Err(d(format!("proxy mapping \"{}\" is invalid (the upstream must be an http:// or https:// URL)", m.path)));
            }
        }
        for u in &blocks.upstreams {
            if !name_ok(&u.name)
                || u.servers.is_empty()
                || u.servers.iter().any(|s| bad(s) || s.contains(' '))
            {
                return Err(d(format!("upstream \"{}\" is invalid", u.name)));
            }
        }
        for i in &blocks.includes {
            if bad(i) {
                return Err(d(
                    "an include path has characters that aren't allowed".to_string()
                ));
            }
        }
        Ok(())
    }

    pub fn add_domain(&self, mut domain: Domain) -> Result<Domain, CoreError> {
        Self::validate_blocks(&domain.blocks)?;
        if domain.https && !domain.redirect_https && domain.generated_hashes.is_empty() {
            // Sensible default for a brand-new HTTPS site (§52); the toggle stays user-controlled.
            domain.redirect_https = true;
        }
        if !Path::new(&domain.root).is_absolute() {
            return Err(CoreError::DomainError(
                "the document root must be a full folder path".into(),
            ));
        }
        crate::domain::ensure_site_dir_and_welcome_file(&domain).map_err(|e| {
            CoreError::DomainError(format!(
                "could not prepare site directory '{}': {e}",
                domain.root
            ))
        })?;
        if domain.project_id.is_none() {
            let root_path = Path::new(&domain.root);
            let mut projects = lock(&self.projects);
            let matched_id = projects.list().iter().find_map(|p| {
                if Path::new(&p.path) == root_path {
                    Some(p.id.clone())
                } else {
                    None
                }
            });
            if let Some(id) = matched_id {
                domain.project_id = Some(id);
            } else if root_path.is_dir() {
                let roots = self.string_list(PROJECT_ROOTS);
                if let Some(parent) = root_path.parent() {
                    let parent_str = parent.display().to_string();
                    if roots.iter().any(|r| r.eq_ignore_ascii_case(&parent_str)) {
                        if let Ok(p) = projects.register(&domain.root) {
                            domain.project_id = Some(p.id);
                        }
                    }
                }
            }
        }
        let hostname = domain.hostname.clone();
        let added = self.domains.lock().unwrap().add(domain)?;
        self.remove_skip(AUTO_SKIP, &hostname)?;
        Ok(added)
    }

    pub fn update_domain(&self, domain: Domain) -> Result<Domain, CoreError> {
        Self::validate_blocks(&domain.blocks)?;
        self.domains.lock().unwrap().update(domain)
    }

    /// Deletes a site and takes it off the list for good. When it was the last site of
    /// its project, the project leaves the list too, so the row the user clicked really
    /// does disappear. Their files are never touched. The folder joins the skip list
    /// (§`PROJECT_SKIP`) so folder rescans don't bring the project back.
    pub fn remove_domain(&self, hostname: &str) -> Result<(), CoreError> {
        let mut domains = self.domains.lock().unwrap();
        let was_auto = domains
            .get(hostname)
            .is_some_and(|d| d.project_id.is_some());
        // The project goes too when this was its last site; another site of the same
        // project keeps it on the list.
        let orphan_project = domains
            .get(hostname)
            .and_then(|d| d.project_id.clone())
            .filter(|project_id| {
                !domains
                    .list()
                    .iter()
                    .any(|d| d.hostname != hostname && d.project_id.as_ref() == Some(project_id))
            });
        domains.remove(hostname)?;
        // Released before anything else locks the project store: a summary read takes
        // projects first and domains second, and holding both ways round deadlocks.
        drop(domains);
        if was_auto {
            // Deleted on purpose: automatic domains must not bring it back.
            self.push_skip(AUTO_SKIP, hostname)?;
        }
        let _ = self.certs.revoke(hostname);
        self.web.restart_app(hostname);
        if let Some(id) = orphan_project {
            let _ = self.remove_project(&id);
        }
        Ok(())
    }

    /// Deletes a site by moving its project folder into an `ols_excluded_sites` folder
    /// in the same directory instead of permanently deleting it.
    /// Revokes its certificate, removes its web configuration, and unregisters the site.
    pub fn delete_site(&self, hostname: &str) -> Result<(), CoreError> {
        if hostname == "openlocalserver.test" {
            return Err(CoreError::failed_fix(
                "openlocalserver.test cannot be deleted.",
                "openlocalserver.test is the built-in home site required by OpenLocalServer.",
                "Delete other custom sites instead.",
            ));
        }
        let (orphan_project, folder_to_move) = {
            let mut domains = self.domains.lock().unwrap();
            let domain = domains.get(hostname).ok_or_else(|| {
                CoreError::DomainError(format!("{hostname} is not a known domain"))
            })?;
            let projects = self.projects.lock().unwrap().list();
            let folder = site_folder(&domain, &projects);
            let orphan_project = domain.project_id.clone().filter(|project_id| {
                !domains
                    .list()
                    .iter()
                    .any(|d| d.hostname != hostname && d.project_id.as_ref() == Some(project_id))
            });
            domains.remove(hostname)?;
            (orphan_project, folder)
        };

        let _ = self.certs.revoke(hostname);
        self.web.restart_app(hostname);

        if let Some(ref id) = orphan_project {
            let _ = self.projects.lock().unwrap().remove(id);
        }

        let folder_path = PathBuf::from(&folder_to_move);
        if folder_path.is_dir() {
            let excluded_folder = self.primary_excluded_folder();
            let _ = move_to_excluded_dir(&folder_path, &excluded_folder)?;
        }

        Ok(())
    }

    // ------------------------------------------------------- automatic domains


    pub fn sync_auto_domains(&self) -> Result<usize, CoreError> {
        if !self.setting_bool("domains.auto", true) {
            return Ok(0);
        }
        // Folders scanned before roots were remembered: adopt any parent holding 2+ projects.
        if self.settings.lock().unwrap().get(PROJECT_ROOTS).is_none() {
            let mut parents: std::collections::HashMap<String, usize> = Default::default();
            for p in self.projects.lock().unwrap().list() {
                if let Some(parent) = Path::new(&p.path).parent() {
                    *parents.entry(parent.display().to_string()).or_default() += 1;
                }
            }
            let roots: Vec<String> = parents
                .into_iter()
                .filter(|(_, n)| *n >= 2)
                .map(|(p, _)| p)
                .collect();
            self.settings
                .lock()
                .unwrap()
                .set(PROJECT_ROOTS, serde_json::json!(roots))?;
        }
        let ignored = self.ignored_folders();
        for root in self.string_list(PROJECT_ROOTS) {
            let Ok(entries) = std::fs::read_dir(&root) else {
                continue;
            };
            // Read the skips before taking the project store: settings and projects are
            // never held together (see the lock order at the top of this module).
            let skipped = self.project_skips();
            let mut projects = self.projects.lock().unwrap();
            for e in entries.flatten() {
                let dir = e.path();
                let file_name = e.file_name().to_string_lossy().to_string();
                let hidden = file_name.starts_with('.');
                let is_ignored = ignored.iter().any(|ig| ig.eq_ignore_ascii_case(&file_name));
                if dir.is_dir()
                    && !hidden
                    && !is_ignored
                    && !skip_matches(&skipped, &dir.display().to_string())
                    && (crate::detection::looks_like_a_project(&dir) || has_index(&dir))
                {
                    let _ = projects.register(&dir.display().to_string());
                }
            }
        }

        let skip: Vec<String> = self
            .skip_entries(AUTO_SKIP)
            .into_iter()
            .map(|e| e.value)
            .collect();
        let default_tld = self
            .settings
            .lock()
            .unwrap()
            .get("domains.default_tld")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().trim_start_matches('.').to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "local".to_string());
        let auto_template = format!("{{project}}.{default_tld}");
        let projects = self.projects.lock().unwrap().list();
        let mut created = 0;
        let mut created_hosts: Vec<String> = Vec::new();
        for p in &projects {
            let path = Path::new(&p.path);
            if !path.is_dir() {
                continue;
            }

            let tld_suffix = format!(".{default_tld}");
            let effective_name = p.name.strip_suffix(&tld_suffix).unwrap_or(&p.name);
            let candidate_hostname = crate::domain::apply_template(&auto_template, effective_name);

            let mut domain_to_link: Option<Domain> = None;
            let already_served = {
                let domains = self.domains.lock().unwrap();
                let linked = domains
                    .list()
                    .iter()
                    .any(|d| d.project_id.as_deref() == Some(&p.id));

                let matching_by_path = domains.list().into_iter().find(|d| {
                    let d_folder = site_folder(d, &projects);
                    Path::new(&d_folder) == path || Path::new(&d.root) == path
                });

                if let Some(mut existing) = matching_by_path {
                    if existing.project_id.as_deref() != Some(&p.id) {
                        existing.project_id = Some(p.id.clone());
                        domain_to_link = Some(existing);
                    }
                    true
                } else {
                    linked
                        || domains.get(&candidate_hostname).is_some()
                        || skip.contains(&candidate_hostname)
                }
            };

            if let Some(domain) = domain_to_link {
                let _ = self.domains.lock().unwrap().update(domain);
            }

            if already_served {
                continue;
            }
            let Some((kind, root)) = auto_site(path) else {
                continue;
            };
            let domain = Domain {
                hostname: candidate_hostname,
                project_id: Some(p.id.clone()),
                root: root.display().to_string(),
                kind,
                https: true,
                redirect_https: true,
                wildcard: false,
                enabled: true,
                ownership: Default::default(),
                app: None,
                blocks: Default::default(),
                generated_hashes: Default::default(),
                public_domain: None,
                tunnel_id: None,
                server: None,
                path_prefix: None,
                icon: None,
                ..Default::default()
            };
            match self.add_domain(domain) {
                Ok(d) => {
                    tracing::info!(hostname = %d.hostname, project = %p.name, "created an automatic domain");
                    created += 1;
                    created_hosts.push(d.hostname);
                }
                Err(e) => tracing::warn!(project = %p.name, error = %e, "automatic domain skipped"),
            }
        }
        if created > 0 {
            // A site whose name does not resolve is not a working site, so the new names
            // go into the hosts file here too. Names the built-in DNS already answers for
            // are left out, and `hosts::ensure` returns early when everything is already
            // listed — so a folder dropped into a projects folder costs a read in the
            // common case, and at most one elevation prompt across the whole install.
            let wanted: Vec<String> = {
                let domains = self.domains.lock().unwrap();
                created_hosts
                    .iter()
                    .filter(|h| {
                        domains.get(h).is_some_and(|d| d.enabled) && !self.web.dns_covers(h)
                    })
                    .cloned()
                    .collect()
            };
            if !wanted.is_empty() {
                if let Err(e) = crate::hosts::ensure(&wanted) {
                    // Not fatal: the site exists and the web server will render it. The
                    // name simply will not resolve until someone runs Sync hosts, so the
                    // reason is logged rather than raised at a folder watcher.
                    tracing::warn!(%e, "the hosts file could not be updated for the new sites");
                }
            }
            if self.web.is_running() {
                self.apply_web(&[])?;
            }
        }
        Ok(created)
    }

    /// Gives a site a different name (any valid hostname, any ending). Its certificate is
    /// reissued for the new name on the next apply, and its config files move with it.
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
fn site_group(d: &Domain, projects: &[crate::project::Project]) -> &'static str {
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


fn has_index(dir: &Path) -> bool {
    ["index.php", "index.html", "index.htm"]
        .iter()
        .any(|f| dir.join(f).is_file())
}

/// How an automatic domain serves a project, or `None` when it needs a dev server
/// (Node/Python) that a plain domain can't start.
fn auto_site(path: &Path) -> Option<(SiteKind, PathBuf)> {
    use crate::detection::Framework as F;
    let detection = crate::detection::detect(path);
    let root = detection
        .doc_root
        .as_ref()
        .map(|d| path.join(d))
        .unwrap_or_else(|| path.to_path_buf());
    match detection.framework {
        F::Laravel | F::Symfony | F::WordPress | F::GenericPhp => {
            Some((SiteKind::Php { version: None }, root))
        }
        _ if has_index(&root) => Some((SiteKind::Static, root)),
        _ => None,
    }
}

