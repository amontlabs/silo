//! Makes sure one `silod` is running before Silo connects (95 A-1).
//!
//! Silo.app carries a copy of the engine (`Silo Engine.app`) and its LaunchAgent plist.
//! At launch it uses the engine already serving the socket, whichever app supplied it, and
//! otherwise registers its own agent. It never starts an engine in this process.

use std::{
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use semver::Version;
use serde::Deserialize;

/// Why no engine could be reached, in words the user can act on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LaunchError {
    pub message: String,
    /// The agent is registered but waits for approval in Login Items.
    pub requires_approval: bool,
}

impl LaunchError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            requires_approval: false,
        }
    }
}

pub(crate) trait Launcher: Send + Sync {
    /// Blocks until `engine.sock` accepts connections or says why it cannot.
    fn ensure_running(&self) -> Result<(), LaunchError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ServiceStatus {
    NotRegistered,
    Enabled,
    RequiresApproval,
    NotFound,
    Unknown,
}

/// The Service Management agent registered from this app's bundle.
pub(crate) trait ServiceControl: Send + Sync {
    fn status(&self) -> ServiceStatus;
    fn register(&self) -> Result<ServiceStatus, String>;
    fn unregister(&self) -> Result<(), String>;
    /// Whether launchd has the agent's job: `None` when that cannot be told.
    fn job_loaded(&self) -> Option<bool>;
    /// What launchd reports about the loaded job: `None` when that cannot be told.
    fn job_details(&self) -> Option<JobDetails>;
    /// Removes the job from launchd, whichever app bundle registered it.
    fn drop_job(&self) -> Result<(), String>;
    /// Asks launchd to run the loaded job now. It does nothing for a job that is running.
    fn start(&self);
    fn open_login_items(&self);
}

/// The parts of `launchctl print` that tell a healthy engine job from one that cannot start.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct JobDetails {
    /// The program launchd runs; relative for a job registered from an app bundle.
    pub program: Option<String>,
    pub running: bool,
    pub last_exit_code: Option<i32>,
}

/// Exit status `EX_CONFIG`: launchd could not find or run the job's program.
const EXIT_CONFIG: i32 = 78;

impl JobDetails {
    /// Reads the output of `launchctl print`. Only the job's own keys (one tab deep) count.
    pub(crate) fn parse(output: &str) -> Self {
        let mut details = Self::default();
        for line in output.lines() {
            let Some(entry) = line.strip_prefix('\t') else {
                continue;
            };
            if entry.starts_with(char::is_whitespace) {
                continue;
            }
            let Some((key, value)) = entry.split_once(" = ") else {
                continue;
            };
            let value = value.trim();
            match key.trim() {
                "program" if details.program.is_none() => {
                    details.program = Some(value.to_owned());
                }
                "state" => details.running = value == "running",
                "pid" => details.running = true,
                "last exit code" => details.last_exit_code = value.parse().ok(),
                _ => {}
            }
        }
        details
    }

    /// Why launchd's job cannot start the engine this app carries, when it cannot.
    ///
    /// A job that is running is never judged: it may be a long data migration, or an engine
    /// of this channel that is serving work for another copy of the app.
    fn stale_reason(&self, engine_program: Option<&Path>) -> Option<&'static str> {
        if self.running {
            return None;
        }
        if let (Some(program), Some(ours)) = (self.program.as_deref(), engine_program) {
            let program = Path::new(program);
            if program.is_absolute() && !same_file(program, ours) {
                return Some("it points at another copy of the app");
            }
        }
        if self.last_exit_code == Some(EXIT_CONFIG) {
            return Some("launchd cannot run its program");
        }
        None
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// The owner record `silod` keeps beside the socket.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OwnerRecord {
    pub engine_version: String,
    pub engine_build: String,
}

pub(crate) fn read_owner_record(path: &Path) -> Option<OwnerRecord> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() > 64 * 1024 {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

/// How the running engine compares with the one this app carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Comparison {
    Newer,
    Same,
    Older,
    Unknown,
}

pub(crate) fn compare(owner: &OwnerRecord, bundled: &OwnerRecord) -> Comparison {
    let (Ok(running), Ok(carried)) = (
        Version::parse(&owner.engine_version),
        Version::parse(&bundled.engine_version),
    ) else {
        return Comparison::Unknown;
    };
    match running.cmp_precedence(&carried) {
        std::cmp::Ordering::Greater => Comparison::Newer,
        std::cmp::Ordering::Less => Comparison::Older,
        std::cmp::Ordering::Equal if owner.engine_build == bundled.engine_build => Comparison::Same,
        std::cmp::Ordering::Equal => Comparison::Unknown,
    }
}

