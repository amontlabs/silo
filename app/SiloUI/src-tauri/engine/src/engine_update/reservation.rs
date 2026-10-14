//! Computers an unresolved engine update still owns.
//!
//! While the update journal names a computer that is neither resumed nor reported lost, its
//! capture is the only way back to the run the update promised to keep. Cold-starting,
//! modifying or deleting such a computer would orphan or corrupt that capture, so the
//! operation gate refuses every operation on it until recovery settles. The set follows the
//! journal: it is recomputed after every transaction step that changes it.
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::Mutex,
};

use super::journal::Store;

#[derive(Clone, Debug, Eq, PartialEq)]
enum Reserved {
    Ids(BTreeSet<String>),
    /// The journal cannot be read, so which computers it names is unknown.
    Everything,
}

static RESERVED: Mutex<BTreeMap<PathBuf, Reserved>> = Mutex::new(BTreeMap::new());

fn lock() -> std::sync::MutexGuard<'static, BTreeMap<PathBuf, Reserved>> {
    RESERVED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Recomputes the reservation of the journal in `store`: every computer whose entry still
/// needs the journal, or everything when the journal is present but unreadable.
pub fn sync(store: &Store) {
    let reserved = match store.load() {
        Ok(None) => None,
        Ok(Some(journal)) => {
            let ids: BTreeSet<String> = journal
                .computers
                .iter()
                .filter(|entry| !entry.phase.settled())
                .map(|entry| entry.id.clone())
                .collect();
            (!ids.is_empty()).then_some(Reserved::Ids(ids))
        }
        Err(_) => Some(Reserved::Everything),
    };
    let mut all = lock();
    match reserved {
        Some(reserved) => {
            all.insert(store.directory().to_path_buf(), reserved);
        }
        None => {
            all.remove(store.directory());
        }
    }
}

/// Forgets the reservation of `store` (its journal was resolved).
pub fn clear(store: &Store) {
    lock().remove(store.directory());
}

/// Why operations on `id` are refused, if an unresolved update reserves it.
pub fn reason(id: &str) -> Option<String> {
    lock()
        .values()
        .any(|reserved| match reserved {
            Reserved::Ids(ids) => ids.contains(id),
            Reserved::Everything => true,
        })
        .then(|| {
            "This computer was saved for an engine update that has not finished resuming it. Wait for Silo to resume it, or relaunch Silo."
                .to_owned()
        })
}

pub fn is_reserved(id: &str) -> bool {
    reason(id).is_some()
}
