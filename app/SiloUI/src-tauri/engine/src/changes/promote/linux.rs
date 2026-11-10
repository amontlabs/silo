//! The Linux half of a promote: the runtime names each computer's sandbox after the computer, so
//! swapping the names renames both stopped sandboxes (`msb rename`), then the saved computer
//! names, the SSH entries that connect by name, the published ports (keyed by name), and puts
//! the fork's parked scheduled jobs back through the guest.
//!
//! Every method can be repeated, and `*_revert` leaves the device as it was whether or not the
//! step was applied. A sandbox is told apart from the other computers by the Silo identity it
//! carries (`silo.machine-id`), never by its name, because the names are what a crash can leave
//! half swapped.

use serde_json::{json, Value};

use super::journal::Swap;
use crate::{
    network,
    runtime::{self, checkpoints, RuntimePaths, RuntimeRunner},
};

/// Moves the fork's parked scheduled jobs back (`restore`) or parks them again (`repark`).
pub const QUIET_RESTORE: &str = include_str!("quiet_restore.sh");

/// What the Linux steps need from the device. The engine supplies the real runtime; a test
/// supplies a scripted one.
pub struct Linux<'a> {
    pub paths: &'a RuntimePaths,
    pub runner: &'a dyn RuntimeRunner,
    /// Starts or stops the computer called this (`start`, `stop`); the caller holds its lane.
    pub lifecycle: &'a dyn Fn(&str, &str) -> Result<(), String>,
    /// Runs a command as root in the running computer called this and returns its output.
    pub guest: &'a dyn Fn(&str, &[String]) -> Result<String, String>,
}

fn text(error: runtime::RuntimeError) -> String {
    error.to_string()
}

