//! Checkpoint retention that returns disk. A Linux computer's checkpoints form one linear
//! chain of MicroSandbox snapshots and each snapshot holds its whole layer closure, so
//! removing old snapshots alone frees nothing while the computer's own disk chain still
//! carries their layers. Retention first merges the computer's layers older than the oldest
//! checkpoint that stays (`msb modify --compact --layers K`), then removes the trimmed
//! checkpoints oldest first with `--force`.
//!
//! The merge and the removal are one unit: when the merge fails nothing is removed and the
//! failure is recorded in the computer's checkpoint operation. It is attempted once per
//! retention pass and never retried inside it; the next capture is the next attempt. A
//! failure that leaves the paused computer on its compaction journal is resolved by
//! restarting the computer, which follows the journal.
use super::*;

const COMPACT_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// The stage shown while retention merges and removes, and the label of its failure.
pub(super) const RECLAIM_STAGE: &str = "Freeing checkpoint disk";

/// What `msb modify --compact --dry-run` reports for one disk.
struct DiskPlan {
    guest: String,
    input_layers: usize,
    selected_layers: usize,
}

/// Why a candidate stays: something other than the computer's own automatic checkpoints
/// builds on it.
fn blocker(
    survey: &Survey,
    computer_id: &str,
    owned: &HashMap<native::Key, &Checkpoint>,
    key: &native::Key,
    member: &native::Member,
    checkpoint: &Checkpoint,
) -> Option<&'static str> {
    let own_entry = native::Purpose::Checkpoint(checkpoint.id.clone(), checkpoint.name.clone());
    let shared = survey
        .uses
        .get(key)
        .into_iter()
        .flatten()
        .any(|used| !(used.computer_id == computer_id && used.purpose == own_entry));
    if shared {
        return Some("another computer or operation uses it");
    }
    if survey.positions.contains_key(&member.snapshot_id) {
        return Some("a computer builds on it");
    }
    let mut stack = vec![member.snapshot_id.as_str()];
    let mut seen: HashSet<&str> = HashSet::new();
    while let Some(id) = stack.pop() {
        for child in survey
            .inventory
            .iter()
            .filter(|child| child.parent_digest.as_deref() == Some(id))
        {
            if !seen.insert(child.snapshot_id.as_str()) {
                continue;
            }
            let Some(child_key) = child.key() else {
                return Some("a later saved state builds on it");
            };
            let own = owned
                .get(&child_key)
                .is_some_and(|later| later.kind != "manual");
            let only_ours = survey
                .uses
                .get(&child_key)
                .into_iter()
                .flatten()
                .all(|used| {
                    used.computer_id == computer_id
                        && matches!(used.purpose, native::Purpose::Checkpoint(..))
                });
            if !own || !only_ours {
                return Some("a named checkpoint, export or other computer builds on it");
            }
            stack.push(child.snapshot_id.as_str());
        }
    }
    None
}

fn disk_plans(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    computer: &str,
    layers: Option<usize>,
) -> Result<Vec<DiskPlan>, String> {
    let mut args = vec![
        "modify".into(),
        computer.to_owned(),
        "--compact".into(),
        "--dry-run".into(),
    ];
    if let Some(layers) = layers {
        args.extend(["--layers".into(), layers.to_string()]);
    }
    args.extend(["--format".into(), "json".into()]);
    let output = runner
        .run(paths, &args, READ_TIMEOUT)
        .map_err(|failure| safe_activity_error(&failure))?;
    let unreadable = || "The runtime returned an unreadable compaction plan.".to_owned();
    let value: Value = serde_json::from_str(&output.stdout).map_err(|_| unreadable())?;
    let disks = value["disks"].as_array().ok_or_else(unreadable)?;
    let plans: Vec<DiskPlan> = disks
        .iter()
        .filter_map(|disk| {
            Some(DiskPlan {
                guest: disk["guest_path"].as_str()?.to_owned(),
                input_layers: usize::try_from(disk["input_layers"].as_u64()?).ok()?,
                selected_layers: usize::try_from(disk["selected_layers"].as_u64()?).ok()?,
            })
        })
        .collect();
    if plans.is_empty() || plans.len() != disks.len() {
        return Err(unreadable());
    }
    Ok(plans)
}

