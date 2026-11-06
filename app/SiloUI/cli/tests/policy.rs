//! Recipe write-back and `auto_keep` rules against a fake engine.

mod fake_engine;

use std::sync::{Arc, Mutex};

use fake_engine::{finished, listing, FakeEngine};
use serde_json::{json, Value};
use silo_cli::{
    config::{parse_recipe, parse_settings, Project, Settings},
    engine::Engine,
    flows::{self, Context, Mode},
    ops::FoldOptions,
};

fn ignore(_: &str) {}

/// A world with `main` and a fork `try`. `diff` is the diff document the engine returns,
/// `test_exit` the exit code of `guest.exec`, `keep` what the keep job answers.
fn engine(diff: Value, test_exit: i64, keep: Value) -> (FakeEngine, Arc<Mutex<Vec<String>>>) {
    let started = Arc::new(Mutex::new(Vec::new()));
    let seen = started.clone();
    let engine = FakeEngine::start(move |method, params| {
        seen.lock().unwrap().push(method.to_owned());
        match method {
            "computers.list" => Ok(listing(&[("main", "running"), ("try", "running")])),
            "changes.diff" => Ok(json!({"jobId": "job:1"})),
            "changes.keep" => Ok(json!({"jobId": "job:2"})),
            "operations.job.get" if params["jobId"] == "job:1" => Ok(finished(json!({
                "diff": diff.clone(),
                "text": "Code\n  ~/app: a..b",
                "reviewDigest": diff["reviewDigest"],
                "baselineApplied": false, "baselineAvailable": false,
            }))),
            "operations.job.get" => Ok(finished(keep.clone())),
            "operations.job.acknowledge" => Ok(Value::Null),
            "guest.exec" => Ok(json!({
                "exitCode": test_exit, "signal": null, "stdout": [], "stderr": [],
            })),
            "changes.request" => Ok(json!({"request": {
                "id": "fr-1", "forkId": params["forkId"], "target": params["targetId"],
                "createdAt": "t", "reviewDigest": params["reviewDigest"],
                "summary": params["summary"], "status": "open"
            }})),
            other => Err(("internal", format!("unexpected {other}"))),
        }
    });
    (engine, started)
}

fn kept() -> Value {
    json!({
        "status": "kept",
        "report": {"fork": "id-try", "target": "id-main", "summary": [], "steps": [], "notes": [], "checkpoint": "cp1"}
    })
}

fn code_diff() -> Value {
    json!({"format": 1, "code": [{"repo": "/root/app"}], "reviewDigest": "d1"})
}

fn project_with(recipe: &str, settings: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let silo = dir.path().join(".silo");
    std::fs::create_dir_all(silo.join("recipes")).unwrap();
    std::fs::write(silo.join("recipes/web.yaml"), recipe).unwrap();
    std::fs::write(silo.join("silo.yaml"), settings).unwrap();
    dir
}

async fn request(engine: &Engine, project: &Project, agent: Option<&str>) -> Value {
    let settings = project.load_settings(agent).unwrap();
    let recipe = parse_recipe("test: make test\n").unwrap();
    let context = Context {
        project,
        settings: settings.as_ref(),
        recipe: Some(&recipe),
        write_back: true,
    };
    flows::run(
        engine,
        "try",
        None,
        &context,
        FoldOptions::default(),
        Mode::Request,
        &ignore,
    )
    .await
    .unwrap()
}

const RULE: &str = "auto_keep:\n  - when: { only: [code], tests: pass }\n";

