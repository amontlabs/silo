//! Each computer's network policy, keyed by the computer's id, in
//! `<app data>/network-policy.json`, with the preset versions it was approved at. A computer
//! without an entry is one saved before policies existed; [`super::effective`] decides what it
//! runs with. A file that cannot be read is an error, never an empty store.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use super::{Held, NetworkPolicy, NetworkPolicyMode, Pins, GITHUB_GRANT_PIN};

const FILE: &str = "network-policy.json";
const VERSION: u8 = 1;
const MAX_DOCUMENT_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Document {
    version: u8,
    #[serde(default)]
    computers: BTreeMap<String, Stored>,
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
    /// The GitHub grant's hosts keep the version they were approved at the same way.
    pub fn preserving_pins(
        policy: &NetworkPolicy,
        before: Option<&Stored>,
        incoming_denied: bool,
    ) -> Self {
        let mut pins = Held::latest(policy.clone()).pins;
        if let Some(before) = before {
            for preset in before.presets.iter().filter(|name| policy.presets.contains(name)) {
                match before.preset_versions.get(preset) {
                    Some(version) => pins.insert(preset.clone(), *version),
                    None => pins.remove(preset),
                };
            }
            match before.preset_versions.get(GITHUB_GRANT_PIN) {
                Some(version) => pins.insert(GITHUB_GRANT_PIN.to_owned(), *version),
                None => pins.remove(GITHUB_GRANT_PIN),
            };
        }
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

/// Changes the stored policies. Without an installed store (no engine) nothing is kept.
fn update(change: impl FnOnce(&mut BTreeMap<String, Stored>) -> Result<(), String>) -> Result<(), String> {
    let Some(path) = path() else {
        return Ok(());
    };
    let _held = LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut document = read(&path)?;
    let before = document.clone();
    change(&mut document.computers)?;
    if document != before {
        document.version = VERSION;
        write(&path, &document)?;
    }
    Ok(())
}

/// The stored policy of `computer_id` with its approvals; `None` when it has none or no store
/// is installed. An error when the saved policies cannot be read: callers must not treat that
/// as "no policy".
pub fn get_stored(computer_id: &str) -> Result<Option<Stored>, String> {
    let Some(path) = path() else {
        return Ok(None);
    };
    let _held = LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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
    update(|computers| {
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
    update(|computers| {
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
/// `None`. A source with no entry gives nothing, so the target is treated like its source.
pub fn inherit(
    source: &str,
    target: &str,
    policy: Option<&NetworkPolicy>,
) -> Result<(), String> {
    update(|computers| {
        let chosen = match policy {
            Some(policy) => Some(Stored::approved(policy)),
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

pub fn remove(computer_id: &str) -> Result<(), String> {
    update(|computers| {
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
