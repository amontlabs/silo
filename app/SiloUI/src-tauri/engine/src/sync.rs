//! Mutex poison policy (K-24; see `docs/SiloUI-REVIEW-DESIGN-NOTES.md`).
//!
//! A panic while a lock is held poisons it. Mapping that to an error turns one
//! panic into failures until Silo restarts, and a `try_lock` update guard would
//! then report "busy" forever. The rule for using these helpers:
//!
//! - Recover `Mutex<()>` serialisation locks and caches rebuilt from disk.
//! - For guards of state persisted to disk, recover and re-read the file before
//!   use (every current caller loads the file after taking the lock).
//! - Never keep an in-memory invariant across a panic: data that cannot be
//!   rebuilt needs its own reset instead of these helpers.
//!
//! Recovery clears the poison, so each panic is logged once.
use std::sync::{Mutex, MutexGuard, TryLockError};

fn recovered(name: &str) {
    eprintln!("Silo recovered the {name} lock after a panic while it was held.");
}

/// Lock `mutex`, recovering it if a panic poisoned it. `name` identifies the lock
/// in the log line written once per recovery.
pub(crate) fn lock_or_recover<'a, T: ?Sized>(mutex: &'a Mutex<T>, name: &str) -> MutexGuard<'a, T> {
    mutex.lock().unwrap_or_else(|poisoned| {
        recovered(name);
        mutex.clear_poison();
        poisoned.into_inner()
    })
}

/// Like `lock_or_recover` without waiting: `None` only while another thread holds
/// the lock, never because an earlier holder panicked.
pub(crate) fn try_lock_or_recover<'a, T: ?Sized>(
    mutex: &'a Mutex<T>,
    name: &str,
) -> Option<MutexGuard<'a, T>> {
    match mutex.try_lock() {
        Ok(guard) => Some(guard),
        Err(TryLockError::Poisoned(poisoned)) => {
            recovered(name);
            mutex.clear_poison();
            Some(poisoned.into_inner())
        }
        Err(TryLockError::WouldBlock) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn poison<T: Send + 'static>(mutex: &'static Mutex<T>) {
        let _ = std::thread::spawn(move || {
            let _guard = mutex.lock().unwrap();
            panic!("simulated panic while holding the lock");
        })
        .join();
        assert!(mutex.is_poisoned());
    }

    #[test]
    fn a_poisoned_lock_is_recovered_once_and_keeps_working() {
        static LOCK: Mutex<u32> = Mutex::new(7);
        poison(&LOCK);
        *lock_or_recover(&LOCK, "test") += 1;
        assert!(!LOCK.is_poisoned(), "recovery clears the poison");
        assert_eq!(*lock_or_recover(&LOCK, "test"), 8);
    }

    #[test]
    fn try_lock_recovers_poison_but_still_reports_a_held_lock() {
        static LOCK: Mutex<()> = Mutex::new(());
        poison(&LOCK);
        assert!(try_lock_or_recover(&LOCK, "test").is_some());
        assert!(!LOCK.is_poisoned());
        let held = LOCK.lock().unwrap();
        let busy = std::thread::spawn(|| try_lock_or_recover(&LOCK, "test").is_none())
            .join()
            .unwrap();
        assert!(busy);
        drop(held);
    }
}
