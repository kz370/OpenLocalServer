use super::types::*;
use crate::error::CoreError;

impl Domain {
    pub fn url(&self) -> String {
        let scheme = if self.https { "https" } else { "http" };
        format!("{scheme}://{}", self.hostname)
    }
}

/// Rejects a per-site server id that isn't one we ship. A blank value means "use the
/// default" and is stored as such, so the form and the file agree.
pub fn validate_domain_server(server: Option<&str>) -> Result<Option<String>, CoreError> {
    match server.map(str::trim) {
        None | Some("") => Ok(None),
        Some(s) if crate::web::SERVER_IDS.contains(&s) => Ok(Some(s.to_string())),
        Some(s) => Err(CoreError::DomainError(format!(
            "\"{s}\" is not a web server OLS ships. \
             Pick Default, Nginx, Apache or Caddy."
        ))),
    }
}

/// Normalizes the `localhost/<prefix>` path a site is also served under (§55). A blank
/// value means "no path route" and is stored as `None`. The value lands as a URL path
/// segment in a generated config, so it is one segment of safe characters and nothing
/// that could escape the prefix (`..`, `/`, `\`) or change the request's meaning.
pub fn validate_path_prefix(prefix: Option<&str>) -> Result<Option<String>, CoreError> {
    let Some(raw) = prefix.map(str::trim) else {
        return Ok(None);
    };
    if raw.is_empty() {
        return Ok(None);
    }
    let bad = |why: &str| {
        Err(CoreError::DomainError(format!(
            "\"{raw}\" cannot be used as a localhost path: {why}"
        )))
    };
    if raw.len() > 40 {
        return bad("keep it under 40 characters");
    }
    if raw.contains('/') || raw.contains('\\') {
        return bad("it is one path segment, with no '/' in it");
    }
    if raw == "." || raw == ".." {
        return bad("'.' and '..' are reserved");
    }
    if !raw
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
    {
        return bad("use lowercase letters, digits, '-' and '_' only");
    }
    Ok(Some(raw.to_string()))
}

/// The server that actually renders this site: its own override, or the default one.
pub fn resolved_server(d: &Domain, cfg: &crate::web::WebConfig) -> String {
    cfg.resolve_server(d.server.as_deref())
}
