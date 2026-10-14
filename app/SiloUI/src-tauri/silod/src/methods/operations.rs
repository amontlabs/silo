//! `operations.list`, `operations.job.get`, `operations.job.acknowledge` and
//! `operations.cancel`.
//!
//! A job belongs to the engine that does the work and is addressed by `(deviceId, jobId)`
//! (05 job contract, item 6). A request that names this engine's own device, or none, reads
//! this engine's registry. A request that names another device is relayed to that owner over
//! Connections, so the owner keeps the job and its outcome until a client acknowledges it,
//! and the controller never holds a second copy under a different id.

use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use silo_api::{
    jobs::{ErrorCode, JobLookupResult},
    methods::{
        operations::{
            AcknowledgeJobParams, CancelJobParams, GetJobParams, OperationQueue,
            OperationsListParams,
        },
        MethodName,
    },
};

use super::{access::call_owner, HandlerContext};
use crate::dispatcher::{DispatchError, HandlerOutput};

pub(super) fn owns(method: MethodName) -> bool {
    matches!(
        method,
        MethodName::OperationsList
            | MethodName::OperationsJobGet
            | MethodName::OperationsJobAcknowledge
            | MethodName::OperationsCancel
    )
}

pub(crate) async fn handle(
    method: MethodName,
    params: &Value,
    context: &HandlerContext,
) -> Option<Result<HandlerOutput, DispatchError>> {
    if !owns(method) {
        return None;
    }
    Some(match method {
        MethodName::OperationsList => list(params, context).await,
        MethodName::OperationsJobGet => get(params, context).await,
        MethodName::OperationsJobAcknowledge => acknowledge(params, context).await,
        _ => cancel(params, context).await,
    })
}

fn value(value: impl serde::Serialize) -> Result<Value, DispatchError> {
    serde_json::to_value(value).map_err(|error| DispatchError::internal(error.to_string()))
}

fn output(value: Value) -> HandlerOutput {
    HandlerOutput {
        value,
        event_barrier: None,
    }
}

/// `Some(device)` when the request names an engine other than this one.
fn owner_of(context: &HandlerContext, device_id: Option<&str>) -> Option<String> {
    device_id
        .filter(|id| *id != context.silod_context().local_device().device_id)
        .map(str::to_owned)
}

/// Asks the owner of a job, off the async workers, and checks its reply against the
/// canonical type before this engine hands it on. A reply that does not match is the
/// owner's fault, not a result.
async fn ask_owner<T: DeserializeOwned + serde::Serialize>(
    device: String,
    method: &'static str,
    params: Value,
) -> Result<Value, DispatchError> {
    let reply = tokio::task::spawn_blocking(move || call_owner(&device, method, params))
        .await
        .map_err(|_| DispatchError::internal("The remote operation could not finish."))?
        .map_err(HandlerContext::map_engine_error)?;
    let typed: T = serde_json::from_value(reply).map_err(|error| {
        DispatchError::internal(format!(
            "The other device returned an invalid reply to {method}: {error}"
        ))
    })?;
    value(typed)
}

async fn get(params: &Value, context: &HandlerContext) -> Result<HandlerOutput, DispatchError> {
    let typed: GetJobParams = context.decode_params(params)?;
    if let Some(device) = owner_of(context, typed.device_id.as_deref()) {
        let mut request = json!({ "jobId": typed.job_id });
        if let Some(offset) = typed.offset_bytes {
            request["offsetBytes"] = json!(offset);
        }
        return ask_owner::<JobLookupResult>(device, "operations.job.get", request)
            .await
            .map(output);
    }
    let found =
        silo_engine::jobs::get_at(context.engine_context(), &typed.job_id, typed.offset_bytes)
            .map_err(HandlerContext::map_registry_error)?
            .ok_or_else(|| DispatchError {
                code: ErrorCode::NotFound,
                message: "The job was not found.".to_owned(),
            })?;
    Ok(output(value(found)?))
}

async fn acknowledge(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let typed: AcknowledgeJobParams = context.decode_params(params)?;
    if let Some(device) = owner_of(context, typed.device_id.as_deref()) {
        return ask_owner::<Option<()>>(
            device,
            "operations.job.acknowledge",
            json!({ "jobId": typed.job_id }),
        )
        .await
        .map(output);
    }
    context
        .job_registry()?
        .acknowledge(&typed.job_id)
        .map_err(HandlerContext::map_registry_error)?;
    // The outcome is saved and acknowledged: the replay of an exec job is no longer needed
    // (and does nothing for any other job).
    silo_engine::guest::forget_finished(&typed.job_id);
    context.engine_context().job_queue_changed();
    Ok(output(Value::Null))
}

