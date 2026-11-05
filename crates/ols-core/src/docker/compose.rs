//! Docker Compose stacks (§166): find compose files, read / check / save them, and run
//! `docker compose` actions with live output.
//!
//! Compose has no Engine API, so actions run the `docker compose` CLI. Discovery is file
//! checks plus the engine's own labels; nothing here runs at startup.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::types::ContainerSummary;
use crate::error::CoreError;

/// The file names `docker compose` looks for, in its own order of preference.
pub const COMPOSE_FILE_NAMES: &[&str] = &[
    "compose.yaml",
    "compose.yml",
    "docker-compose.yaml",
    "docker-compose.yml",
];

/// Compose files bigger than this are not opened in the editor.
const MAX_FILE: u64 = 1024 * 1024;
/// Backups kept per compose file under `data/docker/backups`.
const KEEP_BACKUPS: usize = 10;
/// Long enough for a large `pull` or `build`.
const ACTION_TIMEOUT: Duration = Duration::from_secs(60 * 60);
const CONFIG_TIMEOUT: Duration = Duration::from_secs(30);
/// Output lines shown live in the task before switching to "last lines only".
const LIVE_LINES: usize = 300;
const TAIL_LINES: usize = 60;

/// A published port as written in a compose file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComposePort {
    /// Port on this computer; `None` when Docker picks one or the value uses a variable.
    pub host: Option<u16>,
    pub container: u16,
}

/// One service of a stack: from the file, plus its container when one exists.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComposeService {
    pub name: String,
    pub image: Option<String>,
    /// Built from a Dockerfile (`build:`).
    pub build: bool,
    pub ports: Vec<ComposePort>,
    /// Container state (`running`, `exited`, ...) when a container exists.
    pub state: Option<String>,
    pub container: Option<String>,
}

/// A Compose project: a file on disk, a running stack, or both.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComposeStack {
    /// Compose project name (what `docker compose ls` shows).
    pub name: String,
    pub file: Option<String>,
    pub dir: Option<String>,
    /// The OLS project whose folder holds the file.
    pub project_id: Option<String>,
    pub project_name: Option<String>,
    pub services: Vec<ComposeService>,
    pub running: usize,
    /// Why the file could not be read, when it could not.
    pub file_error: Option<String>,
    /// The file uses `${OLS_SECRET_*}` values that OLS supplies from the keyring.
    pub uses_secrets: bool,
    /// "Connect to OLS" is on: runs add OLS site names and OLS's CA certificate.
    #[serde(default)]
    pub connected: bool,
}

/// What `validate` found. `ok` is false only for errors; warnings never block a save.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComposeCheck {
    pub ok: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    /// `docker compose config` ran (the CLI was found).
    pub docker_checked: bool,
}

/// What a stack action does.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ComposeAction {
    Up,
    Down,
    Stop,
    Start,
    Restart,
    Pull,
    Build,
}

impl ComposeAction {
    fn args(self) -> &'static [&'static str] {
        match self {
            ComposeAction::Up => &["up", "-d"],
            // Volumes are kept: `down` never deletes data.
            ComposeAction::Down => &["down"],
            ComposeAction::Stop => &["stop"],
            ComposeAction::Start => &["start"],
            ComposeAction::Restart => &["restart"],
            ComposeAction::Pull => &["pull"],
            ComposeAction::Build => &["build"],
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ComposeAction::Up => "Start (up)",
            ComposeAction::Down => "Stop and remove containers (down)",
            ComposeAction::Stop => "Stop",
            ComposeAction::Start => "Start",
            ComposeAction::Restart => "Restart",
            ComposeAction::Pull => "Pull images",
            ComposeAction::Build => "Build images",
        }
    }
}

// ---------------------------------------------------------------- parsing

/// The compose project name Docker would use: the file's `name:`, else the folder name,
/// lower-cased with anything outside `[a-z0-9_-]` dropped.
pub fn project_name(file: &Path, doc: Option<&serde_yaml_ng::Value>) -> String {
    if let Some(name) = doc
        .and_then(|d| d.get("name"))
        .and_then(|v| v.as_str())
        .map(sanitize_name)
        .filter(|n| !n.is_empty())
    {
        return name;
    }
    let folder = file
        .parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let name = sanitize_name(&folder);
    if name.is_empty() {
        "stack".into()
    } else {
        name
    }
}

pub(crate) fn sanitize_name(raw: &str) -> String {
    let s: String = raw
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
        .collect();
    s.trim_start_matches(['_', '-']).to_string()
}

