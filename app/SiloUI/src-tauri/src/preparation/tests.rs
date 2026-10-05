use super::*;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    mpsc, Arc,
};

fn never() -> Result<(), Failure> {
    panic!("nothing should run");
}

#[test]
fn a_ready_job_runs_nothing_and_announces_itself_once() {
    let job = Job::new();
    let changes = AtomicUsize::new(0);
    let change = || {
        changes.fetch_add(1, Ordering::SeqCst);
    };
    for _ in 0..3 {
        job.ensure(&|| true, &|_| never(), &change, &|_| {})
            .unwrap();
    }
    assert_eq!(job.task().state, TaskState::Ready);
    assert_eq!(changes.load(Ordering::SeqCst), 1);
}

#[test]
fn a_caller_joins_the_run_in_flight_instead_of_starting_another() {
    let job = Arc::new(Job::new());
    let runs = Arc::new(AtomicUsize::new(0));
    let (started_sender, started) = mpsc::channel();
    let (release, released) = mpsc::channel::<()>();
    let first = {
        let (job, runs) = (job.clone(), runs.clone());
        std::thread::spawn(move || {
            job.ensure(
                &|| false,
                &|report| {
                    runs.fetch_add(1, Ordering::SeqCst);
                    report(Some(40));
                    started_sender.send(()).unwrap();
                    released.recv().unwrap();
                    Ok(())
                },
                &|| {},
                &|_| {},
            )
        })
    };
    started.recv().unwrap();
    assert_eq!(job.task().state, TaskState::Running);
    assert_eq!(job.task().fraction, Some(40));
    let reported = Arc::new(Mutex::new(Vec::new()));
    let second = {
        let (job, reported) = (job.clone(), reported.clone());
        std::thread::spawn(move || {
            job.ensure(
                &|| panic!("a joiner does not check"),
                &|_| never(),
                &|| {},
                &|fraction| reported.lock().unwrap().push(fraction),
            )
        })
    };
    while reported.lock().unwrap().is_empty() {
        std::thread::yield_now();
    }
    release.send(()).unwrap();
    assert_eq!(first.join().unwrap(), Ok(()));
    assert_eq!(second.join().unwrap(), Ok(()));
    assert_eq!(runs.load(Ordering::SeqCst), 1);
    assert_eq!(reported.lock().unwrap()[0], Some(40));
    assert_eq!(job.task(), PreparationTask::new(TaskState::Ready));
}

#[test]
fn a_failure_is_kept_for_the_ui_and_the_next_call_retries() {
    let job = Job::new();
    let error = job
        .ensure(
            &|| false,
            &|_| {
                Err(Failure {
                    message: "No space left.".into(),
                    retryable: false,
                })
            },
            &|| {},
            &|_| {},
        )
        .unwrap_err();
    assert_eq!(error, "No space left.");
    let task = job.task();
    assert_eq!(task.state, TaskState::Failed);
    assert_eq!(task.message.as_deref(), Some("No space left."));
    assert!(!task.retryable);
    job.ensure(&|| false, &|_| Ok(()), &|| {}, &|_| {}).unwrap();
    assert_eq!(job.task(), PreparationTask::new(TaskState::Ready));
}

#[test]
fn a_joiner_of_a_failed_run_gets_its_failure() {
    let job = Arc::new(Job::new());
    let (started_sender, started) = mpsc::channel();
    let (release, released) = mpsc::channel::<()>();
    let first = {
        let job = job.clone();
        std::thread::spawn(move || {
            job.ensure(
                &|| false,
                &|_| {
                    started_sender.send(()).unwrap();
                    released.recv().unwrap();
                    Err("Disk full.".to_owned().into())
                },
                &|| {},
                &|_| {},
            )
        })
    };
    started.recv().unwrap();
    let waiting = Arc::new(AtomicUsize::new(0));
    let second = {
        let (job, waiting) = (job.clone(), waiting.clone());
        std::thread::spawn(move || {
            job.ensure(&|| false, &|_| never(), &|| {}, &|_| {
                waiting.fetch_add(1, Ordering::SeqCst);
            })
        })
    };
    while waiting.load(Ordering::SeqCst) == 0 {
        std::thread::yield_now();
    }
    release.send(()).unwrap();
    assert_eq!(first.join().unwrap(), Err("Disk full.".into()));
    assert_eq!(second.join().unwrap(), Err("Disk full.".into()));
}

