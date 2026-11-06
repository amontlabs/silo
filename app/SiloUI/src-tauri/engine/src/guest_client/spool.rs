//! The bridge folder: the runtime's per-computer share that the guest sees as `/.msb`. The
//! guest writes request files under `req/`; the host takes each one, answers under `resp/`.
//! The guest controls everything in the folder, so every read is bounded and never follows a
//! link.

use std::{
    fs,
    io::Read,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use silo_api::methods::guest_client::{GuestRequestFrame, GuestResponseFrame, MAX_FRAME_BYTES};

/// The most request files taken in one pass.
pub const MAX_PER_PASS: usize = 32;
/// How long an unread response stays.
pub const RESPONSE_TTL: Duration = Duration::from_secs(600);

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

impl Spool {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// Creates `req/` and `resp/` when missing. The folder itself must be a real directory.
    pub fn ensure(&self) -> Result<(), String> {
        let io = |error: std::io::Error| format!("Cannot prepare the bridge folder: {error}");
        for part in ["req", "resp"] {
            let path = self.dir.join(part);
            match fs::symlink_metadata(&path) {
                Ok(meta) if meta.is_dir() => {}
                Ok(_) => {
                    return Err("The bridge folder holds something that is not a folder.".into())
                }
                Err(_) => fs::DirBuilder::new()
                    .recursive(true)
                    .mode(0o755)
                    .create(&path)
                    .map_err(io)?,
            }
        }
        Ok(())
    }

    fn name_id(name: &std::ffi::OsStr) -> Option<String> {
        let name = name.to_str()?;
        let id = name.strip_suffix(".json")?;
        valid_id(id).then(|| id.to_owned())
    }

    /// Takes up to [`MAX_PER_PASS`] request files, removing each one first.
    pub fn take(&self) -> Vec<Taken> {
        let Ok(entries) = fs::read_dir(self.dir.join("req")) else {
            return Vec::new();
        };
        let mut taken = Vec::new();
        for entry in entries.flatten() {
            if taken.len() >= MAX_PER_PASS {
                break;
            }
            let Some(id) = Self::name_id(&entry.file_name()) else {
                continue;
            };
            let path = entry.path();
            let frame = read_frame(&path).and_then(|frame| {
                if frame.id == id {
                    Ok(frame)
                } else {
                    Err("The request id does not match its file name.".into())
                }
            });
            let _ = fs::remove_file(&path);
            taken.push(Taken { id, frame });
        }
        taken
    }

    /// Writes the answer atomically.
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
                error: Some(silo_api::methods::guest_client::GuestErrorBody {
                    code: "too_large".into(),
                    message: "The answer is larger than the bridge allows.".into(),
                }),
            })
            .map_err(|error| error.to_string())?;
        }
        let io = |error: std::io::Error| format!("Cannot write the answer: {error}");
        let target = self.dir.join("resp").join(format!("{}.json", frame.id));
        let temporary = self.dir.join("resp").join(format!("{}.tmp", frame.id));
        let _ = fs::remove_file(&temporary);
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o644)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&temporary)
            .map_err(io)?;
        std::io::Write::write_all(&mut file, &bytes).map_err(io)?;
        fs::rename(&temporary, target).map_err(io)
    }

    /// Removes answers nobody read.
    pub fn sweep(&self, now: SystemTime) {
        let Ok(entries) = fs::read_dir(self.dir.join("resp")) else {
            return;
        };
        for entry in entries.flatten().take(256) {
            let old = entry
                .metadata()
                .ok()
                .and_then(|meta| meta.modified().ok())
                .and_then(|modified| now.duration_since(modified).ok())
                .is_some_and(|age| age > RESPONSE_TTL);
            if old && entry.metadata().is_ok_and(|meta| meta.is_file()) {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}

fn read_frame(path: &Path) -> Result<GuestRequestFrame, String> {
    let meta = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !meta.is_file() {
        return Err("The request is not a regular file.".into());
    }
    if meta.len() as usize > MAX_FRAME_BYTES {
        return Err("The request is larger than the bridge allows.".into());
    }
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    file.take(MAX_FRAME_BYTES as u64 + 1)
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

    const ID: &str = "0123456789abcdef0123456789abcdef";

    fn spool() -> (tempfile::TempDir, Spool) {
        let dir = tempfile::tempdir().unwrap();
        let spool = Spool::new(dir.path().join("silo-bridge"));
        spool.ensure().unwrap();
        (dir, spool)
    }

    fn request(id: &str) -> String {
        format!(
            r#"{{"v":1,"id":"{id}","token":"silo_gc_x","method":"computers.list","params":{{}}}}"#
        )
    }

    #[test]
    fn a_request_is_taken_once_and_answered_atomically() {
        let (_dir, spool) = spool();
        fs::write(
            spool.dir.join("req").join(format!("{ID}.json")),
            request(ID),
        )
        .unwrap();
        let taken = spool.take();
        assert_eq!(taken.len(), 1);
        assert_eq!(taken[0].frame.as_ref().unwrap().method, "computers.list");
        assert!(spool.take().is_empty());
        let reply = GuestResponseFrame {
            v: 1,
            id: ID.into(),
            ok: true,
            result: Some(serde_json::json!({})),
            error: None,
        };
        spool.respond(&reply).unwrap();
        assert!(spool.dir.join("resp").join(format!("{ID}.json")).is_file());
    }

    #[test]
    fn bad_names_links_oversize_and_mismatched_ids_are_not_served() {
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
        assert!(req.join("short.json").exists());
    }

    #[test]
    fn an_oversize_answer_is_replaced_by_an_error() {
        let (_dir, spool) = spool();
        let huge = serde_json::json!("x".repeat(MAX_FRAME_BYTES));
        let reply = GuestResponseFrame {
            v: 1,
            id: ID.into(),
            ok: true,
            result: Some(huge),
            error: None,
        };
        spool.respond(&reply).unwrap();
        let written =
            fs::read_to_string(spool.dir.join("resp").join(format!("{ID}.json"))).unwrap();
        assert!(written.contains("too_large"));
    }
}
