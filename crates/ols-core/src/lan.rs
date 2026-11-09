//! Open a site on your phone (§178, part A): a LAN port per site.
//!
//! A phone cannot use OLS's local DNS or this computer's hosts file, so it opens a shared site
//! by this computer's private LAN address and a port of the site's own:
//! `https://192.168.1.5:8700/`. Nothing has to be looked up, so it works on a Wi-Fi without
//! internet. The port alone picks the site: the web server gets one more listener for it
//! (`web::LanListen`). Over HTTPS the site's certificate also covers the LAN IP; the user can
//! pick plain HTTP per site instead (no certificate on the phone). Sites without `lan` render
//! byte for byte as before. OLS does not change the Windows firewall.
//!
//! A shared site whose own domain ends in `.local` (`shop.local`) can also be opened by that
//! name: `mdns::Responder` answers it while the site is shared (`Domain.lan_local`). Other
//! domains (`.test`, ...) cannot be looked up by a phone and use the IP link only. Not every
//! phone resolves `.local` (older Android does not), so the IP link is always shown too.

use std::net::{Ipv4Addr, UdpSocket};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::app::Inner;
use crate::error::CoreError;

/// Where the QR for a site and its warnings come from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanInfo {
    pub on: bool,
    pub ip: Option<String>,
    pub url: Option<String>,
    pub qr_svg: Option<String>,
    pub warnings: Vec<String>,
    /// The site's LAN port while shared.
    #[serde(default)]
    pub port: Option<u16>,
    /// The link is HTTPS (the phone needs OLS's certificate) rather than plain HTTP.
    #[serde(default)]
    pub https: bool,
    /// The site has HTTPS, so the phone link can use it. Without it the link is HTTP only.
    #[serde(default)]
    pub can_https: bool,
    /// The `.local` name and its link, answered over mDNS while shared.
    #[serde(default)]
    pub local_name: Option<String>,
    #[serde(default)]
    pub local_url: Option<String>,
    #[serde(default)]
    pub local_qr_svg: Option<String>,
    /// The mDNS responder is answering.
    #[serde(default)]
    pub mdns_running: bool,
}

/// The ports shared sites get, one each. Clear of the usual dev-server ports (3000, 5173,
/// 8000, 8080, 8443, 8888) and of OLS's own (7420).
pub const LAN_PORTS: std::ops::RangeInclusive<u16> = 8700..=8999;

/// `10/8`, `172.16/12` and `192.168/16` only.
pub fn is_private(ip: Ipv4Addr) -> bool {
    ip.is_private()
}

/// This computer's private IPv4 address on the local network.
///
/// First the address of the interface the OS would use to go out: a UDP `connect` sends no
/// packet, it only makes the OS pick the interface. A Wi-Fi with no internet and no default
/// route has no such interface, so Windows is then asked for its own private addresses.
pub fn lan_ipv4() -> Option<Ipv4Addr> {
    let routed = (|| {
        let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
        socket.connect("192.0.2.1:9").ok()?;
        match socket.local_addr().ok()?.ip() {
            std::net::IpAddr::V4(ip) if is_private(ip) => Some(ip),
            _ => None,
        }
    })();
    routed.or_else(|| first_private(&ps_lines("(Get-NetIPAddress -AddressFamily IPv4 -AddressState Preferred -ErrorAction SilentlyContinue | Where-Object { $_.PrefixOrigin -ne 'WellKnown' }).IPAddress")))
}

/// The first private address in a list of lines (PowerShell output), if any.
fn first_private(lines: &[String]) -> Option<Ipv4Addr> {
    lines
        .iter()
        .filter_map(|l| l.trim().parse::<Ipv4Addr>().ok())
        .find(|ip| is_private(*ip))
}

/// Picks a free LAN port: in `LAN_PORTS`, not `taken` by another shared site, and `free`.
pub fn pick_port(taken: &[u16], free: &dyn Fn(u16) -> bool) -> Option<u16> {
    LAN_PORTS
        .into_iter()
        .find(|p| !taken.contains(p) && free(*p))
}

fn port_is_free(port: u16) -> bool {
    std::net::TcpListener::bind(("0.0.0.0", port)).is_ok()
}

/// An SVG QR code for `text`. Empty when the text is too long to encode.
pub fn qr_svg(text: &str) -> String {
    use qrcode::render::svg;
    qrcode::QrCode::new(text.as_bytes())
        .map(|code| {
            code.render::<svg::Color>()
                .min_dimensions(220, 220)
                .quiet_zone(true)
                .build()
        })
        .unwrap_or_default()
}

