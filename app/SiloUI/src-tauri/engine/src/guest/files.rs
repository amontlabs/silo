//! Programmatic guest files (`guest.files.read/write/list/remove`) as the working account.
//!
//! The bytes cross the command channel as stdin and stdout. A small native helper in the
//! guest (`helpers/files.pl`) opens, stages, commits and lists, so metadata always belongs to
//! the bytes it describes and commits are atomic in the guest filesystem.
use super::{
    exec::{perl_argv, FILES},
    process::{self, Stopped},
    target::Target,
    GuestError, MAX_RESPONSE_RAW_BYTES,
};
use silo_api::jobs::ErrorCode;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};

const FILE_TIMEOUT: Duration = Duration::from_secs(30);
const LIST_BUDGET_BYTES: usize = 8 * 1024 * 1024;

/// A file read: the requested bytes and the metadata of the file they came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileRead {
    pub data: Vec<u8>,
    pub size: u64,
    pub mtime_ms: u64,
    pub inode: u64,
    pub mode: u32,
}

/// What a write may replace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Replace {
    Always,
    /// Fails with `exists` when anything is at the path.
    Never,
    /// Succeeds only if the file's current SHA-256 is the digest. `None` means the file must
    /// not exist (`changed` otherwise).
    IfSha256(Option<String>),
}

