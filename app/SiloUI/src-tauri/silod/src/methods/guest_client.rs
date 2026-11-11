//! `guestClient.enable`, `.revoke` and `.status`: Silo's tools in a computer
//! (`docs/SiloUI-IN-GUEST-CLIENT.md`). Enable opens the computer's channel on a loopback port,
//! makes sure its network will allow that port, and writes the endpoint file into the computer
//! when it is running (otherwise at its next start). No answer carries the token.

use serde_json::Value;
use silo_api::methods::{
    guest_client::{
        GuestClientComputerParams, GuestClientStatus, GUEST_ENDPOINT_HOST, GUEST_ENDPOINT_PATH,
    },
    network::NetworkPolicyTiming,
    MethodName,
};
use silo_engine::{
    context::EngineContext,
    guest_client::{self, endpoint, TokenStore},
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
    let params: GuestClientComputerParams = match context.decode_params(params) {
        Ok(params) => params,
        Err(error) => return Some(Err(error)),
    };
    let engine = context.silod_context();
    let app_data = engine.paths().app_data.clone();
    let runtime = silo_engine::runtime::runtime_paths(engine.runtime_host().as_ref());
    let id = params.computer_id;
    let result = tokio::task::spawn_blocking(move || {
        let store = TokenStore::new(&app_data);
        match method {
            MethodName::GuestClientEnable => enable(&store, &runtime, &app_data, &id),
            MethodName::GuestClientRevoke => {
                guest_client::turn_off_in(&store, &[id.as_str()])?;
                status(&store, &runtime, &id, false)
            }
            _ => status(&store, &runtime, &id, false),
        }
    })
    .await
    .map_err(|_| DispatchError::internal("Silo's tools could not be changed."))
    .and_then(|result| result.map_err(DispatchError::internal));
    Some(result.and_then(output))
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
    Ok(GuestClientStatus {
        computer_id: computer_id.to_owned(),
        enabled: record.is_some(),
        host: record.as_ref().map(|_| GUEST_ENDPOINT_HOST.to_owned()),
        port: record.as_ref().and_then(|record| record.port),
        endpoint_path: record.as_ref().map(|_| GUEST_ENDPOINT_PATH.to_owned()),
        enabled_at: record.map(|record| record.minted_at),
        network,
        written,
    })
}

fn enable(
    store: &TokenStore,
    runtime: &Result<silo_engine::runtime::RuntimePaths, String>,
    app_data: &std::path::Path,
    computer_id: &str,
) -> Result<GuestClientStatus, String> {
    // The computer must exist on this device before anything is opened for it.
    let located = locate(runtime, computer_id)?;
    guest_client::turn_on_in(store, computer_id)?;
    let mut written = false;
    if located.running {
        let target = silo_engine::guest::resolve(runtime.clone(), app_data, computer_id)
            .map_err(|error| error.message)?;
        endpoint::write(&target, store).map_err(|error| {
            format!("Silo's tools are on, but {error} The file is written again when the computer next starts.")
        })?;
        written = true;
    }
    status(store, runtime, computer_id, written)
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
