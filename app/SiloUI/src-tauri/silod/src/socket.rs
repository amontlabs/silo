use std::{
    collections::{HashMap, VecDeque},
    io,
    os::{
        fd::AsRawFd,
        unix::{
            fs::{FileTypeExt, MetadataExt, PermissionsExt},
            net::{UnixListener as StdUnixListener, UnixStream as StdUnixStream},
        },
    },
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use serde_json::Value;
use silo_api::{
    events::EventEnvelope,
    jobs::ErrorCode,
    transport::{ErrorObject, ResponseEnvelope},
};
use silo_engine::{
    context::ClientId,
    events::{BacklogReservation, EventDelivery, SequencedEvent},
};
use tokio::{
    io::AsyncWriteExt,
    net::{UnixListener, UnixStream},
    sync::{mpsc, oneshot, OwnedSemaphorePermit, Semaphore},
    task::JoinSet,
    time,
};

use crate::{
    app::RestartScheduled,
    dispatcher::{Dispatcher, PreparedRequest, SessionState},
    framing::{read_json_frame, write_json_frame},
    paths::ensure_private_directory,
};

const FIRST_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_SOCKET_PATH_BYTES: usize = 104;
const MAX_PENDING_REPLIES: usize = 64;
const MAX_PENDING_REPLY_BYTES: usize = 8 * 1024 * 1024;
const MAX_PENDING_EVENTS: usize = 64;
const MAX_PENDING_EVENT_BYTES: usize = 16 * 1024 * 1024;
const MAX_LOCAL_SESSIONS: usize = 64;
const MAX_INFLIGHT_PER_SESSION: usize = 32;
const MAX_INLINE_REPLY_WORKERS: usize = 8;
const MAX_REQUEST_WORKERS: usize = 60;
const RESERVED_CONTROL_WORKERS: usize = 4;
const REPLY_WRITE_TIMEOUT: Duration = Duration::from_secs(30);

pub trait PeerEuidProvider: Send + Sync + 'static {
    fn peer_euid(&self, fd: i32) -> io::Result<u32>;
}

pub struct SystemPeerEuidProvider;

impl PeerEuidProvider for SystemPeerEuidProvider {
    fn peer_euid(&self, fd: i32) -> io::Result<u32> {
        let mut euid = 0;
        let mut egid = 0;
        let status = unsafe { libc::getpeereid(fd, &mut euid, &mut egid) };
        if status == 0 {
            Ok(euid)
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

pub fn check_peer_euid(
    provider: &dyn PeerEuidProvider,
    fd: i32,
    expected_euid: u32,
) -> io::Result<()> {
    let actual = provider.peer_euid(fd)?;
    if actual == expected_euid {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("peer euid {actual} does not match engine euid {expected_euid}"),
        ))
    }
}

pub struct EngineServer {
    listener: UnixListener,
    socket_guard: SocketGuard,
    expected_euid: u32,
    peer_euid: Arc<dyn PeerEuidProvider>,
    dispatcher: Arc<Dispatcher>,
    session_permits: Arc<Semaphore>,
    request_permits: Arc<Semaphore>,
    control_permits: Arc<Semaphore>,
}

impl EngineServer {
    pub fn bind(
        socket_path: PathBuf,
        expected_euid: u32,
        peer_euid: Arc<dyn PeerEuidProvider>,
        dispatcher: Arc<Dispatcher>,
    ) -> Result<Self, String> {
        let parent = socket_path
            .parent()
            .ok_or_else(|| "engine.sock has no parent directory".to_owned())?;
        ensure_private_directory(parent, expected_euid).map_err(|error| error.to_string())?;
        check_socket_path_length(&socket_path)?;
        remove_stale_socket(&socket_path, expected_euid).map_err(|error| error.to_string())?;
        let std_listener = StdUnixListener::bind(&socket_path)
            .map_err(|error| format!("Cannot bind {}: {error}", socket_path.display()))?;
        let socket_guard = match SocketGuard::new(socket_path.clone(), expected_euid) {
            Ok(guard) => guard,
            Err(error) => {
                let _ = std::fs::remove_file(&socket_path);
                return Err(error.to_string());
            }
        };
        std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600))
            .map_err(|error| error.to_string())?;
        std_listener
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        let listener = UnixListener::from_std(std_listener).map_err(|error| error.to_string())?;
        Ok(Self {
            listener,
            socket_guard,
            expected_euid,
            peer_euid,
            dispatcher,
            session_permits: Arc::new(Semaphore::new(MAX_LOCAL_SESSIONS)),
            request_permits: Arc::new(Semaphore::new(MAX_REQUEST_WORKERS)),
            control_permits: Arc::new(Semaphore::new(RESERVED_CONTROL_WORKERS)),
        })
    }

    pub async fn run(self, shutdown: oneshot::Receiver<()>) {
        let EngineServer {
            listener,
            socket_guard,
            expected_euid,
            peer_euid,
            dispatcher,
            session_permits,
            request_permits,
            control_permits,
        } = self;
        let mut sessions = JoinSet::new();
        tokio::pin!(shutdown);
        loop {
            tokio::select! {
                _ = &mut shutdown => break,
                accepted = listener.accept() => {
                    let (stream, _) = match accepted {
                        Ok(accepted) => accepted,
                        Err(error) => {
                            eprintln!("silod: engine.sock accept failed: {error}");
                            continue;
                        }
                    };
                    // Authenticate immediately after accept, before reading a frame.
                    if let Err(error) = check_peer_euid(peer_euid.as_ref(), stream.as_raw_fd(), expected_euid) {
                        eprintln!("silod: rejected engine.sock peer: {error}");
                        drop(stream);
                        continue;
                    }
                    let Ok(session_permit) = session_permits.clone().try_acquire_owned() else {
                        drop(stream);
                        continue;
                    };
                    sessions.spawn(serve_connection(
                        stream,
                        dispatcher.clone(),
                        session_permit,
                        request_permits.clone(),
                        control_permits.clone(),
                    ));
                }
                Some(result) = sessions.join_next(), if !sessions.is_empty() => {
                    if let Err(error) = result {
                        let message = error.to_string();
                        if !is_restart_scheduled(error) {
                            eprintln!("silod: client session ended unexpectedly: {message}");
                        }
                    }
                }
            }
        }
        sessions.abort_all();
        while sessions.join_next().await.is_some() {}
        drop(listener);
        drop(socket_guard);
    }
}

fn is_restart_scheduled(error: tokio::task::JoinError) -> bool {
    if !error.is_panic() {
        return false;
    }
    error.into_panic().downcast::<RestartScheduled>().is_ok()
}

