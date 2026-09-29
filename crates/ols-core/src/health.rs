//! HTTPS health chain (§53, Stage 6): DNS → TCP → TLS → Certificate → Trust → HTTP.
//! Every step reports on its own so the UI can say *which* link is broken instead of
//! "site not working". Later steps still run after an earlier failure when they can —
//! a missing hosts entry shouldn't hide the fact that TLS itself is fine.

use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::certs::{CertInfo, CertStatus};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStep {
    pub name: String,
    pub ok: bool,
    pub skipped: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthReport {
    pub hostname: String,
    pub ok: bool,
    pub steps: Vec<HealthStep>,
}

fn step(name: &str, ok: bool, detail: impl Into<String>) -> HealthStep {
    HealthStep {
        name: name.to_string(),
        ok,
        skipped: false,
        detail: detail.into(),
    }
}

fn skipped(name: &str, why: &str) -> HealthStep {
    HealthStep {
        name: name.to_string(),
        ok: true,
        skipped: true,
        detail: why.to_string(),
    }
}

pub struct HealthTarget<'a> {
    pub hostname: &'a str,
    pub https: bool,
    pub http_port: u16,
    pub https_port: u16,
    pub ca_pem: &'a Path,
    pub ca_trusted: bool,
    pub cert: Option<&'a CertInfo>,
}

pub fn check_site(target: &HealthTarget) -> HealthReport {
    let port = if target.https {
        target.https_port
    } else {
        target.http_port
    };
    let mut steps = Vec::new();

    steps.push(check_dns(target.hostname, port));

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let tcp = TcpStream::connect_timeout(&addr, Duration::from_secs(3));
    steps.push(match &tcp {
        Ok(_) => step("TCP", true, format!("connected to {addr}")),
        Err(e) => step(
            "TCP",
            false,
            format!("nothing is listening on {addr}: {e}. Start the web server."),
        ),
    });
    drop(tcp);
    let tcp_ok = steps.last().is_some_and(|s| s.ok);

    if target.https {
        let (tls_step, http_step) = if tcp_ok {
            match tls_get(target.hostname, addr, target.ca_pem) {
                Ok(status_line) => (
                    step("TLS", true, "handshake succeeded against the OLS CA"),
                    Some(status_line),
                ),
                Err(e) => (step("TLS", false, e), None),
            }
        } else {
            (skipped("TLS", "no TCP connection"), None)
        };
        steps.push(tls_step);
        steps.push(check_cert(target.cert));
        steps.push(if target.ca_trusted {
            step(
                "Trust",
                true,
                "the CA is trusted by Windows (Edge and Chrome will accept it)",
            )
        } else {
            step(
                "Trust",
                false,
                "the CA is not trusted yet. Trust it from the Certificates page.",
            )
        });
        steps.push(match http_step {
            Some(line) => http_result(&line),
            None => skipped("HTTP", "TLS did not complete"),
        });
    } else {
        steps.push(skipped("TLS", "site is HTTP only"));
        steps.push(skipped("Certificate", "site is HTTP only"));
        steps.push(skipped("Trust", "site is HTTP only"));
        steps.push(if tcp_ok {
            match plain_get(target.hostname, addr) {
                Ok(line) => http_result(&line),
                Err(e) => step("HTTP", false, e),
            }
        } else {
            skipped("HTTP", "no TCP connection")
        });
    }

    let ok = steps.iter().all(|s| s.ok);
    HealthReport {
        hostname: target.hostname.to_string(),
        ok,
        steps,
    }
}

