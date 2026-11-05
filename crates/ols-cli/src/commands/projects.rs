use crate::args::{ProjectCmd, QuickCommandCmd, SnapshotCmd, WorkerCmd};
use crate::commands::system::setup;
use crate::ctx::{table, Ctx, R};
use ols_core::command::{CoreCommand, CoreResponse};
use std::path::{Path, PathBuf};

fn set_ignore_marker(path: PathBuf, marker: &str, label: &str, add: bool) -> R<()> {
    let path = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .map_err(|e| format!("Could not resolve the selected folder: {e}"))?
            .join(path)
    };
    if !path.is_dir() {
        return Err(format!(
            "Cannot change the ignore marker because '{}' is not an accessible folder. Choose an existing project folder.",
            path.display()
        ));
    }
    let marker_path = path.join(marker);
    if add {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&marker_path)
        {
            Ok(_) => println!("{label} is now ignored. Project files were kept."),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                println!("{label} is already ignored.");
            }
            Err(error) => {
                return Err(format!(
                    "Could not create '{}': {error}. Check that you can write to the project folder.",
                    marker_path.display()
                ));
            }
        }
    } else {
        match std::fs::remove_file(&marker_path) {
            Ok(()) => println!("{label} is no longer ignored."),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                println!("{label} is not ignored.");
            }
            Err(error) => {
                return Err(format!(
                    "Could not remove '{}': {error}. Check that you can write to the project folder.",
                    marker_path.display()
                ));
            }
        }
    }
    Ok(())
}

pub fn domain(ctx: &Ctx) -> R<()> {
    let r = ctx.call(CoreCommand::ListDomains)?;
    if !ctx.print_json(&r) {
        if let CoreResponse::Domains { domains } = r {
            let mut rows = vec![vec![
                "SITE".into(),
                "TYPE".into(),
                "STATE".into(),
                "FOLDER".into(),
            ]];
            rows.extend(domains.into_iter().map(|d| {
                vec![
                    d.url,
                    d.group,
                    if d.enabled {
                        "enabled".into()
                    } else {
                        "disabled".into()
                    },
                    d.folder,
                ]
            }));
            table(rows);
        }
    }
    Ok(())
}

pub fn certificate(ctx: &Ctx) -> R<()> {
    let r = ctx.call(CoreCommand::ListCertificates)?;
    if !ctx.print_json(&r) {
        if let CoreResponse::Certificates { certs } = r {
            let mut rows = vec![vec!["HOST".into(), "DAYS LEFT".into(), "NAMES".into()]];
            rows.extend(
                certs
                    .into_iter()
                    .map(|c| vec![c.hostname, c.days_left.to_string(), c.sans.join(", ")]),
            );
            table(rows);
        }
    }
    Ok(())
}

pub fn quick_app(ctx: &Ctx) -> R<()> {
    let r = ctx.call(CoreCommand::ListQuickApps)?;
    if !ctx.print_json(&r) {
        if let CoreResponse::QuickApps { apps } = r {
            table(
                apps.into_iter()
                    .map(|a| vec![a.id, a.name, a.description])
                    .collect(),
            );
        }
    }
    Ok(())
}

pub fn quick_command(ctx: &Ctx, cmd: QuickCommandCmd) -> R<()> {
    match cmd {
        QuickCommandCmd::List => {
            let r = ctx.call(CoreCommand::ListQuickCommands)?;
            if !ctx.print_json(&r) {
                if let CoreResponse::QuickCommands { commands } = r {
                    table(
                        commands
                            .into_iter()
                            .map(|c| vec![c.id, c.name, c.description])
                            .collect(),
                    );
                }
            }
        }
        QuickCommandCmd::Run { id, project } => {
            let pid = match project {
                Some(p) => Some(ctx.project_id(&p)?),
                None => ctx.project_for_path(Path::new(".")).ok().map(|(id, _)| id),
            };
            match ctx.call(CoreCommand::RunQuickCommand {
                id: id.clone(),
                project_id: pid,
            })? {
                CoreResponse::MaybeProcess { id: Some(process) } => {
                    crate::commands::services::follow(ctx, process)?
                }
                _ => println!("Done."),
            }
        }
    }
    Ok(())
}

