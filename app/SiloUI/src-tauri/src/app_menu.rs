//! Native desktop menu actions use the same frontend flows and native operation
//! gates as clicks. Unready or busy views cannot receive stale menu commands.
use serde::Deserialize;
use tauri::{AppHandle, WebviewWindow};

#[cfg_attr(
    not(any(test, target_os = "macos", target_os = "linux")),
    allow(dead_code)
)]
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MenuState {
    ready: bool,
    busy: bool,
    can_go_back: bool,
    can_go_forward: bool,
    can_create_computer: bool,
    can_import: bool,
    can_check_updates: bool,
    sidebar_collapsed: bool,
}
#[cfg_attr(
    not(any(test, target_os = "macos", target_os = "linux")),
    allow(dead_code)
)]
fn enabled(command: &str, state: &MenuState) -> bool {
    if matches!(
        command,
        "show-window"
            | "help"
            | "issues"
            | "releases"
            | "quit"
            | "close-window"
            | "minimize"
            | "maximize"
            | "fullscreen"
            | "undo"
            | "redo"
    ) {
        return true;
    }
    if !state.ready || state.busy {
        return false;
    }
    match command {
        "new-computer" => state.can_create_computer,
        "import-computer" => state.can_import,
        "check-updates" => state.can_check_updates,
        "go-back" => state.can_go_back,
        "go-forward" => state.can_go_forward,
        "settings" | "search" | "toggle-sidebar" | "go-computers" | "go-github" | "go-secrets"
        | "go-files" | "go-logs" | "go-network" | "go-activity" => true,
        _ => false,
    }
}
/// Quit must enter Tauri's ExitRequested gate even when no frontend is ready.
/// The menu item is Silo's own rather than AppKit's `terminate:`; AppKit quits
/// (Dock, logout) reach the same path through `system_shutdown`.
fn request_menu_quit(id: &str, request_exit: impl FnOnce()) -> bool {
    if id != "silo-menu:quit" {
        return false;
    }
    request_exit();
    true
}

