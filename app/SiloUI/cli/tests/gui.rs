mod fake_engine;

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use fake_engine::{finished, listing, FakeEngine};
use serde_json::{json, Value};
use silo_api::methods::computer_use::ApprovalMode;
use silo_cli::{
    engine::Engine,
    gui::{self, GuiArgs},
    ops::ForkArgs,
};

fn ignore(_: &str) {}

const MINI: &str = "mini-0000";

/// An engine with `main` running and a `t1` fork that appears when it is forked, whose
/// desktop reports `starting` for the first poll and ready after.

fn tree() -> Value {
    json!({"nodes": [
        {"id": "id-main", "name": "main", "parent": null},
        {"id": "id-t1", "name": "t1", "parent": "id-main"},
        {"id": "id-old", "name": "main-before-1", "parent": null}]})
}

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
            "computers.tree" => Ok(tree()),
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

/// A fake driver behind `guest.exec.start`, `.input` and `.get`: answers each JSON-RPC line
/// written to it with one line of output.
#[derive(Default)]
struct Driver {
    started: usize,
    exited: bool,
    /// A tool that makes the driver exit without answering.
    die_on: Option<String>,
    output: Vec<(u64, String, Vec<u8>)>,
    requests: Vec<Value>,
}

fn driver_engine(driver: Arc<Mutex<Driver>>, screenshot: Value) -> FakeEngine {
    FakeEngine::start(move |method, params| {
        let mut state = driver.lock().unwrap();
        match method {
            "computers.list" => Ok(listing(&[
                ("main", "running"),
                ("t1", "running"),
                ("main-before-1", "stopped"),
            ])),
            "computers.tree" => Ok(tree()),
            "connections.devices.list" => Ok(json!([
                {"id": MINI, "name": "Mac mini", "address": "ssh://mini"}
            ])),
            "desktop.status" => Ok(json!({
                "state": "running", "sessionState": "running",
                "computerUse": {"state": "ready", "approval": "ask"}
            })),
            "computers.delete" => Ok(json!({"jobId": "job:900"})),
            "operations.job.get" => Ok(finished(json!({}))),
            "operations.job.acknowledge" => Ok(Value::Null),
            "guest.exec" => {
                let script = params["argv"][2].as_str().unwrap_or_default();
                let stdout: Vec<u8> = if script.contains("wc -c") {
                    b"9\n".to_vec()
                } else if script.contains("tail -c") {
                    b"PNG-bytes".to_vec()
                } else {
                    Vec::new()
                };
                Ok(json!({"exitCode": 0, "stdout": stdout, "stderr": []}))
            }
            "guest.exec.start" => {
                state.started += 1;
                state.exited = false;
                state.output.clear();
                Ok(json!({"jobId": format!("job:{}", state.started)}))
            }
            "guest.exec.input" if state.exited => {
                Err(("internal", "the command has exited".into()))
            }
            "guest.exec.input" => {
                let raw: Vec<u8> = params["data"]
                    .as_array()
                    .map(|bytes| bytes.iter().map(|b| b.as_u64().unwrap() as u8).collect())
                    .unwrap_or_default();
                if let Ok(message) = serde_json::from_slice::<Value>(&raw) {
                    state.requests.push(message.clone());
                    let dies = message["method"] == "tools/call"
                        && state.die_on.as_deref() == message["params"]["name"].as_str();
                    if dies {
                        state.exited = true;
                    } else if let Some(id) = message.get("id") {
                        let result = if message["method"] == "tools/call"
                            && message["params"]["name"] == "get_desktop_state"
                        {
                            screenshot.clone()
                        } else {
                            json!({"content": [{"type": "text", "text": "ok"}]})
                        };
                        let line = format!(
                            "{}\n",
                            json!({"jsonrpc": "2.0", "id": id, "result": result})
                        );
                        let seq = state.output.len() as u64 + 1;
                        state.output.push((seq, "stdout".into(), line.into_bytes()));
                    }
                }
                Ok(Value::Null)
            }
            "guest.exec.get" => {
                let since = params["sinceSeq"].as_u64().unwrap_or(0);
                let output: Vec<Value> = state
                    .output
                    .iter()
                    .filter(|(seq, _, _)| *seq > since)
                    .map(|(seq, stream, data)| json!({"seq": seq, "stream": stream, "data": data}))
                    .collect();
                let running = !state.exited;
                Ok(json!({"state": if running { "running" } else { "exited" },
                          "exitCode": null, "output": output, "firstRetainedSeq": 1}))
            }
            other => Err(("internal", format!("unexpected {other}"))),
        }
    })
}

