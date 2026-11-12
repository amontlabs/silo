//! Who is calling the engine, and what that caller may do.
//!
//! Every request carries a [`Caller`], set by the transport it arrived on and never by the
//! client. [`decide`] answers each call: the owner may do anything; another device may call the
//! methods the Connections bridge serves; the agent inside a computer may do what gives it no
//! more than it already has ([`rule`]), anything that widens what it has asks the owner, and
//! a computer outside the ones it made is never its to touch. `docs/SiloUI-POLICY.md` explains
//! the rules in plain words.

pub mod access;
pub mod handoff;
#[cfg(test)]
mod network_tests;
pub mod resources;
mod rules;
pub mod store;
mod summary;
#[cfg(test)]
mod tests;

use std::collections::{BTreeSet, HashSet};

use serde_json::{json, Value};
use silo_api::{
    methods::{audit::AuditCallerKind, guest_client::GUEST_ENDPOINT_PATH, MethodName},
    plain_text,
    transport::ApprovalKind,
};

use crate::network_policy::{Grants, Held, NetworkPolicy, NetworkPolicyMode, GITHUB_GRANT_PIN};
pub use access::{Access, GitHubAccess};
pub use resources::Resources;
pub use rules::{rule, Rule};
use summary::{github_access, github_identity, github_writes, listed, quoted};

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
    /// The computer a review request would merge its fork into.
    fn request_target(&self, request_id: &str) -> Option<String>;
    /// The secrets and GitHub access a computer holds.
    fn access(&self, computer: &str) -> Access;
    /// Each computer's GitHub policy as `github.state` lists it, keyed by `computerId`; `None`
    /// when it could not be read.
    fn github_policies(&self) -> Option<Vec<Value>>;
    /// The name of the GitHub token secret `id`; `None` when there is no such token.
    fn github_token_name(&self, _id: &str) -> Option<String> {
        None
    }
    /// A fresh computer id.
    fn new_id(&self) -> String;
    /// The sizes of a computer; `None` when they cannot be read.
    fn resources(&self, computer: &str) -> Option<Resources>;
    /// The port and bind address saved for a computer's SSH access, when it has any.
    fn ssh_settings(&self, computer: &str) -> Option<(u16, String)>;
    /// The network policy a computer runs with; `None` when it cannot be read, so a check
    /// against it asks.
    fn network_policy(&self, _computer: &str) -> Option<NetworkPolicy> {
        None
    }
    /// The policy a computer runs with and the preset versions it was approved at, which must
    /// be the answer `network.policy.get` gives for that computer; `None` when it cannot be
    /// read, so a check against it asks.
    fn network_held(&self, computer: &str) -> Option<Held> {
        self.network_policy(computer).map(Held::latest)
    }
    /// The public keys saved for a computer's SSH access.
    fn ssh_keys(&self, _computer: &str) -> Vec<String> {
        Vec::new()
    }
    /// The recipe record of a computer, when it has one.
    fn recipe_record(
        &self,
        computer: &str,
    ) -> Result<Option<silo_api::methods::recipes::RecipeRecord>, String> {
        crate::recipes::records::get(computer)
    }
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
    /// The call widens what the caller has; the owner decides. On allow the engine runs
    /// `params` when the policy rewrote them, else the caller's, and records `creates` as the
    /// caller's.
    Ask {
        kind: ApprovalKind,
        summary: String,
        /// What the computer that asked may read of `summary`, when `summary` holds what only
        /// the owner may see.
        computer_summary: Option<String>,
        params: Option<Value>,
        creates: Option<Creation>,
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
        computer_summary: None,
        params: None,
        creates: None,
    }
}

/// An ask for a merge, whose owner-facing text holds the commands of a recipe's hooks and the
/// computer's holds only their number. The computer's text is always stored, so an approval
/// saved without one is known to predate it.
fn ask_merge(kind: ApprovalKind, summary: String, computer_summary: String) -> Decision {
    Decision::Ask {
        kind,
        computer_summary: Some(computer_summary),
        summary,
        params: None,
        creates: None,
    }
}

/// An ask for a call that runs with other parameters than the caller sent.
fn ask_with(kind: ApprovalKind, summary: impl Into<String>, params: Value) -> Decision {
    Decision::Ask {
        kind,
        summary: summary.into(),
        computer_summary: None,
        params: Some(params),
        creates: None,
    }
}

