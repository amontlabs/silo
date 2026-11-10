use std::collections::BTreeSet;

use serde_json::{json, Value};
use silo_api::{
    jobs::ErrorCode,
    methods::{
        audit::{AuditCallerKind, AuditEntry, AuditListParams, AuditOutcome},
        MethodName,
    },
    transport::{ApprovalKind, PROTOCOL_VERSION},
};
use silo_engine::{
    audit::{AuditLog, Scope},
    context::EngineContext,
    policy::{store::PolicyStore, Caller},
};

use crate::{
    dispatcher::{DispatchError, RequestContext},
    methods::{access::tests::fixture, dispatch, HandlerContext},
};

fn context(caller: Caller) -> HandlerContext {
    HandlerContext::new(
        fixture().context.clone(),
        RequestContext {
            client_id: "gate-test".into(),
            protocol: PROTOCOL_VERSION,
            client: None,
            caller,
        },
    )
}

/// A computer of its own for each test (the fixture's data folder is shared), which made
/// `fork`.
fn computer() -> (String, String) {
    let id = format!("gate-{}", uuid::Uuid::new_v4());
    let fork = format!("{id}-fork");
    PolicyStore::new(&fixture().context.paths().app_data)
        .import_legacy(&id, std::slice::from_ref(&fork), &[])
        .unwrap();
    (id, fork)
}

async fn call(caller: &Caller, method: MethodName, params: Value) -> Result<Value, DispatchError> {
    dispatch(&context(caller.clone()), method, &params)
        .await
        .map(|output| output.value)
}

fn rows_about(ids: &[&str]) -> Vec<AuditEntry> {
    let visible: BTreeSet<String> = ids.iter().map(|id| (*id).to_owned()).collect();
    AuditLog::shared(&fixture().context.paths().app_data)
        .list(
            &AuditListParams {
                computer_id: None,
                caller_kind: None,
                before_id: None,
                limit: Some(500),
            },
            Scope::Computers(&visible),
        )
        .unwrap()
        .entries
}

#[tokio::test]
async fn a_computers_call_outside_its_rules_is_refused_before_any_handler_and_recorded() {
    let (id, _fork) = computer();
    let caller = Caller::Computer { id: id.clone() };
    let error = call(&caller, MethodName::SettingsRead, json!({}))
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::PermissionDenied);
    assert!(error.message.contains("settings.read"), "{}", error.message);
    // A computer it did not make, such as one the owner forked by hand.
    let error = call(
        &caller,
        MethodName::GuestExec,
        json!({"computerId": "someone-else", "argv": ["true"], "timeoutMs": 1000}),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::PermissionDenied);
    let error = call(&caller, MethodName::OperationsJobGet, json!({"jobId": "job:424242"}))
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::PermissionDenied);
    let rows = rows_about(&[&id]);
    assert_eq!(rows.len(), 3, "{rows:?}");
    assert!(rows.iter().all(|row| row.outcome == AuditOutcome::Refused
        && row.caller_kind == AuditCallerKind::Computer
        && row.caller_id.as_deref() == Some(id.as_str())
        && row.error_code.as_deref() == Some("permission_denied")));
    assert_eq!(rows[1].computer_id.as_deref(), Some("someone-else"));
}

#[tokio::test]
async fn a_widening_call_asks_with_an_engine_written_summary() {
    let (id, fork) = computer();
    let caller = Caller::Computer { id: id.clone() };
    let error = call(
        &caller,
        MethodName::SecretsSave,
        json!({"request": {"operation": "add", "name": "deploy", "value": "very-secret",
            "computerIds": [fork], "allowedDomains": ["deploy.example.com"]}}),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::ApprovalRequired);
    let details = error.details.unwrap();
    assert_eq!(details.kind, ApprovalKind::Secret);
    assert!(details.summary.contains("deploy.example.com"));
    assert!(!details.summary.contains("very-secret"));
    let approval = details.approval_id.unwrap();
    let rows = rows_about(&[&id]);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].outcome, AuditOutcome::Asked);
    assert_eq!(rows[0].error_code.as_deref(), Some("approval_required"));
    assert_eq!(rows[0].approval_id.as_deref(), Some(approval.as_str()));
}

#[tokio::test]
async fn a_computer_reads_the_audit_rows_about_itself_and_what_it_made_only() {
    let (id, fork) = computer();
    let (other, _) = computer();
    for caller in [&id, &other] {
        let _ = call(
            &Caller::Computer { id: caller.clone() },
            MethodName::SettingsRead,
            json!({}),
        )
        .await;
    }
    let _ = call(
        &Caller::owner("app"),
        MethodName::OperationsCancel,
        json!({"jobId": "job:424243", "computerId": fork}),
    )
    .await;
    let page = call(
        &Caller::Computer { id: id.clone() },
        MethodName::AuditList,
        json!({"limit": 500}),
    )
    .await
    .unwrap();
    let entries: Vec<AuditEntry> = serde_json::from_value(page["entries"].clone()).unwrap();
    assert!(entries.iter().any(|row| row.caller_id.as_deref() == Some(id.as_str())));
    assert!(entries
        .iter()
        .any(|row| row.computer_id.as_deref() == Some(fork.as_str())));
    for row in &entries {
        let about_tree = [Some(id.as_str()), Some(fork.as_str())];
        assert!(
            about_tree.contains(&row.computer_id.as_deref())
                || about_tree.contains(&row.caller_id.as_deref()),
            "{row:?}"
        );
    }
}

#[tokio::test]
async fn an_owners_command_is_recorded_and_a_query_is_not() {
    let marker = format!("job:{}", 900_000 + (uuid::Uuid::new_v4().as_u128() % 99_999) as u64);
    let target = format!("owner-target-{}", uuid::Uuid::new_v4());
    let failed = call(
        &Caller::owner("app"),
        MethodName::OperationsCancel,
        json!({"jobId": marker, "computerId": target}),
    )
    .await;
    assert!(failed.is_err());
    let _ = call(
        &Caller::owner("app"),
        MethodName::AuditList,
        json!({"computerId": target}),
    )
    .await
    .unwrap();
    let rows = rows_about(&[&target]);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].method, "operations.cancel");
    assert_eq!(rows[0].caller_kind, AuditCallerKind::Owner);
    assert_eq!(rows[0].client.as_deref(), Some("app"));
    assert_eq!(rows[0].outcome, AuditOutcome::Error);
    assert!(rows[0].error_code.is_some());
}

#[tokio::test]
async fn another_device_reaches_only_the_connections_methods() {
    let device = Caller::Device {
        key: Some("SHA256:gate-test".into()),
    };
    let error = call(&device, MethodName::AuditList, json!({}))
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::PermissionDenied);
    let error = call(&device, MethodName::SettingsUpdate, json!({"patch": {}}))
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::PermissionDenied);
}
