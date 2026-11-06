//! The engine agent on a Linux host: no AppKit, no bundle. The process main thread runs a
//! plain work queue in place of the AppKit loop until the engine exits, and identity comes
//! from the manifest beside the executable (`paths::default_resource_dir`).
//!
//! Before it reads anything there, every entry point checks the account it runs as and the
//! engine directory (`install_trust`); the agent also checks the bundled runtime's hashes
//! before its first `msb` launch.

use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
    panic::{catch_unwind, AssertUnwindSafe},
    path::{Path, PathBuf},
    sync::{mpsc, Arc},
};

use silo_engine::context::MainThread;

use crate::{
    agent, install_trust,
    manifest::EngineManifest,
    paths::{default_resource_dir, SilodPaths},
    process::AppProcessControl,
};

/// `SILO_CREDENTIAL_STORE`: `file` (the default) keeps credentials in the engine's file store;
/// `secret-service` uses the desktop Secret Service through `keyring`'s own default. The
/// engine records the store it used first and refuses another one later.
pub const CREDENTIAL_STORE_ENV: &str = "SILO_CREDENTIAL_STORE";

/// The record of the chosen store, under the engine data directory.
const CREDENTIAL_STORE_RECORD: &str = "credential-store";

/// This process's account and its engine directory pass `install_trust`, which every entry
/// point checks before it reads anything from that directory. Returns the executable and its
/// resources directory.
fn trusted_engine() -> Result<(PathBuf, PathBuf), String> {
    let euid = unsafe { libc::geteuid() };
    install_trust::check_engine_account(euid)?;
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    install_trust::verify_engine_directory(&executable, euid)?;
    let resources = default_resource_dir(&executable)?;
    Ok((executable, resources))
}

/// The channel and paths of a helper process (the bridge and the guest helper), with the
/// credential store the engine chose installed, so nothing they call reaches `keyring`'s
/// default backend. A helper never records a choice or sweeps the store: the engine does.
fn helper_paths() -> Result<SilodPaths, String> {
    let (_, resources) = trusted_engine()?;
    // Validated as strictly as a bundle's, without the bundle identifier and code signature
    // that a Linux engine does not have.
    let manifest = EngineManifest::load_unbundled(&resources)?;
    let channel = manifest.channel()?;
    silo_engine::channel::init(&manifest.channel_identifier);
    let paths = SilodPaths::resolve(channel, resources)?;
    install_credential_store(
        &paths,
        std::env::var(CREDENTIAL_STORE_ENV).ok().as_deref(),
        false,
    )?;
    Ok(paths)
}

/// Runs the alias ProxyCommand helper. The manifest selects its channel before the
/// engine-socket client reads channel-specific state.
pub fn run_remote_guest(args: &[String]) -> Result<i32, String> {
    if args.len() != 2 && args.len() != 3 {
        return Err("usage: silod --remote-guest DEVICE_ID COMPUTER_ID [PORT]".to_owned());
    }
    crate::run_from_bundle(&std::env::args().collect::<Vec<_>>())?;
    helper_paths()?;
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
    helper_paths()?;
    silo_engine::remote::run_bridge()?;
    Ok(0)
}

