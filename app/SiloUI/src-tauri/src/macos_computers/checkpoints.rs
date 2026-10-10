//! Checkpoints of macOS computers: a copy of a computer's disk and auxiliary storage
//! (APFS clones) and, for a running computer, the memory the framework saved while it was
//! paused. Nothing here touches Virtualization.framework: the machine is reached through
//! `Machine`, so the workflows run on every platform.
//!
//! A checkpoint is a folder under the computer's own folder:
//!
//! | File | Contents |
//! | --- | --- |
//! | `checkpoint.json` | Name, time, kind (memory or disk), why it was made, the Mac's build |
//! | `disk.img`, `auxiliary-storage.img` | Clones of the computer's files when it was made |
//! | `state.vzvmsave` | The saved memory of a memory checkpoint (mode 0600) |
//!
//! Folders are built under a partial name and renamed, so an interrupted run leaves nothing
//! that looks like a checkpoint.
use super::store::{self, Layout};
use super::templates::{self, CopyError};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write as _,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

const SCHEMA_VERSION: u32 = 1;
const META: &str = "checkpoint.json";
const DISK: &str = "disk.img";
const AUXILIARY_STORAGE: &str = "auxiliary-storage.img";
pub(super) const STATE: &str = "state.vzvmsave";
const PARTIAL: &str = ".partial-";
const FOLDER: &str = "checkpoints";
/// The name of the checkpoint Restore saves first.
pub(super) const RECOVERY_NAME: &str = "Before restore";
const MAX_NAME_CHARS: usize = 80;
/// Where a fork finds the credentials of the guest it was copied from, until its own are set.
pub(super) const INHERITED_ACCESS: &str = "inherited-access";
const ACCESS_FILES: [&str; 3] = ["password", "id_ed25519", "id_ed25519.pub"];

pub(super) const BUSY: &str =
    "A checkpoint operation is in progress on this computer. Wait for it to finish.";
pub(super) const NOT_READY: &str =
    "Checkpoints need a computer whose setup has finished. Use Retry setup first.";
pub(super) const CANCELLED: &str = "The checkpoint operation was cancelled.";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum Kind {
    /// Disk, auxiliary storage and the memory of a running computer.
    Memory,
    /// Disk and auxiliary storage only.
    Disk,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Reason {
    Manual,
    /// Saved by Restore before it replaced the computer's files.
    BeforeRestore,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Meta {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub kind: Kind,
    pub reason: Reason,
    /// The macOS the computer had, as `Record::os_version` words it.
    #[serde(default)]
    pub macos_version: Option<String>,
    /// The Mac's own build when the memory was saved: the framework rejects a state after
    /// some host updates, so a different build is not even tried.
    #[serde(default)]
    pub host_build: String,
    /// Bytes of the saved memory; the clones share their blocks with the computer.
    #[serde(default)]
    pub size_bytes: Option<u64>,
}

/// A checkpoint as the UI lists it.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Summary {
    id: String,
    name: String,
    created_at: String,
    /// `full` includes memory, `disk` does not: the words the Linux checkpoint list uses.
    scope: &'static str,
    reason: Reason,
    #[serde(skip_serializing_if = "Option::is_none")]
    size_bytes: Option<u64>,
}

impl Meta {
    pub(super) fn summary(&self) -> Summary {
        Summary {
            id: self.id.clone(),
            name: self.name.clone(),
            created_at: self.created_at.clone(),
            scope: match self.kind {
                Kind::Memory => "full",
                Kind::Disk => "disk",
            },
            reason: self.reason,
            size_bytes: self.size_bytes,
        }
    }
}

/// What the next Start of a restored computer does first. Persisted in the computer's record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PendingRestore {
    pub checkpoint_id: String,
    /// The checkpoint holds memory, which Start restores when it still can.
    pub memory: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum OperationKind {
    Capture,
    Restore,
    Fork,
    Delete,
}

/// The checkpoint operation running on a computer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Operation {
    pub kind: OperationKind,
    pub status: &'static str,
    pub stage: String,
}

impl Operation {
    pub(super) fn running(kind: OperationKind, stage: &str) -> Self {
        Self {
            kind,
            status: "running",
            stage: stage.to_string(),
        }
    }
}

// MARK: Layout

pub(super) fn root(layout: &Layout) -> PathBuf {
    layout.dir.join(FOLDER)
}

pub(super) fn dir(layout: &Layout, id: &str) -> PathBuf {
    root(layout).join(id)
}

pub(super) fn state_file(layout: &Layout, id: &str) -> PathBuf {
    dir(layout, id).join(STATE)
}

fn partial_dir(layout: &Layout, id: &str) -> PathBuf {
    root(layout).join(format!("{PARTIAL}{id}"))
}

/// Whether `id` names a checkpoint folder and not a path.
fn valid_id(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok()
}

fn private_dir(path: &Path) -> Result<(), String> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
        .map_err(|error| store::io_error("create the checkpoint", &error))
}

pub(super) fn validate_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Enter a name for the checkpoint.".into());
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(format!(
            "A checkpoint name can have at most {MAX_NAME_CHARS} characters."
        ));
    }
    if name.chars().any(char::is_control) {
        return Err("A checkpoint name can't contain control characters.".into());
    }
    Ok(name.to_string())
}

/// The mode a saved state is written with. The framework creates the file, so this is
/// applied afterwards; the folder around it is already private.
pub(super) fn secure_state(path: &Path) -> Result<(), String> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|error| store::io_error("protect the saved memory", &error))
}

fn write_meta(dir: &Path, meta: &Meta) -> Result<(), String> {
    let json = serde_json::to_vec_pretty(meta).map_err(|error| error.to_string())?;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(dir.join(META))
        .and_then(|mut file| {
            file.write_all(&json)?;
            file.sync_all()
        })
        .map_err(|error| store::io_error("save the checkpoint", &error))
}

fn read_meta(dir: &Path) -> Option<Meta> {
    let meta: Meta = serde_json::from_slice(&fs::read(dir.join(META)).ok()?).ok()?;
    let folder = dir.file_name()?.to_str()?;
    (meta.schema_version == SCHEMA_VERSION && meta.id == folder && valid_id(&meta.id))
        .then_some(meta)
}

