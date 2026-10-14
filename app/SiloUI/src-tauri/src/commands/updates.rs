//! Tauri adapters for engine updates. The engine owns the schedule, the feed and the state
//! (`updates.*`, `updates.changed`); this app shows them and opens the release page.
//! Replacing the app bundle is the shared installer's work, not an in-process command.

use serde_json::{json, Value};
use tauri::AppHandle;

use crate::engine_client::{Engine, EngineClient, EngineError};

const INSTALL_UNAVAILABLE: &str =
    "Silo cannot install an update by itself yet. Download the new version from the release page.";

pub(crate) async fn state(engine: &EngineClient) -> Result<Value, EngineError> {
    engine.call("updates.state", json!({})).await
}

pub(crate) async fn check(engine: &EngineClient) -> Result<Value, EngineError> {
    engine.call("updates.check", json!({})).await
}

/// The download is a job whose progress arrives as `updates.changed`; the result is the
/// state once it ends.
pub(crate) async fn download(engine: &EngineClient) -> Result<Value, EngineError> {
    engine.run_job("updates.download", json!({})).await?;
    state(engine).await
}

pub(crate) async fn set_automatic_checks(
    engine: &EngineClient,
    enabled: bool,
) -> Result<Value, EngineError> {
    engine
        .call("updates.setAutomaticChecks", json!({"enabled": enabled}))
        .await
}

/// The window gained focus: the engine may check for updates now. Fire and forget.
pub(crate) fn focused(app: &AppHandle) {
    let Some(engine) = crate::engine_client::try_of(app) else {
        return;
    };
    tauri::async_runtime::spawn(async move {
        let _ = engine.call("updates.focused", json!({})).await;
    });
}

#[tauri::command]
pub(crate) async fn get_update_state(engine: Engine<'_>) -> Result<Value, String> {
    state(&engine).await.map_err(String::from)
}

#[tauri::command]
pub(crate) async fn check_for_update(engine: Engine<'_>) -> Result<Value, String> {
    check(&engine).await.map_err(String::from)
}

#[tauri::command]
pub(crate) async fn download_update(engine: Engine<'_>) -> Result<Value, String> {
    download(&engine).await.map_err(String::from)
}

#[tauri::command]
pub(crate) async fn install_update(_stop_computers: bool) -> Result<Value, String> {
    Err(INSTALL_UNAVAILABLE.to_owned())
}

#[tauri::command]
pub(crate) async fn set_update_automatic_checks(
    engine: Engine<'_>,
    enabled: bool,
) -> Result<Value, String> {
    set_automatic_checks(&engine, enabled)
        .await
        .map_err(String::from)
}

#[tauri::command]
pub(crate) async fn open_update_release(app: AppHandle, engine: Engine<'_>) -> Result<(), String> {
    let url = state(&engine)
        .await
        .map_err(String::from)?
        .get("releaseUrl")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or("The engine reported no release page.")?;
    // The shared browser opener honours the browser setting and leaves no unreaped child.
    tauri::async_runtime::spawn_blocking(move || {
        crate::commands::applications::open_browser(&app, &url)
    })
    .await
    .map_err(|_| "The browser could not be opened.".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine_client::testing::{connected_client, FakeEngine};

    fn snapshot(phase: &str) -> Value {
        json!({"phase": phase, "currentVersion": "0.11.1", "automaticChecks": true,
            "releaseUrl": "https://example.test/releases", "canInstall": false})
    }

    #[tokio::test]
    async fn state_check_and_automatic_checks_forward_to_the_engine() {
        let fake = FakeEngine::start(|method, params| match method {
            "updates.state" => Ok(snapshot("idle")),
            "updates.check" => Ok(snapshot("available")),
            "updates.setAutomaticChecks" => {
                Ok(json!({"phase": "idle", "automaticChecks": params["enabled"]}))
            }
            _ => Ok(json!({"subscription": "sub"})),
        });
        let (engine, _) = connected_client(&fake).await;
        assert_eq!(state(&engine).await.unwrap()["phase"], "idle");
        assert_eq!(check(&engine).await.unwrap()["phase"], "available");
        let toggled = set_automatic_checks(&engine, false).await.unwrap();
        assert_eq!(toggled["automaticChecks"], false);
        assert_eq!(
            fake.calls("updates.setAutomaticChecks"),
            vec![json!({"enabled": false})]
        );
    }

    #[tokio::test]
    async fn a_download_waits_for_its_job_then_returns_the_state() {
        let fake = FakeEngine::start(|method, _| match method {
            "updates.download" => Ok(json!({"jobId": "job:5"})),
            "operations.job.get" => {
                Ok(json!({"jobId": "job:5", "state": "finished", "outcome": {"result": null}}))
            }
            "updates.state" => Ok(snapshot("downloaded")),
            _ => Ok(json!({"subscription": "sub"})),
        });
        let (engine, _) = connected_client(&fake).await;
        assert_eq!(download(&engine).await.unwrap()["phase"], "downloaded");
        assert_eq!(fake.calls("operations.job.acknowledge").len(), 1);
    }

    #[tokio::test]
    async fn an_engine_without_updates_reports_a_typed_unavailable_error() {
        let fake = FakeEngine::start(|method, _| match method {
            "updates.state" => Err((
                "unsupported_remote_operation",
                "The engine does not support updates.state.".into(),
            )),
            _ => Ok(json!({"subscription": "sub"})),
        });
        let (engine, _) = connected_client(&fake).await;
        let error = state(&engine).await.unwrap_err();
        assert_eq!(error.code, "unsupported_remote_operation");
    }

    #[tokio::test]
    async fn installing_in_process_is_not_offered() {
        assert!(install_update(true)
            .await
            .unwrap_err()
            .contains("release page"));
    }
}
