//! Each computer's network policy, keyed by the computer's id, in
//! `<app data>/network-policy.json`, with the preset versions it was approved at. A computer
//! without an entry is one saved before policies existed; [`super::effective`] decides what it
//! runs with. A file that cannot be read is an error, never an empty store, and so is a missing
//! entry of a computer marked as having had one ([`super::marks`]): a missing file is an empty
//! store only for computers that never had a policy.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use super::{Held, NetworkPolicy, NetworkPolicyMode, Pins};

const FILE: &str = "network-policy.json";
const VERSION: u8 = 1;
const MAX_DOCUMENT_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Document {
    version: u8,
    #[serde(default)]
    computers: BTreeMap<String, Stored>,
    /// Computers whose saved policy is the closed one given while their real rules could not
    /// be read. Any other write for a computer ends that; a policy not listed here is the
    /// computer's own, whatever it holds. Omitted when empty, so the file reads as before.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    stand_in: BTreeSet<String>,
}

/// One computer's saved policy.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Stored {
    pub mode: NetworkPolicyMode,
    #[serde(default)]
    pub hosts: Vec<String>,
    #[serde(default)]
    pub presets: Vec<String>,
    /// The version each preset was approved at; a preset without an entry was approved at the
    /// first version.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub preset_versions: Pins,
    /// Connections to the computer's published ports are refused, as for an import or the copy
    /// an export makes, until the owner sets a policy.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub incoming_denied: bool,
}

impl Stored {
    /// `policy` as approved now: its presets at the versions this Silo has.
    pub fn approved(policy: &NetworkPolicy) -> Self {
        Self::of(Held::latest(policy.clone()), false)
    }

    /// `policy` saved over `before`: a preset already there keeps the version it was approved
    /// at (none means the first), and a preset added now is pinned to the version this Silo has.
    /// The GitHub grant's hosts keep the version they were approved at the same way; with no
    /// `before` the grant holds the first version.
    pub fn preserving_pins(
        policy: &NetworkPolicy,
        before: Option<&Stored>,
        incoming_denied: bool,
    ) -> Self {
        let source = before.map(Stored::held);
        let pins = Held::inheriting(policy.clone(), source.as_ref()).pins;
        Self {
            mode: policy.mode,
            hosts: policy.hosts.clone(),
            presets: policy.presets.clone(),
            preset_versions: pins,
            incoming_denied,
        }
    }

    pub fn of(held: Held, incoming_denied: bool) -> Self {
        Self {
            mode: held.policy.mode,
            hosts: held.policy.hosts,
            presets: held.policy.presets,
            preset_versions: held.pins,
            incoming_denied,
        }
    }

    /// Closed, with connections to the computer's ports refused too.
    pub fn closed_without_incoming() -> Self {
        Self::of(
            Held::latest(NetworkPolicy {
                mode: NetworkPolicyMode::Closed,
                ..NetworkPolicy::default()
            }),
            true,
        )
    }

    pub fn held(&self) -> Held {
        Held {
            policy: NetworkPolicy {
                mode: self.mode,
                hosts: self.hosts.clone(),
                presets: self.presets.clone(),
            },
            pins: self.preset_versions.clone(),
        }
    }
}

static LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
thread_local! {
    static TEST_PATH: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// Tests on this thread keep policies in `path` instead of the app's file.
#[cfg(test)]
pub(crate) fn use_test_path(path: Option<PathBuf>) {
    TEST_PATH.with(|test| *test.borrow_mut() = path);
}

#[cfg(test)]
pub(crate) fn uses_test_path() -> bool {
    TEST_PATH.with(|test| test.borrow().is_some())
}

fn path() -> Option<PathBuf> {
    #[cfg(test)]
    if let Some(path) = TEST_PATH.with(|test| test.borrow().clone()) {
        return Some(path);
    }
    crate::context::current().map(|context| context.paths().app_data.join(FILE))
}

fn read(path: &PathBuf) -> Result<Document, String> {
    use std::io::Read;
    match fs::File::open(path) {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(MAX_DOCUMENT_BYTES)
                .read_to_end(&mut bytes)
                .map_err(|error| format!("Cannot read the saved network policies: {error}"))?;
            let document: Document = serde_json::from_slice(&bytes)
                .map_err(|error| format!("The saved network policies are unreadable: {error}"))?;
            if document.version != VERSION {
                return Err("The saved network policies are from a newer Silo.".into());
            }
            Ok(document)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Document {
            version: VERSION,
            ..Document::default()
        }),
        Err(error) => Err(format!("Cannot read the saved network policies: {error}")),
    }
}

fn write(path: &PathBuf, document: &Document) -> Result<(), String> {
    let io = |error: std::io::Error| format!("Cannot save the network policies: {error}");
    let directory = path
        .parent()
        .ok_or_else(|| "The network policy file has no folder.".to_owned())?;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(directory)
        .map_err(io)?;
    let temporary = directory.join(format!(
        "network-policy-{}.tmp",
        uuid::Uuid::new_v4().simple()
    ));
    let bytes = serde_json::to_vec_pretty(document).map_err(|error| error.to_string())?;
    let written = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        fs::File::open(directory)?.sync_all()
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    written.map_err(io)
}

