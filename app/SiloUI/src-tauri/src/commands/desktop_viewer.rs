//! Desktop viewer windows: the Tauri half of a Linux computer's screen.
//!
//! The window shell and the guest webview are UI-only and stay here. The connection (tunnel,
//! loopback proxy, viewer registry, page-bridge nonces) belongs to `silod`: attaching asks
//! `display.viewer.open` for a connection and builds the guest webview around what it returns,
//! detaching asks `display.viewer.close`. This file holds a small registry of the shells
//! (window label -> computer, current connection) and never runs engine work.
use std::{
    collections::HashMap,
    sync::{Mutex, MutexGuard, OnceLock},
};

use serde_json::{json, Value};
use silo_api::methods::display::ViewerOpenResult;
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, WebviewBuilder, WebviewUrl,
    WebviewWindowBuilder, Window,
};

use crate::engine_client::{self, Engine, EngineClient, EngineError};

const VIEWER_PAGE_EVENT: &str = "silo://desktop-viewer-page";
const MAX_SHELLS: usize = 16;
const GUEST_PREFIX: &str = "guest-";

// MARK: Geometry and titles

pub(crate) fn is_viewer_label(label: &str) -> bool {
    label.starts_with("desktop-shell-")
}

fn guest_label(shell: &str) -> String {
    format!("{GUEST_PREFIX}{shell}")
}

fn viewer_title(name: &str, product: &str) -> String {
    format!("{name} — {product}")
}

/// WKWebView can inset its CSS viewport below the titlebar while its native
/// frame (and Tao's content frame) still includes that area. Measure that gap.
#[cfg(any(target_os = "macos", test))]
fn viewport_inset(shell_height: u32, scale: f64, viewport_height: f64) -> Result<f64, String> {
    if !scale.is_finite() || scale <= 0. || !viewport_height.is_finite() || viewport_height <= 0. {
        return Err("Could not determine desktop viewport geometry.".into());
    }
    Ok((f64::from(shell_height) / scale - viewport_height).max(0.))
}

/// CSS bounds originate in the shell webview, while native children use the
/// window's coordinate system. Convert the native content offset exactly once.
fn desktop_position(
    shell_x: i32,
    shell_y: i32,
    scale: f64,
    css_x: f64,
    css_y: f64,
) -> Result<(f64, f64), String> {
    if !scale.is_finite() || scale <= 0. {
        return Err("Could not determine desktop display scale.".into());
    }
    Ok((
        f64::from(shell_x) / scale + css_x,
        f64::from(shell_y) / scale + css_y,
    ))
}

fn valid_bounds(x: f64, y: f64, width: f64, height: f64, viewport_height: f64) -> bool {
    [x, y, width, height, viewport_height]
        .iter()
        .all(|value| value.is_finite())
        && x >= 0.
        && y >= 0.
        && (1. ..=16384.).contains(&width)
        && (1. ..=16384.).contains(&height)
        && (1. ..=16384.).contains(&viewport_height)
}

// MARK: Shell registry

/// What a window needs to know about its computer: the shell's identity, never the connection
/// itself, which lives in the engine.
struct Shell {
    computer: String,
    connection: Option<String>,
    connecting: bool,
}

pub(crate) enum Claim {
    /// A shell for this computer exists or is being created.
    Existing(String),
    /// The caller reserved this label and must create its window.
    New(String),
}

#[derive(Debug, PartialEq, Eq)]
enum AttachPlan {
    /// The guest webview is live: only move it.
    Resize,
    /// Connect; `stale` is the connection this shell held, to close first.
    Connect { stale: Option<String> },
}

#[derive(Default)]
pub(crate) struct Shells {
    entries: HashMap<String, Shell>,
}

impl Shells {
    /// Reserves one shell per computer, so a concurrent second open finds the pending
    /// entry instead of creating a duplicate window.
    pub(crate) fn claim(&mut self, computer: &str) -> Result<Claim, String> {
        if let Some((label, _)) = self
            .entries
            .iter()
            .find(|(_, shell)| shell.computer == computer)
        {
            return Ok(Claim::Existing(label.clone()));
        }
        if self.entries.len() >= MAX_SHELLS {
            return Err("Close an unused desktop viewer first.".into());
        }
        let label = format!("desktop-shell-{}", uuid::Uuid::new_v4().simple());
        self.entries.insert(
            label.clone(),
            Shell {
                computer: computer.to_owned(),
                connection: None,
                connecting: false,
            },
        );
        Ok(Claim::New(label))
    }

    fn allows(&self, label: &str, computer: &str) -> Result<(), String> {
        if label == "main" {
            return Ok(());
        }
        if self
            .entries
            .get(label)
            .is_some_and(|shell| shell.computer == computer)
        {
            Ok(())
        } else {
            Err("This window cannot access that desktop.".into())
        }
    }

