use crate::health_watch::{HealthWatchHost, HealthWatchServices, Reading};
use crate::notifications::Notice;
use std::sync::Arc;
use tauri::AppHandle;

struct TauriHealthWatchServices {
    app: AppHandle,
}

struct TauriHealthWatchHost(AppHandle);

impl HealthWatchServices for TauriHealthWatchServices {
    fn observe(&self) -> Reading {
        crate::runtime::health_observations(&crate::commands::runtime::host(&self.app))
    }

    fn wait_for_computer_exit(&self, name: &str) -> bool {
        crate::runtime::wait_for_computer_exit(&crate::commands::runtime::host(&self.app), name)
    }

    fn notify(&self, notice: Notice) {
        crate::commands::notifications::notify(&self.app, notice);
    }
}

impl HealthWatchHost for TauriHealthWatchHost {
    fn services(&self) -> Arc<dyn HealthWatchServices> {
        Arc::new(TauriHealthWatchServices {
            app: self.0.clone(),
        })
    }
}

pub(crate) fn install(app: &AppHandle) {
    crate::health_watch::install(&TauriHealthWatchHost(app.clone()));
}
