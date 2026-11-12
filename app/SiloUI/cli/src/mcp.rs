//! `silo mcp`: the commands as MCP tools over stdio, for an agent on the device and for the
//! agent inside a computer. Every tool makes the same engine calls as its command and answers
//! with the same JSON as `--json`. A tool that needs the owner's answer waits a bounded time
//! and then says it is waiting and gives the request's id.

use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};

use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, ContentBlock},
    schemars, tool, tool_handler, tool_router, ServerHandler, ServiceExt,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    access, approvals,
    endpoint::Transport,
    engine::{build_channel, Engine},
    error::{exit, CliError},
    fanout::{self, ManyArgs},
    flows, folder,
    ops::{self, DiffArgs, ExecArgs, ForkArgs, MergeOptions},
    recipe_file, review,
    steps::{self, StepSettings},
};

/// The tools a server inside a computer does not offer: they reach the device's own folders,
/// SSH names and the owner's views.
pub const DEVICE_ONLY_TOOLS: &[&str] = &["up", "ssh_name", "request_open"];

/// How long a tool waits for the owner's answer when the call names no time. It is shorter
/// than the time out a client usually gives a tool.
pub const DEFAULT_WAIT_SECS: u64 = 30;
/// The longest a tool may be asked to wait.
pub const MAX_WAIT_SECS: u64 = 100;

#[derive(Clone)]
pub struct SiloMcp {
    transport: Arc<Transport>,
    working_directory: PathBuf,
    engine: Arc<tokio::sync::Mutex<Option<Engine>>>,
    /// The driver session of each computer driven through this server, by the device the
    /// computer is on (`None`: this one) and its id.
    sessions:
        Arc<std::sync::Mutex<HashMap<SessionKey, Arc<tokio::sync::Mutex<crate::driver::Session>>>>>,
    tool_router: ToolRouter<Self>,
}