/// The phone link for a shared site.
pub fn lan_url(ip: &str, port: u16, https: bool) -> String {
    format!("{}://{ip}:{port}/", if https { "https" } else { "http" })
}

fn ps_lines(command: &str) -> Vec<String> {
    let args = [
        "-NoProfile".to_string(),
        "-NonInteractive".to_string(),
        "-Command".to_string(),
        command.to_string(),
    ];
    let out = crate::exec::run_capture(
        &crate::exec::powershell_path(),
        &args,
        None,
        &[],
        Duration::from_secs(5),
    );
    if out.success() {
        out.stdout.lines().map(str::to_string).collect()
    } else {
        Vec::new()
    }
}

/// `true` when Windows calls the active network "Public" (inbound connections are blocked).
/// Asking Windows takes a second or two, so the answer is kept for a minute.
fn network_is_public() -> bool {
    static CACHE: std::sync::Mutex<Option<(std::time::Instant, bool)>> =
        std::sync::Mutex::new(None);
    if let Some((at, public)) = *CACHE.lock().unwrap() {
        if at.elapsed() < Duration::from_secs(60) {
            return public;
        }
    }
    let public = ps_lines("(Get-NetConnectionProfile).NetworkCategory")
        .iter()
        .any(|l| l.trim().eq_ignore_ascii_case("Public"));
    *CACHE.lock().unwrap() = Some((std::time::Instant::now(), public));
    public
}

impl Inner {
    /// Ports other shared sites already hold.
    fn lan_ports_taken(&self, except: &str) -> Vec<u16> {
        self.domains
            .lock()
            .unwrap()
            .list()
            .into_iter()
            .filter(|d| d.lan && !d.hostname.eq_ignore_ascii_case(except))
            .filter_map(|d| d.lan_port)
            .collect()
    }

    /// The `.local` link: the site's normal ports, HTTPS when the site has it (its plain-HTTP
    /// vhost would redirect there anyway) unless the user picked plain HTTP for the phone.
    fn local_url(&self, d: &crate::domain::Domain, name: &str) -> String {
        let cfg = self.web_config();
        let ports = cfg.effective_ports(&crate::domain::resolved_server(d, &cfg));
        if d.https && !d.lan_http {
            match ports.https {
                443 => format!("https://{name}/"),
                p => format!("https://{name}:{p}/"),
            }
        } else {
            match ports.http {
                80 => format!("http://{name}/"),
                p => format!("http://{name}:{p}/"),
            }
        }
    }

    /// Starts, updates or stops the `.local` responder so it answers exactly the shared sites'
    /// names on the current LAN address. Returns why it could not start, if it could not.
    pub(crate) fn sync_mdns(&self) -> Option<String> {
        let names: Vec<String> = self
            .domains
            .lock()
            .unwrap()
            .list()
            .into_iter()
            .filter(|d| d.lan)
            .filter_map(|d| d.lan_local)
            .collect();
        let ip = if names.is_empty() { None } else { lan_ipv4() };
        let mut slot = self.mdns.lock().unwrap();
        let Some(ip) = ip else {
            *slot = None;
            return None;
        };
        if let Some(r) = slot.as_ref() {
            if r.ip() == ip {
                r.set_names(names);
                return None;
            }
        }
        *slot = None; // the address changed: say goodbye on the old one first
        match crate::mdns::Responder::start(ip, names) {
            Ok(r) => {
                *slot = Some(r);
                None
            }
            Err(e) => Some(e),
        }
    }

