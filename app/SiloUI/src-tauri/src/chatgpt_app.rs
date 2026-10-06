//! The pinned ChatGPT Linux app, kept once per device for LCU.
//!
//! Silo never publishes OpenAI files. Every device that runs Silo downloads, by
//! itself and in the background (see `auto`), the exact `.deb` pinned in `guest/chatgpt-app-lock.json` from OpenAI, verifies its
//! size and SHA-256, extracts only `usr/lib/chatgpt` (never running maintainer
//! scripts) into one immutable folder per version, and publishes it atomically.
//! Computers later mount that folder read-only. See `docs/SiloUI-CHATGPT-APP.md`.
//!
//! Layout under the channel's application data directory:
//!
//! ```text
//! chatgpt/.lock                      cross-process lock (flock)
//! chatgpt/downloads/*.deb[.part]     resumable download, deleted after success
//! chatgpt/.staging-*/                extraction in progress, never mounted
//! chatgpt/published/                 mounted read-only into computers; only verified trees
//! chatgpt/published/<version>-<debarch>/
//!                                    published, immutable app tree
//! chatgpt/<version>-<debarch>.published.json
//!                                    publication record, written last
//! ```
//!
//! Records, staging, downloads stay outside `published/`: that folder
//! is what every computer mounts, so it holds nothing but verified trees (which also keeps
//! MicroSandbox's first walk of the mount small).
//!
//! A folder is only "ready" when its publication record matches the lock and
//! the tree (see `verify_published`). The storage root and every directory
//! Silo writes through are opened without following symlinks and are checked to
//! be real directories owned by the current user; files are created exclusively
//! and relative to those directory handles.
//!

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    ffi::CString,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
        io::{AsRawFd, FromRawFd, OwnedFd},
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};

const LOCK_JSON: &str = include_str!("../guest/chatgpt-app-lock.json");
const DOWNLOAD_HOST: &str = "persistent.oaistatic.com";
/// The directory dpkg would fill, relative to the archive root.
const TREE_PREFIX: [&str; 3] = ["usr", "lib", "chatgpt"];
/// Executables the app and the LCU runtime need (relative to the tree). A tree
/// where any of them is missing, not a regular file or not executable is damaged.
const REQUIRED_EXECUTABLES: [&str; 3] = [
    "ChatGPT",
    "resources/cua_node/bin/node",
    "resources/cua_node/bin/node_repl",
];
const MAX_ENTRIES: usize = 200_000;
const MAX_UNPACKED_BYTES: u64 = 8 * 1024 * 1024 * 1024;
/// The most a download of unknown size may write.
const UNKNOWN_SIZE_LIMIT: u64 = 1024 * 1024 * 1024;
/// Longest path or link target (bytes), and longest single component.
const MAX_NAME_BYTES: usize = 4096;
const MAX_COMPONENT_BYTES: usize = 255;
/// Total size of the PAX extended header records of one entry.
const MAX_PAX_BYTES: usize = 64 * 1024;
const RECORD_SUFFIX: &str = ".published.json";
/// The folder computers mount, directly under the storage root.
const PUBLISHED_DIR: &str = "published";
const RECORD_SCHEMA: u32 = 1;
const MAX_RECORD_BYTES: u64 = 4096;
const STATUS_EVENT: &str = "chatgpt-app-status";

// ----------------------------------------------------------------- lock

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Lock {
    schema_version: u32,
    pub(crate) package: String,
    pub(crate) version: String,
    pub(crate) cua_runtime_version: String,
    /// The LCU release tested with this app (must equal `guest/lcu-lock.json`).
    pub(crate) lcu_version: Option<String>,
    architectures: HashMap<String, Asset>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Asset {
    pub(crate) url: String,
    pub(crate) sha256: String,
    pub(crate) bytes: u64,
}

/// Debian architecture names, which equal the guest architecture on this device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DebArch {
    Arm64,
    Amd64,
}

impl DebArch {
    pub(crate) fn host() -> Result<Self, Error> {
        match std::env::consts::ARCH {
            "aarch64" => Ok(Self::Arm64),
            "x86_64" => Ok(Self::Amd64),
            _ => Err(Error::fatal(
                "The ChatGPT app is only available for 64-bit Intel and Arm devices.",
            )),
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Arm64 => "arm64",
            Self::Amd64 => "amd64",
        }
    }
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn valid_version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.starts_with(|c: char| c.is_ascii_digit())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'+' | b'~' | b'-'))
}

impl Lock {
    /// The lock compiled into this build, validated.
    pub(crate) fn bundled() -> Result<Self, Error> {
        let lock: Self = serde_json::from_str(LOCK_JSON)
            .map_err(|_| Error::fatal("Silo's ChatGPT app information is invalid."))?;
        lock.validate()?;
        Ok(lock)
    }

    pub(crate) fn validate(&self) -> Result<(), Error> {
        let invalid = || Error::fatal("Silo's ChatGPT app information is invalid.");
        if self.schema_version != 1 || self.package != "chatgpt" || !valid_version(&self.version) {
            return Err(invalid());
        }
        for arch in ["arm64", "amd64"] {
            let asset = self.architectures.get(arch).ok_or_else(invalid)?;
            let url = reqwest::Url::parse(&asset.url).map_err(|_| invalid())?;
            if url.scheme() != "https"
                || url.host_str() != Some(DOWNLOAD_HOST)
                || !valid_sha256(&asset.sha256)
                || asset.bytes == 0
                || asset.bytes > 4 * 1024 * 1024 * 1024
            {
                return Err(invalid());
            }
        }
        Ok(())
    }

    pub(crate) fn asset(&self, arch: DebArch) -> Result<&Asset, Error> {
        self.architectures
            .get(arch.name())
            .ok_or_else(|| Error::fatal("Silo's ChatGPT app information is invalid."))
    }

    /// `<version>-<debarch>`: the published folder name.
    pub(crate) fn directory_name(&self, arch: DebArch) -> String {
        format!("{}-{}", self.version, arch.name())
    }
}

// --------------------------------------------------------------- status

/// What the UI shows. `Ready` carries the canonical folder to mount.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub(crate) enum Status {
    /// Nothing present and nothing running: a download is waiting to start.
    Idle,
    #[serde(rename_all = "camelCase")]
    Downloading {
        received_bytes: u64,
        total_bytes: u64,
    },
    Verifying,
    Extracting,
    #[serde(rename_all = "camelCase")]
    Ready {
        path: PathBuf,
        version: String,
    },
    #[serde(rename_all = "camelCase")]
    Failed {
        reason: String,
        retryable: bool,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Error {
    pub(crate) message: String,
    pub(crate) retryable: bool,
}

impl Error {
    fn retry(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            retryable: true,
        }
    }
    fn fatal(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            retryable: false,
        }
    }
    fn status(&self) -> Status {
        Status::Failed {
            reason: self.message.clone(),
            retryable: self.retryable,
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

// ------------------------------------------------- directory-relative I/O

static UNIQUE: AtomicU64 = AtomicU64::new(0);

/// A name no other process or call uses: `<pid>-<nanos>-<counter>`.
fn unique_suffix() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!(
        "{}-{nanos:x}-{}",
        std::process::id(),
        UNIQUE.fetch_add(1, Ordering::Relaxed)
    )
}

fn c_name(name: &str) -> std::io::Result<CString> {
    CString::new(name)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "NUL in name"))
}

fn c_name_path(path: &Path) -> std::io::Result<CString> {
    CString::new(path.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "NUL in path"))
}

fn check(result: libc::c_int) -> std::io::Result<libc::c_int> {
    if result < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(result)
    }
}

fn effective_uid() -> u32 {
    unsafe { libc::geteuid() }
}

fn denied(why: &'static str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::PermissionDenied, why)
}

/// A directory opened without following symlinks. Every operation takes a single
/// name relative to it, so no path component can be swapped for a symlink
/// between a check and the use.
struct Dir(OwnedFd);

