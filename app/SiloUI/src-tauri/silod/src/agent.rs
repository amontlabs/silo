//! The engine agent's startup and serving, shared by the run loops: AppKit's on macOS
//! (`app`) and a plain main-thread queue on Linux (`linux`). The order of the steps is the
//! engine's startup contract; each platform only adds its run loop around it.

use std::{
    path::PathBuf,
    sync::Arc,
    thread::{self, JoinHandle},
    time::Duration,
};

use silo_engine::context::{self, EngineContext, MainThread};
use tokio::{runtime::Builder, sync::oneshot};

use crate::{
    context::SilodContext,
    dispatcher::Dispatcher,
    identity,
    manifest::EngineManifest,
    ownership::{OwnerAcquisition, OwnerLease},
    paths::{ensure_private_directory, SilodPaths},
    process::AppProcessControl,
    socket::{EngineServer, SystemPeerEuidProvider},
};

/// What the engine holds once it owns this channel's state: the lock and version record are
/// established before the engine touches state or accepts connections.
pub(crate) struct Owned {
    pub(crate) paths: SilodPaths,
    pub(crate) local_device: identity::LocalDeviceIdentity,
    pub(crate) owner: OwnerLease,
    pub(crate) euid: u32,
    pub(crate) engine_version: String,
}

/// Selects the manifest's channel, secures the state directory and takes ownership of it.
/// `None` when another instance owns the channel: this one exits without unlinking anything.
pub(crate) fn acquire(
    manifest: &EngineManifest,
    resources: PathBuf,
) -> Result<Option<Owned>, String> {
    let channel = initialize_channel(manifest)?;
    let paths = SilodPaths::resolve(channel, resources)?;
    let euid = unsafe { libc::geteuid() };
    ensure_private_directory(&paths.state_dir, euid).map_err(|error| error.to_string())?;

    // The lock and version record are established before the engine touches state
    // or accepts connections. A losing instance exits without unlinking anything.
    let owner = OwnerLease::acquire(
        &paths,
        &manifest.engine_version,
        &manifest.engine_build,
        euid,
    )?;
    let owner = match owner {
        OwnerAcquisition::Acquired(owner) => owner,
        OwnerAcquisition::NotOwner(reason) => {
            eprintln!("silod: {reason}");
            return Ok(None);
        }
    };
    let local_device = identity::load_or_create(&paths.state_dir, euid)?;
    Ok(Some(Owned {
        paths,
        local_device,
        owner,
        euid,
        engine_version: manifest.engine_version.clone(),
    }))
}

pub(crate) fn initialize_channel(
    manifest: &EngineManifest,
) -> Result<silo_engine::channel::Channel, String> {
    let channel = manifest.channel()?;
    silo_engine::channel::init(&manifest.channel_identifier);
    Ok(channel)
}

/// The serving engine: the socket server runs on its own thread until [`Serving::finish`].
pub(crate) struct Serving {
    pub(crate) context: Arc<SilodContext>,
    shutdown_tx: oneshot::Sender<()>,
    server_thread: JoinHandle<()>,
    owner: OwnerLease,
}

impl Serving {
    /// Stops the socket server after the run loop returned, then releases ownership.
    pub(crate) fn finish(self) -> Result<(), String> {
        let _ = self.shutdown_tx.send(());
        self.server_thread
            .join()
            .map_err(|_| "The silod I/O runtime panicked".to_owned())?;
        drop(self.owner);
        Ok(())
    }
}

