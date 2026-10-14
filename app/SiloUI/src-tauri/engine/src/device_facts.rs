//! Facts about this Mac that clients read with `device.facts`: the ceilings Silo accepts for
//! a computer's resources, and whether Remote Login is on. The slower dependency check is a
//! separate method (`device.dependencies`).

use std::{
    io::ErrorKind,
    net::{SocketAddr, TcpStream},
    time::Duration,
};

use silo_api::methods::{
    computers::DeviceCapacity,
    device::{DeviceFacts, MacosHostLimits, RemoteLoginStatus},
};

/// A measurement that may fail: `None` when the Mac cannot tell.
fn physical_memory_bytes() -> Option<u64> {
    let mut value = 0_u64;
    let mut length = std::mem::size_of::<u64>();
    // SAFETY: the name is NUL-terminated and both output pointers refer to writable values
    // of the declared length for the duration of the call.
    let status = unsafe {
        libc::sysctlbyname(
            c"hw.memsize".as_ptr(),
            (&raw mut value).cast(),
            &mut length,
            std::ptr::null_mut(),
            0,
        )
    };
    (status == 0 && length == std::mem::size_of::<u64>() && value > 0).then_some(value)
}

/// The limits for Linux computers: logical CPUs and physical memory, `None` when either
/// cannot be measured (every computer change is then rejected).
pub fn device_capacity() -> Option<DeviceCapacity> {
    let logical_cpus = std::thread::available_parallelism().ok()?.get();
    let physical_memory_bytes = physical_memory_bytes()?;
    Some(DeviceCapacity {
        logical_cpus,
        physical_memory_bytes,
        max_memory_gib: physical_memory_bytes / (1024 * 1024 * 1024),
    })
}

/// The limits for macOS computers, with the fallback values the macOS engine uses when
/// the host cannot be measured. `None` where macOS computers are not supported.
pub fn macos_host_limits() -> Option<MacosHostLimits> {
    if !cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        return None;
    }
    Some(MacosHostLimits {
        cpus: std::thread::available_parallelism().map_or(2, |count| count.get() as u64),
        memory_gib: physical_memory_bytes().map_or(8, |bytes| bytes >> 30),
    })
}

/// Remote Login without enabling it or needing an administrator: a connection to the
/// loopback SSH port. An accepted connection is `enabled`, a refusal is `disabled`, and
/// anything else (a timeout, a filter) is `unknown` rather than a guess.
pub fn remote_login() -> RemoteLoginStatus {
    remote_login_at(SocketAddr::from(([127, 0, 0, 1], 22)))
}

fn remote_login_at(address: SocketAddr) -> RemoteLoginStatus {
    match TcpStream::connect_timeout(&address, Duration::from_secs(2)) {
        Ok(_) => RemoteLoginStatus::Enabled,
        Err(error) if error.kind() == ErrorKind::ConnectionRefused => RemoteLoginStatus::Disabled,
        Err(_) => RemoteLoginStatus::Unknown,
    }
}

pub fn read() -> DeviceFacts {
    DeviceFacts {
        device_capacity: device_capacity(),
        macos_host_limits: macos_host_limits(),
        remote_login: remote_login(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn remote_login_reads_a_listening_port_as_enabled() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        assert_eq!(remote_login_at(address), RemoteLoginStatus::Enabled);
    }

    #[test]
    fn remote_login_reads_a_refused_port_as_disabled_not_unknown() {
        let address = {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap()
        };
        assert_eq!(remote_login_at(address), RemoteLoginStatus::Disabled);
    }

    #[test]
    fn capacity_and_limits_follow_the_measured_host() {
        let capacity = device_capacity().expect("this Mac reports its memory");
        assert!(capacity.logical_cpus > 0);
        assert_eq!(
            capacity.max_memory_gib,
            capacity.physical_memory_bytes / (1024 * 1024 * 1024)
        );
        if let Some(limits) = macos_host_limits() {
            assert_eq!(limits.memory_gib, capacity.physical_memory_bytes >> 30);
        }
    }
}