/// An ask for a call that makes a computer: what runs on allow and the computer it makes.
fn ask_to_create(
    kind: ApprovalKind,
    summary: impl Into<String>,
    params: Value,
    creates: Creation,
) -> Decision {
    Decision::Ask {
        kind,
        summary: summary.into(),
        computer_summary: None,
        params: Some(params),
        creates: Some(creates),
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

/// `words` as the start of a sentence: a command in code quotes, a phrase with a capital.
fn sentence_start(words: &str) -> String {
    if words.starts_with("silo ") {
        return format!("`{words}`");
    }
    let mut chars = words.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

/// The longest name a computer made by an agent may have.
const NAME_MAX_CHARS: usize = 64;

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

    /// A computer's name as a summary shows it: quoted, plain and bounded.
    fn shown(&self, id: &str) -> String {
        quoted(&self.name(id))
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
            Rule::OwnerOnly => Err(match rules::command_words(method) {
                Some(command) => format!(
                    "{} is for the person who runs Silo; a computer's agent cannot do it.",
                    sentence_start(command)
                ),
                None => {
                    "This is for the person who runs Silo; a computer's agent cannot do it.".into()
                }
            }),
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
            // Retention would let the caller evict the owner's automatic and step checkpoints of
            // its own computer, so there it makes a manual checkpoint that is never trimmed.
            MethodName::CheckpointsCreate => {
                let target = self.tree_target(&params, "computerId")?;
                let mut params = params;
                if target == self.caller {
                    params["kind"] = json!("manual");
                    if let Some(object) = params.as_object_mut() {
                        object.remove("keepNewest");
                    }
                }
                Ok(allow_with(params))
            }
            // The status says whether the tools are on and how they fared; it is read for the
            // computer asking and for no other.
            MethodName::GuestClientStatus => {
                match text(&params, "computerId") {
                    Some(id) if id == self.caller => {}
                    Some(_) => return Err(
                        "A computer's agent can read the tools status of its own computer only."
                            .into(),
                    ),
                    None => return Err("The call must name a computer (computerId).".into()),
                }
                Ok(allow_with(json!({ "computerId": self.caller })))
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
                            "A review request can target only this computer or one it made.".into(),
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
            other => Err(format!(
                "{} is not a call that makes a computer.",
                other.name()
            )),
        }
    }

    /// A new computer's name: a Linux computer's is held to the runtime's rule; every name is
    /// plain text and not one Silo reserves.
    fn checked_name<'p>(&self, name: Option<&'p str>, linux: bool) -> Result<&'p str, String> {
        let name = name.unwrap_or_default();
        if name.trim().is_empty() {
            return Err("A new computer needs a name.".into());
        }
        if crate::computer_tree::hidden(name) {
            return Err("That name is reserved for Silo's own computers.".into());
        }
        if linux {
            crate::runtime::validate_name(name).map_err(|error| error.to_string())?;
        } else if name != name.trim()
            || plain_text(name) != name
            || name.chars().count() > NAME_MAX_CHARS
        {
            return Err(format!(
                "A computer's name is at most {NAME_MAX_CHARS} characters, with no control characters and no space around it."
            ));
        }
        Ok(name)
    }

    /// Whether `id` is the hidden computer a recipe was built in. It holds nothing but what the
    /// recipe installs, and its name is one no agent can give a computer, so any computer may
    /// fork it to make a computer from that recipe.
    fn is_recipe_build(&self, id: &str) -> bool {
        self.world
            .name(id)
            .is_some_and(|name| name.starts_with(crate::recipes::BUILDER_PREFIX))
    }

    fn fork(&self, params: &Value) -> Result<Decision, String> {
        let source = text(params, "computerId")
            .filter(|id| self.in_tree(id) || self.is_recipe_build(id))
            .ok_or("A computer's agent can fork its own computer or a computer it made.")?;
        let new_name = self.checked_name(text(params, "newName"), false)?;
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
        if !scrub.iter().any(|path| path == GUEST_ENDPOINT_PATH) {
            scrub.push(json!(GUEST_ENDPOINT_PATH));
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
        let mut secrets_wider: Vec<String> = Vec::new();
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
                    wider.push(format!("the GitHub access of {}", self.shown(source)));
                }
            } else if !own.secrets.contains(name) {
                secrets_wider.push(name.clone());
            }
        }
        match secrets_wider.as_slice() {
            [] => {}
            [one] => wider.push(format!("the secret {}", quoted(one))),
            many => wider.push(format!("the secrets {}", listed(many))),
        }
        let asked_network = Self::asked_network_policy(params, "networkPolicy")?;
        let network = self.new_computer_network(
            asked_network.clone(),
            self.world.network_held(source),
            named.iter().any(|name| name == "github"),
            new_name,
        );
        rewritten["networkPolicy"] = json!(Self::chosen_network(&network, asked_network));
        if !wider.is_empty() {
            if network.is_err() {
                wider.push("a network policy wider than its own".into());
            }
            let mut summary = format!(
                "More access than {} has: fork {} as {} with {}.",
                self.shown(self.caller),
                self.shown(source),
                quoted(new_name),
                wider.join(" and "),
            );
            if let Err(network) = &network {
                summary.push(' ');
                summary.push_str(network);
            }
            return Ok(ask_to_create(
                ApprovalKind::Access,
                summary,
                rewritten,
                creates,
            ));
        }
        if let Err(summary) = network {
            return Ok(ask_to_create(
                ApprovalKind::Network,
                summary,
                rewritten,
                creates,
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
                let name = self.checked_name(text(configuration, "name"), true)?;
                // The engine names the new computer, so the call cannot reuse an existing id.
                let id = self.world.new_id();
                rewritten["configuration"]["id"] = json!(id);
                // Whether the desktop starts is the only desktop choice a caller makes; Silo
                // decides whether the desktop is built in.
                match configuration.get("desktop") {
                    None | Some(Value::Null) => {
                        rewritten["configuration"]
                            .as_object_mut()
                            .map(|object| object.remove("desktop"));
                    }
                    Some(Value::Object(desktop)) => {
                        let mut kept = json!({});
                        match desktop.get("startWithComputer") {
                            None | Some(Value::Null) => {}
                            Some(Value::Bool(start)) => kept["startWithComputer"] = json!(start),
                            Some(_) => {
                                return Err(
                                    "The desktop's startWithComputer is true or false.".into()
                                )
                            }
                        }
                        rewritten["configuration"]["desktop"] = kept;
                    }
                    Some(_) => return Err("The desktop configuration is an object.".into()),
                }
                let creates = Creation {
                    name: name.to_owned(),
                    id: Some(id),
                    source: None,
                };
                let asked_network = Self::asked_network_policy(params, "networkPolicy")?;
                let network = self.new_computer_network(asked_network.clone(), None, false, name);
                rewritten["networkPolicy"] = json!(Self::chosen_network(&network, asked_network));
                let asked = Resources::of_linux(configuration)
                    .ok_or("A Linux computer's CPUs, memory and storage are numbers.")?;
                let container =
                    configuration.get("runtime").and_then(Value::as_str) == Some("container");
                if container {
                    let mut summary = format!(
                        "Less isolated: make {}, a container computer that shares this device's kernel with {}.",
                        quoted(name),
                        asked.describe()
                    );
                    if let Err(network) = &network {
                        summary.push(' ');
                        summary.push_str(network);
                    }
                    return Ok(ask_to_create(
                        ApprovalKind::Container,
                        summary,
                        rewritten,
                        creates,
                    ));
                }
                if let Err(summary) = network {
                    return Ok(ask_to_create(
                        ApprovalKind::Network,
                        summary,
                        rewritten,
                        creates,
                    ));
                }
                Ok(Decision::Allow(Allowed {
                    params: Some(rewritten),
                    creates: Some(creates),
                    ..Allowed::plain()
                }))
            }
            Some("macos") => {
                let request = params.get("request").ok_or("The request is missing.")?;
                let name = self.checked_name(text(request, "name"), false)?;
                Resources::of_macos(request)
                    .ok_or("A macOS computer's CPUs, memory and disk are numbers.")?;
                let asked_network = Self::asked_network_policy(params, "networkPolicy")?;
                let network = self.new_computer_network(asked_network.clone(), None, false, name);
                rewritten["networkPolicy"] = json!(Self::chosen_network(&network, asked_network));
                let creates = Creation {
                    name: name.to_owned(),
                    id: None,
                    source: None,
                };
                if let Err(summary) = network {
                    return Ok(ask_to_create(
                        ApprovalKind::Network,
                        summary,
                        rewritten,
                        creates,
                    ));
                }
                Ok(Decision::Allow(Allowed {
                    params: Some(rewritten),
                    creates: Some(creates),
                    ..Allowed::plain()
                }))
            }
            _ => Err("A new computer is Linux or macOS (kind).".into()),
        }
    }

    /// The network policy a new computer is saved with: the one it was judged against, or, when
    /// that is wider than the caller's and so waits for the owner, the one that was asked for.
    /// A caller whose own policy cannot be read gets a closed policy for the owner to widen, so
    /// an approved creation is never saved without one.
    fn chosen_network(
        network: &Result<NetworkPolicy, String>,
        asked: Option<NetworkPolicy>,
    ) -> NetworkPolicy {
        match network {
            Ok(policy) => policy.clone(),
            Err(_) => asked.unwrap_or_else(|| NetworkPolicy {
                mode: NetworkPolicyMode::Closed,
                ..NetworkPolicy::default()
            }),
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
                let record = self.world.recipe_record(&target)?;
                if self.made.contains(&target) {
                    if record.is_some() && has_keep_hooks(params)? {
                        return Err(format!(
                            "{} was built from a recipe; its keep hooks come from the recipe.",
                            self.shown(&target)
                        ));
                    }
                    return Ok(allow_with(params.clone()));
                }
                if target != self.caller {
                    return Err("A merge can go only into this computer or one it made.".into());
                }
                let details = MergeDetails::of(params, record.as_ref(), &self.shown(&target))?;
                let base = format!(
                    "Merge the changes of {} into {}.",
                    self.shown(&fork),
                    self.shown(&target)
                );
                Ok(ask_merge(
                    kind,
                    self.merge_summary(base.clone(), &target, &details, false),
                    self.merge_summary(base, &target, &details, true),
                ))
            }
            MethodName::ChangesRequestsKeep => {
                let fork = self.request_target(params)?;
                if !self.made.contains(&fork) {
                    return Err("That review request is not about a computer this one made.".into());
                }
                let target = self
                    .world
                    .request_target(text(params, "requestId").unwrap_or_default());
                if target
                    .as_ref()
                    .is_some_and(|target| self.made.contains(target))
                {
                    if let Some(target) = &target {
                        if self.world.recipe_record(target)?.is_some() && has_keep_hooks(params)? {
                            return Err(format!(
                                "{} was built from a recipe; its keep hooks come from the recipe.",
                                self.shown(target)
                            ));
                        }
                    }
                    return Ok(allow_with(params.clone()));
                }
                let target = target.unwrap_or_else(|| self.caller.to_owned());
                let record = self.world.recipe_record(&target)?;
                let details = MergeDetails::of(params, record.as_ref(), &self.shown(&target))?;
                let base = format!(
                    "Merge the changes of {} that its review request lists.",
                    self.shown(&fork)
                );
                Ok(ask_merge(
                    kind,
                    self.merge_summary(base.clone(), &target, &details, false),
                    self.merge_summary(base, &target, &details, true),
                ))
            }
            MethodName::ChangesUndo => {
                let target = self.tree_target(params, "computerId")?;
                if self.made.contains(&target) {
                    return Ok(allow_with(params.clone()));
                }
                Ok(ask(
                    kind,
                    format!(
                        "Undo the last merge or replacement of {}, putting it back as it was before.",
                        self.shown(&target)
                    ),
                ))
            }
            MethodName::ChangesPromote => {
                let fork = self.made_target(params, "forkId")?;
                let target = text(params, "targetId")
                    .map(str::to_owned)
                    .or_else(|| self.world.parent(&fork))
                    .filter(|target| self.in_tree(target))
                    .ok_or("A fork can replace only this computer or one it made.")?;
                if self.made.contains(&target) {
                    return Ok(allow_with(params.clone()));
                }
                Ok(ask(
                    kind,
                    format!(
                        "Replace {} with its fork {}: this restarts {}, and what it did since the fork is set aside.",
                        self.shown(&target),
                        self.shown(&fork),
                        self.shown(&target)
                    ),
                ))
            }
            MethodName::NetworkPublish => self.publish_port(kind, params),
            MethodName::SshAccessSave => self.ssh_access(kind, params),
            MethodName::PushStart => self.push(kind, params),
            MethodName::GitHubConfigurationSave => self.github_configuration(kind, params),
            MethodName::NetworkPolicySet => self.network_policy_set(kind, params),
            MethodName::SecretsSave => self.add_secret(kind, params),
            other => Err(format!(
                "{} has no rule for a computer's agent yet.",
                other.name()
            )),
        }
    }

    /// A network policy in `params[key]`, checked and in its stored form; `None` when absent.
    fn asked_network_policy(params: &Value, key: &str) -> Result<Option<NetworkPolicy>, String> {
        match params.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(value) => {
                let policy: NetworkPolicy = serde_json::from_value(value.clone())
                    .map_err(|_| format!("{key} is not a network policy."))?;
                crate::network_policy::validate(&policy).map(Some)
            }
        }
    }

    /// The network policy a computer the caller makes gets: the one asked for, else
    /// `inherited` when it is no wider than the caller's own, else the caller's own. `Err` holds
    /// the summary of what is wider than the caller's own policy, for a request.
    ///
    /// A fork (`inherited` is the source's policy) is judged with the versions the engine will
    /// save for it: presets and the GitHub grant keep the source's, so a fork never reaches
    /// hosts a newer list added unless the owner approves it. `github` is whether the new
    /// computer gets the source's GitHub access.
    fn new_computer_network(
        &self,
        asked: Option<NetworkPolicy>,
        inherited: Option<Held>,
        github: bool,
        new_name: &str,
    ) -> Result<NetworkPolicy, String> {
        let Some(own) = self.world.network_held(self.caller) else {
            let mut summary = format!(
                "Give {} a network policy; the network policy of {} could not be read to compare.",
                quoted(new_name),
                self.shown(self.caller)
            );
            if let Some(asked) = asked.filter(|asked| asked.mode != NetworkPolicyMode::Closed) {
                let closed = Held::latest(NetworkPolicy {
                    mode: NetworkPolicyMode::Closed,
                    ..NetworkPolicy::default()
                });
                summary.push(' ');
                summary.push_str(&crate::network_policy::widening_summary_held(
                    &quoted(new_name),
                    &closed,
                    &crate::network_policy::Grants::default(),
                    &Held::latest(asked),
                    &crate::network_policy::Grants::github_only(github),
                ));
            }
            return Err(summary);
        };
        let own_grants = Grants::github_only(self.world.access(self.caller).github.is_some());
        let new_grants = Grants::github_only(github);
        // What the engine saves for the new computer: a fork keeps its source's versions, a
        // computer made new is approved at the versions of this Silo.
        let held = |policy: NetworkPolicy| match &inherited {
            Some(source) => Held::inheriting(policy, Some(source)),
            None => Held::latest(policy),
        };
        let within_own = |chosen: &Held| {
            crate::network_policy::no_wider_held(chosen, &new_grants, &own, &own_grants)
        };
        let chosen = match asked {
            Some(asked) => held(asked),
            None => match inherited.as_ref().map(|source| held(source.policy.clone())) {
                Some(inherited) if within_own(&inherited) => inherited,
                _ => held(own.policy.clone()),
            },
        };
        if within_own(&chosen) {
            Ok(chosen.policy)
        } else {
            Err(crate::network_policy::widening_summary_held(
                &quoted(new_name),
                &own,
                &own_grants,
                &chosen,
                &new_grants,
            ))
        }
    }

    /// Narrowing a network policy is free; anything wider than the target's current policy, or
    /// for a computer it made wider than the caller's own, asks. A free change carries the
    /// policy it was judged against, so it is saved only if that is still the policy.
    fn network_policy_set(&self, kind: ApprovalKind, params: &Value) -> Result<Decision, String> {
        let target = self.tree_target(params, "computerId")?;
        let asked = Self::asked_network_policy(params, "policy")?
            .ok_or("The call must give a policy (policy).")?;
        let mut rewritten = json!({ "computerId": target, "policy": asked });
        let current = self.world.network_held(&target);
        let own = self.world.network_held(self.caller);
        // Presets are judged at this Silo's versions, so setting a policy never takes a preset
        // update unasked. The GitHub grant keeps the version the target was approved at, which
        // is what the engine saves for it.
        let mut asked_held = Held::latest(asked.clone());
        match current
            .as_ref()
            .and_then(|held| held.pins.get(GITHUB_GRANT_PIN))
        {
            Some(version) => asked_held
                .pins
                .insert(GITHUB_GRANT_PIN.to_owned(), *version),
            None => asked_held.pins.remove(GITHUB_GRANT_PIN),
        };
        let target_grants = Grants::github_only(self.world.access(&target).github.is_some());
        let own_grants = Grants::github_only(self.world.access(self.caller).github.is_some());
        let within_current = current.as_ref().is_some_and(|current| {
            crate::network_policy::no_wider_held(
                &asked_held,
                &target_grants,
                current,
                &target_grants,
            )
        });
        let within_own = target == self.caller
            || own.as_ref().is_some_and(|own| {
                crate::network_policy::no_wider_held(&asked_held, &target_grants, own, &own_grants)
            });
        if let (true, true, Some(current)) = (within_current, within_own, &current) {
            rewritten["expected"] = json!(current.policy);
            return Ok(allow_with(rewritten));
        }
        let closed = Held::latest(NetworkPolicy {
            mode: NetworkPolicyMode::Closed,
            ..NetworkPolicy::default()
        });
        let (base, base_grants) = if within_current {
            (own, own_grants)
        } else {
            (current, target_grants.clone())
        };
        let base = base.unwrap_or(closed);
        Ok(ask_with(
            kind,
            crate::network_policy::widening_summary_held(
                &self.shown(&target),
                &base,
                &base_grants,
                &asked_held,
                &target_grants,
            ),
            rewritten,
        ))
    }

    /// The summary of a merge; `for_computer` leaves out the commands of a recipe's hooks,
    /// which no computer may read.
    fn merge_summary(
        &self,
        base: String,
        target: &str,
        details: &MergeDetails,
        for_computer: bool,
    ) -> String {
        let commands = if for_computer {
            &details.computer_commands
        } else {
            &details.commands
        };
        let mut lead = Vec::new();
        if !commands.is_empty() {
            lead.push(format!("Runs commands in {}", self.shown(target)));
        }
        if details.data_ok {
            lead.push("accepts database changes".to_owned());
        }
        let mut summary = if lead.is_empty() {
            String::new()
        } else {
            let mut lead = lead.join(" and ");
            lead[..1].make_ascii_uppercase();
            format!("{lead}. ")
        };
        summary.push_str(&base);
        if !commands.is_empty() {
            summary.push_str(&format!(
                " In {} it {}.",
                self.shown(target),
                commands.join("; ")
            ));
        }
        if details.data_ok {
            summary.push_str(
                " Database changes are carried even where no migration step covers them.",
            );
        }
        if !details.choices.is_empty() {
            summary.push_str(&format!(" Choices: {}.", listed(&details.choices)));
        }
        if !details.skipped.is_empty() {
            summary.push_str(&format!(" Not brought over: {}.", listed(&details.skipped)));
        }
        if let Some(recipe_name) = &details.recipe_name {
            summary.push_str(&format!(
                " The saved recipe {} will get the merged packages and files.",
                quoted(recipe_name)
            ));
        }
        summary
    }

    /// `network.publish`. The device port is the engine's to choose for a computer's agent, so
    /// a port it asks for is not passed on; the scheme is.
    fn publish_port(&self, kind: ApprovalKind, params: &Value) -> Result<Decision, String> {
        let target = self.tree_target(params, "computerId")?;
        let port = params
            .get("port")
            .and_then(Value::as_u64)
            .filter(|port| (1..=65535).contains(port))
            .ok_or("Enter a port from 1 to 65535.")?;
        let mut rewritten = json!({ "computerId": target, "port": port });
        let how = match params.get("scheme") {
            None | Some(Value::Null) => "as a plain port",
            Some(Value::String(scheme)) if scheme == "http" => "as a website over http",
            Some(Value::String(scheme)) if scheme == "https" => "as a website over https",
            Some(_) => return Err("The scheme is http or https.".into()),
        };
        if let Some(scheme) = params.get("scheme").filter(|scheme| !scheme.is_null()) {
            rewritten["scheme"] = scheme.clone();
        }
        Ok(ask_with(
            kind,
            format!(
                "Opens a port on this device: publish port {port} of {} {how}, on a device port Silo chooses.",
                self.shown(&target)
            ),
            rewritten,
        ))
    }

    /// `sshAccess.save`. Turning access off only narrows it; turning it on keeps the saved
    /// address and keys unless the call changes them, and then says so.
    fn ssh_access(&self, kind: ApprovalKind, params: &Value) -> Result<Decision, String> {
        let target = self.tree_target(params, "computerId")?;
        let (saved_port, saved_address) = self
            .world
            .ssh_settings(&target)
            .unwrap_or((2222, "127.0.0.1".into()));
        // Only the computer and the switch are the caller's when access goes off; the port and
        // address stay what is saved, so nothing the caller sends is waiting for the next time
        // the owner turns access on.
        if params.get("enabled") == Some(&Value::Bool(false)) {
            return Ok(allow_with(json!({
                "computerId": target,
                "enabled": false,
                "port": saved_port,
                "bindAddress": saved_address,
            })));
        }
        let port = params
            .get("port")
            .and_then(Value::as_u64)
            .filter(|port| (1..=65535).contains(port))
            .ok_or("Enter a port from 1 to 65535.")?;
        let address = match params.get("bindAddress") {
            None | Some(Value::Null) => saved_address.clone(),
            Some(Value::String(address)) => address.clone(),
            Some(_) => return Err("The bind address is an IPv4 address.".into()),
        };
        let ip: std::net::Ipv4Addr = address
            .parse()
            .map_err(|_| "Choose an IPv4 address assigned to the device.")?;
        if ip.is_unspecified()
            || ip.is_multicast()
            || ip.is_broadcast()
            || (ip.is_loopback() && ip != std::net::Ipv4Addr::LOCALHOST)
        {
            return Err("Choose 127.0.0.1 or a specific network interface address.".into());
        }
        let saved_keys: BTreeSet<String> = self
            .world
            .ssh_keys(&target)
            .iter()
            .filter_map(|key| crate::ssh_access::normalize_key(key).ok())
            .collect();
        let mut rewritten = json!({
            "computerId": target,
            "enabled": true,
            "port": port,
            "bindAddress": address,
        });
        let mut added: Vec<String> = Vec::new();
        match params.get("keys") {
            None | Some(Value::Null) => {}
            Some(Value::Array(keys)) => {
                if keys.len() > 128 {
                    return Err("Use at most 128 authorized keys.".into());
                }
                let mut normalized = Vec::new();
                for key in keys {
                    let key = key.as_str().ok_or("Each key is a public key string.")?;
                    let key = crate::ssh_access::normalize_key(key)?;
                    if !normalized.contains(&key) {
                        if !saved_keys.contains(&key) {
                            added.push(
                                crate::ssh_access::key_fingerprint(&key)
                                    .ok_or("A key could not be read.")?,
                            );
                        }
                        normalized.push(key);
                    }
                }
                rewritten["keys"] = json!(normalized);
            }
            Some(_) => return Err("The keys are a list of public keys.".into()),
        }
        let reach = if address == "127.0.0.1" {
            "reachable only from this device".to_owned()
        } else {
            format!("reachable from the network at {address}")
        };
        let mut summary = String::new();
        if !added.is_empty() {
            summary.push_str(&format!(
                "Authorises {} key chosen by the agent, {}. ",
                if added.len() == 1 { "a" } else { "keys" },
                listed(&added)
            ));
        }
        summary.push_str(&format!(
            "Opens SSH into {} on port {port}, {reach}.",
            self.shown(&target)
        ));
        if added.is_empty() && !saved_keys.is_empty() && rewritten.get("keys").is_none() {
            summary.push_str(" The keys already saved for it stay authorised.");
        }
        Ok(ask_with(kind, summary, rewritten))
    }

    /// `push.start`: exactly one commit to one branch of one repository.
    fn push(&self, kind: ApprovalKind, params: &Value) -> Result<Decision, String> {
        let computer = self.tree_target(params, "computerId")?;
        let path = text(params, "repositoryPath").ok_or("Name the repository folder to push.")?;
        if !crate::host_push_transport::valid_repository_path(path) {
            return Err("The repository must be a folder below /workspace.".into());
        }
        let operation = text(params, "operationId").ok_or("The push needs an operationId.")?;
        let target: crate::host_push::PushTarget = serde_json::from_value(
            params
                .get("target")
                .cloned()
                .ok_or("Name the repository, branch and commit to push (target).")?,
        )
        .map_err(|_| "The target is a repository, a branch and a commit.")?;
        target.validate()?;
        let rewritten = json!({
            "computerId": computer,
            "repositoryPath": path,
            "operationId": operation,
            "target": {
                "repository": target.repository,
                "branch": target.branch,
                "commit": target.commit,
            },
        });
        Ok(ask_with(
            kind,
            format!(
                "Publishes to GitHub: pushes commit {} of {} in {} to the branch {} of {}, through this device.",
                &target.commit[..12],
                quoted(path),
                self.shown(&computer),
                quoted(&target.branch),
                quoted(&target.repository)
            ),
            rewritten,
        ))
    }

    /// `secrets.save` adding a secret.
    fn add_secret(&self, kind: ApprovalKind, params: &Value) -> Result<Decision, String> {
        let request = params.get("request").ok_or("The secret is missing.")?;
        if request.get("operation").and_then(Value::as_str) != Some("add") {
            return Err(
                "Only the person who runs Silo changes a saved secret; a computer's agent can ask to add one."
                    .into(),
            );
        }
        let name = text(request, "name").ok_or("A secret needs a name.")?;
        let token = request.get("kind").and_then(Value::as_str) == Some("github-token");
        let valid = if token {
            crate::secrets::valid_token_name(name)
        } else {
            crate::secrets::valid_name(name)
        };
        if !valid {
            return Err(if token {
                "Choose a name of letters, digits, hyphens and underscores for this token."
            } else {
                "Choose a valid, unreserved secret name."
            }
            .into());
        }
        if request
            .get("value")
            .and_then(Value::as_str)
            .is_none_or(|value| value.is_empty() || value.len() > 64 * 1024 || value.contains('\0'))
        {
            return Err(
                "Secret values must contain between 1 and 65536 bytes without null characters."
                    .into(),
            );
        }
        let strings = |key: &str| -> Result<Vec<String>, String> {
            match request.get(key) {
                None | Some(Value::Null) => Ok(Vec::new()),
                Some(Value::Array(items)) => items
                    .iter()
                    .map(|item| item.as_str().map(str::to_owned))
                    .collect::<Option<_>>()
                    .ok_or_else(|| format!("{key} is a list of strings.")),
                Some(_) => Err(format!("{key} is a list of strings.")),
            }
        };
        let computers = strings("computerIds")?;
        let domains = strings("allowedDomains")?;
        if token {
            if !computers.is_empty() || !domains.is_empty() {
                return Err(
                    "A GitHub token is given to computers in GitHub access, with no hosts of its own."
                        .into(),
                );
            }
            return Ok(ask(
                kind,
                format!(
                    "Full GitHub permissions: add the GitHub token {}. A token gives its full permissions to any computer that uses it.",
                    quoted(name)
                ),
            ));
        }
        if computers.iter().any(|id| !self.in_tree(id)) {
            return Err(
                "A secret can be asked for only for this computer or computers it made.".into(),
            );
        }
        if computers.is_empty()
            || computers.len() > 100
            || computers.iter().collect::<BTreeSet<_>>().len() != computers.len()
            || domains.is_empty()
            || domains.len() > 100
            || !domains
                .iter()
                .all(|domain| crate::secrets::valid_domain(domain))
        {
            return Err("Select computers and valid allowed domains.".into());
        }
        let wildcard = domains.iter().any(|domain| domain == "*");
        let any = request.get("allowAnyDestination") == Some(&Value::Bool(true));
        if wildcard && !any {
            return Err(
                "Confirm that this secret may go to any HTTPS destination (allowAnyDestination) before saving a wildcard secret."
                    .into(),
            );
        }
        let names: Vec<String> = computers.iter().map(|id| self.name(id)).collect();
        let sent = if wildcard {
            "Sent to any destination".to_owned()
        } else {
            format!("Sent only to {}", listed(&domains))
        };
        let mut summary = format!(
            "{sent}: add the secret {} for {}.",
            quoted(name),
            listed(&names)
        );
        let reachable: Vec<String> = domains
            .iter()
            .filter(|domain| crate::network_policy::validate_host(domain).is_ok())
            .cloned()
            .collect();
        if !reachable.is_empty() {
            summary.push_str(&format!(
                " A computer whose network is a list of allowed hosts can also reach {} because of it.",
                listed(&reachable)
            ));
        }
        let warnings = crate::network_policy::shared_hosting_warnings(&domains);
        for warning in &warnings {
            summary.push(' ');
            summary.push_str(warning);
        }
        if domains
            .iter()
            .any(|domain| crate::network_policy::is_too_wide_wildcard(domain))
        {
            summary.push_str(
                " A pattern that covers a whole public domain is not added to a list of allowed hosts.",
            );
        }
        // Only the owner confirms shared-hosting hosts, and only by approving a summary that
        // names them; whatever a computer sends for it is dropped.
        let sent_by_computer = request.get("allowSharedHosting").is_some();
        if !warnings.is_empty() || sent_by_computer {
            let mut confirmed = params.clone();
            if let Some(object) = confirmed["request"].as_object_mut() {
                object.remove("allowSharedHosting");
                if !warnings.is_empty() {
                    object.insert("allowSharedHosting".to_owned(), Value::Bool(true));
                }
            }
            return Ok(ask_with(kind, summary, confirmed));
        }
        Ok(ask(kind, summary))
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
            return Err("Nothing in that GitHub configuration changes.".into());
        }
        let uses_token = |policy: &Value| {
            policy.get("authenticationMethod").and_then(Value::as_str) == Some("token")
        };
        let token_of = |policy: &Value| {
            policy
                .get("tokenSecretId")
                .and_then(Value::as_str)
                .map(str::to_owned)
        };
        let mut sentences = Vec::new();
        let mut writes = false;
        let mut tokens = false;
        for id in &changed {
            let (was, now) = (before.get(*id), after.get(*id));
            let mut change = format!(
                "{} goes from {} to {}",
                self.shown(id),
                github_access(was),
                github_access(now)
            );
            writes |= github_writes(now) && !github_writes(was);
            let identity =
                |policy: Option<&Value>| policy.and_then(|policy| policy.get("identity")).cloned();
            if identity(was) != identity(now) {
                change.push_str(&format!("; {}", github_identity(now)));
            }
            if let Some(policy) = now.filter(|policy| uses_token(policy)) {
                let token = token_of(policy).ok_or(
                    "Say which GitHub token the computer should use; a computer's agent has to name the token secret.",
                )?;
                if was.filter(|policy| uses_token(policy)).and_then(token_of) != Some(token.clone())
                {
                    let token_name = self
                        .world
                        .github_token_name(&token)
                        .ok_or("That GitHub token does not exist.")?;
                    tokens = true;
                    change.push_str(&format!(
                        "; it would use the GitHub token {}, which gives its full permissions",
                        quoted(&token_name)
                    ));
                }
            } else if now.is_some() && was.is_some_and(|policy| uses_token(policy)) {
                change.push_str("; it would use the GitHub sign-in instead of a token");
            }
            sentences.push(change);
        }
        let lead = if tokens {
            "Full GitHub token permissions"
        } else if writes {
            "GitHub write access"
        } else {
            "GitHub access"
        };
        Ok(ask(kind, format!("{lead}: {}.", sentences.join(". "))))
    }
}