/// How many of the oldest sealed layers to merge so that every layer older than the oldest
/// checkpoint that stays is folded into one, or `None` when there is nothing worth merging.
///
/// Each capture seals the computer's writable head into one layer and the newest capture
/// is the layer just below the head, so the capture `j` steps back from the computer's
/// current position sealed layer `total - 2 - j` (layer 0 is the oldest, and a merged
/// prefix counts as layer 0). `total` is the layer count the runtime reports, which
/// includes the head. MicroSandbox merges at least two layers and never the head.
fn merge_layers(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    computer: &str,
    survey: &Survey,
    group: &str,
    removed: &HashSet<&str>,
) -> Result<Option<usize>, String> {
    let inspected = inspect_computer(runner, paths, computer)
        .map_err(|failure| safe_activity_error(&failure))?;
    let Some(tip) = inspected
        .active_config
        .iter()
        .chain(std::iter::once(&inspected.config))
        .find_map(|config| config.get("snapshot_parent").and_then(Value::as_str))
    else {
        return Ok(None);
    };
    let members: HashMap<&str, &native::Member> = survey
        .inventory
        .iter()
        .filter(|member| member.group.as_deref() == Some(group))
        .map(|member| (member.snapshot_id.as_str(), member))
        .collect();
    let mut chain: Vec<&str> = Vec::new();
    let mut next = Some(tip);
    while let Some(id) = next {
        let Some(member) = members.get(id) else { break };
        if chain.contains(&id) {
            break;
        }
        chain.push(id);
        next = member.parent_digest.as_deref();
    }
    let Some(oldest_staying) = chain.iter().rposition(|id| !removed.contains(id)) else {
        return Ok(None);
    };
    let overview = disk_plans(runner, paths, computer, None)?;
    let total = overview
        .iter()
        .map(|disk| disk.input_layers)
        .min()
        .unwrap_or(0);
    let sealed_at = total as isize - 2 - oldest_staying as isize;
    if sealed_at < 2 || sealed_at as usize >= total {
        return Ok(None);
    }
    let layers = sealed_at as usize;
    let confirmed = disk_plans(runner, paths, computer, Some(layers))?;
    let root = confirmed
        .iter()
        .find(|disk| disk.guest == "/")
        .or_else(|| confirmed.first());
    if root.is_none_or(|disk| disk.selected_layers != layers) {
        return Err(format!(
            "The runtime would merge a different number of layers than the {layers} Silo planned."
        ));
    }
    Ok(Some(layers))
}

fn record_failure(paths: &RuntimePaths, computer_id: &str, message: &str) {
    let Ok(mut record) = load(paths, computer_id) else {
        return;
    };
    if record
        .checkpoint_operation
        .as_ref()
        .is_some_and(|operation| operation.status == "running")
    {
        return;
    }
    record.checkpoint_operation = Some(Operation {
        kind: "capture".into(),
        status: "failed".into(),
        stage: RECLAIM_STAGE.into(),
        error: Some(message.to_owned()),
    });
    let _ = save(paths, computer_id, &record);
}

