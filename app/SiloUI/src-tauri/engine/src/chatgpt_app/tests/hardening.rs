//! Hostile-filesystem and tamper tests: planted links, forged folders, damaged
//! or interrupted publications, size and metadata limits.
use super::*;

fn chatgpt_root(dir: &tempfile::TempDir) -> PathBuf {
    dir.path().join("chatgpt")
}

/// Publishes the good package and returns (dir, package, lock, published path).
fn published() -> (tempfile::TempDir, Vec<u8>, Lock, PathBuf) {
    let dir = root();
    let package = deb(&good_items());
    let lock = lock_for(&package);
    let path = run(&dir, &package, &lock).0.unwrap();
    (dir, package, lock, path)
}

/// Runs `ensure` with a fresh fake and returns (result, downloads made).
fn again(dir: &tempfile::TempDir, package: &[u8], lock: &Lock) -> (Result<PathBuf, Error>, usize) {
    let fake = Fake::new(package.to_vec());
    let result = ensure(&chatgpt_root(dir), lock, DebArch::Arm64, &fake, &|_| {});
    (result, fake.calls.load(Ordering::SeqCst))
}

/// Forget only this root's fully verified trees, as a new process would.
/// Clearing the whole process-wide cache races with tests running in parallel.
pub(super) fn forget_session(root: &Path) {
    if let Some(map) = VERIFIED.lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
        map.retain(|path, _| !path.starts_with(root));
    }
}

fn verified(dir: &tempfile::TempDir, lock: &Lock) -> bool {
    verify_published(&chatgpt_root(dir), lock, DebArch::Arm64).is_some()
}

fn keep_mtime_write(path: &Path, bytes: &[u8]) {
    let modified = fs::metadata(path).unwrap().modified().unwrap();
    fs::write(path, bytes).unwrap();
    File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(modified)
        .unwrap();
}

#[test]
fn a_fifo_record_is_refused_without_waiting_for_a_writer() {
    let dir = root();
    let package = deb(&good_items());
    let lock = lock_for(&package);
    let base = chatgpt_root(&dir);
    let record = base.join("1.2.3-arm64.published.json");
    let name = c_name_path(&record).unwrap();
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    let (send, receive) = std::sync::mpsc::channel();
    let status_root = base.clone();
    let status_lock = lock.clone();
    let reader = std::thread::spawn(move || {
        send.send(current_status(&status_root, &status_lock, DebArch::Arm64))
            .unwrap();
    });
    let result = receive.recv_timeout(Duration::from_secs(1));
    if result.is_err() {
        // Release a blocked reader before failing, so the test owns no stray thread.
        drop(
            OpenOptions::new()
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(&record)
                .unwrap(),
        );
    }
    reader.join().unwrap();
    assert_eq!(result.unwrap(), Status::Idle);
    let (result, calls) = again(&dir, &package, &lock);
    assert!(result.is_ok());
    assert_eq!(calls, 1);
    assert!(fs::symlink_metadata(record).unwrap().is_file());
}

#[test]
fn collection_removes_interrupted_deletions_from_the_published_folder() {
    let (dir, _, lock, path) = published();
    let base = chatgpt_root(&dir);
    let abandoned = base.join("published/.rejected-interrupted");
    fs::create_dir_all(abandoned.join("resources")).unwrap();
    fs::write(abandoned.join("resources/app.asar"), b"leftover").unwrap();
    let in_use = base.join("published/1.1.0-arm64");
    fs::create_dir_all(&in_use).unwrap();
    let keep = HashSet::from(["1.1.0-arm64".to_owned()]);
    assert!(collect_garbage(&base, &lock, DebArch::Arm64, &keep)
        .unwrap()
        .is_empty());
    assert!(!abandoned.exists());
    assert!(in_use.exists());
    assert!(path.join("ChatGPT").is_file());
}

