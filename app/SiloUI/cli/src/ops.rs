//! The operations behind both the commands and the MCP tools. Each returns the JSON the
//! `--json` flag prints; `render` turns the same values into short text.

use std::time::Duration;

use serde_json::{json, Value};
use silo_api::methods::{
    changes::{ChangesBaselineParams, ChangesDiffParams},
    checkpoints::{CheckpointCreateParams, CheckpointForkParams, CheckpointKind},
    common::{ComputerTargetParams, DeviceParams},
    computers::{
        ComputerConfiguration, ComputerConfigurationChange, ComputerConfigurationChangeParams,
        ComputerCreateParams, ComputerDeleteParams, ComputerRebuildParams, ComputerRuntimeKind,
        DesktopConfiguration, MacosComputerCreateRequest,
    },
    desktop::{DesktopAction, DesktopActionParams},
    guest::GuestExecParams,
    keep::{ChangesKeepParams, ChangesPromoteParams, ChangesUndoParams, KeepAnswer},
    recipes::{
        RecipeBuildParams, RecipeDefinition, RecipeGetParams, RecipeRemoveParams, RecipeSaveParams,
    },
    requests,
    secrets::{SecretKind, SecretSaveAction, SecretSaveRequest, SecretsSaveParams},
};

use crate::{
    engine::{find, Computer, Engine, Progress},
    error::CliError,
};

/// The computer that holds the long-lived work; dropping it needs `--force`.
pub const MAIN: &str = "main";

pub async fn list(engine: &Engine) -> Result<Value, CliError> {
    let computers = engine.computers().await?;
    Ok(json!({ "computers": computers.iter().map(Computer::to_json).collect::<Vec<_>>() }))
}

/// `silo list --all`: the throwaway baseline and recipe-builder computers too.
pub async fn list_all(engine: &Engine) -> Result<Value, CliError> {
    let computers = engine.all_computers().await?;
    Ok(json!({ "computers": computers.iter().map(Computer::to_json).collect::<Vec<_>>() }))
}

/// How long one device may take to list its computers.
pub const DEVICE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// `silo list --all-devices`: this device's computers and every connected device's, each with
/// the device it is on. Devices are asked at once, each for at most `DEVICE_TIMEOUT`; one that
/// is known to be offline, fails or is too slow is listed in `unreachable`.
pub async fn list_everywhere(engine: &Engine, hidden: bool) -> Result<Value, CliError> {
    list_everywhere_within(engine, hidden, DEVICE_TIMEOUT).await
}

pub async fn list_everywhere_within(
    engine: &Engine,
    hidden: bool,
    timeout: std::time::Duration,
) -> Result<Value, CliError> {
    engine.require_routing()?;
    let read = |engine: Engine| async move {
        if hidden {
            engine.all_computers().await
        } else {
            engine.computers().await
        }
    };
    let mut rows = Vec::new();
    for computer in read(engine.on_device(None)).await? {
        let mut row = computer.to_json();
        row["deviceName"] = json!("this device");
        rows.push(row);
    }
    let offline = engine.known_offline().await;
    let asked = engine.devices().await?.into_iter().map(|(id, name)| {
        let known = offline.get(&id).cloned();
        let remote = engine.on_device(Some(id.clone()));
        async move {
            let result = match known {
                Some(reason) => Err(reason),
                None => match tokio::time::timeout(timeout, read(remote)).await {
                    Ok(Ok(computers)) => Ok(computers),
                    Ok(Err(error)) => Err(error.message),
                    Err(_) => Err("It did not answer in time.".to_owned()),
                },
            };
            (id, name, result)
        }
    });
    let mut unreachable = Vec::new();
    for (id, name, result) in futures::future::join_all(asked).await {
        match result {
            Ok(computers) => {
                for computer in computers {
                    let mut row = computer.to_json();
                    row["deviceId"] = json!(id);
                    row["deviceName"] = json!(name);
                    rows.push(row);
                }
            }
            Err(reason) => {
                unreachable.push(json!({ "deviceId": id, "deviceName": name, "reason": reason }))
            }
        }
    }
    Ok(json!({ "computers": rows, "unreachable": unreachable }))
}

/// How long a throwaway baseline computer may exist before it counts as left behind.
pub const THROWAWAY_MAX_AGE_SECONDS: i64 = 3600;

/// `silo-baseline-<unix seconds in hex>-<short id>`: the time lets a later run tell a copy
/// that is being measured from one left behind.
pub fn baseline_name(now: i64) -> String {
    format!("{BASELINE_PREFIX}{now:x}-{}", short_id())
}

