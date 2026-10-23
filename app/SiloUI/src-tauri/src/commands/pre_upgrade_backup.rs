//! The backup taken before an upgrade, through `migration.preUpgradeBackup.*`. Showing it in
//! the file manager stays on this device.

use std::path::PathBuf;

use serde_json::{Map, Value};
use tauri::WebviewWindow;

use crate::engine_client::{
    forwarding::not_found, params::device, Engine, EngineClient, EngineError,
};
use silo_api::methods::requests;

fn require_main(window: &WebviewWindow) -> Result<(), EngineError> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err(EngineError::internal(
            "Only the main window can manage the pre-upgrade backup.",
        ))
    }
}

/// What the UI shows of the backup: when it is deleted and whether the notice is pending.
/// Where it lives is for revealing only.
fn status_for_ui(backup: &Value) -> Value {
    if !backup.is_object() {
        return Value::Null;
    }
    let mut status = Map::new();
    for key in ["deleteAt", "noticePending"] {
        status.insert(
            key.to_owned(),
            backup.get(key).cloned().unwrap_or(Value::Null),
        );
    }
    Value::Object(status)
}

pub(crate) async fn read(engine: &EngineClient) -> Result<Value, EngineError> {
    let backup = engine
        .call(requests::migration_pre_upgrade_backup_get(device(None)))
        .await?;
    Ok(status_for_ui(&backup))
}

pub(crate) async fn measure(engine: &EngineClient) -> Result<Value, EngineError> {
    engine
        .call(requests::migration_pre_upgrade_backup_measure(device(None)))
        .await
}

pub(crate) async fn delete(engine: &EngineClient) -> Result<(), EngineError> {
    engine
        .call(requests::migration_pre_upgrade_backup_delete(device(None)))
        .await?;
    Ok(())
}

pub(crate) async fn acknowledge_notice(engine: &EngineClient) -> Result<(), EngineError> {
    engine
        .call(requests::migration_pre_upgrade_backup_acknowledge_notice(
            device(None),
        ))
        .await?;
    Ok(())
}

pub(crate) async fn reveal_path(engine: &EngineClient) -> Result<PathBuf, EngineError> {
    engine
        .call(requests::migration_pre_upgrade_backup_get(device(None)))
        .await?
        .get("path")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .ok_or_else(|| not_found("There is no pre-upgrade backup to show."))
}

/// Whether a pre-upgrade backup exists and when it is deleted. Cheap: it measures nothing.
#[tauri::command]
pub(crate) async fn read_pre_upgrade_backup(
    engine: Engine<'_>,
    window: WebviewWindow,
) -> Result<Value, EngineError> {
    require_main(&window)?;
    read(&engine).await
}

/// Allocated bytes of the backup, or `None` once it is gone. Walks the folder.
#[tauri::command]
pub(crate) async fn measure_pre_upgrade_backup(
    engine: Engine<'_>,
    window: WebviewWindow,
) -> Result<Value, EngineError> {
    require_main(&window)?;
    measure(&engine).await
}

#[tauri::command]
pub(crate) async fn delete_pre_upgrade_backup(
    engine: Engine<'_>,
    window: WebviewWindow,
) -> Result<(), EngineError> {
    require_main(&window)?;
    delete(&engine).await
}

#[tauri::command]
pub(crate) async fn reveal_pre_upgrade_backup(
    engine: Engine<'_>,
    window: WebviewWindow,
) -> Result<(), EngineError> {
    require_main(&window)?;
    let previous = reveal_path(&engine).await?;
    tauri_plugin_opener::reveal_item_in_dir(&previous).map_err(|error| {
        EngineError::internal(format!(
            "Silo could not show the pre-upgrade backup: {error}"
        ))
    })
}

/// The migration screen told the user about the backup.
#[tauri::command]
pub(crate) async fn acknowledge_pre_upgrade_backup_notice(
    engine: Engine<'_>,
    window: WebviewWindow,
) -> Result<(), EngineError> {
    require_main(&window)?;
    acknowledge_notice(&engine).await
}

#[cfg(test)]
mod forwarding_tests {
    use super::*;
    use crate::engine_client::testing::{connected_client, FakeEngine};
    use serde_json::json;
    use silo_api::methods::MethodName;

    fn replies(method: MethodName, _params: &Value) -> Result<Value, (&'static str, String)> {
        match method {
            MethodName::EventsSubscribe => Ok(json!({"subscription": "sub"})),
            MethodName::MigrationPreUpgradeBackupGet => Ok(json!({
                "path": "/data/pre-upgrade", "deleteAt": "2030-01-01T00:00:00Z", "noticePending": true
            })),
            MethodName::MigrationPreUpgradeBackupMeasure => Ok(json!(4096)),
            _ => Ok(Value::Null),
        }
    }

    #[tokio::test]
    async fn the_status_hides_the_path_and_the_path_is_for_revealing() {
        let fake = FakeEngine::start(|method, params| replies(method, params));
        let (engine, _) = connected_client(&fake).await;
        assert_eq!(
            read(&engine).await.unwrap(),
            json!({"deleteAt": "2030-01-01T00:00:00Z", "noticePending": true})
        );
        assert_eq!(
            reveal_path(&engine).await.unwrap(),
            PathBuf::from("/data/pre-upgrade")
        );
        assert_eq!(measure(&engine).await.unwrap(), json!(4096));
        delete(&engine).await.unwrap();
        acknowledge_notice(&engine).await.unwrap();
        assert_eq!(
            fake.calls(MethodName::MigrationPreUpgradeBackupDelete)
                .len(),
            1
        );
        assert_eq!(
            fake.calls(MethodName::MigrationPreUpgradeBackupAcknowledgeNotice)
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn no_backup_is_null_and_has_nothing_to_reveal() {
        let fake = FakeEngine::start(|method, params| match method {
            MethodName::MigrationPreUpgradeBackupGet => Ok(Value::Null),
            _ => replies(method, params),
        });
        let (engine, _) = connected_client(&fake).await;
        assert!(read(&engine).await.unwrap().is_null());
        assert_eq!(reveal_path(&engine).await.unwrap_err().code, "not_found");
    }

    #[tokio::test]
    async fn engine_errors_pass_through() {
        let fake = FakeEngine::start(|method, params| match method {
            MethodName::MigrationPreUpgradeBackupDelete => {
                Err(("busy", "A migration is running.".into()))
            }
            _ => replies(method, params),
        });
        let (engine, _) = connected_client(&fake).await;
        assert_eq!(delete(&engine).await.unwrap_err().code, "busy");
    }
}
