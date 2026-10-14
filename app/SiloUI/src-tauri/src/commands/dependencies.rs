use tauri::Manager;

use crate::dependencies::{self, DependencyReport, ProbePaths};

#[tauri::command]
pub(crate) async fn read_dependencies(
    app: tauri::AppHandle,
    request_id: String,
) -> Result<DependencyReport, String> {
    eprintln!("dependency check request: {request_id}");
    dependencies::validate_request_id(&request_id)?;

    let executable_dir = crate::bundled_tools::directory(&app)?;
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|error| format!("Silo resource directory is unavailable: {error}"))?;
    #[cfg(target_os = "macos")]
    let frameworks_dir = executable_dir
        .parent()
        .map(|contents| contents.join("Frameworks"));
    #[cfg(not(target_os = "macos"))]
    let frameworks_dir = None;
    #[cfg(target_os = "linux")]
    let runtime_library = Some(crate::runtime::bundled_runtime_library(
        &executable_dir.join("msb"),
        &resource_dir,
        tauri::utils::platform::bundle_type(),
    ));

    tokio::task::spawn_blocking(move || {
        dependencies::read_report(
            request_id,
            ProbePaths {
                executable_dir,
                resource_dir,
                frameworks_dir,
                #[cfg(target_os = "linux")]
                runtime_library,
            },
        )
    })
    .await
    .map_err(|error| format!("Dependency checks could not run: {error}"))
}
