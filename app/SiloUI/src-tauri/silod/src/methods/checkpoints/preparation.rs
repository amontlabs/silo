//! `preparation.*`: the Linux guest image and LCU preparation, and the ChatGPT app copy a
//! device keeps for computer use. Retry returns the status at once; progress follows as
//! `preparation.changed` and `preparation.chatgpt.changed`.

use std::sync::Arc;

use serde_json::Value;
use silo_api::methods::{
    common::EmptyParams,
    preparation::{ChatGptPreparationStatus, ChatGptPreparationStatusParams, PreparationStatus},
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
    let _: EmptyParams = context.decode_params(params)?;
    output(to_api::<PreparationStatus>(preparation::snapshot())?)
}

pub(super) async fn retry(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    let _: EmptyParams = context.decode_params(params)?;
    output(to_api::<PreparationStatus>(preparation::retry())?)
}

/// The ChatGPT app folder of this channel: `<app data>/chatgpt`.
fn storage_root(context: &HandlerContext) -> std::path::PathBuf {
    context.engine_context().paths().app_data.join("chatgpt")
}

/// Every status the worker reports reaches every subscribed client.
pub(super) fn status_publisher(context: Arc<dyn EngineContext>) -> chatgpt_app::StatusPublisher {
    Arc::new(move |status| {
        if let Ok(payload) = chatgpt_app::to_value(status) {
            context.publish(
                Event::new("preparation.chatgpt.changed", payload),
                Audience::All,
            );
        }
    })
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
    let idle_root = root.clone();
    // Once the app is ready, versions no computer can still be using are removed. A running
    // computer keeps what it booted from, so the pass waits until none runs.
    let on_ready: chatgpt_app::ReadyHook = Arc::new(move || {
        let host = host.clone();
        chatgpt_app::collect_unused(
            idle_root.clone(),
            Arc::new(move || {
                silo_engine::runtime::update_recovery::running_names(host.as_ref())
                    .is_ok_and(|names| names.is_empty())
            }),
        );
    });
    let status = blocking(move || {
        chatgpt_app::retry_now(&root, publisher, on_ready)
            .and_then(chatgpt_app::to_value)
            .map_err(message_error)
    })
    .await?;
    output(to_api::<ChatGptPreparationStatus>(status)?)
}
