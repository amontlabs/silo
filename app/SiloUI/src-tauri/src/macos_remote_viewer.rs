//! The window that shows a macOS computer running on another device. It is a `desktop-shell`
//! window that runs the noVNC viewer; the pixels arrive over a loopback WebSocket
//! (`macos_display_gateway`) that is spliced to the owner's `macos.display.stream`.
//!
//! The window learns the computer from this registry, not from its page: the commands below
//! work on the computer the window was opened for and nothing else.
use crate::{
    bridge_error::BridgeError,
    macos_display_gateway::{self as gateway, Gateway, Upstream},
    macos_remote::DISPLAY_STREAM,
    remote,
};
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, Window};

const MAX_VIEWERS: usize = 8;
const NOT_A_VIEWER: &str = "This window cannot show a computer's screen.";

struct Viewer {
    device: String,
    computer: String,
    gateway: Option<Gateway>,
}

static VIEWERS: Mutex<Option<HashMap<String, Viewer>>> = Mutex::new(None);

fn with_viewers<R>(work: impl FnOnce(&mut HashMap<String, Viewer>) -> R) -> R {
    let mut guard = VIEWERS
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

pub(crate) fn is_macos_viewer_label(label: &str) -> bool {
    label.starts_with("desktop-shell-macos-")
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
            gateway: None,
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

/// Opens the window for a running macOS computer on a connected device, or brings it forward.
#[tauri::command]
pub(crate) async fn open_macos_remote_display(
    app: AppHandle,
    window: Window,
    device_id: String,
    computer_id: String,
) -> Result<(), BridgeError> {
    if window.label() != "main" {
        return Err("Open screens from the main Silo window.".to_string().into());
    }
    tauri::async_runtime::spawn_blocking(move || open(&app, &device_id, &computer_id))
        .await
        .map_err(|_| BridgeError::from("Silo could not open the screen."))?
}

fn open(app: &AppHandle, device_id: &str, computer_id: &str) -> Result<(), BridgeError> {
    crate::runtime::shutdown::ensure_accepting_operations()?;
    let device = remote::saved_devices()?
        .into_iter()
        .find(|saved| saved.id == device_id)
        .ok_or("This device is no longer connected.")?;
    // The owner confirms the computer, its state and that it can host macOS before a window opens.
    let state = remote::call_remote_typed(app, device_id, "macos.snapshot", json!({}))?;
    let name = running_computer_name(&state, computer_id)?;
    let label = viewer_label(device_id, computer_id);
    let claimed = with_viewers(|viewers| claim(viewers, &label, device_id, computer_id))?;
    if !claimed {
        // Without a window yet, another call is still creating it.
        if let Some(existing) = app.get_webview_window(&label) {
            let _ = existing.show();
            let _ = existing.set_focus();
        }
        return Ok(());
    }
    let title = crate::desktop_viewer::viewer_title(
        &format!("{name} · {}", device.name),
        crate::channel::current(),
    );
    let route = viewer_route(device_id, computer_id, &format!("{name} · {}", device.name));
    let built = WebviewWindowBuilder::new(app, &label, WebviewUrl::App(route.into()))
        .title(title)
        .inner_size(1200., 820.)
        .min_inner_size(672., 480.)
        .build();
    match built {
        Ok(window) => {
            window.on_window_event(move |event| {
                if matches!(event, tauri::WindowEvent::Destroyed) {
                    // Dropping the entry stops its gateway.
                    let removed = with_viewers(|viewers| viewers.remove(&label));
                    drop(removed);
                }
            });
            Ok(())
        }
        Err(_) => {
            let removed = with_viewers(|viewers| viewers.remove(&label));
            drop(removed);
            Err("Silo could not open the screen window.".to_string().into())
        }
    }
}

/// The name of a running computer in the owner's snapshot, or why it has no screen to show.
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

/// What the viewer needs to connect: the endpoint, the account and the framebuffer size.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Session {
    url: String,
    username: String,
    password: String,
    width: u64,
    height: u64,
}

/// Signs in for the window's computer. The owner checks it runs and has finished setup, so a
/// refusal comes back as a message the viewer shows. The password is returned to the window
/// that asked and is never logged or kept here.
#[tauri::command]
pub(crate) async fn macos_display_session(
    app: AppHandle,
    window: Window,
) -> Result<Session, String> {
    let (device, computer) = target_of(&window)?;
    let label = window.label().to_string();
    tauri::async_runtime::spawn_blocking(move || {
        session(&app, &label, &device, &computer).map_err(|error| error.message)
    })
    .await
    .map_err(|_| "Silo could not connect to the screen.".to_string())?
}

fn session(
    app: &AppHandle,
    label: &str,
    device: &str,
    computer: &str,
) -> Result<Session, BridgeError> {
    let connect = remote::call_remote_typed(
        app,
        device,
        "macos.display.connect",
        json!({"computerId": computer}),
    )?;
    let field = |name: &str| connect[name].as_str().map(str::to_string);
    let (Some(username), Some(password)) = (field("username"), field("password")) else {
        return Err("The computer returned no sign-in for its screen."
            .to_string()
            .into());
    };
    let url = endpoint(app, label, device, computer)?;
    Ok(Session {
        url,
        username,
        password,
        width: connect["width"].as_u64().unwrap_or(0),
        height: connect["height"].as_u64().unwrap_or(0),
    })
}

/// The window's WebSocket endpoint, started on first use and again if it stopped.
fn endpoint(app: &AppHandle, label: &str, device: &str, computer: &str) -> Result<String, String> {
    let current = with_viewers(|viewers| {
        viewers
            .get(label)
            .and_then(|viewer| viewer.gateway.as_ref())
            .filter(|gateway| gateway.running())
            .map(Gateway::url)
    });
    if let Some(url) = current {
        return Ok(url);
    }
    let (device_id, computer_id) = (device.to_string(), computer.to_string());
    let open: gateway::Open = Arc::new(move || {
        let stream = remote::open_bridge_stream(
            &device_id,
            DISPLAY_STREAM,
            json!({"computerId": computer_id}),
        )
        .map_err(|error| error.message)?;
        Ok(Upstream {
            reader: Box::new(stream.output),
            writer: Box::new(stream.input),
            keep: Box::new(stream.child),
        })
    });
    let dev_url = app.config().build.dev_url.as_ref().map(ToString::to_string);
    let started = Gateway::start(open, gateway::app_origins(dev_url.as_deref()))?;
    let url = started.url();
    let kept = with_viewers(|viewers| {
        viewers.get_mut(label).map(|viewer| {
            viewer.gateway = Some(started);
        })
    });
    kept.map(|()| url).ok_or_else(|| NOT_A_VIEWER.to_string())
}

/// Resizes the window's computer to the pixel size of the window. The owner clamps it and
/// the size it applied is returned.
#[tauri::command]
pub(crate) async fn macos_display_resize(
    app: AppHandle,
    window: Window,
    width_px: u64,
    height_px: u64,
) -> Result<Value, String> {
    let (device, computer) = target_of(&window)?;
    tauri::async_runtime::spawn_blocking(move || {
        remote::call_remote_typed(
            &app,
            &device,
            "macos.display.resize",
            json!({"computerId": computer, "widthPx": width_px, "heightPx": height_px}),
        )
        .map_err(|error| error.message)
    })
    .await
    .map_err(|_| "Silo could not resize the screen.".to_string())?
}

/// Ends the screen connections of every window, as the app quits or updates.
pub(crate) fn close_all() {
    let stopped: Vec<_> = with_viewers(|viewers| {
        viewers
            .values_mut()
            .filter_map(|viewer| viewer.gateway.take())
            .collect()
    });
    drop(stopped);
}

/// Ends the screen connections of the windows for computers on a device that is no longer used.
pub(crate) fn close_device(device: &str) {
    let stopped: Vec<_> = with_viewers(|viewers| {
        viewers
            .values_mut()
            .filter(|viewer| viewer.device == device)
            .filter_map(|viewer| viewer.gateway.take())
            .collect()
    });
    drop(stopped);
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEVICE: &str = "22222222-2222-4222-8222-222222222222";
    const COMPUTER: &str = "11111111-1111-4111-8111-111111111111";

    #[test]
    fn a_computer_has_one_stable_viewer_label_that_is_a_desktop_shell() {
        let label = viewer_label(DEVICE, COMPUTER);
        assert_eq!(label, viewer_label(DEVICE, COMPUTER));
        assert_ne!(label, viewer_label(COMPUTER, DEVICE));
        assert!(crate::desktop_viewer::is_viewer_label(&label));
        assert!(is_macos_viewer_label(&label));
        assert!(!is_macos_viewer_label("desktop-shell-abc"));
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

    #[test]
    fn closing_a_device_ends_only_its_gateways() {
        let gateway = |tag: &str| {
            let open: gateway::Open = Arc::new(|| Err("closed".to_string()));
            let started = Gateway::start(open, Vec::new()).unwrap();
            (tag.to_string(), started)
        };
        let (_, first) = gateway("one");
        let (_, second) = gateway("two");
        let first_url = first.url();
        with_viewers(|viewers| {
            viewers.clear();
            viewers.insert(
                "w1".into(),
                Viewer {
                    device: DEVICE.into(),
                    computer: COMPUTER.into(),
                    gateway: Some(first),
                },
            );
            viewers.insert(
                "w2".into(),
                Viewer {
                    device: "other".into(),
                    computer: COMPUTER.into(),
                    gateway: Some(second),
                },
            );
        });
        close_device(DEVICE);
        with_viewers(|viewers| {
            assert!(viewers["w1"].gateway.is_none());
            assert!(viewers["w2"].gateway.as_ref().is_some_and(Gateway::running));
            viewers.clear();
        });
        assert!(first_url.starts_with("ws://127.0.0.1:"));
    }
}