/// Builds the engine context, prepares stored data, binds the socket and starts the
/// background work. The caller then runs its main-thread loop until `process` exits.
pub(crate) fn serve(
    owned: Owned,
    main_thread: Arc<dyn MainThread>,
    process: Arc<AppProcessControl>,
) -> Result<Serving, String> {
    let Owned {
        paths,
        local_device,
        owner,
        euid,
        engine_version,
    } = owned;
    let context = Arc::new(SilodContext::new(
        paths.engine.clone(),
        paths.identity(&engine_version),
        local_device,
        main_thread,
        process,
    ));
    context.job_registry()?;
    context::install(context.clone())?;
    // Saved files are converted to the current vocabulary before anything reads them.
    crate::startup::prepare_stored_data()?;
    context.install_queue_events();
    // Checkpoint records written before automatic checkpoints existed become manual ones,
    // once, before anything reads them.
    silo_engine::checkpoints::upgrade_layouts(context.as_ref(), &context.migration_controller())?;
    // Computers made before the network bootstrap existed are converted once per launch. Start
    // and computer-changing work stay closed until the pass ends; the pass boots guests, so it
    // runs on its own thread and waits for the main-thread loop to start.
    silo_engine::macos_computers::begin_proxy_bootstrap_migration();
    start_proxy_bootstrap_migration(context.clone());
    // Resumes computers a previous engine update saved, before anything else advances them,
    // and starts the engine's own update schedule.
    let update_recovery = crate::methods::services::start(context.clone());
    // The engine watches its computers whether or not a client is connected (95 A-26).
    crate::methods::computers::install_health_watch(context.clone());
    crate::methods::access::install(context.clone())?;
    // A copy over Connections ended with the engine that served it; empty the area before the
    // bridge accepts the first stream, so a new copy is never swept.
    crate::timers::sweep_transfer_area(&context, true);
    crate::methods::access::start_connections(context.clone())?;

    let storage = silo_engine::runtime_migration::selected_runtime_storage(&paths.engine.app_data)?;
    let runtime_home =
        silo_engine::runtime::runtime_home_alias(&paths.engine.home, &storage.join("microsandbox"));
    silo_engine::host_push_operations::reconcile_jobs_before_serving(
        context.as_ref(),
        &runtime_home,
    )?;
    silo_engine::backup_controller::reconcile_jobs_before_serving(context.as_ref())?;
    // An approval allowed just before the engine stopped is ended, never run again on its own.
    crate::methods::approvals::recover(&context);

    let runtime = Builder::new_multi_thread()
        .enable_all()
        .thread_name("silod-io")
        .build()
        .map_err(|error| format!("Could not create Tokio runtime: {error}"))?;
    // `UnixListener::from_std` registers its fd with the current Tokio reactor.
    // Bind while the runtime is entered, even though the listener is later
    // driven on the dedicated I/O thread.
    let _runtime_context = runtime.enter();
    crate::methods::remote::install_runtime(runtime.handle().clone());
    silo_engine::changes::fork_base::install(context.clone());
    // The exports controller is installed at boot: it settles an interrupted operation's
    // journal before any client can start a new one.
    let exports = crate::startup::install_exports(&context);
    // On Linux the handlers are registered before the socket exists, so a signal sent once
    // a client can reach the engine always gets the session end, never the default exit.
    #[cfg(target_os = "linux")]
    let signals = ending_signals();
    let server = EngineServer::bind(
        paths.socket_path(),
        euid,
        Arc::new(SystemPeerEuidProvider),
        Arc::new(Dispatcher::core(context.clone())),
    )?;
    drop(_runtime_context);
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let signal_context = context.clone();
    let server_thread = thread::Builder::new()
        .name("silod-runtime".to_owned())
        .spawn(move || {
            // SIGTERM is a session end: no prompt, bounded, never cancelled. On Linux, where
            // silod also runs in a terminal, Ctrl-C (SIGINT) ends the session the same way;
            // the engine starts `msb` in its own process group there, so the terminal's
            // SIGINT reaches silod alone and the session end stops the computers in order.
            #[cfg(target_os = "linux")]
            for (signal, name) in signals {
                runtime.spawn(end_session_on_signal(signal, name, signal_context.clone()));
            }
            #[cfg(not(target_os = "linux"))]
            runtime.spawn(end_session_on_signal(
                tokio::signal::unix::SignalKind::terminate(),
                "SIGTERM",
                signal_context,
            ));
            runtime.block_on(server.run(shutdown_rx));
        })
        .map_err(|error| format!("Could not start the I/O runtime: {error}"))?;

    // Launch migration, computer-use pass, engine timers and the launch pass (05 §7).
    crate::startup::start_background(context.clone(), exports, update_recovery);

    Ok(Serving {
        context,
        shutdown_tx,
        server_thread,
        owner,
    })
}

/// Waits for the first `kind` signal, then ends the session through the engine's bounded
/// session-end shutdown, which exits the process when it is done.
#[cfg(not(target_os = "linux"))]
async fn end_session_on_signal(
    kind: tokio::signal::unix::SignalKind,
    name: &str,
    context: Arc<SilodContext>,
) {
    match tokio::signal::unix::signal(kind) {
        Ok(mut signal) => {
            if signal.recv().await.is_some() {
                crate::methods::services::session_end(context);
            }
        }
        Err(error) => eprintln!("silod: could not install {name} handler: {error}"),
    }
}

