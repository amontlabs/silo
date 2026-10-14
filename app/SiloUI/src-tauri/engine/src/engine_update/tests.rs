//! Journal, transaction and lifecycle tests over a deterministic fake of both platforms.
//!
//! The fake world keeps what physically exists: a macOS machine lives in the engine process
//! and dies with it (`crash`), while a Linux runtime is a detached process that survives the
//! engine. No code path of the fake can start a computer cold, so a computer that ends up
//! running without a `restore` or a `resume_held` call is a test failure.
use std::{
    collections::{BTreeMap, HashSet},
    panic::{catch_unwind, AssertUnwindSafe},
    path::Path,
    sync::{Arc, Mutex},
    time::Instant,
};

use silo_api::{
    jobs::JobId,
    methods::engine::{ShutdownPhase, ShutdownReason},
};

use super::{
    adapter::{
        OriginalState, Reconciled, RestoreError, ResumeHeldError, RunningComputer, SaveAdapter,
        SaveContext,
    },
    journal::{
        CaptureRef, ComputerKind, ComputerPhase, Fault, LinuxCapture, MacCapture, RuntimeIdentity,
        Store, TransactionPhase,
    },
    transaction::{Coordinator, PrepareRequest},
    HeldGuards, Hold, LifecycleError, Platform, PrepareStart, ShutdownRequest, ShutdownStart,
    UpdateState,
};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Machine {
    Running,
    /// Paused by `save_and_hold` and still owned by the engine that saved it.
    Held,
    /// A Linux runtime left paused after its engine released it.
    Detached,
    /// A macOS machine whose engine process ended.
    Gone,
    /// Executing again after a restore.
    Executing(String),
}

#[derive(Default)]
struct World {
    machines: BTreeMap<String, (ComputerKind, Machine)>,
    calls: Vec<String>,
    unsupported: HashSet<String>,
    fail_save: HashSet<String>,
    fail_resume_held: HashSet<String>,
    fail_restore: HashSet<String>,
    fail_after_arm: HashSet<String>,
    stop_failure: Option<String>,
    saved_before_first_pause: Option<bool>,
    /// How many runs each computer has had; a restore starts a new one.
    runs: BTreeMap<String, u64>,
    /// Computers whose guest executed since their capture was made. Restoring one of them
    /// from that capture would replay it over an advanced disk.
    advanced: HashSet<String>,
    replayed: Vec<String>,
}

type Shared = Arc<Mutex<World>>;

fn name(id: &str) -> String {
    format!("computer-{id}")
}

fn world(computers: &[(&str, ComputerKind, Machine)]) -> Shared {
    let mut world = World::default();
    for (id, kind, machine) in computers {
        world
            .machines
            .insert((*id).into(), (*kind, machine.clone()));
        world.runs.insert((*id).into(), 1);
    }
    Arc::new(Mutex::new(world))
}

impl World {
    /// The engine process ends.
    fn crash(&mut self) {
        for (kind, machine) in self.machines.values_mut() {
            *machine = match (*kind, machine.clone()) {
                (ComputerKind::Macos, _) => Machine::Gone,
                (ComputerKind::Linux, Machine::Held) => Machine::Detached,
                (ComputerKind::Linux, other) => other,
            };
        }
    }

    fn run(&self, id: &str) -> RuntimeIdentity {
        RuntimeIdentity {
            owner: None,
            generation: None,
            instance: Some(format!("{id}#{}", self.runs[id])),
        }
    }

    fn count(&self, prefix: &str) -> usize {
        self.calls
            .iter()
            .filter(|call| call.starts_with(prefix))
            .count()
    }
}

struct Fake {
    kind: ComputerKind,
    world: Shared,
    store_directory: std::path::PathBuf,
}

impl Fake {
    fn all(world: &Shared, store: &Store) -> Vec<Box<dyn SaveAdapter>> {
        [ComputerKind::Macos, ComputerKind::Linux]
            .into_iter()
            .map(|kind| {
                Box::new(Fake {
                    kind,
                    world: world.clone(),
                    store_directory: store.directory().to_path_buf(),
                }) as Box<dyn SaveAdapter>
            })
            .collect()
    }

    fn capture(&self, computer: &RunningComputer, context: &SaveContext<'_>) -> CaptureRef {
        match self.kind {
            ComputerKind::Macos => {
                let directory = context
                    .saves_root
                    .join(context.transaction_id)
                    .join(&computer.id);
                std::fs::create_dir_all(&directory).unwrap();
                std::fs::write(directory.join("state"), b"memory").unwrap();
                CaptureRef::Macos(MacCapture {
                    state_file: directory.join("state"),
                    disk: directory.join("disk"),
                    auxiliary_storage: directory.join("aux"),
                    directory,
                    source_generation: 1,
                    host_build: "25F71".into(),
                    save_format_version: 1,
                    artifact_sizes: BTreeMap::new(),
                })
            }
            ComputerKind::Linux => CaptureRef::Linux(LinuxCapture {
                group: "group".into(),
                member: format!("c{}", computer.id),
                scope: "full".into(),
                network_policy: serde_json::json!({}),
                integrity_verified: true,
                runtime_instance: None,
            }),
        }
    }
}

impl SaveAdapter for Fake {
    fn kind(&self) -> ComputerKind {
        self.kind
    }

    fn running(&self) -> Result<Vec<RunningComputer>, String> {
        let world = self.world.lock().unwrap();
        Ok(world
            .machines
            .iter()
            .filter(|(_, (kind, machine))| *kind == self.kind && *machine == Machine::Running)
            .map(|(id, (kind, _))| RunningComputer {
                id: id.clone(),
                name: name(id),
                kind: *kind,
            })
            .collect())
    }

    fn identity(&self, computer: &RunningComputer) -> Result<RuntimeIdentity, String> {
        Ok(self.world.lock().unwrap().run(&computer.id))
    }

    fn check_support(&self, computer: &RunningComputer) -> Result<(), String> {
        if self
            .world
            .lock()
            .unwrap()
            .unsupported
            .contains(&computer.id)
        {
            Err("its memory can't be saved".into())
        } else {
            Ok(())
        }
    }

    fn save_and_hold(
        &self,
        computer: &RunningComputer,
        context: &SaveContext<'_>,
    ) -> Result<CaptureRef, String> {
        let mut world = self.world.lock().unwrap();
        if world.saved_before_first_pause.is_none() {
            let journal = Store::new(&self.store_directory).load().unwrap();
            world.saved_before_first_pause = Some(journal.is_some_and(|journal| {
                journal.computers.len()
                    == world
                        .machines
                        .values()
                        .filter(|(_, m)| *m == Machine::Running)
                        .count()
            }));
        }
        world.calls.push(format!("save:{}", computer.id));
        if world.fail_save.contains(&computer.id) {
            return Err("the save failed".into());
        }
        world.machines.get_mut(&computer.id).unwrap().1 = Machine::Held;
        Ok(self.capture(computer, context))
    }

    fn resume_held(
        &self,
        computer: &RunningComputer,
        original: &RuntimeIdentity,
    ) -> Result<(), ResumeHeldError> {
        let mut world = self.world.lock().unwrap();
        world.calls.push(format!("resume-held:{}", computer.id));
        if world.fail_resume_held.contains(&computer.id) {
            return Err(ResumeHeldError::Failed("could not resume".into()));
        }
        if !original.matches(&world.run(&computer.id)) {
            return Err(ResumeHeldError::Gone("a different run".into()));
        }
        let world = &mut *world;
        let advanced = &mut world.advanced;
        let machine = &mut world.machines.get_mut(&computer.id).unwrap().1;
        match machine {
            Machine::Running | Machine::Executing(_) => Ok(()),
            Machine::Held | Machine::Detached => {
                *machine = Machine::Running;
                advanced.insert(computer.id.clone());
                Ok(())
            }
            Machine::Gone => Err(ResumeHeldError::Gone("the machine is gone".into())),
        }
    }

    fn probe_original(
        &self,
        computer: &RunningComputer,
        original: &RuntimeIdentity,
    ) -> Result<OriginalState, String> {
        let mut world = self.world.lock().unwrap();
        world.calls.push(format!("probe-original:{}", computer.id));
        if !original.matches(&world.run(&computer.id)) {
            return Ok(OriginalState::Gone);
        }
        Ok(match &world.machines[&computer.id].1 {
            Machine::Running => OriginalState::Running,
            Machine::Held | Machine::Detached => OriginalState::Held,
            Machine::Gone | Machine::Executing(_) => OriginalState::Gone,
        })
    }

    fn release(
        &self,
        computer: &RunningComputer,
        _capture: &CaptureRef,
        _original: &RuntimeIdentity,
    ) -> Result<(), String> {
        let mut world = self.world.lock().unwrap();
        world.calls.push(format!("release:{}", computer.id));
        let (kind, machine) = world.machines.get_mut(&computer.id).unwrap();
        // As in production: a released macOS machine stays paused and owned by this process
        // until the process exits (`crash`); a Linux runtime is detached.
        *machine = match kind {
            ComputerKind::Macos => Machine::Held,
            ComputerKind::Linux => Machine::Detached,
        };
        Ok(())
    }

    fn reconcile_unsaved(
        &self,
        computer: &RunningComputer,
        _original: &RuntimeIdentity,
    ) -> Result<Reconciled, String> {
        let mut world = self.world.lock().unwrap();
        world.calls.push(format!("reconcile:{}", computer.id));
        let machine = &mut world.machines.get_mut(&computer.id).unwrap().1;
        match machine {
            Machine::Running | Machine::Executing(_) => Ok(Reconciled::Running),
            Machine::Held | Machine::Detached => {
                *machine = Machine::Running;
                Ok(Reconciled::Running)
            }
            Machine::Gone => Ok(Reconciled::Lost("its memory was not saved".into())),
        }
    }

