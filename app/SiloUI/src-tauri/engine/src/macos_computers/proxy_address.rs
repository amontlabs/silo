//! Stable private /30 links for macOS guest proxy networking.
//!
//! Each computer keeps its slot in its own directory. Allocation scans sibling
//! sidecars while holding a process-wide lock, so two simultaneous allocations
//! cannot choose the same link. The host's default route is intentionally ignored:
//! it does not reserve an on-link address range.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    net::Ipv4Addr,
    path::{Path, PathBuf},
    process::Command,
    sync::{Mutex, MutexGuard, PoisonError},
};
use tempfile::NamedTempFile;

use super::store::Layout;

const POOL: Ipv4Cidr = Ipv4Cidr {
    network: Ipv4Addr::new(172, 30, 0, 0),
    prefix_len: 16,
};
const LINK_PREFIX_LEN: u8 = 30;
const SLOT_COUNT: u32 = 1 << (LINK_PREFIX_LEN - POOL.prefix_len);
const SIDE_CAR: &str = "proxy-address.json";
const SIDE_CAR_VERSION: u32 = 1;

static ALLOCATION_LOCK: Mutex<()> = Mutex::new(());

fn allocation_lock() -> MutexGuard<'static, ()> {
    ALLOCATION_LOCK
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// A canonical IPv4 network. Prefix zero represents the default route and is
/// ignored by `overlaps`, because it does not reserve a host's on-link range.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct Ipv4Cidr {
    network: Ipv4Addr,
    prefix_len: u8,
}

impl Ipv4Cidr {
    pub(super) fn new(address: Ipv4Addr, prefix_len: u8) -> Result<Self, String> {
        if prefix_len > 32 {
            return Err("Invalid IPv4 route prefix.".into());
        }
        let mask = prefix_mask(prefix_len);
        Ok(Self {
            network: Ipv4Addr::from(u32::from(address) & mask),
            prefix_len,
        })
    }

    pub(super) fn network(self) -> Ipv4Addr {
        self.network
    }

    pub(super) fn prefix_len(self) -> u8 {
        self.prefix_len
    }

    pub(super) fn overlaps(self, other: Self) -> bool {
        if self.prefix_len == 0 || other.prefix_len == 0 {
            return false;
        }
        let common_prefix = self.prefix_len.min(other.prefix_len);
        let mask = prefix_mask(common_prefix);
        (u32::from(self.network) & mask) == (u32::from(other.network) & mask)
    }
}

fn prefix_mask(prefix_len: u8) -> u32 {
    match prefix_len {
        0 => 0,
        1..=31 => u32::MAX << (32 - prefix_len),
        _ => u32::MAX,
    }
}

/// The point-to-point /30 assigned to one macOS computer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ProxyAddress {
    slot: u16,
    subnet: Ipv4Cidr,
    host: Ipv4Addr,
    guest: Ipv4Addr,
}

impl ProxyAddress {
    pub(super) fn slot(self) -> u16 {
        self.slot
    }

    pub(super) fn subnet(self) -> Ipv4Cidr {
        self.subnet
    }

    /// Address assigned to the host side of the point-to-point link.
    pub(super) fn host(self) -> Ipv4Addr {
        self.host
    }

    /// Address assigned to the guest side of the point-to-point link.
    pub(super) fn guest(self) -> Ipv4Addr {
        self.guest
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Sidecar {
    schema_version: u32,
    computer_id: String,
    slot: u16,
}

/// Return the saved /30 or allocate and persist a free one.
///
/// `host_routes` should contain the host's non-default IPv4 routes. The allocator
/// rejects a saved slot that now overlaps one of them, preserving the computer's
/// stable address rather than silently changing its guest configuration.
pub(super) fn load_or_allocate(
    layout: &Layout,
    host_routes: &[Ipv4Cidr],
) -> Result<ProxyAddress, String> {
    let _lock = allocation_lock();
    let computer_id = layout
        .dir
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| "Cannot identify the macOS computer network.".to_owned())?;
    fs::create_dir_all(&layout.dir)
        .map_err(|_| "Cannot prepare the macOS computer network directory.".to_owned())?;

    let occupied = occupied_slots(layout, computer_id)?;
    let sidecar_path = sidecar_path(&layout.dir);
    if sidecar_path.exists() {
        let sidecar = read_sidecar(&sidecar_path, computer_id)?;
        if occupied.contains(&sidecar.slot) {
            return Err("A macOS computer proxy address is already assigned.".into());
        }
        let address = address_for_slot(sidecar.slot)?;
        if host_routes
            .iter()
            .any(|route| address.subnet.overlaps(*route))
        {
            return Err("The macOS computer network overlaps a host route.".into());
        }
        return Ok(address);
    }

    let start = initial_slot(computer_id);
    let slot = find_slot(start, &occupied, host_routes)
        .ok_or_else(|| "No non-overlapping macOS computer network is available.".to_owned())?;
    let address = address_for_slot(slot)?;
    write_sidecar(&sidecar_path, computer_id, slot)?;
    Ok(address)
}

/// Read and validate an existing address assignment without creating files.
/// Ordinary Start and gateway setup use this so a missing setup step cannot
/// silently allocate a new network identity.
pub(super) fn load_existing(
    layout: &Layout,
    host_routes: &[Ipv4Cidr],
) -> Result<ProxyAddress, String> {
    let _lock = allocation_lock();
    let computer_id = layout
        .dir
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| "Cannot identify the macOS computer network.".to_owned())?;
    let path = sidecar_path(&layout.dir);
    if !path.is_file() {
        return Err(
            "The macOS computer network assignment is missing. Retry computer setup.".into(),
        );
    }
    let sidecar = read_sidecar(&path, computer_id)?;
    let occupied = occupied_slots(layout, computer_id)?;
    if occupied.contains(&sidecar.slot) {
        return Err("A macOS computer proxy address is already assigned.".into());
    }
    let address = address_for_slot(sidecar.slot)?;
    if host_routes
        .iter()
        .any(|route| address.subnet.overlaps(*route))
    {
        return Err("The macOS computer network overlaps a host route.".into());
    }
    Ok(address)
}

