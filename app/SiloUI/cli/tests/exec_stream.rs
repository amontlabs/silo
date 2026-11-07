mod fake_engine;

use std::{sync::Mutex, time::Duration};

use fake_engine::{finished, listing, FakeEngine};
use serde_json::json;
use silo_cli::{engine::Engine, fanout};

fn ignore(_: &str) {}

fn engine() -> FakeEngine {
    FakeEngine::start(|method, _| match method {
        "computers.list" => Ok(listing(&[("ci", "running")])),
        "guest.exec.start" => Ok(json!({"jobId": "job:7"})),
        "guest.exec.get" => Ok(json!({
            "state": "exited", "exitCode": 0, "firstRetainedSeq": 1,
            "output": [{"seq": 1, "stream": "stdout", "data": b"Listening for Jobs\n".to_vec()}],
        })),
        "operations.job.get" => Ok(finished(json!({"exitCode": 0, "signal": null}))),
        "operations.job.acknowledge" => Ok(serde_json::Value::Null),
        other => Err(("internal", format!("unexpected {other}"))),
    })
}

#[tokio::test]
async fn streamed_input_travels_in_the_request_and_never_in_argv_or_env() {
    let engine = engine();
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let computer = connected.computer("ci").await.unwrap();
    let secret = b"eyJzZWNyZXQiOiJ2YWx1ZSJ9".to_vec();
    let argv: Vec<String> = ["sh", "-c", "read -r JIT; exec ./run.sh"]
        .into_iter()
        .map(String::from)
        .collect();
    let seen = Mutex::new(Vec::new());
    let ran = tokio::time::timeout(
        Duration::from_secs(20),
        fanout::exec_streaming_with_input(
            &connected,
            &computer,
            &argv,
            Some("/home/runner"),
            Some(secret.clone()),
            &ignore,
            |is_err, bytes| seen.lock().unwrap().push((is_err, bytes.to_vec())),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(ran.exit_code, 0);
    assert_eq!(
        seen.lock().unwrap().as_slice(),
        [(false, b"Listening for Jobs\n".to_vec())]
    );
    let sent = &engine.calls("guest.exec.start")[0];
    assert_eq!(sent["stdin"], json!(secret));
    assert_eq!(sent["stdinOpen"], false);
    assert_eq!(sent["argv"], json!(argv));
    assert!(sent.get("env").is_none() || sent["env"].is_null());
    assert!(!sent["argv"].to_string().contains("eyJz"));
    // A streamed command is not recorded as a step, so nothing is checkpointed.
    assert!(engine.calls("steps.record").is_empty());
    assert!(engine.calls("checkpoints.create").is_empty());
}

#[tokio::test]
async fn a_streamed_command_without_input_sends_none() {
    let engine = engine();
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let computer = connected.computer("ci").await.unwrap();
    fanout::exec_streaming(
        &connected,
        &computer,
        &["true".to_owned()],
        None,
        &ignore,
        |_, _| {},
    )
    .await
    .unwrap();
    let sent = &engine.calls("guest.exec.start")[0];
    assert!(sent.get("stdin").is_none() || sent["stdin"].is_null());
}
