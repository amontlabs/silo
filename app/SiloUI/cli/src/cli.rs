//! Argument parsing and command dispatch.

use std::{
    io::Write,
    path::{Path, PathBuf},
};

use clap::{Args, Parser, Subcommand};
use serde_json::Value;

use crate::{
    access, approvals,
    config::{Project, Recipe, Settings},
    engine::{build_channel, default_socket, Engine},
    error::CliError,
    fanout::{self, ManyArgs},
    flows, folder, hosted,
    ops::{self, CreateArgs, DiffArgs, ExecArgs, ForkArgs, Platform},
    pr, render, review, steps,
};

#[derive(Parser, Debug)]
#[command(
    name = "silo",
    version,
    about = "Fork a computer, let an agent try, keep what worked."
)]
pub struct Cli {
    /// The engine socket (default: SILO_SOCKET, else the build channel's own).
    #[arg(long, global = true, value_name = "PATH")]
    pub socket: Option<PathBuf>,
    /// Print JSON instead of text.
    #[arg(long, global = true)]
    pub json: bool,
    /// Use a socket outside this build's own home (never the production home from a debug build).
    #[arg(long, global = true)]
    pub allow_other_channel: bool,
    /// The agent the settings in `agents:` of `.silo/silo.yaml` are read for (default: SILO_AGENT).
    #[arg(long, global = true, value_name = "NAME", env = "SILO_AGENT")]
    pub agent: Option<String>,
    /// Run the command on a connected device (its name or id) instead of this one, over
    /// Connections. Folders, ports, SSH names and secrets stay on this device.
    #[arg(long, global = true, value_name = "DEVICE")]
    pub device: Option<String>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum ToolsCommand {
    /// Turn on Silo's tools in a computer: `silo` inside it reaches Silo, as that computer.
    /// A running computer may need a restart before it can reach them.
    Enable {
        computer: String,
        /// A `silo` program built for Linux computers to copy to /usr/local/bin/silo (the
        /// computer must be running).
        #[arg(long, value_name = "PATH")]
        binary: Option<PathBuf>,
    },
    /// Turn off Silo's tools in a computer; its open connections close at once.
    Disable { computer: String },
    /// Say whether a computer has Silo's tools and whether it can reach them.
    Status { computer: String },
}

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesktopState {
    On,
    Off,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// List computers and their state.
    List {
        /// Also show the hidden throwaway computers (baselines, recipe builders).
        #[arg(long)]
        all: bool,
        /// List this device's computers and every connected device's.
        #[arg(long)]
        all_devices: bool,
    },
    /// Show one computer.
    Status { computer: String },
    /// Create a computer.
    Create(CreateCommand),
    /// Start a computer.
    Start { computer: String },
    /// Stop a computer.
    Stop { computer: String },
    /// Finish the setup of a computer that stopped before it was ready.
    RetrySetup { computer: String },
    /// Turn a Linux computer's desktop on or off. Its forks start with the same choice.
    Desktop {
        /// `on` starts the desktop now and with the computer; `off` stops it.
        #[arg(value_enum)]
        state: DesktopState,
        computer: String,
    },
    /// Start `main` (made from a recipe the first time).
    Up(UpCommand),
    /// List, build or remove recipes.
    #[command(subcommand)]
    Recipe(RecipeCommand),
    /// Copy a computer for a task.
    Fork(ForkCommand),
    /// Keep a fork's changes into the computer it came from.
    Fold(FoldCommand),
    /// List review requests, or show, keep, throw away or open one.
    Requests(RequestsCommand),
    /// List what computers' agents asked you to allow, or show, allow or deny one.
    Approvals(ApprovalsCommand),
    /// Put a computer back to before its latest keep, or a folder back to before its latest fold.
    Undo {
        /// A computer, a fork made from a folder, or the folder's path.
        computer: String,
    },
    /// Show what changed in a computer since it was forked.
    Diff(DiffCommand),
    /// Ask the hosted reviewer to read a fork's diff (needs reviewer.url or SILO_REVIEWER_URL).
    Review {
        /// A computer or a review request id (fr-...).
        target: String,
        /// general, packages, code or all (default).
        #[arg(long, value_name = "LENS")]
        lens: Option<String>,
    },
    /// Throw a computer away.
    Drop(DropCommand),
    /// Run a command in a computer.
    Exec(ExecCommand),
    /// Save a checkpoint of a computer.
    Checkpoint(CheckpointCommand),
    /// List a computer's checkpoints.
    Checkpoints { computer: String },
    /// List the commands run in a computer through Silo (its steps).
    Steps { computer: String },
    /// Show how the computers descend from each other, with their state and pending changes.
    Tree,
    /// List the ports published from a computer and where they are reachable.
    Ports { computer: String },
    /// Publish a port of a computer on this device.
    Publish {
        computer: String,
        /// The port inside the computer.
        port: u16,
        /// Serve it as a website (a URL per computer).
        #[arg(long)]
        http: bool,
    },
    /// Stop publishing a port of a computer.
    Unpublish { computer: String, port: u16 },
    /// Show or change what a computer may reach on the network.
    #[command(subcommand)]
    Network(NetworkCommand),
    /// Make `ssh <computer>.silo` reach a computer (`silo up` does it for main).
    SshSetup { computer: String },
    /// Manage the Silo secrets and which computers have them.
    #[command(subcommand)]
    Secrets(SecretsCommand),
    /// Turn Silo's tools in a computer on or off, or say whether it has them.
    #[command(subcommand)]
    Tools(ToolsCommand),
    /// Prepare this machine to host computers that other devices manage, or show its state.
    #[command(subcommand)]
    Host(crate::hosts::HostCommand),
    /// Add, list or remove the devices whose computers this one manages (Connections).
    #[command(subcommand)]
    Devices(crate::hosts::DevicesCommand),
    /// Save a whole computer as a file to share. A Linux computer's known login files are removed from it unless you ask to include them.
    Export(ExportCommand),
    /// Add the computer in an export file as a new, stopped computer.
    Import(ImportCommand),
    /// Wait for a job that is already running, by id.
    Job { id: String },
    /// Serve the MCP tools on stdio.
    Mcp,
}

#[derive(Subcommand, Debug)]
pub enum NetworkCommand {
    /// Show a computer's network policy, or change it. A change applies when the computer next
    /// starts; a running computer keeps its rules until it restarts.
    Policy {
        computer: String,
        /// open (any public address), allowlist (only the hosts listed) or closed (nothing out).
        #[arg(long, value_enum)]
        mode: Option<PolicyMode>,
        /// A host the computer may reach on port 443: `example.com` or `*.example.com`
        /// (repeatable).
        #[arg(long = "allow", value_name = "HOST")]
        allow: Vec<String>,
        /// A host or preset to take off the list (repeatable).
        #[arg(long = "remove", value_name = "HOST")]
        remove: Vec<String>,
        /// A named list of hosts kept by Silo, such as npm, pypi or github (repeatable).
        #[arg(long = "preset", value_name = "NAME")]
        presets: Vec<String>,
    },
}

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicyMode {
    Open,
    Allowlist,
    Closed,
}

#[derive(Subcommand, Debug)]
pub enum SecretsCommand {
    /// List secret names and the computers they are assigned to (never values).
    List,
    /// Add a secret or replace its value; the value is read from stdin.
    Set {
        name: String,
        /// A computer that gets the secret (repeatable; required for a new secret).
        #[arg(long = "computer", value_name = "COMPUTER")]
        computers: Vec<String>,
        /// A domain the secret may be sent to (repeatable; required for a new secret).
        #[arg(long = "domain", value_name = "DOMAIN")]
        domains: Vec<String>,
    },
    /// Give a computer an existing secret.
    Assign { name: String, computer: String },
    /// Take a secret away from a computer.
    Unassign { name: String, computer: String },
}

#[derive(Args, Debug)]
pub struct CreateCommand {
    pub name: String,
    /// A Linux computer (the default).
    #[arg(long, conflicts_with_all = ["macos", "container"])]
    pub linux: bool,
    /// A macOS computer.
    #[arg(long, conflicts_with = "container")]
    pub macos: bool,
    /// A Linux computer that runs as a container: lighter than a VM but less isolated. Linux hosts only.
    #[arg(long)]
    pub container: bool,
    #[arg(long)]
    pub cpus: Option<u64>,
    /// Memory in GiB.
    #[arg(long)]
    pub memory: Option<u64>,
    /// Disk in GiB.
    #[arg(long)]
    pub disk: Option<u64>,
    /// Create a Linux computer without starting its desktop (`silo desktop on` turns it on later).
    #[arg(long, conflicts_with = "macos")]
    pub no_desktop: bool,
}

#[derive(Args, Debug)]
pub struct UpCommand {
    /// The recipe `main` is made from when it does not exist (default linux-dev).
    #[arg(long, value_name = "NAME")]
    pub recipe: Option<String>,
}

#[derive(Args, Debug)]
pub struct ExportCommand {
    pub computer: String,
    /// Where to write the file (default: <computer>-<date>.silo-backup here).
    #[arg(short = 'o', long, value_name = "FILE")]
    pub output: Option<PathBuf>,
    /// Export this checkpoint of the computer (its id or name) instead of its current state.
    #[arg(long, value_name = "CHECKPOINT")]
    pub checkpoint: Option<String>,
    /// Share without asking when logins are found or Silo could not check for them.
    #[arg(long)]
    pub yes: bool,
    /// Keep the logins found on the computer in the file. Without it, an export of a Linux
    /// computer removes its known login files from the exported computer (traces may remain in
    /// the file's disk layers).
    #[arg(long)]
    pub include_logins: bool,
}

#[derive(Args, Debug)]
pub struct ImportCommand {
    /// The .silo-backup file.
    pub file: PathBuf,
    /// The new computer's name (default: the sender's, else with -imported added).
    #[arg(long, value_name = "NAME")]
    pub name: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum RecipeCommand {
    /// List the built-in and project recipes and which are built.
    Ls,
    /// Build a recipe now (or again, with --force) so forks from it start at once.
    Build {
        name: String,
        #[arg(long)]
        force: bool,
    },
    /// Remove a recipe's build and its hidden computer.
    Rm { name: String },
}

#[derive(Args, Debug)]
pub struct ForkCommand {
    /// The computer to copy, or a folder (./repo) to copy into a new Linux fork; with
    /// --recipe, the new fork's name.
    pub source: String,
    /// The fork's name; with -n, the prefix of the names (`<prefix>-1`, `<prefix>-2`).
    #[arg(long)]
    pub name: Option<String>,
    /// Make this many forks of the same checkpoint at once and start them.
    #[arg(
        short = 'n',
        long = "count",
        value_name = "N",
        conflicts_with = "recipe"
    )]
    pub count: Option<u32>,
    /// A command to run in each fork once it is started (after `--`); the fork is left for review.
    #[arg(last = true, value_name = "COMMAND", conflicts_with = "recipe")]
    pub command: Vec<String>,
    /// Fork from this checkpoint instead of the current state.
    #[arg(long, value_name = "ID")]
    pub checkpoint: Option<String>,
    /// Fork from the checkpoint taken after step N of the computer (see `silo steps`).
    #[arg(long, value_name = "N", conflicts_with_all = ["checkpoint", "recipe"])]
    pub from_step: Option<u32>,
    /// A Silo-managed secret the fork may use (repeatable).
    #[arg(long = "secret", value_name = "NAME")]
    pub secrets: Vec<String>,
    /// Fork from this recipe's cached build (building it first when needed), not from a computer.
    #[arg(long, value_name = "NAME", conflicts_with_all = ["checkpoint", "name"])]
    pub recipe: Option<String>,
    /// A recipe file whose quiet: and scrub: lists apply to the fork.
    #[arg(long, value_name = "PATH")]
    pub recipe_file: Option<PathBuf>,
    /// Start the fork and wait until its desktop and computer-use driver are ready, then
    /// print its name and computer-use status.
    #[arg(long, conflicts_with_all = ["count", "command", "recipe"])]
    pub gui: bool,
    /// The fork's computer-use approval: ask (the default) or auto, which lets an agent in
    /// the fork drive its desktop without asking. Only forks; main keeps asking.
    #[arg(
        long,
        value_name = "ask|auto",
        conflicts_with_all = ["count", "command", "recipe"]
    )]
    pub computer_use: Option<String>,
    /// How long --gui waits for the desktop and the driver.
    #[arg(long, value_name = "SECONDS", default_value_t = crate::gui::DEFAULT_TIMEOUT_SECS, requires = "gui")]
    pub gui_timeout: u64,
}

