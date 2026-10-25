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

use super::policy::*;
use super::types::*;
use super::*;

use super::chat::*;
use super::providers::*;
use super::tools::*;

pub(crate) struct ToolCall {
    id: String,
    name: String,
    args: String,
}

#[derive(Default)]
pub(crate) struct Completion {
    content: String,
    calls: Vec<ToolCall>,
    usage: Option<(u64, u64)>,
}

pub(crate) enum ChatError {
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

pub(crate) async fn complete(
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

pub(crate) async fn run_chat(
    inner: Arc<Inner>,
    provider: AiProvider,
    key: Option<String>,
    prompt: AiPrompt,
    _req: AiRequest,
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
    let (actions, rejected) = off_runtime(|| inner.ai_check_plan(&final_text)).unwrap_or_default();
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
    let mut text = without_fences(&final_text, &plan_blocks);
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
