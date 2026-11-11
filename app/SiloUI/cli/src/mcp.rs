//! `silo mcp`: the commands as MCP tools over stdio. Every tool makes the same engine calls
//! as its command and answers with the same JSON as `--json`.

use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};

use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, ContentBlock},
    schemars, tool, tool_handler, tool_router, ServerHandler, ServiceExt,
};
use serde::Deserialize;
use serde_json::Value;

use crate::{
    access,
    config::Project,
    endpoint::Transport,
    engine::{build_channel, Engine},
    error::CliError,
    fanout::{self, ManyArgs},
    flows, folder,
    ops::{self, DiffArgs, ExecArgs, ForkArgs},
    review, steps,
};

/// The tools a server inside a computer does not offer.
pub const DEVICE_ONLY_TOOLS: &[&str] = &["review", "ssh_name", "request_open"];

#[derive(Clone)]
pub struct SiloMcp {
    transport: Arc<Transport>,
    working_directory: PathBuf,
    engine: Arc<tokio::sync::Mutex<Option<Engine>>>,
    /// The client's name from `initialize`, lowercase: which `agents:` entry applies.
    agent: Arc<std::sync::Mutex<Option<String>>>,
    /// Set when `--agent` or `SILO_AGENT` named the agent: the client's name is then ignored.
    agent_named: bool,
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
    /// The connected device (name or id) the computer is on; default this device. A tool that
    /// changes something there needs it in the agent's `devices:` entry in `.silo/silo.yaml`.
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
pub struct PublishParams {
    /// The computer's name.
    pub computer: String,
    /// The port inside the computer.
    pub port: u16,
    /// Serve it as a website.
    #[serde(default)]
    pub http: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ForkParams {
    /// The computer to copy, usually `main` (the default), or a path to a git repository on
    /// this machine (`./repo`) to copy into a new Linux fork. Ignored when `recipe` is set.
    #[serde(default)]
    pub source: Option<String>,
    /// Make this many forks of the same checkpoint at once (default 1), each started. The
    /// `name` becomes a prefix.
    pub count: Option<u32>,
    /// A command to run in each fork once it is started, with no time limit. The result has
    /// each fork's exit code and output; the fork is left for review.
    #[serde(default)]
    pub command: Vec<String>,
    /// Fork from this recipe's cached build (a built-in such as linux-dev, or a file in
    /// `.silo/recipes/`), building it first when needed.
    pub recipe: Option<String>,
    /// The fork's name; defaults to the source name plus a short id.
    pub name: Option<String>,
    /// Fork from this checkpoint id instead of the current state.
    pub checkpoint: Option<String>,
    /// Fork from the checkpoint taken after this step number of the computer (see
    /// `steps_list`); it needs step checkpoints turned on when the step ran.
    #[serde(rename = "fromStep", alias = "from_step")]
    pub from_step: Option<u32>,
    /// Silo-managed secrets the fork may use; each must be in fork_secrets in `.silo/silo.yaml`.
    #[serde(default)]
    pub secrets: Vec<String>,
    /// Path of a recipe whose quiet and scrub lists apply to the fork.
    pub recipe_file: Option<String>,
    /// A connected device (name or id) to run this on instead of this one.
    pub device: Option<String>,
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
    /// The device the request is on. Needed for a request on another device, and the agent's
    /// `devices:` entry in `.silo/silo.yaml` must list it.
    pub device: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RequestKeepParams {
    /// The request's id, such as fr-ab12cd34.
    pub id: String,
    /// Carry database changes even though no migrate step covers them.
    #[serde(default)]
    pub data_ok: bool,
    /// Path of a recipe whose keep: section applies.
    pub recipe_file: Option<String>,
    /// The device the request is on. Needed for a request on another device, and the agent's
    /// `devices:` entry in `.silo/silo.yaml` must list it.
    pub device: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReviewParams {
    /// A computer, or a review request id such as fr-ab12cd34.
    pub target: String,
    /// general, packages, code or all (default).
    pub lens: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpParams {
    /// The recipe `main` is made from when it does not exist (default linux-dev).
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
    /// The connected device (name or id) the computer is on; default this device. The agent's
    /// `devices:` entry in `.silo/silo.yaml` must list it.
    pub device: Option<String>,
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
pub struct FoldParams {
    /// The fork whose changes are kept.
    pub fork: String,
    /// The computer to keep into; defaults to the one the fork was made from.
    pub into: Option<String>,
    /// Report what would be kept and what would stop it, changing nothing.
    #[serde(default)]
    pub dry_run: bool,
    /// Make the fork main instead of keeping its changes: it takes the name, settings and
    /// published ports of `into` (default: its source), which is archived. `undo` swaps back.
    #[serde(default)]
    pub promote: bool,
    /// Carry database changes even though no migrate step covers them.
    #[serde(default)]
    pub data_ok: bool,
    /// The reviewDigest from `diff`; the keep refuses if the fork changed since.
    pub reviewed: Option<String>,
    /// Path of a recipe whose keep: section applies.
    pub recipe_file: Option<String>,
    /// Record a review request for the fork instead of keeping it; returns the request. When
    /// an auto_keep rule of the project covers every change, the changes are kept instead.
    #[serde(default)]
    pub request: bool,
    /// Do not append kept packages and settings files to the recipe the computer came from.
    #[serde(default)]
    pub no_write_back: bool,
    /// A connected device (name or id) to run this on instead of this one.
    pub device: Option<String>,
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
pub struct ScreenshotParams {
    /// The computer's name.
    pub computer: String,
    /// The longest side of the picture in pixels (default 1280); larger screens are scaled down.
    pub max_side: Option<u32>,
    /// The connected device (name or id) the computer is on; default this device. The agent's
    /// `devices:` entry in `.silo/silo.yaml` must list it.
    pub device: Option<String>,
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
    /// The connected device (name or id) the computer is on; default this device. The agent's
    /// `devices:` entry in `.silo/silo.yaml` must list it.
    pub device: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TypeParams {
    /// The computer's name.
    pub computer: String,
    /// The text to type into the focused window.
    pub text: String,
    /// The connected device (name or id) the computer is on; default this device. The agent's
    /// `devices:` entry in `.silo/silo.yaml` must list it.
    pub device: Option<String>,
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
    /// The connected device (name or id) the computer is on; default this device. The agent's
    /// `devices:` entry in `.silo/silo.yaml` must list it.
    pub device: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HotkeyParams {
    /// The computer's name.
    pub computer: String,
    /// A chord of at least two keys, like `["ctrl", "l"]`.
    pub keys: Vec<String>,
    /// The connected device (name or id) the computer is on; default this device. The agent's
    /// `devices:` entry in `.silo/silo.yaml` must list it.
    pub device: Option<String>,
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
    /// The connected device (name or id) the computer is on; default this device. The agent's
    /// `devices:` entry in `.silo/silo.yaml` must list it.
    pub device: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MoveParams {
    /// The computer's name.
    pub computer: String,
    pub x: f64,
    pub y: f64,
    /// The connected device (name or id) the computer is on; default this device. The agent's
    /// `devices:` entry in `.silo/silo.yaml` must list it.
    pub device: Option<String>,
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
    /// The connected device (name or id) the computer is on; default this device. The agent's
    /// `devices:` entry in `.silo/silo.yaml` must list it.
    pub device: Option<String>,
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
    /// The connected device (name or id) the computer is on; default this device. The agent's
    /// `devices:` entry in `.silo/silo.yaml` must list it.
    pub device: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ForkGuiParams {
    /// The computer to copy (default `main`).
    pub source: Option<String>,
    /// The fork's name.
    pub name: Option<String>,
    /// `ask` (default) or `auto`: the approval for agents running inside the fork. `auto` needs
    /// computer_use_auto: true in .silo/silo.yaml.
    pub computer_use: Option<String>,
    /// How long to wait for the desktop and the driver (default 300).
    pub timeout_secs: Option<u64>,
    /// A connected device (name or id) to fork on instead of this one. The agent's `devices:`
    /// entry in `.silo/silo.yaml` must list it.
    pub device: Option<String>,
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

fn show(result: Result<Value, CliError>) -> Result<String, String> {
    match result {
        Ok(value) => Ok(serde_json::to_string_pretty(&value).unwrap_or_default()),
        Err(error) => Err(error.message),
    }
}

#[tool_router]
impl SiloMcp {
    pub fn new(socket: PathBuf, working_directory: PathBuf) -> Self {
        Self::for_transport(Transport::Socket(socket), working_directory)
    }

    /// The server for `transport`. Inside a computer (its channel), the tools that only make
    /// sense on the device are not offered: the hosted reviewer, SSH names and opening a
    /// review request's fork from the device.
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
            agent: Arc::new(std::sync::Mutex::new(None)),
            agent_named: false,
            sessions: Arc::new(std::sync::Mutex::new(HashMap::new())),
            tool_router: Self::tool_router(),
        }
        .with_agent(std::env::var("SILO_AGENT").ok())
    }

    /// Names the agent (from `--agent` or `SILO_AGENT`); the client's own name is then not used.
    pub fn with_agent(mut self, agent: Option<String>) -> Self {
        if let Some(name) = crate::config::normalize_agent(agent.as_deref()) {
            self.agent = Arc::new(std::sync::Mutex::new(Some(name)));
            self.agent_named = true;
        }
        self
    }

    /// The agent whose settings apply: `--agent` or `SILO_AGENT`, else the connected client's
    /// name (self-reported: its entry then narrows `recipes` and `fork_secrets` but sets no `fold` or
    /// `auto_keep`).
    pub fn agent(&self) -> Option<String> {
        self.agent.lock().unwrap().clone()
    }

    /// The one connection this server keeps, opened again when it has ended.
    /// The engine acting on `device` (a name or id), or on this device when none is given.
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

    /// The engine for a change on `device`. Another device is acted on only when the call names
    /// it, and only when the agent's `devices:` entry in `.silo/silo.yaml` lists it.
    async fn engine_acting_on(&self, device: Option<&str>) -> Result<Engine, CliError> {
        let engine = self.engine_on(device).await?;
        let Some(id) = engine.device().map(str::to_owned) else {
            return Ok(engine);
        };
        let project = Project::discover(&self.working_directory);
        let settings = project.load_settings_as(self.agent().as_deref(), self.agent_named)?;
        let allowed = settings
            .map(|settings| settings.devices)
            .unwrap_or_default();
        let name = engine
            .devices()
            .await?
            .into_iter()
            .find(|(known, _)| *known == id)
            .map(|(_, name)| name)
            .unwrap_or_default();
        if allowed
            .iter()
            .any(|entry| entry.eq_ignore_ascii_case(&name) || *entry == id)
        {
            Ok(engine)
        } else {
            Err(CliError::new(format!(
                "This agent may not act on {name}. List it under devices: in the agent's entry of agents: in .silo/silo.yaml."
            )))
        }
    }

    async fn engine(&self) -> Result<Engine, CliError> {
        let mut held = self.engine.lock().await;
        if let Some(engine) = held.as_ref().filter(|engine| !engine.is_closed()) {
            return Ok(engine.clone());
        }
        let engine = Engine::connect_to(&self.transport).await?;
        *held = Some(engine.clone());
        Ok(engine)
    }

    #[tool(description = "List computers and their state.")]
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

    #[tool(description = "Show one computer: state, size and checkpoints.")]
    async fn status(
        &self,
        Parameters(DeviceComputerParam { computer, device }): Parameters<DeviceComputerParam>,
    ) -> Result<String, String> {
        show(
            async { ops::status(&self.engine_on(device.as_deref()).await?, &computer).await }.await,
        )
    }

    #[tool(
        description = "List the Silo secrets by name with the computers they are assigned to. Values are never returned, and none can be set through MCP."
    )]
    async fn secrets_list(&self) -> Result<String, String> {
        show(async { ops::secrets_list(&self.engine().await?).await }.await)
    }

    #[tool(
        description = "Start `main` (made from a recipe the first time, default linux-dev). A fork of `main` starts from it as it is when the fork is made."
    )]
    async fn up(&self, Parameters(params): Parameters<UpParams>) -> Result<String, String> {
        show(
            async {
                let project = Project::discover(&self.working_directory);
                let name = params
                    .recipe
                    .unwrap_or_else(|| crate::config::DEFAULT_RECIPE.to_owned());
                let recipe = project.named_recipe(&name)?;
                let engine = self.engine().await?;
                let home = access::home_dir();
                let ssh = home
                    .as_ref()
                    .ok()
                    .map(|home| (home.as_path(), build_channel()));
                let mut value = ops::up(&engine, &name, &recipe, ssh, &|_| {}).await?;
                if let Err(error) = home {
                    value["ssh"] = serde_json::json!({ "error": error.message });
                }
                Ok(value)
            }
            .await,
        )
    }

    #[tool(
        description = "List the recipes (built-in and in .silo/recipes/) and whether each is built. Fork from one with `fork` and its `recipe` parameter."
    )]
    async fn recipe_list(&self) -> Result<String, String> {
        show(
            async {
                let project = Project::discover(&self.working_directory);
                ops::recipe_list(&self.engine().await?, &project).await
            }
            .await,
        )
    }

    #[tool(
        description = "Copy a computer into a new fork for a task and wait until it is ready. `count` makes several forks of one checkpoint at once and starts them; each result has its name and URLs. `command` runs in each fork and the result has its exit code and output. A `source` that is a path to a git repository copies it into a new Linux fork; `fold` then brings only its commits back."
    )]
    async fn fork(&self, Parameters(params): Parameters<ForkParams>) -> Result<String, String> {
        show(
            async {
                let project = Project::discover(&self.working_directory);
                let settings = project.load_settings_as(self.agent().as_deref(), self.agent_named)?;
                let explicit = params
                    .recipe_file
                    .as_deref()
                    .map(|path| self.working_directory.join(path));
                let source = params.source.unwrap_or_else(|| ops::MAIN.to_owned());
                self.on_the_device_only(None, Some(&source))?;
                if params.device.is_some() && folder::is_folder(&source) {
                    return Err(CliError::new(
                        "A folder is on this device, so `device` cannot be used with it.",
                    ));
                }
                let engine = self.engine_acting_on(params.device.as_deref()).await?;
                let checkpoint = match params.from_step {
                    Some(number) => {
                        if params.recipe.is_some()
                            || params.checkpoint.is_some()
                            || folder::is_folder(&source)
                        {
                            return Err(CliError::new(
                                "fromStep names a step of a computer; it cannot be combined with recipe, checkpoint or a folder.",
                            ));
                        }
                        Some(steps::checkpoint_for(&engine, &source, number).await?)
                    }
                    None => params.checkpoint.clone(),
                };
                if params.recipe.is_none() && folder::is_folder(&source) {
                    if params.count.unwrap_or(1) != 1 {
                        return Err(CliError::new(
                            "count makes forks of a computer; a folder is copied into one fork.",
                        ));
                    }
                    let recipe = project.named_recipe(crate::config::DEFAULT_RECIPE)?;
                    let mut made = folder::fork_folder(
                        &engine,
                        &self.working_directory.join(&source),
                        params.name.as_deref(),
                        &recipe,
                        settings.as_ref(),
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
                        made["exitCode"] = serde_json::json!(ran.exit_code);
                        made["stdout"] = serde_json::json!(String::from_utf8_lossy(&out));
                        made["stderr"] = serde_json::json!(String::from_utf8_lossy(&err));
                    }
                    return Ok(made);
                }
                let recipe = match &params.recipe {
                    Some(name) => Some(project.named_recipe(name)?),
                    None => ops::resolve_recipe(explicit.as_deref(), &project)?,
                };
                let args = ForkArgs {
                    source,
                    name: params.name,
                    checkpoint,
                    secrets: params.secrets,
                    recipe,
                    recipe_name: params.recipe.clone(),
                };
                if params.recipe.is_none() && (params.count.is_some() || !params.command.is_empty())
                {
                    let many = ManyArgs {
                        fork: args,
                        count: params.count.unwrap_or(1),
                        command: params.command,
                        cwd: None,
                    };
                    let mut value = fanout::fork_many(
                        &engine,
                        &many,
                        settings.as_ref(),
                        &|_| {},
                        &|_, _, _| {},
                    )
                    .await?;
                    let context = flows::Context {
                        project: &project,
                        settings: settings.as_ref(),
                        recipe: many.fork.recipe.as_ref(),
                        write_back: engine.device().is_none(),
                    };
                    let mut results = Vec::new();
                    for fork in value["forks"].as_array().cloned().unwrap_or_default() {
                        if fork.get("exitCode").is_none() {
                            continue;
                        }
                        let ok = fork.get("error").is_none() && fork["exitCode"] == 0;
                        let name = fork["name"].as_str().unwrap_or_default();
                        if let Some(result) =
                            flows::after_command(&engine, name, ok, &context, &|_| {}).await
                        {
                            results.push(result);
                        }
                    }
                    if !results.is_empty() {
                        value["autoKeep"] = Value::Array(results);
                    }
                    return Ok(value);
                }
                ops::fork(&engine, &args, settings.as_ref(), &|_| {}).await
            }
            .await,
        )
    }

    #[tool(
        description = "Keep a fork's changes into the computer it came from (or `into`). Returns status kept, dry-run or refused; a refusal (divergent changes, a database change nothing migrates) lists what stopped it and the target was not changed. Takes a checkpoint of the target first; `undo` puts it back. With `request` nothing is kept: a review request is recorded for the fork and returned. For a fork made from a folder, the fork's commits are fetched into the folder (a fast-forward or a clean merge, with a backup ref `undo` goes back to). With `promote` the fork replaces main instead (the old main is archived as main-before-<time>); a refusal means nothing changed."
    )]
    async fn fold(&self, Parameters(params): Parameters<FoldParams>) -> Result<String, String> {
        show(
            async {
                let engine = self.engine_acting_on(params.device.as_deref()).await?;
                if params.promote && !params.request {
                    // The computers of that device swap roles: their driver sessions end first.
                    if !params.dry_run {
                        let device = engine.device().map(str::to_owned);
                        self.close_sessions(|(on, _)| *on == device).await;
                    }
                    let mut value = ops::promote(
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
                    return Ok(value);
                }
                let project = Project::discover(&self.working_directory);
                let explicit = params
                    .recipe_file
                    .as_deref()
                    .map(|path| self.working_directory.join(path));
                let recipe = ops::resolve_recipe(explicit.as_deref(), &project)?;
                let settings =
                    project.load_settings_as(self.agent().as_deref(), self.agent_named)?;
                let context = flows::Context {
                    project: &project,
                    settings: settings.as_ref(),
                    recipe: recipe.as_ref(),
                    write_back: !params.no_write_back && engine.device().is_none(),
                };
                let mode = if params.request {
                    flows::Mode::Request
                } else {
                    flows::Mode::Keep
                };
                flows::run(
                    &engine,
                    &params.fork,
                    params.into.as_deref(),
                    &context,
                    ops::FoldOptions {
                        dry_run: params.dry_run,
                        data_ok: params.data_ok,
                        reviewed: params.reviewed,
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
        description = "List the review requests recorded with `fold` and `request`: each with its fork's name and state, the computer it would be kept into and how many changes each bucket held. Defaults to the open ones."
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
        description = "Show a review request: a fresh diff of its fork by bucket, the credentials the fork used (`credentials.used`) and was refused (`credentials.blocked`), and `stale: true` when the fork changed since the request was made (a keep then refuses)."
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
        description = "Keep a review request's fork into the computer it would be kept into, pinned to the digest the request was made from. Returns status kept or refused; a refusal (the fork changed, divergent changes, a database change nothing migrates) leaves the request open and the target unchanged. A fork made from a folder is kept with `fold`."
    )]
    async fn request_keep(
        &self,
        Parameters(params): Parameters<RequestKeepParams>,
    ) -> Result<String, String> {
        show(
            async {
                let project = Project::discover(&self.working_directory);
                let explicit = params
                    .recipe_file
                    .as_deref()
                    .map(|path| self.working_directory.join(path));
                let recipe = ops::resolve_recipe(explicit.as_deref(), &project)?;
                let engine = self.engine_acting_on(params.device.as_deref()).await?;
                review::keep(
                    &engine,
                    &params.id,
                    ops::keep_hooks(recipe.as_ref()),
                    params.data_ok,
                    &|_| {},
                )
                .await
            }
            .await,
        )
    }

    #[tool(
        description = "Throw away a review request's fork without keeping its changes. main and the computer the changes would be kept into are never dropped."
    )]
    async fn request_throw_away(
        &self,
        Parameters(RequestIdParam { id, device }): Parameters<RequestIdParam>,
    ) -> Result<String, String> {
        show(
            async {
                let engine = self.engine_acting_on(device.as_deref()).await?;
                review::throw_away(&engine, &id, &|_| {}).await
            }
            .await,
        )
    }

    #[tool(
        description = "Start a review request's fork if it is stopped and return how to reach it: its name and id, the SSH name (after `ssh-setup`) and the URLs of its published ports."
    )]
    async fn request_open(
        &self,
        Parameters(RequestIdParam { id, device }): Parameters<RequestIdParam>,
    ) -> Result<String, String> {
        show(
            async {
                let engine = self.engine_acting_on(device.as_deref()).await?;
                review::open(&engine, &id, &|_| {}).await
            }
            .await,
        )
    }

