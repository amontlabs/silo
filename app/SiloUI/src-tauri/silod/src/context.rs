use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use silo_api::methods::engine::{EngineState, ShutdownPhase};
use silo_api::methods::notifications::{Notice, NotificationsClearedPayload};
use silo_engine::context::{
    Audience, ClientId, EngineContext, EngineIdentity, EnginePaths, EngineRuntimeState, Event,
    MainThread, ProcessControl,
};
use silo_engine::events::{EventBus, EventReceiver};
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
    wire_events: EventBus,
    main_thread: Arc<dyn MainThread>,
    process: Arc<dyn ProcessControl>,
    migration: Arc<silo_engine::runtime_migration::Controller>,
    backup: Arc<Mutex<Option<Arc<silo_engine::backup_controller::Controller>>>>,
    state: Arc<Mutex<EngineState>>,
    queue_projection: Arc<Mutex<()>>,
    notice_projection: Arc<Mutex<()>>,
}

impl SilodContext {
    pub fn new(
        paths: EnginePaths,
        identity: EngineIdentity,
        local_device: LocalDeviceIdentity,
        main_thread: Arc<dyn MainThread>,
        process: Arc<dyn ProcessControl>,
    ) -> Self {
        let wire_events = EventBus::with_device_id(local_device.device_id.clone());
        let migration = Arc::new(silo_engine::runtime_migration::Controller::new(
            paths.app_data.clone(),
        ));
        Self {
            paths,
            identity,
            runtime_state: Arc::new(EngineRuntimeState::default()),
            local_device,
            events: EventSubscriptions::default(),
            wire_events,
            main_thread,
            process,
            migration,
            backup: Arc::new(Mutex::new(None)),
            state: Arc::new(Mutex::new(EngineState {
                shutting_down: false,
                phase: ShutdownPhase::Idle,
                generation: 0,
                session_ending: false,
                update_pending: false,
            })),
            queue_projection: Arc::new(Mutex::new(())),
            notice_projection: Arc::new(Mutex::new(())),
        }
    }

    pub fn local_device(&self) -> &LocalDeviceIdentity {
        &self.local_device
    }

    pub fn migration_controller(&self) -> Arc<silo_engine::runtime_migration::Controller> {
        self.migration.clone()
    }

    pub fn runtime_host(&self) -> Arc<dyn silo_engine::runtime::RuntimeHost> {
        Arc::new(SilodRuntimeHost {
            context: Arc::new(self.clone()),
            migration: self.migration.clone(),
        })
    }

    /// The macOS computer adapter for work that opens no display: checkpoints, and export
    /// and import through the runtime host.
    pub fn macos_context(&self) -> silo_engine::macos_computers_engine::MacosContext {
        silo_engine::macos_computers_engine::MacosContext::new(
            Arc::new(self.clone()),
            Arc::new(NoDisplayMacosHost {
                host: self.runtime_host(),
            }),
        )
    }

