//! The engine-update transaction: prepare, abort, release and recovery (design/p2 §3.3-3.4).
//!
//! Everything here is synchronous and runs on the update hold thread. The journal is written
//! before the first computer is paused, after every save, before a release, before a restored
//! guest may execute (`resumeArmed`) and after it resumed. Nothing here ever cold boots a
//! computer: a computer whose memory cannot be brought back is reported, never restarted.
use std::{cell::Cell, cell::RefCell, path::Path};

use super::{
    adapter::{
        OriginalState, Reconciled, RestoreError, ResumeHeldError, RunningComputer, SaveAdapter,
        SaveContext,
    },
    journal::{
        CaptureRef, ComputerKind, ComputerPhase, Journal, SavedComputer, Store, TransactionPhase,
    },
};

/// What identifies one update attempt in the journal.
#[derive(Clone, Debug)]
pub struct PrepareRequest {
    pub transaction_id: String,
    pub prepare_job_id: String,
    pub source_engine_version: String,
    pub host_build: String,
}

/// How a restore settled each computer.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Recovery {
    pub resumed: Vec<RunningComputer>,
    /// Computers whose memory is gone. Their disks are preserved and untouched.
    pub lost: Vec<(RunningComputer, String)>,
    /// Computers still journaled: a later recovery, or `engine.update.abort`, retries them.
    pub failed: Vec<(RunningComputer, String)>,
}

impl Recovery {
    pub fn settled(&self) -> bool {
        self.failed.is_empty()
    }
}

pub struct Coordinator<'a> {
    pub store: &'a Store,
    pub adapters: &'a [Box<dyn SaveAdapter>],
}

/// The journal in memory with every change written through to disk.
struct Session<'a> {
    store: &'a Store,
    journal: RefCell<Journal>,
}

impl<'a> Session<'a> {
    fn new(store: &'a Store, journal: Journal) -> Self {
        Self {
            store,
            journal: RefCell::new(journal),
        }
    }

    fn save(&self) -> Result<(), String> {
        self.store.save(&self.journal.borrow())
    }

    fn set_phase(&self, phase: TransactionPhase) -> Result<(), String> {
        self.journal.borrow_mut().phase = phase;
        self.save()
    }

    fn update(&self, index: usize, change: impl FnOnce(&mut SavedComputer)) -> Result<(), String> {
        change(&mut self.journal.borrow_mut().computers[index]);
        self.save()
    }

    fn entry(&self, index: usize) -> SavedComputer {
        self.journal.borrow().computers[index].clone()
    }

    fn len(&self) -> usize {
        self.journal.borrow().computers.len()
    }
}

fn running_computer(entry: &SavedComputer) -> RunningComputer {
    RunningComputer {
        id: entry.id.clone(),
        name: entry.name.clone(),
        kind: entry.kind,
    }
}

