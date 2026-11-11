//! One reconcile of one computer: read what it has, decide (`plan`), do the steps, record what
//! happened. A step that fails is recorded and the next one still runs; only a computer that
//! cannot be read at all returns an error, so its caller tries again soon.

use std::time::Duration;

use serde_json::Value;

use super::{
    artifact::{Located, Payload},
    plan::{plan, Observed, Program, ProgramStep, RegistrationStep, Wanted},
    receipt::{Entry, Receipt, State, MAX_RECEIPT_BYTES},
    scripts::{self, Place},
    COMPONENTS, MCP, SILO,
};

const OBSERVE_TIMEOUT: Duration = Duration::from_secs(30);
const INSTALL_TIMEOUT: Duration = Duration::from_secs(170);
const STEP_TIMEOUT: Duration = Duration::from_secs(60);
const SMALL_OUTPUT: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Who {
    Root,
    Account,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    Linux,
    Macos,
}

#[derive(Debug)]
pub struct Output {
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// A computer that commands run in.
pub trait Guest {
    fn kind(&self) -> Kind;
    fn run(
        &self,
        who: Who,
        argv: Vec<String>,
        stdin: Option<Vec<u8>>,
        timeout: Duration,
        max_stdout: usize,
    ) -> Result<Output, String>;
}

impl Guest for crate::guest::Target {
    fn kind(&self) -> Kind {
        if self.is_macos() {
            Kind::Macos
        } else {
            Kind::Linux
        }
    }

    fn run(
        &self,
        who: Who,
        argv: Vec<String>,
        stdin: Option<Vec<u8>>,
        timeout: Duration,
        max_stdout: usize,
    ) -> Result<Output, String> {
        crate::guest::run_bounded(
            self,
            &crate::guest::ExecRequest {
                argv,
                cwd: None,
                env: Vec::new(),
                user: match who {
                    Who::Root => crate::guest::GuestUser::Root,
                    Who::Account => crate::guest::GuestUser::Account,
                },
                stdin,
                timeout,
                max_stdout,
            },
        )
        .map(|output| Output {
            code: output.exit_code,
            stdout: output.stdout,
            stderr: output.stderr,
        })
        .map_err(|error| error.message)
    }
}

/// Where the program comes from.
pub enum Source<'a> {
    /// What this build ships for the kind of computer.
    Located(&'a Located),
    /// A program the owner named for development, already read.
    Override(&'a Payload),
}

/// What is asked of a reconcile.
pub struct Request<'a> {
    pub computer_id: &'a str,
    pub tools: bool,
    pub removal_owed: bool,
    pub source: Source<'a>,
    /// The text of the registration helper to keep in the computer.
    pub helper_text: &'a str,
    pub now: u64,
}

struct Places {
    helper: Place,
    receipt: Place,
    helper_mode: &'static str,
}

fn places(kind: Kind) -> Places {
    match kind {
        Kind::Linux => Places {
            helper: scripts::LINUX_HELPER_PLACE,
            receipt: scripts::LINUX_RECEIPT_PLACE,
            helper_mode: "755",
        },
        Kind::Macos => Places {
            helper: scripts::MACOS_REGISTER_PLACE,
            receipt: scripts::MACOS_RECEIPT_PLACE,
            helper_mode: "755",
        },
    }
}

fn argv(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| (*word).to_owned()).collect()
}

fn failure_text(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let line = stderr
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("it exited without a message");
    format!(
        "{line} (exit {})",
        output.code.map_or("signal".to_owned(), |c| c.to_string())
    )
}

struct Seen {
    observed: Observed,
    helper_sha256: Option<String>,
}

