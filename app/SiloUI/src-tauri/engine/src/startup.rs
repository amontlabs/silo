use serde_json::{Map, Value};
use silo_engine::{Audience, Event};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

#[derive(Default)]
pub struct StartupState {
    cancelled: AtomicBool,
    active: Mutex<()>,
}

impl StartupState {
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn cancel_and_wait(&self) {
        self.cancel();
        // Finish the current bounded start; do not launch the remaining selections.
        drop(self.active.lock());
    }
}

fn state() -> Option<Arc<StartupState>> {
    silo_engine::context::current().map(|context| context.runtime_state().startup().clone())
}

/// The result of resuming saved lifecycle actions, reduced to the values startup needs.
#[derive(Default)]
pub struct RecoveredActions {
    pub keep_stopped: HashSet<String>,
    pub failures: Vec<String>,
}

/// Host services used by the startup coordinator. Runtime and client adapters provide these
/// operations so this module owns the startup policy without owning a UI framework handle.
pub trait StartupServices: Send + Sync + 'static {
    fn blocks_runtime_operations(&self) -> bool;
    fn wait_for_recovery(&self) -> Result<(), String>;
    fn prepare_runtime(&self);
    fn recover_configuration(&self) -> Result<(), String>;
    /// Converts legacy macOS guests after backup and configuration recovery, before
    /// any saved lifecycle action or launch preference can start them.
    fn migrate_macos_proxy_bootstraps(&self) -> Result<(), String> {
        Ok(())
    }
    fn recover_lifecycle(&self) -> Result<RecoveredActions, String>;
    fn recover_update(&self) -> Result<bool, String>;
    fn current_launch_settings(&self) -> Result<(Map<String, Value>, PathBuf), String>;
    /// Local macOS computers share the startup ID preference with Linux computers.
    fn macos_computer_ids(&self) -> Vec<String> {
        Vec::new()
    }
    fn start_at_launch(&self, id: &str, cancelled: &AtomicBool) -> Result<(), String>;
    fn update_recovery_failed(&self, message: String);
    fn notify_failure(&self, key: &str, title: &str, body: &str);
    fn show_integration_error(&self, message: String);
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
    settings: Map<String, Value>,
    metadata: &Path,
    stopped: &HashSet<String>,
) -> Result<Map<String, Value>, String> {
    settings_for_launch_with_macos_ids(settings, metadata, &[], stopped)
}

