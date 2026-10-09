//! Silo's durable names and lifecycle intent for upstream MicroSandbox snapshots.
//! Snapshot data and its reference graph remain owned by MicroSandbox.
use super::*;
use std::collections::{HashMap, HashSet};

pub(crate) const RESTORE_EXPECTED_DURATION: Duration = Duration::from_secs(60 * 60);

mod native;
#[cfg(test)]
mod running_retry_tests;
pub(crate) use native::{plan as native_removal_plan, Member as NativeMember};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Checkpoint {
    pub(super) id: String,
    /// Native immutable member. Older records used `id` for both identities.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    native_id: Option<String>,
    pub(super) name: String,
    pub(super) created_at: u64,
    pub(super) scope: String,
    pub(super) reason: String,
}

impl Checkpoint {
    fn native_id(&self) -> &str {
        self.native_id.as_deref().unwrap_or(&self.id)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Environment {
    key: String,
    value: String,
}

impl Environment {
    fn valid(&self) -> bool {
        !self.key.is_empty() && !self.key.contains(['=', '\0']) && !self.value.contains('\0')
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct PendingRestore {
    pub(super) checkpoint_id: String,
    pub(super) source_computer: String,
    pub(super) state: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Operation {
    pub(super) kind: String,
    pub(super) status: String,
    pub(super) stage: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RestoreJournal {
    target_checkpoint_id: String,
    recovery_checkpoint: Checkpoint,
    prior_running: bool,
    phase: String,
}

#[derive(Default, Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Record {
    version: u8,
    pub(super) checkpoints: Vec<Checkpoint>,
    /// The native MicroSandbox group containing this computer's lineage.
    /// It is absent until the computer first captures a checkpoint or backup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) snapshot_group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    inflight_checkpoint: Option<Checkpoint>,
    #[serde(default)]
    restore_attempted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    restore_attempt_id: Option<String>,
    /// The attempt's computer ran, so it may hold writes to `/workspace`: a retry keeps and
    /// starts it rather than recreating it from the checkpoint (E-06).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    restore_attempt_ran: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) desired_network_policy: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    desired_environment: Vec<Environment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    restore_journal: Option<RestoreJournal>,
    pub(super) pending_checkpoint_restore: Option<PendingRestore>,
    pub(super) checkpoint_operation: Option<Operation>,
}

/// Boot variables that older versions used to set a computer's Git and jj identity.
pub(super) const IDENTITY_ENVIRONMENT: [&str; 6] = [
    "GIT_AUTHOR_NAME",
    "GIT_AUTHOR_EMAIL",
    "GIT_COMMITTER_NAME",
    "GIT_COMMITTER_EMAIL",
    "JJ_USER",
    "JJ_EMAIL",
];

#[cfg(test)]
impl Record {
    pub(super) fn set_environment_for_test(&mut self, entries: &[(&str, &str)]) {
        self.desired_environment = entries
            .iter()
            .map(|(key, value)| Environment {
                key: (*key).into(),
                value: (*value).into(),
            })
            .collect();
    }

    pub(super) fn environment_keys_for_test(&self) -> Vec<&str> {
        self.desired_environment
            .iter()
            .map(|entry| entry.key.as_str())
            .collect()
    }
}

/// Drops the identity boot variables from a computer's recorded environment once Silo
/// has removed them from the device. A computer without a record is left untouched.
pub(super) fn forget_identity_environment(
    paths: &RuntimePaths,
    id: &str,
) -> Result<(), RuntimeError> {
    if !path(paths, id).exists() {
        return Ok(());
    }
    let mut record = load(paths, id)?;
    let before = record.desired_environment.len();
    record
        .desired_environment
        .retain(|entry| !IDENTITY_ENVIRONMENT.contains(&entry.key.as_str()));
    if record.desired_environment.len() == before {
        return Ok(());
    }
    save(paths, id, &record)
}

fn error(message: &str) -> RuntimeError {
    RuntimeError::Unavailable(message.into())
}

fn directory(paths: &RuntimePaths) -> PathBuf {
    paths.metadata.with_file_name("checkpoints")
}

fn path(paths: &RuntimePaths, id: &str) -> PathBuf {
    directory(paths).join(format!("{id}.json"))
}

pub(super) fn load(paths: &RuntimePaths, id: &str) -> Result<Record, RuntimeError> {
    uuid::Uuid::parse_str(id).map_err(|_| error("Silo could not identify this computer. Refresh its status and retry the checkpoint action."))?;
    let bytes = match fs::read(path(paths, id)) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Record::default()),
        Err(_) => return Err(error("Checkpoint history could not be read.")),
    };
    if bytes.len() as u64 > MAX_OUTPUT_BYTES {
        return Err(error("Checkpoint history is too large; it was preserved."));
    }
    let record: Record = serde_json::from_slice(&bytes)
        .map_err(|_| error("Checkpoint history is invalid; it was preserved."))?;
    if record.version != 1
        || record
            .snapshot_group
            .as_deref()
            .is_some_and(|group| !valid_snapshot_group(group))
        || record.checkpoints.iter().any(|checkpoint| {
            validate_name(&checkpoint.id).is_err()
                || checkpoint
                    .native_id
                    .as_deref()
                    .is_some_and(|native_id| !valid_native_id(native_id))
                || !matches!(checkpoint.scope.as_str(), "full" | "disk")
                || !matches!(checkpoint.reason.as_str(), "manual" | "before-restore")
        })
        || !unique_checkpoint_ids(&record.checkpoints)
        || record
            .inflight_checkpoint
            .as_ref()
            .is_some_and(|checkpoint| {
                validate_name(&checkpoint.id).is_err()
                    || checkpoint
                        .native_id
                        .as_deref()
                        .is_some_and(|native_id| !valid_native_id(native_id))
            })
        || record
            .pending_checkpoint_restore
            .as_ref()
            .is_some_and(|pending| {
                !valid_native_id(&pending.checkpoint_id)
                    || !valid_snapshot_group(&pending.source_computer)
                    || !matches!(pending.state.as_str(), "full" | "disk")
            })
        || record
            .restore_attempt_id
            .as_ref()
            .is_some_and(|id| uuid::Uuid::parse_str(id).is_err())
    {
        return Err(error("Checkpoint history is invalid; it was preserved."));
    }
    if record
        .desired_environment
        .iter()
        .any(|entry| !entry.valid())
    {
        return Err(error(
            "Checkpoint environment is invalid; it was preserved.",
        ));
    }
    Ok(record)
}

fn valid_native_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn unique_checkpoint_ids(checkpoints: &[Checkpoint]) -> bool {
    let mut ids = HashSet::with_capacity(checkpoints.len());
    checkpoints
        .iter()
        .all(|checkpoint| ids.insert(&checkpoint.id))
}

fn new_checkpoint_id(record: &Record) -> String {
    loop {
        let id = format!("c{}", &uuid::Uuid::new_v4().simple().to_string()[..31]);
        if !record
            .checkpoints
            .iter()
            .any(|checkpoint| checkpoint.id == id)
            && record
                .inflight_checkpoint
                .as_ref()
                .is_none_or(|checkpoint| checkpoint.id != id)
        {
            return id;
        }
    }
}

fn ensure_pending_runtime_absent(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    computer_name: &str,
) -> Result<(), RuntimeError> {
    let listed = runner.run(
        paths,
        &["list".into(), "--format".into(), "json".into()],
        READ_TIMEOUT,
    )?;
    let listed: Vec<ListedSandbox> = serde_json::from_str(&listed.stdout)
        .map_err(|_| error("The runtime returned an invalid computer list."))?;
    if listed.iter().any(|entry| entry.name == computer_name) {
        return Err(error(
            "A runtime computer exists for this pending restore. Start or recover it before changing checkpoint state.",
        ));
    }
    Ok(())
}

fn valid_snapshot_group(group: &str) -> bool {
    if validate_name(group).is_ok() {
        return true;
    }
    let Some(suffix) = group.strip_prefix("silo-import-") else {
        return false;
    };
    suffix.len() == 32
        && suffix
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Resolve and persist the native group once. Records from the original
/// checkpoint format used the computer name as MicroSandbox's default group.
pub(crate) fn ensure_snapshot_group(
    paths: &RuntimePaths,
    id: &str,
    computer_name: &str,
) -> Result<String, RuntimeError> {
    let mut record = load(paths, id)?;
    if let Some(group) = record.snapshot_group.as_deref() {
        if !valid_snapshot_group(group) {
            return Err(error(
                "The saved checkpoint group is invalid; it was preserved.",
            ));
        }
        return Ok(group.to_owned());
    }
    if record.version != 0
        && record.pending_checkpoint_restore.is_none()
        && record.checkpoints.is_empty()
        && record.inflight_checkpoint.is_none()
    {
        return Err(error(
            "Silo cannot match the saved checkpoints to this computer. Its history was preserved. Relaunch Silo and retry; if the problem continues, report it with the computer name.",
        ));
    }
    let group = record
        .pending_checkpoint_restore
        .as_ref()
        .map(|pending| pending.source_computer.as_str())
        .unwrap_or(computer_name)
        .to_owned();
    if !valid_snapshot_group(&group) {
        return Err(error(
            "Silo could not identify this computer's checkpoints. No checkpoint action was started. Refresh its history and retry.",
        ));
    }
    record.snapshot_group = Some(group.clone());
    save(paths, id, &record)?;
    Ok(group)
}

pub(super) fn save(paths: &RuntimePaths, id: &str, record: &Record) -> Result<(), RuntimeError> {
    uuid::Uuid::parse_str(id).map_err(|_| error("Silo could not identify this computer. Refresh its status and retry the checkpoint action."))?;
    let directory = directory(paths);
    fs::create_dir_all(&directory).map_err(|_| error("Checkpoint history could not be saved."))?;
    let mut file = tempfile::NamedTempFile::new_in(&directory)
        .map_err(|_| error("Checkpoint history could not be saved."))?;
    let mut record = record.clone();
    record.version = 1;
    serde_json::to_writer(&mut file, &record)
        .map_err(|_| error("Checkpoint history could not be encoded."))?;
    file.as_file()
        .sync_all()
        .map_err(|_| error("Checkpoint history could not be synced."))?;
    file.persist(path(paths, id))
        .map_err(|_| error("Checkpoint history could not be saved."))?;
    File::open(&directory)
        .and_then(|dir| dir.sync_all())
        .map_err(|_| error("Checkpoint history could not be synced."))
}

fn computer_configuration(
    paths: &RuntimePaths,
    id: &str,
) -> Result<ComputerConfiguration, RuntimeError> {
    read_metadata(&paths.metadata)?
        .computers
        .into_iter()
        .find(|configuration| configuration.id() == id)
        .ok_or_else(|| RuntimeError::Invalid("This computer is not a local computer.".into()))
}

fn snapshot_ready(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    source: &str,
    checkpoint_id: &str,
    scope: &str,
) -> Result<(), RuntimeError> {
    if snapshot_available(runner, paths, source, checkpoint_id, scope)? {
        Ok(())
    } else {
        Err(error(
            "The checkpoint is absent or incomplete in the runtime. No computer state was changed.",
        ))
    }
}

/// Whether the runtime lists the member as ready. Errors mean the list itself failed.
fn snapshot_available(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    source: &str,
    checkpoint_id: &str,
    scope: &str,
) -> Result<bool, RuntimeError> {
    Ok(snapshot_scope(runner, paths, source, checkpoint_id, scope)?.is_some())
}

fn snapshot_scope(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    source: &str,
    checkpoint_id: &str,
    desired_scope: &str,
) -> Result<Option<String>, RuntimeError> {
    let output = runner.run(
        paths,
        &[
            "snapshot".into(),
            "list".into(),
            "--format".into(),
            "json".into(),
        ],
        READ_TIMEOUT,
    )?;
    let entries: Vec<Value> = serde_json::from_str(&output.stdout)
        .map_err(|_| error("The runtime returned an invalid checkpoint list."))?;
    // A full snapshot contains the disks too, so it also satisfies a disk-only restore
    // (for example an imported checkpoint export, which always restores disks only).
    let scope_matches = |entry: &Value| {
        entry["scope"] == desired_scope || (desired_scope == "disk" && entry["scope"] == "full")
    };
    Ok(entries
        .iter()
        .find(|entry| {
            entry["group"] == source
                && entry["name"] == checkpoint_id
                && scope_matches(entry)
                && entry["availability"] == "ready"
        })
        .and_then(|entry| entry["scope"].as_str().map(str::to_owned)))
}

fn verify_snapshot(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    source: &str,
    checkpoint: &Checkpoint,
) -> Result<(), RuntimeError> {
    snapshot_ready(
        runner,
        paths,
        source,
        checkpoint.native_id(),
        &checkpoint.scope,
    )?;
    runner.run(
        paths,
        &[
            "snapshot".into(),
            "verify".into(),
            format!("{source}:{}", checkpoint.native_id()),
        ],
        Duration::from_secs(900),
    )?;
    Ok(())
}

fn current_network_args(config: &Value) -> Result<Vec<String>, RuntimeError> {
    let policy = config.pointer("/network/policy").ok_or_else(|| {
        error("The current network policy is unavailable. The checkpoint was not started.")
    })?;
    let mut args = Vec::new();
    for (key, flag) in [
        ("default_egress", "--net-default-egress"),
        ("default_ingress", "--net-default-ingress"),
    ] {
        let value = policy[key]
            .as_str()
            .filter(|value| matches!(*value, "allow" | "deny"))
            .ok_or_else(|| {
                error("The current network policy is unsupported. The checkpoint was not started.")
            })?;
        args.extend([flag.into(), value.into()]);
    }
    let rules = policy["rules"].as_array().ok_or_else(|| {
        error("The current network rules are unavailable. The checkpoint was not started.")
    })?;
    if rules.len() > 128 {
        return Err(error("The current network policy has too many rules."));
    }
    for rule in rules {
        let action = rule["action"]
            .as_str()
            .filter(|value| matches!(*value, "allow" | "deny"))
            .ok_or_else(|| error("A current network rule is unsupported."))?;
        let direction = rule["direction"]
            .as_str()
            .filter(|value| matches!(*value, "ingress" | "egress"))
            .ok_or_else(|| error("A current network rule is unsupported."))?;
        let group = rule
            .pointer("/destination/group")
            .and_then(Value::as_str)
            .filter(|value| {
                !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_lowercase())
            })
            .ok_or_else(|| error("A current network destination is unsupported."))?;
        let protocols = rule["protocols"]
            .as_array()
            .ok_or_else(|| error("A current network protocol is unsupported."))?;
        let ports = rule["ports"]
            .as_array()
            .ok_or_else(|| error("A current network port range is unsupported."))?;
        if action == "allow"
            && direction == "egress"
            && group == "host"
            && ports.len() == 1
            && ports[0]["start"] == 53
            && ports[0]["end"] == 53
            && protocols.len() == 2
            && protocols.iter().any(|value| value == "tcp")
            && protocols.iter().any(|value| value == "udp")
        {
            args.extend(["--net-rule".into(), "allow@dns".into()]);
            continue;
        }
        if action == "allow"
            && direction == "egress"
            && group == "public"
            && ports.is_empty()
            && protocols.is_empty()
        {
            args.extend(["--net-rule".into(), "allow@public".into()]);
            continue;
        }
        if ports.len() > 32 || protocols.len() > 3 || (!ports.is_empty() && protocols.is_empty()) {
            return Err(error("A current network rule is unsupported."));
        }
        if protocols.len() > 1 || ports.len() > 1 {
            return Err(error(
                "A current network rule cannot be reproduced exactly.",
            ));
        }
        let protocol_values: Vec<Option<&str>> = if protocols.is_empty() {
            vec![None]
        } else {
            protocols
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .filter(|value| matches!(*value, "tcp" | "udp"))
                })
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| error("A current network protocol is unsupported."))?
                .into_iter()
                .map(Some)
                .collect()
        };
        let port_values: Vec<Option<String>> = if ports.is_empty() {
            vec![None]
        } else {
            ports
                .iter()
                .map(|port| {
                    let start = port["start"]
                        .as_u64()
                        .filter(|value| (1..=65535).contains(value))?;
                    let end = port["end"]
                        .as_u64()
                        .filter(|value| (start..=65535).contains(value))?;
                    Some(Some(if start == end {
                        start.to_string()
                    } else {
                        format!("{start}-{end}")
                    }))
                })
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| error("A current network port range is unsupported."))?
        };
        for protocol in &protocol_values {
            for port in &port_values {
                let mut token = format!("{action}:{direction}@{group}");
                if let Some(protocol) = protocol {
                    token.push_str(&format!(":{protocol}"));
                }
                if let Some(port) = port {
                    token.push_str(&format!(":{port}"));
                }
                args.extend(["--net-rule".into(), token]);
            }
        }
    }
    Ok(args)
}

fn capture_with(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    id: &str,
    display_name: &str,
    reason: &str,
) -> Result<(), RuntimeError> {
    let label = display_name.trim();
    if label.is_empty() || label.chars().count() > 80 || label.chars().any(char::is_control) {
        return Err(RuntimeError::Invalid(
            "Checkpoint name must contain 1 to 80 printable characters.".into(),
        ));
    }
    let configuration = computer_configuration(paths, id)?;
    let mut record = load(paths, id)?;
    if let Some(pending) = record.pending_checkpoint_restore.clone() {
        if record.restore_journal.is_some() {
            return Err(error(
                "A Restore recovery is unfinished. Resolve it before creating a checkpoint.",
            ));
        }
        let snapshot_group = ensure_snapshot_group(paths, id, configuration.name())?;
        record = load(paths, id)?;
        if pending.source_computer != snapshot_group
            || !valid_native_id(&pending.checkpoint_id)
            || !matches!(pending.state.as_str(), "full" | "disk")
        {
            return Err(error(
                "The pending checkpoint reference is invalid. It was preserved.",
            ));
        }
        ensure_pending_runtime_absent(runner, paths, configuration.name())?;
        snapshot_ready(
            runner,
            paths,
            &snapshot_group,
            &pending.checkpoint_id,
            &pending.state,
        )?;
        let checkpoint = Checkpoint {
            id: new_checkpoint_id(&record),
            native_id: Some(pending.checkpoint_id),
            name: label.into(),
            created_at: activity_timestamp(),
            scope: pending.state,
            reason: reason.into(),
        };
        record.checkpoints.insert(0, checkpoint);
        record.checkpoint_operation = None;
        return save(paths, id, &record);
    }
    ensure_no_unfinished_restore(&record, "creating another checkpoint")?;
    let mut inspected = inspect_computer(runner, paths, configuration.name())?;
    ensure_managed(&inspected)?;
    if inspected
        .config
        .pointer("/labels/silo.machine-id")
        .and_then(Value::as_str)
        != Some(id)
    {
        return Err(error(
            "The computer runtime identity changed. No checkpoint was made.",
        ));
    }
    if inspected.status == "Paused"
        && recover_paused_capture(runner, paths, id, configuration.name())?
    {
        inspected = inspect_capture_source(runner, paths, id, configuration.name())?;
    }
    let scope = match inspected.status.as_str() {
        "Running" => "full",
        "Created" | "Stopped" => "disk",
        _ => {
            return Err(RuntimeError::Invalid(
                "Wait until the computer is running or stopped before creating a checkpoint."
                    .into(),
            ));
        }
    };
    // The old external RawDiskImage mount is excluded by upstream snapshots.
    // Conversion to an owned volume is required before a checkpoint is useful.
    let owned_workspace = inspected
        .config
        .get("mounts")
        .and_then(Value::as_array)
        .is_some_and(|mounts| {
            let computer: Vec<_> = mounts
                .iter()
                .filter(|mount| mount.get("guest").and_then(Value::as_str) == Some(WORKSPACE_MOUNT))
                .collect();
            computer.len() == 1
                && computer[0].get("type").and_then(Value::as_str) == Some("Owned")
                && computer[0].pointer("/storage/kind").and_then(Value::as_str) == Some("disk")
        });
    if !owned_workspace {
        return Err(error(
            "This computer still uses the old workspace disk. Finish runtime migration before creating a checkpoint.",
        ));
    }
    let snapshot_group = ensure_snapshot_group(paths, id, configuration.name())?;
    record.snapshot_group = Some(snapshot_group.clone());
    if let Some(interrupted) = record.inflight_checkpoint.clone() {
        if !discard_failed_capture(
            runner,
            paths,
            id,
            (snapshot_group.clone(), interrupted.native_id().to_owned()),
        ) {
            return Err(error("An interrupted checkpoint could not be removed. Retry after its dependencies are released."));
        }
        record.inflight_checkpoint = None;
        record.checkpoint_operation = None;
        save(paths, id, &record)?;
    }
    let checkpoint_id = new_checkpoint_id(&record);
    let new_checkpoint = Checkpoint {
        id: checkpoint_id.clone(),
        native_id: None,
        name: label.into(),
        created_at: activity_timestamp(),
        scope: scope.into(),
        reason: reason.into(),
    };
    record.inflight_checkpoint = Some(new_checkpoint.clone());
    record.checkpoint_operation = Some(Operation {
        kind: "capture".into(),
        status: "running".into(),
        stage: "Capturing computer state".into(),
        error: None,
    });
    save(paths, id, &record)?;
    let mut args = vec![
        "snapshot".into(),
        "create".into(),
        checkpoint_id.clone(),
        "--from-sandbox".into(),
        configuration.name().into(),
        "--group".into(),
        snapshot_group.clone(),
    ];
    if scope == "full" {
        args.extend(["--full".into(), "--guest-flush".into(), "required".into()]);
    }
    args.push("--integrity".into());
    let (result, failure_stage) = match runner.run(paths, &args, Duration::from_secs(900)) {
        Ok(_) => (
            // Once create returns, cancellation must not interrupt verification.
            super::operation_gate::uncancellable(|| {
                snapshot_ready(runner, paths, &snapshot_group, &checkpoint_id, scope)
            }),
            "Verification failed",
        ),
        Err(failure) => (Err(failure), "Checkpoint failed"),
    };
    match result {
        Ok(_) => {
            record.checkpoints.insert(0, new_checkpoint);
            record.inflight_checkpoint = None;
            record.checkpoint_operation = None;
            save(paths, id, &record)
        }
        Err(failure) => {
            let (settled, failure) = if scope == "full" {
                settle_capture_failure(runner, paths, id, configuration.name(), failure)
            } else {
                (true, failure)
            };
            // Keep the journal and partial member until the source is recoverable.
            if settled
                && discard_failed_capture(
                    runner,
                    paths,
                    id,
                    (snapshot_group.clone(), checkpoint_id.clone()),
                )
            {
                record.inflight_checkpoint = None;
            }
            record.checkpoint_operation = Some(Operation {
                kind: "capture".into(),
                status: "failed".into(),
                stage: failure_stage.into(),
                error: Some(failure.to_string()),
            });
            Err(save_failure(paths, id, &record, failure))
        }
    }
}

fn ensure_no_unfinished_restore(record: &Record, action: &str) -> Result<(), RuntimeError> {
    if record.restore_journal.is_some() {
        return Err(error(&format!(
            "Finish the pending Restore before {action}."
        )));
    }
    Ok(())
}

#[tauri::command]
pub async fn create_checkpoint(
    app: AppHandle,
    computer_id: String,
    name: String,
) -> Result<ApplicationSource, String> {
    crate::runtime_migration::ensure_ready_async(&app).await?;
    let worker_app = app.clone();
    super::operation_gate::spawn_blocking(move || {
        let paths = runtime_paths(&worker_app)?;
        // Capture touches only this computer's own snapshot store and per-computer checkpoint
        // record, not the shared inventory, so it is ordered per computer by stable id.
        let computer_name = computer_configuration(&paths, &computer_id)
            .map_err(|error| error.to_string())?
            .name()
            .to_owned();
        let guard = OPERATIONS
            .kind(super::operation_gate::OperationKind::CheckpointCapture)
            .computer(&computer_id, &computer_name, "Creating checkpoint")
            .map_err(|error| error.to_string())?;
        // Checkpoint capture is cancellable and expected to finish within 15 minutes.
        guard.allow_cancel();
        guard.expect_within(std::time::Duration::from_secs(15 * 60));
        shutdown::ensure_accepting_operations()?;
        let result = capture_with(&ProcessRunner, &paths, &computer_id, &name, "manual");
        drop(guard);
        let result = result.and_then(|_| application_state_response(&worker_app, &paths));
        let _ = worker_app.emit("silo://application-state-changed", ());
        result.map_err(|e| e.to_string())
    })
    .await
    .map_err(|_| {
        "Silo could not finish creating the checkpoint. Refresh its history before trying again."
            .to_string()
    })?
}

pub(super) fn is_pending(paths: &RuntimePaths, id: &str) -> Result<bool, RuntimeError> {
    Ok(load(paths, id)?.pending_checkpoint_restore.is_some())
}
pub(super) fn needs_explicit_start(paths: &RuntimePaths, id: &str) -> Result<bool, RuntimeError> {
    let record = load(paths, id)?;
    Ok(record.pending_checkpoint_restore.is_some() || record.restore_journal.is_some())
}

pub(super) fn pending_view(
    paths: &RuntimePaths,
    id: &str,
    runtime_exists: bool,
) -> Result<bool, RuntimeError> {
    let record = load(paths, id)?;
    // Once a restore attempt has created its runtime computer, the computer is present: it is
    // read from the runtime (with its pending restore still exposed for attention) so the
    // configured and listed computers keep matching, and deletion removes that computer.
    Ok((record.pending_checkpoint_restore.is_some()
        && !(runtime_exists && record.restore_attempted))
        || (!runtime_exists
            && record
                .restore_journal
                .as_ref()
                .is_some_and(|journal| journal.phase == "secured")))
}

pub(super) fn view_pending(record: &Record, name: &str) -> Option<PendingRestore> {
    record.pending_checkpoint_restore.clone().or_else(|| {
        let journal = record
            .restore_journal
            .as_ref()
            .filter(|journal| journal.phase == "secured")?;
        let target = record
            .checkpoints
            .iter()
            .find(|checkpoint| checkpoint.id == journal.target_checkpoint_id)?;
        Some(PendingRestore {
            checkpoint_id: target.native_id().to_owned(),
            source_computer: record.snapshot_group.as_deref().unwrap_or(name).into(),
            state: target.scope.clone(),
        })
    })
}

