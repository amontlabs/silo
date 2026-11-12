//! Tauri adapters for review requests: `changes.requests.*`, `steps.list` and `computers.tree`.
//! Job methods wait for their outcome and return its result.
use std::collections::BTreeMap;

use crate::engine_client::{params::device, Engine, EngineClient, EngineError};
use serde_json::Value;
use silo_api::methods::{
    keep::KeepAnswer,
    requests,
    review_requests::{
        ChangesRequestsKeepParams, ChangesRequestsListParams, RequestFilter, RequestIdParams,
    },
    steps::StepsListParams,
};

fn request_id(device_id: Option<&str>, request_id: &str) -> RequestIdParams {
    RequestIdParams {
        device_id: device_id.map(str::to_owned),
        request_id: request_id.to_owned(),
    }
}

pub(crate) async fn list(
    engine: &EngineClient,
    status: Option<RequestFilter>,
    device_id: Option<&str>,
    all_devices: bool,
) -> Result<Value, EngineError> {
    engine
        .call(requests::changes_requests_list(ChangesRequestsListParams {
            status,
            device_id: device_id.map(str::to_owned),
            all_devices: all_devices.then_some(true),
        }))
        .await
}

pub(crate) async fn show(
    engine: &EngineClient,
    device_id: Option<&str>,
    id: &str,
) -> Result<Value, EngineError> {
    engine
        .run_job(requests::changes_requests_show(request_id(device_id, id)))
        .await
}

pub(crate) async fn keep(
    engine: &EngineClient,
    device_id: Option<&str>,
    id: &str,
    data_ok: bool,
    choices: Option<BTreeMap<String, KeepAnswer>>,
) -> Result<Value, EngineError> {
    engine
        .run_job(requests::changes_requests_keep(ChangesRequestsKeepParams {
            device_id: device_id.map(str::to_owned),
            request_id: id.to_owned(),
            data_ok: data_ok.then_some(true),
            keep: None,
            choices: choices.filter(|choices| !choices.is_empty()),
            skip: None,
        }))
        .await
}

pub(crate) async fn throw_away(
    engine: &EngineClient,
    device_id: Option<&str>,
    id: &str,
) -> Result<Value, EngineError> {
    engine
        .run_job(requests::changes_requests_throw_away(request_id(
            device_id, id,
        )))
        .await
}

pub(crate) async fn open(
    engine: &EngineClient,
    device_id: Option<&str>,
    id: &str,
) -> Result<Value, EngineError> {
    engine
        .run_job(requests::changes_requests_open(request_id(device_id, id)))
        .await
}

pub(crate) async fn steps(
    engine: &EngineClient,
    device_id: Option<&str>,
    computer_id: &str,
) -> Result<Value, EngineError> {
    engine
        .call(requests::steps_list(StepsListParams {
            device_id: device_id.map(str::to_owned),
            computer_id: computer_id.to_owned(),
        }))
        .await
}

pub(crate) async fn tree(
    engine: &EngineClient,
    device_id: Option<&str>,
) -> Result<Value, EngineError> {
    engine
        .call(requests::computers_tree(device(device_id)))
        .await
}

#[tauri::command]
pub(crate) async fn list_review_requests(
    engine: Engine<'_>,
    status: Option<RequestFilter>,
    device_id: Option<String>,
    all_devices: Option<bool>,
) -> Result<Value, String> {
    let all = all_devices.unwrap_or(false);
    list(&engine, status, device_id.as_deref(), all)
        .await
        .map_err(String::from)
}

#[tauri::command]
pub(crate) async fn show_review_request(
    engine: Engine<'_>,
    request_id: String,
    device_id: Option<String>,
) -> Result<Value, String> {
    show(&engine, device_id.as_deref(), &request_id)
        .await
        .map_err(String::from)
}

#[tauri::command]
pub(crate) async fn keep_review_request(
    engine: Engine<'_>,
    request_id: String,
    device_id: Option<String>,
    data_ok: Option<bool>,
    choices: Option<BTreeMap<String, KeepAnswer>>,
) -> Result<Value, String> {
    let data_ok = data_ok.unwrap_or(false);
    keep(&engine, device_id.as_deref(), &request_id, data_ok, choices)
        .await
        .map_err(String::from)
}

#[tauri::command]
pub(crate) async fn throw_away_review_request(
    engine: Engine<'_>,
    request_id: String,
    device_id: Option<String>,
) -> Result<Value, String> {
    throw_away(&engine, device_id.as_deref(), &request_id)
        .await
        .map_err(String::from)
}

#[tauri::command]
pub(crate) async fn open_review_request(
    engine: Engine<'_>,
    request_id: String,
    device_id: Option<String>,
) -> Result<Value, String> {
    open(&engine, device_id.as_deref(), &request_id)
        .await
        .map_err(String::from)
}

#[tauri::command]
pub(crate) async fn list_computer_steps(
    engine: Engine<'_>,
    computer_id: String,
    device_id: Option<String>,
) -> Result<Value, String> {
    steps(&engine, device_id.as_deref(), &computer_id)
        .await
        .map_err(String::from)
}

#[tauri::command]
pub(crate) async fn read_computer_tree(
    engine: Engine<'_>,
    device_id: Option<String>,
) -> Result<Value, String> {
    tree(&engine, device_id.as_deref())
        .await
        .map_err(String::from)
}

#[cfg(test)]
mod forwarding_tests {
    use super::*;
    use crate::engine_client::testing::{connected_client, FakeEngine};
    use serde_json::json;
    use silo_api::methods::MethodName;

