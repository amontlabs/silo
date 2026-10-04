//! Sound and screen-size commands for a desktop viewer's shell window.
//!
//! The commands drive the guest page through `desktop_bridge::Bridge`; the
//! decisions live in functions over the small [`Media`] trait so tests can run
//! them without a webview.
use crate::desktop_bridge::{Bridge, Capabilities};
use crate::desktop_viewer::{is_viewer_label, require_computer, with_bridge};
use serde::Serialize;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Window};

/// The screen size the toolbar's reset item requests.
pub(crate) const RESET_WIDTH: u32 = 1440;
pub(crate) const RESET_HEIGHT: u32 = 900;

/// How long one probe waits for the page's answer.
const PROBE_WAIT: Duration = Duration::from_secs(2);
/// The transport opens shortly after the page loads; probing stops after this
/// many attempts spaced by `PROBE_PAUSE`.
const PROBE_ATTEMPTS: u32 = 20;
const PROBE_PAUSE: Duration = Duration::from_millis(500);

/// The longest one viewer's sound check keeps probing, however many times the page
/// reloads and restarts it.
const PROBE_DEADLINE: Duration = Duration::from_secs(60);

const SUPERSEDED: &str = "The desktop sound check was cancelled.";
const NOT_READY: &str = "The desktop display is not ready.";

/// The operations on a viewer's page that sound and resizing need.
pub(crate) trait Media {
    fn capabilities(&self, timeout: Duration) -> Result<Capabilities, String>;
    fn set_audio_muted(&self, muted: bool) -> Result<(), String>;
    fn set_audio_active(&self, active: bool) -> Result<(), String>;
    fn reset_resolution(&self, width: u32, height: u32) -> Result<(), String>;
}

impl Media for Bridge<'_> {
    fn capabilities(&self, timeout: Duration) -> Result<Capabilities, String> {
        Bridge::capabilities(self, timeout)
    }
    fn set_audio_muted(&self, muted: bool) -> Result<(), String> {
        Bridge::set_audio_muted(self, muted)
    }
    fn set_audio_active(&self, active: bool) -> Result<(), String> {
        Bridge::set_audio_active(self, active)
    }
    fn reset_resolution(&self, width: u32, height: u32) -> Result<(), String> {
        Bridge::reset_resolution(self, width, height)
    }
}

/// One probe: `None` while the page's transport is not open yet, otherwise
/// whether this web engine can play the computer's sound. When it cannot, the
/// computer is told to stop encoding audio.
pub(crate) fn probe_step(media: &dyn Media) -> Result<Option<bool>, String> {
    let capabilities = media.capabilities(PROBE_WAIT)?;
    if !capabilities.transport {
        return Ok(None);
    }
    if capabilities.opus {
        return Ok(Some(true));
    }
    media.set_audio_active(false)?;
    Ok(Some(false))
}

/// Repeats `step` until it answers, treating errors (no connection or page
/// yet) like an unopened transport, for at most `attempts` tries. It stops as
/// soon as `obsolete` reports that nobody wants the answer any more.
pub(crate) fn probe(
    mut step: impl FnMut() -> Result<Option<bool>, String>,
    attempts: u32,
    pause: Duration,
    obsolete: impl Fn() -> bool,
) -> Result<bool, String> {
    let mut last = NOT_READY.to_string();
    for attempt in 0..attempts {
        if obsolete() {
            return Err(SUPERSEDED.into());
        }
        match step() {
            Ok(Some(supported)) => return Ok(supported),
            Ok(None) => {}
            Err(error) => last = error,
        }
        if attempt + 1 < attempts {
            std::thread::sleep(pause);
        }
    }
    Err(last)
}

/// Applies the viewer's sound state: the mute choice always, and whether the
/// computer streams audio at all (off while the window is hidden).
pub(crate) fn apply_audio(media: &dyn Media, muted: bool, active: bool) -> Result<(), String> {
    media.set_audio_muted(muted)?;
    media.set_audio_active(active)
}

/// Applies `revision` unless a newer one already ran, so updates that reach
/// the worker pool out of order cannot leave an older state applied last. The
/// lock serializes updates to one page.
pub(crate) fn apply_audio_ordered(
    latest: &Mutex<u64>,
    revision: u64,
    media: &dyn Media,
    muted: bool,
    active: bool,
) -> Result<(), String> {
    let mut latest = latest.lock().map_err(|_| "Desktop sound unavailable.")?;
    if revision < *latest {
        return Ok(());
    }
    *latest = revision;
    apply_audio(media, muted, active)
}

