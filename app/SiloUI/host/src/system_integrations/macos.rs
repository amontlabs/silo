use std::sync::mpsc;
use std::time::Duration;

use block2::RcBlock;
use objc2::runtime::Bool;
use objc2_app_kit::NSWorkspace;
use objc2_foundation::{
    NSBundle, NSError, NSOperatingSystemVersion, NSProcessInfo, NSString, NSURL,
};
use objc2_service_management::{SMAppService, SMAppServiceStatus};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNAuthorizationStatus, UNUserNotificationCenter,
};

use super::{IntegrationStatus, SystemIntegrations};

const CALLBACK_TIMEOUT: Duration = Duration::from_secs(30);

fn macos_10_14_or_newer() -> bool {
    NSProcessInfo::processInfo().isOperatingSystemAtLeastVersion(NSOperatingSystemVersion {
        majorVersion: 10,
        minorVersion: 14,
        patchVersion: 0,
    })
}

/// UserNotifications requires a real application bundle: it throws (and aborts) when the
/// process runs as a bare executable during development. Treat notifications as
/// unavailable there instead of crashing.
pub fn notifications_supported() -> bool {
    macos_10_14_or_newer()
        && NSBundle::mainBundle()
            .bundlePath()
            .to_string()
            .ends_with(".app")
}

fn macos_13_or_newer() -> bool {
    NSProcessInfo::processInfo().isOperatingSystemAtLeastVersion(NSOperatingSystemVersion {
        majorVersion: 13,
        minorVersion: 0,
        patchVersion: 0,
    })
}

fn map_login_status(status: SMAppServiceStatus) -> IntegrationStatus {
    match status {
        SMAppServiceStatus::Enabled => IntegrationStatus::new("enabled"),
        SMAppServiceStatus::NotRegistered => IntegrationStatus::new("notRegistered"),
        SMAppServiceStatus::RequiresApproval => IntegrationStatus::new("requiresApproval"),
        SMAppServiceStatus::NotFound => IntegrationStatus::new("notFound"),
        other => IntegrationStatus::error(format!(
            "macOS returned an unknown login-item status ({})",
            other.0
        )),
    }
}

fn login_item() -> IntegrationStatus {
    if !macos_13_or_newer() {
        return IntegrationStatus::new("unavailable");
    }
    // SAFETY: The availability check above guards every macOS 13 SMAppService call.
    let status = unsafe { SMAppService::mainAppService().status() };
    map_login_status(status)
}

fn error_text(error: &NSError) -> String {
    error.localizedDescription().to_string()
}

fn map_notification_status(status: UNAuthorizationStatus) -> IntegrationStatus {
    match status {
        UNAuthorizationStatus::NotDetermined => IntegrationStatus::new("notDetermined"),
        UNAuthorizationStatus::Denied => IntegrationStatus::new("denied"),
        UNAuthorizationStatus::Authorized => IntegrationStatus::new("authorized"),
        UNAuthorizationStatus::Provisional => IntegrationStatus::new("provisional"),
        other => IntegrationStatus::error(format!(
            "macOS returned an unknown notification authorization status ({})",
            other.0
        )),
    }
}

fn notification_status() -> IntegrationStatus {
    if !notifications_supported() {
        return IntegrationStatus::new("unavailable");
    }
    let center = UNUserNotificationCenter::currentNotificationCenter();
    let (send, receive) = mpsc::sync_channel(1);
    let handler = RcBlock::new(
        move |settings: std::ptr::NonNull<objc2_user_notifications::UNNotificationSettings>| {
            // SAFETY: UserNotifications guarantees a valid settings object for this callback.
            let status = unsafe { settings.as_ref() }.authorizationStatus();
            let _ = send.send(status);
        },
    );
    center.getNotificationSettingsWithCompletionHandler(&handler);
    match receive.recv_timeout(CALLBACK_TIMEOUT) {
        Ok(status) => map_notification_status(status),
        Err(_) => IntegrationStatus::error("macOS did not return notification settings"),
    }
}

pub fn read() -> SystemIntegrations {
    SystemIntegrations {
        platform: "macos",
        login_item: login_item(),
        notifications: notification_status(),
    }
}

pub fn set_login_item(enabled: bool) -> Result<IntegrationStatus, String> {
    if !macos_13_or_newer() {
        return Ok(IntegrationStatus::new("unavailable"));
    }
    // SAFETY: The availability check guards SMAppService, and calls use Apple's main-app singleton.
    let service = unsafe { SMAppService::mainAppService() };
    let before = unsafe { service.status() };
    let already_requested = if enabled {
        before == SMAppServiceStatus::Enabled || before == SMAppServiceStatus::RequiresApproval
    } else {
        before == SMAppServiceStatus::NotRegistered
    };
    if !already_requested {
        let result = if enabled {
            unsafe { service.registerAndReturnError() }
        } else {
            unsafe { service.unregisterAndReturnError() }
        };
        result.map_err(|error| error_text(&error))?;
    }
    let verified = login_item();
    let matches = if enabled {
        matches!(verified.state.as_str(), "enabled" | "requiresApproval")
    } else {
        verified.state == "notRegistered"
    };
    if matches {
        Ok(verified)
    } else {
        Err(format!(
            "macOS did not confirm the requested login-item state (reported {})",
            verified.state
        ))
    }
}

