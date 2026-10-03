//! A privileged local shell and an unprivileged guest child webview.
use crate::{
    desktop_bridge::Bridge, desktop_proxy::Proxy, editor, owned_tunnel::Tunnel, remote,
    remote_access, runtime,
};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs,
    os::unix::{
        fs::{FileTypeExt, MetadataExt},
        net::UnixStream,
    },
    path::{Path, PathBuf},
    process::Command,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
use tauri::{
    AppHandle, LogicalPosition, LogicalSize, Manager, WebviewBuilder, WebviewUrl,
    WebviewWindowBuilder, Window,
};

struct Viewer {
    computer: String,
    proxy: Option<Proxy>,
    tunnel: Option<Tunnel>,
    /// Bumped whenever the connection is replaced or torn down, so a connect
    /// that finishes after a close or a newer attach cannot install itself.
    generation: u64,
    connecting: bool,
}
impl Viewer {
    fn new(computer: String) -> Self {
        Self {
            computer,
            proxy: None,
            tunnel: None,
            generation: 0,
            connecting: false,
        }
    }
    /// Takes the connection out so the caller can drop it after unlocking.
    fn disconnect(&mut self) -> (Option<Proxy>, Option<Tunnel>) {
        self.generation += 1;
        self.connecting = false;
        (self.proxy.take(), self.tunnel.take())
    }
    fn healthy(&mut self) -> bool {
        self.proxy.as_ref().is_some_and(Proxy::running)
            && self.tunnel.as_mut().is_some_and(Tunnel::running)
    }
}
/// What `desktop_viewer_attach` must do once the registry lock is released.
enum AttachPlan {
    /// The display is connected; only its bounds change.
    Resize,
    /// Connect a new display and install it only if `generation` still matches.
    Connect {
        generation: u64,
        stale: (Option<Proxy>, Option<Tunnel>),
    },
}
/// Decides under the registry lock; every slow step runs after it is released.
/// Holding the lock across `connect()` or `add_child()` deadlocks the main
/// thread's `Destroyed` handler (G-01).
fn begin_attach(
    entries: &mut HashMap<String, Viewer>,
    label: &str,
    computer: &str,
    has_view: bool,
) -> Result<AttachPlan, String> {
    let entry = entries
        .get_mut(label)
        .filter(|v| v.computer == computer)
        .ok_or("Desktop viewer closed.")?;
    if entry.connecting {
        return Err("The desktop is still connecting.".into());
    }
    if has_view && entry.healthy() {
        return Ok(AttachPlan::Resize);
    }
    let stale = entry.disconnect();
    entry.connecting = true;
    Ok(AttachPlan::Connect {
        generation: entry.generation,
        stale,
    })
}
/// Installs a finished connection, or hands it back when the viewer closed or
/// was reset meanwhile so the caller can discard it outside the lock.
fn finish_attach(
    entries: &mut HashMap<String, Viewer>,
    label: &str,
    generation: u64,
    proxy: Option<Proxy>,
    tunnel: Option<Tunnel>,
) -> Result<(), (Option<Proxy>, Option<Tunnel>)> {
    match entries
        .get_mut(label)
        .filter(|v| v.connecting && v.generation == generation)
    {
        Some(entry) => {
            entry.connecting = false;
            entry.proxy = proxy;
            entry.tunnel = tunnel;
            Ok(())
        }
        None => Err((proxy, tunnel)),
    }
}
fn abort_attach(entries: &mut HashMap<String, Viewer>, label: &str, generation: u64) {
    if let Some(entry) = entries
        .get_mut(label)
        .filter(|v| v.connecting && v.generation == generation)
    {
        entry.connecting = false;
    }
}
enum ViewerClaim {
    /// A viewer for this computer exists or is being created.
    Existing(String),
    /// The caller reserved this label and must create its window.
    New(String),
}
/// Reserves one viewer per computer, so a concurrent second open finds the
/// pending entry instead of creating a duplicate window and tunnel.
fn claim_viewer(
    entries: &mut HashMap<String, Viewer>,
    computer: &str,
) -> Result<ViewerClaim, String> {
    if let Some((label, _)) = entries.iter().find(|(_, v)| v.computer == computer) {
        return Ok(ViewerClaim::Existing(label.clone()));
    }
    if entries.len() >= 16 {
        return Err("Close an unused desktop viewer first.".into());
    }
    let label = format!("desktop-shell-{}", uuid::Uuid::new_v4().simple());
    entries.insert(label.clone(), Viewer::new(computer.into()));
    Ok(ViewerClaim::New(label))
}
static VIEWERS: OnceLock<Mutex<HashMap<String, Viewer>>> = OnceLock::new();
fn viewers() -> &'static Mutex<HashMap<String, Viewer>> {
    VIEWERS.get_or_init(|| Mutex::new(HashMap::new()))
}
pub(crate) fn require_computer(window: &Window, computer: &str) -> Result<(), String> {
    if window.label() == "main" {
        return Ok(());
    }
    if viewers()
        .lock()
        .map_err(|_| "Desktop unavailable.")?
        .get(window.label())
        .is_some_and(|v| v.computer == computer)
    {
        Ok(())
    } else {
        Err("This window cannot access that desktop.".into())
    }
}
pub(crate) fn local_connection(
    app: &AppHandle,
    computer: &str,
    expected_id: Option<&str>,
) -> Result<Value, String> {
    // Reading desktop connection credentials only observes a running computer; it takes
    // no operation gate so viewing stays available during other operations.
    runtime::shutdown::ensure_accepting_operations()?;
    crate::desktop::connection_local(app, computer, expected_id)
}

