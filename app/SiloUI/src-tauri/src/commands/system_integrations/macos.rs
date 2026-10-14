use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send};
use objc2_foundation::{NSObject, NSObjectProtocol, NSString};
use objc2_user_notifications::{
    UNNotification, UNNotificationPresentationOptions, UNNotificationResponse,
    UNUserNotificationCenter, UNUserNotificationCenterDelegate,
};

static APP: std::sync::OnceLock<tauri::AppHandle> = std::sync::OnceLock::new();

/// Open the main window at the route a clicked notification carries.
fn open_route(route: &str) {
    let (Some(app), Ok(route)) = (APP.get(), serde_json::from_str::<serde_json::Value>(route))
    else {
        return;
    };
    let handle = app.clone();
    // The callback arrives on an arbitrary thread; window work belongs on the main thread.
    let _ = app.run_on_main_thread(move || {
        let _ = crate::status_panel::open_main(handle, Some(route));
    });
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; this delegate has no mutable state.
    #[unsafe(super = NSObject)]
    #[ivars = ()]
    struct NotificationDelegate;
    unsafe impl NSObjectProtocol for NotificationDelegate {}
    unsafe impl UNUserNotificationCenterDelegate for NotificationDelegate {
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            completion: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            // The router already suppresses notices while the main window is focused, so
            // a notice that races a focus change goes quietly to Notification Center
            // instead of interrupting with a banner.
            completion.call((UNNotificationPresentationOptions::List,));
        }

        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn did_receive_response(
            &self,
            _center: &UNUserNotificationCenter,
            response: &UNNotificationResponse,
            completion: &block2::DynBlock<dyn Fn()>,
        ) {
            let info = response.notification().request().content().userInfo();
            let key = NSString::from_str(crate::system_integrations::ROUTE_KEY);
            let route = info
                .objectForKey(&key)
                .and_then(|value| value.downcast::<NSString>().ok())
                .map(|value| value.to_string());
            if let Some(route) = route {
                open_route(&route);
            }
            completion.call(());
        }
    }
);

thread_local! { static NOTIFICATION_DELEGATE: std::cell::RefCell<Option<Retained<NotificationDelegate>>> = const { std::cell::RefCell::new(None) }; }

pub(super) fn install_notifications(app: &tauri::AppHandle) {
    let _ = APP.set(app.clone());
    if !crate::system_integrations::macos::notifications_supported() {
        return;
    }
    use objc2::AnyThread;
    let allocated = NotificationDelegate::alloc().set_ivars(());
    // SAFETY: NSObject's init has the declared signature and initializes our subclass.
    let delegate: Retained<NotificationDelegate> = unsafe { msg_send![super(allocated), init] };
    UNUserNotificationCenter::currentNotificationCenter()
        .setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    // UserNotifications stores its delegate weakly. Keep it alive on the installation thread.
    NOTIFICATION_DELEGATE.with(|slot| *slot.borrow_mut() = Some(delegate));
}
