//! Model-based test: a computer's network access never widens without the owner's approval.
//!
//! Random sequences of operations run over a few computers against the real engine code (the
//! policy store, `network.policy.set`, the agent rules, fork, the closed copy an export makes,
//! import, the rebuild swap, macOS import publish and recovery, Silo's tools, preset updates,
//! restarts and a damaged policy file). A reference model keeps what the owner approved for
//! each computer. After every step the rules each computer would be started with (the `Plan`
//! the runtime or the macOS gateway gets, read as the runtime's JSON) are checked against the
//! model by code written here: never wider (the safety rule) and equal (the model agrees).
//!
//! Entry points that can change a computer's network access, and whether this test drives them:
//!
//! | Entry point | Covered |
//! | --- | --- |
//! | `network_policy::service::set` (`network.policy.set`, owner) | yes, Linux and macOS targets, with and without `acceptPresets` |
//! | `policy::decide` `network.policy.set` (agent) + the engine running an allowed change with `expected` | yes |
//! | approval allowed by the owner, then run (agent set, agent fork) | partly: the call is run as the owner would; `silod` approvals code is not |
//! | `checkpoints::fork_commit` (owner fork, kept or given policy) | yes |
//! | `policy::decide` `checkpoints.fork` (agent) + `fork_commit` with the rewritten policy | yes |
//! | `fork_commit` with `closed_network` (the copy an export makes) | yes |
//! | `checkpoints::import_pending_restore_with_environment` (import, import over an id) | yes |
//! | `rebuild::engine_env::{save_build_policies, swap_with_policy, settle_undo_policy, forget_build_policies}` with the swap interrupted at every state, unreadable rules, failed publish, then retried | yes |
//! | `macos_computers::{publish_import_folder, recover_archive_import_in}` (macOS import, interrupted, recovered) | yes |
//! | `network_policy::computer_removed` (delete) | yes (the store and tools steps of a delete, not `runtime::delete`) |
//! | `guest_client::{turn_on, turn_off, inherit, computer_removed}` (channel rule) | yes |
//! | preset updates (a newer Silo's preset list) and `acceptPresets` | yes |
//! | secret hosts added to an allowlist (`Grants`) | partly: one owner-assigned grant for every computer through `use_test_grants`; per-computer assignment through `secrets::save_secret` needs the vault |
//! | restart: the store, tokens and rebuild state reread from disk | yes |
//! | damaged policy file (bad JSON, newer version, unknown mode) and repair | yes |
//! | missing policy file | yes, in `missing_policy_file_never_widens` (ignored: it fails, see its comment) |
//! | `silod` `computers.create` (`store::set` before the create job) | no: the create job needs the runtime (a VM) |
//! | macOS create (`macos_computers` `set_stored` at create) and macOS fork (`store::inherit`) | no: need an APFS clone and a VM |
//! | `runtime::update_save` (engine update restores `desired_network_policy`) | no: needs a running computer |
//! | `apply_network_policy_change` / start of a pending computer (`plan_for` with the recorded rules) | the plan is computed exactly as the pending start does; the restore itself needs `msb` |
//! | `changes.promote` / `changes.undo` | no: they move maker records and sandboxes, not network policies (no policy write found); a VM is needed to drive them |
//! | `network.publish`, `sshAccess.save` (ports, SSH) | no: port mappings live in the runtime's network state |
//! | container blocked policy (`containers::secrets`) | no: needs the container runtime |
//! | GitHub grant hosts and their pin | no: GitHub access needs the GitHub stores |
//!
//! Two variants are ignored because the engine fails them:
//! - `missing_policy_file_never_widens`: with `network-policy.json` gone, a macOS computer has no
//!   entry and runs open (`effective(None, None)`), and a Linux computer runs by the rules its
//!   runtime holds, which are open for a computer the owner closed before its next start.
//! - `incoming_connections_stay_refused`: an agent's fork of a computer that refuses incoming
//!   connections (an import) accepts them (`store::inherit` with a policy saves
//!   `incoming_denied: false`), and an agent's narrowing of a computer with no saved policy whose
//!   rules deny incoming connections saves a policy that accepts them.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use proptest::prelude::*;
use proptest::test_runner::Config;
use proptest_state_machine::{prop_state_machine, ReferenceStateMachine, StateMachineTest};
use serde_json::{json, Value};
use silo_api::{methods::MethodName, transport::ApprovalKind};

use super::*;
use crate::network_policy::{
    self as np, service, store as policy_store, Allowed as PolicyHost, Grants, Held, NetworkPolicy,
    NetworkPolicyMode, Preset,
};
use crate::policy::{self, Access, Caller, Decision, Resources};
use crate::rebuild::{swap, test_access as rebuild_access, Plan as RebuildPlan};

// MARK: The universe

const HOSTS: [&str; 4] = [
    "a.example.com",
    "b.example.com",
    "c.example.org",
    "*.example.com",
];
const GRANT_HOSTS: [&str; 2] = ["c.example.org", "d.example.net"];
const PRESET: &str = "tools";
const MAX_COMPUTERS: usize = 6;
const GROUP: &str = "dev";
const MEMBER: &str = "c000000000000000000000000000000";

/// The hosts of the `tools` preset at a version.
fn preset_hosts(version: u32) -> Vec<&'static str> {
    match version {
        1 => vec!["a.example.com"],
        _ => vec!["a.example.com", "b.example.com"],
    }
}

fn presets_at(version: u32) -> Vec<Preset> {
    let at = |host: &str| PolicyHost {
        host: host.into(),
        port: np::HTTPS_PORT,
    };
    let hosts = |version| preset_hosts(version).into_iter().map(at).collect();
    vec![Preset {
        name: PRESET.into(),
        title: "Tools".into(),
        version,
        hosts: hosts(version),
        history: (1..version).map(|old| (old, hosts(old))).collect(),
    }]
}

fn open_rules() -> Value {
    json!({"default_egress": "allow", "default_ingress": "allow", "rules": []})
}

fn deny_all() -> Value {
    json!({"default_egress": "deny", "default_ingress": "deny", "rules": []})
}

// MARK: The reference model

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Open,
    List,
    Closed,
}

/// A policy as the owner approved it: the mode, the hosts, and the `tools` preset with the
/// version it was approved at.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Approved {
    mode: Mode,
    hosts: BTreeSet<&'static str>,
    tools: Option<u32>,
    incoming_denied: bool,
}

impl Approved {
    fn closed_without_incoming() -> Self {
        Self {
            mode: Mode::Closed,
            hosts: BTreeSet::new(),
            tools: None,
            incoming_denied: true,
        }
    }
}

/// A policy as an operation asks for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Spec {
    mode: Mode,
    hosts: u8,
    tools: bool,
}

impl Spec {
    fn hosts(&self) -> BTreeSet<&'static str> {
        HOSTS
            .iter()
            .enumerate()
            .filter(|(index, _)| self.hosts & (1 << index) != 0)
            .map(|(_, host)| *host)
            .collect()
    }

    fn policy(&self) -> NetworkPolicy {
        NetworkPolicy {
            mode: match self.mode {
                Mode::Open => NetworkPolicyMode::Open,
                Mode::List => NetworkPolicyMode::Allowlist,
                Mode::Closed => NetworkPolicyMode::Closed,
            },
            hosts: self.hosts().into_iter().map(str::to_owned).collect(),
            presets: if self.tools {
                vec![PRESET.into()]
            } else {
                Vec::new()
            },
        }
    }

    /// The policy approved now, at `version`, keeping the version `before` holds for a preset
    /// it already lists.
    fn approved(&self, version: u32, before: Option<&Approved>, incoming_denied: bool) -> Approved {
        Approved {
            mode: self.mode,
            hosts: self.hosts(),
            tools: self
                .tools
                .then(|| before.and_then(|before| before.tools).unwrap_or(version)),
            incoming_denied,
        }
    }

    /// The policy as an agent asks for it: every preset at this Silo's version.
    fn asked(&self, version: u32) -> Approved {
        self.approved(version, None, false)
    }
}

/// What a computer's network lets through, as the runtime reads its rules.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Reach {
    public: bool,
    dns: bool,
    /// `(pattern, port)`; a pattern `*.x` is the runtime's suffix rule for `x`.
    domains: BTreeSet<(String, u16)>,
    /// TCP ports on the device (the gateway address).
    device_ports: BTreeSet<u16>,
    ingress: bool,
    /// Whether the runtime's strict mode is off (a looser check, see the threat model).
    loose: bool,
}

fn pattern_covers(have: &(String, u16), want: &(String, u16)) -> bool {
    if have.1 != want.1 {
        return false;
    }
    match have.0.strip_prefix("*.") {
        None => !want.0.starts_with("*.") && have.0 == want.0,
        Some(base) => {
            let theirs = want.0.strip_prefix("*.").unwrap_or(&want.0);
            theirs == base || theirs.ends_with(&format!(".{base}"))
        }
    }
}

