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

/// The baseline key of a computer that forks are made from (never a checkpoint id).
fn origin_key(computer: &str) -> String {
    format!("computer:{computer}")
}

/// What is recorded beside a stored manifest.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Meta {
    pub captured_at: String,
    /// The checkpoint (or computer) a fork was made from; set on a base capture.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// The computer a fork was made from, set on a base capture.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    pub records: u64,
    pub bytes: u64,
}

/// What the fork-time record says about where a fork came from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ForkLink {
    /// The computer it was made from.
    pub origin: Option<String>,
    /// The checkpoint it was made from (a `recipe:` key for a recipe fork; the computer
    /// itself while the fork's own checkpoint is not yet known).
    pub source: Option<String>,
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

    /// A folder beside this store under the engine's data directory (the step records live
    /// in one).
    pub fn sibling_dir(&self, name: &str) -> PathBuf {
        self.root.parent().unwrap_or(&self.root).join(name)
    }

    /// Where a computer was forked from, read from the fork-time record (`id`) once the fork
    /// has started, and from the note made when it was created (`name`) until then.
    pub fn fork_link(&self, id: &str, name: &str) -> Option<ForkLink> {
        let recorded = self
            .computer_dir(id)
            .ok()
            .and_then(|directory| fs::read(directory.join("base.json")).ok())
            .and_then(|bytes| serde_json::from_slice::<Meta>(&bytes).ok());
        if let Some(meta) = recorded {
            if meta.source.is_some() || meta.origin.is_some() {
                return Some(ForkLink {
                    origin: meta.origin,
                    source: meta.source,
                });
            }
        }
        let source = fs::read_to_string(self.pending_path(name)).ok()?;
        Some(ForkLink {
            origin: fs::read_to_string(self.origin_path(name)).ok(),
            source: Some(source),
        })
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
        self.put_with_origin(computer, slot, manifest, source, None)
    }

    pub fn put_with_origin(
        &self,
        computer: &str,
        slot: Slot,
        manifest: &str,
        source: Option<&str>,
        origin: Option<&str>,
    ) -> Result<Meta, String> {
        let directory = self.computer_dir(computer)?;
        let meta = Meta {
            captured_at: now(),
            source: source.map(str::to_owned),
            origin: origin.map(str::to_owned),
            records: manifest.lines().count() as u64,
            bytes: manifest.len() as u64,
        };
        if slot == Slot::Base {
            let _ = fs::remove_file(directory.join("base-error.txt"));
        }
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
            origin: None,
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

    /// Like [`Store::put_baseline`], and also under the computer the fork was made from, so
    /// the other forks of that computer (each has a checkpoint of its own) find it too.
    pub fn put_baseline_for(
        &self,
        source: &str,
        origin: Option<&str>,
        base: &str,
        idle: &str,
    ) -> Result<Meta, String> {
        let meta = self.put_baseline(source, base, idle)?;
        if let Some(origin) = origin {
            self.put_baseline(&origin_key(origin), base, idle)?;
        }
        Ok(meta)
    }

    /// The cached baseline of `source`; when there is none, the one measured on another fork
    /// of `origin`, which is then kept under `source` as well.
    pub fn get_baseline_for(
        &self,
        source: &str,
        origin: Option<&str>,
    ) -> Result<Option<(String, String)>, String> {
        if let Some(found) = self.get_baseline(source)? {
            return Ok(Some(found));
        }
        let Some(origin) = origin else {
            return Ok(None);
        };
        let Some((base, idle)) = self.get_baseline(&origin_key(origin))? else {
            return Ok(None);
        };
        self.put_baseline(source, &base, &idle)?;
        Ok(Some((base, idle)))
    }

    fn pending_path(&self, name: &str) -> PathBuf {
        let key = Sha256::digest(name.as_bytes());
        let file: String = key.iter().take(16).map(|b| format!("{b:02x}")).collect();
        self.root.join("pending").join(format!("{file}.json"))
    }

    /// Notes that the computer about to be created as `name` is a fork of `source` and wants
    /// its fork-time base captured at its first start.
    pub fn put_pending(&self, name: &str, source: &str) -> Result<(), String> {
        write_atomic(&self.pending_path(name), source.as_bytes())
    }

    fn origin_path(&self, name: &str) -> PathBuf {
        self.pending_path(name).with_extension("origin")
    }

    /// Notes the computer the fork `name` is being made from.
    pub fn put_pending_origin(&self, name: &str, computer: &str) -> Result<(), String> {
        write_atomic(&self.origin_path(name), computer.as_bytes())
    }

    pub fn take_pending_origin(&self, name: &str) -> Option<String> {
        let path = self.origin_path(name);
        let computer = fs::read_to_string(&path).ok()?;
        let _ = fs::remove_file(&path);
        Some(computer)
    }

    /// The source noted for `name`, removed so that only one start captures it.
    pub fn take_pending(&self, name: &str) -> Option<String> {
        let path = self.pending_path(name);
        let source = fs::read_to_string(&path).ok()?;
        fs::remove_file(&path).ok()?;
        Some(source)
    }

    /// Records why a computer's fork-time base could not be taken.
    pub fn put_failure(&self, computer: &str, message: &str) -> Result<(), String> {
        write_atomic(
            &self.computer_dir(computer)?.join("base-error.txt"),
            message.as_bytes(),
        )
    }

    pub fn get_failure(&self, computer: &str) -> Option<String> {
        fs::read_to_string(self.computer_dir(computer).ok()?.join("base-error.txt")).ok()
    }

    pub fn remove_computer(&self, computer: &str) -> Result<(), String> {
        match fs::remove_dir_all(self.computer_dir(computer)?) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.to_string()),
            _ => Ok(()),
        }
    }
}

