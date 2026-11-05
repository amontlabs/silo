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
    // The engine takes the fork-time record itself; the client asks for nothing more.
    assert!(engine.calls("changes.capture").is_empty());
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

/// An engine whose `main` and fork are listed with a quiet record, one secret and one
/// GitHub assignment, and that remembers saved secrets.
fn engine_with_secrets() -> FakeEngine {
    let secret = Arc::new(Mutex::new(json!({
        "id": "s1", "name": "API_KEY", "computerIds": ["id-main"],
        "allowedDomains": ["api.example.com"], "state": "active",
        "pendingComputerIds": [], "removing": false, "error": null
    })));
    FakeEngine::start(move |method, params| match method {
        "computers.list" => {
            let mut listed = listing(&[("main", "running"), ("fork", "running")]);
            listed["linux"]["computers"][1]["quiet"] = json!({
                "applied": true, "newIdentity": true, "scheduledJobsDisabled": true,
                "stop": ["poller"], "scrub": ["/root/.env"], "secrets": []
            });
            Ok(listed)
        }
        "secrets.list" => Ok(json!([secret.lock().unwrap().clone()])),
        "secrets.save" => {
            let request = &params["request"];
            let mut stored = secret.lock().unwrap();
            stored["computerIds"] = request["computerIds"].clone();
            stored["allowedDomains"] = request["allowedDomains"].clone();
            Ok(json!([stored.clone()]))
        }
        "github.state" => Ok(json!({"computers": [{
            "computerId": "id-fork", "repositoryMode": "selected",
            "repositories": [{"repository": "o/r", "allowPushes": false}],
            "allRepositoriesAllowChanges": false, "authenticationMethod": "oauth"
        }]})),
        other => Err(("internal", format!("unexpected {other}"))),
    })
}

#[tokio::test]
async fn status_shows_the_quiet_state_secrets_and_github() {
    let engine = engine_with_secrets();
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let fork = ops::status(&connected, "fork").await.unwrap();
    assert_eq!(fork["quiet"]["applied"], true);
    assert_eq!(fork["quiet"]["stop"][0], "poller");
    assert_eq!(fork["secrets"], json!([]));
    assert_eq!(fork["github"]["repositoryMode"], "selected");
    let text = silo_cli::render::status(&fork);
    assert!(
        text.contains("quiet: applied; stopped: poller; scrubbed: /root/.env; secrets: none"),
        "{text}"
    );
    assert!(text.contains("github: read-only (o/r)"), "{text}");
    let main = ops::status(&connected, "main").await.unwrap();
    assert!(main["quiet"].is_null());
    assert_eq!(main["secrets"], json!(["API_KEY"]));
    assert!(main["github"].is_null());
    let text = silo_cli::render::status(&main);
    assert!(!text.contains("quiet:"));
    assert!(text.contains("secrets assigned: API_KEY"));
}

#[tokio::test]
async fn secrets_list_names_computers_and_no_values() {
    let engine = engine_with_secrets();
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let listed = ops::secrets_list(&connected).await.unwrap();
    assert_eq!(listed["secrets"][0]["name"], "API_KEY");
    assert_eq!(listed["secrets"][0]["computers"], json!(["main"]));
    assert!(!listed.to_string().contains("value"));
}

#[tokio::test]
async fn secrets_assign_and_unassign_edit_the_computer_list() {
    let engine = engine_with_secrets();
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let assigned = ops::secret_assign(&connected, "API_KEY", "fork", true)
        .await
        .unwrap();
    assert_eq!(assigned["secrets"][0]["computers"], json!(["main", "fork"]));
    let saved = &engine.calls("secrets.save")[0]["request"];
    assert_eq!(saved["operation"], "edit");
    assert_eq!(saved["id"], "s1");
    assert_eq!(saved["computerIds"], json!(["id-main", "id-fork"]));
    assert!(saved.get("value").is_none());
    let removed = ops::secret_assign(&connected, "API_KEY", "main", false)
        .await
        .unwrap();
    assert_eq!(removed["secrets"][0]["computers"], json!(["fork"]));
    let last = ops::secret_assign(&connected, "API_KEY", "fork", false)
        .await
        .unwrap_err();
    assert!(last.message.contains("at least one"));
    assert!(ops::secret_assign(&connected, "NOPE", "fork", true)
        .await
        .is_err());
}

