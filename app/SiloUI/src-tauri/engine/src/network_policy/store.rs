//! Each computer's network policy, keyed by the computer's id, in
//! `<app data>/network-policy.json`, with the preset versions it was approved at. A computer
//! without an entry is one saved before policies existed; [`super::effective`] decides what it
//! runs with. A file that cannot be read is an error, never an empty store.

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

/// Changes the stored policies and the stand-in marks. Without an installed store (no engine)
/// nothing is kept.
fn update_document(change: impl FnOnce(&mut Document) -> Result<(), String>) -> Result<(), String> {
    let Some(path) = path() else {
        return Ok(());
    };
    let _held = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut document = read(&path)?;
    let before = document.clone();
    change(&mut document)?;
    if document != before {
        document.version = VERSION;
        write(&path, &document)?;
    }
    Ok(())
}

/// Changes the policy of `computer_id`: whatever it holds afterwards is the computer's own, so
/// it is no longer a stand-in.
fn update_computer(
    computer_id: &str,
    change: impl FnOnce(&mut BTreeMap<String, Stored>) -> Result<(), String>,
) -> Result<(), String> {
    update_document(|document| {
        change(&mut document.computers)?;
        document.stand_in.remove(computer_id);
        Ok(())
    })
}

/// The stored policy of `computer_id` with its approvals; `None` when it has none or no store
/// is installed. An error when the saved policies cannot be read: callers must not treat that
/// as "no policy".
pub fn get_stored(computer_id: &str) -> Result<Option<Stored>, String> {
    let Some(path) = path() else {
        return Ok(None);
    };
    let _held = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    Ok(read(&path)?.computers.get(computer_id).cloned())
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
/// of its own. A source with no entry gives nothing, so the target is treated like its source.
pub fn inherit(source: &str, target: &str, policy: Option<&NetworkPolicy>) -> Result<(), String> {
    update_computer(target, |computers| {
        let chosen = match policy {
            Some(policy) => Some(Stored::preserving_pins(
                policy,
                computers.get(source),
                false,
            )),
            None => computers.get(source).cloned(),
        };
        match chosen {
            Some(chosen) => {
                computers.insert(target.to_owned(), chosen);
            }
            None => {
                computers.remove(target);
            }
        }
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
    Ok(!document.computers.contains_key(computer_id) || document.stand_in.contains(computer_id))
}

/// Saves the closed policy for `computer_id` while its real rules cannot be read, and marks it
/// as a stand-in. A computer that already has a policy of its own keeps it.
pub fn set_stand_in(computer_id: &str) -> Result<(), String> {
    update_document(|document| {
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
    update_document(|document| {
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
    update_document(|document| {
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
    update_document(|document| {
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

pub fn remove(computer_id: &str) -> Result<(), String> {
    update_computer(computer_id, |computers| {
        computers.remove(computer_id);
        Ok(())
    })
}

/// The computers that have an entry, in order.
#[cfg(test)]
pub(crate) fn get_stored_ids_for_test() -> Result<Vec<String>, String> {
    let Some(path) = path() else {
        return Ok(Vec::new());
    };
    Ok(read(&path)?.computers.keys().cloned().collect())
}
