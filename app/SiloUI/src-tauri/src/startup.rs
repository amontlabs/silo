use serde_json::{Map, Value};
use std::{
    collections::HashSet,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
};
use tauri::{AppHandle, Emitter, Manager};

#[derive(Default)]
struct StartupState {
    cancelled: AtomicBool,
    active: Mutex<()>,
}

fn selected_computers(settings: &Map<String, Value>) -> Vec<String> {
    if settings.get("onboardingComplete").and_then(Value::as_bool) != Some(true)
        || settings
            .get("startComputersAtLaunch")
            .and_then(Value::as_bool)
            != Some(true)
    {
        return Vec::new();
    }
    let mut seen = HashSet::new();
    settings
        .get("startupComputerIds")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|id| !id.is_empty() && seen.insert((*id).to_owned()))
        .map(str::to_owned)
        .collect()
}

fn preserve_recovered_stops(settings: &mut Map<String, Value>, stopped: &HashSet<String>) {
    if let Some(ids) = settings
        .get_mut("startupComputerIds")
        .and_then(Value::as_array_mut)
    {
        ids.retain(|id| !id.as_str().is_some_and(|id| stopped.contains(id)));
    }
}

fn settings_for_launch(
    mut settings: Map<String, Value>,
    metadata: &Path,
    stopped: &HashSet<String>,
) -> Result<Map<String, Value>, String> {
    if settings.get("onboardingComplete").and_then(Value::as_bool) != Some(true)
        || settings
            .get("startComputersAtLaunch")
            .and_then(Value::as_bool)
            != Some(true)
    {
        return Ok(settings);
    }
    // Older switches saved only the opt-in, leaving the displayed default
    // selection absent on disk. Match that default without overriding an
    // explicit selection (including []) or inferring a remote launch target.
    if settings.get("onboardingComplete").and_then(Value::as_bool) == Some(true)
        && settings
            .get("startComputersAtLaunch")
            .and_then(Value::as_bool)
            == Some(true)
        && !settings.contains_key("startupComputerIds")
    {
        let configuration =
            crate::runtime::read_metadata(metadata).map_err(|error| error.to_string())?;
        let mut local = configuration.computers.iter();
        let initial = local
            .clone()
            .find(|configuration| configuration.name() == "dev")
            .or_else(|| local.next());
        settings.insert(
            "startupComputerIds".into(),
            serde_json::json!(initial
                .map(|configuration| configuration.id())
                .into_iter()
                .collect::<Vec<_>>()),
        );
    }
    preserve_recovered_stops(&mut settings, stopped);
    // A one-time migration may retain old computer IDs only in preserved
    // settings while selecting a clean runtime generation.
    if let Some(ids) = settings
        .get_mut("startupComputerIds")
        .and_then(Value::as_array_mut)
    {
        if !ids.is_empty() {
            let available: HashSet<_> = crate::runtime::read_metadata(metadata)
                .map_err(|error| error.to_string())?
                .computers
                .into_iter()
                .map(|configuration| configuration.id().to_owned())
                .collect();
            ids.retain(|id| id.as_str().is_some_and(|id| available.contains(id)));
        }
    }
    Ok(settings)
}

/// Launch continues after lifecycle recovery (D-23): actions that could not be
/// resumed are reported but never stop update recovery or automatic start. Only
/// when the saved actions could not be listed at all is automatic start withheld
/// (`None`), since one of them may be an explicit stop.
fn after_lifecycle_recovery(
    result: Result<crate::runtime::lifecycle_recovery::Recovered, String>,
) -> (Option<HashSet<String>>, Option<String>) {
    match result {
        Ok(recovered) => {
            let failure = (!recovered.failures.is_empty()).then(|| format!(
                "Some saved computer actions could not resume. Their progress was preserved.\n\n{}",
                recovered.failures.join("\n")
            ));
            (Some(recovered.keep_stopped), failure)
        }
        Err(message) => (
            None,
            Some(format!("{message}\n\nComputers selected to start at launch were not started. Start them manually.")),
        ),
    }
}

