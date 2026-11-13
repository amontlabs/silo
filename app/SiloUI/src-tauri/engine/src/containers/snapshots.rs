//! `snapshot create/list/verify/remove` and `restore` for container computers, answered in
//! MicroSandbox's shapes so the engine's checkpoint, retention and quiet-fork records are shared
//! (see [`super::layers`] for how a checkpoint is stored).

use super::args::{Restore, SnapshotCreate};
use super::layers::{self, Snapshot, Store};
use super::layout::{ComputerRecord, Layout, SecretRecord};
use crate::runtime::{RuntimeError, RuntimePaths};
use serde_json::Value;

fn invalid(message: impl Into<String>) -> RuntimeError {
    RuntimeError::Invalid(message.into())
}

/// The entries of `snapshot list`: every checkpoint of `group`, or of all groups.
pub fn list(paths: &RuntimePaths, group: Option<&str>) -> Result<Vec<Value>, RuntimeError> {
    let layout = Layout::new(&paths.home);
    let snapshots = Store::new(&layout)
        .list(group)
        .map_err(RuntimeError::Unavailable)?;
    Ok(snapshots
        .iter()
        .map(|snapshot| snapshot.listed(layers::intact(&layout, snapshot)))
        .collect())
}

/// `snapshot list` of MicroSandbox's answer with the container checkpoints added.
pub fn merge_list(
    paths: &RuntimePaths,
    args: &[String],
    answer: crate::runtime::CommandOutput,
) -> Result<crate::runtime::CommandOutput, RuntimeError> {
    let mut rows: Vec<Value> = serde_json::from_str(&answer.stdout).map_err(|_| {
        RuntimeError::Malformed("The bundled runtime returned an invalid checkpoint list.".into())
    })?;
    let group = args
        .windows(2)
        .find(|pair| pair[0] == "--group")
        .map(|pair| pair[1].as_str());
    rows.extend(list(paths, group)?);
    Ok(crate::runtime::CommandOutput {
        stdout: Value::Array(rows).to_string(),
        stderr: answer.stderr,
    })
}

/// `snapshot head GROUP`: the newest checkpoint of the group, `null` when it has none.
pub fn head(paths: &RuntimePaths, group: &str) -> Result<Value, RuntimeError> {
    let snapshots = Store::new(&Layout::new(&paths.home))
        .list(Some(group))
        .map_err(RuntimeError::Unavailable)?;
    Ok(serde_json::json!({
        "group": group,
        "head": snapshots.last().map(|snapshot| snapshot.snapshot_id.clone()),
    }))
}

pub fn verify(paths: &RuntimePaths, group: &str, name: &str) -> Result<(), RuntimeError> {
    let layout = Layout::new(&paths.home);
    let snapshot = Store::new(&layout).read(group, name).map_err(invalid)?;
    if layers::intact(&layout, &snapshot) {
        Ok(())
    } else {
        Err(invalid(format!(
            "The checkpoint {group}:{name} is missing some of its files."
        )))
    }
}

/// Removes the checkpoint; the layers only it named go at the next collection.
pub fn remove(paths: &RuntimePaths, group: &str, name: &str) -> Result<(), RuntimeError> {
    let _store = layers::lock();
    Store::new(&Layout::new(&paths.home))
        .remove(group, name)
        .map_err(invalid)
}

/// Why a checkpoint could not take the computer: a start holds it, or another checkpoint does.
/// The mark does not say which, so the holder's own kind does: only a start is marked as starting.
fn held_message(name: &str) -> RuntimeError {
    if super::hold::is_starting(name) {
        RuntimeError::Unavailable(format!(
            "Container computer {name} is starting. Try again when it is up."
        ))
    } else {
        RuntimeError::Unavailable(format!(
            "A checkpoint of container computer {name} is already in progress. Try again when it is done."
        ))
    }
}

/// Labels of a fork: what the restore says, over what the checkpoint's computer had.
fn merged_labels(
    template: &[(String, String)],
    given: &[(String, String)],
) -> Vec<(String, String)> {
    let mut labels: Vec<(String, String)> = template
        .iter()
        .filter(|(key, _)| given.iter().all(|(other, _)| other != key))
        .cloned()
        .collect();
    labels.extend(given.iter().cloned());
    labels
}

