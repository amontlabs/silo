//! macOS computers in an engine-update save (design/p2 §3.1, probe B-30).
//!
//! A macOS machine lives inside the engine process, so replacing the engine ends it. The
//! qualified route (probe B-30) is: pause, `saveMachineStateToURL`, clone the disk and
//! auxiliary storage while still paused, replace the owner, `restoreMachineStateFromURL` in a
//! new process with the same configuration, resume.
//!
//! * `save_and_hold` pauses and saves, then *keeps the machine paused*: nothing may let it
//!   execute between its captured state and the engine's exit. A held computer owns a
//!   checkpoint-style operation so the watcher that resumes stray pauses leaves it alone.
//! * `release` verifies the capture and leaves the paused machine for the process exit to end;
//!   the machine is never asked to shut down and the engine never resumes it.
//! * `restore` runs in the new process: it puts clones of the captured disk and auxiliary
//!   storage in place, restores the memory into a new machine that stays paused, and calls
//!   `arm` before the resume. It never boots from the disk: a refused memory is an error.
use super::*;
use crate::engine_update::{
    adapter::{OriginalState, Reconciled, RestoreError, ResumeHeldError, SaveAdapter, SaveContext},
    journal::{boot_id, CaptureRef, ComputerKind, MacCapture, RuntimeIdentity},
    RunningComputer,
};
use std::{
    fs,
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
};

const SAVES: &str = "update-saves";
const DISK: &str = "disk.img";
const AUXILIARY: &str = "auxiliary-storage.img";
const STAGING_SUFFIX: &str = ".restoring";
const SAVE_FORMAT_VERSION: u32 = 1;
const HOLD_STAGE: &str = "Saving the computer for the update";

pub struct MacosUpdateAdapter {
    app: MacosContext,
}

impl MacosUpdateAdapter {
    pub fn new(app: MacosContext) -> Self {
        Self { app }
    }

    fn layout(&self, id: &str) -> Result<Layout, String> {
        Ok(Layout::new(&app_data(&self.app)?, id))
    }

    fn capture(
        &self,
        computer: &RunningComputer,
        context: &SaveContext<'_>,
    ) -> Result<MacCapture, String> {
        ensure_loaded(&self.app)?;
        let data = app_data(&self.app)?;
        let layout = Layout::new(&data, &computer.id);
        let record = begin_hold(&self.app, &computer.id)?;
        let directory = layout.dir.join(SAVES).join(context.transaction_id);
        let result = (|| {
            private_directory(&layout.dir, &directory)?;
            engine::memory_support(&self.app, &computer.id)?;
            // Memory is written in full; the disk clones share blocks until either side changes.
            let _space = templates::reserve_space(&data, record.memory_gib * GIB)?;
            let state = directory.join(checkpoints::STATE);
            let generation = engine::save_and_hold(&self.app, &computer.id, &state, &mut || {
                clone_into(&layout, &directory)
            })?;
            checkpoints::secure_state(&state)?;
            let artifact_sizes = sync_artifacts(&directory)?;
            Ok(MacCapture {
                state_file: state,
                disk: directory.join(DISK),
                auxiliary_storage: directory.join(AUXILIARY),
                directory: directory.clone(),
                source_generation: generation,
                host_build: checkpoints::host_build(),
                save_format_version: SAVE_FORMAT_VERSION,
                artifact_sizes,
            })
        })();
        if result.is_err() {
            // The machine was resumed by `save_and_hold`; the computer is its own again.
            end_hold(&self.app, &computer.id);
            let _ = fs::remove_dir_all(&directory);
        }
        result
    }
}

impl SaveAdapter for MacosUpdateAdapter {
    fn kind(&self) -> ComputerKind {
        ComputerKind::Macos
    }

    fn running(&self) -> Result<Vec<RunningComputer>, String> {
        ensure_loaded(&self.app)?;
        classify(&registry().entries)
    }

