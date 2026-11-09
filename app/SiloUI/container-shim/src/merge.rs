//! `silo-ctr merge --out DIR LAYER...`: folds overlay layers (oldest first) into one directory so
//! that mounting the result in place of those layers shows the same files. Run inside the
//! container's user namespace (`silo-ctr userns --keep-id N -- silo-ctr merge ...`), where the
//! layers' owners are real and the whiteouts can be made.
//!
//! A later layer's entry replaces an earlier one of another kind; a whiteout (character device
//! 0:0) removes what the earlier layers hold at its path and stays as a whiteout, because the
//! layers below the merged ones may still hold the file; an opaque directory (the
//! `user.overlay.opaque` attribute) drops what the earlier layers held inside it and stays opaque.
//! Owners, modes, times and extended attributes are kept.

#[cfg(target_os = "linux")]
pub use linux::command;

/// The options of a `merge` command line.
pub fn parse_args(args: &[String]) -> Result<(String, Vec<String>), String> {
    let mut out = None;
    let mut layers = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--out" => out = Some(iter.next().ok_or("--out needs a folder")?.clone()),
            other if other.starts_with("--") => return Err(format!("unknown option {other}")),
            other => layers.push(other.to_owned()),
        }
    }
    let out = out.ok_or("merge needs --out")?;
    if layers.is_empty() {
        return Err("merge needs at least one layer".into());
    }
    Ok((out, layers))
}

#[cfg(target_os = "linux")]
mod linux {
    use super::parse_args;
    use crate::common::io_err;
    use std::ffi::{CString, OsStr};
    use std::fs;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
    use std::path::Path;

    const OPAQUE: &str = "user.overlay.opaque";

    fn c(path: &Path) -> Result<CString, String> {
        CString::new(path.as_os_str().as_bytes()).map_err(|_| "a path contains a NUL byte".into())
    }

    fn os(what: &str, path: &Path) -> String {
        io_err(
            &format!("{what} {}", path.display()),
            &std::io::Error::last_os_error(),
        )
    }

    fn is_whiteout(meta: &fs::Metadata) -> bool {
        meta.file_type().is_char_device() && meta.rdev() == 0
    }

    fn xattr_names(path: &Path) -> Result<Vec<Vec<u8>>, String> {
        let p = c(path)?;
        // SAFETY: a null buffer of size 0 asks for the length only.
        let len = unsafe { libc::llistxattr(p.as_ptr(), std::ptr::null_mut(), 0) };
        if len < 0 {
            // A file system without extended attributes has none to copy.
            return match std::io::Error::last_os_error().raw_os_error() {
                Some(libc::ENOTSUP) => Ok(Vec::new()),
                _ => Err(os("list the attributes of", path)),
            };
        }
        let mut buf = vec![0u8; len as usize];
        // SAFETY: `buf` is valid for `buf.len()` bytes.
        let len = unsafe { libc::llistxattr(p.as_ptr(), buf.as_mut_ptr().cast(), buf.len()) };
        if len < 0 {
            return Err(os("list the attributes of", path));
        }
        buf.truncate(len as usize);
        Ok(buf
            .split(|b| *b == 0)
            .filter(|name| !name.is_empty())
            .map(<[u8]>::to_vec)
            .collect())
    }

    fn get_xattr(path: &Path, name: &[u8]) -> Result<Vec<u8>, String> {
        let p = c(path)?;
        let n = CString::new(name).map_err(|_| "an attribute name contains NUL")?;
        let mut buf = vec![0u8; 65536];
        // SAFETY: `buf` is valid for `buf.len()` bytes.
        let len =
            unsafe { libc::lgetxattr(p.as_ptr(), n.as_ptr(), buf.as_mut_ptr().cast(), buf.len()) };
        if len < 0 {
            return Err(os("read an attribute of", path));
        }
        buf.truncate(len as usize);
        Ok(buf)
    }

    fn copy_xattrs(src: &Path, dst: &Path) -> Result<(), String> {
        let d = c(dst)?;
        for name in xattr_names(src)? {
            let value = get_xattr(src, &name)?;
            let n = CString::new(name).map_err(|_| "an attribute name contains NUL")?;
            // SAFETY: both buffers are valid for their lengths.
            let rc = unsafe {
                libc::lsetxattr(
                    d.as_ptr(),
                    n.as_ptr(),
                    value.as_ptr().cast(),
                    value.len(),
                    0,
                )
            };
            if rc != 0 {
                return Err(os("set an attribute of", dst));
            }
        }
        Ok(())
    }

    fn is_opaque(dir: &Path) -> bool {
        get_xattr(dir, OPAQUE.as_bytes()).is_ok_and(|value| value == b"y")
    }

