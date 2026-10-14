//! Relays owners' events to this engine's own subscribers (95 A-3/A-4).
//!
//! A client subscribes on its local engine with `deviceIds`. For each other device named,
//! this engine opens one `events.subscribe` stream to the owner over Connections and
//! re-publishes what arrives on its own bus, tagged with the owner's device id, so the
//! client sees owner events exactly as it sees local ones and never talks to the owner.
//!
//! Bounds and recovery:
//! - one stream per device however many local subscriptions want it, ended when none does;
//! - the owner counts the stream against its stream budget and its event-relay limit, and
//!   refuses with a plain message when full; a refusal, a failed open and a lost stream are
//!   retried with a growing delay;
//! - every event is checked against `silo-api` and must carry the owner's device id and the
//!   next sequence number of the owner's subscription. A skipped number, an owner `gap`, or
//!   an event that does not match ends the sessions that listen to that device (the bus's
//!   gap signal), so their clients subscribe again and re-read, as after a local overflow;
//! - a stream that went silent past the stall limit counts as lost; a stream that was lost
//!   and then reopened has missed events, so reopening also ends those sessions.

use std::{
    collections::HashSet,
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use serde_json::json;
use silo_api::events::EventEnvelope;
use silo_engine::{
    context::{Audience, Event},
    events::EventBus,
    remote_events::{read_relay_frame, relayed_event_names, RelayFrame, EVENTS_STREAM},
};

/// Frames buffered between the stream's reader thread and the relay. A relay that cannot
/// keep up stops reading, and the owner's own bounded backlog turns that into a gap.
const FRAME_BUFFER: usize = 256;

#[derive(Clone, Copy)]
pub struct RelayTiming {
    /// How often a relay looks at whether anyone still wants it.
    pub tick: Duration,
    /// Silence on a stream that means it is lost; the owner sends a heartbeat every 10 s.
    pub stall: Duration,
    pub backoff_initial: Duration,
    pub backoff_max: Duration,
    /// A stream that lasted this long resets the retry delay.
    pub stable_after: Duration,
}

impl RelayTiming {
    pub const ENGINE: RelayTiming = RelayTiming {
        tick: Duration::from_secs(1),
        stall: Duration::from_secs(35),
        backoff_initial: Duration::from_secs(1),
        backoff_max: Duration::from_secs(30),
        stable_after: Duration::from_secs(60),
    };
}

/// An open stream from one owner.
pub struct OwnerStream {
    pub frames: mpsc::Receiver<Result<RelayFrame, String>>,
    /// Closes the stream when dropped.
    pub _guard: Box<dyn Send>,
}

/// How this engine reaches an owner's event stream. Production goes over the Connections
/// SSH bridge; tests supply scripted streams.
pub trait OwnerLink: Send + Sync + 'static {
    fn open(
        &self,
        device: &str,
        selectors: &[String],
        cancelled: &dyn Fn() -> bool,
    ) -> Result<OwnerStream, String>;
}

/// The Stream-class bridge method over SSH.
pub struct SshLink;