    fn restore(
        &self,
        computer: &RunningComputer,
        _capture: &CaptureRef,
        original: &RuntimeIdentity,
        attempt_id: &str,
        arm: &mut dyn FnMut(Option<&RuntimeIdentity>) -> Result<(), String>,
    ) -> Result<(), RestoreError> {
        let identity = {
            let mut world = self.world.lock().unwrap();
            world.calls.push(format!("restore:{}", computer.id));
            if world.advanced.contains(&computer.id) {
                world.replayed.push(computer.id.clone());
            }
            if world.fail_restore.contains(&computer.id) {
                return Err(RestoreError::Failed("the saved state was refused".into()));
            }
            match &world.machines[&computer.id].1 {
                Machine::Gone | Machine::Detached | Machine::Held => {}
                other => {
                    return Err(RestoreError::Superseded(format!(
                        "a run the update did not start is {other:?}"
                    )))
                }
            }
            // A detached Linux runtime resumes the original run; otherwise a new run starts.
            if original.matches(&world.run(&computer.id))
                && world.machines[&computer.id].0 == ComputerKind::Linux
            {
                world.run(&computer.id)
            } else {
                *world.runs.get_mut(&computer.id).unwrap() += 1;
                world.run(&computer.id)
            }
        };
        arm(Some(&identity))?;
        let mut world = self.world.lock().unwrap();
        world.machines.get_mut(&computer.id).unwrap().1 = Machine::Executing(attempt_id.into());
        world.advanced.insert(computer.id.clone());
        if world.fail_after_arm.contains(&computer.id) {
            return Err(RestoreError::Failed("the connection dropped".into()));
        }
        Ok(())
    }

    fn probe_armed(
        &self,
        computer: &RunningComputer,
        _capture: &CaptureRef,
        _original: &RuntimeIdentity,
        attempt_id: &str,
        expected: Option<&RuntimeIdentity>,
    ) -> Result<bool, String> {
        let world = self.world.lock().unwrap();
        Ok(
            world.machines[&computer.id].1 == Machine::Executing(attempt_id.into())
                && expected.is_none_or(|expected| expected.matches(&world.run(&computer.id))),
        )
    }

    fn discard(&self, computer: &RunningComputer, capture: &CaptureRef) -> Result<(), String> {
        self.world
            .lock()
            .unwrap()
            .calls
            .push(format!("discard:{}", computer.id));
        if let CaptureRef::Macos(capture) = capture {
            let _ = std::fs::remove_dir_all(&capture.directory);
        }
        Ok(())
    }

    fn owns_live_machines(&self) -> bool {
        self.kind == ComputerKind::Macos
            && self
                .world
                .lock()
                .unwrap()
                .machines
                .values()
                .any(|(kind, machine)| {
                    *kind == ComputerKind::Macos
                        && matches!(machine, Machine::Running | Machine::Held)
                })
    }
}

fn request() -> PrepareRequest {
    PrepareRequest {
        transaction_id: "tx-1".into(),
        prepare_job_id: "job:1".into(),
        source_engine_version: "1.0.0".into(),
        host_build: "25F71".into(),
    }
}

const A: &str = "upd-a";
const B: &str = "upd-b";
const C: &str = "upd-c";

fn two_macs_and_a_linux() -> Shared {
    world(&[
        (A, ComputerKind::Macos, Machine::Running),
        (B, ComputerKind::Macos, Machine::Running),
        (C, ComputerKind::Linux, Machine::Running),
        ("stopped", ComputerKind::Linux, Machine::Gone),
    ])
}

fn machine(world: &Shared, id: &str) -> Machine {
    world.lock().unwrap().machines[id].1.clone()
}

fn executing(world: &Shared, id: &str) -> bool {
    matches!(machine(world, id), Machine::Executing(_))
}

#[test]
fn prepare_journals_the_running_set_before_pausing_then_saves_each_computer() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::new(directory.path());
    let world = two_macs_and_a_linux();
    let adapters = Fake::all(&world, &store);
    let coordinator = Coordinator {
        store: &store,
        adapters: &adapters,
    };
    let progress = Mutex::new(Vec::new());
    let resuming = coordinator
        .prepare(request(), &|index, total, name| {
            progress
                .lock()
                .unwrap()
                .push((index, total, name.to_owned()))
        })
        .unwrap();
    assert_eq!(
        resuming.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
        [A, B, C],
        "stopped computers are not part of the set"
    );
    assert_eq!(
        world.lock().unwrap().saved_before_first_pause,
        Some(true),
        "the complete running set was durable before the first pause"
    );
    assert_eq!(progress.lock().unwrap().len(), 3);
    let journal = store.load().unwrap().unwrap();
    assert_eq!(journal.phase, TransactionPhase::Prepared);
    assert!(journal
        .computers
        .iter()
        .all(|c| c.phase == ComputerPhase::Saved && c.capture.is_some()));
    assert!(coordinator.prepared().unwrap());
    for id in [A, B, C] {
        assert_eq!(machine(&world, id), Machine::Held);
    }
    // A second preparation is refused while the journal exists.
    let error = coordinator.prepare(request(), &|_, _, _| {}).unwrap_err();
    assert!(error.contains("previous update still has computers to resume"));
}

#[test]
fn a_computer_that_cannot_be_saved_refuses_the_update_before_anything_pauses() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::new(directory.path());
    let world = two_macs_and_a_linux();
    world.lock().unwrap().unsupported.insert(B.into());
    let adapters = Fake::all(&world, &store);
    let coordinator = Coordinator {
        store: &store,
        adapters: &adapters,
    };
    let error = coordinator.prepare(request(), &|_, _, _| {}).unwrap_err();
    assert!(error.contains("computer-upd-b can't be saved"), "{error}");
    assert!(world.lock().unwrap().calls.is_empty());
    assert!(!store.exists());
    for id in [A, B, C] {
        assert_eq!(machine(&world, id), Machine::Running);
    }
}

#[test]
fn a_failed_save_resumes_every_original_generation_and_clears_the_journal() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::new(directory.path());
    let world = two_macs_and_a_linux();
    world.lock().unwrap().fail_save.insert(B.into());
    let adapters = Fake::all(&world, &store);
    let coordinator = Coordinator {
        store: &store,
        adapters: &adapters,
    };
    let error = coordinator.prepare(request(), &|_, _, _| {}).unwrap_err();
    assert_eq!(error, "the save failed");
    for id in [A, B, C] {
        assert_eq!(machine(&world, id), Machine::Running, "{id}");
    }
    assert!(!store.exists());
    assert!(
        !store.saves_root().join("tx-1").exists(),
        "captures were removed"
    );
    assert_eq!(world.lock().unwrap().count("restore"), 0);
}

#[test]
fn a_resume_failure_keeps_the_journal_and_abort_retries_it() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::new(directory.path());
    let world = two_macs_and_a_linux();
    {
        let mut state = world.lock().unwrap();
        state.fail_save.insert(B.into());
        state.fail_resume_held.insert(A.into());
    }
    let adapters = Fake::all(&world, &store);
    let coordinator = Coordinator {
        store: &store,
        adapters: &adapters,
    };
    let error = coordinator.prepare(request(), &|_, _, _| {}).unwrap_err();
    assert!(
        error.contains("Computers could not resume: computer-upd-a"),
        "{error}"
    );
    let journal = store.load().unwrap().unwrap();
    assert_eq!(journal.phase, TransactionPhase::Failed);
    assert_eq!(machine(&world, A), Machine::Held);
    world.lock().unwrap().fail_resume_held.clear();
    coordinator.abort().unwrap();
    assert_eq!(machine(&world, A), Machine::Running);
    assert!(!store.exists());
}

#[test]
fn abort_resumes_the_exact_saved_set_and_is_idempotent() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::new(directory.path());
    let world = two_macs_and_a_linux();
    let adapters = Fake::all(&world, &store);
    let coordinator = Coordinator {
        store: &store,
        adapters: &adapters,
    };
    coordinator.prepare(request(), &|_, _, _| {}).unwrap();
    coordinator.abort().unwrap();
    for id in [A, B, C] {
        assert_eq!(machine(&world, id), Machine::Running);
    }
    assert!(!store.exists());
    assert_eq!(
        world.lock().unwrap().count("restore"),
        0,
        "no restore: nothing was released"
    );
    coordinator.abort().unwrap();
}

#[test]
fn release_requires_a_completed_save_and_then_updates_the_journal() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::new(directory.path());
    let world = two_macs_and_a_linux();
    let adapters = Fake::all(&world, &store);
    let coordinator = Coordinator {
        store: &store,
        adapters: &adapters,
    };
    assert!(coordinator
        .release_for_update()
        .unwrap_err()
        .contains("not prepared"));
    coordinator.prepare(request(), &|_, _, _| {}).unwrap();
    coordinator.release_for_update().unwrap();
    assert_eq!(
        store.load().unwrap().unwrap().phase,
        TransactionPhase::Released
    );
    assert_eq!(machine(&world, A), Machine::Held);
    assert_eq!(machine(&world, C), Machine::Detached);
    // Releasing again is idempotent and never re-releases.
    coordinator.release_for_update().unwrap();
    assert_eq!(world.lock().unwrap().count("release"), 3);
}

