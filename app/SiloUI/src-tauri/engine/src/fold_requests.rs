//! Review requests: `changes.request` records that a fork waits for a review, without keeping
//! it. One JSON file per request under `<app data>/fold-requests/`. Listing, showing, keeping
//! and throwing away a request build on this store.

use std::{
    fs,
    path::{Path, PathBuf},
};

use silo_api::methods::keep::{FoldRequest, FoldRequestStatus};

pub struct Store {
    root: PathBuf,
}

/// What a request is made from.
pub struct NewRequest {
    pub fork_id: String,
    pub target: String,
    pub review_digest: String,
    pub summary: String,
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
        let existing = self.list().into_iter().find(|request| {
            request.fork_id == new.fork_id
                && request.target == new.target
                && request.status == FoldRequestStatus::Open
        });
        let request = FoldRequest {
            id: existing.as_ref().map_or_else(
                || format!("fr-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]),
                |request| request.id.clone(),
            ),
            fork_id: new.fork_id,
            target: new.target,
            created_at: existing.map_or_else(now, |request| request.created_at),
            review_digest: new.review_digest,
            summary: new.summary,
            status: FoldRequestStatus::Open,
        };
        self.put(&request)?;
        Ok(request)
    }

    /// Writes the request whole: a reader sees the old file or the new one.
    pub fn put(&self, request: &FoldRequest) -> Result<(), String> {
        let path = self.file(&request.id)?;
        fs::create_dir_all(&self.root)
            .map_err(|error| format!("Cannot create the request store: {error}"))?;
        let text = serde_json::to_string_pretty(request).map_err(|error| error.to_string())?;
        let temporary = path.with_extension("json.tmp");
        fs::write(&temporary, text)
            .and_then(|()| fs::rename(&temporary, &path))
            .map_err(|error| format!("Cannot save the request {}: {error}", request.id))
    }
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
        }
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
}
