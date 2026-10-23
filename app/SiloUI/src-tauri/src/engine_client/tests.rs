use std::{sync::Arc, time::Duration};

use serde_json::{json, Value};
use silo_api::methods::{
    checkpoints::{CheckpointCreateParams, CheckpointTargetParams},
    common::{ComputerTargetParams, EmptyParams},
    computers::{ComputerDeleteParams, ComputerListParams},
    engine::{EngineShutdownParams, ShutdownReason},
    requests, MethodName,
};

use super::{
    testing::{connected_client, metadata, FailingLauncher, FakeEngine, RecordingSink},
    EngineClient, Status,
};

fn list() -> silo_api::methods::Request<silo_api::methods::kind::Query> {
    requests::computers_list(ComputerListParams {
        device_id: None,
        refresh_repositories: None,
    })
}

fn checkpoint(id: &str) -> CheckpointTargetParams {
    CheckpointTargetParams {
        computer_id: "c".into(),
        device_id: None,
        checkpoint_id: id.into(),
    }
}

fn ok(value: Value) -> Result<Value, (&'static str, String)> {
    Ok(value)
}

#[tokio::test]
async fn forwards_a_call_subscribes_once_and_maps_typed_errors() {
    let fake = FakeEngine::start(|method, _| match method {
        MethodName::ComputersList => ok(json!({"linux": {}})),
        MethodName::EventsSubscribe => ok(json!({"subscription": "sub"})),
        MethodName::ComputersDelete => Err(("busy", "Another operation is running.".into())),
        MethodName::SettingsRead => Err(("permission_denied", "No.".into())),
        _ => ok(Value::Null),
    });
    let (client, _sink) = connected_client(&fake).await;
    assert_eq!(client.call(list()).await.unwrap(), json!({"linux": {}}));
    let busy = client
        .call(requests::computers_delete(ComputerDeleteParams {
            computer_id: "c".into(),
            device_id: None,
            expected: None,
            request_id: None,
        }))
        .await
        .unwrap_err();
    assert_eq!(
        (busy.code.as_str(), busy.message.as_str()),
        ("busy", "Another operation is running.")
    );
    // Codes the UI does not know are plain failures.
    assert_eq!(
        client
            .call(requests::settings_read(EmptyParams {}))
            .await
            .unwrap_err()
            .code,
        "internal"
    );
    let subscriptions = fake.calls(MethodName::EventsSubscribe);
    assert_eq!(subscriptions.len(), 1);
    assert!(subscriptions[0]["events"]
        .as_array()
        .unwrap()
        .contains(&json!("computers.*")));
}

#[tokio::test]
async fn relays_engine_events_to_the_sink() {
    let fake = FakeEngine::start(|_, _| ok(json!({"subscription": "sub"})));
    let (_client, sink) = connected_client(&fake).await;
    fake.push_event("operations.queue.changed", json!({"running": []}));
    for _ in 0..50 {
        if !sink.events.lock().unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let events = sink.events.lock().unwrap().clone();
    assert_eq!(events[0]["event"], "operations.queue.changed");
    assert_eq!(sink.connections.lock().unwrap().first(), Some(&true));
}

#[tokio::test]
async fn a_job_is_awaited_through_its_outcome_and_then_acknowledged() {
    let polls = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let seen = polls.clone();
    let fake = FakeEngine::start(move |method, params| match method {
        MethodName::ComputersStart => ok(json!({"jobId": "job:7"})),
        MethodName::OperationsJobGet => {
            assert_eq!(params["jobId"], "job:7");
            if seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst) < 2 {
                ok(json!({"jobId": "job:7", "state": "running"}))
            } else {
                ok(
                    json!({"jobId": "job:7", "state": "finished", "outcome": {"result": {"started": true}}}),
                )
            }
        }
        _ => ok(json!({"subscription": "sub"})),
    });
    let (client, _sink) = connected_client(&fake).await;
    let result = client
        .run_job(requests::computers_start(ComputerTargetParams {
            computer_id: "c".into(),
            device_id: None,
        }))
        .await
        .unwrap();
    assert_eq!(result, json!({"started": true}));
    assert_eq!(
        fake.calls(MethodName::OperationsJobAcknowledge),
        vec![json!({"jobId": "job:7"})]
    );
}

