//! What this device has left for computers: memory that can be given away, free disk, and the
//! floors below which an agent inside a computer may not start, create or checkpoint another.
//! The measurements are cached for a moment, so the query can be called often.

use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};

use silo_api::methods::device::{MemoryPressure, ResourceState};

pub const GIB: u64 = 1024 * 1024 * 1024;

/// The memory floor is the larger of this and [`MEMORY_FLOOR_PERCENT`] of the device's memory.
const MEMORY_FLOOR_MINIMUM: u64 = 4 * GIB;
const MEMORY_FLOOR_PERCENT: u64 = 10;
/// The disk floor is the larger of this and [`DISK_FLOOR_PERCENT`] of the disk.
const DISK_FLOOR_MINIMUM: u64 = 10 * GIB;
const DISK_FLOOR_PERCENT: u64 = 5;

/// The smallest floors the owner can set, so a floor cannot be set to nothing by mistake.
pub const MIN_MEMORY_FLOOR_GIB: u64 = 1;
pub const MIN_DISK_FLOOR_GIB: u64 = 2;
/// A floor above this is a typing mistake, not a choice.
pub const MAX_FLOOR_GIB: u64 = 1_048_576;

/// How long a measurement is reused.
const CACHE_FOR: Duration = Duration::from_secs(2);

/// One reading of the device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Host {
    pub logical_cpus: u64,
    pub memory_total: u64,
    /// Memory that can be given to a computer now; `None` when the system cannot tell.
    pub memory_available: Option<u64>,
    pub pressure: Option<MemoryPressure>,
    pub disk_free: u64,
    pub disk_total: u64,
}

/// The two thresholds, in bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Floors {
    pub memory: u64,
    pub disk: u64,
}

impl Floors {
    /// The floors for `host`: what the owner set, in GiB, or the defaults.
    pub fn resolve(host: &Host, memory_gib: Option<u64>, disk_gib: Option<u64>) -> Self {
        Self {
            memory: memory_gib.map_or_else(
                || MEMORY_FLOOR_MINIMUM.max(host.memory_total / 100 * MEMORY_FLOOR_PERCENT),
                |gib| gib.max(MIN_MEMORY_FLOOR_GIB).saturating_mul(GIB),
            ),
            disk: disk_gib.map_or_else(
                || DISK_FLOOR_MINIMUM.max(host.disk_total / 100 * DISK_FLOOR_PERCENT),
                |gib| gib.max(MIN_DISK_FLOOR_GIB).saturating_mul(GIB),
            ),
        }
    }
}

/// How the device stands against the floors, and what to tell an agent about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Assessment {
    pub state: ResourceState,
    pub message: String,
}

fn amount(bytes: u64) -> String {
    crate::backup::format_bytes(bytes)
}

/// `critical` below a floor, `low` below twice a floor (or while the system reports memory
/// pressure), `ok` otherwise. Memory is judged before disk.
pub fn assess(host: &Host, floors: &Floors) -> Assessment {
    let memory = host.memory_available;
    if let Some(available) = memory.filter(|available| *available < floors.memory) {
        return Assessment {
            state: ResourceState::Critical,
            message: format!(
                "This device has {} of memory available; Silo keeps {} free. Stop a computer first, or ask the person who runs Silo.",
                amount(available),
                amount(floors.memory)
            ),
        };
    }
    if host.disk_free < floors.disk {
        return Assessment {
            state: ResourceState::Critical,
            message: format!(
                "This device has {} of disk space free; Silo keeps {} free. Delete a computer or a checkpoint first, or ask the person who runs Silo.",
                amount(host.disk_free),
                amount(floors.disk)
            ),
        };
    }
    if let Some(available) = memory.filter(|available| *available < floors.memory.saturating_mul(2))
    {
        return Assessment {
            state: ResourceState::Low,
            message: format!(
                "About {} of memory is free. Starting another computer may slow this device.",
                amount(available)
            ),
        };
    }
    if host.disk_free < floors.disk.saturating_mul(2) {
        return Assessment {
            state: ResourceState::Low,
            message: format!(
                "About {} of disk space is free. Another computer or checkpoint may fill the disk.",
                amount(host.disk_free)
            ),
        };
    }
    if matches!(
        host.pressure,
        Some(MemoryPressure::Warning | MemoryPressure::Critical)
    ) {
        return Assessment {
            state: ResourceState::Low,
            message: "The system reports memory pressure. Starting another computer may slow this device."
                .to_owned(),
        };
    }
    let memory_part = memory.map_or_else(String::new, |available| {
        format!("{} of memory and ", amount(available))
    });
    Assessment {
        state: ResourceState::Ok,
        message: format!(
            "{memory_part}{} of disk space are free.",
            amount(host.disk_free)
        ),
    }
}