#[test]
fn the_next_owner_restores_exactly_the_saved_set_without_a_cold_boot() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::new(directory.path());
    let world = two_macs_and_a_linux();
    {
        let adapters = Fake::all(&world, &store);
        let coordinator = Coordinator {
            store: &store,
            adapters: &adapters,
        };
        coordinator.prepare(request(), &|_, _, _| {}).unwrap();
        coordinator.release_for_update().unwrap();
    }
    world.lock().unwrap().crash();
    let adapters = Fake::all(&world, &store);
    let coordinator = Coordinator {
        store: &store,
        adapters: &adapters,
    };
    let recovery = coordinator.recover().unwrap().unwrap();
    assert!(
        recovery.settled() && recovery.lost.is_empty(),
        "{recovery:?}"
    );
    assert_eq!(recovery.resumed.len(), 3);
    for id in [A, B, C] {
        assert!(executing(&world, id), "{id}");
    }
    assert_eq!(
        machine(&world, "stopped"),
        Machine::Gone,
        "stopped computers stay stopped"
    );
    assert!(!store.exists());
    assert_eq!(world.lock().unwrap().count("restore"), 3);
    assert!(coordinator.recover().unwrap().is_none());
    assert!(!store.saves_root().join("tx-1").exists());
}

#[test]
fn a_restore_refused_before_it_runs_stays_journaled_and_is_retried() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::new(directory.path());
    let world = two_macs_and_a_linux();
    {
        let adapters = Fake::all(&world, &store);
        let coordinator = Coordinator {
            store: &store,
            adapters: &adapters,
        };
        coordinator.prepare(request(), &|_, _, _| {}).unwrap();
        coordinator.release_for_update().unwrap();
    }
    world.lock().unwrap().crash();
    world.lock().unwrap().fail_restore.insert(B.into());
    let adapters = Fake::all(&world, &store);
    let coordinator = Coordinator {
        store: &store,
        adapters: &adapters,
    };
    let recovery = coordinator.recover().unwrap().unwrap();
    assert_eq!(recovery.failed.len(), 1);
    assert_eq!(recovery.failed[0].0.id, B);
    assert!(executing(&world, A) && executing(&world, C));
    assert_eq!(machine(&world, B), Machine::Gone, "never cold booted");
    let journal = store.load().unwrap().unwrap();
    assert_eq!(journal.phase, TransactionPhase::Failed);
    assert!(journal
        .computers
        .iter()
        .find(|c| c.id == B)
        .unwrap()
        .capture
        .is_some());
    // Resumed computers are not restored again; only the unresolved one is retried.
    world.lock().unwrap().fail_restore.clear();
    let retry = coordinator.recover().unwrap().unwrap();
    assert!(retry.settled());
    assert!(executing(&world, B));
    assert_eq!(world.lock().unwrap().count("restore:upd-a"), 1);
    assert_eq!(world.lock().unwrap().count("restore:upd-b"), 2);
    assert!(!store.exists());
}

#[test]
fn a_failure_after_the_restore_was_armed_is_never_replayed() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::new(directory.path());
    let world = world(&[(A, ComputerKind::Macos, Machine::Running)]);
    {
        let adapters = Fake::all(&world, &store);
        let coordinator = Coordinator {
            store: &store,
            adapters: &adapters,
        };
        coordinator.prepare(request(), &|_, _, _| {}).unwrap();
        coordinator.release_for_update().unwrap();
    }
    world.lock().unwrap().crash();
    world.lock().unwrap().fail_after_arm.insert(A.into());
    let adapters = Fake::all(&world, &store);
    let coordinator = Coordinator {
        store: &store,
        adapters: &adapters,
    };
    let recovery = coordinator.recover().unwrap().unwrap();
    assert_eq!(recovery.failed.len(), 1);
    assert!(recovery.failed[0].1.contains("uncertain"));
    let entry = store.load().unwrap().unwrap().computers.remove(0);
    assert_eq!(
        (entry.phase, entry.retryable),
        (ComputerPhase::Failed, false)
    );
    // The guest is running: the next owner finds it and settles the entry without a restore.
    let again = coordinator.recover().unwrap().unwrap();
    assert!(again.settled());
    assert_eq!(world.lock().unwrap().count("restore"), 1);
    // If it had died instead, the capture must not be replayed over its advanced disk.
    assert!(!store.exists());
}

#[test]
fn an_armed_restore_whose_owner_died_is_reported_lost_not_replayed() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::new(directory.path());
    let world = world(&[(A, ComputerKind::Macos, Machine::Running)]);
    {
        let adapters = Fake::all(&world, &store);
        let coordinator = Coordinator {
            store: &store,
            adapters: &adapters,
        };
        coordinator.prepare(request(), &|_, _, _| {}).unwrap();
        coordinator.release_for_update().unwrap();
    }
    world.lock().unwrap().crash();
    world.lock().unwrap().fail_after_arm.insert(A.into());
    {
        let adapters = Fake::all(&world, &store);
        let coordinator = Coordinator {
            store: &store,
            adapters: &adapters,
        };
        coordinator.recover().unwrap().unwrap();
    }
    // The owner dies with its restored machine.
    world.lock().unwrap().crash();
    let adapters = Fake::all(&world, &store);
    let coordinator = Coordinator {
        store: &store,
        adapters: &adapters,
    };
    let recovery = coordinator.recover().unwrap().unwrap();
    assert_eq!(recovery.lost.len(), 1);
    assert!(
        recovery.lost[0].1.contains("disk was kept"),
        "{:?}",
        recovery.lost
    );
    assert_eq!(
        world.lock().unwrap().count("restore"),
        1,
        "the capture was not replayed"
    );
    assert!(!store.exists());
}

fn silence_panics<T>(work: impl FnOnce() -> T) -> T {
    static HOOK: Mutex<()> = Mutex::new(());
    let _guard = HOOK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let result = work();
    std::panic::set_hook(previous);
    result
}

/// Crashes the process (a panic, so no rollback code runs) at the `crash_at`-th journal
/// write event, counting both write boundaries. Returns whether a crash happened.
fn run_scenario_until_crash(store_directory: &Path, world: &Shared, crash_at: usize) -> bool {
    let counter = Arc::new(Mutex::new(0usize));
    let store = Store::new(store_directory).with_fault({
        let counter = counter.clone();
        move |_fault: Fault| {
            let mut seen = counter.lock().unwrap();
            *seen += 1;
            if *seen == crash_at {
                panic!("simulated crash");
            }
            Ok(())
        }
    });
    let adapters = Fake::all(world, &store);
    let coordinator = Coordinator {
        store: &store,
        adapters: &adapters,
    };
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        coordinator.prepare(request(), &|_, _, _| {}).unwrap();
        coordinator.release_for_update().unwrap();
    }));
    outcome.is_err()
}

#[test]
fn a_crash_at_any_journal_write_never_loses_or_replays_a_saved_computer() {
    // Count the fault points of an uninterrupted run.
    let total = {
        let directory = tempfile::tempdir().unwrap();
        let counter = Arc::new(Mutex::new(0usize));
        let store = Store::new(directory.path()).with_fault({
            let counter = counter.clone();
            move |_| {
                *counter.lock().unwrap() += 1;
                Ok(())
            }
        });
        let world = two_macs_and_a_linux();
        let adapters = Fake::all(&world, &store);
        let coordinator = Coordinator {
            store: &store,
            adapters: &adapters,
        };
        coordinator.prepare(request(), &|_, _, _| {}).unwrap();
        coordinator.release_for_update().unwrap();
        let total = *counter.lock().unwrap();
        total
    };
    assert!(total > 12, "{total}");
    silence_panics(|| {
        for crash_at in 1..=total {
            let directory = tempfile::tempdir().unwrap();
            let world = two_macs_and_a_linux();
            assert!(run_scenario_until_crash(directory.path(), &world, crash_at));
            world.lock().unwrap().crash();
            let store = Store::new(directory.path());
            let adapters = Fake::all(&world, &store);
            let coordinator = Coordinator {
                store: &store,
                adapters: &adapters,
            };
            match coordinator.recover().unwrap() {
                // The crash came before the journal's first write: nothing was paused.
                None => {
                    for id in [A, B, C] {
                        assert_eq!(
                            machine(&world, id),
                            if id == C {
                                Machine::Running
                            } else {
                                Machine::Gone
                            },
                            "crash {crash_at}"
                        );
                    }
                    continue;
                }
                Some(recovery) => {
                    assert!(recovery.settled(), "crash {crash_at}: {recovery:?}");
                    assert_eq!(
                        recovery.resumed.len() + recovery.lost.len(),
                        3,
                        "crash {crash_at}: every computer is resumed or reported"
                    );
                }
            }
            let state = world.lock().unwrap();
            for id in [A, B, C] {
                let (_, machine) = &state.machines[id];
                assert!(
                    matches!(
                        machine,
                        Machine::Running | Machine::Executing(_) | Machine::Gone
                    ),
                    "crash {crash_at}: {id} is {machine:?}"
                );
                assert!(
                    state.count(&format!("restore:{id}")) <= 1,
                    "crash {crash_at}: {id}"
                );
            }
            // A computer that was saved and journaled comes back with its memory.
            if let Some(saved) = state.calls.iter().find(|call| call.starts_with("restore:")) {
                assert!(state.machines[&saved["restore:".len()..]].1 != Machine::Gone);
            }
            drop(state);
            assert!(!store.exists(), "crash {crash_at}: the journal is resolved");
        }
    });
}