pub fn run_agent() -> Result<i32, String> {
    agent::set_private_umask();
    let (executable, resources) = trusted_engine()?;
    let manifest = EngineManifest::load_unbundled(&resources)?;
    verify_bundled_runtime(&executable, &resources)?;
    let Some(owned) = agent::acquire(&manifest, resources)? else {
        return Ok(0);
    };
    install_credential_store(
        &owned.paths,
        std::env::var(CREDENTIAL_STORE_ENV).ok().as_deref(),
        true,
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

/// The bundled runtime's integrity, once per process before the first `msb` launch (which
/// only the agent makes). An engine directory without the runtime starts, and no computer
/// can start there.
fn verify_bundled_runtime(executable: &Path, resources: &Path) -> Result<(), String> {
    let tools = executable
        .parent()
        .ok_or_else(|| "silod executable has no parent directory".to_owned())?;
    let msb = tools.join("msb");
    let library = silo_engine::runtime::bundled_runtime_library(
        &msb,
        resources,
        silo_engine::runtime::RuntimePackageKind::Other,
    );
    match silo_engine::dependencies::verify_bundled_runtime(&msb, &library, resources) {
        Ok(silo_engine::dependencies::BundledRuntime::Verified) => Ok(()),
        Ok(silo_engine::dependencies::BundledRuntime::Absent) => {
            eprintln!(
                "silod: no MicroSandbox runtime in {}; computers cannot start",
                tools.display()
            );
            Ok(())
        }
        Err(error) => Err(format!(
            "silod will not start: the bundled runtime failed its check. {error}"
        )),
    }
}

/// Where the file credential store keeps its files: beside the engine's data.
pub fn credential_directory(paths: &SilodPaths) -> PathBuf {
    paths
        .engine
        .app_data
        .join(silo_engine::credential_file_store::DIRECTORY_NAME)
}

/// The credential stores a Linux engine can use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CredentialStore {
    File,
    SecretService,
}

impl CredentialStore {
    fn name(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::SecretService => "secret-service",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "file" => Some(Self::File),
            "secret-service" => Some(Self::SecretService),
            _ => None,
        }
    }
}

/// Every `keyring::Entry` the engine makes uses the file store unless the Secret Service was
/// chosen. A server has no desktop session for the Secret Service, and choosing per start
/// whether one answers would split credentials between two stores. The engine (`owner`)
/// records its first choice; every later start, and every helper, uses the recorded store
/// and refuses a different explicit one.
fn install_credential_store(
    paths: &SilodPaths,
    choice: Option<&str>,
    owner: bool,
) -> Result<(), String> {
    let record = paths.engine.app_data.join(CREDENTIAL_STORE_RECORD);
    let recorded = read_store_record(&record)?;
    let store = select_store(parse_choice(choice)?, recorded, &record)?;
    if owner && recorded.is_none() {
        write_store_record(&record, store)?;
    }
    match store {
        CredentialStore::File => {
            let data_root = paths
                .engine
                .app_data
                .parent()
                .ok_or_else(|| "The engine data directory has no parent".to_owned())?;
            silo_engine::credential_file_store::install(
                credential_directory(paths),
                data_root,
                owner,
            )
        }
        CredentialStore::SecretService => Ok(()),
    }
}

/// `SILO_CREDENTIAL_STORE`'s store, or `None` when it is unset or empty.
fn parse_choice(choice: Option<&str>) -> Result<Option<CredentialStore>, String> {
    match choice {
        None | Some("") => Ok(None),
        Some(value) => CredentialStore::parse(value).map(Some).ok_or_else(|| {
            format!("{CREDENTIAL_STORE_ENV} must be file or secret-service, not {value}")
        }),
    }
}

/// The store to use: the explicit choice, which must match the record when there is one,
/// else the recorded store, else the file store.
fn select_store(
    choice: Option<CredentialStore>,
    recorded: Option<CredentialStore>,
    record: &Path,
) -> Result<CredentialStore, String> {
    match (choice, recorded) {
        (Some(choice), Some(recorded)) if choice != recorded => Err(format!(
            "{CREDENTIAL_STORE_ENV}={} but this engine keeps its credentials in the {} store \
             (recorded in {}). Another store would not find them: unset {CREDENTIAL_STORE_ENV}, \
             or move the credentials yourself and then delete that file.",
            choice.name(),
            recorded.name(),
            record.display()
        )),
        (Some(store), _) | (None, Some(store)) => Ok(store),
        (None, None) => Ok(CredentialStore::File),
    }
}

fn read_store_record(path: &Path) -> Result<Option<CredentialStore>, String> {
    let unreadable = |reason: String| {
        format!(
            "The credential store record {} is unusable: {reason}",
            path.display()
        )
    };
    let file = fs::File::options()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path);
    let file = match file {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(unreadable(error.to_string())),
    };
    let metadata = file
        .metadata()
        .map_err(|error| unreadable(error.to_string()))?;
    if !metadata.is_file() {
        return Err(unreadable("it is not a regular file".into()));
    }
    let mut contents = String::new();
    file.take(64)
        .read_to_string(&mut contents)
        .map_err(|error| unreadable(error.to_string()))?;
    CredentialStore::parse(contents.trim())
        .map(Some)
        .ok_or_else(|| unreadable("it names no known store".into()))
}