    fn begin_attach(
        &mut self,
        label: &str,
        computer: &str,
        has_view: bool,
    ) -> Result<AttachPlan, String> {
        let shell = self
            .entries
            .get_mut(label)
            .filter(|shell| shell.computer == computer)
            .ok_or("Desktop viewer closed.")?;
        if shell.connecting {
            return Err("The desktop is still connecting.".into());
        }
        if has_view && shell.connection.is_some() {
            return Ok(AttachPlan::Resize);
        }
        shell.connecting = true;
        Ok(AttachPlan::Connect {
            stale: shell.connection.take(),
        })
    }

    /// Records the connection the engine opened. `Err` when the window went away meanwhile.
    fn set_connection(&mut self, label: &str, connection: &str) -> Result<(), String> {
        let shell = self
            .entries
            .get_mut(label)
            .ok_or("Desktop viewer closed.")?;
        shell.connection = Some(connection.to_owned());
        Ok(())
    }

    fn finish_attach(&mut self, label: &str) {
        if let Some(shell) = self.entries.get_mut(label) {
            shell.connecting = false;
        }
    }

    /// Ends an attach that failed, returning the connection to close, if one was recorded.
    fn abort_attach(&mut self, label: &str) -> Option<String> {
        let shell = self.entries.get_mut(label)?;
        shell.connecting = false;
        shell.connection.take()
    }

    fn take_connection(&mut self, label: &str) -> Option<String> {
        self.entries.get_mut(label)?.connection.take()
    }

    pub(crate) fn connection(&self, label: &str) -> Option<String> {
        self.entries.get(label)?.connection.clone()
    }

    /// Forgets a shell, returning its connection to close.
    fn remove(&mut self, label: &str) -> Option<String> {
        self.entries.remove(label)?.connection
    }

    fn label_for_connection(&self, connection: &str) -> Option<String> {
        self.entries
            .iter()
            .find(|(_, shell)| shell.connection.as_deref() == Some(connection))
            .map(|(label, _)| label.clone())
    }

    fn contains(&self, label: &str) -> bool {
        self.entries.contains_key(label)
    }
}

fn shells() -> MutexGuard<'static, Shells> {
    static SHELLS: OnceLock<Mutex<Shells>> = OnceLock::new();
    SHELLS
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Whether `label` is a live Linux desktop viewer shell of this app.
pub(crate) fn is_shell(label: &str) -> bool {
    shells().contains(label)
}

/// The engine connection the shell currently shows.
pub(crate) fn connection_of(label: &str) -> Option<String> {
    shells().connection(label)
}

/// Only the viewer's own shell window may use the viewer commands for its computer; the main
/// window is let through for the commands that take any computer.
pub(crate) fn require_computer(window: &Window, computer: &str) -> Result<(), String> {
    shells().allows(window.label(), computer)
}

/// The computer name a shell was opened for.
pub(crate) fn computer_of(label: &str) -> Option<String> {
    shells()
        .entries
        .get(label)
        .map(|shell| shell.computer.clone())
}

/// The engine's address of a shell's computer: its id, and its device when it is remote.
pub(crate) async fn engine_address(
    engine: &EngineClient,
    computer: &str,
) -> Result<(String, Option<String>), EngineError> {
    let target = target_of(engine, computer).await?;
    Ok((target.computer_id, target.device_id))
}

/// The viewer commands that act on the shell's current connection.
pub(crate) fn require_connection(window: &Window, computer: &str) -> Result<String, String> {
    if !is_viewer_label(window.label()) {
        return Err("Only a desktop viewer can use this command.".into());
    }
    require_computer(window, computer)?;
    connection_of(window.label()).ok_or_else(|| "The desktop is not connected.".to_owned())
}

// MARK: Engine calls

/// Where the engine finds a computer: its stable id, and the device when it is a remote one.
struct Target {
    computer_id: String,
    device_id: Option<String>,
}

/// A remote computer is addressed by `silo-remote:<device>:<computer>`.
fn remote_parts(computer: &str) -> Result<Option<(String, String)>, String> {
    let Some(rest) = computer.strip_prefix("silo-remote:") else {
        return Ok(None);
    };
    let valid = |value: &str| uuid::Uuid::parse_str(value).is_ok();
    match rest.split_once(':') {
        Some((device, id)) if valid(device) && valid(id) => {
            Ok(Some((device.to_owned(), id.to_owned())))
        }
        _ => Err("Invalid remote computer target.".into()),
    }
}

