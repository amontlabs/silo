use super::*;

struct FailedCaptureRunner {
    runtime: JournalRunner,
    failure: &'static str,
    gate: &'static super::super::super::operation_gate::OperationGate,
    captured: Mutex<bool>,
    resume_stays_paused: bool,
}

impl FailedCaptureRunner {
    fn new(failure: &'static str, recovery_failure: &'static str) -> Self {
        Self {
            runtime: journal_runner("Running", recovery_failure),
            failure,
            gate: Box::leak(Box::new(
                super::super::super::operation_gate::OperationGate::new(),
            )),
            captured: Mutex::new(false),
            resume_stays_paused: false,
        }
    }
}

impl RuntimeRunner for FailedCaptureRunner {
    fn run(
        &self,
        paths: &RuntimePaths,
        args: &[String],
        timeout: Duration,
    ) -> Result<CommandOutput, RuntimeError> {
        if super::super::super::operation_gate::cancel_requested() {
            return Err(RuntimeError::Cancelled {
                operation: args[0].clone(),
            });
        }
        if args[0] == "start" {
            *self.runtime.state.lock().unwrap() = "Running";
        }
        if args[0] == "inspect" {
            if *self.captured.lock().unwrap() && self.failure == "inspect" {
                return Err(error("Source inspection failed."));
            }
            let mut output = self.runtime.run(paths, args, timeout)?;
            let mut computer: Value = serde_json::from_str(&output.stdout).unwrap();
            computer["config"]["resources"] = serde_json::json!({
                "max_cpus": 1, "max_memory_mib": 1024
            });
            if *self.captured.lock().unwrap() && self.failure == "identity" {
                computer["config"]["labels"]["silo.machine-id"] =
                    Value::String("00000000-0000-4000-8000-000000000002".into());
            }
            output.stdout = computer.to_string();
            return Ok(output);
        }
        if self.failure == "success" && args[0] == "snapshot" {
            return self.runtime.run(paths, args, timeout);
        }
        if args.starts_with(&["snapshot".into(), "create".into()]) {
            self.runtime.calls.lock().unwrap().push(args.to_vec());
            *self.captured.lock().unwrap() = true;
            *self.runtime.state.lock().unwrap() = "Paused";
            return match self.failure {
                "cancel" => {
                    self.gate
                        .cancel(self.gate.snapshot().running[0].id)
                        .unwrap();
                    Err(RuntimeError::Cancelled {
                        operation: "snapshot create".into(),
                    })
                }
                "timeout" => Err(RuntimeError::TimedOut {
                    operation: "snapshot create".into(),
                }),
                "verify" => Ok(CommandOutput {
                    stdout: String::new(),
                    stderr: String::new(),
                }),
                _ => Err(error("Full capture failed.")),
            };
        }
        // No usable member was published. Verification fails and cleanup sees no data.
        if args.starts_with(&["snapshot".into(), "list".into()]) {
            self.runtime.calls.lock().unwrap().push(args.to_vec());
            return Ok(CommandOutput {
                stdout: "[]".into(),
                stderr: String::new(),
            });
        }
        if args[0] == "resume" && self.resume_stays_paused {
            self.runtime.calls.lock().unwrap().push(args.to_vec());
            return Ok(CommandOutput {
                stdout: String::new(),
                stderr: String::new(),
            });
        }
        if args[0] == "stop" && self.failure == "stop-running" {
            self.runtime.calls.lock().unwrap().push(args.to_vec());
            *self.runtime.state.lock().unwrap() = "Running";
            return Err(error("Stop failed after the source resumed."));
        }
        self.runtime.run(paths, args, timeout)
    }
}

