//! Persistence and validation of macOS computers. Nothing here touches
//! Virtualization.framework, so it runs on every platform.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub(super) const SCHEMA_VERSION: u32 = 1;
const MIN_CPUS: u64 = 2;
const MIN_MEMORY_GIB: u64 = 4;
/// Memory kept for the host: the guest may not take it all.
const HOST_MEMORY_RESERVE_GIB: u64 = 4;
pub(super) const MIN_DISK_GIB: u64 = 32;
const MAX_DISK_GIB: u64 = 1024;
pub(super) const NEEDS_PERSONALIZING: &str =
    "This computer is not ready yet. Use Retry setup to finish it, or delete it.";
pub(super) const INTERRUPTED_INSTALL: &str =
    "Installation was interrupted. Delete this computer and create it again.";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum State {
    Preparing,
    /// A new computer is being copied from a template.
    Copying,
    Downloading,
    Installing,
    #[serde(rename = "setting-up")]
    SettingUp,
    Stopped,
    Starting,
    Running,
    Stopping,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Action {
    Start,
    Stop,
    ForceStop,
    Delete,
    /// Runs the provisioning steps that have not finished.
    Setup,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct RestoreImageInfo {
    pub version: String,
    pub build: String,
}

/// Which provisioning steps have finished. Persisted so a retry resumes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct SetupProgress {
    pub account: bool,
    pub sip: bool,
    pub computer_use: bool,
    pub clipboard: bool,
    /// A copy of a template still has the template's password, keys and identity.
    pub needs_personalizing: bool,
}

impl SetupProgress {
    pub(super) fn complete(self) -> bool {
        self.account && self.sip && self.computer_use && self.clipboard && !self.needs_personalizing
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Record {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub cpus: u64,
    #[serde(rename = "memoryGiB")]
    pub memory_gib: u64,
    #[serde(rename = "diskGiB")]
    pub disk_gib: u64,
    pub created_at: String,
    pub mac_address: String,
    pub restore_image: Option<RestoreImageInfo>,
    pub installed: bool,
    #[serde(default)]
    pub setup: SetupProgress,
    /// Installed here and not started by the user since its setup: the only kind of
    /// computer a template may be made from.
    #[serde(default)]
    pub pristine: bool,
    /// The folder name of the template this computer was copied from.
    #[serde(default)]
    pub template: Option<String>,
    /// The setup version of what the computer actually has installed, fixed when its
    /// computer use step finished (copies take their template's).
    #[serde(default)]
    pub setup_version: Option<String>,
    /// Set by a Restore: what the next Start does before it hands the computer over.
    #[serde(default)]
    pub pending_restore: Option<super::checkpoints::PendingRestore>,
    /// A fork whose guest still has the credentials of the computer it was copied from; they
    /// are in `inherited-access/` until its personalization has replaced them.
    #[serde(default)]
    pub inherited_access: bool,
}

impl Record {
    pub(super) fn os_version(&self) -> Option<String> {
        self.restore_image
            .as_ref()
            .map(|image| format!("{} ({})", image.version, image.build))
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreateRequest {
    pub name: String,
    pub cpus: u64,
    #[serde(rename = "memoryGiB")]
    pub memory_gib: u64,
    #[serde(rename = "diskGiB")]
    pub disk_gib: u64,
}

/// What the host offers; bounds for the resources a computer may request.
#[derive(Clone, Copy, Debug)]
pub(super) struct HostLimits {
    pub cpus: u64,
    pub memory_gib: u64,
}

impl HostLimits {
    pub(super) fn max_memory_gib(self) -> u64 {
        self.memory_gib
            .saturating_sub(HOST_MEMORY_RESERVE_GIB)
            .max(MIN_MEMORY_GIB)
    }
}

pub(super) fn validate_request(
    request: &CreateRequest,
    host: HostLimits,
    existing: &[Record],
    min_disk_gib: u64,
) -> Result<(), String> {
    crate::runtime::validate_name(&request.name).map_err(|error| error.to_string())?;
    if existing.iter().any(|record| record.name == request.name) {
        return Err(format!(
            "A macOS computer named '{}' already exists.",
            request.name
        ));
    }
    let max_cpus = host.cpus.max(MIN_CPUS);
    if !(MIN_CPUS..=max_cpus).contains(&request.cpus) {
        return Err(format!(
            "CPUs must be between {MIN_CPUS} and {max_cpus} on this Mac."
        ));
    }
    let max_memory = host.max_memory_gib();
    if !(MIN_MEMORY_GIB..=max_memory).contains(&request.memory_gib) {
        return Err(format!(
            "Memory must be between {MIN_MEMORY_GIB} and {max_memory} GiB on this Mac."
        ));
    }
    let min_disk = min_disk_gib.max(MIN_DISK_GIB);
    if !(min_disk..=MAX_DISK_GIB).contains(&request.disk_gib) {
        return Err(if min_disk > MIN_DISK_GIB {
            format!("Disk size must be between {min_disk} and {MAX_DISK_GIB} GiB, as new computers are copied from a template of {min_disk} GiB.")
        } else {
            format!("Disk size must be between {MIN_DISK_GIB} and {MAX_DISK_GIB} GiB.")
        });
    }
    Ok(())
}

pub(super) fn new_record(request: &CreateRequest, mac_address: String) -> Record {
    Record {
        schema_version: SCHEMA_VERSION,
        id: uuid::Uuid::new_v4().to_string(),
        name: request.name.clone(),
        cpus: request.cpus,
        memory_gib: request.memory_gib,
        disk_gib: request.disk_gib,
        created_at: time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default(),
        mac_address,
        restore_image: None,
        installed: false,
        setup: SetupProgress::default(),
        pristine: true,
        template: None,
        setup_version: None,
        pending_restore: None,
        inherited_access: false,
    }
}

/// The state and detail a computer has when Silo loads it from disk.
pub(super) fn initial_state(record: &Record) -> (State, Option<String>) {
    if record.installed {
        (State::Stopped, None)
    } else {
        (State::Failed, Some(INTERRUPTED_INSTALL.into()))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DeleteMode {
    /// Stop the work in progress, then remove the computer.
    Cancel,
    Remove,
}

pub(super) fn delete_mode(state: State) -> Result<DeleteMode, String> {
    match state {
        State::Preparing
        | State::Copying
        | State::Downloading
        | State::Installing
        | State::SettingUp => Ok(DeleteMode::Cancel),
        State::Stopped | State::Failed => Ok(DeleteMode::Remove),
        State::Starting | State::Running | State::Stopping => {
            Err("Stop the computer first.".into())
        }
    }
}

pub(super) fn start_allowed(state: State, record: &Record) -> Result<(), String> {
    if !record.installed {
        return Err(INTERRUPTED_INSTALL.into());
    }
    // Until then it still has its template's password and keys.
    if record.setup.needs_personalizing {
        return Err(NEEDS_PERSONALIZING.into());
    }
    match state {
        State::Stopped | State::Failed => Ok(()),
        State::Running | State::Starting => Err("This computer is already running.".into()),
        State::Stopping => Err("This computer is still stopping.".into()),
        State::Preparing | State::Copying | State::Downloading | State::Installing => {
            Err("macOS is still being installed on this computer.".into())
        }
        State::SettingUp => Err("This computer is still being set up.".into()),
    }
}

/// Whether a checkpoint operation may begin on a computer. Saving and restoring need the
/// machine settled (stopped, or running and not changing state); forking and deleting only
/// read or remove checkpoint files, so a computer that is changing state is fine for them.
pub(super) fn checkpoint_allowed(
    state: State,
    record: &Record,
    deleting: bool,
    operation_running: bool,
    kind: super::checkpoints::OperationKind,
) -> Result<(), String> {
    use super::checkpoints::OperationKind;
    if deleting {
        return Err("This computer is being deleted.".into());
    }
    if operation_running {
        return Err(super::checkpoints::BUSY.into());
    }
    if !record.installed {
        return Err(INTERRUPTED_INSTALL.into());
    }
    if !record.setup.complete() {
        return Err(super::checkpoints::NOT_READY.into());
    }
    match state {
        State::Preparing | State::Copying | State::Downloading | State::Installing => {
            Err("macOS is still being installed on this computer.".into())
        }
        State::SettingUp => Err("This computer is still being set up.".into()),
        State::Starting | State::Stopping
            if matches!(kind, OperationKind::Capture | OperationKind::Restore) =>
        {
            Err("Wait for the computer to finish starting or stopping.".into())
        }
        State::Stopped | State::Failed | State::Starting | State::Running | State::Stopping => {
            Ok(())
        }
    }
}

/// Whether provisioning can be run (again): an installed, idle computer with steps left.
pub(super) fn setup_allowed(state: State, record: &Record) -> Result<(), String> {
    if !record.installed {
        return Err(INTERRUPTED_INSTALL.into());
    }
    if record.setup.complete() {
        return Err("This computer is already set up.".into());
    }
    match state {
        State::Stopped | State::Failed => Ok(()),
        State::SettingUp => Err("This computer is already being set up.".into()),
        State::Running | State::Starting | State::Stopping => {
            Err("Stop the computer before setting it up.".into())
        }
        State::Preparing | State::Copying | State::Downloading | State::Installing => {
            Err("macOS is still being installed on this computer.".into())
        }
    }
}

pub(super) fn root(app_data: &Path) -> PathBuf {
    app_data.join("macos-computers")
}

/// The id of every computer folder on disk, readable or not.
pub(super) fn computer_ids(app_data: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(root(app_data)) else {
        return Vec::new();
    };
    let mut ids: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    ids.sort();
    ids
}

/// The name of every macOS computer folder on disk, lowercased, without needing
/// the registry or a valid record.
pub(super) fn names(app_data: &Path) -> Vec<String> {
    computer_ids(app_data)
        .into_iter()
        .filter_map(|id| fs::read(root(app_data).join(id).join("computer.json")).ok())
        .filter_map(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .filter_map(|record| record.get("name")?.as_str().map(str::to_ascii_lowercase))
        .collect()
}

pub(super) fn restore_images(app_data: &Path) -> PathBuf {
    app_data.join("macos-restore-images")
}

/// The files one computer owns.
#[derive(Clone, Debug)]
pub(super) struct Layout {
    pub dir: PathBuf,
    /// Where the guest-access secrets are read from instead of `guest-access/`: the
    /// first login to a copy of a template uses the template's.
    pub access: Option<PathBuf>,
}

impl Layout {
    pub(super) fn new(app_data: &Path, id: &str) -> Self {
        Self {
            dir: root(app_data).join(id),
            access: None,
        }
    }

    pub(super) fn with_access(mut self, access: PathBuf) -> Self {
        self.access = Some(access);
        self
    }

    pub(super) fn record(&self) -> PathBuf {
        self.dir.join("computer.json")
    }

    pub(super) fn disk(&self) -> PathBuf {
        self.dir.join("disk.img")
    }

    pub(super) fn auxiliary_storage(&self) -> PathBuf {
        self.dir.join("auxiliary-storage.img")
    }

    pub(super) fn hardware_model(&self) -> PathBuf {
        self.dir.join("hardware-model.bin")
    }

    pub(super) fn machine_identifier(&self) -> PathBuf {
        self.dir.join("machine-identifier.bin")
    }
}

pub(super) fn save(layout: &Layout, record: &Record) -> Result<(), String> {
    fs::create_dir_all(&layout.dir).map_err(|error| io_error("create the computer", &error))?;
    let json = serde_json::to_vec_pretty(record).map_err(|error| error.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(&layout.dir)
        .map_err(|error| io_error("save the computer", &error))?;
    std::io::Write::write_all(&mut file, &json)
        .map_err(|error| io_error("save the computer", &error))?;
    file.as_file()
        .sync_all()
        .map_err(|error| io_error("save the computer", &error))?;
    file.persist(layout.record())
        .map_err(|error| io_error("save the computer", &error.error))?;
    sync_dir(&layout.dir).map_err(|error| io_error("save the computer", &error))
}

/// Makes the entries of a folder (a rename, a new or removed file) durable.
pub(super) fn sync_dir(dir: &Path) -> std::io::Result<()> {
    fs::File::open(dir)?.sync_all()
}

/// Every readable computer, ordered by creation. Unreadable entries are skipped.
pub(super) fn load_all(app_data: &Path) -> Vec<Record> {
    let Ok(entries) = fs::read_dir(root(app_data)) else {
        return Vec::new();
    };
    let mut records: Vec<Record> = entries
        .flatten()
        .filter_map(|entry| {
            let bytes = fs::read(entry.path().join("computer.json")).ok()?;
            let record: Record = serde_json::from_slice(&bytes).ok()?;
            let directory = entry.file_name();
            (record.schema_version == SCHEMA_VERSION && directory.to_str() == Some(&record.id))
                .then_some(record)
        })
        .collect();
    records.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
    records
}

pub(super) fn remove(layout: &Layout) -> Result<(), String> {
    match fs::remove_dir_all(&layout.dir) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error("delete the computer", &error)),
    }
}

/// Creates the sparse raw disk image.
pub(super) fn create_disk(path: &Path, gib: u64) -> Result<(), String> {
    let file = fs::File::create(path).map_err(|error| io_error("create the disk", &error))?;
    file.set_len(gib * 1024 * 1024 * 1024)
        .map_err(|error| io_error("create the disk", &error))
}

pub(super) fn io_error(action: &str, error: &std::io::Error) -> String {
    match error.kind() {
        std::io::ErrorKind::StorageFull => {
            format!("Silo could not {action}: this Mac is out of disk space.")
        }
        std::io::ErrorKind::PermissionDenied => {
            format!("Silo could not {action}: permission denied.")
        }
        _ => format!("Silo could not {action}: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOST: HostLimits = HostLimits {
        cpus: 10,
        memory_gib: 32,
    };

    fn request() -> CreateRequest {
        CreateRequest {
            name: "mac-one".into(),
            cpus: 4,
            memory_gib: 8,
            disk_gib: 64,
        }
    }

    fn record(request: &CreateRequest) -> Record {
        new_record(request, "02:00:00:00:00:01".into())
    }

    #[test]
    fn accepts_the_default_request() {
        assert_eq!(validate_request(&request(), HOST, &[], 0), Ok(()));
    }

    #[test]
    fn rejects_names_the_linux_rule_rejects() {
        for name in ["", "Mac", "1mac", "mac_one", &"a".repeat(33)] {
            let request = CreateRequest {
                name: name.into(),
                ..request()
            };
            assert!(validate_request(&request, HOST, &[], 0).is_err(), "{name}");
        }
    }

    #[test]
    fn rejects_duplicate_names() {
        let existing = [record(&request())];
        let error = validate_request(&request(), HOST, &existing, 0).unwrap_err();
        assert!(error.contains("already exists"), "{error}");
    }

    #[test]
    fn bounds_resources_by_the_host() {
        for (cpus, memory_gib, disk_gib) in [
            (1, 8, 64),
            (11, 8, 64),
            (4, 3, 64),
            (4, 29, 64),
            (4, 8, 31),
            (4, 8, 1025),
        ] {
            let request = CreateRequest {
                cpus,
                memory_gib,
                disk_gib,
                ..request()
            };
            assert!(
                validate_request(&request, HOST, &[], 0).is_err(),
                "{cpus} {memory_gib} {disk_gib}"
            );
        }
        let edge = CreateRequest {
            cpus: 10,
            memory_gib: 28,
            disk_gib: 1024,
            ..request()
        };
        assert_eq!(validate_request(&edge, HOST, &[], 0), Ok(()));
    }

    fn finished() -> Record {
        let mut record = record(&request());
        record.installed = true;
        record.setup = SetupProgress {
            account: true,
            sip: true,
            computer_use: true,
            clipboard: true,
            needs_personalizing: false,
        };
        record
    }

    #[test]
    fn checkpoints_need_a_finished_idle_computer() {
        use super::super::checkpoints::OperationKind::{Capture, Delete, Fork, Restore};
        let allowed = |state, record: &Record, deleting, busy, kind| {
            checkpoint_allowed(state, record, deleting, busy, kind)
        };
        let ready = finished();
        for kind in [Capture, Restore, Fork, Delete] {
            for state in [State::Stopped, State::Running, State::Failed] {
                assert_eq!(allowed(state, &ready, false, false, kind), Ok(()));
            }
            assert!(allowed(State::Stopped, &ready, true, false, kind)
                .unwrap_err()
                .contains("deleted"));
            assert!(allowed(State::Stopped, &ready, false, true, kind)
                .unwrap_err()
                .contains("in progress"));
            for state in [
                State::Preparing,
                State::Copying,
                State::Downloading,
                State::Installing,
                State::SettingUp,
            ] {
                assert!(
                    allowed(state, &ready, false, false, kind).is_err(),
                    "{state:?}"
                );
            }
        }
        // Saving and restoring need a settled machine; forking and deleting only touch files.
        for state in [State::Starting, State::Stopping] {
            assert!(allowed(state, &ready, false, false, Capture).is_err());
            assert!(allowed(state, &ready, false, false, Restore).is_err());
            assert_eq!(allowed(state, &ready, false, false, Fork), Ok(()));
            assert_eq!(allowed(state, &ready, false, false, Delete), Ok(()));
        }
        let mut unfinished = finished();
        unfinished.setup.sip = false;
        assert_eq!(
            allowed(State::Stopped, &unfinished, false, false, Capture),
            Err(super::super::checkpoints::NOT_READY.to_string())
        );
        let mut copy = finished();
        copy.setup.needs_personalizing = true;
        assert!(allowed(State::Stopped, &copy, false, false, Capture).is_err());
        let mut interrupted = finished();
        interrupted.installed = false;
        assert_eq!(
            allowed(State::Failed, &interrupted, false, false, Capture),
            Err(INTERRUPTED_INSTALL.to_string())
        );
    }

    #[test]
    fn a_record_saved_before_checkpoints_still_loads() {
        let mut json = serde_json::to_value(finished()).unwrap();
        let object = json.as_object_mut().unwrap();
        object.remove("pendingRestore");
        object.remove("inheritedAccess");
        let loaded: Record = serde_json::from_value(json).unwrap();
        assert_eq!(loaded.pending_restore, None);
        assert!(!loaded.inherited_access);
    }

    #[test]
    fn a_pending_restore_survives_a_save() {
        let data = tempfile::tempdir().unwrap();
        let layout = Layout::new(data.path(), "one");
        let mut record = finished();
        record.id = "one".into();
        record.pending_restore = Some(super::super::checkpoints::PendingRestore {
            checkpoint_id: "c".into(),
            memory: true,
        });
        save(&layout, &record).unwrap();
        let loaded = load_all(data.path());
        assert_eq!(loaded, [record]);
    }

    #[test]
    fn a_template_sets_the_smallest_disk() {
        let at = |disk_gib, min| {
            validate_request(
                &CreateRequest {
                    disk_gib,
                    ..request()
                },
                HOST,
                &[],
                min,
            )
        };
        assert_eq!(at(32, 0), Ok(()));
        assert_eq!(at(64, 64), Ok(()));
        assert_eq!(at(65, 64), Ok(()));
        let error = at(63, 64).unwrap_err();
        assert!(error.contains("between 64 and 1024"), "{error}");
        assert!(error.contains("template"), "{error}");
        // A template smaller than the general minimum changes nothing.
        assert!(at(31, 16).unwrap_err().contains("between 32 and 1024"));
        assert_eq!(at(1024, 1024), Ok(()));
    }

    #[test]
    fn a_copy_is_complete_only_once_it_is_personalized() {
        let mut setup = SetupProgress {
            account: true,
            sip: true,
            computer_use: true,
            clipboard: true,
            needs_personalizing: true,
        };
        assert!(!setup.complete());
        setup.needs_personalizing = false;
        assert!(setup.complete());
    }

    #[test]
    fn records_from_before_templates_load_as_finished_and_not_pristine() {
        let app_data = tempfile::tempdir().unwrap();
        let mut original = record(&request());
        original.installed = true;
        original.setup = SetupProgress {
            account: true,
            sip: true,
            computer_use: true,
            clipboard: true,
            needs_personalizing: false,
        };
        let layout = Layout::new(app_data.path(), &original.id);
        save(&layout, &original).unwrap();
        let mut json: serde_json::Value =
            serde_json::from_slice(&fs::read(layout.record()).unwrap()).unwrap();
        let object = json.as_object_mut().unwrap();
        object.remove("pristine");
        object.remove("template");
        object["setup"]
            .as_object_mut()
            .unwrap()
            .remove("needsPersonalizing");
        fs::write(layout.record(), serde_json::to_vec(&json).unwrap()).unwrap();
        let loaded = &load_all(app_data.path())[0];
        assert!(loaded.setup.complete());
        assert!(!loaded.pristine);
        assert_eq!(loaded.template, None);
    }

    #[test]
    fn a_small_host_still_allows_the_minimum_memory() {
        let host = HostLimits {
            cpus: 8,
            memory_gib: 8,
        };
        let request = CreateRequest {
            memory_gib: 4,
            ..request()
        };
        assert_eq!(validate_request(&request, host, &[], 0), Ok(()));
    }

    #[test]
    fn records_round_trip_through_disk() {
        let app_data = tempfile::tempdir().unwrap();
        let mut original = record(&request());
        original.restore_image = Some(RestoreImageInfo {
            version: "26.6.2".into(),
            build: "25G83".into(),
        });
        let layout = Layout::new(app_data.path(), &original.id);
        save(&layout, &original).unwrap();
        assert_eq!(load_all(app_data.path()), vec![original.clone()]);
        assert_eq!(original.os_version().as_deref(), Some("26.6.2 (25G83)"));
        let json: serde_json::Value =
            serde_json::from_slice(&fs::read(layout.record()).unwrap()).unwrap();
        assert_eq!(json["memoryGiB"], 8);
        assert_eq!(json["diskGiB"], 64);
        assert_eq!(json["schemaVersion"], 1);
        assert_eq!(json["restoreImage"]["build"], "25G83");
    }

    #[test]
    fn loading_skips_unreadable_and_misplaced_entries() {
        let app_data = tempfile::tempdir().unwrap();
        let good = record(&request());
        save(&Layout::new(app_data.path(), &good.id), &good).unwrap();
        let broken = root(app_data.path()).join("broken");
        fs::create_dir_all(&broken).unwrap();
        fs::write(broken.join("computer.json"), b"{").unwrap();
        let mut misplaced = record(&request());
        misplaced.name = "other".into();
        let layout = Layout::new(app_data.path(), "not-its-id");
        save(&layout, &misplaced).unwrap();
        assert_eq!(load_all(app_data.path()), vec![good]);
    }

    #[test]
    fn an_interrupted_install_loads_as_failed() {
        let mut interrupted = record(&request());
        assert_eq!(
            initial_state(&interrupted),
            (State::Failed, Some(INTERRUPTED_INSTALL.into()))
        );
        interrupted.installed = true;
        assert_eq!(initial_state(&interrupted), (State::Stopped, None));
    }

    #[test]
    fn delete_rules_follow_the_state() {
        use State::*;
        for state in [Preparing, Copying, Downloading, Installing, SettingUp] {
            assert_eq!(delete_mode(state), Ok(DeleteMode::Cancel));
        }
        for state in [Stopped, Failed] {
            assert_eq!(delete_mode(state), Ok(DeleteMode::Remove));
        }
        for state in [Starting, Running, Stopping] {
            assert_eq!(delete_mode(state), Err("Stop the computer first.".into()));
        }
    }

    #[test]
    fn start_requires_an_installed_stopped_computer() {
        let mut computer = record(&request());
        assert!(start_allowed(State::Failed, &computer).is_err());
        computer.installed = true;
        assert!(start_allowed(State::Stopped, &computer).is_ok());
        assert!(start_allowed(State::Failed, &computer).is_ok());
        assert!(start_allowed(State::Running, &computer).is_err());
        assert!(start_allowed(State::Installing, &computer).is_err());
        computer.setup.needs_personalizing = true;
        assert_eq!(
            start_allowed(State::Stopped, &computer),
            Err(NEEDS_PERSONALIZING.into())
        );
        assert!(start_allowed(State::Failed, &computer).is_err());
        assert!(setup_allowed(State::Stopped, &computer).is_ok());
    }

    #[test]
    fn setup_runs_on_an_idle_installed_computer_with_steps_left() {
        let mut computer = record(&request());
        assert!(setup_allowed(State::Stopped, &computer).is_err());
        computer.installed = true;
        assert!(setup_allowed(State::Stopped, &computer).is_ok());
        assert!(setup_allowed(State::Failed, &computer).is_ok());
        for state in [State::SettingUp, State::Running, State::Installing] {
            assert!(setup_allowed(state, &computer).is_err());
        }
        computer.setup.account = true;
        computer.setup.sip = true;
        computer.setup.computer_use = true;
        assert!(setup_allowed(State::Stopped, &computer).is_ok());
        computer.setup.clipboard = true;
        assert!(setup_allowed(State::Stopped, &computer).is_err());
        assert!(start_allowed(State::SettingUp, &computer).is_err());
    }

    #[test]
    fn setup_progress_persists_and_old_records_default_to_none_done() {
        let app_data = tempfile::tempdir().unwrap();
        let mut original = record(&request());
        original.installed = true;
        original.setup.account = true;
        original.setup.sip = true;
        let layout = Layout::new(app_data.path(), &original.id);
        save(&layout, &original).unwrap();
        let loaded = load_all(app_data.path());
        assert_eq!(loaded, vec![original.clone()]);
        let json: serde_json::Value =
            serde_json::from_slice(&fs::read(layout.record()).unwrap()).unwrap();
        assert_eq!(
            json["setup"],
            serde_json::json!({"account": true, "sip": true, "computerUse": false, "clipboard": false, "needsPersonalizing": false})
        );

        let mut old = json;
        old.as_object_mut().unwrap().remove("setup");
        fs::write(layout.record(), serde_json::to_vec(&old).unwrap()).unwrap();
        let loaded = load_all(app_data.path());
        assert_eq!(loaded[0].setup, SetupProgress::default());
        assert!(!loaded[0].setup.complete());
    }

    #[test]
    fn states_serialize_to_the_contract_strings() {
        assert_eq!(
            serde_json::to_value(State::SettingUp).unwrap(),
            serde_json::json!("setting-up")
        );
        assert_eq!(
            serde_json::to_value(State::Stopped).unwrap(),
            serde_json::json!("stopped")
        );
    }

    #[test]
    fn every_computer_folder_is_listed_even_when_its_record_is_unreadable() {
        let app_data = tempfile::tempdir().unwrap();
        let good = record(&request());
        save(&Layout::new(app_data.path(), &good.id), &good).unwrap();
        fs::create_dir_all(root(app_data.path()).join("broken")).unwrap();
        fs::write(root(app_data.path()).join("stray-file"), b"").unwrap();
        let mut expected = vec![good.id.clone(), "broken".to_string()];
        expected.sort();
        assert_eq!(computer_ids(app_data.path()), expected);
        assert!(computer_ids(&app_data.path().join("missing")).is_empty());
    }

    #[test]
    fn names_come_from_the_folders_on_disk() {
        let app_data = tempfile::tempdir().unwrap();
        let computer = record(&request());
        save(&Layout::new(app_data.path(), &computer.id), &computer).unwrap();
        // A record this version cannot load still holds a name.
        let odd = root(app_data.path()).join("odd");
        fs::create_dir_all(&odd).unwrap();
        fs::write(odd.join("computer.json"), br#"{"name": "Other-One"}"#).unwrap();
        let mut names = names(app_data.path());
        names.sort();
        assert_eq!(names, ["mac-one", "other-one"]);
    }

    #[test]
    fn removing_deletes_the_directory_and_tolerates_absence() {
        let app_data = tempfile::tempdir().unwrap();
        let computer = record(&request());
        let layout = Layout::new(app_data.path(), &computer.id);
        save(&layout, &computer).unwrap();
        remove(&layout).unwrap();
        assert!(!layout.dir.exists());
        remove(&layout).unwrap();
    }

    #[test]
    fn the_disk_is_sparse() {
        use std::os::unix::fs::MetadataExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("disk.img");
        create_disk(&path, 32).unwrap();
        let metadata = fs::metadata(&path).unwrap();
        assert_eq!(metadata.len(), 32 * 1024 * 1024 * 1024);
        assert!(metadata.blocks() * 512 < 1024 * 1024);
    }

    #[test]
    fn actions_parse_from_the_contract_strings() {
        for (text, action) in [
            ("start", Action::Start),
            ("stop", Action::Stop),
            ("force-stop", Action::ForceStop),
            ("delete", Action::Delete),
            ("setup", Action::Setup),
        ] {
            let parsed: Action = serde_json::from_value(serde_json::json!(text)).unwrap();
            assert_eq!(parsed, action);
        }
    }
}