impl Dir {
    /// Opens `path` as a real directory owned by this user. A symlink, a file or
    /// another owner's directory is refused. `create` makes a missing directory
    /// (and its parents) and tightens the final one to 0700.
    fn open_root(path: &Path, create: bool) -> std::io::Result<Self> {
        let raw = c_name_path(path)?;
        let open = || unsafe {
            libc::open(
                raw.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        let mut fd = open();
        if fd < 0
            && create
            && std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound
        {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            // mkdir (not create_dir_all) so a symlink planted meanwhile is not followed.
            if unsafe { libc::mkdir(raw.as_ptr(), 0o700) } != 0
                && std::io::Error::last_os_error().kind() != std::io::ErrorKind::AlreadyExists
            {
                return Err(std::io::Error::last_os_error());
            }
            fd = open();
        }
        let dir = Self(unsafe { OwnedFd::from_raw_fd(check(fd)?) });
        let meta = dir.stat_self()?;
        if meta.st_uid != effective_uid() {
            return Err(denied("directory is owned by another user"));
        }
        if create {
            dir.chmod(0o700)?;
        } else if meta.st_mode & 0o022 != 0 {
            return Err(denied("directory is writable by others"));
        }
        Ok(dir)
    }

    fn fd(&self) -> libc::c_int {
        self.0.as_raw_fd()
    }

    fn stat_self(&self) -> std::io::Result<libc::stat> {
        let mut stat = unsafe { std::mem::zeroed::<libc::stat>() };
        check(unsafe { libc::fstat(self.fd(), &mut stat) })?;
        Ok(stat)
    }

    /// `lstat` of `name`; with `follow`, `name` may be a relative path and
    /// symlinks are resolved.
    fn stat(&self, name: &str, follow: bool) -> std::io::Result<libc::stat> {
        let name = c_name(name)?;
        let mut stat = unsafe { std::mem::zeroed::<libc::stat>() };
        let flags = if follow { 0 } else { libc::AT_SYMLINK_NOFOLLOW };
        check(unsafe { libc::fstatat(self.fd(), name.as_ptr(), &mut stat, flags) })?;
        Ok(stat)
    }

    /// Creates directory `name` (mode 0700) when `create` and missing, then opens
    /// it. Returns whether it was created. A symlink or non-directory fails.
    fn subdir(&self, name: &str, create: bool) -> std::io::Result<(Self, bool)> {
        let raw = c_name(name)?;
        let mut created = false;
        if create {
            if unsafe { libc::mkdirat(self.fd(), raw.as_ptr(), 0o700) } == 0 {
                created = true;
            } else if std::io::Error::last_os_error().kind() != std::io::ErrorKind::AlreadyExists {
                return Err(std::io::Error::last_os_error());
            }
        }
        let fd = check(unsafe {
            libc::openat(
                self.fd(),
                raw.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        })?;
        let dir = Self(unsafe { OwnedFd::from_raw_fd(fd) });
        if dir.stat_self()?.st_uid != effective_uid() {
            return Err(denied("directory is owned by another user"));
        }
        Ok((dir, created))
    }

    fn open_file(&self, name: &str, flags: libc::c_int, mode: u32) -> std::io::Result<File> {
        let raw = c_name(name)?;
        let fd = check(unsafe {
            libc::openat(
                self.fd(),
                raw.as_ptr(),
                flags | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                mode as libc::c_uint,
            )
        })?;
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    /// A new file that must not exist yet (never follows a planted link).
    fn create_file(&self, name: &str, mode: u32) -> std::io::Result<File> {
        self.open_file(name, libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL, mode)
    }

    fn symlink(&self, target: &str, name: &str) -> std::io::Result<()> {
        let (target, name) = (c_name(target)?, c_name(name)?);
        check(unsafe { libc::symlinkat(target.as_ptr(), self.fd(), name.as_ptr()) }).map(|_| ())
    }

    fn rename(&self, from: &str, to: &str) -> std::io::Result<()> {
        let (from, to) = (c_name(from)?, c_name(to)?);
        check(unsafe { libc::renameat(self.fd(), from.as_ptr(), self.fd(), to.as_ptr()) })
            .map(|_| ())
    }

    /// Renames `from` in this directory to `to` inside `other` (same filesystem).
    fn rename_into(&self, from: &str, other: &Dir, to: &str) -> std::io::Result<()> {
        let (from, to) = (c_name(from)?, c_name(to)?);
        check(unsafe { libc::renameat(self.fd(), from.as_ptr(), other.fd(), to.as_ptr()) })
            .map(|_| ())
    }

    fn unlink(&self, name: &str) -> std::io::Result<()> {
        let name = c_name(name)?;
        check(unsafe { libc::unlinkat(self.fd(), name.as_ptr(), 0) }).map(|_| ())
    }

    fn chmod(&self, mode: u32) -> std::io::Result<()> {
        check(unsafe { libc::fchmod(self.fd(), mode as libc::mode_t) }).map(|_| ())
    }

    /// Flushes this directory's entries to stable storage; errors propagate.
    fn sync(&self) -> std::io::Result<()> {
        // Through `File` so macOS uses F_FULLFSYNC like every other sync here.
        let fd = check(unsafe { libc::dup(self.fd()) })?;
        unsafe { File::from_raw_fd(fd) }.sync_all()
    }

    /// Removes `name` (inside the directory at `path`, which this handle
    /// belongs to) whatever it is, without following it. A directory is renamed
    /// aside first so a half-deleted tree never carries its old name.
    fn remove_entry(&self, path: &Path, name: &str) -> std::io::Result<()> {
        let stat = match self.stat(name, false) {
            Ok(stat) => stat,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        if stat.st_mode & libc::S_IFMT == libc::S_IFDIR {
            let aside = format!(".rejected-{}", unique_suffix());
            self.rename(name, &aside)?;
            let aside = path.join(aside);
            let _ = make_tree_deletable(&aside);
            // std's remove_dir_all never follows symlinks.
            fs::remove_dir_all(aside)
        } else {
            self.unlink(name)
        }
    }
}

/// Restores owner access on directories so a tampered tree (for example modes
/// changed to 0500) can still be deleted.
pub(crate) fn make_tree_deletable(path: &Path) -> std::io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_dir() || meta.uid() != effective_uid() {
        return Ok(());
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    for entry in fs::read_dir(path)?.flatten() {
        let _ = make_tree_deletable(&entry.path());
    }
    Ok(())
}

// ---------------------------------------------------------- small files

/// Reads a small regular file below `dir` (never through a symlink).
fn read_small(dir: &Dir, name: &str) -> Option<Vec<u8>> {
    let file = dir
        .open_file(name, libc::O_RDONLY | libc::O_NONBLOCK, 0)
        .ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() || meta.uid() != effective_uid() || meta.len() > MAX_RECORD_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MAX_RECORD_BYTES).read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

/// Writes `bytes` to a fresh exclusive temporary, syncs it, renames it over
/// `name` and syncs the directory.
fn write_file_atomically(dir: &Dir, name: &str, bytes: &[u8]) -> std::io::Result<()> {
    let temporary = format!(".{name}-{}.tmp", unique_suffix());
    let write = || -> std::io::Result<()> {
        let mut file = dir.create_file(&temporary, 0o600)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        dir.rename(&temporary, name)?;
        dir.sync()
    };
    write().inspect_err(|_| {
        let _ = dir.unlink(&temporary);
    })
}

// ----------------------------------------------------------------- lock

/// An exclusive cross-process lock (also excludes other threads, since every
/// holder uses its own open file description).
struct RootLock(File);

impl RootLock {
    fn take(root: &Path) -> Result<Self, Error> {
        Self::acquire(root, true)?
            .ok_or_else(|| Error::retry("Silo could not lock its ChatGPT app folder."))
    }

    /// Like `take`, but `None` at once when another holder (a download or extraction)
    /// has the lock.
    fn try_take(root: &Path) -> Result<Option<Self>, Error> {
        Self::acquire(root, false)
    }

    fn acquire(root: &Path, wait: bool) -> Result<Option<Self>, Error> {
        let failed = || Error::retry("Silo could not lock its ChatGPT app folder.");
        let dir = Dir::open_root(root, true)
            .map_err(|_| Error::retry("Silo could not prepare its ChatGPT app folder."))?;
        // Open the existing file; create it exclusively when missing and, when
        // another caller wins that race, open the winner's file.
        let mut opened = None;
        for _ in 0..8 {
            let attempt = dir.open_file(".lock", libc::O_RDWR, 0).or_else(|_| {
                dir.open_file(".lock", libc::O_RDWR | libc::O_CREAT | libc::O_EXCL, 0o600)
            });
            if let Ok(file) = attempt {
                opened = Some(file);
                break;
            }
        }
        let file = opened.ok_or_else(failed)?;
        let meta = file.metadata().map_err(|_| failed())?;
        if !meta.is_file() || meta.uid() != effective_uid() {
            return Err(failed());
        }
        let mode = if wait {
            libc::LOCK_EX
        } else {
            libc::LOCK_EX | libc::LOCK_NB
        };
        while unsafe { libc::flock(file.as_raw_fd(), mode) } != 0 {
            let error = std::io::Error::last_os_error();
            if !wait && error.kind() == std::io::ErrorKind::WouldBlock {
                return Ok(None);
            }
            if error.kind() != std::io::ErrorKind::Interrupted {
                return Err(failed());
            }
        }
        Ok(Some(Self(file)))
    }
}

impl Drop for RootLock {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

// ------------------------------------------------------------- download

/// The length of an existing resumable download, 0 when there is none. Anything
/// that is not a plain single-link file of ours (a planted symlink, a hard link
/// to another file) is removed, never followed.
fn safe_part_len(part: &Path) -> std::io::Result<u64> {
    match fs::symlink_metadata(part) {
        Ok(meta) if meta.is_file() && meta.nlink() == 1 && meta.uid() == effective_uid() => {
            Ok(meta.len())
        }
        Ok(_) => fs::remove_file(part).map(|()| 0),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(error),
    }
}

/// Opens the download file without following a link at its name. A fresh
/// download is created exclusively; a resume re-checks what it opened.
fn open_part(part: &Path, append: bool) -> std::io::Result<File> {
    if !append {
        match fs::remove_file(part) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
        return OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(part);
    }
    let file = OpenOptions::new()
        .write(true)
        .append(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(part)?;
    let meta = file.metadata()?;
    if !meta.is_file() || meta.nlink() != 1 || meta.uid() != effective_uid() {
        return Err(denied("download file is not a plain file"));
    }
    Ok(file)
}

/// Fetches `url` into `part` (resuming if the file already has a prefix) until
/// it holds `total` bytes, reporting the byte count. A `total` of 0 means the size is not
/// known: the download is complete when the server ends the stream (the caller verifies
/// it by checksum). Tests substitute this.
pub(crate) trait Downloader {
    fn fetch(
        &self,
        url: &str,
        part: &Path,
        total: u64,
        progress: &mut dyn FnMut(u64),
    ) -> Result<(), Error>;
}

pub(crate) struct HttpDownloader {
    attempts: u32,
    backoff: Duration,
    /// Tests only: serve from a loopback HTTP server instead of HTTPS.
    #[cfg(test)]
    plain_http: bool,
}

/// What an unexpected HTTP status for the pinned package means. Only a confirmed removal
/// (404 or 410) is final; a refusal (401 or 403, typically a proxy, a filtering network
/// or a regional block that another network does not have) and every other answer are
/// retried with the usual backoff.
fn refusal(code: u16) -> Error {
    match code {
        404 | 410 => Error::fatal(format!(
            "OpenAI no longer serves the pinned ChatGPT app (HTTP {code})."
        )),
        401 | 403 => Error::retry(format!(
            "OpenAI's server refused the ChatGPT download (HTTP {code}). A proxy, firewall or network filter may be blocking it. Silo tries again, including after you switch networks."
        )),
        code => Error::retry(format!("OpenAI's server answered HTTP {code}.")),
    }
}

impl Default for HttpDownloader {
    fn default() -> Self {
        Self {
            attempts: 5,
            backoff: Duration::from_secs(2),
            #[cfg(test)]
            plain_http: false,
        }
    }
}

impl HttpDownloader {
    fn attempt(
        &self,
        client: &reqwest::blocking::Client,
        url: &str,
        part: &Path,
        total: u64,
        progress: &mut dyn FnMut(u64),
    ) -> Result<(), Error> {
        let disk = || {
            Error::retry(
                "Silo could not write the ChatGPT download. Check free disk space and retry.",
            )
        };
        let mut have = safe_part_len(part).map_err(|_| disk())?;
        if total > 0 && have > total {
            fs::remove_file(part).map_err(|_| disk())?;
            have = 0;
        }
        if total > 0 && have == total {
            progress(have);
            return Ok(());
        }
        let mut request = client.get(url);
        if have > 0 {
            request = request.header(reqwest::header::RANGE, format!("bytes={have}-"));
        }
        let mut response = request.send().map_err(|error| {
            if error.is_timeout() || error.is_connect() {
                Error::retry(
                    "Silo could not connect to OpenAI to download the ChatGPT app. A firewall or network filter may be holding Silo's connection.",
                )
            } else {
                Error::retry("Silo could not reach OpenAI to download the ChatGPT app.")
            }
        })?;
        let status = response.status();
        let append = match status.as_u16() {
            206 => true,
            200 => false,
            // An unknown size cannot tell a finished download from a stale one: keep it for
            // the caller's checksum, which discards it when it does not match.
            416 if total == 0 => return Ok(()),
            416 => {
                let _ = fs::remove_file(part);
                return Err(Error::retry("The ChatGPT download restarted."));
            }
            code => return Err(refusal(code)),
        };
        let mut file = open_part(part, append).map_err(|_| disk())?;
        let mut written = if append { have } else { 0 };
        let limit = if total == 0 {
            UNKNOWN_SIZE_LIMIT
        } else {
            total
        };
        let mut buffer = vec![0u8; 256 * 1024];
        loop {
            let count = response
                .read(&mut buffer)
                .map_err(|_| Error::retry("The ChatGPT download was interrupted."))?;
            if count == 0 {
                break;
            }
            if written + count as u64 > limit {
                drop(file);
                let _ = fs::remove_file(part);
                return Err(Error::retry(
                    "The ChatGPT download was larger than expected.",
                ));
            }
            file.write_all(&buffer[..count]).map_err(|_| disk())?;
            written += count as u64;
            progress(written);
        }
        file.sync_all().map_err(|_| disk())?;
        if total == 0 || written == total {
            Ok(())
        } else {
            Err(Error::retry("The ChatGPT download ended early."))
        }
    }
}

impl Downloader for HttpDownloader {
    fn fetch(
        &self,
        url: &str,
        part: &Path,
        total: u64,
        progress: &mut dyn FnMut(u64),
    ) -> Result<(), Error> {
        #[cfg(test)]
        let plain_http = self.plain_http;
        #[cfg(not(test))]
        let plain_http = false;
        let client = reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(20))
            // The blocking client has no stall timeout; this bounds one attempt
            // (a 450 MB package at 250 KB/s) and a retry resumes where it stopped.
            .timeout(Duration::from_secs(30 * 60))
            .redirect(reqwest::redirect::Policy::custom(move |attempt| {
                if attempt.previous().len() < 5
                    && (attempt.url().scheme() == "https"
                        || (plain_http && attempt.url().scheme() == "http"))
                {
                    attempt.follow()
                } else {
                    attempt.stop()
                }
            }))
            .https_only(!plain_http)
            .user_agent(concat!("Silo/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| Error::fatal("Silo could not start a secure download."))?;
        let mut last = Error::retry("The ChatGPT download failed.");
        for attempt in 0..self.attempts {
            if attempt > 0 {
                std::thread::sleep(self.backoff * 2u32.pow(attempt - 1));
            }
            match self.attempt(&client, url, part, total, progress) {
                Ok(()) => return Ok(()),
                Err(error) if error.retryable => last = error,
                Err(error) => return Err(error),
            }
        }
        Err(last)
    }
}

pub(crate) fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

/// Size, then SHA-256. A deb that fails is deleted.
fn verify_package(path: &Path, asset: &Asset) -> Result<(), Error> {
    let read = || Error::retry("Silo could not read the downloaded ChatGPT package.");
    let size = fs::metadata(path).map_err(|_| read())?.len();
    if size != asset.bytes {
        let _ = fs::remove_file(path);
        return Err(Error::retry(
            "The downloaded ChatGPT package has the wrong size and was discarded.",
        ));
    }
    if sha256_file(path).map_err(|_| read())? != asset.sha256 {
        let _ = fs::remove_file(path);
        return Err(Error::fatal(
            "The downloaded ChatGPT package does not match Silo's pinned checksum and was discarded.",
        ));
    }
    Ok(())
}

// ----------------------------------------------------------- extraction

/// The decompressed `data.tar` of a deb, produced by an established tool. It
/// never runs maintainer scripts: macOS bsdtar reads the ar container and the
/// compressed member and rewrites it as plain tar; Linux uses `dpkg-deb`.
struct TarStream {
    children: Vec<Child>,
    stdout: std::process::ChildStdout,
    finished: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

/// A short tool run (version or member listing) that must not hang extraction.
const TOOL_TIMEOUT: Duration = Duration::from_secs(30);
/// The longest one unpacking may take before its tools are stopped.
const UNPACK_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// Runs `command` with piped stdout and discarded stderr, killing it at `timeout`.
fn output_with_timeout(
    command: &mut Command,
    timeout: Duration,
) -> std::io::Result<std::process::Output> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut stdout = child.stdout.take().expect("piped");
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stdout.read_to_end(&mut bytes);
        bytes
    });
    let deadline = std::time::Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "the tool did not finish in time",
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    Ok(std::process::Output {
        status,
        stdout: reader.join().unwrap_or_default(),
        stderr: Vec::new(),
    })
}

impl TarStream {
    /// Stops the tools if unpacking has not finished by `UNPACK_TIMEOUT`, which
    /// ends a read blocked on a stalled tool.
    fn guarded(children: Vec<Child>, stdout: std::process::ChildStdout) -> Self {
        let finished = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let pids: Vec<libc::pid_t> = children
            .iter()
            .filter_map(|child| libc::pid_t::try_from(child.id()).ok())
            .collect();
        let watched = finished.clone();
        std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + UNPACK_TIMEOUT;
            while std::time::Instant::now() < deadline {
                if watched.load(std::sync::atomic::Ordering::SeqCst) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(500));
            }
            if !watched.load(std::sync::atomic::Ordering::SeqCst) {
                for pid in pids {
                    // SAFETY: the tools are reaped only after `finished` is set, so
                    // an unset flag means these PIDs still name this stream's children.
                    unsafe { libc::kill(pid, libc::SIGKILL) };
                }
            }
        });
        Self {
            children,
            stdout,
            finished,
        }
    }

    fn open(deb: &Path) -> Result<Self, Error> {
        let missing =
            || Error::fatal("Silo could not find the system tool that unpacks .deb files.");
        if cfg!(target_os = "macos") {
            let tar = "/usr/bin/tar";
            let list = output_with_timeout(Command::new(tar).arg("-tf").arg(deb), TOOL_TIMEOUT)
                .map_err(|_| missing())?;
            let member = String::from_utf8_lossy(&list.stdout)
                .lines()
                .find(|name| name.starts_with("data.tar"))
                .map(str::to_owned)
                .ok_or_else(|| Error::fatal("The ChatGPT package has no data archive."))?;
            let first = Command::new(tar)
                .arg("-xOf")
                .arg(deb)
                .arg(&member)
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|_| missing())?;
            Self::pipe(first, Command::new(tar).args(["-cf", "-", "@-"])).map_err(|_| missing())
        } else {
            let tool = ["/usr/bin/dpkg-deb", "dpkg-deb"]
                .into_iter()
                .find(|candidate| {
                    output_with_timeout(Command::new(candidate).arg("--version"), TOOL_TIMEOUT)
                        .is_ok()
                })
                .ok_or_else(missing)?;
            let mut child = Command::new(tool)
                .arg("--fsys-tarfile")
                .arg(deb)
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|_| missing())?;
            let stdout = child.stdout.take().expect("piped");
            Ok(Self::guarded(vec![child], stdout))
        }
    }

    fn pipe(mut first: Child, second: &mut Command) -> std::io::Result<Self> {
        let input = first.stdout.take().expect("piped");
        let mut second = second
            .stdin(Stdio::from(input))
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .inspect_err(|_| {
                let _ = first.kill();
                let _ = first.wait();
            })?;
        let stdout = second.stdout.take().expect("piped");
        Ok(Self::guarded(vec![first, second], stdout))
    }

    /// Reads the rest of the stream, then requires every tool to have succeeded.
    fn finish(mut self) -> Result<(), Error> {
        let _ = std::io::copy(&mut self.stdout, &mut std::io::sink());
        self.finished
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let mut ok = true;
        for child in &mut self.children {
            ok &= child.wait().is_ok_and(|status| status.success());
        }
        if ok {
            Ok(())
        } else {
            Err(Error::fatal("The ChatGPT package could not be unpacked."))
        }
    }
}

impl Read for TarStream {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.stdout.read(buffer)
    }
}

