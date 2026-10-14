//! AppKit sends Dock Quit, logout, restart, shutdown and Apple-event quits
//! through `terminate:`, which asks the delegate `applicationShouldTerminate:`.
//! tao 0.35 does not implement that method, so AppKit terminated Silo without
//! its Quit path (F-02). Until a tao release used by Tauri carries it, add the
//! method to tao's delegate class at runtime (design note option 2) and reply
//! `NSTerminateLater`, then answer once Silo's Quit path has finished or was
//! cancelled.
use std::sync::{
    atomic::{AtomicBool, Ordering},
    OnceLock,
};

use crate::system_shutdown::{quit_reason, ShutdownReason};
use objc2::{
    ffi, msg_send,
    rc::Retained,
    runtime::{AnyObject, Imp, Sel},
    sel,
};
use objc2_app_kit::NSApplication;
use objc2_foundation::{MainThreadMarker, NSAppleEventDescriptor, NSAppleEventManager};
use tauri::AppHandle;

static APP: OnceLock<AppHandle> = OnceLock::new();
/// AppKit is waiting for `replyToApplicationShouldTerminate:`.
static PENDING: AtomicBool = AtomicBool::new(false);

// NSApplicationTerminateReply
const TERMINATE_NOW: usize = 1;
const TERMINATE_LATER: usize = 2;
/// kAEQuitReason ('why?')
const QUIT_REASON: u32 = u32::from_be_bytes(*b"why?");

type ShouldTerminate = extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject) -> usize;

/// Call on the main thread after Tauri created the event loop (which installs
/// tao's delegate) and before it runs.
pub(super) fn install(app: &AppHandle) -> Result<(), String> {
    let _ = APP.set(app.clone());
    let mtm = MainThreadMarker::new()
        .ok_or("the terminate handler must be installed on the main thread")?;
    let application = NSApplication::sharedApplication(mtm);
    let delegate = application
        .delegate()
        .ok_or("AppKit has no application delegate")?;
    let object: &AnyObject = (*delegate).as_ref();
    let class = object.class();
    // SAFETY: the function matches `- (NSApplicationTerminateReply)
    // applicationShouldTerminate:(NSApplication *)sender` on 64-bit macOS,
    // whose encoding is "Q@:@". Adding a method to a registered class is allowed.
    let added = unsafe {
        let imp = std::mem::transmute::<ShouldTerminate, Imp>(should_terminate);
        ffi::class_addMethod(
            class as *const _ as *mut _,
            sel!(applicationShouldTerminate:),
            imp,
            c"Q@:@".as_ptr(),
        )
    };
    if !added.as_bool() {
        return Err("tao's delegate already implements applicationShouldTerminate:; the local handler is not needed".into());
    }
    // Assign the delegate again so AppKit re-reads which optional methods it implements.
    application.setDelegate(None);
    application.setDelegate(Some(&delegate));
    Ok(())
}

extern "C-unwind" fn should_terminate(_: &AnyObject, _: Sel, _: *mut AnyObject) -> usize {
    // Never unwind into AppKit.
    std::panic::catch_unwind(|| {
        let Some(app) = APP.get() else {
            return TERMINATE_NOW;
        };
        if !crate::commands::settings::accepts_terminate_request() {
            return TERMINATE_NOW;
        }
        begin_terminate(&PENDING, quit_reason(current_quit_reason()), |reason| {
            super::route(app, reason)
        });
        TERMINATE_LATER
    })
    .unwrap_or(TERMINATE_NOW)
}

fn current_quit_reason() -> Option<u32> {
    let event = NSAppleEventManager::sharedAppleEventManager().currentAppleEvent()?;
    // SAFETY: both selectors take an AEKeyword (FourCharCode, UInt32) and
    // return an autoreleased descriptor or nil.
    let reason: Option<Retained<NSAppleEventDescriptor>> =
        unsafe { msg_send![&*event, attributeDescriptorForKeyword: QUIT_REASON] };
    let reason =
        reason.or_else(|| unsafe { msg_send![&*event, paramDescriptorForKeyword: QUIT_REASON] })?;
    Some(match reason.enumCodeValue() {
        0 => reason.typeCodeValue(),
        code => code,
    })
}

fn begin_terminate(
    pending: &AtomicBool,
    reason: ShutdownReason,
    route: impl FnOnce(ShutdownReason),
) {
    if pending.swap(true, Ordering::SeqCst) && reason == ShutdownReason::UserQuit {
        return;
    }
    route(reason);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system_shutdown::ShutdownReason;

    #[test]
    fn session_end_escalates_an_already_pending_user_quit() {
        for reason in [ShutdownReason::Logout, ShutdownReason::Shutdown] {
            let pending = AtomicBool::new(false);
            let mut routed = Vec::new();
            begin_terminate(&pending, ShutdownReason::UserQuit, |reason| {
                routed.push(reason)
            });
            begin_terminate(&pending, reason, |reason| routed.push(reason));
            assert_eq!(routed, vec![ShutdownReason::UserQuit, reason]);
            assert!(pending.load(Ordering::SeqCst));
        }
    }

    #[test]
    fn repeated_user_quit_does_not_open_a_second_confirmation() {
        let pending = AtomicBool::new(false);
        let mut routed = Vec::new();
        for _ in 0..2 {
            begin_terminate(&pending, ShutdownReason::UserQuit, |reason| {
                routed.push(reason)
            });
        }
        assert_eq!(routed, vec![ShutdownReason::UserQuit]);
    }
}

/// Answer a pending `applicationShouldTerminate:`. Returns false when AppKit was
/// not waiting, so the caller exits or continues the ordinary way.
pub(super) fn reply(app: &AppHandle, should_terminate: bool) -> bool {
    if !PENDING.swap(false, Ordering::SeqCst) {
        return false;
    }
    let result = app.run_on_main_thread(move || {
        if let Some(mtm) = MainThreadMarker::new() {
            NSApplication::sharedApplication(mtm)
                .replyToApplicationShouldTerminate(should_terminate);
        }
    });
    if let Err(error) = result {
        eprintln!("Silo quit handling: AppKit could not be answered: {error}");
        return false;
    }
    true
}
