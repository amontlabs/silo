//! Who is calling the engine, and what that caller may do.
//!
//! Every request carries a [`Caller`], set by the transport it arrived on and never by the
//! client. [`decide`] answers each call: the owner may do anything; another device may call the
//! methods the Connections bridge serves; the agent inside a computer may do what gives it no
//! more than it already has ([`rule`]), anything that widens what it has asks the owner, and
//! a computer outside the ones it made is never its to touch. `docs/SiloUI-POLICY.md` explains
//! the rules in plain words.

pub mod access;
mod rules;
pub mod store;
#[cfg(test)]
mod tests;

use std::collections::{BTreeSet, HashSet};

use serde_json::{json, Value};
use silo_api::{
    methods::{audit::AuditCallerKind, guest_client::GUEST_TOKEN_PATH, MethodName},
    transport::ApprovalKind,
};

pub use access::{Access, GitHubAccess};
pub use rules::{rule, Rule};

/// Who made a request. Only the transport sets it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Caller {
    /// A program on this device, through the engine's own socket. `client` is the name it gave
    /// in the handshake; it is diagnostic only.
    Owner { client: String },
    /// Another device, through Connections. `key` is the fingerprint of the SSH key it
    /// connected with, when the bridge could tell.
    Device { key: Option<String> },
    /// The agent inside the computer `id`.
    Computer { id: String },
}

impl Caller {
    pub fn owner(client: impl Into<String>) -> Self {
        Self::Owner {
            client: client.into(),
        }
    }

    pub fn kind(&self) -> AuditCallerKind {
        match self {
            Self::Owner { .. } => AuditCallerKind::Owner,
            Self::Device { .. } => AuditCallerKind::Device,
            Self::Computer { .. } => AuditCallerKind::Computer,
        }
    }

    /// The computer's id, or the device's key fingerprint.
    pub fn id(&self) -> Option<&str> {
        match self {
            Self::Owner { .. } => None,
            Self::Device { key } => key.as_deref(),
            Self::Computer { id } => Some(id),
        }
    }

    pub fn computer(&self) -> Option<&str> {
        match self {
            Self::Computer { id } => Some(id),
            _ => None,
        }
    }
}

/// What the policy needs to know about the device to judge a computer's call.
pub trait World {
    /// Every computer `creator` made, directly or through a computer it made.
    fn made(&self, creator: &str) -> HashSet<String>;
    /// The computer `computer` was forked from, as the tree shows it.
    fn parent(&self, computer: &str) -> Option<String>;
    /// A computer's name, for the summary of a request.
    fn name(&self, computer: &str) -> Option<String>;
    /// Whether `caller` started the job.
    fn owns_job(&self, caller: &str, job_id: &str) -> bool;
    /// The fork a review request is about.
    fn request_fork(&self, request_id: &str) -> Option<String>;
    /// The secrets and GitHub access a computer holds.
    fn access(&self, computer: &str) -> Access;
    /// Each computer's GitHub policy as `github.state` lists it, keyed by `computerId`; `None`
    /// when it could not be read.
    fn github_policies(&self) -> Option<Vec<Value>>;
    /// A fresh computer id.
    fn new_id(&self) -> String;
}

/// How a result is cut before it goes back to a computer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    None,
    /// `computers.list`: only the two lists of computers, cut to the visible ones.
    Computers,
    /// `computers.tree`: nodes cut to the visible ones.
    Tree,
    /// `changes.requests.list`: requests about visible forks.
    Requests,
}

/// A computer a call makes, recorded as the caller's once it exists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Creation {
    pub name: String,
    /// The id the engine chose for it, when the call names it.
    pub id: Option<String>,
    /// The computer a fork is made from.
    pub source: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Allowed {
    /// The parameters to run instead of the caller's, when they were rewritten.
    pub params: Option<Value>,
    pub filter: Filter,
    /// The computers the caller may see, for a handler that cuts its own answer (`audit.list`).
    pub visible: Option<BTreeSet<String>>,
    pub creates: Option<Creation>,
}

