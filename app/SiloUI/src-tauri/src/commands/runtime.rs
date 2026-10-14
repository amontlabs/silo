//! Tauri adapters for the runtime engine and compatibility services used by
//! other domains while they are extracted.
use crate::runtime;
use crate::runtime::{RuntimeHost, RuntimeLifecycleOutcome, RuntimePackageKind};
use silo_engine::context::{self, EngineContext};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{utils::config::BundleType, AppHandle};

fn computer_action_worker_failure(action: &str) -> crate::bridge_error::BridgeError {
    let message = if matches!(action, "open-editor" | "open-terminal") {
        "The application launcher failed.".to_owned()
    } else {
        runtime::internal_failure("running the computer action")
    };
    crate::bridge_error::BridgeError::from(message)
}

pub(crate) struct TauriRuntimeHost(AppHandle);

pub(crate) fn host(app: &AppHandle) -> TauriRuntimeHost {
    TauriRuntimeHost(app.clone())
}

impl RuntimeHost for TauriRuntimeHost {
    fn engine_context(&self) -> std::sync::Arc<dyn EngineContext> {
        context::current().expect("the engine context is installed during app setup")
    }

    fn macos_context(&self) -> Option<silo_engine::macos_computers::MacosContext> {
        crate::commands::macos_computers::new_context(&self.0).ok()
    }

    fn runtime_storage(&self) -> Result<PathBuf, String> {
        crate::commands::runtime_migration::runtime_storage(&self.0)
    }

    fn blocks_operations(&self) -> bool {
        crate::commands::runtime_migration::blocks_operations(&self.0)
    }

    fn bundled_tools_directory(&self) -> Result<PathBuf, String> {
        crate::commands::bundled_tools::directory(&self.0)
    }

    fn ensure_runtime_ready(&self) -> Result<(), String> {
        crate::commands::runtime_migration::ensure_ready(&self.0)
    }

    fn runtime_package_kind(&self) -> RuntimePackageKind {
        package_kind(tauri::utils::platform::bundle_type())
    }

    fn schedule_secret_revocations(&self) {
        crate::secrets::schedule_revocations();
    }

    fn repository_push_operations(&self, local: Vec<serde_json::Value>) -> Vec<serde_json::Value> {
        runtime::runtime_paths(self)
            .and_then(|paths| crate::host_push_operations::merge(&paths, local.clone()))
            .unwrap_or(local)
    }

    fn github_snapshot(&self) -> Result<serde_json::Value, String> {
        crate::github::snapshot()
    }

    fn open_terminal(&self, name: &str) -> Result<(), String> {
        crate::commands::terminal::open(&self.0, name)
    }

    fn open_editor(&self, name: &str, path: Option<&str>) -> Result<(), String> {
        crate::commands::editor::open(&self.0, name, path)
    }

    fn macos_computer_names(&self) -> Vec<String> {
        crate::macos_computers::names(&self.0)
    }

    fn macos_log_target(&self, computer_id: &str) -> Option<(String, PathBuf)> {
        crate::macos_computers::log_target(&self.0, computer_id)
    }

    fn notify(&self, notice: crate::notifications::Notice) {
        deliver_runtime_notice(self, notice);
    }

    fn clear_computer_notices(&self, computer_ids: &[String]) {
        withdraw_runtime_notices(self, computer_ids);
    }

    fn stop_macos_computers(&self, deadline: Option<std::time::Instant>) -> Result<(), String> {
        crate::macos_computers::stop_all(&self.0, deadline)
    }

    fn lifecycle_notice(
        &self,
        action: &str,
        name: &str,
        computer_id: Option<String>,
        elapsed: std::time::Duration,
        outcome: RuntimeLifecycleOutcome,
    ) {
        let computer = computer_id.map(|id| crate::notifications::NoticeComputer {
            id,
            name: name.to_owned(),
        });
        let outcome = match &outcome {
            RuntimeLifecycleOutcome::Cancelled => crate::notifications::Outcome::Cancelled,
            RuntimeLifecycleOutcome::AlreadyQueued => crate::notifications::Outcome::AlreadyQueued,
            RuntimeLifecycleOutcome::Succeeded => crate::notifications::Outcome::Succeeded,
            RuntimeLifecycleOutcome::Failed(message) => {
                crate::notifications::Outcome::Failed(message)
            }
        };
        if let Some(notice) =
            crate::notifications::lifecycle_notice(action, name, computer, elapsed, outcome)
        {
            deliver_runtime_notice(self, notice);
        }
    }
}