fn check_dns(hostname: &str, port: u16) -> HealthStep {
    match (hostname, port).to_socket_addrs() {
        Ok(addrs) => {
            let ips: Vec<IpAddr> = addrs.map(|a| a.ip()).collect();
            if ips.iter().any(|ip| ip.is_loopback()) {
                step(
                    "DNS",
                    true,
                    format!(
                        "{hostname} resolves to {}",
                        ips.iter()
                            .map(|i| i.to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                )
            } else {
                step(
                    "DNS",
                    false,
                    format!("{hostname} resolves to {ips:?}, not 127.0.0.1"),
                )
            }
        }
        Err(_) => step(
            "DNS",
            false,
            format!("{hostname} does not resolve. Sync the hosts file from the Domains page."),
        ),
    }
}

fn check_cert(cert: Option<&CertInfo>) -> HealthStep {
    match cert {
        None => step(
            "Certificate",
            false,
            "no certificate has been generated for this site",
        ),
        Some(c) => match c.status {
            CertStatus::Valid => step(
                "Certificate",
                true,
                format!("valid for {} more days", c.days_left),
            ),
            CertStatus::Expiring => step(
                "Certificate",
                true,
                format!(
                    "expires in {} days. It renews on the next apply.",
                    c.days_left
                ),
            ),
            CertStatus::Expired => step(
                "Certificate",
                false,
                "the certificate has expired. Regenerate it.",
            ),
        },
    }
}

fn http_result(status_line: &str) -> HealthStep {
    // "HTTP/1.1 200 OK" → 200
    let code: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    // A 4xx still proves the whole chain works; a 5xx means the app behind it is broken.
    if (100..500).contains(&code) {
        step("HTTP", true, status_line.trim().to_string())
    } else {
        step(
            "HTTP",
            false,
            format!("the site answered {}", status_line.trim()),
        )
    }
}

fn request(hostname: &str) -> String {
    format!("GET / HTTP/1.1\r\nHost: {hostname}\r\nConnection: close\r\nUser-Agent: OLS-health\r\nAccept: */*\r\n\r\n")
}

fn first_line(mut reader: impl Read) -> Result<String, String> {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    // Read byte-by-byte up to the first newline — TLS close_notify handling differs by
    // server, and we never need more than the status line.
    while buf.len() < 512 {
        match reader.read(&mut byte) {
            Ok(0) => break,
            Ok(_) => {
                if byte[0] == b'\n' {
                    break;
                }
                buf.push(byte[0]);
            }
            Err(e) if !buf.is_empty() && e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(format!("could not read a response: {e}")),
        }
    }
    if buf.is_empty() {
        return Err("the server closed the connection without answering".into());
    }
    Ok(String::from_utf8_lossy(&buf).trim().to_string())
}

fn plain_get(hostname: &str, addr: SocketAddr) -> Result<String, String> {
    let mut stream =
        TcpStream::connect_timeout(&addr, Duration::from_secs(3)).map_err(|e| e.to_string())?;
    stream.set_read_timeout(Some(Duration::from_secs(15))).ok();
    stream
        .write_all(request(hostname).as_bytes())
        .map_err(|e| e.to_string())?;
    first_line(stream)
}

fn tls_get(hostname: &str, addr: SocketAddr, ca_pem: &Path) -> Result<String, String> {
    let pem =
        std::fs::read(ca_pem).map_err(|e| format!("could not read the CA certificate: {e}"))?;
    let mut roots = rustls::RootCertStore::empty();
    for cert in rustls_pemfile::certs(&mut pem.as_slice()) {
        roots
            .add(cert.map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    }
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|e| e.to_string())?
    .with_root_certificates(roots)
    .with_no_client_auth();

    let server_name: rustls::pki_types::ServerName<'static> = hostname
        .to_string()
        .try_into()
        .map_err(|_| format!("{hostname} is not a valid TLS name"))?;
    let conn =
        rustls::ClientConnection::new(Arc::new(config), server_name).map_err(|e| e.to_string())?;
    let socket =
        TcpStream::connect_timeout(&addr, Duration::from_secs(3)).map_err(|e| e.to_string())?;
    socket.set_read_timeout(Some(Duration::from_secs(15))).ok();
    socket.set_write_timeout(Some(Duration::from_secs(5))).ok();
    let mut tls = rustls::StreamOwned::new(conn, socket);

    tls.write_all(request(hostname).as_bytes())
        .map_err(|e| format!("TLS handshake failed: {e}. The certificate does not chain to the OLS CA, or does not cover {hostname}."))?;
    first_line(tls).map_err(|e| format!("TLS handshake failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ca::LocalCa;
    use std::net::TcpListener;

    /// One-shot HTTPS server on an ephemeral port using a cert from `ca` for `host`.
    fn https_server(ca: &LocalCa, host: &str) -> u16 {
        let issued = ca.issue(host).unwrap();
        let certs: Vec<_> = rustls_pemfile::certs(&mut issued.cert_pem.as_bytes())
            .collect::<Result<_, _>>()
            .unwrap();
        let key = rustls_pemfile::private_key(&mut issued.key_pem.as_bytes())
            .unwrap()
            .unwrap();
        let config = Arc::new(
            rustls::ServerConfig::builder_with_provider(Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .unwrap(),
        );
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten().take(4) {
                let mut conn = rustls::ServerConnection::new(config.clone()).unwrap();
                let mut stream = stream;
                let mut tls = rustls::Stream::new(&mut conn, &mut stream);
                let mut buf = [0u8; 1024];
                if tls.read(&mut buf).is_ok() {
                    let _ = tls.write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                    );
                    tls.conn.send_close_notify();
                    let _ = tls.flush();
                }
            }
        });
        port
    }

    #[test]
    fn full_chain_passes_for_a_correctly_issued_site() {
        let home = crate::test_support::isolated_home();
        let ca = LocalCa::new(&home.paths);
        let port = https_server(&ca, "localhost");

        let report = check_site(&HealthTarget {
            hostname: "localhost",
            https: true,
            http_port: 1,
            https_port: port,
            ca_pem: &ca.ca_cert_pem_path(),
            ca_trusted: true,
            cert: Some(&CertInfo {
                hostname: "localhost".into(),
                sans: vec!["localhost".into()],
                issuer: "x".into(),
                issued_at: 0,
                expires_at: 0,
                days_left: 300,
                status: CertStatus::Valid,
                trusted: true,
                project_id: None,
                cert_path: String::new(),
                key_path: String::new(),
            }),
        });
        let failed: Vec<_> = report.steps.iter().filter(|s| !s.ok).collect();
        assert!(report.ok, "steps failed: {failed:?}");
        assert_eq!(
            report
                .steps
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>(),
            ["DNS", "TCP", "TLS", "Certificate", "Trust", "HTTP"]
        );
    }

    #[test]
    fn a_closed_port_fails_tcp_and_skips_the_rest() {
        let home = crate::test_support::isolated_home();
        let ca = LocalCa::new(&home.paths);
        ca.ensure_created().unwrap();
        // Grab a free port then close it.
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let report = check_site(&HealthTarget {
            hostname: "localhost",
            https: true,
            http_port: 1,
            https_port: port,
            ca_pem: &ca.ca_cert_pem_path(),
            ca_trusted: false,
            cert: None,
        });
        assert!(!report.ok);
        assert!(!report.steps[1].ok, "TCP must fail");
        assert!(
            report.steps[2].skipped,
            "TLS is skipped without a TCP connection"
        );
        assert!(report.steps[5].skipped);
    }

    #[test]
    fn a_certificate_from_a_different_ca_fails_tls() {
        let home = crate::test_support::isolated_home();
        let ca = LocalCa::new(&home.paths);
        let port = https_server(&ca, "localhost");
        // A second, unrelated CA that the client is told to trust instead.
        let other_home = tempfile::tempdir().unwrap();
        let other_paths = crate::paths::AppPaths::resolve_at(other_home.path());
        let other = LocalCa::new(&other_paths);
        other.ensure_created().unwrap();

        let report = check_site(&HealthTarget {
            hostname: "localhost",
            https: true,
            http_port: 1,
            https_port: port,
            ca_pem: &other.ca_cert_pem_path(),
            ca_trusted: true,
            cert: None,
        });
        let tls = &report.steps[2];
        assert!(!tls.ok, "wrong CA must fail the TLS step: {tls:?}");
        assert!(!report.ok);
    }

    #[test]
    fn http_only_site_skips_tls_steps() {
        let home = crate::test_support::isolated_home();
        let ca = LocalCa::new(&home.paths);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            // The TCP step connects and hangs up before the HTTP step connects again.
            for mut s in listener.incoming().flatten().take(3) {
                let mut buf = [0u8; 512];
                if s.read(&mut buf).is_ok_and(|n| n > 0) {
                    let _ = s.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
                }
            }
        });
        let report = check_site(&HealthTarget {
            hostname: "localhost",
            https: false,
            http_port: port,
            https_port: 1,
            ca_pem: &ca.ca_cert_pem_path(),
            ca_trusted: false,
            cert: None,
        });
        assert!(
            report.ok,
            "a 404 still proves the chain: {:?}",
            report.steps
        );
        assert!(report.steps[2].skipped);
    }
}
