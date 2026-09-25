//! Hosts-file block management (§44–47 — Stage 6). Pure text transforms only — fully
//! testable without touching the real, admin-protected hosts file. The actual privileged
//! write happens in a separate `devforge-helper` binary (not yet wired up: writing to
//! `C:\Windows\System32\drivers\etc\hosts` needs elevation, and this session can't click
//! through a live UAC prompt — see module docs in the plan). These functions are exactly
//! what that helper will call once it exists; nothing here needs to change.

const BEGIN_MARKER: &str = "# BEGIN OpenLocalServer — do not edit this block by hand";
const END_MARKER: &str = "# END OpenLocalServer";

/// Replaces (or appends) the OpenLocalServer-managed block in `existing` hosts-file
/// content. Idempotent — calling this again with the same `entries` reproduces the same
/// output. Never touches anything outside its own delimited block (§75: DevForge must
/// not clobber a user's other hosts entries).
pub fn apply_hosts_block(existing: &str, entries: &[(String, String)]) -> String {
    let block = render_block(entries);
    replace_block(existing, Some(&block))
}

/// Removes the OpenLocalServer block entirely, leaving everything else untouched.
pub fn remove_hosts_block(existing: &str) -> String {
    replace_block(existing, None)
}

fn render_block(entries: &[(String, String)]) -> String {
    let mut block = String::new();
    block.push_str(BEGIN_MARKER);
    block.push('\n');
    for (ip, host) in entries {
        block.push_str(&format!("{ip} {host}\n"));
    }
    block.push_str(END_MARKER);
    block
}

fn replace_block(existing: &str, new_block: Option<&str>) -> String {
    let lines: Vec<&str> = existing.lines().collect();
    let begin = lines.iter().position(|l| l.trim() == BEGIN_MARKER);
    let end = lines.iter().position(|l| l.trim() == END_MARKER);

    let mut out: Vec<String> = Vec::new();
    match (begin, end) {
        (Some(b), Some(e)) if e >= b => {
            out.extend(lines[..b].iter().map(|s| s.to_string()));
            if let Some(block) = new_block {
                out.extend(block.lines().map(|s| s.to_string()));
            }
            out.extend(lines[e + 1..].iter().map(|s| s.to_string()));
        }
        _ => {
            // No existing block: keep everything as-is, then append (if there's
            // anything to append — `remove` on a file with no block is a no-op).
            out.extend(lines.iter().map(|s| s.to_string()));
            if let Some(block) = new_block {
                out.push(String::new());
                out.extend(block.lines().map(|s| s.to_string()));
            }
        }
    }

    // Collapse any accidental leading blank line from the append path above when the
    // original file was empty.
    while out.first().is_some_and(|l| l.is_empty()) && out.len() > 1 {
        out.remove(0);
    }
    let mut result = out.join("\n");
    result.push('\n');
    // The Windows hosts file uses CRLF; keep it that way rather than rewriting every line ending.
    if existing.contains("\r\n") {
        result = result.replace('\n', "\r\n");
    }
    result
}

/// The OS hosts file. `OLS_HOSTS_FILE` redirects it for tests (§161) — only honored by
/// the unprivileged core, never by `ols-helper`, which always writes the real file.
pub fn hosts_path() -> std::path::PathBuf {
    if let Ok(over) = std::env::var("OLS_HOSTS_FILE") {
        return over.into();
    }
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_string());
    std::path::PathBuf::from(root).join("System32").join("drivers").join("etc").join("hosts")
}

/// Loopback entries for `hostnames` — DevForge never points a name anywhere else (§138).
pub fn loopback_entries(hostnames: &[String]) -> Vec<(String, String)> {
    hostnames.iter().map(|h| ("127.0.0.1".to_string(), h.clone())).collect()
}

/// True when the file already contains exactly `entries` in our block (so no privileged
/// write — and no UAC prompt — is needed).
pub fn is_in_sync(existing: &str, entries: &[(String, String)]) -> bool {
    let normalise = |s: &str| s.replace("\r\n", "\n").trim_end().to_string();
    if entries.is_empty() {
        return !existing.contains(BEGIN_MARKER);
    }
    normalise(&apply_hosts_block(existing, entries)) == normalise(existing)
}

