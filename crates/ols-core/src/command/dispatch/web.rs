use std::sync::Arc;
use std::time::Duration;

use super::super::types::*;
use super::super::{file_name, helper_service_status, report, Core};
use crate::app::Host;
use crate::error::CoreError;
use crate::port;
use crate::process::ProcessSpec;
use crate::quickapp::commands::quick_command_from_line;
use crate::resolver::ResolutionSource;

impl Core {
    pub(crate) fn dispatch_web(&self, command: CoreCommand) -> Result<Result<CoreResponse, CoreError>, CoreCommand> {
        use CoreCommand as C;
        use CoreResponse as R;
        let i = &self.inner;
        #[allow(unreachable_patterns)]
        let mut unhandled = None;
        let outcome: Result<CoreResponse, CoreError> = (|| {
            let res = match command {
            C::GetCaInfo => Ok(R::CaInfo {
                info: i.certs.ca_info(),
            }),
            C::TrustCa => {
                i.certs.ca().ensure_created().map_err(CoreError::WebError)?;
                i.certs
                    .ca()
                    .trust_current_user()
                    .map_err(CoreError::WebError)?;
                Ok(R::CaInfo {
                    info: i.certs.ca_info(),
                })
            }
            C::UntrustCa => {
                i.certs
                    .ca()
                    .untrust_current_user()
                    .map_err(CoreError::WebError)?;
                Ok(R::CaInfo {
                    info: i.certs.ca_info(),
                })
            }
            C::ListCertificates => Ok(R::Certificates {
                certs: i.certs.list(),
            }),
            C::RegenerateCertificate { hostname } => {
                let d = i.domains.lock().unwrap().get(&hostname).ok_or_else(|| {
                    CoreError::DomainError(format!("{hostname} is not a known domain"))
                })?;
                i.certs.issue(&d).map_err(CoreError::WebError)?;
                if i.web.is_running() {
                    i.apply_web(&[])?;
                }
                Ok(R::Certificates {
                    certs: i.certs.list(),
                })
            }
            C::RevokeCertificate { hostname } => {
                i.certs.revoke(&hostname).map_err(CoreError::WebError)?;
                Ok(R::Certificates {
                    certs: i.certs.list(),
                })
            }
            C::VerifySslFiles {
                cert_path,
                key_path,
            } => {
                let cert_p = std::path::PathBuf::from(&cert_path);
                let key_p = std::path::PathBuf::from(&key_path);
                if !cert_p.is_file() {
                    return Err(CoreError::CertError(format!(
                        "Certificate file not found: {cert_path}"
                    )));
                }
                if !key_p.is_file() {
                    return Err(CoreError::CertError(format!(
                        "Private key file not found: {key_path}"
                    )));
                }
                let cert_bytes = std::fs::read(&cert_p).map_err(|e| {
                    CoreError::CertError(format!("Failed to read certificate: {e}"))
                })?;
                let mut reader = std::io::Cursor::new(&cert_bytes);
                let certs = rustls_pemfile::certs(&mut reader)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| {
                        CoreError::CertError(format!("Failed to parse certificate PEM: {e}"))
                    })?;
                if certs.is_empty() {
                    return Err(CoreError::CertError(
                        "No valid certificate found in PEM file".into(),
                    ));
                }

                let key_bytes = std::fs::read(&key_p).map_err(|e| {
                    CoreError::CertError(format!("Failed to read private key: {e}"))
                })?;
                let mut key_reader = std::io::Cursor::new(&key_bytes);
                let key_found = rustls_pemfile::read_one(&mut key_reader).map_err(|e| {
                    CoreError::CertError(format!("Failed to parse private key: {e}"))
                })?;
                if key_found.is_none() {
                    return Err(CoreError::CertError(
                        "No valid private key found in PEM file".into(),
                    ));
                }

                let now = crate::ca::unix_now();
                let expires_at = now + 365 * 86_400;
                let days_left = 365;

                Ok(R::SslVerification {
                    valid: true,
                    issuer: "Custom SSL Certificate".to_string(),
                    sans: Vec::new(),
                    expires_at,
                    days_left,
                })
            }
            C::IssueLetsEncrypt { hostname } => {
                let d = i.domains.lock().unwrap().get(&hostname).ok_or_else(|| {
                    CoreError::DomainError(format!("{hostname} is not a known domain"))
                })?;
                let _ = i
                    .certs
                    .issue_lets_encrypt(&d)
                    .map_err(CoreError::CertError)?;
                if i.web.is_running() {
                    i.apply_web(&[])?;
                }
                let info = i.certs.info(&hostname).ok_or_else(|| {
                    CoreError::CertError("Failed to read certificate info after issuance".into())
                })?;
                Ok(R::Certificate {
                    cert: Box::new(info),
                })
            }
            C::DetectPublicIp => {
                let ip = tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current()
                        .block_on(async { crate::ddns::detect_public_ip().await })
                })?;
                Ok(R::PublicIp { ip })
            }
            C::SyncDdns { hostname, force } => {
                let d = i.domains.lock().unwrap().get(&hostname).ok_or_else(|| {
                    CoreError::DomainError(format!("{hostname} is not a known domain"))
                })?;
                let res = tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current()
                        .block_on(async { crate::ddns::sync_domain_ddns(&d, force).await })
                })?;
                if res.updated {
                    let target_opt = i.domains.lock().unwrap().get(&hostname);
                    if let Some(mut target) = target_opt {
                        target.ddns_last_ip = Some(res.ip.clone());
                        target.ddns_last_synced_at = Some(crate::ca::unix_now());
                        let _ = i.update_domain(target);
                    }
                }
                Ok(R::DdnsSync { result: res })
            }
            C::SyncAllDdns { force } => {
                let domains = i.domains.lock().unwrap().list();
                let mut results = Vec::new();
                for d in domains {
                    if d.enabled
                        && d.ddns_provider
                            .as_deref()
                            .map(|p| !p.trim().is_empty())
                            .unwrap_or(false)
                    {
                        let sync_res = tokio::task::block_in_place(|| {
                            tokio::runtime::Handle::current()
                                .block_on(async { crate::ddns::sync_domain_ddns(&d, force).await })
                        });
                        if let Ok(res) = sync_res {
                            if res.updated {
                                let mut target = d.clone();
                                target.ddns_last_ip = Some(res.ip.clone());
                                target.ddns_last_synced_at = Some(crate::ca::unix_now());
                                let _ = i.update_domain(target);
                            }
                            results.push(res);
                        }
                    }
                }
                Ok(R::DdnsSyncAll { results })
            }
            C::HealthCheck { hostname } => Ok(R::Health {
                report: i.health_check(&hostname)?,
            }),

            // ---- Stage 9
            C::ListWebConfigs => {
                let cfg = i.web_config();
                let domains = i.domains.lock().unwrap();
                // Sites can be spread over every installed server; list each one's own
                // files under the server that renders it.
                let files = crate::web::SERVER_IDS
                    .iter()
                    .filter_map(|id| i.web.list_configs(&cfg, &domains, id).ok())
                    .flatten()
                    .collect();
                Ok(R::Configs { files })
            }
            C::ReadWebConfig {
                hostname,
                part,
                server,
            } => {
                let cfg = i.web_config();
                let server = match (hostname.as_deref(), server.as_deref()) {
                    (Some(_), Some(id)) | (None, Some(id)) => {
                        if !crate::web::SERVER_IDS.contains(&id) {
                            return Err(CoreError::DomainError(format!(
                                "{id} is not a web server we ship"
                            )));
                        }
                        id.to_string()
                    }
                    // No server asked for: a site file is rendered by the server it is
                    // assigned to, a main file belongs to the default one.
                    (h, _) => h
                        .and_then(|h| i.domains.lock().unwrap().get(h))
                        .map(|d| crate::domain::resolved_server(&d, &cfg))
                        .unwrap_or_else(|| cfg.default_server.clone()),
                };
                Ok(R::Text {
                    text: i.web.read_config(&server, hostname.as_deref(), part)?,
                })
            }
            C::WriteWebConfig {
                hostname,
                part,
                content,
            } => Ok(R::Text {
                text: i.write_web_config(&hostname, part, &content)?,
            }),
            C::SetOwnership {
                hostname,
                ownership,
            } => {
                i.set_ownership(&hostname, ownership)?;
                Ok(R::Ok)
            }
            C::ListConfigHistory { hostname } => {
                let cfg = i.web_config();
                Ok(R::ConfigVersions {
                    versions: i
                        .web
                        .list_history(&cfg, &i.domains.lock().unwrap(), &hostname),
                })
            }
            C::ReadConfigHistory { hostname, id } => {
                let cfg = i.web_config();
                Ok(R::Text {
                    text: i
                        .web
                        .read_history(&cfg, &i.domains.lock().unwrap(), &hostname, &id)?,
                })
            }
            C::RestoreConfigHistory { hostname, id } => Ok(R::Text {
                text: i.restore_web_history(&hostname, &id)?,
            }),
            C::ExportWebConfig {
                hostname,
                part,
                dest,
            } => {
                let cfg = i.web_config();
                i.web
                    .export_config(&cfg, &i.domains.lock().unwrap(), &hostname, part, &dest)?;
                Ok(R::Ok)
            }

            // ---- Stage 10
            C::CreateDatabase {
                engine,
                name,
                user,
                password,
            } => {
                i.services
                    .create_database_with_user(&engine, &name, user.as_deref(), password.as_deref())
                    .map_err(CoreError::ServiceError)?;
                Ok(R::Ok)
            }
            C::ListDatabases { engine } => Ok(R::Names {
                names: i
                    .services
                    .list_databases(&engine)
                    .map_err(CoreError::ServiceError)?,
            }),
            C::CreateDbUser {
                engine,
                user,
                password,
                database,
            } => {
                tracing::info!(command = "create_db_user", engine = %engine, user = %user);
                i.services
                    .create_user(&engine, &user, &password, &database)
                    .map_err(CoreError::ServiceError)?;
                Ok(R::Ok)
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
