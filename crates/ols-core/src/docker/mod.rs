//! Docker manager (§166): see Docker's status and control containers from the Docker page.
//!
//! Performance rules (plan §2.5, binding):
//! - Creating the manager does no I/O. Nothing here runs at app startup; the first engine
//!   request happens on the first Docker command (the user opening the Docker page).
//! - Every engine call has a hard timeout and runs off the caller's thread (`client`).
//! - Results are cached briefly so repeated UI calls do not repeat engine requests.
//! - No background polling, timers or open connections.

pub mod builder;
pub mod client;
pub mod compose;
pub mod connect;
pub mod details;
pub mod presets;
pub mod sites;
pub mod types;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::error::CoreError;
use client::{ClientError, Endpoint};
pub use types::{ContainerAction, ContainerSummary, DockerStatus, PortBinding};

/// Setting key: overrides where the engine is (`npipe://`, `unix://`, `tcp://`).
pub const SETTING_ENDPOINT: &str = "docker.endpoint";

const STATUS_TIMEOUT: Duration = Duration::from_secs(2);
const LIST_TIMEOUT: Duration = Duration::from_secs(3);
/// `stop` waits up to 10 s for the container to exit before Docker kills it.
const ACTION_TIMEOUT: Duration = Duration::from_secs(20);
/// How long a container list is reused before asking the engine again.
const LIST_CACHE: Duration = Duration::from_secs(2);
const LOGS_TIMEOUT: Duration = Duration::from_secs(5);
/// How long "Start Docker" waits for the engine to answer.
const START_WAIT: Duration = Duration::from_secs(120);

struct CachedList {
    at: Instant,
    endpoint: Endpoint,
    all: bool,
    containers: Vec<ContainerSummary>,
}

/// Shared Docker state. Cheap to create; holds only a short-lived response cache.
#[derive(Default)]
pub struct DockerManager {
    containers: Mutex<Option<CachedList>>,
    /// Engine requests made in this session (tests assert none happen at startup).
    requests: AtomicU64,
}