/// The usable checkpoints of a computer, newest first. A folder is usable when its record
/// reads and its disk and auxiliary storage are there (and the memory, for a memory checkpoint).
pub(super) fn list(layout: &Layout) -> Vec<Meta> {
    let Ok(entries) = fs::read_dir(root(layout)) else {
        return Vec::new();
    };
    let mut found: Vec<Meta> = entries
        .flatten()
        .filter_map(|entry| read_meta(&entry.path()))
        .filter(|meta| complete(layout, meta))
        .collect();
    found.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
    found
}

fn complete(layout: &Layout, meta: &Meta) -> bool {
    let dir = dir(layout, &meta.id);
    dir.join(DISK).is_file()
        && dir.join(AUXILIARY_STORAGE).is_file()
        && (meta.kind == Kind::Disk || dir.join(STATE).is_file())
}

pub(super) fn find(layout: &Layout, id: &str) -> Result<Meta, String> {
    list(layout)
        .into_iter()
        .find(|meta| meta.id == id)
        .ok_or_else(|| "This checkpoint no longer exists.".to_string())
}

/// Removes what an interrupted run left: partial folders and folders without a readable record.
pub(super) fn sweep(layout: &Layout) {
    let Ok(entries) = fs::read_dir(root(layout)) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let partial = entry.file_name().to_string_lossy().starts_with(PARTIAL);
        if path.is_dir() && (partial || read_meta(&path).is_none()) {
            let _ = fs::remove_dir_all(&path);
        }
    }
}

/// Deletes one checkpoint. The record goes first, so a checkpoint cut short is never listed.
pub(super) fn remove(layout: &Layout, id: &str) -> Result<(), String> {
    if !valid_id(id) {
        return Err("This checkpoint no longer exists.".into());
    }
    let dir = dir(layout, id);
    match fs::remove_file(dir.join(META)) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(store::io_error("delete the checkpoint", &error)),
    }
    match fs::remove_dir_all(&dir) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(store::io_error("delete the checkpoint", &error)),
    }
}

fn copy_error(error: CopyError) -> String {
    error.message()
}

/// Clones the computer's disk and auxiliary storage into `dir`.
fn clone_files(layout: &Layout, dir: &Path) -> Result<(), String> {
    templates::clone_file(&layout.disk(), &dir.join(DISK)).map_err(copy_error)?;
    templates::clone_file(&layout.auxiliary_storage(), &dir.join(AUXILIARY_STORAGE))
        .map_err(copy_error)
}

