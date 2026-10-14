use std::{
    ffi::c_void,
    sync::{
        atomic::{AtomicI32, Ordering},
        Arc,
    },
    thread,
};

use objc2::{rc::autoreleasepool, MainThreadMarker};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSEvent, NSEventModifierFlags, NSEventType,
};
use silo_engine::context::{self, EngineContext, MainThread, ProcessControl};
use tokio::{runtime::Builder, sync::oneshot};

use crate::{
    context::SilodContext,
    dispatcher::Dispatcher,
    identity,
    ownership::{OwnerAcquisition, OwnerLease},
    paths::{default_resource_dir, ensure_private_directory, SilodPaths},
    service,
    socket::{EngineServer, SystemPeerEuidProvider},
};

const RESTART_EXIT_CODE: i32 = 75;

/// Runs the alias ProxyCommand helper from the bundled silod binary. The bundle manifest
/// selects its channel before the engine-socket client reads channel-specific state.
pub fn run_remote_guest(args: &[String]) -> Result<i32, String> {
    autoreleasepool(|_| {
        if args.len() != 2 && args.len() != 3 {
            return Err("usage: silod --remote-guest DEVICE_ID COMPUTER_ID [PORT]".to_owned());
        }
        let manifest = service::current_bundle_manifest()?;
        silo_engine::channel::init(&manifest.channel_identifier);
        let params = silo_engine::remote::guest_stream_params(args)?;
        silo_engine::remote::run_remote_stream(&args[0], "guest.ssh", params)?;
        Ok(0)
    })
}

pub fn run_agent() -> Result<i32, String> {
    set_private_umask();
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let resources = default_resource_dir(&executable)?;
    let manifest = service::current_bundle_manifest()?;
    let channel = initialize_channel(&manifest)?;
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
            return Ok(0);
        }
    };
    let local_device = identity::load_or_create(&paths.state_dir, euid)?;

    let engine_version = manifest.engine_version;
    autoreleasepool(|_| run_appkit(paths, local_device, owner, euid, engine_version))
}

fn initialize_channel(
    manifest: &crate::manifest::EngineManifest,
) -> Result<silo_engine::channel::Channel, String> {
    let channel = manifest.channel()?;
    silo_engine::channel::init(&manifest.channel_identifier);
    Ok(channel)
}

fn run_appkit(
    paths: SilodPaths,
    local_device: identity::LocalDeviceIdentity,
    owner: OwnerLease,
    euid: u32,
    engine_version: String,
) -> Result<i32, String> {
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| "silod must initialize AppKit on the process main thread".to_owned())?;
    let application = NSApplication::sharedApplication(mtm);
    if !application.setActivationPolicy(NSApplicationActivationPolicy::Accessory) {
        return Err("NSApplication refused accessory activation policy".to_owned());
    }

    let process = Arc::new(AppProcessControl::new(|| {
        dispatch_to_main(Box::new(|| {
            if let Some(mtm) = MainThreadMarker::new() {
                let application = NSApplication::sharedApplication(mtm);
                application.stop(None);
                if let Some(event) = NSEvent::otherEventWithType_location_modifierFlags_timestamp_windowNumber_context_subtype_data1_data2(
                    NSEventType::ApplicationDefined,
                    objc2_foundation::NSPoint::new(0.0, 0.0),
                    NSEventModifierFlags::empty(),
                    0.0,
                    0,
                    None,
                    0,
                    0,
                    0,
                ) {
                    application.postEvent_atStart(&event, true);
                } else {
                    eprintln!("silod: could not create the AppKit loop-stop event");
                }
            }
        }))
        .unwrap_or_else(|error| eprintln!("silod: could not schedule AppKit shutdown: {error}"));
    }));
    let context = Arc::new(SilodContext::new(
        paths.engine.clone(),
        paths.identity(&engine_version),
        local_device,
        Arc::new(AppKitMainThread),
        process.clone(),
    ));
    context.job_registry()?;
    context::install(context.clone())?;
    context.install_queue_events();
    // The engine watches its computers whether or not a client is connected (95 A-26).
    crate::methods::computers::install_health_watch(context.clone());

    let storage = silo_engine::runtime_migration::selected_runtime_storage(&paths.engine.app_data)?;
    let runtime_home =
        silo_engine::runtime::runtime_home_alias(&paths.engine.home, &storage.join("microsandbox"));
    silo_engine::host_push_operations::reconcile_jobs_before_serving(
        context.as_ref(),
        &runtime_home,
    )?;
    silo_engine::backup_controller::reconcile_jobs_before_serving(context.as_ref())?;

    let runtime = Builder::new_multi_thread()
        .enable_all()
        .thread_name("silod-io")
        .build()
        .map_err(|error| format!("Could not create Tokio runtime: {error}"))?;
    // `UnixListener::from_std` registers its fd with the current Tokio reactor.
    // Bind while the runtime is entered, even though the listener is later
    // driven on the dedicated I/O thread.
    let _runtime_context = runtime.enter();
    let server = EngineServer::bind(
        paths.socket_path(),
        euid,
        Arc::new(SystemPeerEuidProvider),
        Arc::new(Dispatcher::core(context)),
    )?;
    drop(_runtime_context);
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let signal_process = process.clone();
    let server_thread = thread::Builder::new()
        .name("silod-runtime".to_owned())
        .spawn(move || {
            runtime.spawn(async move {
                match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                    Ok(mut terminate) => {
                        if terminate.recv().await.is_some() {
                            signal_process.exit(0);
                        }
                    }
                    Err(error) => eprintln!("silod: could not install SIGTERM handler: {error}"),
                }
            });
            runtime.block_on(server.run(shutdown_rx));
        })
        .map_err(|error| format!("Could not start the I/O runtime: {error}"))?;

    application.run();
    let exit_code = process.exit_code.load(Ordering::SeqCst);
    let _ = shutdown_tx.send(());
    server_thread
        .join()
        .map_err(|_| "The silod I/O runtime panicked".to_owned())?;
    drop(owner);
    Ok(exit_code)
}

