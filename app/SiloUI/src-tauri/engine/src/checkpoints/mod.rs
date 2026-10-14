//! The two-platform adapter behind the canonical `checkpoints.*` methods (P3-checkpoints).
//!
//! Linux and macOS keep their own record formats and workflows (`runtime::checkpoints`,
//! `macos_computers::checkpoints`); this module picks the owner of a computer by inventory,
//! accepts each change as a durable job, runs it on a blocking thread, and publishes the
//! outcome and `checkpoints.changed`. Automatic pre-run capture is caller orchestration: the
//! caller passes `kind: "automatic"` and awaits the successful `{checkpoint}` outcome, and
//! the engine refuses it while the owner's retention setting is zero.

pub mod retention;

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use serde_json::{json, Value};
use silo_api::{
    jobs::{ErrorCode, JobId},
    methods::checkpoints::{CheckpointKind, CheckpointUsage},
};

use crate::{
    context::{Audience, EngineContext, Event},
    jobs::{self, CancelRoute},
    macos_computers_engine::{self as macos_computers, MacosContext},
    runtime::{self, operation_gate::GateError, RuntimeHost, RuntimePaths},
};

pub use crate::macos_computers_engine::RetentionPolicy;

/// Why an automatic capture is refused while the owner setting is zero.
pub const AUTOMATIC_OFF: &str = "Automatic checkpoints are turned off in Silo settings.";

/// A refusal with the canonical error code a client can branch on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub code: ErrorCode,
    pub message: String,
}

impl Failure {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// Classifies an engine message. Specific sentences the workflows are documented to
    /// return map to their code; anything else is an internal failure.
    pub fn from_message(message: impl Into<String>) -> Self {
        let message = message.into();
        let lower = message.to_ascii_lowercase();
        let code = if message == AUTOMATIC_OFF {
            ErrorCode::PermissionDenied
        } else if lower.contains("no longer exists") || lower.contains("not a local computer") {
            ErrorCode::NotFound
        } else if lower.contains("installing an update") {
            ErrorCode::UpdateInProgress
        } else if lower.starts_with("busy:")
            || lower.contains("operation is in progress")
            || lower.contains("is quitting")
            || lower.contains("storage migration")
            || lower.contains("upgrading")
        {
            ErrorCode::Busy
        } else if lower.contains("was cancelled") || lower.contains("cancelled") {
            ErrorCode::Cancelled
        } else {
            ErrorCode::Internal
        };
        Self { code, message }
    }
}

impl From<jobs::RegistryError> for Failure {
    fn from(error: jobs::RegistryError) -> Self {
        let message = error.to_string();
        let code = if message.starts_with("busy:") {
            ErrorCode::Busy
        } else {
            ErrorCode::Internal
        };
        Self { code, message }
    }
}

impl From<GateError> for Failure {
    fn from(error: GateError) -> Self {
        let code = match error {
            GateError::AlreadyQueued => ErrorCode::AlreadyQueued,
            GateError::Busy => ErrorCode::Busy,
            GateError::Cancelled => ErrorCode::Cancelled,
            _ => ErrorCode::Internal,
        };
        Self::new(code, error.to_string())
    }
}

/// The one-time launch upgrade of saved checkpoint records to the layout that stores the
/// policy kind. Every record made before automatic checkpoints existed becomes `manual`,
/// keeping its reason, native identity, dates, memory fields and journals. It must run
/// before anything reads the records and before the macOS sweep, which would otherwise
/// take an unconverted record for garbage. It is idempotent, so an interrupted launch
/// finishes at the next one.
///
/// Linux records live in the runtime storage generation in use; while the storage
/// migration has not finished that storage is the pre-upgrade backup source and is left
/// byte for byte, and its records are converted at the launch after the migration.
pub fn upgrade_layouts(
    context: &dyn EngineContext,
    migration: &crate::runtime_migration::Controller,
) -> Result<(), String> {
    let mut failures = Vec::new();
    if let Ok(storage) = migration.runtime_storage() {
        if let Err(error) = runtime::checkpoints::upgrade_records(&storage) {
            failures.push(error);
        }
    }
    if let Err(error) = macos_computers::upgrade_checkpoint_records(&context.paths().app_data) {
        failures.push(error);
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join(" "))
    }
}

