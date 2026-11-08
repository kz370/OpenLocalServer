//! PHP upgrade check (§171): before switching a site to a newer PHP, find out what would
//! break, using only tools OLS already has: Composer's platform check, the extension list,
//! `php -l` on every file with the target PHP, and PHPCompatibility when the project already
//! has it. Nothing is downloaded, nothing in the project is changed.

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::app::Inner;
use crate::domain::SiteKind;
use crate::error::CoreError;
use crate::tasks::TaskHandle;

const MAX_FILES: usize = 20_000;
const LINT_THREADS: usize = 4;
const LINT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_ITEMS: usize = 500;

/// Folders never linted: dependencies, caches and generated output.
const SKIP_DIRS: &[&str] = &["vendor", "node_modules", "storage", "var", "cache", ".git"];
const SKIP_PREFIXES: &[&str] = &["bootstrap/cache", "wp-content/cache"];

/// PHP extensions that are part of the Windows binary itself (no `php_*.dll`).
const BUILTIN_EXTENSIONS: &[&str] = &[
    "core",
    "date",
    "hash",
    "pcre",
    "spl",
    "standard",
    "json",
    "ctype",
    "tokenizer",
    "filter",
    "libxml",
    "dom",
    "simplexml",
    "xml",
    "xmlreader",
    "xmlwriter",
    "iconv",
    "session",
    "phar",
    "reflection",
    "random",
    "zlib",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpgradeItem {
    /// `blocker`, `warning` or `info`.
    pub level: String,
    pub title: String,
    pub detail: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhpUpgradeReport {
    pub hostname: String,
    pub from: String,
    pub to: String,
    pub checked_ms: u64,
    pub items: Vec<UpgradeItem>,
    pub files_checked: u32,
    pub truncated: bool,
    /// No blockers.
    pub can_switch: bool,
}

fn item(level: &str, title: impl Into<String>, detail: impl Into<String>) -> UpgradeItem {
    UpgradeItem {
        level: level.into(),
        title: title.into(),
        detail: detail.into(),
        file: None,
        line: None,
    }
}

// ------------------------------------------------------------------ parsers (pure)

/// One row of `composer check-platform-reqs`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformRow {
    pub name: String,
    pub version: String,
    /// `success`, `failed` or `missing`.
    pub status: String,
    pub detail: Option<String>,
}

/// `composer check-platform-reqs --format=json`.
pub fn parse_platform_json(text: &str) -> Option<Vec<PlatformRow>> {
    let v: serde_json::Value = serde_json::from_str(text.trim()).ok()?;
    let rows = v.as_array()?;
    Some(
        rows.iter()
            .filter_map(|r| {
                let name = r.get("name")?.as_str()?.to_string();
                let detail = r.get("failed_requirement").and_then(|f| {
                    Some(format!(
                        "{} {} {} {}",
                        f.get("source")?.as_str()?,
                        f.get("type").and_then(|t| t.as_str()).unwrap_or("requires"),
                        f.get("target").and_then(|t| t.as_str()).unwrap_or(&name),
                        f.get("constraint").and_then(|t| t.as_str()).unwrap_or("")
                    ))
                });
                Some(PlatformRow {
                    version: r
                        .get("version")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string(),
                    status: r.get("status")?.as_str()?.to_string(),
                    name,
                    detail,
                })
            })
            .collect(),
    )
}

/// The plain-text table older Composers print: `<name> <version> <status> [details]`.
pub fn parse_platform_text(text: &str) -> Vec<PlatformRow> {
    text.lines()
        .filter_map(|l| {
            let mut parts = l.split_whitespace();
            let name = parts.next()?.to_string();
            let version = parts.next()?.to_string();
            let status = parts.next()?.to_string();
            if !matches!(status.as_str(), "success" | "failed" | "missing") {
                return None;
            }
            let rest: Vec<&str> = parts.collect();
            Some(PlatformRow {
                name,
                version,
                status,
                detail: (!rest.is_empty()).then(|| rest.join(" ")),
            })
        })
        .collect()
}

fn platform_items(rows: &[PlatformRow], target: &str) -> Vec<UpgradeItem> {
    rows.iter()
        .filter(|r| r.status != "success")
        .map(|r| {
            let detail = r.detail.clone().unwrap_or_default();
            if r.status == "missing" {
                item(
                    "blocker",
                    format!("{} is missing for PHP {target}", r.name),
                    detail,
                )
            } else {
                item(
                    "blocker",
                    format!("{} does not fit PHP {target}", r.name),
                    if detail.is_empty() {
                        format!("Found {} {}", r.name, r.version)
                    } else {
                        detail
                    },
                )
            }
        })
        .collect()
}

/// `\\?\`-less, lower-case, forward-slash form for comparing a path with the project root.
fn norm(p: &str) -> String {
    p.replace('\\', "/").to_lowercase()
}

fn relative_to(root: &Path, file: &str) -> String {
    let (r, f) = (norm(&root.display().to_string()), norm(file));
    let r = r.trim_end_matches('/');
    match f.strip_prefix(&format!("{r}/")) {
        Some(_) => file
            .replace('\\', "/")
            .chars()
            .skip(r.chars().count() + 1)
            .collect(),
        None => file.replace('\\', "/"),
    }
}

/// Parses the output of `php -l -d display_errors=1`. `No syntax errors detected` gives nothing.
pub fn parse_lint(output: &str, root: &Path) -> Vec<UpgradeItem> {
    let mut out = Vec::new();
    for raw in output.lines() {
        let line = raw.trim();
        let Some(pos_line) = line.rfind(" on line ") else {
            continue;
        };
        let Ok(n) = line[pos_line + " on line ".len()..]
            .trim()
            .trim_end_matches('.')
            .parse::<u32>()
        else {
            continue;
        };
        let head = &line[..pos_line];
        let Some(pos_in) = head.rfind(" in ") else {
            continue;
        };
        let (message, file) = (&head[..pos_in], &head[pos_in + 4..]);
        let message = message.strip_prefix("PHP ").unwrap_or(message).trim();
        let lower = message.to_lowercase();
        let level = if lower.starts_with("parse error") || lower.starts_with("fatal error") {
            "blocker"
        } else if lower.starts_with("deprecated") || lower.starts_with("warning") {
            "warning"
        } else {
            continue;
        };
        let rel = relative_to(root, file.trim());
        out.push(UpgradeItem {
            level: level.into(),
            title: message.to_string(),
            detail: String::new(),
            file: Some(rel),
            line: Some(n),
        });
    }
    out
}

/// `phpcs --report=json`: `ERROR` is a blocker, `WARNING` a warning.
pub fn parse_phpcs_json(text: &str, root: &Path) -> Option<Vec<UpgradeItem>> {
    let v: serde_json::Value = serde_json::from_str(text.trim()).ok()?;
    let files = v.get("files")?.as_object()?;
    let mut out = Vec::new();
    for (file, entry) in files {
        for m in entry.get("messages")?.as_array()? {
            let level = match m.get("type").and_then(|t| t.as_str()) {
                Some("ERROR") => "blocker",
                Some("WARNING") => "warning",
                _ => continue,
            };
            out.push(UpgradeItem {
                level: level.into(),
                title: m
                    .get("message")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
                detail: m
                    .get("source")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
                file: Some(relative_to(root, file)),
                line: m.get("line").and_then(|x| x.as_u64()).map(|n| n as u32),
            });
        }
    }
    Some(out)
}

// ------------------------------------------------------------------ files and extensions

fn skipped(rel: &str) -> bool {
    let rel = rel.to_lowercase();
    if SKIP_PREFIXES
        .iter()
        .any(|p| rel == *p || rel.starts_with(&format!("{p}/")))
    {
        return true;
    }
    rel.split('/').any(|c| SKIP_DIRS.contains(&c))
}

/// Every `*.php` under `root` outside the skip list, links never followed. The bool says the
/// limit was reached.
pub fn collect_php_files(root: &Path, limit: usize) -> (Vec<PathBuf>, bool) {
    let mut files = Vec::new();
    let mut stack = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, rel)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let child_rel = if rel.is_empty() {
                name.clone()
            } else {
                format!("{rel}/{name}")
            };
            if skipped(&child_rel) {
                continue;
            }
            let Ok(md) = std::fs::symlink_metadata(e.path()) else {
                continue;
            };
            if md.file_type().is_symlink() {
                continue;
            }
            if md.is_dir() {
                stack.push((e.path(), child_rel));
            } else if name.to_lowercase().ends_with(".php") {
                if files.len() >= limit {
                    return (files, true);
                }
                files.push(e.path());
            }
        }
    }
    files.sort();
    (files, false)
}

