use std::{collections::HashSet, future::Future, pin::Pin, sync::Arc};

use serde::Deserialize;
use serde_json::Value;
use silo_api::{
    jobs::ErrorCode,
    methods::{
        engine::{EngineState, EngineStateParams, EventsSubscribeParams, EventsUnsubscribeParams},
        method_registry,
        notifications::NotificationsListParams,
        operations::{AcknowledgeJobParams, CancelJobParams, GetJobParams, OperationsListParams},
        MethodDescriptor, MethodName,
    },
    transport::{
        ClientMetadata, ErrorObject, HandshakeParams, HandshakeResult,
        IncompatibleVersionDiagnostics, ProtocolRange, RequestEnvelope, ResponseEnvelope,
        PROTOCOL_VERSION,
    },
};
use silo_engine::context::EngineContext;
use uuid::Uuid;

use crate::context::SilodContext;

const MAX_REQUEST_IDS: usize = 65_536;

pub type HandlerFuture<'a> =
    Pin<Box<dyn Future<Output = Result<HandlerOutput, DispatchError>> + Send + 'a>>;

pub struct HandlerOutput {
    pub value: Value,
    pub event_barrier: Option<std::collections::HashMap<String, u64>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestContext {
    pub client_id: String,
    pub protocol: u32,
    pub client: Option<ClientMetadata>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DispatchError {
    pub code: ErrorCode,
    pub message: String,
}

impl DispatchError {
    fn internal(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::Internal,
            message: message.into(),
        }
    }

    fn unsupported(method: &str) -> Self {
        Self {
            code: ErrorCode::UnsupportedRemoteOperation,
            message: format!("The engine does not support {method}."),
        }
    }
}

#[derive(Default)]
pub struct SessionState {
    client_id: Option<String>,
    protocol: Option<u32>,
    client: Option<ClientMetadata>,
    seen_ids: HashSet<String>,
}

impl SessionState {
    pub fn client_id(&self) -> Option<&str> {
        self.client_id.as_deref()
    }

    pub fn protocol(&self) -> Option<u32> {
        self.protocol
    }
}

pub trait MethodRegistry: Send + Sync {
    fn descriptors(&self) -> &[MethodDescriptor];

    fn dispatch<'a>(
        &'a self,
        method: MethodName,
        params: Value,
        context: Arc<SilodContext>,
        request: RequestContext,
    ) -> HandlerFuture<'a>;
}

pub struct CoreMethodRegistry {
    descriptors: Vec<MethodDescriptor>,
}

impl Default for CoreMethodRegistry {
    fn default() -> Self {
        let descriptors = method_registry()
            .iter()
            .copied()
            .filter(|descriptor| {
                matches!(
                    descriptor.name,
                    "engine.handshake"
                        | "engine.state"
                        | "events.subscribe"
                        | "events.unsubscribe"
                        | "notifications.list"
                        | "operations.cancel"
                        | "operations.job.acknowledge"
                        | "operations.job.get"
                        | "operations.list"
                )
            })
            .collect();
        Self { descriptors }
    }
}

impl MethodRegistry for CoreMethodRegistry {
    fn descriptors(&self) -> &[MethodDescriptor] {
        &self.descriptors
    }

