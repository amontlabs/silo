use std::{
    os::unix::fs::PermissionsExt,
    sync::{Arc, Mutex},
};

use super::*;
use crate::context::{
    EngineIdentity, EnginePaths, EngineRuntimeState, MainThread, PackageKind, ProcessControl,
};

fn asked(computer: &str, port: u64) -> NewApproval {
    let params = json!({"computerId": computer, "port": port});
    NewApproval {
        kind: ApprovalKind::Port,
        computer_id: computer.to_owned(),
        computer_name: format!("{computer}-name"),
        method: "network.publish".into(),
        params: params.clone(),
        asked: params,
        summary: format!("Publish port {port} of {computer} on this device."),
        creates: None,
        review_request_id: None,
        review_digest: None,
    }
}

fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path());
    (dir, store)
}

#[test]
fn the_same_call_from_the_same_computer_finds_the_same_approval() {
    let (_dir, store) = store();
    let (first, made) = store.ask(asked("c1", 80)).unwrap();
    assert!(made);
    assert_eq!(first.state(), ApprovalState::Pending);
    assert!(valid_id(first.id()), "{}", first.id());
    let (again, made) = store.ask(asked("c1", 80)).unwrap();
    assert!(!made);
    assert_eq!(again.id(), first.id());
    // Another call, or the same call from another computer, is a new approval.
    assert_ne!(store.ask(asked("c1", 81)).unwrap().0.id(), first.id());
    let other = NewApproval {
        computer_id: "c2".into(),
        ..asked("c1", 80)
    };
    assert_ne!(store.ask(other).unwrap().0.id(), first.id());
    // Once answered, asking again makes a new one.
    store.deny(first.id(), "app", None).unwrap();
    assert_ne!(store.ask(asked("c1", 80)).unwrap().0.id(), first.id());
}

#[test]
fn the_request_key_ignores_key_order_and_reads_null_as_empty() {
    let one: Value = serde_json::from_str(r#"{"a":1,"b":2}"#).unwrap();
    let two: Value = serde_json::from_str(r#"{"b":2,"a":1}"#).unwrap();
    assert_eq!(request_key("m", &one), request_key("m", &two));
    assert_eq!(request_key("m", &Value::Null), request_key("m", &json!({})));
    assert_ne!(request_key("m", &one), request_key("n", &one));
}

#[test]
fn a_computer_holds_at_most_twenty_pending_approvals() {
    let (_dir, store) = store();
    for port in 0..PENDING_LIMIT as u64 {
        store.ask(asked("c1", port)).unwrap();
    }
    assert_eq!(store.ask(asked("c1", 999)), Err(AskError::Limit));
    // The same call as a pending one still finds it, and other computers are not limited.
    assert!(store.ask(asked("c1", 3)).is_ok());
    assert!(store.ask(asked("c2", 999)).is_ok());
    // An answer makes room.
    let first = store.ask(asked("c1", 0)).unwrap().0;
    store.cancel(first.id(), Some("c1"), "no").unwrap();
    assert!(store.ask(asked("c1", 999)).is_ok());
}

#[test]
fn pending_approvals_expire_after_seven_days_and_ended_ones_go_after_thirty() {
    let (_dir, store) = store();
    let start = 1_000_000_000_000;
    let (old, _) = store.ask_at(asked("c1", 1), start).unwrap();
    let (denied, _) = store.ask_at(asked("c1", 2), start).unwrap();
    store.deny(denied.id(), "app", None).unwrap();
    assert!(store.sweep(start + PENDING_FOR_MS - 1).is_empty());
    let expired = store.sweep(start + PENDING_FOR_MS);
    assert_eq!(expired.len(), 1);
    assert_eq!(expired[0].id(), old.id());
    assert_eq!(expired[0].state(), ApprovalState::Expired);
    assert!(expired[0]
        .approval
        .reason
        .as_deref()
        .unwrap()
        .contains("seven days"));
    // Expired, it can no longer be answered.
    assert!(store
        .allow(old.id(), "app", None)
        .unwrap_err()
        .contains("expired"));
    let ended = store.get(old.id()).unwrap().unwrap().ended_ms.unwrap();
    store.sweep(ended + KEPT_FOR_MS - 1);
    assert!(store.get(old.id()).unwrap().is_some());
    store.sweep(ended + KEPT_FOR_MS);
    assert!(store.get(old.id()).unwrap().is_none());
    // The denied one ended at the real time, after these sweeps, so it stays.
    assert!(store.get(denied.id()).unwrap().is_some());
}

#[test]
fn approvals_are_owner_only_files_written_whole() {
    let (dir, store) = store();
    let (record, _) = store.ask(asked("c1", 1)).unwrap();
    let root = dir.path().join("approvals");
    let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&root), 0o700);
    assert_eq!(mode(&root.join(format!("{}.json", record.id()))), 0o600);
    store.note(record.id(), "c1", "why").unwrap();
    let names: Vec<String> = fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        [format!("{}.json", record.id())],
        "no temporary file is left"
    );
    // A damaged file is skipped and an id that is not one is refused.
    fs::write(root.join("ap-ffffffffffffffff.json"), "{").unwrap();
    assert_eq!(store.list().0.len(), 1);
    assert!(store.get("../x").is_err());
}

