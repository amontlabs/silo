//! The engine's [`KeepEnv`]: commands through `guest::run_bounded` (`msb exec` for Linux,
//! SSH for macOS) and checkpoints through the two-platform checkpoint service.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use silo_api::{
    jobs::{JobId, JobState},
    methods::checkpoints::CheckpointKind,
};

use super::{
    env::{CheckpointRef, ExecOut, GuestCmd, KeepEnv, Os, RunAs},
    journal::Journal,
};
use crate::{
    checkpoints::Service,
    guest::{self, ExecRequest, GuestUser},
    jobs,
    runtime::{self, RuntimeHost},
};

/// Longest a single checkpoint or restore may take.
const CHECKPOINT_WAIT: Duration = Duration::from_secs(30 * 60);
/// Largest file a keep carries through the host.
const MAX_FILE_BYTES: usize = 256 * 1024 * 1024;

pub struct GuestEnv {
    host: Arc<dyn RuntimeHost>,
    service: Service,
    journal: Journal,
    progress: Box<dyn Fn(&str) + Send + Sync>,
}

impl GuestEnv {
    pub fn new(
        host: Arc<dyn RuntimeHost>,
        service: Service,
        journal: Journal,
        progress: Box<dyn Fn(&str) + Send + Sync>,
    ) -> Self {
        Self {
            host,
            service,
            journal,
            progress,
        }
    }

    fn target(&self, computer: &str) -> Result<guest::Target, String> {
        let context = self.host.engine_context();
        guest::resolve(
            runtime::runtime_paths(self.host.as_ref()),
            &context.paths().app_data,
            computer,
        )
        .map_err(|error| error.message)
    }

    /// Waits for a job the checkpoint service started and returns its result.
    fn finish(&self, job: JobId) -> Result<serde_json::Value, String> {
        let context = self.host.engine_context();
        let deadline = Instant::now() + CHECKPOINT_WAIT;
        loop {
            let lookup = jobs::get_at(context.as_ref(), &job, None)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "The checkpoint job disappeared.".to_owned())?;
            if lookup.state == JobState::Finished {
                if let Ok(registry) = context.job_registry() {
                    let _ = registry.acknowledge(&job);
                }
                let outcome = lookup
                    .outcome
                    .ok_or_else(|| "The checkpoint job ended without a result.".to_owned())?;
                if let Some(error) = outcome.error {
                    return Err(error.message);
                }
                return Ok(outcome.result.unwrap_or(serde_json::Value::Null));
            }
            if Instant::now() > deadline {
                return Err("The checkpoint took too long.".into());
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }
}

impl KeepEnv for GuestEnv {
    fn os(&self, computer: &str) -> Result<Os, String> {
        Ok(if self.target(computer)?.is_macos() {
            Os::Macos
        } else {
            Os::Linux
        })
    }

    fn run(&self, computer: &str, who: RunAs, cmd: &GuestCmd) -> Result<ExecOut, String> {
        let target = self.target(computer)?;
        let request = ExecRequest {
            argv: vec!["bash".into(), "-c".into(), cmd.script.clone()],
            cwd: None,
            env: cmd
                .args
                .iter()
                .map(|(name, value)| (format!("A_{name}"), value.clone()))
                .collect(),
            user: match who {
                RunAs::Root => GuestUser::Root,
                RunAs::Account => GuestUser::Account,
            },
            stdin: cmd.stdin.clone(),
            timeout: cmd.timeout,
            max_stdout: MAX_FILE_BYTES,
        };
        let output = guest::run_bounded(&target, &request).map_err(|error| error.message)?;
        Ok(ExecOut {
            code: output.exit_code.unwrap_or(-1),
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }

    fn checkpoint_before_keep(&self, computer: &str, name: &str) -> Result<CheckpointRef, String> {
        // macOS computers have no before-keep kind: theirs is a manual checkpoint with this name.
        let kind = if self.os(computer)? == Os::Macos {
            CheckpointKind::Manual
        } else {
            CheckpointKind::BeforeKeep
        };
        let job = self
            .service
            .start_create(computer, name, Some(kind))
            .map_err(|failure| failure.message)?;
        let result = self.finish(job)?;
        let checkpoint = result
            .get("checkpoint")
            .ok_or("The checkpoint job returned no checkpoint.")?;
        let text = |key: &str| {
            checkpoint
                .get(key)
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_owned()
        };
        let reference = CheckpointRef {
            id: text("id"),
            name: text("name"),
            created_at: text("createdAt"),
        };
        if reference.id.is_empty() {
            return Err("The checkpoint job returned no checkpoint id.".into());
        }
        self.journal.record_checkpoint(computer, &reference)?;
        Ok(reference)
    }

    fn before_keep_checkpoints(&self, computer: &str) -> Result<Vec<CheckpointRef>, String> {
        Ok(self.journal.checkpoints(computer))
    }

    fn restore(&self, computer: &str, checkpoint_id: &str) -> Result<(), String> {
        let job = self
            .service
            .start_restore(computer, checkpoint_id)
            .map_err(|failure| failure.message)?;
        self.finish(job).map(|_| ())
    }

    fn note(&self, line: &str) {
        (self.progress)(line);
    }
}
