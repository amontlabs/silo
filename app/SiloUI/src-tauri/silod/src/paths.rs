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

    #[cfg(target_os = "linux")]
    pub fn for_home(channel: Channel, home: PathBuf, resources: PathBuf) -> Self {
        Self::for_home_with(channel, home, resources, |name| env::var_os(name))
    }

    /// The XDG base directories (data, cache, state for logs), from `variable` or their
    /// defaults under `home`. The state directory and engine socket stay at
    /// `Channel::state_dir`, where every client looks on both platforms.
    #[cfg(target_os = "linux")]
    pub fn for_home_with(
        channel: Channel,
        home: PathBuf,
        resources: PathBuf,
        variable: impl Fn(&str) -> Option<std::ffi::OsString>,
    ) -> Self {
        // The specification ignores a relative or empty value.
        let base = |name: &str, default: &str| {
            variable(name)
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .unwrap_or_else(|| home.join(default))
        };
        let data = base("XDG_DATA_HOME", ".local/share").join(channel.identifier());
        let engine = EnginePaths {
            app_data: data.clone(),
            app_config: data,
            app_cache: base("XDG_CACHE_HOME", ".cache").join(channel.identifier()),
            app_log: base("XDG_STATE_HOME", ".local/state")
                .join(channel.identifier())
                .join("logs"),
            resources,
            home: home.clone(),
        };
        Self {
            channel,
            engine,
            state_dir: channel.state_dir(&home),
        }
    }

    #[cfg(not(target_os = "linux"))]
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
            #[cfg(target_os = "macos")]
            package_kind: PackageKind::MacApp,
            #[cfg(not(target_os = "macos"))]
            package_kind: PackageKind::Other,
        }
    }
}

/// On Linux the engine is a plain directory: `<dir>/silod` beside its bundled tools, and
/// `<dir>/resources/` holding `engine-manifest.json` and the runtime resources.
#[cfg(target_os = "linux")]
pub fn default_resource_dir(executable: &Path) -> Result<PathBuf, String> {
    let directory = executable
        .parent()
        .ok_or_else(|| "silod executable has no parent directory".to_owned())?;
    Ok(directory.join(LINUX_RESOURCES_DIR))
}

/// The resources directory beside a Linux `silod`.
#[cfg(target_os = "linux")]
pub const LINUX_RESOURCES_DIR: &str = "resources";

#[cfg(not(target_os = "linux"))]
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
    ensure_private_directory_with_hook(path, euid, || {})
}

