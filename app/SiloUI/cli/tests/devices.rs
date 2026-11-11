//! Commands that act on a connected device, and lists across devices. The engine is a fake
//! `engine.sock`; no device is contacted.

mod fake_engine;

use fake_engine::{listing, FakeEngine};
use serde_json::{json, Value};
use silo_cli::cli::{run, Cli};

const MINI: &str = "mini-0000";
const DOWN: &str = "down-0000";

fn request(id: &str) -> Value {
    json!({
        "id": id, "forkId": "id-try", "target": "id-main", "createdAt": "t",
        "reviewDigest": "d1", "summary": "code: 2 files", "status": "open", "counts": {}
    })
}

/// An engine connected to a Mac mini and an unreachable laptop. Its own request list is empty;
/// the merged list holds one request from the mini.
fn engine() -> FakeEngine {
    FakeEngine::start(|method, params| {
        let device = params["deviceId"].as_str();
        match method {
            "connections.devices.list" => Ok(json!([
                {"id": MINI, "name": "Mac mini", "address": "ssh://mini"},
                {"id": DOWN, "name": "Laptop", "address": "ssh://laptop"}
            ])),
            "connections.devices.health" => Ok(json!({"devices": [
                {"deviceId": MINI, "connected": true, "busy": false, "error": null},
                {"deviceId": DOWN, "connected": false, "busy": false, "error": "Connection refused"}
            ]})),
            "computers.list" if device == Some(DOWN) => {
                Err(("internal", "This device is no longer connected.".to_owned()))
            }
            "computers.list" => Ok(listing(&[("main", "running")])),
            "changes.requests.list" if params["allDevices"] == true => Ok(json!({
                "requests": [{
                    "request": request("fr-mini"), "forkName": "try", "forkState": "running",
                    "deviceId": MINI, "deviceName": "Mac mini"
                }],
                "unreachable": [{"deviceId": DOWN, "deviceName": "Laptop", "reason": "gone"}]
            })),
            "changes.requests.list" if device == Some(MINI) => Ok(json!({"requests": [{
                "request": request("fr-mini"), "forkName": "try", "forkState": "running"
            }]})),
            "changes.requests.list" => Ok(json!({"requests": []})),
            "changes.requests.throwAway" => Ok(json!({"jobId": "job:7"})),
            "operations.job.get" => Ok(json!({"jobId": "job:7", "state": "finished",
                "outcome": {"result": {"request": request("fr-mini"), "dropped": "try"}}})),
            "operations.job.acknowledge" => Ok(Value::Null),
            other => Err(("internal", format!("unexpected {other}"))),
        }
    })
}

fn cli(engine: &FakeEngine, args: &[&str]) -> Cli {
    let socket = engine.socket.to_str().unwrap();
    let mut all = vec!["silo", "--socket", socket, "--allow-other-channel"];
    all.extend_from_slice(args);
    <Cli as clap::Parser>::try_parse_from(all).unwrap()
}

async fn exit_code(engine: &FakeEngine, args: &[&str]) -> i32 {
    run(cli(engine, args), std::env::temp_dir().as_path())
        .await
        .unwrap()
}

async fn error(engine: &FakeEngine, args: &[&str]) -> String {
    run(cli(engine, args), std::env::temp_dir().as_path())
        .await
        .unwrap_err()
        .message
}

#[tokio::test]
async fn a_command_with_device_sends_every_routed_request_to_that_device() {
    let engine = engine();
    assert_eq!(
        exit_code(&engine, &["--device", "mac mini", "list"]).await,
        0
    );
    assert_eq!(engine.calls("computers.list")[0]["deviceId"], MINI);
    // The job that `requests throw-away` starts is followed on the same device.
    let args = ["--device", MINI, "requests", "throw-away", "fr-mini"];
    assert_eq!(exit_code(&engine, &args).await, 0);
    assert_eq!(
        engine.calls("changes.requests.throwAway")[0]["deviceId"],
        MINI
    );
    assert_eq!(engine.calls("operations.job.get")[0]["deviceId"], MINI);
    assert_eq!(
        engine.calls("operations.job.acknowledge")[0]["deviceId"],
        MINI
    );
    // The device list is this device's own.
    assert!(engine.calls("connections.devices.list")[0]
        .get("deviceId")
        .is_none());
}