impl Reach {
    /// Whether everything `self` lets through `other` lets through too.
    fn within(&self, other: &Reach) -> Result<(), String> {
        let mut wider = Vec::new();
        if self.public && !other.public {
            wider.push("every public address".to_owned());
        }
        if self.dns && !other.dns {
            wider.push("name lookups".to_owned());
        }
        if !other.public {
            for domain in &self.domains {
                if !other
                    .domains
                    .iter()
                    .any(|have| pattern_covers(have, domain))
                {
                    wider.push(format!("{}:{}", domain.0, domain.1));
                }
            }
        }
        for port in self.device_ports.difference(&other.device_ports) {
            wider.push(format!("device port {port}"));
        }
        if self.ingress && !other.ingress {
            wider.push("incoming connections".to_owned());
        }
        if self.loose && !other.loose {
            wider.push("strict mode off".to_owned());
        }
        if wider.is_empty() {
            Ok(())
        } else {
            Err(wider.join(", "))
        }
    }
}

/// What a computer's network may let through, from what the owner approved.
fn reach_of(
    approved: &Approved,
    grants: &BTreeSet<&'static str>,
    channel: Option<u16>,
    ingress_forced: bool,
) -> Reach {
    let mut reach = Reach {
        ingress: ingress_forced || !approved.incoming_denied,
        device_ports: channel.into_iter().collect(),
        ..Reach::default()
    };
    match approved.mode {
        Mode::Open => {
            reach.public = true;
            reach.dns = true;
        }
        Mode::Closed => {}
        Mode::List => {
            reach.dns = true;
            reach.loose = true;
            let preset = approved.tools.map(preset_hosts).unwrap_or_default();
            for host in approved.hosts.iter().chain(&preset).chain(grants) {
                reach.domains.insert(((*host).to_owned(), 443));
            }
        }
    }
    reach
}

/// The judge's view of a policy: no grants, channel, ingress or strict.
fn policy_reach(approved: &Approved) -> Reach {
    let mut reach = reach_of(approved, &BTreeSet::new(), None, false);
    reach.ingress = false;
    reach.loose = false;
    reach
}

/// Reads a runtime network policy (`config.network.policy`) and its strict setting the way the
/// runtime applies them.
fn reach_of_runtime(policy: &Value, strict: bool) -> Result<Reach, String> {
    let mut reach = Reach {
        loose: !strict,
        ..Reach::default()
    };
    match policy["default_egress"].as_str() {
        Some("deny") => {}
        Some("allow") => reach.public = true,
        other => return Err(format!("unknown default egress {other:?}")),
    }
    match policy["default_ingress"].as_str() {
        Some("deny") => {}
        Some("allow") => reach.ingress = true,
        other => return Err(format!("unknown default ingress {other:?}")),
    }
    for rule in policy["rules"].as_array().ok_or("no rules")? {
        if rule["action"] != "allow" {
            return Err(format!("unexpected rule {rule}"));
        }
        if rule["direction"] != "egress" {
            reach.ingress = true;
            continue;
        }
        let ports: Vec<(u64, u64)> = rule["ports"]
            .as_array()
            .ok_or("no ports")?
            .iter()
            .map(|port| {
                (
                    port["start"].as_u64().unwrap(),
                    port["end"].as_u64().unwrap(),
                )
            })
            .collect();
        let destination = &rule["destination"];
        if destination["group"] == "public" {
            reach.public = true;
        } else if destination["group"] == "host" {
            for (start, end) in ports {
                for port in start..=end {
                    if port == 53 {
                        reach.dns = true;
                    } else {
                        reach.device_ports.insert(port as u16);
                    }
                }
            }
        } else if let Some(domain) = destination["domain"].as_str() {
            for (start, end) in ports {
                for port in start..=end {
                    reach.domains.insert((domain.to_owned(), port as u16));
                }
            }
        } else if let Some(base) = destination["domain_suffix"].as_str() {
            for (start, end) in ports {
                for port in start..=end {
                    reach.domains.insert((format!("*.{base}"), port as u16));
                }
            }
        } else {
            return Err(format!("unknown destination {destination}"));
        }
    }
    Ok(reach)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Linux,
    Macos,
    /// The computer a rebuild can be undone to.
    Undo,
}

#[derive(Clone, Debug, PartialEq)]
struct Computer {
    id: String,
    name: String,
    kind: Kind,
    approved: Approved,
    /// Whether the store has an entry; without one the runtime's recorded rules are read.
    entry: bool,
    /// Whether the computer's record says it has had a saved policy: marked with no entry, its
    /// policy is missing and it is treated as unreadable.
    marked: bool,
    /// The rules the runtime holds for it (Linux): what a start without an entry reads.
    recorded: Value,
    channel: bool,
    /// The computer whose agent made it.
    maker: Option<String>,
    parent: Option<String>,
    /// For an undo computer: the policies it may hold (see [`Rebuild::allowed`]).
    undo_allowed: Vec<Approved>,
}

impl Computer {
    /// Its policy was saved and is gone: it is read like an unreadable store.
    fn missing(&self) -> bool {
        self.marked && !self.entry
    }

    /// Whether it refuses incoming connections, as a policy set or handed on keeps: the saved
    /// refusal, else what its recorded rules mean.
    fn refusal(&self) -> bool {
        if self.entry {
            self.approved.incoming_denied
        } else {
            self.recorded == deny_all()
        }
    }

    /// What it runs with by its recorded rules alone, as a computer from before policies.
    fn legacy(&self) -> Approved {
        if self.recorded == deny_all() {
            Approved::closed_without_incoming()
        } else {
            Spec {
                mode: Mode::Open,
                hosts: 0,
                tools: false,
            }
            .approved(1, None, false)
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Waiting {
    Set {
        target: String,
        spec: Spec,
    },
    Fork {
        caller: String,
        source: String,
        policy: Value,
    },
}

#[derive(Clone, Debug, PartialEq)]
struct Rebuild {
    computer: String,
    plan: RebuildPlan,
    /// Every policy the computer was approved with since the build was made and until the swap
    /// was published: the undo computer may hold any one of them.
    allowed: Vec<Approved>,
    /// The rules the computer's sandbox held when the build was made.
    recorded: Value,
    published: bool,
    /// The policy file was removed while the build ran: the build's and the undo computer's
    /// policies are missing.
    lost: bool,
}

#[derive(Clone, Debug)]
struct Model {
    computers: Vec<Computer>,
    version: u32,
    grants: BTreeSet<&'static str>,
    damaged: bool,
    waiting: Vec<Waiting>,
    rebuild: Option<Rebuild>,
    next: u32,
}

#[derive(Clone, Debug)]
enum Root {
    Entry(Spec),
    /// An entry saved by a Silo from before computers were marked as having one.
    UnmarkedEntry(Spec),
    LegacyOpen,
    LegacyImport,
}

#[derive(Clone, Debug)]
enum Op {
    OwnerSet {
        c: u8,
        spec: Spec,
        accept: bool,
    },
    AgentSet {
        caller: u8,
        target: u8,
        spec: Spec,
    },
    OwnerAllow {
        index: u8,
    },
    OwnerFork {
        source: u8,
        spec: Option<Spec>,
    },
    AgentFork {
        caller: u8,
        source: u8,
        spec: Option<Spec>,
    },
    ExportCopy {
        source: u8,
    },
    Import,
    ImportOver {
        c: u8,
    },
    Delete {
        c: u8,
    },
    ToolsOn {
        c: u8,
    },
    ToolsOff {
        c: u8,
    },
    Grant {
        host: u8,
    },
    PresetUpdate,
    Restart,
    Damage {
        kind: u8,
    },
    Repair,
    RemoveFile,
    RebuildBegin {
        c: u8,
    },
    RebuildSwap {
        fail_after: Option<u8>,
        readable: bool,
        publish_fails: bool,
    },
    RebuildSettle {
        readable: bool,
    },
    RebuildRollback,
    MacImport {
        interrupt: u8,
    },
}

#[derive(Clone, Debug)]
enum Transition {
    Init(Root),
    Op(Op),
}

fn pick<'a>(
    computers: &'a [Computer],
    selector: u8,
    filter: impl Fn(&Computer) -> bool,
) -> Option<&'a Computer> {
    let candidates: Vec<&Computer> = computers.iter().filter(|c| filter(c)).collect();
    if candidates.is_empty() {
        None
    } else {
        Some(candidates[selector as usize % candidates.len()])
    }
}

fn not_undo(computer: &Computer) -> bool {
    computer.kind != Kind::Undo
}

fn linux(computer: &Computer) -> bool {
    computer.kind == Kind::Linux
}

impl Model {
    fn get(&self, id: &str) -> Option<&Computer> {
        self.computers.iter().find(|c| c.id == id)
    }

    fn get_mut(&mut self, id: &str) -> Option<&mut Computer> {
        self.computers.iter_mut().find(|c| c.id == id)
    }

    /// The caller and every computer it made, directly or through one it made.
    fn tree(&self, caller: &str) -> BTreeSet<String> {
        let mut tree = BTreeSet::from([caller.to_owned()]);
        loop {
            let more: Vec<String> = self
                .computers
                .iter()
                .filter(|c| {
                    c.maker.as_ref().is_some_and(|m| tree.contains(m)) && !tree.contains(&c.id)
                })
                .map(|c| c.id.clone())
                .collect();
            if more.is_empty() {
                return tree;
            }
            tree.extend(more);
        }
    }

    fn fresh(&mut self, prefix: &str) -> (String, String) {
        self.next += 1;
        (
            format!("00000000-0000-4000-8000-{:012}", 100 + self.next),
            format!("{prefix}{}", self.next),
        )
    }

