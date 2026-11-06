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
pub(super) async fn merge(
    context: &HandlerContext,
    params: &Value,
    mut local: ChangesRequestsListResult,
) -> ChangesRequestsListResult {
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
            let owner_params = owner_params.clone();
            let task = tokio::task::spawn_blocking(move || {
                call_owner(&id, "changes.requests.list", owner_params)
            });
            (peer, task)
        })
        .collect();
    for (peer, task) in asked {
        let parsed = match task.await {
            Ok(Ok(value)) => serde_json::from_value::<ChangesRequestsListResult>(value)
                .map_err(|_| "It sent a reply this Silo cannot read.".to_owned()),
            Ok(Err(error)) => Err(error.to_string()),
            Err(_) => Err("The request could not finish.".to_owned()),
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
