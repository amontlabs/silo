use std::env;
use std::fs::{self, File};
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use gio::prelude::*;
use gio::DesktopAppInfo;

use super::{IntegrationStatus, SystemIntegrations};

const GROUP: &str = "Desktop Entry";

fn desktop_id() -> &'static str {
    crate::channel::current().desktop_entry_id()
}

#[derive(Clone, Debug)]
struct Environment {
    config_home: PathBuf,
    config_dirs: Vec<PathBuf>,
    current_desktops: Vec<String>,
    executable: PathBuf,
}

impl Environment {
    fn current() -> Result<Self, String> {
        let config_home = env::var_os("XDG_CONFIG_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| {
                env::var_os("HOME")
                    .map(PathBuf::from)
                    .filter(|path| path.is_absolute())
                    .map(|home| home.join(".config"))
            })
            .ok_or("Neither an absolute XDG_CONFIG_HOME nor HOME is available")?;
        let mut config_dirs: Vec<PathBuf> = env::var_os("XDG_CONFIG_DIRS")
            .filter(|value| !value.is_empty())
            .map(|value| {
                env::split_paths(&value)
                    .filter(|path| path.is_absolute())
                    .collect()
            })
            .unwrap_or_else(|| vec![PathBuf::from("/etc/xdg")]);
        if config_dirs.is_empty() {
            config_dirs.push(PathBuf::from("/etc/xdg"));
        }
        let current_desktops = env::var("XDG_CURRENT_DESKTOP")
            .unwrap_or_default()
            .split(':')
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .collect();
        Ok(Self {
            config_home,
            config_dirs,
            current_desktops,
            executable: launch_target()?,
        })
    }

    fn user_entry(&self) -> PathBuf {
        self.config_home.join("autostart").join(desktop_id())
    }

    fn system_entries(&self) -> impl Iterator<Item = PathBuf> + '_ {
        self.config_dirs
            .iter()
            .map(|directory| directory.join("autostart").join(desktop_id()))
    }

    fn effective_entry(&self) -> Option<PathBuf> {
        std::iter::once(self.user_entry())
            .chain(self.system_entries())
            .find(|path| path.is_file())
    }
}

fn launch_target() -> Result<PathBuf, String> {
    let current =
        env::current_exe().map_err(|error| format!("Silo's executable is unavailable: {error}"))?;
    Ok(select_launch_target(
        env::var_os("APPIMAGE").map(PathBuf::from).as_deref(),
        &current,
    ))
}

fn select_launch_target(app_image: Option<&Path>, current: &Path) -> PathBuf {
    app_image
        .filter(|path| executable(path))
        .unwrap_or(current)
        .to_owned()
}

fn executable(path: &Path) -> bool {
    path.is_file()
        && path
            .metadata()
            .is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
}

fn find_executable(path: &Path) -> bool {
    if path.is_absolute() || path.components().count() > 1 {
        executable(path)
    } else {
        gio::glib::find_program_in_path(path).is_some()
    }
}

fn list(key_file: &gio::glib::KeyFile, key: &str) -> Vec<String> {
    key_file
        .string_list(GROUP, key)
        .map(|values| values.iter().map(ToString::to_string).collect())
        .unwrap_or_default()
}

fn visible_for_desktop(key_file: &gio::glib::KeyFile, current: &[String]) -> bool {
    let only = list(key_file, "OnlyShowIn");
    let excluded = list(key_file, "NotShowIn");
    if !only.is_empty() && !excluded.is_empty() {
        return false;
    }
    (only.is_empty() || current.iter().any(|desktop| only.contains(desktop)))
        && !current.iter().any(|desktop| excluded.contains(desktop))
}

fn entry_enabled(path: &Path, environment: &Environment) -> Result<bool, String> {
    let key_file = gio::glib::KeyFile::new();
    key_file
        .load_from_file(path, gio::glib::KeyFileFlags::NONE)
        .map_err(|error| format!("Autostart entry could not be read: {error}"))?;
    if key_file.boolean(GROUP, "Hidden").unwrap_or(false)
        || key_file
            .boolean(GROUP, "X-GNOME-Autostart-enabled")
            .is_ok_and(|enabled| !enabled)
        || !visible_for_desktop(&key_file, &environment.current_desktops)
    {
        return Ok(false);
    }
    if let Ok(try_exec) = key_file.string(GROUP, "TryExec") {
        if !find_executable(Path::new(try_exec.as_str())) {
            return Ok(false);
        }
    }
    let info = DesktopAppInfo::from_filename(path)
        .ok_or("Autostart entry is not a valid desktop entry")?;
    let command = info.commandline().ok_or("Autostart entry has no command")?;
    let arguments = gio::glib::shell_parse_argv(command)
        .map_err(|error| format!("Autostart command could not be read: {error}"))?;
    Ok(arguments
        .first()
        .is_some_and(|command| find_executable(Path::new(command))))
}