/// Parses one `ports:` entry: `"8080:80"`, `"127.0.0.1:8080:80"`, `"80"`, `"8080:80/udp"`,
/// or the long form `{target: 80, published: 8080}`. Values with variables give `None`.
pub fn parse_port(v: &serde_yaml_ng::Value) -> Option<ComposePort> {
    use serde_yaml_ng::Value;
    match v {
        Value::Number(n) => n
            .as_u64()
            .and_then(|p| u16::try_from(p).ok())
            .map(|c| ComposePort {
                host: None,
                container: c,
            }),
        Value::String(s) => {
            let s = s.split('/').next()?.trim();
            let parts: Vec<&str> = s.rsplitn(3, ':').collect();
            // rsplitn: [container, host?, ip?]
            let container: u16 = parts.first()?.split('-').next()?.parse().ok()?;
            let host = parts
                .get(1)
                .and_then(|h| h.split('-').next())
                .and_then(|h| h.parse::<u16>().ok());
            Some(ComposePort { host, container })
        }
        Value::Mapping(m) => {
            let target = m.get("target")?;
            let container = match target {
                Value::Number(n) => n.as_u64().and_then(|p| u16::try_from(p).ok())?,
                Value::String(s) => s.parse().ok()?,
                _ => return None,
            };
            let host = m.get("published").and_then(|p| match p {
                Value::Number(n) => n.as_u64().and_then(|p| u16::try_from(p).ok()),
                Value::String(s) => s.split('-').next().and_then(|s| s.parse().ok()),
                _ => None,
            });
            Some(ComposePort { host, container })
        }
        _ => None,
    }
}

/// Services as written in the file, in file order.
pub fn parse_services(doc: &serde_yaml_ng::Value) -> Vec<ComposeService> {
    let Some(services) = doc.get("services").and_then(|s| s.as_mapping()) else {
        return Vec::new();
    };
    services
        .iter()
        .filter_map(|(k, v)| {
            let name = k.as_str()?.to_string();
            Some(ComposeService {
                image: v.get("image").and_then(|i| i.as_str()).map(str::to_string),
                build: v.get("build").is_some(),
                ports: v
                    .get("ports")
                    .and_then(|p| p.as_sequence())
                    .map(|seq| seq.iter().filter_map(parse_port).collect())
                    .unwrap_or_default(),
                state: None,
                container: None,
                name,
            })
        })
        .collect()
}

/// `${OLS_SECRET_NAME}` / `${OLS_SECRET_NAME:?...}` variables used in a compose file.
pub fn secret_vars(text: &str) -> Vec<String> {
    let re = regex::Regex::new(r"\$\{(OLS_SECRET_[A-Z0-9_]+)").expect("valid regex");
    let mut vars: Vec<String> = re.captures_iter(text).map(|c| c[1].to_string()).collect();
    vars.sort();
    vars.dedup();
    vars
}

/// Keyring key for one secret of one compose file. Tied to the file's full path, so two
/// projects with the same service names never share a password.
pub fn secret_key(file: &Path, var: &str) -> String {
    format!("docker-compose:{}:{var}", path_hash(file))
}

pub(crate) fn path_hash(file: &Path) -> String {
    let normalized = file.to_string_lossy().replace('\\', "/").to_lowercase();
    let digest = Sha256::digest(normalized.as_bytes());
    digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

// ---------------------------------------------------------------- files

/// Refuses anything that is not an existing compose file of a sane size.
pub fn check_compose_path(path: &str) -> Result<PathBuf, CoreError> {
    let p = PathBuf::from(path);
    let name_ok = p
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| COMPOSE_FILE_NAMES.iter().any(|c| c.eq_ignore_ascii_case(n)));
    if !p.is_absolute() || !name_ok {
        return Err(CoreError::failed_fix(
            "That is not a compose file.",
            format!("\"{path}\" is not a full path to compose.yaml, compose.yml, docker-compose.yaml or docker-compose.yml."),
            "Open the file from the Compose tab on the Docker page.",
        ));
    }
    Ok(p)
}

pub fn read_file(path: &str) -> Result<String, CoreError> {
    let p = check_compose_path(path)?;
    let meta = std::fs::metadata(&p).map_err(|e| {
        CoreError::failed_fix(
            "The compose file could not be opened.",
            format!("{}: {e}", p.display()),
            "Check that the file still exists, then refresh the Compose tab.",
        )
    })?;
    if meta.len() > MAX_FILE {
        return Err(CoreError::failed_fix(
            "The compose file is too large to edit here.",
            format!(
                "{} is {} KB; the editor opens files up to 1 MB.",
                p.display(),
                meta.len() / 1024
            ),
            "Edit it in your code editor instead.",
        ));
    }
    std::fs::read_to_string(&p).map_err(|e| {
        CoreError::failed_fix(
            "The compose file could not be read.",
            format!("{}: {e}", p.display()),
            "Make sure it is a text file and OLS can read it.",
        )
    })
}

