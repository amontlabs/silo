//! Silo's durable names and lifecycle intent for upstream MicroSandbox snapshots.
//! Snapshot data and its reference graph remain owned by MicroSandbox.
use super::*;
use std::collections::{HashMap, HashSet};

pub const RESTORE_EXPECTED_DURATION: Duration = Duration::from_secs(60 * 60);

#[path = "checkpoints/native.rs"]
mod native;
#[cfg(test)]
#[path = "checkpoints/running_retry_tests.rs"]
mod running_retry_tests;
pub use native::{plan as native_removal_plan, Member as NativeMember};

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
    /// Policy origin, `manual` or `automatic`. Records from before the field existed are
    /// rewritten once by the launch upgrade (`upgrade_records`); nothing else accepts them.
    pub(super) kind: String,
}

/// The layout of `checkpoints/<computer id>.json`. Version 1 had no checkpoint `kind`.
const RECORD_VERSION: u8 = 2;

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
    validate(&record)?;
    Ok(record)
}

/// Whether a parsed history is internally consistent. `load` refuses an inconsistent one and
/// the launch upgrade never writes one.
fn validate(record: &Record) -> Result<(), RuntimeError> {
    if record.version != RECORD_VERSION
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
                || !matches!(checkpoint.kind.as_str(), "manual" | "automatic")
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
    Ok(())
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
pub fn ensure_snapshot_group(
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
    record.version = RECORD_VERSION;
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
    capture_with_kind(runner, paths, id, display_name, reason, "manual").map(|_| ())
}

fn capture_with_kind(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    id: &str,
    display_name: &str,
    reason: &str,
    policy_kind: &str,
) -> Result<Checkpoint, RuntimeError> {
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
            kind: policy_kind.into(),
        };
        record.checkpoints.insert(0, checkpoint);
        record.checkpoint_operation = None;
        let checkpoint = record.checkpoints[0].clone();
        save(paths, id, &record)?;
        return Ok(checkpoint);
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
        kind: policy_kind.into(),
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
            let checkpoint = record.checkpoints[0].clone();
            save(paths, id, &record)?;
            Ok(checkpoint)
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

pub fn create(paths: &RuntimePaths, computer_id: &str, name: &str) -> Result<(), String> {
    // Capture touches only this computer's own snapshot store and per-computer checkpoint
    // record, not the shared inventory, so it is ordered per computer by stable id.
    let computer_name = computer_configuration(paths, computer_id)
        .map_err(|error| error.to_string())?
        .name()
        .to_owned();
    let guard = OPERATIONS
        .kind(super::operation_gate::OperationKind::CheckpointCapture)
        .computer(computer_id, &computer_name, "Creating checkpoint")
        .map_err(|error| error.to_string())?;
    // Checkpoint capture is cancellable and expected to finish within 15 minutes.
    guard.allow_cancel();
    guard.expect_within(std::time::Duration::from_secs(15 * 60));
    shutdown::ensure_accepting_operations()?;
    let result = capture_with(&ProcessRunner, paths, computer_id, name, "manual");
    drop(guard);
    result.map_err(|error| error.to_string())
}

/// The one-time launch upgrade of `<storage>/checkpoints/<computer id>.json` from layout 1
/// to layout 2: every checkpoint record gains `kind: "manual"` (it was made before automatic
/// checkpoints existed) and keeps its reason, native identity, dates and journals. A file
/// that is not a layout-1 record is left exactly as it is, so a damaged history stays
/// protected and a converted one is not rewritten. Returns how many files were converted.
pub fn upgrade_records(storage: &Path) -> Result<usize, String> {
    let directory = storage.join("checkpoints");
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(_) => return Err("Checkpoint history could not be scanned for upgrade.".into()),
    };
    let mut converted = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        let is_record = path
            .extension()
            .is_some_and(|extension| extension == "json")
            && path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .is_some_and(|stem| uuid::Uuid::parse_str(stem).is_ok());
        if !is_record {
            continue;
        }
        let Ok(bytes) = fs::read(&path) else {
            return Err("Checkpoint history could not be read for upgrade.".into());
        };
        let Ok(mut value) = serde_json::from_slice::<Value>(&bytes) else {
            continue;
        };
        if value.get("version") != Some(&Value::from(1)) {
            continue;
        }
        let mark = |checkpoint: Option<&mut Value>| {
            if let Some(object) = checkpoint.and_then(Value::as_object_mut) {
                object
                    .entry("kind")
                    .or_insert_with(|| Value::String("manual".into()));
            }
        };
        if let Some(list) = value.get_mut("checkpoints").and_then(Value::as_array_mut) {
            for checkpoint in list {
                mark(Some(checkpoint));
            }
        }
        mark(value.get_mut("inflightCheckpoint"));
        mark(value.pointer_mut("/restoreJournal/recoveryCheckpoint"));
        value["version"] = Value::from(RECORD_VERSION);
        // A history that does not validate once converted keeps its original bytes and is
        // reported; it is never rewritten into a form `load` would refuse.
        if !serde_json::from_value::<Record>(value.clone())
            .is_ok_and(|record| validate(&record).is_ok())
        {
            eprintln!(
                "silo-engine: left damaged checkpoint history untouched: {}",
                path.display()
            );
            continue;
        }
        let mut file = tempfile::NamedTempFile::new_in(&directory)
            .map_err(|_| "Checkpoint history could not be saved for upgrade.".to_owned())?;
        serde_json::to_writer(&mut file, &value)
            .map_err(|_| "Checkpoint history could not be encoded for upgrade.".to_owned())?;
        file.as_file()
            .sync_all()
            .and_then(|()| file.persist(&path).map(drop).map_err(|error| error.error))
            .and_then(|()| File::open(&directory)?.sync_all())
            .map_err(|_| "Checkpoint history could not be saved for upgrade.".to_owned())?;
        converted += 1;
    }
    Ok(converted)
}