/// "1 step" or "3 steps".
fn steps(count: usize) -> String {
    match count {
        1 => "1 step".to_owned(),
        many => format!("{many} steps"),
    }
}

/// What a merge asks to do besides bringing the changes over.
struct MergeDetails {
    /// What the hooks do in the target.
    commands: Vec<String>,
    /// The same, with the commands of a recipe's hooks counted instead of quoted.
    computer_commands: Vec<String>,
    data_ok: bool,
    choices: Vec<String>,
    skipped: Vec<String>,
    recipe_name: Option<String>,
}

fn has_keep_hooks(params: &Value) -> Result<bool, String> {
    match params.get("keep") {
        None | Some(Value::Null) => Ok(false),
        Some(hooks @ Value::Object(_)) => {
            let strings = |value: Option<&Value>, what: &str| -> Result<Vec<String>, String> {
                match value {
                    None | Some(Value::Null) => Ok(Vec::new()),
                    Some(Value::Array(items)) => items
                        .iter()
                        .map(|item| item.as_str().map(str::to_owned))
                        .collect::<Option<_>>()
                        .ok_or_else(|| format!("{what} is a list of strings.")),
                    Some(_) => Err(format!("{what} is a list of strings.")),
                }
            };
            let restart = strings(hooks.get("restart"), "keep.restart")?;
            let migrate = strings(hooks.get("migrate"), "keep.migrate")?;
            let after = strings(hooks.get("after"), "keep.after")?;
            Ok(!restart.is_empty() || !migrate.is_empty() || !after.is_empty())
        }
        Some(_) => Err("keep is an object of hooks.".into()),
    }
}

