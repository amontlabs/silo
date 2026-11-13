//! The shared layer store and the checkpoints of container computers.
//!
//! A computer's file system is an overlay: its writable layer over a chain of sealed layers, the
//! shim layer and the image. Every layer is a directory of the store (`containers/layers/<id>`),
//! so a checkpoint is a list of ids and nothing is copied for a stopped computer:
//!
//! - **Seal.** A stopped computer's checkpoint adds its writable layer's id to its chain and opens a
//!   new empty writable layer. No directory owned by a sub-id moves.
//! - **Running computer.** An overlay cannot change its writable layer under a running container,
//!   so the checkpoint is a copy of the writable layer, made while the container's control group is
//!   frozen (a reflink copy where the file system has them). The computer keeps its layer.
//! - **Fork.** A new computer whose chain is the checkpoint's, with a writable layer of its own and
//!   its own record of those ids, so deleting the parent (or the checkpoint) keeps them.
//! - **Collection.** A layer no computer record and no checkpoint names is removed, unless it is
//!   being made ([`reserve`]). Nothing is removed when a record, a checkpoint or one of their
//!   folders could not be read.
//! - **Flatten.** A chain longer than [`MAX_CHAIN`] has its oldest layers folded into one layer
//!   (`silo-ctr merge`) when the computer is not running.
//!
//! A checkpoint is `containers/snapshots/<group>/<name>.json`; the engine's checkpoint records
//! name it `group:name` as they name MicroSandbox's snapshots, and [`Snapshot`] is listed in the
//! shapes of `snapshot list`.

use super::layout::{ComputerRecord, Layout};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

/// Sealed layers a computer keeps before the oldest ones are flattened. The overlay is mounted
/// with one option string (a page of the kernel's mount data), and each path is about a hundred
/// bytes, so the chain stays well inside it.
pub const MAX_CHAIN: usize = 16;
/// How many of the oldest layers a flatten folds into one.
pub const FLATTEN_COUNT: usize = 8;
/// The most the overlay's option string may hold before a start refuses it.
pub const MAX_LOWER_BYTES: usize = 3400;
/// A layer younger than this is never collected: it may be on its way into a record.
const GRACE: Duration = Duration::from_secs(10 * 60);

/// Held by everything that makes, references or collects layers.
static STORE: Mutex<()> = Mutex::new(());

pub fn lock() -> std::sync::MutexGuard<'static, ()> {
    STORE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The layers being filled before a record or a checkpoint names them.
static RESERVED: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn reserved() -> std::sync::MutexGuard<'static, Vec<String>> {
    RESERVED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A layer that collection leaves alone until this is dropped.
pub struct Reserved(String);

impl Drop for Reserved {
    fn drop(&mut self) {
        let mut ids = reserved();
        if let Some(index) = ids.iter().position(|id| *id == self.0) {
            ids.swap_remove(index);
        }
    }
}

/// Keeps the layer `id` from collection while it is filled outside the store's lock (the copy of
/// a running computer's layer, a fold). Its age does not protect it: a copy takes the times of
/// its source, so a layer written a moment ago can look older than the grace period. Reserve
/// before the layer is made and drop the reservation after the record or checkpoint that names
/// it is saved.
pub fn reserve(id: &str) -> Reserved {
    reserved().push(id.to_owned());
    Reserved(id.to_owned())
}

pub fn new_id() -> String {
    format!("l{}", &uuid::Uuid::new_v4().simple().to_string()[..16])
}

/// A name that is safe to join onto a store path.
pub fn valid_name(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 64
        && !text.starts_with('.')
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// One checkpoint of a container computer.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Snapshot {
    pub snapshot_id: String,
    pub group: String,
    pub name: String,
    pub created_ms: u64,
    /// The chain of sealed layers, oldest first.
    pub layers: Vec<String>,
    /// The computer's record when the checkpoint was made; a fork takes its resources and image.
    pub template: ComputerRecord,
    /// Set while an import is still filling the layers: a half-filled layer is not a checkpoint.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub filling: bool,
}

