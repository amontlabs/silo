//! A local name per computer and port for published websites
//! (`http://3000.fork-1.silo.localhost:<proxy port>`).
//!
//! One loopback listener serves every computer. The first label of the host name is the
//! guest port and the second is the computer's name, so several computers that all serve
//! guest `:3000` can be open side by side while no guest port number is bound on the host.
//! `*.localhost` resolves to loopback (RFC 6761) in browsers and curl, so no `/etc/hosts`
//! edit or elevated privilege is needed. The design and the alternatives are in
//! `docs/SiloUI-URL-PROXY.md`.
//!
//! The route table is replaced from observed published-port state (`UrlProxy::set_routes`),
//! each observation tagged with a ticket taken before it was made; an older ticket never
//! overwrites a newer table. Requests are forwarded by hyper, one upstream connection per
//! request, with hop-by-hop headers removed and `X-Forwarded-*` set here.
use bytes::Bytes;
use http_body_util::{combinators::BoxBody, BodyExt, Full};
use hyper::{
    body::Incoming,
    header::{
        HeaderMap, HeaderName, HeaderValue, CONNECTION, HOST, PROXY_AUTHENTICATE,
        PROXY_AUTHORIZATION, TE, TRAILER, TRANSFER_ENCODING, UPGRADE,
    },
    server::conn::http1 as server_http1,
    service::service_fn,
    Request, Response, StatusCode,
};
use hyper_util::rt::{TokioIo, TokioTimer};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    convert::Infallible,
    io::Write,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener as StdListener, TcpStream as StdStream},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::Duration,
};
use tokio::{
    net::TcpStream,
    runtime::Runtime,
    sync::{watch, Semaphore},
    task::JoinHandle,
};

/// The name suffix every computer's host name ends with.
pub const DOMAIN: &str = "silo.localhost";
/// The proxy's port unless `SILO_URL_PROXY_PORT` names another; an ephemeral port is used
/// when it is busy.
pub const DEFAULT_PORT: u16 = 47383;

type Body = BoxBody<Bytes, Box<dyn std::error::Error + Send + Sync>>;

/// One reachable published website of a computer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route {
    pub computer_id: String,
    pub computer: String,
    pub guest_port: u16,
    /// The loopback port the runtime forwards this guest port on.
    pub upstream_port: u16,
}

/// A computer's name for one guest port.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComputerUrl {
    pub guest_port: u16,
    pub host: String,
    pub url: String,
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Time a client has to send its request head.
    pub header_read: Duration,
    /// Time the computer has to connect and to answer with response headers.
    pub upstream: Duration,
    /// Connections served at once; further ones are closed.
    pub max_connections: usize,
    /// Time a response may go without sending a chunk of its body.
    pub body_idle: Duration,
    /// Time a WebSocket tunnel may carry nothing in either direction.
    pub tunnel_idle: Duration,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            header_read: Duration::from_secs(10),
            upstream: Duration::from_secs(60),
            max_connections: 256,
            body_idle: Duration::from_secs(120),
            tunnel_idle: Duration::from_secs(900),
        }
    }
}

struct PortRoute {
    upstream: u16,
    /// Dropped with the route; every connection of the route watches for that.
    cancel: watch::Sender<()>,
}
struct Entry {
    label: String,
    ports: BTreeMap<u16, PortRoute>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
struct Assigned {
    label: String,
    name: String,
}
#[derive(Default)]
struct Names {
    by_id: BTreeMap<String, Assigned>,
    path: Option<PathBuf>,
}
impl Names {
    fn used(&self) -> BTreeSet<&str> {
        self.by_id.values().map(|a| a.label.as_str()).collect()
    }

    /// The label a computer keeps for as long as it exists. A label is never given to a
    /// second computer: candidates are checked against every label ever assigned.
    fn ensure(&mut self, id: &str, name: &str) -> (String, bool) {
        if let Some(assigned) = self.by_id.get(id) {
            return (assigned.label.clone(), false);
        }
        let base = label_of(name);
        let short: String = id
            .chars()
            .filter(char::is_ascii_hexdigit)
            .take(8)
            .collect::<String>()
            .to_ascii_lowercase();
        let used = self.used();
        let mut candidate = base.clone();
        let mut attempt = 1u32;
        while used.contains(candidate.as_str()) {
            candidate = if attempt == 1 {
                format!("{base}-{short}")
            } else {
                format!("{base}-{short}-{attempt}")
            };
            attempt += 1;
        }
        self.by_id.insert(
            id.to_owned(),
            Assigned {
                label: candidate.clone(),
                name: name.to_owned(),
            },
        );
        (candidate, true)
    }

