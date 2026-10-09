//! Clipboard sharing between this Mac and a macOS computer: `engine` attaches
//! the SPICE agent port; this module installs the guest side, a pinned SPICE
//! vdagent that runs as a launchd agent in the `silo` user's session.
use super::store::{self, Layout, Record};
use super::{app_data, restore_image};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
use tauri::AppHandle;

/// Clipboard sharing needs this macOS major version on the Mac and in the guest.
pub(super) const MINIMUM_MACOS: u64 = 15;
const LOCK: &str = include_str!("../../guest/macos/clipboard-agent-lock.json");
const LABEL: &str = "org.silo.clipboard-agent";
const GUEST_DIR: &str = "/usr/local/libexec/silo";
const GUEST_ARCHIVE: &str = "/tmp/silo-clipboard-agent.tar.gz";
const GUEST_PLIST: &str = "/tmp/silo-clipboard-agent.plist";
const COPY_TIMEOUT: Duration = Duration::from_secs(120);
const RUN_TIMEOUT: Duration = Duration::from_secs(60);

/// Whether a computer's clipboard is shared with this Mac.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Clipboard {
    Available,
    #[serde(rename = "needs-macos-15")]
    NeedsMacos15,
}

/// The pinned guest agent release.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Lock {
    schema_version: u32,
    name: String,
    version: String,
    url: String,
    sha256: String,
    minimum_macos: u64,
    arguments: Vec<String>,
}

fn parse_lock(text: &str) -> Result<Lock, String> {
    let lock: Lock = serde_json::from_str(text)
        .map_err(|error| format!("The clipboard agent pin is invalid: {error}"))?;
    let digest_ok = lock.sha256.len() == 64
        && lock
            .sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    if lock.schema_version != 1
        || lock.name.is_empty()
        || lock.version.is_empty()
        || !lock.url.starts_with("https://")
        || !digest_ok
        || lock.arguments.is_empty()
    {
        return Err("The clipboard agent pin is invalid.".into());
    }
    Ok(lock)
}

/// The major component of a macOS version such as `15.4.1`.
fn major(version: &str) -> Option<u64> {
    version.split('.').next()?.trim().parse().ok()
}

/// Clipboard availability for a Mac running `host_major` and a computer whose
/// macOS version, when known, is `guest_version`.
pub(super) fn status(host_major: u64, guest_version: Option<&str>) -> Clipboard {
    let guest = guest_version.and_then(major).unwrap_or(0);
    if host_major >= MINIMUM_MACOS && guest >= MINIMUM_MACOS {
        Clipboard::Available
    } else {
        Clipboard::NeedsMacos15
    }
}

pub(super) fn status_for(record: &Record) -> Clipboard {
    status(
        super::engine::host_macos_major(),
        record
            .restore_image
            .as_ref()
            .map(|image| image.version.as_str()),
    )
}

fn launch_agent_plist(lock: &Lock) -> String {
    let arguments: String = std::iter::once(format!("{GUEST_DIR}/{}", lock.name))
        .chain(lock.arguments.iter().cloned())
        .map(|argument| format!("        <string>{argument}</string>\n"))
        .collect();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n\
         <dict>\n\
         \x20   <key>Label</key>\n\
         \x20   <string>{LABEL}</string>\n\
         \x20   <key>ProgramArguments</key>\n\
         \x20   <array>\n{arguments}    </array>\n\
         \x20   <key>RunAtLoad</key>\n\
         \x20   <true/>\n\
         \x20   <key>KeepAlive</key>\n\
         \x20   <true/>\n\
         \x20   <key>LimitLoadToSessionType</key>\n\
         \x20   <string>Aqua</string>\n\
         </dict>\n\
         </plist>\n"
    )
}