pub fn worker(ctx: &Ctx, cmd: WorkerCmd) -> R<()> {
    match cmd {
        WorkerCmd::List => {
            let r = ctx.call(CoreCommand::ListWorkers { project_id: None })?;
            if !ctx.print_json(&r) {
                if let CoreResponse::Workers { workers } = r {
                    table(
                        workers
                            .into_iter()
                            .map(|w| {
                                vec![
                                    w.worker.name,
                                    format!("{}/{}", w.running, w.worker.count),
                                    w.command_line,
                                ]
                            })
                            .collect(),
                    );
                }
            }
        }
        WorkerCmd::Start { project } => {
            let id = ctx.project_id(&project)?;
            if let CoreResponse::Count { count } =
                ctx.call(CoreCommand::StartProjectWorkers { project_id: id })?
            {
                println!("{count} worker process(es) running.");
            }
        }
        WorkerCmd::Stop { project } => {
            let id = ctx.project_id(&project)?;
            ctx.call(CoreCommand::StopProjectWorkers { project_id: id })?;
            println!("Workers stopped.");
        }
    }
    Ok(())
}

pub fn snapshot(ctx: &Ctx, cmd: SnapshotCmd) -> R<()> {
    match cmd {
        SnapshotCmd::List { project } => {
            let id = ctx.project_id(&project)?;
            let r = ctx.call(CoreCommand::ListSnapshots { project_id: id })?;
            if !ctx.print_json(&r) {
                if let CoreResponse::Snapshots { snapshots } = r {
                    table(
                        snapshots
                            .into_iter()
                            .map(|s| vec![s.id, s.label, s.summary.join(" · ")])
                            .collect(),
                    );
                }
            }
        }
        SnapshotCmd::Create {
            project,
            label,
            env,
            databases,
            files,
        } => {
            let id = ctx.project_id(&project)?;
            let r = ctx.call(CoreCommand::CreateSnapshot {
                project_id: id,
                label,
                options: ols_core::snapshots::SnapshotOptions {
                    env,
                    databases,
                    files,
                },
            })?;
            if !ctx.print_json(&r) {
                if let CoreResponse::Snapshot { snapshot } = r {
                    println!("Snapshot saved: {}", snapshot.path);
                }
            }
        }
    }
    Ok(())
}

pub fn project(ctx: &Ctx, cmd: ProjectCmd) -> R<()> {
    match cmd {
        ProjectCmd::List => {
            let r = ctx.call(CoreCommand::ListProjects)?;
            if !ctx.print_json(&r) {
                if let CoreResponse::Projects { projects, .. } = r {
                    table(projects.into_iter().map(|p| vec![p.name, p.path]).collect());
                }
            }
        }
        ProjectCmd::Add { path } => {
            let (_, name) = ctx.project_for_path(&path)?;
            println!("{name} is a project.");
        }
        ProjectCmd::Ignore { path } => set_ignore_marker(
            path,
            ols_core::app::OLS_IGNORE_PROJECT_MARKER,
            "Project and site",
            true,
        )?,
        ProjectCmd::Unignore { path } => set_ignore_marker(
            path,
            ols_core::app::OLS_IGNORE_PROJECT_MARKER,
            "Project and site",
            false,
        )?,
        ProjectCmd::IgnoreSite { path } => {
            set_ignore_marker(path, ols_core::app::OLS_IGNORE_SITE_MARKER, "Site", true)?
        }
        ProjectCmd::UnignoreSite { path } => {
            set_ignore_marker(path, ols_core::app::OLS_IGNORE_SITE_MARKER, "Site", false)?
        }
        ProjectCmd::Remove { name } => {
            let id = ctx.project_id(&name)?;
            ctx.call(CoreCommand::RemoveProject { id })?;
            println!("Removed {name} (its files were not touched).");
        }
        ProjectCmd::Start { name } => {
            let id = ctx.project_id(&name)?;
            let CoreResponse::Projects { projects, .. } = ctx.call(CoreCommand::ListProjects)?
            else {
                return Ok(());
            };
            let path = projects
                .into_iter()
                .find(|p| p.id == id)
                .map(|p| PathBuf::from(p.path))
                .unwrap_or_default();
            setup(ctx, path, false, true)?;
            if let CoreResponse::Count { count } =
                ctx.call(CoreCommand::StartProjectWorkers { project_id: id })?
            {
                if count > 0 {
                    println!("{count} worker process(es) running.");
                }
            }
        }
        ProjectCmd::Stop { name } => {
            let id = ctx.project_id(&name)?;
            ctx.call(CoreCommand::StopProjectWorkers { project_id: id })?;
            println!("{name}'s workers are stopped.");
        }
        ProjectCmd::Clone { url, path } => {
            let target = if path.is_absolute() {
                path
            } else {
                std::env::current_dir()
                    .map_err(|e| e.to_string())?
                    .join(path)
            };
            println!("Cloning {url}…");
            if let CoreResponse::Project { project } = ctx.call(CoreCommand::GitClone {
                url,
                target: target.display().to_string(),
                branch: None,
                auth: None,
                standalone: None,
            })? {
                println!(
                    "{} is ready at {}. Next: cd there and run `olsc setup`.",
                    project.name, project.path
                );
            }
        }
    }
    Ok(())
}
