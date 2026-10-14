//! The process-wide guards an engine-update hold keeps for the whole transaction.
//!
//! They are the same ones the app updater took (update admission, GitHub and secrets update
//! locks, the device-wide operation gate, file-transfer admission), plus macOS admission, which
//! now allows running computers because they are saved rather than refused.
use std::sync::{MutexGuard, RwLockWriteGuard};

use silo_api::jobs::JobId;

use crate::{
    macos_computers::MacosContext,
    runtime::{
        operation_gate::{OperationGuard, OperationKind, Scope},
        shutdown, OPERATIONS,
    },
};

use super::hold::HeldGuards;

/// Reopens macOS admission when dropped, for every exit from `acquire`.
struct MacosAdmission;

impl Drop for MacosAdmission {
    fn drop(&mut self) {
        crate::macos_computers::reopen_after_update();
    }
}

pub struct ProcessGuards {
    // Drop order is declaration order: the gate first, then the locks, then admission.
    gate: OperationGuard<'static>,
    _transfers: crate::transfer::Pause,
    _secrets: MutexGuard<'static, ()>,
    _github: MutexGuard<'static, ()>,
    _macos: MacosAdmission,
    _admission: RwLockWriteGuard<'static, ()>,
}

impl ProcessGuards {
    /// Must run on the thread that keeps the guards (they are bound to it).
    pub fn acquire(macos: &MacosContext, job: Option<&JobId>) -> Result<Self, String> {
        shutdown::ensure_accepting_operations()?;
        let admission = crate::updates::installation_guard()?;
        crate::macos_computers::close_for_save(macos)?;
        let macos_admission = MacosAdmission;
        let github = crate::github::update_guard()?;
        let secrets = crate::secrets::update_guard()?;
        // Refuse queued or running ordinary work before taking any destructive step.
        if !OPERATIONS.is_idle() {
            return Err("Wait for computer operations to finish before updating.".into());
        }
        let kind = OPERATIONS.kind(OperationKind::Shutdown);
        let gate = match job {
            Some(job) => kind.acquire_for_job(
                Scope::Device,
                None,
                "Installing update",
                None,
                job.clone(),
                None,
            ),
            None => kind.device("Installing update"),
        }
        .map_err(|error| error.to_string())?;
        shutdown::ensure_accepting_operations()?;
        // Transfers stop first: their cleanup needs the computer they were using.
        let transfers = crate::transfer::pause(shutdown::TRANSFER_DRAIN);
        Ok(Self {
            gate,
            _transfers: transfers,
            _secrets: secrets,
            _github: github,
            _macos: macos_admission,
            _admission: admission,
        })
    }
}

impl HeldGuards for ProcessGuards {
    fn relabel(&self, label: &str) {
        self.gate.relabel(label);
    }
}
