//! The engine's own upkeep timers (95 A-26).
//!
//! Secret revocation retries, expired-log cleanup of stopped computers and published-port
//! repair used to ride on client state reads. An always-on host Mac has no client most of the
//! time, so each pass now also runs on a timer here. The passes are the same functions the
//! reads called, so their guards stay: one revocation pass at a time, one port-repair pass at
//! a time (and a minimum interval per macOS computer), expired logs at most hourly per
//! stopped computer under a hidden try-lock on the gate. Work that finds its computer busy
//! is skipped and retried by the next timer pass.
//!
//! A fourth pass keeps device health honest with no client connected (60 `connections`): every
//! 10 s it asks the saved devices that are due whether they still answer, one at a time, each
//! within 20 s, at most 16 per pass, none for longer than 90 s in all. A device that answers is
//! left alone for 30 s after any contact; one that fails waits twice as long after each
//! failure, up to five minutes. The results are recorded and published as
//! `connections.devices.changed` when something visible changed.
//!
//! The intervals are not sourced in the decisions (05 §10 OQ23); they are chosen here:
//! revocations every minute (the pass returns at once when none is pending), ports every 30 s
//! (macOS repair also keeps its own 10 s floor per computer), logs every 10 minutes (the
//! hourly cap per computer lives in the pass). The first run waits a start delay, so launch
//! recovery goes first.

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::JoinHandle,
    time::Duration,
};

use crate::{context::SilodContext, methods::computers::SilodRuntimeHost};

pub const START_DELAY: Duration = Duration::from_secs(30);
pub const REVOCATION_INTERVAL: Duration = Duration::from_secs(60);
pub const PORT_REPAIR_INTERVAL: Duration = Duration::from_secs(30);
pub const LOG_EXPIRY_INTERVAL: Duration = Duration::from_secs(10 * 60);
/// How often the device pass looks for devices that are due. Each device answers to its own
/// schedule inside the pass: [`DEVICE_QUIET_FOR`], doubling after each failure.
pub const DEVICE_PROBE_INTERVAL: Duration = Duration::from_secs(10);
/// A device that answers is probed again after this long without any other contact.
pub const DEVICE_QUIET_FOR: Duration = Duration::from_secs(30);
/// Devices one pass probes at most.
pub const DEVICES_PER_PASS: usize = 16;

/// The three passes, and whether the engine may run them now.
pub trait Upkeep: Send + Sync + 'static {
    fn retry_revocations(&self);
    fn expire_logs(&self);
    fn repair_ports(&self);
    /// Asks the saved devices that are due whether they still answer.
    fn probe_devices(&self);
    /// True while the engine is shutting down or the storage migration blocks operations.
    fn paused(&self) -> bool;
}

/// How often each pass runs.
#[derive(Clone, Copy)]
pub struct Schedule {
    pub start_delay: Duration,
    pub revocations: Duration,
    pub ports: Duration,
    pub logs: Duration,
    pub devices: Duration,
}

impl Schedule {
    pub const ENGINE: Schedule = Schedule {
        start_delay: START_DELAY,
        revocations: REVOCATION_INTERVAL,
        ports: PORT_REPAIR_INTERVAL,
        logs: LOG_EXPIRY_INTERVAL,
        devices: DEVICE_PROBE_INTERVAL,
    };
}

/// Running timers. Dropping the handle does not stop them; `stop` does (tests).
pub struct Timers {
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
}

impl Timers {
    /// Starts one thread per pass, so a slow pass never delays another.
    pub fn start(upkeep: Arc<dyn Upkeep>, schedule: Schedule) -> Timers {
        let stop = Arc::new(AtomicBool::new(false));
        let passes: [(&str, Duration, fn(&dyn Upkeep)); 4] = [
            ("silod-timer-revocations", schedule.revocations, |upkeep| {
                upkeep.retry_revocations()
            }),
            ("silod-timer-ports", schedule.ports, |upkeep| {
                upkeep.repair_ports()
            }),
            ("silod-timer-logs", schedule.logs, |upkeep| {
                upkeep.expire_logs()
            }),
            ("silod-timer-devices", schedule.devices, |upkeep| {
                upkeep.probe_devices()
            }),
        ];
        let mut threads = Vec::new();
        for (name, interval, pass) in passes {
            let (upkeep, stop) = (upkeep.clone(), stop.clone());
            let spawned = std::thread::Builder::new()
                .name(name.into())
                .spawn(move || run(&*upkeep, &stop, schedule.start_delay, interval, pass));
            match spawned {
                Ok(thread) => threads.push(thread),
                Err(error) => eprintln!("silod: could not start {name}: {error}"),
            }
        }
        Timers { stop, threads }
    }

