//! The AI assistant (Stage 19). Opt-in, off by default, bring your own model: OLS ships no model and no
//! account. Every provider (LM Studio, Ollama, Hugging Face, OpenRouter, any OpenAI-compatible server) is the same
//! code over the chat-completions API with a different base URL and key.
//!
//! Safety, in order of strength:
//! - **Nothing is sent unless the user asks.** Every request starts from a button; the exact prompt can be shown first.
//! - **Everything sent is redacted** (`redact_text`, `.env` values hidden, the home folder masked), including what the
//!   model reads through its tools.
//! - **A provider outside this computer is labelled as such and needs a confirmation** for each request; with only local
//!   providers configured nothing leaves the machine.
//! - **The AI proposes, the core disposes.** A model may end its answer with a plan of `CoreCommand`s. Each one is parsed,
//!   checked against `allowed`, and shown; nothing runs until the user approves it, and destructive steps need a separate
//!   confirmation. The model's tools only read (sites, services, findings, logs, configs, manifests, `.env` names).
//!   It never gets a shell.
//! - API keys live in the Secrets Manager, never in a file.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::app::Inner;
use crate::command::{Core, CoreCommand};
use crate::error::CoreError;
use crate::redact::{redact_env_file, redact_text};
use crate::repair::RepairStep;

pub const KINDS: &[&str] = &["lmstudio", "ollama", "huggingface", "openrouter", "custom"];

/// The places the assistant is used; each can have its own provider.
pub const FEATURES: &[(&str, &str)] = &[
    ("explain", "Explain and fix"),
    ("config", "Configs and manifests"),
    ("logs", "Ask the logs"),
    ("traffic", "Traffic and k6 scripts"),
    ("commit", "Commit messages"),
    ("palette", "Plain-language palette"),
];

const MAX_ROUNDS: usize = 6;
const MAX_ACTIONS: usize = 10;
const MAX_PART: usize = 12_000;
const MAX_TOTAL: usize = 48_000;
const TOOL_OUTPUT: usize = 8_000;
/// Inline cap for a user-picked log excerpt; beyond this the full text spills to a `.log` file readable via `read_excerpt`.
const EXCERPT_INLINE: usize = 12_000;
/// Hard cap stored on disk for a picked excerpt (≈200 KB).
const EXCERPT_MAX_FILE: usize = 200_000;
/// How many excerpt files to keep per workspace before pruning oldest.
const EXCERPT_KEEP: usize = 20;
const IDLE: Duration = Duration::from_secs(120);

