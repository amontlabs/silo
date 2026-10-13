use silo_engine::{
    Audience, EngineContext, EngineIdentity, EnginePaths, Event, MainThread, PackageKind,
    ProcessControl,
};
use tauri::{AppHandle, Emitter, Manager};

pub(crate) struct TauriEngineContext {
    app: AppHandle,
    paths: EnginePaths,
    identity: EngineIdentity,
}

impl TauriEngineContext {
    pub(crate) fn from_app(app: &AppHandle) -> Result<Self, String> {
        let path = app.path();
        let paths = EnginePaths {
            app_data: path.app_data_dir().map_err(|error| error.to_string())?,
            app_config: path.app_config_dir().map_err(|error| error.to_string())?,
            app_cache: path.app_cache_dir().map_err(|error| error.to_string())?,
            app_log: path.app_log_dir().map_err(|error| error.to_string())?,
            resources: path.resource_dir().map_err(|error| error.to_string())?,
            home: path.home_dir().map_err(|error| error.to_string())?,
        };
        let identity = EngineIdentity {
            channel: silo_engine::channel::current(),
            version: app.package_info().version.to_string(),
            package_kind: if cfg!(target_os = "macos") {
                PackageKind::MacApp
            } else {
                PackageKind::Other
            },
        };

        Ok(Self {
            app: app.clone(),
            paths,
            identity,
        })
    }
}

impl EngineContext for TauriEngineContext {
    fn paths(&self) -> &EnginePaths {
        &self.paths
    }

    fn identity(&self) -> &EngineIdentity {
        &self.identity
    }

    fn publish(&self, event: Event, audience: Audience) {
        match audience {
            Audience::All => {
                let _ = self.app.emit(&event.name, &event.payload);
            }
            Audience::Client(client_id) => {
                let _ = self
                    .app
                    .emit_to(client_id.as_str(), &event.name, &event.payload);
            }
        }
    }

    fn main_thread(&self) -> Option<&dyn MainThread> {
        #[cfg(target_os = "macos")]
        {
            Some(self)
        }
        #[cfg(not(target_os = "macos"))]
        {
            None
        }
    }

    fn process(&self) -> &dyn ProcessControl {
        self
    }
}

impl MainThread for TauriEngineContext {
    fn run(&self, work: Box<dyn FnOnce() + Send>) -> Result<(), String> {
        self.app
            .run_on_main_thread(move || work())
            .map_err(|error| error.to_string())
    }
}

impl ProcessControl for TauriEngineContext {
    fn exit(&self, code: i32) {
        self.app.exit(code);
    }

    fn restart(&self) -> ! {
        self.app.restart()
    }
}

pub(crate) fn install(app: &mut tauri::App) -> Result<(), String> {
    let context = TauriEngineContext::from_app(app.handle())?;
    app.manage(context);
    Ok(())
}
