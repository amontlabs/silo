use std::{cell::RefCell, time::Duration};

use super::{
    artifact::{self, sha256_hex, Located, Payload},
    receipt::{Receipt, State},
    run::{reconcile, Guest, Kind, Output, Request, Source, Who},
    scripts, MCP, SILO,
};

const HELPER_TEXT: &str = "#!/usr/bin/python3\n# helper\n";

/// A computer that answers the commands a reconcile runs, from state it keeps.
struct Fake {
    kind: Kind,
    program: RefCell<Option<Vec<u8>>>,
    helper: RefCell<Option<Vec<u8>>>,
    receipt: RefCell<Option<Vec<u8>>>,
    agents_installed: Vec<&'static str>,
    broken_agents: Vec<&'static str>,
    install_fails: bool,
    helper_write_fails: bool,
    receipt_write_fails: bool,
    unreadable: bool,
    observe_delay: Duration,
    conflicting: Vec<&'static str>,
    timeouts: RefCell<Vec<Duration>>,
    registered: RefCell<Vec<String>>,
    calls: RefCell<Vec<(Who, String)>>,
}

impl Fake {
    fn linux() -> Self {
        Self {
            kind: Kind::Linux,
            program: RefCell::new(None),
            helper: RefCell::new(None),
            receipt: RefCell::new(None),
            agents_installed: vec!["claude-code", "codex"],
            broken_agents: vec![],
            install_fails: false,
            helper_write_fails: false,
            receipt_write_fails: false,
            unreadable: false,
            observe_delay: Duration::ZERO,
            conflicting: vec![],
            timeouts: RefCell::new(vec![]),
            registered: RefCell::new(vec![]),
            calls: RefCell::new(vec![]),
        }
    }

    fn count(&self, what: &str) -> usize {
        self.calls
            .borrow()
            .iter()
            .filter(|(_, call)| call == what)
            .count()
    }

    fn out(code: i32, stdout: impl Into<Vec<u8>>, stderr: &str) -> Result<Output, String> {
        Ok(Output {
            code: Some(code),
            stdout: stdout.into(),
            stderr: stderr.as_bytes().to_vec(),
        })
    }
}

impl Guest for Fake {
    fn kind(&self) -> Kind {
        self.kind
    }

    fn run(
        &self,
        who: Who,
        argv: Vec<String>,
        stdin: Option<Vec<u8>>,
        timeout: Duration,
        _max: usize,
    ) -> Result<Output, String> {
        self.timeouts.borrow_mut().push(timeout);
        let script = argv.get(2).cloned().unwrap_or_default();
        if argv[0] == "sh" && script.contains("program=%s") {
            self.calls.borrow_mut().push((who, "observe".into()));
            std::thread::sleep(self.observe_delay);
            if self.unreadable {
                return Err("The computer is not running.".into());
            }
            let hash =
                |bytes: &Option<Vec<u8>>| bytes.as_ref().map(|b| sha256_hex(b)).unwrap_or_default();
            let mut out = format!(
                "program={}\nhelper={}\nreceipt:\n",
                hash(&self.program.borrow()),
                hash(&self.helper.borrow())
            )
            .into_bytes();
            out.extend(self.receipt.borrow().clone().unwrap_or_default());
            return Self::out(0, out, "");
        }
        if argv[0] == "sh" && script == scripts::INSTALL_SCRIPT {
            self.calls.borrow_mut().push((who, "install".into()));
            assert_eq!(who, Who::Root);
            let bytes = stdin.unwrap();
            if self.install_fails {
                return Self::out(1, "", "mv: cannot move: Operation not permitted");
            }
            if sha256_hex(&bytes) != argv[4] {
                return Self::out(
                    1,
                    "",
                    "The program that arrived does not match its checksum; nothing was replaced.",
                );
            }
            assert_eq!(argv[5], scripts::PROGRAM_PATH);
            *self.program.borrow_mut() = Some(bytes);
            return Self::out(0, "", "");
        }
        if argv[0] == "sh" && script.starts_with("set -eu\numask 022") {
            let bytes = stdin.unwrap();
            if script.contains("components.json") {
                self.calls.borrow_mut().push((who, "write-receipt".into()));
                if self.receipt_write_fails {
                    return Self::out(1, "", "Read-only file system");
                }
                *self.receipt.borrow_mut() = Some(bytes);
            } else {
                self.calls.borrow_mut().push((who, "write-helper".into()));
                if self.helper_write_fails {
                    return Self::out(1, "", "No space left on device");
                }
                *self.helper.borrow_mut() = Some(bytes);
            }
            return Self::out(0, "", "");
        }
        let add = argv.iter().any(|word| word == "server-add");
        let remove = argv.iter().any(|word| word == "server-remove");
        if add || remove {
            self.calls
                .borrow_mut()
                .push((who, if add { "register" } else { "unregister" }.into()));
            let reply = if add {
                let registered: Vec<&str> = self
                    .agents_installed
                    .iter()
                    .copied()
                    .filter(|a| !self.broken_agents.contains(a) && !self.conflicting.contains(a))
                    .collect();
                let pending: Vec<&str> = ["claude-code", "codex"]
                    .into_iter()
                    .filter(|a| !self.agents_installed.contains(a))
                    .collect();
                *self.registered.borrow_mut() =
                    registered.iter().map(|a| (*a).to_owned()).collect();
                serde_json::json!({"server":"silo","registered":registered,"pending":pending,"failed":self.broken_agents,"conflicts":self.conflicting})
            } else {
                self.registered.borrow_mut().clear();
                serde_json::json!({"server":"silo","removed":["claude-code","codex"],"failed":self.broken_agents,"conflicts":self.conflicting})
            };
            return Self::out(
                i32::from(!self.broken_agents.is_empty()),
                format!("{reply}\n"),
                "",
            );
        }
        panic!("unexpected command {argv:?}");
    }
}

