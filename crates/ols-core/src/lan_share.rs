//! A temporary web page that hands the OLS certificate authority to a phone (§178, part B).
//!
//! It listens on the computer's private LAN address (never on every interface), on a random
//! port, for at most 10 minutes or 50 requests. The first path segment is a random 128-bit
//! token; anything else answers 404. It serves exactly three things: a small instructions
//! page, the CA certificate (DER) and an iOS configuration profile. This module is given
//! the certificate's PEM text and never opens the CA's private key.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine;

pub const LIFETIME: Duration = Duration::from_secs(10 * 60);
pub const MAX_REQUESTS: usize = 50;

/// A running share. Dropping it does not stop the thread; call [`Share::stop`].
pub struct Share {
    pub url: String,
    pub port: u16,
    pub expires_at: Instant,
    stop: Arc<AtomicBool>,
    served: Arc<AtomicUsize>,
}

impl Share {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    /// Still serving: not stopped, not expired, under the request limit.
    pub fn is_active(&self, max_requests: usize) -> bool {
        !self.stop.load(Ordering::SeqCst)
            && Instant::now() < self.expires_at
            && self.served.load(Ordering::SeqCst) < max_requests
    }

    pub fn expires_in_ms(&self) -> u64 {
        self.expires_at
            .saturating_duration_since(Instant::now())
            .as_millis() as u64
    }
}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    // The OS random source failing is not recoverable; refusing to start is the safe answer.
    getrandom::fill(&mut b).expect("the OS random number generator failed");
    b
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn uuid_v4() -> String {
    let mut b = random_bytes::<16>();
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h = hex(&b).to_uppercase();
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

/// The first certificate of a PEM text as DER bytes.
pub fn pem_to_der(pem: &str) -> Option<Vec<u8>> {
    rustls_pemfile::certs(&mut pem.as_bytes())
        .next()?
        .ok()
        .map(|c| c.as_ref().to_vec())
}

/// An iOS configuration profile that installs `der` as a root certificate. Unsigned, so iOS
/// shows "Not verified"; that is expected.
pub fn mobileconfig(der: &[u8]) -> String {
    let data = base64::engine::general_purpose::STANDARD.encode(der);
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n<key>PayloadContent</key>\n<array>\n<dict>\n<key>PayloadCertificateFileName</key><string>ols-ca.crt</string>\n<key>PayloadContent</key><data>{data}</data>\n<key>PayloadDescription</key><string>Adds the OpenLocalServer local certificate authority.</string>\n<key>PayloadDisplayName</key><string>OpenLocalServer Local CA</string>\n<key>PayloadIdentifier</key><string>local.openlocalserver.ca.cert</string>\n<key>PayloadType</key><string>com.apple.security.root</string>\n<key>PayloadUUID</key><string>{cert_uuid}</string>\n<key>PayloadVersion</key><integer>1</integer>\n</dict>\n</array>\n<key>PayloadDisplayName</key><string>OpenLocalServer Local CA</string>\n<key>PayloadIdentifier</key><string>local.openlocalserver.ca</string>\n<key>PayloadRemovalDisallowed</key><false/>\n<key>PayloadType</key><string>Configuration</string>\n<key>PayloadUUID</key><string>{profile_uuid}</string>\n<key>PayloadVersion</key><integer>1</integer>\n</dict>\n</plist>\n",
        cert_uuid = uuid_v4(),
        profile_uuid = uuid_v4(),
    )
}

const INDEX: &str = "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>OpenLocalServer certificate</title><style>body{font-family:system-ui,sans-serif;max-width:34rem;margin:1.5rem auto;padding:0 1rem;line-height:1.5}a.b{display:block;margin:.6rem 0;padding:.8rem 1rem;background:#2563eb;color:#fff;border-radius:.5rem;text-decoration:none}h2{margin-top:1.6rem}</style></head><body><h1>OpenLocalServer certificate</h1><p>Install this once so your phone trusts the HTTPS sites OLS makes. Only do this on a phone you own, and remove it when you are done testing.</p><a class=\"b\" href=\"ols-ca.crt\">Android: download the certificate</a><a class=\"b\" href=\"ols-ca.mobileconfig\">iPhone / iPad: download the profile</a><h2>Android</h2><p>Open Settings &gt; Security &gt; More security settings &gt; Encryption &amp; credentials &gt; Install a certificate &gt; CA certificate, then pick the downloaded file. (Menus may differ by phone.)</p><h2>iPhone / iPad</h2><p>Open this page in Safari, tap the profile, then Settings &gt; Profile Downloaded &gt; Install (iOS says &ldquo;Not verified&rdquo;; that is expected). Then open Settings &gt; General &gt; About &gt; Certificate Trust Settings and turn on OpenLocalServer Local CA.</p></body></html>";

