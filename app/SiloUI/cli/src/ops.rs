//! The operations behind both the commands and the MCP tools. Each returns the JSON the
//! `--json` flag prints; `render` turns the same values into short text.

use std::time::Duration;

use serde_json::{json, Value};
use silo_api::methods::{
    changes::{ChangesBaselineParams, ChangesDiffParams},
    checkpoints::{CheckpointCreateParams, CheckpointForkParams, CheckpointKind},
    common::{ComputerTargetParams, DeviceParams},
    computers::{
        ComputerConfiguration, ComputerCreateParams, ComputerDeleteParams,
        MacosComputerCreateRequest,
    },
    guest::GuestExecParams,
    keep::{ChangesKeepParams, ChangesPromoteParams, ChangesUndoParams, KeepHooks},
    recipes::{RecipeBuildParams, RecipeRemoveParams},
    requests,
    secrets::{SecretSaveAction, SecretSaveRequest, SecretsSaveParams},
};

use crate::{
    config::{Recipe, Settings},
    engine::{find, Computer, Engine, Progress},
    error::CliError,
};

/// The computer that holds the long-lived work; dropping it needs `--force`.
pub const MAIN: &str = "main";

pub async fn list(engine: &Engine) -> Result<Value, CliError> {
    let computers = engine.computers().await?;
    Ok(json!({ "computers": computers.iter().map(Computer::to_json).collect::<Vec<_>>() }))
}

/// One computer with its checkpoints, its recorded quiet state, the Silo secrets assigned to
/// it and its GitHub assignment. A reading the engine cannot give is `null`.
pub async fn status(engine: &Engine, name: &str) -> Result<Value, CliError> {
    let computer = engine.computer(name).await?;
    let mut value = computer.to_json();
    value["quiet"] = computer.quiet.clone().unwrap_or(Value::Null);
    value["boots"] = computer.boots.map_or(Value::Null, Value::from);
    value["secrets"] = match fetch_secrets(engine).await {
        Ok(secrets) => json!(secrets
            .iter()
            .filter(|secret| secret["removing"] != true)
            .filter(|secret| assigned_ids(secret).any(|id| id == computer.id))
            .filter_map(|secret| secret["name"].as_str())
            .collect::<Vec<_>>()),
        Err(_) => Value::Null,
    };
    value["github"] = match engine
        .call(requests::github_state(DeviceParams { device_id: None }))
        .await
    {
        Ok(state) => state
            .get("computers")
            .and_then(Value::as_array)
            .and_then(|policies| {
                policies
                    .iter()
                    .find(|policy| policy["computerId"].as_str() == Some(&computer.id))
            })
            .map(|policy| {
                json!({
                    "repositoryMode": policy["repositoryMode"],
                    "repositories": policy["repositories"],
                    "allRepositoriesAllowChanges": policy["allRepositoriesAllowChanges"],
                    "authenticationMethod": policy["authenticationMethod"],
                })
            })
            .unwrap_or(Value::Null),
        Err(_) => Value::Null,
    };
    value["checkpointList"] = Value::Array(computer.checkpoints);
    Ok(value)
}

fn assigned_ids(secret: &Value) -> impl Iterator<Item = &str> {
    ["computerIds", "pendingComputerIds"]
        .into_iter()
        .filter_map(|key| secret.get(key).and_then(Value::as_array))
        .flatten()
        .filter_map(Value::as_str)
}

async fn fetch_secrets(engine: &Engine) -> Result<Vec<Value>, CliError> {
    let listed = engine
        .call(requests::secrets_list(DeviceParams { device_id: None }))
        .await?;
    Ok(listed.as_array().cloned().unwrap_or_default())
}

/// The secrets by name with the computers they are assigned to. Values are never read.
pub async fn secrets_list(engine: &Engine) -> Result<Value, CliError> {
    let computers = engine.computers().await?;
    let name_of = |id: &str| {
        computers
            .iter()
            .find(|computer| computer.id == id)
            .map_or_else(|| id.to_owned(), |computer| computer.name.clone())
    };
    let secrets = fetch_secrets(engine).await?;
    Ok(json!({ "secrets": secrets.iter().map(|secret| {
        let names = |key: &str| -> Vec<String> {
            secret.get(key).and_then(Value::as_array).into_iter().flatten()
                .filter_map(Value::as_str).map(name_of).collect()
        };
        json!({
            "name": secret["name"],
            "computers": names("computerIds"),
            "pendingComputers": names("pendingComputerIds"),
            "allowedDomains": secret["allowedDomains"],
            "state": secret["state"],
            "removing": secret["removing"],
            "error": secret["error"],
        })
    }).collect::<Vec<_>>() }))
}