    fn dispatch<'a>(
        &'a self,
        method: MethodName,
        params: Value,
        context: Arc<SilodContext>,
        request: RequestContext,
    ) -> HandlerFuture<'a> {
        Box::pin(async move {
            let mut event_barrier = None;
            let result: Result<Value, DispatchError> = async {
            match method {
                MethodName::EngineState => {
                    serde_json::from_value::<EngineStateParams>(params).map_err(|error| {
                        DispatchError::internal(format!("Invalid params: {error}"))
                    })?;
                    let state: EngineState = context.engine_state();
                    serde_json::to_value(state)
                        .map_err(|error| DispatchError::internal(error.to_string()))
                }
                MethodName::EngineHandshake => Err(DispatchError::internal(
                    "engine.handshake is handled by the session negotiator",
                )),
                MethodName::EventsSubscribe => {
                    let params: EventsSubscribeParams = parse_params(params)?;
                    if let Some(device_ids) = &params.device_ids {
                        if device_ids.iter().any(|id| id != &context.local_device().device_id) {
                            return Err(DispatchError {
                                code: ErrorCode::UnsupportedRemoteOperation,
                                message: "This engine does not relay subscriptions to remote devices."
                                    .to_owned(),
                            });
                        }
                    }
                    let client_id = silo_engine::context::ClientId::new(request.client_id);
                    let subscription = context
                        .subscribe_events(&client_id, params.events)
                        .map_err(DispatchError::internal)?;
                    serde_json::to_value(silo_api::methods::engine::EventsSubscribeResult {
                        subscription,
                    })
                    .map_err(|error| DispatchError::internal(error.to_string()))
                }
                MethodName::EventsUnsubscribe => {
                    let params: EventsUnsubscribeParams = parse_params(params)?;
                    event_barrier = Some(context.unsubscribe_events(
                        &silo_engine::context::ClientId::new(request.client_id),
                        &params.subscription,
                    ));
                    Ok(Value::Null)
                }
                MethodName::OperationsJobGet => {
                    let params: GetJobParams = parse_params(params)?;
                    let registry = context.job_registry().map_err(DispatchError::internal)?;
                    let result = registry
                        .get_at(&params.job_id, params.offset_bytes)
                        .map_err(registry_error)?
                        .ok_or_else(|| DispatchError {
                            code: ErrorCode::NotFound,
                            message: "The job was not found.".to_owned(),
                        })?;
                    serde_json::to_value(result)
                        .map_err(|error| DispatchError::internal(error.to_string()))
                }
                MethodName::OperationsJobAcknowledge => {
                    let params: AcknowledgeJobParams = parse_params(params)?;
                    context
                        .job_registry()
                        .map_err(DispatchError::internal)?
                        .acknowledge(&params.job_id)
                        .map_err(registry_error)?;
                    context.job_queue_changed();
                    Ok(Value::Null)
                }
                MethodName::OperationsList => {
                    let _: OperationsListParams = parse_params(params)?;
                    let snapshot = context
                        .operation_queue_snapshot(&silo_engine::context::ClientId::new(
                            request.client_id.clone(),
                        ))
                        .map_err(DispatchError::internal)?;
                    event_barrier = Some(snapshot.event_barrier);
                    serde_json::to_value(snapshot.queue)
                        .map_err(|error| DispatchError::internal(error.to_string()))
                }
                MethodName::OperationsCancel => {
                    let params: CancelJobParams = parse_params(params)?;
                    context
                        .job_registry()
                        .map_err(DispatchError::internal)?
                        .cancel(&params.job_id)
                        .map_err(registry_error)?;
                    Ok(Value::Null)
                }
                MethodName::NotificationsList => {
                    let _: NotificationsListParams = parse_params(params)?;
                    let snapshot = context
                        .notifications_snapshot(&silo_engine::context::ClientId::new(
                            request.client_id.clone(),
                        ))
                        .map_err(DispatchError::internal)?;
                    event_barrier = Some(snapshot.event_barrier);
                    serde_json::to_value(snapshot.notices)
                        .map_err(|error| DispatchError::internal(error.to_string()))
                }
                _ => Err(DispatchError::unsupported(method_name(method))),
            }
            }
            .await;
            result.map(|value| HandlerOutput {
                value,
                event_barrier,
            })
        })
    }
}

pub struct Dispatcher {
    registry: Arc<dyn MethodRegistry>,
    context: Arc<SilodContext>,
}

impl Dispatcher {
    pub fn new(registry: Arc<dyn MethodRegistry>, context: Arc<SilodContext>) -> Self {
        Self { registry, context }
    }

    pub fn core(context: Arc<SilodContext>) -> Self {
        Self::new(Arc::new(CoreMethodRegistry::default()), context)
    }

    pub fn capabilities(&self) -> Vec<String> {
        self.registry
            .descriptors()
            .iter()
            .map(|descriptor| descriptor.name.to_owned())
            .collect()
    }

    pub fn context(&self) -> Arc<SilodContext> {
        self.context.clone()
    }

