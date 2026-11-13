//! The transfer area: where one machine hands a computer's export file to another over
//! Connections.
//!
//! The Connections SSH key may only run the bridge, so `scp` and `sftp` cannot reach a file on
//! the owner. Instead the owner keeps `<engine dir>/transfer/` (directory 0700, files 0600) and
//! serves exactly one file of it per `exports.file` stream:
//!
//! - **Export files** (`x-<token>.silo-backup`) are written by the owner's own export job and
//!   may only be read, once. The owner picks the name; a client never names a path.
//! - **Import files** (`i-<token>.silo-backup`) are created by a write stream, which mints the
//!   name, and may only be imported from (by the owner's import job), never read back.
//!
//! A write goes to `<name>.part` with a declared size and sha256, after a free-space check that
//! counts the bytes other writes of this process have reserved and not yet finished (up to the
//! stream budget in `remote.rs` may be open at once), and is renamed to its name only when both
//! match; any other end deletes the `.part` and gives its reservation back. Removal is an
//! explicit owner call after a successful copy, and a sweep clears what is left.
//!
//! The stream is one JSON header line from the owner, then the file's bytes, then (for a write)
//! one JSON verdict line from the owner. `serve` is the owner's half and `pull` and `push` are
//! the controller's; they are generic over the byte streams so the whole exchange runs over a
//! pair of sockets in tests.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, SystemTime},
};

/// The stream method that carries one transfer-area file.
pub const FILE_STREAM: &str = "exports.file";
/// The folder of the engine directory that holds the area.
pub const DIRECTORY_NAME: &str = "transfer";
const EXTENSION: &str = "silo-backup";
const EXPORT_PREFIX: &str = "x-";
const IMPORT_PREFIX: &str = "i-";
const PART_SUFFIX: &str = ".part";
const USED_SUFFIX: &str = ".used";
/// The largest file the area carries. The engine's own export has no smaller limit (8 TiB), so
/// this is the transfer's: a larger declared size is refused before anything is reserved, and a
/// file of this size has a time budget of days at the slowest allowed rate (`time_budget`).
pub const MAX_BYTES: u64 = 16 * 1024 * 1024 * 1024;
/// Free space left beyond the file itself on the volume that receives it.
pub const FREE_SPACE_MARGIN: u64 = 64 * 1024 * 1024;
/// How old an entry is before the periodic sweep removes it.
pub const STALE_AFTER: Duration = Duration::from_secs(24 * 60 * 60);
/// How long the owner waits for the next bytes of a write before it gives up on it.
pub const IDLE_LIMIT: Duration = Duration::from_secs(120);
const CHUNK: usize = 64 * 1024;
const HEADER_LIMIT: u64 = 4096;
const TOKEN_LENGTH: usize = 32;

/// The bytes promised to writes that have not finished, per area folder. Free space is read from
/// the volume, which the declared size has not yet used, so concurrent writes would each pass
/// the same check; this is what they subtract.
static RESERVED: Mutex<Vec<(PathBuf, u64)>> = Mutex::new(Vec::new());

fn release_reserved(dir: &Path, bytes: u64) {
    let mut reserved = RESERVED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(index) = reserved.iter().position(|(path, _)| path == dir) {
        reserved[index].1 = reserved[index].1.saturating_sub(bytes);
        if reserved[index].1 == 0 {
            reserved.swap_remove(index);
        }
    }
}

/// Which way a file in the area may be used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// Written by an export job; may be read over the stream once.
    Export,
    /// Written by a stream; may only be imported from.
    Import,
}

fn is_token(text: &str) -> bool {
    text.len() == TOKEN_LENGTH && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The role a name carries, or `None` when it is not a name the area ever hands out.
pub fn parse_name(name: &str) -> Option<Role> {
    let stem = name.strip_suffix(&format!(".{EXTENSION}"))?;
    if let Some(token) = stem.strip_prefix(EXPORT_PREFIX) {
        return is_token(token).then_some(Role::Export);
    }
    let token = stem.strip_prefix(IMPORT_PREFIX)?;
    is_token(token).then_some(Role::Import)
}

fn new_name(role: Role) -> String {
    let prefix = match role {
        Role::Export => EXPORT_PREFIX,
        Role::Import => IMPORT_PREFIX,
    };
    format!(
        "{prefix}{}.{EXTENSION}",
        uuid::Uuid::new_v4().simple().to_string()
    )
}

/// Whether `text` is a lowercase hex sha256.
pub fn is_sha256(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// What the owner reports for a file in the area.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileInfo {
    pub name: String,
    pub bytes: u64,
    pub sha256: String,
}

/// How long a copy of `bytes` may take: the Files budget (45 s plus the time the size needs at
/// 64 KiB/s).
pub fn time_budget(bytes: u64) -> Duration {
    crate::transfer::transfer_limit(bytes)
}

/// The size and sha256 of the file at `path`.
pub fn digest(path: &Path, cancelled: &dyn Fn() -> bool) -> Result<(u64, String), String> {
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; CHUNK];
    let mut bytes = 0u64;
    loop {
        if cancelled() {
            return Err("Cancelled.".into());
        }
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        bytes += read as u64;
    }
    Ok((bytes, hex(&hasher.finalize())))
}

/// Puts `source` at `destination` without ever replacing a file there, and returns
/// `AlreadyExists` when something is already there. The kernel's exclusive rename does it in one
/// step; a volume that lacks it (some SMB shares, FAT) gets a hard link followed by removing
/// `source`; a volume that has neither (exFAT, vfat) gets an existence check and a plain
/// rename, whose only gap is something appearing at `destination` in between.
pub fn publish_without_replacing(source: &Path, destination: &Path) -> std::io::Result<()> {
    publish_without_replacing_with(
        source,
        destination,
        crate::backup::exclusive_rename,
        |source, destination| fs::hard_link(source, destination),
    )
}

