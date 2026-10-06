//! OpenSSH connection sharing for the bridge exchange path.
//!
//! Requests to one device reuse a single authenticated SSH connection (a control master
//! that OpenSSH keeps in the background) instead of paying a handshake per request. The
//! control socket lives in a private directory under the channel's Silo home, and the
//! master is closed when its device is removed, disconnected, refused or rekeyed, when
//! it grows old, and when Silo quits.
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Mutex,
    time::{Duration, Instant},
};

/// How long an idle master stays up; periodic polls keep it alive.
const PERSIST: &str = "60s";
/// A master is closed and replaced after this long, so withdrawn access on the other
/// device takes effect even while polling keeps the connection busy.
const MAX_AGE: Duration = Duration::from_secs(600);
/// A `sockaddr_un` path holds 103 bytes on macOS, and OpenSSH appends 17 more while it
/// creates the socket.
const MAX_CONTROL_PATH: usize = 103 - 17;
const CLOSE_TIMEOUT: Duration = Duration::from_secs(3);

static STARTED: Mutex<BTreeMap<String, Instant>> = Mutex::new(BTreeMap::new());

/// The private directory that holds the control sockets of the current channel.
fn directory() -> Result<PathBuf, String> {
    let dir = super::directory()?.join("mux");
    crate::runtime::prepare_private_directory(&dir).map_err(|error| error.to_string())?;
    Ok(dir)
}

/// The socket path for `address` in `dir`: a short digest, so any address fits. `None`
/// when even that path is too long for a Unix socket.
pub(super) fn control_path_in(dir: &Path, address: &str) -> Option<PathBuf> {
    let digest = Sha256::digest(address.as_bytes());
    let name: String = digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let path = dir.join(name);
    (path.as_os_str().len() <= MAX_CONTROL_PATH).then_some(path)
}

/// The options that share one master per address, started on first use.
pub(super) fn options(path: &Path) -> Vec<OsString> {
    let mut path_option = OsString::from("ControlPath=");
    path_option.push(path);
    [
        "-o".into(),
        "ControlMaster=auto".into(),
        "-o".into(),
        format!("ControlPersist={PERSIST}").into(),
        "-o".into(),
        path_option,
    ]
    .into()
}

/// Adds connection sharing to `command` for `address`. Without a usable socket location
/// the command connects on its own, as before.
pub(super) fn share(command: &mut Command, address: &str) {
    let Some(path) = directory()
        .ok()
        .and_then(|dir| control_path_in(&dir, address))
    else {
        return;
    };
    retire_if_old(address, &path, Instant::now());
    command.args(options(&path));
}

fn retire_if_old(address: &str, path: &Path, now: Instant) {
    let old = {
        let mut started = crate::sync::lock_or_recover(&STARTED, "remote SSH masters");
        match started.get(address) {
            Some(at) if now.duration_since(*at) < MAX_AGE => false,
            Some(_) => {
                started.insert(address.to_owned(), now);
                true
            }
            None => {
                started.insert(address.to_owned(), now);
                false
            }
        }
    };
    if old {
        exit(path);
    }
}

/// The command that asks the master at `path` to exit. It reads no configuration and
/// cannot connect anywhere: a missing or dead socket makes it fail at once.
pub(super) fn exit_command(path: &Path) -> Command {
    let mut command = Command::new("/usr/bin/ssh");
    let mut path_option = OsString::from("ControlPath=");
    path_option.push(path);
    command
        .args(["-F", "none", "-o"])
        .arg(path_option)
        .args(["-O", "exit", "--", "silo"]);
    command
}

