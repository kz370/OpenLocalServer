//! `.local` names for shared sites (§178, mDNS, RFC 6762).
//!
//! While a site whose own domain ends in `.local` is shared on the network, OLS answers
//! multicast DNS questions for that name with this computer's LAN address, so iPhones, Macs,
//! Windows and newer Android phones open `https://shop.local/` without any setup. Sites on
//! other domains (`.test`, ...) are opened by IP address and port instead (`lan.rs`). It only
//! answers `A` questions for exactly those names, never forwards anything, and runs only while
//! at least one site is shared. The socket shares UDP 5353 with Windows' own mDNS (address
//! reuse), joins 224.0.0.251 on the LAN interface only, and says goodbye (TTL 0) when it stops.

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const MDNS_GROUP: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);
pub const MDNS_PORT: u16 = 5353;
/// Seconds a phone may cache a normal answer (RFC 6762 suggests 120 for host records).
const TTL: u32 = 120;
/// Legacy one-shot resolvers (source port is not 5353) get a short TTL (RFC 6762 §6.7).
const LEGACY_TTL: u32 = 10;

/// A site whose own domain ends in `.local` (`shop.local`, `api.shop.local`) can be opened by
/// that name on a phone: OLS answers it over mDNS. Other domains (`.test`, ...) cannot, and are
/// opened by IP address and port instead. Every label must be a plain DNS label.
pub fn is_local_name(hostname: &str) -> bool {
    let h = hostname.trim().trim_end_matches('.').to_ascii_lowercase();
    let Some(stem) = h.strip_suffix(".local") else {
        return false;
    };
    !stem.is_empty()
        && h.len() <= 253
        && stem.split('.').all(|l| {
            !l.is_empty()
                && l.len() <= 63
                && !l.starts_with('-')
                && !l.ends_with('-')
                && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
}

/// Reads a (possibly compressed) name at `pos`. Returns it lowercased without the trailing
/// dot, and the position just after it in the original message.
fn read_name(msg: &[u8], mut pos: usize) -> Option<(String, usize)> {
    let mut labels: Vec<String> = Vec::new();
    let mut end: Option<usize> = None;
    for _ in 0..32 {
        let len = *msg.get(pos)? as usize;
        if len == 0 {
            return Some((labels.join("."), end.unwrap_or(pos + 1)));
        }
        if len & 0xC0 == 0xC0 {
            let ptr = ((len & 0x3F) << 8) | *msg.get(pos + 1)? as usize;
            end.get_or_insert(pos + 2);
            if ptr >= pos {
                return None; // pointers only go backwards
            }
            pos = ptr;
            continue;
        }
        if len > 63 {
            return None;
        }
        let label = std::str::from_utf8(msg.get(pos + 1..pos + 1 + len)?).ok()?;
        labels.push(label.to_ascii_lowercase());
        pos += 1 + len;
    }
    None
}

fn push_name(out: &mut Vec<u8>, name: &str) {
    for label in name.split('.') {
        out.push(label.len() as u8);
        out.extend_from_slice(label.as_bytes());
    }
    out.push(0);
}

fn push_a(out: &mut Vec<u8>, name: &str, ip: Ipv4Addr, ttl: u32, cache_flush: bool) {
    push_name(out, name);
    out.extend_from_slice(&1u16.to_be_bytes()); // A
    let class: u16 = if cache_flush { 0x8001 } else { 0x0001 };
    out.extend_from_slice(&class.to_be_bytes());
    out.extend_from_slice(&ttl.to_be_bytes());
    out.extend_from_slice(&4u16.to_be_bytes());
    out.extend_from_slice(&ip.octets());
}

/// Where a reply goes.
#[derive(Debug, PartialEq, Eq)]
pub enum Dest {
    Multicast,
    Unicast,
}

/// The reply to one mDNS message, or `None` when it asks nothing we answer: a response, a
/// malformed packet, or questions for other names or types.
pub fn answer(msg: &[u8], legacy: bool, names: &[String], ip: Ipv4Addr) -> Option<(Vec<u8>, Dest)> {
    if msg.len() < 12 {
        return None;
    }
    let flags = u16::from_be_bytes([msg[2], msg[3]]);
    if flags & 0x8000 != 0 || (flags >> 11) & 0xF != 0 {
        return None; // a response, or not a standard query
    }
    let qdcount = u16::from_be_bytes([msg[4], msg[5]]) as usize;
    let mut pos = 12;
    let mut hits: Vec<String> = Vec::new();
    let mut unicast = legacy;
    for _ in 0..qdcount.min(32) {
        let (name, next) = read_name(msg, pos)?;
        let qtype = u16::from_be_bytes([*msg.get(next)?, *msg.get(next + 1)?]);
        let qclass = u16::from_be_bytes([*msg.get(next + 2)?, *msg.get(next + 3)?]);
        pos = next + 4;
        let ours = names.iter().any(|n| n.eq_ignore_ascii_case(&name));
        if ours && (qtype == 1 || qtype == 255) && (qclass & 0x7FFF) == 1 {
            if qclass & 0x8000 != 0 {
                unicast = true; // "QU": the asker accepts a unicast answer
            }
            if !hits.contains(&name) {
                hits.push(name);
            }
        }
    }
    if hits.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(64);
    if legacy {
        // A plain DNS client: echo its id and question, short TTL, no cache-flush bit.
        out.extend_from_slice(&msg[0..2]);
        out.extend_from_slice(&0x8400u16.to_be_bytes());
        out.extend_from_slice(&(qdcount as u16).to_be_bytes());
        out.extend_from_slice(&(hits.len() as u16).to_be_bytes());
        out.extend_from_slice(&[0, 0, 0, 0]);
        out.extend_from_slice(msg.get(12..pos)?);
        for h in &hits {
            push_a(&mut out, h, ip, LEGACY_TTL, false);
        }
    } else {
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(&0x8400u16.to_be_bytes());
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(&(hits.len() as u16).to_be_bytes());
        out.extend_from_slice(&[0, 0, 0, 0]);
        for h in &hits {
            push_a(&mut out, h, ip, TTL, true);
        }
    }
    Some((
        out,
        if unicast {
            Dest::Unicast
        } else {
            Dest::Multicast
        },
    ))
}

/// An unsolicited response for `names`: an announcement, or with `ttl` 0 a goodbye.
pub fn announcement(names: &[String], ip: Ipv4Addr, ttl: u32) -> Vec<u8> {
    let mut out = vec![0, 0, 0x84, 0x00, 0, 0];
    out.extend_from_slice(&(names.len() as u16).to_be_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]);
    for n in names {
        push_a(&mut out, n, ip, ttl, true);
    }
    out
}

fn open_socket(ip: Ipv4Addr) -> std::io::Result<UdpSocket> {
    use socket2::{Domain, Protocol, Socket, Type};
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    // Windows' own mDNS (and Bonjour) already hold 5353; address reuse lets both receive.
    socket.set_reuse_address(true)?;
    socket.bind(&SocketAddr::from((Ipv4Addr::UNSPECIFIED, MDNS_PORT)).into())?;
    socket.join_multicast_v4(&MDNS_GROUP, &ip)?;
    socket.set_multicast_if_v4(&ip)?;
    socket.set_multicast_ttl_v4(255)?;
    socket.set_read_timeout(Some(Duration::from_millis(250)))?;
    Ok(socket.into())
}

/// The running responder. Dropping it says goodbye and stops the thread.
pub struct Responder {
    ip: Ipv4Addr,
    names: Arc<Mutex<Vec<String>>>,
    socket: UdpSocket,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Responder {
    pub fn start(ip: Ipv4Addr, names: Vec<String>) -> Result<Self, String> {
        let socket =
            open_socket(ip).map_err(|e| format!("could not open mDNS port {MDNS_PORT}: {e}"))?;
        let rx = socket.try_clone().map_err(|e| e.to_string())?;
        let names = Arc::new(Mutex::new(names));
        let stop = Arc::new(AtomicBool::new(false));
        let (names_t, stop_t) = (names.clone(), stop.clone());
        let group = SocketAddr::V4(SocketAddrV4::new(MDNS_GROUP, MDNS_PORT));
        let thread = std::thread::spawn(move || {
            let mut buf = [0u8; 1500];
            while !stop_t.load(Ordering::Relaxed) {
                let Ok((len, from)) = rx.recv_from(&mut buf) else {
                    continue;
                };
                let current = names_t.lock().unwrap().clone();
                let legacy = from.port() != MDNS_PORT;
                if let Some((reply, dest)) = answer(&buf[..len], legacy, &current, ip) {
                    let to = if dest == Dest::Unicast { from } else { group };
                    let _ = rx.send_to(&reply, to);
                }
            }
        });
        let r = Self {
            ip,
            names,
            socket,
            stop,
            thread: Some(thread),
        };
        r.announce(TTL);
        Ok(r)
    }

    pub fn ip(&self) -> Ipv4Addr {
        self.ip
    }

    /// Replaces the answered names; names that went away get a goodbye, new ones an announcement.
    pub fn set_names(&self, names: Vec<String>) {
        let old = std::mem::replace(&mut *self.names.lock().unwrap(), names.clone());
        let gone: Vec<String> = old.iter().filter(|o| !names.contains(o)).cloned().collect();
        let added: Vec<String> = names.iter().filter(|n| !old.contains(n)).cloned().collect();
        self.send(&gone, 0);
        self.send(&added, TTL);
    }

    fn announce(&self, ttl: u32) {
        let names = self.names.lock().unwrap().clone();
        self.send(&names, ttl);
    }

    fn send(&self, names: &[String], ttl: u32) {
        if names.is_empty() {
            return;
        }
        let group = SocketAddr::V4(SocketAddrV4::new(MDNS_GROUP, MDNS_PORT));
        let _ = self
            .socket
            .send_to(&announcement(names, self.ip, ttl), group);
    }
}

impl Drop for Responder {
    fn drop(&mut self) {
        self.announce(0);
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(names: &[&str], qtype: u16, qu: bool) -> Vec<u8> {
        let mut q = vec![0x12, 0x34, 0, 0, 0, names.len() as u8, 0, 0, 0, 0, 0, 0];
        for n in names {
            push_name(&mut q, n);
            q.extend_from_slice(&qtype.to_be_bytes());
            q.extend_from_slice(&(if qu { 0x8001u16 } else { 1 }).to_be_bytes());
        }
        q
    }

    const IP: Ipv4Addr = Ipv4Addr::new(192, 168, 1, 100);

    fn ours() -> Vec<String> {
        vec!["shop.local".to_string()]
    }

    #[test]
    fn only_local_domains_are_answered_by_name() {
        for yes in [
            "shop.local",
            "Acme-Store.local",
            "api.shop.local",
            "shop.local.",
        ] {
            assert!(is_local_name(yes), "{yes}");
        }
        for no in [
            "shop.test",
            "local",
            ".local",
            "shop.localhost",
            "-bad.local",
            "a_b.local",
            "*.shop.local",
        ] {
            assert!(!is_local_name(no), "{no}");
        }
    }

    #[test]
    fn answers_an_a_question_for_our_name_by_multicast() {
        let (reply, dest) = answer(&query(&["shop.local"], 1, false), false, &ours(), IP).unwrap();
        assert_eq!(dest, Dest::Multicast);
        assert_eq!(&reply[0..2], &[0, 0], "mDNS responses carry id 0");
        assert_eq!(u16::from_be_bytes([reply[2], reply[3]]), 0x8400);
        assert_eq!(u16::from_be_bytes([reply[6], reply[7]]), 1, "one answer");
        assert_eq!(&reply[reply.len() - 4..], &IP.octets());
    }

    #[test]
    fn qu_and_legacy_questions_get_a_unicast_answer() {
        let (_, dest) = answer(&query(&["shop.local"], 1, true), false, &ours(), IP).unwrap();
        assert_eq!(dest, Dest::Unicast);
        let q = query(&["SHOP.local"], 1, false);
        let (reply, dest) = answer(&q, true, &ours(), IP).unwrap();
        assert_eq!(dest, Dest::Unicast);
        assert_eq!(&reply[0..2], &[0x12, 0x34], "legacy keeps the id");
        assert_eq!(
            u16::from_be_bytes([reply[4], reply[5]]),
            1,
            "and the question"
        );
        assert_eq!(&reply[reply.len() - 4..], &IP.octets());
    }

    #[test]
    fn other_names_types_and_responses_are_ignored() {
        assert!(answer(&query(&["other.local"], 1, false), false, &ours(), IP).is_none());
        assert!(
            answer(&query(&["shop.local"], 28, false), false, &ours(), IP).is_none(),
            "AAAA"
        );
        let mut response = query(&["shop.local"], 1, false);
        response[2] = 0x84;
        assert!(answer(&response, false, &ours(), IP).is_none());
        assert!(answer(&[0, 1, 2], false, &ours(), IP).is_none());
    }

    #[test]
    fn a_compressed_second_question_is_read() {
        // Question 1: "x.local"; question 2: "shop" + pointer to "local" in question 1.
        let mut q = vec![0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0];
        push_name(&mut q, "x.local");
        q.extend_from_slice(&[0, 1, 0, 1]);
        q.push(4);
        q.extend_from_slice(b"shop");
        q.extend_from_slice(&[0xC0, 14]); // "local" starts at offset 12 + 2
        q.extend_from_slice(&[0, 1, 0, 1]);
        let (reply, _) = answer(&q, false, &ours(), IP).unwrap();
        assert_eq!(u16::from_be_bytes([reply[6], reply[7]]), 1);
    }

    #[test]
    fn a_pointer_loop_is_rejected() {
        let mut q = vec![0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0];
        q.extend_from_slice(&[0xC0, 12, 0, 1, 0, 1]);
        assert!(answer(&q, false, &ours(), IP).is_none());
    }

    #[test]
    fn a_goodbye_has_ttl_zero() {
        let bye = announcement(&ours(), IP, 0);
        let ttl_at = bye.len() - 4 - 2 - 4;
        assert_eq!(&bye[ttl_at..ttl_at + 4], &[0, 0, 0, 0]);
    }
}
