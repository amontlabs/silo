//! JSON Schema generation for the full wire contract.

use std::path::Path;

use crate::{
    events::EventEnvelope,
    jobs::{ErrorCode, JobFinished, JobId, JobLookupResult, JobProgress},
    methods::{registry::method_registry, MethodParams, MethodResults},
    transport::{ErrorObject, HandshakeParams, HandshakeResult, RequestEnvelope, ResponseEnvelope},
};
use schemars::JsonSchema;

/// Root document that makes every public wire type reachable to schemars.
/// Its fields describe the contract catalogue, not one transport message.
#[derive(JsonSchema)]
#[allow(dead_code)]
pub struct SiloApiSchema {
    request: RequestEnvelope<serde_json::Value>,
    response: ResponseEnvelope<serde_json::Value>,
    handshake: HandshakeParams,
    handshake_result: HandshakeResult,
    methods: MethodContractSchema,
    events: EventEnvelope,
    jobs: JobContractSchema,
    job_terminal_results: JobTerminalResultsSchema,
    errors: ErrorContractSchema,
}

#[derive(JsonSchema)]
#[allow(dead_code)]
struct MethodContractSchema {
    params: MethodParams,
    results: MethodResults,
}

#[derive(JsonSchema)]
#[allow(dead_code)]
struct JobContractSchema {
    id: JobId,
    progress: JobProgress,
    finished: JobFinished,
    lookup: JobLookupResult,
    acknowledge: crate::methods::operations::AcknowledgeJobParams,
}

/// Terminal job values remain untagged on the wire. This catalogue roots every
/// method-specific result schema and maps the defined job completion shapes.
#[derive(JsonSchema)]
#[allow(dead_code)]
struct JobTerminalResultsSchema {
    application_source: crate::methods::computers::ApplicationSource,
    macos_computers: crate::methods::computers::MacosComputersState,
    checkpoint_created: crate::methods::checkpoints::CheckpointCreatedResult,
    desktop: crate::methods::desktop::DesktopStatus,
    backup: crate::methods::exports::BackupOperation,
    archive_inspection: crate::methods::exports::ArchiveInspectionResult,
    upload: crate::methods::files::UploadOutcome,
    download: crate::methods::files::DownloadOutcome,
    github: crate::methods::github::GitHubState,
    guest_exec: crate::methods::guest::GuestExecFinishedResult,
    engine_update_prepare: crate::methods::engine::EngineUpdatePrepareFinishedResult,
    push: crate::methods::push::PushOperation,
    storage: crate::methods::storage::StorageState,
    update: crate::methods::updates::UpdateSnapshot,
}

/// Method-specific final job result schemas. Conditional rows describe the
/// platform-specific terminal outcomes called out in the domain specifications.
pub const JOB_FINISHED_RESULT_REGISTRY: &[(&str, &str)] = &[
    ("checkpoints.abandonRestore", "ApplicationSource | null"),
    ("checkpoints.create", "CheckpointCreatedResult"),
    ("checkpoints.delete", "ApplicationSource | null"),
    ("checkpoints.fork", "ApplicationSource | null"),
    ("checkpoints.restore", "ApplicationSource | null"),
    ("computerUse.setup", "DesktopStatus"),
    ("computers.configuration.change", "ApplicationSource"),
    ("computers.configuration.retry", "ApplicationSource"),
    ("computers.create", "ApplicationSource | null"),
    (
        "computers.delete",
        "ApplicationSource | null | MacosComputersState",
    ),
    ("computers.identities.configure", "null"),
    (
        "computers.restart",
        "ApplicationSource | null | MacosComputersState",
    ),
    (
        "computers.start",
        "ApplicationSource | null | MacosComputersState",
    ),
    (
        "computers.stop",
        "ApplicationSource | null | MacosComputersState",
    ),
    ("desktop.action", "DesktopStatus"),
    ("engine.shutdown", "null"),
    ("engine.update.prepare", "EngineUpdatePrepareFinishedResult"),
    ("exports.create", "BackupOperation"),
    ("exports.import", "BackupOperation"),
    ("exports.inspect", "ArchiveInspectionResult"),
    ("files.download", "DownloadOutcome"),
    ("files.upload", "UploadOutcome"),
    ("github.connect", "GitHubState"),
    ("guest.exec.start", "GuestExecFinishedResult"),
    ("logs.export", "boolean"),
    ("macos.setup.retry", "null"),
    ("push.start", "PushOperation"),
    ("storage.reclaim", "StorageState"),
    ("updates.download", "UpdateSnapshot"),
];

