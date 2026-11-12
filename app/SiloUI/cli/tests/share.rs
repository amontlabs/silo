//! `silo export` and `silo import` against a fake engine.

mod fake_engine;

use std::{cell::RefCell, path::Path};

use fake_engine::{finished, listing, FakeEngine};
use serde_json::{json, Value};
use silo_cli::{
    cli::{run, Cli},
    engine::Engine,
    share::{export, import, Asker, ExportArgs},
};

fn ignore(_: &str) {}

struct Answers {
    interactive: bool,
    yes: bool,
    asked: RefCell<Vec<String>>,
}

impl Answers {
    fn new(interactive: bool, yes: bool) -> Self {
        Self {
            interactive,
            yes,
            asked: RefCell::new(Vec::new()),
        }
    }
}

impl Asker for Answers {
    fn interactive(&self) -> bool {
        self.interactive
    }
    fn ask(&self, prompt: &str) -> Result<bool, silo_cli::error::CliError> {
        self.asked.borrow_mut().push(prompt.to_owned());
        Ok(self.yes)
    }
}

const CLAUDE: &str = "a Claude Code login (/root/.claude/.credentials.json)";

/// An engine with `main`, whose logins are `logins`, and that reports `outcome` for the export.
fn engine(logins: Value, outcome: &'static str) -> FakeEngine {
    FakeEngine::start(move |method, params| match method {
        "computers.list" => {
            let mut listed = listing(&[("main", "running")]);
            listed["linux"]["computers"][0]["checkpoints"] =
                json!([{"id": "cp-1", "name": "before-upgrade"}]);
            Ok(listed)
        }
        "exports.check" => Ok(json!({"jobId": "exports:check"})),
        "exports.create" => Ok(json!({"jobId": "exports:op"})),
        "exports.import" => Ok(json!({"jobId": "exports:in"})),
        "operations.job.get" if params["jobId"] == "exports:check" => Ok(finished(
            json!({"logins": logins.clone(), "checked": ["disk"]}),
        )),
        "operations.job.get" if params["jobId"] == "exports:op" => Ok(finished(json!({
            "kind": "result", "operation": "backup", "outcome": outcome,
            "message": "The destination is full.",
            "archive": {"archivePath": "/out/main.silo-backup", "size": "1.2 GiB",
                        "checkpointName": null}
        }))),
        "operations.job.get" => Ok(finished(json!({
            "kind": "result", "operation": "restore", "outcome": outcome,
            "message": "The export file is not valid.", "archive": {}
        }))),
        "operations.job.acknowledge" => Ok(Value::Null),
        _ => Ok(Value::Null),
    })
}

async fn connect(engine: &FakeEngine) -> Engine {
    Engine::connect(&engine.socket).await.unwrap()
}

fn args(yes: bool) -> ExportArgs {
    ExportArgs {
        computer: "main".into(),
        output: Some("/out/main.silo-backup".into()),
        checkpoint: None,
        yes,
        include_logins: false,
    }
}

/// An engine whose export leaves the logins out: the check says so, and the finished export
/// names the files it left out.
fn leaving_engine() -> FakeEngine {
    FakeEngine::start(move |method, params| match method {
        "computers.list" => Ok(listing(&[("main", "running")])),
        "exports.check" => Ok(json!({"jobId": "exports:check"})),
        "exports.create" => Ok(json!({"jobId": "exports:op"})),
        "operations.job.get" if params["jobId"] == "exports:check" => Ok(finished(
            json!({"logins": [CLAUDE], "checked": ["disk"], "leavesOut": true}),
        )),
        "operations.job.get" => Ok(finished(json!({
            "kind": "result", "operation": "backup", "outcome": "success",
            "message": "Computer exported.",
            "archive": {"archivePath": "/out/main.silo-backup", "size": "1.2 GiB",
                        "leftOut": [CLAUDE]}
        }))),
        "operations.job.acknowledge" => Ok(Value::Null),
        _ => Ok(Value::Null),
    })
}

