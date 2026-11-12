//! Live-check harness: starts one container gateway through `silo_engine::computer_gateway`
//! for a TAP descriptor it inherits, and answers a few commands on stdin. Used by
//! `scripts/gateway-live-check.py --harness`; it is not part of the product.
//!
//! Usage: `gateway_tap_harness --gateway PATH --tap-fd N`, then one JSON line of settings on
//! stdin. It prints the gateway's `ready` as one JSON line, then answers `stats` and
//! `shutdown` lines (a JSON counters line; nothing, then exit).

use serde::Deserialize;
use silo_engine::computer_gateway::{generate_ca, Gateway, Params, Secret};
use std::{
    io::{BufRead, Write},
    net::Ipv4Addr,
    os::fd::{FromRawFd, OwnedFd},
    path::PathBuf,
};
use zeroize::Zeroizing;

#[derive(Deserialize)]
struct Settings {
    slot: u16,
    guest_ipv4: Ipv4Addr,
    gateway_ipv4: Ipv4Addr,
    guest_mac: [u8; 6],
    mtu: u16,
    published_ports: Vec<u16>,
    secrets: Vec<SecretSettings>,
    usage_log: Option<PathBuf>,
}

#[derive(Deserialize)]
struct SecretSettings {
    name: String,
    value: String,
    allowed_hosts: Vec<String>,
}

fn main() -> Result<(), String> {
    let mut gateway_path = None;
    let mut tap_fd = None;
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--gateway" => gateway_path = arguments.next().map(PathBuf::from),
            "--tap-fd" => tap_fd = arguments.next().and_then(|value| value.parse::<i32>().ok()),
            _ => return Err(format!("unknown argument {argument}")),
        }
    }
    let gateway_path = gateway_path.ok_or("--gateway is required")?;
    let tap_fd = tap_fd.ok_or("--tap-fd is required")?;
    // SAFETY: the caller hands this descriptor over for the harness to own.
    let tap = unsafe { OwnedFd::from_raw_fd(tap_fd) };

    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    let first = lines
        .next()
        .ok_or("no settings on stdin")?
        .map_err(|e| e.to_string())?;
    let settings: Settings = serde_json::from_str(&first).map_err(|e| e.to_string())?;
    let (cert, key) = generate_ca(&gateway_path)?;
    let mut gateway = Gateway::start(
        &gateway_path,
        tap,
        &cert,
        &key,
        Params {
            slot: settings.slot,
            guest_ipv4: settings.guest_ipv4,
            gateway_ipv4: settings.gateway_ipv4,
            guest_mac: settings.guest_mac,
            mtu: settings.mtu,
            published_ports: settings.published_ports,
            secrets: settings
                .secrets
                .into_iter()
                .map(|secret| Secret {
                    name: secret.name,
                    value: Zeroizing::new(secret.value),
                    allowed_hosts: secret.allowed_hosts,
                })
                .collect(),
            github_profile_json: None,
            github_hosts: None,
            policy: None,
            strict: None,
            usage_log: settings.usage_log,
            log: None,
        },
    )?;
    let mut out = std::io::stdout();
    writeln!(
        out,
        "{}",
        serde_json::to_string(gateway.ready()).map_err(|e| e.to_string())?
    )
    .and_then(|()| out.flush())
    .map_err(|e| e.to_string())?;
    for line in lines {
        match line.map_err(|e| e.to_string())?.trim() {
            "stats" => writeln!(out, "{}", gateway.stats()?)
                .and_then(|()| out.flush())
                .map_err(|e| e.to_string())?,
            "shutdown" => break,
            _ => return Err("unknown command".into()),
        }
    }
    gateway.shutdown();
    Ok(())
}