/// Which owner a computer's checkpoints belong to, by inventory rather than id format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Linux,
    Macos,
}

#[derive(Clone)]
pub struct Service {
    host: Arc<dyn RuntimeHost>,
    macos: MacosContext,
    retention: RetentionPolicy,
}

impl Service {
    pub fn new(
        host: Arc<dyn RuntimeHost>,
        macos: MacosContext,
        retention: RetentionPolicy,
    ) -> Self {
        Self {
            host,
            macos,
            retention,
        }
    }

    fn context(&self) -> Arc<dyn EngineContext> {
        self.host.engine_context()
    }

    fn linux_paths(&self) -> Result<RuntimePaths, Failure> {
        runtime::runtime_paths(self.host.as_ref()).map_err(Failure::from_message)
    }

    /// Resolves the owner of `computer_id`: the macOS registry first (it needs no runtime),
    /// then this device's Linux inventory.
    pub fn platform(&self, computer_id: &str) -> Result<Platform, Failure> {
        if macos_computers::has_computer(&self.macos, computer_id) {
            return Ok(Platform::Macos);
        }
        let paths = self.linux_paths()?;
        if runtime::checkpoints::has_computer(&paths, computer_id) {
            Ok(Platform::Linux)
        } else {
            Err(Failure::new(
                ErrorCode::NotFound,
                "This computer no longer exists.",
            ))
        }
    }

    /// `checkpoints.create`: a durable job whose outcome is `{checkpoint}`.
    pub fn start_create(
        &self,
        computer_id: &str,
        name: &str,
        kind: Option<CheckpointKind>,
    ) -> Result<JobId, Failure> {
        let automatic = kind == Some(CheckpointKind::Automatic);
        if automatic && (self.retention)().map_err(Failure::from_message)? == 0 {
            return Err(Failure::new(ErrorCode::PermissionDenied, AUTOMATIC_OFF));
        }
        match self.platform(computer_id)? {
            Platform::Macos => macos_computers::start_macos_checkpoint_job(
                self.macos.clone(),
                computer_id.to_owned(),
                name.to_owned(),
                automatic,
                self.retention.clone(),
            )
            .map_err(Failure::from_message),
            Platform::Linux => {
                let (computer_id, name) = (computer_id.to_owned(), name.to_owned());
                let retention = self.retention.clone();
                self.start_linux_job(
                    "checkpoints.create",
                    true,
                    JobResult::Created,
                    move |paths, job_id, cancellation| {
                        runtime::checkpoints::create_for_job(
                            paths,
                            &computer_id,
                            &name,
                            automatic,
                            retention.as_ref(),
                            job_id,
                            cancellation,
                        )
                        .map(|checkpoint| json!({ "checkpoint": checkpoint }))
                    },
                )
            }
        }
    }

    /// `checkpoints.restore`.
    pub fn start_restore(&self, computer_id: &str, checkpoint_id: &str) -> Result<JobId, Failure> {
        match self.platform(computer_id)? {
            Platform::Macos => macos_computers::start_macos_checkpoint_restore_job(
                self.macos.clone(),
                computer_id.to_owned(),
                checkpoint_id.to_owned(),
            )
            .map_err(Failure::from_message),
            Platform::Linux => {
                let (computer_id, checkpoint_id) =
                    (computer_id.to_owned(), checkpoint_id.to_owned());
                self.start_linux_job(
                    "checkpoints.restore",
                    false,
                    JobResult::State,
                    move |paths, job_id, cancellation| {
                        runtime::checkpoints::restore_for_job(
                            paths,
                            &computer_id,
                            &checkpoint_id,
                            job_id,
                            cancellation,
                        )
                        .map(|()| Value::Null)
                    },
                )
            }
        }
    }

