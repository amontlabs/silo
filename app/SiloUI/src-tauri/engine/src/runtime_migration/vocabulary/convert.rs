//! Pure conversions of one saved document from the earlier vocabulary to the current one.
//!
//! Every function edits the parsed document in place and returns whether it changed
//! anything. Each is idempotent: a document that already uses the current names is left
//! as it is. A renamed key replaces a key of the new name when both are present, because
//! the old name is only ever written by a build that predates the rename and so holds the
//! newer data. A value of a shape no build wrote is reported as [`Unexpected`]; the
//! caller then discards the partly edited document and keeps the saved one.
use serde_json::{Map, Value};

/// A document whose structure no version of Silo wrote.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Unexpected;

pub(super) type Converted = Result<bool, Unexpected>;

/// Setup step values that began with `workspace-`; they begin with `computer-` now.
const WORKSPACE_STEPS: [&str; 9] = [
    "configuration",
    "verification",
    "removal",
    "disk-preparation",
    "image-preparation",
    "image-import",
    "runtime-preparation",
    "settings",
    "image-wait",
];

fn object(value: &mut Value) -> Result<&mut Map<String, Value>, Unexpected> {
    value.as_object_mut().ok_or(Unexpected)
}

fn rename(map: &mut Map<String, Value>, old: &str, new: &str) -> bool {
    match map.remove(old) {
        Some(value) => {
            map.insert(new.into(), value);
            true
        }
        None => false,
    }
}

/// Renames keys in every object of the array at `key`. A missing key is nothing to do.
fn rename_in_entries(
    map: &mut Map<String, Value>,
    key: &str,
    renames: &[(&str, &str)],
) -> Converted {
    let Some(list) = map.get_mut(key) else {
        return Ok(false);
    };
    entries(list, |entry| {
        Ok(renames.iter().fold(false, |changed, (old, new)| {
            rename(entry, old, new) | changed
        }))
    })
}

fn entries(
    list: &mut Value,
    mut each: impl FnMut(&mut Map<String, Value>) -> Converted,
) -> Converted {
    let mut changed = false;
    for entry in list.as_array_mut().ok_or(Unexpected)? {
        changed |= each(object(entry)?)?;
    }
    Ok(changed)
}

/// One saved computer configuration: no `kind`, and `startWithComputer` in its desktop.
fn configuration(entry: &mut Value) -> Converted {
    let entry = object(entry)?;
    let mut changed = entry.get("kind") == Some(&Value::String("vm".into()));
    if changed {
        entry.remove("kind");
    }
    if let Some(desktop) = entry.get_mut("desktop") {
        if desktop.is_object() {
            changed |= rename(object(desktop)?, "startWithSandbox", "startWithComputer");
        }
    }
    Ok(changed)
}

/// A list of saved configurations without the SSH entries that no longer exist. The
/// identities of the dropped entries are appended to `dropped`.
fn configurations(list: &mut Value, dropped: &mut Vec<String>) -> Converted {
    let items = list.as_array_mut().ok_or(Unexpected)?;
    let before = items.len();
    items.retain(|item| {
        let ssh = item.get("kind") == Some(&Value::String("ssh".into()));
        if ssh {
            dropped.extend(item.get("id").and_then(Value::as_str).map(str::to_owned));
        }
        !ssh
    });
    let mut changed = items.len() != before;
    for item in items {
        changed |= configuration(item)?;
    }
    Ok(changed)
}

/// The saved inventory: `machines.json` and the requests inside `configuration-operation.json`.
pub(super) fn computers_metadata(value: &mut Value) -> Converted {
    let map = object(value)?;
    let mut changed = rename(map, "machines", "computers");
    if let Some(list) = map.get_mut("computers") {
        changed |= configurations(list, &mut Vec::new())?;
    }
    Ok(changed)
}

/// `appconfig/settings.json`.
pub(super) fn settings(value: &mut Value) -> Converted {
    let map = object(value)?;
    let mut changed = false;
    if let Some(saved) = map.get_mut("settings") {
        let saved = object(saved)?;
        changed |= rename(saved, "startWorkspacesAtLaunch", "startComputersAtLaunch");
        changed |= rename(saved, "startupWorkspaceIds", "startupComputerIds");
        changed |= rename(saved, "sandboxOrder", "computerOrder");
    }
    if let Some(draft) = map.get_mut("onboardingDraft") {
        if !draft.is_null() {
            changed |= onboarding_draft(draft)?;
        }
    }
    Ok(changed)
}

fn onboarding_draft(value: &mut Value) -> Converted {
    let draft = object(value)?;
    let mut changed = false;
    for (old, new) in [
        ("machines", "computers"),
        ("unfinishedMachineEditor", "unfinishedComputerEditor"),
        ("workspaceSelections", "computerSelections"),
        ("workspaceIdentities", "computerIdentities"),
        ("workspaceRepositoryAccess", "computerRepositoryAccess"),
    ] {
        changed |= rename(draft, old, new);
    }
    if draft.get("currentStep") == Some(&Value::String("workspaces".into())) {
        draft.insert("currentStep".into(), "computers".into());
        changed = true;
    }
    let mut dropped = Vec::new();
    if let Some(list) = draft.get_mut("computers") {
        changed |= configurations(list, &mut dropped)?;
    }
    if let Some(editor) = draft.get_mut("unfinishedComputerEditor") {
        if editor.is_object() {
            let ssh = editor
                .pointer("/draft/kind")
                .is_some_and(|kind| kind == "ssh");
            if ssh {
                *editor = Value::Null;
                changed = true;
            } else if let Some(computer) = editor.get_mut("draft") {
                changed |= configuration(computer)?;
            }
        }
    }
    for key in [
        "computerSelections",
        "computerIdentities",
        "computerRepositoryAccess",
    ] {
        if let Some(Value::Object(by_id)) = draft.get_mut(key) {
            for id in &dropped {
                changed |= by_id.remove(id).is_some();
            }
        }
    }
    Ok(changed)
}

