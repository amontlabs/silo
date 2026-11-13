//! The container's network: the engine's configuration lines, and the interface set-up done with ioctls
//! alone (no `ip` program): `lo` up, the TAP device, its address, its default route.

use std::io::BufRead;
use std::net::Ipv4Addr;

// The route structure below is laid out for a 64-bit `long`.
const _: () = assert!(std::mem::size_of::<usize>() == 8);

pub const TAP_NAME: &str = "tap0";
const ARPHRD_ETHER: u16 = 1;
const AF_INET: u16 = 2;
const RTF_UP_GATEWAY: u16 = 0x0001 | 0x0002;
const MAX_CONFIG_LINES: usize = 64;

/// What the engine says the TAP device looks like.
#[derive(Debug, PartialEq)]
pub struct NetConfig {
    pub mac: [u8; 6],
    pub mtu: u32,
    pub address: Ipv4Addr,
    pub prefix_len: u8,
    pub gateway: Option<Ipv4Addr>,
}

pub fn parse_mac(text: &str) -> Result<[u8; 6], String> {
    let bad = || format!("mac {text:?} is not aa:bb:cc:dd:ee:ff");
    let mut mac = [0u8; 6];
    let mut parts = text.split(':');
    for byte in mac.iter_mut() {
        let part = parts.next().ok_or_else(bad)?;
        if part.len() != 2 {
            return Err(bad());
        }
        *byte = u8::from_str_radix(part, 16).map_err(|_| bad())?;
    }
    if parts.next().is_some() {
        return Err(bad());
    }
    Ok(mac)
}

fn parse_ip(key: &str, text: &str) -> Result<Ipv4Addr, String> {
    text.parse()
        .map_err(|_| format!("{key} {text:?} is not an IPv4 address"))
}

/// Reads `key=value` lines up to an empty line. An `error=<text>` line fails with that text.
pub fn parse_config(reader: &mut impl BufRead) -> Result<NetConfig, String> {
    let (mut mac, mut mtu, mut address, mut prefix_len, mut gateway) =
        (None, 1500, None, None, None);
    for _ in 0..MAX_CONFIG_LINES {
        let mut line = String::new();
        let read = reader
            .read_line(&mut line)
            .map_err(|error| crate::common::io_err("read config", &error))?;
        let line = line.trim_end_matches(['\r', '\n']);
        if read == 0 {
            return Err("the engine closed the socket before the configuration ended".into());
        }
        if line.is_empty() {
            return Ok(NetConfig {
                mac: mac.ok_or("config has no mac")?,
                mtu,
                address: address.ok_or("config has no address")?,
                prefix_len: prefix_len.ok_or("config has no prefix_len")?,
                gateway,
            });
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| format!("config line {line:?} is not key=value"))?;
        match key {
            "error" => return Err(value.to_string()),
            "mac" => mac = Some(parse_mac(value)?),
            "mtu" => {
                mtu = value
                    .parse()
                    .ok()
                    .filter(|mtu| (68..=65535).contains(mtu))
                    .ok_or_else(|| format!("mtu {value:?} is not between 68 and 65535"))?
            }
            "address" => address = Some(parse_ip(key, value)?),
            "prefix_len" => {
                prefix_len = Some(
                    value
                        .parse()
                        .ok()
                        .filter(|length| *length <= 32)
                        .ok_or_else(|| format!("prefix_len {value:?} is not between 0 and 32"))?,
                )
            }
            "gateway" => gateway = Some(parse_ip(key, value)?),
            _ => return Err(format!("unknown config key {key:?}")),
        }
    }
    Err("the configuration is too long".into())
}

pub fn netmask(prefix_len: u8) -> Ipv4Addr {
    let bits = u32::MAX
        .checked_shl(32 - u32::from(prefix_len.min(32)))
        .unwrap_or(0);
    Ipv4Addr::from(bits)
}

