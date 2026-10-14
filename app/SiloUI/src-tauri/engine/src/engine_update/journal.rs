//! The engine's update journal (design/p2 §3.2, schema version 2).
//!
//! The journal is the only record that a running computer was saved for an engine update.
//! Every transition is written to a new temporary file in the same directory, synced,
//! atomically renamed over the journal and followed by a directory sync, so a crash leaves
//! either the previous or the next complete journal and never a torn one. Artifacts are
//! synced by their adapters before an entry is recorded as `saved`.
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const SCHEMA_VERSION: u32 = 2;
pub const JOURNAL_FILE: &str = "update-resume.json";
/// Directory under the engine directory that holds captures made by adapters that do not
/// need to stay on a computer's own volume.
pub const SAVES_DIRECTORY: &str = "update-saves";
const MAX_JOURNAL_BYTES: u64 = 1024 * 1024;
const MAX_COMPUTERS: usize = 64;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ComputerKind {
    Linux,
    Macos,
}

/// Where the transaction stands as a whole.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TransactionPhase {
    /// The running set is journaled and computers are being paused and saved one by one.
    Capturing,
    /// Every computer is saved and held; only `engine.shutdown {reason: "update"}` or
    /// `engine.update.abort` may follow.
    Prepared,
    /// Held runtimes are being released without guest shutdown.
    Releasing,
    /// Every saved runtime was released. The next engine owner restores the set.
    Released,
    /// Recovery is restoring the saved set.
    Restoring,
    /// Recovery left at least one entry unresolved.
    Failed,
}

/// One computer's progress through save, release and restore.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ComputerPhase {
    Planned,
    Capturing,
    Saved,
    Released,
    Restoring,
    /// The restored guest is about to execute. Written before the first instruction runs,
    /// so a crash after it can never be replayed from the same capture.
    ResumeArmed,
    /// The original, held generation is about to be resumed in place (an abort, a rollback
    /// or a failed preparation). Written before the guest can execute again, so a crash
    /// after it is probed and never replayed from the capture over the disk it advanced.
    ResumingOriginal,
    Resumed,
    /// The computer's memory could not be brought back (its owner died before the save was
    /// durable, or a resumed guest's outcome is unknown). Its disk is preserved and nothing
    /// is replayed over it. Terminal: it no longer blocks later updates.
    Lost,
    Failed,
}

impl ComputerPhase {
    /// Whether the entry no longer needs the journal.
    pub fn settled(self) -> bool {
        matches!(self, Self::Resumed | Self::Lost)
    }
}

/// Which run of a computer a journal entry is about. A replacement run of the same computer
/// never matches, so a capture is never settled, resumed or discarded on its behalf.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeIdentity {
    /// The engine process that owns the machine (macOS machines live inside it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// The owner's generation counter for the machine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<u64>,
    /// The runtime's own instance id (Linux).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,
}

impl RuntimeIdentity {
    /// Whether `observed` is the run this identity names. Fields this identity does not
    /// record are not compared.
    pub fn matches(&self, observed: &RuntimeIdentity) -> bool {
        (self.owner.is_none() || self.owner == observed.owner)
            && (self.generation.is_none() || self.generation == observed.generation)
            && (self.instance.is_none() || self.instance == observed.instance)
    }
}