    fn save(&self) {
        let Some(path) = &self.path else { return };
        let Ok(bytes) = serde_json::to_vec(&self.by_id) else {
            return;
        };
        let Some(parent) = path.parent() else { return };
        if let Ok(mut file) = tempfile::NamedTempFile::new_in(parent) {
            if file.write_all(&bytes).is_ok() {
                let _ = file.persist(path);
            }
        }
    }
}

struct Inner {
    entries: BTreeMap<String, Entry>,
    names: Names,
    /// The newest ticket applied, by an update or a removal.
    applied: u64,
}

struct Shared {
    inner: Mutex<Inner>,
    tickets: AtomicU64,
    proxy_port: AtomicU16,
    alive: AtomicBool,
    limits: Limits,
    permits: Arc<Semaphore>,
}

pub struct UrlProxy {
    runtime: Runtime,
    shared: Arc<Shared>,
    task: Mutex<Option<JoinHandle<()>>>,
    preferred: u16,
}

impl UrlProxy {
    pub fn new(preferred_port: u16, limits: Limits) -> std::io::Result<Self> {
        let proxy = Self {
            runtime: tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .thread_name("silo-url-proxy")
                .enable_all()
                .build()?,
            shared: Arc::new(Shared {
                inner: Mutex::new(Inner {
                    entries: BTreeMap::new(),
                    names: Names::default(),
                    applied: 0,
                }),
                tickets: AtomicU64::new(0),
                proxy_port: AtomicU16::new(0),
                alive: AtomicBool::new(false),
                limits,
                permits: Arc::new(Semaphore::new(limits.max_connections)),
            }),
            task: Mutex::new(None),
            preferred: preferred_port,
        };
        proxy.ensure_listener();
        if !proxy.shared.alive.load(Ordering::Acquire) {
            return Err(std::io::Error::other("The URL proxy could not listen."));
        }
        Ok(proxy)
    }

    /// The loopback port every name is served on.
    pub fn proxy_port(&self) -> u16 {
        self.shared.proxy_port.load(Ordering::Acquire)
    }

    /// Takes a ticket; take it before observing the state that `set_routes` will apply.
    pub fn ticket(&self) -> u64 {
        self.shared.tickets.fetch_add(1, Ordering::AcqRel) + 1
    }

    /// Keeps name assignments in this file, so a computer keeps its name across restarts.
    pub fn use_names_file(&self, path: PathBuf) {
        let mut inner = lock(&self.shared.inner);
        if inner.names.path.as_ref() == Some(&path) {
            return;
        }
        if let Some(saved) = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<BTreeMap<String, Assigned>>(&bytes).ok())
        {
            let used: BTreeSet<String> = inner
                .names
                .by_id
                .values()
                .map(|a| a.label.clone())
                .collect();
            for (id, assigned) in saved {
                if !inner.names.by_id.contains_key(&id) && !used.contains(&assigned.label) {
                    inner.names.by_id.insert(id, assigned);
                }
            }
        }
        inner.names.path = Some(path);
        inner.names.save();
    }

    /// Replaces every route with an observation made after `ticket` was taken. An
    /// observation older than the last applied change is dropped. Returns whether it applied.
    pub fn set_routes(&self, ticket: u64, routes: &[Route]) -> bool {
        self.ensure_listener();
        let proxy_port = self.proxy_port();
        let mut grouped: BTreeMap<&str, (&str, Vec<&Route>)> = BTreeMap::new();
        for route in routes {
            // A route to the proxy itself would loop.
            if route.guest_port == 0
                || route.upstream_port == 0
                || route.upstream_port == proxy_port
            {
                continue;
            }
            grouped
                .entry(route.computer_id.as_str())
                .or_insert((route.computer.as_str(), Vec::new()))
                .1
                .push(route);
        }
        let mut inner = lock(&self.shared.inner);
        if ticket <= inner.applied {
            return false;
        }
        inner.applied = ticket;
        let mut previous = std::mem::take(&mut inner.entries);
        let mut changed = false;
        let mut entries = BTreeMap::new();
        for (id, (name, rows)) in grouped {
            let (label, new) = inner.names.ensure(id, name);
            changed |= new;
            let mut old = previous.remove(id).map(|entry| entry.ports);
            let mut ports = BTreeMap::new();
            for route in rows {
                let kept = old
                    .as_mut()
                    .and_then(|ports| ports.remove(&route.guest_port))
                    .filter(|port| port.upstream == route.upstream_port);
                ports.insert(
                    route.guest_port,
                    kept.unwrap_or_else(|| PortRoute {
                        upstream: route.upstream_port,
                        cancel: watch::channel(()).0,
                    }),
                );
            }
            entries.insert(id.to_owned(), Entry { label, ports });
        }
        // Whatever is left in `previous` and in replaced ports is dropped here, which
        // closes its connections and tunnels.
        inner.entries = entries;
        if changed {
            inner.names.save();
        }
        true
    }

    /// Removes a deleted computer's routes and its name, by name.
    pub fn forget_computer(&self, computer: &str) {
        self.forget(|_, assigned| assigned.is_some_and(|a| a.name == computer));
    }

    /// Removes a deleted computer's routes and its name, by id.
    pub fn forget_computer_id(&self, computer_id: &str) {
        self.forget(|id, _| id == computer_id);
    }

    fn forget(&self, matches: impl Fn(&str, Option<&Assigned>) -> bool) {
        let mut inner = lock(&self.shared.inner);
        // A removal is a newer observation than any taken before it.
        let ticket = self.shared.tickets.fetch_add(1, Ordering::AcqRel) + 1;
        inner.applied = inner.applied.max(ticket);
        let ids: Vec<String> = inner
            .names
            .by_id
            .keys()
            .chain(inner.entries.keys())
            .filter(|id| matches(id, inner.names.by_id.get(*id)))
            .cloned()
            .collect();
        for id in &ids {
            inner.entries.remove(id);
            inner.names.by_id.remove(id);
        }
        if !ids.is_empty() {
            inner.names.save();
        }
    }