#[tokio::test]
async fn secrets_set_sends_the_value_and_requires_scope_for_a_new_secret() {
    let engine = engine_with_secrets();
    let connected = Engine::connect(&engine.socket).await.unwrap();
    ops::secret_set(&connected, "API_KEY", "v2".into(), &[], &[])
        .await
        .unwrap();
    let saved = &engine.calls("secrets.save")[0]["request"];
    assert_eq!(saved["operation"], "edit");
    assert_eq!(saved["value"], "v2");
    assert_eq!(saved["computerIds"], json!(["id-main"]));
    assert_eq!(saved["allowedDomains"], json!(["api.example.com"]));
    let error = ops::secret_set(&connected, "NEW_KEY", "v".into(), &[], &[])
        .await
        .unwrap_err();
    assert!(error.message.contains("--computer"));
    ops::secret_set(
        &connected,
        "NEW_KEY",
        "v".into(),
        &["fork".into()],
        &["x.example.com".into()],
    )
    .await
    .unwrap();
    let added = &engine.calls("secrets.save")[1]["request"];
    assert_eq!(added["operation"], "add");
    assert!(added.get("id").is_none());
    assert_eq!(added["computerIds"], json!(["id-fork"]));
    assert!(ops::secret_set(&connected, "K", String::new(), &[], &[])
        .await
        .is_err());
}

#[tokio::test]
async fn retry_setup_runs_the_engines_setup_retry_for_the_computer() {
    let engine = FakeEngine::start(|method, _| match method {
        "computers.list" => Ok(listing(&[("mac", "failed")])),
        "macos.setup.retry" => Ok(json!({"jobId": "job:3"})),
        "operations.job.get" => {
            Ok(json!({"jobId": "job:3", "state": "finished", "outcome": {"result": null}}))
        }
        "operations.job.acknowledge" => Ok(Value::Null),
        other => Err(("internal", format!("unexpected {other}"))),
    });
    let connected = Engine::connect(&engine.socket).await.unwrap();
    ops::retry_setup(&connected, "mac", &ignore).await.unwrap();
    assert_eq!(engine.calls("macos.setup.retry")[0]["computerId"], "id-mac");
    let cli = <Cli as clap::Parser>::try_parse_from(["silo", "retry-setup", "mac"]).unwrap();
    assert!(
        matches!(cli.command, silo_cli::cli::Command::RetrySetup { ref computer } if computer == "mac")
    );
}

fn keep_engine(result: Value) -> FakeEngine {
    FakeEngine::start(move |method, _| match method {
        "computers.list" => Ok(listing(&[("main", "running"), ("try", "running")])),
        "changes.keep" | "changes.undo" => Ok(json!({"jobId": "job:1"})),
        "operations.job.get" => Ok(finished(result.clone())),
        "operations.job.acknowledge" => Ok(Value::Null),
        other => Err(("internal", format!("unexpected {other}"))),
    })
}

fn fold_cli(engine: &FakeEngine, extra: &[&str]) -> Cli {
    let socket = engine.socket.to_str().unwrap();
    let mut args = vec!["silo", "--socket", socket, "--allow-other-channel", "fold"];
    args.extend_from_slice(extra);
    <Cli as clap::Parser>::try_parse_from(args).unwrap()
}

#[tokio::test]
async fn fold_sends_the_fork_the_target_and_the_recipe_keep_section() {
    let engine = keep_engine(json!({
        "status": "kept",
        "report": {"fork": "id-try", "target": "id-main", "summary": [], "steps": ["restart web: pid 1 -> 2"], "notes": [], "checkpoint": "cp1"}
    }));
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(project.path().join(".silo")).unwrap();
    std::fs::write(
        project.path().join(".silo/recipe.yaml"),
        "keep:\n  restart: [web]\n  migrate: [make migrate]\n",
    )
    .unwrap();
    let cli = fold_cli(&engine, &["try", "--into", "main", "--dry-run"]);
    assert_eq!(run(cli, project.path()).await.unwrap(), 0);
    let sent = &engine.calls("changes.keep")[0];
    assert_eq!(sent["forkId"], "id-try");
    assert_eq!(sent["targetId"], "id-main");
    assert_eq!(sent["dryRun"], true);
    assert_eq!(sent["keep"]["restart"], json!(["web"]));
    assert_eq!(sent["keep"]["migrate"], json!(["make migrate"]));
}

