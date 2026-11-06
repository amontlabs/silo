//! The credential store of a headless Linux host: one file per credential.
//!
//! A server has no desktop session, so the Secret Service (which needs a session bus and an
//! unlocked login keyring) is not there to ask. The engine daemon installs this store as the
//! `keyring` default instead, so every `keyring::Entry::new` in the engine (GitHub sign-in,
//! token ledger, personal token, secret values, proxy CAs) reads and writes files.
//!
//! Layout: `<root>/<service>/<account>`, the names encoded with [`file_name`]. `<root>` and
//! each service directory are mode 0700 and owned by this account; each credential file is
//! mode 0600. A file is written whole (a new file, `fsync`, then `rename` over the old one), so
//! a reader sees the old value or the new one. A directory or file that is a link, belongs to
//! another account, or (for a file) can be read by others is refused rather than used.
//!
//! The values are plain bytes on disk, readable by this account and by root: the same trust
//! as the engine's own state (`docs/SiloUI-SECRETS.md`). Errors and logs name the file, never
//! the value.

use std::{
    any::Any,
    fs::{self, File},
    io::{self, Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

use keyring::credential::{Credential, CredentialApi, CredentialBuilderApi, CredentialPersistence};

/// The largest credential the store reads back. The engine's largest item, the token ledger,
/// stays far below it.
const MAX_CREDENTIAL_BYTES: u64 = 4 * 1024 * 1024;
/// The longest encoded service or account name: one path component, below `NAME_MAX`.
const MAX_ENCODED_NAME: usize = 200;

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

/// Makes the file store at `root` the process-wide `keyring` default. The root is prepared
/// first, so an unusable directory fails startup instead of every later credential call.
pub fn install(root: PathBuf) -> Result<(), String> {
    prepare_directory(&root, true).map_err(|error| {
        format!(
            "The credential directory {} is unusable: {error}",
            root.display()
        )
    })?;
    keyring::set_default_credential_builder(Box::new(FileCredentialBuilder::new(root)));
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
        let directory = self.root.join(service_name);
        Ok(Box::new(FileCredential {
            root: self.root.clone(),
            path: directory.join(account_name),
            directory,
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
}

impl FileCredential {
    /// The file that holds this credential.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl CredentialApi for FileCredential {
    fn set_secret(&self, secret: &[u8]) -> keyring::Result<()> {
        prepare_directory(&self.root, true).map_err(failure)?;
        prepare_directory(&self.directory, false).map_err(failure)?;
        write_atomically(&self.directory, &self.path, secret).map_err(failure)
    }

    fn get_secret(&self) -> keyring::Result<Vec<u8>> {
        for directory in [&self.root, &self.directory] {
            if !existing_directory(directory).map_err(failure)? {
                return Err(keyring::Error::NoEntry);
            }
        }
        match read_private_file(&self.path) {
            Ok(bytes) => Ok(bytes),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Err(keyring::Error::NoEntry),
            Err(error) => Err(failure(error)),
        }
    }

    fn delete_credential(&self) -> keyring::Result<()> {
        for directory in [&self.root, &self.directory] {
            if !existing_directory(directory).map_err(failure)? {
                return Err(keyring::Error::NoEntry);
            }
        }
        // The file is checked before it is removed, so a link planted in its place is
        // reported rather than silently unlinked.
        match fs::symlink_metadata(&self.path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(keyring::Error::NoEntry)
            }
            Err(error) => return Err(failure(error)),
            Ok(metadata) => check_private_file(&self.path, &metadata).map_err(failure)?,
        }
        match fs::remove_file(&self.path) {
            Ok(()) => sync_directory(&self.directory).map_err(failure),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Err(keyring::Error::NoEntry),
            Err(error) => Err(failure(error)),
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// A store failure. The I/O error names the file and the reason, never the value.
fn failure(error: io::Error) -> keyring::Error {
    if error.kind() == io::ErrorKind::PermissionDenied {
        keyring::Error::NoStorageAccess(Box::new(error))
    } else {
        keyring::Error::PlatformFailure(Box::new(error))
    }
}

/// One path component for a service or account name. Letters, digits, `-`, `_` and a `.`
/// that does not start the name are kept; every other byte becomes `%XX`, so distinct names
/// never share a file and no name can be `.`, `..` or contain `/`.
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

/// Creates `path` (mode 0700) when it is missing, then requires a real directory owned by
/// this account and narrows a wider mode to 0700, like the engine's other private
/// directories. `with_parents` creates missing parents first (the engine data directory).
fn prepare_directory(path: &Path, with_parents: bool) -> io::Result<()> {
    if with_parents {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
    }
    match fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    let directory = File::options()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|error| refused(path, &format!("is not a directory ({error})")))?;
    let metadata = directory.metadata()?;
    if !metadata.is_dir() || metadata.uid() != euid() {
        return Err(refused(path, "is not a directory owned by this account"));
    }
    if metadata.mode() & 0o077 != 0 {
        directory.set_permissions(fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Whether `path` exists as a directory of this account (not a link); an error when something
/// else is there.
fn existing_directory(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && metadata.uid() == euid() => Ok(true),
        Ok(_) => Err(refused(path, "is not a directory owned by this account")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn refused(path: &Path, reason: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!("{} {reason}", path.display()),
    )
}

/// A credential file must be a regular file of this account that nobody else can read.
fn check_private_file(path: &Path, metadata: &fs::Metadata) -> io::Result<()> {
    if !metadata.file_type().is_file() {
        return Err(refused(path, "is not a regular file"));
    }
    if metadata.uid() != euid() {
        return Err(refused(path, "is not owned by this account"));
    }
    if metadata.mode() & 0o077 != 0 {
        return Err(refused(path, "can be read by other accounts"));
    }
    Ok(())
}

fn read_private_file(path: &Path) -> io::Result<Vec<u8>> {
    let mut file = File::options()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)
        .map_err(|error| match error.raw_os_error() {
            // O_NOFOLLOW on a link.
            Some(libc::ELOOP) => refused(path, "is a link"),
            _ => error,
        })?;
    let metadata = file.metadata()?;
    check_private_file(path, &metadata)?;
    if metadata.len() > MAX_CREDENTIAL_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{} is larger than a credential", path.display()),
        ));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    Read::by_ref(&mut file)
        .take(MAX_CREDENTIAL_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_CREDENTIAL_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{} is larger than a credential", path.display()),
        ));
    }
    Ok(bytes)
}

/// Writes `bytes` to a new 0600 file beside `path`, flushes it, and renames it over `path`.
fn write_atomically(directory: &Path, path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if !metadata.file_type().is_file() {
            return Err(refused(path, "is not a regular file"));
        }
    }
    let staged = directory.join(format!(".staged-{}", uuid::Uuid::new_v4().simple()));
    let result = (|| {
        let mut file = File::options()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&staged)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&staged, path)?;
        sync_directory(directory)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&staged);
    }
    result
}

fn sync_directory(directory: &Path) -> io::Result<()> {
    File::options()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(directory)?
        .sync_all()
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

    fn entry(root: &Path, service: &str, user: &str) -> keyring::Entry {
        let credential = FileCredentialBuilder::new(root.to_path_buf())
            .build(None, service, user)
            .unwrap();
        keyring::Entry::new_with_credential(credential)
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
    fn install_prepares_the_root() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join("missing/parents/credentials");
        prepare_directory(&root, true).unwrap();
        assert_eq!(mode(&root), 0o700);
        let file = home.path().join("file");
        fs::write(&file, "").unwrap();
        assert!(prepare_directory(&file, true).is_err());
    }
}
