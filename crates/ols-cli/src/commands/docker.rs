//! `olsc docker …`: the safe parts of the Docker page (§167). Everything goes through a
//! `CoreCommand`; nothing here talks to Docker or runs the `docker` program. Removing
//! containers, images or volumes, recreating, running commands inside a container and
//! cleaning up for real stay in the app.

use crate::args::{ComposeCmd, ComposeTarget, ContainerArg, DockerCmd, DockerizeModeArg};
use crate::ctx::{table, Ctx, R};
use ols_core::command::{CoreCommand, CoreResponse};
use ols_core::docker::compose::ComposeAction;
use ols_core::docker::dockerize::{DockerizeMode, DockerizeOptions};
use ols_core::docker::storage::PruneTargets;
use ols_core::docker::ContainerAction;
use ols_core::tasks::TaskState;
use std::time::{Duration, Instant};

pub fn docker(ctx: &Ctx, cmd: DockerCmd) -> R<()> {
    match cmd {
        DockerCmd::Status => status(ctx),
        DockerCmd::Ps { all } => ps(ctx, all),
        DockerCmd::Start(c) => container_action(ctx, c, ContainerAction::Start, "started"),
        DockerCmd::Stop(c) => container_action(ctx, c, ContainerAction::Stop, "stopped"),
        DockerCmd::Restart(c) => container_action(ctx, c, ContainerAction::Restart, "restarted"),
        DockerCmd::Pause(c) => container_action(ctx, c, ContainerAction::Pause, "paused"),
        DockerCmd::Unpause(c) => container_action(ctx, c, ContainerAction::Unpause, "unpaused"),
        DockerCmd::Logs { container, tail } => logs(ctx, container, tail),
        DockerCmd::Images => images(ctx),
        DockerCmd::Volumes => volumes(ctx),
        DockerCmd::Df { fresh } => df(ctx, fresh),
        DockerCmd::Pull { image } => {
            let r = ctx.call(CoreCommand::PullDockerImage { reference: image })?;
            follow(ctx, r)
        }
        DockerCmd::Compose(c) => compose(ctx, c),
        DockerCmd::Dockerize {
            project,
            mode,
            services,
            create,
            replace_compose,
            show_files,
        } => dockerize(
            ctx,
            &project,
            mode,
            services,
            create,
            replace_compose,
            show_files,
        ),
        DockerCmd::Prune {
            stopped,
            dangling,
            unused_images,
            build_cache,
            ..
        } => prune_plan(
            ctx,
            PruneTargets {
                stopped_containers: stopped,
                dangling_images: dangling,
                unused_images,
                unused_volumes: false,
                build_cache,
            },
        ),
    }
}

fn unexpected() -> String {
    "unexpected reply".into()
}

fn status(ctx: &Ctx) -> R<()> {
    let r = ctx.call(CoreCommand::GetDockerStatus)?;
    if ctx.print_json(&r) {
        return Ok(());
    }
    let CoreResponse::DockerStatus { status } = r else {
        return Err(unexpected());
    };
    let state = if status.running {
        format!(
            "running (Docker {}, API {})",
            status.version.as_deref().unwrap_or("?"),
            status.api_version.as_deref().unwrap_or("?")
        )
    } else {
        status
            .message
            .clone()
            .unwrap_or_else(|| "not running".into())
    };
    println!("Docker: {state}");
    println!("Endpoint: {}", status.endpoint);
    Ok(())
}

fn ps(ctx: &Ctx, all: bool) -> R<()> {
    let r = ctx.call(CoreCommand::ListDockerContainers { all })?;
    if ctx.print_json(&r) {
        return Ok(());
    }
    let CoreResponse::DockerContainers { containers } = r else {
        return Err(unexpected());
    };
    let mut rows = vec![vec![
        "NAME".into(),
        "IMAGE".into(),
        "STATE".into(),
        "PORTS".into(),
        "STACK".into(),
    ]];
    rows.extend(containers.into_iter().map(|c| {
        let ports = c
            .ports
            .iter()
            .filter_map(|p| p.host_port.map(|h| format!("{h}->{}", p.container_port)))
            .collect::<Vec<_>>()
            .join(",");
        vec![
            c.name,
            c.image,
            c.state,
            ports,
            c.compose_project.unwrap_or_default(),
        ]
    }));
    table(rows);
    Ok(())
}

