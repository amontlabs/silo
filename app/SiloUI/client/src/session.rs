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
use silo_api::transport::{
    ClientMetadata, ErrorObject, HandshakeParams, HandshakeResult, ProtocolRange,
    RequestEnvelope, ResponseEnvelope,
};
use tokio::{
    io::{AsyncRead, AsyncWrite, AsyncWriteExt, WriteHalf},
    sync::{mpsc, oneshot},
    time,
};

use crate::codec::{read_frame, write_frame};

const HANDSHAKE_ID: &str = "handshake";

/// What this client announces and the protocol range it can speak.
#[derive(Clone, Debug)]
pub struct ConnectOptions {
    pub client: ClientMetadata,
    pub protocol: ProtocolRange,
    pub handshake_timeout: Duration,
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
            Self::Protocol(message) => write!(formatter, "The engine broke the protocol: {message}"),
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
pub struct Client<W = WriteHalf<tokio::net::UnixStream>> {
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

impl Client {
    /// Connects to `engine.sock` and negotiates the protocol.
    pub async fn connect(
        socket: &Path,
        options: ConnectOptions,
    ) -> Result<(Self, EventFeed), ClientError> {
        let stream = tokio::net::UnixStream::connect(socket).await?;
        establish(stream, options).await
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
    pub async fn request_with(
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
        self.inner.pending.lock().unwrap().insert(id.clone(), sender);
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
        ResponseEnvelope::Error { id, error } if id == HANDSHAKE_ID => Err(ClientError::Remote(error)),
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
        let stream = serve(handshake_reply(6), |_server| async {
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
            "diagnostics": {"engineProtocolRange": {"min": 7, "max": 8},
                "clientProtocolRange": {"min": 6, "max": 6}, "engineAppVersion": "9"}}});
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
        let stream = serve(handshake_reply(6), |mut server| async move {
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
        let stream = serve(handshake_reply(6), |mut server| async move {
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

    #[tokio::test]
    async fn connection_loss_fails_pending_requests_and_ends_the_feed() {
        let stream = serve(handshake_reply(6), |mut server| async move {
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
        let stream = serve(handshake_reply(6), |mut server| async move {
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
            write_frame(&mut stream, &handshake_reply(6)).await.unwrap();
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
}
