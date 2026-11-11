mod fake_engine;

use std::sync::{Arc, Mutex};

use fake_engine::{finished, FakeEngine};
use rmcp::ServiceExt;
use serde_json::{json, Value};
use silo_cli::{
    cli::{run, Cli},
    engine::Engine,
    mcp::SiloMcp,
    ops::{self, ForkArgs},
    recipe_file::parse as parse_recipe,
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
    world_engine_with(start, false, false)
}

/// As [`world_engine`]; with `fail_checkpoint`, saving a checkpoint fails. With `ssh`, the
/// engine answers SSH access requests (only for a test that passes its own home directory:
/// `up` through MCP writes the SSH entry under the real one).
fn world_engine_with(
    start: Vec<Computer>,
    fail_checkpoint: bool,
    ssh: bool,
) -> (FakeEngine, World) {
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
            "recipes.list" => Ok(json!({
                "recipes": [{
                    "name": "linux-dev", "hash": "abc123abc123abc123", "imageVersion": "v5",
                    "os": "linux", "computerId": "id-silo-build-linux-dev",
                    "computerName": "silo-build-linux-dev", "checkpointId": "cp-recipe",
                    "builtAt": "2026-11-05T00:00:00Z"
                }],
                "saved": [
                    {"name": "linux-dev", "builtIn": true, "os": "linux", "hash": "abc123abc123abc123", "built": true},
                    {"name": "macos-xcode", "builtIn": true, "os": "macos", "hash": "def", "built": false},
                    {"name": "web", "builtIn": false, "os": "linux", "hash": "fed", "built": false}
                ]
            })),
            "recipes.save" => Ok(json!({
                "name": params["name"], "builtIn": false, "hash": "abc123abc123abc123",
                "recipe": params["recipe"],
                "warnings": ["quiet: cannot tell which process service web runs; name its program or script in quiet too."]
            })),
            "recipes.get" => Ok(json!({
                "name": params["name"], "builtIn": params["name"] == "linux-dev",
                "hash": "abc123abc123abc123",
                "recipe": {"spec": {"os": "linux", "packages": {"apt": ["jq"]}}, "quiet": ["poller"],
                           "keep": {"restart": ["web"], "migrate": [], "after": []}},
                "built": {"name": params["name"]}
            })),
            "recipes.remove" => Ok(Value::Null),
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
                "checkpoint": {"id": "cp-new", "name": "checkpoint", "kind": "manual"}
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
            "sshAccess.state" if ssh => Ok(json!({"computers": [{
                "computer": "main", "computerId": "id-main", "enabled": true, "port": 2222,
                "bindAddress": "127.0.0.1", "keys": [], "state": "listening"
            }]})),
            "sshAccess.connection" if ssh => {
                Ok(json!("ssh -i '/state/ssh/id-main' -p 2222 silo@127.0.0.1"))
            }
            other => Err(("internal", format!("unexpected {other}"))),
        }
    });
    (engine, world)
}

#[tokio::test]
async fn up_makes_main_from_the_recipe_and_saves_no_checkpoint() {
    let (engine, world) = world_engine(vec![]);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let value = ops::up(&connected, "linux-dev", None, &ignore)
        .await
        .unwrap();
    assert_eq!(value["created"], true);
    assert_eq!(value["main"]["state"], "running");
    assert!(value.get("warmCheckpoint").is_none());
    let build = &engine.calls("recipes.build")[0];
    assert_eq!(build["name"], "linux-dev");
    // The engine builds the stored recipe of that name: no spec travels with the request.
    assert!(build.get("spec").is_none() || build["spec"].is_null());
    let fork = &engine.calls("checkpoints.fork")[0];
    assert_eq!(fork["newName"], "main");
    assert_eq!(fork["computerId"], "id-silo-build-linux-dev");
    assert_eq!(fork["checkpointId"], "cp-recipe");
    assert_eq!(engine.calls("computers.start").len(), 1);
    assert!(engine.calls("checkpoints.create").is_empty());
    assert!(world.lock().unwrap().iter().any(|c| c.name == "main"));
}