/// Adds the secret, or replaces the value of the one with this name. Computers and domains
/// default to the existing secret's own.
pub async fn secret_set(
    engine: &Engine,
    name: &str,
    value: String,
    computers: &[String],
    domains: &[String],
) -> Result<Value, CliError> {
    if value.is_empty() {
        return Err(CliError::new("The secret value is empty."));
    }
    let existing = fetch_secrets(engine)
        .await?
        .into_iter()
        .find(|secret| secret["name"].as_str() == Some(name));
    let known = engine.computers().await?;
    let ids: Vec<String> = if computers.is_empty() {
        existing
            .as_ref()
            .map(|secret| {
                secret["computerIds"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    } else {
        computers
            .iter()
            .map(|computer| find(&known, computer).map(|found| found.id.clone()))
            .collect::<Result<Vec<_>, _>>()?
    };
    let allowed: Vec<String> = if domains.is_empty() {
        existing
            .as_ref()
            .and_then(|secret| serde_json::from_value(secret["allowedDomains"].clone()).ok())
            .unwrap_or_default()
    } else {
        domains.to_vec()
    };
    if existing.is_none() && (ids.is_empty() || allowed.is_empty()) {
        return Err(CliError::new(
            "A new secret needs at least one --computer and one --domain.",
        ));
    }
    save_secret(engine, name, existing.as_ref(), Some(value), ids, allowed).await?;
    secrets_list(engine).await
}

/// Assigns the secret to a computer, or takes it away.
pub async fn secret_assign(
    engine: &Engine,
    name: &str,
    computer: &str,
    assign: bool,
) -> Result<Value, CliError> {
    let existing = fetch_secrets(engine)
        .await?
        .into_iter()
        .find(|secret| secret["name"].as_str() == Some(name))
        .ok_or_else(|| CliError::new(format!("No secret is called {name}.")))?;
    let target = engine.computer(computer).await?;
    let mut ids: Vec<String> = existing["computerIds"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    if assign {
        if !ids.contains(&target.id) {
            ids.push(target.id.clone());
        }
    } else {
        ids.retain(|id| *id != target.id);
        if ids.is_empty() {
            return Err(CliError::new(format!(
                "{name} would have no computer left; a secret needs at least one."
            )));
        }
    }
    let allowed = serde_json::from_value(existing["allowedDomains"].clone()).unwrap_or_default();
    save_secret(engine, name, Some(&existing), None, ids, allowed).await?;
    secrets_list(engine).await
}

async fn save_secret(
    engine: &Engine,
    name: &str,
    existing: Option<&Value>,
    value: Option<String>,
    computer_ids: Vec<String>,
    allowed_domains: Vec<String>,
) -> Result<(), CliError> {
    // A wildcard is kept as it was; this command never widens one.
    let wildcard = allowed_domains.iter().any(|domain| domain.trim() == "*");
    if wildcard && existing.is_none() {
        return Err(CliError::new(
            "A secret that may go to any destination must be created in the app.",
        ));
    }
    engine
        .call(requests::secrets_save(SecretsSaveParams {
            request: SecretSaveRequest {
                operation: if existing.is_some() {
                    SecretSaveAction::Edit
                } else {
                    SecretSaveAction::Add
                },
                id: existing.and_then(|secret| secret["id"].as_str().map(str::to_owned)),
                name: name.to_owned(),
                value,
                computer_ids,
                allowed_domains,
                allow_any_destination: wildcard.then_some(true),
            },
            device_id: None,
        }))
        .await?;
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Platform {
    Linux,
    Macos,
}

#[derive(Clone, Debug)]
pub struct CreateArgs {
    pub name: String,
    pub platform: Platform,
    pub cpus: Option<u64>,
    pub memory_gib: Option<u64>,
    pub disk_gib: Option<u64>,
}

pub async fn create(
    engine: &Engine,
    args: &CreateArgs,
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    let params = match args.platform {
        Platform::Linux => {
            let cpus = args.cpus.unwrap_or(2);
            let memory = args.memory_gib.unwrap_or(4);
            let narrow = |value: u64, what: &str| {
                u32::try_from(value).map_err(|_| CliError::new(format!("{what} is too large.")))
            };
            ComputerCreateParams::Linux {
                configuration: ComputerConfiguration {
                    id: uuid::Uuid::new_v4().to_string(),
                    name: args.name.clone(),
                    cpus: u8::try_from(cpus).map_err(|_| CliError::new("--cpus is too large."))?,
                    max_cpus: u8::try_from(cpus)
                        .map_err(|_| CliError::new("--cpus is too large."))?,
                    memory_gib: narrow(memory, "--memory")?,
                    max_memory_gib: narrow(memory, "--memory")?,
                    workspace_storage_gib: narrow(args.disk_gib.unwrap_or(10), "--disk")?,
                    runtime_storage_gib: 10,
                    desktop: None,
                },
                request_id: None,
                device_id: None,
            }
        }
        Platform::Macos => ComputerCreateParams::Macos {
            request: MacosComputerCreateRequest {
                name: args.name.clone(),
                cpus: args.cpus.unwrap_or(4),
                memory_gib: args.memory_gib.unwrap_or(8),
                disk_gib: args.disk_gib.unwrap_or(64),
            },
            device_id: None,
        },
    };
    engine
        .run_job(
            &format!("Creating {}", args.name),
            requests::computers_create(params),
            progress,
        )
        .await?;
    Ok(engine.computer(&args.name).await?.to_json())
}

pub async fn start(engine: &Engine, name: &str, progress: Progress<'_>) -> Result<Value, CliError> {
    let computer = engine.computer(name).await?;
    engine
        .run_job(
            &format!("Starting {name}"),
            requests::computers_start(target(&computer)),
            progress,
        )
        .await?;
    Ok(engine.computer(name).await?.to_json())
}

pub async fn stop(engine: &Engine, name: &str, progress: Progress<'_>) -> Result<Value, CliError> {
    let computer = engine.computer(name).await?;
    engine
        .run_job(
            &format!("Stopping {name}"),
            requests::computers_stop(target(&computer)),
            progress,
        )
        .await?;
    Ok(engine.computer(name).await?.to_json())
}

pub async fn retry_setup(
    engine: &Engine,
    name: &str,
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    let computer = engine.computer(name).await?;
    engine
        .run_job(
            &format!("Setting up {name}"),
            requests::macos_setup_retry(target(&computer)),
            progress,
        )
        .await?;
    Ok(engine.computer(name).await?.to_json())
}

fn target_of(computer: &Computer) -> ComputerTargetParams {
    target(computer)
}

fn target(computer: &Computer) -> ComputerTargetParams {
    ComputerTargetParams {
        computer_id: computer.id.clone(),
        device_id: None,
    }
}

/// What a fork carries beyond its source and name: the secrets it may use and what to quiet.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ForkExtras {
    pub secrets: Vec<String>,
    pub quiet: Option<QuietSpec>,
}

/// The recipe's `quiet:` processes to stop and `scrub:` paths to remove in the fork.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QuietSpec {
    pub stop: Vec<String>,
    pub scrub: Vec<String>,
}

/// The one place the recipe and `--secret` become fork parameters. `quiet` is present only
/// when the recipe names something to stop or scrub.
pub fn fork_extras(recipe: Option<&Recipe>, secrets: &[String]) -> ForkExtras {
    let quiet = recipe
        .filter(|recipe| !recipe.quiet.is_empty() || !recipe.scrub.is_empty())
        .map(|recipe| QuietSpec {
            stop: recipe.quiet.clone(),
            scrub: recipe.scrub.clone(),
        });
    ForkExtras {
        secrets: secrets.to_vec(),
        quiet,
    }
}

/// Puts the extras onto the engine's fork parameters. The parameters carry no secrets or
/// quiet fields yet, so anything that would be dropped is refused instead.
pub fn apply_fork_extras(
    params: &mut CheckpointForkParams,
    extras: &ForkExtras,
) -> Result<(), CliError> {
    params.secrets = extras.secrets.clone();
    params.quiet = extras
        .quiet
        .as_ref()
        .map(|quiet| silo_api::methods::checkpoints::QuietSpec {
            stop: quiet.stop.clone(),
            scrub: quiet.scrub.clone(),
        });
    Ok(())
}

/// `<source>-<short id>`, the name a fork gets when none is given.
pub fn default_fork_name(source: &str, short_id: &str) -> String {
    format!("{source}-{short_id}")
}

fn short_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..6].to_owned()
}

#[derive(Clone, Debug, Default)]
pub struct ForkArgs {
    pub source: String,
    pub name: Option<String>,
    pub checkpoint: Option<String>,
    pub secrets: Vec<String>,
    pub recipe: Option<Recipe>,
    /// Fork from this recipe's cached build instead of `source`.
    pub recipe_name: Option<String>,
}

pub async fn fork(
    engine: &Engine,
    args: &ForkArgs,
    settings: Option<&Settings>,
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    Settings::check_fork_secrets(settings, &args.secrets)?;
    let mut recipe_hash = None;
    let mut checkpoint = args.checkpoint.clone();
    let source = match &args.recipe_name {
        Some(recipe_name) => {
            Settings::check_recipe(settings, recipe_name)?;
            let recipe = args.recipe.as_ref().ok_or_else(|| {
                CliError::new(format!("The recipe {recipe_name} was not loaded."))
            })?;
            let built = build_recipe(engine, recipe_name, recipe, false, progress).await?;
            let entry = &built["recipe"];
            let text = |key: &str| entry[key].as_str().unwrap_or_default().to_owned();
            recipe_hash = Some(text("hash"));
            checkpoint = Some(text("checkpointId"));
            let all = engine.all_computers().await?;
            let mut builder = find(&all, &text("computerId"))?.clone();
            builder.name = recipe_name.clone();
            builder
        }
        None => engine.computer(&args.source).await?,
    };
    let name = match (&args.name, &args.recipe_name) {
        (Some(name), _) => name.clone(),
        (None, _) => default_fork_name(&source.name, &short_id()),
    };
    if checkpoint.is_none() {
        checkpoint = warm_checkpoint(engine, &source).await;
    }
    let mut params = CheckpointForkParams {
        computer_id: source.id.clone(),
        device_id: None,
        checkpoint_id: checkpoint,
        new_name: name.clone(),
        secrets: Vec::new(),
        quiet: None,
        recipe_hash,
    };
    apply_fork_extras(
        &mut params,
        &fork_extras(args.recipe.as_ref(), &args.secrets),
    )?;
    engine
        .run_job(
            &format!("Forking {} into {name}", source.name),
            requests::checkpoints_fork(params),
            progress,
        )
        .await?;
    let forked = engine.computer(&name).await?;
    let mut fork = forked.to_json();
    fork["source"] = json!(source.name);
    Ok(fork)
}

pub struct DiffArgs {
    pub computer: String,
    pub max_lines: Option<u32>,
    /// Do not subtract the cached no-op baseline.
    pub no_baseline: bool,
    /// Compute the baseline first (the computer must be an idle fork).
    pub compute_baseline: bool,
}

/// Name prefix of the throwaway computers that measure baselines. `silo list` leaves them out.
pub const BASELINE_PREFIX: &str = "silo-baseline-";
/// Name prefix of the hidden computers that recipes are built in.
pub const BUILDER_PREFIX: &str = "silo-build-";
/// The longest a throwaway fork is left idle, however long the real fork has run.
pub const BASELINE_CAP_SECONDS: u64 = 600;

async fn run_diff(
    engine: &Engine,
    target: &Computer,
    args: &DiffArgs,
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    engine
        .run_job(
            &format!("Comparing {} with when it was forked", target.name),
            requests::changes_diff(ChangesDiffParams {
                computer_id: target.id.clone(),
                device_id: None,
                max_lines: args.max_lines,
                baseline: Some(!args.no_baseline),
                refresh: None,
            }),
            progress,
        )
        .await
}

/// What changed in a computer since it was forked: the diff document, plus the readable
/// `text` and what was compared. With `compute_baseline`, a missing no-op baseline is
/// measured first on a throwaway fork of the same checkpoint; one already cached is used.
pub async fn diff(
    engine: &Engine,
    args: &DiffArgs,
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    let target = engine.computer(&args.computer).await?;
    let mut result = run_diff(engine, &target, args, progress).await?;
    let mut baseline_error = None;
    let mut baseline_seconds = None;
    if args.compute_baseline && result["baselineAvailable"] != true {
        let measured = match result["baseSource"].as_str() {
            Some(source) => {
                // The idle time matches the time the real fork has been running.
                let seconds = result["elapsedSeconds"]
                    .as_u64()
                    .unwrap_or(0)
                    .clamp(1, BASELINE_CAP_SECONDS);
                measure_baseline(engine, &target, source, seconds, progress).await
            }
            None => Err(CliError::new(
                "No baseline: this computer has no record of the checkpoint it was forked from.",
            )),
        };
        match measured {
            Ok(seconds) => {
                baseline_seconds = Some(seconds);
                result = run_diff(engine, &target, args, progress).await?;
            }
            Err(error) => baseline_error = Some(error.message),
        }
    }
    let mut document = result["diff"].take();
    if let Some(object) = document.as_object_mut() {
        for key in [
            "text",
            "baselineApplied",
            "baselineAvailable",
            "baseCapturedAt",
            "capturedAt",
            "reviewDigest",
        ] {
            object.insert(key.to_owned(), result[key].clone());
        }
        if let Some(seconds) = baseline_seconds {
            object.insert("baselineIdleSeconds".to_owned(), json!(seconds));
        }
        if let Some(digest) = result["reviewDigest"].as_str().filter(|d| !d.is_empty()) {
            engine.remember_review(&target.id, digest);
        }
        if let Some(error) = baseline_error {
            object.insert("baselineError".to_owned(), json!(error));
        }
    }
    Ok(document)
}

/// Forks `source` (a checkpoint) into a throwaway computer, starts it so that its base is
/// captured like a real fork's, leaves it idle for `seconds` since that capture, captures it
/// again and caches the idle diff under the checkpoint. The throwaway is dropped whichever way
/// this ends. Returns the idle seconds used.
async fn measure_baseline(
    engine: &Engine,
    target: &Computer,
    source: &str,
    seconds: u64,
    progress: Progress<'_>,
) -> Result<u64, CliError> {
    let all = engine.all_computers().await?;
    // A Mac runs two macOS virtual machines at most; the throwaway would be the third.
    if target.kind == "macos" {
        let running = all
            .iter()
            .filter(|c| c.kind == "macos" && c.state == "running")
            .count();
        if running >= 2 {
            return Err(CliError::new(
                "No baseline: two macOS virtual machines are already running, and measuring the baseline needs a third. Stop one and try again.",
            ));
        }
    }
    let owner = all
        .iter()
        .find(|c| c.checkpoints.iter().any(|cp| cp["id"] == source))
        .ok_or_else(|| {
            CliError::new(
                "No baseline: the checkpoint this computer was forked from no longer exists.",
            )
        })?;
    // Throwaways left by an interrupted run are dropped first.
    for stale in all.iter().filter(|c| c.name.starts_with(BASELINE_PREFIX)) {
        let _ = remove_computer(engine, stale, progress).await;
    }
    let name = format!("{BASELINE_PREFIX}{}", short_id());
    let outcome =
        measure_on_throwaway(engine, target, owner, source, &name, seconds, progress).await;
    let cleanup = async {
        let all = engine.all_computers().await?;
        match all.iter().find(|c| c.name == name) {
            Some(throwaway) => remove_computer(engine, throwaway, progress).await,
            None => Ok(()),
        }
    }
    .await;
    match (outcome, cleanup) {
        (Ok(used), Ok(())) => Ok(used),
        (Err(error), Ok(())) => Err(error),
        (_, Err(error)) => Err(CliError::new(format!(
            "The throwaway computer {name} could not be dropped: {}",
            error.message
        ))),
    }
}

async fn measure_on_throwaway(
    engine: &Engine,
    target: &Computer,
    owner: &Computer,
    source: &str,
    name: &str,
    seconds: u64,
    progress: Progress<'_>,
) -> Result<u64, CliError> {
    // Quiet like the real fork.
    let list = |key: &str| -> Vec<String> {
        target
            .quiet
            .as_ref()
            .and_then(|quiet| quiet.get(key))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect()
    };
    let (stop, scrub) = (list("stop"), list("scrub"));
    let extras = ForkExtras {
        secrets: Vec::new(),
        quiet: (!stop.is_empty() || !scrub.is_empty()).then_some(QuietSpec { stop, scrub }),
    };
    let mut params = CheckpointForkParams {
        computer_id: owner.id.clone(),
        device_id: None,
        checkpoint_id: Some(source.to_owned()),
        new_name: name.to_owned(),
        secrets: Vec::new(),
        quiet: None,
        recipe_hash: None,
    };
    apply_fork_extras(&mut params, &extras)?;
    engine
        .run_job(
            "Forking the idle copy that measures the baseline",
            requests::checkpoints_fork(params),
            progress,
        )
        .await?;
    let listed = engine.all_computers().await?;
    let throwaway = find(&listed, name)?.clone();
    // The first start captures the fork-time base, as for any fork.
    engine
        .run_job(
            "Starting the idle copy",
            requests::computers_start(target_of(&throwaway)),
            progress,
        )
        .await?;
    let measured = engine
        .run_job(
            &format!("Leaving the idle copy alone for {seconds}s"),
            requests::changes_baseline(ChangesBaselineParams {
                computer_id: throwaway.id.clone(),
                device_id: None,
                source: Some(source.to_owned()),
                idle_seconds: Some(seconds),
            }),
            progress,
        )
        .await?;
    Ok(measured["idleSeconds"].as_u64().unwrap_or(seconds))
}

/// How long a computer that was asked to stop is waited for before it is dropped anyway.
const STOP_WAIT: Duration = Duration::from_secs(120);
const STOP_POLL: Duration = Duration::from_secs(1);
/// Drops that find the computer still stopping are tried again this many times.
const DROP_RETRIES: u32 = 10;

/// Stops a computer that is not stopped, waits until it reports stopped, then deletes it. A
/// delete that finds it still stopping is retried; only a failure that remains is returned.
pub async fn remove_computer(
    engine: &Engine,
    computer: &Computer,
    progress: Progress<'_>,
) -> Result<(), CliError> {
    if computer.state != "stopped" {
        let stopped = engine
            .run_job(
                &format!("Stopping {}", computer.name),
                requests::computers_stop(target_of(computer)),
                progress,
            )
            .await;
        // A refused stop shows in the delete below; a slow one is waited for.
        if stopped.is_ok() {
            let started = std::time::Instant::now();
            while started.elapsed() < STOP_WAIT {
                let listed = engine.all_computers().await?;
                match listed.iter().find(|c| c.id == computer.id) {
                    Some(now) if now.state != "stopped" => tokio::time::sleep(STOP_POLL).await,
                    _ => break,
                }
            }
        }
    }
    let expected = match &computer.configuration {
        Some(configuration) => Some(serde_json::from_value(configuration.clone())?),
        None => None,
    };
    let mut attempt = 0;
    loop {
        let dropped = engine
            .run_job(
                &format!("Dropping {}", computer.name),
                requests::computers_delete(ComputerDeleteParams {
                    computer_id: computer.id.clone(),
                    device_id: None,
                    expected: expected.clone(),
                    request_id: None,
                }),
                progress,
            )
            .await;
        match dropped {
            Err(error)
                if error.message.contains("Stop the computer first") && attempt < DROP_RETRIES =>
            {
                attempt += 1;
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            other => return other.map(|_| ()),
        }
    }
}

pub fn check_drop(name: &str, force: bool) -> Result<(), CliError> {
    if name == MAIN && !force {
        return Err(CliError::new(
            "main is the long-lived computer. Pass --force to drop it.",
        ));
    }
    Ok(())
}

pub async fn drop_computer(
    engine: &Engine,
    name: &str,
    force: bool,
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    check_drop(name, force)?;
    let computer = engine.computer(name).await?;
    check_drop(&computer.name, force)?;
    let expected = match &computer.configuration {
        Some(configuration) => Some(serde_json::from_value(configuration.clone())?),
        None => None,
    };
    engine
        .run_job(
            &format!("Dropping {}", computer.name),
            requests::computers_delete(ComputerDeleteParams {
                computer_id: computer.id.clone(),
                device_id: None,
                expected,
                request_id: None,
            }),
            progress,
        )
        .await?;
    Ok(json!({ "dropped": computer.name }))
}

/// The longest command `guest.exec` accepts; longer work needs a streaming path.
pub const MAX_EXEC_SECS: u64 = 180;

#[derive(Clone, Debug)]
pub struct ExecArgs {
    pub computer: String,
    pub argv: Vec<String>,
    pub cwd: Option<String>,
    pub timeout_secs: u64,
}

/// A finished command with its output as the bytes the computer wrote.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecOutput {
    pub exit_code: Option<i32>,
    pub signal: Option<String>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl ExecOutput {
    /// The process exit code: the command's own, or 128 plus the signal that ended it.
    pub fn process_exit_code(&self) -> i32 {
        exit_code_for(self.exit_code, self.signal.as_deref())
    }

    /// JSON with the output as text; bytes that are not UTF-8 are replaced.
    pub fn to_json(&self) -> Value {
        json!({
            "exitCode": self.exit_code,
            "signal": self.signal,
            "stdout": String::from_utf8_lossy(&self.stdout),
            "stderr": String::from_utf8_lossy(&self.stderr),
        })
    }
}

/// An exit code for a command that ended normally (`code`) or by a signal (`KILL`, `SIGTERM`
/// or a number): 128 plus the signal number.
pub fn exit_code_for(code: Option<i32>, signal: Option<&str>) -> i32 {
    if let Some(code) = code {
        return code;
    }
    let Some(signal) = signal else {
        return 1;
    };
    let name = signal.trim().to_ascii_uppercase();
    let name = name.strip_prefix("SIG").unwrap_or(&name);
    let number = match name {
        "HUP" => 1,
        "INT" => 2,
        "QUIT" => 3,
        "ILL" => 4,
        "TRAP" => 5,
        "ABRT" => 6,
        "BUS" => 7,
        "FPE" => 8,
        "KILL" => 9,
        "USR1" => 10,
        "SEGV" => 11,
        "USR2" => 12,
        "PIPE" => 13,
        "ALRM" => 14,
        "TERM" => 15,
        other => match other.parse::<i32>() {
            Ok(number) if (1..=64).contains(&number) => number,
            _ => return 1,
        },
    };
    128 + number
}

pub async fn exec_raw(engine: &Engine, args: &ExecArgs) -> Result<ExecOutput, CliError> {
    if args.argv.is_empty() {
        return Err(CliError::new("Give a command to run after --."));
    }
    if args.timeout_secs == 0 || args.timeout_secs > MAX_EXEC_SECS {
        return Err(CliError::new(format!(
            "The timeout must be 1 to {MAX_EXEC_SECS} seconds; the engine does not run longer commands yet."
        )));
    }
    let computer = engine.computer(&args.computer).await?;
    let reply = engine
        .call(requests::guest_exec(GuestExecParams {
            computer_id: computer.id,
            device_id: None,
            argv: args.argv.clone(),
            cwd: args.cwd.clone(),
            env: None,
            as_user: None,
            stdin: None,
            timeout_ms: args.timeout_secs * 1000,
            max_output_bytes: None,
        }))
        .await?;
    let bytes = |key: &str| -> Vec<u8> {
        reply
            .get(key)
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_u64().map(|byte| byte as u8))
                    .collect()
            })
            .unwrap_or_default()
    };
    Ok(ExecOutput {
        exit_code: reply
            .get("exitCode")
            .and_then(Value::as_i64)
            .map(|code| code as i32),
        signal: reply
            .get("signal")
            .and_then(Value::as_str)
            .map(str::to_owned),
        stdout: bytes("stdout"),
        stderr: bytes("stderr"),
    })
}