/// The reason an agent's call that starts or creates a computer must fail now, if there is one.
pub fn refusal(host: &Host, floors: &Floors) -> Option<String> {
    let assessment = assess(host, floors);
    (assessment.state == ResourceState::Critical).then_some(assessment.message)
}

/// The reason an agent's call that starts or creates a computer with `requested` bytes must fail,
/// taking into account the floor, existing `reserved` bytes from starts already admitted, and whether
/// the request exceeds the free headroom.
pub fn refusal_for(host: &Host, floors: &Floors, reserved: u64, requested: u64) -> Option<String> {
    if let Some(refusal) = refusal(host, floors) {
        return Some(refusal);
    }
    if requested > 0 {
        if let Some(available) = host.memory_available {
            let free_headroom = available.saturating_sub(floors.memory);
            let remaining_headroom = free_headroom.saturating_sub(reserved);
            if requested > remaining_headroom {
                return Some(format!(
                    "This computer needs {} of memory, but only {} is free above the {} safety floor. Stop a computer first, or ask the person who runs Silo.",
                    amount(requested),
                    amount(remaining_headroom),
                    amount(floors.memory)
                ));
            }
        }
    }
    None
}

// MARK: Measuring

/// The memory in the pages `vm_stat` lists as free, inactive, purgeable and speculative: what
/// the system can hand to a program without taking it from another.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn macos_available_bytes(
    page_size: u64,
    free: u64,
    inactive: u64,
    purgeable: u64,
    speculative: u64,
) -> u64 {
    [free, inactive, purgeable, speculative]
        .into_iter()
        .fold(0_u64, |sum, pages| sum.saturating_add(pages))
        .saturating_mul(page_size)
}

#[cfg(target_os = "macos")]
fn memory_available() -> Option<u64> {
    use std::sync::OnceLock;

    static HOST: OnceLock<libc::mach_port_t> = OnceLock::new();
    // SAFETY: mach_host_self takes no arguments; its one send right is kept for the process.
    #[allow(deprecated)] // The libc crate points at mach2, which this engine does not use.
    let host = *HOST.get_or_init(|| unsafe { libc::mach_host_self() });
    // SAFETY: vm_statistics64 is plain data, so zeroed is a valid initial value.
    let mut statistics: libc::vm_statistics64 = unsafe { std::mem::zeroed() };
    let mut count = libc::HOST_VM_INFO64_COUNT;
    // SAFETY: the output buffer is a vm_statistics64 and `count` is its length in words.
    let status = unsafe {
        libc::host_statistics64(
            host,
            libc::HOST_VM_INFO64,
            (&raw mut statistics).cast(),
            &mut count,
        )
    };
    // SAFETY: sysconf takes no pointers.
    let page_size = u64::try_from(unsafe { libc::sysconf(libc::_SC_PAGESIZE) })
        .ok()
        .filter(|size| *size > 0)?;
    if status != 0 {
        return None;
    }
    // The kernel counts speculative pages inside the free count; `vm_stat` prints them apart.
    let speculative = u64::from(statistics.speculative_count);
    let free = u64::from(statistics.free_count).saturating_sub(speculative);
    Some(macos_available_bytes(
        page_size,
        free,
        u64::from(statistics.inactive_count),
        u64::from(statistics.purgeable_count),
        speculative,
    ))
}

