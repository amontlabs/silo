//! `guestClient.enable`, `.revoke` and `.status`: Silo's tools in a computer
//! (`docs/SiloUI-IN-GUEST-CLIENT.md`). Enable opens the computer's channel on a loopback port,
//! makes sure its network will allow that port, and writes the endpoint file into the computer
//! when it is running (otherwise at its next start). No answer carries the token.

use std::path::{Path, PathBuf};

use serde_json::Value;
use silo_api::methods::{
    guest_client::{
        GuestClientComputerParams, GuestClientEnableParams, GuestClientStatus, GUEST_ENDPOINT_HOST,
        GUEST_ENDPOINT_PATH,
    },
    network::NetworkPolicyTiming,
    MethodName,
};
use silo_engine::{
    context::EngineContext,
    guest_client::{self, endpoint, TokenStore},
    guest_components::{self, receipt::State, Sources},
    policy::Caller,
};

use crate::dispatcher::{DispatchError, HandlerOutput};

use super::{ownership::owns_prefix, HandlerContext};

pub(super) fn owns(method: MethodName) -> bool {
    owns_prefix(method, &["guestClient."])
}

pub(super) async fn handle(
    method: MethodName,
    params: &Value,
    context: &HandlerContext,
) -> Option<Result<HandlerOutput, DispatchError>> {
    if !owns(method) {
        return None;
    }
    let (id, binary_path) = if method == MethodName::GuestClientEnable {
        match context.decode_params::<GuestClientEnableParams>(params) {
            Ok(params) => (params.computer_id, params.binary_path),
            Err(error) => return Some(Err(error)),
        }
    } else {
        match context.decode_params::<GuestClientComputerParams>(params) {
            Ok(params) => (params.computer_id, None),
            Err(error) => return Some(Err(error)),
        }
    };
    // A program named by path is read from this device's disk, so only the owner at this device
    // may name one: another device's request would put a file of this device into a computer.
    if binary_path.is_some()
        && (context.is_owner_bridge() || !matches!(context.caller(), Caller::Owner { .. }))
    {
        return Some(Err(DispatchError::new(
            silo_api::jobs::ErrorCode::PermissionDenied,
            "A program to send into the computer can only be named by Silo on this device.",
        )));
    }
    let engine = context.silod_context();
    let app_data = engine.paths().app_data.clone();
    let runtime = silo_engine::runtime::runtime_paths(engine.runtime_host().as_ref());
    let places = Places::of(engine, &runtime);
    let result = tokio::task::spawn_blocking(move || {
        let store = TokenStore::new(&app_data);
        match method {
            MethodName::GuestClientEnable => enable(
                &store,
                &runtime,
                &app_data,
                &places,
                &id,
                binary_path.as_deref().map(Path::new),
            ),
            MethodName::GuestClientRevoke => {
                guest_client::turn_off_in(&store, &[id.as_str()])?;
                remove_registration(&runtime, &app_data, &places, &id);
                status(&store, &runtime, &app_data, &places, &id, false)
            }
            _ => status(&store, &runtime, &app_data, &places, &id, false),
        }
    })
    .await
    .map_err(|_| DispatchError::internal("Silo's tools could not be changed."))
    .and_then(|result| result.map_err(DispatchError::internal));
    Some(result.and_then(output))
}

/// Where the programs Silo puts into computers are on this device.
pub(crate) struct Places {
    resources: PathBuf,
    tools_directory: Option<PathBuf>,
}

impl Places {
    pub(crate) fn of(
        engine: &crate::context::SilodContext,
        runtime: &Result<silo_engine::runtime::RuntimePaths, String>,
    ) -> Self {
        Self {
            resources: engine.paths().resources.clone(),
            tools_directory: runtime
                .as_ref()
                .ok()
                .and_then(|paths| paths.executable.parent().map(Path::to_path_buf)),
        }
    }

