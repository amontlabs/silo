//! Where captured manifests are kept: per computer, the fork-time base and the latest
//! capture, and per source checkpoint, the manifests that make up its no-op baseline. All of
//! them are gzipped text under the engine's data directory.

use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::context::EngineContext;

/// Which of a computer's two manifests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Base,
    Latest,
}

impl Slot {
    fn name(self) -> &'static str {
        match self {
            Slot::Base => "base",
            Slot::Latest => "latest",
        }
    }
}

/// What is recorded beside a stored manifest.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Meta {
    pub captured_at: String,
    /// The checkpoint (or computer) a fork was made from; set on a base capture.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    pub records: u64,
    pub bytes: u64,
}

#[derive(Clone, Debug)]
pub struct Store {
    root: PathBuf,
}

pub fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

/// Seconds since the Unix epoch of an RFC 3339 time.
pub fn unix_seconds(text: &str) -> Option<i64> {
    time::OffsetDateTime::parse(text, &time::format_description::well_known::Rfc3339)
        .ok()
        .map(|time| time.unix_timestamp())
}

impl Store {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The store under an engine's data directory, laid out as its job registry is.
    pub fn for_engine(context: &dyn EngineContext) -> Self {
        let paths = context.paths();
        let engine_data = if paths.home.as_os_str().is_empty() {
            paths.app_data.join("engine")
        } else {
            paths
                .home
                .join(context.identity().channel.state_dir_name())
                .join("engine")
        };
        Self::new(engine_data.join("changes"))
    }

    fn computer_dir(&self, computer: &str) -> Result<PathBuf, String> {
        if computer.is_empty()
            || !computer
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err("The computer id is not usable as a folder name.".to_owned());
        }
        Ok(self.root.join("computers").join(computer))
    }

    fn baseline_dir(&self, source: &str) -> PathBuf {
        let key = Sha256::digest(source.as_bytes());
        let name: String = key.iter().take(16).map(|b| format!("{b:02x}")).collect();
        self.root.join("baselines").join(name)
    }

    pub fn put(
        &self,
        computer: &str,
        slot: Slot,
        manifest: &str,
        source: Option<&str>,
    ) -> Result<Meta, String> {
        let directory = self.computer_dir(computer)?;
        let meta = Meta {
            captured_at: now(),
            source: source.map(str::to_owned),
            records: manifest.lines().count() as u64,
            bytes: manifest.len() as u64,
        };
        write_gz(&directory.join(format!("{}.tsv.gz", slot.name())), manifest)?;
        write_atomic(
            &directory.join(format!("{}.json", slot.name())),
            &serde_json::to_vec(&meta).map_err(|e| e.to_string())?,
        )?;
        Ok(meta)
    }

    pub fn get(&self, computer: &str, slot: Slot) -> Result<Option<(String, Meta)>, String> {
        let directory = self.computer_dir(computer)?;
        let Some(manifest) = read_gz(&directory.join(format!("{}.tsv.gz", slot.name())))? else {
            return Ok(None);
        };
        let meta = fs::read(directory.join(format!("{}.json", slot.name())))
            .map_err(|e| e.to_string())
            .and_then(|bytes| serde_json::from_slice(&bytes).map_err(|e| e.to_string()))?;
        Ok(Some((manifest, meta)))
    }

    /// Caches the manifests of an idle fork under its source: the base it was forked from and
    /// the capture taken after it sat idle.
    pub fn put_baseline(&self, source: &str, base: &str, idle: &str) -> Result<Meta, String> {
        let directory = self.baseline_dir(source);
        let meta = Meta {
            captured_at: now(),
            source: Some(source.to_owned()),
            records: idle.lines().count() as u64,
            bytes: idle.len() as u64,
        };
        write_gz(&directory.join("base.tsv.gz"), base)?;
        write_gz(&directory.join("idle.tsv.gz"), idle)?;
        write_atomic(
            &directory.join("meta.json"),
            &serde_json::to_vec(&meta).map_err(|e| e.to_string())?,
        )?;
        Ok(meta)
    }

    /// The cached `(base, idle)` manifests of `source`.
    pub fn get_baseline(&self, source: &str) -> Result<Option<(String, String)>, String> {
        let directory = self.baseline_dir(source);
        // The meta file is written last, so its presence means both manifests are complete.
        if !directory.join("meta.json").is_file() {
            return Ok(None);
        }
        let base = read_gz(&directory.join("base.tsv.gz"))?;
        let idle = read_gz(&directory.join("idle.tsv.gz"))?;
        Ok(base.zip(idle))
    }

    pub fn remove_computer(&self, computer: &str) -> Result<(), String> {
        match fs::remove_dir_all(self.computer_dir(computer)?) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.to_string()),
            _ => Ok(()),
        }
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("The store path has no folder.")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut staged = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    staged.write_all(bytes).map_err(|e| e.to_string())?;
    staged.persist(path).map_err(|e| e.error.to_string())?;
    Ok(())
}

