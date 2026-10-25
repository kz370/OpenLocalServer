use super::super::*;
use crate::command::{Core, CoreCommand};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::net::TcpListener;

use super::mock::*;
#[test]
fn nothing_is_sent_while_the_assistant_is_off_or_to_a_remote_provider_without_confirmation() {
    let mock = Mock::start(|_, _, _| sse(&["hi"]));
    let (core, _home) = setup(&mock.url(), false);
    let e = core
        .inner()
        .ai_start(explain("boom"), false)
        .unwrap_err()
        .to_string();
    assert!(e.contains("off"), "{e}");
    assert!(mock.bodies().is_empty());

    // A provider outside this computer needs a confirmation for each request; the address is never contacted.
    let i = core.inner();
    i.ai_save_provider(
        AiProvider {
            id: String::new(),
            name: "Cloud".into(),
            kind: "openrouter".into(),
            base_url: "https://openrouter.ai/api/v1".into(),
            model: "m".into(),
            tools: true,
            local: false,
            has_key: false,
        },
        None,
    )
    .unwrap();
    i.ai_save_settings(
        true,
        BTreeMap::from([("assistant".to_string(), "cloud".to_string())]),
    )
    .unwrap();
    let e = i.ai_start(explain("boom"), false).unwrap_err().to_string();
    assert!(e.contains("outside this computer"), "{e}");
    assert!(
        i.ai_prompt(&explain("boom")).unwrap().local == false,
        "the preview says where it goes"
    );
}

#[test]
fn what_is_sent_is_redacted_and_the_preview_matches_it() {
    let mock = Mock::start(|_, _, _| sse(&["Looks fine."]));
    let (core, _home) = setup(&mock.url(), true);
    let i = core.inner();
    let secret = "DB_PASSWORD=hunter2\nAuthorization: Bearer abcdefghijklmnop\nkey sk-abcdefghijklmnopqrstuv\nmysql://root:s3cret@127.0.0.1/db";
    let req = explain(secret);
    let preview = i.ai_prompt(&req).unwrap();
    let shown = preview
        .messages
        .iter()
        .map(|m| m.content.clone())
        .collect::<Vec<_>>()
        .join("\n");
    for leaked in [
        "hunter2",
        "abcdefghijklmnop",
        "sk-abcdefghijklmnopqrstuv",
        "s3cret",
    ] {
        assert!(!shown.contains(leaked), "{leaked} is in the preview");
    }
    assert!(
        preview.tools.contains(&"read_log".to_string()) && preview.local,
        "tools and local provider in preview"
    );

    let job = i.ai_start(req, false).unwrap();
    let done = wait(&core, &job.id);
    assert_eq!(done.state, "done", "{:?}", done.error);
    let sent = mock.bodies().join("\n");
    for leaked in [
        "hunter2",
        "abcdefghijklmnop",
        "sk-abcdefghijklmnopqrstuv",
        "s3cret",
    ] {
        assert!(!sent.contains(leaked), "{leaked} reached the provider");
    }
    let body: Value = serde_json::from_str(&mock.bodies()[0]).unwrap();
    assert_eq!(
        body["messages"][1]["content"],
        json!(preview.messages[1].content),
        "the request is what the preview showed"
    );
}

#[test]
fn an_answer_streams_and_carries_a_checked_plan() {
    let answer = "MariaDB isn't running, so the site can't reach its database.\n```ols-plan\n[{\"label\":\"Start MariaDB\",\"command\":{\"type\":\"start_service\",\"id\":\"mariadb\"}},{\"label\":\"Shell\",\"command\":{\"type\":\"run_command\",\"executable\":\"cmd\",\"args\":[],\"cwd\":null,\"timeout_ms\":1}}]\n```";
    let (first, rest) = (answer[..20].to_string(), answer[20..].to_string());
    let mock = Mock::start(move |_, _, _| sse(&[&first, &rest]));
    let (core, _home) = setup(&mock.url(), true);
    let job = core
        .inner()
        .ai_start(explain("connection refused 127.0.0.1:3306"), false)
        .unwrap();
    let done = wait(&core, &job.id);
    assert_eq!(done.state, "done", "{:?}", done.error);
    let a = done.answer.unwrap();
    assert!(
        a.text.starts_with("MariaDB isn't running") && !a.text.contains("ols-plan"),
        "{}",
        a.text
    );
    assert_eq!(a.actions.len(), 1);
    assert!(matches!(
        a.actions[0].command,
        CoreCommand::StartService { .. }
    ));
    assert_eq!(a.rejected.len(), 1, "{:?}", a.rejected);
    assert_eq!((a.tokens_in, a.tokens_out), (Some(120), Some(30)));
    assert!(a.local && a.cost_usd.is_none());
}