type Outcome = Result<bool, String>;

struct ProbeSlot {
    /// Bumped by every request and cancellation; a probe that sees a newer
    /// value than the one it started with is obsolete.
    epoch: u64,
    running: bool,
    waiters: Vec<tokio::sync::oneshot::Sender<Outcome>>,
}

/// Sound probes per viewer window. At most one worker probes a viewer at a
/// time: a request made while it runs restarts its probe so the answer describes
/// the newest page, and settles the older request as superseded, so a viewer has
/// at most one waiter. A cancellation drops the waiter and ends the probe at its
/// next step. A worker that ends removes its viewer's slot.
#[derive(Default)]
pub(crate) struct Probes {
    slots: Mutex<HashMap<String, ProbeSlot>>,
}

impl Probes {
    /// Registers a waiter for the viewer's next answer. The flag says the
    /// caller must run [`Probes::work`] because no worker is running.
    pub(crate) fn enlist(
        &self,
        label: &str,
    ) -> Result<(tokio::sync::oneshot::Receiver<Outcome>, bool), String> {
        let mut slots = self
            .slots
            .lock()
            .map_err(|_| "Desktop sound unavailable.")?;
        let slot = slots.entry(label.to_string()).or_insert(ProbeSlot {
            epoch: 0,
            running: false,
            waiters: Vec::new(),
        });
        slot.epoch += 1;
        for superseded in slot.waiters.drain(..) {
            let _ = superseded.send(Err(SUPERSEDED.into()));
        }
        let (sender, receiver) = tokio::sync::oneshot::channel();
        slot.waiters.push(sender);
        let start = !slot.running;
        slot.running = true;
        Ok((receiver, start))
    }

    /// Abandons the viewer's waiting requests and any probe in progress.
    pub(crate) fn cancel(&self, label: &str) {
        if let Ok(mut slots) = self.slots.lock() {
            if let Some(slot) = slots.get_mut(label) {
                slot.epoch += 1;
                slot.waiters.clear();
            }
        }
    }

    /// Cancels the viewer's probe and drops its slot unless a worker still
    /// uses it, which removes the slot when it ends.
    fn forget(&self, label: &str) {
        self.cancel(label);
        if let Ok(mut slots) = self.slots.lock() {
            if slots.get(label).is_some_and(|slot| !slot.running) {
                slots.remove(label);
            }
        }
    }

    /// Probes until no request is waiting, answering the waiter with the result
    /// of a probe that no later request or cancellation overtook. Restarts stop
    /// after `deadline` in total, which fails the waiter. `run` receives the
    /// check for an obsolete probe.
    pub(crate) fn work(
        &self,
        label: &str,
        deadline: Duration,
        mut run: impl FnMut(&dyn Fn() -> bool) -> Outcome,
    ) {
        let started = Instant::now();
        loop {
            let epoch = {
                let Ok(mut slots) = self.slots.lock() else {
                    return;
                };
                let Some(slot) = slots.get_mut(label) else {
                    return;
                };
                if slot.waiters.is_empty() {
                    slots.remove(label);
                    return;
                }
                slot.epoch
            };
            let expired = || started.elapsed() >= deadline;
            let obsolete = || {
                expired()
                    || self
                        .slots
                        .lock()
                        .map(|slots| slots.get(label).is_none_or(|slot| slot.epoch != epoch))
                        .unwrap_or(true)
            };
            let outcome = run(&obsolete);
            let Ok(mut slots) = self.slots.lock() else {
                return;
            };
            let Some(slot) = slots.get_mut(label) else {
                return;
            };
            if slot.epoch != epoch && !expired() {
                continue;
            }
            let outcome = if slot.epoch != epoch {
                Err(NOT_READY.to_string())
            } else {
                outcome
            };
            for waiter in slot.waiters.drain(..) {
                let _ = waiter.send(outcome.clone());
            }
            slots.remove(label);
            return;
        }
    }

    #[cfg(test)]
    fn waiting(&self, label: &str) -> usize {
        self.slots
            .lock()
            .unwrap()
            .get(label)
            .map_or(0, |slot| slot.waiters.len())
    }
}

static PROBES: OnceLock<Probes> = OnceLock::new();

fn probes() -> &'static Probes {
    PROBES.get_or_init(Default::default)
}

