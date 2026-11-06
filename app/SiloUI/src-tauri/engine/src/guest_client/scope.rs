//! What a guest client's token may ask the engine for. Every method not listed here is
//! refused, whatever its parameters. Each listed method also names the computer it acts on,
//! and that computer must be the caller or one of its forks, as the case needs.

use std::collections::HashSet;

use serde_json::{json, Value};
use silo_api::methods::guest_client::{GuestClientPolicy, GUEST_TOKEN_PATH};

use super::tokens::{MAX_FORKS, MAX_JOBS};

/// The most of its forks one token may have running at once. Each counts as the memory it was
/// created with; the engine does not limit their total.
pub const MAX_RUNNING: usize = 3;

/// What the scope check needs to know about the machine.
pub trait Topology {
    /// The forks of `caller` that its token asked for and that still exist. Forks the person
    /// made, or that another token made, are not among them.
    fn created(&self, caller: &str) -> HashSet<String>;
    /// How many of those are running.
    fn running(&self, caller: &str) -> usize;
    /// How many forks this token holds: made and still being made.
    fn forks_asked(&self, caller: &str) -> usize;
    /// Whether a computer by this name exists or a fork by it is being made.
    fn name_taken(&self, name: &str) -> bool;
    /// How many jobs this caller started and has not acknowledged.
    fn jobs_outstanding(&self, caller: &str) -> usize;
    /// The fork a review request is about.
    fn request_fork(&self, request_id: &str) -> Option<String>;
    /// Whether this caller started the job.
    fn owns_job(&self, caller: &str, job_id: &str) -> bool;
}

/// How a result is cut down before it goes back into the guest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    None,
    Computers,
    Tree,
    Requests,
}

#[derive(Debug)]
pub struct Authorized {
    /// The parameters to run, possibly rewritten.
    pub params: Value,
    pub filter: Filter,
    /// The result holds a `jobId` this caller now owns.
    pub starts_job: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Refusal(pub String);

fn refuse<T>(message: impl Into<String>) -> Result<T, Refusal> {
    Err(Refusal(message.into()))
}

/// The methods a guest client may call, for documentation and tests.
pub const ALLOWED: &[&str] = &[
    "computers.list",
    "computers.tree",
    "computers.start",
    "computers.stop",
    "computers.credentials",
    "checkpoints.fork",
    "changes.diff",
    "changes.request",
    "changes.requests.list",
    "changes.requests.show",
    "steps.list",
    "operations.job.get",
    "operations.job.acknowledge",
];

fn text<'a>(params: &'a Value, key: &str) -> Option<&'a str> {
    params
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
}

/// A request for another device is not served here.
fn local_only(params: &Value) -> Result<(), Refusal> {
    // `allDevices` and `deviceIds` route a call to other devices; nothing from a guest does.
    let routes_elsewhere = params
        .get("allDevices")
        .is_some_and(|value| !matches!(value, Value::Null | Value::Bool(false)))
        || params
            .get("deviceIds")
            .is_some_and(|value| !matches!(value, Value::Null) && value != &json!([]));
    if routes_elsewhere {
        return refuse("The in-guest client reaches this computer's engine only.");
    }
    match params.get("deviceId") {
        None | Some(Value::Null) => Ok(()),
        Some(Value::String(device)) if device == "local" => Ok(()),
        Some(_) => refuse("The in-guest client reaches this computer's engine only."),
    }
}

/// The computer in `key` must be a fork of the caller.
fn fork_of_caller(
    params: &Value,
    key: &str,
    forks: &HashSet<String>,
    what: &str,
) -> Result<(), Refusal> {
    match text(params, key) {
        Some(id) if forks.contains(id) => Ok(()),
        Some(_) => refuse(format!("{what} is not one of this computer's forks.")),
        None => refuse(format!("{what} must be named.")),
    }
}

