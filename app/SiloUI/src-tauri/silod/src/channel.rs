//! The channel inside a computer: for each computer with Silo's tools on, a TCP listener on the
//! device's loopback (`127.0.0.1` only) serving the ordinary engine protocol, with every request
//! made as that computer (`Caller::Computer`). The computer's network allows exactly its own
//! port (`silo_engine::network_policy::extra_rules`); the device side always sees `127.0.0.1`, so
//! the token is the identity (`docs/SiloUI-IN-GUEST-CLIENT.md`).
//!
//! The order of checks on a connection, all before anything is answered:
//!
//! 1. The connection takes one of the computer's [`MAX_PENDING`] handshake places, or is closed
//!    at once. Sessions have their own places, so connections that never prove anything cannot
//!    lock a computer's sessions out.
//! 2. The engine sends a [`ChannelChallenge`] with a fresh random nonce.
//! 3. The first frame is read, the challenge sent and the frame read within
//!    [`HANDSHAKE_TIMEOUT`], at most [`MAX_HANDSHAKE_BYTES`].
//! 4. It must be `engine.handshake` with a `proof` made from the stored token hash of the
//!    computer the port belongs to, for this nonce and this port. The computer comes from the
//!    port, never from the frame.
//! 5. Anything else is recorded in the audit log (at most once a second per computer, so the
//!    rows collapse and cannot fill its queue) and the connection is closed after
//!    [`REFUSAL_DELAY`], with no answer.
//! 6. The session then takes one of the computer's [`MAX_CONNECTIONS`] places, or is closed at
//!    once, and is served. It ends when the tools are turned off or the token is minted again.
//!
//! A port can also be held: bound for a computer whose network may still name it, accepting and
//! closing every connection without reading anything, so no other program can take it.

mod watch;

use std::{
    collections::HashMap,
    net::TcpListener as StdTcpListener,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use serde_json::Value;
use silo_api::{
    methods::audit::{AuditCallerKind, AuditOutcome},
    transport::{to_hex, ChannelChallenge, CHANNEL_NONCE_BYTES},
};
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
    framing::write_json_frame,
    socket::{serve_connection, ComputerSession},
};

pub use watch::start;

/// The most sessions one computer may hold open at once.
pub const MAX_CONNECTIONS: usize = 8;
/// The most connections one computer may have that have not finished their handshake.
pub const MAX_PENDING: usize = 4;
/// The most requests one computer may have running at once, over all its connections.
const MAX_REQUESTS: usize = 16;
/// Requests kept for `engine.state` when the others are taken.
const CONTROL_REQUESTS: usize = 2;
/// How long a new connection has to take the challenge and send its handshake.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(2);
/// The largest handshake frame read before the proof is checked.
pub const MAX_HANDSHAKE_BYTES: usize = 16 * 1024;
/// How long a refused connection is held before it is closed.
pub const REFUSAL_DELAY: Duration = Duration::from_secs(1);
/// At most one refusal row per computer this often.
const REFUSAL_ROW_EVERY: Duration = Duration::from_secs(1);
/// At most one refusal line in the engine's log per computer this often.
const REFUSAL_LOG_EVERY: Duration = Duration::from_secs(60);

/// Checks a handshake proof: the computer, the port the connection arrived on, the nonce it
/// was sent, and the proof.
pub type Verify = dyn Fn(&str, u16, &[u8], &str) -> bool + Send + Sync;

/// Why a connection was not served.
#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The connection ended before it sent anything; nothing is recorded.
    Silent,
    /// It sent something that is not a handshake proving this computer's token on this port.
    BadHandshake,
}