fn login_item(environment: &Environment) -> IntegrationStatus {
    let Some(path) = environment.effective_entry() else {
        return IntegrationStatus::new("notRegistered");
    };
    match entry_enabled(&path, environment) {
        Ok(true) => IntegrationStatus::new("enabled"),
        Ok(false) => IntegrationStatus::new("notRegistered"),
        Err(error) => IntegrationStatus::error(error),
    }
}

fn quote_exec_argument(path: &Path) -> Result<String, String> {
    let text = path
        .to_str()
        .ok_or("Silo's executable path is not valid UTF-8")?;
    let escaped = text
        .replace('%', "%%")
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('`', "\\`")
        .replace('$', "\\$");
    Ok(format!("\"{escaped}\""))
}

fn desktop_entry(environment: &Environment, hidden: bool) -> Result<Vec<u8>, String> {
    let target = environment
        .executable
        .to_str()
        .ok_or("Silo's executable path is not valid UTF-8")?;
    let entry = gio::glib::KeyFile::new();
    entry.set_string(GROUP, "Type", "Application");
    entry.set_string(
        GROUP,
        "Name",
        crate::channel::current().desktop_entry_name(),
    );
    // GIO rejects a desktop entry when the executable token itself contains an
    // escaped percent. A fixed env executable lets field-code expansion happen
    // only in the following argument, which remains one safely quoted path.
    // KeyFile then applies the desktop-entry string escaping to both values.
    entry.set_string(
        GROUP,
        "Exec",
        &format!(
            "/usr/bin/env -- {}",
            quote_exec_argument(&environment.executable)?
        ),
    );
    entry.set_string(GROUP, "TryExec", target);
    entry.set_boolean(GROUP, "X-GNOME-Autostart-enabled", true);
    if hidden {
        entry.set_boolean(GROUP, "Hidden", true);
    }
    Ok(entry.to_data().as_bytes().to_vec())
}

fn write_entry(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "missing autostart directory")
    })?;
    fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    File::open(parent)?.sync_all()
}

fn notifications() -> IntegrationStatus {
    let available = gio::DBusProxy::for_bus_sync(
        gio::BusType::Session,
        gio::DBusProxyFlags::DO_NOT_AUTO_START,
        None,
        "org.freedesktop.Notifications",
        "/org/freedesktop/Notifications",
        "org.freedesktop.Notifications",
        None::<&gio::Cancellable>,
    )
    .ok()
    .and_then(|proxy| proxy.name_owner())
    .is_some();
    IntegrationStatus::new(if available {
        "authorized"
    } else {
        "unavailable"
    })
}

pub fn read() -> SystemIntegrations {
    let login_item = match Environment::current() {
        Ok(environment) => login_item(&environment),
        Err(error) => IntegrationStatus::error(error),
    };
    SystemIntegrations {
        platform: "linux",
        login_item,
        notifications: notifications(),
    }
}

pub fn set_login_item(enabled: bool) -> Result<IntegrationStatus, String> {
    let environment = Environment::current()?;
    let bytes = desktop_entry(&environment, !enabled)?;
    write_entry(&environment.user_entry(), &bytes)
        .map_err(|error| format!("The autostart entry could not be saved: {error}"))?;
    let verified = login_item(&environment);
    let matches =
        (enabled && verified.state == "enabled") || (!enabled && verified.state == "notRegistered");
    if matches {
        Ok(verified)
    } else {
        Err(verified.error.unwrap_or_else(|| {
            format!(
                "The desktop did not confirm the requested autostart state (reported {})",
                verified.state
            )
        }))
    }
}

/// Show a message before Tauri starts (F-11); the process exits afterwards.
pub fn show_startup_notice(message: &str) {
    use gtk::prelude::*;
    eprintln!("{message}");
    if gtk::init().is_err() {
        return;
    }
    let dialog = gtk::MessageDialog::new(
        None::<&gtk::Window>,
        gtk::DialogFlags::MODAL,
        gtk::MessageType::Info,
        gtk::ButtonsType::Ok,
        message,
    );
    dialog.set_title(crate::channel::current().product_name());
    dialog.run();
    dialog.close();
}

