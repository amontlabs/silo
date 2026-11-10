//! The credential store of a headless Linux host: one file per credential.
//!
//! A server has no desktop session, so the Secret Service (which needs a session bus and an
//! unlocked login keyring) is not there to ask. The engine daemon installs this store as the
//! `keyring` default instead, so every `keyring::Entry::new` in the engine (GitHub sign-in,
//! token ledger, personal token, secret values, proxy CAs) reads and writes files.
//!
//! Layout: `<root>/<service>/<account>`, the names encoded with [`file_name`]. `<root>` and
//! each service directory are mode 0700 and owned by this account; each credential file is
//! mode 0600. [`install`] also checks every directory from the XDG data directory down to
//! `<root>`: each must belong to this account or root and be writable by nobody else, so no
//! other account can move the store. A directory its owner's user-private group can write to
//! counts as writable by nobody else (`crate::safe_directories`, Debian's `StrictModes`
//! rule), since `~/.local/share` is mode 0775 in that group for accounts with umask 002.
//! Missing directories are created with fixed modes: 0755 up to the XDG data directory, 0700
//! below it. Every step after opening `<root>` goes through directory
//! descriptors (`openat`, `renameat`, `unlinkat`, `fsync` with `O_NOFOLLOW`), so a link
//! planted in a path is never followed. A file is written whole (a new `.staged-*` file,
//! `fsync`, then `renameat` over the old one, then `fsync` of the directory), so a reader sees
//! the old value or the new one; staged files a crash left behind are removed when the engine
//! installs the store and when it deletes a credential. A directory or file that is a link,
//! belongs to another account, or (for a file) can be read by others is refused rather than
//! used: the refusal is a [`StoreRefusal`], which names the file and never the value.
//!
//! The values are plain bytes on disk, readable by this account and by root: the same trust
//! as the engine's own state (`docs/SiloUI-SECRETS.md`).

use std::{
    any::Any,
    fmt,
    fs::File,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};

use crate::safe_directories::{self, AccountDatabase, SystemAccounts};
use keyring::credential::{Credential, CredentialApi, CredentialBuilderApi, CredentialPersistence};
use rustix::{
    fd::{AsFd, OwnedFd},
    fs::{
        fchmod, fstat, fsync, mkdirat, openat, renameat, statat, unlinkat, AtFlags, Dir, FileType,
        Mode, OFlags, Stat, CWD,
    },
};

/// The largest credential the store reads back. The engine's largest item, the token ledger,
/// stays far below it.
const MAX_CREDENTIAL_BYTES: u64 = 4 * 1024 * 1024;
/// The longest encoded service or account name: one path component, below `NAME_MAX`.
const MAX_ENCODED_NAME: usize = 200;
/// The prefix of a file being written; never a valid encoded name, which cannot start with `.`.
const STAGED_PREFIX: &str = ".staged-";
/// The most staged leftovers one sweep removes from one directory.
const SWEEP_LIMIT: usize = 1024;

/// The directory of the file store under an engine data directory (`EnginePaths::app_data`).
pub const DIRECTORY_NAME: &str = "credentials";

/// Writes, deletes and sweeps of this process take turns, so a sweep never removes a file
/// another thread is about to rename. The engine owner is the only process that writes.
static WRITES: Mutex<()> = Mutex::new(());

fn writing() -> std::sync::MutexGuard<'static, ()> {
    WRITES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Why the store refused a file or directory: its path and the reason, never a value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreRefusal {
    path: PathBuf,
    reason: String,
}

impl StoreRefusal {
    /// The file or directory that was refused.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl fmt::Display for StoreRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} {}", self.path.display(), self.reason)
    }
}

impl std::error::Error for StoreRefusal {}

/// Builds [`FileCredential`]s under one root directory.
#[derive(Debug)]
pub struct FileCredentialBuilder {
    root: PathBuf,
}

