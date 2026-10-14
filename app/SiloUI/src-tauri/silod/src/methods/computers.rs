use serde_json::Value;
use silo_api::methods::MethodName;

use crate::dispatcher::{method_name, DispatchError, HandlerOutput};

use super::{ownership::owns_prefix, HandlerContext};

pub(super) fn owns(method: MethodName) -> bool {
    owns_prefix(
        method,
        &["computers.", "activity.", "logs.", "storage.", "macos."],
    )
}

pub async fn handle(
    method: MethodName,
    _params: &Value,
    _context: &HandlerContext,
) -> Option<Result<HandlerOutput, DispatchError>> {
    owns(method).then(|| Err(DispatchError::unsupported(method_name(method))))
}
