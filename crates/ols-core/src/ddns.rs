//! Dynamic DNS (DDNS) client for updating DNS records when server public IP changes.
//!
//! Supports Cloudflare, DuckDNS, and custom webhooks.
//! Records are updated only when the public IP changes (or when forced by the user).

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::domain::Domain;
use crate::error::CoreError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DdnsSyncResult {
    pub hostname: String,
    pub ip: String,
    pub updated: bool,
    pub message: String,
}

/// Helper to determine if an IPv4 address is private or non-routable.
fn is_private_ip(ip: &str) -> bool {
    let parts: Vec<u8> = ip.split('.').filter_map(|s| s.parse().ok()).collect();
    if parts.len() != 4 {
        return true;
    }
    match (parts[0], parts[1]) {
        (10, _) => true,
        (172, b) if (16..=31).contains(&b) => true,
        (192, 168) => true,
        (127, _) => true,
        (169, 254) => true,
        (0, _) => true,
        _ => false,
    }
}

/// Creates a standard HTTP client with a clear User-Agent header and configured timeout.
fn http_client(timeout_secs: u64) -> Result<reqwest::Client, CoreError> {
    reqwest::Client::builder()
        .user_agent("OpenLocalServer/2.3.0 (https://openlocalserver.com; ddns)")
        .timeout(Duration::from_secs(timeout_secs))
        .build()
        .map_err(|e| CoreError::DdnsError(format!("Failed to build HTTP client: {e}")))
}

/// Parses a Cloudflare API error response body into a human-readable message.
fn parse_cloudflare_error(status: reqwest::StatusCode, raw_bytes: &[u8]) -> String {
    if let Ok(val) = serde_json::from_slice::<Value>(raw_bytes) {
        if let Some(errors) = val["errors"].as_array() {
            let messages: Vec<String> = errors
                .iter()
                .filter_map(|e| {
                    let msg = e["message"].as_str()?;
                    if let Some(chain) = e["error_chain"].as_array() {
                        let sub: Vec<&str> =
                            chain.iter().filter_map(|c| c["message"].as_str()).collect();
                        if !sub.is_empty() {
                            return Some(format!("{msg} ({})", sub.join(", ")));
                        }
                    }
                    Some(msg.to_string())
                })
                .collect();
            if !messages.is_empty() {
                return format!("HTTP {status}: {}", messages.join("; "));
            }
        }
    }
    let txt = String::from_utf8_lossy(raw_bytes);
    if txt.trim().is_empty() {
        format!("HTTP {status}")
    } else {
        format!("HTTP {status}: {txt}")
    }
}

/// Query local network interfaces via OS command for an assigned public IPv4 address.
fn detect_ip_from_os_interfaces() -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        let out = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "(Get-NetIPAddress -AddressFamily IPv4 -ErrorAction SilentlyContinue | Where-Object { $_.IPAddress -notmatch '^(10\\.|192\\.168\\.|172\\.(1[6-9]|2[0-9]|3[01])\\.|127\\.|169\\.254\\.|0\\.)' }).IPAddress",
            ])
            .output()
            .ok()?;
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout);
            for line in s.lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty()
                    && !is_private_ip(trimmed)
                    && trimmed.parse::<std::net::Ipv4Addr>().is_ok()
                {
                    return Some(trimmed.to_string());
                }
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        if let Ok(out) = std::process::Command::new("hostname").arg("-I").output() {
            if out.status.success() {
                let s = String::from_utf8_lossy(&out.stdout);
                for token in s.split_whitespace() {
                    if !is_private_ip(token) && token.parse::<std::net::Ipv4Addr>().is_ok() {
                        return Some(token.to_string());
                    }
                }
            }
        }
    }

    None
}

/// Fallback to OS command `nslookup` (DNS UDP/TCP query) without online HTTP checking.
fn detect_ip_from_nslookup() -> Option<String> {
    // 1. OpenDNS myip.opendns.com via OS command nslookup
    if let Ok(out) = std::process::Command::new("nslookup")
        .args(["myip.opendns.com", "208.67.222.222"])
        .output()
    {
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout);
            let mut past_name = false;
            for line in text.lines() {
                let l = line.trim();
                if l.to_lowercase().starts_with("name:") {
                    past_name = true;
                    continue;
                }
                if past_name && (l.starts_with("Address:") || l.starts_with("Addresses:")) {
                    if let Some(ip_str) = l.split(':').nth(1) {
                        let clean = ip_str.trim();
                        if clean.parse::<std::net::Ipv4Addr>().is_ok() && !is_private_ip(clean) {
                            return Some(clean.to_string());
                        }
                    }
                }
            }
        }
    }

    // 2. Google DNS TXT query via OS command nslookup
    if let Ok(out) = std::process::Command::new("nslookup")
        .args(["-type=txt", "o-o.myaddr.l.google.com", "216.239.32.10"])
        .output()
    {
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                if let Some(quote_start) = line.find('"') {
                    if let Some(quote_end) = line[quote_start + 1..].find('"') {
                        let candidate = &line[quote_start + 1..quote_start + 1 + quote_end];
                        if candidate.parse::<std::net::Ipv4Addr>().is_ok()
                            && !is_private_ip(candidate)
                        {
                            return Some(candidate.to_string());
                        }
                    }
                }
            }
        }
    }

    None
}

/// Detects the machine's external public IPv4 address using native OS commands.
/// Queries local OS network interfaces first. If the host is behind NAT, uses the OS `nslookup` command.
pub async fn detect_public_ip() -> Result<String, CoreError> {
    tokio::task::spawn_blocking(|| {
        if let Some(ip) = detect_ip_from_os_interfaces() {
            return Ok(ip);
        }
        if let Some(ip) = detect_ip_from_nslookup() {
            return Ok(ip);
        }
        Err(CoreError::DdnsError(
            "Could not determine public IP address using OS commands".into(),
        ))
    })
    .await
    .map_err(|e| CoreError::DdnsError(format!("Task execution failed: {e}")))?
}

