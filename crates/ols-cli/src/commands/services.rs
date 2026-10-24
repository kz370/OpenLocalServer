use std::io::Write;
use std::time::{Duration, Instant};
use ols_core::command::{CoreCommand, CoreResponse};
use crate::args::{PhpCmd, RuntimeCmd, ServiceCmd, TunnelCmd};
use crate::ctx::{confirm, table, Ctx, R};

pub fn runtime(ctx: &Ctx, cmd: RuntimeCmd) -> R<()> {
    match cmd {
        RuntimeCmd::List => {
            let r = ctx.call(CoreCommand::ListRuntimeCatalog)?;
            if !ctx.print_json(&r) {
                if let CoreResponse::RuntimeCatalog { entries } = r {
                    let mut rows = vec![vec![
                        "ID".into(),
                        "NAME".into(),
                        "VERSION".into(),
                        "STATE".into(),
                    ]];
                    rows.extend(entries.into_iter().map(|e| {
                        vec![
                            e.id,
                            e.name,
                            e.version,
                            if e.installed {
                                "installed".into()
                            } else {
                                "available".into()
                            },
                        ]
                    }));
                    table(rows);
                }
            }
        }
        RuntimeCmd::Install { id, version } => {
            install(ctx, &id, version.as_deref())?
        }
    }
    Ok(())
}

pub fn php(ctx: &Ctx, cmd: PhpCmd) -> R<()> {
    match cmd {
        PhpCmd::Use { version } => {
            ctx.call(CoreCommand::SetSetting {
                key: "runtime.php.global".into(),
                value: serde_json::json!(version),
            })?;
            println!("PHP {version} is now the default for projects that don't ask for a version.");
        }
    }
    Ok(())
}

pub fn install(ctx: &Ctx, id: &str, version: Option<&str>) -> R<()> {
    let CoreResponse::RuntimeCatalog { entries } = ctx.call(CoreCommand::ListRuntimeCatalog)?
    else {
        return Err("unexpected reply".into());
    };
    let candidates: Vec<_> = entries
        .iter()
        .filter(|e| {
            e.id == id
                && version.is_none_or(|v| e.version == v || e.version.starts_with(&format!("{v}.")))
        })
        .collect();
    let entry = candidates.last().ok_or_else(|| {
        let have: Vec<String> = entries
            .iter()
            .filter(|e| e.id == id)
            .map(|e| e.version.clone())
            .collect();
        if have.is_empty() {
            format!("\"{id}\" is not in the catalog (see `olsc runtime list`)")
        } else {
            format!(
                "{id} {} is not available; available: {}",
                version.unwrap_or(""),
                have.join(", ")
            )
        }
    })?;
    if entry.installed {
        println!("{} {} is already installed.", entry.name, entry.version);
        return Ok(());
    }
    let (name, v) = (entry.name.clone(), entry.version.clone());
    ctx.call(CoreCommand::InstallRuntime {
        id: id.into(),
        version: v.clone(),
    })?;
    print!("Installing {name} {v}");
    let started = Instant::now();
    loop {
        std::thread::sleep(Duration::from_secs(2));
        print!(".");
        let _ = std::io::stdout().flush();
        if let CoreResponse::RuntimeCatalog { entries } =
            ctx.call(CoreCommand::ListRuntimeCatalog)?
        {
            if entries
                .iter()
                .any(|e| e.id == id && e.version == v && e.installed)
            {
                println!(" done.");
                return Ok(());
            }
        }
        if started.elapsed() > Duration::from_secs(45 * 60) {
            println!();
            return Err("the install did not finish in 45 minutes; see the log".into());
        }
    }
}



pub fn service(ctx: &Ctx, cmd: ServiceCmd) -> R<()> {
    match cmd {
        ServiceCmd::List => {
            let r = ctx.call(CoreCommand::ListServices)?;
            if !ctx.print_json(&r) {
                if let CoreResponse::Services { services } = r {
                    let mut rows = vec![vec![
                        "ID".into(),
                        "NAME".into(),
                        "STATE".into(),
                        "PORT".into(),
                    ]];
                    rows.extend(services.into_iter().map(|s| {
                        let state = if !s.installed {
                            "not installed"
                        } else if s.running {
                            "running"
                        } else {
                            "stopped"
                        };
                        vec![
                            s.id,
                            s.name,
                            state.into(),
                            s.port.map(|p| p.to_string()).unwrap_or_default(),
                        ]
                    }));
                    table(rows);
                }
            }
        }
        ServiceCmd::Start { id } => {
            ctx.call(CoreCommand::StartService { id: id.clone() })?;
            println!("Starting {id}.");
        }
        ServiceCmd::Stop { id } => {
            ctx.call(CoreCommand::StopService { id: id.clone() })?;
            println!("Stopped {id}.");
        }
        ServiceCmd::Restart { id } => {
            ctx.call(CoreCommand::RestartService { id: id.clone() })?;
            println!("Restarted {id}.");
        }
        ServiceCmd::Logs { id, lines } => {
            let CoreResponse::LogSources { sources } = ctx.call(CoreCommand::ListLogSources)?
            else {
                return Ok(());
            };
            let CoreResponse::Services { services } = ctx.call(CoreCommand::ListServices)? else {
                return Ok(());
            };
            let name = services
                .iter()
                .find(|s| s.id == id)
                .map(|s| s.name.clone())
                .unwrap_or(id.clone());
            // The service names its own log source; fall back to a same-named
            // process (older cores without `log_source`) and then to a web log.
            let source = services
                .iter()
                .find(|s| s.id == id)
                .and_then(|s| s.log_source.clone())
                .and_then(|wanted| {
                    sources
                        .iter()
                        .find(|s| s.id == wanted)
                        .map(|s| s.id.clone())
                })
                .or_else(|| {
                    sources
                        .iter()
                        .rfind(|s| s.kind == "process" && s.name.eq_ignore_ascii_case(&name))
                        .map(|s| s.id.clone())
                })
                .or_else(|| {
                    sources
                        .iter()
                        .find(|s| s.id == id || (id.starts_with("web") && s.id == "web:error"))
                        .map(|s| s.id.clone())
                })
                .ok_or_else(|| {
                    format!("{name} has no log yet (it hasn't run since the app started)")
                })?;
            if let CoreResponse::LogLines { lines, .. } = ctx.call(CoreCommand::ReadLog {
                source: source.clone(),
                max_lines: lines,
            })? {
                for l in lines {
                    println!("{l}");
                }
            }
        }
    }
    Ok(())
}



