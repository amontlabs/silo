mod control;
mod frames;
mod network;

use std::ffi::OsStr;
use std::io::{self, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::net::{UnixDatagram, UnixStream};
use std::time::Duration;

use control::{ControlDecoder, ErrorResponse, Request, Response, UpdateOperation};
use frames::{FramePump, FrameStats};
use network::{
    NetworkResources, SecretsHandle, SmoltcpNetwork, build_network, generate_ca, validate_init,
};
use serde::Serialize;
use zeroize::Zeroize;

const FRAME_FD: i32 = 3;
const CONTROL_FD: i32 = 4;
const POLICY_CLOSE_TIMEOUT: Duration = Duration::from_secs(5);

fn main() {
    network::install_crypto_provider();
    let mut arguments = std::env::args_os().skip(1);
    match (arguments.next(), arguments.next()) {
        (None, None) => {
            if run().is_err() {
                std::process::exit(1);
            }
        }
        (Some(mode), None) if mode == OsStr::new("--generate-ca") => {
            if generate_ca_mode().is_err() {
                std::process::exit(1);
            }
        }
        (Some(mode), None) if mode == OsStr::new("--issue-trust-probe") => {
            if issue_trust_probe_mode().is_err() {
                std::process::exit(1);
            }
        }
        _ => std::process::exit(64),
    }
}

/// Reads a length-prefixed CA bundle on stdin and writes a length-prefixed trust-probe leaf
/// certificate on stdout. The leaf's key is never kept.
fn issue_trust_probe_mode() -> Result<(), GatewayError> {
    let ca = control::read_ca_input(&mut io::stdin().lock()).map_err(|_| GatewayError::Control)?;
    let leaf = network::issue_trust_probe(&ca).map_err(|_| GatewayError::NetworkInit)?;
    #[derive(Serialize)]
    struct Probe<'a> {
        leaf_pem: &'a str,
    }
    let body = serde_json::to_vec(&Probe { leaf_pem: &leaf }).map_err(|_| GatewayError::Control)?;
    if body.is_empty() || body.len() > control::MAX_MESSAGE_BYTES {
        return Err(GatewayError::Control);
    }
    io::stdout()
        .write_all(&(body.len() as u32).to_be_bytes())
        .and_then(|_| io::stdout().write_all(&body))
        .and_then(|_| io::stdout().flush())
        .map_err(|_| GatewayError::Control)
}

fn generate_ca_mode() -> Result<(), GatewayError> {
    let ca = generate_ca().map_err(|_| GatewayError::NetworkInit)?;
    #[derive(Serialize)]
    struct CaBundle<'a> {
        cert_pem: &'a str,
        key_pem: &'a str,
    }
    let response = CaBundle {
        cert_pem: &ca.cert_pem,
        key_pem: ca.key_pem.as_str(),
    };
    let mut body = serde_json::to_vec(&response).map_err(|_| GatewayError::Control)?;
    if body.is_empty() || body.len() > control::MAX_MESSAGE_BYTES {
        body.zeroize();
        return Err(GatewayError::Control);
    }
    let prefix = (body.len() as u32).to_be_bytes();
    let result = io::stdout()
        .write_all(&prefix)
        .and_then(|_| io::stdout().write_all(&body))
        .and_then(|_| io::stdout().flush());
    body.zeroize();
    result.map_err(|_| GatewayError::Control)
}

