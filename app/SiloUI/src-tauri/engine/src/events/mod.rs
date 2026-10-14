//! Bounded engine event fanout with per-subscription sequences.

use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex, Weak},
};

use silo_api::events::EVENT_NAMES;
use tokio::sync::Notify;
use uuid::Uuid;

use crate::context::{Audience, ClientId, Event};

pub const MAX_EVENTS_PER_SUBSCRIPTION: usize = 256;
pub const MAX_BYTES_PER_SUBSCRIPTION: usize = 8 * 1024 * 1024;
pub const MAX_BYTES_PER_SESSION: usize = 16 * 1024 * 1024;
pub const MAX_SUBSCRIPTIONS_PER_SESSION: usize = 16;

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
}

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

#[derive(Clone, Default)]
pub struct EventBus {
    inner: Arc<Mutex<BusState>>,
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
}

#[derive(Default)]
struct SubscriberState {
    subscriptions: HashMap<String, SubscriptionState>,
    queue: VecDeque<SequencedEvent>,
    queued_bytes: usize,
    gap: Option<GapSignal>,
    closed: bool,
}

struct SubscriptionState {
    selectors: Vec<String>,
    active: bool,
    closing: bool,
    next_seq: u64,
    latest_operations_seq: u64,
    last_delivered: u64,
    queued_events: usize,
    queued_bytes: usize,
    pending: VecDeque<SequencedEvent>,
}

pub struct EventReceiver {
    bus: EventBus,
    client_id: ClientId,
    subscriber: Arc<Subscriber>,
}

impl EventBus {
    pub fn connect(&self, client_id: ClientId) -> EventReceiver {
        let subscriber = Arc::new(Subscriber {
            client_id: client_id.clone(),
            state: Mutex::new(SubscriberState::default()),
            notify: Notify::new(),
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
            return Err(EventError("The event session has too many subscriptions.".to_owned()));
        }
        state.subscriptions.insert(
            id.clone(),
            SubscriptionState {
                selectors,
                active: false,
                closing: false,
                next_seq: 1,
                latest_operations_seq: 0,
                last_delivered: 0,
                queued_events: 0,
                queued_bytes: 0,
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
                return Err(EventError("The event subscription no longer exists.".to_owned()));
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
        let removable = state.subscriptions.get(subscription).is_some_and(|subscription| {
            subscription.queued_events == 0 && subscription.pending.is_empty()
        });
        if removable {
            state.subscriptions.remove(subscription);
        }
        drop(state);
        subscriber.notify.notify_one();
        barrier
    }

    pub fn disconnect(&self, client_id: &ClientId) {
        if let Ok(subscriber) = self.find_subscriber(client_id) {
            let mut state = subscriber
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.closed = true;
            state.subscriptions.clear();
            state.queue.clear();
            state.queued_bytes = 0;
            drop(state);
            subscriber.notify.notify_waiters();
        }
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .subscribers
            .remove(client_id.as_str());
    }

    /// Captures each matching operations subscription's last sequence while a
    /// caller holds the corresponding published-projection lock.
    pub fn operations_barrier(
        &self,
        client_id: &ClientId,
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
                (subscription.latest_operations_seq > 0)
                    .then(|| (id.clone(), subscription.latest_operations_seq))
            })
            .collect())
    }

    pub fn publish(&self, event: Event, audience: Audience) -> u64 {
        let (engine_seq, subscribers) = {
            let mut state = self
                .inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.next_engine_seq = state.next_engine_seq.saturating_add(1);
            let seq = state.next_engine_seq;
            state.subscribers.retain(|_, weak| weak.strong_count() > 0);
            let subscribers = state
                .subscribers
                .values()
                .filter_map(Weak::upgrade)
                .collect::<Vec<_>>();
            (seq, subscribers)
        };
        for subscriber in subscribers {
            if matches!(&audience, Audience::Client(target) if target != &subscriber.client_id) {
                continue;
            }
            subscriber.publish(engine_seq, &event);
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
                    state.queued_bytes = state.queued_bytes.saturating_sub(event_size(&event.event));
                    let remove_subscription = if let Some(subscription) =
                        state.subscriptions.get_mut(&event.subscription)
                    {
                        subscription.queued_events = subscription.queued_events.saturating_sub(1);
                        subscription.queued_bytes =
                            subscription.queued_bytes.saturating_sub(event_size(&event.event));
                        subscription.last_delivered = event.seq;
                        subscription.closing
                            && subscription.queued_events == 0
                            && subscription.pending.is_empty()
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
    fn publish(&self, engine_seq: u64, event: &Event) {
        let bytes = event_size(event);
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
            let queued_bytes = state.queued_bytes;
            let prepared = {
                let Some(subscription) = state.subscriptions.get_mut(&id) else {
                    continue;
                };
                let seq = subscription.next_seq;
                subscription.next_seq = subscription.next_seq.saturating_add(1);
                if event.name.starts_with("operations.") {
                    subscription.latest_operations_seq = seq;
                }
                let overflow = subscription.queued_events >= MAX_EVENTS_PER_SUBSCRIPTION
                    || subscription.queued_bytes.saturating_add(bytes)
                        > MAX_BYTES_PER_SUBSCRIPTION
                    || queued_bytes.saturating_add(bytes) > MAX_BYTES_PER_SESSION;
                let expected_seq = subscription.last_delivered.saturating_add(1);
                let active = subscription.active;
                if !overflow {
                    subscription.queued_events += 1;
                    subscription.queued_bytes += bytes;
                }
                (seq, expected_seq, active, overflow)
            };
            let (seq, expected_seq, active, overflow) = prepared;
            if overflow {
                let gap = GapSignal {
                    subscription: id,
                    expected_seq,
                    dropped_seq: seq,
                };
                state.queue.clear();
                state.queued_bytes = 0;
                for subscription in state.subscriptions.values_mut() {
                    subscription.pending.clear();
                    subscription.queued_bytes = 0;
                    subscription.queued_events = 0;
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
            };
            state.queued_bytes += bytes;
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
        return Err(EventError("At least one event selector is required.".to_owned()));
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

fn event_size(event: &Event) -> usize {
    event.name.len() + serde_json::to_vec(&event.payload).map_or(0, |value| value.len()) + 128
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
        assert!(matches!(selected_events.recv().await, Some(EventDelivery::Event(_))));
        assert!(other_events.subscriber.state.lock().unwrap().queue.is_empty());
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
            Event::new("operations.job.progress", serde_json::json!({"progress": 1})),
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
            Event::new("operations.queue.changed", serde_json::json!({"running": []})),
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
            Event::new("operations.job.finished", serde_json::json!({"jobId": "job:1"})),
            Audience::All,
        );

        let barrier = bus.unsubscribe(&client, &subscription);
        assert_eq!(barrier.get(&subscription), Some(&1));
        assert!(matches!(
            receiver.recv().await,
            Some(EventDelivery::Event(SequencedEvent { seq: 1, .. }))
        ));
        assert!(receiver.subscriber.state.lock().unwrap().subscriptions.is_empty());
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
            Some(EventDelivery::Gap(GapSignal { dropped_seq: 257, .. }))
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

    #[test]
    fn rejects_unknown_selectors_instead_of_silently_subscribing() {
        assert!(validate_selectors(&["not.real".to_owned()]).is_err());
        assert!(validate_selectors(&["operations.*".to_owned()]).is_ok());
    }
}