/// What the store says for a computer marked as having had a saved policy that has no entry.
pub const MISSING: &str = "The saved network policy of this computer is missing, so it cannot start or be copied. Set its network policy again.";

/// The entry of `computer_id` in `document`, or the error for one that a marked computer lost.
fn entry<'a>(document: &'a Document, computer_id: &str) -> Result<Option<&'a Stored>, String> {
    match document.computers.get(computer_id) {
        Some(stored) => Ok(Some(stored)),
        None if super::marks::is_marked(computer_id)? => Err(MISSING.to_owned()),
        None => Ok(None),
    }
}

/// Changes the stored policies and the stand-in marks for `target`. Without an installed store
/// (no engine) nothing is kept. `target`, when it has an entry afterwards, and any computer
/// that gets or changes an entry are marked before the file is written; one whose entry is
/// removed is unmarked after.
fn update_document(
    target: &str,
    change: impl FnOnce(&mut Document) -> Result<(), String>,
) -> Result<(), String> {
    let Some(path) = path() else {
        return Ok(());
    };
    let _held = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut document = read(&path)?;
    let before = document.clone();
    change(&mut document)?;
    if document.computers.contains_key(target) {
        super::marks::mark(target)?;
    }
    if document != before {
        for (id, stored) in &document.computers {
            if id != target && before.computers.get(id) != Some(stored) {
                super::marks::mark(id)?;
            }
        }
        document.version = VERSION;
        write(&path, &document)?;
        for id in before.computers.keys() {
            if !document.computers.contains_key(id) {
                super::marks::unmark(id)?;
            }
        }
    }
    Ok(())
}

/// Marks every computer that has an entry, for policies saved before marks existed. Run when
/// the engine starts; a store that cannot be read marks nothing, and a missing file has no
/// entries.
pub fn mark_saved() -> Result<(), String> {
    let Some(path) = path() else {
        return Ok(());
    };
    let _held = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let document = read(&path)?;
    let mut failed = Vec::new();
    for id in document.computers.keys() {
        if let Err(error) = super::marks::mark_existing(id) {
            failed.push(error);
        }
    }
    if failed.is_empty() {
        Ok(())
    } else {
        Err(failed.join(" "))
    }
}

/// Changes the policy of `computer_id`: whatever it holds afterwards is the computer's own, so
/// it is no longer a stand-in.
fn update_computer(
    computer_id: &str,
    change: impl FnOnce(&mut BTreeMap<String, Stored>) -> Result<(), String>,
) -> Result<(), String> {
    update_document(computer_id, |document| {
        change(&mut document.computers)?;
        document.stand_in.remove(computer_id);
        Ok(())
    })
}

/// The stored policy of `computer_id` with its approvals; `None` when it has none or no store
/// is installed. An error when the saved policies cannot be read or the computer's is missing
/// ([`MISSING`]): callers must not treat that as "no policy".
pub fn get_stored(computer_id: &str) -> Result<Option<Stored>, String> {
    let Some(path) = path() else {
        return Ok(None);
    };
    let _held = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let document = read(&path)?;
    Ok(entry(&document, computer_id)?.cloned())
}

/// [`get_stored`] that reads a marked computer's missing policy as none, for the owner setting
/// it again.
pub(crate) fn get_stored_or_missing(computer_id: &str) -> Result<Option<Stored>, String> {
    let Some(path) = path() else {
        return Ok(None);
    };
    let _held = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    Ok(read(&path)?.computers.get(computer_id).cloned())
}

/// Whether `computer_id` has an entry, whether or not it is marked.
pub fn has_entry(computer_id: &str) -> Result<bool, String> {
    Ok(get_stored_or_missing(computer_id)?.is_some())
}

/// The stored policy of `computer_id`; `None` when it has none or no store is installed.
pub fn get(computer_id: &str) -> Result<Option<NetworkPolicy>, String> {
    Ok(get_stored(computer_id)?.map(|stored| stored.held().policy))
}

/// Saves `policy` (already validated) for `computer_id`, approved now.
pub fn set(computer_id: &str, policy: &NetworkPolicy) -> Result<(), String> {
    set_stored(computer_id, Stored::approved(policy))
}

pub fn set_stored(computer_id: &str, stored: Stored) -> Result<(), String> {
    update_computer(computer_id, |computers| {
        computers.insert(computer_id.to_owned(), stored);
        Ok(())
    })
}

/// Saves `stored` for `computer_id` only if what is saved still equals `expected`; otherwise
/// the policy changed since it was read and nothing is written.
pub fn replace_if(
    computer_id: &str,
    expected: Option<&Stored>,
    stored: Stored,
) -> Result<(), String> {
    update_computer(computer_id, |computers| {
        if computers.get(computer_id) != expected {
            return Err(CHANGED.to_owned());
        }
        computers.insert(computer_id.to_owned(), stored);
        Ok(())
    })
}