fn run() -> Result<(), GatewayError> {
    let frame_socket = unsafe { UnixDatagram::from_raw_fd(FRAME_FD) };
    let mut control = unsafe { UnixStream::from_raw_fd(CONTROL_FD) };

    let init = match control::read_init(&mut control) {
        Ok(init) => init,
        Err(_) => {
            let _ = control::write_response(
                &mut control,
                &Response::Error(ErrorResponse::for_revision(None, "invalid_init")),
            );
            return Err(GatewayError::Control);
        }
    };
    if validate_init(&init).is_err() {
        let _ = control::write_response(
            &mut control,
            &Response::Error(ErrorResponse::for_revision(None, "invalid_configuration")),
        );
        return Err(GatewayError::InvalidConfiguration);
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|_| GatewayError::Runtime)?;
    let NetworkResources {
        mut network,
        secrets,
        mut ready,
    } = match build_network(init) {
        Ok(resources) => resources,
        Err(_) => {
            let _ = control::write_response(
                &mut control,
                &Response::Error(ErrorResponse::for_revision(None, "network_init_failed")),
            );
            return Err(GatewayError::NetworkInit);
        }
    };
    let requested_ports = std::mem::take(&mut ready.loopback_ports);
    let bound_ports =
        match network.start_and_wait_for_ports(runtime.handle().clone(), POLICY_CLOSE_TIMEOUT) {
            Ok(ports) => ports,
            Err(_) => {
                let _ = control::write_response(
                    &mut control,
                    &Response::Error(ErrorResponse::for_revision(None, "port_bind_timeout")),
                );
                return fail_closed(&frame_socket, GatewayError::PortBindTimeout);
            }
        };
    ready.loopback_ports = match network::map_bound_ports(&requested_ports, bound_ports) {
        Ok(ports) => ports,
        Err(_) => {
            let _ = control::write_response(
                &mut control,
                &Response::Error(ErrorResponse::for_revision(None, "loopback_bind_failed")),
            );
            return fail_closed(&frame_socket, GatewayError::PortBindTimeout);
        }
    };
    let backend = network.take_backend();
    let stats = FrameStats::default();
    let mut frame_pump = FramePump::new(
        frame_socket.as_raw_fd(),
        ready.guest_ipv4,
        ready.guest_mac,
        ready.mtu,
        &stats,
        backend,
    );

    control
        .set_nonblocking(true)
        .map_err(|_| GatewayError::Control)?;
    control::write_response(&mut control, &Response::Ready(ready))
        .map_err(|_| GatewayError::Control)?;

    let mut decoder = ControlDecoder::default();
    let mut revision = 0u64;
    loop {
        let buffered = decoder
            .decode_buffered()
            .map_err(|_| GatewayError::Control)?;
        let had_buffered = !buffered.is_empty();
        if had_buffered
            && !dispatch_requests(
                buffered,
                &mut control,
                &runtime,
                &secrets,
                &network,
                &stats,
                &frame_socket,
                &mut revision,
            )?
        {
            return Ok(());
        }
        if had_buffered {
            continue;
        }

        let control_fd = control.as_raw_fd();
        let events = frame_pump.poll_events();
        let mut pollfds = [
            events.frame,
            events.backend,
            libc::pollfd {
                fd: control_fd,
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        let ready_count = unsafe { libc::poll(pollfds.as_mut_ptr(), pollfds.len() as _, -1) };
        if ready_count < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                stats.interrupted_polls();
                continue;
            }
            return fail_closed(&frame_socket, GatewayError::Poll);
        }

        let control_revents = pollfds[2].revents;
        if pollfds[0].revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0 {
            return fail_closed(&frame_socket, GatewayError::FramePeerClosed);
        }
        if control_revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0 {
            return fail_closed(&frame_socket, GatewayError::ControlPeerClosed);
        }

        if frame_pump
            .service(pollfds[0].revents, pollfds[1].revents)
            .is_err()
        {
            return fail_closed(&frame_socket, GatewayError::FramePump);
        }

        if control_revents & libc::POLLIN != 0 {
            let messages = decoder
                .read_available(&mut control)
                .map_err(|_| GatewayError::Control)?;
            if !dispatch_requests(
                messages,
                &mut control,
                &runtime,
                &secrets,
                &network,
                &stats,
                &frame_socket,
                &mut revision,
            )? {
                return Ok(());
            }
        }
    }
}

fn dispatch_requests(
    requests: Vec<Request>,
    control: &mut UnixStream,
    runtime: &tokio::runtime::Runtime,
    secrets: &SecretsHandle,
    network: &SmoltcpNetwork,
    stats: &FrameStats,
    frame_socket: &UnixDatagram,
    revision: &mut u64,
) -> Result<bool, GatewayError> {
    for request in requests {
        match request {
            Request::Update(update) => {
                if update.version != control::PROTOCOL_VERSION || update.revision <= *revision {
                    control::write_response(
                        control,
                        &Response::Error(ErrorResponse::for_revision(
                            Some(update.revision),
                            "invalid_revision",
                        )),
                    )
                    .map_err(|_| GatewayError::Control)?;
                    continue;
                }
                if network::validate_update(&update.operation).is_err() {
                    control::write_response(
                        control,
                        &Response::Error(ErrorResponse::for_revision(
                            Some(update.revision),
                            "update_rejected",
                        )),
                    )
                    .map_err(|_| GatewayError::Control)?;
                    continue;
                }
                match apply_update(runtime, secrets, update.operation, POLICY_CLOSE_TIMEOUT) {
                    Ok(()) => {
                        *revision = update.revision;
                        control::write_response(
                            control,
                            &Response::Ack {
                                version: control::PROTOCOL_VERSION,
                                revision: *revision,
                                closed_previous_generation: true,
                            },
                        )
                        .map_err(|_| GatewayError::Control)?;
                    }
                    Err(UpdateFailure::CloseTimeout) => {
                        let _ = control::write_response(
                            control,
                            &Response::Error(ErrorResponse::for_revision(
                                Some(update.revision),
                                "connection_close_timeout",
                            )),
                        );
                        return fail_closed(frame_socket, GatewayError::CloseTimeout);
                    }
                    Err(UpdateFailure::Rejected) => {
                        control::write_response(
                            control,
                            &Response::Error(ErrorResponse::for_revision(
                                Some(update.revision),
                                "update_rejected",
                            )),
                        )
                        .map_err(|_| GatewayError::Control)?;
                    }
                }
            }
            Request::Stats { version } if version == control::PROTOCOL_VERSION => {
                control::write_response(
                    control,
                    &Response::Stats {
                        version: control::PROTOCOL_VERSION,
                        counters: stats
                            .snapshot_with_backend_tx_ring_drops(network.tx_ring_overflows()),
                    },
                )
                .map_err(|_| GatewayError::Control)?;
            }
            Request::Shutdown { version } if version == control::PROTOCOL_VERSION => {
                control::write_response(
                    control,
                    &Response::Stopped {
                        version: control::PROTOCOL_VERSION,
                    },
                )
                .map_err(|_| GatewayError::Control)?;
                return Ok(false);
            }
            _ => {
                control::write_response(
                    control,
                    &Response::Error(ErrorResponse::for_revision(None, "invalid_request")),
                )
                .map_err(|_| GatewayError::Control)?;
            }
        }
    }
    Ok(true)
}

