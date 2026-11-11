//! The channel inside a computer: for each computer with Silo's tools on, a TCP listener on the
//! device's loopback (`127.0.0.1` only) serving the ordinary engine protocol, with every request
//! made as that computer (`Caller::Computer`). The computer's network allows exactly its own
//! port (`silo_engine::network_policy::extra_rules`); the device side always sees `127.0.0.1`, so
//! the token is the identity (`docs/SiloUI-IN-GUEST-CLIENT.md`).
//!
//! The order of checks on a connection, all before anything is answered:
//!
//! 1. The connection takes one of the computer's connection places, or is closed at once.
//! 2. The first frame is read within [`HANDSHAKE_TIMEOUT`] and at most [`MAX_HANDSHAKE_BYTES`].
//! 3. It must be `engine.handshake` with a `token` that hashes to the stored hash of the computer
//!    the port belongs to. The computer comes from the port, never from the frame.
//! 4. Anything else is recorded in the audit log and the connection is closed after
//!    [`REFUSAL_DELAY`], with no answer.
//! 5. Only then is the session served, and it ends when the tools are turned off.

mod watch;

use std::{
    collections::HashMap,
    net::TcpListener as StdTcpListener,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use serde_json::Value;
use silo_api::methods::audit::{AuditCallerKind, AuditOutcome};
use silo_engine::{
    audit::{AuditLog, Row},
    context::EngineContext,
    guest_client::TokenStore,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    net::{TcpListener, TcpStream},
    sync::{watch as signal, OwnedSemaphorePermit, Semaphore},
    time,
};

use crate::{
    context::SilodContext,
    dispatcher::Dispatcher,
    socket::{serve_connection, ComputerSession},
};

pub use watch::start;

/// The most connections one computer may hold open at once.
pub const MAX_CONNECTIONS: usize = 8;
/// The most requests one computer may have running at once, over all its connections.
const MAX_REQUESTS: usize = 16;
/// Requests kept for `engine.state` when the others are taken.
const CONTROL_REQUESTS: usize = 2;
/// How long a new connection has to send its handshake.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// The largest handshake frame read before the token is checked.
pub const MAX_HANDSHAKE_BYTES: usize = 16 * 1024;
/// How long a refused connection is held before it is closed.
pub const REFUSAL_DELAY: Duration = Duration::from_secs(1);

/// Why a connection was not served.
#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The connection ended before it sent anything; nothing is recorded.
    Silent,
    /// It sent something that is not a handshake carrying this computer's token.
    BadHandshake,
}

/// Checks the first frame of a connection on `computer_id`'s port: an `engine.handshake` whose
/// `token` `verify` accepts for that computer. Nothing else in the frame selects the computer.
pub fn check_handshake(
    frame: &Value,
    computer_id: &str,
    verify: &dyn Fn(&str, &str) -> bool,
) -> Result<(), Refusal> {
    let handshake = frame.get("method").and_then(Value::as_str) == Some("engine.handshake");
    let token = frame.pointer("/params/token").and_then(Value::as_str);
    match token {
        Some(token) if handshake && verify(token, computer_id) => Ok(()),
        _ => Err(Refusal::BadHandshake),
    }
}

/// Reads one length-prefixed JSON frame of at most `limit` bytes. `Ok(None)` when the peer
/// closed before the first byte.
async fn read_limited<R: AsyncRead + Unpin>(
    reader: &mut R,
    limit: usize,
) -> std::io::Result<Option<Value>> {
    let mut header = [0_u8; 4];
    if reader.read(&mut header[..1]).await? == 0 {
        return Ok(None);
    }
    reader.read_exact(&mut header[1..]).await?;
    let length = u32::from_be_bytes(header) as usize;
    if length == 0 || length > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "handshake frame length is outside the allowed range",
        ));
    }
    let mut payload = vec![0_u8; length];
    reader.read_exact(&mut payload).await?;
    serde_json::from_slice(&payload)
        .map(Some)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

/// What one computer's listener needs.
#[derive(Clone)]
struct Served {
    computer_id: String,
    dispatcher: Arc<Dispatcher>,
    verify: Arc<dyn Fn(&str, &str) -> bool + Send + Sync>,
    audit: Option<PathBuf>,
    connections: Arc<Semaphore>,
    requests: Arc<Semaphore>,
    control: Arc<Semaphore>,
    closed: signal::Receiver<bool>,
}

impl Served {
    fn refused(&self) {
        let Some(app_data) = &self.audit else {
            return;
        };
        AuditLog::shared(app_data).record(Row {
            at_ms: silo_engine::audit::now_ms(),
            caller_kind: AuditCallerKind::Computer,
            caller_id: Some(self.computer_id.clone()),
            client: None,
            method: "engine.handshake".into(),
            computer_id: None,
            outcome: AuditOutcome::Refused,
            error_code: Some("permission_denied".into()),
            job_id: None,
            approval_id: None,
        });
    }

