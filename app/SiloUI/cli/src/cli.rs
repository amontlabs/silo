//! Argument parsing and command dispatch.

use std::{
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

use clap::{Args, Parser, Subcommand};
use serde_json::Value;

use crate::{
    access, approvals,
    engine::{build_channel, default_socket, Engine},
    error::{exit, CliError},
    fanout::{self, ManyArgs},
    flows, folder, hosted,
    ops::{self, CreateArgs, DiffArgs, ExecArgs, ForkArgs, MergeOptions, Platform},
    pr, recipe_file, render, review,
    steps::{self, StepSettings},
};

#[derive(Parser, Debug)]
#[command(
    name = "silo",
    version,
    about = "Fork a computer, let an agent try, merge what worked."
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
    /// Run the command on a connected device (its name or id) instead of this one, over
    /// Connections. Folders, ports, SSH names and secrets stay on this device.
    #[arg(long, global = true, value_name = "DEVICE")]
    pub device: Option<String>,
    /// Inside a computer, a command that widens what the computer has waits this long for the
    /// owner's answer in Silo (default 2m; `90s`, `10m`, `1h`).
    #[arg(
        long,
        global = true,
        value_name = "DURATION",
        conflicts_with = "no_wait"
    )]
    pub wait: Option<String>,
    /// Inside a computer, do not wait for the owner's answer: print the request and exit with
    /// code 5. Check it later with `silo approvals status <id>`.
    #[arg(long, global = true)]
    pub no_wait: bool,
    /// Inside a computer, your reason for a command that needs the owner's answer. The owner
    /// reads it beside the request, marked as yours.
    #[arg(long, global = true, value_name = "TEXT")]
    pub reason: Option<String>,
    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    /// How the commands behave when the engine says the owner has to approve a call.
    pub fn wait_options(&self) -> Result<approvals::WaitOptions, CliError> {
        let wait = match (&self.wait, self.no_wait) {
            (_, true) => Duration::ZERO,
            (Some(text), false) => approvals::parse_duration(text)?,
            (None, false) => approvals::DEFAULT_WAIT,
        };
        Ok(approvals::WaitOptions {
            wait,
            reason: self.reason.clone(),
            ..approvals::WaitOptions::default()
        })
    }
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
    /// Show what this device has left (memory, disk, running computers) and whether it is
    /// running short.
    Resources,
    /// Show what a computer holds: its secrets' names and hosts, GitHub access, published
    /// ports, SSH and network policy. Without a name, every computer you can see.
    Access { computer: Option<String> },
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
    /// Make `main` from a recipe the first time, start it, and set up its ports and SSH name.
    Up(UpCommand),
    /// Add, list, show, build or remove recipes.
    #[command(subcommand)]
    Recipe(RecipeCommand),
    /// Copy a computer for a task, or make a fresh one from a recipe.
    Fork(ForkCommand),
    /// Merge a fork's changes into the computer it came from (or another).
    Merge(MergeCommand),
    /// Replace main with a fork: it takes main's place and the old main is kept aside.
    ReplaceMain(ReplaceMainCommand),
    /// Rebuild a Linux computer on the newest guest image, keeping its workspace and home.
    Rebuild(RebuildCommand),
    /// Now `silo merge`.
    #[command(hide = true)]
    Fold(MergeCommand),
    /// List review requests, or show, merge, decline or open one.
    Requests(RequestsCommand),
    /// List what computers' agents asked you to allow, show, allow or deny one, or follow a
    /// request this computer made.
    Approvals(ApprovalsCommand),
    /// Put a computer back to before its latest merge or replace, or a folder back to before
    /// its latest merge.
    Undo {
        /// A computer, a fork made from a folder, or the folder's path.
        computer: String,
    },
    /// Show what changed in a computer since it was forked.
    Diff(DiffCommand),
    /// Ask the hosted reviewer to read a fork's diff (needs SILO_REVIEWER_URL).
    #[command(hide = true)]
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
    /// Put a computer back to one of its checkpoints.
    Restore {
        computer: String,
        checkpoint: String,
    },
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
    /// List the coding agents installed in a computer, or install one.
    #[command(subcommand)]
    Agents(AgentsCommand),
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
pub enum AgentsCommand {
    /// The agents a computer has and whether each is installed.
    List { computer: String },
    /// Install an agent in a computer (claude-code or codex).
    Install {
        computer: String,
        #[arg(value_enum)]
        agent: AgentChoice,
    },
}

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentChoice {
    ClaudeCode,
    Codex,
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
    /// The stored recipe `main` is made from when it does not exist (default linux-dev).
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
    /// Store a recipe file (YAML) under a name, replacing the one of that name.
    Add {
        file: PathBuf,
        /// The recipe's name (default: the file's name without its extension).
        #[arg(long, value_name = "NAME")]
        name: Option<String>,
    },
    /// List the stored recipes and which are built.
    #[command(alias = "ls")]
    List,
    /// Print a stored recipe, with its hash and any warnings.
    Show { name: String },
    /// Forget a stored recipe and drop its build.
    #[command(alias = "rm")]
    Remove { name: String },
    /// Build a recipe now (or again, with --force) so forks from it start at once.
    Build {
        name: String,
        #[arg(long)]
        force: bool,
    },
}