/// The identifier of the current engine process, used as the owner of macOS machines.
pub fn boot_id() -> &'static str {
    static BOOT: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    BOOT.get_or_init(|| uuid::Uuid::new_v4().to_string())
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MacCapture {
    /// Private, generation-specific directory holding the artifacts.
    pub directory: PathBuf,
    pub state_file: PathBuf,
    pub disk: PathBuf,
    pub auxiliary_storage: PathBuf,
    /// The generation of the paused machine that was saved. The restored machine is a new one.
    pub source_generation: u64,
    pub host_build: String,
    pub save_format_version: u32,
    pub artifact_sizes: std::collections::BTreeMap<String, u64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LinuxCapture {
    /// MicroSandbox snapshot lineage group and the immutable member that was captured.
    pub group: String,
    pub member: String,
    /// Always `full`: a disk-only snapshot would cold-boot the guest.
    pub scope: String,
    /// The runtime's network policy at capture time, needed to rebuild the computer if its
    /// held runtime is gone.
    pub network_policy: Value,
    pub integrity_verified: bool,
    /// The runtime instance that was captured, when the runtime reports one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_instance: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "platform", rename_all = "lowercase", deny_unknown_fields)]
pub enum CaptureRef {
    Macos(MacCapture),
    Linux(LinuxCapture),
}

impl CaptureRef {
    pub fn kind(&self) -> ComputerKind {
        match self {
            Self::Macos(_) => ComputerKind::Macos,
            Self::Linux(_) => ComputerKind::Linux,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SavedComputer {
    pub id: String,
    pub name: String,
    pub kind: ComputerKind,
    pub phase: ComputerPhase,
    /// The run that was running when the update was planned (the held generation).
    #[serde(default)]
    pub original: RuntimeIdentity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture: Option<CaptureRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restore_attempt_id: Option<String>,
    /// The restored run of the armed attempt, when it was known before the guest executed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_restore: Option<RuntimeIdentity>,
    /// False once a restore may have executed guest code and its outcome is unknown.
    #[serde(default = "default_true")]
    pub retryable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Journal {
    pub schema_version: u32,
    pub transaction_id: String,
    pub prepare_job_id: String,
    pub source_engine_version: String,
    pub host_build: String,
    pub phase: TransactionPhase,
    pub computers: Vec<SavedComputer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

impl Journal {
    pub fn new(
        transaction_id: String,
        prepare_job_id: String,
        source_engine_version: String,
        host_build: String,
        computers: Vec<SavedComputer>,
    ) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            transaction_id,
            prepare_job_id,
            source_engine_version,
            host_build,
            phase: TransactionPhase::Capturing,
            computers,
            last_error: None,
        }
    }

    fn validate(&self) -> Result<(), String> {
        let invalid = |what: &str| {
            Err(format!(
                "The update journal is invalid ({what}); it was preserved."
            ))
        };
        if self.schema_version != SCHEMA_VERSION {
            return invalid("unsupported version");
        }
        if !safe_component(&self.transaction_id) {
            return invalid("transaction id");
        }
        if self.computers.len() > MAX_COMPUTERS {
            return invalid("too many computers");
        }
        let mut ids = std::collections::HashSet::new();
        for computer in &self.computers {
            if !safe_component(&computer.id) || !ids.insert(computer.id.as_str()) {
                return invalid("computer id");
            }
            if computer.name.is_empty() || computer.name.len() > 256 {
                return invalid("computer name");
            }
            if let Some(capture) = &computer.capture {
                if capture.kind() != computer.kind {
                    return invalid("capture platform");
                }
            }
            let needs_capture = matches!(
                computer.phase,
                ComputerPhase::Saved
                    | ComputerPhase::Released
                    | ComputerPhase::Restoring
                    | ComputerPhase::ResumeArmed
            );
            if needs_capture && computer.capture.is_none() {
                return invalid("missing capture");
            }
        }
        Ok(())
    }
}

/// A component usable as one path segment: no separators, no traversal, bounded.
pub fn safe_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value != "."
        && value != ".."
        && !value.contains(['/', '\\', '\0'])
}

/// Reads and writes the journal in one engine directory.
#[derive(Clone)]
pub struct Store {
    directory: PathBuf,
    #[cfg(test)]
    fault: Option<std::sync::Arc<dyn Fn(Fault) -> Result<(), String> + Send + Sync>>,
}

/// The points where a journal write can be cut short in tests.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// Before the temporary file is created: the previous journal stays.
    BeforeWrite,
    /// After the rename and before the directory sync: the new journal is visible.
    AfterRename,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Store")
            .field("directory", &self.directory)
            .finish()
    }
}

impl Store {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
            #[cfg(test)]
            fault: None,
        }
    }

    #[cfg(test)]
    pub fn with_fault(
        mut self,
        fault: impl Fn(Fault) -> Result<(), String> + Send + Sync + 'static,
    ) -> Self {
        self.fault = Some(std::sync::Arc::new(fault));
        self
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn path(&self) -> PathBuf {
        self.directory.join(JOURNAL_FILE)
    }

    /// The root under which this store's adapters create generation directories.
    pub fn saves_root(&self) -> PathBuf {
        self.directory.join(SAVES_DIRECTORY)
    }

    pub fn exists(&self) -> bool {
        !matches!(
            fs::symlink_metadata(self.path()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound
        )
    }

    /// `Ok(None)` only when the journal is certainly absent. A journal that cannot be read or
    /// understood is an error and is never deleted or overwritten.
    pub fn load(&self) -> Result<Option<Journal>, String> {
        let file = match fs::File::open(self.path()) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err("The update journal could not be read; it was preserved.".into()),
        };
        let mut bytes = Vec::new();
        file.take(MAX_JOURNAL_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "The update journal could not be read; it was preserved.".to_owned())?;
        if bytes.len() as u64 > MAX_JOURNAL_BYTES {
            return Err("The update journal is too large; it was preserved.".into());
        }
        let journal: Journal = serde_json::from_slice(&bytes)
            .map_err(|_| "The update journal is invalid; it was preserved.".to_owned())?;
        journal.validate()?;
        Ok(Some(journal))
    }

    /// Durably replaces the journal. The previous journal remains until the rename.
    pub fn save(&self, journal: &Journal) -> Result<(), String> {
        #[cfg(test)]
        if let Some(fault) = &self.fault {
            fault(Fault::BeforeWrite)?;
        }
        let failed = |what: &str| format!("The update journal could not be {what}.");
        fs::create_dir_all(&self.directory).map_err(|_| failed("created"))?;
        let mut temporary =
            tempfile::NamedTempFile::new_in(&self.directory).map_err(|_| failed("written"))?;
        let mut bytes = serde_json::to_vec_pretty(journal).map_err(|_| failed("encoded"))?;
        bytes.push(b'\n');
        temporary.write_all(&bytes).map_err(|_| failed("written"))?;
        temporary
            .as_file()
            .sync_all()
            .map_err(|_| failed("synced"))?;
        temporary
            .persist(self.path())
            .map_err(|_| failed("replaced"))?;
        #[cfg(test)]
        if let Some(fault) = &self.fault {
            fault(Fault::AfterRename)?;
        }
        sync_directory(&self.directory).map_err(|_| failed("synced"))
    }

    /// Removes a resolved journal and syncs the directory.
    pub fn clear(&self) -> Result<(), String> {
        match fs::remove_file(self.path()) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("The resolved update journal could not be cleared.".into()),
        }
        sync_directory(&self.directory)
            .map_err(|_| "The resolved update journal could not be synced.".into())
    }
}

