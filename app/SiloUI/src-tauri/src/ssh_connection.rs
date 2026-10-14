//! Prepare a usable client command or explicitly export its isolated key.
use crate::{editor, remote, runtime, ssh_access};
use serde::Deserialize;
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Endpoint {
    port: u16,
    address: String,
}
fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
fn command(path: &Path, endpoint: &Endpoint, user: &str) -> Result<String, String> {
    let user = crate::working_account::response_user(&serde_json::json!({"user":user}))?;
    let address: std::net::Ipv4Addr = endpoint
        .address
        .parse()
        .map_err(|_| "Invalid SSH address.")?;
    if endpoint.port == 0 || address.is_unspecified() {
        return Err("Invalid SSH connection.".into());
    }
    Ok(format!(
        "ssh -i {} -o IdentitiesOnly=yes -p {} {user}@{}",
        quote(path.to_str().ok_or("Invalid key path.")?),
        endpoint.port,
        address
    ))
}

fn select_endpoint(
    endpoint: &mut Endpoint,
    network: Option<bool>,
    is_remote: bool,
    download: bool,
) -> Result<(), String> {
    if network == Some(true) && endpoint.address == "127.0.0.1" && !download {
        return Err("Enable SSH from other devices first.".into());
    }
    if network == Some(false) {
        endpoint.address = "127.0.0.1".into();
    }
    // A controller cannot dial the owner's loopback address.
    if is_remote && !download && endpoint.address == "127.0.0.1" {
        return Err("Enable access from other devices to connect to this remote computer.".into());
    }
    Ok(())
}

/// Where this device keeps its own key for SSH access to one computer on another
/// device. The private key never leaves this device; the owner only receives its
/// public key (C-15).
fn remote_client_root(home: &Path) -> PathBuf {
    home.join("ssh/remote-clients")
}
/// Earlier versions kept a copy of the owner's own private key here instead.
fn legacy_connection_root(home: &Path) -> PathBuf {
    home.join("ssh/connections")
}

/// This device's key for one remote computer, created on first use.
fn remote_client_key(home: &Path, device: &str, computer: &str) -> Result<PathBuf, String> {
    static KEYS: Mutex<()> = Mutex::new(());
    uuid::Uuid::parse_str(device).map_err(|_| "Invalid device identity.")?;
    uuid::Uuid::parse_str(computer).map_err(|_| "Invalid computer identity.")?;
    let _guard = KEYS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let name = format!("{device}-{computer}");
    remove_if_present(&legacy_connection_root(home).join(&name))?;
    let root = remote_client_root(home);
    editor::private_directory(&home.join("ssh"))?;
    editor::private_directory(&root)?;
    let key = root.join(name);
    editor::key(&key)?;
    Ok(key)
}

fn remove_if_present(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            Err("Could not remove an SSH connection key.".into())
        }
        _ => Ok(()),
    }
}

/// Delete every SSH connection key this device holds for computers on `host`, when
/// that device is removed (C-15). Keys for other devices are untouched.
pub(crate) fn forget_device(home: &Path, device: &str) -> Result<(), String> {
    uuid::Uuid::parse_str(device).map_err(|_| "Invalid device identity.")?;
    let prefix = format!("{device}-");
    for root in [remote_client_root(home), legacy_connection_root(home)] {
        let entries = match std::fs::read_dir(&root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err("Could not read SSH connection keys.".into()),
        };
        for entry in entries {
            let entry = entry.map_err(|_| "Could not read SSH connection keys.")?;
            if entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with(&prefix))
            {
                remove_if_present(&entry.path())?;
            }
        }
    }
    Ok(())
}

pub enum ConnectionResult {
    Command(String),
    KeyExport { source: PathBuf, filename: String },
}