impl Allowed {
    fn plain() -> Self {
        Self {
            params: None,
            filter: Filter::None,
            visible: None,
            creates: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Decision {
    Allow(Allowed),
    /// The call widens what the caller has; the owner decides.
    Ask {
        kind: ApprovalKind,
        summary: String,
    },
    Refuse {
        reason: String,
    },
}

fn allow() -> Decision {
    Decision::Allow(Allowed::plain())
}

fn allow_with(params: Value) -> Decision {
    Decision::Allow(Allowed {
        params: Some(params),
        ..Allowed::plain()
    })
}

fn refuse(reason: impl Into<String>) -> Decision {
    Decision::Refuse {
        reason: reason.into(),
    }
}

fn ask(kind: ApprovalKind, summary: impl Into<String>) -> Decision {
    Decision::Ask {
        kind,
        summary: summary.into(),
    }
}

/// Answers one call.
pub fn decide(caller: &Caller, method: MethodName, params: &Value, world: &dyn World) -> Decision {
    match caller {
        Caller::Owner { .. } => allow(),
        // The Connections bridge's lists decide what another device may call.
        Caller::Device { .. } if crate::remote::is_owner_method(method.name()) => allow(),
        Caller::Device { .. } => refuse(format!(
            "Another device cannot call {} on this one.",
            method.name()
        )),
        Caller::Computer { id } => Judge::new(id, world).decide(method, params),
    }
}

fn text<'a>(params: &'a Value, key: &str) -> Option<&'a str> {
    params
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
}

/// A request for another device is not served for a computer.
fn local_only(params: &Value) -> Result<(), String> {
    let elsewhere = "A computer's agent reaches this device's engine only.";
    let routes_elsewhere = params
        .get("allDevices")
        .is_some_and(|value| !matches!(value, Value::Null | Value::Bool(false)))
        || params
            .get("deviceIds")
            .is_some_and(|value| !matches!(value, Value::Null) && value != &json!([]));
    if routes_elsewhere {
        return Err(elsewhere.into());
    }
    match params.get("deviceId") {
        None | Some(Value::Null) => Ok(()),
        Some(Value::String(device)) if device == "local" => Ok(()),
        Some(_) => Err(elsewhere.into()),
    }
}

/// One computer's call, judged against what it made.
struct Judge<'a> {
    caller: &'a str,
    world: &'a dyn World,
    made: HashSet<String>,
}

impl<'a> Judge<'a> {
    fn new(caller: &'a str, world: &'a dyn World) -> Self {
        let mut made = world.made(caller);
        made.remove(caller);
        Self {
            caller,
            world,
            made,
        }
    }

    fn visible(&self) -> BTreeSet<String> {
        self.made
            .iter()
            .cloned()
            .chain(std::iter::once(self.caller.to_owned()))
            .collect()
    }

    fn in_tree(&self, id: &str) -> bool {
        id == self.caller || self.made.contains(id)
    }

    fn name(&self, id: &str) -> String {
        self.world.name(id).unwrap_or_else(|| id.to_owned())
    }

    /// The computer in `key`, which must be the caller or one it made.
    fn tree_target(&self, params: &Value, key: &str) -> Result<String, String> {
        match text(params, key) {
            Some(id) if self.in_tree(id) => Ok(id.to_owned()),
            Some(_) => Err("That computer is not this one or one it made.".into()),
            None => Err(format!("The call must name a computer ({key}).")),
        }
    }

    /// The computer in `key`, which must be one the caller made.
    fn made_target(&self, params: &Value, key: &str) -> Result<String, String> {
        match text(params, key) {
            Some(id) if id == self.caller => Err(
                "A computer's agent cannot do this to its own computer, only to computers it made."
                    .into(),
            ),
            Some(id) if self.made.contains(id) => Ok(id.to_owned()),
            Some(_) => Err("That computer is not one this computer made.".into()),
            None => Err(format!("The call must name a computer ({key}).")),
        }
    }