pub(crate) fn forget_removed(paths: &RuntimePaths, id: &str) -> Result<(), RuntimeError> {
    crate::computer_use::forget(paths, id)?;
    let target = path(paths, id);
    match fs::remove_file(target) {
        Ok(()) => File::open(directory(paths))
            .and_then(|directory| directory.sync_all())
            .map_err(|_| error("Removed checkpoint history could not be synced.")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(error("Removed checkpoint history could not be cleared.")),
    }
}

pub(super) fn pending_computer(
    configuration: ComputerConfiguration,
) -> Result<ApplicationComputer, RuntimeError> {
    Ok(ApplicationComputer {
        configuration,
        purpose: "Local MicroSandbox".into(),
        state: ComputerState::Stopped,
        state_detail: "Ready to start from checkpoint".into(),
        can_dismiss_error: false,
        lifecycle_failure: None,
        attention: None,
        freshness: Freshness::Fresh,
        settling: false,
        repositories: Vec::new(),
        files: Vec::new(),
        ports: Vec::new(),
        logs: Vec::new(),
        github_repositories: Vec::new(),
        secret_names: Vec::new(),
        pending_secret_revocations: Vec::new(),
        checkpoints: Vec::new(),
        pending_checkpoint_restore: None,
        checkpoint_operation: None,
        unfinished_restore: None,
    })
}

/// True for the exact `c` + 31 hex-digit form produced by `new_checkpoint_id`.
fn is_checkpoint_native_id(id: &str) -> bool {
    id.len() == 32
        && id.as_bytes()[0] == b'c'
        && id[1..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

/// Resolve a stored checkpoint into the MicroSandbox lineage selector a portable
/// export needs. The checkpoint is addressed by its public Silo id in this
/// computer's record; the returned member is MicroSandbox's immutable name.
/// Reads `(snapshot_group, native_member, scope, display_name)` without changing history.
pub(crate) fn export_source(
    paths: &RuntimePaths,
    computer_id: &str,
    checkpoint_id: &str,
) -> Result<(String, String, String, String), RuntimeError> {
    let configuration = computer_configuration(paths, computer_id)?;
    let record = load(paths, computer_id)?;
    if record.restore_journal.is_some() {
        return Err(error(
            "Finish the pending Restore before exporting a checkpoint.",
        ));
    }
    if record
        .inflight_checkpoint
        .as_ref()
        .is_some_and(|checkpoint| checkpoint.id == checkpoint_id)
    {
        return Err(error(
            "That checkpoint is still being captured. Export it once capture finishes.",
        ));
    }
    let checkpoint = record
        .checkpoints
        .iter()
        .find(|checkpoint| checkpoint.id == checkpoint_id)
        .ok_or_else(|| RuntimeError::Invalid("The selected checkpoint no longer exists.".into()))?
        .clone();
    let snapshot_group = record
        .snapshot_group
        .as_deref()
        .or_else(|| {
            record
                .pending_checkpoint_restore
                .as_ref()
                .map(|pending| pending.source_computer.as_str())
        })
        .unwrap_or(configuration.name());
    if !valid_snapshot_group(snapshot_group) {
        return Err(error(
            "Silo could not identify this computer's checkpoints. No checkpoint action was started. Refresh its history and retry.",
        ));
    }
    Ok((
        snapshot_group.to_owned(),
        checkpoint.native_id().to_owned(),
        checkpoint.scope.clone(),
        checkpoint.name.clone(),
    ))
}

/// Record an archive snapshot as a new stopped computer. Snapshot loading
/// only installs immutable data; activation is deliberately deferred to the
/// common explicit-start path.
#[cfg(test)]
pub(crate) fn import_pending_restore(
    paths: &RuntimePaths,
    computer_id: &str,
    source_group: &str,
    member: &str,
) -> Result<(), RuntimeError> {
    import_pending_restore_with_environment(paths, computer_id, source_group, member, &Value::Null)
}

pub(crate) fn import_pending_restore_with_environment(
    paths: &RuntimePaths,
    computer_id: &str,
    source_group: &str,
    member: &str,
    runtime_config: &Value,
) -> Result<(), RuntimeError> {
    // These are MicroSandbox snapshot selectors, not computer names. Require
    // the exact forms produced by Silo's v3 export/import before saving intent.
    // The member is either a state export's `silo-backup-<n>-<n>-<n>` name or,
    // for a checkpoint export, that checkpoint's `c`+31-hex native id.
    let group_suffix = source_group.strip_prefix("silo-import-").unwrap_or("");
    let member_suffix = member.strip_prefix("silo-backup-").unwrap_or("");
    let member_parts: Vec<_> = member_suffix.split('-').collect();
    let backup_member = member.starts_with("silo-backup-")
        && member_parts.len() == 3
        && member_parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()));
    if source_group.len() != 44
        || group_suffix.len() != 32
        || !group_suffix
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || member.len() > 128
        || !(backup_member || is_checkpoint_native_id(member))
    {
        return Err(RuntimeError::Invalid(
            "Imported snapshot reference is invalid.".into(),
        ));
    }
    let environment: Vec<Environment> = serde_json::from_value(
        runtime_config
            .get("env")
            .cloned()
            .unwrap_or_else(|| serde_json::json!([])),
    )
    .map_err(|_| RuntimeError::Invalid("Imported environment is invalid.".into()))?;
    if environment.iter().any(|entry| !entry.valid()) {
        return Err(RuntimeError::Invalid(
            "Imported environment is invalid.".into(),
        ));
    }
    // An import (and so a transfer) starts from this device's initial approval mode with
    // no attempt known, whatever policy a computer of this id had here: its first boot applies it
    // over the configuration the imported disk carries.
    crate::computer_use::forget(paths, computer_id)?;
    crate::computer_use::start_with(paths, computer_id, crate::computer_use::initial_approval());
    let mut record = Record::default();
    record.snapshot_group = Some(source_group.to_owned());
    record.desired_environment = environment
        .into_iter()
        .filter(|entry| entry.key != "GH_TOKEN")
        .collect();
    record.pending_checkpoint_restore = Some(PendingRestore {
        checkpoint_id: member.to_owned(),
        source_computer: source_group.to_owned(),
        state: "disk".into(),
    });
    // Imported policy and credentials are archive input and carry no authority.
    // Start will resolve current host-side assignments against this deny policy.
    record.desired_network_policy = Some(serde_json::json!({
        "default_egress": "deny",
        "default_ingress": "deny",
        "rules": []
    }));
    save(paths, computer_id, &record)
}

/// Capture a checkpoint through the production capture path and return its public
/// Silo id. Used by opt-in live tests to exercise real checkpoint exports.
#[cfg(test)]
pub(crate) fn capture_for_test(
    paths: &RuntimePaths,
    computer_id: &str,
    name: &str,
) -> Result<String, RuntimeError> {
    capture_with(&ProcessRunner, paths, computer_id, name, "manual")?;
    load(paths, computer_id)?
        .checkpoints
        .first()
        .map(|checkpoint| checkpoint.id.clone())
        .ok_or_else(|| error("The captured checkpoint was not recorded."))
}

fn running_child_matches(
    inspected: &InspectedSandbox,
    id: &str,
    attempt_id: &str,
    material: &secrets_runtime::Material,
    policy: &Value,
) -> bool {
    inspected.status == "Running"
        && inspected
            .config
            .pointer("/labels/silo.managed")
            .and_then(Value::as_str)
            == Some("true")
        && inspected
            .config
            .pointer("/labels/silo.machine-id")
            .and_then(Value::as_str)
            == Some(id)
        && inspected
            .config
            .pointer("/labels/silo.restore-attempt")
            .and_then(Value::as_str)
            == Some(attempt_id)
        && inspected.config.pointer("/network/policy") == Some(policy)
        && secrets_runtime::verify_config(&inspected.config, material)
        && inspected
            .config
            .pointer("/network/secrets/secrets")
            .and_then(Value::as_array)
            .is_some_and(|secrets| {
                secrets.iter().any(|secret| {
                    secret["env_var"] == "SILO_GITHUB"
                        && secret["value"] == ""
                        && secret["source"]["kind"] == "env"
                        && secret["source"]["var"] == "SILO_GITHUB"
                })
            })
}

/// A built-in computer must have the read-only computer-use mount, whichever recovery path
/// accepts it. Returns before any state changes, so the pending restore is preserved.
fn require_mount(
    observed: &InspectedSandbox,
    configuration: &ComputerConfiguration,
) -> Result<(), RuntimeError> {
    if crate::computer_use::mount_present(&observed.config, configuration) {
        Ok(())
    } else {
        Err(error(
            "The restored computer does not have the read-only computer-use folder. It was preserved for inspection; the pending restore is unchanged.",
        ))
    }
}

pub(super) fn start_pending(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    configuration: &ComputerConfiguration,
) -> Result<(), RuntimeError> {
    // Before any state changes: a computer that needs the mount cannot restore without it.
    let mounts = crate::computer_use::mount_args(configuration)?;
    let mut record = load(paths, configuration.id())?;
    let lineage_group = ensure_snapshot_group(paths, configuration.id(), configuration.name())?;
    record.snapshot_group = Some(lineage_group.clone());
    if record.pending_checkpoint_restore.is_none()
        && record
            .restore_journal
            .as_ref()
            .is_some_and(|journal| journal.phase == "secured")
    {
        let listed = runner.run(
            paths,
            &["list".into(), "--format".into(), "json".into()],
            READ_TIMEOUT,
        )?;
        let listed: Vec<ListedSandbox> = serde_json::from_str(&listed.stdout)
            .map_err(|_| error("The runtime returned an invalid computer list."))?;
        if listed
            .iter()
            .any(|entry| entry.name == configuration.name())
        {
            return Err(error(&unfinished_restore_message(
                &record,
                configuration.name(),
            )));
        }
        let journal = record.restore_journal.clone().unwrap();
        let target = record
            .checkpoints
            .iter()
            .find(|checkpoint| checkpoint.id == journal.target_checkpoint_id)
            .ok_or_else(|| error("The selected checkpoint was lost from recovery history."))?;
        record.pending_checkpoint_restore = Some(PendingRestore {
            checkpoint_id: target.native_id().to_owned(),
            source_computer: lineage_group.clone(),
            state: target.scope.clone(),
        });
        record.restore_journal = None;
        record.checkpoint_operation = None;
        save(paths, configuration.id(), &record)?;
    }
    let pending = record.pending_checkpoint_restore.clone().ok_or_else(|| {
        if record.restore_journal.is_some() {
            RuntimeError::Invalid(unfinished_restore_message(&record, configuration.name()))
        } else {
            RuntimeError::Invalid(format!(
                "{} has no checkpoint to start from.",
                configuration.name()
            ))
        }
    })?;
    if !matches!(pending.state.as_str(), "full" | "disk") {
        return Err(error(
            "The pending checkpoint has an unsupported capture scope.",
        ));
    }
    if pending.source_computer != lineage_group {
        return Err(error(
            "Silo cannot match the pending checkpoint to this computer's history. The checkpoint was preserved. Relaunch Silo and retry Start.",
        ));
    }
    let policy = record.desired_network_policy.clone().ok_or_else(|| {
        error("The fork's desired network policy is missing. The checkpoint was preserved.")
    })?;
    let network_args = current_network_args(&serde_json::json!({"network":{"policy":policy}}))?;
    let material = crate::secrets::runtime_material(configuration.name())
        .map_err(RuntimeError::Unavailable)?;
    secrets_runtime::validate_material(&material).map_err(RuntimeError::Invalid)?;
    let native_scope = snapshot_scope(
        runner,
        paths,
        &pending.source_computer,
        &pending.checkpoint_id,
        &pending.state,
    )?
    .ok_or_else(|| {
        error(
            "The checkpoint is absent or incomplete in the runtime. No computer state was changed.",
        )
    })?;
    let listed = runner.run(
        paths,
        &["list".into(), "--format".into(), "json".into()],
        READ_TIMEOUT,
    )?;
    let listed: Vec<ListedSandbox> = serde_json::from_str(&listed.stdout)
        .map_err(|_| error("The runtime returned an invalid computer list."))?;
    if listed
        .iter()
        .any(|entry| entry.name == configuration.name())
    {
        if !record.restore_attempted {
            return Err(error(
                "A runtime computer already uses this fork's name. No restore was attempted.",
            ));
        }
        let observed = inspect_computer(runner, paths, configuration.name())?;
        let attempt_id = record.restore_attempt_id.as_deref().ok_or_else(|| {
            error("The previous restore attempt has no saved identity. It was preserved.")
        })?;
        if running_child_matches(
            &observed,
            configuration.id(),
            attempt_id,
            &material,
            &policy,
        ) {
            // Accepting the running computer must not skip the mount check a fresh restore gets.
            require_mount(&observed, configuration)?;
            record.pending_checkpoint_restore = None;
            record.checkpoint_operation = None;
            record.restore_attempted = false;
            record.restore_attempt_ran = false;
            record.restore_attempt_id = None;
            save(paths, configuration.id(), &record)?;
            return Ok(());
        }
        if observed
            .config
            .pointer("/labels/silo.machine-id")
            .and_then(Value::as_str)
            != Some(configuration.id())
            || observed
                .config
                .pointer("/labels/silo.managed")
                .and_then(Value::as_str)
                != Some("true")
            || observed
                .config
                .pointer("/labels/silo.restore-attempt")
                .and_then(Value::as_str)
                != Some(attempt_id)
            || !matches!(observed.status.as_str(), "Created" | "Stopped" | "Crashed")
        {
            return Err(error(
                "A previous restore attempt has unverified runtime state. It was preserved for inspection.",
            ));
        }
        if record.restore_attempt_ran {
            // Checked before any recovery state is cleared: without the mount the
            // attempt would start without computer use and the record could not show why.
            require_mount(&observed, configuration)?;
            // The attempt already ran and may hold changes; recreating it from the
            // checkpoint would silently discard them. Keep it as the computer and start it
            // like any other; only an explicit, confirmed action (such as Delete) discards it.
            record.pending_checkpoint_restore = None;
            record.restore_attempted = false;
            record.restore_attempt_id = None;
            record.restore_attempt_ran = false;
            record.checkpoint_operation = None;
            save(paths, configuration.id(), &record)?;
            return super::lifecycle_recovery::perform(
                runner,
                paths,
                &device_resources()?,
                "start",
                configuration.name(),
            );
        }
        runner.run(
            paths,
            &[
                "remove".into(),
                "--quiet".into(),
                configuration.name().into(),
            ],
            STOP_TIMEOUT,
        )?;
    }
    record.restore_attempted = true;
    let attempt_id = uuid::Uuid::new_v4().to_string();
    record.restore_attempt_id = Some(attempt_id.clone());
    record.checkpoint_operation = Some(Operation {
        kind: "fork".into(),
        status: "running".into(),
        stage: "Starting from checkpoint".into(),
        error: None,
    });
    save(paths, configuration.id(), &record)?;
    let mut args = vec![
        "restore".into(),
        format!("{}:{}", pending.source_computer, pending.checkpoint_id),
        "--name".into(),
        configuration.name().into(),
    ];
    // A disk snapshot cold-boots by default. MicroSandbox's --disk-only
    // selects the disk from a *full* checkpoint and rejects file/disk captures.
    if pending.state == "disk" && native_scope == "full" {
        args.push("--disk-only".into());
    }
    if pending.state == "full" {
        args.push("--cow-mem".into());
    } else {
        let ComputerConfiguration {
            cpus, memory_gib, ..
        } = configuration;
        // Disk restore starts a new computer and otherwise uses the runtime's 1 CPU / 512 MiB
        // defaults. Preserve the user's saved Silo resources on imported cold boots.
        args.extend([
            "--cpus".into(),
            cpus.to_string(),
            "--memory".into(),
            format!("{memory_gib}G"),
        ]);
    }
    for label in [
        MANAGED_LABEL.to_string(),
        format!("silo.machine-id={}", configuration.id()),
        format!("silo.restore-attempt={attempt_id}"),
        "silo.github-protocol=1".into(),
    ] {
        args.extend(["--label".into(), label]);
    }
    for entry in &record.desired_environment {
        if entry.key != "GH_TOKEN" {
            args.push(format!("--env={}={}", entry.key, entry.value));
        }
    }
    args.extend(["--env".into(), "GH_TOKEN=$MSB_SILO_GITHUB".into()]);
    args.extend([
        "--secret".into(),
        super::secrets_runtime::SILO_GITHUB_SECRET_SPEC.into(),
    ]);
    for (name, _, domains) in &material {
        args.extend([
            "--secret".into(),
            format!("{name}:passthrough=*@{}", domains.join(",")),
        ]);
    }
    args.extend(network_args);
    // A snapshot never carries host mounts: pass the computer-use mount again.
    args.extend(mounts);
    let restored = runner.run(paths, &args, Duration::from_secs(900));
    // A completed restore resumed the computer; after a failure, a running, paused or crashed computer
    // shows it ran as well. Checked even after a cancel.
    let ran = restored.is_ok()
        || super::operation_gate::uncancellable(|| {
            inspect_computer(runner, paths, configuration.name())
        })
        .is_ok_and(|computer| matches!(computer.status.as_str(), "Running" | "Paused" | "Crashed"));
    let result = restored
        .and_then(|_| inspect_computer(runner, paths, configuration.name()))
        .and_then(|observed| if running_child_matches(&observed, configuration.id(), &attempt_id, &material, &policy)
            && crate::computer_use::mount_present(&observed.config, configuration) {
            Ok(observed)
        } else { Err(error("The restored computer did not reach a verified running state. Its checkpoint was preserved.")) });
    match result {
        Ok(observed) => {
            record.pending_checkpoint_restore = None;
            record.restore_attempted = false;
            record.restore_attempt_id = None;
            record.restore_attempt_ran = false;
            record.checkpoint_operation = None;
            save(paths, configuration.id(), &record)?;
            let revision = crate::secrets::computer_revision(configuration.name())
                .map_err(RuntimeError::Unavailable)?;
            crate::secrets::computer_started(configuration.name(), &revision)
                .map_err(RuntimeError::Unavailable)?;
            crate::network::reconcile_started(paths, configuration.name());
            crate::ssh_access::reconcile(paths);
            // Record the verified storage runtime like an ordinary Start, so the Storage
            // panel and automatic reclamation treat the restored computer as current.
            super::storage::after_start(runner, paths, &observed);
            Ok(())
        }
        Err(failure) => {
            record.restore_attempt_ran = ran;
            record.checkpoint_operation = Some(Operation {
                kind: "fork".into(),
                status: "failed".into(),
                stage: "Start failed".into(),
                error: Some(failure.to_string()),
            });
            Err(save_failure(paths, configuration.id(), &record, failure))
        }
    }
}

fn fork_source_policy(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    source: &ComputerConfiguration,
    record: &Record,
) -> Result<Value, RuntimeError> {
    if view_pending(record, source.name()).is_some() {
        let listed = runner.run(
            paths,
            &["list".into(), "--format".into(), "json".into()],
            READ_TIMEOUT,
        )?;
        let listed: Vec<ListedSandbox> = serde_json::from_str(&listed.stdout)
            .map_err(|_| error("The runtime returned an invalid computer list."))?;
        if !listed.iter().any(|entry| entry.name == source.name()) {
            let policy = record.desired_network_policy.clone().ok_or_else(|| {
                error("The source network policy is missing. No fork was created.")
            })?;
            current_network_args(&serde_json::json!({"network":{"policy":policy}}))?;
            return Ok(policy);
        }
    }
    let source_runtime = inspect_computer(runner, paths, source.name())?;
    ensure_managed(&source_runtime)?;
    if source_runtime
        .config
        .pointer("/labels/silo.machine-id")
        .and_then(Value::as_str)
        != Some(source.id())
    {
        return Err(error(
            "The source computer identity changed. No fork was created.",
        ));
    }
    current_network_args(&source_runtime.config)?;
    source_runtime
        .config
        .pointer("/network/policy")
        .cloned()
        .ok_or_else(|| error("The source network policy is missing."))
}

fn pending_current_fork_point(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    computer_name: &str,
    record: &Record,
    snapshot_group: &str,
) -> Result<Option<PendingRestore>, RuntimeError> {
    let Some(pending) = record.pending_checkpoint_restore.clone() else {
        return Ok(None);
    };
    if record.restore_journal.is_some()
        || pending.source_computer != snapshot_group
        || !valid_native_id(&pending.checkpoint_id)
        || !matches!(pending.state.as_str(), "full" | "disk")
    {
        return Err(error(
            "The pending checkpoint reference is invalid or has unfinished recovery. It was preserved.",
        ));
    }
    ensure_pending_runtime_absent(runner, paths, computer_name)?;
    snapshot_ready(
        runner,
        paths,
        snapshot_group,
        &pending.checkpoint_id,
        &pending.state,
    )?;
    Ok(Some(pending))
}

/// Host-side settings a fork copies from its source computer. Production uses the GitHub and
/// secret stores; tests inject failures to prove the rollback (E-17).
pub(super) trait ForkAssignments {
    fn copy_github(&self, source: &str, target: &str) -> Result<(), String>;
    fn forget_github(&self, target: &str) -> Result<(), String>;
    fn copy_secrets(&self, source: &str, target: &str) -> Result<(), String>;
    fn forget_secrets(&self, target: &str) -> Result<(), String>;
}

struct AppForkAssignments<'a>(&'a AppHandle);

impl ForkAssignments for AppForkAssignments<'_> {
    fn copy_github(&self, source: &str, target: &str) -> Result<(), String> {
        crate::github::fork_assignment(self.0, source, target)
    }
    fn forget_github(&self, target: &str) -> Result<(), String> {
        crate::github::forget_fork_assignment(self.0, target)
    }
    fn copy_secrets(&self, source: &str, target: &str) -> Result<(), String> {
        crate::secrets::fork_assignments(source, target)
    }
    fn forget_secrets(&self, target: &str) -> Result<(), String> {
        crate::secrets::computer_removed(target)
    }
}

/// The native member a fork starts from, resolved while the source's own lane is held.
pub(super) struct ForkSource {
    source_id: String,
    snapshot_group: String,
    member: String,
    scope: String,
    desired_policy: Value,
}

fn ensure_fork_name_available(
    metadata: &ComputerConfigurationRequest,
    new_name: &str,
) -> Result<(), RuntimeError> {
    if metadata.computers.len() >= MAX_COMPUTER_COUNT
        || metadata.computers.iter().any(|m| m.name() == new_name)
    {
        return Err(RuntimeError::Invalid(
            "The fork name is already in use or the computer limit was reached.".into(),
        ));
    }
    Ok(())
}

/// Phase one of a fork, under the source computer's lane: resolve the checkpoint, capturing a
/// "Fork point" of the current state when none was chosen. It touches only the source's
/// own snapshot store and checkpoint record, so other computers keep working meanwhile (E-09).
pub(super) fn fork_source(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    computer_id: &str,
    checkpoint_id: Option<&str>,
    new_name: &str,
) -> Result<ForkSource, RuntimeError> {
    validate_name(new_name)?;
    let metadata = read_metadata(&paths.metadata)?;
    let source = metadata
        .computers
        .iter()
        .find(|configuration| configuration.id() == computer_id)
        .ok_or_else(|| {
            RuntimeError::Invalid("The source computer is not a local computer.".into())
        })?
        .clone();
    // Fail before an expensive capture; the inventory write checks again.
    ensure_fork_name_available(&metadata, new_name)?;
    let source_record = load(paths, computer_id)?;
    let snapshot_group = ensure_snapshot_group(paths, computer_id, source.name())?;
    let pending_current = if checkpoint_id.is_none() {
        pending_current_fork_point(
            runner,
            paths,
            source.name(),
            &source_record,
            &snapshot_group,
        )?
    } else {
        None
    };
    if checkpoint_id.is_none() && pending_current.is_none() {
        ensure_no_unfinished_restore(&source_record, "forking its current state")?;
    }
    let desired_policy = fork_source_policy(runner, paths, &source, &source_record)?;
    let (member, scope) = if let Some(pending) = pending_current {
        (pending.checkpoint_id, pending.state)
    } else {
        let selected_id = match checkpoint_id {
            Some(id) => id.to_owned(),
            None => {
                capture_with(runner, paths, computer_id, "Fork point", "manual")?;
                load(paths, computer_id)?
                    .checkpoints
                    .first()
                    .ok_or_else(|| error("The fork checkpoint was not recorded."))?
                    .id
                    .clone()
            }
        };
        let source_record = load(paths, computer_id)?;
        let checkpoint = source_record
            .checkpoints
            .iter()
            .find(|c| c.id == selected_id)
            .ok_or_else(|| {
                RuntimeError::Invalid("The selected checkpoint no longer exists.".into())
            })?;
        snapshot_ready(
            runner,
            paths,
            &snapshot_group,
            checkpoint.native_id(),
            &checkpoint.scope,
        )?;
        (checkpoint.native_id().to_owned(), checkpoint.scope.clone())
    };
    Ok(ForkSource {
        source_id: computer_id.into(),
        snapshot_group,
        member,
        scope,
        desired_policy,
    })
}

/// Append cleanup failures to an error without replacing its message.
fn with_context(failure: RuntimeError, context: &str) -> RuntimeError {
    match failure {
        RuntimeError::Invalid(message) => RuntimeError::Invalid(format!("{message}{context}")),
        RuntimeError::Unavailable(message) => {
            RuntimeError::Unavailable(format!("{message}{context}"))
        }
        RuntimeError::Malformed(message) => RuntimeError::Malformed(format!("{message}{context}")),
        other => other,
    }
}

/// Phase two of a fork, under the device-wide lane: add the stopped fork to the shared
/// inventory. Failures before publication remove what this phase added. A published fork
/// keeps its dependent state even when the inventory's directory sync fails.
pub(super) fn fork_commit(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    assignments: &dyn ForkAssignments,
    fork: &ForkSource,
    new_name: &str,
) -> Result<(), RuntimeError> {
    fork_commit_with_metadata_writer(runner, paths, assignments, fork, new_name, &write_metadata)
}

fn fork_commit_with_metadata_writer(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    assignments: &dyn ForkAssignments,
    fork: &ForkSource,
    new_name: &str,
    write: &dyn Fn(&Path, &ComputerConfigurationRequest) -> Result<(), RuntimeError>,
) -> Result<(), RuntimeError> {
    validate_name(new_name)?;
    let mut metadata = read_metadata(&paths.metadata)?;
    // Resolve the source again: it may have been renamed or deleted between the phases.
    let source = metadata
        .computers
        .iter()
        .find(|configuration| configuration.id() == fork.source_id)
        .ok_or_else(|| {
            RuntimeError::Invalid(
                "The source computer no longer exists. No fork was created.".into(),
            )
        })?
        .clone();
    ensure_fork_name_available(&metadata, new_name)?;
    snapshot_ready(
        runner,
        paths,
        &fork.snapshot_group,
        &fork.member,
        &fork.scope,
    )?;
    let child_id = uuid::Uuid::new_v4().to_string();
    let mut child = source.clone();
    {
        let ComputerConfiguration { id, name, .. } = &mut child;
        *id = child_id.clone();
        *name = new_name.into();
    }
    let mut child_record = Record::default();
    child_record.snapshot_group = Some(fork.snapshot_group.clone());
    child_record.pending_checkpoint_restore = Some(PendingRestore {
        checkpoint_id: fork.member.clone(),
        source_computer: fork.snapshot_group.clone(),
        state: fork.scope.clone(),
    });
    child_record.desired_network_policy = Some(fork.desired_policy.clone());
    child_record.desired_environment = load(paths, source.id())?.desired_environment;
    save(paths, &child_id, &child_record)?;
    let mut copied_github = false;
    let mut copied_secrets = false;
    let result = (|| {
        crate::computer_use::inherit_settings(paths, source.id(), &child_id)?;
        assignments
            .copy_github(source.name(), new_name)
            .map_err(RuntimeError::Unavailable)?;
        copied_github = true;
        assignments
            .copy_secrets(source.name(), new_name)
            .map_err(RuntimeError::Unavailable)?;
        copied_secrets = true;
        metadata.computers.push(child);
        write(&paths.metadata, &metadata)
    })();
    let Err(failure) = result else {
        return Ok(());
    };
    // A metadata write can fail after replacement, while syncing its parent directory.
    // Remove dependencies only when the inventory proves the child was not published.
    match read_metadata(&paths.metadata) {
        Ok(saved)
            if !saved
                .computers
                .iter()
                .any(|configuration| configuration.id() == child_id) => {}
        Ok(_) => {
            return Err(with_context(
                failure,
                " The fork was saved. Its checkpoint and assignments were preserved; refresh the computer list before retrying.",
            ));
        }
        Err(_) => {
            return Err(with_context(
                failure,
                " The computer list could not be checked. The fork's checkpoint and assignments were preserved.",
            ));
        }
    }
    // Undo in reverse order; every step runs even if an earlier one fails.
    let mut failed = Vec::new();
    if copied_secrets {
        failed.extend(assignments.forget_secrets(new_name).err());
    }
    if copied_github {
        failed.extend(assignments.forget_github(new_name).err());
    }
    failed.extend(
        forget_removed(paths, &child_id)
            .err()
            .map(|error| error.to_string()),
    );
    if failed.is_empty() {
        return Err(failure);
    }
    Err(with_context(
        failure,
        &format!(" Cleanup was incomplete: {}", failed.join(" ")),
    ))
}

/// Both fork phases with their lanes: the source's own lane while its checkpoint is
/// resolved or captured, then the device-wide lane only for the inventory write.
fn fork_in_lanes(
    gate: &super::operation_gate::OperationGate,
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    assignments: &dyn ForkAssignments,
    computer_id: &str,
    checkpoint_id: Option<&str>,
    new_name: &str,
    macos_names: &dyn Fn() -> Vec<String>,
) -> Result<(), RuntimeError> {
    use super::operation_gate::OperationKind;
    let computer_name = computer_configuration(paths, computer_id)?
        .name()
        .to_owned();
    let fork = {
        let guard = gate.kind(OperationKind::CheckpointFork).computer(
            computer_id,
            &computer_name,
            "Forking checkpoint",
        )?;
        // Fork is not cancellable; flag it slow after the capture window.
        guard.expect_within(Duration::from_secs(10 * 60));
        shutdown::ensure_accepting_operations().map_err(RuntimeError::Unavailable)?;
        fork_source(runner, paths, computer_id, checkpoint_id, new_name)?
    };
    let guard = gate
        .kind(OperationKind::CheckpointFork)
        .device("Forking checkpoint")?;
    guard.expect_within(Duration::from_secs(60));
    shutdown::ensure_accepting_operations().map_err(RuntimeError::Unavailable)?;
    // Held until the fork is in the inventory, so a macOS creation sees the name.
    let _reservation = reserve_fork_name(new_name, macos_names)?;
    fork_commit(runner, paths, assignments, &fork, new_name)
}

/// Reserves a fork's name against the names of macOS computers.
fn reserve_fork_name(
    new_name: &str,
    macos_names: &dyn Fn() -> Vec<String>,
) -> Result<crate::computer_names::Reservation, RuntimeError> {
    crate::computer_names::reserve(&[new_name.to_string()], macos_names)
        .map_err(RuntimeError::Invalid)
}

#[tauri::command]
pub async fn fork_checkpoint(
    app: AppHandle,
    computer_id: String,
    checkpoint_id: Option<String>,
    new_name: String,
) -> Result<ApplicationSource, String> {
    crate::runtime_migration::ensure_ready_async(&app).await?;
    let worker_app = app.clone();
    super::operation_gate::spawn_blocking(move || {
        let paths = runtime_paths(&worker_app)?;
        let result = fork_in_lanes(
            &OPERATIONS,
            &ProcessRunner,
            &paths,
            &AppForkAssignments(&worker_app),
            &computer_id,
            checkpoint_id.as_deref(),
            &new_name,
            &|| crate::macos_computers::names(&worker_app),
        )
        .and_then(|_| application_state_response(&worker_app, &paths));
        let _ = worker_app.emit("silo://application-state-changed", ());
        result.map_err(|error| error.to_string())
    })
    .await
    .map_err(|_| {
        "Silo could not finish creating the fork. Refresh the computer list before trying again."
            .to_string()
    })?
}

/// Persist a failure record without letting a save failure replace the real error.
fn save_failure(
    paths: &RuntimePaths,
    id: &str,
    record: &Record,
    failure: RuntimeError,
) -> RuntimeError {
    if save(paths, id, record).is_ok() {
        return failure;
    }
    let context = " Checkpoint history could not be updated.";
    match failure {
        RuntimeError::Invalid(message) => RuntimeError::Invalid(format!("{message}{context}")),
        RuntimeError::Unavailable(message) => {
            RuntimeError::Unavailable(format!("{message}{context}"))
        }
        RuntimeError::Malformed(message) => RuntimeError::Malformed(format!("{message}{context}")),
        other => other,
    }
}

/// Leave the `capturing` phase after a failure that happened before the recovery
/// checkpoint existed. The original computer is untouched, so the journal is cleared unless
/// the computer stays paused (a retry then continues from the paused computer).
fn abandon_capture(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    computer_id: &str,
    record: &mut Record,
    computer_name: &str,
    failure: RuntimeError,
) -> RuntimeError {
    let (settled, failure) =
        settle_capture_failure(runner, paths, computer_id, computer_name, failure);
    if settled {
        // The recovery point was never recorded; remove it if the capture published it.
        if let Some(journal) = record.restore_journal.take() {
            let group = record
                .snapshot_group
                .clone()
                .unwrap_or_else(|| computer_name.to_owned());
            discard_failed_capture(
                runner,
                paths,
                computer_id,
                (group, journal.recovery_checkpoint.native_id().to_owned()),
            );
        }
    }
    record.checkpoint_operation = Some(Operation {
        kind: "restore".into(),
        status: "failed".into(),
        stage: "Recovery checkpoint failed".into(),
        error: Some(failure.to_string()),
    });
    save_failure(paths, computer_id, record, failure)
}

fn restore_with(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    computer_id: &str,
    checkpoint_id: &str,
) -> Result<(), RuntimeError> {
    restore_steps(runner, paths, computer_id, checkpoint_id).map_err(|failure| {
        // Every error after the journal save must leave a failed status with the
        // real cause, not a "running" operation later read as an interrupted one.
        let Ok(mut record) = load(paths, computer_id) else {
            return failure;
        };
        let Some(operation) = record
            .checkpoint_operation
            .as_ref()
            .filter(|operation| operation.kind == "restore" && operation.status == "running")
        else {
            return failure;
        };
        record.checkpoint_operation = Some(Operation {
            kind: "restore".into(),
            status: "failed".into(),
            stage: operation.stage.clone(),
            error: Some(failure.to_string()),
        });
        save_failure(paths, computer_id, &record, failure)
    })
}

