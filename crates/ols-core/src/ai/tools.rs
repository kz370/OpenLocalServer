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
use super::*;

// ------------------------------------------------------------------------------ tools (read-only)

pub(crate) fn tool_defs() -> Vec<Value> {
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
        f("list_tunnels", "The tunnels (id, name, provider, target, public hostname, active state).", json!({}), &[]),
        f("list_databases", "Databases across MariaDB, PostgreSQL and SQLite (engine, name, size).", json!({}), &[]),
        f("list_runtimes", "Installed runtimes and available catalog versions (id, name, version, installed).", json!({}), &[]),
        f("list_profiles", "Available configuration profiles (id, name).", json!({}), &[]),
        f("get_site_details", "Complete configuration of one domain: root, runtime, web server, SSL, tunnel, path prefix.", json!({"hostname":{"type":"string"}}), &["hostname"]),
        f("list_tasks", "Background tasks and their statuses (id, kind, state, progress).", json!({}), &[]),
    ]
}

pub(crate) fn tool_names() -> Vec<String> {
    tool_defs()
        .iter()
        .filter_map(|t| t["function"]["name"].as_str().map(str::to_string))
        .collect()
}

impl Inner {
    pub(crate) fn ai_tool(&self, name: &str, args: &Value) -> Result<String, String> {
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
            "list_tunnels" => self
                .list_tunnels()
                .into_iter()
                .map(|t| {
                    format!(
                        "{} · {} · {} · target={} · public={} · {}",
                        t.config.id,
                        t.config.name,
                        t.config.provider,
                        t.config.target,
                        t.public_url.as_deref().unwrap_or("-"),
                        t.state
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
            "list_databases" => {
                let engine = s("engine").unwrap_or_else(|| "mariadb".into());
                match self.services.list_databases(&engine) {
                    Ok(dbs) => dbs.join("\n"),
                    Err(e) => return Err(e),
                }
            }
            "list_runtimes" => self
                .runtimes
                .catalog()
                .into_iter()
                .map(|r| {
                    format!(
                        "{} · {} · {}",
                        r.id,
                        r.name,
                        if r.installed { "installed" } else { "catalog" }
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
            "list_profiles" => self
                .profiles
                .list()
                .into_iter()
                .map(|p| format!("{}: {}", p.id, p.name))
                .collect::<Vec<_>>()
                .join("\n"),
            "get_site_details" => {
                let hostname = s("hostname").ok_or("get_site_details needs a hostname")?;
                let d = self
                    .domains
                    .lock()
                    .unwrap()
                    .get(&hostname)
                    .ok_or("unknown domain")?;
                let runtime = d
                    .app
                    .as_ref()
                    .and_then(|a| a.runtime.as_deref())
                    .unwrap_or("-");
                format!(
                    "hostname={} root={} runtime={} server={} ssl={} tunnel={} path_prefix={}",
                    d.hostname,
                    d.root,
                    runtime,
                    d.server.as_deref().unwrap_or("-"),
                    d.ssl_provider,
                    d.tunnel_id.as_deref().unwrap_or("-"),
                    d.path_prefix.as_deref().unwrap_or("-"),
                )
            }
            "list_tasks" => self
                .tasks
                .list()
                .into_iter()
                .map(|t| format!("{}: {} · {} · {:?}", t.id, t.kind, t.title, t.state))
                .collect::<Vec<_>>()
                .join("\n"),
            other => return Err(format!("there is no tool called {other}")),
        };
        Ok(head(&clean(&text), TOOL_OUTPUT))
    }
}


impl Inner {

    pub(crate) fn excerpt_dir(&self) -> PathBuf {
        self.paths.data_dir().join("ai-excerpts")
    }

    /// Page through a saved excerpt file: `start_line` 1-based, `lines` capped.
    pub(crate) fn read_excerpt(&self, file: &str, start_line: usize, lines: usize) -> Result<String, String> {
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


}