/// Checks the first frame of a connection on `computer_id`'s `port`: an `engine.handshake`
/// whose `proof` `verify` accepts for that computer, port and `nonce`. Nothing else in the frame
/// selects the computer.
pub fn check_handshake(
    frame: &Value,
    computer_id: &str,
    port: u16,
    nonce: &[u8],
    verify: &Verify,
) -> Result<(), Refusal> {
    let handshake = frame.get("method").and_then(Value::as_str) == Some("engine.handshake");
    let proof = frame.pointer("/params/proof").and_then(Value::as_str);
    match proof {
        Some(proof) if handshake && verify(computer_id, port, nonce, proof) => Ok(()),
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

/// Sends the challenge and reads the answer. `Ok(None)` when the peer closed before answering.
async fn challenge(stream: &mut TcpStream, nonce: &[u8]) -> std::io::Result<Option<Value>> {
    let challenge = ChannelChallenge {
        nonce: to_hex(nonce),
    };
    write_json_frame(stream, &challenge).await?;
    read_limited(stream, MAX_HANDSHAKE_BYTES).await
}

/// What a port does with a connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    /// The handshake, then a session.
    Serve,
    /// Accepted and closed at once.
    Hold,
    /// The listener is dropped.
    Release,
}

/// One bound port.
struct Port {
    computer_id: String,
    mode: signal::Sender<Mode>,
}

/// Refusals of one computer since its last row and line.
#[derive(Default)]
pub(crate) struct Refusals {
    last_row: Option<Instant>,
    last_line: Option<Instant>,
    unlogged: u64,
}

impl Refusals {
    /// Counts a refusal at `now`: whether it gets an audit row, and how many refusals the log
    /// line it gets reports, when it gets one.
    pub(crate) fn note(&mut self, now: Instant) -> (bool, Option<u64>) {
        self.unlogged += 1;
        let due = |last: Option<Instant>, every: Duration| {
            last.is_none_or(|last| now.duration_since(last) >= every)
        };
        let row = due(self.last_row, REFUSAL_ROW_EVERY);
        if row {
            self.last_row = Some(now);
        }
        let line = due(self.last_line, REFUSAL_LOG_EVERY).then(|| {
            self.last_line = Some(now);
            std::mem::take(&mut self.unlogged)
        });
        (row, line)
    }
}

/// One computer's budgets and the signal that ends its sessions.
#[derive(Clone)]
struct Computer {
    pending: Arc<Semaphore>,
    connections: Arc<Semaphore>,
    requests: Arc<Semaphore>,
    control: Arc<Semaphore>,
    sessions: Arc<Mutex<signal::Sender<bool>>>,
    refusals: Arc<Mutex<Refusals>>,
}

impl Computer {
    fn new() -> Self {
        Self {
            pending: Arc::new(Semaphore::new(MAX_PENDING)),
            connections: Arc::new(Semaphore::new(MAX_CONNECTIONS)),
            requests: Arc::new(Semaphore::new(MAX_REQUESTS)),
            control: Arc::new(Semaphore::new(CONTROL_REQUESTS)),
            sessions: Arc::new(Mutex::new(signal::channel(false).0)),
            refusals: Arc::new(Mutex::new(Refusals::default())),
        }
    }

    /// The signal the sessions opened from now on end by.
    fn session_signal(&self) -> signal::Receiver<bool> {
        lock(&self.sessions).subscribe()
    }

    /// Ends every session opened so far.
    fn end_sessions(&self) {
        let mut sessions = lock(&self.sessions);
        let _ = sessions.send(true);
        *sessions = signal::channel(false).0;
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Default)]
struct State {
    ports: HashMap<u16, Port>,
    computers: HashMap<String, Computer>,
}

/// What every listener shares.
struct Shared {
    dispatcher: Arc<Dispatcher>,
    verify: Arc<Verify>,
    audit: Option<PathBuf>,
    state: Mutex<State>,
}

impl Shared {
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        lock(&self.state)
    }

    fn computer(&self, computer_id: &str) -> Computer {
        self.state()
            .computers
            .entry(computer_id.to_owned())
            .or_insert_with(Computer::new)
            .clone()
    }

    /// Records a refusal: a row at most every [`REFUSAL_ROW_EVERY`] and a line at most every
    /// [`REFUSAL_LOG_EVERY`] per computer.
    fn refused(&self, computer_id: &str, computer: &Computer) {
        let (row, line) = lock(&computer.refusals).note(Instant::now());
        if let Some(count) = line {
            eprintln!(
                "silod: channel of {computer_id}: refused {count} connection(s) without a valid proof"
            );
        }
        let (true, Some(app_data)) = (row, &self.audit) else {
            return;
        };
        AuditLog::shared(app_data).record(Row {
            at_ms: silo_engine::audit::now_ms(),
            caller_kind: AuditCallerKind::Computer,
            caller_id: Some(computer_id.to_owned()),
            client: None,
            method: "engine.handshake".into(),
            computer_id: None,
            outcome: AuditOutcome::Refused,
            error_code: Some("permission_denied".into()),
            job_id: None,
            approval_id: None,
        });
    }

    async fn accept(
        self: Arc<Self>,
        listener: TcpListener,
        port: u16,
        computer_id: String,
        mut mode: signal::Receiver<Mode>,
    ) {
        loop {
            let current = *mode.borrow_and_update();
            if current == Mode::Release {
                return;
            }
            tokio::select! {
                changed = mode.changed() => {
                    if changed.is_err() {
                        return;
                    }
                }
                accepted = listener.accept() => {
                    let stream = match accepted {
                        Ok((stream, _)) => stream,
                        Err(error) => {
                            eprintln!("silod: channel of {computer_id}: accept failed: {error}");
                            time::sleep(Duration::from_millis(100)).await;
                            continue;
                        }
                    };
                    if *mode.borrow() != Mode::Serve {
                        drop(stream);
                        continue;
                    }
                    let computer = self.computer(&computer_id);
                    let Ok(pending) = computer.pending.clone().try_acquire_owned() else {
                        drop(stream);
                        continue;
                    };
                    tokio::spawn(self.clone().connection(
                        stream,
                        port,
                        computer_id.clone(),
                        computer,
                        mode.clone(),
                        pending,
                    ));
                }
            }
        }
    }

    async fn connection(
        self: Arc<Self>,
        mut stream: TcpStream,
        port: u16,
        computer_id: String,
        computer: Computer,
        mode: signal::Receiver<Mode>,
        pending: OwnedSemaphorePermit,
    ) {
        let _ = stream.set_nodelay(true);
        // Sessions opened before a turn-off or a new token end with it, and so does this one.
        let sessions = computer.session_signal();
        let nonce = match silo_engine::guest_client::tokens::random_bytes() {
            Ok(nonce) => nonce,
            Err(error) => {
                eprintln!("silod: channel of {computer_id}: {error}");
                return;
            }
        };
        debug_assert_eq!(nonce.len(), CHANNEL_NONCE_BYTES);
        let first = time::timeout(HANDSHAKE_TIMEOUT, challenge(&mut stream, &nonce)).await;
        let verdict = match &first {
            Ok(Ok(None)) => Err(Refusal::Silent),
            Ok(Ok(Some(frame))) => {
                check_handshake(frame, &computer_id, port, &nonce, &*self.verify)
            }
            Ok(Err(_)) | Err(_) => Err(Refusal::BadHandshake),
        };
        let still_served = *mode.borrow() == Mode::Serve && !*sessions.borrow();
        match (verdict, first) {
            (Ok(()), Ok(Ok(Some(frame)))) if still_served => {
                drop(pending);
                // A computer at its session limit is closed at once.
                let Ok(permit) = computer.connections.clone().try_acquire_owned() else {
                    return;
                };
                serve_connection(
                    stream,
                    self.dispatcher.clone(),
                    permit,
                    computer.requests.clone(),
                    computer.control.clone(),
                    Some(ComputerSession {
                        computer_id,
                        first: frame,
                        closed: sessions,
                    }),
                )
                .await;
            }
            // The tools were turned off, or the token minted again, while the handshake was read.
            (Ok(()), _) | (Err(Refusal::Silent), _) => {}
            _ => {
                self.refused(&computer_id, &computer);
                // The handshake place stays taken while the refusal waits, so guessing is slow.
                time::sleep(REFUSAL_DELAY).await;
                drop(stream);
                drop(pending);
            }
        }
    }
}

