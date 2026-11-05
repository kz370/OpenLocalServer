//! §166: Docker page commands. Thin wrappers over `crate::docker`.

use super::super::types::*;
use super::super::Core;
use crate::error::CoreError;

impl Core {
    pub(crate) fn dispatch_docker(
        &self,
        command: CoreCommand,
    ) -> Result<Result<CoreResponse, CoreError>, CoreCommand> {
        use crate::docker::{builder, compose};
        use CoreCommand as C;
        use CoreResponse as R;
        let i = &self.inner;
        let backups = || i.paths.data_dir().join("docker").join("backups");
        Ok(match command {
            C::GetDockerStatus => Ok(R::DockerStatus {
                status: i.docker.status(&i.docker_endpoint()),
            }),
            C::ListDockerContainers { all } => (|| {
                let mut containers = i.docker.list_containers(&i.docker_endpoint(), all)?;
                // Sites follow their container to a new port; re-apply only when one moved.
                let moved = i.sync_docker_sites(&containers);
                if !moved.is_empty() {
                    let inner = i.clone();
                    i.tasks.start(
                        "docker-site-sync",
                        "Update Docker sites to new ports",
                        "docker-sites",
                        1,
                        move |h| {
                            h.begin_step(&format!("Re-apply web config for {}", moved.join(", ")));
                            let r = inner.apply_web(&[]).map(|_| ());
                            crate::command::report(&h, r);
                        },
                    );
                }
                i.mark_docker_sites(&mut containers);
                Ok(R::DockerContainers { containers })
            })(),
            C::DockerContainerAction { id, action } => {
                tracing::info!(command = "docker_container_action", id = %id, ?action);
                i.docker
                    .container_action(&i.docker_endpoint(), &id, action)
                    .map(|_| R::Ok)
            }
            C::GetDockerContainerLogs { id, tail } => i
                .docker
                .container_logs(&i.docker_endpoint(), &id, tail.unwrap_or(500))
                .map(|lines| R::DockerLogs { lines }),
            C::StartDockerDesktop => {
                let inner = i.clone();
                let id = i.tasks.start(
                    "docker-start",
                    "Start Docker Desktop",
                    "docker",
                    2,
                    move |h| {
                        let endpoint = inner.docker_endpoint();
                        let r = inner.docker.start_desktop(&endpoint, &h);
                        crate::command::report(&h, r);
                    },
                );
                Ok(R::TaskStarted { id })
            }
            C::ListDockerComposeStacks => {
                let projects = i.projects.lock().unwrap().list();
                // Docker being off only means no running stacks are shown.
                let containers = i
                    .docker
                    .list_containers(&i.docker_endpoint(), true)
                    .unwrap_or_default();
                Ok(R::DockerComposeStacks {
                    stacks: compose::discover(&projects, &containers),
                })
            }
            C::ReadDockerComposeFile { path } => {
                compose::read_file(&path).map(|content| R::DockerComposeFile { path, content })
            }
            C::ValidateDockerCompose { path, content } => {
                compose::validate(&path, &content).map(|check| R::DockerComposeCheck { check })
            }
            C::SaveDockerComposeFile { path, content } => {
                tracing::info!(command = "save_docker_compose_file", path = %path);
                compose::save_file(&path, &content, &backups()).map(|_| R::Ok)
            }
            C::DockerComposeAction {
                path,
                action,
                service,
            } => (|| {
                let file = compose::check_compose_path(&path)?;
                tracing::info!(command = "docker_compose_action", path = %path, ?action, ?service);
                let stack = file
                    .parent()
                    .and_then(|d| d.file_name())
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "stack".into());
                let title = match &service {
                    Some(s) => format!("{} · {stack} / {s}", action.label()),
                    None => format!("{} · {stack}", action.label()),
                };
                let target = format!("{path}#{}", service.clone().unwrap_or_default());
                let id = i
                    .tasks
                    .start("docker-compose", &title, &target, 1, move |h| {
                        let r = compose::run_action(&path, action, service.as_deref(), &h);
                        crate::command::report(&h, r);
                    });
                Ok(R::TaskStarted { id })
            })(),
            C::ListDockerComposePresets => Ok(R::DockerComposePresets {
                presets: builder::presets(),
            }),
            C::PlanDockerCompose { spec } => builder::plan(&spec, &crate::port::port_is_free)
                .map(|plan| R::DockerComposePlan { plan }),
            C::CreateDockerCompose { spec, overwrite } => {
                tracing::info!(command = "create_docker_compose", dir = %spec.dir, overwrite);
                builder::create(
                    &spec,
                    overwrite,
                    &backups(),
                    &crate::port::port_is_free,
                    &crate::secrets::set_secret,
                )
                .map(|plan| R::DockerComposePlan { plan })
            }
            C::RevealDockerComposeSecrets { path } => (|| {
                let text = compose::read_file(&path)?;
                let file = compose::check_compose_path(&path)?;
                Ok(R::DockerComposeSecrets {
                    secrets: compose::secret_env(&file, &text)?,
                })
            })(),
            C::DockerCreateSite { request } => {
                tracing::info!(command = "docker_create_site", container = %request.container, hostname = %request.hostname);
                i.docker_create_site(&request).map(|domain| R::Domain {
                    domain: Box::new(domain),
                })
            }
            other => return Err(other),
        })
    }
}
