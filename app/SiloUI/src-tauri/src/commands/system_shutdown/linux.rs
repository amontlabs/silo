//! A logind "delay" inhibitor holds system shutdown and reboot while Silo stops
//! its local computers (F-20). logind sends `PrepareForShutdown(true)`, then waits
//! until every delay lock is released or `InhibitDelayMaxSec` passes. Silo
//! releases its lock right before exiting. Suspend is not inhibited: sleeping
//! must not stop computers.
use std::{sync::Mutex, time::Duration};

use tauri::AppHandle;
use zbus::{
    blocking::{Connection, Proxy},
    zvariant::OwnedFd,
};

static INHIBITOR: Mutex<Option<OwnedFd>> = Mutex::new(None);

pub(super) fn install(app: &AppHandle) {
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("silo-logind".into())
        .spawn(move || {
            if let Err(error) = watch(&app) {
                eprintln!("Silo shutdown handling: logind is unavailable: {error}");
            }
        });
    if let Err(error) = spawned {
        eprintln!("Silo shutdown handling: {error}");
    }
}

fn watch(app: &AppHandle) -> zbus::Result<()> {
    let connection = Connection::system()?;
    let manager = Proxy::new(
        &connection,
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )?;
    // Subscribe before taking the lock so no PrepareForShutdown can be missed.
    let signals = manager.receive_signal("PrepareForShutdown")?;
    let max_delay = manager
        .get_property::<u64>("InhibitDelayMaxUSec")
        .ok()
        .map(Duration::from_micros);
    let lock: OwnedFd = manager.call(
        "Inhibit",
        &(
            "shutdown",
            crate::channel::current().product_name(),
            "Stopping local computers",
            "delay",
        ),
    )?;
    *INHIBITOR.lock().unwrap_or_else(|error| error.into_inner()) = Some(lock);
    for signal in signals {
        let body = signal.body();
        let starting: bool = body.deserialize()?;
        if starting {
            crate::settings::end_session(app, crate::system_shutdown::logind_budget(max_delay));
        }
    }
    Ok(())
}

/// Let shutdown continue. Closing the descriptor releases the delay lock.
pub(super) fn release() {
    INHIBITOR
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .take();
}
