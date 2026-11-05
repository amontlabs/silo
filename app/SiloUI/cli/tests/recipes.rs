mod fake_engine;

use std::sync::{Arc, Mutex};

use fake_engine::{finished, FakeEngine};
use rmcp::ServiceExt;
use serde_json::{json, Value};
use silo_cli::{
    config::{parse_recipe, Project},
    engine::Engine,
    mcp::SiloMcp,
    ops::{self, ForkArgs},
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

fn ignore(_: &str) {}

#[derive(Clone)]
struct Computer {
    name: String,
    state: String,
    checkpoints: Vec<Value>,
}

type World = Arc<Mutex<Vec<Computer>>>;

fn computer(name: &str, state: &str, checkpoints: Vec<Value>) -> Computer {
    Computer {
        name: name.into(),
        state: state.into(),
        checkpoints,
    }
}

fn checkpoint(id: &str, kind: &str, at: &str) -> Value {
    json!({"id": id, "name": id, "kind": kind, "createdAt": at})
}

/// `seconds` ago, as the engine writes times.
fn ago(seconds: i64) -> String {
    silo_cli::clock::format_rfc3339_utc(silo_cli::clock::now_unix() - seconds)
}

/// An engine that keeps its computers: forks and starts change them and `recipes.build` makes
/// the hidden builder. A checkpoint taken for a fork without one is added to the source.
fn world_engine(start: Vec<Computer>) -> (FakeEngine, World) {
    world_engine_with(start, false)
}

/// As [`world_engine`]; with `fail_checkpoint`, saving a checkpoint fails.
fn world_engine_with(start: Vec<Computer>, fail_checkpoint: bool) -> (FakeEngine, World) {
    let world: World = Arc::new(Mutex::new(start));
    let shared = world.clone();
    let engine = FakeEngine::start(move |method, params| {
        let mut world = shared.lock().unwrap();
        match method {
            "computers.list" => {
                let linux: Vec<Value> = world
                    .iter()
                    .map(|c| {
                        json!({
                            "configuration": {
                                "id": format!("id-{}", c.name), "name": c.name, "cpus": 2,
                                "maxCPUs": 2, "memoryGiB": 4, "maxMemoryGiB": 4,
                                "workspaceStorageGiB": 10, "runtimeStorageGiB": 10
                            },
                            "state": c.state, "stateDetail": "",
                            "checkpoints": c.checkpoints,
                        })
                    })
                    .collect();
                Ok(json!({"linux": {"computers": linux}, "macos": {"computers": []}}))
            }
            "recipes.build" => {
                if !world.iter().any(|c| c.name == "silo-build-linux-dev") {
                    world.push(computer(
                        "silo-build-linux-dev",
                        "stopped",
                        vec![checkpoint("cp-recipe", "manual", "t0")],
                    ));
                }
                Ok(json!({"jobId": "job:2"}))
            }
            "operations.job.get" if params["jobId"] == "job:2" => Ok(finished(json!({
                "built": true,
                "recipe": {
                    "name": "linux-dev", "hash": "abc123abc123abc123",
                    "imageVersion": "v5", "os": "linux",
                    "computerId": "id-silo-build-linux-dev",
                    "computerName": "silo-build-linux-dev",
                    "checkpointId": "cp-recipe", "builtAt": "2026-11-05T00:00:00Z"
                }
            }))),
            "recipes.list" => Ok(json!({"recipes": [{
                "name": "linux-dev", "hash": "abc123abc123abc123", "imageVersion": "v5", "os": "linux",
                "computerId": "id-silo-build-linux-dev", "computerName": "silo-build-linux-dev",
                "checkpointId": "cp-recipe", "builtAt": "2026-11-05T00:00:00Z"
            }]})),
            "checkpoints.fork" => {
                if params["checkpointId"].is_null() {
                    let source = params["computerId"].as_str().unwrap().to_owned();
                    for c in world
                        .iter_mut()
                        .filter(|c| format!("id-{}", c.name) == source)
                    {
                        c.checkpoints
                            .push(checkpoint("cp-auto", "automatic", &ago(0)));
                    }
                }
                world.push(computer(
                    params["newName"].as_str().unwrap(),
                    "stopped",
                    vec![],
                ));
                Ok(json!({"jobId": "job:1"}))
            }
            "computers.start" => {
                let id = params["computerId"].as_str().unwrap();
                for c in world.iter_mut().filter(|c| format!("id-{}", c.name) == id) {
                    c.state = "running".into();
                }
                Ok(json!({"jobId": "job:1"}))
            }
            "checkpoints.create" => {
                let kind = params["kind"].as_str().unwrap_or("manual").to_owned();
                if !fail_checkpoint {
                    for c in world.iter_mut().filter(|c| c.name == "main") {
                        c.checkpoints.push(checkpoint("cp-new", &kind, &ago(0)));
                    }
                }
                Ok(json!({"jobId": "job:3"}))
            }
            "operations.job.get" if params["jobId"] == "job:3" && fail_checkpoint => Ok(json!({
                "jobId": "job:3", "state": "finished",
                "outcome": {"error": {"code": "internal", "message": "No room for a checkpoint."}}
            })),
            "operations.job.get" if params["jobId"] == "job:3" => Ok(finished(json!({
                "checkpoint": {"id": "cp-new", "name": "silo-warm", "kind": "warm"}
            }))),
            "changes.diff" => Ok(json!({"jobId": "job:4"})),
            "changes.baseline" => Ok(json!({"jobId": "job:5"})),
            "operations.job.get" if params["jobId"] == "job:4" => Ok(finished(json!({
                "diff": {"format": 1, "other": []}, "text": "Code",
                "baselineApplied": false, "baselineAvailable": false,
                "baseSource": "recipe:abc123abc123abc123", "elapsedSeconds": 12,
                "baseCapturedAt": "a", "capturedAt": "b"
            }))),
            "operations.job.get" if params["jobId"] == "job:5" => Ok(finished(json!({
                "source": "recipe:abc123abc123abc123", "idleSeconds": 12, "capturedAt": "t"
            }))),
            "checkpoints.delete" => {
                let gone = params["checkpointId"].as_str().unwrap().to_owned();
                for c in world.iter_mut() {
                    c.checkpoints.retain(|cp| cp["id"] != gone.as_str());
                }
                Ok(json!({"jobId": "job:1"}))
            }
            "operations.job.get" => Ok(finished(Value::Null)),
            "operations.job.acknowledge" => Ok(Value::Null),
            "computers.stop" => {
                let id = params["computerId"].as_str().unwrap();
                for c in world.iter_mut().filter(|c| format!("id-{}", c.name) == id) {
                    c.state = "stopped".into();
                }
                Ok(json!({"jobId": "job:1"}))
            }
            "computers.delete" => {
                let id = params["computerId"].as_str().unwrap().to_owned();
                world.retain(|c| format!("id-{}", c.name) != id);
                Ok(json!({"jobId": "job:1"}))
            }
            other => Err(("internal", format!("unexpected {other}"))),
        }
    });
    (engine, world)
}

fn linux_dev() -> silo_cli::config::Recipe {
    Project::default().named_recipe("linux-dev").unwrap()
}

#[tokio::test]
async fn up_makes_main_from_the_recipe_and_keeps_a_warm_checkpoint() {
    let (engine, world) = world_engine(vec![]);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let value = ops::up(&connected, "linux-dev", &linux_dev(), &ignore)
        .await
        .unwrap();
    assert_eq!(value["created"], true);
    assert_eq!(value["main"]["state"], "running");
    assert_eq!(value["warmCheckpoint"], "cp-new");
    let build = &engine.calls("recipes.build")[0];
    assert_eq!(build["name"], "linux-dev");
    assert_eq!(build["spec"]["packages"]["apt"][0], "build-essential");
    let fork = &engine.calls("checkpoints.fork")[0];
    assert_eq!(fork["newName"], "main");
    assert_eq!(fork["computerId"], "id-silo-build-linux-dev");
    assert_eq!(fork["checkpointId"], "cp-recipe");
    assert_eq!(engine.calls("computers.start").len(), 1);
    assert_eq!(engine.calls("checkpoints.create")[0]["kind"], "warm");
    assert!(world.lock().unwrap().iter().any(|c| c.name == "main"));
}

#[tokio::test]
async fn up_on_an_existing_main_builds_nothing_and_replaces_the_warm_checkpoint() {
    let main = computer(
        "main",
        "running",
        vec![
            checkpoint("cp-manual", "manual", "t1"),
            checkpoint("cp-old-warm", "warm", "t2"),
        ],
    );
    let (engine, world) = world_engine(vec![main]);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let value = ops::up(&connected, "linux-dev", &linux_dev(), &ignore)
        .await
        .unwrap();
    assert_eq!(value["created"], false);
    assert!(engine.calls("recipes.build").is_empty());
    assert!(engine.calls("checkpoints.fork").is_empty());
    assert!(engine.calls("computers.start").is_empty());
    assert_eq!(
        engine.calls("checkpoints.delete")[0]["checkpointId"],
        "cp-old-warm"
    );
    let ids: Vec<String> = world.lock().unwrap()[0]
        .checkpoints
        .iter()
        .map(|cp| cp["id"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(ids, ["cp-manual", "cp-new"]);
}

#[tokio::test]
async fn up_starts_a_stopped_main() {
    let (engine, _) = world_engine(vec![computer("main", "stopped", vec![])]);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    ops::up(&connected, "linux-dev", &linux_dev(), &ignore)
        .await
        .unwrap();
    assert_eq!(engine.calls("computers.start")[0]["computerId"], "id-main");
}

fn main_with_warm(age_seconds: i64) -> Computer {
    computer(
        "main",
        "running",
        vec![
            checkpoint("cp-manual", "manual", &ago(30)),
            checkpoint("cp-warm", "warm", &ago(age_seconds)),
        ],
    )
}

fn fork_main() -> ForkArgs {
    ForkArgs {
        source: "main".into(),
        ..ForkArgs::default()
    }
}

#[tokio::test]
async fn a_fork_of_main_starts_from_a_warm_checkpoint_taken_in_the_last_five_minutes() {
    let (engine, _) = world_engine(vec![main_with_warm(120)]);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let fork = ops::fork(&connected, &fork_main(), None, &ignore)
        .await
        .unwrap();
    assert_eq!(
        engine.calls("checkpoints.fork")[0]["checkpointId"],
        "cp-warm"
    );
    assert_eq!(fork["checkpoint"]["id"], "cp-warm");
    assert_eq!(fork["checkpoint"]["reused"], true);
    let age = fork["checkpoint"]["ageSeconds"].as_i64().unwrap();
    assert!((120..=125).contains(&age), "{age}");
    let line = silo_cli::render::forked(&fork);
    assert!(
        line.contains("main as of ") && line.contains("warm checkpoint, 2m"),
        "{line}"
    );
}

#[tokio::test]
async fn an_older_warm_checkpoint_is_not_used_and_the_fork_reports_the_new_one() {
    let (engine, _) = world_engine(vec![main_with_warm(600)]);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let fork = ops::fork(&connected, &fork_main(), None, &ignore)
        .await
        .unwrap();
    assert!(engine.calls("checkpoints.fork")[0]["checkpointId"].is_null());
    assert_eq!(fork["checkpoint"]["id"], "cp-auto");
    assert_eq!(fork["checkpoint"]["reused"], false);
    assert!(fork["checkpoint"]["ageSeconds"].as_i64().unwrap() <= 5);
    let line = silo_cli::render::forked(&fork);
    assert!(line.contains("new checkpoint"), "{line}");
    assert!(!fork["forkedAsOf"].as_str().unwrap().is_empty());
}

#[tokio::test]
async fn warm_max_age_in_the_settings_changes_how_old_a_warm_checkpoint_may_be() {
    let settings = silo_cli::config::parse_settings("warm_max_age: 15m\n").unwrap();
    let (engine, _) = world_engine(vec![main_with_warm(600)]);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    ops::fork(&connected, &fork_main(), Some(&settings), &ignore)
        .await
        .unwrap();
    assert_eq!(
        engine.calls("checkpoints.fork")[0]["checkpointId"],
        "cp-warm"
    );
    let strict = silo_cli::config::parse_settings("warm_max_age: 30s\n").unwrap();
    let (engine, _) = world_engine(vec![main_with_warm(120)]);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    ops::fork(&connected, &fork_main(), Some(&strict), &ignore)
        .await
        .unwrap();
    assert!(engine.calls("checkpoints.fork")[0]["checkpointId"].is_null());
}

#[tokio::test]
async fn a_named_checkpoint_is_always_used_and_reported() {
    let (engine, _) = world_engine(vec![main_with_warm(120)]);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let args = ForkArgs {
        checkpoint: Some("cp-manual".into()),
        ..fork_main()
    };
    let fork = ops::fork(&connected, &args, None, &ignore).await.unwrap();
    assert_eq!(
        engine.calls("checkpoints.fork")[0]["checkpointId"],
        "cp-manual"
    );
    assert_eq!(fork["checkpoint"]["id"], "cp-manual");
}

#[test]
fn only_the_newest_warm_checkpoint_counts() {
    let now = silo_cli::clock::now_unix();
    let at = |seconds: i64| silo_cli::clock::format_rfc3339_utc(now - seconds);
    let list = vec![
        json!({"id": "old", "kind": "warm", "createdAt": at(1000)}),
        json!({"id": "manual", "kind": "manual", "createdAt": at(5)}),
        json!({"id": "young", "kind": "warm", "createdAt": at(100)}),
        json!({"id": "broken", "kind": "warm", "createdAt": "yesterday"}),
    ];
    assert_eq!(ops::fresh_warm(&list, now, 300).unwrap()["id"], "young");
    assert!(ops::fresh_warm(&list, now, 50).is_none());
    assert!(ops::fresh_warm(&[], now, 300).is_none());
}

#[tokio::test]
async fn a_failed_up_keeps_the_previous_warm_checkpoint() {
    let main = computer(
        "main",
        "running",
        vec![checkpoint("cp-old-warm", "warm", &ago(100))],
    );
    let (engine, world) = world_engine_with(vec![main], true);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let error = ops::up(&connected, "linux-dev", &linux_dev(), &ignore)
        .await
        .unwrap_err();
    assert!(error.message.contains("No room"), "{}", error.message);
    assert!(engine.calls("checkpoints.delete").is_empty());
    assert_eq!(world.lock().unwrap()[0].checkpoints.len(), 1);
}

#[tokio::test]
async fn the_baseline_of_a_recipe_fork_is_measured_from_the_recipes_checkpoint() {
    let (engine, _) = world_engine(vec![]);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let args = ForkArgs {
        name: Some("try".into()),
        recipe: Some(linux_dev()),
        recipe_name: Some("linux-dev".into()),
        ..ForkArgs::default()
    };
    ops::fork(&connected, &args, None, &ignore).await.unwrap();
    let diff = ops::DiffArgs {
        computer: "try".into(),
        max_lines: None,
        no_baseline: false,
        compute_baseline: true,
    };
    let value = ops::diff(&connected, &diff, &ignore).await.unwrap();
    assert!(value["baselineError"].is_null(), "{value}");
    let copy = &engine.calls("checkpoints.fork")[1];
    assert!(copy["newName"]
        .as_str()
        .unwrap()
        .starts_with(ops::BASELINE_PREFIX));
    assert_eq!(copy["computerId"], "id-silo-build-linux-dev");
    assert_eq!(copy["checkpointId"], "cp-recipe");
    assert_eq!(copy["recipeHash"], "abc123abc123abc123");
    let measured = &engine.calls("changes.baseline")[0];
    assert_eq!(measured["source"], "recipe:abc123abc123abc123");
}

#[tokio::test]
async fn fork_from_a_recipe_builds_first_and_keys_the_baseline_by_its_hash() {
    let (engine, world) = world_engine(vec![]);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let args = ForkArgs {
        name: Some("try".into()),
        recipe: Some(linux_dev()),
        recipe_name: Some("linux-dev".into()),
        ..ForkArgs::default()
    };
    let fork = ops::fork(&connected, &args, None, &ignore).await.unwrap();
    assert_eq!(fork["name"], "try");
    assert_eq!(fork["source"], "linux-dev");
    let sent = &engine.calls("checkpoints.fork")[0];
    assert_eq!(sent["computerId"], "id-silo-build-linux-dev");
    assert_eq!(sent["checkpointId"], "cp-recipe");
    assert_eq!(sent["recipeHash"], "abc123abc123abc123");
    assert_eq!(engine.calls("recipes.build").len(), 1);
    assert!(world.lock().unwrap().iter().any(|c| c.name == "try"));

    // The hidden builder never shows in the list.
    let listed = ops::list(&connected).await.unwrap();
    let names: Vec<&str> = listed["computers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["try"]);
}

#[tokio::test]
async fn settings_can_limit_the_recipes_a_fork_may_use() {
    let (engine, _) = world_engine(vec![]);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let settings = silo_cli::config::parse_settings("recipes: [macos-xcode]\n").unwrap();
    let args = ForkArgs {
        recipe: Some(linux_dev()),
        recipe_name: Some("linux-dev".into()),
        ..ForkArgs::default()
    };
    let error = ops::fork(&connected, &args, Some(&settings), &ignore)
        .await
        .unwrap_err();
    assert!(error.message.contains("linux-dev"));
    assert!(engine.calls("recipes.build").is_empty());
}

#[tokio::test]
async fn the_recipe_list_marks_which_recipes_are_built() {
    let (engine, _) = world_engine(vec![]);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let listed = ops::recipe_list(&connected, &Project::default())
        .await
        .unwrap();
    let rows = listed["recipes"].as_array().unwrap();
    let dev = rows.iter().find(|r| r["name"] == "linux-dev").unwrap();
    let xcode = rows.iter().find(|r| r["name"] == "macos-xcode").unwrap();
    assert_eq!(dev["built"], true);
    assert_eq!(xcode["built"], false);
    assert_eq!(xcode["origin"], "built-in");
    let recipe = parse_recipe("os: linux\n").unwrap();
    assert_eq!(recipe.spec().ports.len(), 0);
}

async fn call(
    writer: &mut (impl AsyncWriteExt + Unpin),
    reader: &mut (impl AsyncBufReadExt + Unpin),
    id: u64,
    method: &str,
    params: Value,
) -> Value {
    let request = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
    writer
        .write_all(format!("{request}\n").as_bytes())
        .await
        .unwrap();
    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    serde_json::from_str(&line).unwrap()
}

#[tokio::test]
async fn mcp_offers_up_recipe_list_and_fork_with_a_recipe() {
    let (engine, _) = world_engine(vec![]);
    let (server_side, client_side) = tokio::io::duplex(64 * 1024);
    let (server_read, server_write) = tokio::io::split(server_side);
    let server = SiloMcp::new(engine.socket.clone(), std::env::temp_dir());
    tokio::spawn(async move {
        let running = server.serve((server_read, server_write)).await.unwrap();
        let _ = running.waiting().await;
    });
    let (read, mut write) = tokio::io::split(client_side);
    let mut read = BufReader::new(read);
    call(
        &mut write,
        &mut read,
        1,
        "initialize",
        json!({"protocolVersion": "2025-06-18", "capabilities": {},
               "clientInfo": {"name": "test", "version": "0"}}),
    )
    .await;
    write
        .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n")
        .await
        .unwrap();

    let tools = call(&mut write, &mut read, 2, "tools/list", json!({})).await;
    let tools = tools["result"]["tools"].as_array().unwrap().clone();
    for name in ["up", "recipe_list", "fork"] {
        assert!(tools.iter().any(|t| t["name"] == name), "{name} missing");
    }
    let fork = tools.iter().find(|t| t["name"] == "fork").unwrap();
    assert!(fork["inputSchema"]["properties"]["recipe"].is_object());

    let recipes = call(
        &mut write,
        &mut read,
        3,
        "tools/call",
        json!({"name": "recipe_list", "arguments": {}}),
    )
    .await;
    let text = recipes["result"]["content"][0]["text"].as_str().unwrap();
    let listed: Value = serde_json::from_str(text).unwrap();
    assert!(listed["recipes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["name"] == "linux-dev" && r["built"] == true));

    let up = call(
        &mut write,
        &mut read,
        4,
        "tools/call",
        json!({"name": "up", "arguments": {}}),
    )
    .await;
    assert_ne!(up["result"]["isError"], true, "{up}");
    let text = up["result"]["content"][0]["text"].as_str().unwrap();
    let value: Value = serde_json::from_str(text).unwrap();
    assert_eq!(value["created"], true);

    let forked = call(
        &mut write,
        &mut read,
        5,
        "tools/call",
        json!({"name": "fork", "arguments": {"recipe": "linux-dev", "name": "agent-1"}}),
    )
    .await;
    assert_ne!(forked["result"]["isError"], true, "{forked}");
    let sent = engine.calls("checkpoints.fork");
    assert_eq!(sent.last().unwrap()["newName"], "agent-1");
    assert_eq!(sent.last().unwrap()["recipeHash"], "abc123abc123abc123");
}
