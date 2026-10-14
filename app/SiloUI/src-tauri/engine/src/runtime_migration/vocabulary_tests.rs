//! The vocabulary migration on disposable folders: nothing here touches a real home.
use super::vocabulary::{self, Locations};
use super::*;
use serde_json::{json, Value};
use std::{cell::Cell, collections::BTreeMap, io};

pub(crate) const ID: &str = "fcfbc268-ae3f-40ff-8dfa-8af78911e52f";
pub(crate) const OTHER: &str = "60e2e26f-c248-4f24-9035-c766d8168fa8";
const SSH: &str = "0b7f0d6c-6a53-4b0d-8f7a-0d2f4f3c0a11";
pub(crate) const DEVICE: &str = "9d1c7e1a-2f0b-4a53-8c52-5d7ea2c5a001";

struct Fixture {
    dir: tempfile::TempDir,
    locations: Locations,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let locations = Locations {
            config: dir.path().join("config"),
            app_data: dir.path().join("data"),
            state: dir.path().join("home/.silo"),
        };
        fs::create_dir_all(&locations.config).unwrap();
        fs::create_dir_all(&locations.app_data).unwrap();
        fs::create_dir_all(locations.state.join("desktop-remote")).unwrap();
        Self { dir, locations }
    }

    fn data(&self, relative: &str) -> PathBuf {
        self.locations.app_data.join(relative)
    }

    fn run(&self) -> Result<(), String> {
        vocabulary::run_in(&self.locations, &|_| Ok(()))
    }

    fn select(&self, directory: &str, status: &str) {
        put(
            &self.data("runtime-generation.json"),
            &json!({"version": 1, "directory": directory}),
        );
        record_state(&self.locations.app_data, status);
    }
}

fn record_state(app_data: &Path, status: &str) {
    put(
        &app_data.join("runtime-migration.json"),
        &json!({
            "version": 1, "status": status, "stage": "x", "logs": [],
            "migratedCount": 1, "failedCount": 0, "totalCount": 1, "canContinue": false
        }),
    );
}

fn put(path: &Path, value: &Value) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

fn get(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

/// Every file under `root`, by relative path.
fn snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(root: &Path, folder: &Path, found: &mut BTreeMap<String, Vec<u8>>) {
        let Ok(entries) = fs::read_dir(folder) else {
            return;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, found);
            } else {
                let name = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                found.insert(name, fs::read(&path).unwrap());
            }
        }
    }
    let mut found = BTreeMap::new();
    walk(root, root, &mut found);
    found
}

fn old_computer(id: &str, name: &str, desktop: bool) -> Value {
    let mut entry = json!({
        "kind": "vm", "id": id, "name": name, "cpus": 2, "maxCPUs": 4, "memoryGiB": 4,
        "maxMemoryGiB": 8, "workspaceStorageGiB": 20, "runtimeStorageGiB": 10
    });
    if desktop {
        entry["desktop"] = json!({"startWithSandbox": false, "builtIn": true});
    }
    entry
}

fn new_computer(id: &str, name: &str, desktop: bool) -> Value {
    let mut entry = old_computer(id, name, false);
    entry.as_object_mut().unwrap().remove("kind");
    if desktop {
        entry["desktop"] = json!({"startWithComputer": false, "builtIn": true});
    }
    entry
}

fn ssh_entry() -> Value {
    json!({"kind": "ssh", "id": SSH, "name": "legacy", "host": "h", "user": "u", "port": 22})
}

fn old_event(action: &str) -> Value {
    json!({
        "id": "e1", "action": action, "workspace": "dev", "machineId": ID, "timestamp": 5,
        "completed": true, "failure": null, "process": 1
    })
}

fn new_event(action: &str) -> Value {
    json!({
        "id": "e1", "action": action, "computer": "dev", "computerId": ID, "timestamp": 5,
        "completed": true, "failure": null, "process": 1
    })
}

