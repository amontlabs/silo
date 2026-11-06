//! What a guest client's token may ask the engine for. Every method not listed here is
//! refused, whatever its parameters. Each listed method also names the computer it acts on,
//! and that computer must be the caller or one of its forks, as the case needs.

use std::collections::HashSet;

use serde_json::{json, Value};
use silo_api::methods::guest_client::{GuestClientPolicy, GUEST_TOKEN_PATH};

/// What the scope check needs to know about the machine.
pub trait Topology {
    /// Every computer made from `computer`, directly or through other forks (not `computer`).
    fn descendants(&self, computer: &str) -> HashSet<String>;
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
    "checkpoints.create",
    "checkpoints.fork",
    "changes.capture",
    "changes.baseline",
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
    if !ALLOWED.contains(&method) {
        return refuse(format!(
            "{method} is not available to a computer's own agent. Ask the person who runs Silo."
        ));
    }
    if !params.is_object() && !params.is_null() {
        return refuse("The parameters must be an object.");
    }
    let params = if params.is_null() { json!({}) } else { params.clone() };
    local_only(&params)?;
    let mut allowed = |params: Value, filter: Filter, starts_job: bool| {
        Ok(Authorized {
            params,
            filter,
            starts_job,
        })
    };
    let forks = topology.descendants(caller);
    match method {
        "computers.list" => allowed(params, Filter::Computers, false),
        "computers.tree" => allowed(params, Filter::Tree, false),
        "computers.start" | "computers.stop" | "computers.credentials" => {
            fork_of_caller(&params, "computerId", &forks, "That computer")?;
            allowed(params, Filter::None, method != "computers.credentials")
        }
        "changes.capture" | "changes.baseline" | "changes.diff" => {
            fork_of_caller(&params, "computerId", &forks, "That computer")?;
            allowed(params, Filter::None, true)
        }
        "steps.list" => {
            if text(&params, "computerId") != Some(caller) {
                fork_of_caller(&params, "computerId", &forks, "That computer")?;
            }
            allowed(params, Filter::None, false)
        }
        "checkpoints.create" => {
            if text(&params, "computerId") != Some(caller) {
                return refuse("A checkpoint can be made of this computer only.");
            }
            allowed(params, Filter::None, true)
        }
        "checkpoints.fork" => {
            if text(&params, "computerId") != Some(caller) {
                return refuse("A fork can be made from this computer only.");
            }
            fork_rewrite(params, policy)
        }
        "changes.request" => {
            fork_of_caller(&params, "forkId", &forks, "That fork")?;
            if text(&params, "targetId").is_some_and(|target| target != caller) {
                return refuse("A review request can name only this computer as its target.");
            }
            allowed(params, Filter::None, false)
        }
        "changes.requests.list" => allowed(params, Filter::Requests, false),
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
                Some(job) if topology.owns_job(caller, job) => {
                    allowed(params, Filter::None, false)
                }
                _ => refuse("That job was not started by this computer's client."),
            }
        }
    }
}

/// A fork made from inside gets no secret or GitHub access the settings did not name, and
/// loses the token file.
fn fork_rewrite(mut params: Value, policy: &GuestClientPolicy) -> Result<Authorized, Refusal> {
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
    let object = params.as_object_mut().expect("checked to be an object");
    let quiet = object
        .entry("quiet")
        .or_insert_with(|| json!({ "stop": [], "scrub": [] }));
    let quiet = quiet
        .as_object_mut()
        .ok_or_else(|| Refusal("quiet must be an object.".into()))?;
    let scrub = quiet.entry("scrub").or_insert_with(|| json!([]));
    let scrub = scrub
        .as_array_mut()
        .ok_or_else(|| Refusal("quiet.scrub must be a list.".into()))?;
    if !scrub.iter().any(|path| path == GUEST_TOKEN_PATH) {
        scrub.push(json!(GUEST_TOKEN_PATH));
    }
    Ok(Authorized {
        params,
        filter: Filter::None,
        starts_job: true,
    })
}

