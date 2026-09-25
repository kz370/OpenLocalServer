//! Local Certificate Authority (§49–52 — Stage 6). Generates a root CA once, then issues
//! a leaf certificate per domain signed by it. CA trust goes into the **CurrentUser**
//! Root store (`certutil -user -addstore Root ...`), which — unlike the machine-wide
//! store — does not require elevation, so this whole module needs no privileged helper.

use std::path::{Path, PathBuf};

use rcgen::{
    date_time_ymd, BasicConstraints, CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer,
    KeyPair, KeyUsagePurpose, SanType,
};

use crate::paths::AppPaths;

pub struct LocalCa {
    dir: PathBuf,
}

pub struct IssuedCert {
    pub cert_pem: String,
    pub key_pem: String,
}

/// Common name of the root CA — also how the Windows store is searched for it.
pub const CA_COMMON_NAME: &str = "OpenLocalServer Local CA";

/// Leaf certificates last 397 days — the longest Chrome/Safari accept without complaint.
pub const LEAF_VALIDITY_DAYS: u64 = 397;

pub fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Unix seconds → (year, month, day) in UTC (Howard Hinnant's civil-from-days).
fn ymd_from_unix(secs: u64) -> (i32, u8, u8) {
    let days = (secs / 86_400) as i64 + 719_468;
    let era = days.div_euclid(146_097);
    let doe = days.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
    let y = (yoe + era * 400 + i64::from(m <= 2)) as i32;
    (y, m, d)
}

impl LocalCa {
    pub fn new(paths: &AppPaths) -> Self {
        Self { dir: paths.certs_dir() }
    }

    fn ca_cert_path(&self) -> PathBuf {
        self.dir.join("ca.pem")
    }
    fn ca_key_path(&self) -> PathBuf {
        self.dir.join("ca-key.pem")
    }

    pub fn ca_cert_pem_path(&self) -> PathBuf {
        self.ca_cert_path()
    }

    pub fn exists(&self) -> bool {
        self.ca_cert_path().is_file() && self.ca_key_path().is_file()
    }

    /// The CA's own certificate parameters — deterministic, so re-deriving them (rather
    /// than round-tripping through the stored PEM) is enough to reconstruct an `Issuer`
    /// for signing leaf certs after a restart.
    fn ca_params() -> CertificateParams {
        let mut params = CertificateParams::default();
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, CA_COMMON_NAME);
        dn.push(DnType::OrganizationName, "OpenLocalServer");
        params.distinguished_name = dn;
        // Fixed dates keep `ca_params()` deterministic, which is what lets an `Issuer` be
        // rebuilt after a restart without round-tripping the stored PEM.
        params.not_before = date_time_ymd(2024, 1, 1);
        params.not_after = date_time_ymd(2044, 1, 1);
        params
    }

    /// Generates the root CA if it doesn't already exist. Idempotent — safe to call on
    /// every startup. The private key is written with owner-only permissions (§142).
    pub fn ensure_created(&self) -> Result<(), String> {
        if self.exists() {
            return Ok(());
        }
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;

        let key_pair = KeyPair::generate().map_err(|e| e.to_string())?;
        let cert = Self::ca_params().self_signed(&key_pair).map_err(|e| e.to_string())?;

        write_restricted(&self.ca_cert_path(), cert.pem().as_bytes())?;
        write_restricted(&self.ca_key_path(), key_pair.serialize_pem().as_bytes())?;
        Ok(())
    }

    /// Issues a leaf certificate for `domain`, signed by the local CA (§51). Returns PEM
    /// for both the certificate and its private key — caller decides where to store them.
    pub fn issue(&self, domain: &str) -> Result<IssuedCert, String> {
        self.issue_for(&[domain.to_string()])
    }

    /// Issues one leaf covering every name in `names` (e.g. `shop.test` + `*.shop.test`,
    /// §50). The first name becomes the common name.
    pub fn issue_for(&self, names: &[String]) -> Result<IssuedCert, String> {
        let first = names.first().ok_or("no names to issue a certificate for")?;
        self.ensure_created()?;

        let ca_key_pem = std::fs::read_to_string(self.ca_key_path()).map_err(|e| e.to_string())?;
        let ca_key_pair = KeyPair::from_pem(&ca_key_pem).map_err(|e| e.to_string())?;
        let ca_params = Self::ca_params();
        let issuer = Issuer::from_params(&ca_params, ca_key_pair);

        let mut leaf_params = CertificateParams::new(Vec::<String>::new()).map_err(|e| e.to_string())?;
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, first.as_str());
        leaf_params.distinguished_name = dn;
        leaf_params.subject_alt_names = names
            .iter()
            .map(|n| n.as_str().try_into().map(SanType::DnsName).map_err(|e| format!("{e:?}")))
            .collect::<Result<_, _>>()?;
        leaf_params.key_usages = vec![KeyUsagePurpose::DigitalSignature, KeyUsagePurpose::KeyEncipherment];
        leaf_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];

        let now = unix_now();
        let (y, m, d) = ymd_from_unix(now.saturating_sub(86_400));
        leaf_params.not_before = date_time_ymd(y, m, d);
        let (y, m, d) = ymd_from_unix(now + LEAF_VALIDITY_DAYS * 86_400);
        leaf_params.not_after = date_time_ymd(y, m, d);

        let leaf_key = KeyPair::generate().map_err(|e| e.to_string())?;
        let leaf_cert = leaf_params.signed_by(&leaf_key, &issuer).map_err(|e| e.to_string())?;

        Ok(IssuedCert { cert_pem: leaf_cert.pem(), key_pem: leaf_key.serialize_pem() })
    }

    /// Trusts the CA in the CurrentUser Root store. No elevation needed — CurrentUser
    /// scope only affects this Windows account, unlike LocalMachine\Root.
    #[cfg(windows)]
    pub fn trust_current_user(&self) -> Result<(), String> {
        self.ensure_created()?;
        let mut cmd = std::process::Command::new("certutil");
        cmd.args(["-user", "-addstore", "Root", &self.ca_cert_path().display().to_string()]);
        crate::exec::hide_window(&mut cmd);
        let output = cmd.output().map_err(|e| e.to_string())?;
        if output.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).to_string())
        }
    }
}