#[tokio::test]
async fn an_export_that_leaves_the_logins_out_asks_nothing_and_reports_what_it_left_out() {
    let fake = leaving_engine();
    let asker = Answers::new(false, false);
    let value = export(
        &connect(&fake).await,
        &args(false),
        Path::new("/"),
        &asker,
        &ignore,
    )
    .await
    .unwrap();
    assert!(asker.asked.borrow().is_empty());
    assert_eq!(value["logins"], json!([]));
    assert_eq!(value["unchecked"], Value::Null);
    assert_eq!(value["leftOut"], json!([CLAUDE]));
    assert!(fake.calls("exports.check")[0]
        .get("includeLogins")
        .is_none());
    assert!(fake.calls("exports.create")[0]
        .get("includeLogins")
        .is_none());
    let line = silo_cli::render::exported(&value);
    assert!(
        line.contains(&format!("Removed from the exported computer: {CLAUDE}")),
        "{line}"
    );
    assert!(line.contains("Traces of them may remain"), "{line}");
}

#[tokio::test]
async fn including_logins_is_asked_for_to_the_engine_and_the_old_question_applies() {
    let fake = engine(json!([CLAUDE]), "success");
    let asker = Answers::new(true, true);
    let mut with = args(false);
    with.include_logins = true;
    let value = export(
        &connect(&fake).await,
        &with,
        Path::new("/"),
        &asker,
        &ignore,
    )
    .await
    .unwrap();
    assert_eq!(fake.calls("exports.check")[0]["includeLogins"], json!(true));
    assert_eq!(
        fake.calls("exports.create")[0]["includeLogins"],
        json!(true)
    );
    assert_eq!(asker.asked.borrow().len(), 1);
    assert_eq!(value["logins"], json!([CLAUDE]));
    assert_eq!(value["leftOut"], json!([]));
}

#[tokio::test]
async fn a_computer_without_logins_is_exported_to_the_named_file_without_a_question() {
    let fake = engine(json!([]), "success");
    let asker = Answers::new(false, false);
    let value = export(
        &connect(&fake).await,
        &args(false),
        Path::new("/"),
        &asker,
        &ignore,
    )
    .await
    .unwrap();
    assert!(asker.asked.borrow().is_empty());
    assert_eq!(value["archive"], "/out/main.silo-backup");
    assert_eq!(value["logins"], json!([]));
    let sent = &fake.calls("exports.create")[0];
    assert_eq!(sent["archivePath"], "/out/main.silo-backup");
    assert_eq!(sent["destination"], "/out");
    assert_eq!(sent["computerIds"], json!(["id-main"]));
    assert!(sent.get("checkpointId").is_none());
}

#[tokio::test]
async fn logins_are_named_in_the_question_and_a_no_writes_nothing() {
    let fake = engine(json!([CLAUDE]), "success");
    let asker = Answers::new(true, false);
    let error = export(
        &connect(&fake).await,
        &args(false),
        Path::new("/"),
        &asker,
        &ignore,
    )
    .await
    .unwrap_err();
    assert_eq!(
        asker.asked.borrow().as_slice(),
        [format!(
            "This computer contains {CLAUDE}. Share anyway? [y/N]"
        )]
    );
    assert!(error.message.starts_with("Not shared"));
    assert!(fake.calls("exports.create").is_empty());
}

#[tokio::test]
async fn a_yes_exports_with_the_logins_reported() {
    let fake = engine(json!([CLAUDE]), "success");
    let asker = Answers::new(true, true);
    let value = export(
        &connect(&fake).await,
        &args(false),
        Path::new("/"),
        &asker,
        &ignore,
    )
    .await
    .unwrap();
    assert_eq!(value["logins"], json!([CLAUDE]));
    assert_eq!(fake.calls("exports.create").len(), 1);
}

#[tokio::test]
async fn without_a_terminal_logins_stop_the_export_unless_yes_is_passed() {
    let fake = engine(json!([CLAUDE]), "success");
    let asker = Answers::new(false, true);
    let error = export(
        &connect(&fake).await,
        &args(false),
        Path::new("/"),
        &asker,
        &ignore,
    )
    .await
    .unwrap_err();
    assert!(error.message.contains(CLAUDE), "{}", error.message);
    assert!(error.message.contains("--yes"));
    assert!(asker.asked.borrow().is_empty());
    assert!(fake.calls("exports.create").is_empty());

    export(
        &connect(&fake).await,
        &args(true),
        Path::new("/"),
        &asker,
        &ignore,
    )
    .await
    .unwrap();
    assert!(asker.asked.borrow().is_empty());
    assert_eq!(fake.calls("exports.create").len(), 1);
}