pub(super) fn host_build() -> String {
    let mut buffer = [0u8; 64];
    let mut size = buffer.len();
    // SAFETY: `buffer` and `size` describe the output buffer sysctlbyname fills.
    let status = unsafe {
        libc::sysctlbyname(
            c"kern.osversion".as_ptr(),
            buffer.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if status != 0 {
        return String::new();
    }
    String::from_utf8_lossy(&buffer[..size.min(buffer.len())])
        .trim_end_matches('\0')
        .to_string()
}

fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

// MARK: Machine

/// What the workflows need from a computer's machine.
pub(super) trait Machine {
    /// Whether the running machine's memory can be saved, or why not.
    fn memory_support(&self) -> Result<(), String>;
    /// Pauses the running machine, saves its memory to `state`, runs `copy` while it is
    /// paused and resumes it. The machine is resumed whatever happened.
    fn save_running(
        &self,
        state: &Path,
        copy: &mut dyn FnMut() -> Result<(), String>,
    ) -> Result<(), String>;
    /// Turns the machine off at once and returns when it has stopped.
    fn force_stop(&self) -> Result<(), String>;
    /// Names the step under way, for the UI.
    fn stage(&self, stage: &str);
    /// Writes a line to the computer's log.
    fn log(&self, line: &str);
    /// Whether the operation was asked to end.
    fn cancelled(&self) -> bool;
}

/// A computer a workflow acts on.
pub(super) struct Subject<'a, M: Machine> {
    pub layout: &'a Layout,
    pub machine: &'a M,
    pub running: bool,
    pub macos_version: Option<String>,
}

// MARK: Create

/// Saves a checkpoint of the computer: memory too when it runs, the disk alone when it is
/// stopped. A running computer whose memory can't be saved gets no checkpoint.
pub(super) fn create<M: Machine>(
    subject: &Subject<'_, M>,
    name: &str,
    reason: Reason,
) -> Result<Meta, String> {
    let name = validate_name(name)?;
    let machine = subject.machine;
    let layout = subject.layout;
    if subject.running {
        machine.memory_support().map_err(|why| {
            format!("This computer's memory can't be saved ({why}). Stop it to save a checkpoint of its disk.")
        })?;
    }
    let id = uuid::Uuid::new_v4().to_string();
    let partial = partial_dir(layout, &id);
    let outcome = build(subject, &partial, &id, name, reason);
    match outcome {
        Ok(meta) => {
            fs::rename(&partial, dir(layout, &id)).map_err(|error| {
                let _ = fs::remove_dir_all(&partial);
                store::io_error("save the checkpoint", &error)
            })?;
            store::sync_dir(&root(layout))
                .map_err(|error| store::io_error("save the checkpoint", &error))?;
            machine.log(&format!(
                "checkpoint saved: {} ({} {})",
                meta.name,
                match meta.kind {
                    Kind::Memory => "memory",
                    Kind::Disk => "disk",
                },
                meta.id
            ));
            Ok(meta)
        }
        Err(message) => {
            let _ = fs::remove_dir_all(&partial);
            machine.log(&format!("checkpoint failed: {message}"));
            Err(message)
        }
    }
}

fn build<M: Machine>(
    subject: &Subject<'_, M>,
    partial: &Path,
    id: &str,
    name: String,
    reason: Reason,
) -> Result<Meta, String> {
    let machine = subject.machine;
    private_dir(partial)?;
    let mut size_bytes = None;
    if subject.running {
        let state = partial.join(STATE);
        machine.stage("Pausing the computer and saving its memory");
        let mut copy = || {
            machine.stage("Copying the disk");
            clone_files(subject.layout, partial)
        };
        machine.save_running(&state, &mut copy)?;
        secure_state(&state)?;
        size_bytes = fs::metadata(&state).ok().map(|meta| meta.len());
    } else {
        machine.stage("Copying the disk");
        clone_files(subject.layout, partial)?;
    }
    if machine.cancelled() {
        return Err(CANCELLED.into());
    }
    let meta = Meta {
        schema_version: SCHEMA_VERSION,
        id: id.to_string(),
        name,
        created_at: now(),
        kind: if subject.running {
            Kind::Memory
        } else {
            Kind::Disk
        },
        reason,
        macos_version: subject.macos_version.clone(),
        host_build: host_build(),
        size_bytes,
    };
    write_meta(partial, &meta)?;
    // Everything is on disk before the folder gets its final name, and so before a Restore
    // may replace the computer's files with it.
    let mut files = vec![DISK, AUXILIARY_STORAGE];
    if subject.running {
        files.push(STATE);
    }
    for name in files {
        fs::File::open(partial.join(name))
            .and_then(|file| file.sync_all())
            .map_err(|error| store::io_error("save the checkpoint", &error))?;
    }
    store::sync_dir(partial).map_err(|error| store::io_error("save the checkpoint", &error))?;
    Ok(meta)
}

// MARK: Restore

/// Rewinds the computer to checkpoint `target`: a recovery checkpoint of the current state
/// first (memory too when it runs), then it is turned off and its disk and auxiliary storage
/// are replaced. Returns what the next Start should do. The computer stays stopped.
pub(super) fn restore<M: Machine>(
    subject: &Subject<'_, M>,
    target: &str,
) -> Result<PendingRestore, String> {
    let machine = subject.machine;
    let layout = subject.layout;
    let target = find(layout, target)?;
    machine.stage("Saving a recovery checkpoint");
    create(subject, RECOVERY_NAME, Reason::BeforeRestore).map_err(|message| {
        format!("Nothing was changed. The recovery checkpoint could not be saved: {message}")
    })?;
    if machine.cancelled() {
        return Err(CANCELLED.into());
    }
    if subject.running {
        machine.stage("Stopping the computer");
        machine.force_stop()?;
    }
    machine.stage("Restoring the disk");
    let pending = PendingRestore {
        checkpoint_id: target.id.clone(),
        memory: target.kind == Kind::Memory,
    };
    swap_files(layout, &dir(layout, &target.id), &pending)?;
    machine.log(&format!(
        "restored checkpoint: {} ({})",
        target.name, target.id
    ));
    Ok(pending)
}

// MARK: Restore journal

const JOURNAL: &str = "restore-journal.json";
const UNREADABLE_JOURNAL: &str = "The journal of an unfinished Restore can't be read. Silo kept its files and will not start this computer. Contact support or delete the computer.";
const RESTORING: &str = ".restoring";

/// How far a Restore got in replacing the computer's files.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Phase {
    /// The clones are being made under temporary names; the live files are untouched.
    Staging,
    /// Both clones are complete; the live files are being replaced by renaming them.
    Swapping,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Journal {
    phase: Phase,
    target: String,
    memory: bool,
}

fn journal_path(layout: &Layout) -> PathBuf {
    layout.dir.join(JOURNAL)
}

/// Whether a Restore began and has not been rolled forward or back and finished.
pub(super) fn restore_unfinished(layout: &Layout) -> bool {
    // Only a journal that is certainly absent counts as no journal.
    !matches!(
        fs::metadata(journal_path(layout)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound
    )
}

fn write_journal(
    layout: &Layout,
    journal: &Journal,
    sync: &dyn Fn(&Path) -> std::io::Result<()>,
) -> Result<(), String> {
    let json = serde_json::to_vec(journal).map_err(|error| error.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(&layout.dir)
        .map_err(|error| store::io_error("restore the computer's files", &error))?;
    file.write_all(&json)
        .map_err(|error| store::io_error("restore the computer's files", &error))?;
    file.as_file()
        .sync_all()
        .map_err(|error| store::io_error("restore the computer's files", &error))?;
    file.persist(journal_path(layout))
        .map_err(|error| store::io_error("restore the computer's files", &error.error))?;
    sync(&layout.dir).map_err(|error| store::io_error("restore the computer's files", &error))
}

/// The phase the journal on disk is in, if it can be read.
fn published_phase(layout: &Layout) -> Option<Phase> {
    serde_json::from_slice::<Journal>(&fs::read(journal_path(layout)).ok()?)
        .ok()
        .map(|journal| journal.phase)
}

/// The live files and the temporary names their replacements are staged under.
fn staged(layout: &Layout) -> [(PathBuf, PathBuf); 2] {
    [layout.disk(), layout.auxiliary_storage()].map(|live| {
        let mut name = live.as_os_str().to_os_string();
        name.push(RESTORING);
        (live, PathBuf::from(name))
    })
}

fn remove_staged(layout: &Layout) {
    for (_, temporary) in staged(layout) {
        let _ = fs::remove_file(temporary);
    }
}

fn rename_staged(layout: &Layout) -> Result<(), String> {
    for (live, temporary) in staged(layout) {
        if temporary.exists() {
            fs::rename(&temporary, &live)
                .map_err(|error| store::io_error("restore the computer's files", &error))?;
        }
    }
    store::sync_dir(&layout.dir)
        .map_err(|error| store::io_error("restore the computer's files", &error))
}

/// Replaces the computer's disk and auxiliary storage with clones of the checkpoint's, as a
/// journaled step: both clones are staged and complete before the journal says `swapping`,
/// and from then on `recover` can finish the renames after an interruption. The journal
/// stays until `finish_restore`, which the caller runs once the new pending Restore is saved.
fn swap_files(layout: &Layout, from: &Path, pending: &PendingRestore) -> Result<(), String> {
    swap_files_with(layout, from, pending, &store::sync_dir)
}

fn swap_files_with(
    layout: &Layout,
    from: &Path,
    pending: &PendingRestore,
    sync: &dyn Fn(&Path) -> std::io::Result<()>,
) -> Result<(), String> {
    let mut journal = Journal {
        phase: Phase::Staging,
        target: pending.checkpoint_id.clone(),
        memory: pending.memory,
    };
    write_journal(layout, &journal, sync)?;
    remove_staged(layout);
    for ((_, temporary), source) in staged(layout).iter().zip([DISK, AUXILIARY_STORAGE]) {
        if let Err(error) = templates::clone_file(&from.join(source), temporary) {
            remove_staged(layout);
            let _ = fs::remove_file(journal_path(layout));
            return Err(copy_error(error));
        }
    }
    journal.phase = Phase::Swapping;
    // The staged files are on disk before the journal says that they are complete.
    let durable = staged(layout).iter().try_for_each(|(_, temporary)| {
        fs::File::open(temporary)
            .and_then(|file| file.sync_all())
            .map_err(|error| store::io_error("restore the computer's files", &error))
    });
    if let Err(message) = durable.and_then(|()| write_journal(layout, &journal, sync)) {
        // Once `swapping` is published, even if making it durable failed, the staged clones
        // are what recovery rolls forward with: they and the journal stay.
        if published_phase(layout) != Some(Phase::Swapping) {
            remove_staged(layout);
            let _ = fs::remove_file(journal_path(layout));
        }
        return Err(message);
    }
    // A failure from here on is finished by `recover`, never by undoing half of it.
    rename_staged(layout)
}

/// Settles a Restore that was interrupted. Before the clones were complete nothing is
/// changed (the staged files are dropped); after, the renames are finished. Returns the
/// pending Restore the journal names when the files now are the checkpoint's, which the
/// caller saves in the record before calling `finish_restore`.
pub(super) fn recover(layout: &Layout) -> Result<Option<PendingRestore>, String> {
    let bytes = match fs::read(journal_path(layout)) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(store::io_error("read the Restore journal", &error)),
    };
    match serde_json::from_slice::<Journal>(&bytes) {
        Ok(Journal {
            phase: Phase::Swapping,
            target,
            memory,
        }) => {
            rename_staged(layout)?;
            Ok(Some(PendingRestore {
                checkpoint_id: target,
                memory,
            }))
        }
        // Only a journal that positively says `staging` may be rolled back.
        Ok(Journal {
            phase: Phase::Staging,
            ..
        }) => {
            remove_staged(layout);
            finish_restore(layout);
            Ok(None)
        }
        // A journal that can't be understood keeps every file of the transaction, and keeps
        // the computer from starting, until someone can look at it.
        Err(_) => Err(UNREADABLE_JOURNAL.into()),
    }
}

/// Ends the journal once the record names the Restore the files already carry out.
pub(super) fn finish_restore(layout: &Layout) {
    let _ = fs::remove_file(journal_path(layout));
    let _ = store::sync_dir(&layout.dir);
}

/// Whether an unfinished Restore names checkpoint `id` (or can't say which it names): that
/// checkpoint is what the computer's files are being made into, so it can't be deleted.
pub(super) fn journal_pins(layout: &Layout, id: &str) -> bool {
    match fs::read(journal_path(layout)) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(_) => true,
        Ok(bytes) => serde_json::from_slice::<Journal>(&bytes).map_or(true, |j| j.target == id),
    }
}

// MARK: Start

/// How the next Start of a computer begins.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum StartPlan {
    /// Boot from the disk.
    Boot,
    /// Restore the saved memory, then resume.
    RestoreMemory { state: PathBuf },
    /// Boot from the disk and tell the user why the saved memory was not used.
    BootBecause(String),
}

