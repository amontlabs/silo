//! The migration gate. Until the migration is `complete` or `not-required`, nothing
//! may name, run `msb` against, or open the previous runtime generation: a newer
//! `msb` upgrades the database it opens in place (the released one then refuses it),
//! and a live one can tear the conversion's copy. After "Continue" the previous
//! generation holds the only copy of unconverted computers.
use super::*;
use std::collections::BTreeMap;

const UNFINISHED: [&str; 3] = ["scanning", "running", "failed"];
const FINISHED: [&str; 2] = ["complete", "not-required"];
const COMPUTER_ID: &str = "fcfbc268-ae3f-40ff-8dfa-8af78911e52f";

fn app_data() -> tempfile::TempDir {
    // The runtime alias must keep Unix socket paths short, so use /tmp, not TMPDIR.
    tempfile::Builder::new()
        .prefix("sg")
        .tempdir_in(crate::test_support::live::temp_root())
        .unwrap()
}

/// A previous generation: one computer's settings, a database, and the backup journal.
fn previous(app_data: &Path) -> PathBuf {
    let old = app_data.join("runtime");
    fs::create_dir_all(old.join("microsandbox/db")).unwrap();
    fs::create_dir_all(old.join("volumes/dev")).unwrap();
    let computer: runtime::ComputerConfigurationRequest = serde_json::from_value(serde_json::json!({
        "schemaVersion": 1,
        "computers": [{"id":COMPUTER_ID,"name":"dev","cpus":1,"maxCPUs":1,"memoryGiB":1,"maxMemoryGiB":1,"workspaceStorageGiB":1,"runtimeStorageGiB":1}]
    }))
    .unwrap();
    runtime::write_metadata(&old.join("computers.json"), &computer).unwrap();
    fs::write(old.join("microsandbox/db/msb.db"), b"released database").unwrap();
    fs::write(old.join("microsandbox/db/msb.db-wal"), b"released wal").unwrap();
    fs::write(old.join("volumes/dev/workspace.raw"), b"computer").unwrap();
    old
}

/// Every file under `root` with its bytes.
fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for entry in fs::read_dir(&directory).unwrap() {
            let path = entry.unwrap().path();
            if fs::symlink_metadata(&path).unwrap().is_dir() {
                stack.push(path);
            } else {
                files.insert(
                    path.strip_prefix(root).unwrap().to_path_buf(),
                    fs::read(&path).unwrap_or_default(),
                );
            }
        }
    }
    files
}

/// The paths production derives for `storage`, with a fake `msb` that records the
/// `MSB_HOME` of every call it receives.
fn paths_in(app_data: &Path, storage: &Path) -> runtime::RuntimePaths {
    let msb = app_data.join("msb");
    crate::test_support::write_shell_script(
        &msb,
        "printf '%s\\n' \"$MSB_HOME\" >> \"$(dirname \"$0\")/homes\"\necho '[]'\n",
    );
    let library = app_data.join("libkrunfw");
    fs::write(&library, b"fixture").unwrap();
    let storage_home = storage.join("microsandbox");
    runtime::RuntimePaths {
        guest_image: app_data.join("guest-image"),
        executable: msb,
        home: runtime::runtime_home_alias(app_data, &storage_home),
        storage_home: Some(storage_home),
        library,
        metadata: storage.join("computers.json"),
        volumes: storage.join("volumes"),
    }
}

/// `runtime::runtime_paths` followed by a runtime command: no paths, no command,
/// when the gate refuses. Returns the `MSB_HOME` the command ran with.
fn run_runtime_command(app_data: &Path, writable: bool, status: &str) -> Result<PathBuf, String> {
    let storage = usable_storage(app_data, writable, status)?;
    let paths = paths_in(app_data, &storage);
    runtime::RuntimeRunner::run(
        &runtime::ProcessRunner,
        &paths,
        &["list".into()],
        Duration::from_secs(20),
    )
    .map_err(|error| error.to_string())?;
    Ok(paths.home)
}

fn recorded_homes(app_data: &Path) -> Vec<String> {
    fs::read_to_string(app_data.join("homes"))
        .map(|text| text.lines().map(str::to_owned).collect())
        .unwrap_or_default()
}

