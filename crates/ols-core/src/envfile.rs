//! `.env` files (§103, Stage 4/5): read, edit, compare, import and export a project's
//! environment files without reformatting them. Editing works on the file's own lines, so
//! comments, blank lines, ordering, `export` prefixes, quote style and line endings all
//! survive a change to one value.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvEntry {
    pub key: String,
    /// The value with its quotes removed.
    pub value: String,
    /// 1-based line number.
    pub line: usize,
    /// Looks like a password, token or key: the UI hides it until asked.
    pub secret: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvIssue {
    pub line: usize,
    /// "error" blocks saving; "warning" doesn't.
    pub severity: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvFileView {
    pub name: String,
    pub content: String,
    pub entries: Vec<EnvEntry>,
    pub issues: Vec<EnvIssue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvFileInfo {
    pub name: String,
    pub size: u64,
    pub entries: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvDiffRow {
    pub key: String,
    pub a: Option<String>,
    pub b: Option<String>,
    /// "same" | "different" | "only_a" | "only_b"
    pub status: String,
    pub secret: bool,
}

/// One line of a file, classified.
enum Line<'a> {
    Blank,
    Comment,
    Entry {
        key: &'a str,
        raw_value: &'a str,
    },
    /// Not blank, not a comment, and no `=`.
    Junk,
}

/// `.env`, `.env.local`, `.env.testing`, ... — and nothing that could name another path.
pub fn valid_file_name(name: &str) -> bool {
    name == ".env"
        || name.strip_prefix(".env.").is_some_and(|rest| {
            !rest.is_empty()
                && rest
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
                && !rest.contains("..")
        })
}

pub fn is_secret_key(key: &str) -> bool {
    let k = key.to_ascii_uppercase();
    if k.ends_with("_PUBLIC") || k.contains("PUBLIC_KEY") {
        return false;
    }
    [
        "PASS",
        "SECRET",
        "TOKEN",
        "KEY",
        "PRIVATE",
        "CREDENTIAL",
        "AUTH",
        "SALT",
        "DSN",
    ]
    .iter()
    .any(|w| k.contains(w))
}

fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && !key.starts_with(|c: char| c.is_ascii_digit())
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.'))
}

fn classify(line: &str) -> Line<'_> {
    let t = line.trim();
    if t.is_empty() {
        return Line::Blank;
    }
    if t.starts_with('#') {
        return Line::Comment;
    }
    let t = t.strip_prefix("export ").map(str::trim_start).unwrap_or(t);
    match t.split_once('=') {
        Some((key, raw)) => Line::Entry {
            key: key.trim(),
            raw_value: raw.trim(),
        },
        None => Line::Junk,
    }
}

/// Removes the quotes and a trailing ` # comment`. `None` when a quote is never closed.
fn unquote(raw: &str) -> Option<String> {
    let mut chars = raw.chars();
    match chars.next() {
        Some(q @ ('"' | '\'')) => {
            let mut out = String::new();
            let mut escaped = false;
            for c in chars {
                if escaped {
                    out.push(match c {
                        'n' if q == '"' => '\n',
                        other => other,
                    });
                    escaped = false;
                } else if c == '\\' && q == '"' {
                    escaped = true;
                } else if c == q {
                    return Some(out);
                } else {
                    out.push(c);
                }
            }
            None
        }
        _ => {
            // Unquoted: a comment starts at " #".
            let end = raw.find(" #").unwrap_or(raw.len());
            Some(raw[..end].trim_end().to_string())
        }
    }
}

/// How a value is written back: bare when it can be, otherwise double-quoted.
fn format_value(value: &str) -> String {
    let plain = !value.is_empty()
        && !value
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '#' | '"' | '\'' | '\\' | '$' | '`'));
    if value.is_empty() || plain {
        value.to_string()
    } else {
        format!(
            "\"{}\"",
            value
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
        )
    }
}