/// `ext-*` names a project asks for (`composer.json` require and require-dev, and every
/// locked package's require and platform).
pub fn required_extensions(root: &Path) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut take = |v: &serde_json::Value| {
        if let Some(map) = v.as_object() {
            for k in map.keys() {
                if let Some(ext) = k.strip_prefix("ext-") {
                    out.insert(ext.to_lowercase());
                }
            }
        }
    };
    if let Ok(t) = std::fs::read_to_string(root.join("composer.json")) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&t) {
            for key in ["require", "require-dev"] {
                if let Some(r) = v.get(key) {
                    take(r);
                }
            }
        }
    }
    if let Ok(t) = std::fs::read_to_string(root.join("composer.lock")) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&t) {
            if let Some(p) = v.get("platform") {
                take(p);
            }
            for key in ["packages", "packages-dev"] {
                for pkg in v.get(key).and_then(|x| x.as_array()).into_iter().flatten() {
                    if let Some(r) = pkg.get("require") {
                        take(r);
                    }
                }
            }
        }
    }
    out
}

/// Is a PECL build of `ext` pinned for this PHP minor (`8.4`)? Then OLS can install it.
fn pecl_available(ext: &str, minor: &str) -> bool {
    let data = include_str!("../catalog/pecl-pins.json");
    let prefix = format!("\"php_{ext}-");
    let infix = format!("-{minor}-");
    data.lines()
        .any(|l| l.trim_start().starts_with(&prefix) && l.contains(&infix))
}