#[test]
fn a_panicking_run_releases_the_job() {
    let job = Job::new();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = job.ensure(&|| false, &|_| panic!("boom"), &|| {}, &|_| {});
    }));
    assert!(result.is_err());
    assert_eq!(job.task().state, TaskState::Failed);
    job.ensure(&|| true, &|_| never(), &|| {}, &|_| {}).unwrap();
}

#[test]
fn the_status_serializes_with_the_documented_names() {
    let value = serde_json::to_value(PreparationStatus {
        image: PreparationTask {
            state: TaskState::Running,
            fraction: Some(5),
            message: None,
            retryable: false,
        },
        lcu: PreparationTask::new(TaskState::Pending),
    })
    .unwrap();
    assert_eq!(value["image"]["state"], "running");
    assert_eq!(value["image"]["fraction"], 5);
    assert_eq!(value["lcu"]["state"], "pending");
    assert_eq!(value["lcu"]["retryable"], false);
}

// ------------------------------------------------------------------ archives

use crate::runtime::{CommandOutput, RuntimeError};
use sha2::{Digest, Sha256};
use std::time::Duration;

const BODY: &[u8] = b"pinned archive body";

fn spec_of(version: &str, archive: &str, bytes: u64) -> ArchiveSpec {
    ArchiveSpec {
        version: version.into(),
        archive: archive.into(),
        url: format!("https://example.test/{archive}"),
        sha256: format!("{:x}", Sha256::digest(BODY)),
        bytes,
        what: "LCU",
        file: "LCU file",
    }
}

fn spec(version: &str) -> ArchiveSpec {
    spec_of(version, &format!("lcu-{version}-linux-x64.tar.gz"), 0)
}

/// Serves `full`, or stops after `interrupt_after` bytes once, as a dropped connection does.
struct Fake {
    full: Vec<u8>,
    error: Option<chatgpt_app::Error>,
    interrupt_after: Mutex<Option<usize>>,
    expected_total: u64,
    calls: AtomicUsize,
    resumed_from: Mutex<Vec<u64>>,
}

impl Fake {
    fn serving(body: &[u8]) -> Self {
        Self {
            full: body.to_vec(),
            error: None,
            interrupt_after: Mutex::new(None),
            expected_total: 0,
            calls: AtomicUsize::new(0),
            resumed_from: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl Downloader for Fake {
    fn fetch(
        &self,
        url: &str,
        part: &Path,
        total: u64,
        progress: &mut dyn FnMut(u64),
    ) -> Result<(), chatgpt_app::Error> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert!(url.starts_with("https://"));
        assert_eq!(total, self.expected_total);
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        let have = fs::metadata(part).map_or(0, |meta| meta.len());
        self.resumed_from.lock().unwrap().push(have);
        if let Some(count) = self.interrupt_after.lock().unwrap().take() {
            fs::write(part, &self.full[..count]).unwrap();
            progress(count as u64);
            return Err(error(true));
        }
        fs::write(part, &self.full).unwrap();
        progress(self.full.len() as u64);
        Ok(())
    }
}

fn error(retryable: bool) -> chatgpt_app::Error {
    chatgpt_app::Error {
        message: "OpenAI-specific text".into(),
        retryable,
    }
}

fn part_of(root: &Path, spec: &ArchiveSpec) -> PathBuf {
    root.join(DOWNLOAD_DIR).join(spec.part_name())
}

fn no_progress(_: u64) {}

#[test]
fn the_bundled_lock_names_an_https_archive_for_each_architecture() {
    for arch in [DebArch::Arm64, DebArch::Amd64] {
        let spec = ArchiveSpec::parse_lcu(LCU_LOCK, arch).unwrap();
        assert!(spec.url.ends_with(&spec.archive));
        assert!(spec.archive.contains(&spec.version));
    }
    let plain = LCU_LOCK.replace("https://", "http://");
    assert!(ArchiveSpec::parse_lcu(&plain, DebArch::Amd64).is_err());
    let traversal = LCU_LOCK.replace("lcu-0.9.3-linux-x64.tar.gz", "..");
    assert!(ArchiveSpec::parse_lcu(&traversal, DebArch::Amd64).is_err());
}

#[test]
fn a_verified_archive_is_published_read_only_and_older_versions_are_removed() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("lcu");
    let old = spec("0.8.1");
    fs::create_dir_all(root.join(&old.version)).unwrap();
    fs::write(root.join(&old.version).join(&old.archive), b"old").unwrap();
    fs::create_dir_all(root.join(".publish-1-1")).unwrap();
    fs::create_dir_all(root.join(DOWNLOAD_DIR)).unwrap();
    fs::write(part_of(&root, &old), b"partial of an older version").unwrap();
    let current = spec("0.9.3");
    assert!(published(&root, &current).is_none());
    let downloader = Fake::serving(BODY);
    download_and_publish(&root, &current, &downloader, &no_progress)
        .map_err(|f| f.message)
        .unwrap();
    let folder = published(&root, &current).unwrap();
    assert_eq!(folder, root.join("0.9.3"));
    let file = folder.join(&current.archive);
    assert_eq!(fs::read(&file).unwrap(), BODY);
    assert_eq!(fs::metadata(&file).unwrap().permissions().mode() & 0o222, 0);
    assert_eq!(
        fs::metadata(&folder).unwrap().permissions().mode() & 0o222,
        0
    );
    assert!(!root.join("0.8.1").exists());
    assert!(!root.join(".publish-1-1").exists());
    assert!(!part_of(&root, &old).exists());
    assert!(!part_of(&root, &current).exists());
    // A later run replaces the published copy and ends read-only again.
    download_and_publish(&root, &current, &downloader, &no_progress)
        .map_err(|f| f.message)
        .unwrap();
    assert!(published(&root, &current).is_some());
    remove_tree(&root);
}

#[test]
fn an_archive_that_fails_its_checksum_is_discarded_and_not_published() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("lcu");
    let current = spec("0.9.3");
    let failure = download_and_publish(&root, &current, &Fake::serving(b"tampered"), &no_progress)
        .err()
        .unwrap();
    assert!(failure.retryable);
    assert!(failure.message.contains("checksum"));
    assert!(published(&root, &current).is_none());
    assert!(!root.join("0.9.3").exists());
    assert!(!part_of(&root, &current).exists());
}

