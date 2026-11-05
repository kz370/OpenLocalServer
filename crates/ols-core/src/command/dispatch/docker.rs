//! §166: Docker page commands. Thin wrappers over `crate::docker::DockerManager`.

use super::super::types::*;
use super::super::Core;
use crate::error::CoreError;

impl Core {
    pub(crate) fn dispatch_docker(
        &self,
        command: CoreCommand,
    ) -> Result<Result<CoreResponse, CoreError>, CoreCommand> {
        use CoreCommand as C;
        use CoreResponse as R;
        let i = &self.inner;
        Ok(match command {
            C::GetDockerStatus => Ok(R::DockerStatus {
                status: i.docker.status(&i.docker_endpoint()),
            }),
            C::ListDockerContainers { all } => i
                .docker
                .list_containers(&i.docker_endpoint(), all)
                .map(|containers| R::DockerContainers { containers }),
            C::DockerContainerAction { id, action } => {
                tracing::info!(command = "docker_container_action", id = %id, ?action);
                i.docker
                    .container_action(&i.docker_endpoint(), &id, action)
                    .map(|_| R::Ok)
            }
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
            other => return Err(other),
        })
    }
}
