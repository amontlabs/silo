//! Tauri adapters for desktop and computer-use operations: each forwards to `silod`.
use crate::engine_client::{targets, Engine, EngineClient, EngineError};
use serde_json::{json, Value};
use tauri::Window;

/// The computer's desktop status (`desktop.status`).
pub(crate) async fn desktop_state(
    engine: &EngineClient,
    computer: &str,
) -> Result<Value, EngineError> {
    let target = targets::resolve(engine, computer).await?;
    engine
        .call("desktop.status", target.params(Value::Null))
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
                .run_job("computerUse.setup", target.params(Value::Null))
                .await?
        }
        other => {
            engine
                .run_job("desktop.action", target.params(json!({"action": other})))
                .await?
        }
    };
    // A finished desktop action carries the status; otherwise read it.
    if outcome.get("installed").is_some() {
        Ok(outcome)
    } else {
        engine
            .call("desktop.status", target.params(Value::Null))
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
    engine
        .call("computerUse.approval", target.params(json!({"mode": mode})))
        .await?;
    engine
        .call("desktop.status", target.params(Value::Null))
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

    const DEV: &str = "00000000-0000-4000-8000-00000000000d";

    fn replies(method: &str, _params: &Value) -> Result<Value, (&'static str, String)> {
        match method {
            "events.subscribe" => Ok(json!({"subscription": "sub"})),
            "computers.list" => Ok(json!({
                "linux": {"computers": [{"kind": "linux", "configuration": {"id": DEV, "name": "dev"}}]},
                "macos": {"computers": []}
            })),
            "desktop.status" => {
                Ok(json!({"installed": true, "state": "running", "autoStart": true}))
            }
            "desktop.action" | "computerUse.setup" => Ok(json!({"jobId": "job:1"})),
            "operations.job.get" => Ok(json!({"jobId": "job:1", "state": "finished",
                "outcome": {"result": {"installed": true, "state": "stopped", "autoStart": true}}})),
            "computerUse.approval" => Ok(json!({"approval": "auto"})),
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
            fake.calls("desktop.status"),
            vec![json!({"computerId": DEV})]
        );
        let remote = format!("silo-remote:device-1:{DEV}");
        desktop_state(&engine, &remote).await.unwrap();
        assert_eq!(
            fake.calls("desktop.status")[1],
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
            fake.calls("desktop.action"),
            vec![json!({"computerId": DEV, "action": "restart-streamer"})]
        );
        assert_eq!(
            fake.calls("operations.job.acknowledge"),
            vec![json!({"jobId": "job:1"})]
        );
    }

    #[tokio::test]
    async fn setting_up_computer_use_is_its_own_job_and_reads_the_status_after() {
        let fake = FakeEngine::start(|method, params| match method {
            "operations.job.get" => {
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
            fake.calls("computerUse.setup"),
            vec![json!({"computerId": DEV})]
        );
        assert!(fake.calls("desktop.action").is_empty());
    }

    #[tokio::test]
    async fn an_approval_change_returns_the_refreshed_status() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        let status = approve(&engine, "dev", "auto").await.unwrap();
        assert_eq!(status["installed"], true);
        assert_eq!(
            fake.calls("computerUse.approval"),
            vec![json!({"computerId": DEV, "mode": "auto"})]
        );
    }

    #[tokio::test]
    async fn a_refused_action_is_a_typed_error() {
        let fake = FakeEngine::start(|method, params| match method {
            "desktop.action" => Err(("unsupported_remote_operation", "Not on that device.".into())),
            other => replies(other, params),
        });
        let (engine, _) = connected_client(&fake).await;
        let error = desktop_do(&engine, "dev", "start").await.unwrap_err();
        assert_eq!(error.code, "unsupported_remote_operation");
    }
}
