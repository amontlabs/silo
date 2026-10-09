//! Computer names are unique across the Linux and macOS computers of a device.
//! The two kinds are stored apart and changed under different locks, so a name is
//! reserved here, atomically with the check that no other kind uses it, before either
//! side records it. A reservation lasts until its holder has committed the computer
//! (or given up), after which the committed record itself is what others find.
//!
//! The lock is held only for the check and the insertion, never while a computer is
//! being created, and nothing is acquired under it except file reads. Linux creation
//! takes its operation gate first and then this lock; macOS creation takes only this
//! lock, so the order cannot invert.
use std::sync::Mutex;

pub(crate) const TAKEN: &str = "Computer names must be unique.";

static RESERVED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Names held for computers being created. Releases them when dropped.
#[derive(Debug)]
pub(crate) struct Reservation(Vec<String>);

/// Reserves `names` unless one is reserved already or `taken` (read while holding the
/// lock, so a commit that finished meanwhile is seen) lists it. Names compare in lowercase.
pub(crate) fn reserve(
    names: &[String],
    taken: &dyn Fn() -> Vec<String>,
) -> Result<Reservation, String> {
    let mut reserved = RESERVED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let names: Vec<String> = names.iter().map(|name| name.to_ascii_lowercase()).collect();
    if names.is_empty() {
        return Ok(Reservation(names));
    }
    let used = taken();
    if names.iter().any(|name| {
        reserved.contains(name) || used.iter().any(|other| other.eq_ignore_ascii_case(name))
    }) {
        return Err(TAKEN.into());
    }
    reserved.extend(names.iter().cloned());
    Ok(Reservation(names))
}

impl Drop for Reservation {
    fn drop(&mut self) {
        let mut reserved = RESERVED
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for name in &self.0 {
            if let Some(index) = reserved.iter().position(|held| held == name) {
                reserved.remove(index);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none() -> Vec<String> {
        Vec::new()
    }

    #[test]
    fn a_name_used_by_another_kind_is_refused_whatever_its_case() {
        let taken = || vec!["web".to_string()];
        assert!(reserve(&["cn-api".into()], &taken).is_ok());
        assert_eq!(
            reserve(&["cn-web".into()], &|| vec!["cn-web".into()]).unwrap_err(),
            TAKEN
        );
        assert!(reserve(&["CN-WEB2".into()], &|| vec!["cn-web2".into()]).is_err());
    }

    #[test]
    fn two_creations_cannot_both_hold_a_name() {
        let first = reserve(&["cn-shared".into()], &none).unwrap();
        // Neither kind's record exists yet, so only the reservation can refuse it.
        assert_eq!(reserve(&["cn-shared".into()], &none).unwrap_err(), TAKEN);
        assert_eq!(reserve(&["CN-SHARED".into()], &none).unwrap_err(), TAKEN);
        drop(first);
        // Once the holder is done and its record is visible, the record refuses it.
        assert!(reserve(&["cn-shared".into()], &none).is_ok());
        assert!(reserve(&["cn-shared".into()], &|| vec!["cn-shared".into()]).is_err());
    }

    #[test]
    fn a_refused_reservation_holds_nothing() {
        let _held = reserve(&["cn-held".into()], &none).unwrap();
        assert!(reserve(&["cn-free".into(), "cn-held".into()], &none).is_err());
        assert!(reserve(&["cn-free".into()], &none).is_ok());
    }

    #[test]
    fn the_taken_names_are_read_under_the_lock() {
        // A commit that lands between the caller's decision and the reservation is seen.
        let calls = std::cell::Cell::new(0);
        let result = reserve(&["cn-late".into()], &|| {
            calls.set(calls.get() + 1);
            vec!["cn-late".into()]
        });
        assert!(result.is_err());
        assert_eq!(calls.get(), 1);
    }
}
