use std::{collections::VecDeque, io::Write};

use sha2::{Digest, Sha256};

use super::*;
use crate::{
    changes::collect::{ExecOut, FileChunk},
    guest::GuestUser,
};

/// A guest whose successive captures are the given manifests.
struct ScriptedGuest {
    captures: Mutex<VecDeque<String>>,
    current: Mutex<Vec<u8>>,
    owners: &'static str,
}

impl ScriptedGuest {
    fn new(captures: &[&str]) -> Self {
        Self {
            captures: Mutex::new(captures.iter().map(|text| (*text).to_owned()).collect()),
            current: Mutex::new(Vec::new()),
            owners: "",
        }
    }
}

fn out(text: &str) -> ExecOut {
    ExecOut {
        exit_code: Some(0),
        stdout: text.as_bytes().to_vec(),
        stderr: Vec::new(),
    }
}

impl GuestRunner for ScriptedGuest {
    fn is_macos(&self) -> bool {
        false
    }

    fn exec(
        &self,
        argv: &[String],
        _stdin: Option<Vec<u8>>,
        _user: GuestUser,
        _timeout: Duration,
    ) -> Result<ExecOut, GuestError> {
        match argv[0].as_str() {
            "sh" if argv[2].starts_with("d=$1\numask") => {
                let next = self
                    .captures
                    .lock()
                    .unwrap()
                    .pop_front()
                    .expect("a capture is left");
                let mut encoder =
                    flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
                encoder.write_all(next.as_bytes()).unwrap();
                *self.current.lock().unwrap() = encoder.finish().unwrap();
                Ok(out("started\n"))
            }
            "sh" => {
                let digest: String = Sha256::digest(&*self.current.lock().unwrap())
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect();
                Ok(out(&format!("done 0 {digest}\n")))
            }
            "bash" => Ok(out(self.owners)),
            _ => Ok(out("")),
        }
    }

    fn read(&self, _: &str, offset: u64, length: u64) -> Result<FileChunk, GuestError> {
        let gz = self.current.lock().unwrap();
        let start = (offset as usize).min(gz.len());
        let end = (start + length as usize).min(gz.len());
        Ok(FileChunk {
            data: gz[start..end].to_vec(),
            size: gz.len() as u64,
        })
    }
}

fn manifest(files: &[(&str, &str)]) -> String {
    let mut text = String::from("META\tformat\t2\n");
    for (path, signature) in files {
        text.push_str(&format!("F\t{path}\t{signature}\n"));
    }
    text
}

/// `path:change` of every entry of the diff's Other changed list.
fn other(result: &Value) -> Vec<String> {
    result["diff"]["other"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            format!(
                "{}:{}",
                entry["path"].as_str().unwrap(),
                entry["change"].as_str().unwrap()
            )
        })
        .collect()
}

fn capture_work(role: ChangesRole, source: Option<&str>) -> Work {
    Work::Capture {
        role,
        source: source.map(str::to_owned),
    }
}

fn diff_work(baseline: bool, refresh: bool) -> Work {
    Work::Diff {
        max_lines: None,
        baseline,
        refresh,
    }
}

#[test]
fn a_diff_compares_the_current_capture_with_the_fork_time_base() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path());
    let guest = ScriptedGuest::new(&[
        &manifest(&[("/srv/data/a", "1\t1\t-"), ("/srv/data/b", "1\t1\t-")]),
        &manifest(&[
            ("/srv/data/a", "2\t2\t-"),
            ("/srv/data/b", "1\t1\t-"),
            ("/srv/data/c", "1\t1\t-"),
        ]),
    ]);
    let options = Options::default();
    let base = run(
        &guest,
        &store,
        "c1",
        capture_work(ChangesRole::Base, Some("cp1")),
        &options,
    )
    .unwrap();
    assert_eq!(base["role"], "base");
    let result = run(&guest, &store, "c1", diff_work(true, true), &options).unwrap();
    assert_eq!(
        other(&result),
        ["/srv/data/a:modified", "/srv/data/c:added"]
    );
    assert_eq!(result["baselineAvailable"], false);
    assert_eq!(result["baselineApplied"], false);
    assert!(result["text"].as_str().unwrap().contains("/srv/data"));
    // The capture was stored: the same comparison without a new scan.
    let again = run(&guest, &store, "c1", diff_work(true, false), &options).unwrap();
    assert_eq!(again["diff"], result["diff"]);
}