/// The Linux record of a computer exists in this device's inventory.
pub fn has_computer(paths: &RuntimePaths, computer_id: &str) -> bool {
    computer_configuration(paths, computer_id).is_ok()
}

/// Captures a Linux checkpoint as one durable, cancellable job operation and returns the
/// exact record it committed. An automatic capture is refused while the setting is off,
/// both before it queues (the caller) and once the queue admits it (here).
pub fn create_for_job(
    paths: &RuntimePaths,
    computer_id: &str,
    name: &str,
    automatic: bool,
    retention: &dyn Fn() -> Result<u32, String>,
    job_id: silo_api::jobs::JobId,
    cancellation: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<silo_api::methods::checkpoints::Checkpoint, String> {
    use super::operation_gate::{OperationKind, Scope};

    let computer_name = computer_configuration(paths, computer_id)
        .map_err(|error| error.to_string())?
        .name()
        .to_owned();
    let guard = OPERATIONS
        .kind(OperationKind::CheckpointCapture)
        .acquire_for_job(
            Scope::Computer {
                id: computer_id.to_owned(),
            },
            Some(computer_name.clone()),
            "Creating checkpoint",
            None,
            job_id.clone(),
            Some(cancellation),
        )
        .map_err(|error| error.to_string())?;
    admitted()?;
    guard.allow_cancel();
    guard.expect_within(std::time::Duration::from_secs(15 * 60));
    shutdown::ensure_accepting_operations()?;
    if automatic && retention()? == 0 {
        return Err(crate::checkpoints::AUTOMATIC_OFF.into());
    }
    let policy_kind = if automatic { "automatic" } else { "manual" };
    let checkpoint = capture_with_kind(
        &ProcessRunner,
        paths,
        computer_id,
        name,
        "manual",
        policy_kind,
    )
    .map_err(|error| error.to_string())?;
    // The capture's computer lane is released before retention asks for the device lane.
    drop(guard);
    if automatic {
        prune_automatic_with(
            &OPERATIONS,
            &ProcessRunner,
            paths,
            computer_id,
            retention,
            job_id,
        );
    }
    Ok(api_checkpoint(checkpoint))
}

fn api_checkpoint(checkpoint: Checkpoint) -> silo_api::methods::checkpoints::Checkpoint {
    use silo_api::methods::checkpoints::{
        Checkpoint as ApiCheckpoint, CheckpointKind, CheckpointReason, CheckpointScope,
    };
    ApiCheckpoint {
        id: checkpoint.id,
        name: checkpoint.name,
        created_at: crate::checkpoints::retention::format_millis(checkpoint.created_at),
        scope: if checkpoint.scope == "full" {
            CheckpointScope::Full
        } else {
            CheckpointScope::Disk
        },
        reason: if checkpoint.reason == "before-restore" {
            CheckpointReason::BeforeRestore
        } else {
            CheckpointReason::Manual
        },
        kind: if checkpoint.kind == "automatic" {
            CheckpointKind::Automatic
        } else {
            CheckpointKind::Manual
        },
        size_bytes: None,
    }
}

/// Removes this computer's automatic checkpoints beyond the newest N, oldest first, through
/// ordinary safe deletion. It never touches manual or before-restore records. The count is
/// read again once each deletion is admitted so turning the setting off stops later deletions, and a
/// checkpoint something still needs is kept (visible with its `deleteBlocker`).
fn prune_automatic_with(
    gate: &super::operation_gate::OperationGate,
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    computer_id: &str,
    retention: &dyn Fn() -> Result<u32, String>,
    job_id: silo_api::jobs::JobId,
) {
    use super::operation_gate::{OperationKind, Scope};

    let entries_of = |record: &Record| -> Vec<crate::checkpoints::retention::Entry> {
        record
            .checkpoints
            .iter()
            .map(|checkpoint| crate::checkpoints::retention::Entry {
                id: checkpoint.id.clone(),
                created_at_ms: i128::from(checkpoint.created_at),
                automatic: checkpoint.kind == "automatic",
            })
            .collect()
    };
    let Ok(count) = retention() else { return };
    if count == 0 {
        return;
    }
    let Ok(record) = load(paths, computer_id) else {
        return;
    };
    for candidate in crate::checkpoints::retention::plan(&entries_of(&record), count).candidates {
        let Ok(_guard) = gate.kind(OperationKind::CheckpointDelete).acquire_for_job(
            Scope::Device,
            None,
            "Removing an older automatic checkpoint",
            None,
            job_id.clone(),
            None,
        ) else {
            return;
        };
        // The plan is made again once the queue admits this deletion: the setting and the
        // history may have changed while it waited, and only a candidate that the current
        // count still drops is deleted.
        let Ok(current) = retention() else { return };
        if current == 0 {
            return;
        }
        let Ok(record) = load(paths, computer_id) else {
            return;
        };
        if !crate::checkpoints::retention::plan(&entries_of(&record), current)
            .candidates
            .contains(&candidate)
        {
            continue;
        }
        if let Err(error) = delete_checkpoint_with(runner, paths, computer_id, &candidate) {
            // Dependencies and in-progress recovery defer retention. Never force-delete a
            // checkpoint the runtime says something still needs.
            eprintln!("Silo kept an older automatic checkpoint: {error}");
        }
    }
}

thread_local! {
    /// What the current job worker does once the queue admits its operation, before the
    /// operation changes anything: it marks the durable job running.
    static ADMISSION: std::cell::RefCell<Option<Box<dyn FnOnce() -> Result<(), String>>>> =
        const { std::cell::RefCell::new(None) };
}

/// Runs `work` with `on_admitted` armed for the checkpoint operation it starts. The first
/// admission of a `*_for_job` operation on this thread runs it; a failure refuses the
/// operation, so a job never reports running without having been admitted.
pub(crate) fn with_admission<T>(
    on_admitted: impl FnOnce() -> Result<(), String> + 'static,
    work: impl FnOnce() -> T,
) -> T {
    ADMISSION.with(|hook| *hook.borrow_mut() = Some(Box::new(on_admitted)));
    let result = work();
    ADMISSION.with(|hook| hook.borrow_mut().take());
    result
}

/// Called right after the queue admits a job-owned operation.
fn admitted() -> Result<(), String> {
    match ADMISSION.with(|hook| hook.borrow_mut().take()) {
        Some(on_admitted) => on_admitted(),
        None => Ok(()),
    }
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

pub fn forget_removed(paths: &RuntimePaths, id: &str) -> Result<(), RuntimeError> {
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
pub fn export_source(
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

pub fn import_pending_restore_with_environment(
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
pub trait ForkAssignments {
    fn copy_github(&self, source: &str, target: &str) -> Result<(), String>;
    fn forget_github(&self, target: &str) -> Result<(), String>;
    fn copy_secrets(&self, source: &str, target: &str) -> Result<(), String>;
    fn forget_secrets(&self, target: &str) -> Result<(), String>;
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
    job: Option<(
        silo_api::jobs::JobId,
        std::sync::Arc<std::sync::atomic::AtomicBool>,
    )>,
) -> Result<(), RuntimeError> {
    use super::operation_gate::{OperationKind, Scope};
    let computer_name = computer_configuration(paths, computer_id)?
        .name()
        .to_owned();
    let fork = {
        let gate = gate.kind(OperationKind::CheckpointFork);
        let guard = if let Some((job_id, cancellation)) = &job {
            gate.acquire_for_job(
                Scope::Computer {
                    id: computer_id.to_owned(),
                },
                Some(computer_name.clone()),
                "Forking checkpoint",
                None,
                job_id.clone(),
                Some(cancellation.clone()),
            )?
        } else {
            gate.computer(computer_id, &computer_name, "Forking checkpoint")?
        };
        admitted().map_err(RuntimeError::Unavailable)?;
        // Fork is not cancellable; flag it slow after the capture window.
        guard.expect_within(Duration::from_secs(10 * 60));
        shutdown::ensure_accepting_operations().map_err(RuntimeError::Unavailable)?;
        fork_source(runner, paths, computer_id, checkpoint_id, new_name)?
    };
    let gate = gate.kind(OperationKind::CheckpointFork);
    let guard = if let Some((job_id, cancellation)) = &job {
        gate.acquire_for_job(
            Scope::Device,
            None,
            "Forking checkpoint",
            None,
            job_id.clone(),
            Some(cancellation.clone()),
        )?
    } else {
        gate.device("Forking checkpoint")?
    };
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

pub fn fork(
    paths: &RuntimePaths,
    assignments: &dyn ForkAssignments,
    computer_id: &str,
    checkpoint_id: Option<&str>,
    new_name: &str,
    macos_names: &dyn Fn() -> Vec<String>,
) -> Result<(), String> {
    fork_in_lanes(
        &OPERATIONS,
        &ProcessRunner,
        paths,
        assignments,
        computer_id,
        checkpoint_id,
        new_name,
        macos_names,
        None,
    )
    .map_err(|error| error.to_string())
}

/// Forks a Linux checkpoint while both admission lanes are linked to its durable job.
pub fn fork_for_job(
    paths: &RuntimePaths,
    assignments: &dyn ForkAssignments,
    computer_id: &str,
    checkpoint_id: Option<&str>,
    new_name: &str,
    macos_names: &dyn Fn() -> Vec<String>,
    job_id: silo_api::jobs::JobId,
    cancellation: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<(), String> {
    fork_in_lanes(
        &OPERATIONS,
        &ProcessRunner,
        paths,
        assignments,
        computer_id,
        checkpoint_id,
        new_name,
        macos_names,
        Some((job_id, cancellation)),
    )
    .map_err(|error| error.to_string())
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
            kind: "manual".into(),
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
            kind: "manual".into(),
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

pub fn restore(paths: &RuntimePaths, computer_id: &str, checkpoint_id: &str) -> Result<(), String> {
    // Restore rewrites only this computer's runtime state and per-computer checkpoint record,
    // not the shared inventory, so it is ordered per computer by stable id.
    let computer_name = computer_configuration(paths, computer_id)
        .map_err(|error| error.to_string())?
        .name()
        .to_owned();
    let guard = OPERATIONS
        .kind(super::operation_gate::OperationKind::CheckpointRestore)
        .computer(computer_id, &computer_name, "Restoring checkpoint")
        .map_err(|error| error.to_string())?;
    // Restore is deliberately not cancellable; flag it slow after the restore window.
    guard.expect_within(RESTORE_EXPECTED_DURATION);
    let _guard = guard;
    shutdown::ensure_accepting_operations()?;
    restore_with(&ProcessRunner, paths, computer_id, checkpoint_id)
        .map_err(|error| error.to_string())
}

/// Restores a Linux checkpoint under the operation gate owned by a durable job.
pub fn restore_for_job(
    paths: &RuntimePaths,
    computer_id: &str,
    checkpoint_id: &str,
    job_id: silo_api::jobs::JobId,
    cancellation: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<(), String> {
    use super::operation_gate::{OperationKind, Scope};

    let computer_name = computer_configuration(paths, computer_id)
        .map_err(|error| error.to_string())?
        .name()
        .to_owned();
    let guard = OPERATIONS
        .kind(OperationKind::CheckpointRestore)
        .acquire_for_job(
            Scope::Computer {
                id: computer_id.to_owned(),
            },
            Some(computer_name),
            "Restoring checkpoint",
            None,
            job_id,
            Some(cancellation),
        )
        .map_err(|error| error.to_string())?;
    admitted()?;
    guard.expect_within(RESTORE_EXPECTED_DURATION);
    shutdown::ensure_accepting_operations()?;
    restore_with(&ProcessRunner, paths, computer_id, checkpoint_id)
        .map_err(|error| error.to_string())
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

pub fn abandon_restore(paths: &RuntimePaths, computer_id: &str) -> Result<(), String> {
    let computer_name = computer_configuration(paths, computer_id)
        .map_err(|error| error.to_string())?
        .name()
        .to_owned();
    let guard = OPERATIONS
        .kind(super::operation_gate::OperationKind::CheckpointRestore)
        .computer(computer_id, &computer_name, "Abandoning Restore")
        .map_err(|error| error.to_string())?;
    guard.expect_within(std::time::Duration::from_secs(5 * 60));
    let _guard = guard;
    shutdown::ensure_accepting_operations()?;
    abandon_restore_with(&ProcessRunner, paths, computer_id).map_err(|error| error.to_string())
}

/// Abandons a Linux restore while exposing its device change as a job-owned queue entry.
pub fn abandon_restore_for_job(
    paths: &RuntimePaths,
    computer_id: &str,
    job_id: silo_api::jobs::JobId,
    cancellation: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<(), String> {
    use super::operation_gate::{OperationKind, Scope};

    let computer_name = computer_configuration(paths, computer_id)
        .map_err(|error| error.to_string())?
        .name()
        .to_owned();
    let guard = OPERATIONS
        .kind(OperationKind::CheckpointRestore)
        .acquire_for_job(
            Scope::Computer {
                id: computer_id.to_owned(),
            },
            Some(computer_name),
            "Abandoning Restore",
            None,
            job_id,
            Some(cancellation),
        )
        .map_err(|error| error.to_string())?;
    admitted()?;
    guard.expect_within(std::time::Duration::from_secs(5 * 60));
    shutdown::ensure_accepting_operations()?;
    abandon_restore_with(&ProcessRunner, paths, computer_id).map_err(|error| error.to_string())
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

pub fn delete(paths: &RuntimePaths, computer_id: &str, checkpoint_id: &str) -> Result<(), String> {
    // A deleted child can release a deleted source's member in another computer's
    // lineage group. Serialize the shared cleanup journal and native store changes.
    let guard = OPERATIONS
        .kind(super::operation_gate::OperationKind::CheckpointDelete)
        .device("Deleting checkpoint")
        .map_err(|error| error.to_string())?;
    guard.expect_within(std::time::Duration::from_secs(5 * 60));
    let _guard = guard;
    shutdown::ensure_accepting_operations()?;
    delete_checkpoint_with(&ProcessRunner, paths, computer_id, checkpoint_id)
        .map_err(|error| error.to_string())
}

/// Deletes a Linux checkpoint under a device-wide operation gate owned by a durable job.
pub fn delete_for_job(
    paths: &RuntimePaths,
    computer_id: &str,
    checkpoint_id: &str,
    job_id: silo_api::jobs::JobId,
    cancellation: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<(), String> {
    use super::operation_gate::{OperationKind, Scope};

    let guard = OPERATIONS
        .kind(OperationKind::CheckpointDelete)
        .acquire_for_job(
            Scope::Device,
            None,
            "Deleting checkpoint",
            None,
            job_id,
            Some(cancellation),
        )
        .map_err(|error| error.to_string())?;
    admitted()?;
    guard.expect_within(std::time::Duration::from_secs(5 * 60));
    shutdown::ensure_accepting_operations()?;
    delete_checkpoint_with(&ProcessRunner, paths, computer_id, checkpoint_id)
        .map_err(|error| error.to_string())
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

pub fn read_usage(paths: &RuntimePaths, computer_id: &str) -> Result<CheckpointUsage, String> {
    usage_with(&ProcessRunner, paths, computer_id).map_err(|error| safe_activity_error(&error))
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
pub fn discard_failed_capture(
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
#[path = "checkpoints/tests/mod.rs"]
mod tests;
