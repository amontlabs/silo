//! Tauri adapters for privileged desktop viewer windows and guest webviews.
use crate::{
    desktop_bridge::Bridge, desktop_proxy::Proxy, desktop_viewer as engine, editor,
    owned_tunnel::Tunnel, remote, remote_access, runtime,
};
use serde_json::json;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, WebviewBuilder, WebviewUrl,
    WebviewWindowBuilder, Window,
};

const GUEST_BRIDGE_SCRIPT: &str = include_str!("../desktop_viewer_bridge.js");
const VIEWER_PAGE_EVENT: &str = "silo://desktop-viewer-page";

pub(crate) fn require_computer(window: &Window, computer: &str) -> Result<(), String> {
    engine::allows_window(window.label(), computer)
}

fn connect(app: &AppHandle, computer: &str) -> Result<(Proxy, Option<Tunnel>), String> {
    editor::require_openssh("view computer desktops")?;
    let remote_target = remote_access::target(computer)?;
    let connection = if let Some((device, computer)) = &remote_target {
        remote::call_remote(device, "desktop.connect", json!({"computerId":computer}))?
    } else {
        let paths = runtime::runtime_paths(&crate::commands::runtime::host(app))?;
        engine::local_connection(&paths, computer, None)?
    };
    let guest = connection["port"]
        .as_u64()
        .and_then(|p| u16::try_from(p).ok())
        .filter(|p| *p != 0)
        .ok_or("Invalid desktop endpoint.")?;
    let (alias, config) = if let Some((device, computer)) = remote_target {
        editor::prepare_remote_private(&device, &computer, "/workspace")?
    } else {
        let paths = runtime::runtime_paths(&crate::commands::runtime::host(app))?;
        let directory = paths.home.join("ssh/desktop-viewer").join(computer);
        editor::prepare_private_transport(&paths, computer, &directory)?
    };
    let home = app
        .path()
        .home_dir()
        .map_err(|_| "Could not prepare the desktop connection.")?;
    let directory = engine::socket_directory(&crate::channel::current().state_dir(&home))?;
    let socket: PathBuf = directory.path().join("desktop.sock");
    let command = engine::forward_command(&config, &alias, &socket, guest)?;
    let mut tunnel = Tunnel::spawn(&command, Some(directory))
        .map_err(|_| "Could not connect to the desktop.")?;
    let uid = unsafe { libc::geteuid() };
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if !tunnel.running() {
            return Err("Desktop tunnel closed. Check the computer connection.".into());
        }
        if engine::socket_ready(&socket, uid)? {
            break;
        }
        if Instant::now() >= deadline {
            return Err("Desktop connection timed out. Reconnect the computer.".into());
        }
        std::thread::sleep(Duration::from_millis(80));
    }
    let username = connection["username"]
        .as_str()
        .ok_or("Missing desktop credentials.")?;
    let password = connection["password"]
        .as_str()
        .ok_or("Missing desktop credentials.")?;
    let proxy = Proxy::start(socket, guest, username, password)?;
    Ok((proxy, Some(tunnel)))
}