/// The record of the computer a `restore` makes, from the checkpoint and what the caller gave.
pub fn fork_record(snapshot: &Snapshot, restore: &Restore) -> ComputerRecord {
    let mut record = layers::forked(snapshot, &restore.name);
    record.labels = merged_labels(&snapshot.template.labels, &restore.labels);
    record.env = restore.env.clone();
    record.github = restore
        .secrets
        .iter()
        .any(|secret| secret.name == super::args::GITHUB_SECRET_NAME);
    record.secrets = restore
        .secrets
        .iter()
        .filter(|secret| secret.name != super::args::GITHUB_SECRET_NAME)
        .map(|secret| SecretRecord {
            name: secret.name.clone(),
            allowed_hosts: secret.allowed_hosts.clone(),
        })
        .collect();
    record.secrets.sort_by(|a, b| a.name.cmp(&b.name));
    // Nothing unblocks a computer: a fork of a blocked one stays blocked whatever it is asked.
    record.network_blocked |= restore.network_blocked;
    if let Some(cpus) = restore.cpus {
        record.cpus = cpus;
        record.max_cpus = record.max_cpus.max(cpus);
    }
    if let Some(memory) = restore.memory_mib {
        record.memory_mib = memory;
        record.max_memory_mib = record.max_memory_mib.max(memory);
    }
    record
}

/// Makes the stopped computer a `restore` asks for. It starts afterwards, like any restore.
pub fn make_fork(paths: &RuntimePaths, restore: &Restore) -> Result<(), RuntimeError> {
    super::host_disk::require_room(&paths.home, "make a fork")?;
    let _store = layers::lock();
    let layout = Layout::new(&paths.home);
    let snapshot = Store::new(&layout)
        .read(&restore.group, &restore.member)
        .map_err(invalid)?;
    if !layers::intact(&layout, &snapshot) {
        return Err(invalid(format!(
            "The checkpoint {}:{} is missing some of its files. No computer was changed.",
            restore.group, restore.member
        )));
    }
    let dir = layout.computer(&restore.name);
    if dir.exists() {
        return Err(invalid(format!(
            "A container computer named {} already exists.",
            restore.name
        )));
    }
    std::fs::create_dir_all(dir.path())
        .map_err(|_| RuntimeError::Unavailable("The fork's folder could not be made.".into()))?;
    dir.write_record(&fork_record(&snapshot, restore))
        .map_err(RuntimeError::Unavailable)
}

/// The longest a running container's processes stay frozen for the copy of its writable layer.
/// Connections of the computer (and its clients') time out in the pause that follows, so a
/// longer copy is not worth it: the checkpoint fails and the computer carries on.
#[cfg(target_os = "linux")]
pub const FROZEN_LIMIT: std::time::Duration = std::time::Duration::from_secs(30);

/// What a checkpoint whose copy took longer than [`FROZEN_LIMIT`] says.
#[cfg(target_os = "linux")]
fn too_slow(name: &str) -> String {
    format!(
        "The checkpoint of {name} was not made: copying its files took longer than {} seconds, so the computer was resumed. Stop it and make the checkpoint again (a stopped computer's checkpoint takes no time), or free some space in it first.",
        FROZEN_LIMIT.as_secs()
    )
}