    /// `checkpoints.fork`. Linux forks the computer's current state when no checkpoint is
    /// named; macOS always needs one.
    pub fn start_fork(
        &self,
        computer_id: &str,
        checkpoint_id: Option<&str>,
        new_name: &str,
    ) -> Result<JobId, Failure> {
        match self.platform(computer_id)? {
            Platform::Macos => {
                let checkpoint_id = checkpoint_id.ok_or_else(|| {
                    Failure::new(
                        ErrorCode::Internal,
                        "Choose the checkpoint to fork. macOS computers cannot fork their current state.",
                    )
                })?;
                macos_computers::start_macos_checkpoint_fork_job(
                    self.macos.clone(),
                    computer_id.to_owned(),
                    checkpoint_id.to_owned(),
                    new_name.to_owned(),
                )
                .map_err(Failure::from_message)
            }
            Platform::Linux => {
                let computer_id = computer_id.to_owned();
                let checkpoint_id = checkpoint_id.map(str::to_owned);
                let new_name = new_name.to_owned();
                let macos = self.macos.clone();
                self.start_linux_job(
                    "checkpoints.fork",
                    false,
                    JobResult::State,
                    move |paths, job_id, cancellation| {
                        runtime::checkpoints::fork_for_job(
                            paths,
                            &EngineForkAssignments,
                            &computer_id,
                            checkpoint_id.as_deref(),
                            &new_name,
                            &|| macos_computers::names(&macos),
                            job_id,
                            cancellation,
                        )
                        .map(|()| Value::Null)
                    },
                )
            }
        }
    }

    /// `checkpoints.delete`.
    pub fn start_delete(&self, computer_id: &str, checkpoint_id: &str) -> Result<JobId, Failure> {
        match self.platform(computer_id)? {
            Platform::Macos => macos_computers::start_macos_checkpoint_delete_job(
                self.macos.clone(),
                computer_id.to_owned(),
                checkpoint_id.to_owned(),
            )
            .map_err(Failure::from_message),
            Platform::Linux => {
                let (computer_id, checkpoint_id) =
                    (computer_id.to_owned(), checkpoint_id.to_owned());
                self.start_linux_job(
                    "checkpoints.delete",
                    false,
                    JobResult::State,
                    move |paths, job_id, cancellation| {
                        runtime::checkpoints::delete_for_job(
                            paths,
                            &computer_id,
                            &checkpoint_id,
                            job_id,
                            cancellation,
                        )
                        .map(|()| Value::Null)
                    },
                )
            }
        }
    }

    /// `checkpoints.abandonRestore`.
    pub fn start_abandon_restore(&self, computer_id: &str) -> Result<JobId, Failure> {
        match self.platform(computer_id)? {
            Platform::Macos => macos_computers::start_macos_checkpoint_abandon_job(
                self.macos.clone(),
                computer_id.to_owned(),
            )
            .map_err(Failure::from_message),
            Platform::Linux => {
                let computer_id = computer_id.to_owned();
                self.start_linux_job(
                    "checkpoints.abandonRestore",
                    false,
                    JobResult::State,
                    move |paths, job_id, cancellation| {
                        runtime::checkpoints::abandon_restore_for_job(
                            paths,
                            &computer_id,
                            job_id,
                            cancellation,
                        )
                        .map(|()| Value::Null)
                    },
                )
            }
        }
    }

    /// `checkpoints.usage`: a query. It can run `msb` listings, so callers run it off the
    /// async workers.
    pub fn usage(&self, computer_id: &str) -> Result<CheckpointUsage, Failure> {
        match self.platform(computer_id)? {
            Platform::Macos => macos_computers::macos_checkpoint_usage(&self.macos, computer_id)
                .map_err(Failure::from_message),
            Platform::Linux => {
                let paths = self.linux_paths()?;
                let usage = runtime::checkpoints::read_usage(&paths, computer_id)
                    .map_err(Failure::from_message)?;
                serde_json::to_value(usage)
                    .and_then(serde_json::from_value)
                    .map_err(|error| Failure::new(ErrorCode::Internal, error.to_string()))
            }
        }
    }