/// The files of one storage generation as an earlier build wrote them.
fn write_old_storage(storage: &Path) {
    put(
        &storage.join("machines.json"),
        &json!({
            "schemaVersion": 1,
            "machines": [old_computer(ID, "dev", true), ssh_entry(), old_computer(OTHER, "fork", false)]
        }),
    );
    put(
        &storage.join("sandbox-activity.json"),
        &json!([old_event("start")]),
    );
    put(
        &storage.join("setup-activity.json"),
        &json!([
            {"schemaVersion": 1, "type": "progress", "requestId": "r", "phase": "workspaces",
             "step": "workspace-image-import", "workspace": "dev", "message": "m", "safeForDisplay": true,
             "timestamp": 1, "level": "info", "elapsedSeconds": 0},
            {"schemaVersion": 1, "type": "progress", "requestId": "r", "phase": "workspaces",
             "step": "image-resolving", "workspace": "dev", "message": "m", "safeForDisplay": true,
             "timestamp": 2, "level": "info", "elapsedSeconds": 0},
            {"schemaVersion": 1, "type": "progress", "requestId": "r", "phase": "workspaces",
             "step": "workspace-settings", "workspace": "dev", "message": "m", "safeForDisplay": true,
             "timestamp": 3, "level": "info", "elapsedSeconds": 0}
        ]),
    );
    put(
        &storage.join(format!("checkpoints/{ID}.json")),
        &json!({
            "version": 1, "checkpoints": [], "snapshotGroup": "g",
            "pendingCheckpointRestore": {
                "checkpointId": "c", "sourceWorkspace": "dev", "state": "full"
            },
            "checkpointOperation": null
        }),
    );
    put(
        &storage.join(format!("checkpoints/{OTHER}.json")),
        &json!({"version": 1, "checkpoints": [], "pendingCheckpointRestore": null,
                "checkpointOperation": null}),
    );
    put(
        &storage.join("lifecycle-operations/abc.json"),
        &json!({
            "version": 1, "machine_id": ID, "name": "dev", "action": "stop",
            "phase": "stop-pending", "event": old_event("stop")
        }),
    );
    put(
        &storage.join("update-resume.json"),
        &json!({"version": 1, "machines": [{"id": ID, "name": "dev"}]}),
    );
    put(
        &storage.join("configuration-operation.json"),
        &json!({
            "version": 1,
            "previous": {"schemaVersion": 1, "machines": [old_computer(ID, "dev", false), ssh_entry()]},
            "request": {"schemaVersion": 1, "machines": [old_computer(ID, "dev", true)]}
        }),
    );
    put(
        &storage.join("network.json"),
        &json!({"mappings": [
            {"workspace": "dev", "port": 80, "hostPort": 8080, "scheme": null, "enabled": true}
        ]}),
    );
    put(
        &storage.join("ssh-access.json"),
        &json!([{"workspace": "dev", "machineId": ID, "enabled": true, "port": 2222,
                 "bindAddress": "127.0.0.1", "keys": []}]),
    );
    put(
        &storage.join("microsandbox/repository-push-operations.json"),
        &json!({"op1": {"session": "s", "updated": 1, "dismissed": false,
                        "operation": {"operationId": "op1", "workspace": "dev",
                                      "repositoryPath": "/workspace/r", "status": "pushing"}}}),
    );
    // Files that keep their names and content.
    put(
        &storage.join("network-policy.json"),
        &json!({"workspaces": "kept"}),
    );
    put(
        &storage.join("computer-use/x.json"),
        &json!({"approval": "ask"}),
    );
    put(
        &storage.join("storage-maintenance/x.json"),
        &json!({"history": []}),
    );
}

fn assert_new_storage(storage: &Path) {
    assert_eq!(
        get(&storage.join("computers.json")),
        json!({
            "schemaVersion": 1,
            "computers": [new_computer(ID, "dev", true), new_computer(OTHER, "fork", false)]
        })
    );
    assert!(!storage.join("machines.json").exists());
    assert_eq!(
        get(&storage.join("computer-activity.json")),
        json!([new_event("start")])
    );
    assert!(!storage.join("sandbox-activity.json").exists());
    assert_eq!(
        get(&storage.join("setup-activity.json")),
        json!([
            {"schemaVersion": 1, "type": "progress", "requestId": "r", "phase": "computers",
             "step": "computer-image-import", "computer": "dev", "message": "m", "safeForDisplay": true,
             "timestamp": 1, "level": "info", "elapsedSeconds": 0},
            {"schemaVersion": 1, "type": "progress", "requestId": "r", "phase": "computers",
             "step": "image-resolving", "computer": "dev", "message": "m", "safeForDisplay": true,
             "timestamp": 2, "level": "info", "elapsedSeconds": 0},
            {"schemaVersion": 1, "type": "progress", "requestId": "r", "phase": "computers",
             "step": "computer-settings", "computer": "dev", "message": "m", "safeForDisplay": true,
             "timestamp": 3, "level": "info", "elapsedSeconds": 0}
        ])
    );
    assert_eq!(
        get(&storage.join(format!("checkpoints/{ID}.json"))),
        json!({
            "version": 1, "checkpoints": [], "snapshotGroup": "g",
            "pendingCheckpointRestore": {
                "checkpointId": "c", "sourceComputer": "dev", "state": "full"
            },
            "checkpointOperation": null
        })
    );
    assert_eq!(
        get(&storage.join("lifecycle-operations/abc.json")),
        json!({
            "version": 1, "computer_id": ID, "name": "dev", "action": "stop",
            "phase": "stop-pending", "event": new_event("stop")
        })
    );
    assert_eq!(
        get(&storage.join("update-resume.json")),
        json!({"version": 1, "computers": [{"id": ID, "name": "dev"}]})
    );
    assert_eq!(
        get(&storage.join("configuration-operation.json")),
        json!({
            "version": 1,
            "previous": {"schemaVersion": 1, "computers": [new_computer(ID, "dev", false)]},
            "request": {"schemaVersion": 1, "computers": [new_computer(ID, "dev", true)]}
        })
    );
    assert_eq!(
        get(&storage.join("network.json")),
        json!({"mappings": [
            {"computer": "dev", "port": 80, "hostPort": 8080, "scheme": null, "enabled": true}
        ]})
    );
    assert_eq!(
        get(&storage.join("ssh-access.json")),
        json!([{"computer": "dev", "computerId": ID, "enabled": true, "port": 2222,
                "bindAddress": "127.0.0.1", "keys": []}])
    );
    assert_eq!(
        get(&storage.join("microsandbox/repository-push-operations.json")),
        json!({"op1": {"session": "s", "updated": 1, "dismissed": false,
                       "operation": {"operationId": "op1", "computer": "dev",
                                     "repositoryPath": "/workspace/r", "status": "pushing"}}})
    );
    assert_eq!(
        get(&storage.join("network-policy.json")),
        json!({"workspaces": "kept"})
    );
    assert_eq!(
        get(&storage.join("computer-use/x.json")),
        json!({"approval": "ask"})
    );
}

