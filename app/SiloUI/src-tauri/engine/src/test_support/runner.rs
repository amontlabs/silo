//! Ordered runtime expectations. An unlisted command is a test failure, even if
//! the caller would otherwise recover from a runtime error.
use crate::runtime::{CommandOutput, RuntimeError, RuntimePaths, RuntimeRunner};
use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Duration;

pub(crate) struct ExpectedCommand {
    args: Vec<String>,
    timeout: Option<Duration>,
    result: Result<CommandOutput, RuntimeError>,
}

impl ExpectedCommand {
    pub(crate) fn ok(
        args: impl IntoIterator<Item = impl Into<String>>,
        stdout: impl Into<String>,
    ) -> Self {
        Self::result(
            args,
            Ok(CommandOutput {
                stdout: stdout.into(),
                stderr: String::new(),
            }),
        )
    }

    pub(crate) fn error(
        args: impl IntoIterator<Item = impl Into<String>>,
        error: RuntimeError,
    ) -> Self {
        Self::result(args, Err(error))
    }

    pub(crate) fn result(
        args: impl IntoIterator<Item = impl Into<String>>,
        result: Result<CommandOutput, RuntimeError>,
    ) -> Self {
        Self {
            args: args.into_iter().map(Into::into).collect(),
            timeout: None,
            result,
        }
    }

    pub(crate) fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }
}

struct Script {
    pending: VecDeque<ExpectedCommand>,
    calls: Vec<Vec<String>>,
}

pub(crate) struct ScriptedRunner {
    script: Mutex<Script>,
}

impl ScriptedRunner {
    pub(crate) fn new(commands: impl IntoIterator<Item = ExpectedCommand>) -> Self {
        Self {
            script: Mutex::new(Script {
                pending: commands.into_iter().collect(),
                calls: Vec::new(),
            }),
        }
    }

    pub(crate) fn calls(&self) -> Vec<Vec<String>> {
        self.script.lock().unwrap().calls.clone()
    }

    pub(crate) fn assert_finished(&self) {
        let script = self.script.lock().unwrap();
        assert!(
            script.pending.is_empty(),
            "{} expected runtime command(s) were not run; next: {:?}",
            script.pending.len(),
            script.pending.front().map(|command| &command.args)
        );
    }
}

impl RuntimeRunner for ScriptedRunner {
    fn run(
        &self,
        _: &RuntimePaths,
        args: &[String],
        timeout: Duration,
    ) -> Result<CommandOutput, RuntimeError> {
        let mut script = self.script.lock().unwrap();
        script.calls.push(args.to_vec());
        let expected = script
            .pending
            .pop_front()
            .unwrap_or_else(|| panic!("Unexpected runtime command: {args:?}"));
        assert_eq!(
            args, expected.args,
            "Runtime command did not match the next expectation"
        );
        if let Some(expected_timeout) = expected.timeout {
            assert_eq!(timeout, expected_timeout, "Runtime timeout for {args:?}");
        }
        expected.result
    }
}

impl Drop for ScriptedRunner {
    fn drop(&mut self) {
        // Preserve the original assertion if a command mismatch already panicked.
        if !std::thread::panicking() {
            self.assert_finished();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(
        runner: &ScriptedRunner,
        args: &[&str],
        timeout: Duration,
    ) -> Result<CommandOutput, RuntimeError> {
        let dir = tempfile::tempdir().unwrap();
        runner.run(
            &crate::test_support::paths(dir.path()),
            &args.iter().map(|arg| (*arg).into()).collect::<Vec<_>>(),
            timeout,
        )
    }

    #[test]
    fn returns_ordered_outputs_and_typed_errors() {
        let runner = ScriptedRunner::new([
            ExpectedCommand::result(
                ["inspect", "dev"],
                Ok(CommandOutput {
                    stdout: "running".into(),
                    stderr: "diagnostic".into(),
                }),
            )
            .with_timeout(Duration::from_secs(10)),
            ExpectedCommand::error(["stop", "dev"], RuntimeError::Busy),
        ]);
        let output = run(&runner, &["inspect", "dev"], Duration::from_secs(10)).unwrap();
        assert_eq!(output.stdout, "running");
        assert_eq!(output.stderr, "diagnostic");
        assert!(matches!(
            run(&runner, &["stop", "dev"], Duration::from_secs(60)),
            Err(RuntimeError::Busy)
        ));
        assert_eq!(
            runner.calls(),
            vec![vec!["inspect", "dev"], vec!["stop", "dev"]]
        );
        runner.assert_finished();
    }

    #[test]
    #[should_panic(expected = "Unexpected runtime command")]
    fn rejects_unexpected_mutations() {
        let runner = ScriptedRunner::new([]);
        let _ = run(
            &runner,
            &["delete", "dev", "--force"],
            Duration::from_secs(60),
        );
    }

    #[test]
    #[should_panic(expected = "Unexpected runtime command")]
    fn rejects_extra_reads_after_consuming_the_script() {
        let runner = ScriptedRunner::new([ExpectedCommand::ok(["inspect", "dev"], "")]);
        run(&runner, &["inspect", "dev"], Duration::from_secs(10)).unwrap();
        let _ = run(&runner, &["list"], Duration::from_secs(10));
    }

    #[test]
    #[should_panic(expected = "Runtime command did not match the next expectation")]
    fn rejects_wrong_command_arguments() {
        let runner = ScriptedRunner::new([ExpectedCommand::ok(["stop", "dev"], "")]);
        let _ = run(&runner, &["stop", "other"], Duration::from_secs(60));
    }

    #[test]
    #[should_panic(expected = "Runtime timeout")]
    fn rejects_wrong_timeout() {
        let runner = ScriptedRunner::new([
            ExpectedCommand::ok(["stop", "dev"], "").with_timeout(Duration::from_secs(60))
        ]);
        let _ = run(&runner, &["stop", "dev"], Duration::from_secs(1));
    }

    #[test]
    #[should_panic(expected = "expected runtime command(s) were not run")]
    fn rejects_unconsumed_expectations_on_drop() {
        let _runner = ScriptedRunner::new([ExpectedCommand::ok(["inspect", "dev"], "")]);
    }
}