#[test]
fn an_approval_is_allowed_once_and_its_outcome_stored_once() {
    let (_dir, store) = store();
    let (record, _) = store.ask(asked("c1", 1)).unwrap();
    let allowed = store.allow(record.id(), "app", Some("ok")).unwrap();
    assert_eq!(allowed.state(), ApprovalState::Allowed);
    assert_eq!(allowed.approval.decision_note.as_deref(), Some("ok"));
    assert!(store
        .allow(record.id(), "app", None)
        .unwrap_err()
        .contains("already allowed"));
    assert!(store.deny(record.id(), "app", None).is_err());
    assert!(store.cancel(record.id(), None, "x").is_err());
    let outcome = ApprovalOutcome {
        result: Some(json!({"ok": true})),
        job_id: None,
        error: None,
    };
    store.finish(record.id(), outcome.clone()).unwrap();
    assert!(store.finish(record.id(), outcome).is_err());
}

#[test]
fn two_allows_at_once_hand_the_call_over_once() {
    let (_dir, store) = store();
    let (record, _) = store.ask(asked("c1", 1)).unwrap();
    let store = Arc::new(store);
    let id = record.id().to_owned();
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let (store, id) = (store.clone(), id.clone());
            std::thread::spawn(move || store.allow(&id, "app", None).is_ok())
        })
        .collect();
    let won = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .filter(|won| *won)
        .count();
    assert_eq!(won, 1);
}

#[test]
fn an_allowed_call_without_an_outcome_ends_as_interrupted_at_start() {
    let (_dir, store) = store();
    let (record, _) = store.ask(asked("c1", 1)).unwrap();
    store.allow(record.id(), "app", None).unwrap();
    let (done, _) = store.ask(asked("c1", 2)).unwrap();
    store.allow(done.id(), "app", None).unwrap();
    store
        .finish(
            done.id(),
            ApprovalOutcome {
                result: None,
                job_id: Some("job:1".into()),
                error: None,
            },
        )
        .unwrap();
    let recovered = store.recover();
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].id(), record.id());
    let error = recovered[0]
        .approval
        .outcome
        .as_ref()
        .and_then(|outcome| outcome.error.clone())
        .unwrap();
    assert!(error.contains("not run again"), "{error}");
    // Its outcome is now stored, so nothing can hand the call over again.
    assert!(store
        .finish(record.id(), recovered[0].approval.outcome.clone().unwrap())
        .is_err());
    assert!(store.recover().is_empty());
}

#[test]
fn only_the_asking_computer_writes_its_note_and_never_into_the_summary() {
    let (_dir, store) = store();
    let (record, _) = store.ask(asked("c1", 1)).unwrap();
    assert!(store.note(record.id(), "c2", "mine").is_err());
    assert!(store
        .note(record.id(), "c1", &"x".repeat(APPROVAL_NOTE_MAX_CHARS + 1))
        .is_err());
    let noted = store
        .note(record.id(), "c1", "  to try the site  ")
        .unwrap();
    assert_eq!(noted.approval.note.as_deref(), Some("to try the site"));
    assert_eq!(noted.approval.summary, record.approval.summary);
    assert_eq!(noted.approval.params, record.approval.params);
    assert_eq!(noted.approval.method, record.approval.method);
    assert!(!notice(&noted).title.contains("to try"));
    assert!(!notice(&noted).body.contains("to try"));
    assert!(store
        .note(record.id(), "c1", "")
        .unwrap()
        .approval
        .note
        .is_none());
    // A computer cannot withdraw another's approval.
    assert!(store.cancel(record.id(), Some("c2"), "x").is_err());
    assert_eq!(
        store.cancel(record.id(), Some("c1"), "x").unwrap().state(),
        ApprovalState::Cancelled
    );
    assert!(store.note(record.id(), "c1", "late").is_err());
}

#[test]
fn deleting_a_computer_cancels_its_pending_approvals_only() {
    let (_dir, store) = store();
    let (pending, _) = store.ask(asked("c1", 1)).unwrap();
    let (answered, _) = store.ask(asked("c1", 2)).unwrap();
    store.deny(answered.id(), "app", None).unwrap();
    let (other, _) = store.ask(asked("c2", 1)).unwrap();
    let cancelled = store.cancel_for_computer("c1");
    assert_eq!(cancelled.len(), 1);
    assert_eq!(cancelled[0].id(), pending.id());
    assert_eq!(cancelled[0].state(), ApprovalState::Cancelled);
    assert!(cancelled[0]
        .approval
        .reason
        .as_deref()
        .unwrap()
        .contains("deleted"));
    assert_eq!(
        store.get(answered.id()).unwrap().unwrap().state(),
        ApprovalState::Denied
    );
    assert_eq!(
        store.get(other.id()).unwrap().unwrap().state(),
        ApprovalState::Pending
    );
}