impl Drop for TarStream {
    fn drop(&mut self) {
        self.finished
            .store(true, std::sync::atomic::Ordering::SeqCst);
        for child in &mut self.children {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn reject(path: &str, why: &str) -> Error {
    Error::fatal(format!(
        "The ChatGPT package was refused: {why} ({}).",
        path.chars().take(120).collect::<String>()
    ))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Directory,
    File,
    Symlink,
}

/// Path components of an entry below the app root, `None` outside the tree.
fn tree_components(raw: &[u8]) -> Result<Option<Vec<String>>, Error> {
    let text = std::str::from_utf8(raw).map_err(|_| reject("non-UTF-8 name", "invalid name"))?;
    if text.is_empty() || text.contains('\0') {
        return Err(reject(text, "empty or invalid name"));
    }
    if text.starts_with('/') {
        return Err(reject(text, "absolute path"));
    }
    let mut parts = Vec::new();
    for part in text.split('/') {
        match part {
            "" | "." => {}
            ".." => return Err(reject(text, "path with `..`")),
            other => parts.push(other),
        }
    }
    if parts.len() < TREE_PREFIX.len() || parts[..TREE_PREFIX.len()] != TREE_PREFIX {
        return Ok(None);
    }
    Ok(Some(
        parts[TREE_PREFIX.len()..]
            .iter()
            .map(|part| (*part).to_owned())
            .collect(),
    ))
}

/// A relative link target that can never leave the tree: any leading `..` stays
/// within the link's own (symlink-free) directory depth, and no `..` follows a
/// normal component (which could pass through another symlink).
fn check_link_target(target: &[u8], parent_depth: usize, path: &str) -> Result<(), Error> {
    let text = std::str::from_utf8(target).map_err(|_| reject(path, "non-UTF-8 symlink target"))?;
    if text.is_empty() || text.starts_with('/') || text.contains('\0') {
        return Err(reject(path, "absolute or empty symlink target"));
    }
    let mut up = 0usize;
    let mut normal = false;
    for part in text.split('/') {
        match part {
            "" | "." => {}
            ".." if normal => return Err(reject(path, "symlink target escapes the tree")),
            ".." => up += 1,
            _ => normal = true,
        }
    }
    if up > parent_depth {
        return Err(reject(path, "symlink target escapes the tree"));
    }
    Ok(())
}

/// Hard limits for one extraction. Tests lower them.
struct Limits {
    max_entries: usize,
    /// Bytes written to disk (the effective sizes of all entries, summed).
    max_bytes: u64,
    /// Longest entry path or link target, including GNU long names.
    max_name: usize,
    /// Total PAX extended header bytes of one entry.
    max_pax: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_entries: MAX_ENTRIES,
            max_bytes: MAX_UNPACKED_BYTES,
            max_name: MAX_NAME_BYTES,
            max_pax: MAX_PAX_BYTES,
        }
    }
}

/// Fails once more than `left` bytes were read. Bounds everything the tar
/// parser consumes, including metadata records it buffers by itself.
struct Capped<R> {
    inner: R,
    left: u64,
}

impl<R: Read> Read for Capped<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let count = self.inner.read(buffer)?;
        self.left = self.left.checked_sub(count as u64).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "package stream too large")
        })?;
        Ok(count)
    }
}

