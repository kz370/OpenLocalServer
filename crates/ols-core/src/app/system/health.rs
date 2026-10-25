use crate::app::system::*;
use crate::app::*;
use crate::dbtools;
use crate::domain::Domain;
use crate::error::CoreError;
use crate::health::{HealthReport, HealthTarget};
use std::path::{Path, PathBuf};

impl Inner {
    pub fn health_check(&self, hostname: &str) -> Result<HealthReport, CoreError> {
        let cfg = self.web_config();
        let d =
            self.domains.lock().unwrap().get(hostname).ok_or_else(|| {
                CoreError::DomainError(format!("{hostname} is not a known domain"))
            })?;
        let cert = self.certs.info(hostname);
        let ca_info = self.certs.ca_info();
        let ca_path = PathBuf::from(&ca_info.cert_path);
        Ok(crate::health::check_site(&HealthTarget {
            hostname,
            https: d.https,
            http_port: cfg.http_port(),
            https_port: cfg.https_port(),
            ca_pem: &ca_path,
            ca_trusted: ca_info.trusted,
            cert: cert.as_ref(),
        }))
    }

    // ------------------------------------------------------------------ databases

    pub fn environment_health(&self) -> Vec<HealthItem> {
        let cfg = self.web_config();
        let web = self.web.status(&cfg, &self.domains.lock().unwrap().list());
        let services = self.services.list();
        self.environment_health_with(&web, &services)
    }

