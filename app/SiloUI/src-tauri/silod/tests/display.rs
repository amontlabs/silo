//! Socket-level test of the display domain: two clients, each with its own viewer connection,
//! a page round trip over the proxy's reserved route, ownership refusals, and cleanup when a
//! client disconnects. No guest runs: each connection is a loopback proxy plus an idle tunnel.

use std::{
    io::{Read, Write},
    net::TcpStream,
    sync::Arc,
};

use serde_json::{json, Value};
use silo_engine::{
    context::{EngineIdentity, EnginePaths, MainThread, PackageKind, ProcessControl},
    desktop_viewer_api::{claim, ensure_owned, finish, OpenPlan},
};
use silod::{
    context::SilodContext,
    dispatcher::Dispatcher,
    framing::{read_json_frame, write_json_frame},
    identity::LocalDeviceIdentity,
    socket::{EngineServer, SystemPeerEuidProvider},
};
use tokio::{
    net::UnixStream,
    sync::oneshot,
    time::{sleep, timeout, Duration},
};

struct NoopMainThread;
impl MainThread for NoopMainThread {
    fn run(&self, _work: Box<dyn FnOnce() + Send>) -> Result<(), String> {
        Ok(())
    }
}

struct NoopProcess;
impl ProcessControl for NoopProcess {
    fn exit(&self, _code: i32) {}

    fn restart(&self) -> ! {
        panic!("restart is not expected in a socket integration test")
    }
}

struct Viewer {
    connection_id: String,
    cookie: String,
    port: u16,
}

/// A healthy viewer connection for `client`, as a finished `display.viewer.open` leaves it.
fn hold_viewer(client: &str, computer: &str) -> Viewer {
    let OpenPlan::Connect {
        connection_id,
        generation,
        ..
    } = claim(client, "fixture-device", computer, "Fixture", true).unwrap()
    else {
        panic!("expected a new connection");
    };
    let proxy = silo_engine::desktop_proxy::Proxy::start(
        "/nonexistent/desktop.sock".into(),
        6901,
        "silo",
        "secret",
    )
    .unwrap();
    let port = proxy.port;
    let mut command = std::process::Command::new("/bin/sleep");
    command.arg("60");
    let tunnel = silo_engine::owned_tunnel::Tunnel::spawn(&command, None).unwrap();
    let opened = finish(client, &connection_id, generation, proxy, tunnel, "Fixture")
        .unwrap_or_else(|(message, _, _)| panic!("{message}"));
    Viewer {
        connection_id,
        cookie: format!("{}={}", opened.cookie_name, opened.cookie_value),
        port,
    }
}

/// The page's answer to the engine: a POST on the proxy's reserved route.
fn answer_page(viewer: &Viewer, route: &str, nonce: &str, kind: &str, body: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", viewer.port)).unwrap();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    let request = format!(
        "POST /__silo/v1/{route}?nonce={nonce}&kind={kind} HTTP/1.1\r\n\
         Host: 127.0.0.1:{port}\r\nCookie: {cookie}\r\n\
         Content-Length: {length}\r\nConnection: close\r\n\r\n{body}",
        port = viewer.port,
        cookie = viewer.cookie,
        length = body.len(),
    );
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);
    response.lines().next().unwrap_or_default().to_owned()
}

struct Client {
    stream: UnixStream,
    id: String,
}

impl Client {
    async fn connect(socket: &std::path::Path, name: &str) -> Self {
        let mut stream = UnixStream::connect(socket).await.unwrap();
        write_json_frame(
            &mut stream,
            &json!({
                "id": "handshake",
                "method": "engine.handshake",
                "params": {
                    "protocol": {"min": 6, "max": 6},
                    "client": {"name": name, "version": "1.0"}
                }
            }),
        )
        .await
        .unwrap();
        let reply = Self::frame(&mut stream).await;
        let id = reply["result"]["clientId"].as_str().unwrap().to_owned();
        Self { stream, id }
    }