pub fn authorize(
    caller: &str,
    policy: &GuestClientPolicy,
    method: &str,
    params: &Value,
    topology: &dyn Topology,
) -> Result<Authorized, Refusal> {
    let authorized = authorize_call(caller, policy, method, params, topology)?;
    // A job nobody follows still takes a place in the engine's job admission.
    if authorized.starts_job && topology.jobs_outstanding(caller) >= MAX_JOBS {
        return refuse(format!(
            "{MAX_JOBS} jobs of this computer's client are not finished and acknowledged yet."
        ));
    }
    Ok(authorized)
}

fn authorize_call(
    caller: &str,
    policy: &GuestClientPolicy,
    method: &str,
    params: &Value,
    topology: &dyn Topology,
) -> Result<Authorized, Refusal> {
    if !ALLOWED.contains(&method) {
        return refuse(format!(
            "{method} is not available to a computer's own agent. Ask the person who runs Silo."
        ));
    }
    if !params.is_object() && !params.is_null() {
        return refuse("The parameters must be an object.");
    }
    let params = if params.is_null() {
        json!({})
    } else {
        params.clone()
    };
    local_only(&params)?;
    let allowed = |params: Value, filter: Filter, starts_job: bool| {
        Ok(Authorized {
            params,
            filter,
            starts_job,
        })
    };
    let forks = topology.created(caller);
    match method {
        // Only the two lists the client reads; nothing else the caller sends is passed on.
        "computers.list" => allowed(json!({}), Filter::Computers, false),
        "computers.tree" => allowed(json!({}), Filter::Tree, false),
        "computers.start" | "computers.stop" | "computers.credentials" => {
            fork_of_caller(&params, "computerId", &forks, "That computer")?;
            if method == "computers.start" && topology.running(caller) >= MAX_RUNNING {
                return refuse(format!(
                    "{MAX_RUNNING} of this computer's forks are running; stop one first."
                ));
            }
            allowed(params, Filter::None, method != "computers.credentials")
        }
        "changes.diff" => {
            fork_of_caller(&params, "computerId", &forks, "That computer")?;
            allowed(params, Filter::None, true)
        }
        "steps.list" => {
            if text(&params, "computerId") != Some(caller) {
                fork_of_caller(&params, "computerId", &forks, "That computer")?;
            }
            allowed(params, Filter::None, false)
        }
        "checkpoints.fork" => {
            if text(&params, "computerId") != Some(caller) {
                return refuse("A fork can be made from this computer only.");
            }
            if topology.forks_asked(caller) >= MAX_FORKS {
                return refuse(format!("This token holds {MAX_FORKS} forks already."));
            }
            let name = text(&params, "newName").unwrap_or_default();
            if crate::computer_tree::hidden(name) {
                return refuse("That name is reserved for Silo's own computers.");
            }
            if topology.name_taken(name) {
                return refuse(format!("A computer named {name} exists or is being made."));
            }
            if topology.running(caller) >= MAX_RUNNING {
                return refuse(format!(
                    "{MAX_RUNNING} of this computer's forks are running; stop one first."
                ));
            }
            fork_rewrite(&params, policy)
        }
        "changes.request" => {
            fork_of_caller(&params, "forkId", &forks, "That fork")?;
            if text(&params, "targetId").is_some_and(|target| target != caller) {
                return refuse("A review request can name only this computer as its target.");
            }
            allowed(params, Filter::None, false)
        }
        "changes.requests.list" => {
            let status = params.get("status").cloned().unwrap_or(Value::Null);
            let list = if status.is_null() {
                json!({})
            } else {
                json!({ "status": status })
            };
            allowed(list, Filter::Requests, false)
        }
        "changes.requests.show" => {
            let fork = text(&params, "requestId").and_then(|id| topology.request_fork(id));
            match fork {
                Some(fork) if forks.contains(&fork) => allowed(params, Filter::None, true),
                _ => refuse("That request is not about one of this computer's forks."),
            }
        }
        _ => {
            // The job methods: only a job this caller started.
            match text(&params, "jobId") {
                Some(job) if topology.owns_job(caller, job) => allowed(params, Filter::None, false),
                _ => refuse("That job was not started by this computer's client."),
            }
        }
    }
}