fn settings_for_launch_with_macos_ids(
    mut settings: Map<String, Value>,
    metadata: &Path,
    macos_ids: &[String],
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
        let initial_id = initial
            .map(|configuration| configuration.id().to_owned())
            .or_else(|| macos_ids.first().cloned());
        settings.insert(
            "startupComputerIds".into(),
            serde_json::json!(initial_id.into_iter().collect::<Vec<_>>()),
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
            let mut available: HashSet<_> = crate::runtime::read_metadata(metadata)
                .map_err(|error| error.to_string())?
                .computers
                .into_iter()
                .map(|configuration| configuration.id().to_owned())
                .collect();
            available.extend(macos_ids.iter().cloned());
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
    result: Result<RecoveredActions, String>,
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
    mut start: impl FnMut(&str, &AtomicBool) -> Result<(), String>,
) -> Vec<String> {
    let mut failures = Vec::new();
    for id in selected_computers(settings) {
        if cancelled.load(Ordering::SeqCst) {
            break;
        }
        if let Err(error) = start(&id, cancelled) {
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

// Called once by native app setup, never by a webview mount, refresh or reopen.
pub fn install(state: Arc<StartupState>, services: Arc<dyn StartupServices>) {
    let Ok(_active) = state.active.lock() else {
        return;
    };
    if services.blocks_runtime_operations() {
        return;
    }
    if let Err(message) = services.wait_for_recovery() {
        services.show_integration_error(message);
        return;
    }
    if state.cancelled.load(Ordering::SeqCst) {
        return;
    }
    // Runtime-image repair and preparation are provided by the host adapter because they
    // still cross runtime-storage and client notification boundaries.
    services.prepare_runtime();
    if let Err(message) = services.recover_configuration() {
        services.notify_failure(
            "startup:setup",
            "Computer setup couldn\u{2019}t resume",
            &message,
        );
        services.show_integration_error(format!(
            "{message}\n\nSaved setup progress was preserved. Relaunch Silo to retry."
        ));
        return;
    }
    if let Err(message) = services.migrate_macos_proxy_bootstraps() {
        services.notify_failure(
            "startup:macos-proxy-bootstrap",
            "macOS network setup couldn’t finish",
            &message,
        );
        services.show_integration_error(format!(
            "{message}\n\nSilo kept the computer data and will retry this migration the next time it starts."
        ));
        return;
    }
    let (recovered_stops, failure) = after_lifecycle_recovery(services.recover_lifecycle());
    if let Some(message) = failure {
        services.notify_failure(
            "startup:actions",
            "Computer actions couldn\u{2019}t resume",
            &message,
        );
        services.show_integration_error(message);
    }
    match services.recover_update() {
        Ok(true) => return, // Preserve the exact pre-update running set, even if empty.
        Ok(false) => (),
        Err(message) => {
            services.update_recovery_failed(message);
            return;
        }
    }
    let Some(recovered_stops) = recovered_stops else {
        return;
    };
    let result = services
        .current_launch_settings()
        .and_then(|(settings, metadata)| {
            let macos_ids = services.macos_computer_ids();
            let settings = settings_for_launch_with_macos_ids(
                settings,
                &metadata,
                &macos_ids,
                &recovered_stops,
            )?;
            Ok(start_selected(
                &settings,
                &state.cancelled,
                |id, cancelled| {
                    let result = services.start_at_launch(id, cancelled);
                    if let Some(context) = silo_engine::context::current() {
                        context.publish(
                            Event::new("silo://application-state-changed", Value::Null),
                            Audience::All,
                        );
                    }
                    result
                },
            ))
        });
    let failures = result.unwrap_or_else(|error| vec![error]);
    if !failures.is_empty() && !state.cancelled.load(Ordering::SeqCst) {
        services.notify_failure(
            "startup:launch",
            &startup_failure_title(failures.len()),
            &failures.join(" "),
        );
        let message = format!("Some computers could not start automatically:\n\n{}\n\nOther selected computers may have started. Check their status and use Start to retry. Update the startup selection in Settings if needed.", failures.join("\n"));
        services.show_integration_error(message);
    }
}

pub fn is_cancelled<T>(_host: T) -> bool {
    state().is_some_and(|state| state.is_cancelled())
}

pub fn cancel<T>(_host: T) {
    if let Some(state) = state() {
        state.cancel();
    }
}

pub fn cancel_and_wait<T>(_host: T) {
    if let Some(state) = state() {
        state.cancel_and_wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[derive(Default)]
    struct StartupOrder(Mutex<Vec<&'static str>>);

    impl StartupServices for StartupOrder {
        fn blocks_runtime_operations(&self) -> bool {
            self.0.lock().unwrap().push("runtime-gate");
            false
        }

        fn wait_for_recovery(&self) -> Result<(), String> {
            self.0.lock().unwrap().push("backup-recovery");
            Ok(())
        }

        fn prepare_runtime(&self) {
            self.0.lock().unwrap().push("prepare-runtime");
        }

        fn recover_configuration(&self) -> Result<(), String> {
            self.0.lock().unwrap().push("configuration-recovery");
            Ok(())
        }

        fn migrate_macos_proxy_bootstraps(&self) -> Result<(), String> {
            self.0.lock().unwrap().push("proxy-migration");
            Ok(())
        }

        fn recover_lifecycle(&self) -> Result<RecoveredActions, String> {
            self.0.lock().unwrap().push("lifecycle-recovery");
            Ok(RecoveredActions::default())
        }

        fn recover_update(&self) -> Result<bool, String> {
            self.0.lock().unwrap().push("update-recovery");
            Ok(false)
        }

        fn current_launch_settings(&self) -> Result<(Map<String, Value>, PathBuf), String> {
            self.0.lock().unwrap().push("launch-settings");
            Ok((Map::new(), PathBuf::new()))
        }

        fn start_at_launch(&self, _: &str, _: &AtomicBool) -> Result<(), String> {
            panic!("empty launch settings must not start a computer")
        }

        fn update_recovery_failed(&self, _: String) {}

        fn notify_failure(&self, _: &str, _: &str, _: &str) {}

        fn show_integration_error(&self, _: String) {}
    }

    #[test]
    fn proxy_migration_runs_after_recovery_and_before_lifecycle_or_launch_start() {
        let services = Arc::new(StartupOrder::default());
        install(Arc::new(StartupState::default()), services.clone());
        assert_eq!(
            services.0.lock().unwrap().as_slice(),
            &[
                "runtime-gate",
                "backup-recovery",
                "prepare-runtime",
                "configuration-recovery",
                "proxy-migration",
                "lifecycle-recovery",
                "update-recovery",
                "launch-settings",
            ]
        );
    }

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
        let failures = start_selected(&resolved, &AtomicBool::new(false), |id, _cancelled| {
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
    fn startup_selection_keeps_local_macos_ids_and_uses_them_when_no_linux_computer_exists() {
        let directory = tempfile::tempdir().unwrap();
        let metadata = directory.path().join("computers.json");
        let mac_first = "00000000-0000-4000-8000-000000000011".to_string();
        let mac_second = "00000000-0000-4000-8000-000000000012".to_string();
        let linux = "00000000-0000-4000-8000-000000000013";
        std::fs::write(
            &metadata,
            json!({"schemaVersion":1,"computers":[computer(linux, "dev")]}).to_string(),
        )
        .unwrap();
        let settings = json!({
            "onboardingComplete":true,
            "startComputersAtLaunch":true,
            "startupComputerIds":[mac_second, "missing", linux, mac_first]
        });
        let resolved = settings_for_launch_with_macos_ids(
            settings.as_object().unwrap().clone(),
            &metadata,
            &[mac_first.clone(), mac_second.clone()],
            &HashSet::new(),
        )
        .unwrap();
        assert_eq!(
            selected_computers(&resolved),
            vec![mac_second.clone(), linux.to_string(), mac_first.clone()]
        );

        std::fs::write(
            &metadata,
            json!({"schemaVersion":1,"computers":[]}).to_string(),
        )
        .unwrap();
        let defaults = json!({"onboardingComplete":true,"startComputersAtLaunch":true});
        let resolved = settings_for_launch_with_macos_ids(
            defaults.as_object().unwrap().clone(),
            &metadata,
            &[mac_first.clone(), mac_second],
            &HashSet::new(),
        )
        .unwrap();
        assert_eq!(selected_computers(&resolved), vec![mac_first]);
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
        let (stops, failure) = after_lifecycle_recovery(Ok(RecoveredActions {
            keep_stopped: HashSet::from(["stopped".to_owned()]),
            failures: vec!["dev: The computer was replaced.".into()],
        }));
        assert_eq!(stops, Some(HashSet::from(["stopped".to_owned()])));
        assert!(failure.unwrap().contains("dev: The computer was replaced."));
        assert_eq!(
            after_lifecycle_recovery(Ok(RecoveredActions::default())),
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
        let failures = start_selected(
            settings.as_object().unwrap(),
            &cancelled,
            |id, _cancelled| {
                calls.push(id.to_owned());
                if id == "first" {
                    Err("First failed".into())
                } else {
                    cancelled.store(true, Ordering::SeqCst);
                    Ok(())
                }
            },
        );
        assert_eq!(calls, vec!["first", "second"]);
        assert_eq!(failures, vec!["First failed"]);
    }
}