async fn serve_connection(
    mut stream: UnixStream,
    dispatcher: Arc<Dispatcher>,
    _session_permit: OwnedSemaphorePermit,
    request_permits: Arc<Semaphore>,
    control_permits: Arc<Semaphore>,
) {
    let first = match time::timeout(
        FIRST_HANDSHAKE_TIMEOUT,
        read_json_frame::<_, Value>(&mut stream),
    )
    .await
    {
        Ok(Ok(Some(value))) => value,
        Ok(Ok(None)) => return,
        Ok(Err(error)) => {
            eprintln!("silod: invalid first frame: {error}");
            return;
        }
        Err(_) => {
            eprintln!("silod: engine.sock handshake timed out");
            return;
        }
    };
    let mut session = SessionState::default();
    let first_prepared = dispatcher.prepare(first, &mut session);
    let first_prepared = match first_prepared {
        PreparedRequest::Close(response, reason) => {
            if let Some(response) = response {
                let _ = write_initial_response(&mut stream, &response).await;
            }
            eprintln!("silod: closed client before handshake: {reason}");
            return;
        }
        prepared => prepared,
    };
    let first_result = dispatcher.execute(first_prepared).await;
    if let Some(response) = first_result.response {
        if let Err(error) = write_initial_response(&mut stream, &response).await {
            eprintln!("silod: handshake response failed: {error}");
            return;
        }
    }
    if first_result.close {
        return;
    }

    // Inspect the first request after negotiation before starting the multiplexed session.
    // guest.ssh turns the connection into raw bytes, so it is accepted only here, once.
    let initial_request = match read_json_frame::<_, Value>(&mut stream).await {
        Ok(Some(value)) => value,
        Ok(None) => return,
        Err(error) => {
            eprintln!("silod: client frame failed: {error}");
            return;
        }
    };
    if initial_request.get("method").and_then(Value::as_str) == Some("guest.ssh") {
        let prepared = dispatcher.prepare(initial_request, &mut session);
        let (response, opened) = dispatcher.execute_stream(prepared).await;
        if let Some(response) = response {
            if let Err(error) = write_initial_response(&mut stream, &response).await {
                eprintln!("silod: guest stream response failed: {error}");
                return;
            }
        }
        if let Some(opened) = opened {
            relay_guest_stream(stream, opened).await;
        }
        return;
    }

    let (mut reader, mut writer) = stream.into_split();

    let client_id = ClientId::new(
        session
            .client_id()
            .expect("handshake populated client id")
            .to_owned(),
    );
    let mut event_stream = dispatcher.context().connect_event_stream(client_id.clone());

    let (reply_tx, mut reply_rx) = mpsc::channel::<OutboundReply>(MAX_PENDING_REPLIES);
    let reply_byte_budget = Arc::new(Semaphore::new(MAX_PENDING_REPLY_BYTES));
    let (event_tx, mut event_rx) = mpsc::channel::<OutboundEvent>(MAX_PENDING_EVENTS);
    let event_byte_budget = Arc::new(Semaphore::new(MAX_PENDING_EVENT_BYTES));
    let outbound_order = Arc::new(tokio::sync::Mutex::new(()));
    let (frame_tx, mut frame_rx) = mpsc::channel(1);
    if frame_tx.send(Ok(Some(initial_request))).await.is_err() {
        return;
    }
    let reader_task = tokio::spawn(async move {
        loop {
            let frame = read_json_frame::<_, Value>(&mut reader).await;
            let terminal = matches!(frame, Ok(None) | Err(_));
            if frame_tx.send(frame).await.is_err() || terminal {
                return;
            }
        }
    });
    let mut writer_task = tokio::spawn(async move {
        let mut replies_open = true;
        let mut events_open = true;
        let mut pending_reply: Option<OutboundReply> = None;
        let mut deferred_events = VecDeque::<OutboundEvent>::new();
        let mut written_event_seq = HashMap::<String, u64>::new();
        loop {
            let mut selected = if pending_reply.as_ref().is_some_and(|reply| {
                barrier_satisfied(reply.event_barrier.as_ref(), &written_event_seq)
            }) {
                pending_reply.take().map(OutboundFrame::Reply)
            } else {
                let eligible = deferred_events.iter().position(|event| {
                    pending_reply.as_ref().is_none_or(|reply| {
                        event_precedes_barrier(event, reply.event_barrier.as_ref())
                    })
                });
                eligible.and_then(|index| deferred_events.remove(index).map(OutboundFrame::Event))
            };
            if selected.is_none() {
                tokio::select! {
                    biased;
                    outbound = reply_rx.recv(), if replies_open && pending_reply.is_none() => match outbound {
                        Some(outbound) => {
                            if barrier_satisfied(outbound.event_barrier.as_ref(), &written_event_seq) {
                                selected = Some(OutboundFrame::Reply(outbound));
                            } else {
                                pending_reply = Some(outbound);
                            }
                        }
                        None => replies_open = false,
                    },
                    outbound = event_rx.recv(), if events_open => match outbound {
                        Some(outbound) => {
                            if pending_reply.as_ref().is_some_and(|reply| {
                                !event_precedes_barrier(&outbound, reply.event_barrier.as_ref())
                            }) {
                                deferred_events.push_back(outbound);
                            } else {
                                selected = Some(OutboundFrame::Event(outbound));
                            }
                        }
                        None => events_open = false,
                    },
                    else => {
                        if pending_reply.is_some() {
                            return Err(io::Error::new(
                                io::ErrorKind::ConnectionAborted,
                                "event stream ended before an operations snapshot barrier",
                            ));
                        }
                        break;
                    }
                }
            }
            let Some(selected) = selected else {
                continue;
            };
            let (bytes, byte_permit, ack, event_progress, _backlog_reservation) = match selected {
                OutboundFrame::Reply(reply) => {
                    (reply.bytes, reply.byte_permit, reply.ack, None, None)
                }
                OutboundFrame::Event(event) => (
                    event.bytes,
                    event.byte_permit,
                    None,
                    Some((event.subscription, event.seq)),
                    event.backlog_reservation,
                ),
            };
            let result =
                match time::timeout(REPLY_WRITE_TIMEOUT, write_frame_bytes(&mut writer, &bytes))
                    .await
                {
                    Ok(result) => result,
                    Err(_) => Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "engine.sock client did not accept a frame within 30 seconds",
                    )),
                };
            if let Some(ack) = ack {
                let _ = ack.send(
                    result
                        .as_ref()
                        .map(|_| ())
                        .map_err(|error| error.to_string()),
                );
            }
            drop(byte_permit);
            if let Err(error) = result {
                return Err(error);
            }
            if let Some((subscription, seq)) = event_progress {
                written_event_seq.insert(subscription, seq);
            }
        }
        Ok::<(), io::Error>(())
    });
    let mut tasks = JoinSet::new();
    let session_request_permits = Arc::new(Semaphore::new(MAX_INFLIGHT_PER_SESSION));
    let inline_reply_permits = Arc::new(Semaphore::new(MAX_INLINE_REPLY_WORKERS));
    let mut close_after_reply = false;
    let mut close_ack: Option<oneshot::Receiver<Result<(), String>>> = None;
    let mut immediate_disconnect = false;
    let mut writer_result = None;
    loop {
        let read_result = tokio::select! {
            delivery = event_stream.recv() => {
                match delivery {
                    Some(EventDelivery::Event(event)) => {
                        let subscription = event.subscription.clone();
                        let seq = event.seq;
                        let backlog_reservation = event.backlog_reservation.clone();
                        let frame = match event_frame(event, &dispatcher.context().local_device().device_id) {
                            Ok(frame) => frame,
                            Err(error) => {
                                eprintln!("silod: invalid event payload; closing client session: {error}");
                                close_after_reply = true;
                                break;
                            }
                        };
                        let _outbound_guard = outbound_order.lock().await;
                        if enqueue_event(
                            &event_tx,
                            &event_byte_budget,
                            frame,
                            subscription,
                            seq,
                            backlog_reservation,
                        )
                        .is_err()
                        {
                            close_after_reply = true;
                            immediate_disconnect = true;
                            break;
                        }
                    }
                    Some(EventDelivery::Gap(_gap)) => {
                        // A gap is a transport control signal, not a new wire event.
                        // Close this owner session so the client must resubscribe and read.
                        close_after_reply = true;
                        immediate_disconnect = true;
                        break;
                    }
                    None => {
                        close_after_reply = true;
                        break;
                    }
                }
                continue;
            }
            frame = frame_rx.recv(), if !close_after_reply => frame,
            ack = async {
                close_ack
                    .as_mut()
                    .expect("close acknowledgment branch is guarded")
                    .await
            }, if close_ack.is_some() => {
                if let Err(error) = ack {
                    eprintln!("silod: close reply writer ended before acknowledgment: {error}");
                }
                break;
            }
            result = &mut writer_task => {
                writer_result = Some(result);
                close_after_reply = true;
                break;
            }
            result = tasks.join_next(), if !tasks.is_empty() => {
                if let Some(Err(error)) = result {
                    let message = error.to_string();
                    if !is_restart_scheduled(error) {
                        eprintln!("silod: request worker ended unexpectedly: {message}");
                    }
                }
                continue;
            }
        };
        let Some(read_result) = read_result else {
            break;
        };
        let payload = match read_result {
            Ok(Some(payload)) => payload,
            Ok(None) => break,
            Err(error) => {
                eprintln!("silod: client frame failed: {error}");
                break;
            }
        };
        let prepared = dispatcher.prepare(payload, &mut session);
        match prepared {
            PreparedRequest::Invoke {
                id,
                method,
                params,
                request_context,
            } => {
                let Ok(session_permit) = session_request_permits.clone().try_acquire_owned() else {
                    if !spawn_inline_response(
                        &mut tasks,
                        &inline_reply_permits,
                        &reply_tx,
                        &reply_byte_budget,
                        dispatcher.busy_response(id),
                        None,
                    ) {
                        close_after_reply = true;
                        immediate_disconnect = true;
                        break;
                    }
                    continue;
                };
                let permit_pool = if matches!(method, silo_api::methods::MethodName::EngineState) {
                    &control_permits
                } else {
                    &request_permits
                };
                let Ok(request_permit) = permit_pool.clone().try_acquire_owned() else {
                    drop(session_permit);
                    if !spawn_inline_response(
                        &mut tasks,
                        &inline_reply_permits,
                        &reply_tx,
                        &reply_byte_budget,
                        dispatcher.busy_response(id),
                        None,
                    ) {
                        close_after_reply = true;
                        immediate_disconnect = true;
                        break;
                    }
                    continue;
                };
                let dispatcher = dispatcher.clone();
                let reply_tx = reply_tx.clone();
                let reply_byte_budget = reply_byte_budget.clone();
                let context = dispatcher.context();
                let ordered_response = is_ordered_response(method);
                let outbound_order = outbound_order.clone();
                let ordered_reply_tx = reply_tx.clone();
                let ordered_reply_budget = reply_byte_budget.clone();
                tasks.spawn(async move {
                    let reserved_reply = if ordered_response {
                        match reserve_ordered_reply(&ordered_reply_tx, &ordered_reply_budget).await
                        {
                            Ok(reserved_reply) => Some(reserved_reply),
                            Err(error) => {
                                eprintln!(
                                    "silod: could not reserve ordered reply capacity: {error}"
                                );
                                drop((session_permit, request_permit));
                                return;
                            }
                        }
                    } else {
                        None
                    };
                    let _outbound_guard = ordered_response_guard(method, &outbound_order).await;
                    let invoke = PreparedRequest::Invoke {
                        id,
                        method,
                        params,
                        request_context,
                    };
                    let result = dispatcher.execute(invoke).await;
                    let queued = if let Some(response) = result.response {
                        if let Some(reserved_reply) = reserved_reply {
                            match reserved_reply.enqueue(response, None, result.event_barrier) {
                                Ok(()) => true,
                                Err(error) => {
                                    eprintln!("silod: could not enqueue ordered reply: {error}");
                                    false
                                }
                            }
                        } else {
                            match enqueue_response(
                                &reply_tx,
                                &reply_byte_budget,
                                response,
                                None,
                                result.event_barrier,
                            )
                            .await
                            {
                                Ok(()) => true,
                                Err(error) => {
                                    eprintln!("silod: could not enqueue engine reply: {error}");
                                    false
                                }
                            }
                        }
                    } else {
                        true
                    };
                    if queued {
                        if let Some((client_id, subscription)) = result.activate_subscription {
                            let _ =
                                context.activate_events(&ClientId::new(client_id), &subscription);
                        }
                    }
                    drop((session_permit, request_permit));
                });
            }
            PreparedRequest::Reply(response) => {
                if !spawn_inline_response(
                    &mut tasks,
                    &inline_reply_permits,
                    &reply_tx,
                    &reply_byte_budget,
                    response,
                    None,
                ) {
                    close_after_reply = true;
                    immediate_disconnect = true;
                    break;
                }
            }
            PreparedRequest::Close(response, reason) => {
                eprintln!("silod: closing client session: {reason}");
                if let Some(response) = response {
                    let (ack_tx, ack_rx) = oneshot::channel();
                    if !spawn_inline_response(
                        &mut tasks,
                        &inline_reply_permits,
                        &reply_tx,
                        &reply_byte_budget,
                        response,
                        Some(ack_tx),
                    ) {
                        close_after_reply = true;
                        immediate_disconnect = true;
                        break;
                    }
                    close_ack = Some(ack_rx);
                    close_after_reply = true;
                } else {
                    close_after_reply = true;
                    break;
                }
            }
        }
    }
    reader_task.abort();
    let _ = reader_task.await;
    drop(reply_tx);
    drop(event_tx);
    // Stopping a tunnel can wait up to half a second, so it must not hold an async worker.
    let closing = client_id.as_str().to_owned();
    let _ = tokio::task::spawn_blocking(move || {
        silo_engine::desktop_viewer_api::close_client(&closing);
    })
    .await;
    event_stream.disconnect();

    if (immediate_disconnect || !close_after_reply) && writer_result.is_none() {
        // A disconnected client cannot consume queued frames. Stop the writer so
        // blocked reply workers release their byte reservations before teardown.
        writer_task.abort();
    }
    if close_after_reply {
        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
    } else {
        // Accepted engine work outlives its client connection. Response workers
        // will observe the closed writer and release their session resources.
        tasks.detach_all();
    }

    if let Some(result) = writer_result {
        match result {
            Ok(Ok(())) => {}
            Ok(Err(error)) => eprintln!("silod: socket writer failed: {error}"),
            Err(error) => eprintln!("silod: socket writer task failed: {error}"),
        }
    } else {
        match writer_task.await {
            Err(error) if close_after_reply && !immediate_disconnect => {
                eprintln!("silod: socket writer task failed: {error}");
            }
            _ => {}
        }
    }
}