fn old_settings() -> Value {
    json!({
        "schemaVersion": 1,
        "settings": {
            "theme": "dark", "startWorkspacesAtLaunch": true,
            "startupWorkspaceIds": [ID], "sandboxOrder": [format!("local:{ID}"), format!("remote:{DEVICE}:{OTHER}")],
            "futureSetting": {"kept": true}
        },
        "onboardingDraft": {
            "currentStep": "workspaces",
            "machines": [old_computer(ID, "dev", true), ssh_entry()],
            "unfinishedMachineEditor": {"draft": old_computer(OTHER, "fork", false), "insertAt": 1, "originalID": OTHER},
            "workspaceSelections": {ID: [{"repository": "a/b", "allowPushes": true}], SSH: []},
            "workspaceIdentities": {ID: {"name": "n", "email": "e", "apply": true}, SSH: {"name": "", "email": "", "apply": false}},
            "workspaceRepositoryAccess": {ID: {"repositoryMode": "all", "allRepositoriesAllowChanges": false}, SSH: {"repositoryMode": "selected", "allRepositoriesAllowChanges": false}}
        }
    })
}

fn new_settings() -> Value {
    json!({
        "schemaVersion": 1,
        "settings": {
            "theme": "dark", "startComputersAtLaunch": true,
            "startupComputerIds": [ID], "computerOrder": [format!("local:{ID}"), format!("remote:{DEVICE}:{OTHER}")],
            "futureSetting": {"kept": true}
        },
        "onboardingDraft": {
            "currentStep": "computers",
            "computers": [new_computer(ID, "dev", true)],
            "unfinishedComputerEditor": {"draft": new_computer(OTHER, "fork", false), "insertAt": 1, "originalID": OTHER},
            "computerSelections": {ID: [{"repository": "a/b", "allowPushes": true}]},
            "computerIdentities": {ID: {"name": "n", "email": "e", "apply": true}},
            "computerRepositoryAccess": {ID: {"repositoryMode": "all", "allRepositoriesAllowChanges": false}}
        }
    })
}

fn old_github() -> Value {
    json!({
        "revision": 4, "accessEnabled": true, "account": "me",
        "workspaces": [{"workspace": "dev", "identity": {"name": "n", "email": "e", "apply": true},
                        "repositories": [], "repositoryMode": "all", "allRepositoriesAllowChanges": false}],
        "operations": [{"workspace": "dev", "status": "failed", "message": "m", "canRetry": true}],
        "policyStamps": {"dev": {"revision": 3, "base": null}},
        "identityErrors": {"dev": "x"},
        "futureField": [1, 2]
    })
}

fn new_github() -> Value {
    json!({
        "revision": 4, "accessEnabled": true, "account": "me",
        "computers": [{"computer": "dev", "identity": {"name": "n", "email": "e", "apply": true},
                       "repositories": [], "repositoryMode": "all", "allRepositoriesAllowChanges": false}],
        "operations": [{"computer": "dev", "status": "failed", "message": "m", "canRetry": true}],
        "policyStamps": {"dev": {"revision": 3, "base": null}},
        "identityErrors": {"dev": "x"},
        "futureField": [1, 2]
    })
}

fn old_secrets() -> Value {
    json!({
        "pendingRevocations": [{"secretId": "s", "generation": "g", "name": "TOKEN", "workspace": "dev"}],
        "secrets": [{"id": "s", "name": "TOKEN", "valueId": "v", "workspaces": ["dev"],
                     "allowedDomains": ["example.com"], "affected": ["dev"],
                     "pendingWorkspaces": ["dev"], "errors": {"dev": "e"}, "removing": false}],
        "activities": [{"title": "kept"}]
    })
}

fn new_secrets() -> Value {
    json!({
        "pendingRevocations": [{"secretId": "s", "generation": "g", "name": "TOKEN", "computer": "dev"}],
        "secrets": [{"id": "s", "name": "TOKEN", "valueId": "v", "computers": ["dev"],
                     "allowedDomains": ["example.com"], "affected": ["dev"],
                     "pendingComputers": ["dev"], "errors": {"dev": "e"}, "removing": false}],
        "activities": [{"title": "kept"}]
    })
}

fn old_backup_operation() -> Value {
    json!({
        "version": 1, "id": ID,
        "archive": {"name": "a", "archivePath": "/a", "completedLabel": "l", "size": "1",
                    "destination": "/", "sandboxes": ["dev"]},
        "request": {"kind": "backup", "names": ["dev"], "machines": [["dev", ID]], "running": [],
                    "pending_capture": {"workspaceId": ID, "group": "g", "member": "silo-backup-1-2-3"}},
        "cancelled": false, "terminal": null
    })
}

fn new_backup_operation() -> Value {
    json!({
        "version": 1, "id": ID,
        "archive": {"name": "a", "archivePath": "/a", "completedLabel": "l", "size": "1",
                    "destination": "/", "computers": ["dev"]},
        "request": {"kind": "backup", "names": ["dev"], "computers": [["dev", ID]], "running": [],
                    "pending_capture": {"computerId": ID, "group": "g", "member": "silo-backup-1-2-3"}},
        "cancelled": false, "terminal": null
    })
}

fn old_connections() -> Value {
    json!({"hostId": DEVICE, "enabled": true,
           "hosts": [{"id": "h", "name": "box", "address": "me@box"}], "extra": 1})
}