    fn request_target(&self, params: &Value) -> Result<String, String> {
        text(params, "requestId")
            .and_then(|id| self.world.request_fork(id))
            .ok_or_else(|| "That review request was not found.".to_owned())
    }

    fn decide(&self, method: MethodName, params: &Value) -> Decision {
        let params = match params {
            Value::Null => json!({}),
            Value::Object(_) => params.clone(),
            _ => return refuse("The parameters must be an object."),
        };
        if let Err(reason) = local_only(&params) {
            return refuse(reason);
        }
        let decided = match rule(method) {
            Rule::Open => self.open(method, params),
            Rule::OwnJob => self.own_job(&params),
            Rule::Tree => self.tree(method, params),
            Rule::Made => self.made(method, &params),
            Rule::Create => self.create(method, &params),
            Rule::Ask(kind) => self.ask(method, kind, &params),
            Rule::OwnerOnly => Err(format!(
                "{} is for the person who runs Silo; a computer's agent cannot call it.",
                method.name()
            )),
        };
        decided.unwrap_or_else(refuse)
    }

    fn open(&self, method: MethodName, params: Value) -> Result<Decision, String> {
        if method == MethodName::AuditList {
            return Ok(Decision::Allow(Allowed {
                params: Some(params),
                visible: Some(self.visible()),
                ..Allowed::plain()
            }));
        }
        Ok(allow())
    }

    fn own_job(&self, params: &Value) -> Result<Decision, String> {
        match text(params, "jobId") {
            Some(job) if self.world.owns_job(self.caller, job) => Ok(allow()),
            _ => Err("That job was not started by this computer's agent.".into()),
        }
    }

    fn tree(&self, method: MethodName, params: Value) -> Result<Decision, String> {
        let filtered = |params: Value, filter: Filter| {
            Ok(Decision::Allow(Allowed {
                params: Some(params),
                filter,
                ..Allowed::plain()
            }))
        };
        match method {
            // Only the lists are asked for; nothing else the caller sends is passed on.
            MethodName::ComputersList => filtered(json!({}), Filter::Computers),
            MethodName::ComputersTree => filtered(json!({}), Filter::Tree),
            MethodName::ChangesRequestsList => {
                let list = match params.get("status") {
                    None | Some(Value::Null) => json!({}),
                    Some(status) => json!({ "status": status }),
                };
                filtered(list, Filter::Requests)
            }
            MethodName::ChangesRequestsShow => {
                let fork = self.request_target(&params)?;
                if !self.in_tree(&fork) {
                    return Err("That review request is not about a computer this one made.".into());
                }
                Ok(allow_with(params))
            }
            _ => {
                self.tree_target(&params, "computerId")?;
                Ok(allow_with(params))
            }
        }
    }

    fn made(&self, method: MethodName, params: &Value) -> Result<Decision, String> {
        match method {
            MethodName::ChangesRequest => {
                self.made_target(params, "forkId")?;
                if let Some(target) = text(params, "targetId") {
                    if !self.in_tree(target) {
                        return Err(
                            "A review request can target only this computer or one it made."
                                .into(),
                        );
                    }
                }
            }
            MethodName::ChangesRequestsThrowAway => {
                let fork = self.request_target(params)?;
                if !self.made.contains(&fork) {
                    return Err("That review request is not about a computer this one made.".into());
                }
            }
            _ => {
                self.made_target(params, "computerId")?;
            }
        }
        Ok(allow_with(params.clone()))
    }

    fn create(&self, method: MethodName, params: &Value) -> Result<Decision, String> {
        match method {
            MethodName::CheckpointsFork => self.fork(params),
            MethodName::ComputersCreate => self.new_computer(params),
            other => Err(format!("{} is not a call that makes a computer.", other.name())),
        }
    }