    pub fn stop(self) {
        self.stop.store(true, Ordering::SeqCst);
        for thread in self.threads {
            let _ = thread.join();
        }
    }
}

fn run(
    upkeep: &dyn Upkeep,
    stop: &AtomicBool,
    start_delay: Duration,
    interval: Duration,
    pass: fn(&dyn Upkeep),
) {
    if !sleep(stop, start_delay) {
        return;
    }
    loop {
        if !upkeep.paused() {
            // A panic in one pass must not end the timer.
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| pass(upkeep)));
        }
        if !sleep(stop, interval) {
            return;
        }
    }
}

/// Sleeps in short steps so a stop is noticed. False when stopped.
fn sleep(stop: &AtomicBool, total: Duration) -> bool {
    let step = Duration::from_millis(10).min(total);
    let mut slept = Duration::ZERO;
    while slept < total {
        if stop.load(Ordering::SeqCst) {
            return false;
        }
        std::thread::sleep(step);
        slept += step;
    }
    !stop.load(Ordering::SeqCst)
}

/// The real passes over silod's context.
pub struct SilodUpkeep {
    context: Arc<SilodContext>,
}

impl SilodUpkeep {
    pub fn new(context: Arc<SilodContext>) -> Self {
        Self { context }
    }

    fn host(&self) -> SilodRuntimeHost {
        SilodRuntimeHost::for_engine(self.context.clone())
    }
}

impl Upkeep for SilodUpkeep {
    fn retry_revocations(&self) {
        silo_engine::secrets::schedule_revocations();
    }

    fn expire_logs(&self) {
        if let Err(error) = silo_engine::runtime::expire_stopped_logs(&self.host()) {
            eprintln!("silod: expired-log cleanup skipped: {error}");
        }
    }

    fn repair_ports(&self) {
        let host = self.host();
        if let Ok(paths) = silo_engine::runtime::runtime_paths(&host) {
            let _ = silo_engine::network::repair_published_ports(&paths);
        }
        let _ = silo_engine::macos_computers::repair_published_ports(&host.macos_context());
    }

    fn probe_devices(&self) {
        silo_engine::remote::probe_devices(silo_engine::remote::ProbeSchedule {
            interval: DEVICE_QUIET_FOR,
            max_devices: DEVICES_PER_PASS,
        });
    }

    fn paused(&self) -> bool {
        use silo_engine::runtime::RuntimeHost;
        self.context.engine_state().shutting_down || self.host().blocks_operations()
    }
}