fn exit(path: &Path) {
    let Ok(mut child) = exit_command(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return;
    };
    let deadline = Instant::now() + CLOSE_TIMEOUT;
    while matches!(child.try_wait(), Ok(None)) {
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Closes the master for `address`, so nothing authenticated earlier stays usable.
pub(super) fn close(address: &str) {
    if let Ok(dir) = directory() {
        close_in(&dir, address);
    }
}

fn close_in(dir: &Path, address: &str) {
    crate::sync::lock_or_recover(&STARTED, "remote SSH masters").remove(address);
    if let Some(path) = control_path_in(dir, address) {
        exit(&path);
        let _ = fs::remove_file(&path);
    }
}

/// Closes every master of the current channel and removes the sockets left behind.
pub(crate) fn close_all() {
    if let Ok(dir) = directory() {
        close_all_in(&dir);
    }
}

fn close_all_in(dir: &Path) {
    crate::sync::lock_or_recover(&STARTED, "remote SSH masters").clear();
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        exit(&path);
        let _ = fs::remove_file(&path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(command: &Command) -> Vec<String> {
        command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn options_share_one_master_through_the_private_socket() {
        let args: Vec<String> = options(Path::new("/private/mux/abc"))
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            [
                "-o",
                "ControlMaster=auto",
                "-o",
                "ControlPersist=60s",
                "-o",
                "ControlPath=/private/mux/abc"
            ]
        );
    }

    #[test]
    fn control_paths_are_short_distinct_and_fit_a_unix_socket() {
        let dir = Path::new("/Users/someone/.silo-dev/desktop-remote/mux");
        let long = format!("user@{}.example.com", "h".repeat(200));
        let first = control_path_in(dir, &long).unwrap();
        let second = control_path_in(dir, "office").unwrap();
        assert_ne!(first, second);
        assert_eq!(first, control_path_in(dir, &long).unwrap());
        assert!(first.as_os_str().len() + 17 <= 103);
        assert!(control_path_in(Path::new(&format!("/{}", "d".repeat(90))), "office").is_none());
    }

    #[test]
    fn the_directory_is_private_and_inside_the_channel_home() {
        let _test_state = crate::test_support::global_state();
        let home = tempfile::tempdir().unwrap();
        let dir = super::super::directory_in(home.path()).unwrap().join("mux");
        crate::runtime::prepare_private_directory(&dir).unwrap();
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert!(dir.starts_with(crate::channel::current().state_dir(home.path())));
    }

    #[test]
    fn exit_commands_only_talk_to_the_control_socket() {
        let command = exit_command(Path::new("/private/mux/abc"));
        assert_eq!(
            arguments(&command),
            [
                "-F",
                "none",
                "-o",
                "ControlPath=/private/mux/abc",
                "-O",
                "exit",
                "--",
                "silo"
            ]
        );
    }

    #[test]
    fn closing_a_device_or_quitting_removes_the_sockets() {
        let _test_state = crate::test_support::global_state();
        let dir = tempfile::tempdir().unwrap();
        let office = control_path_in(dir.path(), "office").unwrap();
        let studio = control_path_in(dir.path(), "studio").unwrap();
        fs::write(&office, "").unwrap();
        fs::write(&studio, "").unwrap();
        retire_if_old("office", &office, Instant::now());
        close_in(dir.path(), "office");
        assert!(!office.exists() && studio.exists());
        assert!(
            !crate::sync::lock_or_recover(&STARTED, "remote SSH masters").contains_key("office")
        );
        close_all_in(dir.path());
        assert!(!studio.exists());
    }

    #[test]
    fn a_master_older_than_the_limit_is_closed_once_and_restarted() {
        let address = format!("age-{}", uuid::Uuid::new_v4());
        let path = Path::new("/nonexistent/mux/socket");
        let start = Instant::now();
        retire_if_old(&address, path, start);
        let recorded = |address: &str| {
            *crate::sync::lock_or_recover(&STARTED, "remote SSH masters")
                .get(address)
                .unwrap()
        };
        assert_eq!(recorded(&address), start);
        retire_if_old(&address, path, start + MAX_AGE / 2);
        assert_eq!(recorded(&address), start);
        retire_if_old(&address, path, start + MAX_AGE);
        assert_eq!(recorded(&address), start + MAX_AGE);
    }
}