/// The kernel's `struct ifreq`: a 16-byte name and a 24-byte union, filled by byte offset.
#[repr(C, align(8))]
pub struct IfReq {
    name: [u8; 16],
    data: [u8; 24],
}

impl IfReq {
    pub fn new(name: &str) -> IfReq {
        let mut request = IfReq {
            name: [0; 16],
            data: [0; 24],
        };
        let bytes = name.as_bytes();
        let length = bytes.len().min(15);
        request.name[..length].copy_from_slice(&bytes[..length]);
        request
    }

    pub fn flags(&self) -> i16 {
        i16::from_ne_bytes([self.data[0], self.data[1]])
    }

    pub fn set_flags(&mut self, flags: i16) {
        self.data[..2].copy_from_slice(&flags.to_ne_bytes());
    }

    pub fn set_mtu(&mut self, mtu: u32) {
        self.data[..4].copy_from_slice(&(mtu as i32).to_ne_bytes());
    }

    /// A `sockaddr_in` holding the address.
    pub fn set_addr(&mut self, addr: Ipv4Addr) {
        self.data[..2].copy_from_slice(&AF_INET.to_ne_bytes());
        self.data[4..8].copy_from_slice(&addr.octets());
    }

    /// A `sockaddr` of family ARPHRD_ETHER holding the hardware address.
    pub fn set_hwaddr(&mut self, mac: [u8; 6]) {
        self.data[..2].copy_from_slice(&ARPHRD_ETHER.to_ne_bytes());
        self.data[2..8].copy_from_slice(&mac);
    }
}

/// The kernel's `struct rtentry` (120 bytes on 64-bit) for the default route through `gateway`.
pub fn default_route(gateway: Ipv4Addr) -> [u64; 15] {
    let mut bytes = [0u8; 120];
    let mut put_addr = |offset: usize, addr: Ipv4Addr| {
        bytes[offset..offset + 2].copy_from_slice(&AF_INET.to_ne_bytes());
        bytes[offset + 4..offset + 8].copy_from_slice(&addr.octets());
    };
    put_addr(8, Ipv4Addr::UNSPECIFIED); // rt_dst
    put_addr(24, gateway); // rt_gateway
    put_addr(40, Ipv4Addr::UNSPECIFIED); // rt_genmask
    bytes[56..58].copy_from_slice(&RTF_UP_GATEWAY.to_ne_bytes()); // rt_flags
    let mut words = [0u64; 15];
    for (word, chunk) in words.iter_mut().zip(bytes.chunks(8)) {
        *word = u64::from_ne_bytes(chunk.try_into().unwrap_or([0; 8]));
    }
    words
}

#[cfg(target_os = "linux")]
pub use linux::*;

#[cfg(target_os = "linux")]
mod linux {
    use super::{default_route, netmask, IfReq, NetConfig, TAP_NAME};
    use crate::common::{io_err, os_err};
    use std::fs::File;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

