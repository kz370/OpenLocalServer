use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use super::policy::*;
use crate::command::CoreCommand;

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
    /// Previous conversation turns so the assistant remembers context across messages.
    #[serde(default)]
    pub messages: Vec<PromptMessage>,
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

pub(crate) struct Job {
    pub(crate) view: AiJobView,
    pub(crate) cancel: Arc<AtomicBool>,
}

#[derive(Default)]
pub struct AiJobs {
    pub(crate) jobs: Mutex<HashMap<String, Arc<Mutex<Job>>>>,
    pub(crate) counter: AtomicU64,
    /// Model prices per base URL, so a cost can be shown without listing models for every answer.
    pub(crate) prices: Mutex<HashMap<String, (Instant, Vec<AiModel>)>>,
}

// ------------------------------------------------------------------------------ what a plan may contain
