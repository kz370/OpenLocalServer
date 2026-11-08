//! Dependency audit (§169, part B). On demand only, in a background task, one project after
//! another. It asks Composer, npm, pnpm and (when already installed in the project's virtual
//! environment) pip-audit what they know about the packages in the lock files. It never
//! installs anything and never runs at startup or on a timer.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::app::Inner;
use crate::error::CoreError;
use crate::snapshots::is_plain_file_name;
use crate::tasks::TaskHandle;

const TIMEOUT: Duration = Duration::from_secs(120);
const MAX_TITLE: usize = 300;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditFinding {
    pub package: String,
    /// critical | high | moderate | low | unknown
    pub severity: String,
    pub title: String,
    #[serde(default)]
    pub advisory: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub affected: Option<String>,
    #[serde(default)]
    pub fix: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditTool {
    /// composer | npm | pnpm | pip-audit
    pub tool: String,
    pub ok: bool,
    #[serde(default)]
    pub error: Option<String>,
    pub findings: Vec<AuditFinding>,
    pub info: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditResult {
    pub project_id: String,
    pub checked_ms: u64,
    pub tools: Vec<AuditTool>,
}

// ------------------------------------------------------------------ pure parsers

fn severity(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "critical" => "critical",
        "high" => "high",
        "moderate" | "medium" => "moderate",
        "low" | "info" => "low",
        _ => "unknown",
    }
    .into()
}

fn clean_title(t: &str) -> String {
    t.trim().chars().take(MAX_TITLE).collect()
}

/// Only plain https links are kept; anything else (`javascript:`, `file:`) is dropped.
fn clean_url(u: Option<&str>) -> Option<String> {
    u.map(str::trim)
        .filter(|u| u.starts_with("https://") && !u.contains(char::is_whitespace))
        .map(str::to_string)
}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_string)
}

/// `composer audit --format=json`. Exit code 1 (vulnerable) and 2 (abandoned) are normal, so
/// success here means "the output is JSON".
pub fn parse_composer(stdout: &str) -> Result<AuditTool, String> {
    let v: Value = serde_json::from_str(stdout.trim())
        .map_err(|e| format!("Composer did not answer with JSON: {e}"))?;
    let mut tool = AuditTool {
        tool: "composer".into(),
        ok: true,
        error: None,
        findings: vec![],
        info: vec![],
    };
    if let Some(map) = v.get("advisories").and_then(Value::as_object) {
        for (package, list) in map {
            for a in list.as_array().into_iter().flatten() {
                tool.findings.push(AuditFinding {
                    package: text(a, "packageName").unwrap_or_else(|| package.clone()),
                    severity: severity(a.get("severity").and_then(Value::as_str).unwrap_or("")),
                    title: clean_title(&text(a, "title").unwrap_or_default()),
                    advisory: text(a, "cve").or_else(|| text(a, "advisoryId")),
                    url: clean_url(a.get("link").and_then(Value::as_str)),
                    affected: text(a, "affectedVersions"),
                    fix: None,
                });
            }
        }
    }
    if let Some(map) = v.get("abandoned").and_then(Value::as_object) {
        for (package, replacement) in map {
            tool.findings.push(AuditFinding {
                package: package.clone(),
                severity: "low".into(),
                title: "This package is abandoned.".into(),
                advisory: None,
                url: None,
                affected: None,
                fix: replacement.as_str().map(|r| format!("Use {r} instead")),
            });
        }
    }
    Ok(tool)
}

