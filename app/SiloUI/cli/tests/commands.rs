mod fake_engine;

use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use fake_engine::{finished, listing, listing_without_desktop, FakeEngine};
use serde_json::{json, Value};
use silo_cli::{
    cli::{run, Cli},
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
    let fork = ops::fork(&connected, &args, &ignore).await.unwrap();
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
async fn fork_sends_the_named_secrets_and_leaves_the_recipe_rules_to_the_engine() {
    let (engine, _) = engine_with(&["main"]);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let args = ForkArgs {
        source: "main".into(),
        secrets: vec!["github".into()],
        ..ForkArgs::default()
    };
    ops::fork(&connected, &args, &ignore).await.unwrap();
    let sent = &engine.calls("checkpoints.fork")[0];
    assert_eq!(sent["secrets"], json!(["github"]));
    // The quiet and scrub lists of the recipe the source was built from are the engine's to
    // apply: the client sends none.
    assert!(sent.get("quiet").is_none() || sent["quiet"].is_null());
}

/// An engine whose forks are made stopped and run once started.
fn stopped_fork_engine() -> FakeEngine {
    let forked = Arc::new(Mutex::new(Vec::<(String, bool)>::new()));
    FakeEngine::start(move |method, params| match method {
        "computers.list" => {
            let forks = forked.lock().unwrap().clone();
            let mut all = vec![("main", "running")];
            all.extend(forks.iter().map(|(name, running)| {
                (name.as_str(), if *running { "running" } else { "stopped" })
            }));
            Ok(listing(&all))
        }
        "checkpoints.fork" => {
            forked
                .lock()
                .unwrap()
                .push((params["newName"].as_str().unwrap().to_owned(), false));
            Ok(json!({"jobId": "job:1"}))
        }
        "computers.start" => {
            for fork in forked.lock().unwrap().iter_mut() {
                if format!("id-{}", fork.0) == params["computerId"].as_str().unwrap() {
                    fork.1 = true;
                }
            }
            Ok(json!({"jobId": "job:1"}))
        }
        "device.resources" => Ok(json!({
            "state": "low", "message": "Memory is running short: 2 GiB are free.",
            "computers": []
        })),
        "operations.job.get" => Ok(finished(json!({}))),
        "operations.job.acknowledge" => Ok(Value::Null),
        other => Err(("internal", format!("unexpected {other}"))),
    })
}

#[tokio::test]
async fn a_fork_made_to_work_in_is_started_and_the_device_is_warned_when_it_runs_short() {
    let engine = stopped_fork_engine();
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let args = ForkArgs {
        source: "main".into(),
        name: Some("try".into()),
        ..ForkArgs::default()
    };
    let value = ops::fork(&connected, &args, &ignore).await.unwrap();
    assert_eq!(engine.calls("computers.start")[0]["computerId"], "id-try");
    assert_eq!(value["state"], "running");
    assert_eq!(value["warning"], "Memory is running short: 2 GiB are free.");
    let text = silo_cli::render::forked(&value);
    assert!(text.contains("warning: Memory is running short"), "{text}");
}

#[tokio::test]
async fn no_start_leaves_the_fork_stopped() {
    let engine = stopped_fork_engine();
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let args = ForkArgs {
        source: "main".into(),
        name: Some("try".into()),
        no_start: true,
        ..ForkArgs::default()
    };
    ops::fork(&connected, &args, &ignore).await.unwrap();
    assert!(engine.calls("computers.start").is_empty());
}

#[tokio::test]
async fn the_command_line_needs_no_project_folder() {
    let (engine, _) = engine_with(&["main"]);
    let socket = engine.socket.to_str().unwrap();
    let project = tempfile::tempdir().unwrap();
    // A leftover `.silo` folder is ignored: nothing reads it any more.
    std::fs::create_dir_all(project.path().join(".silo")).unwrap();
    std::fs::write(project.path().join(".silo/recipe.yaml"), "bogus: 1\n").unwrap();
    std::fs::write(project.path().join(".silo/silo.yaml"), "bogus: 1\n").unwrap();
    let cli = <Cli as clap::Parser>::try_parse_from([
        "silo",
        "--socket",
        socket,
        "--allow-other-channel",
        "fork",
        "main",
    ])
    .unwrap();
    run(cli, project.path()).await.unwrap();
    assert_eq!(engine.calls("checkpoints.fork").len(), 1);
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

fn desktop_engine(off: bool, state: &'static str) -> FakeEngine {
    FakeEngine::start(move |method, _| match method {
        "computers.list" => {
            let off: &[&str] = if off { &["main"] } else { &[] };
            Ok(listing_without_desktop(&[("main", state)], off))
        }
        "computers.configuration.change" | "desktop.action" => Ok(json!({"jobId": "job:7"})),
        "operations.job.get" => Ok(finished(Value::Null)),
        "operations.job.acknowledge" => Ok(Value::Null),
        other => Err(("internal", format!("unexpected {other}"))),
    })
}

#[tokio::test]
async fn desktop_on_changes_the_setting_once_and_lets_the_guest_start_it() {
    let engine = desktop_engine(true, "running");
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let value = ops::desktop_set(&connected, "main", true, &ignore)
        .await
        .unwrap();
    assert_eq!(value["desktop"], true);
    let change = &engine.calls("computers.configuration.change")[0]["change"];
    assert_eq!(change["kind"], "upsert");
    assert_eq!(
        change["configuration"]["desktop"],
        json!({"startWithComputer": true, "builtIn": true})
    );
    assert_eq!(
        change["expected"]["desktop"],
        json!({"startWithComputer": false, "builtIn": true})
    );
    assert!(engine.calls("desktop.action").is_empty());
}

#[tokio::test]
async fn desktop_off_turns_the_setting_off_and_stops_the_running_desktop() {
    let engine = desktop_engine(false, "running");
    let connected = Engine::connect(&engine.socket).await.unwrap();
    ops::desktop_set(&connected, "main", false, &ignore)
        .await
        .unwrap();
    let change = &engine.calls("computers.configuration.change")[0]["change"];
    assert_eq!(
        change["configuration"]["desktop"]["startWithComputer"],
        false
    );
    assert_eq!(change["configuration"]["desktop"]["builtIn"], true);
    assert_eq!(engine.calls("desktop.action")[0]["action"], "stop");
}

#[tokio::test]
async fn desktop_on_for_a_desktop_that_is_on_only_starts_it_and_off_for_one_that_is_off_does_nothing(
) {
    let engine = desktop_engine(false, "running");
    let connected = Engine::connect(&engine.socket).await.unwrap();
    ops::desktop_set(&connected, "main", true, &ignore)
        .await
        .unwrap();
    assert!(engine.calls("computers.configuration.change").is_empty());
    assert_eq!(engine.calls("desktop.action")[0]["action"], "start");
    let engine = desktop_engine(true, "running");
    let connected = Engine::connect(&engine.socket).await.unwrap();
    ops::desktop_set(&connected, "main", false, &ignore)
        .await
        .unwrap();
    assert!(engine.calls("computers.configuration.change").is_empty());
    assert!(engine.calls("desktop.action").is_empty());
}

#[tokio::test]
async fn desktop_needs_a_running_linux_computer() {
    let engine = desktop_engine(true, "stopped");
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let error = ops::desktop_set(&connected, "main", true, &ignore)
        .await
        .unwrap_err();
    assert!(
        error.message.contains("silo start main"),
        "{}",
        error.message
    );
    assert!(engine.calls("computers.configuration.change").is_empty());

    let mac = FakeEngine::start(|method, _| match method {
        "computers.list" => Ok(json!({
            "linux": {"computers": []},
            "macos": {"computers": [{
                "id": "id-mac", "name": "mac", "state": "running", "detail": "",
                "cpus": 4, "memoryGiB": 8, "diskGiB": 64, "boots": 1, "checkpoints": []
            }]}
        })),
        other => Err(("internal", format!("unexpected {other}"))),
    });
    let connected = Engine::connect(&mac.socket).await.unwrap();
    let error = ops::desktop_set(&connected, "mac", false, &ignore)
        .await
        .unwrap_err();
    assert!(
        error.message.contains("macOS computer"),
        "{}",
        error.message
    );
}

/// An older computer: its desktop was installed by hand, or it has none, and `startWithComputer`
/// does not mean "no desktop" there.
fn legacy_desktop_engine(desktop: Value) -> FakeEngine {
    FakeEngine::start(move |method, _| match method {
        "computers.list" => {
            let mut listed = listing(&[("old", "running")]);
            let configuration = &mut listed["linux"]["computers"][0]["configuration"];
            if desktop.is_null() {
                configuration.as_object_mut().unwrap().remove("desktop");
            } else {
                configuration["desktop"] = desktop.clone();
            }
            Ok(listed)
        }
        other => Err(("internal", format!("unexpected {other}"))),
    })
}

#[tokio::test]
async fn a_computer_without_a_built_in_desktop_is_never_said_to_have_no_desktop() {
    for desktop in [json!({"startWithComputer": false}), Value::Null] {
        let engine = legacy_desktop_engine(desktop);
        let connected = Engine::connect(&engine.socket).await.unwrap();
        let computer = connected.computer("old").await.unwrap();
        assert_eq!(computer.desktop_on(), None);
        assert!(silo_cli::gui::ensure_desktop(&computer).is_ok());
        let value = ops::status(&connected, "old").await.unwrap();
        assert!(value["desktop"].is_null());
        assert!(!silo_cli::render::status(&value).contains("no desktop"));
        let error = ops::desktop_set(&connected, "old", true, &ignore)
            .await
            .unwrap_err();
        assert!(
            error.message.contains("does not have a built-in desktop"),
            "{}",
            error.message
        );
        assert!(engine.calls("computers.configuration.change").is_empty());
    }
}

#[test]
fn the_desktop_command_takes_on_or_off_and_a_computer() {
    let cli = <Cli as clap::Parser>::try_parse_from(["silo", "desktop", "off", "main"]).unwrap();
    assert!(matches!(
        cli.command,
        silo_cli::cli::Command::Desktop { state: silo_cli::cli::DesktopState::Off, ref computer }
            if computer == "main"
    ));
    assert!(<Cli as clap::Parser>::try_parse_from(["silo", "desktop", "main"]).is_err());
}

#[tokio::test]
async fn status_says_when_a_computer_has_no_desktop() {
    let engine = desktop_engine(true, "running");
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let value = ops::status(&connected, "main").await.unwrap();
    assert_eq!(value["desktop"], false);
    assert!(silo_cli::render::status(&value)
        .contains("main has no desktop; turn it on with `silo desktop on main`."));
}

#[test]
fn status_leaves_out_computer_use_for_a_computer_with_no_desktop() {
    let computer_use = json!({"state": "failed", "approval": "ask", "driverVersion": "0.38.0"});
    let value = |desktop: bool| {
        json!({"name": "main", "kind": "linux", "state": "running", "cpus": 2, "memoryGiB": 4,
               "checkpoints": 0, "desktop": desktop, "computerUse": computer_use})
    };
    assert!(silo_cli::render::status(&value(true)).contains("computer use: failed"));
    assert!(!silo_cli::render::status(&value(false)).contains("computer use"));
}

fn keep_engine(result: Value) -> FakeEngine {
    FakeEngine::start(move |method, _| match method {
        "computers.list" => Ok(listing(&[("main", "running"), ("try", "running")])),
        "changes.keep" | "changes.undo" | "changes.promote" | "computers.rebuild" => Ok(json!({"jobId": "job:1"})),
        "operations.job.get" => Ok(finished(result.clone())),
        "operations.job.acknowledge" => Ok(Value::Null),
        other => Err(("internal", format!("unexpected {other}"))),
    })
}

fn merge_cli(engine: &FakeEngine, extra: &[&str]) -> Cli {
    let socket = engine.socket.to_str().unwrap();
    let mut args = vec!["silo", "--socket", socket, "--allow-other-channel", "merge"];
    args.extend_from_slice(extra);
    <Cli as clap::Parser>::try_parse_from(args).unwrap()
}

#[tokio::test]
async fn merge_sends_the_fork_the_target_and_no_hooks_the_engine_reads_them_from_the_recipe() {
    let engine = keep_engine(json!({
        "status": "kept",
        "report": {"fork": "id-try", "target": "id-main", "summary": [], "steps": ["restart web: pid 1 -> 2"], "notes": [], "checkpoint": "cp1"}
    }));
    let project = tempfile::tempdir().unwrap();
    let cli = merge_cli(&engine, &["try", "--into", "main", "--dry-run"]);
    assert_eq!(run(cli, project.path()).await.unwrap(), 0);
    let sent = &engine.calls("changes.keep")[0];
    assert_eq!(sent["forkId"], "id-try");
    assert_eq!(sent["targetId"], "id-main");
    assert_eq!(sent["dryRun"], true);
    assert!(sent.get("keep").is_none() || sent["keep"].is_null());
}

#[tokio::test]
async fn a_refused_merge_is_a_result_with_a_distinct_exit_code() {
    let engine = keep_engine(json!({
        "status": "refused",
        "report": {"fork": "id-try", "target": "id-main", "summary": [], "steps": [], "notes": [], "checkpoint": null},
        "refusal": {"reason": "database", "message": "Database changed and nothing migrates it.", "conflicts": [], "hint": "Replace main with the fork."}
    }));
    let cli = merge_cli(&engine, &["try"]);
    assert_eq!(run(cli, std::env::temp_dir().as_path()).await.unwrap(), 2);
    assert!(engine.calls("changes.keep")[0]["targetId"].is_null());
}

fn choices_needed() -> Value {
    json!({
        "status": "refused",
        "report": {"fork": "try", "target": "main", "summary": [], "steps": [], "notes": [], "checkpoint": null},
        "refusal": {
            "reason": "choices-needed",
            "message": "2 items need an answer before try can be merged into main.",
            "hint": "Merge again with choices.",
            "conflicts": [
                {"kind": "file", "item": "/etc/app.conf", "detail": "both changed it",
                 "id": "file:/etc/app.conf", "allowed": ["fork", "main"],
                 "forkChange": "modified", "mainChange": "modified"},
                {"kind": "git", "item": "/work/app", "detail": "diverged",
                 "id": "repo:/work/app", "allowed": ["branch", "main"],
                 "forkChange": "2 commits", "mainChange": "1 commit"}
            ]
        }
    })
}

#[tokio::test]
async fn a_merge_that_needs_answers_lists_the_conflicts_and_exits_with_its_own_code() {
    let engine = keep_engine(choices_needed());
    let cli = merge_cli(&engine, &["try"]);
    assert_eq!(run(cli, std::env::temp_dir().as_path()).await.unwrap(), 3);
    let text = silo_cli::render::merge(&choices_needed());
    assert!(text.contains("file:/etc/app.conf"), "{text}");
    assert!(text.contains("answers: fork, main"), "{text}");
    assert!(
        text.contains("repo:/work/app") && text.contains("answers: branch, main"),
        "{text}"
    );
    assert!(text.contains("--choose <id>=<answer>"), "{text}");
    assert!(!text.contains("fold") && !text.contains("keep"), "{text}");
}

#[tokio::test]
async fn the_answers_of_choose_and_skip_reach_the_engine() {
    let engine = keep_engine(json!({
        "status": "kept",
        "report": {"fork": "id-try", "target": "id-main", "summary": [], "steps": [], "notes": [], "checkpoint": "cp1"}
    }));
    let cli = merge_cli(
        &engine,
        &[
            "try",
            "--choose",
            "file:/etc/app.conf=fork",
            "--choose",
            "repo:/work/app=branch",
            "--skip",
            "package:dpkg:jq",
        ],
    );
    assert_eq!(run(cli, std::env::temp_dir().as_path()).await.unwrap(), 0);
    let sent = &engine.calls("changes.keep")[0];
    assert_eq!(
        sent["choices"],
        json!({"file:/etc/app.conf": "fork", "repo:/work/app": "branch"})
    );
    assert_eq!(sent["skip"], json!(["package:dpkg:jq"]));
    // A wrong answer is refused before the engine is asked.
    let engine = keep_engine(json!({}));
    let cli = merge_cli(&engine, &["try", "--choose", "file:/x=theirs"]);
    assert!(run(cli, std::env::temp_dir().as_path()).await.is_err());
    assert!(engine.calls("changes.keep").is_empty());
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
async fn an_undone_promote_is_reported_as_a_promote_not_a_keep() {
    let engine = keep_engine(json!({
        "status": "undone", "kind": "promote", "computer": "main", "fork": "try",
        "archive": "main-before-20261105-1432", "recovered": false, "changedSince": []
    }));
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let value = ops::undo(&connected, "main", &ignore).await.unwrap();
    let text = silo_cli::render::undo(&value);
    assert_eq!(
        text,
        "Undid the replace: main is main again, and try has its own name back."
    );
    assert!(!text.contains("Put "));
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
    /// The job-starting methods, in order.
    started: Vec<String>,
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
        started: Vec::new(),
    }));
    let shared = state.clone();
    let engine = FakeEngine::start(move |method, params| {
        let mut state = shared.lock().unwrap();
        let named = |id: &str| id.trim_start_matches("id-").to_owned();
        if params.get("jobId").is_none() && method != "computers.list" {
            state.started.push(method.to_owned());
        }
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
async fn merge_defaults_to_the_digest_the_last_diff_of_that_fork_showed() {
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
    ops::merge(
        &connected,
        "try",
        None,
        ops::MergeOptions::default(),
        &ignore,
    )
    .await
    .unwrap();
    assert_eq!(engine.calls("changes.keep")[0]["reviewDigest"], "abc123");
    // An explicit digest wins.
    let options = ops::MergeOptions {
        reviewed: Some("zzz".into()),
        ..Default::default()
    };
    ops::merge(&connected, "try", None, options, &ignore)
        .await
        .unwrap();
    assert_eq!(engine.calls("changes.keep")[1]["reviewDigest"], "zzz");
}

#[tokio::test]
async fn a_dry_run_merge_never_sends_a_review_digest() {
    let engine = FakeEngine::start(|method, _| match method {
        "computers.list" => Ok(listing(&[("main", "running"), ("try", "running")])),
        "changes.diff" | "changes.keep" => Ok(json!({"jobId": "job:1"})),
        "operations.job.get" => Ok(finished(json!({
            "diff": {"format": 1, "other": []}, "text": "", "baselineApplied": false,
            "baselineAvailable": false, "baseCapturedAt": "a", "capturedAt": "b",
            "reviewDigest": "stale", "status": "dry-run",
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
    ops::diff(&connected, &args, &ignore).await.unwrap();
    let options = ops::MergeOptions {
        dry_run: true,
        reviewed: Some("older".into()),
        ..Default::default()
    };
    ops::merge(&connected, "try", None, options, &ignore)
        .await
        .unwrap();
    assert!(engine.calls("changes.keep")[0]["reviewDigest"].is_null());
}

#[tokio::test]
async fn a_throwaway_is_stopped_and_seen_stopped_before_it_is_dropped() {
    // The computer reports "stopping" for two listings after its stop job, and the engine
    // refuses a delete until it reads stopped.
    let polls = Arc::new(Mutex::new(0u32));
    let order = Arc::new(Mutex::new(Vec::<String>::new()));
    let (p, o) = (polls.clone(), order.clone());
    let engine = FakeEngine::start(move |method, _| match method {
        "computers.list" => {
            let seen = order_state(&o);
            let state = if seen.iter().any(|m| m == "stop") {
                let mut n = p.lock().unwrap();
                *n += 1;
                if *n > 2 {
                    "stopped"
                } else {
                    "stopping"
                }
            } else {
                "running"
            };
            Ok(listing(&[("silo-baseline-aaaaaa", state)]))
        }
        "computers.stop" => {
            o.lock().unwrap().push("stop".into());
            Ok(json!({"jobId": "job:1"}))
        }
        "computers.delete" => {
            if *p.lock().unwrap() <= 2 {
                return Err(("invalid", "Stop the computer first.".into()));
            }
            o.lock().unwrap().push("delete".into());
            Ok(json!({"jobId": "job:1"}))
        }
        "operations.job.get" => Ok(finished(json!({}))),
        "operations.job.acknowledge" => Ok(Value::Null),
        other => Err(("internal", format!("unexpected {other}"))),
    });
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let all = connected.all_computers().await.unwrap();
    let computer = all
        .iter()
        .find(|c| c.name == "silo-baseline-aaaaaa")
        .unwrap()
        .clone();
    ops::remove_computer(&connected, &computer, &ignore)
        .await
        .unwrap();
    assert_eq!(order_state(&order), ["stop", "delete"]);
}

fn order_state(order: &Arc<Mutex<Vec<String>>>) -> Vec<String> {
    order.lock().unwrap().clone()
}

#[tokio::test]
async fn replace_main_sends_changes_promote_and_prints_the_archive_name() {
    let engine = keep_engine(json!({
        "status": "promoted",
        "report": {"fork": "try", "target": "main", "archive": "main-before-20261105-1432", "steps": [], "notes": [], "checkpoint": "cp1"}
    }));
    let socket = engine.socket.to_str().unwrap();
    let cli = <Cli as clap::Parser>::try_parse_from([
        "silo",
        "--socket",
        socket,
        "--allow-other-channel",
        "replace-main",
        "try",
        "--dry-run",
    ])
    .unwrap();
    assert_eq!(run(cli, std::env::temp_dir().as_path()).await.unwrap(), 0);
    let sent = &engine.calls("changes.promote")[0];
    assert_eq!(sent["forkId"], "id-try");
    assert_eq!(sent["dryRun"], true);
    assert!(engine.calls("changes.keep").is_empty());
    let text = silo_cli::render::replace_main(&json!({
        "status": "promoted",
        "report": {"fork": "try", "target": "main", "archive": "main-before-x", "notes": []}
    }));
    assert!(
        text.contains("try replaced main") && text.contains("main-before-x"),
        "{text}"
    );
    assert!(!text.contains("promot"), "{text}");
}

#[tokio::test]
async fn rebuild_sends_computers_rebuild_and_prints_result() {
    let engine = keep_engine(json!({
        "computerId": "id-main",
        "name": "main",
        "undoName": "main-undo",
        "image": "ubuntu-24.04-v5",
    }));
    let socket = engine.socket.to_str().unwrap();
    let cli = <Cli as clap::Parser>::try_parse_from([
        "silo",
        "--socket",
        socket,
        "--allow-other-channel",
        "rebuild",
        "main",
    ])
    .unwrap();
    assert_eq!(run(cli, std::env::temp_dir().as_path()).await.unwrap(), 0);
    let sent = &engine.calls("computers.rebuild")[0];
    assert_eq!(sent["computerId"], "id-main");
    assert!(sent.get("force").is_none(), "{sent}");
    let text = silo_cli::render::rebuild(&json!({
        "name": "main",
        "undoName": "main-undo",
        "image": "ubuntu-24.04-v5",
    }));
    assert!(text.contains("Rebuilt main on ubuntu-24.04-v5"), "{text}");
    assert!(text.contains("main-undo (stopped)"), "{text}");
}

#[tokio::test]
async fn rebuild_force_is_sent_when_asked_for() {
    let engine = keep_engine(json!({
        "computerId": "id-main",
        "name": "main",
        "undoName": "main-undo",
        "image": "ubuntu-24.04-v5",
    }));
    let socket = engine.socket.to_str().unwrap();
    let cli = <Cli as clap::Parser>::try_parse_from([
        "silo",
        "--socket",
        socket,
        "--allow-other-channel",
        "rebuild",
        "main",
        "--force",
    ])
    .unwrap();
    assert_eq!(run(cli, std::env::temp_dir().as_path()).await.unwrap(), 0);
    assert_eq!(engine.calls("computers.rebuild")[0]["force"], true);
}

#[tokio::test]
async fn rebuild_abandon_is_sent_when_asked_for_and_reads_plainly() {
    let engine = keep_engine(json!({
        "computerId": "id-main",
        "name": "main",
        "undoName": "",
        "image": "",
    }));
    let socket = engine.socket.to_str().unwrap();
    let cli = <Cli as clap::Parser>::try_parse_from([
        "silo",
        "--socket",
        socket,
        "--allow-other-channel",
        "rebuild",
        "main",
        "--abandon",
    ])
    .unwrap();
    assert_eq!(run(cli, std::env::temp_dir().as_path()).await.unwrap(), 0);
    let sent = &engine.calls("computers.rebuild")[0];
    assert_eq!(sent["abandon"], true);
    assert!(sent.get("force").is_none(), "{sent}");
    let text = silo_cli::render::rebuild(&json!({"name": "main", "undoName": "", "image": ""}));
    assert!(text.contains("Abandoned the rebuild of main"), "{text}");
    let both = ["silo", "rebuild", "main", "--abandon", "--force"];
    assert!(<Cli as clap::Parser>::try_parse_from(both).is_err());
}

#[tokio::test]
async fn fold_promote_still_replaces_main_through_the_old_name() {
    let engine = keep_engine(json!({
        "status": "promoted",
        "report": {"fork": "try", "target": "main", "archive": "a", "steps": [], "notes": [], "checkpoint": "cp1"}
    }));
    let socket = engine.socket.to_str().unwrap();
    let cli = <Cli as clap::Parser>::try_parse_from([
        "silo",
        "--socket",
        socket,
        "--allow-other-channel",
        "fold",
        "try",
        "--promote",
        "--dry-run",
    ])
    .unwrap();
    assert_eq!(run(cli, std::env::temp_dir().as_path()).await.unwrap(), 0);
    assert_eq!(engine.calls("changes.promote").len(), 1);
    assert!(engine.calls("changes.keep").is_empty());
}

fn names_in(state: &Arc<Mutex<Baseline>>) -> Vec<String> {
    state
        .lock()
        .unwrap()
        .computers
        .iter()
        .map(|c| c.0.clone())
        .collect()
}

#[tokio::test]
async fn list_all_shows_the_hidden_throwaways_and_drop_takes_them_by_name_or_id() {
    let (engine, state) = baseline_engine(
        false,
        &[
            ("silo-baseline-8fa22f", "stopped"),
            ("silo-build-linux-dev", "stopped"),
        ],
    );
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let shown = ops::list(&connected).await.unwrap();
    assert_eq!(shown["computers"].as_array().unwrap().len(), 1);
    let all = ops::list_all(&connected).await.unwrap();
    assert_eq!(all["computers"].as_array().unwrap().len(), 3);

    let dropped = ops::drop_computer(&connected, "silo-baseline-8fa22f", false, &ignore)
        .await
        .unwrap();
    assert_eq!(dropped["dropped"], "silo-baseline-8fa22f");
    ops::drop_computer(&connected, "id-silo-build-linux-dev", false, &ignore)
        .await
        .unwrap();
    assert_eq!(names_in(&state), ["try"]);
}

#[tokio::test]
async fn only_stopped_throwaways_older_than_an_hour_are_swept() {
    let now = silo_cli::clock::now_unix();
    let old = ops::baseline_name(now - 7200);
    let fresh = ops::baseline_name(now - 60);
    let (engine, state) = baseline_engine(
        false,
        &[
            ("silo-baseline-8fa22f", "stopped"),
            (old.as_str(), "stopped"),
            (fresh.as_str(), "stopped"),
            ("silo-baseline-bbbbbb", "running"),
        ],
    );
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let swept = ops::sweep_throwaways(&connected, &ignore).await;
    assert_eq!(swept.len(), 2, "{swept:?}");
    assert_eq!(
        names_in(&state),
        ["try", fresh.as_str(), "silo-baseline-bbbbbb"]
    );
    // Its jobs are never the one Ctrl-C names.
    assert_eq!(connected.last_job(), None);
}

#[tokio::test]
async fn a_fork_sweeps_after_it_returns_silently_and_keeps_its_own_job() {
    let (engine, state) = baseline_engine(false, &[("silo-baseline-8fa22f", "stopped")]);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let lines = Mutex::new(Vec::new());
    let progress = |line: &str| lines.lock().unwrap().push(line.to_owned());
    let args = ForkArgs {
        source: "try".into(),
        checkpoint: Some("cp1".into()),
        ..ForkArgs::default()
    };
    ops::fork(&connected, &args, &progress).await.unwrap();
    // The fork's job, and everything it reads, comes before the sweep's drop.
    let started = state.lock().unwrap().started.clone();
    assert_eq!(
        started.first().map(String::as_str),
        Some("checkpoints.fork")
    );
    assert_eq!(started.last().map(String::as_str), Some("computers.delete"));
    assert!(!names_in(&state).contains(&"silo-baseline-8fa22f".to_owned()));
    // The job Ctrl-C names is the fork's (the sweep alone records none: see above).
    assert_eq!(connected.last_job().as_deref(), Some("job:3"));
    let lines = lines.into_inner().unwrap();
    assert!(
        lines.iter().all(|line| !line.contains("baseline")),
        "{lines:?}"
    );
}
