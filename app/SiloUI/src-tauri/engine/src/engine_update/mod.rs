//! Engine shutdown and update handoff (05 §6, design/p2 §1.7 and §3).
//!
//! `engine.update.prepare` saves and holds every running computer behind a crash-safe journal
//! ([`journal`], [`transaction`]); `engine.shutdown {reason: "update"}` releases them without a
//! guest shutdown; the next engine start restores exactly that set; `engine.update.abort`
//! resumes it in place. [`UpdateState`] is the per-engine lifecycle state those methods share.
pub mod adapter;
pub mod guards;
pub mod hold;
pub mod installer;
pub mod journal;
pub mod reservation;
pub mod transaction;
pub mod updater;

use std::{
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant},
};

use silo_api::{
    jobs::JobId,
    methods::engine::{EngineState, ShutdownPhase, ShutdownReason},
};

use crate::context::EngineContext;

pub use adapter::{
    OriginalState, Reconciled, RestoreError, ResumeHeldError, RunningComputer, SaveAdapter,
    SaveContext,
};
pub use hold::{HeldGuards, Hold, Platform};
pub use journal::{ComputerKind, Store};

pub use transaction::{Coordinator, PrepareRequest, Recovery};

/// The default bound for a session-end shutdown (`SESSION_END_BUDGET`, 05 §6).
pub const SESSION_END_BUDGET: Duration = crate::system_shutdown::SESSION_END_BUDGET;

const NOT_PREPARED: &str =
    "The update was not prepared: no completed save is held. Prepare the update before shutting down for it.";

/// The directory that holds the update journal and its captures: the engine's private data
/// directory, beside the job registry.
pub fn engine_directory(context: &dyn EngineContext) -> PathBuf {
    let paths = context.paths();
    if paths.home.as_os_str().is_empty() {
        paths.app_data.join("engine")
    } else {
        paths
            .home
            .join(context.identity().channel.state_dir_name())
            .join("engine")
    }
}

pub fn store(context: &dyn EngineContext) -> Store {
    Store::new(engine_directory(context))
}

/// Why a lifecycle request was refused.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LifecycleError {
    /// Another shutdown, preparation or recovery owns the engine.
    Busy(String),
    Refused(String),
}

impl LifecycleError {
    pub fn message(&self) -> &str {
        match self {
            Self::Busy(message) | Self::Refused(message) => message,
        }
    }
}

/// What one watchdog probe did.
#[derive(Debug)]
pub enum WatchdogOutcome {
    /// Nothing is held, or a shutdown or abort owns the transaction.
    Idle,
    /// The coordinator is present, or has not been absent long enough.
    Waiting,
    Aborted(Result<(), String>),
    Recovered(Result<Option<Recovery>, String>),
}

/// The options of one `engine.shutdown` request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShutdownRequest {
    pub stop_computers: bool,
    pub reason: ShutdownReason,
    pub deadline: Option<Duration>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ShutdownMode {
    /// Release the saved set without a guest shutdown.
    Update,
    /// Stop this device's computers.
    Stop,
    /// Exit while Linux runtimes stay detached; no macOS machine may be owned.
    Detach,
}

#[derive(Clone, Debug)]
enum PrepareState {
    Idle,
    /// Accepted; the job id is attached once the handler allocated it.
    Preparing(Option<JobId>),
    Prepared(JobId),
    /// An earlier owner's prepared update is still under the installer's control. This
    /// (old-version) engine serves with admission closed and resumes nothing behind it.
    Deferred,
}

struct ActiveShutdown {
    request: ShutdownRequest,
    mode: ShutdownMode,
    job: Option<JobId>,
}

struct Inner {
    shutdown: Option<ActiveShutdown>,
    phase: ShutdownPhase,
    generation: u64,
    session_ending: bool,
    prepare: PrepareState,
    hold: Option<Arc<Hold>>,
    /// A journal from an earlier owner still has computers to resume.
    recovery_pending: bool,
    recovery_error: Option<String>,
    /// An abort holds the transaction; a shutdown cannot claim it meanwhile (and vice versa).
    aborting: bool,
    /// The earliest absolute deadline of a session-end shutdown.
    deadline: Option<Instant>,
    watchdog: installer::Watchdog,
}

