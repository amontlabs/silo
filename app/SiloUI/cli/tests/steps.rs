mod fake_engine;

use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use fake_engine::{finished, listing, FakeEngine};
use serde_json::{json, Value};
use silo_cli::{
    cli::{run, Cli},
    engine::Engine,
    ops::ExecArgs,
    steps::{self, StepSettings},
};

fn ignore(_: &str) {}

fn exec_args() -> ExecArgs {
    ExecArgs {
        computer: "main".into(),
        argv: vec!["make".into(), "test".into()],
        cwd: None,
        timeout_secs: 5,
    }
}

/// An engine with `main`, whose checkpoints are `checkpoints`, whose steps are `steps`, and
/// that numbers the steps it is told about.
fn engine(
    checkpoints: Value,
    listed_steps: Value,
    record_fails: bool,
) -> (FakeEngine, Arc<Mutex<u32>>) {
    let recorded = Arc::new(Mutex::new(0u32));
    let count = recorded.clone();
    let engine = FakeEngine::start(move |method, params| match method {
        "computers.list" => {
            let mut listed = listing(&[("main", "running")]);
            listed["linux"]["computers"][0]["checkpoints"] = checkpoints.clone();
            Ok(listed)
        }
        "guest.exec" => Ok(json!({"exitCode": 3, "signal": null, "stdout": [], "stderr": []})),
        "checkpoints.create" => Ok(json!({"jobId": "job:1"})),
        "operations.job.get" => Ok(finished(json!({
            "checkpoint": {"id": "cp-step-new", "name": "step", "kind": "step"}
        }))),
        "operations.job.acknowledge" => Ok(Value::Null),
        "steps.record" if record_fails => Err(("internal", "The engine is busy.".into())),
        "steps.record" => {
            let mut number = count.lock().unwrap();
            *number += 1;
            Ok(json!({"step": {
                "id": "st-1", "computerId": "id-main", "number": *number,
                "command": "make test", "exitCode": params["exitCode"],
                "startedAt": params["startedAt"], "finishedAt": params["finishedAt"],
                "checkpointId": params["checkpointId"],
            }}))
        }
        "steps.list" => Ok(json!({"steps": listed_steps.clone()})),
        "checkpoints.fork" => Ok(json!({"jobId": "job:1"})),
        other => Err(("internal", format!("unexpected {other}"))),
    });
    (engine, recorded)
}

#[test]
fn step_settings_default_off_with_twenty_kept() {
    assert_eq!(
        StepSettings::default(),
        StepSettings {
            checkpoints: false,
            keep: 20
        }
    );
    assert_eq!(
        StepSettings::new(true, None),
        StepSettings {
            checkpoints: true,
            keep: 20
        }
    );
    assert_eq!(
        StepSettings::new(true, Some(5)),
        StepSettings {
            checkpoints: true,
            keep: 5
        }
    );
}

#[test]
fn the_step_options_of_a_command_choose_the_settings() {
    let parse = |args: &[&str]| {
        let parsed = <Cli as clap::Parser>::try_parse_from(
            std::iter::once("silo").chain(args.iter().copied()),
        )
        .unwrap();
        match parsed.command {
            silo_cli::cli::Command::Exec(exec) => exec.steps.settings(),
            _ => panic!("not an exec"),
        }
    };
    assert_eq!(
        parse(&["exec", "main", "--", "ls"]),
        StepSettings::default()
    );
    assert_eq!(
        parse(&[
            "exec",
            "main",
            "--step-checkpoints",
            "--keep-steps",
            "3",
            "--",
            "ls"
        ]),
        StepSettings::new(true, Some(3))
    );
    assert!(<Cli as clap::Parser>::try_parse_from([
        "silo",
        "exec",
        "main",
        "--keep-steps",
        "3",
        "--",
        "ls"
    ])
    .is_err());
}

#[tokio::test]
async fn a_step_without_step_checkpoints_is_recorded_and_takes_no_checkpoint() {
    let (engine, _) = engine(json!([]), json!([]), false);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let (output, step) =
        steps::run_exec(&connected, &exec_args(), StepSettings::default(), &ignore)
            .await
            .unwrap();
    assert_eq!(output.process_exit_code(), 3);
    assert_eq!(step["number"], 1);
    assert!(engine.calls("checkpoints.create").is_empty());
    let sent = &engine.calls("steps.record")[0];
    assert_eq!(sent["command"], json!(["make", "test"]));
    assert_eq!(sent["exitCode"], 3);
    assert_eq!(sent["computerId"], "id-main");
    assert!(sent.get("checkpointId").is_none());
    assert_eq!(steps::note(&step), "step 1");
}

#[tokio::test]
async fn with_step_checkpoints_a_step_checkpoint_is_taken_and_linked() {
    let (engine, _) = engine(json!([]), json!([]), false);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let on = StepSettings::new(true, Some(7));
    let (_, step) = steps::run_exec(&connected, &exec_args(), on, &ignore)
        .await
        .unwrap();
    let made = &engine.calls("checkpoints.create")[0];
    assert_eq!(made["kind"], "step");
    assert_eq!(made["keepNewest"], 7);
    assert_eq!(made["computerId"], "id-main");
    assert!(made["name"]
        .as_str()
        .unwrap()
        .starts_with("step: make test"));
    assert_eq!(
        engine.calls("steps.record")[0]["checkpointId"],
        "cp-step-new"
    );
    assert_eq!(steps::note(&step), "step 1 (checkpoint cp-step-new)");
}

