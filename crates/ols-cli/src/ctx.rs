use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ols_core::command::{CoreCommand, CoreResponse};
use ols_core::control::{self, ClientError};
use ols_core::{AppPaths, Diagnostic};

pub struct Ctx {
    pub paths: AppPaths,
    pub json: bool,
}

pub type R<T> = Result<T, String>;

pub fn diag(d: Diagnostic) -> String {
    let mut s = d.problem;
    if !d.cause.is_empty() {
        s.push_str(&format!("\n  {}", d.cause));
    }
    if let Some(f) = d.fix {
        s.push_str(&format!("\n  → {f}"));
    }
    s
}

#[cfg(windows)]
pub fn spawn_daemon() -> R<()> {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    std::process::Command::new(exe)
        .arg("daemon")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
        .spawn()
        .map_err(|e| format!("could not start the background service: {e}"))?;
    Ok(())
}

#[cfg(not(windows))]
pub fn spawn_daemon() -> R<()> {
    Err("the background service is Windows-only for now".into())
}

impl Ctx {
    /// Sends a command, starting the background daemon first if nothing is running.
    pub fn call(&self, cmd: CoreCommand) -> R<CoreResponse> {
        match control::send(&self.paths, cmd.clone()) {
            Ok(r) => r.map_err(diag),
            Err(ClientError::NotRunning) => {
                eprintln!("Starting OLS in the background…");
                spawn_daemon()?;
                let started = Instant::now();
                while control::is_running(&self.paths).is_none() {
                    if started.elapsed() > Duration::from_secs(30) {
                        return Err(
                            "the background service did not start (see the log in the data folder)"
                                .into(),
                        );
                    }
                    std::thread::sleep(Duration::from_millis(200));
                }
                control::send(&self.paths, cmd)
                    .map_err(|e| e.to_string())?
                    .map_err(diag)
            }
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn print_json(&self, r: &CoreResponse) -> bool {
        if self.json {
            println!("{}", serde_json::to_string_pretty(r).unwrap_or_default());
        }
        self.json
    }

    pub fn project_id(&self, name: &str) -> R<String> {
        let CoreResponse::Projects { projects } = self.call(CoreCommand::ListProjects)? else {
            return Err("unexpected reply".into());
        };
        projects
            .iter()
            .find(|p| p.id == name || p.name.eq_ignore_ascii_case(name))
            .map(|p| p.id.clone())
            .ok_or_else(|| format!("no project named \"{name}\" (see `olsc project list`)"))
    }

    /// The project for a folder, registering it when it isn't one yet.
    pub fn project_for_path(&self, path: &Path) -> R<(String, String)> {
        let path = &explorer_path(path);
        let full = std::fs::canonicalize(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let full = full.display().to_string();
        // canonicalize answers with the \\?\ spelling, which the filesystem accepts but
        // a person should never have to read: it is what the project row, the site
        // folder and the error messages all show. A share comes back as
        // `\\?\UNC\server\share`, and that one cannot simply lose its prefix -- what is
        // left, `UNC\server\share`, is not a path -- so it is rewritten. Both forms are
        // handled here rather than in the core because this string is also what the
        // stored projects are compared against.
        let full = match full.strip_prefix(r"\\?\UNC\") {
            Some(unc) => format!(r"\\{unc}"),
            None => full.strip_prefix(r"\\?\").unwrap_or(&full).to_string(),
        };
        let CoreResponse::Projects { projects } = self.call(CoreCommand::ListProjects)? else {
            return Err("unexpected reply".into());
        };
        if let Some(p) = projects.iter().find(|p| p.path.eq_ignore_ascii_case(&full)) {
            return Ok((p.id.clone(), p.name.clone()));
        }
        match self.call(CoreCommand::RegisterProject { path: full })? {
            CoreResponse::Project { project } => {
                eprintln!("Registered {} as a project.", project.name);
                Ok((project.id, project.name))
            }
            _ => Err("unexpected reply".into()),
        }
    }
}

/// A path as Explorer spells it.
///
/// For a folder on a network share, Explorer substitutes `%V` with
/// `UNC\server\share\path` rather than `\\server\share\path` -- the same folder, in a
/// spelling that is not a path to anything, so the Explorer right-click menu
/// ("Add to OLS", "Set up with OLS") refused every shared folder with "that path is not
/// a directory on disk" while the folder was sitting right there. The rewrite is here,
/// at the one place the CLI takes a path from whatever called it, rather than in the
/// core: the core is given paths by the app, which has no `%V` to expand, and a
/// general "accept this odd spelling" rule there would be a rule with no caller.
pub fn explorer_path(path: &Path) -> PathBuf {
    let s = path.as_os_str().to_string_lossy();
    if s.starts_with(r"\\?\") {
        return path.to_path_buf();
    }
    for prefix in [r"UNC\", "UNC/"] {
        if let Some(rest) = s.strip_prefix(prefix) {
            return PathBuf::from(format!(r"\\{rest}"));
        }
    }
    path.to_path_buf()
}

pub fn confirm(question: &str) -> bool {
    if !std::io::stdin().is_terminal() {
        return false;
    }
    print!("{question} [y/N] ");
    let _ = std::io::stdout().flush();
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer).is_ok()
        && matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

pub fn table(rows: Vec<Vec<String>>) {
    if rows.is_empty() {
        return;
    }
    let cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    let widths: Vec<usize> = (0..cols)
        .map(|c| {
            rows.iter()
                .map(|r| r.get(c).map(|s| s.chars().count()).unwrap_or(0))
                .max()
                .unwrap_or(0)
        })
        .collect();
    for r in rows {
        let line: Vec<String> = r
            .iter()
            .enumerate()
            .map(|(i, s)| {
                if i + 1 == r.len() {
                    s.clone()
                } else {
                    format!("{s:<w$}", w = widths[i])
                }
            })
            .collect();
        println!("{}", line.join("  ").trim_end());
    }
}