impl DockerManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Engine requests made so far in this session.
    pub fn request_count(&self) -> u64 {
        self.requests.load(Ordering::Relaxed)
    }

    fn call(
        &self,
        endpoint: &Endpoint,
        method: &str,
        path: &str,
        timeout: Duration,
    ) -> Result<client::Response, ClientError> {
        self.call_body(endpoint, method, path, None, timeout)
    }

    fn call_body(
        &self,
        endpoint: &Endpoint,
        method: &str,
        path: &str,
        body: Option<Vec<u8>>,
        timeout: Duration,
    ) -> Result<client::Response, ClientError> {
        self.requests.fetch_add(1, Ordering::Relaxed);
        client::request(endpoint, method, path, body, timeout)
    }

    /// Asks the engine for its version. Never fails: an unreachable engine is a status,
    /// not an error, so the Docker page can show "stopped" with a Start button.
    pub fn status(&self, endpoint: &Endpoint) -> DockerStatus {
        let desktop_path = find_desktop().map(|p| p.display().to_string());
        let cli = find_cli().is_some();
        let mut status = DockerStatus {
            installed: desktop_path.is_some() || cli,
            endpoint: endpoint.display(),
            desktop_path,
            ..Default::default()
        };
        match self
            .call(endpoint, "GET", "/version", STATUS_TIMEOUT)
            .and_then(|r| r.json::<types::EngineVersion>())
        {
            Ok(v) => {
                status.running = true;
                // An engine that answers is installed, whatever the file checks found.
                status.installed = true;
                status.version = v.version;
                status.api_version = v.api_version;
                status.os_type = v.os;
            }
            Err(e) => {
                status.message = Some(match e {
                    ClientError::NotRunning(_) if !status.installed => {
                        "Docker is not installed on this computer.".into()
                    }
                    ClientError::NotRunning(_) => "Docker is installed but not running.".into(),
                    ClientError::Timeout(_) => {
                        "Docker did not answer in time. It may still be starting.".into()
                    }
                    other => diagnostic_cause(&other),
                });
            }
        }
        status
    }

    /// Lists containers (`all` includes stopped ones). Reuses a list younger than 2 s.
    pub fn list_containers(
        &self,
        endpoint: &Endpoint,
        all: bool,
    ) -> Result<Vec<ContainerSummary>, CoreError> {
        if let Some(cached) = self.lock_cache().as_ref() {
            if cached.all == all && &cached.endpoint == endpoint && cached.at.elapsed() < LIST_CACHE
            {
                return Ok(cached.containers.clone());
            }
        }
        let path = if all {
            "/containers/json?all=1"
        } else {
            "/containers/json"
        };
        let raw: Vec<types::EngineContainer> = self
            .call(endpoint, "GET", path, LIST_TIMEOUT)
            .and_then(|r| r.json())
            .map_err(|e| to_core_error(e, "list Docker containers"))?;
        let mut containers: Vec<ContainerSummary> = raw.into_iter().map(Into::into).collect();
        containers.sort_by(|a, b| {
            (a.compose_project.is_none(), &a.compose_project, &a.name).cmp(&(
                b.compose_project.is_none(),
                &b.compose_project,
                &b.name,
            ))
        });
        *self.lock_cache() = Some(CachedList {
            at: Instant::now(),
            endpoint: endpoint.clone(),
            all,
            containers: containers.clone(),
        });
        Ok(containers)
    }

    /// Starts, stops, restarts, pauses or unpauses one container.
    pub fn container_action(
        &self,
        endpoint: &Endpoint,
        id: &str,
        action: ContainerAction,
    ) -> Result<(), CoreError> {
        if !types::valid_container_ref(id) {
            return Err(CoreError::failed_fix(
                "That is not a valid container name.",
                format!("\"{id}\" contains characters Docker never uses in a name or id."),
                "Pick the container from the list on the Docker page.",
            ));
        }
        let path = format!("/containers/{id}/{}", action.path_segment());
        let result = self.call(endpoint, "POST", &path, ACTION_TIMEOUT);
        // Whatever happened, the next list must ask the engine again.
        *self.lock_cache() = None;
        result
            .map(|_| ())
            .map_err(|e| to_core_error(e, &format!("{} the container", action.path_segment())))
    }

    /// Launches Docker Desktop, then waits until the engine answers (up to 2 minutes),
    /// reporting through the given task handle. Runs on the task's own thread.
    pub fn start_desktop(
        &self,
        endpoint: &Endpoint,
        task: &crate::tasks::TaskHandle,
    ) -> Result<(), CoreError> {
        if self.status(endpoint).running {
            return Ok(());
        }
        let exe = find_desktop().ok_or_else(|| {
            CoreError::failed_fix(
                "Docker Desktop was not found.",
                "OLS looked in Program Files and found no \"Docker Desktop.exe\".",
                "Install Docker Desktop from docker.com, then press Start Docker again.",
            )
        })?;
        task.begin_step("Launching Docker Desktop");
        let mut cmd = std::process::Command::new(&exe);
        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        cmd.spawn().map_err(|e| {
            CoreError::failed_fix(
                "Docker Desktop could not be started.",
                format!("Starting \"{}\" failed: {e}", exe.display()),
                "Start Docker Desktop from the Start menu.",
            )
        })?;
        task.end_step(None);
        task.begin_step("Waiting for Docker to answer");
        let deadline = Instant::now() + START_WAIT;
        loop {
            if task.cancel_requested() {
                return Err(CoreError::failed("Stopped", "Stopped by the user."));
            }
            if self.status(endpoint).running {
                task.end_step(None);
                *self.lock_cache() = None;
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(CoreError::failed_fix(
                    "Docker did not start in time.",
                    "Docker Desktop was launched, but its engine did not answer within 2 minutes.",
                    "Open Docker Desktop and check for a message (for example, WSL 2 needs an update), then refresh the Docker page.",
                ));
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    }

    /// The last `tail` log lines of a container (stdout and stderr, oldest first), with
    /// secrets redacted. At most 2,000 lines.
    pub fn container_logs(
        &self,
        endpoint: &Endpoint,
        id: &str,
        tail: u32,
    ) -> Result<Vec<String>, CoreError> {
        if !types::valid_container_ref(id) {
            return Err(CoreError::failed_fix(
                "That is not a valid container name.",
                format!("\"{id}\" contains characters Docker never uses in a name or id."),
                "Pick the container from the list on the Docker page.",
            ));
        }
        let tail = tail.clamp(1, 2000);
        let path = format!("/containers/{id}/logs?stdout=1&stderr=1&timestamps=1&tail={tail}");
        let r = self
            .call(endpoint, "GET", &path, LOGS_TIMEOUT)
            .map_err(|e| to_core_error(e, "read the container's logs"))?;
        Ok(split_log_stream(&r.body)
            .iter()
            .map(|l| crate::redact::redact_text(l))
            .collect())
    }

    /// Container settings for the dialog (`docker inspect`), with password-like values removed.
    pub fn container_details(
        &self,
        endpoint: &Endpoint,
        id: &str,
    ) -> Result<details::ContainerDetails, CoreError> {
        check_ref(id)?;
        let v: serde_json::Value = self
            .call(
                endpoint,
                "GET",
                &format!("/containers/{id}/json"),
                LIST_TIMEOUT,
            )
            .and_then(|r| r.json())
            .map_err(|e| to_core_error(e, "read the container's settings"))?;
        Ok(details::parse(&v))
    }

    /// Changes a running container's restart policy and limits (no recreate needed).
    pub fn update_container(
        &self,
        endpoint: &Endpoint,
        id: &str,
        update: &details::ContainerUpdate,
    ) -> Result<(), CoreError> {
        check_ref(id)?;
        let body = details::update_body(update).map_err(|cause| {
            CoreError::failed_fix(
                "Those settings cannot be applied.",
                cause,
                "Correct the value and save again.",
            )
        })?;
        let r = self.call_body(
            endpoint,
            "POST",
            &format!("/containers/{id}/update"),
            Some(serde_json::to_vec(&body).unwrap_or_default()),
            ACTION_TIMEOUT,
        );
        *self.lock_cache() = None;
        r.map(|_| ())
            .map_err(|e| to_core_error(e, "update the container"))
    }

    /// Renames a container.
    pub fn rename_container(
        &self,
        endpoint: &Endpoint,
        id: &str,
        name: &str,
    ) -> Result<(), CoreError> {
        check_ref(id)?;
        if !types::valid_container_ref(name) || name.len() > 64 {
            return Err(CoreError::failed_fix(
                "That name cannot be used.",
                format!("\"{name}\" must start with a letter or digit and use only letters, digits, `_`, `.` and `-`."),
                "Type a simple name such as shop-db.",
            ));
        }
        let r = self.call(
            endpoint,
            "POST",
            &format!("/containers/{id}/rename?name={name}"),
            ACTION_TIMEOUT,
        );
        *self.lock_cache() = None;
        r.map(|_| ())
            .map_err(|e| to_core_error(e, "rename the container"))
    }

    /// Removes a container. `force` stops a running one first; `volumes` also removes its
    /// anonymous volumes (named volumes are never removed here).
    pub fn remove_container(
        &self,
        endpoint: &Endpoint,
        id: &str,
        force: bool,
        volumes: bool,
    ) -> Result<(), CoreError> {
        check_ref(id)?;
        let r = self.call(
            endpoint,
            "DELETE",
            &format!("/containers/{id}?force={force}&v={volumes}"),
            ACTION_TIMEOUT,
        );
        *self.lock_cache() = None;
        r.map(|_| ())
            .map_err(|e| to_core_error(e, "remove the container"))
    }

    /// Recreates a (non-Compose) container with a new environment, the way Portainer does:
    /// set the old one aside under a temporary name, create the new one under the real
    /// name, stop the old one and start the new one, then remove the old one. Any failure
    /// after the old container was touched puts it back exactly as it was.
    /// Runs on a task thread; returns the new container's id.
    pub fn recreate_with_env(
        &self,
        endpoint: &Endpoint,
        id: &str,
        edits: &[details::EnvEdit],
        task: &crate::tasks::TaskHandle,
    ) -> Result<String, CoreError> {
        check_ref(id)?;
        let fail = |what: &str, e: ClientError| to_core_error(e, what);
        task.begin_step("Read the container's settings");
        let v: serde_json::Value = self
            .call(
                endpoint,
                "GET",
                &format!("/containers/{id}/json"),
                LIST_TIMEOUT,
            )
            .and_then(|r| r.json())
            .map_err(|e| fail("read the container's settings", e))?;
        let plan = details::recreate_plan(&v, edits).map_err(|cause| {
            CoreError::failed_fix(
                "The environment was not changed.",
                cause,
                "Correct the variables, or edit the compose file for a Compose container.",
            )
        })?;
        let name = v["Name"]
            .as_str()
            .unwrap_or_default()
            .trim_start_matches('/')
            .to_string();
        let old_id = v["Id"].as_str().unwrap_or_default().to_string();
        let was_running = v["State"]["Running"].as_bool().unwrap_or(false);
        if !types::valid_container_ref(&name) || !types::valid_container_ref(&old_id) {
            return Err(CoreError::failed(
                "The container could not be recreated.",
                "Docker reported an unexpected name or id.",
            ));
        }
        task.end_step(None);

        let backup = format!("{name}-ols-old-{}", chrono::Utc::now().timestamp());
        task.begin_step("Set the old container aside");
        self.call(
            endpoint,
            "POST",
            &format!("/containers/{old_id}/rename?name={backup}"),
            ACTION_TIMEOUT,
        )
        .map_err(|e| fail("set the old container aside", e))?;
        task.end_step(Some(backup.clone()));

        // From here on, failures restore the old container.
        let restore = |new_id: Option<&str>, start_old: bool| {
            if let Some(n) = new_id {
                let _ = self.call(
                    endpoint,
                    "DELETE",
                    &format!("/containers/{n}?force=true&v=false"),
                    ACTION_TIMEOUT,
                );
            }
            let _ = self.call(
                endpoint,
                "POST",
                &format!("/containers/{old_id}/rename?name={name}"),
                ACTION_TIMEOUT,
            );
            if start_old {
                let _ = self.call(
                    endpoint,
                    "POST",
                    &format!("/containers/{old_id}/start"),
                    ACTION_TIMEOUT,
                );
            }
            *self.lock_cache() = None;
        };

        task.begin_step("Create the new container");
        let created = self
            .call_body(
                endpoint,
                "POST",
                &format!("/containers/create?name={name}"),
                Some(serde_json::to_vec(&plan.create_body).unwrap_or_default()),
                ACTION_TIMEOUT,
            )
            .and_then(|r| r.json::<serde_json::Value>());
        let new_id = match created {
            Ok(r) => r["Id"].as_str().unwrap_or_default().to_string(),
            Err(e) => {
                restore(None, false);
                return Err(fail("create the new container (the old one was kept)", e));
            }
        };
        if !types::valid_container_ref(&new_id) {
            restore(None, false);
            return Err(CoreError::failed(
                "The container could not be recreated.",
                "Docker did not return the new container's id; the old one was kept.",
            ));
        }
        for (net, aliases) in &plan.extra_networks {
            if !types::valid_container_ref(net) {
                task.problem(format!(
                    "Network \"{net}\" was not joined: its name has unusual characters."
                ));
                continue;
            }
            let body = serde_json::json!({ "Container": new_id, "EndpointConfig": { "Aliases": aliases } });
            if let Err(e) = self.call_body(
                endpoint,
                "POST",
                &format!("/networks/{net}/connect"),
                Some(serde_json::to_vec(&body).unwrap_or_default()),
                ACTION_TIMEOUT,
            ) {
                restore(Some(&new_id), false);
                return Err(fail(
                    &format!("join network {net} (the old container was kept)"),
                    e,
                ));
            }
        }
        task.end_step(None);

        if was_running {
            task.begin_step("Stop the old container and start the new one");
            if let Err(e) = self.call(
                endpoint,
                "POST",
                &format!("/containers/{old_id}/stop"),
                ACTION_TIMEOUT,
            ) {
                restore(Some(&new_id), false);
                return Err(fail("stop the old container (nothing was changed)", e));
            }
            if let Err(e) = self.call(
                endpoint,
                "POST",
                &format!("/containers/{new_id}/start"),
                ACTION_TIMEOUT,
            ) {
                restore(Some(&new_id), true);
                return Err(fail(
                    "start the new container (the old one was restarted)",
                    e,
                ));
            }
            task.end_step(None);
        }

        task.begin_step("Remove the old container");
        // v=false: its volumes stay; the new container already uses them.
        if let Err(e) = self.call(
            endpoint,
            "DELETE",
            &format!("/containers/{old_id}?v=false"),
            ACTION_TIMEOUT,
        ) {
            task.problem(format!(
                "The old container \"{backup}\" could not be removed ({}); remove it from the Docker page.",
                diagnostic_cause(&e)
            ));
        }
        task.end_step(None);
        task.result(format!(
            "{name} now runs with {} environment variables.",
            plan.env.len()
        ));
        *self.lock_cache() = None;
        Ok(new_id)
    }

    fn lock_cache(&self) -> std::sync::MutexGuard<'_, Option<CachedList>> {
        self.containers.lock().unwrap_or_else(|p| p.into_inner())
    }
}