impl Snapshot {
    pub fn new(group: &str, name: &str, layers: Vec<String>, template: ComputerRecord) -> Self {
        Self {
            snapshot_id: format!("snap_{}", uuid::Uuid::new_v4().simple()),
            group: group.to_owned(),
            name: name.to_owned(),
            created_ms: now_ms(),
            layers,
            template,
            filling: false,
        }
    }

    /// The entry of `snapshot list --format json`. The scope is always disk.
    pub fn listed(&self, available: bool) -> Value {
        json!({
            "snapshot_id": self.snapshot_id,
            "name": self.name,
            "group": self.group,
            "scope": "disk",
            "availability": if available { "ready" } else { "incomplete" },
            "parent_digest": null,
            "created_at": format!("{:020}", self.created_ms),
            "artifact_path": null,
        })
    }
}

/// The checkpoints on disk.
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(layout: &Layout) -> Self {
        Self {
            root: layout.snapshots(),
        }
    }

    fn file(&self, group: &str, name: &str) -> Result<PathBuf, String> {
        if !valid_name(group) || !valid_name(name) {
            return Err("The checkpoint name is not valid.".into());
        }
        Ok(self.root.join(group).join(format!("{name}.json")))
    }

    pub fn group_exists(&self, group: &str) -> bool {
        valid_name(group) && self.root.join(group).is_dir()
    }

    pub fn write(&self, snapshot: &Snapshot) -> Result<(), String> {
        let file = self.file(&snapshot.group, &snapshot.name)?;
        let folder = file.parent().ok_or("The checkpoint has no folder.")?;
        std::fs::create_dir_all(folder)
            .map_err(|e| format!("The checkpoint could not be saved: {e}"))?;
        let temporary = folder.join(format!(".{}.new", snapshot.name));
        std::fs::write(
            &temporary,
            serde_json::to_vec_pretty(snapshot).map_err(|e| e.to_string())?,
        )
        .and_then(|()| std::fs::rename(&temporary, &file))
        .map_err(|e| format!("The checkpoint could not be saved: {e}"))
    }

    pub fn read(&self, group: &str, name: &str) -> Result<Snapshot, String> {
        let file = self.file(group, name)?;
        let text = std::fs::read_to_string(&file)
            .map_err(|_| format!("The checkpoint {group}:{name} does not exist."))?;
        serde_json::from_str(&text)
            .map_err(|_| format!("The checkpoint {group}:{name} is damaged."))
    }

    pub fn remove(&self, group: &str, name: &str) -> Result<(), String> {
        let file = self.file(group, name)?;
        match std::fs::remove_file(&file) {
            Ok(()) => {
                let _ = std::fs::remove_dir(self.root.join(group));
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(format!("The checkpoint {group}:{name} does not exist."))
            }
            Err(e) => Err(format!("The checkpoint could not be removed: {e}")),
        }
    }

    /// Every checkpoint (of `group` only, when given). An unreadable file or folder fails the
    /// listing: a partial answer could be taken for absence.
    pub fn list(&self, group: Option<&str>) -> Result<Vec<Snapshot>, String> {
        let unreadable = |_| "A checkpoint could not be read.".to_owned();
        let mut found = Vec::new();
        let groups: Vec<PathBuf> = match group {
            Some(group) if valid_name(group) => vec![self.root.join(group)],
            Some(_) => return Ok(found),
            None => entries(&self.root).map_err(unreadable)?,
        };
        for folder in groups {
            for path in entries(&folder).map_err(unreadable)? {
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                let text = std::fs::read_to_string(&path).map_err(unreadable)?;
                found.push(
                    serde_json::from_str(&text)
                        .map_err(|_| "A checkpoint is damaged.".to_owned())?,
                );
            }
        }
        found.sort_by(|a: &Snapshot, b| (a.created_ms, &a.name).cmp(&(b.created_ms, &b.name)));
        Ok(found)
    }
}

