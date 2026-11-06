//! `guestClient.enable`, `.revoke` and `.status`: mint, remove and read the token a computer's
//! own agent uses (`docs/SiloUI-IN-GUEST-CLIENT.md`). The token is returned once; the engine
//! keeps only its hash, and the caller installs it in the guest.

use serde_json::Value;
use silo_api::methods::{
    guest_client::{
        GuestClientComputerParams, GuestClientEnableParams, GuestClientEnableResult,
        GuestClientStatus, GUEST_SPOOL_DIR, GUEST_TOKEN_PATH,
    },
    MethodName,
};
use silo_engine::{
    context::EngineContext,
    guest_client::{spool_dir, Spool, TokenStore},
};

use crate::dispatcher::{DispatchError, HandlerOutput};
#[cfg(not(test))]
use crate::guest_bridge::EngineRunner;

use super::{ownership::owns_prefix, HandlerContext};

pub(super) fn owns(method: MethodName) -> bool {
    owns_prefix(method, &["guestClient."])
}

pub(super) async fn handle(
    method: MethodName,
    params: &Value,
    context: &HandlerContext,
) -> Option<Result<HandlerOutput, DispatchError>> {
    if !owns(method) {
        return None;
    }
    Some(match method {
        MethodName::GuestClientEnable => enable(params, context).await,
        MethodName::GuestClientRevoke => revoke(params, context).await,
        _ => status(params, context).await,
    })
}

fn output(value: impl serde::Serialize) -> Result<HandlerOutput, DispatchError> {
    serde_json::to_value(value)
        .map(|value| HandlerOutput {
            value,
            event_barrier: None,
        })
        .map_err(|error| DispatchError::internal(error.to_string()))
}

fn status_of(store: &TokenStore, computer_id: &str) -> Result<GuestClientStatus, DispatchError> {
    let record = store.status(computer_id).map_err(DispatchError::internal)?;
    Ok(GuestClientStatus {
        computer_id: computer_id.to_owned(),
        enabled: record.is_some(),
        minted_at: record.as_ref().map(|record| record.minted_at.clone()),
        policy: record.map(|record| record.policy),
    })
}

/// The computers as the engine lists them; the handler tests supply their own.
async fn snapshot_for(
    context: &HandlerContext,
) -> Result<crate::guest_bridge::Snapshot, DispatchError> {
    #[cfg(test)]
    {
        let _ = context;
        Ok(tests::snapshot())
    }
    #[cfg(not(test))]
    {
        EngineRunner::new(context.engine.clone()).snapshot().await
    }
}

async fn enable(params: &Value, context: &HandlerContext) -> Result<HandlerOutput, DispatchError> {
    let params: GuestClientEnableParams = context.decode_params(params)?;
    let snapshot = snapshot_for(context).await?;
    let name = snapshot
        .names
        .get(&params.computer_id)
        .filter(|_| snapshot.linux.contains(&params.computer_id))
        .cloned()
        .ok_or_else(|| {
            DispatchError::internal(
                "The in-guest client is available for Linux computers only; that computer was not found.",
            )
        })?;
    let paths =
        silo_engine::runtime::runtime_paths(context.silod_context().runtime_host().as_ref())
            .map_err(DispatchError::internal)?;
    let app_data = context.silod_context().paths().app_data.clone();
    let result = tokio::task::spawn_blocking(move || {
        Spool::new(spool_dir(&paths.home, &name)).ensure()?;
        let minted = TokenStore::new(&app_data).mint(&params.computer_id, params.policy)?;
        Ok::<_, String>(GuestClientEnableResult {
            computer_id: params.computer_id,
            token: minted.token,
            token_path: GUEST_TOKEN_PATH.into(),
            spool_dir: GUEST_SPOOL_DIR.into(),
            minted_at: minted.record.minted_at,
        })
    })
    .await
    .map_err(|_| DispatchError::internal("The token could not be minted."))?
    .map_err(DispatchError::internal)?;
    output(result)
}

async fn revoke(params: &Value, context: &HandlerContext) -> Result<HandlerOutput, DispatchError> {
    let params: GuestClientComputerParams = context.decode_params(params)?;
    let store = TokenStore::new(&context.silod_context().paths().app_data);
    store
        .revoke(&[params.computer_id.as_str()])
        .map_err(DispatchError::internal)?;
    output(status_of(&store, &params.computer_id)?)
}

async fn status(params: &Value, context: &HandlerContext) -> Result<HandlerOutput, DispatchError> {
    let params: GuestClientComputerParams = context.decode_params(params)?;
    let store = TokenStore::new(&context.silod_context().paths().app_data);
    output(status_of(&store, &params.computer_id)?)
}

/// Removes the tokens of computers whose standing changed: a promote or undo swaps which
/// computer is main, so the agent that held a token must be given a new one on purpose.
pub(super) fn revoke_for(context: &HandlerContext, computers: &[&str]) {
    let store = TokenStore::new(&context.silod_context().paths().app_data);
    if let Err(error) = store.revoke(computers) {
        eprintln!("silod: could not revoke guest client tokens: {error}");
    }
}

#[cfg(test)]
mod tests;
