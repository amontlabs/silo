//! Bounded engine event fanout with per-subscription sequences.

use std::{
    collections::{HashMap, VecDeque},
    fmt,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex, Weak,
    },
};

use silo_api::events::EVENT_NAMES;
use tokio::sync::Notify;
use uuid::Uuid;

use crate::context::{Audience, ClientId, Event};

pub const MAX_EVENTS_PER_SUBSCRIPTION: usize = 256;
pub const MAX_BYTES_PER_SUBSCRIPTION: usize = 8 * 1024 * 1024;
pub const MAX_BYTES_PER_SESSION: usize = 16 * 1024 * 1024;
pub const MAX_SUBSCRIPTIONS_PER_SESSION: usize = 16;
const FRAME_LENGTH_PREFIX_BYTES: usize = std::mem::size_of::<u32>();

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GapSignal {
    pub subscription: String,
    pub expected_seq: u64,
    pub dropped_seq: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SequencedEvent {
    pub subscription: String,
    /// Monotonic publication order for this engine process.
    pub engine_seq: u64,
    /// Per-subscription sequence after topic and audience filtering.
    pub seq: u64,
    pub event: Event,
    /// Holds the event's subscription and session backlog reservation until the
    /// socket writes or discards the corresponding frame.
    pub backlog_reservation: BacklogReservation,
}

#[derive(Clone)]
pub struct BacklogReservation(Arc<ReservationLease>);

impl fmt::Debug for BacklogReservation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BacklogReservation")
            .field("bytes", &self.0.bytes)
            .finish()
    }
}

impl PartialEq for BacklogReservation {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for BacklogReservation {}

#[derive(Clone, Debug, PartialEq)]
pub enum EventDelivery {
    Event(SequencedEvent),
    Gap(GapSignal),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventError(pub String);

impl std::fmt::Display for EventError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for EventError {}

#[derive(Clone)]
pub struct EventBus {
    inner: Arc<Mutex<BusState>>,
    device_id: Arc<str>,
}

#[derive(Default)]
struct BusState {
    next_engine_seq: u64,
    subscribers: HashMap<String, Weak<Subscriber>>,
}

struct Subscriber {
    client_id: ClientId,
    state: Mutex<SubscriberState>,
    notify: Notify,
    session_backlog_bytes: Arc<AtomicUsize>,
}

#[derive(Default)]
struct SubscriberState {
    subscriptions: HashMap<String, SubscriptionState>,
    queue: VecDeque<SequencedEvent>,
    gap: Option<GapSignal>,
    closed: bool,
}

#[derive(Default)]
struct SubscriptionBacklog {
    events: AtomicUsize,
    bytes: AtomicUsize,
}

struct ReservationLease {
    subscription: Arc<SubscriptionBacklog>,
    session_bytes: Arc<AtomicUsize>,
    bytes: usize,
}

impl Drop for ReservationLease {
    fn drop(&mut self) {
        self.subscription.events.fetch_sub(1, Ordering::AcqRel);
        self.subscription
            .bytes
            .fetch_sub(self.bytes, Ordering::AcqRel);
        self.session_bytes.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

struct SubscriptionState {
    selectors: Vec<String>,
    active: bool,
    closing: bool,
    next_seq: u64,
    latest_by_domain: HashMap<String, u64>,
    last_delivered: u64,
    backlog: Arc<SubscriptionBacklog>,
    pending: VecDeque<SequencedEvent>,
}

pub struct EventReceiver {
    bus: EventBus,
    client_id: ClientId,
    subscriber: Arc<Subscriber>,
}

impl EventBus {
    pub fn with_device_id(device_id: impl Into<String>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(BusState::default())),
            device_id: Arc::from(device_id.into()),
        }
    }

    pub fn connect(&self, client_id: ClientId) -> EventReceiver {
        let subscriber = Arc::new(Subscriber {
            client_id: client_id.clone(),
            state: Mutex::new(SubscriberState::default()),
            notify: Notify::new(),
            session_backlog_bytes: Arc::new(AtomicUsize::new(0)),
        });
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .subscribers
            .insert(client_id.as_str().to_owned(), Arc::downgrade(&subscriber));
        EventReceiver {
            bus: self.clone(),
            client_id,
            subscriber,
        }
    }