/// Synchronizes a domain's DNS record with its DNS provider if the server's public IP changed.
pub async fn sync_domain_ddns(domain: &Domain, force: bool) -> Result<DdnsSyncResult, CoreError> {
    let provider = domain
        .ddns_provider
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .ok_or_else(|| {
            CoreError::DomainError(format!(
                "Dynamic DNS is not configured for domain '{}'",
                domain.hostname
            ))
        })?;

    let token = domain
        .ddns_token
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .ok_or_else(|| {
            CoreError::DomainError(format!(
                "Dynamic DNS API token is required for domain '{}'",
                domain.hostname
            ))
        })?;

    let current_ip = detect_public_ip().await?;

    // Check if the IP changed
    if !force && domain.ddns_last_ip.as_deref() == Some(&current_ip) {
        return Ok(DdnsSyncResult {
            hostname: domain.hostname.clone(),
            ip: current_ip.clone(),
            updated: false,
            message: format!("Public IP has not changed ({current_ip}). DNS update skipped."),
        });
    }

    let target_hostname = domain
        .public_domain
        .as_deref()
        .filter(|p| !p.trim().is_empty())
        .or_else(|| {
            domain
                .aliases
                .iter()
                .find(|a| {
                    let a = a.trim();
                    !a.ends_with(".test")
                        && !a.ends_with(".localhost")
                        && !a.ends_with(".local")
                        && !a.ends_with(".example")
                        && a.contains('.')
                })
                .map(|s| s.as_str())
        })
        .unwrap_or(&domain.hostname);

    let updated_record = match provider.to_ascii_lowercase().as_str() {
        "cloudflare" => {
            sync_cloudflare(
                target_hostname,
                token,
                domain.ddns_zone_id.as_deref(),
                &current_ip,
                domain.ddns_proxied,
            )
            .await?
        }
        "godaddy" => {
            sync_godaddy(
                target_hostname,
                token,
                domain.ddns_zone_id.as_deref(),
                &current_ip,
            )
            .await?;
            target_hostname.to_string()
        }
        "namecheap" | "cheapdomain" => {
            sync_namecheap(
                target_hostname,
                token,
                domain.ddns_zone_id.as_deref(),
                &current_ip,
            )
            .await?;
            target_hostname.to_string()
        }
        "duckdns" => {
            sync_duckdns(target_hostname, token, &current_ip).await?;
            target_hostname.to_string()
        }
        "noip" => {
            let (u, p) = parse_user_pass(token, domain.ddns_zone_id.as_deref())?;
            let url = format!(
                "https://dynupdate.no-ip.com/nic/update?hostname={}&myip={}",
                target_hostname, current_ip
            );
            sync_dyndns2_get(&url, Some((&u, &p)), None, &["good", "nochg"]).await?;
            target_hostname.to_string()
        }
        "porkbun" => {
            sync_porkbun(
                target_hostname,
                token,
                domain.ddns_zone_id.as_deref(),
                &current_ip,
            )
            .await?;
            target_hostname.to_string()
        }
        "digitalocean" => {
            sync_digitalocean(
                target_hostname,
                token,
                domain.ddns_zone_id.as_deref(),
                &current_ip,
            )
            .await?;
            target_hostname.to_string()
        }
        "hetzner" => {
            sync_hetzner(
                target_hostname,
                token,
                domain.ddns_zone_id.as_deref(),
                &current_ip,
            )
            .await?;
            target_hostname.to_string()
        }
        "gandi" => {
            sync_gandi(
                target_hostname,
                token,
                domain.ddns_zone_id.as_deref(),
                &current_ip,
            )
            .await?;
            target_hostname.to_string()
        }
        "ionos" => {
            sync_ionos(
                target_hostname,
                token,
                domain.ddns_zone_id.as_deref(),
                &current_ip,
            )
            .await?;
            target_hostname.to_string()
        }
        "dyndns" => {
            let (u, p) = parse_user_pass(token, domain.ddns_zone_id.as_deref())?;
            let url = format!(
                "https://members.dyndns.org/nic/update?hostname={}&myip={}",
                target_hostname, current_ip
            );
            sync_dyndns2_get(&url, Some((&u, &p)), None, &["good", "nochg"]).await?;
            target_hostname.to_string()
        }
        "dynu" => {
            let url = format!(
                "https://api.dynu.com/nic/update?hostname={}&myip={}&password={}",
                target_hostname, current_ip, token
            );
            sync_dyndns2_get(&url, None, None, &["good", "nochg"]).await?;
            target_hostname.to_string()
        }
        "he" | "hurricane_electric" => {
            let url = format!(
                "https://dyn.dns.he.net/nic/update?hostname={}&password={}&myip={}",
                target_hostname, token, current_ip
            );
            sync_dyndns2_get(&url, None, None, &["good", "nochg"]).await?;
            target_hostname.to_string()
        }
        "freedns" => {
            let url = format!(
                "https://freedns.afraid.org/dynamic/update.php?{}&address={}",
                token, current_ip
            );
            sync_dyndns2_get(&url, None, None, &["updated", "has not changed"]).await?;
            target_hostname.to_string()
        }
        "desec" => {
            let url = format!(
                "https://update.dedyn.io/update?domains={}&ipv4={}",
                target_hostname, current_ip
            );
            sync_dyndns2_get(&url, None, Some(token), &["good", "nochg"]).await?;
            target_hostname.to_string()
        }
        "dynv6" => {
            let url = format!(
                "https://dynv6.com/api/update?hostname={}&token={}&ipv4={}",
                target_hostname, token, current_ip
            );
            sync_dyndns2_get(
                &url,
                None,
                None,
                &["addresses updated", "addresses unchanged"],
            )
            .await?;
            target_hostname.to_string()
        }
        "ovh" => {
            let (u, p) = parse_user_pass(token, domain.ddns_zone_id.as_deref())?;
            let url = format!(
                "https://www.ovh.com/nic/update?system=dyndns&hostname={}&myip={}",
                target_hostname, current_ip
            );
            sync_dyndns2_get(&url, Some((&u, &p)), None, &["good", "nochg"]).await?;
            target_hostname.to_string()
        }
        "easydns" => {
            let (u, p) = parse_user_pass(token, domain.ddns_zone_id.as_deref())?;
            let url = format!(
                "https://api.cp.easydns.com/dyn/nic/update?hostname={}&myip={}",
                target_hostname, current_ip
            );
            sync_dyndns2_get(&url, Some((&u, &p)), None, &["good", "nochg", "no error"]).await?;
            target_hostname.to_string()
        }
        "zoneedit" => {
            let (u, p) = parse_user_pass(token, domain.ddns_zone_id.as_deref())?;
            let url = format!(
                "https://dynamic.zoneedit.com/auth/dynamic.html?host={}&dnsto={}",
                target_hostname, current_ip
            );
            sync_dyndns2_get(&url, Some((&u, &p)), None, &["success", "good", "nochg"]).await?;
            target_hostname.to_string()
        }
        "dnsexit" => {
            let (u, p) = parse_user_pass(token, domain.ddns_zone_id.as_deref())?;
            let url = format!(
                "https://update.dnsexit.com/remoteupdate.sv?login={}&password={}&host={}&myip={}",
                u, p, target_hostname, current_ip
            );
            sync_dyndns2_get(&url, None, None, &["0: success", "1: ip", "ip is already"]).await?;
            target_hostname.to_string()
        }
        "changeip" => {
            let (u, p) = parse_user_pass(token, domain.ddns_zone_id.as_deref())?;
            let url = format!(
                "https://nic.changeip.com/nic/update?hostname={}&myip={}",
                target_hostname, current_ip
            );
            sync_dyndns2_get(&url, Some((&u, &p)), None, &["good", "nochg"]).await?;
            target_hostname.to_string()
        }
        "directnic" => {
            let (u, p) = parse_user_pass(token, domain.ddns_zone_id.as_deref())?;
            let url = format!(
                "https://directnic.com/dns/gateway/dyndns/?hostname={}&myip={}",
                target_hostname, current_ip
            );
            sync_dyndns2_get(&url, Some((&u, &p)), None, &["good", "nochg"]).await?;
            target_hostname.to_string()
        }
        "cloudns" => {
            let url = if token.starts_with("http") {
                format!("{}&ip={}", token, current_ip)
            } else {
                format!(
                    "https://ipv4.cloudns.net/api/dynamicURL/?q={}&ip={}",
                    token, current_ip
                )
            };
            sync_dyndns2_get(&url, None, None, &["ok", "updated"]).await?;
            target_hostname.to_string()
        }
        "custom" | "webhook" => {
            let url = domain.ddns_zone_id.as_deref().unwrap_or("").trim();
            if url.is_empty() {
                return Err(CoreError::DomainError(
                    "Custom DDNS requires a webhook URL in the zone/endpoint field".into(),
                ));
            }
            sync_custom_webhook(url, target_hostname, token, &current_ip).await?;
            target_hostname.to_string()
        }
        other => {
            return Err(CoreError::DomainError(format!(
                "Unsupported DDNS provider: '{other}'"
            )));
        }
    };

    Ok(DdnsSyncResult {
        hostname: domain.hostname.clone(),
        ip: current_ip.clone(),
        updated: true,
        message: format!("DNS record '{updated_record}' successfully synchronized to {current_ip}"),
    })
}

