use std::path::PathBuf;
use tauri::Manager;

use crate::bundled_tools::BundleKind;

pub(crate) fn directory(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let executable = std::env::current_exe()
        .map_err(|_| "Silo could not locate its bundled tools.".to_string())?;
    let resources = app
        .path()
        .resource_dir()
        .map_err(|_| "Silo could not locate its bundled resources.".to_string())?;
    let appimage_root = std::env::var_os("APPDIR").map(PathBuf::from);
    crate::bundled_tools::resolve(
        &executable,
        &resources,
        bundle_kind(tauri::utils::platform::bundle_type()),
        appimage_root.as_deref(),
    )
}

pub(crate) fn bundle_kind(bundle: Option<tauri::utils::config::BundleType>) -> Option<BundleKind> {
    use tauri::utils::config::BundleType;

    bundle.map(|bundle| match bundle {
        BundleType::App => BundleKind::App,
        BundleType::AppImage => BundleKind::AppImage,
        BundleType::Deb => BundleKind::Deb,
        BundleType::Rpm => BundleKind::Rpm,
        _ => BundleKind::Other,
    })
}
