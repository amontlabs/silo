//! A local name per computer for published websites (`http://<computer>.silo.localhost:3000`).
//!
//! One loopback listener serves each guest port that some computer has published. It routes
//! by `Host` to that computer's published host port, so several computers can all serve
//! guest `:3000` and be opened side by side. `*.localhost` resolves to loopback (RFC 6761)
//! in browsers and curl, so no `/etc/hosts` edit or elevated privilege is needed. The design
//! and the alternatives are in `docs/SiloUI-URL-PROXY.md`.
//!
//! The route table is replaced as a whole from observed published-port state
//! (`UrlProxy::set_routes`), so a stopped, unpublished or deleted computer simply disappears.
//! HTTP and upgraded streams (WebSocket) go through hyper; each request opens its own
//! upstream connection.
use bytes::Bytes;
use http_body_util::{combinators::BoxBody, BodyExt, Full};
use hyper::{
    body::Incoming, header::HOST, server::conn::http1 as server_http1, service::service_fn,
    Request, Response, StatusCode,
};
use hyper_util::rt::TokioIo;
use std::{
    collections::{BTreeMap, BTreeSet},
    convert::Infallible,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener as StdListener, TcpStream as StdStream},
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};
use tokio::{net::TcpStream, runtime::Runtime, task::JoinHandle};

/// The name suffix every computer's host name ends with.
pub const DOMAIN: &str = "silo.localhost";

type Body = BoxBody<Bytes, hyper::Error>;

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

struct Entry {
    computer: String,
    computer_id: String,
    ports: BTreeMap<u16, u16>,
}

#[derive(Default)]
struct Table {
    /// Lower-case host name to its computer.
    hosts: BTreeMap<String, Entry>,
    /// Guest port to the loopback port its listener actually bound.
    listening: BTreeMap<u16, u16>,
}

struct Listener {
    task: JoinHandle<()>,
}

pub struct UrlProxy {
    runtime: Runtime,
    table: Arc<Mutex<Table>>,
    listeners: Mutex<BTreeMap<u16, Listener>>,
    /// Try the guest port number itself before falling back to a free port.
    prefer_guest_port: bool,
}

impl UrlProxy {
    pub fn new(prefer_guest_port: bool) -> std::io::Result<Self> {
        Ok(Self {
            runtime: tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .thread_name("silo-url-proxy")
                .enable_all()
                .build()?,
            table: Arc::default(),
            listeners: Mutex::default(),
            prefer_guest_port,
        })
    }

    /// Replaces every route. Listeners open for guest ports that gained a route and close for
    /// ports no computer serves any more; an unchanged port keeps its listener and URL.
    pub fn set_routes(&self, routes: &[Route]) {
        let mut hosts: BTreeMap<String, Entry> = BTreeMap::new();
        let mut computers: BTreeMap<(&str, &str), Vec<&Route>> = BTreeMap::new();
        for route in routes {
            computers
                .entry((route.computer.as_str(), route.computer_id.as_str()))
                .or_default()
                .push(route);
        }
        for ((name, id), rows) in computers {
            let plain = host_name(name, None);
            let host = if hosts.contains_key(&plain) {
                host_name(name, Some(id))
            } else {
                plain
            };
            hosts.insert(
                host,
                Entry {
                    computer: name.to_owned(),
                    computer_id: id.to_owned(),
                    ports: rows
                        .iter()
                        .map(|route| (route.guest_port, route.upstream_port))
                        .collect(),
                },
            );
        }
        let wanted: BTreeSet<u16> = hosts
            .values()
            .flat_map(|entry| entry.ports.keys().copied())
            .collect();
        let mut listeners = self.listeners.lock().unwrap_or_else(|e| e.into_inner());
        listeners.retain(|port, listener| {
            let keep = wanted.contains(port);
            if !keep {
                listener.task.abort();
            }
            keep
        });
        let mut table = self.table.lock().unwrap_or_else(|e| e.into_inner());
        table.listening.retain(|port, _| wanted.contains(port));
        table.hosts = hosts;
        drop(table);
        for port in wanted {
            if listeners.contains_key(&port) {
                continue;
            }
            // A port that cannot be bound is left without a name; the next sync retries.
            if let Some(listener) = self.open(port) {
                listeners.insert(port, listener);
            }
        }
    }