pub fn prepare(
    paths: &runtime::RuntimePaths,
    computer: Option<String>,
    device_id: Option<String>,
    computer_id: Option<String>,
    download: bool,
    network: Option<bool>,
) -> Result<ConnectionResult, String> {
    let is_remote = device_id.is_some();
    let (value, key, identity) = if let Some(device) = device_id {
        uuid::Uuid::parse_str(&device).map_err(|_| "Invalid device identity.")?;
        let computer = computer_id.ok_or("Missing computer identity.")?;
        uuid::Uuid::parse_str(&computer).map_err(|_| "Invalid computer identity.")?;
        let key = remote_client_key(&paths.home, &device, &computer)?;
        // The owner authorizes this device's public key under this device's
        // Silo identity, so a replaced key supersedes the previous one.
        let (controller, _) = remote::log_identity()?;
        let value = remote::call_remote(
            &device,
            "ssh.access.connection",
            serde_json::json!({
                "computerId":computer,"accountProtocol":1,"publicKey":editor::public_key(&key)?,"controllerId":controller,
            }),
        )?;
        if value.get("privateKey").is_some() {
            return Err("Update Silo on both devices to connect over SSH.".into());
        }
        (value, key, format!("{device}-{computer}"))
    } else {
        // Exporting a connection reconciles the shared SSH listeners and port
        // allocation across computers; device scope.
        let _guard = runtime::OPERATIONS
            .device("Preparing SSH access")
            .map_err(|e| e.to_string())?;
        runtime::shutdown::ensure_accepting_operations()?;
        let name = computer.ok_or("Missing computer name.")?;
        let metadata =
            runtime::read_metadata(&paths.metadata).map_err(|_| "Could not read computers.")?;
        let computer = metadata
            .computers
            .iter()
            .find(|m| m.name() == name)
            .ok_or("Computer no longer exists.")?;
        let value = ssh_access::connection_material(paths, computer.id())?;
        let private = value["privateKey"]
            .as_str()
            .ok_or("Could not read the connection key.")?;
        let root = legacy_connection_root(&paths.home);
        editor::private_directory(&root)?;
        let local = root.join(computer.id());
        editor::write_private(&local, private.as_bytes())?;
        // Verify the exported bytes form a usable private key before presenting them.
        editor::public_key(&local)?;
        (value, local, computer.id().to_owned())
    };
    let user = crate::working_account::response_user(&value)?;
    let mut endpoint: Endpoint = serde_json::from_value(value)
        .map_err(|_| "Invalid connection response. Update Silo on both devices.")?;
    select_endpoint(&mut endpoint, network, is_remote, download)?;
    if download {
        Ok(ConnectionResult::KeyExport {
            source: key,
            filename: format!("silo-{identity}.key"),
        })
    } else {
        command(&key, &endpoint, user).map(ConnectionResult::Command)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn each_row_uses_its_endpoint_and_remote_loopback_is_not_presented_as_usable() {
        let mut endpoint = Endpoint {
            port: 2222,
            address: "192.168.1.42".into(),
        };
        select_endpoint(&mut endpoint, Some(true), false, false).unwrap();
        assert_eq!(endpoint.address, "192.168.1.42");
        select_endpoint(&mut endpoint, Some(false), false, false).unwrap();
        assert_eq!(endpoint.address, "127.0.0.1");
        assert!(select_endpoint(&mut endpoint, Some(true), false, false).is_err());
        assert!(select_endpoint(&mut endpoint, Some(false), true, false).is_err());
        select_endpoint(&mut endpoint, Some(false), true, true).unwrap();
    }
    #[test]
    fn command_quotes_key_paths_and_uses_explicit_identity() {
        let endpoint = Endpoint {
            port: 2223,
            address: "127.0.0.1".into(),
        };
        assert_eq!(
            command(std::path::Path::new("/a'b $(bad)/key"), &endpoint, "silo").unwrap(),
            "ssh -i '/a'\\''b $(bad)/key' -o IdentitiesOnly=yes -p 2223 silo@127.0.0.1"
        );
        assert!(command(std::path::Path::new("/key"), &endpoint, "silo")
            .unwrap()
            .ends_with("silo@127.0.0.1"));
        assert!(command(std::path::Path::new("/key"), &endpoint, "unknown").is_err());
    }
    #[test]
    fn each_device_keeps_its_own_remote_key_and_drops_the_owners_copy() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let (device, computer) = (
            uuid::Uuid::new_v4().to_string(),
            uuid::Uuid::new_v4().to_string(),
        );
        let legacy = legacy_connection_root(home).join(format!("{device}-{computer}"));
        std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        std::fs::write(&legacy, "owner's private key from an earlier version").unwrap();
        let key = remote_client_key(home, &device, &computer).unwrap();
        assert!(
            !legacy.exists(),
            "a copy of the owner's key must not be kept"
        );
        let public = editor::public_key(&key).unwrap();
        // Stable across connections, so the owner's authorization keeps working.
        assert_eq!(
            editor::public_key(&remote_client_key(home, &device, &computer).unwrap()).unwrap(),
            public
        );
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&key).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(remote_client_key(home, "../escape", &computer).is_err());
    }
    #[test]
    fn removing_a_device_reports_unreadable_key_directories() {
        let device = "00000000-0000-4000-8000-000000000001";
        let missing = tempfile::tempdir().unwrap();
        forget_device(missing.path(), device).unwrap();
        for root in [remote_client_root, legacy_connection_root] {
            let directory = tempfile::tempdir().unwrap();
            std::fs::create_dir(directory.path().join("ssh")).unwrap();
            std::fs::write(root(directory.path()), "not a key directory").unwrap();
            assert_eq!(
                forget_device(directory.path(), device).unwrap_err(),
                "Could not read SSH connection keys."
            );
        }
    }
    #[test]
    fn removing_a_device_deletes_only_its_connection_keys() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let (removed, kept, computer) = (
            uuid::Uuid::new_v4().to_string(),
            uuid::Uuid::new_v4().to_string(),
            uuid::Uuid::new_v4().to_string(),
        );
        let removed_key = remote_client_key(home, &removed, &computer).unwrap();
        let kept_key = remote_client_key(home, &kept, &computer).unwrap();
        let legacy = legacy_connection_root(home).join(format!("{removed}-{computer}"));
        std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        std::fs::write(&legacy, "owner's private key from an earlier version").unwrap();
        let local = legacy_connection_root(home).join(&computer);
        std::fs::write(&local, "this device's own computer key").unwrap();
        forget_device(home, &removed).unwrap();
        assert!(!removed_key.exists() && !legacy.exists());
        assert!(kept_key.exists() && local.exists());
        assert!(forget_device(home, "..").is_err());
    }
}
