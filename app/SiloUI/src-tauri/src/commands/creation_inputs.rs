//! Tauri adapter for the engine's creation wait service.
use crate::{
    chatgpt_app::{self, Status},
    creation_inputs::{self, ChatGpt, Inputs, InputsProvider, Step},
    runtime::{self, RuntimePaths},
};
use std::sync::Arc;

/// Installs the app-backed inputs used by computer creation waits.
pub(crate) fn install(app: &tauri::AppHandle) -> Result<(), String> {
    creation_inputs::install_provider(Arc::new(LiveProvider { app: app.clone() }))
}

struct LiveProvider {
    app: tauri::AppHandle,
}

impl InputsProvider for LiveProvider {
    fn create(&self, paths: &RuntimePaths, request_id: &str) -> Arc<dyn Inputs> {
        Arc::new(Live {
            app: self.app.clone(),
            paths: paths.clone(),
            request_id: request_id.to_owned(),
        })
    }
}

struct Live {
    app: tauri::AppHandle,
    paths: RuntimePaths,
    request_id: String,
}

impl From<Option<Status>> for ChatGpt {
    fn from(status: Option<Status>) -> Self {
        match status {
            None | Some(Status::Idle) => Self::Pending,
            Some(Status::Downloading {
                received_bytes,
                total_bytes,
            }) => Self::Downloading {
                received: received_bytes,
                total: total_bytes,
            },
            Some(Status::Verifying) => Self::Verifying,
            Some(Status::Extracting) => Self::Extracting,
            Some(Status::Ready { .. }) => Self::Ready,
            Some(Status::Failed { reason, .. }) => Self::Failed { reason },
        }
    }
}

impl Inputs for Live {
    fn ensure_image(&self) -> Result<(), String> {
        crate::preparation::ensure_image(&self.paths, &|_| {})
    }

    fn chatgpt(&self) -> ChatGpt {
        chatgpt_app::cached_status().into()
    }

    fn start_chatgpt(&self) {
        // The status is unknown before the first read of a process; start the download
        // worker unless the app is already published.
        let status = chatgpt_app::local_status(&self.app).ok();
        if !matches!(status, Some(Status::Ready { .. })) {
            chatgpt_app::ensure_in_background(&self.app);
        }
    }

    fn quitting(&self) -> bool {
        runtime::shutdown::ensure_accepting_operations().is_err()
    }

    fn show(&self, step: &Step) {
        runtime::publish_creation_wait(&self.request_id, step);
    }

    fn skipped(&self) -> bool {
        creation_inputs::was_skipped(&self.request_id)
    }
}
