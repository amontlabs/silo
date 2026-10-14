use std::{
    collections::HashMap,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    sync::{Mutex, MutexGuard, OnceLock, PoisonError},
};

#[derive(Clone, Copy, PartialEq, Eq)]
struct EndpointSet {
    vm_generation: u64,
    helper_generation: u64,
    ssh: SocketAddrV4,
    screen: SocketAddrV4,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RouteState {
    Gateway(EndpointSet),
    GatewayUnavailable {
        vm_generation: u64,
        helper_generation: u64,
    },
}

static ENDPOINTS: OnceLock<Mutex<HashMap<String, RouteState>>> = OnceLock::new();

fn endpoints() -> &'static Mutex<HashMap<String, RouteState>> {
    ENDPOINTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lock() -> MutexGuard<'static, HashMap<String, RouteState>> {
    endpoints().lock().unwrap_or_else(PoisonError::into_inner)
}

/// Publish only loopback listeners owned by the current helper generation.
pub(in crate::macos_computers_engine) fn publish(
    computer_id: &str,
    vm_generation: u64,
    helper_generation: u64,
    ssh_port: u16,
    screen_port: u16,
) -> Result<(), String> {
    if ssh_port == 0 || screen_port == 0 || ssh_port == screen_port {
        return Err("The computer proxy did not reserve private service ports.".into());
    }
    let endpoint = EndpointSet {
        vm_generation,
        helper_generation,
        ssh: SocketAddrV4::new(Ipv4Addr::LOCALHOST, ssh_port),
        screen: SocketAddrV4::new(Ipv4Addr::LOCALHOST, screen_port),
    };
    let mut endpoints = lock();
    if endpoints.contains_key(computer_id) {
        return Err("A computer proxy endpoint is already active.".into());
    }
    endpoints.insert(computer_id.to_owned(), RouteState::Gateway(endpoint));
    Ok(())
}

/// Mark a gateway-required guest unavailable without allowing NAT fallback.
pub(in crate::macos_computers_engine) fn mark_unavailable(
    computer_id: &str,
    vm_generation: u64,
    helper_generation: u64,
) {
    let mut endpoints = lock();
    if endpoints.get(computer_id).is_some_and(|state| match state {
        RouteState::Gateway(endpoint) => {
            endpoint.vm_generation == vm_generation
                && endpoint.helper_generation == helper_generation
        }
        RouteState::GatewayUnavailable {
            vm_generation: current_vm,
            helper_generation: current_helper,
        } => *current_vm == vm_generation && *current_helper == helper_generation,
    }) {
        endpoints.insert(
            computer_id.to_owned(),
            RouteState::GatewayUnavailable {
                vm_generation,
                helper_generation,
            },
        );
    }
}

/// Remove a listener only when the stopping VM still owns the published generation.
pub(in crate::macos_computers_engine) fn remove(
    computer_id: &str,
    vm_generation: u64,
    helper_generation: u64,
) {
    let mut endpoints = lock();
    if endpoints.get(computer_id).is_some_and(|state| match state {
        RouteState::Gateway(endpoint) => {
            endpoint.vm_generation == vm_generation
                && endpoint.helper_generation == helper_generation
        }
        RouteState::GatewayUnavailable {
            vm_generation: current_vm,
            helper_generation: current_helper,
        } => *current_vm == vm_generation && *current_helper == helper_generation,
    }) {
        endpoints.remove(computer_id);
    }
}

/// Resolve the current route for one guest service. `None` means setup/recovery NAT;
/// an error means this computer requires a gateway which is no longer available.
pub(in crate::macos_computers_engine) fn endpoint(
    computer_id: &str,
    guest_port: u16,
) -> Result<Option<SocketAddr>, String> {
    let endpoints = lock();
    let Some(state) = endpoints.get(computer_id) else {
        return Ok(None);
    };
    match state {
        RouteState::GatewayUnavailable { .. } => {
            Err("The computer network gateway is unavailable.".into())
        }
        RouteState::Gateway(endpoint) => match guest_port {
            22 => Ok(Some(SocketAddr::V4(endpoint.ssh))),
            5900 => Ok(Some(SocketAddr::V4(endpoint.screen))),
            _ => Err("This computer service has no private gateway endpoint.".into()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_are_loopback_scoped_and_stale_stop_cannot_remove_new_generation() {
        let id = "endpoint-generation-test";
        assert!(publish(id, 10, 2, 43211, 43212).is_ok());
        assert_eq!(
            endpoint(id, 22).unwrap(),
            Some(SocketAddr::from(([127, 0, 0, 1], 43211)))
        );
        assert_eq!(
            endpoint(id, 5900).unwrap(),
            Some(SocketAddr::from(([127, 0, 0, 1], 43212)))
        );
        assert!(endpoint(id, 80).is_err());
        assert!(publish(id, 11, 1, 43213, 43214).is_err());
        remove(id, 10, 1);
        assert!(endpoint(id, 22).unwrap().is_some());
        remove(id, 10, 2);
        assert_eq!(endpoint(id, 22).unwrap(), None);
    }

    #[test]
    fn rejects_unassigned_and_duplicate_private_ports() {
        assert!(publish("zero", 1, 1, 0, 2).is_err());
        assert!(publish("same", 1, 1, 3, 3).is_err());
    }

    #[test]
    fn a_failed_gateway_does_not_fall_back_to_nat() {
        let id = "endpoint-unavailable-test";
        publish(id, 20, 1, 43215, 43216).unwrap();
        mark_unavailable(id, 20, 1);
        assert!(endpoint(id, 22).is_err());
        remove(id, 20, 1);
        assert_eq!(endpoint(id, 22).unwrap(), None);
    }
}
