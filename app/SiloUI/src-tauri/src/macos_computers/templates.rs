//! Templates: a set-up macOS computer kept stopped as the source of later computers.
//!
//! A template is a folder in `macos-templates/` named `<build>-<setup version>`
//! holding APFS clones of a finished computer's disk, auxiliary storage and
//! hardware model, a `template.json`, and `template-access/`, the guest-access
//! secrets of the computer it came from (the first login to a copy needs them).
//! The structure follows `cua-vmm` (trycua/cua, MIT, commit ba4c636): a base image
//! that is cloned with `clonefile`, a free-space check before the clone, and
//! bookkeeping of what is in use. Lume's clone (`LumeController.clone`, MIT) is the
//! model for what a copy changes: a new MAC address and a new machine identifier;
//! the disk is grown, never shrunk, as Lume and Tart do.
//!
//! Nothing here touches Virtualization.framework, so it runs on every platform.
use super::{
    guest_access, guest_computer_use, offline_setup,
    store::{self, Layout, Record},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::Mutex,
};

const SCHEMA_VERSION: u32 = 1;
const META: &str = "template.json";
const ACCESS: &str = "template-access";
/// The secrets of the source computer that a first login to a copy needs.
const ACCESS_FILES: [&str; 3] = ["password", "id_ed25519", "id_ed25519.pub"];
const PARTIAL: &str = ".partial-";
const GIB: u64 = 1 << 30;
/// What a copy writes before it is first started: the personalization and two boots.
pub(super) const COPY_ESTIMATE: u64 = 4 * GIB;
/// Free space a copy leaves to the Mac.
const FREE_RESERVE: u64 = 5 * GIB;

/// Bump to invalidate every template when the setup changes in a way no hashed input shows.
const SETUP_REVISION: u32 = 1;

pub(super) fn root(app_data: &Path) -> PathBuf {
    app_data.join("macos-templates")
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Meta {
    pub schema_version: u32,
    pub macos_version: String,
    pub build: String,
    /// The base fingerprint: everything setup does except computer use.
    pub setup_version: String,
    /// The computer-use fingerprint the template's guest has; a template without one
    /// (made before the split) counts as stale.
    #[serde(default)]
    pub computer_use_version: String,
    #[serde(rename = "diskGiB")]
    pub disk_gib: u64,
    pub created_at: String,
    pub source_computer_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Template {
    /// The folder name.
    pub name: String,
    pub dir: PathBuf,
    pub meta: Meta,
}

impl Template {
    pub(super) fn disk(&self) -> PathBuf {
        self.dir.join("disk.img")
    }

    pub(super) fn auxiliary_storage(&self) -> PathBuf {
        self.dir.join("auxiliary-storage.img")
    }

    pub(super) fn hardware_model(&self) -> PathBuf {
        self.dir.join("hardware-model.bin")
    }

    /// The source computer's guest-access secrets.
    pub(super) fn access_dir(&self) -> PathBuf {
        self.dir.join(ACCESS)
    }
}

// MARK: Setup version

/// The base fingerprint: a hash of what setup does to a computer apart from computer
/// use (the account and its automatic login, SSH, the recovery step). A template made
/// under a different base is never copied; a computer's base is fixed when its setup ends.
pub(super) fn setup_version() -> String {
    hash_inputs([
        offline_setup::setup_inputs().as_str(),
        &SETUP_REVISION.to_string(),
    ])
}

/// The computer-use fingerprint of a computer set up with `approval`: the pinned ChatGPT
/// app and LCU, the guest script and the approval mode. It changes whenever those do, and
/// a computer or template with another one is brought up to date in place; it is never a
/// reason to install macOS again.
pub(super) fn computer_use_version_for(approval: crate::computer_use::Approval) -> String {
    hash_inputs(
        guest_computer_use::setup_inputs()
            .into_iter()
            .chain([approval.as_str()]),
    )
}

/// The computer-use fingerprint a computer created now gets.
pub(super) fn computer_use_version() -> String {
    computer_use_version_for(crate::computer_use::initial_approval())
}

fn hash_inputs<'a>(inputs: impl IntoIterator<Item = &'a str>) -> String {
    let mut hash = Sha256::new();
    for input in inputs {
        // Length-prefixed, so that moving text between two inputs changes the hash.
        hash.update((input.len() as u64).to_le_bytes());
        hash.update(input.as_bytes());
    }
    hash.finalize()[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The folder name of the template for `build` under `setup_version` and
/// `computer_use_version`.
pub(super) fn dir_name(
    build: &str,
    setup_version: &str,
    computer_use_version: &str,
) -> Result<String, String> {
    let safe = |text: &str| {
        !text.is_empty()
            && text
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
    };
    if safe(build) && safe(setup_version) && safe(computer_use_version) {
        Ok(format!("{build}-{setup_version}-{computer_use_version}"))
    } else {
        Err("The macOS build has an unexpected name.".into())
    }
}

// MARK: Reading

/// Every complete template, newest first. Unfinished and unreadable folders are skipped.
pub(super) fn list(app_data: &Path) -> Vec<Template> {
    let Ok(entries) = fs::read_dir(root(app_data)) else {
        return Vec::new();
    };
    let mut templates: Vec<Template> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            if name.contains(PARTIAL) {
                return None;
            }
            let meta: Meta =
                serde_json::from_slice(&fs::read(entry.path().join(META)).ok()?).ok()?;
            (meta.schema_version == SCHEMA_VERSION).then(|| Template {
                name,
                dir: entry.path(),
                meta,
            })
        })
        .collect();
    templates.sort_by(|a, b| created(b).cmp(&created(a)).then(a.name.cmp(&b.name)));
    templates
}

fn created(template: &Template) -> Option<time::OffsetDateTime> {
    time::OffsetDateTime::parse(
        &template.meta.created_at,
        &time::format_description::well_known::Rfc3339,
    )
    .ok()
}

/// The template a new computer is copied from. `latest_build` is the build the framework
/// offers; with `None` (the lookup failed, as when offline) the newest template of the
/// current base is used. A template whose base matches is usable even when its computer-use
/// part is stale (the copy updates it); one that is up to date is preferred.
pub(super) fn choose<'a>(
    templates: &'a [Template],
    latest_build: Option<&str>,
    setup_version: &str,
    computer_use_version: &str,
) -> Option<&'a Template> {
    let usable = |template: &&Template| {
        template.meta.setup_version == setup_version
            && latest_build.is_none_or(|build| template.meta.build == build)
    };
    templates
        .iter()
        .filter(usable)
        .find(|template| template.meta.computer_use_version == computer_use_version)
        .or_else(|| templates.iter().find(usable))
}

