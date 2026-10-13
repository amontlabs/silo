//! One-time conversion of saved data to the current vocabulary: what Silo calls a
//! *computer* (earlier *workspace*, *machine* and *sandbox*), a *device* (earlier *host*)
//! and a *connection* (earlier *remote management*). It runs once per channel at the first
//! launch after the update, before anything reads the converted files, so the normal code
//! reads only the current names. The converters here are the only code that still knows
//! the earlier ones. See `docs/SiloUI-VOCABULARY.md`.
//!
//! # What is converted
//!
//! Application settings (`appconfig/`):
//!
//! | File | Change |
//! | --- | --- |
//! | `settings.json` | settings `startWorkspacesAtLaunch`, `startupWorkspaceIds`, `sandboxOrder` become `startComputersAtLaunch`, `startupComputerIds`, `computerOrder`; in `onboardingDraft`: `machines`, `unfinishedMachineEditor`, `workspaceSelections`, `workspaceIdentities`, `workspaceRepositoryAccess` become `computers`, `unfinishedComputerEditor`, `computerSelections`, `computerIdentities`, `computerRepositoryAccess`; `currentStep` `"workspaces"` becomes `"computers"`; configurations lose `kind` and their `desktop.startWithSandbox` becomes `startWithComputer`; SSH configurations (`kind: "ssh"`), an unfinished SSH editor and the draft entries keyed by an SSH configuration are dropped |
//!
//! Application data (`appdata/`):
//!
//! | File | Change |
//! | --- | --- |
//! | `github.json` | `workspaces` becomes `computers`; `workspace` becomes `computer` in its entries and in `operations` |
//! | `secrets.json` | `secrets[].workspaces`, `secrets[].pendingWorkspaces` become `computers`, `pendingComputers`; `pendingRevocations[].workspace` becomes `computer` |
//! | `backup-operation.json` | `archive.sandboxes` and `request.machines` become `computers`; `request.pending_capture.workspaceId` becomes `computerId` |
//!
//! The storage generation Silo reads (the selected one, or `runtime/` before any is selected):
//!
//! | File | Change |
//! | --- | --- |
//! | `machines.json` | becomes `computers.json`; `machines` becomes `computers`; entries lose `kind`, SSH entries are dropped, `desktop.startWithSandbox` becomes `startWithComputer` |
//! | `sandbox-activity.json` | becomes `computer-activity.json`; `workspace` becomes `computer`, `machineId` becomes `computerId` |
//! | `setup-activity.json` | `workspace` becomes `computer`; `phase` `"workspaces"` becomes `"computers"`; steps `workspace-configuration`, `-verification`, `-removal`, `-disk-preparation`, `-image-preparation`, `-image-import`, `-runtime-preparation`, `-settings`, `-image-wait` become `computer-...` |
//! | `checkpoints/<id>.json` | `pendingCheckpointRestore.sourceWorkspace` becomes `sourceComputer` |
//! | `lifecycle-operations/*.json` | `machine_id` becomes `computer_id`; the embedded `event` is converted like an activity record |
//! | `update-resume.json` | `machines` becomes `computers` |
//! | `configuration-operation.json` | `previous` and `request` are converted like `machines.json` |
//! | `network.json` | `mappings[].workspace` becomes `computer` |
//! | `ssh-access.json` | `workspace` becomes `computer`, `machineId` becomes `computerId` |
//! | `microsandbox/repository-push-operations.json` | `operation.workspace` becomes `computer` |
//!
//! The private state folder (`~/.silo` or `~/.silo-dev`, from `channel.rs`):
//!
//! | File | Change |
//! | --- | --- |
//! | `desktop-remote/config.json` | `hostId` becomes `deviceId`, `hosts` becomes `devices` |
//!
//! Left as they are: MicroSandbox's own data and labels, Keychain items, the `/workspace`
//! storage files, names and identifiers that keep their older wording on purpose, and
//! values that hold a computer's name or identifier.
//!
//! # Which storage is converted
//!
//! Only storage the normal runtime reads is converted in place:
//!
//! * a selected generation (`runtime-generation.json`): the converted or clean folder;
//! * `runtime/` when no generation is selected and the storage migration does not need to
//!   run (`not-required`, or no record and no computer to convert).
//!
//! `runtime/` is left byte for byte as it was whenever the storage migration still has to
//! convert it, and once a generation is selected, because it is then the pre-upgrade
//! backup that `pre_upgrade_backup` keeps for 14 days and measures, reveals or deletes as a
//! whole. Because the storage migration copies that folder, the copy it stages is
//! converted with [`convert_storage_directory`] before anything reads it, and a clean
//! generation left by an interrupted attempt is converted the same way. The storage
//! migration reads the previous generation's inventory only through
//! [`read_previous_computers`], which accepts both file names and both vocabularies, so the
//! old format is read nowhere else.
//!
//! # Crash safety and failure
//!
//! Each file is written to a temporary file in its folder, synced, renamed over the
//! original and followed by a folder sync, so a file is either wholly old or wholly new. A
//! renamed file (`machines.json`, `sandbox-activity.json`) is written under its new name
//! before the old one is removed; if both exist the old one is converted again and wins,
//! as the only build that writes it is the older one. Files that already use the current
//! names are not rewritten. The record `vocabulary-migration.json` is written last, after
//! every file, so an interrupted run converts what is left at the next launch and a file
//! converted before the interruption is left alone.
//!
//! A file that is not valid JSON or has a structure no build wrote is carried over
//! unchanged (a renamed file keeps its bytes under the new name): the normal code treats it
//! exactly as the earlier code did. A file that exists but cannot be converted safely (a
//! link, a pipe, larger than any build writes), a file that cannot be read or written, a
//! storage selection or migration record that cannot be read, and a channel folder that is
//! a link or lies behind one stop the migration before anything is changed or the record is
//! saved: skipping them would leave the converted code without data it expects. Silo then
//! does not start, explains why, and converts the rest at the next launch.
use serde_json::Value;
use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

