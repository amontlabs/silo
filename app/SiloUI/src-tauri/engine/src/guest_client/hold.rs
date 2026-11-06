//! A computer's bridge folder can be put on hold while a full capture of the running computer
//! is made: the worker neither takes requests nor answers during the hold, so the engine does
//! not remove a file the guest has just looked up (which MicroSandbox refuses to capture).

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

fn held() -> &'static Mutex<HashMap<String, usize>> {
    static HELD: OnceLock<Mutex<HashMap<String, usize>>> = OnceLock::new();
    HELD.get_or_init(Default::default)
}

/// Holds the bridge folder of computer `name` until the guard is dropped.
pub struct BridgeHold(String);

pub fn hold_bridge(name: &str) -> BridgeHold {
    *held()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .entry(name.to_owned())
        .or_default() += 1;
    BridgeHold(name.to_owned())
}

impl Drop for BridgeHold {
    fn drop(&mut self) {
        let mut held = held()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(count) = held.get_mut(&self.0) {
            *count -= 1;
            if *count == 0 {
                held.remove(&self.0);
            }
        }
    }
}

/// Whether the bridge folder of computer `name` is on hold.
pub fn bridge_held(name: &str) -> bool {
    held()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .contains_key(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hold_lasts_until_every_guard_is_dropped() {
        assert!(!bridge_held("hold-test"));
        let first = hold_bridge("hold-test");
        let second = hold_bridge("hold-test");
        drop(first);
        assert!(bridge_held("hold-test"));
        drop(second);
        assert!(!bridge_held("hold-test"));
    }
}