pub async fn exec(engine: &Engine, args: &ExecArgs) -> Result<Value, CliError> {
    Ok(exec_raw(engine, args).await?.to_json())
}

/// Waits for a job that is already running, by id.
pub async fn job(engine: &Engine, id: &str, progress: Progress<'_>) -> Result<Value, CliError> {
    silo_api::JobId::new(id).map_err(|_| CliError::new(format!("{id} is not a job id.")))?;
    engine
        .wait_job(id, &format!("Waiting for {id}"), progress)
        .await
}

/// The recipe a fork uses: the explicit file, else the project's `.silo/recipe.yaml`.
pub fn resolve_recipe(
    explicit: Option<&std::path::Path>,
    project: &crate::config::Project,
) -> Result<Option<Recipe>, CliError> {
    match explicit {
        Some(path) => crate::config::load_recipe(path).map(Some),
        None => project.load_recipe(),
    }
}

pub fn parse_checkpoint_kind(text: &str) -> Result<CheckpointKind, CliError> {
    match text {
        "warm" => Ok(CheckpointKind::Warm),
        "manual" => Ok(CheckpointKind::Manual),
        "automatic" => Ok(CheckpointKind::Automatic),
        "before-keep" => Ok(CheckpointKind::BeforeKeep),
        other => Err(CliError::new(format!(
            "Unknown checkpoint kind {other}; use manual, automatic, before-keep or warm."
        ))),
    }
}