/// The newest sound revision applied for each viewer window.
static SOUND_REVISIONS: OnceLock<Mutex<HashMap<String, Arc<Mutex<u64>>>>> = OnceLock::new();

fn sound_revision(label: &str) -> Result<Arc<Mutex<u64>>, String> {
    let mut slots = SOUND_REVISIONS
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| "Desktop sound unavailable.")?;
    Ok(slots.entry(label.to_string()).or_default().clone())
}

/// Drops the ordering state of a closed viewer window.
pub(crate) fn forget_viewer(label: &str) {
    probes().forget(label);
    if let Some(slots) = SOUND_REVISIONS.get() {
        if let Ok(mut slots) = slots.lock() {
            slots.remove(label);
        }
    }
}

pub(crate) fn reset_screen(media: &dyn Media) -> Result<(), String> {
    media.reset_resolution(RESET_WIDTH, RESET_HEIGHT)
}

#[derive(Serialize)]
pub(crate) struct SoundSupport {
    sound: bool,
}

fn require_viewer(window: &Window, computer: &str) -> Result<String, String> {
    if !is_viewer_label(window.label()) {
        return Err("Only a desktop viewer can use this command.".into());
    }
    require_computer(window, computer)?;
    Ok(window.label().to_string())
}

/// Reports whether this device can play the computer's sound, waiting briefly
/// for the page's connection. An unsupported engine also stops the audio stream.
#[tauri::command]
pub(crate) async fn desktop_viewer_sound_support(
    app: AppHandle,
    window: Window,
    computer: String,
) -> Result<SoundSupport, String> {
    let label = require_viewer(&window, &computer)?;
    let (answer, start) = probes().enlist(&label)?;
    if start {
        tauri::async_runtime::spawn_blocking(move || {
            probes().work(&label, PROBE_DEADLINE, |obsolete| {
                probe(
                    || with_bridge(&app, &label, |bridge| probe_step(bridge)),
                    PROBE_ATTEMPTS,
                    PROBE_PAUSE,
                    obsolete,
                )
            })
        });
    }
    answer
        .await
        .map_err(|_| SUPERSEDED)?
        .map(|sound| SoundSupport { sound })
}

/// Abandons the viewer's sound check, for a page or connection that is gone.
#[tauri::command]
pub(crate) fn desktop_viewer_sound_cancel(window: Window, computer: String) -> Result<(), String> {
    let label = require_viewer(&window, &computer)?;
    probes().cancel(&label);
    Ok(())
}

#[tauri::command]
pub(crate) async fn desktop_viewer_set_audio(
    app: AppHandle,
    window: Window,
    computer: String,
    muted: bool,
    active: bool,
    revision: u64,
) -> Result<(), String> {
    let label = require_viewer(&window, &computer)?;
    let latest = sound_revision(&label)?;
    tauri::async_runtime::spawn_blocking(move || {
        with_bridge(&app, &label, |bridge| {
            apply_audio_ordered(&latest, revision, bridge, muted, active)
        })
    })
    .await
    .map_err(|_| "Desktop sound change failed.")?
}