#[derive(Args, Debug)]
pub struct FoldCommand {
    /// The fork whose changes are kept.
    pub fork: String,
    /// The computer to keep into (default: the one the fork was made from).
    #[arg(long, value_name = "COMPUTER")]
    pub into: Option<String>,
    /// Report what would be kept and what would stop it, changing nothing.
    #[arg(long)]
    pub dry_run: bool,
    /// Make the fork main instead of keeping its changes: it takes main's name, settings and
    /// published ports, and the old main is archived (`silo undo main` swaps back).
    #[arg(long)]
    pub promote: bool,
    /// Carry database changes even though no migrate step covers them.
    #[arg(long)]
    pub data_ok: bool,
    /// The review digest from `silo diff --json`; the keep refuses if the fork has changed since.
    #[arg(long, value_name = "DIGEST")]
    pub reviewed: Option<String>,
    /// A recipe whose keep: section applies.
    #[arg(long, value_name = "PATH")]
    pub recipe_file: Option<PathBuf>,
    /// Ask for a review instead of keeping: records a fold request for the fork and prints its id.
    #[arg(long, conflicts_with_all = ["promote", "dry_run", "pr"])]
    pub request: bool,
    /// Push the fork's code to a new branch `silo/<fork>-<id>` and open a pull request; keeps
    /// nothing. Needs GITHUB_TOKEN or `gh auth login` on this device.
    #[arg(long, conflicts_with_all = ["promote", "dry_run", "into"])]
    pub pr: bool,
    /// With --pr: the GitHub repository (OWNER/NAME; default GITHUB_REPOSITORY, else origin).
    #[arg(long, value_name = "OWNER/NAME", requires = "pr")]
    pub repo: Option<String>,
    /// With --pr: the branch to merge into (default: the branch the folder is on, or the
    /// repository's default branch).
    #[arg(long, value_name = "BRANCH", requires = "pr")]
    pub base: Option<String>,
    /// Do not append kept packages and settings files to the recipe the computer came from.
    #[arg(long)]
    pub no_write_back: bool,
}