fn payload(bytes: &[u8]) -> Payload {
    Payload {
        sha256: sha256_hex(bytes),
        bytes: bytes.to_vec(),
    }
}

fn ask<'a>(source: Source<'a>, tools: bool, owed: bool) -> Request<'a> {
    Request {
        computer_id: "c1",
        tools,
        removal_owed: owed,
        source,
        helper_text: HELPER_TEXT,
        now: 1000,
        budget: Duration::from_secs(90),
    }
}

fn state(receipt: &Receipt, id: &str) -> State {
    receipt
        .entry(id)
        .unwrap_or_else(|| panic!("no entry for {id}"))
        .state
}

#[test]
fn a_new_computer_gets_the_program_the_helper_and_the_registration_and_a_receipt() {
    let guest = Fake::linux();
    let program = payload(b"ELF silo");
    let receipt = reconcile(&guest, &ask(Source::Override(&program), true, false)).unwrap();
    assert_eq!(state(&receipt, SILO.id), State::Ok);
    assert_eq!(
        receipt.entry(SILO.id).unwrap().sha256.as_deref(),
        Some(program.sha256.as_str())
    );
    assert_eq!(state(&receipt, MCP.id), State::Ok);
    assert_eq!(guest.program.borrow().as_deref(), Some(&b"ELF silo"[..]));
    assert_eq!(
        guest.helper.borrow().as_deref(),
        Some(HELPER_TEXT.as_bytes())
    );
    assert_eq!(*guest.registered.borrow(), vec!["claude-code", "codex"]);
    let written = Receipt::parse(guest.receipt.borrow().as_deref().unwrap(), "c1");
    assert_eq!(written, receipt, "the computer holds the same receipt");
    // Everything in the computer ran as root on Linux.
    assert!(guest
        .calls
        .borrow()
        .iter()
        .filter(|(_, c)| c != "observe")
        .all(|(who, _)| *who == Who::Root));
}