fn publish_without_replacing_with(
    source: &Path,
    destination: &Path,
    exclusive: impl Fn(&Path, &Path) -> std::io::Result<()>,
    link: impl Fn(&Path, &Path) -> std::io::Result<()>,
) -> std::io::Result<()> {
    // ENOTSUP and EOPNOTSUPP are the same value on Linux and differ on macOS.
    let unsupported = |error: &std::io::Error| {
        error.raw_os_error().is_some_and(|code| {
            [libc::EINVAL, libc::ENOTSUP, libc::EOPNOTSUPP, libc::ENOSYS].contains(&code)
        })
    };
    match exclusive(source, destination) {
        Err(error) if unsupported(&error) => {}
        result => return result,
    }
    match link(source, destination) {
        Ok(()) => {
            let _ = fs::remove_file(source);
            return Ok(());
        }
        Err(error)
            if unsupported(&error)
                || matches!(error.raw_os_error(), Some(libc::EPERM | libc::EMLINK)) => {}
        Err(error) => return Err(error),
    }
    if fs::symlink_metadata(destination).is_ok() {
        return Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists));
    }
    fs::rename(source, destination)
}

/// The bytes available to this account at `path` (or its nearest existing parent).
pub fn free_space(path: &Path) -> Result<u64, String> {
    crate::backup::available_bytes(path)
        .map_err(|error| format!("Silo could not read the free space there: {error}"))
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

/// The transfer area of one engine directory.
#[derive(Clone, Debug)]
pub struct Area {
    dir: PathBuf,
}

impl Area {
    /// The area under `engine_directory`, created private when it is missing. A folder that is
    /// a link, or belongs to another account, is refused.
    pub fn at(engine_directory: &Path) -> Result<Self, String> {
        let dir = engine_directory.join(DIRECTORY_NAME);
        match fs::symlink_metadata(&dir) {
            Ok(metadata) => {
                if !metadata.is_dir() {
                    return Err("The transfer folder is not a folder.".into());
                }
                if metadata.uid() != unsafe { libc::geteuid() } {
                    return Err("The transfer folder belongs to another account.".into());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
            }
            Err(error) => return Err(error.to_string()),
        }
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
        // The export controller works with canonical folders, so the area is named that way.
        let dir = dir.canonicalize().map_err(|error| error.to_string())?;
        Ok(Self { dir })
    }

    /// The area of the engine behind `context`.
    pub fn of_engine(context: &dyn crate::context::EngineContext) -> Result<Self, String> {
        Self::at(&crate::engine_update::engine_directory(context))
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// A fresh export name and the path the owner's export job writes it to.
    pub fn export_target(&self) -> (String, PathBuf) {
        let name = new_name(Role::Export);
        let path = self.dir.join(&name);
        (name, path)
    }

    /// Whether `path` is the area or lies inside it, wherever its links lead. A path that does
    /// not exist yet is judged by its nearest existing parent.
    pub fn contains(&self, path: &Path) -> bool {
        let mut existing = path;
        loop {
            if let Ok(canonical) = existing.canonicalize() {
                return canonical.starts_with(&self.dir);
            }
            match existing.parent() {
                Some(parent) => existing = parent,
                None => return false,
            }
        }
    }

    /// Whether `path` is an export file this area would hand out.
    pub fn owns_export_path(&self, path: &Path) -> bool {
        path.parent() == Some(self.dir.as_path())
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(parse_name)
                == Some(Role::Export)
    }

    /// The path of an import file for the owner's import job. Refused unless `name` is an import
    /// name of a file that is here.
    pub fn import_path(&self, name: &str) -> Result<PathBuf, String> {
        if parse_name(name) != Some(Role::Import) {
            return Err("That is not a file this machine received.".into());
        }
        let path = self.dir.join(name);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_file() => Ok(path),
            _ => Err("The received file is no longer here. Send it again.".into()),
        }
    }

    /// Opens an export file for its one read. The second open of a name is refused, and so is
    /// a name that is not an export name or has no file.
    pub fn open_for_read(&self, name: &str) -> Result<(File, u64), String> {
        if parse_name(name) != Some(Role::Export) {
            return Err("That is not a file this machine can send.".into());
        }
        let path = self.dir.join(name);
        let metadata = fs::symlink_metadata(&path)
            .map_err(|_| "That file is no longer here. Export it again.".to_owned())?;
        if !metadata.is_file() {
            return Err("That file is no longer here. Export it again.".into());
        }
        // One marker per name makes the claim atomic across threads and processes.
        let marker = self.dir.join(format!("{name}{USED_SUFFIX}"));
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&marker)
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    "That file was already sent once. Export it again.".to_owned()
                } else {
                    error.to_string()
                }
            })?;
        // The marker means "sent once"; a file that could not be opened was not sent.
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)
            .inspect_err(|_| {
                let _ = fs::remove_file(&marker);
            })
            .map_err(|error| error.to_string())?;
        let bytes = file.metadata().map_err(|error| error.to_string())?.len();
        if bytes > MAX_BYTES {
            return Err("That file is larger than Silo transfers between machines.".into());
        }
        Ok((file, bytes))
    }

    /// Reserves a write of `bytes` with this `sha256`: refused when the size is zero or past the
    /// cap, or the volume cannot hold it with `FREE_SPACE_MARGIN` to spare after the writes
    /// already reserved. The reservation lasts until the returned `Writer` is renamed or dropped.
    pub fn begin_write(&self, bytes: u64, sha256: &str) -> Result<Writer, String> {
        let available = crate::backup::available_bytes(&self.dir)
            .map_err(|error| format!("Silo could not read the free space here: {error}"))?;
        self.begin_write_with_space(bytes, sha256, available)
    }

    fn begin_write_with_space(
        &self,
        bytes: u64,
        sha256: &str,
        available: u64,
    ) -> Result<Writer, String> {
        if bytes == 0 {
            return Err("The file to send is empty.".into());
        }
        if bytes > MAX_BYTES {
            return Err(format!(
                "That file is larger than Silo transfers between machines ({} GiB).",
                MAX_BYTES / (1024 * 1024 * 1024)
            ));
        }
        if !is_sha256(sha256) {
            return Err("The file's checksum is not valid.".into());
        }
        // Check and reserve under one lock, so two writes cannot both pass on the same space.
        {
            let mut reserved = RESERVED
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let promised = reserved
                .iter()
                .find(|(path, _)| *path == self.dir)
                .map_or(0, |(_, promised)| *promised);
            if available.saturating_sub(promised) < bytes.saturating_add(FREE_SPACE_MARGIN) {
                return Err(
                    "This machine does not have enough free space to receive that file.".into(),
                );
            }
            match reserved.iter_mut().find(|(path, _)| *path == self.dir) {
                Some((_, promised)) => *promised += bytes,
                None => reserved.push((self.dir.clone(), bytes)),
            }
        }
        let name = new_name(Role::Import);
        let part = self.dir.join(format!("{name}{PART_SUFFIX}"));
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&part)
            .inspect_err(|_| release_reserved(&self.dir, bytes))
            .map_err(|error| error.to_string())?;
        Ok(Writer {
            area: self.clone(),
            name,
            part,
            file: Some(file),
            bytes,
            sha256: sha256.to_owned(),
        })
    }

    /// Removes a file of the area, its `.part` and its used marker. Only names the area hands
    /// out are accepted. Returns whether anything was removed.
    pub fn remove(&self, name: &str) -> Result<bool, String> {
        if parse_name(name).is_none() {
            return Err("That is not a file of the transfer area.".into());
        }
        let mut removed = false;
        for suffix in ["", PART_SUFFIX, USED_SUFFIX] {
            match fs::remove_file(self.dir.join(format!("{name}{suffix}"))) {
                Ok(()) => removed = true,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.to_string()),
            }
        }
        Ok(removed)
    }

    /// Removes entries: all of them with `older_than` unset (the engine's start), otherwise those
    /// not changed for that long. Returns how many it removed.
    pub fn sweep(&self, older_than: Option<Duration>) -> usize {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return 0;
        };
        let now = SystemTime::now();
        let mut removed = 0;
        for entry in entries.flatten() {
            if let Some(age) = older_than {
                let modified = entry
                    .metadata()
                    .and_then(|metadata| metadata.modified())
                    .unwrap_or(SystemTime::UNIX_EPOCH);
                if now.duration_since(modified).unwrap_or_default() < age {
                    continue;
                }
            }
            let path = entry.path();
            let result = match fs::symlink_metadata(&path) {
                Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(&path),
                Ok(_) => fs::remove_file(&path),
                Err(error) => Err(error),
            };
            if result.is_ok() {
                removed += 1;
            }
        }
        removed
    }
}