#[tokio::test]
async fn an_unknown_device_names_the_connected_ones() {
    let engine = engine();
    let message = error(&engine, &["--device", "nas", "list"]).await;
    assert!(message.contains("Mac mini"), "{message}");
    assert!(engine.calls("computers.list").is_empty());
}

#[tokio::test]
async fn commands_that_work_on_this_device_refuse_a_device() {
    let engine = engine();
    for args in [
        vec!["--device", "mac mini", "secrets", "list"],
        vec!["--device", "mac mini", "fork", "./repo"],
        vec!["--device", "mac mini", "ports", "main"],
    ] {
        let message = error(&engine, &args).await;
        assert!(message.contains("--device"), "{message}");
    }
    assert!(engine.calls("computers.list").is_empty());
}

#[tokio::test]
async fn list_across_devices_asks_each_device_and_reports_the_one_it_cannot_reach() {
    let engine = engine();
    assert_eq!(exit_code(&engine, &["list", "--all-devices"]).await, 0);
    let asked: Vec<Option<String>> = engine
        .calls("computers.list")
        .iter()
        .map(|params| params["deviceId"].as_str().map(str::to_owned))
        .collect();
    assert_eq!(
        asked,
        vec![None, Some(MINI.to_owned())],
        "a device known to be offline is not asked"
    );
}

#[tokio::test]
async fn requests_across_devices_asks_for_every_device_at_once() {
    let engine = engine();
    assert_eq!(exit_code(&engine, &["requests", "--all-devices"]).await, 0);
    let asked = engine.calls("changes.requests.list");
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0]["allDevices"], true);
    assert!(asked[0].get("deviceId").is_none());
}

#[tokio::test]
async fn an_action_on_a_request_of_another_device_goes_to_that_device() {
    let engine = engine();
    let args = ["requests", "throw-away", "fr-mini"];
    assert_eq!(exit_code(&engine, &args).await, 0);
    let thrown = engine.calls("changes.requests.throwAway");
    assert_eq!(thrown.len(), 1);
    assert_eq!(thrown[0]["deviceId"], MINI);
    assert_eq!(thrown[0]["requestId"], "fr-mini");
    assert_eq!(engine.calls("operations.job.get")[0]["deviceId"], MINI);
}

#[tokio::test]
async fn an_action_on_a_request_of_this_device_stays_here() {
    let engine = FakeEngine::start(|method, params| match method {
        "changes.requests.list" => Ok(json!({"requests": [{
            "request": request("fr-here"), "forkName": "try", "forkState": "running"
        }]})),
        "changes.requests.throwAway" => Ok(json!({"jobId": "job:7"})),
        "operations.job.get" => Ok(json!({"jobId": "job:7", "state": "finished",
            "outcome": {"result": {"request": request("fr-here"), "dropped": "try"}}})),
        "operations.job.acknowledge" => Ok(Value::Null),
        other => Err(("internal", format!("unexpected {other} {params}"))),
    });
    let args = ["requests", "throw-away", "fr-here"];
    assert_eq!(exit_code(&engine, &args).await, 0);
    assert!(engine.calls("changes.requests.throwAway")[0]
        .get("deviceId")
        .is_none());
}