pub fn sync_directory(directory: &Path) -> std::io::Result<()> {
    fs::File::open(directory).and_then(|directory| directory.sync_all())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn computer(id: &str, phase: ComputerPhase) -> SavedComputer {
        SavedComputer {
            id: id.into(),
            name: format!("computer-{id}"),
            kind: ComputerKind::Macos,
            phase,
            original: RuntimeIdentity::default(),
            capture: None,
            restore_attempt_id: None,
            active_restore: None,
            retryable: true,
            last_error: None,
        }
    }

    fn journal() -> Journal {
        Journal::new(
            "tx-1".into(),
            "job:1".into(),
            "1.0.0".into(),
            "25F71".into(),
            vec![computer("a", ComputerPhase::Planned)],
        )
    }

    #[test]
    fn a_journal_round_trips_and_clears() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::new(directory.path());
        assert_eq!(store.load().unwrap(), None);
        store.save(&journal()).unwrap();
        assert_eq!(store.load().unwrap(), Some(journal()));
        store.clear().unwrap();
        assert_eq!(store.load().unwrap(), None);
        store.clear().unwrap();
    }

    #[test]
    fn a_write_cut_short_before_the_rename_keeps_the_previous_journal() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::new(directory.path());
        store.save(&journal()).unwrap();
        let mut next = journal();
        next.phase = TransactionPhase::Prepared;
        let crashing = Store::new(directory.path()).with_fault(|fault| {
            (fault == Fault::BeforeWrite)
                .then(|| Err("crash".into()))
                .unwrap_or(Ok(()))
        });
        assert!(crashing.save(&next).is_err());
        assert_eq!(store.load().unwrap(), Some(journal()));
        // After the rename the new journal is complete even though the directory sync never ran.
        let crashing = Store::new(directory.path()).with_fault(|fault| {
            (fault == Fault::AfterRename)
                .then(|| Err("crash".into()))
                .unwrap_or(Ok(()))
        });
        assert!(crashing.save(&next).is_err());
        assert_eq!(store.load().unwrap(), Some(next));
        let leftovers = fs::read_dir(directory.path()).unwrap().count();
        assert_eq!(
            leftovers, 1,
            "only the journal remains: no torn temporary files"
        );
    }

    #[test]
    fn an_unreadable_or_invalid_journal_is_an_error_and_is_preserved() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::new(directory.path());
        fs::write(store.path(), b"not json").unwrap();
        assert!(store.load().unwrap_err().contains("preserved"));
        assert_eq!(fs::read(store.path()).unwrap(), b"not json");
        let mut bad = journal();
        bad.computers[0].id = "../escape".into();
        fs::write(store.path(), serde_json::to_vec(&bad).unwrap()).unwrap();
        assert!(store.load().unwrap_err().contains("invalid"));
        let mut missing = journal();
        missing.computers[0].phase = ComputerPhase::Saved;
        fs::write(store.path(), serde_json::to_vec(&missing).unwrap()).unwrap();
        assert!(store.load().unwrap_err().contains("missing capture"));
        let mut oversized = journal();
        oversized.last_error = Some("x".repeat(MAX_JOURNAL_BYTES as usize));
        fs::write(store.path(), serde_json::to_vec(&oversized).unwrap()).unwrap();
        assert!(store.load().unwrap_err().contains("too large"));
    }
}