    fn lan_info_for(&self, d: &crate::domain::Domain, check_network: bool) -> LanInfo {
        let can_https = d.https;
        let (true, Some(port), Some(ip)) = (d.lan, d.lan_port, d.lan_ip.clone()) else {
            return LanInfo {
                on: false,
                ip: None,
                url: None,
                qr_svg: None,
                warnings: vec![],
                port: None,
                https: false,
                can_https,
                local_name: None,
                local_url: None,
                local_qr_svg: None,
                mdns_running: false,
            };
        };
        let https = can_https && !d.lan_http;
        let url = lan_url(&ip, port, https);
        let mut warnings = Vec::new();
        if check_network && network_is_public() {
            warnings.push(
                "Windows treats this network as Public, so your phone may not reach the site. Set it to Private in Windows Settings > Network, or allow the web server in Windows Firewall."
                    .to_string(),
            );
        }
        let mdns_running = self.mdns.lock().unwrap().is_some();
        if d.lan_local.is_some() && !mdns_running {
            warnings.push(
                "The .local name is not being announced: OLS could not open UDP port 5353. The IP link still works."
                    .to_string(),
            );
        }
        let local_url = d.lan_local.as_deref().map(|n| self.local_url(d, n));
        LanInfo {
            on: true,
            ip: Some(ip),
            qr_svg: Some(qr_svg(&url)),
            url: Some(url),
            warnings,
            port: Some(port),
            https,
            can_https,
            local_name: d.lan_local.clone(),
            local_qr_svg: local_url.as_deref().map(qr_svg),
            local_url,
            mdns_running,
        }
    }

    fn lan_site(&self, hostname: &str, problem: &str) -> Result<crate::domain::Domain, CoreError> {
        self.domains.lock().unwrap().get(hostname).ok_or_else(|| {
            CoreError::failed_fix(
                problem.to_string(),
                format!("No site called \"{hostname}\" exists."),
                "Pick a site from the Sites page.",
            )
        })
    }

    /// Turns sharing on or off for one site and re-renders the web configuration. On picks
    /// the site's own LAN port (kept while it stays shared) and records the LAN address.
    pub fn set_site_lan(&self, hostname: &str, on: bool) -> Result<LanInfo, CoreError> {
        let mut d = self.lan_site(hostname, "OLS cannot share that site.")?;
        if on {
            // A phone can only reach a site the web server is serving right now: a disabled
            // site has no virtual host and a stopped server has no listener.
            if !d.enabled {
                return Err(CoreError::failed_fix(
                    "OLS cannot share a site that is turned off.",
                    format!("{hostname} is disabled, so the web server does not serve it."),
                    "Turn the site on in the Sites page, then share it.",
                ));
            }
            if !self
                .web
                .is_running_id(&crate::domain::resolved_server(&d, &self.web_config()))
            {
                return Err(CoreError::failed_fix(
                    "OLS cannot share a site while its web server is stopped.",
                    format!("The web server that serves {hostname} is not running, so a phone has nothing to connect to."),
                    "Start the web server from the Sites page (Start), then share the site.",
                ));
            }
            let ip = lan_ipv4().ok_or_else(|| {
                CoreError::failed_fix(
                    "This computer is not on a private network (Wi-Fi or Ethernet).",
                    "OLS found no private address (10.x, 172.16-31.x or 192.168.x) to share the site on.",
                    "Connect to your Wi-Fi or Ethernet network, then try again.",
                )
            })?;
            let taken = self.lan_ports_taken(hostname);
            let port = match d.lan_port.filter(|p| !taken.contains(p)) {
                Some(p) => p,
                None => pick_port(&taken, &port_is_free).ok_or_else(|| {
                    CoreError::failed_fix(
                        "OLS could not find a free port to share the site on.",
                        format!(
                            "Every port from {} to {} is in use.",
                            LAN_PORTS.start(),
                            LAN_PORTS.end()
                        ),
                        "Stop sharing a site you no longer test on a phone, or close the program using those ports.",
                    )
                })?,
            };
            d.lan = true;
            d.lan_ip = Some(ip.to_string());
            d.lan_port = Some(port);
            d.lan_local = local_name_for(&d.hostname);
        } else {
            d.lan = false;
            d.lan_ip = None;
            d.lan_port = None;
            d.lan_local = None;
        }
        let saved = self.domains.lock().unwrap().update(d)?;
        self.apply_web(&[])?;
        let _ = self.sync_mdns();
        Ok(self.lan_info_for(&saved, false))
    }

    /// Plain HTTP (no certificate on the phone) or HTTPS for a shared site's phone link.
    pub fn set_site_lan_http(&self, hostname: &str, http: bool) -> Result<LanInfo, CoreError> {
        let mut d = self.lan_site(hostname, "OLS cannot change that site.")?;
        d.lan_http = http;
        let saved = self.domains.lock().unwrap().update(d)?;
        if saved.lan {
            self.apply_web(&[])?;
        }
        Ok(self.lan_info_for(&saved, false))
    }