/// What the folder `dir` holds; nothing when there is no such folder (or it is a file). Any other
/// failure is an error: a folder that could not be read is not an empty one.
fn entries(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .map(|entry| entry.map(|entry| entry.path()))
            .collect(),
        Err(error) if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => {
            Ok(Vec::new())
        }
        Err(error) => Err(error),
    }
}

/// Whether every layer the checkpoint names is in the store, and none is still being filled.
pub fn intact(layout: &Layout, snapshot: &Snapshot) -> bool {
    !snapshot.filling
        && snapshot
            .layers
            .iter()
            .all(|id| valid_name(id) && layout.layer(id).is_dir())
}

/// The record of a computer after its writable layer `sealed` joined the chain and `fresh` became
/// its writable layer.
pub fn sealed(record: &ComputerRecord, fresh: &str) -> ComputerRecord {
    let mut next = record.clone();
    next.layers.push(record.upper.clone());
    next.upper = fresh.to_owned();
    next
}

/// The record of a fork: the checkpoint's template under a new name, over the checkpoint's chain,
/// with a writable layer of its own. Labels, environment and secrets come from the restore.
pub fn forked(snapshot: &Snapshot, name: &str) -> ComputerRecord {
    let mut record = snapshot.template.clone();
    record.name = name.to_owned();
    record.layers = snapshot.layers.clone();
    record.upper = new_id();
    record.started_once = false;
    record.created_ms = now_ms();
    record
}

/// How many of the oldest layers of `chain` a maintenance step folds into one; zero when the chain
/// is short enough.
pub fn flatten_count(chain: &[String]) -> usize {
    if chain.len() > MAX_CHAIN {
        FLATTEN_COUNT.min(chain.len())
    } else {
        0
    }
}

/// `chain` after its oldest `count` layers became the one layer `merged`.
pub fn flattened(chain: &[String], count: usize, merged: &str) -> Vec<String> {
    let mut next = vec![merged.to_owned()];
    next.extend(chain[count.min(chain.len())..].iter().cloned());
    next
}

/// Every computer's record. Fails when a record, or the folder that holds the computers, cannot
/// be read: what could not be read might name anything.
fn records(layout: &Layout) -> Result<Vec<ComputerRecord>, String> {
    let unreadable = |_| "The container computers could not be read.".to_owned();
    let mut records = Vec::new();
    for folder in entries(&layout.computers()).map_err(unreadable)? {
        // A folder without a record is a computer being made or removed.
        match std::fs::symlink_metadata(folder.join("computer.json")) {
            Ok(_) => {}
            Err(error)
                if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) =>
            {
                continue
            }
            Err(error) => return Err(unreadable(error)),
        }
        let name = folder.file_name().and_then(|name| name.to_str());
        records.push(
            layout
                .computer(name.ok_or("A container computer's name could not be read.")?)
                .read_record()?,
        );
    }
    Ok(records)
}

/// The ids the computers' records and the checkpoints name. Fails when any of them, or a folder
/// that holds them, cannot be read: a layer is only collected when everything that might name it
/// was read.
pub fn referenced(layout: &Layout) -> Result<HashSet<String>, String> {
    let mut ids = HashSet::new();
    for record in records(layout)? {
        ids.extend(record.layers);
        if !record.upper.is_empty() {
            ids.insert(record.upper);
        }
    }
    for snapshot in Store::new(layout).list(None)? {
        ids.extend(snapshot.layers);
    }
    Ok(ids)
}