#[test]
fn preparation_removes_interrupted_deletions_without_following_links() {
    let dir = root();
    let base = chatgpt_root(&dir);
    let abandoned = base.join("published/.rejected-interrupted");
    fs::create_dir_all(abandoned.join("resources")).unwrap();
    fs::write(abandoned.join("resources/app.asar"), b"leftover").unwrap();
    let outside = dir.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("keep"), b"precious").unwrap();
    std::os::unix::fs::symlink(&outside, base.join("published/.rejected-link")).unwrap();
    let package = deb(&good_items());
    let (result, _) = again(&dir, &package, &lock_for(&package));
    assert!(result.is_ok());
    assert!(!abandoned.exists());
    assert!(fs::symlink_metadata(base.join("published/.rejected-link")).is_err());
    assert_eq!(fs::read(outside.join("keep")).unwrap(), b"precious");
}

#[test]
fn a_planted_part_symlink_is_removed_not_written_through() {
    let dir = root();
    let package = deb(&good_items());
    let victim = dir.path().join("victim");
    fs::write(&victim, b"precious").unwrap();
    let downloads = chatgpt_root(&dir).join("downloads");
    fs::create_dir_all(&downloads).unwrap();
    std::os::unix::fs::symlink(&victim, downloads.join("chatgpt_1.2.3_arm64.deb.part")).unwrap();
    run(&dir, &package, &lock_for(&package)).0.unwrap();
    assert_eq!(fs::read(&victim).unwrap(), b"precious");
    // The real downloader's open refuses a link and a hard link, fresh or resuming.
    let part = dir.path().join("p.part");
    std::os::unix::fs::symlink(&victim, &part).unwrap();
    assert!(open_part(&part, true).is_err());
    assert_eq!(safe_part_len(&part).unwrap(), 0);
    assert!(!part.exists(), "a planted link is deleted, never followed");
    std::os::unix::fs::symlink(&victim, &part).unwrap();
    assert!(
        open_part(&part, false).is_ok(),
        "a fresh download replaces the link"
    );
    assert_eq!(fs::read(&victim).unwrap(), b"precious");
    fs::remove_file(&part).unwrap();
    fs::hard_link(&victim, &part).unwrap();
    assert!(open_part(&part, true).is_err());
    assert_eq!(safe_part_len(&part).unwrap(), 0);
    assert_eq!(fs::read(&victim).unwrap(), b"precious");
}

#[test]
fn planted_staging_and_version_symlinks_never_redirect_writes() {
    let dir = root();
    let package = deb(&good_items());
    let outside = dir.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("keep"), b"keep").unwrap();
    let base = chatgpt_root(&dir);
    std::os::unix::fs::symlink(&outside, base.join(".staging-evil")).unwrap();
    fs::create_dir_all(base.join("published")).unwrap();
    std::os::unix::fs::symlink(&outside, base.join("published/1.2.3-arm64")).unwrap();
    std::os::unix::fs::symlink(&outside, base.join(".rejected-evil")).unwrap();
    let path = run(&dir, &package, &lock_for(&package)).0.unwrap();
    assert!(fs::symlink_metadata(&path).unwrap().is_dir());
    assert!(path.join("ChatGPT").is_file());
    let names: Vec<_> = fs::read_dir(&outside).unwrap().flatten().collect();
    assert_eq!(names.len(), 1, "nothing was written through a link");
    assert_eq!(fs::read(outside.join("keep")).unwrap(), b"keep");
    assert!(fs::symlink_metadata(base.join(".staging-evil")).is_err());
}

#[test]
fn a_symlinked_storage_root_or_subdirectory_is_refused() {
    let (dir, package, lock, path) = published();
    let real = dir.path().join("real");
    fs::rename(chatgpt_root(&dir), &real).unwrap();
    std::os::unix::fs::symlink(&real, chatgpt_root(&dir)).unwrap();
    assert!(!verified(&dir, &lock));
    assert!(ensure_published_dir(&chatgpt_root(&dir)).is_err());
    let (result, calls) = again(&dir, &package, &lock);
    assert!(result.is_err() && calls == 0);
    assert!(RootLock::take(&chatgpt_root(&dir)).is_err());
    assert!(real.join("published/1.2.3-arm64").exists() && !path.starts_with("/nonexistent"));
    // A symlinked downloads folder: nothing is written through it.
    let dir = root();
    let outside = dir.path().join("outside");
    fs::create_dir(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, chatgpt_root(&dir).join("downloads")).unwrap();
    let package = deb(&good_items());
    let (result, calls) = again(&dir, &package, &lock_for(&package));
    assert!(result.is_err() && calls == 0);
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
    assert_nothing_published(&dir);
}

