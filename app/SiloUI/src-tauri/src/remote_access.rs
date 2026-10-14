//! Guest access is routed through the owning Silo process. Only public SSH keys
//! cross devices; editor and terminal applications always launch locally.
use crate::{remote, runtime};
use serde_json::{json, Value};
use std::process::{Child, Command, Stdio};

pub fn target(value: &str) -> Result<Option<(String, String)>, String> {
    let Some(rest) = value.strip_prefix("silo-remote:") else {
        return Ok(None);
    };
    let Some((device, computer)) = rest.split_once(':') else {
        return Err("Invalid remote computer target.".into());
    };
    if uuid::Uuid::parse_str(device).is_err() || uuid::Uuid::parse_str(computer).is_err() {
        return Err("Invalid remote computer target.".into());
    }
    Ok(Some((device.into(), computer.into())))
}
fn string<'a>(params: &'a Value, key: &str) -> Result<&'a str, String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("Missing {key}."))
}
fn computer_name(paths: &runtime::RuntimePaths, params: &Value) -> Result<String, String> {
    runtime::remote_ops::local_computer_name_in(paths, string(params, "computerId")?)
}

pub type CheckpointHandler = dyn Fn(&str, &Value) -> Result<Value, String> + Send + Sync;

/// Client services used by the local Connections adapter while their domains finish
/// moving behind engine paths. The bridge passes only plain values through this seam.
pub trait LocalAccess: Send + Sync {
    fn desktop_connection(&self, computer: &str, computer_id: &str) -> Result<Value, String>;
    fn desktop_dispatch(&self, method: &str, params: &Value) -> Result<Value, String>;
    fn chatgpt_status(&self) -> Result<Value, String>;
    fn chatgpt_retry(&self) -> Result<Value, String>;
}

pub(crate) fn dispatch(
    paths: &runtime::RuntimePaths,
    method: &str,
    params: &Value,
    checkpoints: &CheckpointHandler,
    local: &dyn LocalAccess,
) -> Result<Value, String> {
    match method {
        "desktop.connect" => local.desktop_connection(
            &computer_name(paths, params)?,
            string(params, "computerId")?,
        ),
        "desktop.status" => {
            let mut state = local.desktop_dispatch(method, params)?;
            state["name"] = Value::String(computer_name(paths, params)?);
            Ok(state)
        }
        "desktop.action" | "computerUse.approval" => local.desktop_dispatch(method, params),
        "chatgpt.status" => local.chatgpt_status(),
        "chatgpt.retry" => local.chatgpt_retry(),
        "ssh.access.state" | "ssh.access.save" | "ssh.access.connection" => {
            crate::ssh_access::remote_dispatch(paths, method, params)
        }
        "files.list" => {
            // A file listing observes one computer's guest without starting or changing it
            // (`--no-start`), so it takes no gate and stays available during operations.
            let name = computer_name(paths, params)?;
            let offset = params.get("offset").and_then(Value::as_u64).unwrap_or(0);
            let offset = usize::try_from(offset).map_err(|_| "Invalid folder offset.")?;
            let page = crate::files::list_computer_directory(
                paths,
                name,
                string(params, "path")?.into(),
                offset,
                params
                    .get("snapshotId")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            )?;
            serde_json::to_value(page).map_err(|_| "Could not encode folder listing.".into())
        }
        "guest.prepare" => {
            // Authorizes a remote key inside one computer's guest; wait its turn per computer.
            let (_guard, name) =
                prepare_guest_target(&runtime::OPERATIONS, paths, string(params, "computerId")?)?;
            let user = crate::working_account::inspect_user(paths, &name)?;
            crate::working_account::require_client_protocol(params)?;
            let public = crate::editor::authorize_remote(
                paths,
                &name,
                string(params, "publicKey")?,
                string(params, "path")?,
            )?;
            let guest_address = if params["forwarding"].as_bool() == Some(true) {
                Some(guest_forwarding_address(paths, &name)?)
            } else {
                None
            };
            Ok(json!({"hostPublicKey": public, "user":user, "guestAddress":guest_address}))
        }
        "network.state" => crate::remote_network::device_state(paths),
        "network.publish" => {
            // `save_network_port` takes this computer's operation gate and shutdown check;
            // taking a device gate here too would deadlock against that computer guard.
            let name = computer_name(paths, params)?;
            let port = params["port"]
                .as_u64()
                .and_then(|p| u16::try_from(p).ok())
                .filter(|p| *p != 0)
                .ok_or("Invalid port.")?;
            let scheme = params["scheme"].as_str().map(str::to_owned);
            crate::network::save_network_port(paths, name, port, None, scheme)?;
            crate::remote_network::fresh_device_state(paths)
        }
        "network.unpublish" => {
            // The owner's mapping is shared by every controller; removing it from one
            // removes it here too, like removing the port on this device.
            let name = computer_name(paths, params)?;
            let port = params["port"]
                .as_u64()
                .and_then(|p| u16::try_from(p).ok())
                .filter(|p| *p != 0)
                .ok_or("Invalid port.")?;
            crate::network::remove_network_port(paths, name, port)?;
            crate::remote_network::fresh_device_state(paths)
        }
        // Pushes are bound to the repository, branch and commit the user confirmed;
        // the older unbound "repository.push" method is no longer served.
        "repository.push.start" => {
            crate::host_push_operations::start_remote(paths, computer_name(paths, params)?, params)
        }
        "repository.push.status" => crate::host_push_operations::status(
            paths,
            &computer_name(paths, params)?,
            string(params, "path")?,
            string(params, "operationId")?,
        ),
        "repository.dismiss" => {
            let name = computer_name(paths, params)?;
            crate::host_push::dismiss_repository_push(paths, &name, string(params, "path")?)?;
            Ok(Value::Null)
        }
        "checkpoint.create" | "checkpoint.fork" | "checkpoint.restore" => {
            // Resolve the computer through this device's saved identity before
            // forwarding checkpoint state changes to the owning runtime.
            let _ = computer_name(paths, params)?;
            checkpoints(method, params)
        }
        _ => Err("This Silo version does not support that remote operation.".into()),
    }
}

