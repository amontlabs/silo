//! Serves the in-guest client: reads the request files computers write into their bridge
//! folders, checks each against the computer's token, runs it as that computer's call
//! (`Caller::Computer`, which the policy judges in `methods::dispatch`) and writes the answer
//! back (`docs/SiloUI-IN-GUEST-CLIENT.md`). Each computer with a token has a worker of its own,
//! so one slow call holds up no other computer.

mod frames;
mod worker;

use std::{collections::HashMap, collections::HashSet, future::Future, pin::Pin, sync::Arc};

use serde_json::{json, Value};
use silo_api::{methods::MethodName, transport::PROTOCOL_VERSION};
use silo_engine::policy::Caller;

use crate::{
    context::SilodContext,
    dispatcher::{DispatchError, RequestContext},
    methods::{self, HandlerContext},
};

pub use frames::{handle_frame, reap, Frames, Reserved};
pub use worker::start;

/// Runs one method as the agent inside `computer`, or as the engine itself when `computer` is
/// `None`.
pub trait Runner: Send + Sync {
    fn run<'a>(
        &'a self,
        computer: Option<&'a str>,
        method: &'a str,
        params: Value,
    ) -> Pin<Box<dyn Future<Output = Result<Value, DispatchError>> + Send + 'a>>;
}

/// Which computers exist, what they are called and which were made from which.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub names: HashMap<String, String>,
    pub linux: HashSet<String>,
    pub parents: HashMap<String, Option<String>>,
    pub states: HashMap<String, String>,
}

impl Snapshot {
    pub fn from_values(list: &Value, tree: &Value) -> Self {
        let mut snapshot = Self::default();
        for computer in list
            .pointer("/linux/computers")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(id) = computer
                .pointer("/configuration/id")
                .and_then(Value::as_str)
            {
                snapshot.linux.insert(id.to_owned());
            }
        }
        for node in tree
            .get("nodes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(id) = node.get("id").and_then(Value::as_str) else {
                continue;
            };
            if let Some(name) = node.get("name").and_then(Value::as_str) {
                snapshot.names.insert(id.to_owned(), name.to_owned());
            }
            let parent = node
                .get("parent")
                .and_then(Value::as_str)
                .map(str::to_owned);
            snapshot.parents.insert(id.to_owned(), parent);
            if let Some(state) = node.get("state").and_then(Value::as_str) {
                snapshot.states.insert(id.to_owned(), state.to_owned());
            }
        }
        snapshot
    }

    pub fn descendants_of(&self, root: &str) -> HashSet<String> {
        let mut found = HashSet::new();
        let mut frontier = vec![root.to_owned()];
        while let Some(parent) = frontier.pop() {
            for (id, of) in &self.parents {
                if of.as_deref() == Some(parent.as_str()) && id != root && found.insert(id.clone())
                {
                    frontier.push(id.clone());
                }
            }
        }
        found
    }
}

/// Runs methods through the engine's own handlers, as the client `guest-client`.
pub struct EngineRunner {
    engine: Arc<SilodContext>,
}

impl EngineRunner {
    pub fn new(engine: Arc<SilodContext>) -> Self {
        Self { engine }
    }

    fn context(&self, computer: Option<&str>) -> HandlerContext {
        let caller = match computer {
            Some(id) => Caller::Computer { id: id.to_owned() },
            None => Caller::owner("guest-client"),
        };
        // No socket client asked, so what would answer only the asker (a list's event
        // barrier) is announced to every subscriber, as for another device's request.
        HandlerContext::for_owner_bridge(
            self.engine.clone(),
            RequestContext {
                client_id: "guest-client".to_owned(),
                protocol: PROTOCOL_VERSION,
                client: None,
                caller,
            },
        )
    }

    pub async fn snapshot(&self) -> Result<Snapshot, DispatchError> {
        let list = self.run(None, "computers.list", json!({})).await?;
        let tree = self.run(None, "computers.tree", json!({})).await?;
        Ok(Snapshot::from_values(&list, &tree))
    }
}

impl Runner for EngineRunner {
    fn run<'a>(
        &'a self,
        computer: Option<&'a str>,
        method: &'a str,
        params: Value,
    ) -> Pin<Box<dyn Future<Output = Result<Value, DispatchError>> + Send + 'a>> {
        Box::pin(async move {
            let method =
                MethodName::from_name(method).ok_or_else(|| DispatchError::unsupported(method))?;
            methods::dispatch(&self.context(computer), method, &params)
                .await
                .map(|output| output.value)
        })
    }
}

#[cfg(test)]
mod tests;