    /// Remembers a policy the owner approved for the computer a rebuild runs on.
    fn note_rebuild(&mut self, id: &str) {
        let approved = self.get(id).map(|c| c.approved.clone());
        if let (Some(rebuild), Some(approved)) = (self.rebuild.as_mut(), approved) {
            if rebuild.computer == id && !rebuild.published {
                rebuild.allowed.push(approved);
            }
        }
    }

    /// What the agent rules decide for `caller` setting `spec` on `target`, by this model.
    fn agent_set_decision(&self, caller: &str, target: &str, spec: &Spec) -> Expect {
        if !self.tree(caller).contains(target) {
            return Expect::Refuse;
        }
        if self.damaged || self.get(target).unwrap().missing() {
            return Expect::Ask;
        }
        if target != caller && self.get(caller).unwrap().missing() {
            return Expect::Ask;
        }
        let asked = policy_reach(&spec.asked(self.version));
        let current = policy_reach(&self.get(target).unwrap().approved);
        let own = policy_reach(&self.get(caller).unwrap().approved);
        if asked.within(&current).is_ok() && (target == caller || asked.within(&own).is_ok()) {
            Expect::Allow
        } else {
            Expect::Ask
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Expect {
    Allow,
    Ask,
    Refuse,
}

fn spec_strategy() -> impl Strategy<Value = Spec> {
    (
        prop_oneof![Just(Mode::Open), Just(Mode::List), Just(Mode::Closed)],
        0u8..16,
        any::<bool>(),
    )
        .prop_map(|(mode, hosts, tools)| Spec {
            mode,
            hosts: if mode == Mode::List { hosts } else { 0 },
            tools: mode == Mode::List && tools,
        })
}

/// Which variant of the test runs.
const DEFAULT: u8 = 0;
/// Also removes the policy file among the operations.
const MISSING_FILE: u8 = 1;
/// Expects incoming connections to stay refused on an agent's change too.
const STRICT_INGRESS: u8 = 2;

/// The reference state machine for one variant.
struct NetworkModel<const VARIANT: u8>;

impl<const VARIANT: u8> ReferenceStateMachine for NetworkModel<VARIANT> {
    type State = Model;
    type Transition = Transition;

    fn init_state() -> BoxedStrategy<Model> {
        Just(Model {
            computers: Vec::new(),
            version: 1,
            grants: BTreeSet::new(),
            damaged: false,
            waiting: Vec::new(),
            rebuild: None,
            next: 0,
        })
        .boxed()
    }

    fn transitions(state: &Model) -> BoxedStrategy<Transition> {
        if state.computers.is_empty() && state.next == 0 {
            if VARIANT == STRICT_INGRESS {
                return Just(Transition::Init(Root::LegacyImport)).boxed();
            }
            return prop_oneof![
                spec_strategy().prop_map(Root::Entry),
                spec_strategy().prop_map(Root::UnmarkedEntry),
                Just(Root::LegacyOpen),
                Just(Root::LegacyImport),
            ]
            .prop_map(Transition::Init)
            .boxed();
        }
        let sel = any::<u8>;
        let ops = prop_oneof![
            6 => (sel(), spec_strategy(), any::<bool>()).prop_map(|(c, spec, accept)| Op::OwnerSet { c, spec, accept }),
            6 => (sel(), sel(), spec_strategy()).prop_map(|(caller, target, spec)| Op::AgentSet { caller, target, spec }),
            3 => sel().prop_map(|index| Op::OwnerAllow { index }),
            3 => (sel(), proptest::option::of(spec_strategy())).prop_map(|(source, spec)| Op::OwnerFork { source, spec }),
            5 => (sel(), sel(), proptest::option::of(spec_strategy())).prop_map(|(caller, source, spec)| Op::AgentFork { caller, source, spec }),
            2 => sel().prop_map(|source| Op::ExportCopy { source }),
            2 => Just(Op::Import),
            1 => sel().prop_map(|c| Op::ImportOver { c }),
            2 => sel().prop_map(|c| Op::Delete { c }),
            2 => sel().prop_map(|c| Op::ToolsOn { c }),
            1 => sel().prop_map(|c| Op::ToolsOff { c }),
            1 => (0u8..2).prop_map(|host| Op::Grant { host }),
            1 => Just(Op::PresetUpdate),
            2 => Just(Op::Restart),
            1 => (0u8..3).prop_map(|kind| Op::Damage { kind }),
            2 => Just(Op::Repair),
            3 => sel().prop_map(|c| Op::RebuildBegin { c }),
            4 => (proptest::option::of(0u8..5), any::<bool>(), proptest::bool::weighted(0.2))
                .prop_map(|(fail_after, readable, publish_fails)| Op::RebuildSwap { fail_after, readable, publish_fails }),
            1 => any::<bool>().prop_map(|readable| Op::RebuildSettle { readable }),
            1 => Just(Op::RebuildRollback),
            2 => (0u8..4).prop_map(|interrupt| Op::MacImport { interrupt }),
        ];
        if VARIANT == MISSING_FILE {
            prop_oneof![30 => ops, 1 => Just(Op::RemoveFile)]
                .prop_map(Transition::Op)
                .boxed()
        } else {
            ops.prop_map(Transition::Op).boxed()
        }
    }

    fn preconditions(state: &Model, transition: &Transition) -> bool {
        let first = state.computers.is_empty() && state.next == 0;
        matches!(transition, Transition::Init(_)) == first
    }

    fn apply(mut state: Model, transition: &Transition) -> Model {
        match transition {
            Transition::Init(root) => {
                let (approved, entry, marked, recorded) = match root {
                    Root::Entry(spec) => (spec.approved(1, None, false), true, true, open_rules()),
                    Root::UnmarkedEntry(spec) => {
                        (spec.approved(1, None, false), true, false, open_rules())
                    }
                    Root::LegacyOpen => (
                        Spec {
                            mode: Mode::Open,
                            hosts: 0,
                            tools: false,
                        }
                        .approved(1, None, false),
                        false,
                        false,
                        open_rules(),
                    ),
                    Root::LegacyImport => (
                        Approved::closed_without_incoming(),
                        false,
                        false,
                        deny_all(),
                    ),
                };
                state.next = 1;
                state.computers.push(Computer {
                    id: ID.into(),
                    name: "dev".into(),
                    kind: Kind::Linux,
                    approved,
                    entry,
                    marked,
                    recorded,
                    channel: false,
                    maker: None,
                    parent: None,
                    undo_allowed: Vec::new(),
                });
            }
            Transition::Op(op) => {
                model_apply(&mut state, op);
            }
        }
        state
    }
}

/// What the engine must answer to an operation, by the model.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Want {
    /// What the agent rules decide.
    decision: Option<Expect>,
    /// Whether the operation succeeds.
    ok: Option<bool>,
}

fn ok_if(ok: bool) -> Want {
    Want {
        decision: None,
        ok: Some(ok),
    }
}

/// The model's effect of `op`. Inapplicable operations change nothing, here and in the engine.
fn model_apply(state: &mut Model, op: &Op) -> Want {
    let version = state.version;
    match op {
        Op::OwnerSet { c, spec, accept } => {
            let Some(target) = pick(&state.computers, *c, not_undo).map(|c| c.id.clone()) else {
                return Want::default();
            };
            if state.damaged {
                return ok_if(false);
            }
            // The owner's policy replaces a missing one; the refusal of incoming connections
            // stays as it was.
            let computer = state.get_mut(&target).unwrap();
            let before = computer.entry.then(|| computer.approved.clone());
            let incoming = computer.refusal();
            computer.approved = if *accept {
                spec.approved(version, None, incoming)
            } else {
                spec.approved(version, before.as_ref(), incoming)
            };
            computer.entry = true;
            computer.marked = true;
            state.note_rebuild(&target);
            ok_if(true)
        }
        Op::AgentSet {
            caller,
            target,
            spec,
        } => {
            let Some(caller) = pick(&state.computers, *caller, not_undo).map(|c| c.id.clone())
            else {
                return Want::default();
            };
            let Some(target) = pick(&state.computers, *target, not_undo).map(|c| c.id.clone())
            else {
                return Want::default();
            };
            let decision = state.agent_set_decision(&caller, &target, spec);
            match decision {
                Expect::Refuse => {}
                Expect::Ask => state.waiting.push(Waiting::Set {
                    target,
                    spec: *spec,
                }),
                Expect::Allow => {
                    let computer = state.get_mut(&target).unwrap();
                    let before = computer.approved.clone();
                    // A narrowing keeps the refused incoming connections: the saved ones, or
                    // those of the rules of a computer with none saved.
                    let incoming = computer.refusal();
                    computer.approved = spec.approved(version, Some(&before), incoming);
                    computer.entry = true;
                    computer.marked = true;
                    state.note_rebuild(&target);
                }
            }
            Want {
                decision: Some(decision),
                ok: (decision == Expect::Allow).then_some(true),
            }
        }
        Op::OwnerAllow { index } => {
            if state.waiting.is_empty() {
                return Want::default();
            }
            let waiting = state.waiting.remove(*index as usize % state.waiting.len());
            match waiting {
                Waiting::Set { target, spec } => {
                    if state.get(&target).is_none() {
                        return Want::default();
                    }
                    if state.damaged {
                        return ok_if(false);
                    }
                    let computer = state.get_mut(&target).unwrap();
                    let before = computer.entry.then(|| computer.approved.clone());
                    let incoming = computer.refusal();
                    computer.approved = spec.approved(version, before.as_ref(), incoming);
                    computer.entry = true;
                    computer.marked = true;
                    state.note_rebuild(&target);
                    ok_if(true)
                }
                Waiting::Fork {
                    caller,
                    source,
                    policy,
                } => {
                    if state.computers.len() >= MAX_COMPUTERS
                        || state.get(&source).is_none()
                        || state.get(&caller).is_none()
                    {
                        return Want::default();
                    }
                    if state.damaged || state.get(&source).unwrap().missing() {
                        return ok_if(false);
                    }
                    model_fork(state, &source, Some(spec_of(&policy)), Some(caller), false);
                    ok_if(true)
                }
            }
        }
        Op::OwnerFork { source, spec } => {
            let Some(source) = pick(&state.computers, *source, linux).map(|c| c.id.clone()) else {
                return Want::default();
            };
            if state.computers.len() >= MAX_COMPUTERS {
                return Want::default();
            }
            if state.damaged || state.get(&source).unwrap().missing() {
                return ok_if(false);
            }
            model_fork(state, &source, *spec, None, false);
            ok_if(true)
        }
        Op::AgentFork {
            caller,
            source,
            spec,
        } => {
            let Some(caller) = pick(&state.computers, *caller, not_undo).map(|c| c.id.clone())
            else {
                return Want::default();
            };
            let Some(source) = pick(&state.computers, *source, linux).map(|c| c.id.clone()) else {
                return Want::default();
            };
            if state.computers.len() >= MAX_COMPUTERS {
                return Want::default();
            }
            if !state.tree(&caller).contains(&source) {
                return Want {
                    decision: Some(Expect::Refuse),
                    ok: None,
                };
            }
            let ask = Want {
                decision: Some(Expect::Ask),
                ok: None,
            };
            if state.damaged || state.get(&caller).unwrap().missing() {
                // The caller's policy cannot be read: the fork asks, for the policy asked for
                // or else a closed one.
                let policy = match spec {
                    Some(spec) => spec.policy(),
                    None => Spec {
                        mode: Mode::Closed,
                        hosts: 0,
                        tools: false,
                    }
                    .policy(),
                };
                state.waiting.push(Waiting::Fork {
                    caller,
                    source,
                    policy: json!(policy),
                });
                return ask;
            }
            let own = state.get(&caller).unwrap().approved.clone();
            // A source whose policy is missing is judged as one with none: the policy is
            // approved now, and the fork itself fails.
            let lost = state.get(&source).unwrap().missing();
            let from = (!lost).then(|| state.get(&source).unwrap().approved.clone());
            let held = |spec: Spec| spec.approved(version, from.as_ref(), false);
            let chosen = match (spec, &from) {
                (Some(spec), _) => held(*spec),
                (None, Some(from)) => {
                    let inherited = held(spec_of_approved(from));
                    if policy_reach(&inherited).within(&policy_reach(&own)).is_ok() {
                        inherited
                    } else {
                        held(spec_of_approved(&own))
                    }
                }
                (None, None) => held(spec_of_approved(&own)),
            };
            let chosen_spec = spec_of_approved(&chosen);
            if policy_reach(&chosen).within(&policy_reach(&own)).is_ok() {
                if !lost {
                    model_fork(state, &source, Some(chosen_spec), Some(caller), false);
                }
                Want {
                    decision: Some(Expect::Allow),
                    ok: Some(!lost),
                }
            } else {
                state.waiting.push(Waiting::Fork {
                    caller,
                    source,
                    policy: json!(chosen_spec.policy()),
                });
                ask
            }
        }
        Op::ExportCopy { source } => {
            let Some(source) = pick(&state.computers, *source, linux).map(|c| c.id.clone()) else {
                return Want::default();
            };
            if state.computers.len() >= MAX_COMPUTERS {
                return Want::default();
            }
            if state.damaged || state.get(&source).unwrap().missing() {
                return ok_if(false);
            }
            model_fork(state, &source, None, None, true);
            ok_if(true)
        }
        Op::Import => {
            if state.computers.len() >= MAX_COMPUTERS {
                return Want::default();
            }
            let (id, name) = state.fresh("imp");
            if state.damaged {
                return ok_if(false);
            }
            state.computers.push(Computer {
                id,
                name,
                kind: Kind::Linux,
                approved: Approved::closed_without_incoming(),
                entry: true,
                marked: true,
                recorded: deny_all(),
                channel: false,
                maker: None,
                parent: None,
                undo_allowed: Vec::new(),
            });
            ok_if(true)
        }
        Op::ImportOver { c } => {
            let Some(target) = pick(&state.computers, *c, linux).map(|c| c.id.clone()) else {
                return Want::default();
            };
            if state.rebuild.as_ref().is_some_and(|r| r.computer == target) {
                return Want::default();
            }
            if state.damaged {
                return ok_if(false);
            }
            let computer = state.get_mut(&target).unwrap();
            computer.approved = Approved::closed_without_incoming();
            computer.entry = true;
            computer.marked = true;
            computer.recorded = deny_all();
            computer.channel = false;
            ok_if(true)
        }
        Op::Delete { c } => {
            let Some(target) = pick(&state.computers, *c, |_| true).map(|c| c.id.clone()) else {
                return Want::default();
            };
            if state.rebuild.as_ref().is_some_and(|r| r.computer == target) {
                return Want::default();
            }
            delete_from_model(state, &target);
            Want::default()
        }
        Op::ToolsOn { c } | Op::ToolsOff { c } => {
            let Some(target) = pick(&state.computers, *c, not_undo).map(|c| c.id.clone()) else {
                return Want::default();
            };
            state.get_mut(&target).unwrap().channel = matches!(op, Op::ToolsOn { .. });
            ok_if(true)
        }
        Op::Grant { host } => {
            state
                .grants
                .insert(GRANT_HOSTS[*host as usize % GRANT_HOSTS.len()]);
            Want::default()
        }
        Op::PresetUpdate => {
            state.version = 2;
            Want::default()
        }
        Op::Restart => {
            // The engine marks every computer that has an entry.
            for computer in &mut state.computers {
                computer.marked |= computer.entry;
            }
            Want::default()
        }
        Op::Damage { .. } => {
            state.damaged = true;
            Want::default()
        }
        Op::Repair => {
            state.damaged = false;
            Want::default()
        }
        Op::RemoveFile => {
            if !state.damaged {
                // Every saved policy is gone. A marked computer's is missing: it does not start
                // until the owner sets one. An unmarked one (saved before marks, not started
                // since) is read as a computer from before policies, by its recorded rules.
                for computer in &mut state.computers {
                    if computer.entry && !computer.marked {
                        computer.approved = computer.legacy();
                    }
                    computer.entry = false;
                }
                if let Some(rebuild) = state.rebuild.as_mut() {
                    rebuild.lost = true;
                }
            }
            Want::default()
        }
        Op::RebuildBegin { c } => {
            if state.rebuild.is_some() {
                return Want::default();
            }
            let Some(computer) = pick(&state.computers, *c, linux).cloned() else {
                return Want::default();
            };
            let (new_id, new_name) = state.fresh("silo-rebuild-");
            let (undo_id, undo_name) = state.fresh("undo");
            if state.damaged || computer.missing() {
                return ok_if(false);
            }
            let plan = RebuildPlan {
                old_id: computer.id.clone(),
                old_name: computer.name.clone(),
                new_id,
                new_name,
                undo_id,
                undo_name,
                image: "image".into(),
                stage: "stage".into(),
            };
            state.rebuild = Some(Rebuild {
                computer: computer.id.clone(),
                plan,
                allowed: vec![computer.approved.clone()],
                recorded: computer.recorded.clone(),
                published: false,
                lost: false,
            });
            ok_if(true)
        }
        // Where a swap gets to is the engine's to say; the harness tells the model.
        Op::RebuildSwap { .. } | Op::RebuildSettle { .. } | Op::RebuildRollback => Want::default(),
        Op::MacImport { interrupt } => {
            if state.computers.len() >= MAX_COMPUTERS {
                return Want::default();
            }
            let (id, name) = state.fresh("mac");
            if state.damaged {
                return ok_if(false);
            }
            match interrupt {
                // Published, or published by a Silo that saved no policy and then recovered.
                0 | 3 => {
                    state.computers.push(Computer {
                        id,
                        name,
                        kind: Kind::Macos,
                        approved: Approved::closed_without_incoming(),
                        entry: true,
                        marked: true,
                        recorded: Value::Null,
                        channel: false,
                        maker: None,
                        parent: None,
                        undo_allowed: Vec::new(),
                    });
                    ok_if(true)
                }
                // The folder could not be moved.
                1 => ok_if(false),
                // Stopped before the folder moved, then recovered: no computer.
                _ => ok_if(true),
            }
        }
    }
}

fn delete_from_model(state: &mut Model, target: &str) {
    let maker = state.get(target).and_then(|c| c.maker.clone());
    state.computers.retain(|c| c.id != target);
    for computer in &mut state.computers {
        if computer.maker.as_deref() == Some(target) {
            computer.maker = maker.clone();
        }
    }
    state.waiting.retain(|waiting| match waiting {
        Waiting::Set { target: t, .. } => t != target,
        Waiting::Fork { caller, source, .. } => caller != target && source != target,
    });
}

fn spec_of_approved(approved: &Approved) -> Spec {
    Spec {
        mode: approved.mode,
        hosts: HOSTS
            .iter()
            .enumerate()
            .filter(|(_, host)| approved.hosts.contains(*host))
            .fold(0, |bits, (index, _)| bits | (1 << index)),
        tools: approved.tools.is_some(),
    }
}

fn spec_of(policy: &Value) -> Spec {
    let policy: NetworkPolicy = serde_json::from_value(policy.clone()).unwrap();
    Spec {
        mode: match policy.mode {
            NetworkPolicyMode::Open => Mode::Open,
            NetworkPolicyMode::Allowlist => Mode::List,
            NetworkPolicyMode::Closed => Mode::Closed,
        },
        hosts: HOSTS
            .iter()
            .enumerate()
            .filter(|(_, host)| policy.hosts.iter().any(|h| h == *host))
            .fold(0, |bits, (index, _)| bits | (1 << index)),
        tools: policy.presets.iter().any(|p| p == PRESET),
    }
}

/// A fork of `source` as the model expects it: the policy given (approved by whoever gave it,
/// with the source's preset versions), else the source's own; the copy an export makes is
/// closed with incoming connections refused.
fn model_fork(
    state: &mut Model,
    source: &str,
    spec: Option<Spec>,
    maker: Option<String>,
    closed_copy: bool,
) {
    let version = state.version;
    let from = state.get(source).unwrap().clone();
    let (approved, entry) = if closed_copy {
        (Approved::closed_without_incoming(), true)
    } else {
        match spec {
            // A fork given a policy refuses the incoming connections its source refuses.
            Some(spec) => (
                spec.approved(version, Some(&from.approved), from.refusal()),
                true,
            ),
            None => (from.approved.clone(), from.entry),
        }
    };
    let (id, name) = state.fresh("f");
    state.computers.push(Computer {
        id,
        name,
        kind: Kind::Linux,
        approved,
        entry,
        // Marked when it gets an entry.
        marked: entry,
        recorded: if closed_copy {
            deny_all()
        } else {
            from.recorded.clone()
        },
        channel: !closed_copy && from.channel,
        maker,
        parent: Some(from.id),
        undo_allowed: Vec::new(),
    });
}

// MARK: The engine under test

/// The runtime's sandboxes for the rebuild swap. Renames and relabels fail once `fail_after`
/// reaches zero.
#[derive(Default)]
struct Table {
    sandboxes: RefCell<Vec<(String, String)>>,
    fail_after: RefCell<Option<u8>>,
}

impl swap::Sandboxes for Table {
    fn owner(&self, name: &str) -> Result<Option<String>, String> {
        Ok(self
            .sandboxes
            .borrow()
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, id)| id.clone()))
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), String> {
        self.step()?;
        let mut all = self.sandboxes.borrow_mut();
        if all.iter().any(|(n, _)| n == to) {
            return Err(format!("{to} exists"));
        }
        all.iter_mut()
            .find(|(n, _)| n == from)
            .ok_or_else(|| format!("{from} is not a sandbox"))?
            .0 = to.into();
        Ok(())
    }

