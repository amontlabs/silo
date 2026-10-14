//! The Stream-class `events.subscribe` bridge method (95 A-3/A-4).
//!
//! A controller's engine opens one stream to each owner that one of its clients asked for.
//! The owner subscribes to its own event bus on the controller's behalf and writes what it
//! publishes, one framed message at a time. The controller checks every event against
//! `silo-api`, requires the owner's sequence to be contiguous, and re-publishes the event on
//! its own bus tagged with the owner's device id. This file holds what both ends share and
//! the owner's loop; the controller's half is in `silod`.
//!
//! Frames, after the stream's `{"result":{}}` reply:
//! - `{"type":"event","envelope":{event, subscription, deviceId, seq, payload}}`;
//! - `{"type":"heartbeat"}`, at least every [`HEARTBEAT`], so a silent owner is told apart
//!   from an idle one;
//! - `{"type":"gap"}`, after which the owner ends the stream: its backlog for this
//!   controller overflowed, or one event did not fit a frame.
//!
//! The owner counts each stream against the stream budget and against its own, smaller
//! [`EVENT_RELAY_LIMIT`], so event relays cannot take the room that ports and desktops need.

use std::{
    future::Future,
    io::{Read, Write},
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
    thread,
    time::{Duration, Instant},
};

use serde_json::{json, Value};
use silo_api::events::EVENT_NAMES;

use crate::{
    context::ClientId,
    events::{EventBus, EventDelivery, EventReceiver},
};

/// The stream method's name on the bridge.
pub use crate::remote_access::EVENTS_STREAM;

/// Event relays one owner serves at once, a share of its stream budget.
pub const EVENT_RELAY_LIMIT: usize = 16;

/// The longest an owner stays silent on a stream that has nothing to send.
pub const HEARTBEAT: Duration = Duration::from_secs(10);

/// Events that belong to one connection or one subscription on the owner and so are never
/// relayed: they have no meaning to another engine's clients.
const CLIENT_SCOPED: &[&str] = &[
    "display.viewer.page",
    "guest.exec.output",
    "guest.exec.exited",
    "guest.files.changed",
];

/// The events a controller subscribes to on an owner.
pub fn relayed_event_names() -> Vec<String> {
    EVENT_NAMES
        .iter()
        .filter(|name| !CLIENT_SCOPED.contains(name))
        .map(|name| (*name).to_owned())
        .collect()
}

/// The selectors in a `events.subscribe` bridge request, each a relayed event name.
pub fn parse_selectors(params: &Value) -> Result<Vec<String>, String> {
    let relayed = relayed_event_names();
    let selectors = params
        .get("events")
        .and_then(Value::as_array)
        .filter(|selectors| !selectors.is_empty() && selectors.len() <= relayed.len())
        .ok_or("An event subscription needs at least one event.")?;
    let mut parsed = Vec::new();
    for selector in selectors {
        let selector = selector.as_str().ok_or("Invalid event selector.")?;
        if !relayed.iter().any(|name| name == selector) {
            return Err(format!("Event {selector} is not relayed between engines."));
        }
        if !parsed.iter().any(|known| known == selector) {
            parsed.push(selector.to_owned());
        }
    }
    Ok(parsed)
}

/// What the owner's source yields next.
#[derive(Debug, PartialEq)]
pub enum EventPoll {
    /// A wire envelope of this engine's own event.
    Event(Value),
    /// Nothing arrived within the wait.
    Idle,
    /// Events were lost; the stream must end.
    Gap,
    /// The source is gone.
    Ended,
}

/// The owner's events for one controller.
pub trait EventSource: Send {
    fn next(&mut self, wait: Duration) -> EventPoll;
}

/// A subscription on this engine's own bus, read from a plain thread.
pub struct BusSource {
    receiver: Option<EventReceiver>,
    bus: EventBus,
    client: ClientId,
    device_id: String,
}

impl BusSource {
    /// Connects a client to `bus`, subscribes it to `selectors` and starts delivery.
    pub fn open(bus: &EventBus, device_id: &str, selectors: Vec<String>) -> Result<Self, String> {
        let client = ClientId::new(format!("relay:{}", uuid::Uuid::new_v4()));
        let receiver = bus.connect(client.clone());
        let subscribed = bus
            .subscribe(&client, selectors)
            .and_then(|subscription| bus.activate(&client, &subscription));
        if let Err(error) = subscribed {
            bus.disconnect(&client);
            return Err(error.to_string());
        }
        Ok(Self {
            receiver: Some(receiver),
            bus: bus.clone(),
            client,
            device_id: device_id.to_owned(),
        })
    }
}

impl Drop for BusSource {
    fn drop(&mut self) {
        self.receiver = None;
        self.bus.disconnect(&self.client);
    }
}

