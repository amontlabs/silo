//! What silod runs at start, besides serving (05 §7). It is what Tauri's `setup` ran, minus
//! the UI.
//!
//! * `prepare_stored_data` runs before anything reads saved files: the vocabulary conversion
//!   and the pre-upgrade backup's expiry timer.
//! * `start_background` runs once the I/O runtime exists: it installs the exports controller
//!   (which settles an interrupted operation's journal), drives a pending launch migration
//!   (publishing `migration.changed`), starts the ChatGPT app worker, the engine timers
//!   (95 A-26) and the launch pass.
//! * The launch pass is `silo_engine::startup::install` over silod's `StartupServices`:
//!   recoveries in order, the preparation worker, then start-at-launch for Linux and macOS
//!   computers, with the last pass recorded for `computers.launch`.

use std::{
    path::PathBuf,
    sync::{atomic::AtomicBool, Arc, Mutex},
    time::Duration,
};

use serde_json::{Map, Value};
use silo_engine::{
    backup_controller,
    context::EngineContext,
    runtime::{self, RuntimeHost},
    startup::{RecoveredActions, StartupServices},
};

use crate::{
    context::SilodContext,
    methods::{
        checkpoints::preparation::{ready_hook, status_publisher},
        computers::SilodRuntimeHost,
        services::UpdateRecoveryOutcome,
    },
};

/// Pause before a restart after a successful launch migration, so its last
/// `migration.changed` leaves first.
const RESTART_AFTER_MIGRATION: Duration = Duration::from_millis(250);

/// Early start work that must finish before any saved file is read.
pub fn prepare_stored_data() -> Result<(), String> {
    silo_engine::runtime_migration::vocabulary::run()?;
    silo_engine::pre_upgrade_backup::install();
    // macOS Start and computer-changing work stay closed until the launch pass has
    // converted the legacy guest bootstrap (`migrate_macos_proxy_bootstraps`).
    silo_engine::macos_computers::begin_proxy_bootstrap_migration();
    Ok(())
}

/// Installs the exports controller at boot instead of on first use. Needs a Tokio runtime
/// entered on the calling thread. A failure is reported and leaves the controller to be
/// retried by its first caller.
pub fn install_exports(context: &SilodContext) -> Option<Arc<backup_controller::Controller>> {
    match context.backup_controller() {
        Ok(controller) => Some(controller),
        Err(error) => {
            eprintln!("silod: could not install the exports controller: {error}");
            None
        }
    }
}

/// Starts the background start work. `exports` is the controller `install_exports` returned.
pub fn start_background(
    context: Arc<SilodContext>,
    exports: Option<Arc<backup_controller::Controller>>,
    update_recovery: Arc<UpdateRecoveryOutcome>,
) {
    drive_launch_migration(context.clone(), exports.clone());
    start_chatgpt(context.clone());
    // The timers are the engine's own upkeep with no client connected (95 A-26).
    crate::timers::start(context.clone());
    let services = Arc::new(SilodStartupServices::new(
        context.clone(),
        exports,
        update_recovery,
    ));
    let state = context.runtime_state().startup().clone();
    let spawned = std::thread::Builder::new()
        .name("silod-launch-pass".into())
        .spawn(move || silo_engine::startup::install(state, services));
    if let Err(error) = spawned {
        eprintln!("silod: could not start the launch pass: {error}");
    }
}

// MARK: Launch migration