fn failed_full_capture_settles_its_paused_source(failure: &'static str) {
    let _test_state = crate::test_support::global_state();
    for resume_failure in ["", "resume"] {
        let directory = tempfile::tempdir().unwrap();
        let paths = restore_fixture(&directory, None);
        let runner = FailedCaptureRunner::new(failure, resume_failure);
        let guard = runner
            .gate
            .computer(ID, "dev", "Creating checkpoint")
            .unwrap();
        guard.allow_cancel();
        let reported = capture_with(&runner, &paths, ID, "Failed", "manual")
            .unwrap_err()
            .to_string();
        drop(guard);
        assert_eq!(
            *runner.runtime.state.lock().unwrap(),
            if resume_failure.is_empty() {
                "Running"
            } else {
                "Stopped"
            },
            "{failure} with {resume_failure}: {reported}"
        );
        let record = load(&paths, ID).unwrap();
        assert!(record.inflight_checkpoint.is_none());
        assert_eq!(
            record.checkpoints.len(),
            1,
            "existing checkpoint is preserved"
        );
        if !resume_failure.is_empty() {
            assert!(reported.contains("resume failed"), "{reported}");
            assert!(record
                .checkpoint_operation
                .unwrap()
                .error
                .unwrap()
                .contains("resume failed"));
        }
    }
}

#[test]
fn full_capture_error_settles_its_paused_source() {
    failed_full_capture_settles_its_paused_source("error");
}

#[test]
fn full_capture_timeout_settles_its_paused_source() {
    failed_full_capture_settles_its_paused_source("timeout");
}

#[test]
fn full_capture_cancellation_settles_its_paused_source() {
    failed_full_capture_settles_its_paused_source("cancel");
}

#[test]
fn full_capture_verification_failure_settles_its_paused_source() {
    failed_full_capture_settles_its_paused_source("verify");
}

#[test]
fn successful_resume_response_without_running_state_uses_force_stop() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = restore_fixture(&directory, None);
    let mut runner = FailedCaptureRunner::new("error", "");
    runner.resume_stays_paused = true;
    capture_with(&runner, &paths, ID, "Failed", "manual").unwrap_err();
    assert_eq!(*runner.runtime.state.lock().unwrap(), "Stopped");
    assert!(runner
        .runtime
        .calls
        .lock()
        .unwrap()
        .iter()
        .any(|args| { args == &["stop".to_string(), "--force".to_string(), "dev".to_string()] }));
}

#[test]
fn failed_resume_and_stop_remain_recoverable_through_ordinary_stop() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = restore_fixture(&directory, None);
    let runner = FailedCaptureRunner::new("error", "resume|stop");
    let reported = capture_with(&runner, &paths, ID, "Failed", "manual")
        .unwrap_err()
        .to_string();
    assert_eq!(*runner.runtime.state.lock().unwrap(), "Paused");
    assert!(
        reported.contains("resume failed") && reported.contains("stop failed"),
        "{reported}"
    );
    assert!(load(&paths, ID).unwrap().inflight_checkpoint.is_some());
    let retry = journal_runner("Paused", "resume");
    let device = DeviceResources {
        logical_cpus: 8,
        physical_memory_bytes: Some(16 * 1024 * 1024 * 1024),
    };
    super::super::super::lifecycle_recovery::perform(&retry, &paths, &device, "stop", "dev")
        .unwrap();
    assert_eq!(*retry.state.lock().unwrap(), "Stopped");
    assert!(!retry
        .calls
        .lock()
        .unwrap()
        .iter()
        .any(|args| args[0] == "remove"));
}

#[test]
fn failed_source_inspection_preserves_recovery_for_relaunch() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = restore_fixture(&directory, None);
    let runner = FailedCaptureRunner::new("inspect", "");
    let reported = capture_with(&runner, &paths, ID, "Failed", "manual")
        .unwrap_err()
        .to_string();
    assert!(reported.contains("Source inspection failed"), "{reported}");
    assert!(load(&paths, ID).unwrap().inflight_checkpoint.is_some());
    let retry = FailedCaptureRunner::new("error", "resume");
    *retry.runtime.state.lock().unwrap() = "Paused";
    let recovery = recover_interrupted(&retry, &paths).unwrap();
    assert!(recovery.unresolved.is_empty());
    assert!(recovery.cleanup_error.is_none());
    assert_eq!(*retry.runtime.state.lock().unwrap(), "Stopped");
    let record = load(&paths, ID).unwrap();
    assert!(record.inflight_checkpoint.is_none());
    let reported = record.checkpoint_operation.unwrap().error.unwrap();
    assert!(
        reported.contains("Source inspection failed") && reported.contains("resume failed"),
        "{reported}"
    );
}