pub async fn checkpoint(
    engine: &Engine,
    computer: &str,
    name: Option<&str>,
    kind: Option<CheckpointKind>,
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    let target = engine.computer(computer).await?;
    let name = name
        .map(str::to_owned)
        .unwrap_or_else(|| format!("checkpoint-{}", short_id()));
    let result = engine
        .run_job(
            &format!("Checkpointing {}", target.name),
            requests::checkpoints_create(CheckpointCreateParams {
                computer_id: target.id,
                device_id: None,
                name,
                kind,
            }),
            progress,
        )
        .await?;
    Ok(result.get("checkpoint").cloned().unwrap_or(result))
}

/// The recipe's `keep:` section as the engine takes it.
pub fn keep_hooks(recipe: Option<&Recipe>) -> Option<KeepHooks> {
    recipe.map(|recipe| KeepHooks {
        restart: recipe.keep.restart.clone(),
        migrate: recipe.keep.migrate.clone(),
        after: recipe.keep.after.clone(),
    })
}

/// What a keep is asked beyond the fork and target.
#[derive(Clone, Debug, Default)]
pub struct FoldOptions {
    pub dry_run: bool,
    /// Carry database changes even though no migrate step covers them.
    pub data_ok: bool,
    /// The review digest from `silo diff --json`.
    pub reviewed: Option<String>,
}

