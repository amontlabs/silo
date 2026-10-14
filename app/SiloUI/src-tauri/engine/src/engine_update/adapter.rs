//! The platform seam of an engine update (design/p2 §3.1): one adapter per computer kind.
//!
//! There are two real adapters, macOS (VZ `saveMachineState`) and Linux (MicroSandbox pause,
//! full snapshot and `--cow-mem` restore), so a small trait is enough; no plugin system.
use std::path::Path;

use super::journal::{CaptureRef, ComputerKind};

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

pub trait SaveAdapter: Send + Sync {
    fn kind(&self) -> ComputerKind;

    /// Exactly the computers that are running. An error means a computer is in a state this
    /// adapter cannot classify (for example starting or stopping): refuse the update rather
    /// than guess.
    fn running(&self) -> Result<Vec<RunningComputer>, String>;

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

    /// Resumes the original held generation. Idempotent: a computer that already runs is Ok.
    fn resume_held(
        &self,
        computer: &RunningComputer,
        capture: Option<&CaptureRef>,
    ) -> Result<(), ResumeHeldError>;

    /// Drops the engine's ownership of a held computer without a guest shutdown. The saved
    /// capture is what carries the computer across the process replacement.
    fn release(&self, computer: &RunningComputer, capture: &CaptureRef) -> Result<(), String>;

    /// For an entry whose save never completed: leave the original generation running when it
    /// still exists, or report that its memory is lost.
    fn reconcile_unsaved(&self, computer: &RunningComputer) -> Result<Reconciled, String>;

    /// Brings the computer back from `capture`. It restores into a new private working
    /// generation and leaves the immutable capture untouched. It must call `arm` exactly once,
    /// immediately before the first operation that lets the guest execute (a resume, or a
    /// restore that resumes by itself), and must not execute the guest if `arm` fails.
    /// It never boots the computer cold.
    fn restore(
        &self,
        computer: &RunningComputer,
        capture: &CaptureRef,
        attempt_id: &str,
        arm: &mut dyn FnMut() -> Result<(), String>,
    ) -> Result<(), String>;

    /// After a crash with a restore armed: whether the restored guest of that attempt is
    /// running now.
    fn probe_armed(
        &self,
        computer: &RunningComputer,
        capture: &CaptureRef,
        attempt_id: &str,
    ) -> Result<bool, String>;

    /// Removes the artifacts of a resolved capture.
    fn discard(&self, computer: &RunningComputer, capture: &CaptureRef) -> Result<(), String>;

    /// Whether this adapter's engine currently owns live machines that exiting would destroy.
    fn owns_live_machines(&self) -> bool;
}