/// `appdata/github.json`.
pub(super) fn github(value: &mut Value) -> Converted {
    let map = object(value)?;
    let mut changed = rename(map, "workspaces", "computers");
    changed |= rename_in_entries(map, "computers", &[("workspace", "computer")])?;
    changed |= rename_in_entries(map, "operations", &[("workspace", "computer")])?;
    Ok(changed)
}

/// `appdata/secrets.json`.
pub(super) fn secrets(value: &mut Value) -> Converted {
    let map = object(value)?;
    let mut changed = rename_in_entries(
        map,
        "secrets",
        &[
            ("workspaces", "computers"),
            ("pendingWorkspaces", "pendingComputers"),
        ],
    )?;
    changed |= rename_in_entries(map, "pendingRevocations", &[("workspace", "computer")])?;
    Ok(changed)
}

/// `appdata/backup-operation.json`, the unfinished export or import.
pub(super) fn backup_operation(value: &mut Value) -> Converted {
    let map = object(value)?;
    let mut changed = false;
    if let Some(archive) = map.get_mut("archive") {
        changed |= rename(object(archive)?, "sandboxes", "computers");
    }
    if let Some(request) = map.get_mut("request") {
        let request = object(request)?;
        changed |= rename(request, "machines", "computers");
        if let Some(capture) = request.get_mut("pending_capture") {
            if capture.is_object() {
                changed |= rename(object(capture)?, "workspaceId", "computerId");
            }
        }
    }
    Ok(changed)
}

/// An array of records that name a computer: `sandbox-activity.json`, `ssh-access.json`.
pub(super) fn computer_records(value: &mut Value) -> Converted {
    entries(value, computer_reference)
}

fn computer_reference(entry: &mut Map<String, Value>) -> Converted {
    let mut changed = rename(entry, "workspace", "computer");
    changed |= rename(entry, "machineId", "computerId");
    Ok(changed)
}

/// `setup-activity.json`.
pub(super) fn setup_activity(value: &mut Value) -> Converted {
    entries(value, |event| {
        let mut changed = rename(event, "workspace", "computer");
        if event.get("phase") == Some(&Value::String("workspaces".into())) {
            event.insert("phase".into(), "computers".into());
            changed = true;
        }
        let renamed = event
            .get("step")
            .and_then(Value::as_str)
            .and_then(|step| step.strip_prefix("workspace-"))
            .filter(|rest| WORKSPACE_STEPS.contains(rest))
            .map(|rest| format!("computer-{rest}"));
        if let Some(step) = renamed {
            event.insert("step".into(), step.into());
            changed = true;
        }
        Ok(changed)
    })
}

/// A file under `lifecycle-operations/`.
pub(super) fn lifecycle_operation(value: &mut Value) -> Converted {
    let map = object(value)?;
    let mut changed = rename(map, "machine_id", "computer_id");
    if let Some(event) = map.get_mut("event") {
        changed |= computer_reference(object(event)?)?;
    }
    Ok(changed)
}

/// `checkpoints/<id>.json`.
pub(super) fn checkpoint_record(value: &mut Value) -> Converted {
    let map = object(value)?;
    let mut changed = false;
    if let Some(restore) = map.get_mut("pendingCheckpointRestore") {
        if restore.is_object() {
            changed |= rename(object(restore)?, "sourceWorkspace", "sourceComputer");
        }
    }
    Ok(changed)
}

/// `update-resume.json`.
pub(super) fn update_resume(value: &mut Value) -> Converted {
    Ok(rename(object(value)?, "machines", "computers"))
}

/// `configuration-operation.json`: the inventory before and the one being applied.
pub(super) fn configuration_operation(value: &mut Value) -> Converted {
    let map = object(value)?;
    let mut changed = false;
    for key in ["previous", "request"] {
        if let Some(inventory) = map.get_mut(key) {
            changed |= computers_metadata(inventory)?;
        }
    }
    Ok(changed)
}

/// `network.json`.
pub(super) fn network(value: &mut Value) -> Converted {
    rename_in_entries(object(value)?, "mappings", &[("workspace", "computer")])
}

/// `repository-push-operations.json`: saved push operations by operation id.
pub(super) fn push_operations(value: &mut Value) -> Converted {
    let mut changed = false;
    for job in object(value)?.values_mut() {
        if let Some(operation) = object(job)?.get_mut("operation") {
            if operation.is_object() {
                changed |= rename(object(operation)?, "workspace", "computer");
            }
        }
    }
    Ok(changed)
}

/// `desktop-remote/config.json` in the private state folder.
pub(super) fn connections(value: &mut Value) -> Converted {
    let map = object(value)?;
    let mut changed = rename(map, "hostId", "deviceId");
    changed |= rename(map, "hosts", "devices");
    Ok(changed)
}