    pub fn busy_response(&self, id: String) -> ResponseEnvelope<Value> {
        error_response(
            id,
            ErrorCode::Busy,
            "The engine is at its current request capacity; retry shortly.".to_owned(),
        )
    }

    pub fn prepare(&self, payload: Value, session: &mut SessionState) -> PreparedRequest {
        let raw: RawRequest = match serde_json::from_value(payload.clone()) {
            Ok(raw) => raw,
            Err(error) => {
                return PreparedRequest::Close(None, format!("Invalid request envelope: {error}"));
            }
        };
        if raw.id.is_empty() {
            return PreparedRequest::Close(None, "Request id must not be empty".to_owned());
        }
        if raw.id.len() > 256 {
            return PreparedRequest::Close(
                Some(error_response(
                    raw.id,
                    ErrorCode::Internal,
                    "Request id exceeds the 256-byte limit".to_owned(),
                )),
                "Request id exceeds the 256-byte limit".to_owned(),
            );
        }
        if session.seen_ids.contains(&raw.id) {
            return PreparedRequest::Close(
                Some(error_response(
                    raw.id,
                    ErrorCode::Internal,
                    "Request id was already used on this connection".to_owned(),
                )),
                "Duplicate request id".to_owned(),
            );
        }
        if session.seen_ids.len() >= MAX_REQUEST_IDS {
            return PreparedRequest::Close(
                Some(error_response(
                    raw.id,
                    ErrorCode::Busy,
                    "Reconnect before issuing more requests on this connection".to_owned(),
                )),
                "Request id limit reached".to_owned(),
            );
        }
        session.seen_ids.insert(raw.id.clone());

        if session.protocol.is_none() {
            if raw.method != "engine.handshake" {
                return PreparedRequest::Close(
                    Some(error_response(
                        raw.id,
                        ErrorCode::Internal,
                        "The first request must be engine.handshake".to_owned(),
                    )),
                    "Handshake must be the first request".to_owned(),
                );
            }
            return self.negotiate(raw, session);
        }

        let Some(descriptor) = self
            .registry
            .descriptors()
            .iter()
            .find(|descriptor| descriptor.name == raw.method)
            .copied()
        else {
            return PreparedRequest::Reply(error_response(
                raw.id,
                ErrorCode::UnsupportedRemoteOperation,
                DispatchError::unsupported(&raw.method).message,
            ));
        };
        if descriptor.method == MethodName::EngineHandshake {
            return PreparedRequest::Reply(error_response(
                raw.id,
                ErrorCode::Internal,
                "engine.handshake may only be sent once per connection".to_owned(),
            ));
        }
        let request: RequestEnvelope<Value> = match serde_json::from_value(payload) {
            Ok(request) => request,
            Err(error) => {
                return PreparedRequest::Reply(error_response(
                    raw.id,
                    ErrorCode::Internal,
                    format!("Invalid request: {error}"),
                ));
            }
        };
        let request_context = RequestContext {
            client_id: session
                .client_id
                .clone()
                .expect("negotiated session has client id"),
            protocol: session.protocol.expect("negotiated session has protocol"),
            client: session.client.clone(),
        };
        PreparedRequest::Invoke {
            id: request.id,
            method: descriptor.method,
            params: request.params,
            request_context,
        }
    }

    pub async fn execute(&self, prepared: PreparedRequest) -> DispatchResult {
        match prepared {
            PreparedRequest::Reply(response) => DispatchResult {
                response: Some(response),
                close: false,
                close_reason: None,
                activate_subscription: None,
                event_barrier: None,
            },
            PreparedRequest::Close(response, reason) => DispatchResult {
                response,
                close: true,
                close_reason: Some(reason),
                activate_subscription: None,
                event_barrier: None,
            },
            PreparedRequest::Invoke {
                id,
                method,
                params,
                request_context,
            } => {
                let client_id = request_context.client_id.clone();
                let result = self
                    .registry
                    .dispatch(method, params, self.context.clone(), request_context)
                    .await;
                let (response, activate_subscription, event_barrier) = match result {
                    Ok(result) => {
                        let HandlerOutput {
                            value,
                            event_barrier,
                        } = result;
                        let activate = if method == MethodName::EventsSubscribe {
                            value
                                .get("subscription")
                                .and_then(Value::as_str)
                                .map(|subscription| (client_id, subscription.to_owned()))
                        } else {
                            None
                        };
                        (success_response(id, value), activate, event_barrier)
                    }
                    Err(error) => (error_response(id, error.code, error.message), None, None),
                };
                DispatchResult {
                    response: Some(response),
                    close: false,
                    close_reason: None,
                    activate_subscription,
                    event_barrier,
                }
            }
        }
    }