#[derive(Args, Debug)]
#[command(args_conflicts_with_subcommands = true)]
pub struct RequestsCommand {
    #[command(subcommand)]
    pub action: Option<RequestsAction>,
    /// Which requests to list: open (default), kept, thrown-away or all.
    #[arg(long, value_name = "STATUS")]
    pub status: Option<String>,
    /// List this device's requests and every connected device's. `show`, `keep`,
    /// `throw-away` and `open` find the device a request is on by themselves.
    #[arg(long)]
    pub all_devices: bool,
}

#[derive(Args, Debug)]
pub struct ApprovalsCommand {
    #[command(subcommand)]
    pub action: Option<ApprovalsAction>,
}

#[derive(Subcommand, Debug)]
pub enum ApprovalsAction {
    /// What the request asks, the agent's note, and what became of it.
    Show { id: String },
    /// Allow the request: Silo checks it again and runs its call.
    Allow { id: String },
    /// Deny the request; the agent reads your note.
    Deny {
        id: String,
        #[arg(long, value_name = "TEXT")]
        note: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum RequestsAction {
    /// A request with a fresh diff of its fork and the credentials the fork used.
    Show {
        id: String,
        /// Also send the diff to the hosted reviewer and print what it finds.
        #[arg(long)]
        review: bool,
        /// With --review: general, packages, code or all (default).
        #[arg(long, value_name = "LENS", requires = "review")]
        lens: Option<String>,
    },
    /// Keep the request's fork into its target, pinned to the digest the request was made from.
    Keep {
        id: String,
        /// Carry database changes even though no migrate step covers them.
        #[arg(long)]
        data_ok: bool,
        /// A recipe whose keep: section applies.
        #[arg(long, value_name = "PATH")]
        recipe_file: Option<PathBuf>,
    },
    /// Drop the request's fork without keeping it. main and the target are never dropped.
    ThrowAway { id: String },
    /// Start the request's fork if it is stopped and say how to reach it.
    Open { id: String },
}

#[derive(Args, Debug)]
pub struct DiffCommand {
    pub computer: String,
    /// Cut the text to this many lines (default 15).
    #[arg(long, value_name = "N")]
    pub max_lines: Option<u32>,
    /// Show the idle fork's own background changes too.
    #[arg(long)]
    pub no_baseline: bool,
    /// Measure the no-op baseline first: the computer must be a fork that has sat idle.
    #[arg(long, conflicts_with = "no_baseline")]
    pub baseline: bool,
}

#[derive(Args, Debug)]
pub struct DropCommand {
    pub computer: String,
    /// Allow dropping main.
    #[arg(long)]
    pub force: bool,
}

#[derive(Args, Debug)]
pub struct ExecCommand {
    pub computer: String,
    /// Working directory in the computer.
    #[arg(long)]
    pub cwd: Option<String>,
    /// Seconds before the command is stopped (at most 180).
    #[arg(long, default_value_t = ops::MAX_EXEC_SECS)]
    pub timeout: u64,
    /// Run with no time limit and print the output as it arrives; the exit code is the
    /// command's. The command is not recorded as a step, so nothing is checkpointed.
    #[arg(long, conflicts_with = "timeout")]
    pub stream: bool,
    /// With --stream: give the command this process's standard input, read to its end, as
    /// its own. It travels to the engine in the request, so it appears in no command line.
    #[arg(long, requires = "stream")]
    pub stdin: bool,
    #[arg(last = true, required = true, value_name = "COMMAND")]
    pub command: Vec<String>,
}

#[derive(Args, Debug)]
pub struct CheckpointCommand {
    pub computer: String,
    #[arg(long)]
    pub name: Option<String>,
    /// manual, automatic or before-keep.
    #[arg(long)]
    pub kind: Option<String>,
}

/// The secret value from stdin, without its final newline. Never taken from an argument.
fn read_secret_value() -> Result<String, CliError> {
    use std::io::Read;
    let mut value = String::new();
    std::io::stdin()
        .read_to_string(&mut value)
        .map_err(|error| CliError::new(format!("Cannot read the secret from stdin: {error}")))?;
    Ok(trim_newline(value))
}

/// The most standard input `silo exec --stream --stdin` forwards.
const MAX_EXEC_INPUT_BYTES: u64 = 1 << 20;

/// This process's standard input to its end, for the command `exec --stream --stdin` runs.
fn read_exec_input() -> Result<Vec<u8>, CliError> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(MAX_EXEC_INPUT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| CliError::new(format!("Cannot read standard input: {error}")))?;
    if bytes.len() as u64 > MAX_EXEC_INPUT_BYTES {
        return Err(CliError::new(format!(
            "Standard input is longer than {MAX_EXEC_INPUT_BYTES} bytes."
        )));
    }
    Ok(bytes)
}

fn trim_newline(mut value: String) -> String {
    if value.ends_with('\n') {
        value.pop();
        if value.ends_with('\r') {
            value.pop();
        }
    }
    value
}

fn stderr_progress(line: &str) {
    eprintln!("{line}");
}

fn emit(json: bool, value: &Value, human: impl FnOnce(&Value) -> String) {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(value).unwrap_or_default()
        );
    } else {
        println!("{}", human(value));
    }
}

/// Runs one command and returns the process exit code.
pub async fn run(cli: Cli, working_directory: &Path) -> Result<i32, CliError> {
    if matches!(cli.command, Command::Host(_) | Command::Devices(_)) {
        // They change this build's own engine and its SSH access, never another channel's.
        if cli.allow_other_channel {
            return Err(CliError::new(
                "silo host and silo devices work on this build's own engine, so --allow-other-channel cannot be used with them.",
            ));
        }
        let socket = default_socket(cli.socket.clone(), false)?;
        return run_connections(cli, socket).await;
    }
    let transport = crate::endpoint::for_this_process(cli.socket.clone(), cli.allow_other_channel)?;
    if matches!(cli.command, Command::Mcp) {
        crate::mcp::serve(transport, working_directory.to_owned(), cli.agent.clone()).await?;
        return Ok(0);
    }
    let project = Project::discover(working_directory);
    // Read before connecting so a bad file is reported without the engine.
    let settings = project.load_settings(cli.agent.as_deref())?;
    if let Command::Fork(fork) = &cli.command {
        if let Some(mode) = fork
            .computer_use
            .as_deref()
            .map(crate::gui::parse_approval)
            .transpose()?
        {
            // A named agent (`--agent` or SILO_AGENT) is asking like an MCP client would.
            crate::gui::check_auto(mode, settings.as_ref(), cli.agent.is_some())?;
        }
    }
    let recipe = match &cli.command {
        Command::Fork(fork) => match &fork.recipe {
            Some(name) => Some(project.named_recipe(name)?),
            None if folder::is_folder(&fork.source) => {
                Some(project.named_recipe(crate::config::DEFAULT_RECIPE)?)
            }
            None => ops::resolve_recipe(fork.recipe_file.as_deref(), &project)?,
        },
        Command::Up(up) => Some(
            project.named_recipe(
                up.recipe
                    .as_deref()
                    .unwrap_or(crate::config::DEFAULT_RECIPE),
            )?,
        ),
        Command::Recipe(RecipeCommand::Build { name, .. }) => Some(project.named_recipe(name)?),
        Command::Fold(fold) => ops::resolve_recipe(fold.recipe_file.as_deref(), &project)?,
        Command::Requests(RequestsCommand {
            action: Some(RequestsAction::Keep { recipe_file, .. }),
            ..
        }) => ops::resolve_recipe(recipe_file.as_deref(), &project)?,
        _ => None,
    };
    let engine = Engine::connect_to(&transport).await?;
    let engine = match cli.device.as_deref() {
        None | Some("local") => engine,
        Some(name) => {
            let id = engine.resolve_device(name).await?;
            engine.on_device(Some(id))
        }
    };
    refuse_on_device(&cli.command, &engine)?;
    let json = cli.json;
    if let Command::Tools(action) = &cli.command {
        let result = match action {
            ToolsCommand::Enable { computer, binary } => {
                crate::tools::enable(&engine, computer, binary.as_deref(), &stderr_progress).await?
            }
            ToolsCommand::Disable { computer } => crate::tools::disable(&engine, computer).await?,
            ToolsCommand::Status { computer } => crate::tools::status(&engine, computer).await?,
        };
        emit(json, &result, crate::tools::render);
        return Ok(0);
    }
    tokio::select! {
        result = dispatch(&engine, cli.command, json, settings, recipe, &project, working_directory) => result,
        _ = tokio::signal::ctrl_c() => {
            match engine.last_job() {
                Some(job) => eprintln!("Interrupted. {}", Engine::reattach_hint(&job)),
                None => eprintln!("Interrupted."),
            }
            Ok(130)
        }
    }
}

