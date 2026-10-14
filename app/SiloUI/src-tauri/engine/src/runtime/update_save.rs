//! Linux computers in an engine-update save (design/p2 §3.1, 95 A-6).
//!
//! A Linux computer is a detached MicroSandbox runtime that outlives the engine process, so an
//! engine replacement does not have to kill it. Saving still goes through the runtime's own
//! durable mechanism so a host crash or a lost runtime cannot lose the run:
//!
//! 1. `msb pause --guest-flush required` stops the guest at a known point and keeps the
//!    runtime alive (the *held generation*).
//! 2. `msb snapshot create --full --guest-flush required --integrity` captures that exact
//!    paused state: memory and disks, verified by the runtime.
//! 3. The computer stays paused through the engine's exit. The next engine finds the same
//!    runtime still paused and resumes it (`msb resume`). If the runtime is gone it restores
//!    the capture with `msb restore --cow-mem`, which resumes the guest by itself, so the
//!    journal's `resumeArmed` is written first. It never starts the computer cold.
use super::*;
use crate::engine_update::{
    adapter::{Reconciled, ResumeHeldError, SaveAdapter, SaveContext},
    journal::{CaptureRef, ComputerKind, LinuxCapture},
    RunningComputer,
};

pub struct LinuxUpdateAdapter {
    host: Arc<dyn RuntimeHost>,
}

impl LinuxUpdateAdapter {
    pub fn new(host: Arc<dyn RuntimeHost>) -> Self {
        Self { host }
    }

    /// Runs `work` against the live runtime. `None` while the storage migration holds the
    /// runtime back: nothing runs then.
    fn with_runtime<T>(
        &self,
        unavailable: impl FnOnce() -> Result<T, String>,
        work: impl FnOnce(&Session<'_>) -> Result<T, String>,
    ) -> Result<T, String> {
        let Some(paths) = runtime_paths_if_in_use(self.host.as_ref())? else {
            return unavailable();
        };
        work(&Session {
            runner: &ProcessRunner,
            paths: &paths,
        })
    }
}

fn runtime_unavailable<T>() -> Result<T, String> {
    Err("The computer runtime is unavailable while the storage migration finishes.".into())
}

impl SaveAdapter for LinuxUpdateAdapter {
    fn kind(&self) -> ComputerKind {
        ComputerKind::Linux
    }

    fn running(&self) -> Result<Vec<RunningComputer>, String> {
        self.with_runtime(|| Ok(Vec::new()), |session| session.running())
    }

    fn check_support(&self, computer: &RunningComputer) -> Result<(), String> {
        self.with_runtime(runtime_unavailable, |session| {
            session.check_support(computer)
        })
    }

    fn save_and_hold(
        &self,
        computer: &RunningComputer,
        _context: &SaveContext<'_>,
    ) -> Result<CaptureRef, String> {
        self.with_runtime(runtime_unavailable, |session| {
            session.save_and_hold(computer).map(CaptureRef::Linux)
        })
    }

    fn resume_held(
        &self,
        computer: &RunningComputer,
        _capture: Option<&CaptureRef>,
    ) -> Result<(), ResumeHeldError> {
        match self.with_runtime(
            || {
                Ok(Err(ResumeHeldError::Failed(
                    "The runtime is unavailable.".into(),
                )))
            },
            |session| session.resume_held(computer),
        ) {
            Ok(outcome) => outcome,
            Err(why) => Err(ResumeHeldError::Failed(why)),
        }
    }

    fn release(&self, computer: &RunningComputer, capture: &CaptureRef) -> Result<(), String> {
        let CaptureRef::Linux(capture) = capture else {
            return Err("The capture does not belong to a Linux computer.".into());
        };
        self.with_runtime(runtime_unavailable, |session| {
            session.release(computer, capture)
        })
    }

    fn reconcile_unsaved(&self, computer: &RunningComputer) -> Result<Reconciled, String> {
        self.with_runtime(runtime_unavailable, |session| {
            session.reconcile_unsaved(computer)
        })
    }

