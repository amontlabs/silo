//! Tauri adapters for engine updates. The engine owns the schedule, the feed and the state
//! (`updates.*`, `updates.changed`); this app shows them and opens the release page.
//! Installing replaces the app bundle, so it follows the shared installer handoff (the same
//! sequence Fjord's `EngineHandoff` runs): take `installer.lock`, `engine.update.prepare`,
//! `engine.shutdown {reason: "update"}`, swap the bundle, restart. The lock is held until the
//! new package is in place; if the app dies first the engine's watchdog aborts the update.

use std::path::Path;

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
use tauri_plugin_updater::UpdaterExt;

use crate::engine_client::{Engine, EngineClient, EngineError};

const DEVELOPMENT_CHANNEL: &str =
    "Silo Dev is built from source and does not install releases. Build the new version instead.";
const NO_UPDATE: &str = "There is no update to install. Check for updates first.";
const INSTALLER_BUSY: &str =
    "Another app is installing a Silo update. Wait for it to finish, then try again.";

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

/// Runs the installer side of an engine update. `swap` replaces the app bundle once the
/// engine has saved its running computers and exited; it runs with the lock still held.
///
/// Everything that fails before the engine leaves aborts the prepared update, so the engine
/// keeps serving. A failure after it left cannot be undone: the lock is released and whichever
/// engine starts next restores the saved computers.
pub(crate) async fn hand_off<L>(
    engine: &EngineClient,
    take_lock: impl FnOnce() -> std::io::Result<L>,
    swap: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let _lock = take_lock().map_err(|error| match error.kind() {
        std::io::ErrorKind::WouldBlock => INSTALLER_BUSY.to_owned(),
        _ => format!("Silo could not take the installer lock: {error}"),
    })?;
    // The engine saves and holds the running computers; `resuming` is for the new engine.
    if let Err(error) = engine.run_job("engine.update.prepare", json!({})).await {
        let _ = engine.call("engine.update.abort", json!({})).await;
        return Err(String::from(error));
    }
    if let Err(error) = engine
        .run_job_until_exit(
            "engine.shutdown",
            json!({"stopComputers": false, "reason": "update"}),
        )
        .await
    {
        let _ = engine.call("engine.update.abort", json!({})).await;
        return Err(String::from(error));
    }
    swap()
}

fn engine_directory(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let home = app.path().home_dir().map_err(|error| error.to_string())?;
    Ok(silo_client::channel::current().engine_directory(&home))
}

fn locked(directory: &Path) -> impl FnOnce() -> std::io::Result<crate::installer::Held> + '_ {
    move || crate::installer::Held::take(directory)
}

#[tauri::command]
pub(crate) async fn install_update(
    app: AppHandle,
    engine: Engine<'_>,
    _stop_computers: bool,
) -> Result<Value, String> {
    if silo_client::channel::current() == silo_client::channel::Channel::Development {
        return Err(DEVELOPMENT_CHANNEL.to_owned());
    }
    // The verified package is fetched first, so the engine is down only for the swap.
    let update = app
        .updater()
        .map_err(|error| error.to_string())?
        .check()
        .await
        .map_err(|error| error.to_string())?
        .ok_or(NO_UPDATE)?;
    let package = update
        .download(|_, _| {}, || {})
        .await
        .map_err(|error| error.to_string())?;
    let directory = engine_directory(&app)?;
    hand_off(&engine, locked(&directory), || {
        update.install(package).map_err(|error| error.to_string())
    })
    .await?;
    // The new app starts its own bundled engine, which restores the saved computers.
    app.restart()
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

    fn handoff_engine(fail: Option<&'static str>) -> FakeEngine {
        FakeEngine::start(move |method, _| {
            if fail == Some(method) {
                return Err(("busy", "The engine refused.".into()));
            }
            match method {
                "engine.update.prepare" | "engine.shutdown" => Ok(json!({"jobId": "job:1"})),
                "operations.job.get" => Ok(
                    json!({"jobId": "job:1", "state": "finished", "outcome": {"result": {"resuming": []}}}),
                ),
                _ => Ok(json!({"subscription": "sub"})),
            }
        })
    }

    #[tokio::test]
    async fn the_handoff_prepares_shuts_down_for_the_update_then_swaps_under_the_lock() {
        let directory = tempfile::tempdir().unwrap();
        let fake = handoff_engine(None);
        let (engine, _) = connected_client(&fake).await;
        let mut swapped = false;
        hand_off(&engine, locked(directory.path()), || {
            // The lock is still held while the bundle is replaced.
            assert!(crate::installer::coordinator_present(directory.path()));
            swapped = true;
            Ok(())
        })
        .await
        .unwrap();
        assert!(swapped);
        assert_eq!(fake.calls("engine.update.prepare").len(), 1);
        assert_eq!(
            fake.calls("engine.shutdown"),
            vec![json!({"stopComputers": false, "reason": "update"})]
        );
        assert!(fake.calls("engine.update.abort").is_empty());
        // Dropping the guard released the lock.
        assert!(!crate::installer::coordinator_present(directory.path()));
    }

    #[tokio::test]
    async fn a_refused_prepare_aborts_and_never_shuts_down_or_swaps() {
        let directory = tempfile::tempdir().unwrap();
        let fake = handoff_engine(Some("engine.update.prepare"));
        let (engine, _) = connected_client(&fake).await;
        let error = hand_off(&engine, locked(directory.path()), || {
            panic!("the bundle must not be replaced")
        })
        .await
        .unwrap_err();
        assert!(error.contains("refused"), "{error}");
        assert_eq!(fake.calls("engine.update.abort").len(), 1);
        assert!(fake.calls("engine.shutdown").is_empty());
        assert!(!crate::installer::coordinator_present(directory.path()));
    }

    #[tokio::test]
    async fn a_refused_shutdown_aborts_and_keeps_the_bundle() {
        let directory = tempfile::tempdir().unwrap();
        let fake = handoff_engine(Some("engine.shutdown"));
        let (engine, _) = connected_client(&fake).await;
        hand_off(&engine, locked(directory.path()), || {
            panic!("the bundle must not be replaced")
        })
        .await
        .unwrap_err();
        assert_eq!(fake.calls("engine.update.abort").len(), 1);
    }

    #[tokio::test]
    async fn a_held_lock_means_another_installer_and_nothing_is_asked_of_the_engine() {
        let directory = tempfile::tempdir().unwrap();
        let _other = crate::installer::Held::take(directory.path()).unwrap();
        let fake = handoff_engine(None);
        let (engine, _) = connected_client(&fake).await;
        let error = hand_off(&engine, locked(directory.path()), || Ok(()))
            .await
            .unwrap_err();
        assert_eq!(error, INSTALLER_BUSY);
        assert!(fake.calls("engine.update.prepare").is_empty());
    }

    #[tokio::test]
    async fn a_failed_swap_after_the_engine_left_releases_the_lock_without_an_abort() {
        let directory = tempfile::tempdir().unwrap();
        let fake = handoff_engine(None);
        let (engine, _) = connected_client(&fake).await;
        let error = hand_off(
            &engine,
            locked(directory.path()),
            || Err("disk full".into()),
        )
        .await
        .unwrap_err();
        assert_eq!(error, "disk full");
        assert!(fake.calls("engine.update.abort").is_empty());
        assert!(!crate::installer::coordinator_present(directory.path()));
    }
}
