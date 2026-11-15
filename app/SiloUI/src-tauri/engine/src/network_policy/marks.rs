//! Each computer's "has a saved policy" mark, kept outside `network-policy.json` in the
//! computer's own record: a macOS computer's `computer.json` (or its import staging folder's,
//! which becomes it), else a Linux computer's checkpoint record in the runtime storage in use.
//!
//! A computer with the mark and no entry in the policy store has lost its policy: the store
//! reads it like a damaged file ([`super::store::MISSING`]). A computer without the mark and
//! without an entry is one saved before policies existed. The store sets the mark before it
//! writes an entry and clears it only after the entry is removed, so a crash in between leaves
//! a computer that does not start rather than one that runs open.

use std::path::{Path, PathBuf};

/// Where the marks are kept.
#[derive(Clone, Debug)]
pub(crate) struct Roots {
    /// The app data folder, which holds the macOS computers.
    pub app_data: PathBuf,
    /// The `checkpoints` folder of the Linux runtime storage; `None` when it cannot be found.
    pub linux_records: Option<PathBuf>,
}

#[cfg(test)]
thread_local! {
    static TEST_ROOTS: std::cell::RefCell<Option<Roots>> = const { std::cell::RefCell::new(None) };
}

/// Tests on this thread keep marks under `roots`. A test that installs a policy store path
/// without roots has no marks: every computer reads as unmarked.
#[cfg(test)]
pub(crate) fn use_test_roots(roots: Option<Roots>) {
    TEST_ROOTS.with(|test| *test.borrow_mut() = roots);
}

fn roots() -> Option<Roots> {
    #[cfg(test)]
    if super::store::uses_test_path() {
        return TEST_ROOTS.with(|test| test.borrow().clone());
    }
    let app_data = crate::context::current()?.paths().app_data.clone();
    let linux_records = crate::runtime_migration::selected_runtime_storage(&app_data)
        .ok()
        .map(|storage| storage.join("checkpoints"));
    Some(Roots {
        app_data,
        linux_records,
    })
}

fn linux_records(roots: &Roots) -> Result<&Path, String> {
    roots
        .linux_records
        .as_deref()
        .ok_or_else(|| "Silo could not find where its computers are saved.".to_owned())
}

/// Whether `computer_id` has had a saved network policy. False when no marks are kept.
pub(crate) fn is_marked(computer_id: &str) -> Result<bool, String> {
    let Some(roots) = roots() else {
        return Ok(false);
    };
    if let Some(marked) = crate::macos_computers::network_policy_mark(&roots.app_data, computer_id)?
    {
        return Ok(marked);
    }
    match roots.linux_records.as_deref() {
        Some(records) => crate::runtime::checkpoints::network_policy_mark(records, computer_id),
        None => Ok(false),
    }
}

/// Marks `computer_id` as having a saved network policy.
pub(crate) fn mark(computer_id: &str) -> Result<(), String> {
    set(computer_id, true)
}

/// Clears the mark of `computer_id`, whose policy entry is gone.
pub(crate) fn unmark(computer_id: &str) -> Result<(), String> {
    set(computer_id, false)
}

fn set(computer_id: &str, on: bool) -> Result<(), String> {
    let Some(roots) = roots() else {
        return Ok(());
    };
    if crate::macos_computers::set_network_policy_mark(&roots.app_data, computer_id, on)? {
        return Ok(());
    }
    if !on && roots.linux_records.is_none() {
        return Ok(());
    }
    crate::runtime::checkpoints::set_network_policy_mark(linux_records(&roots)?, computer_id, on)
}

/// Marks `computer_id` only when it has a record of its own: a macOS computer's, or a Linux
/// computer's checkpoint record or inventory entry. A policy entry left by a computer that no
/// longer exists makes no record.
pub(crate) fn mark_existing(computer_id: &str) -> Result<(), String> {
    let Some(roots) = roots() else {
        return Ok(());
    };
    if crate::macos_computers::set_network_policy_mark(&roots.app_data, computer_id, true)? {
        return Ok(());
    }
    let Some(records) = roots.linux_records.as_deref() else {
        return Ok(());
    };
    let listed = || {
        let inventory = records.with_file_name("computers.json");
        crate::runtime::read_metadata(&inventory).is_ok_and(|saved| {
            saved
                .computers
                .iter()
                .any(|computer| computer.id() == computer_id)
        })
    };
    if uuid::Uuid::parse_str(computer_id).is_ok()
        && (records.join(format!("{computer_id}.json")).exists() || listed())
    {
        crate::runtime::checkpoints::set_network_policy_mark(records, computer_id, true)?;
    }
    Ok(())
}