/// A checkpoint of a computer: seals a stopped computer's writable layer, or copies a running one's
/// while its processes are frozen.
///
/// The layer store's lock is held only to register the checkpoint, never for the copy: a copy of a
/// large layer would block every start, restore and fork. The copy is made first, into a layer no
/// record names yet, which is reserved so that collection leaves it alone until the checkpoint
/// names it. The computer's processes are frozen for the copy itself and for no longer than
/// [`FROZEN_LIMIT`].
#[cfg(target_os = "linux")]
pub fn create(paths: &RuntimePaths, request: &SnapshotCreate) -> Result<(), RuntimeError> {
    use super::{hold::StartMark, lifecycle, tools::Tools};
    super::require_host()?;
    let tools = Tools::locate(paths)?;
    let layout = Layout::new(&paths.home);
    let dir = layout.computer(&request.computer);
    let store = Store::new(&layout);
    // No start may begin while the layers are being sealed or copied. The computer is held, not
    // marked as starting: a running one stays running to a command or a secret change.
    let _mark =
        StartMark::hold(&request.computer).ok_or_else(|| held_message(&request.computer))?;
    super::host_disk::require_room(&paths.home, "make a checkpoint")?;
    let exists = || {
        invalid(format!(
            "The checkpoint {}:{} already exists.",
            request.group, request.name
        ))
    };
    if store.read(&request.group, &request.name).is_ok() {
        return Err(exists());
    }
    let record = dir.read_record().map_err(RuntimeError::Unavailable)?;
    let upper = dir.upper_of(&record);
    let unchanged = !upper.exists() || layers::is_empty_dir(&upper);
    let recreate = |name: &str| RuntimeError::Unavailable(super::idmap::recreate_message(name));
    // A running computer's layer is copied before the store is locked.
    let mut copied = None;
    if !unchanged {
        if record.upper.is_empty() {
            return Err(recreate(&request.computer));
        }
        if lifecycle::holder_alive(paths, &request.computer) {
            let copy = layers::new_id();
            let reservation = layers::reserve(&copy);
            let target = layout.layer(&copy);
            lifecycle::make_layer(&tools, &target).map_err(RuntimeError::Unavailable)?;
            let outcome = {
                let _frozen = lifecycle::freeze(paths, &request.computer)
                    .map_err(RuntimeError::Unavailable)?;
                lifecycle::copy_layer(&tools, &upper, &target, FROZEN_LIMIT)
            };
            if let Err(failure) = outcome {
                let _ = super::image::remove_tree(&tools, &target);
                return Err(RuntimeError::Unavailable(match failure {
                    lifecycle::CopyFailure::TooSlow => too_slow(&request.computer),
                    lifecycle::CopyFailure::Failed(detail) => detail,
                }));
            }
            copied = Some((copy, reservation));
        }
    }
    let _store = layers::lock();
    if store.read(&request.group, &request.name).is_ok() {
        return Err(exists());
    }
    let chain = if let Some((copy, _)) = &copied {
        let mut chain = record.layers.clone();
        chain.push(copy.clone());
        chain
    } else if unchanged {
        record.layers.clone()
    } else {
        let fresh = layers::new_id();
        lifecycle::make_layer(&tools, &layout.layer(&fresh)).map_err(RuntimeError::Unavailable)?;
        dir.update_record(|record| *record = layers::sealed(record, &fresh))
            .map_err(RuntimeError::Unavailable)?
            .layers
    };
    store
        .write(&Snapshot::new(&request.group, &request.name, chain, record))
        .map_err(RuntimeError::Unavailable)
}