/// `MemAvailable` of a `/proc/meminfo` text, in bytes.
#[cfg_attr(target_os = "macos", allow(dead_code))]
fn parse_meminfo(text: &str) -> Option<u64> {
    let line = text
        .lines()
        .find_map(|line| line.strip_prefix("MemAvailable:"))?;
    let mut parts = line.split_whitespace();
    let amount: u64 = parts.next()?.parse().ok()?;
    match parts.next() {
        Some("kB") => amount.checked_mul(1024),
        None => Some(amount),
        Some(_) => None,
    }
}

#[cfg(not(target_os = "macos"))]
fn memory_available() -> Option<u64> {
    parse_meminfo(&std::fs::read_to_string("/proc/meminfo").ok()?)
}

/// The system's memory pressure level on macOS (`kern.memorystatus_vm_pressure_level`).
#[cfg(target_os = "macos")]
fn memory_pressure() -> Option<MemoryPressure> {
    let mut value = 0_i32;
    let mut length = std::mem::size_of::<i32>();
    // SAFETY: the name is NUL-terminated and both output pointers refer to writable values
    // of the declared length for the duration of the call.
    let status = unsafe {
        libc::sysctlbyname(
            c"kern.memorystatus_vm_pressure_level".as_ptr(),
            (&raw mut value).cast(),
            &mut length,
            std::ptr::null_mut(),
            0,
        )
    };
    if status != 0 {
        return None;
    }
    pressure_of_level(value)
}

