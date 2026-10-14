//! The journal records intent before side effects. After a relaunch, recovery
//! never repeats an export or import: it removes this operation's partial
//! output and reports what happened. It adopts only a verified export file or
//! an import whose computer was saved under the identity journaled for it.
use super::*;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Journal {
    version: u32,
    id: String,
    archive: Archive,
    request: Request,
    cancelled: bool,
    terminal: Option<Terminal>,
    /// Set by the first launch of an upgrade for an operation that left data only the
    /// runtime can remove. Everything else it left was removed without the runtime; this
    /// cleanup waits for the converted storage, where recovery finishes it (see
    /// `settle_before_migration`). Omitted when false, so older builds can still read it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    awaiting_upgrade: bool,
    /// Set on a result the user could not have seen as it happened: one an upgrade
    /// produced (an operation interrupted before it, settled before or after the storage
    /// migration), or the notice that an unreadable record was set aside. It stays until
    /// the user has been shown the result and acknowledges it (see [`acknowledge`]), or
    /// dismisses it, which removes the journal. Journals written before this field existed
    /// have none, and so count as seen. Omitted when false, so older builds can read every
    /// result that is not waiting to be shown.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    unseen: bool,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum Request {
    Backup {
        names: Vec<String>,
        /// Unused: older builds stopped computers for an export and listed them
        /// here to restart them. Kept (empty) so older builds can still read it.
        #[serde(default)]
        computers: Vec<(String, String)>,
        #[serde(default)]
        running: Vec<(String, String)>,
        /// Present when the export packages a stored checkpoint rather than the
        /// computer's current state. Absent in journals written before this field.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        checkpoint_id: Option<String>,
        /// Saved before snapshot create; cleared after capture verification.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pending_capture: Option<ExportCapture>,
    },
    Restore {
        name: String,
        source: Option<String>,
        /// The import is committed to `macos-computers/<id>`, not a
        /// MicroSandbox snapshot group.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        macos: bool,
        /// The new computer's id, saved before its checkpoint record and settings
        /// are written. A saved computer with this id means the import finished.
        id: Option<String>,
        /// The native snapshot group the import loaded, saved with `id`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        group: Option<String>,
    },
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExportCapture {
    computer_id: String,
    group: String,
    member: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Terminal {
    outcome: String,
    title: String,
    message: String,
    detail: Option<String>,
    running: Vec<String>,
}
impl Journal {
    pub(super) fn is_pending(&self) -> bool {
        self.terminal.is_none()
    }
    pub(super) fn identity(&self) -> &str {
        &self.id
    }
    #[cfg(test)]
    pub(super) fn archive_path(&self) -> String {
        self.archive.archive_path.clone()
    }
    pub(super) fn backup(
        archive: Archive,
        names: Vec<String>,
        checkpoint_id: Option<String>,
    ) -> Self {
        Self {
            version: 1,
            id: uuid::Uuid::new_v4().to_string(),
            archive,
            request: Request::Backup {
                names,
                computers: vec![],
                running: vec![],
                checkpoint_id,
                pending_capture: None,
            },
            cancelled: false,
            terminal: None,
            awaiting_upgrade: false,
            unseen: false,
        }
    }
    pub(super) fn restore(archive: Archive, name: String, source: Option<String>) -> Self {
        Self {
            version: 1,
            id: uuid::Uuid::new_v4().to_string(),
            archive,
            request: Request::Restore {
                name,
                source,
                macos: false,
                id: None,
                group: None,
            },
            cancelled: false,
            terminal: None,
            awaiting_upgrade: false,
            unseen: false,
        }
    }
    /// Whether this holds a result the user has not yet been shown (see `unseen`).
    pub(super) fn is_unseen_result(&self) -> bool {
        self.terminal.is_some() && self.unseen
    }
    /// Whether the first launch of an upgrade settled everything it could without the
    /// runtime and left the rest for recovery in the converted storage.
    pub(super) fn is_awaiting_upgrade(&self) -> bool {
        self.terminal.is_none() && self.awaiting_upgrade
    }
    /// Whether startup should let this operation's recovery finish before
    /// other computer recovery and automatic starts. Only an import touches
    /// computer records; an export's recovery only checks files it wrote.
    pub(super) fn blocks_startup(&self) -> bool {
        matches!(self.request, Request::Restore { .. })
    }
    fn kind(&self) -> &'static str {
        match self.request {
            Request::Backup { .. } => "backup",
            Request::Restore { .. } => "restore",
        }
    }
    fn target(&self) -> Option<String> {
        match &self.request {
            Request::Restore { name, .. } => Some(name.clone()),
            _ => None,
        }
    }
    pub(super) fn operation(&self) -> Operation {
        match &self.terminal {
            Some(result) => Operation::Result {
                operation: self.kind(),
                archive: self.archive.clone(),
                running_names: Vec::new(),
                target_name: self.target(),
                outcome: match result.outcome.as_str() {
                    // Older builds reported a complete export whose computer did not
                    // restart as "restart-required"; exports no longer stop computers.
                    "success" | "restart-required" => "success",
                    "cancelled" => "cancelled",
                    _ => "failed",
                },
                title: result.title.clone(),
                message: result.message.clone(),
                detail: result.detail.clone(),
            },
            None => Operation::Running {
                operation: self.kind(),
                archive: self.archive.clone(),
                running_names: vec![],
                target_name: self.target(),
                progress: 0,
                indeterminate: Some(true),
                // Recovery removes this operation's own output; it is short and
                // is not interrupted part-way.
                can_cancel: Some(false),
                phases: vec![Phase {
                    title: if self.cancelled {
                        "Finishing cancellation"
                    } else {
                        "Resuming interrupted operation"
                    }
                    .into(),
                    detail: "Checking saved progress before continuing.".into(),
                    tone: "running",
                }],
            },
        }
    }
}
fn journal_path(history: &Path) -> PathBuf {
    history.with_file_name("backup-operation.json")
}
fn write(history: &Path, journal: &Journal) -> Result<(), String> {
    let path = journal_path(history);
    let parent = path
        .parent()
        .ok_or("Missing operation storage directory.")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    serde_json::to_writer(&mut file, journal).map_err(|e| e.to_string())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    file.persist(&path).map_err(|e| e.to_string())?;
    fs::File::open(parent)
        .and_then(|dir| dir.sync_all())
        .map_err(|e| e.to_string())
}
/// Why [`try_load`] found no journal to use. The kind decides what may be done with the
/// file: only one that was read can be set aside (see [`set_aside`]).
#[derive(Debug)]
pub(super) enum LoadFailure {
    /// The file could not be opened or read (an `io::Error`: permission denied, a failing
    /// disk, a folder in its place). Nothing is known about what it holds, so it is never
    /// set aside; the failure may pass, and the next launch reads it again.
    Io(String),
    /// The file was read but is no journal this version can use: damaged or empty, not a
    /// journal, with a field it does not know, or written by a version it does not support.
    /// Nothing can settle it, so it may be set aside.
    Unusable(String),
}

impl LoadFailure {
    pub(super) fn into_message(self) -> String {
        match self {
            Self::Io(message) | Self::Unusable(message) => message,
        }
    }
}

/// [`load`], with the reason it failed as a type rather than a message.
pub(super) fn try_load(history: &Path) -> Result<Option<Journal>, LoadFailure> {
    let bytes = match fs::read(journal_path(history)) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(LoadFailure::Io(format!(
                "Silo could not read the interrupted export or import: {e}"
            )))
        }
    };
    let journal: Journal = serde_json::from_slice(&bytes).map_err(|e| {
        LoadFailure::Unusable(format!("Silo could not read the interrupted export or import. The saved file was preserved: {e}"))
    })?;
    validate(&journal).map_err(LoadFailure::Unusable)?;
    Ok(Some(journal))
}

/// Returns a terminal operation already committed in the domain journal. The
/// socket host calls this before serving job get/ack requests after restart.
pub(super) fn retained_terminal_operation(
    history: &Path,
) -> Result<Option<(String, Value)>, String> {
    let Some(journal) = try_load(history).map_err(LoadFailure::into_message)? else {
        return Ok(None);
    };
    if journal.terminal.is_none() {
        return Ok(None);
    }
    let operation = serde_json::to_value(journal.operation()).map_err(|error| error.to_string())?;
    Ok(Some((journal.identity().to_owned(), operation)))
}

/// [`try_load`] with its failure as a message, for tests that only read what was saved.
#[cfg(test)]
pub(super) fn load(history: &Path) -> Result<Option<Journal>, String> {
    try_load(history).map_err(LoadFailure::into_message)
}
/// How the export and import journal in `app_data` bears on the storage migration.
pub(crate) enum JournalState {
    /// There is none, or it recorded its result, or it only waits for the upgrade to
    /// finish its cleanup: the migration may proceed and leaves it in place.
    Settled,
    /// An interrupted operation no launch has settled yet.
    Pending,
    /// It was read but cannot be used, or a version this one does not support wrote it.
    /// Nothing can settle it, so the migration sets it aside instead of waiting for it.
    Unreadable,
    /// The file could not be read at all (an I/O error), so nothing is known about it. It
    /// is neither waited for nor set aside: the migration refuses to start until a launch
    /// can read it.
    Unavailable,
}

pub(crate) fn journal_state(app_data: &Path) -> JournalState {
    match try_load(&app_data.join(HISTORY_FILE)) {
        Ok(None) => JournalState::Settled,
        Ok(Some(journal)) if !journal.is_pending() || journal.is_awaiting_upgrade() => {
            JournalState::Settled
        }
        Ok(Some(_)) => JournalState::Pending,
        Err(LoadFailure::Unusable(_)) => JournalState::Unreadable,
        Err(LoadFailure::Io(_)) => JournalState::Unavailable,
    }
}

/// Set the journal in `app_data` aside as `backup-operation.unreadable-<UTC date>.json`
/// and return where it went. Only for a journal [`try_load`] read and found unusable
/// ([`LoadFailure::Unusable`], [`JournalState::Unreadable`]), never for one it could not
/// read at all. The file is only renamed, never read, changed or deleted, so
/// it stays for diagnosis. A result in its place tells the user an export or import
/// record was set aside and may need to be run again; failing to save that result never
/// fails the setting aside. The result is marked unseen: nothing the user did led to it.
pub(crate) fn set_aside_unreadable_journal(app_data: &Path) -> Result<PathBuf, String> {
    set_aside(
        app_data,
        "If an export or import was running before the upgrade, run it again.",
    )
    .map(|(aside, _)| aside)
}

/// Outside a migration, set aside a journal that [`try_load`] found unusable, as the migration does
/// (see [`set_aside_unreadable_journal`]), so exports and imports are available again
/// instead of staying unavailable until the file is removed by hand. Returns the notice
/// that took its place, which is the journal to start with even when saving it failed.
pub(super) fn set_aside_at_startup(history: &Path) -> Result<Journal, String> {
    let app_data = history
        .parent()
        .ok_or("Missing operation storage directory.")?;
    set_aside(
        app_data,
        "If an export or import was running, run it again.",
    )
    .map(|(_, notice)| notice)
}

fn set_aside(app_data: &Path, detail: &str) -> Result<(PathBuf, Journal), String> {
    let journal = journal_path(&app_data.join(HISTORY_FILE));
    let today = time::OffsetDateTime::now_utc();
    let date = format!(
        "{:04}-{:02}-{:02}",
        today.year(),
        today.month() as u8,
        today.day()
    );
    let aside = (1_u32..)
        .map(|attempt| match attempt {
            1 => app_data.join(format!("backup-operation.unreadable-{date}.json")),
            _ => app_data.join(format!("backup-operation.unreadable-{date}-{attempt}.json")),
        })
        .find(|path| {
            matches!(fs::symlink_metadata(path), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
        })
        .ok_or("The unreadable export or import record could not be set aside.")?;
    fs::rename(&journal, &aside)
        .and_then(|()| fs::File::open(app_data).and_then(|directory| directory.sync_all()))
        .map_err(|_| "The unreadable export or import record could not be set aside.")?;
    let notice = Journal {
        version: 1,
        id: uuid::Uuid::new_v4().to_string(),
        archive: Archive {
            name: "Export or import record".into(),
            archive_path: aside.to_string_lossy().into_owned(),
            completed_label: "Set aside".into(),
            size: "Unknown".into(),
            destination: app_data.to_string_lossy().into_owned(),
            computers: Vec::new(),
            checkpoint_name: None,
        },
        request: Request::Backup {
            names: Vec::new(),
            computers: Vec::new(),
            running: Vec::new(),
            checkpoint_id: None,
            pending_capture: None,
        },
        cancelled: false,
        terminal: Some(Terminal {
            outcome: "failed".into(),
            title: "Export or import record set aside".into(),
            message: "An export or import record couldn\u{2019}t be read and was set aside.".into(),
            detail: Some(detail.into()),
            running: Vec::new(),
        }),
        awaiting_upgrade: false,
        unseen: true,
    };
    if let Err(error) = write(&app_data.join(HISTORY_FILE), &notice) {
        eprintln!("Silo could not record that an export or import record was set aside: {error}");
    }
    Ok((aside, notice))
}

/// The rules a saved journal must meet to be loaded. `begin` applies them too,
/// so Silo never saves a journal the next launch would refuse (E-51).
fn validate(journal: &Journal) -> Result<(), String> {
    uuid::Uuid::parse_str(&journal.id).map_err(|_| "Invalid saved export operation identity.")?;
    if journal.version != 1 || !Path::new(&journal.archive.archive_path).is_absolute() {
        return Err("Unsupported saved export or import. The file was preserved.".into());
    }
    match &journal.request {
        Request::Backup {
            names,
            computers,
            running,
            pending_capture,
            ..
        } => {
            for name in names
                .iter()
                .chain(computers.iter().map(|(name, _)| name))
                .chain(running.iter().map(|(name, _)| name))
            {
                runtime::validate_name(name).map_err(|e| e.to_string())?;
            }
            if let Some(capture) = pending_capture {
                validate_export_capture(capture)?;
            }
        }
        Request::Restore {
            name,
            source,
            id,
            group,
            ..
        } => {
            runtime::validate_name(name).map_err(|e| e.to_string())?;
            if let Some(source) = source {
                runtime::validate_name(source).map_err(|e| e.to_string())?;
            }
            if let Some(id) = id {
                uuid::Uuid::parse_str(id).map_err(|e| e.to_string())?;
            }
            if let Some(group) = group {
                validate_import_group(group)?;
            }
        }
    }
    Ok(())
}
/// The exact `silo-import-<32 hex>` form `BackupService::prepare_restore` creates.
fn validate_import_group(group: &str) -> Result<(), String> {
    let suffix = group.strip_prefix("silo-import-").unwrap_or_default();
    if suffix.len() == 32
        && suffix
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err("Invalid saved import checkpoint group.".into())
    }
}
fn validate_export_capture(capture: &ExportCapture) -> Result<(), String> {
    uuid::Uuid::parse_str(&capture.computer_id).map_err(|_| "Invalid export capture identity.")?;
    runtime::validate_name(&capture.group)
        .map_err(|error| error.to_string())
        .or_else(|_| validate_import_group(&capture.group))?;
    let parts: Vec<_> = capture
        .member
        .strip_prefix("silo-backup-")
        .unwrap_or_default()
        .split('-')
        .collect();
    if parts.len() != 3
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err("Invalid saved export capture member.".into());
    }
    Ok(())
}

/// Save intent in the existing operation journal before an export capture writes
/// native data. A verified complete capture clears that intent.
pub(super) fn export_capture_intent(
    controller: &Controller,
    source: &backup::BackupSource,
    member: Option<&str>,
) -> Result<(), backup::BackupError> {
    let capture = member.map(|member| ExportCapture {
        computer_id: source.computer_configuration["id"]
            .as_str()
            .unwrap_or_default()
            .into(),
        group: source.snapshot_group.clone(),
        member: member.into(),
    });
    if let Some(capture) = &capture {
        validate_export_capture(capture).map_err(backup::BackupError::InvalidRequest)?;
    }
    update(controller, |journal| {
        if let Request::Backup {
            pending_capture, ..
        } = &mut journal.request
        {
            *pending_capture = capture;
        }
    })
    .map_err(backup::BackupError::InvalidRequest)
}

pub(super) fn settle_export_capture(
    runner: &dyn runtime::RuntimeRunner,
    paths: &runtime::RuntimePaths,
    controller: &Controller,
) -> Result<(), String> {
    let capture = controller
        .journal
        .lock()
        .map_err(|_| "Saved operation unavailable.")?
        .as_ref()
        .and_then(|journal| match &journal.request {
            Request::Backup {
                pending_capture, ..
            } => pending_capture.clone(),
            _ => None,
        });
    let Some(capture) = capture else {
        return Ok(());
    };
    if !runtime::checkpoints::discard_failed_capture(
        runner,
        paths,
        &capture.computer_id,
        (capture.group.clone(), capture.member.clone()),
    ) && !controller
        .service
        .export_capture_ready(&capture.group, &capture.member)
        .map_err(|error| error.to_string())?
    {
        return Err(format!("The incomplete export capture {}:{} could not be removed. Its cleanup progress was kept.", capture.group, capture.member));
    }
    update(controller, |journal| {
        if let Request::Backup {
            pending_capture, ..
        } = &mut journal.request
        {
            *pending_capture = None;
        }
    })
}