#[test]
fn a_crash_during_recovery_is_settled_by_the_next_owner_without_replay() {
    // Crash the recovering owner at each of its journal writes.
    let prepare_released = |directory: &Path| {
        let store = Store::new(directory);
        let world = two_macs_and_a_linux();
        let adapters = Fake::all(&world, &store);
        let coordinator = Coordinator {
            store: &store,
            adapters: &adapters,
        };
        coordinator.prepare(request(), &|_, _, _| {}).unwrap();
        coordinator.release_for_update().unwrap();
        world.lock().unwrap().crash();
        world
    };
    let total = {
        let directory = tempfile::tempdir().unwrap();
        let world = prepare_released(directory.path());
        let counter = Arc::new(Mutex::new(0usize));
        let store = Store::new(directory.path()).with_fault({
            let counter = counter.clone();
            move |_| {
                *counter.lock().unwrap() += 1;
                Ok(())
            }
        });
        let adapters = Fake::all(&world, &store);
        Coordinator {
            store: &store,
            adapters: &adapters,
        }
        .recover()
        .unwrap();
        let total = *counter.lock().unwrap();
        total
    };
    silence_panics(|| {
        for crash_at in 1..=total {
            let directory = tempfile::tempdir().unwrap();
            let world = prepare_released(directory.path());
            let counter = Arc::new(Mutex::new(0usize));
            let store = Store::new(directory.path()).with_fault({
                let counter = counter.clone();
                move |_| {
                    let mut seen = counter.lock().unwrap();
                    *seen += 1;
                    if *seen == crash_at {
                        panic!("simulated crash");
                    }
                    Ok(())
                }
            });
            let adapters = Fake::all(&world, &store);
            let first = catch_unwind(AssertUnwindSafe(|| {
                Coordinator {
                    store: &store,
                    adapters: &adapters,
                }
                .recover()
            }));
            assert!(first.is_err(), "crash {crash_at}");
            // The recovering process dies with every macOS machine it restored.
            world.lock().unwrap().crash();
            let store = Store::new(directory.path());
            let adapters = Fake::all(&world, &store);
            let coordinator = Coordinator {
                store: &store,
                adapters: &adapters,
            };
            let recovery = coordinator.recover().unwrap().unwrap();
            assert!(recovery.settled(), "crash {crash_at}: {recovery:?}");
            let state = world.lock().unwrap();
            for id in [A, B] {
                // The capture was restored at most once: a dead armed restore is reported,
                // not replayed over a disk the guest may have advanced.
                let restores = state.count(&format!("restore:{id}"));
                assert!(
                    restores <= 2,
                    "crash {crash_at}: {id} restored {restores} times"
                );
                if restores == 2 {
                    assert!(
                        matches!(state.machines[id].1, Machine::Executing(_)),
                        "crash {crash_at}"
                    );
                }
            }
            drop(state);
            assert!(!store.exists());
        }
    });
}

// MARK: Lifecycle

struct TestContext {
    paths: crate::context::EnginePaths,
    identity: crate::context::EngineIdentity,
    runtime_state: Arc<crate::context::EngineRuntimeState>,
}

impl TestContext {
    fn new(home: &Path) -> Self {
        Self::versioned(home, "1.0.0")
    }

    fn versioned(home: &Path, version: &str) -> Self {
        Self {
            paths: crate::context::EnginePaths {
                home: home.to_path_buf(),
                app_data: home.join("data"),
                app_config: home.join("config"),
                ..Default::default()
            },
            identity: crate::context::EngineIdentity {
                channel: crate::channel::Channel::Development,
                version: version.into(),
                package_kind: crate::context::PackageKind::MacApp,
            },
            runtime_state: Arc::new(crate::context::EngineRuntimeState::default()),
        }
    }
}

impl Drop for TestContext {
    fn drop(&mut self) {
        // Tests may end with a journal in place; the process-wide gate must not keep refusing
        // their computers.
        super::reservation::clear(&Store::new(super::engine_directory(self)));
    }
}

impl crate::context::EngineContext for TestContext {
    fn paths(&self) -> &crate::context::EnginePaths {
        &self.paths
    }

    fn identity(&self) -> &crate::context::EngineIdentity {
        &self.identity
    }

    fn runtime_state(&self) -> Arc<crate::context::EngineRuntimeState> {
        self.runtime_state.clone()
    }

    fn publish(&self, _: crate::context::Event, _: crate::context::Audience) {}

    fn main_thread(&self) -> Option<&dyn crate::context::MainThread> {
        None
    }

    fn process(&self) -> &dyn crate::context::ProcessControl {
        self
    }
}

impl crate::context::ProcessControl for TestContext {
    fn exit(&self, _: i32) {}

    fn restart(&self) -> ! {
        panic!("tests never restart")
    }
}

struct Guards(Arc<Mutex<Vec<String>>>);

impl HeldGuards for Guards {
    fn relabel(&self, label: &str) {
        self.0.lock().unwrap().push(label.to_owned());
    }
}

impl Drop for Guards {
    fn drop(&mut self) {
        self.0.lock().unwrap().push("guards released".into());
    }
}

struct FakePlatform {
    world: Shared,
    store_directory: std::path::PathBuf,
    events: Arc<Mutex<Vec<String>>>,
    /// When set, `stop_computers` blocks until the sender is dropped or used.
    block_stop: Arc<Mutex<Option<std::sync::mpsc::Receiver<()>>>>,
}

fn fake_platform(
    world: &Shared,
    store_directory: std::path::PathBuf,
    events: &Arc<Mutex<Vec<String>>>,
) -> FakePlatform {
    FakePlatform {
        world: world.clone(),
        store_directory,
        events: events.clone(),
        block_stop: Arc::default(),
    }
}

impl Platform for FakePlatform {
    fn adapters(&self) -> Vec<Box<dyn SaveAdapter>> {
        Fake::all(&self.world, &Store::new(&self.store_directory))
    }

    fn acquire_guards(&self, job: Option<&JobId>) -> Result<Box<dyn HeldGuards>, String> {
        self.events
            .lock()
            .unwrap()
            .push(format!("guards acquired for {:?}", job.map(JobId::as_str)));
        Ok(Box::new(Guards(self.events.clone())))
    }

    fn stop_computers(&self, deadline: Option<Instant>) -> Result<(), String> {
        let blocked = self.block_stop.lock().unwrap().take();
        if let Some(blocked) = blocked {
            let _ = blocked.recv();
        }
        let mut world = self.world.lock().unwrap();
        world
            .calls
            .push(format!("stop-computers deadline={}", deadline.is_some()));
        if let Some(why) = world.stop_failure.clone() {
            return Err(why);
        }
        for (_, machine) in world.machines.values_mut() {
            *machine = Machine::Gone;
        }
        Ok(())
    }

    fn flush_settings(&self) -> Result<(), String> {
        self.events.lock().unwrap().push("flush settings".into());
        Ok(())
    }

    fn close_connections(&self) {
        self.events.lock().unwrap().push("close connections".into());
    }

    fn reopen_admission(&self) {
        self.events.lock().unwrap().push("reopen admission".into());
    }
}

struct Lifecycle {
    _directory: tempfile::TempDir,
    context: TestContext,
    state: Arc<UpdateState>,
    world: Shared,
    events: Arc<Mutex<Vec<String>>>,
    block_stop: Arc<Mutex<Option<std::sync::mpsc::Receiver<()>>>>,
}

fn lifecycle(world: Shared) -> Lifecycle {
    let directory = tempfile::tempdir().unwrap();
    let context = TestContext::new(directory.path());
    let state = crate::context::EngineContext::runtime_state(&context)
        .update()
        .clone();
    let events = Arc::new(Mutex::new(Vec::new()));
    let platform = fake_platform(&world, super::engine_directory(&context), &events);
    let block_stop = platform.block_stop.clone();
    state.set_platform(Arc::new(platform));
    Lifecycle {
        _directory: directory,
        context,
        state,
        world,
        events,
        block_stop,
    }
}

fn prepare(lifecycle: &Lifecycle, job: &str) {
    let job = JobId::new(job).unwrap();
    assert_eq!(lifecycle.state.begin_prepare().unwrap(), PrepareStart::New);
    lifecycle.state.prepare_job(job.clone());
    let hold = Hold::start(
        lifecycle.state.platform().unwrap(),
        super::store(&lifecycle.context),
        Some(job.clone()),
    )
    .unwrap();
    hold.prepare(request()).unwrap();
    lifecycle.state.prepared(job, hold);
}

fn shutdown(reason: ShutdownReason, stop: bool) -> ShutdownRequest {
    ShutdownRequest {
        stop_computers: stop,
        reason,
        deadline: None,
    }
}

#[test]
fn preparing_twice_attaches_to_the_same_job_and_projects_engine_state() {
    let lifecycle = lifecycle(two_macs_and_a_linux());
    assert!(!lifecycle.state.engine_state().update_pending);
    prepare(&lifecycle, "job:7");
    let state = lifecycle.state.engine_state();
    assert!(state.update_pending && !state.shutting_down);
    assert_eq!(
        lifecycle.state.begin_prepare().unwrap(),
        PrepareStart::Attach(JobId::new("job:7").unwrap())
    );
    assert!(lifecycle.events.lock().unwrap()[0].contains("job:7"));
    lifecycle.state.abort(&lifecycle.context).unwrap();
    assert!(!lifecycle.state.engine_state().update_pending);
    assert!(lifecycle
        .events
        .lock()
        .unwrap()
        .contains(&"guards released".to_owned()));
    for id in [A, B, C] {
        assert_eq!(machine(&lifecycle.world, id), Machine::Running);
    }
    // Abort with nothing prepared succeeds.
    lifecycle.state.abort(&lifecycle.context).unwrap();
}

