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
    fn open_login_items(&self);
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
            ServiceStatus::NotFound => {
                return Err(LaunchError::new(format!(
                    "This build of {} does not include its engine. Reinstall {}.",
                    self.product, self.product
                )));
            }
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
                // Registered but not listening: it may still be starting, or crashed.
            }
            ServiceStatus::NotRegistered | ServiceStatus::Unknown => {
                match self.service.register() {
                    Ok(ServiceStatus::RequiresApproval) => {
                        self.service.open_login_items();
                        return Err(LaunchError {
                            message: format!(
                                "Allow the {} engine in System Settings > General > Login Items, then reopen {}.",
                                self.product, self.product
                            ),
                            requires_approval: true,
                        });
                    }
                    Ok(_) => {}
                    Err(message) => {
                        return Err(LaunchError::new(format!(
                            "{} could not register its engine: {message}",
                            self.product
                        )));
                    }
                }
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
    fn service(
        &self,
    ) -> objc2::rc::Retained<objc2_service_management::SMAppService> {
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

    fn open_login_items(&self) {}
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

        fn open_login_items(&self) {
            *self.settings_opened.lock().unwrap() += 1;
        }
    }

    fn launcher(
        directory: &Path,
        service: Arc<FakeService>,
        bundled: Option<OwnerRecord>,
    ) -> BundleLauncher {
        BundleLauncher {
            socket: directory.join("engine.sock"),
            owner_record: directory.join("engine-owner.json"),
            bundled,
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
        let launcher = launcher(directory.path(), service.clone(), Some(record("0.1.0", "b")));
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
        let service = Arc::new(FakeService::new(ServiceStatus::NotFound));
        let error = launcher(directory.path(), service, None)
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
    }

    #[test]
    fn versions_compare_by_precedence_then_build() {
        let bundled = record("0.2.0", "a");
        assert_eq!(compare(&record("0.3.0", "z"), &bundled), Comparison::Newer);
        assert_eq!(compare(&record("0.1.9", "z"), &bundled), Comparison::Older);
        assert_eq!(compare(&record("0.2.0", "a"), &bundled), Comparison::Same);
        assert_eq!(compare(&record("0.2.0", "b"), &bundled), Comparison::Unknown);
        assert_eq!(compare(&record("nope", "a"), &bundled), Comparison::Unknown);
    }
}
