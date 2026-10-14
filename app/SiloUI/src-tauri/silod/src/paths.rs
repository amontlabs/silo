use std::{
    env, fs, io,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
};

use silo_engine::{
    channel::Channel,
    context::{EngineIdentity, EnginePaths, PackageKind},
};

#[derive(Clone, Debug)]
pub struct SilodPaths {
    pub channel: Channel,
    pub engine: EnginePaths,
    pub state_dir: PathBuf,
}

impl SilodPaths {
    pub fn resolve(channel: Channel, resources: PathBuf) -> Result<Self, String> {
        let home = env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| "The user's home directory is unavailable".to_owned())?;
        Ok(Self::for_home(channel, home, resources))
    }

    pub fn for_home(channel: Channel, home: PathBuf, resources: PathBuf) -> Self {
        let support = home
            .join("Library/Application Support")
            .join(channel.identifier());
        let engine = EnginePaths {
            app_data: support.clone(),
            app_config: support,
            app_cache: home.join("Library/Caches").join(channel.identifier()),
            app_log: home.join("Library/Logs").join(channel.identifier()),
            resources,
            home: home.clone(),
        };
        Self {
            channel,
            engine,
            state_dir: channel.state_dir(&home),
        }
    }

    pub fn socket_path(&self) -> PathBuf {
        self.state_dir.join("engine.sock")
    }

    pub fn lock_path(&self) -> PathBuf {
        self.state_dir.join("engine.lock")
    }

    pub fn owner_path(&self) -> PathBuf {
        self.state_dir.join("engine-owner.json")
    }

    pub fn identity(&self, version: &str) -> EngineIdentity {
        EngineIdentity {
            channel: self.channel,
            version: version.to_owned(),
            package_kind: PackageKind::MacApp,
        }
    }
}

pub fn default_resource_dir(executable: &Path) -> Result<PathBuf, String> {
    let macos_dir = executable
        .parent()
        .ok_or_else(|| "silod executable has no parent directory".to_owned())?;
    if macos_dir.file_name().is_some_and(|name| name == "MacOS")
        && macos_dir
            .parent()
            .is_some_and(|contents| contents.file_name().is_some_and(|name| name == "Contents"))
    {
        Ok(macos_dir
            .join("../Resources")
            .canonicalize()
            .unwrap_or_else(|_| macos_dir.join("../Resources")))
    } else {
        Ok(macos_dir.to_path_buf())
    }
}

pub fn ensure_private_directory(path: &Path, euid: u32) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.file_type().is_dir() || metadata.uid() != euid {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("{} is not an owned directory", path.display()),
                ));
            }
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(path)?;
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        }
        Err(error) => return Err(error),
    }
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_dir()
        || metadata.uid() != euid
        || metadata.permissions().mode() & 0o777 != 0o700
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{} must be owned by euid {euid} with mode 0700",
                path.display()
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_paths_are_isolated_and_use_the_engine_socket_name() {
        let home = PathBuf::from("/Users/tester");
        let production = SilodPaths::for_home(Channel::Production, home.clone(), PathBuf::new());
        let development = SilodPaths::for_home(Channel::Development, home, PathBuf::new());

        assert_eq!(
            production.socket_path(),
            Path::new("/Users/tester/.silo/engine.sock")
        );
        assert_eq!(
            development.socket_path(),
            Path::new("/Users/tester/.silo-dev/engine.sock")
        );
        assert_ne!(production.engine.app_data, development.engine.app_data);
        assert_eq!(
            production.engine.app_data,
            Path::new("/Users/tester/Library/Application Support/org.silo.preview")
        );
        assert_eq!(
            development.engine.app_data,
            Path::new("/Users/tester/Library/Application Support/org.silo.dev")
        );
    }

    #[test]
    fn private_directory_rejects_a_symlink() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("target");
        fs::create_dir(&target).unwrap();
        let alias = root.path().join("alias");
        std::os::unix::fs::symlink(&target, &alias).unwrap();
        let euid = unsafe { libc::geteuid() };

        assert_eq!(
            ensure_private_directory(&alias, euid).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
    }
}
