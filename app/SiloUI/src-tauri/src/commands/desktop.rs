//! Tauri adapters for desktop and computer-use operations: each forwards to `silod`.
use crate::engine_client::{params::decode, targets, Engine, EngineClient, EngineError};
use serde_json::Value;
use silo_api::methods::{
    computer_use::ComputerUseApprovalParams, desktop::DesktopActionParams, requests,
};
use tauri::Window;

/// The computer's desktop status (`desktop.status`).
pub(crate) async fn desktop_state(
    engine: &EngineClient,
    computer: &str,
) -> Result<Value, EngineError> {
    let target = targets::resolve(engine, computer).await?;
    engine
        .call(requests::desktop_status(target.computer()))
        .await
}

/// Runs a desktop action and returns the desktop status afterwards. Setting up computer use
/// is its own canonical job (`computerUse.setup`); the other actions are `desktop.action`.
pub(crate) async fn desktop_do(
    engine: &EngineClient,
    computer: &str,
    action: &str,
) -> Result<Value, EngineError> {
    let target = targets::resolve(engine, computer).await?;
    let outcome = match action {
        "setup-computer-use" | "setup-lcu" => {
            engine
                .run_job(requests::computer_use_setup(target.computer()))
                .await?
        }
        other => {
            let action = decode(Value::String(other.to_owned()), "desktop action")?;
            engine
                .run_job(requests::desktop_action(DesktopActionParams {
                    computer_id: target.computer_id.clone(),
                    device_id: target.device_id.clone(),
                    action,
                }))
                .await?
        }
    };
    // A finished desktop action carries the status; otherwise read it.
    if outcome.get("installed").is_some() {
        Ok(outcome)
    } else {
        engine
            .call(requests::desktop_status(target.computer()))
            .await
    }
}

/// Changes the computer-use approval and returns the desktop status the UI renders.
pub(crate) async fn approve(
    engine: &EngineClient,
    computer: &str,
    mode: &str,
) -> Result<Value, EngineError> {
    let target = targets::resolve(engine, computer).await?;
    let mode = decode(Value::String(mode.to_owned()), "approval mode")?;
    engine
        .call(requests::computer_use_approval(ComputerUseApprovalParams {
            computer_id: target.computer_id.clone(),
            device_id: target.device_id.clone(),
            mode,
        }))
        .await?;
    engine
        .call(requests::desktop_status(target.computer()))
        .await
}

#[tauri::command]
pub(crate) async fn set_computer_use_approval(
    engine: Engine<'_>,
    window: Window,
    computer: String,
    mode: String,
) -> Result<Value, String> {
    crate::commands::desktop_viewer::require_computer(&window, &computer)?;
    approve(&engine, &computer, &mode)
        .await
        .map_err(String::from)
}

#[tauri::command]
pub(crate) async fn read_desktop_state(
    engine: Engine<'_>,
    window: Window,
    computer: String,
) -> Result<Value, String> {
    crate::commands::desktop_viewer::require_computer(&window, &computer)?;
    desktop_state(&engine, &computer)
        .await
        .map_err(String::from)
}

#[tauri::command]
pub(crate) async fn desktop_action(
    engine: Engine<'_>,
    window: Window,
    computer: String,
    action: String,
) -> Result<Value, String> {
    crate::commands::desktop_viewer::require_computer(&window, &computer)?;
    desktop_do(&engine, &computer, &action)
        .await
        .map_err(String::from)
}

#[cfg(test)]
mod forwarding_tests {
    use super::*;
    use crate::engine_client::testing::{connected_client, FakeEngine};
    use serde_json::json;
    use silo_api::methods::MethodName;

    const DEV: &str = "00000000-0000-4000-8000-00000000000d";

    fn replies(method: MethodName, _params: &Value) -> Result<Value, (&'static str, String)> {
        match method {
            MethodName::EventsSubscribe => Ok(json!({"subscription": "sub"})),
            MethodName::ComputersList => Ok(json!({
                "linux": {"computers": [{"kind": "linux", "configuration": {"id": DEV, "name": "dev"}}]},
                "macos": {"computers": []}
            })),
            MethodName::DesktopStatus => {
                Ok(json!({"installed": true, "state": "running", "autoStart": true}))
            }
            MethodName::DesktopAction | MethodName::ComputerUseSetup => {
                Ok(json!({"jobId": "job:1"}))
            }
            MethodName::OperationsJobGet => Ok(json!({"jobId": "job:1", "state": "finished",
                "outcome": {"result": {"installed": true, "state": "stopped", "autoStart": true}}})),
            MethodName::ComputerUseApproval => Ok(json!({"approval": "auto"})),
            _ => Ok(Value::Null),
        }
    }

    #[tokio::test]
    async fn the_state_is_read_by_computer_id() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        assert_eq!(
            desktop_state(&engine, "dev").await.unwrap()["state"],
            "running"
        );
        assert_eq!(
            fake.calls(MethodName::DesktopStatus),
            vec![json!({"computerId": DEV})]
        );
        let remote = format!("silo-remote:device-1:{DEV}");
        desktop_state(&engine, &remote).await.unwrap();
        assert_eq!(
            fake.calls(MethodName::DesktopStatus)[1],
            json!({"computerId": DEV, "deviceId": "device-1"})
        );
        assert_eq!(
            desktop_state(&engine, "ghost").await.unwrap_err().code,
            "not_found"
        );
    }

    #[tokio::test]
    async fn an_action_runs_the_job_and_returns_the_status_it_finished_with() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        let status = desktop_do(&engine, "dev", "restart-streamer")
            .await
            .unwrap();
        assert_eq!(status["state"], "stopped");
        assert_eq!(
            fake.calls(MethodName::DesktopAction),
            vec![json!({"computerId": DEV, "action": "restart-streamer"})]
        );
        assert_eq!(
            fake.calls(MethodName::OperationsJobAcknowledge),
            vec![json!({"jobId": "job:1"})]
        );
    }

    #[tokio::test]
    async fn setting_up_computer_use_is_its_own_job_and_reads_the_status_after() {
        let fake = FakeEngine::start(|method, params| match method {
            MethodName::OperationsJobGet => {
                Ok(json!({"jobId": "job:1", "state": "finished", "outcome": {"result": null}}))
            }
            other => replies(other, params),
        });
        let (engine, _) = connected_client(&fake).await;
        let status = desktop_do(&engine, "dev", "setup-computer-use")
            .await
            .unwrap();
        assert_eq!(status["state"], "running");
        assert_eq!(
            fake.calls(MethodName::ComputerUseSetup),
            vec![json!({"computerId": DEV})]
        );
        assert!(fake.calls(MethodName::DesktopAction).is_empty());
    }

    #[tokio::test]
    async fn an_approval_change_returns_the_refreshed_status() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        let status = approve(&engine, "dev", "auto").await.unwrap();
        assert_eq!(status["installed"], true);
        assert_eq!(
            fake.calls(MethodName::ComputerUseApproval),
            vec![json!({"computerId": DEV, "mode": "auto"})]
        );
    }

    #[tokio::test]
    async fn a_refused_action_is_a_typed_error() {
        let fake = FakeEngine::start(|method, params| match method {
            MethodName::DesktopAction => {
                Err(("unsupported_remote_operation", "Not on that device.".into()))
            }
            other => replies(other, params),
        });
        let (engine, _) = connected_client(&fake).await;
        let error = desktop_do(&engine, "dev", "start").await.unwrap_err();
        assert_eq!(error.code, "unsupported_remote_operation");
    }
}
