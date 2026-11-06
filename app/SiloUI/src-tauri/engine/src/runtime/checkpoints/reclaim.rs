//! Checkpoint retention that returns disk. A Linux computer's checkpoints form one linear
//! chain of MicroSandbox snapshots and each snapshot holds its whole layer closure, so
//! removing old snapshots alone frees nothing while the computer's own disk chain still
//! carries their layers. Retention first merges the computer's layers older than the oldest
//! checkpoint that stays (`msb modify --compact --layers K`), then removes the trimmed
//! checkpoints oldest first with `--force`.
//!
//! The merge and the removal are one unit: when the merge fails nothing is removed. Every
//! problem (an unreadable store, a failed merge, a checkpoint that could not be removed) is
//! returned to the caller and stored as the computer's failed checkpoint operation. A pass
//! never retries inside itself; the next capture starts the next pass. A failure that
//! leaves the paused computer on its compaction journal is resolved by restarting the
//! computer, which follows the journal. Routine outcomes (a checkpoint something builds
//! on is kept, fewer than two layers to merge) are not problems.
use super::*;

const COMPACT_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// How long a poll waits between asking a busy runtime whether it has finished compacting.
const COMPACT_POLL: Duration = Duration::from_secs(3);

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

fn compact_args(
    computer: &str,
    disk: Option<&str>,
    layers: Option<usize>,
    dry_run: bool,
) -> Vec<String> {
    let mut args = vec!["modify".into(), computer.to_owned(), "--compact".into()];
    if dry_run {
        args.push("--dry-run".into());
    }
    if let Some(disk) = disk {
        args.extend(["--disk".into(), disk.to_owned()]);
    }
    if let Some(layers) = layers {
        args.extend(["--layers".into(), layers.to_string()]);
    }
    args.extend(["--format".into(), "json".into()]);
    args
}

/// Why a compaction plan could not be read.
enum PlanError {
    Run(RuntimeError),
    Unreadable,
}

impl PlanError {
    fn message(&self) -> String {
        match self {
            Self::Run(failure) => safe_activity_error(failure),
            Self::Unreadable => "The runtime returned an unreadable compaction plan.".to_owned(),
        }
    }
}

fn read_plans(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    computer: &str,
    disk: Option<&str>,
    layers: Option<usize>,
) -> Result<Vec<DiskPlan>, PlanError> {
    let output = runner
        .run(
            paths,
            &compact_args(computer, disk, layers, true),
            READ_TIMEOUT,
        )
        .map_err(PlanError::Run)?;
    let value: Value = serde_json::from_str(&output.stdout).map_err(|_| PlanError::Unreadable)?;
    let disks = value["disks"].as_array().ok_or(PlanError::Unreadable)?;
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
        return Err(PlanError::Unreadable);
    }
    Ok(plans)
}

fn disk_plans(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    computer: &str,
    disk: Option<&str>,
    layers: Option<usize>,
) -> Result<Vec<DiskPlan>, String> {
    read_plans(runner, paths, computer, disk, layers).map_err(|failure| failure.message())
}

/// The snapshot the computer's next capture names as parent. MicroSandbox's capture cursor
/// (`snapshot-lineage.json`) holds the latest capture; the configuration's `snapshot_parent`
/// is only where the computer was created or restored from, and is used only while no
/// capture has written a cursor.
fn computer_tip(
    paths: &RuntimePaths,
    computer: &str,
    inspected: &InspectedSandbox,
) -> Result<Option<String>, String> {
    let cursor = paths
        .home
        .join("sandboxes")
        .join(computer)
        .join("snapshot-lineage.json");
    match fs::symlink_metadata(&cursor) {
        Ok(metadata) if metadata.is_file() && metadata.len() <= 4096 => {
            let value: Value = fs::read(&cursor)
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                .ok_or_else(|| "The computer's checkpoint lineage could not be read.".to_owned())?;
            return match value.get("snapshot_id").and_then(Value::as_str) {
                Some(id) => Ok(Some(id.to_owned())),
                None => Err("The computer's checkpoint lineage is not in a known format.".into()),
            };
        }
        Err(failure) if failure.kind() == std::io::ErrorKind::NotFound => {}
        _ => return Err("The computer's checkpoint lineage could not be read.".into()),
    }
    Ok(inspected
        .active_config
        .iter()
        .chain(std::iter::once(&inspected.config))
        .find_map(|config| config.get("snapshot_parent").and_then(Value::as_str))
        .map(str::to_owned))
}

/// One `msb modify --compact` call: every disk at once, or a single disk.
struct Merge {
    disk: Option<String>,
    layers: usize,
    /// The layer count each merged disk had when the merge was planned.
    before: Vec<(String, usize)>,
}

