//! The window that shows a macOS computer running on another device. It is a `desktop-shell`
//! window that runs the noVNC viewer. The window is UI-only and lives here; the screen's
//! loopback gateway and the sign-in belong to the engine (`macos.display.session`), and the
//! resize is `macos.display.resize`.
//!
//! The window learns the computer from this registry, not from its page: the commands below
//! work on the computer the window was opened for and nothing else.
use crate::engine_client::{Engine, EngineClient, EngineError};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{Mutex, MutexGuard},
};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, Window};

const MAX_VIEWERS: usize = 8;
const NOT_A_VIEWER: &str = "This window cannot show a computer's screen.";

struct Viewer {
    device: String,
    computer: String,
}

static VIEWERS: Mutex<Option<HashMap<String, Viewer>>> = Mutex::new(None);

fn with_viewers<R>(work: impl FnOnce(&mut HashMap<String, Viewer>) -> R) -> R {
    let mut guard: MutexGuard<'_, Option<HashMap<String, Viewer>>> = VIEWERS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    work(guard.get_or_insert_with(HashMap::new))
}

/// The label of the window for one computer of one device: stable, so a second open finds it.
fn viewer_label(device: &str, computer: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(format!("{device}\n{computer}").as_bytes());
    let hex: String = digest[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("desktop-shell-macos-{hex}")
}

/// What the window's page is given: the computer's name and where it runs. Nothing secret.
fn viewer_route(device: &str, computer: &str, name: &str) -> String {
    let mut route = tauri::Url::parse("http://silo.local/index.html").expect("a static URL parses");
    route
        .query_pairs_mut()
        .append_pair("macosDisplay", computer)
        .append_pair("device", device)
        .append_pair("name", name);
    format!("index.html?{}", route.query().unwrap_or_default())
}

/// Registers the viewer for a label, or reports that it exists. `Err` is a full registry.
fn claim(
    viewers: &mut HashMap<String, Viewer>,
    label: &str,
    device: &str,
    computer: &str,
) -> Result<bool, String> {
    if viewers.contains_key(label) {
        return Ok(false);
    }
    if viewers.len() >= MAX_VIEWERS {
        return Err("Close an unused screen window first.".into());
    }
    viewers.insert(
        label.to_string(),
        Viewer {
            device: device.to_string(),
            computer: computer.to_string(),
        },
    );
    Ok(true)
}

fn target_of(window: &Window) -> Result<(String, String), String> {
    with_viewers(|viewers| {
        viewers
            .get(window.label())
            .map(|viewer| (viewer.device.clone(), viewer.computer.clone()))
    })
    .ok_or_else(|| NOT_A_VIEWER.to_string())
}

/// The name of a running computer in the owner's computer list, or why it has no screen to show.
fn running_computer_name(state: &Value, computer_id: &str) -> Result<String, String> {
    if state["supported"] != true {
        return Err(state["unsupportedReason"]
            .as_str()
            .unwrap_or("That device cannot run macOS computers.")
            .to_string());
    }
    let computer = state["computers"]
        .as_array()
        .and_then(|computers| {
            computers
                .iter()
                .find(|computer| computer["id"] == computer_id)
        })
        .ok_or("This computer no longer exists.")?;
    if computer["state"] != "running" {
        return Err("The computer is not running. Start it to show its screen.".into());
    }
    Ok(computer["name"].as_str().unwrap_or("macOS").to_string())
}

fn bad_request(message: impl Into<String>) -> EngineError {
    EngineError::not_found(message)
}

/// The window's title name: the computer and the device it runs on. The owner confirms the
/// computer, its state and that it can host macOS before a window opens.
async fn display_name(
    engine: &EngineClient,
    device_id: &str,
    computer_id: &str,
) -> Result<String, EngineError> {
    let listed = engine
        .call("computers.list", json!({"deviceId": device_id}))
        .await?;
    let name = running_computer_name(&listed["macos"], computer_id).map_err(bad_request)?;
    let devices = engine.call("connections.devices.list", json!({})).await?;
    let device_name = devices
        .as_array()
        .into_iter()
        .flatten()
        .find(|saved| saved.get("id").and_then(Value::as_str) == Some(device_id))
        .and_then(|saved| saved.get("name").and_then(Value::as_str))
        .ok_or_else(|| bad_request("This device is no longer connected."))?;
    Ok(format!("{name} · {device_name}"))
}

/// Opens the window for a running macOS computer on a connected device, or brings it forward.
#[tauri::command]
pub(crate) async fn open_macos_remote_display(
    app: AppHandle,
    engine: Engine<'_>,
    window: Window,
    device_id: String,
    computer_id: String,
) -> Result<(), EngineError> {
    if window.label() != "main" {
        return Err(EngineError::internal(
            "Open screens from the main Silo window.",
        ));
    }
    let name = display_name(&engine, &device_id, &computer_id).await?;
    tokio::task::spawn_blocking(move || open(&app, &device_id, &computer_id, &name))
        .await
        .map_err(|_| EngineError::internal("Silo could not open the screen."))?
}

fn open(
    app: &AppHandle,
    device_id: &str,
    computer_id: &str,
    name: &str,
) -> Result<(), EngineError> {
    let label = viewer_label(device_id, computer_id);
    let claimed = with_viewers(|viewers| claim(viewers, &label, device_id, computer_id))
        .map_err(EngineError::internal)?;
    if !claimed {
        // Without a window yet, another call is still creating it.
        if let Some(existing) = app.get_webview_window(&label) {
            let _ = existing.show();
            let _ = existing.set_focus();
        }
        return Ok(());
    }
    let product = silo_client::channel::current().product_name();
    let built = WebviewWindowBuilder::new(
        app,
        &label,
        WebviewUrl::App(viewer_route(device_id, computer_id, name).into()),
    )
    .title(format!("{name} — {product}"))
    .inner_size(1200., 820.)
    .min_inner_size(672., 480.)
    .build();
    match built {
        Ok(window) => {
            window.on_window_event(move |event| {
                if matches!(event, tauri::WindowEvent::Destroyed) {
                    with_viewers(|viewers| viewers.remove(&label));
                }
            });
            Ok(())
        }
        Err(_) => {
            with_viewers(|viewers| viewers.remove(&label));
            Err(EngineError::internal(
                "Silo could not open the screen window.",
            ))
        }
    }
}

/// What the viewer needs to connect: the endpoint, the account and the framebuffer size.
async fn display_session(
    engine: &EngineClient,
    device: &str,
    computer: &str,
) -> Result<Value, EngineError> {
    let session = engine
        .call(
            "macos.display.session",
            json!({"deviceId": device, "computerId": computer}),
        )
        .await?;
    let text = |name: &str| session.get(name).and_then(Value::as_str);
    let (Some(url), Some(username), Some(password)) =
        (text("url"), text("username"), text("password"))
    else {
        return Err(EngineError::internal(
            "The computer returned no sign-in for its screen.",
        ));
    };
    Ok(json!({
        "url": url,
        "username": username,
        "password": password,
        "width": session.get("width").and_then(Value::as_u64).unwrap_or(0),
        "height": session.get("height").and_then(Value::as_u64).unwrap_or(0),
    }))
}

/// Signs in for the window's computer. The engine checks it runs and has finished setup, so a
/// refusal comes back as a message the viewer shows. The password is returned to the window
/// that asked and is never logged or kept here.
#[tauri::command]
pub(crate) async fn macos_display_session(
    engine: Engine<'_>,
    window: Window,
) -> Result<Value, String> {
    let (device, computer) = target_of(&window)?;
    display_session(&engine, &device, &computer)
        .await
        .map_err(String::from)
}

async fn display_resize(
    engine: &EngineClient,
    computer: &str,
    width_px: u64,
    height_px: u64,
) -> Result<Value, EngineError> {
    engine
        .call(
            "macos.display.resize",
            json!({"computerId": computer, "widthPx": width_px, "heightPx": height_px}),
        )
        .await
}

/// Resizes the window's computer to the pixel size of the window. The engine clamps it and
/// the size it applied is returned.
#[tauri::command]
pub(crate) async fn macos_display_resize(
    engine: Engine<'_>,
    window: Window,
    width_px: u64,
    height_px: u64,
) -> Result<Value, String> {
    let (_, computer) = target_of(&window)?;
    display_resize(&engine, &computer, width_px, height_px)
        .await
        .map_err(String::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine_client::testing::{connected_client, FakeEngine};

    const DEVICE: &str = "22222222-2222-4222-8222-222222222222";
    const COMPUTER: &str = "11111111-1111-4111-8111-111111111111";

    #[test]
    fn a_computer_has_one_stable_viewer_label_that_is_a_desktop_shell() {
        let label = viewer_label(DEVICE, COMPUTER);
        assert_eq!(label, viewer_label(DEVICE, COMPUTER));
        assert_ne!(label, viewer_label(COMPUTER, DEVICE));
        assert!(label.starts_with("desktop-shell-macos-"));
    }

    #[test]
    fn the_route_carries_names_and_ids_but_nothing_secret() {
        let route = viewer_route(DEVICE, COMPUTER, "Studio mac & co");
        assert!(route.starts_with("index.html?macosDisplay="));
        assert!(route.contains(&format!("device={DEVICE}")));
        assert!(route.contains("name=Studio+mac+%26+co"));
        assert!(!route.contains("password"));
    }

    #[test]
    fn the_registry_holds_one_viewer_per_label_and_a_limited_number() {
        let mut viewers = HashMap::new();
        assert_eq!(claim(&mut viewers, "a", DEVICE, COMPUTER), Ok(true));
        assert_eq!(claim(&mut viewers, "a", DEVICE, COMPUTER), Ok(false));
        for index in 1..MAX_VIEWERS {
            assert_eq!(
                claim(&mut viewers, &format!("v{index}"), DEVICE, COMPUTER),
                Ok(true)
            );
        }
        assert!(claim(&mut viewers, "one-more", DEVICE, COMPUTER).is_err());
        assert_eq!(claim(&mut viewers, "a", DEVICE, COMPUTER), Ok(false));
    }

    #[test]
    fn only_a_running_computer_on_a_device_that_hosts_macos_opens_a_window() {
        let state = |supported: bool, computer_state: &str| {
            json!({
                "supported": supported,
                "unsupportedReason": "macOS computers need a Mac with Apple silicon.",
                "computers": [{"id": COMPUTER, "name": "mac", "state": computer_state}],
            })
        };
        assert_eq!(
            running_computer_name(&state(true, "running"), COMPUTER).unwrap(),
            "mac"
        );
        assert!(running_computer_name(&state(true, "stopped"), COMPUTER)
            .unwrap_err()
            .contains("not running"));
        assert!(running_computer_name(&state(true, "running"), "other")
            .unwrap_err()
            .contains("no longer exists"));
        assert!(running_computer_name(&state(false, "running"), COMPUTER)
            .unwrap_err()
            .contains("Apple silicon"));
    }

    #[tokio::test]
    async fn the_window_name_comes_from_the_owners_list_and_the_device() {
        let fake = FakeEngine::start(|method, _| match method {
            "computers.list" => Ok(json!({"linux": {"computers": []}, "macos": {
                "supported": true,
                "computers": [{"id": COMPUTER, "name": "mac", "state": "running"}]}})),
            "connections.devices.list" => {
                Ok(json!([{"id": DEVICE, "name": "Studio", "address": "x"}]))
            }
            "engine.state" | "events.subscribe" => Ok(Value::Null),
            _ => Err(("internal", "unexpected".into())),
        });
        let (client, _) = connected_client(&fake).await;
        assert_eq!(
            display_name(&client, DEVICE, COMPUTER).await.unwrap(),
            "mac · Studio"
        );
        assert_eq!(fake.calls("computers.list"), [json!({"deviceId": DEVICE})]);
    }

    #[tokio::test]
    async fn a_remote_device_the_engine_cannot_serve_passes_its_typed_error_through() {
        let fake = FakeEngine::start(|method, _| match method {
            "engine.state" | "events.subscribe" => Ok(Value::Null),
            _ => Err((
                "unsupported_remote_operation",
                "computers.list on a remote device".into(),
            )),
        });
        let (client, _) = connected_client(&fake).await;
        let error = display_name(&client, DEVICE, COMPUTER).await.unwrap_err();
        assert!(error.is("unsupported_remote_operation"));
    }

    #[tokio::test]
    async fn the_session_is_forwarded_and_translated_to_the_viewers_shape() {
        let fake = FakeEngine::start(|_, _| {
            Ok(
                json!({"url": "ws://127.0.0.1:5900", "username": "u", "password": "p",
                      "width": 1280, "height": 800}),
            )
        });
        let (client, _) = connected_client(&fake).await;
        let session = display_session(&client, DEVICE, COMPUTER).await.unwrap();
        assert_eq!(
            session,
            json!({"url": "ws://127.0.0.1:5900", "username": "u", "password": "p",
                   "width": 1280, "height": 800})
        );
        assert_eq!(
            fake.calls("macos.display.session"),
            [json!({"deviceId": DEVICE, "computerId": COMPUTER})]
        );
    }

    #[tokio::test]
    async fn a_session_without_sign_in_and_a_refusal_are_errors() {
        let fake = FakeEngine::start(|_, _| Ok(json!({"url": "ws://x"})));
        let (client, _) = connected_client(&fake).await;
        assert!(display_session(&client, DEVICE, COMPUTER).await.is_err());
        let refusing = FakeEngine::start(|method, _| match method {
            "engine.state" | "events.subscribe" => Ok(Value::Null),
            _ => Err((
                "unsupported_remote_operation",
                "macos.display.session on a remote device".into(),
            )),
        });
        let (client, _) = connected_client(&refusing).await;
        let error = display_session(&client, DEVICE, COMPUTER)
            .await
            .unwrap_err();
        assert!(error.is("unsupported_remote_operation"));
    }

    #[tokio::test]
    async fn the_resize_sends_pixels_and_returns_the_applied_size() {
        let fake = FakeEngine::start(|_, _| Ok(json!({"widthPx": 1280, "heightPx": 800})));
        let (client, _) = connected_client(&fake).await;
        let applied = display_resize(&client, COMPUTER, 1300, 900).await.unwrap();
        assert_eq!(applied, json!({"widthPx": 1280, "heightPx": 800}));
        assert_eq!(
            fake.calls("macos.display.resize"),
            [json!({"computerId": COMPUTER, "widthPx": 1300, "heightPx": 900})]
        );
    }
}
