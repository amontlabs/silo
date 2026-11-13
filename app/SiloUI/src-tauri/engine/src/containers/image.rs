//! The pinned guest image, unpacked once per digest into the engine's cache.
//!
//! The engine already downloads and verifies the image archive (an OCI image layout). skopeo
//! reads it into a layout directory, the layout's config digest is checked against the pin, and
//! umoci unpacks it inside the holder's user namespace (`silo-ctr userns`), where file owners
//! are kept: the image's uid 1000 becomes a sub-id of the account, as it would be in the
//! container. (`umoci unpack --rootless` would give every file to the account itself.)
//! Containers mount the result read-only as the bottom overlay layer.

use super::{
    layout::Layout,
    tools::{run_checked, Tools},
};
use crate::runtime::guest_image::{extract_verified_archive, GuestImageManifest, PinnedImage};
use crate::runtime::{RuntimeError, RuntimePaths};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static UNPACK_LOCK: Mutex<()> = Mutex::new(());
const TAG: &str = "silo";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unpacked {
    pub rootfs: PathBuf,
    pub digest: String,
    pub reference: String,
}

pub(crate) fn pinned() -> Result<GuestImageManifest, RuntimeError> {
    PinnedImage::host()
        .map(|pinned| pinned.manifest)
        .map_err(RuntimeError::Unavailable)
}

/// Where the image with config `digest` is unpacked, when it is. The marker file must name the
/// same digest, so a half-made or swapped directory is not taken for the image.
pub(crate) fn unpacked_digest(layout: &Layout, digest: &str, reference: &str) -> Option<Unpacked> {
    let dir = layout.image(digest);
    let marker = std::fs::read_to_string(dir.join("digest")).ok()?;
    (marker.trim() == digest && dir.join("rootfs").is_dir()).then(|| Unpacked {
        rootfs: dir.join("rootfs"),
        digest: digest.to_owned(),
        reference: reference.to_owned(),
    })
}

/// Where the pinned image is unpacked, when it is.
pub(crate) fn unpacked(layout: &Layout, manifest: &GuestImageManifest) -> Option<Unpacked> {
    unpacked_digest(layout, &manifest.image_digest, &manifest.image_reference)
}

/// The image a computer runs on is the one recorded when it was made, never whatever is pinned
/// now: its writable layer holds changes against exactly that image.
#[derive(Debug, PartialEq, Eq)]
pub enum RecordedImage {
    Ready(Unpacked),
    /// Not unpacked, but it is the pinned image: unpack it.
    UnpackPinned,
    /// Not unpacked and no longer pinned: this Silo cannot make it again.
    Missing,
}

pub fn recorded_image(
    layout: &Layout,
    record: &super::layout::ComputerRecord,
    pinned_digest: &str,
) -> RecordedImage {
    match unpacked_digest(layout, &record.image_digest, &record.image_reference) {
        Some(found) => RecordedImage::Ready(found),
        None if record.image_digest == pinned_digest => RecordedImage::UnpackPinned,
        None => RecordedImage::Missing,
    }
}

/// The image of a computer's record, unpacking the pinned image when that is the one it needs.
pub fn ensure_recorded(
    paths: &RuntimePaths,
    tools: &Tools,
    name: &str,
    record: &super::layout::ComputerRecord,
) -> Result<Unpacked, RuntimeError> {
    let layout = Layout::new(&paths.home);
    match recorded_image(&layout, record, &pinned()?.image_digest) {
        RecordedImage::Ready(found) => Ok(found),
        RecordedImage::UnpackPinned => ensure(paths, tools),
        RecordedImage::Missing => Err(RuntimeError::Unavailable(format!(
            "Container computer {name} was made from an image ({}) that this Silo no longer carries and has not kept. \
             Update Silo to the version that made it, or create a new computer.",
            record.image_digest
        ))),
    }
}

pub fn ensure(paths: &RuntimePaths, tools: &Tools) -> Result<Unpacked, RuntimeError> {
    let manifest = pinned()?;
    let layout = Layout::new(&paths.home);
    let _guard = UNPACK_LOCK.lock().map_err(|_| {
        RuntimeError::Unavailable(
            "Image preparation is unavailable. Restart Silo and retry.".into(),
        )
    })?;
    if let Some(done) = unpacked(&layout, &manifest) {
        return Ok(done);
    }
    let failed = |detail: String| {
        eprintln!("Silo could not unpack the container image: {detail}");
        RuntimeError::Unavailable(
            "Silo could not prepare the image for a container computer. Check disk space and retry."
                .into(),
        )
    };
    let images = layout.images();
    std::fs::create_dir_all(&images).map_err(|error| failed(error.to_string()))?;
    super::layout::strip_default_acl(layout.root());
    super::layout::strip_default_acl(&images);
    // Old images no computer needs go first, so the space check counts only what is needed.
    #[cfg(target_os = "linux")]
    super::lifecycle::prune_storage(paths, tools);
    crate::runtime::guest_image::check_space(&images, manifest.unpacked_bytes.saturating_mul(3))
        .map_err(RuntimeError::Unavailable)?;
    remove_partial(tools, &images);
    let work = images.join(format!("{PARTIAL}{}", std::process::id()));
    let result = unpack_into(paths, tools, &manifest, &layout, &work);
    // A partial unpack holds files of sub-ids: only the namespace can remove them.
    let _ = remove_tree(tools, &work);
    result.map_err(failed)?;
    unpacked(&layout, &manifest).ok_or_else(|| failed("the unpacked image is incomplete".into()))
}

