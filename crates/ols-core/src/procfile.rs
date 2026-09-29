//! Procfile import for projects that describe their local processes in Heroku's format.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{domain::AppSpec, error::CoreError, workers::Worker};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcfileEntry {
    pub name: String,
    pub command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcfilePreview {
    pub source: String,
    pub workers: Vec<Worker>,
    pub web: Option<AppSpec>,
    pub web_port: Option<u16>,
    pub warnings: Vec<String>,
}

pub fn parse(text: &str) -> Result<Vec<ProcfileEntry>, String> {
    let mut entries = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((name, command)) = line.split_once(':') else {
            return Err(format!("line {} needs a name followed by ':'", index + 1));
        };
        let name = name.trim();
        let command = command.trim();
        if name.is_empty()
            || !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(format!("line {} has an invalid process name", index + 1));
        }
        if command.is_empty() {
            return Err(format!("line {} has no command", index + 1));
        }
        entries.push(ProcfileEntry {
            name: name.to_string(),
            command: command.to_string(),
        });
    }
    Ok(entries)
}

pub fn project_file(root: &Path) -> Option<(String, String)> {
    ["Procfile.dev", "Procfile"].into_iter().find_map(|name| {
        let path = root.join(name);
        std::fs::read_to_string(&path)
            .ok()
            .map(|contents| (name.to_string(), contents))
    })
}

fn port() -> Result<u16, CoreError> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))
        .map_err(|e| CoreError::ServiceError(format!("could not find a free app port: {e}")))?;
    Ok(listener
        .local_addr()
        .map_err(|e| CoreError::ServiceError(e.to_string()))?
        .port())
}

impl crate::app::Inner {
    pub fn procfile_preview(&self, project_id: &str) -> Result<ProcfilePreview, CoreError> {
        let project = self
            .projects
            .lock()
            .unwrap()
            .get(project_id)
            .ok_or_else(|| CoreError::InvalidProjectPath(project_id.to_string()))?;
        let root = std::path::PathBuf::from(&project.path);
        let (source, contents) = project_file(&root).ok_or_else(|| {
            CoreError::ServiceError("this project has no Procfile or Procfile.dev".into())
        })?;
        let entries = parse(&contents).map_err(CoreError::ServiceError)?;
        let mut workers = Vec::new();
        let mut web = None;
        let web_port = if entries.iter().any(|e| e.name == "web") {
            Some(port()?)
        } else {
            None
        };
        for entry in entries {
            let command = web_port.map_or(entry.command.clone(), |p| {
                substitute_port(&entry.command, p)
            });
            if entry.name == "web" {
                let tokens = crate::quickapp::plan::split_command_line(&command);
                let (executable, args) = tokens
                    .split_first()
                    .ok_or_else(|| CoreError::ServiceError("the web command is empty".into()))?;
                web = Some(AppSpec {
                    executable: executable.clone(),
                    args: args.to_vec(),
                    cwd: project.path.clone(),
                    runtime: runtime_for(executable),
                });
            } else {
                workers.push(Worker {
                    id: crate::workers::worker_id(project_id, &entry.name),
                    project_id: project_id.to_string(),
                    name: entry.name,
                    command,
                    count: 1,
                    timeout_secs: None,
                    memory_mb: None,
                    max_retries: 5,
                    restart: true,
                    autostart: true,
                });
            }
        }
        let mut warnings = Vec::new();
        if web.is_some()
            && self
                .project_detail(project_id)
                .is_some_and(|d| d.detection.requirements.php.is_some())
        {
            warnings.push("The detected project is PHP; its web process is omitted. Import the other processes as workers.".into());
            web = None;
        }
        Ok(ProcfilePreview {
            source,
            workers,
            web,
            web_port: if warnings.is_empty() { web_port } else { None },
            warnings,
        })
    }

    pub fn import_procfile(&self, project_id: &str) -> Result<ProcfilePreview, CoreError> {
        let preview = self.procfile_preview(project_id)?;
        for worker in &preview.workers {
            self.save_worker(worker.clone())?;
        }
        if let (Some(app), Some(port)) = (&preview.web, preview.web_port) {
            let project = self
                .projects
                .lock()
                .unwrap()
                .get(project_id)
                .ok_or_else(|| CoreError::InvalidProjectPath(project_id.to_string()))?;
            let mut domain = self
                .domains
                .lock()
                .unwrap()
                .list()
                .into_iter()
                .find(|d| d.project_id.as_deref() == Some(project_id));
            if let Some(site) = domain.as_mut() {
                site.kind = crate::domain::SiteKind::Proxy {
                    upstream_port: port,
                    upstream_host: None,
                    upstream_https: false,
                };
                site.app = Some(app.clone());
                self.domains.lock().unwrap().update(site.clone())?;
            } else {
                let host = format!("{}.test", crate::domain::slugify(&project.name));
                let site = crate::domain::Domain {
                    hostname: host,
                    project_id: Some(project_id.to_string()),
                    root: project.path.clone(),
                    kind: crate::domain::SiteKind::Proxy {
                        upstream_port: port,
                        upstream_host: None,
                        upstream_https: false,
                    },
                    https: true,
                    redirect_https: false,
                    wildcard: false,
                    enabled: true,
                    ownership: Default::default(),
                    app: Some(app.clone()),
                    blocks: Default::default(),
                    generated_hashes: Default::default(),
                    public_domain: None,
                    tunnel_id: None,
                    server: None,
                    path_prefix: None,
                };
                self.domains.lock().unwrap().add(site)?;
            }
            self.apply_web(&[])?;
        }
        Ok(preview)
    }
}

fn substitute_port(command: &str, port: u16) -> String {
    command
        .replace("${PORT}", &port.to_string())
        .replace("$PORT", &port.to_string())
}

fn runtime_for(exe: &str) -> Option<String> {
    let e = exe
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(exe)
        .trim_end_matches(".exe")
        .to_ascii_lowercase();
    match e.as_str() {
        "node" | "npm" | "npx" | "pnpm" | "yarn" => Some("node".into()),
        "python" | "python3" | "uvicorn" | "gunicorn" => Some("python".into()),
        "php" => Some("php".into()),
        _ => None,
    }
}