/// Pure extension comparison. `enabled`/`disabled` are the DLL extensions of the target PHP.
pub fn extension_items(
    wanted: &BTreeSet<String>,
    enabled: &HashSet<String>,
    disabled: &HashSet<String>,
    target: &str,
    pecl: &dyn Fn(&str) -> bool,
) -> Vec<(String, UpgradeItem)> {
    let mut out = Vec::new();
    for ext in wanted {
        if enabled.contains(ext) || BUILTIN_EXTENSIONS.contains(&ext.as_str()) {
            continue;
        }
        let it = if disabled.contains(ext) || pecl(ext) {
            item(
                "warning",
                format!("Turn on {ext} for PHP {target}"),
                "Runtimes \u{2192} PHP \u{2192} Extensions",
            )
        } else {
            item(
                "blocker",
                format!("The {ext} extension is not available for PHP {target}"),
                "It is not shipped with this PHP and OLS has no build of it.",
            )
        };
        out.push((ext.clone(), it));
    }
    out
}

/// No blockers means the site can switch.
pub fn can_switch(items: &[UpgradeItem]) -> bool {
    !items.iter().any(|i| i.level == "blocker")
}

fn minor_of(version: &str) -> String {
    version.split('.').take(2).collect::<Vec<_>>().join(".")
}

// ------------------------------------------------------------------ the check

