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
    /// Secrets and GitHub access: the only `modify` a container computer answers.
    Modify(Box<Modify>),
    /// A command only MicroSandbox computers answer yet.
    Unsupported {
        verb: String,
    },
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Exec {
    pub name: String,
    pub user: Option<String>,
    pub env: Vec<(String, String)>,
    pub workdir: Option<String>,
    pub no_start: bool,
    pub timeout: Option<Duration>,
    pub argv: Vec<String>,
}

/// The computer a command is about, for the routing decision, without parsing the rest.
pub fn target_name(args: &[String]) -> Option<&str> {
    let verb = args.first()?.as_str();
    match verb {
        "inspect" | "start" | "stop" | "restart" | "exec" | "rename" | "modify" | "cp" | "fork" => {
            args.get(1)
                .map(String::as_str)
                .filter(|name| !name.starts_with('-'))
        }
        "remove" => args
            .iter()
            .skip(1)
            .map(String::as_str)
            .find(|arg| !arg.starts_with('-')),
        "snapshot" => flag_value(args, "--sandbox"),
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
                modify
                    .put
                    .push(parse_secret_spec(text).ok_or("--secret needs NAME:passthrough=*@HOSTS.")?);
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
    let separator = args
        .iter()
        .position(|arg| arg == "--")
        .ok_or("exec needs a command after `--`.")?;
    let options = &args[2..separator];
    let argv = args[separator + 1..].to_vec();
    if argv.is_empty() {
        return Err("exec needs a command after `--`.".into());
    }
    let mut exec = Exec {
        name,
        user: None,
        env: Vec::new(),
        workdir: None,
        no_start: false,
        timeout: None,
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
            "--tty" => return Err("A terminal in a container computer comes later.".into()),
            other => {
                return Err(format!(
                    "exec option {other} is not available for container computers."
                ))
            }
        }
    }
    Ok(exec)
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
        assert_eq!(
            target_name(&args(&["remove", "--quiet", "dev"])),
            Some("dev")
        );
        assert_eq!(
            target_name(&args(&["snapshot", "create", "--sandbox", "dev", "older"])),
            Some("dev")
        );
        assert_eq!(target_name(&args(&["list", "--format", "json"])), None);
        assert_eq!(target_name(&args(&["--silo-github-protocol"])), None);
    }

    #[test]
    fn a_checkpoint_named_like_a_computer_does_not_route_to_it() {
        assert_eq!(
            target_name(&args(&["snapshot", "create", "--sandbox", "vm", "dev"])),
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
    fn a_terminal_and_unknown_exec_options_are_refused() {
        assert!(parse(&args(&["exec", "dev", "--tty", "--", "sh"])).is_err());
        assert!(parse(&args(&["exec", "dev", "--rlimit", "x", "--", "sh"])).is_err());
        assert!(parse(&args(&["exec", "dev", "--user", "root"])).is_err());
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
            parse(&args(&["snapshot", "create", "--sandbox", "dev", "x"])).unwrap(),
            Command::Unsupported {
                verb: "snapshot".into()
            }
        );
    }
}