/// A driver session's computer: the device it is on (`None`: this one) and its id.
type SessionKey = (Option<String>, String);

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    /// A connected device (name or id) to list instead of this one.
    pub device: Option<String>,
    /// List this device's computers and every connected device's.
    #[serde(default)]
    pub all_devices: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeviceParam {
    /// A connected device (name or id) to read instead of this one.
    pub device: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeviceComputerParam {
    /// The computer's name.
    pub computer: String,
    /// A connected device (name or id) to read instead of this one.
    pub device: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ComputerParam {
    /// The computer's name.
    pub computer: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OptionalComputerParam {
    /// The computer's name; without it, every computer you can see.
    pub computer: Option<String>,
}

/// What a tool that may need the owner's answer takes besides its own parameters is spelled out
/// in each of them: how long to wait for the answer, and your reason.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PublishParams {
    /// The computer's name.
    pub computer: String,
    /// The port inside the computer.
    pub port: u16,
    /// Serve it as a website.
    #[serde(default)]
    pub http: bool,
    /// Seconds to wait for the owner's answer if one is needed (default 30, at most 100).
    pub wait_secs: Option<u64>,
    /// Why you ask, shown to the owner beside the request.
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UnpublishParams {
    /// The computer's name.
    pub computer: String,
    /// The port inside the computer.
    pub port: u16,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateParams {
    /// The new computer's name.
    pub name: String,
    /// The stored recipe to build it from (see `recipe_list`); default linux-dev.
    pub recipe: Option<String>,
    /// Leave it stopped.
    #[serde(default)]
    pub no_start: bool,
    /// Seconds to wait for the owner's answer if one is needed (default 30, at most 100).
    pub wait_secs: Option<u64>,
    /// Why you ask, shown to the owner beside the request.
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ForkParams {
    /// The computer to copy: your own computer by default, `main` on the device, or on the
    /// device a path to a git repository (`./repo`) to copy into a new Linux fork.
    #[serde(default)]
    pub source: Option<String>,
    /// Make this many forks of the same checkpoint at once (default 1), each started. The
    /// `name` becomes a prefix.
    pub count: Option<u32>,
    /// A command to run in each fork once it is started, with no time limit. The result has
    /// each fork's exit code and output; the fork is left for review.
    #[serde(default)]
    pub command: Vec<String>,
    /// The fork's name; defaults to the source name plus a short id.
    pub name: Option<String>,
    /// Fork from this checkpoint id instead of the current state.
    pub checkpoint: Option<String>,
    /// Fork from the checkpoint taken after this step number of the computer (see
    /// `steps_list`); it needs step checkpoints turned on when the step ran.
    #[serde(rename = "fromStep", alias = "from_step")]
    pub from_step: Option<u32>,
    /// Silo-managed secrets the fork may use (names from `access`); one the source lacks needs
    /// the owner's answer.
    #[serde(default)]
    pub secrets: Vec<String>,
    /// Leave the fork stopped. A fork starts by default.
    #[serde(default)]
    pub no_start: bool,
    /// Save a checkpoint after each command run with `command`.
    #[serde(default)]
    pub step_checkpoints: bool,
    /// Seconds to wait for the owner's answer if one is needed (default 30, at most 100).
    pub wait_secs: Option<u64>,
    /// Why you ask, shown to the owner beside the request.
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MergeParams {
    /// The fork whose changes are merged.
    pub fork: String,
    /// The computer to merge into; defaults to the one the fork was made from.
    pub into: Option<String>,
    /// Report what would be merged and what would stop it, changing nothing.
    #[serde(default)]
    pub dry_run: bool,
    /// How to settle each item both sides changed, by the item's id from an earlier refusal:
    /// `fork` (the fork's version), `main` (leave it) or, for a repository, `branch` (the fork's
    /// commits arrive as a branch and nothing is merged).
    #[serde(default)]
    pub choices: std::collections::BTreeMap<String, String>,
    /// Item ids not to bring over even though they do not conflict.
    #[serde(default)]
    pub skip: Vec<String>,
    /// Carry database changes even though no migrate step covers them.
    #[serde(default)]
    pub data_ok: bool,
    /// The reviewDigest from `diff`; the merge refuses if the fork changed since.
    pub reviewed: Option<String>,
    /// Record a review request for the fork instead of merging it; returns the request.
    #[serde(default)]
    pub request: bool,
    /// Seconds to wait for the owner's answer if one is needed (default 30, at most 100).
    pub wait_secs: Option<u64>,
    /// Why you ask, shown to the owner beside the request.
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReplaceMainParams {
    /// The fork that takes main's place.
    pub fork: String,
    /// The computer to replace; defaults to the one the fork was made from.
    pub into: Option<String>,
    /// Report what would happen and what would stop it, changing nothing.
    #[serde(default)]
    pub dry_run: bool,
    /// Seconds to wait for the owner's answer if one is needed (default 30, at most 100).
    pub wait_secs: Option<u64>,
    /// Why you ask, shown to the owner beside the request.
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UndoParams {
    /// The computer a merge or replace changed (for a fork made from a folder, the fork or the
    /// folder's path).
    pub computer: String,
    /// Seconds to wait for the owner's answer if one is needed (default 30, at most 100).
    pub wait_secs: Option<u64>,
    /// Why you ask, shown to the owner beside the request.
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RequestsListParams {
    /// open (default), kept, thrown-away or all.
    pub status: Option<String>,
    /// List this device's requests and every connected device's; each has `deviceName`, and
    /// `unreachable` names the devices that could not be read.
    #[serde(default)]
    pub all_devices: bool,
    /// A connected device (name or id) to run this on instead of this one.
    pub device: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RequestIdParam {
    /// The request's id, such as fr-ab12cd34.
    pub id: String,
    /// The device the request is on, for a request on another device.
    pub device: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RequestOnlyParam {
    /// The request's id, such as fr-ab12cd34.
    pub id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RequestMergeParams {
    /// The request's id, such as fr-ab12cd34.
    pub id: String,
    /// Carry database changes even though no migrate step covers them.
    #[serde(default)]
    pub data_ok: bool,
    /// Answers for items both sides changed (see `merge`).
    #[serde(default)]
    pub choices: std::collections::BTreeMap<String, String>,
    /// Item ids not to bring over.
    #[serde(default)]
    pub skip: Vec<String>,
    /// Seconds to wait for the owner's answer if one is needed (default 30, at most 100).
    pub wait_secs: Option<u64>,
    /// Why you ask, shown to the owner beside the request.
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpParams {
    /// The stored recipe `main` is made from when it does not exist (default linux-dev).
    pub recipe: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExecParams {
    /// The computer to run in.
    pub computer: String,
    /// The command and its arguments.
    pub command: Vec<String>,
    /// Working directory in the computer.
    pub cwd: Option<String>,
    /// Seconds before the command is stopped (default and maximum 180).
    pub timeout_secs: Option<u64>,
    /// Save a checkpoint of the computer after the command (a step checkpoint).
    #[serde(default)]
    pub step_checkpoints: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckpointParams {
    /// The computer to checkpoint.
    pub computer: String,
    /// A name for the checkpoint.
    pub name: Option<String>,
    /// manual, automatic or before-keep.
    pub kind: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RestoreParams {
    /// The computer to put back.
    pub computer: String,
    /// The checkpoint's id or name (see `checkpoints`).
    pub checkpoint: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiffParams {
    /// The fork to look at.
    pub computer: String,
    /// Cut the text to this many lines (default 15).
    pub max_lines: Option<u32>,
    /// Include the idle fork's own background changes.
    #[serde(default)]
    pub no_baseline: bool,
    /// A connected device (name or id) to run this on instead of this one.
    pub device: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NetworkPolicyParams {
    /// The computer whose policy to read or change.
    pub computer: String,
    /// open, allowlist or closed. Without any of the fields below, the policy is only read.
    pub mode: Option<String>,
    /// Hosts to allow on port 443, like `example.com` or `*.example.com`.
    #[serde(default)]
    pub allow: Vec<String>,
    /// Hosts or presets to take off the list.
    #[serde(default)]
    pub remove: Vec<String>,
    /// Named host lists to add, like `npm` or `github`.
    #[serde(default)]
    pub presets: Vec<String>,
    /// Seconds to wait for the owner's answer if one is needed (default 30, at most 100).
    pub wait_secs: Option<u64>,
    /// Why you ask, shown to the owner beside the request.
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentsParams {
    /// The computer.
    pub computer: String,
    /// `claude-code` or `codex` to install it; without it, the agents are only listed.
    pub install: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ApprovalsParams {
    /// `list` (default), `status`, `wait` or `note`.
    pub action: Option<String>,
    /// The request's id, for status, wait and note.
    pub id: Option<String>,
    /// Your reason, for note.
    pub text: Option<String>,
    /// For wait: seconds to wait for the owner's answer (default 30, at most 100).
    pub wait_secs: Option<u64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StartStopParams {
    /// The computer's name.
    pub computer: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScreenshotParams {
    /// The computer's name.
    pub computer: String,
    /// The longest side of the picture in pixels (default 1280); larger screens are scaled down.
    pub max_side: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClickParams {
    /// The computer's name.
    pub computer: String,
    /// Pixels from the left of the last screenshot.
    pub x: f64,
    /// Pixels from the top of the last screenshot.
    pub y: f64,
    /// `left` (default), `right` or `middle`.
    pub button: Option<String>,
    /// Double-click.
    #[serde(default)]
    pub double: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TypeParams {
    /// The computer's name.
    pub computer: String,
    /// The text to type into the focused window.
    pub text: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KeyParams {
    /// The computer's name.
    pub computer: String,
    /// One key, like `Return` or `Tab`.
    pub key: String,
    /// Modifiers held with it, like `["ctrl"]`.
    #[serde(default)]
    pub modifiers: Vec<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HotkeyParams {
    /// The computer's name.
    pub computer: String,
    /// A chord of at least two keys, like `["ctrl", "l"]`.
    pub keys: Vec<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScrollParams {
    /// The computer's name.
    pub computer: String,
    /// Where to scroll, in pixels of the latest screenshot.
    pub x: f64,
    pub y: f64,
    /// `up`, `down`, `left` or `right`.
    pub direction: String,
    /// How much, 1 to 50 (default 3).
    pub amount: Option<u32>,
    /// `line` (default) or `page`.
    pub by: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MoveParams {
    /// The computer's name.
    pub computer: String,
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DragParams {
    /// The computer's name.
    pub computer: String,
    pub from_x: f64,
    pub from_y: f64,
    pub to_x: f64,
    pub to_y: f64,
    /// `left` (default), `right` or `middle`.
    pub button: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WaitParams {
    /// The computer's name.
    pub computer: String,
    /// Seconds to wait, at most 30.
    pub seconds: f64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DriverCallParams {
    /// The computer's name.
    pub computer: String,
    /// A tool of the computer's Cua Driver (see `computer_use_tools`).
    pub tool: String,
    /// The tool's arguments.
    #[serde(default)]
    pub arguments: Option<Value>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ForkGuiParams {
    /// The computer to copy (your own computer by default, `main` on the device).
    pub source: Option<String>,
    /// The fork's name.
    pub name: Option<String>,
    /// How long to wait for the desktop and the driver (default 300).
    pub timeout_secs: Option<u64>,
}

fn drive_result(answer: crate::usetools::Answer) -> CallToolResult {
    let content = answer
        .parts
        .into_iter()
        .map(|part| match part {
            crate::usetools::Part::Text(text) => ContentBlock::text(text),
            crate::usetools::Part::Image { data, mime } => ContentBlock::image(data, mime),
        })
        .collect();
    if answer.is_error {
        CallToolResult::error(content)
    } else {
        CallToolResult::success(content)
    }
}

/// A tool's answer. A command that has to wait for the owner and ran out of time is not a
/// failure: it says it is waiting and names the request to check later.
fn show(result: Result<Value, CliError>) -> Result<String, String> {
    match result {
        Ok(value) => Ok(serde_json::to_string_pretty(&value).unwrap_or_default()),
        Err(error) if error.exit == exit::WAITING => Ok(serde_json::to_string_pretty(&json!({
            "status": "waiting-for-owner",
            "approvalId": error.approval_id,
            "message": error.message,
        }))
        .unwrap_or_default()),
        Err(error) => Err(error.message),
    }
}

fn answers(
    choices: &std::collections::BTreeMap<String, String>,
) -> Result<std::collections::BTreeMap<String, silo_api::methods::keep::KeepAnswer>, CliError> {
    choices
        .iter()
        .map(|(item, answer)| ops::parse_choice(&format!("{item}={answer}")))
        .collect()
}

#[tool_router]
impl SiloMcp {
    pub fn new(socket: PathBuf, working_directory: PathBuf) -> Self {
        Self::for_transport(Transport::Socket(socket), working_directory)
    }

    /// The server for `transport`. Inside a computer (its channel), the tools that only make
    /// sense on the device are not offered: starting main from a recipe, SSH names and opening
    /// a review request's fork from the device.
    pub fn for_transport(transport: Transport, working_directory: PathBuf) -> Self {
        let in_computer = matches!(transport, Transport::Channel(_));
        let mut server = Self::with_transport(transport, working_directory);
        if in_computer {
            for name in DEVICE_ONLY_TOOLS {
                server.tool_router.remove_route(name);
            }
        }
        server
    }

    fn in_computer(&self) -> bool {
        matches!(*self.transport, Transport::Channel(_))
    }

    /// Refuses what needs the device this server is not on: another device, or a folder.
    fn on_the_device_only(
        &self,
        device: Option<&str>,
        source: Option<&str>,
    ) -> Result<(), CliError> {
        if !self.in_computer() {
            return Ok(());
        }
        if device.is_some_and(|device| device != "local") {
            return Err(CliError::new(
                "Connected devices are reached from the device, not from inside a computer.",
            ));
        }
        if source.is_some_and(folder::is_folder) {
            return Err(CliError::new(
                "Forking a folder is done on the device, not from inside a computer.",
            ));
        }
        Ok(())
    }

    fn with_transport(transport: Transport, working_directory: PathBuf) -> Self {
        Self {
            transport: Arc::new(transport),
            working_directory,
            engine: Arc::new(tokio::sync::Mutex::new(None)),
            sessions: Arc::new(std::sync::Mutex::new(HashMap::new())),
            tool_router: Self::tool_router(),
        }
    }

    /// The engine acting on `device` (a name or id), or on this device when none is given.
    /// Only reads go to another device through this server.
    async fn engine_on(&self, device: Option<&str>) -> Result<Engine, CliError> {
        self.on_the_device_only(device, None)?;
        let engine = self.engine().await?;
        match device {
            None | Some("local") => Ok(engine),
            Some(name) => {
                let id = engine.resolve_device(name).await?;
                Ok(engine.on_device(Some(id)))
            }
        }
    }

    /// The one connection this server keeps, opened again when it has ended.
    async fn engine(&self) -> Result<Engine, CliError> {
        let mut held = self.engine.lock().await;
        if let Some(engine) = held.as_ref().filter(|engine| !engine.is_closed()) {
            return Ok(engine.clone());
        }
        let engine = Engine::connect_to(&self.transport).await?;
        *held = Some(engine.clone());
        Ok(engine)
    }

    /// The connection for a call that may need the owner's answer: it waits `wait_secs` for it
    /// (default 30, at most 100) and then reports the request instead of the result.
    async fn engine_asking(
        &self,
        wait_secs: Option<u64>,
        reason: Option<String>,
    ) -> Result<Engine, CliError> {
        let wait = Duration::from_secs(
            wait_secs
                .unwrap_or(DEFAULT_WAIT_SECS)
                .clamp(0, MAX_WAIT_SECS),
        );
        Ok(self.engine().await?.with_approvals(approvals::WaitOptions {
            wait,
            reason,
            announce: false,
            ..approvals::WaitOptions::default()
        }))
    }

    /// The computer a fork is made from when the call names none: the computer this server
    /// runs in, else `main`.
    async fn default_source(&self, engine: &Engine) -> Result<String, CliError> {
        let Transport::Channel(endpoint) = &*self.transport else {
            return Ok(ops::MAIN.to_owned());
        };
        engine
            .computers()
            .await?
            .into_iter()
            .find(|computer| computer.id == endpoint.computer_id)
            .map(|computer| computer.name)
            .ok_or_else(|| CliError::new("Say which computer to fork: `source`."))
    }

    #[tool(
        description = "List the computers you can see and whether each is running. Start here to learn the names you can pass to the other tools."
    )]
    async fn list(&self, Parameters(params): Parameters<ListParams>) -> Result<String, String> {
        show(
            async {
                let engine = self.engine_on(params.device.as_deref()).await?;
                if params.all_devices {
                    ops::list_everywhere(&engine, false).await
                } else {
                    ops::list(&engine).await
                }
            }
            .await,
        )
    }

    #[tool(
        description = "Show one computer: its state, size, checkpoints, the recipe it was built from and what it has used."
    )]
    async fn status(
        &self,
        Parameters(DeviceComputerParam { computer, device }): Parameters<DeviceComputerParam>,
    ) -> Result<String, String> {
        show(
            async { ops::status(&self.engine_on(device.as_deref()).await?, &computer).await }.await,
        )
    }

    #[tool(
        description = "Show how the computers descend from each other: each has a parent, what it was forked from, its state and its pending changes. Use it to see which forks exist and are waiting to be merged."
    )]
    async fn tree(&self, Parameters(params): Parameters<DeviceParam>) -> Result<String, String> {
        show(async { steps::tree(&self.engine_on(params.device.as_deref()).await?).await }.await)
    }

    #[tool(
        description = "Show what the device has left (memory, disk, running computers) and whether it is running short, and the CPUs, memory and disk size of each computer you can see. Check it before you make several computers."
    )]
    async fn resources(&self) -> Result<String, String> {
        show(async { ops::device_resources(&self.engine().await?).await }.await)
    }

    #[tool(
        description = "Show what a computer holds: the names of its secrets and the hosts they may be sent to (never their values), its GitHub access, published ports, SSH state and network policy. Without a computer, every computer you can see."
    )]
    async fn access(
        &self,
        Parameters(OptionalComputerParam { computer }): Parameters<OptionalComputerParam>,
    ) -> Result<String, String> {
        show(async { access::held(&self.engine().await?, computer.as_deref()).await }.await)
    }

    #[tool(
        description = "List the Silo secrets by name for each computer you can see, with the hosts each may be sent to. Values are never returned, and none can be set through this server."
    )]
    async fn secrets_list(&self) -> Result<String, String> {
        show(async { ops::secrets_from_access(&self.engine().await?).await }.await)
    }

    #[tool(
        description = "Start `main`, making it from a recipe the first time (default linux-dev). Only on the device, not inside a computer."
    )]
    async fn up(&self, Parameters(params): Parameters<UpParams>) -> Result<String, String> {
        show(
            async {
                let name = params
                    .recipe
                    .unwrap_or_else(|| recipe_file::DEFAULT_RECIPE.to_owned());
                recipe_file::check_name(&name)?;
                let engine = self.engine().await?;
                let home = access::home_dir();
                let ssh = home
                    .as_ref()
                    .ok()
                    .map(|home| (home.as_path(), build_channel()));
                let mut value = ops::up(&engine, &name, ssh, &|_| {}).await?;
                if let Err(error) = home {
                    value["ssh"] = json!({ "error": error.message });
                }
                Ok(value)
            }
            .await,
        )
    }

    #[tool(
        description = "List the stored recipes (descriptions of a computer's system, packages and setup) and whether each is built. Use one with `create`."
    )]
    async fn recipe_list(&self) -> Result<String, String> {
        show(async { ops::recipe_list(&self.engine().await?).await }.await)
    }

    #[tool(
        description = "Make a new computer from a stored recipe (default linux-dev) and start it. Use it for a clean machine; use `fork` to copy a computer you already have."
    )]
    async fn create(&self, Parameters(params): Parameters<CreateParams>) -> Result<String, String> {
        show(
            async {
                let engine = self.engine_asking(params.wait_secs, params.reason).await?;
                let recipe = params
                    .recipe
                    .unwrap_or_else(|| recipe_file::DEFAULT_RECIPE.to_owned());
                recipe_file::check_name(&recipe)?;
                let args = ForkArgs {
                    source: String::new(),
                    name: Some(params.name),
                    checkpoint: None,
                    secrets: Vec::new(),
                    recipe_name: Some(recipe),
                    no_start: params.no_start,
                };
                ops::fork(&engine, &args, &|_| {}).await
            }
            .await,
        )
    }

    #[tool(
        description = "Copy a computer into a new fork to try something without risking the original, and start it. `count` makes several at once and `command` runs in each; look at the result with `diff`, then `merge` or `drop` it."
    )]
    async fn fork(&self, Parameters(params): Parameters<ForkParams>) -> Result<String, String> {
        show(
            async {
                let engine = self.engine_asking(params.wait_secs, params.reason).await?;
                let source = match params.source {
                    Some(source) => source,
                    None => self.default_source(&engine).await?,
                };
                self.on_the_device_only(None, Some(&source))?;
                let checkpoint = match params.from_step {
                    Some(number) => {
                        if params.checkpoint.is_some() || folder::is_folder(&source) {
                            return Err(CliError::new(
                                "fromStep names a step of a computer; it cannot be combined with checkpoint or a folder.",
                            ));
                        }
                        Some(steps::checkpoint_for(&engine, &source, number).await?)
                    }
                    None => params.checkpoint.clone(),
                };
                if folder::is_folder(&source) {
                    if params.count.unwrap_or(1) != 1 {
                        return Err(CliError::new(
                            "count makes forks of a computer; a folder is copied into one fork.",
                        ));
                    }
                    let mut made = folder::fork_folder(
                        &engine,
                        &self.working_directory.join(&source),
                        params.name.as_deref(),
                        &params.secrets,
                        &|_| {},
                    )
                    .await?;
                    if !params.command.is_empty() {
                        let computer = engine
                            .computer(made["name"].as_str().unwrap_or_default())
                            .await?;
                        let (mut out, mut err) = (Vec::new(), Vec::new());
                        let ran = fanout::exec_streaming(
                            &engine,
                            &computer,
                            &params.command,
                            made["guestPath"].as_str(),
                            &|_| {},
                            |is_err, bytes| {
                                if is_err { &mut err } else { &mut out }.extend_from_slice(bytes)
                            },
                        )
                        .await?;
                        made["exitCode"] = json!(ran.exit_code);
                        made["stdout"] = json!(String::from_utf8_lossy(&out));
                        made["stderr"] = json!(String::from_utf8_lossy(&err));
                    }
                    return Ok(made);
                }
                let args = ForkArgs {
                    source,
                    name: params.name,
                    checkpoint,
                    secrets: params.secrets,
                    recipe_name: None,
                    no_start: params.no_start,
                };
                if params.count.is_some() || !params.command.is_empty() {
                    let many = ManyArgs {
                        fork: args,
                        count: params.count.unwrap_or(1),
                        command: params.command,
                        cwd: None,
                        steps: StepSettings::new(params.step_checkpoints, None),
                    };
                    return fanout::fork_many(&engine, &many, &|_| {}, &|_, _, _| {}).await;
                }
                ops::fork(&engine, &args, &|_| {}).await
            }
            .await,
        )
    }

    #[tool(description = "Start a stopped computer.")]
    async fn start(
        &self,
        Parameters(StartStopParams { computer }): Parameters<StartStopParams>,
    ) -> Result<String, String> {
        show(async { ops::start(&self.engine().await?, &computer, &|_| {}).await }.await)
    }

    #[tool(description = "Stop a running computer. Its disk is kept; `start` brings it back.")]
    async fn stop(
        &self,
        Parameters(StartStopParams { computer }): Parameters<StartStopParams>,
    ) -> Result<String, String> {
        show(async { ops::stop(&self.engine().await?, &computer, &|_| {}).await }.await)
    }

    #[tool(
        description = "Merge a fork's changes into the computer it came from (or `into`); inside a computer the person is asked first. A refusal with reason `choices-needed` lists each conflict with an id and its allowed answers (`fork`, `main`, or `branch` for a repository): run it again with `choices` mapping ids to answers, and `undo` takes a merge back."
    )]
    async fn merge(&self, Parameters(params): Parameters<MergeParams>) -> Result<String, String> {
        show(
            async {
                let engine = self.engine_asking(params.wait_secs, params.reason).await?;
                let mode = if params.request {
                    flows::Mode::Request
                } else {
                    flows::Mode::Merge
                };
                flows::run(
                    &engine,
                    &params.fork,
                    params.into.as_deref(),
                    MergeOptions {
                        dry_run: params.dry_run,
                        data_ok: params.data_ok,
                        reviewed: params.reviewed,
                        choices: answers(&params.choices)?,
                        skip: params.skip,
                    },
                    mode,
                    &|_| {},
                )
                .await
            }
            .await,
        )
    }

    #[tool(
        description = "Replace main with a fork: the fork takes main's name and settings and the old main is kept aside. It restarts main, so the person is always asked; `undo` swaps back."
    )]
    async fn replace_main(
        &self,
        Parameters(params): Parameters<ReplaceMainParams>,
    ) -> Result<String, String> {
        show(
            async {
                let engine = self.engine_asking(params.wait_secs, params.reason).await?;
                if !params.dry_run {
                    // The computers swap roles: their driver sessions end first.
                    self.close_sessions(|(on, _)| *on == engine.device().map(str::to_owned))
                        .await;
                }
                let mut value = ops::replace_main(
                    &engine,
                    &params.fork,
                    params.into.as_deref(),
                    params.dry_run,
                    &|_| {},
                )
                .await?;
                if let Ok(home) = access::home_dir() {
                    access::ssh_after_change(&engine, &mut value, &home, build_channel()).await;
                }
                Ok(value)
            }
            .await,
        )
    }

    #[tool(
        description = "List the review requests recorded with `merge` and `request`: each with its fork's name and state, the computer it would be merged into and how many changes each bucket held. Defaults to the open ones."
    )]
    async fn requests_list(
        &self,
        Parameters(params): Parameters<RequestsListParams>,
    ) -> Result<String, String> {
        show(
            async {
                let status = params
                    .status
                    .as_deref()
                    .map(review::parse_filter)
                    .transpose()?;
                let engine = self.engine_on(params.device.as_deref()).await?;
                if params.all_devices {
                    review::list_all(&engine, status).await
                } else {
                    review::list(&engine, status).await
                }
            }
            .await,
        )
    }

    #[tool(
        description = "Show a review request: a fresh diff of its fork by bucket, the credentials the fork used, and `stale: true` when the fork changed since the request was made (a merge then refuses)."
    )]
    async fn request_show(
        &self,
        Parameters(RequestIdParam { id, device }): Parameters<RequestIdParam>,
    ) -> Result<String, String> {
        show(
            async {
                let engine = self.engine_on(device.as_deref()).await?;
                review::show(&engine, &id, &|_| {}).await
            }
            .await,
        )
    }

    #[tool(
        description = "Merge a review request's fork into the computer it would be merged into, pinned to the digest the request was made from. Returns status merged or refused; a refusal leaves the request open and the target unchanged."
    )]
    async fn request_merge(
        &self,
        Parameters(params): Parameters<RequestMergeParams>,
    ) -> Result<String, String> {
        show(
            async {
                let engine = self.engine_asking(params.wait_secs, params.reason).await?;
                review::merge(
                    &engine,
                    &params.id,
                    params.data_ok,
                    answers(&params.choices)?,
                    params.skip,
                    &|_| {},
                )
                .await
            }
            .await,
        )
    }

    #[tool(
        description = "Decline a review request: drop its fork without merging. main and the computer the changes would be merged into are never dropped."
    )]
    async fn request_decline(
        &self,
        Parameters(RequestOnlyParam { id }): Parameters<RequestOnlyParam>,
    ) -> Result<String, String> {
        show(async { review::decline(&self.engine().await?, &id, &|_| {}).await }.await)
    }

    #[tool(
        description = "Start a review request's fork if it is stopped and return how to reach it. Only on the device."
    )]
    async fn request_open(
        &self,
        Parameters(RequestOnlyParam { id }): Parameters<RequestOnlyParam>,
    ) -> Result<String, String> {
        show(async { review::open(&self.engine().await?, &id, &|_| {}).await }.await)
    }

    #[tool(
        description = "Show what changed in a computer since it was forked, by bucket (code, packages, settings and files, databases, other). Returns the diff and its readable text. Look at it before you merge."
    )]
    async fn diff(&self, Parameters(params): Parameters<DiffParams>) -> Result<String, String> {
        show(
            async {
                let args = DiffArgs {
                    computer: params.computer.clone(),
                    max_lines: params.max_lines,
                    no_baseline: params.no_baseline,
                    compute_baseline: false,
                };
                let engine = self.engine_on(params.device.as_deref()).await?;
                ops::diff(&engine, &args, &|_| {}).await
            }
            .await,
        )
    }

    #[tool(
        description = "Take back the latest merge or replace_main of a computer: it is put back to the checkpoint taken before. The person is asked first when you are inside a computer."
    )]
    async fn undo(&self, Parameters(params): Parameters<UndoParams>) -> Result<String, String> {
        show(
            async {
                let engine = self.engine_asking(params.wait_secs, params.reason).await?;
                let mut value =
                    flows::undo(&engine, &params.computer, &self.working_directory, &|_| {})
                        .await?;
                if let Ok(home) = access::home_dir() {
                    access::ssh_after_change(&engine, &mut value, &home, build_channel()).await;
                }
                Ok(value)
            }
            .await,
        )
    }

    #[tool(
        description = "Finish the setup of a macOS computer that stopped before it was ready, and wait until it ends."
    )]
    async fn retry_setup(
        &self,
        Parameters(ComputerParam { computer }): Parameters<ComputerParam>,
    ) -> Result<String, String> {
        show(async { ops::retry_setup(&self.engine().await?, &computer, &|_| {}).await }.await)
    }

    #[tool(
        description = "Throw a computer away for good. Use it on forks you are done with. main cannot be dropped through this tool."
    )]
    async fn drop(
        &self,
        Parameters(ComputerParam { computer }): Parameters<ComputerParam>,
    ) -> Result<String, String> {
        show(
            async {
                let engine = self.engine().await?;
                if let Ok(found) = engine.computer(&computer).await {
                    let key = (engine.device().map(str::to_owned), found.id);
                    self.close_sessions(|held| *held == key).await;
                }
                ops::drop_computer(&engine, &computer, false, &|_| {}).await
            }
            .await,
        )
    }

    #[tool(
        description = "Run a command in a computer (at most 180 seconds). Returns exitCode, signal, stdout and stderr as text; bytes that are not UTF-8 are replaced."
    )]
    async fn exec(&self, Parameters(params): Parameters<ExecParams>) -> Result<String, String> {
        show(
            async {
                let args = ExecArgs {
                    computer: params.computer,
                    argv: params.command,
                    cwd: params.cwd,
                    timeout_secs: params.timeout_secs.unwrap_or(ops::MAX_EXEC_SECS),
                };
                let engine = self.engine().await?;
                let (output, step) = steps::run_exec(
                    &engine,
                    &args,
                    StepSettings::new(params.step_checkpoints, None),
                    &|_| {},
                )
                .await?;
                let mut value = output.to_json();
                value["step"] = step;
                Ok(value)
            }
            .await,
        )
    }

    #[tool(
        description = "List the steps of a computer: the commands run in it through Silo (`exec`, or `fork` with a command), numbered from 1, with exit code, times and the checkpoint taken after each when step checkpoints were on. Fork from one with `fork` and `fromStep`."
    )]
    async fn steps_list(
        &self,
        Parameters(DeviceComputerParam { computer, device }): Parameters<DeviceComputerParam>,
    ) -> Result<String, String> {
        show(
            async { steps::list(&self.engine_on(device.as_deref()).await?, &computer).await }.await,
        )
    }

    #[tool(
        description = "Save a checkpoint of a computer: a state you can fork from or `restore` later."
    )]
    async fn checkpoint(
        &self,
        Parameters(params): Parameters<CheckpointParams>,
    ) -> Result<String, String> {
        show(
            async {
                let kind = params
                    .kind
                    .as_deref()
                    .map(ops::parse_checkpoint_kind)
                    .transpose()?;
                ops::checkpoint(
                    &self.engine().await?,
                    &params.computer,
                    params.name.as_deref(),
                    kind,
                    &|_| {},
                )
                .await
            }
            .await,
        )
    }

    #[tool(description = "List a computer's checkpoints.")]
    async fn checkpoints(
        &self,
        Parameters(DeviceComputerParam { computer, device }): Parameters<DeviceComputerParam>,
    ) -> Result<String, String> {
        show(
            async { ops::checkpoints(&self.engine_on(device.as_deref()).await?, &computer).await }
                .await,
        )
    }

    #[tool(
        description = "Put a computer back to one of its checkpoints, given by id or name. What it did since is lost."
    )]
    async fn restore(
        &self,
        Parameters(params): Parameters<RestoreParams>,
    ) -> Result<String, String> {
        show(
            async {
                ops::restore(
                    &self.engine().await?,
                    &params.computer,
                    &params.checkpoint,
                    &|_| {},
                )
                .await
            }
            .await,
        )
    }

    #[tool(
        description = "List the ports published from a computer on the device, with the host port and URL each is reachable at. Forks publish the ports their source does."
    )]
    async fn ports(
        &self,
        Parameters(ComputerParam { computer }): Parameters<ComputerParam>,
    ) -> Result<String, String> {
        show(async { access::ports(&self.engine().await?, &computer).await }.await)
    }

    #[tool(
        description = "Publish a port of a computer on the device so it can be reached from outside; with `http` it is served as a website. Inside a computer the person is asked first."
    )]
    async fn publish(
        &self,
        Parameters(params): Parameters<PublishParams>,
    ) -> Result<String, String> {
        show(
            async {
                let engine = self.engine_asking(params.wait_secs, params.reason).await?;
                access::publish(&engine, &params.computer, params.port, params.http).await
            }
            .await,
        )
    }

    #[tool(description = "Stop publishing a port of a computer.")]
    async fn unpublish(
        &self,
        Parameters(params): Parameters<UnpublishParams>,
    ) -> Result<String, String> {
        show(
            async { access::unpublish(&self.engine().await?, &params.computer, params.port).await }
                .await,
        )
    }

    #[tool(description = "Make `ssh <computer>.silo` reach a computer. Only on the device.")]
    async fn ssh_name(
        &self,
        Parameters(ComputerParam { computer }): Parameters<ComputerParam>,
    ) -> Result<String, String> {
        show(
            async {
                let home = access::home_dir()?;
                access::ssh_setup(&self.engine().await?, &computer, &home, build_channel()).await
            }
            .await,
        )
    }

    #[tool(
        description = "Read or change what a computer may reach on the network (open, an allowlist of hosts, or closed). With no other field it only reads. Widening the policy from inside a computer asks the person; narrowing it does not."
    )]
    async fn network_policy(
        &self,
        Parameters(params): Parameters<NetworkPolicyParams>,
    ) -> Result<String, String> {
        show(
            async {
                use silo_api::methods::network::NetworkPolicyMode;
                let engine = self.engine_asking(params.wait_secs, params.reason).await?;
                let mode = match params.mode.as_deref() {
                    None => None,
                    Some("open") => Some(NetworkPolicyMode::Open),
                    Some("allowlist") => Some(NetworkPolicyMode::Allowlist),
                    Some("closed") => Some(NetworkPolicyMode::Closed),
                    Some(other) => {
                        return Err(CliError::new(format!(
                            "`{other}` is not a mode; use open, allowlist or closed."
                        )))
                    }
                };
                let change = access::PolicyChange {
                    mode,
                    allow: params.allow,
                    remove: params.remove,
                    presets: params.presets,
                };
                access::network_policy(&engine, &params.computer, &change).await
            }
            .await,
        )
    }

    #[tool(
        description = "List the coding agents a computer has (Claude Code, Codex) and whether each is installed, or install one with `install`."
    )]
    async fn agents(&self, Parameters(params): Parameters<AgentsParams>) -> Result<String, String> {
        show(
            async {
                let engine = self.engine().await?;
                match params.install.as_deref() {
                    None => ops::agents_list(&engine, &params.computer).await,
                    Some(name) => {
                        let agent = match name {
                            "claude-code" => silo_api::methods::agents::AgentId::ClaudeCode,
                            "codex" => silo_api::methods::agents::AgentId::Codex,
                            other => {
                                return Err(CliError::new(format!(
                                    "`{other}` is not an agent; use claude-code or codex."
                                )))
                            }
                        };
                        ops::agent_install(&engine, &params.computer, agent, &|_| {}).await
                    }
                }
            }
            .await,
        )
    }

    #[tool(
        description = "Check on requests you made that need the person's answer. `action` is `list` (default), `status` (one request by `id`), `wait` (up to `wait_secs` for the answer) or `note` (add your reason in `text`). A request is pending, allowed, denied, cancelled or expired; an allowed one says what ran."
    )]
    async fn approvals(
        &self,
        Parameters(params): Parameters<ApprovalsParams>,
    ) -> Result<String, String> {
        show(
            async {
                let engine = self.engine().await?;
                let id = || {
                    params
                        .id
                        .clone()
                        .ok_or_else(|| CliError::new("Name the request: `id`."))
                };
                match params.action.as_deref().unwrap_or("list") {
                    "list" => approvals::list_state(&engine, None).await,
                    "status" => approvals::show(&engine, &id()?).await,
                    "wait" => {
                        let wait = Duration::from_secs(
                            params
                                .wait_secs
                                .unwrap_or(DEFAULT_WAIT_SECS)
                                .clamp(0, MAX_WAIT_SECS),
                        );
                        approvals::wait(&engine, &id()?, wait, Duration::from_millis(1500)).await
                    }
                    "note" => {
                        let text = params
                            .text
                            .clone()
                            .ok_or_else(|| CliError::new("Give your reason in `text`."))?;
                        approvals::note(&engine, &id()?, &text).await
                    }
                    other => Err(CliError::new(format!(
                        "`{other}` is not an action; use list, status, wait or note."
                    ))),
                }
            }
            .await,
        )
    }

    /// One request to the computer's driver session. The host tools are for forks: `main` is
    /// refused, like `drop`.
    async fn session_request(
        &self,
        computer: &str,
        method: &str,
        params: Value,
    ) -> Result<Value, CliError> {
        self.session_call(computer, Some((method, params)), 0).await
    }

    /// A request (`Some`) or, with `None`, a screenshot of at most `max_side` pixels.
    async fn session_call(
        &self,
        computer: &str,
        request: Option<(&str, Value)>,
        max_side: u32,
    ) -> Result<Value, CliError> {
        let engine = self.engine().await?;
        let found = engine.computer(computer).await?;
        crate::gui::ensure_fork(&engine, &found).await?;
        crate::gui::ensure_desktop(&found)?;
        let key = (engine.device().map(str::to_owned), found.id.clone());
        let session = self
            .sessions
            .lock()
            .unwrap()
            .entry(key)
            .or_default()
            .clone();
        let mut session = session.lock().await;
        let timeout = Duration::from_secs(60);
        match request {
            Some((method, params)) => {
                session
                    .request(&engine, &found, method, params, timeout)
                    .await
            }
            None => session.screenshot(&engine, &found, max_side, timeout).await,
        }
    }

    /// Ends the driver sessions this server holds whose (device, computer id) `which` picks,
    /// each on the device its computer is on.
    async fn close_sessions(&self, which: impl Fn(&SessionKey) -> bool) {
        let held: Vec<(SessionKey, _)> = {
            let mut sessions = self.sessions.lock().unwrap();
            let keys: Vec<SessionKey> = sessions.keys().filter(|key| which(key)).cloned().collect();
            keys.into_iter()
                .filter_map(|key| sessions.remove(&key).map(|session| (key, session)))
                .collect()
        };
        if held.is_empty() {
            return;
        }
        if let Ok(engine) = self.engine().await {
            for ((device, _), session) in held {
                session.lock().await.close(&engine.on_device(device)).await;
            }
        }
    }

    /// Runs one driver tool in the computer and shapes its answer for the agent.
    async fn drive(
        &self,
        computer: &str,
        tool: &str,
        arguments: Value,
    ) -> Result<CallToolResult, String> {
        // Screenshots come back through a file, never on the driver's output.
        let result = if tool == "get_desktop_state" {
            let side = arguments["max_image_dimension"]
                .as_u64()
                .map_or(crate::usetools::DEFAULT_MAX_SIDE, |side| {
                    side.min(8192) as u32
                });
            self.session_call(computer, None, side).await
        } else {
            self.session_request(
                computer,
                "tools/call",
                serde_json::json!({"name": tool, "arguments": arguments}),
            )
            .await
        };
        result
            .map(|result| drive_result(crate::usetools::answer_of(&result)))
            .map_err(|error| error.message)
    }

    async fn act(
        &self,
        computer: &str,
        action: crate::usetools::Action,
    ) -> Result<CallToolResult, String> {
        let (tool, arguments) = crate::usetools::driver_call(&action);
        self.drive(computer, tool, arguments).await
    }

    #[tool(
        description = "Fork a computer (default your own, or main on the device), start the fork and wait until its desktop and computer-use driver are ready; the computer_use_* tools then drive it. The fork keeps asking before its own agents drive the desktop."
    )]
    async fn fork_gui(
        &self,
        Parameters(params): Parameters<ForkGuiParams>,
    ) -> Result<String, String> {
        show(
            async {
                let engine = self.engine_asking(None, None).await?;
                let source = match params.source {
                    Some(source) => source,
                    None => self.default_source(&engine).await?,
                };
                if folder::is_folder(&source) {
                    return Err(CliError::new(
                        "fork_gui forks a computer; a folder is copied into a Linux fork with fork.",
                    ));
                }
                let args = crate::gui::GuiArgs {
                    fork: ForkArgs {
                        source,
                        name: params.name,
                        ..ForkArgs::default()
                    },
                    approval: None,
                    timeout: std::time::Duration::from_secs(
                        params
                            .timeout_secs
                            .unwrap_or(crate::gui::DEFAULT_TIMEOUT_SECS)
                            .max(1),
                    ),
                };
                crate::gui::fork_gui(&engine, &args, &|_| {}).await
            }
            .await,
        )
    }

    #[tool(
        description = "Say whether a computer's desktop and computer-use driver are ready now, without waiting: ready, what it is waiting for, and the computer-use state and approval mode."
    )]
    async fn computer_use_ready(
        &self,
        Parameters(ComputerParam { computer }): Parameters<ComputerParam>,
    ) -> Result<String, String> {
        show(
            async {
                let engine = self.engine().await?;
                let found = engine.computer(&computer).await?;
                crate::gui::ready_status(&engine, &found).await
            }
            .await,
        )
    }

    #[tool(
        description = "Take a screenshot of a fork's desktop. The driver scales it so its longest side is at most max_side (default 1280); give coordinates to the other computer_use tools in the pixels of the latest screenshot and the driver maps them to the screen. Not for main."
    )]
    async fn computer_use_screenshot(
        &self,
        Parameters(params): Parameters<ScreenshotParams>,
    ) -> Result<CallToolResult, String> {
        let max_side = params.max_side.unwrap_or(crate::usetools::DEFAULT_MAX_SIDE);
        self.drive(
            &params.computer,
            "get_desktop_state",
            serde_json::json!({"max_image_dimension": max_side}),
        )
        .await
    }

    #[tool(description = "Click (or double-click) at a point of the latest screenshot of a fork.")]
    async fn computer_use_click(
        &self,
        Parameters(p): Parameters<ClickParams>,
    ) -> Result<CallToolResult, String> {
        let action = crate::usetools::Action::Click {
            x: p.x,
            y: p.y,
            button: p.button.unwrap_or_else(|| "left".into()),
            double: p.double,
        };
        self.act(&p.computer, action).await
    }

    #[tool(description = "Type text into the focused window of a fork.")]
    async fn computer_use_type(
        &self,
        Parameters(p): Parameters<TypeParams>,
    ) -> Result<CallToolResult, String> {
        self.act(&p.computer, crate::usetools::Action::Type { text: p.text })
            .await
    }

    #[tool(description = "Press one key, with optional modifiers, in a fork.")]
    async fn computer_use_key(
        &self,
        Parameters(p): Parameters<KeyParams>,
    ) -> Result<CallToolResult, String> {
        self.act(
            &p.computer,
            crate::usetools::Action::Key {
                key: p.key,
                modifiers: p.modifiers,
            },
        )
        .await
    }

    #[tool(
        description = "Press a key chord of two or more keys, like [\"ctrl\", \"l\"], in a fork."
    )]
    async fn computer_use_hotkey(
        &self,
        Parameters(p): Parameters<HotkeyParams>,
    ) -> Result<CallToolResult, String> {
        self.act(
            &p.computer,
            crate::usetools::Action::Hotkey { keys: p.keys },
        )
        .await
    }

    #[tool(description = "Scroll at a point of the latest screenshot of a fork.")]
    async fn computer_use_scroll(
        &self,
        Parameters(p): Parameters<ScrollParams>,
    ) -> Result<CallToolResult, String> {
        self.act(
            &p.computer,
            crate::usetools::Action::Scroll {
                x: p.x,
                y: p.y,
                direction: p.direction,
                amount: p.amount.unwrap_or(3),
                by: p.by,
            },
        )
        .await
    }

    #[tool(description = "Move the pointer to a point of the latest screenshot of a fork.")]
    async fn computer_use_move(
        &self,
        Parameters(p): Parameters<MoveParams>,
    ) -> Result<CallToolResult, String> {
        self.act(
            &p.computer,
            crate::usetools::Action::Move { x: p.x, y: p.y },
        )
        .await
    }

    #[tool(description = "Drag from one point of the latest screenshot of a fork to another.")]
    async fn computer_use_drag(
        &self,
        Parameters(p): Parameters<DragParams>,
    ) -> Result<CallToolResult, String> {
        let action = crate::usetools::Action::Drag {
            from: (p.from_x, p.from_y),
            to: (p.to_x, p.to_y),
            button: p.button,
        };
        self.act(&p.computer, action).await
    }

    #[tool(
        description = "Wait up to 30 seconds (a pause on the host; nothing runs in the computer)."
    )]
    async fn computer_use_wait(
        &self,
        Parameters(p): Parameters<WaitParams>,
    ) -> Result<String, String> {
        tokio::time::sleep(Duration::from_secs_f64(p.seconds.clamp(0.0, 30.0))).await;
        Ok(format!("Waited {} seconds.", p.seconds.clamp(0.0, 30.0)))
    }

    #[tool(description = "List the tools of a fork's Cua Driver with their arguments.")]
    async fn computer_use_tools(
        &self,
        Parameters(ComputerParam { computer }): Parameters<ComputerParam>,
    ) -> Result<String, String> {
        show(
            self.session_request(&computer, "tools/list", serde_json::json!({}))
                .await,
        )
    }

    #[tool(
        description = "Run any tool of a fork's Cua Driver by name with its own arguments. Coordinates are in the pixels of the latest get_desktop_state image."
    )]
    async fn computer_use_call(
        &self,
        Parameters(p): Parameters<DriverCallParams>,
    ) -> Result<CallToolResult, String> {
        self.drive(
            &p.computer,
            &p.tool,
            p.arguments.unwrap_or_else(|| serde_json::json!({})),
        )
        .await
    }
}