/// `sun_path` holds 104 bytes on macOS and 108 on Linux, including the NUL.
const SOCKET_PATH_MAX: usize = 103;

/// Forward the guest listener to a Unix socket (G-04). Unlike a loopback TCP
/// port, the socket sits in a private directory: no other local process can
/// connect to the guest through it or bind it first to receive the viewer's
/// credentials.
fn forward_command(
    config: &Path,
    alias: &str,
    socket: &Path,
    guest_port: u16,
) -> Result<std::process::Command, String> {
    let socket = socket
        .to_str()
        // ssh splits -L on ':'; sun_path bounds the length.
        .filter(|socket| !socket.contains(':') && socket.len() <= SOCKET_PATH_MAX)
        .ok_or("Your home folder's path is too long for a desktop connection.")?;
    let mut command = std::process::Command::new("/usr/bin/ssh");
    command
        .arg("-F")
        .arg(config)
        .args([
            "-N",
            "-o",
            "ExitOnForwardFailure=yes",
            "-o",
            "ConnectTimeout=15",
            "-o",
            "StreamLocalBindUnlink=yes",
            "-o",
            "StreamLocalBindMask=0177",
            // A dead transport ends the forward instead of holding it (G-16).
            "-o",
            "ServerAliveInterval=15",
            "-o",
            "ServerAliveCountMax=3",
            "-L",
        ])
        .arg(format!("{socket}:127.0.0.1:{guest_port}"))
        .arg(alias);
    Ok(command)
}

/// A fresh 0700 directory inside Silo's private `~/.silo`, short enough for
/// `sun_path` whatever the computer or device identifiers are.
fn socket_directory(root: &Path) -> Result<tempfile::TempDir, String> {
    use std::os::unix::fs::PermissionsExt;
    runtime::prepare_private_directory(root).map_err(|error| error.to_string())?;
    tempfile::Builder::new()
        .prefix("desktop-")
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(root)
        .map_err(|_| "Could not prepare the desktop connection.".into())
}

/// True once the tunnel's socket accepts connections. Anything at that path
/// other than a socket owned by this account is refused, never connected to.
fn socket_ready(path: &Path, uid: u32) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err("Could not check the desktop connection.".into()),
        Ok(metadata) if !metadata.file_type().is_socket() || metadata.uid() != uid => {
            Err("The desktop connection is not private. Reconnect the desktop.".into())
        }
        Ok(_) => Ok(UnixStream::connect(path).is_ok()),
    }
}

