//! Load testing with k6 (Stage 18). A project keeps its test scripts in `.openlocalserver/k6/*.js`; a run
//! starts k6 against one of the project's own sites and streams its JSON output into live numbers
//! (requests per second, latency percentiles, error rate, virtual users). Each finished run is saved with its
//! result so runs can be compared.
//!
//! Safety, in order of strength:
//! - a script may only name hosts that are this project's sites (a literal `https://other.example` in it is refused),
//!   and the run is given `BASE_URL` for the chosen site;
//! - a tunnel's public address is a valid target only with an explicit confirmation, because that sends real traffic
//!   through the provider;
//! - the virtual-user numbers written in a script must stay under the limit in Settings → Resources (default 200);
//! - k6 runs at below-normal priority so the machine stays usable, and can be stopped at any time.
//!
//! A script can still build a URL at run time in a way no scan can see; the limits above are guard rails against
//! mistakes, not a sandbox for a hostile script.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::app::Inner;
use crate::error::CoreError;

pub const DEFAULT_MAX_VUS: u32 = 200;

fn secret_key(profile: &str, name: &str) -> String {
    format!("loadtest:{profile}:{name}")
}

fn fail(msg: impl Into<String>) -> CoreError {
    CoreError::failed("The load test couldn't run.", msg)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct K6Info {
    pub installed: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    /// Installed by OpenLocalServer (Runtimes), not found on PATH.
    pub managed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptInfo {
    pub name: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadSite {
    pub host: String,
    pub url: String,
    /// A tunnel's public address: real traffic through the provider.
    pub public: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadOverview {
    pub k6: K6Info,
    pub scripts: Vec<ScriptInfo>,
    pub sites: Vec<LoadSite>,
    pub max_vus: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Metrics {
    pub requests: u64,
    pub failed: u64,
    pub error_rate: f64,
    pub rps: f64,
    pub avg_ms: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
    pub vus: u32,
    pub checks_passed: u64,
    pub checks_failed: u64,
    pub iterations: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeriesPoint {
    pub t: u32,
    pub rps: f64,
    pub p95_ms: f64,
    pub vus: u32,
    pub errors: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadRun {
    pub id: String,
    pub project_id: String,
    pub script: String,
    pub target: String,
    /// running, passed, failed (a threshold), error, stopped.
    pub state: String,
    pub started_ms: u64,
    pub finished_ms: Option<u64>,
    pub exit_code: Option<i32>,
    pub metrics: Metrics,
    pub series: Vec<SeriesPoint>,
    /// The end of k6's own messages: threshold results and errors.
    pub output: Vec<String>,
    pub message: Option<String>,
}

struct Live {
    run: LoadRun,
    child: Option<Child>,
    stop_requested: bool,
    durations: Vec<f64>,
    sum_ms: f64,
    buckets: HashMap<u32, Bucket>,
}

#[derive(Default)]
struct Bucket {
    durations: Vec<f64>,
    reqs: u64,
    errors: u64,
    vus: u32,
}

/// The runs of this session; finished runs are also saved to disk.
#[derive(Default)]
pub struct LoadRuns {
    runs: Mutex<HashMap<String, Arc<Mutex<Live>>>>,
}

// ----------------------------------------------------------------------- pure helpers

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((p / 100.0) * (sorted.len() as f64 - 1.0)).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

/// What is wrong with running `script` for a project whose sites are `allowed_hosts`.
pub fn check_script(script: &str, allowed_hosts: &[String], max_vus: u32) -> Result<(), String> {
    let urls = Regex::new(r#"https?://([A-Za-z0-9.\-]+)"#).unwrap();
    for cap in urls.captures_iter(script) {
        let host = cap[1].to_ascii_lowercase();
        if !allowed_hosts.iter().any(|h| h.eq_ignore_ascii_case(&host)) {
            return Err(format!("the script points at {host}, which isn't one of this project's sites. Use __ENV.BASE_URL, or add the site to the project."));
        }
    }
    let numbers =
        Regex::new(r"\b(vus|preAllocatedVUs|maxVUs|target|startVUs)\s*:\s*(\d+)").unwrap();
    for cap in numbers.captures_iter(script) {
        let n: u32 = cap[2].parse().unwrap_or(u32::MAX);
        if n > max_vus {
            return Err(format!("the script asks for {n} virtual users ({}), and the limit in Settings → Resources is {max_vus}.", &cap[1]));
        }
    }
    Ok(())
}

fn script_name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 80
        && name.ends_with(".js")
        && !name.contains(['/', '\\', ':'])
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stage {
    pub duration_s: u32,
    /// Users at the end of the stage; k6 ramps to it.
    pub target: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlannedRequest {
    pub method: String,
    pub path: String,
    /// Sent as the request body (POST, PUT, PATCH, DELETE). May use `{{NAME}}` variables.
    #[serde(default)]
    pub body: Option<String>,
}

/// A header sent with every request. The value may use `{{NAME}}` variables, e.g. `Bearer {{TOKEN}}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Header {
    pub name: String,
    pub value: String,
}

/// A value the script reads with `__ENV.NAME`. It is handed to k6 when the test runs and is never written into the
/// script; a secret one (a token, a password) is kept in the system keyring rather than in the plan's file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Variable {
    pub name: String,
    pub value: String,
    #[serde(default)]
    pub secret: bool,
}

/// Pass criteria; a blank one isn't checked.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Thresholds {
    #[serde(default)]
    pub p95_ms: Option<u32>,
    #[serde(default)]
    pub p99_ms: Option<u32>,
    #[serde(default)]
    pub error_rate_pct: Option<f64>,
}

/// A test plan as a form: how many users, when, what they request, and what counts as passing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoadProfile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub builtin: bool,
    /// A name the UI maps to an icon: smoke, load, stress, spike, soak or custom.
    #[serde(default)]
    pub icon: String,
    pub stages: Vec<Stage>,
    pub think_time_s: f64,
    pub requests: Vec<PlannedRequest>,
    #[serde(default)]
    pub headers: Vec<Header>,
    #[serde(default)]
    pub variables: Vec<Variable>,
    #[serde(default)]
    pub thresholds: Thresholds,
}

fn var_name_ok(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 40
        && n.chars()
            .enumerate()
            .all(|(i, c)| c == '_' || c.is_ascii_alphabetic() || (i > 0 && c.is_ascii_digit()))
}

fn header_name_ok(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 60
        && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        && !["host", "content-length", "connection", "transfer-encoding"]
            .contains(&n.to_ascii_lowercase().as_str())
}

fn placeholders() -> &'static Regex {
    static R: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    R.get_or_init(|| Regex::new(r"\{\{([^{}]*)\}\}").unwrap())
}

/// A JavaScript expression for text that may hold `{{NAME}}`: literal parts as strings, names as `__ENV.NAME`.
fn js_value(text: &str) -> String {
    let mut parts = Vec::new();
    let mut last = 0;
    for m in placeholders().captures_iter(text) {
        let whole = m.get(0).unwrap();
        if whole.start() > last {
            parts.push(serde_json::to_string(&text[last..whole.start()]).unwrap_or_default());
        }
        parts.push(format!("__ENV.{}", m[1].trim()));
        last = whole.end();
    }
    if last < text.len() || parts.is_empty() {
        parts.push(serde_json::to_string(&text[last..]).unwrap_or_default());
    }
    parts.join(" + ")
}

const METHODS: &[&str] = &["GET", "HEAD", "POST", "PUT", "PATCH", "DELETE"];

fn path_ok(p: &str) -> bool {
    p.starts_with('/')
        && p.len() <= 300
        && p.chars()
            .all(|c| c.is_ascii_alphanumeric() || "/_-.?=&%~:@+,#".contains(c))
}

impl LoadProfile {
    /// What is wrong with the plan. Every number is bounded, and paths and methods are plain data, so a script
    /// made from a valid profile contains nothing but those values.
    pub fn validate(&self, max_vus: u32) -> Result<(), String> {
        if self.name.trim().is_empty() || self.name.len() > 60 {
            return Err("give the test a name of up to 60 characters".into());
        }
        if self.stages.is_empty() || self.stages.len() > 20 {
            return Err("a test needs 1 to 20 stages".into());
        }
        let mut total = 0u64;
        for (i, s) in self.stages.iter().enumerate() {
            if s.duration_s == 0 || s.duration_s > 14_400 {
                return Err(format!(
                    "stage {}: the duration must be 1 second to 4 hours",
                    i + 1
                ));
            }
            if s.target > max_vus {
                return Err(format!(
                    "stage {}: {} users is over the limit of {max_vus} in Settings → Resources",
                    i + 1,
                    s.target
                ));
            }
            total += s.duration_s as u64;
        }
        if total > 43_200 {
            return Err("a test can last at most 12 hours".into());
        }
        if self.stages.iter().all(|s| s.target == 0) {
            return Err("at least one stage needs users".into());
        }
        if !(0.0..=60.0).contains(&self.think_time_s) {
            return Err("the pause between requests is 0 to 60 seconds".into());
        }
        if self.requests.is_empty() || self.requests.len() > 20 {
            return Err("a test needs 1 to 20 requests".into());
        }
        for r in &self.requests {
            if !METHODS.contains(&r.method.as_str()) {
                return Err(format!(
                    "'{}' isn't a request method (GET, HEAD, POST, PUT, PATCH, DELETE)",
                    r.method
                ));
            }
            if !path_ok(&r.path) {
                return Err(format!(
                    "'{}' isn't a usable path: start with / and use plain URL characters",
                    r.path
                ));
            }
        }
        let mut seen = std::collections::HashSet::new();
        if self.variables.len() > 30 {
            return Err("at most 30 variables".into());
        }
        for v in &self.variables {
            if !var_name_ok(&v.name) || v.name == "BASE_URL" || !seen.insert(v.name.as_str()) {
                return Err(format!("variable '{}': use letters, digits and _, not starting with a digit, once each (BASE_URL is taken)", v.name));
            }
            if v.value.len() > 4000 || v.value.contains(['\r', '\n', '\0']) {
                return Err(format!(
                    "variable '{}': the value must be one line of up to 4000 characters",
                    v.name
                ));
            }
        }
        let refs_ok = |text: &str, what: &str| -> Result<(), String> {
            for m in placeholders().captures_iter(text) {
                let n = m[1].trim();
                if !self.variables.iter().any(|v| v.name == n) {
                    return Err(format!(
                        "{what} uses {{{{{n}}}}}, which isn't one of the variables"
                    ));
                }
            }
            Ok(())
        };
        if self.headers.len() > 30 {
            return Err("at most 30 headers".into());
        }
        for h in &self.headers {
            if !header_name_ok(&h.name) {
                return Err(format!("'{}' isn't a header name you can set (letters, digits and -; not Host or Content-Length)", h.name));
            }
            if h.value.len() > 2000 || h.value.contains(['\r', '\n', '\0']) {
                return Err(format!(
                    "header {}: the value must be one line of up to 2000 characters",
                    h.name
                ));
            }
            refs_ok(&h.value, &format!("header {}", h.name))?;
        }
        for r in &self.requests {
            if let Some(b) = &r.body {
                if b.len() > 20_000 {
                    return Err(format!(
                        "{}: the body is limited to 20000 characters",
                        r.path
                    ));
                }
                if !b.is_empty() && !["POST", "PUT", "PATCH", "DELETE"].contains(&r.method.as_str())
                {
                    return Err(format!(
                        "{} {}: only POST, PUT, PATCH and DELETE carry a body",
                        r.method, r.path
                    ));
                }
                refs_ok(b, &format!("the body of {}", r.path))?;
            }
        }
        let t = &self.thresholds;
        if t.p95_ms.is_some_and(|v| v == 0 || v > 600_000)
            || t.p99_ms.is_some_and(|v| v == 0 || v > 600_000)
        {
            return Err("a latency limit is 1 ms to 10 minutes".into());
        }
        if t.error_rate_pct
            .is_some_and(|v| !(0.0..=100.0).contains(&v))
        {
            return Err("the error limit is 0 to 100 percent".into());
        }
        Ok(())
    }

    /// The k6 script for this plan.
    pub fn script(&self) -> String {
        let stages = self
            .stages
            .iter()
            .map(|s| {
                format!(
                    "    {{ duration: '{}s', target: {} }},",
                    s.duration_s, s.target
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let mut thresholds = Vec::new();
        if let Some(e) = self.thresholds.error_rate_pct {
            thresholds.push(format!("    http_req_failed: ['rate<{}'],", e / 100.0));
        }
        let mut latency = Vec::new();
        if let Some(v) = self.thresholds.p95_ms {
            latency.push(format!("'p(95)<{v}'"));
        }
        if let Some(v) = self.thresholds.p99_ms {
            latency.push(format!("'p(99)<{v}'"));
        }
        if !latency.is_empty() {
            thresholds.push(format!("    http_req_duration: [{}],", latency.join(", ")));
        }
        let requests = self
            .requests
            .iter()
            .map(|r| {
                format!(
                    "  {{ method: '{}', path: '{}', body: {} }},",
                    r.method,
                    r.path,
                    r.body
                        .as_deref()
                        .filter(|b| !b.is_empty())
                        .map(js_value)
                        .unwrap_or_else(|| "null".into())
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let mut headers: Vec<String> = self
            .headers
            .iter()
            .map(|h| {
                format!(
                    "  {}: {},",
                    serde_json::to_string(&h.name).unwrap_or_default(),
                    js_value(&h.value)
                )
            })
            .collect();
        if !self
            .headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case("content-type"))
        {
            if let Some(body) = self
                .requests
                .iter()
                .filter_map(|r| r.body.as_deref())
                .find(|b| !b.is_empty())
            {
                let kind = if matches!(body.trim_start().chars().next(), Some('{' | '[')) {
                    "application/json"
                } else {
                    "text/plain"
                };
                headers.push(format!("  \"Content-Type\": \"{kind}\","));
            }
        }
        let headers = headers.join("\n");
        let thresholds = thresholds.join("\n");
        format!(
            "// {name}: {desc}\n// Made by OpenLocalServer from the test form. You can edit it; BASE_URL is the site you run it against, and variables come from __ENV.\nimport http from 'k6/http';\nimport {{ check, sleep }} from 'k6';\n\nexport const options = {{\n  stages: [\n{stages}\n  ],\n  thresholds: {{\n{thresholds}\n  }},\n}};\n\nconst BASE = __ENV.BASE_URL;\nconst HEADERS = {{\n{headers}\n}};\nconst REQUESTS = [\n{requests}\n];\n\nexport default function () {{\n  for (const r of REQUESTS) {{\n    const res = http.request(r.method, `${{BASE}}${{r.path}}`, r.body, {{ headers: HEADERS }});\n    check(res, {{ 'status is 2xx or 3xx': (r) => r.status >= 200 && r.status < 400 }});\n  }}\n  sleep({think});\n}}\n",
            name = self.name.replace(['\n', '\r'], " "),
            desc = self.description.replace(['\n', '\r'], " "),
            think = self.think_time_s,
        )
    }
}

fn profile(
    id: &str,
    name: &str,
    icon: &str,
    description: &str,
    stages: &[(u32, u32)],
    think: f64,
    p95: u32,
    err: f64,
) -> LoadProfile {
    LoadProfile {
        id: id.into(),
        name: name.into(),
        description: description.into(),
        builtin: true,
        icon: icon.into(),
        stages: stages
            .iter()
            .map(|&(duration_s, target)| Stage { duration_s, target })
            .collect(),
        think_time_s: think,
        requests: vec![PlannedRequest {
            method: "GET".into(),
            path: "/".into(),
            body: None,
        }],
        headers: vec![],
        variables: vec![],
        thresholds: Thresholds {
            p95_ms: Some(p95),
            p99_ms: None,
            error_rate_pct: Some(err),
        },
    }
}

/// The ready-made plans. A copy can be changed and saved under a new name.
pub fn builtin_profiles() -> Vec<LoadProfile> {
    vec![
        profile(
            "smoke",
            "Smoke",
            "smoke",
            "One user for 30 seconds. Does every page answer at all?",
            &[(1, 1), (29, 1)],
            1.0,
            1000,
            1.0,
        ),
        profile(
            "load",
            "Load",
            "load",
            "Ramp to 10 users, hold for a minute, ramp down. Everyday traffic.",
            &[(30, 10), (60, 10), (30, 0)],
            1.0,
            1000,
            1.0,
        ),
        profile(
            "stress",
            "Stress",
            "stress",
            "Step up to 50 users to find where the site starts to slow down.",
            &[
                (30, 10),
                (60, 10),
                (30, 25),
                (60, 25),
                (30, 50),
                (60, 50),
                (30, 0),
            ],
            1.0,
            2000,
            5.0,
        ),
        profile(
            "spike",
            "Spike",
            "spike",
            "A sudden jump from 5 to 50 users, then back. Does it recover?",
            &[(10, 5), (10, 50), (30, 50), (10, 5), (10, 0)],
            0.5,
            2000,
            5.0,
        ),
        profile(
            "soak",
            "Soak",
            "soak",
            "10 users for 10 minutes. Looks for slow leaks and things that degrade over time.",
            &[(60, 10), (600, 10), (60, 0)],
            1.0,
            1000,
            1.0,
        ),
    ]
}

/// Applies one line of k6's `--out json` stream. Unknown lines are ignored.
fn apply_line(live: &mut Live, line: &str, elapsed_s: u32) {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return;
    };
    if v["type"] != "Point" {
        return;
    }
    let value = v["data"]["value"].as_f64().unwrap_or(0.0);
    let bucket = live.buckets.entry(elapsed_s).or_default();
    match v["metric"].as_str().unwrap_or("") {
        "http_reqs" => {
            live.run.metrics.requests += value as u64;
            bucket.reqs += value as u64;
        }
        "http_req_duration" => {
            live.durations.push(value);
            live.sum_ms += value;
            bucket.durations.push(value);
            if live.durations.len() > 400_000 {
                // Keep memory bounded: every other sample is enough for percentiles.
                let mut i = 0;
                live.durations.retain(|_| {
                    i += 1;
                    i % 2 == 0
                });
            }
        }
        "http_req_failed" if value >= 1.0 => {
            live.run.metrics.failed += 1;
            bucket.errors += 1;
        }
        "checks" => {
            if value >= 1.0 {
                live.run.metrics.checks_passed += 1
            } else {
                live.run.metrics.checks_failed += 1
            }
        }
        "iterations" => live.run.metrics.iterations += value as u64,
        "vus" => {
            live.run.metrics.vus = value as u32;
            bucket.vus = value as u32;
        }
        _ => {}
    }
}

fn refresh_metrics(live: &mut Live, elapsed_s: f64) {
    let mut sorted = live.durations.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let sum_ms = live.sum_ms;
    let m = &mut live.run.metrics;
    m.p50_ms = percentile(&sorted, 50.0);
    m.p95_ms = percentile(&sorted, 95.0);
    m.p99_ms = percentile(&sorted, 99.0);
    m.max_ms = sorted.last().copied().unwrap_or(0.0);
    m.avg_ms = if sorted.is_empty() {
        0.0
    } else {
        sum_ms / (m.requests.max(1) as f64)
    };
    m.rps = if elapsed_s > 0.0 {
        m.requests as f64 / elapsed_s
    } else {
        0.0
    };
    m.error_rate = if m.requests > 0 {
        m.failed as f64 / m.requests as f64
    } else {
        0.0
    };
    let mut series: Vec<SeriesPoint> = live
        .buckets
        .iter()
        .map(|(t, b)| {
            let mut d = b.durations.clone();
            d.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            SeriesPoint {
                t: *t,
                rps: b.reqs as f64,
                p95_ms: percentile(&d, 95.0),
                vus: b.vus,
                errors: b.errors,
            }
        })
        .collect();
    series.sort_by_key(|p| p.t);
    live.run.series = series;
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ------------------------------------------------------------------------- the Inner API

impl Inner {
    fn k6_binary(&self) -> Option<(PathBuf, String, bool)> {
        let managed = self.runtimes.installed_versions("k6");
        if let Some(v) = managed.first() {
            if let Some(p) = self.runtimes.binary_path("k6", v) {
                return Some((p, v.clone(), true));
            }
        }
        crate::runtime::detect_system_install("k6")
            .map(|s| (PathBuf::from(s.path), s.version, false))
    }

    fn load_project_dir(&self, project_id: &str) -> Result<PathBuf, CoreError> {
        let p = self
            .projects
            .lock()
            .unwrap()
            .get(project_id)
            .ok_or_else(|| fail(format!("no project '{project_id}'")))?;
        Ok(PathBuf::from(p.path).join(".openlocalserver").join("k6"))
    }

    fn load_max_vus(&self) -> u32 {
        self.resource_limits().k6_max_vus.unwrap_or(DEFAULT_MAX_VUS)
    }

    /// The project's own sites, and (only for a confirmed public run) its running tunnels.
    fn load_sites(&self, project_id: &str) -> Vec<LoadSite> {
        let mut sites: Vec<LoadSite> = self
            .domain_summaries()
            .into_iter()
            .filter(|d| d.enabled && d.project_id.as_deref() == Some(project_id))
            .map(|d| LoadSite {
                host: d.hostname,
                url: d.url.trim_end_matches('/').to_string(),
                public: false,
            })
            .collect();
        for t in self.list_tunnels().into_iter().filter(|t| {
            t.config.project_id.as_deref() == Some(project_id) && t.state == "connected"
        }) {
            if let Some(url) = t.public_url {
                let host = url
                    .trim_start_matches("https://")
                    .trim_start_matches("http://")
                    .split('/')
                    .next()
                    .unwrap_or("")
                    .to_string();
                sites.push(LoadSite {
                    host,
                    url: url.trim_end_matches('/').to_string(),
                    public: true,
                });
            }
        }
        sites
    }

    pub fn load_overview(&self, project_id: &str) -> Result<LoadOverview, CoreError> {
        let dir = self.load_project_dir(project_id)?;
        let mut scripts: Vec<ScriptInfo> = std::fs::read_dir(&dir)
            .map(|d| {
                d.flatten()
                    .filter(|e| script_name_ok(&e.file_name().to_string_lossy()))
                    .map(|e| ScriptInfo {
                        name: e.file_name().to_string_lossy().to_string(),
                        size: e.metadata().map(|m| m.len()).unwrap_or(0),
                    })
                    .collect()
            })
            .unwrap_or_default();
        scripts.sort_by(|a, b| a.name.cmp(&b.name));
        let k6 = match self.k6_binary() {
            Some((p, v, managed)) => K6Info {
                installed: true,
                path: Some(p.display().to_string()),
                version: Some(v),
                managed,
            },
            None => K6Info {
                installed: false,
                path: None,
                version: None,
                managed: false,
            },
        };
        Ok(LoadOverview {
            k6,
            scripts,
            sites: self.load_sites(project_id),
            max_vus: self.load_max_vus(),
        })
    }

    fn script_path(&self, project_id: &str, name: &str) -> Result<PathBuf, CoreError> {
        if !script_name_ok(name) {
            return Err(fail(
                "a script name is letters, digits, '-', '_' and '.', ending in .js",
            ));
        }
        Ok(self.load_project_dir(project_id)?.join(name))
    }

    /// Whether `script` passes the same safety scan a run applies (only this project's own sites, limited VUs).
    pub fn load_check_script(&self, project_id: &str, script: &str) -> Result<(), String> {
        let hosts: Vec<String> = self
            .load_sites(project_id)
            .into_iter()
            .map(|s| s.host)
            .collect();
        check_script(script, &hosts, self.load_max_vus())
    }

    pub fn load_read_script(&self, project_id: &str, name: &str) -> Result<String, CoreError> {
        Ok(std::fs::read_to_string(
            self.script_path(project_id, name)?,
        )?)
    }

    pub fn load_save_script(
        &self,
        project_id: &str,
        name: &str,
        content: &str,
    ) -> Result<(), CoreError> {
        let path = self.script_path(project_id, name)?;
        if content.len() > 256 * 1024 {
            return Err(fail("a script is limited to 256 KB"));
        }
        std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))?;
        std::fs::write(path, content)?;
        Ok(())
    }

    pub fn load_delete_script(&self, project_id: &str, name: &str) -> Result<(), CoreError> {
        let path = self.script_path(project_id, name)?;
        if path.is_file() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }

    fn profiles_file(&self) -> PathBuf {
        self.paths.data_dir().join("loadtest_profiles.json")
    }

    fn custom_profiles(&self) -> Vec<LoadProfile> {
        std::fs::read_to_string(self.profiles_file())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    /// The ready-made plans, then the user's own.
    pub fn load_profiles(&self) -> Vec<LoadProfile> {
        let mut all = builtin_profiles();
        for mut p in self.custom_profiles() {
            // Secret values live in the keyring; they are read back for the form and never stored in the file.
            for v in p.variables.iter_mut().filter(|v| v.secret) {
                v.value = crate::secrets::get_secret(&secret_key(&p.id, &v.name))
                    .ok()
                    .flatten()
                    .unwrap_or_default();
            }
            all.push(p);
        }
        all
    }

    pub fn load_save_profile(
        &self,
        mut profile: LoadProfile,
    ) -> Result<Vec<LoadProfile>, CoreError> {
        profile.validate(self.load_max_vus()).map_err(fail)?;
        profile.builtin = false;
        if profile.icon.is_empty() || builtin_profiles().iter().all(|b| b.icon != profile.icon) {
            profile.icon = "custom".into();
        }
        let id = crate::domain::slugify(if profile.id.is_empty() {
            &profile.name
        } else {
            &profile.id
        });
        if id.is_empty() {
            return Err(fail("give the test a name"));
        }
        if builtin_profiles().iter().any(|b| b.id == id) {
            return Err(fail(format!(
                "'{id}' is a ready-made test; save yours under another name"
            )));
        }
        profile.id = id.clone();
        let mut stored = profile.clone();
        for v in stored.variables.iter_mut().filter(|v| v.secret) {
            let key = secret_key(&id, &v.name);
            if v.value.is_empty() {
                let _ = crate::secrets::delete_secret(&key);
            } else {
                crate::secrets::set_secret(&key, &v.value)
                    .map_err(|e| fail(format!("the secret couldn't be stored: {e}")))?;
            }
            v.value.clear();
        }
        let mut custom = self.custom_profiles();
        custom.retain(|p| p.id != id);
        custom.push(stored);
        std::fs::create_dir_all(self.paths.data_dir())?;
        std::fs::write(self.profiles_file(), serde_json::to_string_pretty(&custom)?)?;
        Ok(self.load_profiles())
    }

    pub fn load_delete_profile(&self, id: &str) -> Result<Vec<LoadProfile>, CoreError> {
        let mut custom = self.custom_profiles();
        for p in custom.iter().filter(|p| p.id == id) {
            for v in p.variables.iter().filter(|v| v.secret) {
                let _ = crate::secrets::delete_secret(&secret_key(&p.id, &v.name));
            }
        }
        custom.retain(|p| p.id != id);
        std::fs::write(self.profiles_file(), serde_json::to_string_pretty(&custom)?)?;
        Ok(self.load_profiles())
    }

    /// Writes the script for a test plan into the project (replacing one of the same name); returns its file name.
    pub fn load_generate(
        &self,
        project_id: &str,
        profile: &LoadProfile,
        name: Option<&str>,
    ) -> Result<String, CoreError> {
        profile.validate(self.load_max_vus()).map_err(fail)?;
        let base = crate::domain::slugify(name.unwrap_or(if profile.id.is_empty() {
            &profile.name
        } else {
            &profile.id
        }));
        let file = format!(
            "{}.js",
            if base.is_empty() {
                "test".to_string()
            } else {
                base
            }
        );
        self.load_save_script(project_id, &file, &profile.script())?;
        Ok(file)
    }

    /// Starts k6 against a site of the project. Returns the run (still running).
    pub fn load_run(
        &self,
        project_id: &str,
        script: &str,
        target: Option<&str>,
        confirm_public: bool,
        env: &[(String, String)],
    ) -> Result<LoadRun, CoreError> {
        let (k6, _, _) = self.k6_binary().ok_or_else(|| {
            CoreError::failed_fix(
                "k6 isn't installed.",
                "Load tests run with k6.",
                "Install k6 from the Runtimes page.",
            )
        })?;
        let path = self.script_path(project_id, script)?;
        let text =
            std::fs::read_to_string(&path).map_err(|_| fail(format!("{script} doesn't exist")))?;
        let sites = self.load_sites(project_id);
        let site = match target {
            Some(t) => sites
                .iter()
                .find(|s| s.host.eq_ignore_ascii_case(t) || s.url == t),
            None => sites.iter().find(|s| !s.public),
        }
        .ok_or_else(|| {
            fail("this project has no site to test. Give it a domain first, or start its tunnel.")
        })?
        .clone();
        if site.public && !confirm_public {
            return Err(CoreError::failed_fix("The test wasn't started.", format!("{} is a public tunnel address. A load test sends real traffic through the tunnel provider.", site.host), "Confirm that you mean to test the public address."));
        }
        let allowed: Vec<String> = sites
            .iter()
            .filter(|s| site.public || !s.public)
            .map(|s| s.host.clone())
            .collect();
        check_script(&text, &allowed, self.load_max_vus()).map_err(fail)?;
        for (k, v) in env {
            if !var_name_ok(k)
                || k == "BASE_URL"
                || v.len() > 4000
                || v.contains(['\0', '\r', '\n'])
            {
                return Err(fail(format!("'{k}' isn't a usable variable")));
            }
        }
        if self.load_runs_active(project_id) {
            return Err(fail(
                "a test is already running for this project. Stop it first.",
            ));
        }

        let id = format!("run-{}", now_ms());
        let dir = self.paths.data_dir().join("loadtests").join(project_id);
        std::fs::create_dir_all(&dir)?;
        let json_out = dir.join(format!("{id}.points.json"));
        let mut cmd = Command::new(&k6);
        cmd.arg("run")
            .arg("--quiet")
            .arg("--no-color")
            .arg("--no-usage-report")
            .arg("--out")
            .arg(format!("json={}", json_out.display()))
            .arg("--env")
            .arg(format!("BASE_URL={}", site.url))
            .arg(&path)
            .current_dir(path.parent().unwrap_or(Path::new(".")))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (k, v) in env {
            cmd.arg("--env").arg(format!("{k}={v}"));
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000 | 0x0000_4000); // no window, below-normal priority
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| fail(format!("k6 didn't start: {e}")))?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        let run = LoadRun {
            id: id.clone(),
            project_id: project_id.into(),
            script: script.into(),
            target: site.url.clone(),
            state: "running".into(),
            started_ms: now_ms(),
            finished_ms: None,
            exit_code: None,
            metrics: Metrics::default(),
            series: vec![],
            output: vec![],
            message: None,
        };
        let live = Arc::new(Mutex::new(Live {
            run: run.clone(),
            child: Some(child),
            stop_requested: false,
            durations: vec![],
            sum_ms: 0.0,
            buckets: HashMap::new(),
        }));
        self.loadtests
            .runs
            .lock()
            .unwrap()
            .insert(id.clone(), live.clone());

        // k6's own messages (threshold results, errors) come on both pipes.
        for pipe in [
            stdout.map(|s| Box::new(s) as Box<dyn Read + Send>),
            stderr.map(|s| Box::new(s) as Box<dyn Read + Send>),
        ]
        .into_iter()
        .flatten()
        {
            let live = live.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                    let mut l = live.lock().unwrap();
                    l.run.output.push(crate::redact::redact_text(&line));
                    if l.run.output.len() > 200 {
                        l.run.output.remove(0);
                    }
                }
            });
        }
        let record_dir = dir.clone();
        std::thread::spawn(move || monitor(live, json_out, record_dir));
        Ok(run)
    }

    fn load_runs_active(&self, project_id: &str) -> bool {
        self.loadtests.runs.lock().unwrap().values().any(|l| {
            let l = l.lock().unwrap();
            l.run.project_id == project_id && l.run.state == "running"
        })
    }

    pub fn load_status(&self, run_id: &str) -> Result<LoadRun, CoreError> {
        if let Some(l) = self.loadtests.runs.lock().unwrap().get(run_id) {
            return Ok(l.lock().unwrap().run.clone());
        }
        // A run from an earlier session.
        let found = std::fs::read_dir(self.paths.data_dir().join("loadtests"))
            .ok()
            .and_then(|d| {
                d.flatten().find_map(|p| {
                    std::fs::read_to_string(p.path().join(format!("{run_id}.json")))
                        .ok()
                        .and_then(|t| serde_json::from_str::<LoadRun>(&t).ok())
                })
            });
        found.ok_or_else(|| fail(format!("no run '{run_id}'")))
    }

    pub fn load_stop(&self, run_id: &str) -> Result<LoadRun, CoreError> {
        if let Some(l) = self.loadtests.runs.lock().unwrap().get(run_id).cloned() {
            let mut g = l.lock().unwrap();
            g.stop_requested = true;
            if let Some(child) = g.child.as_mut() {
                let _ = child.kill();
            }
        }
        std::thread::sleep(Duration::from_millis(600));
        self.load_status(run_id)
    }

    /// Saved runs of a project, newest first (running ones included).
    pub fn load_runs(&self, project_id: &str) -> Vec<LoadRun> {
        let mut runs: Vec<LoadRun> =
            std::fs::read_dir(self.paths.data_dir().join("loadtests").join(project_id))
                .map(|d| {
                    d.flatten()
                        .filter(|e| !e.file_name().to_string_lossy().contains(".points."))
                        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
                        .filter_map(|t| serde_json::from_str::<LoadRun>(&t).ok())
                        .collect()
                })
                .unwrap_or_default();
        for l in self.loadtests.runs.lock().unwrap().values() {
            let r = l.lock().unwrap().run.clone();
            if r.project_id == project_id && !runs.iter().any(|x| x.id == r.id) {
                runs.push(r);
            }
        }
        runs.sort_by(|a, b| b.started_ms.cmp(&a.started_ms));
        runs
    }

    pub fn load_delete_run(&self, project_id: &str, run_id: &str) -> Result<(), CoreError> {
        if !run_id.starts_with("run-") || run_id.contains(['/', '\\', '.']) {
            return Err(fail("not a run id"));
        }
        let dir = self.paths.data_dir().join("loadtests").join(project_id);
        let _ = std::fs::remove_file(dir.join(format!("{run_id}.json")));
        let _ = std::fs::remove_file(dir.join(format!("{run_id}.points.json")));
        self.loadtests.runs.lock().unwrap().remove(run_id);
        Ok(())
    }
}

/// Follows k6's point stream while it runs, then records the result.
fn monitor(live: Arc<Mutex<Live>>, points: PathBuf, dir: PathBuf) {
    let start = Instant::now();
    let mut offset = 0u64;
    let mut carry = String::new();
    let read_new = |live: &mut Live, offset: &mut u64, carry: &mut String| {
        let Ok(mut f) = std::fs::File::open(&points) else {
            return;
        };
        if f.seek(SeekFrom::Start(*offset)).is_err() {
            return;
        }
        let mut chunk = String::new();
        // Points can be cut mid-line or mid-character; keep what isn't a whole line for next time.
        let mut buf = Vec::new();
        if f.read_to_end(&mut buf).is_ok() {
            *offset += buf.len() as u64;
            chunk.push_str(&String::from_utf8_lossy(&buf));
        }
        carry.push_str(&chunk);
        while let Some(i) = carry.find('\n') {
            let line: String = carry.drain(..=i).collect();
            apply_line(live, line.trim(), start.elapsed().as_secs() as u32);
        }
    };
    let exit = loop {
        {
            let mut l = live.lock().unwrap();
            read_new(&mut l, &mut offset, &mut carry);
            refresh_metrics(&mut l, start.elapsed().as_secs_f64());
            let status = l.child.as_mut().and_then(|c| c.try_wait().ok().flatten());
            if let Some(status) = status {
                break status.code();
            }
        }
        std::thread::sleep(Duration::from_millis(400));
    };
    std::thread::sleep(Duration::from_millis(300)); // let the last output lines arrive
    let mut l = live.lock().unwrap();
    read_new(&mut l, &mut offset, &mut carry);
    refresh_metrics(&mut l, start.elapsed().as_secs_f64());
    l.child = None;
    l.run.finished_ms = Some(now_ms());
    l.run.exit_code = exit;
    let (state, message) = if l.stop_requested {
        ("stopped", Some("Stopped by you.".to_string()))
    } else {
        match exit {
            Some(0) => ("passed", None),
            Some(99) => ("failed", Some("A threshold failed: the site was slower or less reliable than the script's limits.".to_string())),
            _ => ("error", Some(l.run.output.iter().rev().find(|s| !s.trim().is_empty()).cloned().unwrap_or_else(|| "k6 stopped unexpectedly.".into()))),
        }
    };
    l.run.state = state.into();
    l.run.message = message;
    if let Ok(text) = serde_json::to_string_pretty(&l.run) {
        let _ = std::fs::write(dir.join(format!("{}.json", l.run.id)), text);
    }
    let _ = std::fs::remove_file(&points);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hosts(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_script_may_only_name_the_projects_own_sites() {
        let ok = "http.get('https://shop.test/cart'); http.get(`${__ENV.BASE_URL}/`)";
        assert!(check_script(ok, &hosts(&["shop.test"]), 200).is_ok());
        let err = check_script(
            "http.get('https://example.com/')",
            &hosts(&["shop.test"]),
            200,
        )
        .unwrap_err();
        assert!(err.contains("example.com"), "{err}");
    }

    #[test]
    fn virtual_user_numbers_stay_under_the_limit() {
        assert!(check_script("export const options = { vus: 50 };", &[], 100).is_ok());
        assert!(
            check_script("stages: [{ duration: '1m', target: 5000 }]", &[], 100)
                .unwrap_err()
                .contains("5000")
        );
        assert!(check_script("{ preAllocatedVUs: 101 }", &[], 100).is_err());
    }

    fn plan() -> LoadProfile {
        builtin_profiles().remove(1)
    }

    #[test]
    fn every_ready_made_plan_is_valid_and_its_script_passes_the_safety_scan() {
        for p in builtin_profiles() {
            p.validate(DEFAULT_MAX_VUS)
                .unwrap_or_else(|e| panic!("{}: {e}", p.id));
            let s = p.script();
            assert!(
                s.contains("__ENV.BASE_URL") && s.contains("stages:") && s.contains("thresholds"),
                "{}",
                p.id
            );
            assert!(
                check_script(&s, &[], DEFAULT_MAX_VUS).is_ok(),
                "{} must pass the safety scan",
                p.id
            );
        }
    }

    #[test]
    fn a_form_becomes_the_matching_script() {
        let mut p = plan();
        p.stages = vec![
            Stage {
                duration_s: 20,
                target: 7,
            },
            Stage {
                duration_s: 90,
                target: 7,
            },
        ];
        p.requests = vec![
            PlannedRequest {
                method: "GET".into(),
                path: "/".into(),
                body: None,
            },
            PlannedRequest {
                method: "POST".into(),
                path: "/api/orders?x=1".into(),
                body: None,
            },
        ];
        p.think_time_s = 2.5;
        p.thresholds = Thresholds {
            p95_ms: Some(800),
            p99_ms: Some(1500),
            error_rate_pct: Some(2.0),
        };
        let s = p.script();
        for wanted in [
            "{ duration: '20s', target: 7 }",
            "{ duration: '90s', target: 7 }",
            "{ method: 'POST', path: '/api/orders?x=1', body: null }",
            "sleep(2.5)",
            "'p(95)<800', 'p(99)<1500'",
            "rate<0.02",
        ] {
            assert!(s.contains(wanted), "missing {wanted} in\n{s}");
        }
        p.thresholds = Thresholds::default();
        assert!(!p.script().contains("http_req_duration"));
    }

    #[test]
    fn headers_bodies_and_variables_reach_the_script_without_their_values() {
        let mut p = plan();
        p.variables = vec![Variable {
            name: "TOKEN".into(),
            value: "s3cret-token".into(),
            secret: true,
        }];
        p.headers = vec![
            Header {
                name: "Authorization".into(),
                value: "Bearer {{TOKEN}}".into(),
            },
            Header {
                name: "X-Client".into(),
                value: "ols".into(),
            },
        ];
        p.requests = vec![PlannedRequest {
            method: "POST".into(),
            path: "/api/login".into(),
            body: Some("{\"user\":\"a\",\"t\":\"{{TOKEN}}\"}".into()),
        }];
        p.validate(100).unwrap();
        let s = p.script();
        assert!(
            s.contains("\"Authorization\": \"Bearer \" + __ENV.TOKEN"),
            "{s}"
        );
        assert!(s.contains("\"X-Client\": \"ols\""));
        assert!(s.contains("__ENV.TOKEN"), "the body reads the variable");
        assert!(s.contains("\"Content-Type\": \"application/json\""), "{s}");
        assert!(
            !s.contains("s3cret-token"),
            "the value must never be written into the script"
        );
        assert!(check_script(&s, &[], 100).is_ok());
    }

    #[test]
    fn unsafe_headers_variables_and_bodies_are_refused() {
        let ok = plan();
        let bad = |f: &dyn Fn(&mut LoadProfile)| {
            let mut p = ok.clone();
            f(&mut p);
            p.validate(100).is_err()
        };
        assert!(bad(&|p| p.headers = vec![Header {
            name: "Host".into(),
            value: "evil.example".into()
        }]));
        assert!(bad(&|p| p.headers = vec![Header {
            name: "X-A".into(),
            value: "a\r\nX-B: 1".into()
        }]));
        assert!(bad(&|p| p.headers = vec![Header {
            name: "X-A".into(),
            value: "{{MISSING}}".into()
        }]));
        assert!(bad(&|p| p.variables = vec![Variable {
            name: "1BAD".into(),
            value: "x".into(),
            secret: false
        }]));
        assert!(bad(&|p| p.variables = vec![Variable {
            name: "BASE_URL".into(),
            value: "x".into(),
            secret: false
        }]));
        assert!(
            bad(&|p| p.requests[0].body = Some("x".into())),
            "GET can't carry a body"
        );
        assert!(
            js_value("plain \"quoted\"").starts_with('"'),
            "literals are JSON-escaped"
        );
    }

    #[test]
    fn bad_plans_are_refused() {
        let ok = plan();
        let bad = |f: &dyn Fn(&mut LoadProfile)| {
            let mut p = ok.clone();
            f(&mut p);
            p.validate(100).is_err()
        };
        assert!(bad(&|p| p.stages.clear()));
        assert!(bad(&|p| p.stages[0].target = 101), "over the user limit");
        assert!(bad(&|p| p.stages[0].duration_s = 0));
        assert!(bad(&|p| p.stages.iter_mut().for_each(|s| s.target = 0)));
        assert!(bad(&|p| p.requests.clear()));
        assert!(bad(&|p| p.requests[0].path = "login".into()));
        assert!(bad(&|p| p.requests[0].path = "/a');evil('".into()));
        assert!(bad(&|p| p.requests[0].method = "TRACE".into()));
        assert!(bad(&|p| p.think_time_s = 120.0));
        assert!(bad(&|p| p.thresholds.error_rate_pct = Some(150.0)));
        assert!(bad(&|p| p.name = " ".into()));
        assert!(ok.validate(100).is_ok());
    }

    #[test]
    fn script_names_cannot_leave_the_folder() {
        assert!(script_name_ok("smoke.js") && script_name_ok("my-test_2.js"));
        for bad in [
            "../x.js",
            "a/b.js",
            "a\\b.js",
            ".hidden.js",
            "x.txt",
            "",
            "c:x.js",
        ] {
            assert!(!script_name_ok(bad), "{bad}");
        }
    }

    #[test]
    fn point_lines_become_live_numbers() {
        let mut live = Live {
            run: LoadRun {
                id: "r".into(),
                project_id: "p".into(),
                script: "s".into(),
                target: "t".into(),
                state: "running".into(),
                started_ms: 0,
                finished_ms: None,
                exit_code: None,
                metrics: Metrics::default(),
                series: vec![],
                output: vec![],
                message: None,
            },
            child: None,
            stop_requested: false,
            durations: vec![],
            sum_ms: 0.0,
            buckets: HashMap::new(),
        };
        for i in 1..=100 {
            apply_line(
                &mut live,
                &format!(
                    r#"{{"type":"Point","metric":"http_reqs","data":{{"time":"2026-01-01T00:00:00Z","value":1,"tags":{{}}}}}}"#
                ),
                0,
            );
            apply_line(
                &mut live,
                &format!(
                    r#"{{"type":"Point","metric":"http_req_duration","data":{{"time":"2026-01-01T00:00:00Z","value":{i}.0,"tags":{{}}}}}}"#
                ),
                0,
            );
        }
        apply_line(
            &mut live,
            r#"{"type":"Point","metric":"http_req_failed","data":{"time":"x","value":1,"tags":{}}}"#,
            0,
        );
        apply_line(
            &mut live,
            r#"{"type":"Point","metric":"vus","data":{"time":"x","value":7,"tags":{}}}"#,
            1,
        );
        apply_line(&mut live, "not json", 1);
        refresh_metrics(&mut live, 10.0);
        let m = &live.run.metrics;
        assert_eq!(m.requests, 100);
        assert_eq!(m.failed, 1);
        assert!((m.rps - 10.0).abs() < 1e-9);
        assert!(
            (m.p50_ms - 51.0).abs() < 1.5 && (m.p95_ms - 95.0).abs() < 1.5 && m.p99_ms >= 99.0,
            "{m:?}"
        );
        assert_eq!(m.vus, 7);
        assert_eq!(live.run.series.len(), 2);
    }
}
