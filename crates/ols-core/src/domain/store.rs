use super::*;
use crate::db;
use crate::error::CoreError;
use crate::paths::AppPaths;
use std::path::Path;

pub struct DomainStore {
    paths: AppPaths,
    pub(crate) domains: Vec<Domain>,
}

impl DomainStore {
    pub fn load(paths: &AppPaths) -> Result<Self, CoreError> {
        paths.ensure_dirs()?;
        let mut domains: Vec<Domain> = db::load_docs(paths, "domains").unwrap_or_default();
        let mut seeded = false;
        if !domains.iter().any(|d| d.hostname == HOME_HOSTNAME) {
            // The home site is built in and undeletable, so its absence only
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
        // The home site is the one site the app owns end to end: its folder is always
        let mut rebased = false;
        if let Some(home) = domains.iter_mut().find(|d| d.hostname == HOME_HOSTNAME) {
            let dir = paths.data_dir().join("home");
            let current = dir.display().to_string();
            if home.root != current {
                home.root = current;
                // The config that hash belonged to was written for the old folder.
                home.generated_hashes.clear();
                rebased = true;
            }
            let _ = write_home_page(&dir);
        }
        // A root inside the app may have been written by an older build as an absolute
        for d in domains.iter_mut() {
            let resolved = paths.decode_root(&d.root);
            if resolved.display().to_string() != d.root {
                d.root = resolved.display().to_string();
                d.generated_hashes.clear();
                rebased = true;
            }
        }
        let store = Self {
            paths: paths.clone(),
            domains,
        };
        if seeded || rebased {
            store.persist()?;
        }
        // Roll out new welcome-page designs to installs seeded by older
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
        domain.path_prefix = validate_path_prefix(domain.path_prefix.as_deref())?;
        domain.broadcast_provider =
            validate_broadcast_provider(domain.broadcast_provider.as_deref())?;
        self.check_path_prefix_taken(&domain)?;
        validate_ssl_config(&domain)?;
        if let Some(host) = domain.public_domain.as_mut() {
            *host = host.trim().to_ascii_lowercase();
            validate_hostname(host)?;
            validate_public_hostname(host)?;
        }
        normalize_and_validate_aliases(
            &mut domain.aliases,
            &domain.hostname,
            domain.public_domain.as_deref(),
        )?;
        self.check_domain_conflicts(&domain, None)?;
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
        domain.path_prefix = validate_path_prefix(domain.path_prefix.as_deref())?;
        domain.broadcast_provider =
            validate_broadcast_provider(domain.broadcast_provider.as_deref())?;
        self.check_path_prefix_taken(&domain)?;
        validate_ssl_config(&domain)?;
        if let Some(host) = domain.public_domain.as_mut() {
            *host = host.trim().to_ascii_lowercase();
            validate_hostname(host)?;
            validate_public_hostname(host)?;
        }
        normalize_and_validate_aliases(
            &mut domain.aliases,
            &domain.hostname,
            domain.public_domain.as_deref(),
        )?;
        self.check_domain_conflicts(&domain, Some(&domain.hostname))?;
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

    /// Checks that none of candidate's hostnames or aliases overlap with other existing domains.
    fn check_domain_conflicts(
        &self,
        candidate: &Domain,
        ignoring_hostname: Option<&str>,
    ) -> Result<(), CoreError> {
        let mut cand_names: Vec<&str> = vec![&candidate.hostname];
        if let Some(ref p) = candidate.public_domain {
            cand_names.push(p);
        }
        for a in &candidate.aliases {
            cand_names.push(a);
        }

        for other in self
            .domains
            .iter()
            .filter(|d| Some(d.hostname.as_str()) != ignoring_hostname)
        {
            let mut other_names: Vec<&str> = vec![&other.hostname];
            if let Some(ref p) = other.public_domain {
                other_names.push(p);
            }
            for a in &other.aliases {
                other_names.push(a);
            }

            for cand in &cand_names {
                if other_names.iter().any(|o| o.eq_ignore_ascii_case(cand)) {
                    return Err(CoreError::DomainError(format!(
                        "Domain name or alias '{cand}' is already registered to existing site '{}'",
                        other.hostname
                    )));
                }
            }
        }
        Ok(())
    }

    /// Turns a whole group of sites on or off in one write.
    ///
    /// A batch is never all-or-nothing: a hostname that is not in the store comes back in
    /// `skipped` with the reason, and the rest still go through. Giving up on all thirty
    /// changes because one row in the selection was stale would leave the user with
    /// nothing done and no idea why. The built-in home site is never disabled — it is what
    /// the app itself is reachable on — so it is refused with that reason.
    pub fn bulk_set_enabled(
        &mut self,
        hostnames: &[String],
        enabled: bool,
    ) -> Result<BulkOutcome, CoreError> {
        let mut outcome = BulkOutcome::default();
        for hostname in hostnames {
            match self.domains.iter_mut().find(|d| &d.hostname == hostname) {
                None => outcome.skip(hostname, "is not a known domain"),
                Some(d) if d.hostname == HOME_HOSTNAME && !enabled => {
                    outcome.skip(hostname, "is the built-in home site, which stays on")
                }
                Some(d) => {
                    d.enabled = enabled;
                    outcome.changed += 1;
                }
            }
        }
        self.persist_if_changed(outcome.changed)?;
        Ok(outcome)
    }

    /// Pins a whole group of sites to one web server, or puts them all back on the
    /// default when `server` is `None`. Same partial-failure contract as
    /// [`Self::bulk_set_enabled`]: one bad hostname is reported, the others are pinned.
    /// The server id is checked once for the whole batch, so a typo refuses every site
    /// before anything is written rather than after half of them are.
    pub fn bulk_set_server(
        &mut self,
        hostnames: &[String],
        server: Option<&str>,
    ) -> Result<BulkOutcome, CoreError> {
        let server = validate_domain_server(server)?;
        let mut outcome = BulkOutcome::default();
        for hostname in hostnames {
            match self.domains.iter_mut().find(|d| &d.hostname == hostname) {
                None => outcome.skip(hostname, "is not a known domain"),
                Some(d) => {
                    d.server = server.clone();
                    outcome.changed += 1;
                }
            }
        }
        self.persist_if_changed(outcome.changed)?;
        Ok(outcome)
    }

    /// Deletes a group of sites, one write each, and reports the ones it refused. Each
    /// deletion goes through [`Self::remove`], so the home site is protected here too and
    /// the certificate, skip-list and project-orphaning side effects in `Inner` still run
    /// per site.
    pub fn bulk_remove(&mut self, hostnames: &[String]) -> Result<BulkOutcome, CoreError> {
        let mut outcome = BulkOutcome::default();
        for hostname in hostnames {
            if hostname == HOME_HOSTNAME {
                outcome.skip(hostname, "is built in and can't be deleted");
                continue;
            }
            if !self.domains.iter().any(|d| &d.hostname == hostname) {
                outcome.skip(hostname, "is not a known domain");
                continue;
            }
            self.remove(hostname)?;
            outcome.changed += 1;
        }
        Ok(outcome)
    }

    /// A batch that changed nothing is not worth a write, so an all-skipped bulk edit
    /// leaves the store file exactly as it was.
    fn persist_if_changed(&self, changed: usize) -> Result<(), CoreError> {
        if changed > 0 {
            return self.persist();
        }
        Ok(())
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

    /// Two sites cannot share one `localhost/<prefix>`: they would answer on the same
    /// path and only one of them could win, so the second is refused with the name of
    /// the site that already holds it.
    fn check_path_prefix_taken(&self, candidate: &Domain) -> Result<(), CoreError> {
        let Some(prefix) = candidate.path_prefix.as_deref() else {
            return Ok(());
        };
        if let Some(other) = self
            .domains
            .iter()
            .find(|d| d.hostname != candidate.hostname && d.path_prefix.as_deref() == Some(prefix))
        {
            return Err(CoreError::DomainError(format!(
                "localhost/{prefix} is already used by {}. Give this site a different path.",
                other.hostname
            )));
        }
        Ok(())
    }

    pub(crate) fn persist(&self) -> Result<(), CoreError> {
        // Written back relative when the root is one of the app's own folders, so moving
        // the install moves the site with it instead of stranding it at a dead path.
        let mut rows: Vec<(String, serde_json::Value)> = Vec::with_capacity(self.domains.len());
        for d in &self.domains {
            let mut stored = d.clone();
            stored.root = self.paths.encode_root(Path::new(&d.root));
            rows.push((
                d.hostname.clone(),
                serde_json::to_value(&stored).map_err(|e| CoreError::Db(e.to_string()))?,
            ));
        }
        db::save_values(&self.paths, "domains", &rows)
    }
}

pub fn validate_public_hostname(host: &str) -> Result<(), CoreError> {
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

pub fn is_real_domain(host: &str) -> bool {
    validate_public_hostname(host).is_ok()
}

pub fn validate_broadcast_provider(provider: Option<&str>) -> Result<Option<String>, CoreError> {
    match provider.map(str::trim) {
        None | Some("") => Ok(None),
        Some("cloudflare") | Some("localtunnel") | Some("ngrok") => {
            Ok(Some(provider.unwrap().trim().to_string()))
        }
        Some(s) => Err(CoreError::DomainError(format!(
            "\"{s}\" is not a supported tunnel broadcast provider. Choose cloudflare, localtunnel, or ngrok."
        ))),
    }
}

pub fn validate_ssl_config(domain: &Domain) -> Result<(), CoreError> {
    match domain.ssl_provider {
        SslProvider::LocalCa => Ok(()),
        SslProvider::Custom => {
            let cert = domain.custom_cert_path.as_deref().unwrap_or("").trim();
            let key = domain.custom_key_path.as_deref().unwrap_or("").trim();
            if cert.is_empty() || key.is_empty() {
                return Err(CoreError::DomainError(
                    "Custom SSL requires both certificate and private key file paths".into(),
                ));
            }
            if !std::path::Path::new(cert).is_file() {
                return Err(CoreError::DomainError(format!(
                    "Custom SSL certificate file not found: {cert}"
                )));
            }
            if !std::path::Path::new(key).is_file() {
                return Err(CoreError::DomainError(format!(
                    "Custom SSL private key file not found: {key}"
                )));
            }
            Ok(())
        }
        SslProvider::LetsEncrypt => {
            validate_public_hostname(&domain.hostname)?;
            if let Some(email) = domain.lets_encrypt_email.as_deref() {
                let e = email.trim();
                if !e.is_empty() && (!e.contains('@') || !e.contains('.')) {
                    return Err(CoreError::DomainError(format!(
                        "\"{email}\" is not a valid email address for Let's Encrypt notifications"
                    )));
                }
            }
            Ok(())
        }
    }
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

/// Normalizes, validates, and deduplicates site aliases.
/// Rejects invalid hostnames and drops aliases identical to the primary domain.
pub fn normalize_and_validate_aliases(
    aliases: &mut Vec<String>,
    primary_hostname: &str,
    public_domain: Option<&str>,
) -> Result<(), CoreError> {
    let mut normalized = Vec::new();
    for alias in aliases.drain(..) {
        let clean = alias.trim().to_ascii_lowercase();
        if clean.is_empty() {
            continue;
        }
        validate_hostname(&clean)?;
        if clean == primary_hostname || public_domain.is_some_and(|p| p == clean) {
            continue;
        }
        if !normalized.contains(&clean) {
            normalized.push(clean);
        }
    }
    *aliases = normalized;
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
