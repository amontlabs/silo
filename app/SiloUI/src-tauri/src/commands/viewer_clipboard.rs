//! Paste and Copy between this device and a Linux computer's desktop. The engine does the work
//! (device clipboard, page bridge); this client starts a transfer and reports its outcome.
use serde_json::{json, Value};
use silo_api::methods::clipboard::{ClipboardAction, ClipboardTransferResult};
use tauri::{AppHandle, Emitter, Window};

use crate::{
    commands::desktop_viewer::{connection_of, require_computer},
    engine_client::{self, Engine, EngineClient, EngineError},
};

/// The shell window's own event for a transfer a shortcut or menu item started.
pub(crate) const EVENT: &str = "desktop-clipboard";

/// The outcome the UI parses: the engine's `Report`, which has the same JSON shape.
pub(crate) type Report = ClipboardTransferResult;

fn failed(action: ClipboardAction, message: impl Into<String>) -> Report {
    use silo_api::methods::clipboard::ClipboardTransferStatus;
    Report {
        action,
        status: ClipboardTransferStatus::Failed,
        content: None,
        message: Some(message.into()),
    }
}

fn not_connected(action: ClipboardAction) -> Report {
    use silo_api::methods::clipboard::ClipboardTransferStatus;
    Report {
        action,
        status: ClipboardTransferStatus::NotConnected,
        content: None,
        message: None,
    }
}

/// Runs one transfer through the engine. A viewer with no connection reports `not-connected`
/// like a page that is gone; an engine failure is a `failed` report with its message, so the
/// UI shows it where it shows every transfer outcome.
pub(crate) async fn transfer(
    engine: &EngineClient,
    (computer_id, device_id): &(String, Option<String>),
    connection: Option<&str>,
    action: ClipboardAction,
) -> Report {
    let Some(connection) = connection else {
        return not_connected(action);
    };
    let mut params =
        json!({"computerId": computer_id, "connectionId": connection, "action": action});
    if let Some(device) = device_id {
        params["deviceId"] = json!(device);
    }
    let reply = engine.call("clipboard.transfer", params).await;
    match reply {
        Ok(value) => serde_json::from_value(value)
            .unwrap_or_else(|_| failed(action, "The engine returned an unreadable result.")),
        Err(error) => failed(action, error.message),
    }
}

/// The engine's address of the computer a shell shows: its id, and its device when remote.
async fn address(
    engine: &EngineClient,
    computer: &str,
) -> Result<(String, Option<String>), EngineError> {
    crate::commands::desktop_viewer::engine_address(engine, computer).await
}

/// Pastes the device clipboard into the computer, or copies the computer's clipboard to the
/// device. Only the viewer's own shell window can start it, from its toolbar.
#[tauri::command]
pub(crate) async fn desktop_viewer_clipboard(
    engine: Engine<'_>,
    window: Window,
    computer: String,
    action: ClipboardAction,
) -> Result<Report, String> {
    if !crate::commands::desktop_viewer::is_viewer_label(window.label()) {
        return Err("Use the clipboard from a desktop viewer.".into());
    }
    require_computer(&window, &computer)?;
    let connection = connection_of(window.label());
    let address = address(&engine, &computer).await.map_err(String::from)?;
    Ok(transfer(&engine, &address, connection.as_deref(), action).await)
}

/// Starts a transfer from a shortcut or menu item and reports the outcome to that viewer's
/// shell window.
pub(crate) fn spawn(app: &AppHandle, label: &str, action: ClipboardAction) {
    let (app, label) = (app.clone(), label.to_owned());
    tauri::async_runtime::spawn(async move {
        let engine = engine_client::of(&app);
        let report = match crate::commands::desktop_viewer::computer_of(&label) {
            Some(computer) => match address(&engine, &computer).await {
                Ok(address) => {
                    transfer(&engine, &address, connection_of(&label).as_deref(), action).await
                }
                Err(error) => failed(action, error.message),
            },
            None => not_connected(action),
        };
        let _ = app.emit_to(
            label.as_str(),
            EVENT,
            serde_json::to_value(report).unwrap_or(Value::Null),
        );
    });
}

