//! Keep image VMDK descriptors pointing into the runtime they belong to.
//!
//! MicroSandbox boots a computer's image from a VMDK descriptor that names its EROFS files
//! (`cache/fsmeta/…`, `cache/layers/…`) by absolute path. Copying a runtime, as the
//! checkpoint-runtime conversion does, keeps those paths, so the converted runtime
//! still reads the previous runtime's files; once that runtime is removed, its computers
//! cannot boot ("Data storage file … No such file or directory"). The same files exist
//! in the copied cache, so each missing extent is pointed at its copy here. Sector
//! counts and offsets are kept as they are. MicroSandbox's own `rebind_vmdk` (Silo's
//! portable-image-cache patch) does this for imports inside the runtime, but Silo's
//! crate dependency does not include that patch.
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// The descriptor's extent lines, each pointing at this cache's copy of its file.
/// `None` when nothing needs to change.
fn rebind(descriptor: &str, cache: &Path) -> Result<Option<String>, String> {
    let own =
        fs::canonicalize(cache).map_err(|_| "The runtime's image cache could not be resolved.")?;
    let mut changed = false;
    let mut lines = Vec::new();
    for line in descriptor.lines() {
        let Some(path) = extent_path(line) else {
            lines.push(line.to_owned());
            continue;
        };
        if Path::new(path).starts_with(&own) {
            if !Path::new(path).is_file() {
                return Err("An image file is missing from this runtime's cache.".into());
            }
            lines.push(line.to_owned());
            continue;
        }
        let Some(copy) = cached_copy(path, cache) else {
            // Another cache's file that this runtime has no copy of: keep using it while
            // it exists; if it is gone, the image cannot boot from this runtime.
            if Path::new(path).exists() {
                lines.push(line.to_owned());
                continue;
            }
            return Err("An image file is missing from this runtime's cache.".into());
        };
        let copy = fs::canonicalize(&copy).map_err(|_| "An image file could not be resolved.")?;
        lines.push(line.replacen(
            &format!("\"{path}\""),
            &format!("\"{}\"", copy.display()),
            1,
        ));
        changed = true;
    }
    let mut text = lines.join("\n");
    if descriptor.ends_with('\n') {
        text.push('\n');
    }
    Ok(changed.then_some(text))
}

/// The quoted file of a `RW <sectors> FLAT "<path>" <offset>` extent line.
fn extent_path(line: &str) -> Option<&str> {
    let (access, rest) = line.split_once(' ')?;
    if !matches!(access, "RW" | "RDONLY") {
        return None;
    }
    let (sectors, rest) = rest.split_once(' ')?;
    let rest = rest.strip_prefix("FLAT \"")?;
    let (path, offset) = rest.rsplit_once("\" ")?;
    (sectors.parse::<u64>().is_ok() && offset.parse::<u64>().is_ok()).then_some(path)
}

/// This cache's copy of an image file named by another runtime's absolute path.
fn cached_copy(path: &str, cache: &Path) -> Option<PathBuf> {
    let path = Path::new(path);
    let name = path.file_name()?.to_str()?;
    let kind = path.parent()?.file_name()?.to_str()?;
    let digest = name.strip_prefix("sha256_")?.strip_suffix(".erofs")?;
    let valid = matches!(kind, "fsmeta" | "layers")
        && path.parent()?.parent()?.file_name()? == "cache"
        && digest.len() == 64
        && digest.bytes().all(|byte| byte.is_ascii_hexdigit());
    let copy = cache.join(kind).join(name);
    (valid && fs::symlink_metadata(&copy).is_ok_and(|metadata| metadata.is_file())).then_some(copy)
}

/// A small regular `.vmdk` file: an image descriptor, not a disk's data.
fn is_descriptor(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension == "vmdk")
        && fs::symlink_metadata(path)
            .is_ok_and(|metadata| metadata.is_file() && metadata.len() <= 64 * 1024)
}