struct Reply {
    status: &'static str,
    content_type: &'static str,
    extra: &'static str,
    body: Vec<u8>,
}

fn not_found() -> Reply {
    Reply {
        status: "404 Not Found",
        content_type: "text/plain; charset=utf-8",
        extra: "",
        body: b"Not found".to_vec(),
    }
}

fn route(request_head: &str, token: &str, der: &[u8], profile: &str) -> Reply {
    let Some(line) = request_head.lines().next() else {
        return not_found();
    };
    let mut parts = line.split_whitespace();
    let (Some("GET"), Some(target)) = (parts.next(), parts.next()) else {
        return not_found();
    };
    let path = target.split(['?', '#']).next().unwrap_or("");
    let Some(rest) = path.strip_prefix('/').and_then(|p| p.strip_prefix(token)) else {
        return not_found();
    };
    match rest {
        "/" => Reply {
            status: "200 OK",
            content_type: "text/html; charset=utf-8",
            extra: "",
            body: INDEX.as_bytes().to_vec(),
        },
        "/ols-ca.crt" => Reply {
            status: "200 OK",
            content_type: "application/x-x509-ca-cert",
            extra: "Content-Disposition: attachment; filename=\"ols-ca.crt\"\r\n",
            body: der.to_vec(),
        },
        "/ols-ca.mobileconfig" => Reply {
            status: "200 OK",
            content_type: "application/x-apple-aspen-config",
            extra: "",
            body: profile.as_bytes().to_vec(),
        },
        _ => not_found(),
    }
}

fn handle(mut stream: TcpStream, token: &str, der: &[u8], profile: &str) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    while buf.len() < 8192 && !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    let head = String::from_utf8_lossy(&buf).to_string();
    let reply = route(&head, token, der, profile);
    let header = format!(
        "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\n{}Cache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        reply.status,
        reply.content_type,
        reply.body.len(),
        reply.extra
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(&reply.body);
    let _ = stream.flush();
}

/// Starts the share on `ip` (a private LAN address) and a random port.
pub fn start(
    ip: Ipv4Addr,
    ca_pem: &str,
    lifetime: Duration,
    max_requests: usize,
) -> std::io::Result<Share> {
    let der = pem_to_der(ca_pem)
        .ok_or_else(|| std::io::Error::other("the CA certificate could not be read"))?;
    let profile = mobileconfig(&der);
    let token = hex(&random_bytes::<16>());
    let listener = TcpListener::bind(SocketAddr::from((ip, 0)))?;
    listener.set_nonblocking(true)?;
    let port = listener.local_addr()?.port();
    let stop = Arc::new(AtomicBool::new(false));
    let served = Arc::new(AtomicUsize::new(0));
    let expires_at = Instant::now() + lifetime;
    let (stop_t, served_t, token_t) = (stop.clone(), served.clone(), token.clone());
    std::thread::Builder::new()
        .name("ols-ca-share".into())
        .spawn(move || {
            while !stop_t.load(Ordering::SeqCst)
                && Instant::now() < expires_at
                && served_t.load(Ordering::SeqCst) < max_requests
            {
                match listener.accept() {
                    Ok((stream, _)) => {
                        served_t.fetch_add(1, Ordering::SeqCst);
                        let _ = stream.set_nonblocking(false);
                        handle(stream, &token_t, &der, &profile);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(40));
                    }
                    Err(_) => break,
                }
            }
            stop_t.store(true, Ordering::SeqCst);
        })?;
    Ok(Share {
        url: format!("http://{ip}:{port}/{token}/"),
        port,
        expires_at,
        stop,
        served,
    })
}

impl crate::app::Inner {
    /// Starts the share, or returns the one already running.
    pub fn start_ca_share(&self) -> Result<(String, u64), crate::error::CoreError> {
        let mut slot = self.ca_share.lock().unwrap();
        if let Some(s) = slot.as_ref().filter(|s| s.is_active(MAX_REQUESTS)) {
            return Ok((s.url.clone(), s.expires_in_ms()));
        }
        let info = self.certs.ca_info();
        let pem = if info.exists {
            std::fs::read_to_string(&info.cert_path).ok()
        } else {
            None
        };
        let Some(pem) = pem else {
            return Err(crate::error::CoreError::failed_fix(
                "OLS has no certificate to share yet.",
                "The local certificate authority has not been created.",
                "Turn on HTTPS for a site first, then try again.",
            ));
        };
        let ip = crate::lan::lan_ipv4().ok_or_else(|| {
            crate::error::CoreError::failed_fix(
                "This computer is not on a private network (Wi-Fi or Ethernet).",
                "OLS found no private address to share the certificate on.",
                "Connect to your Wi-Fi or Ethernet network, then try again.",
            )
        })?;
        let share = start(ip, &pem, LIFETIME, MAX_REQUESTS).map_err(|e| {
            crate::error::CoreError::failed_fix(
                "OLS could not start the certificate page.",
                e.to_string(),
                "Check that no firewall tool blocks OLS, then try again.",
            )
        })?;
        let out = (share.url.clone(), share.expires_in_ms());
        *slot = Some(share);
        Ok(out)
    }

