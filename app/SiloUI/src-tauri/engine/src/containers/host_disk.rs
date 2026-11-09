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
//! host has left: it did not use more than it was given, and stopping it frees nothing. A layer the
//! guard cannot measure (the measurement timed out, or `du` itself failed or printed nothing
//! usable) counts as over its size, and as larger than any measured one: a computer must not be
//! able to avoid the guard by making itself slow to measure. It is stopped as soon as its own
//! measurement ends, not after the others. A tool around `du` that fails (the user namespace
//! cannot be set up, say) says nothing about the layer: that is logged and the computer is left
//! alone, so a transient host error does not stop every container. The layers are measured a few
//! at a time, so one slow computer does not hold up the rest.

use crate::runtime::RuntimeError;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// How many layers are measured at the same time.
const MEASURE_WORKERS: usize = 4;

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

/// The script the guard runs in the user namespace to measure a layer (its path is `$1`). It ends
/// by printing `du`'s exit status, so a measurement in which `du` ran can be told from one in
/// which the tools around it failed before it did.
pub const DU_SCRIPT: &str = r#"du -sB1 -- "$1"; echo "silo-du-exit:$?""#;

/// What [`DU_SCRIPT`] printed: `None` when `du` never ran (the tools around it failed, which says
/// nothing about the layer), `Some(None)` when it ran and printed no size, and `Some(Some(bytes))`
/// when it printed one (`du` exits non-zero when a file vanished under it; the total still counts).
pub fn du_result(stdout: &str) -> Option<Option<u64>> {
    if !stdout.lines().any(|line| line.starts_with("silo-du-exit:")) {
        return None;
    }
    Some(
        stdout
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().next())
            .and_then(|total| total.parse().ok()),
    )
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
    let files = match sizes.layer {
        Some(layer) => size(layer),
        None => "which Silo could not measure".to_owned(),
    };
    format!(
        "Silo stopped this computer because the disk of this device was nearly full ({} free; Silo keeps at least {} free) and the computer's files ({files}) were over its size of {}. Free some space on this device, then start it again.",
        size(free),
        size(margin),
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

/// A running computer's writable layer and the size it was given. `layer` is `None` when it could
/// not be measured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sizes {
    pub layer: Option<u64>,
    pub limit: u64,
}

impl Sizes {
    /// Whether the guard may stop the computer: its layer is over its size, or unknown.
    fn over(&self) -> bool {
        self.layer.is_none_or(|layer| layer > self.limit)
    }

    /// The layer for ordering: an unmeasured one sorts as larger than any measured one.
    fn rank(&self) -> u64 {
        self.layer.unwrap_or(u64::MAX)
    }
}

/// The computers to stop, those whose layer is over their stated size or could not be measured,
/// largest layer first (unmeasured ones ahead of every measured one). A computer without sizes at
/// all (its record could not be read, so there is nothing to stop) is left alone.
pub fn select(measured: Vec<(String, Option<Sizes>)>) -> Vec<(String, Sizes)> {
    let mut over: Vec<(String, Sizes)> = measured
        .into_iter()
        .filter_map(|(name, sizes)| sizes.map(|sizes| (name, sizes)))
        .filter(|(_, sizes)| sizes.over())
        .collect();
    // Largest first; the name settles a tie so the order is the same on every pass.
    over.sort_by(|a, b| b.1.rank().cmp(&a.1.rank()).then_with(|| a.0.cmp(&b.0)));
    over
}

/// Measures every one of `names` with `measure`, up to [`MEASURE_WORKERS`] at a time, and returns
/// the results in the order of `names`. A slow measurement holds up only its own worker.
fn measure_all<T: Send>(
    names: Vec<String>,
    measure: &(impl Fn(&str) -> T + Sync),
) -> Vec<(String, T)> {
    let next = AtomicUsize::new(0);
    let done: Mutex<Vec<Option<T>>> = Mutex::new((0..names.len()).map(|_| None).collect());
    std::thread::scope(|scope| {
        for _ in 0..names.len().min(MEASURE_WORKERS) {
            scope.spawn(|| loop {
                let index = next.fetch_add(1, Ordering::SeqCst);
                let Some(name) = names.get(index) else {
                    break;
                };
                let result = measure(name);
                done.lock().unwrap_or_else(|e| e.into_inner())[index] = Some(result);
            });
        }
    });
    let done = done.into_inner().unwrap_or_else(|e| e.into_inner());
    names
        .into_iter()
        .zip(done)
        .filter_map(|(name, result)| result.map(|result| (name, result)))
        .collect()
}

