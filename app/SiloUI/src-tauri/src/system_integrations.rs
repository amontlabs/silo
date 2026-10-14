use crate::notifications::NotificationDelivery;
use serde::Serialize;

pub(crate) const ROUTE_KEY: &str = "route";

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
pub(crate) mod macos;
#[cfg(any(test, target_os = "linux"))]
mod notification_ids;
#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(target_os = "macos")]
use macos as platform;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationStatus {
    pub state: String,
    pub error: Option<String>,
}

impl IntegrationStatus {
    fn new(state: &str) -> Self {
        Self {
            state: state.into(),
            error: None,
        }
    }

    fn error(message: impl Into<String>) -> Self {
        Self {
            state: "error".into(),
            error: Some(message.into()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemIntegrations {
    pub platform: &'static str,
    pub login_item: IntegrationStatus,
    pub notifications: IntegrationStatus,
}

pub(crate) fn require_main_label(label: &str) -> Result<(), String> {
    if label == "main" {
        Ok(())
    } else {
        Err("Only the main window can change system integrations".into())
    }
}

pub(crate) fn read() -> SystemIntegrations {
    platform::read()
}

pub(crate) fn set_login_item(enabled: bool) -> Result<IntegrationStatus, String> {
    platform::set_login_item(enabled)
}

pub(crate) fn request_notification_authorization() -> Result<IntegrationStatus, String> {
    platform::request_notifications()
}

pub(crate) fn open_settings(integration: &str) -> Result<(), String> {
    platform::open_settings(integration)
}

pub(crate) use crate::commands::system_integrations::{
    install_notifications, show_integration_error,
};

/// Delivery never requests authorization or opens a permission prompt. A newer notice
/// with the same key replaces the older one.
pub(crate) fn deliver_notification(
    notice: &crate::notifications::Notice,
) -> Result<NotificationDelivery, String> {
    platform::deliver_notification(notice)
}

/// Withdraw delivered notifications by key. Best effort.
pub(crate) fn clear_notifications(keys: &[String]) {
    platform::clear_notifications(keys);
}

fn notification_authorized(state: &str) -> bool {
    matches!(state, "authorized" | "provisional")
}

/// The Linux packages install `{productName}.desktop` and an icon named after the
/// main binary (tauri-bundler). Notification servers use the desktop entry to
/// attribute notices and list Silo in per-app settings (F-24).
#[cfg_attr(not(any(test, target_os = "linux")), allow(dead_code))]
pub(crate) fn notification_desktop_entry() -> &'static str {
    crate::channel::current().notification_desktop_entry()
}
#[cfg_attr(not(any(test, target_os = "linux")), allow(dead_code))]
pub(crate) const NOTIFICATION_ICON: &str = env!("CARGO_PKG_NAME");

/// Created by the Debian preinst and removed by postinst.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) const PACKAGE_UPDATE_MARKER: &str = "/var/lib/silo/package-update-in-progress";
/// A marker older than this is left over from an interrupted update.
#[cfg_attr(not(any(test, target_os = "linux")), allow(dead_code))]
const PACKAGE_UPDATE_STALE_AFTER: std::time::Duration = std::time::Duration::from_secs(30 * 60);

/// Why Silo did not open while its package update marker exists (F-11).
#[cfg_attr(not(any(test, target_os = "linux")), allow(dead_code))]
fn package_update_notice(age: Option<std::time::Duration>, dpkg_running: bool) -> &'static str {
    if dpkg_running && age.is_some_and(|age| age < PACKAGE_UPDATE_STALE_AFTER) {
        "Silo is being updated. Wait for the package update to finish, then open Silo again."
    } else {
        "A Silo package update did not finish. When no other software update is running, open a terminal and run:\n\nsudo dpkg --configure -a\n\nThen open Silo again."
    }
}

/// Whether a dpkg process is running (package maintainer scripts run under it).
#[cfg_attr(not(any(test, target_os = "linux")), allow(dead_code))]
fn dpkg_running(proc: &std::path::Path) -> bool {
    let Ok(entries) = std::fs::read_dir(proc) else {
        return false;
    };
    entries.flatten().any(|entry| {
        entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.bytes().all(|byte| byte.is_ascii_digit()))
            && std::fs::read_to_string(entry.path().join("comm"))
                .is_ok_and(|comm| comm.trim_end() == "dpkg")
    })
}

/// Show why Silo will not open during an unfinished package update, then return.
#[cfg(target_os = "linux")]
pub(crate) fn explain_unfinished_package_update() {
    let age = std::fs::metadata(PACKAGE_UPDATE_MARKER)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.elapsed().ok());
    let running = dpkg_running(std::path::Path::new("/proc"));
    platform::show_startup_notice(package_update_notice(age, running));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_confirmed_notification_authorization_allows_delivery() {
        for state in ["authorized", "provisional"] {
            assert!(notification_authorized(state));
        }
        for state in ["notDetermined", "denied", "unavailable", "error", "unknown"] {
            assert!(!notification_authorized(state));
        }
    }

    #[test]
    fn package_update_notice_tells_how_to_finish_an_interrupted_update() {
        let recent = Some(std::time::Duration::from_secs(60));
        let stale = Some(std::time::Duration::from_secs(2 * 60 * 60));
        assert!(package_update_notice(recent, true).contains("being updated"));
        for (age, dpkg) in [
            (recent, false),
            (stale, true),
            (stale, false),
            (None, true),
            (None, false),
        ] {
            let notice = package_update_notice(age, dpkg);
            assert!(
                notice.contains("sudo dpkg --configure -a"),
                "{age:?} {dpkg}"
            );
            assert!(
                !notice.contains('%'),
                "GTK dialogs must not see format directives"
            );
        }
    }

    #[test]
    fn notifications_name_the_installed_desktop_entry_and_icon() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(config["productName"], notification_desktop_entry());
        assert!(
            config["mainBinaryName"].is_null(),
            "the icon is named after the Cargo binary"
        );
        assert_eq!(NOTIFICATION_ICON, "silo-ui");
    }

    #[test]
    fn dpkg_is_found_only_among_numeric_process_entries() {
        let root = tempfile::tempdir().unwrap();
        for (entry, comm) in [("1", "systemd\n"), ("self", "dpkg\n"), ("42", "apt\n")] {
            std::fs::create_dir(root.path().join(entry)).unwrap();
            std::fs::write(root.path().join(entry).join("comm"), comm).unwrap();
        }
        assert!(!dpkg_running(root.path()));
        std::fs::create_dir(root.path().join("77")).unwrap();
        std::fs::write(root.path().join("77/comm"), "dpkg\n").unwrap();
        assert!(dpkg_running(root.path()));
        assert!(!dpkg_running(&root.path().join("missing")));
    }

    #[test]
    fn integration_mutations_are_main_window_only() {
        assert!(require_main_label("main").is_ok());
        assert!(require_main_label("status").is_err());
        assert!(require_main_label("other").is_err());
    }
}