#[test]
fn ordinary_start_can_recover_a_failed_capture_without_a_restore_journal() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = restore_fixture(&directory, None);
    let runner = FailedCaptureRunner::new("error", "resume|stop");
    capture_with(&runner, &paths, ID, "Failed", "manual").unwrap_err();
    let retry = FailedCaptureRunner::new("error", "resume");
    *retry.runtime.state.lock().unwrap() = "Paused";
    let device = DeviceResources {
        logical_cpus: 8,
        physical_memory_bytes: Some(16 * 1024 * 1024 * 1024),
    };
    super::super::super::explicit_computer_action_with(&retry, &paths, &device, "start", "dev")
        .unwrap();
    assert_eq!(*retry.runtime.state.lock().unwrap(), "Running");
}

#[test]
fn checkpoint_retry_recovers_the_source_before_capturing_again() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = restore_fixture(&directory, None);
    let runner = FailedCaptureRunner::new("error", "resume|stop");
    capture_with(&runner, &paths, ID, "Failed", "manual").unwrap_err();
    let retry = FailedCaptureRunner::new("success", "resume");
    *retry.runtime.state.lock().unwrap() = "Paused";
    capture_with(&retry, &paths, ID, "Retry", "manual").unwrap();
    assert_eq!(*retry.runtime.state.lock().unwrap(), "Stopped");
    let record = load(&paths, ID).unwrap();
    assert_eq!(record.checkpoints[0].name, "Retry");
    assert_eq!(record.checkpoints[0].scope, "disk");
    assert!(record.inflight_checkpoint.is_none());
}

#[test]
fn failed_full_capture_never_recovers_a_replacement_computer() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = restore_fixture(&directory, None);
    let runner = FailedCaptureRunner::new("identity", "");
    let reported = capture_with(&runner, &paths, ID, "Failed", "manual")
        .unwrap_err()
        .to_string();
    assert!(reported.contains("identity changed"), "{reported}");
    assert!(load(&paths, ID).unwrap().inflight_checkpoint.is_some());
    assert!(runner
        .runtime
        .calls
        .lock()
        .unwrap()
        .iter()
        .all(|args| { !matches!(args[0].as_str(), "resume" | "stop" | "remove") }));
}

#[test]
fn failed_paused_source_recovery_is_reported_for_its_owner_and_keeps_its_journal() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = restore_fixture(&directory, None);
    let runner = FailedCaptureRunner::new("error", "resume|stop");
    capture_with(&runner, &paths, ID, "Failed", "manual").unwrap_err();
    let recovery = recover_interrupted(&runner, &paths).unwrap();
    assert_eq!(recovery.unresolved.len(), 1);
    let reported = recovery.unresolved[ID].to_string();
    assert!(
        reported.contains("resume failed") && reported.contains("stop failed"),
        "{reported}"
    );
    assert!(load(&paths, ID).unwrap().inflight_checkpoint.is_some());
    assert_eq!(*runner.runtime.state.lock().unwrap(), "Paused");
}

#[test]
fn recovery_reports_the_verified_state_when_a_failed_stop_finds_a_running_source() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = restore_fixture(&directory, None);
    let runner = FailedCaptureRunner::new("stop-running", "resume");
    let reported = capture_with(&runner, &paths, ID, "Failed", "manual")
        .unwrap_err()
        .to_string();
    assert_eq!(*runner.runtime.state.lock().unwrap(), "Running");
    assert!(reported.contains("now Running"), "{reported}");
    assert!(!reported.contains("Start it to continue"), "{reported}");
    assert!(load(&paths, ID).unwrap().inflight_checkpoint.is_none());
}