fn observe(guest: &dyn Guest, computer_id: &str) -> Result<Seen, String> {
    let places = places(guest.kind());
    let output = guest.run(
        Who::Account,
        argv(&["sh", "-c", &scripts::observe(places.helper, places.receipt)]),
        None,
        OBSERVE_TIMEOUT,
        MAX_RECEIPT_BYTES + 4096,
    )?;
    if output.code != Some(0) {
        return Err(format!(
            "Silo could not look at the computer's components: {}",
            failure_text(&output)
        ));
    }
    let marker = b"receipt:\n";
    let split = output
        .stdout
        .windows(marker.len())
        .position(|window| window == marker)
        .ok_or("The computer answered a look at its components with something else.")?;
    let head = String::from_utf8_lossy(&output.stdout[..split]);
    let field = |name: &str| -> Option<String> {
        head.lines()
            .find_map(|line| line.strip_prefix(name))
            .map(str::trim)
            .filter(|value| {
                value.len() == 64
                    && value
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
            .map(str::to_owned)
    };
    Ok(Seen {
        observed: Observed {
            program_sha256: field("program="),
            receipt: Receipt::parse(&output.stdout[split + marker.len()..], computer_id),
        },
        helper_sha256: field("helper="),
    })
}

fn put_file(
    guest: &dyn Guest,
    who: Who,
    place: Place,
    mode: &str,
    bytes: &[u8],
) -> Result<(), String> {
    let output = guest.run(
        who,
        argv(&["sh", "-c", &scripts::write_file(place), "silo-write", mode]),
        Some(bytes.to_vec()),
        STEP_TIMEOUT,
        4096,
    )?;
    if output.code == Some(0) {
        Ok(())
    } else {
        Err(failure_text(&output))
    }
}

fn install_program(guest: &dyn Guest, payload: &Payload) -> Result<(), String> {
    let output = guest.run(
        Who::Root,
        argv(&[
            "sh",
            "-c",
            scripts::INSTALL_SCRIPT,
            "silo-install",
            &payload.sha256,
            scripts::PROGRAM_PATH,
            "0:0",
        ]),
        Some(payload.bytes.clone()),
        INSTALL_TIMEOUT,
        4096,
    )?;
    if output.code == Some(0) {
        Ok(())
    } else {
        Err(failure_text(&output))
    }
}

/// What `server-add` or `server-remove` printed.
struct Registration {
    registered: Vec<String>,
    pending: Vec<String>,
    failed: Vec<String>,
}

fn names(value: &Value, key: &str) -> Vec<String> {
    value[key]
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(Value::as_str)
                .filter(|name| name.len() <= 32)
                .map(str::to_owned)
                .take(8)
                .collect()
        })
        .unwrap_or_default()
}

fn register(guest: &dyn Guest, add: bool) -> Result<Registration, String> {
    let macos = guest.kind() == Kind::Macos;
    let who = if macos { Who::Account } else { Who::Root };
    let output = guest.run(
        who,
        scripts::registration(macos, add),
        None,
        STEP_TIMEOUT,
        SMALL_OUTPUT,
    )?;
    let last_line = String::from_utf8_lossy(&output.stdout)
        .lines()
        .rev()
        .find(|line| line.trim_start().starts_with('{'))
        .map(str::to_owned);
    let parsed = last_line.and_then(|line| serde_json::from_str::<Value>(&line).ok());
    match parsed {
        Some(value) => Ok(Registration {
            registered: names(&value, if add { "registered" } else { "removed" }),
            pending: names(&value, "pending"),
            failed: names(&value, "failed"),
        }),
        None => Err(failure_text(&output)),
    }
}

fn agent_words(names: &[String]) -> String {
    names
        .iter()
        .map(|name| match name.as_str() {
            "claude-code" => "Claude Code",
            "codex" => "Codex",
            other => other,
        })
        .collect::<Vec<_>>()
        .join(" and ")
}