impl FileCredentialBuilder {
    /// A builder for `root`, which is created (mode 0700) and checked on first use.
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

/// Makes the file store at `root` the process-wide `keyring` default. Every directory from
/// `checked_from` (the XDG data directory, which `root` must be under) down to `root` is
/// created when missing and checked first, so an unusable or unsafe directory fails startup
/// instead of every later credential call. `owner` is true in the engine that owns this
/// channel, the only process that writes: it also removes staged files a crash left behind.
pub fn install(root: PathBuf, checked_from: &Path, owner: bool) -> Result<(), String> {
    prepare(&root, checked_from, owner, &SystemAccounts)?;
    keyring::set_default_credential_builder(Box::new(FileCredentialBuilder::new(root)));
    Ok(())
}

/// Everything [`install`] does but setting the `keyring` default.
fn prepare(
    root: &Path,
    checked_from: &Path,
    owner: bool,
    accounts: &dyn AccountDatabase,
) -> Result<(), String> {
    let unusable = |error: io::Error| {
        format!(
            "The credential directory {} is unusable: {error}",
            root.display()
        )
    };
    check_ancestors(root, checked_from, euid(), accounts).map_err(unusable)?;
    let directory = open_root(root, true)
        .map_err(unusable)?
        .ok_or_else(|| unusable(io::ErrorKind::NotFound.into()))?;
    if owner {
        let _writing = writing();
        sweep_services(&directory, root).map_err(unusable)?;
    }
    Ok(())
}

/// Creates the directories from `checked_from` down to `root`'s parent when they are missing
/// (`checked_from` and the directories above it mode 0755, those below it 0700, through
/// [`safe_directories::create_directories`], which also requires the directory it creates in
/// to be safe) and requires each from `checked_from` down to be a real directory of `euid` or
/// root that nobody else can write to (its owner's private group may:
/// [`safe_directories::others_can_change`]). `root` itself is checked (and narrowed to 0700)
/// by [`open_root`].
fn check_ancestors(
    root: &Path,
    checked_from: &Path,
    euid: u32,
    accounts: &dyn AccountDatabase,
) -> io::Result<()> {
    let parent = root
        .parent()
        .ok_or_else(|| refused(root, "has no parent directory"))?;
    if !root.is_absolute() || !parent.starts_with(checked_from) {
        return Err(refused(
            root,
            &format!("is not inside {}", checked_from.display()),
        ));
    }
    let mut chain = parent
        .ancestors()
        .take_while(|ancestor| ancestor.starts_with(checked_from))
        .collect::<Vec<_>>();
    chain.reverse();
    safe_directories::create_directories(parent, checked_from, euid, accounts)?;
    for directory in chain {
        let stat = statat(CWD, directory, AtFlags::SYMLINK_NOFOLLOW)?;
        if FileType::from_raw_mode(stat.st_mode) != FileType::Directory {
            return Err(refused(directory, "is not a directory"));
        }
        if stat.st_uid != 0 && stat.st_uid != euid {
            return Err(refused(
                directory,
                "belongs to another account, not this one or root",
            ));
        }
        if let Some(reason) = safe_directories::others_can_change(
            directory,
            stat.st_mode,
            stat.st_uid,
            stat.st_gid,
            accounts,
        ) {
            return Err(refused(directory, &reason));
        }
    }
    Ok(())
}

impl CredentialBuilderApi for FileCredentialBuilder {
    fn build(
        &self,
        target: Option<&str>,
        service: &str,
        user: &str,
    ) -> keyring::Result<Box<Credential>> {
        if target.is_some() {
            return Err(keyring::Error::Invalid(
                "target".into(),
                "the file credential store has no targets".into(),
            ));
        }
        let service_name = file_name(service)
            .map_err(|reason| keyring::Error::Invalid("service".into(), reason))?;
        let account_name =
            file_name(user).map_err(|reason| keyring::Error::Invalid("user".into(), reason))?;
        let directory = self.root.join(&service_name);
        Ok(Box::new(FileCredential {
            root: self.root.clone(),
            path: directory.join(&account_name),
            directory,
            service: service_name,
            account: account_name,
        }))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn persistence(&self) -> CredentialPersistence {
        CredentialPersistence::UntilDelete
    }
}

/// One credential: the file `<root>/<service>/<account>`.
#[derive(Debug)]
pub struct FileCredential {
    root: PathBuf,
    directory: PathBuf,
    path: PathBuf,
    /// The encoded names: one path component each.
    service: String,
    account: String,
}

impl FileCredential {
    /// The file that holds this credential.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The root and service directories, or `None` when either is missing (and not `create`).
    fn service_directory(&self, create: bool) -> io::Result<Option<OwnedFd>> {
        let Some(root) = open_root(&self.root, create)? else {
            return Ok(None);
        };
        open_directory(&root, &self.service, &self.directory, create)
    }
}

impl CredentialApi for FileCredential {
    fn set_secret(&self, secret: &[u8]) -> keyring::Result<()> {
        let _writing = writing();
        let directory = self
            .service_directory(true)
            .map_err(|error| failure(&self.path, error))?
            .ok_or_else(|| failure(&self.directory, io::ErrorKind::NotFound.into()))?;
        write_atomically(&directory, &self.account, &self.path, secret)
            .map_err(|error| failure(&self.path, error))
    }