#[derive(Args, Debug)]
pub struct ForkCommand {
    /// The computer to copy, or a folder (./repo) to copy into a new Linux fork; with
    /// --recipe, the new computer's name.
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
    /// Make a fresh computer from this stored recipe's build (building it first when needed),
    /// not from a computer.
    #[arg(long, value_name = "NAME", conflicts_with_all = ["checkpoint", "name"])]
    pub recipe: Option<String>,
    /// Leave the new computer stopped. A fork you make to work in starts by default.
    #[arg(long, conflicts_with_all = ["gui", "count", "command"])]
    pub no_start: bool,
    /// Start the fork and wait until its desktop and computer-use driver are ready, then
    /// print its name and computer-use status.
    #[arg(long, conflicts_with_all = ["count", "command", "recipe"])]
    pub gui: bool,
    /// The fork's computer-use approval: ask (the default) or auto, which lets an agent in
    /// the fork drive its desktop without asking. Only a person at the device may choose auto.
    #[arg(
        long,
        value_name = "ask|auto",
        conflicts_with_all = ["count", "command", "recipe"]
    )]
    pub computer_use: Option<String>,
    /// How long --gui waits for the desktop and the driver.
    #[arg(long, value_name = "SECONDS", default_value_t = crate::gui::DEFAULT_TIMEOUT_SECS, requires = "gui")]
    pub gui_timeout: u64,
    #[command(flatten)]
    pub steps: StepOptions,
}

/// Whether commands run through Silo are followed by a checkpoint of their step.
#[derive(Args, Debug, Clone, Copy, Default)]
pub struct StepOptions {
    /// Save a checkpoint of the computer after the command (each command run through Silo is a
    /// step; `silo fork --from-step N` starts from one).
    #[arg(long)]
    pub step_checkpoints: bool,
    /// How many step checkpoints a computer keeps, the newest ones (default 20; 0 keeps
    /// every one).
    #[arg(long, value_name = "N", requires = "step_checkpoints")]
    pub keep_steps: Option<u32>,
}

impl StepOptions {
    pub fn settings(self) -> StepSettings {
        StepSettings::new(self.step_checkpoints, self.keep_steps)
    }
}

#[derive(Args, Debug)]
pub struct MergeCommand {
    /// The fork whose changes are merged.
    pub fork: String,
    /// The computer to merge into (default: the one the fork was made from).
    #[arg(long, value_name = "COMPUTER")]
    pub into: Option<String>,
    /// Report what would be merged and what would stop it, changing nothing.
    #[arg(long)]
    pub dry_run: bool,
    /// How to settle an item both sides changed: <item>=fork, <item>=main, or for a repository
    /// <item>=branch (the fork's commits arrive as a branch and nothing is merged). The item is
    /// the id the merge lists, such as file:/etc/app.conf or repo:/work/app (repeatable).
    #[arg(long = "choose", value_name = "ITEM=ANSWER")]
    pub choose: Vec<String>,
    /// An item not to bring over even though it does not conflict (repeatable).
    #[arg(long = "skip", value_name = "ITEM")]
    pub skip: Vec<String>,
    /// Carry database changes even though no migrate step covers them.
    #[arg(long)]
    pub data_ok: bool,
    /// The review digest from `silo diff --json`; the merge refuses if the fork has changed since.
    #[arg(long, value_name = "DIGEST")]
    pub reviewed: Option<String>,
    /// Ask for a review instead of merging: records a request for the fork and prints its id.
    #[arg(long, conflicts_with_all = ["dry_run", "pr", "choose", "skip"])]
    pub request: bool,
    /// Push the fork's code to a new branch `silo/<fork>-<id>` and open a pull request; merges
    /// nothing. Needs GITHUB_TOKEN or `gh auth login` on this device.
    #[arg(long, conflicts_with_all = ["dry_run", "into", "choose", "skip"])]
    pub pr: bool,
    /// With --pr: the GitHub repository (OWNER/NAME; default GITHUB_REPOSITORY, else origin).
    #[arg(long, value_name = "OWNER/NAME", requires = "pr")]
    pub repo: Option<String>,
    /// With --pr: the branch to merge into (default: the branch the folder is on, or the
    /// repository's default branch).
    #[arg(long, value_name = "BRANCH", requires = "pr")]
    pub base: Option<String>,
    /// Kept for `silo fold --promote`; use `silo replace-main`.
    #[arg(long, hide = true, conflicts_with_all = ["request", "pr"])]
    pub promote: bool,
}