fn container_action(ctx: &Ctx, c: ContainerArg, action: ContainerAction, done: &str) -> R<()> {
    let r = ctx.call(CoreCommand::DockerContainerAction {
        id: c.container.clone(),
        action,
    })?;
    if !ctx.print_json(&r) {
        println!("{} {done}.", c.container);
    }
    Ok(())
}

fn logs(ctx: &Ctx, container: String, tail: u32) -> R<()> {
    let r = ctx.call(CoreCommand::GetDockerContainerLogs {
        id: container,
        tail: Some(tail),
    })?;
    if ctx.print_json(&r) {
        return Ok(());
    }
    let CoreResponse::DockerLogs { lines } = r else {
        return Err(unexpected());
    };
    for l in lines {
        println!("{l}");
    }
    Ok(())
}

fn images(ctx: &Ctx) -> R<()> {
    let r = ctx.call(CoreCommand::ListDockerImages)?;
    if ctx.print_json(&r) {
        return Ok(());
    }
    let CoreResponse::DockerImages { images } = r else {
        return Err(unexpected());
    };
    let mut rows = vec![vec![
        "IMAGE".into(),
        "SIZE".into(),
        "USED BY".into(),
        "ID".into(),
    ]];
    rows.extend(images.into_iter().map(|i| {
        let name = if i.tags.is_empty() {
            "<untagged>".to_string()
        } else {
            i.tags.join(", ")
        };
        let id =
            i.id.trim_start_matches("sha256:")
                .chars()
                .take(12)
                .collect();
        vec![name, bytes(i.size), i.used_by.join(", "), id]
    }));
    table(rows);
    Ok(())
}

fn volumes(ctx: &Ctx) -> R<()> {
    let r = ctx.call(CoreCommand::ListDockerVolumes)?;
    if ctx.print_json(&r) {
        return Ok(());
    }
    let CoreResponse::DockerVolumes { volumes } = r else {
        return Err(unexpected());
    };
    let mut rows = vec![vec!["VOLUME".into(), "SIZE".into(), "USED BY".into()]];
    rows.extend(volumes.into_iter().map(|v| {
        let name = if v.anonymous {
            format!(
                "{}… (anonymous)",
                v.name.chars().take(12).collect::<String>()
            )
        } else {
            v.name
        };
        vec![
            name,
            v.size.map(bytes).unwrap_or_else(|| "-".into()),
            if v.used_by.is_empty() {
                "unused".into()
            } else {
                v.used_by.join(", ")
            },
        ]
    }));
    table(rows);
    Ok(())
}

fn df(ctx: &Ctx, fresh: bool) -> R<()> {
    let r = ctx.call(CoreCommand::GetDockerDiskUsage { fresh })?;
    if ctx.print_json(&r) {
        return Ok(());
    }
    let CoreResponse::DockerDiskUsage { usage } = r else {
        return Err(unexpected());
    };
    let mut rows = vec![vec!["TYPE".into(), "SIZE".into(), "RECLAIMABLE".into()]];
    for (name, size, reclaim) in [
        ("Images", usage.images, usage.images_reclaimable),
        ("Containers", usage.containers, usage.containers_reclaimable),
        ("Volumes", usage.volumes, usage.volumes_reclaimable),
        (
            "Build cache",
            usage.build_cache,
            usage.build_cache_reclaimable,
        ),
    ] {
        rows.push(vec![name.into(), bytes(size), bytes(reclaim)]);
    }
    table(rows);
    Ok(())
}