    /// Accepts one Linux checkpoint job durably, registers its cancellation and runs `work`
    /// on a blocking thread. The terminal outcome is persisted before it is published.
    fn start_linux_job<F>(
        &self,
        method: &'static str,
        cancellable: bool,
        result: JobResult,
        work: F,
    ) -> Result<JobId, Failure>
    where
        F: FnOnce(&RuntimePaths, JobId, Arc<AtomicBool>) -> Result<Value, String> + Send + 'static,
    {
        self.host
            .ensure_runtime_ready()
            .map_err(Failure::from_message)?;
        runtime::shutdown::ensure_accepting_operations()
            .map_err(|message| Failure::new(ErrorCode::Busy, message))?;
        let paths = self.linux_paths()?;
        let context = self.context();
        let accepted = jobs::allocate(context.as_ref(), method)?;
        let job_id = accepted.job_id;
        let registry = context.job_registry().map_err(Failure::from_message)?;
        let cancellation = Arc::new(AtomicBool::new(false));
        {
            let (cancel_id, cancel_flag) = (job_id.clone(), cancellation.clone());
            registry.register_cancel(job_id.clone(), CancelRoute::JobGate, move || {
                // Work that is not yet queued has no gate entry to cancel: keep the intent.
                if cancellable || !runtime::OPERATIONS.contains_job(&cancel_id) {
                    cancel_flag.store(true, Ordering::SeqCst);
                }
                runtime::OPERATIONS
                    .cancel_job(&cancel_id)
                    .map_err(|error| error.to_string())
            });
        }
        let host = self.host.clone();
        let response = job_id.clone();
        tokio::task::spawn_blocking(move || {
            // The job runs only once the queue admits its operation: the operation marks it
            // running then, and a failure to do so refuses the operation.
            let (running_registry, running_id) = (registry.clone(), job_id.clone());
            let outcome = runtime::checkpoints::with_admission(
                move || {
                    running_registry
                        .mark_running(&running_id)
                        .map_err(|error| error.to_string())
                },
                || work(&paths, job_id.clone(), cancellation.clone()),
            );
            finish_linux_job(
                host.as_ref(),
                &paths,
                job_id,
                result,
                outcome,
                cancellation.load(Ordering::SeqCst),
            );
        });
        Ok(response)
    }
}

/// What a Linux job reports on success: the capture record it made, or the refreshed
/// application state that today's commands return (10 §1.3).
#[derive(Clone, Copy)]
enum JobResult {
    Created,
    State,
}

fn finish_linux_job(
    host: &dyn RuntimeHost,
    paths: &RuntimePaths,
    job_id: JobId,
    result: JobResult,
    outcome: Result<Value, String>,
    cancelled: bool,
) {
    let context = host.engine_context();
    // The refresh runs whatever the outcome: a failed change can still have recorded a
    // checkpoint operation, and clients re-read on `checkpoints.changed`.
    let state = runtime::application_state_response(host, paths)
        .ok()
        .and_then(|state| serde_json::to_value(state).ok());
    let finished = match outcome {
        Ok(value) => {
            let value = match result {
                JobResult::Created => value,
                JobResult::State => state.clone().unwrap_or(Value::Null),
            };
            jobs::finish_with_result(context.as_ref(), job_id, value, Audience::All)
        }
        Err(message) => {
            let failure = Failure::from_message(message);
            let code = if cancelled && failure.code == ErrorCode::Internal {
                ErrorCode::Cancelled
            } else {
                failure.code
            };
            jobs::finish_with_error(
                context.as_ref(),
                job_id,
                code,
                failure.message,
                Audience::All,
            )
        }
    };
    if let Err(error) = finished {
        eprintln!("silo-engine: could not persist checkpoint job outcome: {error}");
    }
    if let Some(state) = state {
        context.publish(
            Event::new("checkpoints.changed", json!({ "linux": state })),
            Audience::All,
        );
    }
}