#[cfg(not(target_os = "linux"))]
pub fn create(_: &RuntimePaths, _: &SnapshotCreate) -> Result<(), RuntimeError> {
    super::require_host()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::containers::args::{parse, Command, SecretSpec};

    fn strings(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn paths(home: &std::path::Path) -> RuntimePaths {
        RuntimePaths {
            guest_image: Default::default(),
            executable: "/nowhere/msb".into(),
            home: home.to_path_buf(),
            storage_home: None,
            library: Default::default(),
            metadata: Default::default(),
            volumes: Default::default(),
        }
    }

    fn template() -> ComputerRecord {
        let home = tempfile::tempdir().unwrap();
        crate::containers::capability::make_container(home.path(), "box");
        Layout::new(home.path())
            .computer("box")
            .read_record()
            .unwrap()
    }

    #[test]
    fn the_engines_restore_arguments_are_read() {
        let args = strings(&[
            "restore",
            "box:c1",
            "--name",
            "child",
            "--cpus",
            "2",
            "--memory",
            "2G",
            "--label",
            "silo.managed=true",
            "--label",
            "silo.restore-attempt=abc",
            "--env=A=1",
            "--env",
            "GH_TOKEN=$MSB_SILO_GITHUB",
            "--secret",
            "SILO_GITHUB:passthrough=*@github.com",
            "--secret",
            "TOKEN:passthrough=*@example.com,b.example.com",
            "--net-default-egress",
            "allow",
            "--net-default-ingress",
            "deny",
            "--disk-only",
        ]);
        let Ok(Command::Restore(restore)) = parse(&args) else {
            panic!("not a restore")
        };
        assert_eq!(
            (restore.group.as_str(), restore.member.as_str()),
            ("box", "c1")
        );
        assert_eq!(restore.name, "child");
        assert_eq!((restore.cpus, restore.memory_mib), (Some(2), Some(2048)));
        assert_eq!(
            restore.env,
            [
                ("A".into(), "1".into()),
                ("GH_TOKEN".into(), "$MSB_SILO_GITHUB".into())
            ]
        );
        assert_eq!(restore.secrets.len(), 2);
        assert_eq!(
            restore.secrets[1],
            SecretSpec {
                name: "TOKEN".into(),
                allowed_hosts: vec!["example.com".into(), "b.example.com".into()]
            }
        );
        assert_eq!(restore.labels.len(), 2);
        // The deny-all policy an import starts from asks for no network.
        let blocked = parse(&strings(&[
            "restore",
            "box:c1",
            "--name",
            "child",
            "--net-default-egress",
            "deny",
            "--net-default-ingress",
            "deny",
        ]))
        .unwrap();
        let Command::Restore(blocked) = blocked else {
            panic!("not a restore")
        };
        assert!(blocked.network_blocked);
        // Neither policy has rules: the runtime refuses them instead of dropping one.
        let refused = parse(&strings(&[
            "restore",
            "box:c1",
            "--name",
            "child",
            "--net-default-egress",
            "allow",
            "--net-rule",
            "allow@dns",
        ]))
        .unwrap_err();
        assert!(refused.contains("--net-rule"), "{refused}");
    }

    #[test]
    fn a_disk_restore_may_allow_missing_host_mounts_for_a_container() {
        let Command::Restore(restore) = parse(&strings(&[
            "restore",
            "box:c1",
            "--name",
            "child",
            "--cpus",
            "2",
            "--allow-missing-resources",
        ]))
        .unwrap() else {
            panic!("not a restore")
        };
        assert_eq!(restore.name, "child");
    }

    #[test]
    fn a_memory_restore_or_capture_is_refused_for_a_container() {
        let restore =
            parse(&strings(&["restore", "box:c1", "--name", "x", "--cow-mem"])).unwrap_err();
        assert!(
            restore.contains("cannot save the memory of a container computer"),
            "{restore}"
        );
        let create = parse(&strings(&[
            "snapshot",
            "create",
            "c1",
            "--from-sandbox",
            "box",
            "--group",
            "box",
            "--full",
        ]))
        .unwrap_err();
        assert!(
            create.contains("cannot save the memory of a container computer"),
            "{create}"
        );
        assert!(parse(&strings(&["restore", "box:c1"])).is_err());
        assert!(parse(&strings(&["restore", "box", "--name", "x"])).is_err());
    }

    #[test]
    fn the_snapshot_commands_the_engine_issues_are_read() {
        let create = parse(&strings(&[
            "snapshot",
            "create",
            "c1",
            "--from-sandbox",
            "box",
            "--group",
            "box",
            "--guest-flush",
            "required",
            "--integrity",
        ]))
        .unwrap();
        let Command::SnapshotCreate(create) = create else {
            panic!()
        };
        assert_eq!(
            (
                create.name.as_str(),
                create.computer.as_str(),
                create.group.as_str()
            ),
            ("c1", "box", "box")
        );
        assert_eq!(
            parse(&strings(&["snapshot", "verify", "box:c1"])).unwrap(),
            Command::SnapshotVerify {
                group: "box".into(),
                name: "c1".into()
            }
        );
        assert_eq!(
            parse(&strings(&[
                "snapshot", "remove", "--force", "box:c1", "--quiet"
            ]))
            .unwrap(),
            Command::SnapshotRemove {
                group: "box".into(),
                name: "c1".into()
            }
        );
        assert_eq!(
            parse(&strings(&["snapshot", "remove", "--quiet", "box:c1"])).unwrap(),
            Command::SnapshotRemove {
                group: "box".into(),
                name: "c1".into()
            }
        );
        assert_eq!(
            parse(&strings(&[
                "snapshot", "list", "--group", "box", "--format", "json"
            ]))
            .unwrap(),
            Command::SnapshotList {
                group: Some("box".into())
            }
        );
        assert_eq!(
            parse(&strings(&["snapshot", "head", "box", "--format", "json"])).unwrap(),
            Command::SnapshotHead {
                group: "box".into()
            }
        );
        assert_eq!(
            parse(&strings(&["snapshot", "save", "box:c1", "/out/file"])).unwrap(),
            Command::SnapshotSave(Box::new(super::super::args::SnapshotSave {
                group: "box".into(),
                name: "c1".into(),
                file: "/out/file".into(),
            }))
        );
        assert_eq!(
            parse(&strings(&[
                "snapshot",
                "load",
                "/in/file",
                "--group",
                "silo-import-ab",
                "--stage-id",
                "ab",
                "--runtime",
                "container"
            ]))
            .unwrap(),
            Command::SnapshotLoad(Box::new(super::super::args::SnapshotLoad {
                file: "/in/file".into(),
                group: "silo-import-ab".into(),
            }))
        );
        assert!(matches!(
            parse(&strings(&["snapshot", "bogus"])).unwrap(),
            Command::Unsupported { .. }
        ));
    }

    #[test]
    fn a_fork_takes_the_checkpoint_chain_the_callers_labels_and_secrets() {
        let mut template = template();
        template
            .labels
            .push(("silo.machine-id".into(), "parent-id".into()));
        template
            .labels
            .push(("silo.runtime".into(), "container".into()));
        let snapshot = Snapshot::new("box", "c1", vec!["a".into(), "b".into()], template);
        let restore = Restore {
            group: "box".into(),
            member: "c1".into(),
            name: "child".into(),
            cpus: Some(2),
            memory_mib: Some(2048),
            labels: vec![("silo.machine-id".into(), "child-id".into())],
            env: vec![("A".into(), "1".into())],
            secrets: vec![
                SecretSpec {
                    name: "SILO_GITHUB".into(),
                    allowed_hosts: vec!["github.com".into()],
                },
                SecretSpec {
                    name: "TOKEN".into(),
                    allowed_hosts: vec!["example.com".into()],
                },
            ],
            network_blocked: false,
        };
        let record = fork_record(&snapshot, &restore);
        assert_eq!(record.name, "child");
        assert_eq!(record.layers, ["a", "b"]);
        assert_ne!(record.upper, snapshot.template.upper);
        assert_eq!(record.label("silo.machine-id"), Some("child-id"));
        assert_eq!(record.label("silo.runtime"), Some("container"));
        assert_eq!(record.env, restore.env);
        assert!(record.github);
        assert_eq!(record.secrets.len(), 1);
        assert_eq!((record.cpus, record.memory_mib), (2, 2048));
        assert!(!record.network_blocked);
        // A fork of a blocked computer stays blocked, like its network policy.
        let record = fork_record(
            &snapshot,
            &Restore {
                network_blocked: true,
                ..restore.clone()
            },
        );
        assert!(record.network_blocked);
        // Nor does a restore that leaves the flag out unblock one.
        let mut blocked = snapshot.clone();
        blocked.template.network_blocked = true;
        assert!(fork_record(&blocked, &restore).network_blocked);
    }

    #[test]
    fn making_a_fork_writes_its_record_and_refuses_a_taken_name_or_a_lost_layer() {
        let home = tempfile::tempdir().unwrap();
        let layout = Layout::new(home.path());
        let paths = paths(home.path());
        let snapshot = Snapshot::new("box", "c1", vec!["a".into()], template());
        Store::new(&layout).write(&snapshot).unwrap();
        let restore = Restore {
            group: "box".into(),
            member: "c1".into(),
            name: "child".into(),
            ..Restore::default()
        };
        // The layer the checkpoint names is gone.
        assert!(make_fork(&paths, &restore)
            .unwrap_err()
            .to_string()
            .contains("missing some of its files"));
        std::fs::create_dir_all(layout.layer("a")).unwrap();
        make_fork(&paths, &restore).unwrap();
        let fork = layout.computer("child").read_record().unwrap();
        assert_eq!(fork.layers, ["a"]);
        assert!(make_fork(&paths, &restore)
            .unwrap_err()
            .to_string()
            .contains("already exists"));
        assert!(make_fork(
            &paths,
            &Restore {
                member: "nope".into(),
                ..restore
            }
        )
        .is_err());
    }

    #[test]
    fn verify_list_and_remove_follow_the_store() {
        let home = tempfile::tempdir().unwrap();
        let layout = Layout::new(home.path());
        let paths = paths(home.path());
        Store::new(&layout)
            .write(&Snapshot::new("box", "c1", vec!["a".into()], template()))
            .unwrap();
        assert!(verify(&paths, "box", "c1").is_err());
        assert_eq!(
            list(&paths, Some("box")).unwrap()[0]["availability"],
            "incomplete"
        );
        std::fs::create_dir_all(layout.layer("a")).unwrap();
        verify(&paths, "box", "c1").unwrap();
        assert_eq!(list(&paths, None).unwrap()[0]["availability"], "ready");
        remove(&paths, "box", "c1").unwrap();
        assert!(list(&paths, None).unwrap().is_empty());
        assert!(remove(&paths, "box", "c1").is_err());
    }

    #[test]
    fn a_second_checkpoint_says_one_is_in_progress_not_that_the_computer_is_starting() {
        // A start holds the computer: "is starting" is still the right answer.
        let starting = crate::containers::hold::StartMark::begin("held-by-a-start").unwrap();
        assert!(held_message("held-by-a-start")
            .to_string()
            .contains("is starting"));
        drop(starting);
        // Another checkpoint holds it: the message says the checkpoint, not a start.
        let checkpoint = crate::containers::hold::StartMark::hold("held-by-a-checkpoint").unwrap();
        let message = held_message("held-by-a-checkpoint").to_string();
        assert!(message.contains("already in progress"), "{message}");
        assert!(!message.contains("starting"), "{message}");
        drop(checkpoint);
        // Nobody holds it: a checkpoint may begin, and the message still names the checkpoint
        // (it is only asked when the mark could not be taken).
        assert!(crate::containers::hold::StartMark::hold("held-by-nobody").is_some());
    }
}
