//! `silo-ctr pack LAYER` and `silo-ctr unpack [--min-free-bytes N] [--min-free-inodes N] LAYER`:
//! one overlay layer to a tar stream and back, for the export and import of a container computer.
//! Both run inside the container's user namespace (`silo-ctr userns --keep-id N -- silo-ctr pack
//! ...`), where the layer's owners are real and a whiteout can be made.
//!
//! The stream is standard tar (PAX, written and read by the `tar` crate): folders, files, symbolic
//! and hard links, named pipes, and whiteouts (character device 0:0, which the layers below may
//! still need hidden). Extended attributes travel as PAX `SCHILY.xattr.*` records, so
//! `user.overlay.opaque` and file capabilities survive. Tar has no socket type, so sockets are
//! left out of an export (nothing listens on one after an import anyway); any other device is
//! refused.
//!
//! What makes `unpack` safe on a file somebody else made is not the format but where it runs:
//! `pack` and `unpack` both change their root to the layer first, so a path or link in the stream
//! can only name the layer's own files. On top of that `unpack` checks every path, refuses the
//! overlay attributes other than `user.overlay.opaque`, and holds a bounded amount in memory: a
//! folder's owner and attributes are applied as it is made and only its mode and time wait for
//! the end of the folder (the stream lists a folder before its contents, so what waits is the
//! chain of open folders, not every folder). The process runs under an address-space limit, and
//! it measures the real disk (free bytes and free inodes of the layer's file system) instead of a
//! model of what the stream will need.

/// The most an address space may grow to in `pack` and `unpack`. Their buffers are a few MiB, the
/// open folders at most [`MAX_PATH`] bytes per path component, and one entry's attributes at most
/// [`MAX_XATTRS`] times [`MAX_XATTR_VALUE`] bytes, so this leaves room for all of those at once;
/// a stream that makes the tar reader hold more (an extended header of any size) ends in a failed
/// allocation instead of using the host's memory.
pub const ADDRESS_SPACE_LIMIT: u64 = 1 << 30;
pub const MAX_PATH: usize = 4096;
const MAX_XATTRS: usize = 1024;
const MAX_XATTR_NAME: usize = 255;
const MAX_XATTR_VALUE: usize = 65536;
/// The PAX record prefix GNU tar and libarchive use for extended attributes.
const XATTR_PREFIX: &str = "SCHILY.xattr.";
/// The only attribute of the overlay's own namespace a layer may carry.
const OPAQUE: &[u8] = b"user.overlay.opaque";
const OVERLAY_NAMESPACE: &[u8] = b"user.overlay.";
/// The most written between two looks at the disk.
const CHUNK: u64 = 16 << 20;
/// The disk is also looked at once per this many entries, files or not.
const LOOK_EVERY: u64 = 256;

/// The options of an `unpack` command line: how much of the file system must stay free, and the
/// layer.
#[derive(Debug, PartialEq)]
pub struct UnpackArgs {
    pub min_free_bytes: u64,
    pub min_free_inodes: u64,
    pub layer: String,
}

pub fn parse_unpack_args(args: &[String]) -> Result<UnpackArgs, String> {
    let mut parsed = UnpackArgs {
        min_free_bytes: 0,
        min_free_inodes: 0,
        layer: String::new(),
    };
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let mut number = |flag: &str| -> Result<u64, String> {
            iter.next()
                .ok_or_else(|| format!("{flag} needs a number"))?
                .parse()
                .map_err(|_| format!("{flag} needs a number"))
        };
        match arg.as_str() {
            "--min-free-bytes" => parsed.min_free_bytes = number("--min-free-bytes")?,
            "--min-free-inodes" => parsed.min_free_inodes = number("--min-free-inodes")?,
            other if other.starts_with("--") => return Err(format!("unknown option {other}")),
            other if parsed.layer.is_empty() => parsed.layer = other.to_owned(),
            _ => return Err("unpack takes one layer".into()),
        }
    }
    if parsed.layer.is_empty() {
        return Err("unpack needs a layer".into());
    }
    Ok(parsed)
}

pub fn parse_pack_args(args: &[String]) -> Result<String, String> {
    match args {
        [layer] if !layer.starts_with("--") => Ok(layer.clone()),
        _ => Err("pack takes one layer".into()),
    }
}

/// What a path in a stream may be: relative, made of ordinary names, and not too long.
pub fn valid_entry_path(path: &[u8]) -> bool {
    if path == b"." {
        return true;
    }
    !path.is_empty()
        && path.len() <= MAX_PATH
        && !path.contains(&0)
        && path
            .split(|byte| *byte == b'/')
            .all(|name| !name.is_empty() && name != b"." && name != b"..")
}

/// The path of a tar entry in the form [`valid_entry_path`] takes: tar writers add a slash to a
/// folder's name, and the layer's own folder is `./` (or `.`).
pub fn entry_path(raw: &[u8], is_dir: bool) -> Result<Vec<u8>, String> {
    let outside = || "the stream names a path outside the layer".to_string();
    let path = match (is_dir, raw.strip_suffix(b"/")) {
        (true, Some(rest)) => rest,
        (false, Some(_)) => return Err(outside()),
        (_, None) => raw,
    };
    if valid_entry_path(path) {
        Ok(path.to_vec())
    } else {
        Err(outside())
    }
}

/// Whether an attribute may be set from a stream: the overlay's own attributes decide which files
/// of the layers below show through, so only the one the layers use is allowed.
pub fn xattr_allowed(name: &[u8]) -> Result<(), String> {
    if name.is_empty() || name.len() > MAX_XATTR_NAME || name.contains(&0) {
        return Err("the stream has an attribute with a bad name".into());
    }
    if name.starts_with(OVERLAY_NAMESPACE) && name != OPAQUE {
        return Err(format!(
            "the stream has the overlay attribute {}, which a layer may not carry",
            String::from_utf8_lossy(name)
        ));
    }
    Ok(())
}

