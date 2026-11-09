//! `silo-ctr pack LAYER` and `silo-ctr unpack [--max-bytes N] [--max-entries N] LAYER`: one overlay
//! layer to a byte stream and back, for the export and import of a container computer. Both run
//! inside the container's user namespace (`silo-ctr userns --keep-id N -- silo-ctr pack ...`),
//! where the layer's owners are real and a whiteout can be made.
//!
//! `pack` writes the layer's tree to standard output and `unpack` reads it from standard input
//! into the empty layer folder it is given. An import reads a file somebody else made, so
//! `unpack` first changes its root to the layer: whatever the stream contains, a link in it can
//! only point at the layer's own files, and nothing outside the layer can be named. It also stops
//! at a byte or entry limit the caller sets, before it writes past it.
//!
//! The stream (all numbers big-endian) is a magic line, then one record per entry, then an end
//! record. An entry is its kind (a letter), its path relative to the layer (`.` for the layer
//! itself), mode, owner and group, modification time and extended attributes, then what the kind
//! carries: a file's size and bytes, a symbolic link's target, a hard link's earlier path.
//! Whiteouts (character device 0:0) and the `user.overlay.opaque` attribute travel as they are,
//! because the layers below may still hold the files they hide. Sockets are left out; any other
//! device is refused.

use std::io::{self, Read, Write};

/// The first bytes of a stream.
pub const MAGIC: &[u8; 16] = b"SILO-CTR-LAYER1\n";
const DIR: u8 = b'D';
const FILE: u8 = b'F';
const SYMLINK: u8 = b'S';
const HARDLINK: u8 = b'H';
const WHITEOUT: u8 = b'W';
const FIFO: u8 = b'P';
const END: u8 = b'E';

const MAX_PATH: u32 = 4096;
const MAX_XATTRS: u32 = 1024;
const MAX_XATTR_NAME: u16 = 255;
const MAX_XATTR_VALUE: u32 = 65536;
/// What one entry's bookkeeping is counted as against the byte limit, besides its content.
const ENTRY_COST: u64 = 512;

/// The options of an `unpack` command line: the byte limit, the entry limit and the layer.
#[derive(Debug, PartialEq)]
pub struct UnpackArgs {
    pub max_bytes: u64,
    pub max_entries: u64,
    pub layer: String,
}

pub fn parse_unpack_args(args: &[String]) -> Result<UnpackArgs, String> {
    let mut parsed = UnpackArgs {
        max_bytes: u64::MAX,
        max_entries: u64::MAX,
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
            "--max-bytes" => parsed.max_bytes = number("--max-bytes")?,
            "--max-entries" => parsed.max_entries = number("--max-entries")?,
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
        && path.len() <= MAX_PATH as usize
        && !path.contains(&0)
        && path
            .split(|byte| *byte == b'/')
            .all(|name| !name.is_empty() && name != b"." && name != b"..")
}

fn put_u16(out: &mut impl Write, value: u16) -> io::Result<()> {
    out.write_all(&value.to_be_bytes())
}
fn put_u32(out: &mut impl Write, value: u32) -> io::Result<()> {
    out.write_all(&value.to_be_bytes())
}
fn put_u64(out: &mut impl Write, value: u64) -> io::Result<()> {
    out.write_all(&value.to_be_bytes())
}
fn put_bytes(out: &mut impl Write, bytes: &[u8]) -> io::Result<()> {
    put_u32(
        out,
        u32::try_from(bytes.len()).map_err(|_| io::Error::other("a name is too long"))?,
    )?;
    out.write_all(bytes)
}

fn get<const N: usize>(input: &mut impl Read) -> io::Result<[u8; N]> {
    let mut buffer = [0u8; N];
    input.read_exact(&mut buffer)?;
    Ok(buffer)
}
fn get_u8(input: &mut impl Read) -> io::Result<u8> {
    Ok(get::<1>(input)?[0])
}
fn get_u16(input: &mut impl Read) -> io::Result<u16> {
    Ok(u16::from_be_bytes(get(input)?))
}
fn get_u32(input: &mut impl Read) -> io::Result<u32> {
    Ok(u32::from_be_bytes(get(input)?))
}
fn get_u64(input: &mut impl Read) -> io::Result<u64> {
    Ok(u64::from_be_bytes(get(input)?))
}
fn get_i64(input: &mut impl Read) -> io::Result<i64> {
    Ok(i64::from_be_bytes(get(input)?))
}

fn bad(detail: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, detail.to_owned())
}