#[tokio::test]
async fn a_refused_fold_is_a_result_with_a_distinct_exit_code() {
    let engine = keep_engine(json!({
        "status": "refused",
        "report": {"fork": "id-try", "target": "id-main", "summary": [], "steps": [], "notes": [], "checkpoint": null},
        "refusal": {"reason": "database", "message": "Database changed and nothing migrates it.", "conflicts": [], "hint": "Use Make this main."}
    }));
    let cli = fold_cli(&engine, &["try"]);
    assert_eq!(run(cli, std::env::temp_dir().as_path()).await.unwrap(), 2);
    assert!(engine.calls("changes.keep")[0]["targetId"].is_null());
}

#[tokio::test]
async fn the_undo_command_names_the_computer_and_reports_what_was_undone() {
    let engine = keep_engine(json!({
        "status": "undone", "computer": "id-main", "recovered": false,
        "checkpoint": {"id": "cp1", "name": "Before keep of try", "createdAt": "t"}
    }));
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let value = ops::undo(&connected, "main", &ignore).await.unwrap();
    assert_eq!(engine.calls("changes.undo")[0]["computerId"], "id-main");
    assert!(silo_cli::render::undo(&value).contains("Before keep of try"));
}

#[tokio::test]
async fn diff_uses_a_cached_baseline_and_measures_nothing() {
    let engine = FakeEngine::start(|method, _| match method {
        "computers.list" => Ok(listing(&[("try", "running")])),
        "changes.diff" => Ok(json!({"jobId": "job:1"})),
        "operations.job.get" => Ok(finished(json!({
            "diff": {"format": 1, "other": []},
            "text": "No changes.",
            "baselineApplied": true, "baselineAvailable": true,
            "baseSource": "cp1", "elapsedSeconds": 30,
            "baseCapturedAt": "a", "capturedAt": "b"
        }))),
        "operations.job.acknowledge" => Ok(Value::Null),
        other => Err(("internal", format!("unexpected {other}"))),
    });
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let args = ops::DiffArgs {
        computer: "try".into(),
        max_lines: Some(30),
        no_baseline: false,
        compute_baseline: true,
    };
    let value = ops::diff(&connected, &args, &ignore).await.unwrap();
    assert_eq!(value["format"], 1);
    assert_eq!(value["text"], "No changes.");
    assert_eq!(value["baselineApplied"], true);
    assert_eq!(engine.calls("changes.baseline").len(), 0);
    assert_eq!(engine.calls("checkpoints.fork").len(), 0);
    let sent = &engine.calls("changes.diff")[0];
    assert_eq!(sent["maxLines"], 30);
    assert_eq!(sent["baseline"], true);

    let raw = ops::DiffArgs {
        no_baseline: true,
        compute_baseline: false,
        ..args
    };
    ops::diff(&connected, &raw, &ignore).await.unwrap();
    assert_eq!(engine.calls("changes.diff")[1]["baseline"], false);
}

/// An engine that knows one fork (`try`) of checkpoint `cp1` and tracks the throwaway.
struct Baseline {
    computers: Vec<(String, String)>,
    measured: bool,
    fail_baseline: bool,
}