#[derive(JsonSchema)]
#[allow(dead_code)]
struct ErrorContractSchema {
    code: ErrorCode,
    error: ErrorObject,
}

/// Generate a pretty-printed JSON Schema 2020-12 document, including the
/// machine-readable canonical method registry under `x-silo-methods`.
pub fn schema_json() -> Result<String, serde_json::Error> {
    let schema = schemars::schema_for!(SiloApiSchema);
    let mut document = serde_json::to_value(schema)?;
    let methods: Vec<_> = method_registry()
        .iter()
        .map(|entry| {
            serde_json::json!({
                "name": entry.name,
                "kind": entry.kind,
                "paramsType": entry.params_type,
                "resultType": entry.result_type,
            })
        })
        .collect();
    document
        .as_object_mut()
        .expect("schemars returns a JSON object for the root schema")
        .insert(
            "x-silo-methods".to_owned(),
            serde_json::Value::Array(methods),
        );
    document
        .as_object_mut()
        .expect("schemars returns a JSON object for the root schema")
        .insert(
            "x-silo-job-results".to_owned(),
            serde_json::Value::Array(
                JOB_FINISHED_RESULT_REGISTRY
                    .iter()
                    .map(|(method, result_type)| {
                        serde_json::json!({"method": method, "resultType": result_type})
                    })
                    .collect(),
            ),
        );
    Ok(format!("{}\n", serde_json::to_string_pretty(&document)?))
}