#[tokio::test]
async fn a_check_that_fails_asks_the_same_question() {
    let fake = FakeEngine::start(|method, params| match method {
        "computers.list" => Ok(listing(&[("main", "running")])),
        "exports.check" => Ok(json!({"jobId": "exports:check"})),
        "operations.job.get" if params["jobId"] == "exports:check" => Ok(json!({
            "jobId": "exports:check", "state": "finished",
            "outcome": {"error": {"code": "internal", "message": "The computer is busy."}}
        })),
        _ => Ok(Value::Null),
    });
    let asker = Answers::new(false, false);
    let error = export(
        &connect(&fake).await,
        &args(false),
        Path::new("/"),
        &asker,
        &ignore,
    )
    .await
    .unwrap_err();
    assert!(
        error.message.contains("could not check"),
        "{}",
        error.message
    );
    assert!(
        error.message.contains("(The computer is busy)"),
        "{}",
        error.message
    );
    assert!(error.message.contains("--yes"));
    assert!(fake.calls("exports.create").is_empty());
}

#[tokio::test]
async fn a_failed_check_with_yes_is_reported_not_passed_off_as_clean() {
    let fake = FakeEngine::start(|method, params| match method {
        "computers.list" => Ok(listing(&[("main", "running")])),
        "exports.check" => Ok(json!({"jobId": "exports:check"})),
        "exports.create" => Ok(json!({"jobId": "exports:op"})),
        "operations.job.get" if params["jobId"] == "exports:check" => Ok(json!({
            "jobId": "exports:check", "state": "finished",
            "outcome": {"error": {"code": "internal", "message": "The computer is busy."}}
        })),
        "operations.job.get" => Ok(finished(json!({
            "kind": "result", "operation": "backup", "outcome": "success",
            "archive": {"archivePath": "/out/main.silo-backup", "size": "1 GiB"}
        }))),
        _ => Ok(Value::Null),
    });
    let value = export(
        &connect(&fake).await,
        &args(true),
        Path::new("/"),
        &Answers::new(false, false),
        &ignore,
    )
    .await
    .unwrap();
    assert_eq!(value["unchecked"], "The computer is busy.");
    assert_eq!(value["logins"], json!([]));
    let text = silo_cli::render::exported(&value);
    assert!(
        text.contains("could not check it for logins (The computer is busy)"),
        "{text}"
    );
}

#[tokio::test]
async fn a_stopped_computer_that_could_not_be_checked_is_asked_about_like_one_with_logins() {
    let reason = "the computer is stopped, so its files were not listed; start it to check them";
    let fake = FakeEngine::start(move |method, params| match method {
        "computers.list" => Ok(listing(&[("main", "stopped")])),
        "exports.check" => Ok(json!({"jobId": "exports:check"})),
        "exports.create" => Ok(json!({"jobId": "exports:op"})),
        "operations.job.get" if params["jobId"] == "exports:check" => Ok(finished(
            json!({"logins": [], "checked": [], "unchecked": reason}),
        )),
        "operations.job.get" => Ok(finished(json!({
            "kind": "result", "operation": "backup", "outcome": "success",
            "archive": {"archivePath": "/out/main.silo-backup", "size": "1 GiB"}
        }))),
        _ => Ok(Value::Null),
    });
    let asker = Answers::new(true, false);
    let error = export(
        &connect(&fake).await,
        &args(false),
        Path::new("/"),
        &asker,
        &ignore,
    )
    .await
    .unwrap_err();
    assert_eq!(
        asker.asked.borrow().as_slice(),
        [format!(
            "Silo could not check main for logins ({reason}). Share anyway? [y/N]"
        )]
    );
    assert!(error.message.starts_with("Not shared"));
    assert!(fake.calls("exports.create").is_empty());

    let asker = Answers::new(true, true);
    let value = export(
        &connect(&fake).await,
        &args(false),
        Path::new("/"),
        &asker,
        &ignore,
    )
    .await
    .unwrap();
    assert_eq!(value["unchecked"], reason);
    assert_eq!(fake.calls("exports.create").len(), 1);
}

