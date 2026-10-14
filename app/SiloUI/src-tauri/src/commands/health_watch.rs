use crate::health_watch::{HealthWatchHost, HealthWatchServices, Reading};
use crate::notifications::Notice;
use std::sync::Arc;
use tauri::AppHandle;

struct TauriHealthWatchServices {
    app: AppHandle,
}

impl HealthWatchServices for TauriHealthWatchServices {
    fn observe(&self) -> Reading {
        crate::runtime::health_observations(&self.app)
    }

    fn wait_for_computer_exit(&self, name: &str) -> bool {
        crate::runtime::wait_for_computer_exit(&self.app, name)
    }

    fn notify(&self, notice: Notice) {
        crate::notifications::notify(&self.app, notice);
    }
}

impl HealthWatchHost for AppHandle {
    fn services(&self) -> Arc<dyn HealthWatchServices> {
        Arc::new(TauriHealthWatchServices { app: self.clone() })
    }
}