fn compose(ctx: &Ctx, cmd: ComposeCmd) -> R<()> {
    match cmd {
        ComposeCmd::List => {
            let r = ctx.call(CoreCommand::ListDockerComposeStacks)?;
            if ctx.print_json(&r) {
                return Ok(());
            }
            let CoreResponse::DockerComposeStacks { stacks } = r else {
                return Err(unexpected());
            };
            let mut rows = vec![vec![
                "STACK".into(),
                "RUNNING".into(),
                "PROJECT".into(),
                "FILE".into(),
            ]];
            rows.extend(stacks.into_iter().map(|s| {
                vec![
                    s.name,
                    format!("{}/{}", s.running, s.services.len()),
                    s.project_name.unwrap_or_default(),
                    s.file.unwrap_or_default(),
                ]
            }));
            table(rows);
            Ok(())
        }
        ComposeCmd::Up(t) => compose_action(ctx, t, ComposeAction::Up),
        ComposeCmd::Down(t) => compose_action(ctx, t, ComposeAction::Down),
        ComposeCmd::Stop(t) => compose_action(ctx, t, ComposeAction::Stop),
        ComposeCmd::Start(t) => compose_action(ctx, t, ComposeAction::Start),
        ComposeCmd::Restart(t) => compose_action(ctx, t, ComposeAction::Restart),
        ComposeCmd::Pull(t) => compose_action(ctx, t, ComposeAction::Pull),
        ComposeCmd::Build(t) => compose_action(ctx, t, ComposeAction::Build),
        ComposeCmd::Watch(t) => compose_action(ctx, t, ComposeAction::Watch),
        ComposeCmd::Check { file } => {
            let path = file.display().to_string();
            let content = std::fs::read_to_string(&file)
                .map_err(|e| format!("could not read {path}: {e}"))?;
            let r = ctx.call(CoreCommand::ValidateDockerCompose { path, content })?;
            if ctx.print_json(&r) {
                return Ok(());
            }
            let CoreResponse::DockerComposeCheck { check } = r else {
                return Err(unexpected());
            };
            for e in &check.errors {
                println!("error: {e}");
            }
            for w in &check.warnings {
                println!("warning: {w}");
            }
            if check.ok {
                println!("The compose file is valid.");
                Ok(())
            } else {
                Err("the compose file has errors".into())
            }
        }
    }
}

fn compose_action(ctx: &Ctx, t: ComposeTarget, action: ComposeAction) -> R<()> {
    let r = ctx.call(CoreCommand::DockerComposeAction {
        path: t.file.display().to_string(),
        action,
        service: t.service,
    })?;
    follow(ctx, r)
}

fn dockerize(
    ctx: &Ctx,
    project: &str,
    mode: DockerizeModeArg,
    services: Option<Vec<String>>,
    create: bool,
    replace_compose: bool,
    show_files: bool,
) -> R<()> {
    let project_id = ctx.project_id(project)?;
    let options = DockerizeOptions {
        mode: match mode {
            DockerizeModeArg::Dev => DockerizeMode::Dev,
            DockerizeModeArg::Prod => DockerizeMode::Prod,
        },
        services,
    };
    let r = if create {
        ctx.call(CoreCommand::CreateDockerize {
            project_id,
            overwrite_compose: replace_compose,
            options,
        })?
    } else {
        ctx.call(CoreCommand::PlanDockerize {
            project_id,
            options,
        })?
    };
    if ctx.print_json(&r) {
        return Ok(());
    }
    let CoreResponse::DockerizePlan { plan } = r else {
        return Err(unexpected());
    };
    println!("Kind: {} ({:?} mode)", plan.kind, plan.mode);
    for d in &plan.detected {
        println!("  found: {d}");
    }
    let picked: Vec<&str> = plan
        .services
        .iter()
        .filter(|s| s.selected)
        .map(|s| s.id.as_str())
        .collect();
    println!(
        "Services: {}",
        if picked.is_empty() {
            "none".into()
        } else {
            picked.join(", ")
        }
    );
    if !plan.workers.is_empty() {
        println!("Extra processes: {}", plan.workers.join(", "));
    }
    if !plan.secrets.is_empty() {
        println!(
            "{} password(s) {} in the Windows credential store; never written to the files.",
            plan.secrets.len(),
            if create { "kept" } else { "would be generated" }
        );
    }
    println!("Host port: {}", plan.port);
    if !plan.existing.is_empty() {
        println!("Kept as they are: {}", plan.existing.join(", "));
    }
    for n in &plan.notes {
        println!("note: {n}");
    }
    if !plan.unknown_extensions.is_empty() {
        println!("Needs manual setup: {}", plan.unknown_extensions.join(", "));
    }
    println!("\n{}", plan.disclaimer);
    for c in &plan.checklist {
        println!("  - {c}");
    }
    if show_files {
        if let Some(df) = &plan.dockerfile {
            println!("\n--- Dockerfile ---\n{df}");
        }
        println!("\n--- compose.yaml ---\n{}", plan.compose);
        if let Some(di) = &plan.dockerignore {
            println!("\n--- .dockerignore ---\n{di}");
        }
    }
    if create {
        println!("\nFiles written. Compose file: {}", plan.compose_path);
        let verb = if plan.watch { "watch" } else { "up" };
        println!(
            "Start it with: olsc docker compose {verb} \"{}\"",
            plan.compose_path
        );
    } else {
        println!("\nNothing was written. Add --create to write the files.");
    }
    Ok(())
}