trait RuntimeNoticeDelivery {
    fn notify_native(&self, notice: crate::notifications::Notice);
    fn clear_computer(&self, computer_id: &str);
}

impl RuntimeNoticeDelivery for TauriRuntimeHost {
    fn notify_native(&self, notice: crate::notifications::Notice) {
        crate::commands::notifications::notify_native(&self.0, notice);
    }

    fn clear_computer(&self, computer_id: &str) {
        crate::commands::notifications::clear_computer(&self.0, computer_id);
    }
}

fn deliver_runtime_notice(
    delivery: &impl RuntimeNoticeDelivery,
    notice: crate::notifications::Notice,
) {
    delivery.notify_native(notice);
}

fn withdraw_runtime_notices(delivery: &impl RuntimeNoticeDelivery, computer_ids: &[String]) {
    for computer_id in computer_ids {
        delivery.clear_computer(computer_id);
    }
}

pub(crate) fn package_kind(bundle: Option<BundleType>) -> RuntimePackageKind {
    match bundle {
        Some(BundleType::App) => RuntimePackageKind::MacApp,
        Some(BundleType::AppImage) => RuntimePackageKind::AppImage,
        Some(BundleType::Deb) => RuntimePackageKind::Deb,
        Some(BundleType::Rpm) => RuntimePackageKind::Rpm,
        _ => RuntimePackageKind::Other,
    }
}

// MARK: Commands
//
// Each command forwards to the canonical engine method. The commands keep their names and
// results so the UI is unchanged; where the engine answers with a job, the command waits for
// its outcome and returns the state the UI used to receive.

use crate::engine_client::{Engine, EngineClient, EngineError};
use serde_json::{json, Value};

pub(crate) async fn operation_queue(engine: &EngineClient) -> Result<Value, EngineError> {
    engine.call("operations.list", json!({})).await
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
        .ok_or_else(|| EngineError {
            code: "not_found".to_owned(),
            message: "That operation is no longer in the queue.".to_owned(),
        })?
        .get("jobId")
        .and_then(Value::as_str)
        .ok_or_else(|| EngineError::internal("That operation cannot be cancelled."))?
        .to_owned();
    engine.call("operations.cancel", json!({"jobId": job_id})).await?;
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
) -> Result<Vec<Value>, EngineError> {
    let mut converted = Vec::with_capacity(identities.len());
    for identity in identities {
        let name = identity
            .get("computer")
            .and_then(Value::as_str)
            .ok_or_else(|| EngineError::internal("A Git identity names no computer."))?;
        converted.push(json!({
            "computerId": engine.computer_id(name).await?,
            "name": identity.get("name").cloned().unwrap_or(Value::Null),
            "email": identity.get("email").cloned().unwrap_or(Value::Null),
            "apply": identity.get("apply").cloned().unwrap_or(Value::Bool(false)),
        }));
    }
    Ok(converted)
}

pub(crate) async fn verify_identities(
    engine: &EngineClient,
    identities: Vec<Value>,
) -> Result<bool, EngineError> {
    let identities = engine_identities(engine, identities).await?;
    let verified = engine
        .call("computers.identities.verify", json!({"identities": identities}))
        .await?;
    Ok(verified.as_bool().unwrap_or(false))
}