pub(crate) struct BundleLauncher {
    pub socket: PathBuf,
    pub owner_record: PathBuf,
    /// The manifest of the engine this app carries, when the bundle has one.
    pub bundled: Option<OwnerRecord>,
    /// The engine program inside this app's bundle, which the job must run.
    pub engine_program: Option<PathBuf>,
    pub service: Arc<dyn ServiceControl>,
    pub product: &'static str,
    pub patience: Duration,
}

impl BundleLauncher {
    fn reachable(&self) -> bool {
        UnixStream::connect(&self.socket).is_ok()
    }

    fn wait_for_socket(&self) -> bool {
        let deadline = Instant::now() + self.patience;
        while Instant::now() < deadline {
            if self.reachable() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        self.reachable()
    }

    fn stale_job_reason(&self, loaded: Option<bool>) -> Option<&'static str> {
        if loaded != Some(true) {
            return None;
        }
        self.service
            .job_details()?
            .stale_reason(self.engine_program.as_deref())
    }

    /// Registering an Enabled agent is a no-op until it is unregistered, so a failed
    /// unregister means nothing will start it: say why.
    fn register_again(&self) -> Result<(), LaunchError> {
        self.service.unregister().map_err(|message| {
            LaunchError::new(format!(
                "{} could not register its engine again: {message}",
                self.product
            ))
        })?;
        self.check_registration(ServiceStatus::Enabled, self.service.register())
    }

    /// Turns the outcome of registering the agent, from `before`, into an error to show.
    fn check_registration(
        &self,
        before: ServiceStatus,
        result: Result<ServiceStatus, String>,
    ) -> Result<(), LaunchError> {
        let missing = || {
            LaunchError::new(format!(
                "This build of {} does not include its engine. Reinstall {}.",
                self.product, self.product
            ))
        };
        match result {
            Ok(ServiceStatus::NotFound) => Err(missing()),
            Err(_) if before == ServiceStatus::NotFound => Err(missing()),
            Ok(ServiceStatus::RequiresApproval) => {
                self.service.open_login_items();
                Err(LaunchError {
                    message: format!(
                        "Allow the {} engine in System Settings > General > Login Items, then reopen {}.",
                        self.product, self.product
                    ),
                    requires_approval: true,
                })
            }
            Ok(_) => Ok(()),
            Err(message) => Err(LaunchError::new(format!(
                "{} could not register its engine: {message}",
                self.product
            ))),
        }
    }
}

impl Launcher for BundleLauncher {
    fn ensure_running(&self) -> Result<(), LaunchError> {
        // One engine per Mac: whatever already serves the socket is the engine, whichever
        // app supplied it. Protocol negotiation, not this check, decides compatibility.
        if self.reachable() {
            if let (Some(owner), Some(bundled)) =
                (read_owner_record(&self.owner_record), self.bundled.as_ref())
            {
                if compare(&owner, bundled) == Comparison::Older {
                    eprintln!(
                        "{} is using the running engine {} (this app carries {}).",
                        self.product, owner.engine_version, bundled.engine_version
                    );
                }
            }
            return Ok(());
        }
        match self.service.status() {
            ServiceStatus::RequiresApproval => {
                self.service.open_login_items();
                return Err(LaunchError {
                    message: format!(
                        "Allow the {} engine in System Settings > General > Login Items, then reopen {}.",
                        self.product, self.product
                    ),
                    requires_approval: true,
                });
            }
            ServiceStatus::Enabled => {
                // Registered but not listening: it may still be starting (a long data
                // migration) or have crashed, and launchd starts it again either way, so
                // wait. Service Management keeps reporting Enabled after launchd dropped
                // the job, though (`launchctl bootout`, or a registration from another
                // copy of the app), and then nothing would ever start it: register again.
                // Only a job launchd reports missing is touched, never one still starting.
                let loaded = self.service.job_loaded();
                if loaded == Some(false) {
                    self.register_again()?;
                } else if let Some(reason) = self.stale_job_reason(loaded) {
                    // A job registered from another copy of the app, or one launchd keeps
                    // failing to run, would be kickstarted forever. Replace it with this
                    // bundle's own. A job that is running is left alone (see `stale_reason`).
                    eprintln!(
                        "{} is replacing the engine job of this channel: {reason}.",
                        self.product
                    );
                    self.service.drop_job().map_err(|message| {
                        LaunchError::new(format!(
                            "{} could not remove the old engine job: {message}",
                            self.product
                        ))
                    })?;
                    self.register_again()?;
                } else {
                    // launchd restarts the engine only after a crash (`KeepAlive` on an
                    // unsuccessful exit), so one that exited cleanly stays loaded but idle.
                    // Starting a job that is already running does nothing.
                    self.service.start();
                }
            }
            // Service Management also reports an agent that was never registered as not found,
            // so registering is what tells a missing plist apart from a first launch.
            status @ (ServiceStatus::NotFound
            | ServiceStatus::NotRegistered
            | ServiceStatus::Unknown) => {
                self.check_registration(status, self.service.register())?;
            }
        }
        if self.wait_for_socket() {
            Ok(())
        } else {
            Err(LaunchError::new(format!(
                "The {} engine did not start. Quit {} and open it again.",
                self.product, self.product
            )))
        }
    }
}