    fn check_support(&self, computer: &RunningComputer) -> Result<(), String> {
        engine::memory_support(&self.app, &computer.id)?;
        let record = computer_record(&computer.id)?;
        // Checked now, while everything still runs, rather than after the first pause.
        templates::reserve_space(&app_data(&self.app)?, record.memory_gib * GIB).map(|_| ())
    }

    fn save_and_hold(
        &self,
        computer: &RunningComputer,
        context: &SaveContext<'_>,
    ) -> Result<CaptureRef, String> {
        self.capture(computer, context).map(CaptureRef::Macos)
    }

    fn identity(&self, computer: &RunningComputer) -> Result<RuntimeIdentity, String> {
        engine::machine_samples(&self.app)?
            .iter()
            .find(|sample| sample.id == computer.id)
            .map(|sample| machine_identity(sample.generation))
            .ok_or_else(|| format!("{} is not running.", computer.name))
    }

    fn resume_held(
        &self,
        computer: &RunningComputer,
        original: &RuntimeIdentity,
    ) -> Result<(), ResumeHeldError> {
        let samples = engine::machine_samples(&self.app).map_err(ResumeHeldError::Failed)?;
        let Some(sample) = samples.iter().find(|sample| {
            sample.id == computer.id && original.matches(&machine_identity(sample.generation))
        }) else {
            return Err(ResumeHeldError::Gone(format!(
                "{} is no longer held by Silo.",
                computer.name
            )));
        };
        match sample.state {
            engine::MachineState::Running => {}
            engine::MachineState::Paused => {
                engine::resume_paused(&self.app, &computer.id).map_err(ResumeHeldError::Failed)?
            }
            _ => {
                return Err(ResumeHeldError::Gone(format!(
                    "{} is not paused or running.",
                    computer.name
                )))
            }
        }
        end_hold(&self.app, &computer.id);
        Ok(())
    }

    fn probe_original(
        &self,
        computer: &RunningComputer,
        original: &RuntimeIdentity,
    ) -> Result<OriginalState, String> {
        let samples = engine::machine_samples(&self.app)?;
        Ok(classify_original(&samples, &computer.id, original))
    }

    fn release(
        &self,
        computer: &RunningComputer,
        capture: &CaptureRef,
        original: &RuntimeIdentity,
    ) -> Result<(), String> {
        let CaptureRef::Macos(capture) = capture else {
            return Err("The capture does not belong to a macOS computer.".into());
        };
        verify_capture(&self.layout(&computer.id)?, capture)?;
        let samples = engine::machine_samples(&self.app)?;
        let held = samples.iter().any(|sample| {
            sample.id == computer.id
                && sample.state == engine::MachineState::Paused
                && sample.generation == capture.source_generation
                && original.matches(&machine_identity(sample.generation))
        });
        if !held {
            return Err(format!(
                "{} is not held paused at its saved state; the update would not preserve its run.",
                computer.name
            ));
        }
        // Nothing resumes it: the process exit ends the paused machine. Closing the display
        // window and the framework object here would only risk a callback on a half-released
        // machine, and B-30 qualified exactly this: the owner process ends, a new one restores.
        Ok(())
    }

    fn reconcile_unsaved(
        &self,
        computer: &RunningComputer,
        original: &RuntimeIdentity,
    ) -> Result<Reconciled, String> {
        let samples = engine::machine_samples(&self.app)?;
        let sample = samples
            .iter()
            .find(|sample| sample.id == computer.id)
            .filter(|sample| original.matches(&machine_identity(sample.generation)));
        match sample {
            Some(sample) if sample.state == engine::MachineState::Running => {
                Ok(Reconciled::Running)
            }
            Some(sample) if sample.state == engine::MachineState::Paused => {
                engine::resume_paused(&self.app, &computer.id)?;
                end_hold(&self.app, &computer.id);
                Ok(Reconciled::Running)
            }
            _ => Ok(Reconciled::Lost(format!(
                "{} stopped before its memory was saved. Its disk was kept; start it again.",
                computer.name
            ))),
        }
    }