#[test]
fn a_root_writable_by_others_is_refused_and_tightened_when_created() {
    let dir = tempfile::tempdir().unwrap();
    let open = dir.path().join("open");
    fs::create_dir(&open).unwrap();
    fs::set_permissions(&open, fs::Permissions::from_mode(0o777)).unwrap();
    assert!(Dir::open_root(&open, false).is_err());
    assert!(Dir::open_root(&open, true).is_ok());
    assert_eq!(fs::metadata(&open).unwrap().mode() & 0o777, 0o700);
}

fn plant_fake_tree(tree: &Path) {
    fs::create_dir_all(tree.join("resources/cua_node/bin")).unwrap();
    for file in REQUIRED_EXECUTABLES {
        let path = tree.join(file);
        fs::write(&path, b"#!/bin/sh\necho evil\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[test]
fn a_preseeded_version_folder_is_never_accepted() {
    let dir = root();
    let package = deb(&good_items());
    let lock = lock_for(&package);
    fs::create_dir_all(chatgpt_root(&dir).join("published")).unwrap();
    plant_fake_tree(&chatgpt_root(&dir).join("published/1.2.3-arm64"));
    assert!(!verified(&dir, &lock));
    assert_eq!(
        current_status(&chatgpt_root(&dir), &lock, DebArch::Arm64),
        Status::Idle
    );
    let (result, calls) = again(&dir, &package, &lock);
    let path = result.unwrap();
    assert_eq!(calls, 1, "re-downloaded instead of trusting the folder");
    assert_eq!(fs::read(path.join("ChatGPT")).unwrap(), b"#!/bin/sh\n");
    // A genuine record over a forged tree does not match the tree digest.
    fs::remove_dir_all(&path).unwrap();
    plant_fake_tree(&path);
    forget_session(&chatgpt_root(&dir));
    assert!(!verified(&dir, &lock));
    let (result, calls) = again(&dir, &package, &lock);
    assert_eq!(calls, 1);
    assert_eq!(
        fs::read(result.unwrap().join("ChatGPT")).unwrap(),
        b"#!/bin/sh\n"
    );
}

#[test]
fn the_record_binds_the_lock_version_architecture_and_package_hash() {
    let (dir, _package, lock, _path) = published();
    let root = chatgpt_root(&dir);
    assert!(verified(&dir, &lock));
    let mut other_hash = lock.clone();
    other_hash.architectures.get_mut("arm64").unwrap().sha256 = "0".repeat(64);
    assert!(verify_published(&root, &other_hash, DebArch::Arm64).is_none());
    let mut other_version = lock.clone();
    other_version.version = "1.2.4".into();
    assert!(verify_published(&root, &other_version, DebArch::Arm64).is_none());
    // The amd64 name has no record, so the arm64 folder cannot stand in for it.
    assert!(verify_published(&root, &lock, DebArch::Amd64).is_none());
    // A record copied under another architecture's name is refused.
    let record = fs::read(root.join("1.2.3-arm64.published.json")).unwrap();
    fs::write(root.join("1.2.3-amd64.published.json"), record).unwrap();
    fs::create_dir_all(root.join("published/1.2.3-amd64")).unwrap();
    assert!(verify_published(&root, &lock, DebArch::Amd64).is_none());
}

#[test]
fn tampering_after_publication_is_detected() {
    // Same-size, same-mtime edit: only the full digest (new process) sees it.
    let (dir, package, lock, path) = published();
    keep_mtime_write(&path.join("resources/app.asar"), b"EVIL");
    assert!(
        verified(&dir, &lock),
        "within the session the cheap check cannot see it"
    );
    forget_session(&chatgpt_root(&dir));
    assert!(!verified(&dir, &lock));
    let (result, calls) = again(&dir, &package, &lock);
    assert_eq!(calls, 1);
    assert_eq!(
        fs::read(result.unwrap().join("resources/app.asar")).unwrap(),
        b"data"
    );

    // Size change: caught on the very next call, even in-session.
    let (dir, package, lock, path) = published();
    fs::write(path.join("resources/app.asar"), b"longer content").unwrap();
    assert!(!verified(&dir, &lock));
    assert_eq!(again(&dir, &package, &lock).1, 1);

    // An added file, a removed file, a changed link, a loosened mode.
    let tampers: [fn(&Path); 4] = [
        |p| fs::write(p.join("planted"), b"x").unwrap(),
        |p| fs::remove_file(p.join("resources/app.asar")).unwrap(),
        |p| {
            fs::remove_file(p.join("resources/current")).unwrap();
            std::os::unix::fs::symlink("../ChatGPT", p.join("resources/current")).unwrap();
        },
        |p| fs::set_permissions(p.join("resources"), fs::Permissions::from_mode(0o777)).unwrap(),
    ];
    for tamper in tampers {
        let (dir, package, lock, path) = published();
        tamper(&path);
        assert!(!verified(&dir, &lock));
        let (result, calls) = again(&dir, &package, &lock);
        assert_eq!(calls, 1);
        let healed = result.unwrap();
        assert!(!healed.join("planted").exists());
        assert!(healed.join("resources/app.asar").is_file());
    }
}

#[test]
fn an_interrupted_publish_is_never_reused() {
    // The tree was renamed into place but the record never became durable.
    let (dir, package, lock, _path) = published();
    fs::remove_file(chatgpt_root(&dir).join("1.2.3-arm64.published.json")).unwrap();
    assert!(!verified(&dir, &lock));
    assert_eq!(
        current_status(&chatgpt_root(&dir), &lock, DebArch::Arm64),
        Status::Idle
    );
    let (result, calls) = again(&dir, &package, &lock);
    assert_eq!(calls, 1);
    assert!(result.unwrap().join("ChatGPT").is_file());
    assert!(verified(&dir, &lock));

    // A torn record (partial write) is not a record.
    let record = chatgpt_root(&dir).join("1.2.3-arm64.published.json");
    let bytes = fs::read(&record).unwrap();
    fs::write(&record, &bytes[..bytes.len() / 2]).unwrap();
    forget_session(&chatgpt_root(&dir));
    assert!(!verified(&dir, &lock));
    assert_eq!(again(&dir, &package, &lock).1, 1);

    // A record present but the tree partial (files missing) is not ready.
    let (dir, package, lock, path) = published();
    fs::remove_dir_all(path.join("resources")).unwrap();
    forget_session(&chatgpt_root(&dir));
    assert!(!verified(&dir, &lock));
    assert_eq!(again(&dir, &package, &lock).1, 1);

    // Leftover staging, moved-aside folders and temporaries are removed.
    let base = chatgpt_root(&dir);
    fs::create_dir_all(base.join(".staging-crash/resources")).unwrap();
    fs::create_dir_all(base.join(".rejected-old")).unwrap();
    fs::write(base.join(".1.2.3-arm64.published.json-9.tmp"), b"{").unwrap();
    fs::remove_file(base.join("1.2.3-arm64.published.json")).unwrap();
    assert_eq!(again(&dir, &package, &lock).1, 1);
    let leftovers: Vec<_> = fs::read_dir(&base)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| {
            n.starts_with(".staging-") || n.ends_with(".tmp") || n.starts_with(".rejected-")
        })
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn the_record_holds_the_tree_digests_and_is_written_after_the_tree() {
    let (dir, _package, lock, path) = published();
    let record_path = chatgpt_root(&dir).join("1.2.3-arm64.published.json");
    let record: Record = serde_json::from_slice(&fs::read(&record_path).unwrap()).unwrap();
    assert_eq!(record.version, "1.2.3");
    assert_eq!(record.arch, "arm64");
    assert_eq!(
        record.deb_sha256,
        lock.asset(DebArch::Arm64).unwrap().sha256
    );
    let digests = digest_tree(&path, true).unwrap();
    assert_eq!(digests.content.unwrap(), record.tree_sha256);
    assert_eq!(digests.stat, record.stat_sha256);
    assert_eq!(digests.entries, record.entries);
    let recorded = fs::metadata(&record_path).unwrap().modified().unwrap();
    assert!(recorded >= fs::metadata(&path).unwrap().modified().unwrap());
}

#[test]
fn a_non_executable_required_file_is_refused_and_heals_when_it_changes() {
    let node = "./usr/lib/chatgpt/resources/cua_node/bin/node";
    let repl = "./usr/lib/chatgpt/resources/cua_node/bin/node_repl";
    let cases: Vec<Vec<Item>> = vec![
        // ChatGPT without an execute bit (normalized to 0644).
        vec![
            Item::File("./usr/lib/chatgpt/ChatGPT", 0o644, b"x"),
            Item::File(node, 0o755, b"x"),
            Item::File(repl, 0o755, b"x"),
        ],
        // node_repl missing.
        vec![
            Item::File("./usr/lib/chatgpt/ChatGPT", 0o755, b"x"),
            Item::File(node, 0o755, b"x"),
        ],
        // node not executable.
        vec![
            Item::File("./usr/lib/chatgpt/ChatGPT", 0o755, b"x"),
            Item::File(node, 0o644, b"x"),
            Item::File(repl, 0o755, b"x"),
        ],
        // ChatGPT a directory.
        vec![
            Item::Dir("./usr/lib/chatgpt/ChatGPT", 0o755),
            Item::File(node, 0o755, b"x"),
            Item::File(repl, 0o755, b"x"),
        ],
        // ChatGPT a dangling link.
        vec![
            Item::Link("./usr/lib/chatgpt/ChatGPT", "missing"),
            Item::File(node, 0o755, b"x"),
            Item::File(repl, 0o755, b"x"),
        ],
    ];
    for items in cases {
        let dir = root();
        let package = deb(&items);
        let (result, _) = run(&dir, &package, &lock_for(&package));
        assert!(result.is_err());
        assert_nothing_published(&dir);
    }
    // A published tree that loses an execute bit is no longer ready.
    let (dir, package, lock, path) = published();
    fs::set_permissions(
        path.join("resources/cua_node/bin/node_repl"),
        fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    assert!(!verified(&dir, &lock));
    assert_eq!(again(&dir, &package, &lock).1, 1);
}

/// A tar whose single regular file has header size `header_size` and a PAX
/// `size` record equal to the length of `effective`.
fn pax_size_tar(effective: &[u8], header_size: u64) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    let size = effective.len().to_string();
    builder
        .append_pax_extensions([("size", size.as_bytes())])
        .unwrap();
    let mut header = raw_header(
        "./usr/lib/chatgpt/ChatGPT",
        tar::EntryType::Regular,
        0o755,
        header_size,
    );
    header.set_cksum();
    builder.append(&header, std::io::empty()).unwrap();
    let out = builder.get_mut();
    out.extend_from_slice(effective);
    out.resize(out.len().div_ceil(512) * 512, 0);
    builder.into_inner().unwrap()
}

fn destination() -> (tempfile::TempDir, Dir) {
    let dir = tempfile::tempdir().unwrap();
    let dest = Dir::open_root(&dir.path().join("t"), true).unwrap();
    (dir, dest)
}

#[test]
fn a_pax_size_larger_than_the_header_size_is_accounted() {
    let content = vec![b'x'; 100];
    let tar = pax_size_tar(&content, 0);
    // Within limits the effective content is written intact.
    let (dir, dest) = destination();
    let generous = Limits {
        max_bytes: 1000,
        ..Limits::default()
    };
    let error = extract_tree_with(&tar[..], &dest, &generous).unwrap_err();
    assert!(error.message.contains("required executable"), "{error}");
    assert_eq!(fs::read(dir.path().join("t/ChatGPT")).unwrap(), content);
    // Over the cap by effective size: refused before a byte is written.
    let (dir, dest) = destination();
    let tight = Limits {
        max_bytes: 50,
        ..Limits::default()
    };
    let error = extract_tree_with(&tar[..], &dest, &tight).unwrap_err();
    assert!(error.message.contains("too large"), "{error}");
    assert!(!error.retryable);
    assert!(!dir.path().join("t/ChatGPT").exists());
}

#[test]
fn sizes_are_summed_with_checked_arithmetic() {
    let (_dir, dest) = destination();
    let mut items = good_items();
    items.push(Item::File("./usr/lib/chatgpt/big1", 0o644, &[1u8; 60]));
    items.push(Item::File("./usr/lib/chatgpt/big2", 0o644, &[1u8; 60]));
    let limits = Limits {
        max_bytes: 100,
        ..Limits::default()
    };
    let error = extract_tree_with(&data_tar(&items)[..], &dest, &limits).unwrap_err();
    assert!(error.message.contains("too large"), "{error}");
    // A PAX size near u64::MAX is refused, not wrapped around.
    let (_dir, dest) = destination();
    let mut builder = tar::Builder::new(Vec::new());
    builder
        .append_pax_extensions([("size", "18446744073709551615".as_bytes())])
        .unwrap();
    let mut header = raw_header("./usr/lib/chatgpt/a", tar::EntryType::Regular, 0o644, 0);
    header.set_cksum();
    builder.append(&header, std::io::empty()).unwrap();
    let bytes = builder.into_inner().unwrap();
    assert!(extract_tree_with(&bytes[..], &dest, &Limits::default()).is_err());
}

fn long_name_tar(name: &str) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_size(1);
    header.set_mode(0o644);
    builder.append_data(&mut header, name, &b"x"[..]).unwrap();
    builder.into_inner().unwrap()
}

#[test]
fn metadata_sizes_and_entry_counts_are_limited() {
    // A GNU long name (type L) longer than the limit.
    let (_dir, dest) = destination();
    let long = format!("./usr/lib/chatgpt/{}", "a".repeat(5000));
    let error = extract_tree(&long_name_tar(&long)[..], &dest).unwrap_err();
    assert!(error.message.contains("too long"), "{error}");
    // Long names outside the tree count too.
    let (_dir, dest) = destination();
    let outside = format!("./etc/{}", "a".repeat(5000));
    assert!(extract_tree(&long_name_tar(&outside)[..], &dest).is_err());
    // A name component longer than 255 bytes.
    let (_dir, dest) = destination();
    let name = format!("./usr/lib/chatgpt/{}", "b".repeat(300));
    let error = extract_tree(&long_name_tar(&name)[..], &dest).unwrap_err();
    assert!(error.message.contains("too long"), "{error}");
    // PAX extended header records beyond the limit.
    let (_dir, dest) = destination();
    let mut builder = tar::Builder::new(Vec::new());
    builder
        .append_pax_extensions([("comment", vec![b'c'; 100_000].as_slice())])
        .unwrap();
    let mut header = raw_header("./usr/lib/chatgpt/a", tar::EntryType::Regular, 0o644, 1);
    header.set_cksum();
    builder.append(&header, &b"x"[..]).unwrap();
    let error = extract_tree(&builder.into_inner().unwrap()[..], &dest).unwrap_err();
    assert!(error.message.contains("too large"), "{error}");
    // Entry count.
    let (_dir, dest) = destination();
    let limits = Limits {
        max_entries: 3,
        ..Limits::default()
    };
    let error = extract_tree_with(&data_tar(&good_items())[..], &dest, &limits).unwrap_err();
    assert!(error.message.contains("too many"), "{error}");
}

struct Counting<R> {
    inner: R,
    read: std::sync::Arc<AtomicUsize>,
}

impl<R: Read> Read for Counting<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let count = self.inner.read(buffer)?;
        self.read.fetch_add(count, Ordering::SeqCst);
        Ok(count)
    }
}

