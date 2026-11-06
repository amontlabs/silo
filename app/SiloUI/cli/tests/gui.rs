mod fake_engine;

use std::{
    io::Cursor,
    sync::{Arc, Mutex},
    time::Duration,
};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use fake_engine::{finished, listing, FakeEngine};
use serde_json::{json, Value};
use silo_api::methods::computer_use::ApprovalMode;
use silo_cli::{
    bridge,
    engine::Engine,
    gui::{self, GuiArgs},
    ops::ForkArgs,
};

fn ignore(_: &str) {}

fn png_base64(width: u32, height: u32) -> String {
    let mut out = Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(width, height)
        .write_to(&mut out, image::ImageFormat::Png)
        .unwrap();
    STANDARD.encode(out.into_inner())
}

fn bytes(text: &str) -> Value {
    json!(text.bytes().collect::<Vec<u8>>())
}

fn exec_stdin(params: &Value) -> Value {
    let raw: Vec<u8> = params["stdin"]
        .as_array()
        .unwrap()
        .iter()
        .map(|byte| byte.as_u64().unwrap() as u8)
        .collect();
    serde_json::from_slice(&raw).unwrap()
}

/// An engine with `main` running and a `t1` fork that appears when it is forked, whose
/// desktop reports `starting` for the first poll and ready after.
fn fork_engine() -> (FakeEngine, Arc<Mutex<Vec<String>>>) {
    let forked = Arc::new(Mutex::new(Vec::<String>::new()));
    let order = Arc::new(Mutex::new(Vec::<String>::new()));
    let (seen, log) = (forked.clone(), order.clone());
    let polls = Arc::new(Mutex::new(0));
    let engine = FakeEngine::start(move |method, params| {
        log.lock().unwrap().push(method.to_owned());
        match method {
            "computers.list" => {
                let mut computers = vec![("main", "running")];
                if !seen.lock().unwrap().is_empty() {
                    computers.push(("t1", "stopped"));
                }
                Ok(listing(&computers))
            }
            "checkpoints.fork" => {
                seen.lock()
                    .unwrap()
                    .push(params["newName"].as_str().unwrap().into());
                Ok(json!({"jobId": "job:1"}))
            }
            "computers.start" => Ok(json!({"jobId": "job:1"})),
            "operations.job.get" => Ok(finished(json!({}))),
            "operations.job.acknowledge" => Ok(Value::Null),
            "computerUse.approval" => {
                Ok(json!({"state": "unavailable", "approval": params["mode"]}))
            }
            "desktop.status" => {
                let mut count = polls.lock().unwrap();
                *count += 1;
                let (session, driver) = if *count == 1 {
                    ("starting", "installing")
                } else {
                    ("running", "ready")
                };
                Ok(json!({
                    "state": "running", "sessionState": session,
                    "computerUse": {"state": driver, "approval": "auto", "driverVersion": "0.38.0"}
                }))
            }
            other => Err(("internal", format!("unexpected {other}"))),
        }
    });
    (engine, order)
}

#[tokio::test]
async fn fork_gui_sets_the_mode_starts_and_waits_for_the_driver() {
    let (engine, order) = fork_engine();
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let args = GuiArgs {
        fork: ForkArgs {
            source: "main".into(),
            name: Some("t1".into()),
            checkpoint: Some("cp1".into()),
            ..ForkArgs::default()
        },
        approval: Some(ApprovalMode::Auto),
        timeout: Duration::from_secs(60),
    };
    let fork = gui::fork_gui(&connected, &args, None, &ignore)
        .await
        .unwrap();
    assert_eq!(fork["name"], "t1");
    assert_eq!(fork["computerUse"]["state"], "ready");
    assert_eq!(fork["computerUse"]["approval"], "auto");
    assert_eq!(fork["computerUse"]["driverVersion"], "0.38.0");
    let sent = &engine.calls("computerUse.approval")[0];
    assert_eq!(sent["computerId"], "id-t1");
    assert_eq!(sent["mode"], "auto");
    let order = order.lock().unwrap();
    let position = |name: &str| order.iter().position(|m| m == name).unwrap();
    assert!(position("checkpoints.fork") < position("computerUse.approval"));
    assert!(position("computerUse.approval") < position("computers.start"));
    assert!(position("computers.start") < position("desktop.status"));
    assert_eq!(engine.calls("desktop.status").len(), 2);
}

#[tokio::test]
async fn main_never_takes_the_approval_mode() {
    let (engine, _) = fork_engine();
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let main = connected.computer("main").await.unwrap();
    let error = gui::set_fork_approval(&connected, &main, ApprovalMode::Auto)
        .await
        .unwrap_err();
    assert!(
        error.message.contains("main keeps asking"),
        "{}",
        error.message
    );
    assert!(engine.calls("computerUse.approval").is_empty());
}