/// Keeps a fork's changes into `into` (default: the computer the fork was made from). A
/// refused keep is a result (`status: "refused"`), not an error: nothing was changed.
pub async fn fold(
    engine: &Engine,
    fork: &str,
    into: Option<&str>,
    keep: Option<KeepHooks>,
    options: FoldOptions,
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    let FoldOptions {
        dry_run,
        data_ok,
        reviewed,
    } = options;
    let fork = engine.computer(fork).await?;
    // Without a digest, the one the last `silo diff` of this fork showed pins the keep.
    // A dry run reports and never pins: it ignores the digest a review left behind.
    let reviewed = if dry_run {
        None
    } else {
        reviewed.or_else(|| engine.recall_review(&fork.id))
    };
    let target_id = match into {
        Some(name) => Some(engine.computer(name).await?.id),
        None => None,
    };
    let label = if dry_run { "Checking" } else { "Keeping" };
    engine
        .run_job(
            &format!("{label} {}", fork.name),
            requests::changes_keep(ChangesKeepParams {
                fork_id: fork.id,
                target_id,
                keep,
                dry_run: dry_run.then_some(true),
                data_ok: data_ok.then_some(true),
                review_digest: reviewed,
            }),
            progress,
        )
        .await
}

/// Makes a fork main: it replaces `into` (default: the computer it was made from). A refused
/// promote is a result (`status: "refused"`); nothing was changed.
pub async fn promote(
    engine: &Engine,
    fork: &str,
    into: Option<&str>,
    dry_run: bool,
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    let fork = engine.computer(fork).await?;
    let target_id = match into {
        Some(name) => Some(engine.computer(name).await?.id),
        None => None,
    };
    let label = if dry_run { "Checking" } else { "Promoting" };
    engine
        .run_job(
            &format!("{label} {}", fork.name),
            requests::changes_promote(ChangesPromoteParams {
                fork_id: fork.id,
                target_id,
                dry_run: dry_run.then_some(true),
            }),
            progress,
        )
        .await
}

