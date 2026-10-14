use crate::{host_push, host_push_operations, remote, remote_access, runtime};
use serde_json::{json, Value};
use tauri::AppHandle;

#[tauri::command]
pub(crate) async fn dismiss_repository_push(
    app: AppHandle,
    computer: String,
    repository_path: String,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        if let Some((device, computer)) = remote_access::target(&computer)? {
            return remote::call_remote(
                &device,
                "repository.dismiss",
                json!({"computerId": computer, "path": repository_path}),
            )
            .map(|_| ())
            .map_err(|_| "Remote repository request failed.".to_owned());
        }
        let paths = runtime::runtime_paths(&crate::commands::runtime::host(&app))?;
        host_push::dismiss_repository_push(&paths, &computer, &repository_path)
    })
    .await
    .map_err(|_| "Remote repository request failed.".to_owned())?
}

#[tauri::command]
pub(crate) async fn start_repository_push(
    app: AppHandle,
    computer: String,
    repository_path: String,
    operation_id: String,
    target: host_push::PushTarget,
) -> Result<Value, String> {
    tokio::task::spawn_blocking(move || {
        target.validate()?;
        if let Some((device, computer)) = remote_access::target(&computer)? {
            return match remote::call_remote(
                &device,
                "repository.push.start",
                json!({"computerId": computer, "path": repository_path, "operationId": operation_id, "target": target}),
            ) {
                Err(message) if message == "This Silo version does not support that remote operation." => Ok(json!({"operationId":operation_id,"computer":computer,"repositoryPath":repository_path,"status":"failed","commitCount":0,"message":"Update Silo on the remote device before pushing."})),
                result => result,
            };
        }
        let paths = runtime::runtime_paths(&crate::commands::runtime::host(&app))?;
        Ok(host_push_operations::start_result(
            &paths,
            computer,
            repository_path,
            operation_id,
            target,
        ))
    })
    .await
    .map_err(|_| "Cannot start observing the push.".to_owned())?
}

#[tauri::command]
pub(crate) async fn repository_push_status(
    app: AppHandle,
    computer: String,
    repository_path: String,
    operation_id: String,
) -> Result<Value, String> {
    tokio::task::spawn_blocking(move || {
        if let Some((device, computer)) = remote_access::target(&computer)? {
            return remote::call_remote(
                &device,
                "repository.push.status",
                json!({"computerId": computer, "path": repository_path, "operationId": operation_id}),
            );
        }
        let paths = runtime::runtime_paths(&crate::commands::runtime::host(&app))?;
        host_push_operations::status(&paths, &computer, &repository_path, &operation_id)
    })
    .await
    .map_err(|_| "Cannot read push status.".to_owned())?
}
