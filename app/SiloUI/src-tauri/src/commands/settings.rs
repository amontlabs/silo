//! Tauri adapters for settings, native appearance and Quit.
//!
//! Settings belong to the engine (`settings.read`/`update`/`flush`); this app keeps only
//! what is its own: the onboarding draft, the native appearance, and the Quit flow.

use std::{path::PathBuf, sync::Arc, time::Duration};

use serde_json::{json, Map, Value};
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};

use crate::{
    engine_client::{self, Engine, EngineClient, EngineError},
    quit::{Choice, Quit, QuitHost, QuitRequest},
};

// MARK: Client-owned draft

/// The onboarding draft of this window: client state, isolated from engine settings.
pub(crate) struct DraftStore {
    path: PathBuf,
}

impl DraftStore {
    pub(crate) fn new(directory: PathBuf) -> Self {
        Self {
            path: directory.join("onboarding-draft.json"),
        }
    }

    pub(crate) fn read(&self) -> Value {
        std::fs::read(&self.path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or(Value::Null)
    }

    pub(crate) fn write(&self, draft: &Value) -> Result<(), String> {
        let directory = self
            .path
            .parent()
            .ok_or("The onboarding draft has no folder.")?;
        std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
        let temporary = self.path.with_extension("json.tmp");
        let bytes = serde_json::to_vec(draft).map_err(|error| error.to_string())?;
        std::fs::write(&temporary, bytes).map_err(|error| error.to_string())?;
        std::fs::rename(&temporary, &self.path).map_err(|error| error.to_string())
    }
}

fn require_main(label: &str) -> Result<(), String> {
    if label == "main" {
        Ok(())
    } else {
        Err("Only the main window can change settings".into())
    }
}

/// The snapshot the webview parses: the engine's, plus this client's draft for the main window.
pub(crate) fn with_draft(mut snapshot: Value, draft: Option<Value>) -> Value {
    if let Some(object) = snapshot.as_object_mut() {
        object.insert("onboardingDraft".into(), draft.unwrap_or(Value::Null));
    }
    snapshot
}

pub(crate) async fn read_snapshot(
    engine: &EngineClient,
    drafts: &DraftStore,
    main: bool,
) -> Result<Value, EngineError> {
    let snapshot = engine.call("settings.read", json!({})).await?;
    engine.remember_settings(&snapshot);
    Ok(with_draft(snapshot, main.then(|| drafts.read())))
}

pub(crate) async fn update_snapshot(
    engine: &EngineClient,
    drafts: &DraftStore,
    patch: Map<String, Value>,
) -> Result<Value, EngineError> {
    let snapshot = engine.call("settings.update", json!({"patch": patch})).await?;
    engine.remember_settings(&snapshot);
    Ok(with_draft(snapshot, Some(drafts.read())))
}

/// Native glass, titlebars, dialogs and both webviews inherit the app appearance; no theme
/// clears an explicit appearance so System follows the OS again.
pub(crate) fn apply_theme(app: &AppHandle, snapshot: &Value) {
    let theme = snapshot
        .pointer("/settings/theme")
        .and_then(Value::as_str)
        .and_then(|theme| match theme {
            "light" => Some(tauri::Theme::Light),
            "dark" => Some(tauri::Theme::Dark),
            _ => None,
        });
    app.set_theme(theme);
}

#[tauri::command]
pub(crate) async fn initialize_settings(
    app: AppHandle,
    engine: Engine<'_>,
    drafts: tauri::State<'_, DraftStore>,
    window: WebviewWindow,
) -> Result<Value, String> {
    require_main(window.label())?;
    let snapshot = read_snapshot(&engine, &drafts, true).await.map_err(String::from)?;
    apply_theme(&app, &snapshot);
    Ok(snapshot)
}

#[tauri::command]
pub(crate) async fn read_settings(
    engine: Engine<'_>,
    drafts: tauri::State<'_, DraftStore>,
    window: WebviewWindow,
) -> Result<Value, String> {
    read_snapshot(&engine, &drafts, window.label() == "main")
        .await
        .map_err(String::from)
}

#[tauri::command]
pub(crate) async fn update_settings(
    app: AppHandle,
    engine: Engine<'_>,
    drafts: tauri::State<'_, DraftStore>,
    window: WebviewWindow,
    patch: Map<String, Value>,
) -> Result<Value, String> {
    require_main(window.label())?;
    let snapshot = update_snapshot(&engine, &drafts, patch).await.map_err(String::from)?;
    apply_theme(&app, &snapshot);
    Ok(snapshot)
}

#[tauri::command]
pub(crate) async fn update_onboarding_draft(
    engine: Engine<'_>,
    drafts: tauri::State<'_, DraftStore>,
    window: WebviewWindow,
    draft: Value,
) -> Result<Value, String> {
    require_main(window.label())?;
    drafts.write(&draft)?;
    read_snapshot(&engine, &drafts, true).await.map_err(String::from)
}

#[tauri::command]
pub(crate) async fn flush_settings(engine: Engine<'_>, window: WebviewWindow) -> Result<(), String> {
    require_main(window.label())?;
    engine
        .call("settings.flush", json!({}))
        .await
        .map(drop)
        .map_err(String::from)
}

// MARK: Quit

struct TauriQuitHost {
    app: AppHandle,
}

impl TauriQuitHost {
    fn engine(&self) -> Arc<EngineClient> {
        engine_client::of(&self.app)
    }