#[tokio::test]
async fn a_slow_device_is_reported_instead_of_waited_for() {
    let engine = FakeEngine::start(|method, params| match method {
        "connections.devices.list" => Ok(json!([
            {"id": MINI, "name": "Mac mini", "address": "ssh://mini"}
        ])),
        "computers.list" if params["deviceId"] == MINI => {
            std::thread::sleep(std::time::Duration::from_millis(400));
            Ok(listing(&[("main", "running")]))
        }
        "computers.list" => Ok(listing(&[("main", "running")])),
        other => Err(("internal", format!("unexpected {other}"))),
    });
    let socket = engine.socket.clone();
    let connected = silo_cli::engine::Engine::connect(&socket).await.unwrap();
    let listed = silo_cli::ops::list_everywhere_within(
        &connected,
        false,
        std::time::Duration::from_millis(50),
    )
    .await
    .unwrap();
    assert_eq!(listed["computers"].as_array().unwrap().len(), 1);
    assert_eq!(listed["unreachable"][0]["deviceId"], MINI);
    assert!(listed["unreachable"][0]["reason"]
        .as_str()
        .unwrap()
        .contains("in time"));
}

#[tokio::test]
async fn a_request_id_on_two_devices_is_refused_until_a_device_is_named() {
    let engine = FakeEngine::start(|method, params| match method {
        "connections.devices.list" => Ok(json!([
            {"id": MINI, "name": "Mac mini", "address": "a"}
        ])),
        "changes.requests.list" if params["allDevices"] == true => Ok(json!({"requests": [
            {"request": request("fr-same"), "deviceId": MINI, "deviceName": "Mac mini"}
        ]})),
        "changes.requests.list" => Ok(json!({"requests": [{"request": request("fr-same")}]})),
        "changes.requests.throwAway" => Ok(json!({"jobId": "job:7"})),
        "operations.job.get" => Ok(json!({"jobId": "job:7", "state": "finished",
            "outcome": {"result": {"request": request("fr-same"), "dropped": "try"}}})),
        "operations.job.acknowledge" => Ok(Value::Null),
        other => Err(("internal", format!("unexpected {other}"))),
    });
    let message = error(&engine, &["requests", "throw-away", "fr-same"]).await;
    assert!(
        message.contains("this device and Mac mini") && message.contains("--device"),
        "{message}"
    );
    assert!(engine.calls("changes.requests.throwAway").is_empty());
    let args = ["--device", "mac mini", "requests", "throw-away", "fr-same"];
    assert_eq!(exit_code(&engine, &args).await, 0);
    assert_eq!(
        engine.calls("changes.requests.throwAway")[0]["deviceId"],
        MINI
    );
}

#[tokio::test]
async fn opening_a_request_on_another_device_drops_its_ssh_name_and_loopback_urls() {
    let engine = FakeEngine::start(|method, _| match method {
        "connections.devices.list" => Ok(json!([
            {"id": MINI, "name": "Mac mini", "address": "a"}
        ])),
        "changes.requests.open" => Ok(json!({"jobId": "job:7"})),
        "operations.job.get" => Ok(
            json!({"jobId": "job:7", "state": "finished", "outcome": {"result": {
                "request": request("fr-mini"), "computerId": "c", "name": "try", "state": "running",
                "started": false, "sshAlias": "try.silo", "urls": ["http://127.0.0.1:3000"]
            }}}),
        ),
        "operations.job.acknowledge" => Ok(Value::Null),
        other => Err(("internal", format!("unexpected {other}"))),
    });
    let connected = silo_cli::engine::Engine::connect(&engine.socket)
        .await
        .unwrap();
    let id = connected.resolve_device("mac mini").await.unwrap();
    let opened = silo_cli::review::open(&connected.on_device(Some(id)), "fr-mini", &|_| {})
        .await
        .unwrap();
    assert!(opened.get("sshAlias").is_none(), "{opened}");
    assert_eq!(opened["urls"], json!([]));
    assert_eq!(opened["deviceUrls"][0], "http://127.0.0.1:3000");
}

#[tokio::test]
async fn an_engine_without_device_routing_refuses_device() {
    let engine = FakeEngine::start_with(&[], |method, _| {
        Err(("internal", format!("unexpected {method}")))
    });
    let message = error(&engine, &["--device", "mini", "list"]).await;
    assert!(message.contains("older"), "{message}");
    assert!(engine.calls("computers.list").is_empty());
}

