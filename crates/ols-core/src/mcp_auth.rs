//! MCP authentication config (§37): bearer token stored in the OS keyring, with optional
//! TTL-based expiration. The token itself never touches SQLite or a log line.
//!
//! `enabled` is a plain setting (SQLite) so it can flip on/off without a restart.
//! The token and its metadata live in the keyring via `crate::secrets`.

use std::time::{SystemTime, UNIX_EPOCH};

/// Everything the MCP server needs to decide whether a request carries a valid token.
#[derive(Debug, Clone)]
pub struct McpAuthConfig {
    /// Whether MCP authentication is required at all. Off = every request passes
    /// (backward compatible with existing installs).
    pub enabled: bool,
    /// The bearer token, or `None` if none has been configured yet.
    pub token: Option<String>,
    /// Unix ms when the token was created. `None` means "never expires".
    pub created_at_ms: Option<u64>,
    /// TTL in ms. `None` means no expiration.
    pub ttl_ms: Option<u64>,
}

impl McpAuthConfig {
    /// Reads the current auth state from settings + keyring. Never returns an error
    /// — a missing keyring entry just means "no token configured yet".
    pub fn load() -> Self {
        let enabled = crate::settings::SettingsService::load(&crate::paths::AppPaths::resolve())
            .ok()
            .and_then(|s| s.get("mcp.auth.enabled").cloned())
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let token = crate::secrets::get_secret("mcp.auth.token").ok().flatten();
        let created_at_ms = crate::secrets::get_secret("mcp.auth.token.created_at")
            .ok()
            .flatten()
            .and_then(|v| v.parse::<u64>().ok());
        let ttl_ms = crate::secrets::get_secret("mcp.auth.token.ttl_ms")
            .ok()
            .flatten()
            .and_then(|v| v.parse::<u64>().ok());

        Self {
            enabled,
            token,
            created_at_ms,
            ttl_ms,
        }
    }

    /// Whether the token has expired. Always false when `ttl_ms` is `None`.
    pub fn is_expired(&self) -> bool {
        match (self.created_at_ms, self.ttl_ms) {
            (Some(created), Some(ttl)) => {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);
                now.saturating_sub(created) > ttl
            }
            _ => false,
        }
    }

    /// Whether this config is ready to enforce: enabled, token present, not expired.
    pub fn is_ready(&self) -> bool {
        self.enabled && self.token.is_some() && !self.is_expired()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    #[test]
    fn never_expires_when_no_ttl() {
        let cfg = McpAuthConfig {
            enabled: true,
            token: Some("abc".into()),
            created_at_ms: Some(now_ms()),
            ttl_ms: None,
        };
        assert!(!cfg.is_expired());
        assert!(cfg.is_ready());
    }

    #[test]
    fn expired_when_ttl_passed() {
        let cfg = McpAuthConfig {
            enabled: true,
            token: Some("abc".into()),
            created_at_ms: Some(now_ms() - 200_000),
            ttl_ms: Some(60_000),
        };
        assert!(cfg.is_expired());
        assert!(!cfg.is_ready());
    }

    #[test]
    fn not_expired_when_ttl_not_passed() {
        let cfg = McpAuthConfig {
            enabled: true,
            token: Some("abc".into()),
            created_at_ms: Some(now_ms()),
            ttl_ms: Some(60_000),
        };
        assert!(!cfg.is_expired());
        assert!(cfg.is_ready());
    }

    #[test]
    fn disabled_is_not_ready() {
        let cfg = McpAuthConfig {
            enabled: false,
            token: Some("abc".into()),
            created_at_ms: Some(now_ms()),
            ttl_ms: Some(60_000),
        };
        assert!(!cfg.is_ready());
    }

    #[test]
    fn no_token_is_not_ready() {
        let cfg = McpAuthConfig {
            enabled: true,
            token: None,
            created_at_ms: None,
            ttl_ms: None,
        };
        assert!(!cfg.is_ready());
    }
}