/// The config digests of the guest images the computers' records and the checkpoints name. A
/// checkpoint counts because an imported computer has one before it has a record (its first start
/// makes the record), and its layers hold changes against exactly that image. Fails when
/// [`referenced`] does: an unpacked image is only removed when everything that might need it was
/// read.
pub fn images_named(layout: &Layout) -> Result<HashSet<String>, String> {
    let mut digests: HashSet<String> = records(layout)?
        .into_iter()
        .map(|record| record.image_digest)
        .collect();
    for snapshot in Store::new(layout).list(None)? {
        digests.insert(snapshot.template.image_digest);
    }
    Ok(digests)
}

/// The store's entries that nothing names, from `(entry name, age)` pairs. Staging directories
/// (dot-prefixed) go once they are old; a layer goes once it is older than the grace period.
pub fn unreferenced(present: &[(String, Duration)], referenced: &HashSet<String>) -> Vec<String> {
    present
        .iter()
        .filter(|(name, age)| *age > GRACE && !referenced.contains(name))
        .map(|(name, _)| name.clone())
        .collect()
}

/// The store's entries with their age.
pub fn present(layout: &Layout) -> Vec<(String, Duration)> {
    let now = SystemTime::now();
    std::fs::read_dir(layout.layers())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let age = entry
                .metadata()
                .ok()?
                .modified()
                .ok()
                .and_then(|time| now.duration_since(time).ok())
                .unwrap_or_default();
            Some((name, age))
        })
        .collect()
}

/// The layer directories to remove now: everything in the store that nothing names, that is old
/// enough and that is not being made. Empty when a record or checkpoint could not be read.
pub fn collectible(layout: &Layout) -> Vec<PathBuf> {
    let Ok(named) = referenced(layout) else {
        return Vec::new();
    };
    let present = present(layout);
    // Read after the store was listed: a layer is reserved before it is made.
    let making = reserved().clone();
    unreferenced(&present, &named)
        .into_iter()
        .filter(|name| !making.contains(name))
        .map(|name| layout.layers().join(name))
        .collect()
}

/// A mount's lower directories, as the overlay option string holds them.
pub fn lower_option_bytes(lower: &[PathBuf]) -> usize {
    lower
        .iter()
        .map(|path| path.as_os_str().len() + 1)
        .sum::<usize>()
}