/// Runs the storage conversion a previous scan left pending, in the background, then restarts
/// the engine into the converted generation. `migration.changed` is published on every step.
fn drive_launch_migration(
    context: Arc<SilodContext>,
    exports: Option<Arc<backup_controller::Controller>>,
) {
    let migration = context.migration_controller();
    if migration.snapshot().map(|state| state.status) != Ok("scanning".to_owned()) {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("silod-launch-migration".into())
        .spawn(move || run_launch_migration(&context, &migration, exports.as_deref()));
    if let Err(error) = spawned {
        eprintln!("silod: could not start the launch migration: {error}");
    }
}

fn run_launch_migration(
    context: &Arc<SilodContext>,
    migration: &silo_engine::runtime_migration::Controller,
    exports: Option<&backup_controller::Controller>,
) {
    let host = SilodRuntimeHost::for_engine(context.clone());
    let result =
        silo_engine::runtime_migration::drive_pending(context.as_ref(), migration, |progress| {
            let paths = runtime::migration_runtime_paths(&host, &migration.staged_storage())?;
            if migration.needs_recovery()? {
                let exports = exports.ok_or("Export and import status could not be read.")?;
                backup_controller::wait_for_migration_recovery(exports)?;
            }
            migration.convert_with_progress(&runtime::ProcessRunner, &paths, progress)
        });
    match result {
        Ok(silo_engine::runtime_migration::LaunchDrive::Converted) => {
            std::thread::sleep(RESTART_AFTER_MIGRATION);
            context.process().exit(crate::app::RESTART_EXIT_CODE);
        }
        Ok(silo_engine::runtime_migration::LaunchDrive::Nothing) => {}
        Err(error) => eprintln!("silod: the launch migration failed: {error}"),
    }
}

// MARK: ChatGPT app

/// Registers the published ChatGPT app folder and prepares the app in the background, as the
/// app's computer-use setup did: a status check, then a download unless it is ready.
fn start_chatgpt(context: Arc<SilodContext>) {
    let root = context.paths().app_data.join("chatgpt");
    if let Err(error) = silo_engine::computer_use::register_published(&root) {
        eprintln!(
            "silod: the ChatGPT app folder is unavailable: {}",
            error.message
        );
    }
    let host: Arc<dyn RuntimeHost> = Arc::new(SilodRuntimeHost::for_engine(context.clone()));
    let spawned = std::thread::Builder::new()
        .name("silod-chatgpt-start".into())
        .spawn(move || chatgpt_start_pass(context, host, root));
    if let Err(error) = spawned {
        eprintln!("silod: could not start the ChatGPT app preparation: {error}");
    }
}

fn chatgpt_start_pass(context: Arc<SilodContext>, host: Arc<dyn RuntimeHost>, root: PathBuf) {
    use silo_engine::chatgpt_app;
    let engine: Arc<dyn EngineContext> = context;
    let publisher = status_publisher(engine);
    let status = chatgpt_app::refresh_status_blocking(&root, &publisher);
    let idle_host = host.clone();
    chatgpt_app::collect_unused(
        root.clone(),
        Arc::new(move || {
            runtime::update_recovery::running_names(idle_host.as_ref())
                .is_ok_and(|names| names.is_empty())
        }),
    );
    if matches!(status, chatgpt_app::Status::Ready { .. }) {
        // Finish approval changes saved but never applied before the last quit.
        reconcile_computer_use(host.as_ref());
        return;
    }
    // Let the engine finish starting first; this is not urgent.
    std::thread::sleep(chatgpt_app::auto::START_DELAY);
    chatgpt_app::ensure_in_background(root.clone(), publisher, ready_hook(host, root));
}

fn reconcile_computer_use(host: &dyn RuntimeHost) {
    let Ok(paths) = runtime::runtime_paths(host) else {
        return;
    };
    let Ok(running) = runtime::update_recovery::running_names(host) else {
        return;
    };
    let runner: silo_engine::computer_use::SharedRunner = Arc::new(runtime::ProcessRunner);
    let handles =
        silo_engine::computer_use::reconcile_in(&runtime::OPERATIONS, &runner, &paths, &running);
    if handles.is_empty() {
        return;
    }
    std::thread::spawn(move || {
        for handle in handles {
            let _ = handle.join();
        }
        silo_engine::computer_use::publish_application_state_changed();
    });
}

// MARK: Launch pass

/// silod's host services for the launch pass.
pub struct SilodStartupServices {
    context: Arc<SilodContext>,
    host: SilodRuntimeHost,
    exports: Option<Arc<backup_controller::Controller>>,
    update_recovery: Arc<UpdateRecoveryOutcome>,
    /// Whether an engine update's saved set was resumed (known once recovery settled).
    resumed_saved_set: Mutex<bool>,
}

impl SilodStartupServices {
    pub fn new(
        context: Arc<SilodContext>,
        exports: Option<Arc<backup_controller::Controller>>,
        update_recovery: Arc<UpdateRecoveryOutcome>,
    ) -> Self {
        Self {
            host: SilodRuntimeHost::for_engine(context.clone()),
            context,
            exports,
            update_recovery,
            resumed_saved_set: Mutex::new(false),
        }
    }

    fn state(&self) -> Arc<silo_engine::startup::StartupState> {
        self.context.runtime_state().startup().clone()
    }
}

impl StartupServices for SilodStartupServices {
    fn blocks_runtime_operations(&self) -> bool {
        self.host.blocks_operations()
    }

    /// The engine update's recovery goes first (it resumes what the update saved, before
    /// anything else advances a computer), then the export/import journal settles.
    fn wait_for_recovery(&self) -> Result<(), String> {
        let resumed = self.update_recovery.wait();
        *self
            .resumed_saved_set
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = resumed;
        let exports = self
            .exports
            .as_ref()
            .ok_or("Export and import status could not be read. Relaunch Silo and retry.")?;
        let state = self.state();
        backup_controller::wait_for_recovery(exports, &|| state.is_cancelled())
    }

    fn prepare_runtime(&self) {
        if let Ok(paths) = runtime::runtime_paths(&self.host) {
            let storage = paths.storage_home.as_deref().unwrap_or(&paths.home);
            if let Err(message) = runtime::image_cache::repair(&storage.join("cache")) {
                eprintln!("Image cache repair: {message}");
            }
            // The guest image import and the LCU download run on their own threads.
            silo_engine::preparation::start_with_paths(paths);
        }
    }

    fn recover_configuration(&self) -> Result<(), String> {
        runtime::configuration_recovery::recover(&self.host)
    }

    fn migrate_macos_proxy_bootstraps(&self) -> Result<(), String> {
        silo_engine::macos_computers::migrate_proxy_bootstraps_at_launch(&self.host.macos_context())
    }

    fn recover_lifecycle(&self) -> Result<RecoveredActions, String> {
        runtime::lifecycle_recovery::recover(&self.host).map(|recovered| RecoveredActions {
            keep_stopped: recovered.keep_stopped,
            failures: recovered.failures,
        })
    }

    /// True when an engine update had saved a set of computers: the launch pass is then
    /// skipped, so the running set stays exactly what it was before the update.
    fn recover_update(&self) -> Result<bool, String> {
        Ok(*self
            .resumed_saved_set
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner))
    }

    fn current_launch_settings(&self) -> Result<(Map<String, Value>, PathBuf), String> {
        let settings = silo_engine::settings::current_settings()?;
        let paths = runtime::runtime_paths(&self.host)?;
        Ok((settings, paths.metadata))
    }

    fn macos_computer_ids(&self) -> Vec<String> {
        silo_engine::macos_computers::computer_ids(&self.host.macos_context())
    }

    fn start_at_launch(&self, id: &str, cancelled: &AtomicBool) -> Result<(), String> {
        if silo_engine::macos_computers::start_at_launch(&self.host.macos_context(), id, cancelled)?
        {
            Ok(())
        } else {
            runtime::start_at_launch(&self.host, id)
        }
    }

    /// The failure was already retained and shown by the update recovery itself.
    fn update_recovery_failed(&self, message: String) {
        eprintln!("silod: update recovery failed: {message}");
    }

    fn notify_failure(&self, key: &str, title: &str, body: &str) {
        self.host
            .notify(silo_engine::notifications::failure(key, title, body, None));
    }

    /// No window to show it in: the retained notice is what a client shows on connect.
    fn show_integration_error(&self, message: String) {
        eprintln!("silod: {message}");
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use silo_engine::{
        channel::Channel,
        computer_use::Approval,
        context::{EngineIdentity, EnginePaths, MainThread, PackageKind, ProcessControl},
        macos_computers_engine::{install_test_computer, TestComputerState},
    };

    use super::*;
    use crate::identity::LocalDeviceIdentity;

    struct NoMain;
    impl MainThread for NoMain {
        fn run(&self, _work: Box<dyn FnOnce() + Send>) -> Result<(), String> {
            Ok(())
        }
    }

    struct NoProcess;
    impl ProcessControl for NoProcess {
        fn exit(&self, _code: i32) {}
        fn restart(&self) -> ! {
            panic!("restart is not expected")
        }
    }

    struct Fixture {
        _gate: crate::methods::ProcessGateLease,
        _registry: std::sync::MutexGuard<'static, ()>,
        directory: tempfile::TempDir,
        context: Arc<SilodContext>,
    }

    fn fixture() -> Fixture {
        let gate = crate::methods::process_gate_lock();
        let registry = crate::methods::MACOS_REGISTRY_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let context = Arc::new(SilodContext::new(
            EnginePaths {
                home: root.join("home"),
                app_data: root.join("data"),
                resources: root.join("resources"),
                ..EnginePaths::default()
            },
            EngineIdentity {
                channel: Channel::Development,
                version: "0.1.0".into(),
                package_kind: PackageKind::MacApp,
            },
            LocalDeviceIdentity {
                device_id: "local-device".into(),
                name: "Local Mac".into(),
            },
            Arc::new(NoMain),
            Arc::new(NoProcess),
        ));
        Fixture {
            _gate: gate,
            _registry: registry,
            directory,
            context,
        }
    }

    fn services(
        fixture: &Fixture,
        exports: Option<Arc<backup_controller::Controller>>,
        outcome: Arc<UpdateRecoveryOutcome>,
    ) -> SilodStartupServices {
        SilodStartupServices::new(fixture.context.clone(), exports, outcome)
    }

    #[tokio::test]
    async fn the_exports_controller_is_installed_at_boot_and_shared_afterwards() {
        let fixture = fixture();
        let installed = install_exports(&fixture.context).expect("the controller installs");
        let again = fixture.context.backup_controller().unwrap();
        assert!(Arc::ptr_eq(&installed, &again));
    }

    #[test]
    fn the_exports_controller_needs_a_runtime_and_a_failure_is_not_fatal() {
        let fixture = fixture();
        assert!(install_exports(&fixture.context).is_none());
    }

    #[test]
    fn the_update_recovery_wait_returns_what_the_recovery_settled() {
        let outcome = Arc::new(UpdateRecoveryOutcome::default());
        let waiter = {
            let outcome = outcome.clone();
            std::thread::spawn(move || outcome.wait())
        };
        std::thread::sleep(Duration::from_millis(50));
        assert!(
            !waiter.is_finished(),
            "the wait blocks until the recovery settles"
        );
        outcome.settle(true);
        assert!(waiter.join().unwrap());
        assert!(!UpdateRecoveryOutcome::settled(false).wait());
    }

    #[tokio::test]
    async fn the_launch_pass_is_skipped_when_an_update_set_was_resumed() {
        let fixture = fixture();
        let exports = install_exports(&fixture.context);
        for resumed in [false, true] {
            let services = services(
                &fixture,
                exports.clone(),
                UpdateRecoveryOutcome::settled(resumed),
            );
            let waited = tokio::task::spawn_blocking(move || {
                let waited = services.wait_for_recovery();
                (waited, services.recover_update())
            })
            .await
            .unwrap();
            assert_eq!(waited.0, Ok(()));
            assert_eq!(waited.1, Ok(resumed));
        }
    }

    #[test]
    fn recovery_is_refused_when_the_exports_controller_is_missing() {
        let fixture = fixture();
        let services = services(&fixture, None, UpdateRecoveryOutcome::settled(false));
        assert!(services.wait_for_recovery().is_err());
    }

    #[test]
    fn a_fresh_engine_does_not_block_the_launch_pass() {
        let fixture = fixture();
        let services = services(&fixture, None, UpdateRecoveryOutcome::settled(false));
        assert!(!services.blocks_runtime_operations());
    }

    #[test]
    fn the_launch_selection_covers_macos_computers_next_to_linux_ones() {
        let fixture = fixture();
        let id = install_test_computer(
            &fixture.directory.path().join("data"),
            "mac-launch",
            TestComputerState::Stopped,
            Approval::Ask,
        );
        let services = services(&fixture, None, UpdateRecoveryOutcome::settled(false));
        assert!(services.macos_computer_ids().contains(&id));
    }

    #[test]
    fn a_launch_start_of_an_unknown_computer_fails_without_starting_anything() {
        let fixture = fixture();
        let services = services(&fixture, None, UpdateRecoveryOutcome::settled(false));
        let cancelled = AtomicBool::new(false);
        assert!(services
            .start_at_launch("00000000-0000-4000-8000-0000000000ff", &cancelled)
            .is_err());
    }

    #[test]
    fn a_cancelled_launch_start_never_boots_a_macos_computer() {
        let fixture = fixture();
        let id = install_test_computer(
            &fixture.directory.path().join("data"),
            "mac-cancelled",
            TestComputerState::Stopped,
            Approval::Ask,
        );
        let services = services(&fixture, None, UpdateRecoveryOutcome::settled(false));
        let cancelled = AtomicBool::new(true);
        // Cancelled before its turn: the macOS start returns without booting, and the
        // fallback finds no Linux computer of that id.
        let _ = services.start_at_launch(&id, &cancelled);
        cancelled.store(false, Ordering::SeqCst);
        let state = silo_engine::macos_computers::computer_ids(&fixture.context.macos_context());
        assert!(state.contains(&id));
    }

    #[test]
    fn a_failure_of_the_launch_pass_is_retained_for_the_next_client() {
        let fixture = fixture();
        let services = services(&fixture, None, UpdateRecoveryOutcome::settled(false));
        services.notify_failure(
            "startup:launch",
            "A computer couldn\u{2019}t start",
            "dev: no",
        );
        let notices = silo_engine::notices::list(fixture.context.as_ref()).unwrap();
        assert!(notices.iter().any(|notice| notice.key == "startup:launch"));
    }
}
