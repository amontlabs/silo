//! Review requests: `changes.request` records that a fork waits for a review, without keeping
//! it. One JSON file per request under `<app data>/fold-requests/`. Listing, showing, keeping
//! and throwing away a request build on this store.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};

use silo_api::methods::{
    keep::{FoldRequest, FoldRequestClosedReason, FoldRequestStatus},
    review_requests::{CredentialUsage, RequestFilter},
};

pub struct Store {
    root: PathBuf,
}

/// What a request is made from.
pub struct NewRequest {
    pub fork_id: String,
    pub target: String,
    pub review_digest: String,
    pub summary: String,
    pub counts: std::collections::BTreeMap<String, u64>,
}

/// One lock for each (fork, target), so two first requests for the same pair cannot both
/// find no open request and make two.
fn pair_lock(fork: &str, target: &str) -> Arc<Mutex<()>> {
    static LOCKS: OnceLock<Mutex<HashMap<(String, String), Arc<Mutex<()>>>>> = OnceLock::new();
    LOCKS
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .entry((fork.to_owned(), target.to_owned()))
        .or_default()
        .clone()
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 40
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

impl Store {
    pub fn new(app_data: &Path) -> Self {
        Self {
            root: app_data.join("fold-requests"),
        }
    }

    fn file(&self, id: &str) -> Result<PathBuf, String> {
        if !valid_id(id) {
            return Err(format!("`{id}` is not a request id."));
        }
        Ok(self.root.join(format!("{id}.json")))
    }

    pub fn get(&self, id: &str) -> Result<Option<FoldRequest>, String> {
        let path = self.file(id)?;
        match fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text)
                .map(Some)
                .map_err(|error| format!("The request {id} is unreadable: {error}")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(format!("Cannot read the request {id}: {error}")),
        }
    }

    /// Every readable request, oldest first. A damaged file is skipped.
    pub fn list(&self) -> Vec<FoldRequest> {
        let Ok(entries) = fs::read_dir(&self.root) else {
            return Vec::new();
        };
        let mut found: Vec<FoldRequest> = entries
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
            .filter_map(|entry| fs::read_to_string(entry.path()).ok())
            .filter_map(|text| serde_json::from_str(&text).ok())
            .collect();
        found.sort_by(|left, right| {
            (&left.created_at, &left.id).cmp(&(&right.created_at, &right.id))
        });
        found
    }

    /// Records an open request. A fork that already has an open request for the same target
    /// keeps its id: the request is updated with the newer digest and summary.
    pub fn create(&self, new: NewRequest) -> Result<FoldRequest, String> {
        let lock = pair_lock(&new.fork_id, &new.target);
        let _held = lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let existing = self.list().into_iter().find(|request| {
            request.fork_id == new.fork_id
                && request.target == new.target
                && matches!(
                    request.status,
                    FoldRequestStatus::Open | FoldRequestStatus::Keeping
                )
        });
        if let Some(under_way) = existing
            .as_ref()
            .filter(|request| request.status == FoldRequestStatus::Keeping)
        {
            return Err(format!(
                "A keep of the request {} was started and is not settled. Keep it again to settle it before asking for another review.",
                under_way.id
            ));
        }
        let approval_id = existing
            .as_ref()
            .and_then(|request| request.approval_id.clone());
        // Once an approval wraps the request, its text and counts are the engine's: a later
        // request for the same fork only moves the digest.
        let (summary, counts) = match existing
            .as_ref()
            .filter(|request| request.approval_id.is_some())
        {
            Some(linked) => (linked.summary.clone(), linked.counts.clone()),
            None => (new.summary, new.counts),
        };
        let request = FoldRequest {
            id: existing.as_ref().map_or_else(
                || format!("fr-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]),
                |request| request.id.clone(),
            ),
            fork_id: new.fork_id,
            target: new.target,
            created_at: existing.map_or_else(now, |request| request.created_at),
            review_digest: new.review_digest,
            summary,
            status: FoldRequestStatus::Open,
            counts,
            keep_id: None,
            resolved_at: None,
            keeping_since_ms: None,
            closed_reason: None,
            approval_id,
        };
        self.put(&request)?;
        Ok(request)
    }

    /// Records the approval that asks the owner to merge the request, which must be open. The
    /// request's text becomes the engine's `summary` and its counts are cleared, so nothing
    /// the computer wrote stays in what the owner reads.
    pub fn link_approval(
        &self,
        id: &str,
        approval_id: &str,
        summary: &str,
    ) -> Result<FoldRequest, String> {
        self.modify(id, false, |request| {
            request.approval_id = Some(approval_id.to_owned());
            request.summary = summary.to_owned();
            request.counts.clear();
        })
    }

    /// The requests that match `filter`, oldest first.
    pub fn list_matching(&self, filter: RequestFilter) -> Vec<FoldRequest> {
        self.list()
            .into_iter()
            .filter(|request| match filter {
                RequestFilter::All => true,
                RequestFilter::Open => matches!(
                    request.status,
                    FoldRequestStatus::Open | FoldRequestStatus::Keeping
                ),
                RequestFilter::Kept => request.status == FoldRequestStatus::Kept,
                RequestFilter::ThrownAway => request.status == FoldRequestStatus::ThrownAway,
            })
            .collect()
    }

    /// A request that is now kept; `keep_id` is the target's checkpoint from before. It was
    /// open, or its keep was under way.
    pub fn mark_kept(&self, id: &str, keep_id: Option<String>) -> Result<FoldRequest, String> {
        self.modify(id, true, |request| {
            request.status = FoldRequestStatus::Kept;
            request.keep_id = keep_id;
            request.keeping_since_ms = None;
            request.resolved_at = Some(now());
        })
    }

    /// An open request whose fork was dropped.
    pub fn mark_thrown_away(&self, id: &str) -> Result<FoldRequest, String> {
        self.modify(id, false, |request| {
            request.status = FoldRequestStatus::ThrownAway;
            request.resolved_at = Some(now());
        })
    }

    /// An open request whose fork or target computer was deleted.
    pub fn close_deleted(
        &self,
        id: &str,
        reason: FoldRequestClosedReason,
    ) -> Result<FoldRequest, String> {
        self.modify(id, false, |request| {
            request.status = FoldRequestStatus::ThrownAway;
            request.closed_reason = Some(reason);
            request.resolved_at = Some(now());
        })
    }

    /// Closes the open requests that involve a deleted computer, as fork or as target. A
    /// request being kept stays as it is: its keep settles it.
    pub fn close_for_computer(&self, computer_id: &str) -> Vec<FoldRequest> {
        self.close_where(|request| deleted_reason(request, |id| id != computer_id))
    }

    /// Closes the open requests whose fork or target computer is not among `existing`. Call
    /// it only with a complete list of computers: a computer missing from a partial list is
    /// not known to be gone.
    pub fn close_missing(&self, existing: &[String]) -> Vec<FoldRequest> {
        self.close_where(|request| deleted_reason(request, |id| existing.iter().any(|e| e == id)))
    }

    fn close_where(
        &self,
        reason: impl Fn(&FoldRequest) -> Option<FoldRequestClosedReason>,
    ) -> Vec<FoldRequest> {
        self.list()
            .into_iter()
            .filter(|request| request.status == FoldRequestStatus::Open)
            .filter_map(|request| {
                let why = reason(&request)?;
                let _claim = claim(&request.id).ok()?;
                self.close_deleted(&request.id, why).ok()
            })
            .collect()
    }

    /// Records, before a keep starts, that one is under way since `since_ms`.
    pub fn mark_keeping(&self, id: &str, since_ms: u64) -> Result<FoldRequest, String> {
        self.modify(id, true, |request| {
            request.status = FoldRequestStatus::Keeping;
            request.keeping_since_ms = Some(since_ms);
        })
    }

    /// A keep that changed nothing: the request is open again.
    pub fn reopen(&self, id: &str) -> Result<FoldRequest, String> {
        self.modify(id, true, |request| {
            request.status = FoldRequestStatus::Open;
            request.keeping_since_ms = None;
        })
    }

    /// Reads, changes and writes one request under the lock `create` takes for its fork and
    /// target, so neither overwrites the other.
    fn modify(
        &self,
        id: &str,
        allow_keeping: bool,
        change: impl FnOnce(&mut FoldRequest),
    ) -> Result<FoldRequest, String> {
        let first = self.existing(id)?;
        let lock = pair_lock(&first.fork_id, &first.target);
        let _held = lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut request = if allow_keeping {
            self.keepable_request(id)?
        } else {
            self.open_request(id)?
        };
        change(&mut request);
        self.put(&request)?;
        Ok(request)
    }

    /// Marks every open request for the fork and target kept, for a keep made without the
    /// request. A request another job holds is left to that job.
    pub fn mark_kept_for(
        &self,
        fork: &str,
        target: &str,
        keep_id: Option<&str>,
    ) -> Vec<FoldRequest> {
        self.list()
            .into_iter()
            .filter(|request| {
                request.fork_id == fork
                    && request.target == target
                    && matches!(
                        request.status,
                        FoldRequestStatus::Open | FoldRequestStatus::Keeping
                    )
            })
            .filter_map(|request| {
                let _claim = claim(&request.id).ok()?;
                self.mark_kept(&request.id, keep_id.map(str::to_owned)).ok()
            })
            .collect()
    }

    /// The request, which must still be open.
    pub fn open_request(&self, id: &str) -> Result<FoldRequest, String> {
        let request = self.existing(id)?;
        match request.status {
            FoldRequestStatus::Open => Ok(request),
            FoldRequestStatus::Keeping => Err(format!(
                "A keep of the request {id} was started and is not settled. Keep it again to settle it."
            )),
            FoldRequestStatus::Kept => Err(format!("The request {id} was already kept.")),
            FoldRequestStatus::ThrownAway => {
                Err(format!("The request {id} was already thrown away."))
            }
        }
    }

    /// The request, open or with a keep that was started.
    pub fn keepable_request(&self, id: &str) -> Result<FoldRequest, String> {
        let request = self.existing(id)?;
        match request.status {
            FoldRequestStatus::Open | FoldRequestStatus::Keeping => Ok(request),
            FoldRequestStatus::Kept => Err(format!("The request {id} was already kept.")),
            FoldRequestStatus::ThrownAway => {
                Err(format!("The request {id} was already thrown away."))
            }
        }
    }

    fn existing(&self, id: &str) -> Result<FoldRequest, String> {
        self.get(id)?
            .ok_or_else(|| format!("There is no review request {id}."))
    }

    /// Writes the request whole: a reader sees the old file or the new one.
    pub fn put(&self, request: &FoldRequest) -> Result<(), String> {
        let path = self.file(&request.id)?;
        fs::create_dir_all(&self.root)
            .map_err(|error| format!("Cannot create the request store: {error}"))?;
        let text = serde_json::to_string_pretty(request).map_err(|error| error.to_string())?;
        let temporary = path.with_extension(format!("json.{}.tmp", uuid::Uuid::new_v4().simple()));
        fs::write(&temporary, text)
            .and_then(|()| fs::rename(&temporary, &path))
            .map_err(|error| {
                let _ = fs::remove_file(&temporary);
                format!("Cannot save the request {}: {error}", request.id)
            })
    }
}

/// Why an open request is closed: its fork, or a computer target, is not `present`. A folder
/// target is not a computer and is never judged by the list.
fn deleted_reason(
    request: &FoldRequest,
    present: impl Fn(&str) -> bool,
) -> Option<FoldRequestClosedReason> {
    if !present(&request.fork_id) {
        Some(FoldRequestClosedReason::ForkDeleted)
    } else if folder_path(&request.target).is_none() && !present(&request.target) {
        Some(FoldRequestClosedReason::TargetDeleted)
    } else {
        None
    }
}

/// The name of the long-lived computer, which is never thrown away.
pub const MAIN: &str = "main";

/// The host folder a request's target names, when it is a folder.
pub fn folder_path(target: &str) -> Option<&str> {
    target.strip_prefix("folder:")
}

/// Whether the request can be kept by the engine: it is open, its target is a computer, and
/// it carries the digest the keep is pinned to.
pub fn check_keepable(request: &FoldRequest) -> Result<(), String> {
    if !matches!(
        request.status,
        FoldRequestStatus::Open | FoldRequestStatus::Keeping
    ) {
        return Err(format!("The request {} is not open.", request.id));
    }
    if let Some(path) = folder_path(&request.target) {
        return Err(format!(
            "{} was made from the folder {path}. Merge folder forks with `silo merge`, which brings the commits back into the folder.",
            request.fork_id
        ));
    }
    if request.review_digest.is_empty() {
        return Err(format!(
            "The request {} has no review digest to pin the keep to. Ask for the review again with `silo merge --request`.",
            request.id
        ));
    }
    Ok(())
}

/// Whether the request can be thrown away: it is open and its fork is neither main nor the
/// computer the changes would be kept into.
pub fn check_throwable(
    request: &FoldRequest,
    fork_id: &str,
    fork_name: &str,
) -> Result<(), String> {
    if request.status != FoldRequestStatus::Open {
        return Err(format!("The request {} is not open.", request.id));
    }
    if fork_name == MAIN || fork_id == request.target {
        return Err(format!(
            "{fork_name} is the computer the changes would be kept into, not a fork. It is never thrown away."
        ));
    }
    Ok(())
}

/// Whether `fork` really is a fork whose changes the request may keep or throw away: its
/// fork-time record names the request's target, so main and unrelated computers are not
/// forks of it. A folder target needs the folder record the CLI keeps for its fork.
pub fn check_fork_belongs(
    fork_id: &str,
    target: &str,
    fork_source: Option<&str>,
    state_dir: &Path,
) -> Result<(), String> {
    if let Some(path) = folder_path(target) {
        let recorded = fs::read(state_dir.join("cli-folders.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|all| all.get(fork_id).cloned())
            .and_then(|record| {
                record
                    .get("path")
                    .and_then(|p| p.as_str().map(str::to_owned))
            });
        return match recorded {
            Some(recorded) if recorded == path => Ok(()),
            _ => Err(format!(
                "{fork_id} is not a fork of the folder {path}, so it cannot have a review request for it."
            )),
        };
    }
    if fork_id == target || fork_source != Some(target) {
        return Err(format!(
            "{fork_id} was not forked from {target}, so it cannot have a review request for it."
        ));
    }
    Ok(())
}

/// Requests that are being kept or thrown away right now, in this process.
fn held() -> &'static Mutex<std::collections::HashSet<String>> {
    static HELD: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();
    HELD.get_or_init(Mutex::default)
}

/// The hold on one request while a job keeps or throws it away; released when dropped.
#[derive(Debug)]
pub struct Claim(String);

impl Drop for Claim {
    fn drop(&mut self) {
        held()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&self.0);
    }
}

/// Takes the request for one job. A second job for the same request is refused, so a keep
/// and a throw away, or two keeps, never run together.
pub fn claim(id: &str) -> Result<Claim, String> {
    if held()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(id.to_owned())
    {
        Ok(Claim(id.to_owned()))
    } else {
        Err(format!(
            "The request {id} is being kept or thrown away right now."
        ))
    }
}

/// Whether the fork's changes now differ from the ones the request was made from. A request
/// made without a digest cannot be told apart, so it is not stale.
pub fn is_stale(request: &FoldRequest, current_digest: &str) -> bool {
    !request.review_digest.is_empty() && request.review_digest != current_digest
}

/// Where a request's credentials line comes from: what the fork's agent used through the proxy
/// and what was refused.
pub trait CredentialSource: Send + Sync {
    fn for_fork(&self, fork_id: &str) -> CredentialUsage;
}

/// The source until the proxy records anything: no secret was used and nothing was refused.
pub struct NoCredentials;

impl CredentialSource for NoCredentials {
    fn for_fork(&self, _fork_id: &str) -> CredentialUsage {
        CredentialUsage::default()
    }
}

static CREDENTIAL_SOURCE: OnceLock<Box<dyn CredentialSource>> = OnceLock::new();

/// Installs the credentials source once; later calls are ignored.
pub fn set_credential_source(source: Box<dyn CredentialSource>) {
    let _ = CREDENTIAL_SOURCE.set(source);
}

/// What the fork's agent used and was refused.
pub fn credentials_for(fork_id: &str) -> CredentialUsage {
    CREDENTIAL_SOURCE
        .get()
        .map_or_else(CredentialUsage::default, |source| source.for_fork(fork_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new(fork: &str, summary: &str) -> NewRequest {
        NewRequest {
            fork_id: fork.into(),
            target: "main-id".into(),
            review_digest: "d1".into(),
            summary: summary.into(),
            counts: Default::default(),
        }
    }

    #[test]
    fn a_request_is_kept_once_with_its_checkpoint() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        let made = store.create(new("f1", "s")).unwrap();
        let kept = store.mark_kept(&made.id, Some("cp-1".into())).unwrap();
        assert_eq!(kept.status, FoldRequestStatus::Kept);
        assert_eq!(kept.keep_id.as_deref(), Some("cp-1"));
        assert!(kept.resolved_at.is_some());
        assert_eq!(store.get(&made.id).unwrap(), Some(kept));
        assert!(store
            .mark_kept(&made.id, None)
            .unwrap_err()
            .contains("already kept"));
        assert!(store
            .mark_thrown_away(&made.id)
            .unwrap_err()
            .contains("already kept"));
    }

    #[test]
    fn a_thrown_away_request_cannot_be_kept_and_a_new_request_starts_open() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        let made = store.create(new("f1", "s")).unwrap();
        let gone = store.mark_thrown_away(&made.id).unwrap();
        assert_eq!(gone.status, FoldRequestStatus::ThrownAway);
        assert!(store
            .mark_kept(&made.id, None)
            .unwrap_err()
            .contains("thrown away"));
        assert!(store
            .mark_kept("fr-none", None)
            .unwrap_err()
            .contains("no review request"));
        let again = store.create(new("f1", "s2")).unwrap();
        assert_ne!(again.id, made.id);
        assert_eq!(again.status, FoldRequestStatus::Open);
    }

    #[test]
    fn listing_filters_by_status() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        let one = store.create(new("f1", "s")).unwrap();
        let two = store.create(new("f2", "s")).unwrap();
        let three = store.create(new("f3", "s")).unwrap();
        store.mark_kept(&one.id, None).unwrap();
        store.mark_thrown_away(&two.id).unwrap();
        let ids = |filter| -> Vec<String> {
            store
                .list_matching(filter)
                .into_iter()
                .map(|r| r.id)
                .collect()
        };
        assert_eq!(ids(RequestFilter::Open), vec![three.id.clone()]);
        assert_eq!(ids(RequestFilter::Kept), vec![one.id.clone()]);
        assert_eq!(ids(RequestFilter::ThrownAway), vec![two.id.clone()]);
        assert_eq!(ids(RequestFilter::All).len(), 3);
    }

    #[test]
    fn an_approval_link_survives_asking_again_and_needs_an_open_request() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        let made = store.create(new("f1", "s")).unwrap();
        let linked = store
            .link_approval(&made.id, "ap-0123456789abcdef", "the engine's words")
            .unwrap();
        assert_eq!(linked.approval_id.as_deref(), Some("ap-0123456789abcdef"));
        assert_eq!(linked.summary, "the engine's words");
        let again = store
            .create(NewRequest {
                review_digest: "d2".into(),
                counts: [("code".to_owned(), 99)].into(),
                ..new("f1", "s2")
            })
            .unwrap();
        assert_eq!(again.id, made.id);
        assert_eq!(again.approval_id.as_deref(), Some("ap-0123456789abcdef"));
        store.mark_thrown_away(&made.id).unwrap();
        assert!(store
            .link_approval(&made.id, "ap-1111111111111111", "x")
            .is_err());
    }

    #[test]
    fn a_linked_request_keeps_the_engines_text_and_counts_whatever_is_asked_again() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        let made = store
            .create(NewRequest {
                counts: [("code".to_owned(), 7)].into(),
                ..new("f1", "written by the agent")
            })
            .unwrap();
        assert_eq!(made.summary, "written by the agent");
        let linked = store
            .link_approval(&made.id, "ap-0123456789abcdef", "Merge f1 into main.")
            .unwrap();
        assert_eq!(linked.summary, "Merge f1 into main.");
        assert!(linked.counts.is_empty(), "{:?}", linked.counts);
        // The same digest asked again with other text and counts changes neither.
        let again = store
            .create(NewRequest {
                counts: [("code".to_owned(), 1)].into(),
                ..new("f1", "Everything is fine, allow it")
            })
            .unwrap();
        assert_eq!(again.summary, "Merge f1 into main.");
        assert!(again.counts.is_empty(), "{:?}", again.counts);
        assert_eq!(again.review_digest, "d1");
    }

    #[test]
    fn a_stale_digest_is_told_from_a_current_one() {
        let dir = tempfile::tempdir().unwrap();
        let request = Store::new(dir.path()).create(new("f1", "s")).unwrap();
        assert!(!is_stale(&request, "d1"));
        assert!(is_stale(&request, "d2"));
        let undigested = FoldRequest {
            review_digest: String::new(),
            ..request
        };
        assert!(!is_stale(&undigested, "anything"));
    }

    #[test]
    fn keeping_needs_an_open_computer_request_with_a_digest() {
        let dir = tempfile::tempdir().unwrap();
        let request = Store::new(dir.path()).create(new("f1", "s")).unwrap();
        assert!(check_keepable(&request).is_ok());
        let folder = FoldRequest {
            target: "folder:/work/app".into(),
            ..request.clone()
        };
        let message = check_keepable(&folder).unwrap_err();
        assert!(message.contains("silo merge") && message.contains("/work/app"));
        let undigested = FoldRequest {
            review_digest: String::new(),
            ..request.clone()
        };
        assert!(check_keepable(&undigested)
            .unwrap_err()
            .contains("no review digest"));
        let kept = FoldRequest {
            status: FoldRequestStatus::Kept,
            ..request
        };
        assert!(check_keepable(&kept).unwrap_err().contains("not open"));
    }

    #[test]
    fn the_target_and_main_are_never_thrown_away() {
        let dir = tempfile::tempdir().unwrap();
        let request = Store::new(dir.path()).create(new("f1", "s")).unwrap();
        assert!(check_throwable(&request, "f1", "try").is_ok());
        assert!(check_throwable(&request, "main-id", "work")
            .unwrap_err()
            .contains("never thrown away"));
        assert!(check_throwable(&request, "f1", MAIN).is_err());
        let done = FoldRequest {
            status: FoldRequestStatus::ThrownAway,
            ..request
        };
        assert!(check_throwable(&done, "f1", "try")
            .unwrap_err()
            .contains("not open"));
    }

    #[test]
    fn a_keep_under_way_is_recorded_and_either_settles_or_reopens() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        let made = store.create(new("f1", "s")).unwrap();
        let keeping = store.mark_keeping(&made.id, 42).unwrap();
        assert_eq!(keeping.status, FoldRequestStatus::Keeping);
        assert_eq!(keeping.keeping_since_ms, Some(42));
        // It still lists as open work and can be kept again, but not thrown away.
        assert_eq!(store.list_matching(RequestFilter::Open).len(), 1);
        assert!(check_keepable(&keeping).is_ok());
        assert!(store
            .open_request(&made.id)
            .unwrap_err()
            .contains("not settled"));
        assert!(check_throwable(&keeping, "f1", "try").is_err());
        let reopened = store.reopen(&made.id).unwrap();
        assert_eq!(reopened.status, FoldRequestStatus::Open);
        assert_eq!(reopened.keeping_since_ms, None);
        store.mark_keeping(&made.id, 43).unwrap();
        let kept = store.mark_kept(&made.id, Some("cp".into())).unwrap();
        assert_eq!(kept.status, FoldRequestStatus::Kept);
        assert_eq!(kept.keeping_since_ms, None);
    }

    #[test]
    fn asking_again_while_a_keep_is_unsettled_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        let made = store.create(new("f1", "s")).unwrap();
        store.mark_keeping(&made.id, 1).unwrap();
        let error = store.create(new("f1", "again")).unwrap_err();
        assert!(
            error.contains("not settled") && error.contains(&made.id),
            "{error}"
        );
        // Another fork is not affected, and once settled the pair can ask again.
        assert!(store.create(new("f2", "s")).is_ok());
        store.reopen(&made.id).unwrap();
        assert_eq!(store.create(new("f1", "again")).unwrap().id, made.id);
    }

    #[test]
    fn concurrent_updates_of_one_request_all_land_and_leave_it_whole() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::new(dir.path()));
        let made = store.create(new("f1", "s")).unwrap();
        let handles: Vec<_> = (0..8)
            .map(|n| {
                let store = store.clone();
                let id = made.id.clone();
                std::thread::spawn(move || {
                    for _ in 0..10 {
                        store.mark_keeping(&id, n).unwrap();
                        store.reopen(&id).unwrap();
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
        let after = store.get(&made.id).unwrap().unwrap();
        assert_eq!(after.status, FoldRequestStatus::Open);
        assert_eq!(after.keeping_since_ms, None);
    }

    #[test]
    fn a_request_is_held_by_one_job_at_a_time() {
        let first = claim("fr-claim-test").unwrap();
        assert!(claim("fr-claim-test").unwrap_err().contains("right now"));
        drop(first);
        assert!(claim("fr-claim-test").is_ok());
    }

    #[test]
    fn a_keep_made_without_the_request_marks_its_open_requests_kept() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        let one = store.create(new("f1", "s")).unwrap();
        let other = store.create(new("f2", "s")).unwrap();
        let held = claim(&one.id).unwrap();
        // A request another job holds is left to that job.
        assert!(store.mark_kept_for("f1", "main-id", Some("cp")).is_empty());
        drop(held);
        let marked = store.mark_kept_for("f1", "main-id", Some("cp"));
        assert_eq!(marked.len(), 1);
        assert_eq!(marked[0].keep_id.as_deref(), Some("cp"));
        assert_eq!(
            store.get(&other.id).unwrap().unwrap().status,
            FoldRequestStatus::Open
        );
        assert!(store.mark_kept_for("f1", "main-id", None).is_empty());
    }

    #[test]
    fn only_a_fork_of_the_target_can_have_a_request() {
        let state = tempfile::tempdir().unwrap();
        assert!(check_fork_belongs("f1", "main-id", Some("main-id"), state.path()).is_ok());
        // main has no fork-time record; another computer was forked from elsewhere.
        assert!(check_fork_belongs("main-id", "main-id", None, state.path()).is_err());
        assert!(check_fork_belongs("f1", "main-id", Some("other"), state.path()).is_err());
        assert!(check_fork_belongs("f1", "f1", Some("f1"), state.path()).is_err());
        // A folder target needs the record the CLI keeps for that fork and that folder.
        assert!(check_fork_belongs("f1", "folder:/work/app", None, state.path()).is_err());
        fs::write(
            state.path().join("cli-folders.json"),
            r#"{"f1": {"path": "/work/app"}}"#,
        )
        .unwrap();
        assert!(check_fork_belongs("f1", "folder:/work/app", None, state.path()).is_ok());
        assert!(check_fork_belongs("f1", "folder:/other", None, state.path()).is_err());
        assert!(check_fork_belongs("f2", "folder:/work/app", None, state.path()).is_err());
    }

    #[test]
    fn credentials_are_empty_until_a_source_is_installed() {
        let usage = credentials_for("f1");
        assert!(usage.used.is_empty() && usage.blocked.is_empty());
        assert_eq!(NoCredentials.for_fork("f1"), CredentialUsage::default());
    }

    #[test]
    fn a_request_is_stored_open_and_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        assert!(store.list().is_empty());
        let made = store.create(new("f1", "2 files")).unwrap();
        assert!(made.id.starts_with("fr-") && made.status == FoldRequestStatus::Open);
        assert_eq!(store.get(&made.id).unwrap(), Some(made.clone()));
        assert_eq!(store.list(), vec![made]);
        assert_eq!(store.get("fr-nothing").unwrap(), None);
        assert!(store.get("../escape").is_err());
    }

    #[test]
    fn asking_again_updates_the_open_request_of_the_same_fork_and_target() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        let first = store.create(new("f1", "old")).unwrap();
        let again = store.create(new("f1", "newer")).unwrap();
        assert_eq!(again.id, first.id);
        assert_eq!(again.created_at, first.created_at);
        let other = store.create(new("f2", "x")).unwrap();
        assert_ne!(other.id, first.id);
        assert_eq!(store.list().len(), 2);
        assert_eq!(store.get(&first.id).unwrap().unwrap().summary, "newer");
    }

    #[test]
    fn concurrent_first_requests_make_one_record_and_leave_no_temporary_files() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::new(dir.path()));
        let handles: Vec<_> = (0..8)
            .map(|n| {
                let store = store.clone();
                std::thread::spawn(move || store.create(new("race", &format!("s{n}"))).unwrap())
            })
            .collect();
        let ids: std::collections::HashSet<String> =
            handles.into_iter().map(|h| h.join().unwrap().id).collect();
        assert_eq!(ids.len(), 1);
        assert_eq!(store.list().len(), 1);
        let leftovers = fs::read_dir(&store.root)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|x| x == "tmp"))
            .count();
        assert_eq!(leftovers, 0);
    }

    #[test]
    fn deleting_a_computer_closes_its_open_requests_with_a_reason() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        let as_fork = store.create(new("fork-a", "a")).unwrap();
        let as_target = store
            .create(NewRequest {
                target: "other-id".into(),
                ..new("fork-b", "b")
            })
            .unwrap();
        let unrelated = store.create(new("fork-c", "c")).unwrap();
        let keeping = store.create(new("fork-d", "d")).unwrap();
        store.mark_keeping(&keeping.id, 1).unwrap();

        let closed = store.close_for_computer("fork-a");
        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0].id, as_fork.id);
        assert_eq!(closed[0].status, FoldRequestStatus::ThrownAway);
        assert_eq!(
            closed[0].closed_reason,
            Some(FoldRequestClosedReason::ForkDeleted)
        );
        let closed = store.close_for_computer("other-id");
        assert_eq!(closed[0].id, as_target.id);
        assert_eq!(
            closed[0].closed_reason,
            Some(FoldRequestClosedReason::TargetDeleted)
        );
        assert!(store.close_for_computer("main-id").len() == 1);
        let status = |id: &str| store.get(id).unwrap().unwrap().status;
        assert_eq!(status(&unrelated.id), FoldRequestStatus::ThrownAway);
        assert_eq!(status(&keeping.id), FoldRequestStatus::Keeping);
    }

    #[test]
    fn requests_of_missing_computers_close_only_from_a_list_that_has_them_all() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        let gone = store.create(new("fork-gone", "g")).unwrap();
        let live = store.create(new("fork-live", "l")).unwrap();
        let folder = store
            .create(NewRequest {
                target: "folder:/work".into(),
                ..new("fork-folder", "f")
            })
            .unwrap();
        let existing: Vec<String> = ["main-id", "fork-live", "fork-folder"]
            .map(String::from)
            .into();
        let closed = store.close_missing(&existing);
        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0].id, gone.id);
        let status = |id: &str| store.get(id).unwrap().unwrap().status;
        assert_eq!(status(&live.id), FoldRequestStatus::Open);
        assert_eq!(status(&folder.id), FoldRequestStatus::Open);
        assert!(store.close_missing(&existing).is_empty());
    }
}