#[tokio::test]
async fn a_checkpoint_is_found_by_name_and_sent_by_id() {
    let fake = engine(json!([]), "success");
    let mut checkpointed = args(false);
    checkpointed.checkpoint = Some("before-upgrade".into());
    export(
        &connect(&fake).await,
        &checkpointed,
        Path::new("/"),
        &Answers::new(false, false),
        &ignore,
    )
    .await
    .unwrap();
    assert_eq!(fake.calls("exports.check")[0]["checkpointId"], "cp-1");
    assert_eq!(fake.calls("exports.create")[0]["checkpointId"], "cp-1");
    checkpointed.checkpoint = Some("nope".into());
    let error = export(
        &connect(&fake).await,
        &checkpointed,
        Path::new("/"),
        &Answers::new(false, false),
        &ignore,
    )
    .await
    .unwrap_err();
    assert!(error.message.contains("no checkpoint nope"));
}

#[tokio::test]
async fn an_export_that_did_not_succeed_is_an_error_with_the_engine_s_reason() {
    let fake = engine(json!([]), "failed");
    let error = export(
        &connect(&fake).await,
        &args(false),
        Path::new("/"),
        &Answers::new(false, false),
        &ignore,
    )
    .await
    .unwrap_err();
    assert_eq!(error.message, "Export failed: The destination is full.");
}

#[tokio::test]
async fn an_import_names_the_computer_after_the_sender_unless_that_name_is_taken() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("main-2026-11-07.silo-backup");
    std::fs::write(&file, "x").unwrap();
    let fake = engine(json!([]), "success");
    let imported = import(&connect(&fake).await, &file, None, Path::new("/"), &ignore).await;
    // `main` exists, so the new computer is `main-imported`; the fake lists no such computer.
    assert!(imported.is_err());
    let sent = &fake.calls("exports.import")[0];
    assert_eq!(sent["newName"], "main-imported");
    assert_eq!(sent["archivePath"], file.to_str().unwrap());
    assert!(sent.get("sourceName").is_none());

    let _ = import(
        &connect(&fake).await,
        &file,
        Some("friend"),
        Path::new("/"),
        &ignore,
    )
    .await;
    assert_eq!(fake.calls("exports.import")[1]["newName"], "friend");

    let missing = import(
        &connect(&fake).await,
        Path::new("gone.silo-backup"),
        None,
        directory.path(),
        &ignore,
    )
    .await
    .unwrap_err();
    assert!(missing.message.contains("is not a file"));
}

#[tokio::test]
async fn an_import_that_did_not_succeed_is_an_error() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("x.silo-backup");
    std::fs::write(&file, "x").unwrap();
    let fake = engine(json!([]), "failed");
    let error = import(
        &connect(&fake).await,
        &file,
        Some("x"),
        Path::new("/"),
        &ignore,
    )
    .await
    .unwrap_err();
    assert_eq!(
        error.message,
        "Import failed: The export file is not valid."
    );
}

