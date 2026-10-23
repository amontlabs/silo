//! Sound, audio and screen controls of a desktop viewer: each forwards to the engine, which
//! owns the page probe, the revision ordering and the guest bridge.
use serde::Serialize;
use serde_json::Value;
use silo_api::methods::{
    display::{ViewerAudioSetParams, ViewerConnectionParams},
    requests,
};
use tauri::Window;

use crate::{
    commands::desktop_viewer::{connection_of, require_computer, require_connection},
    engine_client::{Engine, EngineClient, EngineError},
};

/// What the UI parses from the sound check.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct SoundSupport {
    pub sound: bool,
}

fn connection_params(connection: &str) -> ViewerConnectionParams {
    ViewerConnectionParams {
        connection_id: connection.to_owned(),
    }
}

/// Whether this device can play the computer's sound, waiting for the page's connection.
pub(crate) async fn sound_check(
    engine: &EngineClient,
    connection: &str,
) -> Result<SoundSupport, EngineError> {
    let answer = engine
        .call(requests::display_viewer_sound_check(connection_params(
            connection,
        )))
        .await?;
    answer
        .get("sound")
        .and_then(Value::as_bool)
        .map(|sound| SoundSupport { sound })
        .ok_or_else(|| EngineError::internal("The engine returned an unreadable sound check."))
}

pub(crate) async fn sound_cancel(
    engine: &EngineClient,
    connection: &str,
) -> Result<(), EngineError> {
    engine
        .call(requests::display_viewer_sound_cancel(connection_params(
            connection,
        )))
        .await
        .map(|_| ())
}

pub(crate) async fn set_audio(
    engine: &EngineClient,
    connection: &str,
    muted: bool,
    active: bool,
    revision: u64,
) -> Result<(), EngineError> {
    engine
        .call(requests::display_viewer_audio_set(ViewerAudioSetParams {
            connection_id: connection.to_owned(),
            muted,
            active,
            revision,
        }))
        .await
        .map(|_| ())
}

pub(crate) async fn reset_screen(
    engine: &EngineClient,
    connection: &str,
) -> Result<(), EngineError> {
    engine
        .call(requests::display_viewer_screen_reset(connection_params(
            connection,
        )))
        .await
        .map(|_| ())
}

/// Reports whether this device can play the computer's sound. An unsupported engine also
/// stops the audio stream.
#[tauri::command]
pub(crate) async fn desktop_viewer_sound_support(
    engine: Engine<'_>,
    window: Window,
    computer: String,
) -> Result<SoundSupport, String> {
    let connection = require_connection(&window, &computer)?;
    sound_check(&engine, &connection)
        .await
        .map_err(String::from)
}

/// Abandons the viewer's sound check, for a page or connection that is gone.
#[tauri::command]
pub(crate) async fn desktop_viewer_sound_cancel(
    engine: Engine<'_>,
    window: Window,
    computer: String,
) -> Result<(), String> {
    if !crate::commands::desktop_viewer::is_viewer_label(window.label()) {
        return Err("Only a desktop viewer can use this command.".into());
    }
    require_computer(&window, &computer)?;
    // Without a connection there is no check to abandon.
    let Some(connection) = connection_of(window.label()) else {
        return Ok(());
    };
    sound_cancel(&engine, &connection)
        .await
        .map_err(String::from)
}

#[tauri::command]
pub(crate) async fn desktop_viewer_set_audio(
    engine: Engine<'_>,
    window: Window,
    computer: String,
    muted: bool,
    active: bool,
    revision: u64,
) -> Result<(), String> {
    let connection = require_connection(&window, &computer)?;
    set_audio(&engine, &connection, muted, active, revision)
        .await
        .map_err(String::from)
}

#[tauri::command]
pub(crate) async fn desktop_viewer_reset_screen(
    engine: Engine<'_>,
    window: Window,
    computer: String,
) -> Result<(), String> {
    let connection = require_connection(&window, &computer)?;
    reset_screen(&engine, &connection)
        .await
        .map_err(String::from)
}

#[cfg(test)]
mod forwarding_tests {
    use super::*;
    use crate::engine_client::testing::{connected_client, FakeEngine};
    use serde_json::json;
    use silo_api::methods::MethodName;

    #[tokio::test]
    async fn the_sound_check_asks_the_engine_and_translates_its_answer() {
        let fake = FakeEngine::start(|method, _| match method {
            MethodName::DisplayViewerSoundCheck => Ok(json!({"sound": true})),
            _ => Ok(Value::Null),
        });
        let (client, _) = connected_client(&fake).await;
        let support = sound_check(&client, "c1").await.unwrap();
        assert_eq!(
            serde_json::to_value(support).unwrap(),
            json!({"sound": true})
        );
        assert_eq!(
            fake.calls(MethodName::DisplayViewerSoundCheck),
            [json!({"connectionId": "c1"})]
        );
    }

    #[tokio::test]
    async fn an_unreadable_sound_answer_is_a_failure_not_silence() {
        let fake = FakeEngine::start(|_, _| Ok(json!({})));
        let (client, _) = connected_client(&fake).await;
        assert!(sound_check(&client, "c1").await.is_err());
    }

    #[tokio::test]
    async fn a_superseded_check_reports_the_engines_cancellation() {
        let fake = FakeEngine::start(|method, _| match method {
            MethodName::EngineState | MethodName::EventsSubscribe => Ok(Value::Null),
            _ => Err(("cancelled", "The desktop sound check was cancelled.".into())),
        });
        let (client, _) = connected_client(&fake).await;
        let error = sound_check(&client, "c1").await.unwrap_err();
        assert!(error.is("cancelled"));
        assert_eq!(error.message, "The desktop sound check was cancelled.");
    }

    #[tokio::test]
    async fn cancel_audio_and_reset_send_the_connection_and_their_fields() {
        let fake = FakeEngine::start(|_, _| Ok(Value::Null));
        let (client, _) = connected_client(&fake).await;
        sound_cancel(&client, "c1").await.unwrap();
        set_audio(&client, "c1", true, false, 7).await.unwrap();
        reset_screen(&client, "c1").await.unwrap();
        assert_eq!(
            fake.calls(MethodName::DisplayViewerSoundCancel),
            [json!({"connectionId": "c1"})]
        );
        assert_eq!(
            fake.calls(MethodName::DisplayViewerAudioSet),
            [json!({"connectionId": "c1", "muted": true, "active": false, "revision": 7})]
        );
        assert_eq!(
            fake.calls(MethodName::DisplayViewerScreenReset),
            [json!({"connectionId": "c1"})]
        );
    }

    #[tokio::test]
    async fn engine_errors_reach_the_ui_as_their_message() {
        let fake = FakeEngine::start(|method, _| match method {
            MethodName::EngineState | MethodName::EventsSubscribe => Ok(Value::Null),
            _ => Err(("not_found", "Unknown desktop viewer.".into())),
        });
        let (client, _) = connected_client(&fake).await;
        let error = reset_screen(&client, "gone").await.unwrap_err();
        assert!(error.is("not_found"));
        assert_eq!(String::from(error), "Unknown desktop viewer.");
    }
}