    pub fn backup_controller(
        &self,
    ) -> Result<Arc<silo_engine::backup_controller::Controller>, String> {
        let runtime = tokio::runtime::Handle::try_current()
            .map_err(|_| "Backup runtime services are unavailable.".to_owned())?;
        let mut backup = self.backup.lock().map_err(|_| {
            "Export and import status could not be read. Relaunch Silo and retry.".to_owned()
        })?;
        if let Some(controller) = backup.as_ref() {
            return Ok(controller.clone());
        }
        let controller = silo_engine::backup_controller::install(self.runtime_host(), runtime)?;
        *backup = Some(controller.clone());
        Ok(controller)
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

    pub fn connect_event_stream(&self, client_id: ClientId) -> EventReceiver {
        self.wire_events.connect(client_id)
    }

    pub fn event_bus(&self) -> &EventBus {
        &self.wire_events
    }

    pub fn subscribe_events(
        &self,
        client_id: &ClientId,
        selectors: Vec<String>,
    ) -> Result<String, String> {
        self.wire_events
            .subscribe(client_id, selectors)
            .map_err(|error| error.to_string())
    }

    pub fn activate_events(&self, client_id: &ClientId, subscription: &str) -> Result<(), String> {
        self.wire_events
            .activate(client_id, subscription)
            .map_err(|error| error.to_string())
    }

    pub fn unsubscribe_events(
        &self,
        client_id: &ClientId,
        subscription: &str,
    ) -> HashMap<String, u64> {
        self.wire_events.unsubscribe(client_id, subscription)
    }

    pub fn disconnect_events(&self, client_id: &ClientId) {
        self.wire_events.disconnect(client_id);
    }

    pub fn install_queue_events(&self) {
        let context = self.clone();
        silo_engine::runtime::OPERATIONS.set_listener(move || context.publish_operation_queue());
    }

    pub fn operation_queue_snapshot(
        &self,
        client_id: &ClientId,
    ) -> Result<OperationQueueSnapshot, String> {
        let _projection = self
            .queue_projection
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let queue = self.operation_queue_unlocked()?;
        let event_barrier = self
            .wire_events
            .operations_barrier(client_id)
            .map_err(|error| error.to_string())?;
        Ok(OperationQueueSnapshot {
            queue,
            event_barrier,
        })
    }

    pub fn notifications_snapshot(
        &self,
        client_id: &ClientId,
    ) -> Result<NotificationsSnapshot, String> {
        self.notifications_snapshot_with_hook(client_id, || {})
    }

    fn notifications_snapshot_with_hook(
        &self,
        client_id: &ClientId,
        after_list: impl FnOnce(),
    ) -> Result<NotificationsSnapshot, String> {
        let _projection = self
            .notice_projection
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let notices = silo_engine::notices::list(self)?;
        after_list();
        let event_barrier = self
            .wire_events
            .domain_barrier(client_id, "notifications")
            .map_err(|error| error.to_string())?;
        Ok(NotificationsSnapshot {
            notices,
            event_barrier,
        })
    }

    fn operation_queue_unlocked(
        &self,
    ) -> Result<silo_api::methods::operations::OperationQueue, String> {
        let registry = self.job_registry()?;
        let mut payload = serde_json::to_value(silo_engine::runtime::OPERATIONS.snapshot())
            .map_err(|error| error.to_string())?;
        for key in ["running", "waiting"] {
            if let Some(entries) = payload
                .get_mut(key)
                .and_then(serde_json::Value::as_array_mut)
            {
                for entry in entries {
                    let Some(gate_id) = entry.get("id").and_then(serde_json::Value::as_u64) else {
                        continue;
                    };
                    if let Some(job_id) = registry.job_for_gate(gate_id) {
                        entry["jobId"] = serde_json::json!(job_id.as_str());
                    }
                }
            }
        }
        payload["unacknowledgedJobIds"] = serde_json::to_value(
            registry
                .list_unacknowledged()
                .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        serde_json::from_value(payload).map_err(|error| error.to_string())
    }

    fn publish_operation_queue(&self) {
        let _projection = self
            .queue_projection
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Ok(queue) = self.operation_queue_unlocked() else {
            return;
        };
        let Ok(payload) = serde_json::to_value(queue) else {
            return;
        };
        self.publish_unlocked(
            Event::new("operations.queue.changed", payload),
            Audience::All,
        );
    }

    fn publish_unlocked(&self, event: Event, audience: Audience) {
        self.wire_events.publish(event.clone(), audience.clone());
        self.events.publish(event, audience);
    }

    fn retain_notice_with_hook(
        &self,
        notice: &Notice,
        audience: Audience,
        after_upsert: impl FnOnce(),
    ) -> Result<(), String> {
        let _projection = self
            .notice_projection
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.job_registry()?
            .upsert_notice(notice)
            .map_err(|error| error.to_string())?;
        after_upsert();
        let payload = serde_json::to_value(notice).map_err(|error| error.to_string())?;
        self.publish_unlocked(Event::new("notifications.notice", payload), audience);
        Ok(())
    }

    fn clear_notices_unlocked(&self, computer_id: &str, audience: Audience) -> Result<(), String> {
        self.job_registry()?
            .clear_notices_for_computer(computer_id)
            .map_err(|error| error.to_string())?;
        let payload = serde_json::to_value(NotificationsClearedPayload {
            computer_id: computer_id.to_owned(),
        })
        .map_err(|error| error.to_string())?;
        self.publish_unlocked(Event::new("notifications.cleared", payload), audience);
        Ok(())
    }
}

/// The macOS side reaches no display window from checkpoint, export or import work; the
/// Linux names it reserves fork names against come from this device's inventory.
struct NoDisplayMacosHost {
    host: Arc<dyn silo_engine::runtime::RuntimeHost>,
}

impl silo_engine::macos_computers_engine::MacosHost for NoDisplayMacosHost {
    fn show_display(
        &self,
        _context: &silo_engine::macos_computers_engine::MacosContext,
        _id: &str,
        _title: &str,
        _toolbar: bool,
    ) -> Result<(), String> {
        Err("This operation does not open a display.".to_owned())
    }

    fn close_display(&self, _id: &str) {}

    fn linux_computer_names(&self) -> Vec<String> {
        silo_engine::runtime::computer_names(self.host.as_ref())
    }
}

struct SilodRuntimeHost {
    context: Arc<SilodContext>,
    migration: Arc<silo_engine::runtime_migration::Controller>,
}

impl silo_engine::runtime::RuntimeHost for SilodRuntimeHost {
    fn engine_context(&self) -> Arc<dyn EngineContext> {
        self.context.clone()
    }

    fn macos_context(&self) -> Option<silo_engine::macos_computers_engine::MacosContext> {
        Some(self.context.macos_context())
    }

    fn runtime_storage(&self) -> Result<PathBuf, String> {
        self.migration.runtime_storage()
    }

    fn blocks_operations(&self) -> bool {
        self.migration.blocks_operations()
    }

    fn bundled_tools_directory(&self) -> Result<PathBuf, String> {
        let executable = std::env::current_exe()
            .map_err(|_| "Silo could not locate its bundled tools.".to_owned())?;
        silo_engine::bundled_tools::resolve(
            &executable,
            &self.context.paths.resources,
            Some(silo_engine::bundled_tools::BundleKind::App),
            None,
        )
    }

    fn ensure_runtime_ready(&self) -> Result<(), String> {
        self.migration.ensure_ready()
    }

    fn runtime_package_kind(&self) -> silo_engine::runtime::RuntimePackageKind {
        silo_engine::runtime::RuntimePackageKind::MacApp
    }
}

#[derive(Clone, Debug)]
pub struct OperationQueueSnapshot {
    pub queue: silo_api::methods::operations::OperationQueue,
    pub event_barrier: HashMap<String, u64>,
}

#[derive(Clone, Debug)]
pub struct NotificationsSnapshot {
    pub notices: Vec<Notice>,
    pub event_barrier: HashMap<String, u64>,
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
        if event.name.starts_with("operations.") {
            let _projection = self
                .queue_projection
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            self.publish_unlocked(event, audience);
        } else if event.name.starts_with("notifications.") {
            let _projection = self
                .notice_projection
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            self.publish_unlocked(event, audience);
        } else {
            self.publish_unlocked(event, audience);
        }
    }

    fn retain_notice(&self, notice: &Notice, audience: Audience) -> Result<(), String> {
        self.retain_notice_with_hook(notice, audience, || {})
    }

    fn clear_notices_for_computer(
        &self,
        computer_id: &str,
        audience: Audience,
    ) -> Result<(), String> {
        let _projection = self
            .notice_projection
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.clear_notices_unlocked(computer_id, audience)
    }

    fn job_queue_changed(&self) {
        self.publish_operation_queue();
    }

    fn main_thread(&self) -> Option<&dyn MainThread> {
        Some(self.main_thread.as_ref())
    }

    fn process(&self) -> &dyn ProcessControl {
        self.process.as_ref()
    }

    fn close_remote_displays(&self, device: Option<&str>) {
        match device {
            Some(device) => silo_engine::desktop_viewer_api::close_device(device),
            None => silo_engine::desktop_viewer_api::close_all(),
        }
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
    use silo_engine::context::{ClientId, Event, MainThread, ProcessControl};
    use silo_engine::events::EventDelivery;

    struct NoopMainThread;
    impl MainThread for NoopMainThread {
        fn run(&self, _work: Box<dyn FnOnce() + Send>) -> Result<(), String> {
            Ok(())
        }
    }

    struct NoopProcess;
    impl ProcessControl for NoopProcess {
        fn exit(&self, _code: i32) {}

        fn restart(&self) -> ! {
            panic!("restart is not expected in context tests")
        }
    }

    fn test_context(home: &std::path::Path) -> SilodContext {
        SilodContext::new(
            EnginePaths {
                home: home.to_path_buf(),
                ..Default::default()
            },
            EngineIdentity {
                channel: silo_engine::channel::Channel::Development,
                version: "0.1.0-test".into(),
                package_kind: silo_engine::context::PackageKind::MacApp,
            },
            LocalDeviceIdentity {
                device_id: "test-device".into(),
                name: "Test Mac".into(),
            },
            Arc::new(NoopMainThread),
            Arc::new(NoopProcess),
        )
    }

    fn notice(key: &str, body: &str) -> Notice {
        Notice {
            category: silo_api::methods::notifications::NoticeCategory::Changes,
            key: key.to_owned(),
            title: "Test notice".into(),
            body: body.to_owned(),
            computer: None,
        }
    }

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

    #[tokio::test]
    async fn notice_snapshot_barrier_and_same_key_publication_share_one_projection() {
        let root = tempfile::tempdir().unwrap();
        let context = test_context(root.path());
        let client_id = ClientId::new("client-a");
        let mut receiver = context.connect_event_stream(client_id.clone());
        let subscription = context
            .subscribe_events(&client_id, vec!["notifications.*".to_owned()])
            .unwrap();
        context.activate_events(&client_id, &subscription).unwrap();

        let before = notice("same-key", "before snapshot");
        silo_engine::notices::retain(&context, &before, Audience::All).unwrap();
        let nested_context = context.clone();
        let interleaved = std::cell::Cell::new(false);
        let attempted = notice("same-key", "during snapshot");
        let snapshot = context
            .notifications_snapshot_with_hook(&client_id, || {
                if nested_context.notice_projection.try_lock().is_ok() {
                    interleaved.set(true);
                    nested_context
                        .retain_notice(&attempted, Audience::All)
                        .unwrap();
                }
            })
            .unwrap();
        assert!(!interleaved.get());
        assert_eq!(snapshot.notices, vec![before]);
        assert_eq!(snapshot.event_barrier.get(&subscription), Some(&1));

        let published = notice("same-key", "outer commit");
        let interleaving = notice("same-key", "nested commit");
        let nested_context = context.clone();
        let interleaved = std::cell::Cell::new(false);
        context
            .retain_notice_with_hook(&published, Audience::All, || {
                if nested_context.notice_projection.try_lock().is_ok() {
                    interleaved.set(true);
                    nested_context
                        .retain_notice(&interleaving, Audience::All)
                        .unwrap();
                }
            })
            .unwrap();

        assert!(!interleaved.get());
        let latest = silo_engine::notices::list(&context).unwrap();
        assert_eq!(latest, vec![published.clone()]);
        let mut observed = Vec::new();
        while let Some(EventDelivery::Event(event)) = receiver.recv().await {
            observed.push(event.event.payload["body"].as_str().unwrap().to_owned());
            if observed.len() == 2 {
                break;
            }
        }
        assert_eq!(observed, vec!["before snapshot", "outer commit"]);
    }
}