mod convert;
#[cfg(test)]
mod convert_tests;

use convert::computers_metadata;
use convert::{Converted, Unexpected};

const RECORD: &str = "vocabulary-migration.json";
const VERSION: u32 = 1;
const MAX_RECORD_BYTES: u64 = 64 * 1024;
/// No build writes a saved document this large.
const MAX_DOCUMENT_BYTES: u64 = 64 * 1024 * 1024;

const FAILED: &str = "Silo could not update its saved data to the computer names this version uses. Nothing was removed. Relaunch Silo to try again; if it keeps failing, check free space and access to Silo's storage, then report the problem.";

/// The failure for a file or folder no relaunch can fix: it names the path and the repair.
fn unusable(path: &Path) -> String {
    eprintln!("Saved data update: {} cannot be converted.", path.display());
    format!(
        "Silo cannot update its saved data because {} is a link, is not an ordinary file, or is larger than any version of Silo writes. Replace it with a regular file, or move it out of Silo's folder, then relaunch Silo.",
        path.display()
    )
}

/// The failure for a folder that is a link or lies behind one.
fn linked(path: &Path) -> String {
    eprintln!(
        "Saved data update: {} is a link or lies behind one.",
        path.display()
    );
    format!(
        "Silo cannot update its saved data because {} is a link or lies behind one. Replace the link with a regular folder, or move it out of Silo's folder, then relaunch Silo.",
        path.display()
    )
}

type Convert = fn(&mut Value) -> Converted;

/// Where the saved data of one channel lives.
pub(super) struct Locations {
    /// The folder of `settings.json`.
    pub(super) config: PathBuf,
    /// The folder of the other saved files and the storage generations.
    pub(super) app_data: PathBuf,
    /// The private state folder (`~/.silo`, `~/.silo-dev`).
    pub(super) state: PathBuf,
}

/// A file to convert. `source` and `destination` differ only when the file is renamed.
struct Target {
    /// The channel folder the file lives under; neither it nor a folder below it may be a link.
    root: PathBuf,
    source: PathBuf,
    destination: PathBuf,
    convert: Convert,
}

impl Target {
    fn in_place(root: &Path, path: PathBuf, convert: Convert) -> Self {
        Self {
            root: root.into(),
            source: path.clone(),
            destination: path,
            convert,
        }
    }