impl EventSource for BusSource {
    fn next(&mut self, wait: Duration) -> EventPoll {
        let Some(receiver) = self.receiver.as_mut() else {
            return EventPoll::Ended;
        };
        match block_on_timeout(receiver.recv(), wait) {
            None => EventPoll::Idle,
            Some(None) => EventPoll::Ended,
            Some(Some(EventDelivery::Gap(_))) => EventPoll::Gap,
            Some(Some(EventDelivery::Event(event))) => {
                EventPoll::Event(crate::events::wire_event_envelope(
                    &event.event,
                    &event.subscription,
                    &self.device_id,
                    event.seq,
                ))
            }
        }
    }
}

struct Unparker(thread::Thread);

impl Wake for Unparker {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

/// Polls `future` on this thread for at most `timeout`. Dropping an unfinished receive loses
/// nothing: the receiver pops an event only inside the poll that returns it.
fn block_on_timeout<F: Future>(future: F, timeout: Duration) -> Option<F::Output> {
    let mut future = Box::pin(future);
    let waker = Waker::from(Arc::new(Unparker(thread::current())));
    let mut context = Context::from_waker(&waker);
    let deadline = Instant::now() + timeout;
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return Some(output);
        }
        let now = Instant::now();
        if now >= deadline {
            return None;
        }
        thread::park_timeout(deadline - now);
    }
}

/// One message on the stream.
#[derive(Debug, PartialEq)]
pub enum RelayFrame {
    Event(Value),
    Heartbeat,
    Gap,
}

/// Decodes a frame read from the stream.
pub fn decode_frame(frame: &Value) -> Result<RelayFrame, String> {
    match frame.get("type").and_then(Value::as_str) {
        Some("event") => frame
            .get("envelope")
            .cloned()
            .map(RelayFrame::Event)
            .ok_or_else(|| "An event frame has no envelope.".to_owned()),
        Some("heartbeat") => Ok(RelayFrame::Heartbeat),
        Some("gap") => Ok(RelayFrame::Gap),
        _ => Err("The other device sent an unknown event frame.".to_owned()),
    }
}

/// Reads one frame from an owner's stream.
pub fn read_relay_frame(reader: impl Read) -> Result<RelayFrame, String> {
    decode_frame(&crate::remote::read_stream_frame(reader)?)
}

/// How often the owner looks at whether the controller is still there.
const LIVENESS: Duration = Duration::from_secs(1);

