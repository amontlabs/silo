//! Steps: recording the commands run through Silo, listing them, forking from one, and the
//! fork tree.

use serde_json::{json, Value};
use silo_api::methods::{
    checkpoints::{CheckpointCreateParams, CheckpointKind},
    requests,
    steps::{ComputersTreeParams, StepsListParams, StepsRecordParams},
};

use crate::{
    clock,
    engine::{Computer, Engine, Progress},
    error::CliError,
    ops::{self, ExecArgs, ExecOutput},
};

/// How many step checkpoints a computer keeps when the command names no count.
pub const DEFAULT_STEP_CHECKPOINTS_KEEP: u32 = 20;

/// Whether a command is followed by a checkpoint of its step, and how many of a computer's
/// step checkpoints are kept. Chosen per command (`--step-checkpoints`, `--keep-steps`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepSettings {
    pub checkpoints: bool,
    pub keep: u32,
}

impl Default for StepSettings {
    fn default() -> Self {
        Self {
            checkpoints: false,
            keep: DEFAULT_STEP_CHECKPOINTS_KEEP,
        }
    }
}

impl StepSettings {
    /// `keep` of 0 keeps every step checkpoint.
    pub fn new(checkpoints: bool, keep: Option<u32>) -> Self {
        Self {
            checkpoints,
            keep: keep.unwrap_or(DEFAULT_STEP_CHECKPOINTS_KEEP),
        }
    }
}

/// The CLI memory file that records which forks hold a host folder (see `folder`).
const FOLDER_MEMORY: &str = "cli-folders.json";

/// The checkpoint name of a step: `step: <command, cut>`.
fn checkpoint_name(argv: &[String]) -> String {
    let text = short_command(argv);
    format!("step: {text}")
}

fn short_command(argv: &[String]) -> String {
    let joined = argv.join(" ");
    let cut: String = joined.chars().take(60).collect();
    if joined.chars().count() > 60 {
        format!("{cut}…")
    } else {
        cut
    }
}

/// Records a finished command as a step of `computer`. With `settings.checkpoints` a `step`
/// checkpoint is saved first and linked to the step. A failure here never fails the command:
/// it is returned as `error` (or `checkpointError` when only the checkpoint failed).
#[allow(clippy::too_many_arguments)]
pub async fn record(
    engine: &Engine,
    computer: &Computer,
    argv: &[String],
    exit_code: i32,
    started: i64,
    finished: i64,
    settings: StepSettings,
    progress: Progress<'_>,
) -> Value {
    let mut checkpoint_id = None;
    let mut checkpoint_error = None;
    if settings.checkpoints {
        let saved = engine
            .run_job(
                &format!("Checkpointing {} after the step", computer.name),
                requests::checkpoints_create(CheckpointCreateParams {
                    computer_id: computer.id.clone(),
                    device_id: None,
                    name: checkpoint_name(argv),
                    kind: Some(CheckpointKind::Step),
                    keep_newest: Some(settings.keep),
                }),
                progress,
            )
            .await;
        match saved {
            Ok(result) => {
                checkpoint_id = result["checkpoint"]["id"].as_str().map(str::to_owned);
            }
            Err(error) => checkpoint_error = Some(error.message),
        }
    }
    let recorded = engine
        .call(requests::steps_record(StepsRecordParams {
            device_id: None,
            computer_id: computer.id.clone(),
            command: argv.to_vec(),
            exit_code: Some(exit_code),
            started_at: clock::format_rfc3339_utc(started),
            finished_at: clock::format_rfc3339_utc(finished),
            checkpoint_id,
        }))
        .await;
    match recorded {
        Ok(result) => {
            let mut step = result["step"].clone();
            if let Some(error) = checkpoint_error {
                step["checkpointError"] = json!(error);
            }
            step
        }
        Err(error) => json!({ "error": error.message }),
    }
}

/// [`record`] under the computer's step lock when a checkpoint is taken, so checkpoint and
/// record of one step are not interleaved with another's and the numbers follow the
/// checkpoints' order. Without step checkpoints there is nothing to order and no lock. When
/// the lock cannot be taken the step is recorded anyway, with a warning on stderr.
#[allow(clippy::too_many_arguments)]
pub async fn record_in_order(
    engine: &Engine,
    computer: &Computer,
    argv: &[String],
    exit_code: i32,
    started: i64,
    finished: i64,
    settings: StepSettings,
    progress: Progress<'_>,
) -> Value {
    let _held = if settings.checkpoints {
        match engine.lock_steps(&computer.id).await {
            Ok(lock) => Some(lock),
            Err(error) => {
                eprintln!(
                    "Warning: steps of {} may be numbered out of order: {}",
                    computer.name, error.message
                );
                None
            }
        }
    } else {
        None
    };
    record(
        engine, computer, argv, exit_code, started, finished, settings, progress,
    )
    .await
}