/// A reserved write. Dropping it before `receive` succeeds deletes the `.part`.
#[derive(Debug)]
pub struct Writer {
    area: Area,
    name: String,
    part: PathBuf,
    file: Option<File>,
    bytes: u64,
    sha256: String,
}

impl Writer {
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Reads exactly the declared number of bytes from `reader` into the `.part`, then renames it
    /// when the hash matches. `progress` is told the bytes written so far.
    pub fn receive(
        mut self,
        reader: &mut dyn Read,
        progress: &mut dyn FnMut(u64),
        aborted: &dyn Fn() -> bool,
    ) -> Result<FileInfo, String> {
        let mut file = self.file.take().ok_or("The file was already received.")?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0u8; CHUNK];
        let mut written = 0u64;
        while written < self.bytes {
            if aborted() {
                return Err("The transfer was stopped.".into());
            }
            let want = CHUNK.min((self.bytes - written) as usize);
            let read = reader
                .read(&mut buffer[..want])
                .map_err(|error| format!("The file stopped arriving: {error}"))?;
            if read == 0 {
                return Err("The file stopped arriving before all of it was received.".into());
            }
            file.write_all(&buffer[..read])
                .map_err(|error| format!("Silo could not write the received file: {error}"))?;
            hasher.update(&buffer[..read]);
            written += read as u64;
            progress(written);
        }
        let actual = hex(&hasher.finalize());
        if written != self.bytes || actual != self.sha256 {
            return Err("The received file does not match what was sent.".into());
        }
        file.sync_all().map_err(|error| error.to_string())?;
        drop(file);
        fs::rename(&self.part, self.area.dir.join(&self.name))
            .map_err(|error| format!("Silo could not keep the received file: {error}"))?;
        Ok(FileInfo {
            name: self.name.clone(),
            bytes: written,
            sha256: actual,
        })
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        // A rename moved the `.part`; anything else leaves it to delete.
        let _ = fs::remove_file(&self.part);
        release_reserved(&self.area.dir, self.bytes);
    }
}

// ---------------------------------------------------------------------------------------
// The stream, owner side