impl OwnerLink for SshLink {
    fn open(
        &self,
        device: &str,
        selectors: &[String],
        cancelled: &dyn Fn() -> bool,
    ) -> Result<OwnerStream, String> {
        let opened = silo_engine::remote::open_bridge_stream(
            device,
            EVENTS_STREAM,
            json!({ "events": selectors }),
            cancelled,
        )
        .map_err(|error| error.message)?;
        let silo_engine::remote::BridgeStream {
            child,
            input,
            mut output,
        } = opened;
        let (sender, frames) = mpsc::sync_channel(FRAME_BUFFER);
        thread::Builder::new()
            .name("silod-events-reader".into())
            .spawn(move || loop {
                let frame = read_relay_frame(&mut output);
                let failed = frame.is_err();
                if sender.send(frame).is_err() || failed {
                    break;
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(OwnerStream {
            frames,
            _guard: Box::new((child, input)),
        })
    }
}

pub struct Relays {
    bus: EventBus,
    link: Arc<dyn OwnerLink>,
    timing: RelayTiming,
    selectors: Vec<String>,
    active: Mutex<HashSet<String>>,
}

impl Relays {
    pub fn new(bus: EventBus, link: Arc<dyn OwnerLink>, timing: RelayTiming) -> Arc<Self> {
        Arc::new(Self {
            bus,
            link,
            timing,
            selectors: relayed_event_names(),
            active: Mutex::new(HashSet::new()),
        })
    }

    /// Devices with a relay running.
    pub fn active(&self) -> Vec<String> {
        let mut devices: Vec<_> = self.lock().iter().cloned().collect();
        devices.sort();
        devices
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashSet<String>> {
        self.active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn wanted(&self, device: &str) -> bool {
        self.bus.subscribed_devices().contains(device)
    }

    /// Makes sure a relay runs for `device`. A relay already running is left alone.
    pub fn ensure(self: &Arc<Self>, device: &str) {
        if !self.lock().insert(device.to_owned()) {
            return;
        }
        let relays = self.clone();
        let name = device.to_owned();
        let spawned = thread::Builder::new()
            .name("silod-events-relay".into())
            .spawn(move || relays.run(name));
        if spawned.is_err() {
            self.lock().remove(device);
        }
    }

    fn run(self: Arc<Self>, device: String) {
        let timing = self.timing;
        let mut backoff = timing.backoff_initial;
        let mut was_open = false;
        while self.wanted(&device) {
            let started = Instant::now();
            let opened = self
                .link
                .open(&device, &self.selectors, &|| !self.wanted(&device));
            match opened {
                Err(_) => self.wait(&device, &mut backoff),
                Ok(stream) => {
                    if was_open {
                        // The stream was lost and came back: what happened in between is
                        // unknown, so the sessions that listen to this device start over.
                        self.bus.raise_gap_for_device(&device);
                    }
                    was_open = true;
                    if self.wanted(&device) {
                        self.pump(&device, &stream);
                    }
                    drop(stream);
                    if started.elapsed() >= timing.stable_after {
                        backoff = timing.backoff_initial;
                    }
                    if self.wanted(&device) {
                        self.wait(&device, &mut backoff);
                    }
                }
            }
        }
        self.lock().remove(&device);
        // A client may have subscribed between the last look and the removal.
        if self.wanted(&device) {
            self.ensure(&device);
        }
    }

    /// Sleeps for the current retry delay (ending early when no one wants the device) and
    /// doubles it for the next time.
    fn wait(&self, device: &str, backoff: &mut Duration) {
        let step = self.timing.tick.min(*backoff).max(Duration::from_millis(1));
        let mut slept = Duration::ZERO;
        while slept < *backoff && self.wanted(device) {
            thread::sleep(step);
            slept += step;
        }
        *backoff = (*backoff * 2).min(self.timing.backoff_max);
    }

    /// Reads one stream until it ends, stalls, breaks the sequence or no one wants it.
    fn pump(&self, device: &str, stream: &OwnerStream) {
        let mut expected = 1u64;
        let mut subscription: Option<String> = None;
        let mut last_frame = Instant::now();
        while self.wanted(device) {
            match stream.frames.recv_timeout(self.timing.tick) {
                Ok(Ok(RelayFrame::Heartbeat)) => last_frame = Instant::now(),
                Ok(Ok(RelayFrame::Gap)) => {
                    self.bus.raise_gap_for_device(device);
                    return;
                }
                Ok(Ok(RelayFrame::Event(envelope))) => {
                    last_frame = Instant::now();
                    if !self.publish(device, envelope, &mut expected, &mut subscription) {
                        self.bus.raise_gap_for_device(device);
                        return;
                    }
                }
                Ok(Err(_)) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if last_frame.elapsed() >= self.timing.stall {
                        return;
                    }
                }
            }
        }
    }

    /// Re-publishes one owner event. False when it cannot be trusted: it is not a canonical
    /// event, names another device, belongs to another subscription or skips a number.
    fn publish(
        &self,
        device: &str,
        envelope: serde_json::Value,
        expected: &mut u64,
        subscription: &mut Option<String>,
    ) -> bool {
        let Ok(typed) = serde_json::from_value::<EventEnvelope>(envelope) else {
            return false;
        };
        let Ok(canonical) = serde_json::to_value(&typed) else {
            return false;
        };
        let (Some(name), Some(owner), Some(seq), Some(id)) = (
            canonical["event"].as_str(),
            canonical["deviceId"].as_str(),
            canonical["seq"].as_u64(),
            canonical["subscription"].as_str(),
        ) else {
            return false;
        };
        if owner != device || seq != *expected {
            return false;
        }
        match subscription {
            Some(known) if known != id => return false,
            Some(_) => {}
            None => *subscription = Some(id.to_owned()),
        }
        *expected += 1;
        self.bus.publish(
            Event::from_device(device, name, canonical["payload"].clone()),
            Audience::All,
        );
        true
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        os::unix::net::UnixStream,
        sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    use serde_json::{json, Value};
    use silo_engine::{
        context::ClientId,
        events::{EventDelivery, EventReceiver},
        remote_events::{BusSource, HEARTBEAT},
    };

    use super::*;

    const OWNER: &str = "owner-device";
    const LOCAL: &str = "local-device";

    /// Short timings: nothing here waits for real seconds.
    const FAST: RelayTiming = RelayTiming {
        tick: Duration::from_millis(5),
        stall: Duration::from_millis(400),
        backoff_initial: Duration::from_millis(5),
        backoff_max: Duration::from_millis(20),
        stable_after: Duration::from_secs(60),
    };

    type Feed = mpsc::SyncSender<Result<RelayFrame, String>>;

    /// Records when the relay lets go of a stream.
    struct Released(Arc<AtomicBool>);

    impl Drop for Released {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    struct Scripted {
        opens: AtomicUsize,
        streams: Mutex<VecDeque<Result<OwnerStream, String>>>,
    }

    impl OwnerLink for Scripted {
        fn open(
            &self,
            _device: &str,
            selectors: &[String],
            _cancelled: &dyn Fn() -> bool,
        ) -> Result<OwnerStream, String> {
            assert!(selectors.iter().any(|name| name == "computers.changed"));
            self.opens.fetch_add(1, Ordering::SeqCst);
            self.streams
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err("The owner is unreachable.".to_owned()))
        }
    }

    fn stream() -> (Feed, Result<OwnerStream, String>, Arc<AtomicBool>) {
        let (feed, frames) = mpsc::sync_channel(64);
        let released = Arc::new(AtomicBool::new(false));
        let owner_stream = OwnerStream {
            frames,
            _guard: Box::new(Released(released.clone())),
        };
        (feed, Ok(owner_stream), released)
    }

    fn scripted(streams: Vec<Result<OwnerStream, String>>) -> Arc<Scripted> {
        Arc::new(Scripted {
            opens: AtomicUsize::new(0),
            streams: Mutex::new(streams.into()),
        })
    }

    fn envelope(device: &str, seq: u64, revision: u64) -> Value {
        json!({
            "event": "notifications.cleared",
            "subscription": "owner-sub",
            "deviceId": device,
            "seq": seq,
            "payload": settings(revision),
        })
    }

    fn settings(revision: u64) -> Value {
        json!({"computerId": format!("computer-{revision}")})
    }

    struct Controller {
        bus: EventBus,
        relays: Arc<Relays>,
    }

    fn controller(link: Arc<dyn OwnerLink>) -> Controller {
        let bus = EventBus::with_device_id(LOCAL);
        let relays = Relays::new(bus.clone(), link, FAST);
        Controller { bus, relays }
    }

    impl Controller {
        fn client(&self, name: &str, devices: Option<Vec<&str>>) -> (ClientId, EventReceiver) {
            let id = ClientId::new(name);
            let receiver = self.bus.connect(id.clone());
            let subscription = self
                .bus
                .subscribe_devices(
                    &id,
                    vec!["notifications.cleared".to_owned()],
                    devices.map(|ids| ids.into_iter().map(str::to_owned).collect()),
                )
                .unwrap();
            self.bus.activate(&id, &subscription).unwrap();
            (id, receiver)
        }
    }

    async fn next(receiver: &mut EventReceiver) -> EventDelivery {
        tokio::time::timeout(Duration::from_secs(10), receiver.recv())
            .await
            .expect("an event or a gap arrives")
            .expect("the session is open")
    }

    async fn until(what: &str, mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !condition() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    }

    #[tokio::test]
    async fn owner_events_reach_subscribers_of_that_device_with_its_id_and_in_order() {
        let (feed, opened, _released) = stream();
        let link = scripted(vec![opened]);
        let controller = controller(link.clone());
        let (_remote_id, mut remote) = controller.client("remote", Some(vec![OWNER]));
        let (_both_id, mut both) = controller.client("both", Some(vec![OWNER, LOCAL]));
        let (_local_id, mut local) = controller.client("local", None);
        let (_own_id, mut own) = controller.client("own", Some(vec![LOCAL]));
        controller.relays.ensure(OWNER);

        for (seq, revision) in [(1, 10), (2, 11), (3, 12)] {
            feed.send(Ok(RelayFrame::Event(envelope(OWNER, seq, revision))))
                .unwrap();
        }
        for receiver in [&mut remote, &mut both] {
            for seq in 1..=3u64 {
                let EventDelivery::Event(event) = next(receiver).await else {
                    panic!("expected an event");
                };
                assert_eq!(event.seq, seq, "local numbering is contiguous");
                assert_eq!(event.event.device_id.as_deref(), Some(OWNER));
                assert_eq!(event.event.name, "notifications.cleared");
                assert_eq!(
                    event.event.payload["computerId"],
                    format!("computer-{}", 9 + seq)
                );
            }
        }
        // This engine's own event reaches the subscribers that asked for this engine.
        controller.bus.publish(
            Event::new("notifications.cleared", settings(1)),
            Audience::All,
        );
        for receiver in [&mut local, &mut own, &mut both] {
            let EventDelivery::Event(event) = next(receiver).await else {
                panic!("expected the local event");
            };
            assert_eq!(event.event.device_id, None);
        }
        // Nobody who did not ask for the owner saw the owner's events: the local event was
        // their first.
        assert_eq!(
            link.opens.load(Ordering::SeqCst),
            1,
            "one stream per device"
        );
    }

    #[tokio::test]
    async fn one_stream_serves_every_subscription_and_ends_with_the_last() {
        let (feed, opened, released) = stream();
        let link = scripted(vec![opened]);
        let controller = controller(link.clone());
        let (first, _first_events) = controller.client("first", Some(vec![OWNER]));
        let (second, _second_events) = controller.client("second", Some(vec![OWNER]));
        controller.relays.ensure(OWNER);
        controller.relays.ensure(OWNER);
        until("the stream to open", || {
            link.opens.load(Ordering::SeqCst) == 1
        })
        .await;
        assert_eq!(controller.relays.active(), [OWNER]);

        controller.bus.disconnect(&first);
        feed.send(Ok(RelayFrame::Heartbeat)).unwrap();
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(
            !released.load(Ordering::SeqCst),
            "a subscriber still wants it"
        );

        controller.bus.disconnect(&second);
        until("the stream to be released", || {
            released.load(Ordering::SeqCst)
        })
        .await;
        until("the relay to stop", || {
            controller.relays.active().is_empty()
        })
        .await;
        assert_eq!(link.opens.load(Ordering::SeqCst), 1);
    }

    async fn assert_gap(frames: Vec<Result<RelayFrame, String>>) {
        let (feed, opened, released) = stream();
        let controller = controller(scripted(vec![opened]));
        let (_id, mut events) = controller.client("remote", Some(vec![OWNER]));
        controller.relays.ensure(OWNER);
        for frame in frames {
            feed.send(frame).unwrap();
        }
        loop {
            match next(&mut events).await {
                EventDelivery::Gap(gap) => {
                    assert!(!gap.subscription.is_empty());
                    break;
                }
                EventDelivery::Event(_) => {}
            }
        }
        until("the stream to be released", || {
            released.load(Ordering::SeqCst)
        })
        .await;
    }

    #[tokio::test]
    async fn a_skipped_sequence_number_ends_the_sessions_that_listen_to_the_device() {
        assert_gap(vec![
            Ok(RelayFrame::Event(envelope(OWNER, 1, 1))),
            Ok(RelayFrame::Event(envelope(OWNER, 3, 3))),
        ])
        .await;
    }

    #[tokio::test]
    async fn the_owners_own_gap_signal_ends_them_too() {
        assert_gap(vec![Ok(RelayFrame::Gap)]).await;
    }

    #[tokio::test]
    async fn an_event_that_cannot_be_trusted_ends_them() {
        // The wrong device, a payload that is not the event's canonical type, and an unknown
        // event name.
        assert_gap(vec![Ok(RelayFrame::Event(envelope("someone-else", 1, 1)))]).await;
        let mut wrong_payload = envelope(OWNER, 1, 1);
        wrong_payload["payload"] = json!({"computerId": 5});
        assert_gap(vec![Ok(RelayFrame::Event(wrong_payload))]).await;
        let mut unknown = envelope(OWNER, 1, 1);
        unknown["event"] = json!("made.up");
        assert_gap(vec![Ok(RelayFrame::Event(unknown))]).await;
        // A second subscription id in one stream.
        let mut other = envelope(OWNER, 2, 2);
        other["subscription"] = json!("another-sub");
        assert_gap(vec![
            Ok(RelayFrame::Event(envelope(OWNER, 1, 1))),
            Ok(RelayFrame::Event(other)),
        ])
        .await;
    }

    #[tokio::test]
    async fn a_refused_open_is_retried_with_a_delay_until_the_owner_answers() {
        let (feed, opened, _released) = stream();
        let link = scripted(vec![
            Err("This device has too many active Silo streams.".to_owned()),
            Err("The owner is unreachable.".to_owned()),
            opened,
        ]);
        let controller = controller(link.clone());
        let (_id, mut events) = controller.client("remote", Some(vec![OWNER]));
        controller.relays.ensure(OWNER);
        until("three attempts", || link.opens.load(Ordering::SeqCst) == 3).await;
        feed.send(Ok(RelayFrame::Event(envelope(OWNER, 1, 7))))
            .unwrap();
        // The first successful stream is not a reconnection: no gap, just the event.
        let EventDelivery::Event(event) = next(&mut events).await else {
            panic!("a stream that never opened before lost nothing");
        };
        assert_eq!(event.event.payload["computerId"], "computer-7");
    }

    #[tokio::test]
    async fn a_lost_stream_is_reopened_and_the_reopening_ends_the_sessions_that_missed_events() {
        let (first_feed, first, first_released) = stream();
        let (second_feed, second, _second_released) = stream();
        let link = scripted(vec![first, second]);
        let controller = controller(link.clone());
        let (_id, mut events) = controller.client("remote", Some(vec![OWNER]));
        controller.relays.ensure(OWNER);
        first_feed
            .send(Ok(RelayFrame::Event(envelope(OWNER, 1, 1))))
            .unwrap();
        assert!(matches!(next(&mut events).await, EventDelivery::Event(_)));

        // The link breaks: the reader ends with an error.
        first_feed
            .send(Err("The remote Silo connection ended.".into()))
            .unwrap();
        until("the lost stream to be released", || {
            first_released.load(Ordering::SeqCst)
        })
        .await;
        until("the reopening", || link.opens.load(Ordering::SeqCst) == 2).await;
        // Events may have been missed in between: the session ends, and its client
        // subscribes again and re-reads (A-4).
        assert!(matches!(next(&mut events).await, EventDelivery::Gap(_)));
        drop(second_feed);
    }

    #[tokio::test]
    async fn a_silent_stream_is_lost_after_the_stall_limit_and_a_heartbeat_keeps_it() {
        let (feed, opened, released) = stream();
        let controller = controller(scripted(vec![opened]));
        let (_id, _events) = controller.client("remote", Some(vec![OWNER]));
        controller.relays.ensure(OWNER);
        // Heartbeats alone keep it open past several stall periods.
        let keep_alive = {
            let feed = feed.clone();
            thread::spawn(move || {
                for _ in 0..30 {
                    if feed.send(Ok(RelayFrame::Heartbeat)).is_err() {
                        return;
                    }
                    thread::sleep(Duration::from_millis(40));
                }
            })
        };
        tokio::time::sleep(FAST.stall * 2).await;
        assert!(!released.load(Ordering::SeqCst));
        keep_alive.join().unwrap();
        // Then silence: lost, so released.
        until("the silent stream to be released", || {
            released.load(Ordering::SeqCst)
        })
        .await;
        drop(feed);
    }

    /// The owner's real half over a real pair of sockets: an owner bus, the subscription
    /// the owner makes for a controller, the framing, and this controller's relay.
    struct Wire {
        owner: EventBus,
    }

    /// Closing a bridge stream ends the ssh process, so the owner sees the controller leave.
    struct Hangup(UnixStream);

    impl Drop for Hangup {
        fn drop(&mut self) {
            let _ = self.0.shutdown(std::net::Shutdown::Both);
        }
    }

    impl OwnerLink for Wire {
        fn open(
            &self,
            _device: &str,
            selectors: &[String],
            _cancelled: &dyn Fn() -> bool,
        ) -> Result<OwnerStream, String> {
            let (controller_end, owner_end) = UnixStream::pair().unwrap();
            let source = BusSource::open(&self.owner, OWNER, selectors.to_vec())?;
            let connected = owner_end.try_clone().unwrap();
            thread::spawn(move || {
                silo_engine::remote_events::serve(
                    owner_end,
                    Box::new(source),
                    // The controller leaving shows as its end closing.
                    &|| silo_engine::remote::connection_open(&connected),
                    &|| true,
                    HEARTBEAT,
                );
            });
            let (sender, frames) = mpsc::sync_channel(FRAME_BUFFER);
            let mut reader = controller_end.try_clone().unwrap();
            thread::spawn(move || loop {
                let frame = read_relay_frame(&mut reader);
                let failed = frame.is_err();
                if sender.send(frame).is_err() || failed {
                    break;
                }
            });
            Ok(OwnerStream {
                frames,
                _guard: Box::new(Hangup(controller_end)),
            })
        }
    }

    #[tokio::test]
    async fn two_engines_in_one_process_relay_owner_events_over_the_stream_protocol() {
        let owner = EventBus::with_device_id(OWNER);
        let wire = Arc::new(Wire {
            owner: owner.clone(),
        });
        let controller = controller(wire.clone());
        let (_id, mut events) = controller.client("remote", Some(vec![OWNER]));
        controller.relays.ensure(OWNER);
        until("the owner to see the relay", || {
            !owner.connected_clients().is_empty()
        })
        .await;

        for revision in 1..=5 {
            owner.publish(
                Event::new("notifications.cleared", settings(revision)),
                Audience::All,
            );
        }
        // Events this engine's own bus publishes are not the owner's.
        controller.bus.publish(
            Event::new("notifications.cleared", settings(99)),
            Audience::All,
        );
        let mut seen = Vec::new();
        while seen.len() < 5 {
            let EventDelivery::Event(event) = next(&mut events).await else {
                panic!("no gap expected");
            };
            assert_eq!(event.event.device_id.as_deref(), Some(OWNER));
            let revision = event.event.payload["computerId"]
                .as_str()
                .unwrap()
                .trim_start_matches("computer-")
                .parse::<u64>()
                .unwrap();
            seen.push((event.seq, revision));
        }
        assert_eq!(seen, [(1, 1), (2, 2), (3, 3), (4, 4), (5, 5)]);

        // The last subscriber leaves: the controller closes its end and the owner's
        // subscription for it goes away.
        controller.bus.disconnect(&ClientId::new("remote"));
        until("the owner's relay client to leave", || {
            owner.connected_clients().is_empty()
        })
        .await;
    }
}
