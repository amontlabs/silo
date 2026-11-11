//! `computers.tree`: how the computers on this device descend from each other.
//!
//! The parent and origin of a fork come from the fork-time record the changes store already
//! keeps (`changes::store::ForkLink`), read through the promote journal so that the tree
//! follows a promote: forks of the old main hang under the promoted fork, which is a root (or
//! takes the old main's own parent). Nothing here is stored.

use std::collections::{BTreeMap, HashMap, HashSet};

use silo_api::methods::steps::{PendingChanges, TreeFrom, TreeNode};

use crate::changes::store::ForkLink;

/// A computer as the tree needs to see it.
#[derive(Clone, Debug)]
pub struct Computer {
    pub id: String,
    pub name: String,
    pub state: String,
    /// The computer's checkpoints as (id, kind).
    pub checkpoints: Vec<(String, String)>,
}

/// Where each answer comes from.
pub struct Sources<'a> {
    /// The fork-time record of a computer.
    pub link: &'a dyn Fn(&Computer) -> Option<ForkLink>,
    /// A fork's parent after the promotes in force, given the computer and its recorded origin.
    pub parent: &'a dyn Fn(&str, Option<String>) -> Option<String>,
    /// The forks that have replaced a computer.
    pub promoted: &'a HashSet<String>,
    /// The step number a checkpoint was taken for.
    pub step_of: &'a dyn Fn(&str) -> Option<u32>,
    /// The id of a computer's open review request.
    pub open_request: &'a dyn Fn(&str) -> Option<String>,
}

/// Names of the throwaway computers Silo makes for its own measurements, recipe builds,
/// exports without logins and rebuilds on the newest image.
pub fn hidden(name: &str) -> bool {
    name.starts_with("silo-baseline-")
        || name.starts_with("silo-build-")
        || name.starts_with(crate::export_scrub::COPY_PREFIX)
        || name.starts_with(crate::rebuild::BUILD_PREFIX)
}

fn from_of(
    computer: &Computer,
    link: &ForkLink,
    parent: &Option<String>,
    all: &HashMap<&str, &Computer>,
    sources: &Sources<'_>,
) -> TreeFrom {
    if sources.promoted.contains(&computer.id) {
        return if parent.is_none() {
            TreeFrom::Root
        } else {
            TreeFrom::Main
        };
    }
    // A recipe fork comes from a hidden build computer, so its provenance does not depend on
    // a parent being shown.
    if let Some(hash) = link
        .source
        .as_deref()
        .and_then(|source| source.strip_prefix(crate::changes::fork_base::RECIPE_KEY_PREFIX))
    {
        return TreeFrom::Recipe {
            hash: hash.to_owned(),
        };
    }
    if parent.is_none() {
        return TreeFrom::Root;
    }
    let Some(source) = link.source.as_deref() else {
        return TreeFrom::Main;
    };
    if let Some(number) = link.step.or_else(|| (sources.step_of)(source)) {
        return TreeFrom::Step {
            number,
            checkpoint_id: source.to_owned(),
        };
    }
    if all.contains_key(source) {
        return TreeFrom::Main;
    }
    let kind = all
        .values()
        .flat_map(|other| other.checkpoints.iter())
        .find(|(id, _)| id == source)
        .map(|(_, kind)| kind.as_str());
    match kind {
        Some("fork-point" | "warm") => TreeFrom::Main,
        _ => TreeFrom::Checkpoint {
            checkpoint_id: source.to_owned(),
        },
    }
}

/// The tree of `computers`, read from the engine's own records.
pub fn read(context: &dyn crate::context::EngineContext, computers: &[Computer]) -> Vec<TreeNode> {
    use silo_api::methods::keep::FoldRequestStatus;

    let store = crate::changes::store::Store::for_engine(context);
    let journal = crate::changes::promote::service::journal_for(context);
    let promoted = crate::changes::promote::lineage::promoted_forks(&journal);
    let steps = crate::steps::StepStore::for_engine(context);
    let requests = crate::fold_requests::Store::new(&context.paths().app_data).list();
    build(
        computers,
        &Sources {
            link: &|computer| store.fork_link(&computer.id, &computer.name),
            parent: &|id, raw| crate::changes::promote::lineage::source_of(&journal, id, raw),
            promoted: &promoted,
            step_of: &|checkpoint| {
                steps
                    .step_of_checkpoint(checkpoint)
                    .map(|(_, number)| number)
            },
            open_request: &|id| {
                requests
                    .iter()
                    .find(|request| {
                        request.fork_id == id && request.status == FoldRequestStatus::Open
                    })
                    .map(|request| request.id.clone())
            },
        },
    )
}

