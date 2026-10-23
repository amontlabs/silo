//! Tauri commands for macOS computers: each forwards to `silod`, which owns the VMs and
//! hosts their display windows.
use crate::engine_client::{
    params::{device, target},
    Engine, EngineClient, EngineError,
};
use serde::Deserialize;
use serde_json::Value;
use silo_api::methods::{
    checkpoints::{CheckpointCreateParams, CheckpointForkParams, CheckpointTargetParams},
    clipboard::{ClipboardAction, ClipboardTransferParams},
    computers::{
        ComputerConfigurationChange, ComputerConfigurationChangeParams, ComputerCreateParams,
        ComputerDeleteParams, ComputerListParams, MacosComputerCreateRequest,
        MacosResourceConfiguration,
    },
    kind, requests, Request,
};
use std::sync::Arc;
use tauri::Window;

fn require_main(window: &Window) -> Result<(), String> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err("Manage macOS computers from the main Silo window.".into())
    }
}

/// What the UI asks a macOS computer to do. The engine has one method for each.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case", rename_all_fields = "camelCase")]
pub(crate) enum Action {
    Start,
    Stop,
    ForceStop,
    Delete,
    Setup,
    ChangeResources {
        cpus: u64,
        #[serde(rename = "memoryGiB")]
        memory_gib: u64,
        expected: Expected,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Expected {
    pub cpus: u64,
    #[serde(rename = "memoryGiB")]
    pub memory_gib: u64,
    #[serde(rename = "diskGiB")]
    pub disk_gib: u64,
}

/// Which way an explicit clipboard transfer goes, as the UI names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub(crate) enum ClipboardDirection {
    #[serde(rename = "paste-into")]
    PasteInto,
    #[serde(rename = "copy-from")]
    CopyFrom,
}

impl ClipboardDirection {
    fn action(self) -> ClipboardAction {
        match self {
            Self::PasteInto => ClipboardAction::Paste,
            Self::CopyFrom => ClipboardAction::Copy,
        }
    }
}

/// The macOS part of the inventory, in the shape the section parses.
pub(crate) async fn read(engine: &EngineClient) -> Result<Value, EngineError> {
    let listed = engine
        .call(requests::computers_list(ComputerListParams {
            device_id: None,
            refresh_repositories: None,
        }))
        .await?;
    listed
        .get("macos")
        .cloned()
        .ok_or_else(|| EngineError::internal("The engine returned no macOS computer state."))
}

/// Creates a computer. The row comes back at once; the creation continues in the engine
/// and its failures are data in the row, so the job's outcome is only acknowledged.
pub(crate) async fn create(
    engine: &Arc<EngineClient>,
    request: Value,
) -> Result<Value, EngineError> {
    let request: MacosComputerCreateRequest =
        crate::engine_client::params::decode(request, "macOS computer request")?;
    let started = engine
        .call(requests::computers_create(ComputerCreateParams::Macos {
            request,
            device_id: None,
        }))
        .await?;
    acknowledge_in_background(engine, &started);
    started
        .get("computer")
        .cloned()
        .ok_or_else(|| EngineError::internal("The engine returned no computer."))
}

fn acknowledge_in_background(engine: &Arc<EngineClient>, started: &Value) {
    if let Some(job_id) = started.get("jobId").and_then(Value::as_str) {
        let (engine, job_id) = (engine.clone(), job_id.to_owned());
        tokio::spawn(async move {
            let _ = engine.wait_job(&job_id).await;
        });
    }
}

pub(crate) async fn act(
    engine: &Arc<EngineClient>,
    id: &str,
    action: Action,
) -> Result<(), EngineError> {
    let computer = target(id, None);
    match action {
        Action::Start => drop(engine.run_job(requests::computers_start(computer)).await?),
        Action::Stop => drop(engine.run_job(requests::computers_stop(computer)).await?),
        Action::Delete => drop(
            engine
                .run_job(requests::computers_delete(ComputerDeleteParams {
                    computer_id: computer.computer_id,
                    device_id: None,
                    expected: None,
                    request_id: None,
                }))
                .await?,
        ),
        Action::ForceStop => drop(
            engine
                .call(requests::computers_force_stop(computer))
                .await?,
        ),
        // Setup runs for as long as the installation steps need; the row shows it.
        Action::Setup => {
            let started = engine.call(requests::macos_setup_retry(computer)).await?;
            acknowledge_in_background(engine, &started);
        }
        Action::ChangeResources {
            cpus,
            memory_gib,
            expected,
        } => {
            let change = ComputerConfigurationChange::MacosResources {
                computer_id: id.to_owned(),
                expected: MacosResourceConfiguration {
                    cpus: expected.cpus,
                    memory_gib: expected.memory_gib,
                    disk_gib: expected.disk_gib,
                },
                cpus,
                memory_gib,
                request_id: uuid::Uuid::new_v4().to_string(),
            };
            engine
                .run_job(requests::computers_configuration_change(
                    ComputerConfigurationChangeParams {
                        change,
                        request_id: None,
                        retry_computer_id: None,
                        device_id: None,
                    },
                ))
                .await?;
        }
    }
    Ok(())
}

pub(crate) async fn open_display(engine: &EngineClient, id: &str) -> Result<(), EngineError> {
    engine
        .call(requests::display_show(target(id, None)))
        .await?;
    Ok(())
}

pub(crate) async fn clipboard(
    engine: &EngineClient,
    id: &str,
    direction: ClipboardDirection,
) -> Result<Value, EngineError> {
    engine
        .call(requests::clipboard_transfer(ClipboardTransferParams {
            computer_id: id.to_owned(),
            device_id: None,
            connection_id: None,
            action: direction.action(),
        }))
        .await
}

pub(crate) async fn delete_template(engine: &EngineClient) -> Result<(), EngineError> {
    engine
        .call(requests::macos_template_delete(device(None)))
        .await?;
    Ok(())
}

pub(crate) async fn checkpoint(
    engine: &EngineClient,
    request: Request<kind::Job>,
) -> Result<(), EngineError> {
    engine.run_job(request).await?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn read_macos_computers(
    engine: Engine<'_>,
    window: Window,
) -> Result<Value, String> {
    require_main(&window)?;
    read(&engine).await.map_err(String::from)
}

#[tauri::command]
pub(crate) async fn delete_macos_template(
    engine: Engine<'_>,
    window: Window,
) -> Result<(), String> {
    require_main(&window)?;
    delete_template(&engine).await.map_err(String::from)
}

#[tauri::command]
pub(crate) async fn create_macos_computer(
    engine: Engine<'_>,
    window: Window,
    request: Value,
) -> Result<Value, String> {
    require_main(&window)?;
    create(engine.inner(), request).await.map_err(String::from)
}

#[tauri::command]
pub(crate) async fn macos_computer_action(
    engine: Engine<'_>,
    window: Window,
    id: String,
    action: Action,
) -> Result<(), String> {
    require_main(&window)?;
    act(engine.inner(), &id, action).await.map_err(String::from)
}

#[tauri::command]
pub(crate) async fn open_macos_display(
    engine: Engine<'_>,
    window: Window,
    id: String,
) -> Result<(), String> {
    require_main(&window)?;
    open_display(&engine, &id).await.map_err(String::from)
}

#[tauri::command]
pub(crate) async fn create_macos_checkpoint(
    engine: Engine<'_>,
    window: Window,
    id: String,
    name: String,
) -> Result<(), String> {
    require_main(&window)?;
    checkpoint(
        &engine,
        requests::checkpoints_create(CheckpointCreateParams {
            computer_id: id,
            device_id: None,
            name,
            kind: None,
        }),
    )
    .await
    .map_err(String::from)
}

#[tauri::command]
pub(crate) async fn restore_macos_checkpoint(
    engine: Engine<'_>,
    window: Window,
    id: String,
    checkpoint_id: String,
) -> Result<(), String> {
    require_main(&window)?;
    checkpoint(
        &engine,
        requests::checkpoints_restore(CheckpointTargetParams {
            computer_id: id,
            device_id: None,
            checkpoint_id,
        }),
    )
    .await
    .map_err(String::from)
}

#[tauri::command]
pub(crate) async fn fork_macos_checkpoint(
    engine: Engine<'_>,
    window: Window,
    id: String,
    checkpoint_id: String,
    new_name: String,
) -> Result<(), String> {
    require_main(&window)?;
    checkpoint(
        &engine,
        requests::checkpoints_fork(CheckpointForkParams {
            computer_id: id,
            device_id: None,
            checkpoint_id: Some(checkpoint_id),
            new_name,
        }),
    )
    .await
    .map_err(String::from)
}

#[tauri::command]
pub(crate) async fn delete_macos_checkpoint(
    engine: Engine<'_>,
    window: Window,
    id: String,
    checkpoint_id: String,
) -> Result<(), String> {
    require_main(&window)?;
    checkpoint(
        &engine,
        requests::checkpoints_delete(CheckpointTargetParams {
            computer_id: id,
            device_id: None,
            checkpoint_id,
        }),
    )
    .await
    .map_err(String::from)
}

#[tauri::command]
pub(crate) async fn macos_computer_clipboard(
    engine: Engine<'_>,
    window: Window,
    id: String,
    direction: ClipboardDirection,
) -> Result<Value, String> {
    require_main(&window)?;
    clipboard(&engine, &id, direction)
        .await
        .map_err(String::from)
}

#[cfg(test)]
mod forwarding_tests {
    use super::*;
    use crate::engine_client::testing::{connected_client, FakeEngine};
    use serde_json::json;
    use silo_api::methods::MethodName;

    fn replies(method: MethodName, _params: &Value) -> Result<Value, (&'static str, String)> {
        match method {
            MethodName::EventsSubscribe => Ok(json!({"subscription": "sub"})),
            MethodName::ComputersList => Ok(json!({"linux": {"computers": []},
                "macos": {"supported": true, "computers": [{"id": "m1", "name": "sonoma"}], "minDiskGiB": 64}})),
            MethodName::ComputersCreate => Ok(
                json!({"jobId": "job:1", "computer": {"id": "m2", "name": "new", "state": "preparing"}}),
            ),
            MethodName::ComputersStart
            | MethodName::ComputersStop
            | MethodName::ComputersDelete
            | MethodName::MacosSetupRetry
            | MethodName::ComputersConfigurationChange
            | MethodName::CheckpointsCreate
            | MethodName::CheckpointsRestore
            | MethodName::CheckpointsFork
            | MethodName::CheckpointsDelete => Ok(json!({"jobId": "job:1"})),
            MethodName::OperationsJobGet => {
                Ok(json!({"jobId": "job:1", "state": "finished", "outcome": {"result": null}}))
            }
            MethodName::ClipboardTransfer => Ok(
                json!({"action": "paste", "status": "pasted", "content": "text", "message": null}),
            ),
            _ => Ok(Value::Null),
        }
    }

    #[tokio::test]
    async fn the_macos_inventory_is_the_macos_part_of_the_list() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        let state = read(&engine).await.unwrap();
        assert_eq!(state["computers"][0]["name"], "sonoma");
        assert_eq!(state["minDiskGiB"], 64);
    }

    #[tokio::test]
    async fn creating_returns_the_new_row_without_waiting_for_the_installation() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        let request = json!({"name": "new", "cpus": 4, "memoryGiB": 8, "diskGiB": 64});
        let row = create(&engine, request.clone()).await.unwrap();
        assert_eq!(row["state"], "preparing");
        assert_eq!(
            fake.calls(MethodName::ComputersCreate),
            vec![json!({"kind": "macos", "request": request})]
        );
    }

