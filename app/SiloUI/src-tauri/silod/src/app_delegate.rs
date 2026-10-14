//! The AppKit application delegate of the engine process.
//!
//! silod has no windows of its own except the VM windows (95 A-21), and it runs as a
//! LaunchAgent. Without a delegate AppKit would answer a Dock "Quit" (shown while a VM
//! window makes the process regular), a logout or a shutdown by killing the process, which
//! would skip the engine's own stop of its computers. The delegate:
//!
//! * keeps the process alive when the last VM window closes (the engine is not a document
//!   app);
//! * hands every `terminate:` to the engine's session-end shutdown (no prompt, bounded; the
//!   same path SIGTERM takes), answers `NSTerminateLater`, and lets that shutdown end the
//!   run loop and the process;
//! * ignores a Dock click on the running engine (no window to reopen).

use std::sync::atomic::{AtomicBool, Ordering};

use objc2::{
    define_class, msg_send, rc::Retained, runtime::ProtocolObject, DefinedClass, MainThreadMarker,
    MainThreadOnly,
};
use objc2_app_kit::{NSApplication, NSApplicationDelegate, NSApplicationTerminateReply};
use objc2_foundation::{NSObject, NSObjectProtocol};

/// Starts the engine's graceful shutdown for an AppKit quit request.
pub type TerminateHandler = Box<dyn Fn() + Send + Sync>;

/// Remembers that a quit request already started the shutdown, so repeated requests (a second
/// Dock Quit, logout after a quit) attach to the one under way instead of starting another.
#[derive(Default)]
pub struct TerminateRequests {
    started: AtomicBool,
}

impl TerminateRequests {
    /// True for the first request only.
    pub fn begin(&self) -> bool {
        !self.started.swap(true, Ordering::SeqCst)
    }
}

struct DelegateState {
    requests: TerminateRequests,
    handler: TerminateHandler,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "SilodApplicationDelegate"]
    #[thread_kind = MainThreadOnly]
    #[ivars = DelegateState]
    struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl NSApplicationDelegate for AppDelegate {
        #[unsafe(method(applicationShouldTerminate:))]
        fn application_should_terminate(
            &self,
            _sender: &NSApplication,
        ) -> NSApplicationTerminateReply {
            let state = self.ivars();
            if state.requests.begin() {
                // Never unwind into AppKit.
                let _ =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (state.handler)()));
            }
            // The shutdown stops the run loop itself when it is done.
            NSApplicationTerminateReply::TerminateLater
        }

        #[unsafe(method(applicationShouldTerminateAfterLastWindowClosed:))]
        fn application_should_terminate_after_last_window_closed(
            &self,
            _sender: &NSApplication,
        ) -> bool {
            false
        }

        #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
        fn application_should_handle_reopen(
            &self,
            _sender: &NSApplication,
            _has_visible_windows: bool,
        ) -> bool {
            false
        }
    }
);

impl AppDelegate {
    fn new(mtm: MainThreadMarker, handler: TerminateHandler) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(DelegateState {
            requests: TerminateRequests::default(),
            handler,
        });
        unsafe { msg_send![super(this), init] }
    }
}

/// Installs the delegate on the shared application. AppKit holds its delegate weakly, so the
/// returned handle must stay alive for as long as the run loop runs.
pub fn install(mtm: MainThreadMarker, handler: TerminateHandler) -> InstalledDelegate {
    let delegate = AppDelegate::new(mtm, handler);
    NSApplication::sharedApplication(mtm).setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    InstalledDelegate(delegate)
}

/// Keeps the delegate alive.
pub struct InstalledDelegate(#[allow(dead_code)] Retained<AppDelegate>);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_first_quit_request_starts_the_shutdown() {
        let requests = TerminateRequests::default();
        assert!(requests.begin());
        assert!(!requests.begin());
        assert!(!requests.begin());
    }
}