/// `silo host` and `silo devices`: this machine's own Connections, never another device's.
/// `run` gives it the build channel's own socket; tests give it theirs.
pub async fn run_connections(cli: Cli, socket: PathBuf) -> Result<i32, CliError> {
    use crate::hosts::{self, DevicesCommand, HostCommand};
    if cli.device.is_some() {
        return Err(CliError::new(
            "silo host and silo devices work on this device, so --device cannot be used with them.",
        ));
    }
    let json = cli.json;
    match cli.command {
        Command::Host(HostCommand::Init { authorize }) => {
            let setup = hosts::HostSetup::detect(socket, build_channel())?;
            emit(
                json,
                &hosts::init(&setup, &authorize).await?,
                hosts::render_host,
            );
        }
        Command::Host(HostCommand::Status) => {
            let setup = hosts::HostSetup::detect(socket, build_channel())?;
            emit(json, &hosts::status(&setup).await?, hosts::render_host);
        }
        Command::Host(HostCommand::Revoke { key }) => {
            let engine = Engine::connect(&socket).await?;
            emit(
                json,
                &hosts::revoke(&engine, &key).await?,
                hosts::render_host,
            );
        }
        Command::Devices(command) => {
            let engine = Engine::connect(&socket).await?;
            match command {
                DevicesCommand::Add {
                    address,
                    replace,
                    host_key,
                } => {
                    let request = hosts::AddRequest {
                        address,
                        replace,
                        host_key,
                        known_hosts: crate::access::home_dir()?.join(".ssh/known_hosts"),
                    };
                    let value = hosts::add_device(
                        &engine,
                        &request,
                        &hosts::SystemTerminal,
                        &stderr_progress,
                    )
                    .await?;
                    emit(json, &value, hosts::render_added);
                }
                DevicesCommand::List => emit(
                    json,
                    &hosts::list_devices(&engine).await?,
                    hosts::render_devices,
                ),
                DevicesCommand::Remove { name } => {
                    let value = hosts::remove_device(&engine, &name).await?;
                    // The engine forgets a device only after the owner removed every Silo key this
                    // device installed there, or found none: it does not say which.
                    emit(json, &value, |value| {
                        format!(
                            "Removed {}. No Silo key of this device is left there.",
                            value["removed"].as_str().unwrap_or_default()
                        )
                    });
                }
                DevicesCommand::Key => {
                    emit(json, &hosts::own_key(&engine).await?, hosts::render_key)
                }
            }
        }
        _ => unreachable!("only host and devices commands come here"),
    }
    Ok(0)
}

/// The commands that work on this device's own files, ports, SSH names or secrets.
fn refuse_on_device(command: &Command, engine: &Engine) -> Result<(), CliError> {
    if engine.device().is_none() {
        return Ok(());
    }
    let what = match command {
        Command::Up(_) => "up",
        Command::Ports { .. } => "ports",
        Command::Publish { .. } => "publish",
        Command::Unpublish { .. } => "unpublish",
        Command::SshSetup { .. } => "ssh-setup",
        Command::Network(_) => "network policy",
        Command::Secrets(_) => "secrets",
        Command::Fork(fork) if fork.recipe.is_none() && folder::is_folder(&fork.source) => {
            "fork of a folder"
        }
        Command::Fold(fold) if fold.pr => "fold --pr",
        Command::Undo { computer } if folder::is_folder(computer) => "undo of a folder",
        _ => return Ok(()),
    };
    Err(CliError::new(format!(
        "{what} works on this device, so --device cannot be used with it."
    )))
}