#[test]
fn extraction_aborts_at_the_first_rejected_entry() {
    let huge = vec![7u8; 8 * 1024 * 1024];
    let items = vec![
        Item::File("./usr/lib/chatgpt/ChatGPT", 0o4755, b"setuid"),
        Item::File("./usr/lib/chatgpt/huge", 0o644, &huge),
    ];
    let bytes = data_tar(&items);
    let read = std::sync::Arc::new(AtomicUsize::new(0));
    let reader = Counting {
        inner: &bytes[..],
        read: read.clone(),
    };
    let (_dir, dest) = destination();
    assert!(extract_tree(reader, &dest).is_err());
    assert!(
        read.load(Ordering::SeqCst) < 64 * 1024,
        "read {} of {} bytes",
        read.load(Ordering::SeqCst),
        bytes.len()
    );
}

#[test]
fn a_rejected_package_leaves_nothing_behind_through_the_real_unpacker() {
    let dir = root();
    let huge = vec![7u8; 32 * 1024 * 1024];
    let items = vec![
        Item::File("./usr/lib/chatgpt/ChatGPT", 0o4755, b"setuid"),
        Item::File("./usr/lib/chatgpt/huge", 0o644, &huge),
    ];
    let package = deb(&items);
    let (result, _) = run(&dir, &package, &lock_for(&package));
    assert!(!result.unwrap_err().retryable);
    assert_nothing_published(&dir);
}

