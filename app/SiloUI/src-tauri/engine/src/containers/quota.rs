//! Holding a container computer to its disk size (`root_disk_mib`).
//!
//! The kernel's tools for a size limit on a directory (a project quota on XFS or ext4, a loop-mounted
//! file system) need administrator rights to set up, and neither a user namespace nor FUSE-less
//! user space can mount a block file system, so a rootless host has no way to make the kernel
//! answer a write with "no space left". The engine measures the computer's writable layer instead
//! and, past the limit, makes the container's root file system read-only
//! (`silo-ctr remount --ro`); writes then fail with "Read-only file system" until space is freed
//! (stop the computer, free space, start it again) and the layer is below the limit.
//!
//! The limit counts the writable layer (and so `/workspace`, which is a folder in it). Sealed
//! layers belong to checkpoints and forks and are not counted against a computer.

use std::path::Path;

/// The most entries one measurement visits, so a layer of millions of files cannot stall the watcher.
const MAX_ENTRIES: usize = 4_000_000;

/// Bytes the files below `path` occupy on disk (allocated blocks, links not followed).
pub fn used_bytes(path: &Path) -> u64 {
    use std::os::unix::fs::MetadataExt;
    let mut total = 0u64;
    let mut visited = 0usize;
    let mut stack = vec![path.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            visited += 1;
            if visited > MAX_ENTRIES {
                return total;
            }
            let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
                continue;
            };
            total = total.saturating_add(meta.blocks().saturating_mul(512));
            if meta.is_dir() {
                stack.push(entry.path());
            }
        }
    }
    total
}

#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    Nothing,
    MakeReadOnly,
    MakeWritable,
}

/// What to do about a layer of `used` bytes under a limit of `limit_mib`. Read-only is lifted
/// only once the layer is 5 % under the limit, so a computer at the edge does not flip back and
/// forth.
pub fn decide(used: u64, limit_mib: u64, read_only: bool) -> Action {
    let limit = limit_mib.saturating_mul(1 << 20);
    match (read_only, used > limit) {
        (false, true) => Action::MakeReadOnly,
        (true, false) if used < limit - limit / 20 => Action::MakeWritable,
        _ => Action::Nothing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_layer_is_measured_in_allocated_blocks() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(used_bytes(dir.path()), 0);
        std::fs::create_dir(dir.path().join("a")).unwrap();
        std::fs::write(dir.path().join("a/f"), vec![1u8; 100_000]).unwrap();
        std::os::unix::fs::symlink("/", dir.path().join("link")).unwrap();
        let used = used_bytes(dir.path());
        assert!((100_000..200_000).contains(&used), "{used}");
    }

    #[test]
    fn a_computer_goes_read_only_past_its_limit_and_back_a_little_below_it() {
        let mib = 1u64 << 20;
        assert_eq!(decide(10 * mib, 100, false), Action::Nothing);
        assert_eq!(decide(100 * mib, 100, false), Action::Nothing);
        assert_eq!(decide(101 * mib, 100, false), Action::MakeReadOnly);
        assert_eq!(decide(101 * mib, 100, true), Action::Nothing);
        assert_eq!(decide(99 * mib, 100, true), Action::Nothing);
        assert_eq!(decide(94 * mib, 100, true), Action::MakeWritable);
        assert_eq!(decide(0, 0, true), Action::Nothing);
    }
}