    /// The names a computer is open at.
    pub fn urls(&self, computer_id: &str) -> Vec<ComputerUrl> {
        let inner = lock(&self.shared.inner);
        let port = self.proxy_port();
        inner
            .entries
            .get(computer_id)
            .into_iter()
            .flat_map(|entry| {
                entry.ports.keys().map(move |guest| {
                    let host = format!("{guest}.{}.{DOMAIN}", entry.label);
                    ComputerUrl {
                        guest_port: *guest,
                        url: format!("http://{host}:{port}"),
                        host,
                    }
                })
            })
            .collect()
    }

    /// Whether any route is open.
    pub fn has_routes(&self) -> bool {
        !lock(&self.shared.inner).entries.is_empty()
    }

    /// Starts the listener when it is not running, on the previous port when it is free.
    fn ensure_listener(&self) {
        let mut task = lock(&self.task);
        if self.shared.alive.load(Ordering::Acquire)
            && task.as_ref().is_some_and(|t| !t.is_finished())
        {
            return;
        }
        if let Some(old) = task.take() {
            old.abort();
        }
        let previous = self.shared.proxy_port.load(Ordering::Acquire);
        let wanted = if previous != 0 {
            previous
        } else {
            self.preferred
        };
        // Loopback only. Something already answering on the number keeps it.
        let listener = (wanted != 0 && !answers(wanted))
            .then(|| StdListener::bind((Ipv4Addr::LOCALHOST, wanted)).ok())
            .flatten()
            .or_else(|| StdListener::bind((Ipv4Addr::LOCALHOST, 0)).ok());
        let Some(listener) = listener else { return };
        let Ok(address) = listener.local_addr() else {
            return;
        };
        if listener.set_nonblocking(true).is_err() {
            return;
        }
        let _enter = self.runtime.enter();
        let Ok(listener) = tokio::net::TcpListener::from_std(listener) else {
            return;
        };
        self.shared
            .proxy_port
            .store(address.port(), Ordering::Release);
        self.shared.alive.store(true, Ordering::Release);
        let shared = self.shared.clone();
        *task = Some(self.runtime.spawn(accept_loop(listener, shared)));
    }

    #[cfg(test)]
    fn stop_listener(&self) {
        if let Some(task) = lock(&self.task).take() {
            task.abort();
        }
        self.shared.alive.store(false, Ordering::Release);
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// Marks the listener gone however its task ends, so the next sync opens it again.
struct Deregister(Arc<Shared>);
impl Drop for Deregister {
    fn drop(&mut self) {
        self.0.alive.store(false, Ordering::Release);
    }
}

async fn accept_loop(listener: tokio::net::TcpListener, shared: Arc<Shared>) {
    let _guard = Deregister(shared.clone());
    let header_read = shared.limits.header_read;
    let mut backoff = Duration::from_millis(10);
    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                backoff = Duration::from_millis(10);
                let Ok(permit) = shared.permits.clone().try_acquire_owned() else {
                    continue; // Over the limit: the connection closes.
                };
                let shared = shared.clone();
                // An upgraded tunnel takes the permit out and holds it until it closes.
                let slot = Arc::new(Mutex::new(Some(permit)));
                tokio::spawn(async move {
                    let service = service_fn(move |request| {
                        let shared = shared.clone();
                        let slot = slot.clone();
                        async move { Ok::<_, Infallible>(route(&shared, peer, slot, request).await) }
                    });
                    let mut server = server_http1::Builder::new();
                    server
                        .timer(TokioTimer::new())
                        .header_read_timeout(header_read)
                        .keep_alive(false);
                    let _ = server
                        .serve_connection(TokioIo::new(stream), service)
                        .with_upgrades()
                        .await;
                });
            }
            // Descriptor exhaustion and aborted handshakes pass; wait and keep accepting.
            Err(_) => {
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(1));
            }
        }
    }
}

/// Whether anything accepts connections on this loopback port, on either address family.
fn answers(port: u16) -> bool {
    let timeout = Duration::from_millis(100);
    [
        SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        SocketAddr::from((Ipv6Addr::LOCALHOST, port)),
    ]
    .iter()
    .any(|address| StdStream::connect_timeout(address, timeout).is_ok())
}

/// One DNS label for a computer name.
fn label_of(name: &str) -> String {
    let mut label = String::new();
    for character in name.chars() {
        let character = character.to_ascii_lowercase();
        if character.is_ascii_lowercase() || character.is_ascii_digit() {
            label.push(character);
        } else if !label.is_empty() && !label.ends_with('-') {
            label.push('-');
        }
    }
    label.truncate(40);
    let label = label.trim_end_matches('-');
    if label.is_empty() {
        "computer".to_owned()
    } else {
        label.to_owned()
    }
}