#[test]
fn a_shutdown_for_an_update_requires_a_completed_save_and_releases_it() {
    let lifecycle = lifecycle(two_macs_and_a_linux());
    let update = shutdown(ShutdownReason::Update, false);
    assert!(matches!(
        lifecycle.state.begin_shutdown(update.clone()),
        Err(LifecycleError::Refused(message)) if message.contains("not prepared")
    ));
    assert!(matches!(
        lifecycle
            .state
            .begin_shutdown(shutdown(ShutdownReason::Update, true)),
        Err(LifecycleError::Refused(_))
    ));
    prepare(&lifecycle, "job:1");
    // Other shutdowns wait for the update to be aborted or released.
    assert!(matches!(
        lifecycle
            .state
            .begin_shutdown(shutdown(ShutdownReason::User, true)),
        Err(LifecycleError::Busy(_))
    ));
    assert_eq!(
        lifecycle.state.begin_shutdown(update.clone()),
        Ok(ShutdownStart::New)
    );
    lifecycle.state.shutdown_job(JobId::new("job:2").unwrap());
    assert_eq!(
        lifecycle.state.begin_shutdown(update),
        Ok(ShutdownStart::Attach(JobId::new("job:2").unwrap()))
    );
    assert!(matches!(
        lifecycle
            .state
            .begin_shutdown(shutdown(ShutdownReason::User, false)),
        Err(LifecycleError::Busy(_))
    ));
    let state = lifecycle.state.engine_state();
    assert!(
        state.shutting_down && state.phase == ShutdownPhase::Requested && state.generation == 1
    );
    lifecycle.state.run_shutdown().unwrap();
    assert_eq!(
        lifecycle.state.engine_state().phase,
        ShutdownPhase::Flushing
    );
    // No guest shutdown: the platform was never asked to stop computers.
    assert_eq!(lifecycle.world.lock().unwrap().count("stop-computers"), 0);
    assert_eq!(machine(&lifecycle.world, A), Machine::Held);
    assert_eq!(machine(&lifecycle.world, C), Machine::Detached);
    let events = lifecycle.events.lock().unwrap().clone();
    assert!(events.contains(&"flush settings".to_owned()));
    assert!(events.contains(&"close connections".to_owned()));
}

#[test]
fn a_user_shutdown_stops_computers_and_a_failure_reopens_service() {
    let lifecycle = lifecycle(two_macs_and_a_linux());
    let stop = shutdown(ShutdownReason::User, true);
    assert_eq!(
        lifecycle.state.begin_shutdown(stop.clone()),
        Ok(ShutdownStart::New)
    );
    lifecycle.state.shutdown_job(JobId::new("job:3").unwrap());
    lifecycle.world.lock().unwrap().stop_failure =
        Some("Some local computers could not stop".into());
    let error = lifecycle.state.run_shutdown().unwrap_err();
    assert!(error.contains("could not stop"));
    lifecycle.state.reopen_after_failed_shutdown().unwrap();
    let state = lifecycle.state.engine_state();
    assert!(!state.shutting_down && state.phase == ShutdownPhase::Idle && state.generation == 1);
    assert!(lifecycle
        .events
        .lock()
        .unwrap()
        .contains(&"reopen admission".to_owned()));
    // A retry succeeds and counts as a new generation.
    lifecycle.world.lock().unwrap().stop_failure = None;
    assert_eq!(lifecycle.state.begin_shutdown(stop), Ok(ShutdownStart::New));
    lifecycle.state.run_shutdown().unwrap();
    assert_eq!(lifecycle.state.engine_state().generation, 2);
    assert_eq!(
        lifecycle
            .world
            .lock()
            .unwrap()
            .count("stop-computers deadline=false"),
        2
    );
}

#[test]
fn a_shutdown_that_would_destroy_live_macos_machines_is_refused() {
    let lifecycle = lifecycle(two_macs_and_a_linux());
    assert!(matches!(
        lifecycle.state.begin_shutdown(shutdown(ShutdownReason::User, false)),
        Err(LifecycleError::Busy(message)) if message.contains("macOS computer is still running")
    ));
    // Linux runtimes stay detached, so a world with only Linux computers may exit.
    let linux_only = self::lifecycle(world(&[(C, ComputerKind::Linux, Machine::Running)]));
    assert_eq!(
        linux_only
            .state
            .begin_shutdown(shutdown(ShutdownReason::User, false)),
        Ok(ShutdownStart::New)
    );
    linux_only.state.run_shutdown().unwrap();
    assert_eq!(machine(&linux_only.world, C), Machine::Running);
}

#[test]
fn a_session_end_never_cancels_and_outranks_a_running_shutdown() {
    let lifecycle = lifecycle(two_macs_and_a_linux());
    lifecycle.world.lock().unwrap().stop_failure = Some("stop failed".into());
    let end = ShutdownRequest {
        stop_computers: true,
        reason: ShutdownReason::SessionEnd,
        deadline: Some(std::time::Duration::from_secs(5)),
    };
    assert_eq!(lifecycle.state.begin_shutdown(end), Ok(ShutdownStart::New));
    assert!(lifecycle.state.engine_state().session_ending);
    lifecycle.state.run_shutdown().unwrap();
    assert_eq!(
        lifecycle
            .world
            .lock()
            .unwrap()
            .count("stop-computers deadline=true"),
        1
    );

    let lifecycle = self::lifecycle(two_macs_and_a_linux());
    let user = shutdown(ShutdownReason::User, true);
    lifecycle.state.begin_shutdown(user).unwrap();
    lifecycle.state.shutdown_job(JobId::new("job:4").unwrap());
    let attached = lifecycle.state.begin_shutdown(ShutdownRequest {
        stop_computers: true,
        reason: ShutdownReason::SessionEnd,
        deadline: None,
    });
    assert_eq!(
        attached,
        Ok(ShutdownStart::Attach(JobId::new("job:4").unwrap()))
    );
    lifecycle.world.lock().unwrap().stop_failure = Some("stop failed".into());
    lifecycle.state.run_shutdown().unwrap();

    // While an update is prepared, termination preserves the saved set.
    let lifecycle = self::lifecycle(two_macs_and_a_linux());
    prepare(&lifecycle, "job:5");
    lifecycle
        .state
        .begin_shutdown(ShutdownRequest {
            stop_computers: true,
            reason: ShutdownReason::SessionEnd,
            deadline: None,
        })
        .unwrap();
    lifecycle.state.run_shutdown().unwrap();
    assert_eq!(lifecycle.world.lock().unwrap().count("stop-computers"), 0);
    assert_eq!(lifecycle.world.lock().unwrap().count("release"), 3);
}

#[test]
fn abort_after_the_engine_restarted_resumes_the_journaled_set_in_a_temporary_hold() {
    let world = two_macs_and_a_linux();
    let first = lifecycle(world.clone());
    prepare(&first, "job:1");
    first
        .state
        .begin_shutdown(shutdown(ShutdownReason::Update, false))
        .unwrap();
    first.state.run_shutdown().unwrap();
    world.lock().unwrap().crash();
    // A new engine instance over the same directory.
    let directory = first._directory;
    let context = TestContext::new(directory.path());
    let state = crate::context::EngineContext::runtime_state(&context)
        .update()
        .clone();
    let events = Arc::new(Mutex::new(Vec::new()));
    state.set_platform(Arc::new(fake_platform(
        &world,
        super::engine_directory(&context),
        &events,
    )));
    // Recovery marks the update pending until it settles, then resumes the set.
    let recovery = state.recover(&context).unwrap().unwrap();
    assert!(recovery.settled());
    assert!(!state.update_pending());
    for id in [A, B, C] {
        assert!(executing(&world, id));
    }
    assert!(state.recover(&context).unwrap().is_none());
    assert!(events
        .lock()
        .unwrap()
        .contains(&"guards released".to_owned()));
}

#[test]
fn a_failed_recovery_keeps_the_update_pending_and_blocks_a_new_preparation() {
    let world = two_macs_and_a_linux();
    let first = lifecycle(world.clone());
    prepare(&first, "job:1");
    first
        .state
        .begin_shutdown(shutdown(ShutdownReason::Update, false))
        .unwrap();
    first.state.run_shutdown().unwrap();
    world.lock().unwrap().crash();
    world.lock().unwrap().fail_restore.insert(A.into());
    let directory = first._directory;
    let context = TestContext::new(directory.path());
    let state = crate::context::EngineContext::runtime_state(&context)
        .update()
        .clone();
    state.set_platform(Arc::new(fake_platform(
        &world,
        super::engine_directory(&context),
        &Arc::new(Mutex::new(Vec::new())),
    )));
    let recovery = state.recover(&context).unwrap().unwrap();
    assert!(!recovery.settled());
    assert!(state.update_pending());
    assert!(state.recovery_error().unwrap().contains("computer-upd-a"));
    assert!(matches!(
        state.begin_prepare(),
        Err(LifecycleError::Refused(_))
    ));
    // Abort retries the unresolved computer and clears the pending state.
    world.lock().unwrap().fail_restore.clear();
    state.abort(&context).unwrap();
    assert!(!state.update_pending());
    assert!(state.recovery_error().is_none());
    assert!(executing(&world, A));
    assert_eq!(state.begin_prepare().unwrap(), PrepareStart::New);
}

// MARK: Review fixes: abort, rollback and recovery boundaries