fn baseline_engine(
    fail_baseline: bool,
    extra: &[(&str, &str)],
) -> (FakeEngine, Arc<Mutex<Baseline>>) {
    let mut computers = vec![("try".to_owned(), "running".to_owned())];
    computers.extend(
        extra
            .iter()
            .map(|(n, s)| ((*n).to_owned(), (*s).to_owned())),
    );
    let state = Arc::new(Mutex::new(Baseline {
        computers,
        measured: false,
        fail_baseline,
    }));
    let shared = state.clone();
    let engine = FakeEngine::start(move |method, params| {
        let mut state = shared.lock().unwrap();
        let named = |id: &str| id.trim_start_matches("id-").to_owned();
        match method {
            "computers.list" => {
                let pairs: Vec<(&str, &str)> = state
                    .computers
                    .iter()
                    .map(|(n, s)| (n.as_str(), s.as_str()))
                    .collect();
                Ok(listing(&pairs))
            }
            "checkpoints.fork" => {
                let name = params["newName"].as_str().unwrap().to_owned();
                state.computers.push((name, "stopped".into()));
                Ok(json!({"jobId": "job:3"}))
            }
            "computers.start" | "computers.stop" => {
                let name = named(params["computerId"].as_str().unwrap());
                let to = if method == "computers.start" {
                    "running"
                } else {
                    "stopped"
                };
                for (n, s) in state.computers.iter_mut() {
                    if *n == name {
                        *s = to.into();
                    }
                }
                Ok(json!({"jobId": "job:3"}))
            }
            "computers.delete" => {
                let name = named(params["computerId"].as_str().unwrap());
                state.computers.retain(|(n, _)| *n != name);
                Ok(json!({"jobId": "job:3"}))
            }
            "changes.baseline" => Ok(json!({"jobId": "job:2"})),
            "changes.diff" => Ok(json!({"jobId": "job:1"})),
            "operations.job.get" => match params["jobId"].as_str().unwrap() {
                "job:2" if state.fail_baseline => Ok(json!({
                    "jobId": "job:2", "state": "finished",
                    "outcome": {"error": {"code": "internal", "message": "The copy did not start."}}})),
                "job:2" => {
                    state.measured = true;
                    Ok(finished(
                        json!({"source": "cp1", "idleSeconds": 42, "capturedAt": "t"}),
                    ))
                }
                "job:1" => Ok(finished(json!({
                    "diff": {"format": 1, "other": []},
                    "text": "Code",
                    "baselineApplied": state.measured, "baselineAvailable": state.measured,
                    "baseSource": "cp1", "elapsedSeconds": 42,
                    "baseCapturedAt": "a", "capturedAt": "b"
                }))),
                _ => Ok(finished(json!({}))),
            },
            "operations.job.acknowledge" => Ok(Value::Null),
            other => Err(("internal", format!("unexpected {other}"))),
        }
    });
    (engine, state)
}

fn compute_baseline_args() -> ops::DiffArgs {
    ops::DiffArgs {
        computer: "try".into(),
        max_lines: None,
        no_baseline: false,
        compute_baseline: true,
    }
}

#[tokio::test]
async fn a_missing_baseline_is_measured_on_a_throwaway_fork_that_is_dropped() {
    let (engine, state) = baseline_engine(false, &[]);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let value = ops::diff(&connected, &compute_baseline_args(), &ignore)
        .await
        .unwrap();
    assert_eq!(value["baselineApplied"], true);
    assert_eq!(value["baselineIdleSeconds"], 42);
    // The copy came from the checkpoint the fork was made from, and the idle time is the
    // time the real fork had run.
    let fork = &engine.calls("checkpoints.fork")[0];
    assert_eq!(fork["checkpointId"], "cp1");
    let name = fork["newName"].as_str().unwrap().to_owned();
    assert!(name.starts_with(ops::BASELINE_PREFIX));
    let baseline = &engine.calls("changes.baseline")[0];
    assert_eq!(baseline["computerId"], format!("id-{name}"));
    assert_eq!(baseline["source"], "cp1");
    assert_eq!(baseline["idleSeconds"], 42);
    assert_eq!(
        engine.calls("computers.start")[0]["computerId"],
        format!("id-{name}")
    );
    // It is dropped, and only it.
    let deleted = engine.calls("computers.delete");
    assert_eq!(deleted.len(), 1);
    assert_eq!(deleted[0]["computerId"], format!("id-{name}"));
    assert_eq!(state.lock().unwrap().computers.len(), 1);
}

#[tokio::test]
async fn the_throwaway_is_dropped_when_the_measurement_fails() {
    let (engine, state) = baseline_engine(true, &[]);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let value = ops::diff(&connected, &compute_baseline_args(), &ignore)
        .await
        .unwrap();
    assert_eq!(value["baselineApplied"], false);
    assert_eq!(value["baselineError"], "The copy did not start.");
    assert_eq!(engine.calls("computers.delete").len(), 1);
    let left: Vec<String> = state
        .lock()
        .unwrap()
        .computers
        .iter()
        .map(|c| c.0.clone())
        .collect();
    assert_eq!(left, ["try"]);
}