/// Opens the parent directory of the tree path `rel` below `dest`, creating
/// missing directories, one `openat` per component and never through a link.
fn parent_of(dest: &Dir, rel: &[String]) -> std::io::Result<Option<Dir>> {
    let mut current: Option<Dir> = None;
    for part in &rel[..rel.len().saturating_sub(1)] {
        let next = current.as_ref().unwrap_or(dest).subdir(part, true)?.0;
        current = Some(next);
    }
    Ok(current)
}

fn extract_tree(reader: impl Read, dest: &Dir) -> Result<(), Error> {
    extract_tree_with(reader, dest, &Limits::default())
}

/// Validates and writes the `usr/lib/chatgpt` entries of a tar stream into the
/// empty directory `dest`. The first violation aborts at once (nothing after
/// it is read); the caller discards `dest`. Every directory and file is synced
/// before this returns.
fn extract_tree_with(reader: impl Read, dest: &Dir, limits: &Limits) -> Result<(), Error> {
    let io = |_: std::io::Error| {
        Error::retry("Silo could not write the ChatGPT app. Check free disk space and retry.")
    };
    let unreadable = |_: std::io::Error| Error::fatal("The ChatGPT package could not be read.");
    let mut archive = tar::Archive::new(Capped {
        inner: reader,
        left: limits.max_bytes.saturating_mul(2).saturating_add(1 << 20),
    });
    let entries = archive.entries().map_err(unreadable)?;
    // Lower-cased relative path -> (exact path, kind).
    let mut seen: HashMap<String, (String, Kind)> = HashMap::new();
    let mut symlinks: HashSet<String> = HashSet::new();
    let (mut count, mut unpacked) = (0usize, 0u64);
    for entry in entries {
        let mut entry = entry.map_err(unreadable)?;
        count += 1;
        if count > limits.max_entries {
            return Err(reject("archive", "too many entries"));
        }
        let raw = entry.path_bytes().into_owned();
        if raw.len() > limits.max_name {
            return Err(reject("long name", "entry name is too long"));
        }
        if entry
            .link_name_bytes()
            .is_some_and(|l| l.len() > limits.max_name)
        {
            return Err(reject("long link", "link target is too long"));
        }
        let mut pax_bytes = 0usize;
        if let Some(extensions) = entry.pax_extensions().map_err(unreadable)? {
            for extension in extensions {
                let extension = extension.map_err(|_| reject("pax", "invalid extended header"))?;
                pax_bytes = pax_bytes
                    .saturating_add(extension.key_bytes().len())
                    .saturating_add(extension.value_bytes().len());
            }
        }
        if pax_bytes > limits.max_pax {
            return Err(reject("pax", "extended header is too large"));
        }
        let Some(rel) = tree_components(&raw)? else {
            continue; // Outside usr/lib/chatgpt: never extracted.
        };
        if rel.iter().any(|part| part.len() > MAX_COMPONENT_BYTES) {
            return Err(reject(
                &String::from_utf8_lossy(&raw),
                "name component is too long",
            ));
        }
        let shown = String::from_utf8_lossy(&raw).into_owned();
        let kind = match entry.header().entry_type() {
            tar::EntryType::Regular | tar::EntryType::Continuous => Kind::File,
            tar::EntryType::Directory => Kind::Directory,
            tar::EntryType::Symlink => Kind::Symlink,
            tar::EntryType::XGlobalHeader | tar::EntryType::XHeader => continue,
            _ => return Err(reject(&shown, "hard link, device or other special file")),
        };
        let mode = entry
            .header()
            .mode()
            .map_err(|_| reject(&shown, "invalid mode"))?;
        if mode & 0o6000 != 0 {
            return Err(reject(&shown, "setuid or setgid bit"));
        }
        // The effective size (a PAX `size` record overrides the header's).
        let size = entry.size();
        if kind != Kind::File && size != 0 {
            return Err(reject(&shown, "directory or link with data"));
        }
        if rel.is_empty() {
            if kind != Kind::Directory {
                return Err(reject(&shown, "app root is not a directory"));
            }
            continue;
        }
        // Parents: must be directories we made, never symlinks, never colliding.
        let mut joined = String::new();
        for (index, part) in rel.iter().enumerate() {
            if !joined.is_empty() {
                joined.push('/');
            }
            joined.push_str(part);
            let lower = joined.to_lowercase();
            let last = index + 1 == rel.len();
            if !last && symlinks.contains(&lower) {
                return Err(reject(&shown, "path passes through a symlink"));
            }
            match seen.get(&lower) {
                Some((exact, _)) if *exact != joined => {
                    return Err(reject(&shown, "case-insensitive name collision"))
                }
                Some((_, existing)) if !last && *existing != Kind::Directory => {
                    return Err(reject(&shown, "parent is not a directory"))
                }
                Some((_, existing))
                    if last && !(*existing == Kind::Directory && kind == Kind::Directory) =>
                {
                    return Err(reject(&shown, "duplicate entry"))
                }
                Some(_) => {}
                None => {
                    let entry_kind = if last { kind } else { Kind::Directory };
                    seen.insert(lower.clone(), (joined.clone(), entry_kind));
                    if entry_kind == Kind::Symlink {
                        symlinks.insert(lower);
                    }
                }
            }
        }
        let name = rel.last().expect("not empty").as_str();
        let parent = parent_of(dest, &rel).map_err(io)?;
        let parent = parent.as_ref().unwrap_or(dest);
        let collision = |error: std::io::Error| {
            if matches!(
                error.raw_os_error(),
                Some(libc::EEXIST | libc::ENOTDIR | libc::ELOOP)
            ) {
                reject(&shown, "name collision")
            } else {
                io(error)
            }
        };
        match kind {
            Kind::Directory => {
                parent.subdir(name, true).map_err(collision)?;
            }
            Kind::File => {
                unpacked = unpacked
                    .checked_add(size)
                    .filter(|total| *total <= limits.max_bytes)
                    .ok_or_else(|| reject(&shown, "package is too large"))?;
                let permissions = if mode & 0o111 != 0 { 0o755 } else { 0o644 };
                // O_EXCL refuses an existing name, including a name the
                // filesystem folds together (case or normalization); O_NOFOLLOW
                // never follows a link planted at that name.
                let mut file = parent.create_file(name, permissions).map_err(collision)?;
                let written = std::io::copy(&mut (&mut entry).take(size), &mut file).map_err(io)?;
                if written != size {
                    return Err(reject(&shown, "entry is shorter than its size"));
                }
                file.set_permissions(fs::Permissions::from_mode(permissions))
                    .map_err(io)?;
                file.sync_all().map_err(io)?;
            }
            Kind::Symlink => {
                let link = entry
                    .link_name_bytes()
                    .ok_or_else(|| reject(&shown, "symlink without a target"))?
                    .into_owned();
                check_link_target(&link, rel.len() - 1, &shown)?;
                let link = std::str::from_utf8(&link).expect("checked");
                parent.symlink(link, name).map_err(collision)?;
            }
        }
    }
    // Created directories get a fixed mode (nothing in the tree is group/world
    // writable) and are synced bottom-up so a published tree is fully durable.
    let mut directories: Vec<Vec<String>> = seen
        .values()
        .filter(|(_, kind)| *kind == Kind::Directory)
        .map(|(exact, _)| exact.split('/').map(str::to_owned).collect())
        .collect();
    directories.sort_by_key(|parts| std::cmp::Reverse(parts.len()));
    for parts in &directories {
        let mut current: Option<Dir> = None;
        for part in parts {
            let next = current
                .as_ref()
                .unwrap_or(dest)
                .subdir(part, false)
                .map_err(io)?
                .0;
            current = Some(next);
        }
        let directory = current.expect("not empty");
        directory.chmod(0o755).map_err(io)?;
        directory.sync().map_err(io)?;
    }
    dest.chmod(0o755).map_err(io)?;
    dest.sync().map_err(io)?;
    check_executables(dest)
}