fn apply_update(
    runtime: &tokio::runtime::Runtime,
    secrets: &SecretsHandle,
    operation: UpdateOperation,
    timeout: Duration,
) -> Result<(), UpdateFailure> {
    let update = async {
        match operation {
            UpdateOperation::PutSecret {
                name,
                mut value,
                placeholder,
                allowed_hosts,
            } => secrets
                .put_secret_and_wait(
                    &name,
                    std::mem::take(&mut *value),
                    placeholder,
                    allowed_hosts,
                )
                .await
                .map_err(|_| UpdateFailure::Rejected),
            UpdateOperation::RotateSecret { name, mut value } => secrets
                .rotate_value_and_wait(&name, std::mem::take(&mut *value))
                .await
                .map_err(|_| UpdateFailure::Rejected),
            UpdateOperation::RemoveSecret { name } => secrets
                .remove_and_wait(&name)
                .await
                .map_err(|_| UpdateFailure::Rejected),
            UpdateOperation::SetAllowedHosts {
                name,
                allowed_hosts,
            } => secrets
                .set_allowed_hosts_and_wait(&name, &allowed_hosts)
                .await
                .map_err(|_| UpdateFailure::Rejected),
            UpdateOperation::SetGithubProfile { mut profile_json } => secrets
                .set_github_profile_and_wait(
                    profile_json
                        .take()
                        .map(|mut profile| std::mem::take(&mut *profile)),
                )
                .await
                .map_err(|_| UpdateFailure::Rejected),
        }
    };

    match runtime.block_on(async { tokio::time::timeout(timeout, update).await }) {
        Ok(result) => result,
        Err(_) => Err(UpdateFailure::CloseTimeout),
    }
}

fn fail_closed<T>(frame_socket: &UnixDatagram, error: GatewayError) -> Result<T, GatewayError> {
    let _ = unsafe { libc::shutdown(frame_socket.as_raw_fd(), libc::SHUT_RDWR) };
    Err(error)
}

#[derive(Debug)]
enum UpdateFailure {
    Rejected,
    CloseTimeout,
}

#[derive(Debug)]
enum GatewayError {
    Control,
    InvalidConfiguration,
    Runtime,
    NetworkInit,
    PortBindTimeout,
    Poll,
    FramePeerClosed,
    ControlPeerClosed,
    FramePump,
    CloseTimeout,
}

#[cfg(test)]
mod tests {
    use super::*;
    use control::{CaInput, InitRequest, LoopbackPort, PortProtocol};
    use microsandbox_types::HostPattern;
    use network::{GeneratedCa, generate_ca};
    use zeroize::Zeroizing;

    #[test]
    fn live_secret_update_runs_inside_the_runtime_and_changes_the_policy() {
        let GeneratedCa { cert_pem, key_pem } = generate_ca().unwrap();
        let init = InitRequest {
            version: control::PROTOCOL_VERSION,
            slot: 1,
            guest_ipv4: "172.30.0.2".parse().unwrap(),
            guest_mac: [2, 0, 0, 0, 0, 1],
            mtu: 1500,
            policy: None,
            dns_nameservers: Vec::new(),
            intercepted_ports: vec![443],
            loopback_ports: vec![
                LoopbackPort {
                    host_port: 0,
                    guest_port: 22,
                    protocol: PortProtocol::Tcp,
                },
                LoopbackPort {
                    host_port: 0,
                    guest_port: 5900,
                    protocol: PortProtocol::Tcp,
                },
            ],
            ca: CaInput {
                cert_pem: Zeroizing::new(cert_pem),
                key_pem: key_pem,
            },
            secrets: Vec::new(),
            github_profile_json: None,
        };
        let resources = build_network(init).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        apply_update(
            &runtime,
            &resources.secrets,
            UpdateOperation::PutSecret {
                name: "API_KEY".into(),
                value: Zeroizing::new("synthetic-value".into()),
                placeholder: "$MSB_API_KEY".into(),
                allowed_hosts: vec!["*".into()],
            },
            Duration::from_secs(1),
        )
        .unwrap();

        let snapshot = resources.secrets.load();
        assert_eq!(snapshot.secrets.len(), 1);
        assert_eq!(snapshot.secrets[0].env_var, "API_KEY");
        assert_eq!(snapshot.secrets[0].value.as_str(), "synthetic-value");
        assert!(matches!(
            snapshot.secrets[0].allowed_hosts.as_slice(),
            [HostPattern::Any]
        ));
    }
}