#[test]
fn exclusive_creation_refuses_existing_names_and_planted_links() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("chatgpt");
    let victim = dir.path().join("victim");
    fs::write(&victim, b"precious").unwrap();
    let handle = Dir::open_root(&root, true).unwrap();
    drop(handle.create_file("existing", 0o600).unwrap());
    assert!(handle.create_file("existing", 0o600).is_err());
    std::os::unix::fs::symlink(&victim, root.join("t.tmp")).unwrap();
    assert!(handle.create_file("t.tmp", 0o600).is_err());
    assert_eq!(fs::read(&victim).unwrap(), b"precious");
}

#[test]
fn the_published_folder_always_exists_and_only_holds_verified_trees() {
    let dir = root();
    let base = chatgpt_root(&dir);
    // Created empty before anything is downloaded.
    let mounted = ensure_published_dir(&base).unwrap();
    assert_eq!(mounted, fs::canonicalize(base.join("published")).unwrap());
    assert_eq!(fs::read_dir(&mounted).unwrap().count(), 0);
    let package = deb(&good_items());
    let lock = lock_for(&package);
    let path = run(&dir, &package, &lock).0.unwrap();
    assert_eq!(path, mounted.join("1.2.3-arm64"));
    // Records, downloads and staging stay outside the mounted folder.
    let names: Vec<_> = fs::read_dir(&mounted)
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["1.2.3-arm64"]);
    assert!(base.join("1.2.3-arm64.published.json").is_file());
    // A symlinked published folder is refused.
    let dir = root();
    let base = chatgpt_root(&dir);
    fs::create_dir_all(&base).unwrap();
    let elsewhere = dir.path().join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, base.join("published")).unwrap();
    assert!(ensure_published_dir(&base).is_err());
}