/// Starts the engine timers. Call once, after the context and the secrets runtime are installed.
pub fn start(context: Arc<SilodContext>) -> Timers {
    Timers::start(Arc::new(SilodUpkeep::new(context)), Schedule::ENGINE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[derive(Default)]
    struct Counting {
        revocations: AtomicUsize,
        logs: AtomicUsize,
        ports: AtomicUsize,
        devices: AtomicUsize,
        paused: AtomicBool,
        panic_in_logs: AtomicBool,
    }

    impl Upkeep for Counting {
        fn retry_revocations(&self) {
            self.revocations.fetch_add(1, Ordering::SeqCst);
        }
        fn expire_logs(&self) {
            self.logs.fetch_add(1, Ordering::SeqCst);
            if self.panic_in_logs.load(Ordering::SeqCst) {
                panic!("a pass failed");
            }
        }
        fn repair_ports(&self) {
            self.ports.fetch_add(1, Ordering::SeqCst);
        }
        fn probe_devices(&self) {
            self.devices.fetch_add(1, Ordering::SeqCst);
        }
        fn paused(&self) -> bool {
            self.paused.load(Ordering::SeqCst)
        }
    }

    const FAST: Schedule = Schedule {
        start_delay: Duration::from_millis(20),
        revocations: Duration::from_millis(20),
        ports: Duration::from_millis(20),
        logs: Duration::from_millis(20),
        devices: Duration::from_millis(20),
    };

    fn wait_until(condition: impl Fn() -> bool) {
        for _ in 0..500 {
            if condition() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the timer passes did not run");
    }

    #[test]
    fn every_pass_runs_repeatedly_with_no_client_involved() {
        let upkeep = Arc::new(Counting::default());
        let timers = Timers::start(upkeep.clone(), FAST);
        wait_until(|| {
            upkeep.revocations.load(Ordering::SeqCst) >= 2
                && upkeep.logs.load(Ordering::SeqCst) >= 2
                && upkeep.ports.load(Ordering::SeqCst) >= 2
                && upkeep.devices.load(Ordering::SeqCst) >= 2
        });
        timers.stop();
    }

    #[test]
    fn nothing_runs_while_the_engine_is_paused_and_it_resumes_after() {
        let upkeep = Arc::new(Counting::default());
        upkeep.paused.store(true, Ordering::SeqCst);
        let timers = Timers::start(upkeep.clone(), FAST);
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(upkeep.revocations.load(Ordering::SeqCst), 0);
        assert_eq!(upkeep.logs.load(Ordering::SeqCst), 0);
        assert_eq!(upkeep.ports.load(Ordering::SeqCst), 0);
        assert_eq!(upkeep.devices.load(Ordering::SeqCst), 0);
        upkeep.paused.store(false, Ordering::SeqCst);
        wait_until(|| upkeep.revocations.load(Ordering::SeqCst) >= 1);
        timers.stop();
    }

    #[test]
    fn a_failing_pass_does_not_end_its_timer_or_the_others() {
        let upkeep = Arc::new(Counting::default());
        upkeep.panic_in_logs.store(true, Ordering::SeqCst);
        let timers = Timers::start(upkeep.clone(), FAST);
        wait_until(|| {
            upkeep.logs.load(Ordering::SeqCst) >= 2 && upkeep.ports.load(Ordering::SeqCst) >= 2
        });
        timers.stop();
    }

    #[test]
    fn a_stopped_timer_never_runs_again() {
        let upkeep = Arc::new(Counting::default());
        let timers = Timers::start(
            upkeep.clone(),
            Schedule {
                start_delay: Duration::from_secs(60),
                ..FAST
            },
        );
        timers.stop();
        assert_eq!(upkeep.revocations.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn the_real_passes_run_on_an_empty_engine_without_a_client() {
        use silo_engine::context::{
            EngineIdentity, EnginePaths, MainThread, PackageKind, ProcessControl,
        };
        struct NoMain;
        impl MainThread for NoMain {
            fn run(&self, _work: Box<dyn FnOnce() + Send>) -> Result<(), String> {
                Ok(())
            }
        }
        struct NoProcess;
        impl ProcessControl for NoProcess {
            fn exit(&self, _code: i32) {}
            fn restart(&self) -> ! {
                panic!("restart is not expected")
            }
        }
        let _gate = crate::methods::process_gate_lock();
        let _registry = crate::methods::MACOS_REGISTRY_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let directory = tempfile::tempdir().unwrap();
        let context = Arc::new(SilodContext::new(
            EnginePaths {
                home: directory.path().join("home"),
                app_data: directory.path().join("data"),
                resources: directory.path().join("resources"),
                ..EnginePaths::default()
            },
            EngineIdentity {
                channel: silo_engine::channel::Channel::Development,
                version: "0.1.0".into(),
                package_kind: PackageKind::MacApp,
            },
            crate::identity::LocalDeviceIdentity {
                device_id: "local-device".into(),
                name: "Local Mac".into(),
            },
            Arc::new(NoMain),
            Arc::new(NoProcess),
        ));
        let upkeep = SilodUpkeep::new(context);
        assert!(!upkeep.paused());
        upkeep.retry_revocations();
        upkeep.repair_ports();
        upkeep.expire_logs();
        upkeep.probe_devices();
    }
}
