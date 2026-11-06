//! `silo host` and `silo devices`. The engine is a fake `engine.sock`, `silod` a shell script
//! that records its arguments, and the terminal a fake that records the SSH commands it would
//! run. Nothing here starts a service, opens SSH or reads `/dev/kvm`.

mod fake_engine;

use std::{
    cell::RefCell,
    fs,
    net::{SocketAddr, TcpListener},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use fake_engine::FakeEngine;
use serde_json::{json, Value};
use silo_cli::{
    engine::Engine,
    hosts::{self, HostSetup, Membership, Platform, Terminal, VmProbe},
};
use silo_client::channel::Channel;

const KEY: &str =
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIKVyZDGnacgUkbjXHi/TsIZ+GxIgprlkfONi9t33BGz3 laptop";
const FINGERPRINT: &str = "SHA256:LH9ifkGkEurhEj7pgiSwRedJtKQomUQeevYOdCTeGWw";
const REGISTERED: &str =
    "enabled\nstate: active\nunit: /h/.config/systemd/user/org.silo.dev.engine.service\nprogram: this silod\nlinger: no\n";

/// An unpacked engine directory with a `silod` that prints `stdout` and `stderr`, exits with
/// `code`, and appends its arguments to `calls` beside it.
struct EngineDirectory {
    _root: tempfile::TempDir,
    directory: PathBuf,
    home: PathBuf,
}

impl EngineDirectory {
    fn new(stdout: &str, stderr: &str, code: i32) -> Self {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("silo-engine");
        let home = root.path().join("home");
        fs::create_dir_all(directory.join("resources")).unwrap();
        fs::create_dir_all(&home).unwrap();
        fs::write(directory.join("silo"), "").unwrap();
        let silod = directory.join("silod");
        fs::write(
            &silod,
            format!(
                "#!/bin/sh\necho \"$@\" >> \"$(dirname \"$0\")/calls\"\nprintf '%s' '{stdout}'\nprintf '%s' '{stderr}' >&2\nexit {code}\n"
            ),
        )
        .unwrap();
        fs::set_permissions(&silod, fs::Permissions::from_mode(0o755)).unwrap();
        Self {
            _root: root,
            directory,
            home,
        }
    }

    fn calls(&self) -> Vec<String> {
        fs::read_to_string(self.directory.join("calls"))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn setup(&self, socket: &Path, ssh: SocketAddr) -> HostSetup {
        HostSetup {
            platform: Platform::Linux,
            channel: Channel::Development,
            euid: 1000,
            home: self.home.clone(),
            cli: self.directory.join("silo"),
            path: Some(
                std::env::join_paths([self.home.join(".local/bin"), PathBuf::from("/usr/bin")])
                    .unwrap(),
            ),
            socket: socket.to_owned(),
            vm: kvm_ok(),
            ssh,
            engine_wait: Duration::from_millis(300),
        }
    }
}

fn kvm_ok() -> VmProbe {
    VmProbe::Linux {
        device: true,
        accessible: true,
        group: Some("kvm".into()),
        membership: Membership::Active,
    }
}

/// The address of an SSH server that answers: a listener kept open for the test.
fn listening() -> (TcpListener, SocketAddr) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    (listener, address)
}

/// An address where nothing listens.
fn closed() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap()
}

fn status_reply(enabled: bool) -> Value {
    json!({
        "enabled": enabled, "deviceId": "dev-1", "name": "box",
        "address": "ada@box.local",
        "addresses": [{"address": "ada@box.local", "kind": "name"}, {"address": "ada@100.64.0.7", "kind": "tailscale"}],
        "error": null
    })
}

/// A host engine that keeps the authorized keys it was given.
fn host_engine() -> FakeEngine {
    let keys: Arc<Mutex<Vec<String>>> = Arc::default();
    FakeEngine::start(move |method, params| match method {
        "connections.setEnabled" => Ok(status_reply(params["enabled"] == true)),
        "connections.status" => Ok(status_reply(true)),
        "connections.keys.authorize" => {
            let key = params["publicKey"].as_str().unwrap().to_owned();
            let mut keys = keys.lock().unwrap();
            let added = !keys.contains(&key);
            if added {
                keys.push(key);
            }
            Ok(json!({"added": added}))
        }
        "connections.keys.revoke" => {
            let key = params["publicKey"].as_str().unwrap();
            keys.lock().unwrap().retain(|known| known != key);
            Ok(Value::Null)
        }
        "connections.keys.list" => Ok(json!({"publicKeys": *keys.lock().unwrap()})),
        other => Err(("internal", format!("unexpected {other}"))),
    })
}

