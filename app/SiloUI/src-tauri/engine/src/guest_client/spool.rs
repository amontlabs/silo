//! The bridge folder: the runtime's per-computer share that the guest sees as `/.msb`. The
//! guest writes request files under `req/`; the host takes each one, answers under `resp/`.
//!
//! The guest controls everything below the runtime folder, including replacing `silo-bridge`,
//! `req` or `resp` with links. Every step therefore goes through directory descriptors
//! (`rustix`: `openat` with `O_NOFOLLOW`, `unlinkat`, `renameat`, `statat` without following
//! links), opened afresh in each pass, and every file is checked after it is opened.

use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use rustix::{
    fd::OwnedFd,
    fs::{
        fchmod, fstat, mkdirat, openat, renameat, statat, unlinkat, AtFlags, Dir, FileType, Mode,
        OFlags, CWD,
    },
};
use crate::containers::idmap::KeepId;
use silo_api::methods::guest_client::{
    GuestErrorBody, GuestRequestFrame, GuestResponseFrame, MAX_FRAME_BYTES,
};

/// The most directory entries looked at in one pass of `req/`.
pub const MAX_PER_PASS: usize = 32;
/// The most answers that may wait unread; above it new requests are dropped unanswered.
pub const MAX_WAITING: usize = 64;
/// How long an unread response stays.
pub const RESPONSE_TTL: Duration = Duration::from_secs(600);
const SWEEP_LIMIT: usize = 4096;
/// The most directory entries examined in one look at `req/` or `resp/`; more than this is
/// backpressure, not something to read on.
pub const SCAN_LIMIT: usize = 256;
/// A request being written (`<id>.json.tmp`) older than this is abandoned and removed.
pub const TMP_AGE: Duration = Duration::from_secs(10);

/// The runtime's stat virtualization: the guest sees the owner, group and type this 20-byte
/// attribute names instead of the host file's (`user.msb.override_stat`, version 1, in
/// MicroSandbox's `passthroughfs/stat_override.rs`).
const OVERRIDE_KEY: &std::ffi::CStr = c"user.msb.override_stat";
const S_IFREG: u32 = 0o100_000;
const S_IFDIR: u32 = 0o040_000;

fn override_bytes(uid: u32, gid: u32, mode: u32) -> [u8; 20] {
    let mut bytes = [0u8; 20];
    bytes[0] = 1;
    bytes[4..8].copy_from_slice(&uid.to_ne_bytes());
    bytes[8..12].copy_from_slice(&gid.to_ne_bytes());
    bytes[12..16].copy_from_slice(&mode.to_ne_bytes());
    bytes
}

/// The owner a guest user's file carries in its override, when it has one.
fn read_owner(fd: &OwnedFd) -> Option<(u32, u32)> {
    read_override(fd).map(|(uid, gid, _)| (uid, gid))
}

/// The owner, group and mode an override names, when the file has one.
fn read_override(fd: &OwnedFd) -> Option<(u32, u32, u32)> {
    use rustix::fd::AsRawFd;
    let mut buf = [0u8; 20];
    // SAFETY: the buffer is 20 bytes and the descriptor is open.
    #[cfg(target_os = "macos")]
    let read = unsafe {
        libc::fgetxattr(
            fd.as_raw_fd(),
            OVERRIDE_KEY.as_ptr(),
            buf.as_mut_ptr().cast(),
            20,
            0,
            0,
        )
    };
    #[cfg(not(target_os = "macos"))]
    let read = unsafe {
        libc::fgetxattr(
            fd.as_raw_fd(),
            OVERRIDE_KEY.as_ptr(),
            buf.as_mut_ptr().cast(),
            20,
        )
    };
    if read != 20 || buf[0] != 1 {
        return None;
    }
    let word = |at: usize| u32::from_ne_bytes([buf[at], buf[at + 1], buf[at + 2], buf[at + 3]]);
    Some((word(4), word(8), word(12)))
}

/// Makes the guest see `fd` as owned by `uid:gid` with `mode`. Best effort: a host file system
/// without extended attributes keeps the runtime's default owner.
fn write_override(fd: &OwnedFd, uid: u32, gid: u32, mode: u32) {
    use rustix::fd::AsRawFd;
    let bytes = override_bytes(uid, gid, mode);
    // SAFETY: the value is 20 bytes and the descriptor is open.
    #[cfg(target_os = "macos")]
    unsafe {
        libc::fsetxattr(
            fd.as_raw_fd(),
            OVERRIDE_KEY.as_ptr(),
            bytes.as_ptr().cast(),
            20,
            0,
            0,
        );
    }
    #[cfg(not(target_os = "macos"))]
    unsafe {
        libc::fsetxattr(
            fd.as_raw_fd(),
            OVERRIDE_KEY.as_ptr(),
            bytes.as_ptr().cast(),
            20,
            0,
        );
    }
}