/// Every executable LCU needs must be a regular file with an execute bit.
fn check_executables(tree: &Dir) -> Result<(), Error> {
    for relative in REQUIRED_EXECUTABLES {
        let ok = tree.stat(relative, true).is_ok_and(|stat| {
            stat.st_mode & libc::S_IFMT == libc::S_IFREG && stat.st_mode & 0o111 != 0
        });
        if !ok {
            return Err(reject(
                relative,
                "a required executable is missing or not executable",
            ));
        }
    }
    Ok(())
}

// --------------------------------------------------- tree digest and record

struct Item {
    relative: String,
    path: PathBuf,
    kind: u8,
    mode: u32,
    size: u64,
    mtime: (i64, i64),
    link: Vec<u8>,
}

/// What `digest_tree` computes: a cheap digest over the tree's shape and file
/// metadata, and (when asked) one over every file's content.
struct Digests {
    stat: String,
    content: Option<String>,
    entries: u64,
    bytes: u64,
}

fn collect_items(directory: &Path, prefix: &str, out: &mut Vec<Item>) -> std::io::Result<()> {
    let invalid = |why: &'static str| std::io::Error::new(std::io::ErrorKind::InvalidData, why);
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| invalid("name is not UTF-8"))?;
        let relative = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        let meta = fs::symlink_metadata(entry.path())?;
        let mode = meta.mode() & 0o7777;
        if mode & 0o6000 != 0 || (!meta.file_type().is_symlink() && mode & 0o022 != 0) {
            return Err(invalid("unsafe mode"));
        }
        let kind = if meta.is_dir() {
            1
        } else if meta.is_file() {
            if meta.nlink() != 1 {
                return Err(invalid("hard link"));
            }
            2
        } else if meta.file_type().is_symlink() {
            3
        } else {
            return Err(invalid("special file"));
        };
        let link = if kind == 3 {
            fs::read_link(entry.path())?.as_os_str().as_bytes().to_vec()
        } else {
            Vec::new()
        };
        out.push(Item {
            relative: relative.clone(),
            path: entry.path(),
            kind,
            mode: if kind == 3 { 0 } else { mode },
            size: if kind == 2 { meta.len() } else { 0 },
            mtime: if kind == 2 {
                (meta.mtime(), meta.mtime_nsec())
            } else {
                (0, 0)
            },
            link,
        });
        if out.len() > MAX_ENTRIES {
            return Err(invalid("too many entries"));
        }
        if kind == 1 {
            collect_items(&entry.path(), &relative, out)?;
        }
    }
    Ok(())
}

fn hash_file(item: &Item) -> std::io::Result<[u8; 32]> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&item.path)?;
    let meta = file.metadata()?;
    if !meta.is_file() || meta.len() != item.size {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "file changed",
        ));
    }
    let mut hash = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    let mut total = 0u64;
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        total += count as u64;
        hash.update(&buffer[..count]);
    }
    if total != item.size {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "file changed",
        ));
    }
    Ok(hash.finalize().into())
}

/// Digests of the tree at `directory`: sorted (path, type, mode, size, link
/// target) and, with `content`, each file's SHA-256. The stat digest also
/// covers file mtimes. Any entry that is not a plain file, directory or
/// symlink, any set-id or group/other-writable mode and any hard link fails.
fn digest_tree(directory: &Path, content: bool) -> std::io::Result<Digests> {
    let mut items = Vec::new();
    collect_items(directory, "", &mut items)?;
    items.sort_by(|a, b| a.relative.cmp(&b.relative));
    let mut stat = Sha256::new();
    let mut full = Sha256::new();
    stat.update(b"silo-chatgpt-tree-stat-v1\0");
    full.update(b"silo-chatgpt-tree-content-v1\0");
    let mut bytes = 0u64;
    for item in &items {
        let mut head = Vec::new();
        head.extend((item.relative.len() as u64).to_le_bytes());
        head.extend(item.relative.as_bytes());
        head.push(item.kind);
        head.extend(item.mode.to_le_bytes());
        head.extend(item.size.to_le_bytes());
        head.extend((item.link.len() as u64).to_le_bytes());
        head.extend(&item.link);
        stat.update(&head);
        full.update(&head);
        stat.update(item.mtime.0.to_le_bytes());
        stat.update(item.mtime.1.to_le_bytes());
        bytes += item.size;
        if content && item.kind == 2 {
            full.update(hash_file(item)?);
        }
    }
    Ok(Digests {
        stat: format!("{:x}", stat.finalize()),
        content: content.then(|| format!("{:x}", full.finalize())),
        entries: items.len() as u64,
        bytes,
    })
}

/// Written after the tree is durable; binds the tree to the lock.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    schema_version: u32,
    version: String,
    arch: String,
    deb_sha256: String,
    tree_sha256: String,
    stat_sha256: String,
    entries: u64,
    bytes: u64,
}

fn record_name(name: &str) -> String {
    format!("{name}{RECORD_SUFFIX}")
}

/// Identity of the record file and tree directory as of the last full check.
type Stamp = (u64, u64, i64, i64, u64, u64);

/// Trees whose content was fully verified in this process, with the stat digest
/// they had then. A later call re-hashes only if the stamp or stat digest moved.
static VERIFIED: Mutex<Option<HashMap<PathBuf, (Stamp, String)>>> = Mutex::new(None);

fn stamp(root: &Dir, published: &Dir, name: &str) -> Option<Stamp> {
    let record = root.stat(&record_name(name), false).ok()?;
    let tree = published.stat(name, false).ok()?;
    Some((
        record.st_dev as u64,
        record.st_ino as u64,
        record.st_mtime,
        record.st_mtime_nsec,
        tree.st_dev as u64,
        tree.st_ino as u64,
    ))
}

fn remember(path: PathBuf, stamp: Stamp, stat_digest: String) {
    VERIFIED
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get_or_insert_with(HashMap::new)
        .insert(path, (stamp, stat_digest));
}

/// The published folder for the lock, only if it is exactly what Silo
/// published: a valid publication record that matches the lock and the .deb
/// hash, a real directory owned by this user, the required executables, and a
/// tree whose digest matches the record.
///
/// Every call checks the record, ownership, executables and the stat digest
/// (shape, modes, sizes, mtimes; a few thousand `lstat`s). The full content
/// digest (every byte) runs the first time a process sees this tree, and again
/// whenever the stat digest or the record/tree identity changes.
fn verify_published(root: &Path, lock: &Lock, arch: DebArch) -> Option<PathBuf> {
    let name = lock.directory_name(arch);
    let asset = lock.asset(arch).ok()?;
    let root_dir = Dir::open_root(root, false).ok()?;
    let record: Record =
        serde_json::from_slice(&read_small(&root_dir, &record_name(&name))?).ok()?;
    if record.schema_version != RECORD_SCHEMA
        || record.version != lock.version
        || record.arch != arch.name()
        || record.deb_sha256 != asset.sha256
        || !valid_sha256(&record.tree_sha256)
        || !valid_sha256(&record.stat_sha256)
    {
        return None;
    }
    let (published_dir, _) = root_dir.subdir(PUBLISHED_DIR, false).ok()?;
    let (tree, _) = published_dir.subdir(&name, false).ok()?;
    if tree.stat_self().ok()?.st_mode & 0o022 != 0 {
        return None;
    }
    check_executables(&tree).ok()?;
    let path = published_path(root).join(&name);
    let identity = stamp(&root_dir, &published_dir, &name)?;
    let stat_digest = digest_tree(&path, false).ok()?.stat;
    let cached = VERIFIED
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .as_ref()
        .and_then(|map| map.get(&path).cloned());
    let trusted = stat_digest == record.stat_sha256
        && cached.is_some_and(|(seen, digest)| seen == identity && digest == stat_digest);
    if !trusted {
        let full = digest_tree(&path, true).ok()?;
        if full.content.as_deref() != Some(record.tree_sha256.as_str()) {
            return None;
        }
        remember(path.clone(), identity, stat_digest);
    }
    Some(path)
}

// --------------------------------------------------------------- ensure

/// Removes leftovers of interrupted or rejected work: staging folders, folders
/// moved aside and half-written temporaries.
fn clean_staging(root: &Path, dir: &Dir) {
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with(".staging-")
                || name.starts_with(".rejected-")
                || (name.starts_with('.') && name.ends_with(".tmp"))
            {
                let _ = make_tree_deletable(&entry.path());
                let _ = dir.remove_entry(root, &name);
            }
        }
    }
}

/// A staging directory that deletes itself unless published.
struct Staging<'a> {
    root_path: &'a Path,
    root: &'a Dir,
    name: String,
    armed: bool,
}