/// Other systems give no level.
#[cfg(not(target_os = "macos"))]
fn memory_pressure() -> Option<MemoryPressure> {
    None
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn pressure_of_level(level: i32) -> Option<MemoryPressure> {
    match level {
        1 => Some(MemoryPressure::Normal),
        2 => Some(MemoryPressure::Warning),
        4 => Some(MemoryPressure::Critical),
        _ => None,
    }
}

fn read_host(disk: &Path) -> Option<Host> {
    let capacity = crate::device_facts::device_capacity()?;
    let (disk_free, disk_total) = crate::backup::volume_bytes(disk).ok()?;
    Some(Host {
        logical_cpus: capacity.logical_cpus as u64,
        memory_total: capacity.physical_memory_bytes,
        memory_available: memory_available(),
        pressure: memory_pressure(),
        disk_free,
        disk_total,
    })
}

static CACHE: Mutex<Option<(Instant, PathBuf, Host)>> = Mutex::new(None);

thread_local! {
    static FIXED: RefCell<Option<Host>> = const { RefCell::new(None) };
}

/// Makes [`measure`] answer `host` on this thread until dropped, so a test does not depend on
/// the memory and disk of the machine it runs on.
pub struct FixedHost(());

impl FixedHost {
    pub fn new(host: Host) -> Self {
        FIXED.with(|fixed| *fixed.borrow_mut() = Some(host));
        Self(())
    }
}

impl Drop for FixedHost {
    fn drop(&mut self) {
        FIXED.with(|fixed| *fixed.borrow_mut() = None);
    }
}

/// The device now, with the disk measured at `disk` (the folder that holds computers). Reused
/// for two seconds; `None` when the device cannot be measured.
pub fn measure(disk: &Path) -> Option<Host> {
    if let Some(host) = FIXED.with(|fixed| fixed.borrow().clone()) {
        return Some(host);
    }
    let mut cache = CACHE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((at, path, host)) = cache.as_ref() {
        if at.elapsed() < CACHE_FOR && path == disk {
            return Some(host.clone());
        }
    }
    let host = read_host(disk)?;
    *cache = Some((Instant::now(), disk.to_path_buf(), host.clone()));
    Some(host)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(memory_available: Option<u64>, disk_free: u64) -> Host {
        Host {
            logical_cpus: 8,
            memory_total: 32 * GIB,
            memory_available,
            pressure: None,
            disk_free,
            disk_total: 500 * GIB,
        }
    }

    #[test]
    fn available_memory_adds_the_pages_vm_stat_lists_as_reclaimable() {
        // A captured `vm_stat` on a 16 KiB-page Mac: free 8021, inactive 120843,
        // purgeable 4470, speculative 3340.
        assert_eq!(
            macos_available_bytes(16384, 8021, 120_843, 4470, 3340),
            (8021 + 120_843 + 4470 + 3340) * 16384
        );
        assert_eq!(macos_available_bytes(16384, 0, 0, 0, 0), 0);
        assert_eq!(macos_available_bytes(u64::MAX, u64::MAX, 1, 1, 1), u64::MAX);
    }

    #[test]
    fn meminfo_gives_memavailable_in_bytes() {
        let sample = "MemTotal:       16318412 kB\nMemFree:          731836 kB\n\
            MemAvailable:    9841764 kB\nBuffers:         1302924 kB\n";
        assert_eq!(parse_meminfo(sample), Some(9_841_764 * 1024));
        assert_eq!(parse_meminfo("MemTotal: 1 kB\n"), None);
        assert_eq!(parse_meminfo("MemAvailable: lots kB\n"), None);
        assert_eq!(parse_meminfo("MemAvailable: 5 MB\n"), None);
    }

    #[test]
    fn pressure_levels_follow_the_kernels_values() {
        assert_eq!(pressure_of_level(1), Some(MemoryPressure::Normal));
        assert_eq!(pressure_of_level(2), Some(MemoryPressure::Warning));
        assert_eq!(pressure_of_level(4), Some(MemoryPressure::Critical));
        assert_eq!(pressure_of_level(3), None);
    }

    #[test]
    fn default_floors_are_the_larger_of_a_fixed_amount_and_a_share() {
        let small = Host {
            memory_total: 16 * GIB,
            disk_total: 100 * GIB,
            ..host(None, 0)
        };
        let floors = Floors::resolve(&small, None, None);
        assert_eq!(floors.memory, 4 * GIB);
        assert_eq!(floors.disk, 10 * GIB);
        let large = Host {
            memory_total: 128 * GIB,
            disk_total: 2000 * GIB,
            ..host(None, 0)
        };
        let floors = Floors::resolve(&large, None, None);
        assert_eq!(floors.memory, 128 * GIB / 100 * 10);
        assert_eq!(floors.disk, 100 * GIB);
        let set = Floors::resolve(&large, Some(2), Some(30));
        assert_eq!((set.memory, set.disk), (2 * GIB, 30 * GIB));
        // A value below the smallest allowed (from a hand-edited file) is raised to it.
        let zero = Floors::resolve(&large, Some(0), Some(0));
        assert_eq!((zero.memory, zero.disk), (GIB, 2 * GIB));
    }

    #[test]
    fn the_state_changes_at_the_floor_and_at_twice_the_floor() {
        let floors = Floors {
            memory: 4 * GIB,
            disk: 10 * GIB,
        };
        let state = |memory: u64, disk: u64| assess(&host(Some(memory), disk), &floors).state;
        assert_eq!(state(4 * GIB - 1, 100 * GIB), ResourceState::Critical);
        assert_eq!(state(4 * GIB, 100 * GIB), ResourceState::Low);
        assert_eq!(state(8 * GIB - 1, 100 * GIB), ResourceState::Low);
        assert_eq!(state(8 * GIB, 100 * GIB), ResourceState::Ok);
        assert_eq!(state(32 * GIB, 10 * GIB - 1), ResourceState::Critical);
        assert_eq!(state(32 * GIB, 10 * GIB), ResourceState::Low);
        assert_eq!(state(32 * GIB, 20 * GIB - 1), ResourceState::Low);
        assert_eq!(state(32 * GIB, 20 * GIB), ResourceState::Ok);
        // The worse of the two decides.
        assert_eq!(state(6 * GIB, 5 * GIB), ResourceState::Critical);
    }

    #[test]
    fn only_a_state_below_a_floor_refuses_and_the_message_carries_the_facts() {
        let floors = Floors {
            memory: 6 * GIB,
            disk: 10 * GIB,
        };
        let short = host(Some(3 * GIB + GIB / 10), 100 * GIB);
        let message = refusal(&short, &floors).unwrap();
        assert!(message.contains("3.1 GiB of memory available"), "{message}");
        assert!(message.contains("keeps 6.0 GiB free"), "{message}");
        assert!(message.contains("Stop a computer first"), "{message}");
        let full = host(Some(32 * GIB), 4 * GIB);
        let message = refusal(&full, &floors).unwrap();
        assert!(message.contains("4.0 GiB of disk space"), "{message}");
        assert_eq!(refusal(&host(Some(6 * GIB), 10 * GIB), &floors), None);
        // When the system cannot say how much memory is free, only the disk is judged.
        assert_eq!(refusal(&host(None, 100 * GIB), &floors), None);
        let low = assess(&host(Some(6 * GIB), 100 * GIB), &floors);
        assert_eq!(
            low.message,
            "About 6.0 GiB of memory is free. Starting another computer may slow this device."
        );
    }

    #[test]
    fn refusal_for_detects_below_floor_and_oversized_requests_and_reservations() {
        let floors = Floors {
            memory: 4 * GIB,
            disk: 25 * GIB,
        };
        // 1. Below floor
        let low_host = host(Some(3 * GIB), 400 * GIB);
        let error = refusal_for(&low_host, &floors, 0, 1 * GIB).unwrap();
        assert!(error.contains("3.0 GiB of memory available; Silo keeps 4.0 GiB free"));

        // 2. Headroom is 2 GiB (6 GiB available - 4 GiB floor). Request for 4 GiB is oversized.
        let ample_host = host(Some(6 * GIB), 400 * GIB);
        let error = refusal_for(&ample_host, &floors, 0, 4 * GIB).unwrap();
        assert!(error.contains("This computer needs 4.0 GiB of memory, but only 2.0 GiB is free above the 4.0 GiB safety floor. Stop a computer first, or ask the person who runs Silo."));

        // 3. Request for 2 GiB fits in 2 GiB headroom.
        assert!(refusal_for(&ample_host, &floors, 0, 2 * GIB).is_none());

        // 4. With 1 GiB reservation, remaining headroom is 1 GiB. Request for 2 GiB exceeds it.
        let error = refusal_for(&ample_host, &floors, 1 * GIB, 2 * GIB).unwrap();
        assert!(error.contains("This computer needs 2.0 GiB of memory, but only 1.0 GiB is free above the 4.0 GiB safety floor. Stop a computer first, or ask the person who runs Silo."));

        // 5. With 1 GiB reservation, request for 1 GiB fits.
        assert!(refusal_for(&ample_host, &floors, 1 * GIB, 1 * GIB).is_none());
    }

    #[test]
    fn memory_pressure_alone_makes_the_state_low_never_critical() {
        let floors = Floors {
            memory: 4 * GIB,
            disk: 10 * GIB,
        };
        let mut pressed = host(Some(32 * GIB), 100 * GIB);
        pressed.pressure = Some(MemoryPressure::Critical);
        let assessment = assess(&pressed, &floors);
        assert_eq!(assessment.state, ResourceState::Low);
        assert!(assessment.message.contains("memory pressure"));
        assert_eq!(refusal(&pressed, &floors), None);
    }

    #[test]
    fn a_fixed_host_replaces_the_measurement_on_its_thread_only() {
        let fixed = host(Some(GIB), GIB);
        {
            let _guard = FixedHost::new(fixed.clone());
            assert_eq!(measure(Path::new("/")), Some(fixed.clone()));
            let other = std::thread::spawn(|| measure(Path::new("/")));
            assert_ne!(other.join().unwrap(), Some(fixed.clone()));
        }
        assert_ne!(measure(Path::new("/")), Some(fixed));
    }

    #[test]
    fn this_device_can_be_measured() {
        let measured = measure(&std::env::temp_dir()).expect("the device reports its resources");
        assert!(measured.logical_cpus > 0);
        assert!(measured.memory_total > 0);
        assert!(measured.disk_total >= measured.disk_free);
        assert!(measured
            .memory_available
            .is_some_and(|available| available > 0 && available <= measured.memory_total));
    }
}