/// Checks a compose text without Docker: YAML syntax, `services`, images, ports.
pub fn check_text(text: &str) -> ComposeCheck {
    let mut check = ComposeCheck::default();
    let doc: serde_yaml_ng::Value = match serde_yaml_ng::from_str(text) {
        Ok(d) => d,
        Err(e) => {
            check.errors.push(format!("YAML syntax: {e}"));
            return check;
        }
    };
    let Some(services) = doc.get("services").and_then(|s| s.as_mapping()) else {
        check.errors.push(
            "There is no `services:` section. A compose file needs at least one service.".into(),
        );
        return check;
    };
    if services.is_empty() {
        check.errors.push("`services:` is empty.".into());
    }
    let mut host_ports: BTreeMap<u16, String> = BTreeMap::new();
    for (k, v) in services {
        let name = k.as_str().unwrap_or("?").to_string();
        let image = v.get("image").and_then(|i| i.as_str());
        if image.is_none() && v.get("build").is_none() {
            check.errors.push(format!(
                "Service `{name}` has neither `image:` nor `build:`."
            ));
        }
        if let Some(image) = image {
            let last = image.rsplit('/').next().unwrap_or(image);
            if !image.contains('@') && (!last.contains(':') || last.ends_with(":latest")) {
                check.warnings.push(format!(
                    "Service `{name}` uses `{image}` without a fixed version. Pin a tag (for example `:1.2`) so the stack does not change on the next pull."
                ));
            }
        }
        for port in v
            .get("ports")
            .and_then(|p| p.as_sequence())
            .into_iter()
            .flatten()
            .filter_map(parse_port)
        {
            if let Some(h) = port.host {
                if let Some(other) = host_ports.insert(h, name.clone()) {
                    check.errors.push(format!(
                        "Port {h} on this computer is used by both `{other}` and `{name}`."
                    ));
                }
            }
        }
        if let Some(env) = v.get("environment") {
            for (key, value) in env_pairs(env) {
                let k = key.to_uppercase();
                let looks_secret = ["PASSWORD", "SECRET", "TOKEN", "_KEY"]
                    .iter()
                    .any(|w| k.contains(w));
                if looks_secret && !value.is_empty() && !value.contains("${") {
                    check.warnings.push(format!(
                        "`{name}` has `{key}` written in the file. Anyone with the project folder can read it; use a variable such as `${{OLS_SECRET_{}}}` so OLS keeps it in the Windows credential store.",
                        k.replace(|c: char| !c.is_ascii_alphanumeric(), "_")
                    ));
                }
            }
        }
    }
    check.ok = check.errors.is_empty();
    check
}

