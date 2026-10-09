use super::*;
use std::os::fd::AsRawFd;

const OWNER: &str = ".silo-configuration-owner";
/// Set once startup recovery has run (successfully or not) in this process.
static STARTUP_SETTLED: AtomicBool = AtomicBool::new(false);
/// Configuration attempts currently running in this process.
static LIVE_ATTEMPTS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Marks a configuration attempt as live for as long as it is held. A saved
/// journal blocks snapshots only while an attempt is live (or before startup
/// recovery has run), so an attempt that fails in-session no longer freezes
/// the computer list until relaunch.
#[must_use]
pub(super) struct Attempt(());
impl Drop for Attempt {
    fn drop(&mut self) {
        LIVE_ATTEMPTS.fetch_sub(1, Ordering::SeqCst);
    }
}
pub(super) fn attempt() -> Attempt {
    LIVE_ATTEMPTS.fetch_add(1, Ordering::SeqCst);
    Attempt(())
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Journal {
    version: u32,
    pub(super) previous: ComputerConfigurationRequest,
    pub(super) request: ComputerConfigurationRequest,
}

fn path(paths: &RuntimePaths) -> PathBuf {
    paths
        .metadata
        .with_file_name("configuration-operation.json")
}
fn failure(error: impl std::fmt::Display) -> RuntimeError {
    RuntimeError::Unavailable(format!("Could not recover computer configuration: {error}"))
}
pub(super) fn load(paths: &RuntimePaths) -> Result<Option<Journal>, RuntimeError> {
    let file = match File::open(path(paths)) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(failure(error)),
    };
    let journal: Journal = serde_json::from_reader(file.take(MAX_OUTPUT_BYTES)).map_err(failure)?;
    if journal.version != 1 {
        return Err(failure("Unsupported saved operation; it was preserved."));
    }
    // First-run metadata is legitimately empty; submitted configurations are not.
    if !journal.previous.computers.is_empty() || journal.previous.schema_version != 1 {
        validate_request(&journal.previous)?;
    }
    validate_request(&journal.request)?;
    Ok(Some(journal))
}

/// Include computers created before their metadata commit without replaying setup on Quit.
pub(super) fn shutdown_computers(
    paths: &RuntimePaths,
) -> Result<Vec<ComputerConfiguration>, RuntimeError> {
    let Some(journal) = load(paths)? else {
        return Ok(Vec::new());
    };
    let mut computers = journal.previous.computers;
    for configuration in journal.request.computers {
        if !computers.iter().any(|old| old.id() == configuration.id()) {
            computers.push(configuration);
        }
    }
    Ok(computers)
}

/// The target configuration of an interrupted attempt, if one is pending. Used by the
/// retry command to resume that attempt against current state without the UI resending
/// the whole list.
pub(super) fn pending_request(
    paths: &RuntimePaths,
) -> Result<Option<ComputerConfigurationRequest>, RuntimeError> {
    Ok(load(paths)?.map(|journal| journal.request))
}

pub(super) fn begin(
    paths: &RuntimePaths,
    request: &ComputerConfigurationRequest,
) -> Result<(), RuntimeError> {
    if let Some(saved) = load(paths)? {
        if saved.request == *request {
            return Ok(());
        }
        return Err(failure("An interrupted configuration is pending. Relaunch Silo to resume it before making another change."));
    }
    let journal = Journal {
        version: 1,
        previous: read_metadata(&paths.metadata)?,
        request: request.clone(),
    };
    write(paths, &journal)
}

fn write(paths: &RuntimePaths, journal: &Journal) -> Result<(), RuntimeError> {
    let parent = paths
        .metadata
        .parent()
        .ok_or_else(|| failure("Missing storage directory."))?;
    fs::create_dir_all(parent).map_err(failure)?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(failure)?;
    serde_json::to_writer(&mut file, &journal).map_err(failure)?;
    file.as_file().sync_all().map_err(failure)?;
    file.persist(path(paths)).map_err(failure)?;
    File::open(parent)
        .and_then(|file| file.sync_all())
        .map_err(failure)
}

