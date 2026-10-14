//! Durable notice retention. Delivery is left to clients; the engine never posts OS notices.

use crate::context::{Audience, EngineContext, Event};
use silo_api::methods::notifications::{Notice, NotificationsClearedPayload};

const MAX_NOTICE_BYTES: usize = 64 * 1024;
const MAX_NOTICE_LIST_BYTES: usize = 3 * 1024 * 1024;

pub fn retain(
    context: &dyn EngineContext,
    notice: &Notice,
    audience: Audience,
) -> Result<(), String> {
    let bytes = serde_json::to_vec(notice).map_err(|error| error.to_string())?;
    if bytes.len() > MAX_NOTICE_BYTES {
        return Err("Notice exceeds the retained notice size limit.".to_owned());
    }
    context
        .job_registry()?
        .upsert_notice(notice)
        .map_err(|error| error.to_string())?;
    context.publish(
        Event::new(
            "notifications.notice",
            serde_json::to_value(notice).map_err(|error| error.to_string())?,
        ),
        audience,
    );
    Ok(())
}

pub fn list(context: &dyn EngineContext) -> Result<Vec<Notice>, String> {
    let notices = context
        .job_registry()?
        .list_notices()
        .map_err(|error| error.to_string())?;
    let mut size = 2;
    for notice in &notices {
        size += serde_json::to_vec(notice)
            .map_err(|error| error.to_string())?
            .len()
            + 1;
        if size > MAX_NOTICE_LIST_BYTES {
            return Err("Retained notices exceed the response size limit.".to_owned());
        }
    }
    Ok(notices)
}

pub fn clear_for_computer(
    context: &dyn EngineContext,
    computer_id: &str,
    audience: Audience,
) -> Result<(), String> {
    context
        .job_registry()?
        .clear_notices_for_computer(computer_id)
        .map_err(|error| error.to_string())?;
    let payload = NotificationsClearedPayload {
        computer_id: computer_id.to_owned(),
    };
    context.publish(
        Event::new(
            "notifications.cleared",
            serde_json::to_value(payload).map_err(|error| error.to_string())?,
        ),
        audience,
    );
    Ok(())
}
