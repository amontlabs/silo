//! A container's published ports after the gateway started.
//!
//! The ports the computer names at `init` are bound before `ready`. A container's service can
//! start at any time, so its owner publishes and unpublishes guest TCP ports while the gateway
//! runs, through the network stack's own port control (the one MicroSandbox's runtime uses for
//! its live ports). Only a container's gateway answers these requests: a macOS VM publishes
//! exactly SSH and the screen. Every listener binds the host's IPv4 loopback.

use microsandbox_network::config::{PortProtocol as NetworkPortProtocol, PublishedPort};
use microsandbox_network::network::SmoltcpNetwork;

use crate::control::{
    ErrorResponse, LoopbackPort, PROTOCOL_VERSION, PortProtocol, Request, Response,
};
use crate::network::MAX_LOOPBACK_PORTS;

/// The lowest host port a caller may ask for; zero lets the gateway pick.
const MIN_REQUESTED_HOST_PORT: u16 = 1024;

/// Whether a request names ports the gateway will publish.
fn acceptable(request: &Request) -> bool {
    match request {
        Request::Ports { .. } => true,
        Request::PortAdd {
            guest_port,
            host_port,
            ..
        } => *guest_port != 0 && (*host_port == 0 || *host_port >= MIN_REQUESTED_HOST_PORT),
        Request::PortRemove { guest_port, .. } | Request::PortProbe { guest_port, .. } => {
            *guest_port != 0
        }
        _ => false,
    }
}

/// The TCP listeners as the protocol reports them; `None` for anything that is not a TCP
/// listener on the IPv4 loopback, or more than a container may publish.
fn to_loopback(published: Vec<PublishedPort>) -> Option<Vec<LoopbackPort>> {
    if published.len() > MAX_LOOPBACK_PORTS {
        return None;
    }
    published
        .into_iter()
        .map(|port| {
            (port.protocol == NetworkPortProtocol::Tcp
                && port.host_bind.is_loopback()
                && port.host_port != 0
                && port.guest_port != 0)
                .then_some(LoopbackPort {
                    host_port: port.host_port,
                    guest_port: port.guest_port,
                    protocol: PortProtocol::Tcp,
                })
        })
        .collect()
}

/// What `handle` asks of the network stack's port control (a trait so the order of its steps can
/// be tested).
trait Control {
    fn list(&self) -> Result<Vec<PublishedPort>, String>;
    fn add(&self, guest_port: u16, host_port: u16) -> Result<Vec<PublishedPort>, String>;
    fn remove(&self, guest_port: u16) -> Result<Vec<PublishedPort>, String>;
}

impl Control for microsandbox_network::network::PortControlHandle {
    fn list(&self) -> Result<Vec<PublishedPort>, String> {
        Self::list(self)
    }
    fn add(&self, guest_port: u16, host_port: u16) -> Result<Vec<PublishedPort>, String> {
        Self::add(self, guest_port, host_port)
    }
    fn remove(&self, guest_port: u16) -> Result<Vec<PublishedPort>, String> {
        Self::remove(self, guest_port)
    }
}

/// Answers a port request, or `None` when it is refused (the caller answers `port_rejected`).
pub fn handle(network: &SmoltcpNetwork, request: &Request) -> Option<Response> {
    if !acceptable(request) {
        return None;
    }
    answer(&network.port_control_handle()?, request)
}

fn answer(control: &impl Control, request: &Request) -> Option<Response> {
    // A port this request publishes, which it takes back if the result cannot be reported.
    let mut added = None;
    let listed = match request {
        Request::Ports { .. } => control.list(),
        Request::PortAdd {
            guest_port,
            host_port,
            ..
        } => {
            // A port beyond the container's cap is refused before it is bound.
            let known = control.list().ok()?;
            let published = known.iter().any(|port| port.guest_port == *guest_port);
            if known.len() >= MAX_LOOPBACK_PORTS && !published {
                return None;
            }
            if !published {
                added = Some(*guest_port);
            }
            control.add(*guest_port, *host_port)
        }
        Request::PortRemove { guest_port, .. } => control.remove(*guest_port),
        // A probe is a TCP handshake with the guest, which needs the frame pump to run: it is
        // answered by `probe_in_background`, never here on the pump's thread.
        _ => return None,
    };
    let reported = listed.ok().and_then(to_loopback);
    match reported {
        Some(ports) => Some(Response::Ports {
            version: PROTOCOL_VERSION,
            ports,
        }),
        None => {
            // The caller is told the request was refused, so the port must not stay published:
            // a listener nobody was told about is one nobody can take down.
            if let Some(guest_port) = added {
                let _ = control.remove(guest_port);
            }
            None
        }
    }
}

