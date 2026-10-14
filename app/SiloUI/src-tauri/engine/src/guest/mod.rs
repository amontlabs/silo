//! Programmatic guest access for Linux and macOS computers (`guest.*`): the working account,
//! bounded and long-running execution, byte-exact files, folder watches and SSH aliases.
//!
//! Every guest operation is a short command run in the guest as the working account (or root),
//! through the platform's own channel: `msb exec` for Linux computers and the system `ssh` with
//! the computer's key for macOS computers. Commands travel as argv and never become shell
//! source, except on macOS where the single command line SSH carries is built from individually
//! quoted words. File bytes and process input cross the same channel as stdin and stdout.
//!
//! Two small Perl programs (`helpers/`) run in the guest: a supervisor that gives each run its
//! own process group, so cancelling and timing out really end the guest process tree, and a
//! file helper that stages, commits and reads files natively. Perl ships with both guest
//! platforms, so neither needs developer tools.
mod commands;
mod exec;
mod files;
mod process;
mod session;
mod ssh_alias;
mod target;
mod watch;

pub use commands::{editor_command, prepare, terminal_command};
pub use exec::{run_bounded, validate_request, ExecOutput, ExecRequest, GuestUser};
pub use files::{
    list, read, remove, write, FileEntry, FileKind, FileRead, FileWrite, Replace, WriteRequest,
};
pub use session::{
    cancel_exec, exec_get, exec_input, forget_finished, ExecSnapshot, ExecStart, ExecState,
    StartedExec,
};
pub use ssh_alias::{ensure_alias, remove_alias, AliasEnsure, AliasOutcome};
pub use target::{resolve, Account, Target};
pub use watch::{start_watch, WatchHandle};

use silo_api::jobs::ErrorCode;

/// Largest raw byte count a response may carry so that its JSON integer-array encoding
/// (up to four characters per byte) and envelope stay inside one 4 MiB frame.
pub const MAX_RESPONSE_RAW_BYTES: usize = 960 * 1024;
/// Encoded bytes a complete result may occupy; the frame limit minus envelope room.
pub const MAX_RESULT_ENCODED_BYTES: usize = 4 * 1024 * 1024 - 64 * 1024;
/// The default stdout allowance of a bounded run (95 A-37; B-32 candidate).
pub const DEFAULT_MAX_OUTPUT_BYTES: usize = 256 * 1024;
/// The largest stdout allowance a caller may request for a bounded run.
pub const MAX_OUTPUT_BYTES: usize = 768 * 1024;
/// The longest a bounded run may take; longer work uses `guest.exec.start`.
pub const MAX_BOUNDED_TIMEOUT_MS: u64 = 180_000;

/// A guest failure with the stable wire code a client branches on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuestError {
    pub code: ErrorCode,
    pub message: String,
}

impl GuestError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, message)
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotFound, message)
    }
}

impl std::fmt::Display for GuestError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for GuestError {}

/// Maps the engine's user-facing messages onto stable codes.
impl From<String> for GuestError {
    fn from(message: String) -> Self {
        let code = if message.starts_with("Start ") && message.ends_with(" first.") {
            ErrorCode::NotRunning
        } else if message.contains("cannot open this computer's Linux account")
            || message.contains("opens this computer with an account this Silo does not support")
        {
            ErrorCode::AccountNotMigrated
        } else if message.starts_with("OpenSSH is required") {
            ErrorCode::OpenSshMissing
        } else if message.contains("Add this line at the top of that file") {
            ErrorCode::IncludeNotWritable
        } else if message.contains("no longer exists") || message.ends_with("was not found.") {
            ErrorCode::NotFound
        } else if message.contains("timed out") || message.contains("took too long") {
            ErrorCode::TimedOut
        } else if message.contains("was cancelled") {
            ErrorCode::Cancelled
        } else {
            ErrorCode::Internal
        };
        Self { code, message }
    }
}

impl From<&str> for GuestError {
    fn from(message: &str) -> Self {
        message.to_owned().into()
    }
}

impl From<crate::runtime::RuntimeError> for GuestError {
    fn from(error: crate::runtime::RuntimeError) -> Self {
        use crate::runtime::RuntimeError;
        match error {
            RuntimeError::TimedOut { operation } => {
                Self::new(ErrorCode::TimedOut, format!("{operation} timed out."))
            }
            RuntimeError::Cancelled { operation } => {
                Self::new(ErrorCode::Cancelled, format!("{operation} was cancelled."))
            }
            other => other.to_string().into(),
        }
    }
}

/// Single-quotes `value` for a POSIX shell word.
pub(crate) fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Lowercase hex of `bytes`.
#[cfg(test)]
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
pub(crate) mod test_support {
    //! A fake `msb` that runs the command after `--` on this machine, so the Linux adapter,
    //! the Perl helpers and the process plumbing are exercised without a guest.
    use crate::runtime::RuntimePaths;
    use std::path::Path;