// MARK: Leases

/// Folders of templates that copies are being made from, with their counts.
static LEASES: Mutex<Option<HashMap<PathBuf, usize>>> = Mutex::new(None);

fn leases() -> std::sync::MutexGuard<'static, Option<HashMap<PathBuf, usize>>> {
    LEASES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A hold on a template: it cannot be pruned or removed while one exists.
pub(super) struct Lease {
    pub template: Template,
}

impl Lease {
    fn take(leases: &mut HashMap<PathBuf, usize>, template: Template) -> Self {
        *leases.entry(template.dir.clone()).or_default() += 1;
        Self { template }
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        let mut guard = leases();
        if let Some(leases) = guard.as_mut() {
            if let Some(count) = leases.get_mut(&self.template.dir) {
                *count -= 1;
                if *count == 0 {
                    leases.remove(&self.template.dir);
                }
            }
        }
    }
}

/// Holds the template a new computer is copied from, if there is one.
pub(super) fn lease_matching(
    app_data: &Path,
    latest_build: Option<&str>,
    setup_version: &str,
    computer_use_version: &str,
) -> Option<Lease> {
    let mut guard = leases();
    let templates = list(app_data);
    let chosen = choose(
        &templates,
        latest_build,
        setup_version,
        computer_use_version,
    )?
    .clone();
    Some(Lease::take(guard.get_or_insert_with(HashMap::new), chosen))
}

/// Holds the named template again, for a copy whose personalization is resumed.
pub(super) fn lease_named(app_data: &Path, name: &str) -> Result<Lease, String> {
    let mut guard = leases();
    let template = list(app_data)
        .into_iter()
        .find(|template| template.name == name)
        .ok_or(TEMPLATE_GONE)?;
    Ok(Lease::take(
        guard.get_or_insert_with(HashMap::new),
        template,
    ))
}

pub(super) const TEMPLATE_GONE: &str =
    "The template this computer was copied from is gone. Delete this computer and create it again.";

// MARK: Removing

fn remove_dir(path: &Path) -> Result<(), String> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(store::io_error("remove the template", &error)),
    }
}

type Leases = std::sync::MutexGuard<'static, Option<HashMap<PathBuf, usize>>>;

/// Removes every template but the newest that no copy is made from. `protected` names the
/// templates computers still depend on; it is read while the leases are locked, so a copy
/// that starts to depend on a template cannot slip between the two checks. The newest is
/// chosen here, under the same lock a publication takes, so one published meanwhile is
/// never mistaken for an old one.
fn prune_locked(
    guard: &Leases,
    app_data: &Path,
    protected: &dyn Fn() -> Vec<String>,
) -> Result<(), String> {
    let held = guard.as_ref();
    let protected = protected();
    let templates = list(app_data);
    let newest = templates.first().map(|template| template.name.clone());
    let mut result = Ok(());
    for template in templates {
        let used = held.is_some_and(|held| held.contains_key(&template.dir))
            || protected.contains(&template.name);
        if Some(&template.name) != newest.as_ref() && !used {
            if let Err(message) = remove_dir(&template.dir) {
                result = Err(message);
            }
        }
    }
    result
}

/// Removes every template but the newest that nothing needs. Run when a copy ends, so a
/// template a copy held is not kept for good.
pub(super) fn prune_stale(
    app_data: &Path,
    protected: &dyn Fn() -> Vec<String>,
) -> Result<(), String> {
    prune_locked(&leases(), app_data, protected)
}

/// Removes every template, unless a copy is being made from one or an unfinished copy
/// depends on one.
pub(super) fn remove_all(
    app_data: &Path,
    protected: &dyn Fn() -> Vec<String>,
) -> Result<(), String> {
    let guard = leases();
    let templates = list(app_data);
    if templates.iter().any(|template| {
        guard
            .as_ref()
            .is_some_and(|held| held.contains_key(&template.dir))
    }) {
        return Err("A computer is being copied from the template. Wait for it to finish.".into());
    }
    let protected = protected();
    if templates
        .iter()
        .any(|template| protected.contains(&template.name))
    {
        return Err(
            "A computer is still being set up from the template. Finish or delete it first.".into(),
        );
    }
    for template in templates {
        remove_dir(&template.dir)?;
    }
    // Folders an interrupted run left half-made.
    remove_partials(app_data)
}

