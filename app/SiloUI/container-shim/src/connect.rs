//! `silo-ctr connect --state DIR --host HOST --port N`: opens a TCP connection to the container's
//! own loopback and relays it over standard input and output.
//!
//! The engine's SSH server answers `direct-tcpip` channels (`ssh -L`, `ssh -D`, an editor's port
//! forwarding) with it. A container has no agent to ask, and the SSH server runs in the host's
//! network, so the connection is made from inside the holder's network namespace, where the
//! container's `127.0.0.1` is. Only a loopback host is accepted, here as in the server.
//!
//! After the connection is made one zero byte is written to standard output, so the caller knows
//! the channel may be opened; a refused connection prints the reason on standard error and exits 1.
//! Then standard input is copied to the connection and the connection to standard output until the
//! connection ends.

use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream};

/// The address a `--host` names inside the container: `localhost`, or a loopback address.
pub fn loopback_addresses(host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
    if port == 0 {
        return Err("port 0 is not a destination".into());
    }
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if host.eq_ignore_ascii_case("localhost") {
        return Ok(vec![
            SocketAddr::new(IpAddr::from([127, 0, 0, 1]), port),
            SocketAddr::new(IpAddr::from([0, 0, 0, 0, 0, 0, 0, 1]), port),
        ]);
    }
    match host.parse::<IpAddr>() {
        Ok(address) if address.is_loopback() => Ok(vec![SocketAddr::new(address, port)]),
        _ => Err(format!("{host} is not the container's loopback")),
    }
}

#[cfg(target_os = "linux")]
pub fn command(args: &[String]) -> Result<i32, String> {
    use crate::common::io_err;
    let (state, host, port) = parse(args)?;
    // Refused before any namespace is joined, so a bad request touches nothing.
    let addresses = loopback_addresses(&host, port)?;
    crate::enter::join_holder(&state)?;
    let mut last = None;
    let stream = addresses.iter().find_map(|address| {
        match TcpStream::connect_timeout(address, std::time::Duration::from_secs(10)) {
            Ok(stream) => Some(stream),
            Err(error) => {
                last = Some(error);
                None
            }
        }
    });
    let Some(stream) = stream else {
        let error = last.expect("an address was tried");
        return Err(io_err(&format!("connect to {host}:{port}"), &error));
    };
    let _ = stream.set_nodelay(true);
    let mut out = std::io::stdout().lock();
    out.write_all(&[0])
        .map_err(|error| io_err("report the connection", &error))?;
    out.flush()
        .map_err(|error| io_err("report the connection", &error))?;
    drop(out);
    relay(stream, std::io::stdin(), std::io::stdout());
    Ok(0)
}

#[cfg(not(target_os = "linux"))]
pub fn command(_: &[String]) -> Result<i32, String> {
    Err("Linux only".into())
}

/// Copies `input` to the connection and the connection to `output`, and returns when the
/// connection has ended (the other direction may still wait for input that never comes).
pub fn relay(stream: TcpStream, mut input: impl Read + Send + 'static, mut output: impl Write) {
    if let Ok(mut write) = stream.try_clone() {
        std::thread::spawn(move || {
            let mut buffer = vec![0u8; 32 * 1024];
            loop {
                match input.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => {
                        if write.write_all(&buffer[..read]).is_err() {
                            return;
                        }
                    }
                }
            }
            // The caller has nothing more to send; the other side may still answer.
            let _ = write.shutdown(std::net::Shutdown::Write);
        });
    }
    let mut read = stream;
    let mut buffer = vec![0u8; 32 * 1024];
    loop {
        match read.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(count) => {
                if output.write_all(&buffer[..count]).is_err() || output.flush().is_err() {
                    break;
                }
            }
        }
    }
}

fn parse(args: &[String]) -> Result<(String, String, u16), String> {
    const USAGE: &str = "usage: silo-ctr connect --state DIR --host HOST --port N";
    let [state_flag, state, host_flag, host, port_flag, port] = args else {
        return Err(USAGE.into());
    };
    if state_flag != "--state" || host_flag != "--host" || port_flag != "--port" {
        return Err(USAGE.into());
    }
    let port = port.parse::<u16>().map_err(|_| USAGE.to_owned())?;
    Ok((state.clone(), host.clone(), port))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| arg.to_string()).collect()
    }

    #[test]
    fn only_the_containers_loopback_is_a_destination() {
        for host in [
            "localhost",
            "LOCALHOST",
            "127.0.0.1",
            "127.1.2.3",
            "::1",
            "[::1]",
        ] {
            assert!(loopback_addresses(host, 80).is_ok(), "{host}");
        }
        for host in [
            "example.com",
            "10.0.0.1",
            "0.0.0.0",
            "192.168.1.5",
            "::",
            "::ffff:10.0.0.1",
            "",
            "localhost.example.com",
            "127.0.0.1 ",
        ] {
            assert!(loopback_addresses(host, 80).is_err(), "{host:?}");
        }
        assert!(loopback_addresses("127.0.0.1", 0).is_err());
    }

    #[test]
    fn the_arguments_are_exactly_the_state_the_host_and_the_port() {
        let (state, host, port) = parse(&args(&[
            "--state",
            "/s",
            "--host",
            "localhost",
            "--port",
            "8080",
        ]))
        .unwrap();
        assert_eq!(
            (state.as_str(), host.as_str(), port),
            ("/s", "localhost", 8080)
        );
        for bad in [
            args(&[]),
            args(&["--state", "/s", "--host", "h"]),
            args(&["--state", "/s", "--host", "h", "--port", "70000"]),
            args(&["--state", "/s", "--port", "1", "--host", "h"]),
        ] {
            assert!(parse(&bad).is_err());
        }
    }

    #[test]
    fn the_relay_copies_both_ways_and_ends_with_the_connection() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut peer, _) = listener.accept().unwrap();
            let mut received = Vec::new();
            // Echoes in upper case until the relay says it has no more input.
            let mut buffer = [0u8; 64];
            loop {
                let count = peer.read(&mut buffer).unwrap();
                if count == 0 {
                    break;
                }
                received.extend_from_slice(&buffer[..count]);
                peer.write_all(&buffer[..count].to_ascii_uppercase())
                    .unwrap();
            }
            received
        });
        let stream = TcpStream::connect(address).unwrap();
        let input = std::io::Cursor::new(b"hello relay".to_vec());
        let mut output = Vec::new();
        relay(stream, input, &mut output);
        assert_eq!(output, b"HELLO RELAY");
        assert_eq!(server.join().unwrap(), b"hello relay");
    }
}