fn unpack_into(
    paths: &RuntimePaths,
    tools: &Tools,
    manifest: &GuestImageManifest,
    layout: &Layout,
    work: &Path,
) -> Result<(), String> {
    std::fs::create_dir_all(work).map_err(|e| e.to_string())?;
    let tar = work.join("image.tar");
    extract_verified_archive(paths, manifest, &tar).map_err(|e| e.to_string())?;
    let oci = work.join("oci");
    run_checked(
        &tools.skopeo,
        &[
            "--insecure-policy".as_ref(),
            "copy".as_ref(),
            format!("oci-archive:{}", tar.display()).as_ref(),
            format!("oci:{}:{TAG}", oci.display()).as_ref(),
        ],
    )?;
    let _ = std::fs::remove_file(&tar);
    let found = config_digest(&oci)?;
    if found != manifest.image_digest {
        return Err(format!(
            "the image is {found}, not the pinned {}",
            manifest.image_digest
        ));
    }
    let bundle = work.join("bundle");
    run_checked(
        &tools.ctr,
        &[
            "userns".as_ref(),
            "--keep-id".as_ref(),
            super::idmap::AGENT_ID.to_string().as_ref(),
            "--".as_ref(),
            tools.umoci.as_ref(),
            "unpack".as_ref(),
            "--image".as_ref(),
            format!("{}:{TAG}", oci.display()).as_ref(),
            bundle.as_ref(),
        ],
    )?;
    let finished = work.join("done");
    std::fs::create_dir_all(&finished).map_err(|e| e.to_string())?;
    // The root file system's top directory is root's (a sub-id), which the engine may not move
    // between folders: the namespace moves it.
    run_checked(
        &tools.ctr,
        &[
            "userns".as_ref(),
            "--keep-id".as_ref(),
            super::idmap::AGENT_ID.to_string().as_ref(),
            "--".as_ref(),
            "mv".as_ref(),
            "--".as_ref(),
            bundle.join("rootfs").as_ref(),
            finished.join("rootfs").as_ref(),
        ],
    )?;
    std::fs::write(
        finished.join("digest"),
        format!("{}\n", manifest.image_digest),
    )
    .map_err(|e| e.to_string())?;
    let target = layout.image(&manifest.image_digest);
    // A concurrent engine process may have finished first; keep its result.
    match std::fs::rename(&finished, &target) {
        Ok(()) => Ok(()),
        Err(_) if target.join("digest").is_file() => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

const PARTIAL: &str = "partial-";
/// A partial unpack whose engine is gone is stale at once; one older than this is stale even if
/// its process id has been reused.
const PARTIAL_MAX_AGE: std::time::Duration = std::time::Duration::from_secs(24 * 3600);

pub(super) fn process_alive(pid: u32) -> bool {
    // SAFETY: signal 0 only checks that the process exists.
    let result = unsafe { libc::kill(pid as i32, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// The partial unpacks (`partial-<engine pid>`) in `images` that no running engine is working on.
/// Another engine process of the same account may be unpacking right now: its directory is left.
pub fn stale_partials(
    images: &Path,
    own_pid: u32,
    alive: &dyn Fn(u32) -> bool,
    now: std::time::SystemTime,
) -> Vec<PathBuf> {
    std::fs::read_dir(images)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let pid: u32 = name.strip_prefix(PARTIAL)?.parse().ok()?;
            let old = entry
                .metadata()
                .and_then(|meta| meta.modified())
                .ok()
                .and_then(|modified| now.duration_since(modified).ok())
                .is_some_and(|age| age > PARTIAL_MAX_AGE);
            // Our own earlier attempt is ours to clear; anyone else's only when it is gone.
            (pid == own_pid || !alive(pid) || old).then(|| entry.path())
        })
        .collect()
}

/// Removes earlier partial unpacks (an engine that stopped half-way), and no one else's.
fn remove_partial(tools: &Tools, images: &Path) {
    for stale in stale_partials(
        images,
        std::process::id(),
        &process_alive,
        std::time::SystemTime::now(),
    ) {
        let _ = remove_tree(tools, &stale);
    }
}

/// The directories in `root` that are not named in `keep`, except the partial ones (an unpack or a
/// boot layer in the making belongs to whoever is making it). Files are never listed.
pub fn unused_dirs(root: &Path, keep: &[String]) -> Vec<PathBuf> {
    std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            !name.starts_with(PARTIAL) && !name.ends_with(".partial") && !keep.contains(&name)
        })
        .map(|entry| entry.path())
        .collect()
}

/// Removes a directory tree that may hold files owned by sub-ids.
pub fn remove_tree(tools: &Tools, path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    run_checked(
        &tools.ctr,
        &[
            "userns".as_ref(),
            "--".as_ref(),
            "/bin/rm".as_ref(),
            "-rf".as_ref(),
            "--".as_ref(),
            path.as_ref(),
        ],
    )
    .map(drop)
}

/// The config digest of the single image in an OCI layout directory.
pub fn config_digest(layout: &Path) -> Result<String, String> {
    let read = |path: PathBuf| -> Result<Value, String> {
        serde_json::from_slice(
            &std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?,
        )
        .map_err(|e| format!("{}: {e}", path.display()))
    };
    let blob = |digest: &str| -> Result<PathBuf, String> {
        let hex = digest
            .strip_prefix("sha256:")
            .ok_or("an unsupported digest")?;
        if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("an invalid digest".into());
        }
        Ok(layout.join("blobs/sha256").join(hex))
    };
    let index = read(layout.join("index.json"))?;
    let manifests = index["manifests"].as_array().ok_or("no manifests")?;
    if manifests.len() != 1 {
        return Err("the layout does not hold exactly one image".into());
    }
    let manifest = read(blob(
        manifests[0]["digest"]
            .as_str()
            .ok_or("no manifest digest")?,
    )?)?;
    Ok(manifest["config"]["digest"]
        .as_str()
        .ok_or("no config digest")?
        .to_owned())
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_directories_nobody_needs_are_unused_and_partial_ones_are_left_to_their_maker() {
        let root = tempfile::tempdir().unwrap();
        for directory in [
            "aaa-keepid",
            "bbb-keepid",
            "ccc",
            "partial-123",
            "d.partial",
        ] {
            std::fs::create_dir(root.path().join(directory)).unwrap();
        }
        std::fs::write(root.path().join("note.txt"), "x").unwrap();
        let mut unused = unused_dirs(root.path(), &["aaa-keepid".to_owned()]);
        unused.sort();
        assert_eq!(
            unused,
            [root.path().join("bbb-keepid"), root.path().join("ccc")]
        );
        assert!(unused_dirs(&root.path().join("missing"), &[]).is_empty());
    }
    use super::*;

    fn fake_layout(config: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let blobs = dir.path().join("blobs/sha256");
        std::fs::create_dir_all(&blobs).unwrap();
        let manifest = "a".repeat(64);
        std::fs::write(
            dir.path().join("index.json"),
            format!(r#"{{"manifests":[{{"digest":"sha256:{manifest}"}}]}}"#),
        )
        .unwrap();
        std::fs::write(
            blobs.join(&manifest),
            format!(r#"{{"config":{{"digest":"{config}"}}}}"#),
        )
        .unwrap();
        dir
    }

    #[test]
    fn the_config_digest_is_read_from_the_layout() {
        let config = format!("sha256:{}", "b".repeat(64));
        let dir = fake_layout(&config);
        assert_eq!(config_digest(dir.path()).unwrap(), config);
    }

    #[test]
    fn a_layout_with_two_images_or_a_bad_digest_is_refused() {
        let dir = fake_layout("sha256:x");
        std::fs::write(
            dir.path().join("index.json"),
            r#"{"manifests":[{"digest":"sha256:../x"},{"digest":"y"}]}"#,
        )
        .unwrap();
        assert!(config_digest(dir.path())
            .unwrap_err()
            .contains("exactly one"));
        std::fs::write(
            dir.path().join("index.json"),
            r#"{"manifests":[{"digest":"sha256:../x"}]}"#,
        )
        .unwrap();
        assert!(config_digest(dir.path())
            .unwrap_err()
            .contains("invalid digest"));
    }

    fn mark_unpacked(layout: &Layout, digest: &str) {
        let dir = layout.image(digest);
        std::fs::create_dir_all(dir.join("rootfs")).unwrap();
        std::fs::write(dir.join("digest"), format!("{digest}\n")).unwrap();
    }

    fn record_with(digest: &str) -> super::super::layout::ComputerRecord {
        super::super::layout::ComputerRecord {
            schema_version: super::super::layout::RECORD_VERSION,
            name: "dev".into(),
            cpus: 1,
            max_cpus: 1,
            memory_mib: 1,
            max_memory_mib: 1,
            root_disk_mib: 1,
            workspace_mib: 1,
            labels: Vec::new(),
            env: Vec::new(),
            image_digest: digest.into(),
            image_reference: "ref:1".into(),
            layers: Vec::new(),
            upper: "l0".into(),
            created_ms: 1,
            started_once: true,
            secrets: Vec::new(),
            github: false,
            network_blocked: false,
        }
    }

    #[test]
    fn a_computer_runs_on_the_image_it_was_made_from_not_the_pinned_one() {
        let home = tempfile::tempdir().unwrap();
        let layout = Layout::new(home.path());
        let (old, pinned) = ("sha256:old", "sha256:new");
        mark_unpacked(&layout, pinned);
        // Both images are unpacked: the record's own is chosen.
        mark_unpacked(&layout, old);
        match recorded_image(&layout, &record_with(old), pinned) {
            RecordedImage::Ready(found) => {
                assert_eq!(found.digest, old);
                assert_eq!(found.rootfs, layout.image(old).join("rootfs"));
                assert_eq!(found.reference, "ref:1");
            }
            other => panic!("{other:?}"),
        }
        // Only the pinned image is unpacked and the record needs another: never the pinned one.
        std::fs::remove_dir_all(layout.image(old)).unwrap();
        assert_eq!(
            recorded_image(&layout, &record_with(old), pinned),
            RecordedImage::Missing
        );
        // The record needs the pinned image and it is gone: it can be unpacked again.
        std::fs::remove_dir_all(layout.image(pinned)).unwrap();
        assert_eq!(
            recorded_image(&layout, &record_with(pinned), pinned),
            RecordedImage::UnpackPinned
        );
    }

    #[test]
    fn a_missing_image_is_named_in_a_clear_refusal() {
        let home = tempfile::tempdir().unwrap();
        let paths = crate::test_support::paths(home.path());
        let record = record_with("sha256:gone");
        // Nothing is run: a missing, unpinned image fails before any tool is needed.
        let tools = Tools {
            ctr: "ctr".into(),
            init: "init".into(),
            gateway: "gateway".into(),
            crun: "crun".into(),
            skopeo: "skopeo".into(),
            umoci: "umoci".into(),
            systemd_run: "systemd-run".into(),
            systemctl: "systemctl".into(),
        };
        let error = ensure_recorded(&paths, &tools, "dev", &record).unwrap_err();
        let text = error.to_string();
        assert!(
            text.contains("sha256:gone") && text.contains("no longer carries"),
            "{text}"
        );
    }

    #[test]
    fn only_partial_unpacks_of_engines_that_are_gone_are_stale() {
        let images = tempfile::tempdir().unwrap();
        for name in [
            "partial-100",
            "partial-200",
            "partial-300",
            "abcdef",
            "partial-x",
        ] {
            std::fs::create_dir(images.path().join(name)).unwrap();
        }
        let now = std::time::SystemTime::now();
        // 100 is another live engine; 200 is gone; 300 is this process.
        let alive = |pid: u32| pid == 100;
        let mut stale: Vec<_> = stale_partials(images.path(), 300, &alive, now)
            .into_iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        stale.sort();
        assert_eq!(stale, ["partial-200", "partial-300"]);
        // A live pid that has not touched its directory for a day was reused: stale.
        let later = now + std::time::Duration::from_secs(25 * 3600);
        assert!(stale_partials(images.path(), 300, &alive, later)
            .iter()
            .any(|path| path.ends_with("partial-100")));
    }

    #[test]
    fn an_unpacked_image_needs_its_marker_and_root_filesystem() {
        let home = tempfile::tempdir().unwrap();
        let layout = Layout::new(home.path());
        let manifest = pinned().unwrap();
        assert!(unpacked(&layout, &manifest).is_none());
        let dir = layout.image(&manifest.image_digest);
        std::fs::create_dir_all(dir.join("rootfs")).unwrap();
        assert!(unpacked(&layout, &manifest).is_none());
        std::fs::write(dir.join("digest"), "sha256:other\n").unwrap();
        assert!(unpacked(&layout, &manifest).is_none());
        std::fs::write(dir.join("digest"), format!("{}\n", manifest.image_digest)).unwrap();
        let found = unpacked(&layout, &manifest).unwrap();
        assert_eq!(found.rootfs, dir.join("rootfs"));
        assert_eq!(found.digest, manifest.image_digest);
    }
}