fn check_ref(id: &str) -> Result<(), CoreError> {
    if types::valid_container_ref(id) {
        Ok(())
    } else {
        Err(CoreError::failed_fix(
            "That is not a valid container name.",
            format!("\"{id}\" contains characters Docker never uses in a name or id."),
            "Pick the container from the list on the Docker page.",
        ))
    }
}

/// `docker exec -it <container> sh -c ...`: bash when the image has it, sh otherwise.
pub fn shell_program(container: &str) -> Result<(std::path::PathBuf, Vec<String>), CoreError> {
    check_ref(container)?;
    let exe = find_cli().ok_or_else(|| {
        CoreError::failed_fix(
            "The docker command was not found.",
            "A container shell runs `docker exec`, and `docker` is not on PATH.",
            "Install Docker Desktop (it adds `docker` to PATH), then restart OLS.",
        )
    })?;
    Ok((
        exe,
        vec![
            "exec".into(),
            "-it".into(),
            container.into(),
            "sh".into(),
            "-c".into(),
            "if command -v bash >/dev/null 2>&1; then exec bash; else exec sh; fi".into(),
        ],
    ))
}

/// The endpoint to use: the `docker.endpoint` setting, then `DOCKER_HOST`, then the
/// platform default. An unparsable value falls through to the next source.
pub fn resolve_endpoint(setting: Option<&str>) -> Endpoint {
    setting
        .and_then(Endpoint::parse)
        .or_else(|| {
            std::env::var("DOCKER_HOST")
                .ok()
                .and_then(|v| Endpoint::parse(&v))
        })
        .unwrap_or_else(Endpoint::platform_default)
}