/// The merges that fold every layer older than the oldest checkpoint that stays into one,
/// per disk, or none when there is nothing worth merging.
///
/// Each capture seals the computer's writable head into one layer and the newest capture
/// is the layer just below the head, so the capture `j` steps back from the computer's
/// current position sealed layer `total - 2 - j` (layer 0 is the oldest, and a merged
/// prefix counts as layer 0). `total` is the layer count the runtime reports for the disk,
/// which includes the head. MicroSandbox merges at least two layers and never the head,
/// so a checkpoint that sits at index 0 or 1 cannot bound a merge: an old named checkpoint
/// stops being the limit once the layers below it are folded. Disks whose chains differ
/// in length get their own count, and each plan is confirmed with a dry run.
fn merges(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    computer: &str,
    survey: &Survey,
    group: &str,
    removed: &HashSet<&str>,
) -> Result<Vec<Merge>, String> {
    let inspected = inspect_computer(runner, paths, computer)
        .map_err(|failure| safe_activity_error(&failure))?;
    let Some(tip) = computer_tip(paths, computer, &inspected)? else {
        return Ok(Vec::new());
    };
    let members: HashMap<&str, &native::Member> = survey
        .inventory
        .iter()
        .filter(|member| member.group.as_deref() == Some(group))
        .map(|member| (member.snapshot_id.as_str(), member))
        .collect();
    let mut chain: Vec<&str> = Vec::new();
    let mut next = Some(tip.as_str());
    while let Some(id) = next {
        let Some(member) = members.get(id) else { break };
        if chain.contains(&id) {
            break;
        }
        chain.push(id);
        next = member.parent_digest.as_deref();
    }
    let overview = disk_plans(runner, paths, computer, None, None)?;
    let per_disk: Vec<(&DiskPlan, Option<usize>)> = overview
        .iter()
        .map(|disk| {
            let oldest = chain
                .iter()
                .enumerate()
                .filter(|(_, id)| !removed.contains(*id))
                .filter_map(|(back, _)| {
                    let sealed = disk.input_layers as isize - 2 - back as isize;
                    (sealed >= 2).then_some(sealed as usize)
                })
                .min();
            (disk, oldest)
        })
        .collect();
    let uniform = per_disk.iter().all(|(_, layers)| *layers == per_disk[0].1);
    let steps: Vec<Merge> = if uniform {
        per_disk[0]
            .1
            .map(|layers| Merge {
                disk: None,
                layers,
                before: overview
                    .iter()
                    .map(|disk| (disk.guest.clone(), disk.input_layers))
                    .collect(),
            })
            .into_iter()
            .collect()
    } else {
        per_disk
            .iter()
            .filter_map(|(disk, layers)| {
                layers.map(|layers| Merge {
                    disk: Some(disk.guest.clone()),
                    layers,
                    before: vec![(disk.guest.clone(), disk.input_layers)],
                })
            })
            .collect()
    };
    for step in &steps {
        let confirmed = disk_plans(
            runner,
            paths,
            computer,
            step.disk.as_deref(),
            Some(step.layers),
        )?;
        if confirmed
            .iter()
            .any(|disk| disk.selected_layers != step.layers)
        {
            return Err(format!(
                "The runtime would merge a different number of layers than the {} Silo planned.",
                step.layers
            ));
        }
    }
    Ok(steps)
}

/// Whether msb's control client stopped waiting for a reply while the runtime went on
/// compacting. MicroSandbox gives up on a request after 30 seconds and the runtime answers
/// nothing else until the merge is done. Silo's own timeout is a different failure.
fn client_deadline_expired(failure: &RuntimeError) -> bool {
    matches!(
        failure,
        RuntimeError::Failed { detail, .. } if detail.to_lowercase().contains("deadline expired")
    )
}

/// A poll that got no answer because the runtime is still busy: msb's deadline, or Silo
/// ending its own short read.
fn runtime_busy(failure: &RuntimeError) -> bool {
    client_deadline_expired(failure) || matches!(failure, RuntimeError::TimedOut { .. })
}

/// Whether msb's control client found no control endpoint for the computer. While a merge
/// runs the runtime can leave its endpoint unavailable for a stretch, so the client reports
/// `transport I/O failed (NotFound)` instead of timing out; a runtime that is gone for good
/// reports the same, which is why the caller also asks whether the computer is still alive.
fn control_endpoint_missing(failure: &RuntimeError) -> bool {
    matches!(
        failure,
        RuntimeError::Failed { detail, .. }
            if detail.to_lowercase().contains("transport i/o failed (notfound)")
    )
}

