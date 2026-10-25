pub mod actions;
pub mod commands;
pub mod resolve;

use crate::app::Inner;
use crate::quickapp::run::{ActionOutcome, ResolvedProgram, RunCtx};
use crate::quickapp::QuickHost;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

impl crate::quickapp::EntryView {
    pub fn source_label(&self) -> String {
        match self.source {
            crate::quickapp::EntrySource::Builtin => "built-in".into(),
            crate::quickapp::EntrySource::Local => "your own".into(),
            crate::quickapp::EntrySource::Imported => {
                self.origin.clone().unwrap_or_else(|| "imported".into())
            }
        }
    }
}

/// `.cmd` / `.bat` shims (npm, npx) can't be launched directly — run them through cmd.exe.
/// Executable, full arguments and extra environment of a resolved project command.
pub(crate) type ProjectProgram = (PathBuf, Vec<String>, Vec<(String, String)>);

fn shim(file: PathBuf, path_dirs: Vec<PathBuf>) -> ResolvedProgram {
    let is_shim = file
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"));
    if is_shim {
        ResolvedProgram {
            executable: PathBuf::from("cmd.exe"),
            pre_args: vec!["/C".into(), file.display().to_string()],
            env: vec![],
            path_dirs,
        }
    } else {
        ResolvedProgram {
            executable: file,
            pre_args: vec![],
            env: vec![],
            path_dirs,
        }
    }
}

/// The Quick App runner's window onto the core.
pub struct Host {
    pub inner: Arc<Inner>,
    /// Set when the run belongs to a known project, so runtimes resolve the way the project does.
    pub project_id: Mutex<Option<String>>,
}

impl QuickHost for Host {
    fn ensure_runtime(
        &self,
        id: &str,
        version: Option<&str>,
        log: &mut dyn FnMut(&str),
    ) -> Result<String, String> {
        self.inner.quick_ensure_runtime(id, version, log)
    }
    fn resolve_program(
        &self,
        program: &str,
        values: &BTreeMap<String, String>,
    ) -> Result<ResolvedProgram, String> {
        let pid = self.project_id.lock().unwrap().clone();
        self.inner
            .quick_resolve_program(program, values, pid.as_deref())
    }
    fn action(
        &self,
        action: &str,
        with: &BTreeMap<String, String>,
        ctx: &RunCtx,
        log: &mut dyn FnMut(&str),
    ) -> Result<ActionOutcome, String> {
        let out = self.inner.quick_action(action, with, ctx, log)?;
        if let Some(pid) = &out.project_id {
            *self.project_id.lock().unwrap() = Some(pid.clone());
        }
        Ok(out)
    }
    fn run_elevated(&self, executable: &Path, args: &[String]) -> Result<(), String> {
        crate::elevate::run_elevated_command(executable, args)
    }
}
