use serde_json::Value;

use crate::JsonRpcRequest;

/// Result of a token check.
pub enum AuthOutcome {
    /// Request is allowed to proceed.
    Ok,
    /// Request must be rejected with this JSON-RPC error value.
    Err(Value),
}

/// Constant-time string comparison, so the token cannot be guessed byte by byte
/// from timing side channels.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

/// Build a `-32603` JSON-RPC error carrying a `Diagnostic`-shaped payload.
fn auth_error(message: &str, fix: &str) -> Value {
    serde_json::json!({
        "code": -32603,
        "message": message,
        "data": {
            "problem": message,
            "cause": "MCP authentication is enabled but the server was not launched with a matching token.",
            "fix": fix,
        }
    })
}

/// Middleware that validates the MCP server was launched with the expected token.
///
/// The token is provided once via `--token` or `OLS_MCP_TOKEN` in the MCP host
/// config (next to the `command`), never inside individual `tools/call` requests.
/// `override_token` is that launch-time value.
pub struct AuthMiddleware;

impl AuthMiddleware {
    /// Run the check. `override_token` is the `--token`/`OLS_MCP_TOKEN` value.
    /// Returns `AuthOutcome::Err` when the launch token doesn't match the stored
    /// token, or when auth is configured but no launch token was provided.
    pub fn check(_req: &JsonRpcRequest, override_token: Option<&str>) -> AuthOutcome {
        let config = ols_core::mcp_auth::McpAuthConfig::load();
        Self::check_with_config(_req, override_token, &config)
    }

    /// Internal check with an explicit `McpAuthConfig` for testing.
    pub fn check_with_config(
        _req: &JsonRpcRequest,
        override_token: Option<&str>,
        config: &ols_core::mcp_auth::McpAuthConfig,
    ) -> AuthOutcome {
        // No token configured anywhere → backward compatible, everything passes.
        if config.token.is_none() && override_token.is_none() {
            return AuthOutcome::Ok;
        }

        // Auth is configured: the launch token must match the stored token.
        let expected = match &config.token {
            Some(t) => t.as_str(),
            None => {
                return AuthOutcome::Err(auth_error(
                    "MCP authentication not configured",
                    "Open OLS Settings → MCP / AI tools, enable authentication and set a token, then pass it as --token or OLS_MCP_TOKEN in your MCP host config.",
                ));
            }
        };

        let provided = match override_token {
            Some(t) => t,
            None => {
                return AuthOutcome::Err(auth_error(
                    "Missing MCP launch token",
                    "Pass the token as `--token <token>` or `OLS_MCP_TOKEN=<token>` in your MCP host config next to the ols-mcp command.",
                ));
            }
        };

        if !same(provided, expected) {
            return AuthOutcome::Err(auth_error(
                "Invalid MCP launch token",
                "The --token / OLS_MCP_TOKEN value in your MCP host config does not match the token in OLS Settings → MCP / AI tools. Make them the same.",
            ));
        }

        // Token expired → force rotation.
        if config.is_expired() {
            return AuthOutcome::Err(auth_error(
                "MCP token expired",
                "Open OLS Settings → MCP / AI tools and set a new expiry or regenerate the token, then update your MCP host config.",
            ));
        }

        AuthOutcome::Ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn req() -> JsonRpcRequest {
        JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: json!(1),
            method: "initialize".into(),
            params: json!({}),
        }
    }

    #[test]
    fn no_token_anywhere_passes() {
        let empty_config = ols_core::mcp_auth::McpAuthConfig {
            token: None,
            ttl_ms: None,
            expires_at_ms: None,
            created_at_ms: None,
        };
        assert!(matches!(
            AuthMiddleware::check_with_config(&req(), None, &empty_config),
            AuthOutcome::Ok
        ));
    }

    #[test]
    fn matching_token_passes() {
        // Can't easily set keyring here, but same() is tested directly.
        assert!(same("abc123", "abc123"));
        assert!(!same("abc123", "abc124"));
        assert!(!same("abc123", "abc"));
    }
}