/// The baseline computers that nothing is using: stopped or failed, and not made within
/// `THROWAWAY_MAX_AGE_SECONDS`. A name with no time in it (made by an earlier version) counts
/// as old.
pub fn stale_throwaways(all: &[Computer], now: i64) -> Vec<&Computer> {
    all.iter()
        .filter(|computer| computer.name.starts_with(BASELINE_PREFIX))
        .filter(|computer| matches!(computer.state.as_str(), "stopped" | "failed"))
        .filter(|computer| {
            let rest = &computer.name[BASELINE_PREFIX.len()..];
            let made = rest
                .split_once('-')
                .and_then(|(stamp, _)| i64::from_str_radix(stamp, 16).ok());
            made.map_or(true, |made| now - made >= THROWAWAY_MAX_AGE_SECONDS)
        })
        .collect()
}

/// The longest a sweep runs; one the engine cannot drop (a macOS throwaway whose Keychain
/// sign-in it may not read) is tried again by a later command.
pub const SWEEP_LIMIT: Duration = Duration::from_secs(20);

/// How long a fork that has finished lets the sweep run before the command ends. The engine
/// finishes a drop it has started either way; a later command sweeps the rest.
pub const SWEEP_GRACE: Duration = Duration::from_secs(5);

/// Drops the stale throwaways, for at most [`SWEEP_LIMIT`]. Best effort; returns the names
/// dropped. Its jobs are never the one Ctrl-C names.
pub async fn sweep_throwaways(engine: &Engine, progress: Progress<'_>) -> Vec<String> {
    sweep_throwaways_within(engine, SWEEP_LIMIT, progress).await
}

async fn sweep_throwaways_within(
    engine: &Engine,
    limit: Duration,
    progress: Progress<'_>,
) -> Vec<String> {
    let engine = &engine.without_job_notes();
    let dropped = std::sync::Mutex::new(Vec::new());
    let sweep = async {
        let Ok(all) = engine.all_computers().await else {
            return;
        };
        for stale in stale_throwaways(&all, crate::clock::now_unix()) {
            if remove_computer(engine, stale, progress).await.is_ok() {
                dropped.lock().unwrap().push(stale.name.clone());
            }
        }
    };
    let _ = tokio::time::timeout(limit, sweep).await;
    dropped.into_inner().unwrap_or_default()
}

/// Runs `fork`, then sweeps silently for at most [`SWEEP_GRACE`]: the fork never waits on a
/// sweep, and no sweep line comes between its progress lines.
pub async fn fork_then_sweep<T>(
    engine: &Engine,
    fork: impl std::future::Future<Output = Result<T, CliError>>,
) -> Result<T, CliError> {
    let forked = fork.await;
    sweep_throwaways_within(engine, SWEEP_GRACE, &|_| {}).await;
    forked
}

/// One computer with its checkpoints, its recorded quiet state, the Silo secrets assigned to
/// it and its GitHub assignment. A reading the engine cannot give is `null`.
pub async fn status(engine: &Engine, name: &str) -> Result<Value, CliError> {
    let computer = engine.computer(name).await?;
    let mut value = computer.to_json();
    value["quiet"] = computer.quiet.clone().unwrap_or(Value::Null);
    value["boots"] = computer.boots.map_or(Value::Null, Value::from);
    // Secrets and GitHub access are read from this device's engine, so a computer on another
    // device shows neither.
    let on_other_device = engine.device().is_some();
    value["secrets"] = match fetch_secrets(engine).await {
        _ if on_other_device => Value::Null,
        Ok(secrets) => json!(secrets
            .iter()
            .filter(|secret| secret["removing"] != true)
            .filter(|secret| assigned_ids(secret).any(|id| id == computer.id))
            .filter_map(|secret| secret["name"].as_str())
            .collect::<Vec<_>>()),
        Err(_) => Value::Null,
    };
    value["github"] = match if on_other_device {
        Err(CliError::new("another device"))
    } else {
        engine
            .call(requests::github_state(DeviceParams { device_id: None }))
            .await
    } {
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
    value["credentialsUsed"] = credentials_used(engine, &computer.id).await;
    value["computerUse"] = computer_use(engine, &computer.id).await;
    value["recipe"] = match computer_recipe(engine, &computer).await {
        Ok(record) if record.is_object() => json!({
            "name": record["name"],
            "hash": record["hash"],
            "builtAt": record["builtAt"],
        }),
        _ => Value::Null,
    };
    value["checkpointList"] = Value::Array(computer.checkpoints);
    Ok(value)
}

/// The computer's computer-use state and approval mode; `null` when the engine cannot say.
pub async fn computer_use(engine: &Engine, computer_id: &str) -> Value {
    engine
        .call(requests::computer_use_state(
            silo_api::methods::computer_use::ComputerUseStateParams {
                computer_id: computer_id.to_owned(),
                device_id: None,
            },
        ))
        .await
        .unwrap_or(Value::Null)
}

/// Which Silo secrets a computer used and which attempts were blocked. `null` when the
/// engine cannot say (an older engine does not know the method).
pub async fn credentials_used(engine: &Engine, computer_id: &str) -> Value {
    engine
        .call(requests::computers_credentials(
            silo_api::methods::credentials_used::ComputerCredentialsParams {
                computer_id: computer_id.to_owned(),
                device_id: None,
                since: None,
            },
        ))
        .await
        .unwrap_or(Value::Null)
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
                kind: SecretKind::Generic,
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
    /// A Linux computer run as a container, on Linux hosts only.
    Container,
}

#[derive(Clone, Debug)]
pub struct CreateArgs {
    pub name: String,
    pub platform: Platform,
    pub cpus: Option<u64>,
    pub memory_gib: Option<u64>,
    pub disk_gib: Option<u64>,
    /// A Linux computer created with its desktop off: the same setting the app's form sends.
    pub no_desktop: bool,
}

/// The desktop setting of a new Linux computer. Without `--no-desktop` the request names none and
/// the engine decides (a desktop the computer starts with, where its image includes one).
fn desktop_of(args: &CreateArgs) -> Option<DesktopConfiguration> {
    args.no_desktop.then_some(DesktopConfiguration {
        start_with_computer: false,
        built_in: false,
    })
}

pub async fn create(
    engine: &Engine,
    args: &CreateArgs,
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    if args.platform == Platform::Container && engine.runs_macos_computers().await? {
        return Err(CliError::new(
            "Container computers run on Linux hosts only. This device is a Mac.",
        ));
    }
    let params = match args.platform {
        Platform::Linux | Platform::Container => {
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
                    desktop: desktop_of(args),
                    runtime: (args.platform == Platform::Container)
                        .then_some(ComputerRuntimeKind::Container),
                },
                request_id: None,
                device_id: None,
                agents: Vec::new(),
                network_policy: None,
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
            agents: Vec::new(),
            network_policy: None,
        },
    };
    engine
        .run_job(
            &format!("Creating {}", args.name),
            requests::computers_create(params),
            progress,
        )
        .await?;
    with_warning(engine, engine.computer(&args.name).await?.to_json()).await
}

