use super::manager::compare_versions;

pub(crate) fn nginx_versions(page: &str) -> Vec<String> {
    let mut found = Vec::new();
    for part in page.split("nginx-").skip(1) {
        let Some(version) = part.split(".zip").next() else {
            continue;
        };
        if !version.is_empty()
            && version
                .split('.')
                .all(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
        {
            found.push(version.to_string());
        }
    }
    found.sort_by(|a, b| compare_versions(b, a));
    found.dedup();
    found
}

pub(crate) fn is_valid_released_python(v: &str) -> bool {
    let parts: Vec<&str> = v.split('.').collect();
    if parts.len() != 3 {
        return false;
    }
    let (Ok(minor), Ok(patch)) = (parts[1].parse::<u32>(), parts[2].parse::<u32>()) else {
        return false;
    };
    match minor {
        7 => patch <= 9,
        8 => patch <= 10,
        9 => patch <= 13,
        10 => patch <= 11,
        11 => patch <= 9,
        12 => patch <= 10,
        13 => patch <= 17,
        14 => patch <= 9,
        15 => patch <= 1,
        _ => false,
    }
}

pub(crate) fn extract_versions(text: &str, start: &str, end: &str) -> Vec<String> {
    let mut found = Vec::new();
    for segment in text.split(start).skip(1) {
        let Some(candidate) = segment.split(end).next() else {
            continue;
        };
        let candidate = candidate.trim().trim_matches(['\'', '"', '`', ' ']);
        if !candidate.is_empty()
            && candidate
                .split('.')
                .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
        {
            found.push(candidate.to_string());
        }
    }
    found.sort_by(|a, b| compare_versions(b, a));
    found.dedup();
    found
}

pub(crate) fn extract_numeric_filename_versions(text: &str, prefix: &str) -> Vec<String> {
    let mut found = Vec::new();
    for segment in text.split(prefix).skip(1) {
        let version: String = segment
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        if !version.is_empty() && version.split('.').all(|p| !p.is_empty()) {
            found.push(version);
        }
    }
    found.sort_by(|a, b| compare_versions(b, a));
    found.dedup();
    found
}

pub(crate) fn collect_mongodb_versions(value: &serde_json::Value, versions: &mut Vec<String>) {
    match value {
        serde_json::Value::Array(items) => items
            .iter()
            .for_each(|item| collect_mongodb_versions(item, versions)),
        serde_json::Value::Object(fields) => {
            if let Some(version) = fields.get("version").and_then(|v| v.as_str()) {
                if mongodb_archive(value).is_some() {
                    versions.push(version.trim_start_matches('v').to_string());
                }
            }
            fields
                .values()
                .for_each(|item| collect_mongodb_versions(item, versions));
        }
        _ => {}
    }
}

pub(crate) fn mongodb_archive(value: &serde_json::Value) -> Option<(String, String)> {
    match value {
        serde_json::Value::Array(items) => items.iter().find_map(mongodb_archive),
        serde_json::Value::Object(fields) => {
            let url = fields.get("url").and_then(|v| v.as_str());
            let sha = fields.get("sha256").and_then(|v| v.as_str());
            if let (Some(url), Some(sha)) = (url, sha) {
                if url.contains("windows")
                    && url.contains(".zip")
                    && sha.len() == 64
                    && sha.chars().all(|c| c.is_ascii_hexdigit())
                {
                    return Some((url.to_string(), sha.to_string()));
                }
            }
            fields.values().find_map(mongodb_archive)
        }
        _ => None,
    }
}

pub(crate) fn mongodb_asset(value: &serde_json::Value, version: &str) -> Option<(String, String)> {
    match value {
        serde_json::Value::Array(items) => {
            items.iter().find_map(|item| mongodb_asset(item, version))
        }
        serde_json::Value::Object(fields) => {
            if fields
                .get("version")
                .and_then(|v| v.as_str())
                .is_some_and(|v| v.trim_start_matches('v') == version)
            {
                return mongodb_archive(value);
            }
            fields
                .values()
                .find_map(|item| mongodb_asset(item, version))
        }
        _ => None,
    }
}

pub(crate) fn linked_download_url(
    page: &str,
    filename_prefix: &str,
    version: &str,
) -> Option<String> {
    for segment in page.split("href=").skip(1) {
        let quote = segment.chars().next()?;
        if quote != '\'' && quote != '"' {
            continue;
        }
        let href = segment[1..].split(quote).next()?;
        if !href.ends_with(".zip") {
            continue;
        }
        let filename = href.rsplit('/').next().unwrap_or(href);
        if filename.starts_with(filename_prefix)
            && filename.contains(version)
            && !filename.to_ascii_lowercase().contains("win32")
        {
            if href.starts_with("http://") || href.starts_with("https://") {
                return Some(href.to_string());
            }
            if href.starts_with('/') {
                return Some(format!("https://www.apachelounge.com{href}"));
            }
            return Some(format!("https://www.apachelounge.com/download/{href}"));
        }
    }
    None
}

pub(crate) fn parse_apache_sha256(text: &str) -> Option<String> {
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        if line.contains("SHA256") {
            for next in lines.by_ref() {
                let trimmed = next.trim().to_ascii_lowercase();
                if trimmed.is_empty() {
                    continue;
                }
                if trimmed.len() == 64 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Some(trimmed);
                }
                break;
            }
        }
    }
    None
}

