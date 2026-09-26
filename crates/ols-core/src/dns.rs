//! Local DNS resolver for wildcard domains (§46–47, Stage 9). A hosts file can't express
//! `*.shop.test`, so a tiny UDP DNS server answers `A` queries for those suffixes with
//! 127.0.0.1, and a Windows NRPT rule (installed through `ols-helper`) sends only those
//! suffixes to it. Everything else on the machine keeps resolving exactly as before.

use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct DnsServer {
    stop: Arc<AtomicBool>,
    suffixes: Arc<Mutex<Vec<String>>>,
    thread: Option<std::thread::JoinHandle<()>>,
    port: u16,
}

impl DnsServer {
    /// Binds `127.0.0.1:port` (0 = any free port; tests use that) and serves until dropped.
    pub fn start(port: u16, suffixes: Vec<String>) -> Result<Self, String> {
        let socket = UdpSocket::bind(("127.0.0.1", port))
            .map_err(|e| format!("could not bind DNS port {port}: {e}"))?;
        socket
            .set_read_timeout(Some(Duration::from_millis(200)))
            .map_err(|e| e.to_string())?;
        let port = socket.local_addr().map_err(|e| e.to_string())?.port();

        let stop = Arc::new(AtomicBool::new(false));
        let suffixes = Arc::new(Mutex::new(suffixes));
        let (stop_t, suffixes_t) = (stop.clone(), suffixes.clone());
        let thread = std::thread::spawn(move || {
            let mut buf = [0u8; 512];
            while !stop_t.load(Ordering::Relaxed) {
                let Ok((len, from)) = socket.recv_from(&mut buf) else {
                    continue;
                };
                let names = suffixes_t.lock().unwrap().clone();
                if let Some(reply) = answer(&buf[..len], &names) {
                    let _ = socket.send_to(&reply, from);
                }
            }
        });
        Ok(Self {
            stop,
            suffixes,
            thread: Some(thread),
            port,
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn set_suffixes(&self, suffixes: Vec<String>) {
        *self.suffixes.lock().unwrap() = suffixes;
    }

    pub fn local_addr(&self) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], self.port))
    }
}

impl Drop for DnsServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Does `name` fall under one of our wildcard domains? `suffixes` hold the bare domain
/// ("shop.test"): both the domain itself and anything beneath it match.
fn covered(name: &str, suffixes: &[String]) -> bool {
    let name = name.trim_end_matches('.').to_ascii_lowercase();
    suffixes
        .iter()
        .any(|s| name == *s || name.ends_with(&format!(".{s}")))
}

