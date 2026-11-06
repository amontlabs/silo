//! MCP tools act on another device only when the call names it and the agent's `devices:`
//! entry in `.silo/silo.yaml` lists it.

mod fake_engine;

use fake_engine::{finished, listing, FakeEngine};
use rmcp::ServiceExt;
use serde_json::{json, Value};
use silo_cli::mcp::SiloMcp;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const MINI: &str = "mini-0000";

fn request() -> Value {
    json!({
        "id": "fr-1", "forkId": "id-try", "target": "id-main", "createdAt": "t",
        "reviewDigest": "d1", "summary": "s", "status": "thrown-away", "counts": {}
    })
}

fn engine() -> FakeEngine {
    FakeEngine::start(|method, _| match method {
        "connections.devices.list" => Ok(json!([
            {"id": MINI, "name": "Mac mini", "address": "ssh://mini"}
        ])),
        "changes.requests.throwAway" => Ok(json!({"jobId": "job:7"})),
        "operations.job.get" => Ok(json!({"jobId": "job:7", "state": "finished",
            "outcome": {"result": {"request": request(), "dropped": "try"}}})),
        "operations.job.acknowledge" => Ok(Value::Null),
        other => Err(("internal", format!("unexpected {other}"))),
    })
}

/// Calls `request_throw_away` and returns the tool's reply.
async fn throw_away(engine: &FakeEngine, directory: &std::path::Path, arguments: Value) -> Value {
    call_tool(engine, directory, "request_throw_away", arguments).await
}

/// Calls the tool `name` as the agent `bot` and returns the tool's reply.
async fn call_tool(
    engine: &FakeEngine,
    directory: &std::path::Path,
    name: &str,
    arguments: Value,
) -> Value {
    let (server_side, client_side) = tokio::io::duplex(64 * 1024);
    let (server_read, server_write) = tokio::io::split(server_side);
    let server = SiloMcp::new(engine.socket.clone(), directory.to_owned())
        .with_agent(Some("bot".to_owned()));
    tokio::spawn(async move {
        let running = server.serve((server_read, server_write)).await.unwrap();
        let _ = running.waiting().await;
    });
    let (read, mut write) = tokio::io::split(client_side);
    let mut read = BufReader::new(read);
    let ask = |id: u64, method: &str, params: Value| {
        format!(
            "{}\n",
            json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
        )
    };
    let init = json!({"protocolVersion": "2025-06-18", "capabilities": {},
                      "clientInfo": {"name": "test", "version": "0"}});
    write
        .write_all(ask(1, "initialize", init).as_bytes())
        .await
        .unwrap();
    let mut line = String::new();
    read.read_line(&mut line).await.unwrap();
    write
        .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n")
        .await
        .unwrap();
    let call = json!({"name": name, "arguments": arguments});
    write
        .write_all(ask(2, "tools/call", call).as_bytes())
        .await
        .unwrap();
    line.clear();
    read.read_line(&mut line).await.unwrap();
    serde_json::from_str(&line).unwrap()
}

fn project(settings: &str) -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join(".silo")).unwrap();
    std::fs::write(directory.path().join(".silo/silo.yaml"), settings).unwrap();
    directory
}

#[tokio::test]
async fn an_agent_may_not_act_on_a_device_its_settings_do_not_list() {
    let engine = engine();
    let directory = project("recipes: [linux-dev]\n");
    let reply = throw_away(
        &engine,
        directory.path(),
        json!({"id": "fr-1", "device": "mac mini"}),
    )
    .await;
    let text = reply["result"]["content"][0]["text"].as_str().unwrap();
    assert_eq!(reply["result"]["isError"], true, "{reply}");
    assert!(text.contains("devices:"), "{text}");
    assert!(engine.calls("changes.requests.throwAway").is_empty());
}

#[tokio::test]
async fn a_listed_device_is_acted_on_only_when_the_call_names_it() {
    let engine = engine();
    let directory = project("agents:\n  bot:\n    devices: [Mac mini]\n");
    let reply = throw_away(
        &engine,
        directory.path(),
        json!({"id": "fr-1", "device": "mac mini"}),
    )
    .await;
    assert_ne!(reply["result"]["isError"], true, "{reply}");
    assert_eq!(
        engine.calls("changes.requests.throwAway")[0]["deviceId"],
        MINI
    );
    // Without `device` the request is looked for on this device only.
    throw_away(&engine, directory.path(), json!({"id": "fr-2"})).await;
    assert!(engine.calls("changes.requests.throwAway")[1]
        .get("deviceId")
        .is_none());
    assert!(engine.calls("changes.requests.list").is_empty());
}

/// An engine connected to the Mac mini, where `main` lives and `t1` appears once forked.
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
            "operations.job.get" => Ok(finished(json!({"exitCode": 0}))),
            "operations.job.acknowledge" => Ok(Value::Null),
            other => Err(("internal", format!("unexpected {other}"))),
        }
    })
}

const ALLOWED: &str = "agents:\n  bot:\n    devices: [Mac mini]\n";

#[tokio::test]
async fn exec_runs_on_a_listed_device() {
    let engine = host_engine();
    let directory = project(ALLOWED);
    let reply = call_tool(
        &engine,
        directory.path(),
        "exec",
        json!({"computer": "main", "command": ["ls"], "device": "mac mini"}),
    )
    .await;
    assert_ne!(reply["result"]["isError"], true, "{reply}");
    let text = reply["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("hi"), "{text}");
    let ran = engine.calls("guest.exec");
    assert_eq!(ran[0]["deviceId"], MINI);
    assert_eq!(ran[0]["argv"], json!(["ls"]));
}

#[tokio::test]
async fn exec_and_fork_with_a_command_are_refused_on_a_device_the_agent_may_not_act_on() {
    let engine = host_engine();
    let directory = project("recipes: [linux-dev]\n");
    for (tool, arguments) in [
        (
            "exec",
            json!({"computer": "main", "command": ["ls"], "device": "mac mini"}),
        ),
        (
            "fork",
            json!({"name": "t1", "command": ["make"], "device": "mac mini"}),
        ),
    ] {
        let reply = call_tool(&engine, directory.path(), tool, arguments).await;
        assert_eq!(reply["result"]["isError"], true, "{tool}: {reply}");
        let text = reply["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("devices:"), "{tool}: {text}");
    }
    assert!(engine.calls("guest.exec").is_empty());
    assert!(engine.calls("checkpoints.fork").is_empty());
}

#[tokio::test]
async fn fork_with_a_command_runs_it_on_a_listed_device() {
    let engine = host_engine();
    let directory = project(ALLOWED);
    let reply = call_tool(
        &engine,
        directory.path(),
        "fork",
        json!({"name": "t1", "command": ["make", "test"], "device": "mac mini"}),
    )
    .await;
    assert_ne!(reply["result"]["isError"], true, "{reply}");
    let text = reply["result"]["content"][0]["text"].as_str().unwrap();
    let value: Value = serde_json::from_str(text).unwrap();
    assert_eq!(value["forks"][0]["exitCode"], 0, "{value}");
    assert_eq!(value["forks"][0]["stdout"], "hi\n");
    for method in [
        "checkpoints.fork",
        "computers.start",
        "guest.exec.start",
        "guest.exec.get",
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
