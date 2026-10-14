//! Tauri commands for macOS computers: each forwards to `silod`, which owns the VMs and
//! hosts their display windows.
use crate::engine_client::{Engine, EngineClient, EngineError};
use serde::Deserialize;
use serde_json::{json, Value};
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
    fn action(self) -> &'static str {
        match self {
            Self::PasteInto => "paste",
            Self::CopyFrom => "copy",
        }
    }
}

/// The macOS part of the inventory, in the shape the section parses.
pub(crate) async fn read(engine: &EngineClient) -> Result<Value, EngineError> {
    let listed = engine.call("computers.list", json!({})).await?;
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
    let started = engine
        .call(
            "computers.create",
            json!({"kind": "macos", "request": request}),
        )
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
    let target = json!({"computerId": id});
    match action {
        Action::Start => drop(engine.run_job("computers.start", target).await?),
        Action::Stop => drop(engine.run_job("computers.stop", target).await?),
        Action::Delete => drop(engine.run_job("computers.delete", target).await?),
        Action::ForceStop => drop(engine.call("computers.forceStop", target).await?),
        // Setup runs for as long as the installation steps need; the row shows it.
        Action::Setup => {
            let started = engine.call("macos.setup.retry", target).await?;
            acknowledge_in_background(engine, &started);
        }
        Action::ChangeResources {
            cpus,
            memory_gib,
            expected,
        } => {
            let change = json!({
                "kind": "macosResources",
                "computerId": id,
                "expected": {"cpus": expected.cpus, "memoryGiB": expected.memory_gib, "diskGiB": expected.disk_gib},
                "cpus": cpus,
                "memoryGiB": memory_gib,
                "requestId": uuid::Uuid::new_v4().to_string(),
            });
            engine
                .run_job("computers.configuration.change", json!({"change": change}))
                .await?;
        }
    }
    Ok(())
}

pub(crate) async fn open_display(engine: &EngineClient, id: &str) -> Result<(), EngineError> {
    engine
        .call("display.show", json!({"computerId": id}))
        .await?;
    Ok(())
}

pub(crate) async fn clipboard(
    engine: &EngineClient,
    id: &str,
    direction: ClipboardDirection,
) -> Result<Value, EngineError> {
    engine
        .call(
            "clipboard.transfer",
            json!({"computerId": id, "action": direction.action()}),
        )
        .await
}

pub(crate) async fn delete_template(engine: &EngineClient) -> Result<(), EngineError> {
    engine.call("macos.template.delete", json!({})).await?;
    Ok(())
}

pub(crate) async fn checkpoint(
    engine: &EngineClient,
    method: &str,
    params: Value,
) -> Result<(), EngineError> {
    engine.run_job(method, params).await?;
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
        "checkpoints.create",
        json!({"computerId": id, "name": name}),
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
        "checkpoints.restore",
        json!({"computerId": id, "checkpointId": checkpoint_id}),
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
        "checkpoints.fork",
        json!({"computerId": id, "checkpointId": checkpoint_id, "newName": new_name}),
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
        "checkpoints.delete",
        json!({"computerId": id, "checkpointId": checkpoint_id}),
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

    fn replies(method: &str, _params: &Value) -> Result<Value, (&'static str, String)> {
        match method {
            "events.subscribe" => Ok(json!({"subscription": "sub"})),
            "computers.list" => Ok(json!({"linux": {"computers": []},
                "macos": {"supported": true, "computers": [{"id": "m1", "name": "sonoma"}], "minDiskGiB": 64}})),
            "computers.create" => Ok(
                json!({"jobId": "job:1", "computer": {"id": "m2", "name": "new", "state": "preparing"}}),
            ),
            "computers.start"
            | "computers.stop"
            | "computers.delete"
            | "macos.setup.retry"
            | "computers.configuration.change"
            | "checkpoints.create"
            | "checkpoints.restore"
            | "checkpoints.fork"
            | "checkpoints.delete" => Ok(json!({"jobId": "job:1"})),
            "operations.job.get" => {
                Ok(json!({"jobId": "job:1", "state": "finished", "outcome": {"result": null}}))
            }
            "clipboard.transfer" => Ok(
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
            fake.calls("computers.create"),
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
        assert_eq!(fake.calls("computers.start"), target);
        assert_eq!(fake.calls("computers.stop"), target);
        assert_eq!(fake.calls("computers.delete"), target);
        assert_eq!(fake.calls("computers.forceStop"), target);
        assert_eq!(fake.calls("macos.setup.retry"), target);
        // The three jobs were waited for and acknowledged.
        assert!(fake.calls("operations.job.acknowledge").len() >= 3);
    }

    #[tokio::test]
    async fn a_resource_change_is_a_configuration_change_with_the_expected_values() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        let action: Action = serde_json::from_value(json!({"change-resources": {
            "cpus": 6, "memoryGiB": 12, "expected": {"cpus": 4, "memoryGiB": 8, "diskGiB": 64}}}))
        .unwrap();
        act(&engine, "m1", action).await.unwrap();
        let sent = &fake.calls("computers.configuration.change")[0]["change"];
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
            "paste"
        );
        assert_eq!(
            serde_json::from_value::<ClipboardDirection>(json!("copy-from"))
                .unwrap()
                .action(),
            "copy"
        );
    }

    #[tokio::test]
    async fn display_template_clipboard_and_checkpoints_forward() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        open_display(&engine, "m1").await.unwrap();
        assert_eq!(
            fake.calls("display.show"),
            vec![json!({"computerId": "m1"})]
        );
        delete_template(&engine).await.unwrap();
        assert_eq!(fake.calls("macos.template.delete"), vec![json!({})]);
        let report = clipboard(&engine, "m1", ClipboardDirection::PasteInto)
            .await
            .unwrap();
        assert_eq!(report["status"], "pasted");
        assert_eq!(
            fake.calls("clipboard.transfer"),
            vec![json!({"computerId": "m1", "action": "paste"})]
        );
        checkpoint(
            &engine,
            "checkpoints.fork",
            json!({"computerId": "m1", "checkpointId": "c", "newName": "n"}),
        )
        .await
        .unwrap();
        assert_eq!(fake.calls("checkpoints.fork").len(), 1);
    }

    #[tokio::test]
    async fn refusals_are_typed_errors() {
        let fake = FakeEngine::start(|method, params| match method {
            "display.show" => Err((
                "unsupported_remote_operation",
                "Start the computer to open its display.".into(),
            )),
            "computers.start" => Err(("busy", "Another operation is running.".into())),
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
