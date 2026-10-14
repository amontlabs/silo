use std::{
    collections::HashSet,
    future::Future,
    net::TcpStream,
    pin::Pin,
    sync::Arc,
};

use serde::Deserialize;
use serde_json::Value;
use silo_api::{
    jobs::ErrorCode,
    methods::{
        engine::{EngineState, EngineStateParams},
        guest::{GuestSshParams, GuestSshPurpose},
        method_registry, MethodDescriptor, MethodName,
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
    Pin<Box<dyn Future<Output = Result<Value, DispatchError>> + Send + 'a>>;
pub type StreamFuture<'a> =
    Pin<Box<dyn Future<Output = Result<TcpStream, DispatchError>> + Send + 'a>>;

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

    fn open_stream<'a>(
        &'a self,
        method: MethodName,
        _params: Value,
        _context: Arc<SilodContext>,
        _request: RequestContext,
    ) -> StreamFuture<'a> {
        Box::pin(async move { Err(DispatchError::unsupported(method_name(method))) })
    }
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
                    "engine.handshake" | "engine.state" | "guest.ssh"
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
        _request: RequestContext,
    ) -> HandlerFuture<'a> {
        Box::pin(async move {
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
                _ => Err(DispatchError::unsupported(method_name(method))),
            }
        })
    }

    fn open_stream<'a>(
        &'a self,
        method: MethodName,
        params: Value,
        context: Arc<SilodContext>,
        _request: RequestContext,
    ) -> StreamFuture<'a> {
        Box::pin(async move {
            if method != MethodName::GuestSsh {
                return Err(DispatchError::unsupported(method_name(method)));
            }
            let params: GuestSshParams = serde_json::from_value(params)
                .map_err(|error| DispatchError::internal(format!("Invalid params: {error}")))?;
            let port = match (params.purpose, params.port) {
                (None, None) => 22,
                (Some(GuestSshPurpose::Port), Some(port)) if port != 0 => port,
                _ => return Err(DispatchError::internal("Invalid guest connection.")),
            };
            let app_data = context.paths().app_data.clone();
            tokio::task::spawn_blocking(move || {
                silo_engine::macos_computers_engine::connect_guest_ssh_stream(
                    &app_data,
                    &params.computer_id,
                    port,
                )
            })
            .await
            .map_err(|error| DispatchError::internal(error.to_string()))?
            .map_err(DispatchError::internal)
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

    pub fn busy_response(&self, id: String) -> ResponseEnvelope<Value> {
        error_response(
            id,
            ErrorCode::Busy,
            "The engine is at its current request capacity; retry shortly.".to_owned(),
        )
    }

    pub fn stream_not_first_response(&self, id: String) -> ResponseEnvelope<Value> {
        error_response(
            id,
            ErrorCode::UnsupportedRemoteOperation,
            "guest.ssh must be the first request after engine.handshake.".to_owned(),
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
            },
            PreparedRequest::Close(response, reason) => DispatchResult {
                response,
                close: true,
                close_reason: Some(reason),
            },
            PreparedRequest::Invoke {
                id,
                method,
                params,
                request_context,
            } => {
                let result = self
                    .registry
                    .dispatch(method, params, self.context.clone(), request_context)
                    .await;
                DispatchResult {
                    response: Some(match result {
                        Ok(result) => success_response(id, result),
                        Err(error) => error_response(id, error.code, error.message),
                    }),
                    close: false,
                    close_reason: None,
                }
            }
        }
    }

    pub async fn execute_stream(
        &self,
        prepared: PreparedRequest,
    ) -> (Option<ResponseEnvelope<Value>>, Option<TcpStream>) {
        match prepared {
            PreparedRequest::Invoke {
                id,
                method,
                params,
                request_context,
            } => match self
                .registry
                .open_stream(method, params, self.context.clone(), request_context)
                .await
            {
                Ok(stream) => (
                    Some(success_response(id, serde_json::json!({}))),
                    Some(stream),
                ),
                Err(error) => (Some(error_response(id, error.code, error.message)), None),
            },
            PreparedRequest::Reply(response) | PreparedRequest::Close(Some(response), _) => {
                (Some(response), None)
            }
            PreparedRequest::Close(None, _) => (None, None),
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
        assert_eq!(
            dispatcher.capabilities(),
            ["engine.handshake", "engine.state", "guest.ssh"]
        );
    }

    #[tokio::test]
    async fn guest_ssh_stream_rejects_invalid_computer_identity() {
        let dispatcher = dispatcher();
        let mut session = SessionState::default();
        let _ = dispatcher
            .execute(dispatcher.prepare(handshake("one", 6, 6), &mut session))
            .await;
        let prepared = dispatcher.prepare(
            serde_json::json!({
                "id": "two",
                "method": "guest.ssh",
                "params": { "computerId": "not-a-uuid" }
            }),
            &mut session,
        );
        let (response, stream) = dispatcher.execute_stream(prepared).await;
        assert!(stream.is_none());
        assert!(matches!(
            response,
            Some(ResponseEnvelope::Error { id, error })
                if id == "two" && error.code == ErrorCode::Internal
        ));
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
}
