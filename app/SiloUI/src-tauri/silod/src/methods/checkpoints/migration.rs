//! `migration.*`: the launch upgrade's state and the pre-upgrade backup it leaves behind.
//! Conversion triggers are retired (95 §3); these methods observe and administer.

use serde_json::Value;
use silo_api::methods::common::DeviceParams;
use silo_engine::{pre_upgrade_backup as backup, runtime_migration};

use crate::dispatcher::{DispatchError, HandlerOutput};

use super::{blocking, message_error, output, require_local_device, HandlerContext};

fn local(context: &HandlerContext, params: &Value) -> Result<(), DispatchError> {
    let params: DeviceParams = context.decode_params(params)?;
    require_local_device(context, params.device_id.as_deref())
}

pub(super) async fn state(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    local(context, params)?;
    let controller = context.silod_context().migration_controller();
    let state =
        blocking(move || runtime_migration::api_state(&controller).map_err(message_error)).await?;
    output(state)
}

fn app_data(context: &HandlerContext) -> std::path::PathBuf {
    context.engine_context().paths().app_data.clone()
}

pub(super) async fn backup_get(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    local(context, params)?;
    let app_data = app_data(context);
    let status = blocking(move || backup::api_status(&app_data).map_err(message_error)).await?;
    output(status)
}

pub(super) async fn backup_measure(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    local(context, params)?;
    let app_data = app_data(context);
    let bytes = blocking(move || backup::measure_backup(&app_data).map_err(message_error)).await?;
    output(bytes)
}

pub(super) async fn backup_delete(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    local(context, params)?;
    let engine = super::engine_context(context);
    blocking(move || {
        let paths = engine.paths();
        let removed =
            backup::delete_backup_for_user(&paths.app_data, &paths.home).map_err(message_error)?;
        if removed {
            backup::announce_changed(engine.as_ref());
        }
        Ok(())
    })
    .await?;
    output(())
}

pub(super) async fn backup_acknowledge_notice(
    context: &HandlerContext,
    params: &Value,
) -> Result<HandlerOutput, DispatchError> {
    local(context, params)?;
    let engine = super::engine_context(context);
    blocking(move || {
        backup::acknowledge_notice(&engine.paths().app_data).map_err(message_error)?;
        backup::announce_changed(engine.as_ref());
        Ok(())
    })
    .await?;
    output(())
}