fn restore_steps(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    computer_id: &str,
    checkpoint_id: &str,
) -> Result<(), RuntimeError> {
    let configuration = computer_configuration(paths, computer_id)?;
    let mut record = load(paths, computer_id)?;
    let lineage_group = ensure_snapshot_group(paths, computer_id, configuration.name())?;
    record.snapshot_group = Some(lineage_group.clone());
    let target = record
        .checkpoints
        .iter()
        .find(|checkpoint| checkpoint.id == checkpoint_id)
        .ok_or_else(|| RuntimeError::Invalid("The selected checkpoint no longer exists.".into()))?
        .clone();
    verify_snapshot(runner, paths, &lineage_group, &target)?;
    if let Some(pending) = record.pending_checkpoint_restore.clone() {
        if record.restore_journal.is_some() {
            return Err(error(
                "A previous Restore recovery is unfinished. The pending checkpoint was preserved.",
            ));
        }
        if pending.source_computer != lineage_group
            || !valid_native_id(&pending.checkpoint_id)
            || !matches!(pending.state.as_str(), "full" | "disk")
        {
            return Err(error(
                "The pending checkpoint reference is invalid. It was preserved.",
            ));
        }
        ensure_pending_runtime_absent(runner, paths, configuration.name())?;
        snapshot_ready(
            runner,
            paths,
            &lineage_group,
            &pending.checkpoint_id,
            &pending.state,
        )?;
        let current = Checkpoint {
            id: new_checkpoint_id(&record),
            native_id: Some(pending.checkpoint_id.clone()),
            name: "Before restore".into(),
            created_at: activity_timestamp(),
            scope: pending.state,
            reason: "before-restore".into(),
        };
        record.checkpoints.insert(0, current);
        let policy = record.desired_network_policy.as_ref().ok_or_else(|| {
            error("The current network policy is missing. The pending checkpoint was preserved.")
        })?;
        current_network_args(&serde_json::json!({"network":{"policy":policy}}))?;
        record.pending_checkpoint_restore = Some(PendingRestore {
            checkpoint_id: target.native_id().to_owned(),
            source_computer: lineage_group,
            state: target.scope,
        });
        record.restore_attempted = false;
        record.restore_attempt_ran = false;
        record.restore_attempt_id = None;
        record.checkpoint_operation = None;
        return save(paths, computer_id, &record);
    }
    if record
        .restore_journal
        .as_ref()
        .is_some_and(|journal| journal.phase == "secured")
    {
        let listed = runner.run(
            paths,
            &["list".into(), "--format".into(), "json".into()],
            READ_TIMEOUT,
        )?;
        let listed: Vec<ListedSandbox> = serde_json::from_str(&listed.stdout)
            .map_err(|_| error("The runtime returned an invalid computer list."))?;
        if !listed
            .iter()
            .any(|entry| entry.name == configuration.name())
        {
            record.pending_checkpoint_restore = Some(PendingRestore {
                checkpoint_id: target.native_id().to_owned(),
                source_computer: lineage_group.clone(),
                state: target.scope,
            });
            record.restore_journal = None;
            record.restore_attempted = false;
            record.restore_attempt_ran = false;
            record.restore_attempt_id = None;
            record.checkpoint_operation = None;
            save(paths, computer_id, &record)?;
            return Ok(());
        }
    }
    let inspected = inspect_computer(runner, paths, configuration.name())?;
    ensure_managed(&inspected)?;
    if inspected
        .config
        .pointer("/labels/silo.machine-id")
        .and_then(Value::as_str)
        != Some(computer_id)
    {
        return Err(error(
            "The computer runtime identity changed. No Restore was performed.",
        ));
    }
    let policy = inspected
        .config
        .pointer("/network/policy")
        .ok_or_else(|| error("The current network policy is unavailable."))?
        .clone();
    current_network_args(&inspected.config)?;
    let prior_running = inspected.status == "Running";
    // A retry of an unfinished Restore also accepts a crashed computer: its disks are intact.
    let retrying = record.restore_journal.is_some();
    if !prior_running
        && !matches!(inspected.status.as_str(), "Paused" | "Stopped" | "Created")
        && !(retrying && inspected.status == "Crashed")
    {
        return Err(RuntimeError::Invalid(
            "Wait until the computer is running or stopped before Restore.".into(),
        ));
    }
    if let Some(existing) = &record.restore_journal {
        if existing.target_checkpoint_id != checkpoint_id {
            return Err(RuntimeError::Invalid(unfinished_restore_message(
                &record,
                configuration.name(),
            )));
        }
    } else {
        let recovery = Checkpoint {
            id: format!("c{}", &uuid::Uuid::new_v4().simple().to_string()[..31]),
            native_id: None,
            name: "Before restore".into(),
            created_at: activity_timestamp(),
            scope: if prior_running { "full" } else { "disk" }.into(),
            reason: "before-restore".into(),
        };
        record.desired_network_policy = Some(policy);
        record.restore_journal = Some(RestoreJournal {
            target_checkpoint_id: checkpoint_id.into(),
            recovery_checkpoint: recovery,
            prior_running,
            phase: "capturing".into(),
        });
        record.checkpoint_operation = Some(Operation {
            kind: "restore".into(),
            status: "running".into(),
            stage: "Creating recovery checkpoint".into(),
            error: None,
        });
        save(paths, computer_id, &record)?;
    }
    let mut journal = record.restore_journal.clone().unwrap();
    if journal.phase == "capturing" {
        if journal.prior_running
            && matches!(inspected.status.as_str(), "Stopped" | "Created" | "Crashed")
        {
            // The computer stopped before its memory was captured (for example Silo or the
            // host closed mid-capture). Only its disks remain, so secure those instead.
            journal.prior_running = false;
            journal.recovery_checkpoint.scope = "disk".into();
            record.restore_journal = Some(journal.clone());
            save(paths, computer_id, &record)?;
        }
        if journal.prior_running && inspected.status == "Running" {
            if let Err(failure) = runner.run(
                paths,
                &[
                    "pause".into(),
                    configuration.name().into(),
                    "--guest-flush".into(),
                    "required".into(),
                ],
                MUTATION_TIMEOUT,
            ) {
                return Err(abandon_capture(
                    runner,
                    paths,
                    computer_id,
                    &mut record,
                    configuration.name(),
                    failure,
                ));
            }
        }
        if journal.prior_running
            && inspect_computer(runner, paths, configuration.name())?.status != "Paused"
        {
            return Err(abandon_capture(
                runner,
                paths,
                computer_id,
                &mut record,
                configuration.name(),
                error("The computer did not remain paused. No replacement was made."),
            ));
        }
        let mut args = vec![
            "snapshot".into(),
            "create".into(),
            journal.recovery_checkpoint.id.clone(),
            "--from-sandbox".into(),
            configuration.name().into(),
            "--group".into(),
            lineage_group.clone(),
        ];
        if journal.prior_running {
            args.extend(["--full".into(), "--guest-flush".into(), "required".into()]);
        }
        args.push("--integrity".into());
        let capture = runner
            .run(paths, &args, Duration::from_secs(900))
            .and_then(|_| {
                verify_snapshot(runner, paths, &lineage_group, &journal.recovery_checkpoint)
            });
        if let Err(failure) = capture {
            return Err(abandon_capture(
                runner,
                paths,
                computer_id,
                &mut record,
                configuration.name(),
                failure,
            ));
        }
        if !record
            .checkpoints
            .iter()
            .any(|checkpoint| checkpoint.id == journal.recovery_checkpoint.id)
        {
            record
                .checkpoints
                .insert(0, journal.recovery_checkpoint.clone());
        }
        journal.phase = "secured".into();
        record.restore_journal = Some(journal.clone());
        record.checkpoint_operation = Some(Operation {
            kind: "restore".into(),
            status: "running".into(),
            stage: "Replacing computer generation".into(),
            error: None,
        });
        save(paths, computer_id, &record)?;
    }
    let current = inspect_computer(runner, paths, configuration.name())?;
    if current.status == "Running" {
        return Err(error(
            "The computer resumed after its recovery checkpoint. Restore was stopped to protect later writes.",
        ));
    }
    if current.status == "Paused" {
        runner.run(
            paths,
            &["stop".into(), "--force".into(), configuration.name().into()],
            STOP_TIMEOUT,
        )?;
    }
    let stopped = inspect_computer(runner, paths, configuration.name())?;
    if !matches!(stopped.status.as_str(), "Stopped" | "Created" | "Crashed") {
        return Err(error(
            "The original computer did not stop. Its recovery checkpoint was preserved.",
        ));
    }
    crate::ssh_access::close_computer(configuration.name());
    crate::desktop_viewer::close_computer(configuration.name());
    runner.run(
        paths,
        &[
            "remove".into(),
            "--quiet".into(),
            configuration.name().into(),
        ],
        STOP_TIMEOUT,
    )?;
    record.pending_checkpoint_restore = Some(PendingRestore {
        checkpoint_id: target.native_id().to_owned(),
        source_computer: lineage_group,
        state: target.scope,
    });
    record.restore_journal = None;
    record.restore_attempted = false;
    record.restore_attempt_ran = false;
    record.restore_attempt_id = None;
    record.checkpoint_operation = None;
    save(paths, computer_id, &record)
}

#[tauri::command]
pub async fn restore_checkpoint(
    app: AppHandle,
    computer_id: String,
    checkpoint_id: String,
) -> Result<ApplicationSource, String> {
    crate::runtime_migration::ensure_ready_async(&app).await?;
    let worker_app = app.clone();
    super::operation_gate::spawn_blocking(move || {
        let paths = runtime_paths(&worker_app)?;
        // Restore rewrites only this computer's runtime state and per-computer checkpoint record,
        // not the shared inventory, so it is ordered per computer by stable id.
        let computer_name = computer_configuration(&paths, &computer_id)
            .map_err(|error| error.to_string())?
            .name()
            .to_owned();
        let guard = OPERATIONS
            .kind(super::operation_gate::OperationKind::CheckpointRestore)
            .computer(&computer_id, &computer_name, "Restoring checkpoint")
            .map_err(|error| error.to_string())?;
        // Restore is deliberately not cancellable; flag it slow after the restore window.
        guard.expect_within(RESTORE_EXPECTED_DURATION);
        let _guard = guard;
        shutdown::ensure_accepting_operations()?;
        let result = restore_with(&ProcessRunner, &paths, &computer_id, &checkpoint_id)
            .and_then(|_| application_state_response(&worker_app, &paths));
        let _ = worker_app.emit("silo://application-state-changed", ());
        result.map_err(|error| error.to_string())
    })
    .await
    .map_err(|_| "Silo could not finish Restore. Open the Checkpoints tab to review recovery and retry Restore.".to_string())?
}

fn inspect_capture_source(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    computer_id: &str,
    computer_name: &str,
) -> Result<InspectedSandbox, RuntimeError> {
    let computer = inspect_computer(runner, paths, computer_name)?;
    ensure_managed(&computer)?;
    if computer.name != computer_name
        || computer
            .config
            .pointer("/labels/silo.machine-id")
            .and_then(Value::as_str)
            != Some(computer_id)
    {
        return Err(error(
            "The computer runtime identity changed. Its state was preserved.",
        ));
    }
    Ok(computer)
}

fn capture_source_recoverable(computer: &InspectedSandbox) -> bool {
    matches!(
        computer.status.as_str(),
        "Running" | "Stopped" | "Created" | "Crashed"
    )
}

/// Recover only the selected managed source. Force-stop cannot flush a paused guest,
/// but preserves its disks and logs when upstream recovery-owned suspension blocks resume.
fn settle_capture_source(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    computer_id: &str,
    computer_name: &str,
) -> Result<Option<String>, RuntimeError> {
    super::operation_gate::uncancellable(|| {
        let inspect = || inspect_capture_source(runner, paths, computer_id, computer_name);
        let computer = inspect()?;
        if capture_source_recoverable(&computer) {
            return Ok(None);
        }
        if computer.status != "Paused" {
            return Err(error(
                "The capture source is still changing state. Retry Start or Stop to recover it.",
            ));
        }
        let resume = runner.run(
            paths,
            &["resume".into(), computer_name.into()],
            MUTATION_TIMEOUT,
        );
        let resume_failure = resume.as_ref().err().map(ToString::to_string);
        let context = resume_failure
            .as_deref()
            .unwrap_or("The computer remained Paused after resume.");
        let computer = inspect().map_err(|failure| error(&format!("{context} {failure}")))?;
        if capture_source_recoverable(&computer) {
            return Ok(resume_failure);
        }
        if computer.status != "Paused" {
            return Err(error(&format!("{context} The capture source is still changing state. Retry Start or Stop to recover it.")));
        }
        let stop = runner.run(
            paths,
            &["stop".into(), "--force".into(), computer_name.into()],
            STOP_TIMEOUT,
        );
        let stop_failure = stop.as_ref().err().map(ToString::to_string);
        let context = match stop_failure {
            Some(failure) => format!("{context} {failure}"),
            None => context.to_owned(),
        };
        let computer = inspect().map_err(|failure| error(&format!("{context} {failure}")))?;
        if capture_source_recoverable(&computer) {
            let action = if computer.status == "Running" {
                ""
            } else {
                " Start it to continue."
            };
            return Ok(Some(format!(
                "{context} The computer is now {}; its disks were preserved.{action}",
                computer.status
            )));
        }
        Err(error(&format!("{context} The computer could not be resumed or stopped. Its capture recovery was kept. Retry Start or Stop.")))
    })
}

fn settle_capture_failure(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    computer_id: &str,
    computer_name: &str,
    failure: RuntimeError,
) -> (bool, RuntimeError) {
    match settle_capture_source(runner, paths, computer_id, computer_name) {
        Ok(None) => (true, failure),
        Ok(Some(context)) => (true, error(&format!("{failure} {context}"))),
        Err(recovery) => (false, error(&format!("{failure} {recovery}"))),
    }
}

/// The in-flight full member also owns source recovery until settlement succeeds.
/// This lets ordinary lifecycle controls recover a capture without a Restore journal.
pub(super) fn recover_paused_capture(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    computer_id: &str,
    computer_name: &str,
) -> Result<bool, RuntimeError> {
    let record = load(paths, computer_id)?;
    if !record
        .inflight_checkpoint
        .as_ref()
        .is_some_and(|capture| capture.scope == "full")
    {
        return Ok(false);
    }
    settle_capture_source(runner, paths, computer_id, computer_name).map(|_| true)
}

fn release_paused(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    computer_id: &str,
    computer_name: &str,
) -> bool {
    settle_capture_source(runner, paths, computer_id, computer_name).is_ok()
}

fn restore_target_name(record: &Record, journal: &RestoreJournal) -> String {
    record
        .checkpoints
        .iter()
        .find(|checkpoint| checkpoint.id == journal.target_checkpoint_id)
        .map_or_else(
            || "the selected checkpoint".into(),
            |checkpoint| format!("“{}”", checkpoint.name),
        )
}

/// Why a computer with an unfinished Restore cannot take another action, naming the checkpoint.
fn unfinished_restore_message(record: &Record, name: &str) -> String {
    match &record.restore_journal {
        Some(journal) => {
            let target = restore_target_name(record, journal);
            format!("The Restore of {name} to {target} is unfinished. Retry it, or abandon it in Checkpoints, first.")
        }
        None => format!("The Restore of {name} is unfinished. Retry it from Checkpoints first."),
    }
}

/// The message for Start, Stop or another action refused while a computer waits for an
/// explicit Start from a checkpoint or has an unfinished Restore (E-08).
pub(super) fn explicit_start_message(paths: &RuntimePaths, id: &str, name: &str) -> String {
    match load(paths, id) {
        Ok(record) if record.restore_journal.is_some() => unfinished_restore_message(&record, name),
        _ => format!("{name} starts from a checkpoint first. Use Start on its page."),
    }
}

/// An unfinished Restore, exposed in the computer view so it can be retried or abandoned.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct UnfinishedRestore {
    /// Silo id of the checkpoint being restored.
    checkpoint_id: String,
    checkpoint_name: Option<String>,
    /// "capturing" before the recovery checkpoint was saved, then "secured".
    phase: String,
}

pub(super) fn view_unfinished_restore(record: &Record) -> Option<UnfinishedRestore> {
    let journal = record.restore_journal.as_ref()?;
    Some(UnfinishedRestore {
        checkpoint_id: journal.target_checkpoint_id.clone(),
        checkpoint_name: record
            .checkpoints
            .iter()
            .find(|checkpoint| checkpoint.id == journal.target_checkpoint_id)
            .map(|checkpoint| checkpoint.name.clone()),
        phase: journal.phase.clone(),
    })
}

/// Give up an unfinished Restore while the original computer still exists: it keeps its current
/// state and is resumed (or stopped) if the Restore left it paused. A recovery checkpoint
/// already saved stays in the history; one only partly captured is removed (E-05).
fn abandon_restore_with(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    computer_id: &str,
) -> Result<(), RuntimeError> {
    let configuration = computer_configuration(paths, computer_id)?;
    let mut record = load(paths, computer_id)?;
    let Some(journal) = record.restore_journal.clone() else {
        return Err(RuntimeError::Invalid(format!(
            "{} has no unfinished Restore.",
            configuration.name()
        )));
    };
    let listed = runner.run(
        paths,
        &["list".into(), "--format".into(), "json".into()],
        READ_TIMEOUT,
    )?;
    let listed: Vec<ListedSandbox> = serde_json::from_str(&listed.stdout)
        .map_err(|_| error("The runtime returned an invalid computer list."))?;
    if !listed
        .iter()
        .any(|entry| entry.name == configuration.name())
    {
        return Err(RuntimeError::Invalid(format!(
            "{name} was already replaced by this Restore. Start {name} to finish it, or Restore another checkpoint.",
            name = configuration.name()
        )));
    }
    let inspected = inspect_computer(runner, paths, configuration.name())?;
    ensure_managed(&inspected)?;
    if inspected
        .config
        .pointer("/labels/silo.machine-id")
        .and_then(Value::as_str)
        != Some(computer_id)
    {
        return Err(error(
            "The computer runtime identity changed. The Restore was not abandoned.",
        ));
    }
    if inspected.status == "Paused"
        && !release_paused(runner, paths, configuration.id(), configuration.name())
    {
        return Err(error(&format!(
            "{} could not be resumed or stopped. The unfinished Restore was kept.",
            configuration.name()
        )));
    }
    if journal.phase == "capturing" {
        let group = record
            .snapshot_group
            .clone()
            .unwrap_or_else(|| configuration.name().to_owned());
        discard_failed_capture(
            runner,
            paths,
            computer_id,
            (group, journal.recovery_checkpoint.native_id().to_owned()),
        );
    }
    record.restore_journal = None;
    record.checkpoint_operation = None;
    save(paths, computer_id, &record)
}

#[tauri::command]
pub async fn abandon_restore(
    app: AppHandle,
    computer_id: String,
) -> Result<ApplicationSource, String> {
    crate::runtime_migration::ensure_ready_async(&app).await?;
    let worker_app = app.clone();
    super::operation_gate::spawn_blocking(move || {
        let paths = runtime_paths(&worker_app)?;
        let computer_name = computer_configuration(&paths, &computer_id)
            .map_err(|error| error.to_string())?
            .name()
            .to_owned();
        let guard = OPERATIONS
            .kind(super::operation_gate::OperationKind::CheckpointRestore)
            .computer(&computer_id, &computer_name, "Abandoning Restore")
            .map_err(|error| error.to_string())?;
        guard.expect_within(std::time::Duration::from_secs(5 * 60));
        let _guard = guard;
        shutdown::ensure_accepting_operations()?;
        let result = abandon_restore_with(&ProcessRunner, &paths, &computer_id)
            .and_then(|_| application_state_response(&worker_app, &paths));
        let _ = worker_app.emit("silo://application-state-changed", ());
        result.map_err(|error| error.to_string())
    })
    .await
    .map_err(|_| {
        "Silo could not abandon Restore. Open the Checkpoints tab to review recovery and retry."
            .to_string()
    })?
}

/// Quit stops computers gracefully, which a paused computer cannot take. Release a computer an unfinished
/// Restore left paused; the Restore itself stays unfinished and can be retried (E-05).
pub(crate) fn release_paused_restore(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    configuration: &ComputerConfiguration,
) {
    if load(paths, configuration.id()).is_ok_and(|record| record.restore_journal.is_some())
        && inspect_computer(runner, paths, configuration.name())
            .is_ok_and(|computer| computer.status == "Paused")
    {
        release_paused(runner, paths, configuration.id(), configuration.name());
    }
}

/// Everything a deletion decision reads, gathered once.
struct Survey {
    uses: HashMap<native::Key, Vec<native::Use>>,
    inventory: Vec<native::Member>,
    positions: HashMap<String, String>,
}

fn survey(runner: &dyn RuntimeRunner, paths: &RuntimePaths) -> Result<Survey, RuntimeError> {
    let metadata = read_metadata(&paths.metadata)?;
    let uses = native::uses(paths, &metadata)?;
    let inventory = native::inventory(runner, paths)?;
    let positions = native::lineage_positions(runner, paths)?;
    Ok(Survey {
        uses,
        inventory,
        positions,
    })
}

/// What deleting one checkpoint entry does to native data.
enum Deletion {
    /// Only the entry goes: another entry of the same computer shares its member, or the
    /// member is already gone.
    EntryOnly,
    /// The entry goes and this member is removed.
    Remove(native::Member),
}

fn quoted_list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [first, second] => format!("{first} and {second}"),
        [first, rest @ ..] => format!("{first}, {}", quoted_list(rest)),
    }
}

fn use_description(computer: &str, purpose: &native::Purpose) -> String {
    match purpose {
        native::Purpose::Checkpoint(_, label) => {
            format!("{computer}’s checkpoint “{label}” shares the same saved state.")
        }
        native::Purpose::PendingStart => {
            format!("{computer} starts from it the next time it starts.")
        }
        native::Purpose::Capturing | native::Purpose::RestoreRecovery => {
            format!("{computer} is using it for an unfinished operation.")
        }
    }
}

/// The reason a checkpoint cannot be deleted, in Silo's words.
fn blocker_message(
    survey: &Survey,
    computer_id: &str,
    computer: &str,
    blocker: &native::Blocker,
) -> String {
    match blocker {
        native::Blocker::Used(uses) => {
            let mut names: Vec<String> = uses.iter().map(|used| used.computer.clone()).collect();
            names.sort();
            names.dedup();
            format!("Used by {}. {}", quoted_list(&names), use_description(&uses[0].computer, &uses[0].purpose))
        }
        native::Blocker::Lineage(owner) if owner == computer => format!(
            "{computer}’s next checkpoint and export build on this one, so it can’t be deleted while it is the latest state {computer} builds on."
        ),
        native::Blocker::Lineage(owner) => format!(
            "Used by {owner}. {owner} was started from this checkpoint and still builds on it."
        ),
        native::Blocker::Children(children) => {
            let mut own = Vec::new();
            let mut others = Vec::new();
            for child in children {
                let Some(key) = child.key() else { continue };
                for used in survey.uses.get(&key).into_iter().flatten() {
                    if let native::Purpose::Checkpoint(_, label) = &used.purpose {
                        if used.computer_id == computer_id {
                            own.push(format!("“{label}”"));
                        } else {
                            others.push(format!("{}’s checkpoint “{label}”", used.computer));
                        }
                    }
                }
            }
            own.sort();
            own.dedup();
            others.sort();
            others.dedup();
            if !own.is_empty() {
                format!("{} was saved after this checkpoint and builds on it. Delete {} first.", quoted_list(&own), if own.len() == 1 { "it" } else { "them" })
            } else if !others.is_empty() {
                format!("{} builds on this checkpoint. Delete {} first.", quoted_list(&others), if others.len() == 1 { "it" } else { "them" })
            } else {
                "A later saved state, such as an export, builds on this checkpoint, so it can’t be deleted yet.".into()
            }
        }
    }
}

/// Decide what deleting `checkpoint` from `record` would do, or why it is not allowed.
fn deletion(
    survey: &Survey,
    computer_id: &str,
    computer: &str,
    record: &Record,
    checkpoint: &Checkpoint,
) -> Result<Deletion, String> {
    if record.restore_journal.is_some() {
        return Err(
            "Finish or abandon the unfinished Restore before deleting a checkpoint.".into(),
        );
    }
    let group = record
        .snapshot_group
        .clone()
        .unwrap_or_else(|| computer.to_owned());
    let key: native::Key = (group, checkpoint.native_id().to_owned());
    if record
        .pending_checkpoint_restore
        .as_ref()
        .is_some_and(|pending| pending.source_computer == key.0 && pending.checkpoint_id == key.1)
    {
        return Err(format!(
            "{computer} starts from this checkpoint the next time it starts. Start {computer} first."
        ));
    }
    let this_entry = native::Purpose::Checkpoint(checkpoint.id.clone(), checkpoint.name.clone());
    let others: Vec<native::Use> = survey
        .uses
        .get(&key)
        .into_iter()
        .flatten()
        .filter(|used| !(used.computer_id == computer_id && used.purpose == this_entry))
        .cloned()
        .collect();
    let elsewhere: Vec<native::Use> = others
        .iter()
        .filter(|used| used.computer_id != computer_id)
        .cloned()
        .collect();
    if !elsewhere.is_empty() {
        return Err(blocker_message(
            survey,
            computer_id,
            computer,
            &native::Blocker::Used(elsewhere),
        ));
    }
    if !others.is_empty() {
        return Ok(Deletion::EntryOnly);
    }
    let Some(member) = survey
        .inventory
        .iter()
        .find(|member| member.key().as_ref() == Some(&key))
    else {
        return Ok(Deletion::EntryOnly);
    };
    let mut uses = survey.uses.clone();
    uses.remove(&key);
    let plan = native::plan(
        &survey.inventory,
        &HashSet::from([key]),
        &uses,
        &survey.positions,
    );
    if let Some((_, blocker)) = plan.kept.first() {
        return Err(blocker_message(survey, computer_id, computer, blocker));
    }
    Ok(Deletion::Remove(member.clone()))
}

/// Delete one checkpoint: remove its native member unless something still needs it,
/// then drop the entry. Never passes `--force`; MicroSandbox's own guards stay in force.
fn delete_checkpoint_with(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    computer_id: &str,
    checkpoint_id: &str,
) -> Result<(), RuntimeError> {
    let configuration = computer_configuration(paths, computer_id)?;
    let _ = ensure_snapshot_group(paths, computer_id, configuration.name())?;
    let mut record = load(paths, computer_id)?;
    let checkpoint = record
        .checkpoints
        .iter()
        .find(|checkpoint| checkpoint.id == checkpoint_id)
        .ok_or_else(|| RuntimeError::Invalid("The selected checkpoint no longer exists.".into()))?
        .clone();
    let survey = survey(runner, paths)?;
    match deletion(
        &survey,
        computer_id,
        configuration.name(),
        &record,
        &checkpoint,
    )
    .map_err(RuntimeError::Invalid)?
    {
        Deletion::EntryOnly => {}
        Deletion::Remove(member) => {
            if let Some((_, failure)) = native::execute(runner, paths, &survey.inventory, &[member])
                .into_iter()
                .next()
            {
                return Err(error(&format!(
                    "The checkpoint could not be deleted; it was kept. {}",
                    safe_activity_error(&failure)
                )));
            }
        }
    }
    record.checkpoints.retain(|entry| entry.id != checkpoint_id);
    if record
        .checkpoint_operation
        .as_ref()
        .is_some_and(|operation| operation.status == "failed")
    {
        record.checkpoint_operation = None;
    }
    save(paths, computer_id, &record)?;
    if let Err(failure) = retry_deleted_snapshots(runner, paths) {
        eprintln!("Previously deleted checkpoint data was kept: {failure}");
    }
    Ok(())
}

#[tauri::command]
pub async fn delete_checkpoint(
    app: AppHandle,
    computer_id: String,
    checkpoint_id: String,
) -> Result<ApplicationSource, String> {
    crate::runtime_migration::ensure_ready_async(&app).await?;
    let worker_app = app.clone();
    super::operation_gate::spawn_blocking(move || {
        let paths = runtime_paths(&worker_app)?;
        // A deleted child can release a deleted source's member in another computer's
        // lineage group. Serialize the shared cleanup journal and native store changes.
        let guard = OPERATIONS
            .kind(super::operation_gate::OperationKind::CheckpointDelete)
            .device("Deleting checkpoint")
            .map_err(|error| error.to_string())?;
        guard.expect_within(std::time::Duration::from_secs(5 * 60));
        let _guard = guard;
        shutdown::ensure_accepting_operations()?;
        let result = delete_checkpoint_with(&ProcessRunner, &paths, &computer_id, &checkpoint_id)
            .and_then(|_| application_state_response(&worker_app, &paths));
        let _ = worker_app.emit("silo://application-state-changed", ());
        result.map_err(|error| error.to_string())
    })
    .await
    .map_err(|_| {
        "Silo could not finish deleting the checkpoint. Refresh its history before trying again."
            .to_string()
    })?
}

/// Per-checkpoint storage and whether Delete is possible, for the Checkpoints panel.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointUsage {
    /// Host allocation of this computer's checkpoints, each member counted once; `None`
    /// when any of them could not be measured.
    total_bytes: Option<u64>,
    checkpoints: Vec<CheckpointUsageEntry>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointUsageEntry {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    size_bytes: Option<u64>,
    /// Other computers that depend on this checkpoint.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    used_by: Vec<String>,
    /// Why Delete is unavailable; absent when the checkpoint can be deleted.
    #[serde(skip_serializing_if = "Option::is_none")]
    delete_blocker: Option<String>,
}

fn usage_with(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    computer_id: &str,
) -> Result<CheckpointUsage, RuntimeError> {
    let configuration = computer_configuration(paths, computer_id)?;
    let record = load(paths, computer_id)?;
    if record.checkpoints.is_empty() {
        return Ok(CheckpointUsage {
            total_bytes: Some(0),
            checkpoints: Vec::new(),
        });
    }
    let group = record
        .snapshot_group
        .clone()
        .unwrap_or_else(|| configuration.name().to_owned());
    let metadata = read_metadata(&paths.metadata)?;
    let uses = native::uses(paths, &metadata)?;
    let inventory = native::inventory(runner, paths)?;
    let native_ids: HashSet<&str> = record
        .checkpoints
        .iter()
        .map(Checkpoint::native_id)
        .collect();
    let positions = if inventory.iter().any(|member| {
        member.group.as_deref() == Some(group.as_str())
            && member
                .name
                .as_deref()
                .is_some_and(|name| native_ids.contains(name))
    }) {
        native::lineage_positions(runner, paths)?
    } else {
        HashMap::new()
    };
    let survey = Survey {
        uses,
        inventory,
        positions,
    };
    let mut counted = HashSet::new();
    let mut total = Some(0u64);
    let mut checkpoints = Vec::with_capacity(record.checkpoints.len());
    for checkpoint in &record.checkpoints {
        let key: native::Key = (group.clone(), checkpoint.native_id().to_owned());
        let member = survey
            .inventory
            .iter()
            .find(|member| member.key().as_ref() == Some(&key));
        let size_bytes = member.and_then(|member| native::artifact_bytes(paths, member));
        if counted.insert(key.clone()) {
            total = match (total, member, size_bytes) {
                (Some(sum), Some(_), Some(size)) => Some(sum.saturating_add(size)),
                (sum, None, _) => sum,
                _ => None,
            };
        }
        let mut used_by: Vec<String> = survey
            .uses
            .get(&key)
            .into_iter()
            .flatten()
            .filter(|used| used.computer_id != computer_id)
            .map(|used| used.computer.clone())
            .chain(
                member
                    .and_then(|member| survey.positions.get(&member.snapshot_id))
                    .filter(|owner| owner.as_str() != configuration.name())
                    .cloned(),
            )
            .collect();
        used_by.sort();
        used_by.dedup();
        checkpoints.push(CheckpointUsageEntry {
            id: checkpoint.id.clone(),
            size_bytes,
            used_by,
            delete_blocker: deletion(
                &survey,
                computer_id,
                configuration.name(),
                &record,
                checkpoint,
            )
            .err(),
        });
    }
    Ok(CheckpointUsage {
        total_bytes: total,
        checkpoints,
    })
}

#[tauri::command]
pub async fn read_checkpoint_usage(
    app: AppHandle,
    computer_id: String,
) -> Result<CheckpointUsage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let paths = runtime_paths(&app)?;
        usage_with(&ProcessRunner, &paths, &computer_id)
            .map_err(|error| safe_activity_error(&error))
    })
    .await
    .map_err(|_| {
        "Silo could not read checkpoint disk usage. Refresh the Storage tab and retry.".to_string()
    })?
}