async fn dispatch(
    engine: &Engine,
    command: Command,
    json: bool,
    settings: Option<Settings>,
    recipe: Option<Recipe>,
    project: &Project,
    working_directory: &Path,
) -> Result<i32, CliError> {
    let progress = &stderr_progress;
    match command {
        Command::Up(up) => {
            let name = up
                .recipe
                .unwrap_or_else(|| crate::config::DEFAULT_RECIPE.to_owned());
            let recipe = recipe.ok_or_else(|| CliError::new("No recipe was loaded."))?;
            let home = access::home_dir();
            let ssh = home
                .as_ref()
                .ok()
                .map(|home| (home.as_path(), build_channel()));
            let mut value = ops::up(engine, &name, &recipe, ssh, progress).await?;
            if let Err(error) = home {
                value["ssh"] = serde_json::json!({ "error": error.message });
            }
            emit(json, &value, render::up);
        }
        Command::Recipe(RecipeCommand::Ls) => emit(
            json,
            &ops::recipe_list(engine, project).await?,
            render::recipes,
        ),
        Command::Recipe(RecipeCommand::Build { name, force }) => {
            let recipe = recipe.ok_or_else(|| CliError::new("No recipe was loaded."))?;
            let value = ops::build_recipe(engine, &name, &recipe, force, progress).await?;
            emit(json, &value, render::recipe_built);
        }
        Command::Recipe(RecipeCommand::Rm { name }) => {
            let value = ops::recipe_remove(engine, &name).await?;
            emit(json, &value, |v| {
                format!(
                    "Removed the build of {}.",
                    v["removed"].as_str().unwrap_or_default()
                )
            });
        }
        Command::List { all, all_devices } => {
            let value = if all_devices {
                ops::list_everywhere(engine, all).await?
            } else if all {
                ops::list_all(engine).await?
            } else {
                ops::list(engine).await?
            };
            emit(json, &value, render::list)
        }
        Command::Status { computer } => {
            emit(json, &ops::status(engine, &computer).await?, render::status)
        }
        Command::Create(create) => {
            let args = CreateArgs {
                name: create.name,
                platform: if create.macos {
                    Platform::Macos
                } else if create.container {
                    Platform::Container
                } else {
                    Platform::Linux
                },
                cpus: create.cpus,
                memory_gib: create.memory,
                disk_gib: create.disk,
                no_desktop: create.no_desktop,
            };
            let value = ops::create(engine, &args, progress).await?;
            emit(json, &value, |v| render::computer_line("Created", v));
        }
        Command::Start { computer } => {
            let value = ops::start(engine, &computer, progress).await?;
            emit(json, &value, |v| render::computer_line("Started", v));
        }
        Command::Stop { computer } => {
            let value = ops::stop(engine, &computer, progress).await?;
            emit(json, &value, |v| render::computer_line("Stopped", v));
        }
        Command::Desktop { state, computer } => {
            let on = state == DesktopState::On;
            let value = ops::desktop_set(engine, &computer, on, progress).await?;
            emit(json, &value, |v| {
                format!(
                    "The desktop of {} is {}.",
                    v["computer"].as_str().unwrap_or_default(),
                    if on { "on" } else { "off" }
                )
            });
        }
        Command::RetrySetup { computer } => {
            let value = ops::retry_setup(engine, &computer, progress).await?;
            emit(json, &value, |v| render::computer_line("Set up", v));
        }
        Command::Fork(mut fork) => {
            if let Some(number) = fork.from_step {
                if folder::is_folder(&fork.source) {
                    return Err(CliError::new("--from-step needs a computer, not a folder."));
                }
                fork.checkpoint = Some(steps::checkpoint_for(engine, &fork.source, number).await?);
            }
            if fork.recipe.is_none()
                && folder::is_folder(&fork.source)
                && (fork.gui || fork.computer_use.is_some())
            {
                return Err(CliError::new(
                    "--gui and --computer-use fork a computer; a folder is copied into a Linux fork that has no desktop.",
                ));
            }
            if fork.recipe.is_none() && folder::is_folder(&fork.source) {
                return fork_folder(
                    engine,
                    fork,
                    json,
                    settings.as_ref(),
                    recipe,
                    project,
                    working_directory,
                )
                .await;
            }
            if fork.count.is_some() || !fork.command.is_empty() {
                let args = ManyArgs {
                    fork: ForkArgs {
                        source: fork.source,
                        name: fork.name,
                        checkpoint: fork.checkpoint,
                        secrets: fork.secrets,
                        recipe,
                        recipe_name: None,
                    },
                    count: fork.count.unwrap_or(1),
                    command: fork.command,
                    cwd: None,
                };
                return fork_many(engine, &args, json, settings.as_ref(), project).await;
            }
            let recipe_name = fork.recipe;
            let (source, name) = match &recipe_name {
                Some(_) => (String::new(), Some(fork.source)),
                None => (fork.source, fork.name),
            };
            let args = ForkArgs {
                source,
                name,
                checkpoint: fork.checkpoint,
                secrets: fork.secrets,
                recipe,
                recipe_name,
            };
            let approval = fork
                .computer_use
                .as_deref()
                .map(crate::gui::parse_approval)
                .transpose()?;
            if fork.gui {
                let gui = crate::gui::GuiArgs {
                    fork: args,
                    approval,
                    timeout: std::time::Duration::from_secs(fork.gui_timeout.max(1)),
                };
                let value = crate::gui::fork_gui(engine, &gui, settings.as_ref(), progress).await?;
                emit(json, &value, render::forked_gui);
                return Ok(0);
            }
            let mut value = ops::fork(engine, &args, settings.as_ref(), progress).await?;
            if let Some(mode) = approval {
                crate::gui::apply_to_fork(engine, &mut value, mode).await?;
            }
            emit(json, &value, render::forked);
        }
        Command::Fold(fold) if fold.promote => {
            let mut value = ops::promote(
                engine,
                &fold.fork,
                fold.into.as_deref(),
                fold.dry_run,
                progress,
            )
            .await?;
            if let Ok(home) = access::home_dir() {
                access::ssh_after_change(engine, &mut value, &home, build_channel()).await;
            }
            emit(json, &value, render::promote);
            return Ok(if value["status"] == "refused" { 2 } else { 0 });
        }
        Command::Fold(fold) => {
            let context = flows::Context {
                project,
                settings: settings.as_ref(),
                recipe: recipe.as_ref(),
                write_back: !fold.no_write_back && engine.device().is_none(),
            };
            let mode = if fold.pr {
                flows::Mode::PullRequest(
                    pr::PrArgs {
                        repo: fold.repo,
                        base: fold.base,
                        host_root: project
                            .directory
                            .as_ref()
                            .and_then(|silo| silo.parent())
                            .map(std::path::Path::to_path_buf),
                    },
                    pr::PrEnv::from_process(),
                )
            } else if fold.request {
                flows::Mode::Request
            } else {
                flows::Mode::Keep
            };
            let value = flows::run(
                engine,
                &fold.fork,
                fold.into.as_deref(),
                &context,
                ops::FoldOptions {
                    dry_run: fold.dry_run,
                    data_ok: fold.data_ok,
                    reviewed: fold.reviewed,
                },
                mode,
                progress,
            )
            .await?;
            if value["status"] == "pull-request" {
                emit(json, &value, render::pull_request);
                return Ok(0);
            }
            if value["status"] == "requested" {
                emit(json, &value, render::requested);
                return Ok(0);
            }
            emit(json, &value, render::fold);
            return Ok(if value["status"] == "refused" { 2 } else { 0 });
        }
        Command::Requests(requests) => match requests.action {
            None => {
                let status = requests
                    .status
                    .as_deref()
                    .map(review::parse_filter)
                    .transpose()?;
                let value = if requests.all_devices {
                    review::list_all(engine, status).await?
                } else {
                    review::list(engine, status).await?
                };
                emit(json, &value, render::requests_list);
            }
            Some(RequestsAction::Show { id, review, lens }) => {
                let engine = &review::locate(engine, &id).await?;
                let mut value = review::show(engine, &id, progress).await?;
                if review {
                    let lenses = hosted::parse_lenses(lens.as_deref())?;
                    let reviewed =
                        hosted::review_target(engine, settings.as_ref(), &id, &lenses, progress)
                            .await?;
                    value["review"] = reviewed;
                }
                emit(json, &value, |v| {
                    let mut text = render::request_show(v);
                    if v["review"].is_object() {
                        text.push_str(&format!("\n\n{}", hosted::render(&v["review"])));
                    }
                    text
                });
            }
            Some(RequestsAction::Keep { id, data_ok, .. }) => {
                let engine = &review::locate(engine, &id).await?;
                let hooks = ops::keep_hooks(recipe.as_ref());
                let value = review::keep(engine, &id, hooks, data_ok, progress).await?;
                emit(json, &value, render::fold);
                return Ok(if value["status"] == "refused" { 2 } else { 0 });
            }
            Some(RequestsAction::ThrowAway { id }) => {
                let engine = &review::locate(engine, &id).await?;
                let value = review::throw_away(engine, &id, progress).await?;
                emit(json, &value, render::request_thrown_away);
            }
            Some(RequestsAction::Open { id }) => {
                let engine = &review::locate(engine, &id).await?;
                let value = review::open(engine, &id, progress).await?;
                emit(json, &value, render::request_open);
            }
        },
        Command::Approvals(approvals) => {
            use silo_api::methods::approvals::ApprovalDecision;
            match approvals.action {
                None => emit(
                    json,
                    &approvals::list(engine).await?,
                    approvals::render_list,
                ),
                Some(ApprovalsAction::Show { id }) => emit(
                    json,
                    &approvals::show(engine, &id).await?,
                    approvals::render_show,
                ),
                Some(ApprovalsAction::Allow { id }) => emit(
                    json,
                    &approvals::decide(engine, &id, ApprovalDecision::Allow, None).await?,
                    approvals::render_show,
                ),
                Some(ApprovalsAction::Deny { id, note }) => emit(
                    json,
                    &approvals::decide(engine, &id, ApprovalDecision::Deny, note).await?,
                    approvals::render_show,
                ),
            }
        }
        Command::Undo { computer } => {
            let mut value = flows::undo(engine, &computer, working_directory, progress).await?;
            if let Ok(home) = access::home_dir() {
                access::ssh_after_change(engine, &mut value, &home, build_channel()).await;
            }
            emit(json, &value, render::undo);
            return Ok(if value["status"] == "refused" { 2 } else { 0 });
        }
        Command::Review { target, lens } => {
            let lenses = hosted::parse_lenses(lens.as_deref())?;
            let value =
                hosted::review_target(engine, settings.as_ref(), &target, &lenses, progress)
                    .await?;
            emit(json, &value, hosted::render);
        }
        Command::Diff(diff) => {
            let args = DiffArgs {
                computer: diff.computer,
                max_lines: diff.max_lines,
                no_baseline: diff.no_baseline,
                compute_baseline: diff.baseline,
            };
            let value = ops::diff(engine, &args, progress).await?;
            emit(json, &value, |v| {
                let mut text = v["text"].as_str().unwrap_or_default().to_owned();
                if v["credentialsUsed"].is_object() {
                    text.push_str(&format!(
                        "\n{}",
                        render::credentials_line(&v["credentialsUsed"])
                    ));
                }
                if let Some(digest) = v["reviewDigest"].as_str().filter(|d| !d.is_empty()) {
                    text.push_str(&format!("\nreview: {digest}"));
                }
                match v["baselineError"].as_str() {
                    Some(error) => format!("{error}\n\n{text}"),
                    None => text,
                }
            });
        }
        Command::Drop(drop) => {
            let value = ops::drop_computer(engine, &drop.computer, drop.force, progress).await?;
            emit(json, &value, |v| {
                format!("Dropped {}", v["dropped"].as_str().unwrap_or_default())
            });
        }
        Command::Exec(exec) if exec.stream => {
            let stdin = if exec.stdin {
                Some(read_exec_input()?)
            } else {
                None
            };
            let computer = engine.computer(&exec.computer).await?;
            let (sink, _) = output_sink(json, false);
            let (mut out, mut err) = (fanout::Capture::default(), fanout::Capture::default());
            let ran = fanout::exec_streaming_with_input(
                engine,
                &computer,
                &exec.command,
                exec.cwd.as_deref(),
                stdin,
                &stderr_progress,
                |is_err, bytes| {
                    if is_err {
                        err.push(bytes);
                    } else {
                        out.push(bytes);
                    }
                    sink(&computer.name, is_err, bytes);
                },
            )
            .await?;
            if json {
                emit(
                    true,
                    &serde_json::json!({
                        "exitCode": ran.exit_code,
                        "signal": ran.signal,
                        "stdout": String::from_utf8_lossy(&out.bytes),
                        "stderr": String::from_utf8_lossy(&err.bytes),
                        "outputTruncated": out.truncated || err.truncated,
                    }),
                    |_| String::new(),
                );
            }
            return Ok(ran.exit_code);
        }
        Command::Exec(exec) => {
            let args = ExecArgs {
                computer: exec.computer,
                argv: exec.command,
                cwd: exec.cwd,
                timeout_secs: exec.timeout,
            };
            let step_settings = Settings::step_settings(settings.as_ref());
            let (output, step) = steps::run_exec(engine, &args, step_settings, progress).await?;
            return Ok(print_exec(json, &output, &step));
        }
        Command::Checkpoint(checkpoint) => {
            let kind = checkpoint
                .kind
                .as_deref()
                .map(ops::parse_checkpoint_kind)
                .transpose()?;
            let value = ops::checkpoint(
                engine,
                &checkpoint.computer,
                checkpoint.name.as_deref(),
                kind,
                progress,
            )
            .await?;
            emit(json, &value, |v| {
                format!(
                    "Checkpoint {} ({})",
                    v["name"].as_str().unwrap_or_default(),
                    v["id"].as_str().unwrap_or_default()
                )
            });
        }
        Command::Ports { computer } => emit(
            json,
            &access::ports(engine, &computer).await?,
            render::ports,
        ),
        Command::Publish {
            computer,
            port,
            http,
        } => emit(
            json,
            &access::publish(engine, &computer, port, http).await?,
            render::ports,
        ),
        Command::Unpublish { computer, port } => emit(
            json,
            &access::unpublish(engine, &computer, port).await?,
            render::ports,
        ),
        Command::Network(NetworkCommand::Policy {
            computer,
            mode,
            allow,
            remove,
            presets,
        }) => {
            use silo_api::methods::network::NetworkPolicyMode;
            let change = access::PolicyChange {
                mode: mode.map(|mode| match mode {
                    PolicyMode::Open => NetworkPolicyMode::Open,
                    PolicyMode::Allowlist => NetworkPolicyMode::Allowlist,
                    PolicyMode::Closed => NetworkPolicyMode::Closed,
                }),
                allow,
                remove,
                presets,
            };
            emit(
                json,
                &access::network_policy(engine, &computer, &change).await?,
                render::network_policy,
            );
        }
        Command::SshSetup { computer } => {
            let home = access::home_dir()?;
            let value = access::ssh_setup(engine, &computer, &home, build_channel()).await?;
            emit(json, &value, render::ssh);
        }
        Command::Export(export) => {
            let args = crate::share::ExportArgs {
                computer: export.computer,
                output: export.output,
                checkpoint: export.checkpoint,
                yes: export.yes,
                include_logins: export.include_logins,
            };
            let value = crate::share::export(
                engine,
                &args,
                working_directory,
                &crate::share::Terminal,
                progress,
            )
            .await?;
            emit(json, &value, render::exported);
        }
        Command::Import(import) => {
            let value = crate::share::import(
                engine,
                &import.file,
                import.name.as_deref(),
                working_directory,
                progress,
            )
            .await?;
            emit(json, &value, render::imported);
        }
        Command::Checkpoints { computer } => emit(
            json,
            &ops::checkpoints(engine, &computer).await?,
            render::checkpoints,
        ),
        Command::Steps { computer } => {
            emit(json, &steps::list(engine, &computer).await?, render::steps)
        }
        Command::Tree => emit(json, &steps::tree(engine).await?, render::tree),
        Command::Secrets(secrets) => {
            let value = match secrets {
                SecretsCommand::List => ops::secrets_list(engine).await?,
                SecretsCommand::Set {
                    name,
                    computers,
                    domains,
                } => {
                    let value = read_secret_value()?;
                    ops::secret_set(engine, &name, value, &computers, &domains).await?
                }
                SecretsCommand::Assign { name, computer } => {
                    ops::secret_assign(engine, &name, &computer, true).await?
                }
                SecretsCommand::Unassign { name, computer } => {
                    ops::secret_assign(engine, &name, &computer, false).await?
                }
            };
            emit(json, &value, render::secrets);
        }
        Command::Job { id } => {
            let value = ops::job(engine, &id, progress).await?;
            emit(json, &value, |v| {
                serde_json::to_string_pretty(v).unwrap_or_default()
            });
        }
        Command::Mcp | Command::Tools(_) | Command::Host(_) | Command::Devices(_) => {
            unreachable!("handled above")
        }
    }
    Ok(0)
}