impl Drop for Staging<'_> {
    fn drop(&mut self) {
        if self.armed {
            let _ = make_tree_deletable(&self.root_path.join(&self.name));
            let _ = self.root.remove_entry(self.root_path, &self.name);
        }
    }
}

fn throttled<'a>(mut report: impl FnMut(u64) + 'a) -> impl FnMut(u64) + 'a {
    let mut last = Instant::now() - Duration::from_secs(1);
    move |bytes| {
        if last.elapsed() >= Duration::from_millis(250) {
            last = Instant::now();
            report(bytes);
        }
    }
}

/// Deletes a download file that is not a plain single-link file of ours (for
/// example a planted symlink) without following it. Returns whether a usable
/// file remains.
fn discard_unsafe_file(dir: &Dir, name: &str) -> Result<bool, Error> {
    let failed = || Error::retry("Silo could not prepare the ChatGPT download folder.");
    match dir.stat(name, false) {
        Ok(stat)
            if stat.st_mode & libc::S_IFMT == libc::S_IFREG
                && stat.st_nlink == 1
                && stat.st_uid == effective_uid() =>
        {
            Ok(true)
        }
        Ok(_) => dir.unlink(name).map(|()| false).map_err(|_| failed()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(failed()),
    }
}

/// Makes the pinned app present under `root` and returns its canonical path.
/// Idempotent; concurrent calls (threads or processes) serialize on the root
/// lock and the later ones find the published folder.
pub(crate) fn ensure(
    root: &Path,
    lock: &Lock,
    arch: DebArch,
    downloader: &dyn Downloader,
    report: &dyn Fn(Status),
) -> Result<PathBuf, Error> {
    let result = ensure_inner(root, lock, arch, downloader, report);
    if let Err(error) = &result {
        report(error.status());
    }
    result
}

fn ensure_inner(
    root: &Path,
    lock: &Lock,
    arch: DebArch,
    downloader: &dyn Downloader,
    report: &dyn Fn(Status),
) -> Result<PathBuf, Error> {
    let name = lock.directory_name(arch);
    let asset = lock.asset(arch)?;
    let ready = |path: PathBuf| -> Result<PathBuf, Error> {
        let path = fs::canonicalize(path)
            .map_err(|_| Error::retry("Silo could not resolve the ChatGPT app folder."))?;
        report(Status::Ready {
            path: path.clone(),
            version: lock.version.clone(),
        });
        Ok(path)
    };
    // Fast path without the lock: a published, verified folder is immutable.
    if let Some(path) = verify_published(root, lock, arch) {
        return ready(path);
    }
    let _lock = RootLock::take(root)?;
    if let Some(path) = verify_published(root, lock, arch) {
        return ready(path);
    }
    let prepare =
        |_: std::io::Error| Error::retry("Silo could not prepare its ChatGPT app folder.");
    let (root_dir, published_dir) = open_storage(root, true).map_err(prepare)?;
    let published_path = published_path(root);
    // Whatever sits under the published name is not something Silo published
    // (no valid record, wrong tree, damaged): remove it, record first, so a
    // crash can never leave a record pointing at a half-deleted tree.
    root_dir
        .remove_entry(root, &record_name(&name))
        .and_then(|()| published_dir.remove_entry(&published_path, &name))
        .map_err(prepare)?;
    clean_staging(root, &root_dir);
    clean_staging(&published_path, &published_dir);

    let (downloads, _) = root_dir.subdir("downloads", true).map_err(prepare)?;
    let deb_name = format!("chatgpt_{}_{}.deb", lock.version, arch.name());
    let part_name = format!("{deb_name}.part");
    let deb = root.join("downloads").join(&deb_name);
    let part = root.join("downloads").join(&part_name);
    free_space_check(root, asset.bytes)?;

    let have_deb = discard_unsafe_file(&downloads, &deb_name)?;
    discard_unsafe_file(&downloads, &part_name)?;
    if !have_deb {
        report(Status::Downloading {
            received_bytes: fs::metadata(&part).map_or(0, |m| m.len()),
            total_bytes: asset.bytes,
        });
        let mut progress = throttled(|received| {
            report(Status::Downloading {
                received_bytes: received,
                total_bytes: asset.bytes,
            })
        });
        downloader.fetch(&asset.url, &part, asset.bytes, &mut progress)?;
        downloads
            .rename(&part_name, &deb_name)
            .map_err(|_| Error::retry("Silo could not finish the ChatGPT download."))?;
    }
    report(Status::Verifying);
    verify_package(&deb, asset)?;

    report(Status::Extracting);
    let staging_name = format!(".staging-{}", unique_suffix());
    let (staging_dir, _) = root_dir
        .subdir(&staging_name, true)
        .map_err(|_| Error::retry("Silo could not prepare space for the ChatGPT app."))?;
    let mut staging = Staging {
        root_path: root,
        root: &root_dir,
        name: staging_name,
        armed: true,
    };
    let mut stream = TarStream::open(&deb)?;
    if let Err(error) = extract_tree(&mut stream, &staging_dir) {
        // Abort on the first rejected entry: dropping the stream kills the
        // unpacking tools instead of draining the rest of the package.
        drop(stream);
        return Err(error);
    }
    stream.finish()?;

    // The tree is complete and synced; take its digests from what is on disk.
    let staged_path = root.join(&staging.name);
    let digests = digest_tree(&staged_path, true)
        .map_err(|_| Error::fatal("The extracted ChatGPT app could not be verified."))?;
    let record = Record {
        schema_version: RECORD_SCHEMA,
        version: lock.version.clone(),
        arch: arch.name().to_owned(),
        deb_sha256: asset.sha256.clone(),
        tree_sha256: digests.content.clone().unwrap_or_default(),
        stat_sha256: digests.stat.clone(),
        entries: digests.entries,
        bytes: digests.bytes,
    };
    let record_bytes = serde_json::to_vec_pretty(&record)
        .map_err(|_| Error::fatal("Silo could not record the ChatGPT app."))?;

    // Publish: one atomic rename, then the parent is synced, then the record,
    // which is the last durable step. Until it exists the folder is not ready.
    let target = published_path.join(&name);
    let renamed = if published_dir.absent_entry(&name) {
        root_dir.rename_into(&staging.name, &published_dir, &name)
    } else {
        Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists))
    };
    if let Err(error) = renamed {
        drop(staging);
        // Never trust a folder that appeared meanwhile without verifying it.
        return match verify_published(root, lock, arch) {
            Some(path) => ready(path),
            None => Err(Error::retry(format!(
                "Silo could not publish the ChatGPT app: {error}"
            ))),
        };
    }
    staging.armed = false;
    let published = (|| -> std::io::Result<()> {
        root_dir.sync()?;
        published_dir.sync()?;
        write_file_atomically(&root_dir, &record_name(&name), &record_bytes)
    })();
    if published.is_err() {
        let _ = root_dir.remove_entry(root, &record_name(&name));
        let _ = published_dir.remove_entry(&published_path, &name);
        return Err(Error::retry(
            "Silo could not finish publishing the ChatGPT app. Check disk access and retry.",
        ));
    }
    if let Some(identity) = stamp(&root_dir, &published_dir, &name) {
        remember(target.clone(), identity, digests.stat);
    }
    let _ = fs::remove_file(&deb);
    ready(target)
}

/// `<root>/published`: the folder computers mount.
pub(crate) fn published_path(root: &Path) -> PathBuf {
    root.join(PUBLISHED_DIR)
}

/// Opens the storage root and its `published/` folder, creating both with
/// `create`. Trees an earlier build published directly under the root move
/// into `published/` (they are verified like any other tree before use).
fn open_storage(root: &Path, create: bool) -> std::io::Result<(Dir, Dir)> {
    let root_dir = Dir::open_root(root, create)?;
    let (published, created) = root_dir.subdir(PUBLISHED_DIR, create)?;
    if created {
        root_dir.sync()?;
    }
    if create {
        // The guest's working account must be able to enter the mounted folder. It holds
        // only verified, read-only trees and nothing private (records live outside).
        if published.stat_self()?.st_mode & 0o7777 != 0o755 {
            published.chmod(0o755)?;
        }
        migrate_legacy_trees(root, &root_dir, &published);
    }
    Ok((root_dir, published))
}

/// Moves version folders left at the old location (`<root>/<version>-<arch>`)
/// into `published/`; one that already exists there is removed instead.
fn migrate_legacy_trees(root: &Path, root_dir: &Dir, published: &Dir) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_version_dir(&name) || !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        if published.absent_entry(&name) {
            let _ = root_dir.rename_into(&name, published, &name);
        } else {
            let _ = root_dir.remove_entry(root, &name);
        }
    }
}

fn is_version_dir(name: &str) -> bool {
    ["-arm64", "-amd64"]
        .iter()
        .any(|suffix| name.strip_suffix(suffix).is_some_and(valid_version))
}

/// Creates the storage root and `published/` (empty is fine), migrates trees from
/// the old layout and returns the canonical folder to mount into computers.
pub(crate) fn ensure_published_dir(root: &Path) -> Result<PathBuf, Error> {
    let failed = || Error::retry("Silo could not prepare its ChatGPT app folder.");
    Dir::open_root(root, true).map_err(|_| failed())?;
    let _lock = RootLock::take(root)?;
    open_storage(root, true).map_err(|_| failed())?;
    fs::canonicalize(published_path(root)).map_err(|_| failed())
}

/// `ensure_published_dir` for callers that must not wait: when a download or extraction
/// holds the storage lock the folders already exist (it prepared them first), so the
/// published folder is only resolved.
pub(crate) fn ensure_published_dir_nowait(root: &Path) -> Result<PathBuf, Error> {
    let failed = || Error::retry("Silo could not prepare its ChatGPT app folder.");
    let base = Dir::open_root(root, true).map_err(|_| failed())?;
    if let Some(_lock) = RootLock::try_take(root)? {
        open_storage(root, true).map_err(|_| failed())?;
    }
    // Lock contention skips preparation, not validation of the folder to mount.
    base.subdir("published", false).map_err(|_| failed())?;
    fs::canonicalize(published_path(root)).map_err(|_| failed())
}

