//! Synthetic packages only: no network and no process-wide Silo state, so these
//! run in parallel without the shared isolation guard.
use super::*;
use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};

enum Item<'a> {
    Dir(&'a str, u32),
    File(&'a str, u32, &'a [u8]),
    Link(&'a str, &'a str),
    Hard(&'a str, &'a str),
    Device(&'a str),
}

fn raw_header(name: &str, kind: tar::EntryType, mode: u32, size: u64) -> tar::Header {
    let mut header = tar::Header::new_gnu();
    // Bypass the builder's own path sanitising: hostile names must reach us.
    header.as_old_mut().name[..name.len()].copy_from_slice(name.as_bytes());
    header.set_entry_type(kind);
    header.set_mode(mode);
    header.set_size(size);
    header
}

fn data_tar(items: &[Item]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for item in items {
        match item {
            Item::Dir(name, mode) => {
                let mut h = raw_header(name, tar::EntryType::Directory, *mode, 0);
                h.set_cksum();
                builder.append(&h, std::io::empty()).unwrap();
            }
            Item::File(name, mode, bytes) => {
                let mut h = raw_header(name, tar::EntryType::Regular, *mode, bytes.len() as u64);
                h.set_cksum();
                builder.append(&h, *bytes).unwrap();
            }
            Item::Link(name, target) | Item::Hard(name, target) => {
                let kind = if matches!(item, Item::Link(..)) {
                    tar::EntryType::Symlink
                } else {
                    tar::EntryType::Link
                };
                let mut h = raw_header(name, kind, 0o777, 0);
                h.as_old_mut().linkname[..target.len()].copy_from_slice(target.as_bytes());
                h.set_cksum();
                builder.append(&h, std::io::empty()).unwrap();
            }
            Item::Device(name) => {
                let mut h = raw_header(name, tar::EntryType::Char, 0o644, 0);
                h.set_cksum();
                builder.append(&h, std::io::empty()).unwrap();
            }
        }
    }
    builder.into_inner().unwrap()
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}

fn ar_member(out: &mut Vec<u8>, name: &str, body: &[u8]) {
    out.extend_from_slice(
        format!(
            "{:<16}{:<12}{:<6}{:<6}{:<8}{:<10}`\n",
            name,
            0,
            0,
            0,
            "100644",
            body.len()
        )
        .as_bytes(),
    );
    out.extend_from_slice(body);
    if body.len() % 2 == 1 {
        out.push(b'\n');
    }
}

/// A real ar container with a gzip data member, readable by bsdtar and dpkg-deb.
fn deb(items: &[Item]) -> Vec<u8> {
    let mut out = b"!<arch>\n".to_vec();
    ar_member(&mut out, "debian-binary", b"2.0\n");
    let control = gzip(&data_tar(&[Item::File(
        "./control",
        0o644,
        b"Package: chatgpt\n",
    )]));
    ar_member(&mut out, "control.tar.gz", &control);
    ar_member(&mut out, "data.tar.gz", &gzip(&data_tar(items)));
    out
}

fn good_items() -> Vec<Item<'static>> {
    vec![
        Item::Dir("./usr/", 0o755),
        Item::Dir("./usr/lib/", 0o755),
        Item::Dir("./usr/lib/chatgpt/", 0o755),
        Item::File("./usr/lib/chatgpt/ChatGPT", 0o755, b"#!/bin/sh\n"),
        Item::Dir("./usr/lib/chatgpt/resources/", 0o700),
        Item::Dir("./usr/lib/chatgpt/resources/cua_node/", 0o755),
        Item::Dir("./usr/lib/chatgpt/resources/cua_node/bin/", 0o755),
        Item::File(
            "./usr/lib/chatgpt/resources/cua_node/bin/node",
            0o755,
            b"node",
        ),
        Item::File(
            "./usr/lib/chatgpt/resources/cua_node/bin/node_repl",
            0o755,
            b"repl",
        ),
        Item::File("./usr/lib/chatgpt/resources/app.asar", 0o644, b"data"),
        Item::Link("./usr/lib/chatgpt/resources/current", "app.asar"),
        Item::Link("./usr/lib/chatgpt/res", "resources"),
        Item::Link("./usr/lib/chatgpt/resources/back", "../ChatGPT"),
        Item::File("./usr/bin/outside", 0o755, b"never extracted"),
        Item::File("./etc/other", 0o644, b"never extracted"),
    ]
}

struct Fake {
    bytes: Vec<u8>,
    calls: AtomicUsize,
    /// Pretend an earlier attempt left this many bytes in the part file.
    delay: Duration,
}

impl Fake {
    fn new(bytes: Vec<u8>) -> Self {
        Self {
            bytes,
            calls: AtomicUsize::new(0),
            delay: Duration::ZERO,
        }
    }
}

