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
pub mod storage;
pub mod types;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::error::CoreError;
use client::{ClientError, Endpoint};
pub use types::{ContainerAction, ContainerSummary, DockerState, DockerStatus, PortBinding};

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
/// `/system/df` is slow on a big engine; reused for a minute.
const DF_CACHE: Duration = Duration::from_secs(60);
const DF_TIMEOUT: Duration = Duration::from_secs(60);
const PRUNE_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const PULL_TIMEOUT: Duration = Duration::from_secs(60 * 60);

#[derive(Default)]
pub struct DockerManager {
    containers: Mutex<Option<CachedList>>,
    disk: Mutex<Option<(Instant, Endpoint, serde_json::Value)>>,
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
        let mut status = DockerStatus {
            endpoint: endpoint.display(),
            ..Default::default()
        };
        match self
            .call(endpoint, "GET", "/version", STATUS_TIMEOUT)
            .and_then(|r| r.json::<types::EngineVersion>())
        {
            Ok(v) => {
                status.running = true;
                status.state = DockerState::Running;
                // An engine that answers is installed, whatever the file checks found.
                status.installed = true;
                status.version = v.version;
                status.api_version = v.api_version;
                status.os_type = v.os;
            }
            Err(e) => {
                // File checks (PATH walk, Program Files) only run when the engine is down.
                status.desktop_path = find_desktop().map(|p| p.display().to_string());
                status.installed = status.desktop_path.is_some() || find_cli().is_some();
                status.state = match (&e, status.installed) {
                    (ClientError::NotRunning(_), true) => DockerState::Stopped,
                    (ClientError::NotRunning(_), false) => DockerState::NotInstalled,
                    _ => DockerState::Error,
                };
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

    /// `/system/df`, reused for a minute unless `fresh`.
    fn system_df(&self, endpoint: &Endpoint, fresh: bool) -> Result<serde_json::Value, CoreError> {
        if !fresh {
            if let Some((at, ep, v)) = self.disk.lock().unwrap_or_else(|p| p.into_inner()).as_ref()
            {
                if ep == endpoint && at.elapsed() < DF_CACHE {
                    return Ok(v.clone());
                }
            }
        }
        let v: serde_json::Value = self
            .call(endpoint, "GET", "/system/df", DF_TIMEOUT)
            .and_then(|r| r.json())
            .map_err(|e| to_core_error(e, "read Docker's disk usage"))?;
        *self.disk.lock().unwrap_or_else(|p| p.into_inner()) =
            Some((Instant::now(), endpoint.clone(), v.clone()));
        Ok(v)
    }

    fn forget_storage(&self) {
        *self.lock_cache() = None;
        *self.disk.lock().unwrap_or_else(|p| p.into_inner()) = None;
    }

    pub fn disk_usage(
        &self,
        endpoint: &Endpoint,
        fresh: bool,
    ) -> Result<storage::DiskUsage, CoreError> {
        Ok(storage::parse_disk_usage(&self.system_df(endpoint, fresh)?))
    }

    pub fn list_images(
        &self,
        endpoint: &Endpoint,
    ) -> Result<Vec<storage::ImageSummary>, CoreError> {
        let containers = self.list_containers(endpoint, true)?;
        let v: serde_json::Value = self
            .call(endpoint, "GET", "/images/json", LIST_TIMEOUT)
            .and_then(|r| r.json())
            .map_err(|e| to_core_error(e, "list images"))?;
        Ok(storage::parse_images(&v, &containers))
    }

    /// Volumes; sizes are filled in only when disk usage was read in the last minute.
    pub fn list_volumes(
        &self,
        endpoint: &Endpoint,
    ) -> Result<Vec<storage::VolumeSummary>, CoreError> {
        let containers = self.list_containers(endpoint, true)?;
        let v: serde_json::Value = self
            .call(endpoint, "GET", "/volumes", LIST_TIMEOUT)
            .and_then(|r| r.json())
            .map_err(|e| to_core_error(e, "list volumes"))?;
        let sizes = self
            .disk
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .filter(|(at, ep, _)| ep == endpoint && at.elapsed() < DF_CACHE)
            .map(|(_, _, df)| storage::parse_disk_usage(df).volume_sizes)
            .unwrap_or_default();
        Ok(storage::parse_volumes(&v, &containers, &sizes))
    }

    pub fn remove_image(
        &self,
        endpoint: &Endpoint,
        id: &str,
        force: bool,
    ) -> Result<(), CoreError> {
        if !storage::valid_image_ref(id) {
            return Err(CoreError::failed_fix(
                "That is not an image name.",
                format!("\"{id}\" has characters Docker never uses in an image name."),
                "Pick the image from the Images tab.",
            ));
        }
        let r = self.call(
            endpoint,
            "DELETE",
            &format!("/images/{id}?force={force}"),
            ACTION_TIMEOUT,
        );
        self.forget_storage();
        r.map(|_| ())
            .map_err(|e| to_core_error(e, "remove the image"))
    }

    pub fn remove_volume(&self, endpoint: &Endpoint, name: &str) -> Result<(), CoreError> {
        if !storage::valid_volume_name(name) {
            return Err(CoreError::failed_fix(
                "That is not a volume name.",
                format!("\"{name}\" has characters Docker never uses in a volume name."),
                "Pick the volume from the Volumes tab.",
            ));
        }
        let r = self.call(
            endpoint,
            "DELETE",
            &format!("/volumes/{name}"),
            ACTION_TIMEOUT,
        );
        self.forget_storage();
        r.map(|_| ())
            .map_err(|e| to_core_error(e, "remove the volume"))
    }

    /// The dry run for Reclaim space, from fresh disk usage.
    pub fn prune_plan(
        &self,
        endpoint: &Endpoint,
        targets: &storage::PruneTargets,
    ) -> Result<storage::PrunePlan, CoreError> {
        Ok(storage::prune_plan(
            &self.system_df(endpoint, true)?,
            targets,
        ))
    }

    /// Runs the cleanup the user confirmed; reports what each step freed.
    pub fn prune(
        &self,
        endpoint: &Endpoint,
        targets: &storage::PruneTargets,
        task: &crate::tasks::TaskHandle,
    ) -> Result<u64, CoreError> {
        // `{"dangling":["false"]}` and `{"all":["true"]}`, URL-encoded.
        const ALL_IMAGES: &str = "?filters=%7B%22dangling%22%3A%5B%22false%22%5D%7D";
        const ALL_VOLUMES: &str = "?filters=%7B%22all%22%3A%5B%22true%22%5D%7D";
        let mut steps: Vec<(&str, String)> = Vec::new();
        if targets.stopped_containers {
            steps.push(("Remove stopped containers", "/containers/prune".into()));
        }
        if targets.unused_images {
            steps.push(("Remove unused images", format!("/images/prune{ALL_IMAGES}")));
        } else if targets.dangling_images {
            steps.push(("Remove untagged images", "/images/prune".into()));
        }
        if targets.unused_volumes {
            steps.push((
                "Remove unused volumes",
                format!("/volumes/prune{ALL_VOLUMES}"),
            ));
        }
        if targets.build_cache {
            steps.push(("Remove unused build cache", "/build/prune".into()));
        }
        if steps.is_empty() {
            return Err(CoreError::failed_fix(
                "Nothing was cleaned.",
                "No kind of item was chosen.",
                "Tick what to clean, then try again.",
            ));
        }
        let mut freed = 0u64;
        for (label, path) in steps {
            if task.cancel_requested() {
                break;
            }
            task.begin_step(label);
            let r = self
                .call(endpoint, "POST", &path, PRUNE_TIMEOUT)
                .and_then(|r| r.json::<serde_json::Value>());
            match r {
                Ok(v) => {
                    let space = v["SpaceReclaimed"].as_u64().unwrap_or(0);
                    freed += space;
                    task.end_step(Some(format!("{} MB freed", space / (1024 * 1024))));
                }
                Err(e) => {
                    task.fail_step(diagnostic_cause(&e));
                    task.problem(format!("{label}: {}", diagnostic_cause(&e)));
                }
            }
        }
        self.forget_storage();
        task.result(format!("Freed about {} MB.", freed / (1024 * 1024)));
        Ok(freed)
    }

    /// Creates and starts a container from an image. Returns its id.
    pub fn run_image(
        &self,
        endpoint: &Endpoint,
        spec: &storage::RunSpec,
    ) -> Result<String, CoreError> {
        let body = storage::run_body(spec).map_err(|cause| {
            CoreError::failed_fix(
                "The container was not created.",
                cause,
                "Correct the value and try again.",
            )
        })?;
        let path = match spec
            .name
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty())
        {
            Some(n) => format!("/containers/create?name={n}"),
            None => "/containers/create".into(),
        };
        let created: serde_json::Value = self
            .call_body(
                endpoint,
                "POST",
                &path,
                Some(serde_json::to_vec(&body).unwrap_or_default()),
                ACTION_TIMEOUT,
            )
            .and_then(|r| r.json())
            .map_err(|e| to_core_error(e, "create the container"))?;
        let id = created["Id"].as_str().unwrap_or_default().to_string();
        check_ref(&id)?;
        let started = self.call(
            endpoint,
            "POST",
            &format!("/containers/{id}/start"),
            ACTION_TIMEOUT,
        );
        self.forget_storage();
        started.map_err(|e| {
            to_core_error(
                e,
                "start the new container (it was created; start it from the Containers tab)",
            )
        })?;
        Ok(id)
    }

    /// `docker pull <reference>` with its progress as task output.
    pub fn pull_image(
        &self,
        reference: &str,
        task: &crate::tasks::TaskHandle,
    ) -> Result<(), CoreError> {
        if !storage::valid_image_ref(reference) {
            return Err(CoreError::failed_fix(
                "That is not an image name.",
                format!("\"{reference}\" is not like nginx:1.28 or ghcr.io/org/app:1.0."),
                "Type the image name with its tag.",
            ));
        }
        let exe = find_cli().ok_or_else(|| {
            CoreError::failed_fix(
                "The docker command was not found.",
                "Pulling uses the `docker` command, and it is not on PATH.",
                "Install Docker Desktop (it adds `docker` to PATH), then restart OLS.",
            )
        })?;
        task.begin_step(&format!("docker pull {reference}"));
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let mut last = String::new();
        let out = std::thread::scope(|scope| {
            let watcher = scope.spawn(|| {
                while !cancel.load(Ordering::Relaxed) {
                    if task.cancel_requested() {
                        cancel.store(true, Ordering::Relaxed);
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(200));
                }
            });
            let out = crate::exec::run_streaming(
                &exe,
                &["pull".into(), reference.into()],
                None,
                &[],
                PULL_TIMEOUT,
                &cancel,
                |line| {
                    // Layer progress lines repeat; show each distinct line once.
                    if !line.trim().is_empty() && line != last {
                        task.result(line.to_string());
                        last = line.to_string();
                    }
                },
            );
            cancel.store(true, Ordering::Relaxed);
            let _ = watcher.join();
            out
        });
        self.forget_storage();
        if out.exit_code == Some(0) {
            task.end_step(None);
            Ok(())
        } else if task.cancel_requested() {
            Err(CoreError::failed("Stopped", "Stopped by the user."))
        } else {
            Err(CoreError::failed_fix(
                format!("Pulling {reference} failed."),
                out.combined().lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("docker pull failed").to_string(),
                "Check the name and tag, your internet connection, and that you are signed in to a private registry.",
            ))
        }
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