    pub const COMPUTER_ID: &str = "00000000-0000-4000-8000-0000000000aa";
    pub const COMPUTER_NAME: &str = "dev";

    const FAKE_MSB: &str = r#"#!/bin/sh
case "$1" in
  inspect)
    printf '%s\n' '{"name":"dev","status":"Running","config":{"labels":{"silo.managed":"true"}},"runtime_instance_id":"instance-1"}'
    exit 0 ;;
  --silo-working-account-protocol)
    printf '1\n'; exit 0 ;;
  exec)
    shift
    printf '%s\n' "$*" >> "$(dirname "$0")/msb.log"
    # There is no /workspace on this machine; the guest has one.
    case " $* " in *" test -d /workspace "*|*" test -d /workspace/project "*) exit 0 ;; esac
    while [ "$#" -gt 0 ] && [ "$1" != "--" ]; do
      if [ "$1" = "--env" ]; then shift; export "$1"; fi
      shift
    done
    shift
    exec "$@" ;;
esac
exit 2
"#;

    /// Runtime paths whose `msb` is the fake above, with `COMPUTER_NAME` in the inventory.
    pub fn runtime(directory: &Path) -> RuntimePaths {
        let paths = crate::test_support::paths(directory);
        crate::test_support::write_shell_script(&paths.executable, FAKE_MSB);
        std::fs::create_dir_all(&paths.home).unwrap();
        std::fs::write(&paths.library, b"").unwrap();
        let inventory = serde_json::json!({
            "schemaVersion": 1,
            "computers": [{
                "id": COMPUTER_ID,
                "name": COMPUTER_NAME,
                "cpus": 1,
                "maxCPUs": 2,
                "memoryGiB": 2,
                "maxMemoryGiB": 4,
                "workspaceStorageGiB": 10,
                "runtimeStorageGiB": 10
            }]
        });
        std::fs::write(&paths.metadata, inventory.to_string()).unwrap();
        paths
    }

    /// A stored macOS computer on `directory` (its record only: there is no live guest).
    pub fn macos_target(directory: &Path) -> super::Target {
        let shell = crate::macos_computers::guest_ssh_for_tests(directory, "mac-one");
        super::Target::Macos(shell)
    }

    pub fn target(directory: &Path) -> super::Target {
        super::resolve(Ok(runtime(directory)), directory, COMPUTER_ID).unwrap()
    }

    use crate::context::{
        Audience, EngineContext, EngineIdentity, EnginePaths, EngineRuntimeState, Event,
        MainThread, PackageKind, ProcessControl,
    };
    use std::sync::{Arc, Mutex};

    /// An engine context that records what it publishes and keeps jobs under a temp folder.
    pub struct RecordingContext {
        paths: EnginePaths,
        identity: EngineIdentity,
        runtime_state: Arc<EngineRuntimeState>,
        pub published: Mutex<Vec<(Event, Audience)>>,
    }

    impl RecordingContext {
        pub fn new(directory: &Path) -> Arc<Self> {
            Arc::new(Self {
                paths: EnginePaths {
                    app_data: directory.join("app-data"),
                    ..EnginePaths::default()
                },
                identity: EngineIdentity {
                    channel: crate::channel::Channel::Development,
                    version: "test".into(),
                    package_kind: PackageKind::Other,
                },
                runtime_state: Arc::new(EngineRuntimeState::default()),
                published: Mutex::new(Vec::new()),
            })
        }

        /// The payloads published under `name`, in order.
        pub fn events(&self, name: &str) -> Vec<serde_json::Value> {
            self.published
                .lock()
                .unwrap()
                .iter()
                .filter(|(event, _)| event.name == name)
                .map(|(event, _)| event.payload.clone())
                .collect()
        }

        /// Waits until job `id` is finished and returns its lookup.
        pub fn wait_finished(&self, id: &silo_api::jobs::JobId) -> silo_api::jobs::JobLookupResult {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            loop {
                if let Some(lookup) = crate::jobs::get_at(self, id, None).unwrap() {
                    if lookup.state == silo_api::jobs::JobState::Finished {
                        return lookup;
                    }
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "the job never finished"
                );
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        }
    }

    impl EngineContext for RecordingContext {
        fn paths(&self) -> &EnginePaths {
            &self.paths
        }

        fn identity(&self) -> &EngineIdentity {
            &self.identity
        }

        fn runtime_state(&self) -> Arc<EngineRuntimeState> {
            self.runtime_state.clone()
        }

        fn publish(&self, event: Event, audience: Audience) {
            self.published.lock().unwrap().push((event, audience));
        }

        fn main_thread(&self) -> Option<&dyn MainThread> {
            None
        }

        fn process(&self) -> &dyn ProcessControl {
            self
        }
    }

    impl ProcessControl for RecordingContext {
        fn exit(&self, _code: i32) {}

        fn restart(&self) -> ! {
            panic!("guest tests never restart the engine")
        }
    }
}
