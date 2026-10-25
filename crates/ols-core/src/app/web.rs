use super::*;
use crate::domain::Domain;
use crate::error::CoreError;
use crate::web::manager::{ApplyContext, ApplyReport};
use crate::web::WebConfig;

impl Inner {
    pub fn apply_web(&self, overwrite: &[String]) -> Result<Vec<ApplyReport>, CoreError> {
        let cfg = self.web_config();
        Self::require_distinct_ports(&cfg)?;
        let php_for = |d: &Domain| -> Option<String> {
            let detail = self.project_detail(d.project_id.as_deref()?)?;
            detail
                .resolved
                .into_iter()
                .find(|r| r.id == "php")
                .and_then(|r| r.installed_version)
        };
        let runtime_bin = |d: &Domain, rt: &str| self.runtime_bin_for(d.project_id.as_deref(), rt);
        let ctx = ApplyContext {
            cfg: &cfg,
            php_for: &php_for,
            runtime_bin: &runtime_bin,
            overwrite,
        };
        let snapshot = self.domains.lock().unwrap().list();
        let mut hashes = Vec::new();
        let reports = self.web.apply(&ctx, &snapshot, &mut hashes)?;
        self.record_generated_hashes(&hashes)?;
        self.ensure_broadcast_tunnels(&snapshot);
        self.sync_all_ddns_background(&snapshot);
        Ok(reports)
    }

    /// Automatically ensures broadcast tunnels are configured and started for all domains with auto_broadcast = true.
    pub fn ensure_broadcast_tunnels(&self, domains: &[Domain]) {
        let cfg = self.web_config();
        let all_tunnels = self.list_tunnels();

        for d in domains {
            let server_id = crate::domain::resolved_server(d, &cfg);
            let _ports = cfg.effective_ports(&server_id);
            let target = d.url();
            let provider = d.broadcast_provider.as_deref().unwrap_or("cloudflare");

            let d_slug = crate::domain::slugify(&d.hostname);
            let mut matching: Vec<crate::tunnel::TunnelStatus> = all_tunnels
                .iter()
                .filter(|t| {
                    if !t.config.provider.eq_ignore_ascii_case(provider) {
                        return false;
                    }
                    d.tunnel_id.as_deref() == Some(&t.config.id)
                        || (d.project_id.is_some() && d.project_id == t.config.project_id)
                        || t.config
                            .target
                            .to_ascii_lowercase()
                            .contains(&d.hostname.to_ascii_lowercase())
                        || t.config
                            .name
                            .to_ascii_lowercase()
                            .contains(&d.hostname.to_ascii_lowercase())
                        || t.config.id.to_ascii_lowercase().contains(&d_slug)
                })
                .cloned()
                .collect();

            if !d.enabled || !d.auto_broadcast {
                continue;
            }

            if matching.len() > 1 {
                let primary_idx = matching
                    .iter()
                    .position(|t| {
                        d.tunnel_id.as_deref() == Some(&t.config.id) || t.state == "connected"
                    })
                    .unwrap_or(0);
                let primary = matching.remove(primary_idx);
                for extra in matching {
                    let _ = self.remove_tunnel(&extra.config.id);
                }
                matching = vec![primary];
            }

            match matching.into_iter().next() {
                Some(mut t) => {
                    let mut needs_save = false;
                    if t.config.target.contains("127.0.0.1") {
                        t.config.target = target.clone();
                        needs_save = true;
                    }
                    if d.public_domain.is_some() && t.config.public_hostname != d.public_domain {
                        t.config.public_hostname = d.public_domain.clone();
                        needs_save = true;
                    }
                    if needs_save {
                        let _ = self.save_tunnel(t.config.clone());
                    }
                    if t.state == "stopped" || t.state == "failed" {
                        let _ = self.start_tunnel(&t.config.id, true);
                    }
                }
                None => {
                    let public_hostname = d.public_domain.clone().or_else(|| {
                        if crate::domain::is_real_domain(&d.hostname) {
                            Some(d.hostname.clone())
                        } else {
                            None
                        }
                    });
                    let config = crate::tunnel::TunnelConfig {
                        id: String::new(),
                        project_id: d.project_id.clone(),
                        name: format!("Broadcast {}", d.hostname),
                        provider: provider.to_string(),
                        target,
                        auth_user: None,
                        allow_internal: false,
                        public_hostname,
                        acknowledged: true,
                        autostart: true,
                    };
                    if let Ok(saved) = self.save_tunnel(config) {
                        let _ = self.start_tunnel(&saved.id, true);
                    }
                }
            }
        }
    }

