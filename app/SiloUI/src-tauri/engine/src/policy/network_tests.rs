//! The network policy decisions for a computer's own agent.

use std::collections::{HashMap, HashSet};

use serde_json::{json, Value};
use silo_api::{methods::MethodName, transport::ApprovalKind};

use super::*;

fn allowlist(hosts: &[&str]) -> NetworkPolicy {
    NetworkPolicy {
        mode: NetworkPolicyMode::Allowlist,
        hosts: hosts.iter().map(|host| host.to_string()).collect(),
        presets: Vec::new(),
    }
}

fn mode(mode: NetworkPolicyMode) -> NetworkPolicy {
    NetworkPolicy {
        mode,
        ..NetworkPolicy::default()
    }
}

/// `main` made `fork-1`; `stranger` is not in its tree.
struct World {
    policies: HashMap<&'static str, NetworkPolicy>,
    /// Sizes that differ from the common ones.
    sizes: HashMap<&'static str, Option<Resources>>,
}

impl World {
    fn with(main: NetworkPolicy, fork: NetworkPolicy) -> Self {
        Self {
            policies: HashMap::from([("main", main), ("fork-1", fork)]),
            sizes: HashMap::new(),
        }
    }
}

impl super::World for World {
    fn made(&self, creator: &str) -> HashSet<String> {
        match creator {
            "main" => HashSet::from(["fork-1".to_owned()]),
            _ => HashSet::new(),
        }
    }
    fn parent(&self, computer: &str) -> Option<String> {
        (computer == "fork-1").then(|| "main".to_owned())
    }
    fn name(&self, computer: &str) -> Option<String> {
        Some(computer.to_owned())
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
        "fresh-id".into()
    }
    fn resources(&self, computer: &str) -> Option<Resources> {
        self.sizes.get(computer).copied().unwrap_or_else(|| {
            Resources::of_linux(&json!({"cpus": 2, "maxCPUs": 4, "memoryGiB": 4,
                "maxMemoryGiB": 8, "workspaceStorageGiB": 20, "runtimeStorageGiB": 10}))
        })
    }
    fn ssh_settings(&self, _: &str) -> Option<(u16, String)> {
        None
    }
    fn network_policy(&self, computer: &str) -> Option<NetworkPolicy> {
        self.policies.get(computer).cloned()
    }
}

fn main() -> Caller {
    Caller::Computer { id: "main".into() }
}

fn set(world: &dyn super::World, computer: &str, policy: &NetworkPolicy) -> Decision {
    decide(
        &main(),
        MethodName::NetworkPolicySet,
        &json!({"computerId": computer, "policy": policy}),
        world,
    )
}

fn allowed_params(decision: Decision) -> Value {
    match decision {
        Decision::Allow(allowed) => allowed.params.expect("rewritten params"),
        other => panic!("expected allow, got {other:?}"),
    }
}

fn asked(decision: Decision) -> String {
    match decision {
        Decision::Ask {
            kind: ApprovalKind::Network,
            summary,
            ..
        } => summary,
        other => panic!("expected a network request, got {other:?}"),
    }
}

#[test]
fn reading_is_for_its_tree_only() {
    let world = World::with(NetworkPolicy::default(), NetworkPolicy::default());
    for computer in ["main", "fork-1"] {
        assert!(matches!(
            decide(&main(), MethodName::NetworkPolicyGet, &json!({"computerId": computer}), &world),
            Decision::Allow(_)
        ));
    }
    assert!(matches!(
        decide(&main(), MethodName::NetworkPolicyGet, &json!({"computerId": "stranger"}), &world),
        Decision::Refuse { .. }
    ));
}