fn new_connections() -> Value {
    json!({"deviceId": DEVICE, "enabled": true,
           "devices": [{"id": "h", "name": "box", "address": "me@box"}], "extra": 1})
}

/// A launch after an update: a converted generation selected, with `runtime/` kept as the
/// pre-upgrade backup. Returns the selected storage folder.
fn old_installation(fixture: &Fixture) -> PathBuf {
    put(
        &fixture.locations.config.join("settings.json"),
        &old_settings(),
    );
    put(&fixture.data("github.json"), &old_github());
    put(&fixture.data("secrets.json"), &old_secrets());
    put(
        &fixture.data("backup-operation.json"),
        &old_backup_operation(),
    );
    put(
        &fixture.data("backup-history.json"),
        &json!({"schemaVersion": 1, "archives": []}),
    );
    put(
        &fixture.locations.state.join("desktop-remote/config.json"),
        &old_connections(),
    );
    write_old_storage(&fixture.data("runtime"));
    write_old_storage(&fixture.data(CONVERTED));
    fixture.select(CONVERTED, "complete");
    fixture.data(CONVERTED)
}

fn assert_new_documents(fixture: &Fixture) {
    assert_eq!(
        get(&fixture.locations.config.join("settings.json")),
        new_settings()
    );
    assert_eq!(get(&fixture.data("github.json")), new_github());
    assert_eq!(get(&fixture.data("secrets.json")), new_secrets());
    assert_eq!(
        get(&fixture.data("backup-operation.json")),
        new_backup_operation()
    );
    assert_eq!(
        get(&fixture.data("backup-history.json")),
        json!({"schemaVersion": 1, "archives": []})
    );
    assert_eq!(
        get(&fixture.locations.state.join("desktop-remote/config.json")),
        new_connections()
    );
}

#[test]
fn a_full_old_installation_becomes_exactly_the_new_one() {
    let fixture = Fixture::new();
    let storage = old_installation(&fixture);
    let backup = snapshot(&fixture.data("runtime"));

    fixture.run().unwrap();

    assert_new_documents(&fixture);
    assert_new_storage(&storage);
    assert_eq!(
        get(&fixture.data("vocabulary-migration.json")),
        json!({"version": 1})
    );
    assert_eq!(
        snapshot(&fixture.data("runtime")),
        backup,
        "the pre-upgrade backup stays as it was"
    );
}

#[test]
fn ssh_entries_and_their_draft_leftovers_are_dropped() {
    let fixture = Fixture::new();
    let storage = old_installation(&fixture);
    let mut draft = old_settings();
    draft["onboardingDraft"]["unfinishedMachineEditor"] =
        json!({"draft": ssh_entry(), "insertAt": 0});
    put(&fixture.locations.config.join("settings.json"), &draft);

    fixture.run().unwrap();

    let settings = get(&fixture.locations.config.join("settings.json"));
    assert_eq!(
        settings["onboardingDraft"]["unfinishedComputerEditor"],
        Value::Null
    );
    let rendered = settings.to_string() + &get(&storage.join("computers.json")).to_string();
    assert!(!rendered.contains(SSH));
    assert!(!rendered.contains("\"kind\""));
    assert!(!rendered.contains("legacy"));
}

#[test]
fn running_again_changes_nothing_even_without_the_record() {
    let fixture = Fixture::new();
    let storage = old_installation(&fixture);
    fixture.run().unwrap();
    let converted = snapshot(fixture.dir.path());

    fixture.run().unwrap();
    assert_eq!(snapshot(fixture.dir.path()), converted);

    fs::remove_file(fixture.data("vocabulary-migration.json")).unwrap();
    fixture.run().unwrap();
    assert_eq!(snapshot(fixture.dir.path()), converted);
    assert_new_storage(&storage);
}

#[test]
fn a_recorded_migration_is_not_run_again() {
    let fixture = Fixture::new();
    put(
        &fixture.data("vocabulary-migration.json"),
        &json!({"version": 1}),
    );
    put(
        &fixture.locations.config.join("settings.json"),
        &old_settings(),
    );
    let before = snapshot(fixture.dir.path());

    fixture.run().unwrap();

    assert_eq!(snapshot(fixture.dir.path()), before);
}

#[test]
fn a_damaged_record_runs_the_migration_again() {
    let fixture = Fixture::new();
    fs::write(fixture.data("vocabulary-migration.json"), b"{broken").unwrap();
    put(
        &fixture.locations.config.join("settings.json"),
        &old_settings(),
    );

    fixture.run().unwrap();

    assert_eq!(
        get(&fixture.locations.config.join("settings.json")),
        new_settings()
    );
    assert_eq!(
        get(&fixture.data("vocabulary-migration.json")),
        json!({"version": 1})
    );
}

#[test]
fn data_that_already_uses_the_new_names_is_left_alone() {
    let fixture = Fixture::new();
    put(
        &fixture.locations.config.join("settings.json"),
        &new_settings(),
    );
    put(&fixture.data("github.json"), &new_github());
    put(&fixture.data("secrets.json"), &new_secrets());
    put(
        &fixture.data("backup-operation.json"),
        &new_backup_operation(),
    );
    put(
        &fixture.locations.state.join("desktop-remote/config.json"),
        &new_connections(),
    );
    put(
        &fixture.data("runtime/computers.json"),
        &json!({"schemaVersion": 1, "computers": [new_computer(ID, "dev", true)]}),
    );
    put(
        &fixture.data(&format!("{CONVERTED}/computers.json")),
        &json!({"schemaVersion": 1, "computers": [new_computer(ID, "dev", true)]}),
    );
    fixture.select(CONVERTED, "complete");
    let before = snapshot(fixture.dir.path());

    fixture.run().unwrap();

    let mut after = snapshot(fixture.dir.path());
    assert!(after.remove("data/vocabulary-migration.json").is_some());
    assert_eq!(after, before);
}

