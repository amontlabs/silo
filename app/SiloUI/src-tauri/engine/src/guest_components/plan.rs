//! What a reconcile does about each piece, decided from what the computer has and what Silo
//! wants there. Pure: nothing here talks to a computer.

use super::receipt::{Receipt, State};

/// What Silo has to give a computer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Program {
    /// The file to install, with its SHA-256.
    Available { sha256: String },
    /// This build cannot provide it; the reason is shown as it is.
    Unavailable { reason: String },
}

/// What Silo wants in a computer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Wanted {
    /// Whether Silo's tools are on in the computer.
    pub tools: bool,
    pub program: Program,
    /// The registration with the agents was owed a removal (the tools were turned off while the
    /// computer was not running).
    pub removal_owed: bool,
}

/// What a read of the computer found.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Observed {
    /// The SHA-256 of `/usr/local/bin/silo` when it is a regular file.
    pub program_sha256: Option<String>,
    /// The computer's receipt, already read as untrusted text (`Receipt::parse`).
    pub receipt: Receipt,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProgramStep {
    /// The file in the computer is the wanted one.
    Current,
    /// Put the wanted file in place (it is missing, replaced or old).
    Install,
    /// An older or different file stays: this build has nothing to replace it with.
    Keep { reason: String },
    /// There is no program and none to give.
    Unavailable { reason: String },
    /// The tools are off: the program is left where it is.
    Leave,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegistrationStep {
    /// Register `silo mcp` with the agents (idempotent).
    Apply,
    /// Take it out of the agents' configuration.
    Remove,
    /// Nothing to do.
    Nothing,
    /// The tools are on but there is no program to register.
    WaitForProgram,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Plan {
    pub program: ProgramStep,
    pub registration: RegistrationStep,
}

/// The steps for one computer.
pub fn plan(wanted: &Wanted, observed: &Observed) -> Plan {
    if !wanted.tools {
        let registered = observed
            .receipt
            .entry(super::MCP.id)
            .is_some_and(|entry| matches!(entry.state, State::Ok | State::Waiting | State::Failed));
        return Plan {
            program: ProgramStep::Leave,
            registration: if wanted.removal_owed || registered {
                RegistrationStep::Remove
            } else {
                RegistrationStep::Nothing
            },
        };
    }
    let program = match &wanted.program {
        Program::Available { sha256 } if observed.program_sha256.as_deref() == Some(sha256) => {
            ProgramStep::Current
        }
        Program::Available { .. } => ProgramStep::Install,
        Program::Unavailable { reason } if observed.program_sha256.is_some() => ProgramStep::Keep {
            reason: reason.clone(),
        },
        Program::Unavailable { reason } => ProgramStep::Unavailable {
            reason: reason.clone(),
        },
    };
    let registration = match program {
        ProgramStep::Unavailable { .. } => RegistrationStep::WaitForProgram,
        _ => RegistrationStep::Apply,
    };
    Plan {
        program,
        registration,
    }
}

#[cfg(test)]
mod tests {
    use super::super::receipt::Entry;
    use super::*;

    fn wanted(tools: bool) -> Wanted {
        Wanted {
            tools,
            program: Program::Available {
                sha256: "a".repeat(64),
            },
            removal_owed: false,
        }
    }

    fn observed(sha: Option<&str>) -> Observed {
        Observed {
            program_sha256: sha.map(str::to_owned),
            receipt: Receipt::empty("c"),
        }
    }

    #[test]
    fn the_wanted_program_is_current_and_anything_else_is_replaced() {
        let current = plan(&wanted(true), &observed(Some(&"a".repeat(64))));
        assert_eq!(current.program, ProgramStep::Current);
        assert_eq!(current.registration, RegistrationStep::Apply);
        for found in [None, Some("b".repeat(64))] {
            let p = plan(&wanted(true), &observed(found.as_deref()));
            assert_eq!(p.program, ProgramStep::Install, "{found:?}");
            assert_eq!(p.registration, RegistrationStep::Apply);
        }
    }

    #[test]
    fn the_receipt_never_stands_in_for_the_file() {
        // A receipt that claims the program is in place does not stop a reinstall when the file
        // in the computer is missing or different.
        let mut view = observed(None);
        view.receipt.components.insert(
            "silo".into(),
            Entry::new(1, State::Ok, 1).with_sha256("a".repeat(64)),
        );
        assert_eq!(plan(&wanted(true), &view).program, ProgramStep::Install);
        view.program_sha256 = Some("c".repeat(64));
        assert_eq!(plan(&wanted(true), &view).program, ProgramStep::Install);
    }

    #[test]
    fn without_a_program_to_give_an_existing_one_stays_and_nothing_is_registered_without_one() {
        let mut want = wanted(true);
        want.program = Program::Unavailable {
            reason: "no Zig".into(),
        };
        let kept = plan(&want, &observed(Some(&"b".repeat(64))));
        assert_eq!(
            kept.program,
            ProgramStep::Keep {
                reason: "no Zig".into()
            }
        );
        assert_eq!(kept.registration, RegistrationStep::Apply);
        let none = plan(&want, &observed(None));
        assert_eq!(
            none.program,
            ProgramStep::Unavailable {
                reason: "no Zig".into()
            }
        );
        assert_eq!(none.registration, RegistrationStep::WaitForProgram);
    }

    #[test]
    fn with_the_tools_off_the_program_stays_and_a_registration_is_taken_out() {
        let off = wanted(false);
        let nothing = plan(&off, &observed(Some(&"a".repeat(64))));
        assert_eq!(
            (nothing.program, nothing.registration),
            (ProgramStep::Leave, RegistrationStep::Nothing)
        );
        let mut view = observed(None);
        view.receipt
            .components
            .insert("silo-mcp".into(), Entry::new(1, State::Ok, 1));
        assert_eq!(plan(&off, &view).registration, RegistrationStep::Remove);
        view.receipt
            .components
            .insert("silo-mcp".into(), Entry::new(1, State::Removed, 1));
        assert_eq!(plan(&off, &view).registration, RegistrationStep::Nothing);
        let mut owed = wanted(false);
        owed.removal_owed = true;
        assert_eq!(
            plan(&owed, &observed(None)).registration,
            RegistrationStep::Remove
        );
    }
}