/// Whether any image descriptor in `cache` still names a file below `runtime`, so removing
/// that runtime would break the computers that boot from this cache. A descriptor that cannot be
/// read, is redirected, or exceeds the descriptor size limit counts as unverifiable
/// and fails, since it might name such a file.
pub fn reads_from(cache: &Path, runtime: &Path) -> Result<bool, String> {
    let entries = match fs::read_dir(cache.join("vmdk")) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(_) => return Err("Silo could not read the runtime's image cache.".into()),
    };
    let canonical = fs::canonicalize(runtime)
        .map_err(|_| "Silo could not resolve the previous runtime's image storage.")?;
    for entry in entries {
        let path = entry
            .map_err(|_| "Silo could not read the runtime's image cache.")?
            .path();
        if !is_descriptor(&path) {
            if path
                .extension()
                .is_some_and(|extension| extension == "vmdk")
            {
                return Err(
                    "Silo could not verify an image descriptor in the runtime's cache.".into(),
                );
            }
            continue;
        }
        let descriptor = fs::read_to_string(&path)
            .map_err(|_| "Silo could not read an image descriptor in the runtime's cache.")?;
        for extent in descriptor.lines().filter_map(extent_path) {
            let extent = Path::new(extent);
            if extent.starts_with(runtime) || extent.starts_with(&canonical) {
                return Ok(true);
            }
            // Runtime aliases can name the previous generation outside its lexical path.
            let resolved = fs::canonicalize(extent)
                .map_err(|_| "Silo could not resolve an image descriptor's storage file.")?;
            if resolved.starts_with(&canonical) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Point every image VMDK in `cache` at this cache's EROFS files, including files that
/// another runtime still has. Returns how many descriptors changed. A descriptor whose
/// files are missing everywhere is left alone and reported; the others are repaired.
pub fn repair(cache: &Path) -> Result<usize, String> {
    let directory = cache.join("vmdk");
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(_) => return Err("Silo could not read the runtime's image cache.".into()),
    };
    let mut repaired = 0;
    let mut failure = None;
    for entry in entries {
        let path = entry
            .map_err(|_| "Silo could not read the runtime's image cache.")?
            .path();
        if !is_descriptor(&path) {
            continue;
        }
        let descriptor = match fs::read_to_string(&path) {
            Ok(descriptor) => descriptor,
            Err(_) => {
                failure = Some(
                    "Silo could not read an image descriptor in the runtime's cache.".to_string(),
                );
                continue;
            }
        };
        match rebind(&descriptor, cache) {
            Ok(Some(text)) => {
                write_atomically(&path, text.as_bytes())?;
                repaired += 1;
            }
            Ok(None) => {}
            Err(error) => failure = Some(error),
        }
    }
    match failure {
        Some(error) => Err(error),
        None => Ok(repaired),
    }
}

fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let failed = || "Silo could not update an image descriptor.".to_string();
    let directory = path.parent().ok_or_else(failed)?;
    let permissions = fs::metadata(path).map_err(|_| failed())?.permissions();
    let mut file = tempfile::NamedTempFile::new_in(directory).map_err(|_| failed())?;
    file.write_all(bytes).map_err(|_| failed())?;
    file.as_file()
        .set_permissions(permissions)
        .map_err(|_| failed())?;
    file.as_file().sync_all().map_err(|_| failed())?;
    file.persist(path).map_err(|_| failed())?;
    fs::File::open(directory)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| failed())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(unix)]
    fn descriptor_publication_reports_an_unreadable_parent_after_replacement() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let directory = tempfile::tempdir().unwrap();
        if fs::metadata(directory.path()).unwrap().uid() == 0 {
            return; // Root bypasses the permission boundary exercised here.
        }
        let path = directory.path().join("image.vmdk");
        fs::write(&path, b"previous descriptor").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o300)).unwrap();
        let result = write_atomically(&path, b"complete replacement");
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"complete replacement");
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o640);
        assert!(
            result.is_err(),
            "an unsynchronized rename must not report success"
        );
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    const FSMETA: &str =
        "sha256_c3cfb36ddc3996ea4b2ce930868bad3d0138688ac9bcdd6164a78d1adc43b30a.erofs";
    const LAYER: &str =
        "sha256_6078cde548a521a729def2ee7875e9f65513c18f0d4bac4db817417617d7006a.erofs";

    fn descriptor(fsmeta: &str, layer: &str) -> String {
        format!(
            "# Disk DescriptorFile\nversion=1\ncreateType=\"twoGbMaxExtentFlat\"\n\n# Extent description\nRW 2624 FLAT \"{fsmeta}\" 0\nRW 4194304 FLAT \"{layer}\" 0\nRW 12 FLAT \"{layer}\" 4194304\n\n# The Disk Data Base\n#DDB\nddb.adapterType = \"ide\"\n"
        )
    }

    fn cache() -> (tempfile::TempDir, PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let cache = directory
            .path()
            .join("Application Support/runtime-converted/microsandbox/cache");
        for kind in ["fsmeta", "layers", "vmdk"] {
            fs::create_dir_all(cache.join(kind)).unwrap();
        }
        fs::write(cache.join("fsmeta").join(FSMETA), vec![0; 512]).unwrap();
        fs::write(cache.join("layers").join(LAYER), vec![0; 1024]).unwrap();
        (directory, cache)
    }

    #[test]
    fn a_copied_runtime_stops_reading_the_previous_runtimes_image_files() {
        let (_directory, cache) = cache();
        let old = "/home/user/.local/share/org.silo.preview/runtime/microsandbox/cache";
        let vmdk = cache.join("vmdk/sha256_c3cf.vmdk");
        fs::write(
            &vmdk,
            descriptor(
                &format!("{old}/fsmeta/{FSMETA}"),
                &format!("{old}/layers/{LAYER}"),
            ),
        )
        .unwrap();
        assert_eq!(repair(&cache).unwrap(), 1);
        let current = fs::canonicalize(&cache).unwrap();
        let expected = descriptor(
            &current.join("fsmeta").join(FSMETA).display().to_string(),
            &current.join("layers").join(LAYER).display().to_string(),
        );
        assert_eq!(fs::read_to_string(&vmdk).unwrap(), expected);
        // Repaired descriptors are left alone.
        assert_eq!(repair(&cache).unwrap(), 0);
        assert_eq!(fs::read_to_string(&vmdk).unwrap(), expected);
    }

    #[test]
    fn only_image_files_present_in_this_cache_are_rebound() {
        let (_directory, cache) = cache();
        let old = "/gone/cache";
        let missing = format!("{old}/layers/sha256_{}.erofs", "0".repeat(64));
        let vmdk = cache.join("vmdk/other.vmdk");
        let text = descriptor(&format!("{old}/fsmeta/{FSMETA}"), &missing);
        fs::write(&vmdk, &text).unwrap();
        assert!(repair(&cache).unwrap_err().contains("missing"));
        // A file another runtime still has, without a copy here, stays in use.
        let elsewhere = tempfile::tempdir().unwrap();
        let kept = elsewhere.path().join("cache/layers");
        fs::create_dir_all(&kept).unwrap();
        let kept = kept.join(format!("sha256_{}.erofs", "1".repeat(64)));
        fs::write(&kept, vec![0; 512]).unwrap();
        let text = descriptor(
            &format!("{old}/fsmeta/{FSMETA}"),
            &kept.display().to_string(),
        );
        fs::write(&vmdk, &text).unwrap();
        assert_eq!(repair(&cache).unwrap(), 1);
        let repaired = fs::read_to_string(&vmdk).unwrap();
        assert!(repaired.contains(&kept.display().to_string()));
        assert!(!repaired.contains(old));
        fs::write(
            &vmdk,
            descriptor(&format!("{old}/fsmeta/{FSMETA}"), &missing),
        )
        .unwrap();
        for path in [
            "/gone/cache/other/x.erofs",
            "/gone/fsmeta/sha256_zz.erofs",
            "/gone/cache/fsmeta/../../etc",
        ] {
            assert_eq!(cached_copy(path, &cache), None, "{path}");
        }
        assert_eq!(repair(&cache.join("absent")).unwrap(), 0);
    }

    #[test]
    fn missing_files_in_the_current_cache_are_reported_without_rewriting_the_descriptor() {
        let (_directory, cache) = cache();
        let current = fs::canonicalize(&cache).unwrap();
        let fsmeta = current.join("fsmeta").join(FSMETA);
        let layer = current.join("layers").join(LAYER);
        let vmdk = cache.join("vmdk/current.vmdk");
        let text = descriptor(&fsmeta.display().to_string(), &layer.display().to_string());
        fs::write(&vmdk, &text).unwrap();
        fs::remove_file(&layer).unwrap();

        assert!(repair(&cache).unwrap_err().contains("missing"));
        assert_eq!(fs::read_to_string(&vmdk).unwrap(), text);
        fs::write(&layer, [0; 512]).unwrap();
        assert_eq!(repair(&cache).unwrap(), 0);
    }

    #[test]
    fn repairing_another_descriptor_does_not_hide_missing_extents() {
        let (_directory, cache) = cache();
        let old = "/gone/cache";
        let repaired = cache.join("vmdk/repairable.vmdk");
        fs::write(
            &repaired,
            descriptor(
                &format!("{old}/fsmeta/{FSMETA}"),
                &format!("{old}/layers/{LAYER}"),
            ),
        )
        .unwrap();
        let broken = cache.join("vmdk/broken.vmdk");
        let missing = format!("{old}/layers/sha256_{}.erofs", "0".repeat(64));
        let broken_text = descriptor(&format!("{old}/fsmeta/{FSMETA}"), &missing);
        fs::write(&broken, &broken_text).unwrap();

        let result = repair(&cache);
        assert!(!fs::read_to_string(&repaired).unwrap().contains(old));
        assert_eq!(fs::read_to_string(&broken).unwrap(), broken_text);
        assert!(result.unwrap_err().contains("missing"));
    }

    #[test]
    fn unreadable_descriptors_are_reported_without_stopping_other_repairs() {
        let (_directory, cache) = cache();
        fs::write(cache.join("vmdk/unreadable.vmdk"), [0xff, 0xfe]).unwrap();
        let repaired = cache.join("vmdk/repairable.vmdk");
        fs::write(
            &repaired,
            descriptor(
                &format!("/gone/cache/fsmeta/{FSMETA}"),
                &format!("/gone/cache/layers/{LAYER}"),
            ),
        )
        .unwrap();

        let result = repair(&cache);
        assert!(!fs::read_to_string(&repaired).unwrap().contains("/gone"));
        assert!(result.unwrap_err().contains("read an image descriptor"));
        // A cache with only the unreadable descriptor must report it as well.
        fs::remove_file(&repaired).unwrap();
        assert!(repair(&cache)
            .unwrap_err()
            .contains("read an image descriptor"));
    }

    #[test]
    fn a_descriptor_naming_a_file_below_a_runtime_is_reported() {
        let (directory, cache) = cache();
        let previous = directory.path().join("Application Support/runtime");
        fs::create_dir_all(&previous).unwrap();
        let inside = previous.join("microsandbox/cache/layers").join(LAYER);
        let own = cache.join("layers").join(LAYER);
        let vmdk = cache.join("vmdk/x.vmdk");
        let text =
            |path: &Path| descriptor(&path.display().to_string(), &path.display().to_string());
        assert!(!reads_from(&cache.join("absent"), &previous).unwrap());
        fs::write(&vmdk, text(&inside)).unwrap();
        assert!(reads_from(&cache, &previous).unwrap());
        // The converted runtime's own files, in a sibling folder with the same prefix, are not below it.
        fs::write(&vmdk, text(&own)).unwrap();
        assert!(!reads_from(&cache, &previous).unwrap());
        // Repair is what removes the dependency for a copied cache.
        fs::write(&vmdk, text(&inside)).unwrap();
        assert_eq!(repair(&cache).unwrap(), 1);
        assert!(!reads_from(&cache, &previous).unwrap());
        // A descriptor that cannot be read cannot be verified.
        fs::write(&vmdk, [0xff, 0xfe, 0xfd]).unwrap();
        assert!(reads_from(&cache, &previous).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_dependency_through_the_previous_runtime_alias_blocks_removal() {
        let (directory, cache) = cache();
        let previous = directory.path().join("runtime");
        let old_cache = previous.join("microsandbox/cache");
        fs::create_dir_all(old_cache.join("layers")).unwrap();
        let name = format!("sha256_{}.erofs", "b".repeat(64));
        fs::write(old_cache.join("layers").join(&name), [0; 512]).unwrap();
        let alias = directory.path().join("runtime-alias");
        std::os::unix::fs::symlink(previous.join("microsandbox"), &alias).unwrap();
        let extent = alias.join("cache/layers").join(&name);
        let vmdk = cache.join("vmdk/aliased.vmdk");
        let text = format!("RW 1 FLAT \"{}\" 0\n", extent.display());
        fs::write(&vmdk, &text).unwrap();

        // The missing local copy leaves the existing external extent in use.
        assert_eq!(repair(&cache).unwrap(), 0);
        assert_eq!(fs::read_to_string(&vmdk).unwrap(), text);
        assert!(reads_from(&cache, &previous).unwrap());

        // Once rebound to a local copy, removing the previous runtime is safe.
        fs::write(cache.join("layers").join(&name), [0; 512]).unwrap();
        assert_eq!(repair(&cache).unwrap(), 1);
        assert!(!reads_from(&cache, &previous).unwrap());
    }

    #[test]
    fn an_unresolved_extent_cannot_prove_independence_from_the_previous_runtime() {
        let (directory, cache) = cache();
        let previous = directory.path().join("runtime");
        fs::create_dir_all(&previous).unwrap();
        let missing = directory.path().join("missing.erofs");
        fs::write(
            cache.join("vmdk/missing.vmdk"),
            format!("RW 1 FLAT \"{}\" 0\n", missing.display()),
        )
        .unwrap();
        assert!(reads_from(&cache, &previous).is_err());
    }

    #[test]
    fn an_oversized_descriptor_cannot_prove_independence() {
        let (directory, cache) = cache();
        let previous = directory.path().join("runtime");
        fs::create_dir_all(&previous).unwrap();
        let extent = previous.join("image.erofs");
        fs::write(&extent, [0; 512]).unwrap();
        let text = format!(
            "{}RW 1 FLAT \"{}\" 0\n",
            "# comment\n".repeat(7_000),
            extent.display()
        );
        assert!(text.len() > 64 * 1024);
        fs::write(cache.join("vmdk/large.vmdk"), text).unwrap();
        assert!(reads_from(&cache, &previous).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_redirected_descriptor_cannot_prove_independence() {
        let (directory, cache) = cache();
        let previous = directory.path().join("runtime");
        fs::create_dir_all(&previous).unwrap();
        let descriptor = directory.path().join("external.vmdk");
        fs::write(
            &descriptor,
            format!(
                "RW 1 FLAT \"{}\" 0\n",
                previous.join("image.erofs").display()
            ),
        )
        .unwrap();
        std::os::unix::fs::symlink(&descriptor, cache.join("vmdk/redirected.vmdk")).unwrap();
        assert!(reads_from(&cache, &previous).is_err());
    }

    #[test]
    fn extent_lines_are_recognised_with_spaces_in_paths() {
        assert_eq!(
            extent_path("RW 12 FLAT \"/a b/c.erofs\" 0"),
            Some("/a b/c.erofs")
        );
        assert_eq!(extent_path("RDONLY 1 FLAT \"/x\" 4194304"), Some("/x"));
        for line in [
            "ddb.adapterType = \"ide\"",
            "RW x FLAT \"/x\" 0",
            "RW 1 SPARSE \"/x\" 0",
            "",
        ] {
            assert_eq!(extent_path(line), None, "{line}");
        }
    }
}

#[cfg(test)]
mod copied_runtime_tests {
    use super::*;

    #[test]
    fn a_copy_stops_using_the_previous_runtime_even_while_it_still_exists() {
        let root = tempfile::tempdir().unwrap();
        let name = format!("sha256_{}.erofs", "a".repeat(64));
        let mut caches = Vec::new();
        for generation in ["runtime", "runtime-converted"] {
            let cache = root.path().join(generation).join("microsandbox/cache");
            for kind in ["fsmeta", "layers", "vmdk"] {
                fs::create_dir_all(cache.join(kind)).unwrap();
            }
            fs::write(cache.join("fsmeta").join(&name), vec![0; 512]).unwrap();
            caches.push(cache);
        }
        let old = fs::canonicalize(&caches[0])
            .unwrap()
            .join("fsmeta")
            .join(&name);
        let vmdk = caches[1].join("vmdk/image.vmdk");
        fs::write(&vmdk, format!("RW 1 FLAT \"{}\" 0\n", old.display())).unwrap();
        assert_eq!(repair(&caches[1]).unwrap(), 1);
        let current = fs::canonicalize(&caches[1])
            .unwrap()
            .join("fsmeta")
            .join(&name);
        assert_eq!(
            fs::read_to_string(&vmdk).unwrap(),
            format!("RW 1 FLAT \"{}\" 0\n", current.display())
        );
    }
}