#[derive(Clone, Debug)]
pub struct WriteRequest {
    pub path: String,
    pub data: Vec<u8>,
    pub mode: u32,
    pub create_parents: bool,
    pub replace: Replace,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileWrite {
    pub size: u64,
    pub mtime_ms: u64,
    pub inode: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    File,
    Dir,
    Link,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileEntry {
    pub name: String,
    pub kind: FileKind,
    pub size: u64,
    pub mtime_ms: u64,
}

fn check_path(path: &str) -> Result<(), GuestError> {
    if !path.starts_with('/') || path.contains('\0') || path.len() > 4096 {
        return Err(GuestError::invalid(
            "Invalid params: path must be an absolute path without NUL.",
        ));
    }
    Ok(())
}

/// Reads `length` bytes from `offset` (the rest of the file when `length` is omitted).
/// A response carries at most `MAX_RESPONSE_RAW_BYTES`; a larger range is refused so callers
/// page explicitly.
pub fn read(
    target: &Target,
    path: &str,
    offset: Option<u64>,
    length: Option<u64>,
) -> Result<FileRead, GuestError> {
    check_path(path)?;
    let offset = offset.unwrap_or(0);
    if let Some(length) = length {
        if offset.checked_add(length).is_none() || length > MAX_RESPONSE_RAW_BYTES as u64 {
            return Err(GuestError::invalid(format!(
                "Invalid params: a read returns at most {MAX_RESPONSE_RAW_BYTES} bytes."
            )));
        }
    }
    let output = run(
        target,
        [
            "read".to_owned(),
            path.to_owned(),
            offset.to_string(),
            length.map(|length| length.to_string()).unwrap_or_default(),
            MAX_RESPONSE_RAW_BYTES.to_string(),
        ],
        None,
        MAX_RESPONSE_RAW_BYTES + 256,
    )?;
    let newline = output
        .iter()
        .position(|byte| *byte == b'\n')
        .ok_or_else(|| GuestError::internal("The computer returned an invalid file."))?;
    let header = String::from_utf8_lossy(&output[..newline]).into_owned();
    let fields: Vec<u64> = header
        .split(' ')
        .map(|field| field.parse::<u64>())
        .collect::<Result<_, _>>()
        .map_err(|_| GuestError::internal("The computer returned an invalid file header."))?;
    let [size, mtime_ms, inode, mode, length] = fields[..] else {
        return Err(GuestError::internal(
            "The computer returned an invalid file header.",
        ));
    };
    let data = output[newline + 1..].to_vec();
    if data.len() as u64 != length {
        return Err(GuestError::internal(
            "The computer returned a truncated file.",
        ));
    }
    Ok(FileRead {
        data,
        size,
        mtime_ms,
        inode,
        mode: mode as u32,
    })
}

type Lock = Arc<Mutex<()>>;

/// Serializes conditional writes to one path across every client of this engine, so two
/// create-only or compare-and-replace writes cannot interleave their check and commit.
fn path_lock(computer: &str, path: &str) -> Lock {
    static LOCKS: OnceLock<Mutex<HashMap<String, Lock>>> = OnceLock::new();
    let mut locks = LOCKS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    locks.retain(|_, lock| Arc::strong_count(lock) > 1);
    locks
        .entry(format!("{computer}\0{path}"))
        .or_default()
        .clone()
}

/// Writes a file atomically: the bytes are staged next to the destination at mode 0600,
/// then given `mode` and committed, so nobody ever reads a partial file or a permissive
/// temporary one.
///
/// `Replace::Never` and `Replace::IfSha256(None)` commit with a hard link, which fails if
/// anything exists at the path; they are race-free against every writer. `IfSha256(Some)`
/// compares inside the guest immediately before the rename and re-checks the file's identity
/// (device, inode, size, mtime), which engine writers serialize completely; a writer outside
/// the engine could still slip in between that last check and the rename.
pub fn write(target: &Target, request: &WriteRequest) -> Result<FileWrite, GuestError> {
    check_path(&request.path)?;
    if request.path.ends_with('/') {
        return Err(GuestError::invalid(
            "Invalid params: path must name a file.",
        ));
    }
    if request.mode > 0o7777 {
        return Err(GuestError::invalid(
            "Invalid params: mode must be permission bits (at most 07777).",
        ));
    }
    let (replace, expected) = match &request.replace {
        Replace::Always => ("always", String::new()),
        Replace::Never => ("never", String::new()),
        Replace::IfSha256(None) => ("ifSha256", String::new()),
        Replace::IfSha256(Some(digest)) => {
            if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(GuestError::invalid(
                    "Invalid params: expectedSha256 must be 64 hexadecimal digits.",
                ));
            }
            ("ifSha256", digest.to_ascii_lowercase())
        }
    };
    let lock = path_lock(target.id(), &request.path);
    let _guard = lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let output = run(
        target,
        [
            "write".to_owned(),
            request.path.clone(),
            request.mode.to_string(),
            if request.create_parents { "1" } else { "0" }.to_owned(),
            replace.to_owned(),
            expected,
        ],
        Some(&request.data),
        4096,
    )?;
    let text = String::from_utf8_lossy(&output);
    let fields: Vec<u64> = text
        .split_whitespace()
        .map(|field| field.parse::<u64>())
        .collect::<Result<_, _>>()
        .map_err(|_| GuestError::internal("The computer returned an invalid write result."))?;
    let [size, mtime_ms, inode] = fields[..] else {
        return Err(GuestError::internal(
            "The computer returned an invalid write result.",
        ));
    };
    Ok(FileWrite {
        size,
        mtime_ms,
        inode,
    })
}

/// Lists one folder: links are reported, not followed, and every entry carries its size and
/// modification time. The listing is not paged.
pub fn list(target: &Target, path: &str) -> Result<Vec<FileEntry>, GuestError> {
    check_path(path)?;
    let output = run(
        target,
        ["list".to_owned(), path.to_owned()],
        None,
        LIST_BUDGET_BYTES,
    )?;
    let mut fields = output.split(|byte| *byte == 0);
    let mut entries = Vec::new();
    loop {
        let Some(kind) = fields.next().filter(|field| !field.is_empty()) else {
            break;
        };
        let (Some(size), Some(mtime), Some(name)) = (fields.next(), fields.next(), fields.next())
        else {
            return Err(GuestError::internal(
                "The computer returned an invalid folder listing.",
            ));
        };
        let number = |bytes: &[u8]| {
            std::str::from_utf8(bytes)
                .ok()
                .and_then(|text| text.parse::<u64>().ok())
                .ok_or_else(|| {
                    GuestError::internal("The computer returned an invalid folder listing.")
                })
        };
        entries.push(FileEntry {
            name: String::from_utf8_lossy(name).into_owned(),
            kind: match kind {
                b"file" => FileKind::File,
                b"dir" => FileKind::Dir,
                b"link" => FileKind::Link,
                _ => FileKind::Other,
            },
            size: number(size)?,
            mtime_ms: number(mtime)?,
        });
    }
    Ok(entries)
}

/// Removes files in order: a missing path counts as removed, a link is unlinked without
/// touching its target, and a directory is refused (`not_a_file`). A failure stops the run;
/// earlier removals stay, and retrying is safe because absent paths succeed.
pub fn remove(target: &Target, paths: &[String]) -> Result<Vec<String>, GuestError> {
    for path in paths {
        check_path(path)?;
    }
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let mut arguments = vec!["remove".to_owned()];
    arguments.extend(paths.iter().cloned());
    run(target, arguments, None, 4096)?;
    Ok(paths.to_vec())
}

/// Runs the file helper and maps its failure codes onto stable error codes.
fn run(
    target: &Target,
    arguments: impl Into<Vec<String>>,
    stdin: Option<&[u8]>,
    max_stdout: usize,
) -> Result<Vec<u8>, GuestError> {
    let argv = perl_argv(FILES, &arguments.into());
    run_argv(target, &argv, stdin, max_stdout)
}

pub(crate) fn run_argv(
    target: &Target,
    argv: &[String],
    stdin: Option<&[u8]>,
    max_stdout: usize,
) -> Result<Vec<u8>, GuestError> {
    target.ensure_running()?;
    let plan = target.plan(argv, &[], false, stdin.is_some())?;
    let outcome = process::run_collect(
        &plan,
        stdin,
        FILE_TIMEOUT,
        max_stdout,
        16 * 1024,
        None,
        &|| {},
    )?;
    let collected = match outcome {
        Ok(collected) => collected,
        Err(Stopped::Timeout) => {
            return Err(GuestError::new(
                ErrorCode::TimedOut,
                "The computer took too long to answer.",
            ))
        }
        Err(Stopped::Overflow) => {
            return Err(GuestError::internal(
                "The result is larger than one response can carry.",
            ))
        }
        Err(Stopped::Cancelled) => return Err(GuestError::new(ErrorCode::Cancelled, "Cancelled.")),
    };
    let code = collected.status.code().ok_or_else(|| {
        GuestError::internal("The connection to the computer ended unexpectedly.")
    })?;
    if code == 0 {
        return Ok(collected.stdout);
    }
    let stderr = String::from_utf8_lossy(&collected.stderr).into_owned();
    if let Some(error) = target.channel_error(code, &stderr) {
        return Err(error);
    }
    let message = stderr
        .lines()
        .find_map(|line| line.strip_prefix("silo-files: "))
        .unwrap_or(stderr.trim())
        .to_owned();
    let code = match code {
        64 => ErrorCode::NotFound,
        65 => ErrorCode::PermissionDenied,
        66 => ErrorCode::NotAFile,
        67 => ErrorCode::Exists,
        68 => ErrorCode::Changed,
        126 | 127 => {
            return Err(GuestError::internal(
                "The computer cannot run its file helper: Perl was not found.",
            ))
        }
        _ => ErrorCode::Internal,
    };
    Err(GuestError::new(code, message))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guest::test_support;
    use sha2::{Digest, Sha256};
    use std::os::unix::fs::PermissionsExt;

    fn sha(bytes: &[u8]) -> String {
        crate::guest::hex(&Sha256::digest(bytes))
    }

    fn put(
        target: &Target,
        path: &std::path::Path,
        data: &[u8],
        replace: Replace,
    ) -> Result<FileWrite, GuestError> {
        write(
            target,
            &WriteRequest {
                path: path.to_string_lossy().into_owned(),
                data: data.to_vec(),
                mode: 0o640,
                create_parents: false,
                replace,
            },
        )
    }

    #[test]
    fn writes_are_atomic_private_and_report_the_metadata_of_what_was_written() {
        let directory = tempfile::tempdir().unwrap();
        let target = test_support::target(directory.path());
        let path = directory.path().join("nested/deeper/file.bin");
        let data: Vec<u8> = (0..=255u8).collect();
        let result = write(
            &target,
            &WriteRequest {
                path: path.to_string_lossy().into_owned(),
                data: data.clone(),
                mode: 0o600,
                create_parents: true,
                replace: Replace::Always,
            },
        )
        .unwrap();
        assert_eq!(result.size, 256);
        assert_eq!(std::fs::read(&path).unwrap(), data);
        let metadata = std::fs::metadata(&path).unwrap();
        assert_eq!(metadata.permissions().mode() & 0o7777, 0o600);
        assert_eq!(
            std::fs::metadata(path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        use std::os::unix::fs::MetadataExt;
        assert_eq!(result.inode, metadata.ino());
        // No staged copy is left behind in the destination folder.
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1
        );

        let missing_parent = directory.path().join("absent/file");
        let error = put(&target, &missing_parent, b"x", Replace::Always).unwrap_err();
        assert_eq!(error.code, ErrorCode::NotFound);
    }

    #[test]
    fn create_only_has_one_winner_and_a_null_hash_means_must_not_exist() {
        let directory = tempfile::tempdir().unwrap();
        let target = test_support::target(directory.path());
        let path = directory.path().join("answer.json");
        put(&target, &path, b"first", Replace::Never).unwrap();
        assert_eq!(
            put(&target, &path, b"second", Replace::Never)
                .unwrap_err()
                .code,
            ErrorCode::Exists
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"first");
        assert_eq!(
            put(&target, &path, b"third", Replace::IfSha256(None))
                .unwrap_err()
                .code,
            ErrorCode::Changed
        );
        let fresh = directory.path().join("fresh");
        put(&target, &fresh, b"ok", Replace::IfSha256(None)).unwrap();
        assert_eq!(std::fs::read(&fresh).unwrap(), b"ok");

        // Two simultaneous create-only writers: exactly one wins.
        let race = directory.path().join("race");
        let results: Vec<_> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..4)
                .map(|index| {
                    let (target, race) = (&target, &race);
                    scope.spawn(move || {
                        put(target, race, format!("{index}").as_bytes(), Replace::Never)
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect()
        });
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    }

    #[test]
    fn compare_and_replace_needs_the_current_digest() {
        let directory = tempfile::tempdir().unwrap();
        let target = test_support::target(directory.path());
        let path = directory.path().join("settings.json");
        put(&target, &path, b"v1", Replace::Always).unwrap();
        let stale = sha(b"other");
        assert_eq!(
            put(&target, &path, b"v2", Replace::IfSha256(Some(stale)))
                .unwrap_err()
                .code,
            ErrorCode::Changed
        );
        put(&target, &path, b"v2", Replace::IfSha256(Some(sha(b"v1")))).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"v2");
        let missing = directory.path().join("gone");
        assert_eq!(
            put(&target, &missing, b"x", Replace::IfSha256(Some(sha(b"v1"))))
                .unwrap_err()
                .code,
            ErrorCode::Changed
        );
        let malformed = put(&target, &path, b"x", Replace::IfSha256(Some("abc".into())));
        assert!(malformed.is_err());
    }

    #[test]
    fn reads_return_ranges_with_the_metadata_of_the_open_file() {
        let directory = tempfile::tempdir().unwrap();
        let target = test_support::target(directory.path());
        let path = directory.path().join("transcript.jsonl");
        put(&target, &path, b"0123456789", Replace::Always).unwrap();
        let whole = read(&target, &path.to_string_lossy(), None, None).unwrap();
        assert_eq!(
            (whole.data.as_slice(), whole.size, whole.mode),
            (&b"0123456789"[..], 10, 0o640)
        );
        let tail = read(&target, &path.to_string_lossy(), Some(7), Some(100)).unwrap();
        assert_eq!(tail.data, b"789");
        let past = read(&target, &path.to_string_lossy(), Some(50), Some(4)).unwrap();
        assert!(past.data.is_empty());
        assert_eq!(past.size, 10);
        let rest = read(&target, &path.to_string_lossy(), Some(4), None).unwrap();
        assert_eq!(rest.data, b"456789");
        assert_eq!(
            read(
                &target,
                &directory.path().join("none").to_string_lossy(),
                None,
                None
            )
            .unwrap_err()
            .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            read(&target, &directory.path().to_string_lossy(), None, None)
                .unwrap_err()
                .code,
            ErrorCode::NotAFile
        );
        assert!(read(&target, "relative", None, None).is_err());
        assert!(read(&target, &path.to_string_lossy(), Some(u64::MAX), Some(2)).is_err());
        assert!(read(
            &target,
            &path.to_string_lossy(),
            None,
            Some(MAX_RESPONSE_RAW_BYTES as u64 + 1)
        )
        .is_err());
    }

    #[test]
    fn a_range_larger_than_one_response_is_refused_instead_of_truncated() {
        let directory = tempfile::tempdir().unwrap();
        let target = test_support::target(directory.path());
        let path = directory.path().join("big");
        std::fs::write(&path, vec![7u8; MAX_RESPONSE_RAW_BYTES + 10]).unwrap();
        let error = read(&target, &path.to_string_lossy(), None, None).unwrap_err();
        assert_eq!(error.code, ErrorCode::Internal);
        let page = read(&target, &path.to_string_lossy(), Some(10), None).unwrap();
        assert_eq!(page.data.len(), MAX_RESPONSE_RAW_BYTES);
    }

    #[test]
    fn listing_reports_kinds_sizes_and_times_without_following_links() {
        let directory = tempfile::tempdir().unwrap();
        let target = test_support::target(directory.path());
        std::fs::write(directory.path().join("a file"), b"abc").unwrap();
        std::fs::create_dir(directory.path().join("sub")).unwrap();
        std::os::unix::fs::symlink("a file", directory.path().join("link")).unwrap();
        let entries = list(&target, &directory.path().to_string_lossy()).unwrap();
        let by_name = |name: &str| entries.iter().find(|entry| entry.name == name).unwrap();
        // The fake runtime's own files live here too; only the fixtures are checked.
        assert_eq!(by_name("a file").kind, FileKind::File);
        assert_eq!(by_name("a file").size, 3);
        assert!(by_name("a file").mtime_ms > 1_600_000_000_000);
        assert_eq!(by_name("sub").kind, FileKind::Dir);
        assert_eq!(by_name("link").kind, FileKind::Link);
        assert_eq!(
            list(&target, &directory.path().join("none").to_string_lossy())
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn removal_unlinks_files_and_links_forgives_missing_paths_and_refuses_folders() {
        let directory = tempfile::tempdir().unwrap();
        let target = test_support::target(directory.path());
        let file = directory.path().join("file");
        let precious = directory.path().join("precious");
        std::fs::write(&file, b"x").unwrap();
        std::fs::write(&precious, b"keep").unwrap();
        let link = directory.path().join("link");
        std::os::unix::fs::symlink(&precious, &link).unwrap();
        let folder = directory.path().join("folder");
        std::fs::create_dir(&folder).unwrap();
        let names = |paths: &[&std::path::Path]| {
            paths
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        };
        let removed = remove(
            &target,
            &names(&[&file, &link, &directory.path().join("never-existed")]),
        )
        .unwrap();
        assert_eq!(removed.len(), 3);
        assert!(!file.exists() && std::fs::symlink_metadata(&link).is_err());
        assert!(precious.exists(), "the link's target must be left alone");
        let error = remove(&target, &names(&[&folder])).unwrap_err();
        assert_eq!(error.code, ErrorCode::NotAFile);
        assert!(folder.exists());
        assert!(remove(&target, &[]).unwrap().is_empty());
    }

    #[test]
    fn a_stopped_computer_is_never_started_by_file_access() {
        let directory = tempfile::tempdir().unwrap();
        let paths = test_support::runtime(directory.path());
        crate::test_support::write_shell_script(
            &paths.executable,
            r#"case "$1" in inspect) printf '%s\n' '{"name":"dev","status":"Stopped","config":{"labels":{"silo.managed":"true"}}}';; esac"#,
        );
        let target =
            crate::guest::resolve(Ok(paths), directory.path(), test_support::COMPUTER_ID).unwrap();
        let error = read(&target, "/tmp/x", None, None).unwrap_err();
        assert_eq!(error.code, ErrorCode::NotRunning);
        assert_eq!(error.message, "Start dev first.");
    }
}