fn text(status: StatusCode, message: &str) -> Response<Body> {
    let mut response = Response::new(
        Full::new(Bytes::from(format!("{message}\n")))
            .map_err(|never| match never {})
            .boxed(),
    );
    *response.status_mut() = status;
    response.headers_mut().insert(
        hyper::header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response
}

/// `(guest port, computer label)` of `<port>.<label>.silo.localhost[:proxy port]`; `None`
/// when the name is not ours or its port is not this proxy's.
fn parse_host(value: &str, proxy_port: u16) -> Option<(u16, String)> {
    if value.starts_with('[') {
        return None;
    }
    let (name, port) = match value.rsplit_once(':') {
        Some((name, port)) => (name, port.parse::<u16>().ok()?),
        None => (value, 80),
    };
    if port != proxy_port {
        return None;
    }
    let name = name.trim_end_matches('.').to_ascii_lowercase();
    let rest = name.strip_suffix(&format!(".{DOMAIN}"))?;
    let (guest, label) = rest.split_once('.')?;
    if label.is_empty() || label.contains('.') || guest.len() > 5 {
        return None;
    }
    Some((
        guest.parse().ok().filter(|port| *port != 0)?,
        label.to_owned(),
    ))
}

fn tokens(headers: &HeaderMap, name: HeaderName) -> impl Iterator<Item = String> + '_ {
    headers
        .get_all(name)
        .into_iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(|token| token.trim().to_ascii_lowercase())
        .filter(|token| !token.is_empty())
}

fn wants_websocket(headers: &HeaderMap) -> bool {
    tokens(headers, UPGRADE).any(|token| token == "websocket")
        && tokens(headers, CONNECTION).any(|token| token == "upgrade")
}

/// Removes the headers that belong to one connection, and those `Connection` names.
fn strip_hop_by_hop(headers: &mut HeaderMap) {
    let nominated: Vec<HeaderName> = tokens(headers, CONNECTION)
        .filter_map(|token| HeaderName::from_bytes(token.as_bytes()).ok())
        .collect();
    for name in nominated {
        headers.remove(name);
    }
    for name in [
        CONNECTION,
        PROXY_AUTHORIZATION,
        PROXY_AUTHENTICATE,
        TE,
        TRAILER,
        TRANSFER_ENCODING,
        UPGRADE,
        HeaderName::from_static("keep-alive"),
        HeaderName::from_static("proxy-connection"),
    ] {
        headers.remove(name);
    }
}

fn allow_websocket(headers: &mut HeaderMap) {
    headers.insert(CONNECTION, HeaderValue::from_static("upgrade"));
    headers.insert(UPGRADE, HeaderValue::from_static("websocket"));
}

async fn closed(mut watcher: watch::Receiver<()>) {
    while watcher.changed().await.is_ok() {}
}

async fn route(
    shared: &Shared,
    peer: SocketAddr,
    slot: Arc<Mutex<Option<tokio::sync::OwnedSemaphorePermit>>>,
    mut request: Request<Incoming>,
) -> Response<Body> {
    let limits = shared.limits;
    let proxy_port = shared.proxy_port.load(Ordering::Acquire);
    let host_value = request
        .headers()
        .get(HOST)
        .and_then(|host| host.to_str().ok())
        .map(str::to_owned);
    let Some(host_value) = host_value else {
        return text(StatusCode::BAD_REQUEST, "The request has no Host header.");
    };
    let Some((guest_port, label)) = parse_host(&host_value, proxy_port) else {
        return text(
            StatusCode::NOT_FOUND,
            &format!("{host_value} is not a Silo computer name on this port."),
        );
    };
    let found = {
        let inner = lock(&shared.inner);
        inner
            .entries
            .values()
            .find(|entry| entry.label == label)
            .map(|entry| {
                entry
                    .ports
                    .get(&guest_port)
                    .map(|port| (port.upstream, port.cancel.subscribe()))
            })
    };
    let (upstream, cancel) = match found {
        None => {
            return text(
                StatusCode::NOT_FOUND,
                &format!("No Silo computer is open at {label}.{DOMAIN}."),
            )
        }
        Some(None) => {
            return text(
                StatusCode::NOT_FOUND,
                &format!("{label}.{DOMAIN} does not publish port {guest_port}."),
            )
        }
        Some(Some(found)) => found,
    };
    if upstream == proxy_port {
        return text(StatusCode::LOOP_DETECTED, "The route points at the proxy.");
    }
    let connect = tokio::time::timeout(
        limits.upstream,
        TcpStream::connect((Ipv4Addr::LOCALHOST, upstream)),
    )
    .await;
    let stream = match connect {
        Ok(Ok(stream)) => stream,
        Ok(Err(_)) => {
            return text(
                StatusCode::BAD_GATEWAY,
                &format!(
                "{label}.{DOMAIN} is not answering on port {guest_port}. Is the computer running?"
            ),
            )
        }
        Err(_) => {
            return text(
                StatusCode::GATEWAY_TIMEOUT,
                "The computer did not accept the connection.",
            )
        }
    };
    let Ok((mut sender, connection)) =
        hyper::client::conn::http1::handshake(TokioIo::new(stream)).await
    else {
        return text(
            StatusCode::BAD_GATEWAY,
            "The computer closed the connection.",
        );
    };
    let watching = cancel.clone();
    tokio::spawn(async move {
        tokio::select! {
            _ = connection.with_upgrades() => {}
            _ = closed(watching) => {}
        }
    });

    let websocket = wants_websocket(request.headers());
    let client_upgrade = websocket.then(|| hyper::upgrade::on(&mut request));
    {
        let headers = request.headers_mut();
        strip_hop_by_hop(headers);
        if websocket {
            allow_websocket(headers);
        }
        for name in [
            "x-forwarded-host",
            "x-forwarded-proto",
            "x-forwarded-for",
            "forwarded",
        ] {
            headers.remove(name);
        }
        if let Ok(value) = HeaderValue::from_str(&host_value) {
            headers.insert("x-forwarded-host", value);
        }
        headers.insert("x-forwarded-proto", HeaderValue::from_static("http"));
        if let Ok(value) = HeaderValue::from_str(&peer.ip().to_string()) {
            headers.insert("x-forwarded-for", value);
        }
    }
    let sent = tokio::time::timeout(
        limits.upstream,
        sender.send_request(request.map(|body| body.boxed())),
    )
    .await;
    let mut response = match sent {
        Ok(Ok(response)) => response,
        Ok(Err(_)) => {
            return text(
                StatusCode::BAD_GATEWAY,
                "The computer did not answer the request.",
            )
        }
        Err(_) => {
            return text(
                StatusCode::GATEWAY_TIMEOUT,
                "The computer took too long to answer.",
            )
        }
    };
    let upgraded = response.status() == StatusCode::SWITCHING_PROTOCOLS;
    if upgraded && !(websocket && wants_websocket(response.headers())) {
        return text(
            StatusCode::BAD_GATEWAY,
            "The computer offered an unexpected upgrade.",
        );
    }
    let upstream_upgrade = upgraded.then(|| hyper::upgrade::on(&mut response));
    strip_hop_by_hop(response.headers_mut());
    if upgraded {
        allow_websocket(response.headers_mut());
    }
    if let (Some(client), Some(upstream)) = (client_upgrade, upstream_upgrade) {
        let permit = lock(&slot).take();
        let idle = limits.tunnel_idle;
        tokio::spawn(async move {
            let _permit = permit;
            if let (Ok(client), Ok(upstream)) = (client.await, upstream.await) {
                tokio::select! {
                    _ = tunnel(TokioIo::new(client), TokioIo::new(upstream), idle) => {}
                    _ = closed(cancel) => {}
                }
            }
        });
    }
    let idle = limits.body_idle;
    response.map(|body| IdleBody::new(body, idle).boxed())
}

