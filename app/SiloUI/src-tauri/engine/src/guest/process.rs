//! Host processes that carry guest commands: spawning, feeding stdin, draining output with
//! bounds, and stopping them on a deadline, an overflow or a cancellation.
use super::GuestError;
use std::{
    ffi::OsString,
    io::{Read, Write},
    path::PathBuf,
    process::{Child, ChildStdin, Command, ExitStatus, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

/// A host program invocation, ready to spawn.
#[derive(Clone, Debug)]
pub(crate) struct Plan {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub envs: Vec<(String, String)>,
    /// The files the arguments name (a container's per-exec directory), removed once the plan and
    /// the process it started are gone.
    pub files: Option<Arc<crate::containers::hold::ExecGroup>>,
}

impl Plan {
    fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command
            .args(self.args.iter().map(OsString::from))
            .envs(self.envs.iter().map(|(key, value)| (key, value)));
        // Like every runtime child on a Linux host (`runtime::runtime_command`): a terminal's
        // Ctrl-C reaches silod alone.
        #[cfg(target_os = "linux")]
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        command
    }
}

/// How a bounded run ended on the host side.
pub(crate) struct Collected {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// Why a bounded run was stopped before the process ended on its own.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Stopped {
    Timeout,
    /// Standard output went past its allowance.
    Overflow,
    /// Standard error went past its allowance.
    StderrOverflow,
    Cancelled,
}

/// What happens when standard error passes its allowance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StderrLimit {
    /// The run is stopped (`Stopped::StderrOverflow`).
    Stop(usize),
    /// The first bytes are kept and the rest is read and thrown away, so the run goes on.
    Keep(usize),
}

impl StderrLimit {
    fn bytes(self) -> usize {
        match self {
            Self::Stop(bytes) | Self::Keep(bytes) => bytes,
        }
    }
}

/// How long cleanup waits for output threads after a stop. A descendant that outlived the
/// command in another process group could hold the pipes open for good, so the wait ends and
/// the threads are left to finish on their own.
#[cfg(not(test))]
pub(crate) const CLEANUP_GRACE: Duration = Duration::from_secs(2);
#[cfg(test)]
pub(crate) const CLEANUP_GRACE: Duration = Duration::from_millis(300);

/// Marks a thread as ended when dropped, even if it panicked.
struct Done(Arc<AtomicBool>);

impl Drop for Done {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// Waits until every flag is set or `grace` passed; true when all are set.
fn wait_all(flags: &[Arc<AtomicBool>], grace: Duration) -> bool {
    let deadline = Instant::now() + grace;
    loop {
        if flags.iter().all(|flag| flag.load(Ordering::SeqCst)) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(5));
    }
}