impl Downloader for Fake {
    fn fetch(
        &self,
        _url: &str,
        part: &Path,
        total: u64,
        progress: &mut dyn FnMut(u64),
    ) -> Result<(), Error> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        std::thread::sleep(self.delay);
        fs::write(part, &self.bytes).unwrap();
        progress(total);
        Ok(())
    }
}

fn lock_for(bytes: &[u8]) -> Lock {
    let asset = Asset {
        url: format!("https://{DOWNLOAD_HOST}/chatgpt.deb"),
        sha256: format!("{:x}", Sha256::digest(bytes)),
        bytes: bytes.len() as u64,
    };
    Lock {
        schema_version: 1,
        package: "chatgpt".into(),
        version: "1.2.3".into(),
        cua_runtime_version: "0.0.1/x".into(),
        lcu_version: None,
        architectures: HashMap::from([
            ("arm64".to_owned(), asset.clone()),
            ("amd64".to_owned(), asset),
        ]),
    }
}

fn root() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    Dir::open_root(&dir.path().join("chatgpt"), true).unwrap();
    dir
}

fn run(
    dir: &tempfile::TempDir,
    package: &[u8],
    lock: &Lock,
) -> (Result<PathBuf, Error>, Vec<Status>) {
    let statuses = Mutex::new(Vec::new());
    let fake = Fake::new(package.to_vec());
    let result = ensure(
        &dir.path().join("chatgpt"),
        lock,
        DebArch::Arm64,
        &fake,
        &|s| statuses.lock().unwrap().push(s),
    );
    (result, statuses.into_inner().unwrap())
}

