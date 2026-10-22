//! Certificate Manager (§51, §142 — Stage 6): per-domain leaf certificates signed by the
//! local CA — generate, renew, revoke, regenerate, expiry and trust checks.
//! Private keys are never returned over IPC, logged, or shown: `CertInfo` carries the
//! key's *path* only (§142).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::ca::{unix_now, LocalCa, CA_COMMON_NAME};
use crate::domain::Domain;
use crate::paths::AppPaths;

/// Certificates inside this window are renewed on the next apply.
const RENEW_WITHIN_DAYS: i64 = 30;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CertMeta {
    hostname: String,
    sans: Vec<String>,
    issued_at: u64,
    expires_at: u64,
    project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    provider: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CertStatus {
    Valid,
    /// Inside the renewal window.
    Expiring,
    Expired,
}

/// §51 detail view: issuer, domains, dates, trust, project, cert path, key path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CertInfo {
    pub hostname: String,
    pub sans: Vec<String>,
    pub issuer: String,
    pub issued_at: u64,
    pub expires_at: u64,
    pub days_left: i64,
    pub status: CertStatus,
    /// Whether the issuing CA is trusted by the OS — a leaf is only as trusted as its CA.
    pub trusted: bool,
    pub project_id: Option<String>,
    pub cert_path: String,
    pub key_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaInfo {
    pub exists: bool,
    pub trusted: bool,
    pub common_name: String,
    pub cert_path: String,
}

pub struct CertificateManager {
    ca: LocalCa,
    sites_dir: PathBuf,
    paths: AppPaths,
}

/// Paths a web server config needs to reference.
#[derive(Debug, Clone)]
pub struct CertPaths {
    pub cert: PathBuf,
    pub key: PathBuf,
}

impl CertificateManager {
    pub fn new(paths: &AppPaths) -> Self {
        Self {
            ca: LocalCa::new(paths),
            sites_dir: paths.certs_dir().join("sites"),
            paths: paths.clone(),
        }
    }

    pub fn ca(&self) -> &LocalCa {
        &self.ca
    }

    fn dir_for(&self, hostname: &str) -> PathBuf {
        self.sites_dir.join(hostname)
    }

    fn paths_for(&self, hostname: &str) -> CertPaths {
        let dir = self.dir_for(hostname);
        CertPaths {
            cert: dir.join("cert.pem"),
            key: dir.join("key.pem"),
        }
    }

    fn read_meta(&self, hostname: &str) -> Option<CertMeta> {
        let raw = std::fs::read_to_string(self.dir_for(hostname).join("meta.json")).ok()?;
        serde_json::from_str(&raw).ok()
    }

    pub fn ca_info(&self) -> CaInfo {
        CaInfo {
            exists: self.ca.exists(),
            trusted: self.ca.exists() && self.ca.is_trusted(),
            common_name: CA_COMMON_NAME.to_string(),
            cert_path: self.ca.ca_cert_pem_path().display().to_string(),
        }
    }

    /// Names a domain's certificate must cover: primary hostname, aliases, and wildcard.
    pub fn names_for(domain: &Domain) -> Vec<String> {
        let mut names = vec![domain.hostname.clone()];
        for alias in &domain.aliases {
            let clean = alias.trim();
            if !clean.is_empty() && !names.contains(&clean.to_string()) {
                names.push(clean.to_string());
            }
        }
        if domain.wildcard {
            names.push(format!("*.{}", domain.hostname));
        }
        names
    }

    /// Makes sure a valid certificate exists for `domain` and returns where it lives.
    /// Reissues when missing, when the covered names changed (e.g. wildcard toggled), or
    /// when it's inside the renewal window (§51 renew).
    pub fn ensure_for(&self, domain: &Domain) -> Result<CertPaths, String> {
        match domain.ssl_provider {
            crate::domain::SslProvider::LocalCa => {
                let names = Self::names_for(domain);
                let paths = self.paths_for(&domain.hostname);
                let reusable = self.read_meta(&domain.hostname).is_some_and(|m| {
                    m.sans == names
                        && paths.cert.is_file()
                        && paths.key.is_file()
                        && days_between(unix_now(), m.expires_at) > RENEW_WITHIN_DAYS
                });
                if reusable {
                    return Ok(paths);
                }
                self.issue(domain)
            }
            crate::domain::SslProvider::Custom => {
                let cert = domain.custom_cert_path.as_deref().unwrap_or("").trim();
                let key = domain.custom_key_path.as_deref().unwrap_or("").trim();
                if cert.is_empty() || key.is_empty() {
                    return Err("Custom SSL requires both certificate and private key paths".into());
                }
                let cert_p = PathBuf::from(cert);
                let key_p = PathBuf::from(key);
                if !cert_p.is_file() {
                    return Err(format!("Custom certificate file not found: {cert}"));
                }
                if !key_p.is_file() {
                    return Err(format!("Custom private key file not found: {key}"));
                }
                let dir = self.dir_for(&domain.hostname);
                let _ = std::fs::create_dir_all(&dir);
                let now = unix_now();
                let meta = CertMeta {
                    hostname: domain.hostname.clone(),
                    sans: Self::names_for(domain),
                    issued_at: now,
                    expires_at: now + 365 * 86_400,
                    project_id: domain.project_id.clone(),
                    provider: Some("custom".into()),
                };
                let _ = std::fs::write(
                    dir.join("meta.json"),
                    serde_json::to_string_pretty(&meta).unwrap_or_default(),
                );
                Ok(CertPaths {
                    cert: cert_p,
                    key: key_p,
                })
            }
            crate::domain::SslProvider::LetsEncrypt => self.ensure_lets_encrypt(domain),
        }
    }

    /// Ensures a Let's Encrypt certificate is issued or reused for `domain`.
    pub fn ensure_lets_encrypt(&self, domain: &Domain) -> Result<CertPaths, String> {
        let paths = self.paths_for(&domain.hostname);
        let reusable = self.read_meta(&domain.hostname).is_some_and(|m| {
            paths.cert.is_file()
                && paths.key.is_file()
                && days_between(unix_now(), m.expires_at) > RENEW_WITHIN_DAYS
        });
        if reusable {
            return Ok(paths);
        }
        self.issue_lets_encrypt(domain)
    }

    /// Issues a new Let's Encrypt certificate using ACME HTTP-01 challenge.
    pub fn issue_lets_encrypt(&self, domain: &Domain) -> Result<CertPaths, String> {
        crate::domain::validate_public_hostname(&domain.hostname).map_err(|e| e.to_string())?;

        let account_key = self
            .paths
            .certs_dir()
            .join("letsencrypt")
            .join("account.key");
        let doc_root = PathBuf::from(&domain.root);
        let email = domain.lets_encrypt_email.as_deref();

        let paths = self.paths_for(&domain.hostname);
        let mut client =
            crate::acme::AcmeClient::new(None, &account_key).map_err(|e| e.to_string())?;

        let issued = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                client
                    .issue_certificate(&domain.hostname, &doc_root, email)
                    .await
            })
        })
        .map_err(|e| e.to_string())?;

        let dir = self.dir_for(&domain.hostname);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        std::fs::write(&paths.cert, issued.cert_pem).map_err(|e| e.to_string())?;
        crate::ca::write_key_restricted(&paths.key, issued.key_pem.as_bytes())?;

        let now = unix_now();
        let meta = CertMeta {
            hostname: domain.hostname.clone(),
            sans: vec![domain.hostname.clone()],
            issued_at: now,
            expires_at: now + 90 * 86_400,
            project_id: domain.project_id.clone(),
            provider: Some("lets_encrypt".into()),
        };
        std::fs::write(
            dir.join("meta.json"),
            serde_json::to_string_pretty(&meta).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;

        Ok(paths)
    }

    /// Unconditionally (re)generates the certificate (§51 regenerate).
    pub fn issue(&self, domain: &Domain) -> Result<CertPaths, String> {
        match domain.ssl_provider {
            crate::domain::SslProvider::LetsEncrypt => self.issue_lets_encrypt(domain),
            crate::domain::SslProvider::Custom => self.ensure_for(domain),
            crate::domain::SslProvider::LocalCa => {
                let names = Self::names_for(domain);
                let issued = self.ca.issue_for(&names)?;
                let dir = self.dir_for(&domain.hostname);
                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

                let paths = self.paths_for(&domain.hostname);
                std::fs::write(&paths.cert, issued.cert_pem).map_err(|e| e.to_string())?;
                crate::ca::write_key_restricted(&paths.key, issued.key_pem.as_bytes())?;

                let now = unix_now();
                let meta = CertMeta {
                    hostname: domain.hostname.clone(),
                    sans: names,
                    issued_at: now,
                    expires_at: now + crate::ca::LEAF_VALIDITY_DAYS * 86_400,
                    project_id: domain.project_id.clone(),
                    provider: Some("local_ca".into()),
                };
                std::fs::write(
                    dir.join("meta.json"),
                    serde_json::to_string_pretty(&meta).map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
                Ok(paths)
            }
        }
    }

    /// §51 revoke: deletes the certificate and key. The web server can no longer serve the
    /// site over HTTPS until one is generated again.
    pub fn revoke(&self, hostname: &str) -> Result<(), String> {
        let dir = self.dir_for(hostname);
        if dir.exists() {
            std::fs::remove_dir_all(dir).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub fn info(&self, hostname: &str) -> Option<CertInfo> {
        let meta = self.read_meta(hostname)?;
        let paths = self.paths_for(hostname);
        let days_left = days_between(unix_now(), meta.expires_at);
        let status = if days_left < 0 {
            CertStatus::Expired
        } else if days_left <= RENEW_WITHIN_DAYS {
            CertStatus::Expiring
        } else {
            CertStatus::Valid
        };
        let issuer = match meta.provider.as_deref() {
            Some("lets_encrypt") => "Let's Encrypt Authority".to_string(),
            Some("custom") => "Custom SSL Certificate".to_string(),
            _ => CA_COMMON_NAME.to_string(),
        };
        let trusted = match meta.provider.as_deref() {
            Some("lets_encrypt") | Some("custom") => true,
            _ => self.ca.is_trusted(),
        };
        Some(CertInfo {
            hostname: meta.hostname,
            sans: meta.sans,
            issuer,
            issued_at: meta.issued_at,
            expires_at: meta.expires_at,
            days_left,
            status,
            trusted,
            project_id: meta.project_id,
            cert_path: paths.cert.display().to_string(),
            key_path: paths.key.display().to_string(),
            provider: meta.provider,
        })
    }

    pub fn list(&self) -> Vec<CertInfo> {
        let Ok(entries) = std::fs::read_dir(&self.sites_dir) else {
            return Vec::new();
        };
        let mut all: Vec<CertInfo> = entries
            .flatten()
            .filter_map(|e| e.file_name().to_str().map(str::to_string))
            .filter_map(|host| self.info(&host))
            .collect();
        all.sort_by(|a, b| a.hostname.cmp(&b.hostname));
        all
    }
}

fn days_between(from: u64, to: u64) -> i64 {
    (to as i64 - from as i64).div_euclid(86_400)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Domain, Ownership, SiteBlocks, SiteKind};

    fn domain(host: &str, wildcard: bool) -> Domain {
        Domain {
            hostname: host.into(),
            project_id: Some("p1".into()),
            root: "C:/x".into(),
            kind: SiteKind::Static,
            https: true,
            redirect_https: false,
            wildcard,
            enabled: true,
            ownership: Ownership::Managed,
            app: None,
            blocks: SiteBlocks::default(),
            generated_hashes: Default::default(),
            public_domain: None,
            tunnel_id: None,
            server: None,
            path_prefix: None,
            icon: None,
            ssl_provider: crate::domain::SslProvider::LocalCa,
            custom_cert_path: None,
            custom_key_path: None,
            lets_encrypt_email: None,
            auto_broadcast: false,
            broadcast_provider: None,
            ..Default::default()
        }
    }

    #[test]
    fn custom_ssl_uses_provided_files_and_writes_meta() {
        let home = crate::test_support::isolated_home();
        let mgr = CertificateManager::new(&home.paths);

        let cert_file = home.paths.root().join("my-cert.pem");
        let key_file = home.paths.root().join("my-key.pem");
        std::fs::write(&cert_file, "fake cert data").unwrap();
        std::fs::write(&key_file, "fake key data").unwrap();

        let mut d = domain("mycustom.org", false);
        d.ssl_provider = crate::domain::SslProvider::Custom;
        d.custom_cert_path = Some(cert_file.display().to_string());
        d.custom_key_path = Some(key_file.display().to_string());

        let paths = mgr.ensure_for(&d).unwrap();
        assert_eq!(paths.cert, cert_file);
        assert_eq!(paths.key, key_file);

        let info = mgr.info("mycustom.org").unwrap();
        assert_eq!(info.issuer, "Custom SSL Certificate");
        assert_eq!(info.provider.as_deref(), Some("custom"));
        assert!(info.trusted);
    }

    #[test]
    fn ensure_for_issues_once_then_reuses() {
        let home = crate::test_support::isolated_home();
        let mgr = CertificateManager::new(&home.paths);
        let d = domain("shop.test", false);

        let first = mgr.ensure_for(&d).unwrap();
        let bytes = std::fs::read(&first.cert).unwrap();
        let again = mgr.ensure_for(&d).unwrap();
        assert_eq!(
            bytes,
            std::fs::read(&again.cert).unwrap(),
            "a valid cert must not be regenerated"
        );
    }

    #[test]
    fn toggling_wildcard_reissues_with_the_new_names() {
        let home = crate::test_support::isolated_home();
        let mgr = CertificateManager::new(&home.paths);
        mgr.ensure_for(&domain("shop.test", false)).unwrap();
        assert_eq!(mgr.info("shop.test").unwrap().sans, vec!["shop.test"]);

        mgr.ensure_for(&domain("shop.test", true)).unwrap();
        assert_eq!(
            mgr.info("shop.test").unwrap().sans,
            vec!["shop.test", "*.shop.test"]
        );
    }

    #[test]
    fn info_reports_dates_project_and_paths_but_never_key_material() {
        let home = crate::test_support::isolated_home();
        let mgr = CertificateManager::new(&home.paths);
        mgr.ensure_for(&domain("shop.test", false)).unwrap();

        let info = mgr.info("shop.test").unwrap();
        assert_eq!(info.status, CertStatus::Valid);
        assert!(info.days_left > 390);
        assert_eq!(info.project_id.as_deref(), Some("p1"));
        assert!(info.key_path.ends_with("key.pem"));
        let json = serde_json::to_string(&info).unwrap();
        assert!(
            !json.contains("PRIVATE KEY"),
            "key material must never appear in CertInfo"
        );
    }

    #[test]
    fn revoke_removes_the_certificate() {
        let home = crate::test_support::isolated_home();
        let mgr = CertificateManager::new(&home.paths);
        mgr.ensure_for(&domain("shop.test", false)).unwrap();
        assert_eq!(mgr.list().len(), 1);
        mgr.revoke("shop.test").unwrap();
        assert!(mgr.list().is_empty());
        assert!(mgr.info("shop.test").is_none());
    }

    #[test]
    fn regenerate_replaces_the_certificate() {
        let home = crate::test_support::isolated_home();
        let mgr = CertificateManager::new(&home.paths);
        let d = domain("shop.test", false);
        let p = mgr.ensure_for(&d).unwrap();
        let before = std::fs::read(&p.cert).unwrap();
        mgr.issue(&d).unwrap();
        assert_ne!(before, std::fs::read(&p.cert).unwrap());
    }
}