/// Host allocation of one computer's checkpoints and how many it has, for the Storage tab.
/// Only lists snapshots: no computer is inspected. `None` bytes when not measurable.
pub(super) fn storage_totals(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    computer_id: &str,
    computer: &str,
) -> (Option<u64>, usize) {
    let Ok(record) = load(paths, computer_id) else {
        return (None, 0);
    };
    let count = record.checkpoints.len();
    if count == 0 {
        return (Some(0), 0);
    }
    let group = record.snapshot_group.as_deref().unwrap_or(computer);
    let Ok(inventory) = native::inventory_in_group(runner, paths, group) else {
        return (None, count);
    };
    let keys: HashSet<native::Key> = native::record_uses(&record, computer)
        .into_iter()
        .filter(|(_, purpose)| matches!(purpose, native::Purpose::Checkpoint(..)))
        .map(|(key, _)| key)
        .collect();
    let mut total = 0u64;
    for member in inventory
        .iter()
        .filter(|member| member.key().is_some_and(|key| keys.contains(&key)))
    {
        let Some(size) = native::artifact_bytes(paths, member) else {
            return (None, count);
        };
        total = total.saturating_add(size);
    }
    (Some(total), count)
}

/// Exact native members selected by a computer deletion. Keep dependency-blocked members
/// in this journal so deleting the last dependent can retry them even after the source's
/// checkpoint history is gone. Snapshot ids prevent a reused name from selecting new data.
fn cleanup_path(paths: &RuntimePaths) -> PathBuf {
    paths.metadata.with_file_name("checkpoint-cleanup.json")
}

fn load_cleanup(paths: &RuntimePaths) -> Result<Vec<native::Member>, RuntimeError> {
    let bytes = match fs::read(cleanup_path(paths)) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(error("Checkpoint cleanup journal could not be read.")),
    };
    if bytes.len() as u64 > MAX_OUTPUT_BYTES {
        return Err(error(
            "Checkpoint cleanup journal is too large; data was preserved.",
        ));
    }
    let members: Vec<native::Member> = serde_json::from_slice(&bytes)
        .map_err(|_| error("Checkpoint cleanup journal is invalid; data was preserved."))?;
    if members.iter().any(|member| {
        member
            .key()
            .is_none_or(|(group, name)| !valid_snapshot_group(&group) || !valid_native_id(&name))
            || member.snapshot_id.is_empty()
    }) {
        return Err(error(
            "Checkpoint cleanup journal is invalid; data was preserved.",
        ));
    }
    Ok(members)
}

fn save_cleanup(paths: &RuntimePaths, members: &[native::Member]) -> Result<(), RuntimeError> {
    let parent = paths
        .metadata
        .parent()
        .ok_or_else(|| error("Checkpoint cleanup journal has no folder."))?;
    fs::create_dir_all(parent)
        .map_err(|_| error("Checkpoint cleanup journal could not be saved."))?;
    let mut file = tempfile::NamedTempFile::new_in(parent)
        .map_err(|_| error("Checkpoint cleanup journal could not be saved."))?;
    serde_json::to_writer(&mut file, members)
        .map_err(|_| error("Checkpoint cleanup journal could not be encoded."))?;
    file.as_file()
        .sync_all()
        .map_err(|_| error("Checkpoint cleanup journal could not be synced."))?;
    file.persist(cleanup_path(paths))
        .map_err(|_| error("Checkpoint cleanup journal could not be saved."))?;
    File::open(parent)
        .and_then(|folder| folder.sync_all())
        .map_err(|_| error("Checkpoint cleanup journal could not be synced."))
}

fn retry_deleted_snapshots(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
) -> Result<(), RuntimeError> {
    let queued = load_cleanup(paths)?;
    if queued.is_empty() {
        return Ok(());
    }
    let survey = survey(runner, paths)?;
    let present: Vec<native::Member> = queued
        .into_iter()
        .filter(|entry| {
            survey.inventory.iter().any(|member| {
                member.snapshot_id == entry.snapshot_id && member.key() == entry.key()
            })
        })
        .collect();
    let candidates = present.iter().filter_map(native::Member::key).collect();
    let plan = native::plan(
        &survey.inventory,
        &candidates,
        &survey.uses,
        &survey.positions,
    );
    let failures = native::execute(runner, paths, &survey.inventory, &plan.remove);
    let removed: HashSet<&str> = plan
        .remove
        .iter()
        .filter(|member| {
            !failures
                .iter()
                .any(|(failed, _)| failed.snapshot_id == member.snapshot_id)
        })
        .map(|member| member.snapshot_id.as_str())
        .collect();
    let remaining: Vec<_> = present
        .into_iter()
        .filter(|member| !removed.contains(member.snapshot_id.as_str()))
        .collect();
    save_cleanup(paths, &remaining)?;
    if let Some((_, failure)) = failures.into_iter().next() {
        return Err(failure);
    }
    Ok(())
}

/// Called after a computer leaves the configured inventory and runtime. Journal its
/// members before removing them, then retry prior deletions released by this deletion.
/// Failure never loses the source history: the caller clears it only after journaling.
pub(crate) fn remove_deleted_snapshots(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    computer_id: &str,
    computer: &str,
) -> Result<(), RuntimeError> {
    let record = load(paths, computer_id)?;
    let referenced: Vec<native::Key> = native::record_uses(&record, computer)
        .into_iter()
        .map(|(key, _)| key)
        .collect();
    if referenced.is_empty() && record.snapshot_group.is_none() {
        if let Err(failure) = retry_deleted_snapshots(runner, paths) {
            eprintln!("Previously deleted checkpoint data was kept: {failure}");
        }
        return Ok(());
    }
    let group = record.snapshot_group.as_deref().unwrap_or(computer);
    let mut queued = load_cleanup(paths)?;
    let inventory = native::inventory(runner, paths)?;
    for member in inventory {
        let Some(key) = member.key() else { continue };
        if (referenced.contains(&key) || (key.0 == group && native::silo_member(&key)))
            && !queued
                .iter()
                .any(|entry| entry.snapshot_id == member.snapshot_id && entry.key() == member.key())
        {
            queued.push(member);
        }
    }
    save_cleanup(paths, &queued)?;
    // The durable journal keeps failed removals; the computer deletion can finish.
    if let Err(failure) = retry_deleted_snapshots(runner, paths) {
        eprintln!("Kept checkpoint data of deleted computer {computer}: {failure}");
    }
    Ok(())
}

/// Remove the member a failed capture may have published, unless something already
/// builds on it. Returns true when it is gone. Runs even after a cancel.
pub(crate) fn discard_failed_capture(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    computer_id: &str,
    key: native::Key,
) -> bool {
    super::operation_gate::uncancellable(|| {
        let Ok(inventory) = native::inventory(runner, paths) else {
            return false;
        };
        if !inventory
            .iter()
            .any(|member| member.key().as_ref() == Some(&key))
        {
            return true;
        }
        let Ok(mut survey) = survey(runner, paths) else {
            return false;
        };
        // This capture's own in-progress reference is the one being discarded.
        if let Some(uses) = survey.uses.get_mut(&key) {
            uses.retain(|used| {
                !(used.computer_id == computer_id
                    && matches!(
                        used.purpose,
                        native::Purpose::Capturing | native::Purpose::RestoreRecovery
                    ))
            });
        }
        let plan = native::plan(
            &survey.inventory,
            &HashSet::from([key]),
            &survey.uses,
            &survey.positions,
        );
        !plan.remove.is_empty()
            && plan.kept.is_empty()
            && native::execute(runner, paths, &survey.inventory, &plan.remove).is_empty()
    })
}

pub(crate) struct Recovery {
    pub(super) unresolved: HashMap<String, RuntimeError>,
    pub(super) cleanup_error: Option<RuntimeError>,
}

