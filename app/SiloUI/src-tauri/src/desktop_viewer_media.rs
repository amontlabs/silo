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
    time::Duration,
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
/// yet) like an unopened transport, for at most `attempts` tries.
pub(crate) fn probe(
    mut step: impl FnMut() -> Result<Option<bool>, String>,
    attempts: u32,
    pause: Duration,
) -> Result<bool, String> {
    let mut last = "The desktop display is not ready.".to_string();
    for attempt in 0..attempts {
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
    tauri::async_runtime::spawn_blocking(move || {
        probe(
            || with_bridge(&app, &label, |bridge| probe_step(bridge)),
            PROBE_ATTEMPTS,
            PROBE_PAUSE,
        )
        .map(|sound| SoundSupport { sound })
    })
    .await
    .map_err(|_| "Desktop sound check failed.")?
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
        );
        assert_eq!(result, Err("not connected".into()));
        assert_eq!(tries.get(), 3);
        assert!(probe(|| Ok(None), 2, Duration::ZERO).is_err());
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