pub(crate) async fn configure_identities(
    engine: &EngineClient,
    identities: Vec<Value>,
) -> Result<(), EngineError> {
    let identities = engine_identities(engine, identities).await?;
    engine
        .run_job("computers.identities.configure", json!({"identities": identities}))
        .await?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn verify_computer_identities(
    engine: Engine<'_>,
    identities: Vec<Value>,
) -> Result<bool, String> {
    verify_identities(&engine, identities).await.map_err(String::from)
}

#[tauri::command]
pub(crate) async fn configure_computer_identities(
    engine: Engine<'_>,
    identities: Vec<Value>,
) -> Result<(), String> {
    configure_identities(&engine, identities).await.map_err(String::from)
}

#[tauri::command]
pub(crate) async fn read_computer_configuration(engine: Engine<'_>) -> Result<Value, String> {
    engine
        .call("computers.configuration.read", json!({}))
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
        .call("computers.shell", json!({"error": error}))
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
                .call("terminal.command", json!({"computerId": computer_id}))
                .await?;
            let settings = engine.call("settings.read", json!({})).await?;
            let settings = settings
                .get("settings")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            open_terminal(&settings, &argv(&command)?).map_err(EngineError::internal)?;
        }
        "open-editor" => {
            let mut params = json!({"computerId": computer_id});
            if let Some(path) = path {
                params["path"] = Value::String(path);
            }
            let command = engine.call("editor.command", params).await?;
            open_editor(&argv(&command)?).map_err(EngineError::internal)?;
        }
        "dismiss-error" => {
            return engine
                .call("computers.dismissError", json!({"computerId": computer_id}))
                .await;
        }
        "start" | "stop" | "restart" => {
            engine
                .run_job(
                    &format!("computers.{action}"),
                    json!({"computerId": computer_id}),
                )
                .await?;
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
        .call("activity.setup.read", json!({}))
        .await
        .map_err(String::from)
}

#[tauri::command]
pub(crate) async fn skip_computer_use_wait(engine: Engine<'_>, request_id: String) -> Result<(), String> {
    engine
        .call(
            "computers.creation.skipComputerUseWait",
            json!({"requestId": request_id}),
        )
        .await
        .map(drop)
        .map_err(String::from)
}

pub(crate) async fn retry_configuration(
    engine: &EngineClient,
    request_id: Option<String>,
    retry_computer: Option<String>,
) -> Result<Value, EngineError> {
    let mut params = json!({});
    if let Some(request_id) = request_id {
        params["requestId"] = Value::String(request_id);
    }
    if let Some(name) = retry_computer {
        params["retryComputerId"] = Value::String(engine.computer_id(&name).await?);
    }
    engine.run_job("computers.configuration.retry", params).await
}

pub(crate) async fn change_configuration(
    engine: &EngineClient,
    change: Value,
    request_id: Option<String>,
    retry_computer: Option<String>,
) -> Result<Value, EngineError> {
    let mut params = json!({"change": change});
    if let Some(request_id) = request_id {
        params["requestId"] = Value::String(request_id);
    }
    if let Some(name) = retry_computer {
        params["retryComputerId"] = Value::String(engine.computer_id(&name).await?);
    }
    engine.run_job("computers.configuration.change", params).await
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
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct RecordingNoticeDelivery {
        native: Mutex<Vec<crate::notifications::Notice>>,
        cleared: Mutex<Vec<String>>,
    }

    impl RuntimeNoticeDelivery for RecordingNoticeDelivery {
        fn notify_native(&self, notice: crate::notifications::Notice) {
            self.native.lock().unwrap().push(notice);
        }

        fn clear_computer(&self, computer_id: &str) {
            self.cleared.lock().unwrap().push(computer_id.to_owned());
        }
    }

    #[test]
    fn launcher_worker_failure_keeps_the_launcher_error() {
        for action in ["open-editor", "open-terminal"] {
            assert_eq!(
                computer_action_worker_failure(action).message,
                "The application launcher failed."
            );
        }
    }

    #[test]
    fn runtime_notice_uses_native_only_delivery() {
        let notice = crate::notifications::Notice {
            category: crate::notifications::Category::Failures,
            key: "lifecycle:dev".into(),
            title: "Couldn’t start dev".into(),
            body: "The framework rejected the start.".into(),
            computer: None,
        };
        let delivery = RecordingNoticeDelivery::default();

        deliver_runtime_notice(&delivery, notice.clone());

        assert_eq!(*delivery.native.lock().unwrap(), vec![notice]);
        assert!(delivery.cleared.lock().unwrap().is_empty());
    }

    #[test]
    fn runtime_notice_withdrawal_uses_native_notification_clear() {
        let delivery = RecordingNoticeDelivery::default();
        let computer_ids = vec!["local:dev".to_owned(), "remote:dev".to_owned()];

        withdraw_runtime_notices(&delivery, &computer_ids);

        assert_eq!(*delivery.cleared.lock().unwrap(), computer_ids);
        assert!(delivery.native.lock().unwrap().is_empty());
    }
}

#[cfg(test)]
mod forwarding_tests {
    use super::*;
    use crate::engine_client::testing::{connected_client, FakeEngine};
    use std::sync::{Arc, Mutex};

    const DEV: &str = "00000000-0000-4000-8000-00000000000d";

    fn listing() -> Value {
        json!({
            "linux": {"computers": [{"kind": "linux", "configuration": {"id": DEV, "name": "dev"}}]},
            "macos": {"computers": [{"id": "mac-1", "name": "sonoma"}]}
        })
    }

    fn engine_replies(method: &str, params: &Value) -> Result<Value, (&'static str, String)> {
        match method {
            "events.subscribe" => Ok(json!({"subscription": "sub"})),
            "computers.list" => Ok(listing()),
            "computers.start" | "computers.stop" | "computers.restart" => Ok(json!({"jobId": "job:1"})),
            "operations.job.get" => Ok(json!({"jobId": "job:1", "state": "finished", "outcome": {"result": null}})),
            "operations.list" => Ok(json!({
                "running": [{"id": 7, "jobId": "job:9"}, {"id": 8}],
                "waiting": [{"id": 9, "jobId": "job:10"}],
                "unacknowledgedJobIds": []
            })),
            "terminal.command" => Ok(json!({"argv": ["/usr/bin/ssh", "-t", "alias"]})),
            "editor.command" => Ok(json!({"argv": ["code", "--folder-uri", params["path"]]})),
            "settings.read" => Ok(json!({"settings": {"terminalApplication": "x"}})),
            "computers.dismissError" => Ok(json!({"computers": []})),
            _ => Ok(Value::Null),
        }
    }

    #[tokio::test]
    async fn the_queue_is_read_and_cancelled_through_the_entry_job() {
        let fake = FakeEngine::start(|method, params| engine_replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        assert_eq!(operation_queue(&engine).await.unwrap()["running"][0]["id"], 7);
        cancel_queue_entry(&engine, 9).await.unwrap();
        assert_eq!(fake.calls("operations.cancel"), vec![json!({"jobId": "job:10"})]);
        // The display id is never the cancellation key, and an entry without a job has none.
        assert!(cancel_queue_entry(&engine, 8).await.is_err());
        assert_eq!(cancel_queue_entry(&engine, 99).await.unwrap_err().code, "not_found");
        assert_eq!(fake.calls("operations.cancel").len(), 1);
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
        assert_eq!(fake.calls("computers.start"), vec![json!({"computerId": DEV})]);
        assert_eq!(fake.calls("operations.job.acknowledge"), vec![json!({"jobId": "job:1"})]);
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
        assert_eq!(fake.calls("terminal.command"), vec![json!({"computerId": DEV})]);

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
        assert_eq!(*editor.lock().unwrap(), vec!["code", "--folder-uri", "/workspace/app"]);
        assert_eq!(
            fake.calls("editor.command"),
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
        let state = lifecycle_action(&engine, "dismiss-error", "dev", None, |_, _| Ok(()), |_| Ok(()))
            .await
            .unwrap();
        assert_eq!(state, json!({"computers": []}));
        assert!(fake.calls("operations.job.get").is_empty());
    }

    #[tokio::test]
    async fn identities_and_configuration_changes_address_computers_by_id() {
        let fake = FakeEngine::start(|method, params| match method {
            "computers.identities.verify" => Ok(json!(true)),
            "computers.identities.configure" | "computers.configuration.retry" | "computers.configuration.change" => {
                Ok(json!({"jobId": "setup:r1"}))
            }
            "operations.job.get" => Ok(json!({"jobId": "setup:r1", "state": "finished",
                "outcome": {"result": {"computers": []}}})),
            other => engine_replies(other, params),
        });
        let (engine, _) = connected_client(&fake).await;
        let identity = json!({"computer": "dev", "name": "Ada", "email": "ada@example.com", "apply": true});
        assert!(verify_identities(&engine, vec![identity.clone()]).await.unwrap());
        configure_identities(&engine, vec![identity]).await.unwrap();
        let expected = json!([{"computerId": DEV, "name": "Ada", "email": "ada@example.com", "apply": true}]);
        assert_eq!(fake.calls("computers.identities.verify")[0]["identities"], expected);
        assert_eq!(fake.calls("computers.identities.configure")[0]["identities"], expected);

        let state = change_configuration(&engine, json!({"kind": "batch", "changes": []}), Some("r1".into()), Some("dev".into()))
            .await
            .unwrap();
        assert_eq!(state, json!({"computers": []}));
        assert_eq!(
            fake.calls("computers.configuration.change"),
            vec![json!({"change": {"kind": "batch", "changes": []}, "requestId": "r1", "retryComputerId": DEV})]
        );
        retry_configuration(&engine, Some("r1".into()), None).await.unwrap();
        assert_eq!(fake.calls("computers.configuration.retry"), vec![json!({"requestId": "r1"})]);
        assert!(verify_identities(&engine, vec![json!({"computer": "ghost"})]).await.is_err());
    }
}