#[test]
fn a_look_that_finds_everything_current_changes_nothing_in_the_computer() {
    let guest = Fake::linux();
    let program = payload(b"ELF silo");
    reconcile(&guest, &ask(Source::Override(&program), true, false)).unwrap();
    let mut later = ask(Source::Override(&program), true, false);
    later.now = 5000;
    let receipt = reconcile(&guest, &later).unwrap();
    assert_eq!(guest.count("install"), 1);
    assert_eq!(guest.count("write-helper"), 1, "the helper is current too");
    assert_eq!(
        guest.count("register"),
        1,
        "the registration worked already"
    );
    assert_eq!(
        guest.count("write-receipt"),
        1,
        "only the times would change"
    );
    assert_eq!(state(&receipt, MCP.id), State::Ok);
    assert_eq!(receipt.entry(SILO.id).unwrap().at, 5000);
}

#[test]
fn a_registration_is_made_again_when_the_helper_changed_or_something_was_waiting() {
    let guest = Fake::linux();
    let program = payload(b"ELF silo");
    reconcile(&guest, &ask(Source::Override(&program), true, false)).unwrap();
    *guest.helper.borrow_mut() = Some(b"another helper".to_vec());
    reconcile(&guest, &ask(Source::Override(&program), true, false)).unwrap();
    assert_eq!(guest.count("write-helper"), 2);
    assert_eq!(guest.count("register"), 2);

    let mut waiting = Fake::linux();
    waiting.agents_installed = vec!["claude-code"];
    reconcile(&waiting, &ask(Source::Override(&program), true, false)).unwrap();
    waiting.agents_installed = vec!["claude-code", "codex"];
    let receipt = reconcile(&waiting, &ask(Source::Override(&program), true, false)).unwrap();
    assert_eq!(waiting.count("register"), 2);
    assert_eq!(receipt.entry(MCP.id).unwrap().detail, None);
}

#[test]
fn a_program_named_earlier_stays_while_the_looks_that_follow_keep_it() {
    let guest = Fake::linux();
    let named = payload(b"ELF dev silo");
    reconcile(&guest, &ask(Source::Override(&named), true, false)).unwrap();
    let receipt = reconcile(&guest, &ask(Source::Kept, true, false)).unwrap();
    assert_eq!(guest.count("install"), 1);
    assert_eq!(
        guest.program.borrow().as_deref(),
        Some(&b"ELF dev silo"[..])
    );
    let silo = receipt.entry(SILO.id).unwrap();
    assert_eq!(silo.state, State::Ok);
    assert_eq!(silo.sha256.as_deref(), Some(named.sha256.as_str()));
    // A program that is gone is not put back from nothing.
    *guest.program.borrow_mut() = None;
    let receipt = reconcile(&guest, &ask(Source::Kept, true, false)).unwrap();
    assert_eq!(guest.count("install"), 1);
    assert_eq!(state(&receipt, SILO.id), State::Unavailable);
    assert_eq!(state(&receipt, MCP.id), State::Waiting);
}

#[test]
fn a_reconcile_that_runs_out_of_time_skips_what_is_left_and_says_so() {
    let mut guest = Fake::linux();
    guest.observe_delay = Duration::from_millis(60);
    let program = payload(b"ELF silo");
    let mut request = ask(Source::Override(&program), true, false);
    request.budget = Duration::from_millis(20);
    let receipt = reconcile(&guest, &request).unwrap();
    for id in [SILO.id, MCP.id] {
        let entry = receipt.entry(id).unwrap();
        assert_eq!(entry.state, State::Waiting, "{id}");
        assert_eq!(entry.detail.as_deref(), Some(super::run::OUT_OF_TIME));
    }
    for step in ["install", "write-helper", "register", "write-receipt"] {
        assert_eq!(guest.count(step), 0, "{step}");
    }
    assert!(super::went_wrong(&receipt));
}

#[test]
fn every_step_is_limited_to_the_time_that_is_left() {
    let guest = Fake::linux();
    let program = payload(b"ELF silo");
    let mut request = ask(Source::Override(&program), true, false);
    request.budget = Duration::from_secs(20);
    reconcile(&guest, &request).unwrap();
    let timeouts = guest.timeouts.borrow();
    assert!(!timeouts.is_empty());
    assert!(timeouts
        .iter()
        .all(|timeout| *timeout <= Duration::from_secs(20)));
}

