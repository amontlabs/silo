use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, PhysicalSize, WindowEvent,
};

#[derive(Default)]
pub struct PanelState {
    anchor: Mutex<Option<PhysicalPosition<f64>>>,
    blurred_at: Mutex<Option<Instant>>,
    route: Mutex<Option<serde_json::Value>>,
}

pub fn install(app: &AppHandle) -> tauri::Result<()> {
    app.manage(PanelState::default());
    let panel = app
        .get_webview_window("status")
        .expect("status window is configured");
    let handle = app.clone();
    panel.on_window_event(move |event| match event {
        WindowEvent::Focused(false) => {
            if let Some(panel) = handle.get_webview_window("status") {
                if panel.is_visible().unwrap_or(false) {
                    *handle
                        .state::<PanelState>()
                        .blurred_at
                        .lock()
                        .unwrap_or_else(|error| error.into_inner()) = Some(Instant::now());
                    report(panel.hide());
                }
            }
        }
        WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            report(hide_status(handle.clone()));
        }
        _ => {}
    });
    Ok(())
}

pub fn report<T, E: std::fmt::Display>(result: Result<T, E>) {
    if let Err(error) = result {
        eprintln!("Silo desktop: {error}");
    }
}

pub fn toggle(app: &AppHandle, anchor: PhysicalPosition<f64>) -> tauri::Result<()> {
    let Some(panel) = app.get_webview_window("status") else {
        return Ok(()); // The window is already gone during teardown.
    };
    let state = app.state::<PanelState>();
    // Clicking the status item can blur the panel before its activation arrives.
    let just_blurred = state
        .blurred_at
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .take()
        .is_some_and(|time| time.elapsed() < Duration::from_millis(200));
    if panel.is_visible()? || just_blurred {
        return panel.hide();
    }
    *state
        .anchor
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = Some(anchor);
    position(app)?;
    app.emit_to("status", "desktop:status-opened", ())?;
    panel.show()?;
    panel.set_focus()
}

fn position(app: &AppHandle) -> tauri::Result<()> {
    let Some(panel) = app.get_webview_window("status") else {
        return Ok(());
    };
    let Some(anchor) = *app
        .state::<PanelState>()
        .anchor
        .lock()
        .unwrap_or_else(|error| error.into_inner())
    else {
        return Ok(());
    };
    let monitor = app
        .monitor_from_point(anchor.x, anchor.y)?
        .or(app.primary_monitor()?);
    if let Some(monitor) = monitor {
        let area = monitor.work_area();
        let size = panel
            .inner_size()?
            .to_logical::<f64>(panel.scale_factor()?)
            .to_physical::<u32>(monitor.scale_factor());
        let point = panel_position(
            anchor,
            size,
            area.position,
            area.size,
            monitor.scale_factor(),
        );
        panel.set_position(point)?;
    }
    Ok(())
}

fn panel_position(
    anchor: PhysicalPosition<f64>,
    panel: PhysicalSize<u32>,
    origin: PhysicalPosition<i32>,
    area: PhysicalSize<u32>,
    scale: f64,
) -> PhysicalPosition<i32> {
    let gap = 8.0 * scale;
    let left = origin.x as f64 + gap;
    let top = origin.y as f64 + gap;
    let right = (origin.x as f64 + area.width as f64 - panel.width as f64 - gap).max(left);
    let bottom = (origin.y as f64 + area.height as f64 - panel.height as f64 - gap).max(top);
    let y = if anchor.y > origin.y as f64 + area.height as f64 / 2.0 {
        anchor.y - panel.height as f64 - gap
    } else {
        anchor.y + gap
    };
    PhysicalPosition::new(
        (anchor.x - panel.width as f64 / 2.0)
            .clamp(left, right)
            .round() as i32,
        y.clamp(top, bottom).round() as i32,
    )
}

#[tauri::command]
pub fn resize_status(app: AppHandle, height: f64) -> Result<(), String> {
    if !height.is_finite() {
        return Err("Invalid panel height".into());
    }
    let panel = app
        .get_webview_window("status")
        .ok_or("Status window is unavailable")?;
    panel
        .set_size(LogicalSize::new(380.0, height.ceil().clamp(1.0, 520.0)))
        .map_err(|e| e.to_string())?;
    position(&app).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn hide_status(app: AppHandle) -> tauri::Result<()> {
    match app.get_webview_window("status") {
        Some(panel) => panel.hide(),
        None => Ok(()),
    }
}

#[tauri::command]
pub fn open_main(app: AppHandle, route: Option<serde_json::Value>) -> tauri::Result<()> {
    if let Some(route) = route {
        *app.state::<PanelState>()
            .route
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(route);
        app.emit_to("main", "desktop:route-requested", ())?;
    }
    hide_status(app.clone())?;
    let Some(main) = app.get_webview_window("main") else {
        return Ok(());
    };
    main.show()?;
    main.unminimize()?;
    main.set_focus()
}

#[tauri::command]
pub fn take_main_route(app: AppHandle) -> Option<serde_json::Value> {
    app.state::<PanelState>().route.lock().ok()?.take()
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    crate::commands::settings::request_quit(&app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_bar_on_retina_display_stays_inside_work_area() {
        assert_eq!(
            panel_position(
                PhysicalPosition::new(2850.0, 48.0),
                PhysicalSize::new(760, 620),
                PhysicalPosition::new(0, 48),
                PhysicalSize::new(2880, 1752),
                2.0
            ),
            PhysicalPosition::new(2104, 64)
        );
    }

    #[test]
    fn bottom_bar_opens_upwards_on_negative_origin_monitor() {
        assert_eq!(
            panel_position(
                PhysicalPosition::new(-40.0, 1080.0),
                PhysicalSize::new(380, 300),
                PhysicalPosition::new(-1920, 0),
                PhysicalSize::new(1920, 1040),
                1.0
            ),
            PhysicalPosition::new(-388, 732)
        );
    }
}
