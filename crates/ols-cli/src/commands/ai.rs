use std::time::Duration;
use ols_core::command::{CoreCommand, CoreResponse};
use crate::args::{AiCmd, AiRun};
use crate::ctx::{Ctx, R};

pub fn ai_cmd(ctx: &Ctx, cmd: AiCmd) -> R<()> {
    let state = || -> R<ols_core::ai::AiState> {
        match ctx.call(CoreCommand::AiGetState)? {
            CoreResponse::AiState { state } => Ok(*state),
            _ => Err("unexpected reply".into()),
        }
    };
    match cmd {
        AiCmd::Status => {
            let st = state()?;
            if ctx.json {
                println!("{}", serde_json::to_string_pretty(&st).unwrap_or_default());
                return Ok(());
            }
            println!(
                "AI assistant: {}",
                if st.settings.enabled {
                    "on"
                } else {
                    "off (turn it on with `olsc ai on`)"
                }
            );
            if st.settings.providers.is_empty() {
                println!("No provider yet. Add LM Studio, Hugging Face, OpenRouter or another server in Settings → AI assistant.");
            }
            for p in &st.settings.providers {
                println!(
                    "  {} [{}] {} model {} {}",
                    p.id,
                    p.kind,
                    p.base_url,
                    if p.model.is_empty() {
                        "(none chosen)"
                    } else {
                        &p.model
                    },
                    if p.local {
                        "(on this computer)"
                    } else {
                        "(sends data off this computer)"
                    }
                );
            }
            for f in &st.features {
                if let Some(p) = st.settings.features.get(&f.id) {
                    println!("  {} -> {p}", f.label);
                }
            }
        }
        AiCmd::On | AiCmd::Off => {
            let enabled = matches!(cmd, AiCmd::On);
            let st = state()?;
            ctx.call(CoreCommand::AiSaveSettings {
                enabled,
                features: st.settings.features,
            })?;
            println!(
                "The AI assistant is {}.",
                if enabled { "on" } else { "off" }
            );
        }
        AiCmd::Test { provider } => {
            let st = state()?;
            let id = match provider {
                Some(p) => p,
                None => st
                    .settings
                    .providers
                    .first()
                    .map(|p| p.id.clone())
                    .ok_or("no provider is set up")?,
            };
            let CoreResponse::AiTest { result } =
                ctx.call(CoreCommand::AiTest { provider_id: id })?
            else {
                return Err("unexpected reply".into());
            };
            println!("{} ({} ms)", result.message, result.ms);
            for m in result.models.iter().take(30) {
                println!("  {}", m.id);
            }
            if !result.ok {
                return Err("the provider didn't answer".into());
            }
        }
        AiCmd::Ask {
            question,
            feature,
            logs,
            run,
        } => {
            let question = question.join(" ");
            if question.trim().is_empty() {
                return Err(
                    "ask a question, for example: ols ai ask \"why does shop.test return 502?\""
                        .into(),
                );
            }
            if !matches!(feature.as_str(), "logs" | "palette") {
                return Err("--feature is logs or palette".into());
            }
            ai_run(
                ctx,
                ols_core::ai::AiRequest {
                    feature,
                    question: Some(question),
                    log_sources: logs,
                    ..Default::default()
                },
                run,
            )?;
        }
        AiCmd::Explain { finding, run } => {
            let CoreResponse::Diagnostics { findings } = ctx.call(CoreCommand::RunDiagnostics)?
            else {
                return Err("unexpected reply".into());
            };
            let needle = finding.to_lowercase();
            let hits: Vec<_> = findings
                .iter()
                .filter(|f| {
                    f.id.to_lowercase() == needle
                        || f.id.to_lowercase().contains(&needle)
                        || f.problem.to_lowercase().contains(&needle)
                })
                .collect();
            let f = match hits.as_slice() {
                [one] => *one,
                [] => {
                    return Err(format!(
                        "no finding matches '{finding}'. Findings: {}",
                        findings
                            .iter()
                            .map(|f| f.id.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ))
                }
                many => {
                    return Err(format!(
                        "'{finding}' matches several findings: {}",
                        many.iter()
                            .map(|f| f.id.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ))
                }
            };
            let text = format!(
                "{}\nCause: {}\nSuggested fix: {}\n{}",
                f.problem,
                f.cause,
                f.fix,
                f.details.join("\n")
            );
            ai_run(
                ctx,
                ols_core::ai::AiRequest {
                    feature: "explain".into(),
                    title: Some(f.problem.clone()),
                    text: Some(text),
                    ..Default::default()
                },
                run,
            )?;
        }
    }
    Ok(())
}

/// Starts a request, streams the answer, then offers the proposed steps for approval.


pub fn ai_run(ctx: &Ctx, request: ols_core::ai::AiRequest, run: AiRun) -> R<()> {
    use std::io::{IsTerminal, Write};
    if run.preview {
        let CoreResponse::AiPrompt { prompt } = ctx.call(CoreCommand::AiPreview { request })?
        else {
            return Err("unexpected reply".into());
        };
        println!(
            "To {} ({}), model {}, {}:\n",
            prompt.provider_name,
            prompt.host,
            prompt.model,
            if prompt.local {
                "on this computer"
            } else {
                "OUTSIDE this computer"
            }
        );
        for m in &prompt.messages {
            println!("--- {} ---\n{}\n", m.role, m.content);
        }
        println!(
            "Tools the model may call (read-only): {}",
            if prompt.tools.is_empty() {
                "none".to_string()
            } else {
                prompt.tools.join(", ")
            }
        );
        return Ok(());
    }
    let CoreResponse::AiJob { job } = ctx
        .call(CoreCommand::AiStart {
            request,
            confirm_remote: run.yes,
        })
        .map_err(|e| {
            if e.contains("outside this computer") {
                format!("{e} (pass --yes to send it)")
            } else {
                e
            }
        })?
    else {
        return Err("unexpected reply".into());
    };
    let (id, mut shown, mut seen_activity) = (job.id, 0usize, 0usize);
    if !ctx.json && !job.local {
        eprintln!("Sending to {} (outside this computer).", job.provider);
    }
    let last = loop {
        let CoreResponse::AiJob { job } = ctx.call(CoreCommand::AiJob { job_id: id.clone() })?
        else {
            return Err("unexpected reply".into());
        };
        if !ctx.json {
            for line in job.activity.iter().skip(seen_activity) {
                eprintln!("  {line}");
            }
            seen_activity = job.activity.len();
            if job.partial.len() >= shown && job.partial.is_char_boundary(shown) {
                print!("{}", &job.partial[shown..]);
                shown = job.partial.len();
                let _ = std::io::stdout().flush();
            }
        }
        if job.state != "running" {
            break *job;
        }
        std::thread::sleep(Duration::from_millis(300));
    };
    if ctx.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&last).unwrap_or_default()
        );
    }
    if last.state != "done" {
        return Err(last
            .error
            .unwrap_or_else(|| format!("the request {}", last.state)));
    }
    let a = last.answer.ok_or("the model gave no answer")?;
    if ctx.json {
        return Ok(());
    }
    println!();
    for r in &a.rejected {
        println!("Refused: {r}");
    }
    let tokens = match (a.tokens_in, a.tokens_out) {
        (Some(i), Some(o)) => format!("{i} in, {o} out"),
        _ => "tokens not reported".into(),
    };
    println!(
        "\n[{} · {} · {tokens}{}]",
        a.provider,
        a.model,
        a.cost_usd
            .map(|c| format!(" · ${c:.4}"))
            .unwrap_or_default()
    );
    if a.actions.is_empty() {
        return Ok(());
    }
    println!("\nProposed steps (nothing has run):");
    for (n, s) in a.actions.iter().enumerate() {
        println!(
            "  {}. {}{}",
            n + 1,
            s.label,
            if s.destructive {
                "  [replaces or removes something]"
            } else {
                ""
            }
        );
    }
    let ask = |q: &str| -> bool {
        if !std::io::stdin().is_terminal() {
            return false;
        }
        print!("{q} [y/N] ");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).is_ok()
            && matches!(line.trim().to_lowercase().as_str(), "y" | "yes")
    };
    let has_destructive = a.actions.iter().any(|s| s.destructive);
    let go = run.apply || ask("Run these steps?");
    if !go {
        println!("Not run. Use --apply to run them.");
        return Ok(());
    }
    let confirm_destructive = has_destructive
        && (run.destructive || ask("Some steps replace or remove something. Run those too?"));
    let CoreResponse::AiApplied { steps } = ctx.call(CoreCommand::AiApply {
        actions: a.actions.iter().map(|s| s.command.clone()).collect(),
        confirm_destructive,
    })?
    else {
        return Err("unexpected reply".into());
    };
    let mut failed = false;
    for s in steps {
        failed |= !s.ok;
        println!(
            "  {} {}: {}",
            if s.ok { "✓" } else { "✗" },
            s.label,
            s.detail
        );
    }
    if failed {
        return Err("some steps didn't run".into());
    }
    Ok(())
}