    fn relabel(&self, name: &str, id: &str) -> Result<(), String> {
        self.step()?;
        self.sandboxes
            .borrow_mut()
            .iter_mut()
            .find(|(n, _)| n == name)
            .ok_or_else(|| format!("{name} is not a sandbox"))?
            .1 = id.into();
        Ok(())
    }
}

impl Table {
    fn step(&self) -> Result<(), String> {
        let mut left = self.fail_after.borrow_mut();
        match left.as_mut() {
            Some(0) => Err("injected failure".into()),
            Some(count) => {
                *count -= 1;
                Ok(())
            }
            None => Ok(()),
        }
    }

    fn has(&self, name: &str) -> bool {
        self.sandboxes.borrow().iter().any(|(n, _)| n == name)
    }
}

/// The engine: its files in a temporary folder, and the model of what the owner approved,
/// advanced in step with it. (The reference state machine generates the operations; the model
/// kept here is the oracle, since some outcomes, such as how far a swap got, are the engine's
/// to say.)
struct Engine {
    model: Model,
    /// The id the engine gave each fork, by the id the model gave it.
    ids: BTreeMap<String, String>,
    paths: RuntimePaths,
    data: PathBuf,
    policy_file: PathBuf,
    tokens: PathBuf,
    /// The bytes the policy file had before it was damaged; `None` when it did not exist.
    saved: Option<Option<Vec<u8>>>,
    table: Option<Table>,
    published: bool,
    /// The last published rebuild, for settling its undo computer: the plan and the rules.
    settled: Option<(RebuildPlan, Value)>,
    steps: usize,
    _directory: tempfile::TempDir,
    _guard: crate::test_support::GlobalStateGuard,
}

impl Drop for Engine {
    fn drop(&mut self) {
        Engine::uninstall();
        if std::env::var_os("SILO_MBT_STATS").is_some() {
            STATS.with(|stats| eprintln!("network_mbt: {:?}", stats.borrow()));
        }
    }
}

thread_local! {
    /// How often each outcome happened, printed with `SILO_MBT_STATS=1`.
    static STATS: RefCell<BTreeMap<String, usize>> = const { RefCell::new(BTreeMap::new()) };
}

fn note(what: impl Into<String>) {
    STATS.with(|stats| *stats.borrow_mut().entry(what.into()).or_default() += 1);
}

fn configuration(id: &str, name: &str) -> ComputerConfiguration {
    ComputerConfiguration {
        id: id.into(),
        name: name.into(),
        ..computer_configuration()
    }
}

/// The agent rules' view of the engine: the tree and names from the model (in the engine's
/// ids), the network policy as `network.policy.get` reads it from the engine.
struct World<'a> {
    model: Model,
    paths: &'a RuntimePaths,
}

