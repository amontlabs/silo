//! `changes.requests.list` with `allDevices`: this device's requests and every connected
//! device's, each row tagged with its device. A device that cannot answer is reported in
//! `unreachable`; it never fails the list.

use serde_json::Value;
use silo_api::methods::review_requests::{
    ChangesRequestsListResult, RequestEntry, UnreachableDevice,
};

use super::HandlerContext;
use crate::methods::access::{call_owner, without_device};

/// One connected device: its id and name.
type Peer = (String, String);

/// How long one device may take to answer before the list goes on without it.
pub(crate) const PEER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

#[cfg(test)]
pub(crate) static TEST_TIMEOUT: std::sync::Mutex<Option<std::time::Duration>> =
    std::sync::Mutex::new(None);

fn timeout() -> std::time::Duration {
    #[cfg(test)]
    if let Some(timeout) = *TEST_TIMEOUT.lock().unwrap() {
        return timeout;
    }
    PEER_TIMEOUT
}

/// Why a device is known to be offline: its last health probe failed and it is not just busy.
fn known_offline(id: &str) -> Option<String> {
    #[cfg(test)]
    if TEST_PEERS.lock().unwrap().is_some() {
        return None;
    }
    let health = silo_engine::remote::device_health().ok()?;
    health
        .devices
        .into_iter()
        .find(|device| device.device_id == id)
        .filter(|device| !device.connected && !device.busy)
        .and_then(|device| device.error)
}

#[cfg(test)]
pub(crate) static TEST_PEERS: std::sync::Mutex<Option<Vec<Peer>>> = std::sync::Mutex::new(None);

async fn peers() -> Result<Vec<Peer>, String> {
    #[cfg(test)]
    if let Some(peers) = TEST_PEERS.lock().unwrap().clone() {
        return Ok(peers);
    }
    let devices = silo_engine::remote::device_list().await?;
    Ok(devices.into_iter().map(|d| (d.id, d.name)).collect())
}

fn tag(rows: &mut [RequestEntry], device: &Peer) {
    for row in rows {
        row.device_id = Some(device.0.clone());
        row.device_name = Some(device.1.clone());
    }
}

/// Merges `local` with the list each connected device returns for `params`.
pub(crate) async fn merge(
    context: &HandlerContext,
    params: &Value,
    mut local: ChangesRequestsListResult,
) -> ChangesRequestsListResult {
    // An owner answers for itself: a request that came over the bridge never asks further.
    if context.is_owner_bridge() {
        return local;
    }
    let identity = context.silod_context().local_device();
    tag(
        &mut local.requests,
        &(identity.device_id.clone(), identity.name.clone()),
    );
    let mut owner_params = without_device(params);
    if let Some(object) = owner_params.as_object_mut() {
        object.remove("allDevices");
    }
    let Ok(peers) = peers().await else {
        return local;
    };
    // Every device is asked at once, so a slow one does not hold up the rest.
    let asked: Vec<_> = peers
        .into_iter()
        .map(|peer| {
            let id = peer.0.clone();
            let offline = known_offline(&id);
            let owner_params = owner_params.clone();
            let task = offline.is_none().then(|| {
                tokio::task::spawn_blocking(move || {
                    call_owner(&id, "changes.requests.list", owner_params)
                })
            });
            (peer, offline, task)
        })
        .collect();
    for (peer, offline, task) in asked {
        let Some(task) = task else {
            local.unreachable.push(UnreachableDevice {
                device_id: peer.0,
                device_name: peer.1,
                reason: offline.unwrap_or_default(),
            });
            continue;
        };
        let parsed = match tokio::time::timeout(timeout(), task).await {
            Err(_) => Err("It did not answer in time.".to_owned()),
            Ok(Ok(Ok(value))) => serde_json::from_value::<ChangesRequestsListResult>(value)
                .map_err(|_| "It sent a reply this Silo cannot read.".to_owned()),
            Ok(Ok(Err(error))) => Err(error.to_string()),
            Ok(Err(_)) => Err("The request could not finish.".to_owned()),
        };
        match parsed {
            Ok(mut remote) => {
                tag(&mut remote.requests, &peer);
                local.requests.extend(remote.requests);
            }
            Err(reason) => local.unreachable.push(UnreachableDevice {
                device_id: peer.0,
                device_name: peer.1,
                reason,
            }),
        }
    }
    local
}