#[test]
fn a_tree_published_at_the_old_location_moves_and_is_still_verified() {
    let (dir, package, lock, path) = published();
    let base = chatgpt_root(&dir);
    // Recreate the old layout: tree directly under the root, no published folder.
    fs::rename(&path, base.join("1.2.3-arm64")).unwrap();
    fs::remove_dir(base.join("published")).unwrap();
    assert!(
        !verified(&dir, &lock),
        "nothing is published before the move"
    );
    let mounted = ensure_published_dir(&base).unwrap();
    assert!(!base.join("1.2.3-arm64").exists());
    assert!(mounted.join("1.2.3-arm64/ChatGPT").is_file());
    // The moved tree is accepted only because the record and digests match.
    let (result, calls) = again(&dir, &package, &lock);
    assert_eq!(result.unwrap(), mounted.join("1.2.3-arm64"));
    assert_eq!(calls, 0);
}

#[test]
fn a_busy_storage_lock_does_not_allow_a_symlinked_mount_folder() {
    let dir = root();
    let base = chatgpt_root(&dir);
    let mounted = ensure_published_dir(&base).unwrap();
    let _held = RootLock::take(&base).unwrap();
    assert_eq!(ensure_published_dir_nowait(&base).unwrap(), mounted);
    let outside = dir.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::remove_dir(base.join("published")).unwrap();
    std::os::unix::fs::symlink(&outside, base.join("published")).unwrap();
    assert!(ensure_published_dir_nowait(&base).is_err());
    assert_eq!(fs::read_dir(outside).unwrap().count(), 0);
}