    /// [`Self::environment_health`] over an already-computed web status and service list.
    /// The dashboard needs both for its own cards, so recomputing them here doubled the
    /// runtime-folder scans, PHP-pool reads and TCP probes on a page polled every 3s.
    pub fn environment_health_with(
        &self,
        web: &crate::web::manager::WebStatus,
        services: &[crate::service::ServiceStatus],
    ) -> Vec<HealthItem> {
        let cfg = self.web_config();
        let mut items = Vec::new();
        let item =
            |id: &str, label: &str, status: &str, detail: String, fix: Option<&str>| HealthItem {
                id: id.into(),
                label: label.into(),
                status: status.into(),
                detail,
                fix: fix.map(str::to_string),
            };

        let server_name = crate::web::server_by_id(cfg.server())
            .map(|s| s.name())
            .unwrap_or("Web server");
        let server_installed = !self.runtimes.installed_versions(cfg.server()).is_empty();
        items.push(if server_installed {
            item(
                "web_installed",
                server_name,
                "ok",
                format!("{server_name} is installed"),
                None,
            )
        } else {
            item(
                "web_installed",
                server_name,
                "error",
                format!("{server_name} is not installed"),
                Some("Install it from the Runtimes page."),
            )
        });

        let domain_count = self
            .domains
            .lock()
            .unwrap()
            .list()
            .iter()
            .filter(|d| d.enabled)
            .count();
        if web.running {
            items.push(item(
                "web_running",
                "Web server",
                "ok",
                format!(
                    "running on ports {} / {}",
                    cfg.http_port(),
                    cfg.https_port()
                ),
                None,
            ));
        } else if domain_count > 0 {
            items.push(item(
                "web_running",
                "Web server",
                "warn",
                "stopped, so your sites are offline".into(),
                Some("Apply the web config from the Domains page."),
            ));
        }
        for conflict in &web.port_conflicts {
            items.push(item(
                "port_conflict",
                "Ports",
                "warn",
                conflict.clone(),
                Some("Stop that program or choose other ports in the web settings."),
            ));
        }

        let ca = self.certs.ca_info();
        let any_https = self.domains.lock().unwrap().list().iter().any(|d| d.https);
        if any_https || ca.exists {
            items.push(if ca.trusted {
                item(
                    "ca_trusted",
                    "Local certificate authority",
                    "ok",
                    "trusted by Windows".into(),
                    None,
                )
            } else {
                item(
                    "ca_trusted",
                    "Local certificate authority",
                    "warn",
                    "not trusted, browsers will warn about HTTPS sites".into(),
                    Some("Trust it from the Certificates page."),
                )
            });
        }
        for cert in self.certs.list() {
            match cert.status {
                crate::certs::CertStatus::Expired => items.push(item(
                    "cert_expired",
                    &format!("Certificate {}", cert.hostname),
                    "error",
                    "expired".into(),
                    Some("Regenerate it from the Certificates page."),
                )),
                crate::certs::CertStatus::Expiring => items.push(item(
                    "cert_expiring",
                    &format!("Certificate {}", cert.hostname),
                    "warn",
                    format!("expires in {} days", cert.days_left),
                    Some("It renews on the next apply."),
                )),
                crate::certs::CertStatus::Valid => {}
            }
        }

        let enabled: Vec<String> = self
            .domains
            .lock()
            .unwrap()
            .list()
            .into_iter()
            .filter(|d| d.enabled)
            .map(|d| d.hostname)
            .collect();
        if !enabled.is_empty() {
            let existing = std::fs::read_to_string(crate::hosts::hosts_path()).unwrap_or_default();
            let missing: Vec<&String> = enabled
                .iter()
                .filter(|h| !self.web.dns_covers(h) && !crate::hosts::lists(&existing, h))
                .collect();
            items.push(if missing.is_empty() {
                item(
                    "hosts",
                    "Domain names",
                    "ok",
                    "every domain resolves to this computer".into(),
                    None,
                )
            } else {
                item(
                    "hosts",
                    "Domain names",
                    "warn",
                    format!(
                        "not resolving yet: {}",
                        missing
                            .iter()
                            .map(|s| s.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    Some("Apply the web config."),
                )
            });
        }

        if self.php.all_versions().is_empty() {
            items.push(item(
                "php",
                "PHP",
                "warn",
                "no PHP version is installed".into(),
                Some("Install one from the Runtimes page."),
            ));
        }
        for s in services.iter().filter(|s| s.installed) {
            items.push(match (s.running, s.healthy) {
                (true, Some(false)) => item(
                    &format!("svc_{}", s.id),
                    &s.name,
                    "warn",
                    "running but not answering on its port".into(),
                    Some("Restart the service."),
                ),
                (true, _) => item(
                    &format!("svc_{}", s.id),
                    &s.name,
                    "ok",
                    "running".into(),
                    None,
                ),
                (false, _) => item(
                    &format!("svc_{}", s.id),
                    &s.name,
                    "ok",
                    "stopped".into(),
                    None,
                ),
            });
        }
        items.extend(self.plugin_health());
        items
    }

    // ------------------------------------------------------------- monitoring

    /// Machine and process usage, plus what each enabled site costs. Shared processes (a
    /// PHP version's workers, the web server) are reported whole, with how many sites
    /// share them, rather than split by guesswork.
    pub fn system_stats(&self) -> crate::monitor::SystemStats {
        let procs = self.supervisor.snapshot();
        let pid_of = |id: ProcessId| procs.iter().find(|p| p.id == id).and_then(|p| p.pid);
        let mut stats = self
            .monitor
            .stats(&procs.iter().filter_map(|p| p.pid).collect::<Vec<_>>());
        let plan = self.web.usage_plan();
        let sum = |ids: &[ProcessId]| {
            ids.iter()
                .filter_map(|id| pid_of(*id))
                .filter_map(|pid| stats.processes.get(&pid))
                .fold((0.0f32, 0u64), |(c, m), s| {
                    (c + s.cpu_percent, m + s.memory)
                })
        };
        let enabled: Vec<Domain> = self
            .domains
            .lock()
            .unwrap()
            .list()
            .into_iter()
            .filter(|d| d.enabled)
            .collect();
        let server = sum(&plan.servers);
        let static_sites = enabled
            .iter()
            .filter(|d| {
                !plan.apps.contains_key(&d.hostname) && !plan.site_php.contains_key(&d.hostname)
            })
            .count();
        let mut sites = Vec::new();
        for d in &enabled {
            let (via, (cpu, memory), shared_by, measured) =
                if let Some(app) = plan.apps.get(&d.hostname) {
                    ("App process".to_string(), sum(&[*app]), 1, true)
                } else if let Some(version) = plan.site_php.get(&d.hostname) {
                    let workers = plan.pools.get(version).cloned().unwrap_or_default();
                    let sharing = plan.site_php.values().filter(|v| *v == version).count();
                    (
                        format!("PHP {version} workers"),
                        sum(&workers),
                        sharing,
                        true,
                    )
                } else if let SiteKind::Proxy {
                    upstream_host: Some(host),
                    upstream_port,
                    ..
                } = &d.kind
                {
                    (
                        format!("Forwarded to {host}:{upstream_port}"),
                        (0.0, 0),
                        1,
                        false,
                    )
                } else {
                    ("Web server".to_string(), server, static_sites.max(1), true)
                };
            sites.push(crate::monitor::SiteUsage {
                hostname: d.hostname.clone(),
                via,
                cpu_percent: cpu,
                memory,
                shared_by,
                measured,
                disk: None,
            });
        }
        let projects = self.projects.lock().unwrap().list();
        let folders: Vec<(String, PathBuf)> = enabled
            .iter()
            .filter(|d| {
                !matches!(
                    d.kind,
                    SiteKind::Proxy {
                        upstream_host: Some(_),
                        ..
                    }
                )
            })
            .map(|d| (d.hostname.clone(), PathBuf::from(site_folder(d, &projects))))
            .collect();
        let sizes = self
            .monitor
            .folder_sizes(&folders.iter().map(|(_, f)| f.clone()).collect::<Vec<_>>());
        for site in &mut sites {
            if let Some((_, folder)) = folders.iter().find(|(h, _)| *h == site.hostname) {
                site.disk = sizes.get(folder).copied();
            }
        }
        stats.sites = sites;

        let mut places = vec![("OLS data".to_string(), self.paths.root().to_path_buf())];
        places.extend(
            self.string_list(PROJECT_ROOTS)
                .into_iter()
                .map(|r| ("Projects".to_string(), PathBuf::from(r))),
        );
        places.push(("Projects".to_string(), self.default_projects_dir()));
        stats.disks = self.monitor.disks(&places);
        stats
    }

    // --------------------------------------------------------- database migration
}
