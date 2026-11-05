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

/// An engine that keeps its computers: forks and starts change them, `recipes.build` makes the
/// hidden builder, and `guest.exec` answers `unchanged` when `unchanged` is set.
fn world_engine(start: Vec<Computer>, unchanged: bool) -> (FakeEngine, World) {
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
                "name": "linux-dev", "hash": "abc", "imageVersion": "v5", "os": "linux",
                "computerId": "x", "computerName": "silo-build-linux-dev",
                "checkpointId": "cp-recipe", "builtAt": "2026-11-05T00:00:00Z"
            }]})),
            "checkpoints.fork" => {
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
                for c in world.iter_mut().filter(|c| c.name == "main") {
                    c.checkpoints.push(checkpoint("cp-new", &kind, "t9"));
                }
                Ok(json!({"jobId": "job:3"}))
            }
            "operations.job.get" if params["jobId"] == "job:3" => Ok(finished(json!({
                "checkpoint": {"id": "cp-new", "name": "silo-warm", "kind": "warm"}
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
            "guest.exec" => {
                let script = params["argv"][2].as_str().unwrap_or_default();
                let out = if script.starts_with("m=") && unchanged {
                    "unchanged\n"
                } else {
                    ""
                };
                Ok(json!({
                    "exitCode": 0, "signal": null,
                    "stdout": out.bytes().collect::<Vec<u8>>(), "stderr": []
                }))
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
    let (engine, world) = world_engine(vec![], false);
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
    let (engine, world) = world_engine(vec![main], false);
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
    let (engine, _) = world_engine(vec![computer("main", "stopped", vec![])], false);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    ops::up(&connected, "linux-dev", &linux_dev(), &ignore)
        .await
        .unwrap();
    assert_eq!(engine.calls("computers.start")[0]["computerId"], "id-main");
}

#[tokio::test]
async fn a_fork_of_an_unchanged_main_starts_from_the_warm_checkpoint() {
    let main = computer(
        "main",
        "running",
        vec![
            checkpoint("cp-manual", "manual", "t1"),
            checkpoint("cp-warm", "warm", "t2"),
        ],
    );
    let (engine, _) = world_engine(vec![main.clone()], true);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let args = ForkArgs {
        source: "main".into(),
        ..ForkArgs::default()
    };
    ops::fork(&connected, &args, None, &ignore).await.unwrap();
    assert_eq!(
        engine.calls("checkpoints.fork")[0]["checkpointId"],
        "cp-warm"
    );

    // Main changed since the warm checkpoint: the engine takes a fresh one.
    let (engine, _) = world_engine(vec![main], false);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    ops::fork(&connected, &args, None, &ignore).await.unwrap();
    assert!(engine.calls("checkpoints.fork")[0]["checkpointId"].is_null());
}

#[tokio::test]
async fn fork_from_a_recipe_builds_first_and_keys_the_baseline_by_its_hash() {
    let (engine, world) = world_engine(vec![], false);
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
    let (engine, _) = world_engine(vec![], false);
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
    let (engine, _) = world_engine(vec![], false);
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
    let (engine, _) = world_engine(vec![], false);
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