impl World<'_> {
    fn target(&self, computer: &str) -> Option<service::Target<'_>> {
        let found = self.model.get(computer)?;
        Some(match found.kind {
            Kind::Linux => service::Target::Linux(self.paths),
            Kind::Macos => service::Target::Macos {
                name: found.name.clone(),
                running: false,
            },
            Kind::Undo => return None,
        })
    }
}

impl policy::World for World<'_> {
    fn made(&self, creator: &str) -> std::collections::HashSet<String> {
        let mut tree = self.model.tree(creator);
        tree.remove(creator);
        tree.into_iter().collect()
    }
    fn parent(&self, computer: &str) -> Option<String> {
        self.model.get(computer)?.parent.clone()
    }
    fn name(&self, computer: &str) -> Option<String> {
        Some(self.model.get(computer)?.name.clone())
    }
    fn owns_job(&self, _: &str, _: &str) -> bool {
        false
    }
    fn request_fork(&self, _: &str) -> Option<String> {
        None
    }
    fn request_target(&self, _: &str) -> Option<String> {
        None
    }
    fn access(&self, _: &str) -> Access {
        Access::default()
    }
    fn github_policies(&self) -> Option<Vec<Value>> {
        Some(Vec::new())
    }
    fn new_id(&self) -> String {
        "agent-new-id".into()
    }
    fn resources(&self, _: &str) -> Option<Resources> {
        Resources::of_linux(&json!({"cpus": 1, "maxCPUs": 1, "memoryGiB": 1,
            "maxMemoryGiB": 1, "workspaceStorageGiB": 1, "runtimeStorageGiB": 1}))
    }
    fn ssh_settings(&self, _: &str) -> Option<(u16, String)> {
        None
    }
    fn network_held(&self, computer: &str) -> Option<Held> {
        service::judged(computer, self.target(computer)?)
    }
}

fn decision_kind(decision: &Decision) -> Expect {
    match decision {
        Decision::Allow(_) => Expect::Allow,
        Decision::Ask { .. } => Expect::Ask,
        Decision::Refuse { .. } => Expect::Refuse,
    }
}

fn decision_params(decision: &Decision) -> Option<Value> {
    match decision {
        Decision::Allow(allowed) => allowed.params.clone(),
        Decision::Ask { params, .. } => params.clone(),
        Decision::Refuse { .. } => None,
    }
}

const IMPORT_GROUP: &str = "silo-import-0123456789abcdef0123456789abcdef";
const IMPORT_MEMBER: &str = "silo-backup-1-2-3";

impl Engine {
    fn install(&self) {
        policy_store::use_test_path(Some(self.policy_file.clone()));
        np::marks::use_test_roots(Some(np::marks::Roots {
            app_data: self.data.clone(),
            linux_records: Some(self.linux_records()),
        }));
        crate::guest_client::tokens::use_test_directory(Some(self.tokens.clone()));
        np::use_test_presets(Some(presets_at(self.model.version)));
        np::use_test_grants(Some(Grants {
            github: false,
            secrets: vec![(
                "SECRET".into(),
                self.model
                    .grants
                    .iter()
                    .map(|host| (*host).to_owned())
                    .collect(),
            )],
            shared_hosting: BTreeSet::new(),
        }));
    }

    fn uninstall() {
        policy_store::use_test_path(None);
        np::marks::use_test_roots(None);
        crate::guest_client::tokens::use_test_directory(None);
        np::use_test_presets(None);
        np::use_test_grants(None);
    }

    /// Where Linux computers' checkpoint records, and so their marks, are kept.
    fn linux_records(&self) -> PathBuf {
        self.paths.metadata.with_file_name("checkpoints")
    }

