//! Stand-in for `engine` on hosts without Virtualization.framework support for
//! macOS guests. Every operation reports that the host is unsupported.
// The shared workflow code names variants that this host never produces.
#![allow(dead_code)]
use super::store::{HostLimits, Layout, Record};
use std::path::Path;
use tauri::AppHandle;

const UNSUPPORTED: &str = "macOS computers need a Mac with Apple silicon.";

pub(super) struct LatestImage {
    pub url: String,
    pub version: String,
    pub build: String,
}

pub(super) enum InstallError {
    Cancelled,
    Failed(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MachineState {
    Running,
    Stopped,
    Failed,
}

pub(super) fn unsupported_reason() -> Option<String> {
    Some(UNSUPPORTED.into())
}

pub(super) fn host_limits() -> HostLimits {
    HostLimits {
        cpus: 2,
        memory_gib: 8,
    }
}

pub(super) fn random_mac() -> String {
    String::new()
}

pub(super) fn fetch_latest() -> Result<LatestImage, String> {
    Err(UNSUPPORTED.into())
}

pub(super) fn install(
    _app: &AppHandle,
    _record: &mut Record,
    _layout: &Layout,
    _image: &Path,
    _cancelled: &dyn Fn() -> bool,
    _progress: &mut dyn FnMut(f64),
) -> Result<(), InstallError> {
    Err(InstallError::Failed(UNSUPPORTED.into()))
}

pub(super) fn start(_app: &AppHandle, _record: &Record, _layout: &Layout) -> Result<(), String> {
    Err(UNSUPPORTED.into())
}

pub(super) fn request_stop(_app: &AppHandle, _id: &str) -> Result<(), String> {
    Err(UNSUPPORTED.into())
}

pub(super) fn force_stop(_app: &AppHandle, _id: &str) -> Result<(), String> {
    Err(UNSUPPORTED.into())
}

pub(super) fn machine_states(_app: &AppHandle) -> Result<Vec<(String, MachineState)>, String> {
    Ok(Vec::new())
}

pub(super) fn attach_display(
    _app: &AppHandle,
    _id: &str,
    _window: &tauri::WebviewWindow,
) -> Result<(), String> {
    Err(UNSUPPORTED.into())
}

pub(super) fn detach_display(_app: &AppHandle, _id: &str) {}

pub(super) fn release_slot(_id: &str) {}

pub(super) fn defer(work: impl FnOnce() + Send + 'static) {
    work();
}
