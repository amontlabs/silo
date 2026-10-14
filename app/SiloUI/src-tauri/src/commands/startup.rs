//! Tauri setup adapter for the startup coordinator.
use crate::runtime;
use crate::startup::{RecoveredActions, StartupServices};
use serde_json::{Map, Value};
use std::sync::Arc;
use tauri::{AppHandle, Manager};

struct TauriStartupServices {
    app: AppHandle,
}

impl StartupServices for TauriStartupServices {
    fn blocks_runtime_operations(&self) -> bool {
        crate::runtime_migration::blocks_operations(&self.app)
    }

    fn wait_for_recovery(&self) -> Result<(), String> {
        crate::backup_controller::wait_for_recovery(&self.app)
    }

    fn prepare_runtime(&self) {
        if let Ok(paths) = runtime::runtime_paths(&self.app) {
            let storage = paths.storage_home.as_deref().unwrap_or(&paths.home);
            if let Err(message) = runtime::image_cache::repair(&storage.join("cache")) {
                eprintln!("Image cache repair: {message}");
            }
            crate::preparation::start(&self.app, paths);
        }
    }

    fn recover_configuration(&self) -> Result<(), String> {
        runtime::configuration_recovery::recover(&self.app)
    }

    fn recover_lifecycle(&self) -> Result<RecoveredActions, String> {
        runtime::lifecycle_recovery::recover(&self.app).map(|recovered| RecoveredActions {
            keep_stopped: recovered.keep_stopped,
            failures: recovered.failures,
        })
    }

    fn recover_update(&self) -> Result<bool, String> {
        runtime::update_recovery::recover(&self.app)
    }

    fn current_launch_settings(&self) -> Result<(Map<String, Value>, std::path::PathBuf), String> {
        let settings = crate::settings::current_settings(&self.app)?;
        let paths = runtime::runtime_paths(&self.app)?;
        Ok((settings, paths.metadata))
    }

    fn start_at_launch(&self, id: &str) -> Result<(), String> {
        runtime::start_at_launch(&self.app, id)
    }

    fn update_recovery_failed(&self, message: String) {
        crate::updates::recovery_failed(&self.app, message);
    }

    fn notify_failure(&self, key: &str, title: &str, body: &str) {
        crate::notifications::notify(
            &self.app,
            crate::notifications::failure(key, title, body, None),
        );
    }

    fn show_integration_error(&self, message: String) {
        if let Some(window) = self.app.get_webview_window("main") {
            let _ = crate::system_integrations::show_integration_error(
                self.app.clone(),
                window,
                message,
            );
        }
    }
}

pub(crate) fn install(app: &AppHandle) {
    let services: Arc<dyn StartupServices> = Arc::new(TauriStartupServices { app: app.clone() });
    tauri::async_runtime::spawn_blocking(move || crate::startup::install(services));
}