/// The listeners of every computer whose tools are on, and the ports held for computers whose
/// networks may still name them.
pub struct Channels {
    shared: Arc<Shared>,
    handle: tokio::runtime::Handle,
}

impl Channels {
    pub fn new(
        dispatcher: Arc<Dispatcher>,
        handle: tokio::runtime::Handle,
        verify: Arc<Verify>,
        audit: Option<PathBuf>,
    ) -> Self {
        Self {
            shared: Arc::new(Shared {
                dispatcher,
                verify,
                audit,
                state: Mutex::new(State::default()),
            }),
            handle,
        }
    }

    /// The engine's channels: proofs are checked against the engine's store.
    pub fn for_engine(context: Arc<SilodContext>, handle: tokio::runtime::Handle) -> Self {
        let app_data = context.paths().app_data.clone();
        let store = Arc::new(TokenStore::new(&app_data));
        Self::new(
            Arc::new(Dispatcher::core(context)),
            handle,
            Arc::new(move |computer, port, nonce, proof| {
                store.verify(computer, port, nonce, proof).is_some()
            }),
            app_data.is_absolute().then_some(app_data),
        )
    }

    /// The port `computer_id`'s channel is served on.
    pub fn port(&self, computer_id: &str) -> Option<u16> {
        silo_engine::guest_client::Channels::serving(self, computer_id)
    }