/// Runs `scenario` once to count the journal write boundaries, then once per boundary with a
/// crash there (a panic, so no cleanup code runs), and recovers in a new owner each time.
/// Nothing may ever be replayed from a capture over a disk its guest advanced.
fn crash_at_every_boundary(
    new_world: fn() -> Shared,
    setup: fn(&Store, &Shared),
    scenario: fn(&Coordinator<'_>),
) {
    let total = {
        let directory = tempfile::tempdir().unwrap();
        let world = new_world();
        setup(&Store::new(directory.path()), &world);
        let counter = Arc::new(Mutex::new(0usize));
        let store = Store::new(directory.path()).with_fault({
            let counter = counter.clone();
            move |_| {
                *counter.lock().unwrap() += 1;
                Ok(())
            }
        });
        let adapters = Fake::all(&world, &store);
        scenario(&Coordinator {
            store: &store,
            adapters: &adapters,
        });
        let total = *counter.lock().unwrap();
        total
    };
    assert!(total >= 4, "{total}");
    silence_panics(|| {
        for crash_at in 1..=total {
            let directory = tempfile::tempdir().unwrap();
            let world = new_world();
            setup(&Store::new(directory.path()), &world);
            let counter = Arc::new(Mutex::new(0usize));
            let store = Store::new(directory.path()).with_fault({
                let counter = counter.clone();
                move |_| {
                    let mut seen = counter.lock().unwrap();
                    *seen += 1;
                    if *seen == crash_at {
                        panic!("simulated crash");
                    }
                    Ok(())
                }
            });
            let adapters = Fake::all(&world, &store);
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                scenario(&Coordinator {
                    store: &store,
                    adapters: &adapters,
                })
            }));
            assert!(outcome.is_err(), "crash {crash_at}");
            world.lock().unwrap().crash();
            let store = Store::new(directory.path());
            let adapters = Fake::all(&world, &store);
            let coordinator = Coordinator {
                store: &store,
                adapters: &adapters,
            };
            if let Some(recovery) = coordinator.recover().unwrap() {
                assert!(recovery.settled(), "crash {crash_at}: {recovery:?}");
            }
            let state = world.lock().unwrap();
            assert!(
                state.replayed.is_empty(),
                "crash {crash_at}: a capture was replayed over the advanced disk of {:?}",
                state.replayed
            );
            for id in [A, B, C] {
                assert!(
                    matches!(
                        state.machines[id].1,
                        Machine::Running | Machine::Executing(_) | Machine::Gone
                    ),
                    "crash {crash_at}: {id} is {:?}",
                    state.machines[id].1
                );
                assert!(
                    state.count(&format!("restore:{id}")) <= 1,
                    "crash {crash_at}"
                );
            }
            drop(state);
            assert!(!store.exists(), "crash {crash_at}: the journal is resolved");
        }
    });
}

fn prepared_under(store: &Store, world: &Shared) {
    let adapters = Fake::all(world, store);
    Coordinator {
        store,
        adapters: &adapters,
    }
    .prepare(request(), &|_, _, _| {})
    .unwrap();
}

#[test]
fn a_crash_at_any_boundary_of_an_abort_never_replays_a_resumed_guest() {
    crash_at_every_boundary(two_macs_and_a_linux, prepared_under, |coordinator| {
        coordinator.abort().unwrap();
    });
}

#[test]
fn a_crash_at_any_boundary_of_a_rollback_never_replays_a_resumed_guest() {
    fn failing_third_save() -> Shared {
        let world = two_macs_and_a_linux();
        world.lock().unwrap().fail_save.insert(C.into());
        world
    }
    crash_at_every_boundary(
        failing_third_save,
        |_, _| {},
        |coordinator| {
            coordinator.prepare(request(), &|_, _, _| {}).unwrap_err();
        },
    );
}

#[test]
fn a_crash_at_any_boundary_of_an_abort_after_release_never_replays_a_resumed_guest() {
    crash_at_every_boundary(
        two_macs_and_a_linux,
        |store, world| {
            prepared_under(store, world);
            let adapters = Fake::all(world, store);
            Coordinator {
                store,
                adapters: &adapters,
            }
            .release_for_update()
            .unwrap();
        },
        |coordinator| coordinator.abort().unwrap(),
    );
}

#[test]
fn a_resume_whose_journal_commit_never_happened_is_probed_and_never_restored() {
    let directory = tempfile::tempdir().unwrap();
    let world = two_macs_and_a_linux();
    prepared_under(&Store::new(directory.path()), &world);
    // Abort resumes every computer, then the process dies before any `resumed` commit: the
    // armed transition was the last thing on disk.
    let writes = Arc::new(Mutex::new(0usize));
    let store = Store::new(directory.path()).with_fault({
        let (writes, world) = (writes.clone(), world.clone());
        move |fault| {
            if fault == Fault::BeforeWrite {
                *writes.lock().unwrap() += 1;
                // Writes: Restoring phase, then (armed, resumed) per computer. Die at the first
                // `resumed` commit, after the guest of A executed.
                if *writes.lock().unwrap() == 3 {
                    assert!(world.lock().unwrap().advanced.contains(A));
                    panic!("simulated crash");
                }
            }
            Ok(())
        }
    });
    let adapters = Fake::all(&world, &store);
    silence_panics(|| {
        assert!(catch_unwind(AssertUnwindSafe(|| {
            let _ = Coordinator {
                store: &store,
                adapters: &adapters,
            }
            .abort();
        }))
        .is_err());
    });
    let on_disk = Store::new(directory.path()).load().unwrap().unwrap();
    assert_eq!(on_disk.computers[0].phase, ComputerPhase::ResumingOriginal);
    world.lock().unwrap().crash();
    let store = Store::new(directory.path());
    let adapters = Fake::all(&world, &store);
    let recovery = Coordinator {
        store: &store,
        adapters: &adapters,
    }
    .recover()
    .unwrap()
    .unwrap();
    // The macOS machine died with its engine after it executed: reported, never replayed.
    assert!(recovery
        .lost
        .iter()
        .any(|(computer, why)| computer.id == A && why.contains("disk was kept")));
    assert_eq!(world.lock().unwrap().count("restore:upd-a"), 0);
    // The computers the abort never reached were not resumed, so their captures are the way
    // back for them (the detached runtime is simply resumed from its pause).
    assert!(executing(&world, B) && executing(&world, C));
    assert!(world.lock().unwrap().replayed.is_empty());
}

#[test]
fn abort_after_a_release_resumes_the_machines_this_process_still_owns() {
    let directory = tempfile::tempdir().unwrap();
    let world = two_macs_and_a_linux();
    let adapters;
    // The settings flush or a later journal write failed after the release.
    let writes = Arc::new(Mutex::new(0usize));
    let store = Store::new(directory.path()).with_fault({
        let writes = writes.clone();
        move |fault| {
            if fault == Fault::BeforeWrite {
                let mut seen = writes.lock().unwrap();
                *seen += 1;
                // prepare: 1 plan + 3 capturing + 3 saved + 1 prepared = 8; release: 1 phase,
                // 3 entries, then the final `released` phase is the 13th write.
                if *seen == 13 {
                    return Err("journal write failed".into());
                }
            }
            Ok(())
        }
    });
    adapters = Fake::all(&world, &store);
    let coordinator = Coordinator {
        store: &store,
        adapters: &adapters,
    };
    coordinator.prepare(request(), &|_, _, _| {}).unwrap();
    assert!(coordinator.release_for_update().is_err());
    let journal = store.load().unwrap().unwrap();
    assert_eq!(journal.phase, TransactionPhase::Releasing);
    assert!(journal
        .computers
        .iter()
        .all(|entry| entry.phase == ComputerPhase::Released));
    assert_eq!(machine(&world, A), Machine::Held);
    // The original, still-owned generations resume in place: no restore replaces them.
    coordinator.abort().unwrap();
    for id in [A, B, C] {
        assert_eq!(machine(&world, id), Machine::Running, "{id}");
    }
    assert_eq!(world.lock().unwrap().count("restore"), 0);
    assert!(!store.exists());
}

#[test]
fn abort_after_a_release_in_a_new_process_restores_the_gone_machines() {
    let directory = tempfile::tempdir().unwrap();
    let world = two_macs_and_a_linux();
    let store = Store::new(directory.path());
    prepared_under(&store, &world);
    let adapters = Fake::all(&world, &store);
    let coordinator = Coordinator {
        store: &store,
        adapters: &adapters,
    };
    coordinator.release_for_update().unwrap();
    world.lock().unwrap().crash();
    coordinator.abort().unwrap();
    assert!(executing(&world, A) && executing(&world, B));
    // The detached Linux runtime is resumed in place, not restored.
    assert_eq!(machine(&world, C), Machine::Running);
    assert_eq!(world.lock().unwrap().count("restore:upd-c"), 0);
}

#[test]
fn a_replacement_run_is_never_settled_by_a_capture_or_discarded_on_its_behalf() {
    let directory = tempfile::tempdir().unwrap();
    let world = world(&[(A, ComputerKind::Macos, Machine::Running)]);
    let store = Store::new(directory.path());
    prepared_under(&store, &world);
    let adapters = Fake::all(&world, &store);
    let coordinator = Coordinator {
        store: &store,
        adapters: &adapters,
    };
    coordinator.release_for_update().unwrap();
    world.lock().unwrap().crash();
    // Something else started the computer: a different run of the same computer.
    {
        let mut state = world.lock().unwrap();
        *state.runs.get_mut(A).unwrap() += 1;
        state.machines.get_mut(A).unwrap().1 = Machine::Executing("someone-else".into());
    }
    let recovery = coordinator.recover().unwrap().unwrap();
    assert_eq!(recovery.lost.len(), 1, "{recovery:?}");
    assert!(recovery.resumed.is_empty());
    assert_eq!(
        machine(&world, A),
        Machine::Executing("someone-else".into()),
        "the replacement was left alone"
    );
    assert_eq!(world.lock().unwrap().count("discard"), 0);
    assert!(
        store.saves_root().join("tx-1").exists() || world.lock().unwrap().count("discard") == 0,
        "the capture is kept"
    );
}