    fn renamed(root: &Path, folder: &Path, from: &str, to: &str, convert: Convert) -> Self {
        Self {
            root: root.into(),
            source: folder.join(from),
            destination: folder.join(to),
            convert,
        }
    }
}

/// Converts the saved data of this channel unless a previous launch already did, and
/// returns a message for the user when it could not. Call before anything reads the files.
pub(crate) fn run() -> Result<(), String> {
    let context = silo_engine::context::current().ok_or_else(|| FAILED.to_string())?;
    let paths = context.paths();
    let locations = Locations {
        config: paths.app_config.clone(),
        app_data: paths.app_data.clone(),
        state: crate::channel::current().state_dir(&paths.home),
    };
    run_in(&locations, &|_| Ok(())).inspect_err(|error| eprintln!("Saved data update: {error}"))?;
    Ok(())
}

/// [`run`] on `locations`. `before_write` is called with each file about to be replaced;
/// an error from it stops the run as a failed write would.
pub(super) fn run_in(
    locations: &Locations,
    before_write: &dyn Fn(&Path) -> io::Result<()>,
) -> Result<(), String> {
    if recorded(&locations.app_data) {
        return Ok(());
    }
    let mut targets = vec![Target::in_place(
        &locations.config,
        locations.config.join("settings.json"),
        convert::settings,
    )];
    for (name, convert) in [
        ("github.json", convert::github as Convert),
        ("secrets.json", convert::secrets),
        ("backup-operation.json", convert::backup_operation),
    ] {
        targets.push(Target::in_place(
            &locations.app_data,
            locations.app_data.join(name),
            convert,
        ));
    }
    targets.push(Target::in_place(
        &locations.state,
        locations.state.join("desktop-remote/config.json"),
        convert::connections,
    ));
    if let Some(storage) = live_storage(&locations.app_data)? {
        targets.extend(storage_targets(&storage)?);
    }
    ensure_unlinked(&locations.app_data, &locations.app_data)?;
    for target in &targets {
        ensure_unlinked(&target.root, target.source.parent().unwrap_or(&target.root))?;
        ensure_unlinked(
            &target.root,
            target.destination.parent().unwrap_or(&target.root),
        )?;
        ensure_convertible(&target.source)?;
    }
    for target in &targets {
        convert_file(target, before_write)?;
    }
    save_record(&locations.app_data, before_write)
}

/// Converts a parsed `desktop-remote/config.json` to the current vocabulary in memory.
/// The modes that read it before the application runs the full conversion
/// (`--remote-guest`, `--remote-bridge`) use this and never write the file, so they cannot
/// replace settings the running application saved since.
pub(crate) fn convert_connections_document(document: &mut Value) -> Result<(), String> {
    convert::connections(document)
        .map(|_| ())
        .map_err(|Unexpected| "Connections settings are damaged.".to_string())
}

/// Converts the storage folder `storage` in place. The storage migration calls this on
/// the copy it stages and on a clean generation left by an earlier attempt, neither of
/// which is read before it. Safe to repeat.
pub(super) fn convert_storage_directory(storage: &Path) -> Result<(), String> {
    let targets = storage_targets(storage)?;
    for target in &targets {
        ensure_unlinked(&target.root, target.source.parent().unwrap_or(&target.root))?;
        ensure_convertible(&target.source)?;
    }
    for target in &targets {
        convert_file(target, &|_| Ok(()))?;
    }
    Ok(())
}

/// The inventory of the previous storage generation in the current vocabulary, whether
/// `folder` holds `machines.json` or an already converted `computers.json`, or `None`
/// when it has neither. The file is not changed.
pub(super) fn read_previous_computers(folder: &Path) -> Result<Option<Value>, String> {
    for name in ["machines.json", "computers.json"] {
        let bytes = match read_source(&folder.join(name)).map_err(|_| FAILED.to_string())? {
            Source::Missing => continue,
            Source::Unusable => return Err(unusable(&folder.join(name))),
            Source::Bytes(bytes) => bytes,
        };
        let mut inventory: Value = serde_json::from_slice(&bytes)
            .map_err(|_| "Silo's saved computer configuration is invalid.".to_string())?;
        computers_metadata(&mut inventory)
            .map_err(|Unexpected| "Silo's saved computer configuration is invalid.".to_string())?;
        return Ok(Some(inventory));
    }
    Ok(None)
}

