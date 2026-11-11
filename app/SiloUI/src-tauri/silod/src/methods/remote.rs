//! Computers and devices that belong to another device, reached through its owner.
//!
//! A controller manages everything on the hosts it is connected to (95 A-14), and every such
//! request goes through the controller's own engine (A-3). A method that names another
//! `deviceId` is validated here against its canonical `silo-api` types, forwarded over
//! Connections without the `deviceId`, and the owner's reply is checked against the canonical
//! result type before it is handed on.
//!
//! The owner serves the same canonical name on its bridge (`silo_engine::remote::OWNER_METHODS`)
//! and runs its *own* handler for it, as a local request from the bridge client
//! ([`serve_owner`]). That keeps one implementation per method: what the controller gets is
//! what a local client of the owner would get, including the owner's error codes and, for
//! a job method, the owner's `jobId`, which the client follows by `(deviceId, jobId)` through
//! `operations.*`. A job's progress and outcome events are published to every subscriber of
//! the owner (not to a socket client the owner does not have), so the controller reads them
//! through its relay of the owner's events or by polling `operations.job.get`.
//!
//! An owner never forwards a request on: a `deviceId` other than its own is refused, so a
//! request makes at most one hop.
//!
//! Methods with their own routing (`operations.*`, GitHub, secrets, push, network, SSH
//! access, desktop and computer use, remote guest exec, `logs.export`) are not in this list.

use std::sync::{Arc, OnceLock};

use serde_json::{json, Value};
use silo_api::{
    methods::{method_registry, MethodName, MethodParams, MethodResults},
    transport::PROTOCOL_VERSION,
};
use silo_engine::bridge_error::{BridgeError, ErrorCode as BridgeCode};

use super::{access::call_owner, HandlerContext};
use crate::{
    context::SilodContext,
    dispatcher::{method_name, DispatchError, HandlerOutput, RequestContext},
};

/// The client every request served for another device runs as.
const OWNER_CLIENT: &str = "owner-bridge";

static RUNTIME: OnceLock<tokio::runtime::Handle> = OnceLock::new();

/// Remembers the engine's I/O runtime, which the owner side runs bridged handlers on.
pub fn install_runtime(handle: tokio::runtime::Handle) {
    let _ = RUNTIME.set(handle);
}

/// The engine's I/O runtime, once installed.
pub(crate) fn runtime_handle() -> Option<tokio::runtime::Handle> {
    RUNTIME.get().cloned()
}

/// Whether the owner serves `method` for a controller.
pub(crate) fn is_passthrough(method: MethodName) -> bool {
    silo_engine::remote::is_owner_method(method_name(method))
}

/// The device a request names when it is not this engine: `deviceId` absent, `"local"` or
/// this device's own id is this engine.
fn remote_device(context: &HandlerContext, params: &Value) -> Option<String> {
    let device = params.get("deviceId")?.as_str()?;
    (device != "local" && device != context.silod_context().local_device().device_id)
        .then(|| device.to_owned())
}

/// Controller side: forwards a request that names another device to its owner. `None` when
/// the method is not one the owner serves, or the request is for this engine.
pub(crate) async fn route(
    method: MethodName,
    params: &Value,
    context: &HandlerContext,
) -> Option<Result<HandlerOutput, DispatchError>> {
    if !is_passthrough(method) {
        return None;
    }
    let device = remote_device(context, params)?;
    Some(forward(method, params, device).await)
}

async fn forward(
    method: MethodName,
    params: &Value,
    device: String,
) -> Result<HandlerOutput, DispatchError> {
    let name = method_name(method);
    // The request is checked against its canonical type here, so a malformed one never
    // costs a round trip and the owner is not the first to reject it.
    serde_json::from_value::<MethodParams>(json!({ "method": name, "params": params }))
        .map_err(|error| DispatchError::internal(format!("Invalid params: {error}")))?;
    let mut owner_params = params.clone();
    if let Some(object) = owner_params.as_object_mut() {
        object.remove("deviceId");
        // The owner answers for itself; it does not ask the devices it knows.
        object.remove("allDevices");
    }
    // A file moved through the owner's transfer area needs an owner that serves `exports.file`;
    // an older one would reject the unknown fields in a confusing way.
    let needs_file_sharing = (name == "exports.create" && params["transferArea"] == json!(true))
        || (name == "exports.import" && params["transferName"].is_string());
    let reply = tokio::task::spawn_blocking(move || {
        if needs_file_sharing {
            super::checkpoints::export_transfer::require_file_sharing(&device).map_err(
                |error| BridgeError::new(BridgeCode::UnsupportedRemoteOperation, error.message),
            )?;
        }
        call_owner(&device, name, owner_params)
    })
    .await
    .map_err(|_| DispatchError::internal("The remote operation could not finish."))?
    .map_err(HandlerContext::map_engine_error)?;
    // The owner's answer is shown only in the canonical shape.
    let typed: MethodResults = serde_json::from_value(json!({ "method": name, "result": reply }))
        .map_err(|error| {
        DispatchError::internal(format!(
            "The other device returned an invalid reply to {name}: {error}"
        ))
    })?;
    let mut tagged =
        serde_json::to_value(typed).map_err(|error| DispatchError::internal(error.to_string()))?;
    let value = tagged
        .get_mut("result")
        .map(Value::take)
        .unwrap_or(Value::Null);
    Ok(HandlerOutput {
        value,
        event_barrier: None,
    })
}