pub fn parse(content: &str) -> Vec<EnvEntry> {
    content
        .lines()
        .enumerate()
        .filter_map(|(i, line)| match classify(line) {
            Line::Entry { key, raw_value } if valid_key(key) => Some(EnvEntry {
                key: key.to_string(),
                value: unquote(raw_value).unwrap_or_default(),
                line: i + 1,
                secret: is_secret_key(key),
            }),
            _ => None,
        })
        .collect()
}

pub fn validate(content: &str) -> Vec<EnvIssue> {
    let mut issues = Vec::new();
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for (i, line) in content.lines().enumerate() {
        let n = i + 1;
        match classify(line) {
            Line::Blank | Line::Comment => {}
            Line::Junk => issues.push(EnvIssue {
                line: n,
                severity: "error".into(),
                message: "This line has no \"=\", so it is not a variable.".into(),
            }),
            Line::Entry { key, raw_value } => {
                if !valid_key(key) {
                    issues.push(EnvIssue { line: n, severity: "error".into(), message: format!("\"{key}\" is not a valid variable name (letters, digits and _ , not starting with a digit).") });
                    continue;
                }
                if let Some(first) = seen.insert(key.to_string(), n) {
                    issues.push(EnvIssue { line: n, severity: "warning".into(), message: format!("{key} is also set on line {first}; the later value wins in most loaders.") });
                }
                match unquote(raw_value) {
                    None => issues.push(EnvIssue {
                        line: n,
                        severity: "error".into(),
                        message: format!("The quote in the value of {key} is never closed."),
                    }),
                    Some(v)
                        if !raw_value.starts_with(['"', '\''])
                            && v.contains(' ')
                            && !v.contains('#') =>
                    {
                        issues.push(EnvIssue { line: n, severity: "warning".into(), message: format!("The value of {key} has spaces but no quotes; some loaders cut it at the first space.") });
                    }
                    Some(_) => {}
                }
            }
        }
    }
    issues
}

pub fn view(name: &str, content: &str) -> EnvFileView {
    EnvFileView {
        name: name.to_string(),
        content: content.to_string(),
        entries: parse(content),
        issues: validate(content),
    }
}

fn eol(content: &str) -> &'static str {
    if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

/// Sets `key` to `value`, changing only that line (or appending it).
pub fn set(content: &str, key: &str, value: &str) -> Result<String, String> {
    if !valid_key(key) {
        return Err(format!("\"{key}\" is not a valid variable name."));
    }
    if value.contains(char::from(13)) {
        return Err("A value can't contain a raw carriage return.".into());
    }
    let nl = eol(content);
    let mut out = String::with_capacity(content.len() + key.len() + value.len() + 4);
    let mut done = false;
    for raw_line in content.split_inclusive('\n') {
        let (text, ending) = match raw_line.strip_suffix("\r\n") {
            Some(t) => (t, "\r\n"),
            None => match raw_line.strip_suffix('\n') {
                Some(t) => (t, "\n"),
                None => (raw_line, ""),
            },
        };
        match classify(text) {
            Line::Entry { key: k, raw_value } if k == key && !done => {
                let indent: String = text.chars().take_while(|c| c.is_whitespace()).collect();
                let export = if text.trim_start().starts_with("export ") {
                    "export "
                } else {
                    ""
                };
                // Keep a trailing comment that followed an unquoted or quoted value.
                let comment = trailing_comment(raw_value)
                    .map(|c| format!(" {c}"))
                    .unwrap_or_default();
                out.push_str(&format!(
                    "{indent}{export}{key}={}{comment}{ending}",
                    format_value(value)
                ));
                done = true;
            }
            _ => out.push_str(raw_line),
        }
    }
    if !done {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push_str(nl);
        }
        out.push_str(&format!("{key}={}{nl}", format_value(value)));
    }
    Ok(out)
}

/// The `# comment` after a value, if any.
fn trailing_comment(raw_value: &str) -> Option<String> {
    match raw_value.chars().next() {
        Some(q @ ('"' | '\'')) => {
            let mut escaped = false;
            for (i, c) in raw_value.char_indices().skip(1) {
                if escaped {
                    escaped = false;
                } else if c == '\\' && q == '"' {
                    escaped = true;
                } else if c == q {
                    let rest = raw_value[i + 1..].trim();
                    return rest.starts_with('#').then(|| rest.to_string());
                }
            }
            None
        }
        _ => raw_value
            .find(" #")
            .map(|i| raw_value[i..].trim().to_string()),
    }
}

