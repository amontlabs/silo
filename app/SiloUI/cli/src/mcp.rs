//! `silo mcp`: the commands as MCP tools over stdio. Every tool makes the same engine calls
//! as its command and answers with the same JSON as `--json`.

use std::{path::PathBuf, sync::Arc};

use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    schemars, tool, tool_handler, tool_router, ServerHandler, ServiceExt,
};
use serde::Deserialize;
use serde_json::Value;

use crate::{
    access,
    config::Project,
    engine::{build_channel, Engine},
    error::CliError,
    fanout::{self, ManyArgs},
    flows, folder,
    ops::{self, DiffArgs, ExecArgs, ForkArgs},
    steps,
};

#[derive(Clone)]
pub struct SiloMcp {
    socket: PathBuf,
    working_directory: PathBuf,
    engine: Arc<tokio::sync::Mutex<Option<Engine>>>,
    /// The client's name from `initialize`, lowercase: which `agents:` entry applies.
    agent: Arc<std::sync::Mutex<Option<String>>>,
    /// Set when `--agent` or `SILO_AGENT` named the agent: the client's name is then ignored.
    agent_named: bool,
    tool_router: ToolRouter<Self>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ComputerParam {
    /// The computer's name.
    pub computer: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
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
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpParams {
    /// The recipe `main` is made from when it does not exist (default linux-dev).
    pub recipe: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ExecParams {
    /// The computer to run in.
    pub computer: String,
    /// The command and its arguments.
    pub command: Vec<String>,
    /// Working directory in the computer.
    pub cwd: Option<String>,
    /// Seconds before the command is stopped (default and maximum 180).
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CheckpointParams {
    /// The computer to checkpoint.
    pub computer: String,
    /// A name for the checkpoint.
    pub name: Option<String>,
    /// manual, automatic or before-keep.
    pub kind: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
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
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DiffParams {
    /// The fork to look at.
    pub computer: String,
    /// Cut the text to this many lines (default 15).
    pub max_lines: Option<u32>,
    /// Include the idle fork's own background changes.
    #[serde(default)]
    pub no_baseline: bool,
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
        Self {
            socket,
            working_directory,
            engine: Arc::new(tokio::sync::Mutex::new(None)),
            agent: Arc::new(std::sync::Mutex::new(None)),
            agent_named: false,
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
    /// name (self-reported, so `agents:` entries may only narrow the top level).
    pub fn agent(&self) -> Option<String> {
        self.agent.lock().unwrap().clone()
    }

    /// The one connection this server keeps, opened again when it has ended.
    async fn engine(&self) -> Result<Engine, CliError> {
        let mut held = self.engine.lock().await;
        if let Some(engine) = held.as_ref().filter(|engine| !engine.is_closed()) {
            return Ok(engine.clone());
        }
        let engine = Engine::connect(&self.socket).await?;
        *held = Some(engine.clone());
        Ok(engine)
    }

    #[tool(description = "List computers and their state.")]
    async fn list(&self) -> Result<String, String> {
        show(async { ops::list(&self.engine().await?).await }.await)
    }

    #[tool(description = "Show one computer: state, size and checkpoints.")]
    async fn status(
        &self,
        Parameters(ComputerParam { computer }): Parameters<ComputerParam>,
    ) -> Result<String, String> {
        show(async { ops::status(&self.engine().await?, &computer).await }.await)
    }

    #[tool(
        description = "List the Silo secrets by name with the computers they are assigned to. Values are never returned, and none can be set through MCP."
    )]
    async fn secrets_list(&self) -> Result<String, String> {
        show(async { ops::secrets_list(&self.engine().await?).await }.await)
    }

    #[tool(
        description = "Start `main` (made from a recipe the first time, default linux-dev) and keep a warm checkpoint of it, so forks of an unchanged main start without a new checkpoint."
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
                let mut value = ops::up(&engine, &name, &recipe, &|_| {}).await?;
                match access::home_dir() {
                    Ok(home) => {
                        access::ssh_for_up(&engine, &mut value, &home, build_channel()).await
                    }
                    Err(error) => value["ssh"] = serde_json::json!({ "error": error.message }),
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
                let settings = project.load_settings(self.agent().as_deref())?;
                let explicit = params
                    .recipe_file
                    .as_deref()
                    .map(|path| self.working_directory.join(path));
                let source = params.source.unwrap_or_else(|| ops::MAIN.to_owned());
                let engine = self.engine().await?;
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
                        write_back: true,
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
                if params.promote && !params.request {
                    let engine = self.engine().await?;
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
                let settings = project.load_settings(self.agent().as_deref())?;
                let context = flows::Context {
                    project: &project,
                    settings: settings.as_ref(),
                    recipe: recipe.as_ref(),
                    write_back: !params.no_write_back,
                };
                let mode = if params.request {
                    flows::Mode::Request
                } else {
                    flows::Mode::Keep
                };
                flows::run(
                    &self.engine().await?,
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
        description = "Show what changed in a computer since it was forked, by bucket (code, packages, settings and files, databases, other, not kept). Returns the diff document and its readable text."
    )]
    async fn diff(&self, Parameters(params): Parameters<DiffParams>) -> Result<String, String> {
        show(
            async {
                let args = DiffArgs {
                    computer: params.computer,
                    max_lines: params.max_lines,
                    no_baseline: params.no_baseline,
                    compute_baseline: false,
                };
                ops::diff(&self.engine().await?, &args, &|_| {}).await
            }
            .await,
        )
    }

    #[tool(
        description = "Put a computer back to before its latest keep. For a fork made from a folder, pass the fork or the folder's path to put the folder's branch back to before its latest fold."
    )]
    async fn undo(
        &self,
        Parameters(ComputerParam { computer }): Parameters<ComputerParam>,
    ) -> Result<String, String> {
        show(
            async {
                let engine = self.engine().await?;
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
        Parameters(ComputerParam { computer }): Parameters<ComputerParam>,
    ) -> Result<String, String> {
        show(
            async { ops::drop_computer(&self.engine().await?, &computer, false, &|_| {}).await }
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
                let settings = project.load_settings(self.agent().as_deref())?;
                let step_settings = crate::config::Settings::step_settings(settings.as_ref());
                let (output, step) =
                    steps::run_exec(&self.engine().await?, &args, step_settings, &|_| {}).await?;
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
        Parameters(ComputerParam { computer }): Parameters<ComputerParam>,
    ) -> Result<String, String> {
        show(async { steps::list(&self.engine().await?, &computer).await }.await)
    }

    #[tool(
        description = "Show how the computers descend from each other: each has a parent, what it was forked from (main, a checkpoint, a step, a recipe or a folder), its state and its pending changes (the open review request)."
    )]
    async fn tree(&self) -> Result<String, String> {
        show(async { steps::tree(&self.engine().await?).await }.await)
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
        Parameters(ComputerParam { computer }): Parameters<ComputerParam>,
    ) -> Result<String, String> {
        show(async { ops::checkpoints(&self.engine().await?, &computer).await }.await)
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
    socket: PathBuf,
    working_directory: PathBuf,
    agent: Option<String>,
) -> Result<(), CliError> {
    let service = SiloMcp::new(socket, working_directory)
        .with_agent(agent)
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|error| CliError::new(format!("The MCP server could not start: {error}")))?;
    service
        .waiting()
        .await
        .map_err(|error| CliError::new(format!("The MCP server stopped: {error}")))?;
    Ok(())
}
