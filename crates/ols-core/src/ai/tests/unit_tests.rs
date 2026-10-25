use super::super::*;
use super::mock::*;
use crate::command::CoreCommand;
use std::collections::BTreeMap;
#[test]
fn only_loopback_addresses_are_local() {
    for url in [
        "http://localhost:1234/v1",
        "http://127.0.0.1:11434/v1",
        "http://[::1]:8080/v1",
        "http://LOCALHOST/v1",
    ] {
        assert!(is_local_url(url), "{url}");
    }
    for url in [
        "https://openrouter.ai/api/v1",
        "http://192.168.1.20:1234/v1",
        "https://localhost.evil.example/v1",
        "not a url",
    ] {
        assert!(!is_local_url(url), "{url}");
    }
}

#[test]
fn fenced_blocks_are_found_and_cut_out() {
    let text = "Start it.\n```ols-plan\n[{\"label\":\"x\"}]\n```\nDone.\n```yaml\na: 1\n```";
    let f = fences(text);
    assert_eq!(f.len(), 2);
    assert_eq!(f[0].lang, "ols-plan");
    assert_eq!(f[1].body, "a: 1");
    assert_eq!(without_fences(text, &[&f[0], &f[1]]), "Start it.\nDone.");
}

#[test]
fn a_plan_may_only_hold_allowlisted_commands() {
    let (core, _home) = setup("http://127.0.0.1:9/v1", true);
    let i = core.inner();
    let plan = r#"```ols-plan
[
 {"label":"Start MariaDB","command":{"type":"start_service","id":"mariadb"}},
 {"label":"Wipe","command":{"type":"restore_database","engine":"mariadb","database":"a","file":"x.sql"}},
 {"label":"Shell","command":{"type":"run_command","executable":"cmd","args":[],"cwd":null,"timeout_ms":1000}},
 {"label":"Secrets","command":{"type":"get_secret","key":"x"}},
 {"label":"Nonsense","command":{"type":"launch_rockets"}},
 {"label":"Set host","command":{"type":"set_env_value","project_id":"p","file":".env","key":"DB_HOST","value":"127.0.0.1"}}
]
```"#;
    let (actions, rejected) = i.ai_check_plan(plan);
    let kinds: Vec<&str> = actions.iter().map(|a| a.label.as_str()).collect();
    assert_eq!(kinds, ["Start MariaDB", "Wipe", "Set host"], "{rejected:?}");
    assert!(
        !actions[0].destructive && actions[1].destructive && actions[2].destructive,
        "restore_database and set_env_value need their own confirmation"
    );
    assert_eq!(rejected.len(), 3, "{rejected:?}");
    assert!(
        rejected.iter().any(|r| r.contains("run_command"))
            && rejected.iter().any(|r| r.contains("get_secret"))
            && rejected.iter().any(|r| r.contains("launch_rockets"))
    );
    assert_eq!(i.ai_check_plan("```ols-plan\nnot json\n```").1.len(), 1);
}

#[test]
fn approved_steps_are_checked_again_and_destructive_ones_need_their_own_confirmation() {
    let (core, _home) = setup("http://127.0.0.1:9/v1", true);
    let steps = core.ai_apply(
        vec![
            CoreCommand::GetSecret { key: "x".into() },
            CoreCommand::RunCommand {
                executable: "cmd".into(),
                args: vec![],
                cwd: None,
                timeout_ms: 1000,
            },
            CoreCommand::SetEnvValue {
                project_id: "p".into(),
                file: ".env".into(),
                key: "A".into(),
                value: "b".into(),
            },
        ],
        false,
    );
    assert!(steps.iter().all(|s| !s.ok));
    assert!(steps[0].detail.contains("refused") && steps[1].detail.contains("refused"));
    assert!(steps[2].detail.contains("confirm"), "{}", steps[2].detail);
}

#[test]
fn an_imported_quick_app_never_gets_its_approval_from_a_plan() {
    let cmd = normalise(CoreCommand::StartQuickApp {
        id: "x".into(),
        values: BTreeMap::new(),
        approval: Some("source".into()),
        allow_elevated: true,
    });
    assert!(matches!(
        cmd,
        CoreCommand::StartQuickApp {
            approval: None,
            allow_elevated: false,
            ..
        }
    ));
}

#[test]
fn providers_are_validated_and_features_can_pick_their_own() {
    let (core, _home) = setup("http://127.0.0.1:9/v1", true);
    let i = core.inner();
    let bad = |kind: &str, url: &str| {
        i.ai_save_provider(
            AiProvider {
                id: String::new(),
                name: "X".into(),
                kind: kind.into(),
                base_url: url.into(),
                model: String::new(),
                tools: true,
                local: false,
                has_key: false,
            },
            None,
        )
    };
    assert!(bad("gpt", "http://localhost/v1").is_err());
    assert!(bad("custom", "ftp://localhost/v1").is_err());
    assert!(bad("custom", "localhost").is_err());
    let state = bad("lmstudio", "http://localhost:1234/v1/").unwrap();
    assert_eq!(state.settings.providers.len(), 2);
    assert_eq!(
        state.settings.providers[1].base_url,
        "http://localhost:1234/v1"
    );
    assert!(state.settings.providers[1].local);
    assert!(i
        .ai_save_settings(
            true,
            BTreeMap::from([("assistant".to_string(), "missing".to_string())])
        )
        .is_err());
    assert!(i
        .ai_save_settings(
            true,
            BTreeMap::from([("dance".to_string(), "mock".to_string())])
        )
        .is_err());
    let state = i.ai_save_settings(
        true,
        BTreeMap::from([("assistant".to_string(), "x".to_string())]),
    );
    assert!(state.is_ok(), "the override was saved");
    assert_eq!(
        pick_provider(&i.ai_settings(), "assistant").unwrap().id,
        "x",
        "the override wins"
    );
    let after = i.ai_remove_provider("x").unwrap();
    assert!(after.settings.features.is_empty());
}

#[test]
fn a_key_is_never_stored_for_or_sent_over_plain_http_to_another_machine() {
    let (core, _home) = setup("http://127.0.0.1:9/v1", true);
    let e = core
        .inner()
        .ai_save_provider(
            AiProvider {
                id: String::new(),
                name: "LAN".into(),
                kind: "custom".into(),
                base_url: "http://192.168.1.5:8000/v1".into(),
                model: "m".into(),
                tools: true,
                local: false,
                has_key: false,
            },
            Some("sk-abcdefghijklmnopqrstuv".into()),
        )
        .unwrap_err()
        .to_string();
    assert!(e.contains("https"), "{e}");
    let p = AiProvider {
        id: "x".into(),
        name: "x".into(),
        kind: "custom".into(),
        base_url: "http://192.168.1.5:8000/v1".into(),
        model: "m".into(),
        tools: true,
        local: false,
        has_key: false,
    };
    assert!(authorize(
        client(false, None).get("http://192.168.1.5/"),
        &p,
        Some("k")
    )
    .is_err());
    assert!(authorize(client(false, None).get("http://192.168.1.5/"), &p, None).is_ok());
}

#[test]
fn every_feature_needs_what_it_asks_about() {
    let (core, _home) = setup("http://127.0.0.1:9/v1", true);
    let i = core.inner();
    // Unknown feature is refused.
    let r = AiRequest {
        feature: "dance".into(),
        question: Some("why".into()),
        ..Default::default()
    };
    assert!(i.ai_prompt(&r).is_err(), "unknown feature must be refused");
    // Assistant needs a question.
    let r = AiRequest {
        feature: "assistant".into(),
        ..Default::default()
    };
    assert!(
        i.ai_prompt(&r).is_err(),
        "assistant without a question must be refused"
    );
    let logs = AiRequest {
        feature: "assistant".into(),
        question: Some("why 502?".into()),
        ..Default::default()
    };
    let p = i.ai_prompt(&logs).unwrap();
    assert!(
        p.messages[0].content.contains("ols-plan")
            && p.messages[0]
                .content
                .contains("OpenLocalServer (OLS) Assistant"),
        "assistant answers propose plans"
    );
}
