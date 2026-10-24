use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::app::*;
use crate::app::quickapp::*;
use crate::domain::{Domain, Ownership};
use crate::error::CoreError;
use crate::quickapp::commands::QuickCommand;
use crate::quickapp::plan::RunPlan;
use crate::quickapp::run::{ActionOutcome, ResolvedProgram, RunCtx};
use crate::quickapp::QuickHost;

impl Inner {

    /// Values available to a Quick Command's `{{placeholders}}`.
    fn command_values(&self, project_id: Option<&str>) -> BTreeMap<String, String> {
        let mut values = BTreeMap::new();
        if let Some(p) = project_id.and_then(|id| self.projects.lock().unwrap().get(id)) {
            values.insert("project_path".into(), p.path.clone());
            values.insert("project_name".into(), p.name.clone());
            if let Some(d) = self
                .domains
                .lock()
                .unwrap()
                .list()
                .into_iter()
                .find(|d| d.project_id.as_deref() == Some(p.id.as_str()))
            {
                values.insert("domain".into(), d.hostname);
            }
        }
        values
    }

    pub fn run_command_line(
        &self,
        line: &str,
        cwd: Option<&str>,
        project_id: Option<&str>,
        name: Option<&str>,
    ) -> Result<ProcessId, CoreError> {
        let tokens = plan::split_command_line(line);
        let (program, args) = tokens
            .split_first()
            .ok_or_else(|| CoreError::QuickAppError("that command line is empty".into()))?;
        self.spawn_project_process(program, args, cwd, project_id, name.unwrap_or(line), line)
    }

    pub(crate) fn spawn_project_process(
        &self,
        program: &str,
        args: &[String],
        cwd: Option<&str>,
        project_id: Option<&str>,
        name: &str,
        history_line: &str,
    ) -> Result<ProcessId, CoreError> {
        let (executable, full_args, env) = self.project_program(program, args, project_id)?;
        let cwd_owned = cwd.map(str::to_string).or_else(|| {
            project_id
                .and_then(|id| self.projects.lock().unwrap().get(id))
                .map(|p| p.path)
        });
        let id = self.supervisor.start(ProcessSpec {
            name: name.to_string(),
            executable: executable.display().to_string(),
            args: full_args,
            cwd: cwd_owned.clone(),
            env,
            restart: None,
        });
        let _ = self
            .history
            .lock()
            .unwrap()
            .record(history_line, cwd_owned.as_deref(), project_id);
        Ok(id)
    }

    /// `program args` resolved the way a project's commands run: its own PHP / Node /
    /// Python, with their folders first on PATH. Returns the executable, all arguments and
    /// the extra environment.
    pub(crate) fn project_program(
        &self,
        program: &str,
        args: &[String],
        project_id: Option<&str>,
    ) -> Result<ProjectProgram, CoreError> {
        let mut values = BTreeMap::new();
        // Pin the resolved versions so `php`/`node` follow the project (§18–19).
        if let Some(detail) = project_id.and_then(|id| self.project_detail(id)) {
            for r in &detail.resolved {
                if let Some(v) = &r.installed_version {
                    values.insert(format!("{}_version", r.id), v.clone());
                }
            }
        }
        let resolved = self
            .quick_resolve_program(program, &values, project_id)
            .map_err(CoreError::QuickAppError)?;
        let mut full_args = resolved.pre_args.clone();
        full_args.extend(args.iter().cloned());
        let mut env = resolved.env.clone();
        // §129: Node's heap limit for every project command (Node ignores it otherwise).
        if let Some(mb) = self.resource_limits().node_max_old_space_mb {
            if !env.iter().any(|(k, _)| k == "NODE_OPTIONS") {
                env.push(("NODE_OPTIONS".into(), format!("--max-old-space-size={mb}")));
            }
        }
        if !resolved.path_dirs.is_empty() {
            let mut dirs: Vec<String> = resolved
                .path_dirs
                .iter()
                .map(|d| d.display().to_string())
                .collect();
            dirs.push(std::env::var("PATH").unwrap_or_default());
            env.push(("PATH".into(), dirs.join(";")));
        }
        Ok((resolved.executable, full_args, env))
    }