fn assert_nothing_published(dir: &tempfile::TempDir) {
    let root = dir.path().join("chatgpt");
    assert!(!root.join("published/1.2.3-arm64").exists());
    let leftovers: Vec<_> = fs::read_dir(&root)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(".staging-"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn bundled_lock_is_valid_and_pins_both_architectures() {
    let lock = Lock::bundled().unwrap();
    assert_eq!(lock.version, "26.928.31416");
    assert_eq!(
        lock.cua_runtime_version,
        "0.0.27/20260927214556-b77d38801cca"
    );
    assert_eq!(lock.lcu_version.as_deref(), Some("0.9.3"));
    let arm = lock.asset(DebArch::Arm64).unwrap();
    assert_eq!(arm.bytes, 453121290);
    assert!(arm
        .url
        .ends_with("pool/main/c/chatgpt/chatgpt_26.928.31416_arm64.deb"));
    assert_eq!(lock.asset(DebArch::Amd64).unwrap().bytes, 474894546);
    assert_eq!(lock.directory_name(DebArch::Arm64), "26.928.31416-arm64");
}

#[test]
fn lock_rejects_plain_http_and_foreign_hosts() {
    for url in [
        "http://persistent.oaistatic.com/a.deb",
        "https://example.com/a.deb",
    ] {
        let mut lock = Lock::bundled().unwrap();
        lock.architectures.get_mut("arm64").unwrap().url = url.into();
        assert!(lock.validate().is_err(), "{url}");
    }
    let mut lock = Lock::bundled().unwrap();
    lock.architectures.get_mut("amd64").unwrap().sha256 = "ABC".into();
    assert!(lock.validate().is_err());
}

#[test]
fn a_fresh_device_downloads_without_any_prior_step() {
    // No notice, no stored choice: the first call on an empty storage directory
    // downloads, verifies and publishes.
    let dir = tempfile::tempdir().unwrap();
    let package = deb(&good_items());
    let fake = Fake::new(package.clone());
    let root = dir.path().join("chatgpt");
    assert_eq!(
        current_status(&root, &lock_for(&package), DebArch::Arm64),
        Status::Idle
    );
    let path = ensure(&root, &lock_for(&package), DebArch::Arm64, &fake, &|_| {}).unwrap();
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
    assert!(matches!(
        current_status(&root, &lock_for(&package), DebArch::Arm64),
        Status::Ready { path: ready, .. } if ready == path
    ));
    assert!(!root.join("consent.json").exists());
}

#[test]
fn valid_package_is_published_with_modes_links_and_a_canonical_path() {
    let dir = root();
    let package = deb(&good_items());
    let (result, statuses) = run(&dir, &package, &lock_for(&package));
    let path = result.unwrap();
    assert_eq!(
        path,
        fs::canonicalize(dir.path().join("chatgpt/published/1.2.3-arm64")).unwrap()
    );
    assert_eq!(
        fs::metadata(path.join("ChatGPT"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
    assert_eq!(
        fs::metadata(path.join("resources/app.asar"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o644
    );
    assert_eq!(
        fs::metadata(path.join("resources"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
    assert_eq!(
        fs::read_link(path.join("resources/current")).unwrap(),
        Path::new("app.asar")
    );
    assert_eq!(fs::read(path.join("res/app.asar")).unwrap(), b"data");
    assert_eq!(
        fs::read_link(path.join("resources/back")).unwrap(),
        Path::new("../ChatGPT")
    );
    assert!(!path.join("usr").exists() && !path.join("etc").exists());
    assert!(!dir
        .path()
        .join("chatgpt/downloads/chatgpt_1.2.3_arm64.deb")
        .exists());
    assert_nothing_published_staging_clean(&dir);
    assert!(matches!(statuses.first(), Some(Status::Downloading { .. })));
    let tail: Vec<_> = statuses.iter().rev().take(3).rev().collect();
    assert_eq!(tail[0], &Status::Verifying);
    assert_eq!(tail[1], &Status::Extracting);
    assert!(matches!(tail[2], Status::Ready { version, .. } if version == "1.2.3"));
}

fn assert_nothing_published_staging_clean(dir: &tempfile::TempDir) {
    for entry in fs::read_dir(dir.path().join("chatgpt")).unwrap().flatten() {
        assert!(!entry.file_name().to_string_lossy().starts_with(".staging-"));
    }
}

#[test]
fn hash_mismatch_is_refused_and_discarded() {
    let dir = root();
    let package = deb(&good_items());
    let mut lock = lock_for(&package);
    lock.architectures.get_mut("arm64").unwrap().sha256 = "0".repeat(64);
    let (result, statuses) = run(&dir, &package, &lock);
    let error = result.unwrap_err();
    assert!(
        !error.retryable && error.message.contains("checksum"),
        "{error}"
    );
    assert!(matches!(
        statuses.last(),
        Some(Status::Failed {
            retryable: false,
            ..
        })
    ));
    assert!(!dir
        .path()
        .join("chatgpt/downloads/chatgpt_1.2.3_arm64.deb")
        .exists());
    assert_nothing_published(&dir);
}

#[test]
fn size_mismatch_is_refused_and_discarded() {
    let dir = root();
    let package = deb(&good_items());
    let mut lock = lock_for(&package);
    lock.architectures.get_mut("arm64").unwrap().bytes += 1;
    let (result, _) = run(&dir, &package, &lock);
    // The fake writes the real (shorter) bytes, so verification sees the size.
    let error = result.unwrap_err();
    assert!(error.retryable && error.message.contains("size"), "{error}");
    assert!(!dir
        .path()
        .join("chatgpt/downloads/chatgpt_1.2.3_arm64.deb")
        .exists());
    assert_nothing_published(&dir);
}

#[test]
fn malicious_entries_are_refused_and_publish_nothing() {
    let cases: Vec<(&str, Vec<Item>)> = vec![
        (
            "absolute",
            vec![Item::File("/usr/lib/chatgpt/ChatGPT", 0o755, b"x")],
        ),
        (
            "absolute elsewhere",
            vec![Item::File("/etc/passwd", 0o644, b"x")],
        ),
        (
            "dotdot",
            vec![Item::File("./usr/lib/chatgpt/../../../x", 0o644, b"x")],
        ),
        ("dotdot outside", vec![Item::File("../x", 0o644, b"x")]),
        (
            "symlink up out",
            vec![Item::Link("./usr/lib/chatgpt/l", "../../../..")],
        ),
        (
            "symlink at root up",
            vec![Item::Link("./usr/lib/chatgpt/l", "..")],
        ),
        (
            "symlink absolute",
            vec![Item::Link("./usr/lib/chatgpt/l", "/etc/passwd")],
        ),
        (
            "symlink via link",
            vec![
                Item::Link("./usr/lib/chatgpt/d", "."),
                Item::Link("./usr/lib/chatgpt/e", "d/../.."),
            ],
        ),
        (
            "write through symlink",
            vec![
                Item::File("./usr/lib/chatgpt/ChatGPT", 0o755, b"x"),
                Item::Link("./usr/lib/chatgpt/d", "."),
                Item::File("./usr/lib/chatgpt/d/evil", 0o644, b"x"),
            ],
        ),
        (
            "setuid",
            vec![Item::File("./usr/lib/chatgpt/ChatGPT", 0o4755, b"x")],
        ),
        ("setgid dir", vec![Item::Dir("./usr/lib/chatgpt/d", 0o2755)]),
        ("device", vec![Item::Device("./usr/lib/chatgpt/null")]),
        (
            "hard link",
            vec![
                Item::File("./usr/lib/chatgpt/ChatGPT", 0o755, b"x"),
                Item::Hard("./usr/lib/chatgpt/h", "./usr/lib/chatgpt/ChatGPT"),
            ],
        ),
        (
            "case collision",
            vec![
                Item::File("./usr/lib/chatgpt/ChatGPT", 0o755, b"x"),
                Item::File("./usr/lib/chatgpt/Readme", 0o644, b"a"),
                Item::File("./usr/lib/chatgpt/README", 0o644, b"b"),
            ],
        ),
        (
            "case collision in directory",
            vec![
                Item::File("./usr/lib/chatgpt/ChatGPT", 0o755, b"x"),
                Item::File("./usr/lib/chatgpt/Dir/a", 0o644, b"a"),
                Item::File("./usr/lib/chatgpt/dir/b", 0o644, b"b"),
            ],
        ),
        (
            "duplicate file",
            vec![
                Item::File("./usr/lib/chatgpt/ChatGPT", 0o755, b"x"),
                Item::File("./usr/lib/chatgpt/ChatGPT", 0o755, b"y"),
            ],
        ),
        (
            "no executable",
            vec![Item::File("./usr/lib/chatgpt/other", 0o644, b"x")],
        ),
    ];
    for (label, items) in cases {
        // The extractor itself, on the raw tar: bsdtar rewrites some hostile
        // names (for example it strips a leading `/`), dpkg-deb does not.
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("tree");
        fs::create_dir(&dest).unwrap();
        // The production check refuses a group- or world-writable root, and `create_dir`
        // follows the process umask (002 by default on Ubuntu). State the mode explicitly.
        fs::set_permissions(&dest, fs::Permissions::from_mode(0o700)).unwrap();
        let dest_dir = Dir::open_root(&dest, false).unwrap();
        let error = extract_tree(&data_tar(&items)[..], &dest_dir).expect_err(label);
        assert!(!error.retryable, "{label}: {error}");
        // Nothing was written outside the destination.
        let outside: Vec<_> = fs::read_dir(dir.path()).unwrap().flatten().collect();
        assert_eq!(outside.len(), 1, "{label}");
    }
}

#[test]
fn a_hostile_package_publishes_nothing() {
    for items in [
        vec![Item::File("./usr/lib/chatgpt/ChatGPT", 0o4755, b"x")],
        vec![Item::Device("./usr/lib/chatgpt/null")],
        vec![
            Item::File("./usr/lib/chatgpt/ChatGPT", 0o755, b"x"),
            Item::Link("./usr/lib/chatgpt/l", "../../.."),
        ],
    ] {
        let dir = root();
        let package = deb(&items);
        let (result, statuses) = run(&dir, &package, &lock_for(&package));
        assert!(!result.unwrap_err().retryable);
        assert!(matches!(
            statuses.last(),
            Some(Status::Failed {
                retryable: false,
                ..
            })
        ));
        assert_nothing_published(&dir);
    }
}

#[test]
fn damaged_published_folder_is_replaced_and_good_one_is_never_touched() {
    let dir = root();
    let package = deb(&good_items());
    let lock = lock_for(&package);
    let target = dir.path().join("chatgpt/published/1.2.3-arm64");
    fs::create_dir_all(target.join("partial")).unwrap();
    let (result, _) = run(&dir, &package, &lock);
    let path = result.unwrap();
    assert!(path.join("ChatGPT").is_file() && !path.join("partial").exists());
    // A verified folder is returned as is, with no download.
    let fake = Fake::new(Vec::new());
    let again = ensure(
        &dir.path().join("chatgpt"),
        &lock,
        DebArch::Arm64,
        &fake,
        &|_| {},
    )
    .unwrap();
    assert_eq!(again, path);
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn concurrent_calls_download_and_extract_once() {
    let dir = root();
    let package = deb(&good_items());
    let lock = lock_for(&package);
    let mut fake = Fake::new(package);
    fake.delay = Duration::from_millis(300);
    let root = dir.path().join("chatgpt");
    let paths: Vec<PathBuf> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..4)
            .map(|_| scope.spawn(|| ensure(&root, &lock, DebArch::Arm64, &fake, &|_| {}).unwrap()))
            .collect();
        workers.into_iter().map(|w| w.join().unwrap()).collect()
    });
    assert!(paths.windows(2).all(|pair| pair[0] == pair[1]));
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn an_existing_complete_download_is_reused_after_an_interruption() {
    let dir = root();
    let package = deb(&good_items());
    let lock = lock_for(&package);
    let downloads = dir.path().join("chatgpt/downloads");
    fs::create_dir_all(&downloads).unwrap();
    fs::write(downloads.join("chatgpt_1.2.3_arm64.deb"), &package).unwrap();
    // A stale staging folder from a crash is removed.
    fs::create_dir_all(dir.path().join("chatgpt/.staging-old/x")).unwrap();
    let fake = Fake::new(Vec::new());
    ensure(
        &dir.path().join("chatgpt"),
        &lock,
        DebArch::Arm64,
        &fake,
        &|_| {},
    )
    .unwrap();
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
    assert_nothing_published_staging_clean(&dir);
}

#[test]
fn garbage_collection_keeps_pinned_and_in_use_versions() {
    let dir = root();
    let package = deb(&good_items());
    let lock = lock_for(&package);
    let (result, _) = run(&dir, &package, &lock);
    result.unwrap();
    let root = dir.path().join("chatgpt");
    let published = root.join("published");
    for name in ["1.0.0-arm64", "1.1.0-arm64", "1.1.0-amd64"] {
        fs::create_dir_all(published.join(name).join("sub")).unwrap();
    }
    fs::create_dir_all(published.join("unrelated")).unwrap();
    fs::create_dir_all(root.join(".staging-x")).unwrap();
    let in_use = HashSet::from(["1.1.0-arm64".to_owned()]);
    let removed = collect_garbage(&root, &lock, DebArch::Arm64, &in_use).unwrap();
    assert_eq!(removed, ["1.0.0-arm64", "1.1.0-amd64"]);
    assert!(
        published.join("1.1.0-arm64").exists() && published.join("1.2.3-arm64/ChatGPT").exists()
    );
    assert!(published.join("unrelated").exists() && !root.join(".staging-x").exists());
    assert!(
        collect_garbage(&dir.path().join("missing"), &lock, DebArch::Arm64, &in_use)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn path_and_link_rules() {
    assert_eq!(
        tree_components(b"./usr/lib/chatgpt/a/b").unwrap(),
        Some(vec!["a".into(), "b".into()])
    );
    assert_eq!(tree_components(b"usr/lib/chatgpt/").unwrap(), Some(vec![]));
    assert_eq!(tree_components(b"./usr/share/doc").unwrap(), None);
    assert!(tree_components(b"/usr/lib/chatgpt/a").is_err());
    assert!(tree_components(b"./usr/lib/chatgpt/a/../b").is_err());
    assert!(tree_components(b"./usr/../etc").is_err());
    assert!(check_link_target(b"../x", 1, "p").is_ok());
    assert!(check_link_target(b"../x", 0, "p").is_err());
    assert!(check_link_target(b"a/../x", 3, "p").is_err());
    assert!(check_link_target(b"/a", 3, "p").is_err());
    assert!(check_link_target(b"", 3, "p").is_err());
}

#[test]
fn commands_address_a_device_not_a_computer() {
    // No device (or an empty one) means this device.
    assert_eq!(remote_device(None), Ok(None));
    assert_eq!(remote_device(Some("")), Ok(None));
    // A remote device is addressed by its device id.
    let device = "00000000-0000-4000-8000-0000000000aa";
    assert_eq!(remote_device(Some(device)), Ok(Some(device.to_owned())));
    // Neither a computer target (the old placeholder routing) nor garbage falls back to this device.
    assert!(remote_device(Some(&format!("silo-remote:{device}:{device}"))).is_err());
    assert!(remote_device(Some("dev")).is_err());
}

#[test]
fn an_owner_without_computer_use_reports_unknown_not_an_error() {
    use crate::bridge_error::BridgeError;
    let older = Err(owner_error(BridgeError::unsupported()));
    assert_eq!(
        remote_status(older),
        Ok(serde_json::json!({"state": "unknown"}))
    );
    // Other failures are not hidden, and a real status passes through untouched.
    assert_eq!(
        remote_status(Err("Device disconnected.".into())),
        Err("Device disconnected.".into())
    );
    let ready = serde_json::json!({"state": "ready", "path": "/p", "version": "1"});
    assert_eq!(remote_status(Ok(ready.clone())), Ok(ready));
}

#[test]
fn an_older_silo_on_the_owning_device_gets_a_clear_message() {
    use crate::bridge_error::{BridgeError, ErrorCode};
    assert_eq!(
        owner_error(BridgeError::unsupported()),
        "Update Silo on that device to use computer use."
    );
    assert_eq!(
        owner_error(BridgeError::new(ErrorCode::Internal, "Disk is full.")),
        "Disk is full."
    );
}

#[test]
fn the_cached_status_is_what_cheap_reads_see() {
    set_test_cache(None);
    set_test_cache(Some(Status::Verifying));
    assert_eq!(cached_status(), Some(Status::Verifying));
    assert!(in_progress(&Status::Extracting));
    assert!(!in_progress(&Status::Idle));
    set_test_cache(None);
}

#[test]
fn status_serializes_for_the_ui() {
    let value = serde_json::to_value(Status::Downloading {
        received_bytes: 5,
        total_bytes: 10,
    })
    .unwrap();
    assert_eq!(
        value,
        serde_json::json!({"state": "downloading", "receivedBytes": 5, "totalBytes": 10})
    );
    let value = serde_json::to_value(Status::Failed {
        reason: "x".into(),
        retryable: true,
    })
    .unwrap();
    assert_eq!(
        value,
        serde_json::json!({"state": "failed", "reason": "x", "retryable": true})
    );
    // The consent state is gone: no status serializes as it.
    for status in [Status::Idle, Status::Verifying, Status::Extracting] {
        assert_ne!(
            serde_json::to_value(status).unwrap()["state"],
            "notConsented"
        );
    }
}

mod hardening;
mod http;

/// Opt-in: downloads the real pinned arm64 package from OpenAI (453 MB) into a
/// temporary directory and checks the extracted layout. Run with
/// `SILO_LIVE_TEST_CONFIRM=disposable-test-fixtures cargo test --locked
/// chatgpt_app::tests::live -- --ignored --nocapture`. Needs about 3 GB free.
#[test]
#[ignore = "downloads 453 MB from OpenAI and extracts about 1.5 GB"]
fn live_download_of_the_pinned_arm64_package() {
    let arch = DebArch::host().unwrap();
    crate::test_support::live::require_confirmation();
    let dir = tempfile::tempdir().unwrap();
    // `SILO_LIVE_CHATGPT_ROOT` keeps the published app for a manual computer check.
    let root = std::env::var_os("SILO_LIVE_CHATGPT_ROOT")
        .map_or_else(|| dir.path().join("chatgpt"), PathBuf::from);
    let lock = Lock::bundled().unwrap();
    let started = Instant::now();
    let last = Mutex::new(None);
    let path = ensure(&root, &lock, arch, &HttpDownloader::default(), &|s| {
        *last.lock().unwrap() = Some(s);
    })
    .unwrap();
    println!("ready in {:?} at {}", started.elapsed(), path.display());
    assert_eq!(
        path,
        fs::canonicalize(root.join("published").join(lock.directory_name(arch))).unwrap()
    );
    let executable = path.join("ChatGPT");
    assert!(fs::metadata(&executable).unwrap().permissions().mode() & 0o111 != 0);
    assert!(path.join("resources").is_dir());
    assert!(path.join("resources/cua_node/manifest.json").is_file());
    assert!(!root
        .join(format!(
            "downloads/chatgpt_26.928.31416_{}.deb",
            arch.name()
        ))
        .exists());
    assert!(matches!(
        last.into_inner().unwrap(),
        Some(Status::Ready { .. })
    ));
    // Idempotent: a second call neither downloads nor changes anything.
    let again = ensure(&root, &lock, arch, &Fake::new(Vec::new()), &|_| {}).unwrap();
    assert_eq!(again, path);
    // Reuse costs: cheap check in this session, then the full digest as a new
    // process would run it for the first time.
    let lock = Lock::bundled().unwrap();
    let started = Instant::now();
    assert!(verify_published(&root, &lock, arch).is_some());
    println!("cheap reuse check: {:?}", started.elapsed());
    hardening::forget_session(&root);
    let started = Instant::now();
    assert!(verify_published(&root, &lock, arch).is_some());
    let full = digest_tree(&path, true).unwrap();
    println!(
        "full digest verification: {:?} ({} entries, {} bytes)",
        started.elapsed(),
        full.entries,
        full.bytes
    );
}

#[test]
fn status_events_identify_their_device() {
    let local = event_payload(serde_json::to_value(Status::Verifying).unwrap(), None);
    assert_eq!(
        local,
        serde_json::json!({ "state": "verifying", "device": null })
    );
    let remote = event_payload(
        serde_json::json!({ "state": "downloading", "receivedBytes": 1, "totalBytes": 2 }),
        Some("host-1"),
    );
    assert_eq!(remote["device"], "host-1");
    assert_eq!(remote["state"], "downloading");
    assert_eq!(remote["receivedBytes"], 1);
}

#[test]
fn collection_holds_the_device_gate_so_a_start_cannot_slip_between_check_and_delete() {
    use crate::runtime::operation_gate::OperationGate;
    use std::sync::mpsc;
    let dir = root();
    let lock = lock_for(&deb(&good_items()));
    let root_dir = dir.path().join("chatgpt");
    let old = root_dir.join("published").join("0.9.0-arm64");
    fs::create_dir_all(old.join("sub")).unwrap();
    let gate = OperationGate::new();
    let computer = "00000000-0000-4000-8000-000000000001";

    // A start already in flight: collection is skipped and nothing is removed.
    {
        let _starting = gate.computer(computer, "dev", "Starting dev").unwrap();
        assert!(!collect_unused_gated(
            &gate,
            &root_dir,
            &lock,
            DebArch::Arm64,
            || panic!("the inventory must not run while a start holds the gate")
        ));
        assert!(old.exists());
    }

    // While the inventory is being read, no start is admitted; after the collection
    // finished, a start is admitted and sees the final folder state.
    let (inspecting, inspecting_seen) = mpsc::channel();
    let (proceed, proceed_seen) = mpsc::channel::<()>();
    std::thread::scope(|scope| {
        let (gate, root_dir, lock) = (&gate, &root_dir, &lock);
        let collector = scope.spawn(move || {
            collect_unused_gated(gate, root_dir, lock, DebArch::Arm64, || {
                inspecting.send(()).unwrap();
                proceed_seen.recv().unwrap();
                true
            })
        });
        inspecting_seen.recv().unwrap();
        assert!(
            gate.try_computer(computer, "dev", "Starting dev").is_err(),
            "a start was admitted between the inventory and the deletion"
        );
        assert!(old.exists());
        proceed.send(()).unwrap();
        assert!(collector.join().unwrap());
    });
    assert!(!old.exists());
    let _starting = gate.computer(computer, "dev", "Starting dev").unwrap();
    assert!(root_dir.join("published").exists());
}

#[test]
fn collection_never_waits_for_a_download_holding_the_storage_lock() {
    use crate::runtime::operation_gate::OperationGate;
    use std::sync::mpsc;
    let dir = root();
    let lock = lock_for(&deb(&good_items()));
    let root_dir = dir.path().join("chatgpt");
    let old = root_dir.join("published").join("0.9.0-arm64");
    fs::create_dir_all(old.join("sub")).unwrap();
    let gate = OperationGate::new();
    let computer = "00000000-0000-4000-8000-000000000001";

    // A download or extraction holds the storage lock.
    let download = RootLock::take(&root_dir).unwrap();
    let (done, finished) = mpsc::channel();
    std::thread::scope(|scope| {
        let (gate, root_dir, lock) = (&gate, &root_dir, &lock);
        scope.spawn(move || {
            done.send(collect_unused_gated(
                gate,
                root_dir,
                lock,
                DebArch::Arm64,
                || true,
            ))
            .unwrap();
        });
        // A blocking implementation would hold the device gate until the download ended.
        let ran = finished
            .recv_timeout(Duration::from_secs(10))
            .expect("collection returned while the storage lock was busy");
        assert!(!ran, "a skipped collection reports that it did not run");
    });
    assert!(old.exists(), "nothing was removed");
    // The device gate was released: lifecycle operations and Quit are not stuck behind it.
    drop(
        gate.computer(computer, "dev", "Starting dev")
            .expect("the gate is free"),
    );
    assert!(gate.is_idle());

    // Once the download ended, the next pass collects.
    drop(download);
    assert!(collect_unused_gated(
        &gate,
        &root_dir,
        &lock,
        DebArch::Arm64,
        || true
    ));
    assert!(!old.exists());
}

#[test]
fn a_busy_storage_lock_is_reported_not_waited_for() {
    let dir = root();
    let root_dir = dir.path().join("chatgpt");
    let held = RootLock::take(&root_dir).unwrap();
    assert!(RootLock::try_take(&root_dir).unwrap().is_none());
    drop(held);
    assert!(RootLock::try_take(&root_dir).unwrap().is_some());
}

#[test]
fn skipped_maintenance_stays_pending_and_is_retried_until_it_ran() {
    let maintenance = Maintenance::new();
    // A pass that ran leaves nothing pending and starts no retry loop.
    assert!(!maintenance.note(true));
    // A skipped pass starts exactly one retry loop, however often it is skipped.
    assert!(maintenance.note(false));
    assert!(!maintenance.note(false));
    assert!(maintenance.pending.load(Ordering::SeqCst));
    // The loop keeps trying (waiting between attempts) until an attempt runs.
    let attempts = AtomicUsize::new(0);
    let waits = AtomicUsize::new(0);
    maintenance.run_retries(
        || attempts.fetch_add(1, Ordering::SeqCst) + 1 == 3,
        || {
            waits.fetch_add(1, Ordering::SeqCst);
        },
    );
    assert_eq!(attempts.load(Ordering::SeqCst), 3);
    assert_eq!(waits.load(Ordering::SeqCst), 3);
    assert!(!maintenance.pending.load(Ordering::SeqCst));
    // The loop ended, so a later skip starts a new one.
    assert!(maintenance.note(false));
    // A regular pass that ran meanwhile cancels the pending retry without an attempt.
    assert!(!maintenance.note(true));
    maintenance.run_retries(
        || panic!("nothing is pending"),
        || panic!("nothing is pending"),
    );
    assert!(
        maintenance.note(false),
        "the finished loop released its claim"
    );
}

#[test]
fn only_a_confirmed_removal_stops_the_automatic_retries() {
    for code in [404, 410] {
        let error = refusal(code);
        assert!(!error.retryable, "{code}");
        assert!(error.message.contains("no longer serves"), "{code}");
    }
    // A refusal by a proxy, a firewall or a regional filter may not repeat on another
    // network, and any other answer is the server's trouble: all are retried.
    for code in [401, 403, 400, 408, 429, 451, 500, 502, 503] {
        assert!(refusal(code).retryable, "{code}");
    }
    let denied = refusal(403).message;
    assert!(
        denied.contains("HTTP 403") && denied.contains("network"),
        "{denied}"
    );
    assert!(!denied.contains("no longer serves"));
}

#[test]
fn an_access_denial_keeps_the_worker_retrying_until_another_network_works() {
    let denials = Cell::new(0);
    let waits = Cell::new(0);
    let status = auto::settle(
        || {
            denials.set(denials.get() + 1);
            if denials.get() < 4 {
                refusal(403).status()
            } else {
                Status::Ready {
                    path: "/chatgpt/1.2.3-arm64".into(),
                    version: "1.2.3".into(),
                }
            }
        },
        |_| {
            waits.set(waits.get() + 1);
            false
        },
        || {},
    );
    assert!(matches!(status, Status::Ready { .. }));
    assert_eq!((denials.get(), waits.get()), (4, 3));
    // A removed package ends the worker at once.
    let ended = auto::settle(|| refusal(404).status(), |_| panic!("no retry"), || {});
    assert!(matches!(
        ended,
        Status::Failed {
            retryable: false,
            ..
        }
    ));
}

#[test]
fn consent_era_owners_report_unknown_not_a_status_that_never_progresses() {
    let consent_era: Vec<String> = [
        "handshake",
        "chatgpt.status",
        "chatgpt.accept",
        "chatgpt.prepare",
    ]
    .map(String::from)
    .into();
    let current: Vec<String> = ["handshake", "chatgpt.status", "chatgpt.retry"]
        .map(String::from)
        .into();
    assert!(!owner_is_current(&consent_era));
    assert!(owner_is_current(&current));
    // The old owner answers `idle` (consent accepted, no app): never passed through.
    let idle = || Ok(serde_json::json!({"state": "idle"}));
    let unreached = || -> Result<serde_json::Value, String> { panic!("not asked") };
    assert_eq!(
        owner_status(Ok(consent_era), unreached).unwrap(),
        serde_json::json!({"state": "unknown"})
    );
    // A current owner's status passes through, including the automatic `idle`.
    assert_eq!(
        owner_status(Ok(current.clone()), idle).unwrap(),
        serde_json::json!({"state": "idle"})
    );
    // A current owner without the status method is still `unknown`; real failures stay errors.
    assert_eq!(
        owner_status(Ok(current), || Err(UPDATE_OWNER.to_owned())).unwrap(),
        serde_json::json!({"state": "unknown"})
    );
    assert_eq!(
        owner_status(Err("This device is offline.".into()), unreached).unwrap_err(),
        "This device is offline."
    );
}

#[test]
fn tar_pipeline_reaps_the_producer_when_the_consumer_cannot_start() {
    let root = tempfile::tempdir().unwrap();
    let first = Command::new("/bin/sleep")
        .arg("30")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let pid = first.id() as libc::pid_t;
    let result = TarStream::pipe(first, &mut Command::new(root.path().join("missing")));
    let mut status = 0;
    // This PID belongs to our child. ECHILD proves the pipeline collected its exit.
    let waited = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
    let error = std::io::Error::last_os_error();
    if waited == 0 {
        // Clean up the fixture even when the regression fails.
        unsafe {
            libc::kill(pid, libc::SIGKILL);
            libc::waitpid(pid, &mut status, 0);
        }
    }
    assert!(result.is_err());
    assert_eq!(waited, -1, "pipeline left its producer unreaped");
    assert_eq!(error.raw_os_error(), Some(libc::ECHILD));
}

#[test]
fn a_download_of_unknown_size_ends_with_the_stream() {
    use std::io::{Read as _, Write as _};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0u8; 2048];
        let _ = stream.read(&mut request).unwrap();
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\nConnection: close\r\n\r\nlcu bytes",
            )
            .unwrap();
    });
    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join("archive.part");
    let downloader = HttpDownloader {
        attempts: 1,
        backoff: Duration::ZERO,
        plain_http: true,
    };
    downloader
        .fetch(&format!("http://{address}/archive"), &part, 0, &mut |_| {})
        .unwrap();
    server.join().unwrap();
    assert_eq!(fs::read(&part).unwrap(), b"lcu bytes");
}
