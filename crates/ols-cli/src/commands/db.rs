//! `olsc db …`: Migration guard copies (§168). Everything goes through a `CoreCommand`.

use crate::args::{BranchCmd, DbCmd, GuardCmd};
use crate::commands::docker::{bytes, follow};
use crate::ctx::{table, Ctx, R};
use ols_core::command::{CoreCommand, CoreResponse};

pub fn db(ctx: &Ctx, cmd: DbCmd) -> R<()> {
    match cmd {
        DbCmd::Guard(GuardCmd::List { project }) => list(ctx, project),
        DbCmd::Guard(GuardCmd::Rewind { id }) => {
            let r = ctx.call(CoreCommand::RewindGuardSnapshot { id })?;
            follow(ctx, r)
        }
        DbCmd::Branch(cmd) => branch(ctx, cmd),
        DbCmd::Remotes { project } => remotes(ctx, project),
        DbCmd::Pull {
            project,
            remote,
            anonymize,
            new,
            yes_real_data,
        } => {
            if !anonymize && !yes_real_data {
                return Err("Without --anonymize this copies real personal data onto your computer. Add --anonymize, or confirm with --yes-real-data.".into());
            }
            let r = ctx.call(CoreCommand::PullRemoteDb {
                project_id: ctx.project_id(&project)?,
                remote_id: remote,
                mode: if new { "new" } else { "replace" }.into(),
                anonymize,
                confirm_real_data: yes_real_data,
            })?;
            follow(ctx, r)
        }
    }
}

/// `olsc project audit <project>|--all`: runs the check, then prints a table.
pub fn audit(ctx: &Ctx, project: Option<String>, all: bool) -> R<()> {
    let started = match (&project, all) {
        (Some(p), false) => ctx.call(CoreCommand::AuditProject {
            project_id: ctx.project_id(p)?,
        })?,
        (None, true) => ctx.call(CoreCommand::AuditAllProjects)?,
        _ => return Err("Give a project, or --all.".into()),
    };
    follow(ctx, started)?;
    if ctx.json {
        return Ok(());
    }
    let CoreResponse::AuditResults { results } = ctx.call(CoreCommand::ListAuditResults)? else {
        return Err("unexpected reply".into());
    };
    let wanted = match &project {
        Some(p) => Some(ctx.project_id(p)?),
        None => None,
    };
    let mut rows = vec![vec![
        "PROJECT".into(),
        "TOOL".into(),
        "CRITICAL".into(),
        "HIGH".into(),
        "MODERATE".into(),
        "LOW".into(),
    ]];
    for r in results
        .iter()
        .filter(|r| wanted.as_ref().is_none_or(|w| *w == r.project_id))
    {
        for t in &r.tools {
            let n = |sev: &str| {
                t.findings
                    .iter()
                    .filter(|f| f.severity == sev)
                    .count()
                    .to_string()
            };
            let tool = if t.ok {
                t.tool.clone()
            } else {
                format!("{} (failed)", t.tool)
            };
            rows.push(vec![
                r.project_id.clone(),
                tool,
                n("critical"),
                n("high"),
                n("moderate"),
                n("low"),
            ]);
        }
    }
    table(rows);
    Ok(())
}

/// `olsc runtime eol`.
pub fn eol(ctx: &Ctx) -> R<()> {
    let r = ctx.call(CoreCommand::GetEolReport)?;
    if ctx.print_json(&r) {
        return Ok(());
    }
    let CoreResponse::EolReport { items } = r else {
        return Err("unexpected reply".into());
    };
    let mut rows = vec![vec![
        "RUNTIME".into(),
        "VERSION".into(),
        "STATUS".into(),
        "USED BY".into(),
    ]];
    rows.extend(items.into_iter().map(|i| {
        let status = match i.status {
            ols_core::eol::EolStatus::Supported => "supported".to_string(),
            ols_core::eol::EolStatus::SecurityOnly { until } => {
                format!("security fixes until {until}")
            }
            ols_core::eol::EolStatus::Ended { since } => format!("ended {since}"),
            ols_core::eol::EolStatus::Unknown => "unknown".to_string(),
        };
        vec![i.runtime, i.version, status, i.used_by.join(", ")]
    }));
    table(rows);
    Ok(())
}

fn remotes(ctx: &Ctx, project: String) -> R<()> {
    let r = ctx.call(CoreCommand::ListRemoteDbs {
        project_id: ctx.project_id(&project)?,
    })?;
    if ctx.print_json(&r) {
        return Ok(());
    }
    let CoreResponse::RemoteDbs { remotes } = r else {
        return Err("unexpected reply".into());
    };
    if remotes.is_empty() {
        println!("No server profiles yet. Add one in the project's Backups tab.");
        return Ok(());
    }
    let mut rows = vec![vec![
        "ID".into(),
        "NAME".into(),
        "SSH".into(),
        "ENGINE".into(),
        "DATABASE".into(),
    ]];
    rows.extend(
        remotes
            .into_iter()
            .map(|r| vec![r.id, r.name, r.ssh_target, r.engine, r.database]),
    );
    table(rows);
    Ok(())
}