#[cfg(test)]
mod forwarding_tests {
    use super::*;
    use crate::engine_client::testing::{connected_client, FakeEngine};
    use silo_api::methods::clipboard::{ClipboardTransferContent, ClipboardTransferStatus};

    fn comp() -> (String, Option<String>) {
        ("comp".to_owned(), None)
    }

    #[tokio::test]
    async fn a_remote_computer_is_addressed_with_its_device() {
        let fake = FakeEngine::start(|method, _| match method {
            "engine.state" | "events.subscribe" => Ok(Value::Null),
            _ => Err((
                "unsupported_remote_operation",
                "clipboard.transfer on a remote device".into(),
            )),
        });
        let (client, _) = connected_client(&fake).await;
        let remote = ("comp".to_owned(), Some("dev".to_owned()));
        let report = transfer(&client, &remote, Some("c1"), ClipboardAction::Copy).await;
        assert_eq!(report.status, ClipboardTransferStatus::Failed);
        assert_eq!(
            fake.calls("clipboard.transfer"),
            [
                json!({"computerId": "comp", "deviceId": "dev", "connectionId": "c1", "action": "copy"})
            ]
        );
    }

    #[tokio::test]
    async fn a_paste_is_forwarded_with_the_connection_and_the_report_translates() {
        let fake = FakeEngine::start(|_, _| {
            Ok(json!({"action": "paste", "status": "pasted", "content": "text", "message": null}))
        });
        let (client, _) = connected_client(&fake).await;
        let report = transfer(&client, &comp(), Some("c1"), ClipboardAction::Paste).await;
        assert_eq!(report.status, ClipboardTransferStatus::Pasted);
        assert_eq!(report.content, Some(ClipboardTransferContent::Text));
        assert_eq!(
            fake.calls("clipboard.transfer"),
            [json!({"computerId": "comp", "connectionId": "c1", "action": "paste"})]
        );
        // The shape the UI parses.
        assert_eq!(
            serde_json::to_value(report).unwrap(),
            json!({"action": "paste", "status": "pasted", "content": "text", "message": null})
        );
    }

    #[tokio::test]
    async fn every_status_the_ui_knows_survives_the_translation() {
        for status in [
            "pasted",
            "copied",
            "device-empty",
            "computer-empty",
            "too-large",
            "not-connected",
            "unsupported",
            "busy",
            "failed",
        ] {
            let fake = FakeEngine::start(move |_, _| {
                Ok(json!({"action": "copy", "status": status, "content": null, "message": null}))
            });
            let (client, _) = connected_client(&fake).await;
            let report = transfer(&client, &comp(), Some("c1"), ClipboardAction::Copy).await;
            assert_eq!(serde_json::to_value(report).unwrap()["status"], status);
        }
    }

    #[tokio::test]
    async fn no_connection_means_not_connected_without_asking_the_engine() {
        let fake = FakeEngine::start(|_, _| Ok(Value::Null));
        let (client, _) = connected_client(&fake).await;
        let report = transfer(&client, &comp(), None, ClipboardAction::Copy).await;
        assert_eq!(report.status, ClipboardTransferStatus::NotConnected);
        assert_eq!(report.action, ClipboardAction::Copy);
        assert!(fake.calls("clipboard.transfer").is_empty());
    }

    #[tokio::test]
    async fn an_engine_failure_is_a_failed_report_with_its_message() {
        let fake = FakeEngine::start(|method, _| match method {
            "engine.state" | "events.subscribe" => Ok(Value::Null),
            _ => Err(("not_found", "Unknown desktop viewer.".into())),
        });
        let (client, _) = connected_client(&fake).await;
        let report = transfer(&client, &comp(), Some("c1"), ClipboardAction::Paste).await;
        assert_eq!(report.status, ClipboardTransferStatus::Failed);
        assert_eq!(report.message.as_deref(), Some("Unknown desktop viewer."));
    }

    #[tokio::test]
    async fn an_unreadable_result_is_a_failed_report() {
        let fake = FakeEngine::start(|_, _| Ok(json!({"status": "nonsense"})));
        let (client, _) = connected_client(&fake).await;
        let report = transfer(&client, &comp(), Some("c1"), ClipboardAction::Paste).await;
        assert_eq!(report.status, ClipboardTransferStatus::Failed);
    }
}