// Reserve new storage before formatting. The marker permits cleanup only of
// files created for this exact, not-yet-committed computer ID.
pub(super) fn claim(
    paths: &RuntimePaths,
    configuration: &ComputerConfiguration,
) -> Result<(), RuntimeError> {
    use std::os::unix::fs::PermissionsExt;
    let Some(journal) = load(paths)? else {
        return Ok(());
    };
    if journal
        .previous
        .computers
        .iter()
        .any(|old| old.id() == configuration.id())
    {
        return Ok(());
    }
    let folder = paths.volumes.join(configuration.name());
    fs::create_dir_all(&paths.volumes).map_err(failure)?;
    if folder.exists() {
        // Reuse only an empty directory, including leftovers from older builds.
        fs::remove_dir(&folder).map_err(|_| {
            failure("New computer storage is already occupied. No existing files were changed.")
        })?;
    }
    let stage = tempfile::Builder::new()
        .prefix(".configuration-claim-")
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(&paths.volumes)
        .map_err(failure)?;
    let mut marker = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(stage.path().join(OWNER))
        .map_err(failure)?;
    marker
        .write_all(configuration.id().as_bytes())
        .and_then(|_| marker.sync_all())
        .map_err(failure)?;
    File::open(stage.path())
        .and_then(|file| file.sync_all())
        .map_err(failure)?;
    fs::rename(stage.path(), &folder).map_err(failure)?;
    File::open(&paths.volumes)
        .and_then(|file| file.sync_all())
        .map_err(failure)
}

pub(super) fn finish(paths: &RuntimePaths) -> Result<(), RuntimeError> {
    if let Some(journal) = load(paths)? {
        for configuration in &journal.request.computers {
            let marker = paths.volumes.join(configuration.name()).join(OWNER);
            if fs::read_to_string(&marker).ok().as_deref() == Some(configuration.id()) {
                fs::remove_file(&marker).map_err(failure)?;
                File::open(marker.parent().unwrap())
                    .and_then(|file| file.sync_all())
                    .map_err(failure)?;
            }
        }
        fs::remove_file(path(paths)).map_err(failure)?;
        File::open(paths.metadata.parent().unwrap())
            .and_then(|file| file.sync_all())
            .map_err(failure)?;
    }
    Ok(())
}

/// A parent-held command lock releases the shared flock even when a concurrent
/// fork temporarily inherited its descriptor before exec closed it.
pub(crate) struct CommandLock {
    file: File,
    inherited_by_child: bool,
    release_on_drop: bool,
}

impl CommandLock {
    /// A shutdown worker shares the parent's flock. Only the parent unlocks after
    /// all workers have joined; each runtime child still inherits its own descriptor.
    pub(crate) fn duplicate_for_shutdown(&self) -> Result<Self, RuntimeError> {
        Ok(Self {
            file: self.file.try_clone().map_err(failure)?,
            inherited_by_child: false,
            release_on_drop: false,
        })
    }

    /// Keep the flock until the deliberately inheriting runtime child exits.
    /// Call only after a child with the lock's close-on-exec flag cleared spawned.
    pub(crate) fn mark_inherited_by_child(&mut self) {
        self.inherited_by_child = true;
    }

    /// Once the child is reaped, the parent can release any copies temporarily
    /// inherited by unrelated forks while that child was running.
    pub(crate) fn mark_child_exited(&mut self) {
        self.inherited_by_child = false;
    }
}

impl AsRawFd for CommandLock {
    fn as_raw_fd(&self) -> std::os::fd::RawFd {
        self.file.as_raw_fd()
    }
}

impl Drop for CommandLock {
    fn drop(&mut self) {
        if self.release_on_drop && !self.inherited_by_child {
            // SAFETY: this descriptor remains owned by self until after Drop.
            // LOCK_UN also releases copies inherited by unrelated forks.
            unsafe {
                libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
            }
        }
    }
}