#[tokio::test]
async fn a_step_that_cannot_be_recorded_never_fails_the_command() {
    let (engine, _) = engine(json!([]), json!([]), true);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let (output, step) =
        steps::run_exec(&connected, &exec_args(), StepSettings::default(), &ignore)
            .await
            .unwrap();
    assert_eq!(output.process_exit_code(), 3);
    assert_eq!(step["error"], "The engine is busy.");
    assert_eq!(steps::note(&step), "step not recorded: The engine is busy.");
}

#[tokio::test]
async fn the_command_line_prints_the_step_in_json() {
    let (engine, _) = engine(json!([]), json!([]), false);
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
    assert_eq!(engine.calls("steps.record").len(), 1);
}

#[tokio::test]
async fn forking_from_a_step_uses_the_checkpoint_that_step_left() {
    let checkpoints = json!([
        {"id": "cp-keep", "name": "s", "kind": "step", "createdAt": "now"},
    ]);
    let steps_listed = json!([
        {"number": 1, "command": "a", "checkpointId": "cp-gone"},
        {"number": 2, "command": "b", "checkpointId": "cp-keep"},
        {"number": 3, "command": "c"},
    ]);
    let (engine, _) = engine(checkpoints, steps_listed, false);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    assert_eq!(
        steps::checkpoint_for(&connected, "main", 2).await.unwrap(),
        "cp-keep"
    );
    let reclaimed = steps::checkpoint_for(&connected, "main", 1)
        .await
        .unwrap_err()
        .message;
    assert!(
        reclaimed.contains("step 1") && reclaimed.contains("was removed"),
        "{reclaimed}"
    );
    let off = steps::checkpoint_for(&connected, "main", 3)
        .await
        .unwrap_err()
        .message;
    assert!(off.contains("step_checkpoints was off"), "{off}");
    let missing = steps::checkpoint_for(&connected, "main", 9)
        .await
        .unwrap_err()
        .message;
    assert!(missing.contains("has no step 9"), "{missing}");

    let socket = engine.socket.to_str().unwrap();
    let cli = <Cli as clap::Parser>::try_parse_from([
        "silo",
        "--socket",
        socket,
        "--allow-other-channel",
        "fork",
        "main",
        "--from-step",
        "2",
    ])
    .unwrap();
    // The fork itself needs engine calls the fake does not answer; what matters is that the
    // checkpoint it asked for is the step's.
    let _ = run(cli, Path::new("/")).await;
    assert_eq!(
        engine.calls("checkpoints.fork")[0]["checkpointId"],
        "cp-keep"
    );
}

#[tokio::test]
async fn forking_from_a_step_refuses_a_step_with_no_checkpoint_before_forking() {
    let (engine, _) = engine(json!([]), json!([{"number": 1, "command": "a"}]), false);
    let socket = engine.socket.to_str().unwrap();
    let cli = <Cli as clap::Parser>::try_parse_from([
        "silo",
        "--socket",
        socket,
        "--allow-other-channel",
        "fork",
        "main",
        "--from-step",
        "1",
    ])
    .unwrap();
    let error = run(cli, Path::new("/")).await.unwrap_err();
    assert!(
        error.message.contains("has no checkpoint"),
        "{}",
        error.message
    );
    assert!(engine.calls("checkpoints.fork").is_empty());
    assert!(<Cli as clap::Parser>::try_parse_from([
        "silo",
        "fork",
        "main",
        "--from-step",
        "1",
        "--checkpoint",
        "x"
    ])
    .is_err());
}

#[tokio::test]
async fn the_tree_names_a_folder_fork_by_its_folder() {
    let engine = FakeEngine::start(|method, _| match method {
        "computers.tree" => Ok(json!({"nodes": [
            {"id": "id-main", "name": "main", "parent": null, "from": {"kind": "root"},
             "state": "running", "pendingChanges": {"requestId": null, "buckets": null}},
            {"id": "id-repo", "name": "repo-ab12", "parent": "id-main", "from": {"kind": "main"},
             "state": "running", "pendingChanges": {"requestId": "fr-1", "buckets": null}},
        ]})),
        other => Err(("internal", format!("unexpected {other}"))),
    });
    let connected = Engine::connect(&engine.socket).await.unwrap();
    connected
        .remember(
            "cli-folders.json",
            "id-repo",
            json!({"forkId": "id-repo", "fork": "repo-ab12", "path": "/work/repo"}),
        )
        .unwrap();
    let tree = steps::tree(&connected).await.unwrap();
    assert_eq!(
        tree["nodes"][1]["from"],
        json!({"kind": "folder", "path": "/work/repo"})
    );
    assert_eq!(tree["nodes"][0]["from"]["kind"], "root");
}

