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
        // The collector reads a Linux computer as root, so a keep does too: commands that
        // touch repositories and folders start as root and the scripts hand git and hooks to
        // the owner of the folder they work in.
        let who = if who == RunAs::Account
            && !target.is_macos()
            && (cmd.tag.starts_with("git_") || cmd.tag == "hook")
        {
            RunAs::Root
        } else {
            who
        };
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

    fn name_of(&self, computer: &str) -> String {
        self.target(computer)
            .map_or_else(|_| computer.to_owned(), |target| target.name().to_owned())
    }

    fn after_kept(&self, computer: &str) {
        let Ok(target) = self.target(computer) else {
            return;
        };
        if target.is_macos() {
            if let Some(macos) = self.host.macos_context() {
                let _ =
                    crate::macos_computers::reapply_macos_computer_use_approval(&macos, computer);
            }
        } else if let Ok(paths) = runtime::runtime_paths(self.host.as_ref()) {
            // `computer` may be the computer's id; the apply is keyed by its name.
            crate::computer_use::reapply_after_keep(&paths, target.name());
        }
    }

    fn checkpoint_before_keep(&self, computer: &str, name: &str) -> Result<CheckpointRef, String> {
        // A running Linux computer is captured with its memory (`--full --guest-flush
        // required`) and a running Mac with its saved state, so an undo resumes it.
        let job = self
            .service
            .start_create(computer, name, Some(CheckpointKind::BeforeKeep))
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
            scope: text("scope"),
        };
        if reference.id.is_empty() {
            return Err("The checkpoint job returned no checkpoint id.".into());
        }
        Ok(reference)
    }

    fn restore(&self, computer: &str, checkpoint_id: &str) -> Result<(), String> {
        let job = self
            .service
            .start_restore(computer, checkpoint_id)
            .map_err(|failure| failure.message)?;
        self.finish(job).map(|_| ())
    }

    fn start(&self, computer: &str) -> Result<(), String> {
        let context = self.host.engine_context();
        let accepted = jobs::allocate(context.as_ref(), "computers.start")
            .map_err(|failure| failure.message().to_owned())?;
        let job = accepted.job_id;
        let target = self.target(computer)?;
        let result = if target.is_macos() {
            let macos = self
                .host
                .macos_context()
                .ok_or("macOS computers are not available here.")?;
            tokio::runtime::Handle::try_current()
                .map_err(|error| error.to_string())?
                .block_on(crate::macos_computers::macos_computer_action_with_job(
                    macos,
                    computer.to_owned(),
                    crate::macos_computers::Action::Start,
                    job.clone(),
                    "computers.start",
                ))
        } else {
            runtime::computer_action_with_host_and_job(
                self.host.as_ref(),
                "start".into(),
                target.name().to_owned(),
                None,
                job.clone(),
                "computers.start",
            )
            .map(|_| ())
            .map_err(|error| error.message)
        };
        if let Ok(registry) = context.job_registry() {
            let _ = registry.acknowledge(&job);
        }
        result
    }

    fn hold(&self, computer: &str, label: &str) -> Result<Box<dyn std::any::Any>, String> {
        let target = self.target(computer)?;
        let guard = runtime::OPERATIONS
            .kind(runtime::operation_gate::OperationKind::Other)
            .computer(target.id(), target.name(), label)
            .map_err(|error| error.to_string())?;
        Ok(Box::new(guard))
    }

    fn recover(&self, computer: &str, checkpoint_id: &str, held: &dyn Fn()) -> Result<(), String> {
        // The restore job takes the lane itself, so the hold is handed over: the job is
        // queued first, then the hold is dropped, and everything else queues behind it.
        let gate = self.hold(computer, "Recovering an interrupted keep").ok();
        held();
        let Some(gate) = gate else {
            return self.restore(computer, checkpoint_id);
        };
        let job = self
            .service
            .start_restore(computer, checkpoint_id)
            .map_err(|failure| failure.message)?;
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            let queued = runtime::OPERATIONS
                .snapshot()
                .waiting
                .iter()
                .any(|entry| entry.job_id.as_ref() == Some(&job));
            let done = jobs::get_at(self.host.engine_context().as_ref(), &job, None)
                .ok()
                .flatten()
                .is_some_and(|lookup| lookup.state == JobState::Finished);
            if queued || done {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        drop(gate);
        self.finish(job).map(|_| ())
    }

    fn note(&self, line: &str) {
        (self.progress)(line);
    }
}