fn screenshot_result() -> Value {
    json!({"content": [{"type": "text", "text": "captured"}],
           "structuredContent": {"screenshot_width": 1280, "screenshot_height": 800,
                                 "screen_width": 2560, "screen_height": 1600}})
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
    tokio::time::timeout(
        std::time::Duration::from_secs(20),
        read.read_line(&mut line),
    )
    .await
    .unwrap_or_else(|_| panic!("no answer to {name}"))
    .unwrap();
    serde_json::from_str(&line).unwrap()
}

type McpWrite = tokio::io::WriteHalf<tokio::io::DuplexStream>;
type McpRead = tokio::io::BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>;

async fn connect_mcp(engine: &FakeEngine) -> (McpWrite, McpRead) {
    connect_mcp_in(engine, &std::env::temp_dir(), None).await
}

/// An MCP server working in `directory`, for the agent `agent`.
async fn connect_mcp_in(
    engine: &FakeEngine,
    directory: &std::path::Path,
    agent: Option<&str>,
) -> (McpWrite, McpRead) {
    use rmcp::ServiceExt;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let (server_side, client_side) = tokio::io::duplex(8 * 1024 * 1024);
    let (server_read, server_write) = tokio::io::split(server_side);
    let server = silo_cli::mcp::SiloMcp::new(engine.socket.clone(), directory.to_owned())
        .with_agent(agent.map(str::to_owned));
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
    (write, read)
}