/// The host path of a computer's bridge folder.
pub fn spool_dir(runtime_home: &Path, computer_name: &str) -> PathBuf {
    runtime_home
        .join("sandboxes")
        .join(computer_name)
        .join("runtime")
        .join("silo-bridge")
}

pub struct Spool {
    dir: PathBuf,
    /// Set for a container computer, whose files carry real owners: the engine's own user is the
    /// working account there (see `containers::idmap`), so the owner of a request comes from its
    /// file's, and nothing is virtualized.
    keep_id: Option<KeepId>,
}

/// A request file, parsed or refused.
pub struct Taken {
    pub id: String,
    pub frame: Result<GuestRequestFrame, String>,
    /// The guest user that wrote the request, when the share says so: the answer is made
    /// readable to that user.
    pub owner: Option<(u32, u32)>,
}

pub fn valid_id(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn io(error: rustix::io::Errno) -> String {
    format!("Cannot use the bridge folder: {error}")
}

fn dir_flags() -> OFlags {
    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC
}

/// Opens (creating, with `mode`, when `create`) a real directory below `parent`.
fn sub(
    parent: &OwnedFd,
    name: &str,
    create: Option<u16>,
    keep_id: Option<KeepId>,
) -> Result<OwnedFd, String> {
    if let Some(mode) = create {
        match mkdirat(parent, name, Mode::from_bits_truncate(mode.into())) {
            Ok(()) | Err(rustix::io::Errno::EXIST) => {}
            Err(error) => return Err(io(error)),
        }
    }
    let fd = openat(parent, name, dir_flags(), Mode::empty())
        .map_err(|_| format!("{name} in the bridge folder is not a real folder."))?;
    if let Some(mode) = create {
        fchmod(&fd, Mode::from_bits_truncate(mode.into())).map_err(io)?;
        // The guest sees these folders as root's, so its sticky bit and modes mean what they say.
        // (A container sees the real owner and mode: nothing to override.)
        if keep_id.is_none() {
            write_override(&fd, 0, 0, S_IFDIR | u32::from(mode));
        }
    }
    Ok(fd)
}

/// The most runtime entries sealed in one pass.
const SEAL_LIMIT: usize = 256;

/// A container's runtime folder: the account that owns it (the working account) writes the bridge
/// in it, and every other user of the container may only search it, as in a virtual machine.
fn seal_container_runtime(root: &OwnedFd) {
    let _ = fchmod(root, Mode::from_bits_truncate(0o711));
}

/// The runtime folder is `0700` on the host and the guest sees it as that user's, so a guest user
/// other than root could not reach `silo-bridge`. Show it as root's with search permission only,
/// and show every other entry in it as root's and private (`0700` folders, `0600` files), so the
/// search permission reaches nothing but the bridge. MicroSandbox writes files here with the host
/// umask (`root-disk.json` is `0666`) and replaces them, so this runs again whenever it may have.
fn seal_runtime(root: &OwnedFd) {
    write_override(root, 0, 0, S_IFDIR | 0o711);
    let Ok(listing) = Dir::read_from(root) else {
        return;
    };
    for entry in listing.flatten().take(SEAL_LIMIT) {
        let bytes = entry.file_name().to_bytes();
        if matches!(bytes, b"." | b"..") || bytes == BRIDGE_NAME.as_bytes() {
            continue;
        }
        let Ok(fd) = openat(
            root,
            entry.file_name(),
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        ) else {
            continue;
        };
        match fstat(&fd).map(|stat| FileType::from_raw_mode(stat.st_mode)) {
            Ok(FileType::Directory) => write_override(&fd, 0, 0, S_IFDIR | 0o700),
            Ok(FileType::RegularFile) => write_override(&fd, 0, 0, S_IFREG | 0o600),
            _ => {}
        }
    }
}

const BRIDGE_NAME: &str = "silo-bridge";

struct Dirs {
    req: OwnedFd,
    resp: OwnedFd,
}

impl Spool {
    pub fn new(dir: PathBuf) -> Self {
        let keep_id = crate::containers::keep_id_for_bridge(&dir);
        Self { dir, keep_id }
    }

    fn seal_root(&self, root: &OwnedFd) {
        match self.keep_id {
            Some(_) => seal_container_runtime(root),
            None => seal_runtime(root),
        }
    }

    fn open(&self, create: bool) -> Result<Dirs, String> {
        let runtime = self
            .dir
            .parent()
            .ok_or("The bridge folder has no runtime folder.")?;
        let name = self
            .dir
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("The bridge folder has no name.")?;
        if create && !runtime.exists() {
            // A computer that has not started yet has no runtime folder; the runtime accepts it.
            std::fs::create_dir_all(runtime)
                .map_err(|error| format!("Cannot create the runtime folder: {error}"))?;
        }
        let root = openat(CWD, runtime, dir_flags(), Mode::empty())
            .map_err(|error| format!("Cannot open the runtime folder: {error}"))?;
        if create {
            self.seal_root(&root);
        }
        let bridge = sub(&root, name, create.then_some(0o755), self.keep_id)?;
        // The guest writes requests it cannot list (sticky, no read); answers are named by the
        // request's random id, in a folder it can enter but not list.
        Ok(Dirs {
            req: sub(&bridge, "req", create.then_some(0o1733), self.keep_id)?,
            resp: sub(&bridge, "resp", create.then_some(0o711), self.keep_id)?,
        })
    }

    /// Shows the runtime folder's other entries as private to root again. Cheap; the worker runs
    /// it every pass, since MicroSandbox recreates the folder and rewrites its files at start,
    /// restore and rename. Does nothing for a runtime folder that does not exist.
    pub fn seal(&self) {
        let Some(runtime) = self.dir.parent() else {
            return;
        };
        if let Ok(root) = openat(CWD, runtime, dir_flags(), Mode::empty()) {
            self.seal_root(&root);
        }
    }

    /// Creates `silo-bridge/{req,resp}` when missing with their modes. Each must be a real folder.
    pub fn ensure(&self) -> Result<(), String> {
        self.open(true).map(|_| ())
    }

    /// Whether `req/` holds anything at all (a cheap look, before the engine is asked anything).
    pub fn has_work(&self) -> bool {
        self.open(false)
            .ok()
            .and_then(|dirs| Dir::read_from(&dirs.req).ok())
            .is_some_and(|listing| {
                listing
                    .flatten()
                    .any(|entry| !matches!(entry.file_name().to_bytes(), b"." | b".."))
            })
    }

    fn name_id(name: &str) -> Option<String> {
        let id = name.strip_suffix(".json")?;
        valid_id(id).then(|| id.to_owned())
    }

    fn count(resp: &OwnedFd) -> usize {
        let Ok(dir) = Dir::read_from(resp) else {
            return MAX_WAITING;
        };
        // Looks at a bounded number of entries; running out of them counts as full.
        let (mut seen, mut waiting) = (0usize, 0usize);
        for entry in dir.flatten() {
            seen += 1;
            if seen > SCAN_LIMIT {
                return MAX_WAITING;
            }
            if entry.file_name().to_bytes().ends_with(b".json") {
                waiting += 1;
            }
        }
        waiting
    }

    /// Takes request files, removing each one first. Looks at no more than [`MAX_PER_PASS`]
    /// entries; anything that is not a well-named request is removed. While [`MAX_WAITING`]
    /// answers wait unread, requests are removed unanswered.
    pub fn take(&self) -> Vec<Taken> {
        let Ok(dirs) = self.open(false) else {
            return Vec::new();
        };
        let Ok(listing) = Dir::read_from(&dirs.req) else {
            return Vec::new();
        };
        let full = Self::count(&dirs.resp) >= MAX_WAITING;
        let mut taken = Vec::new();
        let mut examined = 0usize;
        for entry in listing.flatten() {
            let bytes = entry.file_name().to_bytes();
            if bytes == b"." || bytes == b".." {
                continue;
            }
            // Every entry counts against the budget; what is left waits for the next pass.
            examined += 1;
            if examined > SCAN_LIMIT || taken.len() >= MAX_PER_PASS {
                break;
            }
            let name = String::from_utf8_lossy(bytes).into_owned();
            let id = Self::name_id(&name);
            let (Some(id), false) = (id, full) else {
                // Not a request, or no room for its answer: removed without a reply. A
                // `.tmp` being written is left to finish unless it was abandoned.
                let writing = name.ends_with(".json.tmp") && !tmp_abandoned(&dirs.req, &name);
                if !writing {
                    let _ = unlinkat(&dirs.req, name.as_str(), AtFlags::empty());
                }
                continue;
            };
            let (frame, owner) = match read_frame(&dirs.req, &name, self.keep_id) {
                Ok((frame, owner)) if frame.id == id => (Ok(frame), owner),
                Ok((_, owner)) => (
                    Err("The request id does not match its file name.".to_owned()),
                    owner,
                ),
                Err(error) => (Err(error), None),
            };
            let _ = unlinkat(&dirs.req, name.as_str(), AtFlags::empty());
            taken.push(Taken { id, frame, owner });
        }
        taken
    }

    /// Writes the answer atomically, readable by the owner only.
    pub fn respond(
        &self,
        frame: &GuestResponseFrame,
        owner: Option<(u32, u32)>,
    ) -> Result<(), String> {
        if !valid_id(&frame.id) {
            return Err("Not a request id.".into());
        }
        let mut bytes = serde_json::to_vec(frame).map_err(|error| error.to_string())?;
        if bytes.len() > MAX_FRAME_BYTES {
            bytes = serde_json::to_vec(&GuestResponseFrame {
                v: 1,
                id: frame.id.clone(),
                ok: false,
                result: None,
                error: Some(GuestErrorBody {
                    code: "too_large".into(),
                    message: "The answer is larger than the bridge allows.".into(),
                }),
            })
            .map_err(|error| error.to_string())?;
        }
        let dirs = self.open(false)?;
        let (target, temporary) = (format!("{}.json", frame.id), format!("{}.tmp", frame.id));
        let _ = unlinkat(&dirs.resp, temporary.as_str(), AtFlags::empty());
        let fd = openat(
            &dirs.resp,
            temporary.as_str(),
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o600),
        )
        .map_err(io)?;
        if let (Some((uid, gid)), None) = (owner, self.keep_id) {
            // Mode 0600 for the user that asked, so no other guest user reads the answer.
            write_override(&fd, uid, gid, S_IFREG | 0o600);
        }
        std::io::Write::write_all(&mut File::from(fd), &bytes)
            .map_err(|error| format!("Cannot write the answer: {error}"))?;
        renameat(&dirs.resp, temporary.as_str(), &dirs.resp, target.as_str()).map_err(io)
    }

    /// Removes answers nobody read (and the half-written ones), however many there are.
    pub fn sweep(&self, now: SystemTime) {
        let Ok(dirs) = self.open(false) else {
            return;
        };
        let Ok(listing) = Dir::read_from(&dirs.resp) else {
            return;
        };
        for entry in listing.flatten().take(SWEEP_LIMIT) {
            let name = String::from_utf8_lossy(entry.file_name().to_bytes()).into_owned();
            if name == "." || name == ".." {
                continue;
            }
            let ours = name
                .strip_suffix(".json")
                .or_else(|| name.strip_suffix(".tmp"));
            let expired = statat(&dirs.resp, name.as_str(), AtFlags::SYMLINK_NOFOLLOW)
                .map(|stat| {
                    let modified = SystemTime::UNIX_EPOCH
                        + Duration::from_secs(u64::try_from(stat.st_mtime).unwrap_or(0));
                    now.duration_since(modified)
                        .is_ok_and(|age| age > RESPONSE_TTL)
                })
                .unwrap_or(true);
            // Anything that is not an answer of ours goes at once; an answer goes when old.
            if ours.is_none_or(|id| !valid_id(id)) || expired {
                let _ = unlinkat(&dirs.resp, name.as_str(), AtFlags::empty());
            }
        }
    }
}

