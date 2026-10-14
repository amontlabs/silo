use tauri::{AppHandle, Window};

use crate::desktop_viewer_media::{self, SoundSupport};

fn require_viewer(window: &Window, computer: &str) -> Result<String, String> {
    if !crate::desktop_viewer::is_viewer_label(window.label()) {
        return Err("Only a desktop viewer can use this command.".into());
    }
    crate::desktop_viewer::require_computer(window, computer)?;
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
    let (answer, start) = desktop_viewer_media::probes().enlist(&label)?;
    if start {
        tokio::task::spawn_blocking(move || {
            desktop_viewer_media::probes().work(
                &label,
                desktop_viewer_media::PROBE_DEADLINE,
                |obsolete| {
                    desktop_viewer_media::probe(
                        || {
                            crate::desktop_viewer::with_bridge(&app, &label, |bridge| {
                                desktop_viewer_media::probe_step(bridge)
                            })
                        },
                        desktop_viewer_media::PROBE_ATTEMPTS,
                        desktop_viewer_media::PROBE_PAUSE,
                        obsolete,
                    )
                },
            )
        });
    }
    answer
        .await
        .map_err(|_| desktop_viewer_media::SUPERSEDED)?
        .map(|sound| SoundSupport { sound })
}

/// Abandons the viewer's sound check, for a page or connection that is gone.
#[tauri::command]
pub(crate) fn desktop_viewer_sound_cancel(window: Window, computer: String) -> Result<(), String> {
    let label = require_viewer(&window, &computer)?;
    desktop_viewer_media::probes().cancel(&label);
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
    let latest = desktop_viewer_media::sound_revision(&label)?;
    tokio::task::spawn_blocking(move || {
        crate::desktop_viewer::with_bridge(&app, &label, |bridge| {
            desktop_viewer_media::apply_audio_ordered(&latest, revision, bridge, muted, active)
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
    tokio::task::spawn_blocking(move || {
        crate::desktop_viewer::with_bridge(&app, &label, |bridge| {
            desktop_viewer_media::reset_screen(bridge)
        })
    })
    .await
    .map_err(|_| "Desktop screen reset failed.")?
}
