use std::{
    collections::HashMap,
    fmt, io,
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use serde_json::{json, Value};
use silo_api::jobs::ErrorCode;
use silo_api::transport::{
    ClientMetadata, ErrorObject, HandshakeParams, HandshakeResult, ProtocolRange, RequestEnvelope,
    ResponseEnvelope,
};
use tokio::{
    io::{AsyncRead, AsyncWrite, AsyncWriteExt, WriteHalf},
    sync::{mpsc, oneshot},
    time,
};

use crate::codec::{read_frame, write_frame};

const HANDSHAKE_ID: &str = "handshake";

/// Waits before each retry of a query deferred with `update_in_progress`.
const QUERY_RETRY_DELAYS: &[Duration] = &[
    Duration::from_millis(100),
    Duration::from_millis(250),
    Duration::from_millis(500),
    Duration::from_millis(1000),
];

/// What this client announces and the protocol range it can speak.
#[derive(Clone)]
pub struct ConnectOptions {
    pub client: ClientMetadata,
    pub protocol: ProtocolRange,
    pub handshake_timeout: Duration,
    /// The computer's token, sent in the handshake on the channel inside a computer.
    pub token: Option<String>,
}

impl fmt::Debug for ConnectOptions {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConnectOptions")
            .field("client", &self.client)
            .field("protocol", &self.protocol)
            .field("handshake_timeout", &self.handshake_timeout)
            .field("token", &self.token.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

impl ConnectOptions {
    /// Offers the single protocol version `silo-api` defines.
    pub fn new(client: ClientMetadata) -> Self {
        Self {
            client,
            protocol: ProtocolRange {
                min: silo_api::transport::PROTOCOL_VERSION,
                max: silo_api::transport::PROTOCOL_VERSION,
            },
            handshake_timeout: Duration::from_secs(10),
            token: None,
        }
    }
}

/// Admission fields for a Change method.
#[derive(Clone, Debug, Default)]
pub struct RequestOptions {
    pub operation_id: Option<String>,
    pub start_within_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ClientError {
    /// The socket could not be reached or failed.
    Io(String),
    /// The engine sent something that is not the protocol.
    Protocol(String),
    /// The engine answered with a typed error.
    Remote(ErrorObject),
    /// The connection ended before the reply.
    Disconnected,
    /// The reply did not arrive in time.
    TimedOut,
}

impl fmt::Display for ClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(message) => write!(formatter, "The engine connection failed: {message}"),
            Self::Protocol(message) => {
                write!(formatter, "The engine broke the protocol: {message}")
            }
            Self::Remote(error) => formatter.write_str(&error.message),
            Self::Disconnected => formatter.write_str("The engine connection ended."),
            Self::TimedOut => formatter.write_str("The engine did not answer in time."),
        }
    }
}

impl std::error::Error for ClientError {}

impl From<io::Error> for ClientError {
    fn from(error: io::Error) -> Self {
        match error.kind() {
            io::ErrorKind::InvalidData => Self::Protocol(error.to_string()),
            _ => Self::Io(error.to_string()),
        }
    }
}

/// The negotiated session.
pub type Handshake = HandshakeResult;

type Reply = Result<Value, ErrorObject>;

struct Inner<W> {
    writer: tokio::sync::Mutex<W>,
    pending: Mutex<HashMap<String, oneshot::Sender<Reply>>>,
    closed: AtomicBool,
    sent: AtomicU64,
    handshake: Handshake,
}

/// A connected, handshaken session. Cloning shares the connection.
pub struct Client<W = WriteHalf<Stream>> {
    inner: Arc<Inner<W>>,
}

impl<W> Clone for Client<W> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

/// Event frames in arrival order. `next` returns `None` once the connection has ended.
pub struct EventFeed {
    receiver: mpsc::UnboundedReceiver<Value>,
}

impl EventFeed {
    pub async fn next(&mut self) -> Option<Value> {
        self.receiver.recv().await
    }
}

/// The connection a [`Client`] runs on: `engine.sock`, or a computer's channel.
pub enum Stream {
    Unix(tokio::net::UnixStream),
    Tcp(tokio::net::TcpStream),
}

impl AsyncRead for Stream {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
        buffer: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Unix(stream) => std::pin::Pin::new(stream).poll_read(context, buffer),
            Self::Tcp(stream) => std::pin::Pin::new(stream).poll_read(context, buffer),
        }
    }
}

