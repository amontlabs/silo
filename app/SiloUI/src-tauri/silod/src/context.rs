use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use silo_api::methods::engine::{EngineState, ShutdownPhase};
use silo_engine::context::{
    Audience, ClientId, EngineContext, EngineIdentity, EnginePaths, EngineRuntimeState, Event,
    MainThread, ProcessControl,
};
use tokio::sync::mpsc;

use crate::identity::LocalDeviceIdentity;

const MAX_EVENTS_PER_SUBSCRIPTION: usize = 256;

#[derive(Clone)]
pub struct SilodContext {
    paths: EnginePaths,
    identity: EngineIdentity,
    runtime_state: Arc<EngineRuntimeState>,
    local_device: LocalDeviceIdentity,
    events: EventSubscriptions,
    main_thread: Arc<dyn MainThread>,
    process: Arc<dyn ProcessControl>,
    state: Arc<Mutex<EngineState>>,
}

impl SilodContext {
    pub fn new(
        paths: EnginePaths,
        identity: EngineIdentity,
        local_device: LocalDeviceIdentity,
        main_thread: Arc<dyn MainThread>,
        process: Arc<dyn ProcessControl>,
    ) -> Self {
        Self {
            paths,
            identity,
            runtime_state: Arc::new(EngineRuntimeState::default()),
            local_device,
            events: EventSubscriptions::default(),
            main_thread,
            process,
            state: Arc::new(Mutex::new(EngineState {
                shutting_down: false,
                phase: ShutdownPhase::Idle,
                generation: 0,
                session_ending: false,
                update_pending: false,
            })),
        }
    }

    pub fn local_device(&self) -> &LocalDeviceIdentity {
        &self.local_device
    }

    pub fn engine_state(&self) -> EngineState {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Registers an in-process destination for events belonging to one local client.
    /// The future `events.subscribe` method will add wire-level filters and envelopes.
    pub fn subscribe(&self, client_id: ClientId) -> mpsc::Receiver<Event> {
        self.events.subscribe(client_id)
    }
}

impl EngineContext for SilodContext {
    fn paths(&self) -> &EnginePaths {
        &self.paths
    }

    fn identity(&self) -> &EngineIdentity {
        &self.identity
    }

    fn runtime_state(&self) -> Arc<EngineRuntimeState> {
        self.runtime_state.clone()
    }

    fn publish(&self, event: Event, audience: Audience) {
        self.events.publish(event, audience);
    }

    fn main_thread(&self) -> Option<&dyn MainThread> {
        Some(self.main_thread.as_ref())
    }

    fn process(&self) -> &dyn ProcessControl {
        self.process.as_ref()
    }
}

#[derive(Clone, Default)]
struct EventSubscriptions {
    clients: Arc<Mutex<HashMap<ClientId, Vec<mpsc::Sender<Event>>>>>,
}

impl EventSubscriptions {
    fn subscribe(&self, client_id: ClientId) -> mpsc::Receiver<Event> {
        let (sender, receiver) = mpsc::channel(MAX_EVENTS_PER_SUBSCRIPTION);
        self.clients
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entry(client_id)
            .or_default()
            .push(sender);
        receiver
    }

    fn publish(&self, event: Event, audience: Audience) {
        let mut clients = self
            .clients
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match audience {
            Audience::All => {
                clients.retain(|_, senders| {
                    senders.retain(|sender| sender.try_send(event.clone()).is_ok());
                    !senders.is_empty()
                });
            }
            Audience::Client(client_id) => {
                if let Some(senders) = clients.get_mut(&client_id) {
                    senders.retain(|sender| sender.try_send(event.clone()).is_ok());
                    if senders.is_empty() {
                        clients.remove(&client_id);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use silo_engine::context::{ClientId, Event};

    #[test]
    fn publish_reaches_only_the_matching_subscribed_client() {
        let subscriptions = EventSubscriptions::default();
        let selected = ClientId::new("client-a");
        let other = ClientId::new("client-b");
        let mut selected_events = subscriptions.subscribe(selected.clone());
        let mut other_events = subscriptions.subscribe(other);

        subscriptions.publish(
            Event::new("test.changed", serde_json::json!({"value": 1})),
            Audience::Client(selected),
        );

        assert_eq!(selected_events.try_recv().unwrap().name, "test.changed");
        assert!(other_events.try_recv().is_err());
    }

    #[test]
    fn a_full_subscription_is_removed_without_blocking_publication() {
        let subscriptions = EventSubscriptions::default();
        let client = ClientId::new("slow-client");
        let receiver = subscriptions.subscribe(client.clone());

        for sequence in 0..=MAX_EVENTS_PER_SUBSCRIPTION {
            subscriptions.publish(
                Event::new("test.changed", serde_json::json!({"sequence": sequence})),
                Audience::Client(client.clone()),
            );
        }

        assert!(!subscriptions
            .clients
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains_key(&client));
        drop(receiver);
    }
}