fn npm_like(v: &Value, tool_name: &str) -> Option<AuditTool> {
    let mut tool = AuditTool {
        tool: tool_name.into(),
        ok: true,
        error: None,
        findings: vec![],
        info: vec![],
    };
    if let Some(map) = v.get("vulnerabilities").and_then(Value::as_object) {
        for (name, entry) in map {
            let via: Vec<&Value> = entry
                .get("via")
                .and_then(Value::as_array)
                .map(|a| a.iter().collect())
                .unwrap_or_default();
            let detail = via.iter().find(|x| x.is_object());
            let title = detail
                .and_then(|d| text(d, "title"))
                .or_else(|| {
                    via.iter()
                        .find_map(|x| x.as_str())
                        .map(|dep| format!("Depends on {dep}, which has a known problem."))
                })
                .unwrap_or_else(|| "A known vulnerability.".into());
            let fix = match entry.get("fixAvailable") {
                Some(Value::Object(f)) => {
                    let n = f.get("name").and_then(Value::as_str).unwrap_or(name);
                    let ver = f
                        .get("version")
                        .and_then(Value::as_str)
                        .unwrap_or("a newer version");
                    Some(format!("Update {n} to {ver}"))
                }
                Some(Value::Bool(true)) => Some(format!("Run `{tool_name} audit fix`")),
                _ => None,
            };
            tool.findings.push(AuditFinding {
                package: name.clone(),
                severity: severity(entry.get("severity").and_then(Value::as_str).unwrap_or("")),
                title: clean_title(&title),
                advisory: detail
                    .and_then(|d| d.get("source"))
                    .map(|s| s.to_string().trim_matches('"').to_string()),
                url: clean_url(detail.and_then(|d| d.get("url")).and_then(Value::as_str)),
                affected: text(entry, "range"),
                fix,
            });
        }
        return Some(tool);
    }
    if let Some(map) = v.get("advisories").and_then(Value::as_object) {
        for (_, a) in map {
            tool.findings.push(AuditFinding {
                package: text(a, "module_name").unwrap_or_default(),
                severity: severity(a.get("severity").and_then(Value::as_str).unwrap_or("")),
                title: clean_title(&text(a, "title").unwrap_or_default()),
                advisory: a
                    .get("github_advisory_id")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                url: clean_url(a.get("url").and_then(Value::as_str)),
                affected: text(a, "vulnerable_versions"),
                fix: text(a, "patched_versions").map(|p| format!("Update to {p}")),
            });
        }
        return Some(tool);
    }
    None
}

/// `npm audit --json` (v2 shape).
pub fn parse_npm(stdout: &str) -> Result<AuditTool, String> {
    let v: Value = serde_json::from_str(stdout.trim())
        .map_err(|e| format!("npm did not answer with JSON: {e}"))?;
    if let Some(msg) = v
        .get("error")
        .and_then(|e| e.get("summary"))
        .and_then(Value::as_str)
    {
        return Err(clean_title(msg));
    }
    npm_like(&v, "npm")
        .ok_or_else(|| "npm audit answered in a shape OLS does not know.".to_string())
}

/// `pnpm audit --json`: the legacy `advisories` shape or npm's `vulnerabilities` shape. An
/// unknown shape is reported as `Unknown`, not as a failure.
pub fn parse_pnpm(stdout: &str) -> Result<AuditTool, String> {
    let v: Value = serde_json::from_str(stdout.trim())
        .map_err(|e| format!("pnpm did not answer with JSON: {e}"))?;
    Ok(npm_like(&v, "pnpm").unwrap_or_else(|| AuditTool {
        tool: "pnpm".into(),
        ok: true,
        error: None,
        findings: vec![AuditFinding {
            package: "(unknown)".into(),
            severity: "unknown".into(),
            title: "pnpm answered in a shape OLS does not know, so the result could not be read."
                .into(),
            advisory: None,
            url: None,
            affected: None,
            fix: None,
        }],
        info: vec![],
    }))
}

/// `pip-audit -f json`: `{"dependencies": [...]}` or a bare list.
pub fn parse_pip_audit(stdout: &str) -> Result<AuditTool, String> {
    let v: Value = serde_json::from_str(stdout.trim())
        .map_err(|e| format!("pip-audit did not answer with JSON: {e}"))?;
    let deps = v.get("dependencies").cloned().unwrap_or(v);
    let mut tool = AuditTool {
        tool: "pip-audit".into(),
        ok: true,
        error: None,
        findings: vec![],
        info: vec![],
    };
    for d in deps.as_array().into_iter().flatten() {
        let name = text(d, "name").unwrap_or_default();
        for vuln in d
            .get("vulns")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let fixes: Vec<&str> = vuln
                .get("fix_versions")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            tool.findings.push(AuditFinding {
                package: name.clone(),
                severity: "unknown".into(),
                title: clean_title(
                    &text(vuln, "description").unwrap_or_else(|| "A known vulnerability.".into()),
                ),
                advisory: text(vuln, "id"),
                url: None,
                affected: text(d, "version"),
                fix: (!fixes.is_empty()).then(|| format!("Update to {}", fixes.join(" or "))),
            });
        }
    }
    Ok(tool)
}

