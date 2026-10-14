//! The previous runtime generation, kept as a pre-upgrade backup after a migration that
//! converted every computer, and deleted 14 days later or when the user asks.
//!
//! `<app_data>/runtime` is that backup only while the migration is complete and the
//! converted generation is selected (`runtime_migration::previous_generation_is_backup`).
//! After "Continue" the same folder holds the only copy of the unconverted computers, so
//! nothing here offers, measures, reveals or deletes it then.
//!
//! The 14-day window is recorded in `pre-upgrade-backup.json`, not in
//! `runtime-migration.json`, whose reader rejects unknown fields: builds without this
//! feature never read the new file, and this build reads an install that migrated earlier
//! (no record) as a window starting at its first launch. Without a readable record the
//! backup is still shown and can be deleted by hand, but is never deleted automatically.
//!
//! The folder may also hold `before-checkpoints-backup-history.json`, which the migration moved
//! there so the old export-folder choice is not replayed against the new runtime, and
//! `before-checkpoints-backup-operation.json` when an earlier build moved the export journal
//! there too. A migration that converts every computer moves no journal into it now: one that
//! recorded its result, or whose cleanup only the runtime can do and so waits for the upgrade,
//! stays in the app data folder for the converted storage's recovery to report or finish (the
//! backup keeps its copy of what the operation left, untouched), and one that cannot be read is
//! set aside there as `backup-operation.unreadable-<UTC date>.json`. Only "Continue" moves an
//! unfinished journal into the previous folder, which is not a backup then. Nothing reads the
//! moved files back, so deleting the folder loses nothing the app uses.
//!
//! The previous generation was also reached through an alias symlink in the account's state
//! directory (`runtime::runtime_home_alias`), which dangles once the folder is gone. It is
//! removed with the folder, and later if the folder was deleted by hand, but only when it is a
//! symlink whose link text is exactly the backup's `microsandbox` directory.
use crate::{runtime::image_cache, runtime_migration};
use serde::{Deserialize, Serialize};
use silo_engine::{Audience, Event};
use std::{
    collections::HashSet,
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard, PoisonError},
    time::Duration,
};
use time::{format_description::well_known::Rfc3339, OffsetDateTime, UtcOffset};

const FILE: &str = "pre-upgrade-backup.json";
const VERSION: u32 = 1;
const MAX_RECORD_BYTES: u64 = 64 * 1024;
const RETENTION: time::Duration = time::Duration::days(14);
/// Hourly rather than daily: the sleep does not count time the device spent asleep, and a
/// check that finds nothing due only reads one small file.
const CHECK_EVERY: Duration = Duration::from_secs(60 * 60);
const CHANGED: &str = "silo://pre-upgrade-backup-changed";

pub fn publish_changed() {
    if let Some(context) = silo_engine::context::current() {
        context.publish(Event::new(CHANGED, serde_json::Value::Null), Audience::All);
    }
}

const NOT_A_BACKUP: &str = "The previous computer storage is only treated as a deletable backup after a migration that converted every computer. Nothing was deleted.";
const IN_USE: &str = "Silo reads computers from this folder, so it was not deleted.";
const REDIRECTED: &str =
    "The pre-upgrade backup is a link, not a folder, so Silo left it alone. Nothing was deleted.";
const UNINSPECTABLE: &str = "Silo could not inspect the pre-upgrade backup. Nothing was deleted.";

/// Serializes changes to the record. Held only briefly.
static RECORD: Mutex<()> = Mutex::new(());
/// One deletion at a time. A second one waits, then finds nothing left to delete.
static DELETION: Mutex<()> = Mutex::new(());

fn lock(mutex: &'static Mutex<()>) -> MutexGuard<'static, ()> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    version: u32,
    /// When the migration was first seen complete (RFC 3339, UTC).
    started_at: String,
    /// The migration screen told the user about the backup.
    #[serde(default)]
    notice_acknowledged: bool,
}

enum Saved {
    Missing,
    Valid {
        started: OffsetDateTime,
        acknowledged: bool,
    },
    /// Damaged or written by a later version. Left as it is.
    Unreadable,
}

fn load(app_data: &Path) -> Saved {
    let (file, metadata) = match crate::backup::open_regular_file(&app_data.join(FILE)) {
        Ok(opened) => opened,
        Err(crate::backup::OpenRegularError::Io(error))
            if error.kind() == io::ErrorKind::NotFound =>
        {
            return Saved::Missing;
        }
        Err(_) => return Saved::Unreadable,
    };
    if metadata.len() > MAX_RECORD_BYTES {
        return Saved::Unreadable;
    }
    let mut bytes = Vec::new();
    if file
        .take(MAX_RECORD_BYTES + 1)
        .read_to_end(&mut bytes)
        .is_err()
        || bytes.len() as u64 > MAX_RECORD_BYTES
    {
        return Saved::Unreadable;
    }
    let Ok(record) = serde_json::from_slice::<Record>(&bytes) else {
        return Saved::Unreadable;
    };
    match OffsetDateTime::parse(&record.started_at, &Rfc3339) {
        Ok(started)
            if record.version == VERSION
                && started
                    .checked_add(RETENTION)
                    .and_then(|date| date.checked_to_offset(UtcOffset::UTC))
                    .is_some() =>
        {
            Saved::Valid {
                started,
                acknowledged: record.notice_acknowledged,
            }
        }
        _ => Saved::Unreadable,
    }
}

fn save(app_data: &Path, started: OffsetDateTime, acknowledged: bool) -> Result<(), String> {
    let failed = || "Silo could not save when the pre-upgrade backup will be deleted.".to_string();
    let started = started.to_offset(UtcOffset::UTC);
    let record = Record {
        version: VERSION,
        started_at: started
            .replace_nanosecond(0)
            .unwrap_or(started)
            .format(&Rfc3339)
            .map_err(|_| failed())?,
        notice_acknowledged: acknowledged,
    };
    let mut file = tempfile::NamedTempFile::new_in(app_data).map_err(|_| failed())?;
    serde_json::to_writer_pretty(&mut file, &record).map_err(|_| failed())?;
    io::Write::write_all(&mut file, b"\n")
        .and_then(|_| file.as_file().sync_all())
        .map_err(|_| failed())?;
    file.persist(app_data.join(FILE)).map_err(|_| failed())?;
    fs::File::open(app_data)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| failed())
}

/// The record belongs to a backup that no longer exists.
fn forget(app_data: &Path) {
    match fs::remove_file(app_data.join(FILE)) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => eprintln!("Silo could not remove its pre-upgrade backup record: {error}"),
    }
}