fn method_by_name(name: &str) -> Option<MethodName> {
    method_registry()
        .iter()
        .find(|descriptor| descriptor.name == name)
        .map(|descriptor| descriptor.method)
}

/// Owner side: runs a controller's request as one of this engine's own.
/// `key` is the fingerprint of the SSH key the other device connected with, when the bridge
/// could tell; the request runs as that device (`Caller::Device`).
pub fn serve_owner(
    engine: &Arc<SilodContext>,
    name: &str,
    params: &Value,
    key: Option<&str>,
) -> Result<Value, BridgeError> {
    let method = method_by_name(name)
        .filter(|method| is_passthrough(*method))
        .ok_or_else(BridgeError::unsupported)?;
    // A request makes one hop: this engine does not pass it to a third device.
    if let Some(device) = params.get("deviceId").and_then(Value::as_str) {
        if device != "local" && device != engine.local_device().device_id {
            return Err(BridgeError::new(
                BridgeCode::UnsupportedRemoteOperation,
                "This device does not route a request on to another device.",
            ));
        }
    }
    serde_json::from_value::<MethodParams>(json!({ "method": name, "params": params }))
        .map_err(|error| BridgeError::from(format!("Invalid params: {error}")))?;
    if method == MethodName::NotificationsList {
        let notices = silo_engine::notices::list(engine.as_ref()).map_err(BridgeError::from)?;
        return serde_json::to_value(notices).map_err(|error| BridgeError::from(error.to_string()));
    }
    let handle = RUNTIME
        .get()
        .cloned()
        .or_else(|| tokio::runtime::Handle::try_current().ok())
        .ok_or_else(|| BridgeError::from("The engine's runtime is not available."))?;
    let context = HandlerContext::for_owner_bridge(
        engine.clone(),
        RequestContext {
            client_id: OWNER_CLIENT.to_owned(),
            protocol: PROTOCOL_VERSION,
            client: None,
            caller: silo_engine::policy::Caller::Device {
                key: key.map(str::to_owned),
            },
        },
    );
    let mut params = params.clone();
    // A request makes one hop: an owner lists its own requests, never its devices'.
    if let Some(object) = params.as_object_mut() {
        object.remove("allDevices");
    }
    handle
        .block_on(super::dispatch(&context, method, &params))
        .map(|output| output.value)
        .map_err(super::access::bridge_error)
}