fn start_selected(
    settings: &Map<String, Value>,
    cancelled: &AtomicBool,
    mut start: impl FnMut(&str) -> Result<(), String>,
) -> Vec<String> {
    let mut failures = Vec::new();
    for id in selected_computers(settings) {
        if cancelled.load(Ordering::SeqCst) {
            break;
        }
        if let Err(error) = start(&id) {
            failures.push(error);
        }
    }
    failures
}

fn startup_failure_title(count: usize) -> String {
    match count {
        1 => "A computer couldn\u{2019}t start at launch".into(),
        n => format!("{n} computers couldn\u{2019}t start at launch"),
    }
}

/// Reports a panic in the launch sequence, which would otherwise end it silently.
struct PanicReport<'a>(&'a AppHandle);

impl Drop for PanicReport<'_> {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            return;
        }
        crate::notifications::notify(
            self.0,
            crate::notifications::failure(
                "startup:panic",
                "Silo couldn\u{2019}t finish starting",
                "Launch stopped unexpectedly. Computers selected to start at launch may not have started. Start them manually.",
                None,
            ),
        );
    }
}

// Called once by native app setup, never by a webview mount, refresh or reopen.
pub(crate) fn install(app: &AppHandle) {
    app.manage(StartupState::default());
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<StartupState>();
        let _active = state
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let _panic_report = PanicReport(&app);
        if crate::runtime_migration::blocks_operations(&app) {
            return;
        }
        if let Err(message) = crate::backup_controller::wait_for_recovery(&app) {
            if let Some(window) = app.get_webview_window("main") {
                let _ = crate::system_integrations::show_integration_error(
                    app.clone(),
                    window,
                    message,
                );
            }
            return;
        }
        if state.cancelled.load(Ordering::SeqCst) {
            return;
        }
        // Runtimes converted by an earlier Silo still name the previous runtime's image
        // files, so their computers stop booting once it is removed. Repair before any start.
        if let Ok(paths) = crate::runtime::runtime_paths(&app) {
            let storage = paths.storage_home.as_deref().unwrap_or(&paths.home);
            if let Err(message) = crate::runtime::image_cache::repair(&storage.join("cache")) {
                eprintln!("Image cache repair: {message}");
            }
            crate::preparation::start(&app, paths);
        }
        if let Err(message) = crate::runtime::configuration_recovery::recover(&app) {
            crate::notifications::notify(
                &app,
                crate::notifications::failure(
                    "startup:setup",
                    "Computer setup couldn\u{2019}t resume",
                    &message,
                    None,
                ),
            );
            if let Some(window) = app.get_webview_window("main") {
                let _ = crate::system_integrations::show_integration_error(
                    app.clone(),
                    window,
                    format!(
                        "{message}\n\nSaved setup progress was preserved. Relaunch Silo to retry."
                    ),
                );
            }
            return;
        }
        let (recovered_stops, failure) =
            after_lifecycle_recovery(crate::runtime::lifecycle_recovery::recover(&app));
        if let Some(message) = failure {
            crate::notifications::notify(
                &app,
                crate::notifications::failure(
                    "startup:actions",
                    "Computer actions couldn\u{2019}t resume",
                    &message,
                    None,
                ),
            );
            if let Some(window) = app.get_webview_window("main") {
                let _ = crate::system_integrations::show_integration_error(
                    app.clone(),
                    window,
                    message,
                );
            }
        }
        match crate::runtime::update_recovery::recover(&app) {
            Ok(true) => return, // Preserve the exact pre-update running set, even if empty.
            Ok(false) => (),
            Err(message) => {
                crate::updates::recovery_failed(&app, message);
                return;
            }
        }
        let Some(recovered_stops) = recovered_stops else {
            return;
        };
        let result = crate::settings::current_settings(&app).and_then(|settings| {
            let paths = crate::runtime::runtime_paths(&app)?;
            let settings = settings_for_launch(settings, &paths.metadata, &recovered_stops)?;
            Ok(start_selected(&settings, &state.cancelled, |id| {
                let result = crate::runtime::start_at_launch(&app, id);
                let _ = app.emit("silo://application-state-changed", ());
                result
            }))
        });
        let failures = result.unwrap_or_else(|error| vec![error]);
        if !failures.is_empty() && !state.cancelled.load(Ordering::SeqCst) {
            crate::notifications::notify(
                &app,
                crate::notifications::failure(
                    "startup:launch",
                    &startup_failure_title(failures.len()),
                    &failures.join(" "),
                    None,
                ),
            );
            if let Some(window) = app.get_webview_window("main") {
                let message = format!("Some computers could not start automatically:\n\n{}\n\nOther selected computers may have started. Check their status and use Start to retry. Update the startup selection in Settings if needed.", failures.join("\n"));
                let _ = crate::system_integrations::show_integration_error(
                    app.clone(),
                    window,
                    message,
                );
            }
        }
    });
}