#[tokio::test]
async fn up_on_an_existing_main_builds_nothing_and_removes_a_warm_checkpoint_of_an_earlier_version()
{
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
    let value = ops::up(&connected, "linux-dev", None, &ignore)
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
    assert_eq!(ids, ["cp-manual"]);
    assert!(engine.calls("checkpoints.create").is_empty());
}

#[tokio::test]
async fn up_starts_a_stopped_main() {
    let (engine, _) = world_engine(vec![computer("main", "stopped", vec![])]);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    ops::up(&connected, "linux-dev", None, &ignore)
        .await
        .unwrap();
    assert_eq!(engine.calls("computers.start")[0]["computerId"], "id-main");
}

#[tokio::test]
async fn up_sets_up_ssh_when_asked() {
    let (engine, _) = world_engine_with(vec![computer("main", "running", vec![])], false, true);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let home = tempfile::tempdir().unwrap();
    let ssh = Some((home.path(), silo_client::channel::Channel::Development));
    let value = ops::up(&connected, "linux-dev", ssh, &ignore)
        .await
        .unwrap();
    assert_eq!(value["ssh"]["host"], "main.silo-dev");
    assert_eq!(engine.calls("sshAccess.connection").len(), 1);
}

fn main_with_checkpoints() -> Computer {
    computer(
        "main",
        "running",
        vec![
            checkpoint("cp-manual", "manual", &ago(30)),
            checkpoint("cp-warm", "warm", &ago(5)),
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
async fn a_fork_of_main_takes_a_new_checkpoint_even_when_a_warm_one_is_young() {
    let (engine, _) = world_engine(vec![main_with_checkpoints()]);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let fork = ops::fork(&connected, &fork_main(), &ignore).await.unwrap();
    assert!(engine.calls("checkpoints.fork")[0]["checkpointId"].is_null());
    assert!(engine.calls("checkpoints.create").is_empty());
    assert_eq!(fork["checkpoint"]["id"], "cp-auto");
    assert_eq!(fork["checkpoint"]["reused"], false);
    assert!(fork["checkpoint"].get("unchanged").is_none());
    assert!(fork["checkpoint"]["ageSeconds"].as_i64().unwrap() <= 5);
    let line = silo_cli::render::forked(&fork);
    assert!(
        line.contains("main as of ") && line.contains("new checkpoint"),
        "{line}"
    );
    assert!(!fork["forkedAsOf"].as_str().unwrap().is_empty());
}

#[tokio::test]
async fn a_named_checkpoint_is_always_used_and_reported() {
    let (engine, _) = world_engine(vec![main_with_checkpoints()]);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let args = ForkArgs {
        checkpoint: Some("cp-manual".into()),
        ..fork_main()
    };
    let fork = ops::fork(&connected, &args, &ignore).await.unwrap();
    assert_eq!(
        engine.calls("checkpoints.fork")[0]["checkpointId"],
        "cp-manual"
    );
    assert_eq!(fork["checkpoint"]["id"], "cp-manual");
}

#[tokio::test]
async fn the_baseline_of_a_recipe_fork_is_measured_from_the_recipes_checkpoint() {
    let (engine, _) = world_engine(vec![]);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let args = ForkArgs {
        name: Some("try".into()),
        recipe_name: Some("linux-dev".into()),
        ..ForkArgs::default()
    };
    ops::fork(&connected, &args, &ignore).await.unwrap();
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
        recipe_name: Some("linux-dev".into()),
        ..ForkArgs::default()
    };
    let fork = ops::fork(&connected, &args, &ignore).await.unwrap();
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
async fn the_recipe_list_marks_which_recipes_are_built() {
    let (engine, _) = world_engine(vec![]);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let listed = ops::recipe_list(&connected).await.unwrap();
    let rows = listed["recipes"].as_array().unwrap();
    let dev = rows.iter().find(|r| r["name"] == "linux-dev").unwrap();
    let xcode = rows.iter().find(|r| r["name"] == "macos-xcode").unwrap();
    assert_eq!(dev["built"], true);
    assert_eq!(xcode["built"], false);
    assert_eq!(xcode["origin"], "built-in");
    let web = rows.iter().find(|r| r["name"] == "web").unwrap();
    assert_eq!(web["origin"], "saved");
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
async fn mcp_offers_up_recipe_list_and_create_from_a_recipe() {
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
    for name in ["up", "recipe_list", "create", "fork"] {
        assert!(tools.iter().any(|t| t["name"] == name), "{name} missing");
    }
    let create = tools.iter().find(|t| t["name"] == "create").unwrap();
    assert!(create["inputSchema"]["properties"]["recipe"].is_object());

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
        json!({"name": "create", "arguments": {"recipe": "linux-dev", "name": "agent-1"}}),
    )
    .await;
    assert_ne!(forked["result"]["isError"], true, "{forked}");
    let sent = engine.calls("checkpoints.fork");
    assert_eq!(sent.last().unwrap()["newName"], "agent-1");
    assert_eq!(sent.last().unwrap()["recipeHash"], "abc123abc123abc123");
}

#[test]
fn a_recipe_can_turn_the_desktop_off_and_leaves_it_on_otherwise() {
    let spec = |text: &str| parse_recipe(text).unwrap().definition().spec;
    assert_eq!(spec("os: linux\n").desktop, None);
    assert_eq!(spec("desktop: false\n").desktop, Some(false));
    assert_eq!(spec("desktop: true\n").desktop, Some(true));
    assert!(parse_recipe("desktop: maybe\n").is_err());
}

fn cli(engine: &FakeEngine, args: &[&str]) -> Cli {
    let socket = engine.socket.to_str().unwrap();
    let mut all = vec!["silo", "--socket", socket, "--allow-other-channel"];
    all.extend_from_slice(args);
    <Cli as clap::Parser>::try_parse_from(all).unwrap()
}

#[tokio::test]
async fn recipe_add_uploads_every_key_of_the_file_under_its_name() {
    let (engine, _) = world_engine(vec![]);
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("web.yaml");
    std::fs::write(
        &file,
        "os: linux\npackages:\n  apt: [nginx]\nsetup: [make deps]\nservices:\n  web: python3 -m http.server\nports: [8000]\nquiet: [poller]\nscrub: [/root/.env]\nkeep:\n  restart: [web]\n  migrate: [make migrate]\n  after: [make check]\ntest: make test\nfiles: [/etc/app.conf]\n",
    )
    .unwrap();
    let args = ["recipe", "add", file.to_str().unwrap()];
    assert_eq!(run(cli(&engine, &args), directory.path()).await.unwrap(), 0);
    let sent = &engine.calls("recipes.save")[0];
    assert_eq!(sent["name"], "web");
    let recipe = &sent["recipe"];
    assert_eq!(recipe["spec"]["packages"]["apt"], json!(["nginx"]));
    assert_eq!(recipe["spec"]["setup"], json!(["make deps"]));
    assert_eq!(recipe["spec"]["services"][0]["name"], "web");
    assert_eq!(recipe["spec"]["ports"], json!([8000]));
    assert_eq!(recipe["quiet"], json!(["poller"]));
    assert_eq!(recipe["scrub"], json!(["/root/.env"]));
    assert_eq!(recipe["keep"]["restart"], json!(["web"]));
    assert_eq!(recipe["keep"]["migrate"], json!(["make migrate"]));
    assert_eq!(recipe["keep"]["after"], json!(["make check"]));
    assert_eq!(recipe["test"], "make test");
    assert_eq!(recipe["files"], json!(["/etc/app.conf"]));
    // --name wins over the file's name.
    let args = ["recipe", "add", file.to_str().unwrap(), "--name", "site"];
    run(cli(&engine, &args), directory.path()).await.unwrap();
    assert_eq!(engine.calls("recipes.save")[1]["name"], "site");
    // A mistake in the file is named before anything is sent.
    std::fs::write(&file, "bogus: 1\n").unwrap();
    let error = run(cli(&engine, &args), directory.path())
        .await
        .unwrap_err();
    assert!(error.message.contains("bogus"), "{}", error.message);
    assert_eq!(engine.calls("recipes.save").len(), 2);
}

#[tokio::test]
async fn recipe_list_show_and_remove_use_the_engines_recipes() {
    let (engine, _) = world_engine(vec![]);
    let here = std::env::temp_dir();
    assert_eq!(
        run(cli(&engine, &["recipe", "list"]), &here).await.unwrap(),
        0
    );
    assert_eq!(
        run(cli(&engine, &["recipe", "ls"]), &here).await.unwrap(),
        0
    );
    assert_eq!(
        run(cli(&engine, &["recipe", "show", "web"]), &here)
            .await
            .unwrap(),
        0
    );
    assert_eq!(engine.calls("recipes.get")[0]["name"], "web");
    assert_eq!(
        run(cli(&engine, &["recipe", "remove", "web"]), &here)
            .await
            .unwrap(),
        0
    );
    run(cli(&engine, &["recipe", "rm", "web"]), &here)
        .await
        .unwrap();
    assert_eq!(engine.calls("recipes.remove").len(), 2);
    let shown = ops::recipe_show(&Engine::connect(&engine.socket).await.unwrap(), "web")
        .await
        .unwrap();
    let text = silo_cli::render::recipe_shown(&shown);
    assert!(text.contains("quiet:") && text.contains("poller"), "{text}");
    assert!(text.contains("restart:") && text.contains("web"), "{text}");
    assert!(text.contains("abc123abc123"), "{text}");
}

#[tokio::test]
async fn fork_from_a_recipe_by_the_command_line_starts_the_new_computer() {
    let (engine, world) = world_engine(vec![]);
    let here = std::env::temp_dir();
    let parsed = cli(&engine, &["fork", "try", "--recipe", "linux-dev"]);
    assert_eq!(run(parsed, &here).await.unwrap(), 0);
    let sent = &engine.calls("checkpoints.fork")[0];
    assert_eq!(sent["newName"], "try");
    assert_eq!(sent["computerId"], "id-silo-build-linux-dev");
    assert_eq!(
        world
            .lock()
            .unwrap()
            .iter()
            .find(|c| c.name == "try")
            .unwrap()
            .state,
        "running"
    );
}

// MARK: The CI runner recipes

/// The recipes `silo-runner` ships parse as recipes, install the runner at the path the helper
/// starts it from, pin a version with a SHA-256, and read the JIT configuration from stdin.
#[test]
fn the_ci_runner_recipes_parse_and_pin_the_runner() {
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../runner/recipes");
    for (name, os, path, archive, digests) in [
        (
            "silo-linux",
            "linux",
            "/opt/actions-runner",
            "actions-runner-linux-${arch}",
            2,
        ),
        (
            "silo-macos",
            "macos",
            "/Users/Shared/actions-runner",
            "actions-runner-osx-arm64",
            1,
        ),
    ] {
        let text = std::fs::read_to_string(directory.join(format!("{name}.yaml"))).unwrap();
        let recipe = parse_recipe(&text).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(format!("{:?}", recipe.os).to_lowercase(), os, "{name}");
        let setup = recipe.setup.join("\n");
        assert!(setup.contains(path), "{name}");
        assert!(setup.contains(archive), "{name}");
        assert!(setup.contains(&format!("{path}/silo-run-jit")) || setup.contains("silo-run-jit"));
        assert!(
            setup.contains("IFS= read -r ACTIONS_RUNNER_INPUT_JITCONFIG"),
            "{name}"
        );
        let version = regex::Regex::new(r"(?m)^\s*RUNNER_VERSION=\d+\.\d+\.\d+$").unwrap();
        let sha = regex::Regex::new(r"(?m)^\s*RUNNER_SHA256(_ARM64|_X64)?=[0-9a-f]{64}$").unwrap();
        assert_eq!(version.find_iter(&setup).count(), 1, "{name}");
        assert_eq!(sha.find_iter(&setup).count(), digests, "{name}");
        assert!(!setup.contains("--jitconfig"), "{name}");
    }
}
