//! Tauri adapters for the computer inventory, lifecycle, configuration and operation queue.
//! Every command forwards to `silod`; nothing here runs engine work.

// MARK: Commands
//
// Each command forwards to the canonical engine method. The commands keep their names and
// results so the UI is unchanged; where the engine answers with a job, the command waits for
// its outcome and returns the state the UI used to receive.

use crate::engine_client::{
    params::{decode, device, job_id as parse_job_id, target},
    Engine, EngineClient, EngineError,
};
use serde_json::Value;
use silo_api::methods::{
    common::EmptyParams,
    computers::{
        ComputerConfigurationChangeParams, ComputerConfigurationRetryParams,
        ComputerCreationSkipWaitParams, ComputerIdentitiesParams, ComputerIdentity,
        ComputerShellParams,
    },
    kind,
    operations::{CancelJobParams, OperationsListParams},
    requests,
    terminal::{EditorCommandParams, TerminalCommandParams},
    Request,
};

pub(crate) async fn operation_queue(engine: &EngineClient) -> Result<Value, EngineError> {
    engine
        .call(requests::operations_list(OperationsListParams {
            device_id: None,
        }))
        .await
}

/// Cancels by the queue id the UI shows. The engine cancels jobs, so the id is resolved to
/// its job first; an entry with no job cannot be cancelled.
pub(crate) async fn cancel_queue_entry(engine: &EngineClient, id: u64) -> Result<(), EngineError> {
    let queue = operation_queue(engine).await?;
    let job_id = ["running", "waiting"]
        .iter()
        .filter_map(|list| queue.get(list).and_then(Value::as_array))
        .flatten()
        .find(|entry| entry.get("id").and_then(Value::as_u64) == Some(id))
        .ok_or_else(|| EngineError::not_found("That operation is no longer in the queue."))?
        .get("jobId")
        .and_then(Value::as_str)
        .ok_or_else(|| EngineError::internal("That operation cannot be cancelled."))?
        .to_owned();
    engine
        .call(requests::operations_cancel(CancelJobParams {
            job_id: parse_job_id(&job_id)?,
            device_id: None,
        }))
        .await?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn read_operation_queue(engine: Engine<'_>) -> Result<Value, EngineError> {
    operation_queue(&engine).await
}

/// Ask to cancel a queued or running operation by its queue id.
#[tauri::command]
pub(crate) async fn cancel_operation(engine: Engine<'_>, id: u64) -> Result<(), EngineError> {
    cancel_queue_entry(&engine, id).await
}

/// The UI names a computer by its display name; the engine's identities carry ids.
async fn engine_identities(
    engine: &EngineClient,
    identities: Vec<Value>,
) -> Result<ComputerIdentitiesParams, EngineError> {
    let mut converted = Vec::with_capacity(identities.len());
    for identity in identities {
        let name = identity
            .get("computer")
            .and_then(Value::as_str)
            .ok_or_else(|| EngineError::internal("A Git identity names no computer."))?;
        let text = |field: &str| {
            identity
                .get(field)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| EngineError::internal(format!("A Git identity has no {field}.")))
        };
        converted.push(ComputerIdentity {
            computer_id: engine.computer_id(name).await?,
            name: text("name")?,
            email: text("email")?,
            apply: identity
                .get("apply")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        });
    }
    Ok(ComputerIdentitiesParams {
        identities: converted,
        device_id: None,
    })
}

pub(crate) async fn verify_identities(
    engine: &EngineClient,
    identities: Vec<Value>,
) -> Result<bool, EngineError> {
    let identities = engine_identities(engine, identities).await?;
    let verified = engine
        .call(requests::computers_identities_verify(identities))
        .await?;
    Ok(verified.as_bool().unwrap_or(false))
}