/// A step's one-line note for a person: `step 3`, with its checkpoint when it has one.
pub fn note(step: &Value) -> String {
    if let Some(error) = step["error"].as_str() {
        return format!("step not recorded: {error}");
    }
    let mut line = format!("step {}", step["number"].as_u64().unwrap_or(0));
    if let Some(checkpoint) = step["checkpointId"].as_str() {
        line.push_str(&format!(" (checkpoint {checkpoint})"));
    }
    if let Some(error) = step["checkpointError"].as_str() {
        line.push_str(&format!(" (no checkpoint: {error})"));
    }
    line
}

/// Runs a command like `ops::exec_raw` and records it as a step of the computer.
pub async fn run_exec(
    engine: &Engine,
    args: &ExecArgs,
    settings: StepSettings,
    progress: Progress<'_>,
) -> Result<(ExecOutput, Value), CliError> {
    if args.argv.is_empty() {
        return Err(CliError::new("Give a command to run after --."));
    }
    // The target is fixed before anything runs: a promote or undo while the command runs must
    // not move the step onto the computer that now has the name.
    let computer = engine.computer(&args.computer).await?;
    let started = clock::now_unix();
    let output = ops::exec_on(engine, computer.clone(), args).await?;
    let finished = clock::now_unix();
    let step = record_in_order(
        engine,
        &computer,
        &args.argv,
        output.process_exit_code(),
        started,
        finished,
        settings,
        progress,
    )
    .await;
    Ok((output, step))
}

/// A computer's steps, oldest first.
pub async fn list(engine: &Engine, computer: &str) -> Result<Value, CliError> {
    let computer = engine.computer(computer).await?;
    let listed = engine
        .call(requests::steps_list(StepsListParams {
            device_id: None,
            computer_id: computer.id,
        }))
        .await?;
    Ok(json!({ "computer": computer.name, "steps": listed["steps"] }))
}

/// The checkpoint step `number` of `computer` left, or why there is none.
pub async fn checkpoint_for(
    engine: &Engine,
    computer: &str,
    number: u32,
) -> Result<String, CliError> {
    let found = engine.computer(computer).await?;
    let listed = list(engine, &found.name).await?;
    let step = listed["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|step| step["number"].as_u64() == Some(u64::from(number)))
        .ok_or_else(|| {
            CliError::new(format!(
                "{} has no step {number}; `silo steps {}` lists them.",
                found.name, found.name
            ))
        })?;
    let checkpoint = step["checkpointId"].as_str().ok_or_else(|| {
        CliError::new(format!(
            "Step {number} of {} has no checkpoint: step_checkpoints was off when it ran.",
            found.name
        ))
    })?;
    // Step checkpoints are trimmed, so the record may name one that is gone.
    if !found
        .checkpoints
        .iter()
        .any(|known| known["id"].as_str() == Some(checkpoint))
    {
        return Err(CliError::new(format!(
            "The checkpoint of step {number} of {} was removed (only the newest step checkpoints are kept; --keep-steps sets how many).",
            found.name
        )));
    }
    Ok(checkpoint.to_owned())
}

/// The fork tree, with the folders `silo fork ./dir` copied named as the origin of those forks.
pub async fn tree(engine: &Engine) -> Result<Value, CliError> {
    let mut tree = engine
        .call(requests::computers_tree(ComputersTreeParams {
            device_id: None,
        }))
        .await?;
    let folders = engine.recall_all(FOLDER_MEMORY);
    if let Some(nodes) = tree["nodes"].as_array_mut() {
        for node in nodes {
            let Some(id) = node["id"].as_str() else {
                continue;
            };
            if let Some(path) = folders
                .get(id)
                .and_then(|record| record["path"].as_str())
                .map(str::to_owned)
            {
                node["from"] = json!({ "kind": "folder", "path": path });
            }
        }
    }
    Ok(tree)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_step_note_says_its_number_and_checkpoint() {
        assert_eq!(note(&json!({"number": 3})), "step 3");
        assert_eq!(
            note(&json!({"number": 3, "checkpointId": "cp-1"})),
            "step 3 (checkpoint cp-1)"
        );
        assert_eq!(
            note(&json!({"number": 3, "checkpointError": "disk full"})),
            "step 3 (no checkpoint: disk full)"
        );
        assert_eq!(
            note(&json!({"error": "no engine"})),
            "step not recorded: no engine"
        );
    }

    #[test]
    fn the_checkpoint_name_is_the_command_cut_short() {
        let argv = vec!["echo".to_owned(), "x".repeat(100)];
        let name = checkpoint_name(&argv);
        assert!(name.starts_with("step: echo xxx"));
        assert!(name.ends_with('…'));
        assert_eq!(name.chars().count(), "step: ".len() + 61);
    }
}