fn set_private_umask() {
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
            protocol_min: 6,
            protocol_max: 6,
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
            let output = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "app::channel_startup_tests::channel_startup_subprocess",
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

pub struct AppProcessControl {
    exit_code: AtomicI32,
    request_appkit_stop: Box<dyn Fn() + Send + Sync>,
}

impl AppProcessControl {
    fn new(request_appkit_stop: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            exit_code: AtomicI32::new(0),
            request_appkit_stop: Box::new(request_appkit_stop),
        }
    }
}

impl ProcessControl for AppProcessControl {
    fn exit(&self, code: i32) {
        self.exit_code.store(code, Ordering::SeqCst);
        (self.request_appkit_stop)();
    }

    fn restart(&self) -> ! {
        self.exit(RESTART_EXIT_CODE);
        // ProcessControl's non-returning contract is used from engine worker
        // tasks. Unwind that task after scheduling AppKit shutdown; parking the
        // Tokio worker would prevent the server from draining and releasing its
        // socket and ownership lock.
        std::panic::resume_unwind(Box::new(RestartScheduled))
    }
}

#[derive(Debug)]
pub struct RestartScheduled;

pub struct AppKitMainThread;

impl MainThread for AppKitMainThread {
    fn run(&self, work: Box<dyn FnOnce() + Send>) -> Result<(), String> {
        dispatch_to_main(work)
    }
}

struct ScheduledWork(Option<Box<dyn FnOnce() + Send>>);

unsafe extern "C" fn invoke_scheduled_work(context: *mut c_void) {
    let mut scheduled = unsafe { Box::from_raw(context.cast::<ScheduledWork>()) };
    if let Some(work) = scheduled.0.take() {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work));
    }
}

#[link(name = "System", kind = "dylib")]
unsafe extern "C" {
    #[link_name = "_dispatch_main_q"]
    static DISPATCH_MAIN_QUEUE: c_void;
    fn dispatch_async_f(
        queue: *mut c_void,
        context: *mut c_void,
        work: unsafe extern "C" fn(*mut c_void),
    );
}

fn dispatch_to_main(work: Box<dyn FnOnce() + Send>) -> Result<(), String> {
    let context = Box::into_raw(Box::new(ScheduledWork(Some(work)))).cast::<c_void>();
    unsafe {
        // dispatch_get_main_queue() is an SDK inline function that returns this
        // exported queue object, so bind the object directly here.
        let queue = std::ptr::addr_of!(DISPATCH_MAIN_QUEUE).cast_mut();
        dispatch_async_f(queue, context, invoke_scheduled_work);
    }
    Ok(())
}

/// Tracks engine-owned VM/setup windows. Call these methods on the AppKit main
/// thread before showing the first window and after the last window closes.
pub struct WindowPolicy {
    open_windows: usize,
}

impl WindowPolicy {
    pub const fn new() -> Self {
        Self { open_windows: 0 }
    }

    pub fn window_opened(&mut self, mtm: MainThreadMarker) -> Result<(), String> {
        self.open_windows = self.open_windows.saturating_add(1);
        self.apply(mtm)
    }

    pub fn window_closed(&mut self, mtm: MainThreadMarker) -> Result<(), String> {
        self.open_windows = self.open_windows.saturating_sub(1);
        self.apply(mtm)
    }

    pub fn open_window_count(&self) -> usize {
        self.open_windows
    }

    fn apply(&self, mtm: MainThreadMarker) -> Result<(), String> {
        let policy = activation_policy_for_window_count(self.open_windows);
        let application = NSApplication::sharedApplication(mtm);
        if application.activationPolicy() == policy {
            return Ok(());
        }
        if application.setActivationPolicy(policy) {
            Ok(())
        } else {
            Err(format!(
                "NSApplication refused activation policy {policy:?}"
            ))
        }
    }
}

fn activation_policy_for_window_count(open_windows: usize) -> NSApplicationActivationPolicy {
    if open_windows == 0 {
        NSApplicationActivationPolicy::Accessory
    } else {
        NSApplicationActivationPolicy::Regular
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn window_count_selects_regular_only_while_a_window_is_open() {
        let mut windows = WindowPolicy::new();
        assert_eq!(windows.open_window_count(), 0);
        assert_eq!(
            activation_policy_for_window_count(windows.open_window_count()),
            NSApplicationActivationPolicy::Accessory
        );
        windows.open_windows = 1;
        assert_eq!(
            activation_policy_for_window_count(windows.open_window_count()),
            NSApplicationActivationPolicy::Regular
        );
        windows.open_windows = 0;
        assert_eq!(
            activation_policy_for_window_count(windows.open_window_count()),
            NSApplicationActivationPolicy::Accessory
        );
    }

    #[tokio::test]
    async fn restart_unwinds_the_worker_after_scheduling_appkit_exit() {
        let requested = Arc::new(AtomicUsize::new(0));
        let requested_in_callback = requested.clone();
        let process = Arc::new(AppProcessControl::new(move || {
            requested_in_callback.fetch_add(1, Ordering::SeqCst);
        }));
        let worker_process = process.clone();

        let worker = tokio::spawn(async move {
            worker_process.restart();
        });
        let error = worker.await.unwrap_err();

        assert!(error.is_panic());
        assert_eq!(process.exit_code.load(Ordering::SeqCst), RESTART_EXIT_CODE);
        assert_eq!(requested.load(Ordering::SeqCst), 1);
    }
}
