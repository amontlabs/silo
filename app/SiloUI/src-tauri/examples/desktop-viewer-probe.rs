#[path = "../src/desktop_bridge.rs"]
#[allow(dead_code)]
mod desktop_bridge;
#[path = "../src/desktop_proxy.rs"]
mod desktop_proxy;

use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Deserialize;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    net::TcpStream,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::{
    webview::{Cookie, NewWindowResponse, PageLoadEvent},
    AppHandle, LogicalPosition, LogicalSize, Manager, WebviewBuilder, WebviewUrl,
    WebviewWindowBuilder,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Connection {
    /// Unix socket of an `ssh -L <socket>:127.0.0.1:<port>` forward (G-04).
    socket: PathBuf,
    /// The guest's desktop port.
    port: u16,
    username: String,
    password: String,
}

type ProbeLog = Arc<Mutex<File>>;

fn stage(log: &ProbeLog, message: &str) {
    let line = format!("probe: {message}");
    eprintln!("{line}");
    if let Ok(mut file) = log.lock() {
        let _ = writeln!(file, "{line}");
    }
}

fn read_connection(path: PathBuf) -> Result<Connection, &'static str> {
    let bytes = fs::read(path).map_err(|_| "could not read private connection JSON")?;
    let input: Connection =
        serde_json::from_slice(&bytes).map_err(|_| "invalid private connection JSON")?;
    if input.port == 0
        || !input.socket.is_absolute()
        || input.username.is_empty()
        || input.password.is_empty()
    {
        return Err("invalid connection fields");
    }
    Ok(input)
}

fn capture_rendered_frame(webview: tauri::Webview, log: ProbeLog, frame_path: PathBuf) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(500));
        let script = r#"(() => {
          const canvases = [...document.querySelectorAll('canvas')]
            .map((element) => ({ element, rect: element.getBoundingClientRect() }))
            .filter(({ element, rect }) => element.width > 0 && element.height > 0 && rect.width > 0 && rect.height > 0)
            .sort((a, b) => b.element.width * b.element.height - a.element.width * a.element.height);
          const videos = [...document.querySelectorAll('video')]
            .filter((video) => video.videoWidth > 0 && video.videoHeight > 0 && video.readyState >= 2);
          const canvas = canvases[0]?.element;
          try {
            if (canvas) return { kind: 'canvas', width: canvas.width, height: canvas.height, dataUrl: canvas.toDataURL('image/png') };
            if (videos[0]) {
              const video = videos[0];
              const copy = document.createElement('canvas');
              copy.width = video.videoWidth;
              copy.height = video.videoHeight;
              copy.getContext('2d').drawImage(video, 0, 0);
              return { kind: 'video', width: copy.width, height: copy.height, dataUrl: copy.toDataURL('image/png') };
            }
            return {
              kind: 'no-frame',
              readyState: document.readyState,
              title: document.title,
              canvasCount: document.querySelectorAll('canvas').length,
              canvasSizes: canvases.slice(0, 4).map(({ element }) => [element.width, element.height]),
              videoCount: videos.length,
              videoSizes: videos.slice(0, 4).map((video) => [video.videoWidth, video.videoHeight, video.readyState])
            };
          } catch (error) {
            return { kind: 'capture-error', error: String(error).slice(0, 120) };
          }
        })()"#;
        let callback_log = log.clone();
        let callback_path = frame_path.clone();
        let dispatched = webview.eval_with_callback(script, move |serialized| {
            let result: serde_json::Value = match serde_json::from_str(&serialized) {
                Ok(result) => result,
                Err(_) => {
                    stage(&callback_log, "render probe returned an unreadable result");
                    return;
                }
            };
            let kind = result["kind"].as_str().unwrap_or("unknown");
            if let Some(data_url) = result["dataUrl"].as_str() {
                let Some(encoded) = data_url.strip_prefix("data:image/png;base64,") else {
                    stage(&callback_log, "render probe returned an unsupported image format");
                    return;
                };
                let png = match STANDARD.decode(encoded) {
                    Ok(png) => png,
                    Err(_) => {
                        stage(&callback_log, "render probe image could not be decoded");
                        return;
                    }
                };
                let mut options = OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(unix)]
                options.mode(0o600);
                match options.open(&callback_path).and_then(|mut file| file.write_all(&png)) {
                    Ok(()) => stage(&callback_log, &format!(
                        "render frame captured source={kind} width={} height={} png_bytes={} path={}",
                        result["width"].as_u64().unwrap_or(0),
                        result["height"].as_u64().unwrap_or(0),
                        png.len(),
                        callback_path.display()
                    )),
                    Err(_) => stage(&callback_log, "render frame image could not be saved"),
                }
            } else {
                stage(&callback_log, &format!(
                    "render frame unavailable kind={kind} ready_state={} canvas_count={} video_count={}",
                    result["readyState"].as_str().unwrap_or("unknown"),
                    result["canvasCount"].as_u64().unwrap_or(0),
                    result["videoCount"].as_u64().unwrap_or(0)
                ));
            }
        });
        stage(
            &log,
            &format!("render probe dispatched (success={})", dispatched.is_ok()),
        );
    });
}

