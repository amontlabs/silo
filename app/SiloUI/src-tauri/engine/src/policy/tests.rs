use std::collections::{BTreeSet, HashMap, HashSet};

use serde_json::{json, Value};
use silo_api::methods::{method_registry, MethodName};

use super::*;

/// `main`, with `fork-1` and `fork-2` its agent made and `grand` that `fork-1`'s agent made
/// below it, `by-hand` that the owner forked from main, and an unrelated `stranger`.
struct World {
    created: HashMap<&'static str, &'static str>,
    access: HashMap<&'static str, Access>,
    github: Option<Vec<Value>>,
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
            github: Some(vec![
                json!({"computerId": "main", "repositoryMode": "selected"}),
                json!({"computerId": "stranger", "repositoryMode": "all"}),
            ]),
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
        Some(format!("{computer}-name"))
    }
    fn owns_job(&self, caller: &str, job_id: &str) -> bool {
        caller == "main" && job_id == "job:mine"
    }
    fn request_fork(&self, request_id: &str) -> Option<String> {
        match request_id {
            "fr-mine" => Some("fork-1".into()),
            "fr-hand" => Some("by-hand".into()),
            _ => None,
        }
    }
    fn access(&self, computer: &str) -> Access {
        self.access.get(computer).cloned().unwrap_or_default()
    }
    fn github_policies(&self) -> Option<Vec<Value>> {
        self.github.clone()
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
    ] {
        let decision = check(name, json!({"computerId": "fork-1", "forkId": "fork-1"}));
        match decision {
            Decision::Refuse { reason } => assert!(reason.contains(name), "{name}: {reason}"),
            other => panic!("{name}: {other:?}"),
        }
    }
}

#[test]
fn open_methods_are_allowed_and_job_methods_need_the_callers_job() {
    for name in ["engine.handshake", "engine.state", "device.facts", "engine.disconnect"] {
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
        json!(["/home/x", GUEST_TOKEN_PATH])
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
    let macos = json!({"kind": "macos", "request": {"name": "mac", "cpus": 4,
        "memoryGiB": 8, "diskGiB": 64}});
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
    assert_eq!(
        asked(&check("changes.promote", json!({"forkId": "grand"}))),
        Some(ApprovalKind::ReplaceMain)
    );
    assert_eq!(
        asked(&check("changes.requests.keep", json!({"requestId": "fr-mine"}))),
        Some(ApprovalKind::Merge)
    );
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
        asked(&check("push.start", json!({"computerId": "fork-1", "repositoryPath": "/w"}))),
        Some(ApprovalKind::Github)
    );
    let secret = |operation: &str, computers: Value| {
        json!({"request": {"operation": operation, "name": "token", "value": "s3cr3t-value",
            "computerIds": computers, "allowedDomains": ["api.example.com"]}})
    };
    match check("secrets.save", secret("add", json!(["fork-1"]))) {
        Decision::Ask { kind, summary } => {
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
        store.forget_makers(&["b"]).unwrap();
        assert!(store.snapshot().unwrap().made_by("main").is_empty());
        store.remove_computers(&["main"]).unwrap();
    }
}