/// Installs the archive and launch agent as root, then starts the agent in the
/// `silo` user's session when that session exists; otherwise the next login does.
fn install_script(lock: &Lock) -> String {
    format!(
        "set -eu\n\
         [ \"$(shasum -a 256 {GUEST_ARCHIVE} | cut -d' ' -f1)\" = \"{sha}\" ]\n\
         mkdir -p {GUEST_DIR}\n\
         tar -xzf {GUEST_ARCHIVE} -C {GUEST_DIR} {name} LICENSE\n\
         chown root:wheel {GUEST_DIR}/{name} {GUEST_DIR}/LICENSE\n\
         chmod 755 {GUEST_DIR}/{name}\n\
         install -m 644 -o root -g wheel {GUEST_PLIST} /Library/LaunchAgents/{LABEL}.plist\n\
         rm -f {GUEST_ARCHIVE} {GUEST_PLIST}\n\
         uid=$(id -u silo)\n\
         launchctl bootout gui/$uid/{LABEL} 2>/dev/null || true\n\
         launchctl bootstrap gui/$uid /Library/LaunchAgents/{LABEL}.plist 2>/dev/null || true\n",
        sha = lock.sha256,
        name = lock.name,
    )
}

/// What `install_on` needs from a running guest.
trait Guest {
    fn copy(&self, local: &Path, remote: &str) -> Result<(), String>;
    /// Runs `script` as root and fails when it exits non-zero.
    fn root_script(&self, script: &str) -> Result<(), String>;
}

/// The guest reached over SSH as account `silo` with passwordless sudo.
struct Ssh<'a> {
    layout: &'a Layout,
    record: &'a Record,
}

impl Guest for Ssh<'_> {
    fn copy(&self, local: &Path, remote: &str) -> Result<(), String> {
        access::copy(self.layout, self.record, local, remote, COPY_TIMEOUT)
    }

    fn root_script(&self, script: &str) -> Result<(), String> {
        let output = access::run(
            self.layout,
            self.record,
            "sudo -n /bin/sh -s",
            Some(script.as_bytes()),
            RUN_TIMEOUT,
        )?;
        if output.status == 0 {
            Ok(())
        } else {
            Err(format!(
                "Installing the clipboard agent failed: {}",
                output.stderr.trim()
            ))
        }
    }
}

/// Stand-in for `guest_access`, which another slice provides. Replace this
/// module with `use super::guest_access as access;` once that exists.
mod access {
    use super::{Layout, Record};
    use std::{path::Path, time::Duration};

    pub(super) struct CommandOutput {
        pub status: i32,
        pub stderr: String,
    }

    pub(super) fn run(
        _layout: &Layout,
        _record: &Record,
        _command: &str,
        _stdin: Option<&[u8]>,
        _timeout: Duration,
    ) -> Result<CommandOutput, String> {
        Err("Silo cannot reach the computer yet.".into())
    }

    pub(super) fn copy(
        _layout: &Layout,
        _record: &Record,
        _local: &Path,
        _remote: &str,
        _timeout: Duration,
    ) -> Result<(), String> {
        Err("Silo cannot reach the computer yet.".into())
    }
}

/// Returns the pinned archive from the cache, downloading it when missing, and
/// checks its digest.
fn cached_archive(app_data: &Path, lock: &Lock) -> Result<PathBuf, String> {
    let dir = app_data
        .join("macos-guest-agents")
        .join(format!("{}-{}", lock.name, lock.version));
    for _ in 0..2 {
        let path = restore_image::download(&lock.url, &dir, &|| false, &mut |_, _| {}).map_err(
            |error| match error {
                restore_image::DownloadError::Cancelled => "The download was cancelled.".into(),
                restore_image::DownloadError::Failed(message) => message,
            },
        )?;
        let bytes = fs::read(&path).map_err(|error| store::io_error("read the agent", &error))?;
        if format!("{:x}", Sha256::digest(&bytes)) == lock.sha256 {
            return Ok(path);
        }
        let _ = fs::remove_file(&path);
    }
    Err("The downloaded clipboard agent does not match its pinned digest.".into())
}

fn install_on(
    guest: &dyn Guest,
    lock: &Lock,
    archive: &Path,
    scratch: &Path,
) -> Result<(), String> {
    let plist = scratch.join("agent.plist");
    fs::write(&plist, launch_agent_plist(lock))
        .map_err(|error| store::io_error("prepare the launch agent", &error))?;
    guest.copy(archive, GUEST_ARCHIVE)?;
    guest.copy(&plist, GUEST_PLIST)?;
    guest.root_script(&install_script(lock))
}

