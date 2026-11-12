mod fake_engine;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use fake_engine::{finished, FakeEngine};
use serde_json::{json, Value};
use silo_cli::{
    engine::{Computer, Engine},
    fanout::{self, check_capacity, fork_names, ManyArgs, Prefixer},
    ops::ForkArgs,
};

fn ignore(_: &str) {}

fn computer(name: &str, kind: &'static str, state: &str, memory_gib: u64) -> Computer {
    Computer {
        id: format!("id-{name}"),
        name: name.into(),
        kind,
        state: state.into(),
        detail: None,
        cpus: 2,
        memory_gib,
        disk_gib: Some(10),
        checkpoints: Vec::new(),
        configuration: None,
        quiet: None,
        boots: None,
    }
}

// MARK: Budget and slots

#[test]
fn computers_other_than_macos_have_no_count_to_stay_under() {
    let source = computer("main", "linux", "running", 2);
    let all: Vec<Computer> = std::iter::once(source.clone())
        .chain((0..40).map(|n| computer(&format!("f{n}"), "linux", "running", 8)))
        .collect();
    assert!(check_capacity(&source, &all, 32).is_ok());
}

#[test]
fn macos_forks_are_limited_to_the_free_virtual_machine_slots() {
    let source = computer("mac", "macos", "running", 8);
    let one_more = vec![source.clone(), computer("lin", "linux", "running", 4)];
    // The source itself runs, so one slot is free.
    assert!(check_capacity(&source, &one_more, 1).is_ok());
    let error = check_capacity(&source, &one_more, 2).unwrap_err().message;
    assert!(error.contains("1 already run"), "{error}");
    assert!(error.contains("Linux"), "{error}");
    let stopped = computer("mac", "macos", "stopped", 8);
    assert!(check_capacity(&stopped, &[stopped.clone()], 2).is_ok());
    assert!(check_capacity(&stopped, &[stopped.clone()], 3).is_err());
}

#[test]
fn names_are_one_name_a_numbered_prefix_or_a_short_id_each_and_never_taken() {
    let existing = vec![computer("taken", "linux", "stopped", 2)];
    let mut counter = 0;
    let mut next = || {
        counter += 1;
        format!("id{counter}")
    };
    assert_eq!(
        fork_names("main", Some("try"), 1, &existing, &mut next).unwrap(),
        ["try"]
    );
    assert_eq!(
        fork_names("main", Some("try"), 3, &existing, &mut next).unwrap(),
        ["try-1", "try-2", "try-3"]
    );
    assert_eq!(
        fork_names("main", None, 2, &existing, &mut next).unwrap(),
        ["main-id1", "main-id2"]
    );
    assert!(fork_names("main", Some("taken"), 1, &existing, &mut next).is_err());
}

#[test]
fn the_default_count_is_one_and_the_most_is_bounded() {
    assert_eq!(ManyArgs::default().count, 0);
    assert!(fanout::MAX_FORKS >= 8);
}

#[test]
fn lines_of_several_forks_are_prefixed_and_partial_lines_wait() {
    let mut prefixer = Prefixer::default();
    assert_eq!(prefixer.push("a", false, b"one\ntw"), b"[a] one\n");
    assert_eq!(prefixer.push("b", false, b"x\n"), b"[b] x\n");
    assert_eq!(prefixer.push("a", false, b"o\n"), b"[a] two\n");
    prefixer.push("a", true, b"tail");
    assert_eq!(prefixer.finish(), vec![(true, b"[a] tail\n".to_vec())]);
}

// MARK: The fork engine

