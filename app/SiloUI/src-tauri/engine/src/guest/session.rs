//! Long-running guest executions (`guest.exec.start`) as engine-owned jobs.
//!
//! The engine, not the client, owns the process: it keeps running when the requester
//! disconnects. Output is published as `guest.exec.output` events with a per-job sequence and
//! kept in a bounded replay ring for `guest.exec.get`. The exit is saved in the job registry
//! before `operations.job.finished` goes out, so a completed outcome survives an engine
//! restart until the client acknowledges it. A process that was still running when the engine
//! died is not tracked after the restart: its job reads back as `lost`, and nothing re-runs it.
use super::{
    exec::{self, terminate, validate_request, GuestUser},
    process::{self, StreamKind},
    target::Target,
    GuestError, MAX_RESPONSE_RAW_BYTES,
};
use crate::{
    context::{Audience, EngineContext, Event},
    jobs::CancelRoute,
};
use silo_api::{
    events::{GuestExecExitedEvent, GuestExecOutputEvent},
    jobs::{ErrorCode, JobId, JobState},
    methods::guest::GuestExecStream,
};
use std::{
    collections::{HashMap, VecDeque},
    io::Write,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc, Arc, Condvar, Mutex, OnceLock,
    },
    thread,
    time::{Duration, Instant},
};

/// Chunks kept for replay, whatever their sizes: byte budgets alone would not bound the
/// per-entry overhead of one-byte writes.
const MAX_RETAINED_CHUNKS: usize = 1024;
/// Input accepted but not yet written to the guest.
const MAX_PENDING_INPUT_BYTES: usize = 8 * 1024 * 1024;
const MAX_FINISHED_SESSIONS: usize = 64;
const FINISHED_RETENTION: Duration = Duration::from_secs(3600);
/// How long a cancelled run's host process may linger after the guest tree is gone.
#[cfg(not(test))]
const HOST_EXIT_GRACE: Duration = Duration::from_secs(5);
#[cfg(test)]
const HOST_EXIT_GRACE: Duration = Duration::from_millis(300);

/// The method whose accepted jobs are exec jobs.
pub const EXEC_METHOD: &str = "guest.exec.start";

pub use silo_api::methods::guest::GuestExecState as ExecState;

/// What `guest.exec.start` asks for.
#[derive(Clone, Debug)]
pub struct ExecStart {
    pub argv: Vec<String>,
    pub cwd: Option<String>,
    pub env: Vec<(String, String)>,
    pub user: GuestUser,
    pub stdin: Option<Vec<u8>>,
    pub stdin_open: bool,
    pub retain_output_bytes: u64,
}

/// A validated start: the computer is running and the command is built, but nothing runs yet.
pub struct StartedExec {
    target: Target,
    plan: process::Plan,
    token: String,
    root: bool,
    stdin: Option<Vec<u8>>,
    stdin_open: bool,
    retain: usize,
}

impl StartedExec {
    /// Validates `request` and builds the command. Fails without any effect, so a caller can
    /// refuse before it accepts a job.
    pub fn prepare(target: &Target, request: &ExecStart) -> Result<Self, GuestError> {
        validate_request(&request.argv, request.cwd.as_deref(), &request.env)?;
        if request.retain_output_bytes > MAX_RESPONSE_RAW_BYTES as u64 {
            return Err(GuestError::invalid(format!(
                "Invalid params: retainOutputBytes is at most {MAX_RESPONSE_RAW_BYTES}, so a full replay fits one response."
            )));
        }
        target.ensure_running()?;
        let root = request.user == GuestUser::Root;
        let token = uuid::Uuid::new_v4().simple().to_string();
        let argv = exec::supervised(&token, request.cwd.as_deref(), &request.argv);
        let wants_stdin = request.stdin.is_some() || request.stdin_open;
        let plan = target.plan(&argv, &request.env, root, wants_stdin)?;
        Ok(Self {
            target: target.clone(),
            plan,
            token,
            root,
            stdin: request.stdin.clone(),
            stdin_open: request.stdin_open,
            retain: request.retain_output_bytes as usize,
        })
    }