fn prune_plan(ctx: &Ctx, targets: PruneTargets) -> R<()> {
    let r = ctx.call(CoreCommand::PlanDockerPrune { targets })?;
    if ctx.print_json(&r) {
        return Ok(());
    }
    let CoreResponse::DockerPrunePlan { plan } = r else {
        return Err(unexpected());
    };
    if plan.items.is_empty() {
        println!("Nothing to remove for those choices.");
        return Ok(());
    }
    let mut rows = vec![vec!["KIND".into(), "NAME".into(), "SIZE".into()]];
    rows.extend(
        plan.items
            .into_iter()
            .map(|i| vec![i.kind, i.name, bytes(i.size)]),
    );
    table(rows);
    println!(
        "{} would be freed. Nothing was removed: do the cleanup in the app (Docker → Reclaim space).",
        bytes(plan.total)
    );
    Ok(())
}

/// Waits for a background task and prints its steps; `--json` prints the start reply only.
pub(crate) fn follow(ctx: &Ctx, started: CoreResponse) -> R<()> {
    if ctx.print_json(&started) {
        return Ok(());
    }
    let CoreResponse::TaskStarted { id } = started else {
        return Err(unexpected());
    };
    let begun = Instant::now();
    let mut shown = 0usize;
    loop {
        let CoreResponse::Tasks { tasks } = ctx.call(CoreCommand::ListTasks)? else {
            return Err(unexpected());
        };
        let Some(task) = tasks.into_iter().find(|t| t.id == id) else {
            println!("Started task {id}.");
            return Ok(());
        };
        for step in task.steps.iter().skip(shown) {
            println!("- {}", step.label);
        }
        shown = task.steps.len();
        if task.state != TaskState::Running {
            for p in &task.problems {
                eprintln!("problem: {p}");
            }
            return match (task.state, task.error) {
                (TaskState::Succeeded, None) => {
                    println!("{} — done.", task.title);
                    Ok(())
                }
                (TaskState::Cancelled, _) => Err(format!("{} was stopped.", task.title)),
                (_, Some(d)) => Err(crate::ctx::diag(d)),
                _ => Err(format!("{} did not finish.", task.title)),
            };
        }
        if begun.elapsed() > Duration::from_secs(60 * 60) {
            return Err(format!(
                "task {id} is still running after an hour; see the app"
            ));
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

pub(crate) fn bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1000.0 && i < UNITS.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if i == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}

#[cfg(test)]
mod tests {
    use super::bytes;

    #[test]
    fn sizes_read_plainly() {
        assert_eq!(bytes(0), "0 B");
        assert_eq!(bytes(1_200_000_000), "1.2 GB");
        assert_eq!(bytes(64_000), "64.0 KB");
    }
}
