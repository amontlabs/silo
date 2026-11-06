//! Lineage after a promote, read from the promote journal.
//!
//! A fork's source is the computer it was made from. When a fork replaces that computer, the
//! forks that were made from the old main belong to whichever computer is main now, and the
//! promoted fork is main with the old main's own source. Nothing is rewritten in the stores:
//! while a promote is in force the lookup below follows the journal, and an undo (which ends
//! the record) puts every answer back.

use std::sync::Arc;

use super::journal::{Journal, Record, State};
use crate::changes::keep::ManifestStore;

/// Longest chain of promotes followed (a computer replaced by a fork that was replaced...).
const CHAIN: usize = 16;

fn promoted(journal: &Journal) -> Vec<Record> {
    journal
        .in_force()
        .into_iter()
        .filter(|record| record.state == State::Promoted)
        .collect()
}

/// The computer that holds the role `id` had when it was the source of a fork.
fn redirect(records: &[Record], id: &str) -> String {
    let mut current = id.to_owned();
    for _ in 0..CHAIN {
        let Some(next) = records
            .iter()
            .filter(|record| record.swap.target_id == current)
            .max_by_key(|record| record.at)
        else {
            break;
        };
        current = next.swap.fork_id.clone();
    }
    current
}

/// What `fork` was made from, as it reads after the promotes in force.
pub fn source_of(journal: &Journal, fork: &str, raw: Option<String>) -> Option<String> {
    let records = promoted(journal);
    if let Some(record) = records
        .iter()
        .filter(|record| record.swap.fork_id == fork)
        .max_by_key(|record| record.at)
    {
        return record
            .swap
            .target_source
            .as_deref()
            .map(|source| redirect(&records, source));
    }
    raw.map(|source| redirect(&records, &source))
}

/// The forks that have replaced a computer and are main now.
pub fn promoted_forks(journal: &Journal) -> std::collections::HashSet<String> {
    promoted(journal)
        .into_iter()
        .map(|record| record.swap.fork_id)
        .collect()
}

/// A manifest store whose sources follow the promotes in force.
pub struct Lineage {
    inner: Arc<dyn ManifestStore>,
    journal: Journal,
}

impl Lineage {
    pub fn new(inner: Arc<dyn ManifestStore>, journal: Journal) -> Self {
        Self { inner, journal }
    }
}

impl ManifestStore for Lineage {
    fn fork_time(&self, fork: &str) -> Result<String, String> {
        self.inner.fork_time(fork)
    }

    fn capture(&self, computer: &str) -> Result<String, String> {
        self.inner.capture(computer)
    }

    fn baseline(&self, fork: &str) -> Option<(String, String)> {
        self.inner.baseline(fork)
    }

    fn source(&self, fork: &str) -> Result<Option<String>, String> {
        let raw = self.inner.source(fork)?;
        Ok(source_of(&self.journal, fork, raw))
    }
}