fn connect(app: &AppHandle, computer: &str) -> Result<(Proxy, Option<Tunnel>), String> {
    editor::require_openssh("view computer desktops")?;
    let remote_target = remote_access::target(computer)?;
    let connection = if let Some((device, computer)) = &remote_target {
        remote::call_remote(
            app,
            device,
            "desktop.connect",
            json!({"computerId":computer}),
        )?
    } else {
        local_connection(app, computer, None)?
    };
    let guest = connection["port"]
        .as_u64()
        .and_then(|p| u16::try_from(p).ok())
        .filter(|p| *p != 0)
        .ok_or("Invalid desktop endpoint.")?;
    let (alias, config) = if let Some((device, computer)) = remote_target {
        editor::prepare_remote_private(app, &device, &computer, "/workspace")?
    } else {
        let paths = runtime::runtime_paths(app)?;
        let directory = paths.home.join("ssh/desktop-viewer").join(computer);
        editor::prepare_private_transport(&paths, computer, &directory)?
    };
    let home = app
        .path()
        .home_dir()
        .map_err(|_| "Could not prepare the desktop connection.")?;
    let directory = socket_directory(&crate::channel::current().state_dir(&home))?;
    let socket: PathBuf = directory.path().join("desktop.sock");
    let command = forward_command(&config, &alias, &socket, guest)?;
    let mut tunnel = Tunnel::spawn(&command, Some(directory))
        .map_err(|_| "Could not connect to the desktop.")?;
    let uid = unsafe { libc::geteuid() };
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if !tunnel.running() {
            return Err("Desktop tunnel closed. Check the computer connection.".into());
        }
        if socket_ready(&socket, uid)? {
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
fn viewer_title(name: &str, channel: crate::channel::Channel) -> String {
    format!("{name} — {}", channel.product_name())
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
    tauri::async_runtime::spawn_blocking(move || {
        runtime::shutdown::ensure_accepting_operations()?;
        let name = if let Some((device, computer)) = remote_access::target(&computer)? {
            // Verify the remote identity before creating a shell.
            let state = remote::call_remote(
                &app,
                &device,
                "desktop.status",
                json!({"computerId":computer}),
            )?;
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
            let paths = runtime::runtime_paths(&app)?;
            if !runtime::read_metadata(&paths.metadata)
                .map_err(|e| e.to_string())?
                .computers
                .iter()
                .any(|m| m.name() == computer)
            {
                return Err("Computer no longer exists.".into());
            }
            computer.clone()
        };
        let mut route = tauri::Url::parse("http://silo.local/index.html").unwrap();
        route
            .query_pairs_mut()
            .append_pair("desktop", &computer)
            .append_pair("name", &name);
        let route = format!("index.html?{}", route.query().unwrap());
        let claim = {
            let mut entries = viewers().lock().map_err(|_| "Desktop unavailable.")?;
            claim_viewer(&mut entries, &computer)?
        };
        // Window calls run on the main thread; never make them under the lock.
        let label = match claim {
            ViewerClaim::Existing(label) => {
                // Without a window yet, another call is still creating it (G-15).
                let Some(existing) = app.get_window(&label) else {
                    return Ok(());
                };
                let _ = existing.show();
                return existing
                    .set_focus()
                    .map_err(|_| "Could not focus desktop.".into());
            }
            ViewerClaim::New(label) => label,
        };
        let result = WebviewWindowBuilder::new(&app, &label, WebviewUrl::App(route.into()))
            .title(viewer_title(&name, crate::channel::current()))
            .inner_size(1200., 820.)
            .min_inner_size(672., 480.)
            .build();
        let viewer = match result {
            Ok(v) => v,
            Err(_) => {
                viewers().lock().ok().map(|mut e| e.remove(&label));
                return Err("Could not open desktop viewer.".into());
            }
        };
        let menu_app = app.clone();
        let shortcut_label = label.clone();
        viewer.on_window_event(move |event| match event {
            tauri::WindowEvent::Destroyed => {
                crate::viewer_shortcuts::uninstall(&shortcut_label);
                crate::app_menu::set_viewer_focus(&menu_app, false);
                // Attach never holds the lock across window work, so this cannot
                // wait on the main thread; reap the tunnel after unlocking.
                let removed = viewers().lock().ok().and_then(|mut e| e.remove(&label));
                drop(removed);
            }
            tauri::WindowEvent::Focused(focused) => {
                crate::app_menu::set_viewer_focus(&menu_app, *focused);
            }
            _ => {}
        });
        let (shortcut_app, shortcut_window) = (app.clone(), viewer.clone());
        let _ = viewer.run_on_main_thread(move || {
            crate::viewer_shortcuts::install(&shortcut_app, &shortcut_window);
        });
        Ok(())
    })
    .await
    .map_err(|_| "Desktop window operation failed.")?
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
) -> Result<LogicalPosition<f64>, String> {
    if !scale.is_finite() || scale <= 0. {
        return Err("Could not determine desktop display scale.".into());
    }
    Ok(LogicalPosition::new(
        f64::from(shell_x) / scale + css_x,
        f64::from(shell_y) / scale + css_y,
    ))
}

/// Locks the clipboard and capture APIs in guest frames, seeds the Selkies
/// client settings and installs the page half of the host bridge; behaviour is
/// covered by `desktop/linux-desktop-guest-guard.test.ts` and
/// `desktop/linux-desktop-bridge.test.ts`.
const GUEST_BRIDGE_SCRIPT: &str = include_str!("desktop_viewer_bridge.js");

pub(crate) fn is_viewer_label(label: &str) -> bool {
    label.starts_with("desktop-shell-")
}

/// Runs `f` against the bridge of one viewer's guest page. The bridge waits for
/// the page's answers, so call this from a worker thread, never the main thread.
pub(crate) fn with_bridge<R>(
    app: &AppHandle,
    label: &str,
    f: impl FnOnce(&Bridge) -> Result<R, String>,
) -> Result<R, String> {
    let inbox = viewers()
        .lock()
        .map_err(|_| "Desktop unavailable.")?
        .get(label)
        .and_then(|viewer| viewer.proxy.as_ref())
        .filter(|proxy| proxy.running())
        .map(|proxy| proxy.inbox.clone())
        .ok_or("The desktop is not connected.")?;
    let view = app
        .get_webview(&format!("guest-{label}"))
        .ok_or("The desktop display is closed.")?;
    f(&Bridge {
        page: &view,
        inbox: &inbox,
    })
}

fn viewer_url(origin: &str) -> tauri::Url {
    // Selkies reads only token, offscreen_worker and socket_worker from the URL.
    // Its other settings come from localStorage, which the bridge script seeds.
    tauri::Url::parse(&format!("{origin}/")).unwrap()
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
    tauri::async_runtime::spawn_blocking(move || {
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
        let position = desktop_position(shell_origin.x, shell_origin.y, scale, x, y + inset_y)?;
        let label = format!("guest-{}", window.label());
        let existing = app.get_webview(&label);
        let plan = {
            let mut entries = viewers().lock().map_err(|_| "Desktop unavailable.")?;
            begin_attach(&mut entries, window.label(), &computer, existing.is_some())?
        };
        let generation = match plan {
            AttachPlan::Resize => {
                let view = existing.ok_or("Desktop viewer closed.")?;
                return view
                    .set_bounds(tauri::Rect {
                        position: position.into(),
                        size: LogicalSize::new(width, height).into(),
                    })
                    .map_err(|_| "Could not resize desktop.".into());
            }
            AttachPlan::Connect { generation, stale } => {
                drop(stale);
                generation
            }
        };
        let abort = |message: &str| -> String {
            if let Ok(mut entries) = viewers().lock() {
                abort_attach(&mut entries, window.label(), generation);
            }
            message.into()
        };
        if let Some(view) = existing {
            view.close()
                .map_err(|_| abort("Could not reconnect desktop."))?;
        }
        let (proxy, tunnel) = connect(&app, &computer).map_err(|e| abort(&e))?;
        let origin = format!("http://127.0.0.1:{}", proxy.port);
        let permitted = origin.clone();
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
        });
        let view = window
            .add_child(builder, position, LogicalSize::new(width, height))
            .map_err(|_| abort("Could not create desktop display."))?;
        let cookie =
            tauri::webview::Cookie::build((proxy.cookie_name.clone(), proxy.token.clone()))
                .domain("127.0.0.1")
                .path("/")
                .http_only(true)
                .build();
        if view.set_cookie(cookie).is_err() || view.navigate(viewer_url(&origin)).is_err() {
            let _ = view.close();
            return Err(abort("Could not authenticate desktop viewer."));
        }
        let rejected = match viewers().lock() {
            Ok(mut entries) => finish_attach(
                &mut entries,
                window.label(),
                generation,
                Some(proxy),
                tunnel,
            )
            .err(),
            Err(_) => Some((Some(proxy), tunnel)),
        };
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
    tauri::async_runtime::spawn_blocking(move || {
        let stale = viewers()
            .lock()
            .map_err(|_| "Desktop unavailable.")?
            .get_mut(window.label())
            .ok_or("Unknown desktop viewer.")?
            .disconnect();
        drop(stale);
        if let Some(view) = app.get_webview(&format!("guest-{}", window.label())) {
            let _ = view.close();
        }
        Ok(())
    })
    .await
    .map_err(|_| "Desktop disconnect failed.")?
}
fn disconnect_matching(matches: impl Fn(&Viewer) -> bool) {
    let stale: Vec<_> = match viewers().lock() {
        Ok(mut entries) => entries
            .values_mut()
            .filter(|entry| matches(entry))
            .map(Viewer::disconnect)
            .collect(),
        Err(_) => return,
    };
    drop(stale);
}
pub(crate) fn close_all() {
    disconnect_matching(|_| true);
}
pub(crate) fn close_computer(computer: &str) {
    disconnect_matching(|entry| entry.computer == computer);
}
pub(crate) fn close_device(device: &str) {
    let prefix = format!("silo-remote:{device}:");
    disconnect_matching(|entry| entry.computer.starts_with(&prefix));
}

#[cfg(test)]
mod geometry_tests {
    use super::*;

    #[test]
    fn macos_css_viewport_inset_matches_live_retina_geometry_and_fullscreen() {
        let inset = viewport_inset(1640, 2., 788.).unwrap();
        assert_eq!(inset, 32.);
        assert_eq!(
            desktop_position(0, 0, 2., 0., 44. + inset).unwrap(),
            LogicalPosition::new(0., 76.)
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
    fn child_position_includes_titlebar_inset_at_each_display_scale() {
        for scale in [1., 1.5, 2., 3.] {
            let offset = (28. * scale) as i32;
            let actual = desktop_position(0, offset, scale, 12., 48.).unwrap();
            assert_eq!(actual, LogicalPosition::new(12., 76.));
        }
    }

    #[test]
    fn child_position_translates_both_axes_without_scaling_css_twice() {
        assert_eq!(
            desktop_position(24, 56, 2., 8., 48.).unwrap(),
            LogicalPosition::new(20., 76.)
        );
        assert_eq!(
            desktop_position(0, 0, 2., 8., 48.).unwrap(),
            LogicalPosition::new(8., 48.)
        );
        for invalid in [0., -1., f64::INFINITY, f64::NAN] {
            assert!(desktop_position(0, 0, invalid, 0., 0.).is_err());
        }
    }
}

#[cfg(test)]
mod title_tests {
    use super::*;
    use crate::channel::Channel;

    #[test]
    fn desktop_window_titles_identify_the_build_channel() {
        assert_eq!(
            viewer_title("dev · Office", Channel::Production),
            "dev · Office — Silo"
        );
        assert_eq!(
            viewer_title("dev · Office", Channel::Development),
            "dev · Office — Silo Dev"
        );
    }
}

#[cfg(test)]
mod input_tests {
    use super::*;

    #[test]
    fn viewer_urls_carry_no_stale_client_settings() {
        // Local and SSH-tunneled desktops both receive a fresh loopback origin.
        for port in [42001, 53102] {
            let origin = format!("http://127.0.0.1:{port}");
            let url = viewer_url(&origin);
            assert_eq!(url.origin().ascii_serialization(), origin);
            assert_eq!(url.path(), "/");
            assert_eq!(url.query(), None);
        }
    }

    #[test]
    fn only_desktop_shell_windows_count_as_viewers() {
        assert!(is_viewer_label("desktop-shell-0123"));
        assert!(!is_viewer_label("guest-desktop-shell-0123"));
        assert!(!is_viewer_label("main"));
    }
}

#[cfg(test)]
mod transport_tests {
    use super::*;
    use std::process::Stdio;

    #[test]
    fn desktop_forward_uses_pinned_ssh_config_and_a_private_socket() {
        let command = forward_command(
            Path::new("/tmp/silo-private-ssh.conf"),
            "silo-remote-host-computer",
            Path::new("/home/user/.silo/desktop-abc/desktop.sock"),
            6901,
        )
        .unwrap();
        assert_eq!(command.get_program().to_string_lossy(), "/usr/bin/ssh");
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            [
                "-F",
                "/tmp/silo-private-ssh.conf",
                "-N",
                "-o",
                "ExitOnForwardFailure=yes",
                "-o",
                "ConnectTimeout=15",
                "-o",
                "StreamLocalBindUnlink=yes",
                "-o",
                "StreamLocalBindMask=0177",
                "-o",
                "ServerAliveInterval=15",
                "-o",
                "ServerAliveCountMax=3",
                "-L",
                "/home/user/.silo/desktop-abc/desktop.sock:127.0.0.1:6901",
                "silo-remote-host-computer",
            ]
        );
        // No TCP listener: nothing binds a loopback port for the guest.
        assert!(!args.iter().any(|arg| arg.starts_with("127.0.0.1:")));
        // The system OpenSSH reads it as a Unix-socket forward.
        let command = forward_command(
            Path::new("/dev/null"),
            "silo-test-alias",
            Path::new("/tmp/silo-test/desktop.sock"),
            6901,
        )
        .unwrap();
        let parsed = std::process::Command::new("/usr/bin/ssh")
            .arg("-G")
            .args(command.get_args())
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(parsed.status.success());
        let parsed = String::from_utf8(parsed.stdout).unwrap();
        assert!(
            parsed.contains("localforward /tmp/silo-test/desktop.sock [127.0.0.1]:6901\n"),
            "{parsed}"
        );
        assert!(parsed.contains("streamlocalbindmask 0177\n"));
    }

    #[test]
    fn socket_paths_that_ssh_or_sun_path_cannot_hold_are_refused() {
        let config = Path::new("/tmp/config");
        let long = format!("/home/{}/desktop.sock", "u".repeat(100));
        for socket in ["/home/a:b/.silo/desktop-x/desktop.sock", long.as_str()] {
            assert!(forward_command(config, "alias", Path::new(socket), 6901).is_err());
        }
    }

    #[test]
    fn the_socket_directory_is_private_and_removed_with_the_tunnel() {
        use std::os::unix::fs::PermissionsExt;
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join(".silo");
        let directory = socket_directory(&root).unwrap();
        let path = directory.path().to_path_buf();
        for private in [&root, &path] {
            let metadata = fs::metadata(private).unwrap();
            assert_eq!(
                metadata.permissions().mode() & 0o077,
                0,
                "{}",
                private.display()
            );
            assert_eq!(metadata.uid(), unsafe { libc::geteuid() });
        }
        drop(Tunnel::spawn(Command::new("sleep").arg("30"), Some(directory)).unwrap());
        assert!(!path.exists());
    }

    #[test]
    fn readiness_accepts_only_this_accounts_listening_socket() {
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("desktop.sock");
        let uid = unsafe { libc::geteuid() };
        assert!(
            !socket_ready(&socket, uid).unwrap(),
            "missing socket is not ready yet"
        );
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        assert!(socket_ready(&socket, uid).unwrap());
        // A socket another account created first is never used.
        assert!(socket_ready(&socket, uid + 1).is_err());
        drop(listener);
        fs::remove_file(&socket).unwrap();
        fs::write(&socket, b"not a socket").unwrap();
        assert!(socket_ready(&socket, uid).is_err());
    }

    /// A forward stand-in that records its pid, then runs until it is ended.
    fn recorded_forward(directory: &Path) -> (Command, PathBuf) {
        let pid_file = directory.join("forward.pid");
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", "echo $$ > \"$1\"; exec sleep 30", "forward"])
            .arg(&pid_file);
        (command, pid_file)
    }
    fn recorded_pid(file: &Path) -> i32 {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(pid) = fs::read_to_string(file)
                .ok()
                .and_then(|text| text.trim().parse().ok())
            {
                return pid;
            }
            assert!(Instant::now() < deadline, "the forward never started");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    /// Ended and reaped (by the watchdog, or by init once it is orphaned).
    fn ended(pid: i32) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if unsafe { libc::kill(pid, 0) } == -1
                && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
            {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    #[test]
    fn closing_viewer_tunnel_ends_its_ssh_child() {
        let directory = tempfile::tempdir().unwrap();
        let (forward, pid_file) = recorded_forward(directory.path());
        let tunnel = Tunnel::spawn(&forward, None).unwrap();
        let pid = recorded_pid(&pid_file);
        drop(tunnel);
        assert!(ended(pid));
    }

    #[test]
    fn a_crashed_silo_leaves_no_tunnel_running() {
        let directory = tempfile::tempdir().unwrap();
        let (forward, pid_file) = recorded_forward(directory.path());
        let mut tunnel = Tunnel::spawn(&forward, None).unwrap();
        let pid = recorded_pid(&pid_file);
        // A crash or force-quit closes Silo's end of the pipe without Drop.
        tunnel.close_lifetime_pipe();
        assert!(ended(pid), "the forward outlived Silo");
        let deadline = Instant::now() + Duration::from_secs(5);
        while tunnel.running() {
            assert!(Instant::now() < deadline, "the watchdog outlived Silo");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn closing_or_crashing_reaps_term_ignoring_forward_processes() {
        for (script, crashed) in [
            ("trap '' TERM; echo $$ > \"$1\"; exec sleep 30", false),
            ("trap '' TERM; echo $$ > \"$1\"; exec sleep 30", true),
            (
                "sh -c 'trap \"\" TERM; echo $$ > \"$1\"; exec sleep 30' forward \"$1\" & wait",
                false,
            ),
            (
                "sh -c 'trap \"\" TERM; echo $$ > \"$1\"; exec sleep 30' forward \"$1\" & wait",
                true,
            ),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let pid_file = directory.path().join("forward.pid");
            let mut forward = Command::new("/bin/sh");
            forward.args(["-c", script, "forward"]).arg(&pid_file);
            let mut tunnel = Tunnel::spawn(&forward, None).unwrap();
            let group = tunnel.group_id();
            let pid = recorded_pid(&pid_file);
            assert_eq!(unsafe { libc::getpgid(pid) }, group);
            if crashed {
                tunnel.close_lifetime_pipe();
            } else {
                drop(tunnel);
            }
            let reaped = ended(pid);
            if !reaped && unsafe { libc::getpgid(pid) } == group {
                // Clean up only the recorded fixture's still-reserved group.
                unsafe { libc::killpg(group, libc::SIGKILL) };
                assert!(ended(pid));
            }
            assert!(reaped, "the TERM-ignoring forward outlived its tunnel");
        }
    }

    #[test]
    fn a_forward_that_exits_ends_its_watchdog() {
        let mut tunnel =
            Tunnel::spawn(Command::new("/bin/sh").args(["-c", "exit 3"]), None).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while tunnel.running() {
            assert!(
                Instant::now() < deadline,
                "a closed forward still looks connected"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn the_forward_ends_on_a_dead_transport() {
        let command = forward_command(
            Path::new("/dev/null"),
            "silo-test-alias",
            Path::new("/tmp/silo-test/desktop.sock"),
            6901,
        )
        .unwrap();
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(args
            .windows(2)
            .any(|pair| pair == ["-o", "ServerAliveInterval=15"]));
        assert!(args
            .windows(2)
            .any(|pair| pair == ["-o", "ServerAliveCountMax=3"]));
    }
}

#[cfg(test)]
mod registry_tests {
    use super::*;

    fn registry() -> HashMap<String, Viewer> {
        HashMap::from([("shell".to_string(), Viewer::new("dev".into()))])
    }
    fn live_tunnel() -> Tunnel {
        Tunnel::spawn(Command::new("sleep").arg("30"), None).unwrap()
    }

    #[test]
    fn a_failed_proxy_listener_reconnects_despite_a_live_tunnel() {
        struct ListenerExit(std::sync::mpsc::Sender<()>);
        impl Drop for ListenerExit {
            fn drop(&mut self) {
                let _ = self.0.send(());
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let (ended_tx, ended_rx) = std::sync::mpsc::channel();
        let notify = ListenerExit(ended_tx);
        let proxy = Proxy::start_with_accept(
            directory.path().join("unused.sock"),
            6901,
            "silo",
            "password",
            move |_| {
                let _notify = &notify;
                Err(std::io::ErrorKind::Other.into())
            },
        )
        .unwrap();
        // The callback's captures drop only when the listener worker has exited.
        ended_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        let mut entries = registry();
        let entry = entries.get_mut("shell").unwrap();
        entry.proxy = Some(proxy);
        entry.tunnel = Some(live_tunnel());
        assert!(
            matches!(
                begin_attach(&mut entries, "shell", "dev", true),
                Ok(AttachPlan::Connect {
                    stale: (Some(_), Some(_)),
                    ..
                })
            ),
            "an exited listener must reconnect instead of resizing a dead display"
        );
    }

    #[test]
    fn a_viewer_closed_while_connecting_rejects_the_late_connection() {
        let mut entries = registry();
        let AttachPlan::Connect { generation, .. } =
            begin_attach(&mut entries, "shell", "dev", false).unwrap()
        else {
            panic!("expected a connect plan");
        };
        // The lock is free while connecting: the Destroyed handler removes it.
        entries.remove("shell");
        let (_, tunnel) =
            finish_attach(&mut entries, "shell", generation, None, Some(live_tunnel()))
                .expect_err("closed viewer must not accept the connection");
        assert!(tunnel.is_some(), "the caller reaps the rejected tunnel");
    }

    #[test]
    fn a_reset_during_connect_discards_the_stale_connection() {
        let mut entries = registry();
        let AttachPlan::Connect { generation, .. } =
            begin_attach(&mut entries, "shell", "dev", false).unwrap()
        else {
            panic!("expected a connect plan");
        };
        assert!(begin_attach(&mut entries, "shell", "dev", false).is_err());
        entries.get_mut("shell").unwrap().disconnect();
        assert!(finish_attach(&mut entries, "shell", generation, None, None).is_err());
        assert!(matches!(
            begin_attach(&mut entries, "shell", "dev", false),
            Ok(AttachPlan::Connect { .. })
        ));
    }

    #[test]
    fn a_finished_connection_is_installed_and_later_attaches_only_resize() {
        let mut entries = registry();
        let AttachPlan::Connect { generation, .. } =
            begin_attach(&mut entries, "shell", "dev", false).unwrap()
        else {
            panic!("expected a connect plan");
        };
        assert!(
            finish_attach(&mut entries, "shell", generation, None, Some(live_tunnel())).is_ok()
        );
        let entry = entries.get_mut("shell").unwrap();
        assert!(!entry.connecting);
        assert!(entry.tunnel.is_some());
        // Without a proxy the display is not healthy, so attach reconnects.
        assert!(matches!(
            begin_attach(&mut entries, "shell", "dev", true),
            Ok(AttachPlan::Connect {
                stale: (_, Some(_)),
                ..
            })
        ));
        assert!(begin_attach(&mut entries, "shell", "other", true).is_err());
    }

    #[test]
    fn a_concurrent_second_open_reuses_the_pending_viewer() {
        let mut entries = HashMap::new();
        let ViewerClaim::New(first) = claim_viewer(&mut entries, "dev").unwrap() else {
            panic!("expected a new viewer");
        };
        // The first window is not built yet; the second open must not add one.
        let ViewerClaim::Existing(second) = claim_viewer(&mut entries, "dev").unwrap() else {
            panic!("expected the pending viewer");
        };
        assert_eq!(first, second);
        assert_eq!(entries.len(), 1);
        assert!(matches!(
            claim_viewer(&mut entries, "other"),
            Ok(ViewerClaim::New(_))
        ));
        for index in 0..14 {
            claim_viewer(&mut entries, &format!("computer-{index}")).unwrap();
        }
        assert!(claim_viewer(&mut entries, "one-too-many").is_err());
    }

    #[test]
    fn a_failed_connect_clears_the_connecting_mark() {
        let mut entries = registry();
        let AttachPlan::Connect { generation, .. } =
            begin_attach(&mut entries, "shell", "dev", false).unwrap()
        else {
            panic!("expected a connect plan");
        };
        abort_attach(&mut entries, "shell", generation);
        assert!(begin_attach(&mut entries, "shell", "dev", false).is_ok());
    }
}