/// `value` with the device's `warning` sentence when it is running short of memory or disk.
async fn with_warning(engine: &Engine, mut value: Value) -> Result<Value, CliError> {
    if let Some(warning) = resource_warning(engine).await {
        value["warning"] = json!(warning);
    }
    Ok(value)
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
    with_warning(engine, engine.computer(name).await?.to_json()).await
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

/// `silo desktop on|off`: whether a Linux computer starts a desktop, which its forks inherit.
/// Turning it on starts the desktop now; turning it off stops it. The computer must be running,
/// because the choice is written inside it.
pub async fn desktop_set(
    engine: &Engine,
    name: &str,
    on: bool,
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    let computer = engine.computer(name).await?;
    let Some(current) = &computer.configuration else {
        return Err(CliError::new(format!(
            "{name} is a macOS computer; its display is the computer itself, so there is no desktop to turn on or off."
        )));
    };
    if computer.desktop_on().is_none() {
        return Err(CliError::new(format!(
            "{name} does not have a built-in desktop, so it has no desktop setting for `silo desktop`; change its desktop in the app's computer settings."
        )));
    }
    if computer.state != "running" {
        return Err(CliError::new(format!(
            "{name} is {}; start it with `silo start {name}` first.",
            computer.state
        )));
    }
    let current: ComputerConfiguration =
        serde_json::from_value(current.clone()).map_err(|error| {
            CliError::new(format!("{name}'s configuration is not readable: {error}"))
        })?;
    let was_on = computer.desktop_on() == Some(true);
    if was_on != on {
        let mut next = current.clone();
        next.desktop = Some(DesktopConfiguration {
            start_with_computer: on,
            built_in: current
                .desktop
                .as_ref()
                .is_some_and(|desktop| desktop.built_in),
        });
        engine
            .run_job(
                &format!(
                    "Turning the desktop of {name} {}",
                    if on { "on" } else { "off" }
                ),
                requests::computers_configuration_change(ComputerConfigurationChangeParams {
                    change: ComputerConfigurationChange::Upsert {
                        configuration: next,
                        expected: Some(current),
                    },
                    request_id: None,
                    retry_computer_id: None,
                    device_id: None,
                }),
                progress,
            )
            .await?;
    }
    // Turning the choice on has started the desktop; one that was already on may have been
    // stopped by hand. Turning it off stops the running one.
    let action = match (on, was_on) {
        (true, true) => Some(DesktopAction::Start),
        (false, true) => Some(DesktopAction::Stop),
        _ => None,
    };
    if let Some(action) = action {
        engine
            .run_job(
                &format!(
                    "{} the desktop of {name}",
                    if on { "Starting" } else { "Stopping" }
                ),
                requests::desktop_action(DesktopActionParams {
                    computer_id: computer.id.clone(),
                    device_id: None,
                    action,
                }),
                progress,
            )
            .await?;
    }
    Ok(json!({ "computer": name, "desktop": on }))
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

/// `<source>-<short id>`, the name a fork gets when none is given.
pub fn default_fork_name(source: &str, short_id: &str) -> String {
    format!("{source}-{short_id}")
}

pub(crate) fn short_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..6].to_owned()
}