    /// The engine's id of the computer the model calls `id`.
    fn eid(&self, id: &str) -> String {
        self.ids.get(id).cloned().unwrap_or_else(|| id.to_owned())
    }

    /// `model` in the engine's ids.
    fn in_engine_ids(&self, model: &Model) -> Model {
        let mut view = model.clone();
        for computer in &mut view.computers {
            computer.id = self.eid(&computer.id);
            computer.maker = computer.maker.as_deref().map(|id| self.eid(id));
            computer.parent = computer.parent.as_deref().map(|id| self.eid(id));
        }
        view
    }

    fn world<'a>(&'a self, before: &Model) -> World<'a> {
        World {
            model: self.in_engine_ids(before),
            paths: &self.paths,
        }
    }

    fn add_to_inventory(&self, id: &str, name: &str) {
        let mut metadata = read_metadata(&self.paths.metadata).unwrap();
        metadata.computers.retain(|c| c.id() != id);
        metadata.computers.push(configuration(id, name));
        write_metadata(&self.paths.metadata, &metadata).unwrap();
    }

    fn remove_from_inventory(&self, id: &str) {
        let mut metadata = read_metadata(&self.paths.metadata).unwrap();
        metadata.computers.retain(|c| c.id() != id);
        write_metadata(&self.paths.metadata, &metadata).unwrap();
    }

    /// Forks `source` as `name` through the runtime's fork commit; the new computer's id.
    fn fork(&self, source: &Computer, name: &str, options: ForkOptions) -> Result<String, String> {
        let source_id = self.eid(&source.id);
        let record = load(&self.paths, &source_id).map_err(|e| e.to_string())?;
        // A pending source that the runtime does not list forks with the rules its record
        // holds, as `fork_source_policy` reads them.
        let fork = ForkSource {
            source_id,
            snapshot_group: GROUP.into(),
            member: MEMBER.into(),
            scope: "full".into(),
            desired_policy: record
                .desired_network_policy
                .expect("every computer here records its rules"),
            options,
        };
        fork_commit(
            &journal_runner("Running", ""),
            &self.paths,
            &FakeAssignments::new(&[]),
            &fork,
            name,
        )
        .map_err(|e| e.to_string())?;
        let metadata = read_metadata(&self.paths.metadata).unwrap();
        Ok(metadata
            .computers
            .iter()
            .find(|c| c.name() == name)
            .expect("the fork is in the inventory")
            .id()
            .to_owned())
    }

    fn target<'a>(&'a self, computer: &Computer) -> service::Target<'a> {
        match computer.kind {
            Kind::Macos => service::Target::Macos {
                name: computer.name.clone(),
                running: false,
            },
            _ => service::Target::Linux(&self.paths),
        }
    }

    fn damaged_bytes(kind: u8) -> &'static [u8] {
        match kind {
            0 => b"{ not json",
            1 => br#"{"version": 2, "computers": {}}"#,
            _ => br#"{"version": 1, "computers": {"x": {"mode": "from-the-future"}}}"#,
        }
    }
}

fn new_computer(before: &Model, after: &Model) -> Option<Computer> {
    (after.computers.len() > before.computers.len())
        .then(|| after.computers.last().unwrap().clone())
}

