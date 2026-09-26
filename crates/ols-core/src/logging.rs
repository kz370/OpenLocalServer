//! Structured logging (§118) with secret redaction (§141). Full tracing-Layer-based
//! redaction of live spans lands with the process supervisor (Stage 2); today this module
//! provides `redact_value`, the primitive every log call site is expected to run secrets through.

use std::path::Path;

use tracing_subscriber::EnvFilter;

/// Key name fragments that mark a value as sensitive. Case-insensitive substring match.
const SECRET_KEY_MARKERS: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "token",
    "api_key",
    "apikey",
    "private_key",
    "credential",
];

/// Redact `value` if `key` looks like it holds a secret. Non-secret values pass through unchanged.
pub fn redact_value(key: &str, value: &str) -> String {
    let key_lower = key.to_ascii_lowercase();
    if SECRET_KEY_MARKERS.iter().any(|m| key_lower.contains(m)) {
        "[redacted]".to_string()
    } else {
        value.to_string()
    }
}

/// Initialize the global tracing subscriber: JSON to a rotating file, pretty to stderr in debug.
pub fn init(log_dir: &Path) {
    let _ = std::fs::create_dir_all(log_dir);
    let file_appender = tracing_appender::rolling::daily(log_dir, "ols-core.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    // Leak the guard: it must live for the process lifetime to keep flushing.
    Box::leak(Box::new(guard));

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .json()
        .with_env_filter(filter)
        .with_writer(non_blocking)
        .init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_secret_shaped_keys() {
        assert_eq!(redact_value("db_password", "hunter2"), "[redacted]");
        assert_eq!(redact_value("MAIL_TOKEN", "abc123"), "[redacted]");
        assert_eq!(redact_value("tunnel_secret", "xyz"), "[redacted]");
    }

    #[test]
    fn leaves_ordinary_keys_untouched() {
        assert_eq!(redact_value("php_version", "8.4"), "8.4");
        assert_eq!(redact_value("domain", "shop.test"), "shop.test");
    }
}
