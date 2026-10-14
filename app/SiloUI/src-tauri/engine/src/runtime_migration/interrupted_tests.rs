//! An export or import the previous Silo left unfinished must not hold the storage
//! migration back, and must not leave its data behind in the converted storage.
//!
//! The first launch of the upgrade settles it without the runtime and without writing to
//! the previous generation, a pre-upgrade backup. What only the runtime can remove stays
//! owned by the journal, which waits for the upgrade; the migration copies it with
//! everything else and converts only the computers saved in the settings. The launch after
//! the migration runs the ordinary recovery against the converted generation, which
//! removes the copy and reports the result. The previous generation never changes.
use super::*;
use crate::backup_controller::{FirstLaunch, JournalState};
use std::collections::BTreeMap;

const COMPUTER_ID: &str = "fcfbc268-ae3f-40ff-8dfa-8af78911e52f";
const IMPORT_ID: &str = "0f6d5c1a-7a63-4b0a-9a36-4a6f1d3f6e11";
const GROUP: &str = "silo-import-0123456789abcdef0123456789abcdef";
const SUFFIX: &str = "0123456789abcdef0123456789abcdef";
const MEMBER: &str = "silo-backup-0-1-2";

fn one_computer(name: &str, id: &str) -> runtime::ComputerConfigurationRequest {
    serde_json::from_value(serde_json::json!({
        "schemaVersion": 1,
        "computers": [{"id":id,"name":name,"cpus":1,"maxCPUs":1,"memoryGiB":1,"maxMemoryGiB":1,"workspaceStorageGiB":1,"runtimeStorageGiB":1}]
    }))
    .unwrap()
}

/// What the previous Silo's interrupted operation left in the previous generation.
#[derive(Clone, Copy, Default)]
struct Leftovers {
    /// The disk folder a released import claimed for `copy`, and the runtime's computer
    /// record created over it, in this state. A released import leaves it `Created`.
    released_import: Option<&'static str>,
    /// The native snapshot load stages of the import group.
    load_stages: bool,
    /// The members of an export capture in the runtime's snapshot index.
    capture: bool,
}

/// The runtime's database of the fixture: the computer records and snapshot members that
/// `msb list` and `msb snapshot list` report. A file in `microsandbox/db`, so a copy of
/// the generation holds a copy of the database.
fn database(storage_home: &Path) -> PathBuf {
    storage_home.join("db/fake-msb.json")
}

/// A previous generation with one saved computer and `leftovers`, plus the staged paths
/// the converter would use.
fn previous_generation(leftovers: Leftovers) -> (tempfile::TempDir, runtime::RuntimePaths) {
    // The runtime alias must keep Unix socket paths short, so use /tmp, not TMPDIR.
    let dir = tempfile::Builder::new()
        .prefix("si")
        .tempdir_in(crate::test_support::live::temp_root())
        .unwrap();
    let app_data = dir.path();
    let old = app_data.join("runtime");
    fs::create_dir_all(old.join("volumes/dev")).unwrap();
    runtime::write_metadata(
        &old.join("computers.json"),
        &one_computer("dev", COMPUTER_ID),
    )
    .unwrap();
    fs::write(old.join("volumes/dev/workspace.raw"), b"computer").unwrap();
    fs::create_dir_all(old.join("microsandbox/db")).unwrap();
    fs::write(old.join("microsandbox/db/msb.db"), b"released database").unwrap();
    let mut computers = vec![serde_json::json!(["dev", COMPUTER_ID, "Stopped"])];
    let mut snapshots = Vec::new();
    if let Some(status) = leftovers.released_import {
        fs::create_dir_all(old.join("volumes/copy")).unwrap();
        fs::write(old.join("volumes/copy/.silo-restore-owner"), IMPORT_ID).unwrap();
        fs::write(old.join("volumes/copy/workspace.raw"), b"partial disk").unwrap();
        computers.push(serde_json::json!(["copy", IMPORT_ID, status]));
    }
    if leftovers.load_stages {
        for stage in [
            format!("microsandbox/snapshots/.msb-snapshot-load-{SUFFIX}"),
            format!("microsandbox/cache/tmp/snapshot-load-{SUFFIX}"),
        ] {
            fs::create_dir_all(old.join(&stage).join("partial")).unwrap();
            fs::write(old.join(&stage).join("partial/data"), b"half a load").unwrap();
        }
    }
    if leftovers.capture {
        snapshots.push(serde_json::json!(["dev", MEMBER]));
        snapshots.push(serde_json::json!(["other", "silo-backup-0-3-4"]));
    }
    fs::write(
        database(&old.join("microsandbox")),
        serde_json::json!({"computers": computers, "snapshots": snapshots}).to_string(),
    )
    .unwrap();
    let storage = app_data.join(CONVERTED);
    let storage_home = storage.join("microsandbox");
    let paths = runtime::RuntimePaths {
        guest_image: app_data.join("guest-image"),
        executable: app_data.join("msb"),
        home: runtime::runtime_home_alias(app_data, &storage_home),
        storage_home: Some(storage_home),
        library: app_data.join("libkrunfw"),
        metadata: storage.join("computers.json"),
        volumes: storage.join("volumes"),
    };
    (dir, paths)
}