/// The storage the normal runtime reads right now, if the conversion may touch it. A
/// storage selection or migration record that cannot be read is an error, not "none".
fn live_storage(app_data: &Path) -> Result<Option<PathBuf>, String> {
    let unreadable = |error: String| {
        eprintln!("Saved data update: {error}");
        FAILED.to_string()
    };
    if let Some(selected) = super::generation(app_data).map_err(unreadable)? {
        return Ok(Some(app_data.join(selected)));
    }
    let runtime = app_data.join("runtime");
    Ok(
        match super::read(&app_data.join(super::FILE)).map_err(unreadable)? {
            Some(state) if state.status == "not-required" => Some(runtime),
            Some(_) => None,
            None => (!holds_computers(&runtime)).then_some(runtime),
        },
    )
}

/// Fails when `path` exists but is not a regular file of a size any build writes, before
/// any file is changed.
fn ensure_convertible(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && metadata.len() <= MAX_DOCUMENT_BYTES => Ok(()),
        Ok(_) => Err(unusable(path)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotADirectory => Ok(()),
        Err(_) => Err(FAILED.into()),
    }
}

/// Fails when `root` is a link or a folder between it and `folder` is one, so no write can
/// leave the channel's own tree. Folders that do not exist yet are fine.
fn ensure_unlinked(root: &Path, folder: &Path) -> Result<(), String> {
    let mut current = root.to_path_buf();
    let relative = folder.strip_prefix(root).map_err(|_| linked(folder))?;
    let components = std::iter::once(None).chain(relative.components().map(Some));
    for component in components {
        if let Some(component) = component {
            current.push(component);
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => return Err(linked(&current)),
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(_) => return Err(FAILED.into()),
        }
    }
    Ok(())
}

/// Whether the inventory in `runtime` lists a computer, or cannot be shown to list none.
fn holds_computers(runtime: &Path) -> bool {
    let Ok(Some(inventory)) = read_previous_computers(runtime) else {
        return fs::symlink_metadata(runtime.join("machines.json")).is_ok()
            || fs::symlink_metadata(runtime.join("computers.json")).is_ok();
    };
    inventory
        .get("computers")
        .and_then(Value::as_array)
        .is_none_or(|computers| !computers.is_empty())
}

fn storage_targets(storage: &Path) -> Result<Vec<Target>, String> {
    let root = storage.parent().unwrap_or(storage);
    let mut targets = vec![
        Target::renamed(
            root,
            storage,
            "machines.json",
            "computers.json",
            computers_metadata,
        ),
        Target::renamed(
            root,
            storage,
            "sandbox-activity.json",
            "computer-activity.json",
            convert::computer_records,
        ),
    ];
    for (name, convert) in [
        ("setup-activity.json", convert::setup_activity as Convert),
        ("update-resume.json", convert::update_resume),
        (
            "configuration-operation.json",
            convert::configuration_operation,
        ),
        ("network.json", convert::network),
        ("ssh-access.json", convert::computer_records),
        (
            "microsandbox/repository-push-operations.json",
            convert::push_operations,
        ),
    ] {
        targets.push(Target::in_place(root, storage.join(name), convert));
    }
    for (folder, convert) in [
        ("checkpoints", convert::checkpoint_record as Convert),
        ("lifecycle-operations", convert::lifecycle_operation),
    ] {
        targets.extend(
            json_files(&storage.join(folder))?
                .into_iter()
                .map(|path| Target::in_place(root, path, convert)),
        );
    }
    Ok(targets)
}

