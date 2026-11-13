//! The runtime commands the engine issues in MicroSandbox's argument shape, read for a
//! container computer. Parsing is separate from running so the shapes the rest of the engine
//! produces are pinned by tests.

use std::time::Duration;

/// The label that marks a `create` as a container computer.
pub const RUNTIME_LABEL: &str = "silo.runtime=container";

/// What a container computer is asked to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    List {
        label: Option<String>,
    },
    Inspect {
        name: String,
    },
    Create(Box<Create>),
    Start {
        name: String,
    },
    Stop {
        name: String,
    },
    Remove {
        name: String,
    },
    Exec(Box<Exec>),
    /// `ssh serve`: the SSH server of the computer (a long-running command, not a waited one).
    SshServe(Box<SshServe>),
    /// Secrets and GitHub access: the only `modify` a container computer answers.
    Modify(Box<Modify>),
    /// `snapshot create NAME --from-sandbox COMPUTER --group GROUP`.
    SnapshotCreate(Box<SnapshotCreate>),
    /// `snapshot list`, for one group or all.
    SnapshotList {
        group: Option<String>,
    },
    /// `snapshot verify GROUP:NAME`.
    SnapshotVerify {
        group: String,
        name: String,
    },
    /// `snapshot remove GROUP:NAME`.
    SnapshotRemove {
        group: String,
        name: String,
    },
    /// `snapshot head GROUP[:NAME]`: the group's newest checkpoint.
    SnapshotHead {
        group: String,
    },
    /// `snapshot save GROUP:NAME FILE`: the checkpoint's files as one export payload.
    SnapshotSave(Box<SnapshotSave>),
    /// `snapshot load FILE --group GROUP --runtime container`: an export payload as a checkpoint.
    SnapshotLoad(Box<SnapshotLoad>),
    /// `restore GROUP:NAME --name NEW`: a new computer from a checkpoint, started.
    Restore(Box<Restore>),
    /// Freeze or thaw the container's processes.
    Pause {
        name: String,
    },
    Resume {
        name: String,
    },
    /// A command only MicroSandbox computers answer yet.
    Unsupported {
        verb: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotCreate {
    pub name: String,
    pub computer: String,
    pub group: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotSave {
    pub group: String,
    pub name: String,
    pub file: std::path::PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotLoad {
    pub file: std::path::PathBuf,
    pub group: String,
}

/// The flag pair that sends a `snapshot load` to the container runtime: MicroSandbox's own
/// load takes no such flag, and a file alone does not say which runtime wrote it.
pub const LOAD_RUNTIME: (&str, &str) = ("--runtime", "container");

/// Whether a `snapshot load` names the container runtime.
pub fn loads_container(args: &[String]) -> bool {
    args.first().map(String::as_str) == Some("snapshot")
        && args.get(1).map(String::as_str) == Some("load")
        && args
            .windows(2)
            .any(|pair| pair[0] == LOAD_RUNTIME.0 && pair[1] == LOAD_RUNTIME.1)
}

/// What `restore` asks for: the checkpoint, and what the new computer gets from the caller.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Restore {
    pub group: String,
    pub member: String,
    pub name: String,
    pub cpus: Option<u64>,
    pub memory_mib: Option<u64>,
    pub labels: Vec<(String, String)>,
    pub env: Vec<(String, String)>,
    pub secrets: Vec<SecretSpec>,
    /// Whether the caller asked for no network (`--net-default-egress deny`), as it does for a
    /// computer made from an import.
    pub network_blocked: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Create {
    pub name: String,
    pub cpus: u64,
    pub max_cpus: u64,
    pub memory_mib: u64,
    pub max_memory_mib: u64,
    pub root_disk_mib: u64,
    pub workspace_mib: u64,
    pub labels: Vec<(String, String)>,
    pub env: Vec<(String, String)>,
    /// Whether the create carries the GitHub access secret.
    pub github: bool,
}

/// A secret named by `--secret NAME:passthrough=*@host,host`: the guest-visible name and the
/// hosts the gateway may substitute it for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SecretSpec {
    pub name: String,
    pub allowed_hosts: Vec<String>,
}

/// The secret changes of one `modify`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Modify {
    pub name: String,
    /// Secrets to add or update (`--secret`), the GitHub one included.
    pub put: Vec<SecretSpec>,
    /// Secrets to remove (`--secret-rm`).
    pub remove: Vec<String>,
    /// The change is recorded and applies at the next start (`--next-start`).
    pub next_start: bool,
}

/// `ssh serve NAME` with the flags the engine's callers pass, in MicroSandbox's shape.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SshServe {
    pub name: String,
    /// One connection over standard input and output (an editor's `ProxyCommand`).
    pub stdio: bool,
    /// Never start the computer; it must already be running.
    pub no_start: bool,
    pub no_inactivity_timeout: bool,
    /// Exit when the owner closes standard input, and print the readiness line after binding.
    pub exit_on_stdin_close: bool,
    pub authorized_keys: Option<std::path::PathBuf>,
    pub expected_machine_id: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Exec {
    pub name: String,
    pub user: Option<String>,
    pub env: Vec<(String, String)>,
    pub workdir: Option<String>,
    pub no_start: bool,
    pub timeout: Option<Duration>,
    /// Run in a pseudo-terminal (`--tty`).
    pub tty: bool,
    /// Where `crun` hands the pseudo-terminal's controlling side. Without it a `tty` command uses
    /// the terminal the caller is attached to.
    pub console_socket: Option<std::path::PathBuf>,
    /// A command the engine runs itself (as root, in the computer): it gets the engine's fixed
    /// environment only. The computer's own variables (`PATH`, `LD_PRELOAD`, `HOME`...) are the
    /// owner's to set for the owner's commands; they must not steer a program an account can
    /// replace when it runs as root.
    pub fixed_environment: bool,
    pub argv: Vec<String>,
}

/// What a terminal runs when it names no command.
pub const LOGIN_SHELL: [&str; 2] = ["/bin/bash", "-l"];

/// The computer a command is about, for the routing decision, without parsing the rest.
pub fn target_name(args: &[String]) -> Option<&str> {
    let verb = args.first()?.as_str();
    match verb {
        "ssh" => (args.get(1).map(String::as_str) == Some("serve"))
            .then(|| args.get(2).map(String::as_str))
            .flatten()
            .filter(|name| !name.starts_with('-')),
        "inspect" | "start" | "restart" | "exec" | "rename" | "modify" | "cp" | "fork"
        | "pause" | "resume" => args
            .get(1)
            .map(String::as_str)
            .filter(|name| !name.starts_with('-')),
        "stop" | "remove" => args
            .iter()
            .skip(1)
            .map(String::as_str)
            .find(|arg| !arg.starts_with('-')),
        "snapshot" => flag_value(args, "--from-sandbox"),
        "restore" => flag_value(args, "--name"),
        "create" => flag_value(args, "--name"),
        _ => None,
    }
}

fn flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1].as_str())
}