/// How often an open guest stream re-checks that it may continue.
const GUEST_STREAM_CHECK_INTERVAL: Duration = Duration::from_millis(50);

/// Copies bytes both ways between the client and the guest until either side ends or the
/// stream is no longer allowed (its forward was unpublished, or the computer stopped).
async fn relay_guest_stream(mut client: UnixStream, opened: crate::dispatcher::OpenedStream) {
    let crate::dispatcher::OpenedStream { stream, allowed } = opened;
    if let Err(error) = stream.set_nonblocking(true) {
        eprintln!("silod: guest stream setup failed: {error}");
        return;
    }
    let mut guest = match tokio::net::TcpStream::from_std(stream) {
        Ok(guest) => guest,
        Err(error) => {
            eprintln!("silod: guest stream setup failed: {error}");
            return;
        }
    };
    relay_while_allowed(&mut client, &mut guest, &*allowed).await;
}

async fn relay_while_allowed<A, B>(
    client: &mut A,
    guest: &mut B,
    allowed: &(dyn Fn() -> bool + Send + Sync),
) where
    A: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    B: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let relay = tokio::io::copy_bidirectional(client, guest);
    tokio::pin!(relay);
    let mut check = time::interval(GUEST_STREAM_CHECK_INTERVAL);
    loop {
        tokio::select! {
            result = &mut relay => {
                if let Err(error) = result {
                    eprintln!("silod: guest stream ended: {error}");
                }
                return;
            }
            _ = check.tick() => {
                if !allowed() {
                    return;
                }
            }
        }
    }
}

async fn write_initial_response<W>(
    writer: &mut W,
    response: &ResponseEnvelope<Value>,
) -> io::Result<()>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    match time::timeout(REPLY_WRITE_TIMEOUT, write_json_frame(writer, response)).await {
        Ok(result) => result,
        Err(_) => Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "engine.sock client did not accept the handshake reply within 30 seconds",
        )),
    }
}

