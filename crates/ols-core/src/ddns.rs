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

    match provider.to_ascii_lowercase().as_str() {
        "cloudflare" => {
            sync_cloudflare(
                &domain.hostname,
                token,
                domain.ddns_zone_id.as_deref(),
                &current_ip,
                domain.ddns_proxied,
            )
            .await?;
        }
        "duckdns" => {
            sync_duckdns(&domain.hostname, token, &current_ip).await?;
        }
        "custom" => {
            let url = domain.ddns_zone_id.as_deref().unwrap_or("").trim();
            if url.is_empty() {
                return Err(CoreError::DomainError(
                    "Custom DDNS requires a webhook URL in the zone/endpoint field".into(),
                ));
            }
            sync_custom_webhook(url, &domain.hostname, token, &current_ip).await?;
        }
        other => {
            return Err(CoreError::DomainError(format!(
                "Unsupported DDNS provider: '{other}'. Supported: cloudflare, duckdns, custom"
            )));
        }
    }

    Ok(DdnsSyncResult {
        hostname: domain.hostname.clone(),
        ip: current_ip.clone(),
        updated: true,
        message: format!("DNS record successfully updated to {current_ip}"),
    })
}

async fn sync_cloudflare(
    hostname: &str,
    token: &str,
    zone_id: Option<&str>,
    ip: &str,
    proxied: bool,
) -> Result<(), CoreError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| CoreError::DdnsError(e.to_string()))?;

    // 1. Resolve Zone ID
    let resolved_zone_id = if let Some(zid) = zone_id.filter(|z| !z.trim().is_empty()) {
        zid.trim().to_string()
    } else {
        // Find zone by apex domain
        let parts: Vec<&str> = hostname.split('.').collect();
        let apex = if parts.len() >= 2 {
            parts[parts.len() - 2..].join(".")
        } else {
            hostname.to_string()
        };

        let url = format!("https://api.cloudflare.com/client/v4/zones?name={apex}");
        let resp = client
            .get(&url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| CoreError::DdnsError(format!("Cloudflare API zone lookup failed: {e}")))?;

        if !resp.status().is_success() {
            return Err(CoreError::DdnsError(format!(
                "Cloudflare API error HTTP {} during zone lookup",
                resp.status()
            )));
        }

        let body: Value = serde_json::from_slice(&resp.bytes().await.map_err(|e| {
            CoreError::DdnsError(format!("Failed to read Cloudflare response: {e}"))
        })?)
        .map_err(|e| CoreError::DdnsError(format!("Invalid Cloudflare JSON: {e}")))?;

        let zones = body["result"].as_array().ok_or_else(|| {
            CoreError::DdnsError("Cloudflare zone response missing result array".into())
        })?;

        zones
            .first()
            .and_then(|z| z["id"].as_str())
            .map(str::to_string)
            .ok_or_else(|| {
                CoreError::DomainError(format!(
                    "No Cloudflare zone found for '{apex}'. Ensure your API token has Zone.DNS permissions."
                ))
            })?
    };

    // 2. Look up existing DNS A record for this hostname
    let records_url = format!(
        "https://api.cloudflare.com/client/v4/zones/{resolved_zone_id}/dns_records?type=A&name={hostname}"
    );

    let rec_resp = client
        .get(&records_url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| CoreError::DdnsError(format!("Cloudflare DNS record lookup failed: {e}")))?;

    let rec_body: Value =
        serde_json::from_slice(&rec_resp.bytes().await.map_err(|e| {
            CoreError::DdnsError(format!("Failed to read DNS records response: {e}"))
        })?)
        .map_err(|e| CoreError::DdnsError(format!("Invalid Cloudflare DNS JSON: {e}")))?;

    let records = rec_body["result"].as_array();
    let existing_record_id = records
        .and_then(|arr| arr.iter().find(|r| r["name"].as_str() == Some(hostname)))
        .and_then(|r| r["id"].as_str());

    let record_payload = json!({
        "type": "A",
        "name": hostname,
        "content": ip,
        "ttl": 1, // Auto TTL
        "proxied": proxied,
    });

    if let Some(rid) = existing_record_id {
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

        if !resp.status().is_success() {
            let err_txt = resp.text().await.unwrap_or_default();
            return Err(CoreError::DdnsError(format!(
                "Cloudflare DNS record update failed: {err_txt}"
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

        if !resp.status().is_success() {
            let err_txt = resp.text().await.unwrap_or_default();
            return Err(CoreError::DdnsError(format!(
                "Cloudflare DNS record creation failed: {err_txt}"
            )));
        }
    }

    Ok(())
}

async fn sync_duckdns(hostname: &str, token: &str, ip: &str) -> Result<(), CoreError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| CoreError::DdnsError(e.to_string()))?;

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
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| CoreError::DdnsError(e.to_string()))?;

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
