//! Tauri adapters for approvals: what the agents inside computers ask the person who runs
//! Silo for, the audit log of what was done, and the device's remaining resources.
use crate::engine_client::{
    params::{device, job_id},
    Engine, EngineClient, EngineError,
};
use serde_json::{json, Value};
use silo_api::methods::{
    approvals::{ApprovalDecision, ApprovalState, ApprovalsDecideParams, ApprovalsListParams},
    audit::{AuditCallerKind, AuditListParams},
    operations::GetJobParams,
    requests,
};

pub(crate) async fn list(
    engine: &EngineClient,
    state: Option<ApprovalState>,
    computer_id: Option<&str>,
) -> Result<Value, EngineError> {
    engine
        .call(requests::approvals_list(ApprovalsListParams {
            state,
            computer_id: computer_id.map(str::to_owned),
        }))
        .await
}

pub(crate) async fn get(engine: &EngineClient, id: &str) -> Result<Value, EngineError> {
    engine
        .call(requests::approvals_get(
            silo_api::methods::approvals::ApprovalIdParams { id: id.to_owned() },
        ))
        .await
}

pub(crate) async fn decide(
    engine: &EngineClient,
    id: &str,
    decision: ApprovalDecision,
    note: Option<&str>,
) -> Result<Value, EngineError> {
    engine
        .call(requests::approvals_decide(ApprovalsDecideParams {
            id: id.to_owned(),
            decision,
            note: note
                .map(str::trim)
                .filter(|note| !note.is_empty())
                .map(str::to_owned),
        }))
        .await
}

pub(crate) async fn audit(
    engine: &EngineClient,
    computer_id: Option<&str>,
    caller_kind: Option<AuditCallerKind>,
    before_id: Option<u64>,
    limit: Option<u32>,
) -> Result<Value, EngineError> {
    engine
        .call(requests::audit_list(AuditListParams {
            computer_id: computer_id.map(str::to_owned),
            caller_kind,
            before_id,
            limit,
        }))
        .await
}

pub(crate) async fn resources(
    engine: &EngineClient,
    device_id: Option<&str>,
) -> Result<Value, EngineError> {
    engine
        .call(requests::device_resources(device(device_id)))
        .await
}

/// How a job an allowed approval started is going: its state, why it failed, and the status
/// and refusal message of a result that ended without doing the work.
pub(crate) async fn job_progress(engine: &EngineClient, id: &str) -> Result<Value, EngineError> {
    let job = engine
        .call(requests::operations_job_get(GetJobParams {
            job_id: job_id(id)?,
            device_id: None,
            offset_bytes: None,
        }))
        .await;
    let job = match job {
        Ok(job) => job,
        Err(error) if error.is("not_found") => {
            return Ok(json!({"state": "gone", "error": null, "status": null, "message": null}))
        }
        Err(error) => return Err(error),
    };
    let text = |pointer: &str| job.pointer(pointer).cloned().unwrap_or(Value::Null);
    Ok(json!({
        "state": text("/state"),
        "error": text("/outcome/error/message"),
        "status": text("/outcome/result/status"),
        "message": text("/outcome/result/refusal/message"),
    }))
}

#[tauri::command]
pub(crate) async fn list_approvals(
    engine: Engine<'_>,
    state: Option<ApprovalState>,
    computer_id: Option<String>,
) -> Result<Value, String> {
    list(&engine, state, computer_id.as_deref())
        .await
        .map_err(String::from)
}

#[tauri::command]
pub(crate) async fn get_approval(engine: Engine<'_>, id: String) -> Result<Value, String> {
    get(&engine, &id).await.map_err(String::from)
}

#[tauri::command]
pub(crate) async fn decide_approval(
    engine: Engine<'_>,
    id: String,
    decision: ApprovalDecision,
    note: Option<String>,
) -> Result<Value, String> {
    decide(&engine, &id, decision, note.as_deref())
        .await
        .map_err(String::from)
}

#[tauri::command]
pub(crate) async fn read_approval_job(engine: Engine<'_>, job_id: String) -> Result<Value, String> {
    job_progress(&engine, &job_id).await.map_err(String::from)
}

#[tauri::command]
pub(crate) async fn list_audit(
    engine: Engine<'_>,
    computer_id: Option<String>,
    caller_kind: Option<AuditCallerKind>,
    before_id: Option<u64>,
    limit: Option<u32>,
) -> Result<Value, String> {
    audit(
        &engine,
        computer_id.as_deref(),
        caller_kind,
        before_id,
        limit,
    )
    .await
    .map_err(String::from)
}