#[tauri::command]
pub(crate) async fn desktop_viewer_reset_screen(
    app: AppHandle,
    window: Window,
    computer: String,
) -> Result<(), String> {
    let label = require_viewer(&window, &computer)?;
    tauri::async_runtime::spawn_blocking(move || {
        with_bridge(&app, &label, |bridge| reset_screen(bridge))
    })
    .await
    .map_err(|_| "Desktop screen reset failed.")?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    #[derive(Default)]
    struct Fake {
        capabilities: RefCell<Vec<Result<Capabilities, String>>>,
        calls: RefCell<Vec<String>>,
    }
    impl Fake {
        fn answering(capabilities: Capabilities) -> Self {
            Self {
                capabilities: RefCell::new(vec![Ok(capabilities)]),
                ..Default::default()
            }
        }
    }
    impl Media for Fake {
        fn capabilities(&self, _: Duration) -> Result<Capabilities, String> {
            self.capabilities.borrow_mut().remove(0)
        }
        fn set_audio_muted(&self, muted: bool) -> Result<(), String> {
            self.calls.borrow_mut().push(format!("mute {muted}"));
            Ok(())
        }
        fn set_audio_active(&self, active: bool) -> Result<(), String> {
            self.calls.borrow_mut().push(format!("active {active}"));
            Ok(())
        }
        fn reset_resolution(&self, width: u32, height: u32) -> Result<(), String> {
            self.calls
                .borrow_mut()
                .push(format!("reset {width}x{height}"));
            Ok(())
        }
    }

    fn caps(opus: bool, transport: bool) -> Capabilities {
        Capabilities {
            audio_decoder: opus,
            opus,
            transport,
        }
    }

    #[test]
    fn supported_engine_keeps_the_audio_stream() {
        let fake = Fake::answering(caps(true, true));
        assert_eq!(probe_step(&fake), Ok(Some(true)));
        assert!(fake.calls.borrow().is_empty());
    }

    #[test]
    fn unsupported_engine_stops_the_audio_stream() {
        let fake = Fake::answering(caps(false, true));
        assert_eq!(probe_step(&fake), Ok(Some(false)));
        assert_eq!(*fake.calls.borrow(), ["active false"]);
    }

    #[test]
    fn closed_transport_decides_nothing() {
        let fake = Fake::answering(caps(false, false));
        assert_eq!(probe_step(&fake), Ok(None));
        assert!(fake.calls.borrow().is_empty());
    }

    #[test]
    fn probe_retries_until_the_page_answers() {
        let tries = Cell::new(0);
        let result = probe(
            || {
                tries.set(tries.get() + 1);
                match tries.get() {
                    1 => Err("not connected".into()),
                    2 => Ok(None),
                    _ => Ok(Some(true)),
                }
            },
            5,
            Duration::ZERO,
            || false,
        );
        assert_eq!(result, Ok(true));
        assert_eq!(tries.get(), 3);
    }

    #[test]
    fn probe_gives_up_after_its_attempts_with_the_last_error() {
        let tries = Cell::new(0);
        let result = probe(
            || {
                tries.set(tries.get() + 1);
                Err("not connected".into())
            },
            3,
            Duration::ZERO,
            || false,
        );
        assert_eq!(result, Err("not connected".into()));
        assert_eq!(tries.get(), 3);
        assert!(probe(|| Ok(None), 2, Duration::ZERO, || false).is_err());
    }

    #[test]
    fn an_obsolete_probe_stops_before_its_next_step() {
        let tries = Cell::new(0);
        let result = probe(
            || {
                tries.set(tries.get() + 1);
                Ok(None)
            },
            20,
            Duration::ZERO,
            || tries.get() >= 2,
        );
        assert_eq!(result, Err(SUPERSEDED.into()));
        assert_eq!(tries.get(), 2);
    }

    #[test]
    fn requests_during_a_probe_join_one_worker_and_restart_it() {
        let probes = Probes::default();
        let (first, start) = probes.enlist("v").unwrap();
        assert!(start);
        let runs = Cell::new(0);
        let second = RefCell::new(None);
        probes.work("v", PROBE_DEADLINE, |obsolete| {
            runs.set(runs.get() + 1);
            if runs.get() == 1 {
                let (receiver, start) = probes.enlist("v").unwrap();
                *second.borrow_mut() = Some(receiver);
                assert!(!start);
                assert!(obsolete());
                return Err("stale".into());
            }
            assert!(!obsolete());
            Ok(true)
        });
        assert_eq!(runs.get(), 2);
        assert_eq!(first.blocking_recv().unwrap(), Err(SUPERSEDED.into()));
        let second = second.into_inner().unwrap();
        assert_eq!(second.blocking_recv().unwrap(), Ok(true));
        assert!(probes.enlist("v").unwrap().1, "the worker has finished");
    }

    #[test]
    fn a_repeatedly_reloading_page_never_adds_workers() {
        let probes = Probes::default();
        assert!(probes.enlist("v").unwrap().1);
        for _ in 0..50 {
            assert!(!probes.enlist("v").unwrap().1);
        }
    }

    #[test]
    fn a_reloading_page_settles_each_superseded_request_and_keeps_one_waiter() {
        let probes = Probes::default();
        let mut superseded = Vec::new();
        for _ in 0..200 {
            let (receiver, _) = probes.enlist("v").unwrap();
            superseded.push(receiver);
            assert_eq!(probes.waiting("v"), 1);
        }
        let newest = superseded.pop().unwrap();
        for mut receiver in superseded {
            assert_eq!(receiver.try_recv().unwrap(), Err(SUPERSEDED.into()));
        }
        drop(newest);
    }

    #[test]
    fn restarts_end_at_the_overall_deadline() {
        let probes = Probes::default();
        let (mut last, _) = probes.enlist("v").unwrap();
        let runs = Cell::new(0);
        let newest = RefCell::new(None);
        probes.work("v", Duration::from_millis(300), |_| {
            runs.set(runs.get() + 1);
            std::thread::sleep(Duration::from_millis(20));
            let (receiver, _) = probes.enlist("v").unwrap();
            *newest.borrow_mut() = Some(receiver);
            Err("stale".into())
        });
        // Each run takes at least 20 ms, so the deadline admits at most 16.
        assert!(runs.get() >= 2 && runs.get() <= 16, "{}", runs.get());
        assert!(last.try_recv().is_ok());
        let mut newest = newest.into_inner().unwrap();
        assert_eq!(newest.try_recv().unwrap(), Err(NOT_READY.into()));
        assert_eq!(probes.waiting("v"), 0);
        assert!(probes.enlist("v").unwrap().1, "the worker has finished");
    }

    #[test]
    fn closing_a_viewer_during_a_probe_leaves_no_slot_behind() {
        let probes = Probes::default();
        for cycle in 0..100 {
            let label = format!("viewer-{cycle}");
            let (waiting, start) = probes.enlist(&label).unwrap();
            assert!(start);
            probes.work(&label, PROBE_DEADLINE, |obsolete| {
                probes.forget(&label);
                assert!(obsolete());
                Err("stale".into())
            });
            assert!(waiting.blocking_recv().is_err());
        }
        assert!(probes.slots.lock().unwrap().is_empty());
    }

    #[test]
    fn cancelling_drops_waiters_and_ends_the_probe() {
        let probes = Probes::default();
        let (waiting, _) = probes.enlist("v").unwrap();
        probes.work("v", PROBE_DEADLINE, |obsolete| {
            probes.cancel("v");
            assert!(obsolete());
            Err("stale".into())
        });
        assert!(waiting.blocking_recv().is_err());
        assert!(probes.enlist("v").unwrap().1);
    }

    #[test]
    fn probes_of_different_viewers_are_independent() {
        let probes = Probes::default();
        assert!(probes.enlist("a").unwrap().1);
        assert!(probes.enlist("b").unwrap().1);
    }

    #[test]
    fn audio_state_sets_mute_then_stream() {
        let fake = Fake::default();
        apply_audio(&fake, true, false).unwrap();
        apply_audio(&fake, false, true).unwrap();
        assert_eq!(
            *fake.calls.borrow(),
            ["mute true", "active false", "mute false", "active true"]
        );
    }

    #[test]
    fn an_older_sound_revision_never_overrides_a_newer_one() {
        let fake = Fake::default();
        let latest = Mutex::new(0);
        apply_audio_ordered(&latest, 2, &fake, true, false).unwrap();
        apply_audio_ordered(&latest, 1, &fake, false, true).unwrap();
        apply_audio_ordered(&latest, 2, &fake, true, false).unwrap();
        assert_eq!(
            *fake.calls.borrow(),
            ["mute true", "active false", "mute true", "active false"]
        );
    }

    #[test]
    fn concurrent_sound_updates_end_on_the_newest_state() {
        struct Recording(Mutex<Vec<String>>);
        impl Media for Recording {
            fn capabilities(&self, _: Duration) -> Result<Capabilities, String> {
                unreachable!()
            }
            fn set_audio_muted(&self, muted: bool) -> Result<(), String> {
                std::thread::sleep(Duration::from_millis(5));
                self.0.lock().unwrap().push(format!("mute {muted}"));
                Ok(())
            }
            fn set_audio_active(&self, active: bool) -> Result<(), String> {
                self.0.lock().unwrap().push(format!("active {active}"));
                Ok(())
            }
            fn reset_resolution(&self, _: u32, _: u32) -> Result<(), String> {
                unreachable!()
            }
        }
        let media = Recording(Mutex::new(Vec::new()));
        let latest = Mutex::new(0);
        std::thread::scope(|scope| {
            for revision in 1..=8u64 {
                let (media, latest) = (&media, &latest);
                scope.spawn(move || {
                    let on = revision == 8;
                    apply_audio_ordered(latest, revision, media, !on, on).unwrap();
                });
            }
        });
        assert_eq!(
            media.0.lock().unwrap().last().map(String::as_str),
            Some("active true")
        );
    }

    #[test]
    fn reset_requests_the_default_size() {
        let fake = Fake::default();
        reset_screen(&fake).unwrap();
        assert_eq!(*fake.calls.borrow(), ["reset 1440x900"]);
    }
}