impl AsyncWrite for Stream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
        buffer: &[u8],
    ) -> std::task::Poll<io::Result<usize>> {
        match self.get_mut() {
            Self::Unix(stream) => std::pin::Pin::new(stream).poll_write(context, buffer),
            Self::Tcp(stream) => std::pin::Pin::new(stream).poll_write(context, buffer),
        }
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Unix(stream) => std::pin::Pin::new(stream).poll_flush(context),
            Self::Tcp(stream) => std::pin::Pin::new(stream).poll_flush(context),
        }
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Unix(stream) => std::pin::Pin::new(stream).poll_shutdown(context),
            Self::Tcp(stream) => std::pin::Pin::new(stream).poll_shutdown(context),
        }
    }
}

impl Client {
    /// Connects to `engine.sock` and negotiates the protocol.
    pub async fn connect(
        socket: &Path,
        options: ConnectOptions,
    ) -> Result<(Self, EventFeed), ClientError> {
        let stream = tokio::net::UnixStream::connect(socket).await?;
        establish(Stream::Unix(stream), options).await
    }

    /// Connects to a computer's channel at `host:port` and negotiates the protocol with the
    /// token `options` carries.
    pub async fn connect_channel(
        host: &str,
        port: u16,
        options: ConnectOptions,
    ) -> Result<(Self, EventFeed), ClientError> {
        if options.token.is_none() {
            return Err(ClientError::Protocol(
                "a computer's channel needs its token".to_owned(),
            ));
        }
        let stream = time::timeout(
            options.handshake_timeout,
            tokio::net::TcpStream::connect((host, port)),
        )
        .await
        .map_err(|_| ClientError::TimedOut)??;
        let _ = stream.set_nodelay(true);
        establish(Stream::Tcp(stream), options).await
    }
}

impl<W> Client<W>
where
    W: AsyncWrite + Unpin + Send + 'static,
{
    pub fn handshake(&self) -> &Handshake {
        &self.inner.handshake
    }

    /// Requests issued on this connection. The engine refuses new ids after 65,536, so
    /// callers reconnect well before that.
    pub fn requests_sent(&self) -> u64 {
        self.inner.sent.load(Ordering::SeqCst)
    }

    pub fn is_closed(&self) -> bool {
        self.inner.closed.load(Ordering::SeqCst)
    }

    /// Whether the engine advertised `method` during the handshake.
    pub fn supports(&self, method: &str) -> bool {
        self.inner
            .handshake
            .capabilities
            .iter()
            .any(|capability| capability == method)
    }

    pub async fn request(&self, method: &str, params: Value) -> Result<Value, ClientError> {
        self.request_with(method, params, RequestOptions::default())
            .await
    }

    /// Sends one request and resolves with its reply by id, whatever else arrives meanwhile.
    /// A query the engine defers with `update_in_progress` is retried with a short backoff;
    /// commands and jobs are sent once.
    pub async fn request_with(
        &self,
        method: &str,
        params: Value,
        options: RequestOptions,
    ) -> Result<Value, ClientError> {
        let is_query = silo_api::methods::method_registry()
            .iter()
            .any(|descriptor| {
                descriptor.name == method && descriptor.kind == silo_api::methods::MethodKind::Query
            });
        let mut delays = if is_query { QUERY_RETRY_DELAYS } else { &[] }.iter();
        loop {
            let result = self
                .request_once(method, params.clone(), options.clone())
                .await;
            let deferred = matches!(
                &result,
                Err(ClientError::Remote(error)) if error.code == ErrorCode::UpdateInProgress
            );
            match delays.next() {
                Some(delay) if deferred => time::sleep(*delay).await,
                _ => return result,
            }
        }
    }

    async fn request_once(
        &self,
        method: &str,
        params: Value,
        options: RequestOptions,
    ) -> Result<Value, ClientError> {
        if self.is_closed() {
            return Err(ClientError::Disconnected);
        }
        let id = uuid::Uuid::new_v4().to_string();
        self.inner.sent.fetch_add(1, Ordering::SeqCst);
        let mut frame = json!({"id": id, "method": method, "params": params});
        if let Some(operation_id) = options.operation_id {
            frame["operationId"] = Value::String(operation_id);
        }
        if let Some(start_within_ms) = options.start_within_ms {
            frame["startWithinMs"] = json!(start_within_ms);
        }
        let (sender, receiver) = oneshot::channel();
        self.inner
            .pending
            .lock()
            .unwrap()
            .insert(id.clone(), sender);
        let written = {
            let mut writer = self.inner.writer.lock().await;
            write_frame(&mut *writer, &frame).await
        };
        if let Err(error) = written {
            self.inner.pending.lock().unwrap().remove(&id);
            self.close().await;
            return Err(error.into());
        }
        match receiver.await {
            Ok(Ok(result)) => Ok(result),
            Ok(Err(error)) => Err(ClientError::Remote(error)),
            Err(_) => Err(ClientError::Disconnected),
        }
    }

    /// Ends the session. Pending requests fail with `Disconnected`.
    pub async fn close(&self) {
        fail_all(&self.inner);
        let mut writer = self.inner.writer.lock().await;
        let _ = writer.shutdown().await;
    }
}