#[test]
fn an_armed_restore_is_proved_only_by_the_run_it_named() {
    let directory = tempfile::tempdir().unwrap();
    let world = world(&[(A, ComputerKind::Macos, Machine::Running)]);
    let store = Store::new(directory.path());
    prepared_under(&store, &world);
    let adapters = Fake::all(&world, &store);
    let coordinator = Coordinator {
        store: &store,
        adapters: &adapters,
    };
    coordinator.release_for_update().unwrap();
    world.lock().unwrap().crash();
    world.lock().unwrap().fail_after_arm.insert(A.into());
    coordinator.recover().unwrap().unwrap();
    let entry = store.load().unwrap().unwrap().computers.remove(0);
    let named = entry
        .active_restore
        .clone()
        .expect("named before it executed");
    assert_eq!(named.instance.as_deref(), Some("upd-a#2"));
    // A different run of the same computer executes under another attempt: not ours.
    {
        let mut state = world.lock().unwrap();
        *state.runs.get_mut(A).unwrap() += 1;
        state.machines.get_mut(A).unwrap().1 = Machine::Executing("another-attempt".into());
    }
    let recovery = coordinator.recover().unwrap().unwrap();
    assert_eq!(recovery.lost.len(), 1, "{recovery:?}");
    assert_eq!(world.lock().unwrap().count("discard"), 0);
}

// MARK: Reservation of unresolved computers

#[test]
fn a_failed_recovery_reserves_its_computers_until_it_settles() {
    use crate::runtime::OPERATIONS;
    let (a, b) = ("reserved-a", "reserved-b");
    let world = world(&[
        (a, ComputerKind::Macos, Machine::Running),
        (b, ComputerKind::Macos, Machine::Running),
    ]);
    let first = lifecycle(world.clone());
    prepare(&first, "job:1");
    first
        .state
        .begin_shutdown(shutdown(ShutdownReason::Update, false))
        .unwrap();
    first.state.run_shutdown().unwrap();
    world.lock().unwrap().crash();
    world.lock().unwrap().fail_restore.insert(a.into());
    let directory = first._directory;
    let context = TestContext::new(directory.path());
    let state = crate::context::EngineContext::runtime_state(&context)
        .update()
        .clone();
    state.set_platform(Arc::new(fake_platform(
        &world,
        super::engine_directory(&context),
        &Arc::new(Mutex::new(Vec::new())),
    )));
    state.mark_recovery_pending(&context);
    assert!(super::reservation::is_reserved(a) && super::reservation::is_reserved(b));
    let recovery = state.recover(&context).unwrap().unwrap();
    assert!(!recovery.settled());
    // The settled one is free again; the unresolved one is not.
    assert!(!super::reservation::is_reserved(b));
    assert!(super::reservation::is_reserved(a));
    // Start, delete and configuration changes all pass the operation gate.
    use crate::runtime::operation_gate::{GateError, OperationKind, Scope};
    assert_eq!(
        OPERATIONS.computer(a, "a", "Start").err(),
        Some(GateError::Busy)
    );
    assert_eq!(
        OPERATIONS
            .kind(OperationKind::ComputerConfiguration)
            .computer(a, "a", "Change configuration")
            .err(),
        Some(GateError::Busy)
    );
    assert_eq!(
        OPERATIONS.removing(&[a.to_owned()], "Delete").err(),
        Some(GateError::Busy)
    );
    assert_eq!(
        OPERATIONS
            .try_acquire(Scope::Computer { id: a.to_owned() }, None, "Start")
            .err(),
        Some(GateError::Busy)
    );
    assert!(super::reservation::reason(a)
        .unwrap()
        .contains("engine update"));
    // Abort retries the unresolved computer; its reservation ends with the journal.
    world.lock().unwrap().fail_restore.clear();
    state.abort(&context).unwrap();
    assert!(!super::reservation::is_reserved(a));
    assert!(OPERATIONS.computer(a, "a", "Start").is_ok());
}

#[test]
fn an_unreadable_journal_reserves_every_computer() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::new(directory.path());
    std::fs::write(store.path(), b"not json").unwrap();
    super::reservation::sync(&store);
    assert!(super::reservation::is_reserved("anything"));
    super::reservation::clear(&store);
    assert!(!super::reservation::is_reserved("anything"));
}

#[test]
fn a_failed_preparation_that_could_not_resume_everything_stays_pending_and_reserved() {
    let (a, b) = ("failed-prep-a", "failed-prep-b");
    let world = world(&[
        (a, ComputerKind::Macos, Machine::Running),
        (b, ComputerKind::Macos, Machine::Running),
    ]);
    {
        let mut state = world.lock().unwrap();
        state.fail_save.insert(b.into());
        state.fail_resume_held.insert(a.into());
    }
    let lifecycle = lifecycle(world.clone());
    let job = JobId::new("job:9").unwrap();
    assert_eq!(lifecycle.state.begin_prepare().unwrap(), PrepareStart::New);
    lifecycle.state.prepare_job(job.clone());
    let hold = Hold::start(
        lifecycle.state.platform().unwrap(),
        super::store(&lifecycle.context),
        Some(job),
    )
    .unwrap();
    let error = hold.prepare(request()).unwrap_err();
    assert!(error.contains("could not resume"), "{error}");
    hold.end();
    lifecycle.state.prepare_failed(&lifecycle.context);
    let state = lifecycle.state.engine_state();
    assert!(state.update_pending, "the journal remains");
    assert!(lifecycle.state.recovery_error().is_some());
    assert!(super::reservation::is_reserved(a));
    assert!(!super::reservation::is_reserved(b));
    assert!(matches!(
        lifecycle.state.begin_prepare(),
        Err(LifecycleError::Refused(_))
    ));
    world.lock().unwrap().fail_resume_held.clear();
    lifecycle.state.abort(&lifecycle.context).unwrap();
    assert!(!lifecycle.state.engine_state().update_pending);
    assert!(!super::reservation::is_reserved(a));
    assert_eq!(machine(&world, a), Machine::Running);
}

// MARK: Installer ownership and the watchdog

fn after(base: Instant, seconds: u64) -> Instant {
    base + std::time::Duration::from_secs(seconds)
}

#[test]
fn the_installer_lock_is_present_only_while_a_coordinator_holds_it() {
    use super::installer::{coordinator_present, Held};
    let directory = tempfile::tempdir().unwrap();
    assert!(!coordinator_present(directory.path()), "no lock file");
    let held = Held::take(directory.path()).unwrap();
    assert!(coordinator_present(directory.path()));
    assert!(Held::take(directory.path()).is_err(), "one coordinator");
    // The probe never keeps the lock it took.
    drop(held);
    assert!(!coordinator_present(directory.path()));
    assert!(!coordinator_present(directory.path()));
    drop(Held::take(directory.path()).unwrap());
}

#[test]
fn a_coordinator_absent_for_thirty_seconds_has_the_prepared_update_aborted() {
    use super::{installer::ABANDONED_AFTER, WatchdogOutcome};
    let lifecycle = lifecycle(two_macs_and_a_linux());
    let tick = |present, at| {
        lifecycle
            .state
            .watchdog_tick(&lifecycle.context, present, at)
    };
    let t0 = Instant::now();
    assert!(
        matches!(tick(true, t0), WatchdogOutcome::Idle),
        "nothing held yet"
    );
    prepare(&lifecycle, "job:1");
    assert!(lifecycle.state.watching_installer());
    assert!(matches!(tick(false, t0), WatchdogOutcome::Waiting));
    assert!(matches!(
        tick(false, after(t0, ABANDONED_AFTER.as_secs() - 1)),
        WatchdogOutcome::Waiting
    ));
    // The coordinator came back: the count starts over.
    assert!(matches!(
        tick(true, after(t0, 31)),
        WatchdogOutcome::Waiting
    ));
    assert!(matches!(
        tick(false, after(t0, 32)),
        WatchdogOutcome::Waiting
    ));
    assert!(matches!(
        tick(false, after(t0, 61)),
        WatchdogOutcome::Waiting
    ));
    for id in [A, B, C] {
        assert_eq!(machine(&lifecycle.world, id), Machine::Held);
    }
    match tick(false, after(t0, 62)) {
        WatchdogOutcome::Aborted(Ok(())) => {}
        other => panic!("{other:?}"),
    }
    for id in [A, B, C] {
        assert_eq!(machine(&lifecycle.world, id), Machine::Running, "{id}");
    }
    assert!(!lifecycle.state.engine_state().update_pending);
    assert!(!lifecycle.state.watching_installer());
    assert!(matches!(tick(false, after(t0, 100)), WatchdogOutcome::Idle));
}

#[test]
fn a_shutdown_for_the_update_claims_the_transaction_before_the_watchdog_can() {
    use super::WatchdogOutcome;
    let lifecycle = lifecycle(two_macs_and_a_linux());
    prepare(&lifecycle, "job:1");
    let t0 = Instant::now();
    assert!(matches!(
        lifecycle.state.watchdog_tick(&lifecycle.context, false, t0),
        WatchdogOutcome::Waiting
    ));
    lifecycle
        .state
        .begin_shutdown(shutdown(ShutdownReason::Update, false))
        .unwrap();
    // The coordinator is long gone, but the shutdown owns the saved set now.
    assert!(matches!(
        lifecycle
            .state
            .watchdog_tick(&lifecycle.context, false, after(t0, 3600)),
        WatchdogOutcome::Idle
    ));
    for id in [A, B, C] {
        assert_eq!(machine(&lifecycle.world, id), Machine::Held);
    }
    assert_eq!(lifecycle.world.lock().unwrap().count("resume-held"), 0);
    // And the other way round: an abort holds the claim a shutdown would need.
    let other = self::lifecycle(two_macs_and_a_linux());
    prepare(&other, "job:2");
    other.state.abort(&other.context).unwrap();
    assert!(matches!(
        other
            .state
            .begin_shutdown(shutdown(ShutdownReason::Update, false)),
        Err(LifecycleError::Refused(_))
    ));
}

