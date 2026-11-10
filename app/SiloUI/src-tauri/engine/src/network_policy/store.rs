//! Each computer's network policy, keyed by the computer's id, in
//! `<app data>/network-policy.json`. A computer without an entry is one saved before policies
//! existed; [`super::effective`] decides what it runs with.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use super::NetworkPolicy;

const FILE: &str = "network-policy.json";
const VERSION: u8 = 1;
const MAX_DOCUMENT_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Document {
    version: u8,
    #[serde(default)]
    computers: BTreeMap<String, NetworkPolicy>,
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
fn update(change: impl FnOnce(&mut BTreeMap<String, NetworkPolicy>)) -> Result<(), String> {
    let Some(path) = path() else {
        return Ok(());
    };
    let _held = LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut document = read(&path)?;
    let before = document.clone();
    change(&mut document.computers);
    if document != before {
        document.version = VERSION;
        write(&path, &document)?;
    }
    Ok(())
}

/// The stored policy of `computer_id`; `None` when it has none or no store is installed.
pub fn get(computer_id: &str) -> Result<Option<NetworkPolicy>, String> {
    let Some(path) = path() else {
        return Ok(None);
    };
    let _held = LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    Ok(read(&path)?.computers.get(computer_id).cloned())
}

/// Saves `policy` (already validated) for `computer_id`.
pub fn set(computer_id: &str, policy: &NetworkPolicy) -> Result<(), String> {
    update(|computers| {
        computers.insert(computer_id.to_owned(), policy.clone());
    })
}

/// Gives `target` the policy `policy`, or `source`'s when `policy` is `None`. A source with
/// no entry gives nothing, so the target is treated like its source.
pub fn inherit(
    source: &str,
    target: &str,
    policy: Option<&NetworkPolicy>,
) -> Result<(), String> {
    update(|computers| {
        let chosen = policy.cloned().or_else(|| computers.get(source).cloned());
        match chosen {
            Some(chosen) => {
                computers.insert(target.to_owned(), chosen);
            }
            None => {
                computers.remove(target);
            }
        }
    })
}

pub fn remove(computer_id: &str) -> Result<(), String> {
    update(|computers| {
        computers.remove(computer_id);
    })
}