#[test]
fn a_published_archive_that_changed_is_no_longer_ready() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("lcu");
    let current = spec("0.9.3");
    download_and_publish(&root, &current, &Fake::serving(BODY), &no_progress)
        .map_err(|f| f.message)
        .unwrap();
    let folder = root.join("0.9.3");
    fs::set_permissions(&folder, fs::Permissions::from_mode(0o755)).unwrap();
    let file = folder.join(&current.archive);
    fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
    fs::write(&file, b"another file of the same size").unwrap();
    assert!(published(&root, &current).is_none());
    remove_tree(&root);
}

#[test]
fn download_failures_say_what_to_do_without_naming_another_vendor() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("lcu");
    let current = spec("0.9.3");
    let mut downloader = Fake::serving(BODY);
    downloader.error = Some(error(true));
    let transient = download_and_publish(&root, &current, &downloader, &no_progress)
        .err()
        .unwrap();
    assert!(transient.retryable);
    assert!(transient.message.contains("retry") && !transient.message.contains("OpenAI"));
    downloader.error = Some(error(false));
    let removed = download_and_publish(&root, &current, &downloader, &no_progress)
        .err()
        .unwrap();
    assert!(!removed.retryable);
    assert!(removed.message.contains("Update Silo"));
}

#[test]
fn ensuring_the_archive_through_a_job_downloads_once_then_is_ready() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("lcu");
    let current = spec("0.9.3");
    let job = Job::new();
    let downloader = Fake::serving(BODY);
    for _ in 0..2 {
        job.ensure(
            &|| published(&root, &current).is_some(),
            &|_| download_and_publish(&root, &current, &downloader, &no_progress),
            &|| {},
            &|_| {},
        )
        .unwrap();
    }
    assert_eq!(downloader.calls(), 1);
    remove_tree(&root);
}

// ------------------------------------------------------------------ VM image

struct Importer(AtomicUsize);

impl Importer {
    fn new() -> Self {
        Self(AtomicUsize::new(0))
    }

    fn imports(&self) -> usize {
        self.0.load(Ordering::SeqCst)
    }
}

impl RuntimeRunner for Importer {
    fn run(
        &self,
        _: &RuntimePaths,
        args: &[String],
        _: Duration,
    ) -> Result<CommandOutput, RuntimeError> {
        assert_eq!(&args[..2], ["image", "load"]);
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(CommandOutput {
            stdout: String::new(),
            stderr: String::new(),
        })
    }
}

/// A gzip archive that unpacks to 12 bytes.
fn gzip_body() -> Vec<u8> {
    use std::io::Write;
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(b"test archive").unwrap();
    encoder.finish().unwrap()
}

struct Image {
    body: Vec<u8>,
    _dir: tempfile::TempDir,
    paths: RuntimePaths,
    pinned: PinnedImage,
}