#[tokio::test]
async fn the_mcp_server_keeps_one_driver_session_per_fork_and_refuses_main() {
    let driver = Arc::new(Mutex::new(Driver::default()));
    let engine = driver_engine(driver.clone(), screenshot_result());
    let (mut write, mut read) = connect_mcp(&engine).await;

    let screenshot = call(
        &mut write,
        &mut read,
        3,
        "computer_use_screenshot",
        json!({"computer": "t1"}),
    )
    .await;
    let content = screenshot["result"]["content"].as_array().unwrap();
    let image = content.iter().find(|part| part["type"] == "image").unwrap();
    assert_eq!(
        image["data"], "UE5HLWJ5dGVz",
        "the file's bytes, base64: {screenshot}"
    );
    assert!(content.last().unwrap()["text"]
        .as_str()
        .unwrap()
        .contains("1280x800"));
    // The picture travelled through a temporary file, which was removed again.
    let calls = engine.calls("guest.exec");
    assert_eq!(calls.last().unwrap()["argv"][0], "rm");
    let asked = driver.lock().unwrap().requests.clone();
    let args = &asked
        .iter()
        .find(|r| r["params"]["name"] == "get_desktop_state")
        .unwrap()["params"]["arguments"];
    assert!(args["screenshot_out_file"]
        .as_str()
        .unwrap()
        .starts_with("/tmp/silo-cu.shot-"));

    let clicked = call(
        &mut write,
        &mut read,
        4,
        "computer_use_click",
        json!({"computer": "t1", "x": 100, "y": 50, "double": true}),
    )
    .await;
    assert!(clicked["result"].is_object(), "{clicked}");
    {
        let state = driver.lock().unwrap();
        assert_eq!(state.started, 1, "one driver process serves both calls");
        let calls: Vec<&Value> = state
            .requests
            .iter()
            .filter(|r| r["method"] == "tools/call")
            .collect();
        assert_eq!(calls[0]["params"]["name"], "get_desktop_state");
        assert_eq!(calls[0]["params"]["arguments"]["max_image_dimension"], 1280);
        assert_eq!(calls[1]["params"]["name"], "click");
        assert_eq!(calls[1]["params"]["arguments"]["count"], 2);
        assert_eq!(calls[1]["params"]["arguments"]["x"], 100.0);
        assert_eq!(state.requests[0]["method"], "initialize");
    }

    let refused = call(
        &mut write,
        &mut read,
        5,
        "computer_use_screenshot",
        json!({"computer": "main"}),
    )
    .await;
    assert_eq!(refused["result"]["isError"], true, "{refused}");
    assert!(refused["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("not a fork"));
    let archived = call(
        &mut write,
        &mut read,
        8,
        "computer_use_screenshot",
        json!({"computer": "main-before-1"}),
    )
    .await;
    assert_eq!(archived["result"]["isError"], true, "{archived}");
    assert_eq!(driver.lock().unwrap().started, 1);

    let auto = call(
        &mut write,
        &mut read,
        6,
        "fork_gui",
        json!({"computer_use": "auto"}),
    )
    .await;
    assert_eq!(auto["result"]["isError"], true, "{auto}");
    assert!(auto["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("computer_use_auto"));
    let folder = call(
        &mut write,
        &mut read,
        7,
        "fork_gui",
        json!({"source": "./some/dir"}),
    )
    .await;
    assert!(folder["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("folder"));
    assert!(engine.calls("checkpoints.fork").is_empty());
}

#[tokio::test]
async fn an_exited_driver_is_started_again() {
    let driver = Arc::new(Mutex::new(Driver::default()));
    let engine = driver_engine(driver.clone(), screenshot_result());
    let (mut write, mut read) = connect_mcp(&engine).await;
    let first = call(
        &mut write,
        &mut read,
        3,
        "computer_use_type",
        json!({"computer": "t1", "text": "a"}),
    )
    .await;
    assert!(first["result"].is_object(), "{first}");
    driver.lock().unwrap().exited = true;
    let second = call(
        &mut write,
        &mut read,
        4,
        "computer_use_type",
        json!({"computer": "t1", "text": "b"}),
    )
    .await;
    assert!(
        second["result"].is_object() && second["result"]["isError"] != true,
        "{second}"
    );
    assert_eq!(driver.lock().unwrap().started, 2);
}

#[test]
fn every_call_matches_the_pinned_driver_contract() {
    use silo_cli::usetools::{driver_call, Action};

    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/cua-driver-0.38.0-tools.json")).unwrap();
    let actions = [
        Action::Screenshot { max_side: 800 },
        Action::Click {
            x: 1.0,
            y: 2.0,
            button: "left".into(),
            double: false,
        },
        Action::Type { text: "x".into() },
        Action::Key {
            key: "Return".into(),
            modifiers: vec!["ctrl".into()],
        },
        Action::Hotkey {
            keys: vec!["ctrl".into(), "l".into()],
        },
        Action::Scroll {
            x: 1.0,
            y: 2.0,
            direction: "down".into(),
            amount: 3,
            by: Some("page".into()),
        },
        Action::Move { x: 1.0, y: 2.0 },
        Action::Drag {
            from: (1.0, 2.0),
            to: (3.0, 4.0),
            button: Some("left".into()),
        },
    ];
    for action in actions {
        let (tool, args) = driver_call(&action);
        let spec = &fixture["tools"][tool];
        assert!(spec.is_object(), "the pinned driver has no tool {tool}");
        let allowed: Vec<&str> = spec["properties"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p.as_str().unwrap())
            .collect();
        for key in args.as_object().unwrap().keys() {
            assert!(
                allowed.contains(&key.as_str()),
                "{tool} has no argument {key}"
            );
        }
        for key in spec["required"].as_array().unwrap() {
            assert!(
                args.get(key.as_str().unwrap()).is_some(),
                "{tool} needs {key}"
            );
        }
    }
}

#[tokio::test]
async fn an_action_is_not_repeated_on_a_new_driver_after_the_driver_died_handling_it() {
    let driver = Arc::new(Mutex::new(Driver::default()));
    driver.lock().unwrap().die_on = Some("click".into());
    let engine = driver_engine(driver.clone(), screenshot_result());
    let (mut write, mut read) = connect_mcp(&engine).await;
    let clicked = call(
        &mut write,
        &mut read,
        3,
        "computer_use_click",
        json!({"computer": "t1", "x": 1, "y": 2}),
    )
    .await;
    assert_eq!(clicked["result"]["isError"], true, "{clicked}");
    assert!(clicked["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("may or may not"));
    let clicks = driver
        .lock()
        .unwrap()
        .requests
        .iter()
        .filter(|r| r["params"]["name"] == "click")
        .count();
    assert_eq!(clicks, 1, "the click was not replayed");
    assert_eq!(driver.lock().unwrap().started, 1);
    // The session was closed, so the next request starts a new driver.
    let typed = call(
        &mut write,
        &mut read,
        4,
        "computer_use_type",
        json!({"computer": "t1", "text": "a"}),
    )
    .await;
    assert!(typed["result"]["isError"] != true, "{typed}");
    assert_eq!(driver.lock().unwrap().started, 2);
}

fn project(settings: &str) -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join(".silo")).unwrap();
    std::fs::write(directory.path().join(".silo/silo.yaml"), settings).unwrap();
    directory
}

#[tokio::test]
async fn fork_gui_on_another_device_forks_starts_and_waits_there() {
    let (engine, _) = fork_engine();
    let connected = Engine::connect(&engine.socket)
        .await
        .unwrap()
        .on_device(Some(MINI.to_owned()));
    let args = GuiArgs {
        fork: ForkArgs {
            source: "main".into(),
            name: Some("t1".into()),
            checkpoint: Some("cp1".into()),
            ..ForkArgs::default()
        },
        approval: Some(ApprovalMode::Ask),
        timeout: Duration::from_secs(60),
    };
    let fork = gui::fork_gui(&connected, &args, None, &ignore)
        .await
        .unwrap();
    assert_eq!(fork["computerUse"]["state"], "ready");
    for method in [
        "checkpoints.fork",
        "computers.tree",
        "computerUse.approval",
        "computers.start",
        "desktop.status",
    ] {
        let calls = engine.calls(method);
        assert!(!calls.is_empty(), "{method} was not sent");
        assert!(
            calls.iter().all(|params| params["deviceId"] == MINI),
            "{method}: {calls:?}"
        );
    }
    assert!(engine.calls("network.state").is_empty());
}

#[tokio::test]
async fn a_fork_on_another_device_is_driven_there_and_its_session_closed_there() {
    let driver = Arc::new(Mutex::new(Driver::default()));
    let engine = driver_engine(driver.clone(), screenshot_result());
    let directory = project("agents:\n  bot:\n    devices: [Mac mini]\n");
    let (mut write, mut read) = connect_mcp_in(&engine, directory.path(), Some("bot")).await;

    let ready = call(
        &mut write,
        &mut read,
        3,
        "computer_use_ready",
        json!({"computer": "t1", "device": "mac mini"}),
    )
    .await;
    assert_ne!(ready["result"]["isError"], true, "{ready}");
    assert_eq!(engine.calls("desktop.status")[0]["deviceId"], MINI);

    let screenshot = call(
        &mut write,
        &mut read,
        4,
        "computer_use_screenshot",
        json!({"computer": "t1", "device": "mac mini"}),
    )
    .await;
    let content = screenshot["result"]["content"].as_array().unwrap();
    assert!(
        content.iter().any(|part| part["type"] == "image"),
        "{screenshot}"
    );
    for method in [
        "computers.list",
        "computers.tree",
        "guest.exec.start",
        "guest.exec.input",
        "guest.exec.get",
        "guest.exec",
    ] {
        let calls = engine.calls(method);
        assert!(!calls.is_empty(), "{method} was not sent");
        assert!(
            calls.iter().all(|params| params["deviceId"] == MINI),
            "{method}: {calls:?}"
        );
    }
    // The same computer id on this device is another computer, with a session of its own.
    let here = call(
        &mut write,
        &mut read,
        5,
        "computer_use_type",
        json!({"computer": "t1", "text": "a"}),
    )
    .await;
    assert_ne!(here["result"]["isError"], true, "{here}");
    assert_eq!(driver.lock().unwrap().started, 2);
    let local_start = engine.calls("guest.exec.start").pop().unwrap();
    assert!(local_start.get("deviceId").is_none(), "{local_start}");

    // Dropping the fork there ends its driver on that device and leaves the local one.
    let dropped = call(
        &mut write,
        &mut read,
        6,
        "drop",
        json!({"computer": "t1", "device": "mac mini"}),
    )
    .await;
    assert_ne!(dropped["result"]["isError"], true, "{dropped}");
    assert_eq!(engine.calls("computers.delete")[0]["deviceId"], MINI);
    let closes: Vec<Value> = engine
        .calls("guest.exec.input")
        .into_iter()
        .filter(|params| params["close"] == true)
        .collect();
    assert_eq!(closes.len(), 1, "{closes:?}");
    assert_eq!(closes[0]["deviceId"], MINI);
}

#[tokio::test]
async fn an_agent_may_not_drive_a_fork_on_a_device_its_settings_do_not_list() {
    let driver = Arc::new(Mutex::new(Driver::default()));
    let engine = driver_engine(driver.clone(), screenshot_result());
    let directory = project("agents:\n  bot:\n    recipes: [linux-dev]\n");
    let (mut write, mut read) = connect_mcp_in(&engine, directory.path(), Some("bot")).await;
    for (id, tool, arguments) in [
        (
            3,
            "computer_use_screenshot",
            json!({"computer": "t1", "device": "mac mini"}),
        ),
        (
            4,
            "computer_use_click",
            json!({"computer": "t1", "x": 1, "y": 2, "device": "mac mini"}),
        ),
        (
            5,
            "computer_use_tools",
            json!({"computer": "t1", "device": "mac mini"}),
        ),
        (6, "fork_gui", json!({"device": "mac mini"})),
    ] {
        let refused = call(&mut write, &mut read, id, tool, arguments).await;
        assert_eq!(refused["result"]["isError"], true, "{tool}: {refused}");
        let text = refused["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("devices:"), "{tool}: {text}");
    }
    assert!(engine.calls("guest.exec.start").is_empty());
    assert!(engine.calls("checkpoints.fork").is_empty());
}