#[test]
fn narrowing_its_own_policy_is_free_and_widening_asks() {
    let world = World::with(allowlist(&["a.example.com", "b.example.com"]), allowlist(&[]));
    let params = allowed_params(set(&world, "main", &allowlist(&["a.example.com"])));
    assert_eq!(params["policy"]["hosts"], json!(["a.example.com"]));
    assert!(matches!(
        set(&world, "main", &mode(NetworkPolicyMode::Closed)),
        Decision::Allow(_)
    ));
    let summary = asked(set(&world, "main", &allowlist(&["a.example.com", "evil.example.org"])));
    assert!(summary.contains("evil.example.org"), "{summary}");
    assert!(!summary.contains("a.example.com"), "{summary}");
    let summary = asked(set(&world, "main", &mode(NetworkPolicyMode::Open)));
    assert!(summary.contains("open (any public address)"), "{summary}");
}

#[test]
fn a_computer_it_made_gets_no_more_than_its_current_policy_or_the_callers() {
    // fork-1 is closed and main has an allowlist.
    let world = World::with(allowlist(&["a.example.com"]), mode(NetworkPolicyMode::Closed));
    // Wider than fork-1's current policy, even if within main's own, asks.
    asked(set(&world, "fork-1", &allowlist(&["a.example.com"])));
    // The owner widened fork-1 beyond main; narrowing it within main's own is free...
    let world = World::with(allowlist(&["a.example.com"]), NetworkPolicy::default());
    assert!(matches!(
        set(&world, "fork-1", &allowlist(&["a.example.com"])),
        Decision::Allow(_)
    ));
    // ...but narrowing it to something main itself cannot reach asks, naming the host.
    let summary = asked(set(&world, "fork-1", &allowlist(&["b.example.com"])));
    assert!(summary.contains("b.example.com"), "{summary}");
    // Not its tree: refused.
    assert!(matches!(
        set(&world, "stranger", &mode(NetworkPolicyMode::Closed)),
        Decision::Refuse { .. }
    ));
}

#[test]
fn an_invalid_policy_is_refused() {
    let world = World::with(NetworkPolicy::default(), NetworkPolicy::default());
    for policy in [
        json!({"mode": "allowlist", "hosts": ["1.2.3.4"]}),
        json!({"mode": "allowlist", "presets": ["nope"]}),
        json!({"mode": "sideways"}),
    ] {
        assert!(matches!(
            decide(
                &main(),
                MethodName::NetworkPolicySet,
                &json!({"computerId": "main", "policy": policy}),
                &world
            ),
            Decision::Refuse { .. }
        ));
    }
}

#[test]
fn a_fork_gets_the_callers_policy_or_narrower() {
    let fork = |world: &World, source: &str, policy: Option<NetworkPolicy>| {
        let mut params = json!({"computerId": source, "newName": "child"});
        if let Some(policy) = policy {
            params["networkPolicy"] = json!(policy);
        }
        decide(&main(), MethodName::CheckpointsFork, &params, world)
    };
    let own = allowlist(&["a.example.com"]);
    let world = World::with(own.clone(), NetworkPolicy::default());
    // Forking itself: its own policy.
    let params = allowed_params(fork(&world, "main", None));
    assert_eq!(params["networkPolicy"], json!(own));
    // Forking fork-1, which the owner opened: the fork gets main's policy, not fork-1's.
    let params = allowed_params(fork(&world, "fork-1", None));
    assert_eq!(params["networkPolicy"], json!(own));
    // A narrower one it asks for.
    let params = allowed_params(fork(&world, "main", Some(mode(NetworkPolicyMode::Closed))));
    assert_eq!(params["networkPolicy"]["mode"], "closed");
    // A wider one asks.
    let summary = asked(fork(&world, "main", Some(mode(NetworkPolicyMode::Open))));
    assert!(summary.contains("child"), "{summary}");
    // A source narrower than the caller keeps its own.
    let world = World::with(own.clone(), mode(NetworkPolicyMode::Closed));
    let params = allowed_params(fork(&world, "fork-1", None));
    assert_eq!(params["networkPolicy"]["mode"], "closed");
}

