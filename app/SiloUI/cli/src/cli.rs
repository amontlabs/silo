//! Argument parsing and command dispatch.

use std::{
    io::Write,
    path::{Path, PathBuf},
};

use clap::{Args, Parser, Subcommand};
use serde_json::Value;

use crate::{
    config::{Project, Recipe, Settings},
    engine::{default_socket, Engine},
    error::CliError,
    ops::{self, CreateArgs, DiffArgs, ExecArgs, ForkArgs, Platform},
    render,
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
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// List computers and their state.
    List,
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
    /// Copy a computer for a task.
    Fork(ForkCommand),
    /// Keep a fork's changes into the computer it came from.
    Fold(FoldCommand),
    /// Put a computer back to before its latest keep.
    Undo { computer: String },
    /// Show what changed in a computer since it was forked.
    Diff(DiffCommand),
    /// Throw a computer away.
    Drop(DropCommand),
    /// Run a command in a computer.
    Exec(ExecCommand),
    /// Save a checkpoint of a computer.
    Checkpoint(CheckpointCommand),
    /// List a computer's checkpoints.
    Checkpoints { computer: String },
    /// Manage the Silo secrets and which computers have them.
    #[command(subcommand)]
    Secrets(SecretsCommand),
    /// Wait for a job that is already running, by id.
    Job { id: String },
    /// Serve the MCP tools on stdio.
    Mcp,
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
    #[arg(long, conflicts_with = "macos")]
    pub linux: bool,
    /// A macOS computer.
    #[arg(long)]
    pub macos: bool,
    #[arg(long)]
    pub cpus: Option<u64>,
    /// Memory in GiB.
    #[arg(long)]
    pub memory: Option<u64>,
    /// Disk in GiB.
    #[arg(long)]
    pub disk: Option<u64>,
}

#[derive(Args, Debug)]
pub struct ForkCommand {
    pub source: String,
    #[arg(long)]
    pub name: Option<String>,
    /// Fork from this checkpoint instead of the current state.
    #[arg(long, value_name = "ID")]
    pub checkpoint: Option<String>,
    /// A Silo-managed secret the fork may use (repeatable).
    #[arg(long = "secret", value_name = "NAME")]
    pub secrets: Vec<String>,
    /// A recipe whose quiet: and scrub: lists apply to the fork.
    #[arg(long, value_name = "PATH")]
    pub recipe_file: Option<PathBuf>,
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
    let socket = default_socket(cli.socket.clone(), cli.allow_other_channel)?;
    if matches!(cli.command, Command::Mcp) {
        crate::mcp::serve(socket, working_directory.to_owned()).await?;
        return Ok(0);
    }
    let project = Project::discover(working_directory);
    // Read before connecting so a bad file is reported without the engine.
    let settings = project.load_settings()?;
    let recipe = match &cli.command {
        Command::Fork(fork) => ops::resolve_recipe(fork.recipe_file.as_deref(), &project)?,
        Command::Fold(fold) => ops::resolve_recipe(fold.recipe_file.as_deref(), &project)?,
        _ => None,
    };
    let engine = Engine::connect(&socket).await?;
    let json = cli.json;
    tokio::select! {
        result = dispatch(&engine, cli.command, json, settings, recipe) => result,
        _ = tokio::signal::ctrl_c() => {
            match engine.last_job() {
                Some(job) => eprintln!("Interrupted. {}", Engine::reattach_hint(&job)),
                None => eprintln!("Interrupted."),
            }
            Ok(130)
        }
    }
}

async fn dispatch(
    engine: &Engine,
    command: Command,
    json: bool,
    settings: Option<Settings>,
    recipe: Option<Recipe>,
) -> Result<i32, CliError> {
    let progress = &stderr_progress;
    match command {
        Command::List => emit(json, &ops::list(engine).await?, render::list),
        Command::Status { computer } => {
            emit(json, &ops::status(engine, &computer).await?, render::status)
        }
        Command::Create(create) => {
            let args = CreateArgs {
                name: create.name,
                platform: if create.macos {
                    Platform::Macos
                } else {
                    Platform::Linux
                },
                cpus: create.cpus,
                memory_gib: create.memory,
                disk_gib: create.disk,
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
        Command::RetrySetup { computer } => {
            let value = ops::retry_setup(engine, &computer, progress).await?;
            emit(json, &value, |v| render::computer_line("Set up", v));
        }
        Command::Fork(fork) => {
            let args = ForkArgs {
                source: fork.source,
                name: fork.name,
                checkpoint: fork.checkpoint,
                secrets: fork.secrets,
                recipe,
            };
            let value = ops::fork(engine, &args, settings.as_ref(), progress).await?;
            emit(json, &value, |v| render::computer_line("Forked", v));
        }
        Command::Fold(fold) if fold.promote => {
            let value = ops::promote(
                engine,
                &fold.fork,
                fold.into.as_deref(),
                fold.dry_run,
                progress,
            )
            .await?;
            emit(json, &value, render::promote);
            return Ok(if value["status"] == "refused" { 2 } else { 0 });
        }
        Command::Fold(fold) => {
            let keep = ops::keep_hooks(recipe.as_ref());
            let value = ops::fold(
                engine,
                &fold.fork,
                fold.into.as_deref(),
                keep,
                ops::FoldOptions {
                    dry_run: fold.dry_run,
                    data_ok: fold.data_ok,
                    reviewed: fold.reviewed,
                },
                progress,
            )
            .await?;
            emit(json, &value, render::fold);
            return Ok(if value["status"] == "refused" { 2 } else { 0 });
        }
        Command::Undo { computer } => {
            let value = ops::undo(engine, &computer, progress).await?;
            emit(json, &value, render::undo);
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
        Command::Exec(exec) => {
            let args = ExecArgs {
                computer: exec.computer,
                argv: exec.command,
                cwd: exec.cwd,
                timeout_secs: exec.timeout,
            };
            let output = ops::exec_raw(engine, &args).await?;
            return Ok(print_exec(json, &output));
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
        Command::Checkpoints { computer } => emit(
            json,
            &ops::checkpoints(engine, &computer).await?,
            render::checkpoints,
        ),
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
        Command::Mcp => unreachable!("handled above"),
    }
    Ok(0)
}

/// Prints an exec result and returns the process exit code. The text mode writes the
/// output bytes as they are; `--json` carries them as text with invalid UTF-8 replaced.
fn print_exec(json: bool, output: &ops::ExecOutput) -> i32 {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&output.to_json()).unwrap_or_default()
        );
    } else {
        let _ = std::io::stdout().write_all(&output.stdout);
        let _ = std::io::stderr().write_all(&output.stderr);
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
    fn json_and_socket_are_global() {
        let cli = parse(&["list", "--json", "--socket", "/x.sock"]);
        assert!(cli.json);
        assert_eq!(cli.socket, Some(PathBuf::from("/x.sock")));
    }

    #[test]
    fn create_flags_conflict() {
        assert!(Cli::try_parse_from(["silo", "create", "a", "--linux", "--macos"]).is_err());
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