/// Recovery selects only members named in an unfinished capture or deletion journal.
/// Unknown dependencies preserve native data without blocking independent owners.
pub(crate) fn recover_interrupted(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
) -> Result<Recovery, RuntimeError> {
    let metadata = read_metadata(&paths.metadata)?;
    let mut recovery = Recovery {
        unresolved: HashMap::new(),
        cleanup_error: retry_deleted_snapshots(runner, paths).err(),
    };
    for configuration in metadata.computers.iter() {
        let result = (|| {
            let mut record = load(paths, configuration.id())?;
            let Some(inflight) = record.inflight_checkpoint.clone() else {
                return Ok(());
            };
            let source_recovery = if inflight.scope == "full" {
                settle_capture_source(runner, paths, configuration.id(), configuration.name())?
            } else {
                None
            };
            let group = record
                .snapshot_group
                .clone()
                .unwrap_or_else(|| configuration.name().to_owned());
            if !discard_failed_capture(
                runner,
                paths,
                configuration.id(),
                (group, inflight.native_id().to_owned()),
            ) {
                return Err(error("Interrupted checkpoint data is still in use or could not be removed. It was preserved."));
            }
            record.inflight_checkpoint = None;
            let mut failure = record
                .checkpoint_operation
                .as_ref()
                .filter(|operation| operation.kind == "capture" && operation.status == "failed")
                .and_then(|operation| operation.error.clone())
                .unwrap_or_else(|| {
                    "Silo closed before the checkpoint finished. Create the checkpoint again."
                        .into()
                });
            if let Some(source_recovery) = source_recovery {
                failure.push(' ');
                failure.push_str(&source_recovery);
            }
            record.checkpoint_operation = Some(Operation {
                kind: "capture".into(),
                status: "failed".into(),
                stage: "Checkpoint interrupted".into(),
                error: Some(failure),
            });
            save(paths, configuration.id(), &record)
        })();
        if let Err(failure) = result {
            recovery
                .unresolved
                .insert(configuration.id().to_owned(), failure);
        }
    }
    Ok(recovery)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_migrated_checkpoint_record_loads() {
        let migrated = crate::runtime_migration::vocabulary_tests::migrated_installation();
        let record = load(
            &migrated.runtime_paths(),
            crate::runtime_migration::vocabulary_tests::ID,
        )
        .unwrap();
        let pending = record.pending_checkpoint_restore.unwrap();
        assert_eq!(pending.source_computer, "dev");
        assert_eq!(pending.state, "full");
    }

    mod failed_capture_tests;

    const ID: &str = "00000000-0000-4000-8000-000000000001";

    #[test]
    fn full_snapshot_satisfies_a_disk_only_restore_but_not_the_reverse() {
        let _test_state = crate::test_support::global_state();
        use crate::test_support::runner::{ExpectedCommand, ScriptedRunner};
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        for (available, requested, ready) in [
            ("full", "disk", true),
            ("full", "full", true),
            ("disk", "disk", true),
            ("disk", "full", false),
        ] {
            let runner = ScriptedRunner::new([ExpectedCommand::ok(
                ["snapshot", "list", "--format", "json"],
                serde_json::json!([{"group":"g", "name":"c", "scope":available, "availability":"ready"}]).to_string(),
            ).with_timeout(READ_TIMEOUT)]);
            assert_eq!(
                snapshot_ready(&runner, &paths, "g", "c", requested).is_ok(),
                ready
            );
            runner.assert_finished();
        }
    }

    #[test]
    fn imported_snapshot_intent_accepts_native_selectors_but_rejects_paths() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let group = "silo-import-6b79cf8f70b34f2d93d13eeb3798a8b9";
        let member = "silo-backup-0-330418-1790360984903";
        import_pending_restore(&paths, ID, group, member).unwrap();
        let pending = load(&paths, ID)
            .unwrap()
            .pending_checkpoint_restore
            .unwrap();
        assert_eq!(pending.source_computer, group);
        assert_eq!(
            load(&paths, ID).unwrap().snapshot_group.as_deref(),
            Some(group)
        );
        assert_eq!(pending.checkpoint_id, member);
        assert_eq!(pending.state, "disk");
        for (bad_group, bad_member) in [
            ("../silo-import-6b79cf8f70b34f2d93d13eeb3798a8b9", member),
            (group, "../silo-backup-0-330418-1790360984903"),
            (group, "silo-backup-0:330418-1790360984903"),
            ("silo-import-not-a-uuid", member),
        ] {
            assert!(import_pending_restore(&paths, ID, bad_group, bad_member).is_err());
        }
    }

    #[test]
    fn imported_snapshot_intent_accepts_a_checkpoint_native_member() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let group = "silo-import-6b79cf8f70b34f2d93d13eeb3798a8b9";
        // A `c` + 31 hex-digit member, as produced by a checkpoint export.
        let member = "c0123456789abcdef0123456789abcde";
        import_pending_restore(&paths, ID, group, member).unwrap();
        let pending = load(&paths, ID)
            .unwrap()
            .pending_checkpoint_restore
            .unwrap();
        assert_eq!(pending.checkpoint_id, member);
        assert_eq!(pending.state, "disk");
        // Uppercase hex is not a form Silo produces and stays rejected.
        assert!(
            import_pending_restore(&paths, ID, group, "c0123456789ABCDEF0123456789abcde").is_err()
        );
        // A bare word that is neither a backup member nor a checkpoint id is rejected.
        assert!(import_pending_restore(&paths, ID, group, "imported-member").is_err());
    }

    #[test]
    fn export_preflight_does_not_migrate_lineage_while_checkpoint_work_holds_gate() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(
            &paths.metadata,
            &ComputerConfigurationRequest {
                schema_version: 1,
                computers: vec![computer_configuration()],
            },
        )
        .unwrap();
        let checkpoint_id = "c000000000000000000000000000001";
        let mut record = Record::default();
        record.checkpoints.push(Checkpoint {
            id: checkpoint_id.into(),
            native_id: None,
            name: "Legacy milestone".into(),
            created_at: 1,
            scope: "disk".into(),
            reason: "manual".into(),
        });
        save(&paths, ID, &record).unwrap();
        let before = fs::read(path(&paths, ID)).unwrap();
        let checkpoint_guard = super::super::OPERATIONS
            .kind(super::super::operation_gate::OperationKind::CheckpointCapture)
            .computer(ID, "dev", "Creating checkpoint")
            .unwrap();
        let (result, during_preflight) = std::thread::scope(|scope| {
            let (send, receive) = std::sync::mpsc::channel();
            let preflight_paths = &paths;
            let worker = scope.spawn(move || {
                send.send(export_source(preflight_paths, ID, checkpoint_id))
                    .unwrap();
            });
            let result = receive.recv_timeout(Duration::from_secs(5));
            let during_preflight = fs::read(path(&paths, ID)).unwrap();
            drop(checkpoint_guard);
            worker.join().unwrap();
            (result, during_preflight)
        });
        let (group, member, scope, name) = result.unwrap().unwrap();
        assert_eq!(group, "dev");
        assert_eq!(member, checkpoint_id);
        assert_eq!(scope, "disk");
        assert_eq!(name, "Legacy milestone");
        assert!(
            during_preflight == before,
            "Export preflight rewrote checkpoint history while checkpoint work held the gate."
        );
        assert_eq!(load(&paths, ID).unwrap().snapshot_group, None);

        let _export_guard = super::super::OPERATIONS
            .kind(super::super::operation_gate::OperationKind::Export)
            .device("Exporting computer")
            .unwrap();
        assert_eq!(ensure_snapshot_group(&paths, ID, "dev").unwrap(), "dev");
        assert_eq!(
            load(&paths, ID).unwrap().snapshot_group.as_deref(),
            Some("dev")
        );
    }

    #[test]
    fn failed_computer_use_cleanup_keeps_checkpoint_history_for_retry() {
        let _test_state = crate::test_support::global_state();
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        save(&paths, ID, &Record::default()).unwrap();
        crate::computer_use::set_approval(&paths, ID, crate::computer_use::Approval::Auto).unwrap();
        let policy_directory = paths.metadata.with_file_name("computer-use");
        fs::set_permissions(&policy_directory, fs::Permissions::from_mode(0o500)).unwrap();
        let result = forget_removed(&paths, ID);
        fs::set_permissions(&policy_directory, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(result.is_err(), "cleanup reported success: {result:?}");
        assert!(path(&paths, ID).is_file(), "cleanup lost its retry record");
        assert_eq!(
            crate::computer_use::settings(&paths, ID).approval,
            crate::computer_use::Approval::Auto
        );
        forget_removed(&paths, ID).unwrap();
        assert!(!path(&paths, ID).exists());
        assert!(!policy_directory.join(format!("{ID}.json")).exists());
        forget_removed(&paths, ID).unwrap();
    }

    #[test]
    fn export_source_resolves_checkpoint_member_and_rejects_unknown_or_inflight() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(
            &paths.metadata,
            &ComputerConfigurationRequest {
                schema_version: 1,
                computers: vec![computer_configuration()],
            },
        )
        .unwrap();
        let mut record = Record::default();
        record.snapshot_group = Some("dev".into());
        record.checkpoints.push(Checkpoint {
            id: "c000000000000000000000000000001".into(),
            native_id: Some("c0000000000000000000000000000aa".into()),
            name: "Milestone".into(),
            created_at: 1,
            scope: "full".into(),
            reason: "manual".into(),
        });
        save(&paths, ID, &record).unwrap();
        let (group, member, scope, name) =
            export_source(&paths, ID, "c000000000000000000000000000001").unwrap();
        assert_eq!(group, "dev");
        assert_eq!(member, "c0000000000000000000000000000aa");
        assert_eq!(scope, "full");
        assert_eq!(name, "Milestone");
        // Unknown ids are rejected.
        assert!(export_source(&paths, ID, "c000000000000000000000000000999").is_err());
        // An inflight checkpoint cannot be exported until capture finishes.
        let mut inflight = record;
        inflight.inflight_checkpoint = Some(Checkpoint {
            id: "c000000000000000000000000000002".into(),
            native_id: None,
            name: "Capturing".into(),
            created_at: 2,
            scope: "disk".into(),
            reason: "manual".into(),
        });
        save(&paths, ID, &inflight).unwrap();
        assert!(export_source(&paths, ID, "c000000000000000000000000000002").is_err());
    }

    #[test]
    fn imported_disk_snapshot_uses_native_cold_boot_without_full_checkpoint_flag() {
        let _test_state = crate::test_support::global_state();
        struct DiskRunner(Mutex<Vec<Vec<String>>>);
        impl RuntimeRunner for DiskRunner {
            fn run(
                &self,
                _paths: &RuntimePaths,
                args: &[String],
                _timeout: Duration,
            ) -> Result<CommandOutput, RuntimeError> {
                self.0.lock().unwrap().push(args.to_vec());
                let stdout = match args.first().map(String::as_str) {
                    Some("snapshot") => serde_json::json!([{
                        "group":"silo-import-6b79cf8f70b34f2d93d13eeb3798a8b9",
                        "name":"silo-backup-0-330418-1790360984903",
                        "scope":"disk", "availability":"ready"
                    }])
                    .to_string(),
                    Some("list") => "[]".into(),
                    Some("restore") => return Err(error("synthetic restore failure")),
                    // After a failed restore Silo checks whether the attempt ran.
                    Some("inspect") => return Err(error("computer not found: dev")),
                    _ => panic!("unexpected command: {args:?}"),
                };
                Ok(CommandOutput {
                    stdout,
                    stderr: String::new(),
                })
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        import_pending_restore(
            &paths,
            ID,
            "silo-import-6b79cf8f70b34f2d93d13eeb3798a8b9",
            "silo-backup-0-330418-1790360984903",
        )
        .unwrap();
        let runner = DiskRunner(Mutex::new(Vec::new()));
        assert!(start_pending(&runner, &paths, &computer_configuration())
            .unwrap_err()
            .to_string()
            .contains("synthetic restore failure"));
        let calls = runner.0.lock().unwrap();
        let restore = calls
            .iter()
            .find(|args| args.first().is_some_and(|arg| arg == "restore"))
            .unwrap();
        assert_eq!(
            restore[1],
            "silo-import-6b79cf8f70b34f2d93d13eeb3798a8b9:silo-backup-0-330418-1790360984903"
        );
        assert!(!restore
            .iter()
            .any(|arg| arg == "--disk-only" || arg == "--cow-mem"));
        assert!(restore.windows(2).any(|args| args == ["--cpus", "1"]));
        assert!(restore.windows(2).any(|args| args == ["--memory", "1G"]));
        assert_eq!(
            load(&paths, ID).unwrap().snapshot_group.as_deref(),
            Some("silo-import-6b79cf8f70b34f2d93d13eeb3798a8b9")
        );
    }

    struct ScopeProbe {
        scope: &'static str,
        probe: RestoreProbe,
    }
    impl RuntimeRunner for ScopeProbe {
        fn run(
            &self,
            paths: &RuntimePaths,
            args: &[String],
            timeout: Duration,
        ) -> Result<CommandOutput, RuntimeError> {
            let mut output = self.probe.run(paths, args, timeout)?;
            if args[0] == "snapshot" {
                let mut entries: Value = serde_json::from_str(&output.stdout).unwrap();
                entries[0]["scope"] = self.scope.into();
                output.stdout = entries.to_string();
            }
            Ok(output)
        }
    }

    #[test]
    fn checkpoint_start_selects_native_scope_and_desired_restore_mode_separately() {
        let _test_state = crate::test_support::global_state();
        for (native_scope, desired_mode, disk_only, cow_mem) in [
            ("full", "disk", true, false),
            ("disk", "disk", false, false),
            ("full", "full", false, true),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let paths = paths(&directory);
            pending_import(&paths);
            let mut record = load(&paths, ID).unwrap();
            record.pending_checkpoint_restore.as_mut().unwrap().state = desired_mode.into();
            save(&paths, ID, &record).unwrap();
            let runner = ScopeProbe {
                scope: native_scope,
                probe: RestoreProbe(Mutex::new(Vec::new())),
            };
            assert!(start_pending(&runner, &paths, &computer_configuration())
                .unwrap_err()
                .to_string()
                .contains("synthetic restore failure"));
            let calls = runner.probe.0.lock().unwrap();
            let restore = calls.iter().find(|args| args[0] == "restore").unwrap();
            assert_eq!(
                restore.iter().any(|arg| arg == "--disk-only"),
                disk_only,
                "native={native_scope}, desired={desired_mode}: {restore:?}"
            );
            assert_eq!(restore.iter().any(|arg| arg == "--cow-mem"), cow_mem);
            assert_eq!(
                restore.iter().any(|arg| arg == "--cpus"),
                desired_mode == "disk"
            );
        }
    }

    #[test]
    fn checkpoint_start_reapplies_github_environment_for_current_host_profiles() {
        let _test_state = crate::test_support::global_state();
        for scope in ["disk", "full"] {
            for authorized in [false, true] {
                let directory = tempfile::tempdir().unwrap();
                let paths = paths(&directory);
                pending_import(&paths);
                let mut record = load(&paths, ID).unwrap();
                record.pending_checkpoint_restore.as_mut().unwrap().state = scope.into();
                save(&paths, ID, &record).unwrap();
                let profile = if authorized {
                    serde_json::json!({"enabled":true,"readToken":"synthetic-current-token"})
                        .to_string()
                } else {
                    DISABLED_GITHUB_PROFILE.into()
                };
                GITHUB_PROFILES
                    .get_or_init(|| Mutex::new(HashMap::new()))
                    .lock()
                    .unwrap()
                    .insert((paths.home.clone(), "dev".into()), profile.clone());
                let runner = ScopeProbe {
                    scope,
                    probe: RestoreProbe(Mutex::new(Vec::new())),
                };
                assert!(start_pending(&runner, &paths, &computer_configuration())
                    .unwrap_err()
                    .to_string()
                    .contains("synthetic restore failure"));
                let calls = runner.probe.0.lock().unwrap();
                let restore = calls.iter().find(|args| args[0] == "restore").unwrap();
                assert!(
                    restore
                        .windows(2)
                        .any(|pair| pair == ["--env", "GH_TOKEN=$MSB_SILO_GITHUB"]),
                    "scope={scope}, authorized={authorized}: {restore:?}"
                );
                assert_eq!(github_environment(&paths, restore), profile);
                assert!(!restore
                    .iter()
                    .any(|arg| arg.contains("synthetic-current-token")));
                GITHUB_PROFILES
                    .get()
                    .unwrap()
                    .lock()
                    .unwrap()
                    .remove(&(paths.home.clone(), "dev".into()));
            }
        }
    }

    #[test]
    fn an_import_preserves_environment_until_an_identity_is_saved() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let config = serde_json::json!({"env":[
            {"key":"PROJECT_MODE","value":"portable value=with spaces"},
            {"key":"-PORTABLE_FLAG","value":"literal"},
            {"key":"GIT_AUTHOR_NAME","value":"Old Author"},
            {"key":"GIT_COMMITTER_EMAIL","value":"old@example.test"},
            {"key":"JJ_USER","value":"Old Author"},
            {"key":"GH_TOKEN","value":"synthetic-old-token"}
        ]});
        import_pending_restore_with_environment(
            &paths,
            ID,
            "silo-import-6b79cf8f70b34f2d93d13eeb3798a8b9",
            "silo-backup-0-330418-1790360984903",
            &config,
        )
        .unwrap();
        assert_eq!(load(&paths, ID).unwrap().desired_environment.len(), 5);
        let runner = RestoreProbe(Mutex::new(Vec::new()));
        assert!(start_pending(&runner, &paths, &computer_configuration())
            .unwrap_err()
            .to_string()
            .contains("synthetic restore failure"));
        let calls = runner.0.lock().unwrap();
        let restore = calls.iter().find(|args| args[0] == "restore").unwrap();
        for identity in [
            "--env=GIT_AUTHOR_NAME=Old Author",
            "--env=GIT_COMMITTER_EMAIL=old@example.test",
            "--env=JJ_USER=Old Author",
        ] {
            assert!(restore.iter().any(|arg| arg == identity));
        }
        drop(calls);
        forget_identity_environment(&paths, ID).unwrap();
        assert_eq!(load(&paths, ID).unwrap().desired_environment.len(), 2);
        let runner = RestoreProbe(Mutex::new(Vec::new()));
        assert!(start_pending(&runner, &paths, &computer_configuration()).is_err());
        let calls = runner.0.lock().unwrap();
        let restore = calls.iter().find(|args| args[0] == "restore").unwrap();
        assert!(restore
            .iter()
            .any(|arg| arg == "--env=PROJECT_MODE=portable value=with spaces"));
        assert!(restore
            .iter()
            .any(|arg| arg == "--env=-PORTABLE_FLAG=literal"));
        assert!(restore
            .windows(2)
            .any(|pair| pair == ["--env", "GH_TOKEN=$MSB_SILO_GITHUB"]));
        assert!(!restore.iter().any(|arg| arg.contains("Old Author")
            || arg.contains("old@example.test")
            || arg.contains("GIT_")
            || arg.contains("JJ_")));
        assert!(!fs::read_to_string(path(&paths, ID))
            .unwrap()
            .contains("synthetic-old-token"));
        assert!(!restore
            .iter()
            .any(|arg| arg.contains("synthetic-old-token")));
        for env in [
            serde_json::json!([{"key":"BAD=KEY","value":"value"}]),
            serde_json::json!([{"key":"KEY","value":"bad\u{0000}value"}]),
            serde_json::json!([{"key":"KEY","value":"value","extra":true}]),
        ] {
            assert!(import_pending_restore_with_environment(
                &paths,
                ID,
                "silo-import-6b79cf8f70b34f2d93d13eeb3798a8b9",
                "silo-backup-0-330418-1790360984903",
                &serde_json::json!({"env":env})
            )
            .is_err());
        }
    }

    #[test]
    fn a_fork_after_an_identity_save_inherits_the_cleaned_environment() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let (paths, fork) = fork_fixture(&directory);
        let mut record = load(&paths, ID).unwrap();
        record.desired_environment = vec![
            Environment {
                key: "PROJECT_MODE".into(),
                value: "kept".into(),
            },
            Environment {
                key: "GIT_AUTHOR_NAME".into(),
                value: "Old Author".into(),
            },
            Environment {
                key: "JJ_EMAIL".into(),
                value: "old@example.test".into(),
            },
        ];
        save(&paths, ID, &record).unwrap();
        forget_identity_environment(&paths, ID).unwrap();
        forget_identity_environment(&paths, "11111111-1111-4111-8111-111111111111").unwrap();
        fork_commit(
            &journal_runner("Running", ""),
            &paths,
            &FakeAssignments::new(&[]),
            &fork,
            "branch",
        )
        .unwrap();
        let metadata = read_metadata(&paths.metadata).unwrap();
        let child = metadata
            .computers
            .iter()
            .find(|configuration| configuration.name() == "branch")
            .unwrap();
        for id in [ID, child.id()] {
            let keys: Vec<String> = load(&paths, id)
                .unwrap()
                .desired_environment
                .into_iter()
                .map(|entry| entry.key)
                .collect();
            assert_eq!(keys, ["PROJECT_MODE"]);
        }
    }

    /// Records every command, answers like a runtime whose restore fails.
    struct RestoreProbe(Mutex<Vec<Vec<String>>>);
    impl RuntimeRunner for RestoreProbe {
        fn run(
            &self,
            _paths: &RuntimePaths,
            args: &[String],
            _timeout: Duration,
        ) -> Result<CommandOutput, RuntimeError> {
            self.0.lock().unwrap().push(args.to_vec());
            let stdout = match args.first().map(String::as_str) {
                Some("snapshot") => serde_json::json!([{
                    "group":"silo-import-6b79cf8f70b34f2d93d13eeb3798a8b9",
                    "name":"silo-backup-0-330418-1790360984903",
                    "scope":"disk", "availability":"ready"
                }])
                .to_string(),
                Some("list") => "[]".into(),
                Some("restore") => return Err(error("synthetic restore failure")),
                Some("inspect") => return Err(error("computer not found: dev")),
                _ => panic!("unexpected command: {args:?}"),
            };
            Ok(CommandOutput {
                stdout,
                stderr: String::new(),
            })
        }
    }

    fn built_in_computer() -> ComputerConfiguration {
        let mut configuration = computer_configuration();
        {
            let ComputerConfiguration { desktop, .. } = &mut configuration;
            *desktop = Some(crate::desktop::DesktopConfiguration {
                start_with_computer: true,
                built_in: true,
            });
        }
        configuration
    }

    fn pending_import(paths: &RuntimePaths) {
        import_pending_restore(
            paths,
            ID,
            "silo-import-6b79cf8f70b34f2d93d13eeb3798a8b9",
            "silo-backup-0-330418-1790360984903",
        )
        .unwrap();
    }

    #[test]
    fn restoring_a_built_in_computer_passes_the_computer_use_mount_again() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let published = directory.path().join("published");
        std::fs::create_dir(&published).unwrap();
        // Empty on purpose: the ChatGPT app may still be downloading.
        assert_eq!(std::fs::read_dir(&published).unwrap().count(), 0);
        crate::computer_use::set_test_published_dir(Some(published.clone()));
        pending_import(&paths);
        let runner = RestoreProbe(Mutex::new(Vec::new()));
        assert!(start_pending(&runner, &paths, &built_in_computer()).is_err());
        let calls = runner.0.lock().unwrap();
        let restore = calls.iter().find(|args| args[0] == "restore").unwrap();
        let mount = format!("{}:/opt/silo/chatgpt:ro,uid=0,gid=0", published.display());
        assert!(
            restore
                .windows(2)
                .any(|pair| pair == ["-v", mount.as_str()]),
            "{restore:?}"
        );
        crate::computer_use::set_test_published_dir(None);
    }

    #[test]
    fn restoring_a_computer_without_built_in_computer_use_adds_no_mount() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        crate::computer_use::set_test_published_dir(Some(directory.path().to_path_buf()));
        pending_import(&paths);
        let runner = RestoreProbe(Mutex::new(Vec::new()));
        assert!(start_pending(&runner, &paths, &computer_configuration()).is_err());
        let calls = runner.0.lock().unwrap();
        let restore = calls.iter().find(|args| args[0] == "restore").unwrap();
        assert!(!restore
            .iter()
            .any(|arg| arg == "-v" || arg.contains("/opt/silo")));
        crate::computer_use::set_test_published_dir(None);
    }

    const ATTEMPT: &str = "6b79cf8f-70b3-4d2f-93d1-3b8b7a7c0001";

    /// A runtime where the rejected restore left a stopped, already-run attempt with
    /// the computer's labels but without the computer-use mount.
    struct RejectedRestore(Mutex<Vec<Vec<String>>>);
    impl RuntimeRunner for RejectedRestore {
        fn run(
            &self,
            _paths: &RuntimePaths,
            args: &[String],
            _timeout: Duration,
        ) -> Result<CommandOutput, RuntimeError> {
            self.0.lock().unwrap().push(args.to_vec());
            let stdout = match args.first().map(String::as_str) {
                Some("snapshot") => serde_json::json!([{
                    "group":"silo-import-6b79cf8f70b34f2d93d13eeb3798a8b9",
                    "name":"silo-backup-0-330418-1790360984903",
                    "scope":"disk", "availability":"ready"
                }])
                .to_string(),
                Some("list") => serde_json::json!([{"name":"dev"}]).to_string(),
                Some("inspect") => serde_json::json!({"name":"dev","status":"Stopped","config":{
                    "labels":{"silo.managed":"true","silo.machine-id":ID,
                        "silo.restore-attempt":ATTEMPT},
                    "mounts":[{"type":"Owned","guest":"/workspace"}],
                }})
                .to_string(),
                _ => panic!("a retry without the mount must not change the computer: {args:?}"),
            };
            Ok(CommandOutput {
                stdout,
                stderr: String::new(),
            })
        }
    }

    #[test]
    fn retrying_a_rejected_restore_still_requires_the_computer_use_mount() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let published = directory.path().join("published");
        std::fs::create_dir(&published).unwrap();
        crate::computer_use::set_test_published_dir(Some(published));
        pending_import(&paths);
        let mut record = load(&paths, ID).unwrap();
        record.restore_attempted = true;
        record.restore_attempt_ran = true;
        record.restore_attempt_id = Some(ATTEMPT.into());
        save(&paths, ID, &record).unwrap();
        let runner = RejectedRestore(Mutex::new(Vec::new()));
        let failure = start_pending(&runner, &paths, &built_in_computer())
            .unwrap_err()
            .to_string();
        assert!(failure.contains("computer-use folder"), "{failure}");
        // No command changed the computer, and the recovery state survives for the next Retry.
        assert!(runner
            .0
            .lock()
            .unwrap()
            .iter()
            .all(|args| matches!(args[0].as_str(), "snapshot" | "list" | "inspect")));
        let kept = load(&paths, ID).unwrap();
        assert!(kept.pending_checkpoint_restore.is_some());
        assert!(kept.restore_attempted && kept.restore_attempt_ran);
        assert_eq!(kept.restore_attempt_id.as_deref(), Some(ATTEMPT));
        crate::computer_use::set_test_published_dir(None);
    }

    #[test]
    fn a_built_in_computer_cannot_restore_without_the_shared_folder() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        crate::computer_use::set_test_published_dir(None);
        pending_import(&paths);
        let runner = RestoreProbe(Mutex::new(Vec::new()));
        let failure = start_pending(&runner, &paths, &built_in_computer()).unwrap_err();
        assert!(failure.to_string().contains("shared ChatGPT folder"));
        // Nothing ran and the pending restore is untouched.
        assert!(runner.0.lock().unwrap().is_empty());
        assert!(load(&paths, ID)
            .unwrap()
            .pending_checkpoint_restore
            .is_some());
    }

    #[test]
    fn snapshot_group_is_persisted_and_import_provenance_is_not_replaced() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        assert_eq!(ensure_snapshot_group(&paths, ID, "dev").unwrap(), "dev");
        assert_eq!(ensure_snapshot_group(&paths, ID, "renamed").unwrap(), "dev");

        let imported_id = "00000000-0000-4000-8000-000000000002";
        let imported_group = "silo-import-6b79cf8f70b34f2d93d13eeb3798a8b9";
        import_pending_restore(
            &paths,
            imported_id,
            imported_group,
            "silo-backup-0-330418-1790360984903",
        )
        .unwrap();
        assert_eq!(
            ensure_snapshot_group(&paths, imported_id, "archive-copy").unwrap(),
            imported_group
        );
    }

    #[test]
    fn original_v1_checkpoints_migrate_to_default_group_but_lost_import_group_fails_closed() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let mut original = Record::default();
        original.checkpoints.push(Checkpoint {
            id: "c000000000000000000000000000000".into(),
            native_id: None,
            name: "Before update".into(),
            created_at: 1,
            scope: "disk".into(),
            reason: "manual".into(),
        });
        save(&paths, ID, &original).unwrap();
        assert_eq!(ensure_snapshot_group(&paths, ID, "dev").unwrap(), "dev");
        assert_eq!(
            load(&paths, ID).unwrap().snapshot_group.as_deref(),
            Some("dev")
        );

        let imported_id = "00000000-0000-4000-8000-000000000003";
        let lost_origin = Record::default();
        save(&paths, imported_id, &lost_origin).unwrap();
        assert!(ensure_snapshot_group(&paths, imported_id, "renamed-import").is_err());
        assert_eq!(load(&paths, imported_id).unwrap().snapshot_group, None);
    }

    struct Runner {
        mount_type: &'static str,
        calls: Mutex<Vec<Vec<String>>>,
    }
    impl RuntimeRunner for Runner {
        fn run(
            &self,
            _paths: &RuntimePaths,
            args: &[String],
            _timeout: Duration,
        ) -> Result<CommandOutput, RuntimeError> {
            self.calls.lock().unwrap().push(args.to_vec());
            let stdout = if args[0] == "inspect" {
                serde_json::json!({
                    "name":"dev", "status":"Running", "config":{
                        "labels":{"silo.managed":"true","silo.machine-id":ID},
                        "mounts":[{"guest":"/workspace","type":self.mount_type,
                            "storage":{"kind":"disk","capacity_mib":1024}}]
                    }
                })
                .to_string()
            } else if args[0] == "snapshot" && args[1] == "list" {
                let calls = self.calls.lock().unwrap();
                let name = calls
                    .iter()
                    .find(|call| call.get(1).map(String::as_str) == Some("create"))
                    .and_then(|call| call.get(2))
                    .cloned()
                    .unwrap_or_default();
                serde_json::json!([{"group":"dev","name":name,"scope":"full","availability":"ready"}]).to_string()
            } else {
                String::new()
            };
            Ok(CommandOutput {
                stdout,
                stderr: String::new(),
            })
        }
    }
    fn paths(directory: &tempfile::TempDir) -> RuntimePaths {
        crate::test_support::paths(directory.path())
    }
    fn computer_configuration() -> ComputerConfiguration {
        ComputerConfiguration {
            id: ID.into(),
            name: "dev".into(),
            cpus: 1,
            max_cpus: 1,
            memory_gib: 1,
            max_memory_gib: 1,
            workspace_storage_gib: 1,
            runtime_storage_gib: 1,
            desktop: None,
        }
    }
    #[test]
    fn fork_from_recovery_uses_saved_policy_only_while_source_is_pending_and_absent() {
        let _test_state = crate::test_support::global_state();
        struct SourceRunner {
            present: bool,
            calls: Mutex<Vec<Vec<String>>>,
        }
        impl RuntimeRunner for SourceRunner {
            fn run(
                &self,
                _paths: &RuntimePaths,
                args: &[String],
                _timeout: Duration,
            ) -> Result<CommandOutput, RuntimeError> {
                self.calls.lock().unwrap().push(args.to_vec());
                let stdout = match args[0].as_str() {
                    "list" => serde_json::json!([{"name":"dev","status":"Stopped"}]).to_string(),
                    "inspect" => {
                        return Err(RuntimeError::Unavailable("computer not found: dev".into()))
                    }
                    _ => panic!("unexpected runtime command: {args:?}"),
                };
                Ok(CommandOutput {
                    stdout: if self.present { stdout } else { "[]".into() },
                    stderr: String::new(),
                })
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let mut record = Record::default();
        record.pending_checkpoint_restore = Some(PendingRestore {
            checkpoint_id: "c000000000000000000000000000000".into(),
            source_computer: "silo-import-6b79cf8f70b34f2d93d13eeb3798a8b9".into(),
            state: "full".into(),
        });
        let policy =
            serde_json::json!({"default_egress":"deny","default_ingress":"allow","rules":[]});
        record.desired_network_policy = Some(policy.clone());
        let absent = SourceRunner {
            present: false,
            calls: Mutex::new(Vec::new()),
        };
        assert_eq!(
            fork_source_policy(&absent, &paths, &computer_configuration(), &record).unwrap(),
            policy
        );
        assert_eq!(
            absent
                .calls
                .lock()
                .unwrap()
                .iter()
                .map(|args| args[0].as_str())
                .collect::<Vec<_>>(),
            ["list"]
        );
        let present = SourceRunner {
            present: true,
            calls: Mutex::new(Vec::new()),
        };
        assert!(fork_source_policy(&present, &paths, &computer_configuration(), &record).is_err());
        assert_eq!(
            present
                .calls
                .lock()
                .unwrap()
                .iter()
                .map(|args| args[0].as_str())
                .collect::<Vec<_>>(),
            ["list", "inspect"]
        );
        record.desired_network_policy = None;
        assert!(fork_source_policy(&absent, &paths, &computer_configuration(), &record).is_err());
        record.desired_network_policy = Some(serde_json::json!({"default_egress":"invalid"}));
        assert!(fork_source_policy(&absent, &paths, &computer_configuration(), &record).is_err());
    }
    #[test]
    fn captures_full_state_only_with_an_owned_workspace_disk() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(
            &paths.metadata,
            &ComputerConfigurationRequest {
                schema_version: 1,
                computers: vec![computer_configuration()],
            },
        )
        .unwrap();
        let legacy = Runner {
            mount_type: "DiskImage",
            calls: Mutex::new(Vec::new()),
        };
        assert!(capture_with(&legacy, &paths, ID, "Before work", "manual")
            .unwrap_err()
            .to_string()
            .contains("migration"));
        assert_eq!(legacy.calls.lock().unwrap().len(), 1);
        let owned = Runner {
            mount_type: "Owned",
            calls: Mutex::new(Vec::new()),
        };
        capture_with(&owned, &paths, ID, "Before work", "manual").unwrap();
        let calls = owned.calls.lock().unwrap();
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[1][..2], ["snapshot", "create"]);
        assert!(calls[1].iter().any(|arg| arg == "--full"));
        assert!(calls[1].windows(2).any(|pair| pair == ["--group", "dev"]));
        assert_eq!(load(&paths, ID).unwrap().checkpoints[0].scope, "full");
    }

    #[test]
    fn pending_restore_checkpoint_creation_aliases_immutable_full_snapshot() {
        let _test_state = crate::test_support::global_state();
        struct SnapshotInventory {
            present: bool,
            calls: Mutex<Vec<Vec<String>>>,
        }
        impl RuntimeRunner for SnapshotInventory {
            fn run(
                &self,
                _paths: &RuntimePaths,
                args: &[String],
                _timeout: Duration,
            ) -> Result<CommandOutput, RuntimeError> {
                self.calls.lock().unwrap().push(args.to_vec());
                let stdout = match args[0].as_str() {
                    "list" if self.present => serde_json::json!([{"name":"dev"}]).to_string(),
                    "list" => "[]".into(),
                    "snapshot" if args.get(1).is_some_and(|value| value == "list") => serde_json::json!([
                        {"group":"dev","name":"c000000000000000000000000000000","scope":"full","availability":"ready"}
                    ]).to_string(),
                    _ => panic!("unexpected runtime command: {args:?}"),
                };
                Ok(CommandOutput {
                    stdout,
                    stderr: String::new(),
                })
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(
            &paths.metadata,
            &ComputerConfigurationRequest {
                schema_version: 1,
                computers: vec![computer_configuration()],
            },
        )
        .unwrap();
        let mut pending = Record::default();
        pending.snapshot_group = Some("dev".into());
        pending.pending_checkpoint_restore = Some(PendingRestore {
            checkpoint_id: "c000000000000000000000000000000".into(),
            source_computer: "dev".into(),
            state: "full".into(),
        });
        pending.desired_network_policy = Some(serde_json::json!({
            "default_egress":"deny", "default_ingress":"deny", "rules":[]
        }));
        save(&paths, ID, &pending).unwrap();
        let runner = SnapshotInventory {
            present: false,
            calls: Mutex::new(Vec::new()),
        };
        capture_with(&runner, &paths, ID, "After restore", "manual").unwrap();
        let stored = load(&paths, ID).unwrap();
        let alias = &stored.checkpoints[0];
        assert_ne!(alias.id, "c000000000000000000000000000000");
        assert_eq!(
            alias.native_id.as_deref(),
            Some("c000000000000000000000000000000")
        );
        assert_eq!(alias.scope, "full");
        assert_eq!(
            stored.pending_checkpoint_restore.unwrap().checkpoint_id,
            "c000000000000000000000000000000"
        );
        assert!(runner
            .calls
            .lock()
            .unwrap()
            .iter()
            .all(|args| args[0] != "inspect" && args.get(1).map(String::as_str) != Some("create")));

        let mut legacy = Record::default();
        legacy.pending_checkpoint_restore = Some(PendingRestore {
            checkpoint_id: "c000000000000000000000000000000".into(),
            source_computer: "dev".into(),
            state: "full".into(),
        });
        legacy.desired_network_policy = stored.desired_network_policy;
        save(&paths, ID, &legacy).unwrap();
        capture_with(&runner, &paths, ID, "Legacy alias", "manual").unwrap();
        assert_eq!(
            load(&paths, ID).unwrap().snapshot_group.as_deref(),
            Some("dev")
        );

        let before_collision = load(&paths, ID).unwrap();
        let collision = SnapshotInventory {
            present: true,
            calls: Mutex::new(Vec::new()),
        };
        assert!(capture_with(&collision, &paths, ID, "Unsafe alias", "manual").is_err());
        assert_eq!(
            load(&paths, ID).unwrap().checkpoints.len(),
            before_collision.checkpoints.len()
        );
        assert!(!collision
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|args| args[0] == "inspect"));
    }

    #[test]
    fn pending_full_checkpoint_can_be_aliased_switched_recovered_and_started_explicitly() {
        let _test_state = crate::test_support::global_state();
        struct Inventory {
            calls: Mutex<Vec<Vec<String>>>,
        }
        impl RuntimeRunner for Inventory {
            fn run(
                &self,
                _paths: &RuntimePaths,
                args: &[String],
                _timeout: Duration,
            ) -> Result<CommandOutput, RuntimeError> {
                self.calls.lock().unwrap().push(args.to_vec());
                let stdout = match args[0].as_str() {
                    "list" => "[]".into(),
                    "snapshot" if args.get(1).is_some_and(|value| value == "list") => serde_json::json!([
                        {"group":"dev","name":"c000000000000000000000000000000","scope":"full","availability":"ready"},
                        {"group":"dev","name":"c111111111111111111111111111111","scope":"full","availability":"ready"}
                    ]).to_string(),
                    "snapshot" if args.get(1).is_some_and(|value| value == "verify") => "{}".into(),
                    "restore" => return Err(error("synthetic restore failure after request capture")),
                    "inspect" => return Err(error("computer not found: dev")),
                    _ => panic!("unexpected runtime command: {args:?}"),
                };
                Ok(CommandOutput {
                    stdout,
                    stderr: String::new(),
                })
            }
        }

        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(
            &paths.metadata,
            &ComputerConfigurationRequest {
                schema_version: 1,
                computers: vec![computer_configuration()],
            },
        )
        .unwrap();
        let first = Checkpoint {
            id: "c000000000000000000000000000000".into(),
            native_id: None,
            name: "First full state".into(),
            created_at: 1,
            scope: "full".into(),
            reason: "manual".into(),
        };
        let second = Checkpoint {
            id: "c111111111111111111111111111111".into(),
            native_id: None,
            name: "Second full state".into(),
            created_at: 2,
            scope: "full".into(),
            reason: "manual".into(),
        };
        let mut record = Record::default();
        record.snapshot_group = Some("dev".into());
        record.checkpoints = vec![first.clone(), second.clone()];
        record.pending_checkpoint_restore = Some(PendingRestore {
            checkpoint_id: first.id.clone(),
            source_computer: "dev".into(),
            state: "full".into(),
        });
        record.desired_network_policy = Some(serde_json::json!({
            "default_egress":"deny", "default_ingress":"allow", "rules":[]
        }));
        save(&paths, ID, &record).unwrap();
        let runner = Inventory {
            calls: Mutex::new(Vec::new()),
        };

        capture_with(&runner, &paths, ID, "Saved first state", "manual").unwrap();
        let alias = load(&paths, ID).unwrap().checkpoints[0].clone();
        assert_eq!(alias.native_id(), first.id);
        assert_eq!(alias.scope, "full");

        restore_with(&runner, &paths, ID, &second.id).unwrap();
        let after_switch = load(&paths, ID).unwrap();
        let recovery_of_first = after_switch.checkpoints[0].clone();
        assert_ne!(recovery_of_first.id, first.id);
        assert_eq!(recovery_of_first.name, "Before restore");
        assert_eq!(recovery_of_first.reason, "before-restore");
        assert_eq!(recovery_of_first.native_id(), first.id);
        assert_eq!(recovery_of_first.scope, "full");
        assert_eq!(
            after_switch
                .pending_checkpoint_restore
                .unwrap()
                .checkpoint_id,
            second.id
        );

        restore_with(&runner, &paths, ID, &recovery_of_first.id).unwrap();
        let restored = load(&paths, ID).unwrap();
        assert_eq!(
            restored
                .pending_checkpoint_restore
                .as_ref()
                .unwrap()
                .checkpoint_id,
            first.id
        );
        assert!(restored.checkpoints.iter().any(|checkpoint| {
            checkpoint.native_id() == second.id && checkpoint.reason == "before-restore"
        }));

        let device = super::super::DeviceResources {
            logical_cpus: 8,
            physical_memory_bytes: Some(16 * 1024 * 1024 * 1024),
        };
        let failure =
            super::super::explicit_computer_action_with(&runner, &paths, &device, "start", "dev")
                .unwrap_err();
        assert!(failure.to_string().contains("synthetic restore failure"));
        let calls = runner.calls.lock().unwrap();
        let start = calls.iter().find(|args| args[0] == "restore").unwrap();
        assert_eq!(start[1], format!("dev:{}", first.id));
        assert!(start.iter().any(|arg| arg == "--cow-mem"));
        assert!(!start.iter().any(|arg| arg == "--disk-only"));
        assert!(load(&paths, ID)
            .unwrap()
            .pending_checkpoint_restore
            .is_some());
    }

    #[test]
    fn current_state_fork_from_pending_full_snapshot_reuses_reference_without_starting() {
        let _test_state = crate::test_support::global_state();
        struct ForkInventory {
            present: bool,
            calls: Mutex<Vec<Vec<String>>>,
        }
        impl RuntimeRunner for ForkInventory {
            fn run(
                &self,
                _paths: &RuntimePaths,
                args: &[String],
                _timeout: Duration,
            ) -> Result<CommandOutput, RuntimeError> {
                self.calls.lock().unwrap().push(args.to_vec());
                let stdout = match args[0].as_str() {
                    "list" if self.present => serde_json::json!([{"name":"dev"}]).to_string(),
                    "list" => "[]".into(),
                    "snapshot" if args.get(1).is_some_and(|value| value == "list") => serde_json::json!([
                        {"group":"dev","name":"silo-backup-0-330418-1790360984903","scope":"full","availability":"ready"}
                    ]).to_string(),
                    _ => panic!("unexpected runtime command: {args:?}"),
                };
                Ok(CommandOutput {
                    stdout,
                    stderr: String::new(),
                })
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let mut record = Record::default();
        record.snapshot_group = Some("dev".into());
        record.pending_checkpoint_restore = Some(PendingRestore {
            checkpoint_id: "silo-backup-0-330418-1790360984903".into(),
            source_computer: "dev".into(),
            state: "full".into(),
        });
        record.desired_network_policy = Some(serde_json::json!({
            "default_egress":"deny", "default_ingress":"allow", "rules":[]
        }));
        let runner = ForkInventory {
            present: false,
            calls: Mutex::new(Vec::new()),
        };
        let point = pending_current_fork_point(&runner, &paths, "dev", &record, "dev")
            .unwrap()
            .unwrap();
        assert_eq!(point.checkpoint_id, "silo-backup-0-330418-1790360984903");
        assert_eq!(point.state, "full");
        assert_eq!(
            runner
                .calls
                .lock()
                .unwrap()
                .iter()
                .map(|args| args[0].as_str())
                .collect::<Vec<_>>(),
            ["list", "snapshot"]
        );

        let collision = ForkInventory {
            present: true,
            calls: Mutex::new(Vec::new()),
        };
        assert!(pending_current_fork_point(&collision, &paths, "dev", &record, "dev").is_err());
        assert_eq!(collision.calls.lock().unwrap().len(), 1);
    }

    struct Observed {
        status: &'static str,
        missing: bool,
        calls: Mutex<usize>,
    }
    impl RuntimeRunner for Observed {
        fn run(
            &self,
            _: &RuntimePaths,
            args: &[String],
            _: Duration,
        ) -> Result<CommandOutput, RuntimeError> {
            *self.calls.lock().unwrap() += 1;
            assert_eq!(args[0], "inspect");
            if self.missing {
                return Err(RuntimeError::Failed {
                    operation: "Inspecting the computer".into(),
                    exit_code: Some(1),
                    detail: "computer 'dev' not found".into(),
                });
            }
            Ok(CommandOutput {
                stdout: serde_json::json!({"name":"dev","status":self.status,"config":{"labels":{"silo.managed":"true","silo.machine-id":ID}}}).to_string(),
                stderr: String::new(),
            })
        }
    }
    #[test]
    fn pending_and_stopped_computers_read_as_stopped_and_ask_to_be_started() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(
            &paths.metadata,
            &ComputerConfigurationRequest {
                schema_version: 1,
                computers: vec![computer_configuration()],
            },
        )
        .unwrap();

        // Normally stopped: inspected, present, and actions say to start it.
        let stopped = Observed {
            status: "Stopped",
            missing: false,
            calls: Mutex::new(0),
        };
        assert!(matches!(
            observe_computer(&stopped, &paths, "dev").unwrap(),
            ComputerRuntime::Present(_)
        ));
        assert_eq!(
            crate::terminal::running_computer_with(&stopped, &paths, "dev")
                .err()
                .unwrap(),
            "Start dev first."
        );

        // A runtime that does not know the computer is stopped, not an error.
        let missing = Observed {
            status: "",
            missing: true,
            calls: Mutex::new(0),
        };
        assert!(matches!(
            observe_computer(&missing, &paths, "dev").unwrap(),
            ComputerRuntime::Absent
        ));

        // Pending restore: decided from Silo's record; the runtime is never asked.
        let mut record = Record::default();
        record.pending_checkpoint_restore = Some(PendingRestore {
            checkpoint_id: "c000000000000000000000000000000".into(),
            source_computer: "source".into(),
            state: "full".into(),
        });
        save(&paths, ID, &record).unwrap();
        let pending = Observed {
            status: "Running",
            missing: false,
            calls: Mutex::new(0),
        };
        assert!(matches!(
            observe_computer(&pending, &paths, "dev").unwrap(),
            ComputerRuntime::Absent
        ));
        assert!(is_pending_restore(&paths, "dev"));
        assert_eq!(
            crate::terminal::running_computer_with(&pending, &paths, "dev")
                .err()
                .unwrap(),
            "Start dev first."
        );
        assert_eq!(*pending.calls.lock().unwrap(), 0);
        // Network reads its saved ports as stopped: no error, no raw inspect failure.
        let (error, ports) = crate::network::observe_saved_port_for_test(&paths, "dev", 3000);
        assert_eq!(error, None);
        assert_eq!(ports, vec![("waiting", None)]);

        // A failed restore may already own a live computer. Its pending record must
        // not hide that computer from terminal access or secret revocation checks.
        record.restore_attempted = true;
        save(&paths, ID, &record).unwrap();
        assert!(matches!(
            observe_computer(&pending, &paths, "dev").unwrap(),
            ComputerRuntime::Present(_)
        ));
        assert!(crate::terminal::running_computer_with(&pending, &paths, "dev").is_ok());
        assert_eq!(*pending.calls.lock().unwrap(), 2);
        assert!(matches!(
            observe_computer(&missing, &paths, "dev").unwrap(),
            ComputerRuntime::Absent
        ));

        // Other runtime failures still surface.
        let broken = crate::test_support::runner::ScriptedRunner::new([
            crate::test_support::runner::ExpectedCommand::error(
                ["inspect", "dev", "--format", "json"],
                RuntimeError::Unavailable("runtime down".into()),
            )
            .with_timeout(READ_TIMEOUT),
        ]);
        delete_record_for_test(&paths);
        assert!(
            matches!(observe_computer(&broken, &paths, "dev"), Err(RuntimeError::Unavailable(message)) if message == "runtime down")
        );
        broken.assert_finished();
    }
    fn delete_record_for_test(paths: &RuntimePaths) {
        save(paths, ID, &Record::default()).unwrap();
    }
    #[test]
    fn pending_fork_survives_reload_and_cannot_auto_start_or_use_lifecycle_start() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(
            &paths.metadata,
            &ComputerConfigurationRequest {
                schema_version: 1,
                computers: vec![computer_configuration()],
            },
        )
        .unwrap();
        let mut record = Record::default();
        record.pending_checkpoint_restore = Some(PendingRestore {
            checkpoint_id: "c000000000000000000000000000000".into(),
            source_computer: "source".into(),
            state: "full".into(),
        });
        save(&paths, ID, &record).unwrap();
        assert!(is_pending(&paths, ID).unwrap());
        let runner = Runner {
            mount_type: "Owned",
            calls: Mutex::new(Vec::new()),
        };
        let device = DeviceResources {
            logical_cpus: 8,
            physical_memory_bytes: Some(16 * 1024 * 1024 * 1024),
        };
        start_at_launch_with(&runner, &paths, &device, ID).unwrap();
        assert!(runner.calls.lock().unwrap().is_empty());
        assert!(computer_action_with(&runner, &paths, &device, "start", "dev").is_err());
        assert!(runner.calls.lock().unwrap().is_empty());
        assert!(run_msb(
            &paths,
            &["exec".into(), "dev".into(), "--".into(), "true".into()],
            READ_TIMEOUT
        )
        .unwrap_err()
        .to_string()
        .contains("starts from a checkpoint first"));
        let view = pending_computer(computer_configuration()).unwrap();
        assert!(matches!(view.state, ComputerState::Stopped));
    }
    #[test]
    fn first_start_network_flags_preserve_current_asymmetric_policy() {
        let _test_state = crate::test_support::global_state();
        let config = serde_json::json!({"network":{"policy":{
            "default_egress":"deny", "default_ingress":"allow", "rules":[
                {"action":"allow","direction":"egress","destination":{"group":"host"},
                    "protocols":["udp","tcp"],"ports":[{"start":53,"end":53}]},
                {"action":"allow","direction":"egress","destination":{"group":"public"},
                    "protocols":[],"ports":[]}
            ]
        }}});
        let args = current_network_args(&config).unwrap();
        assert_eq!(
            args,
            [
                "--net-default-egress",
                "deny",
                "--net-default-ingress",
                "allow",
                "--net-rule",
                "allow@dns",
                "--net-rule",
                "allow@public",
            ]
        );
    }
    #[test]
    fn failed_first_start_preserves_pending_checkpoint_and_source() {
        let _test_state = crate::test_support::global_state();
        struct FailedRestore {
            calls: Mutex<Vec<Vec<String>>>,
        }
        impl RuntimeRunner for FailedRestore {
            fn run(
                &self,
                _paths: &RuntimePaths,
                args: &[String],
                _timeout: Duration,
            ) -> Result<CommandOutput, RuntimeError> {
                self.calls.lock().unwrap().push(args.to_vec());
                let stdout = match args[0].as_str() {
                    "inspect" => serde_json::json!({"name":"dev","status":"Running","config":{
                        "labels":{"silo.managed":"true","silo.machine-id":ID},
                        "network":{"policy":{"default_egress":"deny","default_ingress":"allow","rules":[]}}
                    }}).to_string(),
                    "snapshot" => serde_json::json!([{"group":"dev","name":"c000000000000000000000000000000","scope":"full","availability":"ready"}]).to_string(),
                    "list" => serde_json::json!([{"name":"dev"}]).to_string(),
                    "restore" => return Err(RuntimeError::Failed {operation:"restore".into(),exit_code:None,detail:"synthetic failure".into()}),
                    _ => panic!("unexpected runtime command"),
                };
                Ok(CommandOutput {
                    stdout,
                    stderr: String::new(),
                })
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let mut child = computer_configuration();
        let child_id = "00000000-0000-4000-8000-000000000002";
        {
            let ComputerConfiguration { id, name, .. } = &mut child;
            *id = child_id.into();
            *name = "fork".into();
        }
        write_metadata(
            &paths.metadata,
            &ComputerConfigurationRequest {
                schema_version: 1,
                computers: vec![computer_configuration(), child.clone()],
            },
        )
        .unwrap();
        let mut record = Record::default();
        record.pending_checkpoint_restore = Some(PendingRestore {
            checkpoint_id: "c000000000000000000000000000000".into(),
            source_computer: "dev".into(),
            state: "full".into(),
        });
        record.desired_network_policy =
            Some(serde_json::json!({"default_egress":"deny","default_ingress":"allow","rules":[]}));
        save(&paths, child_id, &record).unwrap();
        let runner = FailedRestore {
            calls: Mutex::new(Vec::new()),
        };
        // The runtime's own explanation is the failure's diagnostic, not its summary.
        assert!(
            super::failure_report(&start_pending(&runner, &paths, &child).unwrap_err())
                .diagnostic
                .is_some_and(|text| text.contains("synthetic failure"))
        );
        let stored = load(&paths, child_id).unwrap();
        assert!(stored.pending_checkpoint_restore.is_some());
        assert_eq!(stored.checkpoint_operation.unwrap().status, "failed");
        assert!(read_metadata(&paths.metadata)
            .unwrap()
            .computers
            .iter()
            .any(|m| m.name() == "dev"));
        assert_eq!(
            runner
                .calls
                .lock()
                .unwrap()
                .iter()
                .filter(|args| args[0] == "restore")
                .count(),
            1
        );
    }
    #[test]
    fn restore_journal_distinguishes_original_before_and_after_runtime_removal() {
        let _test_state = crate::test_support::global_state();
        struct Listing {
            present: bool,
            calls: Mutex<Vec<Vec<String>>>,
        }
        impl RuntimeRunner for Listing {
            fn run(
                &self,
                _paths: &RuntimePaths,
                args: &[String],
                _timeout: Duration,
            ) -> Result<CommandOutput, RuntimeError> {
                self.calls.lock().unwrap().push(args.to_vec());
                let stdout = if args[0] == "list" {
                    if self.present {
                        serde_json::json!([{"name":"dev"}]).to_string()
                    } else {
                        "[]".into()
                    }
                } else if args[0] == "snapshot" && args[1] == "list" {
                    serde_json::json!([{"group":"dev","name":"c000000000000000000000000000000","scope":"full","availability":"ready"}]).to_string()
                } else if args[0] == "restore" {
                    return Err(error("synthetic restore failure"));
                } else if args[0] == "inspect" && !self.present {
                    return Err(error("computer not found: dev"));
                } else {
                    panic!("unexpected runtime command")
                };
                Ok(CommandOutput {
                    stdout,
                    stderr: String::new(),
                })
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(
            &paths.metadata,
            &ComputerConfigurationRequest {
                schema_version: 1,
                computers: vec![computer_configuration()],
            },
        )
        .unwrap();
        let target = Checkpoint {
            id: "c000000000000000000000000000000".into(),
            native_id: None,
            name: "target".into(),
            created_at: 1,
            scope: "full".into(),
            reason: "manual".into(),
        };
        let recovery = Checkpoint {
            id: "c111111111111111111111111111111".into(),
            native_id: None,
            name: "Before restore".into(),
            created_at: 2,
            scope: "full".into(),
            reason: "before-restore".into(),
        };
        let mut record = Record::default();
        record.checkpoints = vec![recovery.clone(), target.clone()];
        record.restore_journal = Some(RestoreJournal {
            target_checkpoint_id: target.id.clone(),
            recovery_checkpoint: recovery,
            prior_running: true,
            phase: "secured".into(),
        });
        record.desired_network_policy =
            Some(serde_json::json!({"default_egress":"deny","default_ingress":"allow","rules":[]}));
        save(&paths, ID, &record).unwrap();
        assert!(!pending_view(&paths, ID, true).unwrap());
        assert!(pending_view(&paths, ID, false).unwrap());
        let original = Listing {
            present: true,
            calls: Mutex::new(Vec::new()),
        };
        assert!(start_pending(&original, &paths, &computer_configuration())
            .unwrap_err()
            .to_string()
            .contains("is unfinished"));
        assert_eq!(original.calls.lock().unwrap().len(), 1);
        assert!(load(&paths, ID)
            .unwrap()
            .pending_checkpoint_restore
            .is_none());
        let removed = Listing {
            present: false,
            calls: Mutex::new(Vec::new()),
        };
        let failure = start_pending(&removed, &paths, &computer_configuration()).unwrap_err();
        assert!(
            failure.to_string().contains("synthetic restore failure"),
            "{failure}"
        );
        let after = load(&paths, ID).unwrap();
        assert!(after.pending_checkpoint_restore.is_some());
        assert!(after.restore_journal.is_none());
        assert!(!removed
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|args| args[0] == "remove"));

        let mut fork = load(&paths, ID).unwrap();
        fork.pending_checkpoint_restore = Some(PendingRestore {
            checkpoint_id: target.id,
            source_computer: "dev".into(),
            state: "full".into(),
        });
        save(&paths, ID, &fork).unwrap();
        let fork_failure = start_pending(&removed, &paths, &computer_configuration()).unwrap_err();
        assert!(
            fork_failure
                .to_string()
                .contains("synthetic restore failure"),
            "{fork_failure}"
        );
        let calls = removed.calls.lock().unwrap();
        assert!(calls
            .iter()
            .any(|args| args.first().map(String::as_str) == Some("restore")
                && args.get(1).map(String::as_str) == Some("dev:c000000000000000000000000000000")));
    }
    #[test]
    fn failed_child_cleanup_requires_the_saved_attempt_label() {
        let _test_state = crate::test_support::global_state();
        struct WrongChild {
            calls: Mutex<Vec<Vec<String>>>,
        }
        impl RuntimeRunner for WrongChild {
            fn run(
                &self,
                _paths: &RuntimePaths,
                args: &[String],
                _timeout: Duration,
            ) -> Result<CommandOutput, RuntimeError> {
                self.calls.lock().unwrap().push(args.to_vec());
                let stdout = match args[0].as_str() {
                    "snapshot" => serde_json::json!([{"group":"source","name":"c000000000000000000000000000000","scope":"full","availability":"ready"}]).to_string(),
                    "list" => serde_json::json!([{"name":"fork"}]).to_string(),
                    "inspect" => serde_json::json!({"name":"fork","status":"Stopped","config":{
                        "labels":{"silo.managed":"true","silo.machine-id":"00000000-0000-4000-8000-000000000002"},
                        "network":{"policy":{"default_egress":"deny","default_ingress":"allow","rules":[]}}
                    }}).to_string(),
                    _ => panic!("cleanup must not touch an unverified child"),
                };
                Ok(CommandOutput {
                    stdout,
                    stderr: String::new(),
                })
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        let mut child = computer_configuration();
        let child_id = "00000000-0000-4000-8000-000000000002";
        {
            let ComputerConfiguration { id, name, .. } = &mut child;
            *id = child_id.into();
            *name = "fork".into();
        }
        write_metadata(
            &paths.metadata,
            &ComputerConfigurationRequest {
                schema_version: 1,
                computers: vec![child.clone()],
            },
        )
        .unwrap();
        let mut record = Record::default();
        record.pending_checkpoint_restore = Some(PendingRestore {
            checkpoint_id: "c000000000000000000000000000000".into(),
            source_computer: "source".into(),
            state: "full".into(),
        });
        record.desired_network_policy =
            Some(serde_json::json!({"default_egress":"deny","default_ingress":"allow","rules":[]}));
        record.restore_attempted = true;
        record.restore_attempt_id = Some(uuid::Uuid::new_v4().to_string());
        save(&paths, child_id, &record).unwrap();
        let runner = WrongChild {
            calls: Mutex::new(Vec::new()),
        };
        assert!(start_pending(&runner, &paths, &child)
            .unwrap_err()
            .to_string()
            .contains("unverified"));
        assert!(!runner
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|args| args[0] == "remove"));
    }
    #[test]
    fn restore_secures_recovery_before_retiring_original_and_keeps_stable_identity() {
        let _test_state = crate::test_support::global_state();
        struct RestoreRunner {
            state: Mutex<&'static str>,
            recovery: Mutex<Option<String>>,
            calls: Mutex<Vec<Vec<String>>>,
        }
        impl RuntimeRunner for RestoreRunner {
            fn run(
                &self,
                _paths: &RuntimePaths,
                args: &[String],
                _timeout: Duration,
            ) -> Result<CommandOutput, RuntimeError> {
                self.calls.lock().unwrap().push(args.to_vec());
                let stdout=match args[0].as_str() {
                    "inspect" => serde_json::json!({"name":"dev","status":*self.state.lock().unwrap(),"config":{
                        "labels":{"silo.managed":"true","silo.machine-id":ID},
                        "mounts":[{"guest":"/workspace","type":"Owned","storage":{"kind":"disk","capacity_mib":1024}}],
                        "network":{"policy":{"default_egress":"deny","default_ingress":"allow","rules":[]}}
                    }}).to_string(),
                    "snapshot" if args[1]=="list" => {
                        let mut entries=vec![serde_json::json!({"group":"dev","name":"c000000000000000000000000000000","scope":"full","availability":"ready"})];
                        if let Some(id)=self.recovery.lock().unwrap().as_ref() {
                            entries.push(serde_json::json!({"group":"dev","name":id,"scope":"full","availability":"ready"}));
                        }
                        serde_json::to_string(&entries).unwrap()
                    }
                    "snapshot" if args[1]=="create" => { *self.recovery.lock().unwrap()=Some(args[2].clone()); String::new() }
                    "snapshot" if args[1]=="verify" => String::new(),
                    "pause" => { *self.state.lock().unwrap()="Paused"; String::new() },
                    "stop" => { assert!(args.iter().any(|arg| arg=="--force")); *self.state.lock().unwrap()="Stopped"; String::new() },
                    "remove" => { *self.state.lock().unwrap()="Removed"; String::new() },
                    "list" => "[]".into(),
                    _ => panic!("unexpected runtime command: {args:?}"),
                };
                Ok(CommandOutput {
                    stdout,
                    stderr: String::new(),
                })
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        write_metadata(
            &paths.metadata,
            &ComputerConfigurationRequest {
                schema_version: 1,
                computers: vec![computer_configuration()],
            },
        )
        .unwrap();
        let mut record = Record::default();
        record.checkpoints.push(Checkpoint {
            id: "c000000000000000000000000000000".into(),
            native_id: None,
            name: "Selected".into(),
            created_at: 1,
            scope: "full".into(),
            reason: "manual".into(),
        });
        save(&paths, ID, &record).unwrap();
        let runner = RestoreRunner {
            state: Mutex::new("Running"),
            recovery: Mutex::new(None),
            calls: Mutex::new(Vec::new()),
        };
        restore_with(&runner, &paths, ID, "c000000000000000000000000000000").unwrap();
        let calls = runner.calls.lock().unwrap();
        let position = |command: &str| calls.iter().position(|args| args[0] == command).unwrap();
        assert!(position("pause") < position("stop"));
        assert!(position("snapshot") < position("remove"));
        let verify_recovery = calls
            .iter()
            .rposition(|args| args.get(1).map(String::as_str) == Some("verify"))
            .unwrap();
        assert!(verify_recovery < position("remove"));
        let stored = load(&paths, ID).unwrap();
        assert_eq!(stored.checkpoints[0].reason, "before-restore");
        assert_eq!(
            stored.pending_checkpoint_restore.unwrap().checkpoint_id,
            "c000000000000000000000000000000"
        );
        assert!(stored.restore_journal.is_none());
        assert_eq!(
            read_metadata(&paths.metadata).unwrap().computers[0].id(),
            ID
        );
        assert_eq!(
            read_metadata(&paths.metadata).unwrap().computers[0].name(),
            "dev"
        );
        drop(calls);
        let source = read_application_state_with(&runner, &paths).unwrap();
        assert!(matches!(source.computers[0].state, ComputerState::Stopped));
        assert!(source.computers[0].pending_checkpoint_restore.is_some());
        let calls = runner.calls.lock().unwrap();
        let remove = calls.iter().position(|args| args[0] == "remove").unwrap();
        assert!(calls[remove + 1..].iter().all(|args| args[0] == "list"));
    }

    struct JournalRunner {
        state: Mutex<&'static str>,
        fail: &'static str,
        listed: bool,
        recovery: Mutex<Option<String>>,
        calls: Mutex<Vec<Vec<String>>>,
    }
    impl RuntimeRunner for JournalRunner {
        fn run(
            &self,
            _paths: &RuntimePaths,
            args: &[String],
            _timeout: Duration,
        ) -> Result<CommandOutput, RuntimeError> {
            self.calls.lock().unwrap().push(args.to_vec());
            let command = if args[0] == "snapshot" {
                args[1].as_str()
            } else {
                args[0].as_str()
            };
            if self.fail.split('|').any(|failing| failing == command) {
                return Err(RuntimeError::Unavailable(format!(
                    "{command} failed on this host."
                )));
            }
            let stdout = match command {
                "inspect" => serde_json::json!({"name":"dev","status":*self.state.lock().unwrap(),"config":{
                    "labels":{"silo.managed":"true","silo.machine-id":ID},
                    "mounts":[{"guest":"/workspace","type":"Owned","storage":{"kind":"disk","capacity_mib":1024}}],
                    "network":{"policy":{"default_egress":"deny","default_ingress":"allow","rules":[]}}
                }}).to_string(),
                "list" if args[0] == "snapshot" => {
                    let full = self.calls.lock().unwrap().iter().any(|call| call.iter().any(|arg| arg == "--full"));
                    let mut entries = vec![serde_json::json!({"group":"dev","name":"c000000000000000000000000000000","scope":"full","availability":"ready"})];
                    if let Some(id) = self.recovery.lock().unwrap().as_ref() {
                        let scope = if full { "full" } else { "disk" };
                        entries.push(serde_json::json!({"group":"dev","name":id,"scope":scope,"availability":"ready"}));
                    }
                    serde_json::to_string(&entries).unwrap()
                }
                "create" => {
                    *self.recovery.lock().unwrap() = Some(args[2].clone());
                    String::new()
                }
                "pause" => {
                    *self.state.lock().unwrap() = "Paused";
                    String::new()
                }
                "resume" => {
                    *self.state.lock().unwrap() = "Running";
                    String::new()
                }
                "stop" => {
                    *self.state.lock().unwrap() = "Stopped";
                    String::new()
                }
                "remove" => {
                    *self.state.lock().unwrap() = "Removed";
                    String::new()
                }
                "list" if self.listed => r#"[{"name":"dev"}]"#.into(),
                "list" => "[]".into(),
                _ => String::new(),
            };
            Ok(CommandOutput {
                stdout,
                stderr: String::new(),
            })
        }
    }

    fn journal_runner(state: &'static str, fail: &'static str) -> JournalRunner {
        JournalRunner {
            state: Mutex::new(state),
            fail,
            listed: false,
            recovery: Mutex::new(None),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn restore_fixture(
        directory: &tempfile::TempDir,
        journal: Option<(&str, bool)>,
    ) -> RuntimePaths {
        let paths = paths(directory);
        write_metadata(
            &paths.metadata,
            &ComputerConfigurationRequest {
                schema_version: 1,
                computers: vec![computer_configuration()],
            },
        )
        .unwrap();
        let mut record = Record::default();
        record.checkpoints.push(Checkpoint {
            id: "c000000000000000000000000000000".into(),
            native_id: None,
            name: "Selected".into(),
            created_at: 1,
            scope: "full".into(),
            reason: "manual".into(),
        });
        if let Some((phase, prior_running)) = journal {
            record.restore_journal = Some(RestoreJournal {
                target_checkpoint_id: "c000000000000000000000000000000".into(),
                recovery_checkpoint: Checkpoint {
                    id: "c111111111111111111111111111111".into(),
                    native_id: None,
                    name: "Before restore".into(),
                    created_at: 2,
                    scope: if prior_running { "full" } else { "disk" }.into(),
                    reason: "before-restore".into(),
                },
                prior_running,
                phase: phase.into(),
            });
            record.checkpoint_operation = Some(Operation {
                kind: "restore".into(),
                status: "running".into(),
                stage: "Creating recovery checkpoint".into(),
                error: None,
            });
        }
        save(&paths, ID, &record).unwrap();
        paths
    }

    #[test]
    fn capturing_journal_for_a_stopped_computer_secures_a_disk_recovery_and_finishes() {
        let _test_state = crate::test_support::global_state();
        for status in ["Stopped", "Created", "Crashed"] {
            let directory = tempfile::tempdir().unwrap();
            let paths = restore_fixture(&directory, Some(("capturing", true)));
            let runner = journal_runner(status, "");
            restore_with(&runner, &paths, ID, "c000000000000000000000000000000").unwrap();
            let calls = runner.calls.lock().unwrap();
            let create = calls
                .iter()
                .find(|call| call[0] == "snapshot" && call[1] == "create")
                .unwrap();
            assert!(
                !create.iter().any(|arg| arg == "--full"),
                "{status}: {create:?}"
            );
            assert!(!calls.iter().any(|call| call[0] == "pause"));
            let stored = load(&paths, ID).unwrap();
            assert!(stored.restore_journal.is_none());
            assert!(stored.pending_checkpoint_restore.is_some());
            let recovery = stored
                .checkpoints
                .iter()
                .find(|checkpoint| checkpoint.reason == "before-restore")
                .unwrap();
            assert_eq!(recovery.scope, "disk");
        }
    }

    #[test]
    fn failed_pause_clears_the_capturing_journal_and_records_the_real_error() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = restore_fixture(&directory, None);
        let runner = journal_runner("Running", "pause");
        let failure = restore_with(&runner, &paths, ID, "c000000000000000000000000000000")
            .unwrap_err()
            .to_string();
        assert_eq!(failure, "pause failed on this host.");
        let stored = load(&paths, ID).unwrap();
        assert!(stored.restore_journal.is_none());
        let operation = stored.checkpoint_operation.unwrap();
        assert_eq!(operation.status, "failed");
        assert_eq!(
            operation.error.as_deref(),
            Some("pause failed on this host.")
        );
        assert!(!needs_explicit_start(&paths, ID).unwrap());
    }

    #[test]
    fn failed_capture_of_a_stopped_computer_leaves_no_journal() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = restore_fixture(&directory, Some(("capturing", true)));
        let runner = journal_runner("Stopped", "create");
        restore_with(&runner, &paths, ID, "c000000000000000000000000000000").unwrap_err();
        let stored = load(&paths, ID).unwrap();
        assert!(stored.restore_journal.is_none());
        assert_eq!(stored.checkpoint_operation.unwrap().status, "failed");
    }

    #[test]
    fn secured_restore_errors_persist_a_failed_status_with_the_real_error() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = restore_fixture(&directory, Some(("secured", true)));
        let mut runner = journal_runner("Running", "");
        runner.listed = true;
        let failure = restore_with(&runner, &paths, ID, "c000000000000000000000000000000")
            .unwrap_err()
            .to_string();
        assert!(
            failure.contains("resumed after its recovery checkpoint"),
            "{failure}"
        );
        let operation = load(&paths, ID).unwrap().checkpoint_operation.unwrap();
        assert_eq!(operation.status, "failed");
        assert_eq!(operation.error.as_deref(), Some(failure.as_str()));
    }

    #[test]
    fn crashed_computer_can_retry_a_secured_restore() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = restore_fixture(&directory, Some(("secured", true)));
        let mut runner = journal_runner("Crashed", "");
        runner.listed = true;
        restore_with(&runner, &paths, ID, "c000000000000000000000000000000").unwrap();
        let stored = load(&paths, ID).unwrap();
        assert!(stored.restore_journal.is_none());
        assert!(stored.pending_checkpoint_restore.is_some());
    }

    #[test]
    fn interrupted_checkpoint_is_kept_when_the_snapshot_list_fails() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = restore_fixture(&directory, None);
        let mut record = load(&paths, ID).unwrap();
        record.snapshot_group = Some("dev".into());
        record.inflight_checkpoint = Some(Checkpoint {
            id: "c222222222222222222222222222222".into(),
            native_id: None,
            name: "Interrupted".into(),
            created_at: 3,
            scope: "full".into(),
            reason: "manual".into(),
        });
        save(&paths, ID, &record).unwrap();
        let runner = journal_runner("Running", "list");
        capture_with(&runner, &paths, ID, "Next", "manual").unwrap_err();
        let stored = load(&paths, ID).unwrap();
        assert_eq!(
            stored
                .inflight_checkpoint
                .map(|checkpoint| checkpoint.id)
                .as_deref(),
            Some("c222222222222222222222222222222")
        );
    }

    #[test]
    fn attempted_restore_with_a_listed_runtime_reads_as_a_present_computer() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = restore_fixture(&directory, None);
        let mut record = load(&paths, ID).unwrap();
        record.pending_checkpoint_restore = Some(PendingRestore {
            checkpoint_id: "c000000000000000000000000000000".into(),
            source_computer: "dev".into(),
            state: "full".into(),
        });
        save(&paths, ID, &record).unwrap();
        assert!(pending_view(&paths, ID, false).unwrap());
        assert!(pending_view(&paths, ID, true).unwrap());
        record.restore_attempted = true;
        record.restore_attempt_id = Some(uuid::Uuid::new_v4().to_string());
        save(&paths, ID, &record).unwrap();
        assert!(pending_view(&paths, ID, false).unwrap());
        assert!(!pending_view(&paths, ID, true).unwrap());
        let record = load(&paths, ID).unwrap();
        assert!(view_pending(&record, "dev").is_some());
        assert!(needs_explicit_start(&paths, ID).unwrap());
    }

    #[test]
    fn failure_record_save_errors_keep_the_original_error() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = paths(&directory);
        fs::write(directory.path().join("checkpoints"), b"not a directory").unwrap();
        let failure = save_failure(
            &paths,
            ID,
            &Record::default(),
            RuntimeError::Unavailable("No space left on device.".into()),
        )
        .to_string();
        assert!(failure.starts_with("No space left on device."), "{failure}");
        assert!(failure.contains("could not be updated"));
    }

    struct FakeAssignments {
        fail: &'static [&'static str],
        github: Mutex<Vec<String>>,
        secrets: Mutex<Vec<String>>,
    }
    impl FakeAssignments {
        fn new(fail: &'static [&'static str]) -> Self {
            Self {
                fail,
                github: Mutex::new(Vec::new()),
                secrets: Mutex::new(Vec::new()),
            }
        }
        fn check(&self, step: &str) -> Result<(), String> {
            if self.fail.contains(&step) {
                Err(format!("{step} failed."))
            } else {
                Ok(())
            }
        }
    }
    impl ForkAssignments for FakeAssignments {
        fn copy_github(&self, _: &str, target: &str) -> Result<(), String> {
            self.check("copy_github")?;
            self.github.lock().unwrap().push(target.into());
            Ok(())
        }
        fn forget_github(&self, target: &str) -> Result<(), String> {
            self.check("forget_github")?;
            self.github.lock().unwrap().retain(|name| name != target);
            Ok(())
        }
        fn copy_secrets(&self, _: &str, target: &str) -> Result<(), String> {
            self.check("copy_secrets")?;
            self.secrets.lock().unwrap().push(target.into());
            Ok(())
        }
        fn forget_secrets(&self, target: &str) -> Result<(), String> {
            self.check("forget_secrets")?;
            self.secrets.lock().unwrap().retain(|name| name != target);
            Ok(())
        }
    }

    fn fork_fixture(directory: &tempfile::TempDir) -> (RuntimePaths, ForkSource) {
        let paths = restore_fixture(directory, None);
        let mut record = load(&paths, ID).unwrap();
        record.snapshot_group = Some("dev".into());
        save(&paths, ID, &record).unwrap();
        let fork = ForkSource {
            source_id: ID.into(),
            snapshot_group: "dev".into(),
            member: "c000000000000000000000000000000".into(),
            scope: "full".into(),
            desired_policy: serde_json::json!({"default_egress":"deny","default_ingress":"allow","rules":[]}),
        };
        (paths, fork)
    }

    fn record_ids(paths: &RuntimePaths) -> Vec<String> {
        let mut ids: Vec<String> = fs::read_dir(directory(paths))
            .unwrap()
            .filter_map(|entry| {
                entry
                    .unwrap()
                    .file_name()
                    .to_str()?
                    .strip_suffix(".json")
                    .map(str::to_owned)
            })
            .collect();
        ids.sort();
        ids
    }

    #[test]
    fn fork_commit_adds_the_stopped_fork_and_its_assignments() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let (paths, fork) = fork_fixture(&directory);
        crate::computer_use::set_approval(&paths, ID, crate::computer_use::Approval::Auto).unwrap();
        let assignments = FakeAssignments::new(&[]);
        fork_commit(
            &journal_runner("Running", ""),
            &paths,
            &assignments,
            &fork,
            "branch",
        )
        .unwrap();
        let metadata = read_metadata(&paths.metadata).unwrap();
        let child = metadata
            .computers
            .iter()
            .find(|configuration| configuration.name() == "branch")
            .unwrap();
        assert_eq!(
            crate::computer_use::settings(&paths, child.id()).approval,
            crate::computer_use::Approval::Auto
        );
        let pending = load(&paths, child.id())
            .unwrap()
            .pending_checkpoint_restore
            .unwrap();
        assert_eq!(pending.checkpoint_id, fork.member);
        assert_eq!(pending.source_computer, "dev");
        assert_eq!(*assignments.github.lock().unwrap(), ["branch"]);
        assert_eq!(*assignments.secrets.lock().unwrap(), ["branch"]);
    }

    #[test]
    fn a_failed_approval_copy_does_not_publish_the_fork_or_leave_its_record() {
        let _test_state = crate::test_support::global_state();
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let (paths, fork) = fork_fixture(&directory);
        crate::computer_use::set_approval(&paths, ID, crate::computer_use::Approval::Auto).unwrap();
        let policy_directory = paths.metadata.with_file_name("computer-use");
        fs::set_permissions(&policy_directory, fs::Permissions::from_mode(0o500)).unwrap();
        let assignments = FakeAssignments::new(&[]);
        let result = fork_commit(
            &journal_runner("Running", ""),
            &paths,
            &assignments,
            &fork,
            "branch",
        );
        fs::set_permissions(&policy_directory, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("computer-use setting"));
        assert_eq!(record_ids(&paths), [ID]);
        assert_eq!(read_metadata(&paths.metadata).unwrap().computers.len(), 1);
        assert!(assignments.github.lock().unwrap().is_empty());
        assert!(assignments.secrets.lock().unwrap().is_empty());
        assert_eq!(
            crate::computer_use::settings(&paths, ID).approval,
            crate::computer_use::Approval::Auto
        );
        assert_eq!(fs::read_dir(&policy_directory).unwrap().count(), 1);
    }

    #[test]
    fn a_published_fork_keeps_its_checkpoint_and_assignments_after_a_durability_error() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let (paths, fork) = fork_fixture(&directory);
        let assignments = FakeAssignments::new(&[]);
        let failure = fork_commit_with_metadata_writer(
            &journal_runner("Running", ""),
            &paths,
            &assignments,
            &fork,
            "branch",
            &|path, metadata| {
                write_metadata(path, metadata)?;
                Err(RuntimeError::Unavailable("Directory sync failed.".into()))
            },
        )
        .unwrap_err()
        .to_string();
        assert!(failure.starts_with("Directory sync failed."), "{failure}");
        let metadata = read_metadata(&paths.metadata).unwrap();
        let child = metadata
            .computers
            .iter()
            .find(|configuration| configuration.name() == "branch")
            .unwrap();
        let record = load(&paths, child.id()).unwrap();
        assert_eq!(
            record.pending_checkpoint_restore.unwrap().checkpoint_id,
            fork.member
        );
        assert_eq!(record.desired_network_policy, Some(fork.desired_policy));
        assert_eq!(*assignments.github.lock().unwrap(), ["branch"]);
        assert_eq!(*assignments.secrets.lock().unwrap(), ["branch"]);
    }

    #[test]
    fn an_unreadable_inventory_keeps_fork_dependencies_after_a_write_error() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let (paths, fork) = fork_fixture(&directory);
        let assignments = FakeAssignments::new(&[]);
        let failure = fork_commit_with_metadata_writer(
            &journal_runner("Running", ""),
            &paths,
            &assignments,
            &fork,
            "branch",
            &|path, _| {
                fs::write(path, b"{broken").unwrap();
                Err(RuntimeError::Unavailable("Inventory write failed.".into()))
            },
        )
        .unwrap_err()
        .to_string();
        assert!(failure.starts_with("Inventory write failed."), "{failure}");
        assert!(failure.contains("could not be checked"), "{failure}");
        let ids = record_ids(&paths);
        assert_eq!(ids.len(), 2);
        let child_id = ids.iter().find(|id| id.as_str() != ID).unwrap();
        assert_eq!(
            load(&paths, child_id)
                .unwrap()
                .pending_checkpoint_restore
                .unwrap()
                .checkpoint_id,
            fork.member
        );
        assert_eq!(*assignments.github.lock().unwrap(), ["branch"]);
        assert_eq!(*assignments.secrets.lock().unwrap(), ["branch"]);
    }

    #[test]
    fn failed_fork_steps_leave_no_record_inventory_or_assignments_and_keep_the_original_error() {
        let _test_state = crate::test_support::global_state();
        for (fail, expected) in [
            (&["copy_github"][..], "copy_github failed."),
            (&["copy_secrets"][..], "copy_secrets failed."),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let (paths, fork) = fork_fixture(&directory);
            let assignments = FakeAssignments::new(fail);
            let failure = fork_commit(
                &journal_runner("Running", ""),
                &paths,
                &assignments,
                &fork,
                "branch",
            )
            .unwrap_err()
            .to_string();
            assert_eq!(failure, expected);
            assert_eq!(record_ids(&paths), [ID]);
            assert_eq!(read_metadata(&paths.metadata).unwrap().computers.len(), 1);
            assert!(assignments.github.lock().unwrap().is_empty());
            assert!(assignments.secrets.lock().unwrap().is_empty());
        }
    }

    #[test]
    fn a_failing_fork_cleanup_is_reported_after_the_original_error() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let (paths, fork) = fork_fixture(&directory);
        let assignments = FakeAssignments::new(&["copy_secrets", "forget_github"]);
        let failure = fork_commit(
            &journal_runner("Running", ""),
            &paths,
            &assignments,
            &fork,
            "branch",
        )
        .unwrap_err()
        .to_string();
        assert!(failure.starts_with("copy_secrets failed."), "{failure}");
        assert!(
            failure.contains("Cleanup was incomplete: forget_github failed."),
            "{failure}"
        );
        // The record is still removed although the GitHub cleanup failed.
        assert_eq!(record_ids(&paths), [ID]);
    }

    #[test]
    fn a_failed_inventory_write_removes_the_fork_record_and_assignments() {
        let _test_state = crate::test_support::global_state();
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let (paths, fork) = fork_fixture(&directory);
        crate::computer_use::set_approval(&paths, ID, crate::computer_use::Approval::Auto).unwrap();
        let parent = paths.metadata.parent().unwrap().to_path_buf();
        // The checkpoint and approval directories exist, so only the inventory write fails.
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o555)).unwrap();
        let assignments = FakeAssignments::new(&[]);
        let result = fork_commit(
            &journal_runner("Running", ""),
            &paths,
            &assignments,
            &fork,
            "branch",
        );
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("computer settings"));
        assert_eq!(record_ids(&paths), [ID]);
        assert_eq!(read_metadata(&paths.metadata).unwrap().computers.len(), 1);
        assert!(assignments.github.lock().unwrap().is_empty());
        assert!(assignments.secrets.lock().unwrap().is_empty());
        assert_eq!(
            fs::read_dir(paths.metadata.with_file_name("computer-use"))
                .unwrap()
                .count(),
            1
        );
    }

    #[test]
    fn fork_commit_rechecks_the_name_and_the_checkpoint_after_the_capture() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let (paths, fork) = fork_fixture(&directory);
        let assignments = FakeAssignments::new(&[]);
        assert!(fork_commit(
            &journal_runner("Running", ""),
            &paths,
            &assignments,
            &fork,
            "dev"
        )
        .is_err());
        let missing = ForkSource {
            member: "c999999999999999999999999999999".into(),
            ..fork
        };
        assert!(fork_commit(
            &journal_runner("Running", ""),
            &paths,
            &assignments,
            &missing,
            "branch"
        )
        .is_err());
        assert_eq!(record_ids(&paths), [ID]);
        assert!(assignments.github.lock().unwrap().is_empty());
    }

    #[test]
    fn current_state_fork_captures_under_the_source_lane_and_writes_under_the_device_lane() {
        let _test_state = crate::test_support::global_state();
        struct LaneRunner {
            gate: &'static super::super::operation_gate::OperationGate,
            created: Mutex<Option<String>>,
            lanes: Mutex<Vec<(String, Option<String>, bool)>>,
        }
        impl RuntimeRunner for LaneRunner {
            fn run(
                &self,
                _: &RuntimePaths,
                args: &[String],
                _: Duration,
            ) -> Result<CommandOutput, RuntimeError> {
                let running = self.gate.snapshot().running;
                let lane = running.first().and_then(|entry| entry.computer_id.clone());
                // Another computer can start while this lane is held only if it is not device-wide.
                let other_computer_free = std::thread::scope(|scope| {
                    scope
                        .spawn(|| {
                            self.gate
                                .try_computer("other-computer", "other", "Starting")
                                .is_ok()
                        })
                        .join()
                        .unwrap()
                });
                let command = if args[0] == "snapshot" {
                    format!("snapshot {}", args[1])
                } else {
                    args[0].clone()
                };
                self.lanes
                    .lock()
                    .unwrap()
                    .push((command.clone(), lane, other_computer_free));
                let stdout = match command.as_str() {
                    "inspect" => serde_json::json!({"name":"dev","status":"Running","config":{
                        "labels":{"silo.managed":"true","silo.machine-id":ID},
                        "mounts":[{"guest":"/workspace","type":"Owned","storage":{"kind":"disk","capacity_mib":1024}}],
                        "network":{"policy":{"default_egress":"deny","default_ingress":"allow","rules":[]}}
                    }}).to_string(),
                    "snapshot create" => {
                        *self.created.lock().unwrap() = Some(args[2].clone());
                        String::new()
                    }
                    "snapshot list" => {
                        let name = self.created.lock().unwrap().clone().unwrap_or_default();
                        serde_json::json!([{"group":"dev","name":name,"scope":"full","availability":"ready"}]).to_string()
                    }
                    _ => String::new(),
                };
                Ok(CommandOutput {
                    stdout,
                    stderr: String::new(),
                })
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let (paths, _) = fork_fixture(&directory);
        let gate: &'static super::super::operation_gate::OperationGate =
            Box::leak(Box::new(super::super::operation_gate::OperationGate::new()));
        let runner = LaneRunner {
            gate,
            created: Mutex::new(None),
            lanes: Mutex::new(Vec::new()),
        };
        let assignments = FakeAssignments::new(&[]);
        fork_in_lanes(
            gate,
            &runner,
            &paths,
            &assignments,
            ID,
            None,
            "branch",
            &Vec::new,
        )
        .unwrap();
        let lanes = runner.lanes.lock().unwrap();
        let create = lanes
            .iter()
            .find(|(command, ..)| command == "snapshot create")
            .unwrap();
        assert_eq!(
            create.1.as_deref(),
            Some(ID),
            "the capture holds only the source's lane"
        );
        assert!(
            create.2,
            "other computers are not queued behind the capture"
        );
        let last = lanes.last().unwrap();
        assert_eq!(last.0, "snapshot list");
        assert_eq!(
            last.1, None,
            "the inventory write holds the device-wide lane"
        );
        assert!(!last.2);
        assert!(read_metadata(&paths.metadata)
            .unwrap()
            .computers
            .iter()
            .any(|configuration| configuration.name() == "branch"));
    }

    /// A fake native store that enforces MicroSandbox's children and head guards.
    struct Store {
        members: Mutex<Vec<(String, String, String, Option<String>)>>,
        heads: Mutex<HashMap<String, String>>,
        computers: Vec<(&'static str, Option<&'static str>)>,
        fail_create_after_publish: bool,
        fail_remove: bool,
        calls: Mutex<Vec<Vec<String>>>,
    }
    impl Store {
        fn new(computers: Vec<(&'static str, Option<&'static str>)>) -> Self {
            Self {
                members: Mutex::new(Vec::new()),
                heads: Mutex::new(HashMap::new()),
                computers,
                fail_create_after_publish: false,
                fail_remove: false,
                calls: Mutex::new(Vec::new()),
            }
        }
        fn with(self, group: &str, name: &str, id: &str, parent: Option<&str>) -> Self {
            self.members.lock().unwrap().push((
                group.into(),
                name.into(),
                id.into(),
                parent.map(str::to_owned),
            ));
            self.heads.lock().unwrap().insert(group.into(), id.into());
            self
        }
        fn names(&self) -> Vec<String> {
            self.members
                .lock()
                .unwrap()
                .iter()
                .map(|member| member.1.clone())
                .collect()
        }
        fn removals(&self) -> Vec<String> {
            self.calls
                .lock()
                .unwrap()
                .iter()
                .filter(|call| call[0] == "snapshot" && call[1] == "remove")
                .map(|call| call[2].clone())
                .collect()
        }
    }
    impl RuntimeRunner for Store {
        fn run(
            &self,
            paths: &RuntimePaths,
            args: &[String],
            _: Duration,
        ) -> Result<CommandOutput, RuntimeError> {
            self.calls.lock().unwrap().push(args.to_vec());
            let ok = |stdout: String| {
                Ok(CommandOutput {
                    stdout,
                    stderr: String::new(),
                })
            };
            match (args[0].as_str(), args.get(1).map(String::as_str)) {
                ("list", _) => ok(serde_json::Value::Array(self.computers.iter().map(|(name, _)| serde_json::json!({"name": name})).collect()).to_string()),
                ("inspect", Some(name)) => {
                    let computer_id = read_metadata(&paths.metadata)?.computers.into_iter()
                        .find(|configuration| configuration.name() == name)
                        .map(|configuration| configuration.id().to_owned())
                        .unwrap_or_else(|| ID.into());
                    let parent = self.computers.iter().find(|(listed, _)| listed == &name).and_then(|(_, parent)| *parent);
                    ok(serde_json::json!({"name": name, "status": "Running", "config": {
                        "labels": {"silo.managed": "true", "silo.machine-id": computer_id},
                        "mounts": [{"guest": "/workspace", "type": "Owned", "storage": {"kind": "disk", "capacity_mib": 1024}}],
                        "network": {"policy": {"default_egress": "deny", "default_ingress": "allow", "rules": []}},
                        "snapshot_parent": parent,
                    }}).to_string())
                }
                ("snapshot", Some("list")) => ok(serde_json::Value::Array(self.members.lock().unwrap().iter().map(|(group, name, id, parent)| serde_json::json!({
                    "snapshot_id": id, "name": name, "group": group, "parent_digest": parent,
                    "scope": "full", "availability": "ready", "created_at": "2026-01-01T00:00:00+00:00",
                })).collect()).to_string()),
                ("snapshot", Some("head")) => {
                    let selector = &args[2];
                    if let Some((group, id)) = selector.split_once(':') {
                        self.heads.lock().unwrap().insert(group.into(), id.into());
                    }
                    let group = selector.split(':').next().unwrap();
                    ok(serde_json::json!({"group": group, "head": self.heads.lock().unwrap().get(group)}).to_string())
                }
                ("snapshot", Some("remove")) => {
                    let (group, name) = args[2].split_once(':').unwrap();
                    let mut members = self.members.lock().unwrap();
                    let index = members.iter().position(|member| member.0 == group && member.1 == name)
                        .ok_or_else(|| RuntimeError::Unavailable("snapshot not found".into()))?;
                    let id = members[index].2.clone();
                    if self.fail_remove {
                        return Err(RuntimeError::Unavailable("permission denied".into()));
                    }
                    if members.iter().any(|member| member.3.as_deref() == Some(id.as_str())) {
                        return Err(RuntimeError::Unavailable("snapshot has indexed children; pass --force".into()));
                    }
                    let mut heads = self.heads.lock().unwrap();
                    if heads.get(group) == Some(&id) {
                        if members.iter().filter(|member| member.0 == group).count() > 1 {
                            return Err(RuntimeError::Unavailable("cannot remove current head".into()));
                        }
                        heads.remove(group);
                    }
                    members.remove(index);
                    ok(String::new())
                }
                ("snapshot", Some("create")) => {
                    let group = args[args.iter().position(|arg| arg == "--group").unwrap() + 1].clone();
                    let id = format!("snap_{:032x}", self.members.lock().unwrap().len() + 100);
                    self.members.lock().unwrap().push((group.clone(), args[2].clone(), id.clone(), None));
                    self.heads.lock().unwrap().entry(group).or_insert(id);
                    if self.fail_create_after_publish {
                        return Err(RuntimeError::Cancelled { operation: "snapshot create".into() });
                    }
                    ok(String::new())
                }
                _ => ok(String::new()),
            }
        }
    }

    const A: &str = "c000000000000000000000000000000";
    const B: &str = "c111111111111111111111111111111";
    const C: &str = "c222222222222222222222222222222";
    const FORK_ID: &str = "00000000-0000-4000-8000-000000000009";

    fn entry(id: &str, name: &str, reason: &str) -> Checkpoint {
        Checkpoint {
            id: id.into(),
            native_id: None,
            name: name.into(),
            created_at: 1,
            scope: "full".into(),
            reason: reason.into(),
        }
    }

    /// `dev` with checkpoints; `fork` adds a second configured computer and its record.
    fn delete_fixture(
        directory: &tempfile::TempDir,
        entries: Vec<Checkpoint>,
        fork: Option<Record>,
    ) -> RuntimePaths {
        let paths = paths(directory);
        let mut computers = vec![computer_configuration()];
        if let Some(record) = &fork {
            let ComputerConfiguration {
                cpus,
                max_cpus,
                memory_gib,
                max_memory_gib,
                workspace_storage_gib,
                runtime_storage_gib,
                desktop,
                ..
            } = computer_configuration();
            computers.push(ComputerConfiguration {
                id: FORK_ID.into(),
                name: "branch".into(),
                cpus,
                max_cpus,
                memory_gib,
                max_memory_gib,
                workspace_storage_gib,
                runtime_storage_gib,
                desktop,
            });
            save(&paths, FORK_ID, record).unwrap();
        }
        write_metadata(
            &paths.metadata,
            &ComputerConfigurationRequest {
                schema_version: 1,
                computers,
            },
        )
        .unwrap();
        let mut record = Record::default();
        record.snapshot_group = Some("dev".into());
        record.checkpoints = entries;
        save(&paths, ID, &record).unwrap();
        paths
    }

    fn cursor(paths: &RuntimePaths, computer: &str, snapshot: &str) {
        let directory = paths.home.join("sandboxes").join(computer);
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join("snapshot-lineage.json"),
            serde_json::json!({"computer_id": 7, "snapshot_id": snapshot}).to_string(),
        )
        .unwrap();
    }

    #[test]
    fn removal_plan_goes_leaves_first_and_keeps_used_positioned_and_parent_members() {
        let _test_state = crate::test_support::global_state();
        let member = |name: &str, id: &str, parent: Option<&str>| native::Member {
            snapshot_id: id.into(),
            name: Some(name.into()),
            group: Some("dev".into()),
            parent_digest: parent.map(str::to_owned),
            ..Default::default()
        };
        let inventory = vec![
            member(A, "snap_a", None),
            member(B, "snap_b", Some("snap_a")),
            member(C, "snap_c", Some("snap_b")),
        ];
        let all: HashSet<native::Key> = inventory.iter().filter_map(native::Member::key).collect();
        let order = |plan: native::Plan| {
            plan.remove
                .into_iter()
                .map(|member| member.snapshot_id)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            order(native::plan(
                &inventory,
                &all,
                &HashMap::new(),
                &HashMap::new()
            )),
            ["snap_c", "snap_b", "snap_a"]
        );

        let positioned = native::plan(
            &inventory,
            &all,
            &HashMap::new(),
            &HashMap::from([("snap_c".to_owned(), "dev".to_owned())]),
        );
        assert!(positioned.remove.is_empty());
        assert_eq!(positioned.kept.len(), 3);

        let used = HashMap::from([(
            ("dev".to_owned(), B.to_owned()),
            vec![native::Use {
                computer_id: FORK_ID.into(),
                computer: "branch".into(),
                purpose: native::Purpose::PendingStart,
            }],
        )]);
        let plan = native::plan(&inventory, &all, &used, &HashMap::new());
        assert_eq!(order(plan), ["snap_c"]);
    }

    #[test]
    fn deleting_a_recovery_checkpoint_moves_the_head_removes_its_member_and_drops_the_entry() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = delete_fixture(
            &directory,
            vec![
                entry(B, "Before restore", "before-restore"),
                entry(A, "Selected", "manual"),
            ],
            None,
        );
        // dev was restored from A; B was its previous instance's last capture and the group head.
        let store = Store::new(vec![("dev", Some("snap_a"))])
            .with("dev", A, "snap_a", None)
            .with("dev", B, "snap_b", Some("snap_a"));
        delete_checkpoint_with(&store, &paths, ID, B).unwrap();
        assert_eq!(store.names(), [A]);
        let calls = store.calls.lock().unwrap().clone();
        let moved = calls
            .iter()
            .position(|call| call[1] == "head" && call[2] == "dev:snap_a")
            .expect("the head moved first");
        let removed = calls.iter().position(|call| call[1] == "remove").unwrap();
        assert!(moved < removed);
        assert!(!calls
            .iter()
            .any(|call| call.iter().any(|arg| arg == "--force")));
        let record = load(&paths, ID).unwrap();
        assert_eq!(
            record
                .checkpoints
                .iter()
                .map(|checkpoint| checkpoint.id.as_str())
                .collect::<Vec<_>>(),
            [A]
        );

        // The checkpoint dev was restored from is what its next capture builds on.
        let failure = delete_checkpoint_with(&store, &paths, ID, A)
            .unwrap_err()
            .to_string();
        assert!(
            failure.contains("dev’s next checkpoint and export build on this one"),
            "{failure}"
        );
        assert_eq!(store.names(), [A]);
        assert_eq!(load(&paths, ID).unwrap().checkpoints.len(), 1);
    }

    #[test]
    fn checkpoints_that_forks_later_checkpoints_or_pending_starts_depend_on_are_refused() {
        let _test_state = crate::test_support::global_state();
        // A later checkpoint builds on this one.
        let directory = tempfile::tempdir().unwrap();
        let paths = delete_fixture(
            &directory,
            vec![
                entry(B, "After deploy", "manual"),
                entry(A, "Before deploy", "manual"),
            ],
            None,
        );
        let store = Store::new(vec![("dev", None)])
            .with("dev", A, "snap_a", None)
            .with("dev", B, "snap_b", Some("snap_a"));
        cursor(&paths, "dev", "snap_b");
        let failure = delete_checkpoint_with(&store, &paths, ID, A)
            .unwrap_err()
            .to_string();
        assert!(
            failure.contains(
                "“After deploy” was saved after this checkpoint and builds on it. Delete it first."
            ),
            "{failure}"
        );
        // The latest capture is what dev's next capture names as its parent.
        let failure = delete_checkpoint_with(&store, &paths, ID, B)
            .unwrap_err()
            .to_string();
        assert!(
            failure.contains("next checkpoint and export build on this one"),
            "{failure}"
        );
        assert!(store.removals().is_empty());

        // A fork that has not started yet starts from it.
        let directory = tempfile::tempdir().unwrap();
        let mut fork = Record::default();
        fork.snapshot_group = Some("dev".into());
        fork.pending_checkpoint_restore = Some(PendingRestore {
            checkpoint_id: A.into(),
            source_computer: "dev".into(),
            state: "full".into(),
        });
        let paths = delete_fixture(
            &directory,
            vec![entry(A, "Before deploy", "manual")],
            Some(fork),
        );
        let store = Store::new(vec![]).with("dev", A, "snap_a", None);
        let failure = delete_checkpoint_with(&store, &paths, ID, A)
            .unwrap_err()
            .to_string();
        assert_eq!(
            failure,
            "Used by branch. branch starts from it the next time it starts."
        );

        // A started fork still builds on the checkpoint it was restored from.
        let directory = tempfile::tempdir().unwrap();
        let mut started = Record::default();
        started.snapshot_group = Some("dev".into());
        let paths = delete_fixture(
            &directory,
            vec![entry(A, "Before deploy", "manual")],
            Some(started),
        );
        let store = Store::new(vec![("branch", Some("snap_a"))]).with("dev", A, "snap_a", None);
        let failure = delete_checkpoint_with(&store, &paths, ID, A)
            .unwrap_err()
            .to_string();
        assert!(
            failure.starts_with("Used by branch. branch was started from this checkpoint"),
            "{failure}"
        );

        // This computer restarts from it, or a Restore is unfinished.
        let directory = tempfile::tempdir().unwrap();
        let paths = delete_fixture(&directory, vec![entry(A, "Before deploy", "manual")], None);
        let mut record = load(&paths, ID).unwrap();
        record.pending_checkpoint_restore = Some(PendingRestore {
            checkpoint_id: A.into(),
            source_computer: "dev".into(),
            state: "full".into(),
        });
        save(&paths, ID, &record).unwrap();
        let store = Store::new(vec![]).with("dev", A, "snap_a", None);
        assert!(delete_checkpoint_with(&store, &paths, ID, A)
            .unwrap_err()
            .to_string()
            .contains("dev starts from this checkpoint the next time it starts"));
        assert!(store.removals().is_empty());
        assert_eq!(load(&paths, ID).unwrap().checkpoints.len(), 1);
    }

    #[test]
    fn a_refused_native_removal_keeps_the_checkpoint_and_says_why() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = delete_fixture(
            &directory,
            vec![
                entry(B, "Before restore", "before-restore"),
                entry(A, "Selected", "manual"),
            ],
            None,
        );
        let mut store = Store::new(vec![("dev", Some("snap_a"))])
            .with("dev", A, "snap_a", None)
            .with("dev", B, "snap_b", None);
        store.fail_remove = true;
        let failure = delete_checkpoint_with(&store, &paths, ID, B)
            .unwrap_err()
            .to_string();
        assert!(
            failure.starts_with("The checkpoint could not be deleted; it was kept."),
            "{failure}"
        );
        assert_eq!(load(&paths, ID).unwrap().checkpoints.len(), 2);
    }

    #[test]
    fn a_shared_or_missing_member_drops_only_the_entry() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let mut alias = entry(C, "Alias", "manual");
        alias.native_id = Some(A.into());
        let paths = delete_fixture(
            &directory,
            vec![
                alias,
                entry(A, "Before deploy", "manual"),
                entry(B, "Gone", "manual"),
            ],
            None,
        );
        let store = Store::new(vec![("dev", None)]).with("dev", A, "snap_a", None);
        delete_checkpoint_with(&store, &paths, ID, C).unwrap();
        delete_checkpoint_with(&store, &paths, ID, B).unwrap();
        assert!(store.removals().is_empty());
        assert_eq!(store.names(), [A]);
        assert_eq!(
            load(&paths, ID)
                .unwrap()
                .checkpoints
                .iter()
                .map(|checkpoint| checkpoint.id.as_str())
                .collect::<Vec<_>>(),
            [A]
        );
    }

    #[test]
    fn checkpoint_usage_skips_runtime_surveys_without_native_members() {
        let _test_state = crate::test_support::global_state();
        struct Fleet {
            store: Store,
            count: usize,
        }
        impl RuntimeRunner for Fleet {
            fn run(
                &self,
                paths: &RuntimePaths,
                args: &[String],
                timeout: Duration,
            ) -> Result<CommandOutput, RuntimeError> {
                if args[0] != "list" {
                    return self.store.run(paths, args, timeout);
                }
                self.store.calls.lock().unwrap().push(args.to_vec());
                Ok(CommandOutput {
                    stdout: serde_json::to_string(
                        &(0..self.count)
                            .map(|index| serde_json::json!({"name": format!("computer-{index}")}))
                            .collect::<Vec<_>>(),
                    )
                    .unwrap(),
                    stderr: String::new(),
                })
            }
        }
        let mut measured_calls = Vec::new();
        for count in [1, 10, 100] {
            for has_entry in [false, true] {
                let directory = tempfile::tempdir().unwrap();
                let entries = if has_entry {
                    vec![entry(A, "Missing", "manual")]
                } else {
                    Vec::new()
                };
                let paths = delete_fixture(&directory, entries, None);
                let fleet = Fleet {
                    store: Store::new(vec![]).with("other-group", A, "snap_other", None),
                    count,
                };
                let started = std::time::Instant::now();
                survey(&fleet, &paths).unwrap();
                let baseline = started.elapsed();
                assert_eq!(fleet.store.calls.lock().unwrap().len(), count + 2);
                fleet.store.calls.lock().unwrap().clear();
                let started = std::time::Instant::now();
                let usage = usage_with(&fleet, &paths, ID).unwrap();
                let elapsed = started.elapsed();
                let calls = fleet.store.calls.lock().unwrap();
                eprintln!("usage: computers={count} entry={has_entry} survey_calls={} survey={baseline:?} usage_calls={} usage={elapsed:?}", count + 2, calls.len());
                assert_eq!(usage.total_bytes, Some(0));
                assert_eq!(usage.checkpoints.len(), usize::from(has_entry));
                measured_calls.push((calls.len(), usize::from(has_entry)));
                if has_entry {
                    assert_eq!(calls[0], ["snapshot", "list", "--format", "json"]);
                    assert!(usage.checkpoints[0].delete_blocker.is_none());
                }
            }
        }
        for (actual, expected) in measured_calls {
            assert_eq!(actual, expected);
        }
        let directory = tempfile::tempdir().unwrap();
        let paths = delete_fixture(&directory, Vec::new(), Some(Record::default()));
        fs::write(path(&paths, FORK_ID), b"invalid history").unwrap();
        let store = Store::new(vec![]);
        let usage = usage_with(&store, &paths, ID).unwrap();
        assert_eq!(usage.total_bytes, Some(0));
        assert!(usage.checkpoints.is_empty());
        assert!(store.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn checkpoint_usage_keeps_saved_dependencies_when_native_member_is_missing() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let mut pending = Record::default();
        pending.pending_checkpoint_restore = Some(PendingRestore {
            source_computer: "dev".into(),
            checkpoint_id: A.into(),
            state: "full".into(),
        });
        let paths = delete_fixture(
            &directory,
            vec![entry(A, "Missing", "manual")],
            Some(pending),
        );
        let store = Store::new(vec![("branch", None)]);
        let usage = usage_with(&store, &paths, ID).unwrap();
        assert_eq!(usage.checkpoints[0].used_by, ["branch"]);
        assert!(usage.checkpoints[0]
            .delete_blocker
            .as_ref()
            .unwrap()
            .starts_with("Used by branch."));
        assert_eq!(store.calls.lock().unwrap().len(), 1);
    }

    #[test]
    fn checkpoint_storage_totals_lists_only_the_selected_lineage_group() {
        let _test_state = crate::test_support::global_state();
        for group in [Some("source"), None] {
            let directory = tempfile::tempdir().unwrap();
            let paths = delete_fixture(&directory, vec![entry(A, "Base", "manual")], None);
            let mut record = load(&paths, ID).unwrap();
            record.snapshot_group = group.map(str::to_owned);
            save(&paths, ID, &record).unwrap();
            let group = group.unwrap_or("dev");
            let store = Store::new(vec![]).with(group, A, "snap_a", None);
            assert_eq!(storage_totals(&store, &paths, ID, "dev"), (None, 1));
            assert_eq!(
                *store.calls.lock().unwrap(),
                [vec![
                    "snapshot", "list", "--group", group, "--format", "json"
                ]]
            );
        }
    }

    #[test]
    fn checkpoint_usage_keeps_unconfigured_lineage_and_cross_group_children() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = delete_fixture(&directory, vec![entry(A, "Base", "manual")], None);
        let store =
            Store::new(vec![("outside-silo", Some("snap_a"))]).with("dev", A, "snap_a", None);
        let usage = usage_with(&store, &paths, ID).unwrap();
        assert_eq!(usage.checkpoints[0].used_by, ["outside-silo"]);
        assert!(usage.checkpoints[0]
            .delete_blocker
            .as_ref()
            .unwrap()
            .starts_with("Used by outside-silo."));

        let store = Store::new(vec![]).with("dev", A, "snap_a", None).with(
            "another-group",
            B,
            "snap_b",
            Some("snap_a"),
        );
        let usage = usage_with(&store, &paths, ID).unwrap();
        assert!(usage.checkpoints[0]
            .delete_blocker
            .as_ref()
            .unwrap()
            .contains("A later saved state"));
    }

    #[test]
    fn checkpoint_usage_reports_sizes_users_and_blockers() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let mut started = Record::default();
        started.snapshot_group = Some("dev".into());
        let paths = delete_fixture(
            &directory,
            vec![
                entry(B, "Before restore", "before-restore"),
                entry(A, "Selected", "manual"),
            ],
            Some(started),
        );
        let store = Store::new(vec![("dev", Some("snap_a")), ("branch", Some("snap_a"))])
            .with("dev", A, "snap_a", None)
            .with("dev", B, "snap_b", None);
        let usage = usage_with(&store, &paths, ID).unwrap();
        let json = serde_json::to_value(&usage).unwrap();
        let find = |id: &str| {
            json["checkpoints"]
                .as_array()
                .unwrap()
                .iter()
                .find(|item| item["id"] == id)
                .unwrap()
                .clone()
        };
        assert!(find(B).get("deleteBlocker").is_none());
        assert_eq!(find(A)["usedBy"], serde_json::json!(["branch"]));
        assert!(find(A)["deleteBlocker"]
            .as_str()
            .unwrap()
            .starts_with("Used by branch."));
    }

    #[test]
    fn deleting_a_computer_removes_members_only_it_used_and_keeps_what_a_fork_builds_on() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let mut started = Record::default();
        started.snapshot_group = Some("dev".into());
        let paths = delete_fixture(
            &directory,
            vec![entry(B, "Later", "manual"), entry(A, "Base", "manual")],
            Some(started),
        );
        // dev is deleted: the inventory no longer lists it; its record is removed afterwards.
        let metadata = read_metadata(&paths.metadata).unwrap();
        write_metadata(
            &paths.metadata,
            &ComputerConfigurationRequest {
                schema_version: 1,
                computers: metadata
                    .computers
                    .into_iter()
                    .filter(|configuration| configuration.id() != ID)
                    .collect(),
            },
        )
        .unwrap();
        let store = Store::new(vec![("branch", Some("snap_a"))])
            .with("dev", A, "snap_a", None)
            .with("dev", B, "snap_b", Some("snap_a"))
            .with("dev", "silo-backup-0-1-2", "snap_x", Some("snap_b"))
            .with("elsewhere", C, "snap_c", None);
        remove_deleted_snapshots(&store, &paths, ID, "dev").unwrap();
        assert_eq!(
            store.removals(),
            [
                "dev:silo-backup-0-1-2",
                "dev:c111111111111111111111111111111"
            ]
        );
        assert_eq!(store.names(), [A, C]);
    }

    #[test]
    fn a_failed_capture_removes_its_published_member_unless_the_computer_builds_on_it() {
        let _test_state = crate::test_support::global_state();
        for builds_on_it in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let paths = delete_fixture(&directory, Vec::new(), None);
            let mut store = Store::new(vec![("dev", None)]);
            store.fail_create_after_publish = true;
            if builds_on_it {
                cursor(&paths, "dev", "snap_00000000000000000000000000000064");
            }
            capture_with(&store, &paths, ID, "Interrupted", "manual").unwrap_err();
            let record = load(&paths, ID).unwrap();
            if builds_on_it {
                assert_eq!(store.names().len(), 1, "a member dev builds on is kept");
                assert!(
                    record.inflight_checkpoint.is_some(),
                    "and stays reachable for reconciliation"
                );
            } else {
                assert!(store.names().is_empty());
                assert!(record.inflight_checkpoint.is_none());
            }
            assert_eq!(record.checkpoint_operation.unwrap().status, "failed");
        }
    }

    #[test]
    fn launch_cleanup_removes_only_the_capture_identified_by_its_journal() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = delete_fixture(&directory, vec![entry(A, "Kept", "manual")], None);
        let mut record = load(&paths, ID).unwrap();
        record.inflight_checkpoint = Some(entry(B, "Interrupted", "manual"));
        save(&paths, ID, &record).unwrap();
        let store = Store::new(vec![("dev", None)])
            .with("dev", A, "snap_a", None)
            .with("dev", B, "snap_b", None)
            .with("gone", C, "snap_c", None)
            .with("dev", "user-made", "snap_u", None);
        recover_interrupted(&store, &paths).unwrap();
        assert_eq!(store.names(), [A, C, "user-made"]);
        assert!(load(&paths, ID).unwrap().inflight_checkpoint.is_none());
        assert_eq!(load(&paths, ID).unwrap().checkpoints.len(), 1);
    }

    #[test]
    fn recovery_reports_unresolved_owner_and_continues_independent_captures() {
        let _test_state = crate::test_support::global_state();
        for damaged_history in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let mut fork = Record::default();
            fork.inflight_checkpoint = Some(entry(C, "Interrupted branch", "manual"));
            let paths = delete_fixture(&directory, vec![], Some(fork));
            let mut record = load(&paths, ID).unwrap();
            record.inflight_checkpoint = Some(entry(B, "Interrupted source", "manual"));
            save(&paths, ID, &record).unwrap();
            if damaged_history {
                fs::write(path(&paths, ID), b"{broken").unwrap();
            }
            fs::write(cleanup_path(&paths), b"{broken-cleanup").unwrap();
            let before = fs::read(path(&paths, ID)).unwrap();
            let mut store = Store::new(vec![]).with("dev", B, "snap_b", None);
            store.fail_remove = true;
            let recovery = recover_interrupted(&store, &paths).unwrap();
            assert_eq!(recovery.unresolved.len(), 1);
            assert!(recovery.unresolved.contains_key(ID));
            assert!(recovery.cleanup_error.is_some());
            assert_eq!(fs::read(cleanup_path(&paths)).unwrap(), b"{broken-cleanup");
            assert_eq!(fs::read(path(&paths, ID)).unwrap(), before);
            assert_eq!(store.names(), [B]);
            let recovered = load(&paths, FORK_ID).unwrap();
            assert!(recovered.inflight_checkpoint.is_none());
            assert_eq!(recovered.checkpoint_operation.unwrap().status, "failed");
            store.fail_remove = false;
            fs::remove_file(cleanup_path(&paths)).unwrap();
            if damaged_history {
                save(&paths, ID, &record).unwrap();
            }
            assert!(recover_interrupted(&store, &paths)
                .unwrap()
                .unresolved
                .is_empty());
            assert!(store.names().is_empty());
        }
    }

    #[test]
    fn deleting_the_last_dependent_removes_the_deleted_sources_kept_members() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let mut started = Record::default();
        started.snapshot_group = Some("dev".into());
        let paths = delete_fixture(&directory, vec![entry(A, "Base", "manual")], Some(started));
        let metadata = read_metadata(&paths.metadata).unwrap();
        write_metadata(
            &paths.metadata,
            &ComputerConfigurationRequest {
                schema_version: 1,
                computers: metadata
                    .computers
                    .into_iter()
                    .filter(|configuration| configuration.id() != ID)
                    .collect(),
            },
        )
        .unwrap();
        let mut store = Store::new(vec![("branch", Some("snap_a"))]).with("dev", A, "snap_a", None);
        remove_deleted_snapshots(&store, &paths, ID, "dev").unwrap();
        forget_removed(&paths, ID).unwrap();
        assert_eq!(store.names(), [A]);
        write_metadata(
            &paths.metadata,
            &ComputerConfigurationRequest {
                schema_version: 1,
                computers: vec![],
            },
        )
        .unwrap();
        store.computers.clear();
        remove_deleted_snapshots(&store, &paths, FORK_ID, "branch").unwrap();
        assert!(store.names().is_empty());
    }

    #[test]
    fn deleting_the_last_dependent_checkpoint_releases_the_deleted_sources_member() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let mut fork = Record::default();
        fork.snapshot_group = Some("dev".into());
        fork.checkpoints = vec![entry(B, "Fork checkpoint", "manual")];
        let paths = delete_fixture(&directory, vec![entry(A, "Base", "manual")], Some(fork));
        let metadata = read_metadata(&paths.metadata).unwrap();
        write_metadata(
            &paths.metadata,
            &ComputerConfigurationRequest {
                schema_version: 1,
                computers: metadata
                    .computers
                    .into_iter()
                    .filter(|configuration| configuration.id() != ID)
                    .collect(),
            },
        )
        .unwrap();
        let store = Store::new(vec![("branch", None)])
            .with("dev", A, "snap_a", None)
            .with("dev", B, "snap_b", Some("snap_a"));
        remove_deleted_snapshots(&store, &paths, ID, "dev").unwrap();
        forget_removed(&paths, ID).unwrap();
        assert_eq!(store.names(), [A, B]);
        delete_checkpoint_with(&store, &paths, FORK_ID, B).unwrap();
        assert!(store.names().is_empty());
    }

    #[test]
    fn deletion_recovery_does_not_remove_a_new_member_reusing_the_old_name() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = delete_fixture(&directory, vec![entry(A, "Delete", "manual")], None);
        write_metadata(
            &paths.metadata,
            &ComputerConfigurationRequest {
                schema_version: 1,
                computers: vec![],
            },
        )
        .unwrap();
        let mut store = Store::new(vec![]).with("dev", A, "snap_old", None);
        store.fail_remove = true;
        remove_deleted_snapshots(&store, &paths, ID, "dev").unwrap();
        store.members.lock().unwrap().clear();
        store.heads.lock().unwrap().clear();
        let store = store.with("dev", A, "snap_new", None);
        let attempts = store.removals().len();
        recover_interrupted(&store, &paths).unwrap();
        assert_eq!(store.names(), [A]);
        assert_eq!(store.removals().len(), attempts);
        assert!(load_cleanup(&paths).unwrap().is_empty());
    }

    #[test]
    fn launch_retries_journaled_deletion_without_selecting_other_orphans() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = delete_fixture(&directory, vec![entry(A, "Delete", "manual")], None);
        write_metadata(
            &paths.metadata,
            &ComputerConfigurationRequest {
                schema_version: 1,
                computers: vec![],
            },
        )
        .unwrap();
        let mut store =
            Store::new(vec![])
                .with("dev", A, "snap_a", None)
                .with("elsewhere", C, "snap_c", None);
        store.fail_remove = true;
        remove_deleted_snapshots(&store, &paths, ID, "dev").unwrap();
        forget_removed(&paths, ID).unwrap();
        assert_eq!(store.names(), [A, C]);
        store.fail_remove = false;
        recover_interrupted(&store, &paths).unwrap();
        assert_eq!(store.names(), [C]);
    }

    /// Models the process runner's cancel: any command started while the running operation
    /// was asked to cancel (and not masked) is killed.
    struct CancelRunner {
        gate: &'static super::super::operation_gate::OperationGate,
        kill_create: bool,
        state: Mutex<&'static str>,
        created: Mutex<Option<String>>,
        calls: Mutex<Vec<Vec<String>>>,
    }
    impl RuntimeRunner for CancelRunner {
        fn run(
            &self,
            _: &RuntimePaths,
            args: &[String],
            _: Duration,
        ) -> Result<CommandOutput, RuntimeError> {
            self.calls.lock().unwrap().push(args.to_vec());
            if super::super::operation_gate::cancel_requested() {
                return Err(RuntimeError::Cancelled {
                    operation: args[0].clone(),
                });
            }
            let ok = |stdout: String| {
                Ok(CommandOutput {
                    stdout,
                    stderr: String::new(),
                })
            };
            match (args[0].as_str(), args.get(1).map(String::as_str)) {
                ("inspect", _) => ok(serde_json::json!({"name":"dev","status":*self.state.lock().unwrap(),"config":{
                    "labels":{"silo.managed":"true","silo.machine-id":ID},
                    "mounts":[{"guest":"/workspace","type":"Owned","storage":{"kind":"disk","capacity_mib":1024}}]
                }}).to_string()),
                ("snapshot", Some("create")) => {
                    let entry = self.gate.snapshot().running[0].id;
                    self.gate.cancel(entry).unwrap();
                    if self.kill_create {
                        *self.state.lock().unwrap() = "Paused";
                        return Err(RuntimeError::Cancelled { operation: "snapshot create".into() });
                    }
                    *self.created.lock().unwrap() = Some(args[2].clone());
                    ok(String::new())
                }
                ("snapshot", Some("list")) => {
                    let name = self.created.lock().unwrap().clone();
                    ok(serde_json::Value::Array(name.into_iter().map(|name| serde_json::json!({
                        "snapshot_id": "snap_1", "group": "dev", "name": name, "scope": "full", "availability": "ready"
                    })).collect()).to_string())
                }
                ("resume", _) => {
                    *self.state.lock().unwrap() = "Running";
                    ok(String::new())
                }
                ("list", _) => ok(r#"[{"name":"dev"}]"#.into()),
                _ => ok(String::new()),
            }
        }
    }

    fn cancel_runner(kill_create: bool) -> CancelRunner {
        let gate: &'static super::super::operation_gate::OperationGate =
            Box::leak(Box::new(super::super::operation_gate::OperationGate::new()));
        CancelRunner {
            gate,
            kill_create,
            state: Mutex::new("Running"),
            created: Mutex::new(None),
            calls: Mutex::new(Vec::new()),
        }
    }

    #[test]
    fn a_cancel_after_the_capture_returned_still_records_the_checkpoint() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = restore_fixture(&directory, None);
        let runner = cancel_runner(false);
        let guard = runner
            .gate
            .kind(super::super::operation_gate::OperationKind::CheckpointCapture)
            .computer(ID, "dev", "Creating checkpoint")
            .unwrap();
        guard.allow_cancel();
        capture_with(&runner, &paths, ID, "Late cancel", "manual").unwrap();
        drop(guard);
        let record = load(&paths, ID).unwrap();
        assert_eq!(record.checkpoints[0].name, "Late cancel");
        assert!(record.checkpoint_operation.is_none());
        assert!(record.inflight_checkpoint.is_none());
    }

    #[test]
    fn a_cancelled_full_capture_resumes_the_computer_it_left_paused() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = restore_fixture(&directory, None);
        let runner = cancel_runner(true);
        let guard = runner
            .gate
            .kind(super::super::operation_gate::OperationKind::CheckpointCapture)
            .computer(ID, "dev", "Creating checkpoint")
            .unwrap();
        guard.allow_cancel();
        let failure = capture_with(&runner, &paths, ID, "Cancelled", "manual").unwrap_err();
        drop(guard);
        assert!(matches!(failure, RuntimeError::Cancelled { .. }));
        assert_eq!(*runner.state.lock().unwrap(), "Running");
        assert!(runner
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|call| call[0] == "resume"));
        let record = load(&paths, ID).unwrap();
        assert_eq!(record.checkpoint_operation.unwrap().status, "failed");
        assert!(
            record.inflight_checkpoint.is_none(),
            "nothing was published, so nothing is left to reconcile"
        );
    }

    #[test]
    fn a_computer_that_cannot_resume_after_a_failed_recovery_capture_is_stopped_instead_of_left_paused(
    ) {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = restore_fixture(&directory, None);
        let runner = journal_runner("Running", "create|resume");
        let failure = restore_with(&runner, &paths, ID, "c000000000000000000000000000000")
            .unwrap_err()
            .to_string();
        assert!(failure.starts_with("create failed on this host."));
        assert!(failure.contains("resume failed on this host."));
        assert!(failure.contains("disks were preserved"));
        assert_eq!(*runner.state.lock().unwrap(), "Stopped");
        let stored = load(&paths, ID).unwrap();
        assert!(stored.restore_journal.is_none());
        assert!(
            !needs_explicit_start(&paths, ID).unwrap(),
            "Start, Stop and Quit work again"
        );
    }

    #[test]
    fn abandoning_an_unfinished_restore_resumes_the_paused_computer_and_keeps_its_state() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = restore_fixture(&directory, Some(("capturing", true)));
        let mut runner = journal_runner("Paused", "");
        runner.listed = true;
        abandon_restore_with(&runner, &paths, ID).unwrap();
        assert_eq!(*runner.state.lock().unwrap(), "Running");
        let stored = load(&paths, ID).unwrap();
        assert!(stored.restore_journal.is_none());
        assert!(stored.checkpoint_operation.is_none());
        assert!(stored.pending_checkpoint_restore.is_none());
        assert!(!runner
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|call| call[0] == "remove"));

        // A secured Restore whose original computer was already removed cannot be abandoned.
        let directory = tempfile::tempdir().unwrap();
        let paths = restore_fixture(&directory, Some(("secured", true)));
        let runner = journal_runner("Stopped", "");
        let failure = abandon_restore_with(&runner, &paths, ID)
            .unwrap_err()
            .to_string();
        assert!(failure.contains("already replaced"), "{failure}");
        assert!(load(&paths, ID).unwrap().restore_journal.is_some());
        assert!(abandon_restore_with(
            &journal_runner("Running", ""),
            &restore_fixture(&tempfile::tempdir().unwrap(), None),
            ID
        )
        .is_err());
    }

    #[test]
    fn unfinished_restore_messages_and_view_name_the_checkpoint() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = restore_fixture(&directory, Some(("capturing", true)));
        assert_eq!(
            explicit_start_message(&paths, ID, "dev"),
            "The Restore of dev to “Selected” is unfinished. Retry it, or abandon it in Checkpoints, first."
        );
        let view =
            serde_json::to_value(view_unfinished_restore(&load(&paths, ID).unwrap()).unwrap())
                .unwrap();
        assert_eq!(
            view,
            serde_json::json!({"checkpointId": "c000000000000000000000000000000", "checkpointName": "Selected", "phase": "capturing"})
        );
        let failure = start_pending(
            &journal_runner("Running", ""),
            &paths,
            &computer_configuration(),
        )
        .unwrap_err()
        .to_string();
        assert!(failure.contains("to “Selected” is unfinished"), "{failure}");

        // Restoring a different checkpoint names the unfinished one.
        let mut record = load(&paths, ID).unwrap();
        record.checkpoints.push(Checkpoint {
            id: "c333333333333333333333333333333".into(),
            native_id: Some("c000000000000000000000000000000".into()),
            name: "Other".into(),
            created_at: 3,
            scope: "full".into(),
            reason: "manual".into(),
        });
        save(&paths, ID, &record).unwrap();
        let mut runner = journal_runner("Running", "");
        runner.listed = true;
        let failure = restore_with(&runner, &paths, ID, "c333333333333333333333333333333")
            .unwrap_err()
            .to_string();
        assert!(failure.contains("to “Selected” is unfinished"), "{failure}");

        // A fork waiting for its first Start is not described as a Restore.
        let directory = tempfile::tempdir().unwrap();
        let paths = restore_fixture(&directory, None);
        let mut record = load(&paths, ID).unwrap();
        record.pending_checkpoint_restore = Some(PendingRestore {
            checkpoint_id: "c000000000000000000000000000000".into(),
            source_computer: "dev".into(),
            state: "full".into(),
        });
        save(&paths, ID, &record).unwrap();
        assert_eq!(
            explicit_start_message(&paths, ID, "dev"),
            "dev starts from a checkpoint first. Use Start on its page."
        );
        assert!(view_unfinished_restore(&record).is_none());
    }

    #[test]
    fn quit_releases_a_computer_an_unfinished_restore_left_paused() {
        let _test_state = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = restore_fixture(&directory, Some(("capturing", true)));
        let runner = journal_runner("Paused", "");
        release_paused_restore(&runner, &paths, &computer_configuration());
        assert_eq!(*runner.state.lock().unwrap(), "Running");
        // Without an unfinished Restore, Quit's own handling applies.
        let directory = tempfile::tempdir().unwrap();
        let paths = restore_fixture(&directory, None);
        let runner = journal_runner("Paused", "");
        release_paused_restore(&runner, &paths, &computer_configuration());
        assert_eq!(*runner.state.lock().unwrap(), "Paused");
    }

    /// A restore attempt whose computer runs but never verifies, then is stopped (as Quit does).
    struct AttemptRunner {
        state: Mutex<&'static str>,
        exists: Mutex<bool>,
        restore_ok: bool,
        attempt: Mutex<Option<String>>,
        calls: Mutex<Vec<Vec<String>>>,
    }
    impl RuntimeRunner for AttemptRunner {
        fn run(
            &self,
            _: &RuntimePaths,
            args: &[String],
            _: Duration,
        ) -> Result<CommandOutput, RuntimeError> {
            self.calls.lock().unwrap().push(args.to_vec());
            let ok = |stdout: String| {
                Ok(CommandOutput {
                    stdout,
                    stderr: String::new(),
                })
            };
            match args[0].as_str() {
                "snapshot" => ok(serde_json::json!([{"group":"dev","name":"c000000000000000000000000000000","scope":"full","availability":"ready"}]).to_string()),
                "list" => ok(if *self.exists.lock().unwrap() { r#"[{"name":"dev"}]"#.into() } else { "[]".into() }),
                "restore" => {
                    let label = args.iter().find_map(|arg| arg.strip_prefix("silo.restore-attempt=")).unwrap().to_owned();
                    *self.attempt.lock().unwrap() = Some(label);
                    *self.exists.lock().unwrap() = true;
                    *self.state.lock().unwrap() = "Running";
                    if self.restore_ok { ok(String::new()) } else { Err(RuntimeError::TimedOut { operation: "restore".into() }) }
                }
                "inspect" if *self.exists.lock().unwrap() => ok(serde_json::json!({"name":"dev","status":*self.state.lock().unwrap(),"config":{
                    // No SILO_GITHUB secret: this computer never passes restore verification.
                    "labels":{"silo.managed":"true","silo.machine-id":ID,"silo.restore-attempt":self.attempt.lock().unwrap().clone()},
                    "resources":{"max_cpus":1,"max_memory_mib":1024},
                    "network":{"policy":{"default_egress":"deny","default_ingress":"allow","rules":[]}}
                }}).to_string()),
                "inspect" => Err(RuntimeError::Unavailable("computer not found: dev".into())),
                "start" => {
                    *self.state.lock().unwrap() = "Running";
                    ok(String::new())
                }
                _ => ok(String::new()),
            }
        }
    }

    fn pending_fixture(directory: &tempfile::TempDir) -> RuntimePaths {
        let paths = paths(directory);
        write_metadata(
            &paths.metadata,
            &ComputerConfigurationRequest {
                schema_version: 1,
                computers: vec![computer_configuration()],
            },
        )
        .unwrap();
        let mut record = Record::default();
        record.snapshot_group = Some("dev".into());
        record.pending_checkpoint_restore = Some(PendingRestore {
            checkpoint_id: "c000000000000000000000000000000".into(),
            source_computer: "dev".into(),
            state: "full".into(),
        });
        record.desired_network_policy =
            Some(serde_json::json!({"default_egress":"deny","default_ingress":"allow","rules":[]}));
        save(&paths, ID, &record).unwrap();
        paths
    }

    #[test]
    fn a_retried_start_keeps_and_starts_an_attempt_that_already_ran() {
        let _test_state = crate::test_support::global_state();
        for restore_ok in [true, false] {
            let directory = tempfile::tempdir().unwrap();
            let paths = pending_fixture(&directory);
            let runner = AttemptRunner {
                state: Mutex::new("Stopped"),
                exists: Mutex::new(false),
                restore_ok,
                attempt: Mutex::new(None),
                calls: Mutex::new(Vec::new()),
            };
            start_pending(&runner, &paths, &computer_configuration()).unwrap_err();
            let stored = load(&paths, ID).unwrap();
            assert!(stored.restore_attempt_ran, "restore_ok={restore_ok}");
            // Quit stopped the unverified computer; the user then retries Start.
            *runner.state.lock().unwrap() = "Stopped";
            runner.calls.lock().unwrap().clear();
            start_pending(&runner, &paths, &computer_configuration()).unwrap();
            let calls = runner.calls.lock().unwrap();
            assert!(
                !calls
                    .iter()
                    .any(|call| call[0] == "remove" || call[0] == "restore"),
                "{calls:?}"
            );
            assert!(calls.iter().any(|call| call[0] == "start"));
            assert_eq!(*runner.state.lock().unwrap(), "Running");
            let stored = load(&paths, ID).unwrap();
            assert!(stored.pending_checkpoint_restore.is_none());
            assert!(!stored.restore_attempted && !stored.restore_attempt_ran);
            assert!(!needs_explicit_start(&paths, ID).unwrap());
        }
    }

    #[test]
    fn an_attempt_that_never_ran_is_recreated_from_the_checkpoint() {
        let _test_state = crate::test_support::global_state();
        struct NeverRan(Mutex<Vec<String>>);
        impl RuntimeRunner for NeverRan {
            fn run(
                &self,
                _: &RuntimePaths,
                args: &[String],
                _: Duration,
            ) -> Result<CommandOutput, RuntimeError> {
                self.0.lock().unwrap().push(args[0].clone());
                match args[0].as_str() {
                    "snapshot" => Ok(CommandOutput { stdout: serde_json::json!([{"group":"dev","name":"c000000000000000000000000000000","scope":"full","availability":"ready"}]).to_string(), stderr: String::new() }),
                    "list" => Ok(CommandOutput { stdout: "[]".into(), stderr: String::new() }),
                    "restore" => Err(RuntimeError::Failed { operation: "restore".into(), exit_code: Some(1), detail: "incomplete".into() }),
                    _ => Err(RuntimeError::Unavailable("computer not found: dev".into())),
                }
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let paths = pending_fixture(&directory);
        start_pending(
            &NeverRan(Mutex::new(Vec::new())),
            &paths,
            &computer_configuration(),
        )
        .unwrap_err();
        assert!(!load(&paths, ID).unwrap().restore_attempt_ran);
    }

    /// Real checkpoint Restore and Fork against the bundled runtime: a running computer is
    /// captured as a full checkpoint, changed, forked (the fork starts from RAM), then
    /// restored in place. Uses only a disposable /tmp home and `e2e-*` computers. Run
    /// with `SILO_LIVE_TEST_CONFIRM`, `SILO_TEST_MSB` and `SILO_TEST_LIBKRUNFW`.
    #[test]
    #[ignore = "requires the packaged runtime and hardware virtualization"]
    fn live_checkpoint_restore_and_fork_use_the_runtimes_names() {
        crate::test_support::live::require_confirmation();
        let _test_state = crate::test_support::global_state();
        // The live runtime control socket requires a short root (104 bytes on macOS).
        let directory = tempfile::Builder::new()
            .prefix("silo-ck-")
            .tempdir_in(crate::test_support::live::temp_root())
            .unwrap();
        let paths = RuntimePaths {
            guest_image: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("runtime/guest-image"),
            executable: std::path::PathBuf::from(std::env::var("SILO_TEST_MSB").unwrap()),
            library: std::path::PathBuf::from(std::env::var("SILO_TEST_LIBKRUNFW").unwrap()),
            home: directory.path().join("runtime"),
            storage_home: None,
            metadata: directory.path().join("computers.json"),
            volumes: directory.path().join("volumes"),
        };
        struct Cleanup<'a>(&'a RuntimePaths);
        impl Drop for Cleanup<'_> {
            fn drop(&mut self) {
                for name in ["e2e-ck-source", "e2e-ck-fork"] {
                    let _ = super::super::run_msb(
                        self.0,
                        &["stop".into(), name.into()],
                        Duration::from_secs(60),
                    );
                }
            }
        }
        let _cleanup = Cleanup(&paths);
        let run = |arguments: &[&str]| {
            super::super::run_msb(
                &paths,
                &arguments.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
                Duration::from_secs(180),
            )
            .unwrap()
        };
        let write = |name: &str, value: &str| {
            run(&[
                "exec",
                name,
                "--",
                "sh",
                "-c",
                &format!(
                    "printf {value} > /workspace/e2e-marker; printf {value} > /root/e2e-marker; sync"
                ),
            ]);
        };
        let read = |name: &str| {
            run(&[
                "exec",
                name,
                "--",
                "sh",
                "-c",
                "cat /workspace/e2e-marker; printf :; cat /root/e2e-marker",
            ])
            .stdout
        };

        let configuration =
            super::super::create_disposable_test_computer(&paths, "e2e-ck-source").unwrap();
        run(&["start", "e2e-ck-source"]);
        write("e2e-ck-source", "before");
        let checkpoint = capture_for_test(&paths, configuration.id(), "Milestone").unwrap();
        let record = load(&paths, configuration.id()).unwrap();
        assert_eq!(record.checkpoints[0].scope, "full");
        write("e2e-ck-source", "after");
        assert_eq!(read("e2e-ck-source").trim(), "after:after");

        // Fork the checkpoint into a new, stopped computer and start it from RAM.
        let assignments = FakeAssignments::new(&[]);
        let fork = fork_source(
            &ProcessRunner,
            &paths,
            configuration.id(),
            Some(&checkpoint),
            "e2e-ck-fork",
        )
        .unwrap();
        assert_eq!(fork.scope, "full");
        fork_commit(&ProcessRunner, &paths, &assignments, &fork, "e2e-ck-fork").unwrap();
        super::super::start_disposable_test_import(&paths, "e2e-ck-fork").unwrap();
        assert_eq!(read("e2e-ck-fork").trim(), "before:before");
        // The source kept its later state while the fork ran.
        assert_eq!(read("e2e-ck-source").trim(), "after:after");
        run(&["stop", "e2e-ck-fork"]);

        // Restore the checkpoint in place.
        // Restore leaves the computer stopped with the checkpoint pending; Start resumes it.
        run(&["stop", "e2e-ck-source"]);
        restore_with(&ProcessRunner, &paths, configuration.id(), &checkpoint).unwrap();
        super::super::start_disposable_test_import(&paths, "e2e-ck-source").unwrap();
        assert_eq!(read("e2e-ck-source").trim(), "before:before");
        eprintln!("Verified live checkpoint fork and restore.");
    }

    /// A built-in computer's whole life against the real runtime: restart, stop and start
    /// (the app's own actions), then a checkpoint of the running computer, a fork of it and
    /// an in-place restore. Every boot must end with the desktop session running and
    /// computer use ready, the ChatGPT folder mounted read-only, and the approval mode
    /// kept. Needs the same inputs as the other built-in live tests (see
    /// `test_support::computer_use_live`).
    #[test]
    #[ignore = "requires the v4 guest image, a published ChatGPT app and hardware virtualization"]
    fn live_built_in_lifecycle_keeps_the_desktop_and_computer_use() {
        use crate::test_support::computer_use_live::Fixture;
        let _test_state = crate::test_support::global_state();
        let mut fixture = Fixture::new("silo-life-", None, true);
        let (source, fork_name) = ("e2e-life-src", "e2e-life-fork");
        let configuration = fixture.create(source);
        fixture.track(fork_name);
        crate::computer_use::apply_approval_with(
            std::sync::Arc::new(ProcessRunner),
            &fixture.paths,
            &configuration,
            crate::computer_use::Approval::Auto,
            false,
        )
        .unwrap();
        super::super::start_disposable_test_computer(&fixture.paths, source).unwrap();
        let checks = |name: &str, label: &str| {
            let (status, elapsed) = fixture.wait_ready(name, label);
            eprintln!("RESULT {label}: ready {}s after start", elapsed.as_secs());
            assert_eq!(status["computerUse"]["approval"], "auto", "{label}");
            let mounts = fixture.exec_status(name, "grep ' /opt/silo/chatgpt ' /proc/mounts");
            assert!(mounts.contains(" ro,"), "{label}: {mounts}");
            // Not only what Silo reports: LCU lists windows and takes a screenshot.
            fixture.wait_doctor(name, label);
        };
        checks(source, "fresh");
        super::super::disposable_test_action(&fixture.paths, source, "restart").unwrap();
        checks(source, "restart");
        super::super::disposable_test_action(&fixture.paths, source, "stop").unwrap();
        let stopped = fixture.status(source);
        assert_eq!(stopped["sessionState"], "stopped", "{stopped}");
        assert_eq!(stopped["computerUse"]["approval"], "auto", "{stopped}");
        super::super::start_disposable_test_computer(&fixture.paths, source).unwrap();
        checks(source, "stop-start");

        let write = |name: &str, value: &str| {
            fixture
                .exec(
                    name,
                    "root",
                    &format!("printf {value} > /workspace/e2e-marker; sync"),
                )
                .unwrap();
        };
        let read = |name: &str| {
            fixture
                .exec(name, "root", "cat /workspace/e2e-marker")
                .unwrap()
        };
        write(source, "before");
        let checkpoint = capture_for_test(&fixture.paths, configuration.id(), "Milestone").unwrap();
        assert_eq!(
            load(&fixture.paths, configuration.id())
                .unwrap()
                .checkpoints[0]
                .scope,
            "full"
        );
        write(source, "after");

        // Fork the checkpoint into a new computer and boot it from RAM.
        let fork = fork_source(
            &ProcessRunner,
            &fixture.paths,
            configuration.id(),
            Some(&checkpoint),
            fork_name,
        )
        .unwrap();
        fork_commit(
            &ProcessRunner,
            &fixture.paths,
            &FakeAssignments::new(&[]),
            &fork,
            fork_name,
        )
        .unwrap();
        super::super::start_disposable_test_import(&fixture.paths, fork_name).unwrap();
        checks(fork_name, "fork");
        assert_eq!(read(fork_name), "before");
        assert_eq!(read(source), "after");
        fixture.stop(fork_name);

        // Restore the checkpoint in place; Start resumes it.
        fixture.stop(source);
        restore_with(
            &ProcessRunner,
            &fixture.paths,
            configuration.id(),
            &checkpoint,
        )
        .unwrap();
        super::super::start_disposable_test_import(&fixture.paths, source).unwrap();
        checks(source, "restore");
        assert_eq!(read(source), "before");
        fixture.stop(source);
        eprintln!("Verified the built-in desktop through restart, stop, start, fork and restore.");
    }

    /// A computer created from the previous (v3) guest image is unchanged by the built-in
    /// desktop: no ChatGPT mount, no automatic desktop, no computer-use helper, and its
    /// ordinary flows (restart, stop, start, checkpoint, fork, restore) still work.
    /// `SILO_TEST_V3_GUEST_IMAGE` names a directory with the v3 manifest.json and
    /// image.tar.gz (from the guest-ubuntu-24.04-v3 release); the other inputs are those of
    /// the built-in live tests.
    #[test]
    #[ignore = "requires the v3 guest image, hardware virtualization and the packaged runtime"]
    fn live_pre_v4_computer_gets_no_mount_no_desktop_and_keeps_its_flows() {
        use crate::test_support::computer_use_live::Fixture;
        let _test_state = crate::test_support::global_state();
        let image = std::path::PathBuf::from(std::env::var("SILO_TEST_V3_GUEST_IMAGE").unwrap());
        let mut fixture = Fixture::new("silo-v3-", Some(image), false);
        let (source, fork_name) = ("e2e-v3-src", "e2e-v3-fork");
        let configuration = fixture.create(source);
        fixture.track(fork_name);
        assert!(
            !crate::computer_use::is_built_in(&configuration),
            "{configuration:?}"
        );
        assert!(computer_desktop_absent(&configuration), "{configuration:?}");
        let inspected = inspect_computer(&ProcessRunner, &fixture.paths, source).unwrap();
        assert!(!has_chatgpt_mount(&inspected.config));
        let checks = |label: &str| {
            let report = fixture.exec_status(
                source,
                "ls /usr/local/bin/silo-desktop /usr/local/libexec/silo-computer-use /opt/silo 2>&1; \
                 ls /var/lib/silo-computer-use 2>&1; pgrep -c Xvfb; grep -c /opt/silo/chatgpt /proc/mounts; \
                 cat /etc/silo-guest-version 2>/dev/null; true",
            );
            eprintln!("{label}: legacy guest state:\n{report}");
            assert!(
                !report.contains("/usr/local/libexec/silo-computer-use\n"),
                "{label}: {report}"
            );
            assert!(report.contains("No such file"), "{label}: {report}");
            let status = fixture.status(source);
            assert!(status["computerUse"].is_null(), "{label}: {status}");
            assert_ne!(status["sessionState"], "running", "{label}: {status}");
        };
        super::super::start_disposable_test_computer(&fixture.paths, source).unwrap();
        std::thread::sleep(Duration::from_secs(45));
        checks("fresh");
        super::super::disposable_test_action(&fixture.paths, source, "restart").unwrap();
        std::thread::sleep(Duration::from_secs(30));
        checks("restart");
        super::super::disposable_test_action(&fixture.paths, source, "stop").unwrap();
        super::super::start_disposable_test_computer(&fixture.paths, source).unwrap();
        std::thread::sleep(Duration::from_secs(30));
        checks("stop-start");

        let write = |name: &str, value: &str| {
            fixture
                .exec(
                    name,
                    "root",
                    &format!("printf {value} > /workspace/e2e-marker; sync"),
                )
                .unwrap();
        };
        let read = |name: &str| {
            fixture
                .exec(name, "root", "cat /workspace/e2e-marker")
                .unwrap()
        };
        write(source, "before");
        let checkpoint = capture_for_test(&fixture.paths, configuration.id(), "Milestone").unwrap();
        write(source, "after");
        let fork = fork_source(
            &ProcessRunner,
            &fixture.paths,
            configuration.id(),
            Some(&checkpoint),
            fork_name,
        )
        .unwrap();
        fork_commit(
            &ProcessRunner,
            &fixture.paths,
            &FakeAssignments::new(&[]),
            &fork,
            fork_name,
        )
        .unwrap();
        super::super::start_disposable_test_import(&fixture.paths, fork_name).unwrap();
        assert_eq!(read(fork_name), "before");
        assert_eq!(read(source), "after");
        let fork_computer = fixture.computer_configuration(fork_name);
        assert!(!crate::computer_use::is_built_in(&fork_computer));
        let inspected = inspect_computer(&ProcessRunner, &fixture.paths, fork_name).unwrap();
        assert!(!has_chatgpt_mount(&inspected.config));
        fixture.stop(fork_name);
        fixture.stop(source);
        restore_with(
            &ProcessRunner,
            &fixture.paths,
            configuration.id(),
            &checkpoint,
        )
        .unwrap();
        super::super::start_disposable_test_import(&fixture.paths, source).unwrap();
        assert_eq!(read(source), "before");
        let inspected = inspect_computer(&ProcessRunner, &fixture.paths, source).unwrap();
        assert!(!has_chatgpt_mount(&inspected.config));
        fixture.stop(source);
        eprintln!(
            "Verified a pre-v4 computer through restart, stop, start, checkpoint, fork and restore."
        );
    }

    fn has_chatgpt_mount(config: &Value) -> bool {
        config["mounts"].as_array().is_some_and(|mounts| {
            mounts
                .iter()
                .any(|mount| mount["guest"] == "/opt/silo/chatgpt")
        })
    }

    fn computer_desktop_absent(configuration: &crate::runtime::ComputerConfiguration) -> bool {
        crate::desktop::configuration(configuration).is_none()
    }
}