#[tokio::test]
async fn nothing_that_would_run_here_is_sent_under_device() {
    let engine = engine();
    // `status` reads secrets and GitHub access from this device's engine; on another device
    // it must not ask them.
    let _ = run(
        cli(&engine, &["--device", "mac mini", "status", "main"]),
        std::env::temp_dir().as_path(),
    )
    .await;
    assert!(engine.calls("secrets.list").is_empty());
    assert!(engine.calls("github.state").is_empty());
}

/// An engine connected to the Mac mini whose computers live there: `main`, and `t1` once
/// it is forked. Commands run in them exit 0 with `hi` on stdout.
fn host_engine() -> FakeEngine {
    let forked = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    FakeEngine::start(move |method, _| {
        use std::sync::atomic::Ordering;
        match method {
            "connections.devices.list" => Ok(json!([
                {"id": MINI, "name": "Mac mini", "address": "ssh://mini"}
            ])),
            "computers.list" if forked.load(Ordering::SeqCst) => {
                Ok(listing(&[("main", "running"), ("t1", "running")]))
            }
            "computers.list" => Ok(listing(&[("main", "running")])),
            "guest.exec" => Ok(json!({"exitCode": 0, "signal": null,
                "stdout": b"hi\n".to_vec(), "stderr": []})),
            "steps.record" => Ok(json!({"step": {"number": 1}})),
            "checkpoints.fork" => {
                forked.store(true, Ordering::SeqCst);
                Ok(json!({"jobId": "job:1"}))
            }
            "computers.start" => Ok(json!({"jobId": "job:1"})),
            "guest.exec.start" => Ok(json!({"jobId": "job:2"})),
            "guest.exec.get" => Ok(
                json!({"state": "exited", "exitCode": 0, "firstRetainedSeq": 1,
                "output": [{"seq": 1, "stream": "stdout", "data": b"hi\n".to_vec()}]}),
            ),
            "operations.job.get" => Ok(fake_engine::finished(json!({"exitCode": 0}))),
            "operations.job.acknowledge" => Ok(Value::Null),
            other => Err(("internal", format!("unexpected {other}"))),
        }
    })
}

#[tokio::test]
async fn exec_runs_the_command_on_the_device_the_computer_is_on() {
    let engine = host_engine();
    let args = ["--device", "mac mini", "exec", "main", "--", "ls", "-l"];
    assert_eq!(exit_code(&engine, &args).await, 0);
    let ran = engine.calls("guest.exec");
    assert_eq!(ran.len(), 1);
    assert_eq!(ran[0]["deviceId"], MINI);
    assert_eq!(ran[0]["computerId"], "id-main");
    assert_eq!(ran[0]["argv"], json!(["ls", "-l"]));
    // The step is recorded on that device too.
    assert_eq!(engine.calls("steps.record")[0]["deviceId"], MINI);
}

#[tokio::test]
async fn fork_with_a_command_forks_and_runs_on_the_device() {
    let engine = host_engine();
    let args = [
        "--device", "mac mini", "--json", "fork", "main", "--name", "t1", "--", "make", "test",
    ];
    assert_eq!(exit_code(&engine, &args).await, 0);
    for method in [
        "checkpoints.fork",
        "computers.start",
        "guest.exec.start",
        "guest.exec.get",
        "operations.job.get",
        "steps.record",
    ] {
        let calls = engine.calls(method);
        assert!(!calls.is_empty(), "{method} was not sent");
        assert!(
            calls.iter().all(|params| params["deviceId"] == MINI),
            "{method}: {calls:?}"
        );
    }
    assert_eq!(
        engine.calls("guest.exec.start")[0]["argv"],
        json!(["make", "test"])
    );
    // Ports and URLs are this device's: nothing is published or read for a fork there.
    for local in ["computers.access", "network.publish", "network.urls"] {
        assert!(engine.calls(local).is_empty(), "{local} was sent");
    }
}
