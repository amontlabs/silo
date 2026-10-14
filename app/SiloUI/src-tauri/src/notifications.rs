//! One router for everything Silo tells the user outside a direct UI response.
//!
//! Ownership (who produces a notice for what):
//! - The frontend toast layer owns results of commands it awaited (push, GitHub apply,
//!   ports, checkpoints, storage reclaim, log export). It mirrors failures and long
//!   successes to the system through `deliver_notice`; it already shows its own toast.
//! - The backend owns background work and lifecycle results (computer start/stop/restart,
//!   export/import, computer setup, startup, updates) through `notify_native`: the
//!   frontend shows toasts for those from backend state.
//! - The backend owns events nobody asked for (unexpected computer changes, startup
//!   failures without a UI owner) through `notify`: an in-app toast plus a system notice.
//!
//! Policy lives here: category preferences, OS authorization, and focus. A system
//! notification is never shown while the main window is focused; the in-app toast
//! already covers it.
use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, Weak};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::system_integrations::NotificationDelivery;

/// In-app toast event for backend-originated notices. Payload: `Notice`.
pub(crate) const NOTICE_EVENT: &str = "silo://notice";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Category {
    /// Something the user started, or background work, failed.
    Failures,
    /// A computer changed state without a Silo operation causing it.
    Changes,
    /// Work that ran for at least `LONG_OPERATION` finished successfully.
    Completions,
}

/// A successful operation this long or longer is worth a completion notice. Matches the
/// frontend `LONG_OPERATION_MS`.
pub(crate) const LONG_OPERATION: std::time::Duration = std::time::Duration::from_secs(3);

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NoticeComputer {
    /// Stable computer id, qualified by device when remote. Routes, groups and clears notices.
    pub id: String,
    /// Display name, shown in notification text.
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Notice {
    pub category: Category,
    /// Stable identity. A newer notice with the same key replaces the older one, both
    /// as a system notification and as an in-app toast.
    pub key: String,
    pub title: String,
    pub body: String,
    pub computer: Option<NoticeComputer>,
}

impl Category {
    /// Stable name, used to group system notifications that have no computer.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Failures => "failures",
            Self::Changes => "changes",
            Self::Completions => "completions",
        }
    }
}

impl Notice {
    /// Where a click on the system notification opens the app. Same shape as
    /// `desktop/use-main-route.ts`.
    pub(crate) fn route(&self) -> Value {
        match &self.computer {
            Some(computer) => serde_json::json!({"tab": "computers", "computer": computer.id}),
            None => serde_json::json!({"tab": "computers"}),
        }
    }

    /// Notifications with the same thread collapse together in Notification Center.
    pub(crate) fn thread(&self) -> &str {
        self.computer
            .as_ref()
            .map_or(self.category.as_str(), |computer| computer.id.as_str())
    }
}

fn flag(settings: &Map<String, Value>, key: &str, legacy: &[&str]) -> bool {
    // New keys win; before they are saved, the legacy per-area switches decide.
    if let Some(value) = settings.get(key).and_then(Value::as_bool) {
        return value;
    }
    legacy
        .iter()
        .all(|key| settings.get(*key).and_then(Value::as_bool).unwrap_or(true))
}

pub(crate) fn enabled(settings: &Map<String, Value>, category: Category) -> bool {
    // Match the persisted settings defaults. OS authorization is checked separately.
    flag(settings, "notificationsEnabled", &[])
        && match category {
            Category::Failures => flag(
                settings,
                "notifyFailures",
                &["notifyActions", "notifyBackup"],
            ),
            Category::Changes => flag(settings, "notifyChanges", &["notifyHealth"]),
            Category::Completions => flag(settings, "notifyCompletions", &[]),
        }
}

/// Submitted and queued notification keys by computer id, so deletion withdraws both.
#[derive(Default)]
struct DeliveredIndex {
    keys: HashMap<String, BTreeSet<String>>,
    gates: HashMap<String, Weak<NoticeGate>>,
}

impl DeliveredIndex {
    fn record(&mut self, notice: &Notice) {
        if let Some(computer) = &notice.computer {
            self.keys
                .entry(computer.id.clone())
                .or_default()
                .insert(notice.key.clone());
        }
    }
    fn take(&mut self, computer_id: &str) -> Vec<String> {
        self.keys
            .remove(computer_id)
            .map(|keys| keys.into_iter().collect())
            .unwrap_or_default()
    }
}

