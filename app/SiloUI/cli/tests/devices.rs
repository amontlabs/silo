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
        vec!["--device", "mac mini", "exec", "main", "--", "ls"],
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
        vec![None, Some(MINI.to_owned()), Some(DOWN.to_owned())]
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
    assert!(engine.calls("connections.devices.list").is_empty());
}
