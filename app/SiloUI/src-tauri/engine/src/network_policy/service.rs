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

use super::store::Stored;
use super::{
    effective, extra_rules, plan_held, preset_list, store, validate, Grants, Held, NetworkPolicy,
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

/// What a computer runs with and when it is in force.
struct Resolved {
    /// What is saved for the computer, when anything is.
    stored: Option<Stored>,
    /// The policy it runs with and the preset versions it was approved at.
    held: Held,
    plan: Plan,
    grants: Grants,
    timing: NetworkPolicyTiming,
}

/// The policy a computer runs with, its plan and when the plan is in force. The policy is the
/// stored one, else what the runtime's recorded rules mean, and a container the engine blocked
/// is closed whatever is stored. An unreadable store is an error, never an open computer.
fn resolve(computer_id: &str, target: &Target<'_>) -> Result<Resolved, String> {
    let stored = store::get_stored(computer_id)?;
    match target {
        Target::Linux(paths) => {
            let view = crate::runtime::checkpoints::network_policy_view(paths, computer_id)?;
            Ok(resolve_linux(computer_id, stored, view))
        }
        Target::Macos { name, running } => {
            let held = match &stored {
                Some(stored) => stored.held(),
                None => Held::latest(effective(None, None)),
            };
            let grants = Grants::of_computer(name);
            let plan = plan_held(&held, &grants, &extra_rules(computer_id));
            let matches = gateway_matches(computer_id, &plan);
            let timing = timing(*running, matches);
            Ok(Resolved {
                stored,
                held,
                plan,
                grants,
                timing,
            })
        }
    }
}

/// [`resolve`] for a Linux computer, from what the runtime says about it.
fn resolve_linux(
    computer_id: &str,
    stored: Option<Stored>,
    view: crate::runtime::checkpoints::NetworkPolicyView,
) -> Resolved {
    let runtime_policy = view
        .network
        .as_ref()
        .and_then(|network| network.get("policy"));
    let recorded = runtime_policy.or(view.recorded.as_ref());
    let blocked = view.container
        && runtime_policy
            .is_some_and(|policy| policy == &crate::containers::secrets::blocked_policy());
    let held = if blocked {
        Held::latest(closed())
    } else {
        match &stored {
            Some(stored) => stored.held(),
            None => Held::latest(effective(None, recorded)),
        }
    };
    let grants = Grants::of_computer(&view.name);
    let plan = plan_held(&held, &grants, &extra_rules(computer_id));
    let matches = if view.container {
        gateway_matches(computer_id, &plan)
    } else {
        view.network
            .as_ref()
            .is_some_and(|network| plan.matches(network))
    };
    let timing = timing(view.running && !view.pending, matches);
    Resolved {
        stored,
        held,
        plan,
        grants,
        timing,
    }
}

/// The policy the engine judges a Linux computer's calls against, from what the runtime says
/// about it.
#[cfg(test)]
pub(crate) fn judged_from_view(
    computer_id: &str,
    stored: Option<Stored>,
    view: crate::runtime::checkpoints::NetworkPolicyView,
) -> Held {
    resolve_linux(computer_id, stored, view).held
}

/// The policy the engine judges a computer's calls against: exactly what `network.policy.get`
/// reports. `None` when it cannot be read, so a check against it asks.
pub fn judged(computer_id: &str, target: Target<'_>) -> Option<Held> {
    resolve(computer_id, &target).ok().map(|resolved| resolved.held)
}

fn state(computer_id: &str, resolved: Resolved) -> NetworkPolicyState {
    let notices = resolved
        .grants
        .too_wide()
        .into_iter()
        .map(|(secret, host)| {
            format!(
                "The secret {secret} may be sent to {host}, which covers too many hosts to allow here, so a list does not let this computer reach them."
            )
        })
        .collect();
    NetworkPolicyState {
        computer_id: computer_id.to_owned(),
        preset_updates: resolved
            .held
            .preset_updates()
            .into_iter()
            .chain(resolved.held.grant_updates(&resolved.grants))
            .collect(),
        policy: resolved.held.policy,
        allowed_hosts: resolved.plan.hosts,
        timing: resolved.timing,
        presets: preset_list(),
        notices,
    }
}

/// `network.policy.get`.
pub fn get(computer_id: &str, target: Target<'_>) -> Result<NetworkPolicyState, String> {
    Ok(state(computer_id, resolve(computer_id, &target)?))
}

/// `network.policy.set`: checks and saves the policy, approved now (so it also accepts any
/// preset update). A virtual machine applies it at its next start, by restoring itself in place;
/// a macOS or container computer at its gateway's next start. A running computer keeps its rules
/// until it restarts, and the answer says so.
///
/// With `expected`, the policy is saved only if the computer still has that policy; the engine
/// gives it for a computer's agent narrowing its own network, which was judged against it.
pub fn set(
    computer_id: &str,
    policy: &NetworkPolicy,
    expected: Option<&NetworkPolicy>,
    accept_presets: Option<bool>,
    target: Target<'_>,
) -> Result<NetworkPolicyState, String> {
    let policy = validate(policy)?;
    // The computer must exist before anything is saved for it.
    let before = resolve(computer_id, &target)?;
    let incoming_denied = before
        .stored
        .as_ref()
        .is_some_and(|stored| stored.incoming_denied);
    let stored = if accept_presets.unwrap_or(false) {
        let mut s = Stored::approved(&policy);
        s.incoming_denied = incoming_denied;
        s
    } else {
        Stored::preserving_pins(&policy, before.stored.as_ref(), incoming_denied)
    };
    match expected {
        None => store::set_stored(computer_id, stored)?,
        Some(expected) => {
            if &before.held.policy != expected {
                return Err(store::CHANGED.to_owned());
            }
            store::replace_if(computer_id, before.stored.as_ref(), stored)?;
        }
    }
    Ok(state(computer_id, resolve(computer_id, &target)?))
}