impl Dir {
    /// Whether `name` does not exist (without following it).
    fn absent_entry(&self, name: &str) -> bool {
        self.stat(name, false)
            .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
    }
}

fn free_space_check(root: &Path, download_bytes: u64) -> Result<(), Error> {
    let path = std::ffi::CString::new(root.as_os_str().as_encoded_bytes())
        .map_err(|_| Error::fatal("Invalid ChatGPT app folder."))?;
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    if unsafe { libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) } != 0 {
        return Ok(()); // Not fatal; writes report their own errors.
    }
    let stats = unsafe { stats.assume_init() };
    let available = (stats.f_bavail as u64).saturating_mul(stats.f_frsize as u64);
    // The package plus roughly its unpacked tree (about 3.5x the package).
    let required = download_bytes.saturating_mul(5);
    if available < required {
        return Err(Error::retry(format!(
            "Free at least {} MiB to download the ChatGPT app, then retry.",
            required.div_ceil(1024 * 1024)
        )));
    }
    Ok(())
}

/// The status for a device where no operation is running.
pub(crate) fn current_status(root: &Path, lock: &Lock, arch: DebArch) -> Status {
    match verify_published(root, lock, arch) {
        Some(path) => Status::Ready {
            path: fs::canonicalize(path).unwrap_or_default(),
            version: lock.version.clone(),
        },
        None => Status::Idle,
    }
}

/// Removes version folders (and their records) that are neither the pinned one
/// nor named in `in_use` (folder names like `26.928.31416-arm64`), plus stale
/// staging and downloads of other versions. Returns the removed folder names.
pub(crate) fn collect_garbage(
    root: &Path,
    lock: &Lock,
    arch: DebArch,
    in_use: &HashSet<String>,
) -> Result<Vec<String>, Error> {
    if fs::symlink_metadata(root).is_err() {
        return Ok(Vec::new());
    }
    let held = RootLock::take(root)?;
    collect_garbage_locked(root, lock, arch, in_use, &held)
}

/// `collect_garbage` that never waits for the storage lock: `None` when a download or
/// extraction holds it (the caller keeps the work pending and tries again later).
fn try_collect_garbage(
    root: &Path,
    lock: &Lock,
    arch: DebArch,
    in_use: &HashSet<String>,
) -> Result<Option<Vec<String>>, Error> {
    if fs::symlink_metadata(root).is_err() {
        return Ok(Some(Vec::new()));
    }
    let Some(held) = RootLock::try_take(root)? else {
        return Ok(None);
    };
    collect_garbage_locked(root, lock, arch, in_use, &held).map(Some)
}

fn collect_garbage_locked(
    root: &Path,
    lock: &Lock,
    arch: DebArch,
    in_use: &HashSet<String>,
    _held: &RootLock,
) -> Result<Vec<String>, Error> {
    let list_failed = || Error::retry("Could not list ChatGPT app versions.");
    let (root_dir, published_dir) = open_storage(root, true).map_err(|_| list_failed())?;
    clean_staging(root, &root_dir);
    let pinned = lock.directory_name(arch);
    let published = published_path(root);
    clean_staging(&published, &published_dir);
    let mut removed = Vec::new();
    // A record of a version that is going away goes with it (or alone).
    for entry in fs::read_dir(root).map_err(|_| list_failed())?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(stem) = name.strip_suffix(RECORD_SUFFIX) {
            if is_version_dir(stem) && stem != pinned && !in_use.contains(stem) {
                let _ = root_dir.remove_entry(root, &name);
            }
        }
    }
    for entry in fs::read_dir(&published)
        .map_err(|_| list_failed())?
        .flatten()
    {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_version_dir(&name) || name == pinned || in_use.contains(&name) {
            continue;
        }
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            let _ = root_dir.remove_entry(root, &record_name(&name));
            if published_dir.remove_entry(&published, &name).is_ok() {
                removed.push(name);
            }
        }
    }
    if let Ok(entries) = fs::read_dir(root.join("downloads")) {
        let keep = format!("chatgpt_{}_{}.deb", lock.version, arch.name());
        for entry in entries.flatten() {
            if !entry.file_name().to_string_lossy().starts_with(&keep) {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    removed.sort();
    Ok(removed)
}

// ------------------------------------------------------ Tauri commands

/// The ChatGPT app folder of this channel: `<app data>/chatgpt`.
pub(crate) fn storage_root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    use tauri::Manager;
    app.path()
        .app_data_dir()
        .map(|dir| dir.join("chatgpt"))
        .map_err(|error| format!("Silo could not locate its application storage: {error}"))
}

/// The last status this process settled on or reported. Status reads that must stay
/// cheap (every desktop state read) use this; verifying the app tree the first time
/// reads every byte (seconds), so only `compute_status` and `refresh_status_blocking`
/// may do that, off the main thread.
static CACHE: Mutex<Option<Status>> = Mutex::new(None);
/// True while this process downloads or extracts the app.
static PREPARING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn set_cache(status: &Status) {
    *CACHE.lock().unwrap_or_else(|p| p.into_inner()) = Some(status.clone());
}

#[cfg(test)]
thread_local! {
    static TEST_CACHE: std::cell::RefCell<Option<Status>> = const { std::cell::RefCell::new(None) };
}

/// Pins the status `cached_status` reports on this thread (tests of status mapping).
#[cfg(test)]
pub(crate) fn set_test_cache(status: Option<Status>) {
    TEST_CACHE.with(|slot| *slot.borrow_mut() = status);
}

/// The cached status, or `None` before the first check.
pub(crate) fn cached_status() -> Option<Status> {
    #[cfg(test)]
    if let Some(status) = TEST_CACHE.with(|slot| slot.borrow().clone()) {
        return Some(status);
    }
    CACHE.lock().unwrap_or_else(|p| p.into_inner()).clone()
}

/// The `chatgpt-app-status` payload: the status plus the `device` it describes, `null`
/// for this device or the owning device's id. Listeners that predate the field
/// ignore it.
pub(crate) fn event_payload(status: serde_json::Value, device: Option<&str>) -> serde_json::Value {
    let mut payload = status;
    if let Some(object) = payload.as_object_mut() {
        object.insert(
            "device".into(),
            device.map_or(serde_json::Value::Null, Into::into),
        );
    }
    payload
}

fn emit_local(app: &tauri::AppHandle, status: &Status) {
    use tauri::Emitter;
    if let Ok(value) = serde_json::to_value(status) {
        let _ = app.emit(STATUS_EVENT, event_payload(value, None));
    }
}

fn publish(app: &tauri::AppHandle, status: Status) {
    set_cache(&status);
    emit_local(app, &status);
}

fn in_progress(status: &Status) -> bool {
    matches!(
        status,
        Status::Downloading { .. } | Status::Verifying | Status::Extracting
    )
}

/// Status from disk (verifies the tree: slow the first time in a process).
fn compute_status(app: &tauri::AppHandle) -> Result<Status, String> {
    if let Some(status) = cached_status().filter(|status| in_progress(status)) {
        if PREPARING.load(Ordering::SeqCst) {
            return Ok(status);
        }
    }
    let root = storage_root(app)?;
    let lock = Lock::bundled().map_err(|e| e.message)?;
    let arch = DebArch::host().map_err(|e| e.message)?;
    let status = match current_status(&root, &lock, arch) {
        Status::Idle => cached_status()
            .filter(|s| matches!(s, Status::Failed { .. }))
            .unwrap_or(Status::Idle),
        other => other,
    };
    set_cache(&status);
    Ok(status)
}

/// Recomputes and reports the status (blocking; startup and after changes).
pub(crate) fn refresh_status_blocking(app: &tauri::AppHandle) -> Status {
    let status = compute_status(app).unwrap_or(Status::Idle);
    emit_local(app, &status);
    status
}

/// Removes published versions other than the pinned one, unless a computer runs (a running
/// guest may still use the previous version until it next syncs).
///
/// The check and the removal happen under the device-wide operation gate that every
/// Computer start, restore and resume also takes, so no computer can begin booting from a version
/// between the inventory and the deletion. Collection is skipped, not queued, while
/// any operation runs; the next start or prepare tries again.
///
/// Collection never waits for the storage lock either: a download or extraction can hold
/// it for minutes, and waiting while holding the device-wide gate would stall every
/// lifecycle operation and Quit behind it.
///
/// A skipped collection (an operation or computer running, or the storage busy) stays
/// pending and is retried every `COLLECTION_RETRY` until it ran.
pub(crate) fn collect_unused(app: &tauri::AppHandle) {
    if COLLECTION.note(collect_unused_once(app)) {
        let app = app.clone();
        let _ = std::thread::Builder::new()
            .name("chatgpt-app-gc".into())
            .spawn(move || {
                COLLECTION.run_retries(
                    || collect_unused_once(&app),
                    || std::thread::sleep(COLLECTION_RETRY),
                )
            });
    }
}

fn collect_unused_once(app: &tauri::AppHandle) -> bool {
    let Ok(root) = storage_root(app) else {
        return true;
    };
    let (Ok(lock), Ok(arch)) = (Lock::bundled(), DebArch::host()) else {
        return true;
    };
    collect_unused_gated(&crate::runtime::OPERATIONS, &root, &lock, arch, || {
        crate::runtime::update_recovery::running_names(app).is_ok_and(|names| names.is_empty())
    })
}

const COLLECTION_RETRY: Duration = Duration::from_secs(120);
static COLLECTION: Maintenance = Maintenance::new();

/// Work that was skipped because the device was busy and must run later.
struct Maintenance {
    pending: AtomicBool,
    retrying: AtomicBool,
}

impl Maintenance {
    const fn new() -> Self {
        Self {
            pending: AtomicBool::new(false),
            retrying: AtomicBool::new(false),
        }
    }

    /// Records the outcome of a pass; true when the caller must start the retry loop
    /// (work is pending and no loop runs yet).
    fn note(&self, done: bool) -> bool {
        self.pending.store(!done, Ordering::SeqCst);
        !done && !self.retrying.swap(true, Ordering::SeqCst)
    }

    /// The retry loop: waits, tries, repeats until the work ran.
    fn run_retries(&self, mut attempt: impl FnMut() -> bool, mut wait: impl FnMut()) {
        loop {
            while self.pending.load(Ordering::SeqCst) {
                wait();
                if attempt() {
                    self.pending.store(false, Ordering::SeqCst);
                }
            }
            self.retrying.store(false, Ordering::SeqCst);
            // A pass that was skipped between the last attempt and now started no loop.
            if !self.pending.load(Ordering::SeqCst) || self.retrying.swap(true, Ordering::SeqCst) {
                return;
            }
        }
    }
}

/// Collects garbage while holding `gate` exclusively; `none_running` is evaluated
/// inside it. Returns whether collection ran.
fn collect_unused_gated(
    gate: &crate::runtime::operation_gate::OperationGate,
    root: &Path,
    lock: &Lock,
    arch: DebArch,
    none_running: impl FnOnce() -> bool,
) -> bool {
    let Ok(_gate) = gate.try_device_hidden("Removing unused ChatGPT app versions") else {
        return false;
    };
    if !none_running() {
        return false;
    }
    // Never wait for the storage lock inside the gate (see `collect_unused`).
    matches!(
        try_collect_garbage(root, lock, arch, &HashSet::new()),
        Ok(Some(_))
    )
}

/// Downloads and publishes the pinned app (blocking), reporting progress. Returns the
/// final status; a second concurrent call waits for the first. Only the background
/// worker (`auto`) calls it, so nothing else starts a download.
fn run_prepare(app: &tauri::AppHandle) -> Result<Status, String> {
    let root = storage_root(app)?;
    let lock = Lock::bundled().map_err(|e| e.message)?;
    let arch = DebArch::host().map_err(|e| e.message)?;
    // The folder computers mount: prepared again here in case the start-up attempt failed.
    // Preparation itself reports a real storage problem.
    let _ = crate::computer_use::register_published(&root);
    if PREPARING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        while PREPARING.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(500));
        }
        return compute_status(app);
    }
    struct Done;
    impl Drop for Done {
        fn drop(&mut self) {
            PREPARING.store(false, Ordering::SeqCst);
        }
    }
    let done = Done;
    publish(app, Status::Verifying);
    let announcer = app.clone();
    let report = move |status: Status| publish(&announcer, status);
    let status = match ensure(&root, &lock, arch, &HttpDownloader::default(), &report) {
        Ok(path) => Status::Ready {
            path,
            version: lock.version.clone(),
        },
        Err(error) => error.status(),
    };
    publish(app, status.clone());
    drop(done);
    Ok(status)
}