/// Cuts a result down to the caller and its forks.
pub fn filter_result(filter: Filter, result: &mut Value, caller: &str, topology: &dyn Topology) {
    let mut visible = topology.descendants(caller);
    visible.insert(caller.to_owned());
    match filter {
        Filter::None => {}
        Filter::Computers => {
            for (list, id) in [("linux", "/configuration/id"), ("macos", "/id")] {
                if let Some(computers) = result
                    .get_mut(list)
                    .and_then(|group| group.get_mut("computers"))
                    .and_then(Value::as_array_mut)
                {
                    computers.retain(|computer| {
                        computer
                            .pointer(id)
                            .and_then(Value::as_str)
                            .is_some_and(|found| visible.contains(found))
                    });
                }
            }
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
        fn descendants(&self, computer: &str) -> HashSet<String> {
            if computer == "main" {
                ["fork-1", "fork-2"].map(String::from).into()
            } else {
                HashSet::new()
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
            fold: "ask".into(),
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
            let refusal = check(method, json!({"computerId": "fork-1", "forkId": "fork-1"}))
                .unwrap_err();
            assert!(refusal.0.contains(method), "{method}: {}", refusal.0);
        }
    }

    #[test]
    fn a_fork_of_self_is_allowed_and_another_computer_is_refused() {
        assert!(check("checkpoints.fork", json!({"computerId": "main", "newName": "x"})).is_ok());
        assert!(check("checkpoints.fork", json!({"computerId": "fork-1", "newName": "x"})).is_err());
        assert!(check("checkpoints.fork", json!({"computerId": "stranger", "newName": "x"})).is_err());
        assert!(check("checkpoints.create", json!({"computerId": "fork-1", "name": "x"})).is_err());
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
        assert!(check("changes.request", json!({"forkId": "fork-1", "targetId": "main"})).is_ok());
        assert!(check("changes.request", json!({"forkId": "fork-1", "targetId": "other"})).is_err());
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
        assert!(authorize("fork-1", &policy(&[]), "operations.job.get", &json!({"jobId": "job:mine"}), &World).is_err());
    }

    #[test]
    fn a_fork_loses_the_token_file_and_gets_only_the_secrets_the_settings_name() {
        let params = json!({"computerId": "main", "newName": "x", "quiet": {"stop": ["poller"]}});
        let granted = check("checkpoints.fork", params).unwrap();
        assert_eq!(granted.params["quiet"]["stop"], json!(["poller"]));
        assert_eq!(granted.params["quiet"]["scrub"], json!([GUEST_TOKEN_PATH]));
        assert!(granted.starts_job);
        let with_secret = json!({"computerId": "main", "newName": "x", "secrets": ["github"]});
        assert!(check("checkpoints.fork", with_secret.clone()).is_err());
        let allowed = authorize("main", &policy(&["github"]), "checkpoints.fork", &with_secret, &World);
        assert!(allowed.is_ok());
    }

    #[test]
    fn lists_are_cut_to_the_caller_and_its_forks() {
        let mut computers = json!({
            "linux": {"computers": [
                {"configuration": {"id": "main"}}, {"configuration": {"id": "fork-1"}},
                {"configuration": {"id": "stranger"}}]},
            "macos": {"computers": [{"id": "stranger"}]}
        });
        filter_result(Filter::Computers, &mut computers, "main", &World);
        assert_eq!(computers["linux"]["computers"].as_array().unwrap().len(), 2);
        assert!(computers["macos"]["computers"].as_array().unwrap().is_empty());
        let mut requests = json!({"requests": [
            {"request": {"forkId": "fork-2"}}, {"request": {"forkId": "stranger"}}]});
        filter_result(Filter::Requests, &mut requests, "main", &World);
        assert_eq!(requests["requests"].as_array().unwrap().len(), 1);
    }
}