    /// Runs the command as job `job_id`. The job is marked running, cancellable through
    /// `operations.cancel`, and finished with its exit (or a failure) when the process ends.
    pub fn launch(self, context: Arc<dyn EngineContext>, job_id: JobId) -> Result<(), GuestError> {
        let registry = context
            .job_registry()
            .map_err(|message| GuestError::internal(message))?;
        registry
            .mark_running(&job_id)
            .map_err(|error| GuestError::internal(error.to_string()))?;
        let session = Arc::new(Session::new(
            job_id.clone(),
            self.target.clone(),
            self.token.clone(),
            self.root,
            self.retain,
            self.stdin.is_some() || self.stdin_open,
        ));
        let chunks = {
            let (session, context) = (session.clone(), context.clone());
            Arc::new(move |kind: StreamKind, data: Vec<u8>| session.output(&*context, kind, data))
        };
        let wants_stdin = self.stdin.is_some() || self.stdin_open;
        let mut streaming = match process::spawn_streaming(&self.plan, wants_stdin, chunks) {
            Ok(streaming) => streaming,
            Err(error) => {
                session.finish_failed(&*context, error.code, &error.message);
                sessions().insert(job_id, session);
                return Ok(());
            }
        };
        register(&session, &*registry);
        sessions().insert(job_id.clone(), session.clone());
        prune();
        if let Some(pipe) = streaming.stdin.take() {
            let (sender, receiver) = mpsc::channel();
            *session
                .input
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(sender.clone());
            let pending = session.pending_input.clone();
            let initial = self.stdin.unwrap_or_default();
            let keep_open = self.stdin_open;
            thread::spawn(move || write_input(pipe, initial, keep_open, receiver, pending));
        }
        let child = Arc::new(Mutex::new(streaming.child));
        *session
            .child
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(child.clone());
        let readers = std::mem::take(&mut streaming.readers);
        thread::spawn(move || wait_for_exit(context, session, child, readers));
        Ok(())
    }
}

fn register(session: &Arc<Session>, registry: &crate::jobs::JobRegistry) {
    let cancelling = session.clone();
    registry.register_cancel(session.job_id.clone(), CancelRoute::GuestExec, move || {
        cancelling.request_cancel();
        Ok(())
    });
}

enum Input {
    Data(Vec<u8>),
    Close,
}

fn write_input(
    mut pipe: std::process::ChildStdin,
    initial: Vec<u8>,
    keep_open: bool,
    receiver: mpsc::Receiver<Input>,
    pending: Arc<AtomicUsize>,
) {
    if !initial.is_empty() && pipe.write_all(&initial).is_err() {
        return;
    }
    if !keep_open {
        return;
    }
    while let Ok(message) = receiver.recv() {
        match message {
            Input::Data(bytes) => {
                let length = bytes.len();
                let written = pipe.write_all(&bytes).and_then(|()| pipe.flush());
                pending.fetch_sub(length, Ordering::SeqCst);
                if written.is_err() {
                    return;
                }
            }
            Input::Close => return,
        }
    }
}

struct Chunk {
    seq: u64,
    stream: GuestExecStream,
    data: Vec<u8>,
}

struct Progress {
    next_seq: u64,
    ring: VecDeque<Chunk>,
    ring_bytes: usize,
    state: ExecState,
    exit_code: Option<i32>,
    finished_at: Option<Instant>,
}

struct Session {
    job_id: JobId,
    target: Target,
    token: String,
    root: bool,
    retain: usize,
    accepts_input: bool,
    cancel: AtomicBool,
    /// Set once the host process has ended.
    host_exited: AtomicBool,
    cancelled_at: Mutex<Option<Instant>>,
    /// Why ending the guest process tree failed after a cancellation, once known.
    stop_failure: Mutex<Option<String>>,
    /// Set (with a wake-up) when the cancellation's attempt to end the tree has finished.
    stop_finished: (Mutex<bool>, Condvar),
    input: Mutex<Option<mpsc::Sender<Input>>>,
    pending_input: Arc<AtomicUsize>,
    child: Mutex<Option<Arc<Mutex<std::process::Child>>>>,
    /// The first bytes of standard error, to explain a failed connection.
    stderr_head: Mutex<Vec<u8>>,
    progress: Mutex<Progress>,
}

impl Session {
    fn new(
        job_id: JobId,
        target: Target,
        token: String,
        root: bool,
        retain: usize,
        accepts_input: bool,
    ) -> Self {
        Self {
            job_id,
            target,
            token,
            root,
            retain,
            accepts_input,
            cancel: AtomicBool::new(false),
            host_exited: AtomicBool::new(false),
            cancelled_at: Mutex::new(None),
            stop_failure: Mutex::new(None),
            stop_finished: (Mutex::new(false), Condvar::new()),
            input: Mutex::new(None),
            pending_input: Arc::new(AtomicUsize::new(0)),
            child: Mutex::new(None),
            stderr_head: Mutex::new(Vec::new()),
            progress: Mutex::new(Progress {
                next_seq: 1,
                ring: VecDeque::new(),
                ring_bytes: 0,
                state: ExecState::Running,
                exit_code: None,
                finished_at: None,
            }),
        }
    }