pub fn tunnel(ctx: &Ctx, cmd: TunnelCmd) -> R<()> {
    let CoreResponse::Tunnels { tunnels } = ctx.call(CoreCommand::ListTunnels)? else {
        return Err("unexpected reply".into());
    };
    let find = |name: &str| -> R<ols_core::tunnel::TunnelStatus> {
        let pid = ctx.project_id(name).ok();
        tunnels
            .iter()
            .find(|t| {
                t.config.id == name
                    || t.config.name.eq_ignore_ascii_case(name)
                    || (pid.is_some() && t.config.project_id == pid)
            })
            .cloned()
            .ok_or_else(|| format!("no tunnel named \"{name}\"; create one on the Tunnels page"))
    };
    match cmd {
        TunnelCmd::List => {
            let mut rows = vec![vec![
                "NAME".into(),
                "PROVIDER".into(),
                "STATE".into(),
                "TARGET".into(),
                "PUBLIC URL".into(),
            ]];
            rows.extend(tunnels.iter().map(|t| {
                vec![
                    t.config.name.clone(),
                    t.config.provider.clone(),
                    t.state.clone(),
                    t.config.target.clone(),
                    t.public_url.clone().unwrap_or_default(),
                ]
            }));
            table(rows);
        }
        TunnelCmd::Start { name, yes } => {
            let t = find(&name)?;
            let mut r = ctx.call(CoreCommand::StartTunnel {
                id: t.config.id.clone(),
                confirm_exposure: false,
            })?;
            if let CoreResponse::Tunnel { tunnel } = &r {
                if tunnel.state == "needs_confirmation" {
                    println!("{}", tunnel.exposure);
                    if !yes && !confirm("Make it public?") {
                        println!("Not started.");
                        return Ok(());
                    }
                    r = ctx.call(CoreCommand::StartTunnel {
                        id: t.config.id.clone(),
                        confirm_exposure: true,
                    })?;
                }
            }
            // Wait briefly for the provider's address.
            for _ in 0..40 {
                if let CoreResponse::Tunnels { tunnels } = ctx.call(CoreCommand::ListTunnels)? {
                    if let Some(s) = tunnels.into_iter().find(|x| x.config.id == t.config.id) {
                        if let Some(url) = s.public_url {
                            println!(
                                "PUBLIC: {url} → {}\nStop it with: ols tunnel stop {}",
                                s.config.target, s.config.name
                            );
                            return Ok(());
                        }
                        if let Some(e) = s.error {
                            return Err(e);
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(500));
            }
            let _ = r;
            println!(
                "Started; the provider hasn't given an address yet. Check with `olsc tunnel list`."
            );
        }
        TunnelCmd::Stop { name } => {
            let t = find(&name)?;
            ctx.call(CoreCommand::StopTunnel { id: t.config.id })?;
            println!("{} is no longer public.", t.config.name);
        }
    }
    Ok(())
}

/// Prints a process's output until it exits.


pub fn follow(ctx: &Ctx, id: ols_core::process::ProcessId) -> R<()> {
    let mut printed = 0usize;
    while let CoreResponse::ProcessOutput { lines, .. } =
        ctx.call(CoreCommand::GetProcessOutput { id })?
    {
        if lines.len() < printed {
            printed = 0;
        }
        for l in &lines[printed..] {
            println!("{l}");
        }
        printed = lines.len();
        let CoreResponse::Processes { processes } = ctx.call(CoreCommand::ListProcesses)? else {
            break;
        };
        match processes.into_iter().find(|p| p.id == id) {
            Some(p)
                if matches!(
                    p.state,
                    ols_core::process::ProcessState::Running
                        | ols_core::process::ProcessState::Starting
                ) =>
            {
                std::thread::sleep(Duration::from_millis(400))
            }
            Some(p) => {
                return match p.exit_code {
                    Some(0) | None => Ok(()),
                    Some(c) => Err(format!("exited with code {c}")),
                };
            }
            None => break,
        }
    }
    Ok(())
}