#[test]
fn nothing_names_the_previous_generation_until_the_migration_finishes() {
    let dir = app_data();
    previous(dir.path());
    for status in UNFINISHED {
        assert_eq!(
            usable_storage(dir.path(), true, status).unwrap_err(),
            "Finish the Silo runtime migration before using computers.",
            "{status}"
        );
    }
    // A migration record that needs repair never opens a runtime, whatever it says.
    for status in UNFINISHED.into_iter().chain(FINISHED) {
        assert!(usable_storage(dir.path(), false, status)
            .unwrap_err()
            .contains("manual repair"));
    }
    assert_eq!(
        usable_storage(dir.path(), true, "not-required").unwrap(),
        dir.path().join("runtime"),
        "an upgrade that needs no conversion keeps using its runtime"
    );
    let before = snapshot(dir.path());
    assert_eq!(
        usable_storage(dir.path(), true, "complete").unwrap_err(),
        MISSING_GENERATION
    );
    assert_eq!(snapshot(dir.path()), before);
}

#[test]
fn no_runtime_command_runs_against_the_previous_generation_while_unfinished() {
    let _test_state = crate::test_support::global_state();
    let dir = app_data();
    let old = previous(dir.path());
    let before = snapshot(&old);
    for status in UNFINISHED {
        let error = run_runtime_command(dir.path(), true, status).unwrap_err();
        assert!(
            error.contains("Finish the Silo runtime migration"),
            "{error}"
        );
    }
    assert!(
        recorded_homes(dir.path()).is_empty(),
        "no msb process may start while the migration is unfinished"
    );
    assert_eq!(
        snapshot(&old),
        before,
        "the previous generation is untouched"
    );
    assert!(
        !dir.path().join(".silo").exists() && !dir.path().join(".silo-dev").exists(),
        "no runtime alias was prepared"
    );

    // Once the migration is not required the same command runs, against that runtime.
    let home = run_runtime_command(dir.path(), true, "not-required").unwrap();
    assert_eq!(recorded_homes(dir.path()), [home.to_string_lossy()]);
    assert_eq!(
        fs::read_link(&home).unwrap(),
        old.join("microsandbox"),
        "without a conversion the previous folder is the live runtime"
    );
}

#[test]
fn continuing_into_a_fresh_runtime_never_names_the_previous_generation_again() {
    let _test_state = crate::test_support::global_state();
    let dir = app_data();
    let app_data = dir.path();
    let old = previous(app_data);
    fs::write(app_data.join("backup-operation.json"), b"old journal").unwrap();
    prepare_clean_generation(app_data).unwrap();
    select_generation(app_data, CLEAN).unwrap();
    quarantine_previous_backup_state(app_data, CLEAN).unwrap();
    let before = snapshot(&old);

    // In this process (restart pending) and in the next (complete), the previous
    // generation is never the runtime.
    for status in UNFINISHED {
        assert!(usable_storage(app_data, true, status).is_err(), "{status}");
    }
    for status in FINISHED {
        assert_eq!(
            usable_storage(app_data, true, status).unwrap(),
            app_data.join(CLEAN),
            "{status}"
        );
    }
    let home = run_runtime_command(app_data, true, "complete").unwrap();
    assert_eq!(recorded_homes(app_data), [home.to_string_lossy()]);
    assert_eq!(
        fs::read_link(&home).unwrap(),
        app_data.join(CLEAN).join("microsandbox")
    );
    assert_eq!(
        snapshot(&old),
        before,
        "the only copy of the unconverted computers stays exactly as it was"
    );
}

#[test]
fn a_converted_generation_is_used_only_after_the_migration_completes() {
    let dir = app_data();
    let app_data = dir.path();
    previous(app_data);
    fs::create_dir(app_data.join(CONVERTED)).unwrap();
    select_generation(app_data, CONVERTED).unwrap();
    // Committed but not restarted: this process still refuses.
    for status in UNFINISHED {
        assert!(usable_storage(app_data, true, status).is_err(), "{status}");
    }
    assert_eq!(
        usable_storage(app_data, true, "complete").unwrap(),
        app_data.join(CONVERTED)
    );
}

#[test]
fn work_that_must_settle_during_migration_cannot_start_msb() {
    let _test_state = crate::test_support::global_state();
    let dir = app_data();
    let old = previous(dir.path());
    let before = snapshot(&old);
    // An interrupted export or import settles before the migration (E-50): it reads the
    // saved files, but the paths it is given cannot start the runtime.
    let paths = runtime::without_runtime(paths_in(dir.path(), &old));
    let error = runtime::RuntimeRunner::run(
        &runtime::ProcessRunner,
        &paths,
        &["list".into()],
        Duration::from_secs(20),
    )
    .unwrap_err();
    assert!(
        matches!(error, runtime::RuntimeError::Unavailable(_)),
        "{error}"
    );
    assert!(recorded_homes(dir.path()).is_empty());
    assert_eq!(
        runtime::read_metadata(&paths.metadata)
            .unwrap()
            .computers
            .len(),
        1
    );
    assert_eq!(snapshot(&old), before);
    assert!(
        !paths.home.exists(),
        "an unstartable runtime does not even prepare its home"
    );
}