/// Puts a computer back to before its latest keep.
pub async fn undo(
    engine: &Engine,
    computer: &str,
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    let target = engine.computer(computer).await?;
    engine
        .run_job(
            &format!("Undoing the latest keep or promote of {}", target.name),
            requests::changes_undo(ChangesUndoParams {
                computer_id: target.id,
            }),
            progress,
        )
        .await
}

pub async fn checkpoints(engine: &Engine, computer: &str) -> Result<Value, CliError> {
    let target = engine.computer(computer).await?;
    Ok(json!({ "computer": target.name, "checkpoints": target.checkpoints }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse_recipe;

    #[test]
    fn default_fork_names_come_from_the_source() {
        assert_eq!(default_fork_name("main", "a1b2c3"), "main-a1b2c3");
        assert_eq!(short_id().len(), 6);
    }

    #[test]
    fn fork_extras_map_the_recipe_and_secrets() {
        let recipe = parse_recipe("quiet: [poller]\nscrub: [/root/.env]\n").unwrap();
        let extras = fork_extras(Some(&recipe), &["github".to_owned()]);
        assert_eq!(extras.secrets, ["github"]);
        assert_eq!(
            extras.quiet,
            Some(QuietSpec {
                stop: vec!["poller".into()],
                scrub: vec!["/root/.env".into()]
            })
        );
        assert_eq!(fork_extras(None, &[]), ForkExtras::default());
        let empty = parse_recipe("keep: { restart: [web] }\n").unwrap();
        assert_eq!(fork_extras(Some(&empty), &[]).quiet, None);
    }

    #[test]
    fn signals_map_to_shell_exit_codes() {
        assert_eq!(exit_code_for(Some(3), Some("KILL")), 3);
        assert_eq!(exit_code_for(None, Some("KILL")), 137);
        assert_eq!(exit_code_for(None, Some("SIGINT")), 130);
        assert_eq!(exit_code_for(None, Some("term")), 143);
        assert_eq!(exit_code_for(None, Some("12")), 140);
        assert_eq!(exit_code_for(None, Some("PIPE")), 141);
        assert_eq!(exit_code_for(None, Some("???")), 1);
        assert_eq!(exit_code_for(None, None), 1);
    }

    #[test]
    fn fork_extras_reach_the_fork_parameters() {
        let mut params = CheckpointForkParams {
            computer_id: "c".into(),
            device_id: None,
            checkpoint_id: None,
            new_name: "n".into(),
            secrets: Vec::new(),
            quiet: None,
            recipe_hash: None,
        };
        apply_fork_extras(&mut params, &ForkExtras::default()).unwrap();
        assert!(params.secrets.is_empty());
        assert!(params.quiet.is_none());
        let recipe =
            crate::config::parse_recipe("quiet: [poller]\nscrub: [/root/.bot.env]\n").unwrap();
        apply_fork_extras(
            &mut params,
            &fork_extras(Some(&recipe), &["github".to_owned()]),
        )
        .unwrap();
        assert_eq!(params.secrets, ["github"]);
        let quiet = params.quiet.unwrap();
        assert_eq!(quiet.stop, ["poller"]);
        assert_eq!(quiet.scrub, ["/root/.bot.env"]);
    }

    #[test]
    fn the_recipe_comes_from_the_file_or_the_project() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join(".silo")).unwrap();
        std::fs::write(root.path().join(".silo/recipe.yaml"), "quiet: [p]\n").unwrap();
        let project = crate::config::Project::discover(root.path());
        assert_eq!(
            resolve_recipe(None, &project).unwrap().unwrap().quiet,
            ["p"]
        );
        let none = crate::config::Project::default();
        assert_eq!(resolve_recipe(None, &none).unwrap(), None);
        std::fs::write(root.path().join(".silo/recipe.yaml"), "bogus: 1\n").unwrap();
        let project = crate::config::Project::discover(root.path());
        assert!(resolve_recipe(None, &project).is_err());
    }

    #[test]
    fn dropping_main_needs_force() {
        assert!(check_drop("main", false)
            .unwrap_err()
            .message
            .contains("--force"));
        assert!(check_drop("main", true).is_ok());
        assert!(check_drop("main-a1b2c3", false).is_ok());
    }

    #[test]
    fn checkpoint_kinds_parse() {
        assert_eq!(
            parse_checkpoint_kind("before-keep").unwrap(),
            CheckpointKind::BeforeKeep
        );
        assert!(parse_checkpoint_kind("later").is_err());
    }
}