#[tokio::test]
async fn a_desktop_that_never_gets_ready_times_out_and_says_where_it_stands() {
    let engine = FakeEngine::start(|method, _| match method {
        "computers.list" => Ok(listing(&[("t1", "running")])),
        "desktop.status" => Ok(json!({
            "state": "running", "sessionState": "starting",
            "computerUse": {"state": "installing"}
        })),
        other => Err(("internal", format!("unexpected {other}"))),
    });
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let t1 = connected.computer("t1").await.unwrap();
    let error = gui::wait_ready(&connected, &t1, Duration::ZERO, &ignore)
        .await
        .unwrap_err();
    assert!(error.message.contains("not ready"), "{}", error.message);
    assert!(
        error.message.contains("computer use installing"),
        "{}",
        error.message
    );
}

#[tokio::test]
async fn a_screenshot_runs_the_driver_through_guest_exec_and_comes_back_scaled() {
    let shot = png_base64(2560, 1600);
    let reply = json!({"content": [{"type": "image", "data": shot, "mimeType": "image/png"}]});
    let engine = FakeEngine::start(move |method, _| match method {
        "computers.list" => Ok(listing(&[("t1", "running")])),
        "guest.exec" => {
            Ok(json!({"exitCode": 0, "stdout": bytes(&reply.to_string()), "stderr": []}))
        }
        other => Err(("internal", format!("unexpected {other}"))),
    });
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let t1 = connected.computer("t1").await.unwrap();
    let result = bridge::driver_request(
        &connected,
        &t1,
        "tools/call",
        json!({"name": "screenshot", "arguments": {}}),
        60,
    )
    .await
    .unwrap();
    let answer = silo_cli::usetools::answer_of(&result, 1280).unwrap();
    assert_eq!(answer.scale, Some(2.0));
    let call = &engine.calls("guest.exec")[0];
    assert_eq!(call["argv"][0], "python3");
    assert_eq!(call["argv"][4], "/usr/local/libexec/silo-cua-driver-mcp");
    assert_eq!(exec_stdin(call)["params"]["name"], "screenshot");
}

#[tokio::test]
async fn a_result_left_in_a_file_is_read_in_parts_and_removed() {
    let result = json!({"content": [{"type": "text", "text": "x".repeat(1_100_000)}]}).to_string();
    let data = result.clone().into_bytes();
    let total = data.len();
    let header = json!({"silo_result_file": "/tmp/silo-cu.bridge-77", "bytes": total}).to_string();
    let engine = FakeEngine::start(move |method, params| match method {
        "computers.list" => Ok(listing(&[("t1", "running")])),
        "guest.exec" => {
            let argv: Vec<&str> = params["argv"]
                .as_array()
                .unwrap()
                .iter()
                .map(|word| word.as_str().unwrap())
                .collect();
            let stdout = match argv[0] {
                "rm" => json!([]),
                _ if argv[2].contains("f.seek") => {
                    let offset: usize = argv[4].parse().unwrap();
                    let length: usize = argv[5].parse().unwrap();
                    json!(data[offset..(offset + length).min(total)].to_vec())
                }
                _ => bytes(&header),
            };
            Ok(json!({"exitCode": 0, "stdout": stdout, "stderr": []}))
        }
        other => Err(("internal", format!("unexpected {other}"))),
    });
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let t1 = connected.computer("t1").await.unwrap();
    let got = bridge::driver_request(
        &connected,
        &t1,
        "tools/call",
        json!({"name": "screenshot"}),
        60,
    )
    .await
    .unwrap();
    assert_eq!(got.to_string(), result);
    let calls = engine.calls("guest.exec");
    assert_eq!(calls.len(), 1 + 3 + 1, "bridge, three parts, removal");
    assert_eq!(calls.last().unwrap()["argv"][0], "rm");
}

#[tokio::test]
async fn a_result_file_outside_the_bridge_prefix_is_refused() {
    let header = json!({"silo_result_file": "/etc/passwd", "bytes": 10}).to_string();
    let engine = FakeEngine::start(move |method, _| match method {
        "computers.list" => Ok(listing(&[("t1", "running")])),
        "guest.exec" => Ok(json!({"exitCode": 0, "stdout": bytes(&header), "stderr": []})),
        other => Err(("internal", format!("unexpected {other}"))),
    });
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let t1 = connected.computer("t1").await.unwrap();
    let error = bridge::driver_request(
        &connected,
        &t1,
        "tools/call",
        json!({"name": "screenshot"}),
        60,
    )
    .await
    .unwrap_err();
    assert!(error.message.contains("cannot be read"));
    assert_eq!(engine.calls("guest.exec").len(), 1);
}

#[tokio::test]
async fn a_driver_that_is_missing_is_reported_in_words() {
    let engine = FakeEngine::start(|method, _| match method {
        "computers.list" => Ok(listing(&[("t1", "running")])),
        "guest.exec" => Ok(json!({
            "exitCode": 2, "stdout": [], "stderr": bytes("cannot start the driver: No such file\n")
        })),
        other => Err(("internal", format!("unexpected {other}"))),
    });
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let t1 = connected.computer("t1").await.unwrap();
    let error = bridge::driver_request(&connected, &t1, "tools/list", json!({}), 60)
        .await
        .unwrap_err();
    assert!(
        error.message.contains("no computer-use driver"),
        "{}",
        error.message
    );
}
