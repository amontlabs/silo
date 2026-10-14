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
                outcome_chunked: false,
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
    fn incompatible_version_diagnostics_have_canonical_field_names() {
        let error = ErrorObject {
            code: ErrorCode::IncompatibleVersion,
            message: "No common protocol version".to_owned(),
            diagnostics: Some(crate::transport::IncompatibleVersionDiagnostics {
                engine_protocol_range: crate::transport::ProtocolRange { min: 6, max: 6 },
                client_protocol_range: crate::transport::ProtocolRange { min: 5, max: 5 },
                engine_app_version: "0.1.0".to_owned(),
                client_app_version: Some("9.8.7".to_owned()),
            }),
        };
        let value = serde_json::to_value(&error).unwrap();

        assert_eq!(
            value,
            serde_json::json!({
                "code": "incompatible_version",
                "message": "No common protocol version",
                "diagnostics": {
                    "engineProtocolRange": { "min": 6, "max": 6 },
                    "clientProtocolRange": { "min": 5, "max": 5 },
                    "engineAppVersion": "0.1.0",
                    "clientAppVersion": "9.8.7"
                }
            })
        );
        assert_eq!(serde_json::from_value::<ErrorObject>(value).unwrap(), error);
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
            (DesktopUpdateRequired, "desktop_update_required"),
            (CredentialAccessDenied, "credential_access_denied"),
        ];
        assert_eq!(codes.len(), 25);
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
            schema_version: Some(1),
            state: Some(crate::methods::computer_use::ComputerUseInstallState::Ready),
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
            chatgpt_version: None,
            runtime_version: None,
            lcu_version: None,
            agents: Some(vec!["agent-a".into()]),
            at: None,
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

    #[test]
    fn log_flags_and_optional_page_values_match_native_serde_shapes() {
        use crate::methods::logs::LogPage;

        let payload = serde_json::json!({
            "entries": [{
                "id": "log-1",
                "line": "kernel: started",
                "occurredAt": "2026-10-14T08:15:00Z",
                "computerId": "computer-1",
                "computerName": "Builder",
                "deviceId": "device-1",
                "deviceName": "Mac",
                "source": "kernel",
                "session": null,
                "guestTimestamp": true
            }],
            "nextCursor": null,
            "oldestAvailableTimestamp": null,
            "newestAvailableTimestamp": null,
            "totalMatches": 1,
            "timestampEstimated": false,
            "unreadableRecords": true,
            "snapshot": null
        });
        let page: LogPage = serde_json::from_value(payload).unwrap();
        let output = serde_json::to_value(page).unwrap();
        assert_eq!(output["entries"][0]["guestTimestamp"], true);
        assert_eq!(output["unreadableRecords"], true);
        assert!(output.get("unsupported").is_none());
        assert_eq!(output["nextCursor"], serde_json::Value::Null);
        assert_eq!(output["oldestAvailableTimestamp"], serde_json::Value::Null);
        assert_eq!(output["newestAvailableTimestamp"], serde_json::Value::Null);

        let schema: serde_json::Value = serde_json::from_str(&schema_json().unwrap()).unwrap();
        for (definition, field) in [
            ("LogPage", "unsupported"),
            ("LogPage", "unreadableRecords"),
            ("LogEntry", "guestTimestamp"),
        ] {
            assert_eq!(
                schema["$defs"][definition]["properties"][field]["type"],
                "boolean"
            );
        }

        let without_false_flags: LogPage = serde_json::from_value(serde_json::json!({
            "entries": [{
                "id": "log-2", "line": "ready", "occurredAt": "2026-10-14T08:15:01Z",
                "computerId": "computer-1", "computerName": "Builder",
                "deviceId": "device-1", "deviceName": "Mac", "source": "stdout",
                "session": null
            }],
            "totalMatches": 1,
            "timestampEstimated": false
        }))
        .unwrap();
        let output = serde_json::to_value(without_false_flags).unwrap();
        assert!(output.get("unsupported").is_none());
        assert!(output.get("unreadableRecords").is_none());
        assert!(output["entries"][0].get("guestTimestamp").is_none());
    }

    #[test]
    fn storage_timestamps_are_numeric_and_nullable_outputs_stay_present() {
        use crate::methods::storage::StorageState;

        let payload = serde_json::json!({
            "history": [{"at": 1_728_896_100, "trigger": "manual", "reclaimedBytes": null, "error": null}],
            "workspaceHostBytes": null,
            "runtimeHostBytes": null,
            "checkpointHostBytes": null,
            "checkpointCount": 0,
            "workspaceUsedBytes": null,
            "workspaceCapacityBytes": null,
            "lastReclaimedBytes": null,
            "lastTrimAt": 1_728_896_100,
            "lastError": null
        });
        let state: StorageState = serde_json::from_value(payload.clone()).unwrap();
        let output = serde_json::to_value(state).unwrap();
        assert_eq!(output, payload);
        assert!(serde_json::from_value::<StorageState>(serde_json::json!({
            "history": [{"at": "2026-10-14T08:15:00Z", "trigger": "manual"}],
            "checkpointCount": 0
        }))
        .is_err());
    }

    #[test]
    fn passing_dependency_check_preserves_null_remediation() {
        use crate::methods::device::DependencyCheck;

        let payload = serde_json::json!({
            "id": "ssh", "title": "OpenSSH", "status": "pass",
            "detail": "Available", "remediation": null
        });
        let check: DependencyCheck = serde_json::from_value(payload.clone()).unwrap();
        assert_eq!(serde_json::to_value(check).unwrap(), payload);
    }

    #[test]
    fn pending_restore_variants_use_native_camel_case_fields() {
        use crate::methods::checkpoints::PendingRestore;

        let linux_payload = serde_json::json!({
            "checkpointId": "checkpoint-1", "sourceComputer": "Builder", "state": "full"
        });
        let linux: PendingRestore = serde_json::from_value(linux_payload.clone()).unwrap();
        assert_eq!(serde_json::to_value(linux).unwrap(), linux_payload);

        let macos_payload = serde_json::json!({"checkpointId": "checkpoint-2", "memory": true});
        let macos: PendingRestore = serde_json::from_value(macos_payload.clone()).unwrap();
        assert_eq!(serde_json::to_value(macos).unwrap(), macos_payload);
    }

    #[test]
    fn unavailable_github_catalog_uses_camel_case_retry_field() {
        use crate::methods::github::GitHubRepositoryCatalogStatus;

        let payload = serde_json::json!({
            "status": "unavailable", "message": "Catalog unavailable", "canRetry": true
        });
        let status: GitHubRepositoryCatalogStatus =
            serde_json::from_value(payload.clone()).unwrap();
        assert_eq!(serde_json::to_value(status).unwrap(), payload);
    }

    #[test]
    fn macos_resize_uses_pixel_names_and_native_integer_widths() {
        use crate::methods::macos::{MacosDisplayResizeParams, MacosDisplayResizeResult};

        let request = serde_json::json!({
            "computerId": "computer-1", "widthPx": 4_294_967_296_u64, "heightPx": 1080
        });
        let params: MacosDisplayResizeParams = serde_json::from_value(request.clone()).unwrap();
        assert_eq!(serde_json::to_value(params).unwrap(), request);

        let remote = serde_json::json!({
            "computerId": "computer-1", "widthPx": 1920, "heightPx": 1080, "deviceId": "mac-mini"
        });
        let params: MacosDisplayResizeParams = serde_json::from_value(remote.clone()).unwrap();
        assert_eq!(params.device_id.as_deref(), Some("mac-mini"));
        assert_eq!(serde_json::to_value(params).unwrap(), remote);

        let response = serde_json::json!({"widthPx": 1920, "heightPx": 1080});
        let result: MacosDisplayResizeResult = serde_json::from_value(response.clone()).unwrap();
        assert_eq!(serde_json::to_value(result).unwrap(), response);
    }

    #[test]
    fn desktop_status_accepts_nullable_component_and_display_fields() {
        use crate::methods::desktop::DesktopStatus;

        let payload = serde_json::json!({
            "installed": true, "state": "stopped", "autoStart": false,
            "version": null, "user": null, "display": null, "backend": null,
            "sessionState": null, "streamState": null,
            "updateRequired": false, "updateAvailable": false, "streamerVersion": null,
            "computerUse": null, "name": null, "agentTools": null
        });
        let status: DesktopStatus = serde_json::from_value(payload.clone()).unwrap();
        let output = serde_json::to_value(status).unwrap();
        assert_eq!(output, payload);
        assert!(serde_json::from_value::<DesktopStatus>(serde_json::json!({
            "installed": true, "state": "running", "autoStart": false,
            "display": {"name": ":1"}, "updateRequired": false, "updateAvailable": false
        }))
        .is_err());
    }

    #[test]
    fn desktop_status_carries_the_agent_tools() {
        use crate::methods::desktop::{AgentToolsState, DesktopStatus};

        let payload = serde_json::json!({
            "installed": true, "state": "running", "autoStart": true,
            "version": null, "user": null, "display": null, "backend": null,
            "sessionState": null, "streamState": null,
            "updateRequired": false, "updateAvailable": false, "streamerVersion": null,
            "computerUse": null, "name": null,
            "agentTools": {
                "state": "missing", "missing": ["claude"],
                "nodeVersion": "24.21.0", "claudeVersion": null,
                "expectedNodeVersion": "24.21.0", "expectedClaudeVersion": "2.1.287",
                "recipeVersion": null, "expectedRecipeVersion": 1
            }
        });
        let status: DesktopStatus = serde_json::from_value(payload.clone()).unwrap();
        assert_eq!(status.agent_tools.as_ref().unwrap().state, AgentToolsState::Missing);
        assert_eq!(serde_json::to_value(status).unwrap(), payload);
    }

    #[test]
    fn expected_sha256_distinguishes_omitted_null_and_string() {
        use crate::methods::guest::GuestFilesWriteParams;

        let base = serde_json::json!({
            "computerId": "computer-1", "path": "/workspace/output.txt", "data": [1, 2],
            "mode": 420, "replace": "ifSha256"
        });
        let omitted: GuestFilesWriteParams = serde_json::from_value(base.clone()).unwrap();
        assert_eq!(omitted.expected_sha256, None);
        assert!(serde_json::to_value(omitted)
            .unwrap()
            .get("expectedSha256")
            .is_none());

        let mut absent_payload = base.clone();
        absent_payload["expectedSha256"] = serde_json::Value::Null;
        let absent: GuestFilesWriteParams = serde_json::from_value(absent_payload.clone()).unwrap();
        assert_eq!(absent.expected_sha256, Some(None));
        assert_eq!(serde_json::to_value(absent).unwrap(), absent_payload);

        let mut matched_payload = base;
        matched_payload["expectedSha256"] = serde_json::json!("a".repeat(64));
        let matched: GuestFilesWriteParams =
            serde_json::from_value(matched_payload.clone()).unwrap();
        assert_eq!(matched.expected_sha256, Some(Some("a".repeat(64))));
        assert_eq!(serde_json::to_value(matched).unwrap(), matched_payload);
    }

    #[test]
    fn force_stop_uses_canonical_null_result_without_retired_remote_snapshot() {
        let result = MethodResults::ComputersForceStop(());
        let value = serde_json::to_value(&result).unwrap();
        assert_eq!(
            value,
            serde_json::json!({"method": "computers.forceStop", "result": null})
        );
        assert_eq!(
            serde_json::from_value::<MethodResults>(value).unwrap(),
            result
        );
    }

    #[test]
    fn clipboard_too_large_discriminator_is_kebab_case() {
        use crate::methods::clipboard::ClipboardContent;

        let value = serde_json::to_value(ClipboardContent::TooLarge).unwrap();
        assert_eq!(value, serde_json::json!({"kind": "too-large"}));
        assert_eq!(
            serde_json::from_value::<ClipboardContent>(value).unwrap(),
            ClipboardContent::TooLarge
        );
    }

    #[test]
    fn handshake_capabilities_accept_unrecognized_strings() {
        let payload = serde_json::json!({
            "deviceId": "device-1", "name": "Mac", "protocol": 1,
            "capabilities": ["engine.handshake", "future.experimental"]
        });
        let result: HandshakeResult = serde_json::from_value(payload.clone()).unwrap();
        assert_eq!(serde_json::to_value(result).unwrap(), payload);
    }

    #[test]
    fn computer_use_state_round_trips_macos_receipt_and_unread_state() {
        use crate::methods::computer_use::ComputerUseState;

        let receipt = serde_json::json!({
            "schemaVersion": 1, "state": "ready", "reason": "",
            "chatgptVersion": "1.2026.101", "lcuVersion": "0.11.0",
            "approval": "ask", "at": "2026-10-14T08:15:00Z"
        });
        let state: ComputerUseState = serde_json::from_value(receipt).unwrap();
        let value = serde_json::to_value(state).unwrap();
        assert_eq!(value["schemaVersion"], 1);
        assert_eq!(value["chatgptVersion"], "1.2026.101");
        assert_eq!(value["at"], "2026-10-14T08:15:00Z");
        assert_eq!(value["state"], "ready");

        let before_receipt: ComputerUseState = serde_json::from_value(serde_json::json!({
            "schemaVersion": null, "state": null, "chatgptVersion": null, "at": null
        }))
        .unwrap();
        let before_receipt = serde_json::to_value(before_receipt).unwrap();
        assert!(before_receipt["schemaVersion"].is_null());
        assert!(before_receipt["state"].is_null());
        assert!(before_receipt["chatgptVersion"].is_null());
        assert!(before_receipt["at"].is_null());
    }

    #[test]
    fn migration_stage_is_a_required_native_string() {
        use crate::methods::migration::MigrationState;

        let payload = serde_json::json!({
            "version": 1, "status": "scanning", "stage": "Checking existing computers",
            "logs": [], "migratedCount": 0, "failedCount": 0, "totalCount": 0,
            "canContinue": false
        });
        let state: MigrationState = serde_json::from_value(payload.clone()).unwrap();
        assert_eq!(serde_json::to_value(state).unwrap(), payload);
        let mut invalid = payload;
        invalid["stage"] = serde_json::Value::Null;
        assert!(serde_json::from_value::<MigrationState>(invalid).is_err());
    }
}