async fn cancel(params: &Value, context: &HandlerContext) -> Result<HandlerOutput, DispatchError> {
    let typed: CancelJobParams = context.decode_params(params)?;
    if let Some(device) = owner_of(context, typed.device_id.as_deref()) {
        return ask_owner::<Option<()>>(
            device,
            "operations.cancel",
            json!({ "jobId": typed.job_id }),
        )
        .await
        .map(output);
    }
    context
        .job_registry()?
        .cancel(&typed.job_id)
        .map_err(HandlerContext::map_registry_error)?;
    Ok(output(Value::Null))
}

async fn list(params: &Value, context: &HandlerContext) -> Result<HandlerOutput, DispatchError> {
    let typed: OperationsListParams = context.decode_params(params)?;
    if let Some(device) = owner_of(context, typed.device_id.as_deref()) {
        return ask_owner::<OperationQueue>(device, "operations.list", json!({}))
            .await
            .map(output);
    }
    let snapshot = context
        .silod_context()
        .operation_queue_snapshot(context.client_id())
        .map_err(DispatchError::internal)?;
    Ok(HandlerOutput {
        value: value(snapshot.queue)?,
        event_barrier: Some(snapshot.event_barrier),
    })
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use serde_json::{json, Value};
    use silo_api::{jobs::ErrorCode, methods::MethodName};
    use silo_engine::bridge_error::{BridgeError, ErrorCode as BridgeCode};

    use super::*;
    use crate::methods::access::tests::{fixture, request_context, stub_owner};

    const UNKNOWN: &str = "88888888-8888-4888-8888-8888888888ff";

    async fn call(method: MethodName, params: Value) -> Result<Value, DispatchError> {
        handle(method, &params, &request_context())
            .await
            .expect("operations owns this method")
            .map(|output| output.value)
    }

    type Seen = Arc<Mutex<Vec<(String, Value)>>>;

    fn recording_owner(
        owner: &str,
        answer: impl Fn(&str, &Value) -> Result<Value, BridgeError> + Send + Sync + 'static,
    ) -> (Seen, crate::methods::access::tests::StubOwner) {
        let seen: Seen = Arc::default();
        let log = seen.clone();
        let stub = stub_owner(owner, move |method, params| {
            log.lock()
                .unwrap()
                .push((method.to_owned(), params.clone()));
            answer(method, params)
        });
        (seen, stub)
    }

    #[tokio::test]
    async fn a_job_on_another_device_is_read_acknowledged_and_cancelled_by_its_owners_id() {
        const OWNER: &str = "88888888-8888-4888-8888-888888888801";
        let (seen, _owner) = recording_owner(OWNER, |method, params| match method {
            "operations.job.get" => Ok(json!({
                "jobId": params["jobId"],
                "state": "finished",
                "outcome": {"result": {"connected": true}},
            })),
            "operations.cancel" | "operations.job.acknowledge" => Ok(Value::Null),
            other => panic!("unexpected owner request {other}"),
        });

        let found = call(
            MethodName::OperationsJobGet,
            json!({"jobId": "push:abc", "deviceId": OWNER, "offsetBytes": 4}),
        )
        .await
        .unwrap();
        assert_eq!(found["jobId"], "push:abc");
        assert_eq!(found["state"], "finished");
        assert_eq!(
            call(
                MethodName::OperationsCancel,
                json!({"jobId": "job:7", "deviceId": OWNER})
            )
            .await
            .unwrap(),
            Value::Null
        );
        assert_eq!(
            call(
                MethodName::OperationsJobAcknowledge,
                json!({"jobId": "job:7", "deviceId": OWNER})
            )
            .await
            .unwrap(),
            Value::Null
        );

        // The owner is asked about its own job id, and is never told which device was meant.
        let seen = seen.lock().unwrap();
        assert_eq!(
            *seen,
            vec![
                (
                    "operations.job.get".to_owned(),
                    json!({"jobId": "push:abc", "offsetBytes": 4})
                ),
                ("operations.cancel".to_owned(), json!({"jobId": "job:7"})),
                (
                    "operations.job.acknowledge".to_owned(),
                    json!({"jobId": "job:7"})
                ),
            ]
        );
    }

    #[tokio::test]
    async fn the_owners_queue_is_listed_and_checked_against_the_canonical_type() {
        const OWNER: &str = "88888888-8888-4888-8888-888888888802";
        let (seen, _owner) = recording_owner(OWNER, |_, _| {
            Ok(json!({
                "running": [{
                    "id": 3, "label": "Starting Box", "kind": "lifecycle", "sinceMs": 5,
                    "cancellable": true, "jobId": "job:9",
                }],
                "waiting": [],
                "unacknowledgedJobIds": ["job:8"],
            }))
        });
        let queue = call(MethodName::OperationsList, json!({"deviceId": OWNER}))
            .await
            .unwrap();
        assert_eq!(queue["running"][0]["jobId"], "job:9");
        assert_eq!(queue["unacknowledgedJobIds"], json!(["job:8"]));
        assert_eq!(seen.lock().unwrap()[0].0, "operations.list");
    }

    #[tokio::test]
    async fn an_owner_reply_that_is_not_the_canonical_type_is_refused() {
        const OWNER: &str = "88888888-8888-4888-8888-888888888803";
        let (_seen, _owner) = recording_owner(OWNER, |_, _| Ok(json!({"state": "finished"})));
        let error = call(
            MethodName::OperationsJobGet,
            json!({"jobId": "job:1", "deviceId": OWNER}),
        )
        .await
        .unwrap_err();
        assert!(error.message.contains("invalid reply"), "{}", error.message);
        let error = call(MethodName::OperationsList, json!({"deviceId": OWNER}))
            .await
            .unwrap_err();
        assert!(error.message.contains("invalid reply"), "{}", error.message);
    }

    #[tokio::test]
    async fn the_owners_error_codes_reach_the_client() {
        const OWNER: &str = "88888888-8888-4888-8888-888888888804";
        let (_seen, _owner) = recording_owner(OWNER, |method, _| match method {
            "operations.job.get" => Err(BridgeError::new(
                BridgeCode::NotFound,
                "The job was not found.",
            )),
            _ => Err(BridgeError::new(BridgeCode::Busy, "busy: still running")),
        });
        let missing = call(
            MethodName::OperationsJobGet,
            json!({"jobId": "job:1", "deviceId": OWNER}),
        )
        .await
        .unwrap_err();
        assert_eq!(missing.code, ErrorCode::NotFound);
        let busy = call(
            MethodName::OperationsJobAcknowledge,
            json!({"jobId": "job:1", "deviceId": OWNER}),
        )
        .await
        .unwrap_err();
        assert_eq!(busy.code, ErrorCode::Busy);
    }

    #[tokio::test]
    async fn a_device_that_is_not_connected_is_reported_for_every_operation() {
        for (method, params) in [
            (MethodName::OperationsList, json!({})),
            (MethodName::OperationsJobGet, json!({"jobId": "job:1"})),
            (
                MethodName::OperationsJobAcknowledge,
                json!({"jobId": "job:1"}),
            ),
            (MethodName::OperationsCancel, json!({"jobId": "job:1"})),
        ] {
            let mut params = params;
            params["deviceId"] = json!(UNKNOWN);
            let error = call(method, params).await.unwrap_err();
            assert!(
                error.message.contains("no longer connected"),
                "{}",
                error.message
            );
        }
    }

    #[tokio::test]
    async fn naming_this_device_or_none_reads_this_engines_own_registry() {
        let local = fixture().context.local_device().device_id.clone();
        for device in [None, Some(local)] {
            let mut params = json!({"jobId": "job:999999"});
            if let Some(device) = device {
                params["deviceId"] = json!(device);
            }
            let error = call(MethodName::OperationsJobGet, params.clone())
                .await
                .unwrap_err();
            assert_eq!(error.code, ErrorCode::NotFound);
            let error = call(MethodName::OperationsCancel, params)
                .await
                .unwrap_err();
            assert_ne!(error.message, "", "the local registry answered");
        }
        // The queue read is bound to the client's event stream.
        let _events = fixture()
            .context
            .connect_event_stream(silo_engine::context::ClientId::new("access-test-client"));
        let queue = call(MethodName::OperationsList, json!({})).await.unwrap();
        assert!(queue["unacknowledgedJobIds"].is_array());
    }

    #[tokio::test]
    async fn the_owner_serves_its_queue_to_a_controller() {
        let queue = crate::methods::access::owner_job_operation_for_tests(
            fixture().context.as_ref(),
            "operations.list",
            &json!({}),
        )
        .unwrap();
        assert!(queue["running"].is_array());
        assert!(queue["unacknowledgedJobIds"].is_array());
    }
}