impl LocalCa {
    /// Is the CA currently in the CurrentUser Root store? (§51 trust check)
    #[cfg(windows)]
    pub fn is_trusted(&self) -> bool {
        let mut cmd = std::process::Command::new("certutil");
        cmd.args(["-user", "-store", "Root", CA_COMMON_NAME]);
        crate::exec::hide_window(&mut cmd);
        cmd.output().map(|o| o.status.success()).unwrap_or(false)
    }

    #[cfg(windows)]
    pub fn untrust_current_user(&self) -> Result<(), String> {
        let mut cmd = std::process::Command::new("certutil");
        cmd.args(["-user", "-delstore", "Root", CA_COMMON_NAME]);
        crate::exec::hide_window(&mut cmd);
        let output = cmd.output().map_err(|e| e.to_string())?;
        if output.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&output.stdout).to_string())
        }
    }

    #[cfg(not(windows))]
    pub fn is_trusted(&self) -> bool {
        false
    }
    #[cfg(not(windows))]
    pub fn trust_current_user(&self) -> Result<(), String> {
        Err("trusting the CA is only implemented on Windows so far".into())
    }
    #[cfg(not(windows))]
    pub fn untrust_current_user(&self) -> Result<(), String> {
        Err("trusting the CA is only implemented on Windows so far".into())
    }
}

/// Writes a private key with owner-only permissions (§142).
pub fn write_key_restricted(path: &Path, data: &[u8]) -> Result<(), String> {
    write_restricted(path, data)
}

#[cfg(windows)]
fn write_restricted(path: &Path, data: &[u8]) -> Result<(), String> {
    std::fs::write(path, data).map_err(|e| e.to_string())?;
    // icacls: strip inherited permissions, grant only the current user. Best-effort —
    // the CA private key not existing at all is worse than it existing with default
    // (still user-profile-scoped) ACLs, so a failure here doesn't abort cert generation.
    let mut icacls = std::process::Command::new("icacls");
    icacls.args([&path.display().to_string(), "/inheritance:r", "/grant:r", &format!("{}:F", whoami())]);
    crate::exec::hide_window(&mut icacls);
    let _ = icacls.output();
    Ok(())
}