fn write_all(mut stream: impl Write, bytes: &[u8]) {
    let _ = stream.write_all(bytes);
    let _ = stream.flush();
}

/// Streams a command's output, one fork's as it comes or several with the fork's name in
/// front of each line. Nothing is written in JSON mode, where the result carries the text.
fn output_sink(
    json: bool,
    several: bool,
) -> (
    impl Fn(&str, bool, &[u8]) + Sync,
    std::sync::Arc<std::sync::Mutex<fanout::Prefixer>>,
) {
    let prefixer = std::sync::Arc::new(std::sync::Mutex::new(fanout::Prefixer::default()));
    let held = prefixer.clone();
    let sink = move |name: &str, is_err: bool, bytes: &[u8]| {
        if json {
            return;
        }
        let bytes = if several {
            held.lock().unwrap().push(name, is_err, bytes)
        } else {
            bytes.to_vec()
        };
        write_stream(is_err, &bytes);
    };
    (sink, prefixer)
}

fn write_stream(is_err: bool, bytes: &[u8]) {
    if is_err {
        write_all(std::io::stderr(), bytes);
    } else {
        write_all(std::io::stdout(), bytes);
    }
}

/// Writes the unfinished lines still held once the commands have ended.
fn flush_partial(json: bool, prefixer: &std::sync::Mutex<fanout::Prefixer>) {
    if json {
        return;
    }
    for (is_err, line) in prefixer.lock().unwrap().finish() {
        write_stream(is_err, &line);
    }
}