/// Read installed IPv4 route destinations before allocating a guest link.
/// macOS `netstat` emits abbreviated destinations such as `172.30/16`;
/// `parse_route_destination` canonicalizes those entries.
pub(super) fn host_ipv4_routes() -> Result<Vec<Ipv4Cidr>, String> {
    let output = Command::new("/usr/sbin/netstat")
        .args(["-rn", "-f", "inet"])
        .output()
        .map_err(|_| "Cannot inspect the Mac's IPv4 routes for the computer network.".to_owned())?;
    if !output.status.success() {
        return Err("Cannot inspect the Mac's IPv4 routes for the computer network.".into());
    }
    let text = String::from_utf8(output.stdout)
        .map_err(|_| "The Mac's IPv4 route table is unreadable.".to_owned())?;
    Ok(text
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .filter_map(parse_route_destination)
        .collect())
}

fn parse_route_destination(value: &str) -> Option<Ipv4Cidr> {
    if value == "default" || value.starts_with("link#") || value == "localhost" {
        return None;
    }
    let (address, prefix) = value
        .split_once('/')
        .map_or((value, None), |(address, prefix)| {
            (address, prefix.parse::<u8>().ok())
        });
    if value.contains('/') && prefix.is_none() {
        return None;
    }
    let octets = address
        .split('.')
        .map(str::parse::<u8>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    if octets.is_empty() || octets.len() > 4 {
        return None;
    }
    let mut full = [0u8; 4];
    full[..octets.len()].copy_from_slice(&octets);
    Ipv4Cidr::new(
        Ipv4Addr::from(full),
        prefix.unwrap_or((octets.len() * 8) as u8),
    )
    .ok()
}

fn sidecar_path(directory: &Path) -> PathBuf {
    directory.join(SIDE_CAR)
}

fn read_sidecar(path: &Path, computer_id: &str) -> Result<Sidecar, String> {
    let bytes = fs::read(path)
        .map_err(|_| "Cannot read the macOS computer network assignment.".to_owned())?;
    let sidecar: Sidecar = serde_json::from_slice(&bytes)
        .map_err(|_| "The macOS computer network assignment is invalid.".to_owned())?;
    if sidecar.schema_version != SIDE_CAR_VERSION
        || sidecar.computer_id != computer_id
        || u32::from(sidecar.slot) >= SLOT_COUNT
    {
        return Err("The macOS computer network assignment is invalid.".into());
    }
    Ok(sidecar)
}

fn occupied_slots(layout: &Layout, computer_id: &str) -> Result<HashSet<u16>, String> {
    let root = layout
        .dir
        .parent()
        .ok_or_else(|| "Cannot locate macOS computer network assignments.".to_owned())?;
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(HashSet::new()),
        Err(_) => return Err("Cannot read macOS computer network assignments.".into()),
    };

    let mut occupied = HashSet::new();
    for entry in entries {
        let entry = entry.map_err(|_| "Cannot read macOS computer network assignments.")?;
        if !entry
            .file_type()
            .map_err(|_| "Cannot read macOS computer network assignments.")?
            .is_dir()
        {
            continue;
        }
        let Some(id) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if id == computer_id {
            continue;
        }
        let path = sidecar_path(&entry.path());
        match fs::read(&path) {
            Ok(bytes) => {
                let sidecar: Sidecar = serde_json::from_slice(&bytes)
                    .map_err(|_| "A macOS computer network assignment is invalid.")?;
                if sidecar.schema_version != SIDE_CAR_VERSION
                    || sidecar.computer_id != id
                    || u32::from(sidecar.slot) >= SLOT_COUNT
                {
                    return Err("A macOS computer network assignment is invalid.".into());
                }
                if !occupied.insert(sidecar.slot) {
                    return Err("Two macOS computers have the same proxy address.".into());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("Cannot read macOS computer network assignments.".into()),
        }
    }
    Ok(occupied)
}

fn initial_slot(computer_id: &str) -> u16 {
    let digest = Sha256::digest(computer_id.as_bytes());
    let seed = u32::from_be_bytes(digest[..4].try_into().expect("SHA-256 prefix length"));
    (seed % SLOT_COUNT) as u16
}

fn find_slot(start: u16, occupied: &HashSet<u16>, host_routes: &[Ipv4Cidr]) -> Option<u16> {
    (0..SLOT_COUNT)
        .map(|offset| ((u32::from(start) + offset) % SLOT_COUNT) as u16)
        .find(|slot| {
            !occupied.contains(slot)
                && address_for_slot(*slot).is_ok_and(|address| {
                    !host_routes
                        .iter()
                        .any(|route| address.subnet.overlaps(*route))
                })
        })
}

fn address_for_slot(slot: u16) -> Result<ProxyAddress, String> {
    if u32::from(slot) >= SLOT_COUNT {
        return Err("The macOS computer network slot is out of range.".into());
    }
    let network = u32::from(POOL.network) + (u32::from(slot) << 2);
    let subnet = Ipv4Cidr {
        network: Ipv4Addr::from(network),
        prefix_len: LINK_PREFIX_LEN,
    };
    Ok(ProxyAddress {
        slot,
        subnet,
        host: Ipv4Addr::from(network + 1),
        guest: Ipv4Addr::from(network + 2),
    })
}

fn write_sidecar(path: &Path, computer_id: &str, slot: u16) -> Result<(), String> {
    let directory = path
        .parent()
        .ok_or_else(|| "Cannot save the macOS computer network assignment.".to_owned())?;
    let sidecar = Sidecar {
        schema_version: SIDE_CAR_VERSION,
        computer_id: computer_id.to_owned(),
        slot,
    };
    let bytes = serde_json::to_vec(&sidecar)
        .map_err(|_| "Cannot save the macOS computer network assignment.".to_owned())?;
    let mut temporary = NamedTempFile::new_in(directory)
        .map_err(|_| "Cannot save the macOS computer network assignment.".to_owned())?;
    std::io::Write::write_all(&mut temporary, &bytes)
        .map_err(|_| "Cannot save the macOS computer network assignment.".to_owned())?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|_| "Cannot save the macOS computer network assignment.".to_owned())?;
    temporary
        .persist(path)
        .map_err(|_| "Cannot save the macOS computer network assignment.".to_owned())?;
    fs::File::open(directory)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| "Cannot save the macOS computer network assignment.".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(root: &Path, id: &str) -> Layout {
        Layout {
            dir: root.join(id),
            access: None,
        }
    }

    #[test]
    fn slot_range_covers_the_pool_as_aligned_point_to_point_links() {
        let first = address_for_slot(0).unwrap();
        assert_eq!(first.subnet.network(), Ipv4Addr::new(172, 30, 0, 0));
        assert_eq!(first.subnet.prefix_len(), 30);
        assert_eq!(first.host(), Ipv4Addr::new(172, 30, 0, 1));
        assert_eq!(first.guest(), Ipv4Addr::new(172, 30, 0, 2));

        let last = address_for_slot((SLOT_COUNT - 1) as u16).unwrap();
        assert_eq!(last.subnet.network(), Ipv4Addr::new(172, 30, 255, 252));
        assert_eq!(last.host(), Ipv4Addr::new(172, 30, 255, 253));
        assert_eq!(last.guest(), Ipv4Addr::new(172, 30, 255, 254));
        assert!(address_for_slot(SLOT_COUNT as u16).is_err());
    }

    #[test]
    fn cidr_canonicalization_and_overlap_cover_broad_and_host_routes() {
        let pool_route = Ipv4Cidr::new(Ipv4Addr::new(172, 30, 11, 5), 16).unwrap();
        let one_host = Ipv4Cidr::new(Ipv4Addr::new(172, 30, 0, 2), 32).unwrap();
        let disjoint = Ipv4Cidr::new(Ipv4Addr::new(172, 31, 0, 0), 16).unwrap();
        let default = Ipv4Cidr::new(Ipv4Addr::UNSPECIFIED, 0).unwrap();
        let link = address_for_slot(0).unwrap().subnet();

        assert_eq!(pool_route.network(), Ipv4Addr::new(172, 30, 0, 0));
        assert!(link.overlaps(pool_route));
        assert!(link.overlaps(one_host));
        assert!(!link.overlaps(disjoint));
        assert!(!link.overlaps(default));
        assert!(Ipv4Cidr::new(Ipv4Addr::UNSPECIFIED, 33).is_err());
    }

    #[test]
    fn linear_probe_skips_collisions_and_wraps_at_the_last_slot() {
        let last = (SLOT_COUNT - 1) as u16;
        let occupied = HashSet::from([last, 0]);
        let blocked = address_for_slot(1).unwrap().subnet();
        let selected = find_slot(last, &occupied, &[blocked]).unwrap();
        assert_eq!(selected, 2);
    }

    #[test]
    fn allocation_persists_and_reuses_a_distinct_slot_per_computer() {
        let directory = tempfile::tempdir().unwrap();
        let first_layout = layout(directory.path(), "computer-a");
        let second_layout = layout(directory.path(), "computer-b");

        let first = load_or_allocate(&first_layout, &[]).unwrap();
        let second = load_or_allocate(&second_layout, &[]).unwrap();
        let first_again = load_or_allocate(&first_layout, &[]).unwrap();

        assert_eq!(first_again, first);
        assert_ne!(first.slot(), second.slot());
        assert!(!first.subnet().overlaps(second.subnet()));
        assert!(first_layout.dir.join(SIDE_CAR).is_file());
        assert!(second_layout.dir.join(SIDE_CAR).is_file());
    }

    #[test]
    fn loading_an_existing_address_never_allocates_missing_state() {
        let directory = tempfile::tempdir().unwrap();
        let computer_layout = layout(directory.path(), "computer-a");

        assert!(load_existing(&computer_layout, &[])
            .unwrap_err()
            .contains("assignment is missing"));
        assert!(!computer_layout.dir.exists());

        let allocated = load_or_allocate(&computer_layout, &[]).unwrap();
        assert_eq!(load_existing(&computer_layout, &[]).unwrap(), allocated);
        let conflicting_route = Ipv4Cidr::new(allocated.guest(), 32).unwrap();
        assert!(load_existing(&computer_layout, &[conflicting_route])
            .unwrap_err()
            .contains("overlaps a host route"));
    }

    #[test]
    fn new_allocation_skips_host_routes_but_saved_overlap_fails_closed() {
        let directory = tempfile::tempdir().unwrap();
        let computer_layout = layout(directory.path(), "computer-a");
        let original = load_or_allocate(&computer_layout, &[]).unwrap();
        let conflicting_route = Ipv4Cidr::new(original.guest(), 32).unwrap();

        let other_layout = layout(directory.path(), "computer-b");
        let allocated = load_or_allocate(&other_layout, &[conflicting_route]).unwrap();
        assert!(!allocated.subnet().overlaps(conflicting_route));
        assert!(load_or_allocate(&computer_layout, &[conflicting_route])
            .unwrap_err()
            .contains("overlaps a host route"));
    }

    #[test]
    fn invalid_or_duplicate_sidecars_do_not_get_silently_reused() {
        let directory = tempfile::tempdir().unwrap();
        let first = layout(directory.path(), "computer-a");
        let second = layout(directory.path(), "computer-b");
        fs::create_dir_all(&first.dir).unwrap();
        fs::create_dir_all(&second.dir).unwrap();
        let duplicate = Sidecar {
            schema_version: SIDE_CAR_VERSION,
            computer_id: "computer-a".into(),
            slot: 17,
        };
        fs::write(
            sidecar_path(&first.dir),
            serde_json::to_vec(&duplicate).unwrap(),
        )
        .unwrap();
        let duplicate = Sidecar {
            schema_version: SIDE_CAR_VERSION,
            computer_id: "computer-b".into(),
            slot: 17,
        };
        fs::write(
            sidecar_path(&second.dir),
            serde_json::to_vec(&duplicate).unwrap(),
        )
        .unwrap();

        assert!(load_or_allocate(&first, &[])
            .unwrap_err()
            .contains("already assigned"));
    }

    #[test]
    fn parses_macos_netstat_route_destinations() {
        assert_eq!(parse_route_destination("default"), None);
        assert_eq!(parse_route_destination("link#8"), None);
        assert_eq!(
            parse_route_destination("172.30/16"),
            Some(Ipv4Cidr::new(Ipv4Addr::new(172, 30, 0, 0), 16).unwrap())
        );
        assert_eq!(
            parse_route_destination("10.20.3/24"),
            Some(Ipv4Cidr::new(Ipv4Addr::new(10, 20, 3, 0), 24).unwrap())
        );
        assert_eq!(
            parse_route_destination("192.168.1.8"),
            Some(Ipv4Cidr::new(Ipv4Addr::new(192, 168, 1, 8), 32).unwrap())
        );
        assert_eq!(parse_route_destination("172.30/99"), None);
    }
}
