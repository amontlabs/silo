//! The sweep of empty import-group folders: one `snapshot list` for the whole pass, a cap per
//! pass, and a folder that still lists a member stays.
use super::*;

/// A runtime that lists the given members and counts every call.
struct Listing {
    members: serde_json::Value,
    calls: Mutex<Vec<Vec<String>>>,
}

impl RuntimeRunner for Listing {
    fn run(
        &self,
        _paths: &RuntimePaths,
        args: &[String],
        _timeout: Duration,
    ) -> Result<CommandOutput, RuntimeError> {
        self.calls.lock().unwrap().push(args.to_vec());
        match args.first().map(String::as_str) {
            Some("snapshot") => Ok(CommandOutput {
                stdout: self.members.to_string(),
                stderr: String::new(),
            }),
            _ => panic!("the sweep only lists snapshots: {args:?}"),
        }
    }
}

fn group_name(index: usize) -> String {
    format!("silo-import-{index:032x}")
}

fn empty_shell(paths: &RuntimePaths, group: &str) {
    let directory = paths.home.join("snapshots").join(group);
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join(".group.lock"), "").unwrap();
    fs::write(directory.join("group.json"), "{}").unwrap();
}

fn remaining(paths: &RuntimePaths) -> usize {
    fs::read_dir(paths.home.join("snapshots")).unwrap().count()
}

fn listing(members: serde_json::Value) -> Listing {
    Listing {
        members,
        calls: Mutex::new(Vec::new()),
    }
}

#[test]
fn one_inventory_decides_every_empty_folder() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = crate::test_support::paths(directory.path());
    for index in 0..5 {
        empty_shell(&paths, &group_name(index));
    }
    let runner = listing(serde_json::json!([]));
    remove_unowned_import_group_shells(&runner, &paths);
    assert_eq!(remaining(&paths), 0);
    assert_eq!(runner.calls.lock().unwrap().len(), 1);
}

#[test]
fn the_cap_leaves_the_rest_for_the_next_pass() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = crate::test_support::paths(directory.path());
    let total = SHELLS_PER_PASS + 3;
    for index in 0..total {
        empty_shell(&paths, &group_name(index));
    }
    let runner = listing(serde_json::json!([]));
    remove_unowned_import_group_shells(&runner, &paths);
    assert_eq!(remaining(&paths), 3);
    assert_eq!(runner.calls.lock().unwrap().len(), 1);
    remove_unowned_import_group_shells(&runner, &paths);
    assert_eq!(remaining(&paths), 0);
}

#[test]
fn a_folder_with_a_member_or_another_file_is_never_removed() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = crate::test_support::paths(directory.path());
    let (listed, foreign, empty) = (group_name(1), group_name(2), group_name(3));
    for group in [&listed, &foreign, &empty] {
        empty_shell(&paths, group);
    }
    fs::write(
        paths.home.join("snapshots").join(&foreign).join("other"),
        "",
    )
    .unwrap();
    let runner = listing(serde_json::json!([{
        "group": listed,
        "snapshot_id": "snap-1",
        "name": "silo-backup-0-330418-1790360984903",
        "scope": "disk", "availability": "ready"
    }]));
    remove_unowned_import_group_shells(&runner, &paths);
    assert!(paths.home.join("snapshots").join(&listed).exists());
    assert!(paths.home.join("snapshots").join(&foreign).exists());
    assert!(!paths.home.join("snapshots").join(&empty).exists());
}