#[cfg(target_os = "macos")]
pub(crate) struct SmAppService {
    pub plist_name: String,
}

#[cfg(target_os = "macos")]
impl SmAppService {
    fn job(&self) -> Option<LaunchdJob> {
        // The plist's Label is its file name without the extension (see embed_engine.py).
        let label = self.plist_name.strip_suffix(".plist")?;
        Some(LaunchdJob {
            target: format!("gui/{}/{label}", unsafe { libc::getuid() }),
            launchctl: Arc::new(SystemLaunchctl),
        })
    }

    fn service(&self) -> objc2::rc::Retained<objc2_service_management::SMAppService> {
        let name = objc2_foundation::NSString::from_str(&self.plist_name);
        // SAFETY: Service Management resolves the plist from this app bundle's
        // Contents/Library/LaunchAgents directory.
        unsafe { objc2_service_management::SMAppService::agentServiceWithPlistName(&name) }
    }

    fn map(status: objc2_service_management::SMAppServiceStatus) -> ServiceStatus {
        use objc2_service_management::SMAppServiceStatus as Status;
        match status {
            Status::NotRegistered => ServiceStatus::NotRegistered,
            Status::Enabled => ServiceStatus::Enabled,
            Status::RequiresApproval => ServiceStatus::RequiresApproval,
            Status::NotFound => ServiceStatus::NotFound,
            _ => ServiceStatus::Unknown,
        }
    }
}

#[cfg(target_os = "macos")]
impl ServiceControl for SmAppService {
    fn status(&self) -> ServiceStatus {
        Self::map(unsafe { self.service().status() })
    }

    fn register(&self) -> Result<ServiceStatus, String> {
        let service = self.service();
        unsafe { service.registerAndReturnError() }
            .map_err(|error| error.localizedDescription().to_string())?;
        Ok(Self::map(unsafe { service.status() }))
    }

    fn unregister(&self) -> Result<(), String> {
        unsafe { self.service().unregisterAndReturnError() }
            .map_err(|error| error.localizedDescription().to_string())
    }

    fn job_loaded(&self) -> Option<bool> {
        self.job()?.loaded()
    }

    fn job_details(&self) -> Option<JobDetails> {
        self.job()?.details()
    }

    fn drop_job(&self) -> Result<(), String> {
        match self.job() {
            Some(job) => job.bootout(),
            None => Ok(()),
        }
    }

    fn start(&self) {
        if let Some(job) = self.job() {
            job.kickstart();
        }
    }

    fn open_login_items(&self) {
        unsafe { objc2_service_management::SMAppService::openSystemSettingsLoginItems() };
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) struct UnsupportedService;

#[cfg(not(target_os = "macos"))]
impl ServiceControl for UnsupportedService {
    fn status(&self) -> ServiceStatus {
        ServiceStatus::NotFound
    }

    fn register(&self) -> Result<ServiceStatus, String> {
        Err("The Silo engine runs on macOS only.".to_owned())
    }

    fn unregister(&self) -> Result<(), String> {
        Ok(())
    }

    fn job_loaded(&self) -> Option<bool> {
        None
    }

    fn job_details(&self) -> Option<JobDetails> {
        None
    }

    fn drop_job(&self) -> Result<(), String> {
        Ok(())
    }

    fn start(&self) {}

    fn open_login_items(&self) {}
}

/// What a `launchctl` run printed and exited with.
pub(crate) struct LaunchctlOutput {
    pub code: Option<i32>,
    pub stdout: String,
}

/// Runs `launchctl`; `None` when it cannot be run at all.
pub(crate) trait Launchctl: Send + Sync {
    fn run(&self, args: &[&str]) -> Option<LaunchctlOutput>;
}

#[cfg(target_os = "macos")]
struct SystemLaunchctl;

#[cfg(target_os = "macos")]
impl Launchctl for SystemLaunchctl {
    fn run(&self, args: &[&str]) -> Option<LaunchctlOutput> {
        let output = std::process::Command::new("/bin/launchctl")
            .args(args)
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;
        Some(LaunchctlOutput {
            code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        })
    }
}

/// One launchd job, addressed as `gui/<uid>/<label>`: only this app's channel label is used.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) struct LaunchdJob {
    pub target: String,
    pub launchctl: Arc<dyn Launchctl>,
}

/// launchctl exits 113 ("Could not find service") for a job it does not have.
const EXIT_NO_SERVICE: i32 = 113;
/// `launchctl bootout` exits 3 ("No such process") for a job that is already gone.
const EXIT_NO_PROCESS: i32 = 3;

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
impl LaunchdJob {
    pub(crate) fn loaded(&self) -> Option<bool> {
        match self.launchctl.run(&["print", &self.target])?.code {
            Some(0) => Some(true),
            Some(EXIT_NO_SERVICE) => Some(false),
            _ => None,
        }
    }

