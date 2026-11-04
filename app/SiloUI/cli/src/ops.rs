//! The operations behind both the commands and the MCP tools. Each returns the JSON the
//! `--json` flag prints; `render` turns the same values into short text.

use serde_json::{json, Value};
use silo_api::methods::{
    checkpoints::{CheckpointCreateParams, CheckpointForkParams, CheckpointKind},
    common::ComputerTargetParams,
    computers::{
        ComputerConfiguration, ComputerCreateParams, ComputerDeleteParams,
        MacosComputerCreateRequest,
    },
    guest::GuestExecParams,
    requests,
};

use crate::{
    config::{Recipe, Settings},
    engine::{Computer, Engine, Progress},
    error::CliError,
};

/// The computer that holds the long-lived work; dropping it needs `--force`.
pub const MAIN: &str = "main";

pub async fn list(engine: &Engine) -> Result<Value, CliError> {
    let computers = engine.computers().await?;
    Ok(json!({ "computers": computers.iter().map(Computer::to_json).collect::<Vec<_>>() }))
}

pub async fn status(engine: &Engine, name: &str) -> Result<Value, CliError> {
    let computer = engine.computer(name).await?;
    let mut value = computer.to_json();
    value["checkpointList"] = Value::Array(computer.checkpoints);
    Ok(value)
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
/// quiet fields yet, so this leaves them unchanged until the engine contract has them.
pub fn apply_fork_extras(_params: &mut CheckpointForkParams, _extras: &ForkExtras) {}

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
}

pub async fn fork(
    engine: &Engine,
    args: &ForkArgs,
    settings: Option<&Settings>,
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    Settings::check_fork_secrets(settings, &args.secrets)?;
    let source = engine.computer(&args.source).await?;
    let name = args
        .name
        .clone()
        .unwrap_or_else(|| default_fork_name(&source.name, &short_id()));
    let mut params = CheckpointForkParams {
        computer_id: source.id.clone(),
        device_id: None,
        checkpoint_id: args.checkpoint.clone(),
        new_name: name.clone(),
    };
    apply_fork_extras(
        &mut params,
        &fork_extras(args.recipe.as_ref(), &args.secrets),
    );
    engine
        .run_job(
            &format!("Forking {} into {name}", source.name),
            requests::checkpoints_fork(params),
            progress,
        )
        .await?;
    let mut fork = engine.computer(&name).await?.to_json();
    fork["source"] = json!(source.name);
    Ok(fork)
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

#[derive(Clone, Debug)]
pub struct ExecArgs {
    pub computer: String,
    pub argv: Vec<String>,
    pub cwd: Option<String>,
    pub timeout_secs: u64,
}

pub async fn exec(engine: &Engine, args: &ExecArgs) -> Result<Value, CliError> {
    if args.argv.is_empty() {
        return Err(CliError::new("Give a command to run after --."));
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
            timeout_ms: args.timeout_secs.saturating_mul(1000),
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
    Ok(json!({
        "exitCode": reply.get("exitCode").cloned().unwrap_or(Value::Null),
        "signal": reply.get("signal").cloned().unwrap_or(Value::Null),
        "stdout": String::from_utf8_lossy(&bytes("stdout")),
        "stderr": String::from_utf8_lossy(&bytes("stderr")),
    }))
}

pub fn parse_checkpoint_kind(text: &str) -> Result<CheckpointKind, CliError> {
    match text {
        "manual" => Ok(CheckpointKind::Manual),
        "automatic" => Ok(CheckpointKind::Automatic),
        "before-keep" => Ok(CheckpointKind::BeforeKeep),
        other => Err(CliError::new(format!(
            "Unknown checkpoint kind {other}; use manual, automatic or before-keep."
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
