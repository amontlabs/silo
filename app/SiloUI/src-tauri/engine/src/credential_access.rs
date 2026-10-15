//! Seeing which Keychain items the engine can use, and the user-started repair of the ones
//! it was refused.
//!
//! The engine process never lets the Keychain show UI (`credential_store::forbid_interaction`).
//! A self-signed build is outside the access list of the items an earlier build made, so after
//! an engine update those items are refused until the user allows the new build. `status` shows
//! which items are refused. `repair` is the one deliberate exception: the user clicked, so it
//! lets the Keychain ask for the length of that one call, on its own thread, touches each
//! refused item so macOS shows its dialog (where the user picks Always Allow), and turns the
//! prompts off again whatever happens.
//!
//! Everything here goes through `ItemStore` and `InteractionControl`, so the tests never
//! touch a real Keychain item.

use std::sync::atomic::{AtomicBool, Ordering};

use silo_api::methods::credentials::{
    CredentialItem, CredentialKind, CredentialRepairItem, CredentialRepairOutcome, CredentialState,
    CredentialsRepairResult, CredentialsStatus,
};

use crate::channel::{Channel, Keychain};

/// One Keychain item the engine owns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemRef {
    pub kind: CredentialKind,
    pub computer_id: Option<String>,
    pub service: String,
    pub account: String,
}

impl ItemRef {
    fn label(&self) -> &'static str {
        match self.kind {
            CredentialKind::GithubSignIn => "GitHub sign-in",
            CredentialKind::GithubGrants => "GitHub tokens given to computers",
            CredentialKind::GithubPersonalToken => "GitHub personal token",
            CredentialKind::SecretsVault => "Secrets",
            CredentialKind::ProxyCa => "Network certificate of a macOS computer",
        }
    }
}

/// The engine's Keychain items for `channel`: the fixed ones, and one proxy CA per macOS
/// computer in `computer_ids`.
pub fn items(channel: Channel, computer_ids: &[String]) -> Vec<ItemRef> {
    let fixed = |kind, keychain, account: &str| ItemRef {
        kind,
        computer_id: None,
        service: channel.keychain_service(keychain).to_owned(),
        account: account.to_owned(),
    };
    let mut items = vec![
        fixed(CredentialKind::GithubSignIn, Keychain::Github, "account"),
        fixed(
            CredentialKind::GithubGrants,
            Keychain::Github,
            "runtime-grants",
        ),
        fixed(
            CredentialKind::GithubPersonalToken,
            Keychain::Github,
            "personal-token",
        ),
        fixed(CredentialKind::SecretsVault, Keychain::Secrets, "values"),
    ];
    items.extend(computer_ids.iter().map(|id| ItemRef {
        kind: CredentialKind::ProxyCa,
        computer_id: Some(id.clone()),
        service: channel.keychain_service(Keychain::ProxyCa).to_owned(),
        account: format!("computer:{id}"),
    }));
    items
}

/// What reading one item said.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Probe {
    Accessible,
    /// The engine never made it: nothing to repair.
    Absent,
    Denied,
    Failed,
}

/// Reads an item the way the engine does, to learn whether it may.
pub trait ItemStore: Sync {
    fn probe(&self, item: &ItemRef) -> Probe;
}

/// Turns Keychain prompts on or off for the whole process.
pub trait InteractionControl {
    fn set_allowed(&self, allowed: bool) -> Result<(), String>;
}

/// The real Keychain.
pub struct KeychainStore;

impl ItemStore for KeychainStore {
    fn probe(&self, item: &ItemRef) -> Probe {
        let entry = match keyring::Entry::new(&item.service, &item.account) {
            Ok(entry) => entry,
            Err(_) => return Probe::Failed,
        };
        match entry.get_password() {
            Ok(value) => {
                // Only the access matters; the value is dropped at once.
                let mut value = zeroize::Zeroizing::new(value);
                zeroize::Zeroize::zeroize(&mut *value);
                Probe::Accessible
            }
            Err(keyring::Error::NoEntry) => Probe::Absent,
            Err(error) if crate::credential_store::is_access_denied(&error) => Probe::Denied,
            Err(_) => Probe::Failed,
        }
    }
}

/// The real process-wide Keychain interaction switch.
pub struct SystemInteraction;