async fn fork_many(
    engine: &Engine,
    args: &ManyArgs,
    json: bool,
    settings: Option<&Settings>,
    project: &Project,
) -> Result<i32, CliError> {
    let several = args.count > 1;
    let (sink, prefixer) = output_sink(json, several);
    let mut value = fanout::fork_many(engine, args, settings, &stderr_progress, &sink).await?;
    flush_partial(json, &prefixer);
    let context = flows::Context {
        project,
        settings,
        recipe: args.fork.recipe.as_ref(),
        // Recipe files are this device's; a keep on another device leaves them alone, as
        // `fold` does.
        write_back: engine.device().is_none(),
    };
    let kept = keep_after_commands(engine, &mut value, &context, json).await;
    if json {
        emit(true, &value, |_| String::new());
    } else if args.command.is_empty() || several {
        println!("{}", render::forks(&value));
    } else {
        // A single command's output is on stdout, so the summary goes to stderr.
        eprintln!("{}", render::forks(&value));
    }
    if !json {
        for line in kept {
            eprintln!("{line}");
        }
    }
    Ok(fanout::exit_code(&value))
}

/// Applies the `auto_keep` rules to each fork whose command ran (see
/// [`flows::after_command`]); the results go to `autoKeep` of the JSON, the text lines are
/// returned.
async fn keep_after_commands(
    engine: &Engine,
    value: &mut Value,
    context: &flows::Context<'_>,
    json: bool,
) -> Vec<String> {
    let mut results = Vec::new();
    let mut lines = Vec::new();
    let forks = value["forks"].as_array().cloned().unwrap_or_default();
    for fork in forks.iter().filter(|fork| fork.get("exitCode").is_some()) {
        let name = fork["name"].as_str().unwrap_or_default();
        let ok = fork.get("error").is_none() && fork["exitCode"].as_i64() == Some(0);
        if let Some(result) = flows::after_command(engine, name, ok, context, &|_| {}).await {
            lines.push(render::auto_keep(&result));
            results.push(result);
        }
    }
    if !results.is_empty() {
        value["autoKeep"] = Value::Array(results);
    }
    let _ = json;
    lines
}

async fn fork_folder(
    engine: &Engine,
    fork: ForkCommand,
    json: bool,
    settings: Option<&Settings>,
    recipe: Option<Recipe>,
    project: &Project,
    working_directory: &Path,
) -> Result<i32, CliError> {
    if fork.count.unwrap_or(1) != 1 {
        return Err(CliError::new(
            "-n makes forks of a computer; a folder is copied into one fork.",
        ));
    }
    if fork.checkpoint.is_some() {
        return Err(CliError::new("--checkpoint does not apply to a folder."));
    }
    let recipe = recipe.ok_or_else(|| CliError::new("No recipe was loaded."))?;
    let directory = working_directory.join(&fork.source);
    let value = folder::fork_folder(
        engine,
        &directory,
        fork.name.as_deref(),
        &recipe,
        settings,
        &fork.secrets,
        &stderr_progress,
    )
    .await?;
    if fork.command.is_empty() {
        emit(json, &value, render::folder_fork);
        return Ok(0);
    }
    let computer = engine
        .computer(value["name"].as_str().unwrap_or_default())
        .await?;
    let (sink, _) = output_sink(json, false);
    let (mut out, mut err) = (fanout::Capture::default(), fanout::Capture::default());
    let ran = fanout::exec_streaming(
        engine,
        &computer,
        &fork.command,
        value["guestPath"].as_str(),
        &stderr_progress,
        |is_err, bytes| {
            if is_err {
                err.push(bytes);
            } else {
                out.push(bytes);
            }
            sink(&computer.name, is_err, bytes);
        },
    )
    .await?;
    let context = flows::Context {
        project,
        settings,
        recipe: Some(&recipe),
        write_back: true,
    };
    let kept = flows::after_command(
        engine,
        &computer.name,
        ran.exit_code == 0,
        &context,
        &|_| {},
    )
    .await;
    if json {
        let mut value = value;
        if let Some(kept) = &kept {
            value["autoKeep"] = serde_json::json!([kept]);
        }
        value["exitCode"] = serde_json::json!(ran.exit_code);
        value["signal"] = serde_json::json!(ran.signal);
        value["stdout"] = serde_json::json!(String::from_utf8_lossy(&out.bytes));
        value["stderr"] = serde_json::json!(String::from_utf8_lossy(&err.bytes));
        value["outputTruncated"] = serde_json::json!(out.truncated || err.truncated);
        emit(true, &value, |_| String::new());
    } else {
        eprintln!("{}", render::folder_fork(&value));
        if let Some(kept) = &kept {
            eprintln!("{}", render::auto_keep(kept));
        }
    }
    Ok(ran.exit_code)
}