pub(super) fn begin(controller: &Controller, journal: Journal) -> Result<(), String> {
    if let Some(error) = &controller
        .view
        .lock()
        .map_err(|_| "Export and import status could not be read. Relaunch Silo and retry.")?
        .journal_error
    {
        return Err(error.clone());
    }
    let mut saved = controller
        .journal
        .lock()
        .map_err(|_| "Saved operation unavailable.")?;
    if saved.as_ref().is_some_and(|j| j.terminal.is_none()) {
        return Err(
            "An interrupted operation still needs to finish. Relaunch Silo to resume it.".into(),
        );
    }
    validate(&journal)?;
    write(&controller.history_path, &journal)?;
    *saved = Some(journal);
    Ok(())
}
fn update(controller: &Controller, change: impl FnOnce(&mut Journal)) -> Result<(), String> {
    let mut saved = controller
        .journal
        .lock()
        .map_err(|_| "Saved operation unavailable.")?;
    if let Some(journal) = saved.as_ref() {
        let mut next = journal.clone();
        change(&mut next);
        write(&controller.history_path, &next)?;
        *saved = Some(next);
    }
    Ok(())
}
pub(super) fn unresolved(controller: &Controller) -> Result<bool, String> {
    Ok(!controller.busy.load(Ordering::Acquire) && pending(controller)?)
}
/// Whether the saved journal still describes an unfinished operation,
/// regardless of whether this process currently holds the worker slot.
pub(super) fn pending(controller: &Controller) -> Result<bool, String> {
    Ok(controller
        .journal
        .lock()
        .map_err(|_| "Saved operation unavailable.")?
        .as_ref()
        .is_some_and(Journal::is_pending))
}
pub(super) fn snapshot(controller: &Controller) -> Result<Option<Journal>, String> {
    Ok(controller
        .journal
        .lock()
        .map_err(|_| "Saved operation unavailable.")?
        .clone())
}
pub(super) fn token(controller: &Controller) -> Result<Option<String>, String> {
    Ok(controller
        .journal
        .lock()
        .map_err(|_| "Saved operation unavailable.")?
        .as_ref()
        .map(|j| j.id.clone()))
}
/// Whether the journal holds a result the user has not been shown yet.
#[cfg(test)]
pub(super) fn unseen(controller: &Controller) -> Result<bool, String> {
    Ok(controller
        .journal
        .lock()
        .map_err(|_| "Saved operation unavailable.")?
        .as_ref()
        .is_some_and(Journal::is_unseen_result))
}
/// Record that the user was shown the result the journal holds, when it is the one with
/// the identity `expected`: nothing else is touched, and a result that was replaced or
/// dismissed in the meantime stays as it is. The result itself stays until it is
/// dismissed, but no longer counts as unseen. Returns whether anything changed.
pub(super) fn acknowledge(controller: &Controller, expected: &str) -> Result<bool, String> {
    let mut saved = controller
        .journal
        .lock()
        .map_err(|_| "Saved operation unavailable.")?;
    let Some(journal) = saved
        .as_ref()
        .filter(|journal| journal.is_unseen_result() && journal.id == expected)
    else {
        return Ok(false);
    };
    let mut next = journal.clone();
    next.unseen = false;
    write(&controller.history_path, &next)?;
    *saved = Some(next);
    Ok(true)
}
fn cleanup_archive_partial(journal: &Journal) -> Result<(), String> {
    if !matches!(journal.request, Request::Backup { .. }) {
        return Ok(());
    }
    let parent = Path::new(&journal.archive.archive_path)
        .parent()
        .ok_or("Missing export destination.")?;
    let prefix = format!(".silo-backup-{}-", journal.id);
    let entries = match fs::read_dir(parent) {
        Ok(entries) => entries,
        // An unplugged or renamed destination holds no partial file that this
        // process could reach; failing here would block recovery until the
        // drive returns (E-36).
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        if !entry.file_name().to_string_lossy().starts_with(&prefix) {
            continue;
        }
        // A mixed export also stages whole archives in a folder with this prefix.
        let file_type = entry.file_type().map_err(|e| e.to_string())?;
        if file_type.is_file() {
            fs::remove_file(entry.path()).map_err(|e| e.to_string())?;
        } else if file_type.is_dir() && !file_type.is_symlink() {
            fs::remove_dir_all(entry.path()).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Removes the capture folders an interrupted macOS export left under the macOS computers
/// folder. They carry this operation's id, so no other operation's captures are touched.
fn cleanup_macos_export_captures(controller: &Controller, journal: &Journal) -> Result<(), String> {
    if !matches!(journal.request, Request::Backup { .. }) {
        return Ok(());
    }
    let Ok(host) = controller.runtime_host() else {
        return Ok(());
    };
    let context = host.engine_context();
    crate::macos_computers::remove_export_captures(&context.paths().app_data, &journal.id)
}

/// Own the new group and its suffix as the deterministic `--stage-id` before
/// `snapshot load` writes any native data. The new
/// computer's identity is added later, before its checkpoint record is saved.
pub(super) fn save_restore_group(
    controller: &Controller,
    import_group: &str,
) -> Result<(), String> {
    validate_import_group(import_group)?;
    update(controller, |journal| {
        if let Request::Restore { group, .. } = &mut journal.request {
            *group = Some(import_group.into());
        }
    })
}

/// Journal the new computer's id and the loaded snapshot group before the
/// import writes its checkpoint record and settings (E-24).
pub(super) fn save_restore_identity(
    controller: &Controller,
    identity: &str,
    import_group: &str,
) -> Result<(), String> {
    uuid::Uuid::parse_str(identity).map_err(|_| "Invalid imported computer identity.")?;
    validate_import_group(import_group)?;
    update(controller, |journal| {
        if let Request::Restore { id, group, .. } = &mut journal.request {
            *id = Some(identity.into());
            *group = Some(import_group.into());
        }
    })
}

/// Journal the new macOS identity and selected archive source before extraction
/// or rekeying touches the disposable staging directory.
pub(super) fn save_macos_restore_identity(
    controller: &Controller,
    identity: &str,
    source: Option<&str>,
) -> Result<(), String> {
    uuid::Uuid::parse_str(identity).map_err(|_| "Invalid imported computer identity.")?;
    if let Some(source) = source {
        runtime::validate_name(source).map_err(|error| error.to_string())?;
    }
    update(controller, |journal| {
        if let Request::Restore {
            source: journal_source,
            id,
            group,
            macos,
            ..
        } = &mut journal.request
        {
            *journal_source = source.map(Into::into);
            *id = Some(identity.into());
            *group = None;
            *macos = true;
        }
    })
}
pub(super) fn clear_restore_identity(controller: &Controller) -> Result<(), String> {
    update(controller, |journal| {
        if let Request::Restore {
            id, group, macos, ..
        } = &mut journal.request
        {
            *id = None;
            *group = None;
            *macos = false;
        }
    })
}
/// Remove what an import wrote before its settings were saved: the new
/// computer's checkpoint record and the loaded snapshot group (E-23, E-24).
/// The identity stays journaled until both are gone, so a failure here is
/// retried on the next launch.
pub(super) fn discard_uncommitted_import(
    paths: &runtime::RuntimePaths,
    controller: &Controller,
    identity: &str,
    import_group: Option<&str>,
) -> Result<(), String> {
    runtime::checkpoints::forget_removed(paths, identity).map_err(|e| e.to_string())?;
    if let Some(group) = import_group {
        controller
            .service
            .discard_import_group(group)
            .map_err(|error| {
                format!("Silo could not remove the unfinished import {group}: {error}")
            })?;
    }
    clear_restore_identity(controller)
}
/// Includes a load interrupted before Silo allocated the computer identity.
/// Only the journaled group and its two deterministic stage roots are removed.
/// Unrelated stages, including older random stages, stay untouched at relaunch.
pub(super) fn discard_pending_import(
    paths: &runtime::RuntimePaths,
    controller: &Controller,
) -> Result<(), String> {
    let saved = controller
        .journal
        .lock()
        .map_err(|_| "Saved operation unavailable.")?
        .as_ref()
        .and_then(|journal| match &journal.request {
            Request::Restore { id, group, .. } => Some((id.clone(), group.clone())),
            _ => None,
        });
    let Some((id, group)) = saved else {
        return Ok(());
    };
    if let Some(id) = id {
        return discard_uncommitted_import(paths, controller, &id, group.as_deref());
    }
    if let Some(group) = group {
        controller
            .service
            .discard_import_group(&group)
            .map_err(|error| {
                format!("Silo could not remove the unfinished import {group}: {error}")
            })?;
        clear_restore_identity(controller)?;
    }
    Ok(())
}

pub(super) fn cancel(controller: &Controller, expected: &str) -> Result<(), String> {
    let mut saved = controller
        .journal
        .lock()
        .map_err(|_| "Saved operation unavailable.")?;
    let Some(journal) = saved
        .as_ref()
        .filter(|journal| journal.identity() == expected)
    else {
        return Ok(());
    };
    let mut next = journal.clone();
    next.cancelled = true;
    write(&controller.history_path, &next)?;
    *saved = Some(next);
    Ok(())
}
/// Stop retrying an interrupted operation whose recovery failed. The journal
/// becomes a terminal failure so it can be dismissed; no files are removed.
pub(super) fn abandon(controller: &Controller) -> Result<(), String> {
    update(controller, |j| {
        if j.terminal.is_none() {
            j.terminal = Some(Terminal {
                outcome: "failed".into(),
                title: "Interrupted operation abandoned".into(),
                message: "Silo stopped retrying this interrupted export or import. Files it left were kept.".into(),
                detail: None,
                running: vec![],
            });
        }
    })
}
pub(super) fn dismiss(controller: &Controller) -> Result<(), String> {
    let mut saved = controller
        .journal
        .lock()
        .map_err(|_| "Saved operation unavailable.")?;
    if saved.as_ref().is_some_and(|j| j.terminal.is_none()) {
        return Ok(());
    }
    let path = journal_path(&controller.history_path);
    match fs::remove_file(&path) {
        Ok(()) => fs::File::open(path.parent().unwrap())
            .and_then(|dir| dir.sync_all())
            .map_err(|e| e.to_string())?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(format!(
                "Could not dismiss the saved export or import result: {e}"
            ))
        }
    }
    *saved = None;
    Ok(())
}

pub(super) fn complete(controller: &Controller, operation: Operation) -> Operation {
    complete_marked(controller, operation, false)
}

/// Record the result of an operation, marking it unseen when `unseen`: the result of an
/// operation the first launch of an upgrade settled. The result of one that waited for the
/// upgrade is unseen too, whatever `unseen` says.
pub(super) fn complete_marked(
    controller: &Controller,
    mut operation: Operation,
    unseen: bool,
) -> Operation {
    let cleanup_pending = controller
        .journal
        .lock()
        .ok()
        .and_then(|journal| {
            journal.as_ref().map(|j| {
                matches!(
                    &j.request,
                    Request::Restore { id: Some(_), .. }
                        | Request::Restore { group: Some(_), .. }
                        | Request::Backup {
                            pending_capture: Some(_),
                            ..
                        }
                )
            })
        })
        .unwrap_or(false);
    if cleanup_pending {
        if let Operation::Result {
            outcome, message, ..
        } = &mut operation
        {
            if *outcome != "success" {
                message
                    .push_str(" Saved progress was preserved. Relaunch Silo to finish recovery.");
                return operation;
            }
        }
    }
    if let Operation::Result {
        archive,
        outcome,
        title,
        message,
        detail,
        running_names,
        ..
    } = &operation
    {
        if let Err(error) = update(controller, |j| {
            j.archive = archive.clone();
            // Whatever waited for the upgrade has been settled or is reported now, and
            // the user has not been told yet.
            j.unseen = unseen || j.awaiting_upgrade;
            j.awaiting_upgrade = false;
            j.terminal = Some(Terminal {
                outcome: (*outcome).into(),
                title: title.clone(),
                message: message.clone(),
                detail: detail.clone(),
                running: running_names.clone(),
            });
        }) {
            if let Operation::Result {
                outcome,
                title,
                message,
                ..
            } = &mut operation
            {
                *outcome = "failed";
                *title = "Could not save the operation result".into();
                *message = format!("{error} Silo will check the output again on next launch.");
            }
        }
    }
    operation
}
pub(super) fn resume(
    controller: Arc<Controller>,
    journal: Journal,
    runtime: &tokio::runtime::Handle,
) -> Result<(), String> {
    set_operation(&controller, journal.operation())?;
    if journal.terminal.is_some() {
        return Ok(());
    }
    controller.busy.store(true, Ordering::Release);
    let cancellation = backup::Cancellation::default();
    controller
        .view
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .cancellation = Some(cancellation.clone());
    let (kind, archive, target) = (journal.kind(), journal.archive.clone(), journal.target());
    let outer_controller = controller.clone();
    let work = move || {
        // Decided once: it picks both how the operation is settled and whether its result
        // is marked as one the user has not been shown (the upgrade produced it).
        let before_upgrade = controller
            .runtime_host()
            .is_ok_and(|host| host.blocks_operations());
        let operation = match recover(&controller, &journal, &cancellation, before_upgrade) {
            Ok(settlement) => finish_settlement(&controller, settlement, before_upgrade),
            // Retain the journal: never discard ownership after an uncertain
            // cleanup. The next launch retries; dismissing abandons it (E-43).
            Err(error) => Some(Operation::Result {
                operation: journal.kind(),
                archive: journal.archive.clone(),
                target_name: journal.target(),
                running_names: vec![],
                outcome: "failed",
                title: "Could not resume the interrupted operation".into(),
                message: error,
                detail: Some(
                    "Saved progress was preserved. Relaunch Silo to retry, or dismiss this to stop retrying. Dismissing keeps any files it left."
                        .into(),
                ),
            }),
        };
        match operation {
            Some(operation) => {
                let _ = set_operation(&controller, operation);
            }
            None => {
                controller
                    .view
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .operation = None;
            }
        }
        super::publish_and_finish(&controller, || publish(&controller));
    };
    runtime.spawn_blocking(move || {
        super::contain_worker_panic(&outer_controller, kind, &archive, target, work);
    });
    Ok(())
}
/// What to report once an interrupted operation was settled, recording it. `before_upgrade`
/// is whether it was settled before the storage migration: such a result is one the user
/// has not seen when the migration is over.
pub(super) fn finish_settlement(
    controller: &Controller,
    settlement: Settlement,
    before_upgrade: bool,
) -> Option<Operation> {
    match settlement {
        Settlement::Settled(operation) => {
            Some(complete_marked(controller, operation, before_upgrade))
        }
        // Nothing is running and nothing is reported yet: the upgrade converts the
        // storage first, and the relaunch that follows finishes the cleanup and
        // reports it.
        Settlement::AfterUpgrade => None,
    }
}

fn recover(
    controller: &Controller,
    journal: &Journal,
    cancellation: &backup::Cancellation,
    before_upgrade: bool,
) -> Result<Settlement, String> {
    // The storage migration refuses to start while a journal is pending (E-50), so an
    // interrupted operation settles before it. Until the migration finishes, the
    // previous generation is a pre-upgrade backup: nothing may start `msb` against it
    // or write to it, so the operation is settled without the runtime, and whatever
    // only the runtime can clean up waits for the converted storage. After the
    // migration, this is the ordinary recovery in the storage in use.
    let host = controller.runtime_host()?;
    if matches!(&journal.request, Request::Restore { macos: true, .. }) {
        return recover_macos_archive(controller, journal).map(Settlement::Settled);
    }
    if before_upgrade {
        let paths = runtime::inert_runtime_paths(host)?;
        return settle_before_migration(&paths, controller, journal, cancellation);
    }
    recover_at_paths(
        &runtime::runtime_paths(host)?,
        controller,
        journal,
        cancellation,
    )
    .map(Settlement::Settled)
}

fn recover_macos_archive(controller: &Controller, journal: &Journal) -> Result<Operation, String> {
    let _guard = runtime::OPERATIONS
        .device("Recovering macOS archive import")
        .map_err(|_| "Computer operations unavailable.")?;
    controller
        .service
        .cleanup_interrupted_staging()
        .map_err(|error| format!("Could not clear interrupted working files: {error}"))?;
    cleanup_archive_partial(journal)?;
    let host = controller.runtime_host()?;
    let context = host
        .macos_context()
        .ok_or("macOS archive import recovery needs the Apple Virtualization host.")?;
    let id = match &journal.request {
        Request::Restore {
            macos: true,
            id: Some(id),
            ..
        } => Some(id.as_str()),
        _ => None,
    };
    let committed = match id {
        Some(id) => crate::macos_computers::recover_archive_import(&context, id)?,
        None => false,
    };
    Ok(if committed {
        settled(
            journal,
            journal.archive.clone(),
            "success",
            "Import complete",
            "Silo verified this macOS import after relaunching.",
            None,
        )
    } else if journal.cancelled {
        settled(
            journal,
            journal.archive.clone(),
            "cancelled",
            "Import cancelled",
            "The import was cancelled.",
            Some("No computer was added."),
        )
    } else {
        settled(
            journal,
            journal.archive.clone(),
            "failed",
            "Import interrupted",
            "Silo closed before this macOS import finished.",
            Some("No computer was added. Import the file again."),
        )
    })
}

/// The result of an interrupted operation. `archive` is the file the result names.
fn settled(
    journal: &Journal,
    archive: Archive,
    outcome: &'static str,
    title: &str,
    message: &str,
    detail: Option<&str>,
) -> Operation {
    Operation::Result {
        operation: journal.kind(),
        archive,
        running_names: vec![],
        target_name: journal.target(),
        outcome,
        title: title.into(),
        message: message.into(),
        detail: detail.map(Into::into),
    }
}

/// The file an interrupted export left at its destination, if any. It appears only
/// after it was written and verified, so it is kept whatever it holds; a file that
/// verifies again as this export is reported as complete.
fn left_export(
    journal: &Journal,
    names: &[String],
    controller: &Controller,
    cancellation: &backup::Cancellation,
    interrupted: &str,
) -> Option<Operation> {
    let path = Path::new(&journal.archive.archive_path);
    if !path.exists() {
        return None;
    }
    Some(match controller.service.inspect_archive(path, cancellation) {
        Ok(checked) if checked.computers == *names => {
            let archive = Archive {
                checkpoint_name: journal.archive.checkpoint_name.clone(),
                ..archive_from(path, &checked)
            };
            settled(journal, archive, "success", "Export complete", "Silo verified this export after relaunching.", None)
        }
        _ => settled(
            journal,
            journal.archive.clone(),
            "failed",
            interrupted,
            "Silo closed before this export finished. The file it left could not be verified and was kept.",
            Some("Export the computer again."),
        ),
    })
}

/// What settling an operation interrupted before the storage migration left to do.
pub(super) enum Settlement {
    /// Nothing is left: this is the result to report.
    Settled(Operation),
    /// Data only the runtime can remove is left. The journal keeps owning it until the
    /// migration has converted the storage, where recovery removes it and reports.
    AfterUpgrade,
}

/// Settle an operation interrupted before the storage migration, without the runtime
/// and without writing to the previous generation: until the migration finishes it is
/// a pre-upgrade backup, which the migration copies and never edits. What the
/// operation left outside that folder is handled as in [`recover_at_paths`]: its
/// working files and partial export file are removed, and a finished export file is
/// kept. What only the runtime can clean up (an export capture, a loaded import
/// group, the computer of an unfinished import) is not given up: the migration copies it
/// into the converted storage with everything else, so the journal stays pending,
/// marked as waiting for the upgrade, and recovery removes it from that copy once the
/// converted storage is selected (see [`Settlement::AfterUpgrade`]). The migration
/// converts only the computers saved in the settings, so an import that never saved its
/// computer is never converted.
pub(super) fn settle_before_migration(
    paths: &runtime::RuntimePaths,
    controller: &Controller,
    journal: &Journal,
    cancellation: &backup::Cancellation,
) -> Result<Settlement, String> {
    if matches!(&journal.request, Request::Restore { macos: true, .. }) {
        return recover_macos_archive(controller, journal).map(Settlement::Settled);
    }
    let _guard = runtime::OPERATIONS
        .device("Recovering export or import")
        .map_err(|_| "Computer operations unavailable.")?;
    // A snapshot command from the previous process can outlive it while writing
    // into the working folder. Wait for it without creating the lock file, which
    // would add a file to the previous generation.
    let command = backup::wait_for_interrupted_command_in_place(
        paths.storage_home.as_deref().unwrap_or(&paths.home),
        Duration::from_secs(60),
    )
    .map_err(|e| e.to_string())?;
    // Files outside the previous generation. Leaving some behind harms nothing, so a
    // failure here never keeps the migration waiting; the result says so.
    let staging = controller.service.cleanup_interrupted_staging();
    let partial = cleanup_archive_partial(journal)
        .and_then(|()| cleanup_macos_export_captures(controller, journal));
    drop(command);
    let archive = journal.archive.clone();
    if let Request::Restore { id: Some(id), .. } = &journal.request {
        // Saved settings are the import's commit point: a computer saved under the
        // journaled identity is complete and is converted like any other.
        let saved = runtime::read_metadata(&paths.metadata)
            .map_err(|e| e.to_string())?
            .computers
            .iter()
            .any(|configuration| configuration.id() == id);
        if saved {
            let message = "Silo verified this import after relaunching.";
            return Ok(Settlement::Settled(settled(
                journal,
                archive,
                "success",
                "Import complete",
                message,
                None,
            )));
        }
    }
    let needs_runtime = match &journal.request {
        Request::Backup {
            pending_capture, ..
        } => pending_capture.is_some(),
        Request::Restore { id, group, .. } => id.is_some() || group.is_some(),
    };
    if needs_runtime {
        // Durable before the migration starts: the migration leaves a journal marked
        // like this in place, and the next launch finishes it.
        update(controller, |journal| journal.awaiting_upgrade = true)?;
        return Ok(Settlement::AfterUpgrade);
    }
    let result = match &journal.request {
        Request::Backup { names, .. } => left_export(
            journal,
            names,
            controller,
            cancellation,
            "Export interrupted before the upgrade",
        )
        .unwrap_or_else(|| {
            let (outcome, title, message, again) = if journal.cancelled {
                (
                    "cancelled",
                    "Export cancelled",
                    "The export was cancelled.",
                    "",
                )
            } else {
                (
                    "failed",
                    "Export interrupted before the upgrade",
                    "Silo closed before this export finished.",
                    " Export the computer again.",
                )
            };
            let files = if staging.is_err() || partial.is_err() {
                " Some files it left in the export folder could not be removed."
            } else {
                ""
            };
            let detail = format!("No export file was saved.{files}{again}");
            settled(journal, archive, outcome, title, message, Some(&detail))
        }),
        Request::Restore { .. } => {
            let (outcome, title, message, again) = if journal.cancelled {
                (
                    "cancelled",
                    "Import cancelled",
                    "The import was cancelled.",
                    "",
                )
            } else {
                (
                    "failed",
                    "Import interrupted before the upgrade",
                    "Silo closed before this import finished.",
                    " Import the file again.",
                )
            };
            let detail = format!("No computer was added.{again}");
            settled(journal, archive, outcome, title, message, Some(&detail))
        }
    };
    Ok(Settlement::Settled(result))
}

/// The marker a released Silo (0.9.0 and earlier) wrote into the managed disk folder it
/// claimed for an import. It holds the identity journaled for the new computer.
const RELEASED_DISK_MARKER: &str = ".silo-restore-owner";

/// Remove what an import by a released Silo left when it stopped after journaling the
/// new computer's identity and before saving the computer: the managed disk folder it
/// claimed in `volumes/<name>`, and the runtime's computer it created over that disk.
/// Only what carries the import's own identity is removed: the folder must hold the
/// marker with it, and the runtime's computer must be Silo's own with it. A folder
/// without it is left alone unless it is empty, and nothing is removed when a saved
/// computer uses the name. The current import never leaves these: it records a native
/// snapshot group in the journal instead, which [`discard_uncommitted_import`] removes.
///
/// A released import leaves its computer `Created`, never started. The bundled runtime
/// removes such a computer like a stopped one (the `remove-created` patch), without running
/// guest code, so the computer and its disk both go and the name is free again.
fn discard_released_import(
    runner: &dyn runtime::RuntimeRunner,
    paths: &runtime::RuntimePaths,
    metadata: &runtime::ComputerConfigurationRequest,
    name: &str,
    identity: &str,
) -> Result<(), String> {
    if metadata
        .computers
        .iter()
        .any(|configuration| configuration.name() == name)
    {
        return Ok(());
    }
    let folder = paths.volumes.join(name);
    match fs::symlink_metadata(&folder) {
        Ok(found) if found.is_dir() => {}
        Ok(_) => {
            return Err(format!(
                "The disk storage for {name} is not a folder. No files were removed."
            ))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(format!(
                "Silo could not inspect the incomplete import's disk storage: {error}"
            ))
        }
    }
    let owner = fs::read_to_string(folder.join(RELEASED_DISK_MARKER)).ok();
    if owner.as_deref() != Some(identity) {
        return fs::remove_dir(&folder).map_err(|_| {
            format!("Silo could not verify who owns the disk storage for {name}. No files were removed.")
        });
    }
    runtime::cleanup_failed_create(runner, paths, name, identity)
        .map_err(|error| error.to_string())?;
    fs::remove_dir_all(&folder).map_err(|error| {
        format!("Silo could not remove the incomplete import's disk storage: {error}")
    })
}

/// Settle an operation interrupted by a relaunch without repeating it (E-31):
/// remove this operation's partial output and return the result to report.
/// An error means cleanup is uncertain and the journal must be kept.
pub(super) fn recover_at_paths(
    paths: &runtime::RuntimePaths,
    controller: &Controller,
    journal: &Journal,
    cancellation: &backup::Cancellation,
) -> Result<Operation, String> {
    recover_at_paths_with(
        &runtime::ProcessRunner,
        paths,
        controller,
        journal,
        cancellation,
    )
}

/// [`recover_at_paths`] with the runner that removes the runtime's own records. The
/// backup service of `controller` removes native snapshot data through its own runner.
pub(super) fn recover_at_paths_with(
    runner: &dyn runtime::RuntimeRunner,
    paths: &runtime::RuntimePaths,
    controller: &Controller,
    journal: &Journal,
    cancellation: &backup::Cancellation,
) -> Result<Operation, String> {
    if matches!(&journal.request, Request::Restore { macos: true, .. }) {
        return recover_macos_archive(controller, journal);
    }
    let _guard = runtime::OPERATIONS
        .device("Recovering export or import")
        .map_err(|_| "Computer operations unavailable.")?;
    runtime::prepare_runtime_home(&paths.home, paths.storage_home.as_deref())
        .map_err(|e| e.to_string())?;
    // A snapshot command from the previous process can outlive it while
    // writing into the working folder; let it finish before cleaning up.
    let command = backup::wait_for_interrupted_command(&paths.home, Duration::from_secs(60))
        .map_err(|e| e.to_string())?;
    controller
        .service
        .cleanup_interrupted_staging()
        .map_err(|e| format!("Could not clear interrupted working files: {e}"))?;
    cleanup_archive_partial(journal)?;
    cleanup_macos_export_captures(controller, journal)?;
    // The previous process's command has finished. Release the lock so the
    // snapshot commands that remove an unfinished import can take it.
    drop(command);
    settle_export_capture(runner, paths, controller)?;
    let result = |archive: Archive,
                  outcome: &'static str,
                  title: &str,
                  message: &str,
                  detail: Option<&str>| {
        settled(journal, archive, outcome, title, message, detail)
    };
    match &journal.request {
        Request::Backup { names, .. } => {
            if let Some(operation) = left_export(
                journal,
                names,
                controller,
                cancellation,
                "Export interrupted",
            ) {
                return Ok(operation);
            }
            Ok(if journal.cancelled {
                result(
                    journal.archive.clone(),
                    "cancelled",
                    "Export cancelled",
                    "The export was cancelled.",
                    Some("No export file was saved."),
                )
            } else {
                result(
                    journal.archive.clone(),
                    "failed",
                    "Export interrupted",
                    "Silo closed before this export finished.",
                    Some("No export file was saved. Export the computer again."),
                )
            })
        }
        Request::Restore {
            name, id, group, ..
        } => {
            if let Some(id) = id {
                let metadata =
                    runtime::read_metadata(&paths.metadata).map_err(|e| e.to_string())?;
                // Saved settings are the import's commit point.
                if metadata
                    .computers
                    .iter()
                    .any(|configuration| configuration.id() == id)
                {
                    return Ok(result(
                        journal.archive.clone(),
                        "success",
                        "Import complete",
                        "Silo verified this import after relaunching.",
                        None,
                    ));
                }
                // An identity without a group is the journal of a released Silo.
                if group.is_none() {
                    discard_released_import(runner, paths, &metadata, name, id)?;
                }
                discard_uncommitted_import(paths, controller, id, group.as_deref())?;
            } else if group.is_some() {
                discard_pending_import(paths, controller)?;
            }
            Ok(if journal.cancelled {
                result(
                    journal.archive.clone(),
                    "cancelled",
                    "Import cancelled",
                    "The import was cancelled.",
                    Some("No computer was added."),
                )
            } else {
                result(
                    journal.archive.clone(),
                    "failed",
                    "Import interrupted",
                    "Silo closed before this import finished.",
                    Some("No computer was added. Import the file again."),
                )
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{
        completed_archive, controller_with_scripted_msb, history_controller, scripted_calls,
    };
    use super::*;

    #[test]
    fn a_migrated_export_journal_loads() {
        let migrated = crate::runtime_migration::vocabulary_tests::migrated_installation();
        let journal = load(&migrated.app_data.join(HISTORY_FILE))
            .unwrap()
            .unwrap();
        assert_eq!(journal.archive.computers, ["dev"]);
        match journal.request {
            Request::Backup {
                computers,
                pending_capture: Some(capture),
                ..
            } => {
                let id = crate::runtime_migration::vocabulary_tests::ID;
                assert_eq!(computers, [("dev".to_string(), id.to_string())]);
                assert_eq!(capture.computer_id, id);
            }
            _ => panic!("expected an export request"),
        }
    }

    /// Runtime paths in a temp dir whose `msb` does not exist: any runtime
    /// command recovery runs fails the test.
    fn temp_paths(directory: &Path) -> runtime::RuntimePaths {
        runtime::RuntimePaths {
            guest_image: directory.join("guest-image"),
            executable: directory.join("missing-msb"),
            home: directory.join("home"),
            storage_home: None,
            library: directory.join("library"),
            metadata: directory.join("computers.json"),
            volumes: directory.join("volumes"),
        }
    }

    fn export_to(directory: &Path) -> Archive {
        let mut archive = completed_archive();
        archive.destination = directory.join("exports").to_string_lossy().into_owned();
        archive.archive_path = directory
            .join("exports/dev-2026-09-29.silo-backup")
            .to_string_lossy()
            .into_owned();
        fs::create_dir_all(&archive.destination).unwrap();
        archive
    }

    fn result_of(operation: &Operation) -> (&'static str, String, String, Option<String>) {
        match operation {
            Operation::Result {
                outcome,
                title,
                message,
                detail,
                ..
            } => (*outcome, title.clone(), message.clone(), detail.clone()),
            Operation::Running { .. } => panic!("recovery must report a result"),
        }
    }

    fn save_computer(paths: &runtime::RuntimePaths, name: &str, id: &str) {
        let request: runtime::ComputerConfigurationRequest = serde_json::from_value(serde_json::json!({
            "schemaVersion": 1,
            "computers": [{"id":id,"name":name,"cpus":1,"maxCPUs":1,"memoryGiB":1,"maxMemoryGiB":1,"workspaceStorageGiB":1,"runtimeStorageGiB":1}]
        }))
        .unwrap();
        runtime::write_metadata(&paths.metadata, &request).unwrap();
    }

    const IMPORT_GROUP: &str = "silo-import-0123456789abcdef0123456789abcdef";
    const IMPORT_MEMBER: &str = "silo-backup-0-1-2";

    #[test]
    fn a_delayed_cancellation_cannot_mark_a_replacement_journal() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let controller = history_controller(directory.path().join("backup-history.json"));
        begin(
            &controller,
            Journal::backup(export_to(directory.path()), vec!["dev".into()], None),
        )
        .unwrap();
        let first_id = token(&controller).unwrap().unwrap();
        complete(&controller, result_operation("First export complete"));
        begin(
            &controller,
            Journal::backup(export_to(directory.path()), vec!["dev".into()], None),
        )
        .unwrap();
        let second_id = token(&controller).unwrap().unwrap();

        cancel(&controller, &first_id).unwrap();
        let saved = load(&controller.history_path).unwrap().unwrap();
        assert_eq!(saved.identity(), second_id);
        assert!(
            !saved.cancelled,
            "the first cancellation must not reach the second export"
        );
        assert!(
            !controller
                .journal
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .cancelled
        );

        cancel(&controller, &second_id).unwrap();
        assert!(load(&controller.history_path).unwrap().unwrap().cancelled);
    }

    #[test]
    fn relaunch_reports_an_interrupted_export_without_rerunning_it_or_starting_computers() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = temp_paths(directory.path());
        let controller = history_controller(directory.path().join("backup-history.json"));
        let mut journal = Journal::backup(export_to(directory.path()), vec!["dev".into()], None);
        // Journals from older builds list computers that were running when the
        // export began; relaunch used to start them again.
        let id = uuid::Uuid::new_v4().to_string();
        if let Request::Backup {
            computers, running, ..
        } = &mut journal.request
        {
            *computers = vec![("dev".into(), id.clone())];
            *running = vec![("dev".into(), id)];
        }
        let partial = Path::new(&journal.archive.destination)
            .join(format!(".silo-backup-{}-partial", journal.id));
        fs::write(&partial, b"incomplete").unwrap();
        begin(&controller, journal.clone()).unwrap();
        let recovered = recover_at_paths(
            &paths,
            &controller,
            &journal,
            &backup::Cancellation::default(),
        )
        .unwrap();
        let (outcome, title, _message, detail) = result_of(&recovered);
        assert_eq!((outcome, title.as_str()), ("failed", "Export interrupted"));
        assert!(detail.unwrap().contains("No export file was saved"));
        assert!(!partial.exists());
        // The result is final: completing it leaves nothing to resume.
        complete(&controller, recovered);
        assert!(!pending(&controller).unwrap());
    }

    #[test]
    fn relaunch_reports_a_cancelled_export_and_keeps_an_unverifiable_file() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = temp_paths(directory.path());
        let controller = history_controller(directory.path().join("backup-history.json"));
        let mut journal = Journal::backup(export_to(directory.path()), vec!["dev".into()], None);
        journal.cancelled = true;
        let recovered = recover_at_paths(
            &paths,
            &controller,
            &journal,
            &backup::Cancellation::default(),
        )
        .unwrap();
        assert_eq!(result_of(&recovered).0, "cancelled");

        fs::write(&journal.archive.archive_path, b"not an export").unwrap();
        let recovered = recover_at_paths(
            &paths,
            &controller,
            &journal,
            &backup::Cancellation::default(),
        )
        .unwrap();
        let (outcome, title, message, _) = result_of(&recovered);
        assert_eq!((outcome, title.as_str()), ("failed", "Export interrupted"));
        assert!(message.contains("could not be verified"), "{message}");
        assert_eq!(
            fs::read(&journal.archive.archive_path).unwrap(),
            b"not an export"
        );
    }

    #[test]
    fn recovery_settles_a_journal_from_before_a_runtime_migration_without_the_runtime() {
        let _test_state = crate::test_support::global_state();
        // While the runtime migration blocks computer operations, the saved
        // settings may be unreadable and msb unusable. Recovery must still
        // settle the journal, since migration refuses to run while one is
        // pending (E-50). Older builds never journaled an import's identity.
        let directory = tempfile::tempdir().unwrap();
        let paths = temp_paths(directory.path());
        fs::create_dir_all(&paths.metadata).unwrap();
        let controller = history_controller(directory.path().join("backup-history.json"));
        for journal in [
            Journal::backup(export_to(directory.path()), vec!["dev".into()], None),
            Journal::restore(completed_archive(), "copy".into(), Some("dev".into())),
        ] {
            let recovered = recover_at_paths(
                &paths,
                &controller,
                &journal,
                &backup::Cancellation::default(),
            )
            .unwrap();
            assert_eq!(result_of(&recovered).0, "failed");
        }
    }

    fn export_source(id: &str) -> backup::BackupSource {
        backup::BackupSource {
            name: "dev".into(),
            snapshot_group: "dev".into(),
            was_running: false,
            runtime_config: Value::Null,
            computer_configuration: serde_json::json!({"id":id}),
            existing_member: None,
        }
    }

    struct PartialCaptureRuntime {
        calls: Mutex<Vec<Vec<String>>>,
        fail_remove: bool,
        live_parent: bool,
    }
    impl runtime::RuntimeRunner for PartialCaptureRuntime {
        fn run(
            &self,
            _: &runtime::RuntimePaths,
            arguments: &[String],
            _: Duration,
        ) -> Result<runtime::CommandOutput, runtime::RuntimeError> {
            self.calls.lock().unwrap().push(arguments.to_vec());
            let stdout = match arguments.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
                ["snapshot", "list", "--format", "json"] => serde_json::json!([
                    {"snapshot_id":"snap_00000000000000000000000000000000", "group":"dev", "name":"silo-backup-0-1-2"},
                    {"snapshot_id":"snap_11111111111111111111111111111111", "group":"other", "name":"silo-backup-0-3-4"}
                ]).to_string(),
                ["list", "--format", "json"] => if self.live_parent { r#"[{"name":"dev","status":"Stopped"}]"#.into() } else { "[]".into() },
                ["inspect", "dev", "--format", "json"] => serde_json::json!({"name":"dev","status":"Stopped","config":{"snapshot_parent":"snap_00000000000000000000000000000000"}}).to_string(),
                ["snapshot", "head", "dev", "--format", "json"] => serde_json::json!({"head":"snap_00000000000000000000000000000000"}).to_string(),
                ["snapshot", "remove", "dev:silo-backup-0-1-2", "--quiet"] if self.fail_remove => {
                    return Err(runtime::RuntimeError::Invalid("test removal refused".into()));
                }
                ["snapshot", "remove", "dev:silo-backup-0-1-2", "--quiet"] => String::new(),
                _ => panic!("unexpected cleanup command: {arguments:?}"),
            };
            Ok(runtime::CommandOutput {
                stdout,
                stderr: String::new(),
            })
        }
    }

    #[test]
    fn failed_export_removes_only_its_journaled_capture_and_clears_intent_afterward() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = temp_paths(directory.path());
        let controller = history_controller(directory.path().join("backup-history.json"));
        begin(
            &controller,
            Journal::backup(completed_archive(), vec!["dev".into()], None),
        )
        .unwrap();
        let source = export_source(&uuid::Uuid::new_v4().to_string());
        export_capture_intent(&controller, &source, Some("silo-backup-0-1-2")).unwrap();
        assert!(matches!(
            load(&controller.history_path).unwrap().unwrap().request,
            Request::Backup {
                pending_capture: Some(_),
                ..
            }
        ));
        let runner = PartialCaptureRuntime {
            calls: Mutex::new(Vec::new()),
            fail_remove: false,
            live_parent: false,
        };
        settle_export_capture(&runner, &paths, &controller).unwrap();
        assert_eq!(
            runner
                .calls
                .lock()
                .unwrap()
                .iter()
                .filter(|args| args.get(1).is_some_and(|arg| arg == "remove"))
                .cloned()
                .collect::<Vec<_>>(),
            vec![vec![
                "snapshot",
                "remove",
                "dev:silo-backup-0-1-2",
                "--quiet"
            ]]
        );
        assert!(matches!(
            load(&controller.history_path).unwrap().unwrap().request,
            Request::Backup {
                pending_capture: None,
                ..
            }
        ));
    }

    #[test]
    fn failed_capture_cleanup_keeps_its_journal_for_next_launch() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = temp_paths(directory.path());
        let controller = controller_with_scripted_msb(directory.path(), &paths, IMPORT_GROUP);
        begin(
            &controller,
            Journal::backup(completed_archive(), vec!["dev".into()], None),
        )
        .unwrap();
        export_capture_intent(
            &controller,
            &export_source(&uuid::Uuid::new_v4().to_string()),
            Some("silo-backup-0-1-2"),
        )
        .unwrap();
        let runner = PartialCaptureRuntime {
            calls: Mutex::new(Vec::new()),
            fail_remove: true,
            live_parent: false,
        };
        assert!(settle_export_capture(&runner, &paths, &controller).is_err());
        complete(
            &controller,
            Operation::Result {
                operation: "backup",
                archive: completed_archive(),
                running_names: vec![],
                target_name: None,
                outcome: "failed",
                title: "Export failed".into(),
                message: "Interrupted capture".into(),
                detail: None,
            },
        );
        let saved = load(&controller.history_path).unwrap().unwrap();
        assert!(saved.terminal.is_none());
        assert!(matches!(
            saved.request,
            Request::Backup {
                pending_capture: Some(_),
                ..
            }
        ));
    }

    #[test]
    fn crash_after_verified_capture_keeps_the_live_lineage_parent_and_clears_intent() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = temp_paths(directory.path());
        let controller = controller_with_scripted_msb(directory.path(), &paths, IMPORT_GROUP);
        let artifact = paths
            .home
            .join("snapshots/dev/snap_00000000000000000000000000000000");
        fs::create_dir_all(&artifact).unwrap();
        fs::write(directory.path().join("snapshots.json"), serde_json::json!([
            {"snapshot_id":"snap_00000000000000000000000000000000", "group":"dev", "name":"silo-backup-0-1-2", "availability":"ready", "artifact_path":artifact}
        ]).to_string()).unwrap();
        begin(
            &controller,
            Journal::backup(completed_archive(), vec!["dev".into()], None),
        )
        .unwrap();
        export_capture_intent(
            &controller,
            &export_source(&uuid::Uuid::new_v4().to_string()),
            Some("silo-backup-0-1-2"),
        )
        .unwrap();
        let runner = PartialCaptureRuntime {
            calls: Mutex::new(Vec::new()),
            fail_remove: false,
            live_parent: true,
        };
        settle_export_capture(&runner, &paths, &controller).unwrap();
        assert!(!runner
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|args| args.get(1).is_some_and(|arg| arg == "remove")));
        assert!(scripted_calls(directory.path())
            .iter()
            .any(|call| call.starts_with("snapshot verify")));
        assert!(matches!(
            load(&controller.history_path).unwrap().unwrap().request,
            Request::Backup {
                pending_capture: None,
                ..
            }
        ));
    }

    #[test]
    fn capture_journal_rejects_unowned_members_and_invalid_identities() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let controller = history_controller(directory.path().join("backup-history.json"));
        begin(
            &controller,
            Journal::backup(completed_archive(), vec!["dev".into()], None),
        )
        .unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        for (id, group, member) in [
            ("invalid", "dev", "silo-backup-0-1-2"),
            (id.as_str(), "../other", "silo-backup-0-1-2"),
            (id.as_str(), "dev", "checkpoint-to-keep"),
        ] {
            let mut source = export_source(id);
            source.snapshot_group = group.into();
            assert!(export_capture_intent(&controller, &source, Some(member)).is_err());
        }
        assert!(matches!(
            load(&controller.history_path).unwrap().unwrap().request,
            Request::Backup {
                pending_capture: None,
                ..
            }
        ));
    }

    #[test]
    fn only_an_interrupted_import_holds_back_startup() {
        let _test_state = crate::test_support::global_state();
        assert!(!Journal::backup(completed_archive(), vec!["dev".into()], None).blocks_startup());
        assert!(Journal::restore(completed_archive(), "copy".into(), None).blocks_startup());
    }

    #[test]
    fn relaunch_adopts_an_import_whose_computer_was_saved() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = temp_paths(directory.path());
        let controller = history_controller(directory.path().join("backup-history.json"));
        let id = uuid::Uuid::new_v4().to_string();
        begin(
            &controller,
            Journal::restore(completed_archive(), "copy".into(), Some("dev".into())),
        )
        .unwrap();
        save_restore_identity(&controller, &id, IMPORT_GROUP).unwrap();
        runtime::checkpoints::import_pending_restore(&paths, &id, IMPORT_GROUP, IMPORT_MEMBER)
            .unwrap();
        save_computer(&paths, "copy", &id);
        let journal = load(&controller.history_path).unwrap().unwrap();
        let recovered = recover_at_paths(
            &paths,
            &controller,
            &journal,
            &backup::Cancellation::default(),
        )
        .unwrap();
        let (outcome, title, ..) = result_of(&recovered);
        assert_eq!((outcome, title.as_str()), ("success", "Import complete"));
        assert!(paths
            .metadata
            .with_file_name("checkpoints")
            .join(format!("{id}.json"))
            .exists());
        complete(&controller, recovered);
        assert!(!pending(&controller).unwrap());
    }

    #[test]
    fn relaunch_forgets_an_import_that_was_never_saved() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = temp_paths(directory.path());
        let controller = controller_with_scripted_msb(directory.path(), &paths, IMPORT_GROUP);
        let id = uuid::Uuid::new_v4().to_string();
        begin(
            &controller,
            Journal::restore(completed_archive(), "copy".into(), Some("dev".into())),
        )
        .unwrap();
        save_restore_identity(&controller, &id, IMPORT_GROUP).unwrap();
        runtime::checkpoints::import_pending_restore(&paths, &id, IMPORT_GROUP, IMPORT_MEMBER)
            .unwrap();
        // Another computer now uses the requested name; it is not this import's.
        save_computer(&paths, "copy", &uuid::Uuid::new_v4().to_string());
        let journal = load(&controller.history_path).unwrap().unwrap();
        let recovered = recover_at_paths(
            &paths,
            &controller,
            &journal,
            &backup::Cancellation::default(),
        )
        .unwrap();
        let (outcome, title, _message, detail) = result_of(&recovered);
        assert_eq!((outcome, title.as_str()), ("failed", "Import interrupted"));
        assert!(detail.unwrap().contains("No computer was added"));
        assert!(!paths
            .metadata
            .with_file_name("checkpoints")
            .join(format!("{id}.json"))
            .exists());
        assert_eq!(
            runtime::read_metadata(&paths.metadata)
                .unwrap()
                .computers
                .len(),
            1
        );
        // The journaled import group was removed from the runtime store,
        // child first and root last, and nothing else was touched.
        let removals = scripted_calls(directory.path())
            .into_iter()
            .filter(|call| call.starts_with("snapshot remove") || call.starts_with("snapshot head"))
            .collect::<Vec<_>>();
        assert_eq!(
            removals,
            [
                format!("snapshot head {IMPORT_GROUP}:imported-parent"),
                format!("snapshot remove --quiet {IMPORT_GROUP}:imported-member"),
                format!("snapshot remove --quiet {IMPORT_GROUP}:imported-parent"),
            ]
        );
        complete(&controller, recovered);
        assert!(!pending(&controller).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn relaunch_keeps_stage_ownership_when_a_stage_or_parent_is_redirected() {
        let _test_state = crate::test_support::global_state();
        for redirect_parent in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            let paths = temp_paths(directory.path());
            let controller = controller_with_scripted_msb(directory.path(), &paths, IMPORT_GROUP);
            fs::write(directory.path().join("snapshots.json"), b"[]").unwrap();
            begin(
                &controller,
                Journal::restore(completed_archive(), "copy".into(), Some("dev".into())),
            )
            .unwrap();
            save_restore_group(&controller, IMPORT_GROUP).unwrap();
            let snapshot = paths
                .home
                .join("snapshots/.msb-snapshot-load-0123456789abcdef0123456789abcdef");
            fs::create_dir_all(&snapshot).unwrap();
            fs::write(snapshot.join("keep"), b"owned").unwrap();
            let redirected = if redirect_parent {
                paths.home.join("cache")
            } else {
                paths
                    .home
                    .join("cache/tmp/snapshot-load-0123456789abcdef0123456789abcdef")
            };
            fs::create_dir_all(redirected.parent().unwrap()).unwrap();
            fs::write(outside.path().join("keep"), b"outside").unwrap();
            std::os::unix::fs::symlink(outside.path(), &redirected).unwrap();
            let journal = load(&controller.history_path).unwrap().unwrap();
            let error = recover_at_paths(
                &paths,
                &controller,
                &journal,
                &backup::Cancellation::default(),
            )
            .err()
            .expect("redirected stage must fail closed");
            assert!(error.contains("preserved"));
            assert_eq!(fs::read(snapshot.join("keep")).unwrap(), b"owned");
            assert_eq!(fs::read(outside.path().join("keep")).unwrap(), b"outside");
            assert!(matches!(
                load(&controller.history_path).unwrap().unwrap().request,
                Request::Restore { group: Some(_), .. }
            ));
            fs::remove_file(redirected).unwrap();
            recover_at_paths(
                &paths,
                &controller,
                &journal,
                &backup::Cancellation::default(),
            )
            .unwrap();
            assert!(!snapshot.exists());
            assert_eq!(fs::read(outside.path().join("keep")).unwrap(), b"outside");
        }
    }

    /// Opt-in fixture proof with the rebuilt CLI; never starts a computer or launches Silo.
    #[cfg(unix)]
    #[test]
    #[ignore = "requires SILO_E03_MSB pointing to this worktree's rebuilt CLI"]
    fn rebuilt_cli_killed_load_is_removed_by_next_launch_recovery() {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        use std::os::unix::process::ExitStatusExt;
        use std::process::{Command, Stdio};
        use std::time::{Duration, Instant};

        let _test_state = crate::test_support::global_state();
        let executable =
            fs::canonicalize(std::env::var_os("SILO_E03_MSB").expect("SILO_E03_MSB is required"))
                .unwrap();
        let directory = tempfile::tempdir().unwrap();
        let paths = temp_paths(directory.path());
        let controller = Controller {
            service: backup::BackupService::new(
                backup::MsbCommand {
                    executable: executable.clone(),
                    home: paths.home.clone(),
                    storage_home: None,
                    library: paths.library.clone(),
                },
                directory.path().join("scratch"),
            ),
            ..history_controller(directory.path().join("backup-history.json"))
        };
        begin(
            &controller,
            Journal::restore(completed_archive(), "copy".into(), Some("dev".into())),
        )
        .unwrap();
        save_restore_group(&controller, IMPORT_GROUP).unwrap();
        let owned = [
            paths
                .home
                .join("snapshots/.msb-snapshot-load-0123456789abcdef0123456789abcdef"),
            paths
                .home
                .join("cache/tmp/snapshot-load-0123456789abcdef0123456789abcdef"),
        ];
        let unrelated = [
            paths
                .home
                .join("snapshots/.msb-snapshot-load-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            paths
                .home
                .join("cache/tmp/snapshot-load-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            paths.home.join("snapshots/.msb-snapshot-import-legacy"),
        ];
        for stage in &unrelated {
            fs::create_dir_all(stage).unwrap();
            fs::write(stage.join("keep"), b"unrelated").unwrap();
        }
        let fifo = directory.path().join("partial.tar");
        assert!(Command::new("/usr/bin/mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success());
        // Keep the writer open so msb must block in a large tar member. The partial
        // fixture uses an accepted legacy disk entry; its full payload never arrives.
        let mut stream = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&fifo)
            .unwrap();
        let mut child = Command::new(&executable)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", directory.path())
            .env("MSB_HOME", &paths.home)
            .env("XDG_CONFIG_HOME", directory.path().join("config"))
            .args(["snapshot", "load"])
            .arg(&fifo)
            .args([
                "--group",
                IMPORT_GROUP,
                "--stage-id",
                "0123456789abcdef0123456789abcdef",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut header = tar::Header::new_gnu();
        header.set_path("fixture/upper.ext4").unwrap();
        header.set_size(1024 * 1024 * 1024);
        header.set_mode(0o600);
        header.set_cksum();
        let written = stream
            .write_all(header.as_bytes())
            .and_then(|()| stream.write_all(&[42; 4096]));
        fn has_partial_file(root: &Path) -> bool {
            fs::read_dir(root).is_ok_and(|entries| {
                entries.flatten().any(|entry| {
                    entry.file_type().is_ok_and(|kind| {
                        if kind.is_dir() {
                            has_partial_file(&entry.path())
                        } else {
                            entry
                                .metadata()
                                .is_ok_and(|metadata| metadata.len() >= 4096)
                        }
                    })
                })
            })
        }
        let started = Instant::now();
        while written.is_ok()
            && !has_partial_file(&owned[0])
            && started.elapsed() < Duration::from_secs(20)
        {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let partial_written = has_partial_file(&owned[0]);
        let alive = child.try_wait().unwrap().is_none();
        // This PID comes directly from our spawned child, never from a process-name search.
        if alive {
            assert_eq!(
                unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) },
                0
            );
        }
        let output = child.wait_with_output().unwrap();
        drop(stream);
        assert!(
            written.is_ok() && partial_written && alive,
            "fixture load did not block after writing data: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.status.signal(), Some(libc::SIGTERM));
        for stage in &owned {
            assert!(stage.is_dir(), "killed load lost stage {}", stage.display());
        }
        drop(controller);
        let controller = Controller {
            service: backup::BackupService::new(
                backup::MsbCommand {
                    executable: executable.clone(),
                    home: paths.home.clone(),
                    storage_home: None,
                    library: paths.library.clone(),
                },
                directory.path().join("scratch"),
            ),
            ..history_controller(directory.path().join("backup-history.json"))
        };
        let journal = load(&controller.history_path).unwrap().unwrap();
        *controller.journal.lock().unwrap() = Some(journal.clone());
        let recovered = recover_at_paths(
            &paths,
            &controller,
            &journal,
            &backup::Cancellation::default(),
        )
        .unwrap();
        assert_eq!(result_of(&recovered).1, "Import interrupted");
        for stage in owned {
            assert!(!stage.exists());
        }
        for stage in unrelated {
            assert_eq!(fs::read(stage.join("keep")).unwrap(), b"unrelated");
        }
        assert!(matches!(
            load(&controller.history_path).unwrap().unwrap().request,
            Request::Restore { group: None, .. }
        ));
        println!("Fixture proof: {} wrote partial data; SIGTERM left both owned stages; launch recovery removed them and preserved three unrelated stages. No computer started.", executable.display());
    }

    #[test]
    fn relaunch_removes_only_journaled_stages_before_or_after_load_starts() {
        let _test_state = crate::test_support::global_state();
        for load_started in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let paths = temp_paths(directory.path());
            let controller = controller_with_scripted_msb(directory.path(), &paths, IMPORT_GROUP);
            fs::write(directory.path().join("snapshots.json"), b"[]").unwrap();
            begin(
                &controller,
                Journal::restore(completed_archive(), "copy".into(), Some("dev".into())),
            )
            .unwrap();
            save_restore_group(&controller, IMPORT_GROUP).unwrap();
            let owned = [
                paths
                    .home
                    .join("snapshots/.msb-snapshot-load-0123456789abcdef0123456789abcdef"),
                paths
                    .home
                    .join("cache/tmp/snapshot-load-0123456789abcdef0123456789abcdef"),
            ];
            let unrelated = [
                paths
                    .home
                    .join("snapshots/.msb-snapshot-load-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
                paths
                    .home
                    .join("cache/tmp/snapshot-load-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
                paths.home.join("snapshots/.msb-snapshot-import-legacy"),
            ];
            for stage in &unrelated {
                fs::create_dir_all(stage).unwrap();
                fs::write(stage.join("keep"), b"unrelated").unwrap();
            }
            if load_started {
                for stage in &owned {
                    fs::create_dir_all(stage.join("partial/nested")).unwrap();
                    fs::write(stage.join("partial/nested/payload"), b"incomplete").unwrap();
                }
            }
            // Discard all in-process state: launch reloads only the durable journal.
            drop(controller);
            let controller = controller_with_scripted_msb(directory.path(), &paths, IMPORT_GROUP);
            fs::write(directory.path().join("snapshots.json"), b"[]").unwrap();
            let journal = load(&controller.history_path).unwrap().unwrap();
            *controller.journal.lock().unwrap() = Some(journal.clone());
            let recovered = recover_at_paths(
                &paths,
                &controller,
                &journal,
                &backup::Cancellation::default(),
            )
            .unwrap();
            assert_eq!(result_of(&recovered).1, "Import interrupted");
            for stage in owned {
                assert!(!stage.exists());
            }
            for stage in unrelated {
                assert_eq!(fs::read(stage.join("keep")).unwrap(), b"unrelated");
            }
            assert!(scripted_calls(directory.path())
                .iter()
                .all(|call| !call.starts_with("snapshot load") && !call.starts_with("start")));
            assert!(matches!(
                load(&controller.history_path).unwrap().unwrap().request,
                Request::Restore { group: None, .. }
            ));
        }
    }

    #[test]
    fn relaunch_discards_a_load_group_before_a_computer_identity_was_allocated() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = temp_paths(directory.path());
        let controller = controller_with_scripted_msb(directory.path(), &paths, IMPORT_GROUP);
        begin(
            &controller,
            Journal::restore(completed_archive(), "copy".into(), Some("dev".into())),
        )
        .unwrap();
        save_restore_group(&controller, IMPORT_GROUP).unwrap();
        // Older runtimes left random stages without operation identities.
        // Recovery preserves these rather than inferring ownership.
        let stages = [
            paths.home.join("cache/tmp/snapshot-import-unattributed"),
            paths
                .home
                .join("snapshots/.msb-snapshot-import-unattributed"),
        ];
        for stage in &stages {
            fs::create_dir_all(stage).unwrap();
            fs::write(stage.join("payload"), b"unattributed").unwrap();
        }
        let journal = load(&controller.history_path).unwrap().unwrap();
        assert!(
            matches!(&journal.request, Request::Restore { id: None, group: Some(group), .. } if group == IMPORT_GROUP)
        );
        let recovered = recover_at_paths(
            &paths,
            &controller,
            &journal,
            &backup::Cancellation::default(),
        )
        .unwrap();
        assert_eq!(result_of(&recovered).1, "Import interrupted");
        assert_eq!(
            scripted_calls(directory.path())
                .into_iter()
                .filter(|call| call.starts_with("snapshot remove"))
                .collect::<Vec<_>>(),
            [
                format!("snapshot remove --quiet {IMPORT_GROUP}:imported-member"),
                format!("snapshot remove --quiet {IMPORT_GROUP}:imported-parent"),
            ]
        );
        for stage in stages {
            assert_eq!(fs::read(stage.join("payload")).unwrap(), b"unattributed");
        }
        complete(&controller, recovered);
        assert!(!pending(&controller).unwrap());
        assert!(matches!(
            load(&controller.history_path).unwrap().unwrap().request,
            Request::Restore {
                id: None,
                group: None,
                ..
            }
        ));
    }

    #[test]
    fn failed_load_group_cleanup_keeps_ownership_without_a_computer_identity() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = temp_paths(directory.path());
        let controller = controller_with_scripted_msb(directory.path(), &paths, IMPORT_GROUP);
        begin(
            &controller,
            Journal::restore(completed_archive(), "copy".into(), None),
        )
        .unwrap();
        save_restore_group(&controller, IMPORT_GROUP).unwrap();
        fs::write(directory.path().join("refuse-remove"), b"").unwrap();
        let journal = load(&controller.history_path).unwrap().unwrap();
        assert!(recover_at_paths(
            &paths,
            &controller,
            &journal,
            &backup::Cancellation::default()
        )
        .is_err());
        complete(
            &controller,
            Operation::Result {
                operation: "restore",
                archive: completed_archive(),
                running_names: vec![],
                target_name: Some("copy".into()),
                outcome: "failed",
                title: "Import failed".into(),
                message: "Cleanup failed".into(),
                detail: None,
            },
        );
        let saved = load(&controller.history_path).unwrap().unwrap();
        assert!(saved.terminal.is_none());
        assert!(
            matches!(saved.request, Request::Restore { id: None, group: Some(ref group), .. } if group == IMPORT_GROUP)
        );
    }

    #[test]
    fn relaunch_keeps_the_import_identity_when_its_group_cannot_be_removed() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = temp_paths(directory.path());
        let controller = controller_with_scripted_msb(directory.path(), &paths, IMPORT_GROUP);
        fs::write(directory.path().join("refuse-remove"), b"").unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        begin(
            &controller,
            Journal::restore(completed_archive(), "copy".into(), Some("dev".into())),
        )
        .unwrap();
        save_restore_identity(&controller, &id, IMPORT_GROUP).unwrap();
        let journal = load(&controller.history_path).unwrap().unwrap();
        let error = recover_at_paths(
            &paths,
            &controller,
            &journal,
            &backup::Cancellation::default(),
        )
        .unwrap_err();
        assert!(error.contains(IMPORT_GROUP), "{error}");
        // The next launch retries the cleanup.
        assert!(matches!(
            load(&controller.history_path).unwrap().unwrap().request,
            Request::Restore { id: Some(ref saved), group: Some(ref group), .. } if *saved == id && group == IMPORT_GROUP
        ));
    }

    #[test]
    fn relaunch_reports_an_import_interrupted_before_it_saved_anything() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = temp_paths(directory.path());
        let controller = history_controller(directory.path().join("backup-history.json"));
        let journal = Journal::restore(completed_archive(), "copy".into(), Some("dev".into()));
        let recovered = recover_at_paths(
            &paths,
            &controller,
            &journal,
            &backup::Cancellation::default(),
        )
        .unwrap();
        assert_eq!(result_of(&recovered).1, "Import interrupted");
        let mut cancelled = journal;
        cancelled.cancelled = true;
        let recovered = recover_at_paths(
            &paths,
            &controller,
            &cancelled,
            &backup::Cancellation::default(),
        )
        .unwrap();
        assert_eq!(result_of(&recovered).0, "cancelled");
    }

    #[test]
    fn a_saved_import_identity_and_group_survive_reload_and_are_validated() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("backup-history.json");
        let controller = history_controller(path.clone());
        begin(
            &controller,
            Journal::restore(completed_archive(), "copy".into(), None),
        )
        .unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        save_restore_identity(&controller, &id, IMPORT_GROUP).unwrap();
        assert!(matches!(
            load(&path).unwrap().unwrap().request,
            Request::Restore { id: Some(ref saved), group: Some(ref group), .. } if *saved == id && group == IMPORT_GROUP
        ));
        assert!(save_restore_identity(&controller, &id, "../outside").is_err());
        let mut journal = load(&path).unwrap().unwrap();
        if let Request::Restore { group, .. } = &mut journal.request {
            *group = Some("../outside".into());
        }
        write(&path, &journal).unwrap();
        assert!(load(&path).is_err());
    }

    #[test]
    fn a_journal_that_relaunch_could_not_read_is_never_saved() {
        let _test_state = crate::test_support::global_state();
        // Whatever begin saves, load must accept; otherwise a crash would leave
        // a journal that blocks exports and imports on every launch (E-51).
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("backup-history.json");
        let controller = history_controller(path.clone());
        let mut relative = completed_archive();
        relative.archive_path = "Downloads/dev.silo-backup".into();
        for journal in [
            Journal::restore(relative, "copy".into(), None),
            Journal::restore(completed_archive(), "copy".into(), Some("../dev".into())),
            Journal::restore(completed_archive(), "Bad Name".into(), None),
            Journal::backup(completed_archive(), vec!["../dev".into()], None),
        ] {
            assert!(begin(&controller, journal).is_err());
            assert!(!journal_path(&path).exists());
            assert!(token(&controller).unwrap().is_none());
        }
        begin(
            &controller,
            Journal::restore(completed_archive(), "copy".into(), Some("dev".into())),
        )
        .unwrap();
        assert!(load(&path).unwrap().is_some());
    }

    #[test]
    fn an_old_dismissal_cannot_clear_an_identical_later_restore_result() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let controller = history_controller(directory.path().join("backup-history.json"));
        let result = || Operation::Result {
            operation: "restore",
            archive: completed_archive(),
            target_name: Some("restored".into()),
            running_names: vec![],
            outcome: "success",
            title: "Restore complete".into(),
            message: "Computer restored successfully.".into(),
            detail: None,
        };
        begin(
            &controller,
            Journal::restore(completed_archive(), "restored".into(), Some("dev".into())),
        )
        .unwrap();
        let old_id = token(&controller).unwrap().unwrap();
        complete(&controller, result());
        begin(
            &controller,
            Journal::restore(completed_archive(), "restored".into(), Some("dev".into())),
        )
        .unwrap();
        complete(&controller, result());
        set_operation(&controller, result()).unwrap();
        assert!(!super::super::dismiss_finished_operation(
            &controller,
            Some(&serde_json::to_value(result()).unwrap()),
            Some(&old_id)
        )
        .unwrap());
        assert!(controller.view.lock().unwrap().operation.is_some());
        let current_id = token(&controller).unwrap().unwrap();
        assert!(super::super::dismiss_finished_operation(
            &controller,
            Some(&serde_json::to_value(result()).unwrap()),
            Some(&current_id)
        )
        .unwrap());
    }

    #[test]
    fn failed_restore_with_unfinished_cleanup_keeps_ownership_until_recovery() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("backup-history.json");
        let controller = history_controller(path.clone());
        begin(
            &controller,
            Journal::restore(completed_archive(), "restored".into(), Some("dev".into())),
        )
        .unwrap();
        save_restore_identity(&controller, &uuid::Uuid::new_v4().to_string(), IMPORT_GROUP)
            .unwrap();
        let failure = || Operation::Result {
            operation: "restore",
            archive: completed_archive(),
            target_name: Some("restored".into()),
            running_names: vec![],
            outcome: "failed",
            title: "Restore failed".into(),
            message: "Cleanup failed".into(),
            detail: None,
        };
        let result = complete(&controller, failure());
        assert!(
            matches!(result, Operation::Result { message, .. } if message.contains("Relaunch Silo"))
        );
        assert!(load(&path).unwrap().unwrap().terminal.is_none());
        dismiss(&controller).unwrap();
        assert!(load(&path).unwrap().is_some());
        assert!(begin(
            &controller,
            Journal::backup(completed_archive(), vec!["dev".into()], None)
        )
        .is_err());
        clear_restore_identity(&controller).unwrap();
        complete(&controller, failure());
        assert!(load(&path).unwrap().unwrap().terminal.is_some());
    }

    #[test]
    fn abandoned_archive_cleanup_only_removes_this_operations_files() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let mut archive = completed_archive();
        archive.archive_path = directory
            .path()
            .join("saved.silo-backup")
            .to_string_lossy()
            .into_owned();
        let journal = Journal::backup(archive, vec!["dev".into()], None);
        let owned = directory
            .path()
            .join(format!(".silo-backup-{}-partial", journal.id));
        let other = directory
            .path()
            .join(".silo-backup-other-operation-partial");
        let owned_folder = directory
            .path()
            .join(format!(".silo-backup-{}-mixed-1234", journal.id));
        fs::create_dir_all(owned_folder.join("nested")).unwrap();
        fs::write(owned_folder.join("nested/linux.silo-backup"), b"incomplete").unwrap();
        fs::write(&owned, b"incomplete").unwrap();
        fs::write(&other, b"preserve").unwrap();
        fs::write(&journal.archive.archive_path, b"completed archive").unwrap();
        cleanup_archive_partial(&journal).unwrap();
        assert!(!owned.exists());
        assert!(!owned_folder.exists());
        assert_eq!(fs::read(other).unwrap(), b"preserve");
        assert_eq!(
            fs::read(&journal.archive.archive_path).unwrap(),
            b"completed archive"
        );
    }

    #[test]
    fn archive_cleanup_treats_a_missing_destination_as_nothing_to_clean() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let mut archive = completed_archive();
        archive.archive_path = directory
            .path()
            .join("unplugged")
            .join("saved.silo-backup")
            .to_string_lossy()
            .into_owned();
        let journal = Journal::backup(archive, vec!["dev".into()], None);
        cleanup_archive_partial(&journal).unwrap();
    }

    #[test]
    fn dismissing_a_terminal_result_survives_reload_but_pending_work_is_retained() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("backup-history.json");
        let controller = history_controller(path.clone());
        begin(
            &controller,
            Journal::backup(completed_archive(), vec!["dev".into()], None),
        )
        .unwrap();
        dismiss(&controller).unwrap();
        assert!(load(&path).unwrap().is_some());
        complete(
            &controller,
            Operation::Result {
                operation: "backup",
                archive: completed_archive(),
                running_names: vec![],
                target_name: None,
                outcome: "success",
                title: "Done".into(),
                message: "Done".into(),
                detail: None,
            },
        );
        dismiss(&controller).unwrap();
        assert!(load(&path).unwrap().is_none());
    }

    #[test]
    fn durable_intent_and_cancellation_survive_relaunch() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("backup-history.json");
        let controller = history_controller(path.clone());
        begin(
            &controller,
            Journal::restore(completed_archive(), "restored".into(), Some("dev".into())),
        )
        .unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        save_restore_identity(&controller, &id, IMPORT_GROUP).unwrap();
        cancel(&controller, &token(&controller).unwrap().unwrap()).unwrap();
        let reloaded = load(&path).unwrap().unwrap();
        assert!(reloaded.cancelled);
        assert!(reloaded.terminal.is_none());
        assert!(
            matches!(reloaded.request, Request::Restore { id: Some(ref saved), ref name, .. } if saved == &id && name == "restored")
        );
        assert!(matches!(reloaded.operation(), Operation::Running { .. }));
        assert!(begin(
            &controller,
            Journal::backup(completed_archive(), vec!["dev".into()], None)
        )
        .is_err());
    }

    #[test]
    fn result_is_durable_but_never_inferred_from_pending_progress() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("backup-history.json");
        let controller = history_controller(path.clone());
        begin(
            &controller,
            Journal::backup(completed_archive(), vec!["dev".into()], None),
        )
        .unwrap();
        assert!(matches!(
            load(&path).unwrap().unwrap().operation(),
            Operation::Running { .. }
        ));
        complete(
            &controller,
            Operation::Result {
                operation: "backup",
                archive: completed_archive(),
                target_name: None,
                running_names: vec![],
                outcome: "success",
                title: "Export complete".into(),
                message: "Computer exported.".into(),
                detail: None,
            },
        );
        assert!(matches!(
            load(&path).unwrap().unwrap().operation(),
            Operation::Result {
                outcome: "success",
                ..
            }
        ));
        begin(
            &controller,
            Journal::backup(completed_archive(), vec!["dev".into()], None),
        )
        .unwrap();
    }

    #[test]
    fn a_saved_restart_required_result_from_an_older_silo_reads_as_a_completed_export() {
        let _test_state = crate::test_support::global_state();
        // Older builds could end an export as "restart-required" (the export was
        // complete; a computer did not restart). That outcome no longer exists.
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("backup-history.json");
        let mut journal = Journal::backup(completed_archive(), vec!["dev".into()], None);
        journal.terminal = Some(Terminal {
            outcome: "restart-required".into(),
            title: "Export complete; restart failed".into(),
            message: "The export is complete and verified, but a previously running computer did not restart.".into(),
            detail: Some("dev: start failed".into()),
            running: vec!["dev".into()],
        });
        write(&path, &journal).unwrap();
        let operation = load(&path).unwrap().unwrap().operation();
        let serialized = serde_json::to_value(&operation).unwrap();
        assert_eq!(serialized["outcome"], "success");
        assert_eq!(serialized["runningNames"], serde_json::json!([]));
    }

    #[test]
    fn failed_checkpoint_write_preserves_previous_intent() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("backup-history.json");
        let controller = history_controller(path.clone());
        begin(
            &controller,
            Journal::backup(completed_archive(), vec!["dev".into()], None),
        )
        .unwrap();
        fs::remove_file(journal_path(&path)).unwrap();
        fs::create_dir(journal_path(&path)).unwrap();
        assert!(cancel(&controller, &token(&controller).unwrap().unwrap()).is_err());
        assert!(
            !controller
                .journal
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .cancelled
        );
    }

    #[test]
    fn malformed_and_future_checkpoints_are_preserved() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("backup-history.json");
        let saved = journal_path(&path);
        fs::write(&saved, b"broken").unwrap();
        assert!(load(&path).is_err());
        assert_eq!(fs::read(&saved).unwrap(), b"broken");
        let mut journal = Journal::backup(completed_archive(), vec!["dev".into()], None);
        journal.version = 99;
        write(&path, &journal).unwrap();
        assert!(load(&path).is_err());
        assert!(saved.is_file());
    }

    const OLD_COMPUTER: &str = "fcfbc268-ae3f-40ff-8dfa-8af78911e52f";
    const IMPORT_ID: &str = "0f6d5c1a-7a63-4b0a-9a36-4a6f1d3f6e11";
    const JOURNAL_ID: &str = "5b0c8e3e-3b8e-4c4c-9a0b-1f0f5f2d2b77";
    const LOAD_STAGE: &str = "snapshots/.msb-snapshot-load-0123456789abcdef0123456789abcdef";

    /// The previous runtime generation of an installation upgrading from a released
    /// Silo: its saved computers, database and disk, and what an interrupted import
    /// or export leaves in it. Returns the paths recovery gets while the migration
    /// blocks the runtime: the same files and no way to start `msb`.
    fn previous_generation(app_data: &Path, saved: &[(&str, &str)]) -> runtime::RuntimePaths {
        let old = app_data.join("runtime");
        fs::create_dir_all(old.join("microsandbox/db")).unwrap();
        fs::write(old.join("microsandbox/db/msb.db"), b"released database").unwrap();
        fs::create_dir_all(old.join("volumes/dev")).unwrap();
        fs::write(old.join("volumes/dev/workspace.raw"), b"computer").unwrap();
        // An import that stopped while it wrote its disk, and a native snapshot load.
        fs::create_dir_all(old.join("volumes/copy")).unwrap();
        fs::write(old.join("volumes/copy/.silo-restore-owner"), IMPORT_ID).unwrap();
        fs::write(old.join("volumes/copy/workspace.raw"), b"partial disk").unwrap();
        fs::create_dir_all(old.join("microsandbox").join(LOAD_STAGE).join("partial")).unwrap();
        let computers: Vec<_> = saved
            .iter()
            .map(|(name, id)| serde_json::json!({"id":id,"name":name,"cpus":1,"maxCPUs":1,"memoryGiB":1,"maxMemoryGiB":1,"workspaceStorageGiB":1,"runtimeStorageGiB":1}))
            .collect();
        let request: runtime::ComputerConfigurationRequest =
            serde_json::from_value(serde_json::json!({"schemaVersion": 1, "computers": computers}))
                .unwrap();
        runtime::write_metadata(&old.join("computers.json"), &request).unwrap();
        runtime::RuntimePaths {
            guest_image: app_data.join("guest-image"),
            executable: PathBuf::new(),
            home: app_data.join("alias"),
            storage_home: Some(old.join("microsandbox")),
            library: PathBuf::new(),
            metadata: old.join("computers.json"),
            volumes: old.join("volumes"),
        }
    }

    /// Every entry under `root` with its bytes, link target or a directory marker.
    fn tree(root: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
        let mut entries = std::collections::BTreeMap::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(directory) = stack.pop() {
            for entry in fs::read_dir(&directory).unwrap() {
                let path = entry.unwrap().path();
                let metadata = fs::symlink_metadata(&path).unwrap();
                let value = if metadata.file_type().is_symlink() {
                    fs::read_link(&path)
                        .unwrap()
                        .into_os_string()
                        .into_encoded_bytes()
                } else if metadata.is_dir() {
                    stack.push(path.clone());
                    b"<directory>".to_vec()
                } else {
                    fs::read(&path).unwrap()
                };
                entries.insert(path.strip_prefix(root).unwrap().to_path_buf(), value);
            }
        }
        entries
    }

    /// A controller as the first launch of an upgrade installs it: history and
    /// journal in `app_data`, working files under `app_data/scratch`.
    fn controller_in(app_data: &Path) -> Controller {
        Controller {
            service: backup::BackupService::new(
                backup::MsbCommand {
                    executable: PathBuf::new(),
                    home: app_data.join("alias"),
                    storage_home: None,
                    library: PathBuf::new(),
                },
                app_data.join("scratch"),
            ),
            ..history_controller(app_data.join("backup-history.json"))
        }
    }

    /// A journal written by the released Silo 0.9.0, byte for byte: it has neither
    /// `pending_capture` nor `group`, which no release writes.
    fn released_journal(app_data: &Path, archive: &Path, request: Value) -> (Controller, Journal) {
        let text = serde_json::json!({
            "version": 1,
            "id": JOURNAL_ID,
            "archive": {
                "name": "dev.silo-backup",
                "archivePath": archive,
                "completedLabel": "In progress",
                "size": "Unknown",
                "destination": archive.parent().unwrap(),
                "computers": ["dev"],
            },
            "request": request,
            "cancelled": false,
            "terminal": null,
        })
        .to_string();
        fs::write(app_data.join("backup-operation.json"), text).unwrap();
        let controller = controller_in(app_data);
        let journal = load(&controller.history_path)
            .unwrap()
            .expect("the released journal loads");
        *controller.journal.lock().unwrap() = Some(journal.clone());
        (controller, journal)
    }

    fn released_export(computers: Value) -> Value {
        serde_json::json!({"kind": "backup", "names": ["dev"], "computers": computers, "running": computers})
    }

    fn released_import(id: Option<&str>) -> Value {
        serde_json::json!({"kind": "restore", "name": "copy", "source": "dev", "id": id})
    }

    /// Settle the journal the way the first launch of an upgrade does when the operation
    /// left nothing for the runtime, and record the result, as `resume` does.
    fn settle(
        paths: &runtime::RuntimePaths,
        controller: &Controller,
        journal: &Journal,
    ) -> (&'static str, String, String, Option<String>) {
        let settlement =
            settle_before_migration(paths, controller, journal, &backup::Cancellation::default())
                .unwrap();
        let Settlement::Settled(settled) = &settlement else {
            panic!("this journal leaves data only the runtime can remove");
        };
        let result = result_of(settled);
        finish_settlement(controller, settlement, true);
        result
    }

    /// The first launch of an upgrade for an operation that owns data only the runtime can
    /// remove: nothing is reported, and the journal waits for the upgrade.
    fn settle_for_later(paths: &runtime::RuntimePaths, controller: &Controller, journal: &Journal) {
        let settlement =
            settle_before_migration(paths, controller, journal, &backup::Cancellation::default())
                .unwrap();
        assert!(matches!(settlement, Settlement::AfterUpgrade));
    }

    #[test]
    fn every_journal_a_release_can_leave_settles_before_the_upgrade_without_touching_it() {
        let _test_state = crate::test_support::global_state();
        let cases: [(&str, Value, &str, &str, &str); 3] = [
            (
                "export before its computers were saved",
                released_export(serde_json::json!([])),
                "failed",
                "Export interrupted before the upgrade",
                "No export file was saved. Export the computer again.",
            ),
            (
                "export that stopped a running computer",
                released_export(serde_json::json!([["dev", OLD_COMPUTER]])),
                "failed",
                "Export interrupted before the upgrade",
                "No export file was saved. Export the computer again.",
            ),
            (
                "import before it chose a computer identity",
                released_import(None),
                "failed",
                "Import interrupted before the upgrade",
                "No computer was added. Import the file again.",
            ),
        ];
        for (state, request, outcome, title, detail) in cases {
            let directory = tempfile::tempdir().unwrap();
            let app_data = directory.path();
            let paths = previous_generation(app_data, &[("dev", OLD_COMPUTER)]);
            let exports = app_data.join("exports");
            fs::create_dir_all(&exports).unwrap();
            let (controller, journal) =
                released_journal(app_data, &exports.join("dev.silo-backup"), request);
            // Working files this operation left outside the previous generation.
            let partial = exports.join(format!(".silo-backup-{JOURNAL_ID}-partial"));
            let other = exports.join(".silo-backup-another-operation-partial");
            let staging = app_data.join("scratch/backup-interrupted");
            fs::write(&partial, b"incomplete").unwrap();
            fs::write(&other, b"preserve").unwrap();
            fs::create_dir_all(&staging).unwrap();
            fs::write(staging.join("payload"), b"incomplete").unwrap();
            let before = tree(&app_data.join("runtime"));

            let (got_outcome, got_title, message, got_detail) =
                settle(&paths, &controller, &journal);

            assert_eq!(
                (got_outcome, got_title.as_str(), got_detail.as_deref()),
                (outcome, title, Some(detail)),
                "{state}"
            );
            assert!(
                message.starts_with("Silo closed before this"),
                "{state}: {message}"
            );
            // The previous generation is byte-identical: nothing was created, not even
            // the runtime alias or a worker lock, and nothing was removed from it.
            assert_eq!(tree(&app_data.join("runtime")), before, "{state}");
            assert!(!app_data.join("alias").exists(), "{state}");
            // What it left elsewhere is its own, and only it is removed: an export's
            // partial file, and any operation's working folder.
            assert_eq!(partial.exists(), journal.kind() == "restore", "{state}");
            assert!(!staging.exists(), "{state}");
            assert_eq!(fs::read(other).unwrap(), b"preserve", "{state}");
            // The journal recorded the result: nothing is pending and the next launch
            // shows it on the export and import page until it is dismissed.
            assert!(!pending(&controller).unwrap(), "{state}");
            let saved = load(&controller.history_path).unwrap().unwrap();
            assert!(
                matches!(saved.operation(), Operation::Result { title: ref shown, outcome: "failed", .. } if shown == title),
                "{state}"
            );
        }
    }

    #[test]
    fn a_saved_import_is_complete_even_when_a_marker_of_its_disk_remains() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let app_data = directory.path();
        // The settings are the commit point, written before the disk marker is removed.
        let paths = previous_generation(app_data, &[("dev", OLD_COMPUTER), ("copy", IMPORT_ID)]);
        let (controller, journal) = released_journal(
            app_data,
            &app_data.join("dev.silo-backup"),
            released_import(Some(IMPORT_ID)),
        );
        let before = tree(&app_data.join("runtime"));
        let (outcome, title, message, detail) = settle(&paths, &controller, &journal);
        assert_eq!((outcome, title.as_str()), ("success", "Import complete"));
        assert_eq!(message, "Silo verified this import after relaunching.");
        assert_eq!(detail, None);
        assert_eq!(tree(&app_data.join("runtime")), before);
        assert!(!pending(&controller).unwrap());
    }

    /// A journal that owns data only the runtime can remove, as each Silo that writes one
    /// leaves it: `prepare` writes it through the journal's own functions, or by hand as the
    /// released Silo 0.9.0 did.
    type Prepare = fn(&Controller);
    fn journal_cases() -> Vec<(&'static str, bool, Prepare)> {
        vec![
            (
                "import by the released 0.9.0 that wrote its disk",
                false,
                |controller| {
                    // A released import had journaled only the identity.
                    begin(
                        controller,
                        Journal::restore(completed_archive(), "copy".into(), Some("dev".into())),
                    )
                    .unwrap();
                    update(controller, |journal| {
                        if let Request::Restore { id, .. } = &mut journal.request {
                            *id = Some(IMPORT_ID.into());
                        }
                    })
                    .unwrap();
                },
            ),
            (
                "cancelled import by the released 0.9.0 that wrote its disk",
                true,
                |controller| {
                    begin(
                        controller,
                        Journal::restore(completed_archive(), "copy".into(), Some("dev".into())),
                    )
                    .unwrap();
                    update(controller, |journal| {
                        if let Request::Restore { id, .. } = &mut journal.request {
                            *id = Some(IMPORT_ID.into());
                        }
                    })
                    .unwrap();
                },
            ),
            (
                "export of a development build in the middle of a capture",
                false,
                |controller| {
                    begin(
                        controller,
                        Journal::backup(completed_archive(), vec!["dev".into()], None),
                    )
                    .unwrap();
                    export_capture_intent(
                        controller,
                        &export_source(OLD_COMPUTER),
                        Some("silo-backup-0-1-2"),
                    )
                    .unwrap();
                },
            ),
            (
                "cancelled export of a development build in the middle of a capture",
                true,
                |controller| {
                    begin(
                        controller,
                        Journal::backup(completed_archive(), vec!["dev".into()], None),
                    )
                    .unwrap();
                    export_capture_intent(
                        controller,
                        &export_source(OLD_COMPUTER),
                        Some("silo-backup-0-1-2"),
                    )
                    .unwrap();
                },
            ),
            (
                "import of a development build that loaded a snapshot group",
                false,
                |controller| {
                    begin(
                        controller,
                        Journal::restore(completed_archive(), "copy".into(), Some("dev".into())),
                    )
                    .unwrap();
                    save_restore_group(controller, IMPORT_GROUP).unwrap();
                },
            ),
            (
                "import of a development build that journaled its computer identity and group",
                false,
                |controller| {
                    begin(
                        controller,
                        Journal::restore(completed_archive(), "copy".into(), Some("dev".into())),
                    )
                    .unwrap();
                    save_restore_identity(controller, IMPORT_ID, IMPORT_GROUP).unwrap();
                },
            ),
            (
                "cancelled import of a development build that journaled its computer identity and group",
                true,
                |controller| {
                    begin(
                        controller,
                        Journal::restore(completed_archive(), "copy".into(), Some("dev".into())),
                    )
                    .unwrap();
                    save_restore_identity(controller, IMPORT_ID, IMPORT_GROUP).unwrap();
                },
            ),
        ]
    }

    #[test]
    fn data_only_the_runtime_can_remove_waits_for_the_upgrade_instead_of_being_given_up() {
        let _test_state = crate::test_support::global_state();
        for (state, cancelled, prepare) in journal_cases() {
            let directory = tempfile::tempdir().unwrap();
            let app_data = directory.path();
            let paths = previous_generation(app_data, &[("dev", OLD_COMPUTER)]);
            let controller = controller_in(app_data);
            prepare(&controller);
            if cancelled {
                cancel(&controller, &token(&controller).unwrap().unwrap()).unwrap();
            }
            // The next launch reloads only the durable journal.
            let journal = load(&controller.history_path).unwrap().unwrap();
            assert!(!journal.is_awaiting_upgrade(), "{state}");
            let owned = |request: &Request| {
                matches!(
                    request,
                    Request::Backup {
                        pending_capture: Some(_),
                        ..
                    } | Request::Restore { id: Some(_), .. }
                        | Request::Restore { group: Some(_), .. }
                )
            };
            assert!(owned(&journal.request), "{state}");
            // What it left outside the previous generation is its own.
            let staging = app_data.join("scratch/restore-interrupted");
            fs::create_dir_all(&staging).unwrap();
            fs::write(staging.join("payload"), b"incomplete").unwrap();
            let before = tree(&app_data.join("runtime"));

            for launch in 1..=2 {
                *controller.journal.lock().unwrap() =
                    Some(load(&controller.history_path).unwrap().unwrap());
                let journal = load(&controller.history_path).unwrap().unwrap();
                settle_for_later(&paths, &controller, &journal);
                // The previous generation is byte-identical, not even a runtime alias or a
                // worker lock was added, and the working files are gone.
                assert_eq!(
                    tree(&app_data.join("runtime")),
                    before,
                    "{state}: launch {launch}"
                );
                assert!(!app_data.join("alias").exists(), "{state}");
                assert!(!staging.exists(), "{state}");
                // Nothing was given up: the journal still owns the cleanup, and it is
                // marked so the migration leaves it in place.
                let saved = load(&controller.history_path).unwrap().unwrap();
                assert!(saved.is_awaiting_upgrade(), "{state}");
                assert!(saved.terminal.is_none(), "{state}");
                assert_eq!(saved.cancelled, cancelled, "{state}");
                assert!(owned(&saved.request), "{state}: ownership was kept");
                assert!(pending(&controller).unwrap(), "{state}");
            }
        }
    }

    #[test]
    fn a_journal_waiting_for_the_upgrade_is_recognised_by_its_marker_alone() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let controller = history_controller(directory.path().join("backup-history.json"));
        begin(
            &controller,
            Journal::restore(completed_archive(), "copy".into(), None),
        )
        .unwrap();
        let text = |controller: &Controller| {
            fs::read_to_string(journal_path(&controller.history_path)).unwrap()
        };
        // Written only once set, so a build that does not know it still reads the file.
        assert!(!text(&controller).contains("awaitingUpgrade"));
        update(&controller, |journal| journal.awaiting_upgrade = true).unwrap();
        assert!(text(&controller).contains("\"awaitingUpgrade\":true"));
        assert!(load(&controller.history_path)
            .unwrap()
            .unwrap()
            .is_awaiting_upgrade());
        // A result ends the wait, whatever the marker says.
        complete(
            &controller,
            Operation::Result {
                operation: "restore",
                archive: completed_archive(),
                running_names: vec![],
                target_name: Some("copy".into()),
                outcome: "failed",
                title: "Import interrupted".into(),
                message: "Silo closed before this import finished.".into(),
                detail: None,
            },
        );
        let saved = load(&controller.history_path).unwrap().unwrap();
        assert!(saved.terminal.is_some());
        assert!(!saved.is_awaiting_upgrade());
        assert!(!pending(&controller).unwrap());
        // And the marker goes with it, so a recorded result is the same as ever.
        assert!(!text(&controller).contains("awaitingUpgrade"));
    }

    /// The runtime's computers as `msb list` and `msb inspect` report them. Removing one
    /// goes through `remove`, which can be made to fail.
    struct Computers {
        present: Mutex<Vec<(String, String)>>,
        calls: Mutex<Vec<String>>,
        fail_remove: bool,
        /// What `msb inspect` reports for every computer. The bundled runtime removes a
        /// `Created` computer like a `Stopped` one.
        status: &'static str,
    }
    impl Computers {
        fn with(computers: &[(&str, &str)]) -> Self {
            Self {
                present: Mutex::new(
                    computers
                        .iter()
                        .map(|(name, id)| (name.to_string(), id.to_string()))
                        .collect(),
                ),
                calls: Mutex::new(Vec::new()),
                fail_remove: false,
                status: "Stopped",
            }
        }
        /// As a released import leaves its computer: created, never started.
        fn never_started(mut self) -> Self {
            self.status = "Created";
            self
        }
        fn names(&self) -> Vec<String> {
            self.present
                .lock()
                .unwrap()
                .iter()
                .map(|(name, _)| name.clone())
                .collect()
        }
        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }
    impl runtime::RuntimeRunner for Computers {
        fn run(
            &self,
            _: &runtime::RuntimePaths,
            arguments: &[String],
            _: Duration,
        ) -> Result<runtime::CommandOutput, runtime::RuntimeError> {
            self.calls.lock().unwrap().push(arguments.join(" "));
            let present = self.present.lock().unwrap().clone();
            let stdout = match arguments
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .as_slice()
            {
                ["list", "--format", "json"] => serde_json::Value::Array(
                    present
                        .iter()
                        .map(|(name, _)| serde_json::json!({ "name": name }))
                        .collect(),
                )
                .to_string(),
                ["inspect", name, "--format", "json"] => {
                    let (_, id) = present
                        .iter()
                        .find(|(candidate, _)| candidate == name)
                        .expect("only a computer that is listed is inspected");
                    serde_json::json!({"name": name, "status": self.status, "config": {"labels": {"silo.managed": "true", "silo.machine-id": id}}}).to_string()
                }
                ["remove", "--force", "--quiet", name] => {
                    if self.fail_remove {
                        return Err(runtime::RuntimeError::Invalid(
                            "test removal refused".into(),
                        ));
                    }
                    self.present
                        .lock()
                        .unwrap()
                        .retain(|(candidate, _)| candidate != name);
                    String::new()
                }
                other => panic!("unexpected runtime command: {other:?}"),
            };
            Ok(runtime::CommandOutput {
                stdout,
                stderr: String::new(),
            })
        }
    }

    /// Storage as recovery finds it once the converted generation is selected: the saved
    /// computer `dev`, and the disk folder a released import claimed and wrote for `copy`.
    fn storage_with_released_import(directory: &Path) -> runtime::RuntimePaths {
        let paths = temp_paths(directory);
        save_computer(&paths, "dev", OLD_COMPUTER);
        fs::create_dir_all(paths.volumes.join("dev")).unwrap();
        fs::write(paths.volumes.join("dev/workspace.raw"), b"computer").unwrap();
        fs::create_dir_all(paths.volumes.join("copy")).unwrap();
        fs::write(paths.volumes.join("copy/.silo-restore-owner"), IMPORT_ID).unwrap();
        fs::write(paths.volumes.join("copy/workspace.raw"), b"partial disk").unwrap();
        paths
    }

    fn recover_released_import(
        runtime: &Computers,
        directory: &Path,
        paths: &runtime::RuntimePaths,
    ) -> (Controller, Result<Operation, String>) {
        let (controller, journal) = released_journal(
            directory,
            &directory.join("dev.silo-backup"),
            released_import(Some(IMPORT_ID)),
        );
        let recovered = recover_at_paths_with(
            runtime,
            paths,
            &controller,
            &journal,
            &backup::Cancellation::default(),
        );
        (controller, recovered)
    }

    #[test]
    fn recovery_removes_what_an_import_by_a_released_silo_left() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = storage_with_released_import(directory.path());
        let runtime = Computers::with(&[("dev", OLD_COMPUTER), ("copy", IMPORT_ID)]);
        let (controller, recovered) = recover_released_import(&runtime, directory.path(), &paths);
        let recovered = recovered.unwrap();
        let (outcome, title, message, detail) = result_of(&recovered);
        assert_eq!((outcome, title.as_str()), ("failed", "Import interrupted"));
        assert_eq!(message, "Silo closed before this import finished.");
        assert_eq!(
            detail.as_deref(),
            Some("No computer was added. Import the file again.")
        );
        // Its computer record and its disk are gone, and the saved computer is untouched.
        assert!(!paths.volumes.join("copy").exists());
        assert_eq!(runtime.names(), ["dev"]);
        assert_eq!(
            runtime.calls(),
            [
                "list --format json",
                "inspect copy --format json",
                "remove --force --quiet copy"
            ]
        );
        assert_eq!(
            fs::read(paths.volumes.join("dev/workspace.raw")).unwrap(),
            b"computer"
        );
        complete(&controller, recovered);
        let saved = load(&controller.history_path).unwrap().unwrap();
        assert!(saved.terminal.is_some());
        assert!(matches!(saved.request, Request::Restore { id: None, .. }));
    }

    #[test]
    fn recovery_removes_a_released_import_whose_computer_never_started() {
        let _test_state = crate::test_support::global_state();
        // A released import leaves its computer created and never started. The runtime
        // removes it whole, so neither its record nor its name stays behind.
        for cancelled in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let paths = storage_with_released_import(directory.path());
            let runtime =
                Computers::with(&[("dev", OLD_COMPUTER), ("copy", IMPORT_ID)]).never_started();
            let (controller, journal) = released_journal(
                directory.path(),
                &directory.path().join("dev.silo-backup"),
                released_import(Some(IMPORT_ID)),
            );
            let journal = if cancelled {
                cancel(&controller, &token(&controller).unwrap().unwrap()).unwrap();
                load(&controller.history_path).unwrap().unwrap()
            } else {
                journal
            };
            let recovered = recover_at_paths_with(
                &runtime,
                &paths,
                &controller,
                &journal,
                &backup::Cancellation::default(),
            )
            .unwrap();
            let (outcome, title, _, detail) = result_of(&recovered);
            assert!(!paths.volumes.join("copy").exists());
            assert_eq!(runtime.names(), ["dev"]);
            assert_eq!(
                runtime.calls(),
                [
                    "list --format json",
                    "inspect copy --format json",
                    "remove --force --quiet copy"
                ]
            );
            if cancelled {
                assert_eq!((outcome, title.as_str()), ("cancelled", "Import cancelled"));
                assert_eq!(detail.as_deref(), Some("No computer was added."));
            } else {
                assert_eq!((outcome, title.as_str()), ("failed", "Import interrupted"));
                assert_eq!(
                    detail.as_deref(),
                    Some("No computer was added. Import the file again.")
                );
            }
            // Recovery is complete: the journal no longer owns anything.
            complete(&controller, recovered);
            let saved = load(&controller.history_path).unwrap().unwrap();
            assert!(saved.terminal.is_some());
            assert!(matches!(saved.request, Request::Restore { id: None, .. }));
            assert_eq!(
                fs::read(paths.volumes.join("dev/workspace.raw")).unwrap(),
                b"computer"
            );
        }
    }

    #[test]
    fn recovery_of_a_released_import_removes_a_disk_whose_computer_was_never_created() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = storage_with_released_import(directory.path());
        let runtime = Computers::with(&[("dev", OLD_COMPUTER)]);
        let (_, recovered) = recover_released_import(&runtime, directory.path(), &paths);
        assert_eq!(result_of(&recovered.unwrap()).0, "failed");
        assert!(!paths.volumes.join("copy").exists());
        assert_eq!(runtime.calls(), ["list --format json"]);
        assert_eq!(runtime.names(), ["dev"]);
    }

    #[test]
    fn recovery_of_a_released_import_leaves_everything_it_cannot_prove_is_its_own() {
        let _test_state = crate::test_support::global_state();
        // The folder carries another identity, or none, and is not empty.
        for marker in [Some("another-identity"), None] {
            let directory = tempfile::tempdir().unwrap();
            let paths = storage_with_released_import(directory.path());
            let marker_path = paths.volumes.join("copy/.silo-restore-owner");
            match marker {
                Some(owner) => fs::write(&marker_path, owner).unwrap(),
                None => fs::remove_file(&marker_path).unwrap(),
            }
            let runtime = Computers::with(&[("dev", OLD_COMPUTER), ("copy", IMPORT_ID)]);
            let (controller, recovered) =
                recover_released_import(&runtime, directory.path(), &paths);
            let error = recovered.err().expect("ownership is not proven");
            assert!(error.contains("No files were removed"), "{error}");
            assert_eq!(
                fs::read(paths.volumes.join("copy/workspace.raw")).unwrap(),
                b"partial disk"
            );
            assert_eq!(runtime.names(), ["dev", "copy"]);
            assert!(runtime.calls().is_empty());
            assert!(pending(&controller).unwrap());
        }
        // An empty folder is nothing to keep.
        let directory = tempfile::tempdir().unwrap();
        let paths = storage_with_released_import(directory.path());
        fs::remove_dir_all(paths.volumes.join("copy")).unwrap();
        fs::create_dir(paths.volumes.join("copy")).unwrap();
        let runtime = Computers::with(&[("dev", OLD_COMPUTER)]);
        let (_, recovered) = recover_released_import(&runtime, directory.path(), &paths);
        assert_eq!(result_of(&recovered.unwrap()).0, "failed");
        assert!(!paths.volumes.join("copy").exists());
    }

    #[test]
    fn recovery_of_a_released_import_keeps_a_computer_that_carries_another_identity() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = storage_with_released_import(directory.path());
        let other = uuid::Uuid::new_v4().to_string();
        let runtime = Computers::with(&[("dev", OLD_COMPUTER), ("copy", other.as_str())]);
        let (controller, recovered) = recover_released_import(&runtime, directory.path(), &paths);
        recovered.err().expect("another computer is never removed");
        assert_eq!(runtime.names(), ["dev", "copy"]);
        assert!(!runtime
            .calls()
            .iter()
            .any(|call| call.starts_with("remove")));
        assert!(paths.volumes.join("copy/workspace.raw").exists());
        assert!(pending(&controller).unwrap());
    }

    #[test]
    fn recovery_of_a_released_import_leaves_the_folder_of_a_saved_computer_with_that_name() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = storage_with_released_import(directory.path());
        // The name is a saved computer's now, so the folder belongs to it.
        let saved = uuid::Uuid::new_v4().to_string();
        let request: runtime::ComputerConfigurationRequest = serde_json::from_value(serde_json::json!({
            "schemaVersion": 1,
            "computers": [{"id":saved,"name":"copy","cpus":1,"maxCPUs":1,"memoryGiB":1,"maxMemoryGiB":1,"workspaceStorageGiB":1,"runtimeStorageGiB":1}]
        }))
        .unwrap();
        runtime::write_metadata(&paths.metadata, &request).unwrap();
        let runtime = Computers::with(&[("copy", saved.as_str())]);
        let (_, recovered) = recover_released_import(&runtime, directory.path(), &paths);
        assert_eq!(result_of(&recovered.unwrap()).0, "failed");
        assert_eq!(
            fs::read(paths.volumes.join("copy/workspace.raw")).unwrap(),
            b"partial disk"
        );
        assert!(runtime.calls().is_empty());
    }

    #[test]
    fn a_failed_removal_keeps_the_released_import_for_the_next_launch() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = storage_with_released_import(directory.path());
        let mut runtime = Computers::with(&[("dev", OLD_COMPUTER), ("copy", IMPORT_ID)]);
        runtime.fail_remove = true;
        let (controller, recovered) = recover_released_import(&runtime, directory.path(), &paths);
        recovered.err().expect("the computer could not be removed");
        // Nothing was removed before the computer, and the journal still owns the import.
        assert!(paths.volumes.join("copy/workspace.raw").exists());
        assert_eq!(runtime.names(), ["dev", "copy"]);
        let saved = load(&controller.history_path).unwrap().unwrap();
        assert!(saved.terminal.is_none());
        assert!(matches!(
            saved.request,
            Request::Restore { id: Some(_), .. }
        ));

        // The next launch finishes it.
        let retry = Computers::with(&[("dev", OLD_COMPUTER), ("copy", IMPORT_ID)]);
        let recovered = recover_at_paths_with(
            &retry,
            &paths,
            &controller,
            &saved,
            &backup::Cancellation::default(),
        )
        .unwrap();
        assert_eq!(result_of(&recovered).0, "failed");
        assert!(!paths.volumes.join("copy").exists());
        assert_eq!(retry.names(), ["dev"]);
    }

    /// A journal as the previous Silo left it, then the file in `app_data` (not written
    /// through the journal, which would refuse an unsupported one).
    fn journal_text(version: u64, extra: Option<(&str, Value)>) -> String {
        let mut journal = serde_json::json!({
            "version": version,
            "id": JOURNAL_ID,
            "archive": {
                "name": "dev.silo-backup",
                "archivePath": "/backups/dev.silo-backup",
                "completedLabel": "In progress",
                "size": "Unknown",
                "destination": "/backups",
                "computers": ["dev"],
            },
            "request": released_export(serde_json::json!([])),
            "cancelled": false,
            "terminal": null,
        });
        if let Some((key, value)) = extra {
            journal[key] = value;
        }
        journal.to_string()
    }

    fn utc_date() -> String {
        let today = time::OffsetDateTime::now_utc();
        format!(
            "{:04}-{:02}-{:02}",
            today.year(),
            today.month() as u8,
            today.day()
        )
    }

    #[test]
    fn the_migration_sorts_journals_by_whether_it_must_wait_for_them() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let app_data = directory.path();
        let journal = app_data.join("backup-operation.json");
        let state = |expected: &str| {
            let found = match journal_state(app_data) {
                JournalState::Settled => "settled",
                JournalState::Pending => "pending",
                JournalState::Unreadable => "unreadable",
                JournalState::Unavailable => "unavailable",
            };
            assert_eq!(found, expected);
        };
        state("settled");
        let pending_text = journal_text(1, None);
        fs::write(&journal, &pending_text).unwrap();
        state("pending");
        fs::write(
            &journal,
            journal_text(1, Some(("awaitingUpgrade", true.into()))),
        )
        .unwrap();
        state("settled");
        fs::write(
            &journal,
            journal_text(
                1,
                Some((
                    "terminal",
                    serde_json::json!({"outcome":"failed","title":"t","message":"m","detail":null,"running":[]}),
                )),
            ),
        )
        .unwrap();
        state("settled");
        // Unreadable: damaged, empty, written by another version, or with unknown content.
        for text in [
            "{not json".to_string(),
            String::new(),
            journal_text(2, None),
            journal_text(1, Some(("unknown", true.into()))),
            pending_text.replace(JOURNAL_ID, "not-an-identity"),
            pending_text.replace("/backups/dev.silo-backup", "relative.silo-backup"),
        ] {
            fs::write(&journal, &text).unwrap();
            state("unreadable");
        }
        // A file that cannot be read at all, such as a folder in its place, is not known to
        // be unusable: it is not set aside.
        fs::remove_file(&journal).unwrap();
        fs::create_dir(&journal).unwrap();
        state("unavailable");
    }

    #[test]
    fn an_unreadable_journal_is_set_aside_with_a_notice_and_never_deleted() {
        let _test_state = crate::test_support::global_state();
        let cases = [
            ("damaged", "{not json".to_string()),
            ("empty", String::new()),
            ("another version", journal_text(2, None)),
            (
                "unknown content",
                journal_text(1, Some(("unknown", true.into()))),
            ),
        ];
        for (state, text) in cases {
            let directory = tempfile::tempdir().unwrap();
            let app_data = directory.path();
            fs::write(app_data.join("backup-operation.json"), &text).unwrap();
            fs::write(app_data.join("backup-history.json"), b"history").unwrap();
            let first = utc_date();
            let aside = set_aside_unreadable_journal(app_data).unwrap();
            let second = utc_date();
            // Only renamed: the same bytes, in the app data folder, named for the day.
            assert_eq!(fs::read_to_string(&aside).unwrap(), text, "{state}");
            assert_eq!(aside.parent().unwrap(), app_data, "{state}");
            let name = aside.file_name().unwrap().to_string_lossy().into_owned();
            assert!(
                [first, second]
                    .iter()
                    .any(|date| name == format!("backup-operation.unreadable-{date}.json")),
                "{state}: {name}"
            );
            // In its place the export page has a result that says what happened, and the
            // migration no longer waits for anything.
            assert!(
                matches!(journal_state(app_data), JournalState::Settled),
                "{state}"
            );
            let saved = load(&app_data.join("backup-history.json"))
                .unwrap()
                .unwrap();
            assert!(!saved.is_pending(), "{state}");
            match saved.operation() {
                Operation::Result {
                    outcome,
                    title,
                    message,
                    detail,
                    ..
                } => {
                    assert_eq!(outcome, "failed", "{state}");
                    assert_eq!(title, "Export or import record set aside", "{state}");
                    assert_eq!(
                        message,
                        "An export or import record couldn\u{2019}t be read and was set aside.",
                        "{state}"
                    );
                    assert_eq!(
                        detail.as_deref(),
                        Some(
                            "If an export or import was running before the upgrade, run it again."
                        ),
                        "{state}"
                    );
                }
                Operation::Running { .. } => panic!("{state}: the notice is a result"),
            }
            assert_eq!(
                fs::read(app_data.join("backup-history.json")).unwrap(),
                b"history",
                "{state}"
            );
        }
    }

    #[test]
    fn setting_unreadable_journals_aside_never_replaces_an_earlier_one() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let app_data = directory.path();
        let mut kept = Vec::new();
        for attempt in 1..=3 {
            let text = format!("unreadable record {attempt}");
            fs::write(app_data.join("backup-operation.json"), &text).unwrap();
            let aside = set_aside_unreadable_journal(app_data).unwrap();
            kept.push((aside, text));
            // The notice that replaced it is dismissed before the next one is written.
            fs::remove_file(app_data.join("backup-operation.json")).unwrap();
        }
        let names: std::collections::BTreeSet<_> =
            kept.iter().map(|(path, _)| path.clone()).collect();
        assert_eq!(names.len(), 3, "every record has a file of its own");
        for (path, text) in kept {
            assert_eq!(fs::read_to_string(path).unwrap(), text);
        }
    }

    #[test]
    fn setting_a_journal_aside_fails_without_losing_it_when_it_cannot_be_renamed() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let app_data = directory.path();
        // No journal to set aside: nothing is created.
        assert!(set_aside_unreadable_journal(app_data).is_err());
        assert_eq!(fs::read_dir(app_data).unwrap().count(), 0);
    }

    fn result_operation(title: &str) -> Operation {
        Operation::Result {
            operation: "restore",
            archive: completed_archive(),
            running_names: vec![],
            target_name: Some("copy".into()),
            outcome: "failed",
            title: title.into(),
            message: "Silo closed before this import finished.".into(),
            detail: Some("No computer was added. Import the file again.".into()),
        }
    }

    fn journal_json(controller: &Controller) -> Value {
        serde_json::from_slice(&fs::read(journal_path(&controller.history_path)).unwrap()).unwrap()
    }

    #[test]
    fn a_result_is_unseen_only_when_an_upgrade_produced_it() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let controller = history_controller(directory.path().join("backup-history.json"));
        let journal = Journal::restore(completed_archive(), "copy".into(), None);
        let id = journal.id.clone();

        // An ordinary result, or one recorded after an operation the user ran, is not.
        begin(&controller, journal.clone()).unwrap();
        complete(&controller, result_operation("Import interrupted"));
        assert!(!unseen(&controller).unwrap());
        assert!(journal_json(&controller).get("unseen").is_none());
        assert!(!acknowledge(&controller, &id).unwrap());

        // One settled before the storage migration is.
        begin(&controller, journal.clone()).unwrap();
        complete_marked(
            &controller,
            result_operation("Import interrupted before the upgrade"),
            true,
        );
        assert!(unseen(&controller).unwrap());
        assert_eq!(journal_json(&controller)["unseen"], true);
        assert!(load(&controller.history_path)
            .unwrap()
            .unwrap()
            .is_unseen_result());

        // One that waited for the upgrade is too, wherever it is recorded.
        begin(&controller, journal.clone()).unwrap();
        update(&controller, |journal| journal.awaiting_upgrade = true).unwrap();
        assert!(!unseen(&controller).unwrap(), "nothing is reported yet");
        complete(&controller, result_operation("Import interrupted"));
        assert!(unseen(&controller).unwrap());
        assert_eq!(journal_json(&controller).get("awaitingUpgrade"), None);

        // A later operation replaces it, and the replacement is not unseen.
        begin(&controller, journal).unwrap();
        assert!(!unseen(&controller).unwrap());
        complete(&controller, result_operation("Import interrupted"));
        assert!(!unseen(&controller).unwrap());
    }

    #[test]
    fn what_the_upgrade_settled_is_unseen_once_it_is_reported() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let app_data = directory.path();
        let paths = previous_generation(app_data, &[("dev", OLD_COMPUTER)]);
        let (controller, journal) = released_journal(
            app_data,
            &app_data.join("dev.silo-backup"),
            released_export(serde_json::json!([])),
        );
        // Nothing was reported while the journal only waited to be settled.
        assert!(!unseen(&controller).unwrap());
        let (outcome, title, _, _) = settle(&paths, &controller, &journal);
        assert_eq!(
            (outcome, title.as_str()),
            ("failed", "Export interrupted before the upgrade")
        );
        assert!(unseen(&controller).unwrap());
        assert_eq!(journal_json(&controller)["unseen"], true);
    }

    #[test]
    fn the_unseen_marker_round_trips_and_older_journals_count_as_seen() {
        let _test_state = crate::test_support::global_state();
        let terminal = serde_json::json!({"outcome":"failed","title":"Import interrupted","message":"m","detail":null,"running":[]});
        let loaded = |extra: Vec<(&str, Value)>| {
            let directory = tempfile::tempdir().unwrap();
            let mut journal: Value = serde_json::from_str(&journal_text(1, None)).unwrap();
            for (key, value) in extra {
                journal[key] = value;
            }
            fs::write(
                directory.path().join("backup-operation.json"),
                journal.to_string(),
            )
            .unwrap();
            load(&directory.path().join("backup-history.json"))
                .unwrap()
                .unwrap()
        };
        // Written before the marker existed: seen, like every result the user had before.
        assert!(!loaded(vec![("terminal", terminal.clone())]).is_unseen_result());
        let marked = loaded(vec![
            ("terminal", terminal.clone()),
            ("unseen", true.into()),
        ]);
        assert!(marked.is_unseen_result());
        assert!(!loaded(vec![("terminal", terminal), ("unseen", false.into())]).is_unseen_result());
        // A result is what is unseen: an unfinished operation never is.
        assert!(!loaded(vec![("unseen", true.into())]).is_unseen_result());
        // It survives a write and a read, and is omitted when false so older builds read it.
        let directory = tempfile::tempdir().unwrap();
        let history = directory.path().join("backup-history.json");
        write(&history, &marked).unwrap();
        assert!(load(&history).unwrap().unwrap().is_unseen_result());
        let text = fs::read_to_string(journal_path(&history)).unwrap();
        assert!(text.contains("\"unseen\":true"), "{text}");
        let seen = Journal {
            unseen: false,
            ..marked
        };
        write(&history, &seen).unwrap();
        let text = fs::read_to_string(journal_path(&history)).unwrap();
        assert!(!text.contains("unseen"), "{text}");
    }

    #[test]
    fn acknowledging_a_result_marks_only_that_result_seen_and_keeps_it() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let controller = history_controller(directory.path().join("backup-history.json"));
        let journal = Journal::restore(completed_archive(), "copy".into(), None);
        let id = journal.id.clone();
        begin(&controller, journal).unwrap();
        complete_marked(
            &controller,
            result_operation("Import interrupted before the upgrade"),
            true,
        );

        // A result that was replaced is left alone.
        let other = uuid::Uuid::new_v4().to_string();
        assert!(!acknowledge(&controller, &other).unwrap());
        assert!(unseen(&controller).unwrap());

        assert!(acknowledge(&controller, &id).unwrap());
        assert!(!unseen(&controller).unwrap());
        // Durable, and the result itself stays until it is dismissed.
        let saved = load(&controller.history_path).unwrap().unwrap();
        assert!(!saved.is_unseen_result());
        assert!(saved.terminal.is_some());
        assert!(journal_json(&controller).get("unseen").is_none());
        // Acknowledging again changes nothing.
        assert!(!acknowledge(&controller, &id).unwrap());

        // A result that cannot be saved as seen stays unseen, so it is not lost.
        begin(
            &controller,
            Journal::restore(completed_archive(), "copy".into(), None),
        )
        .unwrap();
        let again = token(&controller).unwrap().unwrap();
        complete_marked(
            &controller,
            result_operation("Import interrupted before the upgrade"),
            true,
        );
        fs::remove_file(journal_path(&controller.history_path)).unwrap();
        fs::create_dir(journal_path(&controller.history_path)).unwrap();
        assert!(acknowledge(&controller, &again).is_err());
        assert!(unseen(&controller).unwrap());
    }

    #[test]
    fn dismissing_an_unseen_result_removes_it_with_its_marker() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let controller = history_controller(directory.path().join("backup-history.json"));
        begin(
            &controller,
            Journal::restore(completed_archive(), "copy".into(), None),
        )
        .unwrap();
        complete_marked(
            &controller,
            result_operation("Import interrupted before the upgrade"),
            true,
        );
        assert!(unseen(&controller).unwrap());
        dismiss(&controller).unwrap();
        assert!(!unseen(&controller).unwrap());
        assert!(load(&controller.history_path).unwrap().is_none());
    }

    #[test]
    fn a_journal_that_cannot_be_used_is_set_aside_at_startup_with_an_unseen_notice() {
        let _test_state = crate::test_support::global_state();
        let cases = [
            ("damaged", "{not json".to_string()),
            ("empty", String::new()),
            ("another version", journal_text(2, None)),
            (
                "unknown content",
                journal_text(1, Some(("unknown", true.into()))),
            ),
        ];
        for (state, text) in cases {
            let directory = tempfile::tempdir().unwrap();
            let app_data = directory.path();
            let history = app_data.join("backup-history.json");
            fs::write(app_data.join("backup-operation.json"), &text).unwrap();
            assert!(load(&history).is_err(), "{state}");

            let notice = set_aside_at_startup(&history).unwrap();
            assert!(notice.is_unseen_result(), "{state}");
            // The notice is the journal, so it is what `load` reads from now on.
            let saved = load(&history).unwrap().unwrap();
            assert_eq!(saved.id, notice.id, "{state}");
            assert!(saved.is_unseen_result(), "{state}");
            assert!(!saved.is_pending(), "{state}");
            assert!(
                matches!(journal_state(app_data), JournalState::Settled),
                "{state}"
            );
            let Operation::Result {
                outcome,
                title,
                message,
                detail,
                ..
            } = saved.operation()
            else {
                panic!("{state}: the notice is a result");
            };
            assert_eq!(outcome, "failed", "{state}");
            assert_eq!(title, "Export or import record set aside", "{state}");
            assert_eq!(
                message, "An export or import record couldn\u{2019}t be read and was set aside.",
                "{state}"
            );
            // Nothing says the upgrade was involved.
            assert_eq!(
                detail.as_deref(),
                Some("If an export or import was running, run it again."),
                "{state}"
            );
            // The record is only renamed: the same bytes, beside the other files.
            let aside: Vec<_> = fs::read_dir(app_data)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .filter(|path| {
                    path.file_name()
                        .unwrap()
                        .to_string_lossy()
                        .starts_with("backup-operation.unreadable-")
                })
                .collect();
            assert_eq!(aside.len(), 1, "{state}: {aside:?}");
            assert_eq!(fs::read_to_string(&aside[0]).unwrap(), text, "{state}");
        }
    }

    #[test]
    fn a_journal_is_unusable_only_when_it_was_read_and_cannot_be_used() {
        let _test_state = crate::test_support::global_state();
        let cases = [
            ("damaged", "{not json".to_string()),
            ("empty", String::new()),
            ("not a journal", "[1, 2, 3]".to_string()),
            ("another version", journal_text(2, None)),
            (
                "unknown content",
                journal_text(1, Some(("unknown", true.into()))),
            ),
            (
                "invalid identity",
                journal_text(1, None).replace(JOURNAL_ID, "not-an-identity"),
            ),
        ];
        for (state, text) in cases {
            let directory = tempfile::tempdir().unwrap();
            let app_data = directory.path();
            fs::write(app_data.join("backup-operation.json"), &text).unwrap();
            let history = app_data.join("backup-history.json");
            // Read, and refused by what it holds: the kind says it may be set aside.
            assert!(
                matches!(try_load(&history), Err(LoadFailure::Unusable(_))),
                "{state}"
            );
            assert!(
                matches!(journal_state(app_data), JournalState::Unreadable),
                "{state}"
            );
            // The message is the same one `load` reports.
            assert_eq!(
                load(&history).err().unwrap(),
                try_load(&history).err().unwrap().into_message(),
                "{state}"
            );
        }
        // No file is no journal, and a file that could not be read at all (here a folder in
        // its place) is not known to be unusable.
        let directory = tempfile::tempdir().unwrap();
        let app_data = directory.path();
        let history = app_data.join("backup-history.json");
        assert!(try_load(&history).unwrap().is_none());
        fs::create_dir(app_data.join("backup-operation.json")).unwrap();
        assert!(matches!(try_load(&history), Err(LoadFailure::Io(_))));
        assert!(matches!(journal_state(app_data), JournalState::Unavailable));
    }

    #[cfg(unix)]
    #[test]
    fn a_journal_that_cannot_be_read_is_not_unusable_and_stays_where_it_is() {
        use std::os::unix::fs::PermissionsExt;
        let _test_state = crate::test_support::global_state();
        // Content that would be set aside if it could be read: the kind of failure decides,
        // not what the file holds.
        for text in ["{not json".to_string(), journal_text(2, None)] {
            let directory = tempfile::tempdir().unwrap();
            let app_data = directory.path();
            let journal = app_data.join("backup-operation.json");
            fs::write(&journal, &text).unwrap();
            fs::set_permissions(&journal, fs::Permissions::from_mode(0o000)).unwrap();
            let readable = fs::read(&journal).is_ok();
            let loaded = try_load(&app_data.join("backup-history.json"));
            let state = journal_state(app_data);
            fs::set_permissions(&journal, fs::Permissions::from_mode(0o600)).unwrap();
            // The owner can read it anyway (running as root): nothing to check.
            if readable {
                return;
            }
            assert!(matches!(loaded, Err(LoadFailure::Io(_))));
            assert!(matches!(state, JournalState::Unavailable));
            assert_eq!(fs::read_to_string(&journal).unwrap(), text);
            assert_eq!(fs::read_dir(app_data).unwrap().count(), 1);
        }
    }

    #[test]
    fn nothing_is_set_aside_at_startup_when_there_is_no_record_to_rename() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        assert!(set_aside_at_startup(&directory.path().join("backup-history.json")).is_err());
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
    }

    #[test]
    fn a_finished_export_file_is_kept_and_reported_when_the_upgrade_interrupts_nothing_else() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let app_data = directory.path();
        let paths = previous_generation(app_data, &[("dev", OLD_COMPUTER)]);
        let exports = app_data.join("exports");
        fs::create_dir_all(&exports).unwrap();
        let archive = exports.join("dev.silo-backup");
        backup::write_finished_export(&archive);
        let finished = fs::read(&archive).unwrap();
        let (controller, journal) = released_journal(
            app_data,
            &archive,
            released_export(serde_json::json!([["dev", OLD_COMPUTER]])),
        );
        let before = tree(&app_data.join("runtime"));
        let (outcome, title, message, detail) = settle(&paths, &controller, &journal);
        assert_eq!((outcome, title.as_str()), ("success", "Export complete"));
        assert_eq!(message, "Silo verified this export after relaunching.");
        assert_eq!(detail, None);
        assert_eq!(fs::read(&archive).unwrap(), finished);
        assert_eq!(tree(&app_data.join("runtime")), before);
        assert!(!pending(&controller).unwrap());
    }

    #[test]
    fn an_export_file_that_cannot_be_verified_is_kept_before_the_upgrade() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let app_data = directory.path();
        let paths = previous_generation(app_data, &[("dev", OLD_COMPUTER)]);
        let archive = app_data.join("dev.silo-backup");
        fs::write(&archive, b"half of an export").unwrap();
        let (controller, journal) =
            released_journal(app_data, &archive, released_export(serde_json::json!([])));
        let (outcome, title, message, detail) = settle(&paths, &controller, &journal);
        assert_eq!(
            (outcome, title.as_str()),
            ("failed", "Export interrupted before the upgrade")
        );
        assert!(
            message.contains("could not be verified and was kept"),
            "{message}"
        );
        assert_eq!(detail.as_deref(), Some("Export the computer again."));
        assert_eq!(fs::read(&archive).unwrap(), b"half of an export");
    }

    #[cfg(unix)]
    #[test]
    fn files_outside_the_previous_generation_that_cannot_be_removed_never_keep_the_migration_waiting(
    ) {
        use std::os::unix::fs::PermissionsExt;
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let app_data = directory.path();
        let paths = previous_generation(app_data, &[("dev", OLD_COMPUTER)]);
        let exports = app_data.join("exports");
        fs::create_dir_all(&exports).unwrap();
        let (controller, journal) = released_journal(
            app_data,
            &exports.join("dev.silo-backup"),
            released_export(serde_json::json!([])),
        );
        // An export folder that can no longer be read, such as a revoked permission.
        fs::set_permissions(&exports, fs::Permissions::from_mode(0o000)).unwrap();
        let settled = settle_before_migration(
            &paths,
            &controller,
            &journal,
            &backup::Cancellation::default(),
        );
        fs::set_permissions(&exports, fs::Permissions::from_mode(0o700)).unwrap();
        let Settlement::Settled(settled) = settled.expect("the journal settles anyway") else {
            panic!("an export that left no capture has nothing to wait for");
        };
        let (outcome, _, _, detail) = result_of(&settled);
        assert_eq!(outcome, "failed");
        assert_eq!(
            detail.as_deref(),
            Some("No export file was saved. Some files it left in the export folder could not be removed. Export the computer again.")
        );
    }
}