fn fail(msg: impl Into<String>) -> CoreError {
    CoreError::failed("The AI assistant couldn't do that.", msg)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn yes() -> bool {
    true
}

fn key_name(provider_id: &str) -> String {
    format!("ai:{provider_id}")
}

// ------------------------------------------------------------------------------ settings

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiProvider {
    pub id: String,
    pub name: String,
    /// lmstudio, ollama, huggingface, openrouter or custom.
    pub kind: String,
    pub base_url: String,
    #[serde(default)]
    pub model: String,
    /// Let the model call the read-only tools. Some small local models can't; they get the context up front instead.
    #[serde(default = "yes")]
    pub tools: bool,
    /// Runs on this computer (a loopback address). Computed, not stored.
    #[serde(default)]
    pub local: bool,
    /// A key is stored for it. Computed, not stored.
    #[serde(default)]
    pub has_key: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AiSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub providers: Vec<AiProvider>,
    /// Feature id to provider id; a feature without an entry uses the first provider.
    #[serde(default)]
    pub features: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiFeature {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiState {
    pub settings: AiSettings,
    pub features: Vec<AiFeature>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiModel {
    pub id: String,
    pub name: String,
    pub context: Option<u64>,
    /// US dollars per million tokens, when the provider publishes a price.
    pub prompt_per_m: Option<f64>,
    pub completion_per_m: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiTestResult {
    pub ok: bool,
    pub message: String,
    pub ms: u64,
    pub models: Vec<AiModel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiDetected {
    pub kind: String,
    pub name: String,
    pub base_url: String,
    pub models: Vec<String>,
}

/// True when the address is on this computer.
pub fn is_local_url(url: &str) -> bool {
    let Some(parsed) = reqwest::Url::parse(url.trim()).ok() else {
        return false;
    };
    let Some(host) = parsed.host_str() else {
        return false;
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

fn url_host(url: &str) -> String {
    reqwest::Url::parse(url.trim())
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .unwrap_or_default()
}

fn get_key(provider_id: &str) -> Option<String> {
    crate::secrets::get_secret(&key_name(provider_id))
        .ok()
        .flatten()
        .filter(|k| !k.is_empty())
}

fn endpoint(base: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base.trim().trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

fn pick_provider<'a>(cfg: &'a AiSettings, feature: &str) -> Option<&'a AiProvider> {
    cfg.features
        .get(feature)
        .and_then(|id| cfg.providers.iter().find(|p| &p.id == id))
        .or_else(|| cfg.providers.first())
}

// ------------------------------------------------------------------------------ text helpers

fn clean(text: &str) -> String {
    crate::support::mask_home(&redact_text(text))
}

fn head(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let n = text.floor_char_boundary(max);
    format!("{}\n[cut: {} more characters]", &text[..n], text.len() - n)
}

fn tail(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut n = text.len() - max;
    while !text.is_char_boundary(n) {
        n += 1;
    }
    format!("[cut: {n} earlier characters]\n{}", &text[n..])
}

struct Fence {
    lang: String,
    body: String,
    start: usize,
    end: usize,
}

/// The ``` fenced blocks of a model's answer.
fn fences(text: &str) -> Vec<Fence> {
    let mut out = Vec::new();
    let mut open: Option<(usize, String, Vec<&str>)> = None;
    for (n, line) in text.lines().enumerate() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("```") {
            match open.take() {
                Some((start, lang, body)) => out.push(Fence {
                    lang,
                    body: body.join("\n"),
                    start,
                    end: n,
                }),
                None => open = Some((n, rest.trim().to_ascii_lowercase(), Vec::new())),
            }
        } else if let Some((_, _, body)) = open.as_mut() {
            body.push(line);
        }
    }
    out
}

fn without_fences(text: &str, drop: &[&Fence]) -> String {
    text.lines()
        .enumerate()
        .filter(|(n, _)| !drop.iter().any(|f| *n >= f.start && *n <= f.end))
        .map(|(_, l)| l)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

// ------------------------------------------------------------------------------ requests and answers

/// What a user asked for. `feature` is one of `FEATURES`; `kind` narrows it (config: manifest or web; traffic: explain,
/// handler or k6).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AiRequest {
    pub feature: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub question: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    /// The subject: a finding, an error line, a config block.
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub tunnel_id: Option<String>,
    #[serde(default)]
    pub request_ids: Vec<u64>,
    #[serde(default)]
    pub log_sources: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptMessage {
    pub role: String,
    pub content: String,
}

/// Exactly what a request would send ("Show what will be sent").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiPrompt {
    pub provider_id: String,
    pub provider_name: String,
    pub model: String,
    pub local: bool,
    /// Where it goes.
    pub host: String,
    pub messages: Vec<PromptMessage>,
    pub attachments: Vec<String>,
    /// Read-only tools the model may call while answering (each result is redacted too).
    pub tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiAction {
    pub label: String,
    pub command: CoreCommand,
    pub destructive: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AiAnswer {
    /// The answer without its plan block.
    pub text: String,
    /// The plan, checked against the allowlist. Nothing in it has run.
    pub actions: Vec<AiAction>,
    /// Steps the model proposed that were refused, and why.
    pub rejected: Vec<String>,
    /// A drafted `environment.yaml` that parses.
    pub manifest: Option<String>,
    /// A complete file draft, shown for review before the editor uses it.
    #[serde(default)]
    pub file: Option<String>,
    /// A drafted k6 script that passes the load-test safety scan.
    pub script: Option<String>,
    pub commit_message: Option<String>,
    pub provider: String,
    pub model: String,
    pub local: bool,
    pub tokens_in: Option<u64>,
    pub tokens_out: Option<u64>,
    pub cost_usd: Option<f64>,
    /// What was read to answer: attachments and tool calls.
    pub used: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiJobView {
    pub id: String,
    pub feature: String,
    /// running, done, failed or cancelled.
    pub state: String,
    /// The answer so far, while it streams.
    pub partial: String,
    /// Tools called and other steps, in order.
    pub activity: Vec<String>,
    pub answer: Option<AiAnswer>,
    pub error: Option<String>,
    pub provider: String,
    pub local: bool,
}

struct Job {
    view: AiJobView,
    cancel: Arc<AtomicBool>,
}

#[derive(Default)]
pub struct AiJobs {
    jobs: Mutex<HashMap<String, Arc<Mutex<Job>>>>,
    counter: AtomicU64,
    /// Model prices per base URL, so a cost can be shown without listing models for every answer.
    prices: Mutex<HashMap<String, (Instant, Vec<AiModel>)>>,
}

// ------------------------------------------------------------------------------ what a plan may contain

/// The commands an answer may propose. Everything else is refused, whatever the model says.
pub fn allowed(cmd: &CoreCommand) -> bool {
    use CoreCommand as C;
    matches!(
        cmd,
        C::StartService { .. }
            | C::StopService { .. }
            | C::RestartService { .. }
            | C::InstallRuntime { .. }
            | C::ApplyWeb { .. }
            | C::ValidateWeb
            | C::StopWeb
            | C::RestartSiteApp { .. }
            | C::SyncHosts
            | C::TrustCa
            | C::RegenerateCertificate { .. }
            | C::SetDomainEnabled { .. }
            | C::CreateDatabase { .. }
            | C::BackupDatabase { .. }
            | C::ApplySetup { .. }
            | C::ApplyMailpitEnv { .. }
            | C::StartWorker { .. }
            | C::StopWorker { .. }
            | C::RestartWorker { .. }
            | C::RunComposer { .. }
            | C::SetEnvValue { .. }
            | C::StartQuickApp { .. }
    )
}

/// Steps that replace something the user may want to keep: they need a separate confirmation.
pub fn destructive(cmd: &CoreCommand) -> bool {
    crate::repair::is_destructive(cmd) || matches!(cmd, CoreCommand::SetEnvValue { .. })
}

/// A proposed command as it will actually run: an imported Quick App never gets its approval, and nothing elevated.
fn normalise(cmd: CoreCommand) -> CoreCommand {
    match cmd {
        CoreCommand::StartQuickApp { id, values, .. } => CoreCommand::StartQuickApp {
            id,
            values,
            approval: None,
            allow_elevated: false,
        },
        other => other,
    }
}

const PLAN_DOC: &str = r#"Commands you may propose, as JSON with the "type" first. Nothing else is accepted:
- start_service, stop_service, restart_service: {"type":"start_service","id":"mariadb"} (ids from list_services)
- install_runtime: {"type":"install_runtime","id":"php","version":"8.4.26"}
- apply_web: {"type":"apply_web","overwrite":[]} (regenerate the web server config and reload it); validate_web; stop_web
- restart_site_app: {"type":"restart_site_app","hostname":"shop.test"}
- set_domain_enabled: {"type":"set_domain_enabled","hostname":"shop.test","enabled":true}
- regenerate_certificate: {"type":"regenerate_certificate","hostname":"shop.test"}; sync_hosts; trust_ca
- create_database / backup_database: {"type":"create_database","engine":"mariadb","name":"shop"} / {"type":"backup_database","engine":"mariadb","database":"shop"}
- apply_setup: {"type":"apply_setup","project_id":"...","dry_run":false}
- apply_mailpit_env: {"type":"apply_mailpit_env","project_id":"...","file":".env"}
- start_worker, stop_worker, restart_worker: {"type":"start_worker","id":"..."}
- run_composer: {"type":"run_composer","project_id":"...","action":"install"}
- set_env_value: {"type":"set_env_value","project_id":"...","file":".env","key":"DB_HOST","value":"127.0.0.1"}
- start_quick_app: {"type":"start_quick_app","id":"laravel","values":{"project_name":"shop"}}
To propose changes, end the answer with exactly one block:
```ols-plan
[{"label":"Start MariaDB","command":{"type":"start_service","id":"mariadb"}}]
```
Propose only what you are sure about, at most 10 steps, and use ids and names you were given. Do not propose anything when no command is needed."#;

const SYSTEM: &str = "You are the assistant inside OLS, a Windows local development environment manager: Nginx, Apache and Caddy, PHP, Node, Python, MariaDB, PostgreSQL, MongoDB, Redis, Mailpit, trusted local HTTPS domains, and project manifests in .openlocalserver/. Be concrete and brief; say what you are unsure of. You cannot run anything yourself: the user approves every change. Secrets in what you are given are replaced by [redacted]; never ask for them.";

const MANIFEST_INSTRUCTIONS: &str = "Draft .openlocalserver/environment.yaml for this project. Reply with the complete file in one ```yaml block, using only the keys shown in the detected example, then a short note on what you chose and what to check. Do not invent versions the project files don't support.";

// ------------------------------------------------------------------------------ tools (read-only)

fn tool_defs() -> Vec<Value> {
    let f = |name: &str, description: &str, properties: Value, required: &[&str]| json!({"type":"function","function":{"name":name,"description":description,"parameters":{"type":"object","properties":properties,"required":required}}});
    vec![
        f("list_projects", "The registered projects (id, name, folder).", json!({}), &[]),
        f("list_sites", "The local sites: hostname, url, kind, project.", json!({}), &[]),
        f("list_services", "The services (id, running, port).", json!({}), &[]),
        f("get_findings", "Problems the diagnostics found, with cause and suggested fix.", json!({}), &[]),
        f("list_log_sources", "The logs that can be read.", json!({}), &[]),
        f("read_log", "The last lines of a log (source from list_log_sources, e.g. app, web:error).", json!({"source":{"type":"string"},"lines":{"type":"integer"}}), &["source"]),
        f("read_excerpt", "A page of a saved log excerpt file (from a previous overlong pick), as <file> lines <a>-<b> of <n>.", json!({"file":{"type":"string"},"start_line":{"type":"integer"},"lines":{"type":"integer"}}), &["file"]),
        f("read_web_config", "The generated web server config for a site (part: site or custom), or that server's main config with no hostname (server: nginx, apache or caddy; defaults to the default server).", json!({"hostname":{"type":"string"},"part":{"type":"string"},"server":{"type":"string"}}), &[]),
        f("read_manifest", "A project's .openlocalserver/environment.yaml.", json!({"project_id":{"type":"string"}}), &["project_id"]),
        f("read_env_names", "The variable names in a project's .env file, values hidden.", json!({"project_id":{"type":"string"},"file":{"type":"string"}}), &["project_id"]),
        f("list_quick_apps", "The Quick Apps (id, name, description).", json!({}), &[]),
    ]
}

fn tool_names() -> Vec<String> {
    tool_defs()
        .iter()
        .filter_map(|t| t["function"]["name"].as_str().map(str::to_string))
        .collect()
}

impl Inner {
    fn ai_tool(&self, name: &str, args: &Value) -> Result<String, String> {
        let s = |k: &str| args.get(k).and_then(Value::as_str).map(str::to_string);
        let e = |e: CoreError| e.to_string();
        let text = match name {
            "list_projects" => self
                .projects
                .lock()
                .unwrap()
                .list()
                .into_iter()
                .map(|p| format!("{}: {} ({})", p.id, p.name, p.path))
                .collect::<Vec<_>>()
                .join("\n"),
            "list_sites" => self
                .domain_summaries()
                .into_iter()
                .map(|d| {
                    format!(
                        "{} {} kind={} project={} enabled={}",
                        d.hostname,
                        d.url,
                        d.kind,
                        d.project_id.unwrap_or_default(),
                        d.enabled
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
            "list_services" => self
                .services
                .list()
                .into_iter()
                .map(|s| {
                    format!(
                        "{}: {} installed={} running={} port={}",
                        s.id,
                        s.name,
                        s.installed,
                        s.running,
                        s.port.map(|p| p.to_string()).unwrap_or_default()
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
            "get_findings" => self
                .diagnose()
                .into_iter()
                .filter(|f| !f.ignored)
                .map(|f| {
                    format!(
                        "[{:?}] {} | cause: {} | fix: {}",
                        f.severity, f.problem, f.cause, f.fix
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
            "list_log_sources" => self
                .log_sources()
                .into_iter()
                .map(|l| format!("{}: {}", l.id, l.name))
                .collect::<Vec<_>>()
                .join("\n"),
            "read_log" => {
                let source = s("source").ok_or("read_log needs a source")?;
                let lines = args
                    .get("lines")
                    .and_then(Value::as_u64)
                    .unwrap_or(80)
                    .clamp(1, 300) as usize;
                self.read_log(&source, lines).map_err(e)?.join("\n")
            }
            "read_excerpt" => {
                let file = s("file").ok_or("read_excerpt needs a file")?;
                let start = args
                    .get("start_line")
                    .and_then(Value::as_u64)
                    .unwrap_or(1)
                    .max(1) as usize;
                let lines = args
                    .get("lines")
                    .and_then(Value::as_u64)
                    .unwrap_or(80)
                    .clamp(1, 300) as usize;
                self.read_excerpt(&file, start, lines)?
            }
            "read_web_config" => {
                let part = match s("part").as_deref() {
                    Some("custom") => crate::web::manager::ConfigPart::Custom,
                    Some("main") => crate::web::manager::ConfigPart::Main,
                    _ => crate::web::manager::ConfigPart::Site,
                };
                let host = s("hostname");
                let part = if host.is_none() {
                    crate::web::manager::ConfigPart::Main
                } else {
                    part
                };
                let cfg = self.web_config();
                let server = s("server")
                    .filter(|id| crate::web::SERVER_IDS.contains(&id.as_str()))
                    .or_else(|| {
                        host.as_deref()
                            .and_then(|h| self.domains.lock().unwrap().get(h))
                            .map(|d| crate::domain::resolved_server(&d, &cfg))
                    })
                    .unwrap_or_else(|| cfg.default_server.clone());
                self.web
                    .read_config(&server, host.as_deref(), part)
                    .map_err(e)?
            }
            "read_manifest" => {
                let id = s("project_id").ok_or("read_manifest needs a project_id")?;
                self.manifest_info(&id)
                    .map_err(e)?
                    .text
                    .unwrap_or_else(|| "(this project has no environment.yaml)".into())
            }
            "read_env_names" => {
                let id = s("project_id").ok_or("read_env_names needs a project_id")?;
                let file = s("file").unwrap_or_else(|| ".env".into());
                if file.contains(['/', '\\']) || file.contains("..") {
                    return Err("only files in the project's own folder".into());
                }
                let dir = self
                    .projects
                    .lock()
                    .unwrap()
                    .get(&id)
                    .ok_or("unknown project")?
                    .path;
                redact_env_file(
                    &std::fs::read_to_string(Path::new(&dir).join(&file))
                        .map_err(|_| format!("{file} doesn't exist"))?,
                )
            }
            "list_quick_apps" => self
                .catalog
                .lock()
                .unwrap()
                .list()
                .into_iter()
                .map(|a| format!("{}: {}: {}", a.id, a.name, a.description))
                .collect::<Vec<_>>()
                .join("\n"),
            other => return Err(format!("there is no tool called {other}")),
        };
        Ok(head(&clean(&text), TOOL_OUTPUT))
    }
}

// ------------------------------------------------------------------------------ the Inner API

impl Inner {
    fn excerpt_dir(&self) -> PathBuf {
        self.paths.data_dir().join("ai-excerpts")
    }

    /// Spill an overlong picked excerpt to a `.log` text file; returns file name. Prunes oldest beyond keep cap.
    fn save_excerpt(&self, text: &str) -> Result<String, String> {
        let dir = self.excerpt_dir();
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("couldn't store the excerpt file: {e}"))?;
        let body = head(&clean(text), EXCERPT_MAX_FILE);
        let name = format!("excerpt-{}.log", now_ms());
        std::fs::write(dir.join(&name), &body)
            .map_err(|e| format!("couldn't store the excerpt file: {e}"))?;
        if let Ok(mut files) = std::fs::read_dir(&dir).map(|d| {
            d.flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "log"))
                .collect::<Vec<_>>()
        }) {
            files.sort_by_key(|e| e.file_name());
            for stale in files.iter().rev().skip(EXCERPT_KEEP) {
                let _ = std::fs::remove_file(stale.path());
            }
        }
        Ok(name)
    }

    /// Page through a saved excerpt file: `start_line` 1-based, `lines` capped.
    fn read_excerpt(&self, file: &str, start_line: usize, lines: usize) -> Result<String, String> {
        if file.contains('/') || file.contains('\\') || file.contains("..") {
            return Err("unknown excerpt file".into());
        }
        if !file.starts_with("excerpt-") || !file.ends_with(".log") {
            return Err("unknown excerpt file".into());
        }
        let text = std::fs::read_to_string(self.excerpt_dir().join(file))
            .map_err(|_| "that excerpt file is gone; re-pick the lines".to_string())?;
        let all: Vec<&str> = text.lines().collect();
        let start = start_line.saturating_sub(1).min(all.len());
        let end = (start + lines.clamp(1, 300)).min(all.len());
        Ok(format!(
            "<file {}> lines {}-{} of {}\n{}",
            file,
            if all.is_empty() { 0 } else { start + 1 },
            end,
            all.len(),
            all[start..end].join("\n")
        ))
    }

    /// The settings with the computed fields filled in.
    pub fn ai_settings(&self) -> AiSettings {
        let mut cfg: AiSettings = crate::db::load_docs(&self.paths, "ai")
            .ok()
            .and_then(|v: Vec<AiSettings>| v.into_iter().next())
            .unwrap_or_default();
        for p in &mut cfg.providers {
            p.local = is_local_url(&p.base_url);
            p.has_key = get_key(&p.id).is_some();
        }
        cfg
    }

    fn ai_write(&self, cfg: &AiSettings) -> Result<(), CoreError> {
        let mut stored = cfg.clone();
        for p in &mut stored.providers {
            p.local = false;
            p.has_key = false;
        }
        let refs = [("state".to_string(), &stored)];
        crate::db::save_docs(&self.paths, "ai", &refs)
    }

    pub fn ai_state(&self) -> AiState {
        AiState {
            settings: self.ai_settings(),
            features: FEATURES
                .iter()
                .map(|(id, label)| AiFeature {
                    id: id.to_string(),
                    label: label.to_string(),
                })
                .collect(),
        }
    }

    pub fn ai_save_settings(
        &self,
        enabled: bool,
        features: BTreeMap<String, String>,
    ) -> Result<AiState, CoreError> {
        let mut cfg = self.ai_settings();
        for (feature, provider) in &features {
            if !FEATURES.iter().any(|(id, _)| id == feature) {
                return Err(fail(format!(
                    "'{feature}' isn't something the assistant does"
                )));
            }
            if !provider.is_empty() && !cfg.providers.iter().any(|p| &p.id == provider) {
                return Err(fail(format!("there is no provider '{provider}'")));
            }
        }
        cfg.enabled = enabled;
        cfg.features = features
            .into_iter()
            .filter(|(_, p)| !p.is_empty())
            .collect();
        self.ai_write(&cfg)?;
        Ok(self.ai_state())
    }

    /// Adds or updates a provider. `api_key`: `None` keeps the stored key, an empty string removes it.
    pub fn ai_save_provider(
        &self,
        mut provider: AiProvider,
        api_key: Option<String>,
    ) -> Result<AiState, CoreError> {
        if !KINDS.contains(&provider.kind.as_str()) {
            return Err(fail(
                "choose LM Studio, Ollama, Hugging Face, OpenRouter or a custom server",
            ));
        }
        provider.name = provider.name.trim().to_string();
        if provider.name.is_empty() {
            return Err(fail("give the provider a name"));
        }
        provider.base_url = provider.base_url.trim().trim_end_matches('/').to_string();
        match reqwest::Url::parse(&provider.base_url) {
            Ok(u) if matches!(u.scheme(), "http" | "https") && u.host_str().is_some() => {}
            _ => {
                return Err(fail(
                    "the address must be a URL like http://localhost:1234/v1",
                ))
            }
        }
        provider.model = provider.model.trim().to_string();
        let mut cfg = self.ai_settings();
        if provider.id.is_empty() {
            let base = crate::domain::slugify(&provider.name);
            let base = if base.is_empty() {
                "provider".to_string()
            } else {
                base
            };
            let mut id = base.clone();
            let mut n = 2;
            while cfg.providers.iter().any(|p| p.id == id) {
                id = format!("{base}-{n}");
                n += 1;
            }
            provider.id = id;
        } else if !cfg.providers.iter().any(|p| p.id == provider.id) {
            return Err(fail(format!("there is no provider '{}'", provider.id)));
        }
        let local = is_local_url(&provider.base_url);
        let key_after = match &api_key {
            Some(k) => !k.is_empty(),
            None => get_key(&provider.id).is_some(),
        };
        if !local && key_after && provider.base_url.starts_with("http://") {
            return Err(fail(
                "a key is only sent over https to a server that isn't on this computer",
            ));
        }
        match api_key {
            Some(k) if k.is_empty() => {
                let _ = crate::secrets::delete_secret(&key_name(&provider.id));
            }
            Some(k) => crate::secrets::set_secret(&key_name(&provider.id), &k)
                .map_err(|e| fail(format!("the key couldn't be stored: {e}")))?,
            None => {}
        }
        match cfg.providers.iter_mut().find(|p| p.id == provider.id) {
            Some(existing) => *existing = provider,
            None => cfg.providers.push(provider),
        }
        self.ai_write(&cfg)?;
        Ok(self.ai_state())
    }

    pub fn ai_remove_provider(&self, id: &str) -> Result<AiState, CoreError> {
        let mut cfg = self.ai_settings();
        cfg.providers.retain(|p| p.id != id);
        cfg.features.retain(|_, p| p != id);
        let _ = crate::secrets::delete_secret(&key_name(id));
        self.ai_write(&cfg)?;
        Ok(self.ai_state())
    }

    fn ai_provider(&self, id: &str) -> Result<AiProvider, CoreError> {
        self.ai_settings()
            .providers
            .into_iter()
            .find(|p| p.id == id)
            .ok_or_else(|| fail(format!("there is no provider '{id}'")))
    }

    pub fn ai_models(&self, provider_id: &str) -> Result<Vec<AiModel>, CoreError> {
        let p = self.ai_provider(provider_id)?;
        block_on(async { fetch_models(&p, get_key(&p.id).as_deref()).await }).map_err(fail)
    }

    pub fn ai_test(&self, provider_id: &str) -> Result<AiTestResult, CoreError> {
        let p = self.ai_provider(provider_id)?;
        Ok(test_provider(&p, get_key(&p.id).as_deref()))
    }

    /// Lists the models of a provider that is not saved yet (the form as typed), so the model picker fills itself.
    /// `api_key` none uses the stored key of an existing provider. Only the model list is requested; no prompt is sent.
    pub fn ai_probe(
        &self,
        mut provider: AiProvider,
        api_key: Option<String>,
    ) -> Result<AiTestResult, CoreError> {
        match reqwest::Url::parse(provider.base_url.trim()) {
            Ok(u) if matches!(u.scheme(), "http" | "https") && u.host_str().is_some() => {}
            _ => {
                return Err(fail(
                    "the address must be a URL like http://localhost:1234/v1",
                ))
            }
        }
        provider.base_url = provider.base_url.trim().trim_end_matches('/').to_string();
        provider.local = is_local_url(&provider.base_url);
        let key = api_key
            .filter(|k| !k.is_empty())
            .or_else(|| get_key(&provider.id));
        Ok(test_provider(&provider, key.as_deref()))
    }

    /// Looks for LM Studio and Ollama on this computer (nothing leaves it).
    pub fn ai_detect_local(&self) -> Vec<AiDetected> {
        let candidates = [
            ("lmstudio", "LM Studio", "http://localhost:1234/v1"),
            ("ollama", "Ollama", "http://localhost:11434/v1"),
        ];
        block_on(async {
            let probes = candidates.iter().map(|(kind, name, url)| async move {
                let p = AiProvider {
                    id: String::new(),
                    name: name.to_string(),
                    kind: kind.to_string(),
                    base_url: url.to_string(),
                    model: String::new(),
                    tools: true,
                    local: true,
                    has_key: false,
                };
                tokio::time::timeout(Duration::from_millis(1200), fetch_models(&p, None))
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .map(|models| AiDetected {
                        kind: kind.to_string(),
                        name: name.to_string(),
                        base_url: url.to_string(),
                        models: models.into_iter().map(|m| m.id).collect(),
                    })
            });
            futures_util::future::join_all(probes)
                .await
                .into_iter()
                .flatten()
                .collect()
        })
    }
}

fn test_provider(p: &AiProvider, key: Option<&str>) -> AiTestResult {
    {
        let started = Instant::now();
        let result = block_on(async { fetch_models(p, key).await });
        let ms = started.elapsed().as_millis() as u64;
        match result {
            Ok(models) if models.is_empty() => AiTestResult {
                ok: true,
                message: if p.kind == "lmstudio" {
                    "Connected, but LM Studio reports no model. Download or load one there.".into()
                } else {
                    "Connected, but the server lists no models.".into()
                },
                ms,
                models,
            },
            Ok(models) => {
                let note = if p.model.is_empty() {
                    "Pick a model below.".to_string()
                } else if models.iter().any(|m| m.id == p.model) {
                    format!("{} is available.", p.model)
                } else {
                    format!("Connected, but the server doesn't list {}.", p.model)
                };
                AiTestResult {
                    ok: true,
                    message: format!("Connected: {} models. {note}", models.len()),
                    ms,
                    models,
                }
            }
            Err(e) => AiTestResult {
                ok: false,
                message: e,
                ms,
                models: vec![],
            },
        }
    }
}

/// Runs `f` on its own thread. The core's synchronous code may start a runtime of its own, which is not allowed on a
/// thread that is already inside one (the request runs in one), and a panic in a tool must not end the request.
fn off_runtime<T: Send>(f: impl FnOnce() -> T + Send) -> Result<T, String> {
    std::thread::scope(|s| s.spawn(f).join()).map_err(|_| "it stopped unexpectedly".to_string())
}

/// One small current-thread runtime per call: the rest of the core is synchronous.
fn block_on<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to start the AI runtime")
        .block_on(f)
}

fn client(local: bool, total: Option<Duration>) -> reqwest::Client {
    let mut b = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .user_agent("OLS");
    if let Some(t) = total {
        b = b.timeout(t);
    }
    if local {
        // A proxy set for the internet must not sit between the app and a model on this computer.
        b = b.no_proxy();
    }
    b.build().expect("failed to build the HTTP client")
}

fn authorize(
    rb: reqwest::RequestBuilder,
    p: &AiProvider,
    key: Option<&str>,
) -> Result<reqwest::RequestBuilder, String> {
    let mut rb = rb;
    if let Some(k) = key {
        if !p.local && p.base_url.starts_with("http://") {
            return Err(
                "a key is only sent over https to a server that isn't on this computer".into(),
            );
        }
        rb = rb.bearer_auth(k);
    }
    if p.kind == "openrouter" {
        rb = rb
            .header("HTTP-Referer", "https://github.com/kz370/OpenLocalServer")
            .header("X-Title", "OLS");
    }
    Ok(rb)
}

fn api_error(status: u16, body: &str) -> String {
    let message = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| {
            v["error"]["message"]
                .as_str()
                .or_else(|| v["error"].as_str())
                .or_else(|| v["message"].as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| head(body.trim(), 300));
    let hint = match status {
        401 | 403 => " Check the API key.",
        404 => " Check the address and the model name.",
        429 => " The provider is rate limiting; try again shortly.",
        _ => "",
    };
    format!("The server answered {status}: {message}{hint}")
}

fn per_million(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.parse::<f64>().ok()))
}

/// LM Studio's own model lists. Its OpenAI-style `/v1/models` (and, in current versions, `/api/v0/models`) list only what is
/// loaded, which hides every other downloaded model from the picker. `/api/v1/models` lists all of them. Older versions
/// have only the v0 list, which is used when the newer one isn't there. `None` when neither works.
async fn lmstudio_models(p: &AiProvider, key: Option<&str>) -> Option<Vec<AiModel>> {
    async fn get(p: &AiProvider, key: Option<&str>, path: &str) -> Option<Value> {
        let mut url = reqwest::Url::parse(p.base_url.trim()).ok()?;
        url.set_path(path);
        let resp = authorize(
            client(p.local, Some(Duration::from_secs(10))).get(url),
            p,
            key,
        )
        .ok()?
        .send()
        .await
        .ok()?;
        if !resp.status().is_success() {
            return None;
        }
        serde_json::from_str(&resp.text().await.ok()?).ok()
    }
    let mut models: Vec<AiModel> = Vec::new();
    if let Some(v) = get(p, key, "/api/v1/models").await {
        for m in v["models"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|m| m["type"].as_str() != Some("embedding"))
        {
            let Some(id) = m["key"].as_str().map(str::to_string) else {
                continue;
            };
            let label = m["display_name"].as_str().unwrap_or(&id);
            let loaded = m["loaded_instances"]
                .as_array()
                .is_some_and(|l| !l.is_empty());
            let name = format!("{label}{}", if loaded { " (loaded)" } else { "" });
            models.push(AiModel {
                name,
                context: m["max_context_length"].as_u64(),
                prompt_per_m: None,
                completion_per_m: None,
                id,
            });
        }
    } else if let Some(v) = get(p, key, "/api/v0/models").await {
        // Embedding models can't chat.
        for m in v["data"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|m| m["type"].as_str() != Some("embeddings"))
        {
            let Some(id) = m["id"].as_str().map(str::to_string) else {
                continue;
            };
            let name = if m["state"].as_str() == Some("loaded") {
                format!("{id} (loaded)")
            } else {
                id.clone()
            };
            models.push(AiModel {
                name,
                context: m["max_context_length"].as_u64(),
                prompt_per_m: None,
                completion_per_m: None,
                id,
            });
        }
    }
    models.sort_by(|a, b| a.id.cmp(&b.id));
    (!models.is_empty()).then_some(models)
}

async fn fetch_models(p: &AiProvider, key: Option<&str>) -> Result<Vec<AiModel>, String> {
    if p.kind == "lmstudio" {
        if let Some(models) = lmstudio_models(p, key).await {
            return Ok(models);
        }
    }
    let rb = authorize(
        client(p.local, Some(Duration::from_secs(20))).get(endpoint(&p.base_url, "models")),
        p,
        key,
    )?;
    let resp = rb.send().await.map_err(|e| {
        if p.local {
            format!(
                "Nothing answered at {}. Is the server running? ({e})",
                p.base_url
            )
        } else {
            format!("Couldn't reach {}: {e}", url_host(&p.base_url))
        }
    })?;
    let status = resp.status();
    let body = resp.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(api_error(status.as_u16(), &body));
    }
    let v: Value = serde_json::from_str(&body)
        .map_err(|_| "The server's answer isn't an OpenAI-style model list.".to_string())?;
    let items = v
        .get("data")
        .and_then(Value::as_array)
        .or_else(|| v.as_array())
        .ok_or("The server's answer isn't an OpenAI-style model list.")?;
    let mut models: Vec<AiModel> = items
        .iter()
        .filter_map(|m| {
            let id = m["id"].as_str()?.to_string();
            // OpenRouter: dollars per token as strings. Hugging Face's router: dollars per million per provider.
            let (input, output) = match (
                per_million(&m["pricing"]["prompt"]),
                per_million(&m["pricing"]["completion"]),
            ) {
                (Some(i), Some(o)) => (Some(i * 1e6), Some(o * 1e6)),
                _ => (
                    per_million(&m["providers"][0]["pricing"]["input"]),
                    per_million(&m["providers"][0]["pricing"]["output"]),
                ),
            };
            Some(AiModel {
                name: m["name"].as_str().unwrap_or(&id).to_string(),
                context: m["context_length"]
                    .as_u64()
                    .or_else(|| m["providers"][0]["context_length"].as_u64()),
                prompt_per_m: input,
                completion_per_m: output,
                id,
            })
        })
        .collect();
    models.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(models)
}

// ------------------------------------------------------------------------------ building a prompt

struct Context {
    instructions: String,
    question: String,
    attachments: Vec<(String, String)>,
}

fn need<'a>(value: &'a Option<String>, what: &str) -> Result<&'a str, CoreError> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| fail(format!("{what} is needed for this request")))
}

fn need_project(req: &AiRequest) -> Result<&str, CoreError> {
    req.project_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| fail("choose a project for this request"))
}

fn format_request(r: &crate::inspector::RecordedRequest) -> String {
    let headers = |h: &[(String, String)]| {
        h.iter()
            .map(|(k, v)| format!("{k}: {v}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "{} {} -> {} in {} ms{}\n{}\n\n{}\n\nResponse headers:\n{}\n\n{}",
        r.method,
        r.path,
        r.status,
        r.duration_ms,
        r.error
            .as_ref()
            .map(|e| format!(" (error: {e})"))
            .unwrap_or_default(),
        headers(&r.request_headers),
        r.request_body.clone().unwrap_or_default(),
        headers(&r.response_headers),
        r.response_body.clone().unwrap_or_default()
    )
}

impl Inner {
    fn ai_context(&self, req: &AiRequest) -> Result<Context, CoreError> {
        let mut attachments: Vec<(String, String)> = Vec::new();
        let (instructions, question) = match req.feature.as_str() {
            "explain" => {
                let subject = need(&req.text, "the problem to explain")?;
                let title = req.title.clone().unwrap_or_default();
                attachments.push((
                    "The problem".into(),
                    if title.is_empty() {
                        subject.to_string()
                    } else {
                        format!("{title}\n{subject}")
                    },
                ));
                let health: Vec<String> = self
                    .environment_health()
                    .into_iter()
                    .filter(|h| h.status != "ok")
                    .map(|h| format!("[{}] {}: {}", h.status, h.label, h.detail))
                    .collect();
                if !health.is_empty() {
                    attachments.push((
                        "Environment health (what is not OK)".into(),
                        health.join("\n"),
                    ));
                }
                let findings: Vec<String> = self
                    .diagnose()
                    .into_iter()
                    .filter(|f| !f.ignored)
                    .map(|f| format!("{} | cause: {}", f.problem, f.cause))
                    .collect();
                if !findings.is_empty() {
                    attachments.push(("Diagnostics findings".into(), findings.join("\n")));
                }
                if let Ok(lines) = self.read_log("web:error", 40) {
                    if !lines.is_empty() {
                        attachments.push((
                            "Web server error log (last 40 lines)".into(),
                            lines.join("\n"),
                        ));
                    }
                }
                ("Explain what went wrong in plain words, why it happened, and how to fix it. If commands you may propose fix it, end with a plan.".to_string(), req.question.clone().unwrap_or_else(|| "Explain this and propose a fix.".into()))
            }
            "config" if req.kind == "manifest" => {
                let id = need_project(req)?;
                let project = self
                    .projects
                    .lock()
                    .unwrap()
                    .get(id)
                    .ok_or_else(|| fail("that project isn't registered"))?;
                let root = PathBuf::from(&project.path);
                let mut names: Vec<String> = std::fs::read_dir(&root)
                    .map(|d| {
                        d.flatten()
                            .map(|e| {
                                format!(
                                    "{}{}",
                                    e.file_name().to_string_lossy(),
                                    if e.path().is_dir() { "/" } else { "" }
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                names.sort();
                names.truncate(80);
                attachments.push((
                    format!("Project folder: {}", project.name),
                    names.join("\n"),
                ));
                for f in [
                    "composer.json",
                    "package.json",
                    "requirements.txt",
                    "pyproject.toml",
                ] {
                    if let Ok(t) = std::fs::read_to_string(root.join(f)) {
                        attachments.push((f.to_string(), head(&t, 3000)));
                    }
                }
                for f in [".env.example", ".env.sample"] {
                    if let Ok(t) = std::fs::read_to_string(root.join(f)) {
                        attachments.push((
                            format!("{f} (values hidden)"),
                            head(&redact_env_file(&t), 2000),
                        ));
                        break;
                    }
                }
                if let Ok(t) = std::fs::read_to_string(root.join("README.md")) {
                    attachments.push(("README.md (start)".into(), head(&t, 1500)));
                }
                if let Ok(m) = self.derive_manifest(id) {
                    if let Ok(y) = serde_yaml_ng::to_string(&m) {
                        attachments.push(("Detected example environment.yaml".into(), y));
                    }
                }
                if let Ok(info) = self.manifest_info(id) {
                    if let Some(t) = info.text {
                        attachments.push(("Current environment.yaml".into(), t));
                    }
                }
                (
                    MANIFEST_INSTRUCTIONS.to_string(),
                    req.question.clone().unwrap_or_else(|| {
                        "Draft the environment manifest for this project.".into()
                    }),
                )
            }
            "config" if req.kind == "htaccess" => {
                let block = req.text.as_deref().unwrap_or("");
                attachments.push(("Current .htaccess".into(), block.to_string()));
                ("Suggest a complete Apache .htaccess file. Return the complete file in one ```apache fenced block, then briefly explain the changes. Do not apply it; the user will review it.".to_string(), req.question.clone().unwrap_or_else(|| "Improve this .htaccess file for this PHP site.".into()))
            }
            "config" => {
                let block = need(&req.text, "the config to look at")?;
                attachments.push((
                    req.title.clone().unwrap_or_else(|| "The config".into()),
                    block.to_string(),
                ));
                ("Explain what the config does. If the user wants a change, show it as a unified diff in one ```diff block, then explain it briefly. Never invent directives the server doesn't have.".to_string(), req.question.clone().unwrap_or_else(|| "Explain this config and point out anything wrong.".into()))
            }
            "logs" => {
                let question = need(&req.question, "a question")?.to_string();
                if let Some(picked) = req.text.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
                    let cleaned = clean(picked);
                    if cleaned.len() <= EXCERPT_INLINE {
                        attachments.push((
                            format!(
                                "Selected log lines (user-picked {} characters)",
                                cleaned.len()
                            ),
                            cleaned,
                        ));
                    } else if let Ok(file) = self.save_excerpt(&cleaned) {
                        attachments.push((
                            format!(
                                "Selected log lines (first {} of {} chars in full excerpt file {file}; use read_excerpt for rest)",
                                EXCERPT_INLINE,
                                cleaned.len(),
                                file = file
                            ),
                            head(&cleaned, EXCERPT_INLINE),
                        ));
                    } else {
                        attachments.push((
                            format!(
                                "Selected log lines (user-picked {} characters, truncated)",
                                cleaned.len()
                            ),
                            head(&cleaned, EXCERPT_INLINE),
                        ));
                    }
                }
                let sources: Vec<String> = if req.log_sources.is_empty() {
                    vec!["app".into(), "web:error".into()]
                } else {
                    req.log_sources.clone()
                };
                for s in sources.iter().take(6) {
                    let lines = self.read_log(s, 150).map_err(|e| fail(e.to_string()))?;
                    if !lines.is_empty() {
                        attachments.push((
                            format!("Log {s} (last {} lines)", lines.len()),
                            lines.join("\n"),
                        ));
                    }
                }
                ("Answer from the logs. Quote the lines you used verbatim, each in a > blockquote, and say when the logs don't contain the answer.".to_string(), question)
            }
            "traffic" => {
                let tunnel = need(&req.tunnel_id, "a tunnel")?;
                let all = self.tunnel_requests(tunnel)?;
                let picked: Vec<_> = if req.request_ids.is_empty() {
                    all.iter().rev().take(3).collect()
                } else {
                    all.iter()
                        .filter(|r| req.request_ids.contains(&r.id))
                        .collect()
                };
                if picked.is_empty() {
                    return Err(fail("no recorded requests to look at"));
                }
                for r in picked {
                    attachments.push((
                        format!("Request #{} (secrets already hidden)", r.id),
                        format_request(r),
                    ));
                }
                match req.kind.as_str() {
                    "handler" => ("Write a webhook handler that receives this request and responds correctly. Use the language or framework the user names; otherwise PHP. Give the code in one fenced block and a short note on validating the signature.".to_string(), req.question.clone().unwrap_or_else(|| "Write a handler for this webhook.".into())),
                    "k6" => {
                        let id = need_project(req)?;
                        let sites: Vec<String> = self.load_overview(id)?.sites.iter().filter(|s| !s.public).map(|s| s.host.clone()).collect();
                        attachments.push(("Load test rules".into(), format!("The script runs against __ENV.BASE_URL only (the project's own sites: {}). Keep virtual users at or under {}.", sites.join(", "), self.load_overview(id)?.max_vus)));
                        ("Write a k6 script that replays this kind of traffic. Reply with the script in one ```javascript block, using `${__ENV.BASE_URL}` for the address, sensible stages and thresholds, and check() on responses. Then a short note.".to_string(), req.question.clone().unwrap_or_else(|| "Write a k6 script from these requests.".into()))
                    }
                    _ => ("Explain what this request is, who most likely sent it, and whether the response was right. Say what to check if it failed.".to_string(), req.question.clone().unwrap_or_else(|| "Explain this request.".into())),
                }
            }
            "commit" => {
                let id = need_project(req)?;
                let diff = self.git_staged_diff(id)?;
                if diff.trim().is_empty() {
                    return Err(fail(
                        "nothing is staged; stage the changes to describe first",
                    ));
                }
                attachments.push(("Staged changes".into(), tail(&diff, 24_000)));
                if let Ok(log) = self.git_log(id, 6) {
                    if !log.is_empty() {
                        attachments.push((
                            "Recent commit subjects (for style)".into(),
                            log.iter()
                                .map(|c| c.subject.clone())
                                .collect::<Vec<_>>()
                                .join("\n"),
                        ));
                    }
                }
                ("Write a Git commit message for the staged changes: an imperative subject of at most 72 characters, a blank line, then a short body only when it helps. Match the style of the recent subjects. Reply with the message only, no fences.".to_string(), req.question.clone().unwrap_or_else(|| "Write the commit message.".into()))
            }
            "palette" => {
                let question = need(&req.question, "what you want")?.to_string();
                let apps: Vec<String> = self
                    .catalog
                    .lock()
                    .unwrap()
                    .list()
                    .into_iter()
                    .map(|a| format!("{}: {} ({})", a.id, a.name, a.description))
                    .collect();
                attachments.push(("Quick Apps".into(), apps.join("\n")));
                let projects: Vec<String> = self
                    .projects
                    .lock()
                    .unwrap()
                    .list()
                    .into_iter()
                    .map(|p| format!("{}: {}", p.id, p.name))
                    .collect();
                attachments.push(("Projects".into(), projects.join("\n")));
                ("Turn the request into something the user can confirm. Prefer one start_quick_app step when a Quick App fits (use its id; put the user's wishes in values, and read the app with the tools when unsure of its fields). Otherwise say what to do by hand. Explain in one or two sentences.".to_string(), question)
            }
            other => {
                return Err(fail(format!(
                    "'{other}' isn't something the assistant does"
                )))
            }
        };
        Ok(Context {
            instructions,
            question,
            attachments,
        })
    }

    /// The exact request a provider would receive, redacted. Nothing is sent.
    pub fn ai_prompt(&self, req: &AiRequest) -> Result<AiPrompt, CoreError> {
        let cfg = self.ai_settings();
        let provider = pick_provider(&cfg, &req.feature).ok_or_else(|| CoreError::failed_fix("No AI provider is set up.", "The assistant needs a model to talk to.", "Add LM Studio, Hugging Face, OpenRouter or another server in Settings → AI assistant."))?;
        let ctx = self.ai_context(req)?;
        let mut user = clean(&ctx.question);
        let mut labels = Vec::new();
        let mut total = 0;
        for (label, text) in &ctx.attachments {
            let body = head(&clean(text), MAX_PART);
            if total + body.len() > MAX_TOTAL {
                labels.push(format!("{label} (left out: too large)"));
                continue;
            }
            total += body.len();
            labels.push(format!("{label} ({} characters)", body.len()));
            user.push_str(&format!("\n\n### {label}\n```\n{body}\n```"));
        }
        let mut system = format!("{SYSTEM}\n\n{}", ctx.instructions);
        if !matches!(req.feature.as_str(), "commit" | "logs") {
            system.push_str(&format!("\n\n{PLAN_DOC}"));
        }
        Ok(AiPrompt {
            provider_id: provider.id.clone(),
            provider_name: provider.name.clone(),
            model: provider.model.clone(),
            local: provider.local,
            host: url_host(&provider.base_url),
            messages: vec![
                PromptMessage {
                    role: "system".into(),
                    content: system,
                },
                PromptMessage {
                    role: "user".into(),
                    content: user,
                },
            ],
            attachments: labels,
            tools: if provider.tools { tool_names() } else { vec![] },
        })
    }
}

// ------------------------------------------------------------------------------ running a request

impl Inner {
    pub fn ai_start(
        self: &Arc<Self>,
        req: AiRequest,
        confirm_remote: bool,
    ) -> Result<AiJobView, CoreError> {
        let cfg = self.ai_settings();
        if !cfg.enabled {
            return Err(CoreError::failed_fix(
                "The AI assistant is off.",
                "It is opt-in, and nothing is sent until you turn it on.",
                "Turn it on in Settings → AI assistant.",
            ));
        }
        let provider = pick_provider(&cfg, &req.feature).cloned().ok_or_else(|| {
            CoreError::failed_fix(
                "No AI provider is set up.",
                "The assistant needs a model to talk to.",
                "Add a provider in Settings → AI assistant.",
            )
        })?;
        if provider.model.trim().is_empty() {
            return Err(CoreError::failed_fix(
                "No model is chosen.",
                format!("{} has no model set.", provider.name),
                "Pick a model in Settings → AI assistant.",
            ));
        }
        if !provider.local && !confirm_remote {
            return Err(CoreError::failed_fix(
                "The request wasn't sent.",
                format!(
                    "{} runs outside this computer ({}). What you send leaves it.",
                    provider.name,
                    url_host(&provider.base_url)
                ),
                "Confirm that you want to send it, or choose a local provider.",
            ));
        }
        let prompt = self.ai_prompt(&req)?;
        let key = get_key(&provider.id);

        let id = format!(
            "ai-{}-{}",
            now_ms(),
            self.ai.counter.fetch_add(1, Ordering::Relaxed)
        );
        let cancel = Arc::new(AtomicBool::new(false));
        let view = AiJobView {
            id: id.clone(),
            feature: req.feature.clone(),
            state: "running".into(),
            partial: String::new(),
            activity: vec![],
            answer: None,
            error: None,
            provider: provider.name.clone(),
            local: provider.local,
        };
        let job = Arc::new(Mutex::new(Job {
            view: view.clone(),
            cancel: cancel.clone(),
        }));
        {
            let mut jobs = self.ai.jobs.lock().unwrap();
            if jobs.len() >= 20 {
                let finished: Vec<String> = jobs
                    .iter()
                    .filter(|(_, j)| j.lock().unwrap().view.state != "running")
                    .map(|(k, _)| k.clone())
                    .collect();
                for k in finished.into_iter().take(10) {
                    jobs.remove(&k);
                }
            }
            jobs.insert(id, job.clone());
        }
        let inner = self.clone();
        std::thread::Builder::new()
            .name("ols-ai".into())
            .spawn(move || {
                let outcome = block_on(run_chat(
                    inner,
                    provider,
                    key,
                    prompt,
                    req,
                    job.clone(),
                    cancel.clone(),
                ));
                let mut j = job.lock().unwrap();
                match outcome {
                    _ if cancel.load(Ordering::Relaxed) => j.view.state = "cancelled".into(),
                    Ok(answer) => {
                        j.view.partial = answer.text.clone();
                        j.view.answer = Some(answer);
                        j.view.state = "done".into();
                    }
                    Err(e) => {
                        j.view.error = Some(e);
                        j.view.state = "failed".into();
                    }
                }
            })
            .map_err(|e| fail(format!("the request couldn't start: {e}")))?;
        Ok(view)
    }

    pub fn ai_job(&self, id: &str) -> Result<AiJobView, CoreError> {
        let jobs = self.ai.jobs.lock().unwrap();
        let job = jobs
            .get(id)
            .ok_or_else(|| fail("that request is no longer known"))?;
        let v = job.lock().unwrap().view.clone();
        Ok(v)
    }

    pub fn ai_cancel(&self, id: &str) -> Result<AiJobView, CoreError> {
        {
            let jobs = self.ai.jobs.lock().unwrap();
            let job = jobs
                .get(id)
                .ok_or_else(|| fail("that request is no longer known"))?;
            job.lock().unwrap().cancel.store(true, Ordering::Relaxed);
        }
        self.ai_job(id)
    }

    /// Reads the plan block of an answer: every step parsed, checked against the allowlist and, for a Quick App,
    /// planned. What can't be proposed comes back as a reason.
    fn ai_check_plan(&self, text: &str) -> (Vec<AiAction>, Vec<String>) {
        let mut actions = Vec::new();
        let mut rejected = Vec::new();
        let Some(block) = fences(text)
            .into_iter()
            .rev()
            .find(|f| f.lang == "ols-plan")
        else {
            return (actions, rejected);
        };
        let value: Value = match serde_json::from_str(block.body.trim()) {
            Ok(v) => v,
            Err(_) => {
                return (
                    actions,
                    vec!["The proposed plan wasn't valid JSON, so it was ignored.".into()],
                )
            }
        };
        let items = match value {
            Value::Array(a) => a,
            other => vec![other],
        };
        for item in items.into_iter().take(MAX_ACTIONS) {
            let label = item["label"].as_str().unwrap_or("").trim().to_string();
            let kind = item["command"]["type"].as_str().unwrap_or("?").to_string();
            let label = if label.is_empty() {
                kind.clone()
            } else {
                label
            };
            let cmd = match serde_json::from_value::<CoreCommand>(item["command"].clone()) {
                Ok(c) => c,
                Err(e) => {
                    rejected.push(format!(
                        "{label}: not a command the app understands ({kind}: {e})"
                    ));
                    continue;
                }
            };
            if !allowed(&cmd) {
                rejected.push(format!("{label}: the assistant may not propose {kind}"));
                continue;
            }
            let cmd = normalise(cmd);
            if let CoreCommand::StartQuickApp { id, values, .. } = &cmd {
                match self.plan_quick_app(id, values) {
                    Ok(r) if r.plan.is_some() => {}
                    Ok(r) => {
                        rejected.push(format!(
                            "{label}: {}",
                            r.errors
                                .iter()
                                .map(|e| e.message.clone())
                                .collect::<Vec<_>>()
                                .join("; ")
                        ));
                        continue;
                    }
                    Err(e) => {
                        rejected.push(format!("{label}: {e}"));
                        continue;
                    }
                }
            }
            actions.push(AiAction {
                label,
                destructive: destructive(&cmd),
                command: cmd,
            });
        }
        (actions, rejected)
    }

    fn ai_price(&self, provider: &AiProvider, model: &str) -> Option<(f64, f64)> {
        let cached = self
            .ai
            .prices
            .lock()
            .unwrap()
            .get(&provider.base_url)
            .filter(|(at, _)| at.elapsed() < Duration::from_secs(600))
            .map(|(_, m)| m.clone());
        let models = match cached {
            Some(m) => m,
            None => {
                let m = block_on(fetch_models(provider, get_key(&provider.id).as_deref())).ok()?;
                self.ai
                    .prices
                    .lock()
                    .unwrap()
                    .insert(provider.base_url.clone(), (Instant::now(), m.clone()));
                m
            }
        };
        let m = models.iter().find(|m| m.id == model)?;
        Some((m.prompt_per_m?, m.completion_per_m?))
    }
}

struct ToolCall {
    id: String,
    name: String,
    args: String,
}

#[derive(Default)]
struct Completion {
    content: String,
    calls: Vec<ToolCall>,
    usage: Option<(u64, u64)>,
}

enum ChatError {
    Cancelled,
    Status(u16, String),
    Other(String),
}

impl ChatError {
    fn text(self) -> String {
        match self {
            ChatError::Cancelled => "cancelled".into(),
            ChatError::Status(s, body) => api_error(s, &body),
            ChatError::Other(e) => e,
        }
    }
}

fn apply_chunk(v: &Value, out: &mut Completion, job: &Arc<Mutex<Job>>) -> Result<(), ChatError> {
    if let Some(err) = v.get("error") {
        let msg = err["message"]
            .as_str()
            .or_else(|| err.as_str())
            .unwrap_or("the provider reported an error");
        return Err(ChatError::Other(msg.to_string()));
    }
    if let Some(u) = v.get("usage").filter(|u| u.is_object()) {
        out.usage = Some((
            u["prompt_tokens"].as_u64().unwrap_or(0),
            u["completion_tokens"].as_u64().unwrap_or(0),
        ));
    }
    for choice in v["choices"].as_array().into_iter().flatten() {
        let delta = if choice.get("delta").is_some() {
            &choice["delta"]
        } else {
            &choice["message"]
        };
        if let Some(text) = delta["content"].as_str() {
            out.content.push_str(text);
            job.lock().unwrap().view.partial = out.content.clone();
        }
        for (pos, tc) in delta["tool_calls"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            let index = tc["index"].as_u64().map(|i| i as usize).unwrap_or(pos);
            while out.calls.len() <= index {
                out.calls.push(ToolCall {
                    id: String::new(),
                    name: String::new(),
                    args: String::new(),
                });
            }
            let call = &mut out.calls[index];
            if let Some(id) = tc["id"].as_str() {
                call.id = id.to_string();
            }
            if let Some(n) = tc["function"]["name"].as_str() {
                call.name.push_str(n);
            }
            match &tc["function"]["arguments"] {
                Value::String(a) => call.args.push_str(a),
                Value::Object(_) => call.args = tc["function"]["arguments"].to_string(),
                _ => {}
            }
        }
    }
    Ok(())
}

async fn wait_cancel(cancel: &AtomicBool) {
    while !cancel.load(Ordering::Relaxed) {
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}

async fn complete(
    client: &reqwest::Client,
    p: &AiProvider,
    key: Option<&str>,
    messages: &[Value],
    tools: &[Value],
    job: &Arc<Mutex<Job>>,
    cancel: &AtomicBool,
) -> Result<Completion, ChatError> {
    let mut body = json!({"model": p.model, "messages": messages, "stream": true});
    if !tools.is_empty() {
        body["tools"] = json!(tools);
    }
    let rb = authorize(
        client
            .post(endpoint(&p.base_url, "chat/completions"))
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body.to_string()),
        p,
        key,
    )
    .map_err(ChatError::Other)?;
    let sent = tokio::select! {
        r = rb.send() => r,
        _ = wait_cancel(cancel) => return Err(ChatError::Cancelled),
    };
    let resp = sent.map_err(|e| {
        ChatError::Other(if p.local {
            format!(
                "Nothing answered at {}. Is the server running? ({e})",
                p.base_url
            )
        } else {
            format!("Couldn't reach {}: {e}", url_host(&p.base_url))
        })
    })?;
    let status = resp.status();
    if !status.is_success() {
        return Err(ChatError::Status(
            status.as_u16(),
            resp.text().await.unwrap_or_default(),
        ));
    }
    let mut out = Completion::default();
    let is_json = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|t| t.contains("application/json"));
    if is_json {
        // A server that ignored `stream`.
        let text = resp
            .text()
            .await
            .map_err(|e| ChatError::Other(e.to_string()))?;
        let v: Value = serde_json::from_str(&text)
            .map_err(|_| ChatError::Other("The server's answer isn't a chat completion.".into()))?;
        apply_chunk(&v, &mut out, job)?;
        return Ok(out);
    }
    let mut stream = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    let handle = |line: &str, out: &mut Completion| -> Result<bool, ChatError> {
        let Some(data) = line.trim().strip_prefix("data:") else {
            return Ok(false);
        };
        let data = data.trim();
        if data == "[DONE]" {
            return Ok(true);
        }
        if let Ok(v) = serde_json::from_str::<Value>(data) {
            apply_chunk(&v, out, job)?;
        }
        Ok(false)
    };
    'read: loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(ChatError::Cancelled);
        }
        let next = tokio::select! {
            n = tokio::time::timeout(IDLE, stream.next()) => n.map_err(|_| ChatError::Other("The model stopped answering (no data for two minutes).".into()))?,
            _ = wait_cancel(cancel) => return Err(ChatError::Cancelled),
        };
        let Some(chunk) = next else { break };
        buf.extend_from_slice(
            &chunk.map_err(|e| ChatError::Other(format!("The connection broke: {e}")))?,
        );
        while let Some(pos) = buf.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = buf.drain(..=pos).collect();
            if handle(&String::from_utf8_lossy(&line), &mut out)? {
                break 'read;
            }
        }
    }
    if !buf.is_empty() {
        let _ = handle(&String::from_utf8_lossy(&buf), &mut out)?;
    }
    Ok(out)
}

async fn run_chat(
    inner: Arc<Inner>,
    provider: AiProvider,
    key: Option<String>,
    prompt: AiPrompt,
    req: AiRequest,
    job: Arc<Mutex<Job>>,
    cancel: Arc<AtomicBool>,
) -> Result<AiAnswer, String> {
    let client = client(provider.local, None);
    let mut messages: Vec<Value> = prompt
        .messages
        .iter()
        .map(|m| json!({"role": m.role, "content": m.content}))
        .collect();
    let mut tools = if provider.tools { tool_defs() } else { vec![] };
    let mut used: Vec<String> = prompt.attachments.clone();
    let (mut tokens_in, mut tokens_out, mut counted) = (0u64, 0u64, false);
    let note = |s: String| job.lock().unwrap().view.activity.push(s);

    let mut round = 0;
    let final_text = loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        job.lock().unwrap().view.partial.clear();
        let done = match complete(
            &client,
            &provider,
            key.as_deref(),
            &messages,
            &tools,
            &job,
            &cancel,
        )
        .await
        {
            Ok(c) => c,
            // A model or server that doesn't take tools: carry on without them.
            Err(ChatError::Status(400 | 404 | 422, _)) if !tools.is_empty() => {
                note("This model doesn't take tools; continuing without them.".into());
                tools.clear();
                continue;
            }
            Err(e) => return Err(e.text()),
        };
        if let Some((i, o)) = done.usage {
            tokens_in += i;
            tokens_out += o;
            counted = true;
        }
        if done.calls.is_empty() || tools.is_empty() {
            break done.content;
        }
        round += 1;
        if round > MAX_ROUNDS {
            break done.content;
        }
        let tool_calls: Vec<Value> = done.calls.iter().enumerate().map(|(n, c)| json!({"id": if c.id.is_empty() { format!("call_{n}") } else { c.id.clone() }, "type": "function", "function": {"name": c.name, "arguments": if c.args.is_empty() { "{}".to_string() } else { c.args.clone() }}})).collect();
        messages.push(json!({"role": "assistant", "content": if done.content.is_empty() { Value::Null } else { json!(done.content) }, "tool_calls": tool_calls}));
        for (n, c) in done.calls.iter().enumerate() {
            let args: Value = serde_json::from_str(if c.args.is_empty() { "{}" } else { &c.args })
                .unwrap_or(Value::Null);
            let label = format!(
                "{}({})",
                c.name,
                args.as_object()
                    .map(|o| o
                        .values()
                        .map(|v| v
                            .as_str()
                            .map(str::to_string)
                            .unwrap_or_else(|| v.to_string()))
                        .collect::<Vec<_>>()
                        .join(", "))
                    .unwrap_or_default()
            );
            note(format!("Read: {label}"));
            used.push(label);
            let result = match off_runtime(|| inner.ai_tool(&c.name, &args)).and_then(|r| r) {
                Ok(t) if t.trim().is_empty() => "(nothing)".to_string(),
                Ok(t) => t,
                Err(e) => format!("error: {e}"),
            };
            messages.push(json!({"role": "tool", "tool_call_id": if c.id.is_empty() { format!("call_{n}") } else { c.id.clone() }, "content": result}));
        }
    };
    if final_text.trim().is_empty() {
        return Err("The model returned an empty answer. Try again, or pick another model.".into());
    }

    let all = fences(&final_text);
    let plan_blocks: Vec<&Fence> = all.iter().filter(|f| f.lang == "ols-plan").collect();
    let (actions, mut rejected) =
        off_runtime(|| inner.ai_check_plan(&final_text)).unwrap_or_default();
    let mut answer = AiAnswer {
        provider: provider.name.clone(),
        model: provider.model.clone(),
        local: provider.local,
        actions,
        used,
        tokens_in: counted.then_some(tokens_in),
        tokens_out: counted.then_some(tokens_out),
        ..Default::default()
    };
    let mut hidden_blocks = plan_blocks.clone();
    if req.feature == "config" && req.kind == "htaccess" {
        hidden_blocks.extend(
            all.iter()
                .rev()
                .find(|f| matches!(f.lang.as_str(), "apache" | "htaccess")),
        );
    }
    let mut text = without_fences(&final_text, &hidden_blocks);

    match (req.feature.as_str(), req.kind.as_str()) {
        ("config", "manifest") => {
            if let Some(f) = all
                .iter()
                .rev()
                .find(|f| f.lang == "yaml" || f.lang == "yml")
            {
                match serde_yaml_ng::from_str::<crate::manifest::EnvironmentManifest>(&f.body) {
                    Ok(_) => answer.manifest = Some(format!("{}\n", f.body.trim_end())),
                    Err(e) => rejected.push(format!("The drafted manifest doesn't read as an environment.yaml ({e}), so it can't be saved.")),
                }
            }
        }
        ("config", "htaccess") => {
            if let Some(f) = all
                .iter()
                .rev()
                .find(|f| matches!(f.lang.as_str(), "apache" | "htaccess"))
            {
                if f.body.len() <= 256 * 1024 {
                    answer.file = Some(format!("{}\n", f.body.trim_end()));
                } else {
                    rejected.push(
                        "The drafted .htaccess file is larger than 256 KB and was not offered."
                            .into(),
                    );
                }
            }
        }
        ("traffic", "k6") => {
            if let Some(f) = all
                .iter()
                .rev()
                .find(|f| matches!(f.lang.as_str(), "javascript" | "js"))
            {
                let checked = match &req.project_id {
                    Some(id) => {
                        off_runtime(|| inner.load_check_script(id, &f.body)).and_then(|r| r)
                    }
                    None => Err("choose a project to save the script into".into()),
                };
                match checked {
                    Ok(()) => answer.script = Some(format!("{}\n", f.body.trim_end())),
                    Err(e) => rejected.push(format!(
                        "The drafted k6 script was refused by the load-test safety scan: {e}"
                    )),
                }
            }
        }
        ("commit", _) => {
            let message = text.trim().trim_matches('`').trim().to_string();
            if !message.is_empty() {
                answer.commit_message = Some(message);
            }
        }
        _ => {}
    }
    if answer.cost_usd.is_none() && provider.kind == "openrouter" && counted {
        if let Some((pi, po)) = off_runtime(|| inner.ai_price(&provider, &provider.model))
            .ok()
            .flatten()
        {
            answer.cost_usd = Some((tokens_in as f64 * pi + tokens_out as f64 * po) / 1e6);
        }
    }
    if text.is_empty() {
        text = "(the model answered with a plan only)".into();
    }
    answer.text = text;
    answer.rejected = rejected;
    Ok(answer)
}

// ------------------------------------------------------------------------------ applying an approved plan

impl Core {
    /// Runs the steps the user approved. Each is checked against the allowlist again here, whatever produced it, and
    /// destructive ones need `confirm_destructive`.
    pub fn ai_apply(
        &self,
        actions: Vec<CoreCommand>,
        confirm_destructive: bool,
    ) -> Vec<RepairStep> {
        let mut steps = Vec::new();
        for cmd in actions.into_iter().take(MAX_ACTIONS) {
            let cmd = normalise(cmd);
            let label = serde_json::to_value(&cmd)
                .ok()
                .and_then(|v| v["type"].as_str().map(str::to_string))
                .unwrap_or_default();
            if !allowed(&cmd) {
                steps.push(RepairStep {
                    label,
                    ok: false,
                    detail: "refused: the assistant may not run this".into(),
                });
                continue;
            }
            if destructive(&cmd) && !confirm_destructive {
                steps.push(RepairStep {
                    label,
                    ok: false,
                    detail: "skipped: this replaces or removes something; confirm it separately"
                        .into(),
                });
                continue;
            }
            tracing::info!(command = "ai_apply", step = %label);
            match self.run_fix(&cmd) {
                Ok(d) => steps.push(RepairStep {
                    label,
                    ok: true,
                    detail: d,
                }),
                Err(e) => steps.push(RepairStep {
                    label,
                    ok: false,
                    detail: e,
                }),
            }
        }
        steps
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// A stand-in OpenAI-compatible server on a loopback port. `respond(path, body, request_number)` returns the
    /// status, content type and body; every request is recorded. No test talks to a real provider.
    struct Mock {
        port: u16,
        seen: Arc<Mutex<Vec<(String, String)>>>,
    }

    impl Mock {
        fn start(
            respond: impl Fn(&str, &str, usize) -> (u16, String, String) + Send + 'static,
        ) -> Mock {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let seen: Arc<Mutex<Vec<(String, String)>>> = Arc::default();
            let log = seen.clone();
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let Ok(mut s) = stream else { break };
                    let mut data = Vec::new();
                    let mut chunk = [0u8; 4096];
                    let (path, body) = loop {
                        let n = s.read(&mut chunk).unwrap_or(0);
                        data.extend_from_slice(&chunk[..n]);
                        let text = String::from_utf8_lossy(&data).to_string();
                        if let Some(end) = text.find("\r\n\r\n") {
                            let head = &text[..end];
                            let len = head
                                .lines()
                                .find_map(|l| {
                                    l.to_ascii_lowercase()
                                        .strip_prefix("content-length:")
                                        .and_then(|v| v.trim().parse::<usize>().ok())
                                })
                                .unwrap_or(0);
                            if data.len() >= end + 4 + len || n == 0 {
                                let path = head
                                    .lines()
                                    .next()
                                    .unwrap_or("")
                                    .split_whitespace()
                                    .nth(1)
                                    .unwrap_or("")
                                    .to_string();
                                break (
                                    path,
                                    String::from_utf8_lossy(&data[end + 4..]).to_string(),
                                );
                            }
                        } else if n == 0 {
                            break (String::new(), String::new());
                        }
                    };
                    let count = {
                        let mut l = log.lock().unwrap();
                        l.push((path.clone(), body.clone()));
                        l.iter()
                            .filter(|(p, _)| p.ends_with("/chat/completions"))
                            .count()
                    };
                    let (status, ct, reply) = respond(&path, &body, count);
                    let _ = write!(s, "HTTP/1.1 {status} Mock\r\nContent-Type: {ct}\r\nConnection: close\r\n\r\n{reply}");
                }
            });
            Mock { port, seen }
        }

        fn url(&self) -> String {
            format!("http://127.0.0.1:{}/v1", self.port)
        }

        fn bodies(&self) -> Vec<String> {
            self.seen
                .lock()
                .unwrap()
                .iter()
                .filter(|(p, _)| p.ends_with("/chat/completions"))
                .map(|(_, b)| b.clone())
                .collect()
        }
    }

    fn json_reply(v: Value) -> (u16, String, String) {
        (200, "application/json".into(), v.to_string())
    }

    fn sse(parts: &[&str]) -> (u16, String, String) {
        let mut body = String::new();
        for p in parts {
            body.push_str(&format!(
                "data: {}\n\n",
                json!({"choices":[{"delta":{"content":p}}]})
            ));
        }
        body.push_str(&format!(
            "data: {}\n\n",
            json!({"choices":[],"usage":{"prompt_tokens":120,"completion_tokens":30}})
        ));
        body.push_str("data: [DONE]\n\n");
        (200, "text/event-stream".into(), body)
    }

    fn setup(mock_url: &str, enabled: bool) -> (Core, crate::test_support::IsolatedHome) {
        let home = crate::test_support::isolated_home();
        let core = Core::new(
            crate::settings::SettingsService::load(&home.paths).unwrap(),
            home.paths.clone(),
        );
        let i = core.inner();
        i.ai_save_provider(
            AiProvider {
                id: String::new(),
                name: "Mock".into(),
                kind: "custom".into(),
                base_url: mock_url.into(),
                model: "test-model".into(),
                tools: true,
                local: false,
                has_key: false,
            },
            None,
        )
        .unwrap();
        i.ai_save_settings(enabled, BTreeMap::new()).unwrap();
        (core, home)
    }

    fn explain(text: &str) -> AiRequest {
        AiRequest {
            feature: "explain".into(),
            title: Some("Web server won't start".into()),
            text: Some(text.into()),
            ..Default::default()
        }
    }

    fn wait(core: &Core, id: &str) -> AiJobView {
        let started = Instant::now();
        loop {
            let v = core.inner().ai_job(id).unwrap();
            if v.state != "running" || started.elapsed() > Duration::from_secs(20) {
                return v;
            }
            std::thread::sleep(Duration::from_millis(30));
        }
    }

    #[test]
    fn only_loopback_addresses_are_local() {
        for url in [
            "http://localhost:1234/v1",
            "http://127.0.0.1:11434/v1",
            "http://[::1]:8080/v1",
            "http://LOCALHOST/v1",
        ] {
            assert!(is_local_url(url), "{url}");
        }
        for url in [
            "https://openrouter.ai/api/v1",
            "http://192.168.1.20:1234/v1",
            "https://localhost.evil.example/v1",
            "not a url",
        ] {
            assert!(!is_local_url(url), "{url}");
        }
    }

    #[test]
    fn fenced_blocks_are_found_and_cut_out() {
        let text = "Start it.\n```ols-plan\n[{\"label\":\"x\"}]\n```\nDone.\n```yaml\na: 1\n```";
        let f = fences(text);
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].lang, "ols-plan");
        assert_eq!(f[1].body, "a: 1");
        assert_eq!(without_fences(text, &[&f[0], &f[1]]), "Start it.\nDone.");
    }

    #[test]
    fn a_plan_may_only_hold_allowlisted_commands() {
        let (core, _home) = setup("http://127.0.0.1:9/v1", true);
        let i = core.inner();
        let plan = r#"```ols-plan
[
 {"label":"Start MariaDB","command":{"type":"start_service","id":"mariadb"}},
 {"label":"Wipe","command":{"type":"restore_database","engine":"mariadb","database":"a","file":"x.sql"}},
 {"label":"Shell","command":{"type":"run_command","executable":"cmd","args":[],"cwd":null,"timeout_ms":1000}},
 {"label":"Secrets","command":{"type":"get_secret","key":"x"}},
 {"label":"Nonsense","command":{"type":"launch_rockets"}},
 {"label":"Set host","command":{"type":"set_env_value","project_id":"p","file":".env","key":"DB_HOST","value":"127.0.0.1"}}
]
```"#;
        let (actions, rejected) = i.ai_check_plan(plan);
        let kinds: Vec<&str> = actions.iter().map(|a| a.label.as_str()).collect();
        assert_eq!(kinds, ["Start MariaDB", "Set host"], "{rejected:?}");
        assert!(
            !actions[0].destructive && actions[1].destructive,
            "an env edit needs its own confirmation"
        );
        assert_eq!(rejected.len(), 4, "{rejected:?}");
        assert!(
            rejected.iter().any(|r| r.contains("run_command"))
                && rejected.iter().any(|r| r.contains("get_secret"))
                && rejected.iter().any(|r| r.contains("restore_database"))
        );
        assert_eq!(i.ai_check_plan("```ols-plan\nnot json\n```").1.len(), 1);
    }

    #[test]
    fn approved_steps_are_checked_again_and_destructive_ones_need_their_own_confirmation() {
        let (core, _home) = setup("http://127.0.0.1:9/v1", true);
        let steps = core.ai_apply(
            vec![
                CoreCommand::GetSecret { key: "x".into() },
                CoreCommand::RunCommand {
                    executable: "cmd".into(),
                    args: vec![],
                    cwd: None,
                    timeout_ms: 1000,
                },
                CoreCommand::SetEnvValue {
                    project_id: "p".into(),
                    file: ".env".into(),
                    key: "A".into(),
                    value: "b".into(),
                },
            ],
            false,
        );
        assert!(steps.iter().all(|s| !s.ok));
        assert!(steps[0].detail.contains("refused") && steps[1].detail.contains("refused"));
        assert!(steps[2].detail.contains("confirm"), "{}", steps[2].detail);
    }

    #[test]
    fn an_imported_quick_app_never_gets_its_approval_from_a_plan() {
        let cmd = normalise(CoreCommand::StartQuickApp {
            id: "x".into(),
            values: BTreeMap::new(),
            approval: Some("source".into()),
            allow_elevated: true,
        });
        assert!(matches!(
            cmd,
            CoreCommand::StartQuickApp {
                approval: None,
                allow_elevated: false,
                ..
            }
        ));
    }

    #[test]
    fn nothing_is_sent_while_the_assistant_is_off_or_to_a_remote_provider_without_confirmation() {
        let mock = Mock::start(|_, _, _| sse(&["hi"]));
        let (core, _home) = setup(&mock.url(), false);
        let e = core
            .inner()
            .ai_start(explain("boom"), false)
            .unwrap_err()
            .to_string();
        assert!(e.contains("off"), "{e}");
        assert!(mock.bodies().is_empty());

        // A provider outside this computer needs a confirmation for each request; the address is never contacted.
        let i = core.inner();
        i.ai_save_provider(
            AiProvider {
                id: String::new(),
                name: "Cloud".into(),
                kind: "openrouter".into(),
                base_url: "https://openrouter.ai/api/v1".into(),
                model: "m".into(),
                tools: true,
                local: false,
                has_key: false,
            },
            None,
        )
        .unwrap();
        i.ai_save_settings(
            true,
            BTreeMap::from([("explain".to_string(), "cloud".to_string())]),
        )
        .unwrap();
        let e = i.ai_start(explain("boom"), false).unwrap_err().to_string();
        assert!(e.contains("outside this computer"), "{e}");
        assert!(
            i.ai_prompt(&explain("boom")).unwrap().local == false,
            "the preview says where it goes"
        );
    }

    #[test]
    fn what_is_sent_is_redacted_and_the_preview_matches_it() {
        let mock = Mock::start(|_, _, _| sse(&["Looks fine."]));
        let (core, _home) = setup(&mock.url(), true);
        let i = core.inner();
        let secret = "DB_PASSWORD=hunter2\nAuthorization: Bearer abcdefghijklmnop\nkey sk-abcdefghijklmnopqrstuv\nmysql://root:s3cret@127.0.0.1/db";
        let req = explain(secret);
        let preview = i.ai_prompt(&req).unwrap();
        let shown = preview
            .messages
            .iter()
            .map(|m| m.content.clone())
            .collect::<Vec<_>>()
            .join("\n");
        for leaked in [
            "hunter2",
            "abcdefghijklmnop",
            "sk-abcdefghijklmnopqrstuv",
            "s3cret",
        ] {
            assert!(!shown.contains(leaked), "{leaked} is in the preview");
        }
        assert!(
            shown.contains("The problem")
                && preview.tools.contains(&"read_log".to_string())
                && preview.local
        );

        let job = i.ai_start(req, false).unwrap();
        let done = wait(&core, &job.id);
        assert_eq!(done.state, "done", "{:?}", done.error);
        let sent = mock.bodies().join("\n");
        for leaked in [
            "hunter2",
            "abcdefghijklmnop",
            "sk-abcdefghijklmnopqrstuv",
            "s3cret",
        ] {
            assert!(!sent.contains(leaked), "{leaked} reached the provider");
        }
        let body: Value = serde_json::from_str(&mock.bodies()[0]).unwrap();
        assert_eq!(
            body["messages"][1]["content"],
            json!(preview.messages[1].content),
            "the request is what the preview showed"
        );
    }

    #[test]
    fn an_answer_streams_and_carries_a_checked_plan() {
        let answer = "MariaDB isn't running, so the site can't reach its database.\n```ols-plan\n[{\"label\":\"Start MariaDB\",\"command\":{\"type\":\"start_service\",\"id\":\"mariadb\"}},{\"label\":\"Shell\",\"command\":{\"type\":\"run_command\",\"executable\":\"cmd\",\"args\":[],\"cwd\":null,\"timeout_ms\":1}}]\n```";
        let (first, rest) = (answer[..20].to_string(), answer[20..].to_string());
        let mock = Mock::start(move |_, _, _| sse(&[&first, &rest]));
        let (core, _home) = setup(&mock.url(), true);
        let job = core
            .inner()
            .ai_start(explain("connection refused 127.0.0.1:3306"), false)
            .unwrap();
        let done = wait(&core, &job.id);
        assert_eq!(done.state, "done", "{:?}", done.error);
        let a = done.answer.unwrap();
        assert!(
            a.text.starts_with("MariaDB isn't running") && !a.text.contains("ols-plan"),
            "{}",
            a.text
        );
        assert_eq!(a.actions.len(), 1);
        assert!(matches!(
            a.actions[0].command,
            CoreCommand::StartService { .. }
        ));
        assert_eq!(a.rejected.len(), 1, "{:?}", a.rejected);
        assert_eq!((a.tokens_in, a.tokens_out), (Some(120), Some(30)));
        assert!(a.local && a.cost_usd.is_none());
    }

    #[test]
    fn the_model_can_read_through_tools_and_the_results_are_redacted() {
        let mock = Mock::start(|_, body, n| {
            if n == 1 {
                let call = json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"read_env_names","arguments":"{\"project_id\":\"p1\"}"}},{"index":1,"id":"c2","function":{"name":"list_services","arguments":"{}"}}]}}]});
                (
                    200,
                    "text/event-stream".into(),
                    format!("data: {call}\n\ndata: [DONE]\n\n"),
                )
            } else {
                assert!(
                    body.contains("\"role\":\"tool\"")
                        && body.contains("c1")
                        && body.contains("c2"),
                    "the tool results go back: {body}"
                );
                sse(&["Done."])
            }
        });
        let (core, home) = setup(&mock.url(), true);
        let project = home.paths.root().join("proj");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join(".env"),
            "APP_KEY=base64:topsecretvalue\nDB_HOST=127.0.0.1\n",
        )
        .unwrap();
        let p = core
            .inner()
            .projects
            .lock()
            .unwrap()
            .register(project.to_str().unwrap())
            .unwrap();
        let mut req = explain("x");
        req.project_id = Some(p.id.clone());
        let job = core.inner().ai_start(req, false).unwrap();
        // The mock's first answer asks for project "p1", which doesn't exist: the error text goes back to the model.
        let done = wait(&core, &job.id);
        assert_eq!(done.state, "done", "{:?}", done.error);
        let a = done.answer.unwrap();
        assert!(
            a.used.iter().any(|u| u.starts_with("read_env_names"))
                && a.used.iter().any(|u| u.starts_with("list_services")),
            "{:?}",
            a.used
        );
        assert!(done.activity.iter().any(|l| l.starts_with("Read: ")));
        assert_eq!(mock.bodies().len(), 2);
        assert!(!mock.bodies().join("").contains("topsecretvalue"));
    }

    #[test]
    fn env_names_are_read_with_their_values_hidden() {
        let (core, home) = setup("http://127.0.0.1:9/v1", true);
        let project = home.paths.root().join("proj");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join(".env"),
            "APP_KEY=base64:topsecretvalue\nDB_HOST=127.0.0.1\n",
        )
        .unwrap();
        let p = core
            .inner()
            .projects
            .lock()
            .unwrap()
            .register(project.to_str().unwrap())
            .unwrap();
        let out = core
            .inner()
            .ai_tool("read_env_names", &json!({"project_id": p.id}))
            .unwrap();
        assert!(
            out.contains("APP_KEY=[redacted]")
                && out.contains("DB_HOST=[redacted]")
                && !out.contains("topsecretvalue"),
            "{out}"
        );
        assert!(core
            .inner()
            .ai_tool(
                "read_env_names",
                &json!({"project_id": p.id, "file": "..\\x"})
            )
            .is_err());
        assert!(
            core.inner().ai_tool("run_shell", &json!({})).is_err(),
            "there is no shell tool"
        );
    }

    #[test]
    fn a_server_that_refuses_tools_gets_the_request_again_without_them() {
        let mock = Mock::start(|_, body, _| {
            if body.contains("\"tools\"") {
                (
                    400,
                    "application/json".into(),
                    json!({"error":{"message":"tools are not supported"}}).to_string(),
                )
            } else {
                sse(&["Plain answer."])
            }
        });
        let (core, _home) = setup(&mock.url(), true);
        let done = wait(
            &core,
            &core.inner().ai_start(explain("x"), false).unwrap().id,
        );
        assert_eq!(done.state, "done", "{:?}", done.error);
        assert_eq!(done.answer.unwrap().text, "Plain answer.");
        assert!(done
            .activity
            .iter()
            .any(|l| l.contains("continuing without them")));
        let bodies = mock.bodies();
        assert!(
            bodies.len() == 2
                && bodies[0].contains("\"tools\"")
                && !bodies[1].contains("\"tools\"")
        );
    }

    #[test]
    fn a_provider_with_tools_off_is_never_offered_any() {
        let mock = Mock::start(|_, _, _| sse(&["ok"]));
        let (core, _home) = setup(&mock.url(), true);
        let i = core.inner();
        let mut p = i.ai_settings().providers.remove(0);
        p.tools = false;
        i.ai_save_provider(p, None).unwrap();
        assert!(i.ai_prompt(&explain("x")).unwrap().tools.is_empty());
        let done = wait(&core, &i.ai_start(explain("x"), false).unwrap().id);
        assert_eq!(done.state, "done", "{:?}", done.error);
        assert!(!mock.bodies()[0].contains("\"tools\""));
    }

    #[test]
    fn provider_errors_come_back_as_a_readable_failure() {
        let mock = Mock::start(|_, _, _| {
            (
                500,
                "application/json".into(),
                json!({"error":{"message":"model not loaded"}}).to_string(),
            )
        });
        let (core, _home) = setup(&mock.url(), true);
        let job = core.inner().ai_start(explain("x"), false).unwrap();
        let done = wait(&core, &job.id);
        assert_eq!(done.state, "failed");
        assert!(done.error.unwrap().contains("model not loaded"));
    }

    #[test]
    fn a_running_request_can_be_cancelled() {
        // A server that never answers.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let mut held = Vec::new();
            for s in listener.incoming().flatten() {
                held.push(s);
            }
        });
        let (core, _home) = setup(&format!("http://127.0.0.1:{port}/v1"), true);
        let job = core.inner().ai_start(explain("x"), false).unwrap();
        core.inner().ai_cancel(&job.id).unwrap();
        assert_eq!(wait(&core, &job.id).state, "cancelled");
        assert!(core.inner().ai_cancel("nope").is_err());
    }

    #[test]
    fn the_connection_test_lists_models_with_prices() {
        let mock = Mock::start(|path, _, _| {
            assert!(path.ends_with("/v1/models"), "{path}");
            json_reply(
                json!({"data":[{"id":"b/model","name":"B","context_length":8000,"pricing":{"prompt":"0.000001","completion":"0.000002"}},{"id":"a/model"}]}),
            )
        });
        let (core, _home) = setup(&mock.url(), true);
        let i = core.inner();
        let id = i.ai_settings().providers[0].id.clone();
        let r = i.ai_test(&id).unwrap();
        assert!(r.ok, "{}", r.message);
        assert_eq!(r.models.len(), 2);
        assert_eq!(r.models[0].id, "a/model");
        let b = &r.models[1];
        assert_eq!(
            (b.context, b.prompt_per_m, b.completion_per_m),
            (Some(8000), Some(1.0), Some(2.0))
        );
        assert!(
            r.message.contains("doesn't list test-model"),
            "{}",
            r.message
        );
        let down = i
            .ai_save_provider(
                AiProvider {
                    id: String::new(),
                    name: "Down".into(),
                    kind: "lmstudio".into(),
                    base_url: "http://127.0.0.1:9/v1".into(),
                    model: String::new(),
                    tools: true,
                    local: false,
                    has_key: false,
                },
                None,
            )
            .unwrap();
        let down_id = down
            .settings
            .providers
            .iter()
            .find(|p| p.name == "Down")
            .unwrap()
            .id
            .clone();
        let r = i.ai_test(&down_id).unwrap();
        assert!(
            !r.ok && r.message.contains("Is the server running"),
            "{}",
            r.message
        );
    }

    #[test]
    fn lm_studio_lists_every_downloaded_model_not_just_the_loaded_one() {
        let mock = Mock::start(|path, _, _| match path {
            "/api/v1/models" => json_reply(json!({"models":[
                {"type":"llm","key":"qwen-9b","display_name":"Qwen 9B","loaded_instances":[{"id":"x"}],"max_context_length":32768},
                {"type":"llm","key":"llama-8b","display_name":"Llama 8B","loaded_instances":[]},
                {"type":"embedding","key":"nomic-embed","display_name":"Nomic","loaded_instances":[]}]})),
            "/api/v0/models" => {
                json_reply(json!({"data":[{"id":"qwen-9b","type":"llm","state":"loaded"}]}))
            }
            _ => json_reply(json!({"data":[{"id":"qwen-9b"}]})),
        });
        let home = crate::test_support::isolated_home();
        let core = Core::new(
            crate::settings::SettingsService::load(&home.paths).unwrap(),
            home.paths.clone(),
        );
        let p = AiProvider {
            id: String::new(),
            name: "LM".into(),
            kind: "lmstudio".into(),
            base_url: mock.url(),
            model: String::new(),
            tools: true,
            local: true,
            has_key: false,
        };
        let r = core.inner().ai_probe(p.clone(), None).unwrap();
        let ids: Vec<&str> = r.models.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(
            ids,
            ["llama-8b", "qwen-9b"],
            "embedding models are left out"
        );
        assert_eq!(r.models[1].context, Some(32768));
        assert_eq!(r.models[1].name, "Qwen 9B (loaded)");
        assert_eq!(r.models[0].name, "Llama 8B");
        // Any other kind keeps to the OpenAI-style list.
        let other = core
            .inner()
            .ai_probe(
                AiProvider {
                    kind: "custom".into(),
                    ..p
                },
                None,
            )
            .unwrap();
        assert_eq!(other.models.len(), 1);
    }

    #[test]
    fn providers_are_validated_and_features_can_pick_their_own() {
        let (core, _home) = setup("http://127.0.0.1:9/v1", true);
        let i = core.inner();
        let bad = |kind: &str, url: &str| {
            i.ai_save_provider(
                AiProvider {
                    id: String::new(),
                    name: "X".into(),
                    kind: kind.into(),
                    base_url: url.into(),
                    model: String::new(),
                    tools: true,
                    local: false,
                    has_key: false,
                },
                None,
            )
        };
        assert!(bad("gpt", "http://localhost/v1").is_err());
        assert!(bad("custom", "ftp://localhost/v1").is_err());
        assert!(bad("custom", "localhost").is_err());
        let state = bad("lmstudio", "http://localhost:1234/v1/").unwrap();
        assert_eq!(state.settings.providers.len(), 2);
        assert_eq!(
            state.settings.providers[1].base_url,
            "http://localhost:1234/v1"
        );
        assert!(state.settings.providers[1].local);
        assert!(i
            .ai_save_settings(
                true,
                BTreeMap::from([("explain".to_string(), "missing".to_string())])
            )
            .is_err());
        assert!(i
            .ai_save_settings(
                true,
                BTreeMap::from([("dance".to_string(), "mock".to_string())])
            )
            .is_err());
        let state = i.ai_save_settings(
            true,
            BTreeMap::from([("logs".to_string(), "x".to_string())]),
        );
        assert!(state.is_ok(), "the second provider was named X");
        assert_eq!(pick_provider(&i.ai_settings(), "logs").unwrap().id, "x");
        assert_eq!(
            pick_provider(&i.ai_settings(), "explain").unwrap().id,
            "mock",
            "the first provider is the default"
        );
        let after = i.ai_remove_provider("x").unwrap();
        assert!(after.settings.features.is_empty());
    }

    #[test]
    fn a_key_is_never_stored_for_or_sent_over_plain_http_to_another_machine() {
        let (core, _home) = setup("http://127.0.0.1:9/v1", true);
        let e = core
            .inner()
            .ai_save_provider(
                AiProvider {
                    id: String::new(),
                    name: "LAN".into(),
                    kind: "custom".into(),
                    base_url: "http://192.168.1.5:8000/v1".into(),
                    model: "m".into(),
                    tools: true,
                    local: false,
                    has_key: false,
                },
                Some("sk-abcdefghijklmnopqrstuv".into()),
            )
            .unwrap_err()
            .to_string();
        assert!(e.contains("https"), "{e}");
        let p = AiProvider {
            id: "x".into(),
            name: "x".into(),
            kind: "custom".into(),
            base_url: "http://192.168.1.5:8000/v1".into(),
            model: "m".into(),
            tools: true,
            local: false,
            has_key: false,
        };
        assert!(authorize(
            client(false, None).get("http://192.168.1.5/"),
            &p,
            Some("k")
        )
        .is_err());
        assert!(authorize(client(false, None).get("http://192.168.1.5/"), &p, None).is_ok());
    }

    #[test]
    fn a_manifest_draft_must_parse_and_a_commit_message_needs_staged_changes() {
        let mock = Mock::start(|_, _, _| {
            sse(&[
                "Here you go.\n```yaml\nname: shop\nruntimes:\n  php: \"8.3\"\n```\n",
                "```yaml\n{{ broken\n```",
            ])
        });
        let (core, home) = setup(&mock.url(), true);
        let project = home.paths.root().join("shop");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("composer.json"), "{}").unwrap();
        let p = core
            .inner()
            .projects
            .lock()
            .unwrap()
            .register(project.to_str().unwrap())
            .unwrap();
        let req = AiRequest {
            feature: "config".into(),
            kind: "manifest".into(),
            project_id: Some(p.id.clone()),
            ..Default::default()
        };
        let prompt = core.inner().ai_prompt(&req).unwrap();
        assert!(
            prompt
                .attachments
                .iter()
                .any(|a| a.starts_with("composer.json"))
                && prompt
                    .attachments
                    .iter()
                    .any(|a| a.starts_with("Detected example")),
            "{:?}",
            prompt.attachments
        );
        let done = wait(&core, &core.inner().ai_start(req, false).unwrap().id);
        assert_eq!(done.state, "done", "{:?}", done.error);
        let a = done.answer.unwrap();
        assert!(
            a.manifest.is_none(),
            "the last yaml block is the one used, and it doesn't parse"
        );
        assert!(
            a.rejected.iter().any(|r| r.contains("manifest")),
            "{:?}",
            a.rejected
        );

        let commit = AiRequest {
            feature: "commit".into(),
            project_id: Some(p.id),
            ..Default::default()
        };
        assert!(
            core.inner().ai_prompt(&commit).is_err(),
            "no repository or nothing staged"
        );
    }

    #[test]
    fn every_feature_needs_what_it_asks_about() {
        let (core, _home) = setup("http://127.0.0.1:9/v1", true);
        let i = core.inner();
        for (feature, kind) in [
            ("explain", ""),
            ("logs", ""),
            ("palette", ""),
            ("traffic", "explain"),
            ("config", "manifest"),
            ("config", "web"),
            ("commit", ""),
            ("dance", ""),
        ] {
            let r = AiRequest {
                feature: feature.into(),
                kind: kind.into(),
                ..Default::default()
            };
            assert!(
                i.ai_prompt(&r).is_err(),
                "{feature}/{kind} without input must be refused"
            );
        }
        let logs = AiRequest {
            feature: "logs".into(),
            question: Some("why 502?".into()),
            ..Default::default()
        };
        let p = i.ai_prompt(&logs).unwrap();
        assert!(
            p.messages[0].content.contains("Quote the lines")
                && !p.messages[0].content.contains("ols-plan"),
            "log answers don't propose plans"
        );
    }
}
