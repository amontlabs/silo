//! `preparation.*`: the Linux guest image and LCU preparation, and the ChatGPT app copy a
//! device keeps for computer use. Retry returns the status at once; progress follows as
//! `preparation.changed` and `preparation.chatgpt.changed`.

use std::sync::Arc;

use serde_json::Value;
use silo_api::methods::preparation::{
    ChatGptPreparationStatus, ChatGptPreparationStatusParams, PreparationRetryParams,
    PreparationStatus, PreparationStatusParams,
};
use silo_engine::{
    chatgpt_app,
    context::{Audience, EngineContext, Event},
    preparation,
};

use crate::dispatcher::{DispatchError, HandlerOutput};

use super::{blocking, message_error, output, require_local_device, HandlerContext};

fn to_api<T: serde::de::DeserializeOwned>(
    value: impl serde::Serialize,
) -> Result<T, DispatchError> {
    serde_json::to_value(value)
        .and_then(serde_json::from_value)
        .map_err(|error| DispatchError::internal(error.to_string()))
}

pub(super) async fn status(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: PreparationStatusParams = context.decode_params(params)?;
    require_local_device(context, params.device_id.as_deref())?;
    output(to_api::<PreparationStatus>(preparation::snapshot())?)
}

pub(super) async fn retry(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: PreparationRetryParams = context.decode_params(params)?;
    require_local_device(context, params.device_id.as_deref())?;
    output(to_api::<PreparationStatus>(preparation::retry())?)
}

/// The ChatGPT app folder of this channel: `<app data>/chatgpt`.
fn storage_root(context: &HandlerContext) -> std::path::PathBuf {
    context.engine_context().paths().app_data.join("chatgpt")
}

/// Every status the worker reports reaches every subscribed client.
pub(crate) fn status_publisher(context: Arc<dyn EngineContext>) -> chatgpt_app::StatusPublisher {
    Arc::new(move |status| {
        if let Ok(payload) = chatgpt_app::to_value(status) {
            context.publish(
                Event::new("preparation.chatgpt.changed", payload),
                Audience::All,
            );
        }
    })
}

/// What runs when the ChatGPT app becomes ready. Running built-in computers set computer use
/// up now instead of at their next boot. Versions no computer can still be using are removed;
/// a running computer keeps what it booted from, so that pass waits until none runs.
pub(crate) fn ready_hook(
    host: Arc<dyn silo_engine::runtime::RuntimeHost>,
    root: std::path::PathBuf,
) -> chatgpt_app::ReadyHook {
    Arc::new(move || {
        let setup_host = host.clone();
        std::thread::spawn(move || set_up_running_computers(setup_host.as_ref()));
        let idle_host = host.clone();
        chatgpt_app::collect_unused(
            root.clone(),
            Arc::new(move || {
                silo_engine::runtime::update_recovery::running_names(idle_host.as_ref())
                    .is_ok_and(|names| names.is_empty())
            }),
        );
    })
}

fn set_up_running_computers(host: &dyn silo_engine::runtime::RuntimeHost) {
    let Ok(paths) = silo_engine::runtime::runtime_paths(host) else {
        return;
    };
    let Ok(running) = silo_engine::runtime::update_recovery::running_names(host) else {
        return;
    };
    for name in running {
        let _ = silo_engine::computer_use::after_boot(
            Arc::new(silo_engine::runtime::ProcessRunner),
            &paths,
            &name,
        );
    }
    silo_engine::computer_use::publish_application_state_changed();
}

pub(super) async fn chatgpt_status(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: ChatGptPreparationStatusParams = context.decode_params(params)?;
    require_local_device(context, params.device_id.as_deref())?;
    let root = storage_root(context);
    let status = blocking(move || {
        chatgpt_app::local_status(&root)
            .and_then(chatgpt_app::to_value)
            .map_err(message_error)
    })
    .await?;
    output(to_api::<ChatGptPreparationStatus>(status)?)
}

pub(super) async fn chatgpt_retry(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let params: ChatGptPreparationStatusParams = context.decode_params(params)?;
    require_local_device(context, params.device_id.as_deref())?;
    let root = storage_root(context);
    let engine = super::engine_context(context);
    let host = context.silod_context().runtime_host();
    let publisher = status_publisher(engine);
    let on_ready = ready_hook(host, root.clone());
    let status = blocking(move || {
        chatgpt_app::retry_now(&root, publisher, on_ready)
            .and_then(chatgpt_app::to_value)
            .map_err(message_error)
    })
    .await?;
    output(to_api::<ChatGptPreparationStatus>(status)?)
}