/// What a prepare request does.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PrepareStart {
    /// A new transaction: the caller allocates the job, then calls `prepare_job`.
    New,
    /// The same transaction is already preparing or prepared.
    Attach(JobId),
}

/// What a shutdown request does.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShutdownStart {
    New,
    Attach(JobId),
}

/// Lifecycle state of one engine instance. It lives in [`crate::EngineRuntimeState`].
pub struct UpdateState {
    inner: Mutex<Inner>,
    platform: Mutex<Option<Arc<dyn Platform>>>,
    updater: Mutex<Option<Arc<updater::Controller>>>,
}

impl Default for UpdateState {
    fn default() -> Self {
        Self {
            inner: Mutex::new(Inner {
                shutdown: None,
                phase: ShutdownPhase::Idle,
                generation: 0,
                session_ending: false,
                prepare: PrepareState::Idle,
                hold: None,
                recovery_pending: false,
                recovery_error: None,
                aborting: false,
                deadline: None,
                watchdog: installer::Watchdog::default(),
            }),
            platform: Mutex::new(None),
            updater: Mutex::new(None),
        }
    }
}

impl UpdateState {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        crate::sync::lock_or_recover(&self.inner, "engine lifecycle")
    }

    pub fn set_platform(&self, platform: Arc<dyn Platform>) {
        *crate::sync::lock_or_recover(&self.platform, "engine platform") = Some(platform);
    }

    pub fn platform(&self) -> Result<Arc<dyn Platform>, String> {
        crate::sync::lock_or_recover(&self.platform, "engine platform")
            .clone()
            .ok_or_else(|| "The engine's computer host is unavailable.".to_owned())
    }

    /// The engine's update schedule and package state, created on first use.
    pub fn updater(&self, context: &dyn EngineContext) -> Result<Arc<updater::Controller>, String> {
        let mut updater = crate::sync::lock_or_recover(&self.updater, "engine updater");
        if let Some(updater) = updater.as_ref() {
            return Ok(updater.clone());
        }
        let controller = Arc::new(updater::Controller::new(
            context.paths().app_data.join("update-preferences.json"),
            context.identity().version.clone(),
            context.identity().channel.is_development(),
            self.platform()?.update_provider(),
        ));
        *updater = Some(controller.clone());
        Ok(controller)
    }

    /// The `engine.state` projection.
    pub fn engine_state(&self) -> EngineState {
        let inner = self.lock();
        EngineState {
            shutting_down: inner.shutdown.is_some(),
            phase: inner.phase,
            generation: inner.generation,
            session_ending: inner.session_ending,
            update_pending: inner.update_pending(),
        }
    }

    pub fn update_pending(&self) -> bool {
        self.lock().update_pending()
    }

    pub fn recovery_error(&self) -> Option<String> {
        self.lock().recovery_error.clone()
    }

    pub fn shutting_down(&self) -> bool {
        self.lock().shutdown.is_some()
    }

    // MARK: Preparation

    /// Claims the engine for a new preparation, or attaches to the one under way.
    pub fn begin_prepare(&self) -> Result<PrepareStart, LifecycleError> {
        let mut inner = self.lock();
        if inner.shutdown.is_some() {
            return Err(LifecycleError::Busy(
                "Silo is shutting down. Wait for shutdown to finish.".into(),
            ));
        }
        match &inner.prepare {
            PrepareState::Prepared(job) | PrepareState::Preparing(Some(job)) => {
                return Ok(PrepareStart::Attach(job.clone()));
            }
            PrepareState::Preparing(None) => {
                return Err(LifecycleError::Busy(
                    "An update is already being prepared.".into(),
                ));
            }
            PrepareState::Deferred => {
                return Err(LifecycleError::Busy(
                    "An earlier update is still being handed off by the installer.".into(),
                ));
            }
            PrepareState::Idle => {}
        }
        if inner.recovery_pending {
            return Err(LifecycleError::Refused(
                "A previous update still has computers to resume. Relaunch Silo before updating again."
                    .into(),
            ));
        }
        inner.prepare = PrepareState::Preparing(None);
        Ok(PrepareStart::New)
    }

    /// Attaches the allocated job to the preparation `begin_prepare` accepted.
    pub fn prepare_job(&self, job: JobId) {
        let mut inner = self.lock();
        if matches!(inner.prepare, PrepareState::Preparing(None)) {
            inner.prepare = PrepareState::Preparing(Some(job));
        }
    }

    /// Records a completed preparation and keeps its hold until abort or release.
    pub fn prepared(&self, job: JobId, hold: Hold) {
        let mut inner = self.lock();
        inner.prepare = PrepareState::Prepared(job);
        inner.hold = Some(Arc::new(hold));
        inner.watchdog.reset();
    }

    /// Forgets a preparation that failed. Its hold ends, which releases admission. When the
    /// rollback left computers unresolved their journal remains: the update stays pending and
    /// their computers stay reserved until `abort` or the next start settles them.
    pub fn prepare_failed(&self, context: &dyn EngineContext) {
        let hold = {
            let mut inner = self.lock();
            inner.prepare = PrepareState::Idle;
            inner.hold.take()
        };
        drop(hold);
        let store = store(context);
        reservation::sync(&store);
        if store.exists() {
            let mut inner = self.lock();
            inner.recovery_pending = true;
            inner.recovery_error = Some(
                "Some computers could not resume after the update failed to prepare. Relaunch Silo to retry."
                    .into(),
            );
        }
    }

    /// The hold of a prepared update.
    pub fn hold(&self) -> Option<Arc<Hold>> {
        self.lock().hold.clone()
    }

    pub fn is_prepared(&self) -> bool {
        matches!(self.lock().prepare, PrepareState::Prepared(_))
    }

    pub fn is_preparing(&self) -> bool {
        matches!(self.lock().prepare, PrepareState::Preparing(_))
    }

    /// `engine.update.abort`: resumes the saved set. With no preparation in this process but a
    /// journal from an earlier owner, a temporary hold resumes that set.
    pub fn abort(&self, context: &dyn EngineContext) -> Result<(), String> {
        let hold = {
            let mut inner = self.lock();
            let hold = inner.hold.clone();
            let preparing = matches!(inner.prepare, PrepareState::Preparing(_));
            if preparing && hold.is_none() {
                return Err("The update is still being prepared. Wait for it to finish.".into());
            }
            if inner.shutdown.is_some() && hold.is_some() {
                return Err(
                    "Silo is shutting down for the update. It can no longer be aborted.".into(),
                );
            }
            if inner.aborting {
                return Err("The update is already being aborted.".into());
            }
            // Claimed under the same lock a shutdown takes its own claim under.
            inner.aborting = true;
            hold
        };
        let result = self.abort_claimed(context, hold);
        self.lock().aborting = false;
        reservation::sync(&store(context));
        result
    }

    fn abort_claimed(
        &self,
        context: &dyn EngineContext,
        hold: Option<Arc<Hold>>,
    ) -> Result<(), String> {
        match hold {
            Some(hold) => {
                hold.abort()?;
                let mut inner = self.lock();
                inner.prepare = PrepareState::Idle;
                inner.hold = None;
                inner.recovery_pending = false;
                inner.recovery_error = None;
                Ok(())
            }
            None => {
                let store = store(context);
                if !store.exists() {
                    return Ok(());
                }
                let temporary = Hold::start(self.platform()?, store, None)?;
                let result = temporary.abort();
                temporary.end();
                if result.is_ok() {
                    let mut inner = self.lock();
                    inner.recovery_pending = false;
                    inner.recovery_error = None;
                }
                result
            }
        }
    }

    // MARK: Recovery

    /// Marks the update pending when an earlier owner left a journal, so `engine.state` says
    /// so from the first request, and reserves the computers it names. `recover` settles it.
    pub fn mark_recovery_pending(&self, context: &dyn EngineContext) {
        let store = store(context);
        if store.exists() {
            self.lock().recovery_pending = true;
            reservation::sync(&store);
        }
    }

    /// Runs at engine start, before ordinary lifecycle recovery. Blocking.
    ///
    /// When the installer still owns a prepared transaction that this (old-version) engine
    /// would otherwise resume behind its back, nothing is resumed: the engine keeps admission
    /// closed and waits for the coordinator to finish or disappear (`watchdog_tick`).
    pub fn recover(&self, context: &dyn EngineContext) -> Result<Option<Recovery>, String> {
        let store = store(context);
        if !store.exists() {
            return Ok(None);
        }
        self.lock().recovery_pending = true;
        reservation::sync(&store);
        let result = match self.defer_to_installer(context, &store) {
            Ok(true) => return Ok(None),
            Ok(false) => Hold::start(self.platform()?, store.clone(), None).and_then(|hold| {
                let result = hold.recover();
                hold.end();
                result
            }),
            Err(why) => Err(why),
        };
        self.apply_recovery(&result);
        reservation::sync(&store);
        result
    }

    fn apply_recovery(&self, result: &Result<Option<Recovery>, String>) {
        let mut inner = self.lock();
        match result {
            Ok(Some(recovery)) if recovery.settled() => {
                inner.recovery_pending = false;
                inner.recovery_error = None;
            }
            Ok(Some(recovery)) => {
                inner.recovery_error = Some(format!(
                    "Some computers could not resume after updating: {}. Relaunch Silo to retry.",
                    recovery
                        .failed
                        .iter()
                        .map(|(computer, why)| format!("{}: {why}", computer.name))
                        .collect::<Vec<_>>()
                        .join("; ")
                ));
            }
            Ok(None) => inner.recovery_pending = false,
            Err(why) => inner.recovery_error = Some(why.clone()),
        }
    }

    /// Whether an old engine must leave the saved set to the installer that holds it. Takes
    /// the admission hold so ordinary work stays out while the engine waits.
    fn defer_to_installer(
        &self,
        context: &dyn EngineContext,
        store: &Store,
    ) -> Result<bool, String> {
        let Some(journal) = store.load()? else {
            return Ok(false);
        };
        let installer_owns = matches!(
            journal.phase,
            journal::TransactionPhase::Prepared
                | journal::TransactionPhase::Releasing
                | journal::TransactionPhase::Released
        ) && journal.source_engine_version == context.identity().version
            && installer::coordinator_present(store.directory());
        if !installer_owns {
            return Ok(false);
        }
        let hold = Hold::start(self.platform()?, store.clone(), None)?;
        let mut inner = self.lock();
        inner.prepare = PrepareState::Deferred;
        inner.hold = Some(Arc::new(hold));
        inner.watchdog.reset();
        Ok(true)
    }

    /// Whether an update is held and waiting on the installer (prepared or deferred).
    pub fn watching_installer(&self) -> bool {
        matches!(
            self.lock().prepare,
            PrepareState::Prepared(_) | PrepareState::Deferred
        )
    }

    /// One probe of the installer lock at `now`. After [`installer::ABANDONED_AFTER`] of
    /// continuously finding no coordinator, a prepared update is aborted and a deferred one
    /// is recovered, serialized against shutdown through the same claim `abort` takes.
    pub fn watchdog_tick(
        &self,
        context: &dyn EngineContext,
        coordinator_present: bool,
        now: Instant,
    ) -> WatchdogOutcome {
        let deferred = {
            let mut inner = self.lock();
            let deferred = match inner.prepare {
                PrepareState::Prepared(_) => false,
                PrepareState::Deferred => true,
                _ => {
                    inner.watchdog.reset();
                    return WatchdogOutcome::Idle;
                }
            };
            // A shutdown for the update owns the transaction from its claim on.
            if inner.shutdown.is_some() || inner.aborting {
                inner.watchdog.reset();
                return WatchdogOutcome::Idle;
            }
            if !inner.watchdog.observe(coordinator_present, now) {
                return WatchdogOutcome::Waiting;
            }
            deferred
        };
        if !deferred {
            return WatchdogOutcome::Aborted(self.abort(context));
        }
        // A deferred set is restored by the engine that waited for it.
        let hold = {
            let mut inner = self.lock();
            if inner.shutdown.is_some() || inner.aborting {
                return WatchdogOutcome::Idle;
            }
            inner.aborting = true;
            inner.hold.clone()
        };
        let result = match hold {
            Some(hold) => hold.recover(),
            None => Ok(None),
        };
        {
            let mut inner = self.lock();
            inner.aborting = false;
            // Settled or not, admission reopens; an unresolved set keeps its journal and the
            // reservation of its computers.
            inner.prepare = PrepareState::Idle;
            inner.hold = None;
        }
        self.apply_recovery(&result);
        reservation::sync(&store(context));
        WatchdogOutcome::Recovered(result)
    }

    // MARK: Shutdown

    /// Claims the engine for a shutdown, or attaches to an equivalent one under way.
    pub fn begin_shutdown(
        &self,
        request: ShutdownRequest,
    ) -> Result<ShutdownStart, LifecycleError> {
        let platform = self.platform().ok();
        let mut inner = self.lock();
        if inner.aborting && request.reason != ShutdownReason::SessionEnd {
            return Err(LifecycleError::Busy(
                "The update is being aborted. Wait for it to finish.".into(),
            ));
        }
        let prepared = matches!(
            inner.prepare,
            PrepareState::Prepared(_) | PrepareState::Deferred
        );
        // The earliest absolute deadline wins, whoever asked for it and whenever.
        let deadline = (request.reason == ShutdownReason::SessionEnd)
            .then(|| Instant::now() + request.deadline.unwrap_or(SESSION_END_BUDGET));
        let mode = match (request.reason, request.stop_computers) {
            (ShutdownReason::Update, true) => {
                return Err(LifecycleError::Refused(
                    "A shutdown for an update never stops computers: they are saved and resumed."
                        .into(),
                ));
            }
            (ShutdownReason::Update, false) => ShutdownMode::Update,
            // Termination while the update is prepared preserves the saved set.
            (ShutdownReason::SessionEnd, _) if prepared => ShutdownMode::Update,
            (_, true) => ShutdownMode::Stop,
            (_, false) => ShutdownMode::Detach,
        };
        if inner.shutdown.is_some() {
            // The session ending outranks whatever shutdown is running: from now on a
            // failure no longer cancels it.
            if request.reason == ShutdownReason::SessionEnd {
                inner.session_ending = true;
                inner.deadline = match (inner.deadline, deadline) {
                    (Some(current), Some(new)) => Some(current.min(new)),
                    (current, new) => current.or(new),
                };
            }
            let active = inner.shutdown.as_ref().expect("checked above");
            if request.reason == ShutdownReason::SessionEnd
                || (active.mode == mode && active.request.reason == request.reason)
            {
                return match &active.job {
                    Some(job) => Ok(ShutdownStart::Attach(job.clone())),
                    None => Err(LifecycleError::Busy(
                        "Silo is starting to shut down. Try again.".into(),
                    )),
                };
            }
            return Err(LifecycleError::Busy(
                "Silo is already shutting down with different options.".into(),
            ));
        }
        match mode {
            ShutdownMode::Update => {
                if !prepared {
                    return Err(if matches!(inner.prepare, PrepareState::Preparing(_)) {
                        LifecycleError::Busy(
                            "The update is still being prepared. Wait for it to finish.".into(),
                        )
                    } else {
                        LifecycleError::Refused(NOT_PREPARED.into())
                    });
                }
            }
            ShutdownMode::Stop | ShutdownMode::Detach => {
                if prepared || matches!(inner.prepare, PrepareState::Preparing(_)) {
                    return Err(LifecycleError::Busy(
                        "An update is prepared. Abort it, or shut down for the update.".into(),
                    ));
                }
                if mode == ShutdownMode::Detach {
                    let owned = platform.as_ref().is_none_or(|platform| {
                        platform
                            .adapters()
                            .iter()
                            .any(|adapter| adapter.owns_live_machines())
                    });
                    // A session end exits regardless; the machines die with the process.
                    if owned && request.reason != ShutdownReason::SessionEnd {
                        return Err(LifecycleError::Busy(
                            "A macOS computer is still running in this engine, and exiting would end it. Stop it, or ask this shutdown to stop computers."
                                .into(),
                        ));
                    }
                }
            }
        }
        inner.generation += 1;
        inner.phase = ShutdownPhase::Requested;
        inner.session_ending = request.reason == ShutdownReason::SessionEnd;
        inner.deadline = deadline;
        inner.shutdown = Some(ActiveShutdown {
            request,
            mode,
            job: None,
        });
        Ok(ShutdownStart::New)
    }

    pub fn shutdown_job(&self, job: JobId) {
        if let Some(active) = &mut self.lock().shutdown {
            active.job = Some(job);
        }
    }

    pub fn set_phase(&self, phase: ShutdownPhase) {
        let mut inner = self.lock();
        if inner.shutdown.is_some() {
            inner.phase = phase;
        }
    }

    /// A shutdown that was refused to proceed returns the engine to normal service.
    pub fn shutdown_cancelled(&self) {
        let mut inner = self.lock();
        inner.shutdown = None;
        inner.phase = ShutdownPhase::Idle;
        inner.session_ending = false;
        inner.deadline = None;
    }

    /// The absolute deadline of a session-end shutdown, if one is under way.
    pub fn deadline(&self) -> Option<Instant> {
        self.lock().deadline
    }

    /// Performs an accepted shutdown. Blocking. The caller records the job outcome and exits
    /// the process after `Ok`; after `Err` it reopens service unless the reason is a session
    /// end, which never cancels.
    ///
    /// A session end is bounded as a whole: the work (gate waits, computer stops, releases)
    /// runs on a worker, and once the earliest absolute deadline any session-end request set
    /// has passed the shutdown stops waiting for it and goes on to flush and exit.
    pub fn run_shutdown(&self) -> Result<(), String> {
        let (request, mode) = {
            let inner = self.lock();
            let active = inner
                .shutdown
                .as_ref()
                .ok_or_else(|| "No shutdown is under way.".to_owned())?;
            (active.request.clone(), active.mode)
        };
        let platform = self.platform()?;
        self.set_phase(ShutdownPhase::Approved);
        if mode != ShutdownMode::Update {
            // Refuse new work everywhere; an update shutdown already holds admission closed.
            platform.begin_shutdown();
        }
        self.set_phase(ShutdownPhase::Finishing);
        let work = {
            let platform = platform.clone();
            let hold = self.hold();
            let deadline = (request.reason == ShutdownReason::SessionEnd)
                .then(|| self.deadline())
                .flatten();
            move || match mode {
                ShutdownMode::Update => match hold {
                    Some(hold) => hold.release(),
                    None => Err(NOT_PREPARED.to_owned()),
                },
                ShutdownMode::Stop => platform.stop_computers(deadline),
                ShutdownMode::Detach => Ok(()),
            }
        };
        let finished = if request.reason == ShutdownReason::SessionEnd {
            self.run_before_deadline(work)
        } else {
            work()
        };
        // A session end never cancels on failure: it flushes and exits regardless.
        let session_end = self.lock().session_ending;
        if let Err(error) = &finished {
            if !session_end {
                return Err(error.clone());
            }
            eprintln!("Silo is exiting although shutdown failed: {error}");
        }
        self.set_phase(ShutdownPhase::Flushing);
        let flushed = platform.flush_settings();
        platform.close_connections();
        match flushed {
            Err(error) if !session_end => Err(format!(
                "Local computers stopped, but Silo could not save its settings: {error}"
            )),
            Err(error) => {
                eprintln!("Silo could not save its settings at exit: {error}");
                Ok(())
            }
            Ok(()) => Ok(()),
        }
    }

    /// Runs `work` on a worker and waits for it only until the shutdown's deadline, which is
    /// re-read as it waits so a later, earlier deadline applies to work already under way.
    fn run_before_deadline(
        &self,
        work: impl FnOnce() -> Result<(), String> + Send + 'static,
    ) -> Result<(), String> {
        const SLICE: Duration = Duration::from_millis(50);
        let (sender, receiver) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("silo-shutdown-work".into())
            .spawn(move || {
                let _ = sender.send(work());
            });
        if let Err(error) = spawned {
            return Err(format!("Shutdown could not start: {error}"));
        }
        loop {
            let wait = self.deadline().map_or(SLICE, |deadline| {
                deadline
                    .saturating_duration_since(Instant::now())
                    .min(SLICE)
            });
            match receiver.recv_timeout(wait) {
                Ok(outcome) => return outcome,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("Shutdown stopped unexpectedly.".into())
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if self
                        .deadline()
                        .is_some_and(|deadline| Instant::now() >= deadline)
                    {
                        return Err("Shutdown did not finish before its deadline.".into());
                    }
                }
            }
        }
    }

    /// Reopens service after a shutdown failed before anything was released.
    pub fn reopen_after_failed_shutdown(&self) -> Result<(), String> {
        let mode = self.lock().shutdown.as_ref().map(|active| active.mode);
        if mode != Some(ShutdownMode::Update) {
            self.platform()?.reopen_admission();
        }
        self.shutdown_cancelled();
        Ok(())
    }
}

impl Inner {
    fn update_pending(&self) -> bool {
        !matches!(self.prepare, PrepareState::Idle) || self.recovery_pending
    }
}

#[cfg(test)]
mod tests;