/// Copies both ways until either side closes or nothing moves in either direction for `idle`.
async fn tunnel<A, B>(a: A, b: B, idle: Duration)
where
    A: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    B: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let start = tokio::time::Instant::now();
    let last = AtomicU64::new(0);
    let (a_read, a_write) = tokio::io::split(a);
    let (b_read, b_write) = tokio::io::split(b);
    tokio::select! {
        _ = pump(a_read, b_write, idle, start, &last) => {}
        _ = pump(b_read, a_write, idle, start, &last) => {}
    }
}

async fn pump<R, W>(
    mut from: R,
    mut to: W,
    idle: Duration,
    start: tokio::time::Instant,
    last: &AtomicU64,
) where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let now = || start.elapsed().as_millis() as u64;
    let mut buffer = vec![0u8; 16 * 1024];
    loop {
        match tokio::time::timeout(idle, from.read(&mut buffer)).await {
            Ok(Ok(count)) if count > 0 => {
                last.store(now(), Ordering::Relaxed);
                if to.write_all(&buffer[..count]).await.is_err() {
                    break;
                }
            }
            Ok(_) => break,
            // Quiet this way; keep going while the other way is moving.
            Err(_) => {
                if now().saturating_sub(last.load(Ordering::Relaxed)) >= idle.as_millis() as u64 {
                    break;
                }
            }
        }
    }
    let _ = to.shutdown().await;
}