pub fn is_empty_dir(path: &Path) -> bool {
    std::fs::read_dir(path).is_ok_and(|mut entries| entries.next().is_none())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> ComputerRecord {
        let home = tempfile::tempdir().unwrap();
        super::super::capability::make_container(home.path(), "box");
        Layout::new(home.path())
            .computer("box")
            .read_record()
            .unwrap()
    }

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_seal_moves_the_upper_into_the_chain_and_opens_a_new_one() {
        let mut before = record();
        before.layers = ids(&["a"]);
        before.upper = "b".into();
        let after = sealed(&before, "c");
        assert_eq!(after.layers, ids(&["a", "b"]));
        assert_eq!(after.upper, "c");
        assert_eq!(before.layers, ids(&["a"]));
    }

    #[test]
    fn a_fork_has_the_checkpoint_chain_and_a_layer_of_its_own() {
        let mut template = record();
        template.upper = "parent".into();
        template.started_once = true;
        let snapshot = Snapshot::new("box", "c1", ids(&["a", "b"]), template);
        let fork = forked(&snapshot, "child");
        assert_eq!(fork.name, "child");
        assert_eq!(fork.layers, ids(&["a", "b"]));
        assert_ne!(fork.upper, "parent");
        assert!(valid_name(&fork.upper));
        assert!(!fork.started_once);
        let second = forked(&snapshot, "other");
        assert_ne!(second.upper, fork.upper);
    }

    #[test]
    fn a_chain_over_the_cap_flattens_its_oldest_layers() {
        let short: Vec<String> = (0..MAX_CHAIN).map(|n| format!("l{n}")).collect();
        assert_eq!(flatten_count(&short), 0);
        let long: Vec<String> = (0..=MAX_CHAIN).map(|n| format!("l{n}")).collect();
        let count = flatten_count(&long);
        assert_eq!(count, FLATTEN_COUNT);
        let next = flattened(&long, count, "merged");
        assert_eq!(next.len(), long.len() - count + 1);
        assert_eq!(next[0], "merged");
        assert_eq!(next[1], long[count]);
        assert_eq!(next.last(), long.last());
    }

    #[test]
    fn the_snapshot_store_writes_reads_lists_and_removes() {
        let home = tempfile::tempdir().unwrap();
        let layout = Layout::new(home.path());
        let store = Store::new(&layout);
        assert!(!store.group_exists("box"));
        let one = Snapshot::new("box", "c1", ids(&["a"]), record());
        store.write(&one).unwrap();
        store
            .write(&Snapshot::new("box", "c2", ids(&["a", "b"]), record()))
            .unwrap();
        store
            .write(&Snapshot::new("other", "c3", Vec::new(), record()))
            .unwrap();
        assert!(store.group_exists("box"));
        assert_eq!(store.read("box", "c1").unwrap(), one);
        assert_eq!(store.list(Some("box")).unwrap().len(), 2);
        assert_eq!(store.list(None).unwrap().len(), 3);
        assert!(store.list(Some("../x")).unwrap().is_empty());
        store.remove("box", "c1").unwrap();
        assert!(store
            .read("box", "c1")
            .unwrap_err()
            .contains("does not exist"));
        assert!(store.remove("box", "c1").is_err());
        store.remove("box", "c2").unwrap();
        assert!(!store.group_exists("box"));
        assert!(store.read("../etc", "passwd").is_err());
    }

    #[test]
    fn a_listed_snapshot_has_the_shape_the_engine_reads() {
        let snapshot = Snapshot::new("box", "c1", ids(&["a"]), record());
        let entry = snapshot.listed(true);
        assert_eq!(entry["group"], "box");
        assert_eq!(entry["name"], "c1");
        assert_eq!(entry["scope"], "disk");
        assert_eq!(entry["availability"], "ready");
        assert!(entry["snapshot_id"].as_str().unwrap().starts_with("snap_"));
        assert_eq!(snapshot.listed(false)["availability"], "incomplete");
    }

    #[test]
    fn layers_that_nothing_names_are_collected_once_they_are_old() {
        let old = GRACE + Duration::from_secs(1);
        let present = vec![
            ("kept".to_owned(), old),
            ("orphan".to_owned(), old),
            ("fresh".to_owned(), Duration::from_secs(1)),
            (".new-stale".to_owned(), old),
        ];
        let named: HashSet<String> = ["kept".to_owned()].into();
        assert_eq!(unreferenced(&present, &named), ["orphan", ".new-stale"]);
    }

    #[test]
    fn what_records_and_checkpoints_name_is_referenced_and_a_deleted_parent_keeps_the_fork_layers()
    {
        let home = tempfile::tempdir().unwrap();
        let layout = Layout::new(home.path());
        let dir = layout.computer("parent");
        std::fs::create_dir_all(dir.path()).unwrap();
        let mut parent = record();
        parent.name = "parent".into();
        parent.layers = ids(&["a"]);
        parent.upper = "b".into();
        dir.write_record(&parent).unwrap();
        let snapshot = Snapshot::new("parent", "c1", ids(&["a"]), parent.clone());
        Store::new(&layout).write(&snapshot).unwrap();
        let fork = forked(&snapshot, "child");
        let child_dir = layout.computer("child");
        std::fs::create_dir_all(child_dir.path()).unwrap();
        child_dir.write_record(&fork).unwrap();
        let all = referenced(&layout).unwrap();
        assert!(all.contains("a") && all.contains("b") && all.contains(&fork.upper));
        // The parent and its checkpoint go: the fork still names the shared layer.
        std::fs::remove_dir_all(dir.path()).unwrap();
        Store::new(&layout).remove("parent", "c1").unwrap();
        let rest = referenced(&layout).unwrap();
        assert!(rest.contains("a") && !rest.contains("b"));
    }

    #[test]
    fn an_imported_checkpoint_names_its_image_before_its_computer_has_a_record() {
        let home = tempfile::tempdir().unwrap();
        let layout = Layout::new(home.path());
        let older = format!("sha256:{}", "a".repeat(64));
        let mut template = record();
        template.image_digest = older.clone();
        Store::new(&layout)
            .write(&Snapshot::new(
                "silo-import-ab",
                "c1",
                ids(&["l1"]),
                template,
            ))
            .unwrap();
        assert!(layout.names().is_empty());
        assert_eq!(images_named(&layout).unwrap(), [older].into());
        // A checkpoint that cannot be read says nothing about what it needs.
        std::fs::write(layout.snapshots().join("silo-import-ab/c2.json"), "{").unwrap();
        assert!(images_named(&layout).is_err());
    }

    #[test]
    fn collection_takes_old_orphans_and_nothing_a_record_or_checkpoint_names() {
        let home = tempfile::tempdir().unwrap();
        let layout = Layout::new(home.path());
        let dir = layout.computer("box");
        std::fs::create_dir_all(dir.path()).unwrap();
        let mut box_record = record();
        box_record.layers = ids(&["sealed"]);
        box_record.upper = "live".into();
        dir.write_record(&box_record).unwrap();
        Store::new(&layout)
            .write(&Snapshot::new("box", "c1", ids(&["held"]), box_record))
            .unwrap();
        let old = SystemTime::now() - GRACE - Duration::from_secs(60);
        for name in ["sealed", "live", "held", "orphan", ".new-stale"] {
            std::fs::create_dir_all(layout.layer(name)).unwrap();
            std::fs::File::open(layout.layer(name))
                .unwrap()
                .set_modified(old)
                .unwrap();
        }
        std::fs::create_dir_all(layout.layer("fresh")).unwrap();
        let mut found: Vec<String> = collectible(&layout)
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        found.sort();
        assert_eq!(found, [".new-stale", "orphan"]);
        // A record that cannot be read could name anything: nothing is collected.
        std::fs::write(dir.record_path(), "{").unwrap();
        assert!(collectible(&layout).is_empty());
    }

    fn old_layer(layout: &Layout, name: &str) {
        std::fs::create_dir_all(layout.layer(name)).unwrap();
        std::fs::File::open(layout.layer(name))
            .unwrap()
            .set_modified(SystemTime::now() - GRACE - Duration::from_secs(3600))
            .unwrap();
    }

    #[test]
    fn a_layer_being_made_is_not_collected_however_old_its_files_look() {
        // A copy takes its source's modification time (`cp -a`, `silo-ctr merge`), and it is in
        // the store before a record or a checkpoint names it.
        let home = tempfile::tempdir().unwrap();
        let layout = Layout::new(home.path());
        let reservation = reserve("copy");
        old_layer(&layout, "copy");
        old_layer(&layout, "orphan");
        assert_eq!(collectible(&layout), [layout.layer("orphan")]);
        // Nothing named it in the end (the copy failed): it goes like any other.
        drop(reservation);
        assert_eq!(collectible(&layout).len(), 2);
    }

    /// Makes `folder` unreadable for the length of `check`. False when this account reads it
    /// anyway (root), so there is nothing to check.
    fn while_unreadable(folder: &Path, check: impl FnOnce()) -> bool {
        use std::os::unix::fs::PermissionsExt;
        let mode = |mode| std::fs::set_permissions(folder, std::fs::Permissions::from_mode(mode));
        mode(0o000).unwrap();
        let unreadable = std::fs::read_dir(folder).is_err();
        if unreadable {
            check();
        }
        mode(0o700).unwrap();
        unreadable
    }

    #[test]
    fn a_folder_of_records_or_checkpoints_that_cannot_be_read_stops_the_scan() {
        let home = tempfile::tempdir().unwrap();
        let layout = Layout::new(home.path());
        let dir = layout.computer("box");
        std::fs::create_dir_all(dir.path()).unwrap();
        let mut box_record = record();
        box_record.layers = Vec::new();
        box_record.upper = "live".into();
        dir.write_record(&box_record).unwrap();
        let store = Store::new(&layout);
        store
            .write(&Snapshot::new("box", "c1", ids(&["held"]), box_record))
            .unwrap();
        for name in ["live", "held", "orphan"] {
            old_layer(&layout, name);
        }
        assert_eq!(collectible(&layout), [layout.layer("orphan")]);
        // What cannot be listed could name anything: it is not taken for empty.
        for folder in [
            layout.computers(),
            layout.snapshots(),
            layout.snapshots().join("box"),
        ] {
            while_unreadable(&folder, || {
                assert!(referenced(&layout).is_err(), "{}", folder.display());
                assert!(images_named(&layout).is_err(), "{}", folder.display());
                assert!(collectible(&layout).is_empty(), "{}", folder.display());
            });
        }
        while_unreadable(&layout.snapshots().join("box"), || {
            assert!(store.list(Some("box")).is_err());
            assert!(store.list(None).is_err());
        });
        // A folder with no record (a computer being made or removed) and a stray file name nothing.
        std::fs::create_dir_all(layout.computers().join("half")).unwrap();
        std::fs::write(layout.computers().join("stray"), "").unwrap();
        std::fs::write(layout.snapshots().join("stray"), "").unwrap();
        assert_eq!(collectible(&layout), [layout.layer("orphan")]);
        assert_eq!(store.list(None).unwrap().len(), 1);
    }

    #[test]
    fn an_unreadable_record_stops_the_scan() {
        let home = tempfile::tempdir().unwrap();
        let layout = Layout::new(home.path());
        let dir = layout.computer("bad");
        std::fs::create_dir_all(dir.path()).unwrap();
        std::fs::write(dir.record_path(), "{").unwrap();
        assert!(referenced(&layout).is_err());
    }

    #[test]
    fn names_that_could_escape_the_store_are_refused() {
        for bad in ["", ".", "..", ".hidden", "a/b", "a b", &"x".repeat(65)] {
            assert!(!valid_name(bad), "{bad:?}");
        }
        assert!(valid_name(&new_id()));
        assert!(valid_name("c0123456789abcdef0123456789abcdef"));
    }

    #[test]
    fn the_lower_option_length_counts_every_path_and_separator() {
        let lower = [PathBuf::from("/a/b"), PathBuf::from("/c")];
        assert_eq!(lower_option_bytes(&lower), 4 + 1 + 2 + 1);
    }

    #[test]
    fn a_checkpoint_still_being_filled_is_not_intact_and_can_still_be_removed() {
        let home = tempfile::tempdir().unwrap();
        let layout = Layout::new(home.path());
        std::fs::create_dir_all(layout.layer("a")).unwrap();
        let mut snapshot = Snapshot::new("silo-import-ab", "c1", vec!["a".into()], record());
        assert!(intact(&layout, &snapshot));
        snapshot.filling = true;
        assert!(!intact(&layout, &snapshot));
        assert_eq!(
            snapshot.listed(intact(&layout, &snapshot))["availability"],
            "incomplete"
        );
        // A crash leaves the record as written; recovery lists and removes it like any other.
        let store = Store::new(&layout);
        store.write(&snapshot).unwrap();
        assert!(store.read("silo-import-ab", "c1").unwrap().filling);
        store.remove("silo-import-ab", "c1").unwrap();
        assert!(store.list(None).unwrap().is_empty());
        // Once finished the flag is gone from the saved record.
        snapshot.filling = false;
        store.write(&snapshot).unwrap();
        let text =
            std::fs::read_to_string(layout.snapshots().join("silo-import-ab/c1.json")).unwrap();
        assert!(!text.contains("filling"));
    }
}