    #[tokio::test]
    async fn actions_use_one_method_each() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        for action in [Action::Start, Action::Stop, Action::Delete] {
            act(&engine, "m1", action).await.unwrap();
        }
        act(&engine, "m1", Action::ForceStop).await.unwrap();
        act(&engine, "m1", Action::Setup).await.unwrap();
        let target = vec![json!({"computerId": "m1"})];
        assert_eq!(fake.calls(MethodName::ComputersStart), target);
        assert_eq!(fake.calls(MethodName::ComputersStop), target);
        assert_eq!(fake.calls(MethodName::ComputersDelete), target);
        assert_eq!(fake.calls(MethodName::ComputersForceStop), target);
        assert_eq!(fake.calls(MethodName::MacosSetupRetry), target);
        // The three jobs were waited for and acknowledged.
        assert!(fake.calls(MethodName::OperationsJobAcknowledge).len() >= 3);
    }

    #[tokio::test]
    async fn a_resource_change_is_a_configuration_change_with_the_expected_values() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        let action: Action = serde_json::from_value(json!({"change-resources": {
            "cpus": 6, "memoryGiB": 12, "expected": {"cpus": 4, "memoryGiB": 8, "diskGiB": 64}}}))
        .unwrap();
        act(&engine, "m1", action).await.unwrap();
        let sent = &fake.calls(MethodName::ComputersConfigurationChange)[0]["change"];
        assert_eq!(sent["kind"], "macosResources");
        assert_eq!(sent["computerId"], "m1");
        assert_eq!(
            sent["expected"],
            json!({"cpus": 4, "memoryGiB": 8, "diskGiB": 64})
        );
        assert_eq!(
            (sent["cpus"].clone(), sent["memoryGiB"].clone()),
            (json!(6), json!(12))
        );
        assert!(!sent["requestId"].as_str().unwrap().is_empty());
    }

    #[test]
    fn actions_and_directions_parse_the_uis_words() {
        assert_eq!(
            serde_json::from_value::<Action>(json!("force-stop")).unwrap(),
            Action::ForceStop
        );
        assert!(serde_json::from_value::<Action>(json!("explode")).is_err());
        assert_eq!(
            serde_json::from_value::<ClipboardDirection>(json!("paste-into"))
                .unwrap()
                .action(),
            ClipboardAction::Paste
        );
        assert_eq!(
            serde_json::from_value::<ClipboardDirection>(json!("copy-from"))
                .unwrap()
                .action(),
            ClipboardAction::Copy
        );
    }

    #[tokio::test]
    async fn display_template_clipboard_and_checkpoints_forward() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        open_display(&engine, "m1").await.unwrap();
        assert_eq!(
            fake.calls(MethodName::DisplayShow),
            vec![json!({"computerId": "m1"})]
        );
        delete_template(&engine).await.unwrap();
        assert_eq!(fake.calls(MethodName::MacosTemplateDelete), vec![json!({})]);
        let report = clipboard(&engine, "m1", ClipboardDirection::PasteInto)
            .await
            .unwrap();
        assert_eq!(report["status"], "pasted");
        assert_eq!(
            fake.calls(MethodName::ClipboardTransfer),
            vec![json!({"computerId": "m1", "action": "paste"})]
        );
        checkpoint(
            &engine,
            requests::checkpoints_fork(CheckpointForkParams {
                computer_id: "m1".into(),
                device_id: None,
                checkpoint_id: Some("c".into()),
                new_name: "n".into(),
            }),
        )
        .await
        .unwrap();
        assert_eq!(fake.calls(MethodName::CheckpointsFork).len(), 1);
    }

    #[tokio::test]
    async fn refusals_are_typed_errors() {
        let fake = FakeEngine::start(|method, params| match method {
            MethodName::DisplayShow => Err((
                "unsupported_remote_operation",
                "Start the computer to open its display.".into(),
            )),
            MethodName::ComputersStart => Err(("busy", "Another operation is running.".into())),
            other => replies(other, params),
        });
        let (engine, _) = connected_client(&fake).await;
        assert_eq!(
            open_display(&engine, "m1").await.unwrap_err().code,
            "unsupported_remote_operation"
        );
        assert_eq!(
            act(&engine, "m1", Action::Start).await.unwrap_err().code,
            "busy"
        );
    }
}