#[tool_handler(router = self.tool_router, instructions = "Silo gives you computers: isolated machines you can run commands in, \
copy and throw away. If you run inside one, it is your own, and the person who set Silo up owns \
the device it runs on. Fork a computer to try something without risking it, work in the fork, \
look at what changed with `diff`, and `merge` it back; a merge, a wider network, a published \
port, a secret or a new repository needs the person's answer in the Silo app, so the tool waits \
a little and then tells you it is waiting and gives a request id to check later with \
`approvals`. `replace_main` swaps a fork in for main and restarts it. `undo` takes a merge \
back. Check `resources` before making many computers, and drop the ones you no longer need.")]
impl ServerHandler for SiloMcp {
    async fn initialize(
        &self,
        request: rmcp::model::InitializeRequestParams,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<rmcp::model::InitializeResult, rmcp::ErrorData> {
        context.peer.set_peer_info(request.clone());
        self.negotiate_initialize(&request)
    }
}

/// Serves the tools on stdin and stdout until the client closes them.
pub async fn serve(transport: Transport, working_directory: PathBuf) -> Result<(), CliError> {
    let server = SiloMcp::for_transport(transport, working_directory);
    let service = server
        .clone()
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|error| CliError::new(format!("The MCP server could not start: {error}")))?;
    let stopped = service.waiting().await;
    server.close_sessions(|_| true).await;
    stopped.map_err(|error| CliError::new(format!("The MCP server stopped: {error}")))?;
    Ok(())
}