/// A length-prefixed byte string of at most `max` bytes.
fn get_bytes(input: &mut impl Read, max: u32) -> io::Result<Vec<u8>> {
    let length = get_u32(input)?;
    if length > max {
        return Err(bad("the stream has a name or value that is too long"));
    }
    let mut bytes = vec![0u8; length as usize];
    input.read_exact(&mut bytes)?;
    Ok(bytes)
}

/// The attributes every entry but a hard link carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attributes {
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub mtime_sec: i64,
    pub mtime_nsec: u32,
    pub xattrs: Vec<(Vec<u8>, Vec<u8>)>,
}

fn put_attributes(out: &mut impl Write, attributes: &Attributes) -> io::Result<()> {
    put_u32(out, attributes.mode)?;
    put_u32(out, attributes.uid)?;
    put_u32(out, attributes.gid)?;
    out.write_all(&attributes.mtime_sec.to_be_bytes())?;
    put_u32(out, attributes.mtime_nsec)?;
    put_u32(out, attributes.xattrs.len() as u32)?;
    for (name, value) in &attributes.xattrs {
        put_u16(out, name.len() as u16)?;
        out.write_all(name)?;
        put_bytes(out, value)?;
    }
    Ok(())
}

fn get_attributes(input: &mut impl Read) -> io::Result<Attributes> {
    let mode = get_u32(input)?;
    let uid = get_u32(input)?;
    let gid = get_u32(input)?;
    let mtime_sec = get_i64(input)?;
    let mtime_nsec = get_u32(input)?;
    if mtime_nsec >= 1_000_000_000 || mode > 0o7777 {
        return Err(bad(
            "the stream has an entry with an impossible mode or time",
        ));
    }
    let count = get_u32(input)?;
    if count > MAX_XATTRS {
        return Err(bad("the stream has an entry with too many attributes"));
    }
    let mut xattrs = Vec::new();
    for _ in 0..count {
        let length = get_u16(input)?;
        if length == 0 || length > MAX_XATTR_NAME {
            return Err(bad("the stream has an attribute with a bad name"));
        }
        let mut name = vec![0u8; length as usize];
        input.read_exact(&mut name)?;
        if name.contains(&0) {
            return Err(bad("the stream has an attribute with a bad name"));
        }
        xattrs.push((name, get_bytes(input, MAX_XATTR_VALUE)?));
    }
    Ok(Attributes {
        mode,
        uid,
        gid,
        mtime_sec,
        mtime_nsec,
        xattrs,
    })
}

/// One entry of a stream, as `unpack` reads it. A file's bytes follow the record in the stream
/// and are read by [`copy_content`].
#[derive(Debug, PartialEq, Eq)]
pub enum Entry {
    Dir(Vec<u8>, Attributes),
    File(Vec<u8>, Attributes, u64),
    Symlink(Vec<u8>, Attributes, Vec<u8>),
    Hardlink(Vec<u8>, Vec<u8>),
    Whiteout(Vec<u8>, Attributes),
    Fifo(Vec<u8>, Attributes),
    End,
}

fn put_path(out: &mut impl Write, tag: u8, path: &[u8]) -> io::Result<()> {
    out.write_all(&[tag])?;
    put_bytes(out, path)
}

pub fn write_header(out: &mut impl Write) -> io::Result<()> {
    out.write_all(MAGIC)
}

pub fn write_end(out: &mut impl Write) -> io::Result<()> {
    out.write_all(&[END])
}

