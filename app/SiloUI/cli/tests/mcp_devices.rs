//! MCP tools read from another device when the call names it. None changes anything there: only
//! the owner, with `silo --device`, acts on a connected device.

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
        "computers.list" => Ok(listing(&[("main", "running")])),
        "changes.requests.show" => Ok(json!({"jobId": "job:7"})),
        "operations.job.get" => Ok(finished(json!({"request": request(), "forkName": "try"}))),
        "operations.job.acknowledge" => Ok(Value::Null),
        other => Err(("internal", format!("unexpected {other}"))),
    })
}

/// Calls the tool `name` and returns the tool's reply.
async fn call_tool(
    engine: &FakeEngine,
    directory: &std::path::Path,
    name: &str,
    arguments: Value,
) -> Value {
    let (server_side, client_side) = tokio::io::duplex(64 * 1024);
    let (server_read, server_write) = tokio::io::split(server_side);
    let server = SiloMcp::new(engine.socket.clone(), directory.to_owned());
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

#[tokio::test]
async fn a_read_names_the_device_and_goes_there() {
    let engine = engine();
    let directory = std::env::temp_dir();
    let reply = call_tool(&engine, &directory, "list", json!({"device": "mac mini"})).await;
    assert_ne!(reply["result"]["isError"], true, "{reply}");
    assert_eq!(engine.calls("computers.list")[0]["deviceId"], MINI);
    let shown = call_tool(
        &engine,
        &directory,
        "request_show",
        json!({"id": "fr-1", "device": "mac mini"}),
    )
    .await;
    assert_ne!(shown["result"]["isError"], true, "{shown}");
    assert_eq!(engine.calls("changes.requests.show")[0]["deviceId"], MINI);
}

#[tokio::test]
async fn nothing_that_changes_a_computer_takes_a_device() {
    let engine = engine();
    let directory = std::env::temp_dir();
    for (tool, arguments) in [
        (
            "exec",
            json!({"computer": "main", "command": ["ls"], "device": "mac mini"}),
        ),
        (
            "fork",
            json!({"name": "t1", "command": ["make"], "device": "mac mini"}),
        ),
        ("merge", json!({"fork": "t1", "device": "mac mini"})),
        ("drop", json!({"computer": "t1", "device": "mac mini"})),
        (
            "request_decline",
            json!({"id": "fr-1", "device": "mac mini"}),
        ),
    ] {
        let reply = call_tool(&engine, &directory, tool, arguments).await;
        assert!(
            reply["error"].is_object() || reply["result"]["isError"] == true,
            "{tool}: {reply}"
        );
    }
    for method in [
        "guest.exec",
        "checkpoints.fork",
        "changes.keep",
        "computers.delete",
        "changes.requests.throwAway",
    ] {
        assert!(engine.calls(method).is_empty(), "{method} was sent");
    }
}