    fn restore(
        &self,
        computer: &RunningComputer,
        capture: &CaptureRef,
        _original: &RuntimeIdentity,
        _attempt_id: &str,
        arm: &mut dyn FnMut(Option<&RuntimeIdentity>) -> Result<(), String>,
    ) -> Result<(), RestoreError> {
        let CaptureRef::Macos(capture) = capture else {
            return Err("The capture does not belong to a macOS computer."
                .to_owned()
                .into());
        };
        ensure_loaded(&self.app)?;
        // A machine that runs now is not one this transaction restored: its disk is its own.
        if engine::machine_samples(&self.app)?
            .iter()
            .any(|sample| sample.id == computer.id)
        {
            return Err(RestoreError::Superseded(format!(
                "{} is running again without being restored from the update. Its disk was kept and the saved state was not applied.",
                computer.name
            )));
        }
        let layout = self.layout(&computer.id)?;
        verify_capture(&layout, capture)?;
        let current_build = checkpoints::host_build();
        if capture.host_build != current_build {
            // The framework rejects a saved state after some host updates; booting from the
            // disk instead would end the run the update promised to keep.
            return Err(format!(
                "This Mac was updated (macOS build {} to {}) after {} was saved, so its memory can't be restored. The saved state was kept.",
                capture.host_build, current_build, computer.name
            )
            .into());
        }
        let (record, attempt) = begin_restore(&self.app, &computer.id)?;
        let restored = restore_machine(&self.app, &record, &layout, capture, attempt, arm);
        if restored.is_err() {
            // Storage is released only when the framework says so: a machine that refused to
            // stop keeps the computer `stopping` (busy, open to Force stop), never `stopped`.
            let released = ensure_released(&self.app, &computer.id, attempt);
            settle_failed_restore(&mut registry(), &computer.id, attempt, released);
            emit(&self.app);
        }
        restored.map_err(RestoreError::Failed)
    }

    fn probe_armed(
        &self,
        computer: &RunningComputer,
        _capture: &CaptureRef,
        _original: &RuntimeIdentity,
        _attempt_id: &str,
        expected: Option<&RuntimeIdentity>,
    ) -> Result<bool, String> {
        // The armed restore named its machine before the guest could execute; without that
        // identity (or in another process, where the machine died with its owner) the
        // attempt cannot be proved running.
        let Some(expected) = expected else {
            return Ok(false);
        };
        Ok(engine::machine_samples(&self.app)?.iter().any(|sample| {
            sample.id == computer.id
                && sample.state == engine::MachineState::Running
                && expected.matches(&machine_identity(sample.generation))
        }))
    }

    fn discard(&self, computer: &RunningComputer, capture: &CaptureRef) -> Result<(), String> {
        let CaptureRef::Macos(capture) = capture else {
            return Ok(());
        };
        let layout = self.layout(&computer.id)?;
        check_location(&layout, capture)?;
        match fs::remove_dir_all(&capture.directory) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(store::io_error("remove the saved state", &error)),
        }
        // The folder of saves goes once its last transaction does.
        let _ = fs::remove_dir(layout.dir.join(SAVES));
        Ok(())
    }

    fn owns_live_machines(&self) -> bool {
        registry().entries.iter().any(|entry| {
            matches!(
                entry.state,
                State::Running | State::Starting | State::Stopping
            )
        })
    }
}

/// The identity of the machine of `generation` in this engine process.
fn machine_identity(generation: u64) -> RuntimeIdentity {
    RuntimeIdentity {
        owner: Some(boot_id().to_owned()),
        generation: Some(generation),
        instance: None,
    }
}

/// Where the original machine stands: only the exact run (same process, same generation)
/// counts, and a paused one can still be resumed.
fn classify_original(
    samples: &[engine::Sample],
    id: &str,
    original: &RuntimeIdentity,
) -> OriginalState {
    match samples
        .iter()
        .find(|sample| sample.id == id && original.matches(&machine_identity(sample.generation)))
    {
        Some(sample) if sample.state == engine::MachineState::Running => OriginalState::Running,
        Some(sample) if sample.state == engine::MachineState::Paused => OriginalState::Held,
        _ => OriginalState::Gone,
    }
}