#[tokio::test]
async fn a_code_only_change_with_passing_tests_is_kept_without_asking() {
    let (fake, _) = engine(code_diff(), 0, kept());
    let project_dir = project_with("", RULE);
    let project = Project::discover(project_dir.path());
    let connected = Engine::connect(&fake.socket).await.unwrap();
    let value = request(&connected, &project, None).await;
    assert_eq!(value["status"], "kept");
    assert_eq!(
        value["autoKeep"]["rule"],
        "when: { only: [code], tests: pass }"
    );
    assert!(fake.calls("changes.request").is_empty());
    // The keep is pinned to the diff just taken, and the tests ran in the fork.
    assert_eq!(fake.calls("changes.keep")[0]["reviewDigest"], "d1");
    let exec = &fake.calls("guest.exec")[0];
    assert_eq!(exec["argv"], json!(["sh", "-c", "make test"]));
    let text = silo_cli::render::fold(&value);
    assert!(text.contains("Kept without asking"), "{text}");
}

#[tokio::test]
async fn the_tests_of_a_fork_on_another_device_run_on_that_device() {
    const MINI: &str = "mini-0000";
    let (fake, _) = engine(code_diff(), 0, kept());
    let project_dir = project_with("", RULE);
    let project = Project::discover(project_dir.path());
    let connected = Engine::connect(&fake.socket)
        .await
        .unwrap()
        .on_device(Some(MINI.to_owned()));
    let value = request(&connected, &project, None).await;
    assert_eq!(value["status"], "kept", "{value}");
    let exec = &fake.calls("guest.exec")[0];
    assert_eq!(exec["deviceId"], MINI);
    assert_eq!(exec["argv"], json!(["sh", "-c", "make test"]));
    assert_eq!(fake.calls("changes.keep")[0]["deviceId"], MINI);
}

#[tokio::test]
async fn failing_tests_leave_a_review_request() {
    let (fake, _) = engine(code_diff(), 1, kept());
    let project_dir = project_with("", RULE);
    let project = Project::discover(project_dir.path());
    let connected = Engine::connect(&fake.socket).await.unwrap();
    let value = request(&connected, &project, None).await;
    assert_eq!(value["status"], "requested");
    assert_eq!(value["autoKeep"]["declined"], "the tests failed");
    assert!(fake.calls("changes.keep").is_empty());
    assert_eq!(fake.calls("changes.request").len(), 1);
    assert!(silo_cli::render::requested(&value).contains("tests failed"));
}

#[tokio::test]
async fn changes_outside_the_rule_or_left_behind_leave_a_review_request() {
    for diff in [
        json!({"code": [{"repo": "/r"}], "other": [{"path": "/usr/local/bin/x"}], "reviewDigest": "d"}),
        json!({"code": [{"repo": "/r"}], "database": [{"name": "app"}], "reviewDigest": "d"}),
        json!({"code": [{"repo": "/r"}], "not_kept": {"credentials": {"/root/.ssh/": ["/root/.ssh/id"]}}, "reviewDigest": "d"}),
        json!({"code": [{"repo": "/r"}]}),
    ] {
        let (fake, _) = engine(diff, 0, kept());
        let project_dir = project_with("", RULE);
        let project = Project::discover(project_dir.path());
        let connected = Engine::connect(&fake.socket).await.unwrap();
        let value = request(&connected, &project, None).await;
        assert_eq!(value["status"], "requested", "{value}");
        assert!(value["autoKeep"]["declined"].is_string());
        assert!(fake.calls("changes.keep").is_empty());
    }
}

#[tokio::test]
async fn a_keep_the_engine_refuses_leaves_a_review_request() {
    let refused = json!({"status": "refused", "report": {"fork": "id-try", "target": "id-main", "summary": [], "steps": [], "notes": [], "checkpoint": null},
        "refusal": {"reason": "changed", "message": "The fork changed since it was reviewed.", "conflicts": [], "hint": ""}});
    let (fake, _) = engine(code_diff(), 0, refused);
    let project_dir = project_with("", RULE);
    let project = Project::discover(project_dir.path());
    let connected = Engine::connect(&fake.socket).await.unwrap();
    let value = request(&connected, &project, None).await;
    assert_eq!(value["status"], "requested");
    assert!(value["autoKeep"]["declined"]
        .as_str()
        .unwrap()
        .contains("changed since"));
}

