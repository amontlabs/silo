//! macOS display, setup, and template contracts.

use super::common::{ComputerTargetParams, DeviceParams, JobStartResult};

pub type MacosDisplayStreamParams = MacosDisplayConnectParams;
pub type MacosSetupRetryParams = ComputerTargetParams;
pub type MacosSetupRetryResult = JobStartResult;
pub type MacosTemplateDeleteParams = DeviceParams;
pub type MacosTemplateDeleteResult = ();

wire_struct! { MacosDisplayConnectParams { computer_id: String } }
wire_struct! { MacosDisplayConnectResult { username: String, password: String, width: u32, height: u32 } }
wire_struct! { MacosDisplayResizeParams { computer_id: String, width_px: u64, height_px: u64 } }
wire_struct! { MacosDisplayResizeResult { width_px: u32, height_px: u32 } }
wire_struct! { MacosDisplaySessionParams { device_id: String, computer_id: String } }
wire_struct! { MacosDisplaySessionResult { url: String, username: String, password: String, width: u32, height: u32 } }