#[cfg(test)]
mod tool_tests {
    use super::*;
    use crate::endpoint::Endpoint;

    fn channel() -> Transport {
        Transport::Channel(Endpoint {
            host: "127.0.0.1".into(),
            port: 9,
            token: "silo_gc_x".into(),
            computer_id: "c1".into(),
        })
    }

    fn names(server: &SiloMcp) -> Vec<String> {
        let mut names: Vec<String> = server
            .tool_router
            .list_all()
            .into_iter()
            .map(|tool| tool.name.to_string())
            .collect();
        names.sort();
        names
    }

    /// What an agent can do, on the device and inside a computer.
    const SHARED: &[&str] = &[
        "list",
        "status",
        "tree",
        "resources",
        "access",
        "recipe_list",
        "create",
        "fork",
        "start",
        "stop",
        "exec",
        "diff",
        "merge",
        "replace_main",
        "undo",
        "drop",
        "checkpoint",
        "checkpoints",
        "restore",
        "steps_list",
        "ports",
        "publish",
        "unpublish",
        "network_policy",
        "secrets_list",
        "agents",
        "approvals",
        "requests_list",
        "request_show",
        "request_merge",
        "request_decline",
        "retry_setup",
        "fork_gui",
        "computer_use_ready",
        "computer_use_screenshot",
        "computer_use_click",
        "computer_use_type",
        "computer_use_key",
        "computer_use_hotkey",
        "computer_use_scroll",
        "computer_use_move",
        "computer_use_drag",
        "computer_use_wait",
        "computer_use_tools",
        "computer_use_call",
    ];

