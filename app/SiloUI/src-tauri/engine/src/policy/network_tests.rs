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
}

impl World {
    fn with(main: NetworkPolicy, fork: NetworkPolicy) -> Self {
        Self {
            policies: HashMap::from([("main", main), ("fork-1", fork)]),
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
    fn resources(&self, _: &str) -> Option<Resources> {
        Resources::of_linux(&json!({"cpus": 2, "maxCPUs": 4, "memoryGiB": 4,
            "maxMemoryGiB": 8, "workspaceStorageGiB": 20, "runtimeStorageGiB": 10}))
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

fn set(world: &World, computer: &str, policy: &NetworkPolicy) -> Decision {
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

#[test]
fn an_unreadable_policy_asks() {
    let world = World {
        policies: HashMap::new(),
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
