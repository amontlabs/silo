//! Keeping container computers from filling the host's disk.
//!
//! A container computer's files are a folder on the host's own file system, in the engine's home.
//! No rootless mechanism limits a folder's size: a project quota is set with an administrator's
//! rights on a file system mounted for it, a loop file needs `mount` (which a user namespace cannot
//! do for a block file system), and making a container's root read-only fails while a process has a
//! file open for writing, is undone by a checkpoint and is bypassed by a directory the engine
//! cannot read. So a container computer's size (`root_disk_mib` plus `workspace_mib`) is reported,
//! not a bound, and what Silo protects instead is the host:
//!
//! * Below a margin of free space on the engine's file system ([`margin_bytes`]), the engine
//!   refuses to start a container computer, make a checkpoint of one or fork one
//!   ([`require_room`]).
//! * Below the margin it also stops (the ordinary graceful stop) the running container computers
//!   whose writable layer is larger than their stated size, largest first ([`pass`]), and leaves a
//!   plain message in the computer's status ([`NOTICE`]). A stop frees nothing by itself; it keeps a
//!   computer from writing more, and the files stay until their owner removes some.
//!
//! The layer is measured inside the user namespace (`du` there reads directories the engine's own
//! account cannot), and only while the host is below the margin: with room, the guard reads one
//! number (`statvfs`) a few times a minute and holds nothing per computer, so nothing about a
//! computer outlives its stop.
//!
//! A computer that stays within its stated size is never stopped by this guard, however little the
//! host has left: it did not use more than it was given, and stopping it frees nothing.

use crate::runtime::RuntimeError;
use std::path::Path;

/// Free space the host keeps, unless [`MARGIN_ENV`] says otherwise. The engine's own writes (its
/// database, logs and SSH state), the host's journal, an image unpacked for a first start (about
/// 1.5 GB) and the copy a checkpoint of a running computer makes all need room after the guard
/// has looked, and a full root file system breaks the host's services, not only Silo. 5 GiB covers
/// those on a host of the size Silo is meant for and still leaves a 25 GB disk mostly usable.
pub const DEFAULT_MARGIN_MIB: u64 = 5 * 1024;
/// The engine's setting for the margin, in MiB (the engine has no settings file on a Linux host; a
/// systemd override of `silod` sets it).
pub const MARGIN_ENV: &str = "SILO_CONTAINER_DISK_MARGIN_MIB";

/// The file in a computer's state folder that holds why the guard stopped it. A start removes it.
pub const NOTICE: &str = "host-disk-notice";

/// How often the guard looks at the free space.
pub const CHECK_EVERY: std::time::Duration = std::time::Duration::from_secs(30);
/// How often it looks (and measures) while the host is below the margin.
pub const CHECK_EVERY_WHEN_LOW: std::time::Duration = std::time::Duration::from_secs(60);

fn margin_from(setting: Option<&str>) -> u64 {
    setting
        .and_then(|text| text.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_MARGIN_MIB)
        .saturating_mul(1 << 20)
}

/// The free space the host keeps, in bytes.
pub fn margin_bytes() -> u64 {
    margin_from(std::env::var(MARGIN_ENV).ok().as_deref())
}

/// The size a computer's writable layer may reach before the guard may stop it, in bytes.
pub fn limit_bytes(record: &super::layout::ComputerRecord) -> u64 {
    record
        .root_disk_mib
        .saturating_add(record.workspace_mib)
        .saturating_mul(1 << 20)
}

fn size(bytes: u64) -> String {
    crate::backup::format_bytes(bytes)
}

/// Why `what` was refused, in words for the person who asked.
pub fn refusal(what: &str, free: u64, margin: u64) -> String {
    format!(
        "This device's disk is nearly full ({} free; Silo keeps at least {} free), so Silo did not {what}. Free some space on this device, then try again.",
        size(free),
        size(margin)
    )
}

/// What a computer's status says after the guard stopped it.
pub fn stop_notice(free: u64, margin: u64, sizes: &Sizes) -> String {
    format!(
        "Silo stopped this computer because the disk of this device was nearly full ({} free; Silo keeps at least {} free) and the computer's files ({}) were over its size of {}. Free some space on this device, then start it again.",
        size(free),
        size(margin),
        size(sizes.layer),
        size(sizes.limit)
    )
}

/// Refuses `what` when the file system holding `home` has less than the margin free. A free-space
/// reading that fails does not refuse: the guard protects the host and must not break a computer
/// because `statvfs` did.
pub fn require_room(home: &Path, what: &str) -> Result<(), RuntimeError> {
    check(
        crate::transfer_area::free_space(home).ok(),
        margin_bytes(),
        what,
    )
}

fn check(free: Option<u64>, margin: u64, what: &str) -> Result<(), RuntimeError> {
    match free {
        Some(free) if free < margin => Err(RuntimeError::Unavailable(refusal(what, free, margin))),
        _ => Ok(()),
    }
}

/// A running computer's writable layer and the size it was given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sizes {
    pub layer: u64,
    pub limit: u64,
}

/// The computers to stop, largest layer first: those whose layer is over their stated size. An
/// unmeasured computer is left alone.
pub fn select(measured: Vec<(String, Option<Sizes>)>) -> Vec<(String, Sizes)> {
    let mut over: Vec<(String, Sizes)> = measured
        .into_iter()
        .filter_map(|(name, sizes)| sizes.map(|sizes| (name, sizes)))
        .filter(|(_, sizes)| sizes.layer > sizes.limit)
        .collect();
    // Largest first; the name settles a tie so the order is the same on every pass.
    over.sort_by(|a, b| b.1.layer.cmp(&a.1.layer).then_with(|| a.0.cmp(&b.0)));
    over
}