impl Image {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let paths = RuntimePaths {
            guest_image: dir.path().join("guest-image"),
            executable: dir.path().join("msb"),
            home: dir.path().join("home"),
            storage_home: None,
            library: dir.path().join("lib"),
            metadata: dir.path().join("metadata"),
            volumes: dir.path().join("volumes"),
        };
        let body = gzip_body();
        let manifest = serde_json::from_value(serde_json::json!({
            "schemaVersion": 1, "version": "ubuntu-24.04-test",
            "architecture": std::env::consts::ARCH,
            "imageReference": "ghcr.io/amontlabs/silo-guest:test",
            "imageDigest": format!("sha256:{}", "a".repeat(64)),
            "archiveSha256": format!("{:x}", Sha256::digest(&body)),
            "archiveBytes": body.len(), "unpackedBytes": 12,
        }))
        .unwrap();
        Self {
            body,
            _dir: dir,
            paths,
            pinned: PinnedImage {
                manifest,
                url: "https://example.test/image-test.tar.gz".into(),
            },
        }
    }

    fn archive(&self) -> PathBuf {
        self.pinned.manifest.archive_path(&self.paths.guest_image)
    }

    fn downloader(&self, body: &[u8]) -> Fake {
        let mut downloader = Fake::serving(body);
        downloader.expected_total = self.body.len() as u64;
        downloader
    }

    fn ensure(
        &self,
        job: &Job,
        imported: bool,
        downloader: &Fake,
        importer: &Importer,
        reported: &Mutex<Vec<Option<u8>>>,
    ) -> Result<(), String> {
        ensure_image_with(
            job,
            &self.paths,
            &self.pinned,
            &|| imported,
            downloader,
            importer,
            &|fraction| reported.lock().unwrap().push(fraction),
        )
    }
}

/// The fake runner cannot fill the runtime's image cache, so a finished import ends in the
/// check that the cache holds the image.
const UNVERIFIED: &str = "did not pass verification";

#[test]
fn the_image_is_downloaded_verified_published_then_imported_with_progress() {
    let image = Image::new();
    let (job, importer, reported) = (Job::new(), Importer::new(), Mutex::new(Vec::new()));
    let downloader = image.downloader(&image.body);
    let error = image
        .ensure(&job, false, &downloader, &importer, &reported)
        .unwrap_err();
    assert!(error.contains(UNVERIFIED), "{error}");
    assert_eq!(downloader.calls(), 1);
    assert_eq!(importer.imports(), 1);
    assert_eq!(fs::read(image.archive()).unwrap(), image.body);
    assert_eq!(
        fs::metadata(image.archive()).unwrap().permissions().mode() & 0o222,
        0
    );
    let reported = reported.lock().unwrap();
    assert!(reported.contains(&Some(100)), "{reported:?}");
    assert_eq!(reported.last(), Some(&None), "{reported:?}");
    remove_tree(&image.paths.guest_image);
}

#[test]
fn a_checksum_mismatch_is_rejected_removed_and_retried() {
    let image = Image::new();
    let (job, importer, reported) = (Job::new(), Importer::new(), Mutex::new(Vec::new()));
    let tampered = vec![b'x'; image.body.len()];
    let bad = image.downloader(&tampered);
    let error = image
        .ensure(&job, false, &bad, &importer, &reported)
        .unwrap_err();
    assert!(error.contains("checksum"), "{error}");
    assert_eq!(job.task().state, TaskState::Failed);
    assert!(job.task().retryable);
    assert!(!image.archive().exists());
    assert!(!part_of(&image.paths.guest_image, &ArchiveSpec::image(&image.pinned)).exists());
    assert_eq!(importer.imports(), 0);
    let good = image.downloader(&image.body);
    let error = image
        .ensure(&job, false, &good, &importer, &reported)
        .unwrap_err();
    assert!(error.contains(UNVERIFIED), "{error}");
    assert_eq!(good.calls(), 1);
    assert_eq!(importer.imports(), 1);
    assert_eq!(fs::read(image.archive()).unwrap(), image.body);
    remove_tree(&image.paths.guest_image);
}

#[test]
fn a_download_of_the_wrong_size_is_rejected() {
    let image = Image::new();
    let (job, importer, reported) = (Job::new(), Importer::new(), Mutex::new(Vec::new()));
    let short = image.downloader(&image.body[..image.body.len() - 1]);
    let error = image
        .ensure(&job, false, &short, &importer, &reported)
        .unwrap_err();
    assert!(error.contains("checksum"), "{error}");
    assert!(!image.archive().exists());
    assert_eq!(importer.imports(), 0);
}

