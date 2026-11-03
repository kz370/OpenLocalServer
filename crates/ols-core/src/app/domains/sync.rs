use crate::app::domains::*;
use crate::app::*;
use crate::domain::{Domain, SiteKind};
use crate::error::CoreError;
use std::path::{Path, PathBuf};

impl Inner {
    pub(crate) fn site_is_active(&self, domain: &Domain) -> bool {
        let projects = self.projects.lock().unwrap().list();
        let folder = site_folder(domain, &projects);
        !self
            .skip_entries(AUTO_SKIP)
            .iter()
            .any(|entry| entry.value.eq_ignore_ascii_case(&domain.hostname))
            && (folder.is_empty() || !Path::new(&folder).join(".ols_ignore_me").exists())
    }

    pub fn reconcile(&self) -> Result<usize, CoreError> {
        self.reconcile_with_reports().map(|(active, _)| active)
    }

    pub fn reconcile_with_reports(
        &self,
    ) -> Result<(usize, Vec<crate::web::manager::ApplyReport>), CoreError> {
        let records = self.domains.lock().unwrap().list();
        let active: Vec<_> = records
            .iter()
            .filter(|domain| self.site_is_active(domain))
            .cloned()
            .collect();
        let hosts: Vec<_> = active
            .iter()
            .filter(|domain| domain.enabled && !self.web.dns_covers(&domain.hostname))
            .map(|domain| domain.hostname.clone())
            .collect();
        if let Err(error) = crate::hosts::sync(&hosts) {
            tracing::warn!(%error, "hosts file could not be reconciled");
        }
        // Applying configs is useful for a running server: it removes inactive
        // vhosts and reloads the server. A stopped/uninstalled server has nothing
        // to reload, and reconciliation must still update the stores and hosts file.
        let reports = if self.web.is_running() {
            self.apply_web(&[])?
        } else {
            Vec::new()
        };
        Ok((active.len(), reports))
    }

    pub fn sync_auto_domains(&self) -> Result<usize, CoreError> {
        let auto_domains = self.setting_bool("domains.auto", true);
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
        let hidden = self.skip_entries(AUTO_SKIP);
        let current_projects = self.projects.lock().unwrap().list();
        let hidden_paths: Vec<String> = self
            .domains
            .lock()
            .unwrap()
            .list()
            .into_iter()
            .filter(|domain| {
                hidden
                    .iter()
                    .any(|entry| entry.value.eq_ignore_ascii_case(&domain.hostname))
            })
            .map(|domain| site_folder(&domain, &current_projects))
            .filter(|path| !path.is_empty())
            .collect();
        for root in self
            .string_list(PROJECT_ROOTS)
            .into_iter()
            .filter(|_| auto_domains)
        {
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
                    && !dir.join(".ols_ignore_me").exists()
                    && !hidden_paths
                        .iter()
                        .any(|path| crate::project::same_path(path, &dir.display().to_string()))
                    && !skip_matches(&skipped, &dir.display().to_string())
                    && (crate::detection::looks_like_a_project(&dir) || has_index(&dir))
                {
                    // Discovering a project folder is not an instruction to publish it
                    // as a local site. Explicit add/scan actions decide that separately.
                    let _ = projects.register_with_standalone(&dir.display().to_string(), true);
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
        for p in projects.iter().filter(|_| auto_domains) {
            let path = Path::new(&p.path);
            if !path.is_dir() || path.join(".ols_ignore_me").exists() {
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
                // Never create a duplicate auto domain for a project that already has
                // any domain (regardless of TLD), so changing the default TLD does not
                // rename or shadow existing sites.
                let has_any_domain = domains.list().iter().any(|d| {
                    d.project_id.as_deref() == Some(&p.id)
                        || {
                            let d_folder = site_folder(d, &projects);
                            Path::new(&d_folder) == path || Path::new(&d.root) == path
                        }
                        || d.hostname.contains(&p.name)
                });

                if let Some(mut existing) = matching_by_path {
                    if existing.project_id.as_deref() != Some(&p.id) {
                        existing.project_id = Some(p.id.clone());
                        domain_to_link = Some(existing);
                    }
                    true
                } else if has_any_domain {
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

            if already_served || p.standalone {
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
                }
                Err(e) => tracing::warn!(project = %p.name, error = %e, "automatic domain skipped"),
            }
        }
        self.reconcile()?;
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
pub(crate) fn auto_site(path: &Path) -> Option<(SiteKind, PathBuf)> {
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