/// What a stream asked for, validated. Built before the reply, so a refusal comes back as one.
pub enum Prepared {
    Send {
        name: String,
        file: File,
        bytes: u64,
    },
    Receive(Writer),
}

/// Validates the params of an `exports.file` stream and reserves what it needs:
/// `{"mode":"read","name":"x-…"}` or `{"mode":"write","bytes":N,"sha256":"…"}`.
pub fn prepare(area: &Area, params: &Value) -> Result<Prepared, String> {
    match params["mode"].as_str() {
        Some("read") => {
            let name = params["name"].as_str().ok_or("Missing file name.")?;
            let (file, bytes) = area.open_for_read(name)?;
            Ok(Prepared::Send {
                name: name.to_owned(),
                file,
                bytes,
            })
        }
        Some("write") => {
            let bytes = params["bytes"].as_u64().ok_or("Missing file size.")?;
            let sha256 = params["sha256"].as_str().ok_or("Missing checksum.")?;
            Ok(Prepared::Receive(area.begin_write(bytes, sha256)?))
        }
        _ => Err("Unknown file transfer mode.".into()),
    }
}

fn write_line(stream: &mut dyn Write, value: &Value) -> std::io::Result<()> {
    let mut line = serde_json::to_vec(value).map_err(std::io::Error::other)?;
    line.push(b'\n');
    stream.write_all(&line)?;
    stream.flush()
}

impl Prepared {
    /// Moves the bytes. `progress` is told the bytes moved so far. For a send the owner writes
    /// the header and the file; for a receive it writes the header, reads the file and writes
    /// the verdict.
    pub fn run(
        self,
        stream: &mut (impl Read + Write),
        progress: &mut dyn FnMut(u64),
        aborted: &dyn Fn() -> bool,
    ) -> Result<(), String> {
        match self {
            Self::Send {
                name,
                mut file,
                bytes,
            } => {
                write_line(stream, &json!({"name": name, "bytes": bytes}))
                    .map_err(|error| error.to_string())?;
                let mut buffer = vec![0u8; CHUNK];
                let mut sent = 0u64;
                while sent < bytes {
                    if aborted() {
                        return Err("The transfer was stopped.".into());
                    }
                    let want = CHUNK.min((bytes - sent) as usize);
                    let read = file
                        .read(&mut buffer[..want])
                        .map_err(|error| error.to_string())?;
                    if read == 0 {
                        return Err("The file changed while it was being sent.".into());
                    }
                    stream
                        .write_all(&buffer[..read])
                        .map_err(|error| error.to_string())?;
                    sent += read as u64;
                    progress(sent);
                }
                stream.flush().map_err(|error| error.to_string())
            }
            Self::Receive(writer) => {
                write_line(stream, &json!({"name": writer.name()}))
                    .map_err(|error| error.to_string())?;
                let received = writer.receive(stream, progress, aborted);
                let verdict = match &received {
                    Ok(info) => json!({"ok": true, "bytes": info.bytes, "sha256": info.sha256}),
                    Err(message) => json!({"error": message}),
                };
                let _ = write_line(stream, &verdict);
                received.map(|_| ())
            }
        }
    }
}

// ---------------------------------------------------------------------------------------
// The stream, controller side

fn read_header(output: &mut dyn BufRead) -> Result<Value, String> {
    let mut line = String::new();
    let mut limited = Read::take(&mut *output, HEADER_LIMIT);
    let read = limited
        .read_line(&mut line)
        .map_err(|error| format!("The other machine stopped answering: {error}"))?;
    if read == 0 || !line.ends_with('\n') {
        return Err("The other machine closed the connection before it answered.".into());
    }
    serde_json::from_str(line.trim())
        .map_err(|_| "The other machine sent an invalid answer.".into())
}

/// Receives the export file whose `expected` size and sha256 the owner reported into `file`.
/// Fails unless exactly that many bytes arrive and their hash matches.
pub fn pull(
    output: &mut dyn BufRead,
    file: &mut File,
    expected: &FileInfo,
    progress: &mut dyn FnMut(u64),
    cancelled: &dyn Fn() -> bool,
) -> Result<(), String> {
    let header = read_header(output)?;
    if let Some(error) = header["error"].as_str() {
        return Err(error.to_owned());
    }
    if header["bytes"].as_u64() != Some(expected.bytes) {
        return Err("The file on the other machine is not the one that was exported.".into());
    }
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; CHUNK];
    let mut received = 0u64;
    while received < expected.bytes {
        if cancelled() {
            return Err("The transfer was cancelled.".into());
        }
        let want = CHUNK.min((expected.bytes - received) as usize);
        let read = output
            .read(&mut buffer[..want])
            .map_err(|error| format!("The file stopped arriving: {error}"))?;
        if read == 0 {
            return Err("The file stopped arriving before all of it was received.".into());
        }
        file.write_all(&buffer[..read])
            .map_err(|error| format!("Silo could not write the file: {error}"))?;
        hasher.update(&buffer[..read]);
        received += read as u64;
        progress(received);
    }
    if hex(&hasher.finalize()) != expected.sha256 {
        return Err("The file that arrived does not match what the other machine exported.".into());
    }
    file.sync_all().map_err(|error| error.to_string())
}