async fn target_of(engine: &EngineClient, computer: &str) -> Result<Target, EngineError> {
    match remote_parts(computer).map_err(EngineError::internal)? {
        Some((device, id)) => Ok(Target {
            computer_id: id,
            device_id: Some(device),
        }),
        None => Ok(Target {
            computer_id: engine.computer_id(computer).await?,
            device_id: None,
        }),
    }
}

fn target_params(target: &Target) -> Value {
    let mut params = json!({"computerId": target.computer_id});
    if let Some(device) = &target.device_id {
        params["deviceId"] = json!(device);
    }
    params
}

/// The window's identity and display name, resolved through the engine; nothing is read
/// from this process's disk.
#[derive(Debug, PartialEq, Eq)]
struct Resolved {
    stable_id: String,
    name: String,
}

async fn resolve_viewer(engine: &EngineClient, computer: &str) -> Result<Resolved, EngineError> {
    let target = target_of(engine, computer).await?;
    let Some(device) = &target.device_id else {
        return Ok(Resolved {
            stable_id: target.computer_id,
            name: computer.to_owned(),
        });
    };
    // Verify the remote identity before creating a shell.
    let status = engine
        .call("desktop.status", target_params(&target))
        .await?;
    let configuration = status
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(&target.computer_id);
    let devices = engine.call("connections.devices.list", json!({})).await?;
    let device_name = devices
        .as_array()
        .into_iter()
        .flatten()
        .find(|saved| saved.get("id").and_then(Value::as_str) == Some(device))
        .and_then(|saved| saved.get("name").and_then(Value::as_str))
        .unwrap_or(device);
    Ok(Resolved {
        stable_id: computer.to_owned(),
        name: format!("{configuration} · {device_name}"),
    })
}

async fn open_connection(
    engine: &EngineClient,
    computer: &str,
) -> Result<ViewerOpenResult, EngineError> {
    let target = target_of(engine, computer).await?;
    let opened = engine
        .call("display.viewer.open", target_params(&target))
        .await?;
    serde_json::from_value(opened)
        .map_err(|_| EngineError::internal("The engine returned an unreadable desktop connection."))
}

async fn close_connection(engine: &EngineClient, connection: &str) -> Result<(), EngineError> {
    engine
        .call("display.viewer.close", json!({"connectionId": connection}))
        .await
        .map(|_| ())
}

/// Closes a connection nobody waits for: a failed attach, a destroyed window.
pub(crate) fn close_in_background(app: &AppHandle, connection: String) {
    let engine = engine_client::of(app);
    tauri::async_runtime::spawn(async move {
        let _ = close_connection(&engine, &connection).await;
    });
}

// MARK: Guest webview

/// What the guest webview needs from the engine's answer, checked before it is used: the page
/// is only ever allowed to be the engine's loopback proxy, and the cookie only that origin's.
#[derive(Debug, PartialEq, Eq)]
struct GuestTarget {
    connection: String,
    url: tauri::Url,
    origin: String,
    cookie_name: String,
    cookie_value: String,
    cookie_domain: String,
    cookie_path: String,
    init_script: String,
}

fn guest_target(opened: ViewerOpenResult) -> Result<GuestTarget, String> {
    let url = tauri::Url::parse(&opened.url).map_err(|_| "Invalid desktop endpoint.")?;
    let loopback = url.scheme() == "http" && url.host_str() == Some("127.0.0.1");
    if !loopback || opened.cookie.domain != "127.0.0.1" || !opened.cookie.http_only {
        return Err("Invalid desktop endpoint.".into());
    }
    if opened.cookie.name.is_empty() || opened.connection_id.is_empty() {
        return Err("Missing desktop credentials.".into());
    }
    Ok(GuestTarget {
        connection: opened.connection_id,
        origin: url.origin().ascii_serialization(),
        url,
        cookie_name: opened.cookie.name,
        cookie_value: opened.cookie.value,
        cookie_domain: opened.cookie.domain,
        cookie_path: opened.cookie.path,
        init_script: opened.init_script,
    })
}

/// Evaluates a script the engine sent for one viewer in that viewer's guest page, and does
/// nothing else with it: the client relays scripts without reading answers, so the guest
/// page stays untrusted and gains no authority from this path (A-32).
pub(crate) fn deliver_page(app: &AppHandle, payload: &Value) {
    let Some((connection, script)) = page_parts(payload) else {
        return;
    };
    let Some(label) = shells().label_for_connection(connection) else {
        return;
    };
    if let Some(view) = app.get_webview(&guest_label(&label)) {
        let _ = view.eval(script);
    }
}