const MINI: &str = "mini-0000";
const STAGED: &str = "x-0123456789abcdef0123456789abcdef.silo-backup";
const RECEIVED: &str = "i-0123456789abcdef0123456789abcdef.silo-backup";
const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// An engine connected to a Mac mini whose `main` can be exported into its transfer area and
/// that imports a received file. `copy` is the outcome of the controller's `exports.transfer`.
fn device_engine(copy_fails: bool, import_outcome: &'static str) -> FakeEngine {
    let mut imported = false;
    FakeEngine::start(move |method, params| match method {
        "connections.devices.list" => Ok(json!([
            {"id": MINI, "name": "Mac mini", "address": "ssh://mini"}
        ])),
        "computers.list" if imported => Ok(listing(&[("main", "running"), ("friend", "stopped")])),
        "computers.list" => Ok(listing(&[("main", "running")])),
        "exports.check" => Ok(json!({"jobId": "exports:check"})),
        "exports.create" => Ok(json!({"jobId": "exports:op"})),
        "exports.import" => {
            imported = true;
            Ok(json!({"jobId": "exports:in"}))
        }
        "exports.transfer" => Ok(json!({"jobId": "transfer:copy"})),
        "exports.discard" => Ok(json!(true)),
        "operations.job.get" if params["jobId"] == "exports:check" => {
            Ok(finished(json!({"logins": [], "checked": ["disk"]})))
        }
        "operations.job.get" if params["jobId"] == "exports:op" => Ok(finished(json!({
            "kind": "result", "operation": "backup", "outcome": "success",
            "message": "Computer exported.",
            "archive": {"archivePath": format!("/engine/transfer/{STAGED}"), "size": "1.2 GiB",
                        "transfer": {"name": STAGED, "bytes": 1_288_490_188u64, "sha256": SHA}}
        }))),
        "operations.job.get" if params["jobId"] == "transfer:copy" && copy_fails => Ok(json!({
            "jobId": "transfer:copy", "state": "finished",
            "outcome": {"error": {"code": "internal", "message": "The file stopped arriving."}}
        })),
        "operations.job.get" if params["jobId"] == "transfer:copy" => {
            Ok(finished(if params["deviceId"].is_null() {
                json!({"name": RECEIVED, "bytes": 5, "sha256": SHA})
            } else {
                json!({"name": "wrong-route", "bytes": 0, "sha256": SHA})
            }))
        }
        "operations.job.get" if import_outcome == "unreachable" => {
            Err(("internal", "The other device is not answering.".to_owned()))
        }
        "operations.job.get" => Ok(finished(json!({
            "kind": "result", "operation": "restore", "outcome": import_outcome,
            "message": "The export file is not valid.", "archive": {}
        }))),
        "operations.job.acknowledge" => Ok(Value::Null),
        _ => Ok(Value::Null),
    })
}

fn device_cli(fake: &FakeEngine, words: &[&str]) -> Cli {
    let socket = fake.socket.to_str().unwrap();
    let mut line = vec![
        "silo",
        "--socket",
        socket,
        "--allow-other-channel",
        "--device",
        "Mac mini",
    ];
    line.extend_from_slice(words);
    <Cli as clap::Parser>::try_parse_from(line).unwrap()
}

#[tokio::test]
async fn a_computer_on_another_device_is_exported_there_copied_here_and_removed_there() {
    let directory = tempfile::tempdir().unwrap();
    let out = directory.path().join("main.silo-backup");
    let fake = device_engine(false, "success");
    let code = run(
        device_cli(&fake, &["export", "main", "-o", out.to_str().unwrap()]),
        Path::new("/"),
    )
    .await
    .unwrap();
    assert_eq!(code, 0);
    // The login check is asked of the owner like a local one.
    assert_eq!(fake.calls("exports.check")[0]["deviceId"], MINI);
    let create = &fake.calls("exports.create")[0];
    assert_eq!(create["deviceId"], MINI);
    assert_eq!(create["transferArea"], true);
    assert_eq!(create["destination"], "");
    assert!(create.get("archivePath").is_none());
    // The copy is this engine's own job: it names the owner and never carries a path of the
    // owner's, and its job is followed here.
    let copy = &fake.calls("exports.transfer")[0];
    assert_eq!(copy["deviceId"], MINI);
    assert_eq!(copy["direction"], "receive");
    assert_eq!(copy["path"], out.to_str().unwrap());
    assert_eq!(copy["name"], STAGED);
    assert_eq!(copy["bytes"], 1_288_490_188u64);
    assert_eq!(copy["sha256"], SHA);
    let polled: Vec<Value> = fake.calls("operations.job.get");
    let copy_poll = polled
        .iter()
        .find(|p| p["jobId"] == "transfer:copy")
        .unwrap();
    assert!(copy_poll.get("deviceId").is_none(), "{copy_poll}");
    let create_poll = polled.iter().find(|p| p["jobId"] == "exports:op").unwrap();
    assert_eq!(create_poll["deviceId"], MINI);
    // The owner's copy is removed by name.
    let discard = &fake.calls("exports.discard")[0];
    assert_eq!(
        (discard["name"].as_str(), discard["deviceId"].as_str()),
        (Some(STAGED), Some(MINI))
    );
}