    #[test]
    fn the_device_offers_the_shared_tools_and_the_device_only_ones() {
        let on_device = SiloMcp::new("/nowhere.sock".into(), std::env::temp_dir());
        let offered = names(&on_device);
        for name in SHARED.iter().chain(DEVICE_ONLY_TOOLS) {
            assert!(offered.iter().any(|offered| offered == name), "{name}");
        }
        assert_eq!(offered.len(), SHARED.len() + DEVICE_ONLY_TOOLS.len(), "{offered:?}");
    }

    #[test]
    fn inside_a_computer_the_device_only_tools_are_not_listed() {
        let inside = SiloMcp::for_transport(channel(), std::env::temp_dir());
        let offered = names(&inside);
        for name in SHARED {
            assert!(offered.iter().any(|offered| offered == name), "{name}");
        }
        for name in DEVICE_ONLY_TOOLS {
            assert!(!offered.iter().any(|offered| offered == name), "{name}");
        }
        assert_eq!(offered.len(), SHARED.len(), "{offered:?}");
    }

    #[test]
    fn the_set_aside_tools_and_the_old_words_are_gone() {
        let on_device = SiloMcp::new("/nowhere.sock".into(), std::env::temp_dir());
        for name in names(&on_device) {
            for old in ["review", "fold", "keep", "throw_away", "promote"] {
                assert!(!name.contains(old), "{name}");
            }
        }
    }