    #[tool(
        description = "Show what changed in a computer since it was forked, by bucket (code, packages, settings and files, databases, other, not kept). Returns the diff document and its readable text."
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
        description = "Ask the hosted reviewer (needs reviewer.url or SILO_REVIEWER_URL) to read the diff of a fork or review request through the general, packages and code lenses. Only the diff is sent. Returns findings with severity, file and message under an advisory note; it can miss things, so the person decides what to keep."
    )]
    async fn review(&self, Parameters(params): Parameters<ReviewParams>) -> Result<String, String> {
        show(
            async {
                let project = Project::discover(&self.working_directory);
                let settings =
                    project.load_settings_as(self.agent().as_deref(), self.agent_named)?;
                let lenses = crate::hosted::parse_lenses(params.lens.as_deref())?;
                crate::hosted::review_target(
                    &self.engine().await?,
                    settings.as_ref(),
                    &params.target,
                    &lenses,
                    &|_| {},
                )
                .await
            }
            .await,
        )
    }

    #[tool(
        description = "Put a computer back to before its latest keep. For a fork made from a folder, pass the fork or the folder's path to put the folder's branch back to before its latest fold."
    )]
    async fn undo(
        &self,
        Parameters(DeviceComputerParam { computer, device }): Parameters<DeviceComputerParam>,
    ) -> Result<String, String> {
        show(
            async {
                let engine = self.engine_acting_on(device.as_deref()).await?;
                if engine.device().is_some() && folder::is_folder(&computer) {
                    return Err(CliError::new(
                        "A folder is on this device, so `device` cannot be used with it.",
                    ));
                }
                let mut value =
                    flows::undo(&engine, &computer, &self.working_directory, &|_| {}).await?;
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

    #[tool(description = "Throw a computer away. main cannot be dropped through this tool.")]
    async fn drop(
        &self,
        Parameters(DeviceComputerParam { computer, device }): Parameters<DeviceComputerParam>,
    ) -> Result<String, String> {
        show(
            async {
                let engine = self.engine_acting_on(device.as_deref()).await?;
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
                let project = Project::discover(&self.working_directory);
                let settings =
                    project.load_settings_as(self.agent().as_deref(), self.agent_named)?;
                let step_settings = crate::config::Settings::step_settings(settings.as_ref());
                let engine = self.engine_acting_on(params.device.as_deref()).await?;
                let (output, step) =
                    steps::run_exec(&engine, &args, step_settings, &|_| {}).await?;
                let mut value = output.to_json();
                value["step"] = step;
                Ok(value)
            }
            .await,
        )
    }

    #[tool(
        description = "List the steps of a computer: the commands run in it through Silo (`exec`, or `fork` with a command), numbered from 1, with exit code, times and the checkpoint taken after each when step checkpoints are on. Fork from one with `fork` and `fromStep`."
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
        description = "Show how the computers descend from each other: each has a parent, what it was forked from (main, a checkpoint, a step, a recipe or a folder), its state and its pending changes (the open review request)."
    )]
    async fn tree(&self, Parameters(params): Parameters<DeviceParam>) -> Result<String, String> {
        show(async { steps::tree(&self.engine_on(params.device.as_deref()).await?).await }.await)
    }

    #[tool(description = "Save a checkpoint of a computer.")]
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

    #[tool(
        description = "List the ports published from a computer, with the host port and URL each is reachable at. Forks publish the ports their source does."
    )]
    async fn ports(
        &self,
        Parameters(ComputerParam { computer }): Parameters<ComputerParam>,
    ) -> Result<String, String> {
        show(async { access::ports(&self.engine().await?, &computer).await }.await)
    }

    #[tool(
        description = "Publish a port of a computer on this device. With `http` it is served as a website."
    )]
    async fn publish(
        &self,
        Parameters(params): Parameters<PublishParams>,
    ) -> Result<String, String> {
        show(
            async {
                access::publish(
                    &self.engine().await?,
                    &params.computer,
                    params.port,
                    params.http,
                )
                .await
            }
            .await,
        )
    }

    #[tool(description = "Stop publishing a port of a computer.")]
    async fn unpublish(
        &self,
        Parameters(params): Parameters<PublishParams>,
    ) -> Result<String, String> {
        show(
            async { access::unpublish(&self.engine().await?, &params.computer, params.port).await }
                .await,
        )
    }

    #[tool(
        description = "Make `ssh <computer>.silo` (`.silo-dev` from a development build) reach a computer; `up` does it for main."
    )]
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

    /// One request to the computer's driver session. The host tools are for forks: `main` is
    /// refused, like `drop`.
    async fn session_request(
        &self,
        device: Option<&str>,
        computer: &str,
        method: &str,
        params: Value,
    ) -> Result<Value, CliError> {
        self.session_call(device, computer, Some((method, params)), 0)
            .await
    }

    /// A request (`Some`) or, with `None`, a screenshot of at most `max_side` pixels. The
    /// computer is on `device` (`None`: this one); driving one on another device needs it in
    /// the agent's `devices:`.
    async fn session_call(
        &self,
        device: Option<&str>,
        computer: &str,
        request: Option<(&str, Value)>,
        max_side: u32,
    ) -> Result<Value, CliError> {
        let engine = self.engine_acting_on(device).await?;
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
        device: Option<&str>,
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
            self.session_call(device, computer, None, side).await
        } else {
            self.session_request(
                device,
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
        device: Option<&str>,
        computer: &str,
        action: crate::usetools::Action,
    ) -> Result<CallToolResult, String> {
        let (tool, arguments) = crate::usetools::driver_call(&action);
        self.drive(device, computer, tool, arguments).await
    }

    #[tool(
        description = "Fork a computer (default main), start the fork and wait until its desktop and computer-use driver are ready. Returns the fork's name and its computer-use status. computer_use sets the approval mode for agents running inside the fork: ask (default) or auto (no prompts; needs computer_use_auto in .silo/silo.yaml, which is a convenience, not a gate). main keeps its own mode. The computer_use_* tools on this server then drive the fork; they are gated by this client's tool prompts, not by that mode. `device` forks on a connected device the agent's `devices:` entry lists; a folder is not accepted."
    )]
    async fn fork_gui(
        &self,
        Parameters(params): Parameters<ForkGuiParams>,
    ) -> Result<String, String> {
        show(
            async {
                let project = Project::discover(&self.working_directory);
                let settings =
                    project.load_settings_as(self.agent().as_deref(), self.agent_named)?;
                let approval = params
                    .computer_use
                    .as_deref()
                    .map(crate::gui::parse_approval)
                    .transpose()?;
                if let Some(mode) = approval {
                    crate::gui::check_auto(mode, settings.as_ref(), true)?;
                }
                let source = params.source.unwrap_or_else(|| ops::MAIN.to_owned());
                if folder::is_folder(&source) {
                    return Err(CliError::new(
                        "fork_gui forks a computer; a folder is copied into a Linux fork with fork.",
                    ));
                }
                let engine = self.engine_acting_on(params.device.as_deref()).await?;
                let args = crate::gui::GuiArgs {
                    fork: ForkArgs {
                        source,
                        name: params.name,
                        ..ForkArgs::default()
                    },
                    approval,
                    timeout: std::time::Duration::from_secs(
                        params
                            .timeout_secs
                            .unwrap_or(crate::gui::DEFAULT_TIMEOUT_SECS)
                            .max(1),
                    ),
                };
                crate::gui::fork_gui(&engine, &args, settings.as_ref(), &|_| {}).await
            }
            .await,
        )
    }

    #[tool(
        description = "Say whether a computer's desktop and computer-use driver are ready now, without waiting: ready, what it is waiting for, and the computer-use state and approval mode."
    )]
    async fn computer_use_ready(
        &self,
        Parameters(DeviceComputerParam { computer, device }): Parameters<DeviceComputerParam>,
    ) -> Result<String, String> {
        show(
            async {
                let engine = self.engine_on(device.as_deref()).await?;
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
            params.device.as_deref(),
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
        self.act(p.device.as_deref(), &p.computer, action).await
    }

    #[tool(description = "Type text into the focused window of a fork.")]
    async fn computer_use_type(
        &self,
        Parameters(p): Parameters<TypeParams>,
    ) -> Result<CallToolResult, String> {
        self.act(
            p.device.as_deref(),
            &p.computer,
            crate::usetools::Action::Type { text: p.text },
        )
        .await
    }

    #[tool(description = "Press one key, with optional modifiers, in a fork.")]
    async fn computer_use_key(
        &self,
        Parameters(p): Parameters<KeyParams>,
    ) -> Result<CallToolResult, String> {
        self.act(
            p.device.as_deref(),
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
            p.device.as_deref(),
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
            p.device.as_deref(),
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
            p.device.as_deref(),
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
        self.act(p.device.as_deref(), &p.computer, action).await
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
        Parameters(DeviceComputerParam { computer, device }): Parameters<DeviceComputerParam>,
    ) -> Result<String, String> {
        show(
            self.session_request(
                device.as_deref(),
                &computer,
                "tools/list",
                serde_json::json!({}),
            )
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
            p.device.as_deref(),
            &p.computer,
            &p.tool,
            p.arguments.unwrap_or_else(|| serde_json::json!({})),
        )
        .await
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for SiloMcp {
    async fn initialize(
        &self,
        request: rmcp::model::InitializeRequestParams,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<rmcp::model::InitializeResult, rmcp::ErrorData> {
        if !self.agent_named {
            if let Some(name) = crate::config::normalize_agent(Some(&request.client_info.name)) {
                *self.agent.lock().unwrap() = Some(name);
            }
        }
        context.peer.set_peer_info(request.clone());
        self.negotiate_initialize(&request)
    }
}

/// Serves the tools on stdin and stdout until the client closes them.
pub async fn serve(
    transport: Transport,
    working_directory: PathBuf,
    agent: Option<String>,
) -> Result<(), CliError> {
    let server = SiloMcp::for_transport(transport, working_directory).with_agent(agent);
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
mod in_computer_tests {
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

    #[test]
    fn inside_a_computer_the_device_only_tools_are_not_listed() {
        let on_device = SiloMcp::new("/nowhere.sock".into(), std::env::temp_dir());
        let inside = SiloMcp::for_transport(channel(), std::env::temp_dir());
        for name in DEVICE_ONLY_TOOLS {
            assert!(on_device.tool_router.has_route(name), "{name}");
            assert!(!inside.tool_router.has_route(name), "{name}");
        }
        for name in ["list", "fork", "diff", "exec", "drop"] {
            assert!(inside.tool_router.has_route(name), "{name}");
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
}