#[test]
fn a_new_computer_gets_the_callers_policy_or_narrower() {
    let own = allowlist(&["a.example.com"]);
    let world = World::with(own.clone(), NetworkPolicy::default());
    let create = |policy: Option<NetworkPolicy>| {
        let mut params = json!({"kind": "linux", "configuration": {
            "id": "x", "name": "new", "cpus": 1, "maxCPUs": 1, "memoryGiB": 1,
            "maxMemoryGiB": 1, "workspaceStorageGiB": 5, "runtimeStorageGiB": 5}});
        if let Some(policy) = policy {
            params["networkPolicy"] = json!(policy);
        }
        decide(&main(), MethodName::ComputersCreate, &params, &world)
    };
    assert_eq!(allowed_params(create(None))["networkPolicy"], json!(own));
    let params = allowed_params(create(Some(mode(NetworkPolicyMode::Closed))));
    assert_eq!(params["networkPolicy"]["mode"], "closed");
    let summary = asked(create(Some(allowlist(&["a.example.com", "other.example.net"]))));
    assert!(summary.contains("other.example.net"), "{summary}");
}

fn ask_params(decision: Decision) -> (ApprovalKind, Value) {
    match decision {
        Decision::Ask {
            kind,
            params: Some(params),
            ..
        } => (kind, params),
        other => panic!("expected a request with parameters, got {other:?}"),
    }
}

fn linux_request(extra: Value) -> Value {
    let mut configuration = json!({
        "id": "x", "name": "new", "cpus": 1, "maxCPUs": 1, "memoryGiB": 1,
        "maxMemoryGiB": 1, "workspaceStorageGiB": 5, "runtimeStorageGiB": 5});
    for (key, value) in extra.as_object().into_iter().flatten() {
        configuration[key] = value.clone();
    }
    json!({"kind": "linux", "configuration": configuration})
}

fn macos_request(cpus: u64) -> Value {
    json!({"kind": "macos", "request": {"name": "mac", "cpus": cpus,
        "memoryGiB": 2, "diskGiB": 10}})
}

#[test]
fn a_creation_that_asks_runs_with_the_policy_it_was_judged_against() {
    let own = allowlist(&["a.example.com"]);
    let world = World::with(own.clone(), NetworkPolicy::default());
    let create = |mut params: Value, policy: Option<&NetworkPolicy>| {
        if let Some(policy) = policy {
            params["networkPolicy"] = json!(policy);
        }
        decide(&main(), MethodName::ComputersCreate, &params, &world)
    };
    // Larger than the caller, asking nothing about the network: the inherited policy goes with it.
    for request in [macos_request(8), linux_request(json!({"cpus": 8}))] {
        let (kind, params) = ask_params(create(request, None));
        assert_eq!(kind, ApprovalKind::Resources);
        assert_eq!(params["networkPolicy"], json!(own));
    }
    let (kind, params) = ask_params(create(linux_request(json!({"runtime": "container"})), None));
    assert_eq!(kind, ApprovalKind::Container);
    assert_eq!(params["networkPolicy"], json!(own));
    // A wider policy that was asked for is the one the owner is asked about.
    let wider = allowlist(&["a.example.com", "other.example.net"]);
    for request in [macos_request(1), linux_request(json!({}))] {
        let (kind, params) = ask_params(create(request, Some(&wider)));
        assert_eq!(kind, ApprovalKind::Network);
        assert_eq!(params["networkPolicy"], json!(wider));
    }
}

#[test]
fn a_creation_by_a_caller_with_an_unreadable_policy_never_runs_without_one() {
    let world = World {
        policies: HashMap::new(),
        sizes: HashMap::new(),
    };
    let closed = mode(NetworkPolicyMode::Closed);
    for request in [macos_request(1), macos_request(8), linux_request(json!({}))] {
        let decision = decide(&main(), MethodName::ComputersCreate, &request, &world);
        let (kind, params) = ask_params(decision);
        assert!(matches!(kind, ApprovalKind::Network | ApprovalKind::Resources));
        assert_eq!(params["networkPolicy"], json!(closed));
    }
    let fork = json!({"computerId": "main", "newName": "child"});
    let (kind, params) = ask_params(decide(&main(), MethodName::CheckpointsFork, &fork, &world));
    assert_eq!(kind, ApprovalKind::Network);
    assert_eq!(params["networkPolicy"], json!(closed));
}