/// Builds the reply to one query, or `None` if it isn't a well-formed standard query.
fn answer(query: &[u8], suffixes: &[String]) -> Option<Vec<u8>> {
    if query.len() < 12 {
        return None;
    }
    let flags = u16::from_be_bytes([query[2], query[3]]);
    let is_query = flags & 0x8000 == 0 && (flags >> 11) & 0xF == 0;
    let qdcount = u16::from_be_bytes([query[4], query[5]]);
    if !is_query || qdcount != 1 {
        return None;
    }

    // Parse the single question: labels, then QTYPE + QCLASS.
    let mut pos = 12;
    let mut labels = Vec::new();
    loop {
        let len = *query.get(pos)? as usize;
        if len == 0 {
            pos += 1;
            break;
        }
        if len & 0xC0 != 0 {
            return None; // compression pointers never appear in a question we generate replies for
        }
        labels.push(
            std::str::from_utf8(query.get(pos + 1..pos + 1 + len)?)
                .ok()?
                .to_string(),
        );
        pos += 1 + len;
    }
    let qtype = u16::from_be_bytes([*query.get(pos)?, *query.get(pos + 1)?]);
    let question_end = pos + 4;
    if query.len() < question_end {
        return None;
    }
    let name = labels.join(".");
    let ours = covered(&name, suffixes);

    let mut reply = Vec::with_capacity(question_end + 16);
    reply.extend_from_slice(&query[0..2]); // id
                                           // QR=1, RD copied, RA=1; RCODE: 0 = ok, 3 = NXDOMAIN for names that aren't ours.
    let rcode: u16 = if ours { 0 } else { 3 };
    let out_flags: u16 = 0x8000 | (flags & 0x0100) | 0x0080 | rcode;
    reply.extend_from_slice(&out_flags.to_be_bytes());
    reply.extend_from_slice(&1u16.to_be_bytes()); // qdcount
    let answer_a = ours && qtype == 1;
    reply.extend_from_slice(&(answer_a as u16).to_be_bytes()); // ancount
    reply.extend_from_slice(&[0, 0, 0, 0]); // nscount, arcount
    reply.extend_from_slice(&query[12..question_end]);
    if answer_a {
        reply.extend_from_slice(&[0xC0, 0x0C]); // pointer to the question name
        reply.extend_from_slice(&1u16.to_be_bytes()); // TYPE A
        reply.extend_from_slice(&1u16.to_be_bytes()); // CLASS IN
        reply.extend_from_slice(&60u32.to_be_bytes()); // TTL
        reply.extend_from_slice(&4u16.to_be_bytes());
        reply.extend_from_slice(&Ipv4Addr::LOCALHOST.octets());
    }
    Some(reply)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(name: &str, qtype: u16) -> Vec<u8> {
        let mut q = vec![0x12, 0x34, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0];
        for label in name.split('.') {
            q.push(label.len() as u8);
            q.extend_from_slice(label.as_bytes());
        }
        q.push(0);
        q.extend_from_slice(&qtype.to_be_bytes());
        q.extend_from_slice(&1u16.to_be_bytes());
        q
    }

    fn ask(server: &DnsServer, q: &[u8]) -> Vec<u8> {
        let client = UdpSocket::bind("127.0.0.1:0").unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        client.send_to(q, server.local_addr()).unwrap();
        let mut buf = [0u8; 512];
        let (n, _) = client.recv_from(&mut buf).unwrap();
        buf[..n].to_vec()
    }

    #[test]
    fn answers_wildcard_names_with_loopback() {
        let server = DnsServer::start(0, vec!["shop.test".into()]).unwrap();
        let reply = ask(&server, &query("tenant1.shop.test", 1));
        assert_eq!(&reply[0..2], &[0x12, 0x34], "id must be echoed");
        assert_eq!(reply[3] & 0x0F, 0, "NOERROR");
        assert_eq!(u16::from_be_bytes([reply[6], reply[7]]), 1, "one answer");
        assert_eq!(&reply[reply.len() - 4..], &[127, 0, 0, 1]);
    }

    #[test]
    fn the_bare_domain_and_deep_subdomains_match() {
        let server = DnsServer::start(0, vec!["shop.test".into()]).unwrap();
        for name in ["shop.test", "a.b.c.shop.test"] {
            let reply = ask(&server, &query(name, 1));
            assert_eq!(&reply[reply.len() - 4..], &[127, 0, 0, 1], "{name}");
        }
    }

    #[test]
    fn foreign_names_get_nxdomain_not_loopback() {
        let server = DnsServer::start(0, vec!["shop.test".into()]).unwrap();
        let reply = ask(&server, &query("example.com", 1));
        assert_eq!(reply[3] & 0x0F, 3, "NXDOMAIN");
        assert_eq!(u16::from_be_bytes([reply[6], reply[7]]), 0);
        // A lookalike must not match: "notshop.test" is not under shop.test.
        let reply = ask(&server, &query("notshop.test", 1));
        assert_eq!(reply[3] & 0x0F, 3);
    }

    #[test]
    fn aaaa_for_our_names_is_an_empty_noerror() {
        let server = DnsServer::start(0, vec!["shop.test".into()]).unwrap();
        let reply = ask(&server, &query("x.shop.test", 28));
        assert_eq!(reply[3] & 0x0F, 0);
        assert_eq!(u16::from_be_bytes([reply[6], reply[7]]), 0);
    }

    #[test]
    fn suffix_list_can_change_while_running() {
        let server = DnsServer::start(0, vec![]).unwrap();
        assert_eq!(ask(&server, &query("a.shop.test", 1))[3] & 0x0F, 3);
        server.set_suffixes(vec!["shop.test".into()]);
        assert_eq!(ask(&server, &query("a.shop.test", 1))[3] & 0x0F, 0);
    }
}
