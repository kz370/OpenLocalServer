use super::super::types::*;
use super::super::Core;
use crate::error::CoreError;

impl Core {
    pub(crate) fn dispatch_domains(
        &self,
        command: CoreCommand,
    ) -> Result<Result<CoreResponse, CoreError>, CoreCommand> {
        use CoreCommand as C;
        use CoreResponse as R;
        let i = &self.inner;
        #[allow(unreachable_patterns)]
        let mut unhandled = None;
        let outcome: Result<CoreResponse, CoreError> = (|| {
            let res = match command {
                C::GetWebStatus => Ok(R::WebStatus {
                    status: Box::new(
                        i.web
                            .status(&i.web_config(), &i.domains.lock().unwrap().list()),
                    ),
                }),
                C::GetWebConfig => Ok(R::WebConfig {
                    config: i.web_config(),
                }),
                C::ListDomains => {
                    // Picks up a site the Explorer right-click menu or the `ols` command line
                    // added while the app was running: that write came from another process,
                    // so the in-memory store has to be re-read before it is reported.
                    if i.reload_external_changes() {
                        tracing::info!("re-read the stores after an outside change");
                    }
                    Ok(R::Domains {
                        domains: i.domain_summaries(),
                    })
                }
                C::GetDomain { hostname } => {
                    let d = i.domains.lock().unwrap().get(&hostname).ok_or_else(|| {
                        CoreError::DomainError(format!("{hostname} is not a known domain"))
                    })?;
                    Ok(R::Domain {
                        domain: Box::new(d),
                    })
                }
                C::AddDomain { domain } => {
                    tracing::info!(command = "add_domain", hostname = %domain.hostname);
                    Ok(R::Domain {
                        domain: Box::new(i.add_domain(domain)?),
                    })
                }
                C::UpdateDomain { domain } => Ok(R::Domain {
                    domain: Box::new(i.update_domain(domain)?),
                }),
                C::RemoveDomain { hostname } => {
                    i.remove_domain(&hostname)?;
                    Ok(R::Ok)
                }
                C::DeleteSite { hostname } => {
                    i.delete_site(&hostname)?;
                    Ok(R::Ok)
                }
                C::SetDomainEnabled { hostname, enabled } => {
                    i.set_domain_enabled(&hostname, enabled)?;
                    Ok(R::Ok)
                }
                C::BulkSetDomainsEnabled { hostnames, enabled } => {
                    tracing::info!(
                        command = "bulk_set_domains_enabled",
                        sites = hostnames.len(),
                        enabled
                    );
                    let out = i.set_domains_enabled(&hostnames, enabled)?;
                    Ok(R::Bulk {
                        changed: out.changed,
                        skipped: out.skipped,
                    })
                }
                C::BulkRemoveDomains { hostnames } => {
                    tracing::info!(command = "bulk_remove_domains", sites = hostnames.len());
                    let out = i.remove_domains(&hostnames)?;
                    Ok(R::Bulk {
                        changed: out.changed,
                        skipped: out.skipped,
                    })
                }
                C::BulkDeleteSites { hostnames } => {
                    tracing::info!(command = "bulk_delete_sites", sites = hostnames.len());
                    let out = i.delete_sites(&hostnames)?;
                    Ok(R::Bulk {
                        changed: out.changed,
                        skipped: out.skipped,
                    })
                }
                C::BulkSetDomainServer { hostnames, server } => {
                    tracing::info!(command = "bulk_set_domain_server", sites = hostnames.len(), server = ?server);
                    let out = i.set_domain_servers(&hostnames, server.as_deref())?;
                    Ok(R::Bulk {
                        changed: out.changed,
                        skipped: out.skipped,
                    })
                }
                C::RenameDomain {
                    hostname,
                    new_hostname,
                } => {
                    tracing::info!(command = "rename_domain", from = %hostname, to = %new_hostname);
                    Ok(R::Domain {
                        domain: Box::new(i.rename_domain(&hostname, &new_hostname)?),
                    })
                }
                C::SetDomainIcon { hostname, icon } => {
                    let mut domains = i.domains.lock().unwrap();
                    let mut d = domains.get(&hostname).ok_or_else(|| {
                        CoreError::DomainError(format!("{hostname} is not a known domain"))
                    })?;
                    let clean_icon = icon.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
                    d.icon = clean_icon;
                    domains.update(d)?;
                    Ok(R::Ok)
                }
                C::SetSiteIconImage {
                    hostname,
                    folder,
                    webp_base64,
                } => {
                    let target_dir = if let Some(f) = folder.filter(|s| !s.trim().is_empty()) {
                        std::path::PathBuf::from(f)
                    } else if let Some(h) = hostname {
                        let d = i.domains.lock().unwrap().get(&h).ok_or_else(|| {
                            CoreError::DomainError(format!("{h} is not a known domain"))
                        })?;
                        let projects = i.projects.lock().unwrap().list();
                        std::path::PathBuf::from(crate::app::site_folder(&d, &projects))
                    } else {
                        return Err(CoreError::DomainError(
                            "Neither hostname nor folder was provided".into(),
                        ));
                    };

                    if !target_dir.is_dir() {
                        return Err(CoreError::DomainError(format!(
                            "Folder does not exist: {}",
                            target_dir.display()
                        )));
                    }

                    let raw_b64 = if let Some(idx) = webp_base64.find(',') {
                        &webp_base64[idx + 1..]
                    } else {
                        &webp_base64
                    };
                    let bytes = crate::app::from_base64(raw_b64.trim()).map_err(|e| {
                        CoreError::DomainError(format!("Invalid base64 image data: {e}"))
                    })?;

                    if bytes.len() > 10 * 1024 * 1024 {
                        return Err(CoreError::DomainError(
                            "Image data exceeds 10MB limit".into(),
                        ));
                    }

                    let icon_path = target_dir.join("ols_site_icon.webp");
                    std::fs::write(&icon_path, &bytes).map_err(|e| {
                        CoreError::DomainError(format!("Failed to write ols_site_icon.webp: {e}"))
                    })?;
                    Ok(R::Ok)
                }
                C::RemoveSiteIconImage { hostname, folder } => {
                    let target_dir = if let Some(f) = folder.filter(|s| !s.trim().is_empty()) {
                        std::path::PathBuf::from(f)
                    } else if let Some(h) = hostname {
                        let d = i.domains.lock().unwrap().get(&h).ok_or_else(|| {
                            CoreError::DomainError(format!("{h} is not a known domain"))
                        })?;
                        let projects = i.projects.lock().unwrap().list();
                        std::path::PathBuf::from(crate::app::site_folder(&d, &projects))
                    } else {
                        return Err(CoreError::DomainError(
                            "Neither hostname nor folder was provided".into(),
                        ));
                    };

                    let icon_path = target_dir.join("ols_site_icon.webp");
                    if icon_path.exists() {
                        let _ = std::fs::remove_file(icon_path);
                    }
                    Ok(R::Ok)
                }
                C::ReadImageFile { path } => {
                    let p = std::path::Path::new(&path);
                    if !p.is_file() {
                        return Err(CoreError::DomainError(format!(
                            "Image file does not exist: {path}"
                        )));
                    }
                    let bytes = std::fs::read(p)
                        .map_err(|e| CoreError::DomainError(format!("Failed to read file: {e}")))?;
                    if bytes.len() > 20 * 1024 * 1024 {
                        return Err(CoreError::DomainError(
                            "Image file exceeds 20MB limit".into(),
                        ));
                    }
                    let ext = p
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_lowercase();
                    let mime = match ext.as_str() {
                        "webp" => "image/webp",
                        "png" => "image/png",
                        "jpg" | "jpeg" => "image/jpeg",
                        "svg" => "image/svg+xml",
                        "gif" => "image/gif",
                        "bmp" => "image/bmp",
                        "ico" => "image/x-icon",
                        _ => "application/octet-stream",
                    };
                    let data_url = format!("data:{mime};base64,{}", crate::app::to_base64(&bytes));
                    Ok(R::ImageFile { data_url })
                }
                C::DuplicateDomain {
                    hostname,
                    new_hostname,
                } => Ok(R::Domain {
                    domain: Box::new(i.duplicate_domain(&hostname, &new_hostname)?),
                }),
                C::SuggestDomain {
                    project_id,
                    template,
                } => {
                    let project = i
                        .projects
                        .lock()
                        .unwrap()
                        .get(&project_id)
                        .ok_or_else(|| CoreError::InvalidProjectPath(project_id.clone()))?;
                    Ok(R::Text {
                        text: crate::domain::apply_template(&template, &project.name),
                    })
                }
                C::ApplyWeb { overwrite } => {
                    tracing::info!(command = "apply_web");
                    let retry = CoreCommand::ApplyWeb { overwrite: vec![] };
                    let reports = i.journaled(
                        "apply_web",
                        "Apply the web configuration",
                        Some(
                            "Restore the previous config from a site's history on the Config page.",
                        ),
                        Some(retry),
                        || i.apply_web(&overwrite),
                    )?;
                    Ok(R::Applied { reports })
                }
                C::StopWeb => {
                    i.web.stop();
                    Ok(R::Ok)
                }
                C::ValidateWeb => Ok(R::Text {
                    text: i.web.validate(&i.web_config())?,
                }),
                C::RestartSiteApp { hostname } => {
                    i.web.restart_app(&hostname);
                    i.apply_web(&[])?;
                    Ok(R::Ok)
                }
                C::SyncHosts => {
                    // Names our local DNS already answers for don't need a hosts entry.
                    let hostnames: Vec<String> = i
                        .domains
                        .lock()
                        .unwrap()
                        .list()
                        .into_iter()
                        .filter(|d| d.enabled && !i.web.dns_covers(&d.hostname))
                        .map(|d| d.hostname)
                        .collect();
                    let changed = crate::hosts::ensure(&hostnames).map_err(CoreError::WebError)?;
                    Ok(R::Text {
                        text: if changed {
                            "The hosts file was updated.".into()
                        } else {
                            "The hosts file was already up to date.".into()
                        },
                    })
                }
                other => {
                    unhandled = Some(other);
                    return Ok(R::Ok);
                }
            };
            res
        })();
        if let Some(cmd) = unhandled {
            Err(cmd)
        } else {
            Ok(outcome)
        }
    }
}