fn remove_partials(app_data: &Path) -> Result<(), String> {
    let Ok(entries) = fs::read_dir(root(app_data)) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().contains(PARTIAL) {
            remove_dir(&entry.path())?;
        }
    }
    Ok(())
}

// MARK: Making

/// Whether `record` may become a template: it finished its setup, was never started by the
/// user afterwards, and is not itself a copy of a template.
pub(super) fn eligible(record: &Record) -> bool {
    record.installed
        && record.setup_version.is_some()
        && record.setup.complete()
        && record.pristine
        && record.template.is_none()
        && record.restore_image.is_some()
}

/// Whether `record` is a finished copy of a stale template that was brought up to date
/// during its setup, so a fresh template may be made from it. The copy keeps the size of
/// the template it came from (a larger disk would raise the size every later copy needs),
/// and its macOS build and base fingerprint are the template's.
pub(super) fn refreshes(app_data: &Path, record: &Record) -> bool {
    let (Some(name), Some(image), Some(base)) = (
        record.template.as_deref(),
        record.restore_image.as_ref(),
        record.setup_version.as_deref(),
    ) else {
        return false;
    };
    record.installed
        && record.setup.complete()
        && !record.inherited_access
        && !record.computer_use_stale()
        && list(app_data).iter().any(|template| {
            template.name == name
                && template.meta.disk_gib == record.disk_gib
                && template.meta.build == image.build
                && template.meta.setup_version == base
                && Some(template.meta.computer_use_version.as_str())
                    != record.computer_use_version.as_deref()
        })
}

/// Serializes template creation: one folder name is made at a time.
static MAKING: Mutex<()> = Mutex::new(());

/// Makes the template of a finished, stopped computer, then removes older ones. Does nothing
/// when a template for this build and setup version exists. `refresh` is true for a copy
/// whose computer use was updated (see `refreshes`, which the caller has checked), which
/// is not pristine and has a template of its own. Returns the folder name made.
pub(super) fn make(
    app_data: &Path,
    record: &Record,
    layout: &Layout,
    setup_version: &str,
    computer_use_version: &str,
    refresh: bool,
    protected: &dyn Fn() -> Vec<String>,
) -> Result<Option<String>, String> {
    if !refresh && !eligible(record) {
        return Ok(None);
    }
    let image = record.restore_image.as_ref().ok_or("No macOS version.")?;
    let name = dir_name(&image.build, setup_version, computer_use_version)?;
    let _making = MAKING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let templates = root(app_data);
    let target = templates.join(&name);
    remove_partials(app_data)?;
    if target.join(META).exists() {
        return Ok(None);
    }
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&templates)
        .map_err(|error| store::io_error("create the template folder", &error))?;
    let partial = templates.join(format!("{name}{PARTIAL}{}", uuid::Uuid::new_v4()));
    let built = build(
        &partial,
        record,
        layout,
        setup_version,
        computer_use_version,
        image,
    );
    if let Err(message) = built {
        let _ = remove_dir(&partial);
        return Err(message);
    }
    // Publishing and pruning happen under one lock, so a prune never runs between them.
    let guard = leases();
    if let Err(error) = fs::rename(&partial, &target) {
        let _ = remove_dir(&partial);
        return Err(store::io_error("save the template", &error));
    }
    prune_locked(&guard, app_data, protected)?;
    Ok(Some(name))
}

fn build(
    partial: &Path,
    record: &Record,
    layout: &Layout,
    setup_version: &str,
    computer_use_version: &str,
    image: &store::RestoreImageInfo,
) -> Result<(), String> {
    fs::DirBuilder::new()
        .mode(0o700)
        .create(partial)
        .map_err(|error| store::io_error("create the template", &error))?;
    for (from, to) in [
        (layout.disk(), "disk.img"),
        (layout.auxiliary_storage(), "auxiliary-storage.img"),
        (layout.hardware_model(), "hardware-model.bin"),
    ] {
        clone_file(&from, &partial.join(to)).map_err(CopyError::message)?;
    }
    let access = partial.join(ACCESS);
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&access)
        .map_err(|error| store::io_error("create the template", &error))?;
    let source = guest_access::credentials_dir(layout);
    for file in ACCESS_FILES {
        let bytes = fs::read(source.join(file))
            .map_err(|error| store::io_error("copy the computer's access", &error))?;
        write_private(&access.join(file), &bytes)?;
    }
    let meta = Meta {
        schema_version: SCHEMA_VERSION,
        macos_version: image.version.clone(),
        build: image.build.clone(),
        setup_version: setup_version.to_string(),
        computer_use_version: computer_use_version.to_string(),
        disk_gib: record.disk_gib,
        created_at: time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default(),
        source_computer_id: record.id.clone(),
    };
    let json = serde_json::to_vec_pretty(&meta).map_err(|error| error.to_string())?;
    write_private(&partial.join(META), &json)
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .and_then(|mut file| file.write_all(bytes))
        .map_err(|error| store::io_error("save the template", &error))
}

// MARK: Copying

