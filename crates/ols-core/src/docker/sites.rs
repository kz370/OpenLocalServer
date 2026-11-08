//! §166: sites that point at Docker containers.
//!
//! A Docker site is a normal proxy site (`SiteKind::Proxy`) to the container's published
//! port on 127.0.0.1, plus a `DockerLink` naming the container. When the container comes
//! back on a different port, `sync_links` moves the site with it.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::types::ContainerSummary;
use crate::app::Inner;
use crate::domain::{DockerLink, Domain, SiteKind};
use crate::error::CoreError;

/// What "Create site" sends.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DockerSiteRequest {
    /// Container name or id.
    pub container: String,
    pub container_port: u16,
    pub hostname: String,
    #[serde(default = "yes")]
    pub https: bool,
}

fn yes() -> bool {
    true
}

/// Ports where the container itself speaks HTTPS.
fn speaks_https(container_port: u16) -> bool {
    matches!(container_port, 443 | 8443 | 9443)
}

/// Finds the container a link points at: by name, else by Compose project + service
/// (a recreated container can get a new name suffix).
pub fn find_linked<'a>(
    link: &DockerLink,
    containers: &'a [ContainerSummary],
) -> Option<&'a ContainerSummary> {
    containers
        .iter()
        .find(|c| c.name == link.container || c.id == link.container)
        .or_else(|| {
            let (Some(project), Some(service)) = (&link.compose_project, &link.compose_service)
            else {
                return None;
            };
            containers.iter().find(|c| {
                c.compose_project.as_deref() == Some(project.as_str())
                    && c.compose_service.as_deref() == Some(service.as_str())
            })
        })
}

/// The host port a container publishes for `container_port` (TCP).
pub fn published_port(c: &ContainerSummary, container_port: u16) -> Option<u16> {
    c.ports
        .iter()
        .find(|p| {
            p.container_port == container_port && p.protocol == "tcp" && p.host_port.is_some()
        })
        .and_then(|p| p.host_port)
}

/// Builds the site for a container. Pure, so it is tested without Docker.
pub fn build_site(
    req: &DockerSiteRequest,
    c: &ContainerSummary,
    default_root: &std::path::Path,
) -> Result<Domain, CoreError> {
    let Some(host_port) = published_port(c, req.container_port) else {
        let published: Vec<String> = c
            .ports
            .iter()
            .filter_map(|p| p.host_port.map(|h| format!("{}→{h}", p.container_port)))
            .collect();
        return Err(CoreError::failed_fix(
            format!("Port {} of {} is not reachable from this computer.", req.container_port, c.name),
            if published.is_empty() {
                "The container publishes no ports.".to_string()
            } else {
                format!("It only publishes {}.", published.join(", "))
            },
            format!(
                "Publish the port in the compose file (for example `ports: [\"127.0.0.1:8080:{}\"]`), run Up, then create the site again.",
                req.container_port
            ),
        ));
    };
    let hostname = req.hostname.trim().to_lowercase();
    if hostname.is_empty() || !hostname.contains('.') {
        return Err(CoreError::failed_fix(
            "Enter a domain for the site.",
            format!("\"{hostname}\" is not a domain like `shop.test`."),
            "Type a name ending in your local domain, for example `shop.test`.",
        ));
    }
    // Compose containers keep their project folder as the site root; others use the
    // projects folder (a proxy site serves nothing from disk, any folder will do).
    let root = c
        .compose_file
        .as_deref()
        .map(std::path::Path::new)
        .and_then(|f| f.parent())
        .filter(|d| d.is_dir())
        .map(|d| d.to_path_buf())
        .unwrap_or_else(|| default_root.to_path_buf());
    Ok(Domain {
        hostname,
        root: root.display().to_string(),
        kind: SiteKind::Proxy {
            upstream_port: host_port,
            upstream_host: None,
            upstream_https: speaks_https(req.container_port),
        },
        https: req.https,
        icon: Some("docker".into()),
        docker: Some(DockerLink {
            container: c.name.clone(),
            container_port: req.container_port,
            compose_project: c.compose_project.clone(),
            compose_service: c.compose_service.clone(),
        }),
        ..Default::default()
    })
}

/// Sites whose container now publishes a different port, with the port to move to.
pub fn drifted(domains: &[Domain], containers: &[ContainerSummary]) -> Vec<(String, u16, String)> {
    domains
        .iter()
        .filter_map(|d| {
            let link = d.docker.as_ref()?;
            let SiteKind::Proxy { upstream_port, .. } = &d.kind else {
                return None;
            };
            let c = find_linked(link, containers)?;
            let now = published_port(c, link.container_port)?;
            (now != *upstream_port || c.name != link.container)
                .then(|| (d.hostname.clone(), now, c.name.clone()))
        })
        .collect()
}