/// The error code a controller shows for `error`, for tests.
#[cfg(test)]
pub(crate) fn code_of(error: &BridgeError) -> Option<silo_api::jobs::ErrorCode> {
    error
        .api_code
        .as_deref()
        .and_then(|code| serde_json::from_value(Value::from(code)).ok())
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeSet,
        sync::{Arc, Mutex},
    };

    use serde_json::{json, Value};
    use silo_api::{
        jobs::ErrorCode,
        methods::{method_registry, MethodKind, MethodName},
    };
    use silo_engine::bridge_error::{BridgeError, ErrorCode as BridgeCode};

    use super::*;
    use crate::methods::access::tests::{
        fixture, request_context, stub_owner, StubOwner, COMPUTER, LOCAL_DEVICE, NOT_CONNECTED,
    };

    const THIRD: &str = "88888888-8888-4888-8888-888888888802";

    type Seen = Arc<Mutex<Vec<(String, Value)>>>;

    /// Another device whose answers are canned; it records what the controller sent.
    fn canned_owner(
        owner: &str,
        answer: impl Fn(&str, &Value) -> Result<Value, BridgeError> + Send + Sync + 'static,
    ) -> (Seen, StubOwner) {
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

    /// Another engine: the owner's bridge runs this process's engine as that device would,
    /// so both ends of a request run their real code and only the SSH hop is missing.
    fn engine_as_owner(owner: &str) -> (Seen, StubOwner) {
        canned_owner(owner, |method, params| {
            serve_owner(&fixture().context, method, params, None)
        })
    }

    async fn as_controller(method: MethodName, params: Value) -> Result<Value, DispatchError> {
        crate::methods::dispatch(&request_context(), method, &params)
            .await
            .map(|output| output.value)
    }

    fn with_device(owner: &str, mut params: Value) -> Value {
        params["deviceId"] = json!(owner);
        params
    }

    /// An owner that answers its handshake with these capabilities and serves no stream.
    struct Capable(Vec<&'static str>);

    impl crate::methods::checkpoints::export_transfer::FileLink for Capable {
        fn capabilities(&self, _device: &str) -> Result<Vec<String>, String> {
            Ok(self.0.iter().map(|name| (*name).to_owned()).collect())
        }
        fn open(
            &self,
            _device: &str,
            _params: Value,
            _cancelled: &dyn Fn() -> bool,
        ) -> Result<crate::methods::checkpoints::export_transfer::Opened, String> {
            Err("no stream in this test".into())
        }
    }

    #[tokio::test]
    async fn sharing_through_the_transfer_area_needs_an_owner_that_serves_the_file_stream() {
        const OWNER: &str = "88888888-8888-4888-8888-888888888811";
        let (seen, _owner) = canned_owner(OWNER, |_, _| Ok(json!({ "jobId": "exports:op" })));
        let old = crate::methods::checkpoints::export_transfer::install_test_link(Arc::new(
            Capable(vec!["handshake", "guest.ssh"]),
        ));
        let create = with_device(
            OWNER,
            json!({"destination": "", "computerIds": ["c"], "transferArea": true}),
        );
        let import = with_device(
            OWNER,
            json!({"newName": "copy", "transferName": "i-00000000000000000000000000000000.silo-backup"}),
        );
        for (method, params) in [
            (MethodName::ExportsCreate, create.clone()),
            (MethodName::ExportsImport, import.clone()),
        ] {
            let refused = as_controller(method, params).await.unwrap_err();
            assert_eq!(refused.code, ErrorCode::UnsupportedRemoteOperation);
            assert!(
                refused.message.contains("too old to share"),
                "{}",
                refused.message
            );
        }
        assert!(
            seen.lock().unwrap().is_empty(),
            "the owner was asked anyway"
        );
        // An export to a folder needs no stream, so an older owner still serves it.
        as_controller(
            MethodName::ExportsCreate,
            with_device(OWNER, json!({"destination": "/out", "computerIds": ["c"]})),
        )
        .await
        .unwrap();
        assert_eq!(seen.lock().unwrap().len(), 1);
        drop(old);
        let _new = crate::methods::checkpoints::export_transfer::install_test_link(Arc::new(
            Capable(vec!["handshake", "exports.file"]),
        ));
        as_controller(MethodName::ExportsCreate, create)
            .await
            .unwrap();
        let sent = seen.lock().unwrap().last().cloned().unwrap();
        assert_eq!(sent.0, "exports.create");
        assert_eq!(sent.1["transferArea"], true);
        assert!(sent.1.get("deviceId").is_none());
    }

    #[tokio::test]
    async fn a_request_for_another_device_goes_to_its_owner_without_the_device_id() {
        const OWNER: &str = "88888888-8888-4888-8888-888888888810";
        let (seen, _owner) = canned_owner(OWNER, |_, _| Ok(json!({ "jobId": "job:7" })));
        let started = as_controller(
            MethodName::ComputersStart,
            with_device(OWNER, json!({ "computerId": COMPUTER })),
        )
        .await
        .unwrap();
        assert_eq!(started, json!({ "jobId": "job:7" }));
        // The owner is asked in the canonical name, about its own computer, and is never
        // told which device was meant.
        assert_eq!(
            *seen.lock().unwrap(),
            vec![(
                "computers.start".to_owned(),
                json!({ "computerId": COMPUTER })
            )]
        );
    }

    #[tokio::test]
    async fn every_flattened_shape_reaches_the_owner() {
        const OWNER: &str = "88888888-8888-4888-8888-888888888811";
        let (seen, _owner) = canned_owner(OWNER, |method, _| match method {
            "computers.create" => Ok(json!({ "jobId": "setup:r1" })),
            "computers.delete" | "checkpoints.restore" => Ok(json!({ "jobId": "job:9" })),
            "computers.dismissError" => Ok(Value::Null),
            other => panic!("unexpected owner request {other}"),
        });
        let configuration = json!({
            "id": COMPUTER, "name": "dev", "cpus": 2, "maxCPUs": 4, "memoryGiB": 4,
            "maxMemoryGiB": 8, "workspaceStorageGiB": 20, "runtimeStorageGiB": 20
        });
        let created = as_controller(
            MethodName::ComputersCreate,
            with_device(
                OWNER,
                json!({ "kind": "linux", "configuration": configuration, "requestId": "r1" }),
            ),
        )
        .await
        .unwrap();
        assert_eq!(created["jobId"], "setup:r1");
        as_controller(
            MethodName::ComputersDelete,
            with_device(
                OWNER,
                json!({ "computerId": COMPUTER, "expected": configuration, "requestId": "r2" }),
            ),
        )
        .await
        .unwrap();
        as_controller(
            MethodName::CheckpointsRestore,
            with_device(
                OWNER,
                json!({ "computerId": COMPUTER, "checkpointId": "c1" }),
            ),
        )
        .await
        .unwrap();
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 3);
        for (_, params) in seen.iter() {
            assert!(params.get("deviceId").is_none(), "{params}");
        }
        assert_eq!(seen[0].1["kind"], "linux");
        assert_eq!(seen[1].1["requestId"], "r2");
    }

    #[tokio::test]
    async fn this_engines_own_device_is_not_forwarded() {
        const OWNER: &str = "88888888-8888-4888-8888-888888888812";
        let (seen, _owner) = canned_owner(OWNER, |_, _| panic!("a local request must not leave"));
        for device in ["local", LOCAL_DEVICE] {
            let read = as_controller(
                MethodName::ComputersConfigurationRead,
                json!({ "deviceId": device }),
            )
            .await
            .expect("a request for this engine runs here");
            assert!(read["computers"].is_array(), "{read}");
        }
        assert!(seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_malformed_request_is_refused_before_any_round_trip() {
        const OWNER: &str = "88888888-8888-4888-8888-888888888813";
        let (seen, _owner) = canned_owner(OWNER, |_, _| panic!("nothing should be sent"));
        let error = as_controller(MethodName::ComputersStart, with_device(OWNER, json!({})))
            .await
            .unwrap_err();
        assert!(error.message.starts_with("Invalid params"), "{error:?}");
        assert!(seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn an_owner_reply_in_the_wrong_shape_is_not_a_result() {
        const OWNER: &str = "88888888-8888-4888-8888-888888888814";
        let (_, _owner) = canned_owner(OWNER, |_, _| Ok(json!({ "unexpected": true })));
        let error = as_controller(
            MethodName::ComputersStart,
            with_device(OWNER, json!({ "computerId": COMPUTER })),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::Internal);
        assert!(
            error.message.contains("invalid reply to computers.start"),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn the_owners_error_codes_survive_the_trip() {
        const OWNER: &str = "88888888-8888-4888-8888-888888888815";
        let (_, _owner) = canned_owner(OWNER, |method, _| match method {
            "computers.start" => Err(BridgeError::new(
                BridgeCode::Internal,
                "Start the computer first.",
            )
            .with_api_code("not_running")),
            _ => Err(BridgeError::new(BridgeCode::NotFound, "gone")),
        });
        let running = as_controller(
            MethodName::ComputersStart,
            with_device(OWNER, json!({ "computerId": COMPUTER })),
        )
        .await
        .unwrap_err();
        assert_eq!(running.code, ErrorCode::NotRunning);
        assert_eq!(running.message, "Start the computer first.");
        let missing = as_controller(
            MethodName::ComputersStop,
            with_device(OWNER, json!({ "computerId": COMPUTER })),
        )
        .await
        .unwrap_err();
        assert_eq!(missing.code, ErrorCode::NotFound);
    }

    #[tokio::test]
    async fn a_device_that_is_not_connected_is_named_as_such() {
        let error = as_controller(
            MethodName::LogsQuery,
            json!({ "computerId": COMPUTER, "deviceId": THIRD }),
        )
        .await
        .unwrap_err();
        assert!(error.message.contains(NOT_CONNECTED), "{error:?}");
    }

    #[tokio::test]
    async fn the_owner_answers_with_its_own_computers_through_the_real_handlers() {
        const OWNER: &str = "88888888-8888-4888-8888-888888888816";
        let (seen, _owner) = engine_as_owner(OWNER);
        let read = as_controller(
            MethodName::ComputersConfigurationRead,
            with_device(OWNER, json!({})),
        )
        .await
        .unwrap();
        let computers = read["computers"].as_array().expect("the owner's computers");
        assert!(
            computers.iter().any(|computer| computer["id"] == COMPUTER),
            "{read}"
        );
        assert_eq!(seen.lock().unwrap()[0].0, "computers.configuration.read");

        // A change the owner refuses comes back with the owner's reason and code.
        let refused = as_controller(
            MethodName::ComputersStart,
            with_device(
                OWNER,
                json!({ "computerId": "99999999-9999-4999-8999-999999999999" }),
            ),
        )
        .await
        .unwrap_err();
        assert_eq!(refused.code, ErrorCode::NotFound, "{refused:?}");
    }

    #[tokio::test]
    async fn the_owner_lists_its_computers_for_a_controller_that_has_no_subscription_there() {
        const OWNER: &str = "88888888-8888-4888-8888-888888888817";
        let (_, _owner) = engine_as_owner(OWNER);
        // The fixture has no bundled runtime, so the listing itself fails; what matters is
        // that it got as far as the runtime: the barrier a socket client would hold is not
        // asked of the owner's bridge.
        let error = as_controller(MethodName::ComputersList, with_device(OWNER, json!({})))
            .await
            .unwrap_err();
        assert!(error.message.contains("MicroSandbox"), "{error:?}");
    }

    #[tokio::test]
    async fn the_owner_never_passes_a_request_on_to_a_third_device() {
        let engine = fixture().context.clone();
        let error = tokio::task::spawn_blocking(move || {
            serve_owner(
                &engine,
                "computers.start",
                &json!({ "computerId": COMPUTER, "deviceId": THIRD }),
                None,
            )
        })
        .await
        .unwrap()
        .unwrap_err();
        assert_eq!(error.code, BridgeCode::UnsupportedRemoteOperation);
    }

    #[tokio::test]
    async fn the_owner_serves_only_the_listed_methods() {
        let engine = fixture().context.clone();
        for method in [
            "settings.update",
            "connections.devices.remove",
            "computers.shell",
            "nonsense",
        ] {
            let engine = engine.clone();
            let error =
                tokio::task::spawn_blocking(move || serve_owner(&engine, method, &json!({}), None))
                    .await
                    .unwrap()
                    .unwrap_err();
            assert_eq!(
                error.code,
                BridgeCode::UnsupportedRemoteOperation,
                "{method}"
            );
        }
    }

    #[tokio::test]
    async fn the_owner_checks_a_request_before_running_it() {
        let engine = fixture().context.clone();
        let error = tokio::task::spawn_blocking(move || {
            serve_owner(&engine, "computers.start", &json!({ "name": "dev" }), None)
        })
        .await
        .unwrap()
        .unwrap_err();
        assert!(error.message.starts_with("Invalid params"), "{error:?}");
    }

    #[tokio::test]
    async fn another_devices_notices_are_the_owners() {
        const OWNER: &str = "88888888-8888-4888-8888-888888888818";
        let (seen, _owner) = engine_as_owner(OWNER);
        let notices = crate::methods::remote::route(
            MethodName::NotificationsList,
            &with_device(OWNER, json!({})),
            &request_context(),
        )
        .await
        .expect("a remote request is routed")
        .unwrap();
        assert!(notices.value.is_array(), "{:?}", notices.value);
        assert_eq!(seen.lock().unwrap()[0].0, "notifications.list");
    }

    fn fold_request(id: &str) -> Value {
        json!({
            "id": id, "forkId": "fork-1", "target": COMPUTER, "createdAt": "2026-11-06T10:00:00Z",
            "reviewDigest": "d1", "summary": "two files", "status": "open", "counts": {}
        })
    }

    #[tokio::test]
    async fn every_v1_owner_method_is_routed_to_the_owner_without_the_device_id() {
        const OWNER: &str = "88888888-8888-4888-8888-888888888820";
        let step = json!({
            "id": "s1", "computerId": COMPUTER, "number": 1, "command": "ls",
            "startedAt": "2026-11-06T10:00:00Z", "finishedAt": "2026-11-06T10:00:01Z"
        });
        let request = fold_request("req-1");
        let (seen, _owner) = canned_owner(OWNER, {
            let (step, request) = (step.clone(), request.clone());
            move |method, _| {
                Ok(match method {
                    "changes.request" => json!({ "request": request }),
                    "changes.requests.list" => json!({ "requests": [] }),
                    "changes.requests.markKept" => json!({ "requests": [request] }),
                    "computers.tree" => json!({ "nodes": [] }),
                    "computers.recipe" => json!({}),
                    "recipes.get" | "recipes.save" => json!({
                        "name": "r", "builtIn": false, "hash": "h",
                        "recipe": { "spec": { "os": "linux", "packages": {} } }
                    }),
                    "recipes.list" => json!({ "recipes": [] }),
                    "recipes.remove" => Value::Null,
                    "steps.list" => json!({ "steps": [] }),
                    "steps.record" => json!({ "step": step }),
                    _ => json!({ "jobId": "job:1" }),
                })
            }
        });
        let record = json!({
            "computerId": COMPUTER, "command": ["ls"],
            "startedAt": "2026-11-06T10:00:00Z", "finishedAt": "2026-11-06T10:00:01Z"
        });
        let fork = json!({ "computerId": COMPUTER, "checkpointId": "c", "newName": "n" });
        let build = json!({ "name": "r", "spec": { "os": "linux" } });
        let by_request = json!({ "requestId": "r" });
        let by_computer = json!({ "computerId": COMPUTER });
        let by_fork = json!({ "forkId": "f" });
        let calls: Vec<(MethodName, &str, Value)> = vec![
            (MethodName::ChangesKeep, "changes.keep", by_fork.clone()),
            (
                MethodName::ChangesPromote,
                "changes.promote",
                by_fork.clone(),
            ),
            (MethodName::ChangesRequest, "changes.request", by_fork),
            (MethodName::ChangesUndo, "changes.undo", by_computer.clone()),
            (MethodName::ChangesDiff, "changes.diff", by_computer.clone()),
            (
                MethodName::ChangesRequestsList,
                "changes.requests.list",
                json!({}),
            ),
            (
                MethodName::ChangesRequestsShow,
                "changes.requests.show",
                by_request.clone(),
            ),
            (
                MethodName::ChangesRequestsKeep,
                "changes.requests.keep",
                by_request.clone(),
            ),
            (
                MethodName::ChangesRequestsMarkKept,
                "changes.requests.markKept",
                json!({ "forkId": "f", "target": "folder:/x" }),
            ),
            (
                MethodName::ChangesRequestsThrowAway,
                "changes.requests.throwAway",
                by_request.clone(),
            ),
            (
                MethodName::ChangesRequestsOpen,
                "changes.requests.open",
                by_request,
            ),
            (MethodName::StepsList, "steps.list", by_computer),
            (MethodName::StepsRecord, "steps.record", record),
            (MethodName::ComputersTree, "computers.tree", json!({})),
            (MethodName::ComputersRecipe, "computers.recipe", json!({ "computerId": COMPUTER })),
            (MethodName::RecipesGet, "recipes.get", json!({ "name": "r" })),
            (
                MethodName::RecipesSave,
                "recipes.save",
                json!({ "name": "r", "recipe": { "spec": { "os": "linux" } } }),
            ),
            (MethodName::CheckpointsFork, "checkpoints.fork", fork),
            (MethodName::RecipesBuild, "recipes.build", build),
            (MethodName::RecipesList, "recipes.list", json!({})),
            (
                MethodName::RecipesRemove,
                "recipes.remove",
                json!({ "name": "r" }),
            ),
        ];
        for (method, name, params) in &calls {
            as_controller(*method, with_device(OWNER, params.clone()))
                .await
                .unwrap_or_else(|error| panic!("{name}: {error:?}"));
            let seen = seen.lock().unwrap();
            let (asked, sent) = seen.last().unwrap();
            assert_eq!(asked, name);
            assert!(sent.get("deviceId").is_none(), "{name}: {sent}");
            assert_eq!(sent, params, "{name}");
        }
        assert_eq!(seen.lock().unwrap().len(), calls.len());
    }

    /// Serializes the tests that set the connected devices a merged list asks.
    static PEERS: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn set_peers(peers: Option<&[(&str, &str)]>) {
        *crate::methods::checkpoints::all_devices::TEST_PEERS
            .lock()
            .unwrap() = peers.map(|peers| {
            peers
                .iter()
                .map(|(id, name)| ((*id).to_owned(), (*name).to_owned()))
                .collect()
        });
    }

    #[tokio::test]
    async fn requests_of_every_device_are_one_list_and_an_unreachable_device_is_reported() {
        const MINI: &str = "88888888-8888-4888-8888-888888888821";
        const DOWN: &str = "88888888-8888-4888-8888-888888888822";
        let _peers = PEERS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let entry = json!({ "request": fold_request("req-mini"), "forkName": "try", "forkState": "running" });
        let (seen, _mini) = canned_owner(MINI, move |method, _| match method {
            "changes.requests.list" => Ok(json!({ "requests": [entry.clone()] })),
            "changes.requests.throwAway" => Ok(json!({ "jobId": "job:5" })),
            other => panic!("unexpected owner request {other}"),
        });
        let _down = stub_owner(DOWN, |_, _| {
            Err(BridgeError::from("This device is no longer connected."))
        });
        set_peers(Some(&[(MINI, "Mac mini"), (DOWN, "Old laptop")]));

        // The local list is the engine's own; its computers are not read in this test.
        let local = serde_json::from_value(json!({ "requests": [
            { "request": fold_request("req-local") }
        ] }))
        .unwrap();
        let listed = serde_json::to_value(
            crate::methods::checkpoints::all_devices::merge(
                &request_context(),
                &json!({ "allDevices": true, "status": "all" }),
                local,
            )
            .await,
        )
        .unwrap();
        let rows = listed["requests"].as_array().unwrap();
        let mini: Vec<_> = rows.iter().filter(|row| row["deviceId"] == MINI).collect();
        assert_eq!(rows.len(), 2, "{listed}");
        let own = rows.iter().find(|row| row["request"]["id"] == "req-local");
        assert_eq!(own.unwrap()["deviceId"], LOCAL_DEVICE, "{listed}");
        assert_eq!(mini.len(), 1, "{listed}");
        assert_eq!(mini[0]["deviceName"], "Mac mini");
        assert_eq!(mini[0]["request"]["id"], "req-mini");
        assert_eq!(
            listed["unreachable"].as_array().unwrap().len(),
            1,
            "{listed}"
        );
        assert_eq!(listed["unreachable"][0]["deviceId"], DOWN);
        assert_eq!(listed["unreachable"][0]["deviceName"], "Old laptop");
        // The owner is asked for its own requests only, with the same filter.
        assert_eq!(
            seen.lock().unwrap()[0],
            (
                "changes.requests.list".to_owned(),
                json!({ "status": "all" })
            )
        );

        // Acting on the row goes to the row's device, which is told nothing of devices.
        let thrown = as_controller(
            MethodName::ChangesRequestsThrowAway,
            json!({ "requestId": mini[0]["request"]["id"], "deviceId": mini[0]["deviceId"] }),
        )
        .await
        .unwrap();
        assert_eq!(thrown["jobId"], "job:5");
        assert_eq!(
            seen.lock().unwrap()[1],
            (
                "changes.requests.throwAway".to_owned(),
                json!({ "requestId": "req-mini" })
            )
        );
        set_peers(None);
    }

    #[tokio::test]
    async fn a_list_across_devices_with_every_device_down_is_still_the_local_list() {
        const DOWN: &str = "88888888-8888-4888-8888-888888888823";
        let _peers = PEERS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _down = stub_owner(DOWN, |_, _| Err(BridgeError::from("Unreachable.")));
        set_peers(Some(&[(DOWN, "Old laptop")]));
        let listed = serde_json::to_value(
            crate::methods::checkpoints::all_devices::merge(
                &request_context(),
                &json!({ "allDevices": true }),
                serde_json::from_value(json!({ "requests": [] })).unwrap(),
            )
            .await,
        )
        .unwrap();
        assert!(
            listed["requests"].as_array().unwrap().is_empty(),
            "{listed}"
        );
        assert_eq!(listed["unreachable"][0]["deviceId"], DOWN);
        set_peers(None);
    }

    #[tokio::test]
    async fn an_owner_never_asks_its_own_devices_for_requests() {
        const PEER: &str = "88888888-8888-4888-8888-888888888840";
        let _peers = PEERS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (seen, _peer) = canned_owner(PEER, |_, _| Ok(json!({ "requests": [] })));
        set_peers(Some(&[(PEER, "Third")]));
        // The bridge's own context merges nothing.
        let bridge = HandlerContext::for_owner_bridge(
            fixture().context.clone(),
            RequestContext {
                client_id: OWNER_CLIENT.to_owned(),
                protocol: PROTOCOL_VERSION,
                client: None,
                caller: silo_engine::policy::Caller::owner("test"),
            },
        );
        let merged = crate::methods::checkpoints::all_devices::merge(
            &bridge,
            &json!({ "allDevices": true }),
            serde_json::from_value(json!({ "requests": [] })).unwrap(),
        )
        .await;
        assert!(merged.unreachable.is_empty());
        // And `serve_owner` drops the option before running the handler.
        let engine = fixture().context.clone();
        let _ = tokio::task::spawn_blocking(move || {
            serve_owner(
                &engine,
                "changes.requests.list",
                &json!({ "allDevices": true }),
                None,
            )
        })
        .await
        .unwrap();
        set_peers(None);
        assert!(seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_device_that_does_not_answer_in_time_is_unreachable() {
        const SLOW: &str = "88888888-8888-4888-8888-888888888842";
        let _peers = PEERS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _slow = stub_owner(SLOW, |_, _| {
            std::thread::sleep(std::time::Duration::from_millis(600));
            Ok(json!({ "requests": [] }))
        });
        set_peers(Some(&[(SLOW, "Slow Mac")]));
        *crate::methods::checkpoints::all_devices::TEST_TIMEOUT
            .lock()
            .unwrap() = Some(std::time::Duration::from_millis(100));
        let merged = crate::methods::checkpoints::all_devices::merge(
            &request_context(),
            &json!({ "allDevices": true }),
            serde_json::from_value(json!({ "requests": [] })).unwrap(),
        )
        .await;
        *crate::methods::checkpoints::all_devices::TEST_TIMEOUT
            .lock()
            .unwrap() = None;
        set_peers(None);
        assert_eq!(merged.unreachable.len(), 1);
        assert_eq!(merged.unreachable[0].device_id, SLOW);
        assert!(merged.unreachable[0].reason.contains("in time"));
    }

    #[tokio::test]
    async fn routed_reads_run_the_owners_real_handlers() {
        const OWNER: &str = "88888888-8888-4888-8888-888888888841";
        let (seen, _owner) = engine_as_owner(OWNER);
        let recipes = as_controller(MethodName::RecipesList, with_device(OWNER, json!({})))
            .await
            .unwrap();
        assert!(recipes["recipes"].is_array(), "{recipes}");
        let steps = as_controller(
            MethodName::StepsList,
            with_device(OWNER, json!({ "computerId": COMPUTER })),
        )
        .await
        .unwrap();
        assert!(steps["steps"].is_array(), "{steps}");
        let seen = seen.lock().unwrap();
        assert_eq!(seen[0].0, "recipes.list");
        assert_eq!(seen[1].0, "steps.list");
        assert!(seen
            .iter()
            .all(|(_, params)| params.get("deviceId").is_none()));
    }

    #[tokio::test]
    async fn a_v1_handler_reached_with_another_device_is_refused() {
        let request = request_context();
        for (method, params) in [
            (MethodName::StepsList, json!({ "computerId": COMPUTER })),
            (MethodName::RecipesList, json!({})),
            (MethodName::ChangesUndo, json!({ "computerId": COMPUTER })),
            (MethodName::ChangesRequestsList, json!({})),
        ] {
            let mut params = params;
            params["deviceId"] = json!(THIRD);
            let error = crate::methods::checkpoints::handle(method, &params, &request)
                .await
                .expect("owned")
                .unwrap_err();
            assert_eq!(
                error.code,
                ErrorCode::UnsupportedRemoteOperation,
                "{method:?}"
            );
        }
    }

    /// Whether the canonical params of `method` have a top-level `deviceId`.
    fn names_a_device(schema: &Value, method: &str) -> bool {
        fn properties(schema: &Value, node: &Value, found: &mut BTreeSet<String>) {
            if let Some(reference) = node.get("$ref").and_then(Value::as_str) {
                let name = reference.rsplit('/').next().unwrap();
                properties(schema, &schema["$defs"][name], found);
                return;
            }
            if let Some(props) = node.get("properties").and_then(Value::as_object) {
                found.extend(props.keys().cloned());
            }
            for key in ["oneOf", "anyOf", "allOf"] {
                for variant in node
                    .get(key)
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    properties(schema, variant, found);
                }
            }
        }
        let variants = schema["$defs"]["MethodParams"]["oneOf"].as_array().unwrap();
        let entry = variants
            .iter()
            .find(|entry| entry["properties"]["method"]["const"] == method)
            .unwrap_or_else(|| panic!("{method} is in the registry"));
        let mut found = BTreeSet::new();
        properties(schema, &entry["properties"]["params"], &mut found);
        found.contains("deviceId")
    }

    /// Methods with their own remote routing, and the reason they are not in the owner list.
    const OWN_ROUTING: &[&str] = &[
        // Jobs and the queue belong to the engine that runs the work.
        "operations.cancel",
        "operations.job.acknowledge",
        "operations.job.get",
        "operations.list",
        // Credentials stay on their owner; each answer is reshaped for the controller.
        "github.access.set",
        "github.configuration.retry",
        "github.configuration.save",
        "github.connect",
        "github.connect.callback",
        "github.connect.reopen",
        "github.disconnect",
        "github.personalToken.remove",
        "github.personalToken.save",
        "github.repositories.manage",
        "github.repositories.refresh",
        "github.state",
        "secrets.list",
        "secrets.remove",
        "secrets.retry",
        "secrets.save",
        // Pushes, published ports and managed SSH access carry controller-side transport.
        "network.open",
        "network.publish",
        "network.state",
        "network.unpublish",
        "network.urls",
        "push.dismiss",
        "push.repositories",
        "push.start",
        "push.status",
        "sshAccess.connection",
        "sshAccess.save",
        "sshAccess.state",
        // Desktop and computer use are the owner's jobs through `desktop.job`.
        "computerUse.approval",
        "computerUse.setup",
        "computerUse.state",
        "desktop.action",
        "desktop.status",
        // Remote commands are the owner's jobs, followed by `(deviceId, jobId)`.
        "guest.exec.get",
        "guest.exec.input",
        "guest.exec.start",
        // The viewer connection and the display session are the controller's.
        "display.viewer.open",
        "macos.display.resize",
        "macos.display.session",
        // One file is written here, from pages read from each owner.
        "logs.export",
        // The owner's own key revocation, sent by the controller when a device is removed.
        "connections.keys.revoke",
        // The controller's own half (its SSH configuration, files and subscription), then
        // the owner's `guest.account`, `guest.prepare`, `guest.ssh` or `guest.files.watch`.
        "editor.command",
        "terminal.command",
        "guest.sshAlias.ensure",
        "guest.sshAlias.remove",
        "files.upload",
        "files.download",
        "guest.files.watch",
        // The controller's copy of an export file over the owner's `exports.file` stream.
        "exports.transfer",
    ];

    /// Methods whose `deviceId` names the device itself, not where a computer lives.
    const DEVICE_IS_THE_SUBJECT: &[&str] = &["connections.devices.remove"];

    /// Methods that take a `deviceId` and refuse another device, with the reason.
    const REFUSES_REMOTE: &[&str] = &[
        // The local window and viewer connections of this engine's clients.
        "display.show",
        "display.close",
        "clipboard.transfer",
        // A computer's network policy is set on the device that runs it.
        "network.policy.get",
        "network.policy.set",
    ];

    #[test]
    fn every_method_is_remote_by_the_owner_by_its_own_route_refused_or_local_by_design() {
        let schema: Value = serde_json::from_str(&silo_api::schema_json().unwrap()).unwrap();
        let owner: BTreeSet<&str> = silo_engine::remote::OWNER_METHODS
            .iter()
            .map(|(name, _)| *name)
            .collect();
        let own: BTreeSet<&str> = OWN_ROUTING.iter().copied().collect();
        let refused: BTreeSet<&str> = REFUSES_REMOTE.iter().copied().collect();
        for (set, label) in [
            (&owner, "owner"),
            (&own, "own routing"),
            (&refused, "refused"),
        ] {
            for name in set {
                assert!(
                    method_registry()
                        .iter()
                        .any(|descriptor| descriptor.name == *name),
                    "{label}: {name} is not a method"
                );
            }
        }
        assert_eq!(owner.intersection(&own).count(), 0);
        assert_eq!(owner.intersection(&refused).count(), 0);
        assert_eq!(own.intersection(&refused).count(), 0);

        for descriptor in method_registry() {
            let name = descriptor.name;
            let takes_device = names_a_device(&schema, name);
            let listed = owner.contains(name)
                || own.contains(name)
                || refused.contains(name)
                || DEVICE_IS_THE_SUBJECT.contains(&name);
            if listed {
                // `logs.export` names its devices in each of its `requests`.
                assert!(
                    takes_device || name == "logs.export",
                    "{name} is routed by device but has no deviceId"
                );
            } else {
                // What is left is this device's own: no per-computer method may be missing.
                assert!(
                    !takes_device,
                    "{name} has a deviceId but is not classified as remote or refused"
                );
            }
        }
    }

    #[test]
    fn the_owner_list_agrees_with_the_registry_kinds() {
        let engine_owner = silo_engine::remote::OWNER_METHODS;
        for (name, changes) in engine_owner {
            let descriptor = method_registry()
                .iter()
                .find(|descriptor| descriptor.name == *name)
                .unwrap_or_else(|| panic!("{name} is a method"));
            let is_read = descriptor.kind == MethodKind::Query;
            // `clipboard.read` is typed as a command and only observes.
            let observes_only = *name == "clipboard.read";
            assert_eq!(
                !*changes,
                is_read || observes_only,
                "{name} is classified against its kind {:?}",
                descriptor.kind
            );
            assert!(
                !matches!(
                    descriptor.kind,
                    MethodKind::Stream | MethodKind::Subscription
                ),
                "{name}: a stream is not bridged as a request"
            );
        }
    }

    #[test]
    fn every_owner_method_is_a_method_a_domain_or_the_notice_list_answers() {
        for (name, _) in silo_engine::remote::OWNER_METHODS {
            let method = method_by_name(name).expect("registry method");
            let owners = [
                super::super::computers::owns as fn(MethodName) -> bool,
                super::super::checkpoints::owns,
                super::super::guest::owns,
                super::super::access::owns,
                super::super::display::owns,
            ]
            .iter()
            .filter(|owns| owns(method))
            .count();
            let notice_list = method == MethodName::NotificationsList;
            assert!(
                owners == 1 || notice_list,
                "{name} would be answered by {owners} domains"
            );
        }
    }
}