/// A copy cannot be smaller than its template; the disk is only ever grown.
pub(super) fn check_disk(requested_gib: u64, template: &Template) -> Result<(), String> {
    if requested_gib < template.meta.disk_gib {
        return Err(format!(
            "This macOS template needs at least {} GiB of disk. Create the computer again with a larger disk.",
            template.meta.disk_gib
        ));
    }
    Ok(())
}

/// Copies a template's files into the new computer's folder. The disk is the template's
/// size or, when `disk_gib` is larger, grown to it; the guest uses the extra space once
/// personalization has expanded its APFS container. The template's secrets are not copied.
pub(super) fn clone_into(
    template: &Template,
    layout: &Layout,
    machine_identifier: &[u8],
    disk_gib: u64,
) -> Result<(), CopyError> {
    let copied = (|| {
        fs::create_dir_all(&layout.dir)
            .map_err(|error| CopyError::Failed(store::io_error("create the computer", &error)))?;
        for (from, to) in [
            (template.disk(), layout.disk()),
            (template.auxiliary_storage(), layout.auxiliary_storage()),
            (template.hardware_model(), layout.hardware_model()),
        ] {
            clone_file(&from, &to)?;
        }
        fs::write(layout.machine_identifier(), machine_identifier).map_err(|error| {
            CopyError::Failed(store::io_error("save the computer's identity", &error))
        })?;
        if disk_gib > template.meta.disk_gib {
            fs::OpenOptions::new()
                .write(true)
                .open(layout.disk())
                .and_then(|file| file.set_len(disk_gib * GIB))
                .map_err(|error| CopyError::Failed(store::io_error("grow the disk", &error)))?;
        }
        Ok(())
    })();
    if copied.is_err() {
        for file in [
            layout.disk(),
            layout.auxiliary_storage(),
            layout.hardware_model(),
            layout.machine_identifier(),
        ] {
            let _ = fs::remove_file(file);
        }
    }
    copied
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum CopyError {
    /// The volume cannot clone files (it is not APFS).
    Unsupported,
    Failed(String),
}

impl CopyError {
    pub(super) fn message(self) -> String {
        match self {
            Self::Unsupported => "Silo's data folder is on a volume that cannot copy files instantly (it needs APFS).".into(),
            Self::Failed(message) => message,
        }
    }
}

/// Copy-on-write copy of one file. On macOS this is `clonefile`, which shares the blocks
/// and costs no space until one side changes; elsewhere (the tests, on Linux) it is a
/// plain copy.
#[cfg(target_os = "macos")]
pub(super) fn clone_file(from: &Path, to: &Path) -> Result<(), CopyError> {
    use std::os::unix::ffi::OsStrExt;
    let cstring = |path: &Path| {
        std::ffi::CString::new(path.as_os_str().as_bytes())
            .map_err(|_| CopyError::Failed("A file name contains a null byte.".into()))
    };
    let (from, to) = (cstring(from)?, cstring(to)?);
    // SAFETY: Both arguments are NUL-terminated paths that outlive the call.
    if unsafe { libc::clonefile(from.as_ptr(), to.as_ptr(), 0) } == 0 {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    match error.raw_os_error() {
        Some(libc::ENOTSUP) => Err(CopyError::Unsupported),
        _ => Err(CopyError::Failed(store::io_error(
            "copy the computer's files",
            &error,
        ))),
    }
}

#[cfg(not(target_os = "macos"))]
pub(super) fn clone_file(from: &Path, to: &Path) -> Result<(), CopyError> {
    fs::copy(from, to)
        .map(|_| ())
        .map_err(|error| CopyError::Failed(store::io_error("copy the computer's files", &error)))
}

// MARK: Space

/// Bytes available to this process on the volume holding `path`.
fn available_space(path: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let mut probe = path;
    while !probe.exists() {
        probe = probe.parent()?;
    }
    let path = std::ffi::CString::new(probe.as_os_str().as_bytes()).ok()?;
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: `path` is NUL-terminated and `stats` is a valid out-pointer.
    if unsafe { libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: statvfs succeeded and initialised the structure.
    let stats = unsafe { stats.assume_init() };
    Some(u64::from(stats.f_bavail) * u64::from(stats.f_frsize))
}

fn enough_space(available: Option<u64>, needed: u64) -> Result<(), String> {
    match available {
        Some(available) if available < needed.saturating_add(FREE_RESERVE) => Err(format!(
            "This Mac is low on disk space: about {} GB are free and this needs {} GB more than the {} GB Silo leaves free.",
            available / 1_000_000_000,
            needed.div_ceil(1_000_000_000),
            FREE_RESERVE / 1_000_000_000
        )),
        _ => Ok(()),
    }
}

/// Bytes that copies in progress are still expected to write.
static PENDING: Mutex<u64> = Mutex::new(0);

/// Space set aside for one copy until it is dropped.
pub(super) struct SpaceReservation(u64);

impl Drop for SpaceReservation {
    fn drop(&mut self) {
        let mut pending = PENDING
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *pending = pending.saturating_sub(self.0);
    }
}

/// Sets `needed` bytes aside on the volume holding `app_data`, failing when that leaves
/// less than the reserve Silo keeps for the Mac once the copies already under way have
/// written theirs too. An unknown amount of free space is not checked.
pub(super) fn reserve_space(app_data: &Path, needed: u64) -> Result<SpaceReservation, String> {
    reserve_in(app_data, needed, available_space)
}

fn reserve_in(
    app_data: &Path,
    needed: u64,
    available: impl FnOnce(&Path) -> Option<u64>,
) -> Result<SpaceReservation, String> {
    let mut pending = PENDING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let free = available(app_data).map(|free| free.saturating_sub(*pending));
    enough_space(free, needed)?;
    *pending += needed;
    Ok(SpaceReservation(needed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::macos_computers::store::{CreateRequest, RestoreImageInfo, SetupProgress};

    fn record(disk_gib: u64) -> (Record, Layout, tempfile::TempDir) {
        let data = tempfile::tempdir().unwrap();
        let mut record = store::new_record(
            &CreateRequest {
                name: "mac-one".into(),
                cpus: 4,
                memory_gib: 8,
                disk_gib,
            },
            "02:00:00:00:00:01".into(),
        );
        record.installed = true;
        record.setup_version = Some("abcd".into());
        record.restore_image = Some(RestoreImageInfo {
            version: "26.6.2".into(),
            build: "25G83".into(),
        });
        record.setup = SetupProgress {
            account: true,
            sip: true,
            computer_use: true,
            clipboard: true,
            needs_personalizing: false,
        };
        let layout = Layout::new(data.path(), &record.id);
        fs::create_dir_all(&layout.dir).unwrap();
        fs::write(layout.disk(), vec![7u8; 4096]).unwrap();
        fs::write(layout.auxiliary_storage(), b"aux").unwrap();
        fs::write(layout.hardware_model(), b"model").unwrap();
        fs::write(layout.machine_identifier(), b"source-identifier").unwrap();
        let access = guest_access::credentials_dir(&layout);
        fs::create_dir_all(&access).unwrap();
        for file in ACCESS_FILES {
            fs::write(access.join(file), format!("secret {file}")).unwrap();
        }
        fs::write(access.join("known_hosts"), b"host").unwrap();
        (record, layout, data)
    }

    fn none() -> Vec<String> {
        Vec::new()
    }

    fn meta(build: &str, setup: &str, created: &str) -> Meta {
        meta_with(build, setup, "cu", created)
    }

    fn meta_with(build: &str, setup: &str, computer_use: &str, created: &str) -> Meta {
        Meta {
            schema_version: SCHEMA_VERSION,
            macos_version: "26.6.2".into(),
            build: build.into(),
            setup_version: setup.into(),
            computer_use_version: computer_use.into(),
            disk_gib: 64,
            created_at: created.into(),
            source_computer_id: "source".into(),
        }
    }

    fn template(build: &str, setup: &str, created: &str) -> Template {
        template_with(build, setup, "cu", created)
    }

    fn template_with(build: &str, setup: &str, computer_use: &str, created: &str) -> Template {
        let name = dir_name(build, setup, computer_use).unwrap();
        Template {
            dir: PathBuf::from(&name),
            name,
            meta: meta_with(build, setup, computer_use, created),
        }
    }

    #[test]
    fn a_template_is_chosen_by_build_and_setup_version() {
        let templates = [
            template("25G83", "old", "2026-10-02"),
            template("25G83", "new", "2026-10-01"),
            template("25H1", "new", "2026-10-03"),
        ];
        let pick = |build, setup| choose(&templates, build, setup, "cu").map(|t| t.name.as_str());
        assert_eq!(pick(Some("25G83"), "new"), Some("25G83-new-cu"));
        assert_eq!(pick(Some("25G83"), "old"), Some("25G83-old-cu"));
        assert_eq!(pick(Some("25H1"), "new"), Some("25H1-new-cu"));
        // A newer macOS than any template, or a changed setup, means a fresh install.
        assert_eq!(pick(Some("25J9"), "new"), None);
        assert_eq!(pick(Some("25G83"), "other"), None);
    }

    #[test]
    fn offline_the_newest_template_of_the_current_setup_is_used() {
        let templates = [
            template("25G83", "old", "2026-10-04"),
            template("25H1", "new", "2026-10-03"),
            template("25G83", "new", "2026-10-01"),
        ];
        assert_eq!(
            choose(&templates, None, "new", "cu").map(|t| t.name.as_str()),
            Some("25H1-new-cu")
        );
        assert_eq!(choose(&templates, None, "other", "cu"), None);
        assert_eq!(choose(&[], None, "new", "cu"), None);
    }

    #[test]
    fn a_template_with_the_base_but_a_stale_computer_use_part_is_still_chosen() {
        let templates = [
            template_with("25G83", "base", "old", "2026-10-03"),
            template_with("25G83", "other", "new", "2026-10-02"),
        ];
        let pick = |build, base, cu| choose(&templates, build, base, cu).map(|t| t.name.as_str());
        // Same base, other computer-use part: the copy updates computer use in place.
        assert_eq!(pick(Some("25G83"), "base", "new"), Some("25G83-base-old"));
        assert_eq!(pick(None, "base", "new"), Some("25G83-base-old"));
        // Another base, or another macOS build, needs a full install.
        assert_eq!(pick(Some("25G83"), "changed", "new"), None);
        assert_eq!(pick(Some("25H1"), "base", "old"), None);
        // An up-to-date template wins over a newer stale one.
        let both = [
            template_with("25G83", "base", "old", "2026-10-03"),
            template_with("25G83", "base", "new", "2026-10-01"),
        ];
        assert_eq!(
            choose(&both, Some("25G83"), "base", "new").map(|t| t.name.as_str()),
            Some("25G83-base-new")
        );
    }

    #[test]
    fn the_two_fingerprints_depend_on_their_own_inputs_only() {
        use crate::computer_use::Approval;
        assert_eq!(setup_version(), setup_version());
        assert_eq!(
            computer_use_version_for(Approval::Ask),
            computer_use_version_for(Approval::Ask)
        );
        assert_ne!(
            computer_use_version_for(Approval::Ask),
            computer_use_version_for(Approval::Auto)
        );
        assert_eq!(computer_use_version_for(Approval::Ask).len(), 16);
        // The base names no computer-use input.
        let base = hash_inputs([
            offline_setup::setup_inputs().as_str(),
            &SETUP_REVISION.to_string(),
        ]);
        assert_eq!(setup_version(), base);
        // A template made before the split has no computer-use part.
        let json = serde_json::to_value(meta("25G83", "a", "2026-10-01")).unwrap();
        let mut old = json.clone();
        old.as_object_mut().unwrap().remove("computerUseVersion");
        let parsed: Meta = serde_json::from_value(old).unwrap();
        assert_eq!(parsed.computer_use_version, "");
    }

    #[test]
    fn the_setup_version_changes_with_any_input() {
        let base = hash_inputs(["a", "b"]);
        assert_eq!(base, hash_inputs(["a", "b"]));
        assert_ne!(base, hash_inputs(["a", "c"]));
        assert_ne!(base, hash_inputs(["ab", ""]));
        assert_ne!(base, hash_inputs(["a"]));
        assert_eq!(base.len(), 16);
        assert_eq!(setup_version().len(), 16);
    }

    #[test]
    fn folder_names_stay_inside_the_templates_folder() {
        assert_eq!(
            dir_name("25G83", "0123abcd", "ef01").unwrap(),
            "25G83-0123abcd-ef01"
        );
        for bad in ["", "../x", "a/b", "a b"] {
            assert!(dir_name(bad, "x", "y").is_err(), "{bad}");
            assert!(dir_name("x", bad, "y").is_err(), "{bad}");
            assert!(dir_name("x", "y", bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn only_a_pristine_finished_install_may_become_a_template() {
        let (mut computer, _, _data) = record(64);
        assert!(eligible(&computer));
        computer.pristine = false;
        assert!(!eligible(&computer));
        computer.pristine = true;
        computer.setup_version = None;
        assert!(!eligible(&computer));
        computer.setup_version = Some("abcd".into());
        computer.setup.clipboard = false;
        assert!(!eligible(&computer));
        computer.setup.clipboard = true;
        computer.template = Some("25G83-x-cu".into());
        assert!(!eligible(&computer));
        computer.template = None;
        computer.setup.needs_personalizing = true;
        assert!(!eligible(&computer));
    }

    #[test]
    fn making_a_template_lays_out_the_files_and_the_source_secrets() {
        let (computer, layout, data) = record(64);
        let name = make(data.path(), &computer, &layout, "abcd", "cu", false, &none)
            .unwrap()
            .unwrap();
        assert_eq!(name, "25G83-abcd-cu");
        let listed = list(data.path());
        assert_eq!(listed.len(), 1);
        let made = &listed[0];
        assert_eq!(made.meta.disk_gib, 64);
        assert_eq!(made.meta.source_computer_id, computer.id);
        assert_eq!(made.meta.macos_version, "26.6.2");
        assert_eq!(fs::read(made.disk()).unwrap(), vec![7u8; 4096]);
        assert_eq!(fs::read(made.auxiliary_storage()).unwrap(), b"aux");
        assert_eq!(fs::read(made.hardware_model()).unwrap(), b"model");
        // The identity belongs to the source alone.
        assert!(!made.dir.join("machine-identifier.bin").exists());
        let access = made.access_dir();
        assert_eq!(
            fs::metadata(&access).unwrap().permissions().mode() & 0o777,
            0o700
        );
        for file in ACCESS_FILES {
            assert_eq!(
                fs::read_to_string(access.join(file)).unwrap(),
                format!("secret {file}")
            );
            assert_eq!(
                fs::metadata(access.join(file))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        assert!(!access.join("known_hosts").exists());
        // Making it again changes nothing.
        assert_eq!(
            make(data.path(), &computer, &layout, "abcd", "cu", false, &none).unwrap(),
            None
        );
        let json: serde_json::Value =
            serde_json::from_slice(&fs::read(made.dir.join(META)).unwrap()).unwrap();
        assert_eq!(json["diskGiB"], 64);
        assert_eq!(json["setupVersion"], "abcd");
    }

    /// A finished copy of the template `source`, brought up to date with the current
    /// computer use.
    fn updated_copy(source: &str, disk_gib: u64) -> Record {
        let approval = crate::computer_use::Approval::Ask;
        let (mut copy, _, _data) = record(disk_gib);
        copy.template = Some(source.into());
        copy.pristine = false;
        copy.setup_version = Some("abcd".into());
        copy.computer_use_approval = Some(approval);
        copy.computer_use_version = Some(computer_use_version_for(approval));
        copy
    }

    #[test]
    fn an_updated_copy_of_a_stale_template_may_replace_it() {
        let (stale_source, layout, data) = record(64);
        let name = make(
            data.path(),
            &stale_source,
            &layout,
            "abcd",
            "old",
            false,
            &none,
        )
        .unwrap()
        .unwrap();
        assert_eq!(name, "25G83-abcd-old");
        let copy = updated_copy(&name, 64);
        assert!(refreshes(data.path(), &copy));
        // A copy that was not updated, one with a larger disk (it would raise the size
        // every later copy needs), one of another build or base, and a fork do not.
        let mut not_updated = copy.clone();
        not_updated.computer_use_version = Some("old".into());
        assert!(!refreshes(data.path(), &not_updated));
        assert!(!refreshes(data.path(), &updated_copy(&name, 128)));
        let mut other_build = copy.clone();
        other_build.restore_image = Some(RestoreImageInfo {
            version: "27.0".into(),
            build: "26A1".into(),
        });
        assert!(!refreshes(data.path(), &other_build));
        let mut other_base = copy.clone();
        other_base.setup_version = Some("changed".into());
        assert!(!refreshes(data.path(), &other_base));
        let mut fork = copy.clone();
        fork.inherited_access = true;
        assert!(!refreshes(data.path(), &fork));
        let mut unfinished = copy.clone();
        unfinished.setup.clipboard = false;
        assert!(!refreshes(data.path(), &unfinished));
        let mut gone = copy.clone();
        gone.template = Some("25G83-abcd-missing".into());
        assert!(!refreshes(data.path(), &gone));
        let mut no_template = copy.clone();
        no_template.template = None;
        assert!(!refreshes(data.path(), &no_template));
        // The same computer use as the template's is nothing to refresh.
        let mut same = copy;
        same.computer_use_version = Some("old".into());
        assert!(!refreshes(data.path(), &same));
    }

    #[test]
    fn a_refreshed_template_is_saved_from_the_copy_and_prunes_the_stale_one() {
        let (source, layout, data) = record(64);
        make(data.path(), &source, &layout, "abcd", "old", false, &none).unwrap();
        let mut copy = updated_copy("25G83-abcd-old", 64);
        copy.id = source.id.clone();
        // Not eligible as a fresh install (it is a copy), only as a refresh.
        assert_eq!(
            make(data.path(), &copy, &layout, "abcd", "new", false, &none).unwrap(),
            None
        );
        let made = make(data.path(), &copy, &layout, "abcd", "new", true, &none)
            .unwrap()
            .unwrap();
        assert_eq!(made, "25G83-abcd-new");
        let names: Vec<_> = list(data.path()).into_iter().map(|t| t.name).collect();
        assert_eq!(names, ["25G83-abcd-new"]);
        let listed = list(data.path());
        assert_eq!(listed[0].meta.computer_use_version, "new");
        // The stale template is kept while a copy is still made from it.
        let (source, layout, data) = record(64);
        make(data.path(), &source, &layout, "abcd", "old", false, &none).unwrap();
        let lease = lease_named(data.path(), "25G83-abcd-old").unwrap();
        let mut copy = updated_copy("25G83-abcd-old", 64);
        copy.id = source.id.clone();
        make(data.path(), &copy, &layout, "abcd", "new", true, &none).unwrap();
        assert_eq!(list(data.path()).len(), 2);
        drop(lease);
        prune_stale(data.path(), &none).unwrap();
        assert_eq!(list(data.path()).len(), 1);
    }

    #[test]
    fn a_computer_that_is_not_pristine_makes_no_template() {
        let (mut computer, layout, data) = record(64);
        computer.pristine = false;
        assert_eq!(
            make(data.path(), &computer, &layout, "abcd", "cu", false, &none).unwrap(),
            None
        );
        assert!(list(data.path()).is_empty());
    }

    #[test]
    fn a_new_template_replaces_older_ones_unless_they_are_in_use() {
        let (computer, layout, data) = record(64);
        make(data.path(), &computer, &layout, "one", "cu", false, &none).unwrap();
        let held = lease_named(data.path(), "25G83-one-cu").unwrap();
        make(data.path(), &computer, &layout, "two", "cu", false, &none).unwrap();
        let names: Vec<_> = list(data.path()).into_iter().map(|t| t.name).collect();
        assert_eq!(
            names,
            ["25G83-two-cu", "25G83-one-cu"],
            "a leased template stays"
        );
        drop(held);
        // A template an unfinished copy depends on also stays.
        let protect = || vec!["25G83-one-cu".to_string()];
        make(
            data.path(),
            &computer,
            &layout,
            "three",
            "cu",
            false,
            &protect,
        )
        .unwrap();
        let names: Vec<_> = list(data.path()).into_iter().map(|t| t.name).collect();
        assert_eq!(names, ["25G83-three-cu", "25G83-one-cu"]);
        make(data.path(), &computer, &layout, "four", "cu", false, &none).unwrap();
        let names: Vec<_> = list(data.path()).into_iter().map(|t| t.name).collect();
        assert_eq!(names, ["25G83-four-cu"]);
    }

    #[test]
    fn unfinished_templates_are_not_listed_and_are_cleaned_up() {
        let (computer, layout, data) = record(64);
        let stale = root(data.path()).join(format!("25G83-x{PARTIAL}1"));
        fs::create_dir_all(&stale).unwrap();
        fs::write(
            stale.join(META),
            serde_json::to_vec(&meta("25G83", "x", "t")).unwrap(),
        )
        .unwrap();
        assert!(list(data.path()).is_empty());
        make(data.path(), &computer, &layout, "abcd", "cu", false, &none).unwrap();
        assert!(!stale.exists());
        assert_eq!(list(data.path()).len(), 1);
    }

    #[test]
    fn removing_refuses_while_a_copy_depends_on_the_template() {
        let (computer, layout, data) = record(64);
        make(data.path(), &computer, &layout, "abcd", "cu", false, &none).unwrap();
        let lease = lease_matching(data.path(), Some("25G83"), "abcd", "cu").unwrap();
        assert!(remove_all(data.path(), &none)
            .unwrap_err()
            .contains("being copied"));
        drop(lease);
        let protect = || vec!["25G83-abcd-cu".to_string()];
        assert!(remove_all(data.path(), &protect)
            .unwrap_err()
            .contains("still being set up"));
        assert_eq!(list(data.path()).len(), 1);
        remove_all(data.path(), &none).unwrap();
        assert!(list(data.path()).is_empty());
        // Nothing to remove is fine.
        remove_all(data.path(), &none).unwrap();
    }

    #[test]
    fn a_copy_has_the_template_files_a_new_identity_and_none_of_its_secrets() {
        let (computer, layout, data) = record(64);
        make(data.path(), &computer, &layout, "abcd", "cu", false, &none).unwrap();
        let lease = lease_matching(data.path(), Some("25G83"), "abcd", "cu").unwrap();
        let copy = Layout::new(data.path(), "copy-id");
        clone_into(&lease.template, &copy, b"new-identifier", 64).unwrap();
        assert_eq!(fs::read(copy.disk()).unwrap(), vec![7u8; 4096]);
        assert_eq!(fs::read(copy.auxiliary_storage()).unwrap(), b"aux");
        assert_eq!(fs::read(copy.hardware_model()).unwrap(), b"model");
        assert_eq!(
            fs::read(copy.machine_identifier()).unwrap(),
            b"new-identifier"
        );
        assert_ne!(
            fs::read(copy.machine_identifier()).unwrap(),
            fs::read(layout.machine_identifier()).unwrap()
        );
        let mut files: Vec<_> = fs::read_dir(&copy.dir)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().into_string().unwrap())
            .collect();
        files.sort();
        assert_eq!(
            files,
            [
                "auxiliary-storage.img",
                "disk.img",
                "hardware-model.bin",
                "machine-identifier.bin"
            ]
        );
        // Writing to the copy leaves the template alone.
        fs::write(copy.disk(), b"changed").unwrap();
        assert_eq!(fs::read(lease.template.disk()).unwrap(), vec![7u8; 4096]);
    }

    #[test]
    fn a_disk_smaller_than_the_selected_template_is_refused_not_enlarged() {
        let template = template("25G83", "abcd", "2026-10-01T00:00:00Z");
        assert!(check_disk(64, &template).is_ok());
        assert!(check_disk(128, &template).is_ok());
        let error = check_disk(63, &template).unwrap_err();
        assert!(error.contains("needs at least 64 GiB"), "{error}");
    }

    #[test]
    fn a_template_published_while_pruning_is_not_pruned() {
        let (computer, layout, data) = record(64);
        make(data.path(), &computer, &layout, "one", "cu", false, &none).unwrap();
        make(data.path(), &computer, &layout, "two", "cu", false, &none).unwrap();
        prune_stale(data.path(), &none).unwrap();
        let names: Vec<_> = list(data.path()).into_iter().map(|t| t.name).collect();
        assert_eq!(names, ["25G83-two-cu"]);
    }

    #[test]
    fn a_larger_disk_is_grown_and_a_template_sized_disk_is_not() {
        let (computer, layout, data) = record(64);
        make(data.path(), &computer, &layout, "abcd", "cu", false, &none).unwrap();
        let lease = lease_matching(data.path(), None, "abcd", "cu").unwrap();
        let same = Layout::new(data.path(), "same");
        clone_into(&lease.template, &same, b"id", 64).unwrap();
        assert_eq!(fs::metadata(same.disk()).unwrap().len(), 4096);
        let larger = Layout::new(data.path(), "larger");
        clone_into(&lease.template, &larger, b"id", 128).unwrap();
        assert_eq!(fs::metadata(larger.disk()).unwrap().len(), 128 * GIB);
        assert_eq!(fs::metadata(lease.template.disk()).unwrap().len(), 4096);
    }

    #[test]
    fn a_copy_needs_free_space_beyond_the_reserve() {
        assert!(enough_space(None, COPY_ESTIMATE).is_ok());
        assert!(enough_space(Some(COPY_ESTIMATE + FREE_RESERVE), COPY_ESTIMATE).is_ok());
        let error =
            enough_space(Some(COPY_ESTIMATE + FREE_RESERVE - 1), COPY_ESTIMATE).unwrap_err();
        assert!(error.contains("low on disk space"), "{error}");
    }

    #[test]
    fn copies_in_progress_count_against_the_free_space() {
        let root = Path::new("/");
        let free = Some(2 * COPY_ESTIMATE + FREE_RESERVE + 1);
        let first = reserve_in(root, COPY_ESTIMATE, |_| free).unwrap();
        let second = reserve_in(root, COPY_ESTIMATE, |_| free).unwrap();
        let error = reserve_in(root, COPY_ESTIMATE, |_| free).err().unwrap();
        assert!(error.contains("low on disk space"), "{error}");
        drop(first);
        let third = reserve_in(root, COPY_ESTIMATE, |_| free).unwrap();
        drop((second, third));
        assert_eq!(*PENDING.lock().unwrap(), 0);
    }
}