    /// Creates a pending subscription. The socket activates it only after it
    /// has queued the `{subscription}` response, preserving the wire barrier.
    pub fn subscribe(
        &self,
        client_id: &ClientId,
        selectors: Vec<String>,
    ) -> Result<String, EventError> {
        validate_selectors(&selectors)?;
        let subscriber = self.find_subscriber(client_id)?;
        let id = Uuid::new_v4().to_string();
        let mut state = subscriber
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.closed {
            return Err(EventError("The event session is closed.".to_owned()));
        }
        if state.subscriptions.len() >= MAX_SUBSCRIPTIONS_PER_SESSION {
            return Err(EventError(
                "The event session has too many subscriptions.".to_owned(),
            ));
        }
        state.subscriptions.insert(
            id.clone(),
            SubscriptionState {
                selectors,
                active: false,
                closing: false,
                next_seq: 1,
                latest_by_domain: HashMap::new(),
                last_delivered: 0,
                backlog: Arc::new(SubscriptionBacklog::default()),
                pending: VecDeque::new(),
            },
        );
        Ok(id)
    }

    pub fn activate(&self, client_id: &ClientId, subscription: &str) -> Result<(), EventError> {
        let subscriber = self.find_subscriber(client_id)?;
        let mut state = subscriber
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let pending = {
            let Some(subscription_state) = state.subscriptions.get_mut(subscription) else {
                return Err(EventError(
                    "The event subscription no longer exists.".to_owned(),
                ));
            };
            subscription_state.active = true;
            subscription_state.pending.drain(..).collect::<Vec<_>>()
        };
        state.queue.extend(pending);
        drop(state);
        subscriber.notify.notify_one();
        Ok(())
    }

    pub fn unsubscribe(&self, client_id: &ClientId, subscription: &str) -> HashMap<String, u64> {
        let Ok(subscriber) = self.find_subscriber(client_id) else {
            return HashMap::new();
        };
        let mut state = subscriber
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(subscription_state) = state.subscriptions.get_mut(subscription) else {
            return HashMap::new();
        };
        let last_seq = subscription_state.next_seq.saturating_sub(1);
        subscription_state.selectors.clear();
        subscription_state.closing = true;
        subscription_state.active = true;
        let pending = subscription_state.pending.drain(..).collect::<Vec<_>>();
        let barrier = (last_seq > 0)
            .then(|| HashMap::from([(subscription.to_owned(), last_seq)]))
            .unwrap_or_default();
        state.queue.extend(pending);
        let removable = state
            .subscriptions
            .get(subscription)
            .is_some_and(|subscription| subscription.pending.is_empty());
        if removable {
            state.subscriptions.remove(subscription);
        }
        drop(state);
        subscriber.notify.notify_one();
        barrier
    }

    pub fn disconnect(&self, client_id: &ClientId) {
        let subscriber = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .subscribers
            .remove(client_id.as_str())
            .and_then(|weak| weak.upgrade());
        if let Some(subscriber) = subscriber {
            let mut state = subscriber
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.closed = true;
            state.subscriptions.clear();
            state.queue.clear();
            drop(state);
            subscriber.notify.notify_waiters();
        }
    }

    /// Captures each matching operations subscription's last sequence while a
    /// caller holds the corresponding published-projection lock.
    pub fn operations_barrier(
        &self,
        client_id: &ClientId,
    ) -> Result<HashMap<String, u64>, EventError> {
        self.domain_barrier(client_id, "operations")
    }

    pub fn domain_barrier(
        &self,
        client_id: &ClientId,
        domain: &str,
    ) -> Result<HashMap<String, u64>, EventError> {
        let subscriber = self.find_subscriber(client_id)?;
        let state = subscriber
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Ok(state
            .subscriptions
            .iter()
            .filter_map(|(id, subscription)| {
                subscription
                    .latest_by_domain
                    .get(domain)
                    .copied()
                    .map(|seq| (id.clone(), seq))
            })
            .collect())
    }

    pub fn publish(&self, event: Event, audience: Audience) -> u64 {
        self.publish_with_fanout_hook(event, audience, || {})
    }