#[test]
fn an_empty_installation_only_records_completion() {
    let fixture = Fixture::new();
    fs::remove_dir_all(&fixture.locations.app_data).unwrap();
    fixture.run().unwrap();
    let names: Vec<_> = snapshot(&fixture.locations.app_data).into_keys().collect();
    assert_eq!(names, ["vocabulary-migration.json"]);
}

#[test]
fn unknown_fields_survive_where_the_file_keeps_them() {
    let fixture = Fixture::new();
    old_installation(&fixture);
    fixture.run().unwrap();
    assert_eq!(
        get(&fixture.locations.config.join("settings.json"))["settings"]["futureSetting"],
        json!({"kept": true})
    );
    assert_eq!(
        get(&fixture.data("github.json"))["futureField"],
        json!([1, 2])
    );
    assert_eq!(
        get(&fixture.locations.state.join("desktop-remote/config.json"))["extra"],
        json!(1)
    );
}

#[test]
fn an_interruption_at_any_point_is_finished_by_the_next_run() {
    let reference = Fixture::new();
    old_installation(&reference);
    let writes = Cell::new(0);
    vocabulary::run_in(&reference.locations, &|_| {
        writes.set(writes.get() + 1);
        Ok(())
    })
    .unwrap();
    let expected = parsed(&snapshot(reference.dir.path()));
    assert!(writes.get() > 12, "every file and the record are written");

    for stop in 1..=writes.get() {
        let fixture = Fixture::new();
        old_installation(&fixture);
        let calls = Cell::new(0);
        let result = vocabulary::run_in(&fixture.locations, &|path| {
            calls.set(calls.get() + 1);
            if calls.get() == stop {
                Err(io::Error::other(format!(
                    "stopped before {}",
                    path.display()
                )))
            } else {
                Ok(())
            }
        });
        assert!(result.is_err(), "stop {stop}");
        assert!(
            !fixture.data("vocabulary-migration.json").exists(),
            "the record is last (stop {stop})"
        );
        // Whatever was converted is whole, and nothing is half written.
        for (name, bytes) in snapshot(fixture.dir.path()) {
            if name.ends_with(".json") {
                serde_json::from_slice::<Value>(&bytes)
                    .unwrap_or_else(|_| panic!("{name} is not whole after stop {stop}"));
            }
        }
        fixture.run().unwrap();
        assert_eq!(
            parsed(&snapshot(fixture.dir.path())),
            expected,
            "stop {stop}"
        );
    }
}

fn parsed(files: &BTreeMap<String, Vec<u8>>) -> BTreeMap<String, Value> {
    files
        .iter()
        .map(|(name, bytes)| {
            (
                name.clone(),
                serde_json::from_slice(bytes).unwrap_or_else(|_| Value::String(name.clone())),
            )
        })
        .collect()
}

#[test]
fn a_failed_write_leaves_every_original_and_no_record() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    old_installation(&fixture);
    let remote = fixture.locations.state.join("desktop-remote");
    fs::set_permissions(&remote, fs::Permissions::from_mode(0o500)).unwrap();
    if fs::File::create(remote.join("probe")).is_ok() {
        // Running with privileges that ignore the mode: nothing to prove here.
        fs::set_permissions(&remote, fs::Permissions::from_mode(0o700)).unwrap();
        return;
    }
    let before = snapshot(&remote);

    let error = fixture.run().unwrap_err();

    fs::set_permissions(&remote, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(error.contains("Relaunch Silo"));
    assert!(!error.contains("workspace") && !error.contains("sandbox"));
    assert_eq!(snapshot(&remote), before);
    assert!(!fixture.data("vocabulary-migration.json").exists());

    fixture.run().unwrap();
    assert_new_documents(&fixture);
}

#[test]
fn unusable_files_are_carried_over_unchanged() {
    let fixture = Fixture::new();
    let storage = fixture.data("runtime");
    fs::create_dir_all(&storage).unwrap();
    fs::write(fixture.locations.config.join("settings.json"), b"{not json").unwrap();
    fs::write(fixture.data("github.json"), b"[1, 2]").unwrap();
    record_state(&fixture.locations.app_data, "not-required");
    fs::write(storage.join("machines.json"), b"{not json").unwrap();
    fs::write(
        storage.join("sandbox-activity.json"),
        br#"{"not":"a list"}"#,
    )
    .unwrap();

    fixture.run().unwrap();

    assert_eq!(
        fs::read(fixture.locations.config.join("settings.json")).unwrap(),
        b"{not json"
    );
    assert_eq!(fs::read(fixture.data("github.json")).unwrap(), b"[1, 2]");
    assert_eq!(
        fs::read(storage.join("computers.json")).unwrap(),
        b"{not json"
    );
    assert_eq!(
        fs::read(storage.join("computer-activity.json")).unwrap(),
        br#"{"not":"a list"}"#
    );
    assert!(!storage.join("machines.json").exists());
    assert!(fixture.data("vocabulary-migration.json").exists());
}

