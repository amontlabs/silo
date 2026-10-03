//! Single-file transfers between this device and a computer, over the system OpenSSH
//! `sftp` client in batch mode. The client uses the same private SSH configuration and
//! ProxyCommand as the editor, so a stopped computer is never started and the session
//! runs as the working account.
//!
//! Remote names reach `sftp` only through commands that do not expand wildcards
//! (`cd`, `put`, `rename`, `get` with a name that has no wildcard characters, and `ls`
//! of the folder or of a wildcard-free name). Local sources are staged behind a fixed
//! symlink name for the same reason.
//!
//! The computer is untrusted. A download is bounded on this device by the size the computer
//! listed (a file-size limit on the client, a watch on the partial file and a free-space
//! check), and a file is re-listed in the same session that reads it. Between that listing
//! and the read the computer can still swap the file for another one it holds under the
//! allowed folders' reach; this only changes which of its own bytes are read, never where
//! anything is written on this device.
use crate::owned_tunnel::Tunnel;
use crate::working_account::USER;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    ffi::OsStr,
    fs,
    io::{Read, Write},
    os::unix::fs::{symlink, PermissionsExt},
    path::{Path, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::{AppHandle, DragDropEvent, Emitter, Window};
use tauri_plugin_dialog::DialogExt;

pub(crate) const PROGRESS_EVENT: &str = "silo://transfer-progress";
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_BATCH_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const MAX_FILES: usize = 100;
const MAX_NAME_BYTES: usize = 255;
const MAX_PATH_BYTES: usize = 4096;
const MAX_OUTPUT_BYTES: u64 = 4 * 1024 * 1024;
const PROBE_LIMIT: Duration = Duration::from_secs(45);
const POLL_LIMIT: Duration = Duration::from_secs(8);
const CLEANUP_LIMIT: Duration = Duration::from_secs(20);
const UPLOAD_POLL: Duration = Duration::from_millis(1500);
const DOWNLOAD_POLL: Duration = Duration::from_millis(250);
const SFTP: &str = "/usr/bin/sftp";
const SSH: &str = "/usr/bin/ssh";
const NOT_RUNNING: &str = "Start this computer to transfer files.";
const MISSING_SFTP: &str =
    "Transferring files needs the OpenSSH sftp client, which was not found. Install OpenSSH and try again.";
const UNREACHABLE: &str =
    "Could not connect to this computer. Make sure it is running, then try again.";
const CANCELLED: &str = "The transfer was cancelled.";
const TOO_LARGE: &str = "The computer sent more data than the size it reported for this file. The download was stopped.";
const LISTING_TOO_LARGE: &str = "This folder holds too many items to check safely.";
const UNREADABLE_FOLDER: &str = "Could not read that folder in the computer.";
const NO_ROOM_HERE: &str = "There is not enough free space on this device for this file.";
const STOPPED_FOR_ROOM: &str =
    "This device is running out of free space. The download was stopped.";
/// What a download leaves free on the destination volume beyond the file itself.
const FREE_SPACE_RESERVE: u64 = 64 * 1024 * 1024;
/// How far past the listed size the client may write before the system stops it.
const WRITE_SLACK: u64 = 1024 * 1024;
const MAX_PUBLISH_ATTEMPTS: usize = 8;
pub(crate) const DRAG_EVENT: &str = "silo://viewer-drag";
pub(crate) const DROP_EVENT: &str = "silo://viewer-drop";
const SELECTION_TTL: Duration = Duration::from_secs(600);
const MAX_SELECTIONS: usize = 16;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ConflictPolicy {
    Ask,
    Replace,
    KeepBoth,
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub(crate) enum UploadOutcome {
    /// Nothing was sent: these names already exist and the caller must choose a policy.
    Conflict {
        names: Vec<String>,
    },
    Done {
        names: Vec<String>,
    },
    Cancelled,
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub(crate) enum DownloadOutcome {
    Done { path: String },
    Cancelled,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Progress {
    id: String,
    computer: String,
    direction: &'static str,
    state: &'static str,
    name: String,
    file_index: usize,
    file_count: usize,
    bytes_done: u64,
    bytes_total: u64,
}

/// What a transfer reports to, and is stopped by.
struct Control<'a> {
    id: &'a str,
    computer: &'a str,
    cancel: &'a AtomicBool,
    emit: &'a dyn Fn(&Progress),
    /// Paths a transfer may touch, as the guest reports them after resolving links.
    roots: &'a [String],
    /// How often an upload asks the computer how much has arrived; `None` skips it.
    upload_poll: Option<Duration>,
    /// Free bytes on the volume holding a path on this device.
    free_space: &'a dyn Fn(&Path) -> Option<u64>,
}

impl Control<'_> {
    fn report(
        &self,
        direction: &'static str,
        state: &'static str,
        name: &str,
        index: usize,
        count: usize,
        done: u64,
        total: u64,
    ) {
        (self.emit)(&Progress {
            id: self.id.into(),
            computer: self.computer.into(),
            direction,
            state,
            name: name.into(),
            file_index: index,
            file_count: count,
            bytes_done: done,
            bytes_total: total,
        });
    }
}

pub(crate) fn downloads_root() -> String {
    format!("/home/{USER}/Downloads")
}

fn default_roots() -> Vec<String> {
    vec!["/workspace".into(), downloads_root()]
}

fn controls_in(value: &str) -> bool {
    value.chars().any(char::is_control)
}

/// A normalized absolute path equal to `root` or strictly below it.
fn below(path: &str, root: &str) -> bool {
    if path.len() > MAX_PATH_BYTES || controls_in(path) {
        return false;
    }
    if root == "/workspace" {
        return crate::files::valid_path(path);
    }
    path == root
        || path.strip_prefix(root).is_some_and(|tail| {
            tail.strip_prefix('/').is_some_and(|tail| {
                tail.split('/')
                    .all(|part| !part.is_empty() && part != "." && part != "..")
            })
        })
}

fn within_any(path: &str, roots: &[String]) -> bool {
    roots.iter().any(|root| below(path, root))
}

/// The canonical form a guest reports must also lie inside a root; this does not
/// require it to be normalized the way a requested path is.
fn canonical_within(path: &str, roots: &[String]) -> bool {
    !controls_in(path)
        && roots.iter().any(|root| {
            path == root
                || path
                    .strip_prefix(root.as_str())
                    .is_some_and(|tail| tail.starts_with('/'))
        })
        && !path.split('/').any(|part| part == "..")
}

/// The base name a host file is stored under in the computer.
fn sanitize_name(raw: &OsStr) -> Result<String, String> {
    let name = raw
        .to_str()
        .ok_or("This file name cannot be transferred because it is not valid text.")?;
    if name.is_empty() || name == "." || name == ".." {
        return Err("This file has no usable name.".into());
    }
    if name.contains('/') || controls_in(name) {
        return Err(format!(
            "\"{}\" has characters in its name that cannot be transferred.",
            name.escape_debug()
        ));
    }
    if name.len() > MAX_NAME_BYTES {
        return Err("This file name is too long to transfer.".into());
    }
    Ok(name.into())
}

/// Names that `sftp` would expand when it reads them from a remote path.
fn has_wildcard(name: &str) -> bool {
    name.contains(['*', '?', '[', ']', '{', '}', '\\'])
}

/// One argument of a batch line. Relative names carry a `./` so they are never read as options.
fn quote(value: &str) -> Result<String, String> {
    if value.is_empty() || controls_in(value) {
        return Err("This name cannot be transferred.".into());
    }
    Ok(format!(
        "\"{}\"",
        value.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}

fn relative(name: &str) -> Result<String, String> {
    quote(&format!("./{name}"))
}

fn split_extension(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(index) if index > 0 && index + 1 < name.len() => name.split_at(index),
        _ => (name, ""),
    }
}

fn truncate_to(value: &str, bytes: usize) -> &str {
    let mut end = value.len().min(bytes);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

/// `name (1).ext`, the lowest number whose result is not taken.
fn keep_both_name(name: &str, taken: &dyn Fn(&str) -> bool) -> String {
    let (stem, mut extension) = split_extension(name);
    // The number needs room beside a name part of at least one byte.
    if extension.len() + " (4294967295)".len() >= MAX_NAME_BYTES {
        extension = "";
    }
    let stem = if extension.is_empty() { name } else { stem };
    for number in 1_u32.. {
        let suffix = format!(" ({number}){extension}");
        let candidate = format!(
            "{}{suffix}",
            truncate_to(stem, MAX_NAME_BYTES.saturating_sub(suffix.len()))
        );
        if !taken(&candidate) {
            return candidate;
        }
    }
    unreachable!("the counter does not end")
}

/// The temporary name beside the final file. Only characters that are never wildcards
/// are kept, so it can be named to commands that expand them.
fn partial_name(name: &str, id: &str) -> String {
    let stem: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .take(100)
        .collect();
    format!(".{}.silo-part-{id}", stem.trim_start_matches('.'))
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["bytes", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} bytes")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[derive(Debug, PartialEq)]
struct Entry {
    kind: char,
    size: u64,
    name: String,
}

#[derive(Debug, Default, PartialEq)]
struct Probe {
    working_directories: Vec<String>,
    entries: Vec<Entry>,
    available_kib: Option<u64>,
    /// A listing that finished names the folder itself.
    listed_folder: bool,
}

fn parse_entry(line: &str) -> Option<Entry> {
    let mut chars = line.chars();
    let kind = chars.next().filter(|c| "-dlcbps".contains(*c))?;
    let permissions: Vec<char> = chars.by_ref().take(9).collect();
    if permissions.len() != 9 || !permissions.iter().all(|c| "rwxsStT-".contains(*c)) {
        return None;
    }
    let mut rest = line;
    let mut fields = Vec::with_capacity(8);
    for _ in 0..8 {
        rest = rest.trim_start_matches(' ');
        let end = rest.find(' ')?;
        fields.push(&rest[..end]);
        rest = &rest[end..];
    }
    let name = rest.strip_prefix(' ')?;
    Some(Entry {
        kind,
        size: fields[4].parse().ok()?,
        name: name.into(),
    })
}

/// Reads the output of `pwd`, `ls -lan` and `df` commands run in one batch.
fn parse_probe(output: &str) -> Probe {
    let mut probe = Probe::default();
    let mut after_header = false;
    for line in output.lines() {
        if let Some(path) = line.strip_prefix("Remote working directory: ") {
            probe.working_directories.push(path.into());
        } else if line.starts_with("sftp> ") {
            after_header = false;
        } else if line.contains("Avail") && line.contains("Size") {
            after_header = true;
        } else if after_header {
            let numbers: Vec<u64> = line
                .split_whitespace()
                .filter_map(|field| field.parse().ok())
                .collect();
            if numbers.len() >= 3 {
                probe.available_kib = Some(numbers[2]);
            }
            after_header = false;
        } else if let Some(entry) = parse_entry(line) {
            if entry.name == "." || entry.name == "./." {
                probe.listed_folder = true;
            } else if entry.name != ".." && entry.name != "./.." {
                probe.entries.push(entry);
            }
        }
    }
    probe
}

#[derive(Debug)]
enum RunError {
    Cancelled,
    Failed(String),
}

impl RunError {
    fn message(self) -> String {
        match self {
            Self::Cancelled => CANCELLED.into(),
            Self::Failed(message) => message,
        }
    }
}

fn failure_message(stderr: &str, code: Option<i32>) -> String {
    let text = stderr.to_ascii_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|needle| text.contains(needle));
    if has(&["stat ./src", "open local", "local open"]) {
        "A file you chose changed on this device. Choose it again.".into()
    } else if has(&["permission denied"]) && !has(&["publickey"]) {
        "Permission denied in the computer.".into()
    } else if has(&["no space left", "quota exceeded"]) {
        "The computer is out of space.".into()
    } else if has(&["no such file", "not found"]) && !has(&["proxycommand"]) {
        "That file or folder is no longer available in the computer.".into()
    } else if code == Some(255)
        || has(&[
            "connection closed",
            "connection reset",
            "timed out",
            "kex_exchange",
            "proxycommand",
            "connection refused",
            "host key",
            "publickey",
        ])
    {
        UNREACHABLE.into()
    } else {
        "The transfer failed.".into()
    }
}

/// What a client printed, up to the cap, and whether it printed more.
fn read_capped(mut source: impl Read + Send + 'static) -> std::thread::JoinHandle<(Vec<u8>, bool)> {
    std::thread::spawn(move || {
        let mut kept = Vec::new();
        let _ = (&mut source).take(MAX_OUTPUT_BYTES).read_to_end(&mut kept);
        let overflow = std::io::copy(&mut source, &mut std::io::sink()).unwrap_or(0) > 0;
        (kept, overflow)
    })
}

/// `128 + SIGXFSZ`: how the client's shell reports a write past the file-size limit.
const FILE_SIZE_EXIT: i32 = 128 + libc::SIGXFSZ;

struct Sftp {
    program: PathBuf,
    ssh: PathBuf,
    config: PathBuf,
    alias: String,
}

impl Sftp {
    fn new(config: PathBuf, alias: String) -> Result<Self, String> {
        if !crate::applications::launch::executable_file(Path::new(SFTP)) {
            return Err(MISSING_SFTP.into());
        }
        Ok(Self {
            program: SFTP.into(),
            ssh: SSH.into(),
            config,
            alias,
        })
    }

    /// Runs one batch to completion and returns what `sftp` printed. `tick` runs about
    /// every 50 ms while it works.
    fn run(
        &self,
        batch: &str,
        limit: Option<Duration>,
        cancel: &AtomicBool,
        tick: &mut dyn FnMut(),
    ) -> Result<String, RunError> {
        self.run_watched(batch, limit, cancel, None, &mut || {
            tick();
            None
        })
    }

    /// Like `run`, but the client cannot write a file larger than `max_file` bytes, and
    /// `watch` can end the transfer with a message.
    fn run_watched(
        &self,
        batch: &str,
        limit: Option<Duration>,
        cancel: &AtomicBool,
        max_file: Option<u64>,
        watch: &mut dyn FnMut() -> Option<String>,
    ) -> Result<String, RunError> {
        let mut file = tempfile::Builder::new()
            .prefix("silo-sftp-")
            .tempfile()
            .map_err(|_| RunError::Failed("Could not prepare the transfer.".into()))?;
        file.write_all(batch.as_bytes())
            .and_then(|()| file.flush())
            .map_err(|_| RunError::Failed("Could not prepare the transfer.".into()))?;
        if !self.program.exists() {
            return Err(RunError::Failed(MISSING_SFTP.into()));
        }
        let mut command = Command::new(&self.program);
        command
            .arg("-F")
            .arg(&self.config)
            .arg("-S")
            .arg(&self.ssh)
            .args([
                "-o",
                "ConnectTimeout=15",
                "-o",
                "ServerAliveInterval=15",
                "-o",
                "ServerAliveCountMax=4",
                "-o",
                "ClearAllForwardings=yes",
                "-o",
                "BatchMode=yes",
                "-b",
            ])
            .arg(file.path())
            .arg(&self.alias);
        let mut task = Tunnel::spawn_task(&command, max_file.map(|bytes| bytes + WRITE_SLACK))
            .map_err(|_| RunError::Failed("Could not start the file transfer.".into()))?;
        let (stdout, stderr) = task.take_output();
        let stdout = stdout.map(read_capped);
        let stderr = stderr.map(read_capped);
        let started = Instant::now();
        let status = loop {
            if cancel.load(Ordering::Acquire) {
                drop(task);
                return Err(RunError::Cancelled);
            }
            match task.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {}
                Err(_) => return Err(RunError::Failed("The transfer failed.".into())),
            }
            if limit.is_some_and(|limit| started.elapsed() > limit) {
                return Err(RunError::Failed(UNREACHABLE.into()));
            }
            if let Some(message) = watch() {
                return Err(RunError::Failed(message));
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        // Whatever the client left in its group must not outlive it.
        task.kill_group();
        let (output, overflow) = stdout
            .map(|h| h.join().unwrap_or_default())
            .unwrap_or_default();
        let (errors, _) = stderr
            .map(|h| h.join().unwrap_or_default())
            .unwrap_or_default();
        if max_file.is_some() && status.code() == Some(FILE_SIZE_EXIT) {
            Err(RunError::Failed(TOO_LARGE.into()))
        } else if status.success() {
            if overflow {
                Err(RunError::Failed(LISTING_TOO_LARGE.into()))
            } else {
                Ok(String::from_utf8_lossy(&output).into_owned())
            }
        } else {
            Err(RunError::Failed(failure_message(
                &String::from_utf8_lossy(&errors),
                status.code(),
            )))
        }
    }

    fn quick(&self, batch: &str, limit: Duration) -> Result<String, RunError> {
        self.run(batch, Some(limit), &AtomicBool::new(false), &mut || {})
    }
}

#[derive(Debug)]
struct Source {
    path: PathBuf,
    name: String,
    size: u64,
}

fn prepare_sources(paths: &[String]) -> Result<Vec<Source>, String> {
    if paths.is_empty() || paths.len() > MAX_FILES {
        return Err(format!("Choose between 1 and {MAX_FILES} files to upload."));
    }
    let mut total = 0_u64;
    let mut sources = Vec::with_capacity(paths.len());
    for raw in paths {
        let original = Path::new(raw);
        if !original.is_absolute() {
            return Err("Choose files from this device.".into());
        }
        let name = sanitize_name(
            original
                .file_name()
                .ok_or("This file has no usable name.")?,
        )?;
        let path = fs::canonicalize(original)
            .map_err(|_| format!("\"{name}\" is no longer available on this device."))?;
        let metadata = fs::metadata(&path)
            .map_err(|_| format!("\"{name}\" cannot be read on this device."))?;
        if !metadata.is_file() {
            return Err(if metadata.is_dir() {
                format!("\"{name}\" is a folder. Folders cannot be uploaded yet.")
            } else {
                format!("\"{name}\" is not a regular file.")
            });
        }
        if metadata.len() > MAX_FILE_BYTES {
            return Err(format!(
                "\"{name}\" is larger than 4 GB, the limit for one file."
            ));
        }
        total += metadata.len();
        if total > MAX_BATCH_BYTES {
            return Err("These files add up to more than 8 GB, the limit for one upload.".into());
        }
        sources.push(Source {
            path,
            name,
            size: metadata.len(),
        });
    }
    Ok(sources)
}

struct Folder {
    canonical: String,
    entries: Vec<Entry>,
    available_kib: Option<u64>,
}

/// Opens `directory` in the computer and reads where it really is and what it holds.
fn inspect_folder(
    sftp: &Sftp,
    control: &Control,
    directory: &str,
    create: bool,
) -> Result<Folder, String> {
    let quoted = quote(directory)?;
    let mut batch = String::new();
    if create {
        batch.push_str(&format!("-mkdir {quoted}\n"));
    }
    batch.push_str(&format!("cd {quoted}\npwd\nls -lan\n-df\n"));
    let output = sftp
        .run(&batch, Some(PROBE_LIMIT), control.cancel, &mut || {})
        .map_err(RunError::message)?;
    let probe = parse_probe(&output);
    if !probe.listed_folder {
        return Err(UNREADABLE_FOLDER.into());
    }
    let canonical = probe
        .working_directories
        .last()
        .filter(|path| canonical_within(path, control.roots))
        .ok_or("That folder is not inside a location Silo can transfer files to.")?
        .clone();
    Ok(Folder {
        canonical,
        entries: probe.entries,
        available_kib: probe.available_kib,
    })
}

fn remove_partial(sftp: &Sftp, directory: &str, partial: &str) {
    if let (Ok(directory), Ok(partial)) = (quote(directory), relative(partial)) {
        let _ = sftp.quick(&format!("cd {directory}\n-rm {partial}\n"), CLEANUP_LIMIT);
    }
}

fn upload(
    sftp: &Sftp,
    control: &Control,
    directory: &str,
    sources: &[Source],
    policy: ConflictPolicy,
    create_directory: bool,
) -> Result<UploadOutcome, String> {
    let total: u64 = sources.iter().map(|source| source.size).sum();
    let folder = inspect_folder(sftp, control, directory, create_directory)?;
    if let Some(available) = folder.available_kib {
        if available.saturating_mul(1024) < total {
            return Err(format!(
                "The computer has {} free, which is not enough for these files.",
                format_bytes(available.saturating_mul(1024))
            ));
        }
    }
    let existing: std::collections::HashMap<&str, char> = folder
        .entries
        .iter()
        .map(|entry| (entry.name.as_str(), entry.kind))
        .collect();
    let mut reserved: HashSet<String> = HashSet::new();
    let mut conflicts = Vec::new();
    let mut targets = Vec::with_capacity(sources.len());
    for source in sources {
        let exists = existing.contains_key(source.name.as_str());
        let in_batch = reserved.contains(&source.name);
        let target = if in_batch || (exists && policy == ConflictPolicy::KeepBoth) {
            keep_both_name(&source.name, &|candidate| {
                existing.contains_key(candidate) || reserved.contains(candidate)
            })
        } else {
            if exists
                && existing.get(source.name.as_str()) == Some(&'d')
                && policy == ConflictPolicy::Replace
            {
                return Err(format!(
                    "A folder named \"{}\" already exists there, so the file cannot replace it.",
                    source.name
                ));
            }
            if exists && policy == ConflictPolicy::Ask {
                conflicts.push(source.name.clone());
            }
            source.name.clone()
        };
        reserved.insert(target.clone());
        targets.push(target);
    }
    if !conflicts.is_empty() {
        return Ok(UploadOutcome::Conflict { names: conflicts });
    }
    let count = sources.len();
    let directory_arg = quote(&folder.canonical)?;
    let mut done = 0_u64;
    let mut names = Vec::with_capacity(count);
    for (index, (source, target)) in sources.iter().zip(&targets).enumerate() {
        control.report("upload", "transferring", target, index, count, done, total);
        let stage = tempfile::Builder::new()
            .prefix("silo-upload-")
            .tempdir()
            .map_err(|_| "Could not prepare the upload.")?;
        symlink(&source.path, stage.path().join("src"))
            .map_err(|_| "Could not prepare the upload.")?;
        let partial = partial_name(target, &uuid::Uuid::new_v4().simple().to_string()[..12]);
        // Replacing publishes with the replacing rename. Otherwise the legacy rename
        // refuses an existing name, and the listing of the partial shows whether it did.
        let publish = if policy == ConflictPolicy::Replace {
            format!("rename {} {}\n", relative(&partial)?, relative(target)?)
        } else {
            format!(
                "-rename -l {} {}\n-ls -lan {}\n",
                relative(&partial)?,
                relative(target)?,
                relative(&partial)?
            )
        };
        let batch = format!(
            "lcd {}\ncd {directory_arg}\nput ./src {}\n{publish}",
            quote(
                stage
                    .path()
                    .to_str()
                    .ok_or("Could not prepare the upload.")?
            )?,
            relative(&partial)?,
        );
        let base = done;
        let mut polled = Instant::now();
        let result = sftp.run(&batch, None, control.cancel, &mut || {
            let Some(every) = control.upload_poll else {
                return;
            };
            if polled.elapsed() < every {
                return;
            }
            let arrived = (|| {
                let batch = format!("cd {directory_arg}\nls -lan\n");
                let output = sftp
                    .run(&batch, Some(POLL_LIMIT), control.cancel, &mut || {})
                    .ok()?;
                parse_probe(&output)
                    .entries
                    .into_iter()
                    .find(|entry| entry.name == partial)
                    .map(|entry| entry.size)
            })();
            polled = Instant::now();
            if let Some(size) = arrived {
                control.report(
                    "upload",
                    "transferring",
                    target,
                    index,
                    count,
                    base + size.min(source.size),
                    total,
                );
            }
        });
        let result = result.and_then(|output| {
            if policy == ConflictPolicy::Replace || !lists(&output, &partial) {
                return Ok(target.clone());
            }
            publish_beside_existing(sftp, control, &folder.canonical, &partial, source, &targets)
        });
        match result {
            Ok(published) => {
                done += source.size;
                names.push(published);
            }
            Err(RunError::Cancelled) => {
                remove_partial(sftp, &folder.canonical, &partial);
                return Ok(UploadOutcome::Cancelled);
            }
            Err(error) => {
                remove_partial(sftp, &folder.canonical, &partial);
                return Err(error.message());
            }
        }
    }
    control.report("upload", "done", "", count, count, total, total);
    Ok(UploadOutcome::Done { names })
}

/// Whether a listing of one name shows it.
fn lists(output: &str, name: &str) -> bool {
    parse_probe(output)
        .entries
        .iter()
        .any(|entry| entry.name.strip_prefix("./").unwrap_or(&entry.name) == name)
}

/// The file is stored under its partial name and its target name was taken in the
/// meantime. Tries the next numbered names, never replacing anything, and returns the
/// name it ended up with.
fn publish_beside_existing(
    sftp: &Sftp,
    control: &Control,
    directory: &str,
    partial: &str,
    source: &Source,
    planned: &[String],
) -> Result<String, RunError> {
    let directory_arg = quote(directory).map_err(RunError::Failed)?;
    let partial_arg = relative(partial).map_err(RunError::Failed)?;
    for _ in 0..MAX_PUBLISH_ATTEMPTS {
        let listing = inspect_folder(sftp, control, directory, false).map_err(RunError::Failed)?;
        let taken: HashSet<&str> = listing
            .entries
            .iter()
            .map(|entry| entry.name.as_str())
            .chain(planned.iter().map(String::as_str))
            .collect();
        let candidate = keep_both_name(&source.name, &|name| taken.contains(name));
        let batch = format!(
            "cd {directory_arg}\n-rename -l {partial_arg} {}\n-ls -lan {partial_arg}\n",
            relative(&candidate).map_err(RunError::Failed)?
        );
        let output = sftp.run(&batch, Some(PROBE_LIMIT), control.cancel, &mut || {})?;
        if !lists(&output, partial) {
            return Ok(candidate);
        }
    }
    Err(RunError::Failed(
        "Files with that name keep appearing in the computer. Try again.".into(),
    ))
}

fn download(
    sftp: &Sftp,
    control: &Control,
    remote: &str,
    choose: &dyn Fn(&str) -> Option<PathBuf>,
) -> Result<DownloadOutcome, String> {
    if !within_any(remote, control.roots) {
        return Err(
            "Silo can only download files from the workspace or the Downloads folder.".into(),
        );
    }
    let (parent, name) = remote
        .rsplit_once('/')
        .filter(|(parent, name)| !parent.is_empty() && !name.is_empty())
        .ok_or("This file cannot be downloaded.")?;
    if has_wildcard(name) || name.len() > MAX_NAME_BYTES {
        return Err("This file's name has characters that cannot be downloaded safely. Rename it in the computer first.".into());
    }
    let folder = inspect_folder(sftp, control, parent, false)?;
    let entry = folder
        .entries
        .iter()
        .find(|entry| entry.name == name)
        .ok_or("That file is no longer available in the computer.")?;
    match entry.kind {
        '-' => {}
        'd' => return Err("Folders cannot be downloaded yet.".into()),
        'l' => return Err("Symbolic links cannot be downloaded.".into()),
        _ => return Err("Only regular files can be downloaded.".into()),
    }
    if entry.size > MAX_FILE_BYTES {
        return Err("This file is larger than 4 GB, the limit for one file.".into());
    }
    let size = entry.size;
    let Some(destination) = choose(name) else {
        return Ok(DownloadOutcome::Cancelled);
    };
    let destination_parent = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or("The download destination is unavailable.")?;
    let destination_name = destination
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or("The download destination is unavailable.")?;
    if (control.free_space)(destination_parent)
        .is_some_and(|free| free < size.saturating_add(FREE_SPACE_RESERVE))
    {
        return Err(NO_ROOM_HERE.into());
    }
    let partial = partial_name(
        destination_name,
        &uuid::Uuid::new_v4().simple().to_string()[..12],
    );
    let partial_path = destination_parent.join(&partial);
    // The same session lists the file again just before reading it.
    let batch = format!(
        "lcd {}\ncd {}\npwd\nls -lan {}\nget {} {}\n",
        quote(
            destination_parent
                .to_str()
                .ok_or("The download destination is unavailable.")?
        )?,
        quote(&folder.canonical)?,
        relative(name)?,
        relative(name)?,
        relative(&partial)?,
    );
    control.report("download", "transferring", name, 0, 1, 0, size);
    let mut reported = Instant::now();
    let result = sftp.run_watched(&batch, None, control.cancel, Some(size), &mut || {
        let written = fs::metadata(&partial_path).map(|m| m.len()).ok()?;
        if written > size {
            return Some(TOO_LARGE.into());
        }
        if reported.elapsed() < DOWNLOAD_POLL {
            return None;
        }
        reported = Instant::now();
        if (control.free_space)(destination_parent)
            .is_some_and(|free| free < FREE_SPACE_RESERVE / 2)
        {
            return Some(STOPPED_FOR_ROOM.into());
        }
        control.report("download", "transferring", name, 0, 1, written, size);
        None
    });
    let finish = match result {
        Ok(output) => {
            let probe = parse_probe(&output);
            let unchanged = probe.working_directories.last() == Some(&folder.canonical)
                && probe.entries.iter().any(|entry| {
                    entry.name.strip_prefix("./").unwrap_or(&entry.name) == name
                        && entry.kind == '-'
                        && entry.size == size
                });
            match fs::metadata(&partial_path) {
                _ if !unchanged => Err(
                    "The file changed in the computer while it was downloading. Try again.".into(),
                ),
                Ok(metadata) if metadata.len() == size => {
                    // The client copies the computer's permissions; the saved file is the user's.
                    fs::set_permissions(&partial_path, fs::Permissions::from_mode(0o644))
                        .and_then(|()| fs::rename(&partial_path, &destination))
                        .map(|()| DownloadOutcome::Done {
                            path: destination.to_string_lossy().into_owned(),
                        })
                        .map_err(|_| "Could not save the file at that location.".to_owned())
                }
                Ok(_) => Err("The file changed while it was downloading. Try again.".into()),
                Err(_) => Err("Could not save the file at that location.".into()),
            }
        }
        Err(RunError::Cancelled) => Ok(DownloadOutcome::Cancelled),
        Err(error) => Err(error.message()),
    };
    if !matches!(finish, Ok(DownloadOutcome::Done { .. })) {
        let _ = fs::remove_file(&partial_path);
    } else {
        control.report("download", "done", name, 1, 1, size, size);
    }
    finish
}

static ACTIVE: Mutex<Option<(String, Arc<AtomicBool>)>> = Mutex::new(None);

/// Only one transfer runs at a time; dropping the guard frees the slot.
#[derive(Debug)]
struct Slot {
    id: String,
    cancel: Arc<AtomicBool>,
}

impl Slot {
    fn take(id: &str) -> Result<Self, String> {
        if id.is_empty()
            || id.len() > 64
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        {
            return Err("Invalid transfer request.".into());
        }
        let mut active = ACTIVE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if active.is_some() {
            return Err("Another file transfer is still running.".into());
        }
        let cancel = Arc::new(AtomicBool::new(false));
        *active = Some((id.into(), cancel.clone()));
        Ok(Self {
            id: id.into(),
            cancel,
        })
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        let mut active = ACTIVE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if active.as_ref().is_some_and(|(id, _)| *id == self.id) {
            *active = None;
        }
    }
}

/// Stops the running transfer, if any, which then removes its partial files.
pub(crate) fn cancel_all() {
    if let Some((_, cancel)) = ACTIVE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
    {
        cancel.store(true, Ordering::Release);
    }
}

/// Cancels the running transfer and waits up to `budget` for it to clean up.
pub(crate) fn close_all(budget: Duration) {
    cancel_all();
    let deadline = Instant::now() + budget;
    while Instant::now() < deadline
        && ACTIVE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_some()
    {
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn host_free_space(path: &Path) -> Option<u64> {
    let path = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).ok()?;
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::zeroed();
    // SAFETY: `path` is NUL-terminated and `stats` is a valid out-pointer.
    if unsafe { libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: statvfs succeeded and filled it.
    let stats = unsafe { stats.assume_init() };
    Some((stats.f_bavail as u64).saturating_mul(stats.f_frsize as u64))
}

/// Files the user chose or dropped on this device, held until an upload uses them.
struct Selection {
    token: String,
    owner: String,
    paths: Vec<String>,
    created: Instant,
}

static SELECTIONS: Mutex<Vec<Selection>> = Mutex::new(Vec::new());

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UploadSelection {
    token: String,
    names: Vec<String>,
}

fn display_names(paths: &[String]) -> Vec<String> {
    paths
        .iter()
        .map(|path| {
            Path::new(path)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
        })
        .collect()
}

/// Remembers `paths` for `owner`, the window whose upload may use them.
fn issue_selection(owner: &str, paths: Vec<String>) -> UploadSelection {
    let names = display_names(&paths);
    let token = uuid::Uuid::new_v4().simple().to_string();
    let mut held = SELECTIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    held.retain(|selection| selection.created.elapsed() < SELECTION_TTL);
    while held.len() >= MAX_SELECTIONS {
        held.remove(0);
    }
    held.push(Selection {
        token: token.clone(),
        owner: owner.into(),
        paths,
        created: Instant::now(),
    });
    UploadSelection { token, names }
}

/// Removes and returns a selection issued to `owner`.
fn take_selection(token: &str, owner: &str) -> Result<Vec<String>, String> {
    let mut held = SELECTIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    held.retain(|selection| selection.created.elapsed() < SELECTION_TTL);
    let index = held
        .iter()
        .position(|selection| selection.token == token && selection.owner == owner)
        .ok_or("Choose the files again.")?;
    Ok(held.remove(index).paths)
}

/// Puts a selection back after an upload that only asked a question.
fn restore_selection(token: &str, owner: &str, paths: Vec<String>) {
    let mut held = SELECTIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    held.push(Selection {
        token: token.into(),
        owner: owner.into(),
        paths,
        created: Instant::now(),
    });
}

/// A native file drag over a desktop viewer, on its shell window or on the computer's
/// display. Only the operating system produces these, and only the shell window hears
/// about them: it receives a token for the dropped files, never their paths.
pub(crate) fn native_drop(app: &AppHandle, shell: &str, event: &DragDropEvent) {
    if !crate::desktop_viewer::is_viewer_label(shell) {
        return;
    }
    match event {
        DragDropEvent::Enter { .. } => {
            let _ = app.emit_to(shell, DRAG_EVENT, true);
        }
        DragDropEvent::Leave => {
            let _ = app.emit_to(shell, DRAG_EVENT, false);
        }
        DragDropEvent::Drop { paths, .. } => {
            let _ = app.emit_to(shell, DRAG_EVENT, false);
            let paths: Vec<String> = paths
                .iter()
                .filter_map(|path| path.to_str().map(str::to_owned))
                .collect();
            if !paths.is_empty() {
                let _ = app.emit_to(shell, DROP_EVENT, issue_selection(shell, paths));
            }
        }
        _ => {}
    }
}

fn require_main(window: &Window) -> Result<(), String> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err("Open the main Silo window to transfer files.".into())
    }
}

/// Resolves the private SSH transport for a computer that is ready for transfers.
fn connect(app: &AppHandle, computer: &str) -> Result<Sftp, String> {
    if !crate::applications::launch::executable_file(Path::new(SFTP)) {
        return Err(MISSING_SFTP.into());
    }
    crate::editor::require_openssh("transfer files")?;
    if crate::remote_access::target(computer)?.is_none() {
        let paths = crate::runtime::runtime_paths(app)?;
        crate::terminal::running_computer(&paths, computer).map_err(|message| {
            if message == crate::terminal::start_first(computer) {
                NOT_RUNNING.to_owned()
            } else {
                message
            }
        })?;
    }
    let (alias, config) = crate::editor::private_computer_transport(app, computer, "transfer")?;
    if !alias
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
    {
        return Err("The computer SSH alias contains unsupported characters.".into());
    }
    Sftp::new(config, alias)
}

#[tauri::command]
pub(crate) async fn choose_upload_files(
    app: AppHandle,
    window: Window,
) -> Result<Option<UploadSelection>, String> {
    require_main(&window)?;
    let owner = window.label().to_owned();
    tauri::async_runtime::spawn_blocking(move || {
        let picked = app
            .dialog()
            .file()
            .set_parent(&window)
            .set_title("Upload files")
            .blocking_pick_files()
            .unwrap_or_default();
        let paths = picked
            .into_iter()
            .map(|file| {
                file.into_path()
                    .ok()
                    .and_then(|path| path.to_str().map(str::to_owned))
                    .ok_or_else(|| "A selected file is unavailable.".to_owned())
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok((!paths.is_empty()).then(|| issue_selection(&owner, paths)))
    })
    .await
    .map_err(|_| "The file picker failed.".to_owned())?
}

#[tauri::command]
pub(crate) async fn upload_files(
    app: AppHandle,
    window: Window,
    transfer_id: String,
    computer: String,
    directory: String,
    selection: String,
    conflict: ConflictPolicy,
) -> Result<UploadOutcome, String> {
    crate::runtime::shutdown::ensure_accepting_operations()?;
    crate::desktop_viewer::require_computer(&window, &computer)?;
    let roots = default_roots();
    if !within_any(&directory, &roots) {
        return Err("Silo can only upload to the workspace or the Downloads folder.".into());
    }
    // A desktop window only drops into the Downloads folder.
    if window.label() != "main" && directory != downloads_root() {
        return Err("This window can only upload to the Downloads folder.".into());
    }
    let slot = Slot::take(&transfer_id)?;
    let owner = window.label().to_owned();
    let paths = take_selection(&selection, &owner)?;
    tauri::async_runtime::spawn_blocking(move || {
        let sources = prepare_sources(&paths)?;
        let sftp = connect(&app, &computer)?;
        let emit = |progress: &Progress| {
            let _ = app.emit(PROGRESS_EVENT, progress);
        };
        let control = Control {
            id: &slot.id,
            computer: &computer,
            cancel: &slot.cancel,
            emit: &emit,
            roots: &roots,
            upload_poll: Some(UPLOAD_POLL),
            free_space: &host_free_space,
        };
        let outcome = upload(
            &sftp,
            &control,
            &directory,
            &sources,
            conflict,
            directory == downloads_root(),
        );
        match &outcome {
            Ok(UploadOutcome::Cancelled) => control.report("upload", "cancelled", "", 0, 0, 0, 0),
            Ok(UploadOutcome::Conflict { .. }) => restore_selection(&selection, &owner, paths),
            Err(_) => control.report("upload", "failed", "", 0, 0, 0, 0),
            _ => {}
        }
        outcome
    })
    .await
    .map_err(|_| "The upload failed.".to_owned())?
}

#[tauri::command]
pub(crate) async fn download_file(
    app: AppHandle,
    window: Window,
    transfer_id: String,
    computer: String,
    path: String,
) -> Result<DownloadOutcome, String> {
    require_main(&window)?;
    crate::runtime::shutdown::ensure_accepting_operations()?;
    let roots = default_roots();
    if !within_any(&path, &roots) {
        return Err(
            "Silo can only download files from the workspace or the Downloads folder.".into(),
        );
    }
    let slot = Slot::take(&transfer_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        let sftp = connect(&app, &computer)?;
        let emit = |progress: &Progress| {
            let _ = app.emit(PROGRESS_EVENT, progress);
        };
        let control = Control {
            id: &slot.id,
            computer: &computer,
            cancel: &slot.cancel,
            emit: &emit,
            roots: &roots,
            upload_poll: None,
            free_space: &host_free_space,
        };
        let outcome = download(&sftp, &control, &path, &|suggested| {
            app.dialog()
                .file()
                .set_parent(&window)
                .set_title("Save file")
                .set_file_name(suggested)
                .blocking_save_file()
                .and_then(|selected| selected.into_path().ok())
        });
        match &outcome {
            Ok(DownloadOutcome::Cancelled) => {
                control.report("download", "cancelled", "", 0, 0, 0, 0)
            }
            Err(_) => control.report("download", "failed", "", 0, 0, 0, 0),
            _ => {}
        }
        outcome
    })
    .await
    .map_err(|_| "The download failed.".to_owned())?
}

#[tauri::command]
pub(crate) fn cancel_transfer(window: Window, transfer_id: String) -> Result<(), String> {
    if window.label() != "main" && !window.label().starts_with("desktop-shell-") {
        return Err("This window cannot cancel transfers.".into());
    }
    let active = ACTIVE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((id, cancel)) = active.as_ref() {
        if *id == transfer_id {
            cancel.store(true, Ordering::Release);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, os::unix::fs::PermissionsExt};

    fn server() -> Option<&'static str> {
        [
            "/usr/libexec/sftp-server",
            "/usr/lib/openssh/sftp-server",
            "/usr/lib/ssh/sftp-server",
        ]
        .into_iter()
        .find(|path| Path::new(path).exists())
    }

    /// An `sftp` that serves the local file system through OpenSSH's own server, ignoring
    /// the connection options. A batch that uploads waits `put_delay` seconds first.
    fn local_sftp(directory: &Path, put_delay: &str) -> Option<Sftp> {
        let server = server()?;
        if !Path::new(SFTP).exists() {
            return None;
        }
        let program = directory.join("fake-sftp");
        fs::write(
            &program,
            format!(
                "#!/bin/sh\nbatch=\nwhile [ $# -gt 0 ]; do case \"$1\" in -F|-S|-o) shift 2;; -b) batch=$2; shift 2;; *) shift;; esac; done\ncase \"$(cat \"$batch\")\" in *put*) sleep {put_delay};; esac\nexec {SFTP} -D {server} -b \"$batch\"\n"
            ),
        )
        .unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        Some(Sftp {
            program,
            ssh: SSH.into(),
            config: "/dev/null".into(),
            alias: "fake".into(),
        })
    }

    fn scripted(directory: &Path, body: &str) -> Sftp {
        let program = directory.join("scripted-sftp");
        fs::write(
            &program,
            format!(
                "#!/bin/sh\nbatch=\nwhile [ $# -gt 0 ]; do case \"$1\" in -F|-S|-o) shift 2;; -b) batch=$2; shift 2;; *) shift;; esac; done\n{body}\n"
            ),
        )
        .unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        Sftp {
            program,
            ssh: SSH.into(),
            config: "/dev/null".into(),
            alias: "fake".into(),
        }
    }

    struct World {
        _root: tempfile::TempDir,
        remote: PathBuf,
        local: PathBuf,
        roots: Vec<String>,
        sftp: Sftp,
    }

    fn world() -> Option<World> {
        let root = tempfile::tempdir().unwrap();
        let base = fs::canonicalize(root.path()).unwrap();
        let remote = base.join("remote");
        let local = base.join("local");
        fs::create_dir_all(&remote).unwrap();
        fs::create_dir_all(&local).unwrap();
        let sftp = local_sftp(&base, "0")?;
        Some(World {
            roots: vec![remote.to_str().unwrap().into()],
            _root: root,
            remote,
            local,
            sftp,
        })
    }

    fn control<'a>(
        world: &'a World,
        cancel: &'a AtomicBool,
        emit: &'a dyn Fn(&Progress),
    ) -> Control<'a> {
        Control {
            id: "test",
            computer: "dev",
            cancel,
            emit,
            roots: &world.roots,
            upload_poll: None,
            free_space: &|_| None,
        }
    }

    fn source(world: &World, name: &str, contents: &[u8]) -> Source {
        let path = world.local.join(name);
        fs::write(&path, contents).unwrap();
        Source {
            path,
            name: name.into(),
            size: contents.len() as u64,
        }
    }

    fn names(directory: &Path) -> Vec<String> {
        let mut found: Vec<String> = fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        found.sort();
        found
    }

    #[test]
    fn host_names_are_sanitized_before_they_reach_the_computer() {
        assert_eq!(sanitize_name(OsStr::new("a b.txt")).unwrap(), "a b.txt");
        for bad in ["", ".", "..", "a/b", "a\0b", "line\nbreak", "tab\t"] {
            assert!(sanitize_name(OsStr::new(bad)).is_err(), "{bad:?}");
        }
        assert!(sanitize_name(OsStr::new(&"a".repeat(255))).is_ok());
        assert!(sanitize_name(OsStr::new(&"a".repeat(256))).is_err());
        assert!(sanitize_name(OsStr::new(&"é".repeat(128))).is_err());
    }

    #[test]
    fn allowed_roots_are_exact_and_normalized() {
        let roots = default_roots();
        for ok in [
            "/workspace",
            "/workspace/a/b",
            "/home/silo/Downloads",
            "/home/silo/Downloads/x y",
        ] {
            assert!(within_any(ok, &roots), "{ok}");
        }
        for bad in [
            "/",
            "/home/silo",
            "/home/silo/Downloadsx",
            "/home/silo/Downloads/../.ssh",
            "/home/silo/Downloads//x",
            "/workspace/../etc",
            "/workspacex",
            "workspace/a",
            "/workspace/a\nb",
        ] {
            assert!(!within_any(bad, &roots), "{bad}");
        }
        assert!(canonical_within("/workspace/app", &roots));
        assert!(!canonical_within("/etc", &roots));
        assert!(!canonical_within("/workspace/../etc", &roots));
    }

    #[test]
    fn keep_both_numbers_before_the_extension() {
        let taken: HashSet<&str> = ["a.txt", "a (1).txt", "Makefile", ".env", "a.tar (1).gz"]
            .into_iter()
            .collect();
        let is_taken = |name: &str| taken.contains(name);
        assert_eq!(keep_both_name("a.txt", &is_taken), "a (2).txt");
        assert_eq!(keep_both_name("Makefile", &is_taken), "Makefile (1)");
        assert_eq!(keep_both_name(".env", &is_taken), ".env (1)");
        assert_eq!(keep_both_name("a.tar.gz", &is_taken), "a.tar (2).gz");
        let long = format!("{}.txt", "é".repeat(120));
        assert!(keep_both_name(&long, &|_| false).len() <= 255);
    }

    #[test]
    fn partial_names_never_contain_wildcards_or_separators() {
        let name = partial_name("we ird*[x]?{y}\\\"z/é.txt", "abc");
        assert!(name.starts_with(".we_ird"));
        assert!(name.ends_with(".silo-part-abc"));
        assert!(!has_wildcard(&name) && !name.contains(['/', ' ', '"']));
        assert!(partial_name(&"x".repeat(300), "abc").len() < 255);
    }

    #[test]
    fn batch_arguments_are_quoted_and_control_characters_refused() {
        assert_eq!(quote("a b\"c\\d").unwrap(), "\"a b\\\"c\\\\d\"");
        assert_eq!(relative("-rf").unwrap(), "\"./-rf\"");
        assert!(quote("a\nrm x").is_err());
        assert!(quote("").is_err());
    }

    #[test]
    fn listings_parse_types_sizes_names_and_free_space() {
        let output = "sftp> pwd\nRemote working directory: /workspace/a b\nsftp> ls -lan\ndrwxr-xr-x    ? 1001 1001          160 Oct  3 22:35 .\n-rw-r--r--    ? 1001 1001            5 Oct  3 22:35 two  words.txt\nlrwxrwxrwx    ? 1001 1001            2 Oct  3 22:35 link\ndrwxr-xr-x    ? 1001 1001           64 Oct  3 22:35 dir\nsftp> df\n        Size         Used        Avail       (root)    %Capacity\n   100    40     60     60          40%\n";
        let probe = parse_probe(output);
        assert_eq!(probe.working_directories, ["/workspace/a b"]);
        assert_eq!(probe.available_kib, Some(60));
        let found: Vec<_> = probe
            .entries
            .iter()
            .map(|e| (e.kind, e.size, e.name.as_str()))
            .collect();
        assert_eq!(
            found,
            [
                ('-', 5, "two  words.txt"),
                ('l', 2, "link"),
                ('d', 64, "dir")
            ]
        );
        assert!(probe.listed_folder);
        assert!(!parse_probe("Remote working directory: /x\n").listed_folder);
        assert!(parse_probe("sftp> ls\n").entries.is_empty());
        assert_eq!(parse_probe("-- not a listing --\n"), Probe::default());
    }

    #[test]
    fn failures_are_explained_without_leaking_client_output() {
        let cases = [
            (
                "remote open(\"x\"): Permission denied",
                Some(1),
                "Permission denied in the computer.",
            ),
            (
                "Couldn't canonicalize: No such file or directory",
                Some(1),
                "That file or folder is no longer available in the computer.",
            ),
            (
                "write: No space left on device",
                Some(1),
                "The computer is out of space.",
            ),
            ("Connection closed", Some(255), UNREACHABLE),
            ("anything", Some(255), UNREACHABLE),
            ("odd failure /secret/path", Some(1), "The transfer failed."),
        ];
        for (stderr, code, expected) in cases {
            assert_eq!(failure_message(stderr, code), expected);
        }
    }

    #[test]
    fn local_sources_must_be_regular_files_within_the_limits() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("ok.txt");
        fs::write(&file, b"abc").unwrap();
        let link = dir.path().join("link.txt");
        symlink(&file, &link).unwrap();
        let sources =
            prepare_sources(&[file.to_str().unwrap().into(), link.to_str().unwrap().into()])
                .unwrap();
        assert_eq!(sources[1].name, "link.txt");
        assert_eq!(sources[1].size, 3);
        assert!(prepare_sources(&[]).is_err());
        assert!(prepare_sources(&["relative.txt".into()]).is_err());
        assert!(prepare_sources(&[dir.path().to_str().unwrap().into()])
            .unwrap_err()
            .contains("folder"));
        assert!(prepare_sources(&[dir.path().join("gone").to_str().unwrap().into()]).is_err());
        let many: Vec<String> = (0..=MAX_FILES)
            .map(|_| file.to_str().unwrap().into())
            .collect();
        assert!(prepare_sources(&many).is_err());
    }

    #[test]
    fn a_missing_client_is_explained() {
        let dir = tempfile::tempdir().unwrap();
        let mut sftp = scripted(dir.path(), "exit 0");
        sftp.program = dir.path().join("absent");
        let error = sftp
            .quick("pwd\n", Duration::from_secs(5))
            .unwrap_err()
            .message();
        assert_eq!(error, MISSING_SFTP);
    }

    #[test]
    fn upload_stores_the_file_then_renames_it_and_leaves_no_partial() {
        let Some(world) = world() else { return };
        let events = RefCell::new(Vec::new());
        let emit = |p: &Progress| {
            events
                .borrow_mut()
                .push((p.state, p.bytes_done, p.bytes_total))
        };
        let cancel = AtomicBool::new(false);
        let sources = [source(&world, "[odd] \"name\" *.txt", b"hello")];
        let outcome = upload(
            &world.sftp,
            &control(&world, &cancel, &emit),
            world.roots[0].as_str(),
            &sources,
            ConflictPolicy::Ask,
            false,
        )
        .unwrap();
        assert_eq!(
            outcome,
            UploadOutcome::Done {
                names: vec!["[odd] \"name\" *.txt".into()]
            }
        );
        assert_eq!(names(&world.remote), ["[odd] \"name\" *.txt"]);
        assert_eq!(
            fs::read(world.remote.join("[odd] \"name\" *.txt")).unwrap(),
            b"hello"
        );
        let events = events.borrow();
        assert_eq!(events.first(), Some(&("transferring", 0, 5)));
        assert_eq!(events.last(), Some(&("done", 5, 5)));
    }

    #[test]
    fn upload_into_a_folder_with_wildcards_and_a_leading_dash_name() {
        let Some(world) = world() else { return };
        let folder = world.remote.join("a[b]*dir");
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join("other"), b"x").unwrap();
        let cancel = AtomicBool::new(false);
        let sources = [source(&world, "-rf", b"dash")];
        upload(
            &world.sftp,
            &control(&world, &cancel, &|_| {}),
            folder.to_str().unwrap(),
            &sources,
            ConflictPolicy::Ask,
            false,
        )
        .unwrap();
        assert_eq!(names(&folder), ["-rf", "other"]);
    }

    #[test]
    fn an_existing_name_asks_before_anything_is_sent() {
        let Some(world) = world() else { return };
        fs::write(world.remote.join("a.txt"), b"old").unwrap();
        let cancel = AtomicBool::new(false);
        let sources = [
            source(&world, "a.txt", b"new"),
            source(&world, "b.txt", b"b"),
        ];
        let run = |policy| {
            upload(
                &world.sftp,
                &control(&world, &cancel, &|_| {}),
                world.roots[0].as_str(),
                &sources,
                policy,
                false,
            )
            .unwrap()
        };
        assert_eq!(
            run(ConflictPolicy::Ask),
            UploadOutcome::Conflict {
                names: vec!["a.txt".into()]
            }
        );
        assert_eq!(names(&world.remote), ["a.txt"]);
        assert_eq!(
            run(ConflictPolicy::KeepBoth),
            UploadOutcome::Done {
                names: vec!["a (1).txt".into(), "b.txt".into()]
            }
        );
        assert_eq!(fs::read(world.remote.join("a.txt")).unwrap(), b"old");
        assert_eq!(fs::read(world.remote.join("a (1).txt")).unwrap(), b"new");
        assert_eq!(
            run(ConflictPolicy::Replace),
            UploadOutcome::Done {
                names: vec!["a.txt".into(), "b.txt".into()]
            }
        );
        assert_eq!(fs::read(world.remote.join("a.txt")).unwrap(), b"new");
        assert_eq!(names(&world.remote), ["a (1).txt", "a.txt", "b.txt"]);
    }

    #[test]
    fn replace_refuses_to_overwrite_a_folder_and_batches_never_collide() {
        let Some(world) = world() else { return };
        fs::create_dir(world.remote.join("same")).unwrap();
        let cancel = AtomicBool::new(false);
        let sources = [source(&world, "same", b"x")];
        let error = upload(
            &world.sftp,
            &control(&world, &cancel, &|_| {}),
            world.roots[0].as_str(),
            &sources,
            ConflictPolicy::Replace,
            false,
        )
        .unwrap_err();
        assert!(error.contains("folder named"));
        let copy = world.local.join("copy");
        fs::create_dir(&copy).unwrap();
        fs::write(copy.join("n.txt"), b"2").unwrap();
        let twins = [
            source(&world, "n.txt", b"1"),
            Source {
                path: copy.join("n.txt"),
                name: "n.txt".into(),
                size: 1,
            },
        ];
        let outcome = upload(
            &world.sftp,
            &control(&world, &cancel, &|_| {}),
            world.roots[0].as_str(),
            &twins,
            ConflictPolicy::Replace,
            false,
        )
        .unwrap();
        assert_eq!(
            outcome,
            UploadOutcome::Done {
                names: vec!["n.txt".into(), "n (1).txt".into()]
            }
        );
    }

    #[test]
    fn a_folder_that_resolves_outside_the_roots_is_refused() {
        let Some(mut world) = world() else { return };
        let outside = world.local.join("outside");
        fs::create_dir(&outside).unwrap();
        symlink(&outside, world.remote.join("escape")).unwrap();
        let cancel = AtomicBool::new(false);
        let sources = [source(&world, "a.txt", b"x")];
        let link = world.remote.join("escape");
        let error = upload(
            &world.sftp,
            &control(&world, &cancel, &|_| {}),
            link.to_str().unwrap(),
            &sources,
            ConflictPolicy::Ask,
            false,
        )
        .unwrap_err();
        assert!(error.contains("not inside"));
        assert!(names(&outside).is_empty());
        world.roots.clear();
    }

    #[test]
    fn a_failed_upload_reports_the_cause_and_removes_its_partial() {
        let Some(world) = world() else { return };
        let cancel = AtomicBool::new(false);
        let mut missing = source(&world, "a.txt", b"x");
        missing.path = world.local.join("vanished");
        let error = upload(
            &world.sftp,
            &control(&world, &cancel, &|_| {}),
            world.roots[0].as_str(),
            &[missing],
            ConflictPolicy::Ask,
            false,
        )
        .unwrap_err();
        assert_eq!(
            error,
            "A file you chose changed on this device. Choose it again."
        );
        assert!(names(&world.remote).is_empty());
    }

    #[test]
    fn cancelling_an_upload_stops_the_client_and_removes_the_partial() {
        let Some(world) = world() else { return };
        let marker = world.local.join("started");
        // An upload batch creates its partial file and then hangs; every other batch is real.
        let program = world.sftp.program.clone();
        fs::write(
            &program,
            format!(
                "#!/bin/sh\nbatch=\nwhile [ $# -gt 0 ]; do case \"$1\" in -b) batch=$2; shift 2;; *) shift;; esac; done\nif grep -q '^put' \"$batch\"; then\n  name=$(sed -n 's/^put [^ ]* \"\\.\\/\\(.*\\)\"$/\\1/p' \"$batch\")\n  : > \"{}/$name\"\n  : > {}\n  exec sleep 30\nfi\nexec {SFTP} -D {} -b \"$batch\"\n",
                world.remote.display(),
                marker.display(),
                server().unwrap(),
            ),
        )
        .unwrap();
        let cancel = AtomicBool::new(false);
        let sources = [source(&world, "x.txt", b"data")];
        let started = Instant::now();
        let cancel_ref = &cancel;
        let marker_ref = &marker;
        let outcome = std::thread::scope(|scope| {
            scope.spawn(move || {
                while !marker_ref.exists() {
                    std::thread::sleep(Duration::from_millis(20));
                }
                cancel_ref.store(true, Ordering::Release);
            });
            upload(
                &world.sftp,
                &control(&world, &cancel, &|_| {}),
                world.roots[0].as_str(),
                &sources,
                ConflictPolicy::Ask,
                false,
            )
        });
        assert_eq!(outcome.unwrap(), UploadOutcome::Cancelled);
        assert!(started.elapsed() < Duration::from_secs(20));
        assert!(
            names(&world.remote).is_empty(),
            "{:?}",
            names(&world.remote)
        );
    }

    #[test]
    fn upload_progress_comes_from_the_partial_in_the_computer() {
        let Some(world) = world() else { return };
        let cancel = AtomicBool::new(false);
        let events = RefCell::new(Vec::new());
        let emit = |p: &Progress| events.borrow_mut().push(p.bytes_done);
        let mut control = control(&world, &cancel, &emit);
        control.upload_poll = Some(Duration::from_millis(100));
        let sources = [source(&world, "big.bin", &[7; 1000])];
        // An upload batch leaves 123 bytes in its partial file and then takes a second to finish.
        fs::write(
            &world.sftp.program,
            format!(
                "#!/bin/sh\nbatch=\nwhile [ $# -gt 0 ]; do case \"$1\" in -b) batch=$2; shift 2;; *) shift;; esac; done\nif grep -q '^put' \"$batch\"; then\n  name=$(sed -n 's/^put [^ ]* \"\\.\\/\\(.*\\)\"$/\\1/p' \"$batch\")\n  head -c 123 /dev/zero > \"{}/$name\"\n  sleep 1\n  exit 0\nfi\nexec {SFTP} -D {} -b \"$batch\"\n",
                world.remote.display(),
                server().unwrap(),
            ),
        )
        .unwrap();
        upload(
            &world.sftp,
            &control,
            world.roots[0].as_str(),
            &sources,
            ConflictPolicy::Ask,
            false,
        )
        .unwrap();
        assert!(events.borrow().contains(&123), "{:?}", events.borrow());
    }

    #[test]
    fn free_space_is_checked_when_the_computer_reports_it() {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let sftp = scripted(
            dir.path(),
            &format!("printf 'Remote working directory: {}\\ndrwxr-xr-x    ? 1 1 160 Oct  3 22:35 .\\n   Size Used Avail (root)\\n 10 9 1 1 90%%\\n'", root.display()),
        );
        let roots = vec![root.to_str().unwrap().to_owned()];
        let cancel = AtomicBool::new(false);
        let world = World {
            _root: tempfile::tempdir().unwrap(),
            remote: root.clone(),
            local: root.clone(),
            roots,
            sftp,
        };
        let file = root.join("f");
        fs::write(&file, vec![0; 4096]).unwrap();
        let error = upload(
            &world.sftp,
            &control(&world, &cancel, &|_| {}),
            root.to_str().unwrap(),
            &[Source {
                path: file,
                name: "f".into(),
                size: 4096,
            }],
            ConflictPolicy::Ask,
            false,
        )
        .unwrap_err();
        assert!(error.contains("not enough"), "{error}");
    }

    #[test]
    fn download_saves_through_a_partial_and_renames_it() {
        let Some(world) = world() else { return };
        fs::write(world.remote.join("report.txt"), b"contents").unwrap();
        let destination = world.local.join("saved copy.txt");
        let events = RefCell::new(Vec::new());
        let emit = |p: &Progress| events.borrow_mut().push((p.state, p.bytes_done));
        let cancel = AtomicBool::new(false);
        let suggested = RefCell::new(String::new());
        let remote = world.remote.join("report.txt");
        let outcome = download(
            &world.sftp,
            &control(&world, &cancel, &emit),
            remote.to_str().unwrap(),
            &|name| {
                *suggested.borrow_mut() = name.into();
                Some(destination.clone())
            },
        )
        .unwrap();
        assert_eq!(
            outcome,
            DownloadOutcome::Done {
                path: destination.to_string_lossy().into()
            }
        );
        assert_eq!(&*suggested.borrow(), "report.txt");
        assert_eq!(fs::read(&destination).unwrap(), b"contents");
        assert_eq!(names(&world.local), ["saved copy.txt"]);
        assert_eq!(events.borrow().last(), Some(&("done", 8)));
    }

    #[test]
    fn download_refuses_links_folders_wildcard_names_and_other_roots() {
        let Some(world) = world() else { return };
        fs::write(world.remote.join("real"), b"x").unwrap();
        symlink(world.remote.join("real"), world.remote.join("link")).unwrap();
        fs::create_dir(world.remote.join("dir")).unwrap();
        fs::write(world.remote.join("wild*card"), b"x").unwrap();
        let cancel = AtomicBool::new(false);
        let chosen = world.local.join("out");
        let ask = |_: &str| Some(chosen.clone());
        let attempt = |path: String| {
            download(&world.sftp, &control(&world, &cancel, &|_| {}), &path, &ask).unwrap_err()
        };
        assert!(attempt(world.remote.join("link").to_str().unwrap().into()).contains("Symbolic"));
        assert!(attempt(world.remote.join("dir").to_str().unwrap().into()).contains("Folders"));
        assert!(
            attempt(world.remote.join("wild*card").to_str().unwrap().into()).contains("characters")
        );
        assert!(
            attempt(world.remote.join("absent").to_str().unwrap().into()).contains("no longer")
        );
        assert!(attempt("/etc/passwd".into()).contains("only download"));
        assert!(!chosen.exists());
    }

    #[test]
    fn a_download_through_a_linked_folder_is_refused() {
        let Some(world) = world() else { return };
        let outside = world.local.join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("secret"), b"x").unwrap();
        symlink(&outside, world.remote.join("escape")).unwrap();
        let cancel = AtomicBool::new(false);
        let path = world.remote.join("escape/secret");
        let error = download(
            &world.sftp,
            &control(&world, &cancel, &|_| {}),
            path.to_str().unwrap(),
            &|_| Some(world.local.join("out")),
        )
        .unwrap_err();
        assert!(error.contains("not inside"));
    }

    #[test]
    fn dismissing_the_save_dialog_cancels_without_transferring() {
        let Some(world) = world() else { return };
        fs::write(world.remote.join("a"), b"x").unwrap();
        let cancel = AtomicBool::new(false);
        let path = world.remote.join("a");
        let outcome = download(
            &world.sftp,
            &control(&world, &cancel, &|_| {}),
            path.to_str().unwrap(),
            &|_| None,
        )
        .unwrap();
        assert_eq!(outcome, DownloadOutcome::Cancelled);
        assert!(names(&world.local).is_empty());
    }

    #[test]
    fn a_cancelled_or_failed_download_leaves_no_partial_beside_the_destination() {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let destination = root.join("out");
        let listing = format!(
            "printf 'Remote working directory: {}\\ndrwxr-xr-x    ? 1 1 160 Oct  3 22:35 .\\n-rw-r--r--    ? 1 1 4 Oct  3 22:35 f\\n'",
            root.display()
        );
        let roots = vec![root.to_str().unwrap().to_owned()];
        let path = format!("{}/f", root.display());
        // The get step writes some bytes and then fails.
        let failing = scripted(
            dir.path(),
            &format!(
                "if grep -q '^get' \"$batch\"; then name=$(sed -n 's/^get [^ ]* \"\\.\\/\\(.*\\)\"$/\\1/p' \"$batch\"); echo partial > \"{}/$name\"; echo 'Permission denied' >&2; exit 1; fi\n{listing}",
                root.display()
            ),
        );
        let cancel = AtomicBool::new(false);
        let world = World {
            _root: tempfile::tempdir().unwrap(),
            remote: root.clone(),
            local: root.clone(),
            roots,
            sftp: failing,
        };
        let error = download(
            &world.sftp,
            &control(&world, &cancel, &|_| {}),
            &path,
            &|_| Some(destination.clone()),
        )
        .unwrap_err();
        assert_eq!(error, "Permission denied in the computer.");
        assert!(!destination.exists());
        assert!(
            !names(&root).iter().any(|n| n.contains("silo-part")),
            "{:?}",
            names(&root)
        );
    }

    #[test]
    fn cancelling_stops_the_whole_process_group() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("child.pid");
        let sftp = scripted(
            dir.path(),
            &format!("sleep 30 &\necho $! > {}\nwait", pid_file.display()),
        );
        let cancel = AtomicBool::new(false);
        let outcome = std::thread::scope(|scope| {
            let cancel_ref = &cancel;
            let pid_ref = &pid_file;
            scope.spawn(move || {
                while std::fs::read_to_string(pid_ref).map_or(true, |text| text.trim().is_empty()) {
                    std::thread::sleep(Duration::from_millis(20));
                }
                cancel_ref.store(true, Ordering::Release);
            });
            sftp.run("pwd\n", None, &cancel, &mut || {})
        });
        assert!(matches!(outcome, Err(RunError::Cancelled)));
        let pid: i32 = fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while unsafe { libc::kill(pid, 0) } == 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_ne!(
            unsafe { libc::kill(pid, 0) },
            0,
            "the client's child outlived the cancel"
        );
    }

    #[test]
    fn a_stalled_client_is_stopped_at_its_limit() {
        let dir = tempfile::tempdir().unwrap();
        let sftp = scripted(dir.path(), "sleep 30");
        let started = Instant::now();
        let error = sftp
            .quick("pwd\n", Duration::from_millis(300))
            .unwrap_err()
            .message();
        assert_eq!(error, UNREACHABLE);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn transfers_run_one_at_a_time_and_ids_are_checked() {
        for bad in ["", "a b", "../x", &"a".repeat(65)] {
            assert!(Slot::take(bad).is_err(), "{bad:?}");
        }
        let first = Slot::take("one").unwrap();
        assert!(Slot::take("two").unwrap_err().contains("still running"));
        cancel_all();
        assert!(first.cancel.load(Ordering::Acquire));
        let started = Instant::now();
        std::thread::scope(|scope| {
            scope.spawn(|| {
                std::thread::sleep(Duration::from_millis(150));
                drop(first);
            });
            close_all(Duration::from_secs(5));
        });
        assert!(started.elapsed() < Duration::from_secs(4));
        drop(Slot::take("two").unwrap());
    }

    /// A client for the real server whose batches that contain `needle` first run `before`,
    /// a shell snippet that can use `$batch`.
    fn intercepted(world: &World, needle: &str, before: &str) {
        fs::write(
            &world.sftp.program,
            format!(
                "#!/bin/sh\nbatch=\nwhile [ $# -gt 0 ]; do case \"$1\" in -b) batch=$2; shift 2;; *) shift;; esac; done\nif grep -q '{needle}' \"$batch\"; then\n{before}\nfi\nexec {SFTP} -D {} -b \"$batch\"\n",
                server().unwrap(),
            ),
        )
        .unwrap();
    }

    fn scripted_listing(dir: &Path, root: &Path, size: u64, on_get: &str) -> Sftp {
        scripted(
            dir,
            &format!(
                "if grep -q '^get' \"$batch\"; then name=$(sed -n 's/^get [^ ]* \"\\.\\/\\(.*\\)\"$/\\1/p' \"$batch\"); partial=\"{root}/$name\"\n{on_get}\nfi\nprintf 'Remote working directory: {root}\\ndrwxr-xr-x    ? 1 1 160 Oct  3 22:35 .\\n-rw-r--r--    ? 1 1 {size} Oct  3 22:35 f\\n'",
                root = root.display(),
            ),
        )
    }

    fn download_world(
        dir: &tempfile::TempDir,
        size: u64,
        on_get: &str,
    ) -> (World, PathBuf, String) {
        let root = fs::canonicalize(dir.path()).unwrap();
        let world = World {
            _root: tempfile::tempdir().unwrap(),
            remote: root.clone(),
            local: root.clone(),
            roots: vec![root.to_str().unwrap().to_owned()],
            sftp: scripted_listing(dir.path(), &root, size, on_get),
        };
        let path = format!("{}/f", root.display());
        (world, root.join("out"), path)
    }

    fn leftovers(directory: &Path) -> Vec<String> {
        names(directory)
            .into_iter()
            .filter(|name| name.contains("silo-part"))
            .collect()
    }

    #[test]
    fn a_guest_that_sends_more_than_it_listed_is_stopped_and_its_partial_removed() {
        let dir = tempfile::tempdir().unwrap();
        let (world, destination, path) =
            download_world(&dir, 4, "head -c 100 /dev/zero > \"$partial\"; sleep 30");
        let cancel = AtomicBool::new(false);
        let started = Instant::now();
        let error = download(
            &world.sftp,
            &control(&world, &cancel, &|_| {}),
            &path,
            &|_| Some(destination.clone()),
        )
        .unwrap_err();
        assert_eq!(error, TOO_LARGE);
        assert!(started.elapsed() < Duration::from_secs(10));
        assert!(!destination.exists());
        assert!(leftovers(&world.local).is_empty());
    }

    #[test]
    fn the_client_cannot_write_far_past_the_listed_size_even_between_checks() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("written");
        let (world, destination, path) = download_world(
            &dir,
            4,
            &format!(
                "head -c 3000000 /dev/zero > \"$partial\"; code=$?; wc -c < \"$partial\" > {}; exit $code",
                marker.display()
            ),
        );
        let cancel = AtomicBool::new(false);
        let error = download(
            &world.sftp,
            &control(&world, &cancel, &|_| {}),
            &path,
            &|_| Some(destination.clone()),
        )
        .unwrap_err();
        assert_eq!(error, TOO_LARGE);
        let written: u64 = fs::read_to_string(&marker).unwrap().trim().parse().unwrap();
        assert!(written <= 4 + WRITE_SLACK, "{written}");
        assert!(!destination.exists());
    }

    #[test]
    fn a_download_needs_room_on_this_device_before_and_while_it_runs() {
        let dir = tempfile::tempdir().unwrap();
        let (world, destination, path) = download_world(&dir, 4, "sleep 30");
        let cancel = AtomicBool::new(false);
        let mut scarce = control(&world, &cancel, &|_| {});
        scarce.free_space = &|_| Some(FREE_SPACE_RESERVE);
        let error =
            download(&world.sftp, &scarce, &path, &|_| Some(destination.clone())).unwrap_err();
        assert_eq!(error, NO_ROOM_HERE);
        assert!(leftovers(&world.local).is_empty());

        let dir = tempfile::tempdir().unwrap();
        let (world, destination, path) =
            download_world(&dir, 4, "echo ab > \"$partial\"; sleep 30");
        let calls = std::cell::Cell::new(0);
        let shrinking = |_: &Path| {
            calls.set(calls.get() + 1);
            Some(if calls.get() == 1 { 1 << 40 } else { 0 })
        };
        let mut running = control(&world, &cancel, &|_| {});
        running.free_space = &shrinking;
        let error =
            download(&world.sftp, &running, &path, &|_| Some(destination.clone())).unwrap_err();
        assert_eq!(error, STOPPED_FOR_ROOM);
        assert!(leftovers(&world.local).is_empty());
    }

    #[test]
    fn a_file_swapped_for_a_link_before_it_is_read_is_discarded() {
        let Some(world) = world() else { return };
        fs::write(world.remote.join("f"), b"data").unwrap();
        fs::write(world.local.join("secret"), b"other").unwrap();
        intercepted(
            &world,
            "^get",
            &format!(
                "rm {r}/f; ln -s {l}/secret {r}/f",
                r = world.remote.display(),
                l = world.local.display()
            ),
        );
        let destination = world.local.join("out");
        let cancel = AtomicBool::new(false);
        let path = world.remote.join("f");
        let error = download(
            &world.sftp,
            &control(&world, &cancel, &|_| {}),
            path.to_str().unwrap(),
            &|_| Some(destination.clone()),
        )
        .unwrap_err();
        assert!(error.contains("changed in the computer"), "{error}");
        assert!(!destination.exists());
        assert!(leftovers(&world.local).is_empty());
    }

    #[test]
    fn a_saved_file_gets_ordinary_permissions_whatever_the_computer_had() {
        let Some(world) = world() else { return };
        let remote = world.remote.join("ro");
        fs::write(&remote, b"data").unwrap();
        fs::set_permissions(&remote, fs::Permissions::from_mode(0o400)).unwrap();
        let destination = world.local.join("saved");
        let cancel = AtomicBool::new(false);
        download(
            &world.sftp,
            &control(&world, &cancel, &|_| {}),
            remote.to_str().unwrap(),
            &|_| Some(destination.clone()),
        )
        .unwrap();
        let mode = fs::metadata(&destination).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o644);
    }

    #[test]
    fn an_unreadable_folder_is_an_error_not_an_empty_listing() {
        let Some(world) = world() else { return };
        let closed = world.remote.join("closed");
        fs::create_dir(&closed).unwrap();
        fs::set_permissions(&closed, fs::Permissions::from_mode(0o311)).unwrap();
        let cancel = AtomicBool::new(false);
        let sources = [source(&world, "a.txt", b"x")];
        let readable = fs::read_dir(&closed).is_ok();
        let outcome = upload(
            &world.sftp,
            &control(&world, &cancel, &|_| {}),
            closed.to_str().unwrap(),
            &sources,
            ConflictPolicy::Ask,
            false,
        );
        fs::set_permissions(&closed, fs::Permissions::from_mode(0o755)).unwrap();
        if !readable {
            assert_eq!(outcome.unwrap_err(), UNREADABLE_FOLDER);
            assert!(names(&closed).is_empty());
        }
    }

    #[test]
    fn a_listing_that_names_no_folder_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let sftp = scripted(
            dir.path(),
            &format!("printf 'Remote working directory: {}\\n'", root.display()),
        );
        let world = World {
            _root: tempfile::tempdir().unwrap(),
            remote: root.clone(),
            local: root.clone(),
            roots: vec![root.to_str().unwrap().to_owned()],
            sftp,
        };
        let cancel = AtomicBool::new(false);
        let error = inspect_folder(
            &world.sftp,
            &control(&world, &cancel, &|_| {}),
            root.to_str().unwrap(),
            false,
        )
        .map(|_| ())
        .unwrap_err();
        assert_eq!(error, UNREADABLE_FOLDER);
    }

    #[test]
    fn a_listing_over_the_output_cap_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let sftp = scripted(dir.path(), "head -c 5000000 /dev/zero | tr '\\0' 'a'");
        let error = sftp
            .quick("ls\n", Duration::from_secs(20))
            .unwrap_err()
            .message();
        assert_eq!(error, LISTING_TOO_LARGE);
    }

    #[test]
    fn keep_both_names_stay_within_the_name_limit_with_any_extension() {
        for name in [
            format!("{}.txt", "a".repeat(251)),
            format!("a.{}", "x".repeat(253)),
            "é".repeat(127),
            format!("{}.{}", "é".repeat(60), "ñ".repeat(60)),
        ] {
            assert!(name.len() <= 255, "{}", name.len());
            let first = keep_both_name(&name, &|_| false);
            assert!(
                first.len() <= 255 && first.contains(" (1)"),
                "{}",
                first.len()
            );
            let again = keep_both_name(&name, &|candidate| candidate == first);
            assert!(again.len() <= 255 && again != first);
        }
    }

    #[test]
    fn a_name_taken_after_inspection_is_never_overwritten() {
        let Some(world) = world() else { return };
        intercepted(
            &world,
            "^put",
            &format!(
                "echo racer > '{r}/a.txt'; echo racer > '{r}/a (1).txt'",
                r = world.remote.display()
            ),
        );
        let cancel = AtomicBool::new(false);
        let sources = [source(&world, "a.txt", b"mine")];
        for policy in [ConflictPolicy::Ask, ConflictPolicy::KeepBoth] {
            let outcome = upload(
                &world.sftp,
                &control(&world, &cancel, &|_| {}),
                world.roots[0].as_str(),
                &sources,
                policy,
                false,
            )
            .unwrap();
            let UploadOutcome::Done { names: stored } = outcome else {
                panic!("{outcome:?}")
            };
            assert_eq!(stored.len(), 1);
            assert!(stored[0].starts_with("a (2)"), "{stored:?}");
            assert_eq!(fs::read(world.remote.join(&stored[0])).unwrap(), b"mine");
            fs::remove_file(world.remote.join(&stored[0])).unwrap();
        }
        assert_eq!(fs::read(world.remote.join("a.txt")).unwrap(), b"racer\n");
        assert_eq!(
            fs::read(world.remote.join("a (1).txt")).unwrap(),
            b"racer\n"
        );
        assert_eq!(names(&world.remote), ["a (1).txt", "a.txt"]);
    }

    #[test]
    fn replacing_still_replaces_a_file_taken_after_inspection() {
        let Some(world) = world() else { return };
        intercepted(
            &world,
            "^put",
            &format!("echo racer > '{}/a.txt'", world.remote.display()),
        );
        let cancel = AtomicBool::new(false);
        let sources = [source(&world, "a.txt", b"mine")];
        upload(
            &world.sftp,
            &control(&world, &cancel, &|_| {}),
            world.roots[0].as_str(),
            &sources,
            ConflictPolicy::Replace,
            false,
        )
        .unwrap();
        assert_eq!(fs::read(world.remote.join("a.txt")).unwrap(), b"mine");
        assert_eq!(names(&world.remote), ["a.txt"]);
    }

    #[test]
    fn a_transfer_client_ends_when_silo_stops_holding_its_pipe() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("child.pid");
        let mut command = Command::new("/bin/sh");
        command
            .arg("-c")
            .arg(format!("sleep 30 & echo $! > {}; wait", pid_file.display()));
        let mut task = Tunnel::spawn_task(&command, None).unwrap();
        while fs::read_to_string(&pid_file).map_or(true, |text| text.trim().is_empty()) {
            std::thread::sleep(Duration::from_millis(20));
        }
        let pid: i32 = fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        task.close_lifetime_pipe();
        let deadline = Instant::now() + Duration::from_secs(5);
        while unsafe { libc::kill(pid, 0) } == 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_ne!(unsafe { libc::kill(pid, 0) }, 0, "the client outlived Silo");
        drop(task);
    }

    #[test]
    fn selections_belong_to_their_window_and_are_used_once() {
        let first = issue_selection("main", vec!["/Users/ada/a.txt".into(), "/b/c.pdf".into()]);
        assert_eq!(first.names, ["a.txt", "c.pdf"]);
        assert!(take_selection(&first.token, "desktop-shell-1").is_err());
        assert!(take_selection("not-a-token", "main").is_err());
        let paths = take_selection(&first.token, "main").unwrap();
        assert_eq!(paths, ["/Users/ada/a.txt", "/b/c.pdf"]);
        assert!(take_selection(&first.token, "main").is_err());
        restore_selection(&first.token, "main", paths);
        assert!(take_selection(&first.token, "main").is_ok());
        let tokens: Vec<_> = (0..MAX_SELECTIONS + 4)
            .map(|index| issue_selection("main", vec![format!("/f{index}")]).token)
            .collect();
        assert!(take_selection(&tokens[0], "main").is_err());
        assert!(take_selection(tokens.last().unwrap(), "main").is_ok());
    }
}