/// The first SIGINT or SIGTERM ends the session through the engine's bounded session-end
/// shutdown, which exits the process when it is done. The handlers keep listening: a second
/// signal of either kind (a second Ctrl-C in a terminal, or a supervisor that gave up
/// waiting) exits at once with status 1, leaving running computers to the next start.
#[cfg(target_os = "linux")]
async fn end_session_on_signal(
    mut signal: tokio::signal::unix::Signal,
    name: &'static str,
    context: Arc<SilodContext>,
) {
    use std::sync::atomic::AtomicBool;
    static SESSION_ENDING: AtomicBool = AtomicBool::new(false);
    while signal.recv().await.is_some() {
        match signal_action(&SESSION_ENDING) {
            SignalAction::EndSession => {
                eprintln!(
                    "silod: {name} received, ending the session (send it again to exit at once)"
                );
                crate::methods::services::session_end(context.clone());
            }
            SignalAction::ExitNow => {
                eprintln!("silod: {name} received again, exiting now");
                std::process::exit(1);
            }
        }
    }
}

/// SIGINT and SIGTERM, registered with the entered Tokio runtime. A signal that cannot be
/// registered is reported and keeps its default action.
#[cfg(target_os = "linux")]
fn ending_signals() -> Vec<(tokio::signal::unix::Signal, &'static str)> {
    use tokio::signal::unix::{signal, SignalKind};
    [
        (SignalKind::interrupt(), "SIGINT"),
        (SignalKind::terminate(), "SIGTERM"),
    ]
    .into_iter()
    .filter_map(|(kind, name)| match signal(kind) {
        Ok(signal) => Some((signal, name)),
        Err(error) => {
            eprintln!("silod: could not install {name} handler: {error}");
            None
        }
    })
    .collect()
}

/// What one ending signal does on Linux.
#[cfg(target_os = "linux")]
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SignalAction {
    /// The first: the bounded session end.
    EndSession,
    /// Any later one: exit at once.
    ExitNow,
}

/// The first call on `ending` (shared by the SIGINT and SIGTERM handlers) ends the session;
/// every later one exits.
#[cfg(target_os = "linux")]
pub(crate) fn signal_action(ending: &std::sync::atomic::AtomicBool) -> SignalAction {
    if ending.swap(true, std::sync::atomic::Ordering::SeqCst) {
        SignalAction::ExitNow
    } else {
        SignalAction::EndSession
    }
}

/// How long a launch migration that the Keychain refused waits before it tries again.
const MIGRATION_RETRY: Duration = Duration::from_secs(15 * 60);

/// What one run of the launch migration thread ended with.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum MigrationEnd {
    /// Every computer was visited and none failed.
    Completed,
    /// Some computers were left for later and the thread was asked to stop retrying.
    Skipped,
    /// A failure that waiting does not fix; computers without a bootstrap refuse Start.
    Failed(String),
}

/// Runs `pass` (the launch migration) until it completes. A pass the Keychain refused
/// (`credential_access_denied`) skips and runs again after `wait`, which returns false to stop.
/// macOS Start opens after the first pass whatever its result, so a refusal never holds the
/// launch closed.
pub(crate) fn drive_proxy_bootstrap_migration(
    mut pass: impl FnMut() -> Result<(), String>,
    mut wait: impl FnMut() -> bool,
) -> MigrationEnd {
    loop {
        let result = pass();
        silo_engine::macos_computers::end_proxy_bootstrap_migration();
        match result {
            Ok(()) => return MigrationEnd::Completed,
            Err(message) if silo_engine::credential_store::is_access_denied_message(&message) => {
                eprintln!("silod: the macOS network migration was skipped, the Keychain asked for approval: {message}");
                if !wait() {
                    return MigrationEnd::Skipped;
                }
            }
            Err(message) => return MigrationEnd::Failed(message),
        }
    }
}

/// Runs the launch migration of macOS computers that predate the proxy bootstrap.
fn start_proxy_bootstrap_migration(context: Arc<SilodContext>) {
    let spawned = thread::Builder::new()
        .name("silod-proxy-migration".to_owned())
        .spawn(move || {
            let end = drive_proxy_bootstrap_migration(
                || {
                    silo_engine::macos_computers::migrate_proxy_bootstraps_at_launch(
                        &context.macos_context(),
                    )
                },
                || {
                    thread::sleep(MIGRATION_RETRY);
                    true
                },
            );
            if let MigrationEnd::Failed(error) = end {
                eprintln!("silod: the macOS network migration reported: {error}");
            }
        });
    if let Err(error) = spawned {
        // Nothing converted: computers without a bootstrap stay not set up and refuse Start.
        eprintln!("silod: could not start the macOS network migration: {error}");
        silo_engine::macos_computers::end_proxy_bootstrap_migration();
    }
}

pub(crate) fn set_private_umask() {
    unsafe {
        libc::umask(0o077);
    }
}