/// Runs the handshake over any duplex stream, then starts the reader task.
pub async fn establish<S>(
    stream: S,
    options: ConnectOptions,
) -> Result<(Client<WriteHalf<S>>, EventFeed), ClientError>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (mut reader, mut writer) = tokio::io::split(stream);
    let request = RequestEnvelope {
        id: HANDSHAKE_ID.to_owned(),
        method: silo_api::methods::MethodName::EngineHandshake,
        params: HandshakeParams {
            ssh_key: None,
            protocol: options.protocol,
            client: Some(options.client.clone()),
            token: options.token.clone(),
        },
        operation_id: None,
        start_within_ms: None,
    };
    let exchange = async {
        write_frame(&mut writer, &request).await?;
        // The reply is the first frame: events cannot precede a subscription.
        let frame: Value = read_frame(&mut reader)
            .await?
            .ok_or(ClientError::Disconnected)?;
        Ok::<_, ClientError>(frame)
    };
    let frame = time::timeout(options.handshake_timeout, exchange)
        .await
        .map_err(|_| ClientError::TimedOut)??;
    let handshake = parse_handshake(frame, options.protocol)?;
    let inner = Arc::new(Inner {
        writer: tokio::sync::Mutex::new(writer),
        pending: Mutex::new(HashMap::new()),
        closed: AtomicBool::new(false),
        sent: AtomicU64::new(0),
        handshake,
    });
    let (events, receiver) = mpsc::unbounded_channel();
    tokio::spawn(read_loop(reader, inner.clone(), events));
    Ok((Client { inner }, EventFeed { receiver }))
}

fn parse_handshake(frame: Value, offered: ProtocolRange) -> Result<Handshake, ClientError> {
    let response: ResponseEnvelope<Value> = serde_json::from_value(frame)
        .map_err(|error| ClientError::Protocol(format!("malformed handshake reply: {error}")))?;
    match response {
        ResponseEnvelope::Error { id, error } if id == HANDSHAKE_ID => {
            Err(ClientError::Remote(error))
        }
        ResponseEnvelope::Success { id, result } if id == HANDSHAKE_ID => {
            let handshake: Handshake = serde_json::from_value(result).map_err(|error| {
                ClientError::Protocol(format!("malformed handshake result: {error}"))
            })?;
            if handshake.protocol < offered.min || handshake.protocol > offered.max {
                return Err(ClientError::Protocol(format!(
                    "the engine chose protocol {} outside {}-{}",
                    handshake.protocol, offered.min, offered.max
                )));
            }
            if handshake.client_id.is_none() {
                return Err(ClientError::Protocol(
                    "the engine issued no client identity".to_owned(),
                ));
            }
            Ok(handshake)
        }
        _ => Err(ClientError::Protocol(
            "the first frame was not the handshake reply".to_owned(),
        )),
    }
}