/// GitHub and secret assignments follow a fork on this device.
struct EngineForkAssignments;

impl runtime::checkpoints::ForkAssignments for EngineForkAssignments {
    fn copy_github(&self, source: &str, target: &str) -> Result<(), String> {
        crate::github::fork_assignment(source, target)
    }

    fn forget_github(&self, target: &str) -> Result<(), String> {
        crate::github::forget_fork_assignment(target)
    }

    fn copy_secrets(&self, source: &str, target: &str) -> Result<(), String> {
        crate::secrets::fork_assignments(source, target)
    }

    fn forget_secrets(&self, target: &str) -> Result<(), String> {
        crate::secrets::computer_removed(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        context::{
            EngineIdentity, EnginePaths, EngineRuntimeState, MainThread, PackageKind,
            ProcessControl,
        },
        macos_computers_engine::MacosHost,
        runtime::RuntimePackageKind,
    };
    use std::{
        path::PathBuf,
        sync::Mutex,
        time::{Duration, Instant},
    };

    const LINUX_ID: &str = "11111111-1111-4111-8111-111111111111";

    struct TestEngine {
        paths: EnginePaths,
        identity: EngineIdentity,
        runtime_state: Arc<EngineRuntimeState>,
        published: Mutex<Vec<Event>>,
    }

    impl EngineContext for TestEngine {
        fn paths(&self) -> &EnginePaths {
            &self.paths
        }
        fn identity(&self) -> &EngineIdentity {
            &self.identity
        }
        fn runtime_state(&self) -> Arc<EngineRuntimeState> {
            self.runtime_state.clone()
        }
        fn publish(&self, event: Event, _audience: Audience) {
            self.published.lock().unwrap().push(event);
        }
        fn main_thread(&self) -> Option<&dyn MainThread> {
            None
        }
        fn process(&self) -> &dyn ProcessControl {
            self
        }
    }

    impl ProcessControl for TestEngine {
        fn exit(&self, _code: i32) {}
        fn restart(&self) -> ! {
            panic!("checkpoint tests must not restart the process")
        }
    }

    struct TestHost {
        engine: Arc<TestEngine>,
        storage: PathBuf,
        tools: PathBuf,
    }

    impl RuntimeHost for TestHost {
        fn engine_context(&self) -> Arc<dyn EngineContext> {
            self.engine.clone()
        }
        fn runtime_storage(&self) -> Result<PathBuf, String> {
            Ok(self.storage.clone())
        }
        fn blocks_operations(&self) -> bool {
            false
        }
        fn bundled_tools_directory(&self) -> Result<PathBuf, String> {
            Ok(self.tools.clone())
        }
        fn ensure_runtime_ready(&self) -> Result<(), String> {
            Ok(())
        }
        fn runtime_package_kind(&self) -> RuntimePackageKind {
            RuntimePackageKind::MacApp
        }
    }

    struct NoDisplays;

    impl MacosHost for NoDisplays {
        fn show_display(
            &self,
            _context: &MacosContext,
            _id: &str,
            _title: &str,
            _toolbar: bool,
        ) -> Result<(), String> {
            Err("no display in tests".into())
        }
        fn close_display(&self, _id: &str) {}
        fn linux_computer_names(&self) -> Vec<String> {
            Vec::new()
        }
    }

    struct Fixture {
        _directory: tempfile::TempDir,
        engine: Arc<TestEngine>,
        service: Service,
        macos_id: String,
        macos_checkpoint: String,
    }

    fn fixture(retention: u32) -> Fixture {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let engine = Arc::new(TestEngine {
            paths: EnginePaths {
                app_data: root.join("data"),
                home: root.join("home"),
                ..EnginePaths::default()
            },
            identity: EngineIdentity {
                channel: crate::channel::Channel::Production,
                version: "test".into(),
                package_kind: PackageKind::MacApp,
            },
            runtime_state: Arc::new(EngineRuntimeState::default()),
            published: Mutex::new(Vec::new()),
        });
        let storage = root.join("runtime");
        std::fs::create_dir_all(&storage).unwrap();
        let configuration: runtime::ComputerConfiguration = serde_json::from_value(json!({
            "id": LINUX_ID, "name": "linux-dev", "cpus": 1, "maxCPUs": 1, "memoryGiB": 1,
            "maxMemoryGiB": 1, "workspaceStorageGiB": 1, "runtimeStorageGiB": 1
        }))
        .unwrap();
        runtime::write_metadata(
            &storage.join("computers.json"),
            &runtime::ComputerConfigurationRequest {
                schema_version: 1,
                computers: vec![configuration],
            },
        )
        .unwrap();
        macos_computers::reset_registry_for_test();
        let (macos_id, macos_checkpoint) =
            macos_computers::seed_test_computer(&engine.paths.app_data, "mac-dev");
        let host = Arc::new(TestHost {
            engine: engine.clone(),
            storage,
            tools: root.join("tools"),
        });
        let macos = MacosContext::new(engine.clone(), Arc::new(NoDisplays));
        let service = Service::new(host, macos, Arc::new(move || Ok(retention)));
        Fixture {
            _directory: directory,
            engine,
            service,
            macos_id,
            macos_checkpoint,
        }
    }

    fn wait_finished(fixture: &Fixture, job_id: &JobId) -> silo_api::jobs::JobLookupResult {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let found = jobs::get_at(fixture.engine.as_ref(), job_id, None)
                .unwrap()
                .expect("the job was accepted");
            if found.state == silo_api::jobs::JobState::Finished {
                return found;
            }
            assert!(Instant::now() < deadline, "the job never finished");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn the_owner_of_a_computer_is_found_by_inventory_not_by_id_format() {
        let _state = crate::test_support::global_state();
        let fixture = fixture(3);
        assert_eq!(fixture.service.platform(LINUX_ID), Ok(Platform::Linux));
        assert_eq!(
            fixture.service.platform(&fixture.macos_id),
            Ok(Platform::Macos)
        );
        let missing = fixture
            .service
            .platform("22222222-2222-4222-8222-222222222222");
        assert_eq!(missing.unwrap_err().code, ErrorCode::NotFound);
    }

    #[test]
    fn an_automatic_capture_is_refused_while_retention_is_zero_on_both_platforms() {
        let _state = crate::test_support::global_state();
        let fixture = fixture(0);
        for id in [LINUX_ID, fixture.macos_id.as_str()] {
            let refused = fixture
                .service
                .start_create(
                    id,
                    "Before run: x, deleted automatically",
                    Some(CheckpointKind::Automatic),
                )
                .unwrap_err();
            assert_eq!(refused.code, ErrorCode::PermissionDenied, "{id}");
            assert_eq!(refused.message, AUTOMATIC_OFF);
        }
        assert!(
            fixture.engine.published.lock().unwrap().is_empty(),
            "a refusal accepts no job"
        );
    }

    fn test_runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap()
    }

    /// Starts a Linux delete job behind a held device lane and returns once it is queued.
    fn queued_delete(
        fixture: &Fixture,
    ) -> (JobId, runtime::operation_gate::OperationGuard<'static>) {
        let hold = runtime::OPERATIONS.device("Holding the lane").unwrap();
        let job_id = fixture
            .service
            .start_delete(LINUX_ID, "no-such-checkpoint")
            .unwrap();
        while runtime::OPERATIONS.snapshot().waiting.is_empty() {
            std::thread::yield_now();
        }
        (job_id, hold)
    }

    #[test]
    fn a_linux_job_waits_until_the_queue_admits_it_and_cancels_while_waiting() {
        let _state = crate::test_support::global_state();
        let fixture = fixture(3);
        let runtime = test_runtime();
        let _entered = runtime.enter();

        let (job_id, hold) = queued_delete(&fixture);
        let queued = jobs::get_at(fixture.engine.as_ref(), &job_id, None)
            .unwrap()
            .unwrap();
        assert_eq!(queued.state, silo_api::jobs::JobState::Waiting);
        drop(hold);
        let finished = wait_finished(&fixture, &job_id);
        assert!(finished.outcome.is_some());

        // Cancelled while waiting, it never runs.
        let (job_id, hold) = queued_delete(&fixture);
        fixture
            .engine
            .job_registry()
            .unwrap()
            .cancel(&job_id)
            .unwrap();
        let cancelled = wait_finished(&fixture, &job_id);
        assert_eq!(
            cancelled.outcome.unwrap().error.unwrap().code,
            ErrorCode::Cancelled
        );
        drop(hold);
    }

    #[test]
    fn a_linux_job_whose_running_mark_cannot_be_saved_finishes_with_an_error_and_does_not_run() {
        let _state = crate::test_support::global_state();
        let fixture = fixture(3);
        let runtime = test_runtime();
        let _entered = runtime.enter();
        let registry = fixture.engine.job_registry().unwrap();
        rusqlite::Connection::open(registry.path())
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER reject_running BEFORE UPDATE OF state ON jobs
                 WHEN NEW.state = 'running'
                 BEGIN SELECT RAISE(ABORT, 'injected mark_running failure'); END;",
            )
            .unwrap();

        let (job_id, hold) = queued_delete(&fixture);
        drop(hold);
        let finished = wait_finished(&fixture, &job_id);
        let error = finished.outcome.unwrap().error.unwrap();
        assert_eq!(error.code, ErrorCode::Internal);
        assert!(
            error.message.contains("injected mark_running failure"),
            "{}",
            error.message
        );
    }