/// Removes every line that sets `key`.
pub fn remove(content: &str, key: &str) -> String {
    content
        .split_inclusive('\n')
        .filter(|raw| !matches!(classify(raw.trim_end_matches(['\r', '\n'])), Line::Entry { key: k, .. } if k == key))
        .collect()
}

/// Every key on either side, marked same / different / only one side. Later duplicates win,
/// like a loader would read them.
pub fn compare(a: &str, b: &str) -> Vec<EnvDiffRow> {
    let map = |c: &str| -> BTreeMap<String, String> {
        parse(c).into_iter().map(|e| (e.key, e.value)).collect()
    };
    let (ma, mb) = (map(a), map(b));
    let keys: BTreeSet<&String> = ma.keys().chain(mb.keys()).collect();
    keys.into_iter()
        .map(|k| {
            let (va, vb) = (ma.get(k).cloned(), mb.get(k).cloned());
            let status = match (&va, &vb) {
                (Some(x), Some(y)) if x == y => "same",
                (Some(_), Some(_)) => "different",
                (Some(_), None) => "only_a",
                _ => "only_b",
            };
            EnvDiffRow {
                key: k.clone(),
                a: va,
                b: vb,
                status: status.into(),
                secret: is_secret_key(k),
            }
        })
        .collect()
}

/// `merge` sets every key from `incoming` in `existing` (keeping the rest); `replace`
/// takes `incoming` as the whole file.
pub fn import(existing: &str, incoming: &str, mode: &str) -> Result<String, String> {
    match mode {
        "replace" => Ok(incoming.to_string()),
        "merge" => {
            let mut out = existing.to_string();
            for e in parse(incoming) {
                out = set(&out, &e.key, &e.value)?;
            }
            Ok(out)
        }
        other => Err(format!(
            "Unknown import mode \"{other}\" (use merge or replace)."
        )),
    }
}

/// The `.env*` files in a project folder, with how many variables each holds.
pub fn list_files(project: &Path) -> Vec<EnvFileInfo> {
    let mut files: Vec<EnvFileInfo> = std::fs::read_dir(project)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let meta = e.metadata().ok()?;
            (meta.is_file() && valid_file_name(&name)).then(|| {
                let entries = std::fs::read_to_string(e.path())
                    .map(|c| parse(&c).len())
                    .unwrap_or(0);
                EnvFileInfo {
                    name,
                    size: meta.len(),
                    entries,
                }
            })
        })
        .collect();
    // `.env` first, then the rest by name.
    files.sort_by(|a, b| (a.name != ".env", &a.name).cmp(&(b.name != ".env", &b.name)));
    files
}