#[test]
fn without_a_base_there_is_nothing_to_compare() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path());
    let guest = ScriptedGuest::new(&[]);
    let error = run(
        &guest,
        &store,
        "c9",
        diff_work(true, true),
        &Options::default(),
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
    let error = run(
        &guest,
        &store,
        "c9",
        diff_work(true, false),
        &Options::default(),
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
}

#[test]
fn the_cached_baseline_is_subtracted_from_a_diff() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path());
    let start = manifest(&[("/srv/data/a", "1\t1\t-"), ("/srv/data/noise", "1\t1\t-")]);
    let idle = manifest(&[("/srv/data/a", "1\t1\t-"), ("/srv/data/noise", "9\t9\t-")]);
    let later = manifest(&[("/srv/data/a", "2\t2\t-"), ("/srv/data/noise", "12\t12\t-")]);
    let guest = ScriptedGuest::new(&[&start, &idle, &later]);
    let options = Options::default();
    run(
        &guest,
        &store,
        "fork",
        capture_work(ChangesRole::Base, Some("cp1")),
        &options,
    )
    .unwrap();
    let baseline = run(
        &guest,
        &store,
        "fork",
        Work::Baseline {
            source: None,
            idle_seconds: Some(0),
        },
        &options,
    )
    .unwrap();
    assert_eq!(baseline["source"], "cp1");
    assert_eq!(baseline["idleSeconds"], 0);
    let result = run(&guest, &store, "fork", diff_work(true, true), &options).unwrap();
    assert_eq!(result["baselineApplied"], true);
    assert_eq!(other(&result), ["/srv/data/a:modified"]);

    // `--no-baseline` shows the idle noise too.
    let guest = ScriptedGuest::new(&[&later]);
    let raw = run(&guest, &store, "fork", diff_work(false, true), &options).unwrap();
    assert_eq!(raw["baselineApplied"], false);
    assert_eq!(raw["baselineAvailable"], true);
    assert_eq!(
        other(&raw),
        ["/srv/data/a:modified", "/srv/data/noise:modified"]
    );
}

#[test]
fn a_baseline_needs_to_know_its_source() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path());
    let guest = ScriptedGuest::new(&[&manifest(&[])]);
    let options = Options::default();
    run(
        &guest,
        &store,
        "forkb",
        capture_work(ChangesRole::Base, None),
        &options,
    )
    .unwrap();
    let work = Work::Baseline {
        source: None,
        idle_seconds: Some(0),
    };
    let error = run(&guest, &store, "forkb", work, &options).unwrap_err();
    assert!(error.message.contains("source"), "{}", error.message);
}

#[test]
fn a_missing_base_says_why_and_how_to_retake_it() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path());
    let guest = ScriptedGuest::new(&[]);
    store
        .put_failure("failed", "The computer is not reachable.")
        .unwrap();
    let options = Options::default();
    let error = run(&guest, &store, "failed", diff_work(true, true), &options).unwrap_err();
    assert!(error.message.contains("not reachable"), "{}", error.message);
    assert!(error.message.contains("changes.capture role=base"));
    let error = run(&guest, &store, "plain", diff_work(true, true), &options).unwrap_err();
    assert!(error.message.contains("changes.capture role=base"));
}

#[test]
fn a_computer_is_captured_one_at_a_time() {
    let claim = InFlight::claim("busy").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let guest = ScriptedGuest::new(&[]);
    let error = run(
        &guest,
        &Store::new(dir.path()),
        "busy",
        capture_work(ChangesRole::Latest, None),
        &Options::default(),
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::Busy);
    drop(claim);
    assert!(InFlight::claim("busy").is_ok());
}

#[test]
fn a_started_job_finishes_with_the_work_result() {
    let dir = tempfile::tempdir().unwrap();
    let context = crate::guest::test_support::RecordingContext::new(dir.path());
    let guest = Arc::new(ScriptedGuest::new(&[&manifest(&[(
        "/srv/data/a",
        "1\t1\t-",
    )])]));
    let job = start(
        context.clone(),
        guest,
        "job-computer".to_owned(),
        "changes.capture",
        capture_work(ChangesRole::Base, Some("cp1")),
    )
    .unwrap();
    let lookup = context.wait_finished(&job);
    let value = serde_json::to_value(&lookup).unwrap();
    assert_eq!(value["outcome"]["result"]["role"], "base", "{value}");
    assert_eq!(value["outcome"]["result"]["records"], 2);
    let stored = Store::for_engine(context.as_ref())
        .get("job-computer", Slot::Base)
        .unwrap()
        .unwrap();
    assert_eq!(stored.1.source.as_deref(), Some("cp1"));
}
