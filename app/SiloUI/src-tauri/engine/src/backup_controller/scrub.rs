//! The export controller's side of an export without logins: which computers get a hidden
//! copy, the copies themselves, and how the copy's steps show in the export's progress.

use std::sync::Arc;

use super::{publish, Controller, Operation, Phase, TransferError};
use crate::{
    backup,
    export_scrub::{self, Failure, Prepared, ScrubEnv},
    runtime::{self, RuntimeHost},
};

/// The phase that leads the progress while the copies are made.
const SCRUB_PHASE: &str = "Leave out logins";

/// Whether an export of this computer's current state leaves its logins out: a Linux virtual
/// machine, on a host that can make the hidden copy.
pub fn leaves_logins_out(host: &dyn RuntimeHost, computer_id: &str) -> bool {
    host.export_scrub().is_some() && linux_vm_name(host, computer_id).is_some()
}

/// The name of the Linux virtual machine with this id; `None` for a container, a macOS computer
/// or an id that is not a computer.
fn linux_vm_name(host: &dyn RuntimeHost, computer_id: &str) -> Option<String> {
    let paths = runtime::runtime_paths(host).ok()?;
    let metadata = runtime::read_metadata(&paths.metadata).ok()?;
    metadata
        .computers
        .iter()
        .find(|configuration| configuration.id() == computer_id && !configuration.is_container())
        .map(|configuration| configuration.name().to_owned())
}

/// The computers of this export that are exported from a copy, as (name, id): the Linux virtual
/// machines among `names`. A container has no block device to discard, and a macOS computer is
/// not a Linux one.
pub(super) fn targets(host: &dyn RuntimeHost, names: &[String]) -> Vec<(String, String)> {
    let Some(metadata) = runtime::runtime_paths(host)
        .ok()
        .and_then(|paths| runtime::read_metadata(&paths.metadata).ok())
    else {
        return Vec::new();
    };
    let computers: Vec<(String, String, bool)> = metadata
        .computers
        .iter()
        .map(|configuration| {
            (
                configuration.name().to_owned(),
                configuration.id().to_owned(),
                configuration.is_container(),
            )
        })
        .collect();
    pick_targets(&computers, names)
}

/// The (name, id) of each of `names` that is a virtual machine among `computers`, given as
/// (name, id, is a container).
fn pick_targets(computers: &[(String, String, bool)], names: &[String]) -> Vec<(String, String)> {
    names
        .iter()
        .filter_map(|name| {
            computers
                .iter()
                .find(|(known, _, container)| known == name && !container)
                .map(|(known, id, _)| (known.clone(), id.clone()))
        })
        .collect()
}

/// The copies an export is made from, until they are dropped.
pub(super) struct Copies {
    env: Arc<dyn ScrubEnv>,
    prepared: Vec<(String, Prepared)>,
}

impl Copies {
    /// The names to export, in the order of `names`: a copy's for each computer that has one.
    pub(super) fn export_names(&self, names: &[String]) -> Vec<String> {
        names
            .iter()
            .map(|name| {
                self.prepared
                    .iter()
                    .find(|(original, _)| original == name)
                    .map_or_else(|| name.clone(), |(_, prepared)| prepared.copy_name.clone())
            })
            .collect()
    }

    pub(super) fn presented(&self) -> Vec<backup::Presented> {
        self.prepared
            .iter()
            .map(|(name, prepared)| backup::Presented {
                copy: prepared.copy_name.clone(),
                name: name.clone(),
                id: prepared.source_id.clone(),
            })
            .collect()
    }

    /// What the copies no longer hold.
    pub(super) fn left_out(&self) -> Vec<String> {
        let mut left_out: Vec<String> = self
            .prepared
            .iter()
            .flat_map(|(_, prepared)| prepared.left_out.clone())
            .collect();
        left_out.sort();
        left_out.dedup();
        left_out
    }

    /// Drops every copy, whether or not the export worked. A failure is returned as a sentence
    /// for the person; a copy that stays is dropped by the next export.
    pub(super) fn drop_all(&self) -> Result<(), String> {
        let failed: Vec<String> = self
            .prepared
            .iter()
            .filter_map(|(_, prepared)| export_scrub::drop_copy(self.env.as_ref(), prepared).err())
            .collect();
        if failed.is_empty() {
            Ok(())
        } else {
            Err(failed.join(" "))
        }
    }
}