impl InteractionControl for SystemInteraction {
    fn set_allowed(&self, allowed: bool) -> Result<(), String> {
        if allowed {
            crate::credential_store::allow_interaction()
        } else {
            crate::credential_store::forbid_interaction()
        }
    }
}

/// Prompts are allowed while this exists and forbidden again when it is dropped: on success,
/// on an error return and on a panic.
struct InteractionWindow<'a> {
    control: &'a dyn InteractionControl,
}

impl<'a> InteractionWindow<'a> {
    fn open(control: &'a dyn InteractionControl) -> Result<Self, String> {
        if let Err(error) = control.set_allowed(true) {
            // A failed switch may be half applied; make sure prompts stay off.
            let _ = control.set_allowed(false);
            return Err(error);
        }
        Ok(Self { control })
    }
}

impl Drop for InteractionWindow<'_> {
    fn drop(&mut self) {
        // Turning prompts off matters more than any result; try again once if it fails.
        if self.control.set_allowed(false).is_err() {
            let _ = self.control.set_allowed(false);
        }
    }
}

/// The status of the items that exist, read without any prompt.
pub fn status_with(store: &dyn ItemStore, items: &[ItemRef]) -> CredentialsStatus {
    let items: Vec<CredentialItem> = items
        .iter()
        .filter_map(|item| {
            let state = match store.probe(item) {
                Probe::Absent => return None,
                Probe::Accessible => CredentialState::Accessible,
                Probe::Denied => CredentialState::Denied,
                Probe::Failed => CredentialState::Failed,
            };
            Some(CredentialItem {
                kind: item.kind,
                label: item.label().to_owned(),
                computer_id: item.computer_id.clone(),
                state,
            })
        })
        .collect();
    let denied_count = items
        .iter()
        .filter(|item| item.state == CredentialState::Denied)
        .count() as u32;
    CredentialsStatus {
        items,
        denied_count,
    }
}

/// Touches every refused item with prompts allowed, then reads the status again with prompts
/// off. Prompts are forbidden again before this returns, whatever happened.
pub fn repair_with(
    control: &dyn InteractionControl,
    store: &dyn ItemStore,
    items: &[ItemRef],
) -> Result<CredentialsRepairResult, String> {
    // Which items need a dialog is learned first, without one.
    let denied: Vec<&ItemRef> = items
        .iter()
        .filter(|item| store.probe(item) == Probe::Denied)
        .collect();
    let mut outcomes: Vec<CredentialRepairItem> = Vec::new();
    if !denied.is_empty() {
        let _window = InteractionWindow::open(control)?;
        for item in &denied {
            let outcome = match store.probe(item) {
                Probe::Accessible => CredentialRepairOutcome::Repaired,
                Probe::Denied => CredentialRepairOutcome::StillDenied,
                // Removed meanwhile: nothing left to repair.
                Probe::Absent => continue,
                Probe::Failed => CredentialRepairOutcome::Failed,
            };
            outcomes.push(CredentialRepairItem {
                kind: item.kind,
                label: item.label().to_owned(),
                computer_id: item.computer_id.clone(),
                outcome,
            });
        }
    }
    // The window is closed here. Items that were fine are reported as such.
    let status = status_with(store, items);
    let mut report: Vec<CredentialRepairItem> = status
        .items
        .iter()
        .filter(|item| {
            !outcomes
                .iter()
                .any(|done| done.kind == item.kind && done.computer_id == item.computer_id)
        })
        .map(|item| CredentialRepairItem {
            kind: item.kind,
            label: item.label.clone(),
            computer_id: item.computer_id.clone(),
            outcome: match item.state {
                CredentialState::Accessible => CredentialRepairOutcome::AlreadyAccessible,
                CredentialState::Denied => CredentialRepairOutcome::StillDenied,
                CredentialState::Failed => CredentialRepairOutcome::Failed,
            },
        })
        .collect();
    report.extend(outcomes);
    Ok(CredentialsRepairResult {
        items: report,
        status,
    })
}

/// The status of this engine's real Keychain items.
pub fn status(computer_ids: &[String]) -> CredentialsStatus {
    status_with(
        &KeychainStore,
        &items(crate::channel::current(), computer_ids),
    )
}

static REPAIRING: AtomicBool = AtomicBool::new(false);