    #[test]
    fn every_description_is_one_or_two_plain_sentences_without_the_old_words() {
        let on_device = SiloMcp::new("/nowhere.sock".into(), std::env::temp_dir());
        for tool in on_device.tool_router.list_all() {
            let text = tool.description.as_deref().unwrap_or_default().to_string();
            assert!(!text.is_empty(), "{}", tool.name);
            // A description says what the tool does and when to use it, in a few plain sentences.
            let sentences = text.matches(". ").count() + 1;
            assert!(sentences <= 3, "{}: {sentences} sentences: {text}", tool.name);
            let lower = text.to_lowercase();
            for old in ["fold", "promote", ".silo/", "auto_keep", "silo.yaml"] {
                assert!(!lower.contains(old), "{}: {text}", tool.name);
            }
        }
    }

    #[test]
    fn inside_a_computer_another_device_and_a_folder_are_refused() {
        let inside = SiloMcp::for_transport(channel(), std::env::temp_dir());
        assert!(inside.on_the_device_only(Some("laptop"), None).is_err());
        assert!(inside.on_the_device_only(Some("local"), None).is_ok());
        let path = "./repo".to_owned();
        let refused = inside.on_the_device_only(None, Some(&path)).unwrap_err();
        assert!(refused.message.contains("device"), "{}", refused.message);
        let on_device = SiloMcp::new("/nowhere.sock".into(), std::env::temp_dir());
        assert!(on_device
            .on_the_device_only(Some("laptop"), Some(&path))
            .is_ok());
    }

    #[test]
    fn a_tool_that_waits_for_the_owner_gives_up_before_a_clients_time_out() {
        assert!(DEFAULT_WAIT_SECS < MAX_WAIT_SECS);
        assert!(MAX_WAIT_SECS < 120);
    }
}
