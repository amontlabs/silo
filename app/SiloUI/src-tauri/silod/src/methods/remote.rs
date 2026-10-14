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
    }
    let reply = tokio::task::spawn_blocking(move || call_owner(&device, name, owner_params))
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
pub fn serve_owner(
    engine: &Arc<SilodContext>,
    name: &str,
    params: &Value,
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
        },
    );
    let params = params.clone();
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
            serve_owner(&fixture().context, method, params)
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
                tokio::task::spawn_blocking(move || serve_owner(&engine, method, &json!({})))
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
            serve_owner(&engine, "computers.start", &json!({ "name": "dev" }))
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
    ];

    /// Methods whose `deviceId` names the device itself, not where a computer lives.
    const DEVICE_IS_THE_SUBJECT: &[&str] = &["connections.devices.remove"];

    /// Methods that take a `deviceId` and refuse another device, with the reason.
    const REFUSES_REMOTE: &[&str] = &[
        // Local launchers and the controller's own SSH configuration.
        "editor.command",
        "terminal.command",
        "guest.prepare",
        "guest.sshAlias.ensure",
        "guest.sshAlias.remove",
        // The local window and viewer connections of this engine's clients.
        "display.show",
        "display.close",
        "clipboard.transfer",
        // Local file paths on the controller need a data transport between the engines.
        "files.upload",
        "files.download",
        // The change events of a watch are client-scoped and not relayed.
        "guest.files.watch",
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
