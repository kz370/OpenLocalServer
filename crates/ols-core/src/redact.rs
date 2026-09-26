//! Redaction of free text (§141): what the support bundle and the AI assistant may send. It is
//! the same idea as the log redaction layer and the traffic inspector, applied to whole pieces of
//! text: values next to secret-looking names, bearer tokens, credentials inside URLs, well-known
//! token shapes and private keys never leave the computer.

use std::sync::OnceLock;

use regex::Regex;

const REDACTED: &str = "[redacted]";

struct Patterns {
    pem: Regex,
    named: Regex,
    bearer: Regex,
    url_credentials: Regex,
    known_tokens: Regex,
    cookie_header: Regex,
}

fn patterns() -> &'static Patterns {
    static P: OnceLock<Patterns> = OnceLock::new();
    P.get_or_init(|| Patterns {
        pem: Regex::new(r"(?s)-----BEGIN [A-Z ]*PRIVATE KEY-----.*?-----END [A-Z ]*PRIVATE KEY-----").unwrap(),
        // NAME = value, NAME: value, "name": "value" where NAME holds a secret-looking word.
        named: Regex::new(
            r#"(?i)(["']?[A-Za-z0-9_.\-]*(?:password|passwd|pwd|secret|token|api[_\-]?key|apikey|private[_\-]?key|credential|authorization|signature|session[_\-]?id|access[_\-]?key)[A-Za-z0-9_.\-]*["']?\s*[=:]\s*)("[^"\r\n]*"|'[^'\r\n]*'|[^\s,;&"'}\]]+)"#,
        )
        .unwrap(),
        bearer: Regex::new(r"(?i)\b(bearer|basic)\s+[A-Za-z0-9._~+/=\-]{8,}").unwrap(),
        url_credentials: Regex::new(r"([a-zA-Z][a-zA-Z0-9+.\-]*://)[^\s/@:]+:[^\s/@]+@").unwrap(),
        known_tokens: Regex::new(r"\b(?:sk-[A-Za-z0-9_\-]{16,}|gh[pousr]_[A-Za-z0-9]{20,}|hf_[A-Za-z0-9]{20,}|xox[abprs]-[A-Za-z0-9\-]{10,}|AKIA[0-9A-Z]{16}|eyJ[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,})").unwrap(),
        cookie_header: Regex::new(r"(?im)^(\s*(?:set-)?cookie\s*:).*$").unwrap(),
    })
}

/// `text` with secrets replaced by `[redacted]`.
pub fn redact_text(text: &str) -> String {
    let p = patterns();
    let t = p.pem.replace_all(text, "[redacted private key]");
    let t = p.cookie_header.replace_all(&t, format!("$1 {REDACTED}").as_str());
    // Bearer/Basic first: `Authorization: Bearer abc` must lose `abc`, not just the word `Bearer`.
    let t = p.bearer.replace_all(&t, format!("$1 {REDACTED}").as_str());
    let t = p.named.replace_all(&t, format!("${{1}}{REDACTED}").as_str());
    let t = p.url_credentials.replace_all(&t, format!("${{1}}{REDACTED}@").as_str());
    p.known_tokens.replace_all(&t, REDACTED).into_owned()
}

/// A `.env` file with every value hidden (names and comments stay), for showing its shape.
pub fn redact_env_file(text: &str) -> String {
    text.lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with('#') || trimmed.is_empty() {
                return line.to_string();
            }
            let body = trimmed.strip_prefix("export ").unwrap_or(trimmed);
            match body.split_once('=') {
                Some((name, value)) if !value.trim().is_empty() => format!("{}={REDACTED}", name.trim_end()),
                _ => line.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_next_to_secret_names_are_hidden() {
        let out = redact_text("DB_PASSWORD=hunter2\napi_key: abcdef123456\n\"token\": \"xyz\"\nAPP_NAME=shop");
        assert!(!out.contains("hunter2") && !out.contains("abcdef123456") && !out.contains("xyz"), "{out}");
        assert!(out.contains("APP_NAME=shop"), "ordinary values stay: {out}");
    }

    #[test]
    fn bearer_tokens_url_credentials_and_known_shapes_are_hidden() {
        let out = redact_text("Authorization: Bearer abcdefghijklmnop\nmysql://root:s3cret@127.0.0.1/db\nkey sk-abcdefghijklmnopqrstuv and ghp_abcdefghijklmnopqrstuvwxyz");
        for leaked in ["abcdefghijklmnop", "s3cret", "sk-abcdefghijklmnopqrstuv", "ghp_abcdefghijklmnopqrstuvwxyz"] {
            assert!(!out.contains(leaked), "{leaked} leaked: {out}");
        }
        assert!(out.contains("127.0.0.1/db"), "the rest of the URL stays: {out}");
    }

    #[test]
    fn private_keys_and_cookies_are_hidden() {
        let out = redact_text("-----BEGIN PRIVATE KEY-----\nMIIabc\n-----END PRIVATE KEY-----\nCookie: session=abc; theme=dark\nSet-Cookie: id=9");
        assert!(!out.contains("MIIabc") && !out.contains("session=abc") && !out.contains("id=9"), "{out}");
    }

    #[test]
    fn env_files_keep_names_and_comments_only() {
        let out = redact_env_file("# database\nDB_HOST=127.0.0.1\nexport APP_KEY=base64:zzz\nEMPTY=\n");
        assert!(out.contains("# database") && out.contains("DB_HOST=[redacted]") && out.contains("APP_KEY=[redacted]") && out.contains("EMPTY="));
        assert!(!out.contains("127.0.0.1") && !out.contains("zzz"));
    }
}