/// Runs `plan` to completion.
///
/// Standard output is read only up to `max_stdout` bytes: one byte more stops the run
/// (`Stopped::Overflow`), so the host never buffers more. Standard error is held the same
/// way up to `max_stderr` bytes (`Stopped::StderrOverflow`); nothing is written to the host's
/// disk, so a command that prints forever costs neither disk nor unbounded memory.
/// `before_stop` runs once, before the host process is killed, so the caller can end the
/// guest process tree first.
///
/// The deadline, the overflow check and `cancel` apply until the process has ended and its
/// output has reached end-of-file, so a descendant that keeps a pipe open cannot outlast
/// them.
pub(crate) fn run_collect(
    plan: &Plan,
    stdin: Option<&[u8]>,
    timeout: Duration,
    max_stdout: usize,
    max_stderr: StderrLimit,
    cancel: Option<&AtomicBool>,
    before_stop: &dyn Fn(),
) -> Result<Result<Collected, Stopped>, GuestError> {
    let mut command = plan.command();
    command
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| {
        GuestError::internal(format!("Silo could not run the guest command: {error}"))
    })?;
    let mut flags = Vec::new();
    let mut flag = || {
        let flag = Arc::new(AtomicBool::new(false));
        flags.push(flag.clone());
        flag
    };
    let writer = stdin.map(|bytes| {
        let (mut pipe, bytes, done) = (child.stdin.take(), bytes.to_vec(), flag());
        thread::spawn(move || {
            let _done = Done(done);
            if let Some(pipe) = pipe.as_mut() {
                let _ = pipe.write_all(&bytes);
            }
        })
    });
    let stdout_exceeded = Arc::new(AtomicBool::new(false));
    let stderr_exceeded = Arc::new(AtomicBool::new(false));
    let mut capped = |stream: Option<Box<dyn Read + Send>>,
                      limit: usize,
                      discard_excess: bool,
                      exceeded: &Arc<AtomicBool>| {
        let (exceeded, done) = (exceeded.clone(), flag());
        thread::spawn(move || {
            let _done = Done(done);
            let Some(stream) = stream else {
                return Vec::new();
            };
            let (bytes, over) = read_capped(stream, limit, discard_excess);
            if over && !discard_excess {
                exceeded.store(true, Ordering::SeqCst);
            }
            bytes
        })
    };
    let stdout = capped(
        child.stdout.take().map(|stream| Box::new(stream) as _),
        max_stdout,
        false,
        &stdout_exceeded,
    );
    let stderr = capped(
        child.stderr.take().map(|stream| Box::new(stream) as _),
        max_stderr.bytes(),
        matches!(max_stderr, StderrLimit::Keep(_)),
        &stderr_exceeded,
    );
    let deadline = Instant::now() + timeout;
    let mut status = None;
    let stopped = loop {
        if stdout_exceeded.load(Ordering::SeqCst) {
            break Some(Stopped::Overflow);
        }
        if stderr_exceeded.load(Ordering::SeqCst) {
            break Some(Stopped::StderrOverflow);
        }
        if cancel.is_some_and(|flag| flag.load(Ordering::SeqCst)) {
            break Some(Stopped::Cancelled);
        }
        if status.is_none() {
            match child.try_wait() {
                Ok(exited) => status = exited,
                Err(error) => {
                    kill(&mut child);
                    return Err(GuestError::internal(format!(
                        "Silo lost the guest command: {error}"
                    )));
                }
            }
        }
        // The command is done once it ended and its output reached end-of-file.
        if status.is_some() && flags.iter().all(|flag| flag.load(Ordering::SeqCst)) {
            break None;
        }
        if Instant::now() >= deadline {
            break Some(Stopped::Timeout);
        }
        thread::sleep(Duration::from_millis(5));
    };
    if let Some(reason) = stopped {
        before_stop();
        kill(&mut child);
        // Abandoned threads end with their pipes; they only hold their own buffers.
        if wait_all(&flags, CLEANUP_GRACE) {
            let _ = stdout.join();
            let _ = stderr.join();
            if let Some(writer) = writer {
                let _ = writer.join();
            }
        }
        return Ok(Err(reason));
    }
    let status = status.expect("the loop only completes with an exit status");
    if let Some(writer) = writer {
        let _ = writer.join();
    }
    let stdout = stdout.join().unwrap_or_default();
    let stderr = stderr.join().unwrap_or_default();
    // Output past an allowance may have been the last thing read before the process ended.
    if stdout.len() > max_stdout {
        return Ok(Err(Stopped::Overflow));
    }
    if stderr.len() > max_stderr.bytes() && matches!(max_stderr, StderrLimit::Stop(_)) {
        return Ok(Err(Stopped::StderrOverflow));
    }
    Ok(Ok(Collected {
        status,
        stdout,
        stderr,
    }))
}

/// The size of one read of a bounded stream.
const READ_STEP: usize = 8 * 1024;

/// Reads `stream` into a buffer whose capacity never passes `limit + 1`. Returns the bytes
/// kept and whether the stream held more than `limit`. Without `discard_excess` reading stops
/// at the first byte past `limit`; with it the first `limit` bytes are kept and the rest is
/// read to the end and dropped.
fn read_capped(mut stream: impl Read, limit: usize, discard_excess: bool) -> (Vec<u8>, bool) {
    let room = limit.saturating_add(1);
    let mut bytes: Vec<u8> = Vec::new();
    let mut step = [0u8; READ_STEP];
    let mut over = false;
    loop {
        let count = match stream.read(&mut step) {
            Ok(0) | Err(_) => break,
            Ok(count) => count,
        };
        let space = if discard_excess { limit } else { room };
        let take = count.min(space - bytes.len());
        if bytes.capacity() - bytes.len() < take {
            let wanted = (bytes.len() + take).max(bytes.capacity() * 2).min(space);
            bytes.reserve_exact(wanted - bytes.len());
        }
        bytes.extend_from_slice(&step[..take]);
        if take < count {
            over = true;
        }
        // One byte past the allowance is enough to know: stop at once instead of waiting
        // for more output or for the pipe to close. (Only a read without `discard_excess`
        // can hold that byte.)
        if bytes.len() > limit {
            break;
        }
    }
    if bytes.len() > limit {
        over = true;
    }
    (bytes, over)
}