#[tauri::command]
pub(crate) async fn read_device_resources(
    engine: Engine<'_>,
    device_id: Option<String>,
) -> Result<Value, String> {
    resources(&engine, device_id.as_deref())
        .await
        .map_err(String::from)
}

#[cfg(test)]
mod forwarding_tests {
    use super::*;
    use crate::engine_client::testing::{connected_client, FakeEngine};
    use serde_json::json;
    use silo_api::methods::MethodName;

    fn replies(method: MethodName, params: &Value) -> Result<Value, (&'static str, String)> {
        match method {
            MethodName::EventsSubscribe => Ok(json!({"subscription": "sub"})),
            MethodName::ApprovalsList => Ok(json!({"approvals": []})),
            MethodName::AuditList => Ok(json!({"entries": []})),
            MethodName::OperationsJobGet if params["jobId"] == "job:2" => {
                Err(("not_found", "The job was not found.".to_owned()))
            }
            MethodName::OperationsJobGet => Ok(json!({"jobId": "job:1", "state": "finished",
                "outcome": {"error": {"code": "busy", "message": "The port is taken."}}})),
            _ => Ok(Value::Null),
        }
    }

    #[tokio::test]
    async fn queries_forward_their_filters() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        list(&engine, Some(ApprovalState::Pending), Some("c1"))
            .await
            .unwrap();
        audit(
            &engine,
            Some("c1"),
            Some(AuditCallerKind::Computer),
            Some(40),
            Some(25),
        )
        .await
        .unwrap();
        resources(&engine, Some("d2")).await.unwrap();
        get(&engine, "ap-1").await.unwrap();
        assert_eq!(
            fake.calls(MethodName::ApprovalsList),
            vec![json!({"state": "pending", "computerId": "c1"})]
        );
        assert_eq!(
            fake.calls(MethodName::AuditList),
            vec![
                json!({"computerId": "c1", "callerKind": "computer", "beforeId": 40, "limit": 25})
            ]
        );
        assert_eq!(
            fake.calls(MethodName::DeviceResources),
            vec![json!({"deviceId": "d2"})]
        );
        assert_eq!(
            fake.calls(MethodName::ApprovalsGet),
            vec![json!({"id": "ap-1"})]
        );
    }

    #[tokio::test]
    async fn a_started_jobs_progress_is_its_state_and_error_message() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        let progress = job_progress(&engine, "job:1").await.unwrap();
        assert_eq!(
            progress,
            json!({"state": "finished", "error": "The port is taken.", "status": null, "message": null})
        );
        assert!(job_progress(&engine, "not a job").await.is_err());
    }

    #[tokio::test]
    async fn a_job_the_engine_no_longer_holds_reads_as_gone() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        let progress = job_progress(&engine, "job:2").await.unwrap();
        assert_eq!(
            progress,
            json!({"state": "gone", "error": null, "status": null, "message": null})
        );
    }

    #[tokio::test]
    async fn an_answer_carries_its_decision_and_a_trimmed_note() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        decide(&engine, "ap-1", ApprovalDecision::Deny, Some("  not now  "))
            .await
            .unwrap();
        decide(&engine, "ap-2", ApprovalDecision::Allow, Some("   "))
            .await
            .unwrap();
        assert_eq!(
            fake.calls(MethodName::ApprovalsDecide),
            vec![
                json!({"id": "ap-1", "decision": "deny", "note": "not now"}),
                json!({"id": "ap-2", "decision": "allow"}),
            ]
        );
    }

    #[test]
    fn the_contract_fixtures_are_what_the_typed_results_read() {
        use silo_api::methods::{approvals as ap, audit as au, device};
        let fixtures: Value =
            serde_json::from_str(include_str!("../../../src/test/contracts/approvals.json"))
                .unwrap();
        fn read<T: serde::de::DeserializeOwned>(fixtures: &Value, key: &str) -> T {
            serde_json::from_value(fixtures[key].clone()).unwrap()
        }
        let listed: ap::ApprovalsListResult = read(&fixtures, "list");
        assert!(listed.approvals.len() >= 3);
        assert_eq!(
            read::<ap::Approval>(&fixtures, "get").state,
            ap::ApprovalState::Pending
        );
        assert_eq!(
            read::<ap::Approval>(&fixtures, "decided").state,
            ap::ApprovalState::Allowed
        );
        assert!(!read::<au::AuditListResult>(&fixtures, "audit")
            .entries
            .is_empty());
        assert!(!read::<device::DeviceResources>(&fixtures, "resources")
            .computers
            .is_empty());
    }
}