impl Inner {
    /// Creates a proxy site for a container's published port. The caller applies the web
    /// config afterwards (the UI does it in the background, as for any new site).
    pub fn docker_create_site(&self, req: &DockerSiteRequest) -> Result<Domain, CoreError> {
        let containers = self.docker.list_containers(&self.docker_endpoint(), true)?;
        let c = containers
            .iter()
            .find(|c| c.name == req.container || c.id == req.container)
            .ok_or_else(|| {
                CoreError::failed_fix(
                    "That container no longer exists.",
                    format!("No container is named \"{}\".", req.container),
                    "Refresh the Docker page and pick the container again.",
                )
            })?;
        let domain = build_site(req, c, &self.default_projects_dir())?;
        self.add_domain(domain)
    }

    /// Moves Docker sites to their container's current port. Returns the hostnames that
    /// changed; the caller re-applies the web config when any did.
    pub fn sync_docker_sites(&self, containers: &[ContainerSummary]) -> Vec<String> {
        let domains = self.domains.lock().unwrap().list();
        let mut changed = Vec::new();
        for (hostname, port, name) in drifted(&domains, containers) {
            let Some(mut d) = domains.iter().find(|d| d.hostname == hostname).cloned() else {
                continue;
            };
            if let SiteKind::Proxy { upstream_port, .. } = &mut d.kind {
                *upstream_port = port;
            }
            if let Some(link) = d.docker.as_mut() {
                link.container = name;
            }
            match self.update_domain(d) {
                Ok(_) => changed.push(hostname),
                Err(e) => {
                    tracing::warn!(%hostname, error = %e, "could not move a Docker site to its new port")
                }
            }
        }
        changed
    }

    /// Re-applies the web config as a background task after sites moved to new ports.
    /// Does nothing when `moved` is empty.
    pub fn docker_apply_site_moves(self: &Arc<Self>, moved: Vec<String>) {
        if moved.is_empty() {
            return;
        }
        let inner = self.clone();
        self.tasks.start(
            "docker-site-sync",
            "Update Docker sites to new ports",
            "docker-sites",
            1,
            move |h| {
                h.begin_step(&format!("Re-apply web config for {}", moved.join(", ")));
                let r = inner.apply_web(&[]).map(|_| ());
                crate::command::report(&h, r);
            },
        );
    }

    /// A Docker site follows its on/off switch: turning it off stops its container, turning it
    /// on starts it. Runs as a background task so the site change never waits for Docker, and
    /// does nothing at all (no Docker request) when none of the sites is linked to a container.
    pub fn docker_follow_enabled(self: &Arc<Self>, hostnames: &[String], enabled: bool) {
        let links: Vec<(String, DockerLink)> = self
            .domains
            .lock()
            .unwrap()
            .list()
            .into_iter()
            .filter(|d| hostnames.contains(&d.hostname))
            .filter_map(|d| d.docker.clone().map(|l| (d.hostname, l)))
            .collect();
        if links.is_empty() {
            return;
        }
        let inner = Arc::clone(self);
        let verb = if enabled { "Start" } else { "Stop" };
        let target = links
            .iter()
            .map(|(h, _)| h.as_str())
            .collect::<Vec<_>>()
            .join(",");
        let title = format!("{verb} Docker containers for {target}");
        self.tasks.start(
            "docker-site-power",
            &title,
            &target,
            links.len(),
            move |h| {
                let ep = inner.docker_endpoint();
                let r = (|| {
                    let containers = inner.docker.list_containers(&ep, true)?;
                    let action = if enabled {
                        crate::docker::ContainerAction::Start
                    } else {
                        crate::docker::ContainerAction::Stop
                    };
                    // Every container is tried: one that fails is reported, never a reason
                    // to leave the others in the wrong state.
                    let mut failed = Vec::new();
                    for (hostname, link) in &links {
                        h.begin_step(&format!("{verb} {} for {hostname}", link.container));
                        match find_linked(link, &containers) {
                            Some(c) => match inner.docker.container_action(&ep, &c.id, action) {
                                Ok(()) => h.end_step(None),
                                Err(e) => {
                                    let d = crate::error::Diagnostic::from(&e);
                                    h.fail_step(d.cause.clone());
                                    h.problem(format!("{hostname}: {} {}", d.problem, d.cause));
                                    failed.push(hostname.clone());
                                }
                            },
                            None => {
                                h.problem(format!(
                                    "{hostname}: container {} no longer exists.",
                                    link.container
                                ));
                                h.end_step(None);
                            }
                        }
                    }
                    if !failed.is_empty() {
                        return Err(CoreError::failed_fix(
                            format!(
                                "Docker could not {} the container for {}.",
                                verb.to_lowercase(),
                                failed.join(", ")
                            ),
                            "Docker refused the request; its message is listed with each site.",
                            format!(
                                "Read the message, then {} the container from the Docker page.",
                                verb.to_lowercase()
                            ),
                        ));
                    }
                    Ok::<(), CoreError>(())
                })();
                crate::command::report(&h, r);
            },
        );
    }