#[derive(Args, Debug)]
pub struct ReplaceMainCommand {
    /// The fork that takes main's place: it gets main's name, settings and published ports,
    /// and the old main is archived (`silo undo main` swaps back). This restarts main.
    pub fork: String,
    /// The computer to replace (default: the one the fork was made from).
    #[arg(long, value_name = "COMPUTER")]
    pub into: Option<String>,
    /// Report what would happen and what would stop it, changing nothing.
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Args, Debug)]
pub struct RebuildCommand {
    /// The computer to rebuild on the newest image.
    pub computer: String,
    /// Rebuild it even when it is already on the newest image.
    #[arg(long)]
    pub force: bool,
    /// Instead of rebuilding, throw away an interrupted rebuild Silo could not settle and put
    /// the old computer back.
    #[arg(long, conflicts_with = "force")]
    pub abandon: bool,
}

#[derive(Args, Debug)]
#[command(args_conflicts_with_subcommands = true)]
pub struct RequestsCommand {
    #[command(subcommand)]
    pub action: Option<RequestsAction>,
    /// Which requests to list: open (default), kept, thrown-away or all.
    #[arg(long, value_name = "STATUS")]
    pub status: Option<String>,
    /// List this device's requests and every connected device's. `show`, `merge`,
    /// `decline` and `open` find the device a request is on by themselves.
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
    /// Where a request this computer made stands. Exits 0 if allowed, 4 if denied, 5 if still
    /// waiting, 6 if it ended without an answer.
    Status { id: String },
    /// Wait for the owner to answer a request this computer made (default 2m; see --wait).
    /// The exit code says how it ended, as for `status`.
    Wait { id: String },
    /// Add your reason to a request this computer made. The owner reads it beside the request,
    /// marked as yours.
    Note { id: String, text: String },
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
    /// Merge the request's fork into its target, pinned to the digest the request was made from.
    #[command(alias = "keep")]
    Merge {
        id: String,
        /// Carry database changes even though no migrate step covers them.
        #[arg(long)]
        data_ok: bool,
        /// How to settle an item both sides changed (see `silo merge --choose`).
        #[arg(long = "choose", value_name = "ITEM=ANSWER")]
        choose: Vec<String>,
        /// An item not to bring over (repeatable).
        #[arg(long = "skip", value_name = "ITEM")]
        skip: Vec<String>,
    },
    /// Drop the request's fork without merging it. main and the target are never dropped.
    #[command(alias = "throw-away")]
    Decline { id: String },
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
    #[command(flatten)]
    pub steps: StepOptions,
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
    let wait = cli.wait_options()?;
    let transport = crate::endpoint::for_this_process(cli.socket.clone(), cli.allow_other_channel)?;
    if matches!(cli.command, Command::Mcp) {
        crate::mcp::serve(transport, working_directory.to_owned()).await?;
        return Ok(0);
    }
    let from_computer = matches!(transport, crate::endpoint::Transport::Channel(_));
    refuse_in_computer(&cli.command, from_computer)?;
    if let Command::Fork(fork) = &cli.command {
        if let Some(mode) = fork
            .computer_use
            .as_deref()
            .map(crate::gui::parse_approval)
            .transpose()?
        {
            crate::gui::check_auto(mode, from_computer)?;
        }
    }
    let engine = Engine::connect_to(&transport).await?.with_approvals(wait);
    let engine = match cli.device.as_deref() {
        None | Some("local") => engine,
        Some(name) => {
            let id = engine.resolve_device(name).await?;
            engine.on_device(Some(id))
        }
    };
    refuse_on_device(&cli.command, &engine)?;
    let json = cli.json;
    let wait_for = cli.wait.clone();
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
        result = dispatch(&engine, cli.command, json, working_directory, wait_for) => result,
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
        Command::Merge(merge) | Command::Fold(merge) if merge.pr => "merge --pr",
        Command::Undo { computer } if folder::is_folder(computer) => "undo of a folder",
        _ => return Ok(()),
    };
    Err(CliError::new(format!(
        "{what} works on this device, so --device cannot be used with it."
    )))
}