#[test]
fn files_keep_their_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    old_installation(&fixture);
    let config = fixture.locations.state.join("desktop-remote/config.json");
    fs::set_permissions(&config, fs::Permissions::from_mode(0o600)).unwrap();
    // A selected generation is converted in place.
    fixture.select(CLEAN, "complete");
    write_old_storage(&fixture.data(CLEAN));
    fs::set_permissions(
        fixture.data(CLEAN).join("machines.json"),
        fs::Permissions::from_mode(0o640),
    )
    .unwrap();

    fixture.run().unwrap();

    let mode = |path: PathBuf| fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(config), 0o600);
    assert_eq!(mode(fixture.data(CLEAN).join("computers.json")), 0o640);
}

#[test]
fn a_linked_file_is_not_followed_and_stops_the_migration() {
    let fixture = Fixture::new();
    let outside = fixture.dir.path().join("outside.json");
    put(&outside, &old_github());
    std::os::unix::fs::symlink(&outside, fixture.data("github.json")).unwrap();
    let before = fs::read(&outside).unwrap();

    let error = fixture.run().unwrap_err();

    let linked = fixture.data("github.json");
    assert!(error.contains(&linked.display().to_string()), "{error}");
    assert!(error.contains("Replace it with a regular file"), "{error}");
    assert_eq!(fs::read(&outside).unwrap(), before);
    assert!(fs::symlink_metadata(fixture.data("github.json"))
        .unwrap()
        .is_symlink());
    assert!(!fixture.data("vocabulary-migration.json").exists());
}

#[test]
fn a_linked_inventory_or_activity_file_never_becomes_an_empty_one() {
    for name in ["machines.json", "sandbox-activity.json"] {
        let fixture = Fixture::new();
        fixture.select(CLEAN, "complete");
        write_old_storage(&fixture.data(CLEAN));
        let outside = fixture.dir.path().join("outside.json");
        fs::rename(fixture.data(CLEAN).join(name), &outside).unwrap();
        std::os::unix::fs::symlink(&outside, fixture.data(CLEAN).join(name)).unwrap();
        let before = snapshot(fixture.dir.path());

        assert!(fixture.run().is_err(), "{name}");

        assert_eq!(snapshot(fixture.dir.path()), before, "{name}");
        assert!(!fixture.data("vocabulary-migration.json").exists());
        assert!(!fixture.data(CLEAN).join("computers.json").exists());
    }
}

#[test]
fn an_oversized_document_stops_the_migration() {
    let fixture = Fixture::new();
    fixture.select(CLEAN, "complete");
    write_old_storage(&fixture.data(CLEAN));
    let inventory = fs::File::options()
        .write(true)
        .open(fixture.data(CLEAN).join("machines.json"))
        .unwrap();
    inventory.set_len(65 * 1024 * 1024).unwrap();

    let error = fixture.run().unwrap_err();

    assert!(error.contains("machines.json"), "{error}");
    assert!(error.contains("move it out of Silo's folder"), "{error}");

    assert!(fixture.data(CLEAN).join("machines.json").exists());
    assert!(!fixture.data(CLEAN).join("computers.json").exists());
    assert!(!fixture.data("vocabulary-migration.json").exists());
}

#[test]
fn a_damaged_storage_selection_or_migration_state_stops_the_migration() {
    for (file, bytes) in [
        ("runtime-generation.json", &b"{broken"[..]),
        (
            "runtime-generation.json",
            br#"{"version":1,"directory":"elsewhere"}"#,
        ),
        ("runtime-migration.json", &b"{broken"[..]),
        (
            "runtime-migration.json",
            br#"{"version":9,"status":"complete"}"#,
        ),
    ] {
        let fixture = Fixture::new();
        write_old_storage(&fixture.data("runtime"));
        fs::write(fixture.data(file), bytes).unwrap();
        let before = snapshot(fixture.dir.path());

        assert!(fixture.run().is_err(), "{file}");

        assert_eq!(snapshot(fixture.dir.path()), before, "{file}");
        assert!(!fixture.data("vocabulary-migration.json").exists());
    }
}

#[test]
fn an_unreadable_storage_selection_is_not_the_same_as_no_selection() {
    let fixture = Fixture::new();
    write_old_storage(&fixture.data(CLEAN));
    fs::create_dir(fixture.data("runtime-generation.json")).unwrap();

    assert!(fixture.run().is_err());

    assert!(fixture.data(CLEAN).join("machines.json").exists());
    assert!(!fixture.data("vocabulary-migration.json").exists());
}

/// `channel`'s `folder` replaced by a link into `other`'s tree, which holds a file in the
/// earlier vocabulary at `relative`. Returns that file.
fn link_into(
    channel: &Path,
    folder: &Path,
    other: &Path,
    relative: &str,
    value: &Value,
) -> PathBuf {
    let real = other.join("real");
    let file = real.join(relative);
    put(&file, value);
    fs::remove_dir_all(folder).ok();
    fs::create_dir_all(folder.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&real, folder).unwrap();
    assert!(channel.exists());
    file
}