#[cfg(windows)]
fn whoami() -> String {
    std::env::var("USERNAME").unwrap_or_else(|_| "%USERNAME%".to_string())
}

#[cfg(not(windows))]
fn write_restricted(path: &Path, data: &[u8]) -> Result<(), String> {
    std::fs::write(path, data).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Arc;

    fn test_ca() -> (LocalCa, crate::test_support::IsolatedHome) {
        let home = crate::test_support::isolated_home();
        let ca = LocalCa::new(&home.paths);
        (ca, home)
    }

    #[test]
    fn ensure_created_is_idempotent() {
        let (ca, _home) = test_ca();
        ca.ensure_created().unwrap();
        let cert_before = std::fs::read(ca.ca_cert_path()).unwrap();
        ca.ensure_created().unwrap();
        let cert_after = std::fs::read(ca.ca_cert_path()).unwrap();
        assert_eq!(cert_before, cert_after, "calling ensure_created twice must not regenerate the CA");
    }

    #[test]
    fn ymd_conversion_matches_known_dates() {
        assert_eq!(ymd_from_unix(0), (1970, 1, 1));
        assert_eq!(ymd_from_unix(951_782_400), (2000, 2, 29));
        assert_eq!(ymd_from_unix(1_798_761_600), (2027, 1, 1));
    }

    #[test]
    fn issue_for_supports_wildcard_names() {
        let (ca, _home) = test_ca();
        let issued = ca.issue_for(&["shop.test".to_string(), "*.shop.test".to_string()]).unwrap();
        assert!(issued.cert_pem.contains("BEGIN CERTIFICATE"));
    }

    #[test]
    fn issue_produces_a_cert_and_key_for_the_requested_domain() {
        let (ca, _home) = test_ca();
        let issued = ca.issue("shop.test").unwrap();
        assert!(issued.cert_pem.contains("BEGIN CERTIFICATE"));
        assert!(issued.key_pem.contains("PRIVATE KEY"));
    }

    /// The real proof: start a TLS server with the issued leaf cert, connect a client
    /// that trusts only our CA (nothing else — not the system store), and confirm the
    /// handshake succeeds. This is what "trusted local HTTPS" (§49) actually means.
    #[test]
    fn issued_certificate_is_trusted_by_a_client_that_trusts_only_this_ca() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let (ca, _home) = test_ca();
        let issued = ca.issue("localhost").unwrap();

        let cert_der = rustls_pemfile::certs(&mut issued.cert_pem.as_bytes()).collect::<Result<Vec<_>, _>>().unwrap();
        let key_der =
            rustls_pemfile::private_key(&mut issued.key_pem.as_bytes()).unwrap().expect("leaf key parses");

        let server_config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(cert_der, key_der)
            .unwrap();
        let server_config = Arc::new(server_config);

        let ca_pem = std::fs::read_to_string(ca.ca_cert_pem_path()).unwrap();
        let ca_der = rustls_pemfile::certs(&mut ca_pem.as_bytes()).next().unwrap().unwrap();
        let mut root_store = rustls::RootCertStore::empty();
        root_store.add(ca_der).unwrap();
        let client_config =
            rustls::ClientConfig::builder().with_root_certificates(root_store).with_no_client_auth();

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();

        let server_thread = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut conn = rustls::ServerConnection::new(server_config).unwrap();
            let mut tls_stream = rustls::Stream::new(&mut conn, &mut stream);
            let mut buf = [0u8; 64];
            let n = tls_stream.read(&mut buf).unwrap();
            tls_stream.write_all(&buf[..n]).unwrap();
        });

        let server_name: rustls::pki_types::ServerName<'_> = "localhost".try_into().unwrap();
        let mut conn = rustls::ClientConnection::new(Arc::new(client_config), server_name).unwrap();
        let mut socket = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        let mut tls_stream = rustls::Stream::new(&mut conn, &mut socket);
        tls_stream.write_all(b"hello").unwrap();
        let mut buf = [0u8; 64];
        let n = tls_stream.read(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"hello", "TLS handshake + round trip through our issued cert must succeed");

        server_thread.join().unwrap();
    }
}