/// The commands that reach this device's folders, SSH names and pull requests: not offered to
/// the command line inside a computer, which has none of them.
fn refuse_in_computer(command: &Command, from_computer: bool) -> Result<(), CliError> {
    if !from_computer {
        return Ok(());
    }
    let what = match command {
        Command::SshSetup { .. } => "ssh-setup",
        Command::Fork(fork) if fork.recipe.is_none() && folder::is_folder(&fork.source) => {
            "A fork of a folder"
        }
        Command::Merge(merge) | Command::Fold(merge) if merge.pr => "merge --pr",
        Command::Undo { computer } if folder::is_folder(computer) => "undo of a folder",
        Command::Export(_) | Command::Import(_) => "export and import",
        Command::Review { .. } => "review",
        _ => return Ok(()),
    };
    Err(CliError::new(format!(
        "{what} works on the device, not inside a computer."
    )))
}

fn parse_choices(
    choose: &[String],
) -> Result<std::collections::BTreeMap<String, silo_api::methods::keep::KeepAnswer>, CliError> {
    choose.iter().map(|text| ops::parse_choice(text)).collect()
}

async fn dispatch(
    engine: &Engine,
    command: Command,
    json: bool,
    working_directory: &Path,
    wait_for: Option<String>,
) -> Result<i32, CliError> {
    let progress = &stderr_progress;
    match command {
        Command::Up(up) => {
            let name = up
                .recipe
                .unwrap_or_else(|| recipe_file::DEFAULT_RECIPE.to_owned());
            recipe_file::check_name(&name)?;
            let home = access::home_dir();
            let ssh = home
                .as_ref()
                .ok()
                .map(|home| (home.as_path(), build_channel()));
            let mut value = ops::up(engine, &name, ssh, progress).await?;
            if let Err(error) = home {
                value["ssh"] = serde_json::json!({ "error": error.message });
            }
            emit(json, &value, render::up);
        }
        Command::Recipe(RecipeCommand::Add { file, name }) => {
            let recipe = recipe_file::load(&file)?;
            let name = match name {
                Some(name) => {
                    recipe_file::check_name(&name)?;
                    name
                }
                None => recipe_file::name_from_path(&file)?,
            };
            let value = ops::recipe_add(engine, &name, recipe.definition()).await?;
            emit(json, &value, render::recipe_added);
        }
        Command::Recipe(RecipeCommand::List) => {
            emit(json, &ops::recipe_list(engine).await?, render::recipes)
        }
        Command::Recipe(RecipeCommand::Show { name }) => emit(
            json,
            &ops::recipe_show(engine, &name).await?,
            render::recipe_shown,
        ),
        Command::Recipe(RecipeCommand::Build { name, force }) => {
            recipe_file::check_name(&name)?;
            let value = ops::build_recipe(engine, &name, force, progress).await?;
            emit(json, &value, render::recipe_built);
        }
        Command::Recipe(RecipeCommand::Remove { name }) => {
            let value = ops::recipe_remove(engine, &name).await?;
            emit(json, &value, |v| {
                format!(
                    "Removed the recipe {}.",
                    v["removed"].as_str().unwrap_or_default()
                )
            });
        }
        Command::Resources => emit(
            json,
            &ops::device_resources(engine).await?,
            render::resources,
        ),
        Command::Access { computer } => emit(
            json,
            &access::held(engine, computer.as_deref()).await?,
            render::held,
        ),
        Command::Agents(AgentsCommand::List { computer }) => emit(
            json,
            &ops::agents_list(engine, &computer).await?,
            render::agents,
        ),
        Command::Agents(AgentsCommand::Install { computer, agent }) => {
            let agent = match agent {
                AgentChoice::ClaudeCode => silo_api::methods::agents::AgentId::ClaudeCode,
                AgentChoice::Codex => silo_api::methods::agents::AgentId::Codex,
            };
            let value = ops::agent_install(engine, &computer, agent, progress).await?;
            emit(json, &value, render::agents);
        }
        Command::Restore {
            computer,
            checkpoint,
        } => {
            let value = ops::restore(engine, &computer, &checkpoint, progress).await?;
            emit(json, &value, |v| render::computer_line("Restored", v));
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
                return fork_folder(engine, fork, json, working_directory).await;
            }
            if fork.count.is_some() || !fork.command.is_empty() {
                let args = ManyArgs {
                    fork: ForkArgs {
                        source: fork.source,
                        name: fork.name,
                        checkpoint: fork.checkpoint,
                        secrets: fork.secrets,
                        recipe_name: None,
                        no_start: false,
                    },
                    count: fork.count.unwrap_or(1),
                    command: fork.command,
                    cwd: None,
                    steps: fork.steps.settings(),
                };
                return fork_many(engine, &args, json).await;
            }
            let recipe_name = fork.recipe;
            if let Some(name) = &recipe_name {
                recipe_file::check_name(name)?;
            }
            let (source, name) = match &recipe_name {
                Some(_) => (String::new(), Some(fork.source)),
                None => (fork.source, fork.name),
            };
            let args = ForkArgs {
                source,
                name,
                checkpoint: fork.checkpoint,
                secrets: fork.secrets,
                recipe_name,
                no_start: fork.no_start,
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
                let value = crate::gui::fork_gui(engine, &gui, progress).await?;
                emit(json, &value, render::forked_gui);
                return Ok(0);
            }
            let mut value = ops::fork(engine, &args, progress).await?;
            if let Some(mode) = approval {
                crate::gui::apply_to_fork(engine, &mut value, mode).await?;
            }
            emit(json, &value, render::forked);
        }
        Command::ReplaceMain(replace) => {
            return replace_main(
                engine,
                &replace.fork,
                replace.into.as_deref(),
                replace.dry_run,
                json,
            )
            .await;
        }
        Command::Rebuild(rebuild) => {
            return rebuild_command(
                engine,
                &rebuild.computer,
                rebuild.force,
                rebuild.abandon,
                json,
            )
            .await;
        }
        Command::Fold(merge) => {
            eprintln!("`silo fold` is now `silo merge`; `silo replace-main` replaces main.");
            return merge_command(engine, merge, json, working_directory).await;
        }
        Command::Merge(merge) => {
            return merge_command(engine, merge, json, working_directory).await
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
                    let reviewed = hosted::review_target(engine, &id, &lenses, progress).await?;
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
            Some(RequestsAction::Merge {
                id,
                data_ok,
                choose,
                skip,
            }) => {
                let engine = &review::locate(engine, &id).await?;
                let choices = parse_choices(&choose)?;
                let value = review::merge(engine, &id, data_ok, choices, skip, progress).await?;
                emit(json, &value, render::merge);
                return Ok(merge_exit(&value));
            }
            Some(RequestsAction::Decline { id }) => {
                let engine = &review::locate(engine, &id).await?;
                let value = review::decline(engine, &id, progress).await?;
                emit(json, &value, render::request_declined);
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
                Some(ApprovalsAction::Status { id }) => {
                    let value = approvals::show(engine, &id).await?;
                    emit(json, &value, approvals::render_show);
                    return Ok(approvals::exit_for(
                        value["state"].as_str().unwrap_or_default(),
                    ));
                }
                Some(ApprovalsAction::Wait { id }) => {
                    let limit = match &wait_for {
                        Some(text) => approvals::parse_duration(text)?,
                        None => approvals::DEFAULT_WAIT,
                    };
                    let mut value =
                        approvals::wait(engine, &id, limit, std::time::Duration::from_millis(1500))
                            .await?;
                    // An allowed request that started a job: follow it as any command would.
                    if let Some(job) = value["outcome"]["jobId"].as_str().map(str::to_owned) {
                        let finished = engine
                            .wait_job(&job, &format!("Following {job}"), progress)
                            .await;
                        match finished {
                            Ok(result) => value["jobResult"] = result,
                            Err(error) => value["jobError"] = serde_json::json!(error.message),
                        }
                    }
                    emit(json, &value, approvals::render_show);
                    return Ok(approvals::exit_for(
                        value["state"].as_str().unwrap_or_default(),
                    ));
                }
                Some(ApprovalsAction::Note { id, text }) => {
                    let value = approvals::note(engine, &id, &text).await?;
                    emit(json, &value, approvals::render_show);
                }
            }
        }
        Command::Undo { computer } => {
            let mut value = flows::undo(engine, &computer, working_directory, progress).await?;
            if let Ok(home) = access::home_dir() {
                access::ssh_after_change(engine, &mut value, &home, build_channel()).await;
            }
            emit(json, &value, render::undo);
            return Ok(if value["status"] == "refused" {
                exit::REFUSED
            } else {
                0
            });
        }
        Command::Review { target, lens } => {
            let lenses = hosted::parse_lenses(lens.as_deref())?;
            let value = hosted::review_target(engine, &target, &lenses, progress).await?;
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
            let (output, step) =
                steps::run_exec(engine, &args, exec.steps.settings(), progress).await?;
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

async fn fork_many(engine: &Engine, args: &ManyArgs, json: bool) -> Result<i32, CliError> {
    let several = args.count > 1;
    let (sink, prefixer) = output_sink(json, several);
    let value = fanout::fork_many(engine, args, &stderr_progress, &sink).await?;
    flush_partial(json, &prefixer);
    if json {
        emit(true, &value, |_| String::new());
    } else if args.command.is_empty() || several {
        println!("{}", render::forks(&value));
    } else {
        // A single command's output is on stdout, so the summary goes to stderr.
        eprintln!("{}", render::forks(&value));
    }
    Ok(fanout::exit_code(&value))
}

async fn fork_folder(
    engine: &Engine,
    fork: ForkCommand,
    json: bool,
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
    let directory = working_directory.join(&fork.source);
    let value = folder::fork_folder(
        engine,
        &directory,
        fork.name.as_deref(),
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
    if json {
        let mut value = value;
        value["exitCode"] = serde_json::json!(ran.exit_code);
        value["signal"] = serde_json::json!(ran.signal);
        value["stdout"] = serde_json::json!(String::from_utf8_lossy(&out.bytes));
        value["stderr"] = serde_json::json!(String::from_utf8_lossy(&err.bytes));
        value["outputTruncated"] = serde_json::json!(out.truncated || err.truncated);
        emit(true, &value, |_| String::new());
    } else {
        eprintln!("{}", render::folder_fork(&value));
    }
    Ok(ran.exit_code)
}

/// The exit code of a merge result: done, refused, or refused until the caller answers the
/// items both sides changed.
pub fn merge_exit(value: &Value) -> i32 {
    if value["status"] != "refused" {
        return 0;
    }
    if value["refusal"]["reason"] == "choices-needed" {
        exit::CHOICES_NEEDED
    } else {
        exit::REFUSED
    }
}

/// `silo merge`: merges a fork, asks for a review, or opens a pull request.
async fn merge_command(
    engine: &Engine,
    merge: MergeCommand,
    json: bool,
    working_directory: &Path,
) -> Result<i32, CliError> {
    if merge.promote {
        return replace_main(
            engine,
            &merge.fork,
            merge.into.as_deref(),
            merge.dry_run,
            json,
        )
        .await;
    }
    let mode = if merge.pr {
        flows::Mode::PullRequest(
            pr::PrArgs {
                repo: merge.repo,
                base: merge.base,
                host_root: Some(working_directory.to_path_buf()),
            },
            pr::PrEnv::from_process(),
        )
    } else if merge.request {
        flows::Mode::Request
    } else {
        flows::Mode::Merge
    };
    let options = MergeOptions {
        dry_run: merge.dry_run,
        data_ok: merge.data_ok,
        reviewed: merge.reviewed,
        choices: parse_choices(&merge.choose)?,
        skip: merge.skip,
    };
    let value = flows::run(
        engine,
        &merge.fork,
        merge.into.as_deref(),
        options,
        mode,
        &stderr_progress,
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
    emit(json, &value, render::merge);
    Ok(merge_exit(&value))
}

/// `silo replace-main`.
async fn replace_main(
    engine: &Engine,
    fork: &str,
    into: Option<&str>,
    dry_run: bool,
    json: bool,
) -> Result<i32, CliError> {
    let mut value = ops::replace_main(engine, fork, into, dry_run, &stderr_progress).await?;
    if let Ok(home) = access::home_dir() {
        access::ssh_after_change(engine, &mut value, &home, build_channel()).await;
    }
    emit(json, &value, render::replace_main);
    Ok(if value["status"] == "refused" {
        exit::REFUSED
    } else {
        0
    })
}

/// `silo rebuild`.
async fn rebuild_command(
    engine: &Engine,
    computer: &str,
    force: bool,
    abandon: bool,
    json: bool,
) -> Result<i32, CliError> {
    let value = ops::rebuild(engine, computer, force, abandon, &stderr_progress).await?;
    emit(json, &value, render::rebuild);
    Ok(0)
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
            "--no-start",
        ]);
        let Command::Fork(fork) = cli.command else {
            panic!("not a fork")
        };
        assert_eq!(fork.source, "main");
        assert_eq!(fork.name.as_deref(), Some("try"));
        assert_eq!(fork.checkpoint.as_deref(), Some("c1"));
        assert_eq!(fork.secrets, ["A", "B"]);
        assert!(fork.no_start);
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
            assert!(Cli::try_parse_from(["silo", "merge", "try", "--pr", extra[0]]).is_err());
        }
        assert!(Cli::try_parse_from(["silo", "merge", "try", "--pr", "--into", "main"]).is_err());
        assert!(Cli::try_parse_from(["silo", "merge", "try", "--repo", "a/b"]).is_err());
        assert!(Cli::try_parse_from(["silo", "merge", "try", "--pr", "--repo", "a/b"]).is_ok());
    }

    #[test]
    fn merge_takes_a_fork_a_target_answers_and_dry_run() {
        let Command::Merge(merge) = parse(&[
            "merge",
            "try",
            "--into",
            "main",
            "--dry-run",
            "--choose",
            "file:/etc/a.conf=fork",
            "--choose",
            "repo:/work/app=branch",
            "--skip",
            "package:dpkg:jq",
        ])
        .command
        else {
            panic!("not a merge")
        };
        assert_eq!(
            (merge.fork.as_str(), merge.into.as_deref(), merge.dry_run),
            ("try", Some("main"), true)
        );
        assert_eq!(merge.choose.len(), 2);
        assert_eq!(merge.skip, ["package:dpkg:jq"]);
        assert!(matches!(
            parse(&["replace-main", "try"]).command,
            Command::ReplaceMain(_)
        ));
        assert!(matches!(
            parse(&["rebuild", "dev"]).command,
            Command::Rebuild(_)
        ));
        assert!(matches!(
            parse(&["undo", "main"]).command,
            Command::Undo { .. }
        ));
    }

    #[test]
    fn fold_still_parses_and_is_hidden() {
        assert!(matches!(
            parse(&["fold", "try", "--promote"]).command,
            Command::Fold(_)
        ));
        use clap::CommandFactory;
        let command = Cli::command();
        let fold = command.find_subcommand("fold").unwrap();
        assert!(fold.is_hide_set());
        assert!(command.find_subcommand("review").unwrap().is_hide_set());
    }

    #[test]
    fn waiting_for_the_owner_is_chosen_with_flags() {
        let waits = |args: &[&str]| parse(args).wait_options().unwrap().wait;
        assert_eq!(waits(&["list"]), approvals::DEFAULT_WAIT);
        assert_eq!(waits(&["--wait", "90s", "list"]), Duration::from_secs(90));
        assert_eq!(waits(&["--wait", "2m", "list"]), Duration::from_secs(120));
        assert_eq!(waits(&["--no-wait", "list"]), Duration::ZERO);
        assert!(Cli::try_parse_from(["silo", "--wait", "1m", "--no-wait", "list"]).is_err());
        assert!(parse(&["--wait", "soon", "list"]).wait_options().is_err());
        let cli = parse(&["--reason", "to test the site", "publish", "main", "8080"]);
        assert_eq!(
            cli.wait_options().unwrap().reason.as_deref(),
            Some("to test the site")
        );
    }

    #[test]
    fn approvals_can_be_followed_later() {
        assert!(matches!(
            parse(&["approvals", "status", "ap-1"]).command,
            Command::Approvals(ApprovalsCommand {
                action: Some(ApprovalsAction::Status { .. })
            })
        ));
        assert!(matches!(
            parse(&["approvals", "wait", "ap-1"]).command,
            Command::Approvals(ApprovalsCommand {
                action: Some(ApprovalsAction::Wait { .. })
            })
        ));
        assert!(matches!(
            parse(&["approvals", "note", "ap-1", "because"]).command,
            Command::Approvals(ApprovalsCommand {
                action: Some(ApprovalsAction::Note { .. })
            })
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