impl Coordinator<'_> {
    fn adapter(&self, kind: ComputerKind) -> Result<&dyn SaveAdapter, String> {
        self.adapters
            .iter()
            .find(|adapter| adapter.kind() == kind)
            .map(|adapter| adapter.as_ref())
            .ok_or_else(|| format!("This engine cannot save {kind:?} computers."))
    }

    /// The journal's phase, if a transaction is recorded.
    pub fn phase(&self) -> Result<Option<TransactionPhase>, String> {
        Ok(self.store.load()?.map(|journal| journal.phase))
    }

    /// Whether a completed save is held: the only state `engine.shutdown {reason: "update"}`
    /// accepts.
    pub fn prepared(&self) -> Result<bool, String> {
        Ok(self
            .store
            .load()?
            .is_some_and(|journal| journal.phase == TransactionPhase::Prepared))
    }

    /// Saves and holds every running computer, one at a time. Fails without changing anything
    /// that runs when any computer cannot be saved; once pausing began, a failure resumes the
    /// original generations and keeps the journal if any of them cannot resume.
    pub fn prepare(
        &self,
        request: PrepareRequest,
        progress: &dyn Fn(usize, usize, &str),
    ) -> Result<Vec<RunningComputer>, String> {
        if self.store.exists() {
            // Either unresolved computers remain, or the journal cannot be read: both block.
            self.store.load()?;
            return Err(
                "A previous update still has computers to resume. Relaunch Silo before updating again."
                    .into(),
            );
        }
        let mut plan = Vec::new();
        for adapter in self.adapters {
            plan.extend(adapter.running()?);
        }
        for computer in &plan {
            self.adapter(computer.kind)?
                .check_support(computer)
                .map_err(|why| {
                    format!(
                        "{} can't be saved for the update ({why}). Nothing was paused and no computer stopped.",
                        computer.name
                    )
                })?;
        }
        // Which run each computer is, so every later resume or probe requires exactly it.
        let mut originals = Vec::with_capacity(plan.len());
        for computer in &plan {
            originals.push(self.adapter(computer.kind)?.identity(computer).map_err(|why| {
                format!(
                    "{} can't be saved for the update ({why}). Nothing was paused and no computer stopped.",
                    computer.name
                )
            })?);
        }
        let journal = Journal::new(
            request.transaction_id.clone(),
            request.prepare_job_id,
            request.source_engine_version,
            request.host_build,
            plan.iter()
                .zip(originals)
                .map(|(computer, original)| SavedComputer {
                    id: computer.id.clone(),
                    name: computer.name.clone(),
                    kind: computer.kind,
                    phase: ComputerPhase::Planned,
                    original,
                    capture: None,
                    restore_attempt_id: None,
                    active_restore: None,
                    retryable: true,
                    last_error: None,
                })
                .collect(),
        );
        // The complete running set is durable before any computer is paused.
        let session = Session::new(self.store, journal);
        session.save()?;
        let saves_root = self.store.saves_root();
        let context = SaveContext {
            transaction_id: &request.transaction_id,
            saves_root: &saves_root,
        };
        let total = plan.len();
        for (index, computer) in plan.iter().enumerate() {
            progress(index + 1, total, &computer.name);
            let saved = session
                .update(index, |entry| entry.phase = ComputerPhase::Capturing)
                .and_then(|()| {
                    self.adapter(computer.kind)?
                        .save_and_hold(computer, &context)
                })
                .and_then(|capture| {
                    session
                        .update(index, |entry| {
                            entry.capture = Some(capture);
                            entry.phase = ComputerPhase::Saved;
                        })
                        .map_err(|why| {
                            format!(
                                "{} was saved but the update journal failed: {why}",
                                computer.name
                            )
                        })
                });
            if let Err(cause) = saved {
                return Err(self.roll_back(&session, cause));
            }
        }
        session
            .set_phase(TransactionPhase::Prepared)
            .map_err(|cause| self.roll_back(&session, cause))?;
        Ok(plan)
    }

    /// Resumes every original generation after a failed preparation.
    fn roll_back(&self, session: &Session<'_>, cause: String) -> String {
        let mut failures = Vec::new();
        for index in 0..session.len() {
            let entry = session.entry(index);
            let outcome = match entry.phase {
                ComputerPhase::Planned => Ok(()),
                ComputerPhase::Capturing
                | ComputerPhase::Saved
                | ComputerPhase::ResumingOriginal => self
                    .adapter(entry.kind)
                    .map_err(ResumeHeldError::Failed)
                    .and_then(|adapter| {
                        self.resume_original(session, index, adapter, &running_computer(&entry))
                    })
                    .map_err(|error| match error {
                        ResumeHeldError::Gone(why) | ResumeHeldError::Failed(why) => why,
                    }),
                _ => continue,
            };
            match outcome {
                Ok(()) => {
                    let _ = session.update(index, |entry| entry.phase = ComputerPhase::Resumed);
                }
                Err(why) => {
                    failures.push(entry.name.clone());
                    // The entry stays `ResumingOriginal` when the resume was armed: it may have
                    // executed, so it is probed and never restored from its capture.
                    let _ = session.update(index, |entry| entry.last_error = Some(why));
                }
            }
        }
        if failures.is_empty() {
            self.finish(session);
            cause
        } else {
            let _ = session.set_phase(TransactionPhase::Failed);
            format!(
                "{cause}\nComputers could not resume: {}. Relaunch Silo to retry.",
                failures.join(", ")
            )
        }
    }

    /// Resumes the original held generation. The armed transition is durable before the guest
    /// can execute again, including for a resume of the original generation: a crash after it
    /// is probed, never replayed from the capture over the disk the guest advanced.
    fn resume_original(
        &self,
        session: &Session<'_>,
        index: usize,
        adapter: &dyn SaveAdapter,
        computer: &RunningComputer,
    ) -> Result<(), ResumeHeldError> {
        let original = session.entry(index).original;
        session
            .update(index, |entry| entry.phase = ComputerPhase::ResumingOriginal)
            .map_err(|why| {
                ResumeHeldError::Failed(format!(
                    "{} was not resumed because the update journal failed: {why}",
                    computer.name
                ))
            })?;
        adapter.resume_held(computer, &original)
    }

    /// Removes the artifacts of settled captures and clears the journal.
    fn finish(&self, session: &Session<'_>) {
        for index in 0..session.len() {
            let entry = session.entry(index);
            if entry.phase == ComputerPhase::Resumed {
                if let (Some(capture), Ok(adapter)) = (&entry.capture, self.adapter(entry.kind)) {
                    if let Err(why) = adapter.discard(&running_computer(&entry), capture) {
                        eprintln!("Silo kept the saved state of {}: {why}", entry.name);
                    }
                }
            }
        }
        let transaction = session.journal.borrow().transaction_id.clone();
        remove_directory(&self.store.saves_root().join(&transaction));
        if let Err(why) = self.store.clear() {
            eprintln!("Silo could not clear the update journal: {why}");
        }
    }

    /// `engine.update.abort`: resumes the exact saved set, then clears the journal.
    /// Idempotent: with no journal it succeeds.
    pub fn abort(&self) -> Result<(), String> {
        let Some(journal) = self.store.load()? else {
            return Ok(());
        };
        let session = Session::new(self.store, journal);
        let recovery = self.settle(&session, Mode::Abort)?;
        if recovery.settled() {
            Ok(())
        } else {
            Err(unresolved_message(&recovery))
        }
    }

    /// Releases every saved runtime without a guest shutdown, after verifying that the
    /// journal says the save completed.
    pub fn release_for_update(&self) -> Result<(), String> {
        let Some(journal) = self.store.load()? else {
            return Err(NOT_PREPARED.into());
        };
        if journal.phase == TransactionPhase::Released
            && journal
                .computers
                .iter()
                .all(|entry| entry.phase == ComputerPhase::Released)
        {
            // Already released by an earlier call: the installer may repeat the request.
            return Ok(());
        }
        let releasing = matches!(
            journal.phase,
            TransactionPhase::Prepared | TransactionPhase::Releasing
        );
        let complete = journal.computers.iter().all(|entry| {
            matches!(entry.phase, ComputerPhase::Saved | ComputerPhase::Released)
                && entry.capture.is_some()
        });
        if !releasing || !complete {
            return Err(NOT_PREPARED.into());
        }
        let session = Session::new(self.store, journal);
        session.set_phase(TransactionPhase::Releasing)?;
        for index in 0..session.len() {
            let entry = session.entry(index);
            if entry.phase == ComputerPhase::Released {
                continue;
            }
            let capture = entry.capture.clone().expect("checked above");
            self.adapter(entry.kind)?
                .release(&running_computer(&entry), &capture, &entry.original)
                .map_err(|why| format!("{} could not be released: {why}", entry.name))?;
            session.update(index, |entry| entry.phase = ComputerPhase::Released)?;
        }
        session.set_phase(TransactionPhase::Released)
    }

    /// Runs at engine start, before any ordinary lifecycle recovery or start-at-launch:
    /// restores exactly the saved set. `Ok(None)` means there was nothing to recover.
    pub fn recover(&self) -> Result<Option<Recovery>, String> {
        let Some(journal) = self.store.load()? else {
            return Ok(None);
        };
        let session = Session::new(self.store, journal);
        self.settle(&session, Mode::Recover).map(Some)
    }

    fn settle(&self, session: &Session<'_>, mode: Mode) -> Result<Recovery, String> {
        if session.journal.borrow().phase != TransactionPhase::Restoring {
            session.set_phase(TransactionPhase::Restoring)?;
        }
        let mut recovery = Recovery::default();
        for index in 0..session.len() {
            let entry = session.entry(index);
            let computer = running_computer(&entry);
            let adapter = self.adapter(entry.kind)?;
            let outcome = match action(&entry, mode) {
                Action::AlreadyResumed => Settled::Resumed,
                Action::AlreadyLost => Settled::Lost(entry.last_error.clone().unwrap_or_default()),
                Action::Reconcile => self.reconcile(session, index, adapter, &computer),
                Action::Probe => self.probe(session, index, adapter, &computer, &entry),
                Action::ProbeOriginal => {
                    self.settle_original(session, index, adapter, &computer, &entry)
                }
                Action::ResumeHeld => self.abort_held(session, index, adapter, &computer, &entry),
                Action::Restore => self.restore(session, index, adapter, &computer, &entry),
            };
            match outcome {
                Settled::Resumed => recovery.resumed.push(computer),
                Settled::Lost(why) => recovery.lost.push((computer, why)),
                Settled::Failed(why) => recovery.failed.push((computer, why)),
            }
        }
        if recovery.settled() {
            self.finish(session);
        } else {
            let _ = session.set_phase(TransactionPhase::Failed);
        }
        Ok(recovery)
    }

    /// Abort: resume the original generation wherever it still exists (this process holds it,
    /// or a detached runtime survived), falling back to the capture only when that run is
    /// gone. A run that is already executing is left alone.
    fn abort_held(
        &self,
        session: &Session<'_>,
        index: usize,
        adapter: &dyn SaveAdapter,
        computer: &RunningComputer,
        entry: &SavedComputer,
    ) -> Settled {
        match adapter.probe_original(computer, &entry.original) {
            Err(why) => Settled::Failed(why),
            Ok(OriginalState::Running) => {
                let _ = session.update(index, |entry| entry.phase = ComputerPhase::Resumed);
                Settled::Resumed
            }
            Ok(OriginalState::Held) => {
                self.resume_original_entry(session, index, adapter, computer)
            }
            // Nothing of the original remains and (before an armed resume) it never advanced
            // the disk, so the capture is the way back.
            Ok(OriginalState::Gone)
                if entry.capture.is_some() && entry.phase != ComputerPhase::ResumingOriginal =>
            {
                self.restore(session, index, adapter, computer, entry)
            }
            Ok(OriginalState::Gone) => self.original_gone(session, index, computer),
        }
    }

    /// After a crash with an original resume armed (or a failed resume that may have run).
    fn settle_original(
        &self,
        session: &Session<'_>,
        index: usize,
        adapter: &dyn SaveAdapter,
        computer: &RunningComputer,
        entry: &SavedComputer,
    ) -> Settled {
        match adapter.probe_original(computer, &entry.original) {
            Err(why) => Settled::Failed(why),
            Ok(OriginalState::Running) => {
                let _ = session.update(index, |entry| entry.phase = ComputerPhase::Resumed);
                Settled::Resumed
            }
            Ok(OriginalState::Held) => {
                self.resume_original_entry(session, index, adapter, computer)
            }
            Ok(OriginalState::Gone) => self.original_gone(session, index, computer),
        }
    }

    fn resume_original_entry(
        &self,
        session: &Session<'_>,
        index: usize,
        adapter: &dyn SaveAdapter,
        computer: &RunningComputer,
    ) -> Settled {
        match self.resume_original(session, index, adapter, computer) {
            Ok(()) => {
                let _ = session.update(index, |entry| entry.phase = ComputerPhase::Resumed);
                Settled::Resumed
            }
            Err(ResumeHeldError::Gone(_)) => self.original_gone(session, index, computer),
            Err(ResumeHeldError::Failed(why)) => {
                // Armed: the guest may have executed. It is probed again, never restored.
                let _ = session.update(index, |entry| entry.last_error = Some(why.clone()));
                Settled::Failed(why)
            }
        }
    }

    fn original_gone(
        &self,
        session: &Session<'_>,
        index: usize,
        computer: &RunningComputer,
    ) -> Settled {
        // The guest may have run before the run ended. Replaying the capture over the disk it
        // advanced would corrupt it, so the computer is reported instead.
        let why = format!(
            "{} was resuming when Silo stopped and its saved state can no longer be used safely. Its disk was kept; start it again.",
            computer.name
        );
        let _ = session.update(index, |entry| {
            entry.phase = ComputerPhase::Lost;
            entry.last_error = Some(why.clone());
        });
        Settled::Lost(why)
    }

    fn reconcile(
        &self,
        session: &Session<'_>,
        index: usize,
        adapter: &dyn SaveAdapter,
        computer: &RunningComputer,
    ) -> Settled {
        match adapter.reconcile_unsaved(computer, &session.entry(index).original) {
            Ok(Reconciled::Running) => {
                let _ = session.update(index, |entry| entry.phase = ComputerPhase::Resumed);
                Settled::Resumed
            }
            Ok(Reconciled::Lost(why)) => {
                let _ = session.update(index, |entry| {
                    entry.phase = ComputerPhase::Lost;
                    entry.last_error = Some(why.clone());
                });
                Settled::Lost(why)
            }
            Err(why) => Settled::Failed(why),
        }
    }

    fn probe(
        &self,
        session: &Session<'_>,
        index: usize,
        adapter: &dyn SaveAdapter,
        computer: &RunningComputer,
        entry: &SavedComputer,
    ) -> Settled {
        let (Some(capture), Some(attempt)) = (&entry.capture, &entry.restore_attempt_id) else {
            let why = "The journal lost the restore attempt; the computer's state is unknown.";
            let _ = session.update(index, |entry| {
                entry.phase = ComputerPhase::Lost;
                entry.last_error = Some(why.into());
            });
            return Settled::Lost(why.into());
        };
        match adapter.probe_armed(
            computer,
            capture,
            &entry.original,
            attempt,
            entry.active_restore.as_ref(),
        ) {
            Ok(true) => {
                let _ = session.update(index, |entry| entry.phase = ComputerPhase::Resumed);
                Settled::Resumed
            }
            Ok(false) => {
                // The guest may have run before the owner died. Replaying the capture over the
                // disk it advanced would corrupt it, so the computer is reported instead.
                let why = format!(
                    "{} was resuming when Silo stopped and its saved state can no longer be used safely. Its disk was kept; start it again.",
                    computer.name
                );
                let _ = session.update(index, |entry| {
                    entry.phase = ComputerPhase::Lost;
                    entry.last_error = Some(why.clone());
                });
                Settled::Lost(why)
            }
            // A probe that cannot answer is retried; the entry is left exactly as journaled.
            Err(why) => Settled::Failed(why),
        }
    }

    fn restore(
        &self,
        session: &Session<'_>,
        index: usize,
        adapter: &dyn SaveAdapter,
        computer: &RunningComputer,
        entry: &SavedComputer,
    ) -> Settled {
        let Some(capture) = entry.capture.clone() else {
            return self.reconcile(session, index, adapter, computer);
        };
        let attempt = uuid::Uuid::new_v4().to_string();
        if let Err(why) = session.update(index, |entry| {
            entry.phase = ComputerPhase::Restoring;
            entry.restore_attempt_id = Some(attempt.clone());
            entry.active_restore = None;
            entry.retryable = true;
            entry.last_error = None;
        }) {
            return Settled::Failed(why);
        }
        let armed = Cell::new(false);
        let result = adapter.restore(
            computer,
            &capture,
            &entry.original,
            &attempt,
            &mut |identity| {
                session.update(index, |entry| {
                    entry.phase = ComputerPhase::ResumeArmed;
                    entry.active_restore = identity.cloned();
                })?;
                armed.set(true);
                Ok(())
            },
        );
        match result {
            Ok(()) => {
                if let Err(why) =
                    session.update(index, |entry| entry.phase = ComputerPhase::Resumed)
                {
                    // The guest runs; the armed entry is probed and settled by the next owner.
                    eprintln!(
                        "Silo could not record that {} resumed: {why}",
                        computer.name
                    );
                }
                Settled::Resumed
            }
            Err(RestoreError::Superseded(why)) => {
                // A run the update did not start owns the computer: its disk is its own and
                // the capture is neither replayed nor settled on its behalf.
                let _ = session.update(index, |entry| {
                    entry.phase = ComputerPhase::Lost;
                    entry.last_error = Some(why.clone());
                });
                Settled::Lost(why)
            }
            Err(RestoreError::Failed(why)) if !armed.get() => {
                let _ = session.update(index, |entry| {
                    entry.phase = ComputerPhase::Failed;
                    entry.last_error = Some(why.clone());
                });
                Settled::Failed(why)
            }
            Err(RestoreError::Failed(why)) => {
                let why =
                    format!("{why} The restored computer may be running; its state is uncertain.");
                let _ = session.update(index, |entry| {
                    entry.phase = ComputerPhase::Failed;
                    entry.retryable = false;
                    entry.last_error = Some(why.clone());
                });
                Settled::Failed(why)
            }
        }
    }
}