/// Writes the record whole (a new 0600 file renamed into place).
fn write_store_record(path: &Path, store: CredentialStore) -> Result<(), String> {
    let failed = |error: std::io::Error| {
        format!(
            "Could not record the credential store in {}: {error}",
            path.display()
        )
    };
    let directory = path
        .parent()
        .ok_or_else(|| "The credential store record has no directory".to_owned())?;
    fs::create_dir_all(directory).map_err(failed)?;
    let staged = directory.join(format!(
        ".{CREDENTIAL_STORE_RECORD}.{}",
        uuid::Uuid::new_v4().simple()
    ));
    let written = (|| {
        let mut file = fs::File::options()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&staged)?;
        writeln!(file, "{}", store.name())?;
        file.sync_all()?;
        fs::rename(&staged, path)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&staged);
    }
    written.map_err(failed)
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
        assert_eq!(
            credential_directory(&paths),
            home.path().join(".local/share/org.silo.dev/credentials")
        );
        let record = Path::new("/record");
        for choice in [None, Some(""), Some("file")] {
            let choice = parse_choice(choice).unwrap();
            assert_eq!(
                select_store(choice, None, record).unwrap(),
                CredentialStore::File
            );
        }
        assert_eq!(
            select_store(parse_choice(Some("secret-service")).unwrap(), None, record).unwrap(),
            CredentialStore::SecretService
        );
        assert!(parse_choice(Some("keychain")).is_err());
    }

    #[test]
    fn the_first_store_is_recorded_and_a_different_one_is_refused_later() {
        let directory = tempfile::tempdir().unwrap();
        let record = directory.path().join("data").join(CREDENTIAL_STORE_RECORD);
        assert_eq!(read_store_record(&record).unwrap(), None);
        write_store_record(&record, CredentialStore::SecretService).unwrap();
        assert_eq!(
            read_store_record(&record).unwrap(),
            Some(CredentialStore::SecretService)
        );
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&record).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let recorded = read_store_record(&record).unwrap();
        // Unset follows the record; the same explicit store is fine; another is refused,
        // naming the record and never guessing.
        assert_eq!(
            select_store(None, recorded, &record).unwrap(),
            CredentialStore::SecretService
        );
        assert_eq!(
            select_store(Some(CredentialStore::SecretService), recorded, &record).unwrap(),
            CredentialStore::SecretService
        );
        let error = select_store(Some(CredentialStore::File), recorded, &record).unwrap_err();
        assert!(
            error.contains("SILO_CREDENTIAL_STORE=file")
                && error.contains("secret-service store")
                && error.contains(&record.display().to_string()),
            "{error}"
        );

        fs::write(&record, "keychain\n").unwrap();
        assert!(read_store_record(&record).is_err());
        fs::remove_file(&record).unwrap();
        std::os::unix::fs::symlink("/etc/hostname", &record).unwrap();
        assert!(read_store_record(&record).is_err());
    }

    #[test]
    fn the_owner_records_the_file_store_and_prepares_it() {
        let home = tempfile::tempdir().unwrap();
        let paths = SilodPaths::for_home_with(
            Channel::Development,
            home.path().to_path_buf(),
            PathBuf::new(),
            |_| None,
        );
        let record = paths.engine.app_data.join(CREDENTIAL_STORE_RECORD);
        // A helper with nothing recorded records nothing.
        install_credential_store(&paths, Some("secret-service"), false).unwrap();
        assert!(!record.exists());
        install_credential_store(&paths, Some("secret-service"), true).unwrap();
        assert_eq!(fs::read_to_string(&record).unwrap(), "secret-service\n");
        assert!(install_credential_store(&paths, Some("file"), true).is_err());
        assert!(install_credential_store(&paths, Some("file"), false).is_err());
        install_credential_store(&paths, None, false).unwrap();
        assert!(!credential_directory(&paths).exists());
    }
}
