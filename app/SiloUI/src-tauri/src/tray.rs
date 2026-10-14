use serde::Deserialize;
use tauri::{image::Image, AppHandle};

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tone {
    Success,
    Neutral,
    Warning,
    Error,
    Busy,
}

fn icon(tone: Tone) -> Image<'static> {
    #[cfg(target_os = "macos")]
    let bytes: &[u8] = match tone {
        Tone::Success | Tone::Neutral => include_bytes!("../icons/tray/ready.png"),
        Tone::Warning => include_bytes!("../icons/tray/warning.png"),
        Tone::Error => include_bytes!("../icons/tray/error.png"),
        Tone::Busy => include_bytes!("../icons/tray/busy.png"),
    };
    #[cfg(target_os = "linux")]
    let bytes: &[u8] = match tone {
        Tone::Success | Tone::Neutral => include_bytes!("../icons/tray/ready-linux.png"),
        Tone::Warning => include_bytes!("../icons/tray/warning-linux.png"),
        Tone::Error => include_bytes!("../icons/tray/error-linux.png"),
        Tone::Busy => include_bytes!("../icons/tray/busy-linux.png"),
    };
    Image::from_bytes(bytes).expect("bundled status icon is valid")
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use crate::status_panel;
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    pub fn install(app: &AppHandle) -> tauri::Result<()> {
        TrayIconBuilder::with_id("silo")
            .icon(icon(Tone::Neutral))
            .icon_as_template(true)
            .tooltip(crate::channel::current().tray_title())
            .show_menu_on_left_click(false)
            .on_tray_icon_event(|tray, event| {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left | MouseButton::Right,
                    button_state: MouseButtonState::Up,
                    rect,
                    position,
                    ..
                } = event
                {
                    let scale = tray
                        .app_handle()
                        .monitor_from_point(position.x, position.y)
                        .ok()
                        .flatten()
                        .map_or(1.0, |monitor| monitor.scale_factor());
                    let origin = rect.position.to_physical::<f64>(scale);
                    let size = rect.size.to_physical::<f64>(scale);
                    status_panel::report(status_panel::toggle(
                        tray.app_handle(),
                        tauri::PhysicalPosition::new(
                            origin.x + size.width / 2.0,
                            origin.y + size.height,
                        ),
                    ));
                }
            })
            .build(app)?;
        Ok(())
    }

    pub async fn update(app: &AppHandle, tone: Tone, label: String) -> Result<(), String> {
        let tray = app.tray_by_id("silo").ok_or("Status item is unavailable")?;
        // Replacing the image must preserve native appearance tinting in one redraw.
        tray.set_icon_with_as_template(Some(icon(tone)), true)
            .map_err(|e| e.to_string())?;
        tray.set_tooltip(Some(format!(
            "{} · {label}",
            crate::channel::current().tray_title()
        )))
        .map_err(|e| e.to_string())
    }

    pub fn available(_: &AppHandle) -> bool {
        true
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use crate::status_panel;
    use ksni::TrayMethods;
    use std::sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    };
    use tauri::Manager;

    struct LinuxTray {
        app: AppHandle,
        online: Arc<AtomicBool>,
        /// Counts watcher disappearances so only the latest one can surface the window.
        offline_generation: Arc<AtomicU64>,
        tone: Tone,
        label: String,
    }

    impl ksni::Tray for LinuxTray {
        fn id(&self) -> String {
            crate::channel::current().tray_id().into()
        }
        fn title(&self) -> String {
            crate::channel::current().tray_title().into()
        }
        fn icon_pixmap(&self) -> Vec<ksni::Icon> {
            let image = icon(self.tone);
            let mut argb = image.rgba().to_vec();
            for pixel in argb.chunks_exact_mut(4) {
                pixel.rotate_right(1);
            }
            vec![ksni::Icon {
                width: image.width() as i32,
                height: image.height() as i32,
                data: argb,
            }]
        }
        fn tool_tip(&self) -> ksni::ToolTip {
            ksni::ToolTip {
                title: crate::channel::current().product_name().into(),
                description: self.label.clone(),
                ..Default::default()
            }
        }
        fn activate(&mut self, x: i32, y: i32) {
            let app = self.app.clone();
            status_panel::report(self.app.run_on_main_thread(move || {
                if super::panel_can_anchor(wayland(), x, y) {
                    status_panel::report(status_panel::toggle(
                        &app,
                        tauri::PhysicalPosition::new(x as f64, y as f64),
                    ));
                } else {
                    status_panel::report(status_panel::open_main(app, None));
                }
            }));
        }
        fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
            vec![
                ksni::menu::StandardItem {
                    label: format!("Open {}", crate::channel::current().product_name()),
                    activate: Box::new(|tray: &mut Self| {
                        status_panel::report(status_panel::open_main(tray.app.clone(), None))
                    }),
                    ..Default::default()
                }
                .into(),
                ksni::menu::StandardItem {
                    label: format!("Quit {}", crate::channel::current().product_name()),
                    activate: Box::new(|tray: &mut Self| {
                        crate::commands::settings::request_quit(&tray.app)
                    }),
                    ..Default::default()
                }
                .into(),
            ]
        }
        fn watcher_online(&self) {
            self.online.store(true, Ordering::Relaxed);
        }
        fn watcher_offline(&self, _: ksni::OfflineReason) -> bool {
            self.online.store(false, Ordering::Relaxed);
            let generation = self.offline_generation.fetch_add(1, Ordering::SeqCst) + 1;
            // A desktop without a tray must never strand an invisible app, but a
            // brief watcher restart (plasmashell, GNOME Shell reload) must not
            // pop the window up either.
            let (app, online, offline) = (
                self.app.clone(),
                self.online.clone(),
                self.offline_generation.clone(),
            );
            std::thread::spawn(move || {
                std::thread::sleep(super::WATCHER_GRACE);
                let visible = app
                    .get_webview_window("main")
                    .is_some_and(|window| window.is_visible().unwrap_or(false));
                if super::surface_after_grace(
                    online.load(Ordering::Relaxed),
                    offline.load(Ordering::SeqCst) == generation,
                    visible,
                ) {
                    status_panel::report(status_panel::open_main(app, None));
                }
            });
            true
        }
    }

    /// Whether GTK runs on Wayland. Call on the GTK main thread.
    fn wayland() -> bool {
        use gtk::glib::prelude::ObjectExt;
        gtk::gdk::Display::default()
            .is_some_and(|display| display.type_().name() == "GdkWaylandDisplay")
    }

    struct TrayState {
        online: Arc<AtomicBool>,
        handle: std::sync::Mutex<Option<ksni::Handle<LinuxTray>>>,
        readiness: tokio::sync::watch::Receiver<Option<Result<(), String>>>,
    }

    pub fn install(app: &AppHandle) -> tauri::Result<()> {
        // ksni only calls watcher_online after a prior watcher_offline callback.
        let online = Arc::new(AtomicBool::new(true));
        let (ready, readiness) = tokio::sync::watch::channel(None);
        app.manage(TrayState {
            online: online.clone(),
            handle: std::sync::Mutex::new(None),
            readiness,
        });
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            match (LinuxTray {
                app: app.clone(),
                online,
                offline_generation: Arc::new(AtomicU64::new(0)),
                tone: Tone::Neutral,
                label: crate::channel::current().product_name().into(),
            })
            .assume_sni_available(true)
            .spawn()
            .await
            {
                Ok(handle) => {
                    *app.state::<TrayState>()
                        .handle
                        .lock()
                        .unwrap_or_else(|error| error.into_inner()) = Some(handle);
                    let _ = ready.send(Some(Ok(())));
                }
                Err(error) => {
                    let _ = ready.send(Some(Err(error.to_string())));
                    app.state::<TrayState>()
                        .online
                        .store(false, Ordering::Relaxed);
                    status_panel::report(status_panel::open_main(app.clone(), None));
                    eprintln!("Silo tray: {error}");
                }
            }
        });
        Ok(())
    }

    pub async fn update(app: &AppHandle, tone: Tone, label: String) -> Result<(), String> {
        super::wait_for_tray(app.state::<TrayState>().readiness.clone()).await?;
        let handle = app
            .state::<TrayState>()
            .handle
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
            .ok_or("Status item is unavailable")?;
        handle
            .update(move |tray| {
                tray.tone = tone;
                tray.label = label;
            })
            .await
            .ok_or("Status item is unavailable")?;
        Ok(())
    }

    pub fn available(app: &AppHandle) -> bool {
        app.state::<TrayState>().online.load(Ordering::Relaxed)
    }
}

