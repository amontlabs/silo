//! Settings, engine updates and the engine's own lifecycle: `settings.*`, `updates.*` and
//! `engine.*` except `engine.handshake` and `engine.state` (05 §6, 60 §2 and §5).
//!
//! * `engine.disconnect` detaches the calling client only (95 A-2).
//! * `engine.update.prepare` saves and holds every running computer behind a crash-safe
//!   journal; `engine.shutdown {reason: "update"}` releases them without a guest shutdown; the
//!   next engine start restores exactly that set; `engine.update.abort` resumes it in place
//!   (95 A-6).
//! * `engine.shutdown` is a durable job for the other shutdowns too.
//! * `updates.*` drive the engine's own signed-package schedule, separate from Fjord's.
#[path = "services/platform.rs"]
mod platform;

use std::{sync::Arc, time::Duration};

use serde_json::Value;
use silo_api::{
    events::ShutdownChangedPayload,
    jobs::{ErrorCode, JobId, JobProgress},
    methods::{
        common::JobStartResult,
        engine::{
            EngineDisconnectParams, EngineShutdownParams, EngineUpdateAbortParams,
            EngineUpdatePrepareFinishedResult, EngineUpdatePrepareParams, ResumingComputer,
        },
        settings::{SettingsSnapshot, SettingsUpdateParams},
        updates::{UpdateSnapshot, UpdatesSetAutomaticChecksParams},
        MethodName,
    },
};
use silo_engine::{
    context::{Audience, EngineContext, Event},
    engine_update::{
        self, Hold, LifecycleError, PrepareRequest, PrepareStart, ShutdownRequest, ShutdownStart,
        UpdateState,
    },
};

use crate::{
    context::SilodContext,
    dispatcher::{method_name, DispatchError, HandlerOutput},
};

use super::{ownership::owns_prefix, HandlerContext};
use platform::{retain_resume_failure, ServicesPlatform, EXIT_DRAIN};

pub(super) fn owns(method: MethodName) -> bool {
    owns_prefix(method, &["settings.", "updates."])
        || (owns_prefix(method, &["engine."])
            && !matches!(
                method,
                MethodName::EngineHandshake | MethodName::EngineState
            ))
}

pub async fn handle(
    method: MethodName,
    params: &Value,
    context: &HandlerContext,
) -> Option<Result<HandlerOutput, DispatchError>> {
    if !owns(method) {
        return None;
    }
    Some(match method {
        MethodName::SettingsRead => settings_read(params, context),
        MethodName::SettingsUpdate => settings_update(params, context),
        MethodName::SettingsFlush => settings_flush(params, context),
        MethodName::EngineDisconnect => engine_disconnect(params, context),
        MethodName::EngineShutdown => engine_shutdown(params, context),
        MethodName::EngineUpdatePrepare => engine_update_prepare(params, context),
        MethodName::EngineUpdateAbort => engine_update_abort(params, context).await,
        MethodName::UpdatesState => updates_state(params, context).await,
        MethodName::UpdatesCheck => updates_check(params, context).await,
        MethodName::UpdatesDownload => updates_download(params, context),
        MethodName::UpdatesFocused => updates_focused(params, context),
        MethodName::UpdatesSetAutomaticChecks => {
            updates_set_automatic_checks(params, context).await
        }
        _ => Err(DispatchError::unsupported(method_name(method))),
    })
}

fn output(value: Value) -> HandlerOutput {
    HandlerOutput {
        value,
        event_barrier: None,
    }
}

fn encode<T: serde::Serialize>(value: &T) -> Result<Value, DispatchError> {
    serde_json::to_value(value).map_err(|error| DispatchError::internal(error.to_string()))
}

// MARK: Settings

fn settings_read(params: &Value, context: &HandlerContext) -> Result<HandlerOutput, DispatchError> {
    let _: silo_api::methods::settings::SettingsReadParams = context.decode_params(params)?;
    // A silod process initializes settings on first access from its channel-specific path.
    silo_engine::settings::current_settings().map_err(DispatchError::internal)?;
    let snapshot = canonical_settings_snapshot(
        silo_engine::settings::read(false).map_err(DispatchError::internal)?,
    )?;
    let event_barrier = context
        .event_bus()
        .domain_barrier(context.client_id(), "settings")
        .map_err(|error| DispatchError::internal(error.to_string()))?;
    Ok(HandlerOutput {
        value: encode(&snapshot)?,
        event_barrier: Some(event_barrier),
    })
}

