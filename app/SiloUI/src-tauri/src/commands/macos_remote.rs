//! macOS computers on a connected device, through the computer methods with a `deviceId`.
//! The local engine routes them to the device that hosts the virtual machines.
use crate::commands::macos_computers::{change_resources, Action};
use crate::engine_client::{
    params::{decode, target},
    Engine, EngineClient, EngineError,
};
use serde_json::Value;
use silo_api::methods::{
    computers::{ComputerCreateParams, ComputerDeleteParams, ComputerListParams},
    requests,
};
use tauri::Window;

// Still read by `macos_remote_viewer.rs`, which opens the display stream itself.

fn require_main(window: &Window) -> Result<(), EngineError> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err(EngineError::internal(
            "Manage computers from the main Silo window.",
        ))
    }
}

/// The macOS computers of a connected device, and whether that device can host them.
pub(crate) async fn snapshot(engine: &EngineClient, device_id: &str) -> Result<Value, EngineError> {
    let listed = engine
        .call(requests::computers_list(ComputerListParams {
            device_id: Some(device_id.to_owned()),
            refresh_repositories: None,
        }))
        .await?;
    listed
        .get("macos")
        .cloned()
        .ok_or_else(|| EngineError::internal("The device returned no macOS computer state."))
}

/// Creates a macOS computer on a connected device and returns its new row. The setup goes
/// on in the engine.
pub(crate) async fn create(
    engine: &EngineClient,
    device_id: &str,
    request: Value,
) -> Result<Value, EngineError> {
    let created = engine
        .call(requests::computers_create(ComputerCreateParams::Macos {
            request: decode(request, "macOS computer request")?,
            device_id: Some(device_id.to_owned()),
            agents: Vec::new(),
            network_policy: None,
        }))
        .await?;
    created
        .get("computer")
        .filter(|computer| !computer.is_null())
        .cloned()
        .ok_or_else(|| EngineError::internal("The device did not return the new computer."))
}

/// Starts, stops, force stops, deletes, sets up or changes the CPUs and memory of a macOS
/// computer on a connected device, and returns that device's macOS state afterwards.
pub(crate) async fn action(
    engine: &EngineClient,
    device_id: &str,
    computer_id: &str,
    action: Action,
) -> Result<Value, EngineError> {
    let params = target(computer_id, Some(device_id));
    match action {
        Action::Start => {
            engine.run_job(requests::computers_start(params)).await?;
        }
        Action::Stop => {
            engine.run_job(requests::computers_stop(params)).await?;
        }
        Action::Delete => {
            engine
                .run_job(requests::computers_delete(ComputerDeleteParams {
                    computer_id: params.computer_id,
                    device_id: params.device_id,
                    expected: None,
                    request_id: None,
                }))
                .await?;
        }
        Action::Setup => {
            engine.run_job(requests::macos_setup_retry(params)).await?;
        }
        Action::ForceStop => {
            engine.call(requests::computers_force_stop(params)).await?;
        }
        Action::ChangeResources {
            cpus,
            memory_gib,
            expected,
        } => {
            change_resources(
                engine,
                computer_id,
                Some(device_id),
                cpus,
                memory_gib,
                expected,
            )
            .await?;
        }
    }
    snapshot(engine, device_id).await
}

/// The macOS computers of a connected device, and whether that device can host them.
#[tauri::command]
pub(crate) async fn remote_macos_snapshot(
    engine: Engine<'_>,
    window: Window,
    device_id: String,
) -> Result<Value, EngineError> {
    require_main(&window)?;
    snapshot(&engine, &device_id).await
}

/// Creates a macOS computer on a connected device.
#[tauri::command]
pub(crate) async fn remote_macos_create(
    engine: Engine<'_>,
    window: Window,
    device_id: String,
    request: Value,
) -> Result<Value, EngineError> {
    require_main(&window)?;
    create(&engine, &device_id, request).await
}

/// Starts, stops, force stops, deletes, sets up or resizes a macOS computer on a connected
/// device. `action` takes the same words and shape as `macos_computer_action`.
#[tauri::command]
pub(crate) async fn remote_macos_action(
    engine: Engine<'_>,
    window: Window,
    device_id: String,
    computer_id: String,
    action: Value,
) -> Result<Value, EngineError> {
    require_main(&window)?;
    let action = decode(action, "macOS computer action")?;
    self::action(&engine, &device_id, &computer_id, action).await
}

#[cfg(test)]
mod forwarding_tests {
    use super::*;
    use crate::engine_client::testing::{connected_client, FakeEngine};
    use serde_json::json;
    use silo_api::methods::MethodName;

    const DEVICE: &str = "00000000-0000-4000-8000-0000000000d1";
    const MAC: &str = "00000000-0000-4000-8000-0000000000a1";

    fn macos_state() -> Value {
        json!({"computers": [{"id": MAC, "name": "sonoma"}], "canHost": true})
    }

