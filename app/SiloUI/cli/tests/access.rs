mod fake_engine;

use std::sync::{Arc, Mutex};

use fake_engine::{finished, listing, FakeEngine};
use serde_json::{json, Value};
use silo_cli::{
    access,
    engine::Engine,
    ops::{self, ForkArgs},
};
use silo_client::channel::Channel;

fn ignore(_: &str) {}

/// The published ports as (computer id, port, scheme).
type Published = Arc<Mutex<Vec<(String, u16, Option<String>)>>>;

fn state_of(published: &Published) -> Value {
    let rows = published.lock().unwrap().clone();
    let mut computers: Vec<Value> = Vec::new();
    for (id, port, scheme) in rows {
        let entry = json!({
            "port": port, "hostPort": 40000 + u64::from(port), "configuredHostPort": null,
            "scheme": scheme, "state": "reachable", "configured": true, "message": null
        });
        match computers
            .iter_mut()
            .find(|row| row["computerId"] == id.as_str())
        {
            Some(row) => row["ports"].as_array_mut().unwrap().push(entry),
            None => computers.push(json!({
                "computer": id, "computerId": id, "error": null, "ports": [entry]
            })),
        }
    }
    json!({ "computers": computers })
}

fn engine_with(names: &[&str], published: Published) -> FakeEngine {
    let names: Vec<String> = names.iter().map(|name| (*name).to_owned()).collect();
    FakeEngine::start(move |method, params| match method {
        "computers.list" => {
            let pairs: Vec<(&str, &str)> = names
                .iter()
                .map(|name| (name.as_str(), "running"))
                .collect();
            Ok(listing(&pairs))
        }
        "computers.access" => {
            let id = params["computerId"].as_str().unwrap_or_default();
            let ports = state_of(&published)["computers"]
                .as_array()
                .and_then(|rows| rows.iter().find(|row| row["computerId"] == id).cloned())
                .map_or(json!([]), |row| row["ports"].clone());
            Ok(json!({"computerId": id, "name": id, "secrets": [], "ports": ports}))
        }
        "network.urls" => Ok(json!({
            "computerId": params["computerId"],
            "urls": [{"port": 3000, "host": "h", "url": "http://main.test:43000"}]
        })),
        "network.publish" => {
            published.lock().unwrap().push((
                params["computerId"].as_str().unwrap().to_owned(),
                params["port"].as_u64().unwrap() as u16,
                params["scheme"].as_str().map(str::to_owned),
            ));
            Ok(state_of(&published))
        }
        "network.unpublish" => {
            let (id, port) = (
                params["computerId"].as_str().unwrap().to_owned(),
                params["port"].as_u64().unwrap() as u16,
            );
            published
                .lock()
                .unwrap()
                .retain(|(other, p, _)| !(other == &id && *p == port));
            Ok(state_of(&published))
        }
        "checkpoints.fork" => Ok(json!({"jobId": "job:1"})),
        "operations.job.get" => Ok(finished(json!({}))),
        "operations.job.acknowledge" => Ok(Value::Null),
        other => Err(("internal", format!("unexpected {other}"))),
    })
}

fn published_with(rows: &[(&str, u16, Option<&str>)]) -> Published {
    Arc::new(Mutex::new(
        rows.iter()
            .map(|(id, port, scheme)| ((*id).to_owned(), *port, scheme.map(str::to_owned)))
            .collect(),
    ))
}

#[tokio::test]
async fn ports_lists_what_a_computer_publishes_with_its_url() {
    let published = published_with(&[("id-main", 3000, Some("http")), ("id-other", 22, None)]);
    let engine = engine_with(&["main", "other"], published);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let value = access::ports(&connected, "main").await.unwrap();
    assert_eq!(value["computer"], "main");
    assert_eq!(value["ports"].as_array().unwrap().len(), 1);
    assert_eq!(value["ports"][0]["port"], 3000);
    assert_eq!(value["ports"][0]["url"], "http://main.test:43000");
}

#[tokio::test]
async fn publish_and_unpublish_use_the_computer_id() {
    let published = published_with(&[]);
    let engine = engine_with(&["main"], published);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let value = access::publish(&connected, "main", 8080, true)
        .await
        .unwrap();
    let call = &engine.calls("network.publish")[0];
    assert_eq!(call["computerId"], "id-main");
    assert_eq!(call["port"], 8080);
    assert_eq!(call["scheme"], "http");
    assert_eq!(value["ports"][0]["port"], 8080);
    let value = access::unpublish(&connected, "main", 8080).await.unwrap();
    assert_eq!(engine.calls("network.unpublish")[0]["port"], 8080);
    assert!(value["ports"].as_array().unwrap().is_empty());
    assert!(access::publish(&connected, "main", 0, false).await.is_err());
}

#[tokio::test]
async fn a_fork_publishes_the_ports_of_its_source() {
    let published = published_with(&[("id-main", 3000, Some("http")), ("id-main", 5432, None)]);
    let engine = engine_with(&["main", "try"], published);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let args = ForkArgs {
        source: "main".into(),
        name: Some("try".into()),
        checkpoint: Some("cp1".into()),
        secrets: Vec::new(),
        recipe_name: None,
        no_start: false,
    };
    let value = ops::fork(&connected, &args, &ignore).await.unwrap();
    assert_eq!(value["ports"], json!([3000, 5432]));
    let calls = engine.calls("network.publish");
    assert_eq!(calls.len(), 2);
    assert!(calls.iter().all(|call| call["computerId"] == "id-try"));
    assert_eq!(calls[0]["scheme"], "http");
    assert!(calls[1]["scheme"].is_null());
}