fn fail_all<W>(inner: &Inner<W>) {
    inner.closed.store(true, Ordering::SeqCst);
    // Dropping the senders resolves every waiter with `Disconnected`.
    inner.pending.lock().unwrap().clear();
}

async fn read_loop<R, W>(mut reader: R, inner: Arc<Inner<W>>, events: mpsc::UnboundedSender<Value>)
where
    R: AsyncRead + Unpin,
{
    loop {
        let frame: Value = match read_frame(&mut reader).await {
            Ok(Some(frame)) => frame,
            Ok(None) | Err(_) => break,
        };
        if frame.get("event").and_then(Value::as_str).is_some() {
            if events.send(frame).is_err() {
                // Nobody listens; keep serving replies.
            }
            continue;
        }
        match serde_json::from_value::<ResponseEnvelope<Value>>(frame) {
            Ok(ResponseEnvelope::Success { id, result }) => resolve(&inner, &id, Ok(result)),
            Ok(ResponseEnvelope::Error { id, error }) => resolve(&inner, &id, Err(error)),
            // Neither an event nor a response: the stream cannot be trusted.
            Err(_) => break,
        }
    }
    fail_all(&inner);
}

fn resolve<W>(inner: &Inner<W>, id: &str, reply: Reply) {
    if let Some(sender) = inner.pending.lock().unwrap().remove(id) {
        let _ = sender.send(reply);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use tokio::io::duplex;

    fn options() -> ConnectOptions {
        ConnectOptions::new(ClientMetadata {
            name: "test".into(),
            version: "0".into(),
        })
    }

    fn handshake_reply(protocol: u32) -> Value {
        json!({"id": "handshake", "result": {
            "deviceId": "dev", "name": "Mac", "protocol": protocol,
            "capabilities": ["computers.list"], "clientId": "c1"}})
    }

    /// A server that answers the handshake and then runs `script` on the stream.
    async fn serve<F, Fut>(reply: Value, script: F) -> tokio::io::DuplexStream
    where
        F: FnOnce(tokio::io::DuplexStream) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let (client, mut server) = duplex(64 * 1024);
        tokio::spawn(async move {
            let request: Value = read_frame(&mut server).await.unwrap().unwrap();
            assert_eq!(request["method"], "engine.handshake");
            assert!(request["params"].get("token").is_none());
            assert_eq!(
                request["params"]["protocol"]["min"],
                silo_api::transport::PROTOCOL_VERSION
            );
            write_frame(&mut server, &reply).await.unwrap();
            script(server).await;
        });
        client
    }

    #[tokio::test]
    async fn negotiates_and_exposes_capabilities_and_client_id() {
        let stream = serve(handshake_reply(8), |_server| async {
            tokio::time::sleep(Duration::from_millis(50)).await;
        })
        .await;
        let (client, _events) = establish(stream, options()).await.unwrap();
        assert_eq!(client.handshake().client_id.as_deref(), Some("c1"));
        assert!(client.supports("computers.list"));
        assert!(!client.supports("computers.start"));
    }

    #[tokio::test]
    async fn refuses_a_protocol_outside_the_offered_range_and_typed_refusals() {
        let stream = serve(handshake_reply(5), |_s| async {}).await;
        assert!(matches!(
            establish(stream, options()).await,
            Err(ClientError::Protocol(_))
        ));
        let refusal = json!({"id": "handshake", "error": {
            "code": "incompatible_version", "message": "Update Silo.",
            "diagnostics": {"engineProtocolRange": {"min": 8, "max": 9},
                "clientProtocolRange": {"min": 8, "max": 8}, "engineAppVersion": "9"}}});
        let stream = serve(refusal, |_s| async {}).await;
        match establish(stream, options()).await {
            Err(ClientError::Remote(error)) => {
                assert_eq!(error.code, silo_api::jobs::ErrorCode::IncompatibleVersion);
                assert_eq!(error.diagnostics.unwrap().engine_app_version, "9");
            }
            other => panic!("unexpected {:?}", other.map(|_| ())),
        }
    }

    #[tokio::test]
    async fn a_malformed_handshake_reply_is_a_protocol_error() {
        let stream = serve(json!({"hello": 1}), |_s| async {}).await;
        assert!(matches!(
            establish(stream, options()).await,
            Err(ClientError::Protocol(_))
        ));
    }

    #[tokio::test]
    async fn correlates_replies_by_id_with_events_interleaved_and_out_of_order() {
        let stream = serve(handshake_reply(8), |mut server| async move {
            let first: Value = read_frame(&mut server).await.unwrap().unwrap();
            let second: Value = read_frame(&mut server).await.unwrap().unwrap();
            write_frame(
                &mut server,
                &json!({"event": "computers.changed", "subscription": "s", "deviceId": "dev", "seq": 1, "payload": {}}),
            )
            .await
            .unwrap();
            // Answer the second request first.
            write_frame(&mut server, &json!({"id": second["id"], "result": {"n": 2}}))
                .await
                .unwrap();
            write_frame(
                &mut server,
                &json!({"id": first["id"], "error": {"code": "not_found", "message": "gone"}}),
            )
            .await
            .unwrap();
            tokio::time::sleep(Duration::from_millis(100)).await;
        })
        .await;
        let (client, mut events) = establish(stream, options()).await.unwrap();
        let (a, b) = tokio::join!(client.request("computers.list", json!({})), async {
            tokio::time::sleep(Duration::from_millis(10)).await;
            client.request("computers.list", json!({"x": 1})).await
        });
        match a {
            Err(ClientError::Remote(error)) => assert_eq!(error.message, "gone"),
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(b.unwrap(), json!({"n": 2}));
        let event = events.next().await.unwrap();
        assert_eq!(event["event"], "computers.changed");
        assert_eq!(event["seq"], 1);
    }

    #[tokio::test]
    async fn sends_operation_admission_fields() {
        let stream = serve(handshake_reply(8), |mut server| async move {
            let request: Value = read_frame(&mut server).await.unwrap().unwrap();
            assert_eq!(request["operationId"], "op-1");
            assert_eq!(request["startWithinMs"], 500);
            write_frame(&mut server, &json!({"id": request["id"], "result": null}))
                .await
                .unwrap();
        })
        .await;
        let (client, _events) = establish(stream, options()).await.unwrap();
        let result = client
            .request_with(
                "computers.start",
                json!({}),
                RequestOptions {
                    operation_id: Some("op-1".into()),
                    start_within_ms: Some(500),
                },
            )
            .await
            .unwrap();
        assert_eq!(result, Value::Null);
    }

    async fn serve_updating(
        method: &'static str,
        deferred: usize,
    ) -> (Client<WriteHalf<tokio::io::DuplexStream>>, Arc<AtomicUsize>) {
        let seen = Arc::new(AtomicUsize::new(0));
        let counted = seen.clone();
        let stream = serve(handshake_reply(8), move |mut server| async move {
            while let Ok(Some(request)) = read_frame::<_, Value>(&mut server).await {
                assert_eq!(request["method"], method);
                let attempt = counted.fetch_add(1, Ordering::SeqCst);
                let reply = if attempt < deferred {
                    json!({"id": request["id"], "error": {
                        "code": "update_in_progress", "message": "updating"}})
                } else {
                    json!({"id": request["id"], "result": {"ok": true}})
                };
                write_frame(&mut server, &reply).await.unwrap();
            }
        })
        .await;
        let (client, _events) = establish(stream, options()).await.unwrap();
        (client, seen)
    }

    #[tokio::test]
    async fn a_query_deferred_with_update_in_progress_is_retried() {
        let (client, seen) = serve_updating("computers.list", 2).await;
        let result = client.request("computers.list", json!({})).await.unwrap();
        assert_eq!(result, json!({"ok": true}));
        assert_eq!(seen.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn a_query_that_stays_deferred_gives_up_with_the_engine_error() {
        let (client, seen) = serve_updating("computers.list", usize::MAX).await;
        let error = client
            .request("computers.list", json!({}))
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            ClientError::Remote(error) if error.code == ErrorCode::UpdateInProgress
        ));
        assert_eq!(seen.load(Ordering::SeqCst), QUERY_RETRY_DELAYS.len() + 1);
    }

    #[tokio::test]
    async fn a_mutation_deferred_with_update_in_progress_is_not_retried() {
        let (client, seen) = serve_updating("computers.start", usize::MAX).await;
        let error = client
            .request("computers.start", json!({}))
            .await
            .unwrap_err();
        assert!(matches!(error, ClientError::Remote(_)));
        assert_eq!(seen.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn connection_loss_fails_pending_requests_and_ends_the_feed() {
        let stream = serve(handshake_reply(8), |mut server| async move {
            let _: Value = read_frame(&mut server).await.unwrap().unwrap();
            // Drop without replying.
        })
        .await;
        let (client, mut events) = establish(stream, options()).await.unwrap();
        assert_eq!(
            client.request("computers.list", json!({})).await,
            Err(ClientError::Disconnected)
        );
        assert!(events.next().await.is_none());
        assert!(client.is_closed());
        assert_eq!(
            client.request("computers.list", json!({})).await,
            Err(ClientError::Disconnected)
        );
    }

    #[tokio::test]
    async fn a_frame_that_is_neither_event_nor_reply_ends_the_session() {
        let stream = serve(handshake_reply(8), |mut server| async move {
            let _: Value = read_frame(&mut server).await.unwrap().unwrap();
            write_frame(&mut server, &json!({"surprise": true}))
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_millis(200)).await;
        })
        .await;
        let (client, _events) = establish(stream, options()).await.unwrap();
        assert_eq!(
            client.request("computers.list", json!({})).await,
            Err(ClientError::Disconnected)
        );
    }

    #[tokio::test]
    async fn connects_over_a_unix_socket() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("engine.sock");
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let _: Value = read_frame(&mut stream).await.unwrap().unwrap();
            write_frame(&mut stream, &handshake_reply(8)).await.unwrap();
            tokio::time::sleep(Duration::from_millis(100)).await;
        });
        let (client, _events) = Client::connect(&path, options()).await.unwrap();
        assert_eq!(client.handshake().device_id, "dev");
        let missing = directory.path().join("missing.sock");
        assert!(matches!(
            Client::connect(&missing, options()).await,
            Err(ClientError::Io(_))
        ));
    }

    #[tokio::test]
    async fn a_channel_connection_sends_the_token_over_tcp_and_needs_one() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request: Value = read_frame(&mut stream).await.unwrap().unwrap();
            write_frame(&mut stream, &handshake_reply(8)).await.unwrap();
            tokio::time::sleep(Duration::from_millis(100)).await;
            request
        });
        let mut with_token = options();
        with_token.token = Some("silo_gc_abc".into());
        assert!(!format!("{with_token:?}").contains("silo_gc_abc"));
        let (client, _events) = Client::connect_channel("127.0.0.1", port, with_token)
            .await
            .unwrap();
        assert_eq!(client.handshake().client_id.as_deref(), Some("c1"));
        let request = seen.await.unwrap();
        assert_eq!(request["params"]["token"], "silo_gc_abc");
        assert!(matches!(
            Client::connect_channel("127.0.0.1", port, options()).await,
            Err(ClientError::Protocol(_))
        ));
    }

    #[tokio::test]
    async fn engine_sock_handshakes_carry_no_token() {
        let stream = serve(handshake_reply(8), |_server| async {}).await;
        let (client, _events) = establish(stream, options()).await.unwrap();
        assert_eq!(client.handshake().protocol, 8);
    }
}
