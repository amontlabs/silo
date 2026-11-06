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

/// Drops the stale throwaways, for at most [`SWEEP_LIMIT`]. Best effort; returns the names
/// dropped. A fork runs it alongside its own work, so a throwaway that cannot be dropped does
/// not hold the fork up.
pub async fn sweep_throwaways(engine: &Engine, progress: Progress<'_>) -> Vec<String> {
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
    let _ = tokio::time::timeout(SWEEP_LIMIT, sweep).await;
    dropped.into_inner().unwrap_or_default()
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
            stop: quiet_names(recipe),
            scrub: recipe.scrub.clone(),
        });
    ForkExtras {
        secrets: secrets.to_vec(),
        quiet,
    }
}

/// The recipe's `quiet:` list as the guest matches it. The guest stops processes by program
/// or script name, and a service's launcher `exec`s its command, so the running process of a
/// service never carries the service's name: an entry naming one of the recipe's `services:`
/// is kept (its launcher and autostart entry go by that name) and followed by the name its
/// process runs under.
fn quiet_names(recipe: &Recipe) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for entry in &recipe.quiet {
        let program = recipe
            .services
            .get(entry)
            .and_then(|command| service_program(command));
        for name in std::iter::once(entry.clone()).chain(program) {
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names
}

/// One line per quiet service whose running process name cannot be derived: only the service's
/// own name is stopped for it, which its process does not carry.
pub fn quiet_warnings(recipe: &Recipe) -> Vec<String> {
    recipe
        .quiet
        .iter()
        .filter_map(|entry| Some((entry, recipe.services.get(entry)?)))
        .filter(|(_, command)| service_program(command).is_none())
        .map(|(entry, _)| {
            format!(
                "quiet: cannot tell which process service {entry} runs; name its program or script in quiet: too."
            )
        })
        .collect()
}

/// Programs that run some other program; their name says nothing about the service.
const LAUNCHERS: [&str; 18] = [
    "env", "sudo", "nohup", "nice", "exec", "timeout", "java", "npm", "npx", "yarn", "pnpm", "uv",
    "docker", "make", "bun", "deno", "bash", "sh",
];

/// Interpreters whose process name is matched by the script they run.
const INTERPRETERS: [&str; 6] = ["python", "node", "nodejs", "perl", "ruby", "php"];

/// Interpreter options that run code or a module instead of a script file.
const NO_SCRIPT: [&str; 8] = [
    "-m",
    "-c",
    "-e",
    "-p",
    "-r",
    "--eval",
    "--print",
    "--require",
];

/// The program or script name a service command runs under, as the guest's quiet step matches
/// it: the script after an interpreter, by its full file name, when the script is a specific
/// file (`python3 -u /opt/app/poller.py` is `poller.py`; `node server` says too little), else
/// the program (`openclaw gateway` is `openclaw`). `None` for a launcher (`npm start`,
/// `sudo x`, `bash run.sh`) and when the command is not a plain command line the launcher
/// `exec`s.
fn service_program(command: &str) -> Option<String> {
    let plain = |c: char| c.is_ascii_alphanumeric() || "-_./:=@%+, ".contains(c);
    if !command.chars().all(plain) {
        return None;
    }
    let mut words = command.split_whitespace();
    let first = words.next().filter(|word| !word.contains('='))?;
    let base = |word: &str| word.rsplit('/').next().unwrap_or(word).to_owned();
    let program = base(first);
    let family = program.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.');
    if LAUNCHERS.contains(&family) {
        return None;
    }
    let name = if INTERPRETERS.contains(&family) {
        let mut options = Vec::new();
        let script = words.find(|word| {
            options.push(*word);
            !word.starts_with('-')
        })?;
        if options.iter().any(|option| NO_SCRIPT.contains(option)) {
            return None;
        }
        let name = base(script);
        let specific = name
            .split_once('.')
            .is_some_and(|(stem, extension)| !stem.is_empty() && !extension.is_empty());
        if !specific {
            return None;
        }
        name
    } else {
        program
    };
    let valid = |c: char| c.is_ascii_alphanumeric() || "._@:+-".contains(c);
    let usable = !name.is_empty() && name != "." && name != ".." && !name.starts_with('-');
    (usable && name.chars().all(valid)).then_some(name)
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

pub(crate) fn short_id() -> String {
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
    let (_, forked) = tokio::join!(
        sweep_throwaways(engine, progress),
        fork_now(engine, args, settings, progress)
    );
    forked
}

async fn fork_now(
    engine: &Engine,
    args: &ForkArgs,
    settings: Option<&Settings>,
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    let mut recipe_hash = None;
    let mut checkpoint = args.checkpoint.clone();
    let mut warm_reused = false;
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
    let started = crate::clock::now_unix();
    if checkpoint.is_none() {
        let max_age = Settings::warm_max_age_secs(settings);
        checkpoint = fresh_warm(&source, started, max_age)
            .and_then(|warm| warm["id"].as_str().map(str::to_owned));
        warm_reused = checkpoint.is_some();
    }
    let known_before: Vec<Value> = source.checkpoints.clone();
    let mut params = CheckpointForkParams {
        computer_id: source.id.clone(),
        device_id: None,
        checkpoint_id: checkpoint.clone(),
        new_name: name.clone(),
        secrets: Vec::new(),
        quiet: None,
        recipe_hash,
    };
    apply_fork_extras(
        &mut params,
        &fork_extras(args.recipe.as_ref(), &args.secrets),
    )?;
    for warning in args.recipe.iter().flat_map(quiet_warnings) {
        progress(&warning);
    }
    engine
        .run_job(
            &format!("Forking {} into {name}", source.name),
            requests::checkpoints_fork(params),
            progress,
        )
        .await?;
    let forked = engine.computer(&name).await?;
    match &args.recipe_name {
        Some(recipe_name) => crate::flows::remember_recipe(engine, &forked.id, recipe_name),
        None => crate::flows::inherit_recipe(engine, &source.id, &forked.id),
    }
    let mut fork = forked.to_json();
    fork["source"] = json!(source.name);
    // The fork reaches the same ports its source publishes (a recipe fork, those the recipe
    // names), each on a host port of its own.
    let published = match (&args.recipe_name, &args.recipe) {
        (Some(_), Some(recipe)) => {
            crate::access::publish_recipe_ports(engine, &forked, &recipe.ports).await
        }
        (Some(_), None) => Ok(Vec::new()),
        (None, _) => crate::access::inherit_ports(engine, &source, &forked).await,
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
        "unchanged": warm_reused,
    });
    fork["forkedAsOf"] = json!(crate::clock::local_clock(at));
    Ok(fork)
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

/// The newest warm checkpoint of `source`, when it was taken no more than `max_age` seconds
/// before `now`, `source` runs, and the engine can tell `source` is unchanged since: the
/// checkpoint records the generation `source` has now. A fork with no named checkpoint starts
/// from it; otherwise the engine takes a new one. A change made over a plain `ssh` session
/// that does not go through the engine is not seen, which is what the age limit is for.
pub fn fresh_warm(source: &Computer, now: i64, max_age: u64) -> Option<&Value> {
    let generation = source.generation?;
    if source.state != "running" {
        return None;
    }
    source
        .checkpoints
        .iter()
        .filter(|cp| cp["kind"] == "warm")
        .filter(|cp| cp["generation"].as_u64() == Some(generation))
        .filter_map(|cp| {
            let at = crate::clock::parse_rfc3339(cp["createdAt"].as_str()?)?;
            Some((at, cp))
        })
        .max_by_key(|(at, _)| *at)
        .filter(|(at, _)| now - at <= max_age as i64)
        .map(|(_, cp)| cp)
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
    let extras = ForkExtras {
        secrets: Vec::new(),
        quiet: (!stop.is_empty() || !scrub.is_empty()).then_some(QuietSpec { stop, scrub }),
    };
    let mut params = CheckpointForkParams {
        computer_id: copy.owner.id.clone(),
        device_id: None,
        checkpoint_id: Some(copy.checkpoint.to_owned()),
        new_name: name.to_owned(),
        secrets: Vec::new(),
        quiet: None,
        recipe_hash: copy.recipe_hash.map(str::to_owned),
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
        "step" => Ok(CheckpointKind::Step),
        other => Err(CliError::new(format!(
            "Unknown checkpoint kind {other}; use manual, automatic, before-keep, warm or step."
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
                device_id: None,
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
    if let Some(record) = crate::folder::record_for(engine, &fork.id) {
        return Err(CliError::new(format!(
            "{} was made from {}; it cannot become main. Fold it into that folder instead.",
            fork.name, record.path
        )));
    }
    let target_id = match into {
        Some(name) => Some(engine.computer(name).await?.id),
        None => None,
    };
    let label = if dry_run { "Checking" } else { "Promoting" };
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
                device_id: None,
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
    fn a_quiet_service_also_stops_the_program_it_runs() {
        let recipe = parse_recipe(concat!(
            "services:\n",
            "  final-poller: python3 -u /opt/final/poller.py\n",
            "  gateway: openclaw gateway\n",
            "  shell: cd /srv && ./run\n",
            "quiet: [final-poller, gateway, openclaw, shell, cron]\n",
        ))
        .unwrap();
        let stop = fork_extras(Some(&recipe), &[]).quiet.unwrap().stop;
        assert_eq!(
            stop,
            [
                "final-poller",
                "poller.py",
                "gateway",
                "openclaw",
                "shell",
                "cron"
            ]
        );
        assert_eq!(
            service_program("node server.js").as_deref(),
            Some("server.js")
        );
        assert_eq!(
            service_program("/usr/bin/redis-server --port 6380").as_deref(),
            Some("redis-server")
        );
        assert_eq!(service_program("PORT=1 node app.js"), None);
        assert_eq!(service_program("python3"), None);
    }

    #[test]
    fn launchers_and_unspecific_scripts_name_no_process_and_warn() {
        for command in [
            "env FOO=1 poller",
            "sudo -u bot poller",
            "nohup poller",
            "nice -n 5 poller",
            "exec poller",
            "timeout 60 poller",
            "java -jar /opt/app.jar",
            "npm start",
            "npx serve",
            "yarn dev",
            "pnpm run bot",
            "uv run bot.py",
            "docker run app",
            "make serve",
            "bun run index.ts",
            "deno run main.ts",
            "bash run.sh",
            "/bin/sh -c poller",
            "node server",
            "python3 -m http.server",
            "python3 -c pass",
            "node -e 1",
            "python3 -u /opt/app/.env",
        ] {
            assert_eq!(service_program(command), None, "{command}");
        }
        assert_eq!(
            service_program("python3.12 -u /opt/app/poller.py").as_deref(),
            Some("poller.py")
        );
        assert_eq!(
            service_program("node /srv/bot/index.mjs").as_deref(),
            Some("index.mjs")
        );
        let recipe = parse_recipe(concat!(
            "services:\n",
            "  web: npm start\n",
            "  poller: python3 /opt/poller.py\n",
            "quiet: [web, poller, cron]\n",
        ))
        .unwrap();
        assert_eq!(
            fork_extras(Some(&recipe), &[]).quiet.unwrap().stop,
            ["web", "poller", "poller.py", "cron"]
        );
        let warnings = quiet_warnings(&recipe);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("service web"), "{}", warnings[0]);
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
                device_id: None,
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
        .call(requests::recipes_list(
            silo_api::methods::common::DeviceParams { device_id: None },
        ))
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
            device_id: None,
            name: name.to_owned(),
        }))
        .await?;
    Ok(json!({ "removed": name }))
}

// MARK: Up

const WARM_NAME: &str = "silo-warm";

/// Makes sure the engine answers, `main` exists (made from the recipe) and runs, and keeps a
/// warm checkpoint of it. The previous warm checkpoint is replaced only after the new one has
/// been saved; a fork that names no checkpoint uses the warm one while it is young enough
/// (`warm_max_age`, one minute by default; see [`fresh_warm`]). With `ssh` (the home
/// directory and channel), `ssh main.silo` is set up first: the engine counts the SSH
/// connection it prepares as a change to main, so done after the checkpoint it would leave
/// the checkpoint unusable.
pub async fn up(
    engine: &Engine,
    recipe_name: &str,
    recipe: &Recipe,
    ssh: Option<(&std::path::Path, silo_client::channel::Channel)>,
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
            crate::flows::remember_recipe(engine, &engine.computer(MAIN).await?.id, recipe_name);
            engine.computer(MAIN).await?
        }
    };
    if main.state != "running" {
        start(engine, MAIN, progress).await?;
        main = engine.computer(MAIN).await?;
    }
    // The recipe's ports are published from main itself, so its forks inherit them.
    let (ports, ports_error) =
        match crate::access::publish_recipe_ports(engine, &main, &recipe.ports).await {
            Ok(ports) => (json!(ports), Value::Null),
            Err(error) => (Value::Null, json!(error.message)),
        };
    let ssh = match ssh {
        Some((home, channel)) => crate::access::ssh_for_up(engine, home, channel).await,
        None => Value::Null,
    };
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
        "publishedPorts": ports,
        "portsError": ports_error,
        "ssh": ssh,
    }))
}