// ------------------------------------------------------------------ storage

fn audit_dir(inner: &Inner) -> PathBuf {
    inner.paths.data_dir().join("audit")
}

fn store(inner: &Inner, result: &AuditResult) -> Result<(), CoreError> {
    if !is_plain_file_name(&result.project_id) {
        return Err(CoreError::failed_fix(
            "OLS cannot save the audit result.",
            "The project id is not a plain name.",
            "Re-add the project, then run the check again.",
        ));
    }
    let dir = audit_dir(inner);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(
        dir.join(format!("{}.json", result.project_id)),
        serde_json::to_string_pretty(result)?,
    )?;
    Ok(())
}

/// The newest result of every project that was checked.
pub fn list_results(inner: &Inner) -> Vec<AuditResult> {
    let Ok(rd) = std::fs::read_dir(audit_dir(inner)) else {
        return Vec::new();
    };
    let mut out: Vec<AuditResult> = rd
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| serde_json::from_str(&std::fs::read_to_string(e.path()).ok()?).ok())
        .collect();
    out.sort_by(|a, b| b.checked_ms.cmp(&a.checked_ms));
    out
}

// ------------------------------------------------------------------ running

fn run_tool(
    inner: &Inner,
    project_id: &str,
    root: &Path,
    program: &str,
    args: &[&str],
) -> Result<crate::exec::Captured, String> {
    let owned: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    let (exe, full, env) = inner
        .project_program(program, &owned, Some(project_id))
        .map_err(|e| {
            format!(
                "{program} is not available for this project: {}",
                crate::error::Diagnostic::from(&e).cause
            )
        })?;
    Ok(crate::exec::run_capture(
        &exe,
        &full,
        Some(root),
        &env,
        TIMEOUT,
    ))
}

fn failed(tool: &str, why: String) -> AuditTool {
    AuditTool {
        tool: tool.into(),
        ok: false,
        error: Some(crate::redact::redact_text(&why)),
        findings: vec![],
        info: vec![],
    }
}

/// Checks one project with whatever applies to it.
pub fn audit_project(inner: &Inner, project_id: &str) -> Result<AuditResult, CoreError> {
    let project = inner.ensure_project_available(project_id)?;
    let root = PathBuf::from(&project.path);
    let mut tools = Vec::new();

    if root.join("composer.lock").is_file() {
        tools.push(
            match run_tool(
                inner,
                project_id,
                &root,
                "composer",
                &["audit", "--format=json", "--locked", "--no-interaction"],
            ) {
                Ok(out) if out.timed_out => {
                    failed("composer", "Composer took longer than 2 minutes.".into())
                }
                Ok(out) => parse_composer(&out.stdout)
                    .unwrap_or_else(|e| failed("composer", format!("{e} {}", out.stderr.trim()))),
                Err(e) => failed("composer", e),
            },
        );
    }
    if root.join("package-lock.json").is_file() {
        tools.push(
            match run_tool(
                inner,
                project_id,
                &root,
                "npm",
                &["audit", "--json", "--package-lock-only"],
            ) {
                Ok(out) if out.timed_out => failed("npm", "npm took longer than 2 minutes.".into()),
                Ok(out) => parse_npm(&out.stdout).unwrap_or_else(|e| failed("npm", e)),
                Err(e) => failed("npm", e),
            },
        );
    }
    if root.join("pnpm-lock.yaml").is_file() {
        tools.push(
            match run_tool(inner, project_id, &root, "pnpm", &["audit", "--json"]) {
                Ok(out) if out.timed_out => {
                    failed("pnpm", "pnpm took longer than 2 minutes.".into())
                }
                Ok(out) => parse_pnpm(&out.stdout).unwrap_or_else(|e| failed("pnpm", e)),
                Err(e) => failed("pnpm", e),
            },
        );
    }
    if root.join("yarn.lock").is_file() {
        tools.push(AuditTool {
            tool: "yarn".into(),
            ok: true,
            error: None,
            findings: vec![],
            info: vec!["Yarn projects are not checked yet.".into()],
        });
    }
    if root.join("requirements.txt").is_file() || root.join("pyproject.toml").is_file() {
        let pip_audit = crate::venv::find_dir(&root)
            .map(|v| crate::venv::scripts_dir(&v).join("pip-audit.exe"))
            .filter(|p| p.is_file());
        tools.push(match pip_audit {
            Some(exe) => {
                let out = crate::exec::run_capture(&exe, &["-f".to_string(), "json".to_string()], Some(&root), &[], TIMEOUT);
                if out.timed_out {
                    failed("pip-audit", "pip-audit took longer than 2 minutes.".into())
                } else {
                    parse_pip_audit(&out.stdout).unwrap_or_else(|e| failed("pip-audit", e))
                }
            }
            None => AuditTool {
                tool: "pip-audit".into(),
                ok: true,
                error: None,
                findings: vec![],
                info: vec!["Install pip-audit in the project's virtual environment to check Python packages.".into()],
            },
        });
    }

    let result = AuditResult {
        project_id: project_id.to_string(),
        checked_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0),
        tools,
    };
    store(inner, &result)?;
    Ok(result)
}