#[test]
fn a_deleted_or_replaced_program_comes_back_at_the_next_reconcile() {
    let guest = Fake::linux();
    let program = payload(b"ELF silo");
    reconcile(&guest, &ask(Source::Override(&program), true, false)).unwrap();
    *guest.program.borrow_mut() = None;
    reconcile(&guest, &ask(Source::Override(&program), true, false)).unwrap();
    assert_eq!(guest.program.borrow().as_deref(), Some(&b"ELF silo"[..]));
    *guest.program.borrow_mut() = Some(b"an agent wrote this".to_vec());
    reconcile(&guest, &ask(Source::Override(&program), true, false)).unwrap();
    assert_eq!(guest.program.borrow().as_deref(), Some(&b"ELF silo"[..]));
    assert_eq!(guest.count("install"), 3);
}

#[test]
fn a_failed_install_is_recorded_and_the_rest_still_runs() {
    let mut guest = Fake::linux();
    *guest.program.borrow_mut() = Some(b"old silo".to_vec());
    guest.install_fails = true;
    let program = payload(b"ELF silo");
    let receipt = reconcile(&guest, &ask(Source::Override(&program), true, false)).unwrap();
    let silo = receipt.entry(SILO.id).unwrap();
    assert_eq!(silo.state, State::Failed);
    assert!(silo
        .detail
        .as_deref()
        .unwrap()
        .contains("Operation not permitted"));
    assert_eq!(
        silo.sha256.as_deref(),
        Some(sha256_hex(b"old silo").as_str()),
        "it records what is there"
    );
    assert_eq!(
        state(&receipt, MCP.id),
        State::Ok,
        "the old program is registered"
    );
    assert_eq!(guest.program.borrow().as_deref(), Some(&b"old silo"[..]));
    assert!(
        guest.receipt.borrow().is_some(),
        "the failure is in the computer's receipt"
    );
}

#[test]
fn without_a_program_a_failed_install_leaves_the_registration_waiting() {
    let mut guest = Fake::linux();
    guest.install_fails = true;
    let program = payload(b"ELF silo");
    let receipt = reconcile(&guest, &ask(Source::Override(&program), true, false)).unwrap();
    assert_eq!(state(&receipt, SILO.id), State::Failed);
    assert_eq!(state(&receipt, MCP.id), State::Waiting);
    assert_eq!(guest.count("register"), 0);
}

#[test]
fn a_program_that_is_not_the_manifests_is_refused_before_anything_is_sent() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join(artifact::RESOURCE_DIRECTORY);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join("manifest.json"),
        serde_json::json!({"schemaVersion":1,"available":true,"architecture":"arm64",
            "binarySha256": sha256_hex(b"the real one"), "size": 12})
        .to_string(),
    )
    .unwrap();
    std::fs::write(folder.join("silo"), b"swapped file").unwrap();
    let located = artifact::linux(dir.path(), "arm64");
    let guest = Fake::linux();
    let receipt = reconcile(&guest, &ask(Source::Located(&located), true, false)).unwrap();
    let silo = receipt.entry(SILO.id).unwrap();
    assert_eq!(silo.state, State::Failed);
    assert!(
        silo.detail.as_deref().unwrap().contains("checksum"),
        "{:?}",
        silo.detail
    );
    assert_eq!(guest.count("install"), 0);
    assert!(guest.program.borrow().is_none());
}

#[test]
fn the_script_in_the_computer_refuses_bytes_that_do_not_match_what_was_asked() {
    // A payload whose checksum is not its bytes' (corrupted on the way): the computer's check
    // stops it and nothing is replaced.
    let guest = Fake::linux();
    *guest.program.borrow_mut() = Some(b"old silo".to_vec());
    let lying = Payload {
        sha256: sha256_hex(b"what was meant"),
        bytes: b"what arrived".to_vec(),
    };
    let receipt = reconcile(&guest, &ask(Source::Override(&lying), true, false)).unwrap();
    assert_eq!(state(&receipt, SILO.id), State::Failed);
    assert!(receipt
        .entry(SILO.id)
        .unwrap()
        .detail
        .as_deref()
        .unwrap()
        .contains("does not match its checksum"));
    assert_eq!(guest.program.borrow().as_deref(), Some(&b"old silo"[..]));
}

