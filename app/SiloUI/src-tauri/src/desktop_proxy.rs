//! Private loopback gateway: native cookie authentication, guest Basic auth,
//! bounded HTTP headers and transparent upgraded WebSocket streams. The guest
//! side is the SSH tunnel's Unix socket in a private directory (G-04), so no
//! other local process can reach the guest through it or impersonate it.
use crate::desktop_bridge::{self, Inbox};
use base64::{engine::general_purpose::STANDARD, Engine};
use std::{
    io::{Read, Write},
    net::{Shutdown, TcpListener, TcpStream},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

pub(crate) struct Proxy {
    pub port: u16,
    pub cookie_name: String,
    pub token: String,
    /// Pending bridge nonces; the reserved route accepts answers only for these.
    pub(crate) inbox: Arc<Inbox>,
    stopped: Arc<AtomicBool>,
}
impl Drop for Proxy {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
    }
}

/// A byte stream the relay can bound with timeouts and half-close.
trait Stream: Read + Write + Send + 'static {
    fn read_timeout(&self, timeout: Option<Duration>) -> std::io::Result<()>;
    fn write_timeout(&self, timeout: Option<Duration>) -> std::io::Result<()>;
    fn shutdown_write(&self);
}
impl Stream for TcpStream {
    fn read_timeout(&self, timeout: Option<Duration>) -> std::io::Result<()> {
        self.set_read_timeout(timeout)
    }
    fn write_timeout(&self, timeout: Option<Duration>) -> std::io::Result<()> {
        self.set_write_timeout(timeout)
    }
    fn shutdown_write(&self) {
        let _ = self.shutdown(Shutdown::Write);
    }
}
impl Stream for UnixStream {
    fn read_timeout(&self, timeout: Option<Duration>) -> std::io::Result<()> {
        self.set_read_timeout(timeout)
    }
    fn write_timeout(&self, timeout: Option<Duration>) -> std::io::Result<()> {
        self.set_write_timeout(timeout)
    }
    fn shutdown_write(&self) {
        let _ = self.shutdown(Shutdown::Write);
    }
}

struct Request {
    header: String,
    method: String,
    target: String,
    body_length: u64,
    /// Whether the request carried a Content-Length header at all.
    length_declared: bool,
    websocket: bool,
}