#[tokio::test]
async fn the_throwaway_never_shows_in_the_list_and_stale_ones_are_dropped() {
    let (engine, state) = baseline_engine(false, &[("silo-baseline-old", "stopped")]);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let listed = ops::list(&connected).await.unwrap();
    let names: Vec<&str> = listed["computers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["try"]);
    ops::diff(&connected, &compute_baseline_args(), &ignore)
        .await
        .unwrap();
    let left: Vec<String> = state
        .lock()
        .unwrap()
        .computers
        .iter()
        .map(|c| c.0.clone())
        .collect();
    assert_eq!(left, ["try"], "the stale copy was dropped too");
}

#[tokio::test]
async fn a_macos_baseline_is_refused_when_two_virtual_machines_already_run() {
    let engine = FakeEngine::start(|method, _| match method {
        "computers.list" => Ok(json!({"linux": {"computers": []}, "macos": {"computers": [
            {"id": "m1", "name": "mac-a", "state": "running", "checkpoints": [{"id": "cp1"}]},
            {"id": "m2", "name": "mac-b", "state": "running", "checkpoints": []}
        ]}})),
        "changes.diff" => Ok(json!({"jobId": "job:1"})),
        "operations.job.get" => Ok(finished(json!({
            "diff": {"format": 1, "other": []}, "text": "Code",
            "baselineApplied": false, "baselineAvailable": false,
            "baseSource": "cp1", "elapsedSeconds": 10,
            "baseCapturedAt": "a", "capturedAt": "b"
        }))),
        "operations.job.acknowledge" => Ok(Value::Null),
        other => Err(("internal", format!("unexpected {other}"))),
    });
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let args = ops::DiffArgs {
        computer: "mac-b".into(),
        ..compute_baseline_args()
    };
    let value = ops::diff(&connected, &args, &ignore).await.unwrap();
    assert!(value["baselineError"]
        .as_str()
        .unwrap()
        .contains("two macOS virtual machines"));
    assert!(engine.calls("checkpoints.fork").is_empty());
}

#[tokio::test]
async fn fold_defaults_to_the_digest_the_last_diff_of_that_fork_showed() {
    let engine = FakeEngine::start(|method, _| match method {
        "computers.list" => Ok(listing(&[("main", "running"), ("try", "running")])),
        "changes.diff" | "changes.keep" => Ok(json!({"jobId": "job:1"})),
        "operations.job.get" => Ok(finished(json!({
            "diff": {"format": 1, "other": []}, "text": "Code\n  ~/app: a..b",
            "baselineApplied": false, "baselineAvailable": false,
            "baseCapturedAt": "a", "capturedAt": "b", "reviewDigest": "abc123",
            "status": "dry-run",
            "report": {"fork": "id-try", "target": "id-main", "summary": [], "steps": [], "notes": [], "checkpoint": null}
        }))),
        "operations.job.acknowledge" => Ok(Value::Null),
        other => Err(("internal", format!("unexpected {other}"))),
    });
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let args = ops::DiffArgs {
        computer: "try".into(),
        max_lines: None,
        no_baseline: false,
        compute_baseline: false,
    };
    let value = ops::diff(&connected, &args, &ignore).await.unwrap();
    assert_eq!(value["reviewDigest"], "abc123");
    ops::fold(
        &connected,
        "try",
        None,
        None,
        ops::FoldOptions::default(),
        &ignore,
    )
    .await
    .unwrap();
    assert_eq!(engine.calls("changes.keep")[0]["reviewDigest"], "abc123");
    // An explicit digest wins.
    let options = ops::FoldOptions {
        reviewed: Some("zzz".into()),
        ..Default::default()
    };
    ops::fold(&connected, "try", None, None, options, &ignore)
        .await
        .unwrap();
    assert_eq!(engine.calls("changes.keep")[1]["reviewDigest"], "zzz");
}