fn settings_update(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let params: SettingsUpdateParams = context.decode_params(params)?;
    silo_engine::settings::current_settings().map_err(DispatchError::internal)?;
    let patch = serde_json::to_value(params.patch)
        .map_err(|error| DispatchError::internal(error.to_string()))?
        .as_object()
        .cloned()
        .ok_or_else(|| DispatchError::internal("Settings patch must be an object."))?;
    let snapshot = silo_engine::settings::update(patch).map_err(DispatchError::internal)?;
    publish_settings(snapshot, context)
}

fn settings_flush(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let _: silo_api::methods::settings::SettingsFlushParams = context.decode_params(params)?;
    silo_engine::settings::current_settings().map_err(DispatchError::internal)?;
    let (snapshot, save_result) =
        silo_engine::settings::flush().map_err(DispatchError::internal)?;
    publish_settings(snapshot, context)?;
    save_result.map_err(DispatchError::internal)?;
    Ok(output(Value::Null))
}

fn publish_settings(
    snapshot: silo_engine::settings::Snapshot,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let value = encode(&canonical_settings_snapshot(snapshot)?)?;
    context
        .engine_context()
        .publish(Event::new("settings.changed", value.clone()), Audience::All);
    Ok(output(value))
}

/// The engine's snapshot in the canonical shape: the retired notification fallback keys are
/// never known to the engine (60 §2.3), and the draft is always null (95 A-5).
fn canonical_settings_snapshot(
    snapshot: silo_engine::settings::Snapshot,
) -> Result<SettingsSnapshot, DispatchError> {
    // The same projection every public settings event uses: no draft ever reaches the
    // canonical shape, so a draft the engine still holds can't fail a write that succeeded.
    let value = serde_json::to_value(silo_engine::settings::public_snapshot(&snapshot))
        .map_err(|error| DispatchError::internal(error.to_string()))?;
    serde_json::from_value(value).map_err(|error| {
        DispatchError::internal(format!("The settings snapshot is invalid: {error}"))
    })
}

// MARK: Engine lifecycle

fn engine_disconnect(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let _: EngineDisconnectParams = context.decode_params(params)?;
    // The dispatcher closes this connection after the reply is written, which disposes the
    // caller's subscriptions. Accepted jobs and computers are independent of the session, and
    // no lifecycle flag changes (05 §6.2).
    Ok(output(Value::Null))
}

fn arc_context(context: &HandlerContext) -> Arc<SilodContext> {
    Arc::new(context.silod_context().clone())
}

fn update_state(context: &dyn EngineContext) -> Arc<UpdateState> {
    context.runtime_state().update().clone()
}

/// Installs silod's real platform unless one was installed already (tests install fakes).
fn ensure_platform(context: &HandlerContext) -> Arc<UpdateState> {
    let state = update_state(context.engine_context());
    if state.platform().is_err() {
        state.set_platform(Arc::new(ServicesPlatform::new(arc_context(context))));
    }
    state
}

fn lifecycle_error(error: LifecycleError) -> DispatchError {
    match error {
        LifecycleError::Busy(message) => DispatchError {
            code: ErrorCode::Busy,
            message,
        },
        LifecycleError::Refused(message) => DispatchError::internal(message),
    }
}

/// `UpdateInProgress` for the updater's own conflicts, `Busy` for admission, else `Internal`.
fn update_error(message: String) -> DispatchError {
    let code = if message.contains("already running") {
        ErrorCode::UpdateInProgress
    } else if message.starts_with("Wait ") || message.contains("shutting down") {
        ErrorCode::Busy
    } else {
        ErrorCode::Internal
    };
    DispatchError { code, message }
}

fn job_output(job_id: JobId) -> Result<HandlerOutput, DispatchError> {
    encode(&JobStartResult { job_id }).map(output)
}

fn finish_result(context: &SilodContext, job: JobId, value: Value) {
    if let Err(error) = silo_engine::jobs::finish_with_result(context, job, value, Audience::All) {
        eprintln!("silod: could not persist a job result: {error}");
    }
}