/// Installs the clipboard agent in the running computer `id`. Does nothing when
/// this Mac or the computer has macOS older than 15.
#[allow(dead_code)]
pub(super) fn install(app: &AppHandle, id: &str) -> Result<(), String> {
    let data = app_data(app)?;
    let record = store::load_all(&data)
        .into_iter()
        .find(|record| record.id == id)
        .ok_or("This computer no longer exists.")?;
    if status_for(&record) == Clipboard::NeedsMacos15 {
        return Ok(());
    }
    let lock = parse_lock(LOCK)?;
    let layout = Layout::new(&data, id);
    let archive = cached_archive(&data, &lock)?;
    let scratch = tempfile::tempdir().map_err(|error| store::io_error("prepare", &error))?;
    let guest = Ssh {
        layout: &layout,
        record: &record,
    };
    install_on(&guest, &lock, &archive, scratch.path())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn the_pinned_lock_is_valid() {
        let lock = parse_lock(LOCK).unwrap();
        assert_eq!(lock.minimum_macos, MINIMUM_MACOS);
        assert!(lock.url.contains(&format!("v{}", lock.version)));
    }

    #[test]
    fn locks_without_a_digest_or_https_are_rejected() {
        assert!(parse_lock(&LOCK.replace("303a50d4", "XXXXXXXX")).is_err());
        assert!(parse_lock(&LOCK.replace("https://", "http://")).is_err());
        assert!(parse_lock("{}").is_err());
    }

    #[test]
    fn versions_gate_the_clipboard() {
        assert_eq!(status(15, Some("15.4.1")), Clipboard::Available);
        assert_eq!(status(26, Some("26.0")), Clipboard::Available);
        assert_eq!(status(14, Some("15.4")), Clipboard::NeedsMacos15);
        assert_eq!(status(15, Some("14.7")), Clipboard::NeedsMacos15);
        assert_eq!(status(15, None), Clipboard::NeedsMacos15);
        assert_eq!(status(15, Some("garbage")), Clipboard::NeedsMacos15);
        assert_eq!(status(0, Some("15.0")), Clipboard::NeedsMacos15);
        assert_eq!(major("26.0.1"), Some(26));
    }

    #[test]
    fn the_clipboard_state_serializes_to_the_frontend_values() {
        assert_eq!(
            serde_json::to_value(Clipboard::Available).unwrap(),
            "available"
        );
        assert_eq!(
            serde_json::to_value(Clipboard::NeedsMacos15).unwrap(),
            "needs-macos-15"
        );
    }

    #[test]
    fn the_launch_agent_runs_only_the_vdagent_in_the_gui_session() {
        let plist = launch_agent_plist(&parse_lock(LOCK).unwrap());
        assert!(plist.contains("<string>/usr/local/libexec/silo/tart-guest-agent</string>"));
        assert!(plist.contains("<string>--run-vdagent</string>"));
        assert!(plist.contains("<string>Aqua</string>"));
        assert!(!plist.contains("--run-agent"));
    }

    struct Recorder(RefCell<Vec<String>>);

    impl Guest for Recorder {
        fn copy(&self, _local: &Path, remote: &str) -> Result<(), String> {
            self.0.borrow_mut().push(format!("copy {remote}"));
            Ok(())
        }

        fn root_script(&self, script: &str) -> Result<(), String> {
            self.0.borrow_mut().push(format!("script {}", script.len()));
            Ok(())
        }
    }

    #[test]
    fn installation_copies_the_archive_and_plist_before_the_script() {
        let lock = parse_lock(LOCK).unwrap();
        let guest = Recorder(RefCell::new(vec![]));
        let scratch = tempfile::tempdir().unwrap();
        install_on(&guest, &lock, Path::new("/nonexistent"), scratch.path()).unwrap();
        let steps = guest.0.borrow();
        assert_eq!(steps[0], format!("copy {GUEST_ARCHIVE}"));
        assert_eq!(steps[1], format!("copy {GUEST_PLIST}"));
        assert!(steps[2].starts_with("script "));
        let script = install_script(&lock);
        assert!(script.contains(&lock.sha256));
        assert!(script.contains("bootstrap gui/$uid"));
    }
}
