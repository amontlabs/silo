//! Loopback WebSocket endpoint for the noVNC viewer of a macOS computer on another device.
//! Each WebSocket connection is spliced to one upstream byte stream (a `macos.display.stream`
//! of the owning device) and carries RFB bytes as binary messages.
//!
//! Only the viewer window can use it: the endpoint listens on 127.0.0.1 only, the URL path is
//! a random per-window token, the `Host` header must name the endpoint and the `Origin` must be
//! one of the app's own web origins. One connection is served at a time; a newer one ends
//! the older. The WebSocket protocol is [`tungstenite`] (MIT or Apache-2.0).
use std::{
    io::{Read, Write},
    net::{Shutdown, TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{sync_channel, RecvTimeoutError},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};
use tungstenite::{
    handshake::server::{ErrorResponse, Request, Response},
    http, Message,
};

/// How long a client may take to complete the WebSocket handshake.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
/// The longest a read waits for the browser before the loop looks at the upstream again.
const POLL: Duration = Duration::from_millis(4);
const CHUNK: usize = 64 * 1024;
/// Messages from the browser carry key and pointer events; none is large.
const MAX_MESSAGE: usize = 1 << 20;
/// Upstream chunks buffered between the reader and the WebSocket.
const BUFFERED_CHUNKS: usize = 32;

/// An upstream byte stream and whatever must be dropped when the connection ends (a child
/// process, for a bridge stream).
pub(crate) struct Upstream {
    pub reader: Box<dyn Read + Send>,
    pub writer: Box<dyn Write + Send>,
    pub keep: Box<dyn Send>,
}

/// Opens the upstream of one WebSocket connection.
pub(crate) type Open = Arc<dyn Fn() -> Result<Upstream, String> + Send + Sync>;

pub(crate) struct Gateway {
    port: u16,
    token: String,
    stopped: Arc<AtomicBool>,
}

impl Drop for Gateway {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
    }
}

/// What the handshake callback checks about a request.
struct Policy {
    port: u16,
    token: String,
    origins: Vec<String>,
}

fn forbidden() -> ErrorResponse {
    http::Response::builder()
        .status(http::StatusCode::FORBIDDEN)
        .body(None)
        .expect("a static response is valid")
}

impl Policy {
    /// Accepts a request only for this endpoint's token, host and an allowed origin, and
    /// answers the `binary` subprotocol noVNC offers.
    fn check(&self, request: &Request, mut response: Response) -> Result<Response, ErrorResponse> {
        let header = |name: &str| {
            request
                .headers()
                .get(name)
                .and_then(|value| value.to_str().ok())
        };
        if request.uri().path() != format!("/{}", self.token)
            || request.uri().query().is_some()
            || header("host") != Some(format!("127.0.0.1:{}", self.port).as_str())
            || !header("origin").is_some_and(|origin| self.origins.iter().any(|o| o == origin))
        {
            return Err(forbidden());
        }
        if let Some(offered) = header("sec-websocket-protocol") {
            if !offered.split(',').any(|name| name.trim() == "binary") {
                return Err(forbidden());
            }
            response.headers_mut().insert(
                "Sec-WebSocket-Protocol",
                http::HeaderValue::from_static("binary"),
            );
        }
        Ok(response)
    }
}

impl Gateway {
    /// Starts listening on a free loopback port. `origins` are the web origins that may connect.
    pub(crate) fn start(open: Open, origins: Vec<String>) -> Result<Self, String> {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .map_err(|_| "Silo could not open the screen connection.".to_string())?;
        listener
            .set_nonblocking(true)
            .map_err(|_| "Silo could not open the screen connection.".to_string())?;
        let port = listener
            .local_addr()
            .map_err(|_| "Silo could not open the screen connection.".to_string())?
            .port();
        let token = uuid::Uuid::new_v4().simple().to_string();
        let stopped = Arc::new(AtomicBool::new(false));
        let policy = Arc::new(Policy {
            port,
            token: token.clone(),
            origins,
        });
        let current: Arc<Current> = Arc::new(Mutex::new(None));
        let worker_stop = stopped.clone();
        thread::spawn(move || {
            while !worker_stop.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((socket, _)) => {
                        let (policy, open, stop, current) = (
                            policy.clone(),
                            open.clone(),
                            worker_stop.clone(),
                            current.clone(),
                        );
                        thread::spawn(move || {
                            serve(socket, &policy, &open, &current, &stop);
                        });
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(30));
                    }
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::Interrupted
                                | std::io::ErrorKind::ConnectionAborted
                                | std::io::ErrorKind::ConnectionReset
                        ) => {}
                    Err(_) => break,
                }
            }
            worker_stop.store(true, Ordering::Release);
        });
        Ok(Self {
            port,
            token,
            stopped,
        })
    }

    /// The URL the viewer connects to; the token is the only secret in it.
    pub(crate) fn url(&self) -> String {
        format!("ws://127.0.0.1:{}/{}", self.port, self.token)
    }

    pub(crate) fn running(&self) -> bool {
        !self.stopped.load(Ordering::Acquire)
    }
}