    /// The current state. When the computer's address changed, or a site shared by an older
    /// version has no port yet, it is brought up to date first.
    pub fn get_lan_info(&self, hostname: &str) -> Result<LanInfo, CoreError> {
        let d = self.lan_site(hostname, "That site does not exist.")?;
        let d = if refresh_one(self, d.clone()) {
            self.apply_web(&[])?;
            self.lan_site(hostname, "That site does not exist.")?
        } else {
            d
        };
        let _ = self.sync_mdns();
        Ok(self.lan_info_for(&d, true))
    }
}

/// The name a phone can open the site by: its own domain when that ends in `.local`.
fn local_name_for(hostname: &str) -> Option<String> {
    crate::mdns::is_local_name(hostname)
        .then(|| hostname.trim_end_matches('.').to_ascii_lowercase())
}

/// Brings one shared site's address and port up to date. `true` when it was saved changed.
fn refresh_one(inner: &Inner, mut d: crate::domain::Domain) -> bool {
    if !d.lan {
        return false;
    }
    let mut changed = false;
    if let Some(ip) = lan_ipv4().map(|ip| ip.to_string()) {
        if d.lan_ip.as_deref() != Some(ip.as_str()) {
            d.lan_ip = Some(ip);
            changed = true;
        }
    }
    if d.lan_port.is_none() {
        if let Some(p) = pick_port(&inner.lan_ports_taken(&d.hostname), &port_is_free) {
            d.lan_port = Some(p);
            changed = true;
        }
    }
    let local = local_name_for(&d.hostname);
    if d.lan_local != local {
        d.lan_local = local;
        changed = true;
    }
    changed && inner.domains.lock().unwrap().update(d).is_ok()
}

/// Turns sharing off for every shared site. Sharing never outlives the run that started it:
/// this is called when OLS starts (a crash or a kill leaves `lan` saved as on), when it quits,
/// and by `reconcile`. `apply` re-renders the web configuration so the extra listeners go
/// away; startup and shutdown skip it because the web servers are not running then and the
/// next start renders from the saved (now unshared) sites.
pub fn stop_all(inner: &Inner, apply: bool) -> usize {
    let shared: Vec<crate::domain::Domain> = inner
        .domains
        .lock()
        .unwrap()
        .list()
        .into_iter()
        .filter(|d| d.lan)
        .collect();
    let mut stopped = 0;
    for mut d in shared {
        d.lan = false;
        d.lan_ip = None;
        d.lan_port = None;
        d.lan_local = None;
        if inner.domains.lock().unwrap().update(d).is_ok() {
            stopped += 1;
        }
    }
    if stopped > 0 {
        if apply {
            let _ = inner.apply_web(&[]);
        }
        tracing::info!(stopped, "sharing on the network turned off");
    }
    let _ = inner.sync_mdns();
    stopped
}

/// Turns sharing off for any shared site that can no longer be reached: the site is disabled
/// or gone, or the web server that serves it is not running. Cheap when nothing is shared
/// (one pass over the domain list), so it runs on every web status poll and every minute.
pub fn reconcile(inner: &Inner) -> usize {
    let cfg = inner.web_config();
    let dead: Vec<crate::domain::Domain> = inner
        .domains
        .lock()
        .unwrap()
        .list()
        .into_iter()
        .filter(|d| d.lan)
        .filter(|d| {
            !d.enabled
                || !inner
                    .web
                    .is_running_id(&crate::domain::resolved_server(d, &cfg))
        })
        .collect();
    if dead.is_empty() {
        return 0;
    }
    let mut stopped = 0;
    for mut d in dead {
        d.lan = false;
        d.lan_ip = None;
        d.lan_port = None;
        d.lan_local = None;
        if inner.domains.lock().unwrap().update(d).is_ok() {
            stopped += 1;
        }
    }
    // The server is down or the site is off, so the configuration is only re-rendered when
    // a server is still up (to drop the listener); otherwise the next start does it.
    if inner.web.is_running() {
        let _ = inner.apply_web(&[]);
    }
    let _ = inner.sync_mdns();
    tracing::info!(
        stopped,
        "sharing stopped: the site is off or its web server is not running"
    );
    stopped
}