pub fn request_notifications() -> Result<IntegrationStatus, String> {
    Ok(notifications())
}

const NOTIFICATIONS_BUS: &str = "org.freedesktop.Notifications";

/// Server-assigned id per notice key. Sending the previous id as `replaces_id` makes a
/// newer notice replace the older one, and lets deletion close it.
static SERVER_IDS: super::notification_ids::NotificationIds =
    super::notification_ids::NotificationIds::new();

fn notifications_proxy() -> Result<gio::DBusProxy, gio::glib::Error> {
    gio::DBusProxy::for_bus_sync(
        gio::BusType::Session,
        gio::DBusProxyFlags::DO_NOT_AUTO_START,
        None,
        NOTIFICATIONS_BUS,
        "/org/freedesktop/Notifications",
        NOTIFICATIONS_BUS,
        None::<&gio::Cancellable>,
    )
}

// Click routing is not implemented on Linux: it needs a GLib main loop to receive the
// `ActionInvoked` signal, and desktops differ in whether they show a default action.
// Notices still replace by key and are closed when their computer is deleted.
pub fn deliver_notification(
    notice: &crate::notifications::Notice,
) -> Result<super::NotificationDelivery, String> {
    use gio::glib::variant::ToVariant;
    let proxy =
        notifications_proxy().map_err(|_| "The desktop notification service is unavailable")?;
    let Some(owner) = proxy.name_owner() else {
        return Ok(super::NotificationDelivery::Skipped);
    };
    SERVER_IDS.deliver(&owner, &notice.key, |replaces| {
        // The standard has no permission prompt. The desktop controls suppression/DND.
        let hints = std::collections::HashMap::from([
            (
                "desktop-entry",
                super::notification_desktop_entry().to_variant(),
            ),
            ("urgency", 1u8.to_variant()),
        ]);
        let parameters = (
            crate::channel::current().tray_title(),
            replaces,
            super::NOTIFICATION_ICON,
            notice.title.as_str(),
            notice.body.as_str(),
            Vec::<String>::new(),
            hints,
            -1i32,
        )
            .to_variant();
        // Address the unique owner: a service restart must not redirect an old ID.
        let reply = proxy
            .connection()
            .call_sync(
                Some(&owner),
                "/org/freedesktop/Notifications",
                NOTIFICATIONS_BUS,
                "Notify",
                Some(&parameters),
                None,
                gio::DBusCallFlags::NO_AUTO_START,
                5000,
                None::<&gio::Cancellable>,
            )
            .map_err(|_| "The desktop could not deliver the notification")?;
        reply
            .get::<(u32,)>()
            .map(|(id,)| id)
            .ok_or_else(|| "The desktop returned an invalid notification ID".into())
    })?;
    Ok(super::NotificationDelivery::Delivered)
}

pub fn clear_notifications(keys: &[String]) {
    use gio::glib::variant::ToVariant;
    let Ok(proxy) = notifications_proxy() else {
        return;
    };
    let Some(owner) = proxy.name_owner() else {
        return;
    };
    SERVER_IDS.clear(&owner, keys, |id| {
        let _ = proxy.connection().call_sync(
            Some(&owner),
            "/org/freedesktop/Notifications",
            NOTIFICATIONS_BUS,
            "CloseNotification",
            Some(&(id,).to_variant()),
            None,
            gio::DBusCallFlags::NO_AUTO_START,
            5000,
            None::<&gio::Cancellable>,
        );
    });
}

