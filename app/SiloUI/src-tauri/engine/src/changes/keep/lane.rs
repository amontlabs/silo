//! One keep or undo at a time per target computer. A second one waits up to a bound, then
//! fails with a busy refusal instead of interleaving with the first.

use std::{
    collections::HashSet,
    sync::{Condvar, Mutex, OnceLock},
    time::{Duration, Instant},
};

struct Lanes {
    held: Mutex<HashSet<String>>,
    freed: Condvar,
}

fn lanes() -> &'static Lanes {
    static LANES: OnceLock<Lanes> = OnceLock::new();
    LANES.get_or_init(|| Lanes {
        held: Mutex::new(HashSet::new()),
        freed: Condvar::new(),
    })
}

pub struct LaneGuard {
    key: String,
}

impl Drop for LaneGuard {
    fn drop(&mut self) {
        let lanes = lanes();
        lanes
            .held
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&self.key);
        lanes.freed.notify_all();
    }
}

/// Takes the target's lane, waiting at most `wait`.
pub fn acquire(target: &str, wait: Duration) -> Result<LaneGuard, String> {
    let lanes = lanes();
    let deadline = Instant::now() + wait;
    let mut held = lanes
        .held
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    while held.contains(target) {
        let now = Instant::now();
        if now >= deadline {
            return Err(format!(
                "busy: another keep or undo is still changing {target}."
            ));
        }
        held = lanes
            .freed
            .wait_timeout(held, deadline - now)
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .0;
    }
    held.insert(target.to_owned());
    Ok(LaneGuard {
        key: target.to_owned(),
    })
}
