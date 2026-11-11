//! What Silo itself puts into a computer and keeps current: a short list of components with
//! versions, a receipt in the computer, and a reconcile at every start of the computer that
//! installs or replaces what is missing or old (`docs/SiloUI-GUEST-IMAGES.md`).
//!
//! The members today are the `silo` program at `/usr/local/bin/silo` and the registration of its
//! MCP server (`silo mcp`) with Claude Code and Codex. Each step's result is recorded in the
//! computer's receipt (`/var/lib/silo/components.json` on Linux, the matching Application Support
//! folder of the working account on macOS) and in a copy on the device that the computer's status
//! reads. A step that fails is recorded and never stops the computer from starting. Computer use
//! and the desktop streamer keep their own paths.

pub mod artifact;
pub mod plan;
pub mod receipt;
pub mod run;
mod scripts;
pub mod standing;

use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex, OnceLock},
};

use silo_api::methods::guest_client::{
    GuestComponentState, GuestComponentStatus, GUEST_ENDPOINT_PATH,
};

use crate::{
    guest::Target,
    guest_client::{now_secs, TokenStore},
};
use artifact::{Located, Payload};
use receipt::{Receipt, Reports, State};
use run::{Kind, Request, Source};

/// The path of the endpoint file, which tells a registration that the tools are on.
pub(crate) const ENDPOINT_PATH: &str = GUEST_ENDPOINT_PATH;
/// The name the MCP server is registered under in the agents.
pub const MCP_SERVER_NAME: &str = "silo";

/// A piece Silo keeps in a computer.
#[derive(Clone, Copy, Debug)]
pub struct Component {
    pub id: &'static str,
    /// Raised when what the piece is changes (its steps, its place), so that computers that
    /// recorded an earlier version are reconciled again.
    pub version: u32,
}

/// The `silo` program at `/usr/local/bin/silo`.
pub const SILO: Component = Component {
    id: "silo",
    version: 1,
};
/// `silo mcp` registered with Claude Code and Codex.
pub const MCP: Component = Component {
    id: "silo-mcp",
    version: 2,
};
/// Every component, in the order they are reconciled.
pub const COMPONENTS: &[Component] = &[SILO, MCP];

const LINUX_HELPER: &str = include_str!("../../../guest/silo-computer-use.py");
const MACOS_REGISTER: &str = include_str!("../../../guest/macos/silo-cua-register.py");

/// Where the program for each kind of computer comes from.
pub struct Sources<'a> {
    /// The engine's resources folder (`resources/guest-support/silo`).
    pub resources: &'a Path,
    /// The folder with the engine's own tools, where the device's `silo` is.
    pub tools_directory: Option<&'a Path>,
}

impl Sources<'_> {
    pub fn located(&self, macos: bool) -> Located {
        if macos {
            artifact::device(self.tools_directory)
        } else {
            artifact::linux(self.resources, artifact::host_architecture())
        }
    }
}

fn lock_for(computer_id: &str) -> Arc<Mutex<()>> {
    static LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();
    LOCKS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .entry(computer_id.to_owned())
        .or_default()
        .clone()
}

fn reports(app_data: &Path) -> Reports {
    Reports::new(&app_data.join("guest-client"))
}

fn helper_text(macos: bool) -> &'static str {
    if macos {
        MACOS_REGISTER
    } else {
        LINUX_HELPER
    }
}

/// Reconciles a running computer: installs or replaces the program when the computer's copy is
/// not the wanted one, registers (or, with the tools off, removes) the MCP server, and records
/// every result. `override_program` is a development program named by the owner; it stays in the
/// computer, whatever later looks find, until the computer starts again or the tools are turned
/// on again. An `Err` means the computer could not be read at all; step failures are in the
/// returned receipt.
pub fn reconcile(
    target: &Target,
    app_data: &Path,
    sources: &Sources<'_>,
    override_program: Option<&Payload>,
) -> Result<Receipt, String> {
    let lock = lock_for(target.id());
    let _guard = lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let store = TokenStore::new(app_data);
    let tools = store
        .status(target.id())?
        .is_some_and(|record| record.enabled());
    let reports = reports(app_data);
    let located = sources.located(target.is_macos());
    let request = Request {
        computer_id: target.id(),
        tools,
        removal_owed: reports.is_owed(target.id()),
        source: match override_program {
            Some(payload) => Source::Override(payload),
            None if standing::program_named(target.id()) => Source::Kept,
            None => Source::Located(&located),
        },
        helper_text: helper_text(target.is_macos()),
        now: now_secs(),
        budget: run::RECONCILE_BUDGET,
    };
    let receipt = match run::reconcile(target, &request) {
        Ok(receipt) => receipt,
        Err(error) => {
            standing::note_look(target.id(), true);
            return Err(error);
        }
    };
    standing::note_look(target.id(), went_wrong(&receipt));
    if override_program.is_some()
        && receipt
            .entry(SILO.id)
            .is_some_and(|entry| entry.state == State::Ok)
    {
        standing::name_program(target.id());
    }
    finish(&reports, &receipt, tools);
    Ok(receipt)
}

