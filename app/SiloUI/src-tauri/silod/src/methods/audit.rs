//! `audit.list`: reads the audit log (`silo_engine::audit`). The owner reads every row; a
//! computer's agent reads the rows about itself and the computers it made.

use serde_json::Value;
use silo_api::methods::{audit::AuditListParams, MethodName};
use silo_engine::{
    audit::{AuditLog, Scope},
    context::EngineContext,
    policy::Caller,
};

use super::{ownership::owns_prefix, HandlerContext};
use crate::dispatcher::{DispatchError, HandlerOutput};

pub(super) fn owns(method: MethodName) -> bool {
    owns_prefix(method, &["audit."])
}

pub(super) async fn handle(
    method: MethodName,
    params: &Value,
    context: &HandlerContext,
) -> Option<Result<HandlerOutput, DispatchError>> {
    owns(method).then_some(())?;
    Some(list(params, context).await)
}

async fn list(params: &Value, context: &HandlerContext) -> Result<HandlerOutput, DispatchError> {
    let params: AuditListParams = context.decode_params(params)?;
    // A caller other than the owner sees only the computers the policy let it see, and nothing
    // when there are none.
    let visible = match context.caller() {
        Caller::Owner { .. } => None,
        _ => Some(context.visible().cloned().unwrap_or_default()),
    };
    let log = AuditLog::shared(&context.silod_context().paths().app_data);
    let page = tokio::task::spawn_blocking(move || {
        let scope = match &visible {
            None => Scope::All,
            Some(visible) => Scope::Computers(visible),
        };
        log.list(&params, scope)
    })
    .await
    .map_err(|_| DispatchError::internal("The audit log could not be read."))?
    .map_err(|error| DispatchError::internal(format!("The audit log could not be read: {error}")))?;
    serde_json::to_value(page)
        .map(|value| HandlerOutput {
            value,
            event_barrier: None,
        })
        .map_err(|error| DispatchError::internal(error.to_string()))
}