impl Inner {
    /// Runs the upgrade check. Call from a background task.
    pub fn check_php_upgrade(
        &self,
        hostname: &str,
        target: &str,
        h: &TaskHandle,
    ) -> Result<PhpUpgradeReport, CoreError> {
        let domain = self.domains.lock().unwrap().get(hostname).ok_or_else(|| {
            CoreError::failed_fix(
                "OLS cannot check that site.",
                format!("No site called \"{hostname}\" exists."),
                "Pick a site from the Sites page.",
            )
        })?;
        let SiteKind::Php { version } = &domain.kind else {
            return Err(CoreError::failed_fix(
                "OLS can only check PHP sites.",
                format!("{hostname} does not run PHP."),
                "Open a PHP site and try again.",
            ));
        };
        let from = version
            .clone()
            .or_else(|| self.php.pick_version(None))
            .unwrap_or_default();
        if !self.php.all_versions().iter().any(|v| v == target) {
            return Err(CoreError::failed_fix(
                format!("PHP {target} is not installed."),
                "The check runs your code with the new PHP, so it must be installed.",
                format!("Install PHP {target} on the Runtimes page first."),
            ));
        }
        let php_exe = self.php.install_dir_of(target).join("php.exe");
        if !php_exe.is_file() {
            return Err(CoreError::failed_fix(
                format!("PHP {target} is incomplete."),
                format!("{} is missing.", php_exe.display()),
                "Reinstall that PHP version on the Runtimes page.",
            ));
        }
        let root = PathBuf::from(&domain.root);
        let mut env = Vec::new();
        if let Ok(ini) = self.php.write_ini(target) {
            env.push(("PHPRC".to_string(), ini.display().to_string()));
        }
        let mut items: Vec<UpgradeItem> = Vec::new();

        // 1. Composer platform check with the target PHP.
        h.begin_step("Checking Composer packages");
        let mut composer_ext_missing: HashSet<String> = HashSet::new();
        if root.join("composer.json").is_file() {
            let phar = self
                .runtimes
                .installed_versions("composer")
                .into_iter()
                .next()
                .and_then(|v| self.runtimes.binary_path("composer", &v));
            match phar {
                Some(phar) => {
                    let mut cenv = env.clone();
                    let home = self.paths.services_dir().join("composer");
                    let _ = std::fs::create_dir_all(&home);
                    cenv.push(("COMPOSER_HOME".into(), home.display().to_string()));
                    cenv.push(("COMPOSER_NO_INTERACTION".into(), "1".into()));
                    let lock = root.join("composer.lock").is_file();
                    let run = |json: bool| {
                        let mut args = vec![
                            phar.display().to_string(),
                            "check-platform-reqs".to_string(),
                            "--no-interaction".to_string(),
                        ];
                        if lock {
                            args.push("--lock".into());
                        }
                        if json {
                            args.push("--format=json".into());
                        }
                        crate::exec::run_capture(
                            &php_exe,
                            &args,
                            Some(&root),
                            &cenv,
                            Duration::from_secs(120),
                        )
                    };
                    let first = run(true);
                    let rows = parse_platform_json(&first.stdout).unwrap_or_else(|| {
                        let second = run(false);
                        parse_platform_text(&format!("{}\n{}", second.stdout, second.stderr))
                    });
                    for r in &rows {
                        if r.status == "missing" {
                            if let Some(ext) = r.name.strip_prefix("ext-") {
                                composer_ext_missing.insert(ext.to_lowercase());
                            }
                        }
                    }
                    items.extend(platform_items(&rows, target));
                    h.end_step(Some(format!("{} requirements read", rows.len())));
                }
                None => {
                    items.push(item(
                        "info",
                        "Composer is not installed in OLS",
                        "Install Composer on the Runtimes page to check packages against the new PHP.",
                    ));
                    h.end_step(Some("Composer not installed".into()));
                }
            }
        } else {
            h.end_step(Some("no composer.json".into()));
        }

        // 2. Extensions.
        h.begin_step("Checking extensions");
        let wanted = required_extensions(&root);
        let all = self.php.extensions(target);
        let enabled: HashSet<String> = all
            .iter()
            .filter(|e| e.enabled)
            .map(|e| e.name.clone())
            .collect();
        let disabled: HashSet<String> = all
            .iter()
            .filter(|e| !e.enabled)
            .map(|e| e.name.clone())
            .collect();
        let minor = minor_of(target);
        let ext_items = extension_items(&wanted, &enabled, &disabled, target, &|e| {
            pecl_available(e, &minor)
        });
        for (ext, it) in ext_items {
            if composer_ext_missing.contains(&ext) {
                // Composer already reported it as a blocker; the clearer message replaces it.
                items.retain(|x| x.title != format!("ext-{ext} is missing for PHP {target}"));
            }
            items.push(it);
        }
        h.end_step(Some(format!("{} extensions asked for", wanted.len())));

        // 3. Compile check.
        let (files, truncated) = collect_php_files(&root, MAX_FILES);
        h.set_phase(&format!("Checking {} PHP files", files.len()), files.len());
        let next = AtomicUsize::new(0);
        let found: std::sync::Mutex<Vec<UpgradeItem>> = std::sync::Mutex::new(Vec::new());
        std::thread::scope(|s| {
            for _ in 0..LINT_THREADS {
                s.spawn(|| loop {
                    if h.cancel_requested() {
                        return;
                    }
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    let Some(file) = files.get(i) else { return };
                    let out = crate::exec::run_capture(
                        &php_exe,
                        &[
                            "-l".into(),
                            "-d".into(),
                            "display_errors=1".into(),
                            "-d".into(),
                            "error_reporting=E_ALL".into(),
                            file.display().to_string(),
                        ],
                        Some(&root),
                        &env,
                        LINT_TIMEOUT,
                    );
                    let mut parsed = parse_lint(&out.combined(), &root);
                    if !parsed.is_empty() {
                        found.lock().unwrap().append(&mut parsed);
                    }
                    h.advance(1);
                });
            }
        });
        if h.cancel_requested() {
            return Err(CoreError::failed(
                "The check was stopped.",
                "It did not finish, so no report was made.",
            ));
        }
        items.extend(found.into_inner().unwrap());
        h.end_step(Some(format!("{} files checked", files.len())));

        // 4. PHPCompatibility, only when the project already has it.
        let phpcs = root.join("vendor/bin/phpcs");
        let phpcs_bat = root.join("vendor/bin/phpcs.bat");
        if (phpcs.exists() || phpcs_bat.exists()) && root.join("vendor/phpcompatibility").is_dir() {
            h.begin_step("Running PHPCompatibility");
            let out = crate::exec::run_capture(
                &php_exe,
                &[
                    phpcs.display().to_string(),
                    "-q".into(),
                    "--report=json".into(),
                    "--standard=PHPCompatibility".into(),
                    "--runtime-set".into(),
                    "testVersion".into(),
                    format!("{target}-"),
                    "--ignore=vendor/*,node_modules/*,storage/*".into(),
                    ".".into(),
                ],
                Some(&root),
                &env,
                Duration::from_secs(300),
            );
            match parse_phpcs_json(&out.stdout, &root) {
                Some(mut v) => items.append(&mut v),
                None => items.push(item(
                    "info",
                    "PHPCompatibility gave no readable answer",
                    "Run it yourself from the project to see why.",
                )),
            }
            h.end_step(None);
        } else {
            items.push(item(
                "info",
                "For deeper checks, add phpcompatibility/php-compatibility as a dev dependency",
                "OLS only uses it when the project already has it. It never installs it.",
            ));
        }

        let rank = |l: &str| match l {
            "blocker" => 0,
            "warning" => 1,
            _ => 2,
        };
        items.sort_by_key(|i| rank(&i.level));
        items.truncate(MAX_ITEMS);
        let report = PhpUpgradeReport {
            hostname: hostname.to_string(),
            from,
            to: target.to_string(),
            checked_ms: crate::ca::unix_now() * 1000,
            files_checked: files.len() as u32,
            truncated,
            can_switch: can_switch(&items),
            items,
        };
        self.php_upgrade
            .lock()
            .unwrap()
            .insert(hostname.to_string(), report.clone());
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composer_json_rows_become_blockers() {
        let json = r#"[
          {"name":"php","version":"8.4.1","status":"failed","failed_requirement":{"source":"laravel/framework","type":"requires","target":"php","constraint":"^8.0.2 <8.3"},"provider":null},
          {"name":"ext-mbstring","version":"8.4.1","status":"success","failed_requirement":null,"provider":null},
          {"name":"ext-redis","version":"n/a","status":"missing","failed_requirement":{"source":"app","type":"requires","target":"ext-redis","constraint":"*"},"provider":null}
        ]"#;
        let rows = parse_platform_json(json).unwrap();
        assert_eq!(rows.len(), 3);
        let items = platform_items(&rows, "8.4");
        assert_eq!(items.len(), 2);
        assert!(items[0]
            .detail
            .contains("laravel/framework requires php ^8.0.2 <8.3"));
        assert!(items[1].title.contains("ext-redis is missing"));
        assert!(items.iter().all(|i| i.level == "blocker"));
        assert!(parse_platform_json("not json").is_none());
    }