    /// Starts listening on `listener` for `computer_id` in `mode`.
    fn bind(&self, computer_id: &str, listener: StdTcpListener, mode: Mode) -> Result<u16, String> {
        let port = match listener.local_addr() {
            Ok(address) if address.ip().is_loopback() => address.port(),
            _ => return Err("A channel listens on the loopback only.".into()),
        };
        listener
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        let listener = {
            let _entered = self.handle.enter();
            TcpListener::from_std(listener).map_err(|error| error.to_string())?
        };
        let (sender, receiver) = signal::channel(mode);
        let previous = {
            let mut state = self.shared.state();
            state
                .computers
                .entry(computer_id.to_owned())
                .or_insert_with(Computer::new);
            state.ports.insert(
                port,
                Port {
                    computer_id: computer_id.to_owned(),
                    mode: sender,
                },
            )
        };
        if let Some(previous) = previous {
            let _ = previous.mode.send(Mode::Release);
        }
        self.handle.spawn(self.shared.clone().accept(
            listener,
            port,
            computer_id.to_owned(),
            receiver,
        ));
        Ok(port)
    }

    /// Holds every port served for `computer_id` but `except`, and ends its sessions when one
    /// was served.
    fn hold_served(&self, computer_id: &str, except: Option<u16>) {
        let computer = {
            let state = self.shared.state();
            let mut held = false;
            for (port, entry) in &state.ports {
                if entry.computer_id == computer_id
                    && Some(*port) != except
                    && *entry.mode.borrow() == Mode::Serve
                {
                    let _ = entry.mode.send(Mode::Hold);
                    held = true;
                }
            }
            held.then(|| state.computers.get(computer_id).cloned())
                .flatten()
        };
        if let Some(computer) = computer {
            computer.end_sessions();
        }
    }
}

impl silo_engine::guest_client::Channels for Channels {
    fn serve(&self, computer_id: &str, listener: StdTcpListener) -> Result<(), String> {
        let port = self.bind(computer_id, listener, Mode::Serve)?;
        self.hold_served(computer_id, Some(port));
        Ok(())
    }

    fn hold(&self, computer_id: &str, listener: StdTcpListener) -> Result<(), String> {
        self.bind(computer_id, listener, Mode::Hold).map(|_| ())
    }

    fn serve_held(&self, computer_id: &str, port: u16) -> bool {
        let served = {
            let state = self.shared.state();
            match state.ports.get(&port) {
                Some(entry) if entry.computer_id == computer_id => {
                    let _ = entry.mode.send(Mode::Serve);
                    true
                }
                _ => false,
            }
        };
        if served {
            self.hold_served(computer_id, Some(port));
        }
        served
    }

    fn close(&self, computer_id: &str) {
        self.hold_served(computer_id, None);
        self.end_sessions(computer_id);
    }

    fn end_sessions(&self, computer_id: &str) {
        let computer = self.shared.state().computers.get(computer_id).cloned();
        if let Some(computer) = computer {
            computer.end_sessions();
        }
    }

    fn release(&self, port: u16) {
        let removed = self.shared.state().ports.remove(&port);
        if let Some(removed) = removed {
            let _ = removed.mode.send(Mode::Release);
        }
    }

    fn serving(&self, computer_id: &str) -> Option<u16> {
        self.shared
            .state()
            .ports
            .iter()
            .find(|(_, entry)| {
                entry.computer_id == computer_id && *entry.mode.borrow() == Mode::Serve
            })
            .map(|(port, _)| *port)
    }

    fn bound(&self, port: u16) -> Option<String> {
        self.shared
            .state()
            .ports
            .get(&port)
            .map(|entry| entry.computer_id.clone())
    }
}

#[cfg(test)]
mod tests;