/// The connection being served, which a newer one that passed the handshake ends.
type Current = Mutex<Option<Arc<AtomicBool>>>;

fn serve(socket: TcpStream, policy: &Policy, open: &Open, current: &Current, stopped: &AtomicBool) {
    // Sockets accepted from a non-blocking listener are non-blocking on macOS.
    if socket.set_nonblocking(false).is_err() {
        return;
    }
    let _ = socket.set_nodelay(true);
    let _ = socket.set_read_timeout(Some(HANDSHAKE_TIMEOUT));
    let _ = socket.set_write_timeout(Some(Duration::from_secs(30)));
    let config = tungstenite::protocol::WebSocketConfig::default()
        .max_message_size(Some(MAX_MESSAGE))
        .max_frame_size(Some(MAX_MESSAGE));
    let callback = |request: &Request, response: Response| policy.check(request, response);
    let Ok(mut socket) = tungstenite::accept_hdr_with_config(socket, callback, Some(config)) else {
        return;
    };
    // Only a client that passed the handshake can end the connection in use.
    let superseded = Arc::new(AtomicBool::new(false));
    let previous = current
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .replace(superseded.clone());
    if let Some(previous) = previous {
        previous.store(true, Ordering::Release);
    }
    let upstream = match open() {
        Ok(upstream) => upstream,
        Err(message) => {
            close(&mut socket, 1011, &message);
            return;
        }
    };
    if superseded.load(Ordering::Acquire) || stopped.load(Ordering::Acquire) {
        return;
    }
    splice(socket, upstream, &superseded, stopped);
}

/// Sends a close frame; the reason is cut to what a control frame can carry.
fn close(socket: &mut tungstenite::WebSocket<TcpStream>, code: u16, reason: &str) {
    let reason: String = reason.chars().take(100).collect();
    let _ = socket.close(Some(tungstenite::protocol::CloseFrame {
        code: code.into(),
        reason: reason.into(),
    }));
    let _ = socket.flush();
}