#[derive(Default)]
struct World {
    computers: Vec<(String, String)>,
    /// Whether main lists a warm checkpoint left by an earlier version.
    warm: bool,
    forks_seen: usize,
    exec: HashMap<String, (i32, &'static str)>,
}

/// An engine that keeps its computers. A fork job only finishes once `expected` fork requests
/// have arrived, so forks made one after another would never finish.
fn engine(expected: usize, exec: &[(&str, i32, &'static str)]) -> (FakeEngine, Arc<Mutex<World>>) {
    let world = Arc::new(Mutex::new(World {
        computers: vec![("main".into(), "running".into())],
        exec: exec
            .iter()
            .map(|(name, code, out)| ((*name).to_owned(), (*code, *out)))
            .collect(),
        ..World::default()
    }));
    let shared = world.clone();
    let engine = FakeEngine::start(move |method, params| {
        let mut world = shared.lock().unwrap();
        match method {
            "computers.list" => {
                let linux: Vec<Value> = world
                    .computers
                    .iter()
                    .map(|(name, state)| {
                        let checkpoints = if name == "main" && world.warm {
                            json!([{"id": "cp-warm", "name": "silo-warm", "kind": "warm",
                                "createdAt": silo_cli::clock::format_rfc3339_utc(silo_cli::clock::now_unix() - 30)}])
                        } else {
                            json!([])
                        };
                        json!({
                            "configuration": {
                                "id": format!("id-{name}"), "name": name, "cpus": 2, "maxCPUs": 2,
                                "memoryGiB": 2, "maxMemoryGiB": 2,
                                "workspaceStorageGiB": 10, "runtimeStorageGiB": 10
                            },
                            "state": state, "stateDetail": "", "checkpoints": checkpoints,
                        })
                    })
                    .collect();
                Ok(json!({"linux": {"computers": linux}, "macos": {"computers": []}}))
            }
            "checkpoints.fork" => {
                world.forks_seen += 1;
                world.computers.push((
                    params["newName"].as_str().unwrap().to_owned(),
                    "stopped".into(),
                ));
                Ok(json!({"jobId": "job:11"}))
            }
            "operations.job.get" if params["jobId"] == "job:11" => {
                if world.forks_seen >= expected {
                    Ok(finished(Value::Null))
                } else {
                    Ok(json!({"jobId": "job:11", "state": "running"}))
                }
            }
            "computers.start" => {
                let id = params["computerId"].as_str().unwrap();
                for (name, state) in world.computers.iter_mut() {
                    if format!("id-{name}") == id {
                        *state = "running".into();
                    }
                }
                Ok(json!({"jobId": "job:1"}))
            }
            "checkpoints.create" => Ok(json!({"jobId": "job:12"})),
            "operations.job.get" if params["jobId"] == "job:12" => Ok(finished(
                json!({"checkpoint": {"id": "cp-shared", "name": "silo-fork", "kind": "manual"}}),
            )),
            "computers.access" => Ok(json!({
                "computerId": params["computerId"], "name": "main", "secrets": [],
                "ports": if params["computerId"] == "id-main" {
                    json!([{"port": 3000, "scheme": "http", "configured": true}])
                } else {
                    json!([])
                }
            })),
            "network.publish" => Ok(json!({})),
            "network.urls" => {
                let id = params["computerId"].as_str().unwrap();
                Ok(json!({"computerId": id, "urls": [{
                    "port": 3000, "host": format!("3000.{id}.silo.localhost"),
                    "url": format!("http://3000.{}.silo.localhost:47383", id.trim_start_matches("id-"))
                }]}))
            }
            "guest.exec.start" => {
                let id = params["computerId"]
                    .as_str()
                    .unwrap()
                    .trim_start_matches("id-");
                let index = world
                    .computers
                    .iter()
                    .position(|(name, _)| name == id)
                    .unwrap();
                Ok(json!({"jobId": format!("job:{}", 100 + index)}))
            }
            "guest.exec.get" => {
                let name = exec_name(&world, params["jobId"].as_str().unwrap());
                let (_, out) = world.exec.get(&name).copied().unwrap_or((0, ""));
                let since = params["sinceSeq"].as_u64().unwrap_or(0);
                let output = if since == 0 {
                    json!([{"seq": 1, "stream": "stdout", "data": out.bytes().collect::<Vec<u8>>()},
                           {"seq": 2, "stream": "stderr", "data": b"warn\n".to_vec()}])
                } else {
                    json!([])
                };
                Ok(
                    json!({"state": "exited", "exitCode": null, "output": output, "firstRetainedSeq": 1}),
                )
            }
            "operations.job.get" if exec_job(params["jobId"].as_str().unwrap()).is_some() => {
                let name = exec_name(&world, params["jobId"].as_str().unwrap());
                let (code, _) = world.exec.get(&name).copied().unwrap_or((0, ""));
                Ok(finished(json!({"exitCode": code, "signal": null})))
            }
            "operations.job.get" => Ok(finished(Value::Null)),
            "operations.job.acknowledge" => Ok(Value::Null),
            other => Err(("internal", format!("unexpected {other}"))),
        }
    });
    (engine, world)
}

fn exec_job(job: &str) -> Option<usize> {
    job.strip_prefix("job:")?
        .parse::<usize>()
        .ok()
        .filter(|n| *n >= 100)
        .map(|n| n - 100)
}

fn exec_name(world: &World, job: &str) -> String {
    world.computers[exec_job(job).unwrap()].0.clone()
}

fn many(count: u32, name: Option<&str>, command: &[&str]) -> ManyArgs {
    ManyArgs {
        fork: ForkArgs {
            source: "main".into(),
            name: name.map(str::to_owned),
            ..ForkArgs::default()
        },
        count,
        command: command.iter().map(|word| (*word).to_owned()).collect(),
        cwd: None,
        steps: Default::default(),
    }
}

async fn run(engine: &FakeEngine, args: &ManyArgs) -> Result<Value, silo_cli::error::CliError> {
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let sink = |_: &str, _: bool, _: &[u8]| {};
    tokio::time::timeout(
        Duration::from_secs(20),
        fanout::fork_many(&connected, args, &ignore, &sink),
    )
    .await
    .expect("the forks were made at the same time")
}

#[tokio::test]
async fn forks_are_made_at_the_same_time_from_one_new_checkpoint() {
    let (engine, world) = engine(3, &[]);
    let result = run(&engine, &many(3, Some("try"), &[])).await.unwrap();
    let names: Vec<&str> = result["forks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|fork| fork["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["try-1", "try-2", "try-3"]);
    // One checkpoint, shared by all three.
    let created = engine.calls("checkpoints.create");
    assert_eq!(created.len(), 1);
    assert_eq!(created[0]["kind"], "fork-point");
    let forks = engine.calls("checkpoints.fork");
    assert_eq!(forks.len(), 3);
    assert!(forks.iter().all(|f| f["checkpointId"] == "cp-shared"));
    assert_eq!(result["checkpoint"], "cp-shared");
    // Each is started and shows its URL.
    assert_eq!(engine.calls("computers.start").len(), 3);
    for fork in result["forks"].as_array().unwrap() {
        assert_eq!(fork["state"], "running");
        assert_eq!(
            fork["urls"][0],
            format!(
                "http://3000.{}.silo.localhost:47383",
                fork["name"].as_str().unwrap()
            )
        );
    }
    assert_eq!(result["ok"], true);
    assert_eq!(world.lock().unwrap().forks_seen, 3);
}

#[tokio::test]
async fn a_warm_checkpoint_of_an_earlier_version_is_not_given_to_the_forks() {
    let (engine, world) = engine(2, &[]);
    world.lock().unwrap().warm = true;
    run(&engine, &many(2, None, &[])).await.unwrap();
    assert_eq!(engine.calls("checkpoints.create").len(), 1);
    let forks = engine.calls("checkpoints.fork");
    assert!(forks.iter().all(|f| f["checkpointId"] == "cp-shared"));
}

#[tokio::test]
async fn one_fork_with_no_count_takes_no_checkpoint_of_its_own() {
    let (engine, _) = engine(1, &[]);
    let result = run(&engine, &many(1, Some("solo"), &[])).await.unwrap();
    assert!(engine.calls("checkpoints.create").is_empty());
    assert_eq!(
        engine.calls("checkpoints.fork")[0]["checkpointId"],
        Value::Null
    );
    assert_eq!(result["forks"][0]["name"], "solo");
}

#[tokio::test]
async fn too_many_or_zero_forks_are_refused() {
    let (engine, _) = engine(1, &[]);
    assert!(run(&engine, &many(0, None, &[])).await.is_err());
    assert!(run(&engine, &many(fanout::MAX_FORKS + 1, None, &[]))
        .await
        .is_err());
}

#[tokio::test]
async fn a_command_runs_in_each_fork_and_its_exit_code_comes_back() {
    let (engine, _) = engine(2, &[("try-1", 0, "ok\n"), ("try-2", 5, "boom\n")]);
    let seen: Mutex<Vec<(String, bool, Vec<u8>)>> = Mutex::new(Vec::new());
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let sink = |name: &str, is_err: bool, bytes: &[u8]| {
        seen.lock()
            .unwrap()
            .push((name.into(), is_err, bytes.to_vec()));
    };
    let args = many(2, Some("try"), &["make", "test"]);
    let result = tokio::time::timeout(
        Duration::from_secs(20),
        fanout::fork_many(&connected, &args, &ignore, &sink),
    )
    .await
    .unwrap()
    .unwrap();
    let starts = engine.calls("guest.exec.start");
    assert_eq!(starts.len(), 2);
    assert!(starts.iter().all(|s| s["argv"] == json!(["make", "test"])));
    let forks = result["forks"].as_array().unwrap();
    assert_eq!(forks[0]["exitCode"], 0);
    assert_eq!(forks[0]["stdout"], "ok\n");
    assert_eq!(forks[1]["exitCode"], 5);
    assert_eq!(forks[1]["stderr"], "warn\n");
    assert_eq!(result["ok"], false);
    assert_eq!(fanout::exit_code(&result), 5);
    let seen = seen.lock().unwrap();
    assert!(seen.contains(&("try-2".into(), false, b"boom\n".to_vec())));
    assert!(seen.contains(&("try-1".into(), true, b"warn\n".to_vec())));
    // The forks are left running for review, not dropped.
    assert!(engine.calls("computers.delete").is_empty());
}

#[tokio::test]
async fn a_signal_exit_is_passed_as_128_plus_the_signal() {
    let (engine, _) = engine(1, &[]);
    let _ = &engine;
    // A killed command reports no exit code and the signal name.
    assert_eq!(silo_cli::ops::exit_code_for(None, Some("KILL")), 137);
    let result = json!({"forks": [{"name": "a", "exitCode": 137}], "ok": false});
    assert_eq!(fanout::exit_code(&result), 137);
    let failed = json!({"forks": [{"name": "a", "error": "x"}]});
    assert_eq!(fanout::exit_code(&failed), 1);
    assert_eq!(
        fanout::exit_code(&json!({"forks": [{"name": "a", "exitCode": 0}]})),
        0
    );
}

#[tokio::test]
async fn every_fork_publishes_the_ports_its_source_publishes() {
    let (engine, _) = engine(3, &[]);
    let result = run(&engine, &many(3, Some("web"), &[])).await.unwrap();
    let published = engine.calls("network.publish");
    assert_eq!(published.len(), 3);
    for fork in result["forks"].as_array().unwrap() {
        let id = format!("id-{}", fork["name"].as_str().unwrap());
        assert!(published
            .iter()
            .any(|call| call["computerId"] == id.as_str() && call["port"] == 3000));
        assert_eq!(fork["ports"], json!([3000]));
        assert!(fork["urls"][0].as_str().unwrap().starts_with("http://"));
    }
}

#[tokio::test]
async fn forks_on_another_device_run_their_command_there_and_publish_nothing_here() {
    const MINI: &str = "mini-0000";
    let (engine, _) = engine(2, &[("try-1", 0, "ok\n"), ("try-2", 3, "no\n")]);
    let connected = Engine::connect(&engine.socket)
        .await
        .unwrap()
        .on_device(Some(MINI.to_owned()));
    let sink = |_: &str, _: bool, _: &[u8]| {};
    let args = many(2, Some("try"), &["make", "test"]);
    let result = tokio::time::timeout(
        Duration::from_secs(20),
        fanout::fork_many(&connected, &args, &ignore, &sink),
    )
    .await
    .unwrap()
    .unwrap();
    for method in [
        "checkpoints.create",
        "checkpoints.fork",
        "computers.start",
        "guest.exec.start",
        "guest.exec.get",
        "operations.job.get",
    ] {
        let calls = engine.calls(method);
        assert!(!calls.is_empty(), "{method} was not sent");
        assert!(
            calls.iter().all(|params| params["deviceId"] == MINI),
            "{method}: {calls:?}"
        );
    }
    let forks = result["forks"].as_array().unwrap();
    assert_eq!(forks[0]["exitCode"], 0);
    assert_eq!(forks[0]["stdout"], "ok\n");
    assert_eq!(forks[1]["exitCode"], 3);
    for fork in forks {
        assert!(fork.get("portsError").is_none(), "{fork}");
        assert_eq!(fork["ports"], json!([]));
        assert_eq!(fork["urls"], json!([]));
    }
    for local in ["computers.access", "network.publish", "network.urls"] {
        assert!(engine.calls(local).is_empty(), "{local} was sent");
    }
}
