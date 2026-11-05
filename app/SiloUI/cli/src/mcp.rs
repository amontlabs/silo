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
    config::Project,
    engine::Engine,
    error::CliError,
    ops::{self, DiffArgs, ExecArgs, ForkArgs},
};

#[derive(Clone)]
pub struct SiloMcp {
    socket: PathBuf,
    working_directory: PathBuf,
    engine: Arc<tokio::sync::Mutex<Option<Engine>>>,
    tool_router: ToolRouter<Self>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ComputerParam {
    /// The computer's name.
    pub computer: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ForkParams {
    /// The computer to copy, usually `main`.
    pub source: String,
    /// The fork's name; defaults to the source name plus a short id.
    pub name: Option<String>,
    /// Fork from this checkpoint id instead of the current state.
    pub checkpoint: Option<String>,
    /// Silo-managed secrets the fork may use; each must be in fork_secrets in `.silo/silo.yaml`.
    #[serde(default)]
    pub secrets: Vec<String>,
    /// Path of a recipe whose quiet and scrub lists apply to the fork.
    pub recipe_file: Option<String>,
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
    /// Carry database changes even though no migrate step covers them.
    #[serde(default)]
    pub data_ok: bool,
    /// The reviewDigest from `diff`; the keep refuses if the fork changed since.
    pub reviewed: Option<String>,
    /// Path of a recipe whose keep: section applies.
    pub recipe_file: Option<String>,
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
            tool_router: Self::tool_router(),
        }
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

    #[tool(description = "Copy a computer into a new fork for a task and wait until it is ready.")]
    async fn fork(&self, Parameters(params): Parameters<ForkParams>) -> Result<String, String> {
        show(
            async {
                let project = Project::discover(&self.working_directory);
                let settings = project.load_settings()?;
                let explicit = params
                    .recipe_file
                    .as_deref()
                    .map(|path| self.working_directory.join(path));
                let recipe = ops::resolve_recipe(explicit.as_deref(), &project)?;
                let args = ForkArgs {
                    source: params.source,
                    name: params.name,
                    checkpoint: params.checkpoint,
                    secrets: params.secrets,
                    recipe,
                };
                ops::fork(&self.engine().await?, &args, settings.as_ref(), &|_| {}).await
            }
            .await,
        )
    }

    #[tool(
        description = "Keep a fork's changes into the computer it came from (or `into`). Returns status kept, dry-run or refused; a refusal (divergent changes, a database change nothing migrates) lists what stopped it and the target was not changed. Takes a checkpoint of the target first; `undo` puts it back."
    )]
    async fn fold(&self, Parameters(params): Parameters<FoldParams>) -> Result<String, String> {
        show(
            async {
                let project = Project::discover(&self.working_directory);
                let explicit = params
                    .recipe_file
                    .as_deref()
                    .map(|path| self.working_directory.join(path));
                let recipe = ops::resolve_recipe(explicit.as_deref(), &project)?;
                ops::fold(
                    &self.engine().await?,
                    &params.fork,
                    params.into.as_deref(),
                    ops::keep_hooks(recipe.as_ref()),
                    ops::FoldOptions { dry_run: params.dry_run, data_ok: params.data_ok, reviewed: params.reviewed },
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

    #[tool(description = "Put a computer back to before its latest keep.")]
    async fn undo(
        &self,
        Parameters(ComputerParam { computer }): Parameters<ComputerParam>,
    ) -> Result<String, String> {
        show(async { ops::undo(&self.engine().await?, &computer, &|_| {}).await }.await)
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
                ops::exec(&self.engine().await?, &args).await
            }
            .await,
        )
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

    #[tool(description = "List a computer's checkpoints.")]
    async fn checkpoints(
        &self,
        Parameters(ComputerParam { computer }): Parameters<ComputerParam>,
    ) -> Result<String, String> {
        show(async { ops::checkpoints(&self.engine().await?, &computer).await }.await)
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for SiloMcp {}

/// Serves the tools on stdin and stdout until the client closes them.
pub async fn serve(socket: PathBuf, working_directory: PathBuf) -> Result<(), CliError> {
    let service = SiloMcp::new(socket, working_directory)
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|error| CliError::new(format!("The MCP server could not start: {error}")))?;
    service
        .waiting()
        .await
        .map_err(|error| CliError::new(format!("The MCP server stopped: {error}")))?;
    Ok(())
}