/// `{connectionId, script}` of a `display.viewer.page` payload. The script is always a call of
/// the page's own `__silo.invoke`; anything else is not relayed.
fn page_parts(payload: &Value) -> Option<(&str, &str)> {
    let connection = payload.get("connectionId")?.as_str()?;
    let script = payload.get("script")?.as_str()?;
    script
        .starts_with("window.__silo&&window.__silo.invoke(")
        .then_some((connection, script))
}

// MARK: Commands

#[tauri::command]
pub(crate) async fn open_desktop(
    app: AppHandle,
    engine: Engine<'_>,
    window: Window,
    computer: String,
) -> Result<(), String> {
    if window.label() != "main" {
        return Err("Open desktops from the main Silo window.".into());
    }
    let Resolved { stable_id, name } = resolve_viewer(&engine, &computer)
        .await
        .map_err(String::from)?;
    tokio::task::spawn_blocking(move || {
        let mut route = tauri::Url::parse("http://silo.local/index.html").unwrap();
        route
            .query_pairs_mut()
            .append_pair("desktop", &computer)
            .append_pair("id", &stable_id)
            .append_pair("name", &name);
        let route = format!("index.html?{}", route.query().unwrap());
        let claim = shells().claim(&computer)?;
        // Window calls run on the main thread; never make them under the lock.
        let label = match claim {
            Claim::Existing(label) => {
                // Without a window yet, another call is still creating it (G-15).
                let Some(existing) = app.get_window(&label) else {
                    return Ok(());
                };
                let _ = existing.show();
                return existing
                    .set_focus()
                    .map_err(|_| "Could not focus desktop.".into());
            }
            Claim::New(label) => label,
        };
        let product = silo_client::channel::current().product_name();
        let result = WebviewWindowBuilder::new(&app, &label, WebviewUrl::App(route.into()))
            .title(viewer_title(&name, product))
            .inner_size(1200., 820.)
            .min_inner_size(672., 480.)
            .build();
        let viewer = match result {
            Ok(v) => v,
            Err(_) => {
                shells().remove(&label);
                return Err("Could not open desktop viewer.".into());
            }
        };
        let menu_app = app.clone();
        let shortcut_label = label.clone();
        viewer.on_window_event(move |event| match event {
            tauri::WindowEvent::Destroyed => {
                crate::commands::viewer_shortcuts::uninstall(&shortcut_label);
                let others = menu_app
                    .webview_windows()
                    .into_iter()
                    .map(|(name, window)| (name, window.is_focused().unwrap_or(false)));
                crate::app_menu::set_viewer_focus(
                    &menu_app,
                    viewer_focused_besides(&shortcut_label, others),
                );
                // Never wait on the engine from the window's event handler.
                let connection = shells().remove(&shortcut_label);
                if let Some(connection) = connection {
                    close_in_background(&menu_app, connection);
                }
            }
            tauri::WindowEvent::Focused(focused) => {
                crate::app_menu::set_viewer_focus(&menu_app, *focused);
            }
            tauri::WindowEvent::DragDrop(drag) => {
                crate::commands::transfer::native_drop(&shortcut_label, drag);
            }
            _ => {}
        });
        let (shortcut_app, shortcut_window) = (app.clone(), viewer.clone());
        let _ = viewer.run_on_main_thread(move || {
            crate::commands::viewer_shortcuts::install(&shortcut_app, &shortcut_window);
        });
        Ok(())
    })
    .await
    .map_err(|_| "Desktop window operation failed.")?
}

