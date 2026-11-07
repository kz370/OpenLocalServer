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
            C::ListDockerImages => i
                .docker
                .list_images(&i.docker_endpoint())
                .map(|images| R::DockerImages { images }),
            C::ListDockerVolumes => i
                .docker
                .list_volumes(&i.docker_endpoint())
                .map(|volumes| R::DockerVolumes { volumes }),
            C::GetDockerDiskUsage { fresh } => i
                .docker
                .disk_usage(&i.docker_endpoint(), fresh)
                .map(|usage| R::DockerDiskUsage { usage }),
            C::RemoveDockerImage { id, force } => {
                tracing::info!(command = "remove_docker_image", id = %id, force);
                i.docker
                    .remove_image(&i.docker_endpoint(), &id, force)
                    .map(|_| R::Ok)
            }
            C::RemoveDockerVolume { name } => {
                tracing::info!(command = "remove_docker_volume", name = %name);
                i.docker
                    .remove_volume(&i.docker_endpoint(), &name)
                    .map(|_| R::Ok)
            }
            C::PlanDockerPrune { targets } => i
                .docker
                .prune_plan(&i.docker_endpoint(), &targets)
                .map(|plan| R::DockerPrunePlan { plan }),
            C::PruneDocker { targets } => {
                tracing::info!(command = "prune_docker", ?targets);
                let inner = i.clone();
                let id = i.tasks.start(
                    "docker-prune",
                    "Reclaim Docker disk space",
                    "docker",
                    4,
                    move |h| {
                        let ep = inner.docker_endpoint();
                        let r = inner.docker.prune(&ep, &targets, &h);
                        crate::command::report(&h, r);
                    },
                );
                Ok(R::TaskStarted { id })
            }
            C::PullDockerImage { reference } => {
                tracing::info!(command = "pull_docker_image", reference = %reference);
                let inner = i.clone();
                let title = format!("Pull {reference}");
                let target = reference.clone();
                let id = i.tasks.start("docker-pull", &title, &target, 1, move |h| {
                    let r = inner.docker.pull_image(&reference, &h);
                    crate::command::report(&h, r);
                });
                Ok(R::TaskStarted { id })
            }
            C::RunDockerImage { spec } => {
                tracing::info!(command = "run_docker_image", image = %spec.image);
                i.docker
                    .run_image(&i.docker_endpoint(), &spec)
                    .map(|id| R::DockerRun { id })
            }
            C::OpenDockerShell { id, rows, cols } => (|| {
                tracing::info!(command = "open_docker_shell", id = %id);
                let program = crate::docker::shell_program(&id)?;
                let tid = i
                    .terminals
                    .open(crate::terminal::TerminalSpec {
                        shell: None,
                        program: Some(program),
                        cwd: None,
                        env: Vec::new(),
                        rows,
                        cols,
                    })
                    .map_err(|e| {
                        CoreError::failed_fix(
                            "The container shell could not be opened.",
                            e,
                            "Check that the container is running, then try again.",
                        )
                    })?;
                Ok(R::Terminal { id: tid })
            })(),
            C::GetDockerContainer { id } => i
                .docker
                .container_details(&i.docker_endpoint(), &id)
                .map(|details| R::DockerContainerDetails { details }),
            C::UpdateDockerContainer { id, update } => (|| {
                tracing::info!(command = "update_docker_container", id = %id, ?update);
                let ep = i.docker_endpoint();
                i.docker.update_container(&ep, &id, &update)?;
                Ok(R::DockerContainerDetails {
                    details: i.docker.container_details(&ep, &id)?,
                })
            })(),
            C::RenameDockerContainer { id, name } => (|| {
                tracing::info!(command = "rename_docker_container", id = %id, name = %name);
                let ep = i.docker_endpoint();
                let old = i.docker.container_details(&ep, &id)?.name;
                i.docker.rename_container(&ep, &id, &name)?;
                i.docker_relink_sites(&old, &name);
                Ok(R::Ok)
            })(),
            C::RecreateDockerContainer { id, env } => {
                tracing::info!(command = "recreate_docker_container", id = %id, vars = env.len());
                let inner = i.clone();
                let title = format!("Update environment · {id}");
                let target = id.clone();
                let tid = i
                    .tasks
                    .start("docker-recreate", &title, &target, 5, move |h| {
                        let ep = inner.docker_endpoint();
                        let r = inner.docker.recreate_with_env(&ep, &id, &env, &h);
                        crate::command::report(&h, r);
                    });
                Ok(R::TaskStarted { id: tid })
            }
            C::RemoveDockerContainer { id, force, volumes } => {
                tracing::info!(command = "remove_docker_container", id = %id, force, volumes);
                i.docker
                    .remove_container(&i.docker_endpoint(), &id, force, volumes)
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
                let connected = i.docker_connected();
                let mut stacks = compose::discover(&projects, &containers);
                for s in &mut stacks {
                    s.connected = s.file.as_deref().is_some_and(|f| {
                        connected.contains(&compose::path_hash(std::path::Path::new(f)))
                    });
                }
                Ok(R::DockerComposeStacks { stacks })
            }
            C::ReadDockerComposeFile { path } => {
                compose::read_file(&path).map(|content| R::DockerComposeFile { path, content })
            }
            C::ValidateDockerCompose { path, content } => (|| {
                let file = compose::check_compose_path(&path)?;
                let connect = i.docker_connect_for(&file);
                // Docker's check writes a temporary copy beside the file: only in a stack
                // folder. Any other file still gets every local rule.
                let known = i.docker_compose_is_known(&file);
                compose::validate(&path, &content, connect.as_ref(), known)
                    .map(|check| R::DockerComposeCheck { check })
            })(),
            C::SaveDockerComposeFile { path, content } => (|| {
                tracing::info!(command = "save_docker_compose_file", path = %path);
                let file = compose::check_compose_path(&path)?;
                if !i.docker_compose_is_known(&file) {
                    return Err(compose::not_a_known_stack(&file));
                }
                compose::save_file(&path, &content, &backups()).map(|_| R::Ok)
            })(),
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
                let inner = i.clone();
                let id = i
                    .tasks
                    .start("docker-compose", &title, &target, 1, move |h| {
                        // Site names and the CA are read now, so a run always has the current ones.
                        let connect = inner.docker_connect_for(&file);
                        let r = compose::run_action(
                            &path,
                            action,
                            service.as_deref(),
                            &h,
                            connect.as_ref(),
                        );
                        crate::command::report(&h, r);
                    });
                Ok(R::TaskStarted { id })
            })(),
            C::ListDockerComposePresets => Ok(R::DockerComposePresets {
                presets: crate::docker::presets::catalog(&i.docker_custom_presets()),
            }),
            C::SaveDockerComposePreset {
                preset,
                previous_id,
            } => (|| {
                tracing::info!(command = "save_docker_compose_preset", name = %preset.name);
                i.save_docker_preset(preset, previous_id.as_deref())?;
                Ok(R::DockerComposePresets {
                    presets: crate::docker::presets::catalog(&i.docker_custom_presets()),
                })
            })(),
            C::DeleteDockerComposePreset { id } => (|| {
                tracing::info!(command = "delete_docker_compose_preset", id = %id);
                i.delete_docker_preset(&id)?;
                Ok(R::DockerComposePresets {
                    presets: crate::docker::presets::catalog(&i.docker_custom_presets()),
                })
            })(),
            C::PlanDockerCompose { spec } => builder::plan_with(
                &spec,
                &crate::docker::presets::catalog(&i.docker_custom_presets()),
                &crate::port::port_is_free,
            )
            .map(|plan| R::DockerComposePlan { plan }),
            C::CreateDockerCompose { spec, overwrite } => (|| {
                tracing::info!(command = "create_docker_compose", dir = %spec.dir, overwrite);
                let plan = builder::create_with(
                    &spec,
                    &crate::docker::presets::catalog(&i.docker_custom_presets()),
                    overwrite,
                    &backups(),
                    &crate::port::port_is_free,
                    &crate::secrets::set_secret,
                )?;
                // Stacks made by OLS start connected to OLS.
                i.set_docker_connected(std::path::Path::new(&plan.file), true)?;
                Ok(R::DockerComposePlan { plan })
            })(),
            C::GetDockerComposeConnect { path } => (|| {
                let file = compose::check_compose_path(&path)?;
                Ok(R::DockerComposeConnect {
                    info: crate::docker::connect::info(
                        &path,
                        i.docker_is_connected(&file),
                        &i.docker_connect_inputs(),
                        i.docker_ols_endpoints(),
                    )?,
                })
            })(),
            C::SetDockerComposeConnect { path, enabled } => (|| {
                let file = compose::check_compose_path(&path)?;
                tracing::info!(command = "set_docker_compose_connect", path = %path, enabled);
                i.set_docker_connected(&file, enabled)?;
                Ok(R::DockerComposeConnect {
                    info: crate::docker::connect::info(
                        &path,
                        enabled,
                        &i.docker_connect_inputs(),
                        i.docker_ols_endpoints(),
                    )?,
                })
            })(),
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