/// One look at the host. With `free` at or above `margin` it does nothing and measures nothing.
/// Below it, it measures each of `running` (a few at a time) and stops, through `stop`, the ones
/// [`select`] names, largest first; a layer that could not be measured is stopped as soon as its
/// own measurement ends. Returns the names it stopped.
pub fn pass(
    free: u64,
    margin: u64,
    running: Vec<String>,
    measure: impl Fn(&str) -> Option<Sizes> + Sync,
    stop: impl Fn(&str, &Sizes) -> bool + Sync,
) -> Vec<String> {
    if free >= margin {
        return Vec::new();
    }
    let tried: Mutex<Vec<(String, bool)>> = Mutex::new(Vec::new());
    let measured = measure_all(running, &|name: &str| {
        let sizes = measure(name);
        if let Some(unmeasured) = sizes.as_ref().filter(|sizes| sizes.layer.is_none()) {
            let stopped = stop(name, unmeasured);
            tried
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push((name.to_owned(), stopped));
        }
        sizes
    });
    let tried = tried.into_inner().unwrap_or_else(|e| e.into_inner());
    let rest: Vec<String> = select(measured)
        .into_iter()
        .filter(|(name, _)| !tried.iter().any(|(done, _)| done == name))
        .filter(|(name, sizes)| stop(name, sizes))
        .map(|(name, _)| name)
        .collect();
    tried
        .into_iter()
        .filter(|(_, stopped)| *stopped)
        .map(|(name, _)| name)
        .chain(rest)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use std::time::{Duration, Instant};

    const GIB: u64 = 1 << 30;

    fn sizes(layer_gib: u64, limit_gib: u64) -> Option<Sizes> {
        Some(Sizes {
            layer: Some(layer_gib * GIB),
            limit: limit_gib * GIB,
        })
    }

    fn unmeasured(limit_gib: u64) -> Option<Sizes> {
        Some(Sizes {
            layer: None,
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
            ("no-record".into(), None),
            ("also-over".into(), sizes(9, 8)),
        ]);
        let names: Vec<&str> = chosen.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["biggest", "also-over", "small-over"]);
    }

    #[test]
    fn with_room_nothing_is_measured_or_stopped() {
        let measured = AtomicUsize::new(0);
        let stopped = pass(
            6 * GIB,
            5 * GIB,
            vec!["a".into()],
            |_| {
                measured.fetch_add(1, Ordering::SeqCst);
                sizes(50, 8)
            },
            |_, _| true,
        );
        assert!(stopped.is_empty());
        assert_eq!(measured.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn below_the_margin_the_oversized_are_stopped_largest_first_and_the_rest_left() {
        let order = Mutex::new(Vec::new());
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
                order
                    .lock()
                    .unwrap()
                    .push((name.to_owned(), sizes.layer.unwrap() / GIB));
                true
            },
        );
        assert_eq!(stopped, ["bigger", "big"]);
        assert_eq!(
            *order.lock().unwrap(),
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
        let message = stop_notice(
            GIB,
            5 * GIB,
            &Sizes {
                layer: Some(30 * GIB),
                limit: 8 * GIB,
            },
        );
        assert!(
            message.starts_with("Silo stopped this computer"),
            "{message}"
        );
        assert!(message.contains("30.0 GiB"), "{message}");
        assert!(message.contains("8.0 GiB"), "{message}");
        assert!(message.contains("start it again"), "{message}");
    }

    #[test]
    fn a_layer_that_could_not_be_measured_is_chosen_even_if_it_might_be_within_its_size() {
        let chosen = select(vec![
            ("within".into(), sizes(1, 8)),
            ("slow".into(), unmeasured(80)),
        ]);
        let names: Vec<&str> = chosen.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["slow"]);
    }

    #[test]
    fn an_unmeasured_layer_sorts_ahead_of_every_measured_one() {
        let chosen = select(vec![
            ("big".into(), sizes(500, 8)),
            ("z-unknown".into(), unmeasured(8)),
            ("a-unknown".into(), unmeasured(8)),
            ("small".into(), sizes(9, 8)),
        ]);
        let names: Vec<&str> = chosen.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["a-unknown", "z-unknown", "big", "small"]);
    }

    #[test]
    fn below_the_margin_an_unmeasurable_computer_is_stopped_first_and_says_so() {
        let notices = Mutex::new(Vec::new());
        let stopped = pass(
            GIB,
            5 * GIB,
            vec!["fine".into(), "big".into(), "slow".into()],
            |name| match name {
                "fine" => sizes(1, 8),
                "big" => sizes(20, 8),
                _ => unmeasured(80),
            },
            |name, sizes| {
                notices
                    .lock()
                    .unwrap()
                    .push((name.to_owned(), stop_notice(GIB, 5 * GIB, sizes)));
                true
            },
        );
        assert_eq!(stopped, ["slow", "big"]);
        let notices = notices.into_inner().unwrap();
        assert!(
            notices[0].1.contains("could not measure"),
            "{}",
            notices[0].1
        );
        assert!(notices[0].1.contains("80.0 GiB"), "{}", notices[0].1);
    }

    #[test]
    fn a_slow_measurement_does_not_hold_up_the_others() {
        let slow = Duration::from_millis(600);
        let started = Instant::now();
        let finished = Mutex::new(Vec::new());
        let stopped = pass(
            0,
            GIB,
            vec!["slow".into(), "a".into(), "b".into(), "c".into()],
            |name| {
                if name == "slow" {
                    std::thread::sleep(slow);
                } else {
                    std::thread::sleep(Duration::from_millis(20));
                }
                finished
                    .lock()
                    .unwrap()
                    .push((name.to_owned(), started.elapsed()));
                sizes(10, 1)
            },
            |_, _| true,
        );
        let total = started.elapsed();
        assert_eq!(stopped.len(), 4);
        let finished = finished.into_inner().unwrap();
        assert_eq!(finished.len(), 4);
        assert_eq!(finished.last().unwrap().0, "slow", "{finished:?}");
        for (name, at) in &finished {
            if name != "slow" {
                assert!(*at < slow / 2, "{name} waited for the slow one: {at:?}");
            }
        }
        // About one sleep, not the sum of them.
        assert!(total >= slow && total < slow * 2, "{total:?}");
    }

    #[test]
    fn more_computers_than_workers_are_all_measured_in_order_of_the_list() {
        let names: Vec<String> = (0..11).map(|n| format!("c{n:02}")).collect();
        let results = measure_all(names.clone(), &|name: &str| name.to_owned());
        let got: Vec<String> = results.into_iter().map(|(_, value)| value).collect();
        assert_eq!(got, names);
    }

    #[test]
    fn a_layer_that_could_not_be_measured_is_stopped_when_its_own_measurement_ends() {
        let unknown_stopped = AtomicBool::new(false);
        let slow_saw_it_stopped = AtomicBool::new(false);
        let stops = Mutex::new(Vec::new());
        let stopped = pass(
            0,
            GIB,
            vec!["slow".into(), "unknown".into(), "big".into()],
            |name| match name {
                "slow" => {
                    // This measurement ends only once the unmeasured layer has been stopped (or,
                    // were it never stopped before the pass was over, after a long wait).
                    let deadline = Instant::now() + Duration::from_secs(20);
                    while !unknown_stopped.load(Ordering::SeqCst) && Instant::now() < deadline {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    slow_saw_it_stopped
                        .store(unknown_stopped.load(Ordering::SeqCst), Ordering::SeqCst);
                    sizes(20, 8)
                }
                "unknown" => unmeasured(8),
                _ => sizes(30, 8),
            },
            |name, _| {
                stops.lock().unwrap().push(name.to_owned());
                if name == "unknown" {
                    unknown_stopped.store(true, Ordering::SeqCst);
                }
                true
            },
        );
        assert_eq!(stopped, ["unknown", "big", "slow"]);
        // Each is stopped once, and the unmeasured one did not wait for the slow measurement.
        assert_eq!(stops.into_inner().unwrap(), ["unknown", "big", "slow"]);
        assert!(slow_saw_it_stopped.load(Ordering::SeqCst));
    }

    #[test]
    fn an_unmeasured_layer_that_would_not_stop_is_not_tried_twice() {
        let tries = AtomicUsize::new(0);
        let stopped = pass(
            0,
            GIB,
            vec!["unknown".into()],
            |_| unmeasured(8),
            |_, _| {
                tries.fetch_add(1, Ordering::SeqCst);
                false
            },
        );
        assert!(stopped.is_empty());
        assert_eq!(tries.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn only_a_measurement_in_which_du_ran_says_anything_about_the_layer() {
        // `du` printed its total.
        assert_eq!(
            du_result("4096\t/layer\nsilo-du-exit:0\n"),
            Some(Some(4096))
        );
        // `du` exits non-zero when a file vanished under it; its total counts.
        assert_eq!(
            du_result("8192\t/layer\nsilo-du-exit:1\n"),
            Some(Some(8192))
        );
        // `du` ran and printed nothing readable: the layer cannot be measured.
        assert_eq!(du_result("silo-du-exit:1\n"), Some(None));
        assert_eq!(du_result("lots\t/layer\nsilo-du-exit:0\n"), Some(None));
        // The user namespace could not be set up (nothing ran inside it): says nothing.
        assert_eq!(du_result(""), None);
        assert_eq!(du_result("4096\t/layer\n"), None);
    }
}