// MARK: Recipes

/// Builds a recipe, or returns its cached build. The result carries `recipe` (the cache entry)
/// and `built` (false for a cache hit).
pub async fn build_recipe(
    engine: &Engine,
    name: &str,
    recipe: &Recipe,
    force: bool,
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    engine
        .run_job(
            &format!("Building the recipe {name}"),
            requests::recipes_build(RecipeBuildParams {
                name: name.to_owned(),
                spec: recipe.spec(),
                force: force.then_some(true),
            }),
            progress,
        )
        .await
}

/// Every recipe the project and the CLI offer, with whether the engine holds a build of it.
pub async fn recipe_list(
    engine: &Engine,
    project: &crate::config::Project,
) -> Result<Value, CliError> {
    let built = engine
        .call(requests::recipes_list(silo_api::methods::EmptyParams {}))
        .await?;
    let built = built["recipes"].as_array().cloned().unwrap_or_default();
    let recipes: Vec<Value> = project
        .recipe_names()
        .into_iter()
        .map(|(name, origin)| {
            let entry = built.iter().find(|entry| entry["name"] == name.as_str());
            json!({
                "name": name,
                "origin": origin,
                "built": entry.is_some(),
                "builtAt": entry.map(|entry| entry["builtAt"].clone()),
                "imageVersion": entry.map(|entry| entry["imageVersion"].clone()),
            })
        })
        .collect();
    Ok(json!({ "recipes": recipes }))
}