#[tokio::test]
async fn init_registers_the_service_links_the_command_and_authorizes_the_key() {
    let engine = host_engine();
    let unpacked = EngineDirectory::new(REGISTERED, "Lingering is off for ada", 0);
    let (_ssh, ssh) = listening();
    let setup = unpacked.setup(&engine.socket, ssh);
    let value = hosts::init(&setup, &[KEY.to_owned()]).await.unwrap();

    assert_eq!(unpacked.calls(), vec!["--service register"]);
    assert_eq!(value["kinds"], json!(["linux"]));
    assert_eq!(value["service"]["status"], "enabled");
    assert_eq!(value["service"]["linger"], "no");
    let link = unpacked.home.join(".local/bin/silo-dev");
    assert_eq!(fs::read_link(&link).unwrap(), setup.cli);
    assert_eq!(value["cli"]["state"], "linked");
    assert_eq!(
        engine.calls("connections.setEnabled"),
        vec![json!({"enabled": true})]
    );
    assert_eq!(
        engine.calls("connections.keys.authorize"),
        vec![json!({"publicKey": KEY})]
    );
    assert_eq!(value["authorized"][0]["added"], true);
    assert_eq!(value["authorized"][0]["fingerprint"], FINGERPRINT);
    assert_eq!(value["controllers"][0]["fingerprint"], FINGERPRINT);
    assert_eq!(value["connections"]["enabled"], true);
    assert_eq!(value["ssh"]["listening"], true);
    let advice: Vec<&str> = value["advice"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(advice, vec!["Lingering is off for ada"]);
    let text = hosts::render_host(&value);
    assert!(text.contains("silo devices add ada@box.local"), "{text}");
    assert!(text.contains("Other addresses: ada@100.64.0.7"), "{text}");
    assert!(
        text.contains(&format!("Authorized {FINGERPRINT}")),
        "{text}"
    );
    assert!(text.contains("starts at boot: no"), "{text}");

    // A second run changes nothing it already did and says so.
    let again = hosts::init(&setup, &[KEY.to_owned()]).await.unwrap();
    assert_eq!(again["cli"]["state"], "alreadyLinked");
    assert_eq!(again["authorized"][0]["added"], false);
    assert_eq!(again["controllers"].as_array().unwrap().len(), 1);
    assert!(hosts::render_host(&again).contains(&format!("Already authorized {FINGERPRINT}")));
}

#[tokio::test]
async fn init_without_a_key_says_how_a_controller_gets_in_and_where_ssh_is_missing() {
    let engine = host_engine();
    let unpacked = EngineDirectory::new(REGISTERED, "", 0);
    let mut setup = unpacked.setup(&engine.socket, closed());
    setup.path = Some("/usr/bin".into());
    let value = hosts::init(&setup, &[]).await.unwrap();
    assert_eq!(value["ssh"]["listening"], false);
    let advice = value["advice"].to_string();
    assert!(advice.contains("is not on PATH"), "{advice}");
    assert!(advice.contains("openssh-server"), "{advice}");
    assert!(advice.contains("silo devices key"), "{advice}");
    assert!(engine.calls("connections.keys.authorize").is_empty());
}

#[tokio::test]
async fn init_checks_the_account_vm_support_and_keys_before_changing_anything() {
    let engine = host_engine();
    let unpacked = EngineDirectory::new(REGISTERED, "", 0);
    let (_ssh, ssh) = listening();

    let mut root = unpacked.setup(&engine.socket, ssh);
    root.euid = 0;
    let error = hosts::init(&root, &[]).await.unwrap_err();
    assert!(error.message.contains("not root"), "{}", error.message);

    let mut no_kvm = unpacked.setup(&engine.socket, ssh);
    no_kvm.vm = VmProbe::Linux {
        device: true,
        accessible: false,
        group: Some("kvm".into()),
        membership: Membership::Absent,
    };
    let error = hosts::init(&no_kvm, &[]).await.unwrap_err();
    assert!(
        error.message.contains("usermod -aG kvm"),
        "{}",
        error.message
    );

    let setup = unpacked.setup(&engine.socket, ssh);
    let error = hosts::init(&setup, &["no-such-key-file".to_owned()])
        .await
        .unwrap_err();
    assert!(
        error.message.contains("no-such-key-file"),
        "{}",
        error.message
    );

    assert!(unpacked.calls().is_empty());
    assert!(fs::symlink_metadata(unpacked.home.join(".local/bin/silo-dev")).is_err());
    assert_eq!(engine.accepted(), 0);
}

#[tokio::test]
async fn init_reports_silods_refusal() {
    let engine = host_engine();
    let unpacked = EngineDirectory::new(
        "",
        "The engine directory /tmp/silo-engine is writable by others",
        1,
    );
    let (_ssh, ssh) = listening();
    let error = hosts::init(&unpacked.setup(&engine.socket, ssh), &[])
        .await
        .unwrap_err();
    assert!(
        error.message.contains("--service register failed"),
        "{}",
        error.message
    );
    assert!(
        error.message.contains("writable by others"),
        "{}",
        error.message
    );
    assert_eq!(engine.accepted(), 0);
}

#[tokio::test]
async fn init_needs_silod_beside_silo_and_an_engine_that_answers() {
    let engine = host_engine();
    let unpacked = EngineDirectory::new(REGISTERED, "", 0);
    let (_ssh, ssh) = listening();
    let mut elsewhere = unpacked.setup(&engine.socket, ssh);
    elsewhere.cli = unpacked.home.join("silo");
    let error = hosts::init(&elsewhere, &[]).await.unwrap_err();
    assert!(
        error.message.contains("no silod beside"),
        "{}",
        error.message
    );

    let silent = unpacked.setup(&unpacked.home.join("engine.sock"), ssh);
    let error = hosts::init(&silent, &[]).await.unwrap_err();
    assert!(
        error.message.contains("does not answer"),
        "{}",
        error.message
    );
    assert!(
        error
            .message
            .contains("journalctl --user -u org.silo.dev.engine.service"),
        "{}",
        error.message
    );
}

#[tokio::test]
async fn init_replaces_only_its_own_link() {
    let engine = host_engine();
    let unpacked = EngineDirectory::new(REGISTERED, "", 0);
    let (_ssh, ssh) = listening();
    let setup = unpacked.setup(&engine.socket, ssh);
    let bin = unpacked.home.join(".local/bin");
    fs::create_dir_all(&bin).unwrap();
    let link = bin.join("silo-dev");

    // A link to a silo that is gone (an engine directory replaced since) is Silo's.
    std::os::unix::fs::symlink(unpacked.home.join("old-engine/silo"), &link).unwrap();
    let value = hosts::init(&setup, &[]).await.unwrap();
    assert_eq!(value["cli"]["state"], "linked");
    assert_eq!(fs::read_link(&link).unwrap(), setup.cli);

    // Anything else is the user's own and stays.
    fs::remove_file(&link).unwrap();
    fs::write(&link, "#!/bin/sh\n").unwrap();
    let value = hosts::init(&setup, &[]).await.unwrap();
    assert_eq!(value["cli"]["state"], "notLinked");
    assert_eq!(fs::read_to_string(&link).unwrap(), "#!/bin/sh\n");
    assert!(value["advice"].to_string().contains("left as it is"));

    fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink("/usr/bin/true", &link).unwrap();
    let value = hosts::init(&setup, &[]).await.unwrap();
    assert_eq!(value["cli"]["state"], "notLinked");
    assert_eq!(
        fs::read_link(&link).unwrap(),
        PathBuf::from("/usr/bin/true")
    );
}

#[tokio::test]
async fn status_reads_without_changing_anything() {
    let engine = host_engine();
    let unpacked = EngineDirectory::new(REGISTERED, "", 0);
    let (_ssh, ssh) = listening();
    let setup = unpacked.setup(&engine.socket, ssh);
    let value = hosts::status(&setup).await.unwrap();
    assert_eq!(unpacked.calls(), vec!["--service status"]);
    assert_eq!(value["engine"]["running"], true);
    assert_eq!(value["vm"]["ok"], true);
    assert_eq!(value["connections"]["enabled"], true);
    assert_eq!(value["connections"]["bridgeLink"], false);
    assert_eq!(value["controllers"], json!([]));
    assert!(engine.calls("connections.setEnabled").is_empty());
    assert!(fs::symlink_metadata(unpacked.home.join(".local/bin/silo-dev")).is_err());
    assert!(hosts::render_host(&value).contains("Controllers: none"));

    let mut stopped = unpacked.setup(&unpacked.home.join("engine.sock"), ssh);
    stopped.vm = VmProbe::Linux {
        device: false,
        accessible: false,
        group: None,
        membership: Membership::Unknown,
    };
    let value = hosts::status(&stopped).await.unwrap();
    assert_eq!(value["engine"]["running"], false);
    assert_eq!(value["vm"]["ok"], false);
    assert!(value.get("connections").is_none());
    let text = hosts::render_host(&value);
    assert!(text.contains("Engine: not running"), "{text}");
    assert!(text.contains("cannot run computers yet"), "{text}");
}

#[tokio::test]
async fn revoke_removes_the_key_and_lists_what_is_left() {
    let engine = host_engine();
    let client = Engine::connect(&engine.socket).await.unwrap();
    client
        .call(silo_api::methods::requests::connections_keys_authorize(
            silo_api::methods::connections::AuthorizeKeyParams {
                public_key: KEY.to_owned(),
            },
        ))
        .await
        .unwrap();
    let value = hosts::revoke(&client, KEY).await.unwrap();
    assert_eq!(value["revoked"][0]["fingerprint"], FINGERPRINT);
    assert_eq!(value["controllers"], json!([]));
    assert_eq!(
        engine.calls("connections.keys.revoke"),
        vec![json!({"publicKey": KEY})]
    );
}

/// Records the commands it is asked to run and succeeds.
struct FakeTerminal {
    interactive: bool,
    ran: RefCell<Vec<String>>,
}

impl Terminal for FakeTerminal {
    fn interactive(&self) -> bool {
        self.interactive
    }

    fn run(&self, command: &str) -> Result<bool, silo_cli::error::CliError> {
        self.ran.borrow_mut().push(command.to_owned());
        Ok(true)
    }

    fn keyscan(&self, host: &str, port: u16) -> Result<String, silo_cli::error::CliError> {
        self.ran.borrow_mut().push(format!("keyscan {host} {port}"));
        Ok(format!(
            "# {host}:{port} SSH-2.0-OpenSSH_9.6\n[{host}]:{port} ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAAAgQC0\n[{host}]:{port} {HOST_KEY}\n"
        ))
    }
}

/// A host key and its `ssh-keygen -lf` fingerprint.
const HOST_KEY: &str =
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIKVyZDGnacgUkbjXHi/TsIZ+GxIgprlkfONi9t33BGz3";

fn add(
    address: &str,
    replace: bool,
    host_key: Option<&str>,
    known_hosts: &Path,
) -> hosts::AddRequest {
    hosts::AddRequest {
        address: address.to_owned(),
        replace,
        host_key: host_key.map(str::to_owned),
        known_hosts: known_hosts.to_owned(),
    }
}

fn no_known_hosts() -> PathBuf {
    PathBuf::from("/nonexistent/known_hosts")
}

const HOST_KEY_FAILED: &str = "Host key verification failed. Connect once with SSH in a terminal to verify and trust the other device's host key.";
const AUTH_FAILED: &str =
    "SSH authentication failed. Set up Silo's SSH key for this device, or configure an SSH key or agent.";

/// A controller engine whose first connects fail with `failures`, in order.
fn controller(failures: Vec<&'static str>) -> FakeEngine {
    let failures = Mutex::new(failures.into_iter());
    FakeEngine::start(move |method, params| match method {
        "connections.devices.connect" => match failures.lock().unwrap().next() {
            Some(message) => Err(("internal", message.to_owned())),
            None => Ok(json!({"id": "dev-1", "name": "box", "address": params["address"]})),
        },
        "connections.ssh.authorizationCommand" => Ok(
            json!({"command": format!("/usr/bin/ssh -- {} true", params["address"].as_str().unwrap())}),
        ),
        "connections.ssh.keySetupCommand" => Ok(
            json!({"command": format!("printf key | /usr/bin/ssh -- {} install", params["address"].as_str().unwrap())}),
        ),
        "connections.devices.list" => {
            Ok(json!([{"id": "dev-1", "name": "box", "address": "ada@box"}]))
        }
        "connections.devices.health" => Ok(
            json!({"devices": [{"deviceId": "dev-1", "connected": false, "busy": false, "error": "Connection refused"}]}),
        ),
        "connections.devices.remove" => Ok(Value::Null),
        "connections.keys.own" => Ok(json!({"publicKey": KEY})),
        other => Err(("internal", format!("unexpected {other}"))),
    })
}

#[tokio::test]
async fn add_trusts_the_host_key_and_installs_the_key_when_ssh_asks() {
    let engine = controller(vec![HOST_KEY_FAILED, AUTH_FAILED]);
    let client = Engine::connect(&engine.socket).await.unwrap();
    let terminal = FakeTerminal {
        interactive: true,
        ran: RefCell::default(),
    };
    let said = RefCell::new(Vec::new());
    let value = hosts::add_device(
        &client,
        &add("ada@box", false, None, &no_known_hosts()),
        &terminal,
        &|line| said.borrow_mut().push(line.to_owned()),
    )
    .await
    .unwrap();
    assert_eq!(value["device"]["id"], "dev-1");
    assert_eq!(value["repaired"], json!(["trustHostKey", "installKey"]));
    assert_eq!(
        *terminal.ran.borrow(),
        vec![
            "/usr/bin/ssh -- ada@box true".to_owned(),
            "printf key | /usr/bin/ssh -- ada@box install".to_owned()
        ]
    );
    assert_eq!(engine.calls("connections.devices.connect").len(), 3);
    assert_eq!(
        engine.calls("connections.devices.connect")[0],
        json!({"address": "ada@box"})
    );
    assert!(said.borrow()[1].contains("Install this device's Silo key"));
    let text = hosts::render_added(&value);
    assert!(text.contains("Connected box (dev-1) at ada@box"), "{text}");
}

#[tokio::test]
async fn add_repairs_each_problem_once_and_never_a_changed_host_key() {
    let engine = controller(vec![AUTH_FAILED, AUTH_FAILED]);
    let client = Engine::connect(&engine.socket).await.unwrap();
    let terminal = FakeTerminal {
        interactive: true,
        ran: RefCell::default(),
    };
    let error = hosts::add_device(
        &client,
        &add("ada@box", true, None, &no_known_hosts()),
        &terminal,
        &|_| {},
    )
    .await
    .unwrap_err();
    assert_eq!(error.message, AUTH_FAILED);
    assert_eq!(terminal.ran.borrow().len(), 1);
    assert_eq!(
        engine.calls("connections.devices.connect")[0],
        json!({"address": "ada@box", "replace": true})
    );

    let changed = "The other device's SSH host key changed. Verify the device before trusting its new key (Host key verification failed).";
    let engine = controller(vec![changed]);
    let client = Engine::connect(&engine.socket).await.unwrap();
    let error = hosts::add_device(
        &client,
        &add("ada@box", false, None, &no_known_hosts()),
        &terminal,
        &|_| {},
    )
    .await
    .unwrap_err();
    assert_eq!(error.message, changed);
    assert_eq!(terminal.ran.borrow().len(), 1);
}

#[tokio::test]
async fn add_without_a_terminal_prints_the_command_instead() {
    let engine = controller(vec![AUTH_FAILED]);
    let client = Engine::connect(&engine.socket).await.unwrap();
    let terminal = FakeTerminal {
        interactive: false,
        ran: RefCell::default(),
    };
    let error = hosts::add_device(
        &client,
        &add("ada@box", false, None, &no_known_hosts()),
        &terminal,
        &|_| {},
    )
    .await
    .unwrap_err();
    assert!(
        error
            .message
            .contains("printf key | /usr/bin/ssh -- ada@box install"),
        "{}",
        error.message
    );
    assert!(terminal.ran.borrow().is_empty());
}

#[tokio::test]
async fn list_remove_and_key() {
    let engine = controller(vec![]);
    let client = Engine::connect(&engine.socket).await.unwrap();
    let listed = hosts::list_devices(&client).await.unwrap();
    assert_eq!(listed["devices"][0]["connected"], false);
    assert!(hosts::render_devices(&listed).contains("offline: Connection refused"));

    let removed = hosts::remove_device(&client, "BOX").await.unwrap();
    assert_eq!(removed["removed"], "dev-1");
    assert_eq!(
        engine.calls("connections.devices.remove"),
        vec![json!({"deviceId": "dev-1"})]
    );
    assert!(hosts::remove_device(&client, "other").await.is_err());

    let key = hosts::own_key(&client).await.unwrap();
    assert_eq!(key["fingerprint"], FINGERPRINT);
    let text = hosts::render_key(&key);
    assert!(
        text.contains(&format!("silo host init --authorize '{KEY}'")),
        "{text}"
    );
}

#[tokio::test]
async fn host_and_devices_refuse_another_device() {
    let engine = controller(vec![]);
    let socket = engine.socket.to_str().unwrap();
    for args in [["host", "status"], ["devices", "list"]] {
        let mut all = vec![
            "silo",
            "--socket",
            socket,
            "--allow-other-channel",
            "--device",
            "box",
        ];
        all.extend_from_slice(&args);
        let cli = <silo_cli::cli::Cli as clap::Parser>::try_parse_from(all).unwrap();
        let error = silo_cli::cli::run(cli, std::env::temp_dir().as_path())
            .await
            .unwrap_err();
        assert!(
            error.message.contains("--device cannot be used"),
            "{}",
            error.message
        );
    }
    for args in [["devices", "list"], ["devices", "remove"]] {
        let mut all = vec![
            "silo",
            "--socket",
            socket,
            "--allow-other-channel",
            "--json",
        ];
        all.extend_from_slice(&args);
        if args[1] == "remove" {
            all.push("box");
        }
        let cli = <silo_cli::cli::Cli as clap::Parser>::try_parse_from(all).unwrap();
        assert!(cli.device.is_none());
        assert_eq!(
            silo_cli::cli::run(cli, std::env::temp_dir().as_path())
                .await
                .unwrap(),
            0
        );
    }
    assert_eq!(
        engine.calls("connections.devices.remove"),
        vec![json!({"deviceId": "dev-1"})]
    );
}

#[tokio::test]
async fn add_pins_the_host_key_by_fingerprint_and_needs_no_prompt() {
    let engine = controller(vec![]);
    let client = Engine::connect(&engine.socket).await.unwrap();
    let home = tempfile::tempdir().unwrap();
    let known_hosts = home.path().join(".ssh/known_hosts");
    let terminal = FakeTerminal {
        interactive: false,
        ran: RefCell::default(),
    };
    let request = add("ada@127.0.0.1:2222", false, Some(FINGERPRINT), &known_hosts);
    let value = hosts::add_device(&client, &request, &terminal, &|_| {})
        .await
        .unwrap();
    assert_eq!(
        *terminal.ran.borrow(),
        vec!["keyscan 127.0.0.1 2222".to_owned()]
    );
    assert_eq!(
        engine.calls("connections.devices.connect"),
        vec![json!({"address": "ssh://ada@127.0.0.1:2222"})]
    );
    assert_eq!(
        fs::read_to_string(&known_hosts).unwrap(),
        format!("[127.0.0.1]:2222 {HOST_KEY}\n")
    );
    assert_eq!(
        fs::metadata(home.path().join(".ssh"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(value["hostKey"]["added"], true);
    assert!(hosts::render_added(&value).contains(FINGERPRINT));
    // Again: already there, nothing written twice.
    let value = hosts::add_device(&client, &request, &terminal, &|_| {})
        .await
        .unwrap();
    assert_eq!(value["hostKey"]["added"], false);
    assert_eq!(fs::read_to_string(&known_hosts).unwrap().lines().count(), 1);
}

#[test]
fn a_host_key_pin_never_trusts_what_it_does_not_name() {
    let home = tempfile::tempdir().unwrap();
    let known_hosts = home.path().join("known_hosts");
    let terminal = FakeTerminal {
        interactive: false,
        ran: RefCell::default(),
    };
    let wrong = "SHA256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    let error =
        hosts::pin_host_key(&known_hosts, "ssh://ada@box:2222", wrong, &terminal).unwrap_err();
    assert!(
        error.message.contains("No host key box offers matches"),
        "{}",
        error.message
    );
    assert!(error.message.contains(FINGERPRINT), "{}", error.message);
    assert!(!known_hosts.exists());

    // A key file, as /etc/ssh/ssh_host_ed25519_key.pub holds it, on the default port.
    let file = home.path().join("ssh_host_ed25519_key.pub");
    fs::write(&file, format!("{HOST_KEY} root@box\n")).unwrap();
    fs::write(&known_hosts, "other ssh-ed25519 AAAA").unwrap();
    let pinned =
        hosts::pin_host_key(&known_hosts, "ada@box", file.to_str().unwrap(), &terminal).unwrap();
    assert_eq!(pinned["name"], "box");
    assert_eq!(
        fs::read_to_string(&known_hosts).unwrap(),
        format!("other ssh-ed25519 AAAA\nbox {HOST_KEY}\n")
    );

    // Another key already listed for the name is reported, never replaced.
    let other = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIHk8t0ahm+m4Qf9wTQ2xV1Vv2Qb2QeQ3bE8m0l2a6y5Z";
    let error = hosts::pin_host_key(&known_hosts, "ada@box", other, &terminal).unwrap_err();
    assert!(
        error.message.contains("ssh-keygen -R 'box'"),
        "{}",
        error.message
    );
    assert!(hosts::pin_host_key(&known_hosts, "ada@box", "nonsense", &terminal).is_err());
}
