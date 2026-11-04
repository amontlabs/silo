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
        "silo",
        "--socket",
        socket,
        "--allow-other-channel",
        "exec",
        "main",
        "--",
        "false",
    ])
    .unwrap();
    assert_eq!(run(cli, Path::new("/")).await.unwrap(), 3);

    let cli = <Cli as clap::Parser>::try_parse_from([
        "silo",
        "--socket",
        socket,
        "--allow-other-channel",
        "drop",
        "main",
    ])
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

#[tokio::test]
async fn exec_rejects_timeouts_above_the_engine_cap() {
    let (engine, _) = engine_with(&["main"]);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    for timeout_secs in [0, ops::MAX_EXEC_SECS + 1] {
        let args = ops::ExecArgs {
            computer: "main".into(),
            argv: vec!["sleep".into(), "1".into()],
            cwd: None,
            timeout_secs,
        };
        let error = ops::exec(&connected, &args).await.unwrap_err();
        assert!(error.message.contains("180"), "{}", error.message);
    }
    assert!(engine.calls("guest.exec").is_empty());
    let args = ops::ExecArgs {
        computer: "main".into(),
        argv: vec!["true".into()],
        cwd: None,
        timeout_secs: ops::MAX_EXEC_SECS,
    };
    ops::exec(&connected, &args).await.unwrap();
    assert_eq!(engine.calls("guest.exec")[0]["timeoutMs"], 180_000);
}

#[tokio::test]
async fn exec_output_keeps_raw_bytes() {
    let engine = FakeEngine::start(|method, _| match method {
        "computers.list" => Ok(listing(&[("main", "running")])),
        "guest.exec" => Ok(json!({"exitCode": null, "signal": "KILL",
            "stdout": [255, 104], "stderr": []})),
        other => Err(("internal", format!("unexpected {other}"))),
    });
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let args = ops::ExecArgs {
        computer: "main".into(),
        argv: vec!["x".into()],
        cwd: None,
        timeout_secs: 5,
    };
    let output = ops::exec_raw(&connected, &args).await.unwrap();
    assert_eq!(output.stdout, [255, 104]);
    assert_eq!(output.process_exit_code(), 137);
}

#[tokio::test]
async fn fork_sends_named_secrets_and_the_recipe_quiet_lists() {
    let (engine, _) = engine_with(&["main"]);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let args = ForkArgs {
        source: "main".into(),
        secrets: vec!["github".into()],
        recipe: Some(
            silo_cli::config::parse_recipe("quiet: [poller]\nscrub: [/root/.bot.env]\n").unwrap(),
        ),
        ..ForkArgs::default()
    };
    ops::fork(&connected, &args, None, &ignore).await.unwrap();
    let sent = &engine.calls("checkpoints.fork")[0];
    assert_eq!(sent["secrets"], json!(["github"]));
    assert_eq!(sent["quiet"]["stop"], json!(["poller"]));
    assert_eq!(sent["quiet"]["scrub"], json!(["/root/.bot.env"]));
}

#[tokio::test]
async fn the_command_line_finds_the_project_recipe() {
    let (engine, _) = engine_with(&["main"]);
    let socket = engine.socket.to_str().unwrap();
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(project.path().join(".silo")).unwrap();
    std::fs::write(
        project.path().join(".silo/recipe.yaml"),
        "quiet: [poller]\n",
    )
    .unwrap();
    let nested = project.path().join("src");
    std::fs::create_dir_all(&nested).unwrap();
    let cli = <Cli as clap::Parser>::try_parse_from([
        "silo",
        "--socket",
        socket,
        "--allow-other-channel",
        "fork",
        "main",
    ])
    .unwrap();
    run(cli, &nested).await.unwrap();
    assert_eq!(
        engine.calls("checkpoints.fork")[0]["quiet"]["stop"],
        json!(["poller"])
    );

    std::fs::write(project.path().join(".silo/recipe.yaml"), "bogus: 1\n").unwrap();
    let cli = <Cli as clap::Parser>::try_parse_from([
        "silo",
        "--socket",
        socket,
        "--allow-other-channel",
        "fork",
        "main",
    ])
    .unwrap();
    let error = run(cli, &nested).await.unwrap_err();
    assert!(error.message.contains("bogus"), "{}", error.message);
}

#[tokio::test]
async fn the_command_line_refuses_a_socket_outside_its_home() {
    let (engine, _) = engine_with(&["main"]);
    let socket = engine.socket.to_str().unwrap();
    let cli = <Cli as clap::Parser>::try_parse_from(["silo", "--socket", socket, "list"]).unwrap();
    let error = run(cli, Path::new("/")).await.unwrap_err();
    assert!(error.message.contains("--allow-other-channel"));
    assert_eq!(engine.accepted(), 0);
}

#[tokio::test]
async fn losing_the_connection_names_the_job_and_how_to_reattach() {
    let engine = FakeEngine::start(|method, _| match method {
        "computers.list" => Ok(listing(&[("main", "running")])),
        "computers.stop" => Ok(json!({"jobId": "job:7"})),
        "operations.job.get" => Err(("__drop", String::new())),
        other => Err(("internal", format!("unexpected {other}"))),
    });
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let error = ops::stop(&connected, "main", &ignore).await.unwrap_err();
    assert!(error.message.contains("job:7"), "{}", error.message);
    assert!(
        error.message.contains("silo job job:7"),
        "{}",
        error.message
    );
    assert_eq!(connected.last_job().as_deref(), Some("job:7"));
}

#[tokio::test]
async fn silo_job_waits_for_a_running_job() {
    let (engine, _) = engine_with(&["main"]);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let result = ops::job(&connected, "job:1", &ignore).await.unwrap();
    assert_eq!(result["checkpoint"]["id"], "cp9");
    assert!(ops::job(&connected, "nope", &ignore).await.is_err());
}

#[tokio::test]
async fn an_acknowledgement_failure_is_reported() {
    let engine = FakeEngine::start(|method, _| match method {
        "operations.job.get" => Ok(finished(json!({"ok": true}))),
        "operations.job.acknowledge" => Err(("internal", "no such job".into())),
        other => Err(("internal", format!("unexpected {other}"))),
    });
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let notes = Mutex::new(Vec::<String>::new());
    let record = |line: &str| notes.lock().unwrap().push(line.to_owned());
    let result = ops::job(&connected, "job:1", &record).await.unwrap();
    assert_eq!(result["ok"], true);
    assert!(notes
        .lock()
        .unwrap()
        .iter()
        .any(|line| line.contains("could not acknowledge")));
}

#[tokio::test]
async fn dropping_the_engine_closes_the_connection() {
    let (engine, _) = engine_with(&["main"]);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    ops::list(&connected).await.unwrap();
    assert_eq!(engine.open(), 1);
    drop(connected);
    for _ in 0..50 {
        if engine.open() == 0 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(engine.open(), 0);
}
