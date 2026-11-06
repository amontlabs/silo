//! The engine agent on a Linux host: no AppKit, no bundle. The process main thread runs a
//! plain work queue in place of the AppKit loop until the engine exits, and identity comes
//! from the manifest beside the executable (`paths::default_resource_dir`).

use std::{
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{mpsc, Arc},
};

use silo_engine::context::MainThread;

use crate::{
    agent,
    manifest::EngineManifest,
    paths::{default_resource_dir, SilodPaths},
    process::AppProcessControl,
};

/// `SILO_CREDENTIAL_STORE`: `file` (the default) keeps credentials in the engine's file store;
/// `secret-service` uses the desktop Secret Service through `keyring`'s own default.
pub const CREDENTIAL_STORE_ENV: &str = "SILO_CREDENTIAL_STORE";

/// The manifest beside this executable. Validated as strictly as a bundle's, without the
/// bundle identifier and code signature that a Linux engine does not have.
pub fn current_manifest() -> Result<EngineManifest, String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    EngineManifest::load_unbundled(&default_resource_dir(&executable)?)
}

/// Runs the alias ProxyCommand helper. The manifest selects its channel before the
/// engine-socket client reads channel-specific state.
pub fn run_remote_guest(args: &[String]) -> Result<i32, String> {
    if args.len() != 2 && args.len() != 3 {
        return Err("usage: silod --remote-guest DEVICE_ID COMPUTER_ID [PORT]".to_owned());
    }
    crate::run_from_bundle(&std::env::args().collect::<Vec<_>>())?;
    let manifest = current_manifest()?;
    silo_engine::channel::init(&manifest.channel_identifier);
    let params = silo_engine::remote::guest_stream_params(args)?;
    silo_engine::remote::run_remote_stream(&args[0], "guest.ssh", params)?;
    Ok(0)
}

/// Runs the Connections bridge that a controller's forced SSH command starts through the
/// `~/.local/bin` link. It relays one request to this channel's `control.sock` and writes
/// nothing to stdout before the reply, which the controller reads as a framed stream.
pub fn run_remote_bridge(args: &[String]) -> Result<i32, String> {
    if !args.is_empty() {
        return Err("usage: silod --remote-bridge".to_owned());
    }
    crate::run_from_bundle(&std::env::args().collect::<Vec<_>>())?;
    let manifest = current_manifest()?;
    silo_engine::channel::init(&manifest.channel_identifier);
    silo_engine::remote::run_bridge()?;
    Ok(0)
}

pub fn run_agent() -> Result<i32, String> {
    agent::set_private_umask();
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let resources = default_resource_dir(&executable)?;
    let manifest = EngineManifest::load_unbundled(&resources)?;
    let Some(owned) = agent::acquire(&manifest, resources)? else {
        return Ok(0);
    };
    install_credential_store(
        &owned.paths,
        std::env::var(CREDENTIAL_STORE_ENV).ok().as_deref(),
    )?;

    let (sender, queue) = mpsc::channel();
    let process = Arc::new(AppProcessControl::new({
        let sender = sender.clone();
        move || {
            let _ = sender.send(MainWork::Stop);
        }
    }));
    let serving = agent::serve(owned, Arc::new(QueueMainThread { sender }), process.clone())?;
    run_main_queue(&queue);
    let exit_code = process.exit_code();
    serving.finish()?;
    Ok(exit_code)
}

/// Where the file credential store keeps its files: beside the engine's data.
pub fn credential_directory(paths: &SilodPaths) -> std::path::PathBuf {
    paths.engine.app_data.join("credentials")
}

/// Every `keyring::Entry` the engine makes uses the file store unless the Secret Service was
/// chosen. A server has no desktop session for the Secret Service, and choosing per start
/// whether one answers would split credentials between two stores.
fn install_credential_store(paths: &SilodPaths, choice: Option<&str>) -> Result<(), String> {
    match file_store_root(paths, choice)? {
        Some(root) => silo_engine::credential_file_store::install(root),
        None => Ok(()),
    }
}

/// The file store's directory, or `None` when the Secret Service was chosen.
fn file_store_root(
    paths: &SilodPaths,
    choice: Option<&str>,
) -> Result<Option<std::path::PathBuf>, String> {
    match choice {
        None | Some("") | Some("file") => Ok(Some(credential_directory(paths))),
        Some("secret-service") => Ok(None),
        Some(other) => Err(format!(
            "{CREDENTIAL_STORE_ENV} must be file or secret-service, not {other}"
        )),
    }
}

enum MainWork {
    Run(Box<dyn FnOnce() + Send>),
    Stop,
}

/// The engine's main thread on Linux: work runs, one item at a time, on the thread that
/// called `run_main_queue` (the process main thread), as AppKit's main queue does on macOS.
struct QueueMainThread {
    sender: mpsc::Sender<MainWork>,
}

impl MainThread for QueueMainThread {
    fn run(&self, work: Box<dyn FnOnce() + Send>) -> Result<(), String> {
        self.sender
            .send(MainWork::Run(work))
            .map_err(|_| "The engine's main thread has stopped.".to_owned())
    }
}

/// Runs queued work until the engine asks to exit. A panicking item is contained, as a
/// dispatched block is on macOS. Work still queued at the exit is dropped unrun.
fn run_main_queue(queue: &mpsc::Receiver<MainWork>) {
    while let Ok(item) = queue.recv() {
        match item {
            MainWork::Run(work) => {
                let _ = catch_unwind(AssertUnwindSafe(work));
            }
            MainWork::Stop => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use silo_engine::{channel::Channel, context::ProcessControl};
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
        thread,
    };

    #[test]
    fn the_main_queue_runs_work_in_order_survives_a_panic_and_stops_on_exit() {
        let (sender, queue) = mpsc::channel();
        let main = QueueMainThread {
            sender: sender.clone(),
        };
        let process = AppProcessControl::new(move || {
            let _ = sender.send(MainWork::Stop);
        });
        let ran = Arc::new(AtomicUsize::new(0));
        let main_thread = thread::current().id();
        for step in 0..3 {
            let ran = ran.clone();
            main.run(Box::new(move || {
                assert_eq!(thread::current().id(), main_thread);
                assert_eq!(ran.fetch_add(1, Ordering::SeqCst), step);
            }))
            .unwrap();
        }
        main.run(Box::new(|| panic!("contained"))).unwrap();
        process.exit(0);
        let late = ran.clone();
        main.run(Box::new(move || {
            late.fetch_add(100, Ordering::SeqCst);
        }))
        .unwrap();

        run_main_queue(&queue);
        assert_eq!(ran.load(Ordering::SeqCst), 3);
        assert_eq!(process.exit_code(), 0);
        drop(queue);
        assert!(main.run(Box::new(|| {})).is_err());
    }

    #[test]
    fn the_credential_store_is_the_file_store_unless_the_secret_service_is_chosen() {
        let home = tempfile::tempdir().unwrap();
        let paths = SilodPaths::for_home_with(
            Channel::Development,
            home.path().to_path_buf(),
            PathBuf::new(),
            |_| None,
        );
        let root = home.path().join(".local/share/org.silo.dev/credentials");
        for choice in [None, Some(""), Some("file")] {
            assert_eq!(file_store_root(&paths, choice).unwrap(), Some(root.clone()));
        }
        assert_eq!(
            file_store_root(&paths, Some("secret-service")).unwrap(),
            None
        );
        assert!(file_store_root(&paths, Some("keychain")).is_err());
    }
}