    async fn accept(self, listener: TcpListener) {
        let mut closed = self.closed.clone();
        loop {
            if *closed.borrow_and_update() {
                return;
            }
            tokio::select! {
                changed = closed.changed() => {
                    if changed.is_err() {
                        return;
                    }
                }
                accepted = listener.accept() => {
                    let stream = match accepted {
                        Ok((stream, _)) => stream,
                        Err(error) => {
                            eprintln!("silod: channel of {}: accept failed: {error}", self.computer_id);
                            time::sleep(Duration::from_millis(100)).await;
                            continue;
                        }
                    };
                    let Ok(permit) = self.connections.clone().try_acquire_owned() else {
                        drop(stream);
                        continue;
                    };
                    tokio::spawn(self.clone().connection(stream, permit));
                }
            }
        }
    }

    async fn connection(self, mut stream: TcpStream, permit: OwnedSemaphorePermit) {
        let _ = stream.set_nodelay(true);
        let first = time::timeout(
            HANDSHAKE_TIMEOUT,
            read_limited(&mut stream, MAX_HANDSHAKE_BYTES),
        )
        .await;
        let verdict = match &first {
            Ok(Ok(None)) => Err(Refusal::Silent),
            Ok(Ok(Some(frame))) => check_handshake(frame, &self.computer_id, &*self.verify),
            Ok(Err(_)) | Err(_) => Err(Refusal::BadHandshake),
        };
        match (verdict, first) {
            (Ok(()), Ok(Ok(Some(frame)))) if !*self.closed.borrow() => {
                serve_connection(
                    stream,
                    self.dispatcher.clone(),
                    permit,
                    self.requests.clone(),
                    self.control.clone(),
                    Some(ComputerSession {
                        computer_id: self.computer_id.clone(),
                        first: frame,
                        closed: self.closed.clone(),
                    }),
                )
                .await;
            }
            // The tools were turned off while the handshake was read.
            (Ok(()), _) | (Err(Refusal::Silent), _) => {}
            _ => {
                eprintln!(
                    "silod: channel of {}: refused a connection (no valid token)",
                    self.computer_id
                );
                self.refused();
                // The place stays taken while the refusal waits, so guessing is slow.
                time::sleep(REFUSAL_DELAY).await;
                drop(stream);
                drop(permit);
            }
        }
    }
}

/// The listeners of every computer whose tools are on.
pub struct Channels {
    dispatcher: Arc<Dispatcher>,
    handle: tokio::runtime::Handle,
    verify: Arc<dyn Fn(&str, &str) -> bool + Send + Sync>,
    audit: Option<PathBuf>,
    served: Mutex<HashMap<String, (u16, signal::Sender<bool>)>>,
}

impl Channels {
    pub fn new(
        dispatcher: Arc<Dispatcher>,
        handle: tokio::runtime::Handle,
        verify: Arc<dyn Fn(&str, &str) -> bool + Send + Sync>,
        audit: Option<PathBuf>,
    ) -> Self {
        Self {
            dispatcher,
            handle,
            verify,
            audit,
            served: Mutex::new(HashMap::new()),
        }
    }

    /// The engine's channels: tokens are checked against the engine's store.
    pub fn for_engine(context: Arc<SilodContext>, handle: tokio::runtime::Handle) -> Self {
        let app_data = context.paths().app_data.clone();
        let store = Arc::new(TokenStore::new(&app_data));
        Self::new(
            Arc::new(Dispatcher::core(context)),
            handle,
            Arc::new(move |token, computer| store.verify(token, computer).is_some()),
            app_data.is_absolute().then_some(app_data),
        )
    }

    fn served(&self) -> std::sync::MutexGuard<'_, HashMap<String, (u16, signal::Sender<bool>)>> {
        self.served
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The port `computer_id`'s channel is served on.
    pub fn port(&self, computer_id: &str) -> Option<u16> {
        self.served().get(computer_id).map(|(port, _)| *port)
    }
}

impl silo_engine::guest_client::Channels for Channels {
    fn serve(&self, computer_id: &str, listener: StdTcpListener) {
        let port = match listener.local_addr() {
            Ok(address) if address.ip().is_loopback() => address.port(),
            _ => return,
        };
        if listener.set_nonblocking(true).is_err() {
            return;
        }
        let listener = {
            let _entered = self.handle.enter();
            match TcpListener::from_std(listener) {
                Ok(listener) => listener,
                Err(error) => {
                    eprintln!("silod: channel of {computer_id}: {error}");
                    return;
                }
            }
        };
        let (closer, closed) = signal::channel(false);
        if let Some((_, previous)) = self.served().insert(computer_id.to_owned(), (port, closer)) {
            let _ = previous.send(true);
        }
        let served = Served {
            computer_id: computer_id.to_owned(),
            dispatcher: self.dispatcher.clone(),
            verify: self.verify.clone(),
            audit: self.audit.clone(),
            connections: Arc::new(Semaphore::new(MAX_CONNECTIONS)),
            requests: Arc::new(Semaphore::new(MAX_REQUESTS)),
            control: Arc::new(Semaphore::new(CONTROL_REQUESTS)),
            closed,
        };
        self.handle.spawn(served.accept(listener));
    }

    fn close(&self, computer_id: &str) {
        if let Some((_, closer)) = self.served().remove(computer_id) {
            let _ = closer.send(true);
        }
    }

    fn serving(&self, computer_id: &str) -> bool {
        self.served().contains_key(computer_id)
    }
}

#[cfg(test)]
mod tests;