pub(crate) const EDB_PAGE_URL: &str = "https://www.enterprisedb.com/download-postgresql-binaries";

/// EDB renders every downloadable version as
/// `Binaries from installer<span …>Version <!-- -->18.6</span>` followed by one
/// `<a href="…getfile.jsp?fileid=…"><img alt="…">` per platform. HTML comment nodes and the
/// span mean the version is never adjacent to the label, so each half is parsed on its own:
/// the version after `Version`, the link from the `alt="Windows x86-64"` image it wraps.
pub(crate) fn edb_versions(page: &str) -> Vec<String> {
    let mut found = Vec::new();
    for section in page.split("Binaries from installer").skip(1) {
        if let Some(version) = edb_section_version(section) {
            if edb_windows_link(section).is_some() {
                found.push(version);
            }
        }
    }
    found.sort_by(|a, b| compare_versions(b, a));
    found.dedup();
    found
}

/// The `18.6` in `…Version <!-- -->18.6</span>`, skipping the markup React interleaves.
fn edb_section_version(section: &str) -> Option<String> {
    let at = section.find("Version")? + "Version".len();
    let version: String = section[at..]
        .trim_start()
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    (!version.is_empty()).then_some(version)
}

/// The Windows x86-64 archive for one `Binaries from installer` section. EDB serves the file
/// through a `getfile.jsp` redirect rather than a direct `.zip` URL, so the caller still
/// resolves the final location over HTTP; only the platform anchor is selected here — the
/// first one in a section is whatever the page happens to list first (often Mac OS X).
fn edb_windows_link(section: &str) -> Option<String> {
    let at = section.find("alt=\"Windows x86-64\"")?;
    let before = &section[..at];
    let href_at = before.rfind("href=")? + 5;
    let quote = before[href_at..].chars().next()?;
    if quote != '\'' && quote != '"' {
        return None;
    }
    let from = href_at + 1;
    let to = section[from..].find(quote)? + from;
    let href = &section[from..to];
    if href.starts_with("http://") || href.starts_with("https://") {
        return Some(href.to_string());
    }
    Some(format!("https://www.enterprisedb.com{href}"))
}

pub(crate) fn edb_archive_link(page: &str, version: &str) -> Option<String> {
    page.split("Binaries from installer")
        .skip(1)
        .filter(|section| edb_section_version(section).as_deref() == Some(version))
        .find_map(edb_windows_link)
}

/// The `13.1` of `13.1.1` — the release branch `downloads.mariadb.org/rest-api` is keyed by.
/// Cuts at the second dot, so `10.11.14` becomes `10.11`; a branch-only `13.1` is unchanged.
pub(crate) fn mariadb_branch(version: &str) -> &str {
    let Some(first) = version.find('.') else {
        return version;
    };
    match version[first + 1..].find('.') {
        Some(second) => &version[..first + 1 + second],
        None => version,
    }
}