/// A fork made from inside gets no secret or GitHub access the settings did not name, and
/// loses the token file.
fn fork_rewrite(params: &Value, policy: &GuestClientPolicy) -> Result<Authorized, Refusal> {
    if let Some(secrets) = params.get("secrets").and_then(Value::as_array) {
        for secret in secrets {
            let name = secret.as_str().unwrap_or_default();
            if !policy.fork_secrets.iter().any(|allowed| allowed == name) {
                return refuse(format!(
                    "{name} is not in this agent's fork_secrets, so a fork made from inside cannot have it."
                ));
            }
        }
    }
    if params
        .get("newName")
        .and_then(Value::as_str)
        .is_none_or(str::is_empty)
    {
        return refuse("A fork needs a name.");
    }
    // Built from the fields the client may set: the name and the secrets. The current state is
    // forked (no checkpoint, no recipe hash), and the quiet list is fixed.
    let mut fork = json!({
        "computerId": params["computerId"],
        "newName": params.get("newName").cloned().unwrap_or(Value::Null),
        "quiet": { "stop": [], "scrub": [GUEST_TOKEN_PATH] },
    });
    if let Some(secrets) = params.get("secrets").filter(|secrets| !secrets.is_null()) {
        fork["secrets"] = secrets.clone();
    }
    Ok(Authorized {
        params: fork,
        filter: Filter::None,
        starts_job: true,
    })
}

