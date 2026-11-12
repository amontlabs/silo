mod fake_engine;

use fake_engine::{finished, listing, FakeEngine};
use rmcp::ServiceExt;
use serde_json::{json, Value};
use silo_cli::{
    cli::{run, Cli},
    mcp::SiloMcp,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

fn engine() -> FakeEngine {
    FakeEngine::start(|method, params| match method {
        "computers.list" => Ok(listing(&[("main", "running"), ("try", "running")])),
        "changes.diff" => Ok(json!({"jobId": "job:5"})),
        "operations.job.get" => Ok(finished(json!({
            "diff": {}, "text": "code: 2 files", "baseCapturedAt": "a", "capturedAt": "b",
            "baselineApplied": false, "baselineAvailable": false,
            "reviewDigest": "digest-1"
        }))),
        "operations.job.acknowledge" => Ok(Value::Null),
        "changes.request" => Ok(json!({"request": {
            "id": "fr-ab12cd34", "forkId": params["forkId"], "target": "id-main",
            "createdAt": "t", "reviewDigest": params["reviewDigest"],
            "summary": params["summary"], "status": "open"
        }})),
        other => Err(("internal", format!("unexpected {other}"))),
    })
}

fn cli(engine: &FakeEngine, args: &[&str]) -> Result<Cli, clap::Error> {
    let socket = engine.socket.to_str().unwrap();
    let mut all = vec!["silo", "--socket", socket, "--allow-other-channel"];
    all.extend_from_slice(args);
    <Cli as clap::Parser>::try_parse_from(all)
}

#[tokio::test]
async fn merge_request_records_the_digest_and_summary_and_keeps_nothing() {
    let engine = engine();
    let parsed = cli(&engine, &["merge", "try", "--request", "--into", "main"]).unwrap();
    assert_eq!(
        run(parsed, std::env::temp_dir().as_path()).await.unwrap(),
        0
    );
    let sent = &engine.calls("changes.request")[0];
    assert_eq!(sent["forkId"], "id-try");
    assert_eq!(sent["targetId"], "id-main");
    assert_eq!(sent["reviewDigest"], "digest-1");
    assert_eq!(sent["summary"], "code: 2 files");
    // The fork was looked at, never kept.
    assert_eq!(engine.calls("changes.diff").len(), 1);
    assert!(engine.calls("changes.keep").is_empty());
    assert!(engine.calls("changes.promote").is_empty());
}

#[tokio::test]
async fn fold_is_still_accepted_and_does_what_merge_does() {
    let engine = engine();
    let parsed = cli(&engine, &["fold", "try", "--request"]).unwrap();
    assert_eq!(
        run(parsed, std::env::temp_dir().as_path()).await.unwrap(),
        0
    );
    assert_eq!(engine.calls("changes.request")[0]["forkId"], "id-try");
    assert!(engine.calls("changes.keep").is_empty());
}

#[tokio::test]
async fn without_into_the_engine_picks_the_forks_source() {
    let engine = engine();
    let parsed = cli(&engine, &["merge", "try", "--request", "--json"]).unwrap();
    assert_eq!(
        run(parsed, std::env::temp_dir().as_path()).await.unwrap(),
        0
    );
    assert!(engine.calls("changes.request")[0].get("targetId").is_none());
}

#[tokio::test]
async fn a_request_cannot_be_combined_with_promote_or_a_dry_run() {
    let engine = engine();
    assert!(cli(&engine, &["merge", "try", "--request", "--promote"]).is_err());
    assert!(cli(&engine, &["merge", "try", "--request", "--dry-run"]).is_err());
}

#[tokio::test]
async fn fork_takes_a_count_a_prefix_and_a_command_after_the_separator() {
    let engine = engine();
    let parsed = cli(
        &engine,
        &[
            "fork", "main", "-n", "3", "--name", "try", "--", "make", "test", "-j4",
        ],
    )
    .unwrap();
    let silo_cli::cli::Command::Fork(fork) = parsed.command else {
        panic!("not a fork")
    };
    assert_eq!(fork.count, Some(3));
    assert_eq!(fork.name.as_deref(), Some("try"));
    assert_eq!(fork.command, ["make", "test", "-j4"]);
    let plain = cli(&engine, &["fork", "main"]).unwrap();
    let silo_cli::cli::Command::Fork(fork) = plain.command else {
        panic!("not a fork")
    };
    assert_eq!((fork.count, fork.command.len()), (None, 0));
    let folder = cli(&engine, &["fork", "./repo"]).unwrap();
    let silo_cli::cli::Command::Fork(fork) = folder.command else {
        panic!("not a fork")
    };
    assert_eq!(fork.source, "./repo");
    assert!(cli(&engine, &["fork", "x", "--recipe", "linux-dev", "-n", "2"]).is_err());
}

#[tokio::test]
async fn the_fork_command_passes_its_exit_code_through() {
    let names = std::sync::Arc::new(std::sync::Mutex::new(vec![
        "main".to_owned(),
        "job-a".to_owned(),
    ]));
    let engine = FakeEngine::start(move |method, params| match method {
        "computers.list" => {
            let names = names.lock().unwrap().clone();
            let pairs: Vec<(&str, &str)> = names.iter().map(|n| (n.as_str(), "running")).collect();
            Ok(listing(&pairs))
        }
        "checkpoints.fork" => {
            names
                .lock()
                .unwrap()
                .push(params["newName"].as_str().unwrap().to_owned());
            Ok(json!({"jobId": "job:1"}))
        }
        "computers.start" => Ok(json!({"jobId": "job:1"})),
        "network.urls" => Ok(json!({"computerId": "x", "urls": []})),
        "guest.exec.start" => Ok(json!({"jobId": "job:7"})),
        "guest.exec.get" => Ok(json!({
            "state": "exited", "exitCode": null, "firstRetainedSeq": 1,
            "output": [{"seq": 1, "stream": "stdout", "data": b"hi\n".to_vec()}]
        })),
        "operations.job.get" if params["jobId"] == "job:7" => {
            Ok(finished(json!({"exitCode": 3, "signal": null})))
        }
        "operations.job.get" => Ok(finished(Value::Null)),
        "operations.job.acknowledge" => Ok(Value::Null),
        "guest.exec" => Ok(json!({"exitCode": 1, "signal": null, "stdout": [], "stderr": []})),
        "checkpoints.create" => Ok(json!({"jobId": "job:1"})),
        other => Err(("internal", format!("unexpected {other}"))),
    });
    let parsed = cli(
        &engine,
        &["fork", "main", "--name", "job-a", "--json", "--", "false"],
    )
    .unwrap();
    // The fork already exists in this fake, so the name is refused before anything is made.
    let error = run(parsed, std::env::temp_dir().as_path())
        .await
        .unwrap_err();
    assert!(
        error.message.contains("already exists"),
        "{}",
        error.message
    );
    assert!(engine.calls("checkpoints.fork").is_empty());

    let parsed = cli(
        &engine,
        &["fork", "main", "--name", "job-b", "--json", "--", "false"],
    )
    .unwrap();
    assert_eq!(
        run(parsed, std::env::temp_dir().as_path()).await.unwrap(),
        3
    );
    assert_eq!(
        engine.calls("guest.exec.start")[0]["argv"],
        json!(["false"])
    );
}

#[tokio::test]
async fn mcp_merge_takes_request_and_fork_takes_count_and_command() {
    let engine = engine();
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
        let request = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        format!("{request}\n")
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
    line.clear();
    write
        .write_all(ask(2, "tools/list", json!({})).as_bytes())
        .await
        .unwrap();
    read.read_line(&mut line).await.unwrap();
    let tools: Value = serde_json::from_str(&line).unwrap();
    let tools = tools["result"]["tools"].as_array().unwrap().clone();
    let property = |tool: &str, name: &str| {
        tools.iter().find(|t| t["name"] == tool).unwrap()["inputSchema"]["properties"][name]
            .is_object()
    };
    assert!(property("merge", "request") && property("merge", "choices"));
    assert!(property("fork", "count") && property("fork", "command"));

    line.clear();
    write
        .write_all(
            ask(
                3,
                "tools/call",
                json!({"name": "merge", "arguments": {"fork": "try", "request": true}}),
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    read.read_line(&mut line).await.unwrap();
    let reply: Value = serde_json::from_str(&line).unwrap();
    assert_ne!(reply["result"]["isError"], true, "{reply}");
    let text = reply["result"]["content"][0]["text"].as_str().unwrap();
    let value: Value = serde_json::from_str(text).unwrap();
    assert_eq!(value["request"]["id"], "fr-ab12cd34");
    assert_eq!(value["request"]["status"], "open");
    assert!(engine.calls("changes.keep").is_empty());
}