#[test]
fn a_fork_larger_than_the_caller_asks_and_carries_the_policy() {
    let own = allowlist(&["a.example.com"]);
    let mut world = World::with(own.clone(), NetworkPolicy::default());
    let fork = |world: &World, source: &str| {
        decide(
            &main(),
            MethodName::CheckpointsFork,
            &json!({"computerId": source, "newName": "child"}),
            world,
        )
    };
    allowed_params(fork(&world, "fork-1"));
    // The owner has enlarged fork-1 since.
    world.sizes.insert(
        "fork-1",
        Resources::of_linux(&json!({"cpus": 8, "maxCPUs": 8, "memoryGiB": 16,
            "maxMemoryGiB": 16, "workspaceStorageGiB": 20, "runtimeStorageGiB": 10})),
    );
    match fork(&world, "fork-1") {
        Decision::Ask {
            kind,
            summary,
            params: Some(params),
            creates: Some(creates),
            ..
        } => {
            assert_eq!(kind, ApprovalKind::Resources);
            assert!(summary.contains("child") && summary.contains("CPUs 8"), "{summary}");
            assert_eq!(params["networkPolicy"], json!(own));
            assert_eq!(creates.source.as_deref(), Some("fork-1"));
        }
        other => panic!("expected a request about the sizes, got {other:?}"),
    }
    // Forking itself copies its own sizes.
    allowed_params(fork(&world, "main"));
}

#[test]
fn an_unreadable_policy_asks() {
    let world = World {
        policies: HashMap::new(),
        sizes: HashMap::new(),
    };
    asked(set(&world, "main", &mode(NetworkPolicyMode::Closed)));
}

#[test]
fn a_network_request_runs_the_checked_policy_and_names_the_computer_plainly() {
    struct Named(World);
    impl super::World for Named {
        fn made(&self, creator: &str) -> HashSet<String> {
            self.0.made(creator)
        }
        fn parent(&self, computer: &str) -> Option<String> {
            self.0.parent(computer)
        }
        fn name(&self, computer: &str) -> Option<String> {
            Some(match computer {
                "fork-1" => "evil\u{202e}\"name".to_owned(),
                other => other.to_owned(),
            })
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
            self.0.new_id()
        }
        fn resources(&self, computer: &str) -> Option<Resources> {
            self.0.resources(computer)
        }
        fn ssh_settings(&self, _: &str) -> Option<(u16, String)> {
            None
        }
        fn network_policy(&self, computer: &str) -> Option<NetworkPolicy> {
            self.0.network_policy(computer)
        }
    }
    let world = Named(World::with(
        allowlist(&["a.example.com"]),
        mode(NetworkPolicyMode::Closed),
    ));
    let asked_policy = allowlist(&["b.example.com", "a.example.com", "a.example.com"]);
    let decision = decide(
        &main(),
        MethodName::NetworkPolicySet,
        &json!({"computerId": "fork-1", "policy": asked_policy}),
        &world,
    );
    let Decision::Ask {
        kind: ApprovalKind::Network,
        summary,
        params: Some(params),
        ..
    } = decision
    else {
        panic!("expected a network request that carries its parameters, got {decision:?}");
    };
    assert!(summary.contains("“evil"), "{summary}");
    assert!(!summary.contains('\u{202e}'), "{summary}");
    assert_eq!(params["policy"]["hosts"], json!(["a.example.com", "b.example.com"]));
}

/// A world whose computers are judged the way the service reads them: from the saved policy
/// and the runtime's rules.
struct Served {
    held: HashMap<&'static str, Held>,
}