/// Settles a restore that failed after `begin_restore`: stopped only once the machine is
/// proved released, otherwise `stopping` with the reason.
pub(super) fn settle_failed_restore(
    registry: &mut Registry,
    id: &str,
    attempt: u64,
    released: Result<(), String>,
) {
    registry.settle_failed_start(id, attempt, released);
}

/// Closes admission of new macOS work for the update. Running computers are allowed (they are
/// saved and resumed); anything changing state or owned by an operation refuses the update.
pub fn close_for_save(app: &MacosContext) -> Result<(), String> {
    ensure_loaded(app)?;
    let mut closed = closed();
    classify(&registry().entries)?;
    closed.update = true;
    Ok(())
}

/// The computers an update would save, or why it cannot start.
fn classify(entries: &[Entry]) -> Result<Vec<RunningComputer>, String> {
    let mut running = Vec::new();
    for entry in entries {
        let wait = || {
            format!(
                "Wait until {} has finished its current action before updating.",
                entry.record.name
            )
        };
        if entry.deleting || entry.operation.is_some() {
            return Err(wait());
        }
        match entry.state {
            State::Stopped | State::Failed => {}
            State::Running => running.push(RunningComputer {
                id: entry.record.id.clone(),
                name: entry.record.name.clone(),
                kind: ComputerKind::Macos,
            }),
            _ => return Err(wait()),
        }
    }
    Ok(running)
}

fn computer_record(id: &str) -> Result<Record, String> {
    computer(id).map(|(record, _)| record)
}

/// Claims a running computer for the update's hold: the watcher that resumes stray pauses
/// and every other operation leave it alone until `end_hold`.
fn begin_hold(app: &MacosContext, id: &str) -> Result<Record, String> {
    let record = {
        let mut registry = registry();
        let entry = registry
            .entry(id)
            .ok_or("This computer no longer exists.")?;
        if entry.state != State::Running || entry.operation.is_some() || entry.deleting {
            return Err(format!(
                "Wait until {} has finished its current action before updating.",
                entry.record.name
            ));
        }
        entry.operation = Some(checkpoints::Operation::running(
            checkpoints::OperationKind::Capture,
            HOLD_STAGE,
        ));
        entry.record.clone()
    };
    emit(app);
    Ok(record)
}

fn end_hold(app: &MacosContext, id: &str) {
    update(app, id, |entry| entry.operation = None);
}

/// Moves a stopped computer to `starting` for its restore, as Start does.
fn begin_restore(app: &MacosContext, id: &str) -> Result<(Record, u64), String> {
    let begun = {
        let mut registry = registry();
        let entry = registry
            .entry(id)
            .ok_or("This computer no longer exists.")?;
        store::start_allowed(entry.state, &entry.record)?;
        if entry.operation.is_some() || entry.deleting {
            return Err(checkpoints::BUSY.into());
        }
        entry.state = State::Starting;
        entry.attempt += 1;
        entry.detail = None;
        entry.since = Instant::now();
        (entry.record.clone(), entry.attempt)
    };
    emit(app);
    watch(app);
    Ok(begun)
}

fn restore_machine(
    app: &MacosContext,
    record: &Record,
    layout: &Layout,
    capture: &MacCapture,
    attempt: u64,
    arm: &mut dyn FnMut(Option<&RuntimeIdentity>) -> Result<(), String>,
) -> Result<(), String> {
    offline_setup::ensure_detached(&layout.disk())?;
    replace_live_files(layout, capture)?;
    let generation = match engine::start_from_state(app, record, layout, &capture.state_file) {
        Ok(generation) => generation,
        Err(engine::StateStartError::Rejected(why)) => {
            return Err(format!(
                "macOS could not restore {}'s memory ({why}). The computer was not started from its disk; the saved state was kept.",
                record.name
            ))
        }
        Err(engine::StateStartError::Failed(message)) => return Err(message),
    };
    // The restored machine is paused. The journal says the guest is about to execute before
    // the first instruction runs.
    if let Err(message) = arm(Some(&machine_identity(generation))) {
        let _ = engine::force_stop_generation(app, &record.id, generation);
        return Err(message);
    }
    let guard = {
        let id = record.id.clone();
        move || registry().start_is_current(&id, attempt)
    };
    if let Err(message) = engine::resume_if(app, &record.id, generation, guard) {
        let _ = engine::force_stop_generation(app, &record.id, generation);
        return Err(message);
    }
    update(app, &record.id, |entry| {
        if entry.state == State::Starting && entry.attempt == attempt {
            entry.state = State::Running;
            entry.since = Instant::now();
            entry.detail = None;
        }
    });
    log_line(app, &record.id, "resumed after the engine update");
    Ok(())
}