/// The `.json` files directly inside `folder`, by name. A missing folder has none.
fn json_files(folder: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = match fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) if error.kind() == io::ErrorKind::NotADirectory => return Ok(Vec::new()),
        Err(_) => return Err(FAILED.into()),
    };
    let mut files = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_| FAILED.to_string())?;
        let path = entry.path();
        let is_folder = entry.file_type().is_ok_and(|kind| kind.is_dir());
        if !is_folder
            && path
                .extension()
                .is_some_and(|extension| extension == "json")
        {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

/// What `path` holds.
enum Source {
    Missing,
    /// Present but not usable as a saved document: a link, a pipe, larger than any build writes.
    Unusable,
    Bytes(Vec<u8>),
}

fn read_source(path: &Path) -> io::Result<Source> {
    let (file, metadata) = match crate::backup::open_regular_file(path) {
        Ok(opened) => opened,
        Err(crate::backup::OpenRegularError::Io(error))
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
            ) =>
        {
            return Ok(Source::Missing)
        }
        Err(crate::backup::OpenRegularError::Io(error)) => return Err(error),
        Err(crate::backup::OpenRegularError::NotRegular) => return Ok(Source::Unusable),
    };
    if metadata.len() > MAX_DOCUMENT_BYTES {
        return Ok(Source::Unusable);
    }
    let mut bytes = Vec::new();
    file.take(MAX_DOCUMENT_BYTES + 1).read_to_end(&mut bytes)?;
    Ok(if bytes.len() as u64 <= MAX_DOCUMENT_BYTES {
        Source::Bytes(bytes)
    } else {
        Source::Unusable
    })
}

fn convert_file(
    target: &Target,
    before_write: &dyn Fn(&Path) -> io::Result<()>,
) -> Result<(), String> {
    let fail = || {
        eprintln!(
            "Saved data update: {} could not be converted.",
            target
                .source
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
        );
        FAILED.to_string()
    };
    let failed = |_: io::Error| fail();
    let original = match read_source(&target.source).map_err(failed)? {
        Source::Missing => return Ok(()),
        Source::Unusable => return Err(unusable(&target.source)),
        Source::Bytes(bytes) => bytes,
    };
    let renamed = target.source != target.destination;
    let converted = serde_json::from_slice::<Value>(&original)
        .ok()
        .and_then(|mut document| {
            (target.convert)(&mut document)
                .ok()
                .map(|changed| (document, changed))
        });
    let bytes = match converted {
        Some((document, true)) => {
            let mut bytes = serde_json::to_vec_pretty(&document).map_err(|_| FAILED.to_string())?;
            bytes.push(b'\n');
            bytes
        }
        Some((_, false)) if !renamed => return Ok(()),
        // Unusable content is not the migration's to repair; a renamed file keeps its bytes.
        _ if !renamed => return Ok(()),
        _ => original,
    };
    before_write(&target.destination).map_err(failed)?;
    let permissions = fs::metadata(&target.source).map_err(failed)?.permissions();
    write_atomically(&target.destination, &bytes, Some(permissions)).map_err(failed)?;
    if renamed {
        fs::remove_file(&target.source).map_err(failed)?;
        sync_folder(target.source.parent()).map_err(failed)?;
    }
    Ok(())
}

fn write_atomically(
    path: &Path,
    bytes: &[u8],
    permissions: Option<fs::Permissions>,
) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no parent folder"))?;
    fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    if let Some(permissions) = permissions {
        temporary.as_file().set_permissions(permissions)?;
    }
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    sync_folder(Some(parent))
}

fn sync_folder(folder: Option<&Path>) -> io::Result<()> {
    match folder {
        Some(folder) => fs::File::open(folder)?.sync_all(),
        None => Ok(()),
    }
}

/// The record `vocabulary-migration.json`: `{"version": 1}`, written after every file.
/// Next to the other records rather than inside `runtime-migration.json`, whose reader
/// rejects fields it does not know.
fn recorded(app_data: &Path) -> bool {
    let Ok(Source::Bytes(bytes)) = read_source(&app_data.join(RECORD)) else {
        return false;
    };
    bytes.len() as u64 <= MAX_RECORD_BYTES
        && serde_json::from_slice::<Value>(&bytes)
            .ok()
            .and_then(|record| record.get("version").and_then(Value::as_u64))
            .is_some_and(|version| version >= u64::from(VERSION))
}

fn save_record(
    app_data: &Path,
    before_write: &dyn Fn(&Path) -> io::Result<()>,
) -> Result<(), String> {
    let path = app_data.join(RECORD);
    let failed = |_| FAILED.to_string();
    before_write(&path).map_err(failed)?;
    write_atomically(
        &path,
        format!("{{\n  \"version\": {VERSION}\n}}\n").as_bytes(),
        None,
    )
    .map_err(failed)
}