/// Serialize OS calls for a key without holding the index lock across callbacks.
#[derive(Default)]
struct NoticeGate {
    submitted: Mutex<u64>,
    revision: AtomicU64,
    cleared_through: AtomicU64,
}

pub(crate) struct PendingDelivery {
    notice: Notice,
    gate: Arc<NoticeGate>,
    revision: u64,
}

impl DeliveredIndex {
    fn gate(&mut self, key: &str) -> Arc<NoticeGate> {
        self.gates.retain(|_, gate| gate.strong_count() > 0);
        if let Some(gate) = self.gates.get(key).and_then(Weak::upgrade) {
            return gate;
        }
        let gate = Arc::new(NoticeGate::default());
        self.gates.insert(key.into(), Arc::downgrade(&gate));
        gate
    }

    fn prepare(&mut self, mut notice: Notice) -> PendingDelivery {
        notice.body = bounded_body(&notice.body);
        // Withdrawal must include keys whose OS delivery has not finished yet.
        self.record(&notice);
        let gate = self.gate(&notice.key);
        let revision = gate.revision.fetch_add(1, Ordering::SeqCst) + 1;
        PendingDelivery {
            notice,
            gate,
            revision,
        }
    }

    fn withdraw(&mut self, computer_id: &str) -> PendingWithdrawal {
        let notices = self
            .take(computer_id)
            .into_iter()
            .map(|key| {
                let gate = self.gate(&key);
                let revision = gate.revision.fetch_add(1, Ordering::SeqCst) + 1;
                gate.cleared_through.store(revision, Ordering::SeqCst);
                (key, gate, revision)
            })
            .collect();
        PendingWithdrawal { notices }
    }
}

impl PendingDelivery {
    fn cancelled(&self) -> bool {
        self.revision <= self.gate.cleared_through.load(Ordering::SeqCst)
    }

    pub(crate) fn deliver(
        self,
        allowed: impl FnOnce(&Notice) -> bool,
        send: impl FnOnce(&Notice) -> Result<NotificationDelivery, String>,
        clear: impl FnOnce(&[String]),
    ) {
        let mut submitted = self
            .gate
            .submitted
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        // Task polling and mutex acquisition need not follow issuance order.
        if self.cancelled() || self.revision != self.gate.revision.load(Ordering::SeqCst) {
            return;
        }
        // Focus and preferences may change while an earlier OS request owns the gate.
        if !allowed(&self.notice)
            || self.cancelled()
            || self.revision != self.gate.revision.load(Ordering::SeqCst)
        {
            return;
        }
        let result = send(&self.notice);
        if self.cancelled() {
            // Deletion can invalidate this request while the OS callback is pending.
            clear(&[self.notice.key]);
        } else if matches!(result, Ok(NotificationDelivery::Delivered)) {
            *submitted = self.revision;
        }
        if result.is_err() {
            eprintln!("Silo could not deliver a system notification.");
        }
    }
}

static DELIVERED: LazyLock<Mutex<DeliveredIndex>> = LazyLock::new(Default::default);

pub(crate) fn prepare_delivery(notice: Notice) -> PendingDelivery {
    DELIVERED
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .prepare(notice)
}

pub(crate) fn withdraw_computer(computer_id: &str) -> PendingWithdrawal {
    DELIVERED
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .withdraw(computer_id)
}

pub(crate) struct PendingWithdrawal {
    notices: Vec<(String, Arc<NoticeGate>, u64)>,
}

impl PendingWithdrawal {
    pub(crate) fn clear(self, mut clear: impl FnMut(&[String])) {
        for (key, gate, revision) in self.notices {
            let submitted = gate
                .submitted
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            // A later notice for this key must survive an older withdrawal task.
            if *submitted <= revision {
                clear(&[key]);
            }
        }
    }
}

pub(crate) use crate::commands::notifications::{clear_computer, notify, notify_native};

/// Longest body shown in a system notification. Full detail stays in the app.
const BODY_LIMIT: usize = 200;

/// One-line, bounded text for a system notification body.
pub(crate) fn bounded_body(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= BODY_LIMIT {
        return collapsed;
    }
    let mut cut: String = collapsed.chars().take(BODY_LIMIT - 1).collect();
    cut.truncate(cut.trim_end().len());
    cut.push('\u{2026}');
    cut
}