impl MergeDetails {
    fn of(
        params: &Value,
        record: Option<&silo_api::methods::recipes::RecipeRecord>,
        target_shown: &str,
    ) -> Result<Self, String> {
        let strings = |value: Option<&Value>, what: &str| -> Result<Vec<String>, String> {
            match value {
                None | Some(Value::Null) => Ok(Vec::new()),
                Some(Value::Array(items)) => items
                    .iter()
                    .map(|item| item.as_str().map(str::to_owned))
                    .collect::<Option<_>>()
                    .ok_or_else(|| format!("{what} is a list of strings.")),
                Some(_) => Err(format!("{what} is a list of strings.")),
            }
        };
        let mut commands = Vec::new();
        let mut computer_commands = Vec::new();
        let mut recipe_name = None;
        if let Some(record) = record {
            if has_keep_hooks(params)? {
                return Err(format!(
                    "{target_shown} was built from a recipe; its keep hooks come from the recipe."
                ));
            }
            let keep = crate::recipes::hooks_of(&record.recipe);
            if !keep.restart.is_empty() {
                commands.push(format!("restarts {}", listed(&keep.restart)));
                computer_commands.push(format!("restarts {}", listed(&keep.restart)));
            }
            if !keep.migrate.is_empty() {
                commands.push(format!("runs {} to migrate", listed(&keep.migrate)));
                computer_commands.push(format!("runs {} to migrate", steps(keep.migrate.len())));
            }
            if !keep.after.is_empty() {
                commands.push(format!("runs {} last", listed(&keep.after)));
                computer_commands.push(format!("runs {} last", steps(keep.after.len())));
            }
            recipe_name = Some(record.name.clone());
        } else {
            match params.get("keep") {
                None | Some(Value::Null) => {}
                Some(hooks @ Value::Object(_)) => {
                    let restart = strings(hooks.get("restart"), "keep.restart")?;
                    let migrate = strings(hooks.get("migrate"), "keep.migrate")?;
                    let after = strings(hooks.get("after"), "keep.after")?;
                    if !restart.is_empty() {
                        commands.push(format!("restarts {}", listed(&restart)));
                    }
                    if !migrate.is_empty() {
                        commands.push(format!("runs {} to migrate", listed(&migrate)));
                    }
                    if !after.is_empty() {
                        commands.push(format!("runs {} last", listed(&after)));
                    }
                }
                Some(_) => return Err("keep is an object of hooks.".into()),
            }
            computer_commands.clone_from(&commands);
        }
        let data_ok = match params.get("dataOk") {
            None | Some(Value::Null) => false,
            Some(Value::Bool(ok)) => *ok,
            Some(_) => return Err("dataOk is true or false.".into()),
        };
        let choices = match params.get("choices") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Object(answers)) => answers
                .iter()
                .map(|(item, answer)| {
                    answer
                        .as_str()
                        .map(|answer| format!("{item} = {answer}"))
                        .ok_or("Each choice is an answer string.".to_owned())
                })
                .collect::<Result<_, _>>()?,
            Some(_) => return Err("choices is an object of answers.".into()),
        };
        Ok(Self {
            commands,
            computer_commands,
            data_ok,
            choices,
            skipped: strings(params.get("skip"), "skip")?,
            recipe_name,
        })
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

