//! A per-computer "changed since" generation.
//!
//! Every engine operation that can change a computer's state (commands, file writes,
//! keep/undo/promote, restore, start/stop, terminal and desktop sessions, ssh sessions opened
//! through the engine) bumps the computer's generation, and the work stays "in flight" while
//! it runs. A warm checkpoint records the generation its computer had when the capture began,
//! only when nothing was in flight. A fork may reuse that checkpoint only while the
//! computer's generation is still the recorded one.
//!
//! The generation lives in this process. After an engine restart no warm checkpoint has a
//! recorded generation, so none is reused. A computer with an SSH access listener (`ssh
//! main.silo`) is in flight for as long as the listener runs, since its sessions are served
//! out of the engine's sight. Changes made over any other `ssh` session that does not go
//! through the engine cannot be seen here.

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
    in_flight: HashMap<String, u32>,
    /// Job id to the computer whose state the job changes; the job's end bumps it again.
    job_computers: HashMap<String, String>,
    /// Warm capture job id to the generation recorded for the checkpoint it makes.
    warm_jobs: HashMap<String, u64>,
    /// Warm checkpoint id to the generation its computer had when it was taken.
    warm: HashMap<String, u64>,
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

/// Work that can change a computer is under way until this is dropped; it also bumps the
/// generation when it starts and again when it ends.
pub struct Touch {
    computer: Option<String>,
}

/// Starts work on `computer`, or on a computer that is not named (every computer's
/// generation moves and nothing is held in flight).
pub fn touch(computer: Option<&str>) -> Touch {
    let mut state = state();
    match computer {
        Some(computer) => {
            state.bump(computer);
            *state.in_flight.entry(computer.to_owned()).or_insert(0) += 1;
        }
        None => state.epoch += 1,
    }
    Touch {
        computer: computer.map(str::to_owned),
    }
}

impl Drop for Touch {
    fn drop(&mut self) {
        let mut state = state();
        let Some(computer) = &self.computer else {
            state.epoch += 1;
            return;
        };
        state.bump(computer);
        if let Some(count) = state.in_flight.get_mut(computer) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                state.in_flight.remove(computer);
            }
        }
    }
}

/// Whether nothing the engine sees is working on `computer` right now.
pub fn idle(computer: &str) -> bool {
    !state().in_flight.contains_key(computer)
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

/// What a warm checkpoint capture will record: the computer's generation, read before the
/// capture starts, and only when nothing was in flight on the computer.
pub struct WarmBaseline(Option<u64>);

/// Reads the baseline for a warm capture of `computer`; take it before starting the capture.
pub fn warm_baseline(computer: &str) -> WarmBaseline {
    let state = state();
    WarmBaseline((!state.in_flight.contains_key(computer)).then(|| state.generation(computer)))
}

/// The warm capture was accepted as job `job_id`: the checkpoint it makes gets the baseline.
pub fn note_warm_job(job_id: &str, baseline: WarmBaseline) {
    if let WarmBaseline(Some(generation)) = baseline {
        state().warm_jobs.insert(job_id.to_owned(), generation);
    }
}

/// A job finished. `checkpoint_id` is the checkpoint the job made, when it made one.
pub fn job_finished(job_id: &str, checkpoint_id: Option<&str>) {
    let mut state = state();
    if let Some(computer) = state.job_computers.remove(job_id) {
        if computer == ALL {
            state.epoch += 1;
        } else {
            state.bump(&computer);
        }
    }
    if let Some(recorded) = state.warm_jobs.remove(job_id) {
        if let Some(checkpoint) = checkpoint_id {
            state.warm.insert(checkpoint.to_owned(), recorded);
        }
    }
}

/// The generation the warm checkpoint `checkpoint_id` was taken at, when the engine recorded
/// one.
pub fn warm_generation(checkpoint_id: &str) -> Option<u64> {
    state().warm.get(checkpoint_id).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_moves_the_generation_and_is_in_flight_until_it_ends() {
        let id = "gen-test-work";
        let before = generation(id);
        let work = touch(Some(id));
        assert!(!idle(id));
        assert!(generation(id) > before);
        let during = generation(id);
        drop(work);
        assert!(idle(id));
        assert!(generation(id) > during);
    }

    #[test]
    fn a_warm_checkpoint_keeps_the_generation_of_a_quiet_computer() {
        let id = "gen-test-warm-quiet";
        drop(touch(Some(id)));
        let baseline = warm_baseline(id);
        let at = generation(id);
        note_warm_job("job:gen-warm-1", baseline);
        job_finished("job:gen-warm-1", Some("cp-gen-warm-1"));
        assert_eq!(warm_generation("cp-gen-warm-1"), Some(at));
        bump(id);
        assert_ne!(warm_generation("cp-gen-warm-1"), Some(generation(id)));
    }

    #[test]
    fn a_warm_checkpoint_taken_while_work_runs_records_nothing() {
        let id = "gen-test-warm-busy";
        let _work = touch(Some(id));
        note_warm_job("job:gen-warm-2", warm_baseline(id));
        job_finished("job:gen-warm-2", Some("cp-gen-warm-2"));
        assert_eq!(warm_generation("cp-gen-warm-2"), None);
    }

    #[test]
    fn a_finished_job_moves_its_computers_generation() {
        let id = "gen-test-job";
        note_job("job:gen-job-1", Some(id));
        let before = generation(id);
        job_finished("job:gen-job-1", None);
        assert!(generation(id) > before);
        note_job("job:gen-job-2", None);
        let before = generation(id);
        job_finished("job:gen-job-2", None);
        assert!(generation(id) > before);
    }

    #[test]
    fn an_unnamed_change_moves_every_computer() {
        let before = generation("gen-test-any");
        bump_all();
        assert!(generation("gen-test-any") > before);
    }
}