    /// Owner, mode, times and attributes of `src` onto `dst` (a link keeps its own).
    fn copy_attributes(src: &Path, dst: &Path, meta: &fs::Metadata) -> Result<(), String> {
        let d = c(dst)?;
        // SAFETY: `d` is a valid path string.
        if unsafe { libc::lchown(d.as_ptr(), meta.uid(), meta.gid()) } != 0 {
            return Err(os("change the owner of", dst));
        }
        copy_xattrs(src, dst)?;
        if !meta.file_type().is_symlink() {
            fs::set_permissions(dst, fs::Permissions::from_mode(meta.mode() & 0o7777))
                .map_err(|e| io_err(&format!("set the mode of {}", dst.display()), &e))?;
        }
        let times = [
            libc::timespec {
                tv_sec: meta.atime(),
                tv_nsec: meta.atime_nsec(),
            },
            libc::timespec {
                tv_sec: meta.mtime(),
                tv_nsec: meta.mtime_nsec(),
            },
        ];
        // SAFETY: `times` holds two timespecs; the flag keeps a link from being followed.
        let rc = unsafe {
            libc::utimensat(
                libc::AT_FDCWD,
                d.as_ptr(),
                times.as_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if rc != 0 {
            return Err(os("set the times of", dst));
        }
        Ok(())
    }

    fn remove_all(path: &Path) -> Result<(), String> {
        match fs::symlink_metadata(path) {
            Ok(meta) if meta.is_dir() => fs::remove_dir_all(path),
            Ok(_) => fs::remove_file(path),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
        .map_err(|e| io_err(&format!("remove {}", path.display()), &e))
    }

    fn mknod(path: &Path, mode: u32, rdev: u64) -> Result<(), String> {
        let p = c(path)?;
        // SAFETY: `p` is a valid path string.
        if unsafe { libc::mknod(p.as_ptr(), mode as libc::mode_t, rdev as libc::dev_t) } != 0 {
            return Err(os("create", path));
        }
        Ok(())
    }

    fn copy_node(src: &Path, dst: &Path, meta: &fs::Metadata) -> Result<(), String> {
        let kind = meta.file_type();
        if kind.is_symlink() {
            let target =
                fs::read_link(src).map_err(|e| io_err(&format!("read {}", src.display()), &e))?;
            std::os::unix::fs::symlink(target, dst)
                .map_err(|e| io_err(&format!("create {}", dst.display()), &e))?;
        } else if kind.is_file() {
            fs::copy(src, dst).map_err(|e| io_err(&format!("copy {}", src.display()), &e))?;
        } else {
            mknod(dst, meta.mode(), meta.rdev())?;
        }
        copy_attributes(src, dst, meta)
    }

    fn merge_dir(src: &Path, dst: &Path) -> Result<(), String> {
        if is_opaque(src) {
            for entry in
                fs::read_dir(dst).map_err(|e| io_err(&format!("read {}", dst.display()), &e))?
            {
                remove_all(&entry.map_err(|e| io_err("read a folder", &e))?.path())?;
            }
        }
        for entry in
            fs::read_dir(src).map_err(|e| io_err(&format!("read {}", src.display()), &e))?
        {
            let entry = entry.map_err(|e| io_err("read a folder", &e))?;
            let (from, to) = (entry.path(), dst.join(entry.file_name()));
            let meta = fs::symlink_metadata(&from)
                .map_err(|e| io_err(&format!("look at {}", from.display()), &e))?;
            if meta.is_dir() {
                if fs::symlink_metadata(&to).is_ok_and(|existing| !existing.is_dir()) {
                    remove_all(&to)?;
                }
                if !to.exists() {
                    fs::create_dir(&to)
                        .map_err(|e| io_err(&format!("create {}", to.display()), &e))?;
                }
                merge_dir(&from, &to)?;
            } else {
                remove_all(&to)?;
                if is_whiteout(&meta) {
                    mknod(&to, meta.mode(), 0)?;
                    copy_attributes(&from, &to, &meta)?;
                } else {
                    copy_node(&from, &to, &meta)?;
                }
            }
        }
        let meta = fs::symlink_metadata(src)
            .map_err(|e| io_err(&format!("look at {}", src.display()), &e))?;
        copy_attributes(src, dst, &meta)
    }

    pub fn command(arguments: &[String]) -> Result<i32, String> {
        let (out, layers) = parse_args(arguments)?;
        let out = Path::new(&out);
        fs::create_dir_all(out).map_err(|e| io_err(&format!("create {}", out.display()), &e))?;
        for layer in &layers {
            merge_dir(Path::new(OsStr::new(layer)), out)?;
        }
        Ok(0)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn put(root: &Path, path: &str, text: &str) {
            let full = root.join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, text).unwrap();
        }

        fn run(out: &Path, layers: &[&Path]) {
            let mut args = vec!["--out".to_owned(), out.display().to_string()];
            args.extend(layers.iter().map(|l| l.display().to_string()));
            command(&args).unwrap();
        }

        #[test]
        fn a_later_layer_wins_and_other_files_stay() {
            let (a, b, out) = (
                tempfile::tempdir().unwrap(),
                tempfile::tempdir().unwrap(),
                tempfile::tempdir().unwrap(),
            );
            put(a.path(), "etc/one", "old");
            put(a.path(), "etc/two", "kept");
            put(b.path(), "etc/one", "new");
            put(b.path(), "usr/bin/x", "x");
            run(out.path(), &[a.path(), b.path()]);
            assert_eq!(
                fs::read_to_string(out.path().join("etc/one")).unwrap(),
                "new"
            );
            assert_eq!(
                fs::read_to_string(out.path().join("etc/two")).unwrap(),
                "kept"
            );
            assert_eq!(
                fs::read_to_string(out.path().join("usr/bin/x")).unwrap(),
                "x"
            );
        }

        #[test]
        fn a_file_and_a_directory_replace_each_other() {
            let (a, b, out) = (
                tempfile::tempdir().unwrap(),
                tempfile::tempdir().unwrap(),
                tempfile::tempdir().unwrap(),
            );
            put(a.path(), "d/inner", "i");
            put(a.path(), "f", "file");
            put(b.path(), "d", "now a file");
            put(b.path(), "f/inner", "now a folder");
            run(out.path(), &[a.path(), b.path()]);
            assert_eq!(
                fs::read_to_string(out.path().join("d")).unwrap(),
                "now a file"
            );
            assert_eq!(
                fs::read_to_string(out.path().join("f/inner")).unwrap(),
                "now a folder"
            );
        }

        #[test]
        fn a_symlink_keeps_its_target_and_the_mode_is_kept() {
            let (a, out) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
            put(a.path(), "bin/tool", "x");
            fs::set_permissions(a.path().join("bin/tool"), fs::Permissions::from_mode(0o751))
                .unwrap();
            std::os::unix::fs::symlink("tool", a.path().join("bin/alias")).unwrap();
            run(out.path(), &[a.path()]);
            assert_eq!(
                fs::read_link(out.path().join("bin/alias")).unwrap(),
                Path::new("tool")
            );
            assert_eq!(
                fs::metadata(out.path().join("bin/tool")).unwrap().mode() & 0o777,
                0o751
            );
        }

        #[test]
        fn a_whiteout_removes_the_earlier_file_and_stays_a_whiteout() {
            let (a, b, out) = (
                tempfile::tempdir().unwrap(),
                tempfile::tempdir().unwrap(),
                tempfile::tempdir().unwrap(),
            );
            put(a.path(), "gone", "x");
            // Only root (or a user namespace) may make the device; elsewhere there is nothing to test.
            if mknod(&b.path().join("gone"), libc::S_IFCHR as u32, 0).is_err() {
                return;
            }
            run(out.path(), &[a.path(), b.path()]);
            let meta = fs::symlink_metadata(out.path().join("gone")).unwrap();
            assert!(is_whiteout(&meta));
        }

        #[test]
        fn an_opaque_directory_drops_what_was_inside() {
            let (a, b, out) = (
                tempfile::tempdir().unwrap(),
                tempfile::tempdir().unwrap(),
                tempfile::tempdir().unwrap(),
            );
            put(a.path(), "d/old", "old");
            put(b.path(), "d/new", "new");
            let d = c(&b.path().join("d")).unwrap();
            let name = CString::new(OPAQUE).unwrap();
            // SAFETY: valid strings and a one-byte value.
            if unsafe { libc::lsetxattr(d.as_ptr(), name.as_ptr(), b"y".as_ptr().cast(), 1, 0) }
                != 0
            {
                return;
            }
            run(out.path(), &[a.path(), b.path()]);
            assert!(!out.path().join("d/old").exists());
            assert!(out.path().join("d/new").exists());
            assert!(is_opaque(&out.path().join("d")));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_args;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_options_are_the_output_and_the_layers_in_order() {
        let (out, layers) = parse_args(&args(&["--out", "/o", "/a", "/b"])).unwrap();
        assert_eq!((out.as_str(), layers), ("/o", args(&["/a", "/b"])));
    }

    #[test]
    fn missing_pieces_are_errors() {
        assert!(parse_args(&args(&["/a"])).is_err());
        assert!(parse_args(&args(&["--out", "/o"])).is_err());
        assert!(parse_args(&args(&["--out"])).is_err());
        assert!(parse_args(&args(&["--bogus", "--out", "/o", "/a"])).is_err());
    }
}