/// Drops everything kept for computers that no longer exist.
pub fn forget(context: &dyn EngineContext, computers: &[String]) {
    let store = Store::for_engine(context);
    let steps = crate::steps::StepStore::for_engine(context);
    for id in computers {
        let _ = store.remove_computer(id);
        let _ = steps.remove_computer(id);
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
    encoder
        .write_all(text.as_bytes())
        .map_err(|e| e.to_string())?;
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
            .put(
                "c1",
                Slot::Base,
                "META\tformat\t2\nF\t/a\t1\t1\t-\n",
                Some("cp1"),
            )
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
    fn pending_forks_are_taken_once_and_failures_clear_on_a_new_base() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        assert_eq!(store.take_pending("try"), None);
        store.put_pending("try", "cp1").unwrap();
        assert_eq!(store.take_pending("try").as_deref(), Some("cp1"));
        assert_eq!(store.take_pending("try"), None);
        store.put_failure("c1", "no ssh").unwrap();
        assert_eq!(store.get_failure("c1").as_deref(), Some("no ssh"));
        store.put("c1", Slot::Base, "x\n", None).unwrap();
        assert_eq!(store.get_failure("c1"), None);
    }

    #[test]
    fn a_forks_origin_computer_is_noted_and_recorded_with_its_base() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        store.put_pending_origin("try", "main").unwrap();
        assert_eq!(store.take_pending_origin("try").as_deref(), Some("main"));
        assert_eq!(store.take_pending_origin("try"), None);
        let meta = store
            .put_with_origin("c1", Slot::Base, "x\n", Some("cp1"), Some("main"))
            .unwrap();
        assert_eq!(meta.origin.as_deref(), Some("main"));
        let (_, read) = store.get("c1", Slot::Base).unwrap().unwrap();
        assert_eq!(
            (read.source.as_deref(), read.origin.as_deref()),
            (Some("cp1"), Some("main"))
        );
    }

    #[test]
    fn deleting_computers_drops_what_was_kept_for_them() {
        let dir = tempfile::tempdir().unwrap();
        let context = crate::guest::test_support::RecordingContext::new(dir.path());
        let store = Store::for_engine(context.as_ref());
        store.put("gone", Slot::Base, "a\n", None).unwrap();
        store.put("gone", Slot::Latest, "b\n", None).unwrap();
        store.put_failure("gone", "why").unwrap();
        store.put("kept", Slot::Base, "c\n", None).unwrap();
        forget(
            context.as_ref(),
            &["gone".to_owned(), "never-existed".to_owned()],
        );
        assert_eq!(store.get("gone", Slot::Base).unwrap(), None);
        assert_eq!(store.get("gone", Slot::Latest).unwrap(), None);
        assert_eq!(store.get_failure("gone"), None);
        assert!(store.get("kept", Slot::Base).unwrap().is_some());
    }

    #[test]
    fn times_round_trip() {
        let text = now();
        let seconds = unix_seconds(&text).unwrap();
        assert!((seconds - time::OffsetDateTime::now_utc().unix_timestamp()).abs() < 5);
        assert_eq!(unix_seconds("nonsense"), None);
    }
}