pub fn open_settings(_integration: &str) -> Result<(), String> {
    Err("This desktop does not provide a standard settings page for this integration".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn environment(root: &Path, executable: &Path) -> Environment {
        Environment {
            config_home: root.join("user"),
            config_dirs: vec![root.join("system-one"), root.join("system-two")],
            current_desktops: vec!["GNOME".into()],
            executable: executable.into(),
        }
    }

    fn write(path: &Path, contents: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn available_entry(executable: &Path) -> String {
        format!(
            "[Desktop Entry]\nType=Application\nName=Silo\nExec=\"{}\"\n",
            executable.display()
        )
    }

    #[test]
    fn user_entry_has_precedence_and_hidden_override_disables_a_system_entry() {
        let root = tempfile::tempdir().unwrap();
        let executable = std::env::current_exe().unwrap();
        let environment = environment(root.path(), &executable);
        let system = environment.system_entries().next().unwrap();
        write(&system, &available_entry(&executable));
        assert_eq!(login_item(&environment).state, "enabled");

        write(
            &environment.user_entry(),
            "[Desktop Entry]\nType=Application\nName=Silo\nHidden=true\n",
        );
        assert_eq!(login_item(&environment).state, "notRegistered");
    }

    #[test]
    fn system_config_directories_follow_declared_precedence() {
        let root = tempfile::tempdir().unwrap();
        let executable = std::env::current_exe().unwrap();
        let environment = environment(root.path(), &executable);
        let entries: Vec<_> = environment.system_entries().collect();
        write(&entries[1], &available_entry(&executable));
        assert_eq!(login_item(&environment).state, "enabled");

        write(
            &entries[0],
            "[Desktop Entry]\nType=Application\nName=Silo\nHidden=true\n",
        );
        assert_eq!(login_item(&environment).state, "notRegistered");
    }

    #[test]
    fn desktop_restrictions_and_unavailable_try_exec_are_not_enabled() {
        let root = tempfile::tempdir().unwrap();
        let executable = std::env::current_exe().unwrap();
        let environment = environment(root.path(), &executable);
        let entry = environment.user_entry();
        for extra in [
            "OnlyShowIn=KDE;",
            "NotShowIn=GNOME;",
            "TryExec=/definitely/missing/silo-preview",
            "X-GNOME-Autostart-enabled=false",
        ] {
            write(
                &entry,
                &format!("{}{}\n", available_entry(&executable), extra),
            );
            assert_eq!(login_item(&environment).state, "notRegistered");
        }
    }

    #[test]
    fn generated_entry_round_trips_and_gio_launches_a_special_character_path() {
        let root = tempfile::tempdir().unwrap();
        let marker = root.path().join("launched");
        let executable = root
            .path()
            .join(r#"Silo $ Preview % `one` \ "quoted".AppImage"#);
        write(
            &executable,
            &format!("#!/bin/sh\n: > '{}'\n", marker.display()),
        );
        let mut permissions = fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions).unwrap();
        let environment = environment(root.path(), &executable);
        let entry_path = root.path().join(desktop_id());
        write_entry(&entry_path, &desktop_entry(&environment, false).unwrap()).unwrap();

        let parsed = gio::glib::KeyFile::new();
        parsed
            .load_from_file(&entry_path, gio::glib::KeyFileFlags::NONE)
            .unwrap();
        assert_eq!(
            parsed.string(GROUP, "TryExec").unwrap(),
            executable.to_str().unwrap()
        );
        assert_eq!(
            parsed.string(GROUP, "Exec").unwrap(),
            format!(
                "/usr/bin/env -- {}",
                quote_exec_argument(&executable).unwrap()
            )
        );

        let info = DesktopAppInfo::from_filename(&entry_path).unwrap();
        assert_eq!(info.executable(), PathBuf::from("/usr/bin/env"));
        info.launch(&[], None::<&gio::AppLaunchContext>).unwrap();
        for _ in 0..200 {
            if marker.is_file() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(marker.is_file(), "GIO did not launch the generated entry");
    }

    #[test]
    fn available_appimage_is_the_launch_target_and_an_unavailable_one_is_ignored() {
        let root = tempfile::tempdir().unwrap();
        let current = std::env::current_exe().unwrap();
        let appimage = root.path().join("Silo Preview.AppImage");
        write(&appimage, "executable");
        let mut permissions = fs::metadata(&appimage).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&appimage, permissions).unwrap();
        assert_eq!(select_launch_target(Some(&appimage), &current), appimage);
        assert_eq!(
            select_launch_target(Some(&root.path().join("missing")), &current),
            current
        );
    }

    #[test]
    fn atomic_write_reports_an_unwritable_destination() {
        let root = tempfile::tempdir().unwrap();
        let blocker = root.path().join("not-a-directory");
        fs::write(&blocker, "file").unwrap();
        let error = write_entry(&blocker.join(desktop_id()), b"entry").unwrap_err();
        assert!(matches!(
            error.kind(),
            io::ErrorKind::NotADirectory | io::ErrorKind::AlreadyExists
        ));
    }
}