fn write_gz(path: &Path, text: &str) -> Result<(), String> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
    write_atomic(path, &encoder.finish().map_err(|e| e.to_string())?)
}

fn read_gz(path: &Path) -> Result<Option<String>, String> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let mut text = String::new();
    flate2::read::GzDecoder::new(bytes.as_slice())
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    Ok(Some(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_and_latest_are_kept_apart_per_computer() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        assert_eq!(store.get("c1", Slot::Base).unwrap(), None);
        let meta = store
            .put("c1", Slot::Base, "META\tformat\t2\nF\t/a\t1\t1\t-\n", Some("cp1"))
            .unwrap();
        assert_eq!((meta.records, meta.source.as_deref()), (2, Some("cp1")));
        store.put("c1", Slot::Latest, "later\n", None).unwrap();
        store.put("c2", Slot::Base, "other\n", None).unwrap();
        let (text, read) = store.get("c1", Slot::Base).unwrap().unwrap();
        assert_eq!(text, "META\tformat\t2\nF\t/a\t1\t1\t-\n");
        assert_eq!(read, meta);
        assert_eq!(store.get("c1", Slot::Latest).unwrap().unwrap().0, "later\n");
        assert_eq!(store.get("c2", Slot::Base).unwrap().unwrap().0, "other\n");
        // Stored gzipped.
        let raw = std::fs::read(dir.path().join("computers/c1/base.tsv.gz")).unwrap();
        assert_eq!(&raw[..2], &[0x1f, 0x8b]);
        store.remove_computer("c1").unwrap();
        assert_eq!(store.get("c1", Slot::Base).unwrap(), None);
        assert!(store.get("c2", Slot::Base).unwrap().is_some());
    }

    #[test]
    fn a_replacing_capture_leaves_no_partial_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        store.put("c", Slot::Latest, "one\n", None).unwrap();
        store.put("c", Slot::Latest, "two\n", None).unwrap();
        assert_eq!(store.get("c", Slot::Latest).unwrap().unwrap().0, "two\n");
        let names: Vec<_> = std::fs::read_dir(dir.path().join("computers/c"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.len(), 2, "{names:?}");
    }

    #[test]
    fn computer_ids_cannot_leave_the_store() {
        let store = Store::new("/nonexistent");
        assert!(store.get("../x", Slot::Base).is_err());
        assert!(store.put("a/b", Slot::Base, "", None).is_err());
        assert!(store.get("", Slot::Base).is_err());
    }

    #[test]
    fn baselines_are_cached_per_source() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        assert_eq!(store.get_baseline("cp1").unwrap(), None);
        store.put_baseline("cp1", "base\n", "idle\n").unwrap();
        assert_eq!(
            store.get_baseline("cp1").unwrap(),
            Some(("base\n".to_owned(), "idle\n".to_owned()))
        );
        assert_eq!(store.get_baseline("cp2").unwrap(), None);
        // A path-like key stays inside the store.
        store.put_baseline("../../escape", "b\n", "i\n").unwrap();
        assert!(store.get_baseline("../../escape").unwrap().is_some());
    }

    #[test]
    fn times_round_trip() {
        let text = now();
        let seconds = unix_seconds(&text).unwrap();
        assert!((seconds - time::OffsetDateTime::now_utc().unix_timestamp()).abs() < 5);
        assert_eq!(unix_seconds("nonsense"), None);
    }
}