pub async fn recipe_remove(engine: &Engine, name: &str) -> Result<Value, CliError> {
    engine
        .call(requests::recipes_remove(RecipeRemoveParams {
            name: name.to_owned(),
        }))
        .await?;
    Ok(json!({ "removed": name }))
}

// MARK: Up

/// Marker file whose time says when `main` was last checkpointed warm.
const WARM_MARKER: &str = "/var/lib/silo-warm-marker";
const WARM_NAME: &str = "silo-warm";

fn exec_args(computer: &str, script: &str) -> ExecArgs {
    ExecArgs {
        computer: computer.to_owned(),
        argv: vec!["sh".into(), "-c".into(), script.to_owned()],
        cwd: None,
        timeout_secs: 120,
    }
}

/// Prints `unchanged` when nothing outside the noisy system folders was written since the
/// warm marker.
fn unchanged_script(macos: bool) -> String {
    if macos {
        "m=/tmp/.silo-warm-marker; [ -e $m ] || exit 0; \
         [ -z \"$(find \"$HOME\" /usr/local /opt -xdev \\( -path \"$HOME/Library/Caches\" -o -path \"$HOME/Library/Logs\" \\) -prune -o -newer $m -print -quit 2>/dev/null)\" ] && echo unchanged"
            .to_owned()
    } else {
        format!(
            "m={WARM_MARKER}; [ -e $m ] || exit 0; \
             [ -z \"$(find / -xdev \\( -path /proc -o -path /sys -o -path /dev -o -path /run -o -path /tmp -o -path /var/log -o -path /var/cache \\) -prune -o -newer $m -print -quit 2>/dev/null)\" ] && echo unchanged"
        )
    }
}

/// The warm checkpoint of `source` when `source` is `main`, runs, and nothing was written
/// since the checkpoint was taken. Any doubt means no warm checkpoint.
async fn warm_checkpoint(engine: &Engine, source: &Computer) -> Option<String> {
    if source.name != MAIN || source.state != "running" {
        return None;
    }
    let warm = source
        .checkpoints
        .iter()
        .filter(|cp| cp["kind"] == "warm")
        .max_by_key(|cp| cp["createdAt"].as_str().unwrap_or_default().to_owned())?;
    let out = exec_raw(
        engine,
        &exec_args(&source.name, &unchanged_script(source.kind == "macos")),
    )
    .await
    .ok()?;
    (out.exit_code == Some(0) && String::from_utf8_lossy(&out.stdout).trim() == "unchanged")
        .then(|| warm["id"].as_str().map(str::to_owned))
        .flatten()
}

/// Makes sure the engine answers, `main` exists (made from the recipe) and runs, and keeps a
/// warm checkpoint of it that replaces the previous one.
pub async fn up(
    engine: &Engine,
    recipe_name: &str,
    recipe: &Recipe,
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    let mut created = false;
    let mut main = match engine.computer(MAIN).await {
        Ok(main) => main,
        Err(_) => {
            let built = build_recipe(engine, recipe_name, recipe, false, progress).await?;
            let entry = &built["recipe"];
            let text = |key: &str| entry[key].as_str().unwrap_or_default().to_owned();
            let all = engine.all_computers().await?;
            let builder = find(&all, &text("computerId"))?.clone();
            let params = CheckpointForkParams {
                computer_id: builder.id,
                device_id: None,
                checkpoint_id: Some(text("checkpointId")),
                new_name: MAIN.to_owned(),
                secrets: Vec::new(),
                quiet: None,
                recipe_hash: None,
            };
            engine
                .run_job(
                    &format!("Making main from {recipe_name}"),
                    requests::checkpoints_fork(params),
                    progress,
                )
                .await?;
            created = true;
            engine.computer(MAIN).await?
        }
    };
    if main.state != "running" {
        start(engine, MAIN, progress).await?;
        main = engine.computer(MAIN).await?;
    }
    let macos = main.kind == "macos";
    let marker = if macos {
        "/tmp/.silo-warm-marker"
    } else {
        WARM_MARKER
    };
    exec_raw(engine, &exec_args(MAIN, &format!("touch {marker}"))).await?;
    let previous: Vec<String> = main
        .checkpoints
        .iter()
        .filter(|cp| cp["kind"] == "warm")
        .filter_map(|cp| cp["id"].as_str().map(str::to_owned))
        .collect();
    let warm = checkpoint(
        engine,
        MAIN,
        Some(WARM_NAME),
        Some(CheckpointKind::Warm),
        progress,
    )
    .await?;
    for id in previous {
        // A checkpoint a fork still depends on stays until a later `up`.
        let _ = engine
            .run_job(
                "Removing the previous warm checkpoint",
                requests::checkpoints_delete(
                    silo_api::methods::checkpoints::CheckpointTargetParams {
                        computer_id: main.id.clone(),
                        device_id: None,
                        checkpoint_id: id,
                    },
                ),
                progress,
            )
            .await;
    }
    Ok(json!({
        "main": engine.computer(MAIN).await?.to_json(),
        "created": created,
        "recipe": recipe_name,
        "warmCheckpoint": warm["id"],
    }))
}