    fn restore(
        &self,
        computer: &RunningComputer,
        capture: &CaptureRef,
        _attempt_id: &str,
        arm: &mut dyn FnMut() -> Result<(), String>,
    ) -> Result<(), String> {
        let CaptureRef::Linux(capture) = capture else {
            return Err("The capture does not belong to a Linux computer.".into());
        };
        self.with_runtime(runtime_unavailable, |session| {
            session.restore(computer, capture, arm)
        })
    }

    fn probe_armed(
        &self,
        computer: &RunningComputer,
        _capture: &CaptureRef,
        _attempt_id: &str,
    ) -> Result<bool, String> {
        self.with_runtime(runtime_unavailable, |session| {
            Ok(session
                .observe(computer)?
                .is_some_and(|inspected| inspected.status.eq_ignore_ascii_case("running")))
        })
    }

    fn discard(&self, computer: &RunningComputer, capture: &CaptureRef) -> Result<(), String> {
        let CaptureRef::Linux(capture) = capture else {
            return Ok(());
        };
        self.with_runtime(runtime_unavailable, |session| {
            if checkpoints::discard_failed_capture(
                session.runner,
                session.paths,
                &computer.id,
                (capture.group.clone(), capture.member.clone()),
            ) {
                Ok(())
            } else {
                Err(format!(
                    "The saved state {}:{} is still in use and was kept.",
                    capture.group, capture.member
                ))
            }
        })
    }

    fn owns_live_machines(&self) -> bool {
        // Linux runtimes are detached processes; exiting the engine does not end them.
        false
    }
}

/// The runtime commands of one adapter call. Separate from the adapter so tests drive them
/// with scripted runtime output.
struct Session<'a> {
    runner: &'a dyn RuntimeRunner,
    paths: &'a RuntimePaths,
}