pub use platform::{available, install};

#[cfg(any(test, target_os = "linux"))]
async fn wait_for_tray(
    mut readiness: tokio::sync::watch::Receiver<Option<Result<(), String>>>,
) -> Result<(), String> {
    loop {
        if let Some(result) = readiness.borrow_and_update().clone() {
            return result;
        }
        readiness
            .changed()
            .await
            .map_err(|_| "Status item is unavailable".to_owned())?;
    }
}

/// How long the StatusNotifierWatcher may be gone before Silo surfaces its
/// window. Desktop shells restart it briefly (plasmashell restart, GNOME Shell
/// reload, AppIndicator extension update).
#[cfg(any(test, target_os = "linux"))]
const WATCHER_GRACE: std::time::Duration = std::time::Duration::from_secs(5);

/// Surface the main window only if the tray is still gone after the grace
/// period, no later disappearance owns the decision, and the window is hidden.
#[cfg(any(test, target_os = "linux"))]
fn surface_after_grace(online: bool, latest_disappearance: bool, main_visible: bool) -> bool {
    !online && latest_disappearance && !main_visible
}

/// The tray panel is a borderless always-on-top window placed at the tray
/// icon. Wayland ignores client positioning and always-on-top, and KDE on
/// Wayland reports activation at (0, 0), so there the tray opens the main
/// window instead of a panel that could appear anywhere and never close.
#[cfg(any(test, target_os = "linux"))]
fn panel_can_anchor(wayland: bool, x: i32, y: i32) -> bool {
    !wayland && (x, y) != (0, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_updates_wait_until_the_linux_tray_exists() {
        use std::{
            future::Future,
            task::{Context, Poll, Waker},
        };

        let (ready, readiness) = tokio::sync::watch::channel(None);
        let mut update = std::pin::pin!(wait_for_tray(readiness));
        let mut context = Context::from_waker(Waker::noop());
        assert!(update.as_mut().poll(&mut context).is_pending());
        ready.send(Some(Ok(()))).unwrap();
        assert_eq!(update.as_mut().poll(&mut context), Poll::Ready(Ok(())));
    }

    #[test]
    fn startup_failure_is_reported_to_pending_tray_updates() {
        use std::{
            future::Future,
            task::{Context, Poll, Waker},
        };

        let (ready, readiness) = tokio::sync::watch::channel(None);
        let mut update = std::pin::pin!(wait_for_tray(readiness));
        let mut context = Context::from_waker(Waker::noop());
        assert!(update.as_mut().poll(&mut context).is_pending());
        ready.send(Some(Err("D-Bus unavailable".into()))).unwrap();
        assert_eq!(
            update.as_mut().poll(&mut context),
            Poll::Ready(Err("D-Bus unavailable".into()))
        );
    }

    #[test]
    fn abandoned_tray_startup_reports_unavailability() {
        use std::{
            future::Future,
            task::{Context, Poll, Waker},
        };

        let (ready, readiness) = tokio::sync::watch::channel(None);
        drop(ready);
        let mut update = std::pin::pin!(wait_for_tray(readiness));
        let mut context = Context::from_waker(Waker::noop());
        assert_eq!(
            update.as_mut().poll(&mut context),
            Poll::Ready(Err("Status item is unavailable".into()))
        );
    }

    #[test]
    fn wayland_or_an_unknown_position_opens_the_main_window() {
        assert!(panel_can_anchor(false, 1880, 12));
        assert!(!panel_can_anchor(true, 1880, 12));
        assert!(!panel_can_anchor(false, 0, 0));
        assert!(!panel_can_anchor(true, 0, 0));
        assert!(
            panel_can_anchor(false, 0, 1050),
            "a panel at the left screen edge is still anchored"
        );
    }

    #[test]
    fn brief_tray_restarts_do_not_pop_up_the_window() {
        assert!(
            surface_after_grace(false, true, false),
            "a tray that stays gone surfaces a hidden window"
        );
        assert!(
            !surface_after_grace(true, true, false),
            "the tray came back"
        );
        assert!(
            !surface_after_grace(false, false, false),
            "a later disappearance decides"
        );
        assert!(
            !surface_after_grace(false, true, true),
            "a visible window is left alone"
        );
        assert!(WATCHER_GRACE >= std::time::Duration::from_secs(3));
    }
}

#[tauri::command]
pub async fn update_tray(app: AppHandle, tone: Tone, label: String) -> Result<(), String> {
    platform::update(&app, tone, label).await
}