/// Runs `op` on the engine and checks its answer against the model.
fn engine_apply(engine: &mut Engine, op: &Op) {
    let before = engine.model.clone();
    let want = model_apply(&mut engine.model, op);
    let after = engine.model.clone();
    let mut decision: Option<Expect> = None;
    let mut ok: Option<bool> = None;
    match op {
        Op::OwnerSet { c, spec, accept } => {
            if let Some(target) = pick(&before.computers, *c, not_undo) {
                let result = service::set(
                    &engine.eid(&target.id),
                    &spec.policy(),
                    None,
                    accept.then_some(true),
                    engine.target(target),
                );
                ok = Some(result.is_ok());
            }
        }
        Op::AgentSet {
            caller,
            target,
            spec,
        } => {
            let (Some(caller), Some(target)) = (
                pick(&before.computers, *caller, not_undo),
                pick(&before.computers, *target, not_undo),
            ) else {
                return finish(engine, op, want, None, None);
            };
            let world = engine.world(&before);
            let made = policy::decide(
                &Caller::Computer {
                    id: engine.eid(&caller.id),
                },
                MethodName::NetworkPolicySet,
                &json!({"computerId": engine.eid(&target.id), "policy": spec.policy()}),
                &world,
            );
            decision = Some(decision_kind(&made));
            if let Decision::Allow(_) = made {
                let params = decision_params(&made).expect("a network change is rewritten");
                let policy: NetworkPolicy =
                    serde_json::from_value(params["policy"].clone()).unwrap();
                let expected: Option<NetworkPolicy> = params
                    .get("expected")
                    .map(|value| serde_json::from_value(value.clone()).unwrap());
                let result = service::set(
                    &engine.eid(&target.id),
                    &policy,
                    expected.as_ref(),
                    None,
                    engine.target(target),
                );
                ok = Some(result.is_ok());
            }
            if let Decision::Ask { kind, .. } = &made {
                assert_eq!(*kind, ApprovalKind::Network);
            }
        }
        Op::OwnerAllow { index } => {
            if !before.waiting.is_empty() {
                match &before.waiting[*index as usize % before.waiting.len()] {
                    Waiting::Set { target, spec } => {
                        if let Some(target) = before.get(target) {
                            let result = service::set(
                                &engine.eid(&target.id),
                                &spec.policy(),
                                None,
                                None,
                                engine.target(target),
                            );
                            ok = Some(result.is_ok());
                        }
                    }
                    Waiting::Fork {
                        caller,
                        source,
                        policy,
                    } => {
                        if before.computers.len() < MAX_COMPUTERS && before.get(caller).is_some() {
                            if let Some(source) = before.get(source) {
                                let policy: NetworkPolicy =
                                    serde_json::from_value(policy.clone()).unwrap();
                                ok = Some(fork_into(
                                    engine,
                                    &before,
                                    &after,
                                    source,
                                    ForkOptions {
                                        network_policy: Some(policy),
                                        ..ForkOptions::default()
                                    },
                                ));
                            }
                        }
                    }
                }
            }
        }
        Op::OwnerFork { source, spec } => {
            if let Some(source) = pick(&before.computers, *source, linux) {
                if before.computers.len() < MAX_COMPUTERS {
                    ok = Some(fork_into(
                        engine,
                        &before,
                        &after,
                        source,
                        ForkOptions {
                            network_policy: spec.map(|spec| spec.policy()),
                            ..ForkOptions::default()
                        },
                    ));
                }
            }
        }
        Op::AgentFork {
            caller,
            source,
            spec,
        } => {
            let (Some(caller), Some(source)) = (
                pick(&before.computers, *caller, not_undo),
                pick(&before.computers, *source, linux),
            ) else {
                return finish(engine, op, want, None, None);
            };
            if before.computers.len() < MAX_COMPUTERS {
                let name = format!("f{}", before.next + 1);
                let mut params = json!({"computerId": engine.eid(&source.id), "newName": name});
                if let Some(spec) = spec {
                    params["networkPolicy"] = json!(spec.policy());
                }
                let world = engine.world(&before);
                let made = policy::decide(
                    &Caller::Computer {
                        id: engine.eid(&caller.id),
                    },
                    MethodName::CheckpointsFork,
                    &params,
                    &world,
                );
                decision = Some(decision_kind(&made));
                let rewritten = decision_params(&made);
                if let (Some(rewritten), Some(Waiting::Fork { policy, .. })) = (
                    &rewritten,
                    after
                        .waiting
                        .last()
                        .filter(|_| after.waiting.len() > before.waiting.len()),
                ) {
                    let asked: NetworkPolicy =
                        serde_json::from_value(rewritten["networkPolicy"].clone()).unwrap();
                    let modelled: NetworkPolicy = serde_json::from_value(policy.clone()).unwrap();
                    assert_eq!(
                        asked, modelled,
                        "{op:?}: the request carries another policy than the model's; decided {made:?}; model before {before:?}"
                    );
                }
                if let (Decision::Allow(_), Some(rewritten)) = (&made, rewritten) {
                    let policy: Option<NetworkPolicy> = rewritten
                        .get("networkPolicy")
                        .map(|value| serde_json::from_value(value.clone()).unwrap());
                    ok = Some(fork_into(
                        engine,
                        &before,
                        &after,
                        source,
                        ForkOptions {
                            network_policy: policy,
                            scrub: vec!["/silo-endpoint".into()],
                            ..ForkOptions::default()
                        },
                    ));
                }
            }
        }
        Op::ExportCopy { source } => {
            if let Some(source) = pick(&before.computers, *source, linux) {
                if before.computers.len() < MAX_COMPUTERS {
                    ok = Some(fork_into(
                        engine,
                        &before,
                        &after,
                        source,
                        ForkOptions {
                            closed_network: true,
                            ..ForkOptions::default()
                        },
                    ));
                }
            }
        }
        Op::Import => {
            if before.computers.len() < MAX_COMPUTERS {
                let id = format!("00000000-0000-4000-8000-{:012}", 100 + after.next);
                let name = format!("imp{}", after.next);
                let result =
                    import_pending_restore(&engine.paths, &id, IMPORT_GROUP, IMPORT_MEMBER);
                if result.is_ok() {
                    engine.add_to_inventory(&id, &name);
                }
                ok = Some(result.is_ok());
            }
        }
        Op::ImportOver { c } => {
            if let Some(target) = pick(&before.computers, *c, linux) {
                if !before
                    .rebuild
                    .as_ref()
                    .is_some_and(|r| r.computer == target.id)
                {
                    let result = import_pending_restore(
                        &engine.paths,
                        &engine.eid(&target.id),
                        IMPORT_GROUP,
                        IMPORT_MEMBER,
                    );
                    ok = Some(result.is_ok());
                }
            }
        }
        Op::Delete { c } => {
            if let Some(target) = pick(&before.computers, *c, |_| true) {
                if !before
                    .rebuild
                    .as_ref()
                    .is_some_and(|r| r.computer == target.id)
                {
                    let id = engine.eid(&target.id);
                    np::computer_removed(&id);
                    crate::guest_client::computer_removed(&id);
                    if target.kind == Kind::Linux {
                        engine.remove_from_inventory(&id);
                    }
                }
            }
        }
        Op::ToolsOn { c } => {
            if let Some(target) = pick(&before.computers, *c, not_undo) {
                ok = Some(crate::guest_client::turn_on(&engine.eid(&target.id)).is_ok());
            }
        }
        Op::ToolsOff { c } => {
            if let Some(target) = pick(&before.computers, *c, not_undo) {
                ok =
                    Some(crate::guest_client::turn_off(&[engine.eid(&target.id).as_str()]).is_ok());
            }
        }
        Op::Grant { .. } | Op::PresetUpdate => {
            // The grants and presets are installed from the model below.
        }
        Op::Restart => {
            // The engine stops and starts again: nothing it held in memory is left, and every
            // store is read again from its files. At start it marks every computer with an
            // entry, when the policy file can be read.
            Engine::uninstall();
            engine.install();
            let marked = policy_store::mark_saved();
            assert_eq!(marked.is_ok(), !before.damaged, "marking at start: {marked:?}");
        }
        Op::Damage { kind } => {
            if engine.saved.is_none() {
                engine.saved = Some(std::fs::read(&engine.policy_file).ok());
            }
            std::fs::write(&engine.policy_file, Engine::damaged_bytes(*kind)).unwrap();
        }
        Op::Repair => match engine.saved.take() {
            Some(Some(bytes)) => std::fs::write(&engine.policy_file, bytes).unwrap(),
            Some(None) => std::fs::remove_file(&engine.policy_file).unwrap(),
            None => {}
        },
        Op::RemoveFile => {
            if !before.damaged {
                let _ = std::fs::remove_file(&engine.policy_file);
            }
        }
        Op::RebuildBegin { c } => {
            if before.rebuild.is_none() {
                if let Some(rebuild) = &after.rebuild {
                    let mut plan = rebuild.plan.clone();
                    plan.old_id = engine.eid(&plan.old_id);
                    let result =
                        rebuild_access::save_build_policies(&plan, Some(&rebuild.recorded));
                    ok = Some(result.is_ok());
                    engine.table = Some(Table {
                        sandboxes: RefCell::new(vec![
                            (plan.old_name.clone(), plan.old_id.clone()),
                            (plan.new_name.clone(), plan.new_id.clone()),
                        ]),
                        fail_after: RefCell::new(None),
                    });
                    engine.published = false;
                } else if let Some(computer) = pick(&before.computers, *c, linux) {
                    // The model refused it (the store is damaged): the engine must too.
                    let plan = RebuildPlan {
                        old_id: engine.eid(&computer.id),
                        old_name: computer.name.clone(),
                        new_id: "unused-new".into(),
                        new_name: "silo-rebuild-unused".into(),
                        undo_id: "unused-undo".into(),
                        undo_name: "unused-undo".into(),
                        image: "image".into(),
                        stage: "stage".into(),
                    };
                    ok = Some(
                        rebuild_access::save_build_policies(&plan, Some(&computer.recorded))
                            .is_ok(),
                    );
                }
            }
        }
        Op::RebuildSwap {
            fail_after,
            readable,
            publish_fails,
        } => {
            if let (Some(rebuild), Some(table)) = (before.rebuild.clone(), engine.table.as_ref()) {
                let mut plan = rebuild.plan.clone();
                plan.old_id = engine.eid(&plan.old_id);
                *table.fail_after.borrow_mut() = *fail_after;
                let published = RefCell::new(engine.published);
                let recorded = rebuild.recorded.clone();
                let result = rebuild_access::swap_with_policy(
                    &plan,
                    table,
                    &|| {
                        if *readable {
                            Ok(Some(recorded.clone()))
                        } else {
                            Err("the rules cannot be read".into())
                        }
                    },
                    &|| Ok(*published.borrow()),
                    &|| {
                        if *publish_fails {
                            Err("the inventory cannot be written".into())
                        } else {
                            *published.borrow_mut() = true;
                            Ok(())
                        }
                    },
                );
                *table.fail_after.borrow_mut() = None;
                engine.published = *published.borrow();
                if before.damaged {
                    assert!(result.is_err(), "a swap ran on an unreadable policy store");
                }
                if result.is_ok() {
                    assert!(engine.published);
                }
                note(format!(
                    "swap ok={} published={}",
                    result.is_ok(),
                    engine.published
                ));
                if engine.published {
                    // The undo computer is a computer now; the rebuild is done.
                    let rebuild = engine.model.rebuild.take().unwrap();
                    engine.model.computers.push(Computer {
                        id: rebuild.plan.undo_id.clone(),
                        name: rebuild.plan.undo_name.clone(),
                        kind: Kind::Undo,
                        approved: Approved::closed_without_incoming(),
                        entry: true,
                        marked: true,
                        recorded: rebuild.recorded.clone(),
                        channel: false,
                        maker: None,
                        parent: None,
                        undo_allowed: rebuild.allowed.clone(),
                    });
                    engine.settled = Some((plan, rebuild.recorded));
                    engine.table = None;
                    engine.published = false;
                }
            }
        }
        Op::RebuildSettle { readable } => {
            if let Some((plan, recorded)) = &engine.settled {
                if engine.model.get(&plan.undo_id).is_some() {
                    let _ = rebuild_access::settle_undo_policy(plan, &|| {
                        if *readable {
                            Ok(Some(recorded.clone()))
                        } else {
                            Err("the rules cannot be read".into())
                        }
                    });
                }
            }
        }
        Op::RebuildRollback => {
            if let (Some(rebuild), Some(table)) = (before.rebuild.clone(), engine.table.take()) {
                let mut plan = rebuild.plan.clone();
                plan.old_id = engine.eid(&plan.old_id);
                swap::revert(&table, &plan).expect("a swap that was not published reverts");
                let _ = rebuild_access::forget_build_policies(&plan);
                engine.model.rebuild = None;
                engine.published = false;
            }
        }
        Op::MacImport { interrupt } => {
            if before.computers.len() < MAX_COMPUTERS && !before.damaged {
                use crate::macos_computers::test_access as mac;
                let id = format!("00000000-0000-4000-8000-{:012}", 100 + after.next);
                let name = format!("mac{}", after.next);
                ok = Some(match interrupt {
                    0 => {
                        // The record is staged with the disk, then the folder is published.
                        mac::write_staged_record(&engine.data, &id, &name);
                        mac::publish_import_folder(&engine.data, &id).is_ok()
                    }
                    1 => mac::publish_import_folder(&engine.data, &id).is_ok(),
                    2 => {
                        // Stopped after the policy was saved, before the folder moved.
                        policy_store::set_stored(
                            &id,
                            policy_store::Stored::closed_without_incoming(),
                        )
                        .unwrap();
                        mac::recover_archive_import(&engine.data, &id) == Ok(false)
                            && policy_store::get_stored(&id).unwrap().is_none()
                    }
                    _ => {
                        // Published by a Silo that saved no policy, then recovered.
                        mac::write_published_record(&engine.data, &id, &name);
                        mac::recover_archive_import(&engine.data, &id) == Ok(true)
                    }
                });
            } else if before.computers.len() < MAX_COMPUTERS {
                use crate::macos_computers::test_access as mac;
                let id = format!("00000000-0000-4000-8000-{:012}", 100 + after.next);
                std::fs::create_dir_all(mac::import_folders(&engine.data, &id).0).unwrap();
                ok = Some(mac::publish_import_folder(&engine.data, &id).is_ok());
            }
        }
    }
    finish(engine, op, want, decision, ok)
}

/// Forks for an operation whose model added the fork last; maps its id. Whether it succeeded.
fn fork_into(
    engine: &mut Engine,
    before: &Model,
    after: &Model,
    source: &Computer,
    options: ForkOptions,
) -> bool {
    let added = new_computer(before, after);
    let name = added
        .as_ref()
        .map(|computer| computer.name.clone())
        .unwrap_or_else(|| format!("f{}", before.next + 1));
    match engine.fork(source, &name, options) {
        Ok(id) => {
            if let Some(added) = added {
                engine.ids.insert(added.id, id);
            }
            true
        }
        Err(_) => false,
    }
}

