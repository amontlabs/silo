//! The recipe each computer was built from, keyed by the computer's id, in
//! `<app data>/recipe-records.json`. A fork has its source's record. The record holds the
//! recipe as it was when the computer was built, so a later edit of the stored recipe does not
//! change a computer that exists.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use silo_api::methods::recipes::RecipeRecord;

const FILE: &str = "recipe-records.json";
const VERSION: u8 = 1;
const MAX_DOCUMENT_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct Document {
    version: u8,
    #[serde(default)]
    computers: BTreeMap<String, RecipeRecord>,
}

static LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
thread_local! {
    static TEST_PATH: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// Tests on this thread keep records in `path` instead of the app's file.
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
                .map_err(|error| format!("Cannot read the saved recipe records: {error}"))?;
            let document: Document = serde_json::from_slice(&bytes)
                .map_err(|error| format!("The saved recipe records are unreadable: {error}"))?;
            if document.version != VERSION {
                return Err("The saved recipe records are from a newer Silo.".into());
            }
            Ok(document)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Document {
            version: VERSION,
            ..Document::default()
        }),
        Err(error) => Err(format!("Cannot read the saved recipe records: {error}")),
    }
}

fn write(path: &PathBuf, document: &Document) -> Result<(), String> {
    let io = |error: std::io::Error| format!("Cannot save the recipe records: {error}");
    let directory = path
        .parent()
        .ok_or_else(|| "The recipe records file has no folder.".to_owned())?;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(directory)
        .map_err(io)?;
    let temporary = directory.join(format!(
        "recipe-records-{}.tmp",
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

/// Changes the stored records. Without an installed store (no engine) nothing is kept.
fn update(change: impl FnOnce(&mut BTreeMap<String, RecipeRecord>)) -> Result<(), String> {
    let Some(path) = path() else {
        return Ok(());
    };
    let _held = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut document = read(&path)?;
    let before = document.clone();
    change(&mut document.computers);
    if document != before {
        document.version = VERSION;
        write(&path, &document)?;
    }
    Ok(())
}

/// The record of `computer_id`; `None` when it has none or no store is installed.
pub fn get(computer_id: &str) -> Result<Option<RecipeRecord>, String> {
    let Some(path) = path() else {
        return Ok(None);
    };
    let _held = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    Ok(read(&path)?.computers.get(computer_id).cloned())
}

/// Records that `computer_id` was built from `record`.
pub fn set(computer_id: &str, record: &RecipeRecord) -> Result<(), String> {
    update(|computers| {
        computers.insert(computer_id.to_owned(), record.clone());
    })
}

/// Gives `target` the record of `source`. A source with no record gives nothing, and the
/// target loses any it had.
pub fn inherit(source: &str, target: &str) -> Result<(), String> {
    update(|computers| match computers.get(source).cloned() {
        Some(record) => {
            computers.insert(target.to_owned(), record);
        }
        None => {
            computers.remove(target);
        }
    })
}

pub fn remove(computer_id: &str) -> Result<(), String> {
    update(|computers| {
        computers.remove(computer_id);
    })
}

/// Forgets a removed computer's record.
pub fn computer_removed(computer_id: &str) {
    if let Err(error) = remove(computer_id) {
        eprintln!("silo-engine: {error}");
    }
}

/// Changes the record of `computer_id` if it has one; whether it had one.
pub fn amend(computer_id: &str, change: impl FnOnce(&mut RecipeRecord)) -> Result<bool, String> {
    let mut found = false;
    update(|computers| {
        if let Some(record) = computers.get_mut(computer_id) {
            change(record);
            found = true;
        }
    })?;
    Ok(found)
}