    fn publish_with_fanout_hook(
        &self,
        event: Event,
        audience: Audience,
        after_first_fanout: impl FnOnce(),
    ) -> u64 {
        // Sequence allocation and fanout are one critical section. Releasing
        // the bus lock between them allowed a later publisher to reach some
        // subscribers first, with contiguous per-subscription seq values
        // concealing the engine-wide inversion.
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.next_engine_seq = state.next_engine_seq.saturating_add(1);
        let engine_seq = state.next_engine_seq;
        state.subscribers.retain(|_, weak| weak.strong_count() > 0);
        let mut after_first_fanout = Some(after_first_fanout);
        for subscriber in state.subscribers.values().filter_map(Weak::upgrade) {
            if matches!(&audience, Audience::Client(target) if target != &subscriber.client_id) {
                continue;
            }
            subscriber.publish(engine_seq, &event, &self.device_id);
            if let Some(hook) = after_first_fanout.take() {
                hook();
            }
        }
        engine_seq
    }

    fn find_subscriber(&self, client_id: &ClientId) -> Result<Arc<Subscriber>, EventError> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .subscribers
            .get(client_id.as_str())
            .and_then(Weak::upgrade)
            .ok_or_else(|| EventError("The event client session is not connected.".to_owned()))
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::with_device_id("")
    }
}

impl EventReceiver {
    pub async fn recv(&mut self) -> Option<EventDelivery> {
        loop {
            let notified = self.subscriber.notify.notified();
            {
                let mut state = self
                    .subscriber
                    .state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some(gap) = state.gap.take() {
                    return Some(EventDelivery::Gap(gap));
                }
                if let Some(event) = state.queue.pop_front() {
                    let remove_subscription = if let Some(subscription) =
                        state.subscriptions.get_mut(&event.subscription)
                    {
                        subscription.last_delivered = event.seq;
                        subscription.closing && subscription.pending.is_empty()
                    } else {
                        false
                    };
                    if remove_subscription {
                        state.subscriptions.remove(&event.subscription);
                    }
                    return Some(EventDelivery::Event(event));
                }
                if state.closed {
                    return None;
                }
            }
            notified.await;
        }
    }