/// What a lifecycle (start/stop/restart) operation ended as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Outcome<'a> {
    Succeeded,
    /// The user cancelled. An expected result, never a notice.
    Cancelled,
    /// A duplicate request was handed to the one already queued. Never a notice.
    AlreadyQueued,
    Failed(&'a str),
}

fn seconds(elapsed: Duration) -> String {
    format!("Took {} seconds.", elapsed.as_secs())
}

/// Notice for a computer start, stop or restart, local or remote. Failures always notify;
/// successes only when the operation was long enough that the user likely looked away.
pub(crate) fn lifecycle_notice(
    action: &str,
    name: &str,
    computer: Option<NoticeComputer>,
    elapsed: Duration,
    outcome: Outcome<'_>,
) -> Option<Notice> {
    let (verb, done) = match action {
        "start" => ("start", format!("{name} is running")),
        "stop" => ("stop", format!("{name} stopped")),
        "restart" => ("restart", format!("{name} restarted")),
        _ => return None,
    };
    let key = computer.as_ref().map_or_else(
        || format!("lifecycle:{name}"),
        |s| format!("computer:{}:lifecycle", s.id),
    );
    match outcome {
        Outcome::Cancelled | Outcome::AlreadyQueued => None,
        Outcome::Failed(message) => Some(Notice {
            category: Category::Failures,
            key,
            title: format!("Couldn\u{2019}t {verb} {name}"),
            body: bounded_body(message),
            computer,
        }),
        Outcome::Succeeded if elapsed >= LONG_OPERATION => Some(Notice {
            category: Category::Completions,
            key,
            title: done,
            body: seconds(elapsed),
            computer,
        }),
        Outcome::Succeeded => None,
    }
}

/// Notice for an export (`backup`) or import (`restore`).
pub(crate) fn transfer_notice(
    operation: &str,
    label: &str,
    computer: Option<NoticeComputer>,
    elapsed: Duration,
    outcome: &str,
    message: &str,
) -> Option<Notice> {
    let (noun, done) = match operation {
        "backup" => ("export", "Exported"),
        "restore" => ("import", "Imported"),
        _ => return None,
    };
    let key = computer.as_ref().map_or_else(
        || "transfer".to_string(),
        |s| format!("computer:{}:transfer", s.id),
    );
    let (category, title, body) = match outcome {
        "failed" => (
            Category::Failures,
            format!("Couldn\u{2019}t {noun} {label}"),
            bounded_body(message),
        ),
        "success" if elapsed >= LONG_OPERATION => (
            Category::Completions,
            format!("{done} {label}"),
            seconds(elapsed),
        ),
        _ => return None,
    };
    Some(Notice {
        category,
        key,
        title,
        body,
        computer,
    })
}

