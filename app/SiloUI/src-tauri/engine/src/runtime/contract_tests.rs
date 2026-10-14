//! Native-emitted wire fixtures. See src/test/contracts/README.md for regeneration.
use super::*;

pub(crate) fn assert_fixture(name: &str, actual: impl Serialize) {
    let actual = serde_json::to_value(actual).unwrap();
    let path = crate::test_support::manifest_root()
        .join("../src/test/contracts")
        .join(name);
    if std::env::var("SILO_UPDATE_CONTRACT_FIXTURES").as_deref() == Ok("1") {
        fs::write(
            &path,
            format!("{}\n", serde_json::to_string_pretty(&actual).unwrap()),
        )
        .unwrap();
    }
    let expected: Value = serde_json::from_slice(&fs::read(&path).unwrap_or_else(|error| {
        panic!(
            "Cannot read {}: {error}. See src/test/contracts/README.md.",
            path.display()
        )
    }))
    .unwrap();
    assert_eq!(
        actual, expected,
        "Native wire contract changed: {name}. Review before regenerating."
    );
}

const ID: &str = "00000000-0000-4000-8000-000000000001";

struct SnapshotRunner(&'static str);
impl RuntimeRunner for SnapshotRunner {
    fn run(
        &self,
        _: &RuntimePaths,
        args: &[String],
        _: Duration,
    ) -> Result<CommandOutput, RuntimeError> {
        let args: Vec<_> = args.iter().map(String::as_str).collect();
        let output = match args.as_slice() {
            ["list", "--label", "silo.managed=true", "--format", "json"] => json!([{"name":"dev"}]),
            ["inspect", "dev", "--format", "json"] => json!({
                "name":"dev", "status":self.0, "updated_at":"2026-01-01T00:00:00Z",
                "config": {
                    "name":"dev", "labels":{"silo.managed":"true", "silo.machine-id":ID},
                    "resources":{"cpus":2,"max_cpus":4,"memory_mib":4096,"max_memory_mib":8192},
                    "image":{"Oci":{"root_disk":{"size_mib":20480}}},
                    "mounts":[{"type":"Owned","guest":"/workspace","storage":{"kind":"disk","capacity_mib":10240}}]
                }
            }),
            _ => panic!("Unhandled contract runtime command: {args:?}"),
        };
        Ok(CommandOutput {
            stdout: output.to_string(),
            stderr: String::new(),
        })
    }
}

#[test]
fn application_and_remote_snapshot_match_wire_contract() {
    let _test_state = crate::test_support::global_state();
    let directory = tempfile::tempdir().unwrap();
    let paths = super::tests::paths(&directory);
    crate::secrets::use_test_store(Some(directory.path().join("secrets.json")));
    struct ResetSecrets;
    impl Drop for ResetSecrets {
        fn drop(&mut self) {
            crate::secrets::use_test_store(None);
        }
    }
    let _reset = ResetSecrets;
    let configuration = ComputerConfiguration {
        id: ID.into(),
        name: "dev".into(),
        cpus: 2,
        max_cpus: 4,
        memory_gib: 4,
        max_memory_gib: 8,
        workspace_storage_gib: 10,
        runtime_storage_gib: 20,
        desktop: None,
    };
    write_metadata(
        &paths.metadata,
        &ComputerConfigurationRequest {
            schema_version: 1,
            computers: vec![configuration],
        },
    )
    .unwrap();
    // Durable native records exercise optional checkpoint and lifecycle fields in
    // the read path, rather than copying the UI's own fixture into the response.
    fs::create_dir_all(directory.path().join("checkpoints")).unwrap();
    fs::write(directory.path().join("checkpoints").join(format!("{ID}.json")), json!({
        "version":1,
        "checkpoints":[{"id":"point-1","nativeId":"native-1","name":"Before change","createdAt":1767225600000u64,"scope":"full","reason":"manual"}],
        "pendingCheckpointRestore":null,
        "checkpointOperation":{"kind":"capture","status":"failed","stage":"Saving checkpoint","error":"Checkpoint storage is unavailable."},
        "restoreJournal":{
            "targetCheckpointId":"point-1", "phase":"secured", "priorRunning":false,
            "recoveryCheckpoint":{"id":"recovery-1","name":"Before restore","createdAt":1767225600000u64,"scope":"disk","reason":"before-restore"}
        }
    }).to_string()).unwrap();
    fs::write(
        directory.path().join("computer-activity.json"),
        json!([{
            "id":"contract-lifecycle-1", "action":"start", "computer":"dev", "computerId":ID,
            "timestamp":1767225600000u64, "completed":true,
            "failure":"The computer could not start.", "diagnostic":"Runtime startup failed.",
            "dismissed":false, "process":0
        }])
        .to_string(),
    )
    .unwrap();
    let mut states = Vec::new();
    for status in ["Running", "Starting", "Stopped", "Crashed"] {
        let mut source = read_application_state_with(&SnapshotRunner(status), &paths).unwrap();
        // Host capacity is the only environment-dependent field. Keep its real
        // serializer while giving it a fixed measured value for this contract.
        source.device_capacity = DeviceCapacity::of(&DeviceResources {
            logical_cpus: 8,
            physical_memory_bytes: Some(16 * 1024 * 1024 * 1024),
        });
        states.push(source);
    }
    assert_fixture("application-state.json", &states);
    // runtime.remote_ops dispatch serializes the same ApplicationSource to Value;
    // device_snapshot passes that JSON through. Cover that wire conversion.
    let remote: Vec<Value> = states
        .iter()
        .map(|source| serde_json::to_value(source).unwrap())
        .collect();
    assert_fixture("device-snapshot.json", remote);
}
