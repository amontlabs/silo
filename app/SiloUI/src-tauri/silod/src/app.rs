use std::{ffi::c_void, sync::Arc};

use objc2::{rc::autoreleasepool, MainThreadMarker};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSEvent, NSEventModifierFlags, NSEventType,
};
use silo_engine::context::MainThread;

use crate::{agent, paths::default_resource_dir, process::AppProcessControl, service};

/// Runs the alias ProxyCommand helper from the bundled silod binary. The bundle manifest
/// selects its channel before the engine-socket client reads channel-specific state.
pub fn run_remote_guest(args: &[String]) -> Result<i32, String> {
    autoreleasepool(|_| {
        if args.len() != 2 && args.len() != 3 {
            return Err("usage: silod --remote-guest DEVICE_ID COMPUTER_ID [PORT]".to_owned());
        }
        crate::run_from_bundle(&std::env::args().collect::<Vec<_>>())?;
        let manifest = service::current_bundle_manifest()?;
        silo_engine::channel::init(&manifest.channel_identifier);
        let params = silo_engine::remote::guest_stream_params(args)?;
        silo_engine::remote::run_remote_stream(&args[0], "guest.ssh", params)?;
        Ok(0)
    })
}

/// Runs the Connections bridge that a controller's forced SSH command starts through the
/// `~/.local/bin` link. It relays one request to this channel's `control.sock` and writes
/// nothing to stdout before the reply, which the controller reads as a framed stream.
pub fn run_remote_bridge(args: &[String]) -> Result<i32, String> {
    autoreleasepool(|_| {
        if !args.is_empty() {
            return Err("usage: silod --remote-bridge".to_owned());
        }
        crate::run_from_bundle(&std::env::args().collect::<Vec<_>>())?;
        let manifest = service::current_bundle_manifest()?;
        silo_engine::channel::init(&manifest.channel_identifier);
        silo_engine::remote::run_bridge()?;
        Ok(0)
    })
}

pub fn run_agent() -> Result<i32, String> {
    agent::set_private_umask();
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let resources = default_resource_dir(&executable)?;
    let manifest = service::current_bundle_manifest()?;
    let Some(owned) = agent::acquire(&manifest, resources)? else {
        return Ok(0);
    };
    autoreleasepool(|_| run_appkit(owned))
}

fn run_appkit(owned: agent::Owned) -> Result<i32, String> {
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| "silod must initialize AppKit on the process main thread".to_owned())?;
    let application = NSApplication::sharedApplication(mtm);
    // The engine is unattended: no Keychain call may wait on a dialog. A refused call fails
    // with `credential_access_denied` and the work is retried later.
    silo_engine::credential_store::forbid_interaction()?;
    // The bundle has no LSUIElement, so AppKit starts the process as a regular app and this call
    // makes it an accessory. If it is refused anyway (an agent-app bundle), the process is
    // already faceless: run on, and the VM window policy below still applies when it can.
    if !application.setActivationPolicy(NSApplicationActivationPolicy::Accessory) {
        eprintln!("silod: NSApplication refused the accessory activation policy; continuing");
    }

    let process = Arc::new(AppProcessControl::new(|| {
        dispatch_to_main(Box::new(|| {
            if let Some(mtm) = MainThreadMarker::new() {
                let application = NSApplication::sharedApplication(mtm);
                application.stop(None);
                if let Some(event) = NSEvent::otherEventWithType_location_modifierFlags_timestamp_windowNumber_context_subtype_data1_data2(
                    NSEventType::ApplicationDefined,
                    objc2_foundation::NSPoint::new(0.0, 0.0),
                    NSEventModifierFlags::empty(),
                    0.0,
                    0,
                    None,
                    0,
                    0,
                    0,
                ) {
                    application.postEvent_atStart(&event, true);
                } else {
                    eprintln!("silod: could not create the AppKit loop-stop event");
                }
            }
        }))
        .unwrap_or_else(|error| eprintln!("silod: could not schedule AppKit shutdown: {error}"));
    }));
    let serving = agent::serve(owned, Arc::new(AppKitMainThread), process.clone())?;

    // The delegate keeps the process alive when the last VM window closes and sends every
    // AppKit quit request (Dock Quit while a VM window is open, logout, shutdown) through the
    // engine's session-end shutdown. AppKit holds its delegate weakly: keep it for the run.
    let terminate_context = serving.context.clone();
    let _delegate = crate::app_delegate::install(
        mtm,
        Box::new(move || crate::methods::services::session_end(terminate_context.clone())),
    );

    application.run();
    let exit_code = process.exit_code();
    serving.finish()?;
    Ok(exit_code)
}

pub struct AppKitMainThread;

impl MainThread for AppKitMainThread {
    fn run(&self, work: Box<dyn FnOnce() + Send>) -> Result<(), String> {
        dispatch_to_main(work)
    }
}

struct ScheduledWork(Option<Box<dyn FnOnce() + Send>>);

unsafe extern "C" fn invoke_scheduled_work(context: *mut c_void) {
    let mut scheduled = unsafe { Box::from_raw(context.cast::<ScheduledWork>()) };
    if let Some(work) = scheduled.0.take() {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work));
    }
}

#[link(name = "System", kind = "dylib")]
unsafe extern "C" {
    #[link_name = "_dispatch_main_q"]
    static DISPATCH_MAIN_QUEUE: c_void;
    fn dispatch_async_f(
        queue: *mut c_void,
        context: *mut c_void,
        work: unsafe extern "C" fn(*mut c_void),
    );
}

fn dispatch_to_main(work: Box<dyn FnOnce() + Send>) -> Result<(), String> {
    let context = Box::into_raw(Box::new(ScheduledWork(Some(work)))).cast::<c_void>();
    unsafe {
        // dispatch_get_main_queue() is an SDK inline function that returns this
        // exported queue object, so bind the object directly here.
        let queue = std::ptr::addr_of!(DISPATCH_MAIN_QUEUE).cast_mut();
        dispatch_async_f(queue, context, invoke_scheduled_work);
    }
    Ok(())
}

/// The activation policy that follows the VM windows: `.regular` (Dock and Cmd-Tab) while at
/// least one is open, `.accessory` otherwise (95 A-21). Applied on the AppKit main thread by
/// the display window table.
pub(crate) fn activation_policy(any_window_open: bool) -> NSApplicationActivationPolicy {
    if any_window_open {
        NSApplicationActivationPolicy::Regular
    } else {
        NSApplicationActivationPolicy::Accessory
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_policy_is_regular_only_while_a_window_is_open() {
        assert_eq!(
            activation_policy(false),
            NSApplicationActivationPolicy::Accessory
        );
        assert_eq!(
            activation_policy(true),
            NSApplicationActivationPolicy::Regular
        );
    }
}
