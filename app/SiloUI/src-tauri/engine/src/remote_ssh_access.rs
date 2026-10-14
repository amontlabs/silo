//! The controller only exchanges settings. Listeners belong to the remote Silo
//! application's ssh_access owner, independent of this controller's connection.
use crate::bridge_error::BridgeError;
use crate::{remote, ssh_access::Settings};
use serde_json::{json, Value};

fn project(mut value: Value, device: &str) -> Result<Value, String> {
    uuid::Uuid::parse_str(device).map_err(|_| "Invalid device identity.")?;
    let rows = value["computers"]
        .as_array_mut()
        .ok_or("Invalid remote SSH state.")?;
    let mut seen = std::collections::HashSet::new();
    for row in rows {
        let id = row["computerId"]
            .as_str()
            .ok_or("Missing remote computer identity. Update Silo on both devices.")?
            .to_owned();
        let id = id.as_str();
        uuid::Uuid::parse_str(id).map_err(|_| "Invalid remote computer identity.")?;
        if !seen.insert(id.to_owned()) {
            return Err("Duplicate remote computer identity.".into());
        }
        // The owner's own name, kept so the canonical API can show it; the target string
        // below is the historical Tauri reference, never an engine input.
        row["computerName"] = row["computer"].clone();
        row["computer"] = json!(format!("silo-remote:{device}:{id}"));
    }
    Ok(value)
}
fn request(device: &str, method: &str, params: Value) -> Result<Value, BridgeError> {
    uuid::Uuid::parse_str(device).map_err(|_| "Invalid device identity.")?;
    project(remote::call_remote_typed(device, method, params)?, device).map_err(BridgeError::from)
}
pub fn remote_ssh_access_state(device_id: &str) -> Result<Value, BridgeError> {
    request(device_id, "ssh.access.state", json!({}))
}
pub fn remote_save_ssh_access(
    device_id: &str,
    computer_id: String,
    enabled: bool,
    port: u16,
    bind_address: String,
    keys: Option<Vec<String>>,
) -> Result<Value, BridgeError> {
    uuid::Uuid::parse_str(&computer_id).map_err(|_| "Invalid computer identity.")?;
    let settings = Settings {
        enabled,
        port,
        bind_address,
        keys,
    };
    request(
        device_id,
        "ssh.access.save",
        json!({"computerId":computer_id,"settings":settings}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn projection_uses_owner_identity_and_preserves_owner_addresses_and_name() {
        let device = uuid::Uuid::new_v4().to_string();
        let computer = uuid::Uuid::new_v4().to_string();
        let value = json!({"computers":[{"computerId":computer,"computer":"dev","deviceName":"Device B","bindAddress":"192.168.5.8","addresses":["192.168.5.8"],"state":"listening"}]});
        let projected = project(value.clone(), &device).unwrap();
        assert_eq!(
            projected["computers"][0]["computer"],
            format!("silo-remote:{device}:{computer}")
        );
        for field in ["deviceName", "bindAddress", "addresses", "state"] {
            assert_eq!(
                projected["computers"][0][field],
                value["computers"][0][field]
            );
        }
        let other = uuid::Uuid::new_v4().to_string();
        assert_ne!(
            project(value, &other).unwrap()["computers"][0]["computer"],
            projected["computers"][0]["computer"]
        );
    }
    #[test]
    fn missing_invalid_or_duplicate_remote_identity_never_falls_back_to_local_name() {
        let device = uuid::Uuid::new_v4().to_string();
        for value in [
            json!({}),
            json!({"computers":[{"computer":"dev"}]}),
            json!({"computers":[{"computerId":"dev","computer":"dev"}]}),
        ] {
            assert!(project(value, &device).is_err());
        }
        let id = uuid::Uuid::new_v4().to_string();
        assert!(project(
            json!({"computers":[{"computerId":id},{"computerId":id}]}),
            &device
        )
        .is_err());
    }
}
