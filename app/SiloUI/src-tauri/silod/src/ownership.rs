use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use semver::Version;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::paths::{ensure_private_directory, SilodPaths};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineOwnerRecord {
    pub channel_identifier: String,
    pub engine_version: String,
    pub engine_build: String,
    pub pid: u32,
    pub euid: u32,
    pub started_at_unix_ms: u128,
}

pub struct OwnerLease {
    lock_file: File,
    owner_path: PathBuf,
    record: EngineOwnerRecord,
}

pub enum OwnerAcquisition {
    Acquired(OwnerLease),
    NotOwner(String),
}

impl OwnerLease {
    pub fn acquire(
        paths: &SilodPaths,
        engine_version: &str,
        engine_build: &str,
        euid: u32,
    ) -> Result<OwnerAcquisition, String> {
        ensure_private_directory(&paths.state_dir, euid).map_err(|error| error.to_string())?;
        let lock_path = paths.lock_path();
        let lock_file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
            .open(&lock_path)
            .map_err(|error| format!("Cannot open engine lock {}: {error}", lock_path.display()))?;
        verify_private_file(&lock_path, euid, 0o600)?;
        let lock_result = unsafe {
            libc::flock(
                std::os::fd::AsRawFd::as_raw_fd(&lock_file),
                libc::LOCK_EX | libc::LOCK_NB,
            )
        };
        if lock_result != 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::EWOULDBLOCK) {
                let running = read_owner_record(&paths.owner_path(), euid)
                    .ok()
                    .flatten()
                    .map(|owner| {
                        format!(
                            "; current owner version {} build {}",
                            owner.engine_version, owner.engine_build
                        )
                    })
                    .unwrap_or_default();
                return Ok(OwnerAcquisition::NotOwner(format!(
                    "An engine already owns this user/channel{running}"
                )));
            }
            return Err(format!("Cannot lock engine owner file: {error}"));
        }

        let owner_path = paths.owner_path();
        if let Some(previous) =
            read_owner_record(&owner_path, euid).map_err(|error| error.to_string())?
        {
            if previous.channel_identifier != paths.channel.identifier() {
                return Err("Engine owner record channel does not match this bundle".to_owned());
            }
            if let Err(reason) = compare_candidate(engine_version, engine_build, &previous) {
                return Ok(OwnerAcquisition::NotOwner(reason));
            }
        }

        let record = EngineOwnerRecord {
            channel_identifier: paths.channel.identifier().to_owned(),
            engine_version: engine_version.to_owned(),
            engine_build: engine_build.to_owned(),
            pid: std::process::id(),
            euid,
            started_at_unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
        };
        write_owner_record(&owner_path, euid, &record).map_err(|error| error.to_string())?;
        Ok(OwnerAcquisition::Acquired(Self {
            lock_file,
            owner_path,
            record,
        }))
    }

    pub fn record(&self) -> &EngineOwnerRecord {
        &self.record
    }
}

impl Drop for OwnerLease {
    fn drop(&mut self) {
        if let Ok(Some(record)) = read_owner_record(&self.owner_path, self.record.euid) {
            if record.pid == self.record.pid
                && record.started_at_unix_ms == self.record.started_at_unix_ms
                && record.engine_build == self.record.engine_build
            {
                let _ = fs::remove_file(&self.owner_path);
            }
        }
        let _ = unsafe {
            libc::flock(
                std::os::fd::AsRawFd::as_raw_fd(&self.lock_file),
                libc::LOCK_UN,
            )
        };
    }
}

fn compare_candidate(version: &str, build: &str, owner: &EngineOwnerRecord) -> Result<(), String> {
    let candidate = Version::parse(version).map_err(|error| error.to_string())?;
    let installed = Version::parse(&owner.engine_version).map_err(|error| error.to_string())?;
    match candidate.cmp_precedence(&installed) {
        std::cmp::Ordering::Less => Err(format!(
            "A newer engine version {} build {} owns this channel; refusing {} build {}",
            owner.engine_version, owner.engine_build, version, build
        )),
        std::cmp::Ordering::Equal if owner.engine_build != build => Err(format!(
            "Conflicting engine builds share version {version}: installed {}, candidate {build}",
            owner.engine_build
        )),
        _ => Ok(()),
    }
}

fn read_owner_record(path: &Path, euid: u32) -> io::Result<Option<EngineOwnerRecord>> {
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
    file.take(64 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 64 * 1024 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "engine owner record is too large",
        ));
    }
    let record = serde_json::from_slice(&bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    Ok(Some(record))
}

fn write_owner_record(path: &Path, euid: u32, record: &EngineOwnerRecord) -> io::Result<()> {
    if path.exists() {
        verify_private_file(path, euid, 0o600)
            .map_err(|message| io::Error::new(io::ErrorKind::PermissionDenied, message))?;
    }
    let temporary = path.with_file_name(format!("engine-owner.{}.tmp", Uuid::new_v4()));
    let bytes = serde_json::to_vec(record)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&temporary)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temporary, path)?;
    fs::File::open(path.parent().unwrap_or_else(|| Path::new(".")))?.sync_all()?;
    Ok(())
}

fn verify_private_file(path: &Path, euid: u32, mode: u32) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !metadata.file_type().is_file()
        || metadata.uid() != euid
        || metadata.permissions().mode() & 0o777 != mode
    {
        return Err(format!(
            "{} must be an euid-owned regular file with mode {:04o}",
            path.display(),
            mode
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use silo_engine::channel::Channel;

    use crate::paths::SilodPaths;

    fn paths(home: &Path, channel: Channel) -> SilodPaths {
        SilodPaths::for_home(channel, home.to_path_buf(), PathBuf::new())
    }

    #[test]
    fn single_owner_is_enforced_per_channel_and_newer_stale_owner_blocks_downgrade() {
        let root = tempfile::tempdir().unwrap();
        let euid = unsafe { libc::geteuid() };
        let development = paths(root.path(), Channel::Development);
        let production = paths(root.path(), Channel::Production);
        fs::create_dir(&development.state_dir).unwrap();
        fs::create_dir(&production.state_dir).unwrap();
        fs::set_permissions(&development.state_dir, fs::Permissions::from_mode(0o700)).unwrap();
        fs::set_permissions(&production.state_dir, fs::Permissions::from_mode(0o700)).unwrap();

        let OwnerAcquisition::Acquired(owner) =
            OwnerLease::acquire(&development, "1.2.0", "build-1", euid).unwrap()
        else {
            panic!("first engine should acquire ownership");
        };
        assert!(matches!(
            OwnerLease::acquire(&development, "1.3.0", "build-2", euid).unwrap(),
            OwnerAcquisition::NotOwner(_)
        ));
        let OwnerAcquisition::Acquired(production_owner) =
            OwnerLease::acquire(&production, "1.0.0", "prod", euid).unwrap()
        else {
            panic!("other channel should acquire separate ownership");
        };
        drop(owner);
        drop(production_owner);

        let owner_path = development.owner_path();
        let stale = EngineOwnerRecord {
            channel_identifier: Channel::Development.identifier().to_owned(),
            engine_version: "2.0.0".to_owned(),
            engine_build: "newer".to_owned(),
            pid: 1,
            euid,
            started_at_unix_ms: 1,
        };
        write_owner_record(&owner_path, euid, &stale).unwrap();
        assert!(matches!(
            OwnerLease::acquire(&development, "1.9.0", "older", euid).unwrap(),
            OwnerAcquisition::NotOwner(_)
        ));
    }
}