pub(crate) fn command_lock(
    paths: &RuntimePaths,
    timeout: Duration,
) -> Result<CommandLock, RuntimeError> {
    prepare_runtime_home(&paths.home, paths.storage_home.as_deref())?;
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(paths.home.join(".silo-configuration-worker.lock"))
        .map_err(failure)?;
    let started = Instant::now();
    loop {
        // SAFETY: the open file owns this descriptor until the lock is dropped.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return Ok(CommandLock {
                file,
                inherited_by_child: false,
                release_on_drop: true,
            });
        }
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::WouldBlock {
            return Err(failure(error));
        }
        if started.elapsed() >= timeout {
            return Err(failure(
                "The previous computer command is still finishing. Retry after it exits.",
            ));
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn reconcile(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    journal: &Journal,
) -> Result<(), RuntimeError> {
    let mut current = read_metadata(&paths.metadata)?;
    for configuration in &current.computers {
        if !journal.previous.computers.contains(configuration)
            && !journal.request.computers.contains(configuration)
        {
            return Err(failure(
                "Computer settings changed since the interruption. Saved data was preserved.",
            ));
        }
    }
    let listed = list_managed(runner, paths)?;
    for configuration in journal.request.computers.iter().filter(|configuration| {
        !journal
            .previous
            .computers
            .iter()
            .any(|old| old.id() == configuration.id())
    }) {
        if listed
            .iter()
            .any(|entry| entry.name == configuration.name())
        {
            let inspected = inspect_computer(runner, paths, configuration.name())?;
            ensure_managed(&inspected)?;
            if inspected
                .config
                .pointer("/labels/silo.machine-id")
                .and_then(Value::as_str)
                != Some(configuration.id())
            {
                return Err(failure(
                    "A different computer now owns the requested name. Its data was preserved.",
                ));
            }
            verify_computer_configuration(runner, paths, configuration)?;
            if !current
                .computers
                .iter()
                .any(|entry| entry.id() == configuration.id())
            {
                verify_guest_tools(runner, paths, configuration.name())?;
                if let Some(desktop) = crate::desktop::configuration(configuration) {
                    crate::desktop::configure_with(
                        runner,
                        paths,
                        configuration.name(),
                        None,
                        desktop,
                    )?;
                    let restored = inspect_computer(runner, paths, configuration.name())?;
                    ensure_managed(&restored)?;
                    if !matches!(restored.status.as_str(), "Created" | "Stopped") {
                        return Err(failure(
                            "Desktop installation did not restore the new computer's stopped state.",
                        ));
                    }
                }
                current.computers.push(configuration.clone());
            }
        } else {
            if current
                .computers
                .iter()
                .any(|entry| entry.id() == configuration.id())
            {
                return Err(failure(
                    "A saved computer is missing from the runtime. Its disk files were preserved.",
                ));
            }
            let folder = paths.volumes.join(configuration.name());
            if folder.exists() {
                if fs::read_to_string(folder.join(OWNER)).ok().as_deref()
                    != Some(configuration.id())
                {
                    if fs::remove_dir(&folder).is_ok() {
                        continue;
                    }
                    return Err(failure("Incomplete storage ownership could not be verified. No files were removed."));
                }
                fs::remove_dir_all(folder).map_err(failure)?;
            }
        }
    }
    for configuration in journal.previous.computers.iter().filter(|configuration| {
        !journal
            .request
            .computers
            .iter()
            .any(|next| next.id() == configuration.id())
    }) {
        if !listed
            .iter()
            .any(|entry| entry.name == configuration.name())
        {
            crate::network::computer_removed(paths, configuration.name()).map_err(failure)?;
            crate::secrets::computer_removed(configuration.name()).map_err(failure)?;
            remove_computer_volumes(paths, configuration)?;
            lifecycle_recovery::forget_removed(paths, configuration)?;
            current
                .computers
                .retain(|entry| entry.id() != configuration.id());
        } else {
            let inspected = inspect_computer(runner, paths, configuration.name())?;
            if inspected
                .config
                .pointer("/labels/silo.machine-id")
                .and_then(Value::as_str)
                != Some(configuration.id())
            {
                return Err(failure(
                    "The computer selected for deletion changed ownership. It was preserved.",
                ));
            }
        }
    }
    if !current.computers.is_empty() || paths.metadata.exists() {
        write_metadata(&paths.metadata, &current)?;
    }
    // An interrupted removal keeps its checkpoint history until exact native members
    // have entered the cleanup journal. The updated inventory releases its own pins.
    for configuration in journal.previous.computers.iter().filter(|configuration| {
        !journal
            .request
            .computers
            .iter()
            .any(|next| next.id() == configuration.id())
            && !current
                .computers
                .iter()
                .any(|next| next.id() == configuration.id())
    }) {
        checkpoints::remove_deleted_snapshots(
            runner,
            paths,
            configuration.id(),
            configuration.name(),
        )?;
        checkpoints::forget_removed(paths, configuration.id())?;
    }
    Ok(())
}

fn verify_committed_edits(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    journal: &Journal,
    requested: &ComputerConfigurationRequest,
) -> Result<(), RuntimeError> {
    // Metadata is committed before the final verification. An existing computer whose
    // edit reached that checkpoint still needs verification after relaunch.
    let current = read_metadata(&paths.metadata)?;
    for configuration in journal.request.computers.iter().filter(|configuration| {
        journal
            .previous
            .computers
            .iter()
            .any(|old| old.id() == configuration.id() && old != *configuration)
            && current.computers.contains(configuration)
            && requested.computers.contains(configuration)
    }) {
        let inspected = inspect_computer(runner, paths, configuration.name())?;
        ensure_managed(&inspected)?;
        if inspected
            .config
            .pointer("/labels/silo.machine-id")
            .and_then(Value::as_str)
            != Some(configuration.id())
        {
            return Err(failure(
                "The updated computer changed ownership. Its data was preserved.",
            ));
        }
        verify_computer_configuration(runner, paths, configuration)?;
    }
    Ok(())
}

pub(super) fn recover_at_paths(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    resources: &DeviceResources,
    progress: &dyn Fn(&str, &str, u8),
) -> Result<(), RuntimeError> {
    debug_assert!(
        operation_gate::held(),
        "configuration recovery requires the device operation gate"
    );
    let Some(journal) = load(paths)? else {
        return Ok(());
    };
    // Drain a surviving child before inspecting state. The caller holds the
    // operation gate (device scope), which serializes the application-level
    // recovery transaction against all other computer-changing work. This comes first: the
    // normalization below asks the runtime which computers already exist, and a creation
    // that is still running would make that answer wrong.
    drop(command_lock(paths, MUTATION_TIMEOUT)?);
    let journal = normalize_desktop_intent(runner, paths, journal)?;
    reconcile(runner, paths, &journal)?;
    verify_committed_edits(runner, paths, &journal, &journal.request)?;
    apply_whole_configuration_with_progress(
        runner,
        paths,
        resources,
        journal.request,
        None,
        progress,
    )?;
    finish(paths)
}

/// A journal written by an older Silo has no desktop defaults. Apply the ones an explicit
/// retry would, and re-save the intent atomically, so replay (which defaults the same way
/// before `begin` compares with the journal) matches what is saved.
///
/// The defaults describe a computer that is still to be created from the current image. A
/// creation that got as far as the runtime before the interruption made its computer from
/// the image of its time (a 0.x journal means the v3 image, with no built-in desktop and no
/// computer-use mount), and recovery adopts that computer as it is: its journaled settings
/// are kept, so it is never promoted to built-in. The runtime is asked only when a default
/// would change something.
pub(super) fn normalize_desktop_intent(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    mut journal: Journal,
) -> Result<Journal, RuntimeError> {
    // Computers already committed count as existing: their desktop settings are kept.
    let mut known = journal.previous.clone();
    for configuration in read_metadata(&paths.metadata)?.computers {
        if !known
            .computers
            .iter()
            .any(|old| old.id() == configuration.id())
        {
            known.computers.push(configuration);
        }
    }
    let mut request = journal.request.clone();
    apply_desktop_defaults(&known, &mut request);
    if request == journal.request {
        return Ok(journal);
    }
    let created: Vec<String> = list_managed(runner, paths)?
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    for (defaulted, original) in request.computers.iter_mut().zip(&journal.request.computers) {
        if defaulted != original && created.iter().any(|name| name == original.name()) {
            *defaulted = original.clone();
        }
    }
    if request != journal.request {
        journal.request = request;
        write(paths, &journal)?;
    }
    Ok(journal)
}

pub(super) fn prepare_retry(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    request: Option<&ComputerConfigurationRequest>,
) -> Result<(), RuntimeError> {
    let _attempt = attempt();
    if let Some(journal) = load(paths)? {
        drop(command_lock(paths, MUTATION_TIMEOUT)?);
        reconcile(runner, paths, &journal)?;
        verify_committed_edits(runner, paths, &journal, request.unwrap_or(&journal.request))?;
        if let Some(request) = request.filter(|request| **request != journal.request) {
            validate_request(request)?;
            let previous = read_metadata(&paths.metadata)?;
            for configuration in &request.computers {
                if let Some(old) = previous
                    .computers
                    .iter()
                    .find(|old| old.id() == configuration.id())
                {
                    validate_computer_update(old, configuration)?;
                }
            }
            for configuration in &previous.computers {
                let marker = paths.volumes.join(configuration.name()).join(OWNER);
                if fs::read_to_string(&marker).ok().as_deref() == Some(configuration.id()) {
                    fs::remove_file(&marker).map_err(failure)?;
                    File::open(marker.parent().unwrap())
                        .and_then(|file| file.sync_all())
                        .map_err(failure)?;
                }
            }
            // Reconciliation has either adopted completed additions or removed
            // owned partial files. Atomically replace intent without losing the
            // committed computers that the revised request may now edit/remove.
            write(
                paths,
                &Journal {
                    version: 1,
                    previous,
                    request: request.clone(),
                },
            )?;
        }
    }
    Ok(())
}

pub(super) fn pending(paths: &RuntimePaths) -> Result<bool, String> {
    let idle = STARTUP_SETTLED.load(Ordering::SeqCst) && LIVE_ATTEMPTS.load(Ordering::SeqCst) == 0;
    blocks_snapshot(paths, idle)
}

fn blocks_snapshot(paths: &RuntimePaths, no_live_attempt: bool) -> Result<bool, String> {
    // Once no attempt is running (startup recovery or an in-session change
    // stopped with an error), let the normal snapshot verifier show the actual
    // committed state so the user can correct the request. The saved intent,
    // activity failure and error remain; this is not completion.
    if no_live_attempt {
        return Ok(false);
    }
    load(paths)
        .map(|journal| journal.is_some())
        .map_err(|e| e.to_string())
}

pub(crate) fn recover(app: &AppHandle) -> Result<(), String> {
    let result = {
        let _attempt = attempt();
        STARTUP_SETTLED.store(true, Ordering::SeqCst);
        recover_inner(app)
    };
    let _ = app.emit("silo://application-state-changed", ());
    result
}

fn recover_inner(app: &AppHandle) -> Result<(), String> {
    let paths = runtime_paths(app)?;
    if load(&paths).map_err(|e| e.to_string())?.is_none() {
        return Ok(());
    }
    let _guard = OPERATIONS
        .device("Recovering computer configuration")
        .map_err(|_| "Computer configuration lock is unavailable.")?;
    // Waiting for the gate let other work replace or finish the journal, so the copy to
    // reserve from is read now. Replaying must not add a name a macOS computer took
    // since the interruption.
    let journal = load(&paths).map_err(|e| e.to_string())?;
    let Some(_reservation) =
        reserve_replay(journal.as_ref(), &|| crate::macos_computers::names(app))?
    else {
        return Ok(());
    };
    let request_id = uuid::Uuid::new_v4().to_string();
    let activity = Mutex::new(ActivityJournal::start(&paths, &request_id)?);
    let progress = |step: &str, name: &str, fraction: u8| {
        let event = activity
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .append(computer_progress(&request_id, step, name, fraction));
        let _ = app.emit_to("main", "silo://computer-configuration-progress", event);
    };
    progress("setup-started", "", 0);
    let result = device_resources()
        .and_then(|resources| recover_at_paths(&ProcessRunner, &paths, &resources, &progress));
    progress(
        if result.is_ok() {
            "setup-completed"
        } else {
            "setup-interrupted"
        },
        "",
        0,
    );
    let _ = app.emit("silo://application-state-changed", ());
    result.map_err(|e| e.to_string())
}

/// Reserves the names replaying `journal` would add. `None` when no journal is pending.
fn reserve_replay(
    journal: Option<&Journal>,
    macos_names: &dyn Fn() -> Vec<String>,
) -> Result<Option<crate::computer_names::Reservation>, String> {
    let Some(journal) = journal else {
        return Ok(None);
    };
    crate::computer_names::reserve(
        &super::new_names(&journal.previous.computers, &journal.request.computers),
        macos_names,
    )
    .map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn computer(id: &str, name: &str) -> ComputerConfiguration {
        ComputerConfiguration {
            id: id.into(),
            name: name.into(),
            cpus: 2,
            max_cpus: 4,
            memory_gib: 4,
            max_memory_gib: 8,
            workspace_storage_gib: 10,
            runtime_storage_gib: 10,
            desktop: None,
        }
    }

    fn journal(previous: &[ComputerConfiguration], request: &[ComputerConfiguration]) -> Journal {
        let request_of = |computers: &[ComputerConfiguration]| ComputerConfigurationRequest {
            schema_version: 1,
            computers: computers.to_vec(),
        };
        Journal {
            version: 1,
            previous: request_of(previous),
            request: request_of(request),
        }
    }

    #[test]
    fn replay_reserves_from_the_journal_read_after_the_gate() {
        let macos = || vec!["rr-foo".to_string()];
        // The journal read before the gate added rr-foo; a change since then replaced it.
        let stale = journal(&[], &[computer("1", "rr-foo")]);
        let fresh = journal(&[], &[computer("2", "rr-bar")]);
        assert!(reserve_replay(Some(&stale), &macos).is_err());
        assert!(reserve_replay(Some(&fresh), &macos).unwrap().is_some());
        // A journal that finished meanwhile leaves nothing to replay or reserve.
        assert!(reserve_replay(None, &macos).unwrap().is_none());
    }

    #[test]
    fn a_migrated_configuration_operation_loads() {
        let migrated = crate::runtime_migration::vocabulary_tests::migrated_installation();
        let journal = load(&migrated.runtime_paths()).unwrap().unwrap();
        let names = |request: &ComputerConfigurationRequest| {
            request
                .computers
                .iter()
                .map(|c| c.name().to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(names(&journal.previous), ["dev"]);
        assert_eq!(names(&journal.request), ["dev"]);
        assert!(journal.request.computers[0].desktop.is_some());
    }

    #[test]
    fn network_mappings_are_removed_when_recovering_an_interrupted_deletion() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = super::super::tests::paths(&directory);
        let computer = ComputerConfiguration {
            id: "00000000-0000-4000-8000-000000000001".into(),
            name: "dev".into(),
            cpus: 1,
            max_cpus: 1,
            memory_gib: 2,
            max_memory_gib: 2,
            workspace_storage_gib: 10,
            runtime_storage_gib: 10,
            desktop: None,
        };
        let previous = ComputerConfigurationRequest {
            schema_version: 1,
            computers: vec![computer],
        };
        write_metadata(&paths.metadata, &previous).unwrap();
        let request = ComputerConfigurationRequest {
            schema_version: 1,
            computers: vec![],
        };
        begin(&paths, &request).unwrap();
        let network = paths.metadata.with_file_name("network.json");
        fs::write(
            &network,
            json!({"mappings":[
                {"computer":"dev","port":3000,"hostPort":null,"scheme":"http","enabled":true}
            ]})
            .to_string(),
        )
        .unwrap();
        let runner = crate::test_support::runner::ScriptedRunner::new([
            crate::test_support::runner::ExpectedCommand::ok(
                ["list", "--label", MANAGED_LABEL, "--format", "json"],
                "[]",
            ),
        ]);
        prepare_retry(&runner, &paths, None).unwrap();
        runner.assert_finished();
        assert!(read_metadata(&paths.metadata).unwrap().computers.is_empty());
        let saved: Value = serde_json::from_slice(&fs::read(network).unwrap()).unwrap();
        assert!(saved["mappings"].as_array().unwrap().is_empty());
    }

    #[test]
    fn claimed_computer_directory_is_private() {
        let _test_state = crate::test_support::global_state();
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let paths = super::super::tests::paths(&directory);
        let configuration = super::super::tests::computer();
        let request = super::super::tests::request(vec![configuration.clone()]);
        begin(&paths, &request).unwrap();
        claim(&paths, &configuration).unwrap();
        let folder = paths.volumes.join(configuration.name());
        assert_eq!(
            fs::metadata(&folder).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::read_to_string(folder.join(OWNER)).unwrap(),
            configuration.id()
        );
    }

    #[cfg(unix)]
    #[test]
    fn ordinary_command_lock_release_survives_an_unrelated_fork() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = super::super::tests::paths(&directory);
        for completed_child in [false, true] {
            let mut lock = command_lock(&paths, Duration::ZERO).unwrap();
            if completed_child {
                lock.mark_inherited_by_child();
                lock.mark_child_exited();
            }
            let mut ready = [0; 2];
            let mut release = [0; 2];
            // SAFETY: only async-signal-safe libc calls run in the child. The pipes
            // keep it alive until the parent has tested the inherited descriptor.
            unsafe {
                assert_eq!(libc::pipe(ready.as_mut_ptr()), 0);
                assert_eq!(libc::pipe(release.as_mut_ptr()), 0);
                let child = libc::fork();
                assert!(child >= 0);
                if child == 0 {
                    libc::close(ready[0]);
                    libc::close(release[1]);
                    let marker = [1u8];
                    libc::write(ready[1], marker.as_ptr().cast(), 1);
                    let mut finish = [0u8];
                    libc::read(release[0], finish.as_mut_ptr().cast(), 1);
                    libc::_exit(0);
                }
                libc::close(ready[1]);
                libc::close(release[0]);
                let mut marker = [0u8];
                assert_eq!(libc::read(ready[0], marker.as_mut_ptr().cast(), 1), 1);
                drop(lock);
                let reacquired = command_lock(&paths, Duration::ZERO).is_ok();
                libc::write(release[1], marker.as_ptr().cast(), 1);
                assert_eq!(libc::waitpid(child, std::ptr::null_mut(), 0), child);
                libc::close(ready[0]);
                libc::close(release[1]);
                assert!(
                    reacquired,
                    "an unrelated fork kept the released command lock"
                );
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn deliberately_inherited_command_lock_survives_parent_release() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = super::super::tests::paths(&directory);
        let mut lock = command_lock(&paths, Duration::ZERO).unwrap();
        let mut ready = [0; 2];
        let mut release = [0; 2];
        // SAFETY: only async-signal-safe libc calls run in the child.
        unsafe {
            assert_eq!(libc::pipe(ready.as_mut_ptr()), 0);
            assert_eq!(libc::pipe(release.as_mut_ptr()), 0);
            let child = libc::fork();
            assert!(child >= 0);
            if child == 0 {
                libc::close(ready[0]);
                libc::close(release[1]);
                let marker = [1u8];
                libc::write(ready[1], marker.as_ptr().cast(), 1);
                let mut finish = [0u8];
                libc::read(release[0], finish.as_mut_ptr().cast(), 1);
                libc::_exit(0);
            }
            libc::close(ready[1]);
            libc::close(release[0]);
            let mut marker = [0u8];
            assert_eq!(libc::read(ready[0], marker.as_mut_ptr().cast(), 1), 1);
            lock.mark_inherited_by_child();
            drop(lock);
            let held = command_lock(&paths, Duration::ZERO).is_err();
            libc::write(release[1], marker.as_ptr().cast(), 1);
            assert_eq!(libc::waitpid(child, std::ptr::null_mut(), 0), child);
            libc::close(ready[0]);
            libc::close(release[1]);
            assert!(held, "the surviving child lost the command lock");
            drop(command_lock(&paths, Duration::ZERO).unwrap());
        }
    }

    #[test]
    fn recovery_checks_the_tools_of_an_interrupted_creation_once() {
        let _test_state = crate::test_support::global_state();
        struct InterruptedRuntime {
            inspected: Value,
            calls: Mutex<Vec<Vec<String>>>,
        }
        impl RuntimeRunner for InterruptedRuntime {
            fn run(
                &self,
                _paths: &RuntimePaths,
                args: &[String],
                _timeout: Duration,
            ) -> Result<CommandOutput, RuntimeError> {
                self.calls.lock().unwrap().push(args.to_vec());
                let value = match args[0].as_str() {
                    "list" => json!([{"name":"dev"}]),
                    "inspect" => self.inspected.clone(),
                    "exec" => Value::Null,
                    other => panic!("Unexpected recovery operation: {other}"),
                };
                Ok(CommandOutput {
                    stdout: value.to_string(),
                    stderr: String::new(),
                })
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let paths = RuntimePaths {
            executable: root.join("msb"),
            library: root.join("library"),
            home: root.join("home"),
            storage_home: None,
            guest_image: root.join("image"),
            metadata: root.join("computers.json"),
            volumes: root.join("volumes"),
        };
        let configuration = ComputerConfiguration {
            id: uuid::Uuid::new_v4().to_string(),
            name: "dev".into(),
            cpus: 1,
            max_cpus: 2,
            memory_gib: 4,
            max_memory_gib: 8,
            workspace_storage_gib: 10,
            runtime_storage_gib: 10,
            desktop: None,
        };
        let request = ComputerConfigurationRequest {
            schema_version: 1,
            computers: vec![configuration.clone()],
        };
        begin(&paths, &request).unwrap();
        claim(&paths, &configuration).unwrap();
        let inspected = json!({"name":"dev","status":"Stopped","config":{
            "image":{"Oci":{"root_disk":{"kind":"managed","size_mib":10240}}},
            "resources":{"cpus":1,"max_cpus":2,"memory_mib":4096,"max_memory_mib":8192},
            "labels":{"silo.managed":"true","silo.machine-id":configuration.id()},
            "mounts":[{"type":"Owned","guest":"/workspace","storage":{"kind":"disk","capacity_mib":10240}}]
        }});
        let runner = InterruptedRuntime {
            inspected,
            calls: Mutex::new(Vec::new()),
        };
        prepare_retry(&runner, &paths, None).unwrap();
        assert_eq!(read_metadata(&paths.metadata).unwrap(), request);
        let calls = runner.calls.lock().unwrap();
        let commands: Vec<_> = calls.iter().filter(|args| args[0] == "exec").collect();
        assert_eq!(commands.len(), 1);
        assert!(commands[0]
            .windows(2)
            .any(|pair| pair == ["--user", "root"]));
        let script = commands[0].last().unwrap();
        // The account is set up by the boot of this exec, not by the script.
        assert_eq!(script, include_str!("../../guest/verify-tools.sh"));
        drop(calls);
        // Metadata adoption makes a subsequent retry read-only.
        runner.calls.lock().unwrap().clear();
        prepare_retry(&runner, &paths, None).unwrap();
        assert!(!runner
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|args| args[0] == "exec"));
    }

    #[test]
    fn failed_recovery_unblocks_verified_current_state_without_discarding_intent() {
        let _test_state = crate::test_support::global_state();
        struct EmptyRuntime;
        impl RuntimeRunner for EmptyRuntime {
            fn run(
                &self,
                _paths: &RuntimePaths,
                args: &[String],
                _timeout: Duration,
            ) -> Result<CommandOutput, RuntimeError> {
                assert_eq!(args[0], "list");
                Ok(CommandOutput {
                    stdout: "[]".into(),
                    stderr: String::new(),
                })
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let paths = RuntimePaths {
            executable: root.join("msb"),
            library: root.join("library"),
            home: root.join("home"),
            storage_home: None,
            guest_image: root.join("image"),
            metadata: root.join("computers.json"),
            volumes: root.join("volumes"),
        };
        let current = ComputerConfigurationRequest {
            schema_version: 1,
            computers: Vec::new(),
        };
        write_metadata(&paths.metadata, &current).unwrap();
        let new = ComputerConfiguration {
            id: uuid::Uuid::new_v4().to_string(),
            name: "dev".into(),
            cpus: 1,
            max_cpus: 2,
            memory_gib: 4,
            max_memory_gib: 8,
            workspace_storage_gib: 10,
            runtime_storage_gib: 10,
            desktop: None,
        };
        let request = ComputerConfigurationRequest {
            schema_version: 1,
            computers: vec![new],
        };
        begin(&paths, &request).unwrap();
        assert!(blocks_snapshot(&paths, false).unwrap());
        assert!(!blocks_snapshot(&paths, true).unwrap());
        // The existing verifier still checks real runtime state, rather than
        // presenting the pending requested computer as successfully created.
        let source = read_application_state_with(&EmptyRuntime, &paths).unwrap();
        assert_eq!(source.computers.len(), 0);
        assert_eq!(read_metadata(&paths.metadata).unwrap(), current);
        assert!(path(&paths).is_file());
        let mut revised = request;
        revised.computers[0].memory_gib = 2;
        prepare_retry(&EmptyRuntime, &paths, Some(&revised)).unwrap();
        assert!(load(&paths)
            .unwrap()
            .is_some_and(|journal| journal.request == revised));
    }

    #[test]
    fn in_session_failure_stops_blocking_snapshots_but_keeps_intent() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = super::super::tests::paths(&directory);
        let new = ComputerConfiguration {
            id: uuid::Uuid::new_v4().to_string(),
            name: "dev".into(),
            cpus: 1,
            max_cpus: 2,
            memory_gib: 4,
            max_memory_gib: 8,
            workspace_storage_gib: 10,
            runtime_storage_gib: 10,
            desktop: None,
        };
        let request = ComputerConfigurationRequest {
            schema_version: 1,
            computers: vec![new],
        };
        let settled = STARTUP_SETTLED.swap(true, Ordering::SeqCst);
        let blocked_while_live = {
            let _attempt = attempt();
            begin(&paths, &request).unwrap();
            pending(&paths).unwrap()
            // The attempt fails here without `finish`.
        };
        let blocked_after_failure = pending(&paths).unwrap();
        STARTUP_SETTLED.store(settled, Ordering::SeqCst);
        assert!(blocked_while_live);
        assert!(!blocked_after_failure);
        assert!(
            path(&paths).is_file(),
            "the interrupted intent stays available for Retry"
        );
    }

    #[test]
    fn retry_resumes_the_recorded_request_and_rejects_settings_changed_since() {
        let _test_state = crate::test_support::global_state();
        struct EmptyRuntime;
        impl RuntimeRunner for EmptyRuntime {
            fn run(
                &self,
                _paths: &RuntimePaths,
                args: &[String],
                _timeout: Duration,
            ) -> Result<CommandOutput, RuntimeError> {
                assert_eq!(args[0], "list");
                Ok(CommandOutput {
                    stdout: "[]".into(),
                    stderr: String::new(),
                })
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let paths = RuntimePaths {
            executable: root.join("msb"),
            library: root.join("library"),
            home: root.join("home"),
            storage_home: None,
            guest_image: root.join("image"),
            metadata: root.join("computers.json"),
            volumes: root.join("volumes"),
        };
        let entry = |name: &str| ComputerConfiguration {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            cpus: 1,
            max_cpus: 2,
            memory_gib: 4,
            max_memory_gib: 8,
            workspace_storage_gib: 10,
            runtime_storage_gib: 10,
            desktop: None,
        };
        let (a, b, c) = (entry("a"), entry("b"), entry("c"));
        // The interrupted attempt was adding "b" to an inventory that held "a".
        let previous = ComputerConfigurationRequest {
            schema_version: 1,
            computers: vec![a.clone()],
        };
        write_metadata(&paths.metadata, &previous).unwrap();
        let request = ComputerConfigurationRequest {
            schema_version: 1,
            computers: vec![a.clone(), b],
        };
        begin(&paths, &request).unwrap();
        // The retry command reads exactly this recorded target to resume.
        assert_eq!(pending_request(&paths).unwrap(), Some(request.clone()));
        // Someone changed the inventory since the attempt: "c" is foreign to both the
        // pre-attempt state and the recorded target, so resuming is rejected.
        write_metadata(
            &paths.metadata,
            &ComputerConfigurationRequest {
                schema_version: 1,
                computers: vec![a, c],
            },
        )
        .unwrap();
        let error = prepare_retry(&EmptyRuntime, &paths, Some(&request))
            .unwrap_err()
            .to_string();
        assert!(error.contains("changed since"), "{error}");
    }
}
