//! The operations behind both the commands and the MCP tools. Each returns the JSON the
//! `--json` flag prints; `render` turns the same values into short text.

use serde_json::{json, Value};
use silo_api::methods::{
    changes::{ChangesBaselineParams, ChangesCaptureParams, ChangesDiffParams, ChangesRole},
    checkpoints::{CheckpointCreateParams, CheckpointForkParams, CheckpointKind},
    common::{ComputerTargetParams, DeviceParams},
    computers::{
        ComputerConfiguration, ComputerCreateParams, ComputerDeleteParams,
        MacosComputerCreateRequest,
    },
    guest::GuestExecParams,
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
        secrets: Vec::new(),
        quiet: None,
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
    // The fork-time record `silo diff` compares against; a fork without one still works.
    let record = engine
        .run_job(
            &format!("Recording {name} as it is now"),
            requests::changes_capture(ChangesCaptureParams {
                computer_id: forked.id.clone(),
                device_id: None,
                role: Some(ChangesRole::Base),
                source: Some(args.checkpoint.clone().unwrap_or_else(|| source.id.clone())),
            }),
            progress,
        )
        .await;
    if let Err(error) = record {
        progress(&format!(
            "warning: {name} has no fork-time record, so silo diff cannot compare it: {error}"
        ));
    }
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

/// What changed in a computer since it was forked: the diff document, plus the readable
/// `text` and what was compared.
pub async fn diff(
    engine: &Engine,
    args: &DiffArgs,
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    let target = engine.computer(&args.computer).await?;
    if args.compute_baseline {
        engine
            .run_job(
                &format!("Measuring the idle baseline of {}", target.name),
                requests::changes_baseline(ChangesBaselineParams {
                    computer_id: target.id.clone(),
                    device_id: None,
                    source: None,
                    idle_seconds: None,
                }),
                progress,
            )
            .await?;
    }
    let mut result = engine
        .run_job(
            &format!("Comparing {} with when it was forked", target.name),
            requests::changes_diff(ChangesDiffParams {
                computer_id: target.id,
                device_id: None,
                max_lines: args.max_lines,
                baseline: Some(!args.no_baseline),
                refresh: None,
            }),
            progress,
        )
        .await?;
    let mut document = result["diff"].take();
    if let Some(object) = document.as_object_mut() {
        for key in [
            "text",
            "baselineApplied",
            "baselineAvailable",
            "baseCapturedAt",
            "capturedAt",
        ] {
            object.insert(key.to_owned(), result[key].clone());
        }
    }
    Ok(document)
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