fn ensure_private_directory_with_hook(
    path: &Path,
    euid: u32,
    on_missing: impl FnOnce(),
) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            on_missing();
            match fs::create_dir(path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        Err(error) => return Err(error),
    }
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_dir() || metadata.uid() != euid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{} is not an owned directory", path.display()),
        ));
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
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
    use silo_engine::channel::Channel;
    use std::{
        process::{Child, Command},
        thread,
        time::{Duration, Instant},
    };

    use crate::ownership::{OwnerAcquisition, OwnerLease};

    const COLD_START_CHILD_MARKER: &str = ".silod-cold-start-child";

    fn wait_for_marker(path: &Path) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while !path.exists() {
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {}",
                path.display()
            );
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn wait_for_markers(paths: &[PathBuf]) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while paths.iter().any(|path| !path.exists()) {
            assert!(Instant::now() < deadline, "timed out waiting for markers");
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn wait_for_child_markers(paths: &[PathBuf], children: &mut [Child]) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while paths.iter().any(|path| !path.exists()) {
            for child in children.iter_mut() {
                if let Some(status) = child.try_wait().unwrap() {
                    panic!("cold-start child exited early with {status}");
                }
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for cold-start children"
            );
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn simultaneous_cold_start_child() {
        let home = std::env::current_dir().unwrap();
        if !home.join(COLD_START_CHILD_MARKER).exists() {
            return;
        }
        let home = PathBuf::from(std::env::var_os("HOME").unwrap());
        let id = std::process::id().to_string();
        let paths = SilodPaths::for_home(Channel::Development, home.clone(), PathBuf::new());
        let euid = unsafe { libc::geteuid() };
        let ready = home.join(format!("ready-{id}"));
        let release_create = home.join("release-create");
        let outcome_path = home.join(format!("outcome-{id}"));

        let directory_result = ensure_private_directory_with_hook(&paths.state_dir, euid, || {
            assert!(!paths.state_dir.exists());
            fs::write(&ready, "observed-missing").unwrap();
            wait_for_marker(&release_create);
        });

        let mut owner = None;
        let outcome = match directory_result {
            Err(error) => format!("directory-error:{:?}", error.kind()),
            Ok(()) => match OwnerLease::acquire(&paths, "0.1.0", "cold-start-test", euid) {
                Ok(OwnerAcquisition::Acquired(lease)) => {
                    owner = Some(lease);
                    "owner".to_owned()
                }
                Ok(OwnerAcquisition::NotOwner(_)) => "not-owner".to_owned(),
                Err(error) => format!("arbitration-error:{error}"),
            },
        };
        // The parent treats the file appearing as the outcome being complete, so publish it
        // whole; a plain write exposes the file empty for a moment.
        let staged = home.join(format!("staging-{id}"));
        fs::write(&staged, outcome).unwrap();
        fs::rename(&staged, &outcome_path).unwrap();
        if owner.is_some() {
            wait_for_marker(&home.join("release-owner"));
        }
        drop(owner);
    }

    #[test]
    fn simultaneous_cold_starts_reach_ownership_arbitration() {
        let home = tempfile::tempdir().unwrap();
        fs::write(home.path().join(COLD_START_CHILD_MARKER), "child").unwrap();
        let executable = std::env::current_exe().unwrap();
        let mut children = Vec::new();
        let mut child_ids = Vec::new();
        for _ in 0..2 {
            let _spawning = crate::FORK_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            children.push(
                Command::new(&executable)
                    .args([
                        "--exact",
                        "paths::tests::simultaneous_cold_start_child",
                        "--nocapture",
                    ])
                    .current_dir(home.path())
                    .env("HOME", home.path())
                    .spawn()
                    .unwrap(),
            );
            child_ids.push(children.last().unwrap().id().to_string());
        }

        let ready = child_ids
            .iter()
            .map(|id| home.path().join(format!("ready-{id}")))
            .collect::<Vec<_>>();
        wait_for_child_markers(&ready, &mut children);
        fs::write(home.path().join("release-create"), "go").unwrap();

        let outcomes = child_ids
            .iter()
            .map(|id| home.path().join(format!("outcome-{id}")))
            .collect::<Vec<_>>();
        wait_for_markers(&outcomes);
        let results = outcomes
            .iter()
            .map(|path| fs::read_to_string(path).unwrap())
            .collect::<Vec<_>>();
        fs::write(home.path().join("release-owner"), "go").unwrap();
        for child in &mut children {
            assert!(child.wait().unwrap().success());
        }

        assert_eq!(
            results.iter().filter(|result| *result == "owner").count(),
            1,
            "exactly one cold start must own the engine: {results:?}"
        );
        assert_eq!(
            results
                .iter()
                .filter(|result| *result == "not-owner")
                .count(),
            1,
            "both cold starts must reach owner arbitration: {results:?}"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_paths_follow_xdg_and_keep_the_state_dir_where_clients_look() {
        let home = PathBuf::from("/home/tester");
        let defaults = |channel| {
            SilodPaths::for_home_with(channel, home.clone(), PathBuf::from("/r"), |_| None)
        };
        let development = defaults(Channel::Development);
        assert_eq!(
            development.socket_path(),
            Path::new("/home/tester/.silo-dev/engine.sock")
        );
        assert_eq!(
            development.engine.app_data,
            Path::new("/home/tester/.local/share/org.silo.dev")
        );
        assert_eq!(development.engine.app_config, development.engine.app_data);
        assert_eq!(
            development.engine.app_cache,
            Path::new("/home/tester/.cache/org.silo.dev")
        );
        assert_eq!(
            development.engine.app_log,
            Path::new("/home/tester/.local/state/org.silo.dev/logs")
        );
        let production = defaults(Channel::Production);
        assert_eq!(
            production.socket_path(),
            Path::new("/home/tester/.silo/engine.sock")
        );
        assert_eq!(
            production.engine.app_data,
            Path::new("/home/tester/.local/share/org.silo.preview")
        );
        assert_eq!(
            production.identity("0.1.0").package_kind,
            PackageKind::Other
        );

        let set =
            SilodPaths::for_home_with(Channel::Development, home.clone(), PathBuf::new(), |name| {
                match name {
                    "XDG_DATA_HOME" => Some("/data".into()),
                    "XDG_CACHE_HOME" => Some("relative/cache".into()),
                    "XDG_STATE_HOME" => Some("".into()),
                    _ => None,
                }
            });
        assert_eq!(set.engine.app_data, Path::new("/data/org.silo.dev"));
        assert_eq!(
            set.engine.app_cache,
            Path::new("/home/tester/.cache/org.silo.dev")
        );
        assert_eq!(
            set.engine.app_log,
            Path::new("/home/tester/.local/state/org.silo.dev/logs")
        );
        assert_eq!(
            set.state_dir,
            Path::new("/home/tester/.silo-dev"),
            "XDG never moves the state directory"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_resources_sit_beside_the_executable() {
        assert_eq!(
            default_resource_dir(Path::new("/opt/silo-engine/silod")).unwrap(),
            Path::new("/opt/silo-engine/resources")
        );
    }

    #[cfg(target_os = "macos")]
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