pub(crate) async fn configure_identities(
    engine: &EngineClient,
    identities: Vec<Value>,
) -> Result<(), EngineError> {
    let identities = engine_identities(engine, identities).await?;
    engine
        .run_job(requests::computers_identities_configure(identities))
        .await?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn verify_computer_identities(
    engine: Engine<'_>,
    identities: Vec<Value>,
) -> Result<bool, String> {
    verify_identities(&engine, identities)
        .await
        .map_err(String::from)
}

#[tauri::command]
pub(crate) async fn configure_computer_identities(
    engine: Engine<'_>,
    identities: Vec<Value>,
) -> Result<(), String> {
    configure_identities(&engine, identities)
        .await
        .map_err(String::from)
}

#[tauri::command]
pub(crate) async fn read_computer_configuration(engine: Engine<'_>) -> Result<Value, String> {
    engine
        .call(requests::computers_configuration_read(device(None)))
        .await
        .map_err(String::from)
}

#[tauri::command]
pub(crate) async fn read_application_state(
    engine: Engine<'_>,
    refresh_repositories: Option<bool>,
) -> Result<Value, EngineError> {
    engine
        .application_source(refresh_repositories.unwrap_or(false))
        .await
}

#[tauri::command]
pub(crate) async fn read_application_shell(
    engine: Engine<'_>,
    error: String,
) -> Result<Value, String> {
    engine
        .call(requests::computers_shell(ComputerShellParams { error }))
        .await
        .map_err(String::from)
}

/// Runs one lifecycle action and returns the resulting Linux state. The launcher closures
/// are the only client-side part: the engine builds the command, this app starts it.
pub(crate) async fn lifecycle_action(
    engine: &EngineClient,
    action: &str,
    name: &str,
    path: Option<String>,
    open_terminal: impl FnOnce(&serde_json::Map<String, Value>, &[String]) -> Result<(), String>,
    open_editor: impl FnOnce(&[String]) -> Result<(), String>,
) -> Result<Value, EngineError> {
    let computer_id = engine.computer_id(name).await?;
    match action {
        "open-terminal" => {
            let command = engine
                .call(requests::terminal_command(TerminalCommandParams {
                    computer_id,
                    device_id: None,
                }))
                .await?;
            let settings = engine.call(requests::settings_read(EmptyParams {})).await?;
            let settings = settings
                .get("settings")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            open_terminal(&settings, &argv(&command)?).map_err(EngineError::internal)?;
        }
        "open-editor" => {
            let command = engine
                .call(requests::editor_command(EditorCommandParams {
                    computer_id,
                    device_id: None,
                    path,
                }))
                .await?;
            open_editor(&argv(&command)?).map_err(EngineError::internal)?;
        }
        "dismiss-error" => {
            return engine
                .call(requests::computers_dismiss_error(target(
                    &computer_id,
                    None,
                )))
                .await;
        }
        "start" | "stop" | "restart" => {
            let params = target(&computer_id, None);
            let request: Request<kind::Job> = match action {
                "start" => requests::computers_start(params),
                "stop" => requests::computers_stop(params),
                _ => requests::computers_restart(params),
            };
            engine.run_job(request).await?;
        }
        other => {
            return Err(EngineError::internal(format!(
                "Silo does not know the computer action {other}."
            )))
        }
    }
    engine.application_source(false).await
}

fn argv(command: &Value) -> Result<Vec<String>, EngineError> {
    command
        .get("argv")
        .and_then(Value::as_array)
        .map(|argv| {
            argv.iter()
                .filter_map(|argument| argument.as_str().map(str::to_owned))
                .collect()
        })
        .ok_or_else(|| EngineError::internal("The engine returned no command."))
}

#[tauri::command]
pub(crate) async fn computer_action(
    engine: Engine<'_>,
    action: String,
    name: String,
    path: Option<String>,
) -> Result<Value, EngineError> {
    lifecycle_action(
        &engine,
        &action,
        &name,
        path,
        crate::host_launch::open_terminal,
        crate::host_launch::launch_editor,
    )
    .await
}

#[tauri::command]
pub(crate) async fn read_setup_activity(engine: Engine<'_>) -> Result<Value, String> {
    engine
        .call(requests::activity_setup_read(device(None)))
        .await
        .map_err(String::from)
}

#[tauri::command]
pub(crate) async fn skip_computer_use_wait(
    engine: Engine<'_>,
    request_id: String,
) -> Result<(), String> {
    engine
        .call(requests::computers_creation_skip_computer_use_wait(
            ComputerCreationSkipWaitParams {
                request_id,
                device_id: None,
            },
        ))
        .await
        .map(drop)
        .map_err(String::from)
}

pub(crate) async fn retry_configuration(
    engine: &EngineClient,
    request_id: Option<String>,
    retry_computer: Option<String>,
) -> Result<Value, EngineError> {
    let retry_computer_id = match retry_computer {
        Some(name) => Some(engine.computer_id(&name).await?),
        None => None,
    };
    engine
        .run_job(requests::computers_configuration_retry(
            ComputerConfigurationRetryParams {
                request_id,
                retry_computer_id,
                device_id: None,
            },
        ))
        .await
}

pub(crate) async fn change_configuration(
    engine: &EngineClient,
    change: Value,
    request_id: Option<String>,
    retry_computer: Option<String>,
) -> Result<Value, EngineError> {
    let change = decode(change, "computer configuration change")?;
    let retry_computer_id = match retry_computer {
        Some(name) => Some(engine.computer_id(&name).await?),
        None => None,
    };
    engine
        .run_job(requests::computers_configuration_change(
            ComputerConfigurationChangeParams {
                change,
                request_id,
                retry_computer_id,
                device_id: None,
            },
        ))
        .await
}

#[tauri::command]
pub(crate) async fn retry_computer_configuration(
    engine: Engine<'_>,
    request_id: Option<String>,
    retry_computer: Option<String>,
) -> Result<Value, String> {
    retry_configuration(&engine, request_id, retry_computer)
        .await
        .map_err(String::from)
}

#[tauri::command]
pub(crate) async fn change_computer_configuration(
    engine: Engine<'_>,
    change: Value,
    request_id: Option<String>,
    retry_computer: Option<String>,
) -> Result<Value, String> {
    change_configuration(&engine, change, request_id, retry_computer)
        .await
        .map_err(String::from)
}

#[cfg(test)]
mod forwarding_tests {
    use super::*;
    use crate::engine_client::testing::{connected_client, FakeEngine};
    use serde_json::json;
    use silo_api::methods::MethodName;
    use std::sync::{Arc, Mutex};

    const DEV: &str = "00000000-0000-4000-8000-00000000000d";

    fn listing() -> Value {
        json!({
            "linux": {"computers": [{"kind": "linux", "configuration": {"id": DEV, "name": "dev"}}]},
            "macos": {"computers": [{"id": "mac-1", "name": "sonoma"}]}
        })
    }

    fn engine_replies(method: MethodName, params: &Value) -> Result<Value, (&'static str, String)> {
        match method {
            MethodName::EventsSubscribe => Ok(json!({"subscription": "sub"})),
            MethodName::ComputersList => Ok(listing()),
            MethodName::ComputersStart
            | MethodName::ComputersStop
            | MethodName::ComputersRestart => Ok(json!({"jobId": "job:1"})),
            MethodName::OperationsJobGet => {
                Ok(json!({"jobId": "job:1", "state": "finished", "outcome": {"result": null}}))
            }
            MethodName::OperationsList => Ok(json!({
                "running": [{"id": 7, "jobId": "job:9"}, {"id": 8}],
                "waiting": [{"id": 9, "jobId": "job:10"}],
                "unacknowledgedJobIds": []
            })),
            MethodName::TerminalCommand => Ok(json!({"argv": ["/usr/bin/ssh", "-t", "alias"]})),
            MethodName::EditorCommand => {
                Ok(json!({"argv": ["code", "--folder-uri", params["path"]]}))
            }
            MethodName::SettingsRead => Ok(json!({"settings": {"terminalApplication": "x"}})),
            MethodName::ComputersDismissError => Ok(json!({"computers": []})),
            _ => Ok(Value::Null),
        }
    }

    #[tokio::test]
    async fn the_queue_is_read_and_cancelled_through_the_entry_job() {
        let fake = FakeEngine::start(|method, params| engine_replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        assert_eq!(
            operation_queue(&engine).await.unwrap()["running"][0]["id"],
            7
        );
        cancel_queue_entry(&engine, 9).await.unwrap();
        assert_eq!(
            fake.calls(MethodName::OperationsCancel),
            vec![json!({"jobId": "job:10"})]
        );
        // The display id is never the cancellation key, and an entry without a job has none.
        assert!(cancel_queue_entry(&engine, 8).await.is_err());
        assert_eq!(
            cancel_queue_entry(&engine, 99).await.unwrap_err().code,
            "not_found"
        );
        assert_eq!(fake.calls(MethodName::OperationsCancel).len(), 1);
    }

    #[tokio::test]
    async fn a_lifecycle_action_resolves_the_id_runs_the_job_and_returns_the_state() {
        let fake = FakeEngine::start(|method, params| engine_replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        let state = lifecycle_action(
            &engine,
            "start",
            "dev",
            None,
            |_, _| panic!("no terminal"),
            |_| panic!("no editor"),
        )
        .await
        .unwrap();
        assert_eq!(state["computers"][0]["configuration"]["name"], "dev");
        assert_eq!(
            fake.calls(MethodName::ComputersStart),
            vec![json!({"computerId": DEV})]
        );
        assert_eq!(
            fake.calls(MethodName::OperationsJobAcknowledge),
            vec![json!({"jobId": "job:1"})]
        );
        let missing = lifecycle_action(&engine, "stop", "ghost", None, |_, _| Ok(()), |_| Ok(()))
            .await
            .unwrap_err();
        assert_eq!(missing.code, "not_found");
        let unknown = lifecycle_action(&engine, "explode", "dev", None, |_, _| Ok(()), |_| Ok(()))
            .await
            .unwrap_err();
        assert!(unknown.message.contains("explode"));
    }

    #[tokio::test]
    async fn terminal_and_editor_run_the_command_the_engine_built() {
        let fake = FakeEngine::start(|method, params| engine_replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        let terminal = Arc::new(Mutex::new(None));
        let seen = terminal.clone();
        lifecycle_action(
            &engine,
            "open-terminal",
            "dev",
            None,
            move |settings, argv| {
                *seen.lock().unwrap() = Some((settings.clone(), argv.to_vec()));
                Ok(())
            },
            |_| Ok(()),
        )
        .await
        .unwrap();
        let (settings, argv) = terminal.lock().unwrap().clone().unwrap();
        assert_eq!(settings["terminalApplication"], "x");
        assert_eq!(argv, vec!["/usr/bin/ssh", "-t", "alias"]);
        assert_eq!(
            fake.calls(MethodName::TerminalCommand),
            vec![json!({"computerId": DEV})]
        );

        let editor = Arc::new(Mutex::new(Vec::new()));
        let seen = editor.clone();
        lifecycle_action(
            &engine,
            "open-editor",
            "dev",
            Some("/workspace/app".into()),
            |_, _| Ok(()),
            move |argv| {
                *seen.lock().unwrap() = argv.to_vec();
                Ok(())
            },
        )
        .await
        .unwrap();
        assert_eq!(
            *editor.lock().unwrap(),
            vec!["code", "--folder-uri", "/workspace/app"]
        );
        assert_eq!(
            fake.calls(MethodName::EditorCommand),
            vec![json!({"computerId": DEV, "path": "/workspace/app"})]
        );
        // A launch that fails is reported, not swallowed.
        let failed = lifecycle_action(
            &engine,
            "open-editor",
            "dev",
            None,
            |_, _| Ok(()),
            |_| Err("The editor could not be opened.".into()),
        )
        .await
        .unwrap_err();
        assert_eq!(failed.message, "The editor could not be opened.");
    }

    #[tokio::test]
    async fn dismissing_an_error_returns_the_engines_state_without_a_job() {
        let fake = FakeEngine::start(|method, params| engine_replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        let state = lifecycle_action(
            &engine,
            "dismiss-error",
            "dev",
            None,
            |_, _| Ok(()),
            |_| Ok(()),
        )
        .await
        .unwrap();
        assert_eq!(state, json!({"computers": []}));
        assert!(fake.calls(MethodName::OperationsJobGet).is_empty());
    }

    #[tokio::test]
    async fn identities_and_configuration_changes_address_computers_by_id() {
        let fake = FakeEngine::start(|method, params| match method {
            MethodName::ComputersIdentitiesVerify => Ok(json!(true)),
            MethodName::ComputersIdentitiesConfigure
            | MethodName::ComputersConfigurationRetry
            | MethodName::ComputersConfigurationChange => Ok(json!({"jobId": "setup:r1"})),
            MethodName::OperationsJobGet => Ok(json!({"jobId": "setup:r1", "state": "finished",
                "outcome": {"result": {"computers": []}}})),
            other => engine_replies(other, params),
        });
        let (engine, _) = connected_client(&fake).await;
        let identity =
            json!({"computer": "dev", "name": "Ada", "email": "ada@example.com", "apply": true});
        assert!(verify_identities(&engine, vec![identity.clone()])
            .await
            .unwrap());
        configure_identities(&engine, vec![identity]).await.unwrap();
        let expected =
            json!([{"computerId": DEV, "name": "Ada", "email": "ada@example.com", "apply": true}]);
        assert_eq!(
            fake.calls(MethodName::ComputersIdentitiesVerify)[0]["identities"],
            expected
        );
        assert_eq!(
            fake.calls(MethodName::ComputersIdentitiesConfigure)[0]["identities"],
            expected
        );

        let state = change_configuration(
            &engine,
            json!({"kind": "batch", "changes": []}),
            Some("r1".into()),
            Some("dev".into()),
        )
        .await
        .unwrap();
        assert_eq!(state, json!({"computers": []}));
        assert_eq!(
            fake.calls(MethodName::ComputersConfigurationChange),
            vec![
                json!({"change": {"kind": "batch", "changes": []}, "requestId": "r1", "retryComputerId": DEV})
            ]
        );
        retry_configuration(&engine, Some("r1".into()), None)
            .await
            .unwrap();
        assert_eq!(
            fake.calls(MethodName::ComputersConfigurationRetry),
            vec![json!({"requestId": "r1"})]
        );
        assert!(
            verify_identities(&engine, vec![json!({"computer": "ghost"})])
                .await
                .is_err()
        );
    }
}
