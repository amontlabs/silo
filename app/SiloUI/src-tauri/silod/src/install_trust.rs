//! What a Linux `silod` checks about itself before it serves: the account it runs as, and
//! that nobody but that account and root can change the engine directory it runs from.
//!
//! A Mac engine is a signed bundle; a Linux engine is a plain directory (`<dir>/silod`, its
//! tools beside it, `<dir>/resources/`) with no signature yet (signed archives are roadmap
//! milestone 11). Whoever can write to that directory, or to any directory above it, can
//! swap `silod`, `msb`, `libkrunfw` or a manifest and run as the engine account, with its
//! credentials and computers. So, as OpenSSH's `safe_path` does for `authorized_keys`, every
//! directory from `/` down to the engine directory, `silod` itself, every entry of the engine
//! directory and everything under `resources/` must be owned by this account or root and
//! must not be writable by group or others. A sticky world-writable directory such as `/tmp`
//! is refused: its other users cannot replace the engine's files, but they can make the
//! names the engine looks for before the engine does. Links inside the directory must point
//! inside it, where the walk checks their targets.

use std::{
    fs, io,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
};

use crate::paths::LINUX_RESOURCES_DIR;

/// The deepest `resources/` tree the walk follows, and the most entries it reads.
const MAX_DEPTH: usize = 32;
const MAX_ENTRIES: usize = 100_000;

/// The account that nobody reserves for a service.
const NOBODY: u32 = 65534;

/// The engine refuses root and `nobody`. Root would give every computer's runtime, every
/// credential and the engine socket to the whole machine's administrator account; `nobody`
/// is shared by unrelated services.
pub fn check_engine_account(euid: u32) -> Result<(), String> {
    let name = match euid {
        0 => "root",
        NOBODY => "nobody",
        _ => return Ok(()),
    };
    Err(format!(
        "silod does not run as {name} (uid {euid}): use a dedicated account in the kvm group."
    ))
}

/// Checks the engine directory of `executable` (the running `silod`) for `euid`.
pub fn verify_engine_directory(executable: &Path, euid: u32) -> Result<(), String> {
    verify_engine_directory_below(executable, euid, Path::new("/"))
}

