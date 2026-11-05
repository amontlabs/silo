//! `recipes.*`: build a recipe into a hidden computer and checkpoint, list the builds, remove
//! one. The build drives the engine's own method handlers, so it creates, starts, stops and
//! deletes computers exactly as a client would.

use std::{sync::Arc, time::Duration};

use serde_json::{json, Value};
use silo_api::{
    jobs::JobState,
    methods::{
        recipes::{
            RecipeBuildParams, RecipeBuildResult, RecipeOs, RecipeRemoveParams, RecipesListResult,
        },
        MethodName,
    },
};
use silo_engine::{
    context::{Audience, EngineContext},
    guest::{self, ExecRequest, GuestUser},
    recipes::{self, BuildEnv, Cache, Created, ExecOut, RunAs},
};

use crate::{
    context::SilodContext,
    dispatcher::{DispatchError, HandlerOutput, RequestContext},
};

use super::{
    super::{guest::resolve_target, HandlerContext},
    output,
};

/// The longest one build step may run.
const STEP_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const JOB_TIMEOUT: Duration = Duration::from_secs(60 * 60);

pub(super) struct DispatchEnv {
    pub(super) silod: Arc<SilodContext>,
    pub(super) handle: tokio::runtime::Handle,
}

impl DispatchEnv {
    /// The build is no client with an event stream, so it runs as the bridge does: work that
    /// would answer only "the client that asked" is announced to every subscriber instead,
    /// and reads need no subscription to order against.
    fn context(&self) -> HandlerContext {
        HandlerContext::for_owner_bridge(
            self.silod.clone(),
            RequestContext {
                client_id: "recipes".to_owned(),
                protocol: 1,
                client: None,
            },
        )
    }

    pub(super) fn call(&self, method: MethodName, params: Value) -> Result<Value, String> {
        let context = self.context();
        self.handle
            .block_on(crate::methods::dispatch(&context, method, &params))
            .map(|output| output.value)
            .map_err(|error| error.message)
    }

    /// Starts a job method and waits for it.
    fn job(&self, method: MethodName, params: Value) -> Result<Value, String> {
        let started = self.call(method, params)?;
        let id = started
            .get("jobId")
            .and_then(Value::as_str)
            .ok_or("The engine did not accept the job.")?;
        let job = silo_api::jobs::JobId::new(id).map_err(|error| error.to_string())?;
        let context = self.silod.as_ref();
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

    pub(super) fn rows(&self) -> Result<Vec<Value>, String> {
        let listed = self.call(MethodName::ComputersList, json!({}))?;
        let mut all = Vec::new();
        for (path, linux) in [("/linux/computers", true), ("/macos/computers", false)] {
            for computer in listed
                .pointer(path)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let base = if linux {
                    computer.get("configuration").unwrap_or(computer)
                } else {
                    computer
                };
                all.push(json!({
                    "id": base.get("id"),
                    "name": base.get("name"),
                    "macos": !linux,
                    "state": computer.get("state"),
                    "configuration": if linux { base.clone() } else { Value::Null },
                    "checkpoints": computer.get("checkpoints").cloned().unwrap_or(json!([])),
                }));
            }
        }
        Ok(all)
    }

    fn row(&self, id: &str) -> Result<Option<Value>, String> {
        Ok(self
            .rows()?
            .into_iter()
            .find(|row| row["id"].as_str() == Some(id)))
    }
}

impl BuildEnv for DispatchEnv {
    fn image_version(&self, os: RecipeOs) -> String {
        match os {
            RecipeOs::Linux => silo_engine::runtime::guest_image::pinned_version()
                .unwrap_or_else(|| "unknown".into()),
            RecipeOs::Macos => {
                silo_engine::macos_computers::recipe_image_version(&self.silod.paths().app_data)
            }
        }
    }

    fn macos_running(&self) -> Result<usize, String> {
        Ok(self
            .rows()?
            .iter()
            .filter(|row| row["macos"] == true && row["state"] == "running")
            .count())
    }

    fn computers(&self) -> Result<Vec<(String, String)>, String> {
        Ok(self
            .rows()?
            .into_iter()
            .filter_map(|row| {
                Some((
                    row["id"].as_str()?.to_owned(),
                    row["name"].as_str()?.to_owned(),
                ))
            })
            .collect())
    }

    fn create(
        &self,
        name: &str,
        os: RecipeOs,
        cpus: u32,
        memory_gib: u32,
        disk_gib: u32,
    ) -> Result<Created, String> {
        let params = match os {
            RecipeOs::Linux => {
                let cpus = u8::try_from(cpus).map_err(|_| "cpus is too large.".to_owned())?;
                json!({
                    "kind": "linux",
                    "configuration": {
                        "id": uuid::Uuid::new_v4().to_string(),
                        "name": name,
                        "cpus": cpus,
                        "maxCPUs": cpus,
                        "memoryGiB": memory_gib,
                        "maxMemoryGiB": memory_gib,
                        "workspaceStorageGiB": disk_gib,
                        "runtimeStorageGiB": 10,
                    },
                })
            }
            RecipeOs::Macos => json!({
                "kind": "macos",
                "request": {
                    "name": name,
                    "cpus": cpus,
                    "memoryGiB": memory_gib,
                    "diskGiB": disk_gib,
                },
            }),
        };
        self.job(MethodName::ComputersCreate, params)?;
        let id = self
            .rows()?
            .into_iter()
            .find(|row| row["name"].as_str() == Some(name))
            .and_then(|row| row["id"].as_str().map(str::to_owned))
            .ok_or("The builder computer did not appear.")?;
        self.job(MethodName::ComputersStart, json!({"computerId": id}))?;
        Ok(Created {
            id,
            name: name.to_owned(),
        })
    }