#[tauri::command]
pub(crate) fn set_app_menu_state(
    app: AppHandle,
    window: WebviewWindow,
    state: MenuState,
) -> Result<(), String> {
    if window.label() != "main" {
        return Err("Only the main window can update the application menu.".into());
    }
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    native::set_state(&app, state)?;
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let _ = (app, state);
    Ok(())
}
#[tauri::command]
pub(crate) fn show_app_menu(window: WebviewWindow) -> Result<(), String> {
    if window.label() != "main" {
        return Err("Only the main window can show the application menu.".into());
    }
    #[cfg(target_os = "linux")]
    {
        let main = window.clone();
        window
            .run_on_main_thread(move || linux_menu::show(&main))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
#[cfg(target_os = "linux")]
#[path = "linux_menu.rs"]
mod linux_menu;

pub(crate) fn install(app: &AppHandle) -> tauri::Result<()> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    native::install(app)?;
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let _ = app;
    Ok(())
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
mod native {
    use super::*;
    use std::sync::Mutex;
    use tauri::{
        menu::{AboutMetadata, Menu, MenuItem, PredefinedMenuItem as Standard, Submenu},
        Emitter, Manager,
    };
    struct Controller {
        state: Mutex<MenuState>,
        items: Vec<(&'static str, MenuItem<tauri::Wry>)>,
    }
    pub(super) fn set_state(app: &AppHandle, state: MenuState) -> Result<(), String> {
        let menu = app.state::<Controller>();
        // Do not hold a lock while set_enabled dispatches work to the main thread.
        *menu
            .state
            .lock()
            .map_err(|_| "Application menu state is unavailable.")? = state.clone();
        for (command, item) in &menu.items {
            if *command == "toggle-sidebar" {
                item.set_text(if state.sidebar_collapsed {
                    "Show Sidebar"
                } else {
                    "Hide Sidebar"
                })
                .map_err(|_| "Application menu could not be updated.")?;
            }
            item.set_enabled(enabled(command, &state))
                .map_err(|_| "Application menu could not be updated.")?;
        }
        Ok(())
    }
    fn link(command: &str) -> Option<&'static str> {
        match command {
            "issues" => Some("https://github.com/amontlabs/silo/issues"),
            "releases" => Some("https://github.com/amontlabs/silo/releases"),
            _ => None,
        }
    }
    pub(super) fn install(app: &AppHandle) -> tauri::Result<()> {
        let mut items = Vec::new();
        let mut item = |id: &'static str,
                        title: &str,
                        shortcut: Option<&str>|
         -> tauri::Result<MenuItem<tauri::Wry>> {
            let entry = MenuItem::with_id(
                app,
                format!("silo-menu:{id}"),
                title,
                enabled(id, &MenuState::default()),
                shortcut,
            )?;
            items.push((id, entry.clone()));
            Ok(entry)
        };
        let name = crate::channel::current().product_name();
        let about = Standard::about(
            app,
            Some(&format!("About {name}")),
            Some(AboutMetadata {
                name: Some(name.into()),
                version: Some(app.package_info().version.to_string()),
                icon: app.default_window_icon().cloned(),
                ..Default::default()
            }),
        )?;
        #[cfg(target_os = "macos")]
        let app_menu = Submenu::with_items(
            app,
            name,
            true,
            &[
                &about,
                &item("settings", "Settings…", Some("CmdOrCtrl+,"))?,
                &item("check-updates", "Check for Updates…", None)?,
                &Standard::separator(app)?,
                &Standard::services(app, None)?,
                &Standard::separator(app)?,
                &Standard::hide(app, Some(&format!("Hide {name}")))?,
                &Standard::hide_others(app, None)?,
                &Standard::show_all(app, None)?,
                &Standard::separator(app)?,
                &item("quit", &format!("Quit {name}"), Some("CmdOrCtrl+Q"))?,
            ],
        )?;
        #[cfg(target_os = "linux")]
        let app_menu = Submenu::with_items(
            app,
            name,
            true,
            &[
                &about,
                &item("settings", "Settings…", Some("CmdOrCtrl+,"))?,
                &item("check-updates", "Check for Updates…", None)?,
                &Standard::separator(app)?,
                &item("quit", &format!("Quit {name}"), Some("CmdOrCtrl+Q"))?,
            ],
        )?;
        let file = Submenu::with_items(
            app,
            "File",
            true,
            &[
                &item("new-computer", "New Computer…", Some("CmdOrCtrl+N"))?,
                &item("import-computer", "Import Computer…", None)?,
                &Standard::separator(app)?,
                #[cfg(target_os = "macos")]
                &Standard::close_window(app, None)?,
                #[cfg(target_os = "linux")]
                &item("close-window", "Close Window", Some("CmdOrCtrl+W"))?,
            ],
        )?;
        let edit = Submenu::with_items(
            app,
            "Edit",
            true,
            &[
                #[cfg(target_os = "macos")]
                &Standard::undo(app, None)?,
                #[cfg(target_os = "linux")]
                &item("undo", "Undo", None)?,
                #[cfg(target_os = "macos")]
                &Standard::redo(app, None)?,
                #[cfg(target_os = "linux")]
                &item("redo", "Redo", None)?,
                &Standard::separator(app)?,
                &Standard::cut(app, None)?,
                &Standard::copy(app, None)?,
                &Standard::paste(app, None)?,
                &Standard::select_all(app, None)?,
            ],
        )?;
        let view = Submenu::with_items(
            app,
            "View",
            true,
            &[
                &item("search", "Search or Jump To…", Some("CmdOrCtrl+K"))?,
                &Standard::separator(app)?,
                &item("go-back", "Back", Some("CmdOrCtrl+["))?,
                &item("go-forward", "Forward", Some("CmdOrCtrl+]"))?,
                &Standard::separator(app)?,
                &item("toggle-sidebar", "Hide Sidebar", Some("CmdOrCtrl+B"))?,
                #[cfg(target_os = "macos")]
                &Standard::fullscreen(app, None)?,
                #[cfg(target_os = "linux")]
                &item("fullscreen", "Toggle Full Screen", Some("F11"))?,
            ],
        )?;
        let go = Submenu::with_items(
            app,
            "Go",
            true,
            &[
                &item("go-computers", "Computers", Some("CmdOrCtrl+1"))?,
                &item("go-files", "Files", Some("CmdOrCtrl+2"))?,
                &item("go-logs", "Logs", Some("CmdOrCtrl+3"))?,
                &item("go-network", "Network", Some("CmdOrCtrl+4"))?,
                &item("go-activity", "Activity", Some("CmdOrCtrl+5"))?,
                &item("go-github", "GitHub", Some("CmdOrCtrl+6"))?,
                &item("go-secrets", "Secrets", Some("CmdOrCtrl+7"))?,
            ],
        )?;
        #[cfg(target_os = "macos")]
        let window = Submenu::with_items(
            app,
            "Window",
            true,
            &[
                &item("show-window", "Open Silo", None)?,
                &Standard::separator(app)?,
                &Standard::minimize(app, None)?,
                &Standard::maximize(app, Some("Zoom"))?,
                &Standard::separator(app)?,
                &Standard::bring_all_to_front(app, None)?,
            ],
        )?;
        #[cfg(target_os = "linux")]
        let window = Submenu::with_items(
            app,
            "Window",
            true,
            &[
                &item("show-window", "Open Silo", None)?,
                &item("minimize", "Minimize", None)?,
                &item("maximize", "Maximize or Restore", None)?,
            ],
        )?;
        let help = Submenu::with_items(
            app,
            "Help",
            true,
            &[
                &item("help", "Documentation", None)?,
                &item("issues", "Report an Issue…", None)?,
                &item("releases", "Release Notes", None)?,
            ],
        )?;
        let menu = Menu::with_items(app, &[&app_menu, &file, &edit, &view, &go, &window, &help])?;
        #[cfg(target_os = "macos")]
        {
            app.set_menu(menu)?;
            window.set_as_windows_menu_for_nsapp()?;
            help.set_as_help_menu_for_nsapp()?;
        }
        #[cfg(target_os = "linux")]
        if let Some(main) = app.get_webview_window("main") {
            main.set_menu(menu)?;
            super::linux_menu::install(&main)?;
        }
        app.manage(Controller {
            state: Mutex::new(MenuState::default()),
            items,
        });
        app.on_menu_event(|app, event| {
            if super::request_menu_quit(event.id().as_ref(), || crate::settings::request_quit(app))
            {
                return;
            }
            let Some(command) = event.id().as_ref().strip_prefix("silo-menu:") else {
                return;
            };
            let state = app
                .state::<Controller>()
                .state
                .lock()
                .map(|s| s.clone())
                .unwrap_or_default();
            if !enabled(command, &state) {
                return;
            }
            #[cfg(target_os = "linux")]
            if let Some(window) = app.get_webview_window("main") {
                let result = match command {
                    "undo" => Some(window.eval("document.execCommand('undo')")),
                    "redo" => Some(window.eval("document.execCommand('redo')")),
                    "close-window" => Some(window.close()),
                    "minimize" => Some(window.minimize()),
                    "maximize" => Some(window.is_maximized().and_then(|v| {
                        if v {
                            window.unmaximize()
                        } else {
                            window.maximize()
                        }
                    })),
                    "fullscreen" => Some(
                        window
                            .is_fullscreen()
                            .and_then(|v| window.set_fullscreen(!v)),
                    ),
                    _ => None,
                };
                if let Some(result) = result {
                    crate::status_panel::report(result);
                    return;
                }
            }
            #[cfg(target_os = "linux")]
            if command == "help" {
                // A snap browser (Ubuntu's default Firefox) cannot read files under
                // /usr/lib or an AppImage mount, so show Help in a Silo window (F-21).
                let result = app
                    .path()
                    .resource_dir()
                    .map_err(|_| "Silo Help could not be located.".to_string())
                    .and_then(|resources| {
                        super::open_help_window(app, &resources.join("docs/silo-help.html"))
                    });
                if let (Err(message), Some(window)) = (result, app.get_webview_window("main")) {
                    crate::status_panel::report(
                        crate::system_integrations::show_integration_error(
                            app.clone(),
                            window,
                            message,
                        ),
                    );
                }
                return;
            }
            if command == "help" || link(command).is_some() {
                // Help ships with this build; external destinations are fixed project URLs.
                let destination = if command == "help" {
                    app.path()
                        .resource_dir()
                        .map(|p| p.join("docs/silo-help.html").into_os_string())
                        .map_err(|_| "Silo Help could not be located.")
                } else {
                    Ok(std::ffi::OsString::from(link(command).unwrap()))
                };
                let app = app.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    let result = destination.and_then(|destination| {
                        crate::applications::launch::sanitize_child(
                            &mut std::process::Command::new(if cfg!(target_os = "macos") {
                                "/usr/bin/open"
                            } else {
                                "xdg-open"
                            }),
                        )
                        .arg(destination)
                        .status()
                        .map_err(|_| "The document or browser could not be opened.")
                        .and_then(|status| {
                            if status.success() {
                                Ok(())
                            } else {
                                Err("The document or browser could not be opened.")
                            }
                        })
                    });
                    if let Err(message) = result {
                        if let Some(window) = app.get_webview_window("main") {
                            crate::status_panel::report(
                                crate::system_integrations::show_integration_error(
                                    app.clone(),
                                    window,
                                    message.into(),
                                ),
                            );
                        }
                    }
                });
                return;
            }
            if let Err(error) = crate::status_panel::open_main(app.clone(), None) {
                crate::status_panel::report::<(), _>(Err(error));
                return;
            }
            if command != "show-window" {
                crate::status_panel::report(app.emit_to("main", "silo://menu-command", command));
            }
        });
        Ok(())
    }
}

