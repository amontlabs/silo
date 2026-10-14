//! The platform seam of an engine update (design/p2 §3.1): one adapter per computer kind.
//!
//! There are two real adapters, macOS (VZ `saveMachineState`) and Linux (MicroSandbox pause,
//! full snapshot and `--cow-mem` restore), so a small trait is enough; no plugin system.
use std::path::Path;

use super::journal::{CaptureRef, ComputerKind, RuntimeIdentity};

/// A computer that was running when preparation planned the update.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunningComputer {
    pub id: String,
    pub name: String,
    pub kind: ComputerKind,
}

/// Where an adapter may put its captures for this transaction.
#[derive(Clone, Copy, Debug)]
pub struct SaveContext<'a> {
    pub transaction_id: &'a str,
    /// The engine directory's `update-saves` root, for adapters whose artifacts need no
    /// particular volume. macOS places its clones beside the computer's own disk.
    pub saves_root: &'a Path,
}

/// Why a held generation could not be resumed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResumeHeldError {
    /// The paused runtime no longer exists (its owner process ended). Only the capture can
    /// bring the computer back.
    Gone(String),
    Failed(String),
}

/// What the owner of a computer found when preparation never finished saving it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Reconciled {
    /// The computer is running its original, unsaved generation again.
    Running,
    /// Its memory was not durably saved and its owner is gone. The disk is preserved.
    Lost(String),
}

/// Where the original generation stands after a crash with its resume armed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OriginalState {
    /// That exact run is executing: it resumed, and nothing is replayed.
    Running,
    /// That exact run is still held paused: the resume never happened, so it can be retried.
    Held,
    /// That run no longer exists, or a different run took its place. Whether the guest
    /// executed before it ended is unknown, so its capture is never replayed over the disk.
    Gone,
}

/// Why a restore did not bring the computer back.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RestoreError {
    /// The restore failed; the capture is intact and a later attempt may retry it.
    Failed(String),
    /// The computer is running a run the update did not start (a replacement). The capture
    /// must not be replayed over its disk and must not be settled on its behalf.
    Superseded(String),
}

impl From<String> for RestoreError {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}

impl RestoreError {
    pub fn message(&self) -> &str {
        match self {
            Self::Failed(message) | Self::Superseded(message) => message,
        }
    }
}

pub trait SaveAdapter: Send + Sync {
    fn kind(&self) -> ComputerKind;

    /// Exactly the computers that are running. An error means a computer is in a state this
    /// adapter cannot classify (for example starting or stopping): refuse the update rather
    /// than guess.
    fn running(&self) -> Result<Vec<RunningComputer>, String>;

    /// The run of this running computer, recorded in the journal before it is paused so every
    /// later resume, release and probe can require exactly that run.
    fn identity(&self, computer: &RunningComputer) -> Result<RuntimeIdentity, String>;

    /// Whether this computer's memory can be saved. Checked for every computer before any is
    /// paused, so an unsupported one refuses the update while everything still runs.
    fn check_support(&self, computer: &RunningComputer) -> Result<(), String>;

    /// Pauses the computer, makes its memory and disk durable and leaves it held (paused,
    /// still owned). On error the adapter has already tried to resume the computer.
    fn save_and_hold(
        &self,
        computer: &RunningComputer,
        context: &SaveContext<'_>,
    ) -> Result<CaptureRef, String>;

    /// Resumes the original held generation `original`, and no other run. Idempotent: that
    /// run already executing is Ok. The caller journals `ResumingOriginal` first.
    fn resume_held(
        &self,
        computer: &RunningComputer,
        original: &RuntimeIdentity,
    ) -> Result<(), ResumeHeldError>;

    /// After a crash with an original resume armed: whether that run executes, is still held,
    /// or is gone.
    fn probe_original(
        &self,
        computer: &RunningComputer,
        original: &RuntimeIdentity,
    ) -> Result<OriginalState, String>;

    /// Drops the engine's ownership of a held computer without a guest shutdown. The saved
    /// capture is what carries the computer across the process replacement.
    fn release(
        &self,
        computer: &RunningComputer,
        capture: &CaptureRef,
        original: &RuntimeIdentity,
    ) -> Result<(), String>;

    /// For an entry whose save never completed: leave the original generation running when it
    /// still exists, or report that its memory is lost.
    fn reconcile_unsaved(
        &self,
        computer: &RunningComputer,
        original: &RuntimeIdentity,
    ) -> Result<Reconciled, String>;

    /// Brings the computer back from `capture`. It restores into a new private working
    /// generation and leaves the immutable capture untouched. It must call `arm` exactly once,
    /// immediately before the first operation that lets the guest execute (a resume, or a
    /// restore that resumes by itself), with the identity of the run that will execute when it
    /// is already known, and must not execute the guest if `arm` fails.
    /// It never boots the computer cold.
    fn restore(
        &self,
        computer: &RunningComputer,
        capture: &CaptureRef,
        original: &RuntimeIdentity,
        attempt_id: &str,
        arm: &mut dyn FnMut(Option<&RuntimeIdentity>) -> Result<(), String>,
    ) -> Result<(), RestoreError>;

    /// After a crash with a restore armed: whether the restored guest of exactly that attempt
    /// is running now. `expected` is the run named when the restore was armed, if it was known.
    fn probe_armed(
        &self,
        computer: &RunningComputer,
        capture: &CaptureRef,
        original: &RuntimeIdentity,
        attempt_id: &str,
        expected: Option<&RuntimeIdentity>,
    ) -> Result<bool, String>;

    /// Removes the artifacts of a resolved capture.
    fn discard(&self, computer: &RunningComputer, capture: &CaptureRef) -> Result<(), String>;

    /// Whether this adapter's engine currently owns live machines that exiting would destroy.
    fn owns_live_machines(&self) -> bool;
}