    pub(crate) fn details(&self) -> Option<JobDetails> {
        let output = self.launchctl.run(&["print", &self.target])?;
        (output.code == Some(0)).then(|| JobDetails::parse(&output.stdout))
    }

    pub(crate) fn kickstart(&self) {
        let _ = self.launchctl.run(&["kickstart", &self.target]);
    }

    pub(crate) fn bootout(&self) -> Result<(), String> {
        match self.launchctl.run(&["bootout", &self.target]) {
            None => Err("launchctl could not be run".to_owned()),
            Some(output) => match output.code {
                Some(0 | EXIT_NO_SERVICE | EXIT_NO_PROCESS) => Ok(()),
                code => Err(format!("launchctl bootout exited with {code:?}")),
            },
        }
    }
}

/// The `silod` program inside the engine bundle this app carries.
pub(crate) fn bundled_engine_program(resources: &Path) -> PathBuf {
    resources.join("SiloEngineDistribution/Silo Engine.app/Contents/MacOS/silod")
}

/// Reads `engine-manifest.json` of the engine bundle this app carries.
pub(crate) fn bundled_manifest(resources: &Path) -> Option<OwnerRecord> {
    let path = resources
        .join("SiloEngineDistribution/Silo Engine.app/Contents/Resources/engine-manifest.json");
    read_owner_record(&path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{os::unix::net::UnixListener, sync::Mutex};

    struct FakeService {
        status: Mutex<ServiceStatus>,
        registered: Mutex<u32>,
        settings_opened: Mutex<u32>,
        after_register: ServiceStatus,
        register_error: Option<String>,
        listener_path: Option<PathBuf>,
        listener: Mutex<Option<UnixListener>>,
        unregistered: Mutex<u32>,
        unregister_error: Option<String>,
        job_loaded: Option<bool>,
        details: Option<JobDetails>,
        dropped: Mutex<u32>,
        started: Mutex<u32>,
        /// Whether starting the loaded job makes it listen, as a launchd kickstart does.
        start_listens: bool,
    }

    impl FakeService {
        fn new(status: ServiceStatus) -> Self {
            Self {
                status: Mutex::new(status),
                registered: Mutex::new(0),
                settings_opened: Mutex::new(0),
                after_register: ServiceStatus::Enabled,
                register_error: None,
                listener_path: None,
                listener: Mutex::new(None),
                unregistered: Mutex::new(0),
                unregister_error: None,
                job_loaded: None,
                details: None,
                dropped: Mutex::new(0),
                started: Mutex::new(0),
                start_listens: false,
            }
        }
    }

    impl ServiceControl for FakeService {
        fn status(&self) -> ServiceStatus {
            *self.status.lock().unwrap()
        }

        fn register(&self) -> Result<ServiceStatus, String> {
            *self.registered.lock().unwrap() += 1;
            if let Some(error) = &self.register_error {
                return Err(error.clone());
            }
            // Registering "starts the agent": it begins listening.
            if let Some(path) = &self.listener_path {
                *self.listener.lock().unwrap() = Some(UnixListener::bind(path).unwrap());
            }
            *self.status.lock().unwrap() = self.after_register;
            Ok(self.after_register)
        }

        fn unregister(&self) -> Result<(), String> {
            *self.unregistered.lock().unwrap() += 1;
            if let Some(error) = &self.unregister_error {
                return Err(error.clone());
            }
            *self.status.lock().unwrap() = ServiceStatus::NotRegistered;
            Ok(())
        }

        fn job_loaded(&self) -> Option<bool> {
            self.job_loaded
        }

        fn job_details(&self) -> Option<JobDetails> {
            self.details.clone()
        }

        fn drop_job(&self) -> Result<(), String> {
            *self.dropped.lock().unwrap() += 1;
            Ok(())
        }

        fn start(&self) {
            *self.started.lock().unwrap() += 1;
            if let (true, Some(path)) = (self.start_listens, &self.listener_path) {
                *self.listener.lock().unwrap() = Some(UnixListener::bind(path).unwrap());
            }
        }

        fn open_login_items(&self) {
            *self.settings_opened.lock().unwrap() += 1;
        }
    }

    const OUR_ENGINE: &str = "/Applications/Silo Dev.app/Contents/Resources/SiloEngineDistribution/Silo Engine.app/Contents/MacOS/silod";
    const OTHER_ENGINE: &str = "/Users/someone/build/Silo Dev.app/Contents/Resources/SiloEngineDistribution/Silo Engine.app/Contents/MacOS/silod";

    fn job(program: Option<&str>, running: bool, last_exit_code: Option<i32>) -> JobDetails {
        JobDetails {
            program: program.map(str::to_owned),
            running,
            last_exit_code,
        }
    }

    /// Runs a launcher over a loaded job with `details`; returns the service and the outcome.
    fn run_with_job(
        details: JobDetails,
        listens_after_register: bool,
    ) -> (Arc<FakeService>, Result<(), LaunchError>) {
        let directory = tempfile::tempdir().unwrap();
        let mut service = FakeService::new(ServiceStatus::Enabled);
        service.job_loaded = Some(true);
        service.details = Some(details);
        if listens_after_register {
            service.listener_path = Some(directory.path().join("engine.sock"));
        }
        let service = Arc::new(service);
        let result = launcher(directory.path(), service.clone(), None).ensure_running();
        (service, result)
    }

    fn launcher(
        directory: &Path,
        service: Arc<dyn ServiceControl>,
        bundled: Option<OwnerRecord>,
    ) -> BundleLauncher {
        BundleLauncher {
            socket: directory.join("engine.sock"),
            owner_record: directory.join("engine-owner.json"),
            bundled,
            engine_program: Some(PathBuf::from(OUR_ENGINE)),
            service,
            product: "Silo",
            patience: Duration::from_millis(400),
        }
    }

    fn record(version: &str, build: &str) -> OwnerRecord {
        OwnerRecord {
            engine_version: version.into(),
            engine_build: build.into(),
        }
    }

    #[test]
    fn a_running_engine_is_used_and_nothing_is_registered() {
        let directory = tempfile::tempdir().unwrap();
        let _listener = UnixListener::bind(directory.path().join("engine.sock")).unwrap();
        let service = Arc::new(FakeService::new(ServiceStatus::NotRegistered));
        // Even an older running engine is used; negotiation decides compatibility.
        std::fs::write(
            directory.path().join("engine-owner.json"),
            br#"{"engineVersion":"0.0.9","engineBuild":"x","pid":1}"#,
        )
        .unwrap();
        let launcher = launcher(
            directory.path(),
            service.clone(),
            Some(record("0.1.0", "b")),
        );
        launcher.ensure_running().unwrap();
        assert_eq!(*service.registered.lock().unwrap(), 0);
    }

    #[test]
    fn without_an_engine_the_bundled_agent_is_registered_once_and_awaited() {
        let directory = tempfile::tempdir().unwrap();
        let mut service = FakeService::new(ServiceStatus::NotRegistered);
        service.listener_path = Some(directory.path().join("engine.sock"));
        let service = Arc::new(service);
        let launcher = launcher(directory.path(), service.clone(), None);
        launcher.ensure_running().unwrap();
        assert_eq!(*service.registered.lock().unwrap(), 1);
    }

    #[test]
    fn an_agent_reported_as_not_found_before_its_first_registration_is_registered() {
        // A freshly built bundle's agent reads as not found until it is first registered.
        let directory = tempfile::tempdir().unwrap();
        let mut service = FakeService::new(ServiceStatus::NotFound);
        service.listener_path = Some(directory.path().join("engine.sock"));
        let service = Arc::new(service);
        launcher(directory.path(), service.clone(), None)
            .ensure_running()
            .unwrap();
        assert_eq!(*service.registered.lock().unwrap(), 1);
    }

    #[test]
    fn approval_is_surfaced_and_opens_login_items() {
        let directory = tempfile::tempdir().unwrap();
        let mut service = FakeService::new(ServiceStatus::NotRegistered);
        service.after_register = ServiceStatus::RequiresApproval;
        let service = Arc::new(service);
        let error = launcher(directory.path(), service.clone(), None)
            .ensure_running()
            .unwrap_err();
        assert!(error.requires_approval);
        assert_eq!(*service.settings_opened.lock().unwrap(), 1);

        let service = Arc::new(FakeService::new(ServiceStatus::RequiresApproval));
        let error = launcher(directory.path(), service.clone(), None)
            .ensure_running()
            .unwrap_err();
        assert!(error.requires_approval);
        assert_eq!(*service.registered.lock().unwrap(), 0);
    }

    #[test]
    fn a_missing_agent_and_a_failed_registration_are_explained() {
        let directory = tempfile::tempdir().unwrap();
        let mut missing = FakeService::new(ServiceStatus::NotFound);
        missing.register_error = Some("no such plist".into());
        let missing = Arc::new(missing);
        let error = launcher(directory.path(), missing.clone(), None)
            .ensure_running()
            .unwrap_err();
        assert!(error.message.contains("does not include its engine"));
        assert_eq!(*missing.registered.lock().unwrap(), 1);

        let mut still_missing = FakeService::new(ServiceStatus::NotFound);
        still_missing.after_register = ServiceStatus::NotFound;
        let error = launcher(directory.path(), Arc::new(still_missing), None)
            .ensure_running()
            .unwrap_err();
        assert!(error.message.contains("does not include its engine"));

        let mut failing = FakeService::new(ServiceStatus::NotRegistered);
        failing.register_error = Some("denied".into());
        let error = launcher(directory.path(), Arc::new(failing), None)
            .ensure_running()
            .unwrap_err();
        assert!(error.message.contains("denied"));
        assert!(!error.requires_approval);
    }

    #[test]
    fn an_enabled_agent_that_never_listens_times_out_without_registering_again() {
        let directory = tempfile::tempdir().unwrap();
        let service = Arc::new(FakeService::new(ServiceStatus::Enabled));
        let error = launcher(directory.path(), service.clone(), None)
            .ensure_running()
            .unwrap_err();
        assert!(error.message.contains("did not start"));
        assert_eq!(*service.registered.lock().unwrap(), 0);
        assert_eq!(*service.unregistered.lock().unwrap(), 0);
    }

    #[test]
    fn an_enabled_agent_launchd_no_longer_has_is_registered_again() {
        // After `launchctl bootout`, Service Management still reports Enabled and
        // nothing would start the engine.
        let directory = tempfile::tempdir().unwrap();
        let mut service = FakeService::new(ServiceStatus::Enabled);
        service.job_loaded = Some(false);
        service.listener_path = Some(directory.path().join("engine.sock"));
        let service = Arc::new(service);
        launcher(directory.path(), service.clone(), None)
            .ensure_running()
            .unwrap();
        assert_eq!(*service.unregistered.lock().unwrap(), 1);
        assert_eq!(*service.registered.lock().unwrap(), 1);
    }

    #[test]
    fn a_failed_registration_of_a_dropped_agent_is_explained() {
        let directory = tempfile::tempdir().unwrap();
        let mut service = FakeService::new(ServiceStatus::Enabled);
        service.job_loaded = Some(false);
        service.register_error = Some("denied".into());
        let error = launcher(directory.path(), Arc::new(service), None)
            .ensure_running()
            .unwrap_err();
        assert!(error
            .message
            .contains("could not register its engine: denied"));
    }

    #[test]
    fn a_failed_unregister_of_a_dropped_agent_is_explained() {
        let directory = tempfile::tempdir().unwrap();
        let mut service = FakeService::new(ServiceStatus::Enabled);
        service.job_loaded = Some(false);
        service.unregister_error = Some("operation not permitted".into());
        let service = Arc::new(service);
        let error = launcher(directory.path(), service.clone(), None)
            .ensure_running()
            .unwrap_err();
        assert!(error
            .message
            .contains("could not register its engine again: operation not permitted"));
        assert_eq!(*service.registered.lock().unwrap(), 0);
    }

    #[test]
    fn a_slow_engine_launchd_still_has_is_awaited_not_registered_again() {
        // A long data migration keeps the socket closed while the job runs: leave it be.
        let directory = tempfile::tempdir().unwrap();
        let mut service = FakeService::new(ServiceStatus::Enabled);
        service.job_loaded = Some(true);
        let service = Arc::new(service);
        let socket = directory.path().join("engine.sock");
        let starting = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            UnixListener::bind(socket).unwrap()
        });
        launcher(directory.path(), service.clone(), None)
            .ensure_running()
            .unwrap();
        let _listener = starting.join().unwrap();
        assert_eq!(*service.unregistered.lock().unwrap(), 0);
        assert_eq!(*service.registered.lock().unwrap(), 0);

        // Still not listening after the wait: report it, and still do not re-register.
        let directory = tempfile::tempdir().unwrap();
        let mut service = FakeService::new(ServiceStatus::Enabled);
        service.job_loaded = Some(true);
        let service = Arc::new(service);
        let error = launcher(directory.path(), service.clone(), None)
            .ensure_running()
            .unwrap_err();
        assert!(error.message.contains("did not start"));
        assert_eq!(*service.unregistered.lock().unwrap(), 0);
    }

    #[test]
    fn a_loaded_agent_that_exited_cleanly_is_started_again() {
        // KeepAlive restarts only a crashed engine: after a clean exit launchd keeps the job
        // loaded but idle, and Service Management still reports Enabled.
        let directory = tempfile::tempdir().unwrap();
        let mut service = FakeService::new(ServiceStatus::Enabled);
        service.job_loaded = Some(true);
        service.start_listens = true;
        service.listener_path = Some(directory.path().join("engine.sock"));
        let service = Arc::new(service);
        launcher(directory.path(), service.clone(), None)
            .ensure_running()
            .unwrap();
        assert_eq!(*service.started.lock().unwrap(), 1);
        assert_eq!(*service.unregistered.lock().unwrap(), 0);
        assert_eq!(*service.registered.lock().unwrap(), 0);
    }

    #[test]
    fn a_running_engine_is_not_started_again() {
        let directory = tempfile::tempdir().unwrap();
        let _listener = UnixListener::bind(directory.path().join("engine.sock")).unwrap();
        let mut service = FakeService::new(ServiceStatus::Enabled);
        service.job_loaded = Some(true);
        let service = Arc::new(service);
        launcher(directory.path(), service.clone(), None)
            .ensure_running()
            .unwrap();
        assert_eq!(*service.started.lock().unwrap(), 0);
    }

    #[test]
    fn a_job_registered_from_another_copy_is_replaced_by_this_bundles() {
        let (service, result) = run_with_job(job(Some(OTHER_ENGINE), false, Some(78)), true);
        result.unwrap();
        assert_eq!(*service.dropped.lock().unwrap(), 1);
        assert_eq!(*service.unregistered.lock().unwrap(), 1);
        assert_eq!(*service.registered.lock().unwrap(), 1);
        assert_eq!(*service.started.lock().unwrap(), 0);

        // Idle on a program elsewhere, without any failure yet.
        let (service, result) = run_with_job(job(Some(OTHER_ENGINE), false, None), true);
        result.unwrap();
        assert_eq!(*service.dropped.lock().unwrap(), 1);
    }

    #[test]
    fn a_job_that_keeps_failing_with_a_configuration_error_is_replaced() {
        // The program is relative, so only the exit status shows launchd cannot run it.
        let relative =
            "Contents/Resources/SiloEngineDistribution/Silo Engine.app/Contents/MacOS/silod";
        let (service, result) = run_with_job(job(Some(relative), false, Some(78)), true);
        result.unwrap();
        assert_eq!(*service.dropped.lock().unwrap(), 1);
        assert_eq!(*service.registered.lock().unwrap(), 1);

        let (service, result) = run_with_job(job(None, false, Some(78)), true);
        result.unwrap();
        assert_eq!(*service.dropped.lock().unwrap(), 1);
    }

    #[test]
    fn a_job_that_runs_this_bundles_engine_is_only_started() {
        for last_exit_code in [None, Some(0), Some(1)] {
            let (service, result) =
                run_with_job(job(Some(OUR_ENGINE), false, last_exit_code), false);
            // Nothing listens, so the wait ends in the usual message.
            assert!(result.unwrap_err().message.contains("did not start"));
            assert_eq!(*service.started.lock().unwrap(), 1);
            assert_eq!(*service.dropped.lock().unwrap(), 0);
            assert_eq!(*service.registered.lock().unwrap(), 0);
        }
    }

    #[test]
    fn a_running_job_of_another_copy_is_left_alone() {
        // Another copy's engine that is up (a long migration, or serving work) is not
        // replaced mid-work, whatever it runs or exited with before.
        let (service, result) = run_with_job(job(Some(OTHER_ENGINE), true, Some(78)), false);
        assert!(result.unwrap_err().message.contains("did not start"));
        assert_eq!(*service.dropped.lock().unwrap(), 0);
        assert_eq!(*service.unregistered.lock().unwrap(), 0);
        assert_eq!(*service.registered.lock().unwrap(), 0);
    }

    #[test]
    fn a_reachable_engine_of_another_copy_is_never_touched() {
        let directory = tempfile::tempdir().unwrap();
        let _listener = UnixListener::bind(directory.path().join("engine.sock")).unwrap();
        let mut service = FakeService::new(ServiceStatus::Enabled);
        service.job_loaded = Some(true);
        service.details = Some(job(Some(OTHER_ENGINE), false, Some(78)));
        let service = Arc::new(service);
        launcher(directory.path(), service.clone(), None)
            .ensure_running()
            .unwrap();
        assert_eq!(*service.dropped.lock().unwrap(), 0);
        assert_eq!(*service.registered.lock().unwrap(), 0);
    }

    #[test]
    fn unreadable_job_details_fall_back_to_starting_the_job() {
        let directory = tempfile::tempdir().unwrap();
        let mut service = FakeService::new(ServiceStatus::Enabled);
        service.job_loaded = Some(true);
        let service = Arc::new(service);
        let _ = launcher(directory.path(), service.clone(), None).ensure_running();
        assert_eq!(*service.started.lock().unwrap(), 1);
        assert_eq!(*service.dropped.lock().unwrap(), 0);
    }

    #[test]
    fn a_failed_removal_of_the_old_job_is_explained() {
        struct Refusing(FakeService);
        impl ServiceControl for Refusing {
            fn status(&self) -> ServiceStatus {
                self.0.status()
            }
            fn register(&self) -> Result<ServiceStatus, String> {
                self.0.register()
            }
            fn unregister(&self) -> Result<(), String> {
                self.0.unregister()
            }
            fn job_loaded(&self) -> Option<bool> {
                self.0.job_loaded()
            }
            fn job_details(&self) -> Option<JobDetails> {
                self.0.job_details()
            }
            fn drop_job(&self) -> Result<(), String> {
                Err("not permitted".into())
            }
            fn start(&self) {
                self.0.start();
            }
            fn open_login_items(&self) {
                self.0.open_login_items();
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let mut inner = FakeService::new(ServiceStatus::Enabled);
        inner.job_loaded = Some(true);
        inner.details = Some(job(Some(OTHER_ENGINE), false, Some(78)));
        let error = launcher(directory.path(), Arc::new(Refusing(inner)), None)
            .ensure_running()
            .unwrap_err();
        assert!(error
            .message
            .contains("could not remove the old engine job: not permitted"));
    }

    const PRINT_OUTPUT: &str = "gui/501/org.silo.dev.engine = {
\tactive count = 0
\tpath = (submitted by smd)
\ttype = LaunchAgent
\tstate = spawn scheduled
\tprogram = Contents/Resources/SiloEngineDistribution/Silo Engine.app/Contents/MacOS/silod
\targuments = {
\t\tsilod
\t\t--agent
\t}
\tproperties = keepalive | runatload
\truns = 9
\tlast exit code = 78
}
";

    #[test]
    fn launchctl_print_output_is_read_for_the_jobs_own_keys() {
        let details = JobDetails::parse(PRINT_OUTPUT);
        assert_eq!(
            details,
            job(
                Some("Contents/Resources/SiloEngineDistribution/Silo Engine.app/Contents/MacOS/silod"),
                false,
                Some(78)
            )
        );
        let running = JobDetails::parse(
            "gui/501/x = {\n\tstate = running\n\tpid = 4242\n\tlast exit code = (never exited)\n\tprogram = /a/silod\n\tendpoints = {\n\t\tpid = 1\n\t}\n}\n",
        );
        assert!(running.running);
        assert_eq!(running.last_exit_code, None);
        assert_eq!(running.program.as_deref(), Some("/a/silod"));
    }

    struct FakeLaunchctl {
        calls: Mutex<Vec<Vec<String>>>,
        print: Option<LaunchctlOutput>,
        bootout_code: Option<i32>,
    }

    impl Launchctl for FakeLaunchctl {
        fn run(&self, args: &[&str]) -> Option<LaunchctlOutput> {
            self.calls
                .lock()
                .unwrap()
                .push(args.iter().map(|arg| (*arg).to_owned()).collect());
            match args[0] {
                "print" => self.print.as_ref().map(|output| LaunchctlOutput {
                    code: output.code,
                    stdout: output.stdout.clone(),
                }),
                "bootout" => Some(LaunchctlOutput {
                    code: self.bootout_code,
                    stdout: String::new(),
                }),
                _ => Some(LaunchctlOutput {
                    code: Some(0),
                    stdout: String::new(),
                }),
            }
        }
    }

    fn launchd_job(print_code: i32, bootout_code: i32) -> (LaunchdJob, Arc<FakeLaunchctl>) {
        let launchctl = Arc::new(FakeLaunchctl {
            calls: Mutex::new(Vec::new()),
            print: Some(LaunchctlOutput {
                code: Some(print_code),
                stdout: PRINT_OUTPUT.to_owned(),
            }),
            bootout_code: Some(bootout_code),
        });
        let job = LaunchdJob {
            target: "gui/501/org.silo.dev.engine".to_owned(),
            launchctl: launchctl.clone(),
        };
        (job, launchctl)
    }

    #[test]
    fn the_launchd_job_is_read_and_removed_by_its_own_label_only() {
        let (job, launchctl) = launchd_job(0, 0);
        assert_eq!(job.loaded(), Some(true));
        assert_eq!(job.details().unwrap().last_exit_code, Some(78));
        job.bootout().unwrap();
        job.kickstart();
        let calls = launchctl.calls.lock().unwrap();
        assert_eq!(
            calls.last().unwrap(),
            &["kickstart", "gui/501/org.silo.dev.engine"]
        );
        assert!(calls
            .iter()
            .all(|call| call.last().unwrap() == "gui/501/org.silo.dev.engine"));
        assert!(calls.contains(&vec![
            "bootout".to_owned(),
            "gui/501/org.silo.dev.engine".to_owned()
        ]));

        let (gone, _) = launchd_job(113, 113);
        assert_eq!(gone.loaded(), Some(false));
        assert_eq!(gone.details(), None);
        gone.bootout().unwrap();

        let (failing, _) = launchd_job(0, 1);
        assert!(failing.bootout().is_err());
    }

    #[test]
    fn versions_compare_by_precedence_then_build() {
        let bundled = record("0.2.0", "a");
        assert_eq!(compare(&record("0.3.0", "z"), &bundled), Comparison::Newer);
        assert_eq!(compare(&record("0.1.9", "z"), &bundled), Comparison::Older);
        assert_eq!(compare(&record("0.2.0", "a"), &bundled), Comparison::Same);
        assert_eq!(
            compare(&record("0.2.0", "b"), &bundled),
            Comparison::Unknown
        );
        assert_eq!(compare(&record("nope", "a"), &bundled), Comparison::Unknown);
    }
}