    fn progress(&self) -> std::sync::MutexGuard<'_, Progress> {
        self.progress
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Numbers a chunk, publishes it and keeps it for replay. The sequence is one stream of
    /// numbers across stdout and stderr, apart from the event envelope's own sequence.
    fn output(&self, context: &dyn EngineContext, kind: StreamKind, data: Vec<u8>) {
        let stream = match kind {
            StreamKind::Stdout => GuestExecStream::Stdout,
            StreamKind::Stderr => GuestExecStream::Stderr,
        };
        if kind == StreamKind::Stderr {
            let mut head = self
                .stderr_head
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let room = 1024usize.saturating_sub(head.len());
            head.extend_from_slice(&data[..data.len().min(room)]);
        }
        let mut progress = self.progress();
        let seq = progress.next_seq;
        progress.next_seq += 1;
        let event = GuestExecOutputEvent {
            job_id: self.job_id.clone(),
            seq,
            stream,
            data: data.clone(),
        };
        if let Ok(payload) = serde_json::to_value(&event) {
            context.publish(Event::new("guest.exec.output", payload), Audience::All);
        }
        if data.len() > self.retain {
            // A chunk the budget cannot hold is not kept, and neither is anything before it:
            // the replay is always a contiguous run of the newest chunks, and
            // `firstRetainedSeq` says where it starts (one past this chunk, until a later
            // chunk fits).
            progress.ring.clear();
            progress.ring_bytes = 0;
        } else {
            progress.ring_bytes += data.len();
            progress.ring.push_back(Chunk { seq, stream, data });
            while progress.ring_bytes > self.retain || progress.ring.len() > MAX_RETAINED_CHUNKS {
                match progress.ring.pop_front() {
                    Some(evicted) => progress.ring_bytes -= evicted.data.len(),
                    None => break,
                }
            }
        }
    }

