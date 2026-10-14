//! Engine shutdown and update handoff (05 §6, design/p2 §1.7 and §3).
//!
//! `engine.update.prepare` saves and holds every running computer behind a crash-safe journal
//! ([`journal`], [`transaction`]); `engine.shutdown {reason: "update"}` releases them without a
//! guest shutdown; the next engine start restores exactly that set; `engine.update.abort`
//! resumes it in place. [`UpdateState`] is the per-engine lifecycle state those methods share.
pub mod adapter;
pub mod guards;
pub mod hold;
pub mod journal;
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

pub use adapter::{Reconciled, ResumeHeldError, RunningComputer, SaveAdapter, SaveContext};
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
    }

    /// Forgets a preparation that failed. Its hold ends, which releases admission.
    pub fn prepare_failed(&self) {
        let hold = {
            let mut inner = self.lock();
            inner.prepare = PrepareState::Idle;
            inner.hold.take()
        };
        drop(hold);
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
        let (hold, preparing, shutting_down) = {
            let inner = self.lock();
            (
                inner.hold.clone(),
                matches!(inner.prepare, PrepareState::Preparing(_)),
                inner.shutdown.is_some(),
            )
        };
        if preparing && hold.is_none() {
            return Err("The update is still being prepared. Wait for it to finish.".into());
        }
        if shutting_down && hold.is_some() {
            return Err(
                "Silo is shutting down for the update. It can no longer be aborted.".into(),
            );
        }
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
    /// so from the first request. `recover` settles it.
    pub fn mark_recovery_pending(&self, context: &dyn EngineContext) {
        if store(context).exists() {
            self.lock().recovery_pending = true;
        }
    }

    /// Runs at engine start, before ordinary lifecycle recovery. Blocking.
    pub fn recover(&self, context: &dyn EngineContext) -> Result<Option<Recovery>, String> {
        let store = store(context);
        if !store.exists() {
            return Ok(None);
        }
        self.lock().recovery_pending = true;
        let result = Hold::start(self.platform()?, store, None).and_then(|hold| {
            let result = hold.recover();
            hold.end();
            result
        });
        let mut inner = self.lock();
        match &result {
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
        result
    }

    // MARK: Shutdown

    /// Claims the engine for a shutdown, or attaches to an equivalent one under way.
    pub fn begin_shutdown(
        &self,
        request: ShutdownRequest,
    ) -> Result<ShutdownStart, LifecycleError> {
        let platform = self.platform().ok();
        let mut inner = self.lock();
        let prepared = matches!(inner.prepare, PrepareState::Prepared(_));
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
    }

    /// Performs an accepted shutdown. Blocking. The caller records the job outcome and exits
    /// the process after `Ok`; after `Err` it reopens service unless the reason is a session
    /// end, which never cancels.
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
        let deadline = (request.reason == ShutdownReason::SessionEnd)
            .then(|| Instant::now() + request.deadline.unwrap_or(SESSION_END_BUDGET));
        self.set_phase(ShutdownPhase::Approved);
        if mode != ShutdownMode::Update {
            // Refuse new work everywhere; an update shutdown already holds admission closed.
            platform.begin_shutdown();
        }
        self.set_phase(ShutdownPhase::Finishing);
        let finished = match mode {
            ShutdownMode::Update => match self.hold() {
                Some(hold) => hold.release(),
                None => Err(NOT_PREPARED.to_owned()),
            },
            ShutdownMode::Stop => platform.stop_computers(deadline),
            ShutdownMode::Detach => Ok(()),
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
