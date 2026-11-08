//! `olsc db …`: Migration guard copies (§168). Everything goes through a `CoreCommand`.

use crate::args::{DbCmd, GuardCmd};
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
    }
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