/// What creating a durable directory chain does to the filesystem; tests substitute a recorder.
trait DirOps {
    fn exists(&self, path: &Path) -> bool;
    fn create(&self, path: &Path) -> std::io::Result<()>;
    fn sync(&self, path: &Path) -> std::io::Result<()>;
}

struct RealDirOps;

impl DirOps for RealDirOps {
    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn create(&self, path: &Path) -> std::io::Result<()> {
        match fs::DirBuilder::new().mode(0o700).create(path) {
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
            other => other,
        }
    }

    fn sync(&self, path: &Path) -> std::io::Result<()> {
        store::sync_dir(path)
    }
}

/// Creates `path` below `base` (an existing, durable directory) privately, and makes every
/// level durable: each directory is created if missing, synced, and then its parent is synced,
/// so a power loss cannot keep the journal's reference to a capture while losing the
/// directories that lead to it. Levels that already exist are synced again, so a retry after a
/// failed sync completes what the failed attempt did not.
fn create_durable_directory(ops: &dyn DirOps, base: &Path, path: &Path) -> std::io::Result<()> {
    let relative = path.strip_prefix(base).map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "path outside its base")
    })?;
    let mut current = base.to_path_buf();
    for component in relative.components() {
        let parent = current.clone();
        current.push(component);
        if !ops.exists(&current) {
            ops.create(&current)?;
        }
        ops.sync(&current)?;
        ops.sync(&parent)?;
    }
    Ok(())
}

fn private_directory(base: &Path, path: &Path) -> Result<(), String> {
    create_durable_directory(&RealDirOps, base, path)
        .map_err(|error| store::io_error("save the computer for the update", &error))
}

fn clone_into(layout: &Layout, directory: &Path) -> Result<(), String> {
    templates::clone_file(&layout.disk(), &directory.join(DISK))
        .and_then(|()| {
            templates::clone_file(&layout.auxiliary_storage(), &directory.join(AUXILIARY))
        })
        .map_err(templates::CopyError::message)
}

/// Syncs every artifact and the directory holding them, then records their sizes, so a capture
/// is durable before the journal names it.
fn sync_artifacts(directory: &Path) -> Result<std::collections::BTreeMap<String, u64>, String> {
    let io = |error: std::io::Error| store::io_error("save the computer for the update", &error);
    let mut sizes = std::collections::BTreeMap::new();
    for name in [checkpoints::STATE, DISK, AUXILIARY] {
        let file = fs::File::open(directory.join(name)).map_err(io)?;
        file.sync_all().map_err(io)?;
        sizes.insert(name.to_owned(), file.metadata().map_err(io)?.len());
    }
    store::sync_dir(directory).map_err(io)?;
    Ok(sizes)
}

/// The capture must sit exactly where this computer's saves go, with the expected names.
fn check_location(layout: &Layout, capture: &MacCapture) -> Result<(), String> {
    let saves = layout.dir.join(SAVES);
    let relative = capture
        .directory
        .strip_prefix(&saves)
        .map_err(|_| "The saved state is outside the computer's folder; it was kept.".to_owned())?;
    let mut parts = relative.components();
    let single = matches!(
        (parts.next(), parts.next()),
        (Some(std::path::Component::Normal(_)), None)
    );
    if !single
        || capture.state_file != capture.directory.join(checkpoints::STATE)
        || capture.disk != capture.directory.join(DISK)
        || capture.auxiliary_storage != capture.directory.join(AUXILIARY)
    {
        return Err("The saved state has an unexpected layout; it was kept.".into());
    }
    Ok(())
}

