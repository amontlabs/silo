//! Resolving a user name to the numeric ids `crun exec` wants, from the computer's own
//! `/etc/passwd` and `/etc/group` as the overlay shows them: the writable layer, then the
//! sealed layers, the shim layer and the image.
//!
//! The writable layer is written by the container, so everything read from it is untrusted: the
//! files are opened component by component without following symbolic links (a link to a host
//! path would otherwise be read with the engine's rights), must be regular files, and are capped
//! in size. Overlay whiteouts and opaque directories hide what lies below them, as they do inside
//! the container.

use std::{
    ffi::CString,
    io::Read,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::ffi::OsStrExt,
    },
    path::{Path, PathBuf},
};

/// The most an account file may hold (`/etc/passwd` of a very large system is far below this).
pub const MAX_ACCOUNT_FILE: u64 = 1 << 20;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub uid: u32,
    pub gid: u32,
    pub home: String,
    /// The other groups the user is listed in (`/etc/group`), without the primary one.
    pub groups: Vec<u32>,
}

/// What a layer holds at one path.
#[derive(Debug, PartialEq, Eq)]
enum Entry {
    Missing,
    /// An overlay whiteout: the path does not exist for this layer and the ones below.
    Whiteout,
    Directory,
    Regular,
    /// A symbolic link, a device, a socket or a pipe.
    Other,
}

/// A character device 0:0 is how overlayfs records a deleted path.
fn is_whiteout(mode: u32, rdev: u64) -> bool {
    mode & libc::S_IFMT as u32 == libc::S_IFCHR as u32 && rdev == 0
}

fn classify(mode: u32, rdev: u64) -> Entry {
    if is_whiteout(mode, rdev) {
        return Entry::Whiteout;
    }
    match mode & libc::S_IFMT as u32 {
        m if m == libc::S_IFDIR as u32 => Entry::Directory,
        m if m == libc::S_IFREG as u32 => Entry::Regular,
        _ => Entry::Other,
    }
}

fn c_name(name: &str) -> Result<CString, String> {
    CString::new(name).map_err(|_| "a path contains a NUL byte".to_owned())
}