/// Cuts a result down to the caller and the forks its token asked for.
pub fn filter_result(filter: Filter, result: &mut Value, caller: &str, topology: &dyn Topology) {
    let mut visible = topology.created(caller);
    visible.insert(caller.to_owned());
    match filter {
        Filter::None => {}
        Filter::Computers => {
            // Only the two lists of computers go back, never the rest of the listing.
            let mut kept = json!({"linux": {"computers": []}, "macos": {"computers": []}});
            for (list, id) in [("linux", "/configuration/id"), ("macos", "/id")] {
                if let Some(computers) = result
                    .pointer(&format!("/{list}/computers"))
                    .and_then(Value::as_array)
                {
                    kept[list]["computers"] = Value::Array(
                        computers
                            .iter()
                            .filter(|computer| {
                                computer
                                    .pointer(id)
                                    .and_then(Value::as_str)
                                    .is_some_and(|found| visible.contains(found))
                            })
                            .cloned()
                            .collect(),
                    );
                }
            }
            *result = kept;
        }
        Filter::Tree => {
            if let Some(nodes) = result.get_mut("nodes").and_then(Value::as_array_mut) {
                nodes.retain(|node| {
                    node.get("id")
                        .and_then(Value::as_str)
                        .is_some_and(|found| visible.contains(found))
                });
            }
        }
        Filter::Requests => {
            if let Some(entries) = result.get_mut("requests").and_then(Value::as_array_mut) {
                entries.retain(|entry| {
                    entry
                        .pointer("/request/forkId")
                        .and_then(Value::as_str)
                        .is_some_and(|found| visible.contains(found))
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct World;

    impl Topology for World {
        fn created(&self, computer: &str) -> HashSet<String> {
            if computer == "main" {
                ["fork-1", "fork-2"].map(String::from).into()
            } else {
                HashSet::new()
            }
        }
        fn running(&self, computer: &str) -> usize {
            usize::from(computer == "busy") * MAX_RUNNING
        }
        fn forks_asked(&self, computer: &str) -> usize {
            if computer == "greedy" {
                MAX_FORKS
            } else {
                2
            }
        }
        fn name_taken(&self, name: &str) -> bool {
            name == "taken"
        }
        fn jobs_outstanding(&self, computer: &str) -> usize {
            if computer == "jobby" {
                MAX_JOBS
            } else {
                0
            }
        }
        fn request_fork(&self, request_id: &str) -> Option<String> {
            match request_id {
                "fr-mine" => Some("fork-1".into()),
                "fr-other" => Some("stranger".into()),
                _ => None,
            }
        }
        fn owns_job(&self, caller: &str, job_id: &str) -> bool {
            caller == "main" && job_id == "job:mine"
        }
    }

    fn policy(secrets: &[&str]) -> GuestClientPolicy {
        GuestClientPolicy {
            fork_secrets: secrets.iter().map(|name| (*name).to_owned()).collect(),
            agent: Some("openclaw".into()),
        }
    }

    fn check(method: &str, params: Value) -> Result<Authorized, Refusal> {
        authorize("main", &policy(&[]), method, &params, &World)
    }

    #[test]
    fn every_method_that_keeps_promotes_undoes_drops_or_grants_is_refused() {
        for method in [
            "changes.keep",
            "changes.promote",
            "changes.undo",
            "changes.requests.keep",
            "changes.requests.throwAway",
            "changes.requests.markKept",
            "changes.requests.open",
            "computers.delete",
            "computers.create",
            "computers.restart",
            "computers.forceStop",
            "checkpoints.restore",
            "checkpoints.delete",
            "secrets.save",
            "secrets.list",
            "secrets.remove",
            "github.signIn",
            "guestClient.enable",
            "guestClient.revoke",
            "guest.exec",
            "guest.files.write",
            "files.upload",
            "network.publish",
            "engine.shutdown",
        ] {
            let refusal =
                check(method, json!({"computerId": "fork-1", "forkId": "fork-1"})).unwrap_err();
            assert!(refusal.0.contains(method), "{method}: {}", refusal.0);
        }
    }

    #[test]
    fn a_fork_of_self_is_allowed_and_another_computer_is_refused() {
        assert!(check(
            "checkpoints.fork",
            json!({"computerId": "main", "newName": "x"})
        )
        .is_ok());
        assert!(check(
            "checkpoints.fork",
            json!({"computerId": "fork-1", "newName": "x"})
        )
        .is_err());
        assert!(check(
            "checkpoints.fork",
            json!({"computerId": "stranger", "newName": "x"})
        )
        .is_err());
        for method in ["checkpoints.create", "changes.capture", "changes.baseline"] {
            assert!(check(method, json!({"computerId": "main", "name": "x"})).is_err());
        }
        assert!(check("changes.diff", json!({"computerId": "fork-2"})).is_ok());
        assert!(check("changes.diff", json!({"computerId": "main"})).is_err());
        assert!(check("changes.diff", json!({"computerId": "stranger"})).is_err());
        assert!(check("computers.stop", json!({"computerId": "main"})).is_err());
        assert!(check("computers.start", json!({"computerId": "fork-1"})).is_ok());
        assert!(check("steps.list", json!({"computerId": "main"})).is_ok());
        assert!(check("steps.list", json!({"computerId": "stranger"})).is_err());
    }

    #[test]
    fn a_request_is_for_own_forks_and_targets_only_the_caller() {
        assert!(check("changes.request", json!({"forkId": "fork-1"})).is_ok());
        assert!(check(
            "changes.request",
            json!({"forkId": "fork-1", "targetId": "main"})
        )
        .is_ok());
        assert!(check(
            "changes.request",
            json!({"forkId": "fork-1", "targetId": "other"})
        )
        .is_err());
        assert!(check("changes.request", json!({"forkId": "stranger"})).is_err());
        assert!(check("changes.requests.show", json!({"requestId": "fr-mine"})).is_ok());
        assert!(check("changes.requests.show", json!({"requestId": "fr-other"})).is_err());
        assert!(check("changes.requests.show", json!({"requestId": "fr-none"})).is_err());
    }

    #[test]
    fn another_device_is_refused_and_jobs_must_be_the_callers() {
        assert!(check("computers.list", json!({"deviceId": "other-mac"})).is_err());
        assert!(check("computers.list", json!({"deviceId": "local"})).is_ok());
        assert!(check("operations.job.get", json!({"jobId": "job:mine"})).is_ok());
        assert!(check("operations.job.get", json!({"jobId": "job:theirs"})).is_err());
        assert!(authorize(
            "fork-1",
            &policy(&[]),
            "operations.job.get",
            &json!({"jobId": "job:mine"}),
            &World
        )
        .is_err());
    }

    #[test]
    fn a_fork_loses_the_token_file_and_gets_only_the_secrets_the_settings_name() {
        let params = json!({"computerId": "main", "newName": "x", "recipeHash": "h",
            "checkpointId": "cp", "quiet": {"stop": ["poller"], "scrub": ["/home/x"]}});
        let granted = check("checkpoints.fork", params).unwrap();
        assert_eq!(granted.params["quiet"]["stop"], json!([]));
        assert_eq!(granted.params["quiet"]["scrub"], json!([GUEST_TOKEN_PATH]));
        assert!(granted.params.get("recipeHash").is_none());
        assert!(granted.params.get("checkpointId").is_none());
        assert!(granted.starts_job);
        let with_secret = json!({"computerId": "main", "newName": "x", "secrets": ["github"]});
        assert!(check("checkpoints.fork", with_secret.clone()).is_err());
        let allowed = authorize(
            "main",
            &policy(&["github"]),
            "checkpoints.fork",
            &with_secret,
            &World,
        );
        assert!(allowed.is_ok());
    }

    #[test]
    fn reserved_and_taken_names_are_refused_and_jobs_are_capped() {
        for name in ["silo-build-x", "silo-baseline-1", "taken", ""] {
            let fork = json!({"computerId": "main", "newName": name});
            assert!(check("checkpoints.fork", fork).is_err(), "{name}");
        }
        let fork = json!({"computerId": "jobby", "newName": "ok"});
        assert!(authorize("jobby", &policy(&[]), "checkpoints.fork", &fork, &World).is_err());
        let diff = json!({"computerId": "fork-1"});
        assert!(authorize("jobby", &policy(&[]), "computers.list", &json!({}), &World).is_ok());
        assert!(check("changes.diff", diff).is_ok());
    }

    #[test]
    fn nothing_routes_a_call_to_another_device() {
        for params in [
            json!({"allDevices": true}),
            json!({"deviceIds": ["other"]}),
            json!({"deviceId": "other"}),
        ] {
            assert!(
                check("changes.requests.list", params.clone()).is_err(),
                "{params}"
            );
            assert!(check("computers.list", params).is_err());
        }
        let list = check(
            "changes.requests.list",
            json!({"status": "open", "allDevices": false}),
        );
        assert_eq!(list.unwrap().params, json!({"status": "open"}));
    }

    #[test]
    fn forks_and_running_forks_are_capped_and_only_the_two_lists_are_asked_for() {
        let fork = json!({"computerId": "greedy", "newName": "x"});
        assert!(authorize("greedy", &policy(&[]), "checkpoints.fork", &fork, &World).is_err());
        let fork = json!({"computerId": "busy", "newName": "x"});
        assert!(authorize("busy", &policy(&[]), "checkpoints.fork", &fork, &World).is_err());
        let list = check("computers.list", json!({"refreshRepositories": true})).unwrap();
        assert_eq!(list.params, json!({}));
    }

    #[test]
    fn lists_are_cut_to_the_caller_and_its_forks() {
        let mut computers = json!({
            "extra": "dropped",
            "linux": {"other": 1, "computers": [
                {"configuration": {"id": "main"}}, {"configuration": {"id": "fork-1"}},
                {"configuration": {"id": "stranger"}}]},
            "macos": {"computers": [{"id": "stranger"}]}
        });
        filter_result(Filter::Computers, &mut computers, "main", &World);
        assert_eq!(computers["linux"]["computers"].as_array().unwrap().len(), 2);
        assert!(computers["macos"]["computers"]
            .as_array()
            .unwrap()
            .is_empty());
        let mut requests = json!({"requests": [
            {"request": {"forkId": "fork-2"}}, {"request": {"forkId": "stranger"}}]});
        filter_result(Filter::Requests, &mut requests, "main", &World);
        assert_eq!(requests["requests"].as_array().unwrap().len(), 1);
    }
}
