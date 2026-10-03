//! Sound and screen-size commands for a desktop viewer's shell window.
//!
//! The commands drive the guest page through `desktop_bridge::Bridge`; the
//! decisions live in functions over the small [`Media`] trait so tests can run
//! them without a webview.
use crate::desktop_bridge::{Bridge, Capabilities};
use crate::desktop_viewer::{is_viewer_label, require_computer, with_bridge};
use serde::Serialize;
use std::time::Duration;
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
) -> Result<(), String> {
    let label = require_viewer(&window, &computer)?;
    tauri::async_runtime::spawn_blocking(move || {
        with_bridge(&app, &label, |bridge| apply_audio(bridge, muted, active))
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
    fn reset_requests_the_default_size() {
        let fake = Fake::default();
        reset_screen(&fake).unwrap();
        assert_eq!(*fake.calls.borrow(), ["reset 1440x900"]);
    }
}