#[tokio::test]
async fn a_computers_steps_run_one_at_a_time_in_order() {
    let order: Arc<Mutex<Vec<String>>> = Arc::default();
    let seen = order.clone();
    let engine = FakeEngine::start(move |method, _| {
        seen.lock().unwrap().push(method.to_owned());
        match method {
            "computers.list" => Ok(listing(&[("main", "running")])),
            "guest.exec" => Ok(json!({"exitCode": 0, "signal": null, "stdout": [], "stderr": []})),
            "checkpoints.create" => Ok(json!({"jobId": "job:1"})),
            "operations.job.get" => Ok(finished(
                json!({"checkpoint": {"id": "cp", "name": "s", "kind": "step"}}),
            )),
            "operations.job.acknowledge" => Ok(Value::Null),
            "steps.record" => Ok(json!({"step": {"number": 1}})),
            other => Err(("internal", format!("unexpected {other}"))),
        }
    });
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let on = StepSettings::new(true, None);
    let args = exec_args();
    let (a, b) = tokio::join!(
        steps::run_exec(&connected, &args, on, &ignore),
        steps::run_exec(&connected, &args, on, &ignore)
    );
    a.unwrap();
    b.unwrap();
    let work: Vec<String> = order
        .lock()
        .unwrap()
        .iter()
        .filter(|method| {
            ["guest.exec", "checkpoints.create", "steps.record"].contains(&method.as_str())
        })
        .cloned()
        .collect();
    assert_eq!(work.len(), 6);
    // Another command may run meanwhile, but one step's checkpoint and record are never
    // separated by the other's.
    for (index, method) in work.iter().enumerate() {
        if method == "checkpoints.create" {
            assert_eq!(work[index + 1], "steps.record", "{work:?}");
        }
    }
}

#[tokio::test]
async fn without_step_checkpoints_nothing_waits_for_the_step_lock() {
    let (engine, _) = engine(json!([]), json!([]), false);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let _held = connected.lock_steps("id-main").await.unwrap();
    let ran = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        steps::run_exec(&connected, &exec_args(), StepSettings::default(), &ignore),
    )
    .await;
    assert!(ran.expect("the command must not wait for the lock").is_ok());
}

#[tokio::test]
async fn a_step_lock_that_cannot_be_taken_does_not_fail_the_command() {
    let (engine, _) = engine(json!([]), json!([]), false);
    // A folder where the lock file would go makes the lock impossible.
    std::fs::create_dir(
        engine
            .socket
            .parent()
            .unwrap()
            .join("cli-steps-id-main.lock"),
    )
    .unwrap();
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let (output, step) = steps::run_exec(
        &connected,
        &exec_args(),
        StepSettings::new(true, None),
        &ignore,
    )
    .await
    .unwrap();
    assert_eq!(output.process_exit_code(), 3);
    assert_eq!(step["number"], 1);
}

#[tokio::test]
async fn run_exec_keeps_the_timeout_limit_without_overflowing() {
    let (engine, _) = engine(json!([]), json!([]), false);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    for timeout_secs in [0, 181, u64::MAX] {
        let mut args = exec_args();
        args.timeout_secs = timeout_secs;
        let error = steps::run_exec(&connected, &args, StepSettings::default(), &ignore)
            .await
            .unwrap_err();
        assert!(error.message.contains("timeout"), "{}", error.message);
    }
    assert!(engine.calls("guest.exec").is_empty());
}

#[tokio::test]
async fn the_step_lock_waits_for_the_holder() {
    let (engine, _) = engine(json!([]), json!([]), false);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let held = connected.lock_steps("id-main").await.unwrap();
    let waiting = tokio::time::timeout(
        std::time::Duration::from_millis(200),
        connected.lock_steps("id-main"),
    )
    .await;
    assert!(waiting.is_err(), "the second lock must wait");
    assert!(connected.lock_steps("id-other").await.is_ok());
    drop(held);
    assert!(connected.lock_steps("id-main").await.is_ok());
    assert!(connected.lock_steps("../x").await.is_err());
}

#[tokio::test]
async fn a_step_belongs_to_the_computer_the_command_ran_on_even_if_the_name_moves() {
    let listings = Arc::new(Mutex::new(0u32));
    let calls = listings.clone();
    let engine = FakeEngine::start(move |method, _| match method {
        "computers.list" => {
            let mut count = calls.lock().unwrap();
            *count += 1;
            // After the first listing a promote hands the name to another computer.
            if *count == 1 {
                Ok(listing(&[("main", "running")]))
            } else {
                Ok(listing(&[("main-before", "running"), ("main", "running")]))
            }
        }
        "guest.exec" => Ok(json!({"exitCode": 0, "signal": null, "stdout": [], "stderr": []})),
        "steps.record" => Ok(json!({"step": {"number": 1}})),
        other => Err(("internal", format!("unexpected {other}"))),
    });
    let connected = Engine::connect(&engine.socket).await.unwrap();
    steps::run_exec(&connected, &exec_args(), StepSettings::default(), &ignore)
        .await
        .unwrap();
    assert_eq!(engine.calls("guest.exec")[0]["computerId"], "id-main");
    assert_eq!(engine.calls("steps.record")[0]["computerId"], "id-main");
}
