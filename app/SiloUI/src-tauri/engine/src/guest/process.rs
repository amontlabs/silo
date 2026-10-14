//! Host processes that carry guest commands: spawning, feeding stdin, draining output with
//! bounds, and stopping them on a deadline, an overflow or a cancellation.
use super::GuestError;
use std::{
    ffi::OsString,
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
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
}

impl Plan {
    fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command
            .args(self.args.iter().map(OsString::from))
            .envs(self.envs.iter().map(|(key, value)| (key, value)));
        command
    }
}

/// How a bounded run ended on the host side.
pub(crate) struct Collected {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    /// At most `stderr_keep` bytes of the spooled standard error.
    pub stderr: Vec<u8>,
    /// The full length of standard error, kept in a private temporary file while running.
    pub stderr_len: u64,
}

/// Why a bounded run was stopped before the process ended on its own.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Stopped {
    Timeout,
    Overflow,
    Cancelled,
}

/// Runs `plan` to completion.
///
/// Standard output is read only up to `max_stdout` bytes: one byte more stops the run
/// (`Stopped::Overflow`), so the host never buffers more. Standard error is drained without
/// a limit into a private temporary file and read back up to `stderr_keep` bytes.
/// `before_stop` runs once, before the host process is killed, so the caller can end the
/// guest process tree first.
pub(crate) fn run_collect(
    plan: &Plan,
    stdin: Option<&[u8]>,
    timeout: Duration,
    max_stdout: usize,
    stderr_keep: usize,
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
    let writer = stdin.map(|bytes| {
        let (mut pipe, bytes) = (child.stdin.take(), bytes.to_vec());
        thread::spawn(move || {
            if let Some(pipe) = pipe.as_mut() {
                let _ = pipe.write_all(&bytes);
            }
        })
    });
    let exceeded = Arc::new(AtomicBool::new(false));
    let stdout = {
        let (mut stream, exceeded) = (child.stdout.take(), exceeded.clone());
        thread::spawn(move || {
            let mut bytes = Vec::new();
            if let Some(stream) = stream.as_mut() {
                let _ = stream.take(max_stdout as u64 + 1).read_to_end(&mut bytes);
            }
            if bytes.len() > max_stdout {
                exceeded.store(true, Ordering::SeqCst);
            }
            bytes
        })
    };
    let stderr = {
        let mut stream = child.stderr.take();
        thread::spawn(move || -> std::io::Result<File> {
            let mut spool = tempfile::tempfile()?;
            if let Some(stream) = stream.as_mut() {
                std::io::copy(stream, &mut spool)?;
            }
            Ok(spool)
        })
    };
    let deadline = Instant::now() + timeout;
    let stopped = loop {
        if exceeded.load(Ordering::SeqCst) {
            break Some(Stopped::Overflow);
        }
        if cancel.is_some_and(|flag| flag.load(Ordering::SeqCst)) {
            break Some(Stopped::Cancelled);
        }
        match child.try_wait() {
            Ok(Some(_)) => break None,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            Ok(None) => break Some(Stopped::Timeout),
            Err(error) => {
                kill(&mut child);
                return Err(GuestError::internal(format!(
                    "Silo lost the guest command: {error}"
                )));
            }
        }
    };
    if let Some(reason) = stopped {
        before_stop();
        kill(&mut child);
        let _ = stdout.join();
        let _ = stderr.join();
        if let Some(writer) = writer {
            let _ = writer.join();
        }
        return Ok(Err(reason));
    }
    let status = child
        .wait()
        .map_err(|error| GuestError::internal(format!("Silo lost the guest command: {error}")))?;
    if let Some(writer) = writer {
        let _ = writer.join();
    }
    let stdout = stdout.join().unwrap_or_default();
    // Output past the allowance may have been the last thing read before the process ended.
    if stdout.len() > max_stdout {
        return Ok(Err(Stopped::Overflow));
    }
    let mut spool = stderr
        .join()
        .map_err(|_| GuestError::internal("Silo lost the guest command's error output."))?
        .map_err(|error| {
            GuestError::internal(format!("Silo could not keep the command's errors: {error}"))
        })?;
    let stderr_len = spool
        .metadata()
        .map_err(|error| GuestError::internal(error.to_string()))?
        .len();
    let mut kept = Vec::new();
    spool
        .seek(SeekFrom::Start(0))
        .and_then(|_| spool.take(stderr_keep as u64).read_to_end(&mut kept))
        .map_err(|error| {
            GuestError::internal(format!("Silo could not read the errors: {error}"))
        })?;
    Ok(Ok(Collected {
        status,
        stdout,
        stderr: kept,
        stderr_len,
    }))
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