/// The owner's loop: writes events, heartbeats and a final gap to `writer` until the
/// controller leaves (`connected` turns false), the device stops allowing it (`allowed`), the
/// source ends or a write fails.
pub fn serve(
    mut writer: impl Write,
    mut source: Box<dyn EventSource>,
    connected: &dyn Fn() -> bool,
    allowed: &dyn Fn() -> bool,
    heartbeat: Duration,
) {
    let mut last_write = Instant::now();
    while connected() && allowed() {
        let wait = LIVENESS.min(heartbeat);
        let frame = match source.next(wait) {
            EventPoll::Event(envelope) => json!({"type": "event", "envelope": envelope}),
            EventPoll::Idle => {
                if last_write.elapsed() < heartbeat {
                    continue;
                }
                json!({"type": "heartbeat"})
            }
            EventPoll::Gap => {
                let _ = crate::remote::write_stream_frame(&mut writer, &json!({"type": "gap"}));
                return;
            }
            EventPoll::Ended => return,
        };
        match crate::remote::write_stream_frame(&mut writer, &frame) {
            Ok(()) => last_write = Instant::now(),
            Err(_) if frame["type"] == "event" => {
                // The event did not fit a frame, or the controller stopped reading. Either
                // way it cannot be sent, and later events must not pass it silently.
                let _ = crate::remote::write_stream_frame(&mut writer, &json!({"type": "gap"}));
                return;
            }
            Err(_) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{Audience, Event};
    use std::sync::atomic::Ordering;

    fn bus() -> EventBus {
        EventBus::with_device_id("owner-device")
    }

    #[test]
    fn client_scoped_events_are_never_relayed() {
        let names = relayed_event_names();
        for scoped in CLIENT_SCOPED {
            assert!(!names.iter().any(|name| name == scoped), "{scoped}");
        }
        assert!(names.iter().any(|name| name == "computers.changed"));
        assert!(names.iter().any(|name| name == "operations.job.finished"));
    }

    #[test]
    fn selectors_are_checked_against_the_relayed_names() {
        let ok = parse_selectors(&json!({"events": ["computers.changed", "computers.changed"]}));
        assert_eq!(ok.unwrap(), ["computers.changed"]);
        for bad in [
            json!({}),
            json!({"events": []}),
            json!({"events": ["guest.exec.output"]}),
            json!({"events": ["computers.*"]}),
            json!({"events": [1]}),
            json!({"events": "computers.changed"}),
        ] {
            assert!(parse_selectors(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_bus_source_yields_this_engines_events_in_order_and_idles_without_them() {
        let bus = bus();
        let mut source =
            BusSource::open(&bus, "owner-device", vec!["settings.changed".to_owned()]).unwrap();
        assert_eq!(source.next(Duration::from_millis(5)), EventPoll::Idle);
        for revision in 1..=3 {
            bus.publish(
                Event::new("settings.changed", json!({"revision": revision})),
                Audience::All,
            );
        }
        bus.publish(Event::new("computers.changed", json!({})), Audience::All);
        for revision in 1..=3 {
            let EventPoll::Event(envelope) = source.next(Duration::from_secs(5)) else {
                panic!("expected an event");
            };
            assert_eq!(envelope["seq"], revision);
            assert_eq!(envelope["deviceId"], "owner-device");
            assert_eq!(envelope["payload"]["revision"], revision);
        }
        assert_eq!(source.next(Duration::from_millis(5)), EventPoll::Idle);
    }

    #[test]
    fn a_bus_source_reports_an_overflow_as_a_gap_and_leaves_the_bus_when_dropped() {
        let bus = bus();
        let mut source =
            BusSource::open(&bus, "owner-device", vec!["settings.changed".to_owned()]).unwrap();
        for revision in 0..=crate::events::MAX_EVENTS_PER_SUBSCRIPTION {
            bus.publish(
                Event::new("settings.changed", json!({"revision": revision})),
                Audience::All,
            );
        }
        assert_eq!(source.next(Duration::from_secs(5)), EventPoll::Gap);
        assert_eq!(bus.connected_clients().len(), 1);
        drop(source);
        assert!(bus.connected_clients().is_empty());
    }

    struct Script(std::collections::VecDeque<EventPoll>);

    impl EventSource for Script {
        fn next(&mut self, _wait: Duration) -> EventPoll {
            self.0.pop_front().unwrap_or(EventPoll::Ended)
        }
    }

    fn read_all(bytes: Vec<u8>) -> Vec<RelayFrame> {
        let mut cursor = std::io::Cursor::new(bytes);
        let mut frames = Vec::new();
        while let Ok(frame) = read_relay_frame(&mut cursor) {
            frames.push(frame);
        }
        frames
    }

    #[test]
    fn the_owner_writes_events_heartbeats_and_a_final_gap() {
        let envelope = json!({"event": "settings.changed", "seq": 1});
        let mut written = Vec::new();
        serve(
            &mut written,
            Box::new(Script(
                [
                    EventPoll::Event(envelope.clone()),
                    EventPoll::Idle,
                    EventPoll::Gap,
                    EventPoll::Event(json!({"never": "sent"})),
                ]
                .into(),
            )),
            &|| true,
            &|| true,
            Duration::ZERO,
        );
        assert_eq!(
            read_all(written),
            vec![
                RelayFrame::Event(envelope),
                RelayFrame::Heartbeat,
                RelayFrame::Gap
            ]
        );
    }

    #[test]
    fn the_owner_stops_when_the_controller_leaves_or_the_device_stops_allowing_it() {
        let events = || -> Box<dyn EventSource> {
            Box::new(Script(
                std::iter::repeat_with(|| EventPoll::Event(json!({"seq": 1})))
                    .take(50)
                    .collect(),
            ))
        };
        let mut written = Vec::new();
        serve(&mut written, events(), &|| false, &|| true, HEARTBEAT);
        assert!(written.is_empty());
        serve(&mut written, events(), &|| true, &|| false, HEARTBEAT);
        assert!(written.is_empty());

        let checks = std::sync::atomic::AtomicUsize::new(0);
        let mut written = Vec::new();
        serve(
            &mut written,
            events(),
            &|| checks.fetch_add(1, Ordering::SeqCst) < 2,
            &|| true,
            HEARTBEAT,
        );
        assert_eq!(read_all(written).len(), 2);
    }

    #[test]
    fn an_event_too_large_for_a_frame_ends_the_stream_with_a_gap() {
        let huge = json!({"payload": "x".repeat(5 * 1024 * 1024)});
        let mut written = Vec::new();
        serve(
            &mut written,
            Box::new(Script(
                [
                    EventPoll::Event(huge),
                    EventPoll::Event(json!({"after": 1})),
                ]
                .into(),
            )),
            &|| true,
            &|| true,
            HEARTBEAT,
        );
        assert_eq!(read_all(written), vec![RelayFrame::Gap]);
    }

    #[test]
    fn a_dead_writer_ends_the_loop() {
        struct Broken;
        impl Write for Broken {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        serve(
            Broken,
            Box::new(Script(
                std::iter::repeat_with(|| EventPoll::Idle)
                    .take(100)
                    .collect(),
            )),
            &|| true,
            &|| true,
            Duration::ZERO,
        );
    }

    #[test]
    fn unknown_frames_are_refused() {
        assert!(decode_frame(&json!({"type": "other"})).is_err());
        assert!(decode_frame(&json!({"type": "event"})).is_err());
        assert!(decode_frame(&json!({})).is_err());
    }
}