    /// Removes every route of the computer with this name.
    pub fn forget_computer(&self, computer: &str) {
        let mut routes = self.routes();
        routes.retain(|route| route.computer != computer);
        self.set_routes(&routes);
    }

    fn routes(&self) -> Vec<Route> {
        let table = self.table.lock().unwrap_or_else(|e| e.into_inner());
        table
            .hosts
            .values()
            .flat_map(|entry| {
                entry.ports.iter().map(|(guest, upstream)| Route {
                    computer_id: entry.computer_id.clone(),
                    computer: entry.computer.clone(),
                    guest_port: *guest,
                    upstream_port: *upstream,
                })
            })
            .collect()
    }

    /// The names a computer is open at.
    pub fn urls(&self, computer_id: &str) -> Vec<ComputerUrl> {
        let table = self.table.lock().unwrap_or_else(|e| e.into_inner());
        let mut urls = Vec::new();
        for (host, entry) in &table.hosts {
            if entry.computer_id != computer_id {
                continue;
            }
            for guest_port in entry.ports.keys() {
                if let Some(listen) = table.listening.get(guest_port) {
                    urls.push(ComputerUrl {
                        guest_port: *guest_port,
                        host: host.clone(),
                        url: format!("http://{host}:{listen}"),
                    });
                }
            }
        }
        urls
    }

