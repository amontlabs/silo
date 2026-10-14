use std::sync::Arc;
use tauri::AppHandle;

use crate::{chatgpt_app, computer_use, runtime};

/// Reconcile every running computer after the published app or LCU changes.
pub(crate) fn reconcile(app: &AppHandle) {
    let Ok(paths) = runtime::runtime_paths(app) else {
        return;
    };
    let Ok(running) = runtime::update_recovery::running_names(app) else {
        return;
    };
    let runner: computer_use::SharedRunner = Arc::new(runtime::ProcessRunner);
    let handles = computer_use::reconcile_in(&runtime::OPERATIONS, &runner, &paths, &running);
    if handles.is_empty() {
        return;
    }
    std::thread::spawn(move || {
        for handle in handles {
            let _ = handle.join();
        }
        computer_use::publish_application_state_changed();
    });
}

/// The ChatGPT app became ready: running built-in computers set up computer use now instead
/// of at their next boot. Stopped computers do it when they start.
pub(crate) fn app_ready(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let Ok(paths) = runtime::runtime_paths(&app) else {
            return;
        };
        let Ok(running) = runtime::update_recovery::running_names(&app) else {
            return;
        };
        for name in running {
            let _ = computer_use::after_boot(Arc::new(runtime::ProcessRunner), &paths, &name);
        }
        computer_use::publish_application_state_changed();
    });
}

/// Prepares the shared folder, then downloads the pinned app in the background.
pub(crate) fn install(app: &AppHandle) {
    let Ok(root) = chatgpt_app::storage_root(app) else {
        return;
    };
    // Cheap (two directories), and a computer created right after launch needs it. A failure
    // here is retried before every preparation attempt and when a computer needs the folder.
    if let Err(error) = computer_use::register_published(&root) {
        eprintln!("ChatGPT app folder unavailable: {}", error.message);
    }
    // Verifying the app tree the first time reads every byte (seconds): off the main thread.
    let app = app.clone();
    std::thread::spawn(move || chatgpt_app::start_automatic(&app));
}