    pub(crate) fn sources(&self) -> Sources<'_> {
        Sources {
            resources: &self.resources,
            tools_directory: self.tools_directory.as_deref(),
        }
    }
}

fn output(status: GuestClientStatus) -> Result<HandlerOutput, DispatchError> {
    serde_json::to_value(status)
        .map(|value| HandlerOutput {
            value,
            event_barrier: None,
        })
        .map_err(|error| DispatchError::internal(error.to_string()))
}

/// A computer on this device: whether it runs, and its network policy target.
struct Located {
    running: bool,
    macos_name: Option<String>,
}

fn locate(
    runtime: &Result<silo_engine::runtime::RuntimePaths, String>,
    computer_id: &str,
) -> Result<Located, String> {
    if crate::methods::access::is_macos_computer(computer_id) {
        let (name, stopped, _) = silo_engine::macos_computers::promote_view(computer_id)
            .ok_or("That computer was not found.")?;
        return Ok(Located {
            running: !stopped,
            macos_name: Some(name),
        });
    }
    let paths = runtime.as_ref().map_err(Clone::clone)?;
    let view = silo_engine::runtime::checkpoints::network_policy_view(paths, computer_id)
        .map_err(|_| "That computer was not found.".to_owned())?;
    Ok(Located {
        running: view.running && !view.pending,
        macos_name: None,
    })
}

/// When the computer's network takes the rules its plan asks for now: `Applied` when it runs
/// with them.
pub(crate) fn network_now(
    runtime: &Result<silo_engine::runtime::RuntimePaths, String>,
    computer_id: &str,
) -> Result<NetworkPolicyTiming, String> {
    let located = locate(runtime, computer_id)?;
    network_timing(runtime, computer_id, &located)
        .ok_or_else(|| "the computer's network could not be read.".to_owned())
}

/// When the computer's network lets it reach its channel.
fn network_timing(
    runtime: &Result<silo_engine::runtime::RuntimePaths, String>,
    computer_id: &str,
    located: &Located,
) -> Option<NetworkPolicyTiming> {
    use silo_engine::network_policy::service::{self, Target};
    let target = match &located.macos_name {
        Some(name) => Target::Macos {
            name: name.clone(),
            running: located.running,
        },
        None => Target::Linux(runtime.as_ref().ok()?),
    };
    service::get(computer_id, target)
        .ok()
        .map(|state| state.timing)
}

fn status(
    store: &TokenStore,
    runtime: &Result<silo_engine::runtime::RuntimePaths, String>,
    app_data: &Path,
    places: &Places,
    computer_id: &str,
    written: bool,
) -> Result<GuestClientStatus, String> {
    let record = store.status(computer_id)?;
    let network = match &record {
        Some(_) => locate(runtime, computer_id)
            .ok()
            .and_then(|located| network_timing(runtime, computer_id, &located)),
        None => None,
    };
    let components = guest_components::status(
        app_data,
        computer_id,
        crate::methods::access::is_macos_computer(computer_id),
        &places.sources(),
        record.is_some(),
    );
    Ok(GuestClientStatus {
        computer_id: computer_id.to_owned(),
        enabled: record.is_some(),
        host: record.as_ref().map(|_| GUEST_ENDPOINT_HOST.to_owned()),
        port: record.as_ref().and_then(|record| record.port),
        endpoint_path: record.as_ref().map(|_| GUEST_ENDPOINT_PATH.to_owned()),
        restart_needed: record.as_ref().is_some_and(|record| record.restart_needed),
        enabled_at: record.map(|record| record.minted_at),
        network,
        written,
        components,
    })
}

