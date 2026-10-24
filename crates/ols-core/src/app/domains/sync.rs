use std::path::{Path, PathBuf};
use crate::app::*;
use crate::app::domains::*;
use crate::domain::{AppSpec, BulkOutcome, Domain, Ownership, SiteBlocks, SiteKind};
use crate::error::CoreError;
use crate::web::WebConfig;

impl Inner {
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