    /// After a rename, sites linked to the old name point at the new one.
    pub fn docker_relink_sites(&self, old: &str, new: &str) {
        let domains = self.domains.lock().unwrap().list();
        for mut d in domains {
            let Some(link) = d.docker.as_mut() else {
                continue;
            };
            if link.container != old {
                continue;
            }
            link.container = new.to_string();
            let hostname = d.hostname.clone();
            if let Err(e) = self.update_domain(d) {
                tracing::warn!(%hostname, error = %e, "could not follow a renamed container");
            }
        }
    }

    /// Fills `ols_site` on containers that a site points at.
    pub fn mark_docker_sites(&self, containers: &mut [ContainerSummary]) {
        let domains = self.domains.lock().unwrap().list();
        for d in domains.iter().filter(|d| d.docker.is_some()) {
            let link = d.docker.as_ref().expect("filtered");
            let target = find_linked(link, containers).map(|c| c.id.clone());
            if let Some(id) = target {
                if let Some(c) = containers.iter_mut().find(|c| c.id == id) {
                    c.ols_site.get_or_insert_with(|| d.hostname.clone());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docker::types::PortBinding;

    fn container(name: &str, host: Option<u16>) -> ContainerSummary {
        ContainerSummary {
            id: format!("id-{name}"),
            name: name.into(),
            image: "nginx:1.28".into(),
            state: "running".into(),
            status: "Up".into(),
            created: 0,
            ports: vec![PortBinding {
                container_port: 80,
                protocol: "tcp".into(),
                host_ip: Some("127.0.0.1".into()),
                host_port: host,
            }],
            compose_project: Some("shop".into()),
            compose_service: Some("web".into()),
            compose_file: None,
            ols_site: None,
            managed: false,
            image_id: String::new(),
            volumes: vec![],
        }
    }

    fn req(port: u16) -> DockerSiteRequest {
        DockerSiteRequest {
            container: "shop-web-1".into(),
            container_port: port,
            hostname: "Shop.test".into(),
            https: true,
        }
    }

    #[test]
    fn site_points_at_the_published_port() {
        let tmp = tempfile::tempdir().unwrap();
        let d = build_site(&req(80), &container("shop-web-1", Some(8080)), tmp.path()).unwrap();
        assert_eq!(d.hostname, "shop.test");
        assert!(d.https);
        assert_eq!(
            d.kind,
            SiteKind::Proxy {
                upstream_port: 8080,
                upstream_host: None,
                upstream_https: false
            }
        );
        let link = d.docker.unwrap();
        assert_eq!(
            (link.container.as_str(), link.container_port),
            ("shop-web-1", 80)
        );
        assert_eq!(link.compose_service.as_deref(), Some("web"));
    }

    #[test]
    fn unpublished_port_explains_how_to_fix_it() {
        let tmp = tempfile::tempdir().unwrap();
        let err = build_site(&req(80), &container("shop-web-1", None), tmp.path()).unwrap_err();
        let d = crate::error::Diagnostic::from(&err);
        assert!(d.problem.contains("not reachable"), "{d:?}");
        assert!(d.fix.unwrap().contains("ports"));
        assert!(build_site(&req(3000), &container("shop-web-1", Some(8080)), tmp.path()).is_err());
    }

    #[test]
    fn drift_follows_port_and_recreated_container() {
        let tmp = tempfile::tempdir().unwrap();
        let site = build_site(&req(80), &container("shop-web-1", Some(8080)), tmp.path()).unwrap();
        // Same port: nothing to do.
        assert!(drifted(
            std::slice::from_ref(&site),
            &[container("shop-web-1", Some(8080))]
        )
        .is_empty());
        // New port after a restart.
        assert_eq!(
            drifted(
                std::slice::from_ref(&site),
                &[container("shop-web-1", Some(8090))]
            ),
            vec![("shop.test".into(), 8090, "shop-web-1".into())]
        );
        // Recreated under another name, same compose service.
        assert_eq!(
            drifted(
                std::slice::from_ref(&site),
                &[container("shop-web-2", Some(8080))]
            ),
            vec![("shop.test".into(), 8080, "shop-web-2".into())]
        );
        // Container gone or stopped without a port: leave the site alone.
        assert!(drifted(&[site], &[container("shop-web-1", None)]).is_empty());
    }
}