    pub fn disconnect(self) {
        self.bus.disconnect(&self.client_id);
    }
}

impl Subscriber {
    fn publish(&self, engine_seq: u64, event: &Event, device_id: &str) {
        let payload_bytes =
            serde_json::to_vec(&event.payload).map_or(usize::MAX, |value| value.len());
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.closed {
            return;
        }
        let matching = state
            .subscriptions
            .iter()
            .filter_map(|(id, subscription)| {
                subscription
                    .selectors
                    .iter()
                    .any(|selector| selector_matches(selector, &event.name))
                    .then(|| id.clone())
            })
            .collect::<Vec<_>>();
        for id in matching {
            let prepared = {
                let Some(subscription) = state.subscriptions.get_mut(&id) else {
                    continue;
                };
                let seq = subscription.next_seq;
                subscription.next_seq = subscription.next_seq.saturating_add(1);
                let bytes = encoded_frame_size(event, &id, device_id, seq, payload_bytes);
                let domain = event.name.split('.').next().unwrap_or(&event.name);
                subscription.latest_by_domain.insert(domain.to_owned(), seq);
                let queued_events = subscription.backlog.events.load(Ordering::Acquire);
                let subscription_bytes = subscription.backlog.bytes.load(Ordering::Acquire);
                let session_bytes = self.session_backlog_bytes.load(Ordering::Acquire);
                let overflow = queued_events >= MAX_EVENTS_PER_SUBSCRIPTION
                    || subscription_bytes.saturating_add(bytes) > MAX_BYTES_PER_SUBSCRIPTION
                    || session_bytes.saturating_add(bytes) > MAX_BYTES_PER_SESSION;
                let expected_seq = subscription.last_delivered.saturating_add(1);
                let active = subscription.active;
                if !overflow {
                    subscription.backlog.events.fetch_add(1, Ordering::AcqRel);
                    subscription
                        .backlog
                        .bytes
                        .fetch_add(bytes, Ordering::AcqRel);
                    self.session_backlog_bytes
                        .fetch_add(bytes, Ordering::AcqRel);
                }
                (
                    seq,
                    expected_seq,
                    active,
                    overflow,
                    bytes,
                    subscription.backlog.clone(),
                )
            };
            let (seq, expected_seq, active, overflow, bytes, subscription_backlog) = prepared;
            if overflow {
                let gap = GapSignal {
                    subscription: id,
                    expected_seq,
                    dropped_seq: seq,
                };
                state.queue.clear();
                for subscription in state.subscriptions.values_mut() {
                    subscription.pending.clear();
                }
                state.gap = Some(gap);
                state.closed = true;
                drop(state);
                self.notify.notify_waiters();
                return;
            }
            let delivery = SequencedEvent {
                subscription: id.clone(),
                engine_seq,
                seq,
                event: event.clone(),
                backlog_reservation: BacklogReservation(Arc::new(ReservationLease {
                    subscription: subscription_backlog,
                    session_bytes: self.session_backlog_bytes.clone(),
                    bytes,
                })),
            };
            if active {
                state.queue.push_back(delivery);
            } else {
                state
                    .subscriptions
                    .get_mut(&id)
                    .expect("subscription remains registered")
                    .pending
                    .push_back(delivery);
            }
        }
        drop(state);
        self.notify.notify_one();
    }
}

fn validate_selectors(selectors: &[String]) -> Result<(), EventError> {
    if selectors.is_empty() {
        return Err(EventError(
            "At least one event selector is required.".to_owned(),
        ));
    }
    for selector in selectors {
        let valid = if selector.ends_with(".*") {
            let prefix = &selector[..selector.len() - 1];
            EVENT_NAMES.iter().any(|name| name.starts_with(prefix))
        } else {
            EVENT_NAMES.contains(&selector.as_str())
        };
        if !valid {
            return Err(EventError(format!("Unknown event selector: {selector}")));
        }
    }
    Ok(())
}

fn selector_matches(selector: &str, event: &str) -> bool {
    if let Some(prefix) = selector.strip_suffix('*') {
        event.starts_with(prefix)
    } else {
        selector == event
    }
}

/// Counts the four-byte frame length prefix and encoded JSON event envelope.
fn encoded_frame_size(
    event: &Event,
    subscription: &str,
    device_id: &str,
    seq: u64,
    payload_bytes: usize,
) -> usize {
    FRAME_LENGTH_PREFIX_BYTES.saturating_add(
        [
            b"{\"deviceId\":".len(),
            json_string_size(device_id),
            b",\"event\":".len(),
            json_string_size(&event.name),
            b",\"payload\":".len(),
            payload_bytes,
            b",\"seq\":".len(),
            seq.to_string().len(),
            b",\"subscription\":".len(),
            json_string_size(subscription),
            b"}".len(),
        ]
        .into_iter()
        .fold(0, usize::saturating_add),
    )
}

fn json_string_size(value: &str) -> usize {
    serde_json::to_vec(value).map_or(usize::MAX, |encoded| encoded.len())
}

/// Builds the JSON event envelope that `silod` validates and sends to clients.
pub fn wire_event_envelope(
    event: &Event,
    subscription: &str,
    device_id: &str,
    seq: u64,
) -> serde_json::Value {
    serde_json::json!({
        "event": &event.name,
        "subscription": subscription,
        "deviceId": device_id,
        "seq": seq,
        "payload": &event.payload,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client() -> ClientId {
        ClientId::new("client-a")
    }

    #[tokio::test]
    async fn topics_are_filtered_and_subscription_sequences_are_contiguous() {
        let bus = EventBus::default();
        let client = client();
        let mut receiver = bus.connect(client.clone());
        let subscription = bus
            .subscribe(&client, vec!["computers.*".to_owned()])
            .unwrap();
        bus.publish(
            Event::new("settings.changed", serde_json::json!({"revision": 1})),
            Audience::All,
        );
        bus.publish(
            Event::new("computers.changed", serde_json::json!({"revision": 2})),
            Audience::All,
        );
        bus.activate(&client, &subscription).unwrap();
        let Some(EventDelivery::Event(event)) = receiver.recv().await else {
            panic!("expected a matching event");
        };
        assert_eq!(event.subscription, subscription);
        assert_eq!(event.seq, 1);
        assert_eq!(event.engine_seq, 2);
        assert!(receiver.subscriber.state.lock().unwrap().queue.is_empty());
    }

    #[tokio::test]
    async fn requester_only_events_reach_the_matching_client() {
        let bus = EventBus::default();
        let selected = client();
        let other = ClientId::new("client-b");
        let mut selected_events = bus.connect(selected.clone());
        let other_events = bus.connect(other.clone());
        let selected_sub = bus
            .subscribe(&selected, vec!["operations.job.progress".to_owned()])
            .unwrap();
        let other_sub = bus
            .subscribe(&other, vec!["operations.job.progress".to_owned()])
            .unwrap();
        bus.activate(&selected, &selected_sub).unwrap();
        bus.activate(&other, &other_sub).unwrap();
        bus.publish(
            Event::new("operations.job.progress", serde_json::json!({"step": 1})),
            Audience::Client(selected.clone()),
        );
        assert!(matches!(
            selected_events.recv().await,
            Some(EventDelivery::Event(_))
        ));
        assert!(other_events
            .subscriber
            .state
            .lock()
            .unwrap()
            .queue
            .is_empty());
    }

    #[tokio::test]
    async fn engine_sequence_and_fanout_share_one_serial_order() {
        let bus = EventBus::default();
        let first_client = client();
        let second_client = ClientId::new("client-b");
        let mut first = bus.connect(first_client.clone());
        let mut second = bus.connect(second_client.clone());
        for client_id in [&first_client, &second_client] {
            let subscription = bus
                .subscribe(client_id, vec!["computers.changed".to_owned()])
                .unwrap();
            bus.activate(client_id, &subscription).unwrap();
        }

        let nested_bus = bus.clone();
        let nested_published = std::cell::Cell::new(false);
        bus.publish_with_fanout_hook(
            Event::new("computers.changed", serde_json::json!({"value": 1})),
            Audience::All,
            || {
                // Before the fix, sequence allocation released the bus mutex
                // before fanout. This hook then publishes sequence 2 after
                // the first subscriber but before the second one.
                if nested_bus.inner.try_lock().is_ok() {
                    nested_published.set(true);
                    nested_bus.publish(
                        Event::new("computers.changed", serde_json::json!({"value": 2})),
                        Audience::All,
                    );
                }
            },
        );
        if !nested_published.get() {
            bus.publish(
                Event::new("computers.changed", serde_json::json!({"value": 2})),
                Audience::All,
            );
        }

        let first_order = [first.recv().await.unwrap(), first.recv().await.unwrap()];
        let second_order = [second.recv().await.unwrap(), second.recv().await.unwrap()];
        let engine_sequences = |deliveries: &[EventDelivery; 2]| {
            deliveries
                .iter()
                .map(|delivery| match delivery {
                    EventDelivery::Event(event) => event.engine_seq,
                    EventDelivery::Gap(_) => panic!("unexpected event gap"),
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(engine_sequences(&first_order), vec![1, 2]);
        assert_eq!(engine_sequences(&second_order), vec![1, 2]);
    }

    #[test]
    fn operations_barrier_tracks_matching_events_per_subscription() {
        let bus = EventBus::default();
        let client = client();
        let _receiver = bus.connect(client.clone());
        let subscription = bus
            .subscribe(&client, vec!["operations.*".to_owned()])
            .unwrap();
        bus.activate(&client, &subscription).unwrap();
        bus.publish(
            Event::new(
                "operations.job.progress",
                serde_json::json!({"progress": 1}),
            ),
            Audience::All,
        );
        assert_eq!(
            bus.operations_barrier(&client).unwrap().get(&subscription),
            Some(&1)
        );
        bus.publish(
            Event::new("computers.changed", serde_json::json!({"revision": 1})),
            Audience::All,
        );
        bus.publish(
            Event::new(
                "operations.queue.changed",
                serde_json::json!({"running": []}),
            ),
            Audience::All,
        );
        assert_eq!(
            bus.operations_barrier(&client).unwrap().get(&subscription),
            Some(&2)
        );
    }

    #[tokio::test]
    async fn unsubscribe_keeps_previously_queued_events_for_its_barrier() {
        let bus = EventBus::default();
        let client = client();
        let mut receiver = bus.connect(client.clone());
        let subscription = bus
            .subscribe(&client, vec!["operations.*".to_owned()])
            .unwrap();
        bus.activate(&client, &subscription).unwrap();
        bus.publish(
            Event::new(
                "operations.job.finished",
                serde_json::json!({"jobId": "job:1"}),
            ),
            Audience::All,
        );

        let barrier = bus.unsubscribe(&client, &subscription);
        assert_eq!(barrier.get(&subscription), Some(&1));
        assert!(matches!(
            receiver.recv().await,
            Some(EventDelivery::Event(SequencedEvent { seq: 1, .. }))
        ));
        assert!(receiver
            .subscriber
            .state
            .lock()
            .unwrap()
            .subscriptions
            .is_empty());
    }

    #[tokio::test]
    async fn overflow_signals_a_gap_and_a_fresh_subscription_resyncs_at_one() {
        let bus = EventBus::default();
        let client = client();
        let mut receiver = bus.connect(client.clone());
        let subscription = bus
            .subscribe(&client, vec!["computers.changed".to_owned()])
            .unwrap();
        bus.activate(&client, &subscription).unwrap();
        for index in 0..=MAX_EVENTS_PER_SUBSCRIPTION {
            bus.publish(
                Event::new("computers.changed", serde_json::json!({"revision": index})),
                Audience::All,
            );
        }
        assert!(matches!(
            receiver.recv().await,
            Some(EventDelivery::Gap(GapSignal {
                dropped_seq: 257,
                ..
            }))
        ));
        assert!(receiver.recv().await.is_none());

        bus.disconnect(&client);
        let mut resynced = bus.connect(client.clone());
        let fresh = bus
            .subscribe(&client, vec!["computers.changed".to_owned()])
            .unwrap();
        bus.activate(&client, &fresh).unwrap();
        bus.publish(
            Event::new("computers.changed", serde_json::json!({"revision": 300})),
            Audience::All,
        );
        assert!(matches!(
            resynced.recv().await,
            Some(EventDelivery::Event(SequencedEvent { seq: 1, .. }))
        ));
        assert_ne!(subscription, fresh);
    }

    #[tokio::test]
    async fn an_over_budget_event_closes_the_session_without_waiting_for_another_publish() {
        let bus = EventBus::default();
        let client = client();
        let mut receiver = bus.connect(client.clone());
        let subscription = bus
            .subscribe(&client, vec!["computers.changed".to_owned()])
            .unwrap();
        bus.activate(&client, &subscription).unwrap();
        bus.publish(
            Event::new(
                "computers.changed",
                serde_json::json!({"snapshot": "x".repeat(MAX_BYTES_PER_SUBSCRIPTION)}),
            ),
            Audience::All,
        );

        assert!(matches!(
            receiver.recv().await,
            Some(EventDelivery::Gap(GapSignal {
                expected_seq: 1,
                dropped_seq: 1,
                ..
            }))
        ));
        assert!(receiver.recv().await.is_none());
    }

    #[tokio::test]
    async fn subscription_event_limit_includes_events_already_taken_by_the_consumer() {
        let bus = EventBus::default();
        let client = client();
        let mut receiver = bus.connect(client.clone());
        let subscription = bus
            .subscribe(&client, vec!["computers.changed".to_owned()])
            .unwrap();
        bus.activate(&client, &subscription).unwrap();
        bus.publish(
            Event::new("computers.changed", serde_json::json!({"revision": 0})),
            Audience::All,
        );
        let in_flight = receiver.recv().await.unwrap();
        for revision in 1..MAX_EVENTS_PER_SUBSCRIPTION {
            bus.publish(
                Event::new(
                    "computers.changed",
                    serde_json::json!({"revision": revision}),
                ),
                Audience::All,
            );
        }

        bus.publish(
            Event::new(
                "computers.changed",
                serde_json::json!({"revision": MAX_EVENTS_PER_SUBSCRIPTION}),
            ),
            Audience::All,
        );
        assert!(matches!(receiver.recv().await, Some(EventDelivery::Gap(_))));
        drop(in_flight);
    }

    #[tokio::test]
    async fn session_byte_limit_includes_events_already_taken_by_other_subscriptions() {
        let bus = EventBus::default();
        let client = client();
        let mut receiver = bus.connect(client.clone());
        let topics = ["computers.changed", "settings.changed", "updates.changed"];
        for topic in topics {
            let subscription = bus.subscribe(&client, vec![topic.to_owned()]).unwrap();
            bus.activate(&client, &subscription).unwrap();
        }
        let payload = || serde_json::json!({"data": "x".repeat(6 * 1024 * 1024)});
        let mut in_flight = Vec::new();
        for topic in &topics[..2] {
            bus.publish(Event::new(*topic, payload()), Audience::All);
            in_flight.push(receiver.recv().await.unwrap());
        }
        bus.publish(Event::new(topics[2], payload()), Audience::All);

        assert!(matches!(receiver.recv().await, Some(EventDelivery::Gap(_))));
        drop(in_flight);
    }

    #[test]
    fn rejects_unknown_selectors_instead_of_silently_subscribing() {
        assert!(validate_selectors(&["not.real".to_owned()]).is_err());
        assert!(validate_selectors(&["operations.*".to_owned()]).is_ok());
    }

    #[tokio::test]
    async fn backlog_byte_limit_counts_the_encoded_wire_envelope_exactly() {
        let device_id = "01234567-89ab-cdef-0123-456789abcdef";
        let bus = EventBus::with_device_id(device_id);
        let client = client();
        let mut receiver = bus.connect(client.clone());
        let subscription = bus
            .subscribe(&client, vec!["checkpoints.changed".to_owned()])
            .unwrap();
        bus.activate(&client, &subscription).unwrap();

        let empty = Event::new("checkpoints.changed", serde_json::json!({"blob": ""}));
        let empty_frame_bytes = FRAME_LENGTH_PREFIX_BYTES
            + serde_json::to_vec(&wire_event_envelope(&empty, &subscription, device_id, 1))
                .unwrap()
                .len();
        let payload_len = MAX_BYTES_PER_SUBSCRIPTION - empty_frame_bytes;
        let at_limit = Event::new(
            "checkpoints.changed",
            serde_json::json!({"blob": "x".repeat(payload_len)}),
        );
        let at_limit_bytes = FRAME_LENGTH_PREFIX_BYTES
            + serde_json::to_vec(&wire_event_envelope(&at_limit, &subscription, device_id, 1))
                .unwrap()
                .len();
        assert_eq!(at_limit_bytes, MAX_BYTES_PER_SUBSCRIPTION);
        let payload_bytes = serde_json::to_vec(&at_limit.payload).unwrap().len();
        assert_eq!(
            encoded_frame_size(&at_limit, &subscription, device_id, 1, payload_bytes),
            at_limit_bytes
        );

        bus.publish(at_limit, Audience::All);
        let Some(EventDelivery::Event(delivery)) = receiver.recv().await else {
            panic!("an envelope exactly at the byte limit must be delivered");
        };
        assert_eq!(delivery.backlog_reservation.0.bytes, at_limit_bytes);
        drop(delivery);

        let over_limit = Event::new(
            "checkpoints.changed",
            serde_json::json!({"blob": "x".repeat(payload_len + 1)}),
        );
        let over_limit_bytes = FRAME_LENGTH_PREFIX_BYTES
            + serde_json::to_vec(&wire_event_envelope(
                &over_limit,
                &subscription,
                device_id,
                2,
            ))
            .unwrap()
            .len();
        assert_eq!(over_limit_bytes, MAX_BYTES_PER_SUBSCRIPTION + 1);
        bus.publish(over_limit, Audience::All);
        assert!(matches!(receiver.recv().await, Some(EventDelivery::Gap(_))));
    }

    #[test]
    fn encoded_wire_size_tracks_json_escaping_and_sequence_digits() {
        let event = Event::new(
            "checkpoints.changed",
            serde_json::json!({"message": "quotes: \\\" and unicode: café"}),
        );
        let subscription = "subscription-\\\"-id";
        let device_id = "device-\\\"-id";
        let payload_bytes = serde_json::to_vec(&event.payload).unwrap().len();
        for seq in [1, 9, 10, 10_000_000_000_000_000] {
            assert_eq!(
                encoded_frame_size(&event, subscription, device_id, seq, payload_bytes),
                FRAME_LENGTH_PREFIX_BYTES
                    + serde_json::to_vec(&wire_event_envelope(
                        &event,
                        subscription,
                        device_id,
                        seq,
                    ))
                    .unwrap()
                    .len()
            );
        }
    }
}