    fn request_cancel(self: &Arc<Self>) {
        if self.progress().finished_at.is_some() || self.cancel.swap(true, Ordering::SeqCst) {
            return;
        }
        *self
            .cancelled_at
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Instant::now());
        let session = self.clone();
        thread::spawn(move || {
            // The guest tree first: SIGTERM, a grace period, SIGKILL, confirmed gone.
            let failure = match terminate(&session.target, session.root, &session.token) {
                Ok(true) => None,
                Ok(false) => Some("Some of its processes are still running.".to_owned()),
                Err(error) => Some(error.message),
            };
            *session
                .stop_failure
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = failure;
            session.mark_stop_finished();
            // The host process ends with the guest command; if it lingers, end it too.
            let deadline = Instant::now() + HOST_EXIT_GRACE;
            while !session.host_exited.load(Ordering::SeqCst) && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(50));
            }
            if !session.host_exited.load(Ordering::SeqCst) {
                session.kill_host();
            }
        });
    }

    fn mark_stop_finished(&self) {
        let (finished, wake) = &self.stop_finished;
        *finished
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
        wake.notify_all();
    }

    /// Waits for the cancellation's attempt to end the guest tree and returns why it failed,
    /// if it did. The attempt is bounded by the control commands' own timeouts.
    fn stop_failure(&self) -> Option<String> {
        let (finished, wake) = &self.stop_finished;
        let mut done = finished
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while !*done {
            done = wake
                .wait(done)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        drop(done);
        self.stop_failure
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn kill_host(&self) {
        let child = self
            .child
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if let Some(child) = child {
            let _ = child
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .kill();
        }
    }

    fn publish_exit(&self, context: &dyn EngineContext, code: Option<i32>, signal: Option<String>) {
        let event = GuestExecExitedEvent {
            job_id: self.job_id.clone(),
            exit_code: code,
            signal,
        };
        if let Ok(payload) = serde_json::to_value(&event) {
            context.publish(Event::new("guest.exec.exited", payload), Audience::All);
        }
    }

    fn settle(&self, state: ExecState, code: Option<i32>) {
        let mut progress = self.progress();
        progress.state = state;
        progress.exit_code = code;
        progress.finished_at = Some(Instant::now());
    }

    fn finish_exited(
        &self,
        context: &dyn EngineContext,
        code: Option<i32>,
        signal: Option<String>,
    ) {
        self.settle(ExecState::Exited, code);
        self.publish_exit(context, code, signal.clone());
        let result = silo_api::methods::guest::GuestExecFinishedResult {
            exit_code: code,
            signal,
        };
        let value = serde_json::to_value(result).unwrap_or(serde_json::Value::Null);
        let _ = crate::jobs::finish_with_result(context, self.job_id.clone(), value, Audience::All);
    }

    fn finish_cancelled(
        &self,
        context: &dyn EngineContext,
        code: Option<i32>,
        signal: Option<String>,
    ) {
        self.settle(ExecState::Cancelled, code);
        self.publish_exit(context, code, signal);
        let _ = crate::jobs::finish_with_error(
            context,
            self.job_id.clone(),
            ErrorCode::Cancelled,
            "The command was cancelled.",
            Audience::All,
        );
    }

    fn finish_failed(&self, context: &dyn EngineContext, code: ErrorCode, message: &str) {
        self.settle(ExecState::Lost, None);
        self.publish_exit(context, None, None);
        let _ = crate::jobs::finish_with_error(
            context,
            self.job_id.clone(),
            code,
            message.to_owned(),
            Audience::All,
        );
    }
}

fn wait_for_exit(
    context: Arc<dyn EngineContext>,
    session: Arc<Session>,
    child: Arc<Mutex<std::process::Child>>,
    readers: Vec<thread::JoinHandle<()>>,
) {
    let status = loop {
        let waited = child
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .try_wait();
        match waited {
            Ok(Some(status)) => {
                session.host_exited.store(true, Ordering::SeqCst);
                break Ok(status);
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(error) => break Err(error),
        }
    };
    drain_readers(readers, &session.cancel);
    session
        .input
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take();
    let cancelled = session.cancel.load(Ordering::SeqCst);
    if cancelled {
        if let Some(reason) = session.stop_failure() {
            session.finish_failed(
                &*context,
                ErrorCode::Internal,
                &format!("The command could not be stopped in the computer. {reason}"),
            );
            return;
        }
    }
    let status = match status {
        Ok(status) => status,
        Err(error) => {
            session.finish_failed(
                &*context,
                ErrorCode::Internal,
                &format!("Silo lost the guest command: {error}"),
            );
            return;
        }
    };
    let Some(code) = status.code() else {
        if cancelled {
            session.finish_cancelled(&*context, None, None);
        } else {
            session.finish_failed(
                &*context,
                ErrorCode::Internal,
                "The connection to the computer ended unexpectedly.",
            );
        }
        return;
    };
    let stderr_head = String::from_utf8_lossy(
        &session
            .stderr_head
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    )
    .into_owned();
    let settled = exec::settle_exit(
        &session.target,
        session.root,
        &session.token,
        code,
        &stderr_head,
    );
    if cancelled {
        let (exit_code, signal) = settled.unwrap_or((None, None));
        session.finish_cancelled(&*context, exit_code, signal);
        return;
    }
    match settled {
        Ok((exit_code, signal)) => session.finish_exited(&*context, exit_code, signal),
        Err(error) => session.finish_failed(&*context, error.code, &error.message),
    }
}

/// Waits for the output readers to reach end-of-file. A descendant of the command that
/// outlived it could hold the pipes open, so once a cancellation is requested (at any point
/// of the wait) the wait ends shortly after and the readers are left to finish on their own.
fn drain_readers(readers: Vec<thread::JoinHandle<()>>, cancel: &AtomicBool) {
    let mut cancelled_at: Option<Instant> = None;
    while !readers.iter().all(thread::JoinHandle::is_finished) {
        if cancel.load(Ordering::SeqCst) {
            let since = *cancelled_at.get_or_insert_with(Instant::now);
            if since.elapsed() >= process::CLEANUP_GRACE {
                return;
            }
        }
        thread::sleep(Duration::from_millis(5));
    }
    for reader in readers {
        let _ = reader.join();
    }
}

fn sessions() -> std::sync::MutexGuard<'static, HashMap<JobId, Arc<Session>>> {
    static SESSIONS: OnceLock<Mutex<HashMap<JobId, Arc<Session>>>> = OnceLock::new();
    SESSIONS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Drops the oldest finished sessions beyond the retention limits.
fn prune() {
    let mut sessions = sessions();
    let mut finished: Vec<(JobId, Instant)> = sessions
        .iter()
        .filter_map(|(id, session)| session.progress().finished_at.map(|at| (id.clone(), at)))
        .collect();
    finished.sort_by_key(|(_, at)| *at);
    let excess = finished.len().saturating_sub(MAX_FINISHED_SESSIONS);
    for (index, (id, at)) in finished.into_iter().enumerate() {
        if index < excess || at.elapsed() > FINISHED_RETENTION {
            sessions.remove(&id);
        }
    }
}

/// Sends input to a running job's standard input, or closes it.
///
/// Errors: `not_running` for an unknown or finished job, `stdin_closed` once input was
/// closed or if the job never took input, `busy` when too much input is already queued.
pub fn exec_input(job_id: &JobId, data: Option<Vec<u8>>, close: bool) -> Result<(), GuestError> {
    let session = sessions().get(job_id).cloned();
    let Some(session) = session else {
        return Err(GuestError::new(
            ErrorCode::NotRunning,
            "The command is not running.",
        ));
    };
    if session.progress().state != ExecState::Running {
        return Err(GuestError::new(
            ErrorCode::NotRunning,
            "The command is not running.",
        ));
    }
    let mut input = session
        .input
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let closed = || GuestError::new(ErrorCode::StdinClosed, "The command's input is closed.");
    if !session.accepts_input {
        return Err(closed());
    }
    let Some(sender) = input.as_ref() else {
        return Err(closed());
    };
    if let Some(data) = data.filter(|data| !data.is_empty()) {
        if session.pending_input.load(Ordering::SeqCst) + data.len() > MAX_PENDING_INPUT_BYTES {
            return Err(GuestError::new(
                ErrorCode::Busy,
                "The command has not read its earlier input yet.",
            ));
        }
        session
            .pending_input
            .fetch_add(data.len(), Ordering::SeqCst);
        if sender.send(Input::Data(data)).is_err() {
            return Err(closed());
        }
    }
    if close {
        let _ = sender.send(Input::Close);
        input.take();
    }
    Ok(())
}

/// A job's current state, exit and replayable output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecSnapshot {
    pub state: ExecState,
    pub exit_code: Option<i32>,
    /// `(seq, stream, data)` for every retained chunk after `since_seq`.
    pub output: Vec<(u64, GuestExecStream, Vec<u8>)>,
    /// The sequence of the oldest chunk still retained; one past the newest when none is.
    pub first_retained_seq: u64,
}

/// Reads a job's state and replays the output it still holds after `since_seq` (the last
/// sequence the caller has seen; omitted means everything retained).
///
/// A job the engine still tracks answers from memory. Otherwise a finished job answers from
/// its saved outcome with no output, and a job that was running when the engine restarted is
/// `lost`.
pub fn exec_get(
    context: &dyn EngineContext,
    job_id: &JobId,
    since_seq: Option<u64>,
) -> Result<ExecSnapshot, GuestError> {
    let lookup = crate::jobs::get_at(context, job_id, None)
        .map_err(|error| GuestError::internal(error.to_string()))?
        .ok_or_else(|| GuestError::not_found("The exec job was not found."))?;
    // Only jobs started by `guest.exec.start` are exec jobs; another domain's job is not one.
    let method = context
        .job_registry()
        .map_err(GuestError::internal)?
        .method_of(job_id)
        .map_err(|error| GuestError::internal(error.to_string()))?;
    if method.as_deref() != Some(EXEC_METHOD) {
        return Err(GuestError::not_found("The exec job was not found."));
    }
    let session = sessions().get(job_id).cloned();
    if let Some(session) = session {
        let progress = session.progress();
        let since = since_seq.unwrap_or(0);
        return Ok(ExecSnapshot {
            state: progress.state,
            exit_code: progress.exit_code,
            output: progress
                .ring
                .iter()
                .filter(|chunk| chunk.seq > since)
                .map(|chunk| (chunk.seq, chunk.stream, chunk.data.clone()))
                .collect(),
            first_retained_seq: progress
                .ring
                .front()
                .map_or(progress.next_seq, |chunk| chunk.seq),
        });
    }
    let (state, exit_code) = match (lookup.state, lookup.outcome) {
        (JobState::Finished, Some(outcome)) => match outcome.error {
            Some(error) if error.code == ErrorCode::Cancelled => (ExecState::Cancelled, None),
            Some(_) => (ExecState::Lost, None),
            None => (
                ExecState::Exited,
                outcome
                    .result
                    .as_ref()
                    .and_then(|result| result.get("exitCode"))
                    .and_then(serde_json::Value::as_i64)
                    .and_then(|code| i32::try_from(code).ok()),
            ),
        },
        (JobState::Finished, None) => (ExecState::Lost, None),
        // Running or waiting with no process the engine tracks.
        _ => (ExecState::Lost, None),
    };
    Ok(ExecSnapshot {
        state,
        exit_code,
        output: Vec::new(),
        first_retained_seq: since_seq.map_or(0, |since| since.saturating_add(1)),
    })
}

/// Cancels a tracked job: SIGTERM, a grace period, SIGKILL, with the exit confirmed in the
/// guest. This is the route behind `operations.cancel` for exec jobs.
pub fn cancel_exec(job_id: &JobId) -> Result<(), GuestError> {
    let session = sessions().get(job_id).cloned();
    match session {
        Some(session) => {
            session.request_cancel();
            Ok(())
        }
        None => Err(GuestError::not_found("The exec job was not found.")),
    }
}

/// Forgets a finished job's in-memory replay once the client has acknowledged it.
pub fn forget_finished(job_id: &JobId) {
    let mut sessions = sessions();
    if sessions
        .get(job_id)
        .is_some_and(|session| session.progress().finished_at.is_some())
    {
        sessions.remove(job_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guest::test_support::{self, RecordingContext};

    fn start(
        directory: &std::path::Path,
        context: &Arc<RecordingContext>,
        request: ExecStart,
    ) -> JobId {
        let target = test_support::target(directory);
        let prepared = StartedExec::prepare(&target, &request).unwrap();
        // Every test has its own registry, so ids are made unique for the shared session map.
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1_000_000);
        let number = NEXT.fetch_add(1, Ordering::SeqCst).to_string();
        let accepted =
            crate::jobs::accept_domain_id(&**context, "job", &number, "guest.exec.start").unwrap();
        prepared
            .launch(context.clone(), accepted.job_id.clone())
            .unwrap();
        accepted.job_id
    }

    fn request(argv: &[&str]) -> ExecStart {
        ExecStart {
            argv: argv.iter().map(|word| (*word).to_owned()).collect(),
            cwd: None,
            env: Vec::new(),
            user: GuestUser::Account,
            stdin: None,
            stdin_open: false,
            retain_output_bytes: 64 * 1024,
        }
    }

    fn output_bytes(context: &RecordingContext, stream: &str) -> Vec<u8> {
        context
            .events("guest.exec.output")
            .iter()
            .filter(|event| event["stream"] == stream)
            .flat_map(|event| {
                event["data"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|byte| byte.as_u64().unwrap() as u8)
            })
            .collect()
    }

    #[test]
    fn a_job_streams_output_takes_ordered_input_and_saves_its_exit() {
        let directory = tempfile::tempdir().unwrap();
        let context = RecordingContext::new(directory.path());
        let mut run = request(&["sh", "-c", "cat; echo done >&2; exit 3"]);
        run.stdin = Some(b"first ".to_vec());
        run.stdin_open = true;
        let job = start(directory.path(), &context, run);
        exec_input(&job, Some(b"second ".to_vec()), false).unwrap();
        exec_input(&job, Some(b"third".to_vec()), true).unwrap();
        assert_eq!(
            exec_input(&job, Some(b"late".to_vec()), false)
                .unwrap_err()
                .code,
            ErrorCode::StdinClosed
        );
        let finished = context.wait_finished(&job);
        let result = finished.outcome.unwrap().result.unwrap();
        assert_eq!(result["exitCode"], 3);
        assert_eq!(output_bytes(&context, "stdout"), b"first second third");
        assert_eq!(output_bytes(&context, "stderr"), b"done\n");
        let exited = context.events("guest.exec.exited");
        assert_eq!(exited.len(), 1);
        assert_eq!(exited[0]["exitCode"], 3);
        assert_eq!(exited[0]["jobId"], job.as_str());
        // Sequence numbers are one increasing stream across stdout and stderr.
        let sequences: Vec<u64> = context
            .events("guest.exec.output")
            .iter()
            .map(|event| event["seq"].as_u64().unwrap())
            .collect();
        assert!(sequences.windows(2).all(|pair| pair[0] + 1 == pair[1]));
        assert_eq!(sequences.first(), Some(&1));

        let snapshot = exec_get(&*context, &job, None).unwrap();
        assert_eq!(snapshot.state, ExecState::Exited);
        assert_eq!(snapshot.exit_code, Some(3));
        assert_eq!(snapshot.first_retained_seq, 1);
        let replayed: Vec<u8> = snapshot
            .output
            .iter()
            .filter(|(_, stream, _)| *stream == GuestExecStream::Stdout)
            .flat_map(|(_, _, data)| data.clone())
            .collect();
        assert_eq!(replayed, b"first second third");
        let later = exec_get(&*context, &job, Some(sequences[sequences.len() - 1])).unwrap();
        assert!(later.output.is_empty());
        assert_eq!(
            exec_input(&job, None, false).unwrap_err().code,
            ErrorCode::NotRunning,
            "a finished job takes no input"
        );
    }

    #[test]
    fn a_finished_job_still_answers_from_its_saved_outcome_without_a_replay() {
        let directory = tempfile::tempdir().unwrap();
        let context = RecordingContext::new(directory.path());
        let job = start(
            directory.path(),
            &context,
            request(&["sh", "-c", "echo hi; exit 5"]),
        );
        context.wait_finished(&job);
        sessions().remove(&job);
        let snapshot = exec_get(&*context, &job, Some(2)).unwrap();
        assert_eq!(snapshot.state, ExecState::Exited);
        assert_eq!(snapshot.exit_code, Some(5));
        assert!(snapshot.output.is_empty());
        assert_eq!(snapshot.first_retained_seq, 3);
        let unknown = JobId::new("job:999999999").unwrap();
        assert_eq!(
            exec_get(&*context, &unknown, None).unwrap_err().code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn replay_keeps_a_bounded_tail_and_reports_where_it_starts() {
        let directory = tempfile::tempdir().unwrap();
        let context = RecordingContext::new(directory.path());
        let mut run = request(&[
            "sh",
            "-c",
            "for i in 1 2 3 4 5 6 7 8; do printf 'line%s\\n' $i; sleep 0.05; done",
        ]);
        run.retain_output_bytes = 12;
        let job = start(directory.path(), &context, run);
        context.wait_finished(&job);
        let snapshot = exec_get(&*context, &job, None).unwrap();
        let retained: usize = snapshot.output.iter().map(|(_, _, data)| data.len()).sum();
        assert!(retained <= 12, "retained {retained} bytes");
        assert!(
            snapshot.first_retained_seq > 1,
            "early chunks must have been evicted"
        );
        assert_eq!(snapshot.output[0].0, snapshot.first_retained_seq);
        // Every chunk was still published live, evicted or not.
        assert_eq!(output_bytes(&context, "stdout").len(), 8 * 6);

        let mut too_much = request(&["true"]);
        too_much.retain_output_bytes = MAX_RESPONSE_RAW_BYTES as u64 + 1;
        let target = test_support::target(directory.path());
        assert!(StartedExec::prepare(&target, &too_much).is_err());
    }

    #[test]
    fn cancelling_ends_the_guest_process_tree_and_finishes_the_job_as_cancelled() {
        let directory = tempfile::tempdir().unwrap();
        let context = RecordingContext::new(directory.path());
        let marker = directory.path().join("survivor");
        let script = format!("(sleep 3; touch '{}') & sleep 60", marker.display());
        let job = start(directory.path(), &context, request(&["sh", "-c", &script]));
        std::thread::sleep(Duration::from_millis(800));
        context.job_registry().unwrap().cancel(&job).unwrap();
        let finished = context.wait_finished(&job);
        let error = finished.outcome.unwrap().error.unwrap();
        assert_eq!(error.code, ErrorCode::Cancelled);
        assert_eq!(
            exec_get(&*context, &job, None).unwrap().state,
            ExecState::Cancelled
        );
        assert_eq!(context.events("guest.exec.exited").len(), 1);
        std::thread::sleep(Duration::from_millis(3500));
        assert!(
            !marker.exists(),
            "no process of the cancelled run may survive"
        );
        // Cancelling again, or after the end, is harmless.
        context.job_registry().unwrap().cancel(&job).unwrap();
    }

    #[test]
    fn cancelling_before_the_guest_has_started_the_command_still_ends_it() {
        let directory = tempfile::tempdir().unwrap();
        let context = RecordingContext::new(directory.path());
        let marker = directory.path().join("survivor");
        let script = format!("(sleep 3; touch '{}') & sleep 60", marker.display());
        let job = start(directory.path(), &context, request(&["sh", "-c", &script]));
        // No pause: the guest supervisor may not have recorded its process group yet.
        context.job_registry().unwrap().cancel(&job).unwrap();
        let started = Instant::now();
        let finished = context.wait_finished(&job);
        assert_eq!(
            finished.outcome.unwrap().error.unwrap().code,
            ErrorCode::Cancelled
        );
        assert!(started.elapsed() < Duration::from_secs(20));
        std::thread::sleep(Duration::from_millis(3500));
        assert!(
            !marker.exists(),
            "the command must not outlive its cancellation"
        );
    }

    #[test]
    fn input_to_a_job_without_open_stdin_or_an_unknown_job_is_refused() {
        let directory = tempfile::tempdir().unwrap();
        let context = RecordingContext::new(directory.path());
        let job = start(directory.path(), &context, request(&["sleep", "5"]));
        assert_eq!(
            exec_input(&job, Some(b"x".to_vec()), false)
                .unwrap_err()
                .code,
            ErrorCode::StdinClosed
        );
        cancel_exec(&job).unwrap();
        context.wait_finished(&job);
        let unknown = JobId::new("job:424242").unwrap();
        assert_eq!(
            exec_input(&unknown, None, true).unwrap_err().code,
            ErrorCode::NotRunning
        );
        assert_eq!(cancel_exec(&unknown).unwrap_err().code, ErrorCode::NotFound);
    }

    #[test]
    fn a_stopped_computer_refuses_the_start_before_any_job_exists() {
        let directory = tempfile::tempdir().unwrap();
        let paths = test_support::runtime(directory.path());
        crate::test_support::write_shell_script(
            &paths.executable,
            r#"case "$1" in inspect) printf '%s\n' '{"name":"dev","status":"Stopped","config":{"labels":{"silo.managed":"true"}}}';; esac"#,
        );
        let target =
            crate::guest::resolve(Ok(paths), directory.path(), test_support::COMPUTER_ID).unwrap();
        let error = StartedExec::prepare(&target, &request(&["true"]))
            .err()
            .unwrap();
        assert_eq!(error.code, ErrorCode::NotRunning);
    }

    /// The pid a script wrote into `file`, once it is complete.
    fn wait_for_pid(file: &std::path::Path) -> i32 {
        loop {
            if let Some(pid) = std::fs::read_to_string(file)
                .ok()
                .filter(|text| text.ends_with('\n'))
                .and_then(|text| text.trim().parse().ok())
            {
                return pid;
            }
            thread::yield_now();
        }
    }

    fn process_exists(pid: i32) -> bool {
        unsafe { libc::kill(pid, 0) == 0 }
    }

    #[test]
    fn cancelling_ends_descendants_that_outlive_a_command_that_exited_first() {
        let directory = tempfile::tempdir().unwrap();
        let context = RecordingContext::new(directory.path());
        let file = directory.path().join("descendant");
        // The command ends at once; its background process keeps the output pipes open, so
        // the job is still running when the cancellation arrives.
        let script = format!("sleep 300 & echo $! > '{}'; exit 0", file.display());
        let job = start(directory.path(), &context, request(&["sh", "-c", &script]));
        let pid = wait_for_pid(&file);
        assert!(process_exists(pid));
        assert_eq!(
            crate::jobs::get_at(&*context, &job, None)
                .unwrap()
                .unwrap()
                .state,
            silo_api::jobs::JobState::Running
        );
        context.job_registry().unwrap().cancel(&job).unwrap();
        let finished = context.wait_finished(&job);
        assert_eq!(
            finished.outcome.unwrap().error.unwrap().code,
            ErrorCode::Cancelled
        );
        assert!(
            !process_exists(pid),
            "the descendant must be gone once the cancellation is confirmed"
        );
    }

    #[test]
    fn a_cancellation_that_could_not_end_the_tree_is_a_failure_not_a_cancellation() {
        let directory = tempfile::tempdir().unwrap();
        let paths = test_support::runtime_with_exec_prelude(
            directory.path(),
            // Every guest control command (`perl … -- kill …`) fails, as if the guest were
            // unreachable at that moment.
            r#"for word in "$@"; do [ "$word" = kill ] && exit 1; done"#,
        );
        let target =
            crate::guest::resolve(Ok(paths), directory.path(), test_support::COMPUTER_ID).unwrap();
        let context = RecordingContext::new(directory.path());
        let file = directory.path().join("survivor");
        let script = format!("sleep 300 & echo $! > '{}'; wait", file.display());
        let prepared = StartedExec::prepare(&target, &request(&["sh", "-c", &script])).unwrap();
        let accepted =
            crate::jobs::accept_domain_id(&*context, "job", "2000001", "guest.exec.start").unwrap();
        let job = accepted.job_id.clone();
        prepared.launch(context.clone(), accepted.job_id).unwrap();
        let pid = wait_for_pid(&file);
        context.job_registry().unwrap().cancel(&job).unwrap();
        let finished = context.wait_finished(&job);
        unsafe { libc::kill(pid, libc::SIGKILL) };
        let error = finished.outcome.unwrap().error.unwrap();
        assert_eq!(error.code, ErrorCode::Internal);
        assert!(
            error.message.contains("could not be stopped"),
            "{}",
            error.message
        );
        assert_eq!(
            exec_get(&*context, &job, None).unwrap().state,
            ExecState::Lost
        );
    }

    #[test]
    fn cancelling_after_the_command_exited_but_before_its_output_ended_finishes_the_job() {
        let directory = tempfile::tempdir().unwrap();
        let context = RecordingContext::new(directory.path());
        let file = directory.path().join("daemon");
        // A process in its own session keeps both output pipes open after the command ended,
        // outside the process group a cancellation ends.
        let script = format!(
            "perl -e 'use POSIX; if (fork() == 0) {{ POSIX::setsid(); open(F, \">\", \"{file}\"); print F \"$$\\n\"; close F; exec \"sleep\", \"300\"; }}'",
            file = file.display()
        );
        let job = start(directory.path(), &context, request(&["sh", "-c", &script]));
        let pid = wait_for_pid(&file);
        context.job_registry().unwrap().cancel(&job).unwrap();
        let finished = context.wait_finished(&job);
        unsafe { libc::kill(pid, libc::SIGKILL) };
        assert_eq!(
            finished.outcome.unwrap().error.unwrap().code,
            ErrorCode::Cancelled
        );
    }

    fn session_with_budget(
        directory: &std::path::Path,
        retain: usize,
    ) -> (Arc<RecordingContext>, Session) {
        let context = RecordingContext::new(directory);
        let session = Session::new(
            JobId::new("job:7777").unwrap(),
            test_support::target(directory),
            "token".into(),
            false,
            retain,
            false,
        );
        (context, session)
    }

    fn retained(session: &Session) -> (Vec<u64>, u64) {
        let progress = session.progress();
        (
            progress.ring.iter().map(|chunk| chunk.seq).collect(),
            progress.ring.front().map_or(progress.next_seq, |c| c.seq),
        )
    }

    #[test]
    fn an_oversized_chunk_clears_the_replay_so_it_stays_a_contiguous_suffix() {
        let directory = tempfile::tempdir().unwrap();
        let (context, session) = session_with_budget(directory.path(), 10);
        let put = |len: usize| session.output(&*context, StreamKind::Stdout, vec![b'x'; len]);
        put(4); // seq 1
        put(4); // seq 2
        assert_eq!(retained(&session), (vec![1, 2], 1));
        put(20); // seq 3, larger than the whole budget
        assert_eq!(
            retained(&session),
            (vec![], 4),
            "nothing older than the oversized chunk may remain"
        );
        put(3); // seq 4
        put(3); // seq 5
        assert_eq!(retained(&session), (vec![4, 5], 4));
        put(11); // seq 6, an oversized final chunk
        assert_eq!(retained(&session), (vec![], 7));
    }

    #[test]
    fn only_exec_jobs_answer_exec_get_and_acknowledging_drops_the_replay() {
        let directory = tempfile::tempdir().unwrap();
        let context = RecordingContext::new(directory.path());
        // Another domain's finished job is not an exec job.
        let other = crate::jobs::accept_domain_id(&*context, "transfer", "t1", "files.upload")
            .unwrap()
            .job_id;
        crate::jobs::finish_with_result(
            &*context,
            other.clone(),
            serde_json::json!({"status": "done"}),
            Audience::All,
        )
        .unwrap();
        assert_eq!(
            exec_get(&*context, &other, None).unwrap_err().code,
            ErrorCode::NotFound
        );
        // An exec job keeps its replay until it is acknowledged.
        let job = start(directory.path(), &context, request(&["echo", "hi"]));
        context.wait_finished(&job);
        assert!(sessions().contains_key(&job));
        forget_finished(&job);
        assert!(!sessions().contains_key(&job));
        // A running job keeps its session.
        let running = start(directory.path(), &context, request(&["sleep", "300"]));
        forget_finished(&running);
        assert!(sessions().contains_key(&running));
        cancel_exec(&running).unwrap();
        context.wait_finished(&running);
    }
}