/// Records the storage every conversion command runs against.
struct Recording {
    storage: Mutex<Vec<(String, Option<PathBuf>, PathBuf)>>,
    old_runtime: PathBuf,
    adopted: Mutex<bool>,
}
impl runtime::RuntimeRunner for Recording {
    fn run(
        &self,
        paths: &runtime::RuntimePaths,
        args: &[String],
        _: Duration,
    ) -> Result<runtime::CommandOutput, runtime::RuntimeError> {
        self.storage.lock().unwrap().push((
            args[0].clone(),
            paths.storage_home.clone(),
            paths.metadata.clone(),
        ));
        if args[0] == "adopt-disk" {
            *self.adopted.lock().unwrap() = true;
        }
        let mount = if *self.adopted.lock().unwrap() {
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

#[test]
fn the_staged_conversion_still_runs_while_every_other_caller_is_refused() {
    let dir = app_data();
    let app_data = dir.path();
    let old = previous(app_data);
    let before = snapshot(&old);
    // The conversion runs while its own migration is `running`...
    assert!(usable_storage(app_data, true, "running").is_err());
    let staged = app_data.join(CONVERTED);
    let mut paths = paths_in(app_data, &staged);
    paths.metadata = staged.join("computers.json");
    let runner = Recording {
        storage: Mutex::new(Vec::new()),
        old_runtime: old.clone(),
        adopted: Mutex::new(false),
    };
    convert_with(&runner, app_data, &paths, &|_| Ok(())).unwrap();

    // ...and every command it ran named the staged runtime, never the previous one.
    let calls = runner.storage.lock().unwrap();
    assert_eq!(
        calls.iter().map(|call| call.0.as_str()).collect::<Vec<_>>(),
        ["inspect", "adopt-disk", "inspect"]
    );
    for (command, storage_home, metadata) in calls.iter() {
        assert_eq!(
            storage_home.as_deref(),
            Some(staged.join("microsandbox").as_path()),
            "{command}"
        );
        assert_eq!(metadata, &staged.join("computers.json"), "{command}");
    }
    assert_eq!(selected_runtime_storage(app_data).unwrap(), staged);
    assert_eq!(
        snapshot(&old),
        before,
        "converting never modifies the previous generation"
    );
}

/// Production source before its first column-zero `#[cfg(test)]`.
fn production_source(path: &Path) -> String {
    let text = fs::read_to_string(path).unwrap();
    match text.find("\n#[cfg(test)]") {
        Some(end) => text[..end].to_owned(),
        None => text,
    }
}

fn source_files(directory: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            source_files(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

#[test]
fn only_the_gate_can_name_a_runtime_and_only_the_migration_names_the_previous_one() {
    let source = crate::test_support::manifest_root().join("src");
    let mut files = Vec::new();
    source_files(&source, &mut files);
    assert!(files.len() > 50, "the scan must see the whole source tree");
    for file in files {
        let relative = file.strip_prefix(&source).unwrap().to_string_lossy();
        // Test fixtures build disposable layouts; they never reach a real runtime.
        if relative.contains("test") || relative.contains("guard_tests") {
            continue;
        }
        let text = production_source(&file);
        if relative != "runtime.rs" {
            assert!(
                !text.contains("RuntimePaths {"),
                "{relative} builds RuntimePaths; use runtime::runtime_paths, which the migration gate guards"
            );
        }
        if relative != "runtime_migration.rs" && !relative.starts_with("runtime_migration/") {
            assert!(
                !text.contains("join(\"runtime\")")
                    && !text.contains("runtime/computers.json")
                    && !text.contains("runtime/machines.json"),
                "{relative} names the previous runtime generation; only the migration may"
            );
            assert!(
                !text.contains("machines.json") && !text.contains("sandbox-activity.json"),
                "{relative} names an inventory file of the earlier vocabulary; only the migration may"
            );
        }
        if !matches!(
            relative.as_ref(),
            "runtime.rs" | "runtime_migration.rs" | "commands/runtime_migration.rs"
        ) {
            assert!(
                !text.contains("migration_runtime_paths")
                    && !text.contains("selected_runtime_storage"),
                "{relative} bypasses the migration gate"
            );
        }
        // Paths that cannot start `msb` are for settling saved export/import state only.
        if !matches!(
            relative.as_ref(),
            "runtime.rs" | "backup_controller.rs" | "backup_controller/recovery.rs"
        ) {
            assert!(
                !text.contains("inert_runtime_paths"),
                "{relative} uses runtime paths meant only for interrupted export/import recovery"
            );
        }
    }
}