    /// Triggers a background sync of dynamic DNS for any enabled domains configured with DDNS.
    pub fn sync_all_ddns_background(&self, domains: &[Domain]) {
        let targets: Vec<Domain> = domains
            .iter()
            .filter(|d| {
                d.enabled
                    && d.ddns_provider
                        .as_deref()
                        .map(|p| !p.trim().is_empty())
                        .unwrap_or(false)
            })
            .cloned()
            .collect();
        if targets.is_empty() {
            return;
        }

        tokio::spawn(async move {
            for d in targets {
                let _ = crate::ddns::sync_domain_ddns(&d, false).await;
            }
        });
    }

    /// Applies a single web server, for "Start" on one row of the Services page.
    pub fn apply_web_server(
        &self,
        server_id: &str,
        overwrite: &[String],
    ) -> Result<ApplyReport, CoreError> {
        let cfg = self.web_config();
        Self::require_distinct_ports(&cfg)?;
        let php_for = |d: &Domain| -> Option<String> {
            let detail = self.project_detail(d.project_id.as_deref()?)?;
            detail
                .resolved
                .into_iter()
                .find(|r| r.id == "php")
                .and_then(|r| r.installed_version)
        };
        let runtime_bin = |d: &Domain, rt: &str| self.runtime_bin_for(d.project_id.as_deref(), rt);
        let ctx = ApplyContext {
            cfg: &cfg,
            php_for: &php_for,
            runtime_bin: &runtime_bin,
            overwrite,
        };
        let snapshot = self.domains.lock().unwrap().list();
        let mut hashes = Vec::new();
        let report = self
            .web
            .apply_one(&ctx, &snapshot, server_id, &mut hashes)?;
        self.record_generated_hashes(&hashes)?;
        Ok(report)
    }

    /// Stores the hashes an apply produced, so a hand-edited config file is reported as
    /// drift on the next apply instead of being silently overwritten.
    pub(crate) fn record_generated_hashes(
        &self,
        hashes: &[crate::web::manager::HashUpdate],
    ) -> Result<(), CoreError> {
        if hashes.is_empty() {
            return Ok(());
        }
        let mut domains = self.domains.lock().unwrap();
        for (server, host, hash) in hashes {
            if let Some(mut d) = domains.get(host) {
                d.generated_hashes.insert(server.clone(), hash.clone());
                let _ = domains.update(d);
            }
        }
        Ok(())
    }

    /// Two servers pointed at one port can never both start, so the clash is refused
    /// up front with the fix, rather than surfacing as a bind error halfway through.
    fn require_distinct_ports(cfg: &WebConfig) -> Result<(), CoreError> {
        let clashes = cfg.port_conflicts();
        if clashes.is_empty() {
            return Ok(());
        }
        Err(CoreError::WebError(clashes.join("\n")))
    }

    pub fn write_web_config(
        &self,
        hostname: &str,
        part: crate::web::manager::ConfigPart,
        content: &str,
    ) -> Result<String, CoreError> {
        let cfg = self.web_config();
        let mut domains = self.domains.lock().unwrap();
        self.web
            .write_config(&cfg, &mut domains, hostname, part, content)
    }
}
