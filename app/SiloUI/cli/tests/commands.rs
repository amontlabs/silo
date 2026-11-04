mod fake_engine;

use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use fake_engine::{finished, listing, FakeEngine};
use serde_json::{json, Value};
use silo_cli::{
    cli::{run, Cli},
    config::parse_settings,
    engine::Engine,
    ops::{self, ForkArgs},
};

fn ignore(_: &str) {}

/// An engine with a growing list of computers; jobs finish at once.
fn engine_with(names: &[&str]) -> (FakeEngine, Arc<Mutex<Vec<String>>>) {
    let state: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(
        names.iter().map(|name| (*name).to_owned()).collect(),
    ));
    let shared = state.clone();
    let engine = FakeEngine::start(move |method, params| match method {
        "computers.list" => {
            let names = shared.lock().unwrap().clone();
            let pairs: Vec<(&str, &str)> = names
                .iter()
                .map(|name| (name.as_str(), "running"))
                .collect();
            Ok(listing(&pairs))
        }
        "checkpoints.fork" => {
            shared
                .lock()
                .unwrap()
                .push(params["newName"].as_str().unwrap().to_owned());
            Ok(json!({"jobId": "job:1"}))
        }
        "computers.delete" => {
            let id = params["computerId"].as_str().unwrap();
            shared
                .lock()
                .unwrap()
                .retain(|name| format!("id-{name}") != id);
            Ok(json!({"jobId": "job:1"}))
        }
        "checkpoints.create" => Ok(json!({"jobId": "job:1"})),
        "operations.job.get" => Ok(finished(json!({
            "checkpoint": {"id": "cp9", "name": "saved", "kind": "manual"}
        }))),
        "operations.job.acknowledge" => Ok(Value::Null),
        "guest.exec" => Ok(json!({
            "exitCode": 3, "signal": null,
            "stdout": [104, 105], "stderr": [101]
        })),
        other => Err(("internal", format!("unexpected {other}"))),
    });
    (engine, state)
}

#[tokio::test]
async fn list_reports_the_computers() {
    let (engine, _) = engine_with(&["main", "other"]);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let value = ops::list(&connected).await.unwrap();
    let names: Vec<&str> = value["computers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|computer| computer["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["main", "other"]);
    assert_eq!(value["computers"][0]["state"], "running");
}

#[tokio::test]
async fn fork_names_the_source_and_waits_for_the_job() {
    let (engine, _) = engine_with(&["main"]);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let args = ForkArgs {
        source: "main".into(),
        checkpoint: Some("cp1".into()),
        ..ForkArgs::default()
    };
    let fork = ops::fork(&connected, &args, None, &ignore).await.unwrap();
    let name = fork["name"].as_str().unwrap();
    assert!(
        name.starts_with("main-") && name.len() == "main-".len() + 6,
        "{name}"
    );
    assert_eq!(fork["source"], "main");
    let sent = &engine.calls("checkpoints.fork")[0];
    assert_eq!(sent["computerId"], "id-main");
    assert_eq!(sent["checkpointId"], "cp1");
    assert_eq!(sent["newName"], name);
    assert_eq!(engine.calls("operations.job.acknowledge").len(), 1);
}

#[tokio::test]
async fn fork_refuses_secrets_outside_fork_secrets() {
    let (engine, _) = engine_with(&["main"]);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let settings = parse_settings("fork_secrets: [OPENAI_API_KEY]\n").unwrap();
    let args = ForkArgs {
        source: "main".into(),
        secrets: vec!["github".into()],
        ..ForkArgs::default()
    };
    let error = ops::fork(&connected, &args, Some(&settings), &ignore)
        .await
        .unwrap_err();
    assert!(error.message.contains("github"));
    assert!(engine.calls("checkpoints.fork").is_empty());
}

#[tokio::test]
async fn drop_refuses_main_and_quotes_the_configuration() {
    let (engine, state) = engine_with(&["main", "main-aaaaaa"]);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let refused = ops::drop_computer(&connected, "main", false, &ignore)
        .await
        .unwrap_err();
    assert!(refused.message.contains("--force"));
    assert!(engine.calls("computers.delete").is_empty());

    let dropped = ops::drop_computer(&connected, "main-aaaaaa", false, &ignore)
        .await
        .unwrap();
    assert_eq!(dropped["dropped"], "main-aaaaaa");
    let sent = &engine.calls("computers.delete")[0];
    assert_eq!(sent["computerId"], "id-main-aaaaaa");
    assert_eq!(sent["expected"]["name"], "main-aaaaaa");
    assert_eq!(state.lock().unwrap().as_slice(), ["main"]);
}

#[tokio::test]
async fn a_failed_job_reports_the_engine_message() {
    let engine = FakeEngine::start(|method, _| match method {
        "computers.list" => Ok(listing(&[("main", "running")])),
        "computers.stop" => Ok(json!({"jobId": "job:1"})),
        "operations.job.get" => Ok(json!({"jobId": "job:1", "state": "finished", "outcome": {
            "error": {"code": "internal", "message": "The disk is full."}}})),
        "operations.job.acknowledge" => Ok(Value::Null),
        other => Err(("internal", format!("unexpected {other}"))),
    });
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let error = ops::stop(&connected, "main", &ignore).await.unwrap_err();
    assert_eq!(error.message, "The disk is full.");
}

#[tokio::test]
async fn exec_returns_output_and_the_exit_code() {
    let (engine, _) = engine_with(&["main"]);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let args = ops::ExecArgs {
        computer: "main".into(),
        argv: vec!["echo".into(), "hi".into()],
        cwd: None,
        timeout_secs: 5,
    };
    let result = ops::exec(&connected, &args).await.unwrap();
    assert_eq!(result["exitCode"], 3);
    assert_eq!(result["stdout"], "hi");
    assert_eq!(result["stderr"], "e");
    let sent = &engine.calls("guest.exec")[0];
    assert_eq!(sent["argv"], json!(["echo", "hi"]));
    assert_eq!(sent["timeoutMs"], 5000);
}

#[tokio::test]
async fn the_command_line_passes_the_exit_code_through() {
    let (engine, _) = engine_with(&["main"]);
    let socket = engine.socket.to_str().unwrap();
    let cli = <Cli as clap::Parser>::try_parse_from([
        "silo", "--socket", socket, "exec", "main", "--", "false",
    ])
    .unwrap();
    assert_eq!(run(cli, Path::new("/")).await.unwrap(), 3);

    let cli = <Cli as clap::Parser>::try_parse_from(["silo", "--socket", socket, "drop", "main"])
        .unwrap();
    assert!(run(cli, Path::new("/")).await.is_err());
}

#[tokio::test]
async fn checkpoint_and_checkpoints_use_the_engine() {
    let (engine, _) = engine_with(&["main"]);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let kind = ops::parse_checkpoint_kind("before-keep").unwrap();
    let saved = ops::checkpoint(&connected, "main", Some("saved"), Some(kind), &ignore)
        .await
        .unwrap();
    assert_eq!(saved["id"], "cp9");
    let sent = &engine.calls("checkpoints.create")[0];
    assert_eq!(sent["name"], "saved");
    assert_eq!(sent["kind"], "before-keep");
    let listed = ops::checkpoints(&connected, "main").await.unwrap();
    assert_eq!(listed["checkpoints"][0]["id"], "cp1");
}

#[tokio::test]
async fn a_missing_engine_says_how_to_recover() {
    let error = Engine::connect(Path::new("/nonexistent/engine.sock"))
        .await
        .err()
        .unwrap();
    assert!(error.message.contains("Open Silo"));
}