#[tokio::test]
async fn a_copy_that_fails_still_removes_the_other_devices_file_and_says_why() {
    let directory = tempfile::tempdir().unwrap();
    let out = directory.path().join("main.silo-backup");
    let fake = device_engine(true, "success");
    let error = run(
        device_cli(&fake, &["export", "main", "-o", out.to_str().unwrap()]),
        Path::new("/"),
    )
    .await
    .unwrap_err();
    assert!(
        error.message.contains("The file stopped arriving."),
        "{}",
        error.message
    );
    assert_eq!(fake.calls("exports.discard").len(), 1);
}

#[tokio::test]
async fn an_export_file_that_exists_or_cannot_be_named_is_refused_before_anything_starts() {
    let directory = tempfile::tempdir().unwrap();
    let taken = directory.path().join("main.silo-backup");
    std::fs::write(&taken, "x").unwrap();
    let wrong = directory.path().join("main.tar");
    let fake = device_engine(false, "success");
    for path in [&taken, &wrong] {
        let error = run(
            device_cli(&fake, &["export", "main", "-o", path.to_str().unwrap()]),
            Path::new("/"),
        )
        .await
        .unwrap_err();
        assert!(
            error.message.contains("already exists") || error.message.contains("must end in"),
            "{}",
            error.message
        );
    }
    assert!(fake.calls("exports.create").is_empty());
    assert!(fake.calls("exports.transfer").is_empty());
}

#[tokio::test]
async fn a_file_is_sent_to_another_device_imported_there_and_removed_there() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("friend.silo-backup");
    std::fs::write(&file, "x").unwrap();
    let fake = device_engine(false, "success");
    let code = run(
        device_cli(&fake, &["import", file.to_str().unwrap()]),
        Path::new("/"),
    )
    .await
    .unwrap();
    assert_eq!(code, 0);
    let send = &fake.calls("exports.transfer")[0];
    assert_eq!(send["direction"], "send");
    assert_eq!(send["deviceId"], MINI);
    assert_eq!(send["path"], file.to_str().unwrap());
    assert!(send.get("name").is_none() && send.get("sha256").is_none());
    let import = &fake.calls("exports.import")[0];
    assert_eq!(import["deviceId"], MINI);
    assert_eq!(import["transferName"], RECEIVED);
    assert_eq!(import["archivePath"], "");
    assert_eq!(import["newName"], "friend");
    assert_eq!(fake.calls("exports.discard")[0]["name"], RECEIVED);
}

#[tokio::test]
async fn an_import_the_other_device_refuses_still_removes_its_copy_of_the_file() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("friend.silo-backup");
    std::fs::write(&file, "x").unwrap();
    let fake = device_engine(false, "failed");
    let error = run(
        device_cli(&fake, &["import", file.to_str().unwrap()]),
        Path::new("/"),
    )
    .await
    .unwrap_err();
    assert_eq!(
        error.message,
        "Import failed: The export file is not valid."
    );
    assert_eq!(fake.calls("exports.discard")[0]["name"], RECEIVED);
}

#[tokio::test]
async fn an_import_still_queued_on_the_other_device_keeps_its_file() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("friend.silo-backup");
    std::fs::write(&file, "x").unwrap();
    // The wait fails while the import has not been reported finished: it may still run.
    let fake = device_engine(false, "unreachable");
    let error = run(
        device_cli(&fake, &["import", file.to_str().unwrap()]),
        Path::new("/"),
    )
    .await
    .unwrap_err();
    assert!(error.message.contains("not answering"), "{}", error.message);
    assert_eq!(fake.calls("exports.import").len(), 1);
    assert!(fake.calls("exports.discard").is_empty());
}

#[tokio::test]
async fn a_file_that_cannot_be_sent_is_never_imported() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("friend.silo-backup");
    std::fs::write(&file, "x").unwrap();
    let fake = device_engine(true, "success");
    let error = run(
        device_cli(&fake, &["import", file.to_str().unwrap()]),
        Path::new("/"),
    )
    .await
    .unwrap_err();
    assert!(
        error.message.contains("The file stopped arriving."),
        "{}",
        error.message
    );
    assert!(fake.calls("exports.import").is_empty());
}
