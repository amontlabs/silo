mod fake_engine;

use fake_engine::{listing, FakeEngine};
use rmcp::ServiceExt;
use serde_json::{json, Value};
use silo_cli::mcp::SiloMcp;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

async fn exchange(
    writer: &mut (impl AsyncWriteExt + Unpin),
    reader: &mut (impl AsyncBufReadExt + Unpin),
    request: Value,
) -> Value {
    writer
        .write_all(format!("{request}\n").as_bytes())
        .await
        .unwrap();
    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    serde_json::from_str(&line).unwrap()
}

#[tokio::test]
async fn initialize_list_and_call_round_trip() {
    let engine = FakeEngine::start(|method, _| match method {
        "computers.list" => Ok(listing(&[("main", "running")])),
        "secrets.list" => Ok(json!([{
            "id": "s1", "name": "API_KEY", "computerIds": ["id-main"],
            "allowedDomains": ["api.example.com"], "state": "active",
            "pendingComputerIds": [], "removing": false, "error": null
        }])),
        other => Err(("internal", format!("unexpected {other}"))),
    });
    let (server_side, client_side) = tokio::io::duplex(64 * 1024);
    let (server_read, server_write) = tokio::io::split(server_side);
    let server = SiloMcp::new(engine.socket.clone(), std::env::temp_dir());
    tokio::spawn(async move {
        let running = server.serve((server_read, server_write)).await.unwrap();
        let _ = running.waiting().await;
    });
    let (read, mut write) = tokio::io::split(client_side);
    let mut read = BufReader::new(read);

    let initialized = exchange(
        &mut write,
        &mut read,
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "test", "version": "0"}}}),
    )
    .await;
    assert!(
        initialized["result"]["capabilities"]["tools"].is_object(),
        "{initialized}"
    );
    write
        .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n")
        .await
        .unwrap();

    let tools = exchange(
        &mut write,
        &mut read,
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
    )
    .await;
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    for expected in [
        "list",
        "status",
        "fork",
        "drop",
        "diff",
        "exec",
        "checkpoint",
        "retry_setup",
        "fold",
        "undo",
        "steps_list",
        "tree",
    ] {
        assert!(
            names.contains(&expected),
            "{expected} missing from {names:?}"
        );
    }
    assert!(names.contains(&"secrets_list"));
    assert!(
        !names
            .iter()
            .any(|name| *name != "retry_setup" && (name.contains("set") || name.contains("assign"))),
        "no tool changes secrets: {names:?}"
    );
    let fork = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "fork")
        .unwrap();
    assert!(fork["inputSchema"]["properties"]["source"].is_object());
    assert!(fork["inputSchema"]["properties"]["fromStep"].is_object());

    let called = exchange(
        &mut write,
        &mut read,
        json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
               "params": {"name": "list", "arguments": {}}}),
    )
    .await;
    let text = called["result"]["content"][0]["text"].as_str().unwrap();
    let listed: Value = serde_json::from_str(text).unwrap();
    assert_eq!(listed["computers"][0]["name"], "main");

    let secrets = exchange(
        &mut write,
        &mut read,
        json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call",
               "params": {"name": "secrets_list", "arguments": {}}}),
    )
    .await;
    let text = secrets["result"]["content"][0]["text"].as_str().unwrap();
    let listed: Value = serde_json::from_str(text).unwrap();
    assert_eq!(listed["secrets"][0]["name"], "API_KEY");
    assert_eq!(listed["secrets"][0]["computers"][0], "main");

    for id in 10..14 {
        exchange(
            &mut write,
            &mut read,
            json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
                   "params": {"name": "list", "arguments": {}}}),
        )
        .await;
    }
    assert_eq!(engine.accepted(), 1, "the server opens one connection");

    let failed = exchange(
        &mut write,
        &mut read,
        json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call",
               "params": {"name": "drop", "arguments": {"computer": "main"}}}),
    )
    .await;
    assert_eq!(failed["result"]["isError"], true);
    assert!(failed["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("--force"));
}

async fn agent_after_initialize(server: SiloMcp, client: &str) -> Option<String> {
    let (server_side, client_side) = tokio::io::duplex(64 * 1024);
    let (server_read, server_write) = tokio::io::split(server_side);
    let held = server.clone();
    tokio::spawn(async move {
        let running = server.serve((server_read, server_write)).await.unwrap();
        let _ = running.waiting().await;
    });
    let (read, mut write) = tokio::io::split(client_side);
    let mut read = BufReader::new(read);
    exchange(
        &mut write,
        &mut read,
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": client, "version": "0"}}}),
    )
    .await;
    held.agent()
}

#[tokio::test]
async fn the_client_name_is_the_agent_only_when_none_was_named() {
    let engine = FakeEngine::start(|_, _| Ok(Value::Null));
    // `new` reads SILO_AGENT, so the first case only holds when it is not set.
    if std::env::var("SILO_AGENT").is_err() {
        let unnamed = SiloMcp::new(engine.socket.clone(), std::env::temp_dir());
        assert_eq!(
            agent_after_initialize(unnamed, "Claude Code")
                .await
                .as_deref(),
            Some("claude-code")
        );
    }
    let named = SiloMcp::new(engine.socket.clone(), std::env::temp_dir())
        .with_agent(Some("Careful".into()));
    assert_eq!(
        agent_after_initialize(named, "Claude Code")
            .await
            .as_deref(),
        Some("careful")
    );
}
