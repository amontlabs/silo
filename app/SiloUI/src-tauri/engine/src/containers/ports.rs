//! The port control of a running container computer.
//!
//! The engine publishes a computer's service ports through a control socket per computer:
//! `ports_list`, `port_add`, `port_remove` and `port_probe`, one JSON line each way. For a
//! virtual machine MicroSandbox serves that socket. For a container the engine serves the same
//! socket itself and passes each request on to the computer's gateway, which binds the
//! listeners on the host's loopback (the gateway picks the host port unless one is asked for)
//! and carries each connection to the guest. The port settings, the URL proxy and the reachable
//! check above it are the ones virtual machines use.
//!
//! The socket sits in a private directory, so only the engine's own account can ask. A request
//! the gateway refuses leaves it running; an answer that does not belong to the request stops
//! it (see `computer_gateway`).

use super::lifecycle::Running;
use crate::computer_gateway::PublishedPort;
use crate::runtime::RuntimePaths;
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{Ipv4Addr, TcpListener},
    os::unix::{
        fs::{DirBuilderExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::Path,
    sync::{Arc, Weak},
    time::Duration,
};

/// The longest request line served.
const MAX_REQUEST_BYTES: u64 = 4096;
const IO_TIMEOUT: Duration = Duration::from_secs(8);
const POLL: Duration = Duration::from_millis(50);

fn error(message: &str) -> Value {
    json!({"ok": false, "error": message})
}

fn ports_value(ports: &[PublishedPort]) -> Value {
    json!({
        "ok": true,
        "ports": ports.iter().map(|port| json!({
            "guest_port": port.guest_port,
            "host_port": port.host_port,
            "host_bind": "127.0.0.1",
        })).collect::<Vec<_>>(),
    })
}

fn port_of(request: &Value, key: &str) -> Option<u16> {
    request
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|port| u16::try_from(port).ok())
}

/// Whether the host's loopback port can be bound now, so the refusal can say why (the
/// gateway's own refusal does not).
fn host_port_free(port: u16) -> bool {
    port == 0 || TcpListener::bind((Ipv4Addr::LOCALHOST, port)).is_ok()
}

/// Answers one request line against the computer's gateway.
pub fn answer(running: &Running, request: &Value) -> Value {
    let op = request
        .get("op")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mut gateway = running.gateway();
    let ports = match op {
        "ports_list" => gateway.ports(),
        "port_add" => {
            let (Some(guest), host) = (
                port_of(request, "guest_port"),
                port_of(request, "host_port").unwrap_or(0),
            ) else {
                return error("A port from 1 to 65535 is needed.");
            };
            if guest == 0 {
                return error("A port from 1 to 65535 is needed.");
            }
            if host != 0 && host < 1024 {
                return error("Permission denied: choose a port above 1023.");
            }
            let already = gateway.ports().is_ok_and(|ports| {
                ports
                    .iter()
                    .any(|port| port.guest_port == guest && port.host_port == host)
            });
            if !already && !host_port_free(host) {
                return error("Address already in use.");
            }
            gateway.add_port(guest, host)
        }
        "port_remove" => match port_of(request, "guest_port") {
            Some(guest) if guest != 0 => gateway.remove_port(guest),
            _ => return error("A port from 1 to 65535 is needed."),
        },
        "port_probe" => {
            return match port_of(request, "guest_port") {
                Some(guest) if guest != 0 => match gateway.probe_port(guest) {
                    Ok(reachable) => json!({"ok": true, "reachable": reachable}),
                    Err(message) => error(&message),
                },
                _ => error("A port from 1 to 65535 is needed."),
            }
        }
        _ => return error("Unknown port control request."),
    };
    match ports {
        Ok(ports) => ports_value(&ports),
        Err(message) => error(&message),
    }
}

fn serve_one(running: &Running, stream: UnixStream) -> std::io::Result<()> {
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    let mut line = Vec::new();
    BufReader::new((&stream).take(MAX_REQUEST_BYTES + 1)).read_until(b'\n', &mut line)?;
    let response = if line.len() as u64 > MAX_REQUEST_BYTES || line.last() != Some(&b'\n') {
        error("The port control request is not valid.")
    } else {
        match serde_json::from_slice::<Value>(&line) {
            Ok(request) => answer(running, &request),
            Err(_) => error("The port control request is not valid."),
        }
    };
    let mut stream = stream;
    serde_json::to_writer(&mut stream, &response)?;
    stream.write_all(b"\n")
}

/// Starts serving the computer's port control at its socket. The socket exists when this
/// returns. The thread ends when the computer's gateway is dropped from the registry.
pub fn serve(paths: &RuntimePaths, name: &str, running: &Arc<Running>) -> Result<(), String> {
    let socket = crate::network::socket_path(paths, name);
    if let Some(directory) = socket.parent() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(directory)
            .map_err(|_| "Silo could not prepare the computer's port control.".to_owned())?;
    }
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket)
        .map_err(|_| "Silo could not open the computer's port control.".to_owned())?;
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))
        .and_then(|()| listener.set_nonblocking(true))
        .map_err(|_| "Silo could not open the computer's port control.".to_owned())?;
    let weak: Weak<Running> = Arc::downgrade(running);
    std::thread::Builder::new()
        .name("silo-ports".into())
        .spawn(move || {
            while let Some(running) = weak.upgrade() {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let _ = stream.set_nonblocking(false);
                        let _ = serve_one(&running, stream);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        drop(running);
                        std::thread::sleep(POLL);
                    }
                    Err(_) => break,
                }
            }
        })
        .map(|_| ())
        .map_err(|_| "Silo could not serve the computer's port control.".to_owned())
}

/// Removes the computer's port control socket (the computer stopped).
pub fn remove(paths: &RuntimePaths, name: &str) {
    remove_socket(&crate::network::socket_path(paths, name));
}

fn remove_socket(socket: &Path) {
    let _ = std::fs::remove_file(socket);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_name_ports_in_range_and_unknown_ones_are_refused() {
        assert_eq!(
            port_of(&json!({"guest_port": 3000}), "guest_port"),
            Some(3000)
        );
        assert_eq!(port_of(&json!({"guest_port": 70000}), "guest_port"), None);
        assert_eq!(port_of(&json!({"guest_port": "3000"}), "guest_port"), None);
        assert_eq!(port_of(&json!({}), "guest_port"), None);
    }

    #[test]
    fn the_answer_has_the_shape_the_port_settings_read() {
        let value = ports_value(&[PublishedPort {
            host_port: 40001,
            guest_port: 3000,
            protocol: "tcp".into(),
        }]);
        assert_eq!(value["ok"], true);
        assert_eq!(value["ports"][0]["guest_port"], 3000);
        assert_eq!(value["ports"][0]["host_port"], 40001);
        assert_eq!(value["ports"][0]["host_bind"], "127.0.0.1");
    }

    #[test]
    fn a_taken_host_port_is_reported_and_zero_is_always_free() {
        let held = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = held.local_addr().unwrap().port();
        assert!(!host_port_free(port));
        assert!(host_port_free(0));
        drop(held);
        assert!(host_port_free(port));
    }
}