/// Decides how to start a computer that may have just been restored.
pub(super) fn start_plan(
    layout: &Layout,
    pending: Option<&PendingRestore>,
    current_build: &str,
) -> StartPlan {
    let Some(pending) = pending.filter(|pending| pending.memory) else {
        return StartPlan::Boot;
    };
    let Ok(meta) = find(layout, &pending.checkpoint_id) else {
        return StartPlan::BootBecause(
            "The checkpoint's memory is gone, so the computer started from its disk.".into(),
        );
    };
    if meta.host_build != current_build {
        return StartPlan::BootBecause(
            "This Mac was updated after the checkpoint was saved, so macOS can't restore its memory. The computer started from the checkpoint's disk instead.".into(),
        );
    }
    StartPlan::RestoreMemory {
        state: state_file(layout, &meta.id),
    }
}

/// The note shown when macOS refused a saved memory that looked restorable.
pub(super) fn rejected_note(why: &str) -> String {
    format!("macOS could not restore the checkpoint's memory ({why}). The computer started from the checkpoint's disk instead.")
}

// MARK: Fork

/// Clones a checkpoint's disk and auxiliary storage, and the hardware model of the computer
/// it belongs to, into the folder of a new computer, with its own machine identifier. The
/// credentials of the source go to `inherited-access/` so the first login can still use
/// them. Everything is removed again if a step fails.
pub(super) fn clone_for_fork(
    source: &Layout,
    checkpoint: &str,
    target: &Layout,
    machine_identifier: &[u8],
) -> Result<(), CopyError> {
    let from = dir(source, checkpoint);
    let copied = (|| {
        fs::create_dir_all(&target.dir)
            .map_err(|error| CopyError::Failed(store::io_error("create the computer", &error)))?;
        for (from, to) in [
            (from.join(DISK), target.disk()),
            (from.join(AUXILIARY_STORAGE), target.auxiliary_storage()),
            (source.hardware_model(), target.hardware_model()),
        ] {
            templates::clone_file(&from, &to)?;
        }
        fs::write(target.machine_identifier(), machine_identifier).map_err(|error| {
            CopyError::Failed(store::io_error("save the computer's identity", &error))
        })?;
        inherit_access(source, target)
    })();
    if copied.is_err() {
        for file in [
            target.disk(),
            target.auxiliary_storage(),
            target.hardware_model(),
            target.machine_identifier(),
        ] {
            let _ = fs::remove_file(file);
        }
        let _ = fs::remove_dir_all(target.dir.join(INHERITED_ACCESS));
    }
    copied
}