/// What a computer reads in place of the engine's summary of a review request that waits for
/// an approval, which can name commands the computer may not read.
pub const WAITING_MERGE_SUMMARY: &str =
    "A merge waiting for the approval of the person who runs Silo.";

/// Replaces the engine's text on a review request an approval wraps, and on the answer that
/// shows it.
fn cut_request_summary(object: &mut serde_json::Map<String, Value>) {
    let wrapped = |request: &serde_json::Map<String, Value>| {
        request.get("approvalId").is_some_and(|id| !id.is_null())
            && request.contains_key("reviewDigest")
    };
    if wrapped(object) {
        object.insert("summary".into(), json!(WAITING_MERGE_SUMMARY));
    }
    if object
        .get("request")
        .and_then(Value::as_object)
        .is_some_and(wrapped)
        && object.contains_key("text")
    {
        object.insert("text".into(), json!(WAITING_MERGE_SUMMARY));
    }
}

/// Cuts any answer to a computer down to what it may see, wherever it sits in the answer (a
/// job's outcome holds the device's state after a fork, for one). A list of computers keeps only
/// the visible ones; the device's own state loses its secrets, GitHub, activities and pushes; a
/// job outcome too large to answer at once, which cannot be cut, is withheld.
pub fn cut_for_computer(result: &mut Value, visible: &BTreeSet<String>) {
    match result {
        Value::Array(items) => {
            for item in items {
                cut_for_computer(item, visible);
            }
        }
        Value::Object(object) => {
            if object.contains_key("outcomeChunk") {
                object.insert("outcomeChunk".into(), Value::Null);
            }
            cut_request_summary(object);
            if let Some(Value::Array(computers)) = object.get_mut("computers") {
                computers.retain(|computer| {
                    ["/configuration/id", "/computerId", "/id"]
                        .iter()
                        .find_map(|pointer| computer.pointer(pointer).and_then(Value::as_str))
                        .is_some_and(|id| visible.contains(id))
                });
                for (key, emptied) in [
                    ("secrets", json!([])),
                    ("github", Value::Null),
                    ("activities", json!([])),
                    ("repositoryPushOperations", json!([])),
                    ("computerConfigurationOperation", Value::Null),
                    ("repositoryCatalog", json!([])),
                    ("account", Value::Null),
                ] {
                    if object.contains_key(key) {
                        object.insert(key.into(), emptied);
                    }
                }
            }
            for value in object.values_mut() {
                cut_for_computer(value, visible);
            }
        }
        _ => {}
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