/// A failure notice with a bounded body.
pub(crate) fn failure(
    key: &str,
    title: &str,
    message: &str,
    computer: Option<NoticeComputer>,
) -> Notice {
    Notice {
        category: Category::Failures,
        key: key.into(),
        title: title.into(),
        body: bounded_body(message),
        computer,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn settings(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }
    const ALL: [Category; 3] = [Category::Failures, Category::Changes, Category::Completions];
    #[test]
    fn master_and_each_category_preference_gate_delivery() {
        for category in ALL {
            assert!(enabled(&settings(json!({})), category));
            assert!(!enabled(
                &settings(json!({"notificationsEnabled": false})),
                category
            ));
        }
        let preferences = settings(
            json!({"notifyFailures": false, "notifyChanges": false, "notifyCompletions": false}),
        );
        for category in ALL {
            assert!(!enabled(&preferences, category));
        }
        assert!(enabled(
            &settings(json!({"notifyChanges": false})),
            Category::Failures
        ));
    }
    #[test]
    fn legacy_switches_apply_until_new_keys_are_saved() {
        assert!(!enabled(
            &settings(json!({"notifyHealth": false})),
            Category::Changes
        ));
        assert!(!enabled(
            &settings(json!({"notifyBackup": false})),
            Category::Failures
        ));
        assert!(enabled(
            &settings(json!({"notifyBackup": false, "notifyFailures": true})),
            Category::Failures
        ));
    }
    #[test]
    fn notice_wire_shape_is_camel_case() {
        let notice = Notice {
            category: Category::Completions,
            key: "computer:1:lifecycle".into(),
            title: "dev is running".into(),
            body: "".into(),
            computer: Some(NoticeComputer {
                id: "1".into(),
                name: "dev".into(),
            }),
        };
        assert_eq!(
            serde_json::to_value(&notice).unwrap(),
            json!({"category": "completions", "key": "computer:1:lifecycle", "title": "dev is running", "body": "", "computer": {"id": "1", "name": "dev"}})
        );
    }
    fn computer() -> Option<NoticeComputer> {
        Some(NoticeComputer {
            id: "1".into(),
            name: "dev".into(),
        })
    }
    const SHORT: Duration = Duration::from_secs(1);
    const LONG: Duration = LONG_OPERATION;
    #[test]
    fn body_is_one_bounded_line() {
        assert_eq!(
            bounded_body("  first\n second\t line "),
            "first second line"
        );
        let body = bounded_body(&"x".repeat(500));
        assert_eq!(body.chars().count(), BODY_LIMIT);
        assert!(body.ends_with('\u{2026}'));
        assert_eq!(
            bounded_body(&"y".repeat(BODY_LIMIT)).chars().count(),
            BODY_LIMIT
        );
    }
    #[test]
    fn lifecycle_failures_name_the_computer_and_share_a_key() {
        let notice = lifecycle_notice(
            "start",
            "dev",
            computer(),
            SHORT,
            Outcome::Failed("no memory\nleft"),
        )
        .unwrap();
        assert_eq!(notice.category, Category::Failures);
        assert_eq!(notice.title, "Couldn\u{2019}t start dev");
        assert_eq!(notice.body, "no memory left");
        assert_eq!(notice.key, "computer:1:lifecycle");
        let done = lifecycle_notice("stop", "dev", computer(), LONG, Outcome::Succeeded).unwrap();
        assert_eq!(done.key, notice.key);
        assert_eq!(done.category, Category::Completions);
        assert_eq!(done.title, "dev stopped");
        assert_eq!(
            lifecycle_notice("restart", "dev", computer(), LONG, Outcome::Succeeded)
                .unwrap()
                .title,
            "dev restarted"
        );
        assert_eq!(
            lifecycle_notice("start", "dev", computer(), LONG, Outcome::Succeeded)
                .unwrap()
                .title,
            "dev is running"
        );
    }
    #[test]
    fn lifecycle_successes_notify_only_after_the_long_threshold() {
        assert!(lifecycle_notice("start", "dev", computer(), SHORT, Outcome::Succeeded).is_none());
        assert!(lifecycle_notice(
            "start",
            "dev",
            computer(),
            LONG - Duration::from_millis(1),
            Outcome::Succeeded
        )
        .is_none());
        assert!(lifecycle_notice("start", "dev", computer(), LONG, Outcome::Succeeded).is_some());
    }
    #[test]
    fn cancellation_queue_dedupe_and_dismiss_never_notify() {
        for elapsed in [SHORT, LONG] {
            assert!(
                lifecycle_notice("start", "dev", computer(), elapsed, Outcome::Cancelled).is_none()
            );
            assert!(
                lifecycle_notice("start", "dev", computer(), elapsed, Outcome::AlreadyQueued)
                    .is_none()
            );
            assert!(lifecycle_notice(
                "dismiss-error",
                "dev",
                computer(),
                elapsed,
                Outcome::Succeeded
            )
            .is_none());
            assert!(lifecycle_notice(
                "dismiss-error",
                "dev",
                computer(),
                elapsed,
                Outcome::Failed("x")
            )
            .is_none());
        }
    }
    #[test]
    fn lifecycle_without_a_known_id_still_has_a_stable_key() {
        let notice = lifecycle_notice("start", "dev", None, SHORT, Outcome::Failed("x")).unwrap();
        assert_eq!(notice.key, "lifecycle:dev");
        assert!(notice.computer.is_none());
    }
    #[test]
    fn transfer_notices_follow_the_outcome() {
        let failed =
            transfer_notice("backup", "dev", computer(), SHORT, "failed", "disk full").unwrap();
        assert_eq!(
            (failed.category, failed.title.as_str(), failed.key.as_str()),
            (
                Category::Failures,
                "Couldn\u{2019}t export dev",
                "computer:1:transfer"
            )
        );
        assert_eq!(failed.body, "disk full");
        assert!(transfer_notice("restore", "dev", computer(), SHORT, "success", "").is_none());
        let long = transfer_notice("restore", "dev", computer(), LONG, "success", "").unwrap();
        assert_eq!(
            (long.category, long.title.as_str()),
            (Category::Completions, "Imported dev")
        );
        assert_eq!(
            transfer_notice("backup", "3 computers", None, LONG, "success", "")
                .unwrap()
                .key,
            "transfer"
        );
        for operation in ["backup", "restore"] {
            // Exports no longer stop computers, so there is no restart outcome to report.
            for outcome in ["cancelled", "running", "restart-required"] {
                assert!(
                    transfer_notice(operation, "dev", computer(), LONG, outcome, "x").is_none()
                );
            }
        }
    }
    #[test]
    fn click_routes_open_the_computer_or_the_list() {
        let mut notice = failure("k", "t", "b", computer());
        assert_eq!(notice.route(), json!({"tab": "computers", "computer": "1"}));
        notice.computer.as_mut().unwrap().name = "renamed".into();
        assert_eq!(notice.route(), json!({"tab": "computers", "computer": "1"}));
        assert_eq!(notice.thread(), "1");
        notice.computer = None;
        assert_eq!(notice.route(), json!({"tab": "computers"}));
        assert_eq!(notice.thread(), "failures");
    }
    #[test]
    fn clearing_remote_notices_preserves_other_devices_with_the_same_computer_id() {
        let mut index = DeliveredIndex::default();
        for id in [
            "same-id",
            "silo-remote:office:same-id",
            "silo-remote:lab:same-id",
        ] {
            index.record(&failure(
                &format!("computer:{id}:lifecycle"),
                "dev is running",
                "",
                Some(NoticeComputer {
                    id: id.into(),
                    name: "dev".into(),
                }),
            ));
        }
        assert_eq!(
            index.take("silo-remote:office:same-id"),
            ["computer:silo-remote:office:same-id:lifecycle"]
        );
        assert!(index.take("silo-remote:office:same-id").is_empty());
        assert_eq!(index.take("same-id"), ["computer:same-id:lifecycle"]);
        assert_eq!(
            index.take("silo-remote:lab:same-id"),
            ["computer:silo-remote:lab:same-id:lifecycle"]
        );
    }

    #[test]
    fn frontend_notice_bodies_are_bounded_before_system_delivery() {
        let mut notice = failure("computer:1:lifecycle", "t", "b", computer());
        // Frontend mirrors deserialize Notice directly rather than using failure().
        notice.body = format!("first\nsecond\t{}", "x".repeat(500));
        let pending = DeliveredIndex::default().prepare(notice);
        pending.deliver(
            |_| true,
            |notice| {
                assert!(notice.body.starts_with("first second "));
                assert_eq!(notice.body.chars().count(), BODY_LIMIT);
                assert!(notice.body.ends_with('\u{2026}'));
                Ok(NotificationDelivery::Delivered)
            },
            |_| {},
        );
    }

    #[test]
    fn deletion_withdraws_a_notice_whose_delivery_is_in_flight() {
        use std::sync::{mpsc, Arc};
        let index = Arc::new(Mutex::new(DeliveredIndex::default()));
        let active = Arc::new(Mutex::new(BTreeSet::<String>::new()));
        let pending =
            index
                .lock()
                .unwrap()
                .prepare(failure("computer:1:lifecycle", "t", "b", computer()));
        let (entered, receiving) = mpsc::channel();
        let (release, waiting) = mpsc::channel();
        let worker_active = active.clone();
        let clearing_active = active.clone();
        let worker = std::thread::spawn(move || {
            pending.deliver(
                |_| true,
                |notice| {
                    entered.send(()).unwrap();
                    waiting.recv_timeout(Duration::from_secs(5)).unwrap();
                    worker_active.lock().unwrap().insert(notice.key.clone());
                    Ok(NotificationDelivery::Delivered)
                },
                |keys| {
                    let mut active = clearing_active.lock().unwrap();
                    for key in keys {
                        active.remove(key);
                    }
                },
            );
        });
        receiving.recv_timeout(Duration::from_secs(5)).unwrap();
        let withdrawal = index.lock().unwrap().withdraw("1");
        release.send(()).unwrap();
        worker.join().unwrap();
        withdrawal.clear(|keys| {
            let mut active = active.lock().unwrap();
            for key in keys {
                active.remove(key);
            }
        });
        assert!(
            active.lock().unwrap().is_empty(),
            "deleted computer still has an OS notification"
        );
        assert!(index.lock().unwrap().take("1").is_empty());
    }

    #[test]
    fn deletion_cancels_a_notice_before_os_submission() {
        let index = Mutex::new(DeliveredIndex::default());
        let pending =
            index
                .lock()
                .unwrap()
                .prepare(failure("computer:1:lifecycle", "t", "b", computer()));
        index.lock().unwrap().withdraw("1").clear(|_| {});
        let submitted = std::cell::Cell::new(false);
        pending.deliver(
            |_| true,
            |_| {
                submitted.set(true);
                Ok(NotificationDelivery::Delivered)
            },
            |_| {},
        );
        assert!(!submitted.get(), "queued notice submitted after deletion");
    }

    #[test]
    fn reversed_delivery_tasks_cannot_replace_a_newer_notice() {
        let mut index = DeliveredIndex::default();
        let older = index.prepare(failure("computer:1:lifecycle", "older", "b", computer()));
        let newer = index.prepare(failure("computer:1:lifecycle", "newer", "b", computer()));
        let visible = std::cell::RefCell::new(String::new());
        newer.deliver(
            |_| true,
            |notice| {
                *visible.borrow_mut() = notice.title.clone();
                Ok(NotificationDelivery::Delivered)
            },
            |_| {},
        );
        older.deliver(
            |_| true,
            |notice| {
                *visible.borrow_mut() = notice.title.clone();
                Ok(NotificationDelivery::Delivered)
            },
            |_| {},
        );
        assert_eq!(visible.into_inner(), "newer");
    }

    #[test]
    fn notification_delivery_does_not_block_a_different_key() {
        let mut index = DeliveredIndex::default();
        let first = index.prepare(failure("computer:1:lifecycle", "t", "b", computer()));
        let second = index.prepare(failure(
            "computer:2:lifecycle",
            "t",
            "b",
            Some(NoticeComputer {
                id: "2".into(),
                name: "second".into(),
            }),
        ));
        let submissions = std::cell::Cell::new(0);
        first.deliver(
            |_| true,
            |_| {
                second.deliver(
                    |_| true,
                    |_| {
                        submissions.set(submissions.get() + 1);
                        Ok(NotificationDelivery::Delivered)
                    },
                    |_| {},
                );
                submissions.set(submissions.get() + 1);
                Ok(NotificationDelivery::Delivered)
            },
            |_| {},
        );
        assert_eq!(submissions.get(), 2);
    }

    #[test]
    fn queued_delivery_rechecks_preferences_after_an_in_flight_notice() {
        use std::sync::mpsc;
        let mut index = DeliveredIndex::default();
        let older = index.prepare(failure("computer:1:lifecycle", "older", "b", computer()));
        let submissions = Arc::new(Mutex::new(Vec::new()));
        let worker_submissions = submissions.clone();
        let (entered, wait_entered) = mpsc::channel();
        let (release, wait_release) = mpsc::channel();
        let first = std::thread::spawn(move || {
            older.deliver(
                |_| true,
                |notice| {
                    entered.send(()).unwrap();
                    wait_release.recv_timeout(Duration::from_secs(5)).unwrap();
                    worker_submissions
                        .lock()
                        .unwrap()
                        .push(notice.title.clone());
                    Ok(NotificationDelivery::Delivered)
                },
                |_| {},
            );
        });
        wait_entered.recv_timeout(Duration::from_secs(5)).unwrap();
        let newer = index.prepare(failure("computer:1:lifecycle", "newer", "b", computer()));
        let preferences_enabled = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let worker_preferences = preferences_enabled.clone();
        let worker_submissions = submissions.clone();
        let (started, wait_started) = mpsc::channel();
        let (policy_read, wait_policy_read) = mpsc::channel();
        let second = std::thread::spawn(move || {
            started.send(()).unwrap();
            newer.deliver(
                |_| {
                    let enabled = worker_preferences.load(Ordering::SeqCst);
                    policy_read.send(()).unwrap();
                    enabled
                },
                |notice| {
                    worker_submissions
                        .lock()
                        .unwrap()
                        .push(notice.title.clone());
                    Ok(NotificationDelivery::Delivered)
                },
                |_| {},
            );
        });
        wait_started.recv_timeout(Duration::from_secs(5)).unwrap();
        // The old path reads preferences before waiting; the fixed path waits first.
        let _ = wait_policy_read.recv_timeout(Duration::from_millis(100));
        preferences_enabled.store(false, Ordering::SeqCst);
        release.send(()).unwrap();
        first.join().unwrap();
        second.join().unwrap();
        assert_eq!(
            *submissions.lock().unwrap(),
            ["older"],
            "queued notice ignored the disabled preference"
        );
    }

    #[test]
    fn deletion_during_policy_evaluation_prevents_os_submission() {
        let mut index = DeliveredIndex::default();
        let pending = index.prepare(failure("computer:1:lifecycle", "t", "b", computer()));
        let submitted = std::cell::Cell::new(false);
        pending.deliver(
            |_| {
                // Deletion can finish while the background task queries window/settings state.
                let _ = index.withdraw("1");
                true
            },
            |_| {
                submitted.set(true);
                Ok(NotificationDelivery::Delivered)
            },
            |_| {},
        );
        assert!(
            !submitted.get(),
            "request submitted after policy evaluation observed deletion"
        );
    }

    #[test]
    fn skipped_replacement_does_not_prevent_withdrawing_an_older_notice() {
        let mut index = DeliveredIndex::default();
        let active = std::cell::Cell::new(false);
        let older = index.prepare(failure("computer:1:lifecycle", "t", "b", computer()));
        older.deliver(
            |_| true,
            |_| {
                active.set(true);
                Ok(NotificationDelivery::Delivered)
            },
            |_| active.set(false),
        );
        let withdrawal = index.withdraw("1");
        let newer = index.prepare(failure("computer:1:lifecycle", "t", "b", computer()));
        // macOS permission denial and an absent Linux service both skip submission.
        newer.deliver(
            |_| true,
            |_| Ok(NotificationDelivery::Skipped),
            |_| active.set(false),
        );
        withdrawal.clear(|_| active.set(false));
        assert!(
            !active.get(),
            "an unsent replacement must not preserve the old notification"
        );
    }

    #[test]
    fn delayed_withdrawal_preserves_a_newer_submission_for_the_same_key() {
        let mut index = DeliveredIndex::default();
        let active = std::cell::Cell::new(false);
        let older = index.prepare(failure("computer:1:lifecycle", "t", "b", computer()));
        older.deliver(
            |_| true,
            |_| {
                active.set(true);
                Ok(NotificationDelivery::Delivered)
            },
            |_| active.set(false),
        );
        let withdrawal = index.withdraw("1");
        let newer = index.prepare(failure("computer:1:lifecycle", "t", "b", computer()));
        newer.deliver(
            |_| true,
            |_| {
                active.set(true);
                Ok(NotificationDelivery::Delivered)
            },
            |_| active.set(false),
        );
        withdrawal.clear(|_| active.set(false));
        assert!(
            active.get(),
            "older withdrawal removed a newer notification"
        );
        assert_eq!(index.take("1"), ["computer:1:lifecycle"]);
    }

    #[test]
    fn delivered_index_tracks_and_clears_per_computer() {
        let mut index = DeliveredIndex::default();
        let notice = |id: &str, key: &str| Notice {
            computer: Some(NoticeComputer {
                id: id.into(),
                name: "n".into(),
            }),
            ..failure(key, "t", "b", None)
        };
        index.record(&notice("1", "computer:1:lifecycle"));
        index.record(&notice("1", "computer:1:lifecycle"));
        index.record(&notice("1", "computer:1:transfer"));
        index.record(&notice("2", "computer:2:lifecycle"));
        index.record(&failure("startup", "t", "b", None));
        assert_eq!(
            index.take("1"),
            vec!["computer:1:lifecycle", "computer:1:transfer"]
        );
        assert!(index.take("1").is_empty());
        assert_eq!(index.take("2"), vec!["computer:2:lifecycle"]);
    }
}
