//! Durability flushes: the one place product code waits for a file or directory to reach the
//! storage device.
//!
//! Every write that must survive a power loss calls [`DurableFile::sync_to_disk`] where it would
//! call `File::sync_all`, on files and directories alike. Shipped builds always call `File::sync_all` (on macOS an
//! `F_FULLFSYNC`). Test builds (`cfg(test)` or the `test-support` feature, which only
//! dev-dependencies enable) skip the wait for the device: the writes, renames and their order are
//! unchanged, so crash and journal tests still see the same files, but concurrent test runs do not
//! queue on the machine-wide disk flush.

use std::fs::File;
use std::io;

/// Flushes a file's data and metadata to the storage device.
pub trait DurableFile {
    /// `File::sync_all`, skipped in test builds.
    fn sync_to_disk(&self) -> io::Result<()>;
}

impl DurableFile for File {
    #[cfg(not(any(test, feature = "test-support")))]
    #[inline]
    fn sync_to_disk(&self) -> io::Result<()> {
        self.sync_all()
    }

    #[cfg(any(test, feature = "test-support"))]
    #[inline]
    fn sync_to_disk(&self) -> io::Result<()> {
        Ok(())
    }
}

/// The source scan behind each crate's check that it flushes only through this module.
#[cfg(any(test, feature = "test-support"))]
pub mod guard {
    use std::fs;
    use std::path::{Path, PathBuf};

    /// Method names that wait for the storage device. Built from parts so this file does not
    /// match its own scan.
    pub(super) fn flush_calls() -> [String; 2] {
        [
            format!(".{}{}(", "sync_", "all"),
            format!(".{}{}(", "sync_", "data"),
        ]
    }

    fn rust_files(directory: &Path, found: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                rust_files(&path, found);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                found.push(path);
            }
        }
    }

    /// Lists `file:line` for every direct flush call in the Rust files under `source`, except
    /// `helper`.
    pub fn direct_flushes(source: &Path, helper: &Path) -> Vec<String> {
        let mut files = Vec::new();
        rust_files(source, &mut files);
        files.sort();
        let calls = flush_calls();
        let mut found = Vec::new();
        for file in files.iter().filter(|file| file.as_path() != helper) {
            let text = fs::read_to_string(file).unwrap();
            for (index, line) in text.lines().enumerate() {
                if calls.iter().any(|call| line.contains(call.as_str())) {
                    found.push(format!("{}:{}", file.display(), index + 1));
                }
            }
        }
        found
    }

    /// Fails with the list of direct flush calls in the crate whose `src` is `source`.
    pub fn assert_no_direct_flushes(source: &Path, helper: &Path) {
        let found = direct_flushes(source, helper);
        assert!(
            found.is_empty(),
            "call silo_engine::durable::DurableFile::sync_to_disk instead of File::sync_all or \
             File::sync_data, so test builds can skip the device flush:\n{}",
            found.join("\n")
        );
    }
}

#[cfg(test)]
mod tests {
    use super::guard::{assert_no_direct_flushes, direct_flushes, flush_calls};
    use std::fs;
    use std::path::Path;

    #[test]
    fn product_code_flushes_only_through_the_helper() {
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        assert_no_direct_flushes(&source, &source.join("durable.rs"));
    }

    #[test]
    fn the_scan_finds_a_direct_flush() {
        let directory = tempfile::tempdir().unwrap();
        let helper = directory.path().join("durable.rs");
        fs::write(
            &helper,
            format!("fn f(file: &File) {{ file{}) }}\n", flush_calls()[0]),
        )
        .unwrap();
        fs::create_dir(directory.path().join("nested")).unwrap();
        fs::write(
            directory.path().join("nested/store.rs"),
            format!(
                "fn ok() {{}}\nfn f(file: &File) {{ file{}) }}\n",
                flush_calls()[1]
            ),
        )
        .unwrap();
        let found = direct_flushes(directory.path(), &helper);
        assert_eq!(
            found,
            [format!(
                "{}:2",
                directory.path().join("nested/store.rs").display()
            )]
        );
    }
}