    fn replies(method: MethodName, _params: &Value) -> Result<Value, (&'static str, String)> {
        match method {
            MethodName::EventsSubscribe => Ok(json!({"subscription": "sub"})),
            MethodName::ComputersList => {
                Ok(json!({"linux": {"computers": []}, "macos": macos_state()}))
            }
            MethodName::ComputersCreate => {
                Ok(json!({"jobId": "job:2", "computer": {"id": MAC, "name": "sonoma"}}))
            }
            MethodName::ComputersStart
            | MethodName::ComputersStop
            | MethodName::ComputersDelete
            | MethodName::MacosSetupRetry
            | MethodName::ComputersConfigurationChange => Ok(json!({"jobId": "job:1"})),
            MethodName::OperationsJobGet => {
                Ok(json!({"jobId": "job:1", "state": "finished", "outcome": {"result": null}}))
            }
            _ => Ok(Value::Null),
        }
    }

    #[tokio::test]
    async fn the_snapshot_is_the_macos_part_of_the_devices_list() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        assert_eq!(snapshot(&engine, DEVICE).await.unwrap(), macos_state());
        assert_eq!(
            fake.calls(MethodName::ComputersList),
            vec![json!({"deviceId": DEVICE})]
        );
    }

    #[tokio::test]
    async fn creating_returns_the_new_row_without_waiting_for_the_setup() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        let request = json!({"name": "sonoma", "cpus": 4, "memoryGiB": 8, "diskGiB": 64});
        let row = create(&engine, DEVICE, request.clone()).await.unwrap();
        assert_eq!(row["id"], MAC);
        assert_eq!(
            fake.calls(MethodName::ComputersCreate),
            vec![json!({"kind": "macos", "request": request, "deviceId": DEVICE})]
        );
        assert!(fake.calls(MethodName::OperationsJobGet).is_empty());
    }

    #[tokio::test]
    async fn each_action_maps_to_its_method_and_returns_the_state() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        for name in ["start", "stop", "delete", "setup", "force-stop"] {
            let parsed: Action = serde_json::from_value(json!(name)).unwrap();
            assert_eq!(
                action(&engine, DEVICE, MAC, parsed).await.unwrap(),
                macos_state()
            );
        }
        let target = json!({"computerId": MAC, "deviceId": DEVICE});
        for method in [
            MethodName::ComputersStart,
            MethodName::ComputersStop,
            MethodName::ComputersDelete,
            MethodName::MacosSetupRetry,
            MethodName::ComputersForceStop,
        ] {
            assert_eq!(
                fake.calls(method),
                vec![target.clone()],
                "{}",
                method.name()
            );
        }
        assert!(decode::<Action>(json!("explode"), "macOS computer action").is_err());
    }

    #[tokio::test]
    async fn a_resource_change_is_sent_to_the_device_that_hosts_the_computer() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        // The shape the CPU and memory form sends, as `macos_computer_action` takes it.
        let parsed = decode::<Action>(
            json!({"change-resources": {"cpus": 6, "memoryGiB": 12,
                "expected": {"cpus": 4, "memoryGiB": 8, "diskGiB": 64}}}),
            "macOS computer action",
        )
        .unwrap();
        assert_eq!(
            action(&engine, DEVICE, MAC, parsed).await.unwrap(),
            macos_state()
        );
        let sent = &fake.calls(MethodName::ComputersConfigurationChange)[0];
        assert_eq!(sent["deviceId"], DEVICE);
        assert_eq!(sent["change"]["kind"], "macosResources");
        assert_eq!(sent["change"]["computerId"], MAC);
        assert_eq!(
            sent["change"]["expected"],
            json!({"cpus": 4, "memoryGiB": 8, "diskGiB": 64})
        );
        assert_eq!(
            (
                sent["change"]["cpus"].clone(),
                sent["change"]["memoryGiB"].clone()
            ),
            (json!(6), json!(12))
        );
        // The job is the owner's, so it is followed and acknowledged on that device.
        for method in [
            MethodName::OperationsJobGet,
            MethodName::OperationsJobAcknowledge,
        ] {
            let calls = fake.calls(method);
            assert!(!calls.is_empty(), "{}", method.name());
            assert!(
                calls.iter().all(|call| call["deviceId"] == DEVICE),
                "{}: {calls:?}",
                method.name()
            );
        }
    }

    #[tokio::test]
    async fn a_device_that_cannot_host_answers_with_the_typed_error() {
        let fake = FakeEngine::start(|method, params| match method {
            MethodName::ComputersCreate => Err((
                "unsupported_remote_operation",
                "That device cannot host macOS computers.".into(),
            )),
            _ => replies(method, params),
        });
        let (engine, _) = connected_client(&fake).await;
        let error = create(
            &engine,
            DEVICE,
            json!({"name": "m", "cpus": 4, "memoryGiB": 8, "diskGiB": 64}),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "unsupported_remote_operation");
    }
}
