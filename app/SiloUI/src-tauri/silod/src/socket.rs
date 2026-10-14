use std::{
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
use silo_api::transport::ResponseEnvelope;
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
const MAX_LOCAL_SESSIONS: usize = 64;
const MAX_INFLIGHT_PER_SESSION: usize = 32;
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
    // guest.ssh changes the connection to raw bytes, so it is accepted only here, once.
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
        let (response, guest) = dispatcher.execute_stream(prepared).await;
        if let Some(response) = response {
            if let Err(error) = write_initial_response(&mut stream, &response).await {
                eprintln!("silod: guest stream response failed: {error}");
                return;
            }
        }
        let Some(guest) = guest else {
            return;
        };
        if let Err(error) = guest.set_nonblocking(true) {
            eprintln!("silod: guest stream setup failed: {error}");
            return;
        }
        let mut guest = match tokio::net::TcpStream::from_std(guest) {
            Ok(guest) => guest,
            Err(error) => {
                eprintln!("silod: guest stream setup failed: {error}");
                return;
            }
        };
        if let Err(error) = tokio::io::copy_bidirectional(&mut stream, &mut guest).await {
            eprintln!("silod: guest stream ended: {error}");
        }
        return;
    }

    let (mut reader, mut writer) = stream.into_split();

    let (reply_tx, mut reply_rx) = mpsc::channel::<OutboundReply>(MAX_PENDING_REPLIES);
    let reply_byte_budget = Arc::new(Semaphore::new(MAX_PENDING_REPLY_BYTES));
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
        while let Some(outbound) = reply_rx.recv().await {
            let result = match time::timeout(
                REPLY_WRITE_TIMEOUT,
                write_frame_bytes(&mut writer, &outbound.bytes),
            )
            .await
            {
                Ok(result) => result,
                Err(_) => Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "engine.sock client did not accept a reply within 30 seconds",
                )),
            };
            if let Some(ack) = outbound.ack {
                let _ = ack.send(
                    result
                        .as_ref()
                        .map(|_| ())
                        .map_err(|error| error.to_string()),
                );
            }
            if let Err(error) = result {
                return Err(error);
            }
            drop(outbound.byte_permit);
        }
        Ok::<(), io::Error>(())
    });
    let mut tasks = JoinSet::new();
    let session_request_permits = Arc::new(Semaphore::new(MAX_INFLIGHT_PER_SESSION));
    let mut close_after_reply = false;
    let mut writer_result = None;
    loop {
        let read_result = tokio::select! {
            frame = frame_rx.recv() => frame,
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
                if method == silo_api::methods::MethodName::GuestSsh {
                    if enqueue_response(
                        &reply_tx,
                        &reply_byte_budget,
                        dispatcher.stream_not_first_response(id),
                        None,
                    )
                    .await
                    .is_err()
                    {
                        break;
                    }
                    close_after_reply = true;
                    break;
                }
                let Ok(session_permit) = session_request_permits.clone().try_acquire_owned() else {
                    if enqueue_response(
                        &reply_tx,
                        &reply_byte_budget,
                        dispatcher.busy_response(id),
                        None,
                    )
                    .await
                    .is_err()
                    {
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
                    if enqueue_response(
                        &reply_tx,
                        &reply_byte_budget,
                        dispatcher.busy_response(id),
                        None,
                    )
                    .await
                    .is_err()
                    {
                        break;
                    }
                    continue;
                };
                let dispatcher = dispatcher.clone();
                let reply_tx = reply_tx.clone();
                let reply_byte_budget = reply_byte_budget.clone();
                tasks.spawn(async move {
                    let invoke = PreparedRequest::Invoke {
                        id,
                        method,
                        params,
                        request_context,
                    };
                    let response = dispatcher.execute(invoke).await.response;
                    if let Some(response) = response {
                        let _ =
                            enqueue_response(&reply_tx, &reply_byte_budget, response, None).await;
                    }
                    drop((session_permit, request_permit));
                });
            }
            PreparedRequest::Reply(response) => {
                if enqueue_response(&reply_tx, &reply_byte_budget, response, None)
                    .await
                    .is_err()
                {
                    break;
                }
            }
            PreparedRequest::Close(response, reason) => {
                eprintln!("silod: closing client session: {reason}");
                if let Some(response) = response {
                    let (ack_tx, ack_rx) = oneshot::channel();
                    if enqueue_response(&reply_tx, &reply_byte_budget, response, Some(ack_tx))
                        .await
                        .is_ok()
                    {
                        let _ = ack_rx.await;
                    }
                }
                close_after_reply = true;
                break;
            }
        }
    }
    if close_after_reply {
        tasks.abort_all();
    }
    reader_task.abort();
    let _ = reader_task.await;
    while tasks.join_next().await.is_some() {}
    drop(reply_tx);
    if let Some(result) = writer_result {
        match result {
            Ok(Ok(())) => {}
            Ok(Err(error)) => eprintln!("silod: socket writer failed: {error}"),
            Err(error) => eprintln!("silod: socket writer task failed: {error}"),
        }
    } else if let Err(error) = writer_task.await {
        eprintln!("silod: socket writer task failed: {error}");
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
}

async fn enqueue_response(
    sender: &mpsc::Sender<OutboundReply>,
    byte_budget: &Arc<Semaphore>,
    response: ResponseEnvelope<Value>,
    ack: Option<oneshot::Sender<Result<(), String>>>,
) -> Result<(), String> {
    let bytes = serde_json::to_vec(&response).map_err(|error| error.to_string())?;
    if bytes.is_empty() || bytes.len() > crate::framing::MAX_FRAME_BYTES {
        return Err(format!(
            "reply frame length {} exceeds the frame limit",
            bytes.len()
        ));
    }
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
        })
        .await
        .map_err(|error| error.to_string())
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
    use std::os::unix::fs::PermissionsExt;

    struct MockPeerEuid(u32);
    impl PeerEuidProvider for MockPeerEuid {
        fn peer_euid(&self, _fd: i32) -> io::Result<u32> {
            Ok(self.0)
        }
    }

    #[test]
    fn mocked_foreign_euid_is_rejected() {
        let error = check_peer_euid(&MockPeerEuid(500), 0, 501).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
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
                "params": {
                    "protocol": { "min": 6, "max": 6 },
                    "client": { "name": "test", "version": "0.1.0" }
                }
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
        let _: Value = read_json_frame(&mut client).await.unwrap().unwrap();
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
    async fn bound_socket_is_private_and_removed_by_inode_on_shutdown() {
        let root = tempfile::tempdir().unwrap();
        let socket = root.path().join("engine.sock");
        let euid = unsafe { libc::geteuid() };
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
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

    fn test_context() -> Arc<crate::context::SilodContext> {
        use crate::{context::SilodContext, identity::LocalDeviceIdentity};
        use silo_engine::{
            channel::Channel,
            context::{EngineIdentity, EnginePaths, MainThread, PackageKind, ProcessControl},
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
            EnginePaths::default(),
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