fn finish_error(context: &SilodContext, job: JobId, code: ErrorCode, message: String) {
    if let Err(error) =
        silo_engine::jobs::finish_with_error(context, job, code, message, Audience::All)
    {
        eprintln!("silod: could not persist a job error: {error}");
    }
}

fn publish_shutdown_changed(context: &dyn EngineContext, shutting_down: bool) {
    if let Ok(payload) = serde_json::to_value(ShutdownChangedPayload { shutting_down }) {
        context.publish(
            Event::new("engine.shutdown.changed", payload),
            Audience::All,
        );
    }
}

fn engine_shutdown(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let params: EngineShutdownParams = context.decode_params(params)?;
    let state = ensure_platform(context);
    let request = ShutdownRequest {
        stop_computers: params.stop_computers,
        reason: params.reason,
        deadline: params.deadline_ms.map(Duration::from_millis),
    };
    match state.begin_shutdown(request).map_err(lifecycle_error)? {
        ShutdownStart::Attach(job) => return job_output(job),
        ShutdownStart::New => {}
    }
    let started = match context.start_job("engine.shutdown") {
        Ok(started) => started,
        Err(error) => {
            let _ = state.reopen_after_failed_shutdown();
            return Err(error);
        }
    };
    let job = started.job_id.clone();
    state.shutdown_job(job.clone());
    publish_shutdown_changed(context.engine_context(), true);
    let engine = arc_context(context);
    tokio::task::spawn_blocking(move || run_shutdown(&engine, &state, Some(job)));
    encode(&started).map(output)
}

/// The shutdown job: the outcome is durable before the process exits, and the process exits
/// only after a shutdown that succeeded (or a session end, which never cancels).
fn run_shutdown(context: &Arc<SilodContext>, state: &UpdateState, job: Option<JobId>) {
    match state.run_shutdown() {
        Ok(()) => {
            if let Some(job) = job {
                finish_result(context, job, Value::Null);
            }
            // Let the reply and the final events leave before the listeners go away.
            std::thread::sleep(EXIT_DRAIN);
            context.process().exit(0);
        }
        Err(message) => {
            if let Err(error) = state.reopen_after_failed_shutdown() {
                eprintln!("silod: could not reopen service after a failed shutdown: {error}");
            }
            publish_shutdown_changed(context.as_ref(), false);
            match job {
                Some(job) => finish_error(context, job, ErrorCode::Internal, message),
                None => eprintln!("silod: shutdown failed: {message}"),
            }
        }
    }
}

/// The session is ending (SIGTERM): shut down as `engine.shutdown {reason: "sessionEnd"}`
/// does, with no prompt and the default bound. A failure never cancels it; with an update
/// prepared the saved set is released instead of stopped. If no shutdown can even begin, the
/// process still exits.
pub fn session_end(context: Arc<SilodContext>) {
    let state = update_state(context.as_ref());
    if state.platform().is_err() {
        state.set_platform(Arc::new(ServicesPlatform::new(context.clone())));
    }
    let request = ShutdownRequest {
        stop_computers: true,
        reason: silo_api::methods::engine::ShutdownReason::SessionEnd,
        deadline: None,
    };
    match state.begin_shutdown(request) {
        // A shutdown is already under way; it exits the process by itself.
        Ok(ShutdownStart::Attach(_)) => {}
        Ok(ShutdownStart::New) => {
            let job = match silo_engine::jobs::allocate(context.as_ref(), "engine.shutdown") {
                Ok(accepted) => Some(accepted.job_id),
                Err(error) => {
                    eprintln!("silod: could not record the session-end shutdown job: {error}");
                    None
                }
            };
            if let Some(job) = &job {
                state.shutdown_job(job.clone());
            }
            publish_shutdown_changed(context.as_ref(), true);
            let spawned = std::thread::Builder::new()
                .name("silod-session-end".into())
                .spawn({
                    let (context, state) = (context.clone(), state.clone());
                    move || run_shutdown(&context, &state, job)
                });
            if let Err(error) = spawned {
                eprintln!("silod: could not start the session-end shutdown: {error}");
                context.process().exit(0);
            }
        }
        Err(error) => {
            eprintln!("silod: session-end shutdown refused: {}", error.message());
            context.process().exit(0);
        }
    }
}

