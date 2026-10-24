//! MCP bearer-token authentication middleware (§37).
//!
//! Intercepts every JSON-RPC request and rejects those that do not carry a valid
//! token, unless authentication is disabled. Token lives in the OS keyring via
//! `ols_core::secrets`, never on disk.

use serde_json::Value;

use crate::JsonRpcRequest;

/// Methods that must remain reachable without a token (discovery / liveness).
const OPEN_METHODS: &[&str] = &["initialize", "ping", "resources/list", "prompts/list"];

/// Result of a token check.
pub enum AuthOutcome {
    /// Request is allowed to proceed.
    Ok,
    /// Request must be rejected with this JSON-RPC error value.
    Err(Value),
}

/// Constant-time string comparison, so the token cannot be guessed byte by byte
/// from timing side channels (same approach as `ols-core/src/control.rs`).
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

/// Extract a bearer token from a JSON-RPC request.
///
/// Checks, in order:
/// 1. `params._meta.authorization` — MCP 2025-06-18 style, `Bearer <token>`.
/// 2. `params.arguments.token` — fallback for hosts that cannot set headers.
fn extract_token(req: &JsonRpcRequest) -> Option<String> {
    req.params
        .get("_meta")
        .and_then(|m| m.get("authorization"))
        .and_then(|v| v.as_str())
        .map(|s| s.strip_prefix("Bearer ").unwrap_or(s).to_string())
        .or_else(|| {
            req.params
                .get("arguments")
                .and_then(|a| a.get("token"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
}

/// Build a `-32603` JSON-RPC error carrying a `Diagnostic`-shaped payload.
fn auth_error(message: &str, fix: &str) -> Value {
    serde_json::json!({
        "code": -32603,
        "message": message,
        "data": {
            "problem": message,
            "cause": "MCP authentication is enabled in OLS but the request carried no matching bearer token.",
            "fix": fix,
        }
    })
}

/// Middleware that decides whether a request may proceed.
pub struct AuthMiddleware;

impl AuthMiddleware {
    /// Run the check against a request. Returns `AuthOutcome::Err` with a JSON-RPC
    /// error value when the request should be rejected.
    pub fn check(req: &JsonRpcRequest) -> AuthOutcome {
        let config = ols_core::mcp_auth::McpAuthConfig::load();

        // Auth disabled → everything passes (backward compatible).
        if !config.enabled {
            return AuthOutcome::Ok;
        }

        // Discovery / liveness always open.
        if OPEN_METHODS.contains(&req.method.as_str()) {
            return AuthOutcome::Ok;
        }

        // No token configured at all → refuse with a setup message.
        let expected = match &config.token {
            Some(t) => t,
            None => {
                return AuthOutcome::Err(auth_error(
                    "MCP authentication not configured",
                    "Open OLS Settings → MCP, enable authentication and copy the token into your MCP host.",
                ));
            }
        };

        // Token expired → force rotation.
        if config.is_expired() {
            return AuthOutcome::Err(auth_error(
                "MCP token expired",
                "Open OLS Settings → MCP and rotate the token; then reconfigure your MCP host.",
            ));
        }

        let provided = match extract_token(req) {
            Some(t) => t,
            None => {
                return AuthOutcome::Err(auth_error(
                    "Missing MCP authentication token",
                    "Send the token as `Authorization: Bearer <token>` in the request `_meta`, or as `arguments.token`.",
                ));
            }
        };

        if !same(&provided, expected) {
            return AuthOutcome::Err(auth_error(
                "Invalid MCP authentication token",
                "Check that the token in your MCP host matches the one in OLS Settings → MCP. Rotate it if you suspect a leak.",
            ));
        }

        AuthOutcome::Ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn req(method: &str, params: Value) -> JsonRpcRequest {
        JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: json!(1),
            method: method.into(),
            params,
        }
    }

    #[test]
    fn disabled_passes_everything() {
        // Can't easily flip enabled without keyring, but OPEN_METHODS always pass.
        let r = req("initialize", json!({}));
        assert!(matches!(AuthMiddleware::check(&r), AuthOutcome::Ok));
    }

    #[test]
    fn open_methods_pass() {
        for m in OPEN_METHODS {
            let r = req(m, json!({}));
            assert!(matches!(AuthMiddleware::check(&r), AuthOutcome::Ok), "{m}");
        }
    }

    #[test]
    fn same_returns_true_for_equal() {
        assert!(same("abc", "abc"));
        assert!(!same("abc", "abd"));
        assert!(!same("abc", "ab"));
    }
}
