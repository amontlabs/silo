//! The Stream-class `guest.files.watch` bridge method.
//!
//! A change hint of a watch belongs to the client that asked for it, so it is not part of the
//! events relay (`remote_events`). A controller whose client watches a folder of a computer
//! on another device opens one stream per watch instead. The owner starts the watch for it
//! and writes one framed message per hint after the stream's `{"result":{}}` reply:
//! - `{"type":"changed","path":"/watched/folder"}`;
//! - `{"type":"heartbeat"}`, at least every [`HEARTBEAT`], so a silent owner is told apart
//!   from a quiet folder.
//!
//! The watch ends when the controller closes the stream, Connections stops being allowed, or
//! the computer stops. The controller re-publishes each hint to the subscription of the
//! client that asked (`silod`); a lost stream ends that subscription.

use std::{
    io::{Read, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};

use serde_json::{json, Value};

/// The stream method's name on the bridge.
pub use crate::remote_access::GUEST_WATCH_STREAM;

/// The longest an owner stays silent on a stream with nothing to send.
pub const HEARTBEAT: Duration = Duration::from_secs(10);

/// How often the owner looks at whether the controller is still there.
const LIVENESS: Duration = Duration::from_secs(1);

/// One message on the stream.
#[derive(Debug, PartialEq)]
pub enum WatchFrame {
    Changed(String),
    Heartbeat,
}

/// Decodes a frame read from the stream.
pub fn decode_frame(frame: &Value) -> Result<WatchFrame, String> {
    match frame.get("type").and_then(Value::as_str) {
        Some("changed") => frame
            .get("path")
            .and_then(Value::as_str)
            .map(|path| WatchFrame::Changed(path.to_owned()))
            .ok_or_else(|| "A change frame has no path.".to_owned()),
        Some("heartbeat") => Ok(WatchFrame::Heartbeat),
        _ => Err("The other device sent an unknown watch frame.".to_owned()),
    }
}

/// Reads one frame from an owner's stream.
pub fn read_watch_frame(reader: impl Read) -> Result<WatchFrame, String> {
    decode_frame(&crate::remote::read_stream_frame(reader)?)
}

/// A watch being served: the hints it produced and the flag that ends it.
pub struct Watching {
    pub hints: mpsc::Receiver<String>,
    live: Arc<AtomicBool>,
}

impl Watching {
    /// Starts the watch with `start`. Its hints queue without bound only as far as the
    /// folder changes: the guest helper coalesces bursts into one hint per look.
    pub fn start(
        params: &Value,
        start: impl FnOnce(
            &Value,
            Arc<dyn Fn(String) + Send + Sync>,
            Arc<dyn Fn() -> bool + Send + Sync>,
        ) -> Result<(), String>,
    ) -> Result<Self, String> {
        let (sender, hints) = mpsc::channel::<String>();
        let live = Arc::new(AtomicBool::new(true));
        let emit: Arc<dyn Fn(String) + Send + Sync> = {
            let sender = std::sync::Mutex::new(sender);
            Arc::new(move |path| {
                let _ = sender
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .send(path);
            })
        };
        let wanted: Arc<dyn Fn() -> bool + Send + Sync> = {
            let live = live.clone();
            Arc::new(move || live.load(Ordering::Acquire))
        };
        start(params, emit, wanted)?;
        Ok(Self { hints, live })
    }
}

impl Drop for Watching {
    fn drop(&mut self) {
        self.live.store(false, Ordering::Release);
    }
}

/// The owner's loop: writes hints and heartbeats to `writer` until the controller leaves
/// (`connected` turns false), the device stops allowing it (`allowed`), the watch ends or a
/// write fails.
pub fn serve(
    mut writer: impl Write,
    watching: &Watching,
    connected: &dyn Fn() -> bool,
    allowed: &dyn Fn() -> bool,
    heartbeat: Duration,
) {
    let mut last_write = Instant::now();
    while connected() && allowed() {
        let frame = match watching.hints.recv_timeout(LIVENESS.min(heartbeat)) {
            Ok(path) => json!({"type": "changed", "path": path}),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if last_write.elapsed() < heartbeat {
                    continue;
                }
                json!({"type": "heartbeat"})
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        };
        if crate::remote::write_stream_frame(&mut writer, &frame).is_err() {
            return;
        }
        last_write = Instant::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip_and_unknown_ones_are_refused() {
        let mut bytes = Vec::new();
        crate::remote::write_stream_frame(&mut bytes, &json!({"type":"changed","path":"/w"}))
            .unwrap();
        crate::remote::write_stream_frame(&mut bytes, &json!({"type":"heartbeat"})).unwrap();
        crate::remote::write_stream_frame(&mut bytes, &json!({"type":"other"})).unwrap();
        crate::remote::write_stream_frame(&mut bytes, &json!({"type":"changed"})).unwrap();
        let mut reader = bytes.as_slice();
        assert_eq!(
            read_watch_frame(&mut reader),
            Ok(WatchFrame::Changed("/w".into()))
        );
        assert_eq!(read_watch_frame(&mut reader), Ok(WatchFrame::Heartbeat));
        assert!(read_watch_frame(&mut reader).is_err());
        assert!(read_watch_frame(&mut reader).is_err());
    }

    #[test]
    fn the_owner_writes_hints_and_heartbeats_and_stops_the_watch_when_it_ends() {
        let emitter = Arc::new(std::sync::Mutex::new(
            None::<Arc<dyn Fn(String) + Send + Sync>>,
        ));
        let wanted_flag = Arc::new(std::sync::Mutex::new(
            None::<Arc<dyn Fn() -> bool + Send + Sync>>,
        ));
        let (e, w) = (emitter.clone(), wanted_flag.clone());
        let watching = Watching::start(&json!({}), move |_, emit, wanted| {
            *e.lock().unwrap() = Some(emit);
            *w.lock().unwrap() = Some(wanted);
            Ok(())
        })
        .unwrap();
        let emit = emitter.lock().unwrap().clone().unwrap();
        let wanted = wanted_flag.lock().unwrap().clone().unwrap();
        emit("/workspace".into());
        let mut written = Vec::new();
        let rounds = std::cell::Cell::new(0);
        serve(
            &mut written,
            &watching,
            &|| {
                rounds.set(rounds.get() + 1);
                rounds.get() < 4
            },
            &|| true,
            Duration::from_millis(1),
        );
        let mut reader = written.as_slice();
        assert_eq!(
            read_watch_frame(&mut reader),
            Ok(WatchFrame::Changed("/workspace".into()))
        );
        assert_eq!(read_watch_frame(&mut reader), Ok(WatchFrame::Heartbeat));
        assert!(wanted());
        drop(watching);
        assert!(!wanted());
    }

    #[test]
    fn a_watch_that_cannot_start_is_reported_before_any_frame() {
        let error = Watching::start(&json!({}), |_, _, _| Err("No such computer.".into()))
            .err()
            .unwrap();
        assert_eq!(error, "No such computer.");
    }
}