#[test]
fn the_model_can_read_through_tools_and_the_results_are_redacted() {
    let mock = Mock::start(|_, body, n| {
        if n == 1 {
            let call = json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"read_env_names","arguments":"{\"project_id\":\"p1\"}"}},{"index":1,"id":"c2","function":{"name":"list_services","arguments":"{}"}}]}}]});
            (
                200,
                "text/event-stream".into(),
                format!("data: {call}\n\ndata: [DONE]\n\n"),
            )
        } else {
            assert!(
                body.contains("\"role\":\"tool\"") && body.contains("c1") && body.contains("c2"),
                "the tool results go back: {body}"
            );
            sse(&["Done."])
        }
    });
    let (core, home) = setup(&mock.url(), true);
    let project = home.paths.root().join("proj");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join(".env"),
        "APP_KEY=base64:topsecretvalue\nDB_HOST=127.0.0.1\n",
    )
    .unwrap();
    let p = core
        .inner()
        .projects
        .lock()
        .unwrap()
        .register(project.to_str().unwrap())
        .unwrap();
    let mut req = explain("x");
    req.project_id = Some(p.id.clone());
    let job = core.inner().ai_start(req, false).unwrap();
    // The mock's first answer asks for project "p1", which doesn't exist: the error text goes back to the model.
    let done = wait(&core, &job.id);
    assert_eq!(done.state, "done", "{:?}", done.error);
    let a = done.answer.unwrap();
    assert!(
        a.used.iter().any(|u| u.starts_with("read_env_names"))
            && a.used.iter().any(|u| u.starts_with("list_services")),
        "{:?}",
        a.used
    );
    assert!(done.activity.iter().any(|l| l.starts_with("Read: ")));
    assert_eq!(mock.bodies().len(), 2);
    assert!(!mock.bodies().join("").contains("topsecretvalue"));
}

#[test]
fn env_names_are_read_with_their_values_hidden() {
    let (core, home) = setup("http://127.0.0.1:9/v1", true);
    let project = home.paths.root().join("proj");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join(".env"),
        "APP_KEY=base64:topsecretvalue\nDB_HOST=127.0.0.1\n",
    )
    .unwrap();
    let p = core
        .inner()
        .projects
        .lock()
        .unwrap()
        .register(project.to_str().unwrap())
        .unwrap();
    let out = core
        .inner()
        .ai_tool("read_env_names", &json!({"project_id": p.id}))
        .unwrap();
    assert!(
        out.contains("APP_KEY=[redacted]")
            && out.contains("DB_HOST=[redacted]")
            && !out.contains("topsecretvalue"),
        "{out}"
    );
    assert!(core
        .inner()
        .ai_tool(
            "read_env_names",
            &json!({"project_id": p.id, "file": "..\\x"})
        )
        .is_err());
    assert!(
        core.inner().ai_tool("run_shell", &json!({})).is_err(),
        "there is no shell tool"
    );
}

#[test]
fn a_server_that_refuses_tools_gets_the_request_again_without_them() {
    let mock = Mock::start(|_, body, _| {
        if body.contains("\"tools\"") {
            (
                400,
                "application/json".into(),
                json!({"error":{"message":"tools are not supported"}}).to_string(),
            )
        } else {
            sse(&["Plain answer."])
        }
    });
    let (core, _home) = setup(&mock.url(), true);
    let done = wait(
        &core,
        &core.inner().ai_start(explain("x"), false).unwrap().id,
    );
    assert_eq!(done.state, "done", "{:?}", done.error);
    assert_eq!(done.answer.unwrap().text, "Plain answer.");
    assert!(done
        .activity
        .iter()
        .any(|l| l.contains("continuing without them")));
    let bodies = mock.bodies();
    assert!(
        bodies.len() == 2 && bodies[0].contains("\"tools\"") && !bodies[1].contains("\"tools\"")
    );
}

#[test]
fn a_provider_with_tools_off_is_never_offered_any() {
    let mock = Mock::start(|_, _, _| sse(&["ok"]));
    let (core, _home) = setup(&mock.url(), true);
    let i = core.inner();
    let mut p = i.ai_settings().providers.remove(0);
    p.tools = false;
    i.ai_save_provider(p, None).unwrap();
    assert!(i.ai_prompt(&explain("x")).unwrap().tools.is_empty());
    let done = wait(&core, &i.ai_start(explain("x"), false).unwrap().id);
    assert_eq!(done.state, "done", "{:?}", done.error);
    assert!(!mock.bodies()[0].contains("\"tools\""));
}