    pub fn run_quick_command(
        &self,
        id: &str,
        project_id: Option<&str>,
    ) -> Result<Option<ProcessId>, CoreError> {
        let cmd: QuickCommand = self
            .quick_commands
            .get(id)
            .ok_or_else(|| CoreError::QuickAppError(format!("no Quick Command \"{id}\"")))?;
        let values = self.command_values(project_id);
        let render = |t: &str| plan::render(t, &values, &[]).map_err(CoreError::QuickAppError);

        if let Some(action) = &cmd.action {
            match action.as_str() {
                "open_url" => {
                    let url = cmd
                        .with
                        .get("url")
                        .map(|s| s.as_string())
                        .unwrap_or_default();
                    self.open_url(&render(&url)?)?;
                }
                "open_site" => {
                    let pid = project_id
                        .ok_or_else(|| CoreError::QuickAppError("pick a project first".into()))?;
                    let cfg = self.web_config();
                    let domain = self
                        .domains
                        .lock()
                        .unwrap()
                        .list()
                        .into_iter()
                        .find(|d| d.project_id.as_deref() == Some(pid))
                        .ok_or_else(|| {
                            CoreError::QuickAppError("this project has no domain yet".into())
                        })?;
                    self.open_url(&self.site_url(&domain, &cfg))?;
                }
                "open_web_config" => {
                    let cfg = self.web_config();
                    let dir = self.paths.web_dir().join(cfg.server());
                    std::fs::create_dir_all(&dir)?;
                    self.open_path(&dir.display().to_string())?;
                }
                "restart_project" => {
                    let pid = project_id
                        .ok_or_else(|| CoreError::QuickAppError("pick a project first".into()))?;
                    let hosts: Vec<String> = self
                        .domains
                        .lock()
                        .unwrap()
                        .list()
                        .into_iter()
                        .filter(|d| d.project_id.as_deref() == Some(pid))
                        .map(|d| d.hostname)
                        .collect();
                    for h in hosts {
                        self.web.restart_app(&h);
                    }
                    self.apply_web(&[])?;
                }
                other => {
                    return Err(CoreError::QuickAppError(format!(
                        "unknown action \"{other}\""
                    )))
                }
            }
            return Ok(None);
        }

        let spec = cmd.command.as_ref().expect("validated");
        let cwd = cmd.working_directory.as_deref().map(render).transpose()?;
        let args = spec
            .arguments
            .iter()
            .map(|a| render(a))
            .collect::<Result<Vec<_>, _>>()?;
        let mut line_parts = vec![spec.executable.clone()];
        line_parts.extend(args.iter().map(|a| {
            if a.contains(' ') {
                format!("\"{a}\"")
            } else {
                a.clone()
            }
        }));
        let pid = self.spawn_project_process(
            &spec.executable,
            &args,
            cwd.as_deref(),
            project_id,
            &format!("Quick Command: {}", cmd.name),
            &line_parts.join(" "),
        )?;
        Ok(Some(pid))
    }

    pub fn plan_quick_app(
        &self,
        id: &str,
        values: &BTreeMap<String, String>,
    ) -> Result<QuickPlanResult, CoreError> {
        let detail = self.catalog.lock().unwrap().get(id)?;
        let ctx = self.plan_ctx();
        let (resolved, errors) = plan::resolve_lenient(&detail.app, values, &ctx);
        // Secrets never leave the core — the wizard only ever sees them masked.
        let masked = |vals: &BTreeMap<String, String>| -> BTreeMap<String, String> {
            vals.iter()
                .map(|(k, v)| {
                    let secret = detail
                        .app
                        .variables
                        .iter()
                        .any(|x| &x.name == k && x.is_secret());
                    (
                        k.clone(),
                        if secret && !v.is_empty() {
                            "••••••••".to_string()
                        } else {
                            v.clone()
                        },
                    )
                })
                .collect()
        };
        let display = masked(&resolved);
        if !errors.is_empty() {
            return Ok(QuickPlanResult {
                ok: false,
                errors,
                plan: None,
                requirements: vec![],
                trusted: detail.view.trusted,
                source: detail.view.source_label(),
                values: display,
            });
        }
        let plan =
            plan::build_plan(&detail.app, resolved, &ctx).map_err(CoreError::QuickAppError)?;
        let requirements = self.requirement_views(&plan);
        Ok(QuickPlanResult {
            ok: true,
            errors: vec![],
            plan: Some(plan),
            requirements,
            trusted: detail.view.trusted,
            source: detail.view.source_label(),
            values: display,
        })
    }
}