/// Copies the source's password and key, which the checkpoint's disk still accepts.
fn inherit_access(source: &Layout, target: &Layout) -> Result<(), CopyError> {
    let to = target.dir.join(INHERITED_ACCESS);
    let failed = |error: std::io::Error| {
        CopyError::Failed(store::io_error("copy the computer's access", &error))
    };
    private_dir(&to).map_err(CopyError::Failed)?;
    let from = super::guest_access::credentials_dir(source);
    for name in ACCESS_FILES {
        let bytes = fs::read(from.join(name)).map_err(failed)?;
        fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(to.join(name))
            .and_then(|mut file| file.write_all(&bytes))
            .map_err(failed)?;
    }
    Ok(())
}

/// Where the first login to a fork reads its credentials from.
pub(super) fn inherited_access(layout: &Layout) -> PathBuf {
    layout.dir.join(INHERITED_ACCESS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Fake {
        calls: RefCell<Vec<String>>,
        memory_unsupported: Option<&'static str>,
        fail_save: bool,
        fail_copy_after_save: bool,
        cancel_after_save: bool,
        fail_stop: bool,
    }

    impl Fake {
        fn calls(&self) -> Vec<String> {
            self.calls.borrow().clone()
        }
        fn note(&self, call: &str) {
            self.calls.borrow_mut().push(call.to_string());
        }
    }

    impl Machine for Fake {
        fn memory_support(&self) -> Result<(), String> {
            self.memory_unsupported
                .map_or(Ok(()), |why| Err(why.to_string()))
        }
        fn save_running(
            &self,
            state: &Path,
            copy: &mut dyn FnMut() -> Result<(), String>,
        ) -> Result<(), String> {
            self.note("pause");
            let result = (|| {
                if self.fail_save {
                    return Err("the framework refused".to_string());
                }
                fs::write(state, b"memory").unwrap();
                self.note("save");
                if self.fail_copy_after_save {
                    return Err("copy failed".to_string());
                }
                copy()
            })();
            self.note("resume");
            result
        }
        fn force_stop(&self) -> Result<(), String> {
            self.note("force-stop");
            if self.fail_stop {
                Err("busy".into())
            } else {
                Ok(())
            }
        }
        fn stage(&self, stage: &str) {
            self.note(&format!("stage: {stage}"));
        }
        fn log(&self, _line: &str) {}
        fn cancelled(&self) -> bool {
            self.cancel_after_save && self.calls.borrow().iter().any(|call| call == "resume")
        }
    }

    fn computer() -> (tempfile::TempDir, Layout) {
        let data = tempfile::tempdir().unwrap();
        let layout = Layout::new(data.path(), "computer");
        fs::create_dir_all(&layout.dir).unwrap();
        fs::write(layout.disk(), b"disk-v1").unwrap();
        fs::write(layout.auxiliary_storage(), b"aux-v1").unwrap();
        fs::write(layout.hardware_model(), b"model").unwrap();
        (data, layout)
    }

    fn subject<'a>(layout: &'a Layout, machine: &'a Fake, running: bool) -> Subject<'a, Fake> {
        Subject {
            layout,
            machine,
            running,
            macos_version: Some("26.6.2 (25G83)".into()),
        }
    }

    fn folder_names(layout: &Layout) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(root(layout))
            .map(|entries| {
                entries
                    .flatten()
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    #[test]
    fn a_stopped_computer_gets_a_disk_checkpoint_with_private_files() {
        let (_data, layout) = computer();
        let machine = Fake::default();
        let meta = create(
            &subject(&layout, &machine, false),
            "  First  ",
            Reason::Manual,
        )
        .unwrap();
        assert_eq!(meta.name, "First");
        assert_eq!(meta.kind, Kind::Disk);
        assert_eq!(meta.size_bytes, None);
        let folder = dir(&layout, &meta.id);
        assert_eq!(fs::read(folder.join(DISK)).unwrap(), b"disk-v1");
        assert_eq!(fs::read(folder.join(AUXILIARY_STORAGE)).unwrap(), b"aux-v1");
        assert!(!folder.join(STATE).exists());
        let mode = |path: PathBuf| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(folder.clone()), 0o700);
        assert_eq!(mode(folder.join(META)), 0o600);
        assert_eq!(folder_names(&layout), [meta.id.clone()]);
        assert_eq!(list(&layout), [meta]);
        // Nothing was paused for a stopped computer.
        assert!(!machine.calls().contains(&"pause".to_string()));
    }

    #[test]
    fn a_running_computer_is_paused_saved_copied_and_resumed_in_that_order() {
        let (_data, layout) = computer();
        let machine = Fake::default();
        let meta = create(&subject(&layout, &machine, true), "Live", Reason::Manual).unwrap();
        assert_eq!(meta.kind, Kind::Memory);
        assert_eq!(meta.size_bytes, Some(6));
        let calls: Vec<String> = machine
            .calls()
            .into_iter()
            .filter(|call| !call.starts_with("stage"))
            .collect();
        assert_eq!(calls, ["pause", "save", "resume"]);
        let state = state_file(&layout, &meta.id);
        assert_eq!(
            fs::metadata(&state).unwrap().permissions().mode() & 0o777,
            0o600
        );
        // The disk was copied while the machine was paused: the copy stage sits between.
        let stages = machine.calls();
        let copy = stages
            .iter()
            .position(|c| c == "stage: Copying the disk")
            .unwrap();
        assert!(copy > stages.iter().position(|c| c == "save").unwrap());
        assert!(copy < stages.iter().position(|c| c == "resume").unwrap());
    }

    #[test]
    fn a_failure_resumes_the_machine_and_leaves_no_partial_checkpoint() {
        for (fake, expected) in [
            (
                Fake {
                    fail_save: true,
                    ..Fake::default()
                },
                "the framework refused",
            ),
            (
                Fake {
                    fail_copy_after_save: true,
                    ..Fake::default()
                },
                "copy failed",
            ),
        ] {
            let (_data, layout) = computer();
            let error = create(&subject(&layout, &fake, true), "Live", Reason::Manual).unwrap_err();
            assert_eq!(error, expected);
            assert_eq!(fake.calls().last().map(String::as_str), Some("resume"));
            assert!(folder_names(&layout).is_empty());
            assert!(list(&layout).is_empty());
        }
    }

    #[test]
    fn a_cancelled_capture_is_removed() {
        let (_data, layout) = computer();
        let machine = Fake {
            cancel_after_save: true,
            ..Fake::default()
        };
        let error = create(&subject(&layout, &machine, true), "Live", Reason::Manual).unwrap_err();
        assert_eq!(error, CANCELLED);
        assert!(folder_names(&layout).is_empty());
    }

    #[test]
    fn a_running_computer_without_save_support_explains_why_and_is_not_paused() {
        let (_data, layout) = computer();
        let machine = Fake {
            memory_unsupported: Some("Audio devices can't be saved."),
            ..Fake::default()
        };
        let error = create(&subject(&layout, &machine, true), "Live", Reason::Manual).unwrap_err();
        assert!(error.contains("Audio devices can't be saved."), "{error}");
        assert!(error.contains("Stop it"), "{error}");
        assert!(machine.calls().is_empty());
        // Stopped, the same computer still gets a disk checkpoint.
        let meta = create(&subject(&layout, &machine, false), "Disk", Reason::Manual).unwrap();
        assert_eq!(meta.kind, Kind::Disk);
    }

    #[test]
    fn names_are_trimmed_and_bounded() {
        assert_eq!(validate_name("  a  ").unwrap(), "a");
        assert!(validate_name("   ").is_err());
        assert!(validate_name(&"x".repeat(81)).is_err());
        assert!(validate_name("a\nb").is_err());
        assert!(validate_name(&"x".repeat(80)).is_ok());
    }

    #[test]
    fn restore_saves_the_recovery_checkpoint_before_it_changes_any_file() {
        let (_data, layout) = computer();
        let machine = Fake::default();
        let target = create(&subject(&layout, &machine, false), "Good", Reason::Manual).unwrap();
        fs::write(layout.disk(), b"disk-v2").unwrap();
        fs::write(layout.auxiliary_storage(), b"aux-v2").unwrap();

        let pending = restore(&subject(&layout, &machine, false), &target.id).unwrap();
        assert_eq!(
            pending,
            PendingRestore {
                checkpoint_id: target.id.clone(),
                memory: false
            }
        );
        assert_eq!(fs::read(layout.disk()).unwrap(), b"disk-v1");
        assert_eq!(fs::read(layout.auxiliary_storage()).unwrap(), b"aux-v1");
        let listed = list(&layout);
        let recovery = listed
            .iter()
            .find(|meta| meta.reason == Reason::BeforeRestore)
            .unwrap();
        assert_eq!(recovery.name, RECOVERY_NAME);
        // The recovery checkpoint holds the files as they were before the Restore.
        let folder = dir(&layout, &recovery.id);
        assert_eq!(fs::read(folder.join(DISK)).unwrap(), b"disk-v2");
        assert_eq!(fs::read(folder.join(AUXILIARY_STORAGE)).unwrap(), b"aux-v2");
        assert!(fs::read_dir(&layout.dir)
            .unwrap()
            .flatten()
            .all(|entry| !entry.file_name().to_string_lossy().ends_with(".restoring")));
    }

    #[test]
    fn restoring_a_running_computer_saves_its_memory_then_stops_it_then_replaces_files() {
        let (_data, layout) = computer();
        let stopped = Fake::default();
        let target = create(&subject(&layout, &stopped, false), "Good", Reason::Manual).unwrap();
        fs::write(layout.disk(), b"disk-v2").unwrap();
        let machine = Fake::default();
        restore(&subject(&layout, &machine, true), &target.id).unwrap();
        let calls: Vec<String> = machine
            .calls()
            .into_iter()
            .filter(|call| !call.starts_with("stage"))
            .collect();
        assert_eq!(calls, ["pause", "save", "resume", "force-stop"]);
        assert_eq!(fs::read(layout.disk()).unwrap(), b"disk-v1");
        let recovery = list(&layout)
            .into_iter()
            .find(|meta| meta.reason == Reason::BeforeRestore)
            .unwrap();
        assert_eq!(recovery.kind, Kind::Memory);
    }

    #[test]
    fn a_failed_recovery_checkpoint_stops_the_restore_before_anything_changes() {
        let (_data, layout) = computer();
        let stopped = Fake::default();
        let target = create(&subject(&layout, &stopped, false), "Good", Reason::Manual).unwrap();
        fs::write(layout.disk(), b"disk-v2").unwrap();
        let machine = Fake {
            fail_save: true,
            ..Fake::default()
        };
        let error = restore(&subject(&layout, &machine, true), &target.id).unwrap_err();
        assert!(error.starts_with("Nothing was changed."), "{error}");
        assert!(!machine.calls().contains(&"force-stop".to_string()));
        assert_eq!(fs::read(layout.disk()).unwrap(), b"disk-v2");
        assert_eq!(list(&layout).len(), 1);
    }

    #[test]
    fn a_restore_that_cannot_stop_the_computer_leaves_the_files_alone() {
        let (_data, layout) = computer();
        let stopped = Fake::default();
        let target = create(&subject(&layout, &stopped, false), "Good", Reason::Manual).unwrap();
        fs::write(layout.disk(), b"disk-v2").unwrap();
        let machine = Fake {
            fail_stop: true,
            ..Fake::default()
        };
        assert_eq!(
            restore(&subject(&layout, &machine, true), &target.id).unwrap_err(),
            "busy"
        );
        assert_eq!(fs::read(layout.disk()).unwrap(), b"disk-v2");
        // The recovery checkpoint stays, as the Linux Restore keeps its own.
        assert!(list(&layout)
            .iter()
            .any(|meta| meta.reason == Reason::BeforeRestore));
    }

    #[test]
    fn restoring_a_memory_checkpoint_asks_the_next_start_to_restore_the_memory() {
        let (_data, layout) = computer();
        let machine = Fake::default();
        let target = create(&subject(&layout, &machine, true), "Live", Reason::Manual).unwrap();
        let stopped = Fake::default();
        let pending = restore(&subject(&layout, &stopped, false), &target.id).unwrap();
        assert!(pending.memory);
        assert!(restore(
            &subject(&layout, &stopped, false),
            "9f3c2a3e-0000-4000-8000-000000000000"
        )
        .is_err());
    }

    #[test]
    fn start_plans_fall_back_to_the_disk_with_a_reason() {
        let (_data, layout) = computer();
        let machine = Fake::default();
        let live = create(&subject(&layout, &machine, true), "Live", Reason::Manual).unwrap();
        let build = host_build();
        let pending = PendingRestore {
            checkpoint_id: live.id.clone(),
            memory: true,
        };
        assert_eq!(start_plan(&layout, None, &build), StartPlan::Boot);
        let disk_only = PendingRestore {
            memory: false,
            ..pending.clone()
        };
        assert_eq!(
            start_plan(&layout, Some(&disk_only), &build),
            StartPlan::Boot
        );
        assert_eq!(
            start_plan(&layout, Some(&pending), &build),
            StartPlan::RestoreMemory {
                state: state_file(&layout, &live.id)
            }
        );
        let StartPlan::BootBecause(note) = start_plan(&layout, Some(&pending), "other-build")
        else {
            panic!("a different Mac build must not try the state");
        };
        assert!(note.contains("updated"), "{note}");
        fs::remove_file(state_file(&layout, &live.id)).unwrap();
        let StartPlan::BootBecause(note) = start_plan(&layout, Some(&pending), &build) else {
            panic!("a missing state falls back");
        };
        assert!(note.contains("gone"), "{note}");
        assert!(rejected_note("bad").contains("(bad)"));
    }

    #[test]
    fn delete_removes_the_record_first_and_ignores_ids_that_are_paths() {
        let (_data, layout) = computer();
        let machine = Fake::default();
        let meta = create(&subject(&layout, &machine, false), "One", Reason::Manual).unwrap();
        assert!(remove(&layout, "../computer").is_err());
        remove(&layout, &meta.id).unwrap();
        assert!(list(&layout).is_empty());
        assert!(!dir(&layout, &meta.id).exists());
        // Deleting again is not an error.
        remove(&layout, &meta.id).unwrap();
    }

    #[test]
    fn a_sweep_removes_partial_and_unreadable_folders_only() {
        let (_data, layout) = computer();
        let machine = Fake::default();
        let meta = create(&subject(&layout, &machine, false), "Keep", Reason::Manual).unwrap();
        fs::create_dir_all(root(&layout).join(".partial-x")).unwrap();
        fs::create_dir_all(root(&layout).join("no-record")).unwrap();
        sweep(&layout);
        assert_eq!(folder_names(&layout), [meta.id]);
    }

    #[test]
    fn a_checkpoint_missing_its_files_is_not_listed() {
        let (_data, layout) = computer();
        let machine = Fake::default();
        let live = create(&subject(&layout, &machine, true), "Live", Reason::Manual).unwrap();
        fs::remove_file(state_file(&layout, &live.id)).unwrap();
        assert!(list(&layout).is_empty());
    }

    #[test]
    fn a_fork_gets_cloned_files_a_new_identity_and_the_source_credentials() {
        let (data, source) = computer();
        let access = source.dir.join("guest-access");
        fs::create_dir_all(&access).unwrap();
        for name in ACCESS_FILES {
            fs::write(access.join(name), name).unwrap();
        }
        fs::write(source.machine_identifier(), b"id-source").unwrap();
        let machine = Fake::default();
        let meta = create(&subject(&source, &machine, false), "Base", Reason::Manual).unwrap();
        fs::write(source.disk(), b"disk-v2").unwrap();

        let target = Layout::new(data.path(), "fork");
        clone_for_fork(&source, &meta.id, &target, b"id-fork").unwrap();
        assert_eq!(fs::read(target.disk()).unwrap(), b"disk-v1");
        assert_eq!(fs::read(target.auxiliary_storage()).unwrap(), b"aux-v1");
        assert_eq!(fs::read(target.hardware_model()).unwrap(), b"model");
        assert_eq!(fs::read(target.machine_identifier()).unwrap(), b"id-fork");
        let inherited = inherited_access(&target);
        assert_eq!(
            fs::metadata(&inherited).unwrap().permissions().mode() & 0o777,
            0o700
        );
        for name in ACCESS_FILES {
            assert_eq!(fs::read_to_string(inherited.join(name)).unwrap(), name);
            assert_eq!(
                fs::metadata(inherited.join(name))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        // A memory state never moves to a new machine identifier.
        assert!(!target.dir.join(STATE).exists());
    }

    fn interrupted(phase: &str) -> (tempfile::TempDir, Layout, Meta) {
        let (data, layout) = computer();
        let machine = Fake::default();
        let meta = create(&subject(&layout, &machine, false), "Good", Reason::Manual).unwrap();
        fs::write(layout.disk(), b"disk-v2").unwrap();
        fs::write(layout.auxiliary_storage(), b"aux-v2").unwrap();
        for ((_, temporary), content) in staged(&layout).iter().zip(["disk-v1", "aux-v1"]) {
            fs::write(temporary, content).unwrap();
        }
        let journal = format!(
            r#"{{"phase":"{phase}","target":"{}","memory":false}}"#,
            meta.id
        );
        fs::write(journal_path(&layout), journal).unwrap();
        (data, layout, meta)
    }

    #[test]
    fn a_restore_finishes_with_its_journal_until_the_record_is_saved() {
        let (_data, layout) = computer();
        let machine = Fake::default();
        let target = create(&subject(&layout, &machine, false), "Good", Reason::Manual).unwrap();
        restore(&subject(&layout, &machine, false), &target.id).unwrap();
        assert!(restore_unfinished(&layout));
        finish_restore(&layout);
        assert!(!restore_unfinished(&layout));
        assert_eq!(recover(&layout), Ok(None));
    }

    #[test]
    fn an_interruption_while_staging_rolls_back() {
        let (_data, layout, _) = interrupted("staging");
        assert_eq!(recover(&layout), Ok(None));
        assert_eq!(fs::read(layout.disk()).unwrap(), b"disk-v2");
        assert_eq!(fs::read(layout.auxiliary_storage()).unwrap(), b"aux-v2");
        assert!(!restore_unfinished(&layout));
        assert!(staged(&layout)
            .iter()
            .all(|(_, temporary)| !temporary.exists()));
    }

    #[test]
    fn an_interruption_while_swapping_rolls_forward_to_matching_files_and_pending() {
        for already_renamed in [false, true] {
            let (_data, layout, meta) = interrupted("swapping");
            if already_renamed {
                let (live, temporary) = staged(&layout)[0].clone();
                fs::rename(temporary, live).unwrap();
            }
            let pending = recover(&layout).unwrap().unwrap();
            assert_eq!(pending.checkpoint_id, meta.id);
            assert_eq!(fs::read(layout.disk()).unwrap(), b"disk-v1");
            assert_eq!(fs::read(layout.auxiliary_storage()).unwrap(), b"aux-v1");
            // The journal stays until the record carries the pending Restore.
            assert!(restore_unfinished(&layout));
            assert_eq!(recover(&layout).unwrap().unwrap(), pending);
            finish_restore(&layout);
            assert_eq!(recover(&layout), Ok(None));
        }
    }

    #[test]
    fn an_unfinished_restore_pins_its_target() {
        let (_data, layout, meta) = interrupted("swapping");
        assert!(journal_pins(&layout, &meta.id));
        assert!(!journal_pins(&layout, "other"));
        fs::write(journal_path(&layout), b"{").unwrap();
        assert!(journal_pins(&layout, "other"));
        finish_restore(&layout);
        assert!(!journal_pins(&layout, &meta.id));
    }

    #[test]
    fn a_published_swapping_journal_and_its_staged_clones_survive_a_failed_sync() {
        let (_data, layout) = computer();
        let machine = Fake::default();
        let target = create(&subject(&layout, &machine, false), "Good", Reason::Manual).unwrap();
        fs::write(layout.disk(), b"disk-v2").unwrap();
        let calls = std::cell::Cell::new(0);
        // The first sync makes `staging` durable; the second, for `swapping`, fails.
        let sync = |_: &Path| {
            calls.set(calls.get() + 1);
            if calls.get() == 2 {
                Err(std::io::Error::other("sync failed"))
            } else {
                Ok(())
            }
        };
        let pending = PendingRestore {
            checkpoint_id: target.id.clone(),
            memory: false,
        };
        assert!(swap_files_with(&layout, &dir(&layout, &target.id), &pending, &sync).is_err());
        assert!(restore_unfinished(&layout));
        assert!(staged(&layout)
            .iter()
            .all(|(_, temporary)| temporary.exists()));
        assert_eq!(fs::read(layout.disk()).unwrap(), b"disk-v2");
        let rolled = recover(&layout).unwrap().unwrap();
        assert_eq!(rolled.checkpoint_id, target.id);
        assert_eq!(fs::read(layout.disk()).unwrap(), b"disk-v1");
    }

    #[test]
    fn only_a_missing_journal_means_no_restore_is_unfinished() {
        let (_data, layout) = computer();
        assert!(!restore_unfinished(&layout));
        assert!(!journal_pins(&layout, "any"));
        // A journal that can't be read is not a missing one.
        fs::create_dir(journal_path(&layout)).unwrap();
        assert!(restore_unfinished(&layout));
        assert!(journal_pins(&layout, "any"));
        assert!(recover(&layout).is_err());
    }

    #[test]
    fn an_unreadable_or_unknown_journal_keeps_every_file_and_blocks_the_computer() {
        for journal in [
            &b"{"[..],
            br#"{"phase":"later","target":"x","memory":false}"#,
        ] {
            let (_data, layout, _) = interrupted("swapping");
            fs::write(journal_path(&layout), journal).unwrap();
            assert_eq!(recover(&layout), Err(UNREADABLE_JOURNAL.to_string()));
            assert_eq!(fs::read(layout.disk()).unwrap(), b"disk-v2");
            assert!(staged(&layout)
                .iter()
                .all(|(_, temporary)| temporary.exists()));
            assert!(restore_unfinished(&layout));
            assert!(journal_pins(&layout, "anything"));
        }
    }

    #[test]
    fn a_failed_fork_removes_what_it_copied() {
        let (data, source) = computer();
        let machine = Fake::default();
        let meta = create(&subject(&source, &machine, false), "Base", Reason::Manual).unwrap();
        // No credentials exist, so the last step fails.
        let target = Layout::new(data.path(), "fork");
        assert!(clone_for_fork(&source, &meta.id, &target, b"id").is_err());
        assert!(!target.disk().exists());
        assert!(!target.machine_identifier().exists());
        assert!(!inherited_access(&target).exists());
    }

    #[test]
    fn summaries_use_the_linux_list_words() {
        let (_data, layout) = computer();
        let machine = Fake::default();
        let live = create(&subject(&layout, &machine, true), "Live", Reason::Manual).unwrap();
        let json = serde_json::to_value(live.summary()).unwrap();
        assert_eq!(json["scope"], "full");
        assert_eq!(json["reason"], "manual");
        assert_eq!(json["sizeBytes"], 6);
        assert!(json["createdAt"].as_str().is_some());
        let disk = create(
            &subject(&layout, &machine, false),
            "Disk",
            Reason::BeforeRestore,
        )
        .unwrap();
        let json = serde_json::to_value(disk.summary()).unwrap();
        assert_eq!(json["scope"], "disk");
        assert_eq!(json["reason"], "before-restore");
        assert!(json.get("sizeBytes").is_none());
    }
}
