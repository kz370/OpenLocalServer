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

use super::types::*;
use super::policy::*;
use super::client::*;
use super::providers::*;
use super::tools::*;
use super::*;

pub(crate) struct Context {
    pub(crate) instructions: String,
    pub(crate) question: String,
    pub(crate) attachments: Vec<(String, String)>,
}

pub(crate) fn need<'a>(value: &'a Option<String>, what: &str) -> Result<&'a str, CoreError> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| fail(format!("{what} is needed for this request")))
}



impl Inner {

    pub(crate) fn ai_context(&self, req: &AiRequest) -> Result<Context, CoreError> {
        let mut attachments: Vec<(String, String)> = Vec::new();
        let (instructions, question) = match req.feature.as_str() {
            "assistant" => {
                let question =
                    need(&req.question, "what you want to know about OpenLocalServer")?.to_string();
                attachments.push((
                    "OpenLocalServer User Manual (v2.3.0 Knowledge Base)".into(),
                    APP_GUIDE_MD.to_string(),
                ));

                let mut env_facts = Vec::new();
                let web = self
                    .web
                    .status(&self.web_config(), &self.domains.lock().unwrap().list());
                env_facts.push(format!(
                    "Active Web Server: {} (running: {}, http: {}, https: {})",
                    web.default_server, web.running, web.http_port, web.https_port
                ));

                let domains = self.domain_summaries();
                env_facts.push(format!("Registered Sites/Domains count: {}", domains.len()));
                let site_names: Vec<String> =
                    domains.iter().take(8).map(|d| d.hostname.clone()).collect();
                if !site_names.is_empty() {
                    env_facts.push(format!("Sample Domains: {}", site_names.join(", ")));
                }

                let svcs = self.services.list();
                let running_svcs: Vec<String> = svcs
                    .iter()
                    .filter(|s| s.running)
                    .map(|s| s.name.clone())
                    .collect();
                if !running_svcs.is_empty() {
                    env_facts.push(format!("Running Services: {}", running_svcs.join(", ")));
                }

                let runtimes = self.runtimes.catalog();
                let installed: Vec<String> = runtimes
                    .iter()
                    .filter(|r| r.installed)
                    .map(|r| format!("{} {}", r.name, r.version))
                    .collect();
                if !installed.is_empty() {
                    env_facts.push(format!("Installed Runtimes: {}", installed.join(", ")));
                }

                attachments.push(("User Live Environment Status".into(), env_facts.join("\n")));

                (
                    "You are the OpenLocalServer (OLS) Assistant, the single conversational front door for the whole application. You answer questions about OLS and propose concrete actions as an ols-plan that the user reviews before anything runs.\n\nCRITICAL INSTRUCTIONS:\n1. Answer correctly, simply, and directly. Avoid long rambling or walls of jargon.\n2. Use the live tools (list_sites, list_services, list_tunnels, list_databases, list_runtimes, list_profiles, get_site_details, list_tasks, get_findings, read_log, read_web_config, read_manifest, read_env_names, list_quick_apps, list_projects) to check current state before proposing changes.\n3. If the user asks how to do something, guide them to the right page or setting, or propose the commands to do it.\n4. You may propose any app operation: sites, bundles, tunnels, services, databases, runtimes, SSL certificates, DNS, profiles, snapshots, quick apps, environment setup.\n5. Destructive steps (delete, remove, restore, import, env edits) need a separate user confirmation — mention this when proposing them.\n6. Refer to the attached OLS User Manual for exact facts. Do not invent non-existent features.\n\nEnd answers that propose changes with exactly one ols-plan block containing JSON commands from the allowlist.".to_string(),
                    question,
                )
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
        let system = format!("{SYSTEM}\n\n{}\n\n{PLAN_DOC}", ctx.instructions);
        // Previous conversation turns come first, then the current question as the final user turn.
        let mut prompt_messages: Vec<PromptMessage> = Vec::with_capacity(req.messages.len() + 2);
        prompt_messages.push(PromptMessage {
            role: "system".into(),
            content: system,
        });
        prompt_messages.extend(req.messages.clone());
        prompt_messages.push(PromptMessage {
            role: "user".into(),
            content: user,
        });
        Ok(AiPrompt {
            provider_id: provider.id.clone(),
            provider_name: provider.name.clone(),
            model: provider.model.clone(),
            local: provider.local,
            host: url_host(&provider.base_url),
            messages: prompt_messages,
            attachments: labels,
            tools: if provider.tools { tool_names() } else { vec![] },
        })
    }

    pub fn app_guide(&self) -> String {
        APP_GUIDE_MD.to_string()
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
    pub(crate) fn ai_check_plan(&self, text: &str) -> (Vec<AiAction>, Vec<String>) {
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

    pub(crate) fn ai_price(&self, provider: &AiProvider, model: &str) -> Option<(f64, f64)> {
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