fn branch(ctx: &Ctx, cmd: BranchCmd) -> R<()> {
    let r = match cmd {
        BranchCmd::Status { project } => ctx.call(CoreCommand::GetBranchDb {
            project_id: ctx.project_id(&project)?,
        })?,
        BranchCmd::On {
            project,
            from_current,
        } => ctx.call(CoreCommand::SetBranchDb {
            project_id: ctx.project_id(&project)?,
            enabled: true,
            clone_from: Some(if from_current { "current" } else { "base" }.into()),
        })?,
        BranchCmd::Off { project } => ctx.call(CoreCommand::SetBranchDb {
            project_id: ctx.project_id(&project)?,
            enabled: false,
            clone_from: None,
        })?,
    };
    if ctx.print_json(&r) {
        return Ok(());
    }
    let CoreResponse::BranchDb { info } = r else {
        return Err("unexpected reply".into());
    };
    match &info.config {
        Some(c) if c.enabled => println!(
            "On. Main branch {} uses {}.",
            c.base_branch, c.base_database
        ),
        _ => println!("Off."),
    }
    println!(
        "Now: branch {} -> database {}",
        info.current_branch.as_deref().unwrap_or("(detached)"),
        info.current_database.as_deref().unwrap_or("(none)")
    );
    if let Some(c) = &info.config {
        for (b, d) in &c.map {
            let tag = if info.orphaned.contains(b) {
                "  (branch is gone)"
            } else {
                ""
            };
            println!("  {b} -> {d}{tag}");
        }
    }
    if let Some(p) = &info.pending {
        println!("Waiting: start the database server to switch to {p}.");
    }
    Ok(())
}

fn list(ctx: &Ctx, project: Option<String>) -> R<()> {
    let project_id = project.map(|p| ctx.project_id(&p)).transpose()?;
    let r = ctx.call(CoreCommand::ListGuardSnapshots { project_id })?;
    if ctx.print_json(&r) {
        return Ok(());
    }
    let CoreResponse::GuardSnapshots { snapshots } = r else {
        return Err("unexpected reply".into());
    };
    if snapshots.is_empty() {
        println!("No copies saved yet. OLS saves the database before a migration it runs.");
        return Ok(());
    }
    let mut rows = vec![vec![
        "ID".into(),
        "DATABASE".into(),
        "ENGINE".into(),
        "SIZE".into(),
        "COMMAND".into(),
    ]];
    rows.extend(
        snapshots
            .into_iter()
            .map(|s| vec![s.id, s.database, s.engine, bytes(s.size), s.command]),
    );
    table(rows);
    Ok(())
}

/// `olsc project reclaim`: scan, print, and (with `--delete --yes`) clear rebuildable folders.
pub fn reclaim(ctx: &Ctx, days: u64, delete: bool, yes: bool) -> R<()> {
    let started = ctx.call(CoreCommand::ScanReclaim)?;
    if ctx.json {
        return follow(ctx, started);
    }
    follow(ctx, started)?;
    let CoreResponse::ReclaimReport { report } = ctx.call(CoreCommand::GetReclaimReport)? else {
        return Err("unexpected reply".into());
    };
    let Some(report) = report else {
        println!("Nothing was found.");
        return Ok(());
    };
    let projects: Vec<_> = report
        .projects
        .into_iter()
        .filter(|p| p.idle_days >= days)
        .collect();
    if projects.is_empty() {
        println!("No project unused for {days} days or more has folders to clear.");
        return Ok(());
    }
    let mut rows = vec![vec![
        "PROJECT".into(),
        "IDLE".into(),
        "FOLDER".into(),
        "SIZE".into(),
        "REBUILD WITH".into(),
    ]];
    let mut chosen = Vec::new();
    let mut total = 0u64;
    for p in &projects {
        for item in &p.items {
            let mark = if p.busy {
                " (busy)"
            } else if !item.default_selected {
                " (not selected)"
            } else {
                ""
            };
            rows.push(vec![
                p.name.clone(),
                format!("{}d", p.idle_days),
                format!("{}{mark}", item.relative),
                format!(
                    "{}{}",
                    if item.size_is_minimum {
                        "at least "
                    } else {
                        ""
                    },
                    bytes(item.bytes)
                ),
                item.rebuild.clone(),
            ]);
            if !p.busy && item.default_selected {
                total += item.bytes;
                chosen.push(ols_core::reclaim::ReclaimItem {
                    project_id: p.project_id.clone(),
                    relative: item.relative.clone(),
                });
            }
        }
    }
    table(rows);
    println!(
        "{} could be freed from {} folders.",
        bytes(total),
        chosen.len()
    );
    if !delete {
        return Ok(());
    }
    if !yes {
        return Err("Nothing was deleted. Add --yes to delete these folders permanently.".into());
    }
    let r = ctx.call(CoreCommand::ReclaimFolders { items: chosen })?;
    follow(ctx, r)
}