    fn negotiate(&self, raw: RawRequest, session: &mut SessionState) -> PreparedRequest {
        let request: RequestEnvelope<Value> = match serde_json::from_value(raw.payload()) {
            Ok(request) => request,
            Err(error) => {
                return PreparedRequest::Close(
                    Some(error_response(
                        raw.id,
                        ErrorCode::Internal,
                        format!("Invalid engine.handshake request: {error}"),
                    )),
                    "Invalid handshake".to_owned(),
                );
            }
        };
        let params: HandshakeParams = match serde_json::from_value(request.params) {
            Ok(params) => params,
            Err(error) => {
                return PreparedRequest::Close(
                    Some(error_response(
                        raw.id,
                        ErrorCode::Internal,
                        format!("Invalid engine.handshake params: {error}"),
                    )),
                    "Invalid handshake params".to_owned(),
                );
            }
        };
        let supported = ProtocolRange {
            min: PROTOCOL_VERSION,
            max: PROTOCOL_VERSION,
        };
        let overlap_min = params.protocol.min.max(supported.min);
        let overlap_max = params.protocol.max.min(supported.max);
        if params.protocol.min == 0
            || params.protocol.min > params.protocol.max
            || overlap_min > overlap_max
        {
            let diagnostics = IncompatibleVersionDiagnostics {
                engine_protocol_range: supported,
                client_protocol_range: params.protocol,
                engine_app_version: self.context.identity().version.clone(),
                client_app_version: params.client.as_ref().map(|client| client.version.clone()),
            };
            return PreparedRequest::Close(
                Some(error_response_with_diagnostics(
                    raw.id,
                    ErrorCode::IncompatibleVersion,
                    "No common protocol version".to_owned(),
                    Some(diagnostics),
                )),
                "Incompatible protocol range".to_owned(),
            );
        }

        let chosen = overlap_max;
        let client_id = Uuid::new_v4().to_string();
        session.protocol = Some(chosen);
        session.client_id = Some(client_id.clone());
        session.client = params.client;
        let result = HandshakeResult {
            device_id: self.context.local_device().device_id.clone(),
            name: self.context.local_device().name.clone(),
            protocol: chosen,
            capabilities: self.capabilities(),
            client_id: Some(client_id),
        };
        match serde_json::to_value(result) {
            Ok(result) => PreparedRequest::Reply(success_response(raw.id, result)),
            Err(error) => PreparedRequest::Close(
                Some(error_response(
                    raw.id,
                    ErrorCode::Internal,
                    error.to_string(),
                )),
                "Handshake serialization failed".to_owned(),
            ),
        }
    }
}

pub enum PreparedRequest {
    Reply(ResponseEnvelope<Value>),
    Close(Option<ResponseEnvelope<Value>>, String),
    Invoke {
        id: String,
        method: MethodName,
        params: Value,
        request_context: RequestContext,
    },
}

pub struct DispatchResult {
    pub response: Option<ResponseEnvelope<Value>>,
    pub close: bool,
    pub close_reason: Option<String>,
    pub activate_subscription: Option<(String, String)>,
    pub event_barrier: Option<std::collections::HashMap<String, u64>>,
}

fn parse_params<T: for<'de> Deserialize<'de>>(params: Value) -> Result<T, DispatchError> {
    serde_json::from_value(params)
        .map_err(|error| DispatchError::internal(format!("Invalid params: {error}")))
}

