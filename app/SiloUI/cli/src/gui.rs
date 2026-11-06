//! Forks that are driven through their desktop: the approval mode a fork is made with, and
//! "fork, start, wait until the desktop and the computer-use driver are ready". Every step is
//! an engine call that already exists (`computers.start`, `desktop.status`, `desktop.action`,
//! `computerUse.approval`, `computerUse.state`).

use std::time::{Duration, Instant};

use serde_json::{json, Value};
use silo_api::methods::{
    common::ComputerTargetParams,
    computer_use::{ApprovalMode, ComputerUseApprovalParams},
    desktop::{DesktopAction, DesktopActionParams},
    requests,
};

use crate::{
    config::Settings,
    engine::{Computer, Engine, Progress},
    error::CliError,
    ops::{self, ForkArgs, MAIN},
};

/// How long `--gui` waits for the desktop and the driver by default.
pub const DEFAULT_TIMEOUT_SECS: u64 = 300;
const POLL: Duration = Duration::from_secs(2);

/// The approval mode named by `--computer-use` or `computer_use`.
pub fn parse_approval(text: &str) -> Result<ApprovalMode, CliError> {
    match text {
        "ask" => Ok(ApprovalMode::Ask),
        "auto" => Ok(ApprovalMode::Auto),
        other => Err(CliError::new(format!(
            "`{other}` is not an approval mode; use ask or auto."
        ))),
    }
}

/// `auto` is a choice the user makes for unattended forks. A person typing the flag has made
/// it; an agent (MCP, or the CLI run with `--agent`/`SILO_AGENT`) needs `computer_use_auto` in
/// the project's settings. That file is one an agent can edit, so this is a convenience that
/// stops a casual `auto`, not a gate.
pub fn check_auto(
    mode: ApprovalMode,
    settings: Option<&Settings>,
    asked_by_agent: bool,
) -> Result<(), CliError> {
    if mode == ApprovalMode::Auto
        && asked_by_agent
        && !settings.is_some_and(|settings| settings.computer_use_auto)
    {
        return Err(CliError::new(
            "A fork made by an agent keeps asking for computer-use approval. To let agents make forks that run without asking, set computer_use_auto: true in .silo/silo.yaml.",
        ));
    }
    Ok(())
}

fn target(computer: &Computer) -> ComputerTargetParams {
    ComputerTargetParams {
        computer_id: computer.id.clone(),
        device_id: None,
    }
}

/// Whether `computer` is a fork: it has a source in the engine's tree and is not the computer
/// holding the `main` role. An archived `main-before-*` has no source, so it is not one either.
pub async fn ensure_fork(engine: &Engine, computer: &Computer) -> Result<(), CliError> {
    let tree = engine
        .call(requests::computers_tree(
            silo_api::methods::common::DeviceParams { device_id: None },
        ))
        .await?;
    let fork = computer.name != MAIN
        && tree["nodes"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|node| node["id"] == computer.id.as_str() && node["parent"].is_string());
    if fork {
        Ok(())
    } else {
        Err(CliError::new(format!(
            "{} is not a fork. Computer-use tools and the unattended approval are for forks; fork main (fork_gui) and use the fork.",
            computer.name
        )))
    }
}

/// Sets the approval mode of a fork. `main` never takes this path: its mode stays what the
/// user chose in the app.
pub async fn set_fork_approval(
    engine: &Engine,
    fork: &Computer,
    mode: ApprovalMode,
) -> Result<Value, CliError> {
    if fork.name == MAIN {
        return Err(CliError::new(
            "main keeps asking for computer-use approval; change it in the app. Only forks can run unattended.",
        ));
    }
    ensure_fork(engine, fork).await?;
    engine
        .call(requests::computer_use_approval(ComputerUseApprovalParams {
            computer_id: fork.id.clone(),
            device_id: None,
            mode,
        }))
        .await
}

/// What `status` and the fork results show about computer use.
pub fn summary(desktop: &Value) -> Value {
    let state = &desktop["computerUse"];
    json!({
        "state": state["state"],
        "approval": state["approval"],
        "appliedApproval": state["appliedApproval"],
        "driverVersion": state["driverVersion"],
        "reason": state["reason"],
        "session": desktop["sessionState"],
    })
}

#[derive(Debug, PartialEq, Eq)]
pub enum Readiness {
    Ready,
    Waiting(String),
    Failed(String),
}

/// Reads a `desktop.status` reply: ready once the desktop session runs and the driver says
/// ready; failed when either reports a failure.
pub fn readiness(desktop: &Value) -> Readiness {
    let session = desktop["sessionState"].as_str().unwrap_or("unknown");
    let driver = desktop["computerUse"]["state"]
        .as_str()
        .unwrap_or("unknown");
    if session == "failed" {
        return Readiness::Failed("the desktop session failed to start".into());
    }
    if driver == "failed" {
        let reason = desktop["computerUse"]["reason"]
            .as_str()
            .unwrap_or("unknown");
        return Readiness::Failed(format!("computer use failed to set up ({reason})"));
    }
    if session == "running" && driver == "ready" {
        return Readiness::Ready;
    }
    Readiness::Waiting(format!("desktop {session}, computer use {driver}"))
}

