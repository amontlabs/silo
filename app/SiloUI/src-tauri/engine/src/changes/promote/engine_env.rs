//! The engine's [`PromoteEnv`]: names, ports, SSH entries, the proxy CA, hostnames and quiet
//! parking through the macOS computer store, secret and GitHub assignments through their
//! stores, checkpoints through the keep environment.
//!
//! Linux computers swap names with `msb rename` of both stopped sandboxes (see `linux`);
//! the settings keyed by name follow the name, as on macOS (`docs/SiloUI-CHANGES.md`,
//! "Making a fork main").

use std::{any::Any, sync::Arc};

use serde_json::{json, Value};

use super::{journal::Step, journal::Swap, linux::Linux, Info, PromoteEnv};
use crate::{
    changes::keep::{
        guest_env::GuestEnv, journal::Journal as KeepJournal, CheckpointRef, KeepEnv, Os,
    },
    checkpoints::Service,
    guest::{self, ExecRequest, GuestUser},
    macos_computers,
    runtime::{self, RuntimeHost, RuntimePaths},
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

fn field<'a>(bindings: &'a Value, name: &str) -> &'a Value {
    bindings.get(name).unwrap_or(&Value::Null)
}

/// Runs `work` with the Linux steps wired to the real runtime and guest.
fn with_linux<T>(
    host: &dyn RuntimeHost,
    work: impl FnOnce(&Linux<'_>) -> Result<T, String>,
) -> Result<T, String> {
    let paths = runtime::runtime_paths(host)?;
    let lifecycle = |action: &str, name: &str| {
        runtime::computer_lifecycle(&paths, action, name).map_err(|error| error.to_string())
    };
    let run_guest = |name: &str, argv: &[String]| {
        let id = runtime::resolve_computer_id(&paths, name).map_err(|error| error.to_string())?;
        let target = guest::Target::Linux {
            paths: paths.clone(),
            id,
            name: name.to_owned(),
        };
        let output = guest::run_bounded(
            &target,
            &ExecRequest {
                argv: argv.to_vec(),
                cwd: None,
                env: Vec::new(),
                user: GuestUser::Root,
                stdin: None,
                timeout: std::time::Duration::from_secs(120),
                max_stdout: 1024 * 1024,
            },
        )
        .map_err(|error| error.message)?;
        if output.exit_code != Some(0) {
            return Err(format!(
                "The computer could not move its scheduled jobs: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    };
    work(&Linux {
        paths: &paths,
        runner: &runtime::ProcessRunner,
        lifecycle: &lifecycle,
        guest: &run_guest,
    })
}

/// The computer ids that hold disks under a name of their own (see `refusal`).
fn stored_under_its_name(paths: &RuntimePaths, name: &str) -> bool {
    let folder = paths.volumes.join(name).to_string_lossy().into_owned();
    runtime::inspect_computer(&runtime::ProcessRunner, paths, name)
        .map(|inspected| inspected.config.to_string().contains(&folder))
        .unwrap_or(true)
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

    /// The names of the Linux computers on this device.
    fn linux_names(&self) -> Result<Vec<String>, String> {
        let Ok(paths) = runtime::runtime_paths(self.host.as_ref()) else {
            return Ok(Vec::new());
        };
        Ok(runtime::read_metadata(&paths.metadata)
            .map_err(|error| error.to_string())?
            .computers
            .iter()
            .map(|computer| computer.name().to_owned())
            .collect())
    }
}

impl EngineEnv {
    fn apply_linux(&self, step: Step, swap: &Swap) -> Result<(), String> {
        with_linux(self.host.as_ref(), |linux| match step {
            Step::Names => linux.names_apply(swap),
            Step::Bindings => {
                // Settings keyed by name stay with the name, which the fork now has. The
                // fork's own grants under its old name are dropped, never added to main's.
                crate::secrets::computer_removed(&swap.fork_name)?;
                crate::github::forget_fork_assignment(&swap.fork_name)?;
                linux.ports_apply(swap)
            }
            // The saved SSH entries connect by name, so they follow the names step.
            Step::Alias | Step::Lineage | Step::MacosIdentity => Ok(()),
            Step::Quiet => linux
                .quiet_apply(swap)
                .map_err(|error| format!("Parked scheduled jobs were not restored: {error}")),
        })
    }

    fn revert_linux(&self, step: Step, swap: &Swap) -> Result<(), String> {
        with_linux(self.host.as_ref(), |linux| match step {
            Step::Names => linux.names_revert(swap),
            Step::Bindings => {
                linux.ports_revert(swap)?;
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
            Step::Alias | Step::Lineage | Step::MacosIdentity => Ok(()),
            Step::Quiet => linux.quiet_revert(swap),
        })
    }
}

impl PromoteEnv for EngineEnv {
    fn info(&self, id: &str) -> Result<Info, String> {
        let os = self.guest.os(id)?;
        if os == Os::Linux {
            let paths = runtime::runtime_paths(self.host.as_ref())?;
            let name = runtime::read_metadata(&paths.metadata)
                .map_err(|error| error.to_string())?
                .computers
                .iter()
                .find(|computer| computer.id() == id)
                .map(|computer| computer.name().to_owned())
                .ok_or("This computer no longer exists.")?;
            let (running, busy) =
                match runtime::observe_computer(&runtime::ProcessRunner, &paths, &name)
                    .map_err(|error| error.to_string())?
                {
                    runtime::ComputerRuntime::Absent => {
                        (false, Some("a checkpoint restore is pending".to_owned()))
                    }
                    runtime::ComputerRuntime::Present(inspected) => (
                        !matches!(inspected.status.as_str(), "Stopped" | "Created" | "Crashed"),
                        None,
                    ),
                };
            return Ok(Info {
                id: id.into(),
                name,
                os,
                running,
                busy,
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

    fn refusal(&self, main: &Info, fork: &Info) -> Option<String> {
        if main.os != Os::Linux {
            return None;
        }
        let paths = runtime::runtime_paths(self.host.as_ref()).ok()?;
        [main, fork]
            .into_iter()
            .find(|info| stored_under_its_name(&paths, &info.name))
            .map(|info| {
                format!(
                    "{} keeps a disk in a folder named after it, so its name cannot move to another computer. Nothing was changed.",
                    info.name
                )
            })
    }

    fn name_in_use(&self, name: &str) -> bool {
        let macos = match self.macos() {
            Ok(macos) => macos_computers::names(&macos),
            Err(_) => Vec::new(),
        };
        let Ok(linux) = self.linux_names() else {
            return true;
        };
        macos
            .iter()
            .chain(&linux)
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
        if self.guest.os(id)? == Os::Linux {
            let name = self.info(id)?.name;
            return with_linux(self.host.as_ref(), |linux| (linux.lifecycle)("stop", &name));
        }
        macos_computers::promote_stop(&self.macos()?, id)
    }

    fn hold(&self, id: &str, label: &str) -> Result<Box<dyn Any>, String> {
        self.guest.hold(id, label)
    }

    fn hold_pair(&self, first: &str, second: &str, label: &str) -> Result<Box<dyn Any>, String> {
        // Both computers change together and the gate admits one operation per thread, so
        // the pair is held as one device-wide admission.
        self.guest.os(first)?;
        self.guest.os(second)?;
        let guard = runtime::OPERATIONS
            .kind(runtime::operation_gate::OperationKind::Other)
            .device(label)
            .map_err(|error| error.to_string())?;
        Ok(Box::new(guard))
    }

    fn export_bindings(&self, id: &str, name: &str) -> Result<Value, String> {
        if self.guest.os(id)? == Os::Linux {
            let linux = with_linux(self.host.as_ref(), |linux| linux.export(id, name))?;
            return Ok(json!({
                "secrets": crate::secrets::assigned_names(name)?,
                "github": crate::github::export_computer(name)?,
                "ports": linux["ports"],
                "quiet": linux["quiet"],
            }));
        }
        let macos = self.macos()?;
        Ok(json!({
            "secrets": crate::secrets::assigned_names(name)?,
            "github": crate::github::export_computer(name)?,
            "ports": macos_computers::promote_export_ports(&macos, id)?,
            "hostnames": macos_computers::promote_read_hostnames(&macos, id)?,
            "quiet": macos_computers::promote_export_quiet(id)?,
        }))
    }

    fn apply(&self, step: Step, swap: &Swap) -> Result<(), String> {
        if !swap.macos {
            return self.apply_linux(step, swap);
        }
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
            Step::Alias => macos_computers::promote_point_ssh_alias(
                &macos,
                &swap.target_id,
                &swap.fork_id,
                true,
            ),
            // The lineage is read from the journal (see `lineage`).
            Step::Lineage => Ok(()),
            Step::MacosIdentity => {
                macos_computers::promote_check_identity(&swap.fork_id)?;
                macos_computers::promote_check_identity(&swap.target_id)?;
                // The new main presents main's CA, which the trust set up for main already
                // covers, and calls itself what main called itself. The archive takes the
                // fork's hostname so the two never share one. The machine identifier stays
                // with each disk: Apple ties a guest's hardware identity (and what it
                // activated or signed in with) to the identifier it first booted with, and
                // the two disks are swapped by name, never by hardware.
                macos_computers::promote_carry_ca(&swap.target_id, &swap.fork_id)?;
                macos_computers::promote_write_hostnames(
                    &macos,
                    &swap.fork_id,
                    field(&swap.target_bindings, "hostnames"),
                )?;
                macos_computers::promote_write_hostnames(
                    &macos,
                    &swap.target_id,
                    field(&swap.fork_bindings, "hostnames"),
                )
            }
            Step::Quiet => macos_computers::promote_restore_quiet(&macos, &swap.fork_id)
                .map(|_| ())
                .map_err(|error| format!("Parked scheduled jobs were not restored: {error}")),
        }
    }

    fn revert(&self, step: Step, swap: &Swap) -> Result<(), String> {
        if !swap.macos {
            return self.revert_linux(step, swap);
        }
        let macos = self.macos()?;
        match step {
            Step::Names => {
                macos_computers::promote_set_name(&macos, &swap.fork_id, &swap.fork_name)?;
                macos_computers::promote_set_name(&macos, &swap.target_id, &swap.target_name)
            }
            Step::Bindings => {
                // Ports changed while the fork was main belong to the main role, so they go
                // back to the old main with it; the fork gets the ports it had. A repeat
                // after a crash finds the fork's own ports already back and moves nothing.
                let mine = ports(&swap.fork_bindings);
                let now = macos_computers::promote_export_ports(&macos, &swap.fork_id)?;
                if now != mine {
                    macos_computers::promote_replace_ports(&macos, &swap.target_id, &now)?;
                }
                macos_computers::promote_replace_ports(&macos, &swap.fork_id, &mine)?;
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
            Step::Alias => macos_computers::promote_point_ssh_alias(
                &macos,
                &swap.target_id,
                &swap.fork_id,
                false,
            ),
            Step::MacosIdentity => {
                macos_computers::promote_uncarry_ca(&swap.fork_id)?;
                macos_computers::promote_write_hostnames(
                    &macos,
                    &swap.fork_id,
                    field(&swap.fork_bindings, "hostnames"),
                )?;
                macos_computers::promote_write_hostnames(
                    &macos,
                    &swap.target_id,
                    field(&swap.target_bindings, "hostnames"),
                )
            }
            Step::Quiet => macos_computers::promote_repark_quiet(
                &macos,
                &swap.fork_id,
                field(&swap.fork_bindings, "quiet"),
            ),
            Step::Lineage => Ok(()),
        }
    }
}