    fn inet_socket() -> Result<OwnedFd, String> {
        // SAFETY: plain integer arguments.
        let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC, 0) };
        if fd < 0 {
            return Err(os_err("socket"));
        }
        // SAFETY: socket returned a fresh descriptor that nothing else owns.
        Ok(unsafe { OwnedFd::from_raw_fd(fd) })
    }

    fn ioctl<T>(
        fd: &impl AsRawFd,
        request: libc::c_ulong,
        argument: &mut T,
        what: &str,
    ) -> Result<(), String> {
        // SAFETY: `argument` is a live, writable value of the structure the request expects, and every
        // caller passes the request that matches its type (IfReq for the interface requests, the rtentry
        // words for SIOCADDRT).
        let rc = unsafe { libc::ioctl(fd.as_raw_fd(), request as _, argument as *mut T) };
        if rc != 0 {
            return Err(os_err(what));
        }
        Ok(())
    }

    fn set_up(socket: &OwnedFd, name: &str) -> Result<(), String> {
        let mut request = IfReq::new(name);
        ioctl(
            socket,
            libc::SIOCGIFFLAGS,
            &mut request,
            &format!("SIOCGIFFLAGS {name}"),
        )?;
        let flags = request.flags() | (libc::IFF_UP | libc::IFF_RUNNING) as i16;
        request.set_flags(flags);
        ioctl(
            socket,
            libc::SIOCSIFFLAGS,
            &mut request,
            &format!("SIOCSIFFLAGS {name}"),
        )
    }

    /// Brings the loopback interface up.
    pub fn loopback_up() -> Result<(), String> {
        set_up(&inet_socket()?, "lo")
    }

    /// Creates the TAP device `tap0` (Ethernet frames, no packet information) and returns its descriptor.
    pub fn create_tap() -> Result<File, String> {
        let tun = File::options()
            .read(true)
            .write(true)
            .open("/dev/net/tun")
            .map_err(|error| io_err("open /dev/net/tun", &error))?;
        let mut request = IfReq::new(TAP_NAME);
        request.set_flags((libc::IFF_TAP | libc::IFF_NO_PI) as i16);
        ioctl(
            &tun,
            libc::TUNSETIFF as libc::c_ulong,
            &mut request,
            "TUNSETIFF tap0",
        )?;
        Ok(tun)
    }

    /// Keeps `tap0` after its last descriptor is closed (it goes with the network namespace), so a
    /// later engine can open it again.
    pub fn keep_tap(tun: &File) -> Result<(), String> {
        // SAFETY: TUNSETPERSIST takes an integer argument; the descriptor is open.
        let rc =
            unsafe { libc::ioctl(tun.as_raw_fd(), libc::TUNSETPERSIST as _, 1 as libc::c_int) };
        if rc != 0 {
            return Err(os_err("TUNSETPERSIST tap0"));
        }
        Ok(())
    }

    /// Gives `tap0` its hardware address, MTU and address, brings it up and adds the default route.
    pub fn configure_tap(config: &NetConfig) -> Result<(), String> {
        let socket = inet_socket()?;
        let mut request = IfReq::new(TAP_NAME);
        request.set_hwaddr(config.mac);
        ioctl(
            &socket,
            libc::SIOCSIFHWADDR,
            &mut request,
            "SIOCSIFHWADDR tap0",
        )?;
        let mut request = IfReq::new(TAP_NAME);
        request.set_mtu(config.mtu);
        ioctl(&socket, libc::SIOCSIFMTU, &mut request, "SIOCSIFMTU tap0")?;
        let mut request = IfReq::new(TAP_NAME);
        request.set_addr(config.address);
        ioctl(&socket, libc::SIOCSIFADDR, &mut request, "SIOCSIFADDR tap0")?;
        let mut request = IfReq::new(TAP_NAME);
        request.set_addr(netmask(config.prefix_len));
        ioctl(
            &socket,
            libc::SIOCSIFNETMASK,
            &mut request,
            "SIOCSIFNETMASK tap0",
        )?;
        set_up(&socket, TAP_NAME)?;
        if let Some(gateway) = config.gateway {
            let mut route = default_route(gateway);
            ioctl(
                &socket,
                libc::SIOCADDRT,
                &mut route,
                "SIOCADDRT default route",
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn parse(text: &str) -> Result<NetConfig, String> {
        parse_config(&mut Cursor::new(text.as_bytes()))
    }

    #[test]
    fn a_full_config_parses() {
        let config = parse(
            "mac=02:00:5e:10:00:01\nmtu=1400\naddress=10.0.2.15\nprefix_len=24\ngateway=10.0.2.2\n\nrest",
        )
        .unwrap();
        assert_eq!(
            config,
            NetConfig {
                mac: [2, 0, 0x5e, 0x10, 0, 1],
                mtu: 1400,
                address: Ipv4Addr::new(10, 0, 2, 15),
                prefix_len: 24,
                gateway: Some(Ipv4Addr::new(10, 0, 2, 2)),
            }
        );
    }

    #[test]
    fn the_mtu_and_gateway_are_optional() {
        let config =
            parse("mac=aa:bb:cc:dd:ee:ff\naddress=10.0.2.15\nprefix_len=24\r\n\r\n").unwrap();
        assert_eq!((config.mtu, config.gateway), (1500, None));
    }

    #[test]
    fn an_error_line_carries_the_engines_text() {
        assert_eq!(
            parse("mac=aa:bb:cc:dd:ee:ff\nerror=no addresses left\n").unwrap_err(),
            "no addresses left"
        );
    }

    #[test]
    fn bad_input_is_an_error_not_a_panic() {
        for text in [
            "",
            "mac=aa:bb:cc:dd:ee:ff\n",
            "\n",
            "address=10.0.2.15\nprefix_len=24\n\n",
            "mac=zz:bb:cc:dd:ee:ff\naddress=10.0.2.15\nprefix_len=24\n\n",
            "mac=aa:bb:cc:dd:ee\naddress=10.0.2.15\nprefix_len=24\n\n",
            "mac=aa:bb:cc:dd:ee:ff:00\naddress=10.0.2.15\nprefix_len=24\n\n",
            "mac=a:bb:cc:dd:ee:ff\naddress=10.0.2.15\nprefix_len=24\n\n",
            "mac=aa:bb:cc:dd:ee:ff\naddress=10.0.2\nprefix_len=24\n\n",
            "mac=aa:bb:cc:dd:ee:ff\naddress=10.0.2.15\nprefix_len=33\n\n",
            "mac=aa:bb:cc:dd:ee:ff\naddress=10.0.2.15\nprefix_len=24\nmtu=10\n\n",
            "mac=aa:bb:cc:dd:ee:ff\naddress=10.0.2.15\nprefix_len=24\nspeed=1\n\n",
            "nonsense\n\n",
        ] {
            assert!(parse(text).is_err(), "{text:?} should fail");
        }
        let endless = "mtu=1500\n".repeat(100);
        assert!(parse(&endless).unwrap_err().contains("too long"));
    }

    #[test]
    fn netmasks() {
        assert_eq!(netmask(24), Ipv4Addr::new(255, 255, 255, 0));
        assert_eq!(netmask(30), Ipv4Addr::new(255, 255, 255, 252));
        assert_eq!(netmask(32), Ipv4Addr::new(255, 255, 255, 255));
        assert_eq!(netmask(0), Ipv4Addr::UNSPECIFIED);
    }

    #[test]
    fn the_interface_request_is_laid_out_like_the_kernels() {
        assert_eq!(std::mem::size_of::<IfReq>(), 40);
        let mut request = IfReq::new("tap0");
        request.set_addr(Ipv4Addr::new(10, 0, 2, 15));
        assert_eq!(&request.name[..5], b"tap0\0");
        assert_eq!(&request.data[..2], &AF_INET.to_ne_bytes());
        assert_eq!(&request.data[4..8], &[10, 0, 2, 15]);
        request.set_hwaddr([1, 2, 3, 4, 5, 6]);
        assert_eq!(&request.data[2..8], &[1, 2, 3, 4, 5, 6]);
        request.set_flags(0x1002);
        assert_eq!(request.flags(), 0x1002);
        assert_eq!(IfReq::new("a-name-longer-than-fifteen").name[15], 0);
    }

    #[test]
    fn the_default_route_has_the_gateway_flag_and_zero_destination() {
        let route = default_route(Ipv4Addr::new(10, 0, 2, 2));
        let bytes: Vec<u8> = route.iter().flat_map(|word| word.to_ne_bytes()).collect();
        assert_eq!(bytes.len(), 120);
        assert_eq!(&bytes[8..10], &AF_INET.to_ne_bytes());
        assert_eq!(&bytes[12..16], &[0, 0, 0, 0]);
        assert_eq!(&bytes[28..32], &[10, 0, 2, 2]);
        assert_eq!(&bytes[40..42], &AF_INET.to_ne_bytes());
        assert_eq!(&bytes[56..58], &3u16.to_ne_bytes());
    }
}