fn request_header(
    header: &str,
    port: u16,
    cookie_name: &str,
    token: &str,
    guest_port: u16,
    authorization: &str,
) -> Result<Request, ()> {
    let mut lines = header.split("\r\n");
    let first = lines.next().ok_or(())?;
    let parts: Vec<_> = first.split(' ').collect();
    if first.bytes().any(|byte| byte.is_ascii_control())
        || parts.len() != 3
        || !matches!(parts[0], "GET" | "POST" | "HEAD")
        || !parts[1].starts_with('/')
        || parts[1].starts_with("//")
        || parts[2] != "HTTP/1.1"
    {
        return Err(());
    }
    let mut host = None;
    let mut authenticated = false;
    let mut websocket = false;
    let mut body_length = None;
    let mut kept = Vec::new();
    let expected_host = format!("127.0.0.1:{port}");
    for line in lines.filter(|line| !line.is_empty()) {
        let (name, value) = line.split_once(':').ok_or(())?;
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err(());
        }
        if value
            .bytes()
            .any(|byte| byte.is_ascii_control() && byte != b'\t')
        {
            return Err(());
        }
        let value = value.trim_matches([' ', '\t']);
        match name.to_ascii_lowercase().as_str() {
            "host" => {
                if host.replace(value).is_some() {
                    return Err(());
                }
            }
            "cookie" => {
                authenticated |= value
                    .split(';')
                    .any(|part| part.trim_matches([' ', '\t']) == format!("{cookie_name}={token}"));
            }
            "origin" => {
                if value != format!("http://{expected_host}") {
                    return Err(());
                }
            }
            "authorization" | "proxy-authorization" | "connection" => {}
            "transfer-encoding" | "expect" => return Err(()),
            "content-length" => {
                if body_length.is_some()
                    || value.is_empty()
                    || !value.bytes().all(|b| b.is_ascii_digit())
                {
                    return Err(());
                }
                let length = value.parse::<u64>().map_err(|_| ())?;
                if length > 64 * 1024 * 1024 {
                    return Err(());
                }
                body_length = Some(length);
                kept.push(line);
            }
            "upgrade" => {
                if websocket || !value.eq_ignore_ascii_case("websocket") {
                    return Err(());
                }
                websocket = true;
                kept.push(line);
            }
            _ => kept.push(line),
        }
    }
    if host != Some(expected_host.as_str()) || !authenticated {
        return Err(());
    }
    if websocket && body_length.is_some_and(|n| n != 0) {
        return Err(());
    }
    let mut forwarded = format!("{first}\r\nHost: 127.0.0.1:{guest_port}\r\nOrigin: http://127.0.0.1:{guest_port}\r\nAuthorization: Basic {authorization}\r\nConnection: {}\r\n", if websocket { "Upgrade" } else { "close" });
    for line in kept {
        forwarded.push_str(line);
        forwarded.push_str("\r\n");
    }
    forwarded.push_str("\r\n");
    Ok(Request {
        body_length: body_length.unwrap_or(0),
        length_declared: body_length.is_some(),
        method: parts[0].to_string(),
        target: parts[1].to_string(),
        websocket,
        header: forwarded,
    })
}
fn forward_body(
    mut from: impl Stream,
    mut to: impl Stream,
    mut remaining: u64,
    stop: Arc<AtomicBool>,
    ended: Arc<AtomicBool>,
    deadline: Option<Instant>,
) {
    let _ = from.read_timeout(Some(Duration::from_millis(250)));
    let _ = to.write_timeout(Some(Duration::from_secs(5)));
    let mut bytes = [0; 32 * 1024];
    while remaining > 0
        && deadline.is_none_or(|at| Instant::now() < at)
        && !stop.load(Ordering::Acquire)
        && !ended.load(Ordering::Acquire)
    {
        let count = remaining.min(bytes.len() as u64) as usize;
        match from.read(&mut bytes[..count]) {
            Ok(0) => break,
            Ok(n) => {
                if to.write_all(&bytes[..n]).is_err() {
                    break;
                }
                remaining -= n as u64;
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::Interrupted
                ) =>
            {
                continue
            }
            Err(_) => break,
        }
    }
    // Content-Length already delimits a complete request. A FIN here can make
    // some servers close the connection before returning the HTTP response.
    if remaining > 0 {
        to.shutdown_write();
    }
}
fn relay(
    mut from: impl Stream,
    mut to: impl Stream,
    stop: Arc<AtomicBool>,
    ended: Arc<AtomicBool>,
    deadline: Option<Instant>,
) {
    let poll_interval = Duration::from_millis(250);
    let _ = from.read_timeout(Some(deadline.map_or(poll_interval, |at| {
        at.saturating_duration_since(Instant::now())
            .max(Duration::from_millis(1))
            .min(poll_interval)
    })));
    let _ = to.write_timeout(Some(Duration::from_secs(5)));
    let mut bytes = [0; 32 * 1024];
    while !stop.load(Ordering::Acquire)
        && !ended.load(Ordering::Acquire)
        && deadline.is_none_or(|at| Instant::now() < at)
    {
        match from.read(&mut bytes) {
            Ok(0) => break,
            Ok(n) => {
                if to.write_all(&bytes[..n]).is_err() {
                    break;
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::Interrupted
                ) =>
            {
                continue
            }
            Err(_) => break,
        }
    }
    to.shutdown_write();
}
#[allow(clippy::too_many_arguments)]
fn serve(
    client: TcpStream,
    port: u16,
    upstream: &Path,
    guest_port: u16,
    cookie_name: &str,
    token: &str,
    authorization: &str,
    stop: Arc<AtomicBool>,
    inbox: &Inbox,
) -> std::io::Result<()> {
    serve_inner(
        client,
        port,
        upstream,
        guest_port,
        cookie_name,
        token,
        authorization,
        stop,
        inbox,
        Duration::from_secs(120),
        |_, _| {},
    )
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn serve_with_header_progress(
    client: TcpStream,
    port: u16,
    upstream: &Path,
    guest_port: u16,
    cookie_name: &str,
    token: &str,
    authorization: &str,
    stop: Arc<AtomicBool>,
    http_timeout: Duration,
    header_progress: impl FnMut(&TcpStream, usize),
) -> std::io::Result<()> {
    serve_inner(
        client,
        port,
        upstream,
        guest_port,
        cookie_name,
        token,
        authorization,
        stop,
        &Inbox::default(),
        http_timeout,
        header_progress,
    )
}

#[allow(clippy::too_many_arguments)]
fn serve_inner(
    mut client: TcpStream,
    port: u16,
    upstream: &Path,
    guest_port: u16,
    cookie_name: &str,
    token: &str,
    authorization: &str,
    stop: Arc<AtomicBool>,
    inbox: &Inbox,
    http_timeout: Duration,
    mut header_progress: impl FnMut(&TcpStream, usize),
) -> std::io::Result<()> {
    // On macOS, accepted sockets inherit the listener's O_NONBLOCK setting.
    // This handler uses timed blocking reads, so clear that flag explicitly.
    client.set_nonblocking(false)?;
    client.set_read_timeout(Some(Duration::from_secs(3)))?;
    client.set_write_timeout(Some(Duration::from_secs(3)))?;
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut bytes = Vec::new();
    let mut byte = [0];
    while !bytes.ends_with(b"\r\n\r\n") {
        if bytes.len() >= 16 * 1024 || Instant::now() >= deadline || stop.load(Ordering::Acquire) {
            return Ok(());
        }
        header_progress(&client, bytes.len());
        match client.read(&mut byte) {
            Ok(0) => return Ok(()),
            Ok(_) => bytes.push(byte[0]),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    let header = std::str::from_utf8(&bytes).ok().and_then(|text| {
        request_header(text, port, cookie_name, token, guest_port, authorization).ok()
    });
    let Some(header) = header else {
        client.write_all(
            b"HTTP/1.1 403 Forbidden\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
        )?;
        return Ok(());
    };
    if desktop_bridge::is_reserved(&header.target) {
        return serve_reserved(&mut client, &header, inbox, stop);
    }
    let deadline = (!header.websocket).then(|| Instant::now() + http_timeout);
    let mut server = match UnixStream::connect(upstream) {
        Ok(server) => server,
        Err(_) => {
            client.write_all(
                b"HTTP/1.1 502 Bad Gateway\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
            )?;
            return Ok(());
        }
    };
    server.set_write_timeout(Some(Duration::from_secs(3)))?;
    server.write_all(header.header.as_bytes())?;
    let incoming = client.try_clone()?;
    let outgoing = server.try_clone()?;
    let ended = Arc::new(AtomicBool::new(false));
    let peer_end = ended.clone();
    let peer_stop = stop.clone();
    let writer = thread::spawn(move || {
        if header.websocket {
            relay(incoming, outgoing, peer_stop, peer_end.clone(), deadline);
            peer_end.store(true, Ordering::Release);
        } else {
            forward_body(
                incoming,
                outgoing,
                header.body_length,
                peer_stop,
                peer_end,
                deadline,
            );
        }
    });
    relay(server, client, stop, ended.clone(), deadline);
    ended.store(true, Ordering::Release);
    let _ = writer.join();
    Ok(())
}
/// Answers the bridge's reserved route itself; the guest never sees it.
fn serve_reserved(
    client: &mut TcpStream,
    request: &Request,
    inbox: &Inbox,
    stop: Arc<AtomicBool>,
) -> std::io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = desktop_bridge::serve_route(
        inbox,
        &request.method,
        &request.target,
        (!request.websocket && request.length_declared).then_some(request.body_length),
        |length| {
            let mut body = vec![0; length];
            let mut filled = 0;
            while filled < length {
                if Instant::now() >= deadline || stop.load(Ordering::Acquire) {
                    return Err(std::io::ErrorKind::TimedOut.into());
                }
                match client.read(&mut body[filled..]) {
                    Ok(0) => return Err(std::io::ErrorKind::UnexpectedEof.into()),
                    Ok(n) => filled += n,
                    Err(e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock
                                | std::io::ErrorKind::TimedOut
                                | std::io::ErrorKind::Interrupted
                        ) => {}
                    Err(e) => return Err(e),
                }
            }
            Ok(body)
        },
        Instant::now(),
    );
    client.write_all(
        format!(
            "HTTP/1.1 {}\r\nConnection: close\r\nCache-Control: no-store\r\nContent-Length: 0\r\n\r\n",
            status.line()
        )
        .as_bytes(),
    )?;
    // Closing with request bytes unread would reset the connection and can
    // discard the response; read what the client still sends, within bounds.
    let _ = client.shutdown(Shutdown::Write);
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut sink = [0; 8192];
    let mut drained = 0;
    while drained < 1024 * 1024 && Instant::now() < deadline && !stop.load(Ordering::Acquire) {
        match client.read(&mut sink) {
            Ok(0) | Err(_) => break,
            Ok(n) => drained += n,
        }
    }
    Ok(())
}

impl Proxy {
    pub(crate) fn running(&self) -> bool {
        !self.stopped.load(Ordering::Acquire)
    }

    /// `upstream` is the tunnel's Unix socket; `guest_port` is the guest's own
    /// listener, named in the Host and Origin headers the guest receives.
    pub fn start(
        upstream: PathBuf,
        guest_port: u16,
        username: &str,
        password: &str,
    ) -> Result<Self, String> {
        Self::start_with_accept(
            upstream,
            guest_port,
            username,
            password,
            TcpListener::accept,
        )
    }

    pub(crate) fn start_with_accept(
        upstream: PathBuf,
        guest_port: u16,
        username: &str,
        password: &str,
        mut accept: impl FnMut(&TcpListener) -> std::io::Result<(TcpStream, std::net::SocketAddr)>
            + Send
            + 'static,
    ) -> Result<Self, String> {
        if guest_port == 0
            || username.contains(':')
            || username.contains(['\r', '\n'])
            || password.contains(['\r', '\n'])
        {
            return Err("Invalid desktop connection.".into());
        }
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .map_err(|_| "Could not open desktop connection.")?;
        listener
            .set_nonblocking(true)
            .map_err(|_| "Could not configure desktop connection.")?;
        let port = listener
            .local_addr()
            .map_err(|_| "Could not read desktop connection.")?
            .port();
        let token = uuid::Uuid::new_v4().simple().to_string();
        let cookie_name = format!("silo_desktop_{port}");
        let authorization = STANDARD.encode(format!("{username}:{password}"));
        let stopped = Arc::new(AtomicBool::new(false));
        let worker_stop = stopped.clone();
        let worker_token = token.clone();
        let worker_cookie = cookie_name.clone();
        let active = Arc::new(AtomicUsize::new(0));
        let inbox = Arc::new(Inbox::default());
        let worker_inbox = inbox.clone();
        thread::spawn(move || {
            while !worker_stop.load(Ordering::Acquire) {
                match accept(&listener) {
                    Ok((socket, _)) => {
                        if active.load(Ordering::Acquire) >= 48 {
                            drop(socket);
                            continue;
                        }
                        active.fetch_add(1, Ordering::AcqRel);
                        let (stop, token, cookie, auth, count, upstream, inbox) = (
                            worker_stop.clone(),
                            worker_token.clone(),
                            worker_cookie.clone(),
                            authorization.clone(),
                            active.clone(),
                            upstream.clone(),
                            worker_inbox.clone(),
                        );
                        thread::spawn(move || {
                            let _ = serve(
                                socket, port, &upstream, guest_port, &cookie, &token, &auth, stop,
                                &inbox,
                            );
                            count.fetch_sub(1, Ordering::AcqRel);
                        });
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(30))
                    }
                    Err(e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::Interrupted
                                | std::io::ErrorKind::ConnectionAborted
                                | std::io::ErrorKind::ConnectionReset
                        ) =>
                    {
                        continue
                    }
                    Err(_) => break,
                }
            }
            worker_stop.store(true, Ordering::Release);
        });
        Ok(Self {
            port,
            cookie_name,
            token,
            inbox,
            stopped,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    /// The guest end of the tunnel: a Unix socket in a private directory.
    fn guest() -> (tempfile::TempDir, UnixListener, PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("desktop.sock");
        let listener = UnixListener::bind(&path).unwrap();
        (directory, listener, path)
    }

    #[test]
    fn transient_accept_errors_do_not_retire_the_listener() {
        for kind in [
            std::io::ErrorKind::ConnectionAborted,
            std::io::ErrorKind::ConnectionReset,
            std::io::ErrorKind::Interrupted,
        ] {
            let (_directory, _upstream, socket) = guest();
            let (retried_tx, retried_rx) = std::sync::mpsc::channel();
            let mut failed = false;
            let mut reported_retry = false;
            let proxy =
                Proxy::start_with_accept(socket, 6901, "silo", "password", move |listener| {
                    if !failed {
                        failed = true;
                        return Err(kind.into());
                    }
                    if !reported_retry {
                        reported_retry = true;
                        retried_tx.send(()).unwrap();
                    }
                    listener.accept()
                })
                .unwrap();
            let retried = retried_rx.recv_timeout(Duration::from_secs(3));
            drop(proxy);
            assert!(
                retried.is_ok(),
                "accept error retired the listener: {kind:?}"
            );
        }
    }

    struct InterruptedOnce {
        stream: UnixStream,
        interrupted: bool,
    }

    impl Read for InterruptedOnce {
        fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
            if !self.interrupted {
                self.interrupted = true;
                return Err(std::io::ErrorKind::Interrupted.into());
            }
            self.stream.read(bytes)
        }
    }

    impl Write for InterruptedOnce {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.stream.write(bytes)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.stream.flush()
        }
    }

    impl Stream for InterruptedOnce {
        fn read_timeout(&self, timeout: Option<Duration>) -> std::io::Result<()> {
            self.stream.set_read_timeout(timeout)
        }

        fn write_timeout(&self, timeout: Option<Duration>) -> std::io::Result<()> {
            self.stream.set_write_timeout(timeout)
        }

        fn shutdown_write(&self) {
            let _ = self.stream.shutdown(Shutdown::Write);
        }
    }

    fn assert_interrupted_read_is_retried(response: bool) {
        let (incoming, mut sender) = UnixStream::pair().unwrap();
        let (outgoing, mut receiver) = UnixStream::pair().unwrap();
        receiver
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        sender.write_all(b"payload").unwrap();
        sender.shutdown(Shutdown::Write).unwrap();
        let incoming = InterruptedOnce {
            stream: incoming,
            interrupted: false,
        };
        let stop = Arc::new(AtomicBool::new(false));
        let ended = Arc::new(AtomicBool::new(false));
        if response {
            relay(incoming, outgoing, stop, ended, None);
        } else {
            forward_body(incoming, outgoing, 7, stop, ended, None);
        }
        let mut received = Vec::new();
        receiver.read_to_end(&mut received).unwrap();
        assert_eq!(received, b"payload", "response stream: {response}");
    }

    #[test]
    fn interrupted_body_reads_do_not_truncate_requests() {
        assert_interrupted_read_is_retried(false);
    }

    #[test]
    fn interrupted_response_reads_do_not_truncate_streams() {
        assert_interrupted_read_is_retried(true);
    }

    #[test]
    fn only_authenticated_same_origin_requests_reach_guest() {
        let valid = "GET /websockify HTTP/1.1\r\nHost: 127.0.0.1:8000\r\nCookie: session=secret\r\nOrigin: http://127.0.0.1:8000\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nAuthorization: Basic attacker\r\n\r\n";
        let result = request_header(valid, 8000, "session", "secret", 9000, "real")
            .unwrap()
            .header;
        assert!(result.contains("Authorization: Basic real"));
        assert!(!result.contains("attacker"));
        assert!(!result.contains("secret"));
        assert!(result.contains("Connection: Upgrade"));
        for bad in [
            valid.replace("session=secret", "session=wrong"),
            valid.replace(
                "Origin: http://127.0.0.1:8000",
                "Origin: https://evil.example",
            ),
            valid.replace("Host: 127.0.0.1:8000", "Host: evil.example"),
            valid.replace("GET /websockify", "GET http://evil.example/"),
        ] {
            assert!(request_header(&bad, 8000, "session", "secret", 9000, "real").is_err());
        }
    }
    #[test]
    fn control_characters_cannot_bypass_header_sanitization() {
        let base = "GET / HTTP/1.1\r\nHost: 127.0.0.1:8000\r\nCookie: session=secret\r\n";
        for field in [
            "X-Note: harmless\nAuthorization: Basic attacker",
            "X-Note: harmless\rCookie: leaked=secret",
            "X-Note: harmless\0suffix",
            "X-Note: harmless\u{000b}suffix",
            "Content-Length: 0\n",
            "Upgrade: websocket\r",
        ] {
            assert!(
                request_header(
                    &format!("{base}{field}\r\n\r\n"),
                    8000,
                    "session",
                    "secret",
                    9000,
                    "real",
                )
                .is_err(),
                "accepted malformed field: {field:?}",
            );
        }
        for target in [
            "/path\nX:injected",
            "/path\rX:injected",
            "/path\0",
            "/path\t",
        ] {
            assert!(request_header(
                &format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1:8000\r\nCookie: session=secret\r\n\r\n"),
                8000,
                "session",
                "secret",
                9000,
                "real",
            )
            .is_err());
        }
        assert!(request_header(
            &format!("{base}X-Note: two\twords\r\n\r\n"),
            8000,
            "session",
            "secret",
            9000,
            "real",
        )
        .is_ok());
    }

    #[test]
    fn unicode_whitespace_cannot_disguise_typed_header_values() {
        let base = "POST / HTTP/1.1\r\nHost: 127.0.0.1:8000\r\nCookie: session=secret\r\n";
        for field in [
            "Content-Length: \u{00a0}1",
            "Content-Length: 1\u{2003}",
            "Upgrade: websocket\u{2003}",
        ] {
            assert!(
                request_header(
                    &format!("{base}{field}\r\n\r\n"),
                    8000,
                    "session",
                    "secret",
                    9000,
                    "real",
                )
                .is_err(),
                "accepted non-HTTP whitespace: {field:?}"
            );
        }
        for field in [
            "Host: 127.0.0.1:8000\u{00a0}\r\nCookie: session=secret",
            "Host: 127.0.0.1:8000\r\nCookie: \u{2003}session=secret",
            "Host: 127.0.0.1:8000\r\nCookie: session=secret\r\nOrigin: http://127.0.0.1:8000\u{3000}",
        ] {
            assert!(request_header(
                &format!("GET / HTTP/1.1\r\n{field}\r\n\r\n"),
                8000, "session", "secret", 9000, "real",
            ).is_err());
        }
        let valid = request_header(
            &format!("{base}Content-Length: \t1\t \r\n\r\n"),
            8000,
            "session",
            "secret",
            9000,
            "real",
        )
        .unwrap();
        assert_eq!(valid.body_length, 1);
        assert!(valid.header.contains("Content-Length: \t1\t \r\n"));
    }

    #[test]
    fn forwards_authenticated_http_and_rejects_missing_cookie() {
        let (_directory, upstream, socket) = guest();
        let proxy = Proxy::start(socket, 6901, "silo", "password").unwrap();
        let worker = thread::spawn(move || {
            let (mut stream, _) = upstream.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut data = Vec::new();
            let mut byte = [0];
            while !data.ends_with(b"\r\n\r\n") {
                stream.read_exact(&mut byte).unwrap();
                data.push(byte[0]);
            }
            let data = String::from_utf8(data).unwrap();
            assert!(data.contains("Authorization: Basic c2lsbzpwYXNzd29yZA=="));
            // The guest sees its own listener, not a host port.
            assert!(data.contains("Host: 127.0.0.1:6901\r\n"));
            assert!(data.contains("Origin: http://127.0.0.1:6901\r\n"));
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .unwrap();
        });
        for authorized in [false, true] {
            let mut socket = TcpStream::connect(("127.0.0.1", proxy.port)).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            write!(
                socket,
                "GET / HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n{}\r\n",
                proxy.port,
                if authorized {
                    format!("Cookie: {}={}\r\n", proxy.cookie_name, proxy.token)
                } else {
                    String::new()
                }
            )
            .unwrap();
            let mut response = String::new();
            socket.read_to_string(&mut response).unwrap();
            assert!(response.starts_with(if authorized {
                "HTTP/1.1 200"
            } else {
                "HTTP/1.1 403"
            }));
        }
        worker.join().unwrap();
    }

    #[test]
    fn stalled_http_response_closes_both_connections_at_deadline() {
        stalled_http_request("GET", "");
    }

    #[test]
    fn a_closed_websocket_client_releases_its_handler_without_guest_eof() {
        let (_directory, upstream, socket) = guest();
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let (accepted, _) = listener.accept().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let worker = thread::spawn(move || {
            let result = serve(
                accepted,
                port,
                &socket,
                6901,
                "session",
                "secret",
                "auth",
                worker_stop,
                &Inbox::default(),
            );
            done_tx.send(result).unwrap();
        });
        write!(client, "GET /websockify HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nCookie: session=secret\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n").unwrap();
        let (mut guest, _) = upstream.accept().unwrap();
        guest
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut request = Vec::new();
        let mut byte = [0];
        while !request.ends_with(b"\r\n\r\n") {
            guest.read_exact(&mut byte).unwrap();
            request.push(byte[0]);
        }
        guest.write_all(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n").unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut response = Vec::new();
        while !response.ends_with(b"\r\n\r\n") {
            client.read_exact(&mut byte).unwrap();
            response.push(byte[0]);
        }
        assert!(response.starts_with(b"HTTP/1.1 101"));
        // Leave the guest's response side open after the client disappears.
        drop(client);
        let completed = done_rx.recv_timeout(Duration::from_secs(1));
        stop.store(true, Ordering::Release);
        worker.join().unwrap();
        assert!(
            completed.is_ok(),
            "closed WebSocket client retained its handler"
        );
        assert!(completed.unwrap().is_ok());
        assert_eq!(guest.read(&mut byte).unwrap(), 0);
    }

    #[test]
    fn continuing_response_bytes_do_not_extend_http_deadline() {
        let (mut guest, source) = UnixStream::pair().unwrap();
        let (destination, mut client) = UnixStream::pair().unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let worker = thread::spawn(move || {
            relay(
                source,
                destination,
                worker_stop,
                Arc::new(AtomicBool::new(false)),
                Some(Instant::now() + Duration::from_millis(50)),
            );
            done_tx.send(()).unwrap();
        });
        let producer = thread::spawn(move || {
            for _ in 0..100 {
                if guest.write_all(b"x").is_err() {
                    break;
                }
                thread::sleep(Duration::from_millis(5));
            }
        });
        let completed = done_rx.recv_timeout(Duration::from_millis(300));
        stop.store(true, Ordering::Release);
        worker.join().unwrap();
        producer.join().unwrap();
        assert!(
            completed.is_ok(),
            "response progress extended the total deadline"
        );
        let mut received = Vec::new();
        client.read_to_end(&mut received).unwrap();
        assert!(!received.is_empty());
    }

    #[test]
    fn incomplete_http_upload_closes_both_connections_at_deadline() {
        stalled_http_request("POST", "Content-Length: 5\r\n");
    }

    fn stalled_http_request(method: &str, headers: &str) {
        let (_directory, upstream, socket) = guest();
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let (accepted, _) = listener.accept().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let worker = thread::spawn(move || {
            let result = serve_with_header_progress(
                accepted,
                port,
                &socket,
                6901,
                "session",
                "secret",
                "auth",
                worker_stop,
                Duration::from_millis(50),
                |_, _| {},
            );
            done_tx.send(result).unwrap();
        });
        write!(
            client,
            "{method} / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nCookie: session=secret\r\n{headers}\r\n"
        )
        .unwrap();
        let (mut guest, _) = upstream.accept().unwrap();
        guest
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut request = Vec::new();
        let mut byte = [0];
        while !request.ends_with(b"\r\n\r\n") {
            guest.read_exact(&mut byte).unwrap();
            request.push(byte[0]);
        }
        // Keep the guest connection open without sending a response.
        let completed = done_rx.recv_timeout(Duration::from_secs(1));
        stop.store(true, Ordering::Release);
        worker.join().unwrap();
        assert!(
            completed.is_ok(),
            "stalled HTTP request exceeded its deadline"
        );
        assert!(completed.unwrap().is_ok());
        assert_eq!(client.read(&mut byte).unwrap(), 0);
        guest.read_to_end(&mut Vec::new()).unwrap();
    }

    #[test]
    fn accepted_nonblocking_client_waits_for_fragmented_request_headers() {
        use std::{os::fd::AsRawFd, sync::mpsc};

        let (_directory, upstream, socket) = guest();
        let upstream_worker = thread::spawn(move || {
            let (mut stream, _) = upstream.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut request = Vec::new();
            let mut byte = [0];
            while !request.ends_with(b"\r\n\r\n") {
                stream.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
            }
            assert!(String::from_utf8(request)
                .unwrap()
                .contains("Authorization: Basic c2lsbzpwYXNzd29yZA=="));
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .unwrap();
        });

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let (accepted, _) = listener.accept().unwrap();
        // Force the inherited state on every platform, even where accept clears it.
        accepted.set_nonblocking(true).unwrap();
        let fragment = b"GET / HTTP/1.1\r\nHost: 127.0.0.1:";
        let (progress_tx, progress_rx) = mpsc::sync_channel(0);
        let proxy_worker = thread::spawn(move || {
            serve_with_header_progress(
                accepted,
                port,
                &socket,
                6901,
                "session",
                "secret",
                "c2lsbzpwYXNzd29yZA==",
                Arc::new(AtomicBool::new(false)),
                Duration::from_secs(120),
                |stream, consumed| {
                    if consumed == 0 {
                        // Inspect the descriptor before sending any request bytes.
                        // This fails deterministically if serve stops clearing O_NONBLOCK.
                        let flags = unsafe { libc::fcntl(stream.as_raw_fd(), libc::F_GETFL) };
                        assert_ne!(flags, -1);
                        assert_eq!(flags & libc::O_NONBLOCK, 0);
                        progress_tx.send(consumed).unwrap();
                    } else if consumed == fragment.len() {
                        progress_tx.send(consumed).unwrap();
                    }
                },
            )
            .unwrap();
        });

        client
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        assert_eq!(progress_rx.recv_timeout(Duration::from_secs(3)).unwrap(), 0);
        client.write_all(fragment).unwrap();
        // The proxy has consumed the entire incomplete fragment before we
        // release the remainder, so scheduling cannot collapse the two reads.
        assert_eq!(
            progress_rx.recv_timeout(Duration::from_secs(3)).unwrap(),
            fragment.len()
        );
        write!(client, "{port}\r\nCookie: session=secret\r\n\r\n").unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 200 OK"));
        assert!(response.ends_with("ok"));
        proxy_worker.join().unwrap();
        upstream_worker.join().unwrap();
    }

    #[test]
    fn completed_http_requests_keep_write_side_open_for_response() {
        let (_directory, upstream, socket) = guest();
        upstream.set_nonblocking(true).unwrap();
        let upstream_worker = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            for _ in 0..3 {
                let (mut stream, _) = loop {
                    match upstream.accept() {
                        Ok(connection) => break connection,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(
                                Instant::now() < deadline,
                                "proxy did not connect to stub upstream"
                            );
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => panic!("upstream accept failed: {error}"),
                    }
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = Vec::new();
                let mut byte = [0];
                while !request.ends_with(b"\r\n\r\n") {
                    stream.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                }
                let header = String::from_utf8(request).unwrap();
                let body_length = header
                    .lines()
                    .find_map(|line| line.strip_prefix("Content-Length: "))
                    .unwrap_or("0")
                    .parse::<usize>()
                    .unwrap();
                let mut body = vec![0; body_length];
                stream.read_exact(&mut body).unwrap();

                // Model the observed server behavior: a write-half-close before
                // the response causes an early close; otherwise it responds.
                stream
                    .set_read_timeout(Some(Duration::from_millis(100)))
                    .unwrap();
                match stream.read(&mut byte) {
                    Ok(0) => continue,
                    Ok(_) => panic!("unexpected bytes after the declared request body"),
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) => {}
                    Err(error) => panic!("upstream read failed: {error}"),
                }
                stream
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                    )
                    .unwrap();
            }
        });

        let proxy = Proxy::start(socket, 6901, "silo", "password").unwrap();
        for (method, body, content_length) in [
            ("GET", &b""[..], false),
            ("GET", &b""[..], true),
            ("POST", &b"body"[..], true),
        ] {
            let mut client = TcpStream::connect(("127.0.0.1", proxy.port)).unwrap();
            client
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            write!(
                client,
                "{method} / HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nCookie: {}={}\r\n{}\r\n",
                proxy.port,
                proxy.cookie_name,
                proxy.token,
                if content_length {
                    format!("Content-Length: {}\r\n", body.len())
                } else {
                    String::new()
                }
            )
            .unwrap();
            client.write_all(body).unwrap();
            client.shutdown(Shutdown::Write).unwrap();
            let mut response = String::new();
            client.read_to_string(&mut response).unwrap();
            assert!(
                response.starts_with("HTTP/1.1 200 OK"),
                "{method} response: {response:?}"
            );
        }
        upstream_worker.join().unwrap();
    }

    #[test]
    fn ordinary_http_response_idle_timeout_releases_silent_upstream() {
        for (method, body) in [("GET", &b""[..]), ("POST", &b"body"[..])] {
            let (_directory, upstream, socket) = guest();
            let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
            let port = listener.local_addr().unwrap().port();
            let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
            let (accepted, _) = listener.accept().unwrap();
            let stop = Arc::new(AtomicBool::new(false));
            let worker_stop = stop.clone();
            let (done_tx, done_rx) = std::sync::mpsc::channel();
            let worker = thread::spawn(move || {
                let result = serve_with_header_progress(
                    accepted,
                    port,
                    &socket,
                    6901,
                    "session",
                    "secret",
                    "real",
                    worker_stop,
                    Duration::from_millis(100),
                    |_, _| {},
                );
                done_tx.send(result).unwrap();
            });
            write!(
                client,
                "{method} / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nCookie: session=secret\r\nContent-Length: {}\r\n\r\n",
                body.len()
            )
            .unwrap();
            client.write_all(body).unwrap();
            let (mut guest, _) = upstream.accept().unwrap();
            guest
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut header = Vec::new();
            let mut byte = [0];
            while !header.ends_with(b"\r\n\r\n") {
                guest.read_exact(&mut byte).unwrap();
                header.push(byte[0]);
            }
            let mut received_body = vec![0; body.len()];
            guest.read_exact(&mut received_body).unwrap();
            assert_eq!(received_body, body);
            // Keep the guest silent and open after the browser abandons its request.
            drop(client);
            let completed = done_rx.recv_timeout(Duration::from_secs(3));
            // Always release the worker, including when the regression fails.
            stop.store(true, Ordering::Release);
            worker.join().unwrap();
            completed
                .expect("silent HTTP response retained its handler")
                .unwrap();
            assert_eq!(guest.read(&mut byte).unwrap(), 0);
        }
    }

    #[test]
    fn ambiguous_or_unbounded_request_bodies_are_rejected() {
        let base = "POST / HTTP/1.1\r\nHost: 127.0.0.1:8000\r\nCookie: session=secret\r\n";
        for fields in [
            "Content-Length: 1\r\nContent-Length: 1",
            "Content-Length: -1",
            "Content-Length: +1",
            "Content-Length: 1x",
            "Content-Length: 67108865",
            "Transfer-Encoding: chunked",
            "Expect: 100-continue",
            "Upgrade: websocket\r\nContent-Length: 1",
        ] {
            assert!(
                request_header(
                    &format!("{base}{fields}\r\n\r\n"),
                    8000,
                    "session",
                    "secret",
                    9000,
                    "real"
                )
                .is_err(),
                "accepted {fields}"
            );
        }
    }
    #[test]
    fn pipelined_unauthenticated_request_never_reaches_guest() {
        let (_directory, upstream, guest_socket) = guest();
        let proxy = Proxy::start(guest_socket, 6901, "silo", "password").unwrap();
        let worker = thread::spawn(move || {
            let (mut stream, _) = upstream.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut byte = [0];
            while !request.ends_with(b"\r\n\r\n") {
                stream.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
            }
            let header = String::from_utf8(request).unwrap();
            let body_length = header
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length: "))
                .unwrap()
                .parse::<usize>()
                .unwrap();
            let mut body = vec![0; body_length];
            stream.read_exact(&mut body).unwrap();
            let request = format!("{header}{}", String::from_utf8(body).unwrap());
            assert!(request.ends_with("\r\n\r\nhello"));
            assert!(!request.contains("/unauthenticated"));
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .unwrap();
        });
        let mut socket = TcpStream::connect(("127.0.0.1", proxy.port)).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        write!(socket,"POST / HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nCookie: {}={}\r\nContent-Length: 5\r\n\r\nhelloGET /unauthenticated HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n\r\n",proxy.port,proxy.cookie_name,proxy.token,proxy.port).unwrap();
        let mut response = [0; 128];
        let count = socket.read(&mut response).unwrap();
        assert!(response[..count].starts_with(b"HTTP/1.1 200"));
        worker.join().unwrap();
    }
    #[test]
    fn websocket_client_disconnect_releases_silent_upstream() {
        let (_directory, upstream, socket) = guest();
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let (accepted, _) = listener.accept().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let worker = thread::spawn(move || {
            let result = serve_with_header_progress(
                accepted,
                port,
                &socket,
                6901,
                "session",
                "secret",
                "real",
                worker_stop,
                Duration::from_millis(50),
                |_, _| {},
            );
            done_tx.send(result).unwrap();
        });
        write!(client, "GET /websockify HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nCookie: session=secret\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n").unwrap();
        let (mut guest, _) = upstream.accept().unwrap();
        guest
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut header = Vec::new();
        let mut byte = [0];
        while !header.ends_with(b"\r\n\r\n") {
            guest.read_exact(&mut byte).unwrap();
            header.push(byte[0]);
        }
        guest.write_all(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n\r\n").unwrap();
        header.clear();
        while !header.ends_with(b"\r\n\r\n") {
            client.read_exact(&mut byte).unwrap();
            header.push(byte[0]);
        }
        // An idle WebSocket must survive beyond the ordinary HTTP deadline.
        assert!(matches!(
            done_rx.recv_timeout(Duration::from_millis(150)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ));
        drop(client);
        let completed = done_rx.recv_timeout(Duration::from_secs(3));
        stop.store(true, Ordering::Release);
        worker.join().unwrap();
        completed
            .expect("disconnected WebSocket retained its handler")
            .unwrap();
        assert_eq!(guest.read(&mut byte).unwrap(), 0);
    }

    #[test]
    fn websocket_streams_bidirectionally_and_closes_when_viewer_drops() {
        let (_directory, upstream, guest_socket) = guest();
        let proxy = Proxy::start(guest_socket, 6901, "silo", "password").unwrap();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let worker = thread::spawn(move || {
            let (mut stream, _) = upstream.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut headers = Vec::new();
            let mut byte = [0];
            while !headers.ends_with(b"\r\n\r\n") {
                stream.read_exact(&mut byte).unwrap();
                headers.push(byte[0]);
            }
            stream.write_all(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n").unwrap();
            let mut ping = [0; 4];
            stream.read_exact(&mut ping).unwrap();
            assert_eq!(&ping, b"ping");
            stream.write_all(b"pong").unwrap();
            assert_eq!(stream.read(&mut byte).unwrap(), 0);
            done_tx.send(()).unwrap();
        });
        let mut socket = TcpStream::connect(("127.0.0.1", proxy.port)).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        write!(socket,"GET /websockify HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nCookie: {}={}\r\nOrigin: http://127.0.0.1:{}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n",proxy.port,proxy.cookie_name,proxy.token,proxy.port).unwrap();
        let mut headers = Vec::new();
        let mut byte = [0];
        while !headers.ends_with(b"\r\n\r\n") {
            socket.read_exact(&mut byte).unwrap();
            headers.push(byte[0]);
        }
        assert!(headers.starts_with(b"HTTP/1.1 101"));
        socket.write_all(b"ping").unwrap();
        let mut pong = [0; 4];
        socket.read_exact(&mut pong).unwrap();
        assert_eq!(&pong, b"pong");
        drop(proxy);
        done_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        worker.join().unwrap();
    }

    /// Sends one raw request to the proxy and returns the status line.
    fn reserved_request(proxy: &Proxy, target: &str, cookie: bool, body: &[u8]) -> String {
        let mut socket = TcpStream::connect(("127.0.0.1", proxy.port)).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        write!(
            socket,
            "POST {target} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n{}Content-Length: {}\r\n\r\n",
            proxy.port,
            if cookie {
                format!("Cookie: {}={}\r\n", proxy.cookie_name, proxy.token)
            } else {
                String::new()
            },
            body.len()
        )
        .unwrap();
        socket.write_all(body).unwrap();
        let mut response = String::new();
        socket.read_to_string(&mut response).unwrap();
        response.lines().next().unwrap_or_default().to_string()
    }

    fn assert_guest_untouched(upstream: &UnixListener) {
        upstream.set_nonblocking(true).unwrap();
        assert_eq!(
            upstream.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock,
            "a reserved request reached the guest"
        );
    }

    #[test]
    fn the_reserved_bridge_route_is_answered_locally_and_never_forwarded() {
        use crate::desktop_bridge::Op;
        let (_directory, upstream, socket) = guest();
        let proxy = Proxy::start(socket, 6901, "silo", "password").unwrap();
        let expectation = proxy.inbox.expect(Op::Clipboard, Duration::from_secs(5));
        let target = format!(
            "/__silo/v1/clipboard?nonce={}&kind=text/plain",
            expectation.nonce
        );
        // Without the viewer's cookie the request is refused before any nonce is looked at.
        assert!(reserved_request(&proxy, &target, false, b"").starts_with("HTTP/1.1 403"));
        // Unknown operations and paths under the reserved prefix never go to the guest either.
        for unknown in [
            "/__silo/v1/files?nonce=a&kind=b",
            "/__silo/v2/clipboard",
            "/__silo",
        ] {
            assert!(
                reserved_request(&proxy, unknown, true, b"").starts_with("HTTP/1.1 404"),
                "{unknown}"
            );
        }
        assert!(reserved_request(&proxy, &target, true, b"hello").starts_with("HTTP/1.1 204"));
        let reply = expectation.wait(Duration::from_secs(1)).unwrap();
        assert_eq!(reply.body, b"hello");
        // The nonce is spent.
        assert!(reserved_request(&proxy, &target, true, b"hello").starts_with("HTTP/1.1 403"));
        assert_guest_untouched(&upstream);
    }

    #[test]
    fn unsolicited_and_oversize_reserved_requests_are_rejected_without_the_guest() {
        use crate::desktop_bridge::Op;
        let (_directory, upstream, socket) = guest();
        let proxy = Proxy::start(socket, 6901, "silo", "password").unwrap();
        let unsolicited = "/__silo/v1/clipboard?nonce=abc123&kind=text/plain";
        assert!(reserved_request(&proxy, unsolicited, true, b"x").starts_with("HTTP/1.1 403"));
        let expectation = proxy.inbox.expect(Op::Capabilities, Duration::from_secs(5));
        let target = format!(
            "/__silo/v1/capabilities?nonce={}&kind=application/json",
            expectation.nonce
        );
        let big = vec![b'x'; Op::Capabilities.max_bytes() + 1];
        assert!(reserved_request(&proxy, &target, true, &big).starts_with("HTTP/1.1 413"));
        assert!(expectation.wait(Duration::from_millis(100)).is_err());
        assert_guest_untouched(&upstream);
    }

    #[test]
    fn expired_nonces_are_rejected_on_the_wire() {
        use crate::desktop_bridge::Op;
        let (_directory, upstream, socket) = guest();
        let proxy = Proxy::start(socket, 6901, "silo", "password").unwrap();
        let expectation = proxy.inbox.expect(Op::Clipboard, Duration::from_millis(1));
        thread::sleep(Duration::from_millis(20));
        let target = format!(
            "/__silo/v1/clipboard?nonce={}&kind=text/plain",
            expectation.nonce
        );
        assert!(reserved_request(&proxy, &target, true, b"late").starts_with("HTTP/1.1 403"));
        assert_guest_untouched(&upstream);
    }
}