#[test]
fn a_channel_folder_linked_into_the_other_channel_is_never_written_through() {
    // Each direction: the Dev state folder reaches into production, and the reverse.
    for _ in 0..2 {
        let channel = Fixture::new();
        let other = Fixture::new();
        let file = link_into(
            channel.dir.path(),
            &channel.locations.state.join("desktop-remote"),
            other.dir.path(),
            "config.json",
            &old_connections(),
        );
        let before = fs::read(&file).unwrap();

        assert!(channel.run().is_err());

        assert_eq!(fs::read(&file).unwrap(), before);
        assert!(!channel.data("vocabulary-migration.json").exists());
        // The other channel's own run is unaffected by its folder being linked to.
        other.run().unwrap();
    }
}

#[test]
fn a_linked_channel_root_or_ancestor_of_a_target_is_refused() {
    let outside = |fixture: &Fixture| fixture.dir.path().join("elsewhere");
    for case in ["config", "app_data", "state", "generation", "checkpoints"] {
        let mut fixture = Fixture::new();
        let elsewhere = outside(&fixture);
        let (link, value, relative) = match case {
            "config" => (
                fixture.locations.config.clone(),
                old_settings(),
                "settings.json",
            ),
            "app_data" => (
                fixture.locations.app_data.clone(),
                old_github(),
                "github.json",
            ),
            "state" => (
                fixture.locations.state.clone(),
                old_connections(),
                "desktop-remote/config.json",
            ),
            "generation" => (
                fixture.data(CLEAN),
                json!({"schemaVersion": 1, "machines": [old_computer(ID, "dev", false)]}),
                "machines.json",
            ),
            _ => (
                fixture.data(CLEAN).join("checkpoints"),
                json!({"pendingCheckpointRestore": {"sourceWorkspace": "dev"}}),
                "one.json",
            ),
        };
        if case == "checkpoints" {
            fixture.select(CLEAN, "complete");
            fs::create_dir_all(fixture.data(CLEAN)).unwrap();
        }
        if case == "generation" {
            fixture.select(CLEAN, "complete");
        }
        let file = link_into(fixture.dir.path(), &link, &elsewhere, relative, &value);
        if case == "app_data" {
            // The record and the selection live in the linked folder as well.
            fixture.locations.app_data = link.clone();
        }
        let before = fs::read(&file).unwrap();

        assert!(fixture.run().is_err(), "{case}");

        assert_eq!(fs::read(&file).unwrap(), before, "{case}");
        assert!(!elsewhere.join("real/vocabulary-migration.json").exists());
    }
}

#[test]
fn when_both_names_exist_the_old_file_wins_and_is_removed() {
    let fixture = Fixture::new();
    let storage = fixture.data("runtime");
    record_state(&fixture.locations.app_data, "not-required");
    put(
        &storage.join("machines.json"),
        &json!({"schemaVersion": 1, "machines": [old_computer(ID, "newer", false)]}),
    );
    put(
        &storage.join("computers.json"),
        &json!({"schemaVersion": 1, "computers": [new_computer(ID, "stale", false)]}),
    );

    fixture.run().unwrap();

    assert_eq!(
        get(&storage.join("computers.json")),
        json!({"schemaVersion": 1, "computers": [new_computer(ID, "newer", false)]})
    );
    assert!(!storage.join("machines.json").exists());
}

fn pending_installation(fixture: &Fixture, state: Option<&str>) {
    write_old_storage(&fixture.data("runtime"));
    if let Some(status) = state {
        record_state(&fixture.locations.app_data, status);
    }
}

#[test]
fn storage_the_storage_migration_still_has_to_convert_is_not_touched() {
    for state in [None, Some("scanning"), Some("running"), Some("failed")] {
        let fixture = Fixture::new();
        pending_installation(&fixture, state);
        let before = snapshot(&fixture.data("runtime"));

        fixture.run().unwrap();

        assert_eq!(
            snapshot(&fixture.data("runtime")),
            before,
            "state {state:?}"
        );
        // The storage migration reads the previous inventory in the new vocabulary.
        let inventory = vocabulary::read_previous_computers(&fixture.data("runtime"))
            .unwrap()
            .unwrap();
        assert_eq!(
            inventory,
            json!({"schemaVersion": 1,
                   "computers": [new_computer(ID, "dev", true), new_computer(OTHER, "fork", false)]})
        );
    }
}

#[test]
fn the_staged_copy_of_a_pending_storage_migration_is_converted_by_the_migration() {
    let fixture = Fixture::new();
    pending_installation(&fixture, Some("scanning"));
    fixture.run().unwrap();
    let staged = fixture.data(CONVERTED);
    write_old_storage(&staged);

    vocabulary::convert_storage_directory(&staged).unwrap();
    vocabulary::convert_storage_directory(&staged).unwrap();

    assert_new_storage(&staged);
    assert!(fixture.data("runtime/machines.json").exists());
}

#[test]
fn storage_that_needs_no_migration_is_converted_in_place() {
    for state in [Some("not-required"), None] {
        let fixture = Fixture::new();
        let runtime = fixture.data("runtime");
        put(
            &runtime.join("machines.json"),
            &json!({"schemaVersion": 1, "machines": [ssh_entry()]}),
        );
        put(
            &runtime.join("sandbox-activity.json"),
            &json!([old_event("start")]),
        );
        if let Some(status) = state {
            record_state(&fixture.locations.app_data, status);
        }

        fixture.run().unwrap();

        assert_eq!(
            get(&runtime.join("computers.json")),
            json!({"schemaVersion": 1, "computers": []}),
            "state {state:?}"
        );
        assert_eq!(
            get(&runtime.join("computer-activity.json")),
            json!([new_event("start")])
        );
        assert!(!runtime.join("machines.json").exists());
        // What the storage migration then reads.
        assert_eq!(
            vocabulary::read_previous_computers(&runtime)
                .unwrap()
                .unwrap(),
            json!({"schemaVersion": 1, "computers": []})
        );
    }
}