    async fn frame(stream: &mut UnixStream) -> Value {
        timeout(Duration::from_secs(10), read_json_frame(stream))
            .await
            .expect("a frame within 10 s")
            .unwrap()
            .expect("the connection stays open")
    }

    async fn send(&mut self, id: &str, method: &str, params: Value) {
        write_json_frame(
            &mut self.stream,
            &json!({"id": id, "method": method, "params": params}),
        )
        .await
        .unwrap();
    }

    /// Reads frames until the reply to `id`, returning it and the events that came first.
    async fn reply(&mut self, id: &str) -> (Value, Vec<Value>) {
        let mut events = Vec::new();
        loop {
            let frame = Self::frame(&mut self.stream).await;
            if frame["id"] == id {
                return (frame, events);
            }
            events.push(frame);
        }
    }

    async fn request(&mut self, id: &str, method: &str, params: Value) -> Value {
        self.send(id, method, params).await;
        self.reply(id).await.0
    }

    /// The next `display.viewer.page` event.
    async fn page_event(&mut self) -> Value {
        loop {
            let frame = Self::frame(&mut self.stream).await;
            if frame["event"] == "display.viewer.page" {
                return frame;
            }
        }
    }
}

/// The method and arguments of a page script such as `window.__silo&&window.__silo.invoke("m",[..]);`.
fn invocation(script: &str) -> (String, Vec<Value>) {
    let call = script
        .strip_prefix("window.__silo&&window.__silo.invoke(")
        .and_then(|rest| rest.strip_suffix(");"))
        .expect("a bridge script");
    let (method, args) = call.split_once(',').unwrap();
    (
        serde_json::from_str(method).unwrap(),
        serde_json::from_str(args).unwrap(),
    )
}