/// Brings the hosts file in line with `hostnames`. Writes directly when redirected for
/// tests; otherwise goes through `ols-helper` (elevating only if the plain write is denied).
/// Returns whether anything had to change.
pub fn sync(hostnames: &[String]) -> Result<bool, String> {
    let entries = loopback_entries(hostnames);
    let path = hosts_path();
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    if is_in_sync(&existing, &entries) {
        return Ok(false);
    }
    if std::env::var("OLS_HOSTS_FILE").is_ok() {
        let updated =
            if entries.is_empty() { remove_hosts_block(&existing) } else { apply_hosts_block(&existing, &entries) };
        std::fs::write(&path, updated).map_err(|e| e.to_string())?;
        return Ok(true);
    }
    let args: Vec<String> = if entries.is_empty() {
        vec!["hosts-remove".to_string()]
    } else {
        std::iter::once("hosts-apply".to_string()).chain(entries.iter().map(|(ip, h)| format!("{ip}={h}"))).collect()
    };
    crate::elevate::run_helper(&args)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries() -> Vec<(String, String)> {
        vec![("127.0.0.1".to_string(), "shop.test".to_string()), ("127.0.0.1".to_string(), "api.shop.test".to_string())]
    }

    #[test]
    fn appends_block_to_a_hosts_file_with_no_existing_block() {
        let original = "127.0.0.1 localhost\n";
        let result = apply_hosts_block(original, &entries());

        assert!(result.contains("127.0.0.1 localhost"));
        assert!(result.contains(BEGIN_MARKER));
        assert!(result.contains("127.0.0.1 shop.test"));
        assert!(result.contains("127.0.0.1 api.shop.test"));
        assert!(result.contains(END_MARKER));
    }

    #[test]
    fn applying_twice_is_idempotent() {
        let original = "127.0.0.1 localhost\n";
        let once = apply_hosts_block(original, &entries());
        let twice = apply_hosts_block(&once, &entries());
        assert_eq!(once, twice);
    }

    #[test]
    fn updating_entries_replaces_only_the_managed_block() {
        let original = "127.0.0.1 localhost\n# my own comment\n10.0.0.5 nas\n";
        let first = apply_hosts_block(original, &entries());
        assert!(first.contains("# my own comment"));
        assert!(first.contains("10.0.0.5 nas"));

        let updated = apply_hosts_block(&first, &[("127.0.0.1".to_string(), "new-project.test".to_string())]);
        assert!(updated.contains("# my own comment"), "unrelated content must survive");
        assert!(updated.contains("10.0.0.5 nas"), "unrelated content must survive");
        assert!(updated.contains("127.0.0.1 new-project.test"));
        assert!(!updated.contains("shop.test"), "old managed entries must be gone, not accumulated");
    }

    #[test]
    fn remove_deletes_the_block_and_nothing_else() {
        let original = "127.0.0.1 localhost\n10.0.0.5 nas\n";
        let with_block = apply_hosts_block(original, &entries());
        let removed = remove_hosts_block(&with_block);

        assert!(removed.contains("127.0.0.1 localhost"));
        assert!(removed.contains("10.0.0.5 nas"));
        assert!(!removed.contains(BEGIN_MARKER));
        assert!(!removed.contains("shop.test"));
    }

    #[test]
    fn crlf_hosts_files_stay_crlf() {
        let original = "127.0.0.1 localhost\r\n";
        let result = apply_hosts_block(original, &entries());
        assert!(result.contains("\r\n"));
        assert!(!result.replace("\r\n", "").contains('\n'), "no bare LF may remain in a CRLF file");
    }

    #[test]
    fn is_in_sync_detects_matching_and_stale_blocks() {
        let original = "127.0.0.1 localhost\n";
        let synced = apply_hosts_block(original, &entries());
        assert!(is_in_sync(&synced, &entries()));
        assert!(!is_in_sync(original, &entries()));
        assert!(is_in_sync(original, &[]), "nothing to write and no block present");
        assert!(!is_in_sync(&synced, &[]), "an old block that should be removed is out of sync");
    }

    #[test]
    fn remove_on_a_file_with_no_block_is_a_no_op() {
        let original = "127.0.0.1 localhost\n";
        assert_eq!(remove_hosts_block(original), original);
    }
}