    #[test]
    fn a_linux_job_is_accepted_durably_and_ends_with_an_error_outcome_when_the_runtime_fails() {
        let _state = crate::test_support::global_state();
        let fixture = fixture(3);
        // The bundled runtime does not exist in this fixture, so the capture itself fails.
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let _entered = runtime.enter();
        let job_id = fixture
            .service
            .start_create(LINUX_ID, "First", None)
            .unwrap();
        assert!(job_id.as_str().starts_with("job:"));

        let finished = wait_finished(&fixture, &job_id);
        let outcome = finished.outcome.expect("a finished job has an outcome");
        assert!(outcome.result.is_none());
        assert_eq!(outcome.error.unwrap().code, ErrorCode::Internal);
        let events = fixture.engine.published.lock().unwrap();
        assert!(events
            .iter()
            .any(|event| event.name == "operations.job.finished"));
    }

    #[test]
    fn usage_reads_both_platforms_and_a_linux_computer_without_checkpoints_costs_nothing() {
        let _state = crate::test_support::global_state();
        let fixture = fixture(3);
        let linux = fixture.service.usage(LINUX_ID).unwrap();
        assert_eq!(linux.total_bytes, Some(0));
        assert!(linux.checkpoints.is_empty());

        let macos = fixture.service.usage(&fixture.macos_id).unwrap();
        assert_eq!(macos.total_bytes, None);
        assert_eq!(macos.checkpoints.len(), 1);
        assert_eq!(macos.checkpoints[0].id, fixture.macos_checkpoint);
        assert_eq!(macos.checkpoints[0].size_bytes, None);

        let missing = fixture
            .service
            .usage("22222222-2222-4222-8222-222222222222")
            .unwrap_err();
        assert_eq!(missing.code, ErrorCode::NotFound);
    }

