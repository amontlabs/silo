//! The pinned guest image: its lock, and the local-only import of the downloaded archive.
//! `preparation` downloads and verifies the archive; this module never reaches a registry.
use super::{prepare_runtime_home, RuntimeError, RuntimePaths, RuntimeRunner};
use flate2::read::GzDecoder;
use microsandbox_image::{Digest as ImageDigest, GlobalCache, Reference};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};

static IMPORT_LOCK: Mutex<()> = Mutex::new(());
const MAX_ARCHIVE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_UNPACKED_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const LOCK: &str = include_str!("../../guest-image/image-lock.json");
/// The file name of the published archive inside `<root>/<version>/`.
pub(crate) const ARCHIVE_FILE: &str = "image.tar.gz";

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GuestImageManifest {
    schema_version: u8,
    pub(crate) version: String,
    architecture: String,
    pub(crate) image_reference: String,
    image_digest: String,
    pub(crate) archive_sha256: String,
    pub(crate) archive_bytes: u64,
    pub(crate) unpacked_bytes: u64,
}

/// The image this build pins for the device's architecture and where its archive is published.
#[derive(Clone, Debug)]
pub(crate) struct PinnedImage {
    pub(crate) manifest: GuestImageManifest,
    pub(crate) url: String,
}

impl PinnedImage {
    /// The image pinned in `guest-image/image-lock.json` for this device.
    pub(crate) fn host() -> Result<Self, String> {
        let key = match std::env::consts::ARCH {
            "aarch64" => "arm64",
            "x86_64" => "amd64",
            _ => return Err("Silo's VM image does not support this device.".into()),
        };
        Self::parse(LOCK, key, std::env::consts::ARCH)
    }

    fn parse(json: &str, key: &str, architecture: &str) -> Result<Self, String> {
        let invalid = || "Silo's VM image information is invalid. Update Silo.".to_owned();
        let lock: serde_json::Value = serde_json::from_str(json).map_err(|_| invalid())?;
        let release = lock["releaseUrl"].as_str().ok_or_else(invalid)?;
        let manifest: GuestImageManifest =
            serde_json::from_value(lock["images"][key].clone()).map_err(|_| invalid())?;
        if !release.starts_with("https://")
            || release.ends_with('/')
            || manifest.architecture != architecture
            || !safe_version(&manifest.version)
        {
            return Err(invalid());
        }
        manifest.validate().map_err(|_| invalid())?;
        Ok(Self {
            url: format!("{release}/image-{key}.tar.gz"),
            manifest,
        })
    }
}

impl GuestImageManifest {
    fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1
            || !valid_sha256(&self.archive_sha256)
            || !self
                .image_digest
                .strip_prefix("sha256:")
                .is_some_and(valid_sha256)
            || self.image_reference.parse::<Reference>().is_err()
            || self.archive_bytes == 0
            || self.archive_bytes > MAX_ARCHIVE_BYTES
            || self.unpacked_bytes == 0
            || self.unpacked_bytes > MAX_UNPACKED_BYTES
        {
            return Err("Silo's VM image information is invalid. Update Silo.".into());
        }
        Ok(())
    }

    /// Where the verified archive of this image is published below `root`.
    pub(crate) fn archive_path(&self, root: &Path) -> PathBuf {
        root.join(&self.version).join(ARCHIVE_FILE)
    }
}

fn safe_version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && !value.starts_with('.')
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}

/// The recipe version of the pinned image (for example `ubuntu-24.04-v4`).
pub(crate) fn pinned_version() -> Option<String> {
    PinnedImage::host()
        .ok()
        .map(|pinned| pinned.manifest.version)
}