pub fn write_dir(out: &mut impl Write, path: &[u8], attributes: &Attributes) -> io::Result<()> {
    put_path(out, DIR, path)?;
    put_attributes(out, attributes)
}

pub fn write_file_start(
    out: &mut impl Write,
    path: &[u8],
    attributes: &Attributes,
    size: u64,
) -> io::Result<()> {
    put_path(out, FILE, path)?;
    put_attributes(out, attributes)?;
    put_u64(out, size)
}

pub fn write_symlink(
    out: &mut impl Write,
    path: &[u8],
    attributes: &Attributes,
    target: &[u8],
) -> io::Result<()> {
    put_path(out, SYMLINK, path)?;
    put_attributes(out, attributes)?;
    put_bytes(out, target)
}

pub fn write_hardlink(out: &mut impl Write, path: &[u8], target: &[u8]) -> io::Result<()> {
    put_path(out, HARDLINK, path)?;
    put_bytes(out, target)
}

pub fn write_whiteout(
    out: &mut impl Write,
    path: &[u8],
    attributes: &Attributes,
) -> io::Result<()> {
    put_path(out, WHITEOUT, path)?;
    put_attributes(out, attributes)
}

pub fn write_fifo(out: &mut impl Write, path: &[u8], attributes: &Attributes) -> io::Result<()> {
    put_path(out, FIFO, path)?;
    put_attributes(out, attributes)
}

/// Reads the magic line.
pub fn read_header(input: &mut impl Read) -> io::Result<()> {
    if &get::<16>(input)? != MAGIC {
        return Err(bad("the stream is not a container layer"));
    }
    Ok(())
}

/// The next entry. Paths are checked; a file's bytes are left in `input`.
pub fn read_entry(input: &mut impl Read) -> io::Result<Entry> {
    let tag = get_u8(input)?;
    if tag == END {
        return Ok(Entry::End);
    }
    let path = get_bytes(input, MAX_PATH)?;
    if !valid_entry_path(&path) {
        return Err(bad("the stream names a path outside the layer"));
    }
    if path == b"." && tag != DIR {
        return Err(bad("the stream's root is not a folder"));
    }
    Ok(match tag {
        DIR => Entry::Dir(path, get_attributes(input)?),
        FILE => {
            let attributes = get_attributes(input)?;
            Entry::File(path, attributes, get_u64(input)?)
        }
        SYMLINK => {
            let attributes = get_attributes(input)?;
            let target = get_bytes(input, MAX_PATH)?;
            if target.is_empty() || target.contains(&0) {
                return Err(bad("the stream has a link with an empty target"));
            }
            Entry::Symlink(path, attributes, target)
        }
        HARDLINK => {
            let target = get_bytes(input, MAX_PATH)?;
            if !valid_entry_path(&target) || target == b"." {
                return Err(bad(
                    "the stream has a hard link to a path outside the layer",
                ));
            }
            Entry::Hardlink(path, target)
        }
        WHITEOUT => Entry::Whiteout(path, get_attributes(input)?),
        FIFO => Entry::Fifo(path, get_attributes(input)?),
        _ => return Err(bad("the stream has an entry of an unknown kind")),
    })
}

/// Copies exactly `size` bytes, failing when the stream ends first.
pub fn copy_content(input: &mut impl Read, out: &mut impl Write, size: u64) -> io::Result<()> {
    let copied = io::copy(&mut input.take(size), out)?;
    if copied != size {
        return Err(bad("the stream ends inside a file"));
    }
    Ok(())
}

/// The running count against the limits of an `unpack`.
#[derive(Debug, Default)]
pub struct Budget {
    pub max_bytes: u64,
    pub max_entries: u64,
    entries: u64,
    bytes: u64,
}

impl Budget {
    pub fn new(max_bytes: u64, max_entries: u64) -> Self {
        Self {
            max_bytes,
            max_entries,
            entries: 0,
            bytes: 0,
        }
    }