impl Linux<'_> {
    fn owner(&self, name: &str) -> Result<Option<String>, String> {
        runtime::sandbox_machine_id(self.runner, self.paths, name).map_err(text)
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), String> {
        runtime::rename_sandbox(self.runner, self.paths, from, to).map_err(text)
    }

    /// What the stores keep under the Linux computer `name` that a promote moves.
    pub fn export(&self, id: &str, name: &str) -> Result<Value, String> {
        Ok(json!({
            "ports": network::export_mappings(self.paths, name)?,
            "quiet": checkpoints::quiet_scheduled_jobs_disabled(self.paths, id)?
                .map_or(Value::Null, |disabled| json!({"scheduledJobsDisabled": disabled})),
        }))
    }

    /// The fork takes main's name and main takes the archive name: both sandboxes are renamed
    /// (main first, so the name is free), then the saved names, then the SSH entries.
    pub fn names_apply(&self, swap: &Swap) -> Result<(), String> {
        let (target, fork, archive) = (&swap.target_name, &swap.fork_name, &swap.archive_name);
        if self.owner(archive)?.is_none()
            && self.owner(target)?.as_deref() == Some(swap.target_id.as_str())
        {
            self.rename(target, archive)?;
        }
        if self.owner(target)?.is_none()
            && self.owner(fork)?.as_deref() == Some(swap.fork_id.as_str())
        {
            self.rename(fork, target)?;
        }
        if self.owner(target)?.as_deref() != Some(swap.fork_id.as_str())
            || self.owner(archive)?.as_deref() != Some(swap.target_id.as_str())
        {
            return Err(format!(
                "{target} and {archive} are not the computers the promote expected. Nothing more was changed."
            ));
        }
        runtime::set_computer_names(
            self.paths,
            &[(&swap.target_id, archive), (&swap.fork_id, target)],
        )
        .map_err(text)?;
        self.forget_runtime_state(swap);
        // A saved SSH entry keeps its id: main's now reaches the new main, the fork's the archive.
        crate::editor::repoint_id_alias(self.paths, &swap.target_id, target)?;
        crate::editor::repoint_id_alias(self.paths, &swap.fork_id, archive)
    }

    /// Puts both names back, in the reverse order.
    pub fn names_revert(&self, swap: &Swap) -> Result<(), String> {
        let (target, fork, archive) = (&swap.target_name, &swap.fork_name, &swap.archive_name);
        runtime::set_computer_names(
            self.paths,
            &[(&swap.target_id, target), (&swap.fork_id, fork)],
        )
        .map_err(text)?;
        if self.owner(fork)?.is_none()
            && self.owner(target)?.as_deref() == Some(swap.fork_id.as_str())
        {
            self.rename(target, fork)?;
        }
        if self.owner(target)?.is_none()
            && self.owner(archive)?.as_deref() == Some(swap.target_id.as_str())
        {
            self.rename(archive, target)?;
        }
        if self.owner(fork)?.as_deref() != Some(swap.fork_id.as_str())
            || self.owner(target)?.as_deref() != Some(swap.target_id.as_str())
        {
            return Err(format!(
                "{fork} and {target} did not return to the computers they were. Nothing more was changed."
            ));
        }
        self.forget_runtime_state(swap);
        crate::editor::repoint_id_alias(self.paths, &swap.target_id, target)?;
        crate::editor::repoint_id_alias(self.paths, &swap.fork_id, fork)
    }

    fn forget_runtime_state(&self, swap: &Swap) {
        for name in [&swap.target_name, &swap.fork_name, &swap.archive_name] {
            runtime::forget_name_state(self.paths, name);
        }
        if let Some(proxy) = crate::url_proxy::shared() {
            proxy.forget_computer_id(&swap.target_id);
            proxy.forget_computer_id(&swap.fork_id);
        }
    }

    /// Published ports are keyed by name, so main's stay with the name `main`, now the fork's;
    /// the fork's own go, like a removed computer's.
    pub fn ports_apply(&self, swap: &Swap) -> Result<(), String> {
        network::replace_mappings(self.paths, &swap.fork_name, &[])
    }

    /// Gives the fork its own ports back. Ports changed on the name while it was main stay
    /// with the name, which the old main has again.
    pub fn ports_revert(&self, swap: &Swap) -> Result<(), String> {
        let saved = swap
            .fork_bindings
            .get("ports")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        network::replace_mappings(self.paths, &swap.fork_name, &saved)
    }

    /// The computer that has the saved `id` now, by its current saved name.
    fn name_of(&self, id: &str) -> Result<String, String> {
        runtime::read_metadata(&self.paths.metadata)
            .map_err(text)?
            .computers
            .iter()
            .find(|computer| computer.id() == id)
            .map(|computer| computer.name().to_owned())
            .ok_or_else(|| "This computer no longer exists.".to_owned())
    }

    fn run_quiet(&self, name: &str, mode: &str) -> Result<String, String> {
        (self.lifecycle)("start", name)?;
        let ran = (self.guest)(
            name,
            &[
                "sh".into(),
                "-c".into(),
                QUIET_RESTORE.into(),
                "silo-promote-quiet".into(),
                mode.into(),
            ],
        );
        let stopped = (self.lifecycle)("stop", name);
        let output = ran?;
        stopped?;
        Ok(output)
    }

    /// Starts the new main, puts the parked scheduled jobs back except what it has replaced,
    /// and stops it again. Best effort: the caller keeps the promote when this fails.
    pub fn quiet_apply(&self, swap: &Swap) -> Result<(), String> {
        let parked = swap
            .fork_bindings
            .pointer("/quiet/scheduledJobsDisabled")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !parked {
            return Ok(());
        }
        // Recorded first: a crash from here on makes the undo park the jobs again, which finds
        // nothing to do when none were put back.
        checkpoints::set_quiet_scheduled_jobs_disabled(self.paths, &swap.fork_id, false)?;
        let output = self.run_quiet(&self.name_of(&swap.fork_id)?, "restore")?;
        // Nothing came back (the fork replaced it all): the jobs are still parked. The guest
        // reports its durable record of what it moved, not only what this run moved, so a run
        // that follows one that stopped half-way still sees the jobs that one put back.
        if !output.lines().any(|line| line.starts_with("recorded ")) {
            checkpoints::set_quiet_scheduled_jobs_disabled(self.paths, &swap.fork_id, true)?;
        }
        Ok(())
    }

    /// Parks again what the promote put back, and gives the fork its record back.
    pub fn quiet_revert(&self, swap: &Swap) -> Result<(), String> {
        let was_parked = swap
            .fork_bindings
            .pointer("/quiet/scheduledJobsDisabled")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !was_parked {
            return Ok(());
        }
        let now = checkpoints::quiet_scheduled_jobs_disabled(self.paths, &swap.fork_id)?;
        if now != Some(true) {
            self.run_quiet(&self.name_of(&swap.fork_id)?, "repark")?;
        }
        checkpoints::set_quiet_scheduled_jobs_disabled(self.paths, &swap.fork_id, true)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        sync::{Arc, Mutex},
        time::Duration,
    };

    use super::{
        super::{
            journal::{Journal, Step},
            promote, undo, Info, Outcome, PromoteEnv, Request,
        },
        *,
    };
    use crate::{
        changes::keep::{CheckpointRef, ManifestStore, Os},
        runtime::{
            CommandOutput, ComputerConfiguration, ComputerConfigurationRequest, RuntimeError,
        },
        test_support,
    };

    const MAIN: &str = "11111111-1111-4111-8111-111111111111";
    const FORK: &str = "22222222-2222-4222-8222-222222222222";

    /// A runtime whose computers are a name-to-identity table, as `msb inspect` and `msb rename`
    /// see them. A rename can be made to fail after a number of successes.
    #[derive(Default)]
    struct Msb {
        sandboxes: Mutex<BTreeMap<String, String>>,
        renames: Mutex<Vec<(String, String)>>,
        renames_left: Mutex<Option<usize>>,
    }

    impl Msb {
        fn with(sandboxes: &[(&str, &str)]) -> Self {
            let msb = Msb::default();
            for (name, id) in sandboxes {
                msb.sandboxes
                    .lock()
                    .unwrap()
                    .insert((*name).into(), (*id).into());
            }
            msb
        }

        fn names(&self) -> BTreeMap<String, String> {
            self.sandboxes.lock().unwrap().clone()
        }

        fn fail_renames_after(&self, successes: usize) {
            *self.renames_left.lock().unwrap() = Some(successes);
        }

        fn heal(&self) {
            *self.renames_left.lock().unwrap() = None;
        }
    }

    impl RuntimeRunner for Msb {
        fn run(
            &self,
            _: &RuntimePaths,
            args: &[String],
            _: Duration,
        ) -> Result<CommandOutput, RuntimeError> {
            let mut sandboxes = self.sandboxes.lock().unwrap();
            match args
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .as_slice()
            {
                ["inspect", name, "--format", "json"] => match sandboxes.get(*name) {
                    Some(id) => Ok(CommandOutput {
                        stdout: serde_json::json!({
                            "name": name, "status": "Stopped",
                            "config": {"labels": {"silo.managed": "true", "silo.machine-id": id}},
                        })
                        .to_string(),
                        stderr: String::new(),
                    }),
                    None => Err(RuntimeError::Failed {
                        operation: "inspect".into(),
                        exit_code: Some(1),
                        detail: format!("sandbox not found: {name}"),
                    }),
                },
                ["rename", from, to, "--quiet"] => {
                    let mut left = self.renames_left.lock().unwrap();
                    if let Some(remaining) = left.as_mut() {
                        if *remaining == 0 {
                            *left = None;
                            return Err(RuntimeError::Failed {
                                operation: "rename".into(),
                                exit_code: Some(1),
                                detail: "the runtime stopped".into(),
                            });
                        }
                        *remaining -= 1;
                    }
                    if sandboxes.contains_key(*to) {
                        return Err(RuntimeError::Failed {
                            operation: "rename".into(),
                            exit_code: Some(1),
                            detail: "sandbox already exists".into(),
                        });
                    }
                    let id = sandboxes
                        .remove(*from)
                        .expect("a rename of a missing sandbox");
                    sandboxes.insert((*to).into(), id);
                    self.renames
                        .lock()
                        .unwrap()
                        .push(((*from).into(), (*to).into()));
                    Ok(CommandOutput {
                        stdout: String::new(),
                        stderr: String::new(),
                    })
                }
                other => panic!("unexpected runtime command {other:?}"),
            }
        }
    }

    fn computer(id: &str, name: &str) -> ComputerConfiguration {
        ComputerConfiguration {
            id: id.into(),
            name: name.into(),
            cpus: 1,
            max_cpus: 1,
            memory_gib: 1,
            max_memory_gib: 1,
            workspace_storage_gib: 10,
            runtime_storage_gib: 10,
            runtime: None,
            desktop: None,
        }
    }

    fn saved_names(paths: &RuntimePaths) -> BTreeMap<String, String> {
        runtime::read_metadata(&paths.metadata)
            .unwrap()
            .computers
            .iter()
            .map(|computer| (computer.id().to_owned(), computer.name().to_owned()))
            .collect()
    }

    fn swap(paths: &RuntimePaths) -> Swap {
        let checkpoint = CheckpointRef {
            id: "cp".into(),
            name: "Before promote".into(),
            created_at: String::new(),
            scope: "disk".into(),
        };
        let mut swap = Swap {
            fork_id: FORK.into(),
            fork_name: "try-it".into(),
            target_id: MAIN.into(),
            target_name: "main".into(),
            archive_name: "main-before-20261105-1432".into(),
            macos: false,
            fork_bindings: Value::Null,
            target_bindings: Value::Null,
            fork_source: Some(MAIN.into()),
            target_source: None,
            checkpoint,
            fork_was_running: false,
            target_was_running: false,
            makers: Default::default(),
        };
        let none: &dyn Fn(&str, &str) -> Result<(), String> = &|_, _| Ok(());
        let guest: &dyn Fn(&str, &[String]) -> Result<String, String> = &|_, _| Ok(String::new());
        let runner = Msb::default();
        let linux = Linux {
            paths,
            runner: &runner,
            lifecycle: none,
            guest,
        };
        swap.fork_bindings = linux.export(FORK, "try-it").unwrap();
        swap.target_bindings = linux.export(MAIN, "main").unwrap();
        swap
    }

    type Lifecycle = Box<dyn Fn(&str, &str) -> Result<(), String> + Send + Sync>;
    type Guest = Box<dyn Fn(&str, &[String]) -> Result<String, String> + Send + Sync>;

    struct Device {
        dir: tempfile::TempDir,
        paths: RuntimePaths,
        msb: Msb,
        calls: Arc<Mutex<Vec<String>>>,
        output: Arc<Mutex<String>>,
        lifecycle: Lifecycle,
        guest: Guest,
    }

    impl Device {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let paths = test_support::paths(dir.path());
            runtime::write_metadata(
                &paths.metadata,
                &ComputerConfigurationRequest {
                    schema_version: 1,
                    computers: vec![computer(MAIN, "main"), computer(FORK, "try-it")],
                },
            )
            .unwrap();
            std::fs::write(
                dir.path().join("network.json"),
                serde_json::json!({"mappings": [
                    {"computer": "main", "port": 3000, "hostPort": null, "scheme": "http", "enabled": true},
                    {"computer": "main", "port": 5432, "hostPort": 15432, "scheme": null, "enabled": true},
                    {"computer": "try-it", "port": 8080, "hostPort": null, "scheme": "http", "enabled": true},
                ]})
                .to_string(),
            )
            .unwrap();
            let msb = Msb::with(&[("main", MAIN), ("try-it", FORK)]);
            let calls: Arc<Mutex<Vec<String>>> = Arc::default();
            let output: Arc<Mutex<String>> = Arc::default();
            let lifecycle: Lifecycle = {
                let calls = calls.clone();
                Box::new(move |action, name| {
                    calls.lock().unwrap().push(format!("{action} {name}"));
                    Ok(())
                })
            };
            let guest: Guest = {
                let (calls, output) = (calls.clone(), output.clone());
                Box::new(move |name, argv| {
                    calls
                        .lock()
                        .unwrap()
                        .push(format!("guest {name} {}", argv.last().unwrap()));
                    Ok(output.lock().unwrap().clone())
                })
            };
            Self {
                dir,
                paths,
                msb,
                calls,
                output,
                lifecycle,
                guest,
            }
        }

        fn linux(&self) -> Linux<'_> {
            Linux {
                paths: &self.paths,
                runner: &self.msb,
                lifecycle: &*self.lifecycle,
                guest: &*self.guest,
            }
        }

        fn ports(&self, name: &str) -> Vec<u16> {
            let mut ports: Vec<u16> = network::export_mappings(&self.paths, name)
                .unwrap()
                .iter()
                .map(|mapping| mapping["port"].as_u64().unwrap() as u16)
                .collect();
            ports.sort_unstable();
            ports
        }
    }

    fn names(device: &Device) -> (BTreeMap<String, String>, BTreeMap<String, String>) {
        (device.msb.names(), saved_names(&device.paths))
    }

    #[test]
    fn the_names_swap_in_the_runtime_and_the_saved_computers_and_swap_back() {
        let device = Device::new();
        let swap = swap(&device.paths);
        let before = names(&device);
        device.linux().names_apply(&swap).unwrap();
        let (runtime_names, saved) = names(&device);
        assert_eq!(runtime_names["main"], FORK);
        assert_eq!(runtime_names["main-before-20261105-1432"], MAIN);
        assert!(!runtime_names.contains_key("try-it"));
        assert_eq!(saved[FORK], "main");
        assert_eq!(saved[MAIN], "main-before-20261105-1432");
        // Main is renamed first, so the name is free for the fork.
        assert_eq!(
            device.msb.renames.lock().unwrap().first().unwrap().1,
            "main-before-20261105-1432"
        );

        device.linux().names_revert(&swap).unwrap();
        assert_eq!(names(&device), before);
    }

    #[test]
    fn repeating_a_names_step_or_its_revert_changes_nothing() {
        let device = Device::new();
        let swap = swap(&device.paths);
        let initial = names(&device);
        device.linux().names_revert(&swap).unwrap();
        assert_eq!(names(&device), initial);
        assert!(device.msb.renames.lock().unwrap().is_empty());

        device.linux().names_apply(&swap).unwrap();
        let applied = names(&device);
        let renames = device.msb.renames.lock().unwrap().len();
        device.linux().names_apply(&swap).unwrap();
        assert_eq!(names(&device), applied);
        assert_eq!(device.msb.renames.lock().unwrap().len(), renames);

        device.linux().names_revert(&swap).unwrap();
        device.linux().names_revert(&swap).unwrap();
        assert_eq!(names(&device), initial);
    }

    #[test]
    fn a_stop_after_any_rename_is_finished_by_repeating_or_undone_by_the_revert() {
        for survived in 0..2 {
            // Repeating the step finishes it.
            let device = Device::new();
            let swap = swap(&device.paths);
            let initial = names(&device);
            device.msb.fail_renames_after(survived);
            assert!(device.linux().names_apply(&swap).is_err());
            assert_eq!(names(&device) != initial, survived > 0);
            device.msb.heal();
            device.linux().names_apply(&swap).unwrap();
            assert_eq!(device.msb.names()["main"], FORK);
            assert_eq!(saved_names(&device.paths)[FORK], "main");

            // The revert puts everything back from the same half-way state.
            let device = Device::new();
            let swap = super::tests::swap(&device.paths);
            device.msb.fail_renames_after(survived);
            assert!(device.linux().names_apply(&swap).is_err());
            device.msb.heal();
            device.linux().names_revert(&swap).unwrap();
            assert_eq!(names(&device), initial);
        }
    }

    #[test]
    fn a_computer_that_is_not_the_expected_one_stops_the_step() {
        let device = Device::new();
        let swap = swap(&device.paths);
        device
            .msb
            .sandboxes
            .lock()
            .unwrap()
            .insert("main".into(), "33333333-3333-4333-8333-333333333333".into());
        let error = device.linux().names_apply(&swap).unwrap_err();
        assert!(
            error.contains("not the computers the promote expected"),
            "{error}"
        );
        assert_eq!(saved_names(&device.paths)[MAIN], "main");
        assert!(device.msb.renames.lock().unwrap().is_empty());
    }

    #[test]
    fn published_ports_stay_with_the_name_and_the_forks_own_come_back() {
        let device = Device::new();
        let swap = swap(&device.paths);
        assert_eq!(device.ports("main"), [3000, 5432]);
        assert_eq!(device.ports("try-it"), [8080]);

        device.linux().names_apply(&swap).unwrap();
        device.linux().ports_apply(&swap).unwrap();
        // Main's ports belong to the name `main`, which the fork has; the fork's own are gone.
        assert_eq!(device.ports("main"), [3000, 5432]);
        assert_eq!(device.ports("try-it"), Vec::<u16>::new());
        device.linux().ports_apply(&swap).unwrap();
        assert_eq!(device.ports("main"), [3000, 5432]);

        // A port published on the name while it was main goes back to the old main with it.
        let mut changed = network::export_mappings(&device.paths, "main").unwrap();
        changed.truncate(1);
        network::replace_mappings(&device.paths, "main", &changed).unwrap();

        device.linux().ports_revert(&swap).unwrap();
        device.linux().names_revert(&swap).unwrap();
        assert_eq!(device.ports("main"), [3000]);
        assert_eq!(device.ports("try-it"), [8080]);
        device.linux().ports_revert(&swap).unwrap();
        assert_eq!(device.ports("try-it"), [8080]);
    }

    #[test]
    fn a_quiet_fork_is_started_restored_through_the_guest_and_stopped() {
        let device = Device::new();
        checkpoints::quiet_for_test(&device.paths, FORK, true);
        let swap = swap(&device.paths);
        device.linux().names_apply(&swap).unwrap();
        *device.output.lock().unwrap() =
            "restored /etc/cron.d/poll\nrecorded done file /etc/cron.d/poll\n".into();

        device.linux().quiet_apply(&swap).unwrap();
        assert_eq!(
            *device.calls.lock().unwrap(),
            ["start main", "guest main restore", "stop main"]
        );
        // The fork is no longer quiet about scheduled jobs.
        assert_eq!(
            checkpoints::quiet_scheduled_jobs_disabled(&device.paths, FORK).unwrap(),
            Some(false)
        );

        device.calls.lock().unwrap().clear();
        device.linux().quiet_revert(&swap).unwrap();
        assert_eq!(
            *device.calls.lock().unwrap(),
            ["start main", "guest main repark", "stop main"]
        );
        assert_eq!(
            checkpoints::quiet_scheduled_jobs_disabled(&device.paths, FORK).unwrap(),
            Some(true)
        );

        // Reverting a promote that never restored anything boots nothing.
        device.calls.lock().unwrap().clear();
        device.linux().quiet_revert(&swap).unwrap();
        assert!(device.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn a_retry_after_a_stop_in_the_middle_still_sees_what_the_first_run_restored() {
        let device = Device::new();
        checkpoints::quiet_for_test(&device.paths, FORK, true);
        let swap = swap(&device.paths);
        device.linux().names_apply(&swap).unwrap();
        // The first run moved a job and stopped before the engine recorded it; the retry finds
        // nothing left to move, but the guest's record still names the job.
        *device.output.lock().unwrap() = "recorded pending file /etc/cron.d/poll\n".into();
        device.linux().quiet_apply(&swap).unwrap();
        assert_eq!(
            checkpoints::quiet_scheduled_jobs_disabled(&device.paths, FORK).unwrap(),
            Some(false)
        );
        // So the undo parks it again.
        device.calls.lock().unwrap().clear();
        device.linux().quiet_revert(&swap).unwrap();
        assert_eq!(
            *device.calls.lock().unwrap(),
            ["start main", "guest main repark", "stop main"]
        );
    }

    #[test]
    fn nothing_is_started_for_a_fork_that_was_not_quiet_or_that_kept_everything_it_replaced() {
        let device = Device::new();
        let swap = swap(&device.paths);
        device.linux().quiet_apply(&swap).unwrap();
        device.linux().quiet_revert(&swap).unwrap();
        assert!(device.calls.lock().unwrap().is_empty());

        let device = Device::new();
        checkpoints::quiet_for_test(&device.paths, FORK, true);
        let swap = super::tests::swap(&device.paths);
        device.linux().names_apply(&swap).unwrap();
        *device.output.lock().unwrap() = "left /etc/cron.d/poll\n".into();
        device.linux().quiet_apply(&swap).unwrap();
        // Everything stayed parked: the record says so, and an undo has nothing to park again.
        assert_eq!(
            checkpoints::quiet_scheduled_jobs_disabled(&device.paths, FORK).unwrap(),
            Some(true)
        );
        device.calls.lock().unwrap().clear();
        device.linux().quiet_revert(&swap).unwrap();
        assert!(device.calls.lock().unwrap().is_empty());
        let _ = device.dir.path();
    }

    /// The promote engine over the Linux steps and a fake runtime, as the real environment wires
    /// them (the secret and GitHub stores are not part of this device).
    struct LinuxEnv<'a>(&'a Device);

    impl PromoteEnv for LinuxEnv<'_> {
        fn info(&self, id: &str) -> Result<Info, String> {
            let name = saved_names(&self.0.paths)
                .remove(id)
                .ok_or("This computer no longer exists.")?;
            Ok(Info {
                id: id.into(),
                name,
                os: Os::Linux,
                running: false,
                busy: None,
            })
        }
        fn name_in_use(&self, name: &str) -> bool {
            saved_names(&self.0.paths)
                .values()
                .any(|taken| taken == name)
        }
        fn stamp(&self) -> String {
            "20261105-1432".into()
        }
        fn checkpoint(&self, _: &str, name: &str) -> Result<CheckpointRef, String> {
            Ok(CheckpointRef {
                id: "cp".into(),
                name: name.into(),
                created_at: String::new(),
                scope: "disk".into(),
            })
        }
        fn stop(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn export_bindings(&self, id: &str, name: &str) -> Result<Value, String> {
            self.0.linux().export(id, name)
        }
        fn apply(&self, step: Step, swap: &Swap) -> Result<(), String> {
            let linux = self.0.linux();
            match step {
                Step::Names => linux.names_apply(swap),
                Step::Bindings => linux.ports_apply(swap),
                Step::Quiet => linux.quiet_apply(swap),
                _ => Ok(()),
            }
        }
        fn revert(&self, step: Step, swap: &Swap) -> Result<(), String> {
            let linux = self.0.linux();
            match step {
                Step::Names => linux.names_revert(swap),
                Step::Bindings => linux.ports_revert(swap),
                Step::Quiet => linux.quiet_revert(swap),
                _ => Ok(()),
            }
        }
    }

    struct Forks;

    impl ManifestStore for Forks {
        fn fork_time(&self, _: &str) -> Result<String, String> {
            Ok(String::new())
        }
        fn capture(&self, _: &str) -> Result<String, String> {
            Ok(String::new())
        }
        fn source(&self, fork: &str) -> Result<Option<String>, String> {
            Ok((fork == FORK).then(|| MAIN.to_owned()))
        }
    }

    fn promote_request() -> Request {
        Request {
            fork: FORK.into(),
            target: None,
            dry_run: false,
        }
    }

    #[test]
    fn a_linux_promote_swaps_the_computers_and_its_undo_swaps_them_back() {
        let device = Device::new();
        checkpoints::quiet_for_test(&device.paths, FORK, true);
        *device.output.lock().unwrap() =
            "restored /etc/cron.d/poll\nrecorded done file /etc/cron.d/poll\n".into();
        let journal_dir = tempfile::tempdir().unwrap();
        let journal = Journal::new(journal_dir.path().join("promote"));
        let env = LinuxEnv(&device);
        let before = (names(&device), device.ports("main"), device.ports("try-it"));

        let outcome = promote(
            &env,
            &Forks,
            &journal,
            &promote_request(),
            Duration::from_secs(1),
        )
        .unwrap();
        let Outcome::Promoted(report) = outcome else {
            panic!("not promoted: {outcome:?}");
        };
        assert_eq!(report.archive, "main-before-20261105-1432");
        assert_eq!(device.msb.names()["main"], FORK);
        assert_eq!(saved_names(&device.paths)[FORK], "main");
        assert_eq!(device.ports("main"), [3000, 5432]);
        assert!(device.ports("try-it").is_empty());
        assert_eq!(
            *device.calls.lock().unwrap(),
            ["start main", "guest main restore", "stop main"]
        );

        let undone = undo(&env, &journal, FORK, Duration::from_secs(1))
            .unwrap()
            .unwrap();
        assert_eq!(undone.computer, "main");
        assert_eq!(undone.fork, "try-it");
        assert_eq!(
            (names(&device), device.ports("main"), device.ports("try-it")),
            before
        );
        assert_eq!(
            checkpoints::quiet_scheduled_jobs_disabled(&device.paths, FORK).unwrap(),
            Some(true)
        );
        assert_eq!(device.calls.lock().unwrap().last().unwrap(), "stop main");
    }

    #[test]
    fn a_linux_promote_interrupted_by_a_stop_is_finished_at_the_next_start() {
        let device = Device::new();
        let journal_dir = tempfile::tempdir().unwrap();
        let journal = Journal::new(journal_dir.path().join("promote"));
        let env = LinuxEnv(&device);
        let before = names(&device);
        // The runtime stops answering after the first rename: the promote rolls itself back.
        device.msb.fail_renames_after(1);
        let error = promote(
            &env,
            &Forks,
            &journal,
            &promote_request(),
            Duration::from_secs(1),
        )
        .unwrap_err();
        assert!(error.contains("rolled back"), "{error}");
        assert_eq!(names(&device), before);
        assert!(super::super::interrupted_keys(&journal).is_empty());
        device.msb.heal();
        assert!(matches!(
            promote(
                &env,
                &Forks,
                &journal,
                &promote_request(),
                Duration::from_secs(1)
            )
            .unwrap(),
            Outcome::Promoted(_)
        ));
    }

    /// Runs the guest script against a directory standing in for the guest's root.
    fn guest_script(root: &std::path::Path, mode: &str) -> String {
        let output = std::process::Command::new("sh")
            .args(["-c", QUIET_RESTORE, "silo-promote-quiet", mode])
            .env("SILO_ROOT", root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    fn put(root: &std::path::Path, relative: &str, text: &str) {
        let path = root.join(relative.trim_start_matches('/'));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn exists(root: &std::path::Path, relative: &str) -> bool {
        std::fs::symlink_metadata(root.join(relative.trim_start_matches('/'))).is_ok()
    }

    fn parked_fork() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let parked = "var/lib/silo-quiet/disabled";
        put(
            root,
            &format!("{parked}/etc/cron.d/poll"),
            "* * * * * root poll\n",
        );
        put(
            root,
            &format!("{parked}/var/spool/cron/crontabs/silo"),
            "0 * * * * run\n",
        );
        put(
            root,
            &format!("{parked}/etc/systemd/system/backup.timer"),
            "[Timer]\n",
        );
        put(
            root,
            &format!("{parked}/etc/crontab"),
            "# comments only\n17 * * * * root run-parts\n",
        );
        // A service the fork was asked to stop is not a scheduled job.
        put(
            root,
            &format!("{parked}/etc/systemd/system/web.service"),
            "[Service]\n",
        );
        // The quiet step masked the timer by name and left comments in /etc/crontab.
        std::fs::create_dir_all(root.join("etc/systemd/system")).unwrap();
        std::os::unix::fs::symlink("/dev/null", root.join("etc/systemd/system/backup.timer"))
            .unwrap();
        std::os::unix::fs::symlink("/dev/null", root.join("etc/systemd/system/apt-daily.timer"))
            .unwrap();
        put(root, "usr/lib/systemd/system/apt-daily.timer", "[Timer]\n");
        put(root, "etc/crontab", "# comments only\n");
        // A mask made before the quiet step and one made after it, each with a vendor unit.
        for unit in ["before-fork.timer", "after-quiet.timer"] {
            std::os::unix::fs::symlink("/dev/null", root.join("etc/systemd/system").join(unit))
                .unwrap();
            put(root, &format!("usr/lib/systemd/system/{unit}"), "[Timer]\n");
        }
        // The quiet step listed the masks it made; the other two are not on its list.
        put(
            root,
            "var/lib/silo-quiet/0a1b.masked-timers.list",
            "/etc/systemd/system/backup.timer\n/etc/systemd/system/apt-daily.timer\n",
        );
        dir
    }

    #[test]
    fn the_guest_script_puts_scheduled_jobs_back_and_parks_them_again() {
        let dir = parked_fork();
        let root = dir.path();
        let output = guest_script(root, "restore");
        assert!(output.contains("restored /etc/cron.d/poll"), "{output}");
        assert_eq!(
            std::fs::read_to_string(root.join("etc/cron.d/poll")).unwrap(),
            "* * * * * root poll\n"
        );
        assert!(exists(root, "/var/spool/cron/crontabs/silo"));
        assert!(
            std::fs::read_to_string(root.join("etc/systemd/system/backup.timer"))
                .unwrap()
                .contains("[Timer]")
        );
        assert!(std::fs::read_to_string(root.join("etc/crontab"))
            .unwrap()
            .contains("run-parts"));
        // A timer the quiet step only masked is unmasked; a mask from before the fork or from
        // after the quiet step stays; the stopped service stays parked.
        assert!(!exists(root, "/etc/systemd/system/apt-daily.timer"));
        assert!(exists(root, "/etc/systemd/system/before-fork.timer"));
        assert!(exists(root, "/etc/systemd/system/after-quiet.timer"));
        assert!(exists(
            root,
            "/var/lib/silo-quiet/disabled/etc/systemd/system/web.service"
        ));
        assert!(!exists(root, "/etc/systemd/system/web.service"));
        // Repeating it finds nothing more to move but still reports what was put back.
        let again = guest_script(root, "restore");
        assert!(!again.contains("restored"), "{again}");
        assert!(
            again.contains("recorded done file /etc/cron.d/poll"),
            "{again}"
        );

        guest_script(root, "repark");
        assert!(!exists(root, "/etc/cron.d/poll"));
        assert!(exists(root, "/var/lib/silo-quiet/disabled/etc/cron.d/poll"));
        assert_eq!(
            std::fs::read_link(root.join("etc/systemd/system/backup.timer")).unwrap(),
            std::path::Path::new("/dev/null")
        );
        assert!(exists(
            root,
            "/var/lib/silo-quiet/disabled/etc/systemd/system/backup.timer"
        ));
        assert_eq!(
            std::fs::read_link(root.join("etc/systemd/system/apt-daily.timer")).unwrap(),
            std::path::Path::new("/dev/null")
        );
        assert_eq!(
            std::fs::read_to_string(root.join("etc/crontab"))
                .unwrap()
                .trim(),
            "# comments only"
        );
        assert!(exists(root, "/var/lib/silo-quiet/disabled/etc/crontab"));
        assert!(!exists(root, "/var/lib/silo-quiet/restored.list"));
        // The masks the quiet step did not make were not touched by either direction.
        assert!(exists(root, "/etc/systemd/system/before-fork.timer"));
        assert!(exists(root, "/etc/systemd/system/after-quiet.timer"));
        guest_script(root, "repark");
    }

    /// What the quiet step leaves on an Ubuntu computer whose main was itself made quiet: the
    /// vendor job, the user's own job and the root crontab, all under the saved originals.
    #[test]
    fn the_jobs_of_a_real_ubuntu_fork_come_back_on_the_new_main() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let parked = "var/lib/silo-quiet/disabled";
        put(
            root,
            &format!("{parked}/etc/cron.d/e2scrub_all"),
            "30 3 * * 0 root e2scrub_all_cron\n",
        );
        put(
            root,
            &format!("{parked}/etc/cron.d/p3-job"),
            "* * * * * root echo p3cron\n",
        );
        put(
            root,
            &format!("{parked}/var/spool/cron/crontabs/root"),
            "*/5 * * * * echo p3root >> /var/log/p3root.log\n",
        );
        put(
            root,
            &format!("{parked}/etc/crontab"),
            "# m h dom mon dow user command\n17 * * * * root cd / && run-parts --report /etc/cron.hourly\n",
        );
        put(
            root,
            &format!("{parked}/etc/anacrontab"),
            "1 5 cron.daily run-parts /etc/cron.daily\n",
        );
        put(root, "etc/crontab", "# m h dom mon dow user command\n");
        std::fs::create_dir_all(root.join("etc/cron.d")).unwrap();
        std::fs::create_dir_all(root.join("var/spool/cron/crontabs")).unwrap();

        let output = guest_script(root, "restore");
        for path in [
            "/etc/cron.d/e2scrub_all",
            "/etc/cron.d/p3-job",
            "/var/spool/cron/crontabs/root",
            "/etc/anacrontab",
        ] {
            assert!(
                output.contains(&format!("restored {path}")),
                "{path}: {output}"
            );
            assert!(exists(root, path), "{path}");
            assert!(
                !exists(root, &format!("/var/lib/silo-quiet/disabled{path}")),
                "{path}"
            );
        }
        assert!(
            std::fs::read_to_string(root.join("var/spool/cron/crontabs/root"))
                .unwrap()
                .contains("p3root")
        );
        assert!(std::fs::read_to_string(root.join("etc/crontab"))
            .unwrap()
            .contains("run-parts"));

        guest_script(root, "repark");
        for path in ["/etc/cron.d/p3-job", "/var/spool/cron/crontabs/root"] {
            assert!(!exists(root, path), "{path}");
            assert!(
                exists(root, &format!("/var/lib/silo-quiet/disabled{path}")),
                "{path}"
            );
        }
    }

    fn marker_of(root: &std::path::Path) -> String {
        std::fs::read_to_string(root.join("var/lib/silo-quiet/restored.list")).unwrap_or_default()
    }

    #[test]
    fn every_change_is_recorded_before_it_is_made_and_marked_when_done() {
        let dir = parked_fork();
        let root = dir.path();
        guest_script(root, "restore");
        let marker = marker_of(root);
        for item in [
            "file /etc/cron.d/poll",
            "file /var/spool/cron/crontabs/silo",
            "mask /etc/systemd/system/backup.timer",
            "crontab /etc/crontab",
            "unmask /etc/systemd/system/apt-daily.timer",
        ] {
            let pending = marker.find(&format!("pending {item}\n")).expect(item);
            let done = marker.find(&format!("done {item}\n")).expect(item);
            assert!(pending < done, "{item}");
        }
    }

    #[test]
    fn a_run_that_stopped_after_moving_an_item_reports_it_and_the_undo_parks_it() {
        let dir = parked_fork();
        let root = dir.path();
        // The job was moved and the run stopped before the mark: the intent is on record, the
        // parked copy is gone.
        std::fs::rename(
            root.join("var/lib/silo-quiet/disabled/etc/cron.d/poll"),
            root.join("etc/cron.d/poll"),
        )
        .unwrap_or_else(|_| {
            put(root, "etc/cron.d/poll", "* * * * * root poll\n");
            std::fs::remove_file(root.join("var/lib/silo-quiet/disabled/etc/cron.d/poll")).unwrap();
        });
        put(
            root,
            "var/lib/silo-quiet/restored.list",
            "pending file /etc/cron.d/poll\n",
        );
        let output = guest_script(root, "restore");
        assert!(!output.contains("restored /etc/cron.d/poll"), "{output}");
        assert!(
            output.contains("recorded pending file /etc/cron.d/poll"),
            "{output}"
        );
        assert!(exists(root, "/etc/cron.d/poll"));
        guest_script(root, "repark");
        assert!(!exists(root, "/etc/cron.d/poll"));
        assert!(exists(root, "/var/lib/silo-quiet/disabled/etc/cron.d/poll"));
    }

    #[test]
    fn a_run_that_stopped_before_moving_an_item_finishes_it() {
        let dir = parked_fork();
        let root = dir.path();
        put(
            root,
            "var/lib/silo-quiet/restored.list",
            "pending mask /etc/systemd/system/backup.timer\n",
        );
        let output = guest_script(root, "restore");
        assert!(
            output.contains("restored /etc/systemd/system/backup.timer"),
            "{output}"
        );
        assert!(
            std::fs::read_to_string(root.join("etc/systemd/system/backup.timer"))
                .unwrap()
                .contains("[Timer]")
        );
        // Parking again puts the mask back over the file it replaced.
        guest_script(root, "repark");
        assert_eq!(
            std::fs::read_link(root.join("etc/systemd/system/backup.timer")).unwrap(),
            std::path::Path::new("/dev/null")
        );
    }

    #[test]
    fn a_crontab_written_before_a_stop_is_finished_not_reported_as_replaced() {
        let dir = parked_fork();
        let root = dir.path();
        // The saved copy was written back and the run stopped before deleting it.
        let saved = "# comments only\n17 * * * * root run-parts\n";
        put(root, "etc/crontab", saved);
        put(
            root,
            "var/lib/silo-quiet/restored.list",
            "pending crontab /etc/crontab\n",
        );
        let output = guest_script(root, "restore");
        assert!(output.contains("restored /etc/crontab"), "{output}");
        assert!(!output.contains("left /etc/crontab"), "{output}");
        assert!(!exists(root, "/var/lib/silo-quiet/disabled/etc/crontab"));
        guest_script(root, "repark");
        assert_eq!(
            std::fs::read_to_string(root.join("etc/crontab"))
                .unwrap()
                .trim(),
            "# comments only"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("var/lib/silo-quiet/disabled/etc/crontab")).unwrap(),
            saved
        );
    }

    #[test]
    fn a_mask_the_quiet_step_did_not_list_is_never_removed() {
        let dir = parked_fork();
        let root = dir.path();
        // Without the list nothing is known to be the quiet step's own.
        std::fs::remove_file(root.join("var/lib/silo-quiet/0a1b.masked-timers.list")).unwrap();
        guest_script(root, "restore");
        assert!(exists(root, "/etc/systemd/system/apt-daily.timer"));
        put(
            root,
            "var/lib/silo-quiet/0a1b.masked-timers.list",
            "/etc/systemd/system/apt-daily.timer\n",
        );
        guest_script(root, "restore");
        assert!(!exists(root, "/etc/systemd/system/apt-daily.timer"));
        assert!(exists(root, "/etc/systemd/system/before-fork.timer"));
        assert!(exists(root, "/etc/systemd/system/after-quiet.timer"));
    }

    #[test]
    fn work_the_fork_replaced_stays_where_it_is() {
        let dir = parked_fork();
        let root = dir.path();
        put(root, "etc/cron.d/poll", "* * * * * root replacement\n");
        put(root, "etc/crontab", "5 4 * * * root new-job\n");
        let output = guest_script(root, "restore");
        assert!(output.contains("left /etc/cron.d/poll"), "{output}");
        assert!(output.contains("left /etc/crontab"), "{output}");
        assert!(std::fs::read_to_string(root.join("etc/cron.d/poll"))
            .unwrap()
            .contains("replacement"));
        assert!(exists(root, "/var/lib/silo-quiet/disabled/etc/cron.d/poll"));
        assert!(std::fs::read_to_string(root.join("etc/crontab"))
            .unwrap()
            .contains("new-job"));
        // Parking again leaves the replacement alone as well.
        guest_script(root, "repark");
        assert!(std::fs::read_to_string(root.join("etc/cron.d/poll"))
            .unwrap()
            .contains("replacement"));
    }
}
