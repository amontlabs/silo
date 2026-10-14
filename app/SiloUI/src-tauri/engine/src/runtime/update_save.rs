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
    adapter::{OriginalState, Reconciled, RestoreError, ResumeHeldError, SaveAdapter, SaveContext},
    journal::{CaptureRef, ComputerKind, LinuxCapture, RuntimeIdentity},
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

    fn identity(&self, computer: &RunningComputer) -> Result<RuntimeIdentity, String> {
        self.with_runtime(runtime_unavailable, |session| session.identity(computer))
    }

    fn resume_held(
        &self,
        computer: &RunningComputer,
        original: &RuntimeIdentity,
    ) -> Result<(), ResumeHeldError> {
        match self.with_runtime(
            || {
                Ok(Err(ResumeHeldError::Failed(
                    "The runtime is unavailable.".into(),
                )))
            },
            |session| session.resume_held(computer, original),
        ) {
            Ok(outcome) => outcome,
            Err(why) => Err(ResumeHeldError::Failed(why)),
        }
    }

    fn probe_original(
        &self,
        computer: &RunningComputer,
        original: &RuntimeIdentity,
    ) -> Result<OriginalState, String> {
        self.with_runtime(runtime_unavailable, |session| {
            session.probe_original(computer, original)
        })
    }

    fn release(
        &self,
        computer: &RunningComputer,
        capture: &CaptureRef,
        original: &RuntimeIdentity,
    ) -> Result<(), String> {
        let CaptureRef::Linux(capture) = capture else {
            return Err("The capture does not belong to a Linux computer.".into());
        };
        self.with_runtime(runtime_unavailable, |session| {
            session.release(computer, capture, original)
        })
    }

    fn reconcile_unsaved(
        &self,
        computer: &RunningComputer,
        original: &RuntimeIdentity,
    ) -> Result<Reconciled, String> {
        self.with_runtime(runtime_unavailable, |session| {
            session.reconcile_unsaved(computer, original)
        })
    }

    fn restore(
        &self,
        computer: &RunningComputer,
        capture: &CaptureRef,
        original: &RuntimeIdentity,
        _attempt_id: &str,
        arm: &mut dyn FnMut(Option<&RuntimeIdentity>) -> Result<(), String>,
    ) -> Result<(), RestoreError> {
        let CaptureRef::Linux(capture) = capture else {
            return Err("The capture does not belong to a Linux computer."
                .to_owned()
                .into());
        };
        match self.with_runtime(runtime_unavailable, |session| {
            Ok(session.restore(computer, capture, original, arm))
        }) {
            Ok(outcome) => outcome,
            Err(why) => Err(RestoreError::Failed(why)),
        }
    }

    fn probe_armed(
        &self,
        computer: &RunningComputer,
        capture: &CaptureRef,
        original: &RuntimeIdentity,
        _attempt_id: &str,
        expected: Option<&RuntimeIdentity>,
    ) -> Result<bool, String> {
        let CaptureRef::Linux(capture) = capture else {
            return Err("The capture does not belong to a Linux computer.".into());
        };
        self.with_runtime(runtime_unavailable, |session| {
            session.probe_armed(computer, capture, original, expected)
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

    fn identity_of(inspected: &InspectedSandbox) -> RuntimeIdentity {
        RuntimeIdentity {
            owner: None,
            generation: None,
            instance: inspected.runtime_instance_id.clone(),
        }
    }

    fn identity(&self, computer: &RunningComputer) -> Result<RuntimeIdentity, String> {
        self.observe(computer)?
            .map(|inspected| Self::identity_of(&inspected))
            .ok_or_else(|| format!("{} is not in the runtime.", computer.name))
    }

    /// The runtime computer if it is exactly the run `original` names. A different run of
    /// the same computer is not the original and is never touched on its behalf.
    fn observe_original(
        &self,
        computer: &RunningComputer,
        original: &RuntimeIdentity,
    ) -> Result<Option<InspectedSandbox>, String> {
        Ok(self
            .observe(computer)?
            .filter(|inspected| original.matches(&Self::identity_of(inspected))))
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
            runtime_instance: before.runtime_instance_id.clone(),
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
        original: &RuntimeIdentity,
    ) -> Result<Result<(), ResumeHeldError>, String> {
        Ok(match self.observe_original(computer, original)? {
            None => Err(ResumeHeldError::Gone(format!(
                "{} is no longer the held run in the runtime.",
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

    fn probe_original(
        &self,
        computer: &RunningComputer,
        original: &RuntimeIdentity,
    ) -> Result<OriginalState, String> {
        Ok(match self.observe_original(computer, original)? {
            Some(inspected) if inspected.status.eq_ignore_ascii_case("running") => {
                OriginalState::Running
            }
            Some(inspected) if inspected.status.eq_ignore_ascii_case("paused") => {
                OriginalState::Held
            }
            _ => OriginalState::Gone,
        })
    }

    fn release(
        &self,
        computer: &RunningComputer,
        capture: &LinuxCapture,
        original: &RuntimeIdentity,
    ) -> Result<(), String> {
        if !self.snapshot_ready(&capture.group, &capture.member)? {
            return Err(format!(
                "The saved state of {} is missing from the runtime.",
                computer.name
            ));
        }
        match self.observe_original(computer, original)? {
            Some(inspected) if inspected.status.eq_ignore_ascii_case("paused") => Ok(()),
            _ => Err(format!(
                "{} is not held paused; the update would not preserve its run.",
                computer.name
            )),
        }
    }

    fn reconcile_unsaved(
        &self,
        computer: &RunningComputer,
        original: &RuntimeIdentity,
    ) -> Result<Reconciled, String> {
        match self.observe_original(computer, original)? {
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
        original: &RuntimeIdentity,
        arm: &mut dyn FnMut(Option<&RuntimeIdentity>) -> Result<(), String>,
    ) -> Result<(), RestoreError> {
        let observed = self.observe(computer)?;
        let Some(inspected) = observed else {
            return self.restore_from_capture(computer, capture, arm);
        };
        let status = inspected.status.to_ascii_lowercase();
        let identity = Self::identity_of(&inspected);
        let ours = original.matches(&identity);
        match status.as_str() {
            // The held generation survived the engine: resume exactly it.
            "paused" if ours => {
                arm(Some(&identity))?;
                Ok(self.resume(computer)?)
            }
            // Some other run, or the original resumed without the transaction arming it: the
            // capture is not applied over a disk that run may have advanced.
            "paused" | "running" => Err(RestoreError::Superseded(format!(
                "{} is running again without being restored from the update. Its disk was kept and the saved state was not applied.",
                computer.name
            ))),
            "stopped" | "created" | "crashed" => {
                // Everything the restore needs is checked before the sandbox is removed: a
                // missing capture or unreadable configuration must never cost the disk.
                let plan = self.restore_plan(computer, capture)?;
                self.command(&["remove", "--quiet", &computer.name], STOP_TIMEOUT)
                    .map_err(|error| error.to_string())?;
                self.start_restore(computer, capture, plan, arm)
            }
            other => Err(RestoreError::Failed(format!(
                "{} is {other}; wait for it to settle, then retry.",
                computer.name
            ))),
        }
    }

    /// `msb restore <group>:<member> --cow-mem`, through the same path a checkpoint fork
    /// starts with. The guest executes as soon as the restore returns, so `arm` comes first.
    fn restore_from_capture(
        &self,
        computer: &RunningComputer,
        capture: &LinuxCapture,
        arm: &mut dyn FnMut(Option<&RuntimeIdentity>) -> Result<(), String>,
    ) -> Result<(), RestoreError> {
        let plan = self.restore_plan(computer, capture)?;
        self.start_restore(computer, capture, plan, arm)
    }

    /// Validates every input of a restore from the capture, changing nothing.
    fn restore_plan(
        &self,
        computer: &RunningComputer,
        capture: &LinuxCapture,
    ) -> Result<RestorePlan, String> {
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
        let record =
            checkpoints::load(self.paths, &computer.id).map_err(|error| error.to_string())?;
        Ok(RestorePlan {
            configuration,
            record,
        })
    }

    fn start_restore(
        &self,
        computer: &RunningComputer,
        capture: &LinuxCapture,
        plan: RestorePlan,
        arm: &mut dyn FnMut(Option<&RuntimeIdentity>) -> Result<(), String>,
    ) -> Result<(), RestoreError> {
        let RestorePlan {
            configuration,
            mut record,
        } = plan;
        record.snapshot_group = Some(capture.group.clone());
        record.pending_checkpoint_restore = Some(checkpoints::PendingRestore {
            checkpoint_id: capture.member.clone(),
            source_computer: capture.group.clone(),
            state: "full".into(),
        });
        record.desired_network_policy = Some(capture.network_policy.clone());
        checkpoints::save(self.paths, &computer.id, &record).map_err(|error| error.to_string())?;
        // The new run's instance is not known until it exists; the probe proves it later.
        arm(None)?;
        checkpoints::start_pending(self.runner, self.paths, &configuration)
            .map_err(|error| RestoreError::Failed(error.to_string()))
    }

    /// After a crash with the restore armed: whether the restored run executes now.
    fn probe_armed(
        &self,
        computer: &RunningComputer,
        capture: &LinuxCapture,
        original: &RuntimeIdentity,
        expected: Option<&RuntimeIdentity>,
    ) -> Result<bool, String> {
        let Some(inspected) = self.observe(computer)? else {
            return Ok(false);
        };
        if !inspected.status.eq_ignore_ascii_case("running") {
            return Ok(false);
        }
        let identity = Self::identity_of(&inspected);
        Ok(match expected {
            // The armed resume named the exact run it would execute.
            Some(expected) => expected.matches(&identity),
            // A restore from the capture creates a new run: it must be a different run than
            // the original, and the capture must have been consumed by it.
            None => {
                identity.instance.is_some()
                    && identity.instance != original.instance
                    && checkpoints::load(self.paths, &computer.id).is_ok_and(|record| {
                        record
                            .pending_checkpoint_restore
                            .as_ref()
                            .is_none_or(|pending| pending.checkpoint_id != capture.member)
                    })
            }
        })
    }
}

/// What a restore from the capture needs, read before anything is removed.
struct RestorePlan {
    configuration: ComputerConfiguration,
    record: checkpoints::Record,
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
        inspect_run(status, "run-1")
    }

    fn inspect_run(status: &str, instance: &str) -> Respond {
        ok(json!({
            "name": "dev",
            "status": status,
            "runtime_instance_id": instance,
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

    fn original() -> RuntimeIdentity {
        RuntimeIdentity {
            owner: None,
            generation: None,
            instance: Some("run-1".into()),
        }
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
            runtime_instance: Some("run-1".into()),
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
        assert_eq!(
            session.resume_held(&computer(), &original()).unwrap(),
            Ok(())
        );
        assert_eq!(
            session.resume_held(&computer(), &original()).unwrap(),
            Ok(())
        );
        assert!(matches!(
            session.resume_held(&computer(), &original()).unwrap(),
            Err(ResumeHeldError::Gone(_))
        ));
        assert!(matches!(
            session.resume_held(&computer(), &original()).unwrap(),
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
            .release(&computer(), &capture, &original())
            .unwrap_err()
            .contains("missing from the runtime"));
        assert!(session
            .release(&computer(), &capture, &original())
            .unwrap_err()
            .contains("not held paused"));
        session.release(&computer(), &capture, &original()).unwrap();
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
            .restore(&computer(), &capture(), &original(), &mut |identity| {
                assert_eq!(identity, Some(&original()));
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
    fn a_runtime_that_runs_without_the_transaction_is_never_settled_or_replayed() {
        let directory = tempfile::tempdir().unwrap();
        let paths = setup(&directory);
        let runner = Script::new(vec![
            ("inspect dev", inspect("Running")),
            ("inspect dev", inspect_run("Paused", "run-2")),
        ]);
        let session = Session {
            runner: &runner,
            paths: &paths,
        };
        for _ in 0..2 {
            let error = session
                .restore(&computer(), &capture(), &original(), &mut |_| {
                    panic!("nothing executes")
                })
                .unwrap_err();
            assert!(matches!(error, RestoreError::Superseded(_)), "{error:?}");
        }
        runner.finished();
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
            .restore(&computer(), &capture(), &original(), &mut |_| {
                Err("journal write failed".into())
            })
            .unwrap_err();
        assert_eq!(error, RestoreError::Failed("journal write failed".into()));
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
            .restore(&computer(), &capture, &original(), &mut |identity| {
                assert_eq!(identity, None, "the new run is not known before it exists");
                armed.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            })
            .unwrap_err();
        assert!(
            error.message().contains("synthetic restore failure"),
            "{error:?}"
        );
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
            .restore(&computer(), &capture(), &original(), &mut |_| {
                panic!("nothing may arm")
            })
            .unwrap_err();
        assert!(
            error.message().contains("missing from the runtime"),
            "{error:?}"
        );
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
            session.reconcile_unsaved(&computer(), &original()).unwrap(),
            Reconciled::Running
        );
        assert!(matches!(
            session.reconcile_unsaved(&computer(), &original()).unwrap(),
            Reconciled::Lost(_)
        ));
        runner.finished();
    }

    #[test]
    fn a_crashed_sandbox_is_removed_only_after_the_recovery_inputs_validated() {
        let directory = tempfile::tempdir().unwrap();
        let paths = setup(&directory);
        let capture = capture();
        // Missing capture: nothing is removed.
        let runner = Script::new(vec![
            ("inspect dev", inspect("Crashed")),
            ("snapshot list", ok("[]")),
        ]);
        let session = Session {
            runner: &runner,
            paths: &paths,
        };
        let error = session
            .restore(&computer(), &capture, &original(), &mut |_| {
                panic!("nothing may arm")
            })
            .unwrap_err();
        assert!(
            error.message().contains("missing from the runtime"),
            "{error:?}"
        );
        assert!(
            !runner.calls().iter().any(|call| call.starts_with("remove")),
            "{:?}",
            runner.calls()
        );
        runner.finished();
        // Unreadable configuration: nothing is removed either.
        fs::write(&paths.metadata, b"not json").unwrap();
        let runner = Script::new(vec![
            ("inspect dev", inspect("Crashed")),
            ("snapshot list", listed(&capture.member)),
        ]);
        let session = Session {
            runner: &runner,
            paths: &paths,
        };
        assert!(session
            .restore(&computer(), &capture, &original(), &mut |_| {
                panic!("nothing may arm")
            })
            .is_err());
        assert!(!runner.calls().iter().any(|call| call.starts_with("remove")));
        runner.finished();
        // A capture that is not a verified full capture is refused before any command.
        let mut disk_only = capture.clone();
        disk_only.integrity_verified = false;
        let runner = Script::new(vec![("inspect dev", inspect("Crashed"))]);
        let session = Session {
            runner: &runner,
            paths: &paths,
        };
        assert!(session
            .restore(&computer(), &disk_only, &original(), &mut |_| {
                panic!("nothing may arm")
            })
            .is_err());
        runner.finished();
    }

    #[test]
    fn only_the_journaled_run_is_resumed_probed_or_released() {
        let directory = tempfile::tempdir().unwrap();
        let paths = setup(&directory);
        let capture = capture();
        let runner = Script::new(vec![
            // A replacement run of the same computer is not the original.
            ("inspect dev", inspect_run("Paused", "run-2")),
            ("inspect dev", inspect_run("Running", "run-2")),
            ("inspect dev", inspect("Paused")),
            ("inspect dev", inspect("Running")),
            ("snapshot list", listed(&capture.member)),
            ("inspect dev", inspect_run("Paused", "run-2")),
        ]);
        let session = Session {
            runner: &runner,
            paths: &paths,
        };
        assert!(matches!(
            session.resume_held(&computer(), &original()).unwrap(),
            Err(ResumeHeldError::Gone(_))
        ));
        assert_eq!(
            session.probe_original(&computer(), &original()).unwrap(),
            OriginalState::Gone
        );
        assert_eq!(
            session.probe_original(&computer(), &original()).unwrap(),
            OriginalState::Held
        );
        assert_eq!(
            session.probe_original(&computer(), &original()).unwrap(),
            OriginalState::Running
        );
        assert!(session
            .release(&computer(), &capture, &original())
            .unwrap_err()
            .contains("not held paused"));
        runner.finished();
    }

    #[test]
    fn an_armed_restore_is_proved_by_its_own_run_and_never_by_a_replacement() {
        let _state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = setup(&directory);
        let capture = capture();
        let named = RuntimeIdentity {
            owner: None,
            generation: None,
            instance: Some("run-1".into()),
        };
        let runner = Script::new(vec![
            ("inspect dev", inspect("Running")),
            ("inspect dev", inspect_run("Running", "run-9")),
            ("inspect dev", missing()),
            // Restored from the capture: a new run, with the capture consumed.
            ("inspect dev", inspect_run("Running", "run-2")),
            ("inspect dev", inspect("Running")),
            ("inspect dev", inspect_run("Running", "run-2")),
        ]);
        let session = Session {
            runner: &runner,
            paths: &paths,
        };
        // Named when armed: exactly that run.
        assert!(session
            .probe_armed(&computer(), &capture, &original(), Some(&named))
            .unwrap());
        assert!(!session
            .probe_armed(&computer(), &capture, &original(), Some(&named))
            .unwrap());
        assert!(!session
            .probe_armed(&computer(), &capture, &original(), Some(&named))
            .unwrap());
        // Not named (restored from the capture): a different run than the original, and the
        // capture must have been consumed.
        let mut record = checkpoints::load(&paths, ID).unwrap();
        record.pending_checkpoint_restore = Some(checkpoints::PendingRestore {
            checkpoint_id: capture.member.clone(),
            source_computer: capture.group.clone(),
            state: "full".into(),
        });
        checkpoints::save(&paths, ID, &record).unwrap();
        assert!(!session
            .probe_armed(&computer(), &capture, &original(), None)
            .unwrap());
        record.pending_checkpoint_restore = None;
        checkpoints::save(&paths, ID, &record).unwrap();
        assert!(!session
            .probe_armed(&computer(), &capture, &original(), None)
            .unwrap());
        assert!(session
            .probe_armed(&computer(), &capture, &original(), None)
            .unwrap());
        runner.finished();
    }
}
