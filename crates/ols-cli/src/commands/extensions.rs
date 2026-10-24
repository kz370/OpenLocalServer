use std::path::Path;
use std::time::Duration;
use ols_core::command::{CoreCommand, CoreResponse};
use ols_core::updater::UpdateStatus;
use crate::args::{ApiCmd, CatalogCmd, PluginCmd, TestCmd, UpdateCmd};
use crate::ctx::{confirm, table, Ctx, R};

pub fn test(ctx: &Ctx, cmd: TestCmd) -> R<()> {
    match cmd {
        TestCmd::Load {
            project,
            script,
            site,
            profile,
            vars,
            public,
        } => load_test(ctx, project, script, site, profile, vars, public),
    }
}

pub fn plugin(ctx: &Ctx, cmd: PluginCmd) -> R<()> {
    let list = |ctx: &Ctx| -> R<Vec<ols_core::plugin::PluginInfo>> {
        match ctx.call(CoreCommand::ListPlugins)? {
            CoreResponse::Plugins { plugins } => Ok(plugins),
            _ => Err("unexpected reply".into()),
        }
    };
    match cmd {
        PluginCmd::List => {
            let plugins = list(ctx)?;
            let mut rows = vec![vec![
                "ID".into(),
                "NAME".into(),
                "VERSION".into(),
                "STATE".into(),
                "ADDS".into(),
            ]];
            rows.extend(plugins.iter().map(|p| {
                let state = if p.problem.is_some() {
                    "unusable"
                } else if p.enabled {
                    "on"
                } else {
                    "off"
                };
                let mut adds = Vec::new();
                for (n, what) in [
                    (p.runtimes, "runtimes"),
                    (p.quick_apps, "quick apps"),
                    (p.detections, "detections"),
                    (p.health_checks, "health checks"),
                ] {
                    if n > 0 {
                        adds.push(format!("{n} {what}"));
                    }
                }
                vec![
                    p.manifest.id.clone(),
                    p.manifest.name.clone(),
                    p.manifest.version.clone(),
                    state.into(),
                    adds.join(", "),
                ]
            }));
            table(rows);
        }
        PluginCmd::Install { source } => {
            let abs = std::fs::canonicalize(&source).unwrap_or(source);
            let r = ctx.call(CoreCommand::InstallPlugin {
                source: abs
                    .display()
                    .to_string()
                    .trim_start_matches(r"\\?\")
                    .to_string(),
            })?;
            if !ctx.print_json(&r) {
                if let CoreResponse::Plugin { plugin } = r {
                    println!(
                        "Installed {} {}. It is off; turn it on with: ols plugin enable {}",
                        plugin.manifest.name, plugin.manifest.version, plugin.manifest.id
                    );
                }
            }
        }
        PluginCmd::Enable { id, yes } => {
            let plugin = list(ctx)?
                .into_iter()
                .find(|p| p.manifest.id == id)
                .ok_or_else(|| format!("no plugin named {id}"))?;
            if let Some(problem) = &plugin.problem {
                return Err(problem.clone());
            }
            println!("{} asks for:", plugin.manifest.name);
            for p in &plugin.permissions {
                println!("  - {}", p.description);
            }
            if !plugin.permissions.is_empty() && !yes && !confirm("Allow these?") {
                println!("Not turned on.");
                return Ok(());
            }
            let approve = plugin.manifest.permissions.clone();
            ctx.call(CoreCommand::SetPluginEnabled {
                id: id.clone(),
                enabled: true,
                approve,
            })?;
            println!("{id} is on.");
        }
        PluginCmd::Disable { id } => {
            ctx.call(CoreCommand::SetPluginEnabled {
                id: id.clone(),
                enabled: false,
                approve: vec![],
            })?;
            println!("{id} is off.");
        }
        PluginCmd::Remove { id } => {
            ctx.call(CoreCommand::RemovePlugin { id: id.clone() })?;
            println!("Removed {id}.");
        }
    }
    Ok(())
}



pub fn catalog(ctx: &Ctx, cmd: CatalogCmd) -> R<()> {
    let show = |ctx: &Ctx, r: CoreResponse| {
        if ctx.print_json(&r) {
            return;
        }
        if let CoreResponse::CatalogSources { catalogs } = r {
            let mut rows = vec![vec![
                "ID".into(),
                "NAME".into(),
                "SIGNATURE".into(),
                "RUNTIMES".into(),
                "PLUGINS".into(),
                "NOTE".into(),
            ]];
            rows.extend(catalogs.iter().map(|c| {
                vec![
                    c.source.id.clone(),
                    c.source.name.clone(),
                    if c.verified {
                        "verified".into()
                    } else {
                        "not verified".into()
                    },
                    c.doc
                        .as_ref()
                        .map(|d| d.runtimes.len().to_string())
                        .unwrap_or_default(),
                    c.doc
                        .as_ref()
                        .map(|d| {
                            d.plugins
                                .iter()
                                .map(|p| p.id.clone())
                                .collect::<Vec<_>>()
                                .join(", ")
                        })
                        .unwrap_or_default(),
                    c.error
                        .clone()
                        .or_else(|| c.note.clone())
                        .unwrap_or_default(),
                ]
            }));
            table(rows);
        }
    };
    match cmd {
        CatalogCmd::List => show(ctx, ctx.call(CoreCommand::ListCatalogSources)?),
        CatalogCmd::Add {
            name,
            url,
            public_key,
        } => {
            let r = ctx.call(CoreCommand::AddCatalogSource {
                name,
                url,
                public_key,
            })?;
            show(ctx, r);
            println!("Added. Run `olsc catalog refresh` to download it.");
        }
        CatalogCmd::Remove { id } => show(ctx, ctx.call(CoreCommand::RemoveCatalogSource { id })?),
        CatalogCmd::Refresh { id } => show(ctx, ctx.call(CoreCommand::RefreshCatalogs { id })?),
        CatalogCmd::Install { catalog, plugin } => {
            ctx.call(CoreCommand::InstallCatalogPlugin {
                source_id: catalog,
                plugin_id: plugin.clone(),
            })?;
            println!("Installed {plugin}. It is off; turn it on with: ols plugin enable {plugin}");
        }
    }
    Ok(())
}


pub fn api(ctx: &Ctx, cmd: ApiCmd) -> R<()> {
    let show = |ctx: &Ctx, r: CoreResponse| {
        if ctx.print_json(&r) {
            return;
        }
        if let CoreResponse::ApiStatus { status } = r {
            println!(
                "API: {} ({} mode) at {}",
                if status.running { "running" } else { "off" },
                status.settings.mode,
                status.url
            );
            println!(
                "Token: {}",
                if status.token_set {
                    "set"
                } else {
                    "not set (ols api token)"
                }
            );
            if let Some(e) = status.error {
                println!("Problem: {e}");
            }
        }
    };
    match cmd {
        ApiCmd::Status => show(ctx, ctx.call(CoreCommand::GetApiStatus)?),
        ApiCmd::Enable { port, operate } => {
            let CoreResponse::ApiStatus { status } = ctx.call(CoreCommand::GetApiStatus)? else {
                return Err("unexpected reply".into());
            };
            if !status.token_set {
                if let CoreResponse::Text { text } = ctx.call(CoreCommand::RotateApiToken)? {
                    println!("New API token (shown once): {text}");
                }
            }
            show(
                ctx,
                ctx.call(CoreCommand::SetApiSettings {
                    enabled: true,
                    port,
                    mode: if operate { "operate" } else { "read_only" }.into(),
                })?,
            );
        }
        ApiCmd::Disable => {
            let CoreResponse::ApiStatus { status } = ctx.call(CoreCommand::GetApiStatus)? else {
                return Err("unexpected reply".into());
            };
            show(
                ctx,
                ctx.call(CoreCommand::SetApiSettings {
                    enabled: false,
                    port: status.settings.port,
                    mode: status.settings.mode,
                })?,
            );
        }
        ApiCmd::Token => {
            if let CoreResponse::Text { text } = ctx.call(CoreCommand::RotateApiToken)? {
                println!(
                    "New API token (shown once; the old one no longer works):
{text}"
                );
            }
        }
    }
    Ok(())
}



pub fn update(ctx: &Ctx, cmd: UpdateCmd) -> R<()> {
    match cmd {
        UpdateCmd::Check => {
            let r = ctx.call(CoreCommand::CheckUpdate)?;
            if !ctx.print_json(&r) {
                if let CoreResponse::Update { update } = r {
                    match update.status {
                        UpdateStatus::UpdateAvailable => {
                            println!(
                                "Version {} is available (you have {}). Signature verified.
{}",
                                update.latest, update.current, update.notes
                            );
                            println!("Download it with: ols update download");
                        }
                        UpdateStatus::UpToDate => {
                            println!("You are up to date ({}).", update.current);
                        }
                        UpdateStatus::DevelopmentVersion => {
                            println!(
                                "Development version ({}) is newer than the online release ({}).",
                                update.current, update.latest
                            );
                        }
                    }
                }
            }
        }
        UpdateCmd::Download => {
            let r = ctx.call(CoreCommand::DownloadUpdate)?;
            if !ctx.print_json(&r) {
                if let CoreResponse::Update { update } = r {
                    println!(
                        "Downloaded and verified: {}
Install it with: ols update install",
                        update.downloaded.unwrap_or_default()
                    );
                }
            }
        }
        UpdateCmd::Install => {
            if !confirm("Start the installer? OLS will be replaced.") {
                return Ok(());
            }
            ctx.call(CoreCommand::InstallUpdate)?;
            println!("The installer has started.");
        }
    }
    Ok(())
}


pub fn load_test(
    ctx: &Ctx,
    project: Option<String>,
    script: Option<String>,
    site: Option<String>,
    profile: String,
    vars: Vec<String>,
    public: bool,
) -> R<()> {
    let id = match project {
        Some(p) => ctx.project_id(&p)?,
        None => ctx.project_for_path(Path::new("."))?.0,
    };
    let CoreResponse::LoadOverview { overview } = ctx.call(CoreCommand::LoadOverview {
        project_id: id.clone(),
    })?
    else {
        return Err("unexpected reply".into());
    };
    if !overview.k6.installed {
        return Err("k6 isn't installed: run `olsc runtime install k6`".into());
    }
    let mut env: Vec<(String, String)> = Vec::new();
    for v in &vars {
        let (k, val) = v
            .split_once('=')
            .ok_or_else(|| format!("--var wants NAME=VALUE, not {v}"))?;
        env.push((k.to_string(), val.to_string()));
    }
    let script = match script {
        Some(s) => s,
        None => match overview.scripts.as_slice() {
            [] => {
                let CoreResponse::LoadProfiles { profiles } =
                    ctx.call(CoreCommand::LoadListProfiles)?
                else {
                    return Err("unexpected reply".into());
                };
                let plan = profiles
                    .into_iter()
                    .find(|p| p.id == profile)
                    .ok_or_else(|| format!("no test plan named {profile}"))?;
                for v in plan.variables.iter().filter(|v| !v.value.is_empty()) {
                    if !env.iter().any(|(k, _)| *k == v.name) {
                        env.push((v.name.clone(), v.value.clone()));
                    }
                }
                let CoreResponse::Text { text } = ctx.call(CoreCommand::LoadGenerate {
                    project_id: id.clone(),
                    profile: plan,
                    name: None,
                })?
                else {
                    return Err("unexpected reply".into());
                };
                println!("No script yet; wrote the {profile} test: .openlocalserver/k6/{text}");
                text
            }
            [only] => only.name.clone(),
            many => {
                return Err(format!(
                    "choose a script: {}",
                    many.iter()
                        .map(|s| s.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            }
        },
    };
    let CoreResponse::LoadRun { run } = ctx.call(CoreCommand::LoadRun {
        project_id: id,
        script: script.clone(),
        target: site,
        confirm_public: public,
        env,
    })?
    else {
        return Err("unexpected reply".into());
    };
    println!("Running {script} against {} ...", run.target);
    let run_id = run.id;
    let last = loop {
        std::thread::sleep(Duration::from_secs(2));
        let CoreResponse::LoadRun { run } = ctx.call(CoreCommand::LoadStatus {
            run_id: run_id.clone(),
        })?
        else {
            return Err("unexpected reply".into());
        };
        let m = &run.metrics;
        if !ctx.json {
            println!(
                "  {} requests, {:.1}/s, p95 {:.0} ms, errors {:.1}%, {} users",
                m.requests,
                m.rps,
                m.p95_ms,
                m.error_rate * 100.0,
                m.vus
            );
        }
        if run.state != "running" {
            break run;
        }
    };
    if ctx.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&last).unwrap_or_default()
        );
    } else {
        let m = &last.metrics;
        println!(
            "
{}: {} requests, p50 {:.0} ms, p95 {:.0} ms, p99 {:.0} ms, errors {:.2}%",
            last.state.to_uppercase(),
            m.requests,
            m.p50_ms,
            m.p95_ms,
            m.p99_ms,
            m.error_rate * 100.0
        );
    }
    match last.state.as_str() {
        "passed" => Ok(()),
        _ => Err(last
            .message
            .unwrap_or_else(|| "the test didn't pass".into())),
    }
}