/// The checkpoint group a `snapshot` or `restore` command names, when it names one (`snapshot
/// list` without `--group` names none).
pub fn snapshot_group(args: &[String]) -> Option<&str> {
    match args.first().map(String::as_str)? {
        "snapshot" => match args.get(1).map(String::as_str)? {
            "create" | "list" => flag_value(args, "--group"),
            "verify" | "remove" | "head" | "save" => args
                .iter()
                .skip(2)
                .find(|arg| !arg.starts_with('-'))
                .map(|selector| {
                    selector
                        .split_once(':')
                        .map_or(selector.as_str(), |(group, _)| group)
                }),
            _ => None,
        },
        "restore" => args
            .get(1)
            .filter(|selector| !selector.starts_with('-'))
            .and_then(|selector| selector.split_once(':').map(|(group, _)| group)),
        _ => None,
    }
}

/// Whether a `create` asks for a container computer.
pub fn creates_container(args: &[String]) -> bool {
    args.first().map(String::as_str) == Some("create")
        && args
            .windows(2)
            .any(|pair| pair[0] == "--label" && pair[1] == RUNTIME_LABEL)
}

pub fn parse(args: &[String]) -> Result<Command, String> {
    let verb = args
        .first()
        .ok_or("No runtime command was given.")?
        .as_str();
    match verb {
        "list" => Ok(Command::List {
            label: flag_value(args, "--label").map(str::to_owned),
        }),
        "inspect" | "start" | "stop" | "remove" => {
            let name = target_name(args)
                .ok_or_else(|| format!("{verb} needs a computer name."))?
                .to_owned();
            Ok(match verb {
                "inspect" => Command::Inspect { name },
                "start" => Command::Start { name },
                "stop" => Command::Stop { name },
                _ => Command::Remove { name },
            })
        }
        "create" => parse_create(args).map(|create| Command::Create(Box::new(create))),
        "exec" => parse_exec(args).map(|exec| Command::Exec(Box::new(exec))),
        "ssh" => parse_ssh(args).map(|serve| Command::SshServe(Box::new(serve))),
        "pause" | "resume" => {
            let name = target_name(args)
                .ok_or_else(|| format!("{verb} needs a computer name."))?
                .to_owned();
            Ok(if verb == "pause" {
                Command::Pause { name }
            } else {
                Command::Resume { name }
            })
        }
        "snapshot" => parse_snapshot(args),
        "restore" => parse_restore(args).map(|restore| Command::Restore(Box::new(restore))),
        "modify" => Ok(match parse_modify(args)? {
            Some(modify) => Command::Modify(Box::new(modify)),
            None => Command::Unsupported {
                verb: "modify".into(),
            },
        }),
        other => Ok(Command::Unsupported {
            verb: other.to_owned(),
        }),
    }
}

