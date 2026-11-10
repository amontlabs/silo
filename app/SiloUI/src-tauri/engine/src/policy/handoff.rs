//! What a promote or an undo does to the record of who made which computer. A promote keeps
//! both computers' ids and swaps their names, so the fork becomes main while the record still
//! says the old main made it. The record is moved with the swap instead: the fork is no longer
//! made by anyone (or takes the old main's place under the computer that made the old main),
//! and everything the old main made, with its pending computers and jobs, is the fork's.

use serde::{Deserialize, Serialize};

use super::store::{Created, Document, OwnedJob, Pending, PolicyStore};

/// The record as it stood before a promote, kept in the promote's journal so an undo puts
/// back exactly what the promote moved.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Handoff {
    /// Who made the promoted fork.
    #[serde(default)]
    pub fork: Option<Created>,
    /// Who made the old main.
    #[serde(default)]
    pub target: Option<Created>,
    /// The computers the old main made itself, other than the fork.
    #[serde(default)]
    pub moved: Vec<Created>,
    /// The computers the old main was making.
    #[serde(default)]
    pub pending: Vec<Pending>,
    /// The jobs the old main started that concern the computers it made.
    #[serde(default)]
    pub jobs: Vec<OwnedJob>,
}

impl Handoff {
    fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

impl Document {
    /// What a promote of `fork` over `target` moves, as the record stands now.
    pub fn handoff(&self, fork: &str, target: &str) -> Handoff {
        let moved: Vec<Created> = self
            .created
            .iter()
            .filter(|record| record.created_by == target && record.id != fork)
            .cloned()
            .collect();
        let pending: Vec<Pending> = self
            .pending
            .iter()
            .filter(|pending| pending.created_by == target)
            .cloned()
            .collect();
        let jobs = self
            .jobs
            .iter()
            .filter(|job| job.caller == target && concerns(job, fork, &moved, &pending))
            .cloned()
            .collect();
        Handoff {
            fork: self.created.iter().find(|record| record.id == fork).cloned(),
            target: self
                .created
                .iter()
                .find(|record| record.id == target)
                .cloned(),
            moved,
            pending,
            jobs,
        }
    }

    /// `fork` replaces `target`. Repeating it changes nothing more. `saved` is what
    /// [`Document::handoff`] read before the promote, which tells who made `target` once its
    /// own record is gone.
    pub fn hand_over(&mut self, fork: &str, target: &str, saved: &Handoff) {
        let now = self.handoff(fork, target);
        let maker = self
            .created
            .iter()
            .find(|record| record.id == target)
            .or(saved.target.as_ref())
            .cloned();
        self.created
            .retain(|record| record.id != fork && record.id != target);
        for record in &mut self.created {
            if now.moved.iter().any(|moved| moved.id == record.id) {
                record.created_by = fork.to_owned();
            }
        }
        for pending in &mut self.pending {
            if pending.created_by == target {
                pending.created_by = fork.to_owned();
            }
        }
        for job in &mut self.jobs {
            if now.jobs.iter().any(|moved| moved.id == job.id) && job.caller == target {
                job.caller = fork.to_owned();
            }
        }
        if let Some(maker) = maker {
            let loops = maker.created_by == fork || self.made_by(fork).contains(&maker.created_by);
            if !loops {
                self.created.push(Created {
                    id: fork.to_owned(),
                    created_by: maker.created_by,
                    at: maker.at,
                });
            }
        }
    }

    /// The promote of `fork` over `target` is undone: the record goes back to `saved`.
    /// A computer deleted in between stays deleted.
    pub fn hand_back(&mut self, fork: &str, target: &str, saved: &Handoff) {
        if saved.is_empty() {
            return;
        }
        self.created.retain(|record| record.id != fork);
        if let Some(record) = &saved.fork {
            self.created.push(record.clone());
        }
        if let Some(record) = &saved.target {
            if !self.created.iter().any(|other| other.id == target) {
                self.created.push(record.clone());
            }
        }
        for record in &mut self.created {
            if let Some(before) = saved.moved.iter().find(|before| before.id == record.id) {
                *record = before.clone();
            }
        }
        for pending in &mut self.pending {
            let before = saved.pending.iter().find(|before| {
                before.name == pending.name
                    && before.job_id == pending.job_id
                    && pending.created_by == fork
            });
            if let Some(before) = before {
                pending.created_by = before.created_by.clone();
            }
        }
        for job in &mut self.jobs {
            let before = saved
                .jobs
                .iter()
                .find(|before| before.id == job.id && job.caller == fork);
            if let Some(before) = before {
                job.caller = before.caller.clone();
            }
        }
    }
}

/// Whether a job the old main started concerns the fork, a computer it made, or a computer it
/// was making.
fn concerns(job: &OwnedJob, fork: &str, moved: &[Created], pending: &[Pending]) -> bool {
    job.computer
        .as_deref()
        .is_some_and(|computer| computer == fork || moved.iter().any(|made| made.id == computer))
        || pending
            .iter()
            .any(|pending| pending.job_id.as_deref() == Some(job.id.as_str()))
}

impl PolicyStore {
    /// What a promote of `fork` over `target` would move.
    pub fn handoff(&self, fork: &str, target: &str) -> Result<Handoff, String> {
        Ok(self.snapshot()?.handoff(fork, target))
    }

    pub fn hand_over(&self, fork: &str, target: &str, saved: &Handoff) -> Result<(), String> {
        self.update(|document| {
            document.hand_over(fork, target, saved);
            Ok(())
        })
    }