fn engine_update_prepare(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let _: EngineUpdatePrepareParams = context.decode_params(params)?;
    let state = ensure_platform(context);
    match state.begin_prepare().map_err(lifecycle_error)? {
        PrepareStart::Attach(job) => return job_output(job),
        PrepareStart::New => {}
    }
    let started = match context.start_job("engine.update.prepare") {
        Ok(started) => started,
        Err(error) => {
            state.prepare_failed(context.engine_context());
            return Err(error);
        }
    };
    let job = started.job_id.clone();
    state.prepare_job(job.clone());
    let engine = arc_context(context);
    tokio::task::spawn_blocking(move || run_prepare(&engine, &state, job));
    encode(&started).map(output)
}

fn run_prepare(context: &Arc<SilodContext>, state: &Arc<UpdateState>, job: JobId) {
    let prepared = state.platform().and_then(|platform| {
        let hold = Hold::start(
            platform,
            engine_update::store(context.as_ref()),
            Some(job.clone()),
        )?;
        let request = PrepareRequest {
            transaction_id: uuid::Uuid::new_v4().to_string(),
            prepare_job_id: job.to_string(),
            source_engine_version: context.identity().version.clone(),
            host_build: String::new(),
        };
        match hold.prepare(request) {
            Ok(computers) => Ok((hold, computers)),
            Err(error) => {
                hold.end();
                Err(error)
            }
        }
    });
    match prepared {
        Ok((hold, computers)) => {
            // Admission stays closed from here until shutdown or abort: the state keeps the hold.
            state.prepared(job.clone(), hold);
            spawn_watchdog(context.clone(), state.clone());
            let result = EngineUpdatePrepareFinishedResult {
                resuming: computers
                    .into_iter()
                    .map(|computer| ResumingComputer {
                        id: computer.id,
                        name: computer.name,
                    })
                    .collect(),
            };
            match serde_json::to_value(result) {
                Ok(value) => finish_result(context, job, value),
                Err(error) => finish_error(context, job, ErrorCode::Internal, error.to_string()),
            }
        }
        Err(message) => {
            // A rollback that could not resume everything leaves its journal: the update stays
            // pending and those computers stay reserved until abort or the next start.
            state.prepare_failed(context.as_ref());
            let code = if message.starts_with("Wait ") {
                ErrorCode::Busy
            } else {
                ErrorCode::Internal
            };
            finish_error(context, job, code, message);
        }
    }
}

async fn engine_update_abort(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let _: EngineUpdateAbortParams = context.decode_params(params)?;
    let state = ensure_platform(context);
    let engine = arc_context(context);
    tokio::task::spawn_blocking(move || state.abort(engine.as_ref()))
        .await
        .map_err(|_| DispatchError::internal("The update abort was interrupted."))?
        .map_err(|message| DispatchError {
            code: ErrorCode::Internal,
            message,
        })?;
    Ok(output(Value::Null))
}

// MARK: Updates

fn publish_updates(context: &dyn EngineContext, snapshot: &UpdateSnapshot) {
    if let Ok(payload) = serde_json::to_value(snapshot) {
        context.publish(Event::new("updates.changed", payload), Audience::All);
    }
}

async fn updates_state(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let _: silo_api::methods::common::EmptyParams = context.decode_params(params)?;
    let state = ensure_platform(context);
    let engine = arc_context(context);
    let snapshot = tokio::task::spawn_blocking(move || -> Result<UpdateSnapshot, String> {
        let updater = state.updater(engine.as_ref())?;
        let readiness = if state.update_pending() || state.shutting_down() {
            Err("An update operation is already running.".to_owned())
        } else {
            state.platform()?.update_readiness()
        };
        let snapshot = updater.apply_readiness(readiness);
        publish_updates(engine.as_ref(), &snapshot);
        Ok(snapshot)
    })
    .await
    .map_err(|_| DispatchError::internal("The update state could not be read."))?
    .map_err(DispatchError::internal)?;
    encode(&snapshot).map(output)
}

