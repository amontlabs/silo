//! MCP tools act on another device only when the call names it and the agent's `devices:`
//! entry in `.silo/silo.yaml` lists it.

mod fake_engine;

use fake_engine::FakeEngine;
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
    let call = json!({"name": "request_throw_away", "arguments": arguments});
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