/// Runs `f` against the bridge for one connected viewer. Call this on a worker
/// thread because page operations wait for responses from the guest.
pub(crate) fn with_bridge<R>(
    app: &AppHandle,
    label: &str,
    f: impl FnOnce(&Bridge<'_>) -> Result<R, String>,
) -> Result<R, String> {
    let ticket = engine::bridge_ticket_for(label).ok_or("The desktop is not connected.")?;
    let view = app
        .get_webview(&format!("guest-{label}"))
        .ok_or("The desktop display is closed.")?;
    if !engine::ticket_is_current_for(label, &ticket) {
        return Err("The desktop reconnected.".into());
    }
    let page = crate::commands::desktop_bridge::TauriPage(&view);
    f(&Bridge {
        page: &page,
        inbox: &ticket.inbox,
    })
}

#[tauri::command]
pub(crate) async fn open_desktop(
    app: AppHandle,
    window: Window,
    computer: String,
) -> Result<(), String> {
    if window.label() != "main" {
        return Err("Open desktops from the main Silo window.".into());
    }
    tokio::task::spawn_blocking(move || {
        runtime::shutdown::ensure_accepting_operations()?;
        let mut stable_id = computer.clone();
        let name = if let Some((device, computer)) = remote_access::target(&computer)? {
            // Verify the remote identity before creating a shell.
            let state =
                remote::call_remote(&device, "desktop.status", json!({"computerId":computer}))?;
            let configuration = state["name"].as_str().unwrap_or(&computer);
            let device_name = remote::saved_devices()?
                .into_iter()
                .find(|h| h.id == device)
                .map(|h| h.name);
            format!(
                "{configuration} · {}",
                device_name.as_deref().unwrap_or(&device)
            )
        } else {
            runtime::validate_name(&computer).map_err(|e| e.to_string())?;
            let paths = runtime::runtime_paths(&crate::commands::runtime::host(&app))?;
            let metadata = runtime::read_metadata(&paths.metadata).map_err(|e| e.to_string())?;
            let configuration = metadata
                .computers
                .iter()
                .find(|m| m.name() == computer)
                .ok_or("Computer no longer exists.")?;
            stable_id = configuration.id().to_string();
            computer.clone()
        };
        let mut route = tauri::Url::parse("http://silo.local/index.html").unwrap();
        route
            .query_pairs_mut()
            .append_pair("desktop", &computer)
            .append_pair("id", &stable_id)
            .append_pair("name", &name);
        let route = format!("index.html?{}", route.query().unwrap());
        let claim = engine::claim(&computer)?;
        // Window calls run on the main thread; never make them under the lock.
        let label = match claim {
            engine::ViewerClaim::Existing(label) => {
                // Without a window yet, another call is still creating it (G-15).
                let Some(existing) = app.get_window(&label) else {
                    return Ok(());
                };
                let _ = existing.show();
                return existing
                    .set_focus()
                    .map_err(|_| "Could not focus desktop.".into());
            }
            engine::ViewerClaim::New(label) => label,
        };
        let result = WebviewWindowBuilder::new(&app, &label, WebviewUrl::App(route.into()))
            .title(engine::viewer_title(&name, crate::channel::current()))
            .inner_size(1200., 820.)
            .min_inner_size(672., 480.)
            .build();
        let viewer = match result {
            Ok(v) => v,
            Err(_) => {
                engine::remove_viewer(&label);
                return Err("Could not open desktop viewer.".into());
            }
        };
        let menu_app = app.clone();
        let shortcut_label = label.clone();
        viewer.on_window_event(move |event| match event {
            tauri::WindowEvent::Destroyed => {
                crate::commands::viewer_shortcuts::uninstall(&shortcut_label);
                crate::desktop_viewer_media::forget_viewer(&shortcut_label);
                let others = menu_app
                    .webview_windows()
                    .into_iter()
                    .map(|(name, window)| (name, window.is_focused().unwrap_or(false)));
                crate::app_menu::set_viewer_focus(
                    &menu_app,
                    engine::viewer_focused_besides(&shortcut_label, others),
                );
                // Attach never holds the lock across window work, so this cannot
                // wait on the main thread; reap the tunnel after unlocking.
                engine::remove_viewer(&label);
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
#[tauri::command]
pub(crate) async fn desktop_viewer_attach(
    app: AppHandle,
    window: Window,
    computer: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    viewport_height: f64,
) -> Result<(), String> {
    require_computer(&window, &computer)?;
    if window.label() == "main"
        || [x, y, width, height, viewport_height]
            .iter()
            .any(|v| !v.is_finite())
        || x < 0.
        || y < 0.
        || width < 1.
        || height < 1.
        || width > 16384.
        || height > 16384.
        || viewport_height < 1.
        || viewport_height > 16384.
    {
        return Err("Invalid desktop viewer bounds.".into());
    }
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
        let inset_y = engine::viewport_inset(
            shell
                .size()
                .map_err(|_| "Could not measure desktop viewer.")?
                .height,
            scale,
            viewport_height,
        )?;
        #[cfg(not(target_os = "macos"))]
        let inset_y = 0.;
        let position =
            engine::desktop_position(shell_origin.x, shell_origin.y, scale, x, y + inset_y)?;
        let label = format!("guest-{}", window.label());
        let existing = app.get_webview(&label);
        let plan = engine::begin_attach_viewer(window.label(), &computer, existing.is_some())?;
        let generation = match plan {
            engine::AttachPlan::Resize => {
                let view = existing.ok_or("Desktop viewer closed.")?;
                return view
                    .set_bounds(tauri::Rect {
                        position: LogicalPosition::new(position.0, position.1).into(),
                        size: LogicalSize::new(width, height).into(),
                    })
                    .map_err(|_| "Could not resize desktop.".into());
            }
            engine::AttachPlan::Connect { generation, stale } => {
                drop(stale);
                generation
            }
        };
        let abort = |message: &str| -> String {
            engine::abort_attach_viewer(window.label(), generation);
            message.into()
        };
        if let Some(view) = existing {
            view.close()
                .map_err(|_| abort("Could not reconnect desktop."))?;
        }
        let (proxy, tunnel) = connect(&app, &computer).map_err(|e| abort(&e))?;
        let origin = format!("http://127.0.0.1:{}", proxy.port);
        let permitted = origin.clone();
        let page_app = app.clone();
        let page_shell = window.label().to_owned();
        let builder = WebviewBuilder::new(
            &label,
            WebviewUrl::External(tauri::Url::parse("about:blank").unwrap()),
        )
        .incognito(true)
        .focused(true)
        .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
        // Guest pages must never write to the host's Downloads folder (G-13).
        .on_download(|_, _| false)
        // Nor use the host clipboard, from any frame (G-20).
        .initialization_script_for_all_frames(GUEST_BRIDGE_SCRIPT)
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
                LogicalSize::new(width, height),
            )
            .map_err(|_| abort("Could not create desktop display."))?;
        let shell_label = window.label().to_owned();
        view.on_webview_event(move |event| {
            if let tauri::WebviewEvent::DragDrop(drag) = event {
                crate::commands::transfer::native_drop(&shell_label, drag);
            }
        });
        let cookie =
            tauri::webview::Cookie::build((proxy.cookie_name.clone(), proxy.token.clone()))
                .domain("127.0.0.1")
                .path("/")
                .http_only(true)
                .build();
        if view.set_cookie(cookie).is_err()
            || view
                .navigate(tauri::Url::parse(&engine::viewer_url(&origin)).unwrap())
                .is_err()
        {
            let _ = view.close();
            return Err(abort("Could not authenticate desktop viewer."));
        }
        let rejected =
            engine::finish_attach_viewer(window.label(), generation, Some(proxy), tunnel).err();
        if let Some(stale) = rejected {
            drop(stale);
            let _ = view.close();
            return Err("Desktop viewer closed.".into());
        }
        view.set_focus()
            .map_err(|_| "Could not focus desktop display.")?;
        Ok(())
    })
    .await
    .map_err(|_| "Desktop connection failed.")?
}
#[tauri::command]
pub(crate) async fn desktop_viewer_detach(app: AppHandle, window: Window) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        engine::disconnect_viewer(window.label())?;
        if let Some(view) = app.get_webview(&format!("guest-{}", window.label())) {
            let _ = view.close();
        }
        Ok(())
    })
    .await
    .map_err(|_| "Desktop disconnect failed.")?
}