/// What a refused compare-and-set says.
pub const CHANGED: &str =
    "The network policy of this computer changed while this was being decided. Read it again and retry.";

/// Gives `target` the policy `policy`, or `source`'s (with its approvals) when `policy` is
/// `None`. A policy given keeps the versions `source` was approved at ([`Held::inheriting`]):
/// handing a policy on never advances a preset or the GitHub grant, which takes an approval
/// of its own. It keeps `source`'s refusal of incoming connections too: the saved one, else
/// what `recorded` (the rules `source`'s runtime holds) means. A source with no entry gives
/// nothing, so the target is treated like its source; a source whose policy is missing
/// ([`MISSING`]) gives an error.
pub fn inherit(
    source: &str,
    target: &str,
    policy: Option<&NetworkPolicy>,
    recorded: Option<&serde_json::Value>,
) -> Result<(), String> {
    update_document(target, |document| {
        let from = entry(document, source)?.cloned();
        let computers = &mut document.computers;
        let chosen = match policy {
            Some(policy) => {
                let incoming_denied = match &from {
                    Some(from) => from.incoming_denied,
                    None => super::held_from_recorded(recorded).1,
                };
                Some(Stored::preserving_pins(policy, from.as_ref(), incoming_denied))
            }
            None => from,
        };
        match chosen {
            Some(chosen) => {
                computers.insert(target.to_owned(), chosen);
            }
            None => {
                computers.remove(target);
            }
        }
        document.stand_in.remove(target);
        Ok(())
    })
}

/// Whether `computer_id` has no policy of its own yet: it has none saved, or only the closed
/// one [`set_stand_in`] gave it.
pub fn is_unresolved(computer_id: &str) -> Result<bool, String> {
    let Some(path) = path() else {
        return Ok(false);
    };
    let _held = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let document = read(&path)?;
    Ok(entry(&document, computer_id)?.is_none() || document.stand_in.contains(computer_id))
}

/// Saves the closed policy for `computer_id` while its real rules cannot be read, and marks it
/// as a stand-in. A computer that already has a policy of its own keeps it.
pub fn set_stand_in(computer_id: &str) -> Result<(), String> {
    update_document(computer_id, |document| {
        if document.computers.contains_key(computer_id) && !document.stand_in.contains(computer_id)
        {
            return Ok(());
        }
        document
            .computers
            .insert(computer_id.to_owned(), Stored::closed_without_incoming());
        document.stand_in.insert(computer_id.to_owned());
        Ok(())
    })
}

/// Saves `stored` for `computer_id` as a stand-in: a later [`resolve`] or [`resolve_from`]
/// replaces it. A computer that already has a policy of its own keeps it.
pub fn set_provisional(computer_id: &str, stored: Stored) -> Result<(), String> {
    update_document(computer_id, |document| {
        if document.computers.contains_key(computer_id) && !document.stand_in.contains(computer_id)
        {
            return Ok(());
        }
        document.computers.insert(computer_id.to_owned(), stored);
        document.stand_in.insert(computer_id.to_owned());
        Ok(())
    })
}

/// Gives `target` the policy `chosen`, only if `target` has no policy of its own yet (see
/// [`is_unresolved`]). Returns whether it was saved; a policy the computer has stays, whatever
/// it holds.
pub fn resolve(target: &str, chosen: Stored) -> Result<bool, String> {
    let mut saved = false;
    update_document(target, |document| {
        if document.computers.contains_key(target) && !document.stand_in.contains(target) {
            return Ok(());
        }
        document.computers.insert(target.to_owned(), chosen);
        document.stand_in.remove(target);
        saved = true;
        Ok(())
    })?;
    Ok(saved)
}

/// [`resolve`] with the policy `source` has saved, with its approvals. Returns whether `source`
/// had one of its own and `target` took it; a stand-in `source` gives nothing.
pub fn resolve_from(source: &str, target: &str) -> Result<bool, String> {
    let mut saved = false;
    update_document(target, |document| {
        if document.computers.contains_key(target) && !document.stand_in.contains(target) {
            return Ok(());
        }
        if document.stand_in.contains(source) {
            return Ok(());
        }
        if let Some(chosen) = document.computers.get(source).cloned() {
            document.computers.insert(target.to_owned(), chosen);
            document.stand_in.remove(target);
            saved = true;
        }
        Ok(())
    })?;
    Ok(saved)
}

/// Removes the policy of `computer_id`, then its mark.
pub fn remove(computer_id: &str) -> Result<(), String> {
    update_computer(computer_id, |computers| {
        computers.remove(computer_id);
        Ok(())
    })?;
    if path().is_some() {
        super::marks::unmark(computer_id)?;
    }
    Ok(())
}

/// The computers that have an entry, in order.
#[cfg(test)]
pub(crate) fn get_stored_ids_for_test() -> Result<Vec<String>, String> {
    let Some(path) = path() else {
        return Ok(Vec::new());
    };
    Ok(read(&path)?.computers.keys().cloned().collect())
}