#[tokio::test]
async fn recipe_ports_are_published_once_on_main() {
    let published = published_with(&[("id-main", 3000, Some("http"))]);
    let engine = engine_with(&["main"], published);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let main = connected.computer("main").await.unwrap();
    let done = access::publish_recipe_ports(&connected, &main, &[3000, 4000, 4000])
        .await
        .unwrap();
    assert_eq!(done, [4000]);
    let calls = engine.calls("network.publish");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["scheme"], "http");
}

fn ssh_engine() -> FakeEngine {
    let enabled = Arc::new(Mutex::new(false));
    FakeEngine::start(move |method, params| match method {
        "computers.list" => Ok(listing(&[("main", "running")])),
        "sshAccess.state" => Ok(json!({"computers": [{
            "computer": "main", "computerId": "id-main",
            "enabled": *enabled.lock().unwrap(), "port": 2222, "bindAddress": "127.0.0.1",
            "keys": [], "state": "listening", "message": null, "fingerprint": null,
            "deviceName": "d", "addresses": [], "user": "silo"
        }]})),
        "sshAccess.save" => {
            *enabled.lock().unwrap() = params["enabled"] == true;
            Ok(json!({"computers": []}))
        }
        "sshAccess.connection" => Ok(json!(
            "ssh -i '/state/ssh/connections/id-main' -o IdentitiesOnly=yes -p 2230 silo@127.0.0.1"
        )),
        other => Err(("internal", format!("unexpected {other}"))),
    })
}

#[tokio::test]
async fn ssh_setup_turns_access_on_and_writes_the_name() {
    let engine = ssh_engine();
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let home = tempfile::tempdir().unwrap();
    let value = access::ssh_setup(&connected, "main", home.path(), Channel::Development)
        .await
        .unwrap();
    assert_eq!(value["host"], "main.silo-dev");
    assert_eq!(engine.calls("sshAccess.save")[0]["enabled"], true);
    let hosts = std::fs::read_to_string(home.path().join(".silo-dev/ssh-hosts.conf")).unwrap();
    assert!(hosts.starts_with("Host main.silo-dev\n"));
    assert!(hosts.contains("Port 2230") && hosts.contains("/state/ssh/connections/id-main"));
    let config = std::fs::read_to_string(home.path().join(".ssh/config")).unwrap();
    assert!(config.contains("ssh-hosts.conf"));
    access::ssh_setup(&connected, "main", home.path(), Channel::Development)
        .await
        .unwrap();
    assert_eq!(engine.calls("sshAccess.save").len(), 1);
    let config = std::fs::read_to_string(home.path().join(".ssh/config")).unwrap();
    assert_eq!(config.matches("Include").count(), 1);
    assert!(access::has_ssh_name(
        "main",
        home.path(),
        Channel::Development
    ));
}

#[tokio::test]
async fn the_name_follows_a_promote_and_an_undo() {
    let engine = ssh_engine();
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let home = tempfile::tempdir().unwrap();
    access::ssh_setup(&connected, "main", home.path(), Channel::Production)
        .await
        .unwrap();
    let before = engine.calls("sshAccess.connection").len();
    let mut promoted = json!({"status": "promoted", "report": {"target": "main"}});
    access::ssh_after_change(&connected, &mut promoted, home.path(), Channel::Production).await;
    assert_eq!(promoted["ssh"]["host"], "main.silo");
    let mut undone = json!({"status": "undone", "kind": "promote", "computer": "main"});
    access::ssh_after_change(&connected, &mut undone, home.path(), Channel::Production).await;
    assert_eq!(undone["ssh"]["host"], "main.silo");
    assert_eq!(engine.calls("sshAccess.connection").len(), before + 2);
    let mut kept = json!({"status": "undone", "kind": "keep", "computer": "main"});
    access::ssh_after_change(&connected, &mut kept, home.path(), Channel::Production).await;
    assert!(kept.get("ssh").is_none());
}

#[tokio::test]
async fn ports_and_the_display_after_unpublish_read_what_a_computer_may_read() {
    let published = published_with(&[("id-main", 3000, Some("http"))]);
    let engine = engine_with(&["main"], published);
    let connected = Engine::connect(&engine.socket).await.unwrap();
    access::ports(&connected, "main").await.unwrap();
    access::unpublish(&connected, "main", 3000).await.unwrap();
    // `network.state` is the owner's; a computer's agent reads `computers.access` instead.
    assert!(engine.calls("network.state").is_empty());
    assert!(!engine.calls("computers.access").is_empty());
}

#[tokio::test]
async fn access_lists_what_each_computer_holds() {
    let engine = FakeEngine::start(|method, params| match method {
        "computers.list" => Ok(listing(&[("main", "running"), ("try", "running")])),
        "computers.access" => Ok(json!({
            "computerId": params["computerId"], "name": params["computerId"],
            "secrets": [{"name": "npm", "hosts": ["registry.npmjs.org"]}],
            "github": null, "ports": [], "ssh": {"enabled": false, "port": 0, "state": "disabled"},
            "networkPolicy": {"mode": "allowlist", "hosts": ["example.com"], "presets": ["npm"]}
        })),
        other => Err(("internal", format!("unexpected {other}"))),
    });
    let connected = Engine::connect(&engine.socket).await.unwrap();
    let one = access::held(&connected, Some("try")).await.unwrap();
    assert_eq!(one["computers"].as_array().unwrap().len(), 1);
    assert_eq!(engine.calls("computers.access")[0]["computerId"], "id-try");
    let all = access::held(&connected, None).await.unwrap();
    assert_eq!(all["computers"].as_array().unwrap().len(), 2);
    let text = silo_cli::render::held(&all);
    assert!(text.contains("npm (to registry.npmjs.org)"), "{text}");
    assert!(
        text.contains("network: allowlist: example.com, npm"),
        "{text}"
    );
    assert!(text.contains("SSH: off"), "{text}");
}