/// Makes sure the pinned app gets published, in the background and without blocking
/// anything: starts the worker, or wakes it when it waits to retry. Returns whether a
/// worker was started. Never needs the user: the app is downloaded on every device
/// that runs Silo.
pub(crate) fn ensure_in_background(app: &tauri::AppHandle) -> bool {
    let Some(claim) = auto::WORKER.claim(&auto::RETRY) else {
        return false;
    };
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("chatgpt-app".into())
        .spawn(move || {
            auto::lower_priority();
            let ready_app = app.clone();
            claim.run(&auto::RETRY, || {
                auto::settle(
                    || {
                        run_prepare(&app).unwrap_or_else(|error| Status::Failed {
                            reason: error,
                            retryable: true,
                        })
                    },
                    |delay| auto::RETRY.wait(delay),
                    || {
                        // Running built-in computers set computer use up now instead of at their next boot.
                        crate::computer_use::app_ready(&ready_app);
                        collect_unused(&ready_app);
                    },
                )
            });
        })
        .is_ok();
    spawned
}

pub(crate) fn local_status(app: &tauri::AppHandle) -> Result<Status, String> {
    compute_status(app)
}

/// "Retry" (also what a remote controller asks for): wakes a waiting worker or starts a
/// new one after a failure that is not retried by itself. Returns the status at once;
/// progress arrives as `chatgpt-app-status` events.
pub(crate) fn retry_now(app: &tauri::AppHandle) -> Result<Status, String> {
    let status = compute_status(app)?;
    if !matches!(status, Status::Ready { .. }) {
        ensure_in_background(app);
    }
    Ok(cached_status().unwrap_or(status))
}

/// App start: reads the status (the first digest of a process reads every byte), then
/// prepares the app in the background when the pinned version is not published.
pub(crate) fn start_automatic(app: &tauri::AppHandle) {
    let status = refresh_status_blocking(app);
    collect_unused(app);
    if matches!(status, Status::Ready { .. }) {
        // Nobody else syncs the running computers now (the worker that does it when the app
        // becomes ready has nothing to wait for): finish approval changes that were
        // saved but never launched before the last quit.
        crate::computer_use::reconcile(app);
        return;
    }
    // Let the app finish starting first; this is not urgent.
    std::thread::sleep(auto::START_DELAY);
    ensure_in_background(app);
}

/// The remote device a command addresses: `None` for this device, else its device id.
fn remote_device(device: Option<&str>) -> Result<Option<String>, String> {
    match device {
        None | Some("") => Ok(None),
        Some(device) if uuid::Uuid::parse_str(device).is_ok() => Ok(Some(device.to_owned())),
        Some(_) => Err("Invalid device.".into()),
    }
}

/// Calls the device that owns the computer. An older Silo there does not serve these
/// methods: say so instead of the generic remote error.
pub(crate) fn call_owner(
    app: &tauri::AppHandle,
    device: &str,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    crate::remote::call_remote_typed(app, device, method, params).map_err(owner_error)
}

const UPDATE_OWNER: &str = "Update Silo on that device to use computer use.";

/// The message for a failed call to the owning device.
fn owner_error(error: crate::bridge_error::BridgeError) -> String {
    if error.code == crate::bridge_error::ErrorCode::UnsupportedRemoteOperation {
        UPDATE_OWNER.to_owned()
    } else {
        error.message
    }
}

fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> impl std::future::Future<Output = Result<T, String>> {
    let task = tauri::async_runtime::spawn_blocking(work);
    async move {
        task.await
            .map_err(|_| "The ChatGPT app task stopped unexpectedly.".to_owned())?
    }
}

fn to_value(status: Status) -> Result<serde_json::Value, String> {
    serde_json::to_value(status).map_err(|_| "Could not encode the ChatGPT app status.".into())
}

/// Whether an owner serves the current integration. A consent-era Silo (it still answers
/// `chatgpt.accept` and `chatgpt.prepare`) has no `chatgpt.retry`, and its `idle` means
/// "consent given, nothing downloaded" with nothing that would start the download, so
/// its statuses must not be shown as if the new automatic worker produced them.
fn owner_is_current(capabilities: &[String]) -> bool {
    capabilities.iter().any(|name| name == "chatgpt.retry")
}

/// A remote owner's status: `unknown` for an owner that is not current, else what it
/// reports. Capabilities that cannot be read are a real failure (offline, disconnected).
fn owner_status(
    capabilities: Result<Vec<String>, String>,
    status: impl FnOnce() -> Result<serde_json::Value, String>,
) -> Result<serde_json::Value, String> {
    if !owner_is_current(&capabilities?) {
        return Ok(serde_json::json!({"state": "unknown"}));
    }
    remote_status(status())
}

/// A remote device's status as the UI shows it: one running a Silo without computer use
/// has no status to report, which is `unknown`, not an error. Real failures (offline,
/// disconnected) stay errors.
fn remote_status(result: Result<serde_json::Value, String>) -> Result<serde_json::Value, String> {
    match result {
        Err(error) if error == UPDATE_OWNER => Ok(serde_json::json!({"state": "unknown"})),
        other => other,
    }
}

/// The ChatGPT app status of this device, or of the remote device `device` (its
/// device id). A remote device running a Silo without computer use answers `unknown`.
#[tauri::command]
pub(crate) async fn chatgpt_app_status(
    app: tauri::AppHandle,
    device: Option<String>,
) -> Result<serde_json::Value, String> {
    blocking(move || match remote_device(device.as_deref())? {
        Some(device) => owner_status(
            crate::remote::device_capabilities(&app, &device).map_err(owner_error),
            || call_owner(&app, &device, "chatgpt.status", serde_json::json!({})),
        ),
        None => to_value(local_status(&app)?),
    })
    .await
}

/// Asks a device to try the download again now. It prepares its own copy; this only
/// wakes it. Resolves with its status at once, progress follows from the status reads.
#[tauri::command]
pub(crate) async fn chatgpt_app_retry(
    app: tauri::AppHandle,
    device: Option<String>,
) -> Result<serde_json::Value, String> {
    blocking(move || match remote_device(device.as_deref())? {
        Some(device) => call_owner(&app, &device, "chatgpt.retry", serde_json::json!({})),
        None => to_value(retry_now(&app)?),
    })
    .await
}

// ---------------------------------------------------------------- tests

mod auto;
#[cfg(test)]
mod tests;

#[cfg(all(test, unix))]
mod tool_timeout_tests {
    use super::*;

    #[test]
    fn a_tool_that_does_not_finish_is_stopped() {
        let started = Instant::now();
        let result =
            output_with_timeout(Command::new("sleep").arg("30"), Duration::from_millis(200));
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_tool_that_finishes_returns_its_output() {
        let output =
            output_with_timeout(Command::new("echo").arg("listing"), TOOL_TIMEOUT).unwrap();
        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "listing");
    }
}
