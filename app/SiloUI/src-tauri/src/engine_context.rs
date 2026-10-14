use silo_engine::{
    Audience, EngineContext, EngineIdentity, EnginePaths, EngineRuntimeState, Event, MainThread,
    PackageKind, ProcessControl,
};
use tauri::{AppHandle, Emitter, Manager};

#[derive(Clone)]
pub(crate) struct TauriEngineContext {
    app: AppHandle,
    paths: EnginePaths,
    identity: EngineIdentity,
    runtime_state: std::sync::Arc<EngineRuntimeState>,
}

fn dispatch_by_audience(
    audience: Audience,
    emit_to_all: impl FnOnce(),
    emit_to_client: impl FnOnce(&str),
) {
    match audience {
        Audience::All => emit_to_all(),
        Audience::Client(client_id) => emit_to_client(client_id.as_str()),
    }
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
            runtime_state: std::sync::Arc::new(EngineRuntimeState::default()),
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

    fn runtime_state(&self) -> std::sync::Arc<EngineRuntimeState> {
        self.runtime_state.clone()
    }

    fn publish(&self, event: Event, audience: Audience) {
        dispatch_by_audience(
            audience,
            || {
                let _ = self.app.emit(&event.name, &event.payload);
            },
            |client_id| {
                let _ = self.app.emit_to(client_id, &event.name, &event.payload);
            },
        );
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

    fn close_remote_displays(&self, device: Option<&str>) {
        match device {
            Some(device) => crate::commands::macos_remote_viewer::close_device(device),
            None => crate::commands::macos_remote_viewer::close_all(),
        }
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
    silo_engine::context::install(std::sync::Arc::new(context.clone()))?;
    app.manage(context);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    #[test]
    fn main_client_events_are_sent_only_to_the_main_window() {
        let broadcast = Cell::new(false);
        let client = RefCell::new(None);

        dispatch_by_audience(
            Audience::Client(silo_engine::ClientId::new("main")),
            || broadcast.set(true),
            |label| *client.borrow_mut() = Some(label.to_owned()),
        );

        assert!(!broadcast.get());
        let client = client.into_inner();
        assert_eq!(client.as_deref(), Some("main"));
    }
}