#[derive(Clone, Debug, Default)]
pub struct ForkArgs {
    pub source: String,
    pub name: Option<String>,
    pub checkpoint: Option<String>,
    pub secrets: Vec<String>,
    /// Fork from this recipe's cached build instead of `source`.
    pub recipe_name: Option<String>,
    /// Leave the new computer stopped.
    pub no_start: bool,
}

pub async fn fork(
    engine: &Engine,
    args: &ForkArgs,
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    fork_then_sweep(engine, fork_now(engine, args, progress)).await
}

async fn fork_now(
    engine: &Engine,
    args: &ForkArgs,
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    let mut recipe_hash = None;
    let mut checkpoint = args.checkpoint.clone();
    let mut recipe_ports: Vec<u16> = Vec::new();
    let source = match &args.recipe_name {
        Some(recipe_name) => {
            let built = build_recipe(engine, recipe_name, false, progress).await?;
            let entry = &built["recipe"];
            let text = |key: &str| entry[key].as_str().unwrap_or_default().to_owned();
            recipe_hash = Some(text("hash"));
            checkpoint = Some(text("checkpointId"));
            recipe_ports = entry["ports"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|port| port.as_u64().and_then(|port| u16::try_from(port).ok()))
                .collect();
            let all = engine.all_computers().await?;
            let mut builder = find(&all, &text("computerId"))?.clone();
            builder.name = recipe_name.clone();
            builder
        }
        None => engine.computer(&args.source).await?,
    };
    let name = match &args.name {
        Some(name) => name.clone(),
        None => default_fork_name(&source.name, &short_id()),
    };
    let started = crate::clock::now_unix();
    let known_before: Vec<Value> = source.checkpoints.clone();
    // The recipe's quiet and scrub lists are applied by the engine, from the recipe the
    // source was built from.
    let params = CheckpointForkParams {
        computer_id: source.id.clone(),
        device_id: None,
        checkpoint_id: checkpoint.clone(),
        new_name: name.clone(),
        secrets: args.secrets.clone(),
        quiet: None,
        recipe_hash,
        network_policy: None,
    };
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
    // The fork reaches the same ports its source publishes (a recipe fork, those the recipe
    // names), each on a host port of its own.
    let published = match &args.recipe_name {
        // Ports are published by this device's engine (`network.*` is not routed).
        _ if engine.device().is_some() => Ok(Vec::new()),
        Some(_) => crate::access::publish_recipe_ports(engine, &forked, &recipe_ports).await,
        None => crate::access::inherit_ports(engine, &source, &forked).await,
    };
    match published {
        Ok(ports) => fork["ports"] = json!(ports),
        Err(error) => fork["portsError"] = json!(error.message),
    }
    let used = match &checkpoint {
        Some(id) => known_before
            .iter()
            .find(|cp| cp["id"] == id.as_str())
            .cloned(),
        None => new_checkpoint(engine, &source.id, &known_before).await,
    };
    let (at, reused) = match &used {
        Some(cp) => (
            cp["createdAt"]
                .as_str()
                .and_then(crate::clock::parse_rfc3339),
            checkpoint.is_some(),
        ),
        None => (None, false),
    };
    // A checkpoint taken for this fork is as of the moment the fork began.
    let at = at.unwrap_or(started);
    fork["checkpoint"] = json!({
        "id": used.as_ref().map(|cp| cp["id"].clone()),
        "name": used.as_ref().map(|cp| cp["name"].clone()),
        "kind": used.as_ref().map(|cp| cp["kind"].clone()),
        "createdAt": crate::clock::format_rfc3339_utc(at),
        "ageSeconds": (started - at).max(0),
        "reused": reused,
    });
    fork["forkedAsOf"] = json!(crate::clock::local_clock(at));
    // A fork made to work in is running; the caller says if it should stay stopped.
    if !args.no_start && forked.state != "running" {
        start(engine, &name, progress).await?;
        let running = engine.computer(&name).await?;
        fork["state"] = json!(running.state);
    }
    if let Some(warning) = resource_warning(engine).await {
        fork["warning"] = json!(warning);
    }
    Ok(fork)
}

/// The sentence about the device's memory or disk when it is running short, so whoever makes
/// another computer knows before the device slows down. `None` when there is enough, or when
/// the engine cannot say.
pub async fn resource_warning(engine: &Engine) -> Option<String> {
    let resources = device_resources(engine).await.ok()?;
    matches!(resources["state"].as_str(), Some("low" | "critical"))
        .then(|| resources["message"].as_str().map(str::to_owned))
        .flatten()
}

/// `device.resources`: what the device has left and the computers running on it.
pub async fn device_resources(engine: &Engine) -> Result<Value, CliError> {
    engine
        .call(requests::device_resources(DeviceParams { device_id: None }))
        .await
}

