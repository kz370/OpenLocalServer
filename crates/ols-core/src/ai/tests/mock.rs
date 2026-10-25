use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::super::policy::*;
use super::super::types::*;
use super::super::*;
use crate::command::Core;

/// A stand-in OpenAI-compatible server on a loopback port. `respond(path, body, request_number)` returns the
/// status, content type and body; every request is recorded. No test talks to a real provider.
pub(crate) struct Mock {
    port: u16,
    seen: Arc<Mutex<Vec<(String, String)>>>,
}

impl Mock {
    pub(crate) fn start(
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
                        if data.len() >= end + 4 + len {
                            let path = head
                                .lines()
                                .next()
                                .and_then(|l| l.split_whitespace().nth(1))
                                .unwrap_or("/")
                                .to_string();
                            let body =
                                String::from_utf8_lossy(&data[end + 4..end + 4 + len]).to_string();
                            break (path, body);
                        }
                    }
                    if n == 0 {
                        break (String::new(), String::new());
                    }
                };
                let n = {
                    let mut lock = log.lock().unwrap();
                    lock.push((path.clone(), body.clone()));
                    lock.len()
                };
                let (status, content_type, reply) = respond(&path, &body, n);
                let response = format!(
                    "HTTP/1.1 {status} OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                    reply.len()
                );
                let _ = s.write_all(response.as_bytes());
            }
        });
        Mock { port, seen }
    }

    pub(crate) fn url(&self) -> String {
        format!("http://127.0.0.1:{}/v1", self.port)
    }

    pub(crate) fn bodies(&self) -> Vec<String> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| p.ends_with("/chat/completions"))
            .map(|(_, b)| b.clone())
            .collect()
    }
}

pub(crate) fn json_reply(v: Value) -> (u16, String, String) {
    (200, "application/json".into(), v.to_string())
}

pub(crate) fn sse(parts: &[&str]) -> (u16, String, String) {
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

pub(crate) fn setup(mock_url: &str, enabled: bool) -> (Core, crate::test_support::IsolatedHome) {
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

pub(crate) fn explain(text: &str) -> AiRequest {
    AiRequest {
        feature: "assistant".into(),
        title: Some("Web server won't start".into()),
        question: Some(text.into()),
        ..Default::default()
    }
}

pub(crate) fn wait(core: &Core, id: &str) -> AiJobView {
    let started = Instant::now();
    loop {
        let v = core.inner().ai_job(id).unwrap();
        if v.state != "running" || started.elapsed() > Duration::from_secs(20) {
            return v;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
}
