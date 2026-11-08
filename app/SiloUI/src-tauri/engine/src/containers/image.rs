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

/// Where the image with `digest` is unpacked, when it is.
pub(crate) fn unpacked(layout: &Layout, manifest: &GuestImageManifest) -> Option<Unpacked> {
    let dir = layout.image(&manifest.image_digest);
    let marker = std::fs::read_to_string(dir.join("digest")).ok()?;
    (marker.trim() == manifest.image_digest && dir.join("rootfs").is_dir()).then(|| Unpacked {
        rootfs: dir.join("rootfs"),
        digest: manifest.image_digest.clone(),
        reference: manifest.image_reference.clone(),
    })
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
    crate::runtime::guest_image::check_space(&images, manifest.unpacked_bytes.saturating_mul(3))
        .map_err(RuntimeError::Unavailable)?;
    remove_partial(tools, &images);
    let work = images.join(format!("partial-{}", std::process::id()));
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
    std::fs::rename(bundle.join("rootfs"), finished.join("rootfs")).map_err(|e| e.to_string())?;
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

/// Removes earlier partial unpacks (an engine that stopped half-way).
fn remove_partial(tools: &Tools, images: &Path) {
    for entry in std::fs::read_dir(images).into_iter().flatten().flatten() {
        if entry.file_name().to_string_lossy().starts_with("partial-") {
            let _ = remove_tree(tools, &entry.path());
        }
    }
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