/// Removes the candidate checkpoints (oldest first) that only the computer's own
/// automatic checkpoints build on, after merging the layers they held. The caller holds the
/// device lane. Returns the failure of the merge or of reading the store; a candidate that
/// cannot go is kept and logged.
pub(super) fn reclaim(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    computer_id: &str,
    candidates: &[String],
) -> Result<(), String> {
    let configuration =
        computer_configuration(paths, computer_id).map_err(|failure| failure.to_string())?;
    let computer = configuration.name();
    let record = load(paths, computer_id).map_err(|failure| failure.to_string())?;
    if record.restore_journal.is_some()
        || record.inflight_checkpoint.is_some()
        || record.pending_checkpoint_restore.is_some()
    {
        return Ok(());
    }
    let survey = survey(runner, paths).map_err(|failure| safe_activity_error(&failure))?;
    let group = record
        .snapshot_group
        .clone()
        .unwrap_or_else(|| computer.to_owned());
    let owned: HashMap<native::Key, &Checkpoint> = record
        .checkpoints
        .iter()
        .map(|checkpoint| {
            (
                (group.clone(), checkpoint.native_id().to_owned()),
                checkpoint,
            )
        })
        .collect();
    let newest = record
        .checkpoints
        .iter()
        .max_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.cmp(&right.id))
        })
        .map(|checkpoint| checkpoint.id.clone());
    let mut doomed: Vec<(&Checkpoint, Option<native::Member>)> = Vec::new();
    for id in candidates {
        let Some(checkpoint) = record.checkpoints.iter().find(|entry| &entry.id == id) else {
            continue;
        };
        if Some(&checkpoint.id) == newest.as_ref()
            || !matches!(checkpoint.kind.as_str(), "automatic" | "before-keep")
        {
            continue;
        }
        let key = (group.clone(), checkpoint.native_id().to_owned());
        let member = survey
            .inventory
            .iter()
            .find(|member| member.key().as_ref() == Some(&key))
            .cloned();
        if let Some(member) = &member {
            if let Some(reason) = blocker(&survey, computer_id, &owned, &key, member, checkpoint) {
                eprintln!(
                    "Silo kept the older {} checkpoint “{}”: {reason}.",
                    checkpoint.kind, checkpoint.name
                );
                continue;
            }
        }
        doomed.push((checkpoint, member));
    }
    if doomed.is_empty() {
        return Ok(());
    }
    let removed: HashSet<&str> = doomed
        .iter()
        .filter_map(|(_, member)| member.as_ref().map(|member| member.snapshot_id.as_str()))
        .collect();
    let merged =
        merge_layers(runner, paths, computer, &survey, &group, &removed).and_then(|layers| {
            let Some(layers) = layers else { return Ok(()) };
            runner
                .run(
                    paths,
                    &[
                        "modify".into(),
                        computer.to_owned(),
                        "--compact".into(),
                        "--layers".into(),
                        layers.to_string(),
                        "--format".into(),
                        "json".into(),
                    ],
                    COMPACT_TIMEOUT,
                )
                .map(|_| ())
                .map_err(|failure| {
                    let detail = safe_activity_error(&failure);
                    if detail.contains("paused") {
                        format!("{detail} Restart {computer} to finish the merge.")
                    } else {
                        detail
                    }
                })
        });
    if let Err(detail) = merged {
        let message = format!("Older checkpoints were kept; their disk was not freed. {detail}");
        record_failure(paths, computer_id, &message);
        return Err(message);
    }
    let order: Vec<native::Member> = doomed
        .iter()
        .filter_map(|(_, member)| member.clone())
        .collect();
    let failures = native::execute(runner, paths, &survey.inventory, &order, true);
    let failed: HashSet<&str> = failures
        .iter()
        .map(|(member, _)| member.snapshot_id.as_str())
        .collect();
    for (_, failure) in &failures {
        eprintln!(
            "Silo kept an older checkpoint: {}",
            safe_activity_error(failure)
        );
    }
    let gone: HashSet<String> = doomed
        .iter()
        .filter(|(_, member)| {
            member
                .as_ref()
                .is_none_or(|member| !failed.contains(member.snapshot_id.as_str()))
        })
        .map(|(checkpoint, _)| checkpoint.id.clone())
        .collect();
    // A capture may have been recorded while the merge ran; only the removed entries go.
    let mut record = load(paths, computer_id).map_err(|failure| failure.to_string())?;
    record.checkpoints.retain(|entry| !gone.contains(&entry.id));
    if record
        .checkpoint_operation
        .as_ref()
        .is_some_and(|operation| operation.status == "failed" && operation.stage == RECLAIM_STAGE)
    {
        record.checkpoint_operation = None;
    }
    save(paths, computer_id, &record).map_err(|failure| failure.to_string())?;
    if let Err(failure) = retry_deleted_snapshots(runner, paths) {
        eprintln!("Previously deleted checkpoint data was kept: {failure}");
    }
    Ok(())
}