/// Whether a step failed or ran out of time.
fn went_wrong(receipt: &Receipt) -> bool {
    receipt.components.values().any(|entry| {
        entry.state == State::Failed || entry.detail.as_deref() == Some(run::OUT_OF_TIME)
    })
}

fn finish(reports: &Reports, receipt: &Receipt, tools: bool) {
    // A removal stays owed until the registration worked: made anew with the tools on, taken
    // out with them off. A step that is only waiting, failed or ran out of time changes nothing.
    let worked = receipt
        .entry(MCP.id)
        .is_some_and(|entry| entry.state == if tools { State::Ok } else { State::Removed });
    if worked {
        reports.clear_owed(&[receipt.computer_id.as_str()]);
    }
    if let Err(error) = reports.save(receipt) {
        eprintln!(
            "silo-engine: components of {}: {error}",
            receipt.computer_id
        );
    }
}

/// Takes Silo's MCP server out of the agents of a running computer whose tools were turned off.
/// The program stays where it is; it refuses to work without the endpoint file's token.
pub fn remove_registration(
    target: &Target,
    app_data: &Path,
    sources: &Sources<'_>,
) -> Result<Receipt, String> {
    // The tools are off, so a reconcile removes what is registered.
    reconcile(target, app_data, sources, None)
}

/// The computers whose registration is waiting to be taken out.
pub fn owed_removals(app_data: &Path) -> Vec<String> {
    reports(app_data).owed()
}

/// Notes that the tools of these computers were turned off: their registration is taken out the
/// next time a reconcile can reach them.
pub fn owe_removal(guest_client_directory: &Path, computer_ids: &[&str]) {
    if let Err(error) = Reports::new(guest_client_directory).owe_removal(computer_ids) {
        eprintln!("silo-engine: components: {error}");
    }
}

/// Forgets what is kept about a computer that no longer exists.
pub fn forget(guest_client_directory: &Path, computer_id: &str) {
    standing::start_over(computer_id);
    Reports::new(guest_client_directory).forget(computer_id);
}

fn state_of(state: State) -> GuestComponentState {
    match state {
        State::Ok => GuestComponentState::Ok,
        State::Failed => GuestComponentState::Failed,
        State::Unavailable => GuestComponentState::Unavailable,
        State::Waiting => GuestComponentState::Waiting,
        State::Removed => GuestComponentState::Removed,
    }
}

/// What the status of a computer shows about its components: the last reconcile, and for the
/// program, the build being unable to provide it even before any reconcile ran.
pub fn status(
    app_data: &Path,
    computer_id: &str,
    macos: bool,
    sources: &Sources<'_>,
    tools: bool,
) -> Vec<GuestComponentStatus> {
    let mut found: Vec<GuestComponentStatus> = reports(app_data)
        .read(computer_id)
        .map(|receipt| {
            COMPONENTS
                .iter()
                .filter_map(|component| {
                    receipt
                        .entry(component.id)
                        .map(|entry| GuestComponentStatus {
                            id: component.id.to_owned(),
                            version: entry.version,
                            state: state_of(entry.state),
                            detail: entry.detail.clone(),
                            at: entry.at,
                        })
                })
                .collect()
        })
        .unwrap_or_default();
    if tools && !found.iter().any(|entry| entry.id == SILO.id) {
        if let Some(reason) = sources.located(macos).unavailable_reason() {
            found.insert(
                0,
                GuestComponentStatus {
                    id: SILO.id.to_owned(),
                    version: SILO.version,
                    state: GuestComponentState::Unavailable,
                    detail: Some(reason.to_owned()),
                    at: 0,
                },
            );
        }
    }
    found
}

/// The command an agent's install script ends with to register Silo's MCP server with agents
/// installed since the tools were set up.
pub fn register_after_install_command(kind: Kind) -> String {
    match kind {
        Kind::Linux => scripts::linux_register_after_install(),
        Kind::Macos => scripts::macos_register_after_install(),
    }
}

#[cfg(test)]
mod tests;
