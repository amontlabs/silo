//! macOS computers on a connected device, through the computer methods with a `deviceId`.
//! The local engine routes them to the device that hosts the virtual machines.
use crate::engine_client::{
    forwarding::{not_found, with_device},
    Engine, EngineClient, EngineError,
};
use crate::macos_remote_engine as engine;
use serde_json::{json, Value};
use tauri::Window;

// Still read by `macos_remote_viewer.rs`, which opens the display stream itself.
pub(crate) const DISPLAY_STREAM: &str = engine::DISPLAY_STREAM;

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
        .call("computers.list", with_device(json!({}), Some(device_id)))
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
        .call(
            "computers.create",
            json!({"kind": "macos", "request": request, "deviceId": device_id}),
        )
        .await?;
    created
        .get("computer")
        .filter(|computer| !computer.is_null())
        .cloned()
        .ok_or_else(|| EngineError::internal("The device did not return the new computer."))
}

/// Starts, stops, force stops, deletes or sets up a macOS computer on a connected device,
/// and returns that device's macOS state afterwards.
pub(crate) async fn action(
    engine: &EngineClient,
    device_id: &str,
    computer_id: &str,
    action: &str,
) -> Result<Value, EngineError> {
    let params = json!({"computerId": computer_id, "deviceId": device_id});
    match action {
        "start" | "stop" => {
            engine
                .run_job(&format!("computers.{action}"), params)
                .await?;
        }
        "delete" => {
            engine.run_job("computers.delete", params).await?;
        }
        "setup" => {
            engine.run_job("macos.setup.retry", params).await?;
        }
        "force-stop" => {
            engine.call("computers.forceStop", params).await?;
        }
        _ => return Err(not_found("Silo does not know that macOS computer action.")),
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

/// Starts, stops, force stops, deletes or sets up a macOS computer on a connected device.
#[tauri::command]
pub(crate) async fn remote_macos_action(
    engine: Engine<'_>,
    window: Window,
    device_id: String,
    computer_id: String,
    action: String,
) -> Result<Value, EngineError> {
    require_main(&window)?;
    self::action(&engine, &device_id, &computer_id, &action).await
}

#[cfg(test)]
mod forwarding_tests {
    use super::*;
    use crate::engine_client::testing::{connected_client, FakeEngine};

    const DEVICE: &str = "00000000-0000-4000-8000-0000000000d1";
    const MAC: &str = "00000000-0000-4000-8000-0000000000a1";

    fn macos_state() -> Value {
        json!({"computers": [{"id": MAC, "name": "sonoma"}], "canHost": true})
    }

    fn replies(method: &str, _params: &Value) -> Result<Value, (&'static str, String)> {
        match method {
            "events.subscribe" => Ok(json!({"subscription": "sub"})),
            "computers.list" => Ok(json!({"linux": {"computers": []}, "macos": macos_state()})),
            "computers.create" => {
                Ok(json!({"jobId": "job:2", "computer": {"id": MAC, "name": "sonoma"}}))
            }
            "computers.start" | "computers.stop" | "computers.delete" | "macos.setup.retry" => {
                Ok(json!({"jobId": "job:1"}))
            }
            "operations.job.get" => {
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
            fake.calls("computers.list"),
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
            fake.calls("computers.create"),
            vec![json!({"kind": "macos", "request": request, "deviceId": DEVICE})]
        );
        assert!(fake.calls("operations.job.get").is_empty());
    }

    #[tokio::test]
    async fn each_action_maps_to_its_method_and_returns_the_state() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        for name in ["start", "stop", "delete", "setup", "force-stop"] {
            assert_eq!(
                action(&engine, DEVICE, MAC, name).await.unwrap(),
                macos_state()
            );
        }
        let target = json!({"computerId": MAC, "deviceId": DEVICE});
        for method in [
            "computers.start",
            "computers.stop",
            "computers.delete",
            "macos.setup.retry",
            "computers.forceStop",
        ] {
            assert_eq!(fake.calls(method), vec![target.clone()], "{method}");
        }
        assert!(action(&engine, DEVICE, MAC, "explode").await.is_err());
    }

    #[tokio::test]
    async fn a_device_that_cannot_host_answers_with_the_typed_error() {
        let fake = FakeEngine::start(|method, params| match method {
            "computers.create" => Err((
                "unsupported_remote_operation",
                "That device cannot host macOS computers.".into(),
            )),
            _ => replies(method, params),
        });
        let (engine, _) = connected_client(&fake).await;
        let error = create(&engine, DEVICE, json!({})).await.unwrap_err();
        assert_eq!(error.code, "unsupported_remote_operation");
    }
}