#[tokio::test]
async fn a_failed_job_surfaces_its_typed_error_and_a_chunked_outcome_is_reassembled() {
    let fake = FakeEngine::start(|method, params| match method {
        MethodName::CheckpointsRestore => ok(json!({"jobId": "job:1"})),
        MethodName::CheckpointsCreate => ok(json!({"jobId": "job:2"})),
        MethodName::OperationsJobGet if params["jobId"] == "job:1" => ok(json!({
            "jobId": "job:1", "state": "finished",
            "outcome": {"error": {"code": "cancelled", "message": "Stopped."}}})),
        MethodName::OperationsJobGet => {
            let whole = r#"{"result":{"big":"abcdefgh"}}"#;
            match params.get("offsetBytes").and_then(Value::as_u64) {
                None => ok(
                    json!({"jobId": "job:2", "state": "finished", "outcomeChunk": {
                    "data": &whole[..12], "totalBytes": whole.len(), "offsetBytes": 0, "nextOffsetBytes": 12}}),
                ),
                Some(12) => ok(
                    json!({"jobId": "job:2", "state": "finished", "outcomeChunk": {
                    "data": &whole[12..], "totalBytes": whole.len(), "offsetBytes": 12}}),
                ),
                Some(_) => Err(("internal", "bad offset".into())),
            }
        }
        _ => ok(json!({"subscription": "sub"})),
    });
    let (client, _sink) = connected_client(&fake).await;
    let error = client
        .run_job(requests::checkpoints_restore(checkpoint("a")))
        .await
        .unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.message.as_str()),
        ("cancelled", "Stopped.")
    );
    let result = client
        .run_job(requests::checkpoints_create(CheckpointCreateParams {
            computer_id: "c".into(),
            device_id: None,
            name: "n".into(),
            kind: None,
        }))
        .await
        .unwrap();
    assert_eq!(result, json!({"big": "abcdefgh"}));
}

#[tokio::test]
async fn the_client_reconnects_and_resubscribes_after_the_connection_drops() {
    let fake = FakeEngine::start(|_, _| ok(json!({"subscription": "sub"})));
    let (client, sink) = connected_client(&fake).await;
    assert_eq!(fake.connections(), 1);
    fake.drop_connections();
    for _ in 0..100 {
        if fake.connections() >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(fake.connections(), 2);
    client
        .call(requests::engine_state(EmptyParams {}))
        .await
        .unwrap();
    assert_eq!(fake.calls(MethodName::EventsSubscribe).len(), 2);
    let connections = sink.connections.lock().unwrap().clone();
    assert_eq!(connections, vec![true, false, true]);
}

#[tokio::test]
async fn an_unavailable_engine_is_reported_with_the_launcher_message() {
    let sink = Arc::new(RecordingSink::default());
    let client = EngineClient::new(
        "/nonexistent/engine.sock".into(),
        metadata(),
        Arc::new(FailingLauncher("Allow the Silo engine in Login Items.")),
        sink.clone(),
    );
    tokio::spawn(client.clone().run());
    for _ in 0..50 {
        if matches!(client.status(), Status::Unavailable(_)) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        client.status(),
        Status::Unavailable("Allow the Silo engine in Login Items.".into())
    );
    assert_eq!(sink.connections.lock().unwrap().first(), Some(&false));
}

#[tokio::test]
async fn a_shutdown_job_ends_when_the_engine_exits_after_stopping_the_computers() {
    let polls = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let seen = polls.clone();
    let fake = Arc::new(FakeEngine::start(move |method, _| match method {
        MethodName::EngineShutdown => ok(json!({"jobId": "job:3"})),
        MethodName::OperationsJobGet => {
            seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            ok(json!({"jobId": "job:3", "state": "running"}))
        }
        _ => ok(json!({"subscription": "sub"})),
    }));
    let (client, _sink) = connected_client(&fake).await;
    let stopper = fake.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        stopper.stop();
    });
    let result = client
        .run_job_until_exit(requests::engine_shutdown(EngineShutdownParams {
            stop_computers: true,
            reason: ShutdownReason::User,
            deadline_ms: None,
        }))
        .await
        .unwrap();
    assert_eq!(result, Value::Null);
    assert!(polls.load(std::sync::atomic::Ordering::SeqCst) >= 1);
    // An ordinary job keeps waiting through a lost connection instead of inventing a result.
    assert!(fake.calls(MethodName::OperationsJobAcknowledge).is_empty());
}

#[tokio::test]
async fn a_method_the_engine_does_not_advertise_is_unavailable_without_a_request() {
    let fake = FakeEngine::start(|_, _| ok(json!({"subscription": "sub"})));
    fake.advertise(&[
        MethodName::EngineState,
        MethodName::EventsSubscribe,
        MethodName::SettingsRead,
        MethodName::ComputersList,
    ]);
    let (client, _sink) = connected_client(&fake).await;
    let error = client
        .call(requests::updates_state(EmptyParams {}))
        .await
        .unwrap_err();
    assert_eq!(error.code, "unsupported_remote_operation");
    assert!(error.message.contains(MethodName::UpdatesState.name()));
    assert!(fake.calls(MethodName::UpdatesState).is_empty());
    assert!(client.call(list()).await.is_ok());
}