/// The paths recovery gets while the migration blocks the runtime: the previous
/// generation's files and no way to start `msb`.
fn inert_paths(app_data: &Path) -> runtime::RuntimePaths {
    let old = app_data.join("runtime");
    let storage_home = old.join("microsandbox");
    runtime::without_runtime(runtime::RuntimePaths {
        guest_image: app_data.join("guest-image"),
        executable: app_data.join("msb"),
        home: runtime::runtime_home_alias(app_data, &storage_home),
        storage_home: Some(storage_home),
        library: app_data.join("libkrunfw"),
        metadata: old.join("computers.json"),
        volumes: old.join("volumes"),
    })
}

/// The alias through which the runtime reaches the previous generation. Nothing may
/// create it before the migration finished, and nothing after names it either.
fn previous_alias(app_data: &Path) -> PathBuf {
    runtime::runtime_home_alias(app_data, &app_data.join("runtime/microsandbox"))
}

/// Every entry under `root` with its bytes, link target or a directory marker.
fn tree(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut entries = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for entry in fs::read_dir(&directory).unwrap() {
            let path = entry.unwrap().path();
            let metadata = fs::symlink_metadata(&path).unwrap();
            let value = if metadata.file_type().is_symlink() {
                fs::read_link(&path)
                    .unwrap()
                    .into_os_string()
                    .into_encoded_bytes()
            } else if metadata.is_dir() {
                stack.push(path.clone());
                b"<directory>".to_vec()
            } else {
                fs::read(&path).unwrap()
            };
            entries.insert(path.strip_prefix(root).unwrap().to_path_buf(), value);
        }
    }
    entries
}

/// The staged runtime: every computer is stopped, and `adopt-disk` makes its disk owned.
struct StagedRuntime {
    old_runtime: PathBuf,
    calls: Mutex<Vec<String>>,
}
impl runtime::RuntimeRunner for StagedRuntime {
    fn run(
        &self,
        _: &runtime::RuntimePaths,
        args: &[String],
        _: Duration,
    ) -> Result<runtime::CommandOutput, runtime::RuntimeError> {
        let mut calls = self.calls.lock().unwrap();
        calls.push(args.join(" "));
        let mount = if calls.iter().any(|call| call.starts_with("adopt-disk")) {
            serde_json::json!({"guest":"/workspace","type":"Owned"})
        } else {
            serde_json::json!({"guest":"/workspace","type":"DiskImage","host":self.old_runtime.join("volumes/dev/workspace.raw")})
        };
        Ok(runtime::CommandOutput {
            stdout: serde_json::json!({"name":"dev","status":"Stopped","config":{"labels":{"silo.machine-id":COMPUTER_ID},"mounts":[mount]}}).to_string(),
            stderr: String::new(),
        })
    }
}

/// The runtime the launch after the migration uses: its own database file inside the
/// storage it is given, which it refuses to be anything but the converted generation.
struct ConvertedRuntime {
    converted: PathBuf,
    calls: Mutex<Vec<String>>,
    fail_remove: Mutex<bool>,
}
impl ConvertedRuntime {
    fn new(app_data: &Path) -> Self {
        Self {
            converted: app_data.join(CONVERTED),
            calls: Mutex::new(Vec::new()),
            fail_remove: Mutex::new(false),
        }
    }
    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}
impl runtime::RuntimeRunner for ConvertedRuntime {
    fn run(
        &self,
        paths: &runtime::RuntimePaths,
        args: &[String],
        _: Duration,
    ) -> Result<runtime::CommandOutput, runtime::RuntimeError> {
        let storage_home = paths.storage_home.as_deref().unwrap();
        assert_eq!(
            storage_home,
            self.converted.join("microsandbox"),
            "the runtime must never run against the previous generation: {args:?}"
        );
        self.calls.lock().unwrap().push(args.join(" "));
        let file = database(storage_home);
        let mut db: serde_json::Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
        let stdout = match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
            ["list", "--format", "json"] => db["computers"]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| serde_json::json!({"name": row[0]}))
                .collect::<Vec<_>>()
                .into(),
            ["inspect", name, "--format", "json"] => {
                let row = db["computers"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|row| row[0] == *name)
                    .expect("only a computer that is listed is inspected");
                serde_json::json!({"name": name, "status": row[2], "config": {"labels": {"silo.managed": "true", "silo.machine-id": row[1]}}})
            }
            ["remove", "--force", "--quiet", name] => {
                let row = db["computers"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|row| row[0] == *name)
                    .expect("only a computer that is listed is removed");
                // The bundled runtime removes a Created computer like a Stopped one.
                if *self.fail_remove.lock().unwrap() {
                    return Err(runtime::RuntimeError::Invalid(
                        "test removal refused".into(),
                    ));
                }
                db["computers"]
                    .as_array_mut()
                    .unwrap()
                    .retain(|row| row[0] != *name);
                fs::write(&file, db.to_string()).unwrap();
                serde_json::Value::Null
            }
            ["snapshot", "list", "--format", "json"] => db["snapshots"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
                .map(|(index, row)| serde_json::json!({"snapshot_id": format!("snap_{index:032x}"), "group": row[0], "name": row[1]}))
                .collect::<Vec<_>>()
                .into(),
            ["snapshot", "head", _, "--format", "json"] => serde_json::json!({"head": null}),
            ["snapshot", "remove", selector, "--quiet"] => {
                db["snapshots"]
                    .as_array_mut()
                    .unwrap()
                    .retain(|row| format!("{}:{}", row[0].as_str().unwrap(), row[1].as_str().unwrap()) != *selector);
                fs::write(&file, db.to_string()).unwrap();
                serde_json::Value::Null
            }
            other => panic!("unexpected runtime command: {other:?}"),
        };
        Ok(runtime::CommandOutput {
            stdout: if stdout.is_null() {
                String::new()
            } else {
                stdout.to_string()
            },
            stderr: String::new(),
        })
    }
}