#[test]
fn a_build_without_the_program_says_so_and_keeps_an_old_one() {
    let located = Located::Unavailable(
        "Zig is not installed. Silo's tools cannot be installed in Linux computers by this build."
            .into(),
    );
    let guest = Fake::linux();
    let receipt = reconcile(&guest, &ask(Source::Located(&located), true, false)).unwrap();
    assert_eq!(state(&receipt, SILO.id), State::Unavailable);
    assert!(receipt
        .entry(SILO.id)
        .unwrap()
        .detail
        .as_deref()
        .unwrap()
        .contains("cannot be installed in Linux computers"));
    assert_eq!(state(&receipt, MCP.id), State::Waiting);
    assert_eq!((guest.count("install"), guest.count("register")), (0, 0));
    *guest.program.borrow_mut() = Some(b"older silo".to_vec());
    let receipt = reconcile(&guest, &ask(Source::Located(&located), true, false)).unwrap();
    assert_eq!(state(&receipt, SILO.id), State::Unavailable);
    assert_eq!(state(&receipt, MCP.id), State::Ok);
    assert_eq!(guest.program.borrow().as_deref(), Some(&b"older silo"[..]));
}

#[test]
fn an_agent_that_is_not_installed_is_waited_for_and_one_that_is_gets_registered() {
    let mut guest = Fake::linux();
    guest.agents_installed = vec!["claude-code"];
    let program = payload(b"ELF silo");
    let receipt = reconcile(&guest, &ask(Source::Override(&program), true, false)).unwrap();
    let mcp = receipt.entry(MCP.id).unwrap();
    assert_eq!(mcp.state, State::Ok);
    assert!(
        mcp.detail
            .as_deref()
            .unwrap()
            .contains("Codex not installed yet"),
        "{:?}",
        mcp.detail
    );
    guest.agents_installed.clear();
    let receipt = reconcile(&guest, &ask(Source::Override(&program), true, false)).unwrap();
    assert_eq!(state(&receipt, MCP.id), State::Waiting);
}

#[test]
fn a_registration_that_fails_is_recorded_as_a_failure() {
    let mut guest = Fake::linux();
    guest.broken_agents = vec!["codex"];
    let program = payload(b"ELF silo");
    let receipt = reconcile(&guest, &ask(Source::Override(&program), true, false)).unwrap();
    let mcp = receipt.entry(MCP.id).unwrap();
    assert_eq!(mcp.state, State::Failed);
    assert!(
        mcp.detail.as_deref().unwrap().contains("Codex"),
        "{:?}",
        mcp.detail
    );
    assert_eq!(state(&receipt, SILO.id), State::Ok);
    let mut guest = Fake::linux();
    guest.helper_write_fails = true;
    let receipt = reconcile(&guest, &ask(Source::Override(&program), true, false)).unwrap();
    assert_eq!(state(&receipt, MCP.id), State::Failed);
    assert!(receipt
        .entry(MCP.id)
        .unwrap()
        .detail
        .as_deref()
        .unwrap()
        .contains("No space left"));
}

#[test]
fn turning_the_tools_off_removes_the_registration_and_keeps_the_program() {
    let guest = Fake::linux();
    let program = payload(b"ELF silo");
    reconcile(&guest, &ask(Source::Override(&program), true, false)).unwrap();
    let receipt = reconcile(&guest, &ask(Source::Override(&program), false, false)).unwrap();
    assert_eq!(state(&receipt, MCP.id), State::Removed);
    assert_eq!(guest.count("unregister"), 1);
    assert!(guest.registered.borrow().is_empty());
    assert_eq!(
        guest.program.borrow().as_deref(),
        Some(&b"ELF silo"[..]),
        "the program stays"
    );
    assert_eq!(
        state(&receipt, SILO.id),
        State::Ok,
        "the program's entry is not touched"
    );
    // Nothing is left to remove at the next reconcile.
    reconcile(&guest, &ask(Source::Override(&program), false, false)).unwrap();
    assert_eq!(guest.count("unregister"), 1);
}