#[tokio::test]
async fn without_rules_or_for_an_agent_that_asks_a_request_stays_a_request() {
    let (fake, _) = engine(code_diff(), 0, kept());
    let project_dir = project_with("", "fold: ask\n");
    let project = Project::discover(project_dir.path());
    let connected = Engine::connect(&fake.socket).await.unwrap();
    let value = request(&connected, &project, None).await;
    assert_eq!(value["status"], "requested");
    assert!(value["autoKeep"].is_null());
    assert!(fake.calls("guest.exec").is_empty() && fake.calls("changes.keep").is_empty());

    let both = format!("{RULE}agents:\n  careful: {{ fold: ask }}\n");
    let project_dir = project_with("", &both);
    let project = Project::discover(project_dir.path());
    let value = request(&connected, &project, Some("careful")).await;
    assert_eq!(value["status"], "requested");
    assert!(value["autoKeep"].is_null());
    let value = request(&connected, &project, Some("other")).await;
    assert_eq!(value["status"], "kept");
}

#[tokio::test]
async fn fold_auto_code_stands_for_the_code_only_rule() {
    let (fake, _) = engine(code_diff(), 0, kept());
    let project_dir = project_with("", "fold: auto-code\n");
    let project = Project::discover(project_dir.path());
    let connected = Engine::connect(&fake.socket).await.unwrap();
    let value = request(&connected, &project, None).await;
    assert_eq!(value["status"], "kept");
}

#[tokio::test]
async fn the_end_of_a_fork_command_keeps_or_asks() {
    let (fake, _) = engine(code_diff(), 0, kept());
    let project_dir = project_with("", RULE);
    let project = Project::discover(project_dir.path());
    let settings = project.load_settings(None).unwrap();
    let recipe = parse_recipe("test: make test\n").unwrap();
    let context = Context {
        project: &project,
        settings: settings.as_ref(),
        recipe: Some(&recipe),
        write_back: true,
    };
    let connected = Engine::connect(&fake.socket).await.unwrap();
    let done = flows::after_command(&connected, "try", true, &context, &ignore)
        .await
        .unwrap();
    assert_eq!(done["status"], "kept");
    assert!(silo_cli::render::auto_keep(&done).contains("Kept try without asking"));
    // A command that failed is never kept; it is left for review.
    let failed = flows::after_command(&connected, "try", false, &context, &ignore)
        .await
        .unwrap();
    assert_eq!(failed["status"], "requested");
    assert_eq!(failed["autoKeep"]["declined"], "the command failed");
    assert!(silo_cli::render::auto_keep(&failed).contains("left a review request"));
    assert_eq!(fake.calls("changes.keep").len(), 1);
    // No rules: nothing happens.
    let plain = Context {
        settings: Some(&Settings::default()),
        ..context
    };
    assert!(
        flows::after_command(&connected, "try", true, &plain, &ignore)
            .await
            .is_none()
    );
}

fn packages_diff() -> Value {
    json!({
        "format": 1,
        "packages": [{"manager": "dpkg", "added": {"jq": "1", "libjq1": "1"}, "top_level": ["jq"], "manual": ["jq"]}],
        "settings": {"files": [{"path": "/etc/app.conf", "change": "added"}]},
        "reviewDigest": "d2"
    })
}

async fn fold_with_write_back(
    recipe_file: &str,
    remembered: Option<&str>,
    write_back: bool,
    keep: Value,
) -> (String, Value) {
    let (fake, _) = engine(packages_diff(), 0, keep);
    let project_dir = project_with(recipe_file, "");
    let project = Project::discover(project_dir.path());
    let connected = Engine::connect(&fake.socket).await.unwrap();
    if let Some(name) = remembered {
        flows::remember_recipe(&connected, "id-main", name);
    }
    let context = Context {
        project: &project,
        settings: None,
        recipe: None,
        write_back,
    };
    let value = flows::run(
        &connected,
        "try",
        None,
        &context,
        FoldOptions::default(),
        Mode::Keep,
        &ignore,
    )
    .await
    .unwrap();
    let text = std::fs::read_to_string(project_dir.path().join(".silo/recipes/web.yaml")).unwrap();
    (text, value)
}