/// Prints an exec result and returns the process exit code. The text mode writes the
/// output bytes as they are; `--json` carries them as text with invalid UTF-8 replaced.
fn print_exec(json: bool, output: &ops::ExecOutput, step: &Value) -> i32 {
    if json {
        let mut value = output.to_json();
        value["step"] = step.clone();
        println!(
            "{}",
            serde_json::to_string_pretty(&value).unwrap_or_default()
        );
    } else {
        let _ = std::io::stdout().write_all(&output.stdout);
        let _ = std::io::stderr().write_all(&output.stderr);
        eprintln!("{}", steps::note(step));
    }
    output.process_exit_code()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Cli {
        Cli::try_parse_from(std::iter::once("silo").chain(args.iter().copied())).unwrap()
    }

    #[test]
    fn fork_takes_repeated_secrets_and_options() {
        let cli = parse(&[
            "fork",
            "main",
            "--name",
            "try",
            "--checkpoint",
            "c1",
            "--secret",
            "A",
            "--secret",
            "B",
            "--recipe-file",
            "r.yaml",
        ]);
        let Command::Fork(fork) = cli.command else {
            panic!("not a fork")
        };
        assert_eq!(fork.source, "main");
        assert_eq!(fork.name.as_deref(), Some("try"));
        assert_eq!(fork.checkpoint.as_deref(), Some("c1"));
        assert_eq!(fork.secrets, ["A", "B"]);
        assert_eq!(fork.recipe_file, Some(PathBuf::from("r.yaml")));
    }

    #[test]
    fn exec_keeps_everything_after_the_separator() {
        let cli = parse(&["exec", "main", "--", "ls", "-la", "--color"]);
        let Command::Exec(exec) = cli.command else {
            panic!("not an exec")
        };
        assert_eq!(exec.command, ["ls", "-la", "--color"]);
        assert!(Cli::try_parse_from(["silo", "exec", "main"]).is_err());
    }

    #[test]
    fn exec_stream_and_stdin_are_opt_in_and_only_stdin_needs_the_stream() {
        let cli = parse(&[
            "exec", "main", "--stream", "--stdin", "--", "sh", "-c", "cat",
        ]);
        let Command::Exec(exec) = cli.command else {
            panic!("not an exec")
        };
        assert!(exec.stream && exec.stdin);
        assert_eq!(exec.command, ["sh", "-c", "cat"]);
        let Command::Exec(plain) = parse(&["exec", "main", "--", "ls"]).command else {
            panic!("not an exec")
        };
        assert!(!plain.stream && !plain.stdin);
        for bad in [
            vec!["silo", "exec", "main", "--stdin", "--", "ls"],
            vec![
                "silo",
                "exec",
                "main",
                "--stream",
                "--timeout",
                "5",
                "--",
                "ls",
            ],
        ] {
            assert!(Cli::try_parse_from(bad.clone()).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn json_and_socket_are_global() {
        let cli = parse(&["list", "--json", "--socket", "/x.sock"]);
        assert!(cli.json);
        assert_eq!(cli.socket, Some(PathBuf::from("/x.sock")));
    }

    #[test]
    fn create_flags_conflict() {
        assert!(Cli::try_parse_from(["silo", "create", "a", "--linux", "--macos"]).is_err());
        assert!(Cli::try_parse_from(["silo", "create", "a", "--container", "--macos"]).is_err());
        assert!(Cli::try_parse_from(["silo", "create", "a", "--container", "--linux"]).is_err());
        let Command::Create(create) = Cli::try_parse_from(["silo", "create", "a", "--container"])
            .unwrap()
            .command
        else {
            panic!("expected create");
        };
        assert!(create.container && !create.macos && !create.linux);
        let cli = parse(&[
            "create", "a", "--macos", "--cpus", "6", "--memory", "12", "--disk", "80",
        ]);
        let Command::Create(create) = cli.command else {
            panic!("not a create")
        };
        assert!(create.macos);
        assert_eq!(
            (create.cpus, create.memory, create.disk),
            (Some(6), Some(12), Some(80))
        );
    }

    #[test]
    fn a_linux_or_container_computer_can_be_created_without_a_desktop() {
        assert!(Cli::try_parse_from(["silo", "create", "a", "--macos", "--no-desktop"]).is_err());
        for flags in [&["--no-desktop"][..], &["--container", "--no-desktop"]] {
            let mut line = vec!["create", "a"];
            line.extend_from_slice(flags);
            let Command::Create(create) = parse(&line).command else {
                panic!("not a create")
            };
            assert!(create.no_desktop);
        }
        let Command::Create(create) = parse(&["create", "a"]).command else {
            panic!("not a create")
        };
        assert!(!create.no_desktop);
    }

    #[test]
    fn secrets_commands_parse_and_never_take_a_value_argument() {
        let Command::Secrets(SecretsCommand::Set {
            name,
            computers,
            domains,
        }) = parse(&[
            "secrets",
            "set",
            "API_KEY",
            "--computer",
            "main",
            "--domain",
            "api.example.com",
        ])
        .command
        else {
            panic!("not a secrets set")
        };
        assert_eq!(
            (name.as_str(), computers.len(), domains.len()),
            ("API_KEY", 1, 1)
        );
        assert!(Cli::try_parse_from(["silo", "secrets", "set", "API_KEY", "the-value"]).is_err());
        assert!(matches!(
            parse(&["secrets", "assign", "API_KEY", "fork"]).command,
            Command::Secrets(SecretsCommand::Assign { .. })
        ));
        assert!(matches!(
            parse(&["secrets", "unassign", "API_KEY", "fork"]).command,
            Command::Secrets(SecretsCommand::Unassign { .. })
        ));
        assert!(matches!(
            parse(&["secrets", "list"]).command,
            Command::Secrets(SecretsCommand::List)
        ));
    }

    #[test]
    fn one_final_newline_is_dropped_from_a_piped_value() {
        assert_eq!(trim_newline("abc\n".into()), "abc");
        assert_eq!(trim_newline("abc\r\n".into()), "abc");
        assert_eq!(trim_newline("abc\n\n".into()), "abc\n");
        assert_eq!(trim_newline("abc".into()), "abc");
    }

    #[test]
    fn pr_cannot_be_combined_with_request_into_or_dry_run() {
        for extra in [["--request"], ["--dry-run"]] {
            assert!(Cli::try_parse_from(["silo", "fold", "try", "--pr", extra[0]]).is_err());
        }
        assert!(Cli::try_parse_from(["silo", "fold", "try", "--pr", "--into", "main"]).is_err());
        assert!(Cli::try_parse_from(["silo", "fold", "try", "--repo", "a/b"]).is_err());
        assert!(Cli::try_parse_from(["silo", "fold", "try", "--pr", "--repo", "a/b"]).is_ok());
        // The agent comes from the option, which wins over the environment.
        let cli = parse(&["--agent", "careful", "list"]);
        assert_eq!(cli.agent.as_deref(), Some("careful"));
    }

    #[test]
    fn fold_takes_a_fork_a_target_and_dry_run() {
        let Command::Fold(fold) = parse(&["fold", "try", "--into", "main", "--dry-run"]).command
        else {
            panic!("not a fold")
        };
        assert_eq!(
            (fold.fork.as_str(), fold.into.as_deref(), fold.dry_run),
            ("try", Some("main"), true)
        );
        assert!(matches!(
            parse(&["undo", "main"]).command,
            Command::Undo { .. }
        ));
    }

    #[test]
    fn diff_takes_a_limit_and_baseline_flags() {
        let Command::Diff(diff) =
            parse(&["diff", "try", "--max-lines", "30", "--no-baseline"]).command
        else {
            panic!("not a diff")
        };
        assert_eq!(diff.computer, "try");
        assert_eq!(diff.max_lines, Some(30));
        assert!(diff.no_baseline && !diff.baseline);
        assert!(Cli::try_parse_from(["silo", "diff", "x", "--baseline", "--no-baseline"]).is_err());
    }

    #[test]
    fn drop_force_is_a_flag() {
        let Command::Drop(drop) = parse(&["drop", "main", "--force"]).command else {
            panic!("not a drop")
        };
        assert!(drop.force);
    }
}