pub(crate) fn kill(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// A long-running process whose output is delivered as it is read.
pub(crate) struct Streaming {
    pub child: Child,
    pub stdin: Option<ChildStdin>,
    pub readers: Vec<JoinHandle<()>>,
    /// Keeps the plan's files until the process is gone.
    _files: Option<Arc<crate::containers::hold::ExecGroup>>,
}

impl Streaming {
    /// Waits until both output streams reached end-of-file and their chunks were delivered.
    pub fn finish_output(&mut self) {
        for reader in self.readers.drain(..) {
            let _ = reader.join();
        }
    }
}

/// Which output stream a chunk came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StreamKind {
    Stdout,
    Stderr,
}

/// The size of one read, and so of one delivered chunk.
pub(crate) const CHUNK_BYTES: usize = 64 * 1024;

/// Starts `plan` with piped output. `on_chunk` receives each read (at most `CHUNK_BYTES`)
/// from either stream. With `stdin` the caller gets the process's standard input;
/// otherwise the process sees end-of-file.
pub(crate) fn spawn_streaming(
    plan: &Plan,
    stdin: bool,
    on_chunk: Arc<dyn Fn(StreamKind, Vec<u8>) + Send + Sync>,
) -> Result<Streaming, GuestError> {
    let mut command = plan.command();
    command
        .stdin(if stdin { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| {
        GuestError::internal(format!("Silo could not run the guest command: {error}"))
    })?;
    let input = child.stdin.take();
    let mut readers = Vec::new();
    if let Some(stream) = child.stdout.take() {
        readers.push(reader(stream, StreamKind::Stdout, on_chunk.clone()));
    }
    if let Some(stream) = child.stderr.take() {
        readers.push(reader(stream, StreamKind::Stderr, on_chunk));
    }
    Ok(Streaming {
        child,
        stdin: input,
        readers,
        _files: plan.files.clone(),
    })
}

fn reader<R: Read + Send + 'static>(
    mut stream: R,
    kind: StreamKind,
    on_chunk: Arc<dyn Fn(StreamKind, Vec<u8>) + Send + Sync>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut buffer = vec![0u8; CHUNK_BYTES];
        loop {
            match stream.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(count) => on_chunk(kind, buffer[..count].to_vec()),
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell(script: &str) -> Plan {
        Plan {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), script.into()],
            envs: Vec::new(),
            files: None,
        }
    }

    #[test]
    fn a_plans_files_outlive_the_plan_while_its_process_runs() {
        let root = tempfile::tempdir().unwrap();
        let files = root.path().join("exec");
        std::fs::create_dir(&files).unwrap();
        let mut plan = shell("exit 0");
        plan.files = Some(Arc::new(crate::containers::hold::ExecGroup::new(
            files.clone(),
            "silo-ct-test",
        )));
        let streaming = spawn_streaming(&plan, false, Arc::new(|_, _| {})).unwrap();
        drop(plan);
        assert!(files.exists(), "the files went before the process ended");
        drop(streaming);
        assert!(!files.exists(), "the files stayed behind");
        // A plan that never ran removes them too.
        std::fs::create_dir(&files).unwrap();
        let mut unused = shell("exit 0");
        unused.files = Some(Arc::new(crate::containers::hold::ExecGroup::new(
            files.clone(),
            "silo-ct-test",
        )));
        drop(unused);
        assert!(!files.exists());
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

    #[test]
    fn cancelling_after_the_process_exited_but_before_end_of_output_stops_the_run() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("pid");
        // The shell ends at once; the background process keeps both output pipes open.
        let script = format!("sleep 300 & echo $! > '{}'", file.display());
        let cancel = AtomicBool::new(false);
        let outcome = thread::scope(|scope| {
            scope.spawn(|| {
                let pid = wait_for_pid(&file);
                PID.store(pid, Ordering::SeqCst);
                cancel.store(true, Ordering::SeqCst);
            });
            run_collect(
                &shell(&script),
                None,
                Duration::from_secs(300),
                1024,
                StderrLimit::Stop(1024),
                Some(&cancel),
                &|| {},
            )
            .unwrap()
        });
        unsafe { libc::kill(PID.load(Ordering::SeqCst), libc::SIGKILL) };
        assert!(matches!(outcome, Err(Stopped::Cancelled)));
    }

    static PID: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

    #[test]
    fn a_run_whose_output_ends_with_the_process_still_completes() {
        let outcome = run_collect(
            &shell("printf out; printf err >&2; exit 3"),
            None,
            Duration::from_secs(60),
            1024,
            StderrLimit::Stop(1024),
            None,
            &|| {},
        )
        .unwrap()
        .ok()
        .unwrap();
        assert_eq!(outcome.status.code(), Some(3));
        assert_eq!(outcome.stdout, b"out");
        assert_eq!(outcome.stderr, b"err");
    }

    #[test]
    fn standard_error_past_its_allowance_stops_the_run_without_spooling() {
        let started = Instant::now();
        let outcome = run_collect(
            &shell("while :; do printf xxxxxxxx >&2; done"),
            None,
            Duration::from_secs(60),
            1024,
            StderrLimit::Stop(1024),
            None,
            &|| {},
        )
        .unwrap();
        assert!(matches!(outcome, Err(Stopped::StderrOverflow)));
        assert!(started.elapsed() < Duration::from_secs(30));
    }

    #[test]
    fn standard_error_that_ends_exactly_at_its_allowance_completes() {
        let outcome = run_collect(
            &shell("printf 12345 >&2"),
            None,
            Duration::from_secs(60),
            1024,
            StderrLimit::Stop(5),
            None,
            &|| {},
        )
        .unwrap()
        .ok()
        .unwrap();
        assert_eq!(outcome.stderr, b"12345");
        let outcome = run_collect(
            &shell("printf 123456 >&2"),
            None,
            Duration::from_secs(60),
            1024,
            StderrLimit::Stop(5),
            None,
            &|| {},
        )
        .unwrap();
        assert!(matches!(outcome, Err(Stopped::StderrOverflow)));
    }

    #[test]
    fn one_byte_past_an_allowance_stops_the_run_while_the_writer_stays_open() {
        for (script, stderr_limit, stdout_limit) in [
            ("printf 123456 >&2; exec sleep 300", 5, 1024),
            ("printf 123456; exec sleep 300", 1024, 5),
        ] {
            let started = Instant::now();
            let outcome = run_collect(
                &shell(script),
                None,
                Duration::from_secs(60),
                stdout_limit,
                StderrLimit::Stop(stderr_limit),
                None,
                &|| {},
            )
            .unwrap();
            let expected = if stdout_limit == 5 {
                matches!(outcome, Err(Stopped::Overflow))
            } else {
                matches!(outcome, Err(Stopped::StderrOverflow))
            };
            assert!(expected, "{script}");
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "{script} waited for the writer: {:?}",
                started.elapsed()
            );
        }
    }

    #[test]
    fn reading_never_holds_much_more_than_the_allowance() {
        let limit = 100_000;
        let (bytes, over) = read_capped(std::io::repeat(b'x'), limit, false);
        assert!(over);
        assert_eq!(bytes.len(), limit + 1);
        assert!(
            bytes.capacity() <= limit + 1,
            "the buffer grew to {} for an allowance of {limit}",
            bytes.capacity()
        );
        let (bytes, over) = read_capped(&b"abcdef"[..], 6, false);
        assert!(!over);
        assert_eq!(bytes, b"abcdef");
        let (bytes, over) = read_capped(std::io::repeat(b'x').take(1_000_000), limit, true);
        assert!(over);
        assert_eq!(bytes.len(), limit);
        assert!(bytes.capacity() <= limit + 1);
    }

    #[test]
    fn kept_standard_error_drops_the_excess_and_the_run_completes() {
        let outcome = run_collect(
            &shell("i=0; while [ $i -lt 400 ]; do printf '%0200d' 0 >&2; i=$((i+1)); done; printf out; exit 4"),
            None,
            Duration::from_secs(60),
            1024,
            StderrLimit::Keep(1000),
            None,
            &|| {},
        )
        .unwrap()
        .ok()
        .unwrap();
        assert_eq!(outcome.status.code(), Some(4));
        assert_eq!(outcome.stdout, b"out");
        assert_eq!(outcome.stderr.len(), 1000);
    }

    #[test]
    fn kept_standard_error_does_not_excuse_standard_output_past_its_allowance() {
        let outcome = run_collect(
            &shell("while :; do printf xxxxxxxx; done"),
            None,
            Duration::from_secs(60),
            1024,
            StderrLimit::Keep(1000),
            None,
            &|| {},
        )
        .unwrap();
        assert!(matches!(outcome, Err(Stopped::Overflow)));
    }
}