async fn updates_check(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let _: silo_api::methods::updates::UpdatesCheckParams = context.decode_params(params)?;
    let state = ensure_platform(context);
    let engine = arc_context(context);
    let snapshot = tokio::task::spawn_blocking(move || -> Result<UpdateSnapshot, String> {
        let updater = state.updater(engine.as_ref())?;
        let snapshot = updater.check(false)?;
        publish_updates(engine.as_ref(), &snapshot);
        Ok(snapshot)
    })
    .await
    .map_err(|_| DispatchError::internal("The update check was interrupted."))?
    .map_err(update_error)?;
    encode(&snapshot).map(output)
}

fn updates_download(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let _: silo_api::methods::updates::UpdatesDownloadParams = context.decode_params(params)?;
    let state = ensure_platform(context);
    let engine = arc_context(context);
    let updater = state
        .updater(engine.as_ref())
        .map_err(DispatchError::internal)?;
    let started = context.start_job("updates.download")?;
    let job = started.job_id.clone();
    let initial = match updater.begin_download() {
        Ok(initial) => initial,
        Err(message) => {
            // The job ends at once with the same refusal the caller receives.
            let error = update_error(message);
            finish_error(&engine, job, error.code, error.message.clone());
            return Err(error);
        }
    };
    publish_updates(engine.as_ref(), &engine_update::updater::to_api(&initial));
    publish_download_progress(engine.as_ref(), &job, &initial);
    tokio::task::spawn_blocking(move || {
        let result = updater.download(|snapshot| {
            publish_updates(engine.as_ref(), &engine_update::updater::to_api(snapshot));
            publish_download_progress(engine.as_ref(), &job, snapshot);
        });
        publish_updates(engine.as_ref(), &updater.snapshot());
        match result {
            Ok(snapshot) => match serde_json::to_value(&snapshot) {
                Ok(value) => finish_result(&engine, job, value),
                Err(error) => finish_error(&engine, job, ErrorCode::Internal, error.to_string()),
            },
            Err(message) => finish_error(&engine, job, ErrorCode::Internal, message),
        }
    });
    encode(&started).map(output)
}

fn publish_download_progress(
    context: &dyn EngineContext,
    job: &JobId,
    snapshot: &silo_engine::updates::Snapshot,
) {
    let data = match engine_update::updater::to_progress(snapshot) {
        Ok(data) => data,
        Err(error) => {
            eprintln!("silod: could not project update progress: {error}");
            return;
        }
    };
    if let Err(error) = silo_engine::jobs::publish_progress(
        context,
        JobProgress::Update {
            job_id: job.clone(),
            kind: None,
            computer_id: None,
            data,
        },
        Audience::All,
    ) {
        eprintln!("silod: could not persist update progress: {error}");
    }
}

fn updates_focused(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let _: silo_api::methods::updates::UpdatesFocusedParams = context.decode_params(params)?;
    let state = ensure_platform(context);
    state
        .updater(context.engine_context())
        .map_err(DispatchError::internal)?
        .focused();
    Ok(output(Value::Null))
}

async fn updates_set_automatic_checks(
    params: &Value,
    context: &HandlerContext,
) -> Result<HandlerOutput, DispatchError> {
    let params: UpdatesSetAutomaticChecksParams = context.decode_params(params)?;
    let state = ensure_platform(context);
    let engine = arc_context(context);
    let snapshot = tokio::task::spawn_blocking(move || -> Result<UpdateSnapshot, String> {
        // The durable write fsyncs twice; keep it off the I/O threads.
        let snapshot = state
            .updater(engine.as_ref())?
            .set_automatic_checks(params.enabled)?;
        publish_updates(engine.as_ref(), &snapshot);
        Ok(snapshot)
    })
    .await
    .map_err(|_| DispatchError::internal("Update preferences could not be saved."))?
    .map_err(DispatchError::internal)?;
    encode(&snapshot).map(output)
}

// MARK: Engine start