/// The newest checkpoint of `computer_id` that was not in `before`: the one a fork without a
/// named checkpoint took.
async fn new_checkpoint(engine: &Engine, computer_id: &str, before: &[Value]) -> Option<Value> {
    let all = engine.all_computers().await.ok()?;
    let now = all.iter().find(|c| c.id == computer_id)?;
    now.checkpoints
        .iter()
        .filter(|cp| !before.iter().any(|old| old["id"] == cp["id"]))
        .max_by_key(|cp| {
            cp["createdAt"]
                .as_str()
                .and_then(crate::clock::parse_rfc3339)
                .unwrap_or(0)
        })
        .cloned()
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
/// What a baseline source starts with when the fork came from a recipe: `recipe:<hash>`.
const RECIPE_KEY_PREFIX: &str = "recipe:";
/// Name prefix of the hidden computers that recipes are built in.
pub const BUILDER_PREFIX: &str = "silo-build-";
/// Name prefix of the hidden copies an export without logins is made from.
pub const EXPORT_COPY_PREFIX: &str = "silo-export-";
/// Name prefix of the hidden computer a rebuild builds beside the one it rebuilds.
pub const REBUILD_BUILD_PREFIX: &str = "silo-rebuild-";
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
        object.insert(
            "credentialsUsed".to_owned(),
            credentials_used(engine, &target.id).await,
        );
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
    // A fork made from a recipe is keyed by the recipe; the copy starts from the recipe's
    // cached checkpoint.
    let (owner, checkpoint, recipe_hash) = match source.strip_prefix(RECIPE_KEY_PREFIX) {
        Some(hash) => {
            let listed = engine
                .call(requests::recipes_list(
                    silo_api::methods::common::DeviceParams { device_id: None },
                ))
                .await?;
            let entry = listed["recipes"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|entry| entry["hash"] == hash)
                .ok_or_else(|| {
                    CliError::new(
                        "No baseline: the recipe this computer was built from is no longer built.",
                    )
                })?;
            let owner = all
                .iter()
                .find(|c| entry["computerId"] == c.id.as_str())
                .ok_or_else(|| {
                    CliError::new("No baseline: the builder of this computer's recipe is gone.")
                })?;
            let checkpoint = entry["checkpointId"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            (owner, checkpoint, Some(hash.to_owned()))
        }
        None => {
            let owner = all
                .iter()
                .find(|c| c.checkpoints.iter().any(|cp| cp["id"] == source))
                .ok_or_else(|| {
                    CliError::new(
                        "No baseline: the checkpoint this computer was forked from no longer exists.",
                    )
                })?;
            (owner, source.to_owned(), None)
        }
    };
    // Throwaways left by an interrupted run are dropped first.
    for stale in stale_throwaways(&all, crate::clock::now_unix()) {
        let _ = remove_computer(engine, stale, progress).await;
    }
    let name = baseline_name(crate::clock::now_unix());
    let copy = BaselineCopy {
        owner,
        checkpoint: &checkpoint,
        recipe_hash: recipe_hash.as_deref(),
    };
    let outcome =
        measure_on_throwaway(engine, target, &copy, source, &name, seconds, progress).await;
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

/// What the idle copy that measures a baseline is made from.
struct BaselineCopy<'a> {
    /// The computer that holds the checkpoint.
    owner: &'a Computer,
    checkpoint: &'a str,
    /// Set when the baseline is keyed by a recipe.
    recipe_hash: Option<&'a str>,
}

async fn measure_on_throwaway(
    engine: &Engine,
    target: &Computer,
    copy: &BaselineCopy<'_>,
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
    let params = CheckpointForkParams {
        computer_id: copy.owner.id.clone(),
        device_id: None,
        checkpoint_id: Some(copy.checkpoint.to_owned()),
        new_name: name.to_owned(),
        secrets: Vec::new(),
        quiet: (!stop.is_empty() || !scrub.is_empty())
            .then_some(silo_api::methods::checkpoints::QuietSpec { stop, scrub }),
        recipe_hash: copy.recipe_hash.map(str::to_owned),
        network_policy: None,
    };
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
                    Some(now) if !matches!(now.state.as_str(), "stopped" | "failed") => {
                        tokio::time::sleep(STOP_POLL).await
                    }
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
    // Hidden throwaways (baselines, recipe builders) can be dropped by name or id.
    let all = engine.all_computers().await?;
    let computer = find(&all, name)?.clone();
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
    engine.forget_step_lock(&computer.id);
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
    let computer = engine.computer(&args.computer).await?;
    exec_on(engine, computer, args).await
}

/// Runs the command on this exact computer (by id), whatever its name has become.
pub async fn exec_on(
    engine: &Engine,
    computer: crate::engine::Computer,
    args: &ExecArgs,
) -> Result<ExecOutput, CliError> {
    exec_with_input(engine, computer, args, None, None).await
}

/// `exec_on` with bytes on the command's standard input and a larger output cap.
pub async fn exec_with_input(
    engine: &Engine,
    computer: crate::engine::Computer,
    args: &ExecArgs,
    stdin: Option<Vec<u8>>,
    max_output_bytes: Option<u64>,
) -> Result<ExecOutput, CliError> {
    if args.argv.is_empty() {
        return Err(CliError::new("Give a command to run after --."));
    }
    if args.timeout_secs == 0 || args.timeout_secs > MAX_EXEC_SECS {
        return Err(CliError::new(format!(
            "The timeout must be 1 to {MAX_EXEC_SECS} seconds; the engine does not run longer commands yet."
        )));
    }
    let reply = engine
        .call(requests::guest_exec(GuestExecParams {
            computer_id: computer.id,
            device_id: None,
            argv: args.argv.clone(),
            cwd: args.cwd.clone(),
            env: None,
            as_user: None,
            stdin,
            timeout_ms: args.timeout_secs.saturating_mul(1000),
            max_output_bytes,
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

pub fn parse_checkpoint_kind(text: &str) -> Result<CheckpointKind, CliError> {
    match text {
        "manual" => Ok(CheckpointKind::Manual),
        "automatic" => Ok(CheckpointKind::Automatic),
        "before-keep" => Ok(CheckpointKind::BeforeKeep),
        "step" => Ok(CheckpointKind::Step),
        other => Err(CliError::new(format!(
            "Unknown checkpoint kind {other}; use manual, automatic, before-keep or step."
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
                keep_newest: None,
            }),
            progress,
        )
        .await?;
    Ok(result.get("checkpoint").cloned().unwrap_or(result))
}

/// What a merge is asked beyond the fork and target.
#[derive(Clone, Debug, Default)]
pub struct MergeOptions {
    pub dry_run: bool,
    /// Carry database changes even though no migrate step covers them.
    pub data_ok: bool,
    /// The review digest from `silo diff --json`.
    pub reviewed: Option<String>,
    /// The answer for each item both sides changed, by item id.
    pub choices: std::collections::BTreeMap<String, KeepAnswer>,
    /// Items not to bring over even though they do not conflict.
    pub skip: Vec<String>,
}

/// Parses `item=fork`, `item=main` or `item=branch`. The item is an id as the merge lists it
/// (`file:/etc/app.conf`, `package:dpkg:jq`, `repo:/work/app`), so the answer follows the last
/// `=`.
pub fn parse_choice(text: &str) -> Result<(String, KeepAnswer), CliError> {
    let wrong = || {
        CliError::new(format!(
            "`{text}` is not an answer. Write <item>=<fork|main|branch>, with the item as the merge lists it."
        ))
    };
    let (item, answer) = text.rsplit_once('=').ok_or_else(wrong)?;
    if item.trim().is_empty() {
        return Err(wrong());
    }
    let answer = match answer.trim() {
        "fork" => KeepAnswer::Fork,
        "main" => KeepAnswer::Main,
        "branch" => KeepAnswer::Branch,
        _ => return Err(wrong()),
    };
    Ok((item.trim().to_owned(), answer))
}

/// Merges a fork's changes into `into` (default: the computer the fork was made from). A
/// refused merge is a result (`status: "refused"`), not an error: nothing was changed. A
/// refusal with `reason: "choices-needed"` lists the conflicts and the answers that settle
/// each.
pub async fn merge(
    engine: &Engine,
    fork: &str,
    into: Option<&str>,
    options: MergeOptions,
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    let MergeOptions {
        dry_run,
        data_ok,
        reviewed,
        choices,
        skip,
    } = options;
    let fork = engine.computer(fork).await?;
    // Without a digest, the one the last `silo diff` of this fork showed pins the merge.
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
    let label = if dry_run { "Checking" } else { "Merging" };
    engine
        .run_job(
            &format!("{label} {}", fork.name),
            requests::changes_keep(ChangesKeepParams {
                device_id: None,
                fork_id: fork.id,
                target_id,
                keep: None,
                dry_run: dry_run.then_some(true),
                data_ok: data_ok.then_some(true),
                review_digest: reviewed,
                choices: (!choices.is_empty()).then_some(choices),
                skip: (!skip.is_empty()).then_some(skip),
            }),
            progress,
        )
        .await
}

/// Replaces main with a fork: it takes `into`'s place (default: the computer it was made
/// from). A refused replace is a result (`status: "refused"`); nothing was changed.
pub async fn replace_main(
    engine: &Engine,
    fork: &str,
    into: Option<&str>,
    dry_run: bool,
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    let fork = engine.computer(fork).await?;
    if let Some(record) = crate::folder::record_for(engine, &fork.id) {
        return Err(CliError::new(format!(
            "{} was made from {}; it cannot replace main. Merge it into that folder instead.",
            fork.name, record.path
        )));
    }
    let target_id = match into {
        Some(name) => Some(engine.computer(name).await?.id),
        None => None,
    };
    let label = if dry_run {
        "Checking"
    } else {
        "Replacing main with"
    };
    engine
        .run_job(
            &format!("{label} {}", fork.name),
            requests::changes_promote(ChangesPromoteParams {
                device_id: None,
                fork_id: fork.id,
                target_id,
                dry_run: dry_run.then_some(true),
            }),
            progress,
        )
        .await
}

/// Rebuilds a Linux computer on the newest guest image, keeping its workspace and home. A
/// computer already on the newest image is refused unless `force` is set.
pub async fn rebuild(
    engine: &Engine,
    computer: &str,
    force: bool,
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    let computer = engine.computer(computer).await?;
    engine
        .run_job(
            &format!("Rebuilding {}", computer.name),
            requests::computers_rebuild(ComputerRebuildParams {
                device_id: None,
                computer_id: computer.id,
                force: force.then_some(true),
            }),
            progress,
        )
        .await
}

/// Puts a computer back to before its latest merge.
pub async fn undo(
    engine: &Engine,
    computer: &str,
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    let target = engine.computer(computer).await?;
    engine
        .run_job(
            &format!("Undoing the latest merge or replace of {}", target.name),
            requests::changes_undo(ChangesUndoParams {
                device_id: None,
                computer_id: target.id,
            }),
            progress,
        )
        .await
}

/// The names of the secrets each computer holds and the hosts each may be sent to, from
/// `computers.access`: the read a computer may make, which never returns a value.
pub async fn secrets_from_access(engine: &Engine) -> Result<Value, CliError> {
    let held = crate::access::held(engine, None).await?;
    let computers: Vec<Value> = held["computers"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|computer| json!({ "computer": computer["name"], "secrets": computer["secrets"] }))
        .collect();
    Ok(json!({ "computers": computers }))
}

/// The coding agents a computer has and whether each is installed.
pub async fn agents_list(engine: &Engine, computer: &str) -> Result<Value, CliError> {
    let target = engine.computer(computer).await?;
    let mut value = engine
        .call(requests::agents_list(target_of(&target)))
        .await?;
    value["computer"] = json!(target.name);
    Ok(value)
}

/// Installs a coding agent in a computer and returns the agents it now has.
pub async fn agent_install(
    engine: &Engine,
    computer: &str,
    agent: silo_api::methods::agents::AgentId,
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    let target = engine.computer(computer).await?;
    let mut value = engine
        .run_job(
            &format!("Installing {} in {}", agent.display_name(), target.name),
            requests::agents_install(silo_api::methods::agents::AgentsInstallParams {
                computer_id: target.id.clone(),
                device_id: None,
                agent,
            }),
            progress,
        )
        .await?;
    value["computer"] = json!(target.name);
    Ok(value)
}

/// Puts a computer back to one of its checkpoints, named by id or by name.
pub async fn restore(
    engine: &Engine,
    computer: &str,
    checkpoint: &str,
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    let target = engine.computer(computer).await?;
    let wanted = target
        .checkpoints
        .iter()
        .find(|cp| cp["id"] == checkpoint || cp["name"] == checkpoint)
        .and_then(|cp| cp["id"].as_str())
        .ok_or_else(|| {
            CliError::new(format!(
                "{} has no checkpoint {checkpoint}. `silo checkpoints {}` lists them.",
                target.name, target.name
            ))
        })?
        .to_owned();
    engine
        .run_job(
            &format!("Restoring {}", target.name),
            requests::checkpoints_restore(silo_api::methods::checkpoints::CheckpointTargetParams {
                computer_id: target.id.clone(),
                device_id: None,
                checkpoint_id: wanted,
            }),
            progress,
        )
        .await?;
    Ok(engine.computer(computer).await?.to_json())
}

pub async fn checkpoints(engine: &Engine, computer: &str) -> Result<Value, CliError> {
    let target = engine.computer(computer).await?;
    Ok(json!({ "computer": target.name, "checkpoints": target.checkpoints }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_desktop_sends_the_setting_the_apps_form_sends() {
        let args = |no_desktop| CreateArgs {
            name: "a".into(),
            platform: Platform::Container,
            cpus: None,
            memory_gib: None,
            disk_gib: None,
            no_desktop,
        };
        assert_eq!(desktop_of(&args(false)), None);
        let off = desktop_of(&args(true)).unwrap();
        assert!(!off.start_with_computer && !off.built_in);
    }

    #[test]
    fn default_fork_names_come_from_the_source() {
        assert_eq!(default_fork_name("main", "a1b2c3"), "main-a1b2c3");
        assert_eq!(short_id().len(), 6);
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
    fn merge_answers_parse_with_the_item_ids_the_merge_lists() {
        assert_eq!(
            parse_choice("file:/etc/app.conf=fork").unwrap(),
            ("file:/etc/app.conf".to_owned(), KeepAnswer::Fork)
        );
        assert_eq!(
            parse_choice("repo:/work/app=branch").unwrap().1,
            KeepAnswer::Branch
        );
        // The answer follows the last `=`: a path may hold one.
        assert_eq!(
            parse_choice("file:/a=b.txt=main").unwrap(),
            ("file:/a=b.txt".to_owned(), KeepAnswer::Main)
        );
        for bad in ["file:/x", "=fork", "file:/x=theirs", ""] {
            assert!(parse_choice(bad).is_err(), "{bad}");
        }
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

/// Builds a stored recipe, or returns its cached build. The result carries `recipe` (the
/// cache entry) and `built` (false for a cache hit).
pub async fn build_recipe(
    engine: &Engine,
    name: &str,
    force: bool,
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    engine
        .run_job(
            &format!("Building the recipe {name}"),
            requests::recipes_build(RecipeBuildParams {
                device_id: None,
                name: name.to_owned(),
                spec: None,
                force: force.then_some(true),
            }),
            progress,
        )
        .await
}

/// Stores a recipe under `name`, replacing the one of that name.
pub async fn recipe_add(
    engine: &Engine,
    name: &str,
    recipe: RecipeDefinition,
) -> Result<Value, CliError> {
    engine
        .call(requests::recipes_save(RecipeSaveParams {
            device_id: None,
            name: name.to_owned(),
            recipe,
        }))
        .await
}

/// One stored recipe with its hash, whether a build of it exists and any warnings.
pub async fn recipe_show(engine: &Engine, name: &str) -> Result<Value, CliError> {
    engine
        .call(requests::recipes_get(RecipeGetParams {
            device_id: None,
            name: name.to_owned(),
        }))
        .await
}

/// Every stored recipe, with whether the engine holds a build of it.
pub async fn recipe_list(engine: &Engine) -> Result<Value, CliError> {
    let listed = engine
        .call(requests::recipes_list(DeviceParams { device_id: None }))
        .await?;
    let built = listed["recipes"].as_array().cloned().unwrap_or_default();
    let recipes: Vec<Value> = listed["saved"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|saved| {
            let entry = built.iter().find(|entry| entry["name"] == saved["name"]);
            json!({
                "name": saved["name"],
                "origin": if saved["builtIn"] == true { "built-in" } else { "saved" },
                "os": saved["os"],
                "built": saved["built"],
                "builtAt": entry.map(|entry| entry["builtAt"].clone()),
                "imageVersion": entry.map(|entry| entry["imageVersion"].clone()),
            })
        })
        .collect();
    Ok(json!({ "recipes": recipes }))
}

/// Forgets a stored recipe and drops its build.
pub async fn recipe_remove(engine: &Engine, name: &str) -> Result<Value, CliError> {
    engine
        .call(requests::recipes_remove(RecipeRemoveParams {
            device_id: None,
            name: name.to_owned(),
        }))
        .await?;
    Ok(json!({ "removed": name }))
}

/// The recipe a computer was built from, as it was when it was built.
pub async fn computer_recipe(engine: &Engine, computer: &Computer) -> Result<Value, CliError> {
    let reply = engine
        .call(requests::computers_recipe(target(computer)))
        .await?;
    Ok(reply.get("recipe").cloned().unwrap_or(Value::Null))
}

// MARK: Up

/// Makes sure the engine answers, `main` exists (made from the recipe) and runs. It saves no
/// checkpoint: a fork starts from `main` as it is when the fork is made. The warm checkpoint
/// an earlier `up` saved is removed, so it does not keep a memory snapshot on disk. With
/// `ssh` (the home directory and channel), `ssh main.silo` is set up.
pub async fn up(
    engine: &Engine,
    recipe_name: &str,
    ssh: Option<(&std::path::Path, silo_client::channel::Channel)>,
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    let mut created = false;
    let mut recipe_ports: Vec<u16> = Vec::new();
    let mut main = match engine.computer(MAIN).await {
        Ok(main) => main,
        Err(_) => {
            let built = build_recipe(engine, recipe_name, false, progress).await?;
            let entry = &built["recipe"];
            let text = |key: &str| entry[key].as_str().unwrap_or_default().to_owned();
            recipe_ports = entry["ports"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|port| port.as_u64().and_then(|port| u16::try_from(port).ok()))
                .collect();
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
                network_policy: None,
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
    // The recipe's ports are published from main itself, so its forks inherit them.
    let (ports, ports_error) =
        match crate::access::publish_recipe_ports(engine, &main, &recipe_ports).await {
            Ok(ports) => (json!(ports), Value::Null),
            Err(error) => (Value::Null, json!(error.message)),
        };
    let ssh = match ssh {
        Some((home, channel)) => crate::access::ssh_for_up(engine, home, channel).await,
        None => Value::Null,
    };
    let left_by_earlier_versions: Vec<String> = main
        .checkpoints
        .iter()
        .filter(|cp| cp["kind"] == "warm")
        .filter_map(|cp| cp["id"].as_str().map(str::to_owned))
        .collect();
    for id in left_by_earlier_versions {
        // A checkpoint a fork still depends on stays until a later `up`.
        let _ = engine
            .run_job(
                "Removing the warm checkpoint of an earlier version",
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
        "publishedPorts": ports,
        "portsError": ports_error,
        "ssh": ssh,
        "warning": resource_warning(engine).await,
    }))
}
