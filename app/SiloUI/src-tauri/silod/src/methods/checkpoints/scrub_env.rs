//! The engine's means for an export without logins ([`silo_engine::export_scrub`]): the hidden
//! copy is forked, started, listed, stopped and dropped with the engine's own methods, as a
//! recipe build is.

use std::{sync::Arc, time::Duration};

use serde_json::{json, Value};
use silo_api::{jobs::JobState, methods::MethodName};
use silo_engine::{
    context::EngineContext,
    changes::{
        collect::{self, Options, TargetRunner},
        credentials::login_paths_in,
    },
    checkpoints::ForkOptions,
    export_scrub::ScrubEnv,
    guest::{self, ExecRequest, GuestUser},
    recipes::{BuildEnv, ExecOut},
};

use super::{
    recipes::DispatchEnv,
    {super::guest::resolve_target, service_for},
};
use crate::context::SilodContext;

/// The longest one command in the copy may run; trimming a large data disk is the slow one.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const JOB_TIMEOUT: Duration = Duration::from_secs(60 * 60);
const MAX_OUTPUT: usize = 1024 * 1024;

pub(crate) fn env(silod: Arc<SilodContext>, handle: tokio::runtime::Handle) -> Arc<dyn ScrubEnv> {
    Arc::new(Scrub {
        env: DispatchEnv { silod, handle },
    })
}

struct Scrub {
    env: DispatchEnv,
}

impl Scrub {
    /// Waits for a job the engine started and returns its result.
    fn wait(&self, job: silo_api::jobs::JobId) -> Result<Value, String> {
        let context = self.env.silod.as_ref();
        let deadline = std::time::Instant::now() + JOB_TIMEOUT;
        loop {
            let lookup = silo_engine::jobs::get_at(context, &job, None)
                .map_err(|error| error.to_string())?
                .ok_or("The job disappeared.")?;
            if lookup.state == JobState::Finished {
                if let Ok(registry) = context.job_registry() {
                    let _ = registry.acknowledge(&job);
                }
                let outcome = lookup.outcome.ok_or("The job ended without a result.")?;
                if let Some(error) = outcome.error {
                    return Err(error.message);
                }
                return Ok(outcome.result.unwrap_or(Value::Null));
            }
            if std::time::Instant::now() > deadline {
                return Err("The job took too long.".into());
            }
            std::thread::sleep(Duration::from_millis(300));
        }
    }
}

impl ScrubEnv for Scrub {
    fn computers(&self) -> Result<Vec<(String, String)>, String> {
        BuildEnv::computers(&self.env)
    }

    fn fork(&self, source_id: &str, name: &str) -> Result<String, String> {
        let _entered = self.env.handle.enter();
        let job = service_for(self.env.silod.as_ref())
            .start_fork(
                source_id,
                None,
                name,
                ForkOptions {
                    closed_network: true,
                    ..ForkOptions::default()
                },
            )
            .map_err(|failure| failure.message)?;
        self.wait(job)?;
        self.env
            .rows()?
            .into_iter()
            .find(|row| row["name"].as_str() == Some(name))
            .and_then(|row| row["id"].as_str().map(str::to_owned))
            .ok_or_else(|| "The copy did not appear.".to_owned())
    }

    fn start(&self, computer_id: &str) -> Result<(), String> {
        self.env
            .job(MethodName::ComputersStart, json!({"computerId": computer_id}))
            .map(|_| ())
    }

    fn run(&self, computer_id: &str, script: &str, stdin: &[u8]) -> Result<ExecOut, String> {
        let target =
            resolve_target(&self.env.silod, computer_id).map_err(|error| error.message)?;
        let request = ExecRequest {
            argv: vec!["sh".into(), "-c".into(), script.to_owned()],
            cwd: None,
            env: Vec::new(),
            user: GuestUser::Root,
            stdin: (!stdin.is_empty()).then(|| stdin.to_vec()),
            timeout: COMMAND_TIMEOUT,
            max_stdout: MAX_OUTPUT,
        };
        let out = guest::run_bounded(&target, &request).map_err(|error| error.message)?;
        Ok(ExecOut {
            code: out.exit_code.unwrap_or(-1),
            stdout: out.stdout,
            stderr: out.stderr,
        })
    }

    fn login_paths(&self, computer_id: &str) -> Result<Vec<String>, String> {
        let target =
            resolve_target(&self.env.silod, computer_id).map_err(|error| error.message)?;
        let manifest = collect::capture(&TargetRunner(target), &Options::default())
            .map_err(|error| error.message)?;
        login_paths_in(&manifest)
    }

    fn stop(&self, computer_id: &str) -> Result<(), String> {
        BuildEnv::stop(&self.env, computer_id)
    }

    fn delete(&self, computer_id: &str) -> Result<(), String> {
        BuildEnv::delete(&self.env, computer_id)
    }
}