/// The task body: one project, or all of them one after another.
pub fn run_task(
    inner: &Inner,
    h: &TaskHandle,
    project_id: Option<&str>,
) -> Result<String, CoreError> {
    if !crate::network::check(false).online {
        return Err(CoreError::failed_fix(
            "OLS cannot check packages while you are offline.",
            "These checks ask the package registries for known problems.",
            "Connect to the internet and try again.",
        ));
    }
    let ids: Vec<(String, String)> = match project_id {
        Some(id) => {
            let p = inner.ensure_project_available(id)?;
            vec![(p.id, p.name)]
        }
        None => inner
            .projects
            .lock()
            .unwrap()
            .list()
            .into_iter()
            .map(|p| (p.id, p.name))
            .collect(),
    };
    let (mut checked, mut vulnerable) = (0usize, 0usize);
    for (id, name) in ids {
        h.check_cancelled()?;
        h.begin_step(&format!("Checking {name}"));
        match audit_project(inner, &id) {
            Ok(r) => {
                checked += 1;
                let n: usize = r.tools.iter().map(|t| t.findings.len()).sum();
                if n > 0 {
                    vulnerable += 1;
                    h.result(format!("{name}: {n} finding(s)"));
                }
            }
            Err(e) => h.problem(format!(
                "{name}: {}",
                crate::error::Diagnostic::from(&e).problem
            )),
        }
    }
    Ok(format!(
        "Checked {checked} project(s); {vulnerable} with findings."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composer_advisories_and_abandoned_packages() {
        let json = r#"{"advisories":{"symfony/http-kernel":[{"advisoryId":"PKSA-1","packageName":"symfony/http-kernel","affectedVersions":">=6.0,<6.4.2","title":"CVE-2024-1: Something bad","cve":"CVE-2024-1","link":"https://symfony.com/cve-2024-1","severity":"high"}]},"abandoned":{"old/pkg":"new/pkg","dead/pkg":null}}"#;
        let t = parse_composer(json).unwrap();
        assert_eq!(t.findings.len(), 3);
        let a = t
            .findings
            .iter()
            .find(|f| f.package == "symfony/http-kernel")
            .unwrap();
        assert_eq!(
            (a.severity.as_str(), a.advisory.as_deref()),
            ("high", Some("CVE-2024-1"))
        );
        assert_eq!(a.url.as_deref(), Some("https://symfony.com/cve-2024-1"));
        let old = t.findings.iter().find(|f| f.package == "old/pkg").unwrap();
        assert_eq!(old.fix.as_deref(), Some("Use new/pkg instead"));
        // No advisories at all is `[]` in PHP's JSON.
        assert!(parse_composer(r#"{"advisories":[],"abandoned":[]}"#)
            .unwrap()
            .findings
            .is_empty());
    }

    #[test]
    fn npm_v2_vulnerabilities() {
        let json = r#"{"vulnerabilities":{"lodash":{"name":"lodash","severity":"critical","via":[{"source":1,"name":"lodash","title":"Prototype Pollution","url":"https://github.com/advisories/GHSA-x","severity":"critical","range":"<4.17.21"}],"range":"<4.17.21","fixAvailable":{"name":"lodash","version":"4.17.21","isSemVerMajor":false}},"express":{"name":"express","severity":"moderate","via":["lodash"],"range":"*","fixAvailable":true}},"metadata":{"vulnerabilities":{"critical":1}}}"#;
        let t = parse_npm(json).unwrap();
        assert_eq!(t.findings.len(), 2);
        let l = t.findings.iter().find(|f| f.package == "lodash").unwrap();
        assert_eq!(l.severity, "critical");
        assert_eq!(l.fix.as_deref(), Some("Update lodash to 4.17.21"));
        let e = t.findings.iter().find(|f| f.package == "express").unwrap();
        assert!(e.title.contains("Depends on lodash"));
        assert_eq!(e.fix.as_deref(), Some("Run `npm audit fix`"));
    }

    #[test]
    fn pnpm_accepts_both_shapes_and_unknown_is_not_a_failure() {
        let legacy = r#"{"advisories":{"1":{"module_name":"minimist","severity":"high","title":"Prototype Pollution","url":"https://npmjs.com/advisories/1","vulnerable_versions":"<1.2.6","patched_versions":">=1.2.6"}}}"#;
        let t = parse_pnpm(legacy).unwrap();
        assert_eq!(t.findings[0].package, "minimist");
        assert_eq!(t.findings[0].fix.as_deref(), Some("Update to >=1.2.6"));
        let v2 = r#"{"vulnerabilities":{"x":{"severity":"low","via":[],"fixAvailable":false}}}"#;
        assert_eq!(parse_pnpm(v2).unwrap().findings.len(), 1);
        let unknown = parse_pnpm(r#"{"something":"else"}"#).unwrap();
        assert!(unknown.ok);
        assert_eq!(unknown.findings[0].severity, "unknown");
    }

    #[test]
    fn pip_audit_both_shapes() {
        let json = r#"{"dependencies":[{"name":"django","version":"3.0","vulns":[{"id":"PYSEC-1","fix_versions":["3.0.1","3.1"],"description":"bad"}]},{"name":"ok","version":"1","vulns":[]}]}"#;
        let t = parse_pip_audit(json).unwrap();
        assert_eq!(t.findings.len(), 1);
        assert_eq!(t.findings[0].fix.as_deref(), Some("Update to 3.0.1 or 3.1"));
        assert_eq!(parse_pip_audit("[]").unwrap().findings.len(), 0);
    }

    #[test]
    fn malformed_output_is_an_error_not_a_panic() {
        assert!(parse_composer("not json").is_err());
        assert!(parse_npm("").is_err());
        assert!(parse_npm(r#"{"error":{"summary":"ENOLOCK"}}"#).is_err());
        assert!(parse_pnpm("<html>").is_err());
        assert!(parse_pip_audit("{").is_err());
    }

    #[test]
    fn unsafe_links_and_long_titles_are_cleaned() {
        let long = "x".repeat(400);
        let json = format!(
            r#"{{"advisories":{{"a/b":[{{"packageName":"a/b","title":"{long}","link":"javascript:alert(1)","severity":"Medium"}}]}}}}"#
        );
        let t = parse_composer(&json).unwrap();
        assert_eq!(t.findings[0].url, None);
        assert_eq!(t.findings[0].title.chars().count(), MAX_TITLE);
        assert_eq!(t.findings[0].severity, "moderate");
        assert_eq!(clean_url(Some("http://insecure.test")), None);
        assert_eq!(
            clean_url(Some("https://ok.test/x")).as_deref(),
            Some("https://ok.test/x")
        );
    }
}