/// Makes a copy of each target. A copy that cannot be made drops the ones already made.
pub(super) fn make_copies(
    controller: &Controller,
    env: Arc<dyn ScrubEnv>,
    targets: &[(String, String)],
    cancellation: &backup::Cancellation,
) -> Result<Copies, TransferError> {
    let mut copies = Copies {
        env: env.clone(),
        prepared: Vec::new(),
    };
    let progress = |line: &str| {
        show_progress(controller, line);
        publish(controller);
    };
    show_progress(controller, "Starting");
    publish(controller);
    export_scrub::drop_stale(env.as_ref());
    for (name, id) in targets {
        let prefix = if targets.len() > 1 {
            format!("{name}: ")
        } else {
            String::new()
        };
        let say = |line: &str| progress(&format!("{prefix}{line}"));
        match export_scrub::prepare(env.as_ref(), id, &say, &|| cancellation.cancelled()) {
            Ok(prepared) => copies.prepared.push((name.clone(), prepared)),
            Err(failure) => {
                let cleanup = copies.drop_all();
                end_progress(controller);
                return Err(match failure {
                    // A cancel that left a copy behind says so instead of passing for clean.
                    Failure::Cancelled => match cleanup {
                        Err(note) => {
                            TransferError::from(format!("The export was cancelled. {note}"))
                        }
                        Ok(()) => TransferError::cancelled(),
                    },
                    Failure::Failed(message) => {
                        let message = match cleanup {
                            Ok(()) => message,
                            Err(note) => format!("{message} {note}"),
                        };
                        TransferError::from(format!(
                            "{message} No export file was saved, and the logins stay on the computer."
                        ))
                    }
                });
            }
        }
    }
    end_progress(controller);
    publish(controller);
    Ok(copies)
}

/// Shows `detail` in the phase that leads the progress, adding the phase when it is not there.
fn show_progress(controller: &Controller, detail: &str) {
    let mut view = controller
        .view
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(Operation::Running { phases, .. }) = view.operation.as_mut() else {
        return;
    };
    if let Some(first) = phases
        .first_mut()
        .filter(|phase| phase.title == SCRUB_PHASE)
    {
        first.detail = detail.to_owned();
        return;
    }
    for phase in phases.iter_mut() {
        phase.tone = "waiting";
    }
    phases.insert(
        0,
        Phase {
            title: SCRUB_PHASE.into(),
            detail: detail.to_owned(),
            tone: "running",
        },
    );
}

