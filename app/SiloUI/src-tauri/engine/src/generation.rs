//! A per-computer "changed since" generation.
//!
//! Every engine operation that can change a computer's state (commands, file writes,
//! keep/undo/promote, restore, start/stop, terminal and desktop sessions) moves the computer's
//! generation, and so does the end of a job that changes it. A caller that remembers the
//! generation it saw can tell later that something happened to the computer in between; SSH
//! access uses this to skip inspecting a stopped computer again until a lifecycle change.
//!
//! The generation lives in this process.

use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex, MutexGuard},
};

#[derive(Default)]
struct State {
    /// Added to every computer's own count: a change that touches computers the engine
    /// cannot name (a keep into a default target).
    epoch: u64,
    computers: HashMap<String, u64>,
    /// Job id to the computer whose state the job changes; the job's end bumps it again.
    job_computers: HashMap<String, String>,
}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(Default::default);

fn state() -> MutexGuard<'static, State> {
    STATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl State {
    fn generation(&self, computer: &str) -> u64 {
        self.epoch + self.computers.get(computer).copied().unwrap_or(0)
    }

    fn bump(&mut self, computer: &str) {
        *self.computers.entry(computer.to_owned()).or_insert(0) += 1;
    }
}

/// The computer's generation: it moves whenever the engine sees something that can change
/// the computer.
pub fn generation(computer: &str) -> u64 {
    state().generation(computer)
}

/// Records a change to `computer`.
pub fn bump(computer: &str) {
    state().bump(computer);
}

/// Records a change to every computer.
pub fn bump_all() {
    state().epoch += 1;
}

/// Work that can change a computer; it moves the generation when it starts and again when it
/// ends (when this is dropped).
pub struct Touch {
    computer: Option<String>,
}

/// Starts work on `computer`, or on a computer that is not named (every computer's
/// generation moves).
pub fn touch(computer: Option<&str>) -> Touch {
    match computer {
        Some(computer) => bump(computer),
        None => bump_all(),
    }
    Touch {
        computer: computer.map(str::to_owned),
    }
}

impl Drop for Touch {
    fn drop(&mut self) {
        match &self.computer {
            Some(computer) => bump(computer),
            None => bump_all(),
        }
    }
}

/// A job started for a change to `computer` (every computer when none is named): its end
/// moves the generation again.
pub fn note_job(job_id: &str, computer: Option<&str>) {
    state()
        .job_computers
        .insert(job_id.to_owned(), computer.unwrap_or(ALL).to_owned());
}

/// Stands for every computer in `job_computers`; no computer id is this.
const ALL: &str = "*";

/// A job finished.
pub fn job_finished(job_id: &str) {
    let mut state = state();
    if let Some(computer) = state.job_computers.remove(job_id) {
        if computer == ALL {
            state.epoch += 1;
        } else {
            state.bump(&computer);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_moves_the_generation_when_it_starts_and_when_it_ends() {
        let id = "gen-test-work";
        let before = generation(id);
        let work = touch(Some(id));
        assert!(generation(id) > before);
        let during = generation(id);
        drop(work);
        assert!(generation(id) > during);
    }

    #[test]
    fn a_finished_job_moves_its_computers_generation() {
        let id = "gen-test-job";
        note_job("job:gen-job-1", Some(id));
        let before = generation(id);
        job_finished("job:gen-job-1");
        assert!(generation(id) > before);
        note_job("job:gen-job-2", None);
        let before = generation(id);
        job_finished("job:gen-job-2");
        assert!(generation(id) > before);
    }

    #[test]
    fn an_unnamed_change_moves_every_computer() {
        let before = generation("gen-test-any");
        bump_all();
        assert!(generation("gen-test-any") > before);
    }
}