/// The journal a Silo build writes while an operation is unfinished.
fn pending_journal(app_data: &Path, request: serde_json::Value, cancelled: bool) {
    let archive = app_data.join("exports/dev.silo-backup");
    let journal = serde_json::json!({
        "version": 1,
        "id": "5b0c8e3e-3b8e-4c4c-9a0b-1f0f5f2d2b77",
        "archive": {
            "name": "dev.silo-backup",
            "archivePath": archive,
            "completedLabel": "In progress",
            "size": "Unknown",
            "destination": archive.parent().unwrap(),
            "computers": ["dev"],
        },
        "request": request,
        "cancelled": cancelled,
        "terminal": null,
    });
    fs::write(app_data.join("backup-operation.json"), journal.to_string()).unwrap();
}

fn journal_file(app_data: &Path) -> serde_json::Value {
    serde_json::from_slice(&fs::read(app_data.join("backup-operation.json")).unwrap()).unwrap()
}

/// Every call the backup service's `msb` received, with the runtime home it ran with.
fn script_calls(scripts: &Path) -> Vec<String> {
    fs::read_to_string(scripts.join("calls"))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

struct Shape {
    name: &'static str,
    request: serde_json::Value,
    cancelled: bool,
    leftovers: Leftovers,
    outcome: &'static str,
    title: &'static str,
    detail: String,
}

fn shapes() -> Vec<Shape> {
    let capture = serde_json::json!({"computerId": COMPUTER_ID, "group": "dev", "member": MEMBER});
    let released = Leftovers {
        released_import: Some("Created"),
        ..Leftovers::default()
    };
    let stopped = Leftovers {
        released_import: Some("Stopped"),
        ..Leftovers::default()
    };
    let stages = Leftovers {
        load_stages: true,
        ..Leftovers::default()
    };
    let capturing = Leftovers {
        capture: true,
        ..Leftovers::default()
    };
    vec![
        Shape {
            name: "import by the released 0.9.0 that wrote its disk",
            request: serde_json::json!({"kind":"restore","name":"copy","source":"dev","id":IMPORT_ID}),
            cancelled: false,
            leftovers: released,
            outcome: "failed",
            title: "Import interrupted",
            detail: "No computer was added. Import the file again.".into(),
        },
        Shape {
            name: "cancelled import by the released 0.9.0 that wrote its disk",
            request: serde_json::json!({"kind":"restore","name":"copy","source":"dev","id":IMPORT_ID}),
            cancelled: true,
            leftovers: released,
            outcome: "cancelled",
            title: "Import cancelled",
            detail: "No computer was added.".into(),
        },
        Shape {
            name: "import by a released Silo whose computer the runtime can remove",
            request: serde_json::json!({"kind":"restore","name":"copy","source":"dev","id":IMPORT_ID}),
            cancelled: false,
            leftovers: stopped,
            outcome: "failed",
            title: "Import interrupted",
            detail: "No computer was added. Import the file again.".into(),
        },
        Shape {
            name: "import of a development build that loaded a snapshot group",
            request: serde_json::json!({"kind":"restore","name":"copy","source":"dev","group":GROUP}),
            cancelled: false,
            leftovers: stages,
            outcome: "failed",
            title: "Import interrupted",
            detail: "No computer was added. Import the file again.".into(),
        },
        Shape {
            name: "import of a development build that journaled its identity and group",
            request: serde_json::json!({"kind":"restore","name":"copy","source":"dev","id":IMPORT_ID,"group":GROUP}),
            cancelled: false,
            leftovers: stages,
            outcome: "failed",
            title: "Import interrupted",
            detail: "No computer was added. Import the file again.".into(),
        },
        Shape {
            name: "cancelled import of a development build that journaled its identity and group",
            request: serde_json::json!({"kind":"restore","name":"copy","source":"dev","id":IMPORT_ID,"group":GROUP}),
            cancelled: true,
            leftovers: stages,
            outcome: "cancelled",
            title: "Import cancelled",
            detail: "No computer was added.".into(),
        },
        Shape {
            name: "export of a development build in the middle of a capture",
            request: serde_json::json!({"kind":"backup","names":["dev"],"computers":[],"running":[],"pending_capture":capture}),
            cancelled: false,
            leftovers: capturing,
            outcome: "failed",
            title: "Export interrupted",
            detail: "No export file was saved. Export the computer again.".into(),
        },
        Shape {
            name: "cancelled export of a development build in the middle of a capture",
            request: serde_json::json!({"kind":"backup","names":["dev"],"computers":[],"running":[],"pending_capture":capture}),
            cancelled: true,
            leftovers: capturing,
            outcome: "cancelled",
            title: "Export cancelled",
            detail: "No export file was saved.".into(),
        },
    ]
}

#[test]
fn what_only_the_runtime_can_remove_is_cleaned_from_the_converted_generation_after_the_migration() {
    let _test_state = crate::test_support::global_state();
    for shape in shapes() {
        let state = shape.name;
        let (dir, paths) = previous_generation(shape.leftovers);
        let app_data = dir.path();
        let old = app_data.join("runtime");
        pending_journal(app_data, shape.request.clone(), shape.cancelled);
        let runner = StagedRuntime {
            old_runtime: old.clone(),
            calls: Mutex::new(Vec::new()),
        };
        // The safety net: a journal nothing settled yet still refuses the conversion.
        let refusal = convert_with(&runner, app_data, &paths, &|_| Ok(())).unwrap_err();
        assert!(refusal.contains("interrupted backup"), "{state}: {refusal}");
        assert!(runner.calls.lock().unwrap().is_empty(), "{state}");
        let before = tree(&old);

        // First launch of the upgrade: everything that needs no runtime is settled; the
        // rest is not given up, and nothing is written to the previous generation.
        let launch = crate::backup_controller::settle_journal_before_migration(
            app_data,
            &inert_paths(app_data),
        );
        assert!(matches!(launch, FirstLaunch::AwaitingUpgrade), "{state}");
        assert_eq!(
            tree(&old),
            before,
            "{state}: settling left the backup as it was"
        );
        assert!(!previous_alias(app_data).exists(), "{state}");
        let waiting = journal_file(app_data);
        assert!(waiting["terminal"].is_null(), "{state}");
        assert_eq!(waiting["awaitingUpgrade"], true, "{state}");
        // Everything the journal recorded is still there, so recovery owns the cleanup.
        for (field, value) in shape.request.as_object().unwrap() {
            assert_eq!(
                waiting["request"][field.as_str()],
                *value,
                "{state}: {field} is kept"
            );
        }
        assert!(matches!(
            crate::backup_controller::journal_state(app_data),
            JournalState::Settled
        ));

        // The migration proceeds and converts only the computer saved in the settings.
        convert_with(&runner, app_data, &paths, &|_| Ok(())).unwrap();
        assert_eq!(
            *runner.calls.lock().unwrap(),
            [
                "inspect dev --format json",
                "adopt-disk dev",
                "inspect dev --format json"
            ],
            "{state}"
        );
        let converted = app_data.join(CONVERTED);
        let metadata = runtime::read_metadata(&converted.join("computers.json")).unwrap();
        assert_eq!(metadata.computers.len(), 1, "{state}");
        assert_eq!(
            selected_runtime_storage(app_data).unwrap(),
            converted,
            "{state}"
        );
        assert_eq!(tree(&old), before, "{state}: the backup is an exact copy");
        // The journal is not quarantined: the launch that follows finishes it.
        assert_eq!(journal_file(app_data), waiting, "{state}");
        assert!(!old
            .join("before-checkpoints-backup-operation.json")
            .exists());
        // The converted generation holds a copy of everything the operation left.
        if shape.leftovers.released_import.is_some() {
            assert!(
                converted.join("volumes/copy/workspace.raw").exists(),
                "{state}"
            );
        }
        if shape.leftovers.load_stages {
            assert!(
                converted
                    .join(format!(
                        "microsandbox/snapshots/.msb-snapshot-load-{SUFFIX}/partial/data"
                    ))
                    .exists(),
                "{state}"
            );
        }

        // Launch after the migration: the ordinary recovery, against the converted
        // generation. The previous process crashed or quit in between, so nothing but the
        // journal and the files carries over.
        let runtime = ConvertedRuntime::new(app_data);
        let scripts = app_data.join("scripts");
        let result = crate::backup_controller::recover_journal_after_migration(
            app_data, &scripts, &paths, &runtime, GROUP,
        )
        .expect("the journal is still there")
        .unwrap();
        assert_eq!(result["outcome"], shape.outcome, "{state}");
        assert_eq!(result["title"], shape.title, "{state}");
        assert_eq!(result["detail"], shape.detail, "{state}");
        let reported = journal_file(app_data);
        assert_eq!(reported["terminal"]["title"], shape.title, "{state}");
        assert!(reported.get("awaitingUpgrade").is_none(), "{state}");
        // The user could not have seen it: it was produced across the upgrade.
        assert_eq!(reported["unseen"], true, "{state}");
        assert!(matches!(
            crate::backup_controller::journal_state(app_data),
            JournalState::Settled
        ));

        // What the operation left is gone from the converted generation...
        let db: serde_json::Value =
            serde_json::from_slice(&fs::read(database(&converted.join("microsandbox"))).unwrap())
                .unwrap();
        let computers: Vec<_> = db["computers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row[0].as_str().unwrap().to_owned())
            .collect();
        if shape.leftovers.released_import.is_some() {
            // The orphan's disk and its computer record are gone, whether the computer never
            // started (a released import leaves it Created) or had stopped.
            assert!(!converted.join("volumes/copy").exists(), "{state}");
            assert_eq!(computers, ["dev"], "{state}: the orphan computer record");
            assert_eq!(
                runtime.calls(),
                [
                    "list --format json",
                    "inspect copy --format json",
                    "remove --force --quiet copy",
                ],
                "{state}"
            );
        } else {
            assert_eq!(computers, ["dev"], "{state}");
        }
        if shape.leftovers.load_stages {
            for stage in [
                format!("microsandbox/snapshots/.msb-snapshot-load-{SUFFIX}"),
                format!("microsandbox/cache/tmp/snapshot-load-{SUFFIX}"),
            ] {
                assert!(!converted.join(&stage).exists(), "{state}: {stage}");
            }
            // The load's snapshot group was removed child first, root last, and every
            // command ran against the converted generation's runtime home.
            let calls = script_calls(&scripts);
            assert_eq!(
                calls,
                [
                    format!(
                        "{home} snapshot list --format json",
                        home = paths.home.display()
                    ),
                    format!(
                        "{home} snapshot head {GROUP}:imported-parent",
                        home = paths.home.display()
                    ),
                    format!(
                        "{home} snapshot remove --quiet {GROUP}:imported-member",
                        home = paths.home.display()
                    ),
                    format!(
                        "{home} snapshot remove --quiet {GROUP}:imported-parent",
                        home = paths.home.display()
                    ),
                ],
                "{state}"
            );
        }
        if shape.leftovers.capture {
            assert_eq!(
                db["snapshots"],
                serde_json::json!([["other", "silo-backup-0-3-4"]]),
                "{state}: only the journaled capture member was removed"
            );
        }
        // ...and the previous generation is still byte-identical, never run against.
        assert_eq!(
            tree(&old),
            before,
            "{state}: the pre-upgrade backup is an exact copy"
        );
        assert!(!previous_alias(app_data).exists(), "{state}");
        assert_eq!(
            fs::read_link(&paths.home).unwrap(),
            converted.join("microsandbox"),
            "{state}: the runtime home is the converted generation"
        );
    }
}

#[test]
fn only_what_the_journal_owns_is_cleaned_from_the_converted_generation() {
    let _test_state = crate::test_support::global_state();
    // A development build's import loaded a group. An earlier released import also left a
    // disk folder and a computer record named `copy`: they are not this journal's.
    let (dir, paths) = previous_generation(Leftovers {
        released_import: Some("Created"),
        load_stages: true,
        capture: false,
    });
    let app_data = dir.path();
    let old = app_data.join("runtime");
    pending_journal(
        app_data,
        serde_json::json!({"kind":"restore","name":"copy","source":"dev","id":IMPORT_ID,"group":GROUP}),
        false,
    );
    let runner = StagedRuntime {
        old_runtime: old.clone(),
        calls: Mutex::new(Vec::new()),
    };
    let before = tree(&old);
    assert!(matches!(
        crate::backup_controller::settle_journal_before_migration(app_data, &inert_paths(app_data)),
        FirstLaunch::AwaitingUpgrade
    ));
    convert_with(&runner, app_data, &paths, &|_| Ok(())).unwrap();
    let runtime = ConvertedRuntime::new(app_data);
    crate::backup_controller::recover_journal_after_migration(
        app_data,
        &app_data.join("scripts"),
        &paths,
        &runtime,
        GROUP,
    )
    .unwrap()
    .unwrap();
    let converted = app_data.join(CONVERTED);
    assert!(converted.join("volumes/copy/workspace.raw").exists());
    assert!(runtime.calls().is_empty(), "no computer record was touched");
    assert!(!converted
        .join(format!(
            "microsandbox/snapshots/.msb-snapshot-load-{SUFFIX}"
        ))
        .exists());
    assert_eq!(tree(&old), before);
}

#[test]
fn a_failed_cleanup_after_the_migration_keeps_the_journal_and_breaks_nothing() {
    let _test_state = crate::test_support::global_state();
    let (dir, paths) = previous_generation(Leftovers {
        released_import: Some("Stopped"),
        ..Leftovers::default()
    });
    let app_data = dir.path();
    let old = app_data.join("runtime");
    pending_journal(
        app_data,
        serde_json::json!({"kind":"restore","name":"copy","source":"dev","id":IMPORT_ID}),
        false,
    );
    let runner = StagedRuntime {
        old_runtime: old.clone(),
        calls: Mutex::new(Vec::new()),
    };
    let before = tree(&old);
    assert!(matches!(
        crate::backup_controller::settle_journal_before_migration(app_data, &inert_paths(app_data)),
        FirstLaunch::AwaitingUpgrade
    ));
    convert_with(&runner, app_data, &paths, &|_| Ok(())).unwrap();
    // The conversion verified every computer before it selected the generation.
    let mut state = fresh("running", 1);
    state.migrated_count = 1;
    write(&app_data.join(FILE), &state).unwrap();
    let waiting = journal_file(app_data);

    // The cleanup fails: the removal of the orphan computer is refused.
    let runtime = ConvertedRuntime::new(app_data);
    *runtime.fail_remove.lock().unwrap() = true;
    let scripts = app_data.join("scripts");
    let error = crate::backup_controller::recover_journal_after_migration(
        app_data, &scripts, &paths, &runtime, GROUP,
    )
    .unwrap()
    .unwrap_err();
    assert!(error.contains("test removal refused"), "{error}");
    // The journal is kept as it was, still owning the cleanup, and nothing was removed.
    assert_eq!(journal_file(app_data), waiting);
    assert!(app_data
        .join(CONVERTED)
        .join("volumes/copy/workspace.raw")
        .exists());
    assert_eq!(tree(&old), before);

    // The migration is complete and stays so: the failure is the recovery's alone, and a
    // relaunch neither quarantines the journal nor reopens the migration.
    for _launch in 0..2 {
        let state = initial(&app_data.join(FILE), app_data).unwrap();
        assert_eq!(state.status, "complete");
        assert_eq!(state.stage, "Migration complete");
        assert_eq!(journal_file(app_data), waiting);
        assert!(!old
            .join("before-checkpoints-backup-operation.json")
            .exists());
    }
    assert!(previous_generation_is_backup(app_data));

    // The next launch finishes the cleanup.
    *runtime.fail_remove.lock().unwrap() = false;
    let result = crate::backup_controller::recover_journal_after_migration(
        app_data, &scripts, &paths, &runtime, GROUP,
    )
    .unwrap()
    .unwrap();
    assert_eq!(result["title"], "Import interrupted");
    assert!(!app_data.join(CONVERTED).join("volumes/copy").exists());
    assert_eq!(tree(&old), before);
}

#[test]
fn an_unreadable_journal_is_set_aside_and_the_migration_proceeds() {
    let _test_state = crate::test_support::global_state();
    let journal_with_version = |version: u64| {
        serde_json::json!({
            "version": version,
            "id": "5b0c8e3e-3b8e-4c4c-9a0b-1f0f5f2d2b77",
            "archive": {"name": "dev.silo-backup", "archivePath": "/exports/dev.silo-backup", "completedLabel": "In progress", "size": "Unknown", "destination": "/exports", "computers": ["dev"]},
            "request": {"kind": "backup", "names": ["dev"]},
            "cancelled": false,
            "terminal": null,
        })
        .to_string()
        .into_bytes()
    };
    let cases: [(&str, Vec<u8>); 4] = [
        ("damaged", b"{\"version\":1,\"id\":".to_vec()),
        ("empty", Vec::new()),
        ("not a journal", b"[1, 2, 3]".to_vec()),
        ("unsupported version", journal_with_version(2)),
    ];
    for (state, bytes) in cases {
        let (dir, paths) = previous_generation(Leftovers::default());
        let app_data = dir.path();
        let old = app_data.join("runtime");
        fs::write(app_data.join("backup-operation.json"), &bytes).unwrap();
        let runner = StagedRuntime {
            old_runtime: old.clone(),
            calls: Mutex::new(Vec::new()),
        };
        let before = tree(&old);

        // A refused migration changes nothing, not even this file: the refusals come
        // before it is set aside, whether they precede or follow the journal's check.
        let redirected = app_data.join("elsewhere");
        fs::create_dir(&redirected).unwrap();
        std::os::unix::fs::symlink(&redirected, app_data.join(CONVERTED)).unwrap();
        let refusal = convert_with(&runner, app_data, &paths, &|_| Ok(())).unwrap_err();
        assert!(refusal.contains("redirected"), "{state}: {refusal}");
        assert_eq!(
            fs::read(app_data.join("backup-operation.json")).unwrap(),
            bytes,
            "{state}"
        );
        assert_eq!(fs::read_dir(&redirected).unwrap().count(), 0, "{state}");
        fs::remove_file(app_data.join(CONVERTED)).unwrap();
        assert_eq!(tree(&old), before, "{state}");

        // Otherwise it never holds the migration back.
        convert_with(&runner, app_data, &paths, &|_| Ok(())).unwrap();
        assert_eq!(
            selected_runtime_storage(app_data).unwrap(),
            app_data.join(CONVERTED),
            "{state}"
        );
        let aside: Vec<_> = fs::read_dir(app_data)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with("backup-operation.unreadable-"))
            .collect();
        assert_eq!(aside.len(), 1, "{state}: {aside:?}");
        assert!(
            aside[0].starts_with("backup-operation.unreadable-")
                && aside[0].ends_with(".json")
                && aside[0].len() == "backup-operation.unreadable-YYYY-MM-DD.json".len(),
            "{state}: {}",
            aside[0]
        );
        // Kept as it was, for diagnosis, beside the other files in app data.
        assert_eq!(
            fs::read(app_data.join(&aside[0])).unwrap(),
            bytes,
            "{state}"
        );
        // The export and import page shows a result in its place after the upgrade, and
        // it stays there (the migration quarantines no journal that holds a result).
        let notice = journal_file(app_data);
        assert_eq!(
            notice["terminal"]["title"], "Export or import record set aside",
            "{state}"
        );
        assert_eq!(notice["terminal"]["outcome"], "failed", "{state}");
        assert_eq!(notice["unseen"], true, "{state}");
        assert!(!old
            .join("before-checkpoints-backup-operation.json")
            .exists());
        // Whatever the unknown operation did to the previous generation is copied like any
        // other content, and the previous generation itself is untouched.
        assert_eq!(tree(&old), before, "{state}");
        assert!(matches!(
            crate::backup_controller::journal_state(app_data),
            JournalState::Settled
        ));
    }
}

#[cfg(unix)]
#[test]
fn a_journal_that_cannot_be_read_refuses_the_migration_and_is_never_set_aside() {
    use std::os::unix::fs::PermissionsExt;
    let _test_state = crate::test_support::global_state();
    let (dir, paths) = previous_generation(Leftovers::default());
    let app_data = dir.path();
    let old = app_data.join("runtime");
    let journal = app_data.join("backup-operation.json");
    // Content that is set aside when it can be read: only the failure to read it differs.
    let bytes = b"{\"version\":1,\"id\":".to_vec();
    fs::write(&journal, &bytes).unwrap();
    fs::set_permissions(&journal, fs::Permissions::from_mode(0o000)).unwrap();
    // The owner can read it anyway (running as root): nothing to check.
    if fs::read(&journal).is_ok() {
        fs::set_permissions(&journal, fs::Permissions::from_mode(0o600)).unwrap();
        return;
    }
    let runner = StagedRuntime {
        old_runtime: old.clone(),
        calls: Mutex::new(Vec::new()),
    };
    let before = tree(&old);
    let aside = || {
        fs::read_dir(app_data)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with("backup-operation.unreadable-"))
            .count()
    };

    // Nothing is known about it, so the migration neither waits for it nor sets it aside:
    // it refuses, changes nothing, and the next launch tries again.
    assert!(matches!(
        crate::backup_controller::journal_state(app_data),
        JournalState::Unavailable
    ));
    let refusal = convert_with(&runner, app_data, &paths, &|_| Ok(())).unwrap_err();
    assert!(
        refusal.contains("Relaunch Silo to try again") && refusal.contains("No data was changed"),
        "{refusal}"
    );
    assert!(runner.calls.lock().unwrap().is_empty());
    assert!(!app_data.join(CONVERTED).exists());
    assert_eq!(aside(), 0);
    assert_eq!(tree(&old), before);
    fs::set_permissions(&journal, fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(
        fs::read(&journal).unwrap(),
        bytes,
        "the file was not touched"
    );

    // Once it can be read, it is a damaged record like any other: set aside, and the
    // migration proceeds.
    convert_with(&runner, app_data, &paths, &|_| Ok(())).unwrap();
    assert_eq!(aside(), 1);
    assert_eq!(
        journal_file(app_data)["terminal"]["title"],
        "Export or import record set aside"
    );
}

#[test]
fn a_folder_in_place_of_the_journal_refuses_the_migration_and_is_never_set_aside() {
    let _test_state = crate::test_support::global_state();
    let (dir, paths) = previous_generation(Leftovers::default());
    let app_data = dir.path();
    let old = app_data.join("runtime");
    let journal = app_data.join("backup-operation.json");
    fs::create_dir(&journal).unwrap();
    let runner = StagedRuntime {
        old_runtime: old.clone(),
        calls: Mutex::new(Vec::new()),
    };
    let refusal = convert_with(&runner, app_data, &paths, &|_| Ok(())).unwrap_err();
    assert!(refusal.contains("Relaunch Silo to try again"), "{refusal}");
    assert!(journal.is_dir());
    assert!(fs::read_dir(app_data).unwrap().all(|entry| !entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with("backup-operation.unreadable-")));
    assert!(!app_data.join(CONVERTED).exists());
}

#[test]
fn a_journal_with_a_result_stays_for_the_export_page_but_pending_progress_is_isolated() {
    let dir = tempfile::tempdir().unwrap();
    let app_data = dir.path();
    let old = app_data.join("runtime");
    fs::create_dir(&old).unwrap();
    let journal = app_data.join("backup-operation.json");
    let moved = old.join("before-checkpoints-backup-operation.json");
    for selected in [CONVERTED, CLEAN] {
        fs::write(&journal, br#"{"terminal":{"outcome":"failed"}}"#).unwrap();
        quarantine_previous_backup_state(app_data, selected).unwrap();
        assert!(
            journal.exists(),
            "a result nothing can resume is not hidden from the user"
        );
        assert!(!moved.exists());

        fs::write(&journal, br#"{"terminal":null}"#).unwrap();
        quarantine_previous_backup_state(app_data, selected).unwrap();
        assert!(!journal.exists());
        assert_eq!(fs::read(&moved).unwrap(), br#"{"terminal":null}"#);
        fs::remove_file(&moved).unwrap();

        // An unreadable file is not known to be finished, so it is isolated too.
        fs::write(&journal, b"{not json").unwrap();
        quarantine_previous_backup_state(app_data, selected).unwrap();
        assert!(!journal.exists());
        fs::remove_file(&moved).unwrap();
    }
}

#[test]
fn a_journal_waiting_for_the_upgrade_stays_only_in_the_converted_generation() {
    let dir = tempfile::tempdir().unwrap();
    let app_data = dir.path();
    let old = app_data.join("runtime");
    fs::create_dir(&old).unwrap();
    let journal = app_data.join("backup-operation.json");
    let moved = old.join("before-checkpoints-backup-operation.json");
    let waiting = br#"{"terminal":null,"awaitingUpgrade":true}"#;

    // The converted generation holds a copy of what it left, and removes it.
    fs::write(&journal, waiting).unwrap();
    quarantine_previous_backup_state(app_data, CONVERTED).unwrap();
    assert_eq!(fs::read(&journal).unwrap(), waiting);
    assert!(!moved.exists());

    // The clean generation (Continue) holds none of it, and the previous generation holds
    // the computers that were not converted: nothing is ever cleaned there, so the journal
    // is isolated like any unfinished one.
    quarantine_previous_backup_state(app_data, CLEAN).unwrap();
    assert!(!journal.exists());
    assert_eq!(fs::read(&moved).unwrap(), waiting);
}

#[test]
fn an_operation_that_left_nothing_for_the_runtime_is_settled_and_reported_before_the_upgrade() {
    let _test_state = crate::test_support::global_state();
    let cases = [
        (
            "export of the released 0.9.0",
            serde_json::json!({"kind":"backup","names":["dev"],"computers":[],"running":[]}),
            false,
            "failed",
            "Export interrupted before the upgrade",
            "No export file was saved. Export the computer again.",
        ),
        (
            "import of the released 0.9.0 before it chose a computer identity",
            serde_json::json!({"kind":"restore","name":"copy","source":"dev","id":null}),
            false,
            "failed",
            "Import interrupted before the upgrade",
            "No computer was added. Import the file again.",
        ),
        (
            "cancelled import before it wrote anything",
            serde_json::json!({"kind":"restore","name":"copy","source":"dev"}),
            true,
            "cancelled",
            "Import cancelled",
            "No computer was added.",
        ),
    ];
    for (state, request, cancelled, outcome, title, detail) in cases {
        let (dir, paths) = previous_generation(Leftovers::default());
        let app_data = dir.path();
        let old = app_data.join("runtime");
        pending_journal(app_data, request, cancelled);
        let runner = StagedRuntime {
            old_runtime: old.clone(),
            calls: Mutex::new(Vec::new()),
        };
        let before = tree(&old);
        let FirstLaunch::Result(result) = crate::backup_controller::settle_journal_before_migration(
            app_data,
            &inert_paths(app_data),
        ) else {
            panic!("{state}: nothing is left for the runtime");
        };
        assert_eq!(result["outcome"], outcome, "{state}");
        assert_eq!(result["title"], title, "{state}");
        assert_eq!(result["detail"], detail, "{state}");
        assert_eq!(tree(&old), before, "{state}");

        // The result is recorded, the migration proceeds, and the result stays for the
        // export and import page.
        convert_with(&runner, app_data, &paths, &|_| Ok(())).unwrap();
        let journal = journal_file(app_data);
        assert_eq!(journal["terminal"]["title"], title, "{state}");
        assert!(journal.get("awaitingUpgrade").is_none(), "{state}");
        // Recorded before the upgrade, so the user has not seen it yet.
        assert_eq!(journal["unseen"], true, "{state}");
        assert!(!old
            .join("before-checkpoints-backup-operation.json")
            .exists());
        assert_eq!(tree(&old), before, "{state}");
        // Nothing is left to recover after the upgrade: the result is final.
        let runtime = ConvertedRuntime::new(app_data);
        assert!(crate::backup_controller::recover_journal_after_migration(
            app_data,
            &app_data.join("scripts"),
            &paths,
            &runtime,
            GROUP
        )
        .is_none());
        assert!(runtime.calls().is_empty(), "{state}");
    }
}