fn prepare_guest_target<'a>(
    gate: &'a runtime::operation_gate::OperationGate,
    paths: &runtime::RuntimePaths,
    id: &str,
) -> Result<(runtime::operation_gate::OperationGuard<'a>, String), String> {
    let name = runtime::remote_ops::local_computer_name_in(paths, id)?;
    let guard = gate
        .computer(id, &name, &format!("Preparing access to {name}"))
        .map_err(|e| e.to_string())?;
    let name = runtime::remote_ops::local_computer_name_in(paths, id)?;
    Ok((guard, name))
}

pub(crate) fn spawn_stream(
    paths: &runtime::RuntimePaths,
    method: &str,
    params: &Value,
) -> Result<Child, String> {
    if method != "guest.ssh" {
        return Err("Unsupported guest connection.".into());
    }
    // Interactive sessions stay outside the operation queue: this only inspects that
    // the computer is Running and spawns an `msb ssh serve` session, so it takes no gate
    // and a long operation never blocks opening a connection.
    let name = computer_name(paths, params)?;
    runtime::shutdown::ensure_accepting_operations()?;
    crate::terminal::running_computer(paths, &name)?;
    crate::working_account::require_runtime(paths)?;
    Command::new(&paths.executable)
        .env("MSB_HOME", &paths.home)
        .env("MSB_PATH", &paths.executable)
        .env("MSB_LIBKRUNFW_PATH", &paths.library)
        .args([
            "ssh",
            "serve",
            &name,
            "--stdio",
            "--no-start",
            "--no-inactivity-timeout",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "Could not open the remote computer connection.".into())
}

fn parse_guest_address(value: &str) -> Result<std::net::IpAddr, String> {
    let address: std::net::IpAddr = value
        .trim()
        .parse()
        .map_err(|_| "Invalid guest forwarding address.")?;
    if address.is_loopback() || address.is_unspecified() || address.is_multicast() {
        return Err("Invalid guest forwarding address.".into());
    }
    Ok(address)
}
const GUEST_FORWARDING_ADDRESS: &str = "import socket\nfor family, destination in [(socket.AF_INET, ('192.0.2.1', 9)), (socket.AF_INET6, ('2001:db8::1', 9))]:\n try:\n  with socket.socket(family, socket.SOCK_DGRAM) as stream:\n   stream.connect(destination)\n   print(stream.getsockname()[0])\n   break\n except OSError:\n  continue\nelse:\n raise SystemExit('Guest network address is unavailable')\n";
fn guest_forwarding_address(
    paths: &runtime::RuntimePaths,
    name: &str,
) -> Result<std::net::IpAddr, String> {
    // UDP connect selects the guest's route/source address without sending a packet.
    let output = runtime::run_msb(
        paths,
        &[
            "exec".into(),
            name.into(),
            "--no-start".into(),
            "--no-tty".into(),
            "--quiet".into(),
            "--timeout".into(),
            "3s".into(),
            "--".into(),
            "python3".into(),
            "-c".into(),
            GUEST_FORWARDING_ADDRESS.into(),
        ],
        std::time::Duration::from_secs(5),
    )
    .map_err(|error| error.to_string())?;
    parse_guest_address(&output.stdout)
}

pub(crate) fn prepare(
    device: &str,
    computer: &str,
    public: &str,
    path: &str,
    forwarding: bool,
) -> Result<(String, &'static str, Option<std::net::IpAddr>), String> {
    let result = remote::call_remote(
        device,
        "guest.prepare",
        json!({"computerId":computer,"publicKey":public,"path":path,"accountProtocol":1,"forwarding":forwarding}),
    )?;
    let key = string(&result, "hostPublicKey")?;
    crate::editor::validate_public_key(key)?;
    let address = if forwarding {
        Some(parse_guest_address(string(&result, "guestAddress")?)?)
    } else {
        None
    };
    Ok((
        key.into(),
        crate::working_account::response_user(&result)?,
        address,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn save_guest_target(paths: &runtime::RuntimePaths, id: &str, name: &str) {
        runtime::write_metadata(
            &paths.metadata,
            &runtime::ComputerConfigurationRequest {
                schema_version: 1,
                computers: vec![runtime::ComputerConfiguration {
                    id: id.into(),
                    name: name.into(),
                    cpus: 1,
                    max_cpus: 2,
                    memory_gib: 2,
                    max_memory_gib: 4,
                    workspace_storage_gib: 10,
                    runtime_storage_gib: 10,
                    desktop: None,
                }],
            },
        )
        .unwrap();
    }

    #[test]
    fn queued_guest_preparation_preserves_the_requested_computer_identity() {
        use std::time::{Duration, Instant};
        let original = "00000000-0000-4000-8000-000000000001";
        let replacement = "00000000-0000-4000-8000-000000000002";
        for replace in [true, false] {
            let dir = tempfile::tempdir().unwrap();
            let paths = crate::test_support::paths(dir.path());
            save_guest_target(&paths, original, "dev");
            let gate = runtime::operation_gate::OperationGate::new();
            let device = gate.device("Change configuration").unwrap();
            std::thread::scope(|scope| {
                let prepare = scope.spawn(|| {
                    prepare_guest_target(&gate, &paths, original).map(|(_guard, name)| name)
                });
                let until = Instant::now() + Duration::from_secs(5);
                while gate.snapshot().waiting.is_empty() {
                    assert!(Instant::now() < until, "preparation never queued");
                    std::thread::sleep(Duration::from_millis(5));
                }
                assert_eq!(
                    gate.snapshot().waiting[0].computer_id.as_deref(),
                    Some(original)
                );
                if replace {
                    save_guest_target(&paths, replacement, "dev");
                } else {
                    save_guest_target(&paths, original, "renamed");
                }
                drop(device);
                let result = prepare.join().unwrap();
                if replace {
                    assert!(
                        result.is_err(),
                        "preparation accepted the replacement computer"
                    );
                } else {
                    assert_eq!(result.unwrap(), "renamed");
                }
            });
            assert!(gate.is_idle());
        }
    }

    #[test]
    fn guest_preparation_rejects_a_stale_id_before_admission() {
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::test_support::paths(dir.path());
        save_guest_target(&paths, "00000000-0000-4000-8000-000000000002", "dev");
        let gate = runtime::operation_gate::OperationGate::new();
        assert!(
            prepare_guest_target(&gate, &paths, "00000000-0000-4000-8000-000000000001").is_err()
        );
        assert!(gate.is_idle());
    }

    #[test]
    fn guest_preparation_accepts_an_unchanged_computer() {
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::test_support::paths(dir.path());
        let id = "00000000-0000-4000-8000-000000000001";
        save_guest_target(&paths, id, "dev");
        let gate = runtime::operation_gate::OperationGate::new();
        let (guard, name) = prepare_guest_target(&gate, &paths, id).unwrap();
        assert_eq!(name, "dev");
        assert_eq!(gate.snapshot().running[0].computer_id.as_deref(), Some(id));
        drop(guard);
        assert!(gate.is_idle());
    }

    #[test]
    fn forwarding_preserves_guest_interface_addresses_and_rejects_shell_text() {
        assert_eq!(
            parse_guest_address("172.16.0.6\n").unwrap().to_string(),
            "172.16.0.6"
        );
        assert_eq!(
            parse_guest_address("fd00::2\n").unwrap().to_string(),
            "fd00::2"
        );
        for input in [
            "127.0.0.1",
            "::1",
            "0.0.0.0",
            "::",
            "224.0.0.1",
            "ff02::1",
            "172.16.0.6:3000",
            "172.16.0.6; touch /tmp/injected",
            "172.16.0.6\n172.16.0.10",
        ] {
            assert!(parse_guest_address(input).is_err(), "{input}");
        }
    }

    #[test]
    fn forwarding_probe_uses_ipv4_then_ipv6_without_sending_data() {
        let fixture = r#"import socket,sys
mode = sys.argv[1]
class RouteSocket:
 def __init__(self, family, kind):
  assert kind == socket.SOCK_DGRAM
  self.family = family
 def __enter__(self): return self
 def __exit__(self, *args): pass
 def connect(self, address):
  if mode == 'none' or (mode == 'ipv6' and self.family == socket.AF_INET):
   raise OSError('No route')
 def getsockname(self):
  return ('172.16.0.6' if self.family == socket.AF_INET else 'fd00::2', 40000)
socket.socket = RouteSocket
"#;
        let script = format!("{fixture}\n{GUEST_FORWARDING_ADDRESS}");
        for (mode, expected) in [
            ("ipv4", Some("172.16.0.6")),
            ("ipv6", Some("fd00::2")),
            ("none", None),
        ] {
            let output = Command::new("python3")
                .args(["-c", &script, mode])
                .output()
                .unwrap();
            if let Some(expected) = expected {
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                assert_eq!(
                    parse_guest_address(&String::from_utf8(output.stdout).unwrap())
                        .unwrap()
                        .to_string(),
                    expected
                );
            } else {
                assert!(!output.status.success());
                assert!(output.stdout.is_empty());
            }
        }
    }

    #[test]
    fn remote_targets_never_fall_back_to_local_names() {
        let device = uuid::Uuid::new_v4();
        let computer = uuid::Uuid::new_v4();
        assert_eq!(
            target(&format!("silo-remote:{device}:{computer}")).unwrap(),
            Some((device.to_string(), computer.to_string()))
        );
        assert_eq!(target("dev").unwrap(), None);
        for input in [
            "silo-remote:dev",
            "silo-remote:host:dev",
            "silo-remote:host:computer:extra",
        ] {
            assert!(target(input).is_err());
        }
    }
}