pub(crate) fn is_cancelled(app: &AppHandle) -> bool {
    app.try_state::<StartupState>()
        .is_some_and(|state| state.cancelled.load(Ordering::SeqCst))
}

pub(crate) fn cancel(app: &AppHandle) {
    if let Some(state) = app.try_state::<StartupState>() {
        state.cancelled.store(true, Ordering::SeqCst);
    }
}

pub(crate) fn cancel_and_wait(app: &AppHandle) {
    if let Some(state) = app.try_state::<StartupState>() {
        state.cancelled.store(true, Ordering::SeqCst);
        // Finish the current bounded start; do not launch the remaining selections.
        drop(state.active.lock());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn computer(id: &str, name: &str) -> Value {
        json!({"id":id, "name":name, "cpus":2, "maxCPUs":2,
            "memoryGiB":2, "maxMemoryGiB":2, "workspaceStorageGiB":10, "runtimeStorageGiB":10})
    }

    #[test]
    fn enabled_startup_without_saved_ids_starts_the_displayed_default() {
        let directory = tempfile::tempdir().unwrap();
        let metadata = directory.path().join("computers.json");
        let id = "00000000-0000-4000-8000-000000000001";
        std::fs::write(
            &metadata,
            json!({"schemaVersion":1,"computers":[computer(id, "dev")]}).to_string(),
        )
        .unwrap();
        // This is the saved state produced by accepting the UI's default chip.
        let settings = json!({"onboardingComplete":true,"startComputersAtLaunch":true});
        let resolved = settings_for_launch(
            settings.as_object().unwrap().clone(),
            &metadata,
            &HashSet::new(),
        )
        .unwrap();
        let mut started = Vec::new();
        let failures = start_selected(&resolved, &AtomicBool::new(false), |id| {
            started.push(id.to_owned());
            Ok(())
        });
        assert!(failures.is_empty());
        assert_eq!(started, vec![id]);
    }

    #[test]
    fn missing_selection_prefers_dev_then_first_local_computer_and_preserves_recovered_stops() {
        let directory = tempfile::tempdir().unwrap();
        let metadata = directory.path().join("computers.json");
        let first = "00000000-0000-4000-8000-000000000001";
        let dev = "00000000-0000-4000-8000-000000000002";
        let settings = json!({"onboardingComplete":true,"startComputersAtLaunch":true});
        for (computers, expected) in [
            (
                vec![computer(first, "alpha"), computer(dev, "dev")],
                vec![dev],
            ),
            (vec![computer(first, "alpha")], vec![first]),
            (vec![], vec![]),
        ] {
            std::fs::write(
                &metadata,
                json!({"schemaVersion":1,"computers":computers}).to_string(),
            )
            .unwrap();
            let resolved = settings_for_launch(
                settings.as_object().unwrap().clone(),
                &metadata,
                &HashSet::new(),
            )
            .unwrap();
            assert_eq!(selected_computers(&resolved), expected);
            let stopped = expected.iter().map(|id| (*id).to_owned()).collect();
            let resolved =
                settings_for_launch(settings.as_object().unwrap().clone(), &metadata, &stopped)
                    .unwrap();
            assert!(selected_computers(&resolved).is_empty());
        }
    }

    #[test]
    fn explicit_selections_and_disabled_startup_do_not_read_default_configuration() {
        let directory = tempfile::tempdir().unwrap();
        let metadata = directory.path().join("computers.json");
        std::fs::write(
            &metadata,
            json!({"schemaVersion":1,"computers":[
                computer("00000000-0000-4000-8000-000000000001", "selected"),
                computer("00000000-0000-4000-8000-000000000002", "dev")
            ]})
            .to_string(),
        )
        .unwrap();
        for settings in [
            json!({"onboardingComplete":true,"startComputersAtLaunch":true,"startupComputerIds":[]}),
            json!({"onboardingComplete":true,"startComputersAtLaunch":true,"startupComputerIds":["00000000-0000-4000-8000-000000000001"]}),
        ] {
            let settings = settings.as_object().unwrap().clone();
            let resolved =
                settings_for_launch(settings.clone(), &metadata, &HashSet::new()).unwrap();
            assert_eq!(resolved, settings);
        }

        std::fs::write(&metadata, "invalid configuration").unwrap();
        for settings in [
            json!({"onboardingComplete":true,"startComputersAtLaunch":false}),
            json!({"onboardingComplete":false,"startComputersAtLaunch":true}),
            json!({}),
        ] {
            let settings = settings.as_object().unwrap().clone();
            let resolved =
                settings_for_launch(settings.clone(), &metadata, &HashSet::new()).unwrap();
            assert_eq!(resolved, settings);
        }
        let enabled = json!({"onboardingComplete":true,"startComputersAtLaunch":true});
        assert!(settings_for_launch(
            enabled.as_object().unwrap().clone(),
            &metadata,
            &HashSet::new()
        )
        .unwrap_err()
        .contains("configuration is invalid"));
    }

    #[test]
    fn recovered_explicit_stops_are_not_undone_by_launch_preferences() {
        let mut settings = serde_json::json!({"onboardingComplete":true,"startComputersAtLaunch":true,"startupComputerIds":["stopped","other"]}).as_object().unwrap().clone();
        preserve_recovered_stops(&mut settings, &HashSet::from(["stopped".into()]));
        assert_eq!(selected_computers(&settings), vec!["other"]);
    }

    #[test]
    fn failed_saved_actions_are_reported_without_stopping_launch() {
        use crate::runtime::lifecycle_recovery::Recovered;
        let (stops, failure) = after_lifecycle_recovery(Ok(Recovered {
            keep_stopped: HashSet::from(["stopped".to_owned()]),
            failures: vec!["dev: The computer was replaced.".into()],
        }));
        assert_eq!(stops, Some(HashSet::from(["stopped".to_owned()])));
        assert!(failure.unwrap().contains("dev: The computer was replaced."));
        assert_eq!(
            after_lifecycle_recovery(Ok(Recovered::default())),
            (Some(HashSet::new()), None)
        );
        // Without the list of saved actions an explicit stop could be among them.
        let (stops, failure) =
            after_lifecycle_recovery(Err("Saved computer actions could not be read.".into()));
        assert_eq!(stops, None);
        assert!(failure.unwrap().contains("were not started"));
    }

    #[test]
    fn startup_requires_completed_onboarding_and_explicit_opt_in() {
        for settings in [
            json!({}),
            json!({"onboardingComplete":false,"startComputersAtLaunch":true,"startupComputerIds":["dev"]}),
            json!({"onboardingComplete":true,"startComputersAtLaunch":false,"startupComputerIds":["dev"]}),
        ] {
            assert!(selected_computers(settings.as_object().unwrap()).is_empty());
        }
    }

    #[test]
    fn startup_uses_only_selected_ids_once_in_saved_order() {
        let settings = json!({"onboardingComplete":true,"startComputersAtLaunch":true,"startupComputerIds":["second","first","second"]});
        assert_eq!(
            selected_computers(settings.as_object().unwrap()),
            vec!["second", "first"]
        );
        let empty = json!({"onboardingComplete":true,"startComputersAtLaunch":true,"startupComputerIds":[]});
        assert!(selected_computers(empty.as_object().unwrap()).is_empty());
    }
    #[test]
    fn failed_start_does_not_block_other_selections_and_quit_stops_remaining() {
        let settings = json!({"onboardingComplete":true,"startComputersAtLaunch":true,"startupComputerIds":["first","second","third"]});
        let cancelled = AtomicBool::new(false);
        let mut calls = Vec::new();
        let failures = start_selected(settings.as_object().unwrap(), &cancelled, |id| {
            calls.push(id.to_owned());
            if id == "first" {
                Err("First failed".into())
            } else {
                cancelled.store(true, Ordering::SeqCst);
                Ok(())
            }
        });
        assert_eq!(calls, vec!["first", "second"]);
        assert_eq!(failures, vec!["First failed"]);
    }
}