fn registry_error(error: silo_engine::jobs::RegistryError) -> DispatchError {
    let message = error.to_string();
    let code = match error.kind() {
        silo_engine::jobs::RegistryErrorKind::NotCancellable => ErrorCode::Internal,
        silo_engine::jobs::RegistryErrorKind::Storage
        | silo_engine::jobs::RegistryErrorKind::Internal => {
            if message == "not_found" {
                ErrorCode::NotFound
            } else if message.starts_with("busy:") || message.contains("cannot be acknowledged") {
                ErrorCode::Busy
            } else {
                ErrorCode::Internal
            }
        }
    };
    DispatchError { code, message }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawRequest {
    id: String,
    method: String,
    params: Value,
    #[serde(default)]
    operation_id: Option<String>,
    #[serde(default)]
    start_within_ms: Option<u64>,
}

impl RawRequest {
    fn payload(&self) -> Value {
        serde_json::json!({
            "id": self.id,
            "method": self.method,
            "params": self.params,
            "operationId": self.operation_id,
            "startWithinMs": self.start_within_ms,
        })
    }
}

fn method_name(method: MethodName) -> &'static str {
    method_registry()
        .iter()
        .find(|descriptor| descriptor.method == method)
        .map(|descriptor| descriptor.name)
        .unwrap_or("unknown method")
}

fn success_response(id: String, result: Value) -> ResponseEnvelope<Value> {
    ResponseEnvelope::Success { id, result }
}

fn error_response(id: String, code: ErrorCode, message: String) -> ResponseEnvelope<Value> {
    error_response_with_diagnostics(id, code, message, None)
}