/// Where a navigation inside the Linux Help window goes.
#[cfg(any(test, target_os = "linux"))]
#[derive(Debug, PartialEq, Eq)]
enum HelpNavigation {
    /// The bundled Help page itself (including its anchors).
    Stay,
    /// A web link: open it in the user's browser.
    Browser,
    Block,
}

#[cfg(any(test, target_os = "linux"))]
fn help_navigation(url: &tauri::Url, help: &tauri::Url) -> HelpNavigation {
    match url.scheme() {
        "http" | "https" if url.host_str().is_some() => HelpNavigation::Browser,
        "file" if url.path() == help.path() => HelpNavigation::Stay,
        _ => HelpNavigation::Block,
    }
}

/// Show the bundled Help page in its own window. It gets no IPC capabilities.
#[cfg(target_os = "linux")]
fn open_help_window(app: &AppHandle, path: &std::path::Path) -> Result<(), String> {
    use tauri::Manager;
    if let Some(window) = app.get_webview_window("help") {
        return window
            .show()
            .and_then(|_| window.unminimize())
            .and_then(|_| window.set_focus())
            .map_err(|error| error.to_string());
    }
    if !path.is_file() {
        return Err("Silo Help could not be located.".into());
    }
    let help = tauri::Url::from_file_path(path).map_err(|_| "Silo Help could not be located.")?;
    let page = help.clone();
    let browser = app.clone();
    tauri::WebviewWindowBuilder::new(app, "help", tauri::WebviewUrl::External(help))
        .title("Silo Help")
        .inner_size(880.0, 720.0)
        .on_navigation(move |url| match help_navigation(url, &page) {
            HelpNavigation::Stay => true,
            HelpNavigation::Browser => {
                let (app, url) = (browser.clone(), url.to_string());
                std::thread::spawn(move || {
                    if let Err(error) = crate::applications::open_browser(&app, &url) {
                        eprintln!("Silo Help: {error}");
                    }
                });
                false
            }
            HelpNavigation::Block => false,
        })
        .build()
        .map(|_| ())
        .map_err(|error| format!("Silo Help could not be opened: {error}"))
}