/// The user-started repair against the real Keychain. Runs on its own thread, because a
/// dialog can wait for the user for as long as they like; one repair at a time.
pub fn repair(computer_ids: &[String]) -> Result<CredentialsRepairResult, String> {
    if REPAIRING.swap(true, Ordering::SeqCst) {
        return Err("A Keychain access repair is already running.".to_owned());
    }
    let items = items(crate::channel::current(), computer_ids);
    let result = std::thread::Builder::new()
        .name("keychain-repair".into())
        .spawn(move || repair_with(&SystemInteraction, &KeychainStore, &items))
        .map_err(|error| format!("Could not start the Keychain access repair: {error}"))
        .and_then(|thread| {
            thread
                .join()
                .map_err(|_| "The Keychain access repair stopped unexpectedly.".to_owned())
        });
    // Belt and braces: whatever the thread did, prompts are off again.
    let _ = crate::credential_store::forbid_interaction();
    REPAIRING.store(false, Ordering::SeqCst);
    result?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::{HashMap, HashSet},
        sync::Mutex,
    };

    struct Control {
        log: Mutex<Vec<bool>>,
        fail_on: Option<bool>,
    }
    impl Control {
        fn new() -> Self {
            Self {
                log: Mutex::new(Vec::new()),
                fail_on: None,
            }
        }
        fn log(&self) -> Vec<bool> {
            self.log.lock().unwrap().clone()
        }
    }
    impl InteractionControl for Control {
        fn set_allowed(&self, allowed: bool) -> Result<(), String> {
            self.log.lock().unwrap().push(allowed);
            if self.fail_on == Some(allowed) {
                return Err("refused".into());
            }
            Ok(())
        }
    }

    /// Answers per account from a script, like a Keychain: what the item says without
    /// prompts, what it says when asked with prompts on, and an allowed item stays allowed.
    struct Store<'a> {
        control: &'a Control,
        /// account -> (answer without prompts, answer with prompts)
        items: HashMap<String, (Probe, Probe)>,
        granted: Mutex<HashSet<String>>,
        seen_allowed: Mutex<Vec<(String, bool)>>,
    }
    impl ItemStore for Store<'_> {
        fn probe(&self, item: &ItemRef) -> Probe {
            let allowed = self.control.log().last().copied().unwrap_or(false);
            self.seen_allowed
                .lock()
                .unwrap()
                .push((item.account.clone(), allowed));
            let (quiet, prompted) = self.items[&item.account];
            if allowed {
                if prompted == Probe::Accessible {
                    self.granted.lock().unwrap().insert(item.account.clone());
                }
                prompted
            } else if self.granted.lock().unwrap().contains(&item.account) {
                Probe::Accessible
            } else {
                quiet
            }
        }
    }

    fn store<'a>(control: &'a Control, items: &[(&str, Probe, Probe)]) -> Store<'a> {
        Store {
            control,
            items: items
                .iter()
                .map(|(a, q, p)| (a.to_string(), (*q, *p)))
                .collect(),
            granted: Mutex::new(HashSet::new()),
            seen_allowed: Mutex::new(Vec::new()),
        }
    }

    fn all_items() -> Vec<ItemRef> {
        items(Channel::Development, &["c1".to_owned()])
    }

    #[test]
    fn the_items_are_the_channels_fixed_ones_and_one_proxy_ca_per_computer() {
        let list = all_items();
        let names: Vec<_> = list
            .iter()
            .map(|item| (item.service.as_str(), item.account.as_str()))
            .collect();
        assert_eq!(
            names,
            vec![
                ("org.silo.dev.github", "account"),
                ("org.silo.dev.github", "runtime-grants"),
                ("org.silo.dev.github", "personal-token"),
                ("org.silo.dev.secrets", "values"),
                ("org.silo.dev.proxy-ca", "computer:c1"),
            ]
        );
        assert_eq!(list[4].computer_id.as_deref(), Some("c1"));
    }

    #[test]
    fn the_status_lists_existing_items_and_counts_the_denied() {
        let control = Control::new();
        let store = store(
            &control,
            &[
                ("account", Probe::Accessible, Probe::Accessible),
                ("runtime-grants", Probe::Absent, Probe::Absent),
                ("personal-token", Probe::Failed, Probe::Failed),
                ("values", Probe::Denied, Probe::Accessible),
                ("computer:c1", Probe::Denied, Probe::Denied),
            ],
        );
        let status = status_with(&store, &all_items());
        let states: Vec<_> = status
            .items
            .iter()
            .map(|item| (item.kind, item.state))
            .collect();
        assert_eq!(
            states,
            vec![
                (CredentialKind::GithubSignIn, CredentialState::Accessible),
                (CredentialKind::GithubPersonalToken, CredentialState::Failed),
                (CredentialKind::SecretsVault, CredentialState::Denied),
                (CredentialKind::ProxyCa, CredentialState::Denied),
            ]
        );
        assert_eq!(status.denied_count, 2);
        assert_eq!(status.items[3].computer_id.as_deref(), Some("c1"));
        assert!(
            control.log().is_empty(),
            "a status never touches the prompt switch"
        );
    }

    #[test]
    fn a_repair_allows_prompts_only_while_it_touches_refused_items() {
        let control = Control::new();
        let store = store(
            &control,
            &[
                ("account", Probe::Accessible, Probe::Accessible),
                ("runtime-grants", Probe::Absent, Probe::Absent),
                ("personal-token", Probe::Absent, Probe::Absent),
                ("values", Probe::Denied, Probe::Accessible),
                ("computer:c1", Probe::Denied, Probe::Denied),
            ],
        );
        let result = repair_with(&control, &store, &all_items()).unwrap();
        assert_eq!(control.log(), vec![true, false]);
        let outcome = |kind| {
            result
                .items
                .iter()
                .find(|item| item.kind == kind)
                .unwrap()
                .outcome
        };
        assert_eq!(
            outcome(CredentialKind::SecretsVault),
            CredentialRepairOutcome::Repaired
        );
        assert_eq!(
            outcome(CredentialKind::ProxyCa),
            CredentialRepairOutcome::StillDenied
        );
        assert_eq!(
            outcome(CredentialKind::GithubSignIn),
            CredentialRepairOutcome::AlreadyAccessible
        );
        // The status after the repair is read with prompts off: the allowed vault stays
        // readable, the declined certificate stays refused.
        assert_eq!(result.status.denied_count, 1);
        // Only refused items were touched while prompts were on.
        let prompted: Vec<_> = store
            .seen_allowed
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, allowed)| *allowed)
            .map(|(account, _)| account.clone())
            .collect();
        assert_eq!(prompted, vec!["values", "computer:c1"]);
    }

    #[test]
    fn a_repair_with_nothing_refused_never_allows_prompts() {
        let control = Control::new();
        let store = store(
            &control,
            &[
                ("account", Probe::Accessible, Probe::Accessible),
                ("runtime-grants", Probe::Absent, Probe::Absent),
                ("personal-token", Probe::Absent, Probe::Absent),
                ("values", Probe::Accessible, Probe::Accessible),
                ("computer:c1", Probe::Accessible, Probe::Accessible),
            ],
        );
        let result = repair_with(&control, &store, &all_items()).unwrap();
        assert!(control.log().is_empty());
        assert_eq!(result.status.denied_count, 0);
        assert!(result
            .items
            .iter()
            .all(|item| item.outcome == CredentialRepairOutcome::AlreadyAccessible));
    }

    #[test]
    fn prompts_are_forbidden_again_when_the_window_ends_by_an_error_or_a_panic() {
        // Opening that fails leaves prompts off.
        let mut control = Control::new();
        control.fail_on = Some(true);
        let failing = store(
            &control,
            &[
                ("account", Probe::Denied, Probe::Denied),
                ("runtime-grants", Probe::Absent, Probe::Absent),
                ("personal-token", Probe::Absent, Probe::Absent),
                ("values", Probe::Absent, Probe::Absent),
                ("computer:c1", Probe::Absent, Probe::Absent),
            ],
        );
        assert!(repair_with(&control, &failing, &all_items()).is_err());
        assert_eq!(control.log(), vec![true, false]);

        // A panic inside the window unwinds through the guard.
        let control = Control::new();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _window = InteractionWindow::open(&control).unwrap();
            panic!("a Keychain call blew up");
        }));
        assert!(result.is_err());
        assert_eq!(control.log(), vec![true, false]);

        // A failing switch-off is retried once.
        let mut control = Control::new();
        control.fail_on = Some(false);
        drop(InteractionWindow::open(&control).unwrap());
        assert_eq!(control.log(), vec![true, false, false]);
    }
}
