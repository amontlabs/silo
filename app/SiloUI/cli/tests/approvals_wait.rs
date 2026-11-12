//! A command run inside a computer that gets `approval_required` says what was asked, waits
//! for the owner's answer and ends with an exit code that says how it went.

mod fake_engine;

use std::sync::{Arc, Mutex};

use fake_engine::{finished, listing, FakeEngine};
use serde_json::{json, Value};
use silo_cli::{
    cli::{run, Cli},
    error::exit,
};

const ASKED: &str = r#"{"message": "The owner has to answer.", "details": {"kind": "merge", "summary": "Merges try into main.", "approvalId": "ap-1"}}"#;

fn approval(state: &str, extra: Value) -> Value {
    let mut base = json!({
        "id": "ap-1", "kind": "merge", "computerId": "id-main", "computerName": "main",
        "method": "changes.keep", "params": {}, "summary": "Merges try into main.",
        "state": state, "createdAt": "2026-11-11T10:00:00Z", "expiresAt": "2026-11-18T10:00:00Z"
    });
    for (key, value) in extra.as_object().unwrap() {
        base[key] = value.clone();
    }
    base
}

/// An engine that asks for approval on `ask` and answers `approvals.get` with the successive
/// approvals in `answers` (the last one repeats).
fn engine(ask: &'static str, answers: Vec<Value>, job_result: Value) -> FakeEngine {
    let seen = Arc::new(Mutex::new(0usize));
    FakeEngine::start(move |method, params| match method {
        "computers.list" => Ok(listing(&[("main", "running"), ("try", "running")])),
        m if m == ask => Err(("approval_required", ASKED.to_owned())),
        "approvals.get" => {
            let mut n = seen.lock().unwrap();
            let answer = answers[(*n).min(answers.len() - 1)].clone();
            *n += 1;
            Ok(answer)
        }
        "approvals.note" => Ok(approval("pending", json!({"note": params["note"]}))),
        "operations.job.get" if params["jobId"] == "job:9" => Ok(finished(job_result.clone())),
        "operations.job.acknowledge" => Ok(Value::Null),
        "network.urls" => Ok(json!({"computerId": "id-main", "urls": []})),
        "computers.access" => Ok(json!({
            "computerId": "id-main", "name": "main", "secrets": [],
            "ports": [{"port": 8080, "configured": true, "state": "reachable", "scheme": "http"}]
        })),
        other => Err(("internal", format!("unexpected {other}"))),
    })
}

fn cli(engine: &FakeEngine, args: &[&str]) -> Cli {
    let socket = engine.socket.to_str().unwrap();
    let mut all = vec!["silo", "--socket", socket, "--allow-other-channel"];
    all.extend_from_slice(args);
    <Cli as clap::Parser>::try_parse_from(all).unwrap()
}

fn kept() -> Value {
    json!({"status": "kept", "report": {"fork": "try", "target": "main", "summary": [],
        "steps": [], "notes": [], "checkpoint": "cp1"}})
}

async fn exit_code(engine: &FakeEngine, args: &[&str]) -> Result<i32, silo_cli::error::CliError> {
    run(cli(engine, args), std::env::temp_dir().as_path()).await
}

#[tokio::test]
async fn an_allowed_job_is_followed_as_the_command_would_have_been() {
    let allowed = approval("allowed", json!({"outcome": {"jobId": "job:9"}}));
    let engine = engine(
        "changes.keep",
        vec![approval("pending", json!({})), allowed],
        kept(),
    );
    let code = exit_code(
        &engine,
        &["--reason", "the tests pass", "merge", "try", "--json"],
    )
    .await
    .unwrap();
    assert_eq!(code, 0);
    // The call was made once; the engine ran it on allow, and the command followed its job.
    assert_eq!(engine.calls("changes.keep").len(), 1);
    assert!(engine.calls("approvals.get").len() >= 2);
    assert_eq!(engine.calls("approvals.get")[0]["id"], "ap-1");
    assert_eq!(engine.calls("approvals.note")[0]["note"], "the tests pass");
    assert!(engine
        .calls("operations.job.get")
        .iter()
        .any(|call| call["jobId"] == "job:9"));
}

#[tokio::test]
async fn an_allowed_command_returns_the_result_the_engine_stored() {
    let allowed = approval(
        "allowed",
        json!({"method": "network.publish", "outcome": {"result": {"computers": []}}}),
    );
    let engine = engine("network.publish", vec![allowed], Value::Null);
    let code = exit_code(&engine, &["publish", "main", "8080"])
        .await
        .unwrap();
    assert_eq!(code, 0);
    assert_eq!(engine.calls("network.publish").len(), 1);
    // The command then shows the ports as usual.
    assert!(!engine.calls("computers.access").is_empty());
}

#[tokio::test]
async fn a_denied_request_prints_the_owners_note_and_exits_with_four() {
    let denied = approval("denied", json!({"decisionNote": "not on a Friday"}));
    let engine = engine("changes.keep", vec![denied], Value::Null);
    let error = exit_code(&engine, &["merge", "try"]).await.unwrap_err();
    assert_eq!(error.exit, exit::DENIED);
    assert_eq!(error.exit, 4);
    assert!(
        error.message.contains("not on a Friday"),
        "{}",
        error.message
    );
    assert_eq!(error.approval_id.as_deref(), Some("ap-1"));
}

