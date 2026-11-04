//! `silo mcp`: the commands as MCP tools over stdio. Every tool makes the same engine calls
//! as its command and answers with the same JSON as `--json`.

use std::path::PathBuf;

use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    schemars, tool, tool_handler, tool_router, ServerHandler, ServiceExt,
};
use serde::Deserialize;
use serde_json::Value;

use crate::{
    config::{load_recipe, Project},
    engine::Engine,
    error::CliError,
    ops::{self, ExecArgs, ForkArgs},
};

#[derive(Clone)]
pub struct SiloMcp {
    socket: PathBuf,
    working_directory: PathBuf,
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
    /// Seconds before the command is stopped (default 600).
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
            tool_router: Self::tool_router(),
        }
    }

    async fn engine(&self) -> Result<Engine, CliError> {
        Engine::connect(&self.socket).await
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

    #[tool(description = "Copy a computer into a new fork for a task and wait until it is ready.")]
    async fn fork(&self, Parameters(params): Parameters<ForkParams>) -> Result<String, String> {
        show(
            async {
                let project = Project::discover(&self.working_directory);
                let settings = project.load_settings()?;
                let recipe = params
                    .recipe_file
                    .as_deref()
                    .map(|path| load_recipe(&self.working_directory.join(path)))
                    .transpose()?;
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

    #[tool(description = "Run a command in a computer. Returns exitCode, stdout and stderr.")]
    async fn exec(&self, Parameters(params): Parameters<ExecParams>) -> Result<String, String> {
        show(
            async {
                let args = ExecArgs {
                    computer: params.computer,
                    argv: params.command,
                    cwd: params.cwd,
                    timeout_secs: params.timeout_secs.unwrap_or(600),
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