fn released_and_gone(world: &Shared, directory: &Path) {
    let store = Store::new(super::engine_directory(&TestContext::new(directory)));
    prepared_under(&store, world);
    let adapters = Fake::all(world, &store);
    Coordinator {
        store: &store,
        adapters: &adapters,
    }
    .release_for_update()
    .unwrap();
    world.lock().unwrap().crash();
}

fn engine_over(directory: &Path, version: &str, world: &Shared) -> (TestContext, Arc<UpdateState>) {
    let context = TestContext::versioned(directory, version);
    let state = crate::context::EngineContext::runtime_state(&context)
        .update()
        .clone();
    state.set_platform(Arc::new(fake_platform(
        world,
        super::engine_directory(&context),
        &Arc::new(Mutex::new(Vec::new())),
    )));
    (context, state)
}

#[test]
fn an_old_engine_relaunched_during_a_handoff_resumes_nothing_behind_the_installer() {
    use super::{installer, WatchdogOutcome};
    let directory = tempfile::tempdir().unwrap();
    let world = two_macs_and_a_linux();
    released_and_gone(&world, directory.path());
    let (context, state) = engine_over(directory.path(), "1.0.0", &world);
    let installer_lock = installer::Held::take(&super::engine_directory(&context)).unwrap();
    assert!(state.recover(&context).unwrap().is_none(), "deferred");
    assert_eq!(world.lock().unwrap().count("restore"), 0);
    assert!(state.update_pending() && state.watching_installer());
    assert!(matches!(
        state.begin_prepare(),
        Err(LifecycleError::Busy(_))
    ));
    for id in [A, B] {
        assert_eq!(machine(&world, id), Machine::Gone);
    }
    // The coordinator is still there: the engine keeps waiting.
    let t0 = Instant::now();
    assert!(matches!(
        state.watchdog_tick(&context, true, t0),
        WatchdogOutcome::Waiting
    ));
    assert_eq!(world.lock().unwrap().count("restore"), 0);
    // It disappears for good: the engine that waited restores the saved set.
    drop(installer_lock);
    assert!(matches!(
        state.watchdog_tick(&context, false, after(t0, 1)),
        WatchdogOutcome::Waiting
    ));
    match state.watchdog_tick(&context, false, after(t0, 31)) {
        WatchdogOutcome::Recovered(Ok(Some(recovery))) => assert!(recovery.settled()),
        other => panic!("{other:?}"),
    }
    for id in [A, B, C] {
        assert!(
            executing(&world, id) || machine(&world, id) == Machine::Running,
            "{id}"
        );
    }
    assert!(!state.update_pending() && !state.watching_installer());
}

#[test]
fn a_deferred_engine_still_serves_the_shutdown_for_the_update() {
    use super::installer;
    let directory = tempfile::tempdir().unwrap();
    let world = two_macs_and_a_linux();
    released_and_gone(&world, directory.path());
    let (context, state) = engine_over(directory.path(), "1.0.0", &world);
    let _installer = installer::Held::take(&super::engine_directory(&context)).unwrap();
    assert!(state.recover(&context).unwrap().is_none());
    // The coordinator finishes the handoff: the already-released set needs no release.
    assert_eq!(
        state.begin_shutdown(shutdown(ShutdownReason::Update, false)),
        Ok(ShutdownStart::New)
    );
    state.run_shutdown().unwrap();
    assert_eq!(world.lock().unwrap().count("restore"), 0);
}

#[test]
fn the_new_version_and_an_engine_without_an_installer_restore_at_once() {
    use super::installer;
    for (version, holds_lock) in [("2.0.0", true), ("1.0.0", false)] {
        let directory = tempfile::tempdir().unwrap();
        let world = two_macs_and_a_linux();
        released_and_gone(&world, directory.path());
        let (context, state) = engine_over(directory.path(), version, &world);
        let _lock =
            holds_lock.then(|| installer::Held::take(&super::engine_directory(&context)).unwrap());
        let recovery = state.recover(&context).unwrap().unwrap();
        assert!(recovery.settled(), "{version}: {recovery:?}");
        assert!(executing(&world, A) && executing(&world, B));
        assert!(!state.update_pending());
    }
}

#[test]
fn a_capturing_transaction_is_reconciled_even_while_the_installer_holds_its_lock() {
    use super::installer;
    let directory = tempfile::tempdir().unwrap();
    let world = two_macs_and_a_linux();
    let store = Store::new(super::engine_directory(&TestContext::new(directory.path())));
    // The engine died while capturing: nothing was released, nothing can authorize a swap.
    prepared_under(&store, &world);
    let mut journal = store.load().unwrap().unwrap();
    journal.phase = TransactionPhase::Capturing;
    store.save(&journal).unwrap();
    world.lock().unwrap().crash();
    let (context, state) = engine_over(directory.path(), "1.0.0", &world);
    let _lock = installer::Held::take(&super::engine_directory(&context)).unwrap();
    let recovery = state.recover(&context).unwrap().unwrap();
    assert!(recovery.settled());
    assert!(!state.watching_installer());
}

// MARK: The session-end deadline

fn session_end(deadline: Option<std::time::Duration>) -> ShutdownRequest {
    ShutdownRequest {
        stop_computers: true,
        reason: ShutdownReason::SessionEnd,
        deadline,
    }
}

#[test]
fn the_earliest_session_end_deadline_wins_whenever_it_was_asked() {
    use std::time::Duration;
    let lifecycle = lifecycle(two_macs_and_a_linux());
    assert_eq!(lifecycle.state.deadline(), None);
    lifecycle
        .state
        .begin_shutdown(shutdown(ShutdownReason::User, true))
        .unwrap();
    lifecycle.state.shutdown_job(JobId::new("job:1").unwrap());
    // A user shutdown has no deadline; the session ending gives it one.
    assert_eq!(lifecycle.state.deadline(), None);
    lifecycle
        .state
        .begin_shutdown(session_end(Some(Duration::from_secs(3600))))
        .unwrap();
    let first = lifecycle.state.deadline().unwrap();
    // A second SIGTERM with a shorter budget tightens it; a longer one never loosens it.
    lifecycle
        .state
        .begin_shutdown(session_end(Some(Duration::from_secs(10))))
        .unwrap();
    let tightened = lifecycle.state.deadline().unwrap();
    assert!(tightened < first);
    lifecycle
        .state
        .begin_shutdown(session_end(Some(Duration::from_secs(7200))))
        .unwrap();
    assert_eq!(lifecycle.state.deadline(), Some(tightened));
    lifecycle.state.begin_shutdown(session_end(None)).unwrap();
    assert_eq!(lifecycle.state.deadline(), Some(tightened));
    // The default budget applies to a request that names none.
    let fresh = self::lifecycle(two_macs_and_a_linux());
    fresh.state.begin_shutdown(session_end(None)).unwrap();
    let remaining = fresh
        .state
        .deadline()
        .unwrap()
        .saturating_duration_since(Instant::now());
    assert!(remaining <= super::SESSION_END_BUDGET && remaining > Duration::from_secs(1));
}

#[test]
fn a_blocked_session_end_stop_is_cut_off_at_the_deadline_and_the_process_still_exits() {
    let lifecycle = lifecycle(two_macs_and_a_linux());
    let (release, blocked) = std::sync::mpsc::channel::<()>();
    *lifecycle.block_stop.lock().unwrap() = Some(blocked);
    lifecycle
        .state
        .begin_shutdown(session_end(Some(std::time::Duration::from_millis(30))))
        .unwrap();
    lifecycle.state.shutdown_job(JobId::new("job:1").unwrap());
    // Without the deadline this would wait for the gate or the stop forever.
    lifecycle.state.run_shutdown().unwrap();
    assert_eq!(lifecycle.world.lock().unwrap().count("stop-computers"), 0);
    let events = lifecycle.events.lock().unwrap().clone();
    assert!(events.contains(&"flush settings".to_owned()), "{events:?}");
    assert!(events.contains(&"close connections".to_owned()));
    drop(release);
}

#[test]
fn a_second_sigterm_tightens_the_deadline_of_work_already_blocked() {
    use std::time::Duration;
    let lifecycle = Arc::new(lifecycle(two_macs_and_a_linux()));
    let (release, blocked) = std::sync::mpsc::channel::<()>();
    *lifecycle.block_stop.lock().unwrap() = Some(blocked);
    lifecycle
        .state
        .begin_shutdown(session_end(Some(Duration::from_secs(3600))))
        .unwrap();
    lifecycle.state.shutdown_job(JobId::new("job:1").unwrap());
    let (done, finished) = std::sync::mpsc::channel();
    let worker = {
        let lifecycle = lifecycle.clone();
        std::thread::spawn(move || {
            let outcome = lifecycle.state.run_shutdown();
            let _ = done.send(outcome);
        })
    };
    // Escalation: the next termination signal asks for a much earlier end.
    assert!(matches!(
        lifecycle
            .state
            .begin_shutdown(session_end(Some(Duration::from_millis(20)))),
        Ok(ShutdownStart::Attach(_))
    ));
    let outcome = finished
        .recv_timeout(Duration::from_secs(60))
        .expect("the shutdown ended at the tightened deadline");
    assert_eq!(outcome, Ok(()));
    worker.join().unwrap();
    drop(release);
}
