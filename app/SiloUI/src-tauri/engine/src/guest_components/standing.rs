//! What this device remembers, only while it runs, about how the looks at one computer have
//! gone: whether a program the owner named for development is in it, and how many looks in a
//! row failed. A start of the computer, or Silo's tools being turned on again, begins afresh.

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

/// Looks in a row that may fail before the watcher stops looking on its own.
pub const MAX_FAILURES: u32 = 5;

#[derive(Default)]
struct Standing {
    /// A program the owner named stays in place of Silo's own.
    program_named: bool,
    failures: u32,
}

fn with<R>(computer_id: &str, change: impl FnOnce(&mut Standing) -> R) -> R {
    static STANDING: OnceLock<Mutex<HashMap<String, Standing>>> = OnceLock::new();
    let mut all = STANDING
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let result = change(all.entry(computer_id.to_owned()).or_default());
    all.retain(|_, standing| standing.program_named || standing.failures > 0);
    result
}

/// Whether a program the owner named is in the computer and not to be replaced by a look.
pub fn program_named(computer_id: &str) -> bool {
    with(computer_id, |standing| standing.program_named)
}

pub fn name_program(computer_id: &str) {
    with(computer_id, |standing| standing.program_named = true);
}

/// Notes how a look went.
pub fn note_look(computer_id: &str, failed: bool) {
    with(computer_id, |standing| {
        standing.failures = if failed {
            standing.failures.saturating_add(1)
        } else {
            0
        };
    });
}

/// Whether so many looks in a row failed that none is made until a start or the tools are
/// turned on again.
pub fn given_up(computer_id: &str) -> bool {
    with(computer_id, |standing| standing.failures >= MAX_FAILURES)
}

/// Forgets all of it: the computer started, or its tools were turned on, or it is gone.
pub fn start_over(computer_id: &str) {
    with(computer_id, |standing| *standing = Standing::default());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failures_in_a_row_give_up_and_a_good_look_or_a_start_begins_afresh() {
        let id = "standing-failures";
        for _ in 0..MAX_FAILURES - 1 {
            note_look(id, true);
        }
        assert!(!given_up(id));
        note_look(id, true);
        assert!(given_up(id));
        start_over(id);
        assert!(!given_up(id));
        note_look(id, true);
        note_look(id, false);
        for _ in 0..MAX_FAILURES - 1 {
            note_look(id, true);
        }
        assert!(!given_up(id));
    }

    #[test]
    fn a_named_program_stays_until_a_start() {
        let id = "standing-program";
        assert!(!program_named(id));
        name_program(id);
        assert!(program_named(id));
        note_look(id, false);
        assert!(program_named(id));
        start_over(id);
        assert!(!program_named(id));
    }
}