    fn get_secret(&self) -> keyring::Result<Vec<u8>> {
        let Some(directory) = self
            .service_directory(false)
            .map_err(|error| failure(&self.path, error))?
        else {
            return Err(keyring::Error::NoEntry);
        };
        match read_private_file(&directory, &self.account, &self.path) {
            Ok(bytes) => Ok(bytes),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Err(keyring::Error::NoEntry),
            Err(error) => Err(failure(&self.path, error)),
        }
    }

    fn delete_credential(&self) -> keyring::Result<()> {
        let _writing = writing();
        let Some(directory) = self
            .service_directory(false)
            .map_err(|error| failure(&self.path, error))?
        else {
            return Err(keyring::Error::NoEntry);
        };
        sweep_staged(&directory, &self.directory).map_err(|error| failure(&self.path, error))?;
        // The file is checked before it is removed, so a link planted in its place is
        // reported rather than silently unlinked.
        match statat(&directory, self.account.as_str(), AtFlags::SYMLINK_NOFOLLOW) {
            Err(rustix::io::Errno::NOENT) => return Err(keyring::Error::NoEntry),
            Err(error) => return Err(failure(&self.path, error.into())),
            Ok(stat) => {
                check_private_file(&self.path, &stat).map_err(|error| failure(&self.path, error))?
            }
        }
        match unlinkat(&directory, self.account.as_str(), AtFlags::empty()) {
            Ok(()) => fsync(&directory).map_err(|error| failure(&self.path, error.into())),
            Err(rustix::io::Errno::NOENT) => Err(keyring::Error::NoEntry),
            Err(error) => Err(failure(&self.path, error.into())),
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// A store failure. A refusal (permission) is a [`StoreRefusal`] naming the file, so the
/// engine reports it as `credential_access_denied`; other errors name the file and the
/// reason. Neither ever holds the value.
fn failure(path: &Path, error: io::Error) -> keyring::Error {
    if error.kind() != io::ErrorKind::PermissionDenied {
        return keyring::Error::PlatformFailure(Box::new(error));
    }
    let message = error.to_string();
    let refusal = match error
        .into_inner()
        .map(|inner| inner.downcast::<StoreRefusal>())
    {
        Some(Ok(refusal)) => *refusal,
        _ => StoreRefusal {
            path: path.to_path_buf(),
            reason: format!("cannot be used ({message})"),
        },
    };
    keyring::Error::NoStorageAccess(Box::new(refusal))
}

/// One path component for a service or account name. Letters, digits, `-`, `_` and a `.`
/// that does not start the name are kept; every other byte becomes `%XX`, so distinct names
/// never share a file and no name can be `.`, `..`, start with `.staged-` or contain `/`.
pub fn file_name(name: &str) -> Result<String, String> {
    if name.is_empty() {
        return Err("must not be empty".into());
    }
    let mut encoded = String::with_capacity(name.len());
    for (index, byte) in name.bytes().enumerate() {
        let keep = byte.is_ascii_alphanumeric()
            || byte == b'-'
            || byte == b'_'
            || (byte == b'.' && index > 0);
        if keep {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    if encoded.len() > MAX_ENCODED_NAME {
        return Err(format!(
            "is longer than {MAX_ENCODED_NAME} bytes once encoded"
        ));
    }
    Ok(encoded)
}

fn euid() -> u32 {
    // SAFETY: geteuid has no preconditions.
    unsafe { libc::geteuid() }
}

fn dir_flags() -> OFlags {
    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC
}

/// Opens the store's root directory. With `create`, its missing parents and the root itself
/// (mode 0700) are created first (the engine data directory); without it a missing root is
/// `None`.
fn open_root(root: &Path, create: bool) -> io::Result<Option<OwnedFd>> {
    if create {
        if let Some(parent) = root.parent() {
            safe_directories::create_directories(
                parent,
                parent.parent().unwrap_or(parent),
                euid(),
                &SystemAccounts,
            )?;
        }
    }
    open_directory(CWD, root, root, create)
}

/// Opens `name` below `parent` as a directory without following a link, creating it (mode
/// 0700) when `create`. It must belong to this account; with `create` a wider mode is
/// narrowed to 0700, like the engine's other private directories. `path` names it in errors.
fn open_directory<P: rustix::path::Arg>(
    parent: impl AsFd,
    name: P,
    path: &Path,
    create: bool,
) -> io::Result<Option<OwnedFd>> {
    let name = name.into_c_str()?;
    if create {
        match mkdirat(&parent, name.as_ref(), Mode::from_bits_truncate(0o700)) {
            Ok(()) | Err(rustix::io::Errno::EXIST) => {}
            Err(error) => return Err(error.into()),
        }
    }
    let directory = match openat(&parent, name.as_ref(), dir_flags(), Mode::empty()) {
        Ok(directory) => directory,
        Err(rustix::io::Errno::NOENT) if !create => return Ok(None),
        Err(error @ (rustix::io::Errno::LOOP | rustix::io::Errno::NOTDIR)) => {
            return Err(refused(path, &format!("is not a directory ({error})")))
        }
        Err(error) => return Err(error.into()),
    };
    let stat = fstat(&directory)?;
    if stat.st_uid != euid() {
        return Err(refused(path, "is not a directory owned by this account"));
    }
    if create && stat.st_mode & 0o077 != 0 {
        fchmod(&directory, Mode::from_bits_truncate(0o700))?;
    }
    Ok(Some(directory))
}

fn refused(path: &Path, reason: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        StoreRefusal {
            path: path.to_path_buf(),
            reason: reason.to_owned(),
        },
    )
}

/// A credential file must be a regular file of this account that nobody else can read.
fn check_private_file(path: &Path, stat: &Stat) -> io::Result<()> {
    if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile {
        return Err(refused(path, "is not a regular file"));
    }
    if stat.st_uid != euid() {
        return Err(refused(path, "is not owned by this account"));
    }
    if stat.st_mode & 0o077 != 0 {
        return Err(refused(path, "can be read by other accounts"));
    }
    Ok(())
}

fn read_private_file(directory: &OwnedFd, name: &str, path: &Path) -> io::Result<Vec<u8>> {
    let fd = openat(
        directory,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| match error {
        // O_NOFOLLOW on a link.
        rustix::io::Errno::LOOP => refused(path, "is a link"),
        error => error.into(),
    })?;
    let stat = fstat(&fd)?;
    check_private_file(path, &stat)?;
    let too_large = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{} is larger than a credential", path.display()),
        )
    };
    let length = u64::try_from(stat.st_size).unwrap_or(u64::MAX);
    if length > MAX_CREDENTIAL_BYTES {
        return Err(too_large());
    }
    let mut file = File::from(fd);
    let mut bytes = Vec::with_capacity(length as usize);
    Read::by_ref(&mut file)
        .take(MAX_CREDENTIAL_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_CREDENTIAL_BYTES {
        return Err(too_large());
    }
    Ok(bytes)
}

/// Writes `bytes` to a new 0600 file in `directory`, flushes it, renames it over `name` and
/// flushes the directory. `path` names the credential in errors.
fn write_atomically(directory: &OwnedFd, name: &str, path: &Path, bytes: &[u8]) -> io::Result<()> {
    match statat(directory, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat) if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile => {
            return Err(refused(path, "is not a regular file"))
        }
        Ok(_) | Err(rustix::io::Errno::NOENT) => {}
        Err(error) => return Err(error.into()),
    }
    let staged = format!("{STAGED_PREFIX}{}", uuid::Uuid::new_v4().simple());
    let result = (|| {
        let fd = openat(
            directory,
            staged.as_str(),
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_bits_truncate(0o600),
        )?;
        fchmod(&fd, Mode::from_bits_truncate(0o600))?;
        let mut file = File::from(fd);
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        renameat(directory, staged.as_str(), directory, name)?;
        fsync(directory)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = unlinkat(directory, staged.as_str(), AtFlags::empty());
    }
    result
}

/// Removes the staged files a crash left in every service directory under `root`.
fn sweep_services(root: &OwnedFd, root_path: &Path) -> io::Result<()> {
    let mut services = Vec::new();
    for entry in Dir::read_from(root)? {
        let entry = entry?;
        let name = entry.file_name().to_bytes();
        if matches!(name, b"." | b"..")
            || !matches!(entry.file_type(), FileType::Directory | FileType::Unknown)
        {
            continue;
        }
        if let Ok(name) = std::str::from_utf8(name) {
            services.push(name.to_owned());
        }
    }
    for service in services {
        let path = root_path.join(&service);
        // A directory the store would refuse is reported when a credential in it is used.
        if let Ok(Some(directory)) = open_directory(root, service.as_str(), &path, false) {
            sweep_staged(&directory, &path)?;
        }
    }
    Ok(())
}

/// Removes the staged files (regular files of this account named `.staged-*`) in `directory`.
fn sweep_staged(directory: &OwnedFd, path: &Path) -> io::Result<()> {
    let mut staged = Vec::new();
    for entry in Dir::read_from(directory)? {
        let entry = entry?;
        if entry
            .file_name()
            .to_bytes()
            .starts_with(STAGED_PREFIX.as_bytes())
        {
            staged.push(entry.file_name().to_owned());
        }
        if staged.len() >= SWEEP_LIMIT {
            break;
        }
    }
    let mut removed = false;
    for name in staged {
        let Ok(stat) = statat(directory, name.as_c_str(), AtFlags::SYMLINK_NOFOLLOW) else {
            continue;
        };
        if FileType::from_raw_mode(stat.st_mode) == FileType::RegularFile && stat.st_uid == euid() {
            match unlinkat(directory, name.as_c_str(), AtFlags::empty()) {
                Ok(()) => removed = true,
                Err(rustix::io::Errno::NOENT) => {}
                Err(error) => {
                    return Err(io::Error::new(
                        io::Error::from(error).kind(),
                        format!("{} could not remove a staged file: {error}", path.display()),
                    ))
                }
            }
        }
    }
    if removed {
        fsync(directory)?;
    }
    Ok(())
}

/// The file that holds `entry` when this store made it.
pub fn credential_path(entry: &keyring::Entry) -> Option<PathBuf> {
    entry
        .get_credential()
        .downcast_ref::<FileCredential>()
        .map(|credential| credential.path.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::safe_directories::StaticAccounts;
    use std::fs;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    fn entry(root: &Path, service: &str, user: &str) -> keyring::Entry {
        let credential = FileCredentialBuilder::new(root.to_path_buf())
            .build(None, service, user)
            .unwrap();
        keyring::Entry::new_with_credential(credential)
    }

    /// Accounts with no private groups, so every group-writable directory is refused
    /// whoever runs the tests.
    fn no_private_groups() -> StaticAccounts {
        StaticAccounts::default()
    }

    fn mode(path: &Path) -> u32 {
        fs::symlink_metadata(path).unwrap().mode() & 0o777
    }

    #[test]
    fn a_value_round_trips_through_private_files_and_is_deleted() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join("data/org.silo.dev/credentials");
        let item = entry(&root, "org.silo.dev.github", "account");
        assert!(matches!(item.get_password(), Err(keyring::Error::NoEntry)));

        item.set_password("first value").unwrap();
        item.set_password("second value").unwrap();
        assert_eq!(item.get_password().unwrap(), "second value");

        let path = credential_path(&item).unwrap();
        assert_eq!(path, root.join("org.silo.dev.github/account"));
        assert_eq!(mode(&root), 0o700);
        assert_eq!(mode(path.parent().unwrap()), 0o700);
        assert_eq!(mode(&path), 0o600);
        // Only the credential is left: the staged file was renamed over it.
        let names = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        assert_eq!(names, ["account"]);

        item.delete_credential().unwrap();
        assert!(!path.exists());
        assert!(matches!(item.get_password(), Err(keyring::Error::NoEntry)));
        assert!(matches!(
            item.delete_credential(),
            Err(keyring::Error::NoEntry)
        ));
    }

    #[test]
    fn services_and_accounts_do_not_share_files() {
        let home = tempfile::tempdir().unwrap();
        let first = entry(home.path(), "org.silo.dev.secrets", "a/b");
        let second = entry(home.path(), "org.silo.dev.secrets", "a%2Fb");
        let third = entry(home.path(), "org.silo.preview.secrets", "a/b");
        first.set_password("one").unwrap();
        second.set_password("two").unwrap();
        third.set_password("three").unwrap();
        assert_eq!(first.get_password().unwrap(), "one");
        assert_eq!(second.get_password().unwrap(), "two");
        assert_eq!(third.get_password().unwrap(), "three");
    }

    #[test]
    fn names_encode_to_one_safe_path_component() {
        assert_eq!(
            file_name("org.silo.dev.github").unwrap(),
            "org.silo.dev.github"
        );
        assert_eq!(file_name("..").unwrap(), "%2E.");
        assert_eq!(file_name(".hidden").unwrap(), "%2Ehidden");
        assert_eq!(file_name("a/b c").unwrap(), "a%2Fb%20c");
        assert_eq!(file_name("é").unwrap(), "%C3%A9");
        assert!(file_name("").is_err());
        assert!(file_name(&"/".repeat(100)).is_err());
        assert!(FileCredentialBuilder::new(PathBuf::from("/unused"))
            .build(Some("target"), "service", "user")
            .is_err());
    }

    #[test]
    fn a_link_or_a_file_others_can_read_is_refused_without_naming_the_value() {
        let home = tempfile::tempdir().unwrap();
        let item = entry(home.path(), "service", "account");
        item.set_password("secret-value-123").unwrap();
        let path = credential_path(&item).unwrap();

        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        let error = item.get_password().unwrap_err();
        assert!(matches!(error, keyring::Error::NoStorageAccess(_)));
        assert!(!format!("{error} {error:?}").contains("secret-value-123"));

        fs::remove_file(&path).unwrap();
        let elsewhere = home.path().join("elsewhere");
        fs::write(&elsewhere, "planted").unwrap();
        fs::set_permissions(&elsewhere, fs::Permissions::from_mode(0o600)).unwrap();
        std::os::unix::fs::symlink(&elsewhere, &path).unwrap();
        assert!(matches!(
            item.get_password(),
            Err(keyring::Error::NoStorageAccess(_))
        ));
        assert!(item.set_password("new").is_err());
        assert!(item.delete_credential().is_err());
        assert_eq!(fs::read_to_string(&elsewhere).unwrap(), "planted");
    }

    #[test]
    fn a_service_directory_that_is_a_link_is_refused() {
        let home = tempfile::tempdir().unwrap();
        let elsewhere = home.path().join("elsewhere");
        fs::create_dir(&elsewhere).unwrap();
        let root = home.path().join("credentials");
        fs::create_dir(&root).unwrap();
        std::os::unix::fs::symlink(&elsewhere, root.join("service")).unwrap();
        let item = entry(&root, "service", "account");
        assert!(item.set_password("value").is_err());
        assert!(matches!(
            item.get_password(),
            Err(keyring::Error::NoStorageAccess(_))
        ));
        assert_eq!(fs::read_dir(&elsewhere).unwrap().count(), 0);
    }

    #[test]
    fn a_wide_directory_is_narrowed_to_this_account() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join("credentials");
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
        entry(&root, "service", "account")
            .set_password("value")
            .unwrap();
        assert_eq!(mode(&root), 0o700);
    }

    /// Another account's file is refused. Changing a file's owner needs root, so the check
    /// runs only where the test can make one.
    #[test]
    fn a_file_owned_by_another_account_is_refused() {
        let home = tempfile::tempdir().unwrap();
        let item = entry(home.path(), "service", "account");
        item.set_password("value").unwrap();
        let path = credential_path(&item).unwrap();
        let c_path = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: a valid C string; the call only changes the test file's owner.
        if unsafe { libc::lchown(c_path.as_ptr(), 4242, 4242) } != 0 {
            return;
        }
        assert!(matches!(
            item.get_password(),
            Err(keyring::Error::NoStorageAccess(_))
        ));
    }

    #[test]
    fn install_prepares_the_root_and_checks_its_ancestors() {
        let home = tempfile::tempdir().unwrap();
        fs::set_permissions(home.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let data = home.path().join("share");
        let root = data.join("org.silo.dev").join(DIRECTORY_NAME);
        prepare(&root, &data, true, &no_private_groups()).unwrap();
        assert_eq!(mode(&root), 0o700);
        assert_eq!(mode(root.parent().unwrap()), 0o700);
        assert_eq!(mode(&data), 0o755);

        // A directory between the data directory and the store that others can change.
        for wide in [0o775, 0o757, 0o1777] {
            fs::set_permissions(root.parent().unwrap(), fs::Permissions::from_mode(wide)).unwrap();
            let error = prepare(&root, &data, true, &no_private_groups()).unwrap_err();
            assert!(
                error.contains(&format!(
                    "{} can be changed by other accounts",
                    root.parent().unwrap().display()
                )),
                "{error}"
            );
        }
        fs::set_permissions(root.parent().unwrap(), fs::Permissions::from_mode(0o755)).unwrap();
        prepare(&root, &data, true, &no_private_groups()).unwrap();
        fs::set_permissions(&data, fs::Permissions::from_mode(0o777)).unwrap();
        assert!(prepare(&root, &data, true, &no_private_groups()).is_err());
        fs::set_permissions(&data, fs::Permissions::from_mode(0o700)).unwrap();

        // A link on the way, or a store outside the data directory.
        let elsewhere = home.path().join("elsewhere");
        fs::create_dir(&elsewhere).unwrap();
        let linked = data.join("linked");
        std::os::unix::fs::symlink(&elsewhere, &linked).unwrap();
        let error = prepare(
            &linked.join(DIRECTORY_NAME),
            &data,
            true,
            &no_private_groups(),
        )
        .unwrap_err();
        assert!(error.contains("is not a directory"), "{error}");
        assert_eq!(fs::read_dir(&elsewhere).unwrap().count(), 0);
        assert!(prepare(
            &home.path().join("credentials"),
            &data,
            true,
            &no_private_groups()
        )
        .is_err());

        let file = data.join("file");
        fs::write(&file, "").unwrap();
        assert!(prepare(&file, &data, true, &no_private_groups()).is_err());
    }

    /// `~/.local/share` of an account with umask 002 and a user-private group (Debian,
    /// Ubuntu) is mode 0775 in that group, and the store accepts it; a shared group, root's
    /// group, or write access for everyone is still refused with the mode, the group and a fix.
    #[test]
    fn a_data_directory_in_the_owners_private_group_is_accepted() {
        let home = tempfile::tempdir().unwrap();
        fs::set_permissions(home.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let data = home.path().join("share");
        let store = data.join("org.silo.dev");
        let root = store.join(DIRECTORY_NAME);
        fs::create_dir_all(&store).unwrap();
        fs::set_permissions(&data, fs::Permissions::from_mode(0o775)).unwrap();
        fs::set_permissions(&store, fs::Permissions::from_mode(0o700)).unwrap();

        // Root gets no exemption: as root, the directories first stay root's.
        if euid() == 0 {
            let error = check_ancestors(&root, &data, 0, &StaticAccounts::private(0, "root", 0))
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("can be changed by other accounts"),
                "{error}"
            );
        }
        // The account the test plays: this one, or (as root, which has no private group)
        // 4242, which the directories are given.
        let (uid, gid) = if euid() == 0 {
            for path in [&data, &store] {
                let path = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
                assert_eq!(unsafe { libc::lchown(path.as_ptr(), 4242, 4242) }, 0);
            }
            (4242, 4242)
        } else {
            let stat = fs::metadata(&data).unwrap();
            (stat.uid(), stat.gid())
        };
        let private = StaticAccounts::private(uid, "silo-controller", gid);
        check_ancestors(&root, &data, uid, &private).unwrap();

        let mut shared = StaticAccounts::private(uid, "silo-controller", gid);
        shared.add_group(gid, "staff", &["silo-controller", "ben"]);
        let error = check_ancestors(&root, &data, uid, &shared)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains(&format!(
                "{} can be changed by other accounts (mode 775, writable by group staff, which \
                 is not a private group of its owner): run chmod g-w {}",
                data.display(),
                data.display()
            )),
            "{error}"
        );

        fs::set_permissions(&data, fs::Permissions::from_mode(0o777)).unwrap();
        let error = check_ancestors(&root, &data, uid, &private)
            .unwrap_err()
            .to_string();
        assert!(error.contains("writable by everyone"), "{error}");
        assert!(error.contains("chmod go-w"), "{error}");
    }

    /// A missing XDG data directory is created only in a directory nobody else can change.
    #[test]
    fn a_data_directory_is_not_created_where_others_can_write() {
        let home = tempfile::tempdir().unwrap();
        fs::set_permissions(home.path(), fs::Permissions::from_mode(0o777)).unwrap();
        let data = home.path().join("local/share");
        let error = prepare(
            &data.join("org.silo.dev").join(DIRECTORY_NAME),
            &data,
            true,
            &no_private_groups(),
        )
        .unwrap_err();
        assert!(
            error.contains(&format!(
                "{} can be changed by other accounts",
                home.path().display()
            )),
            "{error}"
        );
        assert!(!home.path().join("local").exists());
    }

    #[test]
    fn staged_leftovers_are_swept_by_the_owner_and_by_a_delete() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join("credentials");
        let item = entry(&root, "service", "account");
        item.set_password("value").unwrap();
        let service = root.join("service");
        let leftover = |name: &str| {
            let path = service.join(name);
            fs::write(&path, "half").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
            path
        };
        let names = || {
            let mut names = fs::read_dir(&service)
                .unwrap()
                .map(|entry| entry.unwrap().file_name().into_string().unwrap())
                .collect::<Vec<_>>();
            names.sort();
            names
        };

        let crashed = leftover(".staged-0123");
        prepare(&root, home.path(), false, &no_private_groups()).unwrap();
        assert!(crashed.exists(), "only the owner sweeps");
        prepare(&root, home.path(), true, &no_private_groups()).unwrap();
        assert_eq!(names(), ["account"]);
        assert_eq!(item.get_password().unwrap(), "value");

        leftover(".staged-4567");
        // A link named like a staged file is not this store's: it is left alone.
        std::os::unix::fs::symlink("/etc/hostname", service.join(".staged-link")).unwrap();
        item.delete_credential().unwrap();
        assert_eq!(names(), [".staged-link"]);
    }

    #[test]
    fn a_refusal_is_a_store_refusal_that_names_the_file() {
        let home = tempfile::tempdir().unwrap();
        let item = entry(home.path(), "service", "account");
        item.set_password("secret-value-123").unwrap();
        let path = credential_path(&item).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        let keyring::Error::NoStorageAccess(source) = item.get_password().unwrap_err() else {
            panic!("a refusal");
        };
        let refusal = source.downcast_ref::<StoreRefusal>().unwrap();
        assert_eq!(refusal.path(), path);
        assert_eq!(
            refusal.to_string(),
            format!("{} can be read by other accounts", path.display())
        );
    }

    /// The store is never a folder a computer sees: no runtime mount source (the runtime
    /// storage and its `/.msb` shares, the guest image, the legacy read-only folders, the
    /// runtime home alias) is the credential directory, inside it or above it.
    #[test]
    fn the_credential_directory_is_never_a_runtime_mount_source() {
        let home = Path::new("/home/tester");
        let app_data = home.join(".local/share/org.silo.dev");
        let credentials = app_data.join(DIRECTORY_NAME);
        let mut sources = vec![app_data.join("guest-image")];
        for folder in crate::legacy_mounts::ROOT_FOLDERS {
            sources.push(app_data.join(folder).join("computer"));
        }
        for storage in crate::runtime_migration::STORAGE_FOLDERS {
            let storage = app_data.join(storage);
            let runtime = storage.join("microsandbox");
            sources.push(runtime.join("sandboxes/computer"));
            sources.push(storage.join("volumes"));
            sources.push(storage.clone());
            sources.push(
                crate::channel::Channel::Development
                    .state_dir(home)
                    .join("0123456789ab"),
            );
        }
        for source in sources {
            assert!(
                !source.starts_with(&credentials) && !credentials.starts_with(&source),
                "{} overlaps {}",
                source.display(),
                credentials.display()
            );
        }
    }
}