#[tokio::test]
async fn two_clients_keep_separate_viewers_and_a_disconnect_closes_only_its_own() {
    let root = tempfile::tempdir().unwrap();
    let socket = root.path().join("engine.sock");
    let euid = unsafe { libc::geteuid() };
    let context = Arc::new(SilodContext::new(
        EnginePaths {
            home: root.path().join("home"),
            app_data: root.path().join("data"),
            resources: root.path().join("resources"),
            ..EnginePaths::default()
        },
        EngineIdentity {
            channel: silo_engine::channel::Channel::Development,
            version: "0.1.0".into(),
            package_kind: PackageKind::MacApp,
        },
        LocalDeviceIdentity {
            device_id: "fixture-device".into(),
            name: "Fixture Mac".into(),
        },
        Arc::new(NoopMainThread),
        Arc::new(NoopProcess),
    ));
    let server = EngineServer::bind(
        socket.clone(),
        euid,
        Arc::new(SystemPeerEuidProvider),
        Arc::new(Dispatcher::core(context)),
    )
    .unwrap();
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let server_task = tokio::spawn(server.run(shutdown_rx));

    let mut first = Client::connect(&socket, "first").await;
    let mut second = Client::connect(&socket, "second").await;
    assert_ne!(first.id, second.id);

    // Both clients view the same computer, each through its own proxy.
    let computer = uuid::Uuid::new_v4().to_string();
    let viewer_a = hold_viewer(&first.id, &computer);
    let viewer_b = hold_viewer(&second.id, &computer);
    assert_ne!(viewer_a.connection_id, viewer_b.connection_id);
    assert_ne!(viewer_a.port, viewer_b.port);

    let subscribed = first
        .request(
            "subscribe",
            "events.subscribe",
            json!({"events": ["display.viewer.page"]}),
        )
        .await;
    assert!(
        subscribed["result"]["subscription"].is_string(),
        "{subscribed}"
    );

    // The sound check reaches the page as a client-addressed event; the page answers over HTTP.
    first
        .send(
            "sound",
            "display.viewer.sound.check",
            json!({"connectionId": viewer_a.connection_id}),
        )
        .await;
    let event = first.page_event().await;
    assert_eq!(event["payload"]["connectionId"], viewer_a.connection_id);
    let (method, args) = invocation(event["payload"]["script"].as_str().unwrap());
    assert_eq!(method, "capabilities");
    let status = tokio::task::spawn_blocking({
        let (cookie, port) = (viewer_a.cookie.clone(), viewer_a.port);
        let nonce = args[0].as_str().unwrap().to_owned();
        move || {
            answer_page(
                &Viewer {
                    connection_id: String::new(),
                    cookie,
                    port,
                },
                "capabilities",
                &nonce,
                "application%2Fjson",
                r#"{"transport":true,"opus":true,"audioDecoder":true}"#,
            )
        }
    })
    .await
    .unwrap();
    assert!(status.contains("204"), "{status}");
    let (sound, _) = first.reply("sound").await;
    assert_eq!(sound["result"], json!({"sound": true}), "{sound}");

    // A screen reset waits for the page's acknowledgement.
    first
        .send(
            "reset",
            "display.viewer.screen.reset",
            json!({"connectionId": viewer_a.connection_id}),
        )
        .await;
    let event = first.page_event().await;
    let (method, args) = invocation(event["payload"]["script"].as_str().unwrap());
    assert_eq!(method, "resetScreen");
    let status = tokio::task::spawn_blocking({
        let (cookie, port) = (viewer_a.cookie.clone(), viewer_a.port);
        let nonce = args[1].as_str().unwrap().to_owned();
        move || {
            answer_page(
                &Viewer {
                    connection_id: String::new(),
                    cookie,
                    port,
                },
                "sent",
                &nonce,
                "ok",
                "",
            )
        }
    })
    .await
    .unwrap();
    assert!(status.contains("204"), "{status}");
    let (reset, _) = first.reply("reset").await;
    assert!(
        reset["result"].is_null() && reset.get("error").is_none(),
        "{reset}"
    );

    // Another client can neither use nor close this client's connection.
    let refused = second
        .request(
            "foreign-close",
            "display.viewer.close",
            json!({"connectionId": viewer_a.connection_id}),
        )
        .await;
    assert_eq!(refused["error"]["code"], "not_found", "{refused}");
    let refused = second
        .request(
            "foreign-audio",
            "display.viewer.audio.set",
            json!({"connectionId": viewer_a.connection_id, "muted": true, "active": true, "revision": 1}),
        )
        .await;
    assert_eq!(refused["error"]["code"], "not_found", "{refused}");
    assert!(ensure_owned(&first.id, &viewer_a.connection_id).is_ok());

    // A client that disconnects loses its own connection and nobody else's.
    let second_id = second.id.clone();
    drop(second);
    timeout(Duration::from_secs(5), async {
        while ensure_owned(&second_id, &viewer_b.connection_id).is_ok() {
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the disconnected client's viewer is closed");
    assert!(ensure_owned(&first.id, &viewer_a.connection_id).is_ok());
    // An open that was still reading credentials when the client left reaches its claim
    // after the disconnect: it gets no connection, so no proxy or tunnel outlives the client.
    assert!(claim(&second_id, "fixture-device", &computer, "Fixture", true).is_err());

    // A connection id cannot be used for a clipboard transfer to another computer.
    let other_computer = uuid::Uuid::new_v4().to_string();
    let refused = first
        .request(
            "clipboard-target",
            "clipboard.transfer",
            json!({
                "computerId": other_computer,
                "action": "copy",
                "connectionId": viewer_a.connection_id,
            }),
        )
        .await;
    assert_eq!(refused["error"]["code"], "not_found", "{refused}");

    // Closing one's own connection works once; afterwards it is unknown.
    let closed = first
        .request(
            "close",
            "display.viewer.close",
            json!({"connectionId": viewer_a.connection_id}),
        )
        .await;
    assert!(
        closed["result"].is_null() && closed.get("error").is_none(),
        "{closed}"
    );
    let again = first
        .request(
            "close-again",
            "display.viewer.close",
            json!({"connectionId": viewer_a.connection_id}),
        )
        .await;
    assert_eq!(again["error"]["code"], "not_found", "{again}");

    drop(first);
    let _ = shutdown_tx.send(());
    server_task.await.unwrap();
}