struct OutboundReply {
    bytes: Vec<u8>,
    byte_permit: OwnedSemaphorePermit,
    ack: Option<oneshot::Sender<Result<(), String>>>,
    event_barrier: Option<HashMap<String, u64>>,
}

struct OutboundEvent {
    bytes: Vec<u8>,
    byte_permit: OwnedSemaphorePermit,
    subscription: String,
    seq: u64,
    backlog_reservation: Option<BacklogReservation>,
}

struct ReservedReply {
    slot: mpsc::OwnedPermit<OutboundReply>,
    byte_capacity: OwnedSemaphorePermit,
}

impl ReservedReply {
    fn enqueue(
        mut self,
        response: ResponseEnvelope<Value>,
        ack: Option<oneshot::Sender<Result<(), String>>>,
        event_barrier: Option<HashMap<String, u64>>,
    ) -> Result<(), String> {
        let bytes = encode_response(&response)?;
        let byte_permit = self
            .byte_capacity
            .split(bytes.len())
            .ok_or_else(|| "reserved reply byte capacity was too small".to_owned())?;
        drop(self.byte_capacity);
        self.slot.send(OutboundReply {
            bytes,
            byte_permit,
            ack,
            event_barrier,
        });
        Ok(())
    }
}

enum OutboundFrame {
    Reply(OutboundReply),
    Event(OutboundEvent),
}

fn spawn_inline_response(
    tasks: &mut JoinSet<()>,
    permits: &Arc<Semaphore>,
    sender: &mpsc::Sender<OutboundReply>,
    byte_budget: &Arc<Semaphore>,
    response: ResponseEnvelope<Value>,
    ack: Option<oneshot::Sender<Result<(), String>>>,
) -> bool {
    let Ok(permit) = permits.clone().try_acquire_owned() else {
        return false;
    };
    let sender = sender.clone();
    let byte_budget = byte_budget.clone();
    tasks.spawn(async move {
        let _permit = permit;
        if let Err(error) = enqueue_response(&sender, &byte_budget, response, ack, None).await {
            eprintln!("silod: could not enqueue inline reply: {error}");
        }
    });
    true
}

async fn enqueue_response(
    sender: &mpsc::Sender<OutboundReply>,
    byte_budget: &Arc<Semaphore>,
    response: ResponseEnvelope<Value>,
    ack: Option<oneshot::Sender<Result<(), String>>>,
    event_barrier: Option<HashMap<String, u64>>,
) -> Result<(), String> {
    let bytes = encode_response(&response)?;
    let byte_count = u32::try_from(bytes.len()).map_err(|error| error.to_string())?;
    let byte_permit = byte_budget
        .clone()
        .acquire_many_owned(byte_count)
        .await
        .map_err(|error| error.to_string())?;
    sender
        .send(OutboundReply {
            bytes,
            byte_permit,
            ack,
            event_barrier,
        })
        .await
        .map_err(|error| error.to_string())
}

async fn reserve_ordered_reply(
    sender: &mpsc::Sender<OutboundReply>,
    byte_budget: &Arc<Semaphore>,
) -> Result<ReservedReply, String> {
    let slot = sender
        .clone()
        .reserve_owned()
        .await
        .map_err(|error| error.to_string())?;
    let byte_capacity = byte_budget
        .clone()
        .acquire_many_owned(crate::framing::MAX_FRAME_BYTES as u32)
        .await
        .map_err(|error| error.to_string())?;
    Ok(ReservedReply {
        slot,
        byte_capacity,
    })
}

fn enqueue_event(
    sender: &mpsc::Sender<OutboundEvent>,
    byte_budget: &Arc<Semaphore>,
    frame: Value,
    subscription: String,
    seq: u64,
    backlog_reservation: BacklogReservation,
) -> Result<(), String> {
    let bytes = serde_json::to_vec(&frame).map_err(|error| error.to_string())?;
    if bytes.is_empty() || bytes.len() > crate::framing::MAX_FRAME_BYTES {
        return Err(format!(
            "event frame length {} exceeds the frame limit",
            bytes.len()
        ));
    }
    let byte_count = u32::try_from(bytes.len()).map_err(|error| error.to_string())?;
    let byte_permit = byte_budget
        .clone()
        .try_acquire_many_owned(byte_count)
        .map_err(|error| error.to_string())?;
    sender
        .try_send(OutboundEvent {
            bytes,
            byte_permit,
            subscription,
            seq,
            backlog_reservation: Some(backlog_reservation),
        })
        .map_err(|error| error.to_string())
}

fn is_ordered_response(method: silo_api::methods::MethodName) -> bool {
    matches!(
        method,
        silo_api::methods::MethodName::EventsSubscribe
            | silo_api::methods::MethodName::EventsUnsubscribe
            | silo_api::methods::MethodName::NotificationsList
            | silo_api::methods::MethodName::OperationsList
    )
}

async fn ordered_response_guard(
    method: silo_api::methods::MethodName,
    outbound_order: &Arc<tokio::sync::Mutex<()>>,
) -> Option<tokio::sync::OwnedMutexGuard<()>> {
    if is_ordered_response(method) {
        Some(outbound_order.clone().lock_owned().await)
    } else {
        None
    }
}

fn encode_response(response: &ResponseEnvelope<Value>) -> Result<Vec<u8>, String> {
    let response_bytes = match serde_json::to_vec(response) {
        Ok(bytes) if !bytes.is_empty() && bytes.len() <= crate::framing::MAX_FRAME_BYTES => {
            return Ok(bytes);
        }
        Ok(bytes) => format!(
            "response frame length {} exceeds the {} byte limit",
            bytes.len(),
            crate::framing::MAX_FRAME_BYTES
        ),
        Err(error) => format!("response serialization failed: {error}"),
    };
    let id = match response {
        ResponseEnvelope::Success { id, .. } | ResponseEnvelope::Error { id, .. } => id.clone(),
    };
    let fallback: ResponseEnvelope<Value> = ResponseEnvelope::Error {
        id,
        error: ErrorObject {
            code: ErrorCode::OutputTooLarge,
            message: "The response exceeds the 4 MiB engine frame limit.".to_owned(),
            diagnostics: None,
        },
    };
    let bytes = serde_json::to_vec(&fallback).map_err(|error| {
        format!("could not encode bounded response after {response_bytes}: {error}")
    })?;
    if bytes.is_empty() || bytes.len() > crate::framing::MAX_FRAME_BYTES {
        return Err(format!(
            "bounded response frame length {} exceeds the frame limit",
            bytes.len()
        ));
    }
    eprintln!("silod: replaced unframeable engine reply: {response_bytes}");
    Ok(bytes)
}

fn barrier_satisfied(
    barrier: Option<&HashMap<String, u64>>,
    written: &HashMap<String, u64>,
) -> bool {
    barrier.is_none_or(|barrier| {
        barrier.iter().all(|(subscription, required)| {
            written.get(subscription).copied().unwrap_or(0) >= *required
        })
    })
}

fn event_precedes_barrier(event: &OutboundEvent, barrier: Option<&HashMap<String, u64>>) -> bool {
    barrier
        .and_then(|barrier| barrier.get(&event.subscription))
        .is_none_or(|required| event.seq <= *required)
}

fn event_frame(event: SequencedEvent, device_id: &str) -> Result<Value, String> {
    let candidate = silo_engine::events::wire_event_envelope(
        &event.event,
        &event.subscription,
        device_id,
        event.seq,
    );
    let typed = serde_json::from_value::<EventEnvelope>(candidate)
        .map_err(|error| format!("event payload does not match silo-api: {error}"))?;
    serde_json::to_value(typed).map_err(|error| error.to_string())
}