    pub fn hand_back(&self, fork: &str, target: &str, saved: &Handoff) -> Result<(), String> {
        self.update(|document| {
            document.hand_back(fork, target, saved);
            Ok(())
        })
    }

    /// The running engine's record. `None` when no engine is installed (unit tests of other
    /// modules).
    pub fn running() -> Option<Self> {
        crate::context::current().map(|context| Self::new(&context.paths().app_data))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use serde_json::{json, Value};
    use silo_api::methods::MethodName;

    use super::*;
    use crate::policy::{decide, Access, Caller, Decision, Resources, World};

    fn made(id: &str, by: &str) -> Created {
        Created {
            id: id.into(),
            created_by: by.into(),
            at: 1,
        }
    }

    /// The old main `a` made the fork `f` and `c`; `c` made `d`.
    fn document() -> Document {
        Document {
            version: 1,
            created: vec![made("f", "a"), made("c", "a"), made("d", "c")],
            ..Document::default()
        }
    }

    struct Recorded(Document);

    impl World for Recorded {
        fn made(&self, creator: &str) -> HashSet<String> {
            self.0.made_by(creator)
        }
        fn parent(&self, _computer: &str) -> Option<String> {
            Some("a".into())
        }
        fn name(&self, computer: &str) -> Option<String> {
            Some(computer.to_owned())
        }
        fn owns_job(&self, caller: &str, job_id: &str) -> bool {
            self.0.owns_job(caller, job_id)
        }
        fn request_fork(&self, _request_id: &str) -> Option<String> {
            None
        }
        fn request_target(&self, _request_id: &str) -> Option<String> {
            None
        }
        fn resources(&self, _computer: &str) -> Option<Resources> {
            None
        }
        fn ssh_settings(&self, _computer: &str) -> Option<(u16, String)> {
            None
        }
        fn access(&self, _computer: &str) -> Access {
            Access::default()
        }
        fn github_policies(&self) -> Option<Vec<Value>> {
            None
        }
        fn new_id(&self) -> String {
            "fresh".into()
        }
    }

    fn ask(caller: &str, method: &str, computer: &str, world: &Recorded) -> Decision {
        let method = MethodName::from_name(method).unwrap();
        decide(
            &Caller::Computer { id: caller.into() },
            method,
            &json!({"computerId": computer, "command": "id", "checkpointId": "cp"}),
            world,
        )
    }

    #[test]
    fn the_old_main_cannot_reach_the_new_main_after_a_promote() {
        let mut record = document();
        for method in ["guest.exec", "computers.delete", "checkpoints.restore"] {
            assert!(
                matches!(
                    ask("a", method, "f", &Recorded(record.clone())),
                    Decision::Allow(_)
                ),
                "{method}: the maker reaches the fork before the promote"
            );
        }
        let saved = record.handoff("f", "a");
        record.hand_over("f", "a", &saved);
        let world = Recorded(record);
        for method in ["guest.exec", "computers.delete", "checkpoints.restore"] {
            for target in ["f", "c", "d"] {
                assert!(
                    matches!(ask("a", method, target, &world), Decision::Refuse { .. }),
                    "{method} on {target}"
                );
            }
            assert!(
                matches!(ask("f", method, "c", &world), Decision::Allow(_)),
                "{method}: the new main reaches what the old main made"
            );
        }
    }

    #[test]
    fn the_new_main_takes_the_old_mains_place_under_its_maker() {
        let mut record = document();
        record.created.push(made("a", "m"));
        let saved = record.handoff("f", "a");
        record.hand_over("f", "a", &saved);
        assert_eq!(record.made_by("m").len(), 3);
        assert!(record.made_by("a").is_empty());
        assert!(!record.made_by("m").contains("a"));
        record.hand_back("f", "a", &saved);
        let mut want = document();
        want.created.push(made("a", "m"));
        let sorted = |mut document: Document| {
            document.created.sort_by(|a, b| a.id.cmp(&b.id));
            document
        };
        assert_eq!(sorted(record), sorted(want));
    }

    #[test]
    fn a_fork_made_by_a_computer_the_old_main_made_is_not_left_in_a_loop() {
        let mut record = Document {
            created: vec![made("x", "a"), made("f", "x")],
            ..Document::default()
        };
        let saved = record.handoff("f", "a");
        record.hand_over("f", "a", &saved);
        assert_eq!(record.made_by("f"), ["x".to_owned()].into());
        assert!(record.made_by("x").is_empty());
    }

    #[test]
    fn the_store_keeps_the_move_and_the_undo() {
        let directory = tempfile::tempdir().unwrap();
        let store = PolicyStore::new(directory.path());
        store
            .update(|document| {
                *document = document_with_a_job();
                Ok(())
            })
            .unwrap();
        let before = store.snapshot().unwrap();
        let saved = store.handoff("f", "a").unwrap();
        store.hand_over("f", "a", &saved).unwrap();
        let moved = store.snapshot().unwrap();
        assert_eq!(moved.made_by("f"), ["c".to_owned(), "d".to_owned()].into());
        assert!(moved.owns_job("f", "job:1") && !moved.owns_job("a", "job:1"));
        store.hand_back("f", "a", &saved).unwrap();
        let mut now = store.snapshot().unwrap();
        let mut want = before;
        for document in [&mut now, &mut want] {
            document.created.sort_by(|a, b| a.id.cmp(&b.id));
        }
        assert_eq!(now, want);
    }

    fn document_with_a_job() -> Document {
        let mut record = document();
        record.jobs.push(OwnedJob {
            caller: "a".into(),
            id: "job:1".into(),
            at: 2,
            computer: Some("c".into()),
        });
        record
    }
}
