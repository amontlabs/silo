//! Stand-in for `engine` on hosts without Virtualization.framework support for
//! macOS guests. Every operation reports that the host is unsupported.
// The shared workflow code names variants that this host never produces.
#![allow(dead_code)]
use super::input::{KeyEvent, PointerKind, TextLine};
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

pub(super) fn new_machine_identifier() -> Vec<u8> {
    Vec::new()
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

pub(super) fn start_in_recovery(
    _app: &AppHandle,
    _record: &Record,
    _layout: &Layout,
) -> Result<(), String> {
    Err(UNSUPPORTED.into())
}

pub(super) enum StateStartError {
    Rejected(String),
    Failed(String),
}

pub(super) fn start_from_state(
    _app: &AppHandle,
    _record: &Record,
    _layout: &Layout,
    _state: &Path,
) -> Result<(), StateStartError> {
    Err(StateStartError::Failed(UNSUPPORTED.into()))
}

pub(super) fn memory_support(_app: &AppHandle, _id: &str) -> Result<(), String> {
    Err(UNSUPPORTED.into())
}

pub(super) fn save_running(
    _app: &AppHandle,
    _id: &str,
    _state: &Path,
    _copy: &mut dyn FnMut() -> Result<(), String>,
) -> Result<(), String> {
    Err(UNSUPPORTED.into())
}

pub(super) fn focus_display(_app: &AppHandle, _id: &str) -> Result<isize, String> {
    Err(UNSUPPORTED.into())
}

pub(super) fn send_keys(_app: &AppHandle, _id: &str, _events: Vec<KeyEvent>) -> Result<(), String> {
    Err(UNSUPPORTED.into())
}

pub(super) fn send_pointer(
    _app: &AppHandle,
    _id: &str,
    _kind: PointerKind,
    _x: f64,
    _y: f64,
) -> Result<(), String> {
    Err(UNSUPPORTED.into())
}

pub(super) fn read_screen(_app: &AppHandle, _id: &str) -> Result<Vec<TextLine>, String> {
    Err(UNSUPPORTED.into())
}

pub(super) fn clear_last_screen(_id: &str) {}

pub(super) fn save_last_screen(
    _id: &str,
    _png: Option<&std::path::Path>,
) -> Option<(Vec<TextLine>, bool)> {
    None
}

pub(super) fn lock_input(_app: &AppHandle, _id: &str, _subtitle: &str) -> Result<(), String> {
    Err(UNSUPPORTED.into())
}

pub(super) fn unlock_input(_app: &AppHandle, _id: &str) {}

pub(super) fn wait_until_stopped(
    _app: &AppHandle,
    _id: &str,
    _timeout: std::time::Duration,
) -> bool {
    true
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
    _toolbar: bool,
) -> Result<(), String> {
    Err(UNSUPPORTED.into())
}

pub(super) fn detach_display(_app: &AppHandle, _id: &str) {}

pub(super) fn set_display_subtitle(_app: &AppHandle, _id: &str, _text: &str) {}

pub(super) fn release_slot(_id: &str) {}

pub(super) fn defer(work: impl FnOnce() + Send + 'static) {
    work();
}