#[test]
fn a_computer_that_never_had_the_tools_is_left_alone_unless_a_removal_is_owed() {
    let guest = Fake::linux();
    let program = payload(b"ELF silo");
    let receipt = reconcile(&guest, &ask(Source::Override(&program), false, false)).unwrap();
    assert!(receipt.components.is_empty());
    assert_eq!(
        (
            guest.count("install"),
            guest.count("unregister"),
            guest.count("write-helper")
        ),
        (0, 0, 0)
    );
    let receipt = reconcile(&guest, &ask(Source::Override(&program), false, true)).unwrap();
    assert_eq!(state(&receipt, MCP.id), State::Removed);
    assert_eq!(guest.count("unregister"), 1);
}

#[test]
fn a_hostile_receipt_is_ignored_and_a_receipt_that_cannot_be_written_does_not_matter() {
    let mut guest = Fake::linux();
    *guest.receipt.borrow_mut() = Some(vec![b'{'; 200_000]);
    guest.receipt_write_fails = true;
    let program = payload(b"ELF silo");
    let receipt = reconcile(&guest, &ask(Source::Override(&program), true, false)).unwrap();
    assert_eq!(state(&receipt, SILO.id), State::Ok);
    assert_eq!(state(&receipt, MCP.id), State::Ok);
    assert_eq!(
        guest.receipt.borrow().as_ref().map(Vec::len),
        Some(200_000),
        "the old text is untouched"
    );
}

#[test]
fn a_computer_that_cannot_be_read_is_an_error_to_try_again_not_a_recorded_failure() {
    let mut guest = Fake::linux();
    guest.unreadable = true;
    let program = payload(b"ELF silo");
    assert!(reconcile(&guest, &ask(Source::Override(&program), true, false)).is_err());
    assert_eq!(guest.calls.borrow().len(), 1);
}

#[test]
fn a_macos_computer_runs_its_registration_and_files_as_the_working_account() {
    let mut guest = Fake::linux();
    guest.kind = Kind::Macos;
    let program = payload(b"Mach-O silo");
    let receipt = reconcile(&guest, &ask(Source::Override(&program), true, false)).unwrap();
    assert_eq!(state(&receipt, MCP.id), State::Ok);
    let calls = guest.calls.borrow();
    for (who, call) in calls.iter() {
        let expected = if call == "install" {
            Who::Root
        } else {
            Who::Account
        };
        assert_eq!(*who, expected, "{call}");
    }
}

#[test]
fn the_receipt_of_another_computer_is_not_trusted() {
    let guest = Fake::linux();
    let program = payload(b"ELF silo");
    reconcile(&guest, &ask(Source::Override(&program), true, false)).unwrap();
    // The same disk now belongs to a fork with another id: it reads as new and is reconciled whole.
    let mut request = ask(Source::Override(&program), true, false);
    request.computer_id = "c2";
    let receipt = reconcile(&guest, &request).unwrap();
    assert_eq!(receipt.computer_id, "c2");
    assert_eq!(state(&receipt, MCP.id), State::Ok);
    assert_eq!(guest.count("register"), 2);
}

#[test]
fn the_component_list_names_each_piece_once_with_a_version() {
    assert_eq!(
        super::COMPONENTS.iter().map(|c| c.id).collect::<Vec<_>>(),
        vec!["silo", "silo-mcp"]
    );
    assert!(super::COMPONENTS.iter().all(|c| c.version >= 1));
}

#[test]
fn the_status_shows_the_last_reconcile_and_a_build_that_cannot_provide_the_program() {
    use silo_api::methods::guest_client::GuestComponentState as S;
    let dir = tempfile::tempdir().unwrap();
    let empty = tempfile::tempdir().unwrap();
    let sources = super::Sources {
        resources: empty.path(),
        tools_directory: None,
    };
    // No reconcile yet and nothing to give: the program is shown as unavailable.
    let shown = super::status(dir.path(), "c1", false, &sources, true);
    assert_eq!(shown.len(), 1);
    assert_eq!(
        (shown[0].id.as_str(), shown[0].state),
        ("silo", S::Unavailable)
    );
    assert!(shown[0].detail.as_deref().unwrap().contains("Linux"));
    assert!(super::status(dir.path(), "c1", false, &sources, false).is_empty());
    // After a reconcile its entries are shown.
    let guest = Fake::linux();
    let program = payload(b"ELF silo");
    let receipt = reconcile(&guest, &ask(Source::Override(&program), true, false)).unwrap();
    super::reports(dir.path()).save(&receipt).unwrap();
    let shown = super::status(dir.path(), "c1", false, &sources, true);
    assert_eq!(
        shown
            .iter()
            .map(|c| (c.id.as_str(), c.state))
            .collect::<Vec<_>>(),
        vec![("silo", S::Ok), ("silo-mcp", S::Ok)]
    );
}

