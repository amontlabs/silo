//! Acknowledged push jobs belong to the host, not the SSH connection observing them.
use crate::runtime::RuntimePaths;
use crate::{
    host_push::{self, PushTarget},
    runtime,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs,
    io::{Read, Write},
    path::Path,
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

static LOCK: Mutex<()> = Mutex::new(());
static SESSION: OnceLock<String> = OnceLock::new();
static COMPLETED: OnceLock<Mutex<HashMap<String, (Value, u64)>>> = OnceLock::new();
fn session() -> &'static str {
    SESSION.get_or_init(|| uuid::Uuid::new_v4().to_string())
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}
#[derive(Clone, Serialize, Deserialize)]
struct Job {
    session: String,
    updated: u64,
    dismissed: bool,
    operation: Value,
}
type Journal = HashMap<String, Job>;
const MAX_JOBS: usize = 10_000;
const MAX_JOURNAL_BYTES: u64 = 16 * 1024 * 1024;
fn read(path: &Path) -> Result<Journal, String> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(_) => return Err("Cannot read saved push operations.".into()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_JOURNAL_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read saved push operations.")?;
    if bytes.len() as u64 > MAX_JOURNAL_BYTES {
        return Err(
            "Saved push history exceeds its 16 MiB safety limit. No new push was started.".into(),
        );
    }
    let mut jobs: Journal =
        serde_json::from_slice(&bytes).map_err(|_| "Cannot read saved push operations.")?;
    // If recording completion failed (for example a full disk), the live host
    // still reports the real result. A restart retains the honest unknown state.
    if let Some(completed) = COMPLETED.get() {
        if let Ok(completed) = completed.lock() {
            for (id, (value, updated)) in completed.iter() {
                if let Some(job) = jobs.get_mut(id) {
                    job.operation = value.clone();
                    job.updated = *updated;
                }
            }
        }
    }
    Ok(jobs)
}
fn write(path: &Path, jobs: &Journal) -> Result<(), String> {
    let bytes = serde_json::to_vec(jobs).map_err(|_| "Cannot encode push operations.")?;
    if bytes.len() as u64 > MAX_JOURNAL_BYTES {
        return Err(
            "Saved push history reached its 16 MiB safety limit. No new push was started.".into(),
        );
    }
    let parent = path.parent().ok_or("Cannot locate push operations.")?;
    fs::create_dir_all(parent).map_err(|_| "Cannot save push operations.")?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|_| "Cannot save push operations.")?;
    temporary
        .write_all(&bytes)
        .map_err(|_| "Cannot save push operations.")?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|_| "Cannot save push operations.")?;
    temporary
        .persist(path)
        .map_err(|_| "Cannot save push operations.")?;
    fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| "Cannot save push operations.".to_string())
}
fn operation(job: &Job) -> Value {
    let mut value = job.operation.clone();
    if value["status"] == "pushing" && job.session != session() {
        value["status"] = json!("unknown");
        value["message"] = json!("Silo restarted before recording the result. Check this branch on GitHub before retrying.");
    }
    value
}
/// Finished history is bounded: keep active work, the latest job for each
/// repository, and recent undismissed results; drop the rest.
fn prune(jobs: &mut Journal, now: u64) {
    const RETAIN: u64 = 7 * 24 * 60 * 60 * 1_000_000_000;
    let mut latest = HashMap::<String, (u64, String)>::new();
    for (id, job) in jobs.iter() {
        let key = format!(
            "{}\0{}",
            job.operation["computer"], job.operation["repositoryPath"]
        );
        if latest
            .get(&key)
            .is_none_or(|(updated, _)| *updated <= job.updated)
        {
            latest.insert(key, (job.updated, id.clone()));
        }
    }
    let keep: std::collections::HashSet<String> = latest.into_values().map(|(_, id)| id).collect();
    jobs.retain(|id, job| {
        keep.contains(id)
            || job.operation["status"] == "pushing"
            || (!job.dismissed && now.saturating_sub(job.updated) < RETAIN)
    });
}
fn claim(
    jobs: &mut Journal,
    id: &str,
    computer: &str,
    path: &str,
    target: &PushTarget,
) -> Result<(Value, bool), String> {
    uuid::Uuid::parse_str(id).map_err(|_| "Invalid push operation identifier.")?;
    if let Some(job) = jobs.get(id) {
        if job.operation["computer"] != computer || job.operation["repositoryPath"] != path {
            return Err("Push operation identifier belongs to another repository.".into());
        }
        return Ok((operation(job), false));
    }
    if let Some(job) = jobs.values().find(|job| {
        !job.dismissed
            && matches!(
                job.operation["status"].as_str(),
                Some("pushing" | "unknown")
            )
            && job.operation["computer"] == computer
            && job.operation["repositoryPath"] == path
    }) {
        return Ok((operation(job), false));
    }
    prune(jobs, now());
    if jobs.len() >= MAX_JOBS {
        return Err("Saved push history reached its 10,000-operation safety limit. No new push was started. Contact Silo support to archive the history without replaying previous requests.".into());
    }
    let value = json!({"operationId":id,"computer":computer,"repositoryPath":path,"status":"pushing","commitCount":0,"target":target});
    jobs.insert(
        id.into(),
        Job {
            session: session().into(),
            updated: now(),
            dismissed: false,
            operation: value.clone(),
        },
    );
    Ok((value, true))
}
fn persist_claim(
    journal: &Path,
    jobs: &Journal,
    id: &str,
    save: impl Fn(&Path, &Journal) -> Result<(), String>,
) -> Result<(), String> {
    if let Err(message) = save(journal, jobs) {
        if let Some(job) = jobs.get(id) {
            let mut value = job.operation.clone();
            value["status"] = json!("failed");
            value["commitCount"] = json!(0);
            value["message"] = json!(message);
            // A rename can succeed before directory sync fails. No worker was
            // dispatched, so resolve any saved claim even if recovery cannot save.
            record_completion_locked(journal, id, value, save);
        }
        return Err(message);
    }
    Ok(())
}
pub(crate) fn start(
    paths: &RuntimePaths,
    computer: String,
    path: String,
    id: String,
    target: PushTarget,
) -> Result<Value, String> {
    target.validate()?;
    let journal = paths.home.join("repository-push-operations.json");
    let _guard = LOCK.lock().map_err(|_| "Push state unavailable.")?;
    let mut jobs = read(&journal)?;
    let (mut value, created) = claim(&mut jobs, &id, &computer, &path, &target)?;
    if !created {
        return Ok(value);
    }
    let planned = host_push::planned_count(paths, &computer, &path);
    value["commitCount"] = json!(planned);
    if let Some(job) = jobs.get_mut(&id) {
        job.operation["commitCount"] = json!(planned);
    }
    // Persist before acknowledging or starting: retrying a lost reply never starts a second job.
    persist_claim(&journal, &jobs, &id, write)?;
    let paths = paths.clone();
    let failure_journal = journal.clone();
    let failure_id = id.clone();
    let failure_computer = computer.clone();
    let failure_path = path.clone();
    let failure_target = target.clone();
    std::thread::spawn(move || {
        let task = async move {
            let result =
                host_push::push_repository(paths, computer.clone(), path.clone(), target.clone())
                    .await;
            let mut value = result.unwrap_or_else(|message| json!({"computer":computer,"repositoryPath":path,"status":"failed","commitCount":0,"message":message,"target":target}));
            value["operationId"] = json!(id);
            // The std lock and fsync'd journal write block; keep them off the async workers.
            let _ = runtime::operation_gate::spawn_blocking(move || {
                record_completion(&journal, &id, value)
            })
            .await;
            publish_application_state_changed();
        };
        match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime.block_on(task),
            Err(_) => {
                let mut value = json!({
                    "computer": failure_computer,
                    "repositoryPath": failure_path,
                    "status": "failed",
                    "commitCount": 0,
                    "message": "Could not start the repository push.",
                    "target": failure_target
                });
                value["operationId"] = json!(failure_id);
                record_completion(&failure_journal, &failure_id, value);
                publish_application_state_changed();
            }
        }
    });
    Ok(value)
}
fn publish_application_state_changed() {
    if let Some(context) = silo_engine::context::current() {
        context.publish(
            silo_engine::context::Event::new("silo://application-state-changed", json!(null)),
            silo_engine::context::Audience::All,
        );
    }
}
fn record_completion(journal: &Path, id: &str, value: Value) {
    let Ok(_guard) = LOCK.lock() else {
        return;
    };
    record_completion_locked(journal, id, value, write);
}
fn record_completion_locked(
    journal: &Path,
    id: &str,
    value: Value,
    save: impl Fn(&Path, &Journal) -> Result<(), String>,
) {
    if let Ok(mut completed) = COMPLETED.get_or_init(|| Mutex::new(HashMap::new())).lock() {
        completed.insert(id.to_owned(), (value.clone(), now()));
    }
    if let Ok(mut jobs) = read(journal) {
        if let Some(job) = jobs.get_mut(id) {
            job.operation = value;
            job.updated = now();
        }
        // A failed save leaves the previous durable record unresolved, never a fabricated success.
        // Once saved, the durable record replaces the in-memory copy.
        if save(journal, &jobs).is_ok() {
            if let Ok(mut completed) = COMPLETED.get_or_init(|| Mutex::new(HashMap::new())).lock() {
                completed.remove(id);
            }
        }
    }
}
pub fn status(paths: &RuntimePaths, computer: &str, path: &str, id: &str) -> Result<Value, String> {
    let _guard = LOCK.lock().map_err(|_| "Push state unavailable.")?;
    let jobs = read(&paths.home.join("repository-push-operations.json"))?;
    let Some(job) = jobs.get(id) else {
        return Ok(Value::Null);
    };
    if job.operation["computer"] != computer || job.operation["repositoryPath"] != path {
        return Err("Push operation identifier belongs to another repository.".into());
    }
    Ok(operation(job))
}
pub fn merge(paths: &RuntimePaths, legacy: Vec<Value>) -> Result<Vec<Value>, String> {
    let journal = paths.home.join("repository-push-operations.json");
    Ok(merge_journal(&journal, legacy))
}
/// A damaged push journal must not hide the rest of the application state:
/// fall back to the in-memory pushes and report the journal problem.
fn merge_journal(journal: &Path, legacy: Vec<Value>) -> Vec<Value> {
    let Ok(_guard) = LOCK.lock() else {
        eprintln!("Push history unavailable; showing only current pushes.");
        return legacy;
    };
    let jobs = match read(journal) {
        Ok(jobs) => jobs,
        Err(error) => {
            eprintln!("Push history unavailable; showing only current pushes: {error}");
            return legacy;
        }
    };
    let mut latest = HashMap::<String, &Job>::new();
    for job in jobs.values() {
        let key = format!(
            "{}\0{}",
            job.operation["computer"], job.operation["repositoryPath"]
        );
        if latest
            .get(&key)
            .is_none_or(|old| old.updated <= job.updated)
        {
            latest.insert(key, job);
        }
    }
    // Older clients still use the synchronous endpoint. Its active work must
    // remain visible even when this repository has an older durable result.
    for value in &legacy {
        let key = format!("{}\0{}", value["computer"], value["repositoryPath"]);
        if value["status"] == "pushing"
            && latest
                .get(&key)
                .is_some_and(|job| operation(job)["status"] != "pushing")
        {
            latest.remove(&key);
        }
    }
    let mut values: Vec<_> = legacy
        .into_iter()
        .filter(|value| {
            !latest.contains_key(&format!(
                "{}\0{}",
                value["computer"], value["repositoryPath"]
            ))
        })
        .collect();
    values.extend(
        latest
            .values()
            .filter(|job| {
                !job.dismissed
                    && (job.operation["status"] != "succeeded"
                        || now().saturating_sub(job.updated) < 4_000_000_000)
            })
            .map(|job| operation(job)),
    );
    values
}
pub(crate) fn dismiss(paths: &RuntimePaths, computer: &str, path: &str) -> Result<(), String> {
    let _guard = LOCK.lock().map_err(|_| "Push state unavailable.")?;
    let journal = paths.home.join("repository-push-operations.json");
    let mut jobs = read(&journal)?;
    for job in jobs.values_mut().filter(|job| {
        job.operation["computer"] == computer && job.operation["repositoryPath"] == path
    }) {
        if operation(job)["status"] != "pushing" {
            job.dismissed = true;
        }
    }
    write(&journal, &jobs)
}
// Preparation failures are authoritative: no detached job was dispatched.
// Return them as results so a caller can distinguish them from a lost SSH reply.
pub fn start_result(
    paths: &RuntimePaths,
    computer: String,
    path: String,
    id: String,
    target: PushTarget,
) -> Value {
    start(paths, computer.clone(), path.clone(), id.clone(), target).unwrap_or_else(|message| json!({"operationId":id,"computer":computer,"repositoryPath":path,"status":"failed","commitCount":0,"message":message}))
}
const UPDATE_CONTROLLER: &str = "Update Silo on the device you are pushing from, then push again.";
/// A controller that sends no confirmed target predates bound pushes; this
/// device never pushes on its behalf.
fn remote_request(params: &Value) -> Result<(String, String, Option<PushTarget>), String> {
    let text = |key: &str| {
        params
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| format!("Missing {key}."))
    };
    let target = match params.get("target") {
        None | Some(Value::Null) => None,
        Some(target) => Some(
            serde_json::from_value(target.clone())
                .map_err(|_| "Invalid push target.".to_string())?,
        ),
    };
    Ok((text("path")?, text("operationId")?, target))
}
pub(crate) fn start_remote(
    paths: &RuntimePaths,
    computer: String,
    params: &Value,
) -> Result<Value, String> {
    let (path, id, target) = remote_request(params)?;
    Ok(match target {
        Some(target) => start_result(paths, computer, path, id, target),
        None => {
            json!({"operationId":id,"computer":computer,"repositoryPath":path,"status":"failed","commitCount":0,"message":UPDATE_CONTROLLER})
        }
    })
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrated_push_operations_load() {
        let migrated = crate::runtime_migration::vocabulary_tests::migrated_installation();
        let jobs = read(
            &migrated
                .storage
                .join("microsandbox/repository-push-operations.json"),
        )
        .unwrap();
        assert_eq!(jobs["op1"].operation["computer"], "dev");
        assert!(jobs["op1"].operation.get("workspace").is_none());
    }
    fn target() -> PushTarget {
        PushTarget {
            repository: "owner/repo".into(),
            branch: "main".into(),
            commit: "a".repeat(40),
        }
    }
    #[test]
    fn remote_requests_without_a_confirmed_target_never_push() {
        let params = json!({"computerId":"computer","path":"/workspace/repo","operationId":"id"});
        let (path, id, requested) = remote_request(&params).unwrap();
        assert_eq!((path.as_str(), id.as_str()), ("/workspace/repo", "id"));
        assert!(requested.is_none());
        let mut bound = params.clone();
        bound["target"] = serde_json::to_value(target()).unwrap();
        assert_eq!(remote_request(&bound).unwrap().2, Some(target()));
        bound["target"] = json!({"repository":"owner/repo"});
        assert!(remote_request(&bound).is_err());
    }
    #[test]
    fn journaled_pushes_record_their_confirmed_target() {
        let mut jobs = Journal::new();
        let (value, _) = claim(
            &mut jobs,
            &uuid::Uuid::new_v4().to_string(),
            "dev",
            "/workspace/repo",
            &target(),
        )
        .unwrap();
        assert_eq!(value["target"]["commit"], "a".repeat(40));
    }
    #[test]
    fn a_failed_claim_save_never_leaves_an_unstarted_push_active() {
        let _state = crate::test_support::global_state();
        let _guard = LOCK.lock().unwrap();
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("jobs.json");
        let id = uuid::Uuid::new_v4().to_string();
        let mut jobs = Journal::new();
        claim(&mut jobs, &id, "dev", "/workspace/repo", &target()).unwrap();
        // The rename succeeds, but the subsequent durability check fails.
        // Recovery writes fail too, so the live fallback must resolve the job.
        let failed_save = |path: &Path, jobs: &Journal| {
            if !path.exists() {
                write(path, jobs)?;
            }
            Err("Cannot save push operations.".to_string())
        };
        assert!(persist_claim(&path, &jobs, &id, failed_save).is_err());
        let mut recovered = read(&path).unwrap();
        let (result, replayed) =
            claim(&mut recovered, &id, "dev", "/workspace/repo", &target()).unwrap();
        assert!(!replayed);
        assert_eq!(result["status"], "failed");
        let durable: Journal = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(durable[&id].operation["status"], "pushing");
        assert_eq!(result["target"], serde_json::to_value(target()).unwrap());
        assert!(
            claim(
                &mut recovered,
                &uuid::Uuid::new_v4().to_string(),
                "dev",
                "/workspace/repo",
                &target(),
            )
            .unwrap()
            .1
        );
        if let Some(completed) = COMPLETED.get() {
            completed.lock().unwrap().remove(&id);
        }
    }
    #[test]
    fn claim_prunes_dismissed_and_old_finished_history() {
        let mut jobs = Journal::new();
        let job = |status: &str, path: &str, updated: u64, dismissed: bool| Job {
            session: "old".into(),
            updated,
            dismissed,
            operation: json!({"computer":"dev","repositoryPath":path,"status":status}),
        };
        jobs.insert(
            "old-dismissed".into(),
            job("failed", "/workspace/a", 1, true),
        );
        jobs.insert("latest-a".into(), job("failed", "/workspace/a", 2, true));
        jobs.insert(
            "old-finished".into(),
            job("succeeded", "/workspace/b", 1, false),
        );
        jobs.insert(
            "latest-b".into(),
            job("succeeded", "/workspace/b", 2, false),
        );
        jobs.insert("active".into(), job("pushing", "/workspace/c", 1, false));
        prune(&mut jobs, 30 * 24 * 60 * 60 * 1_000_000_000);
        let mut kept: Vec<_> = jobs.keys().cloned().collect();
        kept.sort();
        assert_eq!(kept, ["active", "latest-a", "latest-b"]);
    }
    #[test]
    fn corrupt_journal_degrades_to_current_pushes() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("jobs.json");
        fs::write(&path, b"{not json").unwrap();
        let legacy = vec![
            serde_json::json!({"computer":"dev","repositoryPath":"/workspace/repo","status":"pushing"}),
        ];
        assert_eq!(merge_journal(&path, legacy.clone()), legacy);
    }
    #[test]
    fn lost_acknowledgement_and_concurrent_clicks_share_one_job() {
        let mut jobs = Journal::new();
        let id = uuid::Uuid::new_v4().to_string();
        assert!(
            claim(&mut jobs, &id, "dev", "/workspace/repo", &target())
                .unwrap()
                .1
        );
        assert!(
            !claim(&mut jobs, &id, "dev", "/workspace/repo", &target())
                .unwrap()
                .1
        );
        let (other, created) = claim(
            &mut jobs,
            &uuid::Uuid::new_v4().to_string(),
            "dev",
            "/workspace/repo",
            &target(),
        )
        .unwrap();
        assert!(!created);
        assert_eq!(other["operationId"], id);
        assert_eq!(jobs.len(), 1);
        assert!(claim(&mut jobs, &id, "another", "/workspace/repo", &target()).is_err());
    }
    #[test]
    fn unknown_publication_blocks_new_requests_until_acknowledged() {
        let mut jobs = Journal::new();
        let previous = uuid::Uuid::new_v4().to_string();
        claim(&mut jobs, &previous, "dev", "/workspace/repo", &target()).unwrap();
        jobs.get_mut(&previous).unwrap().operation["status"] = json!("unknown");
        let next = uuid::Uuid::new_v4().to_string();
        let (result, created) =
            claim(&mut jobs, &next, "dev", "/workspace/repo", &target()).unwrap();
        assert!(
            !created,
            "an unacknowledged unknown push must block another publication"
        );
        assert_eq!(result["operationId"], previous);
        assert_eq!(result["status"], "unknown");
        assert!(!jobs.contains_key(&next));
        jobs.get_mut(&previous).unwrap().dismissed = true;
        assert!(
            claim(&mut jobs, &next, "dev", "/workspace/repo", &target())
                .unwrap()
                .1
        );
    }
    #[test]
    fn journal_preserves_results_and_never_replays_after_restart() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("jobs.json");
        let id = uuid::Uuid::new_v4().to_string();
        let mut jobs = Journal::new();
        claim(&mut jobs, &id, "dev", "/workspace/repo", &target()).unwrap();
        jobs.get_mut(&id).unwrap().session = "previous-process".into();
        write(&path, &jobs).unwrap();
        let mut recovered = read(&path).unwrap();
        let (result, created) =
            claim(&mut recovered, &id, "dev", "/workspace/repo", &target()).unwrap();
        assert!(!created);
        let (blocked, launched) = claim(
            &mut recovered,
            &uuid::Uuid::new_v4().to_string(),
            "dev",
            "/workspace/repo",
            &target(),
        )
        .unwrap();
        assert!(!launched);
        assert_eq!(blocked["status"], "unknown");
        assert_eq!(result["status"], "unknown");
        assert!(result["message"]
            .as_str()
            .unwrap()
            .contains("Check this branch on GitHub"));
        recovered.get_mut(&id).unwrap().dismissed = true; // Explicit “I’ve checked GitHub”.
        assert!(
            claim(
                &mut recovered,
                &uuid::Uuid::new_v4().to_string(),
                "dev",
                "/workspace/repo",
                &target()
            )
            .unwrap()
            .1
        );
        recovered.get_mut(&id).unwrap().operation["status"] = json!("succeeded");
        write(&path, &recovered).unwrap();
        assert_eq!(operation(&read(&path).unwrap()[&id])["status"], "succeeded");
    }
    #[test]
    fn history_limits_reject_new_work_but_keep_existing_identifiers() {
        let id = uuid::Uuid::new_v4().to_string();
        let mut jobs = Journal::new();
        claim(&mut jobs, &id, "dev", "/workspace/repo", &target()).unwrap();
        let mut finished = jobs[&id].clone();
        finished.operation["status"] = json!("succeeded");
        for index in 1..MAX_JOBS {
            jobs.insert(index.to_string(), finished.clone());
        }
        assert!(
            !claim(&mut jobs, &id, "dev", "/workspace/repo", &target())
                .unwrap()
                .1
        );
        assert!(claim(
            &mut jobs,
            &uuid::Uuid::new_v4().to_string(),
            "other",
            "/workspace/repo",
            &target()
        )
        .unwrap_err()
        .contains("10,000"));
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("oversized.json");
        fs::File::create(&path)
            .unwrap()
            .set_len(MAX_JOURNAL_BYTES + 1)
            .unwrap();
        assert!(read(&path).err().unwrap().contains("16 MiB"));
    }
}
