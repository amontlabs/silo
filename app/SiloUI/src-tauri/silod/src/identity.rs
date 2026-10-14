use std::{
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::Path,
};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::paths::ensure_private_directory;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalDeviceIdentity {
    pub device_id: String,
    pub name: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectionsConfig {
    device_id: String,
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    devices: Vec<serde_json::Value>,
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
}

pub fn load_or_create(state_dir: &Path, euid: u32) -> Result<LocalDeviceIdentity, String> {
    let directory = state_dir.join("desktop-remote");
    ensure_private_directory(&directory, euid).map_err(|error| error.to_string())?;
    let config_path = directory.join("config.json");
    let config = match read_config(&config_path, euid) {
        Ok(Some(config)) => config,
        Ok(None) => create_config(&config_path, euid)?,
        Err(error) => return Err(error.to_string()),
    };
    Ok(LocalDeviceIdentity {
        device_id: config.device_id,
        name: host_name(),
    })
}

fn read_config(path: &Path, euid: u32) -> io::Result<Option<ConnectionsConfig>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if !metadata.file_type().is_file()
        || metadata.uid() != euid
        || metadata.permissions().mode() & 0o777 != 0o600
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{} must be an euid-owned regular file with mode 0600",
                path.display()
            ),
        ));
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    let mut bytes = Vec::new();
    file.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Connections settings exceed 1 MiB",
        ));
    }
    let config: ConnectionsConfig = serde_json::from_slice(&bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if config.device_id.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "deviceId is empty",
        ));
    }
    Ok(Some(config))
}

fn create_config(path: &Path, euid: u32) -> Result<ConnectionsConfig, String> {
    let config = ConnectionsConfig {
        device_id: Uuid::new_v4().to_string(),
        enabled: false,
        devices: Vec::new(),
        extra: serde_json::Map::new(),
    };
    let mut bytes = serde_json::to_vec_pretty(&config).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    let temporary = path.with_file_name(format!("config.json.{}.tmp", Uuid::new_v4()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&temporary)
        .map_err(|error| error.to_string())?;
    file.write_all(&bytes).map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    drop(file);

    match fs::hard_link(&temporary, path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let _ = fs::remove_file(&temporary);
            return read_config(path, euid)
                .map_err(|read_error| read_error.to_string())?
                .ok_or_else(|| {
                    "Connections settings disappeared during initialization".to_owned()
                });
        }
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            return Err(error.to_string());
        }
    }
    fs::remove_file(&temporary).map_err(|error| error.to_string())?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|error| error.to_string())?;
    if unsafe { libc::geteuid() } != euid {
        return Err("effective uid changed while creating Connections identity".to_owned());
    }
    Ok(config)
}

fn host_name() -> String {
    let mut bytes = [0_u8; 256];
    let status = unsafe { libc::gethostname(bytes.as_mut_ptr().cast(), bytes.len()) };
    if status != 0 {
        return "Mac".to_owned();
    }
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    let name = String::from_utf8_lossy(&bytes[..end]).trim().to_owned();
    if name.is_empty() {
        "Mac".to_owned()
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn creates_and_reuses_the_channel_connections_identity() {
        let root = tempfile::tempdir().unwrap();
        let euid = unsafe { libc::geteuid() };
        let state = root.path().join(".silo");
        fs::create_dir(&state).unwrap();
        fs::set_permissions(&state, fs::Permissions::from_mode(0o700)).unwrap();

        let first = load_or_create(&state, euid).unwrap();
        let second = load_or_create(&state, euid).unwrap();

        assert!(!first.device_id.is_empty());
        assert_eq!(first.device_id, second.device_id);
    }
}