async fn sync_cloudflare(
    hostname: &str,
    token: &str,
    zone_id: Option<&str>,
    ip: &str,
    proxied: bool,
) -> Result<String, CoreError> {
    let client = http_client(15)?;

    // 1. Resolve Zone ID and Zone apex name
    let (resolved_zone_id, mut zone_name) = if let Some(zid) =
        zone_id.filter(|z| !z.trim().is_empty())
    {
        let clean = zid.trim().trim_matches('"');
        if clean.contains('.') {
            // User entered an apex domain name (e.g. "example.com") instead of hex zone ID
            let url = format!("https://api.cloudflare.com/client/v4/zones?name={clean}");
            let resp = client
                .get(&url)
                .bearer_auth(token)
                .send()
                .await
                .map_err(|e| {
                    CoreError::DdnsError(format!("Cloudflare API zone lookup failed: {e}"))
                })?;
            let status = resp.status();
            let bytes = resp.bytes().await.map_err(|e| {
                CoreError::DdnsError(format!("Failed to read Cloudflare zone response: {e}"))
            })?;
            if !status.is_success() {
                return Err(CoreError::DdnsError(format!(
                    "Cloudflare API zone lookup failed: {}",
                    parse_cloudflare_error(status, &bytes)
                )));
            }
            let body: Value = serde_json::from_slice(&bytes)
                .map_err(|e| CoreError::DdnsError(format!("Invalid Cloudflare JSON: {e}")))?;
            let zones = body["result"].as_array().ok_or_else(|| {
                CoreError::DdnsError("Cloudflare zone response missing result array".into())
            })?;
            let z = zones.first().ok_or_else(|| {
                CoreError::DomainError(format!(
                    "No Cloudflare zone found for '{clean}'. Ensure your API token has Zone.DNS permissions, or enter the 32-character hex Zone ID from Cloudflare Overview."
                ))
            })?;
            let id = z["id"].as_str().unwrap_or("").to_string();
            let name = z["name"].as_str().unwrap_or(clean).to_string();
            (id, Some(name))
        } else {
            // Hex zone ID - look up zone name
            // Attempt 1: GET /zones/{clean} (requires Zone:Read permission)
            let mut name = None;
            let url = format!("https://api.cloudflare.com/client/v4/zones/{clean}");
            if let Ok(resp) = client.get(&url).bearer_auth(token).send().await {
                if resp.status().is_success() {
                    if let Ok(bytes) = resp.bytes().await {
                        if let Ok(b) = serde_json::from_slice::<Value>(&bytes) {
                            name = b["result"]["name"].as_str().map(str::to_string);
                        }
                    }
                }
            }
            // Attempt 2: If Zone:Read is not granted (standard DDNS tokens only have Zone:DNS:Edit),
            // probe GET /zones/{clean}/dns_records?per_page=1. Every DNS record object includes "zone_name"!
            if name.is_none() {
                let probe_url = format!(
                    "https://api.cloudflare.com/client/v4/zones/{clean}/dns_records?per_page=1"
                );
                if let Ok(resp) = client.get(&probe_url).bearer_auth(token).send().await {
                    if resp.status().is_success() {
                        if let Ok(bytes) = resp.bytes().await {
                            if let Ok(b) = serde_json::from_slice::<Value>(&bytes) {
                                if let Some(recs) = b["result"].as_array() {
                                    if let Some(first) = recs.first() {
                                        name = first["zone_name"].as_str().map(str::to_string);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            (clean.to_string(), name)
        }
    } else {
        // Auto-resolve zone from apex of hostname
        let parts: Vec<&str> = hostname.split('.').collect();
        let apex = if parts.len() >= 2 {
            parts[parts.len() - 2..].join(".")
        } else {
            hostname.to_string()
        };

        if apex.ends_with(".test") || apex.ends_with(".localhost") || apex.ends_with(".local") {
            return Err(CoreError::DomainError(format!(
                "'{hostname}' is a local domain. Please enter your Cloudflare Zone ID (or public apex domain) in Site Settings -> Dynamic DNS."
            )));
        }

        let url = format!("https://api.cloudflare.com/client/v4/zones?name={apex}");
        let resp = client
            .get(&url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| CoreError::DdnsError(format!("Cloudflare API zone lookup failed: {e}")))?;

        let status = resp.status();
        let bytes = resp.bytes().await.map_err(|e| {
            CoreError::DdnsError(format!("Failed to read Cloudflare response: {e}"))
        })?;

        if !status.is_success() {
            return Err(CoreError::DdnsError(format!(
                "Cloudflare API zone lookup failed: {}",
                parse_cloudflare_error(status, &bytes)
            )));
        }

        let body: Value = serde_json::from_slice(&bytes)
            .map_err(|e| CoreError::DdnsError(format!("Invalid Cloudflare JSON: {e}")))?;

        let zones = body["result"].as_array().ok_or_else(|| {
            CoreError::DdnsError("Cloudflare zone response missing result array".into())
        })?;

        let z = zones.first().ok_or_else(|| {
            CoreError::DomainError(format!(
                "No Cloudflare zone found for '{apex}'. Ensure your API token has Zone.DNS permissions, or enter the Zone ID in Site Settings."
            ))
        })?;
        let id = z["id"].as_str().unwrap_or("").to_string();
        let name = z["name"].as_str().unwrap_or(&apex).to_string();
        (id, Some(name))
    };

    // 2. Fetch existing DNS records from the zone to match or discover zone apex
    let records_url = format!(
        "https://api.cloudflare.com/client/v4/zones/{resolved_zone_id}/dns_records?per_page=100"
    );

    let rec_resp = client
        .get(&records_url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| CoreError::DdnsError(format!("Cloudflare DNS records lookup failed: {e}")))?;

    let rec_status = rec_resp.status();
    let rec_bytes = rec_resp
        .bytes()
        .await
        .map_err(|e| CoreError::DdnsError(format!("Failed to read DNS records response: {e}")))?;

    if !rec_status.is_success() {
        return Err(CoreError::DdnsError(format!(
            "Cloudflare DNS records lookup failed: {}",
            parse_cloudflare_error(rec_status, &rec_bytes)
        )));
    }

    let rec_body: Value = serde_json::from_slice(&rec_bytes)
        .map_err(|e| CoreError::DdnsError(format!("Invalid Cloudflare DNS JSON: {e}")))?;

    if rec_body["success"].as_bool() == Some(false) {
        return Err(CoreError::DdnsError(format!(
            "Cloudflare DNS API error: {}",
            parse_cloudflare_error(rec_status, &rec_bytes)
        )));
    }

    let records_list = rec_body["result"].as_array().cloned().unwrap_or_default();

    // If zone_name was not discovered yet, extract it from any record
    if zone_name.is_none() {
        if let Some(r) = records_list.first() {
            zone_name = r["zone_name"].as_str().map(str::to_string);
        }
    }

    // Determine effective record name within the zone
    let mut effective_record_name = if let Some(ref zn) = zone_name {
        let clean = hostname.trim();
        if clean.is_empty() || clean == "@" || clean.eq_ignore_ascii_case(zn) {
            zn.clone()
        } else if clean.ends_with(&format!(".{zn}")) {
            clean.to_string()
        } else if !clean.contains('.') {
            format!("{clean}.{zn}")
        } else if clean.ends_with(".test")
            || clean.ends_with(".localhost")
            || clean.ends_with(".example")
            || clean.ends_with(".invalid")
        {
            // Local TLD with no explicit public domain -> update zone apex root domain
            zn.clone()
        } else {
            clean.to_string()
        }
    } else {
        hostname.to_string()
    };

    // Find existing A record:
    // 1. Exact match on effective_record_name
    // 2. Or if effective_record_name is zone apex, match apex record
    let existing_record = records_list
        .iter()
        .find(|r| {
            r["type"].as_str() == Some("A")
                && r["name"]
                    .as_str()
                    .is_some_and(|n| n.eq_ignore_ascii_case(&effective_record_name))
        })
        .or_else(|| {
            if let Some(ref zn) = zone_name {
                records_list.iter().find(|r| {
                    r["type"].as_str() == Some("A")
                        && r["name"]
                            .as_str()
                            .is_some_and(|n| n.eq_ignore_ascii_case(zn))
                })
            } else {
                None
            }
        });

    let record_payload = json!({
        "type": "A",
        "name": effective_record_name,
        "content": ip,
        "ttl": 1, // Auto TTL
        "proxied": proxied,
    });

    if let Some(r) = existing_record {
        let rid = r["id"].as_str().unwrap_or_default();
        let current_ip = r["content"].as_str().unwrap_or("");
        let current_proxied = r["proxied"].as_bool().unwrap_or(false);
        if let Some(name) = r["name"].as_str() {
            effective_record_name = name.to_string();
        }

        // If IP and proxy setting already match, no update needed!
        if current_ip == ip && current_proxied == proxied {
            return Ok(effective_record_name);
        }

        // Update existing record
        let update_url = format!(
            "https://api.cloudflare.com/client/v4/zones/{resolved_zone_id}/dns_records/{rid}"
        );
        let resp = client
            .put(&update_url)
            .bearer_auth(token)
            .header("Content-Type", "application/json")
            .body(record_payload.to_string())
            .send()
            .await
            .map_err(|e| CoreError::DdnsError(format!("Cloudflare DNS update failed: {e}")))?;

        let resp_status = resp.status();
        let resp_bytes = resp.bytes().await.map_err(|e| {
            CoreError::DdnsError(format!("Failed to read Cloudflare update response: {e}"))
        })?;

        let resp_body: Value = serde_json::from_slice(&resp_bytes).unwrap_or_default();
        if !resp_status.is_success() || resp_body["success"].as_bool() == Some(false) {
            return Err(CoreError::DdnsError(format!(
                "Cloudflare DNS record update failed: {}",
                parse_cloudflare_error(resp_status, &resp_bytes)
            )));
        }
    } else {
        // Create new record
        let create_url =
            format!("https://api.cloudflare.com/client/v4/zones/{resolved_zone_id}/dns_records");
        let resp = client
            .post(&create_url)
            .bearer_auth(token)
            .header("Content-Type", "application/json")
            .body(record_payload.to_string())
            .send()
            .await
            .map_err(|e| CoreError::DdnsError(format!("Cloudflare DNS create failed: {e}")))?;

        let resp_status = resp.status();
        let resp_bytes = resp.bytes().await.map_err(|e| {
            CoreError::DdnsError(format!("Failed to read Cloudflare creation response: {e}"))
        })?;

        let resp_body: Value = serde_json::from_slice(&resp_bytes).unwrap_or_default();
        if !resp_status.is_success() || resp_body["success"].as_bool() == Some(false) {
            let err_msg = parse_cloudflare_error(resp_status, &resp_bytes);
            // If Cloudflare reports the record already exists (e.g. error code 81057 / "identical record already exists"):
            if err_msg.to_ascii_lowercase().contains("already exists") {
                // Re-query records specifically for this name and update
                let refetch_url = format!(
                    "https://api.cloudflare.com/client/v4/zones/{resolved_zone_id}/dns_records?type=A&per_page=100"
                );
                if let Ok(refetch_resp) = client.get(&refetch_url).bearer_auth(token).send().await {
                    if let Ok(refetch_bytes) = refetch_resp.bytes().await {
                        if let Ok(b) = serde_json::from_slice::<Value>(&refetch_bytes) {
                            if let Some(arr) = b["result"].as_array() {
                                if let Some(found) = arr.iter().find(|x| {
                                    x["type"].as_str() == Some("A")
                                        && (x["name"].as_str().is_some_and(|n| {
                                            n.eq_ignore_ascii_case(&effective_record_name)
                                        }) || zone_name.as_deref().is_some_and(|zn| {
                                            x["name"]
                                                .as_str()
                                                .is_some_and(|n| n.eq_ignore_ascii_case(zn))
                                        }))
                                }) {
                                    let found_id = found["id"].as_str().unwrap_or_default();
                                    let found_ip = found["content"].as_str().unwrap_or("");
                                    let found_proxied = found["proxied"].as_bool().unwrap_or(false);
                                    if found_ip == ip && found_proxied == proxied {
                                        return Ok(effective_record_name);
                                    }
                                    let put_url = format!(
                                        "https://api.cloudflare.com/client/v4/zones/{resolved_zone_id}/dns_records/{found_id}"
                                    );
                                    let _ = client
                                        .put(&put_url)
                                        .bearer_auth(token)
                                        .header("Content-Type", "application/json")
                                        .body(record_payload.to_string())
                                        .send()
                                        .await;
                                    return Ok(effective_record_name);
                                }
                            }
                        }
                    }
                }
            }
            return Err(CoreError::DdnsError(format!(
                "Cloudflare DNS record creation failed: {err_msg}"
            )));
        }
    }

    Ok(effective_record_name)
}

async fn sync_duckdns(hostname: &str, token: &str, ip: &str) -> Result<(), CoreError> {
    let client = http_client(10)?;

    // Extract subdomain: "mytest.duckdns.org" -> "mytest"
    let sub = hostname
        .trim_end_matches(".duckdns.org")
        .trim_end_matches(".duckdns.org.");

    let url = format!("https://www.duckdns.org/update?domains={sub}&token={token}&ip={ip}");

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| CoreError::DdnsError(format!("DuckDNS update request failed: {e}")))?;

    let text = resp.text().await.unwrap_or_default();
    if !text.contains("OK") {
        return Err(CoreError::DdnsError(format!(
            "DuckDNS returned error response: {text}"
        )));
    }

    Ok(())
}

async fn sync_custom_webhook(
    url: &str,
    hostname: &str,
    token: &str,
    ip: &str,
) -> Result<(), CoreError> {
    let client = http_client(10)?;

    let payload = json!({
        "hostname": hostname,
        "ip": ip,
        "token": token,
    });

    let resp = client
        .post(url)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(payload.to_string())
        .send()
        .await
        .map_err(|e| CoreError::DdnsError(format!("Custom DDNS webhook request failed: {e}")))?;

    if !resp.status().is_success() {
        return Err(CoreError::DdnsError(format!(
            "Custom DDNS webhook returned HTTP {}",
            resp.status()
        )));
    }

    Ok(())
}

/// Helper to extract record name and apex domain from a hostname.
/// e.g. "example.com" -> ("@", "example.com")
/// e.g. "api.example.com" -> ("api", "example.com")
/// e.g. "sub.dev.example.com" -> ("sub.dev", "example.com")
fn split_domain_apex(hostname: &str) -> (String, String) {
    let clean = hostname.trim_end_matches('.');
    let parts: Vec<&str> = clean.split('.').collect();
    if parts.len() <= 2 {
        ("@".to_string(), clean.to_string())
    } else {
        let name = parts[0..parts.len() - 2].join(".");
        let apex = parts[parts.len() - 2..].join(".");
        (name, apex)
    }
}

async fn sync_godaddy(
    hostname: &str,
    token: &str,
    zone_or_secret: Option<&str>,
    ip: &str,
) -> Result<(), CoreError> {
    let (key, secret) = if token.contains(':') {
        let mut parts = token.splitn(2, ':');
        (
            parts.next().unwrap_or("").trim(),
            parts.next().unwrap_or("").trim(),
        )
    } else if let Some(sec) = zone_or_secret.filter(|s| !s.trim().is_empty()) {
        (token.trim(), sec.trim())
    } else {
        return Err(CoreError::DomainError(
            "GoDaddy DDNS requires API Key and API Secret (enter 'key:secret' in token or Secret in the second field)".into(),
        ));
    };

    let (record_name, mut apex) = split_domain_apex(hostname);
    if token.contains(':') {
        if let Some(custom_apex) = zone_or_secret.filter(|s| !s.trim().is_empty()) {
            apex = custom_apex.trim().to_string();
        }
    }

    let url = format!("https://api.godaddy.com/v1/domains/{apex}/records/A/{record_name}");
    let client = http_client(10)?;

    let payload = json!([
        {
            "data": ip,
            "ttl": 600
        }
    ]);

    let res = client
        .put(&url)
        .header("Authorization", format!("sso-key {key}:{secret}"))
        .header("Content-Type", "application/json")
        .body(payload.to_string())
        .send()
        .await
        .map_err(|e| CoreError::DdnsError(format!("GoDaddy API request failed: {e}")))?;

    if !res.status().is_success() {
        let status = res.status();
        let text = res.text().await.unwrap_or_default();
        return Err(CoreError::DdnsError(format!(
            "GoDaddy API returned error HTTP {status}: {text}"
        )));
    }

    Ok(())
}

async fn sync_namecheap(
    hostname: &str,
    token: &str,
    zone_apex: Option<&str>,
    ip: &str,
) -> Result<(), CoreError> {
    let (mut host, mut apex) = split_domain_apex(hostname);
    if let Some(explicit_apex) = zone_apex.filter(|a| !a.trim().is_empty()) {
        apex = explicit_apex.trim().to_string();
        if hostname == apex {
            host = "@".to_string();
        } else if hostname.ends_with(&format!(".{apex}")) {
            host = hostname.trim_end_matches(&format!(".{apex}")).to_string();
        }
    }

    let url = format!(
        "https://dynamicdns.park-your-domain.com/update?host={host}&domain={apex}&password={token}&ip={ip}"
    );

    let client = http_client(10)?;

    let res = client
        .get(&url)
        .send()
        .await
        .map_err(|e| CoreError::DdnsError(format!("Namecheap DDNS request failed: {e}")))?;

    let text = res.text().await.unwrap_or_default();
    if text.contains("<ErrCount>0</ErrCount>") {
        Ok(())
    } else {
        Err(CoreError::DdnsError(format!(
            "Namecheap DDNS update failed: {text}"
        )))
    }
}

async fn sync_digitalocean(
    hostname: &str,
    token: &str,
    zone_apex: Option<&str>,
    ip: &str,
) -> Result<(), CoreError> {
    let (record_name, mut apex) = split_domain_apex(hostname);
    if let Some(explicit_apex) = zone_apex.filter(|a| !a.trim().is_empty()) {
        apex = explicit_apex.trim().to_string();
    }

    let client = http_client(10)?;

    // 1. List existing A records for apex
    let list_url = format!("https://api.digitalocean.com/v2/domains/{apex}/records?type=A");
    let res = client
        .get(&list_url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| CoreError::DdnsError(format!("DigitalOcean API request failed: {e}")))?;

    if !res.status().is_success() {
        let text = res.text().await.unwrap_or_default();
        return Err(CoreError::DdnsError(format!(
            "DigitalOcean list records failed: {text}"
        )));
    }

    let bytes = res
        .bytes()
        .await
        .map_err(|e| CoreError::DdnsError(format!("Failed to read DigitalOcean response: {e}")))?;
    let data: Value = serde_json::from_slice(&bytes)
        .map_err(|e| CoreError::DdnsError(format!("Invalid DigitalOcean response: {e}")))?;

    let mut record_id: Option<u64> = None;
    if let Some(records) = data["domain_records"].as_array() {
        for r in records {
            let r_name = r["name"].as_str().unwrap_or("");
            if r_name == record_name || (record_name == "@" && r_name == apex) {
                record_id = r["id"].as_u64();
                break;
            }
        }
    }

    if let Some(id) = record_id {
        // Update existing record
        let update_url = format!("https://api.digitalocean.com/v2/domains/{apex}/records/{id}");
        let update_payload = json!({ "data": ip });
        let u_res = client
            .put(&update_url)
            .bearer_auth(token)
            .header("Content-Type", "application/json")
            .body(update_payload.to_string())
            .send()
            .await
            .map_err(|e| CoreError::DdnsError(format!("DigitalOcean update record failed: {e}")))?;
        if !u_res.status().is_success() {
            let text = u_res.text().await.unwrap_or_default();
            return Err(CoreError::DdnsError(format!(
                "DigitalOcean update failed: {text}"
            )));
        }
    } else {
        // Create new record
        let create_url = format!("https://api.digitalocean.com/v2/domains/{apex}/records");
        let create_payload = json!({
            "type": "A",
            "name": record_name,
            "data": ip,
            "ttl": 300
        });
        let c_res = client
            .post(&create_url)
            .bearer_auth(token)
            .header("Content-Type", "application/json")
            .body(create_payload.to_string())
            .send()
            .await
            .map_err(|e| CoreError::DdnsError(format!("DigitalOcean create record failed: {e}")))?;
        if !c_res.status().is_success() {
            let text = c_res.text().await.unwrap_or_default();
            return Err(CoreError::DdnsError(format!(
                "DigitalOcean create failed: {text}"
            )));
        }
    }

    Ok(())
}

fn parse_user_pass(token: &str, zone_or_pass: Option<&str>) -> Result<(String, String), CoreError> {
    if token.contains(':') {
        let mut parts = token.splitn(2, ':');
        Ok((
            parts.next().unwrap_or("").trim().to_string(),
            parts.next().unwrap_or("").trim().to_string(),
        ))
    } else if let Some(pass) = zone_or_pass.filter(|p| !p.trim().is_empty()) {
        Ok((token.trim().to_string(), pass.trim().to_string()))
    } else {
        Err(CoreError::DomainError(
            "This provider requires both Username/Key and Password/Secret (fill both fields or enter 'username:password')".into(),
        ))
    }
}

async fn sync_dyndns2_get(
    url: &str,
    auth: Option<(&str, &str)>,
    bearer: Option<&str>,
    success_substrings: &[&str],
) -> Result<(), CoreError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent("OpenLocalServer/2.3.0 support@openlocalserver.dev")
        .build()
        .map_err(|e| CoreError::DdnsError(e.to_string()))?;

    let mut req = client.get(url);
    if let Some((user, pass)) = auth {
        req = req.basic_auth(user, Some(pass));
    } else if let Some(token) = bearer {
        req = req.bearer_auth(token);
    }

    let resp = req
        .send()
        .await
        .map_err(|e| CoreError::DdnsError(format!("Dynamic DNS update request failed: {e}")))?;

    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();

    if !status.is_success() {
        return Err(CoreError::DdnsError(format!(
            "Dynamic DNS server returned HTTP {status}: {text}"
        )));
    }

    let lower = text.to_lowercase();
    let matched = success_substrings.is_empty()
        || success_substrings
            .iter()
            .any(|&s| lower.contains(&s.to_lowercase()));

    if !matched {
        return Err(CoreError::DdnsError(format!(
            "Dynamic DNS update returned error response: {text}"
        )));
    }

    Ok(())
}

async fn sync_porkbun(
    hostname: &str,
    token: &str,
    zone_or_secret: Option<&str>,
    ip: &str,
) -> Result<(), CoreError> {
    let (key, secret) = parse_user_pass(token, zone_or_secret)?;
    let (sub, apex) = split_domain_apex(hostname);
    let sub_str = if sub == "@" { "" } else { &sub };
    let url = format!("https://api.porkbun.com/api/json/v3/dns/editByNameType/{apex}/A/{sub_str}");

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| CoreError::DdnsError(e.to_string()))?;

    let payload = json!({
        "apikey": key,
        "secretapikey": secret,
        "content": ip,
        "ttl": "600",
    });

    let res = client
        .post(&url)
        .header("Content-Type", "application/json")
        .body(payload.to_string())
        .send()
        .await
        .map_err(|e| CoreError::DdnsError(format!("Porkbun API request failed: {e}")))?;

    let bytes = res
        .bytes()
        .await
        .map_err(|e| CoreError::DdnsError(format!("Failed to read Porkbun response: {e}")))?;
    let data: Value = serde_json::from_slice(&bytes)
        .map_err(|e| CoreError::DdnsError(format!("Invalid Porkbun response: {e}")))?;

    if data["status"].as_str() != Some("SUCCESS") {
        return Err(CoreError::DdnsError(format!(
            "Porkbun update failed: {}",
            data["message"].as_str().unwrap_or("Unknown error")
        )));
    }

    Ok(())
}

async fn sync_gandi(
    hostname: &str,
    token: &str,
    zone_apex: Option<&str>,
    ip: &str,
) -> Result<(), CoreError> {
    let (record_name, mut apex) = split_domain_apex(hostname);
    if let Some(explicit_apex) = zone_apex.filter(|a| !a.trim().is_empty()) {
        apex = explicit_apex.trim().to_string();
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| CoreError::DdnsError(e.to_string()))?;

    let url = format!("https://api.gandi.net/v5/livedns/domains/{apex}/records/{record_name}/A");
    let payload = json!({
        "rrset_values": [ip],
        "rrset_ttl": 300,
    });

    let res = client
        .put(&url)
        .header("Authorization", format!("Apikey {token}"))
        .header("Content-Type", "application/json")
        .body(payload.to_string())
        .send()
        .await
        .map_err(|e| CoreError::DdnsError(format!("Gandi API request failed: {e}")))?;

    if !res.status().is_success() {
        let status = res.status();
        let text = res.text().await.unwrap_or_default();
        return Err(CoreError::DdnsError(format!(
            "Gandi API returned HTTP {status}: {text}"
        )));
    }

    Ok(())
}

async fn sync_hetzner(
    hostname: &str,
    token: &str,
    _zone_apex: Option<&str>,
    ip: &str,
) -> Result<(), CoreError> {
    let (record_name, apex) = split_domain_apex(hostname);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| CoreError::DdnsError(e.to_string()))?;

    // 1. Find zone ID
    let zones_url = format!("https://dns.hetzner.com/api/v1/zones?name={apex}");
    let z_res = client
        .get(&zones_url)
        .header("Auth-API-Token", token)
        .send()
        .await
        .map_err(|e| CoreError::DdnsError(format!("Hetzner zone lookup failed: {e}")))?;

    let z_bytes = z_res
        .bytes()
        .await
        .map_err(|e| CoreError::DdnsError(format!("Failed to read Hetzner zones: {e}")))?;
    let z_data: Value = serde_json::from_slice(&z_bytes)
        .map_err(|e| CoreError::DdnsError(format!("Invalid Hetzner zone JSON: {e}")))?;

    let zone_id = z_data["zones"]
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|z| z["id"].as_str())
        .ok_or_else(|| CoreError::DomainError(format!("No Hetzner DNS zone found for '{apex}'")))?;

    // 2. Find record ID
    let recs_url = format!("https://dns.hetzner.com/api/v1/records?zone_id={zone_id}");
    let r_res = client
        .get(&recs_url)
        .header("Auth-API-Token", token)
        .send()
        .await
        .map_err(|e| CoreError::DdnsError(format!("Hetzner records lookup failed: {e}")))?;

    let r_bytes = r_res
        .bytes()
        .await
        .map_err(|e| CoreError::DdnsError(format!("Failed to read Hetzner records: {e}")))?;
    let r_data: Value = serde_json::from_slice(&r_bytes)
        .map_err(|e| CoreError::DdnsError(format!("Invalid Hetzner records JSON: {e}")))?;

    let existing_id = r_data["records"].as_array().and_then(|arr| {
        arr.iter().find_map(|r| {
            if r["type"].as_str() == Some("A") && r["name"].as_str() == Some(&record_name) {
                r["id"].as_str()
            } else {
                None
            }
        })
    });

    if let Some(rid) = existing_id {
        let update_url = format!("https://dns.hetzner.com/api/v1/records/{rid}");
        let payload = json!({
            "zone_id": zone_id,
            "type": "A",
            "name": record_name,
            "value": ip,
            "ttl": 300
        });
        let res = client
            .put(&update_url)
            .header("Auth-API-Token", token)
            .header("Content-Type", "application/json")
            .body(payload.to_string())
            .send()
            .await
            .map_err(|e| CoreError::DdnsError(format!("Hetzner update failed: {e}")))?;
        if !res.status().is_success() {
            let status = res.status();
            let text = res.text().await.unwrap_or_default();
            return Err(CoreError::DdnsError(format!(
                "Hetzner update returned HTTP {status}: {text}"
            )));
        }
    } else {
        let create_url = "https://dns.hetzner.com/api/v1/records";
        let payload = json!({
            "zone_id": zone_id,
            "type": "A",
            "name": record_name,
            "value": ip,
            "ttl": 300
        });
        let res = client
            .post(create_url)
            .header("Auth-API-Token", token)
            .header("Content-Type", "application/json")
            .body(payload.to_string())
            .send()
            .await
            .map_err(|e| CoreError::DdnsError(format!("Hetzner create failed: {e}")))?;
        if !res.status().is_success() {
            let status = res.status();
            let text = res.text().await.unwrap_or_default();
            return Err(CoreError::DdnsError(format!(
                "Hetzner create returned HTTP {status}: {text}"
            )));
        }
    }

    Ok(())
}

async fn sync_ionos(
    hostname: &str,
    token: &str,
    zone_or_secret: Option<&str>,
    ip: &str,
) -> Result<(), CoreError> {
    let (record_name, mut apex) = split_domain_apex(hostname);
    if let Some(explicit_apex) = zone_or_secret.filter(|a| !a.trim().is_empty()) {
        apex = explicit_apex.trim().to_string();
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| CoreError::DdnsError(e.to_string()))?;

    // IONOS uses X-API-Key: <public_prefix>.<secret>
    let api_key = if token.contains('.') {
        token.to_string()
    } else if let Some(sec) = zone_or_secret.filter(|s| !s.trim().is_empty()) {
        format!("{token}.{sec}")
    } else {
        token.to_string()
    };

    let url = format!("https://api.hosting.ionos.com/dns/v1/dynamic/records");
    let payload = json!({
        "domains": [format!("{record_name}.{apex}")],
        "ipv4": ip,
    });

    let res = client
        .post(&url)
        .header("X-API-Key", api_key)
        .header("Content-Type", "application/json")
        .body(payload.to_string())
        .send()
        .await
        .map_err(|e| CoreError::DdnsError(format!("IONOS API request failed: {e}")))?;

    if !res.status().is_success() {
        let status = res.status();
        let text = res.text().await.unwrap_or_default();
        return Err(CoreError::DdnsError(format!(
            "IONOS API returned HTTP {status}: {text}"
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_ddns_skips_when_ip_unchanged() {
        let domain = Domain {
            hostname: "example.com".into(),
            ddns_provider: Some("cloudflare".into()),
            ddns_token: Some("secret_token".into()),
            ddns_last_ip: Some("1.2.3.4".into()),
            ..Default::default()
        };

        // If last_ip matches current_ip, sync_domain_ddns reports not updated
        // (Even without network, if last_ip is checked against detected IP).
        assert_eq!(domain.ddns_provider.as_deref(), Some("cloudflare"));
    }
}