/// One reconcile. Returns the computer's new receipt, which it has also been given when it could
/// take it.
pub fn reconcile(guest: &dyn Guest, request: &Request<'_>) -> Result<Receipt, String> {
    let seen = observe(guest, request.computer_id)?;
    // The program Silo has to give. A file whose checksum the manifest records is read only when
    // it is sent; the device's own program is read now, to know its checksum.
    let mut read: Option<Payload> = None;
    let program = match &request.source {
        Source::Override(payload) => Program::Available {
            sha256: payload.sha256.clone(),
        },
        Source::Located(located) => match located.unavailable_reason() {
            Some(reason) => Program::Unavailable {
                reason: reason.to_owned(),
            },
            None => match located {
                Located::File {
                    expected_sha256: Some(sha256),
                    ..
                } => Program::Available {
                    sha256: sha256.clone(),
                },
                _ => match located.load() {
                    Ok(payload) => {
                        let sha256 = payload.sha256.clone();
                        read = Some(payload);
                        Program::Available { sha256 }
                    }
                    Err(reason) => Program::Unavailable { reason },
                },
            },
        },
    };
    let wanted = Wanted {
        tools: request.tools,
        program,
        removal_owed: request.removal_owed,
    };
    let steps = plan(&wanted, &seen.observed);
    let at = request.now;
    let mut receipt = seen.observed.receipt.clone();
    receipt.computer_id = request.computer_id.to_owned();
    let mut program_present = seen.observed.program_sha256.is_some();

    match &steps.program {
        ProgramStep::Current => {
            if let Program::Available { sha256 } = &wanted.program {
                receipt.components.insert(
                    SILO.id.into(),
                    Entry::new(SILO.version, State::Ok, at).with_sha256(sha256),
                );
            }
        }
        ProgramStep::Install => {
            let payload = match (&request.source, read.as_ref()) {
                (Source::Override(payload), _) => Ok(*payload),
                (_, Some(payload)) => Ok(payload),
                (Source::Located(located), None) => {
                    read = located.load().ok();
                    read.as_ref().ok_or_else(|| {
                        "Silo's program could not be read or does not match its recorded checksum."
                            .to_owned()
                    })
                }
            };
            let result = payload.and_then(|payload| {
                install_program(guest, payload).map(|()| payload.sha256.clone())
            });
            match result {
                Ok(sha256) => {
                    program_present = true;
                    receipt.components.insert(
                        SILO.id.into(),
                        Entry::new(SILO.version, State::Ok, at).with_sha256(sha256),
                    );
                }
                Err(detail) => {
                    let mut entry = Entry::new(SILO.version, State::Failed, at).with_detail(
                        format!("The silo program could not be put in place: {detail}"),
                    );
                    entry.sha256 = seen.observed.program_sha256.clone();
                    receipt.components.insert(SILO.id.into(), entry);
                }
            }
        }
        ProgramStep::Keep { reason } | ProgramStep::Unavailable { reason } => {
            let mut entry = Entry::new(SILO.version, State::Unavailable, at).with_detail(reason);
            entry.sha256 = seen.observed.program_sha256.clone();
            receipt.components.insert(SILO.id.into(), entry);
        }
        ProgramStep::Leave => {}
    }

    // The registration helper is pushed when the computer's copy is missing or different.
    let needs_helper = matches!(
        steps.registration,
        RegistrationStep::Apply | RegistrationStep::Remove
    );
    let mut helper_error: Option<String> = None;
    if needs_helper {
        let places = places(guest.kind());
        let wanted_sha = super::artifact::sha256_hex(request.helper_text.as_bytes());
        if seen.helper_sha256.as_deref() != Some(wanted_sha.as_str()) {
            let who = if guest.kind() == Kind::Macos {
                Who::Account
            } else {
                Who::Root
            };
            helper_error = put_file(
                guest,
                who,
                places.helper,
                places.helper_mode,
                request.helper_text.as_bytes(),
            )
            .err();
        }
    }

    let mcp = match steps.registration {
        RegistrationStep::Nothing => None,
        RegistrationStep::WaitForProgram => {
            Some(Entry::new(MCP.version, State::Waiting, at).with_detail(
                "Silo's program is not in the computer, so there is nothing to register.",
            ))
        }
        RegistrationStep::Apply | RegistrationStep::Remove
            if !program_present && steps.registration == RegistrationStep::Apply =>
        {
            Some(
                Entry::new(MCP.version, State::Waiting, at)
                    .with_detail("Waiting for the silo program to be in the computer."),
            )
        }
        RegistrationStep::Apply | RegistrationStep::Remove => {
            let add = steps.registration == RegistrationStep::Apply;
            let result = match &helper_error {
                Some(error) => Err(format!(
                    "the registration helper could not be put in place: {error}"
                )),
                None => register(guest, add),
            };
            Some(match result {
                Ok(done) if !done.failed.is_empty() => Entry::new(MCP.version, State::Failed, at)
                    .with_detail(format!(
                        "Silo's tools could not be {} {}.",
                        if add {
                            "registered with"
                        } else {
                            "removed from"
                        },
                        agent_words(&done.failed)
                    )),
                Ok(done) if add && done.registered.is_empty() => {
                    Entry::new(MCP.version, State::Waiting, at).with_detail(format!(
                        "{} not installed yet; Silo's tools are registered when it is.",
                        agent_words(&done.pending)
                    ))
                }
                Ok(done) if add => {
                    let mut entry = Entry::new(MCP.version, State::Ok, at);
                    if !done.pending.is_empty() {
                        entry = entry.with_detail(format!(
                            "Registered with {}; {} not installed yet.",
                            agent_words(&done.registered),
                            agent_words(&done.pending)
                        ));
                    }
                    entry
                }
                Ok(_) => Entry::new(MCP.version, State::Removed, at),
                Err(detail) => Entry::new(MCP.version, State::Failed, at).with_detail(format!(
                    "Silo's tools could not be {} the agents: {detail}",
                    if add {
                        "registered with"
                    } else {
                        "removed from"
                    }
                )),
            })
        }
    };
    if let Some(entry) = mcp {
        receipt.components.insert(MCP.id.into(), entry);
    }
    debug_assert!(receipt
        .components
        .keys()
        .all(|id| COMPONENTS.iter().any(|c| c.id == id)));

    // Written last, so it records this run; a computer that cannot take it is still reconciled.
    let places = places(guest.kind());
    let who = if guest.kind() == Kind::Macos {
        Who::Account
    } else {
        Who::Root
    };
    if let Err(error) = put_file(
        guest,
        who,
        places.receipt,
        "644",
        receipt.to_json().as_bytes(),
    ) {
        eprintln!(
            "silo-engine: components of {}: the receipt could not be written: {error}",
            request.computer_id
        );
    }
    Ok(receipt)
}
