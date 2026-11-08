//! Open a site on your phone (§178, part A): a LAN hostname per site.
//!
//! Phones cannot use OLS's local DNS, so a site gets a second name under sslip.io, a public
//! DNS service that answers `*.192-168-1-5.sslip.io` with `192.168.1.5`. The name is added to
//! the site's server names and to its certificate. Sites without `lan` render byte for byte
//! as before. OLS does not change the Windows firewall.

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
}

/// `10/8`, `172.16/12` and `192.168/16` only.
pub fn is_private(ip: Ipv4Addr) -> bool {
    ip.is_private()
}

/// This computer's private IPv4 address on the network it uses to reach the outside.
/// A UDP `connect` sends no packet; it only makes the OS pick the outgoing interface.
pub fn lan_ipv4() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?;
    match socket.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(ip) if is_private(ip) => Some(ip),
        _ => None,
    }
}

/// `shop.test` + `192.168.1.5` -> `shop-test.192-168-1-5.sslip.io`. The first label is cut to
/// fit the 63-character limit.
pub fn lan_alias(hostname: &str, ip: Ipv4Addr) -> String {
    let o = ip.octets();
    let mut label: String = hostname
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    label.truncate(63);
    let label = label.trim_matches('-');
    let label = if label.is_empty() { "site" } else { label };
    format!("{label}.{}-{}-{}-{}.sslip.io", o[0], o[1], o[2], o[3])
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

/// `true` when Windows calls the active network "Public" (inbound connections are blocked).
fn network_is_public() -> bool {
    let args = [
        "-NoProfile".to_string(),
        "-NonInteractive".to_string(),
        "-Command".to_string(),
        "(Get-NetConnectionProfile).NetworkCategory".to_string(),
    ];
    let out = crate::exec::run_capture(
        &crate::exec::powershell_path(),
        &args,
        None,
        &[],
        Duration::from_secs(5),
    );
    out.success()
        && out
            .stdout
            .lines()
            .any(|l| l.trim().eq_ignore_ascii_case("Public"))
}

impl Inner {
    fn lan_url(&self, d: &crate::domain::Domain, alias: &str) -> String {
        let cfg = self.web_config();
        let ports = cfg.effective_ports(&crate::domain::resolved_server(d, &cfg));
        if d.https {
            if ports.https == 443 {
                format!("https://{alias}/")
            } else {
                format!("https://{alias}:{}/", ports.https)
            }
        } else if ports.http == 80 {
            format!("http://{alias}/")
        } else {
            format!("http://{alias}:{}/", ports.http)
        }
    }

    fn lan_info_for(&self, d: &crate::domain::Domain, check_network: bool) -> LanInfo {
        let (Some(alias), true) = (d.lan_alias.as_deref(), d.lan) else {
            return LanInfo {
                on: false,
                ip: None,
                url: None,
                qr_svg: None,
                warnings: vec![],
            };
        };
        let url = self.lan_url(d, alias);
        let mut warnings = Vec::new();
        if check_network && network_is_public() {
            warnings.push(
                "Windows treats this network as Public, so your phone may not reach the site. Set it to Private in Windows Settings > Network, or allow the web server in Windows Firewall."
                    .to_string(),
            );
        }
        LanInfo {
            on: true,
            ip: lan_ipv4().map(|ip| ip.to_string()),
            qr_svg: Some(qr_svg(&url)),
            url: Some(url),
            warnings,
        }
    }

    /// Turns the LAN name on or off for one site and re-renders the web configuration.
    pub fn set_site_lan(&self, hostname: &str, on: bool) -> Result<LanInfo, CoreError> {
        let mut d = self.domains.lock().unwrap().get(hostname).ok_or_else(|| {
            CoreError::failed_fix(
                "OLS cannot share that site.",
                format!("No site called \"{hostname}\" exists."),
                "Pick a site from the Sites page.",
            )
        })?;
        if on {
            let ip = lan_ipv4().ok_or_else(|| {
                CoreError::failed_fix(
                    "This computer is not on a private network (Wi-Fi or Ethernet).",
                    "OLS found no private address (10.x, 172.16-31.x or 192.168.x) to share the site on.",
                    "Connect to your Wi-Fi or Ethernet network, then try again.",
                )
            })?;
            let alias = lan_alias(hostname, ip);
            crate::domain::validate_hostname(&alias)?;
            d.lan = true;
            d.lan_alias = Some(alias);
        } else {
            d.lan = false;
            d.lan_alias = None;
        }
        let saved = self.domains.lock().unwrap().update(d)?;
        self.apply_web(&[])?;
        Ok(self.lan_info_for(&saved, false))
    }

    /// The current state; when the computer's IP changed it re-renders the name first.
    pub fn get_lan_info(&self, hostname: &str) -> Result<LanInfo, CoreError> {
        let mut d = self.domains.lock().unwrap().get(hostname).ok_or_else(|| {
            CoreError::failed_fix(
                "That site does not exist.",
                format!("No site called \"{hostname}\" exists."),
                "Pick a site from the Sites page.",
            )
        })?;
        if d.lan {
            if let Some(ip) = lan_ipv4() {
                let alias = lan_alias(hostname, ip);
                if d.lan_alias.as_deref() != Some(alias.as_str()) {
                    d.lan_alias = Some(alias);
                    d = self.domains.lock().unwrap().update(d)?;
                    self.apply_web(&[])?;
                }
            }
        }
        Ok(self.lan_info_for(&d, true))
    }
}

/// Minute tick: only when a site has `lan` on, and then one UDP socket and no packets.
pub fn tick(inner: &Inner) {
    let lan_sites: Vec<String> = inner
        .domains
        .lock()
        .unwrap()
        .list()
        .into_iter()
        .filter(|d| d.lan)
        .map(|d| d.hostname)
        .collect();
    if lan_sites.is_empty() {
        return;
    }
    let Some(ip) = lan_ipv4() else {
        return;
    };
    let mut changed = false;
    for host in lan_sites {
        let Some(mut d) = inner.domains.lock().unwrap().get(&host) else {
            continue;
        };
        let alias = lan_alias(&host, ip);
        if d.lan_alias.as_deref() != Some(alias.as_str()) {
            d.lan_alias = Some(alias);
            changed |= inner.domains.lock().unwrap().update(d).is_ok();
        }
    }
    if changed {
        let _ = inner.apply_web(&[]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_are_valid_hostnames_and_labels_fit() {
        let ip = Ipv4Addr::new(192, 168, 1, 5);
        assert_eq!(lan_alias("shop.test", ip), "shop-test.192-168-1-5.sslip.io");
        for host in [
            "a.test",
            &format!("{}.test", "x".repeat(80)),
            "-weird-.test",
            "UPPER.Case.test",
        ] {
            let alias = lan_alias(host, ip);
            assert!(crate::domain::validate_hostname(&alias).is_ok(), "{alias}");
            assert!(alias.split('.').all(|l| l.len() <= 63), "{alias}");
        }
    }

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
    fn the_certificate_covers_the_lan_name_only_while_sharing_is_on() {
        let mut d = crate::domain::Domain {
            hostname: "shop.test".into(),
            ..Default::default()
        };
        let before = crate::certs::CertificateManager::names_for(&d);
        d.lan_alias = Some("shop-test.192-168-1-5.sslip.io".into());
        assert_eq!(
            crate::certs::CertificateManager::names_for(&d),
            before,
            "off: unchanged"
        );
        d.lan = true;
        let on = crate::certs::CertificateManager::names_for(&d);
        assert!(on.contains(&"shop-test.192-168-1-5.sslip.io".to_string()));
        assert_eq!(&on[..before.len()], &before[..]);
    }

    #[test]
    fn an_old_domain_without_the_new_fields_still_loads() {
        let d = crate::domain::Domain {
            hostname: "old.test".into(),
            ..Default::default()
        };
        let mut v = serde_json::to_value(&d).unwrap();
        v.as_object_mut().unwrap().remove("lan");
        v.as_object_mut().unwrap().remove("lan_alias");
        let back: crate::domain::Domain = serde_json::from_value(v).unwrap();
        assert!(!back.lan && back.lan_alias.is_none());
        // And a default domain does not write the alias at all.
        assert!(serde_json::to_value(&d).unwrap().get("lan_alias").is_none());
    }

    #[test]
    fn qr_codes_are_svg() {
        let svg = qr_svg("https://shop-test.192-168-1-5.sslip.io/");
        assert!(svg.contains("<svg"));
    }
}