    /// Counts one entry of `content` bytes; an error once a limit is passed.
    pub fn charge(&mut self, content: u64) -> Result<(), String> {
        self.entries += 1;
        self.bytes = self
            .bytes
            .saturating_add(content)
            .saturating_add(ENTRY_COST);
        if self.entries > self.max_entries {
            return Err(format!(
                "the layer has more than {} entries, more than this device allows",
                self.max_entries
            ));
        }
        if self.bytes > self.max_bytes {
            return Err("the layer needs more space than this device has for it".into());
        }
        Ok(())
    }
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
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    use std::path::{Path, PathBuf};

    fn c(path: &Path) -> Result<CString, String> {
        CString::new(path.as_os_str().as_bytes()).map_err(|_| "a path contains a NUL byte".into())
    }

    fn xattr_pairs(path: &Path) -> Result<Vec<(Vec<u8>, Vec<u8>)>, String> {
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
        {
            let n = CString::new(name).map_err(|_| "an attribute name contains NUL")?;
            let mut value = vec![0u8; MAX_XATTR_VALUE as usize];
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

    fn attributes_of(path: &Path, meta: &fs::Metadata) -> Result<Attributes, String> {
        Ok(Attributes {
            mode: meta.mode() & 0o7777,
            uid: meta.uid(),
            gid: meta.gid(),
            mtime_sec: meta.mtime(),
            mtime_nsec: meta.mtime_nsec() as u32,
            xattrs: xattr_pairs(path)?,
        })
    }

    fn relative(root: &Path, path: &Path) -> Vec<u8> {
        match path.strip_prefix(root) {
            Ok(rest) if rest.as_os_str().is_empty() => b".".to_vec(),
            Ok(rest) => rest.as_os_str().as_bytes().to_vec(),
            Err(_) => b".".to_vec(),
        }
    }

    fn write_err(error: io::Error) -> String {
        io_err("write the stream", &error)
    }

    pub fn pack_command(arguments: &[String]) -> Result<i32, String> {
        let layer = parse_pack_args(arguments)?;
        let stdout = io::stdout();
        let mut out = io::BufWriter::with_capacity(1 << 20, stdout.lock());
        pack_layer(Path::new(&layer), &mut out)?;
        out.flush().map_err(write_err)?;
        Ok(0)
    }

    /// The whole tree of `root` onto `out`.
    pub fn pack_layer(root: &Path, out: &mut impl Write) -> Result<(), String> {
        let meta = fs::symlink_metadata(root)
            .map_err(|e| io_err(&format!("look at {}", root.display()), &e))?;
        if !meta.is_dir() {
            return Err(format!("{} is not a folder", root.display()));
        }
        write_header(out).map_err(write_err)?;
        let mut links = HashMap::new();
        pack_dir(root, root, &meta, &mut links, out)?;
        write_end(out).map_err(write_err)
    }

    fn pack_dir(
        root: &Path,
        dir: &Path,
        meta: &fs::Metadata,
        links: &mut HashMap<(u64, u64), Vec<u8>>,
        out: &mut impl Write,
    ) -> Result<(), String> {
        let attributes = attributes_of(dir, meta)?;
        write_dir(out, &relative(root, dir), &attributes).map_err(write_err)?;
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
                pack_dir(root, &path, &meta, links, out)?;
                continue;
            }
            if kind.is_socket() {
                continue;
            }
            if !kind.is_symlink() && meta.nlink() > 1 && !kind.is_dir() {
                let key = (meta.dev(), meta.ino());
                if let Some(first) = links.get(&key) {
                    write_hardlink(out, &rel, first).map_err(write_err)?;
                    continue;
                }
                links.insert(key, rel.clone());
            }
            let attributes = attributes_of(&path, &meta)?;
            if kind.is_symlink() {
                let target = fs::read_link(&path)
                    .map_err(|e| io_err(&format!("read {}", path.display()), &e))?;
                write_symlink(out, &rel, &attributes, target.as_os_str().as_bytes())
                    .map_err(write_err)?;
            } else if kind.is_file() {
                write_file_start(out, &rel, &attributes, meta.len()).map_err(write_err)?;
                let mut file = fs::File::open(&path)
                    .map_err(|e| io_err(&format!("open {}", path.display()), &e))?;
                // A file that changes while it is read is a file that is not at rest: the
                // caller packs checkpoints, which are.
                let copied = io::copy(&mut (&mut file).take(meta.len()), out)
                    .map_err(|e| io_err(&format!("read {}", path.display()), &e))?;
                if copied != meta.len() {
                    return Err(format!("{} changed while it was read", path.display()));
                }
            } else if kind.is_fifo() {
                write_fifo(out, &rel, &attributes).map_err(write_err)?;
            } else if kind.is_char_device() && meta.rdev() == 0 {
                write_whiteout(out, &rel, &attributes).map_err(write_err)?;
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
        let layer = PathBuf::from(OsStr::new(&parsed.layer));
        // The layer becomes the whole world of this process.
        let path = c(&layer)?;
        // SAFETY: valid path strings; the calls change only this process's root and directory.
        unsafe {
            if libc::chroot(path.as_ptr()) != 0 {
                return Err(os_err(&format!("enter {}", layer.display())));
            }
            let root = CString::new("/").expect("no NUL");
            if libc::chdir(root.as_ptr()) != 0 {
                return Err(os_err("change to the layer's root"));
            }
        }
        let stdin = io::stdin();
        let mut input = io::BufReader::with_capacity(1 << 20, stdin.lock());
        unpack_layer(
            Path::new("/"),
            &mut input,
            Budget::new(parsed.max_bytes, parsed.max_entries),
        )?;
        Ok(0)
    }

    fn join(root: &Path, relative: &[u8]) -> PathBuf {
        if relative == b"." {
            root.to_path_buf()
        } else {
            root.join(OsStr::from_bytes(relative))
        }
    }

    fn set_xattrs(path: &Path, xattrs: &[(Vec<u8>, Vec<u8>)]) -> Result<(), String> {
        let p = c(path)?;
        for (name, value) in xattrs {
            let n = CString::new(name.clone()).map_err(|_| "an attribute name contains NUL")?;
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

    /// Owner, attributes, mode and time, in the order that keeps each from undoing another.
    fn apply(path: &Path, attributes: &Attributes, is_link: bool) -> Result<(), String> {
        let p = c(path)?;
        // SAFETY: `p` is a valid path string.
        if unsafe { libc::lchown(p.as_ptr(), attributes.uid, attributes.gid) } != 0 {
            return Err(os_err(&format!("change the owner of {}", path.display())));
        }
        set_xattrs(path, &attributes.xattrs)?;
        if !is_link {
            // SAFETY: `p` is a valid path string.
            if unsafe { libc::chmod(p.as_ptr(), attributes.mode as libc::mode_t) } != 0 {
                return Err(os_err(&format!("set the mode of {}", path.display())));
            }
        }
        let times = [
            libc::timespec {
                tv_sec: attributes.mtime_sec,
                tv_nsec: libc::UTIME_OMIT,
            },
            libc::timespec {
                tv_sec: attributes.mtime_sec,
                tv_nsec: attributes.mtime_nsec as _,
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

    /// Reads a stream into `root`, which must be an empty folder (the layer itself, once this
    /// process has made it its root). Nothing is written past `budget`.
    pub fn unpack_layer(
        root: &Path,
        input: &mut impl Read,
        mut budget: Budget,
    ) -> Result<(), String> {
        read_header(input).map_err(|e| e.to_string())?;
        // Folders get their own mode and time last, so that filling them does not change either.
        let mut folders: Vec<(PathBuf, Attributes)> = Vec::new();
        let mut seen_root = false;
        loop {
            match read_entry(input).map_err(|e| io_err("read the stream", &e))? {
                Entry::End => break,
                Entry::Dir(path, attributes) => {
                    budget.charge(0)?;
                    let target = join(root, &path);
                    if path == b"." {
                        seen_root = true;
                    } else {
                        fs::create_dir(&target).map_err(|e| fail(&target, "create", &e))?;
                    }
                    folders.push((target, attributes));
                }
                Entry::File(path, attributes, size) => {
                    budget.charge(size)?;
                    let target = join(root, &path);
                    let mut file = fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&target)
                        .map_err(|e| fail(&target, "create", &e))?;
                    copy_content(input, &mut file, size).map_err(|e| fail(&target, "write", &e))?;
                    drop(file);
                    apply(&target, &attributes, false)?;
                }
                Entry::Symlink(path, attributes, link_target) => {
                    budget.charge(0)?;
                    let target = join(root, &path);
                    std::os::unix::fs::symlink(OsStr::from_bytes(&link_target), &target)
                        .map_err(|e| fail(&target, "create", &e))?;
                    apply(&target, &attributes, true)?;
                }
                Entry::Hardlink(path, earlier) => {
                    budget.charge(0)?;
                    let (target, original) = (join(root, &path), join(root, &earlier));
                    fs::hard_link(&original, &target).map_err(|e| fail(&target, "link", &e))?;
                }
                Entry::Whiteout(path, attributes) => {
                    budget.charge(0)?;
                    let target = join(root, &path);
                    mknod(&target, libc::S_IFCHR as u32)?;
                    apply(&target, &attributes, false)?;
                }
                Entry::Fifo(path, attributes) => {
                    budget.charge(0)?;
                    let target = join(root, &path);
                    let p = c(&target)?;
                    // SAFETY: `p` is a valid path string.
                    if unsafe { libc::mkfifo(p.as_ptr(), 0o600) } != 0 {
                        return Err(os_err(&format!("create {}", target.display())));
                    }
                    apply(&target, &attributes, false)?;
                }
            }
        }
        if !seen_root {
            return Err("the stream has no root folder".into());
        }
        while let Some((path, attributes)) = folders.pop() {
            apply(&path, &attributes, false)?;
        }
        // Whatever follows the end record is not part of a layer.
        let mut extra = [0u8; 1];
        if input
            .read(&mut extra)
            .map_err(|e| io_err("read the stream", &e))?
            != 0
        {
            return Err("the stream has data after the end of the layer".into());
        }
        Ok(())
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

        fn round_trip(source: &Path, target: &Path) -> Result<(), String> {
            let mut stream = Vec::new();
            pack_layer(source, &mut stream)?;
            unpack_layer(
                target,
                &mut stream.as_slice(),
                Budget::new(u64::MAX, u64::MAX),
            )
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
            round_trip(a.path(), b.path()).unwrap();
            assert_eq!(
                fs::read_to_string(b.path().join("etc/one")).unwrap(),
                "hello"
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
        fn a_folder_keeps_its_time_after_it_is_filled() {
            let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
            put(a.path(), "d/file", "x");
            let old = std::time::SystemTime::now() - std::time::Duration::from_secs(86400 * 30);
            fs::File::open(a.path().join("d"))
                .unwrap()
                .set_modified(old)
                .unwrap();
            round_trip(a.path(), b.path()).unwrap();
            assert_eq!(
                fs::metadata(a.path().join("d")).unwrap().mtime(),
                fs::metadata(b.path().join("d")).unwrap().mtime()
            );
        }

        #[test]
        fn a_whiteout_and_an_opaque_folder_travel_as_they_are() {
            let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
            put(a.path(), "d/new", "n");
            // Only root (or a user namespace) may make the device or the attribute.
            if mknod(&a.path().join("gone"), libc::S_IFCHR as u32).is_err() {
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
            round_trip(a.path(), b.path()).unwrap();
            let meta = fs::symlink_metadata(b.path().join("gone")).unwrap();
            assert!(meta.file_type().is_char_device() && meta.rdev() == 0);
            assert!(xattr_pairs(&b.path().join("d"))
                .unwrap()
                .contains(&(b"user.overlay.opaque".to_vec(), b"y".to_vec())));
        }

        #[test]
        fn the_limits_stop_the_unpack_before_it_writes_past_them() {
            let (a, b, c2) = (
                tempfile::tempdir().unwrap(),
                tempfile::tempdir().unwrap(),
                tempfile::tempdir().unwrap(),
            );
            put(a.path(), "big", &"x".repeat(100_000));
            let mut stream = Vec::new();
            pack_layer(a.path(), &mut stream).unwrap();
            let error = unpack_layer(b.path(), &mut stream.as_slice(), Budget::new(50_000, 100))
                .unwrap_err();
            assert!(error.contains("more space"), "{error}");
            assert!(!b.path().join("big").exists());
            put(a.path(), "second", "x");
            let mut stream = Vec::new();
            pack_layer(a.path(), &mut stream).unwrap();
            let error = unpack_layer(c2.path(), &mut stream.as_slice(), Budget::new(u64::MAX, 2))
                .unwrap_err();
            assert!(error.contains("more than 2 entries"), "{error}");
        }

        fn stream_with(entries: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
            let mut stream = Vec::new();
            write_header(&mut stream).unwrap();
            let root = Attributes {
                mode: 0o755,
                uid: unsafe { libc::getuid() },
                gid: unsafe { libc::getgid() },
                mtime_sec: 1,
                mtime_nsec: 0,
                xattrs: Vec::new(),
            };
            write_dir(&mut stream, b".", &root).unwrap();
            entries(&mut stream);
            write_end(&mut stream).unwrap();
            stream
        }

        fn mine() -> Attributes {
            Attributes {
                mode: 0o644,
                uid: unsafe { libc::getuid() },
                gid: unsafe { libc::getgid() },
                mtime_sec: 1,
                mtime_nsec: 0,
                xattrs: Vec::new(),
            }
        }

        #[test]
        fn a_stream_that_names_a_path_outside_the_layer_is_refused() {
            for path in [
                &b"../escape"[..],
                b"/etc/passwd",
                b"a/../../b",
                b"a//b",
                b"",
                b"a/./b",
            ] {
                let target = tempfile::tempdir().unwrap();
                let stream = stream_with(|out| {
                    write_file_start(out, path, &mine(), 1).unwrap();
                    out.push(b'x');
                });
                let error = unpack_layer(
                    target.path(),
                    &mut stream.as_slice(),
                    Budget::new(u64::MAX, u64::MAX),
                )
                .unwrap_err();
                assert!(error.contains("outside the layer"), "{path:?}: {error}");
            }
            let target = tempfile::tempdir().unwrap();
            let stream = stream_with(|out| write_hardlink(out, b"x", b"../../etc/passwd").unwrap());
            assert!(unpack_layer(
                target.path(),
                &mut stream.as_slice(),
                Budget::new(u64::MAX, u64::MAX)
            )
            .is_err());
        }

        #[test]
        fn a_broken_stream_is_an_error_and_not_a_partial_success() {
            let a = tempfile::tempdir().unwrap();
            put(a.path(), "f", "some content");
            let mut stream = Vec::new();
            pack_layer(a.path(), &mut stream).unwrap();
            for cut in [0, 5, 20, stream.len() - 1] {
                let target = tempfile::tempdir().unwrap();
                assert!(
                    unpack_layer(
                        target.path(),
                        &mut &stream[..cut],
                        Budget::new(u64::MAX, u64::MAX)
                    )
                    .is_err(),
                    "cut at {cut}"
                );
            }
            let mut padded = stream.clone();
            padded.push(0);
            let target = tempfile::tempdir().unwrap();
            let error = unpack_layer(
                target.path(),
                &mut padded.as_slice(),
                Budget::new(u64::MAX, u64::MAX),
            )
            .unwrap_err();
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
            assert!(!c2.path().join("sock").exists());
        }

        #[test]
        fn a_symlink_in_the_stream_is_not_followed_for_the_entries_after_it() {
            // The unpack runs with the layer as its root, so a link can only name layer files;
            // this checks the plain part: the later entry is created, not skipped.
            let target = tempfile::tempdir().unwrap();
            fs::create_dir(target.path().join("real")).unwrap();
            let stream = stream_with(|out| {
                write_symlink(out, b"link", &mine(), b"real").unwrap();
            });
            unpack_layer(
                target.path(),
                &mut stream.as_slice(),
                Budget::new(u64::MAX, u64::MAX),
            )
            .unwrap();
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
    fn the_unpack_options_are_the_limits_and_one_layer() {
        let parsed =
            parse_unpack_args(&args(&["--max-bytes", "10", "--max-entries", "3", "/l"])).unwrap();
        assert_eq!(
            parsed,
            UnpackArgs {
                max_bytes: 10,
                max_entries: 3,
                layer: "/l".into()
            }
        );
        assert_eq!(
            parse_unpack_args(&args(&["/l"])).unwrap().max_bytes,
            u64::MAX
        );
        for bad in [
            args(&[]),
            args(&["--max-bytes", "x", "/l"]),
            args(&["--max-bytes"]),
            args(&["/a", "/b"]),
            args(&["--bogus", "/l"]),
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
        assert!(!valid_entry_path(&vec![b'a'; MAX_PATH as usize + 1]));
    }

    #[test]
    fn an_entry_round_trips_through_its_record() {
        let attributes = Attributes {
            mode: 0o4755,
            uid: 1000,
            gid: 1001,
            mtime_sec: -5,
            mtime_nsec: 999,
            xattrs: vec![(b"user.x".to_vec(), b"v".to_vec())],
        };
        let mut stream = Vec::new();
        write_file_start(&mut stream, b"a/b", &attributes, 3).unwrap();
        stream.extend_from_slice(b"abc");
        write_symlink(&mut stream, b"l", &attributes, b"a/b").unwrap();
        write_end(&mut stream).unwrap();
        let mut input = stream.as_slice();
        assert_eq!(
            read_entry(&mut input).unwrap(),
            Entry::File(b"a/b".to_vec(), attributes.clone(), 3)
        );
        let mut content = Vec::new();
        copy_content(&mut input, &mut content, 3).unwrap();
        assert_eq!(content, b"abc");
        assert_eq!(
            read_entry(&mut input).unwrap(),
            Entry::Symlink(b"l".to_vec(), attributes, b"a/b".to_vec())
        );
        assert_eq!(read_entry(&mut input).unwrap(), Entry::End);
    }

    #[test]
    fn an_impossible_record_is_refused() {
        let mut stream = Vec::new();
        write_dir(
            &mut stream,
            b".",
            &Attributes {
                mode: 0o755,
                uid: 0,
                gid: 0,
                mtime_sec: 0,
                mtime_nsec: 2_000_000_000,
                xattrs: Vec::new(),
            },
        )
        .unwrap();
        assert!(read_entry(&mut stream.as_slice()).is_err());
        assert!(read_entry(&mut [b'Z', 0, 0, 0, 1, b'a'].as_slice()).is_err());
        assert!(read_header(&mut b"not a layer line".as_slice()).is_err());
        let mut short = Vec::new();
        write_file_start(
            &mut short,
            b"f",
            &Attributes {
                mode: 0o644,
                uid: 0,
                gid: 0,
                mtime_sec: 0,
                mtime_nsec: 0,
                xattrs: Vec::new(),
            },
            10,
        )
        .unwrap();
        short.extend_from_slice(b"abc");
        let mut input = short.as_slice();
        assert!(matches!(read_entry(&mut input).unwrap(), Entry::File(..)));
        assert!(copy_content(&mut input, &mut Vec::new(), 10).is_err());
    }

    #[test]
    fn the_budget_counts_content_and_each_entry() {
        let mut budget = Budget::new(ENTRY_COST * 2 + 100, 10);
        budget.charge(50).unwrap();
        budget.charge(50).unwrap();
        assert!(budget.charge(1).is_err());
        let mut few = Budget::new(u64::MAX, 1);
        few.charge(0).unwrap();
        assert!(few.charge(0).is_err());
    }
}