#[test]
fn a_secrets_value_is_kept_for_the_call_and_never_shown() {
    let (_dir, store) = store();
    let params = json!({"request": {"operation": "add", "name": "deploy", "value": "s3cr3t",
        "computerIds": ["c1"], "allowedDomains": ["deploy.example.com"]}});
    let (record, _) = store
        .ask(NewApproval {
            kind: ApprovalKind::Secret,
            method: "secrets.save".into(),
            params: params.clone(),
            asked: params,
            ..asked("c1", 1)
        })
        .unwrap();
    let stored = store.get(record.id()).unwrap().unwrap();
    assert_eq!(stored.approval.params["request"]["value"], "s3cr3t");
    let view = serde_json::to_string(&stored.view()).unwrap();
    assert!(!view.contains("s3cr3t"), "{view}");
}

#[test]
fn review_request_links_are_found_and_answered_outside() {
    let (_dir, store) = store();
    let (record, _) = store
        .ask(NewApproval {
            kind: ApprovalKind::Merge,
            review_request_id: Some("fr-1".into()),
            review_digest: Some("d1".into()),
            ..asked("c1", 1)
        })
        .unwrap();
    assert_eq!(store.pending_for_request("fr-1").len(), 1);
    let kept = store
        .answered_outside(
            record.id(),
            true,
            "app",
            Some(ApprovalOutcome {
                result: None,
                job_id: Some("job:9".into()),
                error: None,
            }),
        )
        .unwrap();
    assert_eq!(kept.state(), ApprovalState::Allowed);
    assert_eq!(
        kept.approval.outcome.unwrap().job_id.as_deref(),
        Some("job:9")
    );
    assert!(store.pending_for_request("fr-1").is_empty());
}

struct Recording {
    paths: EnginePaths,
    identity: EngineIdentity,
    runtime_state: Arc<EngineRuntimeState>,
    events: Mutex<Vec<Event>>,
    retained: Mutex<Vec<Notice>>,
    cleared: Mutex<Vec<(String, String)>>,
}

impl EngineContext for Recording {
    fn paths(&self) -> &EnginePaths {
        &self.paths
    }
    fn identity(&self) -> &EngineIdentity {
        &self.identity
    }
    fn runtime_state(&self) -> Arc<EngineRuntimeState> {
        self.runtime_state.clone()
    }
    fn publish(&self, event: Event, _audience: Audience) {
        self.events.lock().unwrap().push(event);
    }
    fn main_thread(&self) -> Option<&dyn MainThread> {
        None
    }
    fn process(&self) -> &dyn ProcessControl {
        self
    }
    fn retain_notice(&self, notice: &Notice, _audience: Audience) -> Result<(), String> {
        self.retained.lock().unwrap().push(notice.clone());
        Ok(())
    }
    fn clear_notice(
        &self,
        key: &str,
        computer_id: &str,
        _audience: Audience,
    ) -> Result<(), String> {
        self.cleared
            .lock()
            .unwrap()
            .push((key.to_owned(), computer_id.to_owned()));
        Ok(())
    }
}

impl ProcessControl for Recording {
    fn exit(&self, _code: i32) {}
    fn restart(&self) -> ! {
        panic!("an approvals test must not restart the process")
    }
}

#[test]
fn a_pending_approval_holds_one_notice_and_every_change_is_announced() {
    let (dir, store) = store();
    let context = Recording {
        paths: EnginePaths {
            app_data: dir.path().to_owned(),
            ..EnginePaths::default()
        },
        identity: EngineIdentity {
            channel: crate::channel::Channel::Development,
            version: "test".into(),
            package_kind: PackageKind::Other,
        },
        runtime_state: Arc::new(EngineRuntimeState::default()),
        events: Mutex::default(),
        retained: Mutex::default(),
        cleared: Mutex::default(),
    };
    let (record, _) = store.ask(asked("c1", 8080)).unwrap();
    announce(&context, &record);
    let denied = store.deny(record.id(), "app", Some("not now")).unwrap();
    announce(&context, &denied);
    let events = context.events.lock().unwrap();
    assert_eq!(
        events
            .iter()
            .map(|event| (
                event.name.as_str(),
                event.payload["state"].as_str().unwrap()
            ))
            .collect::<Vec<_>>(),
        [
            ("approvals.changed", "pending"),
            ("approvals.changed", "denied")
        ]
    );
    assert_eq!(events[0].payload["id"], record.id());
    let retained = context.retained.lock().unwrap();
    assert_eq!(retained.len(), 1);
    let notice = &retained[0];
    assert_eq!(notice.category, NoticeCategory::Approvals);
    assert_eq!(notice.key, format!("approval:{}", record.id()));
    assert!(
        notice.title.starts_with("c1-name asks: Publish port 8080"),
        "{}",
        notice.title
    );
    assert_eq!(notice.computer.as_ref().unwrap().id, "c1");
    assert_eq!(
        *context.cleared.lock().unwrap(),
        [(format!("approval:{}", record.id()), "c1".to_owned())]
    );
    // The wire event parses as the API's own.
    let frame = json!({"event": "approvals.changed", "subscription": "s", "deviceId": "d",
        "seq": 1, "payload": events[1].payload});
    serde_json::from_value::<silo_api::events::EventEnvelope>(frame).unwrap();
}
