//! Git repository manager (§125): the everyday Git work for a project without leaving the
//! app. Status and changed files, branches, stage / commit, pull / push / fetch, diffs,
//! history, remotes, stashes, `.gitignore` help, and cloning a repository into a new site.
//!
//! It drives the real `git` program: the one on PATH, or a portable Git installed from the
//! Runtimes page when there is none. It is not a replacement for a full Git client.
//!
//! HTTPS credentials are kept in the Secrets Manager per host. When a remote's host has
//! saved credentials, Git gets them through a tiny `GIT_ASKPASS` script that reads them
//! from that one child process's environment: never on the command line, never on disk,
//! never in a log. `GIT_TERMINAL_PROMPT=0` means Git fails instead of hanging on a prompt.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::app::Inner;
use crate::error::CoreError;
use crate::exec::{run_capture, Captured};

fn err(msg: impl Into<String>) -> CoreError {
    CoreError::GitError(msg.into())
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GitFile {
    pub path: String,
    /// Renamed from.
    pub from: Option<String>,
    /// Porcelain status letters: index (staged) and work tree.
    pub index: String,
    pub worktree: String,
    pub staged: bool,
    pub unstaged: bool,
    pub untracked: bool,
    pub conflicted: bool,
    /// modified, added, deleted, renamed, untracked, conflicted.
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Commit {
    pub hash: String,
    pub short: String,
    pub author: String,
    pub email: String,
    pub time: u64,
    pub subject: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Branch {
    pub name: String,
    pub current: bool,
    pub remote: bool,
    pub upstream: Option<String>,
    pub commit: String,
    pub subject: String,
    pub time: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Remote {
    pub name: String,
    pub url: String,
    /// Credentials are saved for this remote's host.
    pub has_credentials: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GitStatus {
    pub available: bool,
    pub git_path: Option<String>,
    pub version: Option<String>,
    pub is_repo: bool,
    pub branch: Option<String>,
    pub detached: bool,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub files: Vec<GitFile>,
    pub last_commit: Option<Commit>,
    pub remotes: Vec<Remote>,
    pub stashes: Vec<String>,
    pub has_gitignore: bool,
    /// A merge or rebase is in progress.
    pub operation: Option<String>,
}

/// Output of a network or history-changing command, for the UI to show.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitResult {
    pub ok: bool,
    pub output: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GitAuth {
    Https {
        username: String,
        password: String,
        remember: bool,
    },
    Ssh {
        key_path: String,
        remember: bool,
        passphrase: Option<String>,
    },
}

// ------------------------------------------------------------------------ parsing

fn kind_of(x: char, y: char) -> &'static str {
    match (x, y) {
        ('?', _) => "untracked",
        ('U', _) | (_, 'U') | ('A', 'A') | ('D', 'D') => "conflicted",
        ('R', _) | (_, 'R') => "renamed",
        ('A', _) => "added",
        ('D', _) | (_, 'D') => "deleted",
        _ => "modified",
    }
}

/// Parses `git status --porcelain=v2 --branch --untracked-files=all -z`.
pub fn parse_status(out: &str, st: &mut GitStatus) {
    let mut tokens = out.split('\0').filter(|t| !t.is_empty());
    while let Some(t) = tokens.next() {
        if let Some(h) = t.strip_prefix("# ") {
            if let Some(b) = h.strip_prefix("branch.head ") {
                st.detached = b == "(detached)";
                st.branch = Some(b.to_string());
            } else if let Some(u) = h.strip_prefix("branch.upstream ") {
                st.upstream = Some(u.to_string());
            } else if let Some(ab) = h.strip_prefix("branch.ab ") {
                let mut it = ab.split_whitespace();
                st.ahead = it
                    .next()
                    .and_then(|a| a.trim_start_matches('+').parse().ok())
                    .unwrap_or(0);
                st.behind = it
                    .next()
                    .and_then(|b| b.trim_start_matches('-').parse().ok())
                    .unwrap_or(0);
            }
            continue;
        }
        let mut f = GitFile::default();
        let (xy, path) = match t.as_bytes().first() {
            Some(b'1') => {
                let parts: Vec<&str> = t.splitn(9, ' ').collect();
                (
                    parts.get(1).copied().unwrap_or(".."),
                    parts.get(8).copied().unwrap_or_default(),
                )
            }
            Some(b'2') => {
                let parts: Vec<&str> = t.splitn(10, ' ').collect();
                f.from = tokens.next().map(str::to_string);
                (
                    parts.get(1).copied().unwrap_or(".."),
                    parts.get(9).copied().unwrap_or_default(),
                )
            }
            Some(b'u') => {
                let parts: Vec<&str> = t.splitn(11, ' ').collect();
                (
                    parts.get(1).copied().unwrap_or("UU"),
                    parts.get(10).copied().unwrap_or_default(),
                )
            }
            Some(b'?') => ("??", &t[2..]),
            _ => continue,
        };
        let mut chars = xy.chars();
        let (x, y) = (chars.next().unwrap_or('.'), chars.next().unwrap_or('.'));
        f.path = path.to_string();
        f.index = x.to_string();
        f.worktree = y.to_string();
        f.untracked = x == '?';
        f.kind = kind_of(x, y).into();
        f.conflicted = f.kind == "conflicted";
        f.staged = !f.untracked && !f.conflicted && x != '.';
        f.unstaged = f.untracked || f.conflicted || y != '.';
        st.files.push(f);
    }
}

const SEP: char = '\u{1f}';
const REC: char = '\u{1e}';

pub fn parse_log(out: &str) -> Vec<Commit> {
    out.split(REC)
        .filter_map(|rec| {
            let p: Vec<&str> = rec.trim_start_matches(['\n', '\r']).split(SEP).collect();
            (p.len() >= 6).then(|| Commit {
                hash: p[0].into(),
                short: p[1].into(),
                author: p[2].into(),
                email: p[3].into(),
                time: p[4].parse().unwrap_or(0),
                subject: p[5].into(),
            })
        })
        .collect()
}

/// A branch or remote name Git will accept, and that can't be read as an option.
fn safe_name(name: &str, what: &str) -> Result<String, CoreError> {
    let n = name.trim();
    let bad = n.is_empty()
        || n.starts_with('-')
        || n.contains("..")
        || n.ends_with('/')
        || n.ends_with(".lock")
        || n.chars()
            .any(|c| c.is_whitespace() || c.is_control() || "~^:?*[\\".contains(c));
    if bad {
        return Err(err(format!("\"{name}\" is not a valid {what} name")));
    }
    Ok(n.to_string())
}

/// Host of an https remote, for looking up saved credentials.
pub fn remote_host(url: &str) -> Option<String> {
    let url = url.trim();
    let host = if let Some(rest) = url.strip_prefix("https://").or_else(|| {
        url.strip_prefix("http://")
            .or_else(|| url.strip_prefix("ssh://"))
    }) {
        rest.split(['/', ':']).next()?.rsplit('@').next()?
    } else {
        // SCP-style SSH remotes: [user@]host:path
        let (authority, _) = url.split_once(':')?;
        if authority.contains('/') {
            return None;
        }
        authority.rsplit('@').next()?
    };
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

fn secret_user(host: &str) -> String {
    format!("git.{host}.user")
}

fn secret_token(host: &str) -> String {
    format!("git.{host}.token")
}

fn secret_ssh_passphrase(host: &str) -> String {
    format!("git.{host}.ssh_passphrase")
}

/// Ready-made `.gitignore` sections.
pub fn ignore_template(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "laravel" => "/vendor/\n/node_modules/\n/public/build/\n/public/hot\n/public/storage\n/storage/*.key\n.env\n.env.backup\n.phpunit.result.cache\n",
        "node" => "node_modules/\ndist/\n.next/\n.nuxt/\n*.log\n.env\n.env.local\n",
        "python" => "__pycache__/\n*.py[cod]\n.venv/\nvenv/\n.env\n*.egg-info/\n.pytest_cache/\n",
        "wordpress" => "/wp-content/uploads/\n/wp-content/cache/\n/wp-config.php\n*.log\n",
        "php" => "/vendor/\n.env\n*.cache\n",
        "editor" => ".idea/\n.vscode/*\n!.vscode/extensions.json\n*.swp\n.DS_Store\nThumbs.db\n",
        "openlocalserver" => "# The lock file and manifest are meant to be committed; only local state is ignored.\n.openlocalserver/*.local.yaml\n",
        _ => return None,
    })
}

/// Adds the template's lines the file doesn't already have. Returns the lines added.
pub fn merge_ignore(existing: &str, template: &str) -> (String, usize) {
    let have: std::collections::HashSet<&str> = existing.lines().map(str::trim).collect();
    let new: Vec<&str> = template
        .lines()
        .filter(|l| !l.trim().is_empty() && !have.contains(l.trim()))
        .collect();
    if new.is_empty() {
        return (existing.to_string(), 0);
    }
    let mut out = existing.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(&new.join("\n"));
    out.push('\n');
    (out, new.len())
}

// ------------------------------------------------------------------------ runner

/// Environment variables for git, and arguments that go before its subcommand.
type GitEnv = (Vec<(String, String)>, Vec<String>);

impl Inner {
    /// The Git to use: a pinned custom path, then PATH, then the usual install, then ours.
    pub fn git_path(&self) -> Option<PathBuf> {
        if let Some(c) = self.custom_installs.lock().unwrap().resolve("git", None) {
            return Some(PathBuf::from(&c.path));
        }
        if let Some(path) = std::env::var_os("PATH") {
            if let Some(p) = std::env::split_paths(&path)
                .map(|d| d.join("git.exe"))
                .find(|p| p.is_file())
            {
                return Some(p);
            }
        }
        for p in [
            r"C:\Program Files\Git\cmd\git.exe",
            r"C:\Program Files (x86)\Git\cmd\git.exe",
        ] {
            if Path::new(p).is_file() {
                return Some(PathBuf::from(p));
            }
        }
        self.runtimes
            .installed_versions("git")
            .into_iter()
            .next()
            .and_then(|v| self.runtimes.binary_path("git", &v))
    }

    fn askpass_script(&self) -> Result<PathBuf, CoreError> {
        let file = self.paths.data_dir().join("git-askpass.cmd");
        let body = "@echo off\r\nrem Answers Git's credential prompts from this process's environment (OpenLocalServer).\r\necho %~1| findstr /i \"username\" >nul && (echo %OLS_GIT_USER%) || (echo %OLS_GIT_TOKEN%)\r\n";
        if std::fs::read_to_string(&file).ok().as_deref() != Some(body) {
            std::fs::write(&file, body)?;
        }
        Ok(file)
    }

    fn ssh_askpass_script(&self) -> Result<PathBuf, CoreError> {
        let file = self.paths.data_dir().join("git-ssh-askpass.cmd");
        let body = "@echo off\r\necho %OLS_SSH_PASSPHRASE%\r\n";
        if std::fs::read_to_string(&file).ok().as_deref() != Some(body) {
            std::fs::write(&file, body)?;
        }
        Ok(file)
    }

    fn ssh_key_setting(&self, host: &str) -> Option<String> {
        self.settings
            .lock()
            .unwrap()
            .get(&format!("git.ssh_key.{host}"))?
            .as_str()
            .map(str::to_string)
    }

    fn ssh_command(key_path: &str) -> Result<String, CoreError> {
        let path = if key_path.trim().is_empty() || key_path.trim() == "~/.ssh/id_ed25519" {
            let default =
                directories::UserDirs::new().map(|u| u.home_dir().join(".ssh").join("id_ed25519"));
            default
                .filter(|p| p.is_file())
                .ok_or_else(|| {
                    err("choose an SSH private key file; ~/.ssh/id_ed25519 was not found")
                })?
                .display()
                .to_string()
        } else {
            key_path.trim().to_string()
        };
        if path.contains(['"', '\r', '\n', '\0']) {
            return Err(err("choose a valid SSH private key file"));
        }
        let canonical = std::fs::canonicalize(&path)
            .map_err(|_| err(format!("SSH private key file was not found: {path}")))?;
        Ok(format!(
            "ssh -i \"{}\" -o IdentitiesOnly=yes -o StrictHostKeyChecking=accept-new",
            canonical.display().to_string().replace('\\', "/")
        ))
    }

    /// Environment for a network command: saved credentials for the remote's host, if any.
    fn git_env(&self, url: Option<&str>) -> Result<GitEnv, CoreError> {
        let mut env = vec![("GIT_TERMINAL_PROMPT".to_string(), "0".to_string())];
        let mut pre = Vec::new();
        if let Some(host) = url.and_then(remote_host) {
            if url.is_some_and(|u| {
                u.starts_with("ssh://")
                    || (!u.starts_with("http://") && !u.starts_with("https://") && u.contains(':'))
            }) {
                if let Some(key_path) = self.ssh_key_setting(&host) {
                    env.push(("GIT_SSH_COMMAND".into(), Self::ssh_command(&key_path)?));
                    if let Some(passphrase) =
                        crate::secrets::get_secret(&secret_ssh_passphrase(&host)).map_err(err)?
                    {
                        env.push((
                            "SSH_ASKPASS".into(),
                            self.ssh_askpass_script()?.display().to_string(),
                        ));
                        env.push(("SSH_ASKPASS_REQUIRE".into(), "force".into()));
                        env.push(("OLS_SSH_PASSPHRASE".into(), passphrase));
                    }
                }
            }
            let user = crate::secrets::get_secret(&secret_user(&host))
                .ok()
                .flatten();
            let token = crate::secrets::get_secret(&secret_token(&host))
                .ok()
                .flatten();
            if let Some(token) = token {
                env.push((
                    "GIT_ASKPASS".into(),
                    self.askpass_script()?.display().to_string(),
                ));
                env.push(("OLS_GIT_USER".into(), user.unwrap_or_else(|| "git".into())));
                env.push(("OLS_GIT_TOKEN".into(), token));
                // Our answer, not a credential manager's window.
                pre.extend(["-c".to_string(), "credential.helper=".to_string()]);
            }
        }
        Ok((env, pre))
    }

    fn git_run(
        &self,
        dir: &Path,
        args: &[&str],
        env: &[(String, String)],
        pre: &[String],
        timeout: Duration,
    ) -> Result<Captured, CoreError> {
        let git = self.git_path().ok_or_else(|| err("Git was not found. Install Git for Windows, or the portable Git on the Runtimes page."))?;
        let mut full: Vec<String> = pre.to_vec();
        full.extend(["-c".to_string(), "core.quotepath=off".to_string()]);
        full.extend(args.iter().map(|a| a.to_string()));
        Ok(run_capture(&git, &full, Some(dir), env, timeout))
    }

    /// Runs git and returns stdout, or its error text.
    fn git_ok(&self, dir: &Path, args: &[&str]) -> Result<String, CoreError> {
        let out = self.git_run(
            dir,
            args,
            &[("GIT_TERMINAL_PROMPT".into(), "0".into())],
            &[],
            Duration::from_secs(60),
        )?;
        if out.success() {
            Ok(out.stdout)
        } else {
            Err(err(scrub(&out.combined())))
        }
    }

    fn project_dir(&self, project_id: &str) -> Result<PathBuf, CoreError> {
        let p = self
            .projects
            .lock()
            .unwrap()
            .get(project_id)
            .ok_or_else(|| CoreError::InvalidProjectPath(project_id.to_string()))?;
        Ok(PathBuf::from(p.path))
    }

    pub fn git_status(&self, project_id: &str) -> Result<GitStatus, CoreError> {
        let dir = self.project_dir(project_id)?;
        let mut st = GitStatus {
            git_path: self.git_path().map(|p| p.display().to_string()),
            has_gitignore: dir.join(".gitignore").is_file(),
            ..Default::default()
        };
        if st.git_path.is_none() {
            return Ok(st);
        }
        st.available = true;
        st.version = self
            .git_ok(&dir, &["--version"])
            .ok()
            .map(|v| v.trim().trim_start_matches("git version ").to_string());
        let inside = self.git_run(
            &dir,
            &["rev-parse", "--is-inside-work-tree"],
            &[],
            &[],
            Duration::from_secs(20),
        )?;
        if !inside.success() || inside.stdout.trim() != "true" {
            return Ok(st);
        }
        st.is_repo = true;
        let out = self.git_ok(
            &dir,
            &[
                "status",
                "--porcelain=v2",
                "--branch",
                "--untracked-files=all",
                "-z",
            ],
        )?;
        parse_status(&out, &mut st);
        if let Ok(log) = self.git_ok(
            &dir,
            &[
                "log",
                "-n",
                "1",
                "--pretty=format:%H%x1f%h%x1f%an%x1f%ae%x1f%at%x1f%s%x1e",
            ],
        ) {
            st.last_commit = parse_log(&log).into_iter().next();
        }
        st.remotes = self.git_remotes(&dir);
        st.stashes = self
            .git_ok(&dir, &["stash", "list", "--pretty=format:%gd: %s"])
            .map(|s| s.lines().map(str::to_string).collect())
            .unwrap_or_default();
        let git_dir = self
            .git_ok(&dir, &["rev-parse", "--git-dir"])
            .map(|d| dir.join(d.trim()))
            .unwrap_or_else(|_| dir.join(".git"));
        st.operation = if git_dir.join("MERGE_HEAD").exists() {
            Some("merge".into())
        } else if git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists() {
            Some("rebase".into())
        } else {
            None
        };
        Ok(st)
    }

    fn git_remotes(&self, dir: &Path) -> Vec<Remote> {
        let Ok(out) = self.git_ok(dir, &["remote", "-v"]) else {
            return vec![];
        };
        let mut remotes: Vec<Remote> = Vec::new();
        for line in out.lines().filter(|l| l.ends_with("(fetch)")) {
            let mut it = line.split_whitespace();
            if let (Some(name), Some(url)) = (it.next(), it.next()) {
                let has = remote_host(url).is_some_and(|h| {
                    crate::secrets::get_secret(&secret_token(&h))
                        .ok()
                        .flatten()
                        .is_some()
                });
                remotes.push(Remote {
                    name: name.into(),
                    url: url.into(),
                    has_credentials: has,
                });
            }
        }
        remotes
    }

    pub fn git_init(&self, project_id: &str) -> Result<GitStatus, CoreError> {
        let dir = self.project_dir(project_id)?;
        self.git_ok(&dir, &["init", "-b", "main"])?;
        self.git_status(project_id)
    }

    pub fn git_branches(&self, project_id: &str) -> Result<Vec<Branch>, CoreError> {
        let dir = self.project_dir(project_id)?;
        let out = self.git_ok(&dir, &["for-each-ref", "--sort=-committerdate", "--format=%(refname)%1f%(refname:short)%1f%(HEAD)%1f%(upstream:short)%1f%(objectname:short)%1f%(contents:subject)%1f%(committerdate:unix)", "refs/heads", "refs/remotes"])?;
        Ok(out
            .lines()
            .filter_map(|l| {
                let p: Vec<&str> = l.split(SEP).collect();
                if p.len() < 7 || p[0].ends_with("/HEAD") {
                    return None;
                }
                Some(Branch {
                    remote: p[0].starts_with("refs/remotes/"),
                    name: p[1].into(),
                    current: p[2] == "*",
                    upstream: Some(p[3].to_string()).filter(|u| !u.is_empty()),
                    commit: p[4].into(),
                    subject: p[5].into(),
                    time: p[6].parse().unwrap_or(0),
                })
            })
            .collect())
    }

    pub fn git_create_branch(
        &self,
        project_id: &str,
        name: &str,
        checkout: bool,
    ) -> Result<(), CoreError> {
        let dir = self.project_dir(project_id)?;
        let name = safe_name(name, "branch")?;
        if checkout {
            self.git_ok(&dir, &["switch", "-c", &name])?;
        } else {
            self.git_ok(&dir, &["branch", &name])?;
        }
        Ok(())
    }

    /// Switches branch; a remote branch ("origin/feature") gets a local tracking branch.
    pub fn git_switch(&self, project_id: &str, name: &str) -> Result<(), CoreError> {
        let dir = self.project_dir(project_id)?;
        let name = safe_name(name, "branch")?;
        let is_remote = self
            .git_branches(project_id)?
            .iter()
            .any(|b| b.remote && b.name == name);
        if is_remote {
            let local = name
                .split_once('/')
                .map(|(_, b)| b.to_string())
                .unwrap_or(name.clone());
            self.git_ok(&dir, &["switch", "--track", "-c", &local, &name])
                .or_else(|_| self.git_ok(&dir, &["switch", &local]))?;
        } else {
            self.git_ok(&dir, &["switch", &name])?;
        }
        Ok(())
    }

    pub fn git_delete_branch(
        &self,
        project_id: &str,
        name: &str,
        force: bool,
    ) -> Result<(), CoreError> {
        let dir = self.project_dir(project_id)?;
        let name = safe_name(name, "branch")?;
        self.git_ok(&dir, &["branch", if force { "-D" } else { "-d" }, &name])?;
        Ok(())
    }

    fn paths_args<'a>(base: &[&'a str], paths: &'a [String]) -> Result<Vec<&'a str>, CoreError> {
        if paths
            .iter()
            .any(|p| p.contains("..") || Path::new(p).is_absolute())
        {
            return Err(err("file paths must be inside the project"));
        }
        let mut args = base.to_vec();
        args.push("--");
        if paths.is_empty() {
            args.push(".");
        } else {
            args.extend(paths.iter().map(String::as_str));
        }
        Ok(args)
    }

    /// Stages files (all when `paths` is empty).
    pub fn git_stage(&self, project_id: &str, paths: &[String]) -> Result<(), CoreError> {
        let dir = self.project_dir(project_id)?;
        self.git_ok(&dir, &Self::paths_args(&["add", "-A"], paths)?)?;
        Ok(())
    }

    pub fn git_unstage(&self, project_id: &str, paths: &[String]) -> Result<(), CoreError> {
        let dir = self.project_dir(project_id)?;
        // `restore --staged` needs a commit; before the first one, `rm --cached` does it.
        if self
            .git_ok(&dir, &["rev-parse", "--verify", "HEAD"])
            .is_ok()
        {
            self.git_ok(&dir, &Self::paths_args(&["restore", "--staged"], paths)?)?;
        } else {
            self.git_ok(
                &dir,
                &Self::paths_args(&["rm", "-r", "--cached", "-q"], paths)?,
            )?;
        }
        Ok(())
    }

    /// Throws away work-tree changes (and deletes untracked files) for `paths`. The UI
    /// confirms first; there is no undo.
    pub fn git_discard(&self, project_id: &str, paths: &[String]) -> Result<(), CoreError> {
        if paths.is_empty() {
            return Err(err("choose the files to discard"));
        }
        let dir = self.project_dir(project_id)?;
        let st = self.git_status(project_id)?;
        let (untracked, tracked): (Vec<String>, Vec<String>) = paths
            .iter()
            .cloned()
            .partition(|p| st.files.iter().any(|f| &f.path == p && f.untracked));
        if !tracked.is_empty() {
            self.git_ok(
                &dir,
                &Self::paths_args(&["restore", "--staged", "--worktree"], &tracked)?,
            )?;
        }
        if !untracked.is_empty() {
            self.git_ok(&dir, &Self::paths_args(&["clean", "-f"], &untracked)?)?;
        }
        Ok(())
    }

    pub fn git_commit(
        &self,
        project_id: &str,
        message: &str,
        amend: bool,
    ) -> Result<Commit, CoreError> {
        let dir = self.project_dir(project_id)?;
        if message.trim().is_empty() && !amend {
            return Err(err("write a commit message"));
        }
        let mut args = vec!["commit", "-m", message.trim()];
        if amend {
            args = if message.trim().is_empty() {
                vec!["commit", "--amend", "--no-edit"]
            } else {
                vec!["commit", "--amend", "-m", message.trim()]
            };
        }
        let out = self.git_run(
            &dir,
            &args,
            &[("GIT_TERMINAL_PROMPT".into(), "0".into())],
            &[],
            Duration::from_secs(120),
        )?;
        if !out.success() {
            let text = out.combined();
            return Err(err(if text.contains("Please tell me who you are") {
                "Git doesn't know your name and email yet. Set them in Git (git config --global user.name / user.email) and commit again.".to_string()
            } else if text.contains("nothing to commit") || text.contains("no changes added") {
                "nothing is staged to commit".to_string()
            } else {
                scrub(&text)
            }));
        }
        let log = self.git_ok(
            &dir,
            &[
                "log",
                "-n",
                "1",
                "--pretty=format:%H%x1f%h%x1f%an%x1f%ae%x1f%at%x1f%s%x1e",
            ],
        )?;
        parse_log(&log)
            .into_iter()
            .next()
            .ok_or_else(|| err("the commit was made but could not be read back"))
    }

    fn origin_url(&self, dir: &Path, remote: Option<&str>) -> Option<String> {
        let remotes = self.git_remotes(dir);
        let name = remote.map(str::to_string).or_else(|| {
            self.git_ok(dir, &["rev-parse", "--abbrev-ref", "@{upstream}"])
                .ok()
                .and_then(|u| u.trim().split('/').next().map(str::to_string))
        });
        name.and_then(|n| remotes.iter().find(|r| r.name == n).map(|r| r.url.clone()))
            .or_else(|| remotes.first().map(|r| r.url.clone()))
    }

    /// pull, push or fetch. Network commands get saved credentials and a longer timeout.
    pub fn git_sync(
        &self,
        project_id: &str,
        action: &str,
        remote: Option<&str>,
    ) -> Result<GitResult, CoreError> {
        let dir = self.project_dir(project_id)?;
        let remote = remote.map(|r| safe_name(r, "remote")).transpose()?;
        let url = self.origin_url(&dir, remote.as_deref());
        let (env, pre) = self.git_env(url.as_deref())?;
        let branch = self
            .git_ok(&dir, &["rev-parse", "--abbrev-ref", "HEAD"])
            .map(|b| b.trim().to_string())
            .unwrap_or_default();
        let has_upstream = self
            .git_ok(&dir, &["rev-parse", "--abbrev-ref", "@{upstream}"])
            .is_ok();
        let mut args: Vec<&str> = match action {
            "pull" => vec!["pull", "--ff-only"],
            "fetch" => vec!["fetch", "--prune"],
            "push" if !has_upstream => vec!["push", "-u"],
            "push" => vec!["push"],
            other => return Err(err(format!("unknown Git action \"{other}\""))),
        };
        let remote_name = remote
            .clone()
            .or_else(|| (!has_upstream && action == "push").then(|| "origin".to_string()));
        if let Some(r) = &remote_name {
            args.push(r);
            if action == "push" && !has_upstream && !branch.is_empty() {
                args.push(&branch);
            }
        } else if action == "fetch" {
            args.push("--all");
        }
        let out = self.git_run(&dir, &args, &env, &pre, Duration::from_secs(600))?;
        let mut text = scrub(&out.combined());
        if !out.success() {
            if text.contains("Authentication failed")
                || text.contains("could not read Username")
                || text.contains("terminal prompts disabled")
            {
                text.push_str("\n\nSave a username and access token for this host in the Git tab's credentials, then try again.");
            } else if action == "pull" && text.contains("Not possible to fast-forward") {
                text.push_str("\n\nYour branch and the remote have both changed. Merge or rebase in your Git client, then pull again.");
            }
        }
        Ok(GitResult {
            ok: out.success(),
            output: if text.is_empty() {
                format!("{action}: done")
            } else {
                text
            },
        })
    }

    pub fn git_diff(
        &self,
        project_id: &str,
        path: &str,
        staged: bool,
    ) -> Result<String, CoreError> {
        let dir = self.project_dir(project_id)?;
        let paths = [path.to_string()];
        let base: &[&str] = if staged {
            &["diff", "--cached", "--no-color"]
        } else {
            &["diff", "--no-color"]
        };
        let text = self.git_ok(&dir, &Self::paths_args(base, &paths)?)?;
        if text.is_empty() {
            // Untracked: show the whole file as added.
            let full = dir.join(path);
            if full.is_file() {
                let content = std::fs::read(&full).map_err(|e| err(e.to_string()))?;
                if content.len() > 512 * 1024 || content.contains(&0) {
                    return Ok(format!(
                        "new file {path} ({} bytes, not shown)",
                        content.len()
                    ));
                }
                let body: String = String::from_utf8_lossy(&content)
                    .lines()
                    .map(|l| format!("+{l}\n"))
                    .collect();
                return Ok(format!("new file {path}\n{body}"));
            }
        }
        Ok(text)
    }

    /// Everything that is staged, as one diff (for suggesting a commit message).
    pub fn git_staged_diff(&self, project_id: &str) -> Result<String, CoreError> {
        let dir = self.project_dir(project_id)?;
        self.git_ok(
            &dir,
            &["diff", "--cached", "--no-color", "--stat", "--patch"],
        )
    }

    pub fn git_log(&self, project_id: &str, limit: usize) -> Result<Vec<Commit>, CoreError> {
        let dir = self.project_dir(project_id)?;
        let n = limit.clamp(1, 500).to_string();
        match self.git_ok(
            &dir,
            &[
                "log",
                "-n",
                &n,
                "--pretty=format:%H%x1f%h%x1f%an%x1f%ae%x1f%at%x1f%s%x1e",
            ],
        ) {
            Ok(out) => Ok(parse_log(&out)),
            // A new repository has no commits yet.
            Err(e) if e.to_string().contains("does not have any commits") => Ok(vec![]),
            Err(e) => Err(e),
        }
    }

    pub fn git_show(&self, project_id: &str, hash: &str) -> Result<String, CoreError> {
        let dir = self.project_dir(project_id)?;
        if !hash.chars().all(|c| c.is_ascii_hexdigit()) || hash.is_empty() {
            return Err(err("not a commit id"));
        }
        self.git_ok(&dir, &["show", "--stat", "--patch", "--no-color", hash])
    }

    pub fn git_add_remote(&self, project_id: &str, name: &str, url: &str) -> Result<(), CoreError> {
        let dir = self.project_dir(project_id)?;
        let name = safe_name(name, "remote")?;
        if url.trim().is_empty() || url.trim().starts_with('-') {
            return Err(err("that is not a repository address"));
        }
        self.git_ok(&dir, &["remote", "add", &name, url.trim()])?;
        Ok(())
    }

    pub fn git_remove_remote(&self, project_id: &str, name: &str) -> Result<(), CoreError> {
        let dir = self.project_dir(project_id)?;
        self.git_ok(&dir, &["remote", "remove", &safe_name(name, "remote")?])?;
        Ok(())
    }

    /// `action`: push (with an optional message), pop, apply or drop (with an index).
    pub fn git_stash(
        &self,
        project_id: &str,
        action: &str,
        message: Option<&str>,
        index: Option<u32>,
    ) -> Result<GitResult, CoreError> {
        let dir = self.project_dir(project_id)?;
        let reference = format!("stash@{{{}}}", index.unwrap_or(0));
        let args: Vec<&str> = match action {
            "push" => match message.filter(|m| !m.trim().is_empty()) {
                Some(m) => vec!["stash", "push", "--include-untracked", "-m", m],
                None => vec!["stash", "push", "--include-untracked"],
            },
            "pop" => vec!["stash", "pop", &reference],
            "apply" => vec!["stash", "apply", &reference],
            "drop" => vec!["stash", "drop", &reference],
            other => return Err(err(format!("unknown stash action \"{other}\""))),
        };
        let out = self.git_run(&dir, &args, &[], &[], Duration::from_secs(120))?;
        Ok(GitResult {
            ok: out.success(),
            output: scrub(&out.combined()),
        })
    }

    /// Adds a template's lines to `.gitignore` (only those it doesn't have). Returns how many.
    pub fn git_add_ignore(&self, project_id: &str, template: &str) -> Result<usize, CoreError> {
        let dir = self.project_dir(project_id)?;
        let t = ignore_template(template)
            .ok_or_else(|| err(format!("no .gitignore template \"{template}\"")))?;
        let file = dir.join(".gitignore");
        let existing = std::fs::read_to_string(&file).unwrap_or_default();
        let (text, added) = merge_ignore(&existing, t);
        if added > 0 {
            std::fs::write(&file, text)?;
        }
        Ok(added)
    }

    pub fn git_set_credentials(
        &self,
        host: &str,
        username: &str,
        token: Option<&str>,
    ) -> Result<(), CoreError> {
        let host = host.trim().to_ascii_lowercase();
        if host.is_empty() || host.contains('/') {
            return Err(err("enter a host like github.com"));
        }
        match token.filter(|t| !t.is_empty()) {
            Some(t) => {
                crate::secrets::set_secret(&secret_user(&host), username.trim()).map_err(err)?;
                crate::secrets::set_secret(&secret_token(&host), t).map_err(err)?;
            }
            None => {
                crate::secrets::delete_secret(&secret_user(&host)).map_err(err)?;
                crate::secrets::delete_secret(&secret_token(&host)).map_err(err)?;
            }
        }
        Ok(())
    }

    /// Clones into `target` and registers it as a project (and gives it its automatic site).
    pub fn git_clone(
        &self,
        url: &str,
        target: &str,
        branch: Option<&str>,
        auth: Option<GitAuth>,
    ) -> Result<crate::project::Project, CoreError> {
        let url = url.trim();
        if url.is_empty() || url.starts_with('-') {
            return Err(err("enter the repository's address"));
        }
        let target = PathBuf::from(target.trim());
        if !target.is_absolute() {
            return Err(err("choose a full folder path to clone into"));
        }
        if target.exists()
            && std::fs::read_dir(&target)
                .map(|mut d| d.next().is_some())
                .unwrap_or(false)
        {
            return Err(err(format!("{} is not empty", target.display())));
        }
        let parent = target
            .parent()
            .ok_or_else(|| err("choose a folder inside another folder"))?;
        std::fs::create_dir_all(parent)?;
        let (mut env, mut pre) = self.git_env(Some(url))?;
        if let Some(auth) = auth {
            let host = remote_host(url)
                .ok_or_else(|| err("enter a valid HTTPS or SSH repository address"))?;
            match auth {
                GitAuth::Https {
                    username,
                    password,
                    remember,
                } => {
                    if !url.starts_with("https://") {
                        return Err(err("username and password authentication requires an HTTPS repository address"));
                    }
                    if username.trim().is_empty() || password.is_empty() {
                        return Err(err("enter both a username and password or access token"));
                    }
                    if remember {
                        self.git_set_credentials(&host, &username, Some(&password))?;
                    }
                    env.retain(|(k, _)| {
                        k != "GIT_ASKPASS" && k != "OLS_GIT_USER" && k != "OLS_GIT_TOKEN"
                    });
                    env.push((
                        "GIT_ASKPASS".into(),
                        self.askpass_script()?.display().to_string(),
                    ));
                    env.push(("OLS_GIT_USER".into(), username));
                    env.push(("OLS_GIT_TOKEN".into(), password));
                    if !pre.iter().any(|x| x == "credential.helper=") {
                        pre.extend(["-c".into(), "credential.helper=".into()]);
                    }
                }
                GitAuth::Ssh {
                    key_path,
                    remember,
                    passphrase,
                } => {
                    if !(url.starts_with("ssh://")
                        || (!url.starts_with("http://")
                            && !url.starts_with("https://")
                            && url.contains(':')))
                    {
                        return Err(err(
                            "SSH key authentication requires an SSH repository address",
                        ));
                    }
                    let ssh_command = Self::ssh_command(&key_path)?;
                    if remember {
                        let effective_key = if key_path.trim().is_empty()
                            || key_path.trim() == "~/.ssh/id_ed25519"
                        {
                            directories::UserDirs::new()
                                .map(|u| u.home_dir().join(".ssh").join("id_ed25519"))
                                .ok_or_else(|| err("could not locate the home folder"))?
                        } else {
                            PathBuf::from(key_path.clone())
                        };
                        let effective_key = std::fs::canonicalize(effective_key)
                            .map_err(|_| err("SSH private key file was not found"))?;
                        self.settings.lock().unwrap().set(
                            format!("git.ssh_key.{host}"),
                            serde_json::Value::String(effective_key.display().to_string()),
                        )?;
                        match passphrase.as_deref().filter(|p| !p.is_empty()) {
                            Some(value) => {
                                crate::secrets::set_secret(&secret_ssh_passphrase(&host), value)
                                    .map_err(err)?
                            }
                            None => crate::secrets::delete_secret(&secret_ssh_passphrase(&host))
                                .map_err(err)?,
                        }
                    }
                    env.retain(|(k, _)| k != "GIT_SSH_COMMAND");
                    env.retain(|(k, _)| {
                        k != "SSH_ASKPASS"
                            && k != "SSH_ASKPASS_REQUIRE"
                            && k != "OLS_SSH_PASSPHRASE"
                    });
                    env.push(("GIT_SSH_COMMAND".into(), ssh_command));
                    if let Some(passphrase) = passphrase.filter(|p| !p.is_empty()) {
                        env.push((
                            "SSH_ASKPASS".into(),
                            self.ssh_askpass_script()?.display().to_string(),
                        ));
                        env.push(("SSH_ASKPASS_REQUIRE".into(), "force".into()));
                        env.push(("OLS_SSH_PASSPHRASE".into(), passphrase));
                    }
                }
            }
        }
        let target_s = target.display().to_string();
        let mut args = vec!["clone", "--progress"];
        let branch = branch.map(|b| safe_name(b, "branch")).transpose()?;
        if let Some(b) = &branch {
            args.extend(["--branch", b.as_str()]);
        }
        args.extend(["--", url, target_s.as_str()]);
        let out = self.git_run(parent, &args, &env, &pre, Duration::from_secs(1800))?;
        if !out.success() {
            return Err(err(scrub(&out.combined())));
        }
        let project = self.projects.lock().unwrap().register(&target_s)?;
        if let Err(e) = self.sync_auto_domains() {
            tracing::warn!(error = %e, "automatic domains could not be synced");
        }
        Ok(project)
    }
}

/// Removes credentials Git might echo back in a URL ("https://user:token@host").
fn scrub(text: &str) -> String {
    static URL: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = URL.get_or_init(|| regex::Regex::new(r"(https?://)[^/@\s:]+(:[^@\s/]+)?@").unwrap());
    re.replace_all(text.trim(), "$1[redacted]@").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn porcelain_v2_status_is_understood() {
        let out = "# branch.oid abc\0# branch.head main\0# branch.upstream origin/main\0# branch.ab +2 -1\0\
1 .M N... 100644 100644 100644 aaa bbb src/app.php\0\
1 A. N... 000000 100644 100644 000 ccc new file.txt\0\
2 R. N... 100644 100644 100644 ddd eee R100 renamed.txt\0old.txt\0\
u UU N... 100644 100644 100644 100644 f1 f2 f3 conflict.txt\0\
? notes.md\0";
        let mut st = GitStatus::default();
        parse_status(out, &mut st);
        assert_eq!(
            (
                st.branch.as_deref(),
                st.upstream.as_deref(),
                st.ahead,
                st.behind
            ),
            (Some("main"), Some("origin/main"), 2, 1)
        );
        let by = |p: &str| st.files.iter().find(|f| f.path == p).unwrap().clone();
        assert!(by("src/app.php").unstaged && !by("src/app.php").staged);
        assert!(
            by("new file.txt").staged && by("new file.txt").kind == "added",
            "paths with spaces survive"
        );
        assert_eq!(by("renamed.txt").from.as_deref(), Some("old.txt"));
        assert!(by("conflict.txt").conflicted);
        assert!(by("notes.md").untracked);
    }

    #[test]
    fn names_that_could_be_options_or_bad_refs_are_refused() {
        assert!(safe_name("feature/login", "branch").is_ok());
        for bad in ["-rf", "a..b", "has space", "x.lock", "ends/", ""] {
            assert!(safe_name(bad, "branch").is_err(), "{bad}");
        }
    }

    #[test]
    fn credentials_never_show_in_output() {
        assert_eq!(
            scrub("fatal: https://me:ghp_secret@github.com/x.git not found"),
            "fatal: https://[redacted]@github.com/x.git not found"
        );
        assert_eq!(
            remote_host("https://me@GitHub.com/org/repo.git").as_deref(),
            Some("github.com")
        );
        assert_eq!(remote_host("git@github.com:org/repo.git"), None);
    }

    #[test]
    fn gitignore_templates_only_add_missing_lines() {
        let (text, n) = merge_ignore("/vendor/\n.env\n", ignore_template("laravel").unwrap());
        assert!(
            n > 0
                && text.starts_with("/vendor/\n.env\n\n")
                && text.matches("/vendor/").count() == 1
        );
        let (_, again) = merge_ignore(&text, ignore_template("laravel").unwrap());
        assert_eq!(again, 0);
    }

    /// Uses the real git when this machine has one.
    #[test]
    fn a_repository_can_be_created_committed_and_branched() {
        let home = crate::test_support::isolated_home();
        let settings = crate::settings::SettingsService::load(&home.paths).unwrap();
        let core = crate::command::Core::new(settings, home.paths.clone());
        let i = core.inner();
        if i.git_path().is_none() {
            eprintln!("no git on this machine; skipped");
            return;
        }
        let dir = home.paths.root().join("repo");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "one\n").unwrap();
        let id = i
            .projects
            .lock()
            .unwrap()
            .register(&dir.display().to_string())
            .unwrap()
            .id;
        assert!(!i.git_status(&id).unwrap().is_repo);
        i.git_init(&id).unwrap();
        let _ = i.git_ok(&dir, &["config", "user.email", "t@example.com"]);
        let _ = i.git_ok(&dir, &["config", "user.name", "Test"]);
        let _ = i.git_ok(&dir, &["config", "commit.gpgsign", "false"]);
        assert!(i
            .git_status(&id)
            .unwrap()
            .files
            .iter()
            .any(|f| f.path == "a.txt" && f.untracked));
        assert!(i.git_diff(&id, "a.txt", false).unwrap().contains("+one"));
        i.git_stage(&id, &[]).unwrap();
        let c = i.git_commit(&id, "first", false).unwrap();
        assert_eq!(c.subject, "first");
        assert!(i.git_status(&id).unwrap().files.is_empty());
        i.git_create_branch(&id, "feature", true).unwrap();
        assert!(i
            .git_branches(&id)
            .unwrap()
            .iter()
            .any(|b| b.name == "feature" && b.current));
        assert!(i
            .git_commit(&id, "empty", false)
            .unwrap_err()
            .to_string()
            .contains("nothing"));
        assert_eq!(i.git_log(&id, 10).unwrap().len(), 1);
    }
}
