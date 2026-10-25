use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::app::Inner;
use crate::error::CoreError;

use super::policy::*;
use super::types::*;

impl Inner {
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
pub(crate) fn off_runtime<T: Send>(f: impl FnOnce() -> T + Send) -> Result<T, String> {
    std::thread::scope(|s| s.spawn(f).join()).map_err(|_| "it stopped unexpectedly".to_string())
}

/// One small current-thread runtime per call: the rest of the core is synchronous.
pub(crate) fn block_on<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to start the AI runtime")
        .block_on(f)
}

pub(crate) fn client(local: bool, total: Option<Duration>) -> reqwest::Client {
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

pub(crate) fn authorize(
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

pub(crate) fn api_error(status: u16, body: &str) -> String {
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

pub(crate) fn per_million(v: &Value) -> Option<f64> {
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

pub(crate) async fn fetch_models(
    p: &AiProvider,
    key: Option<&str>,
) -> Result<Vec<AiModel>, String> {
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