#[test]
fn an_interrupted_download_keeps_its_partial_file_and_resumes() {
    let image = Image::new();
    let (job, importer, reported) = (Job::new(), Importer::new(), Mutex::new(Vec::new()));
    let downloader = image.downloader(&image.body);
    *downloader.interrupt_after.lock().unwrap() = Some(5);
    let error = image
        .ensure(&job, false, &downloader, &importer, &reported)
        .unwrap_err();
    assert!(error.contains("could not download the VM image"), "{error}");
    assert!(job.task().retryable);
    let part = part_of(&image.paths.guest_image, &ArchiveSpec::image(&image.pinned));
    assert_eq!(fs::read(&part).unwrap(), &image.body[..5]);
    assert!(!image.archive().exists());
    let error = image
        .ensure(&job, false, &downloader, &importer, &reported)
        .unwrap_err();
    assert!(error.contains(UNVERIFIED), "{error}");
    assert_eq!(*downloader.resumed_from.lock().unwrap(), [0, 5]);
    assert_eq!(fs::read(image.archive()).unwrap(), image.body);
    assert!(!part.exists());
    remove_tree(&image.paths.guest_image);
}

#[test]
fn an_image_the_runtime_already_holds_downloads_and_imports_nothing() {
    let image = Image::new();
    let (job, importer, reported) = (Job::new(), Importer::new(), Mutex::new(Vec::new()));
    let downloader = image.downloader(&image.body);
    image
        .ensure(&job, true, &downloader, &importer, &reported)
        .unwrap();
    assert_eq!(downloader.calls(), 0);
    assert_eq!(importer.imports(), 0);
    assert_eq!(job.task(), PreparationTask::new(TaskState::Ready));
    assert!(!image.paths.guest_image.exists());
}

#[test]
fn a_published_archive_is_imported_without_another_download() {
    let image = Image::new();
    let (job, importer, reported) = (Job::new(), Importer::new(), Mutex::new(Vec::new()));
    let downloader = image.downloader(&image.body);
    let _ = image.ensure(&job, false, &downloader, &importer, &reported);
    assert_eq!(downloader.calls(), 1);
    // The runtime lost its cache (or moved to another storage): the archive is reused.
    let _ = image.ensure(&Job::new(), false, &downloader, &importer, &reported);
    assert_eq!(downloader.calls(), 1);
    assert_eq!(importer.imports(), 2);
    remove_tree(&image.paths.guest_image);
}

#[test]
fn a_published_archive_that_changed_is_downloaded_again() {
    let image = Image::new();
    let (importer, reported) = (Importer::new(), Mutex::new(Vec::new()));
    let downloader = image.downloader(&image.body);
    let _ = image.ensure(&Job::new(), false, &downloader, &importer, &reported);
    let folder = image.archive().parent().unwrap().to_path_buf();
    fs::set_permissions(&folder, fs::Permissions::from_mode(0o755)).unwrap();
    fs::set_permissions(image.archive(), fs::Permissions::from_mode(0o644)).unwrap();
    fs::write(image.archive(), vec![b'x'; image.body.len()]).unwrap();
    let _ = image.ensure(&Job::new(), false, &downloader, &importer, &reported);
    assert_eq!(downloader.calls(), 2);
    assert_eq!(fs::read(image.archive()).unwrap(), image.body);
    remove_tree(&image.paths.guest_image);
}

#[test]
fn a_download_that_is_gone_says_to_update_and_one_without_space_says_to_free_it() {
    let image = Image::new();
    let (job, importer, reported) = (Job::new(), Importer::new(), Mutex::new(Vec::new()));
    let mut gone = image.downloader(&image.body);
    gone.error = Some(error(false));
    let message = image
        .ensure(&job, false, &gone, &importer, &reported)
        .unwrap_err();
    assert_eq!(
        message,
        "The VM image is no longer available at its pinned location. Update Silo."
    );
    assert!(!job.task().retryable);

    let mut spec = ArchiveSpec::image(&image.pinned);
    spec.bytes = u64::MAX / 2;
    let failure = download_and_publish(&image.paths.guest_image, &spec, &gone, &no_progress)
        .err()
        .unwrap();
    assert!(
        failure.message.contains("Free at least"),
        "{}",
        failure.message
    );
    assert!(failure.retryable);
    remove_tree(&image.paths.guest_image);
}

#[test]
fn download_progress_is_a_percentage() {
    assert_eq!(percent(0, 200), 0);
    assert_eq!(percent(50, 200), 25);
    assert_eq!(percent(300, 200), 100);
    assert_eq!(percent(5, 0), 0);
}