/// The GitHub access secret every Silo computer is created with (the same text a virtual
/// machine's create passes). Any other `--secret` on a create is refused: general secrets are
/// assigned afterwards.
pub const GITHUB_SECRET_NAME: &str = "SILO_GITHUB";

/// `NAME:passthrough=*@host1,host2`.
pub fn parse_secret_spec(text: &str) -> Option<SecretSpec> {
    let (name, rest) = text.split_once(':')?;
    let hosts = rest.strip_prefix("passthrough=*@")?;
    let allowed_hosts: Vec<String> = hosts.split(',').map(str::to_owned).collect();
    (!name.is_empty() && allowed_hosts.iter().all(|host| !host.is_empty())).then(|| SecretSpec {
        name: name.to_owned(),
        allowed_hosts,
    })
}

fn parse_create(args: &[String]) -> Result<Create, String> {
    let mut github = false;
    for text in repeated_values(args, "--secret") {
        match parse_secret_spec(text) {
            Some(spec) if spec.name == GITHUB_SECRET_NAME && !github => github = true,
            _ => return Err(crate::containers::capability::refusal("give secrets to")),
        }
    }
    if args.iter().any(|arg| arg.starts_with("--secret=")) {
        return Err(crate::containers::capability::refusal("give secrets to"));
    }
    let name = flag_value(args, "--name")
        .ok_or("create needs a computer name.")?
        .to_owned();
    let number = |flag: &str| -> Result<u64, String> {
        flag_value(args, flag)
            .and_then(|value| value.parse().ok())
            .ok_or_else(|| format!("create needs a number for {flag}."))
    };
    let cpus = number("--cpus")?;
    let max_cpus = number("--max-cpus").unwrap_or(cpus);
    let gib = |flag: &str| -> Result<u64, String> {
        flag_value(args, flag)
            .and_then(parse_size_mib)
            .ok_or_else(|| format!("create needs a size for {flag}."))
    };
    let memory_mib = gib("--memory")?;
    let max_memory_mib = gib("--max-memory").unwrap_or(memory_mib);
    let root_disk_mib = gib("--root-disk")?;
    let workspace_mib = args
        .windows(2)
        .find(|pair| pair[0] == "--mount-owned")
        .and_then(|pair| {
            pair[1]
                .split(',')
                .find_map(|part| part.strip_prefix("size="))
                .and_then(parse_size_mib)
        })
        .ok_or("create needs a workspace size.")?;
    Ok(Create {
        name,
        cpus,
        max_cpus,
        memory_mib,
        max_memory_mib,
        root_disk_mib,
        workspace_mib,
        labels: repeated_pairs(args, "--label"),
        env: repeated_pairs(args, "--env"),
        github,
    })
}

fn selector(text: &str) -> Result<(String, String), String> {
    text.split_once(':')
        .filter(|(group, name)| !group.is_empty() && !name.is_empty())
        .map(|(group, name)| (group.to_owned(), name.to_owned()))
        .ok_or_else(|| "A checkpoint is named GROUP:NAME.".to_owned())
}

