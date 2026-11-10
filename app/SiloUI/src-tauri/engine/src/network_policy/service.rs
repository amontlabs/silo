//! `network.policy.get` and `network.policy.set`.
//!
//! A Linux virtual machine's rules are in its runtime configuration, so the engine compares
//! them with the stored policy. A macOS or container computer's gateway takes its policy when it
//! starts; the engine remembers what each running gateway was started with
//! ([`gateway_started`]).

use std::collections::HashMap;
use std::sync::Mutex;

use serde_json::Value;
use silo_api::methods::network::{NetworkPolicyState, NetworkPolicyTiming};

use super::{
    effective, extra_rules, plan, preset_list, store, validate, Grants, NetworkPolicy,
    NetworkPolicyMode, Plan,
};
use crate::runtime::RuntimePaths;

/// Which kind of computer a call is about.
pub enum Target<'a> {
    Linux(&'a RuntimePaths),
    /// A macOS computer: its name and whether it is running.
    Macos { name: String, running: bool },
}

static GATEWAYS: Mutex<Option<HashMap<String, (Value, bool)>>> = Mutex::new(None);

/// Notes the policy a computer's gateway was started with.
pub fn gateway_started(computer_id: &str, policy: &Value, strict: bool) {
    let mut held = GATEWAYS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    held.get_or_insert_with(HashMap::new)
        .insert(computer_id.to_owned(), (policy.clone(), strict));
}

fn gateway_holds(computer_id: &str) -> Option<(Value, bool)> {
    GATEWAYS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .and_then(|held| held.get(computer_id).cloned())
}

/// When `plan` is in force for a computer that is running or not, given whether what it runs
/// with already matches.
fn timing(running: bool, matches: bool) -> NetworkPolicyTiming {
    match (running, matches) {
        (false, _) => NetworkPolicyTiming::NextStart,
        (true, true) => NetworkPolicyTiming::Applied,
        (true, false) => NetworkPolicyTiming::NextRestart,
    }
}

fn closed() -> NetworkPolicy {
    NetworkPolicy {
        mode: NetworkPolicyMode::Closed,
        ..NetworkPolicy::default()
    }
}

fn gateway_matches(computer_id: &str, plan: &Plan) -> bool {
    gateway_holds(computer_id).is_some_and(|(policy, strict)| {
        strict == plan.strict && super::same_policy(&policy, &plan.policy_value())
    })
}

/// The policy a computer runs with, its plan and when the plan is in force.
fn resolve(
    computer_id: &str,
    target: &Target<'_>,
) -> Result<(NetworkPolicy, Plan, NetworkPolicyTiming), String> {
    let stored = store::get(computer_id)?;
    match target {
        Target::Linux(paths) => {
            let view = crate::runtime::checkpoints::network_policy_view(paths, computer_id)?;
            let runtime_policy = view
                .network
                .as_ref()
                .and_then(|network| network.get("policy"));
            let recorded = runtime_policy.or(view.recorded.as_ref());
            // A container the engine blocked (an import) is closed whatever is stored.
            let blocked = view.container
                && runtime_policy.is_some_and(|policy| {
                    policy == &crate::containers::secrets::blocked_policy()
                });
            let policy = if blocked {
                closed()
            } else {
                effective(stored, recorded)
            };
            let plan = plan(
                &policy,
                &Grants::of_computer(&view.name),
                &extra_rules(computer_id),
            );
            let matches = if view.container {
                gateway_matches(computer_id, &plan)
            } else {
                view.network
                    .as_ref()
                    .is_some_and(|network| plan.matches(network))
            };
            Ok((policy, plan, timing(view.running && !view.pending, matches)))
        }
        Target::Macos { name, running } => {
            let policy = effective(stored, None);
            let plan = plan(&policy, &Grants::of_computer(name), &extra_rules(computer_id));
            let matches = gateway_matches(computer_id, &plan);
            Ok((policy, plan, timing(*running, matches)))
        }
    }
}

fn state(
    computer_id: &str,
    policy: NetworkPolicy,
    plan: Plan,
    timing: NetworkPolicyTiming,
) -> NetworkPolicyState {
    NetworkPolicyState {
        computer_id: computer_id.to_owned(),
        policy,
        allowed_hosts: plan.hosts,
        timing,
        presets: preset_list(),
    }
}

/// `network.policy.get`.
pub fn get(computer_id: &str, target: Target<'_>) -> Result<NetworkPolicyState, String> {
    let (policy, plan, timing) = resolve(computer_id, &target)?;
    Ok(state(computer_id, policy, plan, timing))
}

/// `network.policy.set`: checks and saves the policy. A virtual machine applies it at its next
/// start, by restoring itself in place; a macOS or container computer at its gateway's next
/// start. A running computer keeps its rules until it restarts, and the answer says so.
pub fn set(
    computer_id: &str,
    policy: &NetworkPolicy,
    target: Target<'_>,
) -> Result<NetworkPolicyState, String> {
    let policy = validate(policy)?;
    // The computer must exist before anything is saved for it.
    resolve(computer_id, &target)?;
    store::set(computer_id, &policy)?;
    let (policy, plan, timing) = resolve(computer_id, &target)?;
    Ok(state(computer_id, policy, plan, timing))
}
