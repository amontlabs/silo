//! The real engine host for shutdown and update: silod's adapters, guards and runtime access.
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use serde_json::Value;
use silo_api::jobs::JobId;
use silo_engine::{
    context::{Audience, EngineContext},
    engine_update::{guards::ProcessGuards, HeldGuards, Platform, SaveAdapter},
    macos_computers::{MacosContext, MacosHost, MacosUpdateAdapter},
    runtime::{RuntimeHost, RuntimePackageKind},
};

use crate::context::SilodContext;

/// What silod gives the update coordinator and the shutdown path.
pub struct ServicesPlatform {
    context: Arc<SilodContext>,
}

impl ServicesPlatform {
    pub fn new(context: Arc<SilodContext>) -> Self {
        Self { context }
    }

    fn host(&self) -> ServicesRuntimeHost {
        ServicesRuntimeHost {
            context: self.context.clone(),
        }
    }

    fn macos_context(&self) -> MacosContext {
        self.host().macos_context()
    }
}

impl Platform for ServicesPlatform {
    fn adapters(&self) -> Vec<Box<dyn SaveAdapter>> {
        vec![
            Box::new(MacosUpdateAdapter::new(self.macos_context())),
            Box::new(silo_engine::runtime::update_save::LinuxUpdateAdapter::new(
                Arc::new(self.host()),
            )),
        ]
    }

    fn acquire_guards(&self, job: Option<&JobId>) -> Result<Box<dyn HeldGuards>, String> {
        ProcessGuards::acquire(&self.macos_context(), job)
            .map(|guards| Box::new(guards) as Box<dyn HeldGuards>)
    }

    fn begin_shutdown(&self) {
        silo_engine::runtime::shutdown::begin();
    }

    fn stop_computers(&self, deadline: Option<Instant>) -> Result<(), String> {
        silo_engine::runtime::shutdown::stop_local_computers(&self.host(), deadline)
    }

    fn flush_settings(&self) -> Result<(), String> {
        silo_engine::settings::flush_pending()
    }

    fn close_connections(&self) {
        silo_engine::ssh_access::close_all();
        silo_engine::remote_network::close_all();
        silo_engine::desktop_viewer::close_all();
        self.context.close_remote_displays(None);
    }

    fn host_build(&self) -> String {
        host_build()
    }

    fn reopen_admission(&self) {
        silo_engine::runtime::shutdown::cancel();
    }

    fn update_readiness(&self) -> Result<Vec<String>, String> {
        silo_engine::updates::readiness(|| {
            if !silo_engine::runtime::OPERATIONS.is_idle() {
                return Err("Wait for computer operations to finish before updating.".into());
            }
            silo_engine::runtime::shutdown::ensure_accepting_operations()
        })?;
        let mut names = Vec::new();
        for adapter in self.adapters() {
            names.extend(
                adapter
                    .running()
                    .map_err(|_| {
                        "Silo could not verify computer status. Check Computers before updating."
                            .to_owned()
                    })?
                    .into_iter()
                    .map(|computer| computer.name),
            );
        }
        Ok(names)
    }
}

/// The Mac's own build (`kern.osversion`), which a saved machine state is tied to.
fn host_build() -> String {
    let name = c"kern.osversion";
    let mut buffer = [0u8; 64];
    let mut size = buffer.len();
    // SAFETY: `name` is NUL terminated and `buffer` and `size` are valid for the call.
    let status = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            buffer.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if status != 0 {
        return String::new();
    }
    String::from_utf8_lossy(&buffer[..size.min(buffer.len())])
        .trim_end_matches('\0')
        .to_owned()
}

/// Runtime access for stopping computers and saving Linux ones.
#[derive(Clone)]
pub struct ServicesRuntimeHost {
    context: Arc<SilodContext>,
}

impl ServicesRuntimeHost {
    fn migration(&self) -> silo_engine::runtime_migration::Controller {
        silo_engine::runtime_migration::Controller::new(self.context.paths().app_data.clone())
    }

    fn macos_context(&self) -> MacosContext {
        let engine: Arc<dyn EngineContext> = self.context.clone();
        MacosContext::new(
            engine,
            Arc::new(ServicesMacosHost {
                runtime: self.clone(),
            }),
        )
    }
}

impl RuntimeHost for ServicesRuntimeHost {
    fn engine_context(&self) -> Arc<dyn EngineContext> {
        self.context.clone()
    }

    fn runtime_storage(&self) -> Result<PathBuf, String> {
        self.migration().runtime_storage()
    }

    fn blocks_operations(&self) -> bool {
        self.migration().blocks_operations()
    }

    fn bundled_tools_directory(&self) -> Result<PathBuf, String> {
        let executable = std::env::current_exe().map_err(|error| error.to_string())?;
        let bundle = matches!(
            self.context.identity().package_kind,
            silo_engine::PackageKind::MacApp
        )
        .then_some(silo_engine::bundled_tools::BundleKind::App);
        silo_engine::bundled_tools::resolve(
            &executable,
            &self.context.paths().resources,
            bundle,
            None,
        )
    }

    fn ensure_runtime_ready(&self) -> Result<(), String> {
        self.migration().ensure_ready()
    }

    fn runtime_package_kind(&self) -> RuntimePackageKind {
        if matches!(
            self.context.identity().package_kind,
            silo_engine::PackageKind::MacApp
        ) {
            RuntimePackageKind::MacApp
        } else {
            RuntimePackageKind::Other
        }
    }

    fn macos_computer_names(&self) -> Vec<String> {
        silo_engine::macos_computers::names(&self.macos_context())
    }

    /// The engine posts no OS notification (95 A-27); it keeps the latest notice per key.
    fn notify(&self, notice: silo_engine::notifications::Notice) {
        let result = serde_json::to_value(notice)
            .map_err(|error| error.to_string())
            .and_then(|notice| serde_json::from_value(notice).map_err(|error| error.to_string()))
            .and_then(|notice| self.context.retain_notice(&notice, Audience::All));
        if let Err(error) = result {
            eprintln!("silod: could not retain a computer notice: {error}");
        }
    }

    fn stop_macos_computers(&self, deadline: Option<Instant>) -> Result<(), String> {
        silo_engine::macos_computers::stop_all(&self.macos_context(), deadline)
    }
}

/// Windows belong to the engine's display host; a stop or a save never needs one.
struct ServicesMacosHost {
    runtime: ServicesRuntimeHost,
}

impl MacosHost for ServicesMacosHost {
    fn show_display(
        &self,
        _context: &MacosContext,
        _id: &str,
        _title: &str,
        _toolbar: bool,
    ) -> Result<(), String> {
        Err("The macOS computer display is unavailable in this engine host.".to_owned())
    }

    fn close_display(&self, _id: &str) {}

    fn linux_computer_names(&self) -> Vec<String> {
        silo_engine::runtime::computer_names(&self.runtime)
    }
}

/// How long the engine lets its last reply and events leave before the process exits.
pub const EXIT_DRAIN: Duration = Duration::from_millis(250);

/// Posts a retained failure notice for computers an update could not resume.
pub fn retain_resume_failure(context: &SilodContext, message: &str) {
    let notice = silo_engine::notifications::failure(
        "update:resume",
        "Computers couldn\u{2019}t resume after updating",
        message,
        None,
    );
    let result = serde_json::to_value(notice)
        .map_err(|error| error.to_string())
        .and_then(|notice: Value| serde_json::from_value(notice).map_err(|error| error.to_string()))
        .and_then(|notice| context.retain_notice(&notice, Audience::All));
    if let Err(error) = result {
        eprintln!("silod: could not retain the update resume notice: {error}");
    }
}