#[tokio::test]
async fn a_keep_appends_packages_and_files_to_the_recipe_main_came_from() {
    let (text, value) = fold_with_write_back(
        "# web\npackages:\n  apt: [git]\n",
        Some("web"),
        true,
        kept(),
    )
    .await;
    assert_eq!(
        text,
        "# web\npackages:\n  apt: [git, jq]\nfiles: [/etc/app.conf]\n"
    );
    assert_eq!(value["writeBack"]["written"], true);
    let printed = silo_cli::render::fold(&value);
    assert!(printed.contains("Appended to"), "{printed}");
    assert!(
        printed.contains("apt: jq") && printed.contains("files: /etc/app.conf"),
        "{printed}"
    );
}

#[tokio::test]
async fn no_write_back_a_refused_keep_and_unknown_recipes_change_nothing() {
    let original = "packages:\n  apt: [git]\n";
    let (text, value) = fold_with_write_back(original, Some("web"), false, kept()).await;
    assert_eq!(text, original);
    assert!(value["writeBack"].is_null());

    let refused = json!({"status": "refused", "report": {"fork": "id-try", "target": "id-main", "summary": [], "steps": [], "notes": [], "checkpoint": null},
        "refusal": {"reason": "database", "message": "x", "conflicts": [], "hint": ""}});
    let (text, _) = fold_with_write_back(original, Some("web"), true, refused).await;
    assert_eq!(text, original);

    // main is not known to come from a project recipe: nothing is written, and the output
    // says what to add.
    let (text, value) = fold_with_write_back(original, None, true, kept()).await;
    assert_eq!(text, original);
    assert_eq!(value["writeBack"]["written"], false);
    assert_eq!(value["writeBack"]["packages"]["apt"], json!(["jq"]));
    assert!(silo_cli::render::fold(&value).contains("Add by hand: apt: jq"));
}

#[tokio::test]
async fn an_auto_kept_fork_writes_back_pinned_to_the_kept_digest() {
    let (fake, _) = engine(packages_diff(), 0, kept());
    let project_dir = project_with(
        "packages:\n  apt: [git]\n",
        "auto_keep:\n  - when: { only: [packages, settings] }\n",
    );
    let project = Project::discover(project_dir.path());
    let connected = Engine::connect(&fake.socket).await.unwrap();
    flows::remember_recipe(&connected, "id-main", "web");
    let value = request(&connected, &project, None).await;
    assert_eq!(value["status"], "kept");
    assert_eq!(value["writeBack"]["written"], true);
    assert_eq!(fake.calls("changes.keep")[0]["reviewDigest"], "d2");
    let text = std::fs::read_to_string(project_dir.path().join(".silo/recipes/web.yaml")).unwrap();
    assert_eq!(
        text,
        "packages:\n  apt: [git, jq]\nfiles: [/etc/app.conf]\n"
    );
    let left: Vec<_> = std::fs::read_dir(project_dir.path().join(".silo/recipes"))
        .unwrap()
        .collect();
    assert_eq!(left.len(), 1, "{left:?}");
}

#[tokio::test]
async fn a_recipe_layout_that_cannot_be_edited_is_reported_not_written() {
    let original = "packages: {apt: [git]}\n";
    let (text, value) = fold_with_write_back(original, Some("web"), true, kept()).await;
    assert_eq!(text, original);
    assert_eq!(value["writeBack"]["written"], false);
    let printed = silo_cli::render::fold(&value);
    assert!(printed.contains("Add by hand: apt: jq"), "{printed}");
}