/// Removes the leading phase and hands the progress back to the export's own phase.
fn end_progress(controller: &Controller) {
    let mut view = controller
        .view
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(Operation::Running { phases, .. }) = view.operation.as_mut() else {
        return;
    };
    if phases
        .first()
        .is_some_and(|phase| phase.title == SCRUB_PHASE)
    {
        phases.remove(0);
        if let Some(next) = phases.first_mut() {
            next.tone = "running";
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::super::tests::history_controller;
    use super::*;
    use crate::recipes::ExecOut;

    /// An engine that can make copies, and fails the fork of the computer it is told to.
    #[derive(Default)]
    struct Fake {
        deleted: Mutex<Vec<String>>,
        forked: Mutex<Vec<String>>,
        fail_fork_of: Option<&'static str>,
    }

    impl ScrubEnv for Fake {
        fn computers(&self) -> Result<Vec<(String, String)>, String> {
            Ok(Vec::new())
        }
        fn fork(&self, source: &str, name: &str) -> Result<String, String> {
            if self.fail_fork_of == Some(source) {
                return Err("no room".into());
            }
            self.forked.lock().unwrap().push(name.to_owned());
            Ok(format!("copy-of-{source}"))
        }
        fn start(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn run(&self, _: &str, _: &str, _: &[u8]) -> Result<ExecOut, String> {
            Ok(ExecOut::default())
        }
        fn login_paths(&self, _: &str) -> Result<Vec<String>, String> {
            Ok(vec!["/root/.claude/.credentials.json".into()])
        }
        fn stop(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn delete(&self, id: &str) -> Result<(), String> {
            self.deleted.lock().unwrap().push(id.to_owned());
            Ok(())
        }
    }

    fn running_controller() -> Controller {
        let controller = history_controller(std::path::PathBuf::from("/unused/history.json"));
        controller.view.lock().unwrap().operation = Some(Operation::Running {
            operation: "backup",
            archive: super::super::tests::completed_archive(),
            running_names: Vec::new(),
            target_name: None,
            progress: 0,
            indeterminate: Some(true),
            can_cancel: None,
            phases: vec![Phase {
                title: "Capture and verify".into(),
                detail: String::new(),
                tone: "running",
            }],
        });
        controller
    }

    fn phases(controller: &Controller) -> Vec<(String, &'static str)> {
        let view = controller.view.lock().unwrap();
        let Some(Operation::Running { phases, .. }) = view.operation.as_ref() else {
            panic!("not running")
        };
        phases
            .iter()
            .map(|phase| (phase.title.clone(), phase.tone))
            .collect()
    }

    fn pairs(names: &[&str]) -> Vec<(String, String)> {
        names
            .iter()
            .map(|name| ((*name).to_owned(), format!("id-{name}")))
            .collect()
    }

    #[test]
    fn only_virtual_machines_among_the_chosen_computers_are_exported_from_a_copy() {
        let computers = vec![
            ("main".to_owned(), "id-main".to_owned(), false),
            ("box".to_owned(), "id-box".to_owned(), true),
            ("other".to_owned(), "id-other".to_owned(), false),
        ];
        let chosen = |names: &[&str]| names.iter().map(|n| (*n).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            pick_targets(&computers, &chosen(&["other", "box", "mac", "main"])),
            [
                ("other".to_owned(), "id-other".to_owned()),
                ("main".to_owned(), "id-main".to_owned())
            ]
        );
        assert!(pick_targets(&computers, &chosen(&["box"])).is_empty());
    }

    #[test]
    fn copies_stand_in_for_their_computers_and_report_what_they_no_longer_hold() {
        let controller = running_controller();
        let env = Arc::new(Fake::default());
        let copies = make_copies(
            &controller,
            env.clone(),
            &pairs(&["main", "dev"]),
            &backup::Cancellation::default(),
        )
        .unwrap();
        let names = ["main".to_owned(), "mac".to_owned(), "dev".to_owned()];
        let export = copies.export_names(&names);
        assert!(export_scrub::is_copy(&export[0]) && export_scrub::is_copy(&export[2]));
        assert_eq!(export[1], "mac");
        let presented = copies.presented();
        assert_eq!(
            presented
                .iter()
                .map(|p| (p.name.as_str(), p.id.as_str()))
                .collect::<Vec<_>>(),
            [("main", "id-main"), ("dev", "id-dev")]
        );
        assert_eq!(
            copies.left_out(),
            ["a Claude Code login (/root/.claude/.credentials.json)"]
        );
        assert!(env.deleted.lock().unwrap().is_empty());
        // The export's own phase leads again once the copies are ready.
        assert_eq!(
            phases(&controller),
            [("Capture and verify".to_owned(), "running")]
        );
        copies.drop_all().unwrap();
        assert_eq!(
            *env.deleted.lock().unwrap(),
            ["copy-of-id-main", "copy-of-id-dev"]
        );
    }

    #[test]
    fn a_copy_that_cannot_be_made_drops_the_ones_already_made_and_names_what_stays() {
        let controller = running_controller();
        let env = Arc::new(Fake {
            fail_fork_of: Some("id-dev"),
            ..Fake::default()
        });
        let error = make_copies(
            &controller,
            env.clone(),
            &pairs(&["main", "dev"]),
            &backup::Cancellation::default(),
        )
        .err()
        .unwrap();
        assert!(!error.cancelled);
        assert!(error.message.contains("no room"), "{}", error.message);
        assert!(
            error
                .message
                .contains("No export file was saved, and the logins stay on the computer"),
            "{}",
            error.message
        );
        assert_eq!(*env.deleted.lock().unwrap(), ["copy-of-id-main"]);
        assert_eq!(
            phases(&controller),
            [("Capture and verify".to_owned(), "running")]
        );
    }

    #[test]
    fn a_cancel_before_the_first_copy_is_a_cancel_and_makes_nothing() {
        let controller = running_controller();
        let env = Arc::new(Fake::default());
        let cancellation = backup::Cancellation::default();
        cancellation.cancel();
        let error = make_copies(&controller, env.clone(), &pairs(&["main"]), &cancellation)
            .err()
            .unwrap();
        assert!(error.cancelled);
        assert!(env.forked.lock().unwrap().is_empty());
    }

    #[test]
    fn the_leading_phase_is_added_once_updated_and_removed() {
        let controller = running_controller();
        show_progress(&controller, "Starting");
        show_progress(&controller, "Starting the copy");
        assert_eq!(
            phases(&controller),
            [
                (SCRUB_PHASE.to_owned(), "running"),
                ("Capture and verify".to_owned(), "waiting")
            ]
        );
        let detail = match controller.view.lock().unwrap().operation.as_ref() {
            Some(Operation::Running { phases, .. }) => phases[0].detail.clone(),
            _ => unreachable!(),
        };
        assert_eq!(detail, "Starting the copy");
        end_progress(&controller);
        end_progress(&controller);
        assert_eq!(
            phases(&controller),
            [("Capture and verify".to_owned(), "running")]
        );
    }
}