#[cfg(test)]
thread_local! {
    static TEST_VERSION: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// The image version tests run against on this thread: v3 unless pinned.
#[cfg(test)]
pub(crate) fn test_version() -> Option<String> {
    Some(TEST_VERSION.with(|slot| {
        slot.borrow()
            .clone()
            .unwrap_or_else(|| "ubuntu-24.04-v3".into())
    }))
}

/// Pins the image version for the current test thread until the guard drops.
#[cfg(test)]
pub(crate) fn pin_test_version(version: &str) -> TestVersionGuard {
    let previous = TEST_VERSION.with(|slot| slot.replace(Some(version.into())));
    TestVersionGuard { previous }
}

#[cfg(test)]
pub(crate) struct TestVersionGuard {
    previous: Option<String>,
}

#[cfg(test)]
impl Drop for TestVersionGuard {
    fn drop(&mut self) {
        TEST_VERSION.with(|slot| *slot.borrow_mut() = self.previous.take());
    }
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn open_archive(path: &Path) -> Result<File, String> {
    let mut options = File::options();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(path).map_err(|_| {
        "Silo's VM image is not downloaded yet. Wait for the download to finish, or retry."
    })?;
    let metadata = file
        .metadata()
        .map_err(|_| "Silo's VM image could not be read.")?;
    if !metadata.is_file() {
        return Err("Silo's VM image is not a regular file. Retry to download it again.".into());
    }
    Ok(file)
}

fn hash_archive(archive: &mut File, manifest: &GuestImageManifest) -> Result<(), String> {
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 128 * 1024];
    let mut length = 0u64;
    loop {
        let count = archive
            .read(&mut buffer)
            .map_err(|_| "Silo's VM image could not be read.")?;
        if count == 0 {
            break;
        }
        length += count as u64;
        hash.update(&buffer[..count]);
    }
    if length != manifest.archive_bytes
        || format!("{:x}", hash.finalize()) != manifest.archive_sha256
    {
        return Err(
            "Silo's downloaded VM image failed its integrity check. Retry to download it again."
                .into(),
        );
    }
    Ok(())
}

fn cached(cache: &GlobalCache, manifest: &GuestImageManifest) -> bool {
    let Ok(reference) = manifest.image_reference.parse::<Reference>() else {
        return false;
    };
    let Ok(Some(metadata)) = cache.read_image_metadata(&reference) else {
        return false;
    };
    if metadata.config_digest != manifest.image_digest {
        return false;
    }
    let Ok(digest) = metadata.manifest_digest.parse::<ImageDigest>() else {
        return false;
    };
    let Ok(layers) = metadata
        .layers
        .iter()
        .map(|layer| layer.diff_id.parse::<ImageDigest>())
        .collect::<Result<Vec<_>, _>>()
    else {
        return false;
    };
    cache.is_fsmeta_materialized(&digest)
        && cache.is_vmdk_materialized(&digest)
        && cache.all_layers_materialized(&layers)
}

pub(crate) fn check_space(directory: &Path, required: u64) -> Result<(), String> {
    let path = std::ffi::CString::new(directory.as_os_str().as_encoded_bytes())
        .map_err(|_| "Invalid VM image storage location.")?;
    let mut statistics = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    if unsafe { libc::statvfs(path.as_ptr(), statistics.as_mut_ptr()) } != 0 {
        return Err("Silo could not check free disk space for its VM image. Check storage access and retry.".into());
    }
    let statistics = unsafe { statistics.assume_init() };
    let available = (statistics.f_bavail as u64).saturating_mul(statistics.f_frsize as u64);
    if available < required {
        return Err(format!(
            "Free at least {} MiB to prepare Silo's VM image, then retry.",
            required.div_ceil(1024 * 1024)
        ));
    }
    Ok(())
}

fn unpack(archive: &Path, output: &mut File, expected_bytes: u64) -> Result<(), String> {
    let input = open_archive(archive)?;
    let mut decoder = GzDecoder::new(input).take(expected_bytes + 1);
    let written = std::io::copy(&mut decoder, output)
        .map_err(|_| "Silo's VM image could not be unpacked. Check disk space and retry.")?;
    if written != expected_bytes {
        return Err(
            "Silo's VM image has an invalid unpacked size. Retry to download it again.".into(),
        );
    }
    output
        .flush()
        .map_err(|_| "Silo's unpacked VM image could not be saved. Check disk space and retry.")?;
    Ok(())
}

/// Whether the pinned image is already in the runtime's image cache. Reads the cache metadata
/// only, so an image imported by an earlier Silo counts without its archive.
pub(crate) fn is_imported(paths: &RuntimePaths) -> bool {
    PinnedImage::host().is_ok_and(|pinned| is_imported_as(paths, &pinned.manifest))
}

pub(crate) fn is_imported_as(paths: &RuntimePaths, manifest: &GuestImageManifest) -> bool {
    // A read-only check must not create the runtime home: before the alias is prepared,
    // creating `cache` beneath it would make the alias a real directory.
    let directory = paths.home.join("cache");
    directory.is_dir() && GlobalCache::new(&directory).is_ok_and(|cache| cached(&cache, manifest))
}

/// Imports the pinned image from its downloaded archive unless the cache already holds it.
pub(crate) fn prepare<R: RuntimeRunner + ?Sized>(
    runner: &R,
    paths: &RuntimePaths,
) -> Result<String, RuntimeError> {
    let pinned = PinnedImage::host().map_err(RuntimeError::Unavailable)?;
    prepare_as(runner, paths, &pinned.manifest)
}

pub(crate) fn prepare_as<R: RuntimeRunner + ?Sized>(
    runner: &R,
    paths: &RuntimePaths,
    manifest: &GuestImageManifest,
) -> Result<String, RuntimeError> {
    let _lock = IMPORT_LOCK.lock().map_err(|_| {
        RuntimeError::Unavailable(
            "VM image preparation is unavailable. Restart Silo and retry.".into(),
        )
    })?;
    manifest.validate().map_err(RuntimeError::Unavailable)?;
    // The cache lives below the runtime home, which may be a symbolic link that
    // only this call creates; it must exist before anything is created under it.
    prepare_runtime_home(&paths.home, paths.storage_home.as_deref())?;
    let cache = GlobalCache::new(&paths.home.join("cache")).map_err(|_| {
        RuntimeError::Unavailable(
            "Silo's VM image storage could not be opened. Check storage access and retry.".into(),
        )
    })?;
    // An image already in the cache needs no archive, so the archive is only
    // opened and hashed when it is about to be imported.
    if cached(&cache, manifest) {
        return Ok(manifest.image_reference.clone());
    }
    let archive_path = manifest.archive_path(&paths.guest_image);
    let mut downloaded = open_archive(&archive_path).map_err(RuntimeError::Unavailable)?;
    hash_archive(&mut downloaded, manifest).map_err(RuntimeError::Unavailable)?;
    // Tar staging plus uncompressed layers and materialized filesystem data. This
    // is temporary import space, not a minimum capacity imposed on each computer.
    check_space(cache.tmp_dir(), manifest.unpacked_bytes.saturating_mul(4))
        .map_err(RuntimeError::Unavailable)?;
    let mut archive = tempfile::NamedTempFile::new_in(cache.tmp_dir()).map_err(|_| {
        RuntimeError::Unavailable("Silo could not prepare temporary VM image storage.".into())
    })?;
    unpack(
        &archive_path,
        archive.as_file_mut(),
        manifest.unpacked_bytes,
    )
    .map_err(RuntimeError::Unavailable)?;
    runner
        .run(
            paths,
            &[
                "image".into(),
                "load".into(),
                "--input".into(),
                archive.path().to_string_lossy().into_owned(),
                "--tag".into(),
                manifest.image_reference.clone(),
                "--quiet".into(),
            ],
            Duration::from_secs(300),
        )
        .map_err(|error| {
            eprintln!("Silo could not load its VM image into the runtime: {error}");
            RuntimeError::Unavailable(
                "Silo could not prepare its VM image. Check disk space and retry.".into(),
            )
        })?;
    if !cached(&cache, manifest) {
        return Err(RuntimeError::Unavailable(
            "The imported VM image did not pass verification. Retry image preparation.".into(),
        ));
    }
    Ok(manifest.image_reference.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{write::GzEncoder, Compression};
    use serde_json::json;
    use std::fs;

    struct Fixture {
        directory: tempfile::TempDir,
        manifest: GuestImageManifest,
        paths: RuntimePaths,
    }

    /// A pinned image whose archive is published below the paths' guest image root.
    fn fixture() -> Fixture {
        let directory = tempfile::tempdir().unwrap();
        let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(b"test archive").unwrap();
        let archive = encoder.finish().unwrap();
        let manifest: GuestImageManifest = serde_json::from_value(json!({
            "schemaVersion": 1, "version": "ubuntu-24.04-test",
            "architecture": std::env::consts::ARCH,
            "imageReference": "ghcr.io/amontlabs/silo-guest:test",
            "imageDigest": format!("sha256:{}", "a".repeat(64)),
            "archiveSha256": format!("{:x}", Sha256::digest(&archive)),
            "archiveBytes": archive.len(), "unpackedBytes": 12,
        }))
        .unwrap();
        let paths = RuntimePaths {
            guest_image: directory.path().join("guest-image"),
            executable: directory.path().join("msb"),
            home: directory.path().join("home"),
            storage_home: None,
            library: directory.path().join("lib"),
            metadata: directory.path().join("metadata"),
            volumes: directory.path().join("volumes"),
        };
        let path = manifest.archive_path(&paths.guest_image);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, &archive).unwrap();
        Fixture {
            directory,
            manifest,
            paths,
        }
    }

    #[test]
    fn the_embedded_lock_pins_a_valid_image_for_each_architecture() {
        for (key, architecture) in [("arm64", "aarch64"), ("amd64", "x86_64")] {
            let pinned = PinnedImage::parse(LOCK, key, architecture).unwrap();
            assert_eq!(
                pinned.url,
                format!(
                    "https://github.com/amontlabs/silo/releases/download/guest-{}/image-{key}.tar.gz",
                    pinned.manifest.version
                )
            );
            assert!(pinned
                .manifest
                .image_reference
                .contains(&pinned.manifest.version));
        }
        assert!(PinnedImage::host().is_ok());
        assert!(pinned_version().is_some_and(|version| version.starts_with("ubuntu-")));
    }

    #[test]
    fn a_lock_that_is_not_https_or_names_another_architecture_is_rejected() {
        assert!(PinnedImage::parse(LOCK, "arm64", "x86_64").is_err());
        assert!(PinnedImage::parse(LOCK, "riscv", "aarch64").is_err());
        let plain = LOCK.replacen("https://", "http://", 1);
        assert!(PinnedImage::parse(&plain, "arm64", "aarch64").is_err());
        let traversal = LOCK.replacen("\"version\": \"ubuntu-24.04-v4\"", "\"version\": \"..\"", 1);
        assert!(PinnedImage::parse(&traversal, "arm64", "aarch64").is_err());
        assert!(PinnedImage::parse("{", "arm64", "aarch64").is_err());
    }

    #[test]
    fn nested_image_version_guards_restore_the_outer_fixture() {
        let original = test_version();
        {
            let _outer = pin_test_version("ubuntu-24.04-v4");
            {
                let _inner = pin_test_version("ubuntu-24.04-v3");
                assert_eq!(test_version().as_deref(), Some("ubuntu-24.04-v3"));
            }
            assert_eq!(test_version().as_deref(), Some("ubuntu-24.04-v4"));
        }
        assert_eq!(test_version(), original);
    }

    #[test]
    fn the_archive_is_found_by_version_and_checked_by_size_and_checksum() {
        let fixture = fixture();
        let path = fixture.manifest.archive_path(&fixture.paths.guest_image);
        assert!(path.ends_with("guest-image/ubuntu-24.04-test/image.tar.gz"));
        hash_archive(&mut File::open(&path).unwrap(), &fixture.manifest).unwrap();
        let mut changed = fs::read(&path).unwrap();
        *changed.last_mut().unwrap() ^= 0xff;
        fs::write(&path, &changed).unwrap();
        let error = hash_archive(&mut File::open(&path).unwrap(), &fixture.manifest).unwrap_err();
        assert!(error.contains("integrity"), "{error}");
        changed.push(0);
        fs::write(&path, changed).unwrap();
        assert!(hash_archive(&mut File::open(&path).unwrap(), &fixture.manifest).is_err());
    }

    #[test]
    fn decompression_checks_exact_size_and_removes_temporary_file() {
        let fixture = fixture();
        let archive = fixture.manifest.archive_path(&fixture.paths.guest_image);
        let mut output = tempfile::NamedTempFile::new_in(fixture.directory.path()).unwrap();
        let path = output.path().to_owned();
        assert!(unpack(&archive, output.as_file_mut(), 11).is_err());
        drop(output);
        assert!(!path.exists());
        let mut output = tempfile::NamedTempFile::new_in(fixture.directory.path()).unwrap();
        unpack(&archive, output.as_file_mut(), 12).unwrap();
    }

    struct FailingImporter(std::sync::atomic::AtomicUsize);
    impl RuntimeRunner for FailingImporter {
        fn run(
            &self,
            _paths: &RuntimePaths,
            args: &[String],
            _timeout: Duration,
        ) -> Result<super::super::CommandOutput, RuntimeError> {
            assert_eq!(&args[..2], ["image", "load"]);
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(RuntimeError::Unavailable("interrupted".into()))
        }
    }

    #[test]
    fn failed_import_is_not_successful_and_retry_reimports() {
        let fixture = fixture();
        let runner = FailingImporter(std::sync::atomic::AtomicUsize::new(0));
        for _ in 0..2 {
            assert!(prepare_as(&runner, &fixture.paths, &fixture.manifest)
                .unwrap_err()
                .to_string()
                .contains("could not prepare"));
            assert_eq!(
                fs::read_dir(fixture.paths.home.join("cache/tmp"))
                    .unwrap()
                    .count(),
                0
            );
        }
        assert_eq!(runner.0.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[test]
    fn an_uncached_image_without_a_verified_archive_is_not_imported() {
        struct Unreachable;
        impl RuntimeRunner for Unreachable {
            fn run(
                &self,
                _: &RuntimePaths,
                _: &[String],
                _: Duration,
            ) -> Result<super::super::CommandOutput, RuntimeError> {
                panic!("an image without a verified archive must not be imported");
            }
        }
        let fixture = fixture();
        assert!(!is_imported_as(&fixture.paths, &fixture.manifest));
        let archive = fixture.manifest.archive_path(&fixture.paths.guest_image);
        fs::write(&archive, b"tampered").unwrap();
        assert!(prepare_as(&Unreachable, &fixture.paths, &fixture.manifest)
            .unwrap_err()
            .to_string()
            .contains("integrity"));
        fs::remove_file(&archive).unwrap();
        assert!(prepare_as(&Unreachable, &fixture.paths, &fixture.manifest)
            .unwrap_err()
            .to_string()
            .contains("not downloaded yet"));
    }

    #[test]
    fn insufficient_space_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(check_space(dir.path(), u64::MAX)
            .unwrap_err()
            .contains("Free at least"));
    }

    /// Succeeds at `image load` only if the runtime home is prepared the way the real runner
    /// prepares it, recording what that preparation saw.
    struct HomePreparingImporter(std::sync::Mutex<Vec<bool>>);
    impl RuntimeRunner for HomePreparingImporter {
        fn run(
            &self,
            paths: &RuntimePaths,
            _args: &[String],
            _timeout: Duration,
        ) -> Result<super::super::CommandOutput, RuntimeError> {
            let prepared = prepare_runtime_home(&paths.home, paths.storage_home.as_deref());
            self.0.lock().unwrap().push(prepared.is_ok());
            prepared.map(|()| unreachable_output())
        }
    }

    fn unreachable_output() -> super::super::CommandOutput {
        super::super::CommandOutput {
            stdout: String::new(),
            stderr: String::new(),
        }
    }

    /// A fixture whose alias and storage sit below a short root, which keeps the modeled
    /// control socket under macOS's 104-byte limit.
    fn aliased_fixture() -> (Fixture, tempfile::TempDir) {
        let mut fixture = fixture();
        let root = tempfile::Builder::new()
            .prefix("silo")
            .tempdir_in(crate::test_support::live::temp_root())
            .unwrap();
        fixture.paths.home = root.path().join("a/h");
        fixture.paths.storage_home = Some(root.path().join("s"));
        (fixture, root)
    }

    #[cfg(unix)]
    #[test]
    fn a_fresh_install_creates_the_runtime_alias_as_a_symlink_before_importing() {
        let _test_state = crate::test_support::global_state();
        let (fixture, _root) = aliased_fixture();
        let storage = fixture.paths.storage_home.clone().unwrap();
        let runner = HomePreparingImporter(Default::default());
        // The fake runner cannot populate the cache, so verification fails after the load.
        let error = prepare_as(&runner, &fixture.paths, &fixture.manifest).unwrap_err();
        assert!(
            error.to_string().contains("did not pass verification"),
            "{error}"
        );
        assert_eq!(*runner.0.lock().unwrap(), [true]);
        assert_eq!(fs::read_link(&fixture.paths.home).unwrap(), storage);
        assert!(storage.join("cache/tmp").is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn checking_for_an_image_does_not_create_the_runtime_home() {
        let _test_state = crate::test_support::global_state();
        let (fixture, _root) = aliased_fixture();
        assert!(!is_imported_as(&fixture.paths, &fixture.manifest));
        assert!(fs::symlink_metadata(&fixture.paths.home).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn an_empty_cache_skeleton_left_at_the_alias_is_recovered_but_data_is_not() {
        let _test_state = crate::test_support::global_state();
        let (fixture, _root) = aliased_fixture();
        let storage = fixture.paths.storage_home.clone().unwrap();
        let home = &fixture.paths.home;
        fs::create_dir_all(home.join("cache/tmp")).unwrap();
        prepare_runtime_home(home, Some(&storage)).unwrap();
        assert_eq!(fs::read_link(home).unwrap(), storage);
        fs::remove_file(home).unwrap();
        fs::create_dir_all(home.join("cache/tmp")).unwrap();
        fs::write(home.join("cache/tmp/keep"), b"data").unwrap();
        let error = prepare_runtime_home(home, Some(&storage)).unwrap_err();
        assert!(error.to_string().contains("No existing data was changed"));
        assert!(home.join("cache/tmp/keep").is_file());
        fs::remove_file(home.join("cache/tmp/keep")).unwrap();
        fs::write(home.join("other"), b"data").unwrap();
        assert!(prepare_runtime_home(home, Some(&storage)).is_err());
        assert!(home.join("other").is_file());
    }

    #[test]
    #[ignore = "requires signed bundled msb, hypervisor access and a downloaded guest image"]
    fn live_downloaded_image_import_and_cache_reuse() {
        crate::test_support::live::require_confirmation();
        use super::super::{CommandOutput, ProcessRunner};
        struct NoImport;
        impl RuntimeRunner for NoImport {
            fn run(
                &self,
                _: &RuntimePaths,
                _: &[String],
                _: Duration,
            ) -> Result<CommandOutput, RuntimeError> {
                panic!("A verified cached image must not be imported again");
            }
        }
        let pinned = PinnedImage::host().unwrap();
        let directory = tempfile::Builder::new()
            .prefix("silo-image-live-")
            .tempdir_in(crate::test_support::live::temp_root())
            .unwrap();
        let guest_image = directory.path().join("guest-image");
        let archive = pinned.manifest.archive_path(&guest_image);
        fs::create_dir_all(archive.parent().unwrap()).unwrap();
        fs::copy(
            std::env::var("SILO_TEST_GUEST_ARCHIVE").expect("set SILO_TEST_GUEST_ARCHIVE"),
            &archive,
        )
        .unwrap();
        let paths = RuntimePaths {
            guest_image,
            executable: std::env::var("SILO_TEST_MSB")
                .expect("set SILO_TEST_MSB")
                .into(),
            library: std::env::var("SILO_TEST_LIBKRUNFW")
                .expect("set SILO_TEST_LIBKRUNFW")
                .into(),
            home: directory.path().join("msb"),
            storage_home: None,
            metadata: directory.path().join("computers.json"),
            volumes: directory.path().join("volumes"),
        };
        struct CountingImporter(std::sync::atomic::AtomicUsize);
        impl RuntimeRunner for CountingImporter {
            fn run(
                &self,
                paths: &RuntimePaths,
                args: &[String],
                timeout: Duration,
            ) -> Result<CommandOutput, RuntimeError> {
                self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                ProcessRunner.run(paths, args, timeout)
            }
        }
        let runner = CountingImporter(std::sync::atomic::AtomicUsize::new(0));
        let image = std::thread::scope(|scope| {
            let first = scope.spawn(|| prepare(&runner, &paths));
            let second = scope.spawn(|| prepare(&runner, &paths));
            let image = first.join().unwrap().unwrap();
            assert_eq!(second.join().unwrap().unwrap(), image);
            image
        });
        assert_eq!(runner.0.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(prepare(&NoImport, &paths).unwrap(), image);
        assert_eq!(
            fs::read_dir(paths.home.join("cache/tmp")).unwrap().count(),
            0
        );
        assert!(is_imported(&paths));
    }
}