impl super::World for Served {
    fn made(&self, creator: &str) -> HashSet<String> {
        match creator {
            "main" => HashSet::from(["fork-1".to_owned()]),
            _ => HashSet::new(),
        }
    }
    fn parent(&self, computer: &str) -> Option<String> {
        (computer == "fork-1").then(|| "main".to_owned())
    }
    fn name(&self, computer: &str) -> Option<String> {
        Some(computer.to_owned())
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
        "fresh-id".into()
    }
    fn resources(&self, _: &str) -> Option<Resources> {
        Resources::of_linux(&json!({"cpus": 2, "maxCPUs": 4, "memoryGiB": 4,
            "maxMemoryGiB": 8, "workspaceStorageGiB": 20, "runtimeStorageGiB": 10}))
    }
    fn ssh_settings(&self, _: &str) -> Option<(u16, String)> {
        None
    }
    fn network_held(&self, computer: &str) -> Option<Held> {
        self.held.get(computer).cloned()
    }
}

fn view(network: Value) -> crate::runtime::checkpoints::NetworkPolicyView {
    crate::runtime::checkpoints::NetworkPolicyView {
        name: "main".into(),
        container: false,
        running: true,
        pending: false,
        network: Some(network),
        recorded: None,
    }
}

#[test]
fn an_imported_computer_cannot_open_itself_or_hand_an_open_policy_to_a_fork() {
    let directory = tempfile::tempdir().unwrap();
    crate::network_policy::store::use_test_path(Some(directory.path().join("network-policy.json")));
    let deny_all = json!({"default_egress": "deny", "default_ingress": "deny", "rules": []});
    let github_default: Value =
        serde_json::from_str(include_str!("../../../guest/github-network-default.json")).unwrap();
    // The import ran closed on deny-all rules; however its entry got lost, it is judged closed.
    let judged = |stored, network: &Value| {
        crate::network_policy::service::judged_from_view("main", stored, view(network.clone()))
    };
    let imported = judged(None, &json!({"policy": deny_all}));
    assert_eq!(imported.policy.mode, NetworkPolicyMode::Closed);
    let world = Served {
        held: HashMap::from([("main", imported.clone()), ("fork-1", imported.clone())]),
    };
    let summary = asked(set(&world, "main", &mode(NetworkPolicyMode::Open)));
    assert!(summary.contains("open (any public address)"), "{summary}");
    // A fork it makes without asking for a policy gets the closed one.
    let params = allowed_params(decide(
        &main(),
        MethodName::CheckpointsFork,
        &json!({"computerId": "main", "newName": "copy"}),
        &world,
    ));
    assert_eq!(params["networkPolicy"]["mode"], "closed");
    // An older computer with the open rules and no entry is still judged open.
    let older = judged(None, &github_default);
    assert_eq!(older.policy.mode, NetworkPolicyMode::Open);
    let world = Served {
        held: HashMap::from([("main", older.clone()), ("fork-1", older)]),
    };
    assert!(matches!(
        set(&world, "main", &mode(NetworkPolicyMode::Open)),
        Decision::Allow(_)
    ));
    // A computer whose policy cannot be read asks.
    let world = Served { held: HashMap::new() };
    assert!(matches!(
        set(&world, "main", &mode(NetworkPolicyMode::Closed)),
        Decision::Ask { .. }
    ));
    crate::network_policy::store::use_test_path(None);
}

#[test]
fn a_free_narrowing_carries_the_policy_it_was_judged_against() {
    let wide = allowlist(&["a.example.com", "b.example.com"]);
    let world = World::with(wide.clone(), allowlist(&[]));
    let params = allowed_params(set(&world, "main", &allowlist(&["a.example.com"])));
    assert_eq!(params["expected"], json!(wide));
    // A request the owner approves is exactly what was asked and carries no expectation.
    let Decision::Ask { params, .. } = set(&world, "main", &mode(NetworkPolicyMode::Open)) else {
        panic!("widening asks");
    };
    assert!(params.as_ref().and_then(|params| params.get("expected")).is_none());
}