/// Takes Silo's MCP server out of the agents of a running computer whose tools were just turned
/// off. A computer that is not running, or does not answer, keeps the removal owed until it next
/// runs (`guest_client::turn_off_in` recorded it).
fn remove_registration(
    runtime: &Result<silo_engine::runtime::RuntimePaths, String>,
    app_data: &Path,
    places: &Places,
    computer_id: &str,
) {
    let running = locate(runtime, computer_id).is_ok_and(|located| located.running);
    if !running {
        return;
    }
    let outcome = silo_engine::guest::resolve(runtime.clone(), app_data, computer_id)
        .map_err(|error| error.message)
        .and_then(|target| {
            guest_components::remove_registration(&target, app_data, &places.sources())
        });
    if let Err(error) = outcome {
        eprintln!("silod: tools of {computer_id}: the registration is taken out at its next start: {error}");
    }
}

fn enable(
    store: &TokenStore,
    runtime: &Result<silo_engine::runtime::RuntimePaths, String>,
    app_data: &Path,
    places: &Places,
    computer_id: &str,
    binary: Option<&Path>,
) -> Result<GuestClientStatus, String> {
    // The computer must exist on this device before anything is opened for it.
    let located = locate(runtime, computer_id)?;
    // A program named for development is read before anything changes.
    let payload = match binary {
        Some(path) if located.macos_name.is_some() => {
            return Err(format!(
                "{} cannot be sent: a macOS computer uses the device's own silo.",
                path.display()
            ));
        }
        Some(path) => Some(
            guest_components::artifact::override_file(path)
                .map_err(|error| format!("The program could not be read: {error}"))?,
        ),
        None => None,
    };
    if payload.is_some() && !located.running {
        return Err("Start the computer first; the program is copied into it.".into());
    }
    let record = guest_client::turn_on_in(store, computer_id)?;
    guest_components::standing::start_over(computer_id);
    let mut written = false;
    // A computer waiting for a restart gets its file once it runs with rules that match.
    if located.running && !record.restart_needed {
        let target = silo_engine::guest::resolve(runtime.clone(), app_data, computer_id)
            .map_err(|error| error.message)?;
        endpoint::write(&target, store).map_err(|error| {
            format!("Silo's tools are on, but {error} The file is written again when the computer next starts.")
        })?;
        written = true;
        match guest_components::reconcile(&target, app_data, &places.sources(), payload.as_ref()) {
            Ok(receipt) => {
                // A program the owner named must arrive; for what Silo sends itself a failure is
                // in the status and the next start tries again.
                let failed = receipt
                    .entry(guest_components::SILO.id)
                    .filter(|entry| payload.is_some() && entry.state == State::Failed);
                if let Some(entry) = failed {
                    return Err(entry
                        .detail
                        .clone()
                        .unwrap_or_else(|| "The program could not be put in place.".into()));
                }
            }
            Err(error) if payload.is_some() => return Err(error),
            Err(error) => {
                eprintln!("silod: tools of {computer_id}: components not reconciled yet: {error}")
            }
        }
    }
    status(store, runtime, app_data, places, computer_id, written)
}

/// Turns off Silo's tools in computers whose standing changed: a promote or undo swaps which
/// computer is main, so the agent that had them must be given them again on purpose. A failure
/// fails the operation that asked for it.
pub(super) fn revoke_for(
    context: &HandlerContext,
    computers: &[&str],
) -> Result<(), DispatchError> {
    let store = TokenStore::new(&context.silod_context().paths().app_data);
    guest_client::turn_off_in(&store, computers)
        .map(|_| ())
        .map_err(|error| {
            DispatchError::internal(format!(
                "Silo's tools could not be turned off, so nothing was changed: {error}"
            ))
        })
}

/// Every computer the promotes in force tie to `id`, and `id` itself: an undo swaps two.
pub(super) fn swap_participants(context: &HandlerContext, id: &str) -> Vec<String> {
    let journal = silo_engine::changes::promote::service::journal_for(context.engine_context());
    let mut found = vec![id.to_owned()];
    for record in journal.live_for(id) {
        for other in [record.swap.fork_id, record.swap.target_id] {
            if !found.contains(&other) {
                found.push(other);
            }
        }
    }
    found
}

#[cfg(test)]
mod tests;