async fn write_frame_bytes<W>(writer: &mut W, bytes: &[u8]) -> io::Result<()>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let length = u32::try_from(bytes.len())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    writer.write_all(&length.to_be_bytes()).await?;
    writer.write_all(bytes).await?;
    writer.flush().await
}

struct SocketGuard {
    path: PathBuf,
    device: u64,
    inode: u64,
    euid: u32,
}

impl SocketGuard {
    fn new(path: PathBuf, euid: u32) -> io::Result<Self> {
        let metadata = std::fs::symlink_metadata(&path)?;
        if !metadata.file_type().is_socket() || metadata.uid() != euid {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "bound engine.sock is not an euid-owned socket",
            ));
        }
        Ok(Self {
            path,
            device: metadata.dev(),
            inode: metadata.ino(),
            euid,
        })
    }
}

impl Drop for SocketGuard {
    fn drop(&mut self) {
        if let Ok(metadata) = std::fs::symlink_metadata(&self.path) {
            if metadata.file_type().is_socket()
                && metadata.uid() == self.euid
                && metadata.dev() == self.device
                && metadata.ino() == self.inode
            {
                let _ = std::fs::remove_file(&self.path);
            }
        }
    }
}

fn check_socket_path_length(path: &Path) -> Result<(), String> {
    use std::os::unix::ffi::OsStrExt;
    if path.as_os_str().as_bytes().len() >= MAX_SOCKET_PATH_BYTES {
        return Err(format!(
            "engine.sock path is too long for sockaddr_un: {}",
            path.display()
        ));
    }
    Ok(())
}