/// Whether `folder` (a path, `.` for the layer) holds `path`.
pub fn holds(folder: &[u8], path: &[u8]) -> bool {
    folder == b"."
        || (path.len() > folder.len() && path.starts_with(folder) && path[folder.len()] == b'/')
}

#[cfg(target_os = "linux")]
pub use linux::{pack_command, unpack_command};

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use crate::common::{io_err, os_err};
    use std::collections::HashMap;
    use std::ffi::{CString, OsStr};
    use std::fs;
    use std::io::{self, Read, Write};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    use std::path::{Path, PathBuf};
    use tar::{Archive, Builder, EntryType, Header};

    fn c(path: &Path) -> Result<CString, String> {
        CString::new(path.as_os_str().as_bytes()).map_err(|_| "a path contains a NUL byte".into())
    }

    fn name_c(name: &[u8]) -> Result<CString, String> {
        CString::new(name).map_err(|_| "an attribute name contains NUL".into())
    }

    /// The limit on memory, then the layer as the whole world of this process.
    fn confine(layer: &str) -> Result<(), String> {
        let limit = libc::rlimit {
            rlim_cur: ADDRESS_SPACE_LIMIT,
            rlim_max: ADDRESS_SPACE_LIMIT,
        };
        // SAFETY: `limit` is a valid rlimit; the call changes only this process.
        if unsafe { libc::setrlimit(libc::RLIMIT_AS, &limit) } != 0 {
            return Err(os_err("limit the memory"));
        }
        let path = c(Path::new(layer))?;
        let root = CString::new("/").expect("no NUL");
        // SAFETY: valid path strings; the calls change only this process's root and directory.
        unsafe {
            if libc::chroot(path.as_ptr()) != 0 {
                return Err(os_err(&format!("enter {layer}")));
            }
            if libc::chdir(root.as_ptr()) != 0 {
                return Err(os_err("change to the layer's root"));
            }
        }
        Ok(())
    }

    type Xattrs = Vec<(Vec<u8>, Vec<u8>)>;

    fn xattr_pairs(path: &Path) -> Result<Xattrs, String> {
        let p = c(path)?;
        // SAFETY: a null buffer of size 0 asks for the length only.
        let length = unsafe { libc::llistxattr(p.as_ptr(), std::ptr::null_mut(), 0) };
        if length < 0 {
            return match std::io::Error::last_os_error().raw_os_error() {
                Some(libc::ENOTSUP) => Ok(Vec::new()),
                _ => Err(os_err(&format!(
                    "list the attributes of {}",
                    path.display()
                ))),
            };
        }
        let mut names = vec![0u8; length as usize];
        // SAFETY: `names` is valid for its length.
        let length =
            unsafe { libc::llistxattr(p.as_ptr(), names.as_mut_ptr().cast(), names.len()) };
        if length < 0 {
            return Err(os_err(&format!(
                "list the attributes of {}",
                path.display()
            )));
        }
        names.truncate(length as usize);
        let mut pairs = Vec::new();
        for name in names
            .split(|byte| *byte == 0)
            .filter(|name| !name.is_empty())
            // Overlayfs keeps its own records (`user.overlay.uuid`, `origin`, `impure`) on a
            // layer it has mounted and writes them again on the next mount; only the opaque
            // mark is the layer's content.
            .filter(|name| !name.starts_with(OVERLAY_NAMESPACE) || *name == OPAQUE)
        {
            let n = name_c(name)?;
            let mut value = vec![0u8; MAX_XATTR_VALUE];
            // SAFETY: `value` is valid for its length.
            let got = unsafe {
                libc::lgetxattr(
                    p.as_ptr(),
                    n.as_ptr(),
                    value.as_mut_ptr().cast(),
                    value.len(),
                )
            };
            if got < 0 {
                return Err(os_err(&format!("read an attribute of {}", path.display())));
            }
            value.truncate(got as usize);
            pairs.push((name.to_vec(), value));
        }
        Ok(pairs)
    }

    fn relative(root: &Path, path: &Path) -> PathBuf {
        match path.strip_prefix(root) {
            Ok(rest) if rest.as_os_str().is_empty() => PathBuf::from("."),
            Ok(rest) => rest.to_path_buf(),
            Err(_) => PathBuf::from("."),
        }
    }

    fn write_err(error: io::Error) -> String {
        io_err("write the stream", &error)
    }

    pub fn pack_command(arguments: &[String]) -> Result<i32, String> {
        let layer = parse_pack_args(arguments)?;
        confine(&layer)?;
        let stdout = io::stdout();
        let out = io::BufWriter::with_capacity(1 << 20, stdout.lock());
        pack_tree(Path::new("/"), out)?;
        Ok(0)
    }

    /// A file's bytes, failing when the file ends before the size its header gave.
    struct Exact<R> {
        inner: R,
        left: u64,
    }

    impl<R: Read> Read for Exact<R> {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if self.left == 0 {
                return Ok(0);
            }
            let limit = buffer.len().min(self.left as usize);
            let got = self.inner.read(&mut buffer[..limit])?;
            if got == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "the file changed while it was read",
                ));
            }
            self.left -= got as u64;
            Ok(got)
        }
    }

    fn header_of(meta: &fs::Metadata, kind: EntryType) -> Header {
        let mut header = Header::new_gnu();
        header.set_entry_type(kind);
        header.set_mode(meta.mode() & 0o7777);
        header.set_uid(u64::from(meta.uid()));
        header.set_gid(u64::from(meta.gid()));
        header.set_mtime(meta.mtime().max(0) as u64);
        header.set_size(0);
        header
    }

    /// The whole tree of `root` as tar onto `out`.
    pub fn pack_tree(root: &Path, out: impl Write) -> Result<(), String> {
        let meta = fs::symlink_metadata(root)
            .map_err(|e| io_err(&format!("look at {}", root.display()), &e))?;
        if !meta.is_dir() {
            return Err(format!("{} is not a folder", root.display()));
        }
        let mut builder = Builder::new(out);
        let mut links = HashMap::new();
        pack_dir(root, root, &meta, &mut links, &mut builder)?;
        let mut out = builder.into_inner().map_err(write_err)?;
        out.flush().map_err(write_err)
    }

    /// The PAX record for an entry's attributes, ahead of its header.
    fn put_xattrs<W: Write>(builder: &mut Builder<W>, path: &Path) -> Result<(), String> {
        let pairs = xattr_pairs(path)?;
        if pairs.len() > MAX_XATTRS {
            return Err(format!("{} has too many attributes", path.display()));
        }
        let mut records = Vec::new();
        for (name, value) in &pairs {
            let key = std::str::from_utf8(name).map_err(|_| {
                format!(
                    "an attribute of {} has a name that is not text",
                    path.display()
                )
            })?;
            records.push((format!("{XATTR_PREFIX}{key}"), value.as_slice()));
        }
        builder
            .append_pax_extensions(records.iter().map(|(key, value)| (key.as_str(), *value)))
            .map_err(write_err)
    }

    type Links = HashMap<(u64, u64), PathBuf>;

    fn pack_dir<W: Write>(
        root: &Path,
        dir: &Path,
        meta: &fs::Metadata,
        links: &mut Links,
        builder: &mut Builder<W>,
    ) -> Result<(), String> {
        put_xattrs(builder, dir)?;
        let mut header = header_of(meta, EntryType::Directory);
        builder
            .append_data(&mut header, relative(root, dir), io::empty())
            .map_err(write_err)?;
        let mut names: Vec<_> = fs::read_dir(dir)
            .map_err(|e| io_err(&format!("read {}", dir.display()), &e))?
            .map(|entry| entry.map(|entry| entry.file_name()))
            .collect::<Result<_, _>>()
            .map_err(|e| io_err(&format!("read {}", dir.display()), &e))?;
        names.sort();
        for name in names {
            let path = dir.join(&name);
            let meta = fs::symlink_metadata(&path)
                .map_err(|e| io_err(&format!("look at {}", path.display()), &e))?;
            let kind = meta.file_type();
            let rel = relative(root, &path);
            if kind.is_dir() {
                pack_dir(root, &path, &meta, links, builder)?;
                continue;
            }
            if kind.is_socket() {
                continue;
            }
            if !kind.is_symlink() && meta.nlink() > 1 {
                let key = (meta.dev(), meta.ino());
                if let Some(first) = links.get(&key) {
                    let mut header = header_of(&meta, EntryType::Link);
                    builder
                        .append_link(&mut header, &rel, first)
                        .map_err(write_err)?;
                    continue;
                }
                links.insert(key, rel.clone());
            }
            if kind.is_symlink() {
                let target = fs::read_link(&path)
                    .map_err(|e| io_err(&format!("read {}", path.display()), &e))?;
                put_xattrs(builder, &path)?;
                let mut header = header_of(&meta, EntryType::Symlink);
                builder
                    .append_link(&mut header, &rel, target)
                    .map_err(write_err)?;
            } else if kind.is_file() {
                put_xattrs(builder, &path)?;
                let mut header = header_of(&meta, EntryType::Regular);
                header.set_size(meta.len());
                let file = fs::File::open(&path)
                    .map_err(|e| io_err(&format!("open {}", path.display()), &e))?;
                // A file that changes while it is read is a file that is not at rest: the
                // caller packs checkpoints, which are.
                let data = Exact {
                    inner: file,
                    left: meta.len(),
                };
                builder
                    .append_data(&mut header, &rel, data)
                    .map_err(|e| io_err(&format!("pack {}", path.display()), &e))?;
            } else if kind.is_fifo() {
                put_xattrs(builder, &path)?;
                let mut header = header_of(&meta, EntryType::Fifo);
                builder
                    .append_data(&mut header, &rel, io::empty())
                    .map_err(write_err)?;
            } else if kind.is_char_device() && meta.rdev() == 0 {
                put_xattrs(builder, &path)?;
                let mut header = header_of(&meta, EntryType::Char);
                header.set_device_major(0).map_err(write_err)?;
                header.set_device_minor(0).map_err(write_err)?;
                builder
                    .append_data(&mut header, &rel, io::empty())
                    .map_err(write_err)?;
            } else {
                return Err(format!(
                    "{} is a device, which a container computer's files cannot hold",
                    path.display()
                ));
            }
        }
        Ok(())
    }

    pub fn unpack_command(arguments: &[String]) -> Result<i32, String> {
        let parsed = parse_unpack_args(arguments)?;
        confine(&parsed.layer)?;
        let stdin = io::stdin();
        let input = io::BufReader::with_capacity(1 << 20, stdin.lock());
        unpack_tree(
            Path::new("/"),
            input,
            &Room {
                min_free_bytes: parsed.min_free_bytes,
                min_free_inodes: parsed.min_free_inodes,
            },
        )?;
        Ok(0)
    }

    /// How much of the file system must stay free.
    pub struct Room {
        pub min_free_bytes: u64,
        pub min_free_inodes: u64,
    }

    impl Room {
        /// An error when writing `need` more bytes would leave the file system holding `root`
        /// with less than the margin of free bytes, or when it is already short of free inodes.
        fn check(&self, root: &Path, need: u64) -> Result<(), String> {
            let p = c(root)?;
            // SAFETY: `stat` is written by the call, and `p` is a valid path string.
            let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
            if unsafe { libc::statvfs(p.as_ptr(), &mut stat) } != 0 {
                return Err(os_err("look at the free space"));
            }
            let free = (stat.f_bavail as u64).saturating_mul(stat.f_frsize as u64);
            if free < self.min_free_bytes.saturating_add(need) {
                return Err("this device does not have the room for the layer".into());
            }
            // A file system that keeps no inode count (btrfs) reports none.
            if stat.f_files != 0 && (stat.f_favail as u64) < self.min_free_inodes {
                return Err("this device has run out of files to make for the layer".into());
            }
            Ok(())
        }
    }

    fn join(root: &Path, relative: &[u8]) -> PathBuf {
        if relative == b"." {
            root.to_path_buf()
        } else {
            root.join(OsStr::from_bytes(relative))
        }
    }

    fn set_xattrs(path: &Path, xattrs: &Xattrs) -> Result<(), String> {
        let p = c(path)?;
        for (name, value) in xattrs {
            let n = name_c(name)?;
            // SAFETY: both buffers are valid for their lengths.
            let rc = unsafe {
                libc::lsetxattr(
                    p.as_ptr(),
                    n.as_ptr(),
                    value.as_ptr().cast(),
                    value.len(),
                    0,
                )
            };
            if rc != 0 {
                return Err(os_err(&format!("set an attribute of {}", path.display())));
            }
        }
        Ok(())
    }

    /// What an entry says about itself besides its kind.
    struct Meta {
        mode: u32,
        uid: u32,
        gid: u32,
        mtime: i64,
        xattrs: Xattrs,
    }

    /// Owner, then attributes: a folder gets both as it is made.
    fn own(path: &Path, meta: &Meta) -> Result<(), String> {
        let p = c(path)?;
        // SAFETY: `p` is a valid path string.
        if unsafe { libc::lchown(p.as_ptr(), meta.uid, meta.gid) } != 0 {
            return Err(os_err(&format!("change the owner of {}", path.display())));
        }
        set_xattrs(path, &meta.xattrs)
    }

    fn set_mode(path: &Path, mode: u32) -> Result<(), String> {
        let p = c(path)?;
        // SAFETY: `p` is a valid path string.
        if unsafe { libc::chmod(p.as_ptr(), mode as libc::mode_t) } != 0 {
            return Err(os_err(&format!("set the mode of {}", path.display())));
        }
        Ok(())
    }

    fn set_time(path: &Path, mtime: i64) -> Result<(), String> {
        let p = c(path)?;
        let times = [
            libc::timespec {
                tv_sec: mtime,
                tv_nsec: libc::UTIME_OMIT,
            },
            libc::timespec {
                tv_sec: mtime,
                tv_nsec: 0,
            },
        ];
        // SAFETY: `times` holds two timespecs; the flag keeps a link from being followed.
        let rc = unsafe {
            libc::utimensat(
                libc::AT_FDCWD,
                p.as_ptr(),
                times.as_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if rc != 0 {
            return Err(os_err(&format!("set the time of {}", path.display())));
        }
        Ok(())
    }

    /// Owner, attributes, mode and time, in the order that keeps each from undoing another.
    fn apply(path: &Path, meta: &Meta, is_link: bool) -> Result<(), String> {
        own(path, meta)?;
        if !is_link {
            set_mode(path, meta.mode)?;
        }
        set_time(path, meta.mtime)
    }

    /// A whiteout (character device 0:0): the node `mknod` makes without a device.
    fn mknod(path: &Path, mode: u32) -> Result<(), String> {
        let p = c(path)?;
        // SAFETY: `p` is a valid path string.
        if unsafe { libc::mknod(p.as_ptr(), mode as libc::mode_t, 0) } != 0 {
            return Err(os_err(&format!("create {}", path.display())));
        }
        Ok(())
    }

    fn fail(path: &Path, what: &str, error: &io::Error) -> String {
        io_err(&format!("{what} {}", path.display()), error)
    }

    /// A folder still being filled: its mode and time wait for the end of it.
    struct Open {
        path: Vec<u8>,
        target: PathBuf,
        mode: u32,
        mtime: i64,
    }

    fn close(folder: Open) -> Result<(), String> {
        set_mode(&folder.target, folder.mode)?;
        set_time(&folder.target, folder.mtime)
    }

    /// Copies `size` bytes in chunks, looking at the disk before each.
    fn copy_content(
        input: &mut impl Read,
        file: &mut fs::File,
        size: u64,
        root: &Path,
        room: &Room,
    ) -> Result<(), String> {
        let mut left = size;
        let mut buffer = vec![0u8; 1 << 16];
        while left > 0 {
            let chunk = left.min(CHUNK);
            room.check(root, chunk)?;
            let mut piece = chunk;
            while piece > 0 {
                let want = (piece as usize).min(buffer.len());
                input
                    .read_exact(&mut buffer[..want])
                    .map_err(|e| io_err("read the stream", &e))?;
                file.write_all(&buffer[..want])
                    .map_err(|e| io_err("write a file", &e))?;
                piece -= want as u64;
            }
            left -= chunk;
        }
        Ok(())
    }

    /// The attributes of an entry from its PAX records; anything else in them is ignored.
    fn xattrs_of<R: Read>(entry: &mut tar::Entry<'_, R>) -> Result<Xattrs, String> {
        let mut xattrs = Vec::new();
        let records = entry
            .pax_extensions()
            .map_err(|e| io_err("read the stream", &e))?;
        for record in records.into_iter().flatten() {
            let record = record.map_err(|e| io_err("read the stream", &e))?;
            let Some(name) = record.key_bytes().strip_prefix(XATTR_PREFIX.as_bytes()) else {
                continue;
            };
            xattr_allowed(name)?;
            if record.value_bytes().len() > MAX_XATTR_VALUE || xattrs.len() == MAX_XATTRS {
                return Err("the stream has an entry with too many or too large attributes".into());
            }
            xattrs.push((name.to_vec(), record.value_bytes().to_vec()));
        }
        Ok(xattrs)
    }

    fn impossible() -> String {
        "the stream has an entry with an impossible owner, mode or time".into()
    }

    /// Reads a tar stream into `root`, which must be an empty folder (the layer itself, once this
    /// process has made it its root). The disk is looked at before every chunk of a file, every
    /// [`LOOK_EVERY`] entries, and whenever names, link targets and attributes add up to a chunk;
    /// nothing is written below the margin `room` names.
    pub fn unpack_tree(root: &Path, input: impl Read, room: &Room) -> Result<(), String> {
        let mut archive = Archive::new(input);
        let entries = archive
            .entries()
            .map_err(|e| io_err("read the stream", &e))?;
        let mut open: Vec<Open> = Vec::new();
        let mut count = 0u64;
        // Names, link targets and attributes written since the disk was last looked at.
        let mut since_look = 0u64;
        for entry in entries {
            let mut entry = entry.map_err(|e| io_err("read the stream", &e))?;
            let kind = entry.header().entry_type();
            let path = entry_path(&entry.path_bytes(), kind.is_dir())?;
            if (count == 0) != (path == b".") || (count == 0 && !kind.is_dir()) {
                return Err(
                    "the stream's first entry must be the layer's folder, and only it".into(),
                );
            }
            count += 1;
            while let Some(top) = open.last() {
                if holds(&top.path, &path) {
                    break;
                }
                close(open.pop().expect("a last"))?;
            }
            let header = entry.header();
            let meta = Meta {
                mode: header.mode().map_err(|_| impossible())?,
                uid: u32::try_from(header.uid().map_err(|_| impossible())?)
                    .map_err(|_| impossible())?,
                gid: u32::try_from(header.gid().map_err(|_| impossible())?)
                    .map_err(|_| impossible())?,
                mtime: i64::try_from(header.mtime().map_err(|_| impossible())?)
                    .map_err(|_| impossible())?,
                xattrs: xattrs_of(&mut entry)?,
            };
            if meta.mode > 0o7777 {
                return Err(impossible());
            }
            let link = entry.link_name_bytes().map_or(0, |link| link.len());
            let attributes: usize = meta.xattrs.iter().map(|(n, v)| n.len() + v.len()).sum();
            let metadata = (path.len() + link + attributes) as u64;
            since_look += metadata;
            if (count - 1).is_multiple_of(LOOK_EVERY) || since_look >= CHUNK {
                room.check(root, metadata)?;
                since_look = 0;
            }
            let target = join(root, &path);
            match kind {
                EntryType::Directory => {
                    if path != b"." {
                        fs::create_dir(&target).map_err(|e| fail(&target, "create", &e))?;
                    }
                    own(&target, &meta)?;
                    open.push(Open {
                        path,
                        target,
                        mode: meta.mode,
                        mtime: meta.mtime,
                    });
                }
                EntryType::Regular => {
                    let size = entry.size();
                    let mut file = fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&target)
                        .map_err(|e| fail(&target, "create", &e))?;
                    copy_content(&mut entry, &mut file, size, root, room)?;
                    drop(file);
                    apply(&target, &meta, false)?;
                }
                EntryType::Symlink => {
                    let link = entry
                        .link_name_bytes()
                        .filter(|link| {
                            !link.is_empty() && link.len() <= MAX_PATH && !link.contains(&0)
                        })
                        .ok_or("the stream has a link with a bad target")?;
                    std::os::unix::fs::symlink(OsStr::from_bytes(&link), &target)
                        .map_err(|e| fail(&target, "create", &e))?;
                    apply(&target, &meta, true)?;
                }
                EntryType::Link => {
                    let earlier = entry
                        .link_name_bytes()
                        .map(|link| link.to_vec())
                        .filter(|link| valid_entry_path(link) && link != b".")
                        .ok_or("the stream has a hard link to a path outside the layer")?;
                    let original = join(root, &earlier);
                    fs::hard_link(&original, &target).map_err(|e| fail(&target, "link", &e))?;
                }
                EntryType::Fifo => {
                    let p = c(&target)?;
                    // SAFETY: `p` is a valid path string.
                    if unsafe { libc::mkfifo(p.as_ptr(), 0o600) } != 0 {
                        return Err(os_err(&format!("create {}", target.display())));
                    }
                    apply(&target, &meta, false)?;
                }
                EntryType::Char => {
                    let header = entry.header();
                    if header.device_major().ok().flatten() != Some(0)
                        || header.device_minor().ok().flatten() != Some(0)
                    {
                        return Err("the stream has a device, which a layer may not hold".into());
                    }
                    mknod(&target, libc::S_IFCHR)?;
                    apply(&target, &meta, false)?;
                }
                _ => return Err("the stream has an entry of a kind a layer may not hold".into()),
            }
        }
        if count == 0 {
            return Err("the stream has no root folder".into());
        }
        while let Some(folder) = open.pop() {
            close(folder)?;
        }
        // Whatever follows the end of the archive is not part of a layer, but zeros (padding).
        let mut rest = archive.into_inner();
        let mut buffer = [0u8; 4096];
        loop {
            let got = rest
                .read(&mut buffer)
                .map_err(|e| io_err("read the stream", &e))?;
            if got == 0 {
                return Ok(());
            }
            if buffer[..got].iter().any(|byte| *byte != 0) {
                return Err("the stream has data after the end of the layer".into());
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::os::unix::fs::{symlink, PermissionsExt};

        fn put(root: &Path, path: &str, text: &str) {
            let full = root.join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, text).unwrap();
        }

        fn anywhere() -> Room {
            Room {
                min_free_bytes: 0,
                min_free_inodes: 0,
            }
        }

        fn round_trip(source: &Path, target: &Path) -> Result<(), String> {
            let mut stream = Vec::new();
            pack_tree(source, &mut stream)?;
            unpack_tree(target, stream.as_slice(), &anywhere())
        }

        #[test]
        fn a_layer_comes_back_with_its_files_links_modes_and_times() {
            let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
            put(a.path(), "etc/one", "hello");
            put(a.path(), "usr/bin/tool", "x");
            fs::set_permissions(
                a.path().join("usr/bin/tool"),
                fs::Permissions::from_mode(0o751),
            )
            .unwrap();
            symlink("tool", a.path().join("usr/bin/alias")).unwrap();
            symlink("/absolute/elsewhere", a.path().join("usr/bin/far")).unwrap();
            fs::hard_link(a.path().join("etc/one"), a.path().join("etc/two")).unwrap();
            fs::create_dir(a.path().join("empty")).unwrap();
            let long = "d".repeat(90);
            put(a.path(), &format!("{long}/{long}/{long}/file"), "deep");
            round_trip(a.path(), b.path()).unwrap();
            assert_eq!(
                fs::read_to_string(b.path().join("etc/one")).unwrap(),
                "hello"
            );
            assert_eq!(
                fs::read_to_string(b.path().join(format!("{long}/{long}/{long}/file"))).unwrap(),
                "deep"
            );
            assert_eq!(
                fs::metadata(b.path().join("usr/bin/tool")).unwrap().mode() & 0o7777,
                0o751
            );
            assert_eq!(
                fs::read_link(b.path().join("usr/bin/alias")).unwrap(),
                Path::new("tool")
            );
            assert_eq!(
                fs::read_link(b.path().join("usr/bin/far")).unwrap(),
                Path::new("/absolute/elsewhere")
            );
            assert_eq!(
                fs::metadata(b.path().join("etc/one")).unwrap().ino(),
                fs::metadata(b.path().join("etc/two")).unwrap().ino()
            );
            assert!(b.path().join("empty").is_dir());
            assert_eq!(
                fs::metadata(a.path().join("etc/one")).unwrap().mtime(),
                fs::metadata(b.path().join("etc/one")).unwrap().mtime()
            );
        }

        #[test]
        fn a_folder_keeps_its_time_and_mode_after_it_is_filled() {
            let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
            put(a.path(), "d/e/file", "x");
            put(a.path(), "d/other/file", "y");
            let old = std::time::SystemTime::now() - std::time::Duration::from_secs(86400 * 30);
            for dir in ["d", "d/e"] {
                fs::File::open(a.path().join(dir))
                    .unwrap()
                    .set_modified(old)
                    .unwrap();
            }
            fs::set_permissions(a.path().join("d/e"), fs::Permissions::from_mode(0o555)).unwrap();
            round_trip(a.path(), b.path()).unwrap();
            for dir in ["d", "d/e"] {
                assert_eq!(
                    fs::metadata(a.path().join(dir)).unwrap().mtime(),
                    fs::metadata(b.path().join(dir)).unwrap().mtime(),
                    "{dir}"
                );
            }
            assert_eq!(
                fs::metadata(b.path().join("d/e")).unwrap().mode() & 0o7777,
                0o555
            );
            fs::set_permissions(a.path().join("d/e"), fs::Permissions::from_mode(0o755)).unwrap();
            fs::set_permissions(b.path().join("d/e"), fs::Permissions::from_mode(0o755)).unwrap();
        }

        #[test]
        fn a_whiteout_and_an_opaque_folder_travel_as_they_are() {
            let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
            put(a.path(), "d/new", "n");
            // Only root (or a user namespace) may make the device or the attribute.
            if mknod(&a.path().join("gone"), libc::S_IFCHR).is_err() {
                return;
            }
            let d = c(&a.path().join("d")).unwrap();
            let name = CString::new("user.overlay.opaque").unwrap();
            // SAFETY: valid strings and a one-byte value.
            if unsafe { libc::lsetxattr(d.as_ptr(), name.as_ptr(), b"y".as_ptr().cast(), 1, 0) }
                != 0
            {
                return;
            }
            // Overlayfs's own record on a layer it has mounted stays behind.
            let root = c(a.path()).unwrap();
            let uuid = CString::new("user.overlay.uuid").unwrap();
            // SAFETY: valid strings and a one-byte value.
            assert_eq!(
                unsafe {
                    libc::lsetxattr(root.as_ptr(), uuid.as_ptr(), b"u".as_ptr().cast(), 1, 0)
                },
                0
            );
            round_trip(a.path(), b.path()).unwrap();
            assert!(xattr_pairs(b.path())
                .unwrap()
                .iter()
                .all(|(name, _)| name != b"user.overlay.uuid"));
            let meta = fs::symlink_metadata(b.path().join("gone")).unwrap();
            assert!(meta.file_type().is_char_device() && meta.rdev() == 0);
            assert!(xattr_pairs(&b.path().join("d"))
                .unwrap()
                .contains(&(b"user.overlay.opaque".to_vec(), b"y".to_vec())));
        }

        fn root_header() -> Header {
            let mut root = Header::new_gnu();
            root.set_entry_type(EntryType::Directory);
            root.set_mode(0o755);
            root.set_size(0);
            root.set_mtime(1);
            root.set_uid(unsafe { libc::getuid() }.into());
            root.set_gid(unsafe { libc::getgid() }.into());
            root
        }

        fn stream_with(entries: impl FnOnce(&mut Builder<Vec<u8>>)) -> Vec<u8> {
            let mut builder = Builder::new(Vec::new());
            builder
                .append_data(&mut root_header(), ".", io::empty())
                .unwrap();
            entries(&mut builder);
            builder.into_inner().unwrap()
        }

        fn mine(kind: EntryType, size: u64) -> Header {
            let mut header = root_header();
            header.set_entry_type(kind);
            header.set_mode(0o644);
            header.set_size(size);
            header
        }

        #[test]
        fn an_attribute_travels_and_the_overlays_other_attributes_are_refused() {
            let (a, b, c2) = (
                tempfile::tempdir().unwrap(),
                tempfile::tempdir().unwrap(),
                tempfile::tempdir().unwrap(),
            );
            put(a.path(), "f", "x");
            let f = c(&a.path().join("f")).unwrap();
            let name = CString::new("user.note").unwrap();
            // SAFETY: valid strings and a one-byte value.
            if unsafe { libc::lsetxattr(f.as_ptr(), name.as_ptr(), b"v".as_ptr().cast(), 1, 0) }
                != 0
            {
                return; // the file system keeps no user attributes
            }
            round_trip(a.path(), b.path()).unwrap();
            assert!(xattr_pairs(&b.path().join("f"))
                .unwrap()
                .contains(&(b"user.note".to_vec(), b"v".to_vec())));
            // A stream made by hand with an overlay attribute that redirects to a lower folder.
            let stream = stream_with(|out| {
                out.append_pax_extensions([("SCHILY.xattr.user.overlay.redirect", &b"/etc"[..])])
                    .unwrap();
                out.append_data(&mut mine(EntryType::Directory, 0), "d", io::empty())
                    .unwrap();
            });
            let error = unpack_tree(c2.path(), stream.as_slice(), &anywhere()).unwrap_err();
            assert!(error.contains("overlay attribute"), "{error}");
        }

        #[test]
        fn the_unpack_stops_when_the_disk_has_less_than_the_margin() {
            let (a, b, c2) = (
                tempfile::tempdir().unwrap(),
                tempfile::tempdir().unwrap(),
                tempfile::tempdir().unwrap(),
            );
            put(a.path(), "big", &"x".repeat(100_000));
            let mut stream = Vec::new();
            pack_tree(a.path(), &mut stream).unwrap();
            let all = Room {
                min_free_bytes: u64::MAX / 2,
                min_free_inodes: 0,
            };
            let error = unpack_tree(b.path(), stream.as_slice(), &all).unwrap_err();
            assert!(error.contains("room"), "{error}");
            assert!(!b.path().join("big").exists());
            let inodes = Room {
                min_free_bytes: 0,
                min_free_inodes: u64::MAX,
            };
            // A file system without an inode count (btrfs) cannot refuse on it.
            if let Err(error) = unpack_tree(c2.path(), stream.as_slice(), &inodes) {
                assert!(error.contains("files to make"), "{error}");
            }
        }

        #[test]
        fn a_stream_that_names_a_path_outside_the_layer_is_refused() {
            for path in ["../escape", "/etc/passwd", "a/../../b", "a//b", "a/./b"] {
                let target = tempfile::tempdir().unwrap();
                // The tar crate refuses to write such a name, so the header is written raw.
                let stream = stream_with(|out| {
                    let mut header = mine(EntryType::Regular, 1);
                    header.as_old_mut().name[..path.len()].copy_from_slice(path.as_bytes());
                    header.set_cksum();
                    out.append(&header, &b"x"[..]).unwrap();
                });
                let error = unpack_tree(target.path(), stream.as_slice(), &anywhere()).unwrap_err();
                assert!(error.contains("outside the layer"), "{path:?}: {error}");
            }
            let target = tempfile::tempdir().unwrap();
            let stream = stream_with(|out| {
                let mut header = mine(EntryType::Link, 0);
                let link = b"../../etc/passwd";
                header.as_old_mut().name[0] = b'x';
                header.as_old_mut().linkname[..link.len()].copy_from_slice(link);
                header.set_cksum();
                out.append(&header, io::empty()).unwrap();
            });
            assert!(unpack_tree(target.path(), stream.as_slice(), &anywhere()).is_err());
        }

        #[test]
        fn a_device_or_a_second_root_is_refused() {
            let target = tempfile::tempdir().unwrap();
            let stream = stream_with(|out| {
                let mut header = mine(EntryType::Char, 0);
                header.set_device_major(1).unwrap();
                header.set_device_minor(3).unwrap();
                out.append_data(&mut header, "null", io::empty()).unwrap();
            });
            assert!(unpack_tree(target.path(), stream.as_slice(), &anywhere()).is_err());
            let target = tempfile::tempdir().unwrap();
            let stream = stream_with(|out| {
                out.append_data(&mut mine(EntryType::Directory, 0), ".", io::empty())
                    .unwrap();
            });
            assert!(unpack_tree(target.path(), stream.as_slice(), &anywhere()).is_err());
        }

        #[test]
        fn a_broken_stream_is_an_error_and_not_a_partial_success() {
            let a = tempfile::tempdir().unwrap();
            put(a.path(), "f", &"some content".repeat(100));
            let mut stream = Vec::new();
            pack_tree(a.path(), &mut stream).unwrap();
            // Inside a header and inside a file's bytes.
            for cut in [100, 600, 700] {
                let target = tempfile::tempdir().unwrap();
                assert!(
                    unpack_tree(target.path(), &stream[..cut], &anywhere()).is_err(),
                    "cut at {cut}"
                );
            }
            let target = tempfile::tempdir().unwrap();
            assert!(unpack_tree(target.path(), &stream[..0], &anywhere()).is_err());
            let mut padded = stream.clone();
            padded.extend_from_slice(&[0; 2048]);
            let target = tempfile::tempdir().unwrap();
            unpack_tree(target.path(), padded.as_slice(), &anywhere()).unwrap();
            padded.push(1);
            let target = tempfile::tempdir().unwrap();
            let error = unpack_tree(target.path(), padded.as_slice(), &anywhere()).unwrap_err();
            assert!(error.contains("after the end"), "{error}");
        }

        #[test]
        fn a_socket_is_left_out_and_a_file_that_exists_is_not_replaced() {
            let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
            put(a.path(), "f", "x");
            let _socket = std::os::unix::net::UnixListener::bind(a.path().join("sock")).unwrap();
            put(b.path(), "f", "already here");
            let error = round_trip(a.path(), b.path()).unwrap_err();
            assert!(error.contains("create"), "{error}");
            assert_eq!(
                fs::read_to_string(b.path().join("f")).unwrap(),
                "already here"
            );
            let c2 = tempfile::tempdir().unwrap();
            round_trip(a.path(), c2.path()).unwrap();
            assert!(c2.path().join("f").exists());
            assert!(fs::symlink_metadata(c2.path().join("sock")).is_err());
        }

        #[test]
        fn a_symlink_in_the_stream_is_not_followed_for_the_entries_after_it() {
            // The unpack runs with the layer as its root, so a link can only name layer files;
            // this checks the plain part: the later entry is created, not skipped.
            let target = tempfile::tempdir().unwrap();
            fs::create_dir(target.path().join("real")).unwrap();
            let stream = stream_with(|out| {
                out.append_link(&mut mine(EntryType::Symlink, 0), "link", "real")
                    .unwrap();
            });
            unpack_tree(target.path(), stream.as_slice(), &anywhere()).unwrap();
            assert_eq!(
                fs::read_link(target.path().join("link")).unwrap(),
                Path::new("real")
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_unpack_options_are_the_margins_and_one_layer() {
        let parsed = parse_unpack_args(&args(&[
            "--min-free-bytes",
            "10",
            "--min-free-inodes",
            "3",
            "/l",
        ]))
        .unwrap();
        assert_eq!(
            parsed,
            UnpackArgs {
                min_free_bytes: 10,
                min_free_inodes: 3,
                layer: "/l".into()
            }
        );
        assert_eq!(parse_unpack_args(&args(&["/l"])).unwrap().min_free_bytes, 0);
        for bad in [
            args(&[]),
            args(&["--min-free-bytes", "x", "/l"]),
            args(&["--min-free-bytes"]),
            args(&["/a", "/b"]),
            args(&["--bogus", "/l"]),
            args(&["--max-bytes", "1", "/l"]),
        ] {
            assert!(parse_unpack_args(&bad).is_err(), "{bad:?}");
        }
        assert_eq!(parse_pack_args(&args(&["/l"])).unwrap(), "/l");
        assert!(parse_pack_args(&args(&["/a", "/b"])).is_err());
        assert!(parse_pack_args(&args(&[])).is_err());
    }

    #[test]
    fn paths_are_relative_and_made_of_ordinary_names() {
        for good in ["a", "a/b", ".", "a.b/.c", "..x"] {
            assert!(valid_entry_path(good.as_bytes()), "{good}");
        }
        for bad in [
            "", "/a", "a/", "a//b", "../a", "a/..", "a/./b", "./a", "a\0b",
        ] {
            assert!(!valid_entry_path(bad.as_bytes()), "{bad:?}");
        }
        assert!(!valid_entry_path(&vec![b'a'; MAX_PATH + 1]));
    }

    #[test]
    fn tar_names_become_layer_paths_only_in_the_forms_tar_writes() {
        assert_eq!(entry_path(b"./", true).unwrap(), b".");
        assert_eq!(entry_path(b".", true).unwrap(), b".");
        assert_eq!(entry_path(b"a/b/", true).unwrap(), b"a/b");
        assert_eq!(entry_path(b"a/b", false).unwrap(), b"a/b");
        for (raw, dir) in [
            (&b"a/"[..], false),
            (b"/a", true),
            (b"../a", false),
            (b"a//", true),
            (b"./a", false),
        ] {
            assert!(entry_path(raw, dir).is_err(), "{raw:?}");
        }
    }

    #[test]
    fn only_the_opaque_overlay_attribute_may_be_set() {
        assert!(xattr_allowed(b"user.overlay.opaque").is_ok());
        assert!(xattr_allowed(b"user.note").is_ok());
        assert!(xattr_allowed(b"security.capability").is_ok());
        for bad in [
            &b"user.overlay.redirect"[..],
            b"user.overlay.origin",
            b"user.overlay.metacopy",
            b"user.overlay.opaquex",
            b"",
            b"a\0b",
        ] {
            assert!(xattr_allowed(bad).is_err(), "{bad:?}");
        }
        assert!(xattr_allowed(&[b'a'; 256]).is_err());
    }

    #[test]
    fn a_folder_holds_what_is_below_it() {
        assert!(holds(b".", b"a"));
        assert!(holds(b"a", b"a/b"));
        assert!(!holds(b"a", b"a"));
        assert!(!holds(b"a", b"ab/c"));
        assert!(!holds(b"a/b", b"a/c"));
    }
}