/// Whether the clipboard menu items stay enabled after `closing` is destroyed:
/// only if another viewer window has focus.
fn viewer_focused_besides(
    closing: &str,
    windows: impl IntoIterator<Item = (String, bool)>,
) -> bool {
    windows
        .into_iter()
        .any(|(label, focused)| focused && label != closing && is_viewer_label(&label))
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn desktop_viewer_attach(
    app: AppHandle,
    engine: Engine<'_>,
    window: Window,
    computer: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    viewport_height: f64,
) -> Result<(), String> {
    require_computer(&window, &computer)?;
    if window.label() == "main" || !valid_bounds(x, y, width, height, viewport_height) {
        return Err("Invalid desktop viewer bounds.".into());
    }
    let label = window.label().to_owned();
    let geometry = {
        let (app, window) = (app.clone(), window.clone());
        tokio::task::spawn_blocking(move || {
            let shell = app
                .get_webview(window.label())
                .ok_or("Desktop viewer closed.")?;
            let shell_origin = shell
                .position()
                .map_err(|_| "Could not locate desktop viewer.")?;
            let scale = window
                .scale_factor()
                .map_err(|_| "Could not determine desktop display scale.")?;
            #[cfg(target_os = "macos")]
            let inset_y = viewport_inset(
                shell
                    .size()
                    .map_err(|_| "Could not measure desktop viewer.")?
                    .height,
                scale,
                viewport_height,
            )?;
            #[cfg(not(target_os = "macos"))]
            let inset_y = 0.;
            desktop_position(shell_origin.x, shell_origin.y, scale, x, y + inset_y)
        })
        .await
        .map_err(|_| "Desktop connection failed.")??
    };
    let existing = app.get_webview(&guest_label(&label));
    let plan = shells().begin_attach(&label, &computer, existing.is_some())?;
    let stale = match plan {
        AttachPlan::Resize => {
            let view = existing.ok_or("Desktop viewer closed.")?;
            return view
                .set_bounds(tauri::Rect {
                    position: LogicalPosition::new(geometry.0, geometry.1).into(),
                    size: LogicalSize::new(width, height).into(),
                })
                .map_err(|_| "Could not resize desktop.".into());
        }
        AttachPlan::Connect { stale } => stale,
    };
    // Every failure from here on gives the attach back and closes what the engine opened.
    let abort = |message: String| {
        let connection = shells().abort_attach(&label);
        if let Some(connection) = connection {
            close_in_background(&app, connection);
        }
        message
    };
    if let Some(stale) = stale {
        let _ = close_connection(&engine, &stale).await;
    }
    if let Some(view) = existing {
        view.close()
            .map_err(|_| abort("Could not reconnect desktop.".into()))?;
    }
    let opened = open_connection(&engine, &computer)
        .await
        .map_err(|error| abort(error.message))?;
    let target = guest_target(opened).map_err(abort)?;
    if let Err(message) = shells().set_connection(&label, &target.connection) {
        close_in_background(&app, target.connection);
        return Err(message);
    }
    let built = {
        let (app, window, label) = (app.clone(), window.clone(), label.clone());
        tokio::task::spawn_blocking(move || {
            build_guest(&app, &window, &label, target, geometry, (width, height))
        })
        .await
        .map_err(|_| abort("Desktop connection failed.".into()))?
    };
    built.map_err(abort)?;
    shells().finish_attach(&label);
    Ok(())
}

fn build_guest(
    app: &AppHandle,
    window: &Window,
    shell_label: &str,
    target: GuestTarget,
    position: (f64, f64),
    size: (f64, f64),
) -> Result<(), String> {
    let label = guest_label(shell_label);
    let permitted = target.origin.clone();
    let page_app = app.clone();
    let page_shell = shell_label.to_owned();
    let builder = WebviewBuilder::new(
        &label,
        WebviewUrl::External(tauri::Url::parse("about:blank").unwrap()),
    )
    .incognito(true)
    .focused(true)
    .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
    // Guest pages must never write to the host's Downloads folder (G-13).
    .on_download(|_, _| false)
    // The engine's bridge script locks the web clipboard and capture APIs in every frame
    // (G-20) and installs the page half of the host bridge.
    .initialization_script_for_all_frames(&target.init_script)
    .on_navigation(move |url| {
        url.as_str() == "about:blank" || url.origin().ascii_serialization() == permitted
    })
    // Selkies reloads its page after a connection failure; the shell then
    // reapplies the sound state to the replacement page.
    .on_page_load(move |_, payload| {
        if matches!(payload.event(), tauri::webview::PageLoadEvent::Finished)
            && payload.url().as_str() != "about:blank"
        {
            let _ = page_app.emit_to(page_shell.as_str(), VIEWER_PAGE_EVENT, ());
        }
    });
    let view = window
        .add_child(
            builder,
            LogicalPosition::new(position.0, position.1),
            LogicalSize::new(size.0, size.1),
        )
        .map_err(|_| "Could not create desktop display.".to_owned())?;
    let drag_label = shell_label.to_owned();
    view.on_webview_event(move |event| {
        if let tauri::WebviewEvent::DragDrop(drag) = event {
            crate::commands::transfer::native_drop(&drag_label, drag);
        }
    });
    let cookie = tauri::webview::Cookie::build((target.cookie_name, target.cookie_value))
        .domain(target.cookie_domain)
        .path(target.cookie_path)
        .http_only(true)
        .build();
    if view.set_cookie(cookie).is_err() || view.navigate(target.url).is_err() {
        let _ = view.close();
        return Err("Could not authenticate desktop viewer.".into());
    }
    view.set_focus()
        .map_err(|_| "Could not focus desktop display.".to_owned())
}

#[tauri::command]
pub(crate) async fn desktop_viewer_detach(
    app: AppHandle,
    engine: Engine<'_>,
    window: Window,
) -> Result<(), String> {
    let label = window.label().to_owned();
    let connection = shells().take_connection(&label);
    let closed = match connection {
        Some(connection) => close_connection(&engine, &connection).await,
        None => Ok(()),
    };
    if let Some(view) = app.get_webview(&guest_label(&label)) {
        let _ = view.close();
    }
    // A connection the engine no longer knows is already closed.
    match closed {
        Err(error) if !error.is("not_found") => Err(error.message),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine_client::testing::{connected_client, FakeEngine};
    use silo_api::methods::display::ViewerCookie;

    const DEVICE: &str = "22222222-2222-4222-8222-222222222222";
    const COMPUTER: &str = "11111111-1111-4111-8111-111111111111";

    #[test]
    fn macos_css_viewport_inset_matches_live_retina_geometry_and_fullscreen() {
        let inset = viewport_inset(1640, 2., 788.).unwrap();
        assert_eq!(inset, 32.);
        assert_eq!(
            desktop_position(0, 0, 2., 0., 44. + inset).unwrap(),
            (0., 76.)
        );
        assert_eq!(viewport_inset(1640, 2., 820.).unwrap(), 0.);
        assert_eq!(viewport_inset(1600, 2., 820.).unwrap(), 0.);
        for scale in [1., 1.5, 2., 3.] {
            assert_eq!(
                viewport_inset((820. * scale) as u32, scale, 788.).unwrap(),
                32.
            );
        }
        for invalid in [0., -1., f64::INFINITY, f64::NAN] {
            assert!(viewport_inset(1640, invalid, 788.).is_err());
            assert!(viewport_inset(1640, 2., invalid).is_err());
        }
    }

    #[test]
    fn child_position_translates_both_axes_without_scaling_css_twice() {
        assert_eq!(desktop_position(24, 56, 2., 8., 48.).unwrap(), (20., 76.));
        assert_eq!(desktop_position(0, 0, 2., 8., 48.).unwrap(), (8., 48.));
        for scale in [1., 1.5, 2., 3.] {
            let offset = (28. * scale) as i32;
            assert_eq!(
                desktop_position(0, offset, scale, 12., 48.).unwrap(),
                (12., 76.)
            );
        }
        for invalid in [0., -1., f64::INFINITY, f64::NAN] {
            assert!(desktop_position(0, 0, invalid, 0., 0.).is_err());
        }
    }

    #[test]
    fn bounds_must_be_finite_positive_and_bounded() {
        assert!(valid_bounds(0., 0., 800., 600., 700.));
        assert!(!valid_bounds(-1., 0., 800., 600., 700.));
        assert!(!valid_bounds(0., 0., 0.5, 600., 700.));
        assert!(!valid_bounds(0., 0., 800., 16385., 700.));
        assert!(!valid_bounds(0., 0., 800., 600., 0.));
        assert!(!valid_bounds(f64::NAN, 0., 800., 600., 700.));
    }

    #[test]
    fn window_titles_name_the_product() {
        assert_eq!(viewer_title("dev · Office", "Silo"), "dev · Office — Silo");
        assert_eq!(
            viewer_title("dev · Office", "Silo Dev"),
            "dev · Office — Silo Dev"
        );
    }

    #[test]
    fn only_desktop_shell_windows_count_as_viewers() {
        assert!(is_viewer_label("desktop-shell-0123"));
        assert!(!is_viewer_label("main"));
        assert!(!is_viewer_label("guest-desktop-shell-0123"));
        assert!(viewer_focused_besides(
            "desktop-shell-a",
            [("desktop-shell-b".to_owned(), true)]
        ));
        assert!(!viewer_focused_besides(
            "desktop-shell-a",
            [
                ("desktop-shell-a".to_owned(), true),
                ("main".to_owned(), true),
                ("desktop-shell-b".to_owned(), false)
            ]
        ));
    }

    #[test]
    fn the_registry_holds_one_shell_per_computer_and_a_limited_number() {
        let mut shells = Shells::default();
        let Ok(Claim::New(first)) = shells.claim("dev") else {
            panic!("a new shell")
        };
        assert!(is_viewer_label(&first));
        assert!(matches!(shells.claim("dev"), Ok(Claim::Existing(label)) if label == first));
        for index in 1..MAX_SHELLS {
            assert!(matches!(
                shells.claim(&format!("c{index}")),
                Ok(Claim::New(_))
            ));
        }
        assert_eq!(
            shells.claim("one-more").err().as_deref(),
            Some("Close an unused desktop viewer first.")
        );
        shells.remove(&first);
        assert!(matches!(shells.claim("one-more"), Ok(Claim::New(_))));
    }

    #[test]
    fn a_window_reaches_only_its_own_computer_and_main_reaches_any() {
        let mut shells = Shells::default();
        let Ok(Claim::New(label)) = shells.claim("dev") else {
            panic!()
        };
        assert!(shells.allows(&label, "dev").is_ok());
        assert!(shells.allows(&label, "other").is_err());
        assert!(shells.allows("desktop-shell-unknown", "dev").is_err());
        assert!(shells.allows("main", "anything").is_ok());
    }

    #[test]
    fn attaching_resizes_a_live_view_and_reconnects_otherwise() {
        let mut shells = Shells::default();
        let Ok(Claim::New(label)) = shells.claim("dev") else {
            panic!()
        };
        assert_eq!(
            shells.begin_attach(&label, "dev", false),
            Ok(AttachPlan::Connect { stale: None })
        );
        // A second attach while the first connects is refused.
        assert_eq!(
            shells.begin_attach(&label, "dev", false),
            Err("The desktop is still connecting.".to_owned())
        );
        shells.set_connection(&label, "c1").unwrap();
        shells.finish_attach(&label);
        assert_eq!(shells.connection(&label).as_deref(), Some("c1"));
        assert_eq!(
            shells.begin_attach(&label, "dev", true),
            Ok(AttachPlan::Resize)
        );
        // Without a guest view the old connection is replaced.
        assert_eq!(
            shells.begin_attach(&label, "dev", false),
            Ok(AttachPlan::Connect {
                stale: Some("c1".to_owned())
            })
        );
        assert_eq!(shells.connection(&label), None);
        assert!(shells.begin_attach(&label, "other", false).is_err());
        assert!(shells.begin_attach("missing", "dev", false).is_err());
    }

    #[test]
    fn an_aborted_attach_returns_its_connection_and_a_destroyed_shell_refuses_one() {
        let mut shells = Shells::default();
        let Ok(Claim::New(label)) = shells.claim("dev") else {
            panic!()
        };
        shells.begin_attach(&label, "dev", false).unwrap();
        shells.set_connection(&label, "c1").unwrap();
        assert_eq!(shells.abort_attach(&label).as_deref(), Some("c1"));
        assert_eq!(
            shells.begin_attach(&label, "dev", false),
            Ok(AttachPlan::Connect { stale: None })
        );
        shells.set_connection(&label, "c2").unwrap();
        assert_eq!(shells.remove(&label).as_deref(), Some("c2"));
        assert!(shells.set_connection(&label, "c3").is_err());
        assert_eq!(shells.abort_attach(&label), None);
    }

    #[test]
    fn pages_are_routed_by_connection_id() {
        let mut shells = Shells::default();
        let Ok(Claim::New(a)) = shells.claim("a") else {
            panic!()
        };
        let Ok(Claim::New(b)) = shells.claim("b") else {
            panic!()
        };
        shells.set_connection(&a, "conn-a").unwrap();
        shells.set_connection(&b, "conn-b").unwrap();
        assert_eq!(shells.label_for_connection("conn-b"), Some(b));
        assert_eq!(shells.label_for_connection("conn-a"), Some(a));
        assert_eq!(shells.label_for_connection("nope"), None);
    }

    #[test]
    fn only_a_bridge_invocation_for_a_connection_is_relayed() {
        let script = "window.__silo&&window.__silo.invoke(\"capabilities\",{});";
        assert_eq!(
            page_parts(&json!({"connectionId": "c1", "script": script})),
            Some(("c1", script))
        );
        assert_eq!(
            page_parts(&json!({"connectionId": "c1", "script": "alert(1)"})),
            None
        );
        assert_eq!(page_parts(&json!({"script": script})), None);
        assert_eq!(page_parts(&json!({"connectionId": "c1"})), None);
    }

    #[test]
    fn remote_targets_parse_and_reject_garbage() {
        assert_eq!(remote_parts("dev"), Ok(None));
        assert_eq!(
            remote_parts(&format!("silo-remote:{DEVICE}:{COMPUTER}")),
            Ok(Some((DEVICE.to_owned(), COMPUTER.to_owned())))
        );
        assert!(remote_parts("silo-remote:x:y").is_err());
        assert!(remote_parts("silo-remote:nocolon").is_err());
    }

    fn opened(url: &str, domain: &str, http_only: bool) -> ViewerOpenResult {
        ViewerOpenResult {
            connection_id: "c1".into(),
            url: url.into(),
            cookie: ViewerCookie {
                name: "silo".into(),
                value: "secret".into(),
                domain: domain.into(),
                path: "/".into(),
                http_only,
            },
            init_script: "/* bridge */".into(),
            name: "dev".into(),
        }
    }

    #[test]
    fn the_guest_only_ever_navigates_to_the_loopback_proxy() {
        let target = guest_target(opened("http://127.0.0.1:41000/", "127.0.0.1", true)).unwrap();
        assert_eq!(target.origin, "http://127.0.0.1:41000");
        assert_eq!(target.connection, "c1");
        assert_eq!(target.init_script, "/* bridge */");
        for bad in [
            opened("http://example.com:41000/", "127.0.0.1", true),
            opened("https://127.0.0.1:41000/", "127.0.0.1", true),
            opened("http://127.0.0.1:41000/", "example.com", true),
            opened("http://127.0.0.1:41000/", "127.0.0.1", false),
            opened("not a url", "127.0.0.1", true),
        ] {
            assert!(guest_target(bad).is_err());
        }
    }

    fn computers_list() -> Value {
        json!({"linux": {"computers": [{"configuration": {"id": COMPUTER, "name": "dev"}}]},
               "macos": {"computers": []}})
    }

    #[tokio::test]
    async fn a_local_viewer_is_resolved_through_computers_list() {
        let fake = FakeEngine::start(|method, _| match method {
            "computers.list" => Ok(computers_list()),
            _ => Ok(Value::Null),
        });
        let (client, _) = connected_client(&fake).await;
        let resolved = resolve_viewer(&client, "dev").await.unwrap();
        assert_eq!(
            resolved,
            Resolved {
                stable_id: COMPUTER.to_owned(),
                name: "dev".to_owned()
            }
        );
        let error = resolve_viewer(&client, "gone").await.unwrap_err();
        assert!(error.is("not_found"));
    }

    #[tokio::test]
    async fn a_remote_viewer_is_named_by_the_engine_and_its_device() {
        let fake = FakeEngine::start(|method, _| match method {
            "desktop.status" => Ok(json!({"name": "build box"})),
            "connections.devices.list" => {
                Ok(json!([{"id": DEVICE, "name": "Office", "address": "x"}]))
            }
            "engine.state" | "events.subscribe" => Ok(Value::Null),
            _ => Err(("internal", "unexpected".into())),
        });
        let (client, _) = connected_client(&fake).await;
        let computer = format!("silo-remote:{DEVICE}:{COMPUTER}");
        let resolved = resolve_viewer(&client, &computer).await.unwrap();
        assert_eq!(resolved.name, "build box · Office");
        assert_eq!(resolved.stable_id, computer);
        assert_eq!(
            fake.calls("desktop.status"),
            [json!({"computerId": COMPUTER, "deviceId": DEVICE})]
        );
    }

    #[tokio::test]
    async fn a_remote_refusal_passes_through_typed() {
        let fake = FakeEngine::start(|method, _| match method {
            "engine.state" | "events.subscribe" => Ok(Value::Null),
            _ => Err((
                "unsupported_remote_operation",
                "desktop.status on a remote device".into(),
            )),
        });
        let (client, _) = connected_client(&fake).await;
        let computer = format!("silo-remote:{DEVICE}:{COMPUTER}");
        let error = resolve_viewer(&client, &computer).await.unwrap_err();
        assert!(error.is("unsupported_remote_operation"));
        assert!(fake.calls("display.viewer.open").is_empty());
    }

    #[tokio::test]
    async fn attach_opens_a_connection_and_detach_closes_it() {
        let fake = FakeEngine::start(|method, _| match method {
            "computers.list" => Ok(computers_list()),
            "display.viewer.open" => Ok(json!({
                "connectionId": "c1", "url": "http://127.0.0.1:41000/",
                "cookie": {"name": "silo", "value": "v", "domain": "127.0.0.1",
                           "path": "/", "httpOnly": true},
                "initScript": "/*b*/", "name": "dev"})),
            "display.viewer.close" => Ok(Value::Null),
            "engine.state" | "events.subscribe" => Ok(Value::Null),
            _ => Err(("internal", "unexpected".into())),
        });
        let (client, _) = connected_client(&fake).await;
        let result = open_connection(&client, "dev").await.unwrap();
        assert_eq!(result.connection_id, "c1");
        assert_eq!(
            fake.calls("display.viewer.open"),
            [json!({"computerId": COMPUTER})]
        );
        close_connection(&client, "c1").await.unwrap();
        assert_eq!(
            fake.calls("display.viewer.close"),
            [json!({"connectionId": "c1"})]
        );
    }

    #[tokio::test]
    async fn a_refused_open_keeps_the_engines_code() {
        let fake = FakeEngine::start(|method, _| match method {
            "computers.list" => Ok(computers_list()),
            "engine.state" | "events.subscribe" => Ok(Value::Null),
            _ => Err(("busy", "Close an unused desktop viewer first.".into())),
        });
        let (client, _) = connected_client(&fake).await;
        let error = open_connection(&client, "dev").await.unwrap_err();
        assert!(error.is("busy"));
        assert_eq!(error.message, "Close an unused desktop viewer first.");
    }
}