fn remove_stale_socket(path: &Path, euid: u32) -> io::Result<()> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if !metadata.file_type().is_socket() || metadata.uid() != euid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{} is not an euid-owned socket", path.display()),
        ));
    }
    let socket_id = (metadata.dev(), metadata.ino());
    match StdUnixStream::connect(path) {
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AddrInUse,
            "engine.sock has a live listener",
        )),
        Err(error) if error.kind() == io::ErrorKind::ConnectionRefused => {
            let current = std::fs::symlink_metadata(path)?;
            if !current.file_type().is_socket()
                || current.uid() != euid
                || (current.dev(), current.ino()) != socket_id
            {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "engine.sock changed while checking stale socket",
                ));
            }
            std::fs::remove_file(path)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if std::fs::symlink_metadata(path).is_err() {
                Ok(())
            } else {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "engine.sock changed while checking stale socket",
                ))
            }
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use silo_engine::context::EngineContext;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Mutex;

    struct MockPeerEuid(u32);
    impl PeerEuidProvider for MockPeerEuid {
        fn peer_euid(&self, _fd: i32) -> io::Result<u32> {
            Ok(self.0)
        }
    }

    #[test]
    fn snapshot_response_waits_for_earlier_events_and_precedes_later_events() {
        let barrier = HashMap::from([("operations-subscription".to_owned(), 4)]);
        let written = HashMap::from([("operations-subscription".to_owned(), 3)]);
        assert!(!barrier_satisfied(Some(&barrier), &written));

        let budget = Arc::new(Semaphore::new(2));
        let earlier = OutboundEvent {
            bytes: Vec::new(),
            byte_permit: budget.clone().try_acquire_owned().unwrap(),
            subscription: "operations-subscription".to_owned(),
            seq: 4,
            backlog_reservation: None,
        };
        let later = OutboundEvent {
            bytes: Vec::new(),
            byte_permit: budget.clone().try_acquire_owned().unwrap(),
            subscription: "operations-subscription".to_owned(),
            seq: 5,
            backlog_reservation: None,
        };
        assert!(event_precedes_barrier(&earlier, Some(&barrier)));
        assert!(!event_precedes_barrier(&later, Some(&barrier)));

        assert!(barrier_satisfied(
            Some(&barrier),
            &HashMap::from([("operations-subscription".to_owned(), 4)])
        ));
    }

    #[tokio::test]
    async fn subscribe_reply_queues_before_concurrent_event_fanout_and_activation() {
        assert!(is_ordered_response(
            silo_api::methods::MethodName::EventsUnsubscribe
        ));
        assert!(is_ordered_response(
            silo_api::methods::MethodName::OperationsList
        ));
        assert!(is_ordered_response(
            silo_api::methods::MethodName::NotificationsList
        ));
        assert!(!is_ordered_response(
            silo_api::methods::MethodName::OperationsJobGet
        ));

        let bus = silo_engine::events::EventBus::default();
        let client_id = ClientId::new("subscribe-order-test");
        let mut event_stream = bus.connect(client_id.clone());
        let existing_subscription = bus
            .subscribe(&client_id, vec!["computers.changed".to_owned()])
            .unwrap();
        bus.activate(&client_id, &existing_subscription).unwrap();

        let (reply_sender, mut reply_receiver) = mpsc::channel(1);
        let reply_budget = Arc::new(Semaphore::new(MAX_PENDING_REPLY_BYTES));
        let reserved_reply = reserve_ordered_reply(&reply_sender, &reply_budget)
            .await
            .unwrap();
        let outbound_order = Arc::new(tokio::sync::Mutex::new(()));
        let request_guard = ordered_response_guard(
            silo_api::methods::MethodName::EventsSubscribe,
            &outbound_order,
        )
        .await;
        let pending_subscription = bus
            .subscribe(&client_id, vec!["notifications.*".to_owned()])
            .unwrap();

        let (event_sender, mut event_receiver) = mpsc::channel(1);
        let event_budget = Arc::new(Semaphore::new(MAX_PENDING_EVENT_BYTES));
        let order = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let (event_ready_sender, event_ready_receiver) = oneshot::channel();
        let (event_enqueued_sender, event_enqueued_receiver) = oneshot::channel();
        let mut event_enqueued_receiver = Some(event_enqueued_receiver);
        let (event_pump_release_sender, event_pump_release_receiver) = oneshot::channel();
        let event_order = outbound_order.clone();
        let event_log = order.clone();
        let event_task = tokio::spawn(async move {
            let EventDelivery::Event(event) = event_stream.recv().await.unwrap() else {
                panic!("expected the existing subscription event");
            };
            let subscription = event.subscription.clone();
            let seq = event.seq;
            let backlog_reservation = event.backlog_reservation.clone();
            let frame = event_frame(event, "test-device").unwrap();
            event_ready_sender.send(()).unwrap();

            let _outbound_guard = event_order.lock_owned().await;
            enqueue_event(
                &event_sender,
                &event_budget,
                frame,
                subscription,
                seq,
                backlog_reservation,
            )
            .unwrap();
            drop(_outbound_guard);
            event_log.lock().await.push("event");
            event_enqueued_sender.send(()).unwrap();
            event_pump_release_receiver.await.unwrap();
        });

        bus.publish(
            silo_engine::context::Event::new(
                "computers.changed",
                serde_json::json!({"revision": 1}),
            ),
            silo_engine::context::Audience::All,
        );
        event_ready_receiver.await.unwrap();
        let event_can_run_before_reply = request_guard.is_none();
        if event_can_run_before_reply {
            // Model the regression interleaving: with no shared ordering
            // guard, let the event pump take the queue first.
            event_enqueued_receiver.take().unwrap().await.unwrap();
        }

        reserved_reply
            .enqueue(
                ResponseEnvelope::Success {
                    id: "subscribe".to_owned(),
                    result: serde_json::json!({"subscription": pending_subscription}),
                },
                None,
                None,
            )
            .unwrap();
        order.lock().await.push("reply");
        bus.activate(&client_id, &pending_subscription).unwrap();
        drop(request_guard);
        event_pump_release_sender.send(()).unwrap();
        if !event_can_run_before_reply {
            event_enqueued_receiver.take().unwrap().await.unwrap();
        }
        event_task.await.unwrap();

        assert_eq!(*order.lock().await, vec!["reply", "event"]);
        let reply = reply_receiver.recv().await.unwrap();
        let response: ResponseEnvelope<Value> = serde_json::from_slice(&reply.bytes).unwrap();
        assert!(matches!(response, ResponseEnvelope::Success { id, .. } if id == "subscribe"));
        let event = event_receiver.recv().await.unwrap();
        assert_eq!(event.subscription, existing_subscription);
    }

    #[tokio::test]
    async fn oversized_job_get_enqueues_a_bounded_error_reply() {
        let (sender, mut receiver) = mpsc::channel(1);
        let byte_budget = Arc::new(Semaphore::new(MAX_PENDING_REPLY_BYTES));
        let response = ResponseEnvelope::Success {
            id: "large-job-get".to_owned(),
            result: serde_json::json!({
                "jobId": "job:1",
                "outcome": {"value": "x".repeat(crate::framing::MAX_FRAME_BYTES)},
            }),
        };

        enqueue_response(&sender, &byte_budget, response, None, None)
            .await
            .unwrap();
        let queued = receiver.recv().await.unwrap();

        assert!(queued.bytes.len() <= crate::framing::MAX_FRAME_BYTES);
        let response: ResponseEnvelope<Value> = serde_json::from_slice(&queued.bytes).unwrap();
        assert!(matches!(
            response,
            ResponseEnvelope::Error { id, error }
                if id == "large-job-get"
                    && error.code == ErrorCode::OutputTooLarge
                    && error.message == "The response exceeds the 4 MiB engine frame limit."
        ));
    }

    #[tokio::test]
    async fn one_mib_outcome_chunk_remains_a_successful_readable_reply() {
        let (sender, mut receiver) = mpsc::channel(1);
        let byte_budget = Arc::new(Semaphore::new(MAX_PENDING_REPLY_BYTES));
        let chunk = "x".repeat(1024 * 1024);
        let response = ResponseEnvelope::Success {
            id: "chunked-job-get".to_owned(),
            result: serde_json::json!({
                "jobId": "job:1",
                "outcomeChunk": {
                    "data": chunk,
                    "totalBytes": 2 * 1024 * 1024,
                    "offsetBytes": 0,
                    "nextOffsetBytes": 1024 * 1024,
                },
            }),
        };

        enqueue_response(&sender, &byte_budget, response, None, None)
            .await
            .unwrap();
        let queued = receiver.recv().await.unwrap();

        assert!(queued.bytes.len() <= crate::framing::MAX_FRAME_BYTES);
        let decoded: Value = serde_json::from_slice(&queued.bytes).unwrap();
        assert_eq!(decoded["id"], "chunked-job-get");
        assert_eq!(
            decoded["result"]["outcomeChunk"]["data"]
                .as_str()
                .unwrap()
                .len(),
            1024 * 1024
        );
        assert_eq!(decoded["result"]["outcomeChunk"]["offsetBytes"], 0);
    }

    #[tokio::test]
    async fn socket_event_queue_keeps_the_shared_backlog_reservation() {
        let bus = silo_engine::events::EventBus::default();
        let client_id = ClientId::new("socket-backlog-test");
        let mut receiver = bus.connect(client_id.clone());
        let subscription = bus
            .subscribe(&client_id, vec!["computers.changed".to_owned()])
            .unwrap();
        bus.activate(&client_id, &subscription).unwrap();
        bus.publish(
            silo_engine::context::Event::new(
                "computers.changed",
                serde_json::json!({"revision": 0}),
            ),
            silo_engine::context::Audience::All,
        );

        let silo_engine::events::EventDelivery::Event(event) = receiver.recv().await.unwrap()
        else {
            panic!("expected the first queued event");
        };
        let subscription = event.subscription.clone();
        let sequence = event.seq;
        let backlog_reservation = event.backlog_reservation.clone();
        let frame = event_frame(event, "test-device").unwrap();
        let (sender, mut socket_queue) = mpsc::channel(1);
        let byte_budget = Arc::new(Semaphore::new(MAX_PENDING_EVENT_BYTES));
        enqueue_event(
            &sender,
            &byte_budget,
            frame,
            subscription,
            sequence,
            backlog_reservation,
        )
        .unwrap();
        let outbound = socket_queue.recv().await.unwrap();

        for revision in 1..silo_engine::events::MAX_EVENTS_PER_SUBSCRIPTION {
            bus.publish(
                silo_engine::context::Event::new(
                    "computers.changed",
                    serde_json::json!({"revision": revision}),
                ),
                silo_engine::context::Audience::All,
            );
        }
        bus.publish(
            silo_engine::context::Event::new(
                "computers.changed",
                serde_json::json!({
                    "revision": silo_engine::events::MAX_EVENTS_PER_SUBSCRIPTION
                }),
            ),
            silo_engine::context::Audience::All,
        );

        assert!(matches!(receiver.recv().await, Some(EventDelivery::Gap(_))));
        drop(outbound);
    }

    #[test]
    fn mocked_foreign_euid_is_rejected() {
        let error = check_peer_euid(&MockPeerEuid(500), 0, 501).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    }

    #[tokio::test]
    async fn operations_list_response_is_barriered_after_prior_events() {
        let root = tempfile::tempdir().unwrap();
        let socket = root.path().join("engine.sock");
        let euid = unsafe { libc::geteuid() };
        let context = test_context_with_paths(silo_engine::context::EnginePaths {
            home: root.path().to_path_buf(),
            ..Default::default()
        });
        let server = EngineServer::bind(
            socket.clone(),
            euid,
            Arc::new(MockPeerEuid(euid)),
            Arc::new(Dispatcher::core(context.clone())),
        )
        .unwrap();
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server_task = tokio::spawn(server.run(shutdown_rx));
        let mut client = UnixStream::connect(&socket).await.unwrap();
        write_json_frame(
            &mut client,
            &serde_json::json!({
                "id": "handshake",
                "method": "engine.handshake",
                "params": {
                    "protocol": {"min": silo_api::transport::PROTOCOL_VERSION, "max": silo_api::transport::PROTOCOL_VERSION},
                    "client": {"name": "barrier-test", "version": "0.1"}
                }
            }),
        )
        .await
        .unwrap();
        let _: Value = read_json_frame(&mut client).await.unwrap().unwrap();
        write_json_frame(
            &mut client,
            &serde_json::json!({
                "id": "subscribe",
                "method": "events.subscribe",
                "params": {"events": ["operations.*"]}
            }),
        )
        .await
        .unwrap();
        let _: Value = read_json_frame(&mut client).await.unwrap().unwrap();

        context.publish(
            silo_engine::context::Event::new(
                "operations.queue.changed",
                serde_json::json!({"running": [], "waiting": [], "unacknowledgedJobIds": []}),
            ),
            silo_engine::context::Audience::All,
        );
        write_json_frame(
            &mut client,
            &serde_json::json!({"id": "queue", "method": "operations.list", "params": {}}),
        )
        .await
        .unwrap();
        // Pipeline a protocol-closing request behind the barriered response. The
        // connection task must keep forwarding the event the writer needs before
        // it can acknowledge the close reply.
        write_json_frame(
            &mut client,
            &serde_json::json!({"id": "queue", "method": "operations.list", "params": {}}),
        )
        .await
        .unwrap();
        let event: Value =
            tokio::time::timeout(Duration::from_secs(2), read_json_frame(&mut client))
                .await
                .unwrap()
                .unwrap()
                .unwrap();
        assert_eq!(event["event"], "operations.queue.changed");
        let response: Value =
            tokio::time::timeout(Duration::from_secs(2), read_json_frame(&mut client))
                .await
                .unwrap()
                .unwrap()
                .unwrap();
        assert_eq!(response["id"], "queue");
        assert!(response["result"]["running"].is_array());
        let close_error: Value =
            tokio::time::timeout(Duration::from_secs(2), read_json_frame(&mut client))
                .await
                .unwrap()
                .unwrap()
                .unwrap();
        assert_eq!(close_error["id"], "queue");
        assert!(close_error["error"]["message"]
            .as_str()
            .unwrap()
            .contains("already used"));

        drop(client);
        let _ = shutdown_tx.send(());
        server_task.await.unwrap();
    }

    #[test]
    fn listener_can_bind_on_the_initial_thread_with_an_entered_io_runtime() {
        let root = tempfile::tempdir().unwrap();
        let socket = root.path().join("engine.sock");
        let euid = unsafe { libc::geteuid() };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let _runtime_context = runtime.enter();
        let server = EngineServer::bind(
            socket.clone(),
            euid,
            Arc::new(MockPeerEuid(euid)),
            Arc::new(Dispatcher::core(test_context())),
        )
        .unwrap();

        assert!(socket.exists());
        drop(server);
        assert!(!socket.exists());
    }

    #[tokio::test]
    async fn server_closes_a_foreign_peer_before_reading_bytes() {
        let root = tempfile::tempdir().unwrap();
        let socket = root.path().join("engine.sock");
        let euid = unsafe { libc::geteuid() };
        let dispatcher = Arc::new(Dispatcher::core(test_context()));
        let server = EngineServer::bind(
            socket.clone(),
            euid,
            Arc::new(MockPeerEuid(euid.wrapping_add(1))),
            dispatcher,
        )
        .unwrap();
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server_task = tokio::spawn(server.run(shutdown_rx));
        let mut client = UnixStream::connect(&socket).await.unwrap();
        let mut byte = [0_u8; 1];
        let read = tokio::time::timeout(
            Duration::from_secs(2),
            tokio::io::AsyncReadExt::read(&mut client, &mut byte),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(read, 0);
        let _ = shutdown_tx.send(());
        server_task.await.unwrap();
    }

    #[tokio::test]
    async fn engine_socket_subscribes_streams_and_unsubscribes_catalogue_events() {
        let root = tempfile::tempdir().unwrap();
        let socket = root.path().join("engine.sock");
        let euid = unsafe { libc::geteuid() };
        let context = test_context();
        let server = EngineServer::bind(
            socket.clone(),
            euid,
            Arc::new(MockPeerEuid(euid)),
            Arc::new(Dispatcher::core(context.clone())),
        )
        .unwrap();
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server_task = tokio::spawn(server.run(shutdown_rx));
        let mut client = UnixStream::connect(&socket).await.unwrap();

        write_json_frame(
            &mut client,
            &serde_json::json!({
                "id": "handshake",
                "method": "engine.handshake",
                "params": {
                    "protocol": {"min": silo_api::transport::PROTOCOL_VERSION, "max": silo_api::transport::PROTOCOL_VERSION},
                    "client": {"name": "socket-test", "version": "0.1"}
                }
            }),
        )
        .await
        .unwrap();
        let handshake: Value = read_json_frame(&mut client).await.unwrap().unwrap();
        assert_eq!(handshake["result"]["deviceId"], "test-device");

        write_json_frame(
            &mut client,
            &serde_json::json!({
                "id": "subscribe",
                "method": "events.subscribe",
                "params": {"events": ["operations.job.finished"]}
            }),
        )
        .await
        .unwrap();
        let subscribed: Value = read_json_frame(&mut client).await.unwrap().unwrap();
        let subscription = subscribed["result"]["subscription"].as_str().unwrap();
        assert!(!subscription.is_empty());

        context.publish(
            silo_engine::context::Event::new(
                "operations.job.finished",
                serde_json::json!({"jobId":"job:1", "result":{"done":true}}),
            ),
            silo_engine::context::Audience::All,
        );
        let event: Value =
            tokio::time::timeout(Duration::from_secs(2), read_json_frame(&mut client))
                .await
                .unwrap()
                .unwrap()
                .unwrap();
        assert_eq!(event["event"], "operations.job.finished");
        assert_eq!(event["subscription"], subscription);
        assert_eq!(event["deviceId"], "test-device");
        assert_eq!(event["seq"], 1);
        assert_eq!(event["payload"]["jobId"], "job:1");

        context.publish(
            silo_engine::context::Event::new(
                "operations.job.finished",
                serde_json::json!({"jobId":"job:2", "result":{"done":true}}),
            ),
            silo_engine::context::Audience::All,
        );
        write_json_frame(
            &mut client,
            &serde_json::json!({
                "id": "unsubscribe",
                "method": "events.unsubscribe",
                "params": {"subscription": subscription}
            }),
        )
        .await
        .unwrap();
        let final_event: Value = read_json_frame(&mut client).await.unwrap().unwrap();
        assert_eq!(final_event["event"], "operations.job.finished");
        assert_eq!(final_event["payload"]["jobId"], "job:2");
        let unsubscribed: Value = read_json_frame(&mut client).await.unwrap().unwrap();
        assert!(matches!(
            unsubscribed,
            serde_json::Value::Object(ref response) if response.get("id") == Some(&serde_json::json!("unsubscribe"))
        ));
        context.publish(
            silo_engine::context::Event::new(
                "operations.job.finished",
                serde_json::json!({"jobId":"job:3", "result":{"done":true}}),
            ),
            silo_engine::context::Audience::All,
        );
        assert!(tokio::time::timeout(
            Duration::from_millis(100),
            read_json_frame::<_, Value>(&mut client),
        )
        .await
        .is_err());

        let _ = shutdown_tx.send(());
        server_task.await.unwrap();
    }

    #[tokio::test]
    async fn bound_socket_is_private_and_removed_by_inode_on_shutdown() {
        let root = tempfile::tempdir().unwrap();
        let socket = root.path().join("engine.sock");
        let euid = unsafe { libc::geteuid() };
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        // No child process may be forked while the first listener exists: it would inherit
        // the descriptor and keep the dropped socket live.
        let _no_fork = crate::FORK_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let listener = StdUnixListener::bind(&socket).unwrap();
        drop(listener);
        // The returned connection refusal marks this owned socket as stale.
        let listener = EngineServer::bind(
            socket.clone(),
            euid,
            Arc::new(MockPeerEuid(euid)),
            Arc::new(Dispatcher::core(test_context())),
        )
        .unwrap();
        assert_eq!(
            std::fs::symlink_metadata(&socket)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        drop(listener);
        assert!(!socket.exists());
    }

    /// A registry whose `guest.ssh` opens a loopback pair and shares the "allowed" flag with the
    /// test, in place of a computer.
    struct StreamRegistry {
        guest: Mutex<Option<std::net::TcpStream>>,
        allowed: Arc<std::sync::atomic::AtomicBool>,
        calls: Mutex<Vec<Value>>,
    }

    impl crate::dispatcher::MethodRegistry for StreamRegistry {
        fn descriptors(&self) -> &[silo_api::methods::MethodDescriptor] {
            silo_api::methods::method_registry()
        }

        fn dispatch<'a>(
            &'a self,
            method: silo_api::methods::MethodName,
            _params: Value,
            _context: Arc<crate::context::SilodContext>,
            _request: crate::dispatcher::RequestContext,
        ) -> crate::dispatcher::HandlerFuture<'a> {
            Box::pin(async move {
                Err(crate::dispatcher::DispatchError::unsupported(
                    crate::dispatcher::method_name(method),
                ))
            })
        }

        fn open_stream<'a>(
            &'a self,
            _method: silo_api::methods::MethodName,
            params: Value,
            _context: Arc<crate::context::SilodContext>,
            _request: crate::dispatcher::RequestContext,
        ) -> crate::dispatcher::StreamFuture<'a> {
            Box::pin(async move {
                self.calls.lock().unwrap().push(params);
                let stream = self.guest.lock().unwrap().take().unwrap();
                let allowed = self.allowed.clone();
                Ok(crate::dispatcher::OpenedStream {
                    stream,
                    allowed: Box::new(move || allowed.load(std::sync::atomic::Ordering::Acquire)),
                })
            })
        }
    }

    async fn handshaken_client(socket: &Path) -> UnixStream {
        let mut client = UnixStream::connect(socket).await.unwrap();
        write_json_frame(
            &mut client,
            &serde_json::json!({
                "id": "handshake",
                "method": "engine.handshake",
                "params": { "protocol": { "min": 6, "max": 6 } }
            }),
        )
        .await
        .unwrap();
        let _: Value = read_json_frame(&mut client).await.unwrap().unwrap();
        client
    }

    #[tokio::test]
    async fn guest_ssh_is_a_one_shot_stream_after_the_canonical_handshake() {
        let root = tempfile::tempdir().unwrap();
        let socket = root.path().join("engine.sock");
        let euid = unsafe { libc::geteuid() };
        let server = EngineServer::bind(
            socket.clone(),
            euid,
            Arc::new(MockPeerEuid(euid)),
            Arc::new(Dispatcher::core(test_context())),
        )
        .unwrap();
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server_task = tokio::spawn(server.run(shutdown_rx));
        let mut client = UnixStream::connect(&socket).await.unwrap();
        write_json_frame(
            &mut client,
            &serde_json::json!({
                "id": "handshake",
                "method": "engine.handshake",
                "params": { "protocol": { "min": 6, "max": 6 } }
            }),
        )
        .await
        .unwrap();
        let handshake: Value = read_json_frame(&mut client).await.unwrap().unwrap();
        assert!(handshake["result"]["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|method| method == "guest.ssh"));

        write_json_frame(
            &mut client,
            &serde_json::json!({
                "id": "stream",
                "method": "guest.ssh",
                "params": { "computerId": "not-a-uuid" }
            }),
        )
        .await
        .unwrap();
        let refusal: Value = read_json_frame(&mut client).await.unwrap().unwrap();
        assert_eq!(refusal["id"], "stream");
        assert_eq!(refusal["error"]["code"], "internal");
        let mut byte = [0_u8; 1];
        assert_eq!(
            tokio::io::AsyncReadExt::read(&mut client, &mut byte)
                .await
                .unwrap(),
            0
        );

        let _ = shutdown_tx.send(());
        server_task.await.unwrap();
    }

    #[tokio::test]
    async fn guest_ssh_is_rejected_after_a_regular_request() {
        let root = tempfile::tempdir().unwrap();
        let socket = root.path().join("engine.sock");
        let euid = unsafe { libc::geteuid() };
        let server = EngineServer::bind(
            socket.clone(),
            euid,
            Arc::new(MockPeerEuid(euid)),
            Arc::new(Dispatcher::core(test_context())),
        )
        .unwrap();
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server_task = tokio::spawn(server.run(shutdown_rx));
        let mut client = handshaken_client(&socket).await;
        write_json_frame(
            &mut client,
            &serde_json::json!({ "id": "state", "method": "engine.state", "params": {} }),
        )
        .await
        .unwrap();
        let _: Value = read_json_frame(&mut client).await.unwrap().unwrap();
        write_json_frame(
            &mut client,
            &serde_json::json!({
                "id": "late-stream",
                "method": "guest.ssh",
                "params": { "computerId": "not-a-uuid" }
            }),
        )
        .await
        .unwrap();
        let refusal: Value = read_json_frame(&mut client).await.unwrap().unwrap();
        assert_eq!(refusal["id"], "late-stream");
        assert_eq!(
            refusal["error"]["message"],
            "guest.ssh must be the first request after engine.handshake."
        );

        let _ = shutdown_tx.send(());
        server_task.await.unwrap();
    }

    #[tokio::test]
    async fn guest_stream_relays_bytes_and_ends_when_the_stream_is_revoked() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let guest_client = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (guest_server, _) = listener.accept().unwrap();
        let registry = Arc::new(StreamRegistry {
            guest: Mutex::new(Some(guest_client)),
            allowed: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            calls: Mutex::new(Vec::new()),
        });
        let allowed = registry.allowed.clone();
        let root = tempfile::tempdir().unwrap();
        let socket = root.path().join("engine.sock");
        let euid = unsafe { libc::geteuid() };
        let server = EngineServer::bind(
            socket.clone(),
            euid,
            Arc::new(MockPeerEuid(euid)),
            Arc::new(Dispatcher::new(registry.clone(), test_context())),
        )
        .unwrap();
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server_task = tokio::spawn(server.run(shutdown_rx));
        let mut client = handshaken_client(&socket).await;
        write_json_frame(
            &mut client,
            &serde_json::json!({
                "id": "forward",
                "method": "guest.ssh",
                "params": {
                    "computerId": "c690f54f-4cc9-4950-8782-30f21e89d18b",
                    "purpose": "port",
                    "port": 3000
                }
            }),
        )
        .await
        .unwrap();
        let accepted: Value = read_json_frame(&mut client).await.unwrap().unwrap();
        assert_eq!(accepted["result"], serde_json::json!({}));
        assert_eq!(registry.calls.lock().unwrap()[0]["port"], 3000);

        client.write_all(b"ssh-in").await.unwrap();
        let mut guest_server = tokio::net::TcpStream::from_std({
            guest_server.set_nonblocking(true).unwrap();
            guest_server
        })
        .unwrap();
        let mut received = [0_u8; 6];
        guest_server.read_exact(&mut received).await.unwrap();
        assert_eq!(&received, b"ssh-in");

        // Revocation ends the relay: the client observes the end of its stream.
        allowed.store(false, std::sync::atomic::Ordering::Release);
        let mut byte = [0_u8; 1];
        let read = time::timeout(Duration::from_secs(5), client.read(&mut byte))
            .await
            .expect("a revoked stream must end");
        assert!(matches!(read, Ok(0) | Err(_)));

        let _ = shutdown_tx.send(());
        server_task.await.unwrap();
    }

    #[tokio::test]
    async fn relay_while_allowed_stops_when_the_check_turns_false() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let (mut client, _client_peer) = tokio::io::duplex(64);
        let (mut guest, _guest_peer) = tokio::io::duplex(64);
        let allowed = Arc::new(AtomicBool::new(true));
        let check = allowed.clone();
        let relay = tokio::spawn(async move {
            relay_while_allowed(&mut client, &mut guest, &move || {
                check.load(Ordering::Acquire)
            })
            .await;
        });
        allowed.store(false, Ordering::Release);
        time::timeout(Duration::from_secs(5), relay)
            .await
            .expect("the relay must observe revocation")
            .unwrap();
    }

    fn test_context() -> Arc<crate::context::SilodContext> {
        test_context_with_paths(silo_engine::context::EnginePaths::default())
    }

    fn test_context_with_paths(
        paths: silo_engine::context::EnginePaths,
    ) -> Arc<crate::context::SilodContext> {
        use crate::{context::SilodContext, identity::LocalDeviceIdentity};
        use silo_engine::{
            channel::Channel,
            context::{EngineIdentity, MainThread, PackageKind, ProcessControl},
        };

        struct Main;
        impl MainThread for Main {
            fn run(&self, _work: Box<dyn FnOnce() + Send>) -> Result<(), String> {
                Ok(())
            }
        }
        struct Process;
        impl ProcessControl for Process {
            fn exit(&self, _code: i32) {}
            fn restart(&self) -> ! {
                panic!("restart not expected")
            }
        }
        Arc::new(SilodContext::new(
            paths,
            EngineIdentity {
                channel: Channel::Development,
                version: "0.1.0".into(),
                package_kind: PackageKind::MacApp,
            },
            LocalDeviceIdentity {
                device_id: "test-device".into(),
                name: "Test Mac".into(),
            },
            Arc::new(Main),
            Arc::new(Process),
        ))
    }
}
