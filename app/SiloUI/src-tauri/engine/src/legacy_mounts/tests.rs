use super::*;
use crate::runtime::{CommandOutput, RuntimeError};
use serde_json::json;

fn mount(host: &str, guest: &str, readonly: bool) -> Value {
    json!({"type": "Bind", "host": host, "guest": guest, "options": {"readonly": readonly}})
}

fn config(host: &str) -> Value {
    json!({"mounts": [
        mount(&format!("{host}/chatgpt/1"), CHATGPT_GUEST_MOUNT, true),
        mount(&format!("{host}/lcu/2"), LCU_GUEST_MOUNT, true),
        mount("/data/other", "/mnt/other", false),
    ]})
}

#[test]
fn only_the_two_guest_paths_count_as_legacy_mounts() {
    let value = config("/h");
    assert_eq!(
        sources(&value),
        [PathBuf::from("/h/chatgpt/1"), PathBuf::from("/h/lcu/2")]
    );
    assert!(sources(&json!({})).is_empty());
}

#[test]
fn missing_sources_are_recreated_empty_only_below_the_silo_folders() {
    let directory = tempfile::tempdir().unwrap();
    let base = directory.path().to_str().unwrap();
    let roots = [
        directory.path().join("chatgpt"),
        directory.path().join("lcu"),
    ];
    restore_missing(&config(base), &roots);
    assert!(directory.path().join("chatgpt/1").is_dir());
    assert!(directory.path().join("lcu/2").is_dir());
    // A source elsewhere, or with an odd name, is left for the runtime to report.
    let elsewhere = json!({"mounts": [
        mount(&format!("{base}/other/x"), LCU_GUEST_MOUNT, true),
        mount(&format!("{base}/lcu/.hidden"), CHATGPT_GUEST_MOUNT, true),
        mount(&format!("{base}/lcu/a b"), CHATGPT_GUEST_MOUNT, true),
    ]});
    restore_missing(&elsewhere, &roots);
    assert!(!directory.path().join("other").exists());
    assert!(!directory.path().join("lcu/.hidden").exists());
    assert!(!directory.path().join("lcu/a b").exists());
}

#[test]
fn restore_arguments_repeat_read_only_legacy_mounts_whose_source_exists() {
    let directory = tempfile::tempdir().unwrap();
    let base = directory.path().to_str().unwrap();
    fs::create_dir_all(directory.path().join("chatgpt/1")).unwrap();
    assert_eq!(
        mount_args(&config(base)),
        [
            "-v",
            &format!("{base}/chatgpt/1:{CHATGPT_GUEST_MOUNT}:ro,uid=0,gid=0")
        ]
    );
    assert!(mount_args(&json!({})).is_empty());
    let writable = json!({"mounts": [mount(base, LCU_GUEST_MOUNT, false)]});
    assert!(mount_args(&writable).is_empty());
}

#[test]
fn an_export_drops_both_mounts_and_refuses_a_writable_one() {
    let mut value = config("/h");
    strip_for_export(&mut value).unwrap();
    assert_eq!(
        value["mounts"],
        json!([mount("/data/other", "/mnt/other", false)])
    );
    let mut writable = json!({"mounts": [mount("/h/lcu/2", LCU_GUEST_MOUNT, false)]});
    assert!(strip_for_export(&mut writable).is_err());
    let mut none = json!({});
    strip_for_export(&mut none).unwrap();
}

struct Listing {
    status: &'static str,
}

impl RuntimeRunner for Listing {
    fn run(
        &self,
        _: &RuntimePaths,
        args: &[String],
        _: Duration,
    ) -> Result<CommandOutput, RuntimeError> {
        let stdout = match args.first().map(String::as_str) {
            Some("list") => json!([{"name": "dev"}]).to_string(),
            _ => json!({"name": "dev", "status": self.status, "config": {}}).to_string(),
        };
        Ok(CommandOutput {
            stdout,
            stderr: String::new(),
        })
    }
}

#[test]
fn reclaiming_empties_the_download_folders_only_while_no_computer_runs() {
    let directory = tempfile::tempdir().unwrap();
    let paths = crate::test_support::paths(directory.path());
    let app_data = directory.path().join("data");
    let source = app_data.join("lcu/2");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("archive.tar.gz"), b"x").unwrap();
    fs::write(app_data.join("lcu/stray"), b"x").unwrap();
    let gate = OperationGate::new();

    let running = Listing { status: "running" };
    assert!(!reclaim_disk_with(&gate, &running, &paths, &app_data));
    assert!(source.join("archive.tar.gz").exists());

    let stopped = Listing { status: "stopped" };
    assert!(reclaim_disk_with(&gate, &stopped, &paths, &app_data));
    assert!(source.is_dir(), "the recorded mount source stays");
    assert!(!source.join("archive.tar.gz").exists());
    assert!(!app_data.join("lcu/stray").exists());
    // Nothing to do once the folders are gone.
    fs::remove_dir_all(app_data.join("lcu")).unwrap();
    assert!(reclaim_disk_with(&gate, &running, &paths, &app_data));
}
