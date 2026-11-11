//! The identity swap at the end of a rebuild: the rebuilt sandbox takes the computer's name and
//! id, and the old sandbox becomes `<name>-undo` under a new id.
//!
//! Sandboxes are told apart by the Silo identity they carry (`silo.machine-id`), never by their
//! names, because the names are what a crash can leave half moved. The two sandboxes (`old`
//! and `new`) and the three names (`n` = the computer's, `u` = the undo name, `r` = the build
//! name) are always in one of five states, read from who owns each name:
//!
//! | state | `n` | `u` | `r` |
//! | --- | --- | --- | --- |
//! | 0 before | old id | nobody | new id |
//! | 1 old renamed | nobody | old id | new id |
//! | 2 old relabelled | nobody | undo id | new id |
//! | 3 new renamed | new id | undo id | nobody |
//! | 4 after | old id | undo id | nobody |
//!
//! `apply` moves one state forward at a time up to 4 and `revert` one back down to 0; each
//! repeats harmlessly. Any other combination is refused, and nothing more is changed.

use super::journal::Plan;
use crate::runtime::ComputerConfiguration;

/// What the swap needs from the runtime's sandboxes. All of them are stopped.
pub trait Sandboxes {
    /// The Silo identity of the sandbox called `name` (empty when it has none), or `None` when
    /// no sandbox has that name.
    fn owner(&self, name: &str) -> Result<Option<String>, String>;
    fn rename(&self, from: &str, to: &str) -> Result<(), String>;
    /// Gives the stopped sandbox called `name` the identity `id`.
    fn relabel(&self, name: &str, id: &str) -> Result<(), String>;
}

/// The state (0 to 4) the sandboxes are in, or why they are in none of them.
pub fn state(sandboxes: &dyn Sandboxes, plan: &Plan) -> Result<u8, String> {
    let n = sandboxes.owner(&plan.old_name)?;
    let u = sandboxes.owner(&plan.undo_name)?;
    let r = sandboxes.owner(&plan.new_name)?;
    let is = |owner: &Option<String>, id: &str| owner.as_deref() == Some(id);
    let found = match (&n, &u, &r) {
        (n, None, r) if is(n, &plan.old_id) && is(r, &plan.new_id) => Some(0),
        (None, u, r) if is(u, &plan.old_id) && is(r, &plan.new_id) => Some(1),
        (None, u, r) if is(u, &plan.undo_id) && is(r, &plan.new_id) => Some(2),
        (n, u, None) if is(n, &plan.new_id) && is(u, &plan.undo_id) => Some(3),
        (n, u, None) if is(n, &plan.old_id) && is(u, &plan.undo_id) => Some(4),
        _ => None,
    };
    found.ok_or_else(|| {
        format!(
            "{}, {} and {} are not the computers the rebuild expected. Nothing more was changed.",
            plan.old_name, plan.undo_name, plan.new_name
        )
    })
}

/// Moves the sandboxes forward to state 4.
pub fn apply(sandboxes: &dyn Sandboxes, plan: &Plan) -> Result<(), String> {
    loop {
        match state(sandboxes, plan)? {
            0 => sandboxes.rename(&plan.old_name, &plan.undo_name)?,
            1 => sandboxes.relabel(&plan.undo_name, &plan.undo_id)?,
            2 => sandboxes.rename(&plan.new_name, &plan.old_name)?,
            3 => sandboxes.relabel(&plan.old_name, &plan.old_id)?,
            _ => return Ok(()),
        }
    }
}

/// Moves the sandboxes back to state 0.
pub fn revert(sandboxes: &dyn Sandboxes, plan: &Plan) -> Result<(), String> {
    loop {
        match state(sandboxes, plan)? {
            4 => sandboxes.relabel(&plan.old_name, &plan.new_id)?,
            3 => sandboxes.rename(&plan.old_name, &plan.new_name)?,
            2 => sandboxes.relabel(&plan.undo_name, &plan.old_id)?,
            1 => sandboxes.rename(&plan.undo_name, &plan.old_name)?,
            _ => return Ok(()),
        }
    }
}

fn copy_of(
    computers: &[ComputerConfiguration],
    id: &str,
    new_id: &str,
    new_name: &str,
) -> Option<ComputerConfiguration> {
    computers.iter().find(|c| c.id() == id).map(|source| {
        let mut copy = source.clone();
        copy.id = new_id.to_owned();
        copy.name = new_name.to_owned();
        copy
    })
}

fn position(computers: &[ComputerConfiguration], id: &str) -> usize {
    computers
        .iter()
        .position(|c| c.id() == id)
        .map_or(computers.len(), |at| at + 1)
}

/// The saved computers after the swap: the computer keeps its entry (the rebuilt sandbox
/// carries its id and name), the undo computer gets a copy of it under the undo id, and the
/// entry the rebuilt sandbox was built under goes. Repeating it changes nothing.
pub fn swap_inventory(computers: &mut Vec<ComputerConfiguration>, plan: &Plan) {
    if !computers.iter().any(|c| c.id() == plan.undo_id) {
        if let Some(undo) = copy_of(computers, &plan.old_id, &plan.undo_id, &plan.undo_name) {
            let at = position(computers, &plan.old_id);
            computers.insert(at, undo);
        }
    }
    computers.retain(|c| c.id() != plan.new_id);
}

/// The saved computers before the swap: the undo entry goes and the build entry is back, so a
/// removal of the build computer finds it. Repeating it changes nothing.
pub fn unswap_inventory(computers: &mut Vec<ComputerConfiguration>, plan: &Plan) {
    computers.retain(|c| c.id() != plan.undo_id);
    if !computers.iter().any(|c| c.id() == plan.new_id) {
        if let Some(build) = copy_of(computers, &plan.old_id, &plan.new_id, &plan.new_name) {
            let at = position(computers, &plan.old_id);
            computers.insert(at, build);
        }
    }
}
