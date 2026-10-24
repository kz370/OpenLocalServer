use std::path::{Path, PathBuf};
use crate::app::*;
use crate::app::system::*;
use crate::dbtools;
use crate::domain::Domain;
use crate::error::CoreError;
use crate::health::{HealthReport, HealthTarget};

impl Inner {
    pub fn log_sources(&self) -> Vec<LogSource> {
        let mut out = vec![LogSource {
            id: "app".into(),
            name: "OLS".into(),
            kind: "app".into(),
        }];
        // Every web server writes its own logs, not just the configured default,
        // so all three are listed by id. The bare `web:error` / `web:access` ids
        // still read the default server, but the picker shows the per-server ones
        // so opening logs for a service row never shows another server's file.
        for id in crate::web::SERVER_IDS {
            let Some(s) = crate::web::server_by_id(id) else {
                continue;
            };
            out.push(LogSource {
                id: format!("web:{id}:error"),
                name: format!("{} error log", s.name()),
                kind: "web".into(),
            });
            out.push(LogSource {
                id: format!("web:{id}:access"),
                name: format!("{} access log", s.name()),
                kind: "web".into(),
            });
        }
        // A service's own stdout/stderr, so "View logs" on a service row lands on
        // that service instead of the app log. Only while it runs: the output
        // buffer dies with the process.
        for s in self.services.list().into_iter().filter(|s| s.kind != "web") {
            if let Some(source) = s.log_source {
                out.push(LogSource {
                    id: source,
                    name: format!("{} log", s.name),
                    kind: "service".into(),
                });
            }
        }
        for p in self.supervisor.snapshot() {
            out.push(LogSource {
                id: format!("process:{}", p.id.0),
                name: p.name.clone(),
                kind: "process".into(),
            });
        }
        for r in self.runs.list() {
            out.push(LogSource {
                id: format!("run:{}", r.id),
                name: format!("Quick App: {}", r.app_name),
                kind: "run".into(),
            });
        }
        out
    }

    pub fn read_log(&self, source: &str, max_lines: usize) -> Result<Vec<String>, CoreError> {
        let tail = |mut lines: Vec<String>| {
            if lines.len() > max_lines {
                lines.drain(..lines.len() - max_lines);
            }
            lines
        };
        let file_lines = |p: &Path| {
            std::fs::read_to_string(p)
                .map(|t| t.lines().map(str::to_string).collect::<Vec<_>>())
                .unwrap_or_default()
        };
        match source {
            "app" => {
                let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
                if let Ok(entries) = std::fs::read_dir(self.paths.logs_dir()) {
                    for e in entries.flatten() {
                        if let Ok(m) = e.metadata().and_then(|m| m.modified()) {
                            if newest.as_ref().is_none_or(|(t, _)| m > *t) {
                                newest = Some((m, e.path()));
                            }
                        }
                    }
                }
                Ok(tail(
                    newest.map(|(_, p)| file_lines(&p)).unwrap_or_default(),
                ))
            }
            "web:error" | "web:access" => {
                let cfg = self.web_config();
                let server = crate::web::server_by_id(cfg.server())
                    .ok_or_else(|| svc("unknown web server"))?;
                let logs = self.paths.web_dir().join(server.id()).join("logs");
                let file = if source == "web:error" {
                    "error.log"
                } else {
                    "access.log"
                };
                Ok(tail(file_lines(&logs.join(file))))
            }
            // Per-server form (`web:nginx:error`) so a service row for a server
            // that isn't the configured default still reads its own file.
            s if s.starts_with("web:") => {
                let (id, file) = match s[4..].rsplit_once(':') {
                    Some((id, "error")) => (id, "error.log"),
                    Some((id, "access")) => (id, "access.log"),
                    _ => return Err(svc(format!("unknown log source {s}"))),
                };
                crate::web::server_by_id(id)
                    .ok_or_else(|| svc(format!("unknown web server {id}")))?;
                Ok(tail(file_lines(
                    &self.paths.web_dir().join(id).join("logs").join(file),
                )))
            }
            // A service's own output. Empty while it is stopped: the buffer went
            // with the process, and an error here would look like a broken page. A
            // Start that never spawned is the exception — there is no buffer to read,
            // so the recorded reason is the whole story.
            s if s.starts_with("service:") => {
                let id = &s[8..];
                let lines = self
                    .services
                    .process_id(id)
                    .map(|process_id| tail(self.supervisor.recent_output(process_id)))
                    .unwrap_or_default();
                if !lines.is_empty() {
                    return Ok(lines);
                }
                Ok(self
                    .services
                    .last_start_failure(id)
                    .map(|reason| vec![format!("last start attempt failed: {reason}")])
                    .unwrap_or_default())
            }
            s if s.starts_with("process:") => {
                let id: u64 = s[8..].parse().map_err(|_| svc("bad process id"))?;
                Ok(tail(self.supervisor.recent_output(ProcessId(id))))
            }
            s if s.starts_with("run:") => Ok(tail(
                self.runs.get(&s[4..]).map(|r| r.log).unwrap_or_default(),
            )),
            other => Err(svc(format!("unknown log source {other}"))),
        }
    }

    /// Empties a log. Files are truncated in place rather than deleted: the server still
    /// holds them open and keeps appending.
    pub fn clear_log(&self, source: &str) -> Result<(), CoreError> {
        let truncate = |p: &Path| -> Result<(), CoreError> {
            if !p.exists() {
                return Ok(());
            }
            std::fs::OpenOptions::new()
                .write(true)
                .open(p)
                .and_then(|f| f.set_len(0))
                .map_err(|e| svc(format!("could not clear {}: {e}", p.display())))
        };
        match source {
            "app" => {
                if let Ok(entries) = std::fs::read_dir(self.paths.logs_dir()) {
                    for e in entries.flatten().filter(|e| e.path().is_file()) {
                        truncate(&e.path())?;
                    }
                }
                Ok(())
            }
            "web:error" | "web:access" => {
                let cfg = self.web_config();
                let server = crate::web::server_by_id(cfg.server())
                    .ok_or_else(|| svc("unknown web server"))?;
                let file = if source == "web:error" {
                    "error.log"
                } else {
                    "access.log"
                };
                truncate(
                    &self
                        .paths
                        .web_dir()
                        .join(server.id())
                        .join("logs")
                        .join(file),
                )
            }
            s if s.starts_with("web:") => {
                let (id, file) = match s[4..].rsplit_once(':') {
                    Some((id, "error")) => (id, "error.log"),
                    Some((id, "access")) => (id, "access.log"),
                    _ => return Err(svc(format!("unknown log source {s}"))),
                };
                crate::web::server_by_id(id)
                    .ok_or_else(|| svc(format!("unknown web server {id}")))?;
                truncate(&self.paths.web_dir().join(id).join("logs").join(file))
            }
            s if s.starts_with("service:") => {
                // Nothing to clear while the service is stopped: the buffer that
                // held its output went away with the process.
                if let Some(process_id) = self.services.process_id(&s[8..]) {
                    self.supervisor.clear_output(process_id);
                }
                Ok(())
            }
            s if s.starts_with("process:") => {
                let id: u64 = s[8..].parse().map_err(|_| svc("bad process id"))?;
                self.supervisor.clear_output(ProcessId(id));
                Ok(())
            }
            s if s.starts_with("run:") => {
                self.runs.clear_log(&s[4..]);
                Ok(())
            }
            other => Err(svc(format!("unknown log source {other}"))),
        }
    }

}