fn env_pairs(env: &serde_yaml_ng::Value) -> Vec<(String, String)> {
    use serde_yaml_ng::Value;
    match env {
        Value::Mapping(m) => m
            .iter()
            .filter_map(|(k, v)| {
                let value = match v {
                    Value::String(s) => s.clone(),
                    Value::Number(n) => n.to_string(),
                    Value::Bool(b) => b.to_string(),
                    _ => String::new(),
                };
                Some((k.as_str()?.to_string(), value))
            })
            .collect(),
        Value::Sequence(seq) => seq
            .iter()
            .filter_map(|v| v.as_str())
            .filter_map(|s| {
                s.split_once('=')
                    .map(|(k, v)| (k.to_string(), v.to_string()))
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Saves `text` over `path`, keeping the previous version under `backups_dir`.
pub fn save_file(path: &str, text: &str, backups_dir: &Path) -> Result<(), CoreError> {
    let p = check_compose_path(path)?;
    let check = check_text(text);
    if !check.ok {
        return Err(CoreError::failed_fix(
            "The compose file was not saved.",
            check.errors.join(" "),
            "Fix the problems shown under the editor, then save again.",
        ));
    }
    let io = |what: &str, e: std::io::Error| {
        CoreError::failed_fix(
            "The compose file could not be saved.",
            format!("{what}: {e}"),
            "Check that the project folder is writable and the file is not open elsewhere.",
        )
    };
    if p.is_file() {
        let dir = backups_dir.join(path_hash(&p));
        std::fs::create_dir_all(&dir).map_err(|e| io("creating the backup folder", e))?;
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S-%3f");
        std::fs::copy(&p, dir.join(format!("{stamp}.yaml"))).map_err(|e| io("backing up", e))?;
        prune_backups(&dir);
    }
    let tmp = p.with_extension("ols-tmp");
    std::fs::write(&tmp, text).map_err(|e| io("writing", e))?;
    std::fs::rename(&tmp, &p).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        io("replacing the file", e)
    })
}

fn prune_backups(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    files.sort();
    while files.len() > KEEP_BACKUPS {
        let _ = std::fs::remove_file(files.remove(0));
    }
}

// ---------------------------------------------------------------- discovery

/// Stacks from compose files in project folders, merged with stacks the engine reports
/// (by their `com.docker.compose.*` labels). `containers` is empty when Docker is off.
pub fn discover(
    projects: &[crate::project::Project],
    containers: &[ContainerSummary],
) -> Vec<ComposeStack> {
    let mut stacks: Vec<ComposeStack> = Vec::new();
    for project in projects {
        let dir = Path::new(&project.path);
        let Some(file) = COMPOSE_FILE_NAMES
            .iter()
            .map(|n| dir.join(n))
            .find(|f| f.is_file())
        else {
            continue;
        };
        stacks.push(stack_from_file(&file, Some(project)));
    }
    // Running stacks whose file is not in a project folder.
    let mut engine_projects: BTreeMap<String, Option<String>> = BTreeMap::new();
    for c in containers {
        if let Some(name) = &c.compose_project {
            let file = c.compose_file.clone();
            engine_projects.entry(name.clone()).or_insert(file);
        }
    }
    for (name, file) in engine_projects {
        if stacks.iter().any(|s| s.name == name) {
            continue;
        }
        let from_file = file
            .as_deref()
            .map(Path::new)
            .filter(|f| f.is_file())
            .map(|f| stack_from_file(f, None));
        stacks.push(match from_file {
            Some(mut s) => {
                s.name = name;
                s
            }
            None => ComposeStack {
                name,
                file,
                dir: None,
                project_id: None,
                project_name: None,
                services: Vec::new(),
                running: 0,
                file_error: None,
                uses_secrets: false,
                connected: false,
            },
        });
    }
    // Attach containers to their services.
    for stack in &mut stacks {
        for c in containers
            .iter()
            .filter(|c| c.compose_project.as_deref() == Some(stack.name.as_str()))
        {
            let service = c.compose_service.clone().unwrap_or_else(|| c.name.clone());
            match stack.services.iter_mut().find(|s| s.name == service) {
                Some(s) => {
                    s.state = Some(c.state.clone());
                    s.container = Some(c.name.clone());
                }
                None => stack.services.push(ComposeService {
                    name: service,
                    image: Some(c.image.clone()),
                    build: false,
                    ports: c
                        .ports
                        .iter()
                        .map(|p| ComposePort {
                            host: p.host_port,
                            container: p.container_port,
                        })
                        .collect(),
                    state: Some(c.state.clone()),
                    container: Some(c.name.clone()),
                }),
            }
        }
        stack.running = stack
            .services
            .iter()
            .filter(|s| s.state.as_deref() == Some("running"))
            .count();
    }
    stacks.sort_by_key(|s| (s.running == 0, s.name.clone()));
    stacks
}

fn stack_from_file(file: &Path, project: Option<&crate::project::Project>) -> ComposeStack {
    let (doc, file_error, uses_secrets) = match std::fs::read_to_string(file) {
        Ok(text) => {
            let secrets = !secret_vars(&text).is_empty();
            match serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&text) {
                Ok(d) => (Some(d), None, secrets),
                Err(e) => (None, Some(format!("YAML syntax: {e}")), secrets),
            }
        }
        Err(e) => (None, Some(e.to_string()), false),
    };
    ComposeStack {
        name: project_name(file, doc.as_ref()),
        file: Some(file.display().to_string()),
        dir: file.parent().map(|d| d.display().to_string()),
        project_id: project.map(|p| p.id.clone()),
        project_name: project.map(|p| p.name.clone()),
        services: doc.as_ref().map(parse_services).unwrap_or_default(),
        running: 0,
        file_error,
        uses_secrets,
        connected: false,
    }
}

// ---------------------------------------------------------------- CLI

/// Environment for `docker compose`: every `${OLS_SECRET_*}` the file uses, from the keyring.
/// A variable with no stored value is reported, not silently left empty.
pub fn secret_env(file: &Path, text: &str) -> Result<Vec<(String, String)>, CoreError> {
    let mut env = Vec::new();
    let mut missing = Vec::new();
    for var in secret_vars(text) {
        match crate::secrets::get_secret(&secret_key(file, &var)) {
            Ok(Some(v)) => env.push((var, v)),
            Ok(None) => missing.push(var),
            Err(e) => {
                return Err(CoreError::failed_fix(
                    "OLS could not read this stack's passwords.",
                    format!("The Windows credential store answered: {e}"),
                    "Sign in to Windows normally (not as a temporary profile) and try again.",
                ))
            }
        }
    }
    if !missing.is_empty() {
        return Err(CoreError::failed_fix(
            "This stack needs passwords OLS does not have.",
            format!("No stored value for {}.", missing.join(", ")),
            "Open the file and replace these variables with your own values, or recreate the stack with the compose builder.",
        ));
    }
    Ok(env)
}

fn cli() -> Result<PathBuf, CoreError> {
    super::find_cli().ok_or_else(|| {
        CoreError::failed_fix(
            "The docker command was not found.",
            "Compose stacks run through the `docker compose` command, and `docker` is not on PATH.",
            "Install Docker Desktop (it adds `docker` to PATH), then restart OLS.",
        )
    })
}

/// "Connect to OLS" for one run: where to write the generated override, and what goes in it.
#[derive(Debug, Clone)]
pub struct Connect {
    pub data_dir: PathBuf,
    pub inputs: super::connect::ConnectInputs,
}

/// `compose -f <file> [-f <override>] --project-directory <dir>`. The project directory is
/// always the file's own folder, so the override (kept in OLS's data folder) never changes
/// how relative paths in the user's file resolve.
fn base_args(file: &Path, extra: Option<&Path>) -> Vec<String> {
    let dir = file.parent().unwrap_or(Path::new("."));
    let mut args = vec!["compose".into(), "-f".into(), file.display().to_string()];
    if let Some(extra) = extra {
        args.extend(["-f".into(), extra.display().to_string()]);
    }
    args.extend(["--project-directory".into(), dir.display().to_string()]);
    args
}

/// Writes this run's override from `text` (the services being run or checked).
fn override_for(
    file: &Path,
    text: &str,
    connect: Option<&Connect>,
) -> Result<Option<PathBuf>, CoreError> {
    connect
        .map(|c| super::connect::write_override(&c.data_dir, file, text, &c.inputs))
        .transpose()
}

/// `docker compose config --quiet` on `text` (unsaved editor content is checked through
/// a temporary copy in the same folder, so relative paths resolve the same way).
pub fn docker_config_check(
    file: &Path,
    text: &str,
    connect: Option<&Connect>,
) -> Option<Result<(), String>> {
    let exe = super::find_cli()?;
    let extra = match override_for(file, text, connect) {
        Ok(e) => e,
        Err(e) => return Some(Err(crate::error::Diagnostic::from(&e).cause)),
    };
    let env = secret_env(file, text).unwrap_or_default();
    let dir = file.parent().unwrap_or(Path::new("."));
    let tmp = dir.join(format!(".ols-check-{}.yaml", std::process::id()));
    if let Err(e) = std::fs::write(&tmp, text) {
        return Some(Err(format!("could not write a temporary copy: {e}")));
    }
    let mut args = base_args(file, extra.as_deref());
    args[2] = tmp.display().to_string();
    args.extend(["config".into(), "--quiet".into()]);
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let out = crate::exec::run_streaming(
        &exe,
        &args,
        Some(dir),
        &env,
        CONFIG_TIMEOUT,
        &cancel,
        |_| {},
    );
    let _ = std::fs::remove_file(&tmp);
    Some(if out.exit_code == Some(0) {
        Ok(())
    } else if out.timed_out {
        Err("`docker compose config` did not finish within 30 s".into())
    } else {
        let text = out.combined();
        Err(text
            .replace(&tmp.display().to_string(), &file.display().to_string())
            .trim()
            .to_string())
    })
}

/// Full check: local rules, then Docker's own parser when the CLI is present.
pub fn validate(
    path: &str,
    text: &str,
    connect: Option<&Connect>,
) -> Result<ComposeCheck, CoreError> {
    let file = check_compose_path(path)?;
    let mut check = check_text(text);
    if check.ok {
        if let Some(result) = docker_config_check(&file, text, connect) {
            check.docker_checked = true;
            if let Err(msg) = result {
                check.errors.push(format!("Docker says: {msg}"));
                check.ok = false;
            }
        }
    }
    Ok(check)
}

/// A line that reads like an error (always shown, and preferred in failure summaries).
fn looks_like_error(line: &str) -> bool {
    const MARKERS: &[&str] = &[
        "error",
        "no such",
        "denied",
        "not found",
        "failed",
        "invalid",
        "conflict",
        "unable",
        "cannot",
    ];
    let l = line.to_lowercase();
    MARKERS.iter().any(|m| l.contains(m))
}

/// The lines that explain a failed run. Output and errors arrive on two streams read by
/// separate threads, so an error line can land anywhere in the captured text (a burst of
/// buffered output can be read after it). Prefer the last lines that read like errors
/// anywhere in the output, and fall back to the last 3 lines.
fn failure_summary(output: &str) -> String {
    let lines: Vec<&str> = output.lines().filter(|l| !l.trim().is_empty()).collect();

    let errors: Vec<&str> = lines
        .iter()
        .copied()
        .filter(|l| looks_like_error(l))
        .collect();
    let picked = if errors.is_empty() {
        &lines[..]
    } else {
        &errors[..]
    };
    picked[picked.len().saturating_sub(3)..].join(" ")
}

/// Valid compose service name.
pub fn valid_service(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

/// Runs one action, streaming output into the task. Runs on the task's own thread.
pub fn run_action(
    path: &str,
    action: ComposeAction,
    service: Option<&str>,
    task: &crate::tasks::TaskHandle,
    connect: Option<&Connect>,
) -> Result<(), CoreError> {
    let file = check_compose_path(path)?;
    if let Some(s) = service {
        if !valid_service(s) {
            return Err(CoreError::failed_fix(
                "That is not a valid service name.",
                format!("\"{s}\" contains characters compose does not allow."),
                "Pick the service from the stack's list.",
            ));
        }
    }
    let exe = cli()?;
    let text = read_file(path)?;
    let env = secret_env(&file, &text)?;
    let extra = override_for(&file, &text, connect)?;
    let mut args = base_args(&file, extra.as_deref());
    args.extend(action.args().iter().map(|a| a.to_string()));
    if let Some(s) = service {
        args.push(s.to_string());
    }
    task.begin_step(&format!("docker compose {}", action.args().join(" ")));
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let mut shown = 0usize;
    let mut tail: std::collections::VecDeque<String> = std::collections::VecDeque::new();
    let mut hidden = 0usize;
    let dir = file.parent().map(Path::to_path_buf);
    let out = std::thread::scope(|scope| {
        // Mirror the task's cancel flag into the process runner.
        let watcher = scope.spawn(|| {
            while !cancel.load(std::sync::atomic::Ordering::Relaxed) {
                if task.cancel_requested() {
                    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                    break;
                }
                std::thread::sleep(Duration::from_millis(200));
            }
        });
        let out = crate::exec::run_streaming(
            &exe,
            &args,
            dir.as_deref(),
            &env,
            ACTION_TIMEOUT,
            &cancel,
            |line| {
                let line = crate::redact::redact_text(line);
                if line.trim().is_empty() {
                    return;
                }
                if shown < LIVE_LINES {
                    shown += 1;
                    task.result(line);
                } else if looks_like_error(&line) {
                    // Never hide an error among the skipped lines.
                    task.result(line);
                } else {
                    hidden += 1;
                    tail.push_back(line);
                    if tail.len() > TAIL_LINES {
                        tail.pop_front();
                    }
                }
            },
        );
        cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = watcher.join();
        out
    });
    if hidden > 0 {
        let skipped = hidden.saturating_sub(tail.len());
        if skipped > 0 {
            task.result(format!("… {skipped} more lines …"));
        }
        for line in tail {
            task.result(line);
        }
    }
    if task.cancel_requested() {
        return Err(CoreError::failed("Stopped", "Stopped by the user."));
    }
    if out.exit_code == Some(0) {
        task.end_step(None);
        return Ok(());
    }
    let last = failure_summary(&out.combined());
    Err(CoreError::failed_fix(
        format!("docker compose {} failed.", action.args()[0]),
        if out.timed_out {
            "It did not finish within an hour.".to_string()
        } else {
            crate::redact::redact_text(&last)
        },
        "Read the output above. A port already in use, a typo in the file, or Docker not running are the usual causes.",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_yaml_ng::Value;

    fn port(s: &str) -> Option<ComposePort> {
        parse_port(&Value::String(s.into()))
    }

    #[test]
    fn port_forms() {
        assert_eq!(
            port("8080:80"),
            Some(ComposePort {
                host: Some(8080),
                container: 80
            })
        );
        assert_eq!(
            port("127.0.0.1:8080:80/tcp"),
            Some(ComposePort {
                host: Some(8080),
                container: 80
            })
        );
        assert_eq!(
            port("80"),
            Some(ComposePort {
                host: None,
                container: 80
            })
        );
        assert_eq!(port("${PORT:-8080}:80").map(|p| p.host), Some(None));
        let long: Value = serde_yaml_ng::from_str("{target: 80, published: \"8081\"}").unwrap();
        assert_eq!(
            parse_port(&long),
            Some(ComposePort {
                host: Some(8081),
                container: 80
            })
        );
    }

    #[test]
    fn names_follow_compose_rules() {
        let f = Path::new("/x/My Shop!/compose.yaml");
        assert_eq!(project_name(f, None), "myshop");
        let doc: Value = serde_yaml_ng::from_str("name: Billing_App\nservices: {}").unwrap();
        assert_eq!(project_name(f, Some(&doc)), "billing_app");
    }

    #[test]
    fn check_finds_errors_and_warnings() {
        let c = check_text("services:\n  web:\n    image: nginx\n    ports: ['8080:80']\n  api:\n    image: node:22\n    ports: ['8080:3000']\n    environment:\n      DB_PASSWORD: hunter2\n  broken: {}\n");
        assert!(!c.ok);
        assert!(c.errors.iter().any(|e| e.contains("8080")), "{c:?}");
        assert!(c.errors.iter().any(|e| e.contains("broken")), "{c:?}");
        assert!(c.warnings.iter().any(|w| w.contains("nginx")), "{c:?}");
        assert!(
            c.warnings.iter().any(|w| w.contains("DB_PASSWORD")),
            "{c:?}"
        );

        let ok = check_text("services:\n  db:\n    image: mariadb:11.4\n    environment:\n      MARIADB_PASSWORD: ${OLS_SECRET_DB_PASSWORD:?x}\n");
        assert!(ok.ok && ok.warnings.is_empty(), "{ok:?}");
        assert!(!check_text("services: [").ok);
        assert!(!check_text("version: '3'\n").ok);
    }

    #[test]
    fn secret_vars_are_found_once() {
        let vars = secret_vars(
            "a: ${OLS_SECRET_A:?x}\nb: ${OLS_SECRET_A}\nc: ${OLS_SECRET_B_1}\nd: ${OTHER}",
        );
        assert_eq!(vars, vec!["OLS_SECRET_A", "OLS_SECRET_B_1"]);
        let k1 = secret_key(Path::new("C:\\Sites\\a\\compose.yaml"), "OLS_SECRET_A");
        let k2 = secret_key(Path::new("c:/sites/a/compose.yaml"), "OLS_SECRET_A");
        assert_eq!(k1, k2, "same file, different spelling");
        assert_ne!(
            k1,
            secret_key(Path::new("C:\\Sites\\b\\compose.yaml"), "OLS_SECRET_A")
        );
    }

    #[test]
    fn only_compose_files_are_accepted() {
        let tmp = tempfile::tempdir().unwrap();
        let good = tmp.path().join("compose.yaml");
        assert!(check_compose_path(&good.display().to_string()).is_ok());
        assert!(check_compose_path(&tmp.path().join(".env").display().to_string()).is_err());
        assert!(
            check_compose_path("compose.yaml").is_err(),
            "relative paths refused"
        );
    }

    #[test]
    fn failure_summary_prefers_error_lines() {
        let mut out: Vec<String> = (1..=50).map(|i| format!("line {i}")).collect();
        // Anywhere in the output, not only near the end.
        out.insert(3, "Error response from daemon: no such image".into());
        assert_eq!(
            failure_summary(&out.join("\n")),
            "Error response from daemon: no such image"
        );
        assert_eq!(failure_summary("a\nb\nc\nd"), "b c d");
        assert_eq!(failure_summary(""), "");
    }

    #[test]
    fn save_keeps_backups_and_refuses_broken_yaml() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("compose.yaml");
        let backups = tmp.path().join("backups");
        let path = file.display().to_string();
        std::fs::write(&file, "services:\n  a:\n    image: redis:7.4\n").unwrap();
        save_file(&path, "services:\n  b:\n    image: redis:7.4\n", &backups).unwrap();
        assert!(read_file(&path).unwrap().contains("  b:"));
        let backup_dir = std::fs::read_dir(&backups)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let saved: Vec<_> = std::fs::read_dir(&backup_dir).unwrap().collect();
        assert_eq!(saved.len(), 1);
        assert!(save_file(&path, "services: [", &backups).is_err());
        assert!(
            read_file(&path).unwrap().contains("  b:"),
            "broken text never replaces the file"
        );
    }

    #[test]
    fn discovery_merges_files_and_running_stacks() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("shop");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("compose.yaml"),
            "services:\n  web:\n    image: nginx:1.28\n    ports: ['8080:80']\n  db:\n    image: mariadb:11.4\n",
        )
        .unwrap();
        let project: crate::project::Project = serde_json::from_value(serde_json::json!({
            "id": "p1", "name": "Shop", "path": dir.display().to_string()
        }))
        .unwrap();
        let running = ContainerSummary {
            id: "c1".into(),
            name: "shop-web-1".into(),
            image: "nginx:1.28".into(),
            state: "running".into(),
            status: "Up".into(),
            created: 0,
            ports: vec![],
            compose_project: Some("shop".into()),
            compose_service: Some("web".into()),
            compose_file: None,
            ols_site: None,
            managed: false,
        };
        let other = ContainerSummary {
            id: "c2".into(),
            name: "other-cache-1".into(),
            compose_project: Some("other".into()),
            compose_service: Some("cache".into()),
            ..running.clone()
        };
        let stacks = discover(&[project], &[running, other]);
        assert_eq!(stacks.len(), 2);
        let shop = stacks.iter().find(|s| s.name == "shop").unwrap();
        assert_eq!(shop.project_id.as_deref(), Some("p1"));
        assert_eq!(shop.running, 1);
        assert_eq!(shop.services.len(), 2);
        let web = shop.services.iter().find(|s| s.name == "web").unwrap();
        assert_eq!(web.state.as_deref(), Some("running"));
        assert_eq!(
            web.ports,
            vec![ComposePort {
                host: Some(8080),
                container: 80
            }]
        );
        let other = stacks.iter().find(|s| s.name == "other").unwrap();
        assert!(other.file.is_none());
        assert_eq!(other.services[0].name, "cache");
    }
}

/// `run_action` against a fake `docker` on PATH (Unix only: the fake is a shell script).
#[cfg(all(test, unix))]
mod action_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn actions_stream_output_and_report_failures() {
        let _lock = crate::test_support::isolated_home();
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let fake = bin.join("docker");
        std::fs::write(
            &fake,
            "#!/bin/sh\necho \"ARGS $*\"\nfor i in $(seq 1 400); do echo \"line $i\"; done\ncase \"$*\" in *pull*) echo 'no such image' >&2; exit 1;; esac\nexit 0\n",
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let old_path = std::env::var_os("PATH").unwrap_or_default();
        let mut paths = vec![bin.clone()];
        paths.extend(std::env::split_paths(&old_path));
        std::env::set_var("PATH", std::env::join_paths(paths).unwrap());

        let dir = tmp.path().join("shop");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("compose.yaml");
        std::fs::write(&file, "services:\n  web:\n    image: nginx:1.28\n").unwrap();
        let path = file.display().to_string();

        let tasks = crate::tasks::TaskManager::new();
        let run = |action, service: Option<&str>| {
            let (p, s) = (path.clone(), service.map(str::to_string));
            let id = tasks.start("t", "t", &format!("{p}{s:?}{action:?}"), 1, move |h| {
                let r = run_action(&p, action, s.as_deref(), &h, None);
                crate::command::report(&h, r);
            });
            let deadline = std::time::Instant::now() + Duration::from_secs(20);
            while tasks.is_running(&id) && std::time::Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
            tasks.get(&id).unwrap()
        };

        let up = run(ComposeAction::Up, Some("web"));
        let connect = Connect {
            data_dir: tmp.path().join("data"),
            inputs: super::super::connect::ConnectInputs {
                hostnames: vec!["shop.test".into()],
                ca_file: None,
            },
        };
        let connected = {
            let (p, c) = (path.clone(), connect.clone());
            let id = tasks.start("t", "t", "connected", 1, move |h| {
                let r = run_action(&p, ComposeAction::Restart, None, &h, Some(&c));
                crate::command::report(&h, r);
            });
            let deadline = std::time::Instant::now() + Duration::from_secs(20);
            while tasks.is_running(&id) && std::time::Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
            tasks.get(&id).unwrap()
        };
        std::env::set_var("PATH", &old_path);
        assert!(up.error.is_none(), "{:?}", up.error);
        let first = &up.results[0];
        assert!(
            first.contains("compose -f") && first.contains("--project-directory"),
            "{first}"
        );
        assert!(first.ends_with("up -d web"), "{first}");
        // 300 live lines, a "more lines" marker, then the tail.
        assert!(
            up.results.iter().any(|l| l.contains("more lines")),
            "{:?}",
            up.results.len()
        );
        assert_eq!(up.results.last().map(String::as_str), Some("line 400"));
        // Connected: the generated override is a second -f, before --project-directory.
        let args = &connected.results[0];
        let override_file = super::super::connect::override_path(&connect.data_dir, &file);
        assert!(
            args.contains(&format!(
                "-f {} -f {} --project-directory",
                file.display(),
                override_file.display()
            )),
            "{args}"
        );
        assert!(std::fs::read_to_string(&override_file)
            .unwrap()
            .contains("shop.test:host-gateway"));

        std::env::set_var(
            "PATH",
            std::env::join_paths(
                [bin.clone()]
                    .into_iter()
                    .chain(std::env::split_paths(&old_path)),
            )
            .unwrap(),
        );
        let pull = run(ComposeAction::Pull, None);
        std::env::set_var("PATH", &old_path);
        let err = pull.error.expect("pull fails");
        assert!(err.problem.contains("pull"), "{err:?}");
        assert!(
            err.cause.contains("no such image"),
            "{err:?}\nlast results: {:?}\nresults with error: {:?}",
            &pull.results[pull.results.len().saturating_sub(5)..],
            pull.results
                .iter()
                .filter(|l| l.contains("no such"))
                .collect::<Vec<_>>()
        );

        let bad = run(ComposeAction::Up, Some("web; rm -rf /"));
        assert!(bad.error.is_some());
    }
}