// Keep gesture policy platform-independent so AltGr and shortcut regressions run on every host.
#[cfg(any(test, target_os = "linux"))]
#[derive(Default)]
struct MenuKeys {
    /// Event time (ms) of the bare Left Alt press that may toggle the menu.
    alt_pressed_at: Option<u32>,
}
/// A longer bare Alt hold is a modifier (for example a window-manager Alt+drag
/// that never reached Silo), not a menu toggle.
#[cfg(any(test, target_os = "linux"))]
const ALT_TAP_LIMIT_MS: u32 = 500;
#[cfg(any(test, target_os = "linux"))]
#[derive(Clone, Copy)]
enum MenuKey {
    LeftAlt,
    F10,
    Escape,
    Other,
}
#[cfg(any(test, target_os = "linux"))]
impl MenuKeys {
    fn cancel(&mut self) {
        self.alt_pressed_at = None;
    }
    /// `time` is the key event's timestamp in milliseconds (wrapping).
    fn press(&mut self, key: MenuKey, modified: bool, visible: bool, time: u32) -> Option<bool> {
        self.alt_pressed_at = match key {
            // Key repeat keeps the time of the first press.
            MenuKey::LeftAlt if !modified => Some(self.alt_pressed_at.unwrap_or(time)),
            _ => None,
        };
        match key {
            MenuKey::F10 if !modified => Some(true),
            MenuKey::Escape if visible => Some(false),
            _ => None,
        }
    }
    fn release(&mut self, key: MenuKey, visible: bool, time: u32) -> Option<bool> {
        let pressed = self.alt_pressed_at.take();
        (matches!(key, MenuKey::LeftAlt)
            && pressed.is_some_and(|pressed| time.wrapping_sub(pressed) <= ALT_TAP_LIMIT_MS))
        .then_some(!visible)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quit_enters_exit_gate_without_frontend_readiness_or_a_main_window() {
        let requests = std::cell::Cell::new(0);
        assert!(request_menu_quit("silo-menu:quit", || requests.set(requests.get() + 1)));
        assert_eq!(requests.get(), 1);
        for other in ["quit", "silo-menu:close-window", "silo-menu:settings"] {
            assert!(!request_menu_quit(other, || requests.set(requests.get() + 1)));
        }
        assert_eq!(requests.get(), 1);
        assert!(enabled("quit", &MenuState::default()));
        assert!(enabled(
            "quit",
            &MenuState {
                busy: true,
                ..Default::default()
            }
        ));
    }

    #[test]
    fn bare_alt_toggles_only_after_release_and_f10_focuses() {
        let mut keys = MenuKeys::default();
        assert_eq!(keys.press(MenuKey::LeftAlt, false, false, 1_000), None);
        assert_eq!(keys.release(MenuKey::LeftAlt, false, 1_100), Some(true));
        assert_eq!(keys.press(MenuKey::LeftAlt, false, true, 2_000), None);
        assert_eq!(keys.release(MenuKey::LeftAlt, true, 2_050), Some(false));
        assert_eq!(keys.press(MenuKey::F10, false, false, 3_000), Some(true));
        assert_eq!(keys.press(MenuKey::Escape, false, true, 3_100), Some(false));
        assert_eq!(keys.press(MenuKey::Escape, false, false, 3_200), None);
    }
    #[test]
    fn altgr_ctrl_alt_chords_and_shift_f10_do_not_reveal_menu() {
        let mut keys = MenuKeys::default();
        // AltGr maps to Other; modifier+Alt never arms the bare-Alt gesture.
        for key in [MenuKey::Other, MenuKey::LeftAlt] {
            assert_eq!(keys.press(key, true, false, 0), None);
            assert_eq!(keys.release(key, false, 10), None);
        }
        keys.press(MenuKey::LeftAlt, false, false, 20);
        keys.press(MenuKey::Other, true, false, 30);
        assert_eq!(keys.release(MenuKey::LeftAlt, false, 40), None);
        assert_eq!(keys.press(MenuKey::F10, true, false, 50), None);
    }
    #[test]
    fn focus_loss_or_pointer_action_cancels_pending_alt() {
        let mut keys = MenuKeys::default();
        keys.press(MenuKey::LeftAlt, false, false, 0);
        keys.cancel();
        assert_eq!(keys.release(MenuKey::LeftAlt, false, 10), None);
    }
    #[test]
    fn help_window_keeps_its_page_and_sends_web_links_to_the_browser() {
        let help: tauri::Url = "file:///usr/lib/Silo/docs/silo-help.html".parse().unwrap();
        let at = |url: &str| help_navigation(&url.parse().unwrap(), &help);
        assert_eq!(
            at("file:///usr/lib/Silo/docs/silo-help.html"),
            HelpNavigation::Stay
        );
        assert_eq!(
            at("file:///usr/lib/Silo/docs/silo-help.html#checkpoints"),
            HelpNavigation::Stay
        );
        assert_eq!(
            at("https://github.com/amontlabs/silo/issues"),
            HelpNavigation::Browser
        );
        assert_eq!(at("file:///etc/passwd"), HelpNavigation::Block);
        assert_eq!(at("javascript:alert(1)"), HelpNavigation::Block);
        assert_eq!(at("mailto:someone@example.com"), HelpNavigation::Block);
        assert_eq!(at("about:blank"), HelpNavigation::Block);
    }

    #[test]
    fn held_alt_from_a_window_manager_drag_does_not_toggle_the_menu() {
        let mut keys = MenuKeys::default();
        // Alt+drag on X11 KDE/Xfce: the WM takes the button, Silo sees only Alt.
        keys.press(MenuKey::LeftAlt, false, false, 10_000);
        for repeat in [10_300, 10_600, 10_900] {
            keys.press(MenuKey::LeftAlt, false, false, repeat);
        }
        assert_eq!(keys.release(MenuKey::LeftAlt, false, 11_200), None);
        // A quick tap still toggles, including across the 32-bit timestamp wrap.
        keys.press(MenuKey::LeftAlt, false, false, u32::MAX - 50);
        assert_eq!(keys.release(MenuKey::LeftAlt, false, 100), Some(true));
    }
    #[test]
    fn unready_or_busy_ui_cannot_receive_navigation_or_mutation_commands() {
        let unready = MenuState::default();
        let busy = MenuState {
            ready: true,
            busy: true,
            can_create_computer: true,
            can_import: true,
            can_check_updates: true,
            can_go_back: true,
            can_go_forward: true,
            sidebar_collapsed: false,
        };
        for state in [unready, busy] {
            for command in [
                "settings",
                "new-computer",
                "import-computer",
                "check-updates",
                "search",
                "go-back",
                "go-forward",
                "go-github",
            ] {
                assert!(!enabled(command, &state), "{command}");
            }
            assert!(enabled("show-window", &state));
        }
    }
    #[test]
    fn action_permissions_do_not_disable_unrelated_navigation() {
        let mut state = MenuState {
            ready: true,
            ..Default::default()
        };
        assert!(!enabled("new-computer", &state));
        assert!(!enabled("import-computer", &state));
        assert!(!enabled("check-updates", &state));
        assert!(enabled("settings", &state));
        assert!(enabled("go-computers", &state));
        state.can_import = true;
        state.can_go_back = true;
        assert!(enabled("import-computer", &state));
        assert!(enabled("go-back", &state));
        assert!(!enabled("go-forward", &state));
        assert!(!enabled("unknown-command", &state));
    }
}