    pub fn stop_ca_share(&self) {
        if let Some(s) = self.ca_share.lock().unwrap().take() {
            s.stop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_ca_pem() -> String {
        let key = rcgen::KeyPair::generate().unwrap();
        let mut params = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
        params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, "Test CA");
        params.self_signed(&key).unwrap().pem()
    }

    fn get(port: u16, path: &str) -> (String, Vec<u8>) {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        write!(
            s,
            "GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut raw = Vec::new();
        let _ = s.read_to_end(&mut raw);
        let split = raw
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .map(|i| i + 4)
            .unwrap_or(raw.len());
        (
            String::from_utf8_lossy(&raw[..split]).to_string(),
            raw[split..].to_vec(),
        )
    }

    fn token_of(share: &Share) -> String {
        share
            .url
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap()
            .to_string()
    }

    #[test]
    fn routes_serve_three_things_and_everything_else_is_404() {
        let pem = test_ca_pem();
        let share = start(Ipv4Addr::LOCALHOST, &pem, Duration::from_secs(30), 50).unwrap();
        let t = token_of(&share);
        assert!(get(share.port, "/nothing").0.starts_with("HTTP/1.1 404"));
        assert!(get(share.port, &format!("/{}x/", t))
            .0
            .starts_with("HTTP/1.1 404"));
        assert!(get(share.port, &format!("/{t}/other"))
            .0
            .starts_with("HTTP/1.1 404"));
        let (head, body) = get(share.port, &format!("/{t}/"));
        assert!(
            head.starts_with("HTTP/1.1 200")
                && String::from_utf8_lossy(&body).contains("OpenLocalServer certificate")
        );
        let (head, der) = get(share.port, &format!("/{t}/ols-ca.crt"));
        assert!(head.contains("application/x-x509-ca-cert"));
        assert_eq!(der, pem_to_der(&pem).unwrap());
        assert!(der.len() > 100 && der[0] == 0x30, "DER certificate");
        let (head, plist) = get(share.port, &format!("/{t}/ols-ca.mobileconfig"));
        assert!(head.contains("application/x-apple-aspen-config"));
        let plist = String::from_utf8(plist).unwrap();
        assert!(
            plist.contains("com.apple.security.root") && plist.contains("local.openlocalserver.ca")
        );
        share.stop();
    }

    #[test]
    fn no_response_ever_contains_a_private_key() {
        let pem = test_ca_pem();
        let share = start(Ipv4Addr::LOCALHOST, &pem, Duration::from_secs(30), 50).unwrap();
        let t = token_of(&share);
        for path in [
            "/",
            "/ols-ca.crt",
            "/ols-ca.mobileconfig",
            "/ca.key",
            "/../ca.key",
        ] {
            let (head, body) = get(share.port, &format!("/{t}{path}"));
            let all = format!("{head}{}", String::from_utf8_lossy(&body));
            assert!(!all.contains("PRIVATE KEY"), "{path}");
        }
        share.stop();
        // The module is handed certificate text only and never refers to the key file.
        assert!(!include_str!("lan_share.rs").contains(concat!("ca_key", "_path")));
    }

    #[test]
    fn it_stops_after_the_request_limit_and_after_expiry() {
        let pem = test_ca_pem();
        let share = start(Ipv4Addr::LOCALHOST, &pem, Duration::from_secs(30), 3).unwrap();
        let t = token_of(&share);
        for _ in 0..3 {
            get(share.port, &format!("/{t}/"));
        }
        std::thread::sleep(Duration::from_millis(200));
        assert!(!share.is_active(3));
        assert!(TcpStream::connect_timeout(
            &SocketAddr::from((Ipv4Addr::LOCALHOST, share.port)),
            Duration::from_millis(300)
        )
        .is_err());

        let short = start(Ipv4Addr::LOCALHOST, &pem, Duration::from_millis(300), 50).unwrap();
        std::thread::sleep(Duration::from_millis(700));
        assert!(!short.is_active(50));
        assert!(TcpStream::connect_timeout(
            &SocketAddr::from((Ipv4Addr::LOCALHOST, short.port)),
            Duration::from_millis(300)
        )
        .is_err());
    }

    #[test]
    fn tokens_are_random_and_uuids_well_formed() {
        assert_ne!(hex(&random_bytes::<16>()), hex(&random_bytes::<16>()));
        let u = uuid_v4();
        assert_eq!(u.len(), 36);
        assert_eq!(&u[14..15], "4");
    }
}