/// One look at the host. With `free` at or above `margin` it does nothing and measures nothing.
/// Below it, it measures each of `running`, and stops the ones [`select`] names, largest first,
/// through `stop`. Returns the names it stopped.
pub fn pass(
    free: u64,
    margin: u64,
    running: Vec<String>,
    measure: impl Fn(&str) -> Option<Sizes>,
    mut stop: impl FnMut(&str, &Sizes) -> bool,
) -> Vec<String> {
    if free >= margin {
        return Vec::new();
    }
    let measured = running
        .into_iter()
        .map(|name| {
            let sizes = measure(&name);
            (name, sizes)
        })
        .collect();
    select(measured)
        .into_iter()
        .filter(|(name, sizes)| stop(name, sizes))
        .map(|(name, _)| name)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    const GIB: u64 = 1 << 30;

    fn sizes(layer_gib: u64, limit_gib: u64) -> Option<Sizes> {
        Some(Sizes {
            layer: layer_gib * GIB,
            limit: limit_gib * GIB,
        })
    }

    #[test]
    fn the_margin_is_five_gib_unless_the_setting_says_otherwise() {
        assert_eq!(margin_from(None), 5 * GIB);
        assert_eq!(margin_from(Some("2048")), 2 * GIB);
        assert_eq!(margin_from(Some(" 1024 ")), GIB);
        assert_eq!(margin_from(Some("lots")), 5 * GIB);
        assert_eq!(margin_from(Some("")), 5 * GIB);
    }

    #[test]
    fn the_limit_adds_the_workspace_to_the_root_disk() {
        let home = tempfile::tempdir().unwrap();
        crate::containers::capability::make_container(home.path(), "box");
        let record = super::super::layout::Layout::new(home.path())
            .computer("box")
            .read_record()
            .unwrap();
        assert_eq!(
            limit_bytes(&record),
            (record.root_disk_mib + record.workspace_mib) << 20
        );
    }

    #[test]
    fn below_the_margin_work_is_refused_with_a_plain_message_and_otherwise_not() {
        assert!(check(Some(6 * GIB), 5 * GIB, "start a container computer").is_ok());
        assert!(check(Some(5 * GIB), 5 * GIB, "start a container computer").is_ok());
        assert!(check(None, 5 * GIB, "start a container computer").is_ok());
        let RuntimeError::Unavailable(message) =
            check(Some(2 * GIB), 5 * GIB, "make a checkpoint").unwrap_err()
        else {
            panic!("not an Unavailable error")
        };
        assert!(message.contains("nearly full"), "{message}");
        assert!(message.contains("2.0 GiB free"), "{message}");
        assert!(message.contains("make a checkpoint"), "{message}");
        assert!(message.contains("Free some space"), "{message}");
    }

    #[test]
    fn a_missing_home_does_not_refuse() {
        // The nearest existing parent is read, so a path that is not made yet still resolves, and a
        // host with room passes.
        let home = tempfile::tempdir().unwrap();
        assert!(require_room(&home.path().join("not-yet"), "start a container computer").is_ok());
    }

    #[test]
    fn only_computers_over_their_stated_size_are_chosen_largest_first() {
        let chosen = select(vec![
            ("small-over".into(), sizes(9, 8)),
            ("within".into(), sizes(8, 8)),
            ("huge-within".into(), sizes(60, 80)),
            ("biggest".into(), sizes(30, 8)),
            ("unmeasured".into(), None),
            ("also-over".into(), sizes(9, 8)),
        ]);
        let names: Vec<&str> = chosen.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["biggest", "also-over", "small-over"]);
    }

    #[test]
    fn with_room_nothing_is_measured_or_stopped() {
        let measured = RefCell::new(0);
        let stopped = pass(
            6 * GIB,
            5 * GIB,
            vec!["a".into()],
            |_| {
                *measured.borrow_mut() += 1;
                sizes(50, 8)
            },
            |_, _| true,
        );
        assert!(stopped.is_empty());
        assert_eq!(*measured.borrow(), 0);
    }

    #[test]
    fn below_the_margin_the_oversized_are_stopped_largest_first_and_the_rest_left() {
        let order = RefCell::new(Vec::new());
        let stopped = pass(
            GIB,
            5 * GIB,
            vec!["within".into(), "big".into(), "bigger".into()],
            |name| match name {
                "within" => sizes(1, 8),
                "big" => sizes(10, 8),
                _ => sizes(20, 8),
            },
            |name, sizes| {
                order.borrow_mut().push((name.to_owned(), sizes.layer / GIB));
                true
            },
        );
        assert_eq!(stopped, ["bigger", "big"]);
        assert_eq!(
            *order.borrow(),
            [("bigger".to_owned(), 20), ("big".to_owned(), 10)]
        );
    }

    #[test]
    fn a_computer_that_would_not_stop_is_not_reported_stopped() {
        let stopped = pass(
            0,
            GIB,
            vec!["a".into(), "b".into()],
            |_| sizes(10, 1),
            |name, _| name == "b",
        );
        assert_eq!(stopped, ["b"]);
    }

    #[test]
    fn the_stop_notice_names_what_happened_and_what_to_do() {
        let message = stop_notice(GIB, 5 * GIB, &Sizes { layer: 30 * GIB, limit: 8 * GIB });
        assert!(message.starts_with("Silo stopped this computer"), "{message}");
        assert!(message.contains("30.0 GiB"), "{message}");
        assert!(message.contains("8.0 GiB"), "{message}");
        assert!(message.contains("start it again"), "{message}");
    }
}