    fn run(
        &self,
        computer: &str,
        who: RunAs,
        script: &str,
        args: &[(String, String)],
    ) -> Result<ExecOut, String> {
        let target = resolve_target(&self.silod, computer).map_err(|error| error.message)?;
        let request = ExecRequest {
            argv: vec!["bash".into(), "-c".into(), script.to_owned()],
            cwd: None,
            env: args
                .iter()
                .map(|(name, value)| (format!("A_{name}"), value.clone()))
                .collect(),
            user: match who {
                RunAs::Root => GuestUser::Root,
                RunAs::Account => GuestUser::Account,
            },
            stdin: None,
            timeout: STEP_TIMEOUT,
            max_stdout: 16 * 1024 * 1024,
        };
        let out = guest::run_bounded(&target, &request).map_err(|error| error.message)?;
        Ok(ExecOut {
            code: out.exit_code.unwrap_or(-1),
            stdout: out.stdout,
            stderr: out.stderr,
        })
    }

    fn checkpoint(&self, computer: &str, name: &str) -> Result<String, String> {
        let result = self.job(
            MethodName::CheckpointsCreate,
            json!({"computerId": computer, "name": name}),
        )?;
        result
            .pointer("/checkpoint/id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| "The checkpoint job returned no checkpoint id.".into())
    }

    fn checkpoint_exists(&self, computer: &str, checkpoint: &str) -> bool {
        self.row(computer)
            .ok()
            .flatten()
            .and_then(|row| {
                row["checkpoints"]
                    .as_array()
                    .map(|list| list.iter().any(|cp| cp["id"] == checkpoint))
            })
            .unwrap_or(false)
    }

    fn stop(&self, computer: &str) -> Result<(), String> {
        self.job(MethodName::ComputersStop, json!({"computerId": computer}))
            .map(|_| ())
    }

    fn delete(&self, computer: &str) -> Result<(), String> {
        let Some(row) = self.row(computer)? else {
            return Ok(());
        };
        if row["state"] != "stopped" {
            let _ = self.stop(computer);
            for _ in 0..120 {
                match self.row(computer)? {
                    Some(now) if now["state"] != "stopped" => {
                        std::thread::sleep(Duration::from_secs(1))
                    }
                    _ => break,
                }
            }
        }
        let mut params = json!({"computerId": computer});
        if !row["configuration"].is_null() {
            params["expected"] = row["configuration"].clone();
        }
        let mut last = String::new();
        for _ in 0..10 {
            match self.job(MethodName::ComputersDelete, params.clone()) {
                Ok(_) => return Ok(()),
                Err(message) if message.contains("Stop the computer first") => {
                    last = message;
                    std::thread::sleep(Duration::from_secs(2));
                }
                Err(message) => return Err(message),
            }
        }
        Err(last)
    }

    fn note(&self, _line: &str) {}
}

fn cache(context: &HandlerContext) -> Cache {
    Cache::new(&context.engine_context().paths().app_data)
}

pub(super) async fn build(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: RecipeBuildParams = context.decode_params(params)?;
    recipes::validate(&params.spec).map_err(|message| DispatchError {
        code: silo_api::jobs::ErrorCode::Internal,
        message,
    })?;
    let started = context.start_job("recipes.build")?;
    let job_id = started.job_id.clone();
    let silod = Arc::new(context.silod_context().clone());
    let handle = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let env = DispatchEnv {
            silod: silod.clone(),
            handle,
        };
        let cache = Cache::new(&silod.paths().app_data);
        let outcome = recipes::build(
            &env,
            &cache,
            &params.name,
            &params.spec,
            params.force.unwrap_or(false),
        );
        let finished = match outcome {
            Ok(done) => {
                let result = RecipeBuildResult {
                    recipe: done.entry,
                    built: done.built,
                };
                silo_engine::jobs::finish_with_result(
                    silod.as_ref(),
                    job_id,
                    serde_json::to_value(result).unwrap_or(Value::Null),
                    Audience::All,
                )
            }
            Err(message) => silo_engine::jobs::finish_with_error(
                silod.as_ref(),
                job_id,
                silo_api::jobs::ErrorCode::Internal,
                message,
                Audience::All,
            ),
        };
        if let Err(error) = finished {
            eprintln!("silo-engine: could not persist the recipes.build outcome: {error}");
        }
    });
    output(started)
}

pub(super) async fn list(context: &HandlerContext) -> Result<HandlerOutput, DispatchError> {
    let cache = cache(context);
    let recipes = tokio::task::spawn_blocking(move || cache.list())
        .await
        .map_err(|_| DispatchError::internal("The engine task stopped unexpectedly."))?;
    output(RecipesListResult { recipes })
}

pub(super) async fn remove(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: RecipeRemoveParams = context.decode_params(params)?;
    let silod = Arc::new(context.silod_context().clone());
    let handle = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let env = DispatchEnv {
            silod: silod.clone(),
            handle,
        };
        recipes::remove(&env, &Cache::new(&silod.paths().app_data), &params.name)
    })
    .await
    .map_err(|_| DispatchError::internal("The engine task stopped unexpectedly."))?
    .map_err(DispatchError::internal)?;
    output(())
}