/// Moves bytes between the WebSocket and the upstream until either side ends.
fn splice(
    mut socket: tungstenite::WebSocket<TcpStream>,
    upstream: Upstream,
    superseded: &AtomicBool,
    stopped: &AtomicBool,
) {
    let Upstream {
        mut reader,
        mut writer,
        keep,
    } = upstream;
    let _ = socket.get_ref().set_read_timeout(Some(POLL));
    let (send, receive) = sync_channel::<Vec<u8>>(BUFFERED_CHUNKS);
    let reading = thread::spawn(move || {
        let mut buffer = vec![0u8; CHUNK];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(count) => {
                    if send.send(buffer[..count].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    let over = || superseded.load(Ordering::Acquire) || stopped.load(Ordering::Acquire);
    let mut upstream_open = true;
    'run: while !over() {
        match socket.read() {
            Ok(Message::Binary(bytes)) => {
                if writer
                    .write_all(&bytes)
                    .and_then(|()| writer.flush())
                    .is_err()
                {
                    break;
                }
            }
            Ok(Message::Close(_)) => break,
            Ok(_) => {}
            Err(tungstenite::Error::Io(error))
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => break,
        }
        // Everything the upstream has produced since the last pass.
        loop {
            match receive.recv_timeout(Duration::ZERO) {
                Ok(chunk) => {
                    if socket.send(Message::Binary(chunk.into())).is_err() {
                        break 'run;
                    }
                }
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => {
                    upstream_open = false;
                    break 'run;
                }
            }
        }
    }
    if !upstream_open {
        close(
            &mut socket,
            1000,
            "The computer closed the screen connection.",
        );
    } else {
        let _ = socket.close(None);
        let _ = socket.flush();
    }
    let _ = socket.get_ref().shutdown(Shutdown::Both);
    drop(receive);
    // Dropping the keep-alive ends a child process, which releases a reader blocked in `read`.
    drop(writer);
    drop(keep);
    let _ = reading.join();
}

/// The origins a window of this app loads from: the production scheme on each platform, and
/// the dev server in debug builds.
pub(crate) fn app_origins(dev_url: Option<&str>) -> Vec<String> {
    let mut origins = vec![
        "tauri://localhost".to_string(),
        "http://tauri.localhost".to_string(),
        "https://tauri.localhost".to_string(),
    ];
    if cfg!(debug_assertions) {
        if let Some(origin) = dev_url.and_then(origin_of) {
            origins.push(origin);
        }
    }
    origins
}

fn origin_of(url: &str) -> Option<String> {
    let parsed = http::Uri::try_from(url).ok()?;
    Some(format!(
        "{}://{}",
        parsed.scheme_str()?,
        parsed.authority()?
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use tungstenite::client::IntoClientRequest;

    const ORIGIN: &str = "tauri://localhost";

    /// An upstream that echoes what it receives, and counts how many were opened.
    fn echo_open(opened: Arc<std::sync::atomic::AtomicUsize>) -> Open {
        Arc::new(move || {
            opened.fetch_add(1, Ordering::AcqRel);
            let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|e| e.to_string())?;
            let address = listener.local_addr().map_err(|e| e.to_string())?;
            thread::spawn(move || {
                if let Ok((mut socket, _)) = listener.accept() {
                    let mut buffer = [0u8; 1024];
                    while let Ok(count) = socket.read(&mut buffer) {
                        if count == 0 || socket.write_all(&buffer[..count]).is_err() {
                            break;
                        }
                    }
                }
            });
            let socket = TcpStream::connect(address).map_err(|e| e.to_string())?;
            Ok(Upstream {
                reader: Box::new(socket.try_clone().map_err(|e| e.to_string())?),
                writer: Box::new(socket),
                keep: Box::new(()),
            })
        })
    }

    fn refusing_open() -> Open {
        Arc::new(|| Err("The computer is not running.".to_string()))
    }

    fn gateway(open: Open) -> Gateway {
        Gateway::start(open, vec![ORIGIN.to_string()]).unwrap()
    }

    fn port_of(gateway: &Gateway) -> u16 {
        gateway.port
    }

    /// Connects with the given request path, host and origin headers.
    fn connect(
        port: u16,
        path: &str,
        host: Option<&str>,
        origin: Option<&str>,
        protocol: Option<&str>,
    ) -> Result<tungstenite::WebSocket<TcpStream>, tungstenite::Error> {
        let socket = TcpStream::connect(("127.0.0.1", port)).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = format!("ws://127.0.0.1:{port}{path}")
            .into_client_request()
            .unwrap();
        if let Some(host) = host {
            request.headers_mut().insert("Host", host.parse().unwrap());
        }
        if let Some(origin) = origin {
            request
                .headers_mut()
                .insert("Origin", origin.parse().unwrap());
        }
        if let Some(protocol) = protocol {
            request
                .headers_mut()
                .insert("Sec-WebSocket-Protocol", protocol.parse().unwrap());
        }
        tungstenite::client(request, socket)
            .map(|(socket, _)| socket)
            .map_err(|error| match error {
                tungstenite::HandshakeError::Failure(error) => error,
                tungstenite::HandshakeError::Interrupted(_) => tungstenite::Error::ConnectionClosed,
            })
    }

    fn path(gateway: &Gateway) -> String {
        format!("/{}", gateway.token)
    }

    #[test]
    fn bytes_flow_both_ways_as_binary_messages() {
        let opened = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let gateway = gateway(echo_open(opened.clone()));
        let port = port_of(&gateway);
        let host = format!("127.0.0.1:{port}");
        let mut socket = connect(
            port,
            &path(&gateway),
            Some(&host),
            Some(ORIGIN),
            Some("binary"),
        )
        .expect("an allowed client connects");
        socket
            .send(Message::Binary(b"RFB 003.889\n".to_vec().into()))
            .unwrap();
        let mut seen = Vec::new();
        while seen.len() < 12 {
            if let Message::Binary(bytes) = socket.read().unwrap() {
                seen.extend_from_slice(&bytes);
            }
        }
        assert_eq!(seen, b"RFB 003.889\n");
        assert_eq!(opened.load(Ordering::Acquire), 1);
    }

    #[test]
    fn a_wrong_token_host_or_origin_is_refused_before_any_upstream_opens() {
        let opened = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let gateway = gateway(echo_open(opened.clone()));
        let port = port_of(&gateway);
        let host = format!("127.0.0.1:{port}");
        let good = path(&gateway);
        for (label, result) in [
            (
                "token",
                connect(port, "/wrong", Some(&host), Some(ORIGIN), None),
            ),
            (
                "no token",
                connect(port, "/", Some(&host), Some(ORIGIN), None),
            ),
            (
                "query",
                connect(
                    port,
                    &format!("{good}?x=1"),
                    Some(&host),
                    Some(ORIGIN),
                    None,
                ),
            ),
            (
                "host",
                connect(port, &good, Some("localhost:80"), Some(ORIGIN), None),
            ),
            (
                "origin",
                connect(port, &good, Some(&host), Some("https://example.com"), None),
            ),
            (
                "web origin",
                connect(port, &good, Some(&host), Some("http://127.0.0.1"), None),
            ),
            ("no origin", connect(port, &good, Some(&host), None, None)),
            (
                "protocol",
                connect(port, &good, Some(&host), Some(ORIGIN), Some("chat")),
            ),
        ] {
            assert!(result.is_err(), "{label} must be refused");
        }
        assert_eq!(opened.load(Ordering::Acquire), 0);
    }

    #[test]
    fn an_upstream_that_cannot_open_closes_the_socket_with_its_reason() {
        let gateway = gateway(refusing_open());
        let port = port_of(&gateway);
        let host = format!("127.0.0.1:{port}");
        let mut socket = connect(port, &path(&gateway), Some(&host), Some(ORIGIN), None).unwrap();
        loop {
            match socket.read() {
                Ok(Message::Close(Some(frame))) => {
                    assert_eq!(frame.reason.as_str(), "The computer is not running.");
                    break;
                }
                Ok(Message::Close(None)) | Err(_) => panic!("expected a close with a reason"),
                Ok(_) => {}
            }
        }
    }

    #[test]
    fn a_newer_connection_ends_the_older_one() {
        let opened = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let gateway = gateway(echo_open(opened));
        let port = port_of(&gateway);
        let host = format!("127.0.0.1:{port}");
        let mut first = connect(port, &path(&gateway), Some(&host), Some(ORIGIN), None).unwrap();
        first.send(Message::Binary(vec![1].into())).unwrap();
        assert!(matches!(first.read().unwrap(), Message::Binary(_)));
        let mut second = connect(port, &path(&gateway), Some(&host), Some(ORIGIN), None).unwrap();
        second.send(Message::Binary(vec![2].into())).unwrap();
        assert!(matches!(second.read().unwrap(), Message::Binary(_)));
        let ended = loop {
            match first.read() {
                Ok(Message::Close(_)) | Err(_) => break true,
                Ok(_) => {}
            }
        };
        assert!(ended);
    }

    #[test]
    fn dropping_the_gateway_stops_it_accepting() {
        let gateway = gateway(refusing_open());
        let port = port_of(&gateway);
        assert!(gateway.running());
        drop(gateway);
        thread::sleep(Duration::from_millis(200));
        let result = std::net::TcpStream::connect(("127.0.0.1", port));
        // The listener thread may have closed its socket already; if not, it never serves.
        if let Ok(socket) = result {
            let _ = socket.set_read_timeout(Some(Duration::from_millis(300)));
            let mut byte = [0u8; 1];
            let mut socket = socket;
            assert!(!matches!(socket.read(&mut byte), Ok(count) if count > 0));
        }
    }

    #[test]
    fn the_urls_carry_only_a_token_and_are_loopback() {
        let gateway = gateway(refusing_open());
        let url = gateway.url();
        assert!(url.starts_with("ws://127.0.0.1:"));
        assert_eq!(gateway.token.len(), 32);
        assert!(url.ends_with(&gateway.token));
    }

    #[test]
    fn app_origins_include_the_app_schemes_and_in_debug_the_dev_server() {
        let origins = app_origins(Some("http://localhost:1420"));
        assert!(origins.contains(&"tauri://localhost".to_string()));
        assert!(origins.contains(&"http://tauri.localhost".to_string()));
        assert_eq!(
            origins.contains(&"http://localhost:1420".to_string()),
            cfg!(debug_assertions)
        );
        assert!(!app_origins(None).iter().any(|o| o.contains("1420")));
    }
}
