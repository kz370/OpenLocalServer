use crate::app::domains::*;
use crate::app::*;
use crate::domain::{Domain, SiteBlocks, SiteKind};
use crate::error::CoreError;
use crate::web::WebConfig;
use std::path::{Path, PathBuf};

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
        if !std::path::Path::new(&domain.root).is_dir() {
            if let Some(ref pid) = domain.project_id {
                if let Some(p) = self.projects.lock().unwrap().get(pid) {
                    return Err(CoreError::ProjectFolderMissing {
                        name: p.name,
                        path: p.path,
                    });
                }
            }
            return Err(CoreError::ProjectFolderMissing {
                name: domain.hostname.clone(),
                path: domain.root.clone(),
            });
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
        if !std::path::Path::new(&domain.root).is_dir() {
            if let Some(ref pid) = domain.project_id {
                if let Some(p) = self.projects.lock().unwrap().get(pid) {
                    return Err(CoreError::ProjectFolderMissing {
                        name: p.name,
                        path: p.path,
                    });
                }
            }
            return Err(CoreError::ProjectFolderMissing {
                name: domain.hostname.clone(),
                path: domain.root.clone(),
            });
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
        let proxy_dir = self.paths.proxy_site_images_dir();
        self.domains
            .lock()
            .unwrap()
            .list()
            .into_iter()
            .map(|d| {
                let folder = site_folder(&d, &projects);
                let project = d
                    .project_id
                    .as_ref()
                    .and_then(|id| projects.iter().find(|p| &p.id == id));
                let icon_image = if matches!(d.kind, SiteKind::Proxy { .. }) {
                    read_proxy_site_icon_image(&proxy_dir, &d.hostname)
                } else {
                    read_domain_icon_image(&d.root, &folder)
                        .or_else(|| read_proxy_site_icon_image(&proxy_dir, &d.hostname))
                };
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
                    document_root: d.root.clone(),
                    public_domain: d.public_domain.clone(),
                    server: crate::domain::resolved_server(&d, &cfg),
                    path_prefix: d.path_prefix.clone(),
                    http_port: cfg
                        .effective_ports(&crate::domain::resolved_server(&d, &cfg))
                        .http,
                    icon: d.icon.clone().or_else(|| {
                        project.and_then(|p| {
                            p.icon.clone().or_else(|| {
                                p.project_type.as_deref().and_then(|kind| {
                                    kind.rsplit(':')
                                        .next()
                                        .is_some_and(|name| name.eq_ignore_ascii_case("wordpress"))
                                        .then(|| "wordpress".to_string())
                                })
                            })
                        })
                    }),
                    icon_image,
                    project_type: project.and_then(|p| p.project_type.clone()),
                    ssl_provider: Some(d.ssl_provider.to_string()),
                    auto_broadcast: d.auto_broadcast,
                    broadcast_url,
                    aliases: d.aliases.clone(),
                    ddns_provider: d.ddns_provider.clone(),
                    ddns_last_ip: d.ddns_last_ip.clone(),
                    proxy_target: d.kind.upstream_url(),
                }
            })
            .collect::<Vec<_>>()
            .into_iter()
            .filter(|s| {
                s.folder.is_empty()
                    || !std::path::Path::new(&s.folder)
                        .join(".ols_ignore_me")
                        .exists()
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

    /// Hides a site while retaining its database record and project settings.
    pub fn remove_domain(&self, hostname: &str) -> Result<(), CoreError> {
        if hostname == crate::domain::HOME_HOSTNAME {
            return Err(CoreError::failed_fix(
                "The built-in home site cannot be hidden.",
                "openlocalserver.test is required by OpenLocalServer.",
                "Hide or remove another site instead.",
            ));
        }
        if self.domains.lock().unwrap().get(hostname).is_none() {
            return Err(CoreError::DomainError(format!(
                "{hostname} is not a known domain"
            )));
        }
        self.push_skip(AUTO_SKIP, hostname)?;
        self.web.restart_app(hostname);
        Ok(())
    }

    /// Removes only the site from OLS, keeping its project registration and files.
    pub fn remove_site_from_list(&self, hostname: &str) -> Result<(), CoreError> {
        let domain =
            self.domains.lock().unwrap().get(hostname).ok_or_else(|| {
                CoreError::DomainError(format!("{hostname} is not a known domain"))
            })?;
        let projects = self.projects.lock().unwrap().list();
        let folder = site_folder(&domain, &projects);
        let project_id = domain.project_id.clone().or_else(|| {
            projects
                .iter()
                .find(|project| crate::project::same_path(&project.path, &folder))
                .map(|project| project.id.clone())
        });

        {
            let mut projects = lock(&self.projects);
            let project_id = match project_id {
                Some(id) if projects.get(&id).is_some() => Some(id),
                _ if Path::new(&folder).is_dir() => {
                    Some(projects.register_with_standalone(&folder, true)?.id)
                }
                _ => None,
            };
            if let Some(project_id) = project_id {
                projects.set_standalone(&project_id, true)?;
            }
        }
        self.domains.lock().unwrap().remove(hostname)?;
        self.web.restart_app(hostname);
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
        let (orphan_project, undo_project, undo_domains, folder_to_move) = {
            let domains = self.domains.lock().unwrap();
            let domain = domains.get(hostname).ok_or_else(|| {
                CoreError::DomainError(format!("{hostname} is not a known domain"))
            })?;
            let projects = self.projects.lock().unwrap().list();
            let folder = site_folder(&domain, &projects);
            let project_id = domain.project_id.clone().or_else(|| {
                projects
                    .iter()
                    .find(|project| crate::project::same_path(&project.path, &folder))
                    .map(|project| project.id.clone())
            });
            let orphan_project = project_id.clone().filter(|project_id| {
                !domains
                    .list()
                    .iter()
                    .any(|d| d.hostname != hostname && d.project_id.as_ref() == Some(project_id))
            });
            let undo_project = project_id
                .as_deref()
                .and_then(|id| projects.iter().find(|project| project.id == id).cloned());
            let undo_domains = domains
                .list()
                .into_iter()
                .filter(|candidate| site_folder(candidate, &projects) == folder)
                .collect();
            (orphan_project, undo_project, undo_domains, folder)
        };

        let folder_path = PathBuf::from(&folder_to_move);
        let mut destination = None;
        if folder_path.is_dir() {
            let excluded_folder = self.primary_excluded_folder();
            destination = Some(move_to_excluded_dir(&folder_path, &excluded_folder)?);
        }

        self.domains.lock().unwrap().remove(hostname)?;
        if let Some(ref id) = orphan_project {
            let _ = self.projects.lock().unwrap().remove(id);
        }
        if let Some(destination) = destination {
            self.pending_site_undo.lock().unwrap().insert(
                hostname.to_string(),
                super::super::PendingSiteUndo {
                    source: folder_path,
                    destination,
                    domains: undo_domains,
                    project: undo_project,
                },
            );
        }
        let _ = self.certs.revoke(hostname);
        self.web.restart_app(hostname);

        Ok(())
    }

    /// Reverses a site folder move made during this app session and restores its saved records.
    pub fn restore_excluded_site(&self, hostname: &str) -> Result<(), CoreError> {
        let pending = self.pending_site_undo.lock().unwrap().get(hostname).cloned().ok_or_else(|| {
            CoreError::failed_fix(
                "Undo is no longer available.",
                format!("No excluded-folder move for {hostname} is pending in this app session."),
                "Restore the folder from its excluded directory and add the project again from Projects.",
            )
        })?;
        if pending.source.exists() {
            return Err(CoreError::failed_fix(
                "The original folder location is occupied.",
                pending.source.display().to_string(),
                "Move or rename the existing folder, then try Undo again.",
            ));
        }
        std::fs::rename(&pending.destination, &pending.source).map_err(|e| {
            CoreError::failed_fix(
                "Could not restore the excluded project folder.",
                format!(
                    "{} → {}: {e}",
                    pending.destination.display(),
                    pending.source.display()
                ),
                "Check folder permissions and ensure the excluded folder is still available.",
            )
        })?;
        if let Some(project) = pending.project.clone() {
            if let Err(error) = self.projects.lock().unwrap().restore(project) {
                let _ = std::fs::rename(&pending.source, &pending.destination);
                return Err(error);
            }
        }
        let mut restored_domains = Vec::new();
        for domain in pending.domains.iter().cloned() {
            match self.domains.lock().unwrap().add(domain.clone()) {
                Ok(_) => restored_domains.push(domain),
                Err(error) => {
                    for restored in restored_domains {
                        let _ = self.domains.lock().unwrap().remove(&restored.hostname);
                    }
                    if let Some(project) = pending.project {
                        let _ = self.projects.lock().unwrap().remove(&project.id);
                    }
                    let _ = std::fs::rename(&pending.source, &pending.destination);
                    return Err(error);
                }
            }
        }
        self.pending_site_undo.lock().unwrap().remove(hostname);
        let _ = self.project_events.send(());
        Ok(())
    }

    // ------------------------------------------------------- automatic domains
}