#[test]
fn a_clean_generation_is_converted_in_place() {
    let fixture = Fixture::new();
    write_old_storage(&fixture.data("runtime"));
    write_old_storage(&fixture.data(CLEAN));
    fixture.select(CLEAN, "complete");
    let backup = snapshot(&fixture.data("runtime"));

    fixture.run().unwrap();

    assert_new_storage(&fixture.data(CLEAN));
    assert_eq!(snapshot(&fixture.data("runtime")), backup);
}

#[test]
fn the_previous_inventory_is_read_from_either_file_without_changing_it() {
    let fixture = Fixture::new();
    let folder = fixture.data("runtime");
    assert_eq!(vocabulary::read_previous_computers(&folder).unwrap(), None);

    put(
        &folder.join("computers.json"),
        &json!({"schemaVersion": 1, "computers": [new_computer(ID, "dev", false)]}),
    );
    let new = vocabulary::read_previous_computers(&folder)
        .unwrap()
        .unwrap();
    assert_eq!(new["computers"][0]["name"], "dev");

    put(
        &folder.join("machines.json"),
        &json!({"schemaVersion": 1, "machines": [ssh_entry(), old_computer(OTHER, "old", false)]}),
    );
    let old = vocabulary::read_previous_computers(&folder)
        .unwrap()
        .unwrap();
    assert_eq!(old["computers"], json!([new_computer(OTHER, "old", false)]));
    assert!(folder.join("machines.json").exists());

    fs::write(folder.join("machines.json"), b"{broken").unwrap();
    assert!(vocabulary::read_previous_computers(&folder).is_err());
}

#[test]
fn a_renamed_file_written_but_not_yet_unlinked_is_finished_by_the_next_run() {
    // The new file is on disk and the old one still is: the old one is the only build's
    // latest write, so it wins and is then removed.
    let fixture = Fixture::new();
    fixture.select(CLEAN, "complete");
    let storage = fixture.data(CLEAN);
    write_old_storage(&storage);
    fixture.run().unwrap();
    fs::remove_file(fixture.data("vocabulary-migration.json")).unwrap();
    assert_new_storage(&storage);

    put(
        &storage.join("machines.json"),
        &json!({"schemaVersion": 1, "machines": [old_computer(ID, "renamed later", false)]}),
    );
    put(
        &storage.join("sandbox-activity.json"),
        &json!([old_event("stop")]),
    );

    fixture.run().unwrap();

    assert_eq!(
        get(&storage.join("computers.json")),
        json!({"schemaVersion": 1, "computers": [new_computer(ID, "renamed later", false)]})
    );
    assert_eq!(
        get(&storage.join("computer-activity.json")),
        json!([new_event("stop")])
    );
    assert!(!storage.join("machines.json").exists());
    assert!(!storage.join("sandbox-activity.json").exists());
    assert!(fixture.data("vocabulary-migration.json").exists());
}

#[test]
fn an_interruption_between_writing_and_unlinking_a_renamed_file_loses_nothing() {
    // Stop when the next file is about to be written: `computers.json` is replaced and
    // `machines.json` removed, while the activity file is still in the earlier name.
    let fixture = Fixture::new();
    fixture.select(CLEAN, "complete");
    let storage = fixture.data(CLEAN);
    write_old_storage(&storage);
    let seen = Cell::new(false);
    let result = vocabulary::run_in(&fixture.locations, &|path| {
        if path.ends_with("computer-activity.json") {
            seen.set(true);
            assert!(!path.parent().unwrap().join("machines.json").exists());
            return Err(io::Error::other("stopped"));
        }
        Ok(())
    });
    assert!(result.is_err() && seen.get());
    assert!(storage.join("computers.json").exists());
    assert!(storage.join("sandbox-activity.json").exists());

    fixture.run().unwrap();

    assert_new_storage(&storage);
}

/// An old-format installation after the migration ran on it, for the tests of the readers
/// that load the converted files.
pub(crate) struct Migrated {
    _dir: tempfile::TempDir,
    pub(crate) settings: PathBuf,
    pub(crate) app_data: PathBuf,
    /// The storage generation Silo reads.
    pub(crate) storage: PathBuf,
    /// The private state folder.
    pub(crate) state: PathBuf,
}

impl Migrated {
    /// Paths over the converted storage generation, as the runtime builds them.
    pub(crate) fn runtime_paths(&self) -> crate::runtime::RuntimePaths {
        crate::runtime::RuntimePaths {
            guest_image: self.app_data.join("image"),
            executable: self.app_data.join("msb"),
            home: self.storage.join("microsandbox"),
            storage_home: None,
            library: self.app_data.join("lib"),
            metadata: self.storage.join("computers.json"),
            volumes: self.storage.join("volumes"),
        }
    }
}

pub(crate) fn migrated_installation() -> Migrated {
    let fixture = Fixture::new();
    let storage = old_installation(&fixture);
    fixture.run().unwrap();
    let Fixture { dir, locations } = fixture;
    Migrated {
        _dir: dir,
        settings: locations.config.join("settings.json"),
        app_data: locations.app_data,
        storage,
        state: locations.state,
    }
}