#[test]
fn provider_errors_come_back_as_a_readable_failure() {
    let mock = Mock::start(|_, _, _| {
        (
            500,
            "application/json".into(),
            json!({"error":{"message":"model not loaded"}}).to_string(),
        )
    });
    let (core, _home) = setup(&mock.url(), true);
    let job = core.inner().ai_start(explain("x"), false).unwrap();
    let done = wait(&core, &job.id);
    assert_eq!(done.state, "failed");
    assert!(done.error.unwrap().contains("model not loaded"));
}

#[test]
fn a_running_request_can_be_cancelled() {
    // A server that never answers.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for s in listener.incoming().flatten() {
            held.push(s);
        }
    });
    let (core, _home) = setup(&format!("http://127.0.0.1:{port}/v1"), true);
    let job = core.inner().ai_start(explain("x"), false).unwrap();
    core.inner().ai_cancel(&job.id).unwrap();
    assert_eq!(wait(&core, &job.id).state, "cancelled");
    assert!(core.inner().ai_cancel("nope").is_err());
}

#[test]
fn the_connection_test_lists_models_with_prices() {
    let mock = Mock::start(|path, _, _| {
        assert!(path.ends_with("/v1/models"), "{path}");
        json_reply(
            json!({"data":[{"id":"b/model","name":"B","context_length":8000,"pricing":{"prompt":"0.000001","completion":"0.000002"}},{"id":"a/model"}]}),
        )
    });
    let (core, _home) = setup(&mock.url(), true);
    let i = core.inner();
    let id = i.ai_settings().providers[0].id.clone();
    let r = i.ai_test(&id).unwrap();
    assert!(r.ok, "{}", r.message);
    assert_eq!(r.models.len(), 2);
    assert_eq!(r.models[0].id, "a/model");
    let b = &r.models[1];
    assert_eq!(
        (b.context, b.prompt_per_m, b.completion_per_m),
        (Some(8000), Some(1.0), Some(2.0))
    );
    assert!(
        r.message.contains("doesn't list test-model"),
        "{}",
        r.message
    );
    let down = i
        .ai_save_provider(
            AiProvider {
                id: String::new(),
                name: "Down".into(),
                kind: "lmstudio".into(),
                base_url: "http://127.0.0.1:9/v1".into(),
                model: String::new(),
                tools: true,
                local: false,
                has_key: false,
            },
            None,
        )
        .unwrap();
    let down_id = down
        .settings
        .providers
        .iter()
        .find(|p| p.name == "Down")
        .unwrap()
        .id
        .clone();
    let r = i.ai_test(&down_id).unwrap();
    assert!(
        !r.ok && r.message.contains("Is the server running"),
        "{}",
        r.message
    );
}

#[test]
fn lm_studio_lists_every_downloaded_model_not_just_the_loaded_one() {
    let mock = Mock::start(|path, _, _| match path {
        "/api/v1/models" => json_reply(json!({"models":[
                {"type":"llm","key":"qwen-9b","display_name":"Qwen 9B","loaded_instances":[{"id":"x"}],"max_context_length":32768},
                {"type":"llm","key":"llama-8b","display_name":"Llama 8B","loaded_instances":[]},
                {"type":"embedding","key":"nomic-embed","display_name":"Nomic","loaded_instances":[]}]})),
        "/api/v0/models" => {
            json_reply(json!({"data":[{"id":"qwen-9b","type":"llm","state":"loaded"}]}))
        }
        _ => json_reply(json!({"data":[{"id":"qwen-9b"}]})),
    });
    let home = crate::test_support::isolated_home();
    let core = Core::new(
        crate::settings::SettingsService::load(&home.paths).unwrap(),
        home.paths.clone(),
    );
    let p = AiProvider {
        id: String::new(),
        name: "LM".into(),
        kind: "lmstudio".into(),
        base_url: mock.url(),
        model: String::new(),
        tools: true,
        local: true,
        has_key: false,
    };
    let r = core.inner().ai_probe(p.clone(), None).unwrap();
    let ids: Vec<&str> = r.models.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(
        ids,
        ["llama-8b", "qwen-9b"],
        "embedding models are left out"
    );
    assert_eq!(r.models[1].context, Some(32768));
    assert_eq!(r.models[1].name, "Qwen 9B (loaded)");
    assert_eq!(r.models[0].name, "Llama 8B");
    // Any other kind keeps to the OpenAI-style list.
    let other = core
        .inner()
        .ai_probe(
            AiProvider {
                kind: "custom".into(),
                ..p
            },
            None,
        )
        .unwrap();
    assert_eq!(other.models.len(), 1);
}
