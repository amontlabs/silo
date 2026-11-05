//! The engine's [`PromoteEnv`]: names and ports through the macOS computer store, secret and
//! GitHub assignments through their stores, checkpoints through the keep environment.
//!
//! Linux computers are not promoted yet: the bundled runtime names each sandbox after its
//! computer and has no rename, so the names cannot be swapped (`unsupported`).

use std::{any::Any, sync::Arc};

use serde_json::{json, Value};

use super::{journal::Step, journal::Swap, Info, PromoteEnv};
use crate::{
    changes::keep::{
        guest_env::GuestEnv, journal::Journal as KeepJournal, CheckpointRef, KeepEnv, Os,
    },
    checkpoints::Service,
    macos_computers,
    runtime::RuntimeHost,
};

pub struct EngineEnv {
    host: Arc<dyn RuntimeHost>,
    guest: GuestEnv,
}

fn ports(bindings: &Value) -> Vec<Value> {
    bindings
        .get("ports")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

impl EngineEnv {
    pub fn new(host: Arc<dyn RuntimeHost>, service: Service, keep_journal: KeepJournal) -> Self {
        let guest = GuestEnv::new(host.clone(), service, keep_journal, Box::new(|_| {}));
        Self { host, guest }
    }

    fn macos(&self) -> Result<macos_computers::MacosContext, String> {
        self.host
            .macos_context()
            .ok_or_else(|| "macOS computers are unavailable on this device.".to_owned())
    }
}

impl PromoteEnv for EngineEnv {
    fn info(&self, id: &str) -> Result<Info, String> {
        let os = self.guest.os(id)?;
        if os == Os::Linux {
            return Ok(Info {
                id: id.into(),
                name: id.into(),
                os,
                running: false,
                busy: None,
            });
        }
        let (name, stopped, deleting) =
            macos_computers::promote_view(id).ok_or("This computer no longer exists.")?;
        Ok(Info {
            id: id.into(),
            name,
            os,
            running: !stopped,
            busy: deleting.then(|| "it is being deleted".to_owned()),
        })
    }

    fn unsupported(&self, os: Os) -> Option<String> {
        (os == Os::Linux).then(|| {
            "Linux computers cannot be promoted yet: the runtime names each one after its computer and cannot rename it.".to_owned()
        })
    }

    fn name_in_use(&self, name: &str) -> bool {
        let Ok(macos) = self.macos() else {
            return true;
        };
        macos_computers::names(&macos)
            .iter()
            .any(|other| other.eq_ignore_ascii_case(name))
    }

    fn stamp(&self) -> String {
        let now = time::OffsetDateTime::now_utc();
        format!(
            "{:04}{:02}{:02}-{:02}{:02}",
            now.year(),
            u8::from(now.month()),
            now.day(),
            now.hour(),
            now.minute()
        )
    }

    fn checkpoint(&self, id: &str, name: &str) -> Result<CheckpointRef, String> {
        self.guest.checkpoint_before_keep(id, name)
    }

    fn stop(&self, id: &str) -> Result<(), String> {
        macos_computers::promote_stop(&self.macos()?, id)
    }

    fn hold(&self, id: &str, label: &str) -> Result<Box<dyn Any>, String> {
        self.guest.hold(id, label)
    }

    fn export_bindings(&self, id: &str, name: &str) -> Result<Value, String> {
        Ok(json!({
            "secrets": crate::secrets::assigned_names(name)?,
            "github": crate::github::export_computer(name)?,
            "ports": macos_computers::promote_export_ports(&self.macos()?, id)?,
        }))
    }

    fn apply(&self, step: Step, swap: &Swap) -> Result<(), String> {
        let macos = self.macos()?;
        match step {
            Step::Names => {
                macos_computers::promote_set_name(&macos, &swap.target_id, &swap.archive_name)?;
                macos_computers::promote_set_name(&macos, &swap.fork_id, &swap.target_name)
            }
            Step::Bindings => {
                // Settings keyed by name stay with the name, which the fork now has. The
                // fork's own grants under its old name are dropped, never added to main's.
                crate::secrets::computer_removed(&swap.fork_name)?;
                crate::github::forget_fork_assignment(&swap.fork_name)?;
                macos_computers::promote_replace_ports(
                    &macos,
                    &swap.fork_id,
                    &ports(&swap.target_bindings),
                )?;
                macos_computers::promote_replace_ports(&macos, &swap.target_id, &[])
            }
            // The journal keeps the lineage; the fork's checkpoint dependency stays as it is.
            Step::Lineage => Ok(()),
            Step::MacosIdentity => {
                macos_computers::promote_check_identity(&swap.fork_id)?;
                macos_computers::promote_check_identity(&swap.target_id)
            }
            Step::Quiet => {
                Err("Parked scheduled jobs are listed on the fork and were not restored.".into())
            }
        }
    }

    fn revert(&self, step: Step, swap: &Swap) -> Result<(), String> {
        let macos = self.macos()?;
        match step {
            Step::Names => {
                macos_computers::promote_set_name(&macos, &swap.fork_id, &swap.fork_name)?;
                macos_computers::promote_set_name(&macos, &swap.target_id, &swap.target_name)
            }
            Step::Bindings => {
                macos_computers::promote_replace_ports(
                    &macos,
                    &swap.target_id,
                    &ports(&swap.target_bindings),
                )?;
                macos_computers::promote_replace_ports(
                    &macos,
                    &swap.fork_id,
                    &ports(&swap.fork_bindings),
                )?;
                let names: Vec<String> = swap
                    .fork_bindings
                    .get("secrets")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();
                crate::secrets::reassign(&swap.fork_name, &names)?;
                crate::github::import_computer(
                    &swap.fork_name,
                    swap.fork_bindings.get("github").unwrap_or(&Value::Null),
                )
            }
            Step::Lineage | Step::MacosIdentity | Step::Quiet => Ok(()),
        }
    }
}