    const DEV: &str = "00000000-0000-4000-8000-00000000000d";

    fn replies(method: MethodName, _params: &Value) -> Result<Value, (&'static str, String)> {
        match method {
            MethodName::EventsSubscribe => Ok(json!({"subscription": "sub"})),
            MethodName::ChangesRequestsShow
            | MethodName::ChangesRequestsKeep
            | MethodName::ChangesRequestsThrowAway
            | MethodName::ChangesRequestsOpen => Ok(json!({"jobId": "job:1"})),
            MethodName::OperationsJobGet => {
                Ok(json!({"jobId": "job:1", "state": "finished", "outcome": {"result": {"ok": 1}}}))
            }
            MethodName::ChangesRequestsList => Ok(json!({"requests": []})),
            MethodName::StepsList => Ok(json!({"steps": []})),
            MethodName::ComputersTree => Ok(json!({"nodes": []})),
            MethodName::ComputersCredentials => Ok(json!({"used": [], "blocked": []})),
            _ => Ok(Value::Null),
        }
    }

    #[tokio::test]
    async fn queries_forward_their_parameters() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        let all = Some(RequestFilter::All);
        list(&engine, all, None, true).await.unwrap();
        steps(&engine, Some("d2"), DEV).await.unwrap();
        tree(&engine, None).await.unwrap();
        assert_eq!(
            fake.calls(MethodName::ChangesRequestsList),
            vec![json!({"status": "all", "allDevices": true})]
        );
        assert_eq!(
            fake.calls(MethodName::StepsList),
            vec![json!({"deviceId": "d2", "computerId": DEV})]
        );
        assert_eq!(fake.calls(MethodName::ComputersTree), vec![json!({})]);
    }

    #[tokio::test]
    async fn jobs_wait_and_return_their_result() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        assert_eq!(show(&engine, None, "r1").await.unwrap()["ok"], 1);
        let choices = BTreeMap::from([("repo:/root/app".to_owned(), KeepAnswer::Branch)]);
        keep(&engine, None, "r1", true, Some(choices))
            .await
            .unwrap();
        throw_away(&engine, None, "r1").await.unwrap();
        open(&engine, None, "r1").await.unwrap();
        assert_eq!(
            fake.calls(MethodName::ChangesRequestsShow),
            vec![json!({"requestId": "r1"})]
        );
        assert_eq!(
            fake.calls(MethodName::ChangesRequestsKeep),
            vec![
                json!({"requestId": "r1", "dataOk": true, "choices": {"repo:/root/app": "branch"}})
            ]
        );
        assert_eq!(fake.calls(MethodName::ChangesRequestsThrowAway).len(), 1);
        assert_eq!(fake.calls(MethodName::ChangesRequestsOpen).len(), 1);
        assert_eq!(fake.calls(MethodName::OperationsJobAcknowledge).len(), 4);
    }

    #[tokio::test]
    async fn a_failed_job_is_the_commands_error() {
        let fake = FakeEngine::start(|method, params| match method {
            MethodName::OperationsJobGet => Ok(json!({"jobId": "job:1", "state": "finished",
                "outcome": {"error": {"code": "busy", "message": "The changes moved on."}}})),
            other => replies(other, params),
        });
        let (engine, _) = connected_client(&fake).await;
        let error = keep(&engine, None, "r1", false, None).await.unwrap_err();
        assert_eq!(error.code, "busy");
    }

    #[test]
    fn the_contract_fixtures_are_what_the_typed_results_read() {
        use silo_api::methods::{credentials_used, review_requests as rr, steps};
        let fixtures: Value = serde_json::from_str(include_str!(
            "../../../src/test/contracts/review-requests.json"
        ))
        .unwrap();
        fn read<T: serde::de::DeserializeOwned>(fixtures: &Value, key: &str) -> T {
            serde_json::from_value(fixtures[key].clone()).unwrap()
        }
        assert_eq!(
            read::<rr::ChangesRequestsListResult>(&fixtures, "list")
                .requests
                .len(),
            2
        );
        assert!(read::<rr::ChangesRequestsShowResult>(&fixtures, "show").fresh);
        assert_eq!(
            read::<rr::ChangesRequestsKeepResult>(&fixtures, "keep").status,
            "kept"
        );
        assert!(
            read::<rr::ChangesRequestsKeepResult>(&fixtures, "refusedKeep")
                .refusal
                .is_some()
        );
        let needed = read::<rr::ChangesRequestsKeepResult>(&fixtures, "choicesNeeded");
        assert_eq!(needed.refusal.unwrap().conflicts.len(), 2);
        assert_eq!(needed.request.approval_id.as_deref(), Some("ap-merge"));
        assert_eq!(
            read::<rr::ChangesRequestsKeepResult>(&fixtures, "fastForwardKeep")
                .report
                .fast_forward,
            Some(true)
        );
        assert_eq!(
            read::<rr::ChangesRequestsThrowAwayResult>(&fixtures, "throwAway").dropped,
            "login-fix"
        );
        assert!(read::<rr::ChangesRequestsOpenResult>(&fixtures, "open").started);
        assert_eq!(
            read::<steps::StepsListResult>(&fixtures, "steps")
                .steps
                .len(),
            2
        );
        assert_eq!(
            read::<steps::ComputersTreeResult>(&fixtures, "tree")
                .nodes
                .len(),
            2
        );
        assert_eq!(
            read::<credentials_used::CredentialsUsage>(&fixtures, "credentials")
                .used
                .len(),
            1
        );
    }
}