fn receipt_with(state: State) -> Receipt {
    let mut receipt = Receipt::empty("c1");
    receipt.components.insert(
        MCP.id.into(),
        super::receipt::Entry::new(MCP.version, state, 1),
    );
    receipt
}

#[test]
fn an_owed_removal_is_cleared_only_when_the_registration_worked() {
    let dir = tempfile::tempdir().unwrap();
    let reports = super::reports(dir.path());
    let owed = |tools: bool, state: State| {
        reports.owe_removal(&["c1"]).unwrap();
        super::finish(&reports, &receipt_with(state), tools);
        reports.is_owed("c1")
    };
    for state in [State::Waiting, State::Failed, State::Removed] {
        assert!(owed(true, state), "tools on, {state:?}");
    }
    assert!(!owed(true, State::Ok));
    for state in [State::Waiting, State::Failed, State::Ok] {
        assert!(owed(false, state), "tools off, {state:?}");
    }
    assert!(!owed(false, State::Removed));
}

#[test]
fn an_agents_own_server_of_the_same_name_is_reported_plainly_and_left_alone() {
    let mut guest = Fake::linux();
    guest.conflicting = vec!["codex"];
    let program = payload(b"ELF silo");
    let receipt = reconcile(&guest, &ask(Source::Override(&program), true, false)).unwrap();
    let mcp = receipt.entry(MCP.id).unwrap();
    assert_eq!(mcp.state, State::Failed);
    let detail = mcp.detail.as_deref().unwrap();
    assert!(
        detail.contains("Codex already has a tool of its own called silo"),
        "{detail}"
    );
    assert!(super::went_wrong(&receipt));
    // Taking Silo's tools out has nothing of Silo's to take out there, which is done.
    let receipt = reconcile(&guest, &ask(Source::Override(&program), false, true)).unwrap();
    assert_eq!(state(&receipt, MCP.id), State::Removed);
}

#[test]
fn with_the_tools_off_what_the_computer_wrote_in_its_receipt_is_not_shown_as_status() {
    let guest = Fake::linux();
    let program = payload(b"ELF silo");
    reconcile(&guest, &ask(Source::Override(&program), true, false)).unwrap();
    let mut lying = Receipt::parse(guest.receipt.borrow().as_deref().unwrap(), "c1");
    lying.components.get_mut(SILO.id).unwrap().detail =
        Some("Visit example.invalid to repair this".into());
    *guest.receipt.borrow_mut() = Some(lying.to_json().into_bytes());
    let receipt = reconcile(&guest, &ask(Source::Override(&program), false, false)).unwrap();
    assert!(receipt
        .components
        .values()
        .all(|entry| entry.detail.is_none()));
    let written = Receipt::parse(guest.receipt.borrow().as_deref().unwrap(), "c1");
    assert!(written
        .components
        .values()
        .all(|entry| entry.detail.is_none()));
}

#[test]
fn a_computer_that_keeps_failing_is_given_up_until_a_start_and_a_named_program_is_remembered() {
    use super::standing;
    let id = "c-standing";
    for _ in 0..standing::MAX_FAILURES {
        standing::note_look(id, true);
    }
    assert!(standing::given_up(id));
    standing::start_over(id);
    assert!(!standing::given_up(id));
    standing::name_program(id);
    assert!(standing::program_named(id));
    super::forget(tempfile::tempdir().unwrap().path(), id);
    assert!(!standing::program_named(id));
}
