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
}

/// A request file, parsed or refused.
pub struct Taken {
    pub id: String,
    pub frame: Result<GuestRequestFrame, String>,
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
fn sub(parent: &OwnedFd, name: &str, create: Option<u16>) -> Result<OwnedFd, String> {
    if let Some(mode) = create {
        match mkdirat(parent, name, Mode::from_raw_mode(mode)) {
            Ok(()) | Err(rustix::io::Errno::EXIST) => {}
            Err(error) => return Err(io(error)),
        }
    }
    let fd = openat(parent, name, dir_flags(), Mode::empty())
        .map_err(|_| format!("{name} in the bridge folder is not a real folder."))?;
    if let Some(mode) = create {
        fchmod(&fd, Mode::from_raw_mode(mode)).map_err(io)?;
    }
    Ok(fd)
}

struct Dirs {
    req: OwnedFd,
    resp: OwnedFd,
}

impl Spool {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
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
        let root = openat(CWD, runtime, dir_flags(), Mode::empty())
            .map_err(|error| format!("Cannot open the runtime folder: {error}"))?;
        let bridge = sub(&root, name, create.then_some(0o755))?;
        // The guest writes requests it cannot list (sticky, no read); answers are named by the
        // request's random id, in a folder it can enter but not list.
        Ok(Dirs {
            req: sub(&bridge, "req", create.then_some(0o1733))?,
            resp: sub(&bridge, "resp", create.then_some(0o711))?,
        })
    }

    /// Creates `silo-bridge/{req,resp}` when missing with their modes. Each must be a real folder.
    pub fn ensure(&self) -> Result<(), String> {
        self.open(true).map(|_| ())
    }

    fn name_id(name: &str) -> Option<String> {
        let id = name.strip_suffix(".json")?;
        valid_id(id).then(|| id.to_owned())
    }

    fn count(resp: &OwnedFd) -> usize {
        let Ok(dir) = Dir::read_from(resp) else {
            return MAX_WAITING;
        };
        dir.flatten()
            .filter(|entry| entry.file_name().to_bytes().ends_with(b".json"))
            .take(MAX_WAITING + 1)
            .count()
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
        for entry in listing.flatten().take(MAX_PER_PASS + 2) {
            let bytes = entry.file_name().to_bytes();
            if bytes == b"." || bytes == b".." {
                continue;
            }
            let name = String::from_utf8_lossy(bytes).into_owned();
            let id = Self::name_id(&name);
            let (Some(id), false) = (id, full) else {
                // Not a request, or no room for its answer: removed without a reply. A
                // `.tmp` being written is left to finish.
                if !name.ends_with(".json.tmp") {
                    let _ = unlinkat(&dirs.req, name.as_str(), AtFlags::empty());
                }
                continue;
            };
            let frame = read_frame(&dirs.req, &name).and_then(|frame| {
                if frame.id == id {
                    Ok(frame)
                } else {
                    Err("The request id does not match its file name.".into())
                }
            });
            let _ = unlinkat(&dirs.req, name.as_str(), AtFlags::empty());
            taken.push(Taken { id, frame });
        }
        taken
    }

    /// Writes the answer atomically, readable by the owner only.
    pub fn respond(&self, frame: &GuestResponseFrame) -> Result<(), String> {
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
            let ours = name.strip_suffix(".json").or_else(|| name.strip_suffix(".tmp"));
            let expired = statat(&dirs.resp, name.as_str(), AtFlags::SYMLINK_NOFOLLOW)
                .map(|stat| {
                    let modified = SystemTime::UNIX_EPOCH
                        + Duration::from_secs(u64::try_from(stat.st_mtime).unwrap_or(0));
                    now.duration_since(modified).is_ok_and(|age| age > RESPONSE_TTL)
                })
                .unwrap_or(true);
            // Anything that is not an answer of ours goes at once; an answer goes when old.
            if ours.is_none_or(|id| !valid_id(id)) || expired {
                let _ = unlinkat(&dirs.resp, name.as_str(), AtFlags::empty());
            }
        }
    }
}

fn read_frame(req: &OwnedFd, name: &str) -> Result<GuestRequestFrame, String> {
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
    Ok(frame)
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
    fn folders_get_their_modes_and_a_request_is_taken_once_and_answered_privately() {
        let (_dir, spool) = spool();
        let mode = |part: &str| {
            fs::metadata(spool.dir.join(part)).unwrap().permissions().mode() & 0o7777
        };
        assert_eq!((mode("req"), mode("resp")), (0o1733, 0o711));
        fs::write(spool.dir.join("req").join(format!("{ID}.json")), request(ID)).unwrap();
        let taken = spool.take();
        assert_eq!(taken.len(), 1);
        assert_eq!(taken[0].frame.as_ref().unwrap().method, "computers.list");
        assert!(spool.take().is_empty());
        spool.respond(&reply(ID)).unwrap();
        let answer = spool.dir.join("resp").join(format!("{ID}.json"));
        assert_eq!(fs::metadata(answer).unwrap().permissions().mode() & 0o777, 0o600);
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
        fs::write(req.join(format!("{big}.json")), vec![b' '; MAX_FRAME_BYTES + 1]).unwrap();
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
        spool.respond(&huge).unwrap();
        let written = fs::read_to_string(spool.dir.join("resp").join(format!("{ID}.json"))).unwrap();
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
            assert!(spool.respond(&reply(ID)).is_err(), "{linked}");
            assert!(spool.ensure().is_err(), "{linked}");
            spool.sweep(SystemTime::now() + RESPONSE_TTL * 2);
            assert!(outside.join("req").join(format!("{ID}.json")).exists(), "{linked}");
            assert!(outside.join("resp").join("keep.me").exists(), "{linked}");
            assert!(!outside.join("resp").join(format!("{ID}.json")).exists(), "{linked}");
        }
    }

    #[test]
    fn a_full_answer_folder_drops_new_requests_and_sweep_clears_every_old_answer() {
        let (_dir, spool) = spool();
        for number in 0..MAX_WAITING {
            let id = format!("{number:032x}");
            fs::write(spool.dir.join("resp").join(format!("{id}.json")), "{}").unwrap();
        }
        fs::write(spool.dir.join("req").join(format!("{ID}.json")), request(ID)).unwrap();
        assert!(spool.take().is_empty());
        assert!(!spool.dir.join("req").join(format!("{ID}.json")).exists());
        fs::write(spool.dir.join("resp").join("junk"), "x").unwrap();
        spool.sweep(SystemTime::now() + RESPONSE_TTL * 2);
        assert_eq!(fs::read_dir(spool.dir.join("resp")).unwrap().count(), 0);
    }
}