/// [`verify_engine_directory`], checking ancestors only from `trusted` down (tests, whose
/// temporary directories sit under `/tmp`).
fn verify_engine_directory_below(
    executable: &Path,
    euid: u32,
    trusted: &Path,
) -> Result<(), String> {
    let refuse = |path: &Path, reason: &str| {
        format!(
            "silod will not start: {} {reason}. Install the engine in a directory that only \
             this account or root can change, for example ~/silo-engine or a root-owned \
             /opt/silo-engine (chmod -R go-w removes group and other write access).",
            path.display()
        )
    };
    let executable = fs::canonicalize(executable)
        .map_err(|error| refuse(executable, &format!("cannot be resolved ({error})")))?;
    let directory = executable
        .parent()
        .ok_or_else(|| refuse(&executable, "has no parent directory"))?
        .to_path_buf();
    if !directory.starts_with(trusted) {
        return Err(refuse(&directory, "is outside the checked tree"));
    }
    let mut walk = Walk {
        euid,
        root: directory.clone(),
        entries: 0,
    };
    let checked = (|| {
        // Every ancestor, from the trusted top down to the engine directory itself.
        let mut ancestors = directory
            .ancestors()
            .take_while(|ancestor| ancestor.starts_with(trusted))
            .collect::<Vec<_>>();
        ancestors.reverse();
        for ancestor in ancestors {
            walk.owned(ancestor, &fs::symlink_metadata(ancestor)?, true)?;
        }
        let metadata = fs::symlink_metadata(&executable)?;
        if !metadata.is_file() {
            return Err(Refusal::at(&executable, "is not a regular file"));
        }
        walk.owned(&executable, &metadata, false)?;
        // The directory's own entries (the bundled tools), then the resources below it.
        walk.directory(&directory, 1)?;
        let resources = directory.join(LINUX_RESOURCES_DIR);
        match fs::symlink_metadata(&resources) {
            Ok(metadata) if metadata.is_dir() => walk.directory(&resources, MAX_DEPTH),
            Ok(_) => Err(Refusal::at(&resources, "is not a directory")),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    })();
    checked.map_err(|refusal| match refusal {
        Refusal::Path(path, reason) => refuse(&path, &reason),
        Refusal::Io(error) => refuse(&directory, &format!("could not be checked ({error})")),
    })
}

enum Refusal {
    Path(PathBuf, String),
    Io(io::Error),
}

impl Refusal {
    fn at(path: &Path, reason: &str) -> Self {
        Self::Path(path.to_path_buf(), reason.to_owned())
    }
}

impl From<io::Error> for Refusal {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

struct Walk {
    euid: u32,
    /// The canonical engine directory; links must resolve inside it.
    root: PathBuf,
    entries: usize,
}

impl Walk {
    /// `path` (described by `metadata`, never followed) is a directory (when `directory`) or
    /// a file of this account or root, and nobody else may write to it.
    fn owned(&self, path: &Path, metadata: &fs::Metadata, directory: bool) -> Result<(), Refusal> {
        if metadata.file_type().is_symlink() {
            return Err(Refusal::at(path, "is a link"));
        }
        if directory && !metadata.is_dir() {
            return Err(Refusal::at(path, "is not a directory"));
        }
        if metadata.uid() != 0 && metadata.uid() != self.euid {
            return Err(Refusal::Path(
                path.to_path_buf(),
                format!(
                    "belongs to uid {}, not this account (uid {}) or root",
                    metadata.uid(),
                    self.euid
                ),
            ));
        }
        let mode = metadata.permissions().mode();
        if mode & 0o022 != 0 {
            return Err(Refusal::Path(
                path.to_path_buf(),
                format!(
                    "can be changed by other accounts (mode {:o})",
                    mode & 0o7777
                ),
            ));
        }
        Ok(())
    }

    /// Checks every entry of `directory`, descending `depth - 1` more levels.
    fn directory(&mut self, directory: &Path, depth: usize) -> Result<(), Refusal> {
        self.owned(directory, &fs::symlink_metadata(directory)?, true)?;
        for entry in fs::read_dir(directory)? {
            self.entries += 1;
            if self.entries > MAX_ENTRIES {
                return Err(Refusal::at(directory, "holds too many files to check"));
            }
            let path = entry?.path();
            let metadata = fs::symlink_metadata(&path)?;
            let kind = metadata.file_type();
            if kind.is_symlink() {
                self.link(&path)?;
            } else if kind.is_dir() {
                if depth > 1 {
                    self.directory(&path, depth - 1)?;
                } else {
                    self.owned(&path, &metadata, true)?;
                }
            } else if kind.is_file() {
                self.owned(&path, &metadata, false)?;
            } else {
                return Err(Refusal::at(&path, "is not a regular file or directory"));
            }
        }
        Ok(())
    }

    /// A link may only point to an existing file or directory inside the engine directory,
    /// which the walk checks where it is.
    fn link(&self, path: &Path) -> Result<(), Refusal> {
        match fs::canonicalize(path) {
            Ok(target) if target.starts_with(&self.root) => Ok(()),
            Ok(target) => Err(Refusal::Path(
                path.to_path_buf(),
                format!(
                    "links outside the engine directory, to {}",
                    target.display()
                ),
            )),
            Err(_) => Err(Refusal::at(path, "is a link to nothing")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn euid() -> u32 {
        unsafe { libc::geteuid() }
    }

    fn set_mode(path: &Path, mode: u32) {
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    }

    /// `<top>/engine/silod` and `<top>/engine/resources/...`, every mode 0755 or 0644.
    fn engine() -> (tempfile::TempDir, PathBuf) {
        let top = tempfile::tempdir().unwrap();
        set_mode(top.path(), 0o755);
        let directory = top.path().join("engine");
        fs::create_dir_all(directory.join("resources/microsandbox/lib")).unwrap();
        for path in [
            directory.clone(),
            directory.join("resources"),
            directory.join("resources/microsandbox"),
            directory.join("resources/microsandbox/lib"),
        ] {
            set_mode(&path, 0o755);
        }
        for (name, mode) in [
            ("silod", 0o755),
            ("msb", 0o755),
            ("resources/engine-manifest.json", 0o644),
            ("resources/microsandbox/lib/libkrunfw.so.5.6.1", 0o644),
        ] {
            fs::write(directory.join(name), name).unwrap();
            set_mode(&directory.join(name), mode);
        }
        (top, directory)
    }

    fn check(top: &Path, directory: &Path) -> Result<(), String> {
        verify_engine_directory_below(&directory.join("silod"), euid(), top)
    }

    #[test]
    fn root_and_nobody_are_refused_and_other_accounts_run() {
        for euid in [0, 65534] {
            let error = check_engine_account(euid).unwrap_err();
            assert!(
                error.contains("dedicated account in the kvm group"),
                "{error}"
            );
            assert!(error.contains(&format!("uid {euid}")), "{error}");
        }
        for euid in [1, 1000, 65533, 65535] {
            check_engine_account(euid).unwrap();
        }
    }

    #[test]
    fn an_engine_directory_only_this_account_can_change_is_accepted() {
        let (top, directory) = engine();
        check(top.path(), &directory).unwrap();
        // Links that stay inside the directory are fine: the walk checks their targets.
        symlink(
            "libkrunfw.so.5.6.1",
            directory.join("resources/microsandbox/lib/libkrunfw.so.5"),
        )
        .unwrap();
        check(top.path(), &directory).unwrap();
    }

    #[test]
    fn a_writable_file_or_directory_anywhere_in_the_engine_is_refused_by_name() {
        for (relative, mode) in [
            ("", 0o775),
            ("silod", 0o775),
            ("msb", 0o757),
            ("resources", 0o777),
            ("resources/engine-manifest.json", 0o664),
            ("resources/microsandbox/lib", 0o1777),
            ("resources/microsandbox/lib/libkrunfw.so.5.6.1", 0o646),
        ] {
            let (top, directory) = engine();
            let path = if relative.is_empty() {
                directory.clone()
            } else {
                directory.join(relative)
            };
            set_mode(&path, mode);
            let error = check(top.path(), &directory).unwrap_err();
            assert!(
                error.contains(&format!(
                    "{} can be changed by other accounts",
                    path.display()
                )),
                "{relative}: {error}"
            );
        }
    }

    #[test]
    fn a_writable_ancestor_is_refused_even_when_sticky() {
        let (top, directory) = engine();
        for mode in [0o777, 0o1777, 0o775] {
            set_mode(top.path(), mode);
            let error = check(top.path(), &directory).unwrap_err();
            assert!(
                error.contains(&format!("{} can be changed", top.path().display())),
                "{error}"
            );
        }
    }

    /// The whole path is checked from `/`: an engine unpacked under a world-writable `/tmp`
    /// is refused even though everything inside it is in order.
    #[test]
    fn an_engine_under_the_real_tmp_is_refused() {
        match fs::metadata("/tmp") {
            Ok(metadata) if metadata.permissions().mode() & 0o022 != 0 => {}
            _ => return,
        }
        let top = tempfile::tempdir_in("/tmp").unwrap();
        set_mode(top.path(), 0o755);
        let executable = top.path().join("silod");
        fs::write(&executable, "silod").unwrap();
        set_mode(&executable, 0o755);
        let error = verify_engine_directory(&executable, euid()).unwrap_err();
        assert!(
            error.contains("silod will not start: /tmp can be changed"),
            "{error}"
        );
        assert!(error.contains("~/silo-engine"), "{error}");
    }

    #[test]
    fn links_out_of_the_engine_and_dangling_links_are_refused() {
        let (top, directory) = engine();
        let outside = top.path().join("outside");
        fs::write(&outside, "planted").unwrap();
        set_mode(&outside, 0o644);
        let link = directory.join("resources/microsandbox/lib/libkrunfw.so.5");
        symlink(&outside, &link).unwrap();
        let error = check(top.path(), &directory).unwrap_err();
        assert!(
            error.contains("links outside the engine directory"),
            "{error}"
        );
        fs::remove_file(&link).unwrap();
        symlink("missing", &link).unwrap();
        let error = check(top.path(), &directory).unwrap_err();
        assert!(error.contains("is a link to nothing"), "{error}");
    }

    #[test]
    fn a_resources_link_and_special_files_are_refused() {
        let (top, directory) = engine();
        let resources = directory.join("resources");
        let moved = top.path().join("moved");
        fs::rename(&resources, &moved).unwrap();
        symlink(&moved, &resources).unwrap();
        assert!(check(top.path(), &directory).is_err());

        let (top, directory) = engine();
        let fifo = std::ffi::CString::new(
            directory
                .join("resources/fifo")
                .as_os_str()
                .as_encoded_bytes(),
        )
        .unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        let error = check(top.path(), &directory).unwrap_err();
        assert!(
            error.contains("is not a regular file or directory"),
            "{error}"
        );
    }

    /// Another account's file is refused. Changing an owner needs root, so the check runs
    /// only where the test can make one.
    #[test]
    fn a_file_of_another_account_is_refused() {
        let (top, directory) = engine();
        let manifest = directory.join("resources/engine-manifest.json");
        let path = std::ffi::CString::new(manifest.as_os_str().as_encoded_bytes()).unwrap();
        if unsafe { libc::lchown(path.as_ptr(), 4242, 4242) } != 0 {
            return;
        }
        let error = check(top.path(), &directory).unwrap_err();
        assert!(error.contains("belongs to uid 4242"), "{error}");
    }
}