/// A response body that fails when no chunk arrives for `idle`.
struct IdleBody {
    inner: BoxBody<Bytes, hyper::Error>,
    sleep: std::pin::Pin<Box<tokio::time::Sleep>>,
    idle: Duration,
}
impl IdleBody {
    fn new(inner: Incoming, idle: Duration) -> Self {
        Self {
            inner: inner.boxed(),
            sleep: Box::pin(tokio::time::sleep(idle)),
            idle,
        }
    }
}
impl hyper::body::Body for IdleBody {
    type Data = Bytes;
    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn poll_frame(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<hyper::body::Frame<Bytes>, Self::Error>>> {
        use std::{future::Future, task::Poll};
        let this = &mut *self;
        match std::pin::Pin::new(&mut this.inner).poll_frame(cx) {
            Poll::Ready(frame) => {
                let deadline = tokio::time::Instant::now() + this.idle;
                this.sleep.as_mut().reset(deadline);
                Poll::Ready(frame.map(|frame| frame.map_err(Into::into)))
            }
            Poll::Pending => match this.sleep.as_mut().poll(cx) {
                Poll::Ready(()) => Poll::Ready(Some(Err("The computer stopped sending.".into()))),
                Poll::Pending => Poll::Pending,
            },
        }
    }
}

static SHARED: OnceLock<Option<UrlProxy>> = OnceLock::new();

/// The engine's proxy, started on first use. `None` when it could not listen.
pub fn shared() -> Option<&'static UrlProxy> {
    SHARED
        .get_or_init(|| {
            let port = std::env::var("SILO_URL_PROXY_PORT")
                .ok()
                .and_then(|port| port.parse().ok())
                .unwrap_or(DEFAULT_PORT);
            UrlProxy::new(port, Limits::default()).ok()
        })
        .as_ref()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    /// A fake computer. `/headers` answers with the request head; an upgrade echoes;
    /// `/hang` never answers; anything else answers with its own name.
    fn upstream(name: &'static str) -> u16 {
        let listener = StdListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                std::thread::spawn(move || {
                    let mut seen = Vec::new();
                    let mut byte = [0u8; 1];
                    while !seen.ends_with(b"\r\n\r\n") {
                        if stream.read(&mut byte).unwrap_or(0) == 0 {
                            return;
                        }
                        seen.push(byte[0]);
                    }
                    let head = String::from_utf8_lossy(&seen).to_string();
                    let lower = head.to_ascii_lowercase();
                    if lower.contains("upgrade: websocket") {
                        let _ = stream.write_all(
                            b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n",
                        );
                        let mut buffer = [0u8; 64];
                        while let Ok(count) = stream.read(&mut buffer) {
                            if count == 0 || stream.write_all(&buffer[..count]).is_err() {
                                return;
                            }
                        }
                        return;
                    }
                    if head.starts_with("GET /stall") {
                        let _ = stream
                            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\npartial");
                        std::thread::sleep(Duration::from_secs(5));
                        return;
                    }
                    if head.starts_with("GET /hang") {
                        std::thread::sleep(Duration::from_secs(5));
                        return;
                    }
                    let body = if head.starts_with("GET /headers") {
                        head.clone()
                    } else {
                        format!("hello from {name}")
                    };
                    let _ = write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nKeep-Alive: timeout=5\r\nX-Hop: 1\r\nConnection: close, x-hop\r\n\r\n{body}",
                        body.len()
                    );
                });
            }
        });
        port
    }

    fn proxy() -> UrlProxy {
        UrlProxy::new(0, Limits::default()).unwrap()
    }

    fn apply(proxy: &UrlProxy, routes: &[Route]) {
        assert!(proxy.set_routes(proxy.ticket(), routes));
    }

    fn request(port: u16, host: &str, path: &str, extra: &str) -> (u16, String, String) {
        let mut stream = StdStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: {host}\r\n{extra}Connection: close\r\n\r\n"
        )
        .unwrap();
        let mut reply = String::new();
        stream.read_to_string(&mut reply).unwrap();
        let status = reply.split(' ').nth(1).unwrap().parse().unwrap();
        let (head, body) = reply.split_once("\r\n\r\n").unwrap_or((&reply, ""));
        (status, head.to_owned(), body.trim().to_owned())
    }

    fn get(proxy: &UrlProxy, host: &str) -> (u16, String) {
        let port = proxy.proxy_port();
        let (status, _, body) = request(port, &format!("{host}:{port}"), "/", "");
        (status, body)
    }

    fn route_for(name: &str, id: &str, guest: u16, upstream: u16) -> Route {
        Route {
            computer_id: id.into(),
            computer: name.into(),
            guest_port: guest,
            upstream_port: upstream,
        }
    }

    fn host_of(proxy: &UrlProxy, id: &str, guest: u16) -> String {
        proxy
            .urls(id)
            .into_iter()
            .find(|url| url.guest_port == guest)
            .unwrap()
            .host
    }

    #[test]
    fn computers_serving_the_same_guest_port_are_told_apart_by_the_name() {
        let proxy = proxy();
        apply(
            &proxy,
            &[
                route_for("fork-a", "aaaaaaaa-0000", 3000, upstream("a")),
                route_for("Fork B", "bbbbbbbb-0000", 3000, upstream("b")),
            ],
        );
        assert_eq!(
            host_of(&proxy, "aaaaaaaa-0000", 3000),
            "3000.fork-a.silo.localhost"
        );
        assert_eq!(
            get(&proxy, "3000.fork-a.silo.localhost"),
            (200, "hello from a".into())
        );
        assert_eq!(
            get(&proxy, "3000.fork-b.SILO.localhost"),
            (200, "hello from b".into())
        );
        assert_eq!(
            proxy.urls("bbbbbbbb-0000")[0].url,
            format!("http://3000.fork-b.silo.localhost:{}", proxy.proxy_port())
        );
    }

    #[test]
    fn unknown_names_unpublished_ports_and_wrong_proxy_ports_are_404_and_stopped_computers_502() {
        let proxy = proxy();
        let closed = {
            let listener = StdListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            listener.local_addr().unwrap().port()
        };
        apply(
            &proxy,
            &[
                route_for("web", "11111111", 3000, upstream("web")),
                route_for("stopped", "22222222", 3000, closed),
            ],
        );
        assert_eq!(get(&proxy, "3000.nobody.silo.localhost").0, 404);
        assert_eq!(get(&proxy, "4000.web.silo.localhost").0, 404);
        assert_eq!(get(&proxy, "web.silo.localhost").0, 404);
        assert_eq!(get(&proxy, "127.0.0.1").0, 404);
        assert_eq!(get(&proxy, "3000.stopped.silo.localhost").0, 502);
        let port = proxy.proxy_port();
        let other = port.wrapping_add(1);
        assert_eq!(
            request(port, &format!("3000.web.silo.localhost:{other}"), "/", "").0,
            404
        );
        assert_eq!(request(port, "3000.web.silo.localhost", "/", "").0, 404);
    }

    #[test]
    fn dropped_computers_lose_routes_and_open_tunnels() {
        let proxy = proxy();
        apply(
            &proxy,
            &[
                route_for("one", "11111111", 3000, upstream("one")),
                route_for("two", "22222222", 3000, upstream("two")),
            ],
        );
        let port = proxy.proxy_port();
        let mut tunnel = open_websocket(port, "3000.one.silo.localhost");
        tunnel.write_all(b"hi").unwrap();
        let mut echoed = [0u8; 2];
        tunnel.read_exact(&mut echoed).unwrap();
        proxy.forget_computer("one");
        assert!(proxy.urls("11111111").is_empty());
        assert_eq!(get(&proxy, "3000.one.silo.localhost").0, 404);
        assert_eq!(get(&proxy, "3000.two.silo.localhost").0, 200);
        let mut rest = Vec::new();
        assert!(
            tunnel.read_to_end(&mut rest).is_ok() && rest.is_empty(),
            "the open tunnel closes with its route"
        );
        proxy.forget_computer_id("22222222");
        assert_eq!(get(&proxy, "3000.two.silo.localhost").0, 404);
    }

    fn open_websocket(port: u16, host: &str) -> StdStream {
        let mut stream = StdStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        write!(
            stream,
            "GET /ws HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n"
        )
        .unwrap();
        let mut seen = Vec::new();
        let mut byte = [0u8; 1];
        while !seen.ends_with(b"\r\n\r\n") {
            assert_eq!(stream.read(&mut byte).unwrap(), 1);
            seen.push(byte[0]);
        }
        assert!(String::from_utf8_lossy(&seen).starts_with("HTTP/1.1 101"));
        stream
    }

    #[test]
    fn websocket_upgrades_pass_through_both_ways() {
        let proxy = proxy();
        apply(
            &proxy,
            &[route_for("hmr", "11111111", 3000, upstream("hmr"))],
        );
        let mut stream = open_websocket(proxy.proxy_port(), "3000.hmr.silo.localhost");
        stream.write_all(b"ping-frame").unwrap();
        let mut echoed = [0u8; 10];
        stream.read_exact(&mut echoed).unwrap();
        assert_eq!(&echoed, b"ping-frame");
    }

    #[test]
    fn names_are_unique_stable_and_never_transferred() {
        let proxy = proxy();
        let path = tempfile::tempdir().unwrap();
        let file = path.path().join("names.json");
        proxy.use_names_file(file.clone());
        let u = upstream("x");
        // `a--b` and `a-b` reduce to one label, and `a--b-bbbbbbbb` takes the suffixed one.
        apply(
            &proxy,
            &[
                route_for("a--b", "aaaaaaaa", 3000, u),
                route_for("a--b-bbbbbbbb", "bbbbbbbb", 3000, u),
                route_for("a-b", "bbbbbbbb2", 3000, u),
            ],
        );
        let hosts: BTreeSet<String> = ["aaaaaaaa", "bbbbbbbb", "bbbbbbbb2"]
            .iter()
            .map(|id| host_of(&proxy, id, 3000))
            .collect();
        assert_eq!(hosts.len(), 3, "{hosts:?}");
        let first = host_of(&proxy, "aaaaaaaa", 3000);
        // The first computer stops and later returns: it keeps its name, and a new computer
        // with the same name does not take it.
        apply(&proxy, &[route_for("a-b", "cccccccc", 3000, u)]);
        assert_ne!(host_of(&proxy, "cccccccc", 3000), first);
        apply(&proxy, &[route_for("a--b", "aaaaaaaa", 3000, u)]);
        assert_eq!(host_of(&proxy, "aaaaaaaa", 3000), first);
        // A restart keeps the assignments.
        let again = UrlProxy::new(0, Limits::default()).unwrap();
        again.use_names_file(file);
        apply(&again, &[route_for("a-b", "cccccccc", 3000, u)]);
        assert_ne!(host_of(&again, "cccccccc", 3000), first);
        apply(&again, &[route_for("a--b", "aaaaaaaa", 3000, u)]);
        assert_eq!(host_of(&again, "aaaaaaaa", 3000), first);
    }

    #[test]
    fn a_route_to_the_proxy_itself_is_refused() {
        let proxy = proxy();
        apply(
            &proxy,
            &[route_for("loop", "11111111", 3000, proxy.proxy_port())],
        );
        assert!(proxy.urls("11111111").is_empty());
    }

    #[test]
    fn an_older_observation_never_overwrites_a_newer_one_or_a_removal() {
        let proxy = proxy();
        let u = upstream("x");
        let old = proxy.ticket();
        let newer = proxy.ticket();
        assert!(proxy.set_routes(newer, &[route_for("one", "11111111", 3000, u)]));
        assert!(!proxy.set_routes(old, &[]));
        assert_eq!(proxy.urls("11111111").len(), 1);
        let before_removal = proxy.ticket();
        proxy.forget_computer_id("11111111");
        assert!(!proxy.set_routes(before_removal, &[route_for("one", "11111111", 3000, u)]));
        assert!(proxy.urls("11111111").is_empty());
    }

    #[test]
    fn hop_by_hop_headers_go_and_forwarded_headers_are_set_here() {
        let proxy = proxy();
        apply(
            &proxy,
            &[route_for("web", "11111111", 3000, upstream("web"))],
        );
        let port = proxy.proxy_port();
        let host = format!("3000.web.silo.localhost:{port}");
        let (status, head, body) = request(
            port,
            &host,
            "/headers",
            "Connection: close, x-secret\r\nX-Secret: 1\r\nProxy-Authorization: Basic eDp5\r\nKeep-Alive: 5\r\nX-Forwarded-For: 6.6.6.6\r\nX-Forwarded-Host: evil.example\r\nForwarded: for=6.6.6.6\r\n",
        );
        assert_eq!(status, 200);
        let seen = body.to_ascii_lowercase();
        for gone in [
            "x-secret",
            "proxy-authorization",
            "keep-alive",
            "6.6.6.6",
            "evil.example",
            "forwarded: for",
        ] {
            assert!(!seen.contains(gone), "{gone} reached the computer: {body}");
        }
        assert!(seen.contains(&format!("x-forwarded-host: {}", host.to_ascii_lowercase())));
        assert!(seen.contains("x-forwarded-proto: http"));
        assert!(seen.contains("x-forwarded-for: 127.0.0.1"));
        let reply = head.to_ascii_lowercase();
        assert!(
            !reply.contains("keep-alive") && !reply.contains("x-hop"),
            "{head}"
        );
    }

    #[test]
    fn slow_clients_hung_computers_and_floods_are_cut_off() {
        let limits = Limits {
            header_read: Duration::from_millis(300),
            upstream: Duration::from_millis(500),
            max_connections: 2,
            ..Limits::default()
        };
        let proxy = UrlProxy::new(0, limits).unwrap();
        apply(
            &proxy,
            &[route_for("web", "11111111", 3000, upstream("web"))],
        );
        let port = proxy.proxy_port();
        // A client that never finishes its head is closed.
        let mut slow = StdStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        slow.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        slow.write_all(b"GET / HT").unwrap();
        let mut sink = Vec::new();
        assert!(
            slow.read_to_end(&mut sink).is_ok(),
            "the slow client was closed"
        );
        // A computer that never answers is a 504.
        let host = format!("3000.web.silo.localhost:{port}");
        assert_eq!(request(port, &host, "/hang", "").0, 504);
        // With both slots held, a third connection is closed at once.
        let _a = StdStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        let _b = StdStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        let mut third = StdStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        third
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut rest = Vec::new();
        assert!(third.read_to_end(&mut rest).is_ok() && rest.is_empty());
    }

    #[test]
    fn tunnels_hold_their_permit_and_stalled_bodies_and_idle_tunnels_are_cut() {
        let limits = Limits {
            max_connections: 1,
            body_idle: Duration::from_millis(300),
            tunnel_idle: Duration::from_millis(600),
            ..Limits::default()
        };
        let proxy = UrlProxy::new(0, limits).unwrap();
        apply(
            &proxy,
            &[route_for("web", "11111111", 3000, upstream("web"))],
        );
        let port = proxy.proxy_port();
        // An open tunnel keeps the only permit, so another connection is closed.
        let mut tunnel = open_websocket(port, "3000.web.silo.localhost");
        let mut other = StdStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        other
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut rest = Vec::new();
        assert!(other.read_to_end(&mut rest).is_ok() && rest.is_empty());
        // Quiet in both directions, the tunnel is closed and the permit comes back.
        tunnel
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut rest = Vec::new();
        assert!(tunnel.read_to_end(&mut rest).is_ok() && rest.is_empty());
        std::thread::sleep(Duration::from_millis(100));
        let host = format!("3000.web.silo.localhost:{port}");
        assert_eq!(request(port, &host, "/", "").0, 200);
        // Headers followed by a stall: the body is cut and the permit comes back.
        let mut stalled = StdStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        stalled
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        write!(stalled, "GET /stall HTTP/1.1\r\nHost: {host}\r\n\r\n").unwrap();
        let mut reply = Vec::new();
        let _ = stalled.read_to_end(&mut reply);
        assert!(String::from_utf8_lossy(&reply).starts_with("HTTP/1.1 200"));
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(request(port, &host, "/", "").0, 200);
    }

    #[test]
    fn a_stopped_listener_is_reopened_by_the_next_sync() {
        let proxy = proxy();
        let u = upstream("x");
        apply(&proxy, &[route_for("web", "11111111", 3000, u)]);
        let port = proxy.proxy_port();
        proxy.stop_listener();
        std::thread::sleep(Duration::from_millis(100));
        apply(&proxy, &[route_for("web", "11111111", 3000, u)]);
        assert_eq!(proxy.proxy_port(), port);
        assert_eq!(get(&proxy, "3000.web.silo.localhost").0, 200);
    }

    #[test]
    fn a_busy_preferred_port_falls_back_to_a_free_one() {
        let busy = StdListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let busy_port = busy.local_addr().unwrap().port();
        let proxy = UrlProxy::new(busy_port, Limits::default()).unwrap();
        assert_ne!(proxy.proxy_port(), busy_port);
    }
}