#[tokio::test]
async fn a_request_still_waiting_exits_with_five_and_names_how_to_check_later() {
    let pending = approval("pending", json!({}));
    let engine = engine("changes.keep", vec![pending.clone()], Value::Null);
    let error = exit_code(&engine, &["--wait", "1s", "merge", "try"])
        .await
        .unwrap_err();
    assert_eq!(error.exit, exit::WAITING);
    assert_eq!(error.exit, 5);
    assert!(
        error.message.contains("silo approvals status ap-1"),
        "{}",
        error.message
    );
    assert!(
        error.message.contains("Merges try into main"),
        "{}",
        error.message
    );
    // --no-wait does not even look again.
    let engine = self::engine("changes.keep", vec![pending], Value::Null);
    let error = exit_code(&engine, &["--no-wait", "merge", "try"])
        .await
        .unwrap_err();
    assert_eq!(error.exit, 5);
    assert!(engine.calls("approvals.get").is_empty());
}

#[tokio::test]
async fn a_withdrawn_or_expired_request_exits_with_six() {
    for state in ["cancelled", "expired"] {
        let ended = approval(state, json!({"reason": "Nobody answered in seven days."}));
        let engine = engine("changes.keep", vec![ended], Value::Null);
        let error = exit_code(&engine, &["merge", "try"]).await.unwrap_err();
        assert_eq!(error.exit, exit::ENDED, "{state}");
        assert!(
            error.message.contains("Nobody answered"),
            "{}",
            error.message
        );
    }
}

#[tokio::test]
async fn an_allowed_call_that_could_not_run_says_so() {
    let failed = approval(
        "allowed",
        json!({"outcome": {"error": "try no longer exists"}}),
    );
    let engine = engine("changes.keep", vec![failed], Value::Null);
    let error = exit_code(&engine, &["merge", "try"]).await.unwrap_err();
    assert!(
        error.message.contains("try no longer exists"),
        "{}",
        error.message
    );
    assert_eq!(error.exit, 1);
}

#[tokio::test]
async fn approvals_status_and_wait_let_an_agent_come_back_later() {
    let pending = approval("pending", json!({}));
    let engine = engine("never", vec![pending], Value::Null);
    assert_eq!(
        exit_code(&engine, &["approvals", "status", "ap-1"])
            .await
            .unwrap(),
        exit::WAITING
    );
    let denied = approval("denied", json!({"decisionNote": "no"}));
    let engine = self::engine("never", vec![denied], Value::Null);
    assert_eq!(
        exit_code(&engine, &["approvals", "status", "ap-1"])
            .await
            .unwrap(),
        exit::DENIED
    );
    let allowed = approval("allowed", json!({"outcome": {"jobId": "job:9"}}));
    let engine = self::engine(
        "never",
        vec![approval("pending", json!({})), allowed],
        kept(),
    );
    assert_eq!(
        exit_code(&engine, &["approvals", "wait", "ap-1"])
            .await
            .unwrap(),
        0
    );
    assert!(engine
        .calls("operations.job.get")
        .iter()
        .any(|call| call["jobId"] == "job:9"));
    // A wait that ends with no answer reports where the request stands.
    let engine = self::engine("never", vec![approval("pending", json!({}))], Value::Null);
    assert_eq!(
        exit_code(&engine, &["--wait", "1s", "approvals", "wait", "ap-1"])
            .await
            .unwrap(),
        exit::WAITING
    );
}

#[tokio::test]
async fn approvals_note_adds_the_agents_reason() {
    let engine = engine("never", vec![approval("pending", json!({}))], Value::Null);
    assert_eq!(
        exit_code(
            &engine,
            &["approvals", "note", "ap-1", "needed for the demo"]
        )
        .await
        .unwrap(),
        0
    );
    assert_eq!(
        engine.calls("approvals.note")[0],
        json!({"id": "ap-1", "note": "needed for the demo"})
    );
}

#[test]
fn durations_read_seconds_minutes_and_hours() {
    use silo_cli::approvals::parse_duration;
    use std::time::Duration;
    assert_eq!(parse_duration("90").unwrap(), Duration::from_secs(90));
    assert_eq!(parse_duration("90s").unwrap(), Duration::from_secs(90));
    assert_eq!(parse_duration("2m").unwrap(), Duration::from_secs(120));
    assert_eq!(parse_duration("1h30m").unwrap(), Duration::from_secs(5400));
    for bad in ["", "soon", "5x", "m5"] {
        assert!(parse_duration(bad).is_err(), "{bad}");
    }
}

#[tokio::test]
async fn text_an_agent_wrote_cannot_reach_the_terminal_as_control_characters() {
    let hostile = "\u{1b}[2J\u{202E}evil";
    let denied = approval(
        "denied",
        json!({"decisionNote": hostile, "summary": hostile}),
    );
    let engine = engine("changes.keep", vec![denied], Value::Null);
    let error = exit_code(&engine, &["merge", "try"]).await.unwrap_err();
    assert!(
        !error.message.chars().any(|c| c.is_control()),
        "{:?}",
        error.message
    );
    assert!(!error.message.contains('\u{202E}'));
}