fn parse_snapshot(args: &[String]) -> Result<Command, String> {
    let sub = args
        .get(1)
        .map(String::as_str)
        .ok_or("snapshot needs a verb.")?;
    let positional: Vec<&String> = {
        let mut found = Vec::new();
        let mut index = 2;
        while index < args.len() {
            match args[index].as_str() {
                "--from-sandbox" | "--group" | "--guest-flush" | "--format" | "--runtime"
                | "--stage-id" => index += 2,
                flag if flag.starts_with('-') => index += 1,
                _ => {
                    found.push(&args[index]);
                    index += 1;
                }
            }
        }
        found
    };
    match sub {
        "create" => {
            if args.iter().any(|arg| arg == "--full") {
                return Err(crate::containers::capability::refusal(
                    crate::containers::capability::Capability::MemorySnapshots.verb(),
                ));
            }
            Ok(Command::SnapshotCreate(Box::new(SnapshotCreate {
                name: positional
                    .first()
                    .ok_or("snapshot create needs a name.")?
                    .to_string(),
                computer: flag_value(args, "--from-sandbox")
                    .ok_or("snapshot create needs --from-sandbox.")?
                    .to_owned(),
                group: flag_value(args, "--group")
                    .ok_or("snapshot create needs --group.")?
                    .to_owned(),
            })))
        }
        "list" => Ok(Command::SnapshotList {
            group: flag_value(args, "--group").map(str::to_owned),
        }),
        "verify" | "remove" => {
            let (group, name) = selector(
                positional
                    .first()
                    .ok_or("A checkpoint is named GROUP:NAME.")?,
            )?;
            Ok(if sub == "verify" {
                Command::SnapshotVerify { group, name }
            } else {
                Command::SnapshotRemove { group, name }
            })
        }
        "head" => {
            let (group, _) = positional
                .first()
                .map(|text| text.split_once(':').unwrap_or((text.as_str(), "")))
                .ok_or("snapshot head needs a group.")?;
            Ok(Command::SnapshotHead {
                group: group.to_owned(),
            })
        }
        "save" => {
            let (group, name) = selector(
                positional
                    .first()
                    .ok_or("A checkpoint is named GROUP:NAME.")?,
            )?;
            Ok(Command::SnapshotSave(Box::new(SnapshotSave {
                group,
                name,
                file: positional
                    .get(1)
                    .ok_or("snapshot save needs a file.")?
                    .as_str()
                    .into(),
            })))
        }
        "load" => Ok(Command::SnapshotLoad(Box::new(SnapshotLoad {
            file: positional
                .first()
                .ok_or("snapshot load needs a file.")?
                .as_str()
                .into(),
            group: flag_value(args, "--group")
                .ok_or("snapshot load needs --group.")?
                .to_owned(),
        }))),
        other => Ok(Command::Unsupported {
            verb: format!("snapshot {other}"),
        }),
    }
}

fn parse_restore(args: &[String]) -> Result<Restore, String> {
    let (group, member) = selector(
        args.get(1)
            .filter(|text| !text.starts_with('-'))
            .ok_or("restore needs GROUP:NAME.")?,
    )?;
    let mut restore = Restore {
        group,
        member,
        ..Restore::default()
    };
    let mut index = 2;
    while index < args.len() {
        let flag = args[index].as_str();
        let value = |at: usize| args.get(at).ok_or_else(|| format!("{flag} needs a value."));
        if let Some(pair) = flag.strip_prefix("--env=") {
            let (key, text) = pair.split_once('=').ok_or("--env needs KEY=VALUE.")?;
            restore.env.push((key.to_owned(), text.to_owned()));
            index += 1;
            continue;
        }
        match flag {
            "--name" => restore.name = value(index + 1)?.clone(),
            "--cpus" => restore.cpus = value(index + 1)?.parse().ok(),
            "--memory" => restore.memory_mib = parse_size_mib(value(index + 1)?),
            "--label" | "--env" => {
                let (key, text) = value(index + 1)?
                    .split_once('=')
                    .ok_or_else(|| format!("{flag} needs KEY=VALUE."))?;
                let pair = (key.to_owned(), text.to_owned());
                if flag == "--label" {
                    restore.labels.push(pair);
                } else {
                    restore.env.push(pair);
                }
            }
            "--secret" => restore.secrets.push(
                parse_secret_spec(value(index + 1)?)
                    .ok_or("--secret needs NAME:passthrough=*@HOSTS.")?,
            ),
            // A container's network is its gateway's policy or none: the default for outgoing
            // connections says which, and there are no rules to keep.
            "--net-default-egress" => {
                restore.network_blocked = match value(index + 1)?.as_str() {
                    "deny" => true,
                    "allow" => false,
                    _ => return Err("--net-default-egress needs allow or deny.".into()),
                }
            }
            "--net-default-ingress" => {
                value(index + 1)?;
            }
            // A container has no host mounts a snapshot could record.
            "--disk-only" | "--allow-missing-resources" => {
                index += 1;
                continue;
            }
            "--cow-mem" => {
                return Err(crate::containers::capability::refusal(
                    crate::containers::capability::Capability::MemorySnapshots.verb(),
                ))
            }
            _ => {
                return Err(format!(
                    "restore does not take {flag} for a container computer."
                ))
            }
        }
        index += 2;
    }
    if restore.name.is_empty() {
        return Err("restore needs --name.".into());
    }
    Ok(restore)
}

fn repeated_values<'a>(args: &'a [String], flag: &str) -> Vec<&'a str> {
    args.windows(2)
        .filter(|pair| pair[0] == flag)
        .map(|pair| pair[1].as_str())
        .collect()
}