#[cfg(test)]
mod channel_startup_tests {
    use std::{path::PathBuf, process::Command};

    use super::*;
    use crate::{manifest::EngineManifest, paths::SilodPaths};
    use silo_engine::{channel, context::EngineIdentity};

    const CHANNEL_IDENTIFIER_ENV: &str = "SILOD_TEST_CHANNEL_IDENTIFIER";

    #[test]
    fn channel_startup_subprocess() {
        let Ok(identifier) = std::env::var(CHANNEL_IDENTIFIER_ENV) else {
            return;
        };
        let manifest = EngineManifest {
            channel_identifier: identifier.clone(),
            engine_version: "0.1.0".to_owned(),
            engine_build: "test".to_owned(),
            protocol_min: 8,
            protocol_max: 8,
        };
        let channel = initialize_channel(&manifest).unwrap();
        let identity: EngineIdentity =
            SilodPaths::for_home(channel, PathBuf::from("/fixture-home"), PathBuf::new())
                .identity(&manifest.engine_version);

        assert_eq!(channel::current(), identity.channel);
        assert_eq!(identity.channel, manifest.channel().unwrap());
    }

    #[test]
    fn startup_initializes_the_global_channel_for_both_builds() {
        for identifier in [
            silo_engine::channel::PRODUCTION_IDENTIFIER,
            silo_engine::channel::DEVELOPMENT_IDENTIFIER,
        ] {
            let _spawning = crate::FORK_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let output = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "agent::channel_startup_tests::channel_startup_subprocess",
                    "--nocapture",
                ])
                .env(CHANNEL_IDENTIFIER_ENV, identifier)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "channel startup failed for {identifier}:\n{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn the_first_ending_signal_ends_the_session_and_any_later_one_exits() {
        let ending = std::sync::atomic::AtomicBool::new(false);
        assert_eq!(signal_action(&ending), SignalAction::EndSession);
        assert_eq!(signal_action(&ending), SignalAction::ExitNow);
        assert_eq!(signal_action(&ending), SignalAction::ExitNow);
    }

    /// The migration gate is process-wide: tests that open and close it take turns.
    static MIGRATION_GATE: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn the_launch_migration_runs_once_completes_and_opens_start() {
        let _gate = MIGRATION_GATE.lock().unwrap_or_else(|e| e.into_inner());
        silo_engine::macos_computers::begin_proxy_bootstrap_migration();
        let mut passes = 0;
        let end = drive_proxy_bootstrap_migration(
            || {
                passes += 1;
                Ok(())
            },
            || panic!("a completed migration does not wait"),
        );
        assert_eq!(end, MigrationEnd::Completed);
        assert_eq!(passes, 1);
        assert!(silo_engine::macos_computers::proxy_bootstrap_migration_ready());
    }

    #[test]
    fn a_keychain_refusal_skips_opens_start_and_retries_until_it_completes() {
        let _gate = MIGRATION_GATE.lock().unwrap_or_else(|e| e.into_inner());
        silo_engine::macos_computers::begin_proxy_bootstrap_migration();
        let mut passes = 0;
        let mut waits = 0;
        let end = drive_proxy_bootstrap_migration(
            || {
                passes += 1;
                if passes < 3 {
                    Err(format!(
                        "Computer 1: {}",
                        silo_engine::credential_store::ACCESS_DENIED_MESSAGE
                    ))
                } else {
                    Ok(())
                }
            },
            || {
                waits += 1;
                true
            },
        );
        assert_eq!(end, MigrationEnd::Completed);
        assert_eq!((passes, waits), (3, 2));
        assert!(silo_engine::macos_computers::proxy_bootstrap_migration_ready());
    }

    #[test]
    fn a_refused_migration_stops_when_asked_to_and_other_failures_do_not_retry() {
        let _gate = MIGRATION_GATE.lock().unwrap_or_else(|e| e.into_inner());
        silo_engine::macos_computers::begin_proxy_bootstrap_migration();
        let denied = silo_engine::credential_store::ACCESS_DENIED_MESSAGE.to_owned();
        let end = drive_proxy_bootstrap_migration(|| Err(denied.clone()), || false);
        assert_eq!(end, MigrationEnd::Skipped);
        assert!(silo_engine::macos_computers::proxy_bootstrap_migration_ready());

        let mut passes = 0;
        let end = drive_proxy_bootstrap_migration(
            || {
                passes += 1;
                Err("disk full".to_owned())
            },
            || panic!("only a refusal waits"),
        );
        assert_eq!(end, MigrationEnd::Failed("disk full".to_owned()));
        assert_eq!(passes, 1);
    }
}