const NOT_PREPARED: &str =
    "The update was not prepared: no completed save is held. Prepare the update before shutting down for it.";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Mode {
    /// A new owner after a crash or an update.
    Recover,
    /// `engine.update.abort` in whichever process is running now.
    Abort,
}

/// What settling does with one journaled computer.
enum Action {
    AlreadyResumed,
    AlreadyLost,
    /// The save never completed: leave the original running or report it lost.
    Reconcile,
    /// A restore was armed or its outcome is unknown: find out whether the guest runs.
    Probe,
    /// An original resume was armed: find out whether that exact run executes.
    ProbeOriginal,
    /// The same process still owns the held generation: resume it.
    ResumeHeld,
    /// Bring the computer back from its capture.
    Restore,
}

fn action(entry: &SavedComputer, mode: Mode) -> Action {
    use ComputerPhase as P;
    match (entry.phase, mode) {
        (P::Resumed, _) => Action::AlreadyResumed,
        (P::Lost, _) => Action::AlreadyLost,
        (P::ResumeArmed, _) => Action::Probe,
        (P::ResumingOriginal, _) => Action::ProbeOriginal,
        (P::Failed, _) if !entry.retryable => Action::Probe,
        // An abort resumes the original wherever it still lives; a release that already
        // happened does not make the machine this process still owns any less resumable.
        (P::Planned | P::Capturing | P::Saved | P::Released, Mode::Abort) => Action::ResumeHeld,
        (P::Failed, Mode::Abort) if entry.restore_attempt_id.is_none() => Action::ResumeHeld,
        (P::Planned | P::Capturing, Mode::Recover) => Action::Reconcile,
        (P::Failed, Mode::Recover) if entry.capture.is_none() => Action::Reconcile,
        (P::Saved | P::Released | P::Restoring | P::Failed, _) => Action::Restore,
    }
}

enum Settled {
    Resumed,
    Lost(String),
    Failed(String),
}

fn unresolved_message(recovery: &Recovery) -> String {
    let names: Vec<_> = recovery
        .failed
        .iter()
        .map(|(computer, why)| format!("{}: {why}", computer.name))
        .collect();
    format!(
        "Computers could not resume: {}. Relaunch Silo to retry.",
        names.join("; ")
    )
}

fn remove_directory(path: &Path) {
    match std::fs::remove_dir_all(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => eprintln!("Silo kept {}: {error}", path.display()),
    }
}

/// `CaptureRef` helpers used by adapters and tests.
impl CaptureRef {
    pub fn describe(&self) -> String {
        match self {
            Self::Macos(capture) => capture.directory.display().to_string(),
            Self::Linux(capture) => format!("{}:{}", capture.group, capture.member),
        }
    }
}