/// When Silo deletes a backup whose window started at `started`.
fn delete_at(started: OffsetDateTime) -> OffsetDateTime {
    (started + RETENTION).to_offset(UtcOffset::UTC)
}

fn is_due(started: OffsetDateTime, now: OffsetDateTime) -> bool {
    now >= delete_at(started)
}

enum Backup {
    /// The previous folder is not a backup: the migration is unfinished, nothing migrated, or
    /// the clean generation is in use.
    NotABackup,
    /// It was a backup and has been deleted.
    Gone,
    Refused(&'static str),
    Present(PathBuf),
}

/// What `<app_data>/runtime` is, given the folder Silo currently reads computers from.
fn locate(previous: &Path, selected: &Path) -> Backup {
    if selected == previous {
        return Backup::Refused(IN_USE);
    }
    match fs::symlink_metadata(previous) {
        Ok(metadata) if metadata.file_type().is_symlink() => Backup::Refused(REDIRECTED),
        Ok(metadata) if metadata.is_dir() => Backup::Present(previous.to_path_buf()),
        Ok(_) => Backup::Refused(UNINSPECTABLE),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Backup::Gone,
        Err(_) => Backup::Refused(UNINSPECTABLE),
    }
}

fn inspect(app_data: &Path) -> Backup {
    match runtime_migration::backup_locations(app_data) {
        Some(at) => locate(&at.previous, &at.selected),
        None => Backup::NotABackup,
    }
}

/// The start of the window for a backup that is present, recording it now when this is the
/// first time it is seen (a fresh migration, or one made before this feature).
fn ensure_started(
    _held: &MutexGuard<'static, ()>,
    app_data: &Path,
    now: OffsetDateTime,
) -> Option<Saved> {
    match inspect(app_data) {
        Backup::Present(_) => {}
        Backup::Gone => {
            forget(app_data);
            return None;
        }
        _ => return None,
    }
    Some(match load(app_data) {
        Saved::Missing => match save(app_data, now, false) {
            Ok(()) => Saved::Valid {
                started: now,
                acknowledged: false,
            },
            Err(message) => {
                eprintln!("{message} It will not be deleted automatically.");
                Saved::Unreadable
            }
        },
        saved => saved,
    })
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// RFC 3339 instant after which Silo deletes the backup by itself. `None` when the saved
    /// date cannot be read: the backup is then never deleted automatically.
    delete_at: Option<String>,
    /// The migration screen has not yet told the user the backup exists.
    notice_pending: bool,
}

fn status(app_data: &Path, now: OffsetDateTime) -> Result<Option<Status>, String> {
    let held = lock(&RECORD);
    Ok(match ensure_started(&held, app_data, now) {
        None => None,
        Some(Saved::Valid {
            started,
            acknowledged,
        }) => Some(Status {
            delete_at: Some(
                delete_at(started)
                    .format(&Rfc3339)
                    .map_err(|_| "The deletion date could not be formatted.")?,
            ),
            notice_pending: !acknowledged,
        }),
        Some(_) => Some(Status {
            delete_at: None,
            notice_pending: false,
        }),
    })
}

fn acknowledge(app_data: &Path, now: OffsetDateTime) -> Result<(), String> {
    let held = lock(&RECORD);
    match ensure_started(&held, app_data, now) {
        Some(Saved::Valid { started, .. }) => save(app_data, started, true),
        _ => Ok(()),
    }
}

/// Allocated bytes, not apparent size: a sparse disk image counts what it occupies.
/// Symlinks are not followed, and a file with several names counts once.
fn allocated_bytes(root: &Path) -> io::Result<u64> {
    let mut total = 0;
    let mut linked: HashSet<(u64, u64)> = HashSet::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            // Removed while measuring.
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if metadata.nlink() > 1
                && !metadata.is_dir()
                && !linked.insert((metadata.dev(), metadata.ino()))
            {
                continue;
            }
            // `st_blocks` counts 512-byte units on every supported platform.
            total += metadata.blocks() * 512;
        }
        #[cfg(not(unix))]
        {
            let _ = &mut linked;
            total += metadata.len();
        }
        if metadata.is_dir() {
            match fs::read_dir(&path) {
                Ok(entries) => {
                    for entry in entries {
                        pending.push(entry?.path());
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
    }
    Ok(total)
}

fn measure(app_data: &Path) -> Result<Option<u64>, String> {
    match inspect(app_data) {
        Backup::Present(previous) => allocated_bytes(&previous)
            .map(Some)
            .map_err(|_| "Silo could not measure the pre-upgrade backup.".into()),
        _ => Ok(None),
    }
}

/// Converted runtimes made by earlier Silo versions still named the previous runtime's image
/// files. Startup repairs that too, but this must not depend on which runs first: a computer whose
/// image still reads from the backup stops booting once the backup is gone.
fn ensure_converted_runtime_is_independent(app_data: &Path, previous: &Path) -> Result<(), String> {
    let cache = runtime_migration::backup_locations(app_data)
        .ok_or_else(|| NOT_A_BACKUP.to_string())?
        .selected
        .join("microsandbox/cache");
    if let Err(message) = image_cache::repair(&cache) {
        eprintln!("Image cache repair: {message}");
    }
    match image_cache::reads_from(&cache, previous) {
        Ok(false) => Ok(()),
        Ok(true) => Err("A converted computer's image still reads files from the pre-upgrade backup, so Silo kept it. Relaunch Silo and try again.".into()),
        Err(_) => Err("Silo could not check that your computers no longer need the pre-upgrade backup, so it kept it. Relaunch Silo and try again.".into()),
    }
}

/// Removes the previous generation's runtime alias, which dangles once its backup is gone.
/// Best effort, never a reason to fail a deletion that has already succeeded. Only a
/// symlink whose link text is exactly the backup's `microsandbox` directory goes: a folder,
/// a file, a link to anywhere else, the converted generation's alias and every other entry in
/// the state directory stay. The link is never followed. `user_home` is `None` when the
/// account's home is unknown, which leaves the alias alone.
fn remove_previous_alias(app_data: &Path, user_home: Option<&Path>) {
    let Some(alias) = user_home
        .zip(runtime_migration::backup_locations(app_data))
        .map(|(home, at)| at.previous_alias(home))
    else {
        return;
    };
    let failed = |error: io::Error| {
        eprintln!("Silo could not remove the previous runtime's alias link: {error}");
    };
    match fs::symlink_metadata(&alias.link) {
        Ok(metadata) if metadata.file_type().is_symlink() => {}
        Ok(_) => return,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return,
        Err(error) => return failed(error),
    }
    match fs::read_link(&alias.link) {
        Ok(target) if target == alias.target => {}
        Ok(_) => return,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return,
        Err(error) => return failed(error),
    }
    match fs::remove_file(&alias.link) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => failed(error),
    }
}

/// A backup that was deleted by hand leaves its alias behind. Cheap enough for every check.
fn tidy_alias_of_deleted_backup(app_data: &Path, user_home: Option<&Path>) {
    let _deletion = lock(&DELETION);
    if matches!(inspect(app_data), Backup::Gone) {
        remove_previous_alias(app_data, user_home);
    }
}

/// Deletes exactly `<app_data>/runtime`, then its runtime alias. `Ok(false)` when the folder
/// was already gone.
fn delete_backup(app_data: &Path, user_home: Option<&Path>) -> Result<bool, String> {
    let _deletion = lock(&DELETION);
    let previous = match inspect(app_data) {
        Backup::Present(previous) => previous,
        Backup::Gone => {
            remove_previous_alias(app_data, user_home);
            forget(app_data);
            return Ok(false);
        }
        Backup::NotABackup => return Err(NOT_A_BACKUP.into()),
        Backup::Refused(reason) => return Err(reason.into()),
    };
    ensure_converted_runtime_is_independent(app_data, &previous)?;
    // `remove_dir_all` unlinks symlinks instead of following them. A failure part-way keeps
    // the rest, the record and the row, so the user can retry.
    fs::remove_dir_all(&previous).map_err(|error| {
        format!("Silo could not finish deleting the pre-upgrade backup: {error}. Your computers were not affected. Try again.")
    })?;
    let _ = fs::File::open(app_data).and_then(|directory| directory.sync_all());
    remove_previous_alias(app_data, user_home);
    let _held = lock(&RECORD);
    forget(app_data);
    Ok(true)
}

/// Deletes the backup when its 14 days are over. `Ok(true)` when this call deleted it.
fn delete_if_due(
    app_data: &Path,
    user_home: Option<&Path>,
    now: OffsetDateTime,
) -> Result<bool, String> {
    let saved = {
        let held = lock(&RECORD);
        ensure_started(&held, app_data, now)
    };
    match saved {
        Some(Saved::Valid { started, .. }) if is_due(started, now) => {
            delete_backup(app_data, user_home)
        }
        Some(_) => Ok(false),
        None => {
            tidy_alias_of_deleted_backup(app_data, user_home);
            Ok(false)
        }
    }
}

/// Checks at launch and then every hour while Silo runs. A backup that came due while Silo
/// was closed is therefore deleted at the next launch.
pub fn install() {
    let Some(context) = silo_engine::context::current() else {
        return;
    };
    let app_data = context.paths().app_data.clone();
    let user_home = Some(context.paths().home.clone());
    let spawned = std::thread::Builder::new()
        .name("pre-upgrade-backup".into())
        .spawn(move || loop {
            match delete_if_due(&app_data, user_home.as_deref(), OffsetDateTime::now_utc()) {
                Ok(true) => {
                    eprintln!("Silo deleted the pre-upgrade backup after 14 days.");
                    context.publish(Event::new(CHANGED, serde_json::Value::Null), Audience::All);
                }
                Ok(false) => {}
                Err(message) => eprintln!("The pre-upgrade backup was kept: {message}"),
            }
            std::thread::sleep(CHECK_EVERY);
        });
    if let Err(error) = spawned {
        eprintln!("Silo could not schedule deleting the pre-upgrade backup: {error}");
    }
}

/// Whether a pre-upgrade backup exists and when it is deleted. Cheap: it measures nothing.
pub fn read_status(app_data: &Path) -> Result<Option<Status>, String> {
    status(app_data, OffsetDateTime::now_utc())
}

/// Allocated bytes of the backup, or `None` once it is gone. Walks the folder.
pub fn measure_backup(app_data: &Path) -> Result<Option<u64>, String> {
    measure(app_data)
}

pub fn delete_backup_for_user(app_data: &Path, user_home: &Path) -> Result<bool, String> {
    delete_backup(app_data, Some(user_home))
}

pub fn reveal_path(app_data: &Path) -> Result<PathBuf, String> {
    match inspect(app_data) {
        Backup::Present(previous) => Ok(previous),
        Backup::Gone => Err("The pre-upgrade backup has already been deleted.".into()),
        Backup::NotABackup => Err(NOT_A_BACKUP.into()),
        Backup::Refused(reason) => Err(reason.into()),
    }
}

/// The migration screen told the user about the backup.
pub fn acknowledge_notice(app_data: &Path) -> Result<(), String> {
    acknowledge(app_data, OffsetDateTime::now_utc())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONVERTED: &str = "runtime-checkpoints-converted";
    const CLEAN: &str = "runtime-checkpoints-clean";
    const LAYER: &str =
        "sha256_6078cde548a521a729def2ee7875e9f65513c18f0d4bac4db817417617d7006a.erofs";

    fn at(text: &str) -> OffsetDateTime {
        OffsetDateTime::parse(text, &Rfc3339).unwrap()
    }

    fn json(path: PathBuf, text: &str) {
        fs::write(path, text).unwrap();
    }

    /// An app-data folder after a migration, with a previous generation holding a root disk,
    /// a sparse workspace disk and the two files the migration quarantined.
    fn migrated(generation: Option<&str>, migration: Option<&str>) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let app_data = dir.path();
        let previous = app_data.join("runtime");
        fs::create_dir_all(previous.join("microsandbox/sandboxes/dev")).unwrap();
        fs::create_dir_all(previous.join("volumes/dev")).unwrap();
        fs::write(
            previous.join("microsandbox/sandboxes/dev/upper.ext4"),
            vec![1; 64 * 1024],
        )
        .unwrap();
        fs::write(previous.join("volumes/dev/workspace.raw"), vec![2; 4096]).unwrap();
        fs::write(
            previous.join("before-checkpoints-backup-history.json"),
            b"{}",
        )
        .unwrap();
        fs::write(
            previous.join("before-checkpoints-backup-operation.json"),
            b"{}",
        )
        .unwrap();
        if let Some(generation) = generation {
            json(
                app_data.join("runtime-generation.json"),
                &format!(r#"{{"version":1,"directory":"{generation}"}}"#),
            );
            fs::create_dir_all(app_data.join(generation).join("microsandbox/cache/vmdk")).unwrap();
        }
        if let Some(status) = migration {
            json(
                app_data.join("runtime-migration.json"),
                &format!(
                    r#"{{"version":1,"status":"{status}","stage":"Migration complete","logs":[],"migratedCount":1,"failedCount":0,"totalCount":1,"canContinue":false}}"#
                ),
            );
        }
        dir
    }

    fn complete() -> tempfile::TempDir {
        migrated(Some(CONVERTED), Some("complete"))
    }

    fn started_at(app_data: &Path) -> String {
        let record: Record =
            serde_json::from_slice(&fs::read(app_data.join(FILE)).unwrap()).unwrap();
        record.started_at
    }

    #[test]
    fn the_window_is_fourteen_days_across_month_year_and_leap_boundaries() {
        for (start, end) in [
            ("2026-10-01T10:00:00Z", "2026-10-15T10:00:00Z"),
            ("2026-10-25T23:59:59Z", "2026-11-08T23:59:59Z"),
            ("2026-12-25T00:00:00Z", "2027-01-08T00:00:00Z"),
            ("2028-02-20T12:00:00Z", "2028-03-05T12:00:00Z"),
            ("2027-02-20T12:00:00Z", "2027-03-06T12:00:00Z"),
            ("9999-12-17T23:59:59Z", "9999-12-31T23:59:59Z"),
            // A local start is stored and compared as the same instant in UTC.
            ("2026-10-01T23:30:00-07:00", "2026-10-16T06:30:00Z"),
        ] {
            assert_eq!(delete_at(at(start)), at(end), "{start}");
            assert_eq!(delete_at(at(start)).offset(), UtcOffset::UTC);
        }
    }

    #[test]
    fn a_backup_is_due_exactly_when_its_fourteen_days_end() {
        let start = at("2026-10-01T10:00:00Z");
        assert!(!is_due(start, start));
        assert!(!is_due(start, at("2026-10-15T09:59:59Z")));
        assert!(is_due(start, at("2026-10-15T10:00:00Z")));
        assert!(is_due(start, at("2027-01-01T00:00:00Z")));
        // A clock set back never makes it due.
        assert!(!is_due(start, at("2026-09-01T00:00:00Z")));
    }

    #[test]
    fn a_fresh_migration_starts_its_window_once_and_reports_the_date() {
        let dir = complete();
        let first = status(dir.path(), at("2026-10-01T10:00:00Z"))
            .unwrap()
            .unwrap();
        assert_eq!(
            first,
            Status {
                delete_at: Some("2026-10-15T10:00:00Z".into()),
                notice_pending: true
            }
        );
        assert_eq!(started_at(dir.path()), "2026-10-01T10:00:00Z");
        // Later reads, including from another launch, keep the first start.
        let later = status(dir.path(), at("2026-10-09T08:00:00Z"))
            .unwrap()
            .unwrap();
        assert_eq!(later.delete_at.as_deref(), Some("2026-10-15T10:00:00Z"));
        assert_eq!(started_at(dir.path()), "2026-10-01T10:00:00Z");
        acknowledge(dir.path(), at("2026-10-02T00:00:00Z")).unwrap();
        let acknowledged = status(dir.path(), at("2026-10-03T00:00:00Z"))
            .unwrap()
            .unwrap();
        assert_eq!(
            acknowledged,
            Status {
                delete_at: Some("2026-10-15T10:00:00Z".into()),
                notice_pending: false
            }
        );
        assert_eq!(started_at(dir.path()), "2026-10-01T10:00:00Z");
    }

    #[test]
    fn an_install_migrated_before_this_feature_starts_its_window_at_the_first_launch() {
        // Completed migration, previous folder still there, no record anywhere.
        let dir = complete();
        assert!(!dir.path().join(FILE).exists());
        // The launch check records the start without deleting anything.
        assert!(!delete_if_due(dir.path(), None, at("2026-10-01T09:00:00Z")).unwrap());
        assert_eq!(started_at(dir.path()), "2026-10-01T09:00:00Z");
        assert!(dir.path().join("runtime").is_dir());
        // Not due on day 13, due on day 14.
        assert!(!delete_if_due(dir.path(), None, at("2026-10-14T09:00:00Z")).unwrap());
        assert!(dir.path().join("runtime").is_dir());
        assert!(delete_if_due(dir.path(), None, at("2026-10-15T09:00:00Z")).unwrap());
        assert!(!dir.path().join("runtime").exists());
    }

    #[test]
    fn a_backup_that_came_due_while_silo_was_closed_is_deleted_at_the_next_launch() {
        let dir = complete();
        save(dir.path(), at("2026-09-01T10:00:00Z"), true).unwrap();
        let outside = dir.path().join("runtime-checkpoints-converted/keep.txt");
        fs::write(&outside, b"converted").unwrap();
        assert!(delete_if_due(dir.path(), None, at("2026-10-01T10:00:00Z")).unwrap());
        assert!(!dir.path().join("runtime").exists());
        assert!(
            !dir.path().join(FILE).exists(),
            "the record goes with the backup"
        );
        assert_eq!(fs::read(outside).unwrap(), b"converted");
        // The next hourly check, and the status, find nothing to do.
        assert!(!delete_if_due(dir.path(), None, at("2026-10-01T11:00:00Z")).unwrap());
        assert_eq!(
            status(dir.path(), at("2026-10-01T11:00:00Z")).unwrap(),
            None
        );
    }

    #[test]
    fn deletion_is_idempotent_and_leaves_the_converted_runtime_alone() {
        let dir = complete();
        let converted = dir.path().join(CONVERTED).join("computers.json");
        fs::write(&converted, b"converted").unwrap();
        assert!(delete_backup(dir.path(), None).unwrap());
        assert!(!dir.path().join("runtime").exists());
        assert!(!delete_backup(dir.path(), None).unwrap());
        assert!(!delete_backup(dir.path(), None).unwrap());
        assert_eq!(fs::read(converted).unwrap(), b"converted");
        assert!(dir.path().join("runtime-generation.json").exists());
        assert!(dir.path().join("runtime-migration.json").exists());
    }

    #[test]
    fn concurrent_deletions_agree_that_one_of_them_deleted_it() {
        let dir = complete();
        let app_data = dir.path().to_path_buf();
        let results: Vec<_> = (0..8)
            .map(|_| {
                let app_data = app_data.clone();
                std::thread::spawn(move || delete_backup(&app_data, None))
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();
        assert!(results.iter().all(Result::is_ok), "{results:?}");
        assert_eq!(
            results.iter().filter(|result| **result == Ok(true)).count(),
            1
        );
        assert!(!app_data.join("runtime").exists());
    }

    #[test]
    fn the_clean_generation_never_offers_or_deletes_the_only_copy() {
        let dir = migrated(Some(CLEAN), Some("complete"));
        save(dir.path(), at("2020-01-01T00:00:00Z"), false).unwrap();
        let before = fs::read(dir.path().join("runtime/volumes/dev/workspace.raw")).unwrap();
        assert_eq!(
            status(dir.path(), at("2026-10-01T10:00:00Z")).unwrap(),
            None
        );
        assert_eq!(measure(dir.path()).unwrap(), None);
        assert!(delete_backup(dir.path(), None)
            .unwrap_err()
            .contains("converted"));
        // Even an ancient record never makes it due.
        assert!(!delete_if_due(dir.path(), None, at("2026-10-01T10:00:00Z")).unwrap());
        assert!(matches!(inspect(dir.path()), Backup::NotABackup));
        assert_eq!(
            fs::read(dir.path().join("runtime/volumes/dev/workspace.raw")).unwrap(),
            before
        );
    }

    #[test]
    fn nothing_is_offered_unless_the_migration_completed_into_the_converted_generation() {
        for (name, dir) in [
            ("never migrated", migrated(None, None)),
            ("no migration record", migrated(Some(CONVERTED), None)),
            (
                "restarting after conversion",
                migrated(Some(CONVERTED), Some("running")),
            ),
            ("failed", migrated(Some(CONVERTED), Some("failed"))),
            ("not required", migrated(None, Some("not-required"))),
            (
                "unknown generation",
                migrated(Some("runtime-other"), Some("complete")),
            ),
        ] {
            let now = at("2026-10-01T10:00:00Z");
            assert_eq!(status(dir.path(), now).unwrap(), None, "{name}");
            assert!(delete_backup(dir.path(), None).is_err(), "{name}");
            assert!(!delete_if_due(dir.path(), None, now).unwrap(), "{name}");
            assert!(!dir.path().join(FILE).exists(), "{name}");
            assert!(dir.path().join("runtime/volumes/dev").is_dir(), "{name}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_previous_folder_that_is_a_symlink_is_refused_and_its_target_is_untouched() {
        let dir = complete();
        let elsewhere = tempfile::tempdir().unwrap();
        fs::write(elsewhere.path().join("precious"), b"mine").unwrap();
        fs::remove_dir_all(dir.path().join("runtime")).unwrap();
        std::os::unix::fs::symlink(elsewhere.path(), dir.path().join("runtime")).unwrap();
        assert_eq!(
            status(dir.path(), at("2026-10-01T10:00:00Z")).unwrap(),
            None
        );
        let error = delete_backup(dir.path(), None).unwrap_err();
        assert!(error.contains("link"), "{error}");
        assert!(!delete_if_due(dir.path(), None, at("2030-01-01T00:00:00Z")).unwrap());
        assert_eq!(measure(dir.path()).unwrap(), None);
        assert!(fs::symlink_metadata(dir.path().join("runtime"))
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(
            fs::read(elsewhere.path().join("precious")).unwrap(),
            b"mine"
        );
    }

    #[test]
    fn the_folder_silo_reads_from_is_never_the_backup() {
        let dir = complete();
        let previous = dir.path().join("runtime");
        assert!(matches!(
            locate(&previous, &previous),
            Backup::Refused(reason) if reason == IN_USE
        ));
        assert!(matches!(
            locate(&previous, &dir.path().join(CONVERTED)),
            Backup::Present(_)
        ));
        fs::write(dir.path().join("file"), b"x").unwrap();
        fs::remove_dir_all(&previous).unwrap();
        fs::write(&previous, b"not a folder").unwrap();
        assert!(matches!(
            locate(&previous, &dir.path().join(CONVERTED)),
            Backup::Refused(_)
        ));
        assert!(delete_backup(dir.path(), None).is_err());
        assert_eq!(fs::read(&previous).unwrap(), b"not a folder");
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_inside_the_backup_are_removed_not_followed() {
        let dir = complete();
        let elsewhere = tempfile::tempdir().unwrap();
        fs::create_dir(elsewhere.path().join("folder")).unwrap();
        fs::write(elsewhere.path().join("folder/precious"), b"mine").unwrap();
        fs::write(elsewhere.path().join("file"), b"mine").unwrap();
        let previous = dir.path().join("runtime");
        std::os::unix::fs::symlink(
            elsewhere.path().join("folder"),
            previous.join("folder-link"),
        )
        .unwrap();
        std::os::unix::fs::symlink(elsewhere.path().join("file"), previous.join("file-link"))
            .unwrap();
        std::os::unix::fs::symlink("missing", previous.join("volumes/dangling")).unwrap();
        assert!(delete_backup(dir.path(), None).unwrap());
        assert!(!previous.exists());
        assert_eq!(
            fs::read(elsewhere.path().join("folder/precious")).unwrap(),
            b"mine"
        );
        assert_eq!(fs::read(elsewhere.path().join("file")).unwrap(), b"mine");
    }

    #[test]
    fn a_retention_record_cannot_block_on_a_fifo() {
        use std::os::unix::{
            ffi::OsStrExt,
            fs::{FileTypeExt, OpenOptionsExt},
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        let encoded = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        // SAFETY: the path is NUL terminated and belongs to this temporary fixture.
        assert_eq!(unsafe { libc::mkfifo(encoded.as_ptr(), 0o600) }, 0);
        let app_data = dir.path().to_path_buf();
        let (send, receive) = std::sync::mpsc::channel();
        let reader = std::thread::spawn(move || {
            let _ = send.send(matches!(load(&app_data), Saved::Unreadable));
        });
        let result = receive.recv_timeout(Duration::from_secs(1));
        // Release a blocking reader before failing the regression, so the test
        // leaves no thread behind when run against the previous implementation.
        if result.is_err() {
            if let Ok(mut writer) = fs::OpenOptions::new()
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(&path)
            {
                let _ = io::Write::write_all(&mut writer, b"invalid record");
            }
        }
        reader.join().unwrap();
        assert!(result.unwrap());
        assert!(fs::symlink_metadata(path).unwrap().file_type().is_fifo());
    }

    #[test]
    fn oversized_and_redirected_retention_records_are_unreadable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        let valid = br#"{"version":1,"startedAt":"2020-01-01T00:00:00Z"}"#;
        let mut oversized = valid.to_vec();
        oversized.resize(64 * 1024, b' ');
        fs::write(&path, &oversized).unwrap();
        assert!(matches!(load(dir.path()), Saved::Valid { .. }));
        oversized.resize(64 * 1024 + 1, b' ');
        fs::write(&path, &oversized).unwrap();
        assert!(matches!(load(dir.path()), Saved::Unreadable));
        assert_eq!(fs::read(&path).unwrap(), oversized);
        fs::remove_file(&path).unwrap();
        let target = dir.path().join("other-record.json");
        fs::write(&target, valid).unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert!(matches!(load(dir.path()), Saved::Unreadable));
        assert_eq!(fs::read(&target).unwrap(), valid);
        assert!(fs::symlink_metadata(path).unwrap().file_type().is_symlink());
    }

    #[test]
    fn an_unreadable_record_keeps_the_backup_but_is_never_deleted_automatically() {
        for (name, contents) in [
            ("damaged", &b"{not json"[..]),
            (
                "later version",
                br#"{"version":2,"startedAt":"2020-01-01T00:00:00Z"}"#,
            ),
            ("bad date", br#"{"version":1,"startedAt":"last tuesday"}"#),
            (
                "deletion date overflow",
                br#"{"version":1,"startedAt":"9999-12-31T23:59:59Z"}"#,
            ),
            (
                "UTC deletion date overflow",
                br#"{"version":1,"startedAt":"9999-12-17T23:59:59-23:59"}"#,
            ),
        ] {
            let dir = complete();
            fs::write(dir.path().join(FILE), contents).unwrap();
            let now = at("2026-10-01T10:00:00Z");
            assert_eq!(
                status(dir.path(), now).unwrap(),
                Some(Status {
                    delete_at: None,
                    notice_pending: false
                }),
                "{name}"
            );
            assert!(!delete_if_due(dir.path(), None, now).unwrap(), "{name}");
            assert!(dir.path().join("runtime").is_dir(), "{name}");
            assert_eq!(fs::read(dir.path().join(FILE)).unwrap(), contents, "{name}");
            acknowledge(dir.path(), now).unwrap();
            assert_eq!(fs::read(dir.path().join(FILE)).unwrap(), contents, "{name}");
            // The user can still reclaim the space by hand.
            assert!(delete_backup(dir.path(), None).unwrap(), "{name}");
            assert!(!dir.path().join("runtime").exists(), "{name}");
        }
    }

    #[test]
    fn a_record_without_a_backup_is_forgotten() {
        let dir = complete();
        save(dir.path(), at("2026-10-01T10:00:00Z"), false).unwrap();
        fs::remove_dir_all(dir.path().join("runtime")).unwrap();
        assert_eq!(
            status(dir.path(), at("2026-10-02T10:00:00Z")).unwrap(),
            None
        );
        assert!(!dir.path().join(FILE).exists());
    }

    #[test]
    fn size_counts_allocated_bytes_without_following_links_or_counting_names_twice() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        fs::create_dir(&root).unwrap();
        // 1 GiB apparent, almost nothing allocated.
        let sparse = fs::File::create(root.join("sparse.raw")).unwrap();
        sparse.set_len(1024 * 1024 * 1024).unwrap();
        drop(sparse);
        fs::write(root.join("dense.bin"), vec![7; 1024 * 1024]).unwrap();
        let measured = allocated_bytes(&root).unwrap();
        assert!(
            (1024 * 1024..16 * 1024 * 1024).contains(&measured),
            "{measured}"
        );
        #[cfg(unix)]
        {
            let outside = tempfile::tempdir().unwrap();
            fs::write(outside.path().join("big"), vec![9; 8 * 1024 * 1024]).unwrap();
            std::os::unix::fs::symlink(outside.path(), root.join("link")).unwrap();
            let linked = allocated_bytes(&root).unwrap();
            assert!(linked < measured + 1024 * 1024, "{linked} {measured}");
            fs::hard_link(root.join("dense.bin"), root.join("second-name")).unwrap();
            assert!(allocated_bytes(&root).unwrap() < linked + 1024 * 1024);
        }
        assert!(allocated_bytes(&root.join("absent")).unwrap() == 0);
    }

    #[test]
    fn measuring_reports_the_backup_and_nothing_once_it_is_gone() {
        let dir = complete();
        let bytes = measure(dir.path()).unwrap().unwrap();
        assert!(bytes >= 64 * 1024, "{bytes}");
        assert!(delete_backup(dir.path(), None).unwrap());
        assert_eq!(measure(dir.path()).unwrap(), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_deletion_keeps_the_rest_the_record_and_the_row_so_it_can_be_retried() {
        use std::os::unix::fs::PermissionsExt;
        if unsafe { libc::geteuid() } == 0 {
            return; // root ignores directory permissions
        }
        let dir = complete();
        let now = at("2026-10-01T10:00:00Z");
        status(dir.path(), now).unwrap();
        let locked = dir.path().join("runtime/microsandbox/sandboxes/dev");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o500)).unwrap();
        let error = delete_backup(dir.path(), None).unwrap_err();
        assert!(error.contains("could not finish deleting"), "{error}");
        assert!(error.contains("Try again"), "{error}");
        assert!(!error.contains(dir.path().to_str().unwrap()), "{error}");
        assert!(dir.path().join(FILE).exists());
        assert!(status(dir.path(), now).unwrap().is_some());
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(delete_backup(dir.path(), None).unwrap());
        assert!(!dir.path().join("runtime").exists());
        assert!(!dir.path().join(FILE).exists());
    }

    fn descriptor(path: &str) -> String {
        format!("# Disk DescriptorFile\nversion=1\nRW 12 FLAT \"{path}\" 0\n")
    }

    #[test]
    fn a_converted_image_still_reading_the_backup_is_repaired_first_or_blocks_deletion() {
        let dir = complete();
        let app_data = dir.path();
        let cache = app_data.join(CONVERTED).join("microsandbox/cache");
        let old = app_data
            .join("runtime/microsandbox/cache/layers")
            .join(LAYER);
        let vmdk = cache.join("vmdk/image.vmdk");
        fs::write(&vmdk, descriptor(&old.display().to_string())).unwrap();
        // No copy in the converted cache: the image would stop booting.
        let error = delete_backup(app_data, None).unwrap_err();
        assert!(error.contains("still reads files"), "{error}");
        assert!(app_data.join("runtime").is_dir());
        // With the converted copy present, deletion points the image at it, then deletes.
        fs::create_dir_all(cache.join("layers")).unwrap();
        fs::write(cache.join("layers").join(LAYER), vec![0; 512]).unwrap();
        assert!(delete_backup(app_data, None).unwrap());
        assert!(!app_data.join("runtime").exists());
        let repaired = fs::read_to_string(&vmdk).unwrap();
        assert!(repaired.contains(CONVERTED), "{repaired}");
        assert!(!repaired.contains("runtime/microsandbox"), "{repaired}");
    }

    /// The alias the runtime makes for the storage `generation` under `app_data`.
    #[cfg(unix)]
    fn alias_for(home: &Path, app_data: &Path, generation: &str) -> PathBuf {
        crate::runtime::runtime_home_alias(home, &app_data.join(generation).join("microsandbox"))
    }

    /// Makes that alias as the runtime does: a symlink to the generation's `microsandbox`.
    #[cfg(unix)]
    fn link_alias(home: &Path, app_data: &Path, generation: &str) -> PathBuf {
        let alias = alias_for(home, app_data, generation);
        fs::create_dir_all(alias.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(app_data.join(generation).join("microsandbox"), &alias).unwrap();
        alias
    }

    /// Whether anything, even a dangling link, is at `path`.
    #[cfg(unix)]
    fn present(path: &Path) -> bool {
        fs::symlink_metadata(path).is_ok()
    }

    #[cfg(unix)]
    #[test]
    fn deleting_the_backup_removes_the_previous_alias_and_nothing_else_in_the_state_dir() {
        let dir = complete();
        let home = tempfile::tempdir().unwrap();
        let previous = link_alias(home.path(), dir.path(), "runtime");
        let converted = link_alias(home.path(), dir.path(), CONVERTED);
        // The alias is where the runtime puts it, which follows the running channel.
        assert_eq!(
            previous.parent().unwrap(),
            crate::channel::current().state_dir(home.path())
        );
        let state_dir = previous.parent().unwrap().to_path_buf();
        let elsewhere = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(elsewhere.path(), state_dir.join("0123456789ab")).unwrap();
        fs::write(state_dir.join("notes.txt"), b"mine").unwrap();
        fs::create_dir(state_dir.join("editor")).unwrap();

        assert!(delete_backup(dir.path(), Some(home.path())).unwrap());

        assert!(!dir.path().join("runtime").exists());
        assert!(!present(&previous));
        assert_eq!(
            fs::read_link(&converted).unwrap(),
            dir.path().join(CONVERTED).join("microsandbox")
        );
        assert_eq!(
            fs::read_link(state_dir.join("0123456789ab")).unwrap(),
            elsewhere.path()
        );
        assert_eq!(fs::read(state_dir.join("notes.txt")).unwrap(), b"mine");
        assert!(state_dir.join("editor").is_dir());
        // Repeating the deletion has nothing left to do.
        assert!(!delete_backup(dir.path(), Some(home.path())).unwrap());
        assert!(present(&converted));
    }

    #[cfg(unix)]
    #[test]
    fn the_automatic_deletion_removes_the_alias_only_once_the_backup_is_deleted() {
        let dir = complete();
        let home = tempfile::tempdir().unwrap();
        let previous = link_alias(home.path(), dir.path(), "runtime");
        let converted = link_alias(home.path(), dir.path(), CONVERTED);
        save(dir.path(), at("2026-10-01T10:00:00Z"), true).unwrap();
        let home = Some(home.path());
        // Not due: the folder is still there, and so is the alias that reaches it.
        assert!(!delete_if_due(dir.path(), home, at("2026-10-14T10:00:00Z")).unwrap());
        assert!(dir.path().join("runtime").is_dir());
        assert!(previous.exists());
        assert!(delete_if_due(dir.path(), home, at("2026-10-15T10:00:00Z")).unwrap());
        assert!(!dir.path().join("runtime").exists());
        assert!(!present(&previous));
        assert!(converted.exists());
    }

    #[cfg(unix)]
    #[test]
    fn an_alias_that_points_elsewhere_is_kept_with_its_target() {
        let elsewhere = tempfile::tempdir().unwrap();
        fs::write(elsewhere.path().join("precious"), b"mine").unwrap();
        for (name, target) in [
            ("another folder", elsewhere.path().to_path_buf()),
            (
                "another folder, same name",
                elsewhere.path().join("microsandbox"),
            ),
            ("a relative link", PathBuf::from("../runtime/microsandbox")),
        ] {
            let dir = complete();
            let home = tempfile::tempdir().unwrap();
            let alias = alias_for(home.path(), dir.path(), "runtime");
            fs::create_dir_all(alias.parent().unwrap()).unwrap();
            std::os::unix::fs::symlink(&target, &alias).unwrap();
            assert!(
                delete_backup(dir.path(), Some(home.path())).unwrap(),
                "{name}"
            );
            assert!(!dir.path().join("runtime").exists(), "{name}");
            assert_eq!(fs::read_link(&alias).unwrap(), target, "{name}");
            assert_eq!(
                fs::read(elsewhere.path().join("precious")).unwrap(),
                b"mine",
                "{name}"
            );
        }
        // The converted generation's own folder is not the previous alias's target either.
        let dir = complete();
        let home = tempfile::tempdir().unwrap();
        let alias = alias_for(home.path(), dir.path(), "runtime");
        fs::create_dir_all(alias.parent().unwrap()).unwrap();
        let converted = dir.path().join(CONVERTED).join("microsandbox");
        std::os::unix::fs::symlink(&converted, &alias).unwrap();
        assert!(delete_backup(dir.path(), Some(home.path())).unwrap());
        assert_eq!(fs::read_link(&alias).unwrap(), converted);
        assert!(converted.join("cache/vmdk").is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_or_file_at_the_alias_path_is_never_removed() {
        for as_folder in [true, false] {
            let dir = complete();
            let home = tempfile::tempdir().unwrap();
            let alias = alias_for(home.path(), dir.path(), "runtime");
            fs::create_dir_all(alias.parent().unwrap()).unwrap();
            if as_folder {
                fs::create_dir_all(alias.join("microsandbox")).unwrap();
                fs::write(alias.join("keep.txt"), b"mine").unwrap();
            } else {
                fs::write(&alias, b"mine").unwrap();
            }
            assert!(delete_backup(dir.path(), Some(home.path())).unwrap());
            assert!(!dir.path().join("runtime").exists());
            if as_folder {
                assert_eq!(fs::read(alias.join("keep.txt")).unwrap(), b"mine");
                assert!(alias.join("microsandbox").is_dir());
            } else {
                assert_eq!(fs::read(&alias).unwrap(), b"mine");
            }
            // The check that finds the folder already gone keeps it as well.
            assert!(
                !delete_if_due(dir.path(), Some(home.path()), at("2026-10-01T10:00:00Z")).unwrap()
            );
            assert!(present(&alias));
        }
    }

    #[cfg(unix)]
    #[test]
    fn the_converted_generations_alias_is_untouched_when_there_is_no_previous_alias() {
        let dir = complete();
        let home = tempfile::tempdir().unwrap();
        let converted = link_alias(home.path(), dir.path(), CONVERTED);
        let state_dir = converted.parent().unwrap().to_path_buf();
        assert!(delete_backup(dir.path(), Some(home.path())).unwrap());
        assert!(!delete_backup(dir.path(), Some(home.path())).unwrap());
        assert!(!delete_if_due(dir.path(), Some(home.path()), at("2026-10-01T10:00:00Z")).unwrap());
        assert_eq!(
            fs::read_link(&converted).unwrap(),
            dir.path().join(CONVERTED).join("microsandbox")
        );
        assert_eq!(fs::read_dir(state_dir).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn a_dangling_alias_is_removed_when_the_backup_was_already_deleted_by_hand() {
        for by_check in [true, false] {
            let dir = complete();
            let home = tempfile::tempdir().unwrap();
            let previous = link_alias(home.path(), dir.path(), "runtime");
            let converted = link_alias(home.path(), dir.path(), CONVERTED);
            save(dir.path(), at("2026-10-01T10:00:00Z"), false).unwrap();
            fs::remove_dir_all(dir.path().join("runtime")).unwrap();
            assert!(!previous.exists() && present(&previous), "it dangles");
            let home = Some(home.path());
            if by_check {
                // The check at launch, and every hour after it.
                assert!(!delete_if_due(dir.path(), home, at("2026-10-02T10:00:00Z")).unwrap());
            } else {
                assert!(!delete_backup(dir.path(), home).unwrap());
            }
            assert!(!present(&previous));
            assert!(converted.exists());
            assert!(!dir.path().join(FILE).exists());
            // Once removed, later checks leave everything alone.
            assert!(!delete_if_due(dir.path(), home, at("2026-10-03T10:00:00Z")).unwrap());
            assert!(converted.exists());
        }
    }

    #[cfg(unix)]
    #[test]
    fn no_alias_is_touched_unless_the_migration_completed_into_the_converted_generation() {
        for (name, dir) in [
            ("after Continue", migrated(Some(CLEAN), Some("complete"))),
            ("restarting", migrated(Some(CONVERTED), Some("running"))),
            ("failed", migrated(Some(CONVERTED), Some("failed"))),
            ("no migration record", migrated(Some(CONVERTED), None)),
            ("never migrated", migrated(None, None)),
        ] {
            let home = tempfile::tempdir().unwrap();
            // The alias is exactly the one the runtime made for the previous generation.
            let alias = link_alias(home.path(), dir.path(), "runtime");
            let now = at("2026-10-01T10:00:00Z");
            let home = Some(home.path());
            assert!(delete_backup(dir.path(), home).is_err(), "{name}");
            assert!(!delete_if_due(dir.path(), home, now).unwrap(), "{name}");
            assert!(present(&alias), "{name}");
            // Even with the folder missing, it is not Silo's to tidy.
            fs::remove_dir_all(dir.path().join("runtime")).unwrap();
            assert!(delete_backup(dir.path(), home).is_err(), "{name}");
            assert!(!delete_if_due(dir.path(), home, now).unwrap(), "{name}");
            assert!(present(&alias), "{name}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_backup_that_is_refused_keeps_its_alias() {
        let dir = complete();
        let home = tempfile::tempdir().unwrap();
        let alias = link_alias(home.path(), dir.path(), "runtime");
        let elsewhere = tempfile::tempdir().unwrap();
        fs::remove_dir_all(dir.path().join("runtime")).unwrap();
        std::os::unix::fs::symlink(elsewhere.path(), dir.path().join("runtime")).unwrap();
        assert!(delete_backup(dir.path(), Some(home.path())).is_err());
        assert!(!delete_if_due(dir.path(), Some(home.path()), at("2030-01-01T00:00:00Z")).unwrap());
        assert!(present(&alias));
    }

    #[cfg(unix)]
    #[test]
    fn an_unknown_or_empty_home_never_fails_the_deletion() {
        let dir = complete();
        let home = tempfile::tempdir().unwrap();
        let alias = link_alias(home.path(), dir.path(), "runtime");
        assert!(delete_backup(dir.path(), None).unwrap());
        assert!(
            present(&alias),
            "without a home Silo cannot tell where it is"
        );
        // A home with no state directory at all.
        let dir = complete();
        let empty = tempfile::tempdir().unwrap();
        assert!(delete_backup(dir.path(), Some(empty.path())).unwrap());
        assert!(!dir.path().join("runtime").exists());
        assert_eq!(fs::read_dir(empty.path()).unwrap().count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn failing_to_remove_the_alias_does_not_fail_the_deletion_and_is_retried_by_the_next_check() {
        use std::os::unix::fs::PermissionsExt;
        if unsafe { libc::geteuid() } == 0 {
            return; // root ignores directory permissions
        }
        let dir = complete();
        let home = tempfile::tempdir().unwrap();
        let alias = link_alias(home.path(), dir.path(), "runtime");
        let state_dir = alias.parent().unwrap().to_path_buf();
        status(dir.path(), at("2026-10-01T10:00:00Z")).unwrap();
        // Unlinking needs write access to the folder that holds the link.
        fs::set_permissions(&state_dir, fs::Permissions::from_mode(0o500)).unwrap();
        let outcome = delete_backup(dir.path(), Some(home.path()));
        fs::set_permissions(&state_dir, fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(outcome, Ok(true));
        assert!(!dir.path().join("runtime").exists());
        assert!(
            !dir.path().join(FILE).exists(),
            "the record goes with the backup"
        );
        assert!(present(&alias), "the alias was left, only logged");
        assert_eq!(
            status(dir.path(), at("2026-10-01T11:00:00Z")).unwrap(),
            None
        );
        // The next check finds the folder gone and the alias still there, and finishes.
        assert!(!delete_if_due(dir.path(), Some(home.path()), at("2026-10-01T12:00:00Z")).unwrap());
        assert!(!present(&alias));
    }
}