fn proxy_self_check(proxy: &desktop_proxy::Proxy, log: &ProbeLog) {
    let started = Instant::now();
    let result = (|| -> Result<(String, String, String), &'static str> {
        let mut socket =
            TcpStream::connect(("127.0.0.1", proxy.port)).map_err(|_| "loopback_connect")?;
        socket
            .set_read_timeout(Some(Duration::from_secs(4)))
            .map_err(|_| "read_timeout_setup")?;
        write!(socket, "GET / HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nOrigin: http://127.0.0.1:{}\r\nCookie: {}={}\r\nConnection: close\r\n\r\n", proxy.port, proxy.port, proxy.cookie_name, proxy.token).map_err(|_| "request_write")?;
        let mut response = Vec::new();
        let mut chunk = [0; 4096];
        let mut timed_out = false;
        while response.len() < 128 * 1024 {
            match socket.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => response.extend_from_slice(&chunk[..n]),
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                    ) =>
                {
                    timed_out = true;
                    break;
                }
                Err(_) => return Err("response_read"),
            }
        }
        let boundary = response
            .windows(4)
            .position(|bytes| bytes == b"\r\n\r\n")
            .ok_or(if response.is_empty() {
                if timed_out {
                    "response_timeout"
                } else {
                    "no_response"
                }
            } else {
                "incomplete_response_headers"
            })?;
        let headers = String::from_utf8_lossy(&response[..boundary]);
        let status = headers
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .ok_or("invalid_status")?
            .to_owned();
        let mime = headers
            .lines()
            .find_map(|line| {
                line.split_once(':')
                    .filter(|(name, _)| name.eq_ignore_ascii_case("content-type"))
                    .map(|(_, value)| {
                        value
                            .trim()
                            .split(';')
                            .next()
                            .unwrap_or("unknown")
                            .to_owned()
                    })
            })
            .unwrap_or_else(|| "unknown".into());
        let body = String::from_utf8_lossy(&response[boundary + 4..]);
        let lower = body.to_ascii_lowercase();
        let title = lower
            .find("<title>")
            .and_then(|start| {
                let start = start + 7;
                let end = lower[start..].find("</title>")? + start;
                body.get(start..end)
            })
            .map(|value| {
                value
                    .chars()
                    .filter(|ch| ch.is_ascii_alphanumeric() || " -_".contains(*ch))
                    .take(80)
                    .collect()
            })
            .unwrap_or_else(|| "unavailable".into());
        Ok((status, mime, title))
    })();
    match result {
        Ok((status, mime, title)) => stage(
            log,
            &format!(
                "authenticated proxy GET status={status} mime={mime} title={title} elapsed_ms={}",
                started.elapsed().as_millis()
            ),
        ),
        Err(category) => stage(
            log,
            &format!(
                "authenticated proxy GET failed category={category} elapsed_ms={}",
                started.elapsed().as_millis()
            ),
        ),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let input_path = args.next().ok_or("pass private connection JSON path")?;
    let log_path = args.next().ok_or("pass private stage log path")?;
    let frame_path = args.next().ok_or("pass private frame image path")?;
    let mut log_options = OpenOptions::new();
    log_options.write(true).create_new(true);
    #[cfg(unix)]
    log_options.mode(0o600);
    let log = Arc::new(Mutex::new(log_options.open(log_path)?));
    let frame_path = PathBuf::from(frame_path);
    let input = read_connection(PathBuf::from(input_path))?;
    let upstream = input.socket.display().to_string();
    stage(&log, "starting loopback proxy");
    let proxy = desktop_proxy::Proxy::start(
        input.socket.clone(),
        input.port,
        &input.username,
        &input.password,
    )
    .map_err(|error| {
        stage(&log, &format!("proxy startup failed ({error})"));
        error
    })?;
    drop(input);
    stage(
        &log,
        &format!(
            "proxy ready on 127.0.0.1:{}; SSH upstream is {upstream}",
            proxy.port
        ),
    );
    let cleanup = Arc::new(Mutex::new(Some(proxy)));
    let cleanup_for_setup = cleanup.clone();
    let cleanup_on_close = cleanup.clone();
    let frame_path_for_setup = frame_path.clone();

    tauri::Builder::default()
        .setup(move |app| {
            let _parent = parent_window(app.handle())?;
            let window = app
                .get_window("desktop-viewer-probe")
                .ok_or("probe parent window unavailable")?;
            let cleanup_on_destroy = cleanup_on_close.clone();
            let log_on_destroy = log.clone();
            window.on_window_event(move |event| {
                if matches!(event, tauri::WindowEvent::Destroyed) {
                    if let Ok(mut proxy) = cleanup_on_destroy.lock() {
                        proxy.take();
                    }
                    stage(&log_on_destroy, "parent destroyed; proxy stopped");
                }
            });
            let log_for_attach = log.clone();
            let frame_path = frame_path_for_setup.clone();
            std::thread::spawn(move || {
                if let Err(error) = attach_child(
                    window,
                    cleanup_for_setup,
                    log_for_attach.clone(),
                    frame_path,
                ) {
                    stage(&log_for_attach, &format!("child setup failed ({error})"));
                }
            });
            stage(&log, "setup completed");
            Ok(())
        })
        .run(tauri::generate_context!("examples/tauri.conf.json"))?;
    drop(cleanup);
    Ok(())
}

