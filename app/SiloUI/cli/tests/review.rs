mod fake_engine;

use std::sync::{Arc, Mutex};

use fake_engine::{listing, FakeEngine};
use rmcp::ServiceExt;
use serde_json::{json, Value};
use silo_cli::{
    cli::{run, Cli},
    mcp::SiloMcp,
    render,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

fn request(status: &str) -> Value {
    json!({
        "id": "fr-ab12cd34", "forkId": "id-try", "target": "id-main", "createdAt": "t",
        "reviewDigest": "digest-1", "summary": "code: 2 files", "status": status,
        "counts": {"code": 2, "packages": 0, "settings": 1, "database": 0, "other": 0}
    })
}

/// An engine whose `changes.requests.*` jobs finish with `results[method]`.
fn engine(results: Vec<(&'static str, Value)>) -> FakeEngine {
    let started: Arc<Mutex<String>> = Arc::default();
    FakeEngine::start(move |method, _params| match method {
        "computers.list" => Ok(listing(&[("main", "running"), ("try", "running")])),
        "changes.requests.list" => Ok(json!({"requests": [
            {"request": request("open"), "forkName": "try", "forkState": "running",
             "targetName": "main"}
        ]})),
        name if name.starts_with("changes.requests.") => {
            *started.lock().unwrap() = name.to_owned();
            Ok(json!({"jobId": "job:7"}))
        }
        "operations.job.get" => {
            let name = started.lock().unwrap().clone();
            let result = results
                .iter()
                .find(|(method, _)| *method == name)
                .map(|(_, value)| value.clone())
                .unwrap_or(Value::Null);
            Ok(json!({"jobId": "job:7", "state": "finished", "outcome": {"result": result}}))
        }
        "operations.job.acknowledge" => Ok(Value::Null),
        other => Err(("internal", format!("unexpected {other}"))),
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

#[tokio::test]
async fn requests_lists_the_open_ones_unless_asked_for_another_status() {
    let engine = engine(vec![]);
    assert_eq!(exit_code(&engine, &["requests"]).await, 0);
    assert_eq!(engine.calls("changes.requests.list")[0], json!({}));
    assert_eq!(
        exit_code(&engine, &["requests", "--status", "thrown-away", "--json"]).await,
        0
    );
    assert_eq!(
        engine.calls("changes.requests.list")[1]["status"],
        "thrown-away"
    );
    let bad = run(
        cli(&engine, &["requests", "--status", "pending"]),
        std::env::temp_dir().as_path(),
    )
    .await;
    assert!(bad.is_err());
}

#[tokio::test]
async fn requests_show_asks_the_engine_for_the_request_by_id() {
    let engine = engine(vec![]);
    assert_eq!(
        exit_code(&engine, &["requests", "show", "fr-ab12cd34"]).await,
        0
    );
    assert_eq!(
        engine.calls("changes.requests.show")[0],
        json!({"requestId": "fr-ab12cd34"})
    );
}

#[tokio::test]
async fn requests_merge_passes_the_flags_and_exits_two_when_refused() {
    let kept = json!({"status": "kept", "report": {"fork": "try", "target": "main",
        "summary": [], "steps": [], "notes": [], "checkpoint": "cp1"}, "request": request("kept")});
    let refused = json!({"status": "refused", "report": {"fork": "try", "target": "main",
        "summary": [], "steps": [], "notes": [], "checkpoint": null},
        "refusal": {"reason": "changed", "message": "try changed since it was reviewed.",
                    "conflicts": [], "hint": "Review it again."},
        "request": request("open")});
    let engine = engine(vec![("changes.requests.keep", kept)]);
    assert_eq!(
        exit_code(&engine, &["requests", "merge", "fr-ab12cd34", "--data-ok"]).await,
        0
    );
    let sent = &engine.calls("changes.requests.keep")[0];
    assert_eq!(sent["requestId"], "fr-ab12cd34");
    assert_eq!(sent["dataOk"], true);
    // A keep through a request never goes through the plain keep method.
    assert!(engine.calls("changes.keep").is_empty());
    let engine = self::engine(vec![("changes.requests.keep", refused)]);
    assert_eq!(
        exit_code(&engine, &["requests", "merge", "fr-ab12cd34"]).await,
        2
    );
}

#[tokio::test]
async fn requests_decline_and_open_call_their_methods() {
    let engine = engine(vec![
        (
            "changes.requests.throwAway",
            json!({"request": request("thrown-away"), "dropped": "try"}),
        ),
        (
            "changes.requests.open",
            json!({"request": request("open"), "computerId": "id-try", "name": "try",
                   "state": "running", "started": true, "sshAlias": "try.silo",
                   "urls": ["http://try.silo.localhost:8080"]}),
        ),
    ]);
    assert_eq!(
        exit_code(&engine, &["requests", "decline", "fr-ab12cd34"]).await,
        0
    );
    assert_eq!(
        engine.calls("changes.requests.throwAway")[0],
        json!({"requestId": "fr-ab12cd34"})
    );
    assert_eq!(
        exit_code(&engine, &["requests", "open", "fr-ab12cd34", "--json"]).await,
        0
    );
    assert_eq!(engine.calls("changes.requests.open").len(), 1);
    // Nothing is dropped by the CLI itself.
    assert!(engine.calls("computers.delete").is_empty());
}

#[test]
fn show_renders_the_buckets_then_the_credentials_line() {
    let value = json!({
        "request": request("open"), "forkName": "try", "forkState": "running",
        "stale": false, "fresh": true, "text": "code: 2 files\nsettings: 1 change",
        "credentials": {"used": [], "blocked": []}
    });
    let text = render::request_show(&value);
    assert!(text.contains("code: 2 files\nsettings: 1 change"));
    assert!(text.ends_with("Credentials used: none"), "{text}");
    assert!(!text.contains("changed since"));

    let used = json!({
        "request": request("open"), "forkName": "try", "forkState": "stopped",
        "stale": true, "fresh": false, "text": "code: 2 files",
        "credentials": {
            "used": [{"secret": "GITHUB_TOKEN", "host": "api.github.com", "method": "POST", "count": 3}],
            "blocked": [{"host": "evil.example", "method": "GET", "count": 1, "reason": "not allowed for this secret"}]
        }
    });
    let text = render::request_show(&used);
    assert!(text.contains("Credentials used: GITHUB_TOKEN to api.github.com (POST x3)"));
    assert!(text.contains("Blocked: GET evil.example x1 (not allowed for this secret)"));
    assert!(text.contains("last look"));
    assert!(text.contains("a merge refuses"));
}

#[test]
fn list_renders_each_request_with_its_fork_target_and_counts() {
    let value = json!({"requests": [
        {"request": request("open"), "forkName": "try", "forkState": "running", "targetName": "main"},
        {"request": request("open")}
    ]});
    let text = render::requests_list(&value);
    let mut lines = text.lines();
    assert_eq!(
        lines.next().unwrap(),
        "fr-ab12cd34  try (running)  into main  code 2, settings 1  [open]"
    );
    assert!(lines.next().unwrap().contains("(fork gone)"));
    let closed = render::requests_list(&json!({"requests": [{
        "request": {"id": "fr-1", "target": "m", "status": "thrown-away",
            "closedReason": "fork-deleted", "counts": {}}
    }]}));
    assert!(
        closed.ends_with("[closed: fork deleted (thrown-away)]"),
        "{closed}"
    );
    assert!(render::requests_list(&json!({"requests": []})).starts_with("No review requests"));
}

#[test]
fn decline_and_open_render_short_lines() {
    assert_eq!(
        render::request_declined(&json!({"request": request("thrown-away"), "dropped": "try"})),
        "Declined try and closed fr-ab12cd34."
    );
    let text = render::request_open(&json!({
        "name": "try", "state": "running", "started": true, "sshAlias": "try.silo",
        "urls": ["http://a.localhost"]
    }));
    assert!(text.starts_with("try is running (started now)."));
    assert!(text.contains("ssh try.silo") && text.contains("http://a.localhost"));
}

#[tokio::test]
async fn mcp_serves_the_request_tools_on_the_same_methods() {
    let engine = engine(vec![(
        "changes.requests.throwAway",
        json!({"request": request("thrown-away"), "dropped": "try"}),
    )]);
    let (server_side, client_side) = tokio::io::duplex(64 * 1024);
    let (server_read, server_write) = tokio::io::split(server_side);
    let server = SiloMcp::new(engine.socket.clone(), std::env::temp_dir());
    tokio::spawn(async move {
        let running = server.serve((server_read, server_write)).await.unwrap();
        let _ = running.waiting().await;
    });
    let (read, mut write) = tokio::io::split(client_side);
    let mut read = BufReader::new(read);
    let ask = |id: u64, method: &'static str, params: Value| {
        format!(
            "{}\n",
            json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
        )
    };
    write
        .write_all(
            ask(
                1,
                "initialize",
                json!({"protocolVersion": "2025-06-18", "capabilities": {},
                       "clientInfo": {"name": "test", "version": "0"}}),
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let mut line = String::new();
    read.read_line(&mut line).await.unwrap();
    write
        .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n")
        .await
        .unwrap();
    write
        .write_all(ask(2, "tools/list", json!({})).as_bytes())
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
        "requests_list",
        "request_show",
        "request_merge",
        "request_decline",
        "request_open",
    ] {
        assert!(
            names.contains(&expected),
            "{expected} missing from {names:?}"
        );
    }
    write
        .write_all(
            ask(
                3,
                "tools/call",
                json!({"name": "requests_list", "arguments": {"status": "all"}}),
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    line.clear();
    read.read_line(&mut line).await.unwrap();
    let listed: Value = serde_json::from_str(&line).unwrap();
    assert_ne!(listed["result"]["isError"], true, "{listed}");
    assert_eq!(engine.calls("changes.requests.list")[0]["status"], "all");
    write
        .write_all(
            ask(
                4,
                "tools/call",
                json!({"name": "request_decline", "arguments": {"id": "fr-ab12cd34"}}),
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    line.clear();
    read.read_line(&mut line).await.unwrap();
    let reply: Value = serde_json::from_str(&line).unwrap();
    let text = reply["result"]["content"][0]["text"].as_str().unwrap();
    let value: Value = serde_json::from_str(text).unwrap();
    assert_eq!(value["dropped"], "try");
    assert_eq!(
        engine.calls("changes.requests.throwAway")[0],
        json!({"requestId": "fr-ab12cd34"})
    );
}