/// Every artifact is present and has the size it had when it was made durable.
fn verify_capture(layout: &Layout, capture: &MacCapture) -> Result<(), String> {
    check_location(layout, capture)?;
    if capture.save_format_version != SAVE_FORMAT_VERSION {
        return Err("The saved state has an unsupported format; it was kept.".into());
    }
    for name in [checkpoints::STATE, DISK, AUXILIARY] {
        let expected = capture
            .artifact_sizes
            .get(name)
            .ok_or("The saved state lacks a size record; it was kept.")?;
        let actual = fs::metadata(capture.directory.join(name))
            .map_err(|_| format!("The saved state is missing {name}; it was kept."))?
            .len();
        if actual != *expected {
            return Err(format!(
                "The saved {name} changed after it was saved; it was kept."
            ));
        }
    }
    Ok(())
}

/// Replaces the computer's disk and auxiliary storage with clones of the capture's: both
/// clones are complete and synced under temporary names before either is renamed in. Safe to
/// repeat after an interruption, because the capture is immutable and the machine has not run.
fn replace_live_files(layout: &Layout, capture: &MacCapture) -> Result<(), String> {
    let io = |error: std::io::Error| store::io_error("restore the computer's files", &error);
    let pairs = [
        (layout.disk(), &capture.disk),
        (layout.auxiliary_storage(), &capture.auxiliary_storage),
    ];
    let staged: Vec<PathBuf> = pairs
        .iter()
        .map(|(live, _)| {
            let mut name = live.as_os_str().to_os_string();
            name.push(STAGING_SUFFIX);
            PathBuf::from(name)
        })
        .collect();
    for (staged, (_, source)) in staged.iter().zip(&pairs) {
        let _ = fs::remove_file(staged);
        templates::clone_file(source, staged).map_err(templates::CopyError::message)?;
        fs::File::open(staged)
            .and_then(|file| file.sync_all())
            .map_err(io)?;
    }
    for (staged, (live, _)) in staged.iter().zip(&pairs) {
        fs::rename(staged, live).map_err(io)?;
    }
    store::sync_dir(&layout.dir).map_err(io)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(name: &str, id: &str) -> Record {
        let mut record = store::new_record(
            &store::CreateRequest {
                name: name.into(),
                cpus: 2,
                memory_gib: 4,
                disk_gib: 64,
            },
            "02:00:00:00:00:01".into(),
        );
        record.id = id.into();
        record.installed = true;
        record
    }

    fn entry(name: &str, id: &str, state: State) -> Entry {
        Entry::new(record(name, id), state, None)
    }

    #[test]
    fn only_running_computers_are_saved_and_a_busy_one_refuses_the_update() {
        let entries = vec![
            entry(
                "one",
                "11111111-1111-4111-8111-111111111111",
                State::Stopped,
            ),
            entry(
                "two",
                "22222222-2222-4222-8222-222222222222",
                State::Running,
            ),
            entry(
                "three",
                "33333333-3333-4333-8333-333333333333",
                State::Failed,
            ),
        ];
        let running = classify(&entries).unwrap();
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].name, "two");
        assert_eq!(running[0].kind, ComputerKind::Macos);

        for busy in [
            State::Starting,
            State::Stopping,
            State::SettingUp,
            State::Copying,
        ] {
            let mut entries = entries_with(busy);
            assert!(classify(&entries)
                .unwrap_err()
                .contains("Wait until busy has finished"));
            entries[1].state = State::Running;
            entries[1].operation = Some(checkpoints::Operation::running(
                checkpoints::OperationKind::Capture,
                "Preparing",
            ));
            assert!(classify(&entries).unwrap_err().contains("Wait until busy"));
        }
        let mut deleting = entries_with(State::Stopped);
        deleting[1].deleting = true;
        assert!(classify(&deleting).is_err());
    }

    fn entries_with(state: State) -> Vec<Entry> {
        vec![
            entry(
                "one",
                "11111111-1111-4111-8111-111111111111",
                State::Running,
            ),
            entry("busy", "22222222-2222-4222-8222-222222222222", state),
        ]
    }

    fn capture_in(layout: &Layout) -> MacCapture {
        let directory = layout.dir.join(SAVES).join("tx-1");
        private_directory(&layout.dir, &directory).unwrap();
        fs::write(directory.join(checkpoints::STATE), b"memory").unwrap();
        fs::write(directory.join(DISK), b"captured disk").unwrap();
        fs::write(directory.join(AUXILIARY), b"captured auxiliary").unwrap();
        MacCapture {
            state_file: directory.join(checkpoints::STATE),
            disk: directory.join(DISK),
            auxiliary_storage: directory.join(AUXILIARY),
            artifact_sizes: sync_artifacts(&directory).unwrap(),
            directory,
            source_generation: 1,
            host_build: "25F71".into(),
            save_format_version: SAVE_FORMAT_VERSION,
        }
    }

    fn layout(data: &tempfile::TempDir) -> Layout {
        let layout = Layout::new(data.path(), "11111111-1111-4111-8111-111111111111");
        fs::create_dir_all(&layout.dir).unwrap();
        fs::write(layout.disk(), b"live disk").unwrap();
        fs::write(layout.auxiliary_storage(), b"live auxiliary").unwrap();
        layout
    }

    #[test]
    fn a_capture_must_be_where_this_computer_keeps_its_saves_with_every_artifact_intact() {
        let data = tempfile::tempdir().unwrap();
        let layout = layout(&data);
        let capture = capture_in(&layout);
        verify_capture(&layout, &capture).unwrap();

        let mut moved = capture.clone();
        moved.directory = data.path().join("elsewhere");
        assert!(verify_capture(&layout, &moved)
            .unwrap_err()
            .contains("outside"));
        let mut nested = capture.clone();
        nested.directory = layout.dir.join(SAVES).join("a").join("b");
        assert!(verify_capture(&layout, &nested).is_err());
        let mut renamed = capture.clone();
        renamed.state_file = capture.directory.join("other");
        assert!(verify_capture(&layout, &renamed)
            .unwrap_err()
            .contains("layout"));
        let mut other_format = capture.clone();
        other_format.save_format_version = 99;
        assert!(verify_capture(&layout, &other_format)
            .unwrap_err()
            .contains("format"));

        fs::write(&capture.disk, b"tampered disk content").unwrap();
        assert!(verify_capture(&layout, &capture)
            .unwrap_err()
            .contains("changed"));
        fs::remove_file(&capture.state_file).unwrap();
        assert!(verify_capture(&layout, &capture)
            .unwrap_err()
            .contains("missing"));
    }

    #[test]
    fn restoring_replaces_the_live_files_with_the_captured_state_and_is_repeatable() {
        let data = tempfile::tempdir().unwrap();
        let layout = layout(&data);
        let capture = capture_in(&layout);
        replace_live_files(&layout, &capture).unwrap();
        assert_eq!(fs::read(layout.disk()).unwrap(), b"captured disk");
        assert_eq!(
            fs::read(layout.auxiliary_storage()).unwrap(),
            b"captured auxiliary"
        );
        // An interruption left a staged file behind: the next attempt starts clean.
        fs::write(layout.dir.join("disk.img.restoring"), b"stale").unwrap();
        fs::write(&layout.disk(), b"advanced disk").unwrap();
        replace_live_files(&layout, &capture).unwrap();
        assert_eq!(fs::read(layout.disk()).unwrap(), b"captured disk");
        assert!(!layout.dir.join("disk.img.restoring").exists());
        // The capture itself is untouched.
        assert_eq!(fs::read(&capture.disk).unwrap(), b"captured disk");
    }

    use std::{collections::BTreeSet, sync::Mutex};

    /// A filesystem that records what is done to it and can fail at any step.
    struct Recorder {
        directories: Mutex<BTreeSet<PathBuf>>,
        events: Mutex<Vec<String>>,
        fail_at: Mutex<Option<usize>>,
    }

    impl Recorder {
        fn new(existing: &[&str]) -> Self {
            Self {
                directories: Mutex::new(existing.iter().map(PathBuf::from).collect()),
                events: Mutex::new(Vec::new()),
                fail_at: Mutex::new(None),
            }
        }

        fn step(&self, event: String) -> std::io::Result<()> {
            let mut events = self.events.lock().unwrap();
            events.push(event);
            if *self.fail_at.lock().unwrap() == Some(events.len()) {
                events.pop();
                return Err(std::io::Error::other("injected"));
            }
            Ok(())
        }

        fn events(&self) -> Vec<String> {
            self.events.lock().unwrap().clone()
        }
    }

    impl DirOps for Recorder {
        fn exists(&self, path: &Path) -> bool {
            self.directories.lock().unwrap().contains(path)
        }

        fn create(&self, path: &Path) -> std::io::Result<()> {
            self.step(format!("create {}", path.display()))?;
            self.directories.lock().unwrap().insert(path.to_path_buf());
            Ok(())
        }

        fn sync(&self, path: &Path) -> std::io::Result<()> {
            self.step(format!("sync {}", path.display()))
        }
    }

    #[test]
    fn every_new_ancestor_of_a_capture_is_created_then_synced_then_its_parent_is() {
        let ops = Recorder::new(&["/data", "/data/c"]);
        create_durable_directory(
            &ops,
            Path::new("/data/c"),
            Path::new("/data/c/update-saves/tx"),
        )
        .unwrap();
        assert_eq!(
            ops.events(),
            [
                "create /data/c/update-saves",
                "sync /data/c/update-saves",
                "sync /data/c",
                "create /data/c/update-saves/tx",
                "sync /data/c/update-saves/tx",
                "sync /data/c/update-saves",
            ]
        );
    }

    #[test]
    fn a_failure_at_any_filesystem_step_fails_the_capture_and_a_retry_completes_the_syncs() {
        let steps = {
            let ops = Recorder::new(&["/data/c"]);
            create_durable_directory(&ops, Path::new("/data/c"), Path::new("/data/c/a/b")).unwrap();
            ops.events().len()
        };
        assert_eq!(steps, 6);
        for failing in 1..=steps {
            let ops = Recorder::new(&["/data/c"]);
            *ops.fail_at.lock().unwrap() = Some(failing);
            let error =
                create_durable_directory(&ops, Path::new("/data/c"), Path::new("/data/c/a/b"));
            assert!(error.is_err(), "step {failing}");
            // Nothing later ran: a capture is never recorded over a path that is not durable.
            assert_eq!(ops.events().len(), failing - 1, "step {failing}");
            // The retry sees directories that already exist and still syncs every level, so
            // the entry that was created but never synced becomes durable.
            *ops.fail_at.lock().unwrap() = None;
            let before = ops.events().len();
            create_durable_directory(&ops, Path::new("/data/c"), Path::new("/data/c/a/b")).unwrap();
            let retried = ops.events()[before..].to_vec();
            for needed in ["sync /data/c/a", "sync /data/c", "sync /data/c/a/b"] {
                assert!(
                    retried.iter().any(|event| event == needed),
                    "step {failing}: {retried:?}"
                );
            }
        }
    }

    #[test]
    fn the_real_filesystem_chain_is_private_and_a_path_outside_its_base_is_refused() {
        let data = tempfile::tempdir().unwrap();
        let base = data.path().join("computer");
        fs::create_dir_all(&base).unwrap();
        let path = base.join("update-saves").join("tx");
        private_directory(&base, &path).unwrap();
        private_directory(&base, &path).unwrap();
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert!(private_directory(&base, &data.path().join("elsewhere")).is_err());
    }
}