fn finish(engine: &mut Engine, op: &Op, want: Want, decision: Option<Expect>, ok: Option<bool>) {
    let name = format!("{op:?}");
    let name = name.split([' ', '{']).next().unwrap_or_default().to_owned();
    note(format!("{name} {decision:?} {ok:?}"));
    if want.decision.is_some() || decision.is_some() {
        assert_eq!(
            decision, want.decision,
            "{op:?}: the agent rules decided otherwise than the model"
        );
    }
    if want.ok.is_some() || ok.is_some() {
        assert_eq!(ok, want.ok, "{op:?}: the result differs from the model's");
    }
    engine.steps += 1;
    engine.install();
}

/// The rules a computer would be started with, as the runtime gets them; `Err` when the engine
/// refuses to start it.
fn enforced(engine: &Engine, computer: &Computer) -> Result<Reach, String> {
    let id = engine.eid(&computer.id);
    let plan = match computer.kind {
        Kind::Linux => {
            // As a pending start does: the rules its record holds stand for a computer with no
            // saved policy.
            let recorded = load(&engine.paths, &id)
                .map_err(|e| e.to_string())?
                .desired_network_policy
                .ok_or("no recorded rules")?;
            assert_eq!(
                recorded, computer.recorded,
                "{id} records other rules than the model's"
            );
            np::plan_for(&id, &computer.name, Some(&recorded))?
        }
        Kind::Macos => np::plan_for_gateway(&id, &computer.name)?,
        Kind::Undo => np::plan_for(&id, &computer.name, Some(&computer.recorded))?,
    };
    reach_of_runtime(&plan.policy_value(), plan.strict)
}

fn within_any(
    actual: &Reach,
    allowed: &[Approved],
    grants: &BTreeSet<&'static str>,
) -> Result<(), String> {
    let mut why = Vec::new();
    for approved in allowed {
        match actual.within(&reach_of(approved, grants, None, false)) {
            Ok(()) => return Ok(()),
            Err(wider) => why.push(wider),
        }
    }
    Err(why.join(" / "))
}

/// The safety rule and the model's agreement, for every computer.
fn check(engine: &Engine) {
    let model = &engine.model;
    let tokens = crate::guest_client::TokenStore::current().unwrap();
    let mut ports = BTreeMap::new();
    for computer in &model.computers {
        let id = engine.eid(&computer.id);
        let actual = enforced(engine, computer);
        if model.damaged {
            assert!(
                actual.is_err(),
                "{id} would start while its policy cannot be read: {actual:?}"
            );
            continue;
        }
        if computer.missing() {
            assert_eq!(
                actual,
                Err(policy_store::MISSING.to_owned()),
                "{id} would start while its saved policy is missing"
            );
            note("checked missing");
            continue;
        }
        let actual = actual.unwrap_or_else(|error| panic!("{id} cannot start: {error}"));
        note(format!("checked {:?}", computer.kind));
        if computer.kind == Kind::Undo {
            if let Err(wider) = within_any(&actual, &computer.undo_allowed, &model.grants) {
                panic!("WIDENED: undo computer {id} reaches {wider} beyond any policy its original was approved with");
            }
            continue;
        }
        let port = if computer.channel {
            let port = tokens
                .port_of(&id)
                .expect("a computer with tools has a port");
            if let Some(other) = ports.insert(port, id.clone()) {
                panic!("{id} and {other} share the channel port {port}");
            }
            Some(port)
        } else {
            None
        };
        let expected = reach_of(
            &computer.approved,
            &model.grants,
            port,
            computer.kind == Kind::Macos,
        );
        if let Err(wider) = actual.within(&expected) {
            panic!(
                "WIDENED: {id} ({:?}) reaches {wider}; approved {:?}, enforced {actual:?}",
                computer.kind, computer.approved
            );
        }
        assert_eq!(
            actual, expected,
            "{id}: the enforced rules are narrower than the model's; approved {:?}",
            computer.approved
        );
    }
    // While a rebuild runs, the build and the undo computer's sandbox hold one of the policies
    // the computer was approved with since the build was made.
    if let (Some(rebuild), Some(table)) = (&model.rebuild, &engine.table) {
        if model.damaged {
            return;
        }
        let plan = &rebuild.plan;
        for (id, name, recorded) in [
            (&plan.undo_id, &plan.undo_name, Some(&rebuild.recorded)),
            (&plan.new_id, &plan.new_name, None),
        ] {
            if !table.has(name)
                && !(id == &plan.undo_id
                    && table.has(&plan.old_name)
                    && table.owner(&plan.old_name).unwrap().as_deref() == Some(id.as_str()))
            {
                continue;
            }
            note(format!(
                "checked rebuild sandbox {}",
                if id == &plan.undo_id { "undo" } else { "build" }
            ));
            if rebuild.lost {
                assert_eq!(
                    np::plan_for(id, name, recorded).err().as_deref(),
                    Some(policy_store::MISSING),
                    "{name} would start while its saved policy is missing"
                );
                continue;
            }
            let plan = np::plan_for(id, name, recorded)
                .unwrap_or_else(|error| panic!("{name} cannot start: {error}"));
            let actual = reach_of_runtime(&plan.policy_value(), plan.strict).unwrap();
            if let Err(wider) = within_any(&actual, &rebuild.allowed, &model.grants) {
                panic!("WIDENED: {name} reaches {wider} during the rebuild");
            }
        }
    }
}

use swap::Sandboxes as _;

struct NetworkTest;

impl StateMachineTest for NetworkTest {
    type SystemUnderTest = Engine;
    type Reference = NetworkModel<DEFAULT>;

    fn init_test(initial: &Model) -> Engine {
        let guard = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let paths = restore_fixture(&directory, None);
        let data = directory.path().join("app-data");
        std::fs::create_dir_all(&data).unwrap();
        let engine = Engine {
            model: initial.clone(),
            ids: BTreeMap::new(),
            policy_file: data.join("network-policy.json"),
            tokens: data.join("guest-client"),
            data,
            paths,
            saved: None,
            table: None,
            published: false,
            settled: None,
            steps: 0,
            _directory: directory,
            _guard: guard,
        };
        engine.install();
        engine
    }

    fn apply(mut engine: Engine, after: &Model, transition: Transition) -> Engine {
        engine.install();
        match transition {
            Transition::Init(root) => {
                engine.model = after.clone();
                let mut record = load(&engine.paths, ID).unwrap();
                record.pending_checkpoint_restore = Some(PendingRestore {
                    checkpoint_id: MEMBER.into(),
                    source_computer: GROUP.into(),
                    state: "disk".into(),
                });
                record.snapshot_group = Some(GROUP.into());
                record.desired_network_policy = Some(after.computers[0].recorded.clone());
                save(&engine.paths, ID, &record).unwrap();
                if let Root::Entry(spec) | Root::UnmarkedEntry(spec) = &root {
                    service::set(
                        ID,
                        &spec.policy(),
                        None,
                        None,
                        service::Target::Linux(&engine.paths),
                    )
                    .unwrap();
                }
                if let Root::UnmarkedEntry(_) = root {
                    // Saved by a Silo from before marks.
                    crate::runtime::checkpoints::set_network_policy_mark(
                        &engine.linux_records(),
                        ID,
                        false,
                    )
                    .unwrap();
                }
            }
            Transition::Op(op) => engine_apply(&mut engine, &op),
        }
        engine
    }

    fn check_invariants(engine: &Engine, _: &Model) {
        engine.install();
        check(engine);
    }
}

fn cases() -> u32 {
    std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|cases| cases.parse().ok())
        .unwrap_or(1024)
}

prop_state_machine! {
    #![proptest_config(Config {
        cases: cases(),
        failure_persistence: None,
        max_shrink_iters: 4096,
        .. Config::default()
    })]

    /// Network access never widens without the owner's approval.
    #[test]
    fn network_access_never_widens_without_approval(sequential 1..50 => NetworkTest);
}

prop_state_machine! {
    #![proptest_config(Config {
        cases: cases(),
        failure_persistence: None,
        max_shrink_iters: 4096,
        .. Config::default()
    })]

    /// The same with the policy file removed among the operations.
    #[test]
    fn missing_policy_file_never_widens(sequential 1..50 => MissingFileTest);
}

struct MissingFileTest;

impl StateMachineTest for MissingFileTest {
    type SystemUnderTest = Engine;
    type Reference = NetworkModel<MISSING_FILE>;

    fn init_test(initial: &Model) -> Engine {
        NetworkTest::init_test(initial)
    }

    fn apply(engine: Engine, after: &Model, transition: Transition) -> Engine {
        NetworkTest::apply(engine, after, transition)
    }

    fn check_invariants(engine: &Engine, after: &Model) {
        NetworkTest::check_invariants(engine, after)
    }
}

prop_state_machine! {
    #![proptest_config(Config {
        cases: cases(),
        failure_persistence: None,
        max_shrink_iters: 4096,
        .. Config::default()
    })]

    /// The same, expecting an agent's narrowing or fork to keep refusing incoming connections.
    #[test]
    fn incoming_connections_stay_refused(sequential 1..50 => StrictIngressTest);
}

struct StrictIngressTest;

impl StateMachineTest for StrictIngressTest {
    type SystemUnderTest = Engine;
    type Reference = NetworkModel<STRICT_INGRESS>;

    fn init_test(initial: &Model) -> Engine {
        NetworkTest::init_test(initial)
    }

    fn apply(engine: Engine, after: &Model, transition: Transition) -> Engine {
        NetworkTest::apply(engine, after, transition)
    }

    fn check_invariants(engine: &Engine, after: &Model) {
        NetworkTest::check_invariants(engine, after)
    }
}