/// Docker Desktop's program file, from the usual install folders.
pub fn find_desktop() -> Option<std::path::PathBuf> {
    if !cfg!(windows) {
        return None;
    }
    ["ProgramFiles", "ProgramW6432", "LOCALAPPDATA"]
        .iter()
        .filter_map(std::env::var_os)
        .map(|base| {
            std::path::Path::new(&base)
                .join("Docker")
                .join("Docker")
                .join("Docker Desktop.exe")
        })
        .find(|p| p.is_file())
}

/// The `docker` command on PATH (a file check only; nothing is run).
pub fn find_cli() -> Option<std::path::PathBuf> {
    let name = if cfg!(windows) {
        "docker.exe"
    } else {
        "docker"
    };
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(name))
            .find(|p| p.is_file())
    })
}

/// Docker sends logs of a container without a terminal as frames:
/// `[stream, 0, 0, 0, len (4 bytes, big-endian)] + payload`. A container with a terminal
/// sends plain text. Both become lines.
pub(crate) fn split_log_stream(body: &[u8]) -> Vec<String> {
    let framed = body.len() >= 8 && matches!(body[0], 0..=2) && body[1..4] == [0, 0, 0];
    let mut text = Vec::with_capacity(body.len());
    if framed {
        let mut rest = body;
        while rest.len() >= 8 {
            let len = u32::from_be_bytes([rest[4], rest[5], rest[6], rest[7]]) as usize;
            let end = (8 + len).min(rest.len());
            text.extend_from_slice(&rest[8..end]);
            rest = &rest[end..];
        }
    } else {
        text.extend_from_slice(body);
    }
    String::from_utf8_lossy(&text)
        .lines()
        .map(|l| l.trim_end_matches('\r').to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

fn diagnostic_cause(e: &ClientError) -> String {
    match e {
        ClientError::NotRunning(at) => format!("Nothing is listening at {at}."),
        ClientError::Timeout(t) => format!("Docker did not answer within {} s.", t.as_secs()),
        ClientError::Http { status, message } => format!("Docker answered {status}: {message}"),
        ClientError::Protocol(why) => why.clone(),
    }
}

/// Turns a client failure into the user-facing problem / cause / fix shape.
fn to_core_error(e: ClientError, doing: &str) -> CoreError {
    let cause = diagnostic_cause(&e);
    match e {
        ClientError::NotRunning(_) => CoreError::failed_fix(
            "Docker is not running.",
            cause,
            "Start Docker Desktop (Docker page → Start Docker), then try again.",
        ),
        ClientError::Timeout(_) => CoreError::failed_fix(
            format!("Docker took too long to {doing}."),
            cause,
            "Docker may still be starting. Wait a moment and refresh; if it stays stuck, restart Docker Desktop.",
        ),
        ClientError::Http { status: 404, .. } => CoreError::failed_fix(
            format!("Could not {doing}: the container no longer exists."),
            cause,
            "Refresh the Docker page; the container may have been removed.",
        ),
        ClientError::Http { status: 409, .. } => CoreError::failed_fix(
            format!("Could not {doing} right now."),
            cause,
            "The container is in a state that does not allow this. Refresh and try again.",
        ),
        ClientError::Http { .. } => CoreError::failed_fix(
            format!("Docker refused to {doing}."),
            cause,
            "Read Docker's message above. If it mentions a port, free that port or change the container's port.",
        ),
        ClientError::Protocol(_) => CoreError::failed_fix(
            format!("Could not {doing}."),
            cause,
            "Restart Docker Desktop. If the problem stays, check the Docker endpoint setting.",
        ),
    }
}

#[cfg(test)]
mod tests;
