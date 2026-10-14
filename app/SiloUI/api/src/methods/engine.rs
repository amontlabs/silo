//! Engine lifecycle, negotiation, update handoff, and subscription methods.

use super::common::{DeviceParams, EmptyParams, JobStartResult};

pub type EngineDisconnectParams = EmptyParams;
pub type EngineDisconnectResult = ();
pub type EngineHandshakeParams = crate::transport::HandshakeParams;
pub type EngineHandshakeResult = crate::transport::HandshakeResult;
pub type EngineStateParams = EmptyParams;

#[derive(
    Clone, Copy, Debug, serde::Deserialize, PartialEq, Eq, serde::Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum ShutdownPhase {
    Idle,
    Requested,
    Approved,
    Finishing,
    Flushing,
}

#[derive(
    Clone, Copy, Debug, serde::Deserialize, PartialEq, Eq, serde::Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum ShutdownReason {
    User,
    SessionEnd,
    Update,
}

wire_struct! { EngineState { shutting_down: bool, phase: ShutdownPhase, generation: u64, session_ending: bool, update_pending: bool } }
wire_struct! { EngineShutdownParams { stop_computers: bool, reason: ShutdownReason, #[serde(default, skip_serializing_if = "Option::is_none")] deadline_ms: Option<u64> } }
pub type EngineShutdownResult = JobStartResult;
pub type EngineShutdownFinishedResult = ();
pub type EngineUpdatePrepareParams = EmptyParams;
pub type EngineUpdatePrepareResult = JobStartResult;
wire_struct! { EngineUpdatePrepareFinishedResult { resuming: Vec<ResumingComputer> } }
wire_struct! { ResumingComputer { id: String, name: String } }
pub type EngineUpdateAbortParams = EmptyParams;
pub type EngineUpdateAbortResult = ();
pub type EngineDeviceParams = DeviceParams;

wire_struct! { EventsSubscribeParams { events: Vec<String>, #[serde(default, skip_serializing_if = "Option::is_none")] device_ids: Option<Vec<String>> } }
wire_struct! { EventsSubscribeResult { subscription: String } }
wire_struct! { EventsUnsubscribeParams { subscription: String } }
pub type EventsUnsubscribeResult = ();