    fn checked_name<'p>(&self, name: Option<&'p str>) -> Result<&'p str, String> {
        let name = name.unwrap_or_default();
        if name.trim().is_empty() {
            return Err("A new computer needs a name.".into());
        }
        if crate::computer_tree::hidden(name) {
            return Err("That name is reserved for Silo's own computers.".into());
        }
        Ok(name)
    }

    fn fork(&self, params: &Value) -> Result<Decision, String> {
        let source = text(params, "computerId")
            .filter(|id| self.in_tree(id))
            .ok_or("A computer's agent can fork its own computer or a computer it made.")?;
        let new_name = self.checked_name(text(params, "newName"))?;
        let named: Vec<String> = match params.get("secrets") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(names)) => names
                .iter()
                .map(|name| name.as_str().map(str::to_owned))
                .collect::<Option<_>>()
                .ok_or("Each secret is named by a string.")?,
            Some(_) => return Err("The secrets are a list of names.".into()),
        };
        // The fork's quiet list keeps what the caller asked for, and the token file of the
        // in-guest client always goes. The recipe hash names a baseline other forks share, so
        // a computer cannot set it.
        let quiet = params.get("quiet").filter(|quiet| !quiet.is_null());
        let list = |key: &str| -> Result<Vec<Value>, String> {
            match quiet.and_then(|quiet| quiet.get(key)) {
                None | Some(Value::Null) => Ok(Vec::new()),
                Some(Value::Array(items)) if items.iter().all(Value::is_string) => {
                    Ok(items.clone())
                }
                Some(_) => Err(format!("quiet.{key} is a list of strings.")),
            }
        };
        let stop = list("stop")?;
        let mut scrub = list("scrub")?;
        if !scrub.iter().any(|path| path == GUEST_TOKEN_PATH) {
            scrub.push(json!(GUEST_TOKEN_PATH));
        }
        let mut rewritten = json!({
            "computerId": source,
            "newName": new_name,
            "quiet": { "stop": stop, "scrub": scrub },
        });
        if !named.is_empty() {
            rewritten["secrets"] = json!(named);
        }
        let checkpoint = text(params, "checkpointId");
        if let Some(checkpoint) = checkpoint {
            rewritten["checkpointId"] = json!(checkpoint);
        }
        let creates = Creation {
            name: new_name.to_owned(),
            id: None,
            source: Some(source.to_owned()),
        };
        let mut wider: Vec<String> = Vec::new();
        // An earlier state of the caller can hold what the owner has since taken away.
        if source == self.caller && checkpoint.is_some() {
            wider.push("an earlier checkpoint of this computer".into());
        }
        let own = self.world.access(self.caller);
        let from = (source != self.caller).then(|| self.world.access(source));
        for name in &named {
            if name == "github" {
                let copied = match &from {
                    None => own.github.clone(),
                    Some(from) => from.github.clone(),
                };
                let covered = match (&own.github, &copied) {
                    (Some(own), Some(copied)) => own.covers(copied),
                    _ => false,
                };
                if !covered {
                    wider.push(format!("the GitHub access of {}", self.name(source)));
                }
            } else if !own.secrets.contains(name) {
                wider.push(format!("the secret {name}"));
            }
        }
        if !wider.is_empty() {
            return Ok(ask(
                ApprovalKind::Access,
                format!(
                    "Fork {} as {new_name} with {}, which {} does not have.",
                    self.name(source),
                    wider.join(" and "),
                    self.name(self.caller),
                ),
            ));
        }
        Ok(Decision::Allow(Allowed {
            params: Some(rewritten),
            creates: Some(creates),
            ..Allowed::plain()
        }))
    }

    fn new_computer(&self, params: &Value) -> Result<Decision, String> {
        let mut rewritten = params.clone();
        match params.get("kind").and_then(Value::as_str) {
            Some("linux") => {
                let configuration = params
                    .get("configuration")
                    .filter(|value| value.is_object())
                    .ok_or("A Linux computer needs its configuration.")?;
                let name = self.checked_name(text(configuration, "name"))?;
                // The engine names the new computer, so the call cannot reuse an existing id.
                let id = self.world.new_id();
                rewritten["configuration"]["id"] = json!(id);
                let creates = Creation {
                    name: name.to_owned(),
                    id: Some(id),
                    source: None,
                };
                if configuration.get("runtime").and_then(Value::as_str) == Some("container") {
                    return Ok(ask(
                        ApprovalKind::Container,
                        format!(
                            "Make {name}, a container computer: it shares this device's kernel, so it is less isolated than a virtual machine."
                        ),
                    ));
                }
                Ok(Decision::Allow(Allowed {
                    params: Some(rewritten),
                    creates: Some(creates),
                    ..Allowed::plain()
                }))
            }
            Some("macos") => {
                let name =
                    self.checked_name(params.get("request").and_then(|r| text(r, "name")))?;
                Ok(Decision::Allow(Allowed {
                    params: Some(rewritten),
                    creates: Some(Creation {
                        name: name.to_owned(),
                        id: None,
                        source: None,
                    }),
                    ..Allowed::plain()
                }))
            }
            _ => Err("A new computer is Linux or macOS (kind).".into()),
        }
    }

    fn ask(
        &self,
        method: MethodName,
        kind: ApprovalKind,
        params: &Value,
    ) -> Result<Decision, String> {
        match method {
            MethodName::ChangesKeep => {
                let fork = self.made_target(params, "forkId")?;
                let target = match text(params, "targetId") {
                    Some(target) => target.to_owned(),
                    None => self
                        .world
                        .parent(&fork)
                        .ok_or("That fork has no computer to merge into; name one (targetId).")?,
                };
                if self.made.contains(&target) {
                    return Ok(allow_with(params.clone()));
                }
                if target != self.caller {
                    return Err("A merge can go only into this computer or one it made.".into());
                }
                Ok(ask(
                    kind,
                    format!(
                        "Merge the changes of {} into {}.",
                        self.name(&fork),
                        self.name(&target)
                    ),
                ))
            }
            MethodName::ChangesRequestsKeep => {
                let fork = self.request_target(params)?;
                if !self.made.contains(&fork) {
                    return Err("That review request is not about a computer this one made.".into());
                }
                Ok(ask(
                    kind,
                    format!(
                        "Merge the changes of {} that its review request lists.",
                        self.name(&fork)
                    ),
                ))
            }
            MethodName::ChangesUndo => {
                let target = self.tree_target(params, "computerId")?;
                if self.made.contains(&target) {
                    return Ok(allow_with(params.clone()));
                }
                Ok(ask(
                    kind,
                    format!("Undo the last merge into {}.", self.name(&target)),
                ))
            }
            MethodName::ChangesPromote => {
                let fork = self.made_target(params, "forkId")?;
                let target = text(params, "targetId")
                    .map(str::to_owned)
                    .or_else(|| self.world.parent(&fork))
                    .filter(|target| self.in_tree(target))
                    .ok_or("A fork can replace only this computer or one it made.")?;
                Ok(ask(
                    kind,
                    format!(
                        "Replace {} with its fork {}. This restarts {}, and what it did since the fork is set aside.",
                        self.name(&target),
                        self.name(&fork),
                        self.name(&target)
                    ),
                ))
            }
            MethodName::NetworkPublish => {
                let target = self.tree_target(params, "computerId")?;
                let port = params.get("port").and_then(Value::as_u64).unwrap_or(0);
                Ok(ask(
                    kind,
                    format!(
                        "Publish port {port} of {} on this device.",
                        self.name(&target)
                    ),
                ))
            }
            MethodName::SshAccessSave => {
                let target = self.tree_target(params, "computerId")?;
                // Turning SSH access off only narrows it.
                if params.get("enabled") == Some(&Value::Bool(false)) {
                    return Ok(allow_with(params.clone()));
                }
                let port = params.get("port").and_then(Value::as_u64).unwrap_or(0);
                Ok(ask(
                    kind,
                    format!(
                        "Turn on SSH access to {} from outside, on port {port}.",
                        self.name(&target)
                    ),
                ))
            }
            MethodName::PushStart => {
                let target = self.tree_target(params, "computerId")?;
                let path = text(params, "repositoryPath").unwrap_or("a repository");
                Ok(ask(
                    kind,
                    format!(
                        "Push {path} from {} to GitHub through this device.",
                        self.name(&target)
                    ),
                ))
            }
            MethodName::GitHubConfigurationSave => self.github_configuration(kind, params),
            MethodName::SecretsSave => {
                let request = params.get("request").ok_or("The secret is missing.")?;
                if request.get("operation").and_then(Value::as_str) != Some("add") {
                    return Err(
                        "Only the person who runs Silo changes a saved secret; a computer's agent can ask to add one."
                            .into(),
                    );
                }
                let name = text(request, "name").ok_or("A secret needs a name.")?;
                let computers: Vec<&str> = request
                    .get("computerIds")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect();
                if computers.iter().any(|id| !self.in_tree(id)) {
                    return Err(
                        "A secret can be asked for only for this computer or computers it made."
                            .into(),
                    );
                }
                let names: Vec<String> = computers.iter().map(|id| self.name(id)).collect();
                let any = request.get("allowAnyDestination") == Some(&Value::Bool(true));
                let domains: Vec<&str> = request
                    .get("allowedDomains")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect();
                let sent = if any {
                    "to any destination".to_owned()
                } else {
                    format!("only to {}", domains.join(", "))
                };
                Ok(ask(
                    kind,
                    format!(
                        "Add the secret {name} for {}, sent {sent}.",
                        if names.is_empty() {
                            "no computer".to_owned()
                        } else {
                            names.join(", ")
                        }
                    ),
                ))
            }
            other => Err(format!(
                "{} has no rule for a computer's agent yet.",
                other.name()
            )),
        }
    }

    /// A GitHub configuration replaces every computer's policy, so it may change only the
    /// caller's and those of computers it made.
    fn github_configuration(&self, kind: ApprovalKind, params: &Value) -> Result<Decision, String> {
        let current = self
            .world
            .github_policies()
            .ok_or("GitHub access cannot be read now; try again.")?;
        let proposed: Vec<Value> = params
            .pointer("/configuration/computers")
            .and_then(Value::as_array)
            .cloned()
            .ok_or("The GitHub configuration lists no computers.")?;
        let by_id = |list: &[Value]| -> std::collections::BTreeMap<String, Value> {
            list.iter()
                .filter_map(|policy| {
                    policy
                        .get("computerId")
                        .and_then(Value::as_str)
                        .map(|id| (id.to_owned(), policy.clone()))
                })
                .collect()
        };
        let (before, after) = (by_id(&current), by_id(&proposed));
        let changed: BTreeSet<&String> = before
            .keys()
            .chain(after.keys())
            .filter(|id| before.get(*id) != after.get(*id))
            .collect();
        if changed.iter().any(|id| !self.in_tree(id)) {
            return Err(
                "A computer's agent can ask for GitHub access only for its own computer or computers it made."
                    .into(),
            );
        }
        if changed.is_empty() {
            return Ok(allow_with(params.clone()));
        }
        let names: Vec<String> = changed.iter().map(|id| self.name(id)).collect();
        Ok(ask(
            kind,
            format!("Change the GitHub access of {}.", names.join(", ")),
        ))
    }
}

/// Cuts a result down to the computers in `visible`.
pub fn filter_result(filter: Filter, result: &mut Value, visible: &BTreeSet<String>) {
    match filter {
        Filter::None => {}
        Filter::Computers => {
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
                // A visible node does not name a parent the caller cannot see.
                for node in nodes.iter_mut() {
                    let hidden = node
                        .get("parent")
                        .and_then(Value::as_str)
                        .is_some_and(|parent| !visible.contains(parent));
                    if hidden {
                        node["parent"] = Value::Null;
                    }
                }
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

/// The computer a call names, for the audit log.
pub fn target_computer(params: &Value) -> Option<String> {
    ["computerId", "forkId"]
        .iter()
        .find_map(|key| text(params, key))
        .or_else(|| {
            params
                .pointer("/configuration/id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
        })
        .map(str::to_owned)
}
