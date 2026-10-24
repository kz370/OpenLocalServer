//! MCP authentication config (§37): a single bearer token stored in the OS keyring,
//! with flexible expiry — either a TTL in hours or an absolute datetime. The token
//! lives in the MCP host config (--token / OLS_MCP_TOKEN), not inside every tools/call.

use std::time::{SystemTime, UNIX_EPOCH};

/// Everything the MCP server needs to decide whether a request carries a valid token.
#[derive(Debug, Clone)]
pub struct McpAuthConfig {
    /// The bearer token, or `None` if none has been configured yet.
    pub token: Option<String>,
    /// TTL in ms from creation. `None` = no TTL-based expiry.
    pub ttl_ms: Option<u64>,
    /// Absolute expiry as unix ms. `None` = no absolute expiry.
    pub expires_at_ms: Option<u64>,
    /// Unix ms when the token was created (needed for TTL math).
    pub created_at_ms: Option<u64>,
}

impl McpAuthConfig {
    /// Reads the current auth state from the keyring. Never returns an error
    /// — a missing keyring entry just means "no token configured yet".
    pub fn load() -> Self {
        let token = crate::secrets::get_secret("mcp.auth.token").ok().flatten();
        let ttl_ms = crate::secrets::get_secret("mcp.auth.token.ttl_ms")
            .ok()
            .flatten()
            .and_then(|v| v.parse::<u64>().ok());
        let expires_at_ms = crate::secrets::get_secret("mcp.auth.token.expires_at")
            .ok()
            .flatten()
            .and_then(|v| v.parse::<u64>().ok());
        let created_at_ms = crate::secrets::get_secret("mcp.auth.token.created_at")
            .ok()
            .flatten()
            .and_then(|v| v.parse::<u64>().ok());

        Self {
            token,
            ttl_ms,
            expires_at_ms,
            created_at_ms,
        }
    }

    /// Whether the token has expired. Checks both TTL (from created_at) and absolute
    /// expiry. Always false when neither is set.
    pub fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        // Absolute expiry takes priority if set.
        if let Some(expires) = self.expires_at_ms {
            if now >= expires {
                return true;
            }
        }

        // TTL expiry: created_at + ttl_ms.
        if let (Some(created), Some(ttl)) = (self.created_at_ms, self.ttl_ms) {
            if now.saturating_sub(created) >= ttl {
                return true;
            }
        }

        false
    }

    /// Whether this config is ready to enforce: token present, not expired.
    pub fn is_ready(&self) -> bool {
        self.token.is_some() && !self.is_expired()
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
    fn never_expires_when_nothing_set() {
        let cfg = McpAuthConfig {
            token: Some("abc".into()),
            ttl_ms: None,
            expires_at_ms: None,
            created_at_ms: None,
        };
        assert!(!cfg.is_expired());
        assert!(cfg.is_ready());
    }

    #[test]
    fn expired_when_ttl_passed() {
        let cfg = McpAuthConfig {
            token: Some("abc".into()),
            ttl_ms: Some(60_000),
            expires_at_ms: None,
            created_at_ms: Some(now_ms() - 120_000),
        };
        assert!(cfg.is_expired());
        assert!(!cfg.is_ready());
    }

    #[test]
    fn not_expired_when_ttl_not_passed() {
        let cfg = McpAuthConfig {
            token: Some("abc".into()),
            ttl_ms: Some(86_400_000),
            expires_at_ms: None,
            created_at_ms: Some(now_ms()),
        };
        assert!(!cfg.is_expired());
        assert!(cfg.is_ready());
    }

    #[test]
    fn expired_when_absolute_passed() {
        let cfg = McpAuthConfig {
            token: Some("abc".into()),
            ttl_ms: None,
            expires_at_ms: Some(now_ms() - 1000),
            created_at_ms: None,
        };
        assert!(cfg.is_expired());
        assert!(!cfg.is_ready());
    }

    #[test]
    fn not_expired_when_absolute_future() {
        let cfg = McpAuthConfig {
            token: Some("abc".into()),
            ttl_ms: None,
            expires_at_ms: Some(now_ms() + 86_400_000),
            created_at_ms: None,
        };
        assert!(!cfg.is_expired());
        assert!(cfg.is_ready());
    }

    #[test]
    fn no_token_is_not_ready() {
        let cfg = McpAuthConfig {
            token: None,
            ttl_ms: None,
            expires_at_ms: None,
            created_at_ms: None,
        };
        assert!(!cfg.is_ready());
    }
}