fn stat_at(dir: &OwnedFd, name: &CString) -> Result<Entry, std::io::Error> {
    // SAFETY: a zeroed stat is valid for fstatat to fill; `dir` and `name` outlive the call.
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    let rc = unsafe {
        libc::fstatat(
            dir.as_raw_fd(),
            name.as_ptr(),
            &mut stat,
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if rc != 0 {
        let error = std::io::Error::last_os_error();
        return match error.raw_os_error() {
            Some(libc::ENOENT) => Ok(Entry::Missing),
            _ => Err(error),
        };
    }
    Ok(classify(stat.st_mode as u32, stat.st_rdev as u64))
}

fn open_at(dir: &OwnedFd, name: &CString, flags: libc::c_int) -> Result<OwnedFd, std::io::Error> {
    // SAFETY: `dir` is an open descriptor and `name` a NUL-terminated string.
    let fd = unsafe {
        libc::openat(
            dir.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        // SAFETY: the descriptor was just opened and is owned by nobody else.
        Ok(unsafe { OwnedFd::from_raw_fd(fd) })
    }
}

/// Whether a directory is overlay-opaque: what lies below it in lower layers is hidden.
#[cfg(target_os = "linux")]
fn is_opaque(dir: &OwnedFd) -> bool {
    ["user.overlay.opaque", "trusted.overlay.opaque"]
        .iter()
        .any(|attribute| {
            let name = CString::new(*attribute).expect("no NUL");
            let mut value = [0u8; 2];
            // SAFETY: the buffer is valid for its length; `dir` is an open descriptor.
            let length = unsafe {
                libc::fgetxattr(
                    dir.as_raw_fd(),
                    name.as_ptr(),
                    value.as_mut_ptr().cast(),
                    value.len(),
                )
            };
            length == 1 && value[0] == b'y'
        })
}

#[cfg(not(target_os = "linux"))]
fn is_opaque(_: &OwnedFd) -> bool {
    false
}

enum InLayer {
    /// This layer has the file: its text.
    Found(String),
    /// The layer says nothing about the file; look below.
    Absent,
    /// The file does not exist, and nothing below this layer can bring it back.
    Hidden,
}

fn read_in_layer(layer: &Path, relative: &str) -> Result<InLayer, String> {
    let refuse = |why: &str| {
        Err(format!(
            "{} in {} {why}; Silo does not read it.",
            relative,
            layer.display()
        ))
    };
    let root = CString::new(layer.as_os_str().as_bytes())
        .map_err(|_| "a layer path contains a NUL byte".to_owned())?;
    // SAFETY: `root` is a NUL-terminated path.
    let raw = unsafe { libc::open(root.as_ptr(), libc::O_DIRECTORY | libc::O_CLOEXEC) };
    if raw < 0 {
        return Ok(InLayer::Absent);
    }
    // SAFETY: just opened, owned here.
    let mut dir = unsafe { OwnedFd::from_raw_fd(raw) };
    let parts: Vec<&str> = relative
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    let Some((last, parents)) = parts.split_last() else {
        return Ok(InLayer::Absent);
    };
    let mut opaque = false;
    for part in parents {
        let name = c_name(part)?;
        match stat_at(&dir, &name).map_err(|e| e.to_string())? {
            Entry::Missing => {
                return Ok(if opaque {
                    InLayer::Hidden
                } else {
                    InLayer::Absent
                })
            }
            Entry::Whiteout => return Ok(InLayer::Hidden),
            Entry::Directory => {}
            _ => return refuse("goes through something that is not a directory"),
        }
        dir = open_at(&dir, &name, libc::O_DIRECTORY | libc::O_RDONLY)
            .map_err(|e| format!("{relative} in {}: {e}", layer.display()))?;
        opaque = opaque || is_opaque(&dir);
    }
    let name = c_name(last)?;
    match stat_at(&dir, &name).map_err(|e| e.to_string())? {
        Entry::Missing => Ok(if opaque {
            InLayer::Hidden
        } else {
            InLayer::Absent
        }),
        Entry::Whiteout => Ok(InLayer::Hidden),
        Entry::Regular => {
            let file = open_at(&dir, &name, libc::O_RDONLY | libc::O_NONBLOCK)
                .map_err(|e| format!("{relative} in {}: {e}", layer.display()))?;
            let mut file = std::fs::File::from(file);
            let metadata = file.metadata().map_err(|e| e.to_string())?;
            if !metadata.is_file() {
                return refuse("is not a regular file");
            }
            if metadata.len() > MAX_ACCOUNT_FILE {
                return refuse("is too large");
            }
            let mut text = String::new();
            (&mut file)
                .take(MAX_ACCOUNT_FILE + 1)
                .read_to_string(&mut text)
                .map_err(|_| {
                    format!(
                        "{relative} in {} is not text; Silo does not read it.",
                        layer.display()
                    )
                })?;
            if text.len() as u64 > MAX_ACCOUNT_FILE {
                return refuse("is too large");
            }
            Ok(InLayer::Found(text))
        }
        Entry::Directory => refuse("is a directory"),
        Entry::Other => refuse("is not a regular file"),
    }
}

/// Reads `relative` (such as `etc/passwd`) from the first layer that has it, top layer first.
///
/// `Ok(None)`: the path does not exist as the container sees it. `Err`: a layer holds something
/// Silo will not read (a link, a device, a pipe, an oversized file) at the path.
pub fn read_from_layers(layers: &[PathBuf], relative: &str) -> Result<Option<String>, String> {
    for layer in layers {
        match read_in_layer(layer, relative)? {
            InLayer::Found(text) => return Ok(Some(text)),
            InLayer::Hidden => return Ok(None),
            InLayer::Absent => {}
        }
    }
    Ok(None)
}

/// The groups other than `primary` that list `user` as a member.
fn supplementary_groups(user: &str, primary: u32, group: &str) -> Vec<u32> {
    let mut gids: Vec<u32> = group
        .lines()
        .filter(|line| !line.starts_with('#'))
        .map(|line| line.split(':').collect::<Vec<_>>())
        .filter(|fields| fields.len() >= 4 && fields[3].split(',').any(|member| member == user))
        .filter_map(|fields| fields[2].parse().ok())
        .filter(|gid| *gid != primary)
        .collect();
    gids.sort_unstable();
    gids.dedup();
    gids
}

/// `name` as a number (`1001`, `1001:1001`) or looked up in `passwd` and `group`.
pub fn resolve(name: &str, passwd: &str, group: &str) -> Option<Identity> {
    let (user, group_name) = match name.split_once(':') {
        Some((user, group)) => (user, Some(group)),
        None => (name, None),
    };
    let entry = passwd
        .lines()
        .filter(|line| !line.starts_with('#'))
        .map(|line| line.split(':').collect::<Vec<_>>())
        .find(|fields| fields.len() >= 6 && (fields[0] == user || fields[2] == user));
    let (uid, mut gid, home, login) = match entry {
        Some(fields) => (
            fields[2].parse().ok()?,
            fields[3].parse().ok()?,
            fields[5].to_owned(),
            Some(fields[0]),
        ),
        None => {
            let number = user.parse::<u32>().ok()?;
            (number, number, "/".to_owned(), None)
        }
    };
    if let Some(group_name) = group_name {
        gid = group
            .lines()
            .map(|line| line.split(':').collect::<Vec<_>>())
            .find(|fields| {
                fields.len() >= 3 && (fields[0] == group_name || fields[2] == group_name)
            })
            .and_then(|fields| fields[2].parse().ok())
            .or_else(|| group_name.parse().ok())?;
    }
    // An explicit group is the whole answer, as for `docker run --user name:group`.
    let groups = match (login, group_name) {
        (Some(login), None) => supplementary_groups(login, gid, group),
        _ => Vec::new(),
    };
    Some(Identity {
        uid,
        gid,
        home,
        groups,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};

    const PASSWD: &str =
        "root:x:0:0:root:/root:/bin/bash\nsilo:x:1001:1001::/home/silo:/bin/bash\n";
    const GROUP: &str = "root:x:0:\nsilo:x:1001:\nsudo:x:27:silo\nvideo:x:44:silo,other\n";

    #[test]
    fn names_resolve_to_ids_and_home() {
        let silo = resolve("silo", PASSWD, GROUP).unwrap();
        assert_eq!(
            (silo.uid, silo.gid, silo.home.as_str()),
            (1001, 1001, "/home/silo")
        );
        let root = resolve("root", PASSWD, GROUP).unwrap();
        assert_eq!((root.uid, root.gid, root.home.as_str()), (0, 0, "/root"));
    }

    #[test]
    fn supplementary_groups_come_from_group_membership() {
        assert_eq!(resolve("silo", PASSWD, GROUP).unwrap().groups, [27, 44]);
        assert!(resolve("root", PASSWD, GROUP).unwrap().groups.is_empty());
        // By number the entry is the same; with an explicit group there are none.
        assert_eq!(resolve("1001", PASSWD, GROUP).unwrap().groups, [27, 44]);
        assert!(resolve("silo:video", PASSWD, GROUP)
            .unwrap()
            .groups
            .is_empty());
        // The primary group is never repeated, and an unknown user has none.
        let primary = "silo:x:1001:silo\nsudo:x:27:silo,silo\n";
        assert_eq!(resolve("silo", PASSWD, primary).unwrap().groups, [27]);
        assert!(resolve("4242", PASSWD, GROUP).unwrap().groups.is_empty());
    }

    #[test]
    fn numbers_and_explicit_groups_resolve() {
        assert_eq!(resolve("1001", PASSWD, GROUP).unwrap().home, "/home/silo");
        let unknown = resolve("4242", PASSWD, GROUP).unwrap();
        assert_eq!(
            (unknown.uid, unknown.gid, unknown.home.as_str()),
            (4242, 4242, "/")
        );
        assert_eq!(resolve("silo:video", PASSWD, GROUP).unwrap().gid, 44);
        assert_eq!(resolve("nobody-here", PASSWD, GROUP), None);
    }

    fn put(layer: &Path, relative: &str, text: &str) {
        let path = layer.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn the_first_layer_with_the_file_wins() {
        let upper = tempfile::tempdir().unwrap();
        let lower = tempfile::tempdir().unwrap();
        put(lower.path(), "etc/passwd", "lower");
        let layers = [upper.path().to_path_buf(), lower.path().to_path_buf()];
        assert_eq!(
            read_from_layers(&layers, "etc/passwd").unwrap().as_deref(),
            Some("lower")
        );
        put(upper.path(), "etc/passwd", "upper");
        assert_eq!(
            read_from_layers(&layers, "etc/passwd").unwrap().as_deref(),
            Some("upper")
        );
        assert_eq!(read_from_layers(&layers, "etc/group").unwrap(), None);
    }

    #[test]
    fn a_link_is_never_followed_whether_it_is_the_file_or_a_directory_above_it() {
        let secret = tempfile::tempdir().unwrap();
        put(secret.path(), "passwd", "root:x:0:0::/host:/bin/sh\n");
        let upper = tempfile::tempdir().unwrap();
        let lower = tempfile::tempdir().unwrap();
        put(lower.path(), "etc/passwd", "lower");
        let layers = [upper.path().to_path_buf(), lower.path().to_path_buf()];
        // The file itself is a link to a host file.
        std::fs::create_dir_all(upper.path().join("etc")).unwrap();
        symlink(
            secret.path().join("passwd"),
            upper.path().join("etc/passwd"),
        )
        .unwrap();
        let error = read_from_layers(&layers, "etc/passwd").unwrap_err();
        assert!(error.contains("not a regular file"), "{error}");
        // A directory above it is a link to a host directory.
        std::fs::remove_dir_all(upper.path().join("etc")).unwrap();
        symlink(secret.path(), upper.path().join("etc")).unwrap();
        let error = read_from_layers(&layers, "etc/passwd").unwrap_err();
        assert!(error.contains("not a directory"), "{error}");
        // A relative link inside the layer is refused as well.
        std::fs::remove_file(upper.path().join("etc")).unwrap();
        std::fs::create_dir_all(upper.path().join("real")).unwrap();
        std::fs::write(upper.path().join("real/passwd"), "x").unwrap();
        symlink("real", upper.path().join("etc")).unwrap();
        assert!(read_from_layers(&layers, "etc/passwd").is_err());
    }

    #[test]
    fn a_pipe_a_directory_and_an_oversized_file_are_refused_without_blocking() {
        let upper = tempfile::tempdir().unwrap();
        let layers = [upper.path().to_path_buf()];
        std::fs::create_dir_all(upper.path().join("etc")).unwrap();
        let fifo = CString::new(upper.path().join("etc/passwd").as_os_str().as_bytes()).unwrap();
        // SAFETY: a valid NUL-terminated path.
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o644) }, 0);
        let error = read_from_layers(&layers, "etc/passwd").unwrap_err();
        assert!(error.contains("not a regular file"), "{error}");
        std::fs::remove_file(upper.path().join("etc/passwd")).unwrap();
        std::fs::create_dir(upper.path().join("etc/passwd")).unwrap();
        assert!(read_from_layers(&layers, "etc/passwd")
            .unwrap_err()
            .contains("directory"));
        std::fs::remove_dir(upper.path().join("etc/passwd")).unwrap();
        let big = std::fs::File::create(upper.path().join("etc/passwd")).unwrap();
        big.set_len(MAX_ACCOUNT_FILE + 1).unwrap();
        let error = read_from_layers(&layers, "etc/passwd").unwrap_err();
        assert!(error.contains("too large"), "{error}");
        big.set_len(MAX_ACCOUNT_FILE).unwrap();
        std::fs::set_permissions(
            upper.path().join("etc/passwd"),
            std::fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        // At the cap it is read.
        assert_eq!(
            read_from_layers(&layers, "etc/passwd")
                .unwrap()
                .unwrap()
                .len() as u64,
            MAX_ACCOUNT_FILE
        );
    }

    #[test]
    fn a_whiteout_hides_the_file_and_the_layers_below_it() {
        // A character device 0:0 is the whiteout overlayfs leaves; `mknod` needs privilege to
        // make one here, so the classification is checked on its own.
        assert!(is_whiteout(libc::S_IFCHR as u32 | 0o000, 0));
        assert_eq!(classify(libc::S_IFCHR as u32, 0), Entry::Whiteout);
        assert_eq!(classify(libc::S_IFCHR as u32, 5), Entry::Other);
        assert_eq!(classify(libc::S_IFREG as u32 | 0o644, 0), Entry::Regular);
        assert_eq!(classify(libc::S_IFDIR as u32 | 0o755, 0), Entry::Directory);
        assert_eq!(classify(libc::S_IFLNK as u32, 0), Entry::Other);
        assert_eq!(classify(libc::S_IFIFO as u32, 0), Entry::Other);
    }

    /// Makes a real overlay whiteout where the kernel lets an ordinary user (Linux); false elsewhere.
    fn whiteout(path: &Path) -> bool {
        let name = CString::new(path.as_os_str().as_bytes()).unwrap();
        // SAFETY: a valid NUL-terminated path; device 0:0 is the whiteout.
        unsafe { libc::mknod(name.as_ptr(), libc::S_IFCHR as _, 0) == 0 }
    }

    #[test]
    fn a_real_whiteout_in_a_higher_layer_hides_the_file_below() {
        let upper = tempfile::tempdir().unwrap();
        let lower = tempfile::tempdir().unwrap();
        put(lower.path(), "etc/passwd", "lower");
        let layers = [upper.path().to_path_buf(), lower.path().to_path_buf()];
        std::fs::create_dir_all(upper.path().join("etc")).unwrap();
        if !whiteout(&upper.path().join("etc/passwd")) {
            eprintln!("skipped: this system does not let an ordinary user make a whiteout");
            return;
        }
        assert_eq!(read_from_layers(&layers, "etc/passwd").unwrap(), None);
        // The whole directory deleted.
        std::fs::remove_dir_all(upper.path().join("etc")).unwrap();
        assert!(whiteout(&upper.path().join("etc")));
        assert_eq!(read_from_layers(&layers, "etc/passwd").unwrap(), None);
    }
}
