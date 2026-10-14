//! Relays engine events to the webview's existing `silo://` listeners.
//!
//! The raw frame always goes out as `silo://engine-event`. The UI keeps its established
//! listeners, so each canonical event also produces the hint or payload those listeners
//! read: most only trigger a re-read through the forwarding commands.

use serde_json::{json, Value};

/// Every event the client subscribes to. One subscription carries them all.
pub(crate) const SUBSCRIBED: &[&str] = &[
    "computers.*",
    "checkpoints.*",
    "exports.*",
    "migration.*",
    "github.*",
    "secrets.*",
    "push.*",
    "operations.*",
    "network.*",
    "editor.*",
    "engine.*",
    "settings.*",
    "updates.*",
    "preparation.*",
    "notifications.*",
    "connections.*",
    "display.*",
];

pub(crate) const ENGINE_EVENT: &str = "silo://engine-event";
pub(crate) const ENGINE_CONNECTION: &str = "silo://engine-connection";
pub(crate) const APPLICATION_STATE: &str = "silo://application-state-changed";

/// The webview events one engine frame stands for, in emission order.
pub(crate) fn legacy_events(frame: &Value) -> Vec<(&'static str, Value)> {
    let Some(name) = frame.get("event").and_then(Value::as_str) else {
        return Vec::new();
    };
    let payload = frame.get("payload").cloned().unwrap_or(Value::Null);
    match name {
        "computers.changed" => {
            let mut events = vec![(APPLICATION_STATE, Value::Null)];
            if let Some(macos) = payload.get("macos").filter(|value| !value.is_null()) {
                events.push(("silo://macos-computers-changed", macos.clone()));
            }
            events
        }
        "computers.unexpectedChange"
        | "checkpoints.changed"
        | "exports.changed"
        | "migration.changed"
        | "github.changed"
        | "secrets.changed"
        | "push.changed"
        | "operations.job.finished" => vec![(APPLICATION_STATE, Value::Null)],
        "operations.queue.changed" => vec![("silo://operation-queue-changed", Value::Null)],
        "network.changed" => vec![("silo://network-state-changed", Value::Null)],
        "migration.preUpgradeBackup.changed" => {
            vec![("silo://pre-upgrade-backup-changed", Value::Null)]
        }
        "editor.includeNotice.changed" => vec![("silo://editor-include-changed", Value::Null)],
        "engine.shutdown.changed" => vec![(
            "silo://shutdown-state-changed",
            payload
                .get("shuttingDown")
                .cloned()
                .unwrap_or(Value::Bool(false)),
        )],
        "settings.changed" => {
            let mut snapshot = payload;
            if let Some(object) = snapshot.as_object_mut() {
                object.entry("onboardingDraft").or_insert(Value::Null);
            }
            vec![("settings:changed", snapshot)]
        }
        "updates.changed" => vec![("silo://update-state", payload)],
        "preparation.changed" => vec![("silo://preparation-status", payload)],
        "preparation.chatgpt.changed" => vec![("chatgpt-app-status", payload)],
        "notifications.notice" => vec![("silo://notice", payload)],
        "display.viewer.page" => vec![("silo://desktop-viewer-page", payload)],
        "operations.job.progress" => progress_events(&payload),
        _ => Vec::new(),
    }
}

fn progress_events(payload: &Value) -> Vec<(&'static str, Value)> {
    let data = payload.get("data").cloned().unwrap_or(Value::Null);
    match payload.get("source").and_then(Value::as_str) {
        Some("setup") => vec![("silo://computer-configuration-progress", data)],
        Some("lifecycle") => vec![("silo://lifecycle-progress", data)],
        Some("transfer") => vec![("silo://transfer-progress", data)],
        Some("macos" | "backup" | "push") => vec![(APPLICATION_STATE, Value::Null)],
        _ => Vec::new(),
    }
}

/// The payload of `silo://engine-connection`.
pub(crate) fn connection_payload(connected: bool, message: Option<&str>) -> Value {
    json!({"connected": connected, "message": message})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(event: &str, payload: Value) -> Value {
        json!({"event": event, "subscription": "s", "deviceId": "d", "seq": 1, "payload": payload})
    }

    #[test]
    fn catalogue_events_map_to_the_established_listeners() {
        let names = |events: Vec<(&str, Value)>| -> Vec<String> {
            events
                .into_iter()
                .map(|(name, _)| name.to_owned())
                .collect()
        };
        assert_eq!(
            names(legacy_events(&frame(
                "computers.changed",
                json!({"linux": {}})
            ))),
            vec!["silo://application-state-changed"]
        );
        let with_macos = legacy_events(&frame(
            "computers.changed",
            json!({"macos": {"computers": []}}),
        ));
        assert_eq!(with_macos[1].0, "silo://macos-computers-changed");
        assert_eq!(with_macos[1].1, json!({"computers": []}));
        assert_eq!(
            names(legacy_events(&frame(
                "operations.queue.changed",
                Value::Null
            ))),
            vec!["silo://operation-queue-changed"]
        );
        assert_eq!(
            legacy_events(&frame(
                "engine.shutdown.changed",
                json!({"shuttingDown": true})
            )),
            vec![("silo://shutdown-state-changed", json!(true))]
        );
        assert_eq!(
            legacy_events(&frame("updates.changed", json!({"state": "idle"}))),
            vec![("silo://update-state", json!({"state": "idle"}))]
        );
        assert_eq!(
            names(legacy_events(&frame(
                "notifications.notice",
                json!({"key": "k"})
            ))),
            vec!["silo://notice"]
        );
    }

    #[test]
    fn job_progress_goes_to_the_listener_for_its_source() {
        let progress = |source: &str| {
            legacy_events(&frame(
                "operations.job.progress",
                json!({"source": source, "jobId": "job:1", "data": {"step": "x"}}),
            ))
        };
        assert_eq!(
            progress("setup"),
            vec![(
                "silo://computer-configuration-progress",
                json!({"step": "x"})
            )]
        );
        assert_eq!(progress("lifecycle")[0].0, "silo://lifecycle-progress");
        assert_eq!(progress("transfer")[0].0, "silo://transfer-progress");
        assert_eq!(progress("backup")[0].0, "silo://application-state-changed");
        assert!(progress("update").is_empty());
    }

    #[test]
    fn settings_events_carry_the_snapshot_shape_the_store_parses() {
        let events = legacy_events(&frame(
            "settings.changed",
            json!({"revision": 3, "settings": {}}),
        ));
        assert_eq!(events[0].0, "settings:changed");
        assert_eq!(events[0].1["onboardingDraft"], Value::Null);
        assert_eq!(events[0].1["revision"], 3);
    }

    #[test]
    fn frames_without_an_event_name_and_unmapped_events_produce_nothing() {
        assert!(legacy_events(&json!({"id": "x"})).is_empty());
        assert!(legacy_events(&frame("guest.files.changed", Value::Null)).is_empty());
    }

    #[test]
    fn every_subscribed_prefix_selects_a_known_event() {
        for selector in SUBSCRIBED {
            let prefix = selector.trim_end_matches('*');
            assert!(
                silo_api::events::EVENT_NAMES
                    .iter()
                    .any(|name| name.starts_with(prefix)),
                "{selector}"
            );
        }
    }
}