impl Session<'_> {
    fn running(&self) -> Result<Vec<RunningComputer>, String> {
        Ok(update_recovery::running(self.runner, self.paths)?
            .into_iter()
            .map(|computer| RunningComputer {
                id: computer.id,
                name: computer.name,
                kind: ComputerKind::Linux,
            })
            .collect())
    }

    fn configuration(&self, computer: &RunningComputer) -> Result<ComputerConfiguration, String> {
        read_metadata(&self.paths.metadata)
            .map_err(|error| error.to_string())?
            .computers
            .into_iter()
            .find(|configuration| {
                configuration.id() == computer.id && configuration.name() == computer.name
            })
            .ok_or_else(|| {
                format!(
                    "{} was removed or replaced. No replacement computer was changed.",
                    computer.name
                )
            })
    }

    /// The runtime computer with this exact identity, or `None` when the runtime reports it
    /// missing. A different identity under the same name is an error.
    fn observe(&self, computer: &RunningComputer) -> Result<Option<InspectedSandbox>, String> {
        match inspect_computer(self.runner, self.paths, &computer.name) {
            Ok(inspected) => {
                ensure_managed(&inspected).map_err(|error| error.to_string())?;
                let same = inspected.name == computer.name
                    && inspected
                        .config
                        .pointer("/labels/silo.machine-id")
                        .and_then(Value::as_str)
                        == Some(&computer.id);
                if same {
                    Ok(Some(inspected))
                } else {
                    Err(format!(
                        "{} has a different identity. No replacement computer was changed.",
                        computer.name
                    ))
                }
            }
            Err(error) if is_missing_computer(&error) => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }

    fn command(&self, args: &[&str], timeout: Duration) -> Result<CommandOutput, RuntimeError> {
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        self.runner.run(self.paths, &args, timeout)
    }

    fn check_support(&self, computer: &RunningComputer) -> Result<(), String> {
        let inspected = self
            .observe(computer)?
            .ok_or_else(|| format!("{} is not in the runtime.", computer.name))?;
        // The runtime's snapshots exclude the old external workspace mount.
        let owned_workspace = inspected
            .config
            .get("mounts")
            .and_then(Value::as_array)
            .is_some_and(|mounts| {
                let workspace: Vec<_> = mounts
                    .iter()
                    .filter(|mount| {
                        mount.get("guest").and_then(Value::as_str) == Some(WORKSPACE_MOUNT)
                    })
                    .collect();
                workspace.len() == 1
                    && workspace[0].get("type").and_then(Value::as_str) == Some("Owned")
                    && workspace[0]
                        .pointer("/storage/kind")
                        .and_then(Value::as_str)
                        == Some("disk")
            });
        if !owned_workspace {
            return Err(
                "it still uses the old workspace disk; finish runtime migration first".into(),
            );
        }
        if inspected.config.pointer("/network/policy").is_none() {
            return Err("its network policy is unavailable".into());
        }
        Ok(())
    }

    fn pause(&self, computer: &RunningComputer) -> Result<(), String> {
        self.command(
            &["pause", &computer.name, "--guest-flush", "required"],
            MUTATION_TIMEOUT,
        )
        .map_err(|error| error.to_string())?;
        match self.observe(computer)? {
            Some(inspected) if inspected.status.eq_ignore_ascii_case("paused") => Ok(()),
            _ => Err(format!(
                "{} did not stay paused. Nothing was saved.",
                computer.name
            )),
        }
    }

    fn resume(&self, computer: &RunningComputer) -> Result<(), String> {
        self.command(&["resume", &computer.name], MUTATION_TIMEOUT)
            .map_err(|error| error.to_string())?;
        match self.observe(computer)? {
            Some(inspected) if inspected.status.eq_ignore_ascii_case("running") => Ok(()),
            _ => Err(format!("{} did not resume.", computer.name)),
        }
    }

    fn snapshot_ready(&self, group: &str, member: &str) -> Result<bool, String> {
        let output = self
            .command(&["snapshot", "list", "--format", "json"], READ_TIMEOUT)
            .map_err(|error| error.to_string())?;
        let entries: Vec<Value> = serde_json::from_str(&output.stdout)
            .map_err(|_| "The runtime returned an invalid checkpoint list.".to_owned())?;
        Ok(entries.iter().any(|entry| {
            entry["group"] == group
                && entry["name"] == member
                && entry["scope"] == "full"
                && entry["availability"] == "ready"
        }))
    }

    fn save_and_hold(&self, computer: &RunningComputer) -> Result<LinuxCapture, String> {
        let before = self
            .observe(computer)?
            .ok_or_else(|| format!("{} is not in the runtime.", computer.name))?;
        if !before.status.eq_ignore_ascii_case("running") {
            return Err(format!("{} is not running.", computer.name));
        }
        let network_policy = before
            .config
            .pointer("/network/policy")
            .cloned()
            .ok_or("The network policy is unavailable.")?;
        let group = checkpoints::ensure_snapshot_group(self.paths, &computer.id, &computer.name)
            .map_err(|error| error.to_string())?;
        let member = format!("c{}", &uuid::Uuid::new_v4().simple().to_string()[..31]);
        // After the pause succeeds, any failure resumes the original generation.
        if let Err(cause) = self.pause(computer) {
            let _ = self.resume_if_paused(computer);
            return Err(cause);
        }
        let captured = self
            .command(
                &[
                    "snapshot",
                    "create",
                    &member,
                    "--from-sandbox",
                    &computer.name,
                    "--group",
                    &group,
                    "--full",
                    "--guest-flush",
                    "required",
                    "--integrity",
                ],
                Duration::from_secs(900),
            )
            .map_err(|error| error.to_string())
            .and_then(|_| {
                // Verification must not be cut short by a cancel.
                operation_gate::uncancellable(|| self.snapshot_ready(&group, &member))
            })
            .and_then(|ready| {
                ready
                    .then_some(())
                    .ok_or_else(|| "The runtime did not publish a complete saved state.".to_owned())
            });
        if let Err(cause) = captured {
            let resumed = self.resume_if_paused(computer);
            let _ = checkpoints::discard_failed_capture(
                self.runner,
                self.paths,
                &computer.id,
                (group, member),
            );
            return Err(match resumed {
                Ok(()) => cause,
                Err(why) => format!("{cause} {} could not be resumed: {why}", computer.name),
            });
        }
        Ok(LinuxCapture {
            group,
            member,
            scope: "full".into(),
            network_policy,
            integrity_verified: true,
        })
    }

    /// Resumes the computer if the runtime reports it paused.
    fn resume_if_paused(&self, computer: &RunningComputer) -> Result<(), String> {
        match self.observe(computer)? {
            Some(inspected) if inspected.status.eq_ignore_ascii_case("paused") => {
                self.resume(computer)
            }
            Some(inspected) if inspected.status.eq_ignore_ascii_case("running") => Ok(()),
            _ => Err(format!("{} is no longer running or paused.", computer.name)),
        }
    }

    fn resume_held(
        &self,
        computer: &RunningComputer,
    ) -> Result<Result<(), ResumeHeldError>, String> {
        Ok(match self.observe(computer)? {
            None => Err(ResumeHeldError::Gone(format!(
                "{} is no longer in the runtime.",
                computer.name
            ))),
            Some(inspected) => match inspected.status.to_ascii_lowercase().as_str() {
                "running" => Ok(()),
                "paused" => self.resume(computer).map_err(ResumeHeldError::Failed),
                _ => Err(ResumeHeldError::Gone(format!(
                    "{} is {} and can no longer be resumed in place.",
                    computer.name, inspected.status
                ))),
            },
        })
    }

    fn release(&self, computer: &RunningComputer, capture: &LinuxCapture) -> Result<(), String> {
        if !self.snapshot_ready(&capture.group, &capture.member)? {
            return Err(format!(
                "The saved state of {} is missing from the runtime.",
                computer.name
            ));
        }
        match self.observe(computer)? {
            Some(inspected) if inspected.status.eq_ignore_ascii_case("paused") => Ok(()),
            _ => Err(format!(
                "{} is not held paused; the update would not preserve its run.",
                computer.name
            )),
        }
    }

    fn reconcile_unsaved(&self, computer: &RunningComputer) -> Result<Reconciled, String> {
        match self.observe(computer)? {
            Some(inspected) if inspected.status.eq_ignore_ascii_case("running") => {
                Ok(Reconciled::Running)
            }
            Some(inspected) if inspected.status.eq_ignore_ascii_case("paused") => {
                self.resume(computer)?;
                Ok(Reconciled::Running)
            }
            _ => Ok(Reconciled::Lost(format!(
                "{} stopped before its memory was saved. Its disk was kept; start it again.",
                computer.name
            ))),
        }
    }

    fn restore(
        &self,
        computer: &RunningComputer,
        capture: &LinuxCapture,
        arm: &mut dyn FnMut() -> Result<(), String>,
    ) -> Result<(), String> {
        let observed = self.observe(computer)?;
        match observed
            .as_ref()
            .map(|inspected| inspected.status.to_ascii_lowercase())
        {
            // The held generation survived the engine: resume exactly it.
            Some(status) if status == "paused" => {
                arm()?;
                self.resume(computer)
            }
            Some(status) if status == "running" => Ok(()),
            Some(status) if matches!(status.as_str(), "stopped" | "created" | "crashed") => {
                // Its runtime is gone. The capture is the only way back: replace the dead
                // sandbox (its disks come from the snapshot) and restore with memory.
                self.command(&["remove", "--quiet", &computer.name], STOP_TIMEOUT)
                    .map_err(|error| error.to_string())?;
                self.restore_from_capture(computer, capture, arm)
            }
            Some(status) => Err(format!(
                "{} is {status}; wait for it to settle, then retry.",
                computer.name
            )),
            None => self.restore_from_capture(computer, capture, arm),
        }
    }

    /// `msb restore <group>:<member> --cow-mem`, through the same path a checkpoint fork
    /// starts with. The guest executes as soon as the restore returns, so `arm` comes first.
    fn restore_from_capture(
        &self,
        computer: &RunningComputer,
        capture: &LinuxCapture,
        arm: &mut dyn FnMut() -> Result<(), String>,
    ) -> Result<(), String> {
        if capture.scope != "full" || !capture.integrity_verified {
            return Err("The saved state is not a verified full capture; it was preserved.".into());
        }
        if !self.snapshot_ready(&capture.group, &capture.member)? {
            return Err(format!(
                "The saved state of {} is missing from the runtime. It was preserved.",
                computer.name
            ));
        }
        let configuration = self.configuration(computer)?;
        let mut record =
            checkpoints::load(self.paths, &computer.id).map_err(|error| error.to_string())?;
        record.snapshot_group = Some(capture.group.clone());
        record.pending_checkpoint_restore = Some(checkpoints::PendingRestore {
            checkpoint_id: capture.member.clone(),
            source_computer: capture.group.clone(),
            state: "full".into(),
        });
        record.desired_network_policy = Some(capture.network_policy.clone());
        checkpoints::save(self.paths, &computer.id, &record).map_err(|error| error.to_string())?;
        arm()?;
        checkpoints::start_pending(self.runner, self.paths, &configuration)
            .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    const ID: &str = "00000000-0000-4000-8000-000000000001";

    type Respond = Box<dyn Fn(&[String]) -> Result<String, RuntimeError> + Send + Sync>;

    /// Runtime commands in order, each matched by its leading arguments.
    struct Script {
        steps: Mutex<VecDeque<(&'static str, Respond)>>,
        calls: Mutex<Vec<Vec<String>>>,
    }

    impl Script {
        fn new(steps: Vec<(&'static str, Respond)>) -> Self {
            Self {
                steps: Mutex::new(steps.into()),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn finished(&self) {
            assert!(
                self.steps.lock().unwrap().is_empty(),
                "commands not run: {:?}",
                self.steps
                    .lock()
                    .unwrap()
                    .iter()
                    .map(|step| step.0)
                    .collect::<Vec<_>>()
            );
        }

        fn calls(&self) -> Vec<String> {
            self.calls
                .lock()
                .unwrap()
                .iter()
                .map(|call| call.join(" "))
                .collect()
        }
    }

    impl RuntimeRunner for Script {
        fn run(
            &self,
            _: &RuntimePaths,
            args: &[String],
            _: Duration,
        ) -> Result<CommandOutput, RuntimeError> {
            self.calls.lock().unwrap().push(args.to_vec());
            let (prefix, respond) = self
                .steps
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| panic!("unexpected runtime command: {args:?}"));
            assert!(
                args.join(" ").starts_with(prefix),
                "expected `{prefix}`, ran {args:?}"
            );
            respond(args).map(|stdout| CommandOutput {
                stdout,
                stderr: String::new(),
            })
        }
    }

    fn ok(stdout: impl Into<String>) -> Respond {
        let stdout = stdout.into();
        Box::new(move |_| Ok(stdout.clone()))
    }

    fn missing() -> Respond {
        Box::new(|_| {
            Err(RuntimeError::Failed {
                operation: "inspect".into(),
                exit_code: Some(1),
                detail: "sandbox not found".into(),
            })
        })
    }

    fn computer() -> RunningComputer {
        RunningComputer {
            id: ID.into(),
            name: "dev".into(),
            kind: ComputerKind::Linux,
        }
    }

    fn inspect(status: &str) -> Respond {
        ok(json!({
            "name": "dev",
            "status": status,
            "config": {
                "name": "dev",
                "labels": {"silo.managed": "true", "silo.machine-id": ID},
                "mounts": [{
                    "type": "Owned", "guest": WORKSPACE_MOUNT,
                    "storage": {"kind": "disk", "capacity_mib": 61440}
                }],
                "network": {"policy": {"default_egress": "allow", "default_ingress": "deny", "rules": []}}
            }
        })
        .to_string())
    }

    fn listed(member: &str) -> Respond {
        ok(json!([{
            "group": "dev", "name": member, "scope": "full", "availability": "ready"
        }])
        .to_string())
    }

    fn setup(directory: &tempfile::TempDir) -> RuntimePaths {
        let paths = crate::test_support::paths(directory.path());
        let configuration = ComputerConfiguration {
            id: ID.into(),
            name: "dev".into(),
            cpus: 4,
            max_cpus: 6,
            memory_gib: 16,
            max_memory_gib: 32,
            workspace_storage_gib: 60,
            runtime_storage_gib: 80,
            desktop: None,
        };
        fs::write(
            &paths.metadata,
            serde_json::to_vec(&ComputerConfigurationRequest {
                schema_version: 1,
                computers: vec![configuration],
            })
            .unwrap(),
        )
        .unwrap();
        paths
    }

    fn capture() -> LinuxCapture {
        LinuxCapture {
            group: "dev".into(),
            member: "c0123456789abcdef0123456789abcde".into(),
            scope: "full".into(),
            network_policy: json!({"default_egress": "allow", "default_ingress": "deny", "rules": []}),
            integrity_verified: true,
        }
    }

    #[test]
    fn saving_pauses_then_captures_a_full_snapshot_and_keeps_the_computer_held() {
        let directory = tempfile::tempdir().unwrap();
        let paths = setup(&directory);
        let member = Arc::new(Mutex::new(String::new()));
        let seen = member.clone();
        let runner = Script::new(vec![
            ("inspect dev", inspect("Running")),
            ("pause dev --guest-flush required", ok("")),
            ("inspect dev", inspect("Paused")),
            (
                "snapshot create",
                Box::new(move |args: &[String]| {
                    *seen.lock().unwrap() = args[2].clone();
                    let flags: Vec<_> = args[3..].iter().map(String::as_str).collect();
                    assert_eq!(
                        flags,
                        [
                            "--from-sandbox",
                            "dev",
                            "--group",
                            "dev",
                            "--full",
                            "--guest-flush",
                            "required",
                            "--integrity"
                        ]
                    );
                    Ok(String::new())
                }),
            ),
            ("snapshot list", {
                let member = member.clone();
                Box::new(move |_: &[String]| {
                    Ok(json!([{"group":"dev","name":*member.lock().unwrap(),"scope":"full","availability":"ready"}]).to_string())
                })
            }),
        ]);
        let session = Session {
            runner: &runner,
            paths: &paths,
        };
        let captured = session.save_and_hold(&computer()).unwrap();
        runner.finished();
        assert_eq!(captured.group, "dev");
        assert_eq!(captured.member, *member.lock().unwrap());
        assert!(captured.member.starts_with('c') && captured.member.len() == 32);
        assert_eq!(captured.scope, "full");
        assert_eq!(captured.network_policy["default_egress"], "allow");
        // The computer is left paused: nothing resumed it.
        assert!(!runner.calls().iter().any(|call| call.starts_with("resume")));
    }

    #[test]
    fn a_failed_capture_resumes_the_original_generation() {
        let directory = tempfile::tempdir().unwrap();
        let paths = setup(&directory);
        let runner = Script::new(vec![
            ("inspect dev", inspect("Running")),
            ("pause dev", ok("")),
            ("inspect dev", inspect("Paused")),
            (
                "snapshot create",
                Box::new(|_: &[String]| Err(RuntimeError::Unavailable("disk full".into()))),
            ),
            // The failed capture resumes the original generation before anything else.
            ("inspect dev", inspect("Paused")),
            ("resume dev", ok("")),
            ("inspect dev", inspect("Running")),
            // Then it discards whatever member the runtime may have published.
            ("snapshot list", ok("[]")),
        ]);
        let session = Session {
            runner: &runner,
            paths: &paths,
        };
        let error = session.save_and_hold(&computer()).unwrap_err();
        assert!(error.contains("disk full"), "{error}");
        runner.finished();
    }

    #[test]
    fn a_pause_that_does_not_hold_resumes_and_saves_nothing() {
        let directory = tempfile::tempdir().unwrap();
        let paths = setup(&directory);
        let runner = Script::new(vec![
            ("inspect dev", inspect("Running")),
            ("pause dev", ok("")),
            ("inspect dev", inspect("Running")),
            ("inspect dev", inspect("Running")),
        ]);
        let session = Session {
            runner: &runner,
            paths: &paths,
        };
        let error = session.save_and_hold(&computer()).unwrap_err();
        assert!(error.contains("did not stay paused"), "{error}");
        runner.finished();
        assert!(!runner
            .calls()
            .iter()
            .any(|call| call.starts_with("snapshot")));
    }

    #[test]
    fn support_requires_the_owned_workspace_disk() {
        let directory = tempfile::tempdir().unwrap();
        let paths = setup(&directory);
        let runner = Script::new(vec![
            ("inspect dev", inspect("Running")),
            (
                "inspect dev",
                ok(json!({
                    "name": "dev", "status": "Running",
                    "config": {"labels": {"silo.managed": "true", "silo.machine-id": ID}, "mounts": []}
                })
                .to_string()),
            ),
        ]);
        let session = Session {
            runner: &runner,
            paths: &paths,
        };
        session.check_support(&computer()).unwrap();
        assert!(session
            .check_support(&computer())
            .unwrap_err()
            .contains("old workspace disk"));
    }

    #[test]
    fn resuming_a_held_computer_resumes_it_and_reports_a_missing_runtime_as_gone() {
        let directory = tempfile::tempdir().unwrap();
        let paths = setup(&directory);
        let runner = Script::new(vec![
            ("inspect dev", inspect("Paused")),
            ("resume dev", ok("")),
            ("inspect dev", inspect("Running")),
            ("inspect dev", inspect("Running")),
            ("inspect dev", inspect("Crashed")),
            ("inspect dev", missing()),
        ]);
        let session = Session {
            runner: &runner,
            paths: &paths,
        };
        assert_eq!(session.resume_held(&computer()).unwrap(), Ok(()));
        assert_eq!(session.resume_held(&computer()).unwrap(), Ok(()));
        assert!(matches!(
            session.resume_held(&computer()).unwrap(),
            Err(ResumeHeldError::Gone(_))
        ));
        assert!(matches!(
            session.resume_held(&computer()).unwrap(),
            Err(ResumeHeldError::Gone(_))
        ));
        runner.finished();
    }

    #[test]
    fn release_requires_the_capture_and_a_paused_runtime() {
        let directory = tempfile::tempdir().unwrap();
        let paths = setup(&directory);
        let capture = capture();
        let runner = Script::new(vec![
            ("snapshot list", ok("[]")),
            ("snapshot list", listed(&capture.member)),
            ("inspect dev", inspect("Running")),
            ("snapshot list", listed(&capture.member)),
            ("inspect dev", inspect("Paused")),
        ]);
        let session = Session {
            runner: &runner,
            paths: &paths,
        };
        assert!(session
            .release(&computer(), &capture)
            .unwrap_err()
            .contains("missing from the runtime"));
        assert!(session
            .release(&computer(), &capture)
            .unwrap_err()
            .contains("not held paused"));
        session.release(&computer(), &capture).unwrap();
        runner.finished();
    }

    #[test]
    fn a_surviving_held_runtime_is_resumed_after_arming_without_a_restore() {
        let directory = tempfile::tempdir().unwrap();
        let paths = setup(&directory);
        let runner = Script::new(vec![
            ("inspect dev", inspect("Paused")),
            ("resume dev", ok("")),
            ("inspect dev", inspect("Running")),
        ]);
        let session = Session {
            runner: &runner,
            paths: &paths,
        };
        let mut armed = 0;
        session
            .restore(&computer(), &capture(), &mut || {
                armed += 1;
                Ok(())
            })
            .unwrap();
        assert_eq!(armed, 1);
        runner.finished();
        assert!(!runner
            .calls()
            .iter()
            .any(|call| call.starts_with("restore")));
    }

    #[test]
    fn a_runtime_that_is_already_running_needs_no_arm() {
        let directory = tempfile::tempdir().unwrap();
        let paths = setup(&directory);
        let runner = Script::new(vec![("inspect dev", inspect("Running"))]);
        let session = Session {
            runner: &runner,
            paths: &paths,
        };
        session
            .restore(&computer(), &capture(), &mut || panic!("nothing executes"))
            .unwrap();
    }

    #[test]
    fn a_failed_arm_never_lets_the_guest_run() {
        let directory = tempfile::tempdir().unwrap();
        let paths = setup(&directory);
        let runner = Script::new(vec![("inspect dev", inspect("Paused"))]);
        let session = Session {
            runner: &runner,
            paths: &paths,
        };
        let error = session
            .restore(&computer(), &capture(), &mut || {
                Err("journal write failed".into())
            })
            .unwrap_err();
        assert_eq!(error, "journal write failed");
        runner.finished();
    }

    #[test]
    fn a_lost_runtime_is_restored_from_the_capture_with_cow_memory_and_never_cold_booted() {
        let _state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = setup(&directory);
        let capture = capture();
        let armed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let restore_after_arm = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let runner = Script::new(vec![
            ("inspect dev", missing()),
            ("snapshot list", listed(&capture.member)),
            // start_pending: the snapshot is verified, and the name must be free.
            ("snapshot list", listed(&capture.member)),
            ("list", ok("[]")),
            ("restore", {
                let (armed, seen) = (armed.clone(), restore_after_arm.clone());
                let member = capture.member.clone();
                Box::new(move |args: &[String]| {
                    seen.store(
                        armed.load(std::sync::atomic::Ordering::SeqCst),
                        std::sync::atomic::Ordering::SeqCst,
                    );
                    assert_eq!(args[1], format!("dev:{member}"));
                    assert!(args.iter().any(|arg| arg == "--cow-mem"));
                    assert!(!args.iter().any(|arg| arg == "--disk-only"));
                    Err(RuntimeError::Unavailable(
                        "synthetic restore failure".into(),
                    ))
                })
            }),
            // After the failure the runtime is asked whether the attempt created a computer.
            ("inspect dev", missing()),
        ]);
        let session = Session {
            runner: &runner,
            paths: &paths,
        };
        let error = session
            .restore(&computer(), &capture, &mut || {
                armed.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            })
            .unwrap_err();
        assert!(error.contains("synthetic restore failure"), "{error}");
        assert!(
            restore_after_arm.load(std::sync::atomic::Ordering::SeqCst),
            "the journal was armed before the guest could execute"
        );
        runner.finished();
        assert!(
            !runner
                .calls()
                .iter()
                .any(|call| call.starts_with("start") || call.starts_with("run")),
            "{:?}",
            runner.calls()
        );
        // The pending restore references exactly the capture.
        let record = checkpoints::load(&paths, ID).unwrap();
        let pending = record.pending_checkpoint_restore.unwrap();
        assert_eq!(
            (pending.checkpoint_id, pending.state.as_str()),
            (capture.member, "full")
        );
    }

    #[test]
    fn a_restore_of_a_capture_that_left_the_runtime_is_refused_before_arming() {
        let directory = tempfile::tempdir().unwrap();
        let paths = setup(&directory);
        let runner = Script::new(vec![
            ("inspect dev", missing()),
            ("snapshot list", ok("[]")),
        ]);
        let session = Session {
            runner: &runner,
            paths: &paths,
        };
        let error = session
            .restore(&computer(), &capture(), &mut || panic!("nothing may arm"))
            .unwrap_err();
        assert!(error.contains("missing from the runtime"), "{error}");
        runner.finished();
    }

    #[test]
    fn reconciling_an_unsaved_computer_resumes_a_pause_the_engine_left_behind() {
        let directory = tempfile::tempdir().unwrap();
        let paths = setup(&directory);
        let runner = Script::new(vec![
            ("inspect dev", inspect("Paused")),
            ("resume dev", ok("")),
            ("inspect dev", inspect("Running")),
            ("inspect dev", inspect("Crashed")),
        ]);
        let session = Session {
            runner: &runner,
            paths: &paths,
        };
        assert_eq!(
            session.reconcile_unsaved(&computer()).unwrap(),
            Reconciled::Running
        );
        assert!(matches!(
            session.reconcile_unsaved(&computer()).unwrap(),
            Reconciled::Lost(_)
        ));
        runner.finished();
    }
}