/// Waits until `computer` reports a ready desktop and driver, starting the desktop service
/// of a Linux computer that is running without one. Returns the final `desktop.status`.
pub async fn wait_ready(
    engine: &Engine,
    computer: &Computer,
    timeout: Duration,
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    let begun = Instant::now();
    let (mut last, mut desktop_started) = (String::new(), computer.kind != "linux");
    loop {
        let status = engine
            .call(requests::desktop_status(target(computer)))
            .await?;
        match readiness(&status) {
            Readiness::Ready => return Ok(status),
            Readiness::Failed(why) => {
                return Err(CliError::new(format!("{}: {why}.", computer.name)))
            }
            Readiness::Waiting(what) => {
                if what != last {
                    progress(&format!("{}: waiting for {what}", computer.name));
                    last = what.clone();
                }
                let stopped = status["sessionState"] == "stopped"
                    && !matches!(
                        status["state"].as_str(),
                        Some("computer-stopped") | Some("uninstalled") | None
                    );
                if stopped && !desktop_started {
                    desktop_started = true;
                    engine
                        .run_job(
                            "Starting the desktop",
                            requests::desktop_action(DesktopActionParams {
                                computer_id: computer.id.clone(),
                                device_id: None,
                                action: DesktopAction::Start,
                            }),
                            progress,
                        )
                        .await?;
                    continue;
                }
                if begun.elapsed() >= timeout {
                    return Err(CliError::new(format!(
                        "{} was not ready after {}s: {what}. It keeps running; `silo status {}` shows where it stands.",
                        computer.name,
                        timeout.as_secs(),
                        computer.name
                    )));
                }
            }
        }
        tokio::time::sleep(POLL).await;
    }
}

/// One look at whether `computer` can be driven now (no waiting): `ready`, what it waits for,
/// and the computer-use summary.
pub async fn ready_status(engine: &Engine, computer: &Computer) -> Result<Value, CliError> {
    let status = engine
        .call(requests::desktop_status(target(computer)))
        .await?;
    let (ready, waiting, failed) = match readiness(&status) {
        Readiness::Ready => (true, Value::Null, Value::Null),
        Readiness::Waiting(what) => (false, json!(what), Value::Null),
        Readiness::Failed(why) => (false, Value::Null, json!(why)),
    };
    Ok(json!({
        "computer": computer.name,
        "state": computer.state,
        "ready": ready,
        "waitingFor": waiting,
        "failed": failed,
        "computerUse": summary(&status),
    }))
}

pub struct GuiArgs {
    pub fork: ForkArgs,
    pub approval: Option<ApprovalMode>,
    pub timeout: Duration,
}

/// Forks, sets the fork's approval mode, starts it and waits for the desktop and the driver.
pub async fn fork_gui(
    engine: &Engine,
    args: &GuiArgs,
    settings: Option<&Settings>,
    progress: Progress<'_>,
) -> Result<Value, CliError> {
    let mut fork = ops::fork(engine, &args.fork, settings, progress).await?;
    let name = fork["name"].as_str().unwrap_or_default().to_owned();
    let computer = engine.computer(&name).await?;
    // Before the start, so the first boot applies the mode.
    if let Some(mode) = args.approval {
        set_fork_approval(engine, &computer, mode).await?;
    }
    if computer.state != "running" {
        ops::start(engine, &name, progress).await?;
    }
    let ready = wait_ready(engine, &computer, args.timeout, progress).await?;
    fork["state"] = json!("running");
    fork["computerUse"] = summary(&ready);
    fork["gui"] = json!(true);
    Ok(fork)
}

/// A fork's approval mode, applied when `--computer-use` is given without `--gui`.
pub async fn apply_to_fork(
    engine: &Engine,
    fork: &mut Value,
    mode: ApprovalMode,
) -> Result<(), CliError> {
    let computer = engine
        .computer(fork["name"].as_str().unwrap_or_default())
        .await?;
    let state = set_fork_approval(engine, &computer, mode).await?;
    fork["computerUse"] = json!({"approval": state["approval"], "appliedApproval": state["appliedApproval"], "state": state["state"]});
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn desktop(session: &str, driver: &str) -> Value {
        json!({"sessionState": session, "computerUse": {"state": driver, "reason": "doctor-failed"}})
    }

    #[test]
    fn ready_needs_the_session_and_the_driver() {
        assert_eq!(readiness(&desktop("running", "ready")), Readiness::Ready);
        assert!(matches!(
            readiness(&desktop("running", "installing")),
            Readiness::Waiting(_)
        ));
        assert!(matches!(
            readiness(&desktop("starting", "ready")),
            Readiness::Waiting(_)
        ));
    }

    #[test]
    fn failures_end_the_wait() {
        assert!(matches!(
            readiness(&desktop("failed", "ready")),
            Readiness::Failed(_)
        ));
        let Readiness::Failed(why) = readiness(&desktop("running", "failed")) else {
            panic!("a failed driver fails the wait");
        };
        assert!(why.contains("doctor-failed"));
    }

    #[test]
    fn auto_needs_the_settings_when_an_agent_asks() {
        assert!(check_auto(ApprovalMode::Auto, None, false).is_ok());
        assert!(check_auto(ApprovalMode::Auto, None, true).is_err());
        assert!(check_auto(ApprovalMode::Ask, None, true).is_ok());
        let allowed = Settings {
            computer_use_auto: true,
            ..Settings::default()
        };
        assert!(check_auto(ApprovalMode::Auto, Some(&allowed), true).is_ok());
    }

    #[test]
    fn modes_parse() {
        assert_eq!(parse_approval("auto").unwrap(), ApprovalMode::Auto);
        assert!(parse_approval("yes").is_err());
    }
}