/// Sends `bytes` of `source` (whose sha256 is declared in the open params) and returns the name
/// the owner gave the file, once the owner says it has kept it.
pub fn push(
    input: &mut dyn Write,
    output: &mut dyn BufRead,
    source: &mut dyn Read,
    bytes: u64,
    progress: &mut dyn FnMut(u64),
    cancelled: &dyn Fn() -> bool,
) -> Result<String, String> {
    let header = read_header(output)?;
    if let Some(error) = header["error"].as_str() {
        return Err(error.to_owned());
    }
    let name = header["name"]
        .as_str()
        .filter(|name| parse_name(name) == Some(Role::Import))
        .ok_or("The other machine sent an invalid name.")?
        .to_owned();
    let mut buffer = vec![0u8; CHUNK];
    let mut sent = 0u64;
    while sent < bytes {
        if cancelled() {
            return Err("The transfer was cancelled.".into());
        }
        let want = CHUNK.min((bytes - sent) as usize);
        let read = source
            .read(&mut buffer[..want])
            .map_err(|error| error.to_string())?;
        if read == 0 {
            return Err("The file changed while it was being sent.".into());
        }
        if let Err(error) = input.write_all(&buffer[..read]) {
            // The owner ends a refused write after sending its reason; show that, not the pipe.
            return Err(match read_header(output) {
                Ok(verdict) => verdict["error"].as_str().map_or_else(
                    || format!("The other machine stopped receiving: {error}"),
                    str::to_owned,
                ),
                Err(_) => format!("The other machine stopped receiving: {error}"),
            });
        }
        sent += read as u64;
        progress(sent);
    }
    input.flush().map_err(|error| error.to_string())?;
    let verdict = read_header(output)?;
    if let Some(error) = verdict["error"].as_str() {
        return Err(error.to_owned());
    }
    if verdict["ok"] != json!(true) || verdict["bytes"].as_u64() != Some(bytes) {
        return Err("The other machine did not confirm the file.".into());
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::os::unix::net::UnixStream;

    fn area() -> (tempfile::TempDir, Area) {
        let dir = tempfile::tempdir().unwrap();
        let area = Area::at(dir.path()).unwrap();
        (dir, area)
    }

    fn sha(data: &[u8]) -> String {
        hex(&Sha256::digest(data))
    }

    fn token() -> String {
        "0123456789abcdef0123456789abcdef".into()
    }

    #[test]
    fn publishing_never_replaces_and_falls_back_when_a_volume_lacks_a_primitive() {
        let dir = tempfile::tempdir().unwrap();
        let unsupported =
            |_: &Path, _: &Path| Err(std::io::Error::from_raw_os_error(libc::ENOTSUP));
        let einval = |_: &Path, _: &Path| Err(std::io::Error::from_raw_os_error(libc::EINVAL));
        let eperm = |_: &Path, _: &Path| Err(std::io::Error::from_raw_os_error(libc::EPERM));
        let make = |name: &str| {
            let path = dir.path().join(name);
            fs::write(&path, name).unwrap();
            path
        };
        // The real primitives.
        let (source, destination) = (make("a.part"), dir.path().join("a"));
        publish_without_replacing(&source, &destination).unwrap();
        assert!(!source.exists());
        assert_eq!(fs::read(&destination).unwrap(), b"a.part");
        let source = make("a2.part");
        let error = publish_without_replacing(&source, &destination).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&destination).unwrap(), b"a.part");

        // No exclusive rename: a hard link, and the source name goes away.
        let (source, destination) = (make("b.part"), dir.path().join("b"));
        publish_without_replacing_with(&source, &destination, einval, |s, d| fs::hard_link(s, d))
            .unwrap();
        assert!(!source.exists());
        assert_eq!(fs::read(&destination).unwrap(), b"b.part");
        let source = make("b2.part");
        let error = publish_without_replacing_with(&source, &destination, unsupported, |s, d| {
            fs::hard_link(s, d)
        })
        .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);

        // Neither: an existence check and a plain rename.
        let (source, destination) = (make("c.part"), dir.path().join("c"));
        publish_without_replacing_with(&source, &destination, unsupported, eperm).unwrap();
        assert!(!source.exists());
        assert_eq!(fs::read(&destination).unwrap(), b"c.part");
        let source = make("c2.part");
        let error = publish_without_replacing_with(&source, &destination, unsupported, unsupported)
            .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&destination).unwrap(), b"c.part");
        assert!(source.exists());

        // An error that is not "unsupported" is reported, not worked around.
        let denied = |_: &Path, _: &Path| Err(std::io::Error::from_raw_os_error(libc::EACCES));
        let source = make("d.part");
        let error =
            publish_without_replacing_with(&source, &dir.path().join("d"), denied, |s, d| {
                fs::hard_link(s, d)
            })
            .unwrap_err();
        assert_eq!(error.raw_os_error(), Some(libc::EACCES));
    }

    #[test]
    fn the_area_is_private() {
        let (_dir, area) = area();
        let mode = fs::metadata(area.dir()).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }

    #[test]
    fn a_name_is_a_fixed_pattern_and_carries_its_role() {
        let export = format!("x-{}.silo-backup", token());
        let import = format!("i-{}.silo-backup", token());
        assert_eq!(parse_name(&export), Some(Role::Export));
        assert_eq!(parse_name(&import), Some(Role::Import));
        for bad in [
            "",
            "x-.silo-backup",
            "../x-0123456789abcdef0123456789abcdef.silo-backup",
            "x-0123456789abcdef0123456789abcdef.silo-backup/../etc",
            "x-0123456789ABCDEF0123456789abcdef.silo-backup",
            "x-0123456789abcdef0123456789abcde.silo-backup",
            "x-0123456789abcdef0123456789abcdef.tar",
            "y-0123456789abcdef0123456789abcdef.silo-backup",
            "x-0123456789abcdef0123456789abcdef.silo-backup.part",
            "/etc/passwd",
        ] {
            assert_eq!(parse_name(bad), None, "{bad}");
        }
        assert_eq!(parse_name(&new_name(Role::Export)), Some(Role::Export));
        assert_eq!(parse_name(&new_name(Role::Import)), Some(Role::Import));
    }

    #[test]
    fn an_export_file_is_read_once_and_never_through_an_import_name() {
        let (_dir, area) = area();
        let (name, path) = area.export_target();
        fs::write(&path, b"archive").unwrap();
        let (mut file, bytes) = area.open_for_read(&name).unwrap();
        let mut text = String::new();
        file.read_to_string(&mut text).unwrap();
        assert_eq!((text.as_str(), bytes), ("archive", 7));
        assert!(area
            .open_for_read(&name)
            .unwrap_err()
            .contains("already sent"));
        // The file stays until it is removed.
        assert!(path.exists());
        let import = format!("i-{}.silo-backup", token());
        fs::write(area.dir().join(&import), b"x").unwrap();
        assert!(area
            .open_for_read(&import)
            .unwrap_err()
            .contains("not a file this machine can send"));
        assert!(area
            .open_for_read("../escape")
            .unwrap_err()
            .contains("not a file"));
    }

    #[test]
    fn a_name_that_is_a_link_is_not_read() {
        let (dir, area) = area();
        let secret = dir.path().join("secret");
        fs::write(&secret, b"secret").unwrap();
        let name = format!("x-{}.silo-backup", token());
        std::os::unix::fs::symlink(&secret, area.dir().join(&name)).unwrap();
        assert!(area.open_for_read(&name).is_err());
    }

    #[test]
    fn a_missing_export_file_is_refused_without_a_marker() {
        let (_dir, area) = area();
        let name = format!("x-{}.silo-backup", token());
        assert!(area.open_for_read(&name).unwrap_err().contains("no longer"));
        assert!(!area.dir().join(format!("{name}.used")).exists());
    }

    #[test]
    fn a_write_keeps_the_file_only_when_size_and_hash_match() {
        let (_dir, area) = area();
        let data = b"hello archive".to_vec();
        let writer = area.begin_write(data.len() as u64, &sha(&data)).unwrap();
        let name = writer.name().to_owned();
        assert!(area.dir().join(format!("{name}.part")).exists());
        let info = writer
            .receive(&mut Cursor::new(data.clone()), &mut |_| {}, &|| false)
            .unwrap();
        assert_eq!(info.bytes, data.len() as u64);
        assert!(!area.dir().join(format!("{name}.part")).exists());
        assert_eq!(fs::read(area.import_path(&name).unwrap()).unwrap(), data);
        let mode = fs::metadata(area.dir().join(&name))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn a_hash_mismatch_deletes_the_part_and_keeps_nothing() {
        let (_dir, area) = area();
        let data = b"hello archive".to_vec();
        let writer = area
            .begin_write(data.len() as u64, &sha(b"something else"))
            .unwrap();
        let name = writer.name().to_owned();
        let error = writer
            .receive(&mut Cursor::new(data), &mut |_| {}, &|| false)
            .unwrap_err();
        assert!(error.contains("does not match"));
        assert_eq!(fs::read_dir(area.dir()).unwrap().count(), 0, "{name}");
    }

    #[test]
    fn a_short_stream_deletes_the_part_and_a_long_one_is_read_no_further() {
        let (_dir, area) = area();
        let data = b"0123456789".to_vec();
        let writer = area.begin_write(10, &sha(&data)).unwrap();
        let error = writer
            .receive(&mut Cursor::new(data[..4].to_vec()), &mut |_| {}, &|| false)
            .unwrap_err();
        assert!(error.contains("before all of it"));
        assert_eq!(fs::read_dir(area.dir()).unwrap().count(), 0);

        let writer = area.begin_write(10, &sha(&data)).unwrap();
        let mut longer = Cursor::new([data.clone(), b"extra".to_vec()].concat());
        writer.receive(&mut longer, &mut |_| {}, &|| false).unwrap();
        assert_eq!(longer.position(), 10);
    }

    #[test]
    fn an_abort_or_a_dropped_writer_deletes_the_part() {
        let (_dir, area) = area();
        let data = vec![7u8; 200_000];
        let writer = area.begin_write(data.len() as u64, &sha(&data)).unwrap();
        let error = writer
            .receive(&mut Cursor::new(data.clone()), &mut |_| {}, &|| true)
            .unwrap_err();
        assert!(error.contains("stopped"));
        assert_eq!(fs::read_dir(area.dir()).unwrap().count(), 0);
        drop(area.begin_write(5, &sha(b"12345")).unwrap());
        assert_eq!(fs::read_dir(area.dir()).unwrap().count(), 0);
    }

    #[test]
    fn a_write_needs_the_declared_size_the_cap_and_free_space_before_the_first_byte() {
        let (_dir, area) = area();
        let good = sha(b"x");
        assert!(area.begin_write(0, &good).is_err());
        assert!(area
            .begin_write(MAX_BYTES + 1, &good)
            .unwrap_err()
            .contains("larger"));
        assert!(area.begin_write(1, "not a hash").is_err());
        assert!(area.begin_write(1, &good.to_uppercase()).is_err());
        let size = 1_000_000;
        assert!(area
            .begin_write_with_space(size, &good, size + FREE_SPACE_MARGIN - 1)
            .unwrap_err()
            .contains("free space"));
        assert_eq!(fs::read_dir(area.dir()).unwrap().count(), 0);
        assert!(area
            .begin_write_with_space(size, &good, size + FREE_SPACE_MARGIN)
            .is_ok());
    }

    #[test]
    fn writes_in_flight_count_against_free_space_until_they_end() {
        let (_dir, area) = area();
        let good = sha(b"x");
        let size = 1_000_000;
        let available = 2 * size + FREE_SPACE_MARGIN;
        let first = area.begin_write_with_space(size, &good, available).unwrap();
        // The second would fit alone but not beside the first.
        assert!(area
            .begin_write_with_space(size + 1, &good, available)
            .unwrap_err()
            .contains("free space"));
        let second = area.begin_write_with_space(size, &good, available).unwrap();
        assert!(area
            .begin_write_with_space(1, &good, available)
            .unwrap_err()
            .contains("free space"));
        // A dropped write gives its bytes back.
        drop(first);
        let third = area.begin_write_with_space(size, &good, available).unwrap();
        // A finished write does too (its file is then on the volume, so `available` falls).
        drop((second, third));
        let data = b"hello".to_vec();
        area.begin_write_with_space(5, &sha(&data), 5 + FREE_SPACE_MARGIN)
            .unwrap()
            .receive(&mut Cursor::new(data), &mut |_| {}, &|| false)
            .unwrap();
        assert!(area
            .begin_write_with_space(size, &good, size + FREE_SPACE_MARGIN)
            .is_ok());
    }

    #[test]
    fn a_failed_open_of_an_export_does_not_leave_the_sent_marker() {
        let (_dir, area) = area();
        let (name, path) = area.export_target();
        fs::write(&path, b"x").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
        // The owner of the file can still open it as root; the check needs a refused open.
        if File::open(&path).is_err() {
            assert!(area.open_for_read(&name).is_err());
            assert!(!area.dir().join(format!("{name}.used")).exists());
        }
    }

    #[test]
    fn an_import_file_is_only_importable_by_its_import_name() {
        let (_dir, area) = area();
        let (export, path) = area.export_target();
        fs::write(path, b"x").unwrap();
        assert!(area.import_path(&export).is_err());
        assert!(area.import_path("../x").is_err());
        let missing = format!("i-{}.silo-backup", token());
        assert!(area.import_path(&missing).unwrap_err().contains("Send it"));
    }

    #[test]
    fn remove_takes_the_file_its_part_and_its_marker_and_only_for_valid_names() {
        let (_dir, area) = area();
        let (name, path) = area.export_target();
        fs::write(&path, b"x").unwrap();
        area.open_for_read(&name).unwrap();
        fs::write(area.dir().join(format!("{name}.part")), b"x").unwrap();
        assert!(area.remove(&name).unwrap());
        assert_eq!(fs::read_dir(area.dir()).unwrap().count(), 0);
        assert!(!area.remove(&name).unwrap());
        assert!(area.remove("../elsewhere").is_err());
    }

    #[test]
    fn the_sweep_clears_everything_at_start_and_only_old_entries_later() {
        let (_dir, area) = area();
        let old = area.dir().join(format!("i-{}.silo-backup", token()));
        let fresh = area.dir().join(format!("x-{}.silo-backup", token()));
        fs::write(&old, b"x").unwrap();
        fs::write(&fresh, b"x").unwrap();
        let day_ago = SystemTime::now() - Duration::from_secs(25 * 3600);
        File::options()
            .write(true)
            .open(&old)
            .unwrap()
            .set_modified(day_ago)
            .unwrap();
        assert_eq!(area.sweep(Some(STALE_AFTER)), 1);
        assert!(!old.exists() && fresh.exists());
        assert_eq!(area.sweep(None), 1);
        assert_eq!(fs::read_dir(area.dir()).unwrap().count(), 0);
    }

    #[test]
    fn a_path_in_the_area_is_found_even_when_it_does_not_exist_yet_or_goes_through_a_link() {
        let (dir, area) = area();
        assert!(area.contains(area.dir()));
        assert!(area.contains(&area.dir().join("new.silo-backup")));
        assert!(area.contains(&area.dir().join("a/b/c.silo-backup")));
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(area.dir(), &link).unwrap();
        assert!(area.contains(&link.join("x.silo-backup")));
        assert!(!area.contains(&dir.path().join("elsewhere.silo-backup")));
        assert!(!area.contains(Path::new("/nonexistent/place")));
    }

    #[test]
    fn an_export_path_is_recognised_only_inside_the_area() {
        let (dir, area) = area();
        let (_, inside) = area.export_target();
        assert!(area.owns_export_path(&inside));
        let outside = dir.path().join(inside.file_name().unwrap());
        assert!(!area.owns_export_path(&outside));
        assert!(!area.owns_export_path(&area.dir().join("notes.silo-backup")));
    }

    /// The owner's half on a thread, the controller's half here, over a pair of sockets.
    fn owner(
        area: Area,
        params: Value,
        stream: UnixStream,
    ) -> std::thread::JoinHandle<Result<(), String>> {
        std::thread::spawn(move || {
            let prepared = prepare(&area, &params)?;
            let mut stream = stream;
            prepared.run(&mut stream, &mut |_| {}, &|| false)
        })
    }

    #[test]
    fn a_file_pulled_over_a_stream_arrives_whole_and_cannot_be_pulled_twice() {
        let (_dir, area) = area();
        let data: Vec<u8> = (0..300_000u32).map(|n| (n % 251) as u8).collect();
        let (name, path) = area.export_target();
        fs::write(&path, &data).unwrap();
        let expected = FileInfo {
            name: name.clone(),
            bytes: data.len() as u64,
            sha256: sha(&data),
        };
        let (near, far) = UnixStream::pair().unwrap();
        let handle = owner(area.clone(), json!({"mode": "read", "name": name}), far);
        let mut destination = tempfile::tempfile().unwrap();
        let mut reader = std::io::BufReader::new(near);
        let mut last = 0;
        pull(
            &mut reader,
            &mut destination,
            &expected,
            &mut |done| last = done,
            &|| false,
        )
        .unwrap();
        handle.join().unwrap().unwrap();
        assert_eq!(last, data.len() as u64);
        let mut got = Vec::new();
        std::io::Seek::rewind(&mut destination).unwrap();
        destination.read_to_end(&mut got).unwrap();
        assert_eq!(got, data);
        assert!(prepare(&area, &json!({"mode": "read", "name": name})).is_err());
    }

    #[test]
    fn a_pull_whose_hash_or_size_differs_from_the_report_fails() {
        let (_dir, area) = area();
        let data = b"the archive".to_vec();
        let (name, path) = area.export_target();
        fs::write(&path, &data).unwrap();
        let wrong_hash = FileInfo {
            name: name.clone(),
            bytes: data.len() as u64,
            sha256: sha(b"other"),
        };
        let (near, far) = UnixStream::pair().unwrap();
        let handle = owner(area.clone(), json!({"mode": "read", "name": name}), far);
        let mut reader = std::io::BufReader::new(near);
        let error = pull(
            &mut reader,
            &mut tempfile::tempfile().unwrap(),
            &wrong_hash,
            &mut |_| {},
            &|| false,
        )
        .unwrap_err();
        assert!(error.contains("does not match"));
        let _ = handle.join();

        let (name, path) = area.export_target();
        fs::write(&path, &data).unwrap();
        let wrong_size = FileInfo {
            name: name.clone(),
            bytes: 3,
            sha256: sha(&data),
        };
        let (near, far) = UnixStream::pair().unwrap();
        let handle = owner(area, json!({"mode": "read", "name": name}), far);
        let mut reader = std::io::BufReader::new(near);
        let error = pull(
            &mut reader,
            &mut tempfile::tempfile().unwrap(),
            &wrong_size,
            &mut |_| {},
            &|| false,
        )
        .unwrap_err();
        assert!(error.contains("not the one"));
        let _ = handle.join();
    }

    #[test]
    fn a_file_pushed_over_a_stream_is_kept_under_a_name_the_owner_chose() {
        let (_dir, area) = area();
        let data: Vec<u8> = (0..300_000u32).map(|n| (n % 241) as u8).collect();
        let (near, far) = UnixStream::pair().unwrap();
        let handle = owner(
            area.clone(),
            json!({"mode": "write", "bytes": data.len(), "sha256": sha(&data)}),
            far,
        );
        let mut input = near.try_clone().unwrap();
        let mut reader = std::io::BufReader::new(near);
        let name = push(
            &mut input,
            &mut reader,
            &mut Cursor::new(data.clone()),
            data.len() as u64,
            &mut |_| {},
            &|| false,
        )
        .unwrap();
        handle.join().unwrap().unwrap();
        assert_eq!(parse_name(&name), Some(Role::Import));
        assert_eq!(fs::read(area.import_path(&name).unwrap()).unwrap(), data);
    }

    #[test]
    fn a_push_whose_hash_is_wrong_is_refused_by_the_owner_and_leaves_nothing() {
        let (_dir, area) = area();
        let data = b"payload bytes".to_vec();
        let (near, far) = UnixStream::pair().unwrap();
        let handle = owner(
            area.clone(),
            json!({"mode": "write", "bytes": data.len(), "sha256": sha(b"other")}),
            far,
        );
        let mut input = near.try_clone().unwrap();
        let mut reader = std::io::BufReader::new(near);
        let error = push(
            &mut input,
            &mut reader,
            &mut Cursor::new(data.clone()),
            data.len() as u64,
            &mut |_| {},
            &|| false,
        )
        .unwrap_err();
        assert!(error.contains("does not match"));
        assert!(handle.join().unwrap().is_err());
        assert_eq!(fs::read_dir(area.dir()).unwrap().count(), 0);
    }

    #[test]
    fn a_push_cut_off_by_the_owner_shows_the_owners_reason() {
        let (near, far) = UnixStream::pair().unwrap();
        let owner = std::thread::spawn(move || {
            let mut far = far;
            write_line(&mut far, &json!({"name": new_name(Role::Import)})).unwrap();
            write_line(&mut far, &json!({"error": "This machine is full."})).unwrap();
            // Closing is what makes the controller's next write fail.
        });
        owner.join().unwrap();
        let mut input = near.try_clone().unwrap();
        let mut reader = std::io::BufReader::new(near);
        let data = vec![1u8; 4 * CHUNK];
        let error = push(
            &mut input,
            &mut reader,
            &mut Cursor::new(data.clone()),
            data.len() as u64,
            &mut |_| {},
            &|| false,
        )
        .unwrap_err();
        assert_eq!(error, "This machine is full.");
    }

    #[test]
    fn a_refusal_before_the_first_byte_comes_back_as_the_reason() {
        let (_dir, area) = area();
        assert!(prepare(&area, &json!({"mode": "read", "name": "../etc/passwd"})).is_err());
        assert!(prepare(
            &area,
            &json!({"mode": "write", "bytes": 0, "sha256": sha(b"x")})
        )
        .is_err());
        assert!(prepare(&area, &json!({"mode": "delete"})).is_err());
        assert!(prepare(&area, &json!({})).is_err());
    }
}