/// Starts the engine's lifecycle upkeep: installs the real platform, resumes computers a
/// previous engine update saved before anything else advances them, and runs the engine
/// update schedule on its own timer (95 A-26). Call once after the context is installed.
pub fn start(context: Arc<SilodContext>) {
    // Launch migration (95 §3: no legacy support but this): the canonical settings have no
    // onboarding draft, so one a previous app version left in the file is dropped here and
    // never handled when settings are read or written.
    if let Err(error) = silo_engine::settings::current_settings()
        .and_then(|_| silo_engine::settings::discard_onboarding_draft())
    {
        eprintln!("silod: could not migrate the settings at launch: {error}");
    }
    let state = update_state(context.as_ref());
    state.set_platform(Arc::new(ServicesPlatform::new(context.clone())));
    // Visible in `engine.state` from the first request, before the recovery thread runs.
    state.mark_recovery_pending(context.as_ref());
    let recovery_context = context.clone();
    let recovery_state = state.clone();
    let spawned = std::thread::Builder::new()
        .name("silod-update-recovery".into())
        .spawn(move || recover_saved_computers(&recovery_context, &recovery_state));
    if let Err(error) = spawned {
        eprintln!("silod: could not start update recovery: {error}");
    }
    let spawned = std::thread::Builder::new()
        .name("silod-update-schedule".into())
        .spawn(move || loop {
            std::thread::sleep(silo_engine::updates::POLL_INTERVAL);
            let Ok(updater) = state.updater(context.as_ref()) else {
                continue;
            };
            let before = updater.snapshot();
            if let Ok(after) = updater.check(true) {
                if after != before {
                    publish_updates(context.as_ref(), &after);
                }
            }
        });
    if let Err(error) = spawned {
        eprintln!("silod: could not start the update schedule: {error}");
    }
}

/// Resumes the set a previous engine update saved, and reports what could not resume.
fn recover_saved_computers(context: &Arc<SilodContext>, state: &Arc<UpdateState>) {
    let result = state.recover(context.as_ref());
    if matches!(result, Ok(None)) && state.watching_installer() {
        // An installer still owns the saved set: this engine waits with admission closed.
        spawn_watchdog(context.clone(), state.clone());
    }
    report_recovery(context, state, result);
}

fn report_recovery(
    context: &Arc<SilodContext>,
    state: &Arc<UpdateState>,
    result: Result<Option<engine_update::Recovery>, String>,
) {
    let failure = match result {
        Ok(None) => return,
        Ok(Some(recovery)) => {
            let mut notes: Vec<String> = recovery
                .failed
                .iter()
                .map(|(computer, why)| format!("{}: {why}", computer.name))
                .collect();
            notes.extend(
                recovery
                    .lost
                    .iter()
                    .map(|(computer, why)| format!("{}: {why}", computer.name)),
            );
            (!notes.is_empty()).then(|| notes.join("\n"))
        }
        Err(message) => Some(message),
    };
    if let Some(message) = failure {
        retain_resume_failure(context, &message);
        if let Ok(updater) = state.updater(context.as_ref()) {
            let snapshot = updater.recovery_failed(
                "Some computers could not resume after updating. Relaunch Silo to retry.",
                &message,
            );
            publish_updates(context.as_ref(), &snapshot);
        }
    }
}

/// While an update is held for the installer, probes the installer's lock every
/// [`installer::PROBE_INTERVAL`]; a coordinator that stays gone for
/// [`installer::ABANDONED_AFTER`] has the update aborted (or a deferred set restored).
fn spawn_watchdog(context: Arc<SilodContext>, state: Arc<UpdateState>) {
    let spawned = std::thread::Builder::new()
        .name("silod-update-watchdog".into())
        .spawn(move || {
            while state.watching_installer() {
                std::thread::sleep(engine_update::installer::PROBE_INTERVAL);
                let present = engine_update::installer::coordinator_present(
                    &engine_update::engine_directory(context.as_ref()),
                );
                match state.watchdog_tick(context.as_ref(), present, std::time::Instant::now()) {
                    engine_update::WatchdogOutcome::Recovered(result) => {
                        report_recovery(&context, &state, result)
                    }
                    engine_update::WatchdogOutcome::Aborted(Err(message)) => {
                        eprintln!("silod: the abandoned update could not be aborted: {message}");
                    }
                    _ => {}
                }
            }
        });
    if let Err(error) = spawned {
        eprintln!("silod: could not start the update watchdog: {error}");
    }
}

#[cfg(test)]
#[path = "services/tests.rs"]
mod tests;