/// Write the generated schema to the supplied path.
pub fn write_schema(path: impl AsRef<Path>) -> std::io::Result<()> {
    let text = schema_json().map_err(std::io::Error::other)?;
    std::fs::write(path, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committed_schema_matches_the_wire_types_and_registry() {
        assert_eq!(
            include_str!("../schema/silo-api.schema.json"),
            schema_json().unwrap()
        );
        let schema: serde_json::Value = serde_json::from_str(&schema_json().unwrap()).unwrap();
        assert_eq!(schema["x-silo-methods"].as_array().unwrap().len(), 146);
        assert_eq!(schema["x-silo-job-results"].as_array().unwrap().len(), 29);
        assert_eq!(
            schema["$schema"],
            "https://json-schema.org/draft/2020-12/schema"
        );
        assert!(schema["x-silo-methods"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| {
                entry["name"] == "computers.create"
                    && entry["kind"] == "job"
                    && entry["paramsType"] == "computers::ComputerCreateParams"
                    && entry["resultType"] == "computers::ComputerCreateResult"
            }));
        assert!(schema["x-silo-job-results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| {
                entry["method"] == "computers.identities.configure" && entry["resultType"] == "null"
            }));
    }

    #[test]
    fn method_union_round_trips_a_typed_empty_request() {
        let call = MethodParams::ComputersList(crate::methods::computers::ComputerListParams {
            device_id: None,
            refresh_repositories: None,
        });
        let json = serde_json::to_string(&call).unwrap();
        assert_eq!(json, r#"{"method":"computers.list","params":{}}"#);
        assert_eq!(serde_json::from_str::<MethodParams>(&json).unwrap(), call);

        let result = MethodResults::ComputersList(crate::methods::computers::ComputersListResult {
            linux: crate::methods::computers::ApplicationSource {
                runtime_repair: None,
                computers: vec![],
                activities: vec![],
                computer_configuration_operation: None,
                repository_push_operations: vec![],
                github: serde_json::Value::Null,
                secrets: vec![],
                device_capacity: None,
            },
            macos: crate::methods::computers::MacosComputersState {
                supported: false,
                unsupported_reason: None,
                computers: vec![],
                template: None,
                min_disk_gib: 0,
            },
        });
        let json = serde_json::to_string(&result).unwrap();
        assert_eq!(
            serde_json::from_str::<MethodResults>(&json).unwrap(),
            result
        );
    }

    #[test]
    fn request_handshake_and_job_event_round_trip() {
        let request = RequestEnvelope {
            id: "r1".to_owned(),
            method: crate::methods::MethodName::EngineHandshake,
            params: serde_json::json!({ "protocol": { "min": 1, "max": 2 } }),
            operation_id: None,
            start_within_ms: None,
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains(r#""method":"engine.handshake""#));
        assert_eq!(
            serde_json::from_str::<RequestEnvelope<serde_json::Value>>(&json).unwrap(),
            request
        );

        let handshake = HandshakeParams {
            ssh_key: None,
            protocol: crate::transport::ProtocolRange { min: 1, max: 2 },
            client: Some(crate::transport::ClientMetadata {
                name: "Silo".into(),
                version: "0.1".into(),
            }),
        };
        assert_eq!(
            serde_json::from_str::<HandshakeParams>(&serde_json::to_string(&handshake).unwrap())
                .unwrap(),
            handshake
        );

        let event = EventEnvelope::JobFinished {
            subscription: "s1".into(),
            device_id: "device-1".into(),
            seq: 7,
            payload: JobFinished {
                job_id: JobId::new("job:3").unwrap(),
                result: Some(serde_json::json!({ "ok": true })),
                error: None,
            },
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""event":"operations.job.finished""#));
        assert_eq!(serde_json::from_str::<EventEnvelope>(&json).unwrap(), event);
    }

    #[test]
    fn response_rejects_envelopes_with_both_result_and_error() {
        let invalid = r#"{"id":"r1","result":{},"error":{"code":"internal","message":"failed"}}"#;
        assert!(serde_json::from_str::<ResponseEnvelope<serde_json::Value>>(invalid).is_err());
    }

    #[test]
    fn error_codes_use_the_complete_snake_case_wire_set() {
        use crate::jobs::ErrorCode::*;

        let codes = [
            (UpdateInProgress, "update_in_progress"),
            (UnsupportedRemoteOperation, "unsupported_remote_operation"),
            (Cancelled, "cancelled"),
            (AlreadyQueued, "already_queued"),
            (Busy, "busy"),
            (NotFound, "not_found"),
            (IncompatibleVersion, "incompatible_version"),
            (Internal, "internal"),
            (NotRunning, "not_running"),
            (OpenSshMissing, "open_ssh_missing"),
            (IncludeNotWritable, "include_not_writable"),
            (AccountNotMigrated, "account_not_migrated"),
            (TimedOut, "timed_out"),
            (OutputTooLarge, "output_too_large"),
            (StdinClosed, "stdin_closed"),
            (PermissionDenied, "permission_denied"),
            (NotAFile, "not_a_file"),
            (Exists, "exists"),
            (Changed, "changed"),
            (RunningLimit, "running_limit"),
            (NeedsPersonalizing, "needs_personalizing"),
            (SetupIncomplete, "setup_incomplete"),
            (ScreenSharingUnavailable, "screen_sharing_unavailable"),
        ];
        assert_eq!(codes.len(), 23);
        for (code, expected) in codes {
            let json = serde_json::to_string(&code).unwrap();
            assert_eq!(json, format!("\"{expected}\""));
            assert_eq!(
                serde_json::from_str::<crate::jobs::ErrorCode>(&json).unwrap(),
                code
            );
        }
    }

    #[test]
    fn chatgpt_progress_fields_are_camel_case() {
        let status = crate::methods::preparation::ChatGptPreparationStatus::Downloading {
            received_bytes: 4,
            total_bytes: 8,
        };
        assert_eq!(
            serde_json::to_value(status).unwrap(),
            serde_json::json!({"state":"downloading","receivedBytes":4,"totalBytes":8})
        );
    }

    #[test]
    fn method_params_preserve_absent_and_nullable_settings_values() {
        use crate::methods::settings::{SettingsUpdateParams, SettingsValues};

        let params: SettingsUpdateParams = serde_json::from_value(serde_json::json!({
            "patch": {"theme": "dark", "terminalPath": null}
        }))
        .unwrap();
        assert_eq!(
            params.patch.theme,
            Some(crate::methods::settings::Theme::Dark)
        );
        assert_eq!(params.patch.terminal_path, Some(None));
        let value = serde_json::to_value(&params).unwrap();
        assert_eq!(
            value,
            serde_json::json!({"patch":{"theme":"dark","terminalPath":null}})
        );
        assert!(
            serde_json::from_value::<SettingsUpdateParams>(serde_json::json!({
                "patch": {"theme": null}
            }))
            .is_err()
        );

        let empty = serde_json::from_value::<SettingsValues>(serde_json::json!({})).unwrap();
        assert_eq!(serde_json::to_value(empty).unwrap(), serde_json::json!({}));
    }

    #[test]
    fn computer_use_and_network_state_round_trip_wire_enums() {
        let state = crate::methods::computer_use::ComputerUseState {
            state: crate::methods::computer_use::ComputerUseInstallState::Ready,
            reason: None,
            cause: None,
            compatibility: Some(crate::methods::computer_use::ComputerUseCompatibility::Unknown),
            warning: None,
            approval: Some(crate::methods::computer_use::ComputerUseApprovalState::Auto),
            applied_approval: Some(crate::methods::computer_use::ComputerUseApprovalState::Unknown),
            approval_apply: Some(
                crate::methods::computer_use::ComputerUseApprovalApplyState::Applied,
            ),
            approval_apply_reason: None,
            app_version: Some("1.2".into()),
            runtime_version: None,
            lcu_version: None,
            agents: Some(vec!["agent-a".into()]),
        };
        let state_json = serde_json::to_value(&state).unwrap();
        assert_eq!(state_json["state"], "ready");
        assert_eq!(state_json["cause"], serde_json::Value::Null);
        assert_eq!(
            serde_json::from_value::<crate::methods::computer_use::ComputerUseState>(state_json)
                .unwrap(),
            state
        );

        let port = crate::methods::network::PublishedPort {
            port: 8080,
            host_port: Some(8081),
            configured_host_port: None,
            scheme: Some(crate::methods::network::NetworkScheme::Https),
            state: crate::methods::network::PublishedPortState::Reachable,
            configured: true,
            message: None,
        };
        let port_json = serde_json::to_value(&port).unwrap();
        assert_eq!(port_json["scheme"], "https");
        assert_eq!(
            serde_json::from_value::<crate::methods::network::PublishedPort>(port_json).unwrap(),
            port
        );
    }

    #[test]
    fn native_display_messages_use_protocol_tag_and_bounded_axes() {
        use crate::methods::display::{DisplayAxis, NativeDisplayMessage};

        let message = NativeDisplayMessage::Abs {
            x: DisplayAxis::new(0).unwrap(),
            y: DisplayAxis::new(32_767).unwrap(),
        };
        let value = serde_json::to_value(&message).unwrap();
        assert_eq!(value, serde_json::json!({"t":"abs","x":0,"y":32767}));
        assert_eq!(
            serde_json::from_value::<NativeDisplayMessage>(value).unwrap(),
            message
        );
        assert!(DisplayAxis::new(32_768).is_err());
        assert!(serde_json::from_value::<DisplayAxis>(serde_json::json!(32_768)).is_err());
    }
}