/// `modify NAME` with only secret flags; `None` when it asks for anything else (resources,
/// labels, environment), which a container computer does not answer yet.
fn parse_modify(args: &[String]) -> Result<Option<Modify>, String> {
    let name = target_name(args)
        .ok_or("modify needs a computer name.")?
        .to_owned();
    let mut modify = Modify {
        name,
        put: Vec::new(),
        remove: Vec::new(),
        next_start: false,
    };
    let mut index = 2;
    while index < args.len() {
        match args[index].as_str() {
            "--secret" => {
                let text = args.get(index + 1).ok_or("--secret needs a value.")?;
                modify.put.push(
                    parse_secret_spec(text).ok_or("--secret needs NAME:passthrough=*@HOSTS.")?,
                );
                index += 2;
            }
            "--secret-rm" => {
                let text = args.get(index + 1).ok_or("--secret-rm needs a name.")?;
                modify.remove.push(text.clone());
                index += 2;
            }
            "--next-start" => {
                modify.next_start = true;
                index += 1;
            }
            "--format" => index += 2,
            _ => return Ok(None),
        }
    }
    Ok(Some(modify))
}

fn repeated_pairs(args: &[String], flag: &str) -> Vec<(String, String)> {
    args.windows(2)
        .filter(|pair| pair[0] == flag)
        .filter_map(|pair| pair[1].split_once('='))
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect()
}

fn parse_exec(args: &[String]) -> Result<Exec, String> {
    let name = target_name(args)
        .ok_or("exec needs a computer name.")?
        .to_owned();
    let separator = args.iter().position(|arg| arg == "--");
    let options = &args[2..separator.unwrap_or(args.len())];
    let argv = separator.map_or_else(Vec::new, |at| args[at + 1..].to_vec());
    // A terminal without a command is the working account's login shell, as the runtime's is.
    let wants_shell = options.iter().any(|option| option == "--tty") && argv.is_empty();
    let argv = if wants_shell {
        LOGIN_SHELL.iter().map(|part| (*part).to_owned()).collect()
    } else if argv.is_empty() {
        return Err("exec needs a command after `--`.".into());
    } else {
        argv
    };
    let mut exec = Exec {
        name,
        user: None,
        env: Vec::new(),
        workdir: None,
        no_start: false,
        timeout: None,
        tty: false,
        console_socket: None,
        fixed_environment: false,
        argv,
    };
    let mut index = 0;
    while index < options.len() {
        let flag = options[index].as_str();
        let value = options.get(index + 1);
        match flag {
            "--user" | "--env" | "--workdir" | "--timeout" => {
                let value = value.ok_or_else(|| format!("{flag} needs a value."))?;
                match flag {
                    "--user" => exec.user = Some(value.clone()),
                    "--env" => {
                        let (key, text) = value.split_once('=').ok_or("--env needs KEY=VALUE.")?;
                        exec.env.push((key.to_owned(), text.to_owned()));
                    }
                    "--workdir" => exec.workdir = Some(value.clone()),
                    _ => {
                        exec.timeout = Some(
                            parse_duration(value)
                                .ok_or_else(|| format!("{value} is not a duration."))?,
                        )
                    }
                }
                index += 2;
            }
            "--no-start" => {
                exec.no_start = true;
                index += 1;
            }
            "--no-tty" | "--quiet" | "--stream" | "--no-stdin" => index += 1,
            "--tty" => {
                exec.tty = true;
                index += 1;
            }
            other => {
                return Err(format!(
                    "exec option {other} is not available for container computers."
                ))
            }
        }
    }
    Ok(exec)
}

/// `ssh serve NAME [--stdio] [--no-start] [--no-inactivity-timeout] [--exit-on-stdin-close]
/// [--authorized-keys PATH] [--expected-machine-id ID] [--host HOST] [--port PORT]`.
fn parse_ssh(args: &[String]) -> Result<SshServe, String> {
    if args.get(1).map(String::as_str) != Some("serve") {
        return Err("Only `ssh serve` is available for container computers.".into());
    }
    let name = target_name(args)
        .ok_or("ssh serve needs a computer name.")?
        .to_owned();
    let mut serve = SshServe {
        name,
        ..SshServe::default()
    };
    let mut index = 3;
    while index < args.len() {
        let flag = args[index].as_str();
        let next = args.get(index + 1).cloned();
        let value = || next.clone().ok_or_else(|| format!("{flag} needs a value."));
        match flag {
            "--stdio" => serve.stdio = true,
            "--no-start" => serve.no_start = true,
            "--no-inactivity-timeout" => serve.no_inactivity_timeout = true,
            "--exit-on-stdin-close" => serve.exit_on_stdin_close = true,
            "--authorized-keys" => {
                serve.authorized_keys = Some(value()?.into());
                index += 1;
            }
            "--expected-machine-id" => {
                serve.expected_machine_id = Some(value()?);
                index += 1;
            }
            "--host" => {
                serve.host = Some(value()?);
                index += 1;
            }
            "--port" => {
                serve.port = Some(
                    value()?
                        .parse()
                        .map_err(|_| "--port needs a port number.".to_owned())?,
                );
                index += 1;
            }
            other => {
                return Err(format!(
                    "ssh serve option {other} is not available for container computers."
                ))
            }
        }
        index += 1;
    }
    if serve.expected_machine_id.is_some() && !serve.no_start {
        return Err("--expected-machine-id needs --no-start.".into());
    }
    if serve.exit_on_stdin_close && !serve.no_start {
        return Err("--exit-on-stdin-close needs --no-start.".into());
    }
    if serve.exit_on_stdin_close && serve.stdio {
        return Err("--exit-on-stdin-close cannot be used with --stdio.".into());
    }
    Ok(serve)
}

