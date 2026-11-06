//! Linux without the `desktop` feature: a headless host has no session bus, autostart
//! directory or notification server to integrate with, so each integration reads as
//! unavailable and delivery is skipped.

use super::{IntegrationStatus, SystemIntegrations};

pub fn read() -> SystemIntegrations {
    SystemIntegrations {
        platform: "linux",
        login_item: IntegrationStatus::new("unavailable"),
        notifications: IntegrationStatus::new("unavailable"),
    }
}

pub fn set_login_item(_enabled: bool) -> Result<IntegrationStatus, String> {
    Err("This Linux host has no desktop session to start Silo with.".into())
}

pub fn show_startup_notice(message: &str) {
    eprintln!("{message}");
}

pub fn request_notifications() -> Result<IntegrationStatus, String> {
    Ok(IntegrationStatus::new("unavailable"))
}

pub fn deliver_notification(
    _notice: &crate::notifications::Notice,
) -> Result<crate::notifications::NotificationDelivery, String> {
    Ok(crate::notifications::NotificationDelivery::Skipped)
}

pub fn clear_notifications(_keys: &[String]) {}

pub fn open_settings(_integration: &str) -> Result<(), String> {
    Err("This Linux host has no desktop settings.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_headless_host_reports_every_integration_unavailable() {
        let status = read();
        assert_eq!(status.login_item.state, "unavailable");
        assert_eq!(status.notifications.state, "unavailable");
        assert!(set_login_item(true).is_err());
    }
}