pub fn path_of(project: &Path, name: &str) -> Result<PathBuf, String> {
    if !valid_file_name(name) {
        return Err(format!(
            "\"{name}\" is not an environment file name (.env, .env.local, .env.testing, ...)."
        ));
    }
    Ok(project.join(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# App\nAPP_NAME=\"My App\"   # the name\nexport APP_ENV=local\n\nDB_PASSWORD=s3cret\nEMPTY=\n";

    #[test]
    fn parses_quotes_comments_and_flags_secrets() {
        let e = parse(SAMPLE);
        let get = |k: &str| e.iter().find(|x| x.key == k).unwrap();
        assert_eq!(get("APP_NAME").value, "My App");
        assert_eq!(get("APP_ENV").value, "local");
        assert_eq!(get("EMPTY").value, "");
        assert!(get("DB_PASSWORD").secret && !get("APP_ENV").secret);
        assert_eq!(get("DB_PASSWORD").line, 5);
    }

    #[test]
    fn setting_one_value_leaves_every_other_byte_alone() {
        let out = set(SAMPLE, "APP_ENV", "production").unwrap();
        assert_eq!(out, "# App\nAPP_NAME=\"My App\"   # the name\nexport APP_ENV=production\n\nDB_PASSWORD=s3cret\nEMPTY=\n");
        let out = set(SAMPLE, "APP_NAME", "Other App").unwrap();
        assert!(out.contains("APP_NAME=\"Other App\" # the name\n"), "{out}");
    }

    #[test]
    fn new_keys_are_appended_and_crlf_is_kept() {
        let crlf = "A=1\r\nB=2\r\n";
        assert_eq!(
            set(crlf, "C", "x y").unwrap(),
            "A=1\r\nB=2\r\nC=\"x y\"\r\n"
        );
        assert_eq!(set("A=1", "B", "2").unwrap(), "A=1\nB=2\n");
        assert_eq!(set("", "A", "1").unwrap(), "A=1\n");
    }

    #[test]
    fn values_needing_quotes_are_escaped_and_round_trip() {
        for v in [
            "plain",
            "with space",
            "has\"quote",
            "hash#tag",
            "back\\slash",
            "line\nbreak",
            "$dollar",
        ] {
            let out = set("", "K", v).unwrap();
            assert_eq!(parse(&out)[0].value, v, "{v}: {out}");
        }
    }

    #[test]
    fn remove_deletes_only_that_key() {
        assert_eq!(
            remove(SAMPLE, "APP_ENV"),
            "# App\nAPP_NAME=\"My App\"   # the name\n\nDB_PASSWORD=s3cret\nEMPTY=\n"
        );
        assert_eq!(remove("A=1\r\nB=2\r\n", "A"), "B=2\r\n");
    }

    #[test]
    fn validation_catches_broken_lines_duplicates_and_open_quotes() {
        let issues = validate("A=1\nnot a variable\n1BAD=x\nA=2\nB=\"open\nC=two words\n");
        let has =
            |line: usize, sev: &str| issues.iter().any(|i| i.line == line && i.severity == sev);
        assert!(has(2, "error") && has(3, "error") && has(5, "error"));
        assert!(has(4, "warning"), "duplicate key");
        assert!(has(6, "warning"), "unquoted spaces");
        assert!(validate(SAMPLE).iter().all(|i| i.severity != "error"));
    }

    #[test]
    fn compare_marks_each_key() {
        let rows = compare("A=1\nB=2\nC=3\n", "A=1\nB=9\nD=4\n");
        let status = |k: &str| rows.iter().find(|r| r.key == k).unwrap().status.clone();
        assert_eq!(
            (
                status("A").as_str(),
                status("B").as_str(),
                status("C").as_str(),
                status("D").as_str()
            ),
            ("same", "different", "only_a", "only_b")
        );
    }

    #[test]
    fn import_merges_or_replaces() {
        assert_eq!(
            import("A=1\nB=2\n", "B=3\nC=4\n", "merge").unwrap(),
            "A=1\nB=3\nC=4\n"
        );
        assert_eq!(import("A=1\n", "B=3\n", "replace").unwrap(), "B=3\n");
        assert!(import("", "", "wipe").is_err());
    }

    #[test]
    fn only_env_file_names_are_accepted() {
        for ok in [".env", ".env.local", ".env.testing", ".env.production"] {
            assert!(valid_file_name(ok), "{ok}");
        }
        for bad in [
            "env",
            ".env.",
            "..\\.env",
            ".env/../x",
            "config.php",
            ".envrc",
        ] {
            assert!(!valid_file_name(bad), "{bad}");
        }
    }

    #[test]
    fn lists_env_files_with_dot_env_first() {
        let dir = tempfile::tempdir().unwrap();
        for (n, c) in [
            (".env.local", "A=1\n"),
            (".env", "A=1\nB=2\n"),
            ("readme.md", "x"),
        ] {
            std::fs::write(dir.path().join(n), c).unwrap();
        }
        let files = list_files(dir.path());
        assert_eq!(
            files.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
            [".env", ".env.local"]
        );
        assert_eq!(files[0].entries, 2);
    }
}