    #[test]
    fn composer_text_rows_are_read() {
        let text = "Checking platform requirements for packages in the vendor dir\r\nphp       8.4.1  success\r\next-gd    n/a    missing\r\next-foo   1.0    failed   provided by x\r\n";
        let rows = parse_platform_text(text);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1].status, "missing");
        assert_eq!(rows[2].detail.as_deref(), Some("provided by x"));
    }

    #[test]
    fn lint_output_with_windows_paths_and_crlf() {
        let root = Path::new(r"C:\sites\shop");
        let out = "PHP Parse error:  syntax error, unexpected token \"}\" in C:\\sites\\shop\\app\\Foo.php on line 12\r\nDeprecated: Implicitly marking parameter $x as nullable is deprecated in C:\\sites\\shop\\app\\Bar.php on line 7\r\nNo syntax errors detected in C:\\sites\\shop\\ok.php\r\n";
        let items = parse_lint(out, root);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].level, "blocker");
        assert_eq!(items[0].file.as_deref(), Some("app/Foo.php"));
        assert_eq!(items[0].line, Some(12));
        assert_eq!(items[1].level, "warning");
        assert_eq!(items[1].file.as_deref(), Some("app/Bar.php"));
        assert!(parse_lint("No syntax errors detected in x.php", root).is_empty());
    }

    #[test]
    fn phpcs_errors_and_warnings() {
        let json = r#"{"totals":{"errors":1,"warnings":1},"files":{"C:\\sites\\shop\\a.php":{"errors":1,"warnings":1,"messages":[
          {"message":"Function each() is removed","source":"PHPCompatibility.FunctionUse.RemovedFunctions","severity":5,"type":"ERROR","line":3,"column":1},
          {"message":"Deprecated thing","source":"x","severity":5,"type":"WARNING","line":9,"column":1}]}}}"#;
        let items = parse_phpcs_json(json, Path::new(r"C:\sites\shop")).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].level, "blocker");
        assert_eq!(items[0].file.as_deref(), Some("a.php"));
        assert_eq!(items[1].level, "warning");
        assert!(parse_phpcs_json("garbage", Path::new(".")).is_none());
    }

    #[test]
    fn skip_list_really_skips_vendor_and_caches() {
        let t = tempfile::tempdir().unwrap();
        let w = |rel: &str, body: &str| {
            let p = t.path().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        };
        w("app/Ok.php", "<?php");
        w("vendor/x.php", "<?php broken(");
        w("node_modules/y/z.php", "<?php");
        w("storage/framework/a.php", "<?php");
        w("bootstrap/cache/services.php", "<?php");
        w("wp-content/cache/c.php", "<?php");
        w("src/deep/Real.PHP", "<?php");
        let (files, truncated) = collect_php_files(t.path(), 100);
        let names: Vec<String> = files
            .iter()
            .map(|f| {
                f.strip_prefix(t.path())
                    .unwrap()
                    .display()
                    .to_string()
                    .replace('\\', "/")
            })
            .collect();
        assert!(!truncated);
        assert_eq!(
            names,
            vec!["app/Ok.php".to_string(), "src/deep/Real.PHP".to_string()]
        );
        let (_, truncated) = collect_php_files(t.path(), 1);
        assert!(truncated);
    }

    #[test]
    fn extensions_missing_but_installable_are_warnings() {
        let wanted: BTreeSet<String> = ["gd", "redis", "weirdo", "json", "intl"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let enabled: HashSet<String> = ["gd".to_string()].into();
        let disabled: HashSet<String> = ["intl".to_string()].into();
        let items = extension_items(&wanted, &enabled, &disabled, "8.4", &|e| e == "redis");
        let level = |n: &str| {
            items
                .iter()
                .find(|(e, _)| e == n)
                .map(|(_, i)| i.level.clone())
        };
        assert_eq!(level("gd"), None, "enabled");
        assert_eq!(level("json"), None, "built in");
        assert_eq!(level("intl").as_deref(), Some("warning"), "shipped but off");
        assert_eq!(level("redis").as_deref(), Some("warning"), "installable");
        assert_eq!(level("weirdo").as_deref(), Some("blocker"));
    }

    #[test]
    fn required_extensions_come_from_json_and_lock() {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(
            t.path().join("composer.json"),
            r#"{"require":{"php":"^8.1","ext-gd":"*"},"require-dev":{"ext-xdebug":"*"}}"#,
        )
        .unwrap();
        std::fs::write(
            t.path().join("composer.lock"),
            r#"{"packages":[{"name":"a/b","require":{"ext-curl":"*","php":">=8"}}],"platform":{"ext-intl":"*"}}"#,
        )
        .unwrap();
        let got = required_extensions(t.path());
        assert_eq!(
            got.into_iter().collect::<Vec<_>>(),
            vec!["curl", "gd", "intl", "xdebug"]
        );
    }

    #[test]
    fn can_switch_is_false_with_any_blocker() {
        let w = item("warning", "w", "");
        let b = item("blocker", "b", "");
        assert!(can_switch(&[]));
        assert!(can_switch(std::slice::from_ref(&w)));
        assert!(!can_switch(&[w, b]));
    }

    #[test]
    fn pecl_pins_are_matched_by_minor() {
        assert!(pecl_available("apcu", "8.1"));
        assert!(!pecl_available("apcu", "5.0"));
        assert!(!pecl_available("nonexistentext", "8.1"));
    }
}
