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

async fn call(
    write: &mut (impl tokio::io::AsyncWriteExt + Unpin),
    read: &mut (impl tokio::io::AsyncBufReadExt + Unpin),
    id: u64,
    name: &str,
    arguments: Value,
) -> Value {
    let request = json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
                         "params": {"name": name, "arguments": arguments}});
    write
        .write_all(format!("{request}\n").as_bytes())
        .await
        .unwrap();
    let mut line = String::new();
    read.read_line(&mut line).await.unwrap();
    serde_json::from_str(&line).unwrap()
}

#[tokio::test]
async fn the_mcp_server_drives_a_computer_and_maps_clicks_to_the_screen() {
    use rmcp::ServiceExt;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let shot = json!({"content": [{"type": "image", "data": png_base64(2560, 1600), "mimeType": "image/png"}]});
    let engine = FakeEngine::start(move |method, _| match method {
        "computers.list" => Ok(listing(&[("t1", "running")])),
        "guest.exec" => {
            Ok(json!({"exitCode": 0, "stdout": bytes(&shot.to_string()), "stderr": []}))
        }
        other => Err(("internal", format!("unexpected {other}"))),
    });
    let (server_side, client_side) = tokio::io::duplex(8 * 1024 * 1024);
    let (server_read, server_write) = tokio::io::split(server_side);
    let server = silo_cli::mcp::SiloMcp::new(engine.socket.clone(), std::env::temp_dir());
    tokio::spawn(async move {
        let running = server.serve((server_read, server_write)).await.unwrap();
        let _ = running.waiting().await;
    });
    let (read, mut write) = tokio::io::split(client_side);
    let mut read = BufReader::new(read);
    let init = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "protocolVersion": "2025-06-18", "capabilities": {},
        "clientInfo": {"name": "test", "version": "0"}}});
    write
        .write_all(format!("{init}\n").as_bytes())
        .await
        .unwrap();
    let mut line = String::new();
    read.read_line(&mut line).await.unwrap();
    write
        .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n")
        .await
        .unwrap();
    let list = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"});
    write
        .write_all(format!("{list}\n").as_bytes())
        .await
        .unwrap();
    line.clear();
    read.read_line(&mut line).await.unwrap();
    let tools: Value = serde_json::from_str(&line).unwrap();
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    for expected in [
        "fork_gui",
        "computer_use_ready",
        "computer_use_screenshot",
        "computer_use_click",
        "computer_use_type",
        "computer_use_key",
        "computer_use_scroll",
        "computer_use_move",
        "computer_use_drag",
        "computer_use_wait",
        "computer_use_tools",
        "computer_use_call",
    ] {
        assert!(
            names.contains(&expected),
            "{expected} missing from {names:?}"
        );
    }

    let screenshot = call(
        &mut write,
        &mut read,
        3,
        "computer_use_screenshot",
        json!({"computer": "t1"}),
    )
    .await;
    let content = &screenshot["result"]["content"];
    assert_eq!(content[0]["type"], "image", "{screenshot}");
    assert_eq!(content[0]["mimeType"], "image/png");
    assert!(content[1]["text"].as_str().unwrap().contains("1280x800"));

    let clicked = call(
        &mut write,
        &mut read,
        4,
        "computer_use_click",
        json!({"computer": "t1", "x": 100, "y": 50}),
    )
    .await;
    assert!(clicked["result"].is_object(), "{clicked}");
    let sent = engine.calls("guest.exec");
    let args = &exec_stdin(sent.last().unwrap())["params"];
    assert_eq!(args["name"], "click");
    assert_eq!(args["arguments"]["x"], 200.0);
    assert_eq!(args["arguments"]["y"], 100.0);

    let refused = call(
        &mut write,
        &mut read,
        5,
        "fork_gui",
        json!({"computer_use": "auto"}),
    )
    .await;
    assert_eq!(refused["result"]["isError"], true, "{refused}");
    assert!(refused["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("computer_use_auto"));
    assert!(engine.calls("checkpoints.fork").is_empty());
}
