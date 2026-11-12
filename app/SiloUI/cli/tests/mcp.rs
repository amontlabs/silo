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
        "computers.access" => Ok(json!({
            "computerId": "id-main", "name": "main",
            "secrets": [{"name": "API_KEY", "hosts": ["api.example.com"]}],
            "ports": []
        })),
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
        "merge",
        "replace_main",
        "undo",
        "steps_list",
        "tree",
        "resources",
        "access",
        "create",
        "restore",
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
    assert_eq!(listed["computers"][0]["computer"], "main");
    assert_eq!(listed["computers"][0]["secrets"][0]["name"], "API_KEY");
    assert_eq!(
        listed["computers"][0]["secrets"][0]["hosts"][0],
        "api.example.com"
    );
    assert!(
        engine.calls("secrets.list").is_empty(),
        "names come from what a computer may read"
    );

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

    let stale = exchange(
        &mut write,
        &mut read,
        json!({"jsonrpc": "2.0", "id": 6, "method": "tools/call",
               "params": {"name": "merge", "arguments": {"fork": "f", "pr": true}}}),
    )
    .await;
    assert_eq!(stale["result"]["isError"], true, "{stale}");
    assert!(stale["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("pr"));
}

#[tokio::test]
async fn the_server_explains_the_model_to_an_agent_that_has_never_seen_silo() {
    let engine = FakeEngine::start(|_, _| Ok(Value::Null));
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
    let instructions = initialized["result"]["instructions"].as_str().unwrap();
    for word in ["fork", "merge", "person", "resources", "undo"] {
        assert!(instructions.contains(word), "{word}: {instructions}");
    }
    assert!(!instructions.contains("fold") && !instructions.contains("promote"));
}

#[tokio::test]
async fn a_tool_that_needs_the_owner_waits_then_reports_the_request_to_check_later() {
    let engine = FakeEngine::start(|method, _| {
        match method {
        "computers.list" => Ok(listing(&[("main", "running"), ("try", "running")])),
        "approvals.note" => Ok(Value::Null),
        "approvals.get" => Ok(json!({"id": "ap-1", "state": "pending", "summary": "Merge",
            "computerName": "main", "method": "changes.keep", "params": {}, "createdAt": "now"})),
        "changes.keep" => Err((
            "approval_required",
            r#"{"message": "The owner has to answer.", "details": {"kind": "merge", "summary": "Merges try into main.", "approvalId": "ap-1"}}"#.to_owned(),
        )),
        other => Err(("internal", format!("unexpected {other}"))),
    }
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
    exchange(
        &mut write,
        &mut read,
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "test", "version": "0"}}}),
    )
    .await;
    write
        .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n")
        .await
        .unwrap();
    let waiting = exchange(
        &mut write,
        &mut read,
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
               "params": {"name": "merge", "arguments": {
                   "fork": "try", "wait_secs": 0, "reason": "the tests pass"}}}),
    )
    .await;
    assert_ne!(waiting["result"]["isError"], true, "{waiting}");
    let text = waiting["result"]["content"][0]["text"].as_str().unwrap();
    let value: Value = serde_json::from_str(text).unwrap();
    assert_eq!(value["status"], "waiting-for-owner");
    assert_eq!(value["approvalId"], "ap-1");
    assert!(value["message"].as_str().unwrap().contains("ap-1"));
    // The reason went to the request right after asking.
    assert_eq!(engine.calls("approvals.note")[0]["note"], "the tests pass");
}