/// `2G`, `512M`, `2048` (mebibytes) as mebibytes.
pub fn parse_size_mib(text: &str) -> Option<u64> {
    let (digits, unit) = match text.char_indices().find(|(_, c)| !c.is_ascii_digit()) {
        Some((index, _)) => text.split_at(index),
        None => (text, "M"),
    };
    let value: u64 = digits.parse().ok()?;
    match unit {
        "G" | "g" | "GiB" => value.checked_mul(1024),
        "M" | "m" | "MiB" => Some(value),
        _ => None,
    }
}

/// `30s`, `5m`, `30m`, `1h`.
pub fn parse_duration(text: &str) -> Option<Duration> {
    let (digits, unit) = text.split_at(text.find(|c: char| !c.is_ascii_digit())?);
    let value: u64 = digits.parse().ok()?;
    let seconds = match unit {
        "s" => value,
        "m" => value.checked_mul(60)?,
        "h" => value.checked_mul(3600)?,
        _ => return None,
    };
    Some(Duration::from_secs(seconds))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| (*part).to_owned()).collect()
    }

    #[test]
    fn the_names_the_engine_uses_route_by_position() {
        assert_eq!(
            target_name(&args(&["inspect", "dev", "--format", "json"])),
            Some("dev")
        );
        assert_eq!(target_name(&args(&["stop", "dev", "--quiet"])), Some("dev"));
        assert_eq!(target_name(&args(&["stop", "--force", "dev"])), Some("dev"));
        assert_eq!(
            target_name(&args(&["pause", "dev", "--guest-flush", "required"])),
            Some("dev")
        );
        assert_eq!(target_name(&args(&["resume", "dev"])), Some("dev"));
        assert_eq!(
            snapshot_group(&args(&["snapshot", "verify", "box:c1"])),
            Some("box")
        );
        assert_eq!(
            snapshot_group(&args(&["snapshot", "remove", "--quiet", "box:c1"])),
            Some("box")
        );
        assert_eq!(
            snapshot_group(&args(&["snapshot", "list", "--format", "json"])),
            None
        );
        assert_eq!(
            snapshot_group(&args(&["snapshot", "list", "--group", "box"])),
            Some("box")
        );
        assert_eq!(
            snapshot_group(&args(&["restore", "box:c1", "--name", "kid"])),
            Some("box")
        );
        assert_eq!(snapshot_group(&args(&["restore", "--name", "kid"])), None);
        assert_eq!(
            target_name(&args(&["remove", "--quiet", "dev"])),
            Some("dev")
        );
        assert_eq!(
            target_name(&args(&[
                "snapshot",
                "create",
                "older",
                "--from-sandbox",
                "dev",
                "--group",
                "dev"
            ])),
            Some("dev")
        );
        assert_eq!(target_name(&args(&["list", "--format", "json"])), None);
        assert_eq!(target_name(&args(&["--silo-github-protocol"])), None);
    }

    #[test]
    fn a_checkpoint_named_like_a_computer_does_not_route_to_it() {
        assert_eq!(
            target_name(&args(&[
                "snapshot",
                "create",
                "dev",
                "--from-sandbox",
                "vm",
                "--group",
                "vm"
            ])),
            Some("vm")
        );
    }

    fn create_args() -> Vec<String> {
        [
            "create",
            "img",
            "--name",
            "c",
            "--cpus",
            "1",
            "--memory",
            "1G",
            "--root-disk",
            "1G",
            "--mount-owned",
            "/workspace:kind=disk,size=1G",
            "--label",
            RUNTIME_LABEL,
        ]
        .iter()
        .map(|part| part.to_string())
        .collect()
    }

    #[test]
    fn only_the_github_secret_is_accepted_on_create_and_any_other_is_refused() {
        let mut create = create_args();
        let Command::Create(plain) = parse(&create).unwrap() else {
            panic!("a create")
        };
        assert!(!plain.github);
        create.extend([
            "--secret".to_owned(),
            "SILO_GITHUB:passthrough=*@github.com,api.github.com,uploads.github.com".to_owned(),
            "--env".to_owned(),
            "GH_TOKEN=$MSB_SILO_GITHUB".to_owned(),
        ]);
        let Command::Create(github) = parse(&create).unwrap() else {
            panic!("a create")
        };
        assert!(github.github);
        assert_eq!(
            github.env,
            [("GH_TOKEN".to_owned(), "$MSB_SILO_GITHUB".to_owned())]
        );
        for other in [
            "TOKEN:passthrough=*@api.example.test",
            "GITHUB=x@api.github.com",
        ] {
            let mut create = create_args();
            create.extend(["--secret".to_owned(), other.to_owned()]);
            let error = parse(&create).unwrap_err();
            assert!(
                error.contains("cannot give secrets to a container computer yet"),
                "{error}"
            );
        }
        let mut twice = create_args();
        for _ in 0..2 {
            twice.extend([
                "--secret".to_owned(),
                "SILO_GITHUB:passthrough=*@github.com,api.github.com".to_owned(),
            ]);
        }
        assert!(parse(&twice).is_err());
    }

    #[test]
    fn the_modify_commands_the_secret_code_issues_are_read() {
        let command = parse(&args(&[
            "modify",
            "dev",
            "--secret-rm",
            "OLD",
            "--secret",
            "TOKEN:passthrough=*@a.example,b.example",
            "--next-start",
            "--format",
            "json",
        ]))
        .unwrap();
        assert_eq!(
            command,
            Command::Modify(Box::new(Modify {
                name: "dev".into(),
                put: vec![SecretSpec {
                    name: "TOKEN".into(),
                    allowed_hosts: vec!["a.example".into(), "b.example".into()],
                }],
                remove: vec!["OLD".into()],
                next_start: true,
            }))
        );
        // Resources, labels and environment are not secrets.
        for other in [
            args(&["modify", "dev", "--cpus", "2"]),
            args(&["modify", "dev", "--env-rm", "GIT_AUTHOR_NAME"]),
        ] {
            assert_eq!(
                parse(&other).unwrap(),
                Command::Unsupported {
                    verb: "modify".into()
                }
            );
        }
        assert!(parse(&args(&["modify", "dev", "--secret", "oops"])).is_err());
    }

    #[test]
    fn create_is_recognised_by_its_runtime_label() {
        let create = args(&["create", "image", "--name", "dev", "--label", RUNTIME_LABEL]);
        assert!(creates_container(&create));
        assert!(!creates_container(&args(&[
            "create", "image", "--name", "dev"
        ])));
        assert!(!creates_container(&args(&[
            "stop",
            "--label",
            RUNTIME_LABEL
        ])));
    }

    #[test]
    fn the_create_arguments_the_engine_builds_are_read() {
        let create = args(&[
            "create",
            "image",
            "--pull",
            "never",
            "--name",
            "dev",
            "--cpus",
            "2",
            "--max-cpus",
            "4",
            "--memory",
            "2G",
            "--max-memory",
            "4G",
            "--root-disk",
            "8G",
            "--mount-owned",
            "/workspace:kind=disk,size=3G",
            "--label",
            "silo.managed=true",
            "--label",
            RUNTIME_LABEL,
            "--env",
            "A=b=c",
            "--no-start",
            "--quiet",
        ]);
        let Command::Create(create) = parse(&create).unwrap() else {
            panic!("not a create");
        };
        assert_eq!(create.name, "dev");
        assert_eq!((create.cpus, create.max_cpus), (2, 4));
        assert_eq!((create.memory_mib, create.max_memory_mib), (2048, 4096));
        assert_eq!((create.root_disk_mib, create.workspace_mib), (8192, 3072));
        assert!(create
            .labels
            .contains(&("silo.managed".into(), "true".into())));
        assert_eq!(create.env, vec![("A".into(), "b=c".into())]);
    }

    #[test]
    fn the_exec_arguments_the_engine_builds_are_read() {
        let exec = args(&[
            "exec",
            "dev",
            "--no-start",
            "--no-tty",
            "--quiet",
            "--timeout",
            "30s",
            "--user",
            "root",
            "--env",
            "USER=root",
            "--workdir",
            "/",
            "--",
            "sh",
            "-c",
            "echo $USER",
        ]);
        let Command::Exec(exec) = parse(&exec).unwrap() else {
            panic!("not an exec");
        };
        assert_eq!(exec.name, "dev");
        assert!(exec.no_start);
        assert_eq!(exec.timeout, Some(Duration::from_secs(30)));
        assert_eq!(exec.user.as_deref(), Some("root"));
        assert_eq!(exec.env, vec![("USER".into(), "root".into())]);
        assert_eq!(exec.workdir.as_deref(), Some("/"));
        assert_eq!(exec.argv, ["sh", "-c", "echo $USER"]);
    }

    #[test]
    fn unknown_exec_options_and_a_missing_command_are_refused() {
        assert!(parse(&args(&["exec", "dev", "--rlimit", "x", "--", "sh"])).is_err());
        assert!(parse(&args(&["exec", "dev", "--user", "root"])).is_err());
    }

    #[test]
    fn a_terminal_runs_the_given_command_or_the_login_shell() {
        let Command::Exec(shell) = parse(&args(&[
            "exec",
            "dev",
            "--user",
            "silo",
            "--env",
            "USER=silo",
            "--no-start",
            "--workdir",
            "/workspace",
            "--tty",
        ]))
        .unwrap() else {
            panic!("an exec")
        };
        assert!(shell.tty && shell.console_socket.is_none());
        assert_eq!(shell.argv, ["/bin/bash", "-l"]);
        assert_eq!(shell.workdir.as_deref(), Some("/workspace"));
        let Command::Exec(given) = parse(&args(&["exec", "dev", "--tty", "--", "top"])).unwrap()
        else {
            panic!("an exec")
        };
        assert!(given.tty);
        assert_eq!(given.argv, ["top"]);
        let Command::Exec(plain) = parse(&args(&["exec", "dev", "--", "id"])).unwrap() else {
            panic!("an exec")
        };
        assert!(!plain.tty);
    }

    #[test]
    fn ssh_serve_takes_the_flags_the_engines_callers_pass() {
        // The editor's ProxyCommand.
        let Command::SshServe(editor) = parse(&args(&[
            "ssh",
            "serve",
            "dev",
            "--stdio",
            "--no-start",
            "--no-inactivity-timeout",
        ]))
        .unwrap() else {
            panic!("an ssh serve")
        };
        assert_eq!(
            *editor,
            SshServe {
                name: "dev".into(),
                stdio: true,
                no_start: true,
                no_inactivity_timeout: true,
                ..SshServe::default()
            }
        );
        // SSH access's listener.
        let Command::SshServe(listener) = parse(&args(&[
            "ssh",
            "serve",
            "dev",
            "--no-start",
            "--no-inactivity-timeout",
            "--exit-on-stdin-close",
            "--authorized-keys",
            "/h/keys",
            "--expected-machine-id",
            "abc",
            "--host",
            "127.0.0.1",
            "--port",
            "2222",
        ]))
        .unwrap() else {
            panic!("an ssh serve")
        };
        assert_eq!(
            listener.authorized_keys.as_deref(),
            Some(std::path::Path::new("/h/keys"))
        );
        assert_eq!(
            (
                listener.expected_machine_id.as_deref(),
                listener.host.as_deref(),
                listener.port
            ),
            (Some("abc"), Some("127.0.0.1"), Some(2222))
        );
        assert!(listener.exit_on_stdin_close && !listener.stdio);
        assert_eq!(
            target_name(&args(&["ssh", "serve", "dev", "--stdio"])),
            Some("dev")
        );
        assert_eq!(target_name(&args(&["ssh", "authorize", "dev"])), None);
    }

    #[test]
    fn ssh_serve_refuses_what_the_runtime_refuses_and_what_it_does_not_know() {
        for bad in [
            // These need --no-start, and the owner's pipe cannot be the connection.
            vec!["ssh", "serve", "dev", "--expected-machine-id", "abc"],
            vec!["ssh", "serve", "dev", "--exit-on-stdin-close"],
            vec![
                "ssh",
                "serve",
                "dev",
                "--no-start",
                "--exit-on-stdin-close",
                "--stdio",
            ],
            vec!["ssh", "serve", "dev", "--port", "http"],
            vec!["ssh", "serve", "dev", "--port"],
            vec!["ssh", "serve", "dev", "--bogus"],
            vec!["ssh", "serve"],
            vec!["ssh", "authorize", "dev"],
        ] {
            assert!(parse(&args(&bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn sizes_and_durations() {
        assert_eq!(parse_size_mib("2G"), Some(2048));
        assert_eq!(parse_size_mib("512M"), Some(512));
        assert_eq!(parse_size_mib("1T"), None);
        assert_eq!(parse_duration("30m"), Some(Duration::from_secs(1800)));
        assert_eq!(parse_duration("45"), None);
    }

    #[test]
    fn commands_only_virtual_machines_answer_are_named() {
        assert_eq!(
            parse(&args(&["rename", "dev", "x"])).unwrap(),
            Command::Unsupported {
                verb: "rename".into()
            }
        );
    }
}