/// The tree, parents before their children and siblings by name.
pub fn build(computers: &[Computer], sources: &Sources<'_>) -> Vec<TreeNode> {
    let shown: Vec<&Computer> = computers
        .iter()
        .filter(|computer| !hidden(&computer.name))
        .collect();
    let all: HashMap<&str, &Computer> = shown
        .iter()
        .map(|computer| (computer.id.as_str(), *computer))
        .collect();
    let mut nodes: Vec<TreeNode> = shown
        .iter()
        .map(|computer| {
            let link = (sources.link)(computer).unwrap_or_default();
            let raw = link.origin.clone().or_else(|| {
                link.source
                    .clone()
                    .filter(|source| all.contains_key(source.as_str()))
            });
            let parent = (sources.parent)(&computer.id, raw)
                .filter(|parent| parent != &computer.id && all.contains_key(parent.as_str()));
            let from = from_of(computer, &link, &parent, &all, sources);
            TreeNode {
                id: computer.id.clone(),
                name: computer.name.clone(),
                parent,
                from,
                state: computer.state.clone(),
                pending_changes: PendingChanges {
                    request_id: (sources.open_request)(&computer.id),
                    buckets: None::<BTreeMap<String, u64>>,
                },
            }
        })
        .collect();
    nodes.sort_by(|left, right| (&left.name, &left.id).cmp(&(&right.name, &right.id)));
    order(nodes)
}