/// Minute tick: follows a changed LAN address (the certificate then covers the new IP), gives
/// a port and a `.local` name to a site shared by an earlier build, and keeps the mDNS
/// responder on the current address (or stops it once nothing is shared).
pub fn tick(inner: &Inner) {
    reconcile(inner);
    let shared: Vec<crate::domain::Domain> = inner
        .domains
        .lock()
        .unwrap()
        .list()
        .into_iter()
        .filter(|d| d.lan)
        .collect();
    let mut changed = false;
    for d in shared {
        changed |= refresh_one(inner, d);
    }
    if changed {
        let _ = inner.apply_web(&[]);
    }
    let _ = inner.sync_mdns();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_private_ranges_count() {
        for ok in [
            [10, 0, 0, 1],
            [172, 16, 0, 1],
            [172, 31, 255, 1],
            [192, 168, 0, 9],
        ] {
            assert!(is_private(Ipv4Addr::from(ok)), "{ok:?}");
        }
        for no in [
            [8, 8, 8, 8],
            [172, 32, 0, 1],
            [172, 15, 0, 1],
            [169, 254, 1, 1],
            [100, 64, 0, 1],
            [127, 0, 0, 1],
        ] {
            assert!(!is_private(Ipv4Addr::from(no)), "{no:?}");
        }
        if let Some(ip) = lan_ipv4() {
            assert!(is_private(ip));
        }
    }

    #[test]
    fn the_address_list_fallback_takes_the_first_private_address() {
        let lines: Vec<String> = ["127.0.0.1", "169.254.3.4", "  192.168.1.100 ", "10.0.0.2"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(first_private(&lines), Some(Ipv4Addr::new(192, 168, 1, 100)));
        assert_eq!(first_private(&["not an ip".to_string()]), None);
    }

    #[test]
    fn ports_skip_taken_and_busy_ones() {
        let busy = |p: u16| p != 8701;
        assert_eq!(pick_port(&[8700], &busy), Some(8702));
        assert_eq!(pick_port(&[], &|_| true), Some(*LAN_PORTS.start()));
        assert_eq!(pick_port(&[], &|_| false), None);
    }

    #[test]
    fn only_local_domains_get_a_name_link() {
        assert_eq!(
            local_name_for("Acme-Store.local"),
            Some("acme-store.local".into())
        );
        assert_eq!(local_name_for("shop.test"), None);
    }

    #[test]
    fn links_are_ip_and_port() {
        assert_eq!(
            lan_url("192.168.1.5", 8700, true),
            "https://192.168.1.5:8700/"
        );
        assert_eq!(
            lan_url("192.168.1.5", 8701, false),
            "http://192.168.1.5:8701/"
        );
    }

    #[test]
    fn the_certificate_covers_the_lan_ip_only_while_shared_over_https() {
        let mut d = crate::domain::Domain {
            hostname: "shop.test".into(),
            lan_ip: Some("192.168.1.5".into()),
            ..Default::default()
        };
        let before = crate::certs::CertificateManager::names_for(&d);
        assert!(
            !before.contains(&"192.168.1.5".to_string()),
            "off: unchanged"
        );
        d.lan = true;
        let on = crate::certs::CertificateManager::names_for(&d);
        assert!(on.contains(&"192.168.1.5".to_string()));
        assert_eq!(&on[..before.len()], &before[..]);
        d.lan_http = true;
        assert_eq!(
            crate::certs::CertificateManager::names_for(&d),
            before,
            "plain HTTP needs no IP"
        );
    }

    #[test]
    fn an_old_domain_without_the_new_fields_still_loads() {
        let d = crate::domain::Domain {
            hostname: "old.test".into(),
            ..Default::default()
        };
        let mut v = serde_json::to_value(&d).unwrap();
        for k in ["lan", "lan_ip", "lan_port", "lan_http", "lan_local"] {
            v.as_object_mut().unwrap().remove(k);
        }
        // A site shared by the first 2.12 build still has its sslip.io name; it is ignored.
        v.as_object_mut()
            .unwrap()
            .insert("lan_alias".into(), "old-test.192-168-1-5.sslip.io".into());
        let back: crate::domain::Domain = serde_json::from_value(v).unwrap();
        assert!(!back.lan && back.lan_ip.is_none() && back.lan_port.is_none() && !back.lan_http);
        // And a default domain writes none of them.
        let out = serde_json::to_value(&d).unwrap();
        for k in ["lan_ip", "lan_port", "lan_http", "lan_local"] {
            assert!(out.get(k).is_none(), "{k}");
        }
    }

    #[test]
    fn qr_codes_are_svg() {
        let svg = qr_svg("https://192.168.1.5:8700/");
        assert!(svg.contains("<svg"));
    }
}