    fn open(&self, guest_port: u16) -> Option<Listener> {
        let std_listener = self.bind(guest_port)?;
        let listen_port = std_listener.local_addr().ok()?.port();
        std_listener.set_nonblocking(true).ok()?;
        let table = self.table.clone();
        let _enter = self.runtime.enter();
        let listener = tokio::net::TcpListener::from_std(std_listener).ok()?;
        self.table
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .listening
            .insert(guest_port, listen_port);
        let task = self.runtime.spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let table = table.clone();
                tokio::spawn(async move {
                    let service = service_fn(move |request| {
                        let table = table.clone();
                        async move { Ok::<_, Infallible>(route(&table, guest_port, request).await) }
                    });
                    let _ = server_http1::Builder::new()
                        .serve_connection(TokioIo::new(stream), service)
                        .with_upgrades()
                        .await;
                });
            }
        });
        Some(Listener { task })
    }

    fn bind(&self, guest_port: u16) -> Option<StdListener> {
        // Loopback only. Something already answering on the number (the user's own dev server,
        // a published forward) keeps it; a fresh port is used instead and reported in the URL.
        if self.prefer_guest_port && !answers(guest_port) {
            if let Ok(listener) = StdListener::bind((Ipv4Addr::LOCALHOST, guest_port)) {
                return Some(listener);
            }
        }
        StdListener::bind((Ipv4Addr::LOCALHOST, 0)).ok()
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

/// `<label>.silo.localhost`, with the start of the immutable id when a name collides.
fn host_name(name: &str, id: Option<&str>) -> String {
    let mut label = String::new();
    for character in name.chars() {
        let character = character.to_ascii_lowercase();
        if character.is_ascii_lowercase() || character.is_ascii_digit() {
            label.push(character);
        } else if !label.is_empty() && !label.ends_with('-') {
            label.push('-');
        }
    }
    let suffix: String = id
        .map(|id| {
            id.chars()
                .filter(char::is_ascii_hexdigit)
                .take(8)
                .collect::<String>()
                .to_ascii_lowercase()
        })
        .unwrap_or_default();
    label.truncate(62usize.saturating_sub(suffix.len() + 1));
    let label = label.trim_end_matches('-');
    let label = if label.is_empty() { "computer" } else { label };
    if suffix.is_empty() {
        format!("{label}.{DOMAIN}")
    } else {
        format!("{label}-{suffix}.{DOMAIN}")
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
        hyper::header::HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response
}

fn request_host(request: &Request<Incoming>) -> Option<String> {
    let value = request
        .headers()
        .get(HOST)
        .and_then(|host| host.to_str().ok())
        .map(str::to_owned)
        .or_else(|| request.uri().host().map(str::to_owned))?;
    let host = match value.strip_prefix('[') {
        Some(_) => value.as_str(),
        None => value
            .rsplit_once(':')
            .map_or(value.as_str(), |(host, _)| host),
    };
    Some(host.trim_end_matches('.').to_ascii_lowercase())
}

async fn route(
    table: &Mutex<Table>,
    guest_port: u16,
    mut request: Request<Incoming>,
) -> Response<Body> {
    let Some(host) = request_host(&request) else {
        return text(StatusCode::BAD_REQUEST, "The request has no Host header.");
    };
    let upstream = {
        let table = table.lock().unwrap_or_else(|e| e.into_inner());
        match table.hosts.get(&host) {
            None => None,
            Some(entry) => Some(entry.ports.get(&guest_port).copied()),
        }
    };
    let upstream = match upstream {
        None => {
            return text(
                StatusCode::NOT_FOUND,
                &format!("No Silo computer is open at {host}."),
            )
        }
        Some(None) => {
            return text(
                StatusCode::NOT_FOUND,
                &format!("{host} does not publish port {guest_port}."),
            )
        }
        Some(Some(port)) => port,
    };
    let Ok(stream) = TcpStream::connect((Ipv4Addr::LOCALHOST, upstream)).await else {
        return text(
            StatusCode::BAD_GATEWAY,
            &format!("{host} is not answering on port {guest_port}. Is the computer running?"),
        );
    };
    let Ok((mut sender, connection)) =
        hyper::client::conn::http1::handshake(TokioIo::new(stream)).await
    else {
        return text(
            StatusCode::BAD_GATEWAY,
            "The computer closed the connection.",
        );
    };
    tokio::spawn(async move {
        let _ = connection.with_upgrades().await;
    });
    let client_upgrade = hyper::upgrade::on(&mut request);
    let Ok(mut response) = sender.send_request(request.map(|body| body.boxed())).await else {
        return text(
            StatusCode::BAD_GATEWAY,
            "The computer did not answer the request.",
        );
    };
    if response.status() == StatusCode::SWITCHING_PROTOCOLS {
        let upstream_upgrade = hyper::upgrade::on(&mut response);
        tokio::spawn(async move {
            if let (Ok(client), Ok(upstream)) = (client_upgrade.await, upstream_upgrade.await) {
                let mut client = TokioIo::new(client);
                let mut upstream = TokioIo::new(upstream);
                let _ = tokio::io::copy_bidirectional(&mut client, &mut upstream).await;
            }
        });
    }
    response.map(|body| body.boxed())
}

static SHARED: OnceLock<Option<UrlProxy>> = OnceLock::new();

/// The engine's proxy, started on first use. `None` when its runtime could not start.
pub fn shared() -> Option<&'static UrlProxy> {
    SHARED.get_or_init(|| UrlProxy::new(true).ok()).as_ref()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    /// A fake computer: answers every request with its own name, and echoes after a
    /// WebSocket-style upgrade.
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
                    let head = String::from_utf8_lossy(&seen).to_ascii_lowercase();
                    if head.contains("upgrade: websocket") {
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
                    let body = format!("hello from {name}");
                    let _ = write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                });
            }
        });
        port
    }

    fn get(port: u16, host: &str) -> (u16, String) {
        let mut stream = StdStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        write!(
            stream,
            "GET / HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut reply = String::new();
        stream.read_to_string(&mut reply).unwrap();
        let status = reply.split(' ').nth(1).unwrap().parse().unwrap();
        let body = reply
            .split("\r\n\r\n")
            .nth(1)
            .unwrap_or("")
            .trim()
            .to_owned();
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

    fn listen_port(proxy: &UrlProxy, id: &str) -> u16 {
        let url = proxy.urls(id).remove(0).url;
        url.rsplit(':').next().unwrap().parse().unwrap()
    }

    #[test]
    fn two_computers_serving_the_same_guest_port_are_told_apart_by_host() {
        let proxy = UrlProxy::new(false).unwrap();
        proxy.set_routes(&[
            route_for("fork-a", "aaaaaaaa-0000", 3000, upstream("a")),
            route_for("Fork B", "bbbbbbbb-0000", 3000, upstream("b")),
        ]);
        let port = listen_port(&proxy, "aaaaaaaa-0000");
        assert_eq!(port, listen_port(&proxy, "bbbbbbbb-0000"));
        assert_eq!(
            get(port, &format!("fork-a.silo.localhost:{port}")),
            (200, "hello from a".into())
        );
        assert_eq!(
            get(port, &format!("fork-b.silo.localhost:{port}")),
            (200, "hello from b".into())
        );
        assert_eq!(
            proxy.urls("bbbbbbbb-0000")[0].url,
            format!("http://fork-b.silo.localhost:{port}")
        );
    }

    #[test]
    fn unknown_hosts_and_unpublished_ports_are_404_and_stopped_computers_are_502() {
        let proxy = UrlProxy::new(false).unwrap();
        let closed = {
            let listener = StdListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            listener.local_addr().unwrap().port()
        };
        proxy.set_routes(&[
            route_for("web", "11111111", 3000, upstream("web")),
            route_for("web", "11111111", 4000, upstream("web4")),
            route_for("stopped", "22222222", 3000, closed),
        ]);
        let port = proxy.urls("11111111")[0]
            .url
            .rsplit(':')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(get(port, "nobody.silo.localhost").0, 404);
        assert_eq!(get(port, "127.0.0.1").0, 404);
        assert_eq!(get(port, "stopped.silo.localhost").0, 502);
        let other = proxy
            .urls("11111111")
            .into_iter()
            .find(|url| url.guest_port == 4000)
            .unwrap();
        let other: u16 = other.url.rsplit(':').next().unwrap().parse().unwrap();
        assert_eq!(get(other, "stopped.silo.localhost").0, 404);
    }

    #[test]
    fn routes_are_removed_when_a_computer_is_dropped() {
        let proxy = UrlProxy::new(false).unwrap();
        proxy.set_routes(&[
            route_for("one", "11111111", 3000, upstream("one")),
            route_for("two", "22222222", 3000, upstream("two")),
        ]);
        let port = listen_port(&proxy, "11111111");
        proxy.forget_computer("one");
        assert!(proxy.urls("11111111").is_empty());
        assert_eq!(get(port, "one.silo.localhost").0, 404);
        assert_eq!(get(port, "two.silo.localhost").0, 200);
        proxy.set_routes(&[]);
        assert!(proxy.urls("22222222").is_empty());
        std::thread::sleep(Duration::from_millis(100));
        assert!(!answers(port), "the listener closes with its last route");
    }

    #[test]
    fn a_listener_keeps_its_port_across_syncs() {
        let proxy = UrlProxy::new(false).unwrap();
        let routes = [route_for("one", "11111111", 3000, upstream("one"))];
        proxy.set_routes(&routes);
        let before = proxy.urls("11111111");
        proxy.set_routes(&routes);
        assert_eq!(before, proxy.urls("11111111"));
    }

    #[test]
    fn colliding_names_get_an_id_suffix_and_the_guest_port_is_preferred_when_free() {
        let proxy = UrlProxy::new(false).unwrap();
        proxy.set_routes(&[
            route_for("My App", "aaaaaaaa-1", 3000, upstream("x")),
            route_for("my-app", "bbbbbbbb-2", 3000, upstream("y")),
        ]);
        let hosts: Vec<_> = ["aaaaaaaa-1", "bbbbbbbb-2"]
            .iter()
            .map(|id| proxy.urls(id)[0].host.clone())
            .collect();
        assert_ne!(hosts[0], hosts[1]);
        assert!(hosts.iter().all(|host| host.ends_with(".silo.localhost")));

        let busy = StdListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let busy_port = busy.local_addr().unwrap().port();
        let preferring = UrlProxy::new(true).unwrap();
        preferring.set_routes(&[route_for("one", "11111111", busy_port, upstream("one"))]);
        assert_ne!(listen_port(&preferring, "11111111"), busy_port);
    }

    #[test]
    fn websocket_upgrades_pass_through_both_ways() {
        let proxy = UrlProxy::new(false).unwrap();
        proxy.set_routes(&[route_for("hmr", "11111111", 3000, upstream("hmr"))]);
        let port = listen_port(&proxy, "11111111");
        let mut stream = StdStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        write!(
            stream,
            "GET /ws HTTP/1.1\r\nHost: hmr.silo.localhost:{port}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n"
        )
        .unwrap();
        let mut seen = Vec::new();
        let mut byte = [0u8; 1];
        while !seen.ends_with(b"\r\n\r\n") {
            assert_eq!(stream.read(&mut byte).unwrap(), 1);
            seen.push(byte[0]);
        }
        assert!(String::from_utf8_lossy(&seen).starts_with("HTTP/1.1 101"));
        stream.write_all(b"ping-frame").unwrap();
        let mut echoed = [0u8; 10];
        stream.read_exact(&mut echoed).unwrap();
        assert_eq!(&echoed, b"ping-frame");
    }
}