fn attach_child(
    window: tauri::Window,
    cleanup: Arc<Mutex<Option<desktop_proxy::Proxy>>>,
    log: ProbeLog,
    frame_path: PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    let proxy_guard = cleanup.lock().map_err(|_| "probe cleanup unavailable")?;
    let proxy = proxy_guard.as_ref().ok_or("probe proxy unavailable")?;
    let origin = format!("http://127.0.0.1:{}", proxy.port);
    let permitted_origin = origin.clone();
    let load_origin = origin.clone();
    let child_builder = WebviewBuilder::new(
        "guest-probe",
        WebviewUrl::External(tauri::Url::parse("about:blank")?),
    )
    .incognito(true)
    .focused(true)
    .devtools(true)
    .on_page_load({
        let log = log.clone();
        let frame_path = frame_path.clone();
        move |webview, payload| {
            let url = payload.url();
            let safe_url = format!("{}{}", url.origin().ascii_serialization(), url.path());
            match payload.event() {
                PageLoadEvent::Started => stage(&log, &format!("page load started ({safe_url})")),
                PageLoadEvent::Finished => {
                    stage(&log, &format!("page load finished ({safe_url})"));
                    if url.origin().ascii_serialization() == load_origin {
                        capture_rendered_frame(webview.clone(), log.clone(), frame_path.clone());
                    }
                }
            }
        }
    })
    .on_new_window(|_, _| NewWindowResponse::Deny)
    .on_navigation({
        let log = log.clone();
        move |url| {
            let allowed = url.as_str() == "about:blank"
                || url.origin().ascii_serialization() == permitted_origin;
            let safe_url = if url.as_str() == "about:blank" {
                "about:blank".to_owned()
            } else {
                format!("{}{}", url.origin().ascii_serialization(), url.path())
            };
            stage(
                &log,
                &format!(
                    "navigation {safe_url} {}",
                    if allowed { "allowed" } else { "denied" }
                ),
            );
            allowed
        }
    });
    stage(&log, "creating native child");
    let child = window.add_child(
        child_builder,
        LogicalPosition::new(0., 44.),
        LogicalSize::new(1200., 776.),
    )?;
    let cookie_name = proxy.cookie_name.clone();
    let cookie = Cookie::build((cookie_name.clone(), proxy.token.clone()))
        .domain("127.0.0.1")
        .path("/")
        .http_only(true)
        .build();
    let cookie_set = child.set_cookie(cookie).is_ok();
    stage(&log, &format!("set_cookie accepted (success={cookie_set})"));
    if !cookie_set {
        let _ = child.close();
        return Err("cookie setup failed".into());
    }
    let cookie_present = child
        .cookies()?
        .iter()
        .any(|cookie| cookie.name() == cookie_name);
    stage(
        &log,
        &format!("cookie presence confirmed ({cookie_present})"),
    );
    if !cookie_present {
        let _ = child.close();
        return Err("cookie was not present in child webview".into());
    }
    proxy_self_check(proxy, &log);
    let target = tauri::Url::parse(&format!("{origin}/?resize=scale&clipboard_seamless=false"))?;
    let safe_target = format!("{}{}", target.origin().ascii_serialization(), target.path());
    stage(&log, &format!("navigate target={safe_target}"));
    let navigated = child.navigate(target).is_ok();
    stage(&log, &format!("navigate dispatched (success={navigated})"));
    if !navigated {
        let _ = child.close();
        return Err("navigation failed".into());
    }
    child.set_focus()?;
    Ok(())
}

fn parent_window(app: &AppHandle) -> Result<tauri::WebviewWindow, Box<dyn std::error::Error>> {
    Ok(WebviewWindowBuilder::new(
        app,
        "desktop-viewer-probe",
        WebviewUrl::External(tauri::Url::parse("about:blank")?),
    )
    .title("Silo desktop viewer probe")
    .inner_size(1200., 820.)
    .build()?)
}