    #[test]
    fn the_launch_upgrade_converts_linux_and_macos_records_and_waits_out_a_pending_migration() {
        let _state = crate::test_support::global_state();
        let fixture = fixture(3);
        let app_data = fixture.engine.paths.app_data.clone();
        // A layout-1 macOS record: no policy kind, schema version 1.
        let layout_one = |macos_id: &str, checkpoint_id: &str| {
            let file = app_data
                .join("macos-computers")
                .join(macos_id)
                .join("checkpoints")
                .join(checkpoint_id)
                .join("checkpoint.json");
            let mut value: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
            value["schemaVersion"] = json!(1);
            value.as_object_mut().unwrap().remove("policyKind");
            std::fs::write(&file, serde_json::to_vec(&value).unwrap()).unwrap();
            file
        };
        let macos_file = layout_one(&fixture.macos_id, &fixture.macos_checkpoint);
        // A layout-1 Linux record in the storage the normal runtime reads.
        let storage = app_data.join("runtime");
        std::fs::create_dir_all(storage.join("checkpoints")).unwrap();
        let linux_file = storage.join("checkpoints").join(format!("{LINUX_ID}.json"));
        std::fs::write(
            &linux_file,
            json!({"version": 1, "checkpoints": [{
                "id": "c000000000000000000000000000000", "name": "Old", "createdAt": 1,
                "scope": "disk", "reason": "manual"
            }], "pendingCheckpointRestore": null, "checkpointOperation": null})
            .to_string(),
        )
        .unwrap();

        // While the storage migration is pending, the Linux storage is the backup source and
        // stays byte for byte; the macOS records have no such constraint.
        std::fs::write(storage.join("computers.json"), b"{}").unwrap();
        let pending = crate::runtime_migration::Controller::new(app_data.clone());
        assert!(pending.runtime_storage().is_err());
        let before = std::fs::read(&linux_file).unwrap();
        upgrade_layouts(fixture.engine.as_ref(), &pending).unwrap();
        assert_eq!(std::fs::read(&linux_file).unwrap(), before);
        assert!(!std::fs::read_to_string(&macos_file)
            .unwrap()
            .contains("schemaVersion\": 1"));
        assert!(std::fs::read_to_string(&macos_file)
            .unwrap()
            .contains("policyKind"));
        assert_eq!(
            macos_computers::seed_check_policy(&app_data, &fixture.macos_id),
            vec!["manual".to_owned()]
        );

        // Once the migration is not required the Linux record is converted too.
        std::fs::remove_file(storage.join("computers.json")).unwrap();
        let ready = crate::runtime_migration::Controller::new(app_data.clone());
        assert!(ready.runtime_storage().is_ok());
        upgrade_layouts(fixture.engine.as_ref(), &ready).unwrap();
        let converted: Value =
            serde_json::from_slice(&std::fs::read(&linux_file).unwrap()).unwrap();
        assert_eq!(converted["version"], 2);
        assert_eq!(converted["checkpoints"][0]["kind"], "manual");
        // And running it again changes nothing.
        let settled = std::fs::read(&linux_file).unwrap();
        upgrade_layouts(fixture.engine.as_ref(), &ready).unwrap();
        assert_eq!(std::fs::read(&linux_file).unwrap(), settled);
    }

    #[test]
    fn engine_messages_map_to_the_canonical_codes() {
        let code = |message: &str| Failure::from_message(message).code;
        assert_eq!(code(AUTOMATIC_OFF), ErrorCode::PermissionDenied);
        assert_eq!(code("This computer no longer exists."), ErrorCode::NotFound);
        assert_eq!(
            code("A checkpoint operation is in progress on this computer. Wait for it to finish."),
            ErrorCode::Busy
        );
        assert_eq!(
            code("Silo is installing an update. Wait for it to finish."),
            ErrorCode::UpdateInProgress
        );
        assert_eq!(
            code("The checkpoint operation was cancelled."),
            ErrorCode::Cancelled
        );
        assert_eq!(code("The disk is full."), ErrorCode::Internal);
    }
}