pub fn request_notifications() -> Result<IntegrationStatus, String> {
    if !notifications_supported() {
        return Ok(IntegrationStatus::new("unavailable"));
    }
    let before = notification_status();
    if before.state != "notDetermined" {
        return Ok(before);
    }
    let center = UNUserNotificationCenter::currentNotificationCenter();
    let (send, receive) = mpsc::sync_channel(1);
    let handler = RcBlock::new(move |granted: Bool, error: *mut NSError| {
        let result = if error.is_null() {
            Ok(granted.as_bool())
        } else {
            // SAFETY: A non-null NSError callback pointer remains valid for the callback.
            Err(error_text(unsafe { &*error }))
        };
        let _ = send.send(result);
    });
    center.requestAuthorizationWithOptions_completionHandler(
        UNAuthorizationOptions::Alert
            | UNAuthorizationOptions::Sound
            | UNAuthorizationOptions::Badge,
        &handler,
    );
    // The user owns this prompt and can leave it open indefinitely. This runs
    // on a blocking worker, so the app and Quit path remain responsive while
    // the UI keeps the switch pending and rejects duplicate requests.
    receive
        .recv()
        .map_err(|_| "macOS did not finish notification authorization".to_owned())??;
    let verified = notification_status();
    if verified.state == "error" {
        Err(verified
            .error
            .unwrap_or_else(|| "Notification settings are unavailable".into()))
    } else {
        Ok(verified)
    }
}

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_foundation::NSDictionary;

pub fn deliver_notification(
    notice: &crate::notifications::Notice,
) -> Result<super::NotificationDelivery, String> {
    if !super::notification_authorized(&notification_status().state) {
        return Ok(super::NotificationDelivery::Skipped);
    }
    use objc2_user_notifications::{UNMutableNotificationContent, UNNotificationRequest};
    let content = UNMutableNotificationContent::new();
    content.setTitle(&NSString::from_str(&notice.title));
    content.setBody(&NSString::from_str(&notice.body));
    content.setThreadIdentifier(&NSString::from_str(notice.thread()));
    let route = NSString::from_str(&notice.route().to_string());
    let info =
        NSDictionary::from_retained_objects(&[&*NSString::from_str(super::ROUTE_KEY)], &[route]);
    // SAFETY: The dictionary holds only NSString keys and values, which are property-list types.
    unsafe {
        content.setUserInfo(&Retained::cast_unchecked::<
            NSDictionary<AnyObject, AnyObject>,
        >(info))
    };
    // The key is the identifier: a newer notice with the same key replaces the older one.
    let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
        &NSString::from_str(&notice.key),
        &content,
        None,
    );
    let (send, receive) = mpsc::sync_channel(1);
    let handler = RcBlock::new(move |error: *mut NSError| {
        let _ = send.send(error.is_null());
    });
    UNUserNotificationCenter::currentNotificationCenter()
        .addNotificationRequest_withCompletionHandler(&request, Some(&handler));
    match receive.recv_timeout(CALLBACK_TIMEOUT) {
        Ok(true) => Ok(super::NotificationDelivery::Delivered),
        _ => Err("macOS could not schedule the notification".into()),
    }
}

pub fn clear_notifications(keys: &[String]) {
    if keys.is_empty() || !notifications_supported() {
        return;
    }
    let identifiers: Vec<Retained<NSString>> =
        keys.iter().map(|key| NSString::from_str(key)).collect();
    let identifiers = objc2_foundation::NSArray::from_retained_slice(&identifiers);
    let center = UNUserNotificationCenter::currentNotificationCenter();
    center.removeDeliveredNotificationsWithIdentifiers(&identifiers);
    center.removePendingNotificationRequestsWithIdentifiers(&identifiers);
}

pub fn open_settings(integration: &str) -> Result<(), String> {
    match integration {
        "loginItem" if macos_13_or_newer() => {
            // SAFETY: The availability check guards the macOS 13 selector.
            unsafe { SMAppService::openSystemSettingsLoginItems() };
            Ok(())
        }
        "loginItem" => Err("Login Items settings require macOS 13 or later".into()),
        "notifications" if macos_10_14_or_newer() => {
            let url = NSURL::URLWithString(&NSString::from_str(
                "x-apple.systempreferences:com.apple.Notifications-Settings.extension",
            ))
            .ok_or("Notification settings are unavailable")?;
            if NSWorkspace::sharedWorkspace().openURL(&url) {
                Ok(())
            } else {
                Err("Notification settings could not be opened".into())
            }
        }
        "notifications" => Err("Notification settings require macOS 10.14 or later".into()),
        _ => Err("Unknown system integration".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_status_values_remain_distinct() {
        assert_eq!(
            map_login_status(SMAppServiceStatus::Enabled).state,
            "enabled"
        );
        assert_eq!(
            map_login_status(SMAppServiceStatus::NotRegistered).state,
            "notRegistered"
        );
        assert_eq!(
            map_login_status(SMAppServiceStatus::RequiresApproval).state,
            "requiresApproval"
        );
        assert_eq!(
            map_login_status(SMAppServiceStatus::NotFound).state,
            "notFound"
        );
        assert_eq!(map_login_status(SMAppServiceStatus(99)).state, "error");
        assert_eq!(
            map_notification_status(UNAuthorizationStatus::NotDetermined).state,
            "notDetermined"
        );
        assert_eq!(
            map_notification_status(UNAuthorizationStatus::Denied).state,
            "denied"
        );
        assert_eq!(
            map_notification_status(UNAuthorizationStatus::Authorized).state,
            "authorized"
        );
        assert_eq!(
            map_notification_status(UNAuthorizationStatus::Provisional).state,
            "provisional"
        );
        assert_eq!(
            map_notification_status(UNAuthorizationStatus(99)).state,
            "error"
        );
    }
}