fn error_response_with_diagnostics(
    id: String,
    code: ErrorCode,
    message: String,
    diagnostics: Option<IncompatibleVersionDiagnostics>,
) -> ResponseEnvelope<Value> {
    ResponseEnvelope::Error {
        id,
        error: ErrorObject {
            code,
            message,
            diagnostics,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use silo_api::transport::ProtocolRange;
    use silo_engine::{
        channel::Channel,
        context::{EngineIdentity, EnginePaths, PackageKind},
    };

    use crate::{context::SilodContext, identity::LocalDeviceIdentity};

    struct NoopMainThread;
    impl silo_engine::context::MainThread for NoopMainThread {
        fn run(&self, _work: Box<dyn FnOnce() + Send>) -> Result<(), String> {
            Ok(())
        }
    }
    struct NoopProcess;
    impl silo_engine::context::ProcessControl for NoopProcess {
        fn exit(&self, _code: i32) {}
        fn restart(&self) -> ! {
            panic!("restart should not be called in dispatcher tests")
        }
    }

    fn dispatcher() -> Dispatcher {
        let context = Arc::new(SilodContext::new(
            EnginePaths::default(),
            EngineIdentity {
                channel: Channel::Development,
                version: "0.1.0".into(),
                package_kind: PackageKind::MacApp,
            },
            LocalDeviceIdentity {
                device_id: "device-1".into(),
                name: "Test Mac".into(),
            },
            Arc::new(NoopMainThread),
            Arc::new(NoopProcess),
        ));
        Dispatcher::core(context)
    }

    fn handshake(id: &str, min: u32, max: u32) -> Value {
        serde_json::json!({
            "id": id,
            "method": "engine.handshake",
            "params": {
                "protocol": ProtocolRange { min, max },
                "client": { "name": "test", "version": "1.0" }
            }
        })
    }

    #[tokio::test]
    async fn dispatcher_serves_only_registered_engine_methods() {
        let dispatcher = dispatcher();
        let mut session = SessionState::default();
        let handshake = dispatcher.prepare(
            handshake("one", PROTOCOL_VERSION, PROTOCOL_VERSION),
            &mut session,
        );
        let handshake = dispatcher.execute(handshake).await;
        assert!(!handshake.close);
        assert_eq!(session.protocol(), Some(PROTOCOL_VERSION));

        let state = dispatcher.prepare(
            serde_json::json!({ "id": "two", "method": "engine.state", "params": {} }),
            &mut session,
        );
        let state = dispatcher.execute(state).await;
        assert!(matches!(
            state.response,
            Some(ResponseEnvelope::Success { .. })
        ));

        let unsupported = dispatcher.prepare(
            serde_json::json!({ "id": "three", "method": "computers.list", "params": {} }),
            &mut session,
        );
        let unsupported = dispatcher.execute(unsupported).await;
        assert!(matches!(
            unsupported.response,
            Some(ResponseEnvelope::Error { error, .. })
                if error.code == ErrorCode::UnsupportedRemoteOperation
        ));
        let capabilities = dispatcher.capabilities();
        assert!(capabilities.contains(&"engine.handshake".to_owned()));
        assert!(capabilities.contains(&"engine.state".to_owned()));
        assert!(capabilities.contains(&"events.subscribe".to_owned()));
        assert!(capabilities.contains(&"operations.job.get".to_owned()));
        assert!(capabilities.contains(&"notifications.list".to_owned()));
    }

    #[tokio::test]
    async fn handshake_negotiates_the_highest_supported_overlap_and_closes_on_mismatch() {
        let dispatcher = dispatcher();
        let mut session = SessionState::default();
        let reply = dispatcher
            .execute(dispatcher.prepare(handshake("one", 5, 7), &mut session))
            .await;
        assert!(matches!(
            reply.response,
            Some(ResponseEnvelope::Success { result, .. }) if result["protocol"] == PROTOCOL_VERSION
        ));

        let mut session = SessionState::default();
        let reply = dispatcher
            .execute(dispatcher.prepare(handshake("one", 5, 5), &mut session))
            .await;
        assert!(reply.close);
        assert!(matches!(
            reply.response,
            Some(ResponseEnvelope::Error { error, .. })
                if error.code == ErrorCode::IncompatibleVersion
        ));
    }

    #[tokio::test]
    async fn incompatible_handshake_reports_distinct_engine_and_client_diagnostics() {
        let dispatcher = dispatcher();
        let mut session = SessionState::default();
        let request = serde_json::json!({
            "id": "incompatible",
            "method": "engine.handshake",
            "params": {
                "protocol": { "min": 5, "max": 5 },
                "client": { "name": "Fjord", "version": "9.8.7" }
            }
        });
        let reply = dispatcher
            .execute(dispatcher.prepare(request, &mut session))
            .await;
        let value = serde_json::to_value(reply.response.unwrap()).unwrap();

        assert!(reply.close);
        assert_eq!(
            value["error"]["diagnostics"]["engineProtocolRange"],
            serde_json::json!({ "min": PROTOCOL_VERSION, "max": PROTOCOL_VERSION })
        );
        assert_eq!(
            value["error"]["diagnostics"]["clientProtocolRange"],
            serde_json::json!({ "min": 5, "max": 5 })
        );
        assert_eq!(value["error"]["diagnostics"]["engineAppVersion"], "0.1.0");
        assert_eq!(value["error"]["diagnostics"]["clientAppVersion"], "9.8.7");
        assert!(value["error"].get("appVersion").is_none());
    }

    #[tokio::test]
    async fn request_ids_are_unique_for_the_connection() {
        let dispatcher = dispatcher();
        let mut session = SessionState::default();
        let _ = dispatcher
            .execute(dispatcher.prepare(handshake("one", 6, 6), &mut session))
            .await;
        let request = serde_json::json!({ "id": "two", "method": "engine.state", "params": {} });
        let _ = dispatcher
            .execute(dispatcher.prepare(request.clone(), &mut session))
            .await;
        assert!(matches!(
            dispatcher.prepare(request, &mut session),
            PreparedRequest::Close(Some(ResponseEnvelope::Error { .. }), _)
        ));
    }

    #[test]
    fn not_cancellable_uses_the_contract_internal_error_code() {
        let temporary = tempfile::tempdir().unwrap();
        let registry = silo_engine::jobs::JobRegistry::open(temporary.path()).unwrap();
        let job_id = silo_api::jobs::JobId::new("job:1").unwrap();
        registry.accept(job_id.clone(), "test.operation").unwrap();

        let mapped = registry_error(registry.cancel(&job_id).unwrap_err());

        assert_eq!(mapped.code, ErrorCode::Internal);
        assert_eq!(mapped.message, "This operation can't be cancelled.");
    }
}