/// Parents first, each followed by its descendants. A cycle (which a damaged record could
/// make) is cut: its members become roots.
fn order(nodes: Vec<TreeNode>) -> Vec<TreeNode> {
    let mut children: HashMap<String, Vec<usize>> = HashMap::new();
    let mut roots = Vec::new();
    for (index, node) in nodes.iter().enumerate() {
        match &node.parent {
            Some(parent) => children.entry(parent.clone()).or_default().push(index),
            None => roots.push(index),
        }
    }
    let mut seen = vec![false; nodes.len()];
    let mut ordered = Vec::with_capacity(nodes.len());
    let mut stack: Vec<usize> = roots.into_iter().rev().collect();
    while let Some(index) = stack.pop() {
        if std::mem::replace(&mut seen[index], true) {
            continue;
        }
        ordered.push(index);
        if let Some(below) = children.get(&nodes[index].id) {
            stack.extend(below.iter().rev().copied());
        }
    }
    let mut leftover: Vec<usize> = (0..nodes.len()).filter(|index| !seen[*index]).collect();
    ordered.append(&mut leftover);
    let mut slots: Vec<Option<TreeNode>> = nodes.into_iter().map(Some).collect();
    let mut result: Vec<TreeNode> = ordered
        .into_iter()
        .filter_map(|index| slots[index].take())
        .collect();
    // A member of a cycle has no root above it; it is shown as one.
    let present: HashSet<String> = result.iter().map(|node| node.id.clone()).collect();
    let mut reached = HashSet::new();
    for node in &mut result {
        if let Some(parent) = &node.parent {
            if !present.contains(parent) || !reached.contains(parent) {
                node.parent = None;
                node.from = TreeFrom::Root;
            }
        }
        reached.insert(node.id.clone());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::changes::promote::{
        journal::{Journal, Record, State, Swap},
        lineage,
    };

    fn computer(id: &str, name: &str, checkpoints: &[(&str, &str)]) -> Computer {
        Computer {
            id: id.into(),
            name: name.into(),
            state: "running".into(),
            checkpoints: checkpoints
                .iter()
                .map(|(id, kind)| ((*id).to_owned(), (*kind).to_owned()))
                .collect(),
        }
    }

    fn link(origin: Option<&str>, source: Option<&str>) -> ForkLink {
        ForkLink {
            origin: origin.map(str::to_owned),
            source: source.map(str::to_owned),
            step: None,
        }
    }

    struct Fixture {
        links: HashMap<String, ForkLink>,
        journal: Journal,
        steps: HashMap<String, u32>,
        requests: HashMap<String, String>,
        _dir: tempfile::TempDir,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            Self {
                links: HashMap::new(),
                journal: Journal::new(dir.path()),
                steps: HashMap::new(),
                requests: HashMap::new(),
                _dir: dir,
            }
        }

        fn build(&self, computers: &[Computer]) -> Vec<TreeNode> {
            let promoted = lineage::promoted_forks(&self.journal);
            build(
                computers,
                &Sources {
                    link: &|computer| self.links.get(&computer.id).cloned(),
                    parent: &|id, raw| lineage::source_of(&self.journal, id, raw),
                    promoted: &promoted,
                    step_of: &|checkpoint| self.steps.get(checkpoint).copied(),
                    open_request: &|id| self.requests.get(id).cloned(),
                },
            )
        }
    }

    fn node<'a>(nodes: &'a [TreeNode], id: &str) -> &'a TreeNode {
        nodes.iter().find(|node| node.id == id).unwrap()
    }

    fn promoted_record(fork: &str, target: &str, target_source: Option<&str>) -> Record {
        Record {
            swap: Swap {
                fork_id: fork.into(),
                fork_name: "f".into(),
                target_id: target.into(),
                target_name: "main".into(),
                archive_name: "main-before".into(),
                macos: false,
                fork_bindings: serde_json::Value::Null,
                target_bindings: serde_json::Value::Null,
                fork_source: Some(target.into()),
                target_source: target_source.map(str::to_owned),
                checkpoint: crate::changes::keep::CheckpointRef {
                    id: "cp".into(),
                    name: "cp".into(),
                    created_at: String::new(),
                    scope: String::new(),
                },
                fork_was_running: false,
                target_was_running: false,
                makers: Default::default(),
            },
            state: State::Promoted,
            at: 1,
            steps: Vec::new(),
        }
    }

    #[test]
    fn forks_hang_under_their_parent_with_what_they_came_from() {
        let mut fixture = Fixture::new();
        fixture
            .links
            .insert("a".into(), link(Some("m"), Some("cp-fp")));
        fixture
            .links
            .insert("b".into(), link(Some("m"), Some("cp-step")));
        fixture
            .links
            .insert("c".into(), link(Some("a"), Some("cp-manual")));
        fixture
            .links
            .insert("d".into(), link(Some("m"), Some("recipe:ab12")));
        fixture.steps.insert("cp-step".into(), 4);
        fixture.requests.insert("a".into(), "fr-1".into());
        let nodes = fixture.build(&[
            computer("c", "c", &[]),
            computer("b", "b", &[]),
            computer(
                "m",
                "main",
                &[("cp-fp", "fork-point"), ("cp-manual", "manual")],
            ),
            computer("a", "a", &[]),
            computer("d", "d", &[]),
            computer("h", "silo-baseline-1", &[]),
            computer("e", "silo-export-0123abcd", &[]),
        ]);
        let ids: Vec<&str> = nodes.iter().map(|node| node.id.as_str()).collect();
        assert_eq!(
            ids,
            ["m", "a", "c", "b", "d"],
            "parents first, hidden left out"
        );
        assert_eq!(node(&nodes, "m").from, TreeFrom::Root);
        assert_eq!(node(&nodes, "a").from, TreeFrom::Main);
        assert_eq!(
            node(&nodes, "a").pending_changes.request_id.as_deref(),
            Some("fr-1")
        );
        assert_eq!(
            node(&nodes, "b").from,
            TreeFrom::Step {
                number: 4,
                checkpoint_id: "cp-step".into()
            }
        );
        assert_eq!(node(&nodes, "c").parent.as_deref(), Some("a"));
        assert_eq!(
            node(&nodes, "c").from,
            TreeFrom::Checkpoint {
                checkpoint_id: "cp-manual".into()
            }
        );
        assert_eq!(
            node(&nodes, "d").from,
            TreeFrom::Recipe {
                hash: "ab12".into()
            }
        );
    }

    #[test]
    fn a_recipe_fork_is_a_recipe_fork_even_though_its_build_parent_is_hidden() {
        let mut fixture = Fixture::new();
        fixture
            .links
            .insert("r".into(), link(Some("b"), Some("recipe:ab12cd")));
        let nodes = fixture.build(&[
            computer("b", "silo-build-linux-dev", &[("cp", "manual")]),
            computer("r", "fork-r", &[]),
        ]);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].parent, None);
        assert_eq!(
            nodes[0].from,
            TreeFrom::Recipe {
                hash: "ab12cd".into()
            }
        );
    }

    #[test]
    fn the_step_a_fork_came_from_survives_the_step_records_being_trimmed() {
        let mut fixture = Fixture::new();
        let mut recorded = link(Some("m"), Some("cp-step"));
        recorded.step = Some(7);
        fixture.links.insert("a".into(), recorded);
        // No step record names the checkpoint any more.
        let nodes = fixture.build(&[computer("m", "main", &[]), computer("a", "a", &[])]);
        assert_eq!(
            node(&nodes, "a").from,
            TreeFrom::Step {
                number: 7,
                checkpoint_id: "cp-step".into()
            }
        );
    }

    #[test]
    fn a_fork_not_yet_started_still_shows_its_origin() {
        let mut fixture = Fixture::new();
        fixture.links.insert("a".into(), link(Some("m"), Some("m")));
        let nodes = fixture.build(&[computer("m", "main", &[]), computer("a", "a", &[])]);
        assert_eq!(node(&nodes, "a").parent.as_deref(), Some("m"));
        assert_eq!(node(&nodes, "a").from, TreeFrom::Main);
    }

    #[test]
    fn after_a_promote_the_fork_is_main_and_the_old_mains_forks_follow_it() {
        let mut fixture = Fixture::new();
        // f and g were forked from m; f was promoted over m, which is archived.
        fixture
            .links
            .insert("f".into(), link(Some("m"), Some("cp1")));
        fixture
            .links
            .insert("g".into(), link(Some("m"), Some("cp2")));
        fixture
            .journal
            .save(&promoted_record("f", "m", None))
            .unwrap();
        let computers = [
            computer("f", "main", &[]),
            computer(
                "m",
                "main-before-1",
                &[("cp1", "fork-point"), ("cp2", "manual")],
            ),
            computer("g", "g", &[]),
        ];
        let nodes = fixture.build(&computers);
        assert_eq!(node(&nodes, "f").parent, None);
        assert_eq!(node(&nodes, "f").from, TreeFrom::Root);
        assert_eq!(node(&nodes, "m").parent, None);
        assert_eq!(node(&nodes, "g").parent.as_deref(), Some("f"));
        assert_eq!(
            node(&nodes, "g").from,
            TreeFrom::Checkpoint {
                checkpoint_id: "cp2".into()
            }
        );
        // An undo ends the record, and every answer goes back.
        let mut over = promoted_record("f", "m", None);
        over.state = State::Over;
        fixture.journal.save(&over).unwrap();
        let nodes = fixture.build(&computers);
        assert_eq!(node(&nodes, "f").parent.as_deref(), Some("m"));
        assert_eq!(node(&nodes, "f").from, TreeFrom::Main);
        assert_eq!(node(&nodes, "g").parent.as_deref(), Some("m"));
    }

    #[test]
    fn a_promoted_fork_takes_the_old_mains_own_parent() {
        let mut fixture = Fixture::new();
        fixture
            .links
            .insert("m".into(), link(Some("root"), Some("cp0")));
        fixture
            .links
            .insert("f".into(), link(Some("m"), Some("cp1")));
        fixture
            .journal
            .save(&promoted_record("f", "m", Some("root")))
            .unwrap();
        let nodes = fixture.build(&[
            computer("root", "root", &[]),
            computer("m", "main-before-1", &[]),
            computer("f", "main", &[]),
        ]);
        assert_eq!(node(&nodes, "f").parent.as_deref(), Some("root"));
        assert_eq!(node(&nodes, "f").from, TreeFrom::Main);
    }

    #[test]
    fn a_damaged_cycle_is_cut_instead_of_looping() {
        let mut fixture = Fixture::new();
        fixture.links.insert("a".into(), link(Some("b"), Some("b")));
        fixture.links.insert("b".into(), link(Some("a"), Some("a")));
        let nodes = fixture.build(&[computer("a", "a", &[]), computer("b", "b", &[])]);
        assert_eq!(nodes.len(), 2);
        assert!(nodes.iter().any(|node| node.parent.is_none()));
    }
}