    fn show_main(&self) {
        if let Some(window) = self.app.get_webview_window("main") {
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_focus();
        }
    }
}

/// Names of the computers that are running or starting in a `computers.list` result.
pub(crate) fn running_names(listed: &Value) -> Vec<String> {
    let linux = listed
        .pointer("/linux/computers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|computer| (computer.get("configuration").unwrap_or(&Value::Null), computer));
    let macos = listed
        .pointer("/macos/computers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|computer| (computer, computer));
    linux
        .chain(macos)
        .filter(|(_, row)| {
            matches!(
                row.get("state").and_then(Value::as_str),
                Some("running" | "starting")
            )
        })
        .filter_map(|(identity, _)| identity.get("name").and_then(Value::as_str))
        .map(str::to_owned)
        .collect()
}

impl QuitHost for TauriQuitHost {
    fn running_computers(&self) -> Result<Vec<String>, String> {
        let engine = self.engine();
        tauri::async_runtime::block_on(async move {
            engine
                .call("computers.list", json!({}))
                .await
                .map(|listed| running_names(&listed))
                .map_err(String::from)
        })
    }

    fn ask(&self, request: &QuitRequest) -> bool {
        if self.app.get_webview_window("main").is_none() {
            return false;
        }
        self.show_main();
        self.app.emit_to("main", "silo://quit-requested", request).is_ok()
    }

    fn request_flush(&self) {
        if self.app.get_webview_window("main").is_some() {
            crate::status_panel::report(self.app.emit_to("main", "settings:flush-request", ()));
        }
    }

    fn announce(&self, shutting_down: bool) {
        let _ = self.app.emit("silo://shutdown-state-changed", shutting_down);
    }

    fn stop_computers(&self) -> Result<(), String> {
        let engine = self.engine();
        tauri::async_runtime::block_on(async move {
            engine
                .run_job_until_exit(
                    "engine.shutdown",
                    json!({"stopComputers": true, "reason": "user"}),
                )
                .await
                .map(drop)
                .map_err(String::from)
        })
    }

    fn flush_settings(&self) -> Result<(), String> {
        let engine = self.engine();
        tauri::async_runtime::block_on(async move {
            match engine.call("settings.flush", json!({})).await {
                Ok(_) => Ok(()),
                // No engine, nothing unsaved: this app holds no settings of its own to lose.
                Err(error) if error.unreachable => Ok(()),
                Err(error) => Err(error.message),
            }
        })
    }

    fn exit(&self) {
        crate::commands::system_shutdown::exit(&self.app);
    }

    fn cancelled(&self) {
        crate::commands::system_shutdown::cancel(&self.app);
    }

    fn explain(&self, message: String) {
        if let Some(window) = self.app.get_webview_window("main") {
            self.show_main();
            let _ = crate::commands::system_integrations::show_integration_error(
                self.app.clone(),
                window,
                message,
            );
        }
    }
}

/// Builds the Quit controller and registers it as managed state.
pub(crate) fn install_quit(app: &AppHandle) {
    app.manage(Quit::new(Arc::new(TauriQuitHost { app: app.clone() })));
}

fn quit_of(app: &AppHandle) -> Arc<Quit> {
    app.state::<Arc<Quit>>().inner().clone()
}

/// Every user Quit entry point delegates here.
pub(crate) fn request_quit(app: &AppHandle) {
    quit_of(app).request();
}

/// Quit after the user already chose what happens to running computers (the tray asked).
pub(crate) fn quit_with(app: &AppHandle, choice: Choice) {
    quit_of(app).quit_chosen(choice);
}

/// Logout, restart, shutdown or SIGTERM: no prompt, and the engine ends its own session.
pub(crate) fn end_session(app: &AppHandle, budget: Duration) {
    quit_of(app).end_session(budget);
}

/// Whether AppKit should wait for Silo's Quit path instead of terminating now.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn accepts_terminate_request(app: &AppHandle) -> bool {
    app.try_state::<Arc<Quit>>()
        .is_some_and(|quit| quit.accepts_terminate_request())
}

/// `RunEvent::ExitRequested`: hold the exit until settings are saved, unless it is a restart
/// or the Quit path already approved it. Exiting never stops computers by itself.
pub(crate) fn prevent_exit_until_saved(
    app: &AppHandle,
    api: &tauri::ExitRequestApi,
    code: Option<i32>,
) {
    if code == Some(tauri::RESTART_EXIT_CODE) {
        return;
    }
    let Some(quit) = app.try_state::<Arc<Quit>>() else {
        return;
    };
    if quit.exit_approved() {
        return;
    }
    api.prevent_exit();
    quit.request();
}

#[tauri::command]
pub(crate) fn read_shutdown_state(app: AppHandle) -> bool {
    app.try_state::<Arc<Quit>>().is_some_and(|quit| quit.active())
}

#[tauri::command]
pub(crate) fn enable_quit_confirmation(app: AppHandle, window: WebviewWindow) -> Result<(), String> {
    require_main(window.label())?;
    quit_of(&app).enable_confirmation();
    Ok(())
}

#[tauri::command]
pub(crate) fn answer_quit_request(
    app: AppHandle,
    window: WebviewWindow,
    request_id: u64,
    choice: String,
) -> Result<(), String> {
    require_main(window.label())?;
    let choice = Choice::parse(&choice).ok_or("Silo did not understand that answer to Quit.")?;
    quit_of(&app).answer(request_id, choice)
}

#[tauri::command]
pub(crate) fn begin_settings_flush(app: AppHandle, window: WebviewWindow) -> Result<(), String> {
    require_main(window.label())?;
    quit_of(&app).begin_flush()
}

#[tauri::command]
pub(crate) fn cancel_settings_flush(app: AppHandle, window: WebviewWindow) -> Result<(), String> {
    require_main(window.label())?;
    quit_of(&app).cancel_flush();
    Ok(())
}

#[tauri::command]
pub(crate) async fn complete_settings_flush(
    app: AppHandle,
    window: WebviewWindow,
) -> Result<(), String> {
    require_main(window.label())?;
    let quit = quit_of(&app);
    tokio::task::spawn_blocking(move || quit.complete_flush())
        .await
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine_client::testing::{connected_client, FakeEngine};

    fn temporary() -> (tempfile::TempDir, DraftStore) {
        let directory = tempfile::tempdir().unwrap();
        let store = DraftStore::new(directory.path().to_path_buf());
        (directory, store)
    }

    fn engine_replies(method: &str, params: &Value) -> Result<Value, (&'static str, String)> {
        match method {
            "settings.read" => Ok(json!({"revision": 4, "settings": {"theme": "dark"},
                "onboardingDraft": null, "saveError": null, "writeProtected": false})),
            "settings.update" => Ok(json!({"revision": 5, "settings": params["patch"].clone(),
                "onboardingDraft": null, "saveError": null, "writeProtected": false})),
            "settings.flush" => Ok(Value::Null),
            _ => Ok(json!({"subscription": "sub"})),
        }
    }

    #[test]
    fn the_draft_is_client_state_that_survives_and_defaults_to_null() {
        let (_directory, store) = temporary();
        assert_eq!(store.read(), Value::Null);
        store.write(&json!({"step": 2})).unwrap();
        assert_eq!(store.read(), json!({"step": 2}));
        std::fs::write(&store.path, b"not json").unwrap();
        assert_eq!(store.read(), Value::Null);
    }

    #[tokio::test]
    async fn the_snapshot_is_the_engines_plus_this_clients_draft_for_the_main_window_only() {
        let fake = FakeEngine::start(|method, params| engine_replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        let (_directory, store) = temporary();
        store.write(&json!({"step": 3})).unwrap();
        let main = read_snapshot(&engine, &store, true).await.unwrap();
        assert_eq!(main["revision"], 4);
        assert_eq!(main["onboardingDraft"], json!({"step": 3}));
        let other = read_snapshot(&engine, &store, false).await.unwrap();
        assert_eq!(other["onboardingDraft"], Value::Null);
        // The settings are remembered for callers that need them synchronously.
        assert_eq!(engine.cached_settings()["theme"], "dark");
    }

    #[tokio::test]
    async fn an_update_sends_the_patch_to_the_engine_and_returns_the_new_snapshot() {
        let fake = FakeEngine::start(|method, params| engine_replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        let (_directory, store) = temporary();
        let mut patch = Map::new();
        patch.insert("reduceMotion".into(), json!(true));
        let snapshot = update_snapshot(&engine, &store, patch).await.unwrap();
        assert_eq!(snapshot["revision"], 5);
        assert_eq!(fake.calls("settings.update"), vec![json!({"patch": {"reduceMotion": true}})]);
    }

    #[tokio::test]
    async fn a_disconnected_engine_is_a_save_error_not_a_silent_drop() {
        let fake = FakeEngine::start(|method, params| match method {
            "settings.update" => Err(("busy", "The settings file is locked.".into())),
            other => engine_replies(other, params),
        });
        let (engine, _) = connected_client(&fake).await;
        let (_directory, store) = temporary();
        let error = update_snapshot(&engine, &store, Map::new()).await.unwrap_err();
        assert_eq!((error.code.as_str(), error.message.as_str()), ("busy", "The settings file is locked."));
    }

    #[test]
    fn settings_changes_are_limited_to_the_main_window() {
        assert!(require_main("main").is_ok());
        assert_eq!(
            require_main("status"),
            Err("Only the main window can change settings".into())
        );
    }

    #[test]
    fn running_and_starting_computers_of_both_kinds_are_named() {
        let listed = json!({
            "linux": {"computers": [
                {"configuration": {"id": "1", "name": "dev"}, "state": "running"},
                {"configuration": {"id": "2", "name": "idle"}, "state": "stopped"},
                {"configuration": {"id": "3", "name": "booting"}, "state": "starting"}]},
            "macos": {"computers": [
                {"id": "m", "name": "sonoma", "state": "running"},
                {"id": "n", "name": "off", "state": "stopped"}]}
        });
        assert_eq!(running_names(&listed), ["dev", "booting", "sonoma"]);
        assert!(running_names(&json!({})).is_empty());
    }
}