/// Starts a probe of a guest port and answers it on `control` when it ends. The probe waits for
/// the guest's reply to a connection attempt, and that reply reaches the network stack only
/// while the thread that called us keeps pumping frames, so the wait happens on its own thread.
/// The control link carries one request at a time, so nothing else writes to it meanwhile.
/// `false` when the request is refused (the caller answers `port_rejected`).
pub fn probe_in_background(
    network: &SmoltcpNetwork,
    request: &Request,
    control: &std::os::unix::net::UnixStream,
) -> bool {
    let Request::PortProbe { guest_port, .. } = request else {
        return false;
    };
    if !acceptable(request) {
        return false;
    }
    let Some(handle) = network.port_control_handle() else {
        return false;
    };
    let Ok(mut writer) = control.try_clone() else {
        return false;
    };
    let guest_port = *guest_port;
    std::thread::Builder::new()
        .name("port-probe".into())
        .spawn(move || {
            let response = match handle.probe(guest_port) {
                Ok(reachable) => Response::PortProbe {
                    version: PROTOCOL_VERSION,
                    reachable,
                },
                Err(_) => Response::Error(ErrorResponse::for_revision(None, "port_rejected")),
            };
            let _ = crate::control::write_response(&mut writer, &response);
        })
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    fn published(host_port: u16, guest_port: u16, bind: IpAddr) -> PublishedPort {
        PublishedPort {
            host_port,
            guest_port,
            protocol: NetworkPortProtocol::Tcp,
            host_bind: bind,
        }
    }

    #[test]
    fn requests_name_nonzero_guest_ports_and_unprivileged_host_ports() {
        let add = |guest_port, host_port| Request::PortAdd {
            version: PROTOCOL_VERSION,
            guest_port,
            host_port,
        };
        assert!(acceptable(&add(3000, 0)));
        assert!(acceptable(&add(3000, 40000)));
        assert!(!acceptable(&add(0, 0)));
        assert!(!acceptable(&add(3000, 80)));
        assert!(!acceptable(&Request::PortRemove {
            version: PROTOCOL_VERSION,
            guest_port: 0
        }));
        assert!(!acceptable(&Request::Shutdown {
            version: PROTOCOL_VERSION
        }));
    }

    /// A port control that publishes what it is asked and lists it with the bind it was given.
    struct Fake {
        ports: std::cell::RefCell<Vec<PublishedPort>>,
        bind: IpAddr,
    }

    impl Control for Fake {
        fn list(&self) -> Result<Vec<PublishedPort>, String> {
            Ok(self.ports.borrow().clone())
        }
        fn add(&self, guest_port: u16, host_port: u16) -> Result<Vec<PublishedPort>, String> {
            self.ports
                .borrow_mut()
                .push(published(host_port.max(40000), guest_port, self.bind));
            self.list()
        }
        fn remove(&self, guest_port: u16) -> Result<Vec<PublishedPort>, String> {
            self.ports
                .borrow_mut()
                .retain(|port| port.guest_port != guest_port);
            self.list()
        }
    }

    fn add_request(guest_port: u16) -> Request {
        Request::PortAdd {
            version: PROTOCOL_VERSION,
            guest_port,
            host_port: 0,
        }
    }

    #[test]
    fn a_port_that_cannot_be_reported_is_taken_back() {
        // The port is bound on all interfaces: the answer would be refused.
        let control = Fake {
            ports: Default::default(),
            bind: Ipv4Addr::UNSPECIFIED.into(),
        };
        assert!(answer(&control, &add_request(3000)).is_none());
        assert!(
            control.ports.borrow().is_empty(),
            "the port stayed published"
        );
        // A port that was already published is not taken away by a refused repeat.
        control
            .ports
            .borrow_mut()
            .push(published(40001, 3000, Ipv4Addr::LOCALHOST.into()));
        control
            .ports
            .borrow_mut()
            .push(published(40002, 4000, Ipv4Addr::UNSPECIFIED.into()));
        assert!(answer(&control, &add_request(3000)).is_none());
        assert!(
            control
                .ports
                .borrow()
                .iter()
                .any(|port| port.guest_port == 3000)
        );
    }

    #[test]
    fn a_port_on_the_loopback_is_published_and_reported() {
        let control = Fake {
            ports: Default::default(),
            bind: Ipv4Addr::LOCALHOST.into(),
        };
        let Some(Response::Ports { ports, .. }) = answer(&control, &add_request(3000)) else {
            panic!("a loopback port is reported");
        };
        assert_eq!(ports.len(), 1);
        assert_eq!(control.ports.borrow().len(), 1);
    }

    #[test]
    fn only_tcp_listeners_on_the_loopback_are_reported() {
        let loopback = IpAddr::V4(Ipv4Addr::LOCALHOST);
        let ports = to_loopback(vec![published(40001, 3000, loopback)]).unwrap();
        assert_eq!(ports[0].host_port, 40001);
        assert_eq!(ports[0].guest_port, 3000);
        assert!(to_loopback(vec![published(40001, 3000, Ipv4Addr::UNSPECIFIED.into())]).is_none());
        assert!(to_loopback(vec![published(0, 3000, loopback)]).is_none());
        let mut udp = published(40001, 3000, loopback);
        udp.protocol = NetworkPortProtocol::Udp;
        assert!(to_loopback(vec![udp]).is_none());
        let many = (1..=MAX_LOOPBACK_PORTS as u16 + 1)
            .map(|port| published(40000 + port, port, loopback))
            .collect();
        assert!(to_loopback(many).is_none());
    }
}