/// Whether a half-written request has sat longer than [`TMP_AGE`].
fn tmp_abandoned(req: &OwnedFd, name: &str) -> bool {
    statat(req, name, AtFlags::SYMLINK_NOFOLLOW).is_ok_and(|stat| {
        let modified =
            SystemTime::UNIX_EPOCH + Duration::from_secs(u64::try_from(stat.st_mtime).unwrap_or(0));
        SystemTime::now()
            .duration_since(modified)
            .is_ok_and(|age| age > TMP_AGE)
    })
}

type Owner = Option<(u32, u32)>;

fn read_frame(
    req: &OwnedFd,
    name: &str,
    keep_id: Option<KeepId>,
) -> Result<(GuestRequestFrame, Owner), String> {
    let fd = openat(
        req,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| format!("The request cannot be opened: {error}"))?;
    let stat = fstat(&fd).map_err(io)?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile {
        return Err("The request is not a regular file.".into());
    }
    if usize::try_from(stat.st_size).unwrap_or(usize::MAX) > MAX_FRAME_BYTES {
        return Err("The request is larger than the bridge allows.".into());
    }
    // A container's files carry their real owners; a virtual machine's are virtualized.
    let owner = match keep_id {
        Some(map) => map.guest_owner(stat.st_uid, stat.st_gid),
        None => read_owner(&fd),
    };
    let mut bytes = Vec::new();
    File::from(fd)
        .take(MAX_FRAME_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > MAX_FRAME_BYTES {
        return Err("The request is larger than the bridge allows.".into());
    }
    let frame: GuestRequestFrame =
        serde_json::from_slice(&bytes).map_err(|error| format!("Not a request: {error}"))?;
    if frame.v != 1 {
        return Err("Unsupported bridge version.".into());
    }
    Ok((frame, owner))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    const ID: &str = "0123456789abcdef0123456789abcdef";

    fn spool() -> (tempfile::TempDir, Spool) {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("runtime")).unwrap();
        let spool = Spool::new(dir.path().join("runtime").join("silo-bridge"));
        spool.ensure().unwrap();
        (dir, spool)
    }

    fn request(id: &str) -> String {
        format!(
            r#"{{"v":1,"id":"{id}","token":"silo_gc_x","method":"computers.list","params":{{}}}}"#
        )
    }

    fn reply(id: &str) -> GuestResponseFrame {
        GuestResponseFrame {
            v: 1,
            id: id.into(),
            ok: true,
            result: Some(serde_json::json!({})),
            error: None,
        }
    }

    #[test]
    fn the_runtime_folder_is_shown_to_the_guest_as_roots_with_search_permission() {
        let (dir, _spool) = spool();
        let runtime = openat(CWD, dir.path().join("runtime"), dir_flags(), Mode::empty()).unwrap();
        // Skips on a file system without extended attributes, where nothing is overridden.
        if let Some(owner) = read_owner(&runtime) {
            assert_eq!(owner, (0, 0));
            let mut buf = [0u8; 20];
            use rustix::fd::AsRawFd;
            #[cfg(target_os = "macos")]
            let read = unsafe {
                libc::fgetxattr(
                    runtime.as_raw_fd(),
                    OVERRIDE_KEY.as_ptr(),
                    buf.as_mut_ptr().cast(),
                    20,
                    0,
                    0,
                )
            };
            #[cfg(not(target_os = "macos"))]
            let read = unsafe {
                libc::fgetxattr(
                    runtime.as_raw_fd(),
                    OVERRIDE_KEY.as_ptr(),
                    buf.as_mut_ptr().cast(),
                    20,
                )
            };
            assert_eq!(read, 20);
            assert_eq!(
                u32::from_ne_bytes(buf[12..16].try_into().unwrap()),
                S_IFDIR | 0o711
            );
        }
    }

    #[test]
    fn sealing_makes_every_other_runtime_entry_private_to_root_and_leaves_the_bridge() {
        let (dir, spool) = spool();
        let runtime = dir.path().join("runtime");
        fs::write(runtime.join("root-disk.json"), "{}").unwrap();
        fs::set_permissions(
            runtime.join("root-disk.json"),
            fs::Permissions::from_mode(0o666),
        )
        .unwrap();
        fs::create_dir(runtime.join("rootfs")).unwrap();
        std::os::unix::fs::symlink("/etc", runtime.join("link")).unwrap();
        spool.seal();
        let shown = |part: &str| {
            let fd = openat(CWD, runtime.join(part), OFlags::RDONLY, Mode::empty()).unwrap();
            read_override(&fd)
        };
        // Skips on a file system without extended attributes, where nothing is overridden.
        if shown("rootfs").is_none() {
            return;
        }
        assert_eq!(shown("root-disk.json"), Some((0, 0, S_IFREG | 0o600)));
        assert_eq!(shown("rootfs"), Some((0, 0, S_IFDIR | 0o700)));
        assert_eq!(shown("silo-bridge"), Some((0, 0, S_IFDIR | 0o755)));
        assert!(
            read_override(&openat(CWD, "/etc", OFlags::RDONLY, Mode::empty()).unwrap()).is_none()
        );
        // A file MicroSandbox replaced is sealed again by the next pass.
        fs::remove_file(runtime.join("root-disk.json")).unwrap();
        fs::write(runtime.join("root-disk.json"), "{}").unwrap();
        assert_eq!(shown("root-disk.json"), None);
        spool.seal();
        assert_eq!(shown("root-disk.json"), Some((0, 0, S_IFREG | 0o600)));
    }

    #[test]
    fn folders_get_their_modes_and_a_request_is_taken_once_and_answered_privately() {
        let (_dir, spool) = spool();
        let mode = |part: &str| {
            fs::metadata(spool.dir.join(part))
                .unwrap()
                .permissions()
                .mode()
                & 0o7777
        };
        assert_eq!((mode("req"), mode("resp")), (0o1733, 0o711));
        fs::write(
            spool.dir.join("req").join(format!("{ID}.json")),
            request(ID),
        )
        .unwrap();
        let taken = spool.take();
        assert_eq!(taken.len(), 1);
        assert_eq!(taken[0].frame.as_ref().unwrap().method, "computers.list");
        assert!(spool.take().is_empty());
        spool.respond(&reply(ID), None).unwrap();
        let answer = spool.dir.join("resp").join(format!("{ID}.json"));
        assert_eq!(
            fs::metadata(answer).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn bad_names_links_oversize_and_mismatched_ids_are_not_served_and_junk_is_removed() {
        let (_dir, spool) = spool();
        let req = spool.dir.join("req");
        fs::write(req.join("short.json"), request(ID)).unwrap();
        std::os::unix::fs::symlink("/etc/hosts", req.join(format!("{ID}.json"))).unwrap();
        let other = "fedcba9876543210fedcba9876543210";
        fs::write(req.join(format!("{other}.json")), request(ID)).unwrap();
        let big = "1111111111111111aaaaaaaaaaaaaaaa";
        fs::write(
            req.join(format!("{big}.json")),
            vec![b' '; MAX_FRAME_BYTES + 1],
        )
        .unwrap();
        let taken = spool.take();
        assert_eq!(taken.len(), 3);
        assert!(taken.iter().all(|item| item.frame.is_err()));
        assert!(!req.join("short.json").exists());
    }

    #[test]
    fn an_oversize_answer_is_replaced_by_an_error() {
        let (_dir, spool) = spool();
        let mut huge = reply(ID);
        huge.result = Some(serde_json::json!("x".repeat(MAX_FRAME_BYTES)));
        spool.respond(&huge, None).unwrap();
        let written =
            fs::read_to_string(spool.dir.join("resp").join(format!("{ID}.json"))).unwrap();
        assert!(written.contains("too_large"));
    }

    #[test]
    fn a_linked_req_resp_or_bridge_folder_is_never_followed() {
        for linked in ["req", "resp", "bridge"] {
            let (dir, spool) = spool();
            let outside = dir.path().join("outside");
            fs::create_dir_all(outside.join("req")).unwrap();
            fs::create_dir_all(outside.join("resp")).unwrap();
            fs::write(outside.join("req").join(format!("{ID}.json")), request(ID)).unwrap();
            fs::write(outside.join("resp").join("keep.me"), "x").unwrap();
            if linked == "bridge" {
                fs::remove_dir_all(&spool.dir).unwrap();
                std::os::unix::fs::symlink(&outside, &spool.dir).unwrap();
            } else {
                let path = spool.dir.join(linked);
                fs::remove_dir_all(&path).unwrap();
                std::os::unix::fs::symlink(outside.join(linked), &path).unwrap();
            }
            assert!(spool.take().is_empty(), "{linked}");
            assert!(spool.respond(&reply(ID), None).is_err(), "{linked}");
            assert!(spool.ensure().is_err(), "{linked}");
            spool.sweep(SystemTime::now() + RESPONSE_TTL * 2);
            assert!(
                outside.join("req").join(format!("{ID}.json")).exists(),
                "{linked}"
            );
            assert!(outside.join("resp").join("keep.me").exists(), "{linked}");
            assert!(
                !outside.join("resp").join(format!("{ID}.json")).exists(),
                "{linked}"
            );
        }
    }

    #[test]
    fn a_full_answer_folder_drops_new_requests_and_sweep_clears_every_old_answer() {
        let (_dir, spool) = spool();
        for number in 0..MAX_WAITING {
            let id = format!("{number:032x}");
            fs::write(spool.dir.join("resp").join(format!("{id}.json")), "{}").unwrap();
        }
        fs::write(
            spool.dir.join("req").join(format!("{ID}.json")),
            request(ID),
        )
        .unwrap();
        assert!(spool.take().is_empty());
        assert!(!spool.dir.join("req").join(format!("{ID}.json")).exists());
        fs::write(spool.dir.join("resp").join("junk"), "x").unwrap();
        spool.sweep(SystemTime::now() + RESPONSE_TTL * 2);
        assert_eq!(fs::read_dir(spool.dir.join("resp")).unwrap().count(), 0);
    }

    #[test]
    fn a_fresh_tmp_is_left_alone_an_old_one_goes_and_a_flood_of_entries_is_bounded() {
        let (_dir, spool) = spool();
        let req = spool.dir.join("req");
        let fresh = req.join(format!("{ID}.json.tmp"));
        fs::write(&fresh, "{").unwrap();
        assert!(spool.take().is_empty());
        assert!(fresh.exists());
        let old = SystemTime::now() - TMP_AGE * 3;
        File::options()
            .write(true)
            .open(&fresh)
            .unwrap()
            .set_modified(old)
            .unwrap();
        assert!(spool.take().is_empty());
        assert!(!fresh.exists());
        for number in 0..SCAN_LIMIT + 50 {
            fs::write(req.join(format!("junk{number}")), "x").unwrap();
        }
        spool.take();
        let left = fs::read_dir(&req).unwrap().count();
        assert!(left >= 50 && left < SCAN_LIMIT + 50, "{left}");
        // More entries in resp/ than a pass may look at is a full folder.
        for number in 0..SCAN_LIMIT + 1 {
            fs::write(spool.dir.join("resp").join(format!("x{number}")), "").unwrap();
        }
        fs::write(req.join(format!("{ID}.json")), request(ID)).unwrap();
        assert!(spool.take().is_empty());
    }

    #[test]
    fn a_containers_requests_are_owned_through_the_id_mapping_and_its_folders_are_sealed_by_mode() {
        let home = tempfile::tempdir().unwrap();
        crate::containers::capability::make_container(home.path(), "box");
        let spool = Spool::new(spool_dir(home.path(), "box"));
        assert!(spool.keep_id.is_some());
        // A virtual machine's (or an unknown) bridge folder is not a container's.
        assert!(Spool::new(spool_dir(home.path(), "vm")).keep_id.is_none());
        spool.ensure().unwrap();
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o7777;
        let runtime = spool.dir.parent().unwrap();
        assert_eq!(mode(runtime), 0o711);
        assert_eq!((mode(&spool.dir.join("req")), mode(&spool.dir.join("resp"))), (0o1733, 0o711));
        // The runtime folder is made private again if the guest's account loosened it.
        fs::set_permissions(runtime, fs::Permissions::from_mode(0o755)).unwrap();
        spool.seal();
        assert_eq!(mode(runtime), 0o711);
        fs::write(
            spool.dir.join("req").join(format!("{ID}.json")),
            request(ID),
        )
        .unwrap();
        let taken = spool.take();
        // The file is the engine's own user's, which is the working account in the container.
        assert_eq!(taken[0].owner, Some((1001, 1001)));
        spool.respond(&reply(ID), taken[0].owner).unwrap();
        let answer = spool.dir.join("resp").join(format!("{ID}.json"));
        assert_eq!(mode(&answer) & 0o777, 0o600);
    }

    #[test]
    fn the_override_names_the_owner_the_guest_sees() {
        let bytes = override_bytes(1000, 1001, S_IFREG | 0o600);
        assert_eq!(bytes[0], 1);
        assert_eq!(u32::from_ne_bytes(bytes[4..8].try_into().unwrap()), 1000);
        assert_eq!(u32::from_ne_bytes(bytes[8..12].try_into().unwrap()), 1001);
        // Round trip through a real file when the host file system keeps the attribute.
        let (dir, spool) = spool();
        let path = dir.path().join("probe");
        fs::write(&path, "x").unwrap();
        let fd = openat(CWD, &path, OFlags::RDONLY, Mode::empty()).unwrap();
        write_override(&fd, 1000, 1001, S_IFREG | 0o600);
        if let Some(owner) = read_owner(&fd) {
            assert_eq!(owner, (1000, 1001));
        }
        drop(spool);
    }
}