#[test]
fn a_preset_update_is_wider_for_a_computer_that_has_not_accepted_it() {
    use crate::network_policy::{use_test_presets, Allowed, Pins, Preset};
    let at = |host: &str| Allowed {
        host: host.to_owned(),
        port: 443,
    };
    use_test_presets(Some(vec![Preset {
        name: "tools".into(),
        title: "Tools".into(),
        version: 2,
        hosts: vec![at("a.example.com"), at("b.example.com")],
        history: vec![(1, vec![at("a.example.com")])],
    }]));
    let policy = NetworkPolicy {
        mode: NetworkPolicyMode::Allowlist,
        hosts: Vec::new(),
        presets: vec!["tools".into()],
    };
    let approved_at_one = Held {
        policy: policy.clone(),
        pins: Pins::new(),
    };
    let world = Served {
        held: HashMap::from([("main", approved_at_one.clone()), ("fork-1", approved_at_one)]),
    };
    // Setting the same presets would take the update, so it asks and names the new host.
    let summary = asked(set(&world, "main", &policy));
    assert!(summary.contains("b.example.com") && !summary.contains("a.example.com"), "{summary}");
    use_test_presets(None);
}

fn fork_with_secret(world: &World, source: &str, params: Value) -> Decision {
    let mut params = params;
    params["computerId"] = json!(source);
    params["newName"] = json!("child");
    decide(&main(), MethodName::CheckpointsFork, &params, world)
}

fn access_summary(decision: Decision) -> String {
    match decision {
        Decision::Ask {
            kind: ApprovalKind::Access,
            summary,
            ..
        } => summary,
        other => panic!("expected a request for more access, got {other:?}"),
    }
}

#[test]
fn a_fork_wider_in_access_and_larger_shows_both_in_one_summary() {
    let mut world = World::with(allowlist(&["a.example.com"]), NetworkPolicy::default());
    world.sizes.insert(
        "fork-1",
        Resources::of_linux(&json!({"cpus": 8, "maxCPUs": 8, "memoryGiB": 16,
            "maxMemoryGiB": 16, "workspaceStorageGiB": 20, "runtimeStorageGiB": 10})),
    );
    let summary = access_summary(fork_with_secret(
        &world,
        "fork-1",
        json!({"secrets": ["API_KEY"]}),
    ));
    assert!(summary.contains("API_KEY"), "{summary}");
    assert!(
        summary.contains("8 CPUs") && summary.contains("16 GiB"),
        "{summary}"
    );
    assert!(summary.contains("which is more than “main” has"), "{summary}");
}

#[test]
fn a_fork_wider_in_access_says_when_the_sizes_could_not_be_read() {
    let mut world = World::with(allowlist(&["a.example.com"]), NetworkPolicy::default());
    world.sizes.insert("fork-1", None);
    let summary = access_summary(fork_with_secret(
        &world,
        "fork-1",
        json!({"secrets": ["API_KEY"]}),
    ));
    assert!(summary.contains("API_KEY"), "{summary}");
    assert!(
        summary.contains("could not be read to compare them"),
        "{summary}"
    );
}

#[test]
fn a_creation_by_a_caller_with_an_unreadable_policy_names_the_policy_it_asked_for() {
    let world = World {
        policies: HashMap::new(),
        sizes: HashMap::new(),
    };
    let open = mode(NetworkPolicyMode::Open);
    let mut request = linux_request(json!({}));
    request["networkPolicy"] = json!(open);
    match decide(&main(), MethodName::ComputersCreate, &request, &world) {
        Decision::Ask {
            summary,
            params: Some(params),
            ..
        } => {
            assert_eq!(params["networkPolicy"], json!(open));
            assert!(summary.contains("open (any public address)"), "{summary}");
        }
        other => panic!("expected a request, got {other:?}"),
    }
    let fork = json!({"computerId": "main", "newName": "child", "secrets": ["API_KEY"]});
    let summary = access_summary(decide(&main(), MethodName::CheckpointsFork, &fork, &world));
    assert!(
        summary.contains("could not be read to compare"),
        "{summary}"
    );
}