/// Whether `msb inspect` lists the computer as running; it answers from the store rather
/// than from the busy runtime. Any other status ends the wait.
fn still_alive(runner: &dyn RuntimeRunner, paths: &RuntimePaths, computer: &str) -> bool {
    inspect_computer(runner, paths, computer).is_ok_and(|inspected| inspected.status == "Running")
}

fn pause(duration: Duration) {
    if !cfg!(test) {
        std::thread::sleep(duration);
    }
}

/// Waits for a runtime that is still compacting after the client stopped waiting, then
/// reports whether the merge was applied: every merged disk has fewer layers than planned.
/// The whole pass, merge included, stays within one `COMPACT_TIMEOUT` from `started`. Any
/// answer other than "busy" ends the wait at once.
fn wait_for_compaction(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    computer: &str,
    step: &Merge,
    started: Instant,
) -> Result<bool, String> {
    loop {
        match read_plans(runner, paths, computer, step.disk.as_deref(), None) {
            Ok(now) => {
                return Ok(step.before.iter().all(|(guest, before)| {
                    now.iter()
                        .find(|disk| &disk.guest == guest)
                        .is_some_and(|disk| disk.input_layers < *before)
                }));
            }
            Err(PlanError::Run(failure)) if runtime_busy(&failure) => {
                if started.elapsed() >= COMPACT_TIMEOUT {
                    return Err(format!(
                        "{computer} was still merging layers after {} minutes.",
                        COMPACT_TIMEOUT.as_secs() / 60
                    ));
                }
                pause(COMPACT_POLL);
            }
            Err(PlanError::Run(failure))
                if control_endpoint_missing(&failure) && still_alive(runner, paths, computer) =>
            {
                if started.elapsed() >= COMPACT_TIMEOUT {
                    return Err(format!(
                        "{computer} was still merging layers after {} minutes.",
                        COMPACT_TIMEOUT.as_secs() / 60
                    ));
                }
                pause(COMPACT_POLL);
            }
            Err(other) => return Err(paused_hint(other.message(), computer)),
        }
    }
}

fn paused_hint(detail: String, computer: &str) -> String {
    if detail.contains("paused") {
        format!("{detail} Restart {computer} to finish the merge.")
    } else {
        detail
    }
}

fn run_merge(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    computer: &str,
    step: &Merge,
) -> Result<(), String> {
    let started = Instant::now();
    let failure = match runner.run(
        paths,
        &compact_args(computer, step.disk.as_deref(), Some(step.layers), false),
        COMPACT_TIMEOUT,
    ) {
        Ok(_) => return Ok(()),
        Err(failure) => failure,
    };
    if client_deadline_expired(&failure) {
        return match wait_for_compaction(runner, paths, computer, step, started) {
            Ok(true) => Ok(()),
            Ok(false) => Err(format!(
                "The runtime did not merge the layers of {computer}. The next checkpoint tries again."
            )),
            Err(detail) => Err(detail),
        };
    }
    Err(paused_hint(safe_activity_error(&failure), computer))
}

/// Stores `message` as the computer's failed checkpoint operation. The caller holds the
/// computer's lane, so an operation still marked running is one a crash left behind and
/// belongs to recovery; it is kept, and the message reaches the caller through the
/// returned error and its job instead.
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
/// automatic checkpoints build on, after merging the layers they held. The caller holds
/// the computer's lane. Any problem is recorded in the computer's checkpoint operation and
/// returned; a candidate that cannot go is kept.
pub(super) fn reclaim(
    runner: &dyn RuntimeRunner,
    paths: &RuntimePaths,
    computer_id: &str,
    candidates: &[String],
) -> Result<(), String> {
    let outcome = reclaim_inner(runner, paths, computer_id, candidates);
    if let Err(message) = &outcome {
        record_failure(paths, computer_id, message);
    }
    outcome
}

fn reclaim_inner(
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
    let survey = survey(runner, paths).map_err(|failure| {
        format!(
            "Older checkpoints were kept; the checkpoint store could not be read. {}",
            safe_activity_error(&failure)
        )
    })?;
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
            || !matches!(
                checkpoint.kind.as_str(),
                "automatic" | "before-keep" | "step"
            )
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
    let merged = merges(runner, paths, computer, &survey, &group, &removed).and_then(|steps| {
        steps
            .iter()
            .try_for_each(|step| run_merge(runner, paths, computer, step))
    });
    if let Err(detail) = merged {
        return Err(format!(
            "Older checkpoints were kept; their disk was not freed. {detail}"
        ));
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
    if let Some((_, failure)) = failures.first() {
        return Err(format!(
            "{} older checkpoint(s) could not be removed and were kept. {}",
            failures.len(),
            safe_activity_error(failure)
        ));
    }
    Ok(())
}
