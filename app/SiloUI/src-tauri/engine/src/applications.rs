use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

#[path = "applications/launch.rs"]
pub mod launch;
#[cfg(target_os = "linux")]
#[path = "applications/linux.rs"]
mod linux;
#[cfg(target_os = "macos")]
#[path = "applications/macos.rs"]
mod macos;
#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(target_os = "macos")]
use macos as platform;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Application {
    pub name: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

#[derive(Default, Debug, Serialize)]
pub struct ApplicationCatalog {
    pub terminal: Vec<Application>,
    pub editor: Vec<Application>,
    pub browser: Vec<Application>,
    pub defaults: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApplicationKind {
    Terminal,
    Editor,
    Browser,
}

/// Open a web address using the selected browser path. Call from a worker.
pub fn validated_browser_url(value: &str) -> Result<String, String> {
    browser_url(value)
}

pub fn open_browser_with_settings(
    settings: &serde_json::Map<String, serde_json::Value>,
    url: &str,
) -> Result<(), String> {
    let selection = browser_selection(settings, cfg!(target_os = "linux"))?;
    platform::open_browser(selection, url)
}

fn browser_url(value: &str) -> Result<String, String> {
    if value.len() > 8192 || value.chars().any(char::is_control) {
        return Err("The website address is invalid.".into());
    }
    let url = reqwest::Url::parse(value).map_err(|_| "The website address is invalid.")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("Only HTTP and HTTPS website addresses can be opened.".into());
    }
    Ok(url.into())
}

fn browser_selection(
    settings: &serde_json::Map<String, serde_json::Value>,
    desktop_entry_required: bool,
) -> Result<Option<&Path>, String> {
    if settings
        .get("browserUseSystemDefault")
        .and_then(|value| value.as_bool())
        .unwrap_or_else(|| {
            !settings.contains_key("browser") && !settings.contains_key("browserPath")
        })
    {
        return Ok(None);
    }
    let path = settings
        .get("browserPath")
        .and_then(|value| value.as_str())
        .filter(|path| Path::new(path).is_absolute())
        .ok_or("Choose an available browser in Settings.")?;
    Ok(Some(browser_path(Path::new(path), desktop_entry_required)?))
}

fn browser_path(path: &Path, desktop_entry_required: bool) -> Result<&Path, String> {
    if desktop_entry_required
        && path
            .extension()
            .is_none_or(|extension| extension != "desktop")
    {
        return Err("Choose a browser desktop entry (.desktop) in Settings.".into());
    }
    Ok(path)
}

fn validate_browser_path(path: &Path, desktop_entry_required: bool) -> Result<(), String> {
    browser_path(path, desktop_entry_required).map(|_| ())
}

pub fn selected_terminal_from_settings(
    settings: &serde_json::Map<String, serde_json::Value>,
) -> Result<Application, String> {
    let path = if settings
        .get("terminalUseSystemDefault")
        .and_then(|v| v.as_bool())
        .unwrap_or(true)
    {
        platform::discover()?.defaults.remove("terminal")
    } else {
        settings
            .get("terminalPath")
            .and_then(|v| v.as_str())
            .map(str::to_owned)
    }
    .ok_or("Choose an available terminal in Settings.")?;
    platform::application_at(Path::new(&path))
        .ok_or_else(|| "The selected terminal is unavailable. Choose another in Settings.".into())
}
pub fn open_terminal_with_application(
    application: &Application,
    command: &str,
) -> Result<(), String> {
    platform::open_terminal(application, command)
}

pub fn selected_editor_from_settings(
    settings: &serde_json::Map<String, serde_json::Value>,
) -> Result<Application, String> {
    let path = if settings
        .get("editorUseSystemDefault")
        .and_then(|value| value.as_bool())
        .unwrap_or_else(|| !settings.contains_key("editor") && !settings.contains_key("editorPath"))
    {
        platform::discover()?.defaults.remove("editor")
    } else {
        settings
            .get("editorPath")
            .and_then(|value| value.as_str())
            .map(str::to_owned)
    }
    .ok_or("Choose an available code editor in Settings.")?;
    platform::application_at(Path::new(&path))
        .ok_or_else(|| "The selected editor is unavailable. Choose another in Settings.".into())
}

pub fn editor_command(application: &Application) -> Result<launch::EditorCommand, String> {
    platform::editor_command(application)
}

fn include_selections(
    catalog: &mut ApplicationCatalog,
    selections: BTreeMap<String, String>,
    desktop_entry_required: bool,
    read: impl Fn(&Path) -> Option<Application>,
) {
    for (kind, applications) in [
        ("terminal", &mut catalog.terminal),
        ("editor", &mut catalog.editor),
        ("browser", &mut catalog.browser),
    ] {
        if let Some(path) = selections.get(kind) {
            if path.encode_utf16().count() <= 4096
                && Path::new(path).is_absolute()
                && (kind != "browser"
                    || browser_path(Path::new(path), desktop_entry_required).is_ok())
            {
                if let Some(application) = read(Path::new(path)) {
                    applications.push(application);
                }
            }
        }
        applications.retain(|application| {
            !application.name.is_empty()
                && application.name.encode_utf16().count() <= 256
                && application.path.encode_utf16().count() <= 4096
                && Path::new(&application.path).is_absolute()
        });
        applications
            .sort_by_key(|application| (application.name.to_lowercase(), application.path.clone()));
        let mut paths = std::collections::HashSet::new();
        applications.retain(|application| paths.insert(application.path.clone()));
        if catalog.defaults.get(kind).is_some_and(|path| {
            !applications
                .iter()
                .any(|application| &application.path == path)
        }) {
            catalog.defaults.remove(kind);
        }
    }
}

pub fn discover_with_selections(
    selections: BTreeMap<String, String>,
) -> Result<ApplicationCatalog, String> {
    let mut catalog = platform::discover()?;
    include_selections(
        &mut catalog,
        selections,
        cfg!(target_os = "linux"),
        platform::application_at,
    );
    Ok(catalog)
}

pub(crate) fn application_at(path: &Path) -> Option<Application> {
    platform::application_at(path)
}

pub fn chosen_application(path: &Path, kind: ApplicationKind) -> Result<Application, String> {
    validate_browser_path(
        path,
        cfg!(target_os = "linux") && matches!(kind, ApplicationKind::Browser),
    )?;
    let application =
        application_at(path).ok_or("The selected item is not an available application")?;
    if application.name.encode_utf16().count() > 256
        || application.path.encode_utf16().count() > 4096
    {
        return Err("The selected application's name or location is too long".into());
    }
    Ok(application)
}

#[cfg(target_os = "linux")]
pub(crate) fn load_icons(catalog: &mut ApplicationCatalog) {
    platform::load_icons(catalog);
}

#[cfg(target_os = "linux")]
pub(crate) fn load_icon(application: &mut Application) {
    platform::load_icon(application);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_addresses_are_web_only_without_embedded_credentials() {
        for invalid in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "https://user:secret@localhost/",
            "http://localhost/\n",
            "--args",
        ] {
            assert!(browser_url(invalid).is_err());
        }
        assert_eq!(
            browser_url("http://127.0.0.1:3000/a?x=$(touch)").unwrap(),
            "http://127.0.0.1:3000/a?x=$(touch)"
        );
    }

    #[test]
    fn browser_preference_honors_default_and_requires_explicit_available_selection() {
        let settings =
            serde_json::json!({"browserUseSystemDefault": true, "browserPath": "/old/browser.app"});
        assert_eq!(
            browser_selection(settings.as_object().unwrap(), false).unwrap(),
            None
        );
        let settings = serde_json::json!({"browserUseSystemDefault": false, "browserPath": "/Applications/Selected.app"});
        assert_eq!(
            browser_selection(settings.as_object().unwrap(), false).unwrap(),
            Some(Path::new("/Applications/Selected.app"))
        );
        let settings = serde_json::json!({"browserUseSystemDefault": false});
        assert!(browser_selection(settings.as_object().unwrap(), false).is_err());
        let settings = serde_json::json!({"browserPath": "/Applications/Selected.app"});
        assert_eq!(
            browser_selection(settings.as_object().unwrap(), false).unwrap(),
            Some(Path::new("/Applications/Selected.app"))
        );
    }

    #[test]
    fn linux_browser_choices_require_a_desktop_entry() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("browser");
        std::fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let selection =
            serde_json::json!({"browserUseSystemDefault": false, "browserPath": executable});
        assert!(browser_selection(selection.as_object().unwrap(), true).is_err());
        assert!(browser_path(&executable, true)
            .unwrap_err()
            .contains(".desktop"));
        assert_eq!(
            browser_path(&executable, false).unwrap(),
            executable.as_path()
        );
        let desktop = directory.path().join("browser.desktop");
        std::fs::write(
            &desktop,
            "[Desktop Entry]\nType=Application\nName=Browser\nExec=/bin/true %u\n",
        )
        .unwrap();
        let selection =
            serde_json::json!({"browserUseSystemDefault": false, "browserPath": desktop});
        assert_eq!(
            browser_selection(selection.as_object().unwrap(), true).unwrap(),
            Some(desktop.as_path())
        );
        let selection =
            serde_json::json!({"browserUseSystemDefault": true, "browserPath": executable});
        assert_eq!(
            browser_selection(selection.as_object().unwrap(), true).unwrap(),
            None
        );
    }

    #[test]
    fn executable_browser_preferences_are_not_offered_as_available_choices() {
        let executable = Application {
            name: "Fixture browser".into(),
            path: "/fixture/browser".into(),
            icon: None,
        };
        let mut catalog = ApplicationCatalog::default();
        include_selections(
            &mut catalog,
            BTreeMap::from([
                ("browser".into(), executable.path.clone()),
                ("editor".into(), executable.path.clone()),
                ("terminal".into(), executable.path.clone()),
            ]),
            true,
            |path| (path == Path::new(&executable.path)).then(|| executable.clone()),
        );
        assert!(catalog.browser.is_empty());
        assert_eq!(catalog.editor, [executable.clone()]);
        assert_eq!(catalog.terminal, [executable]);
    }

    #[test]
    fn custom_choices_survive_discovery_without_offering_removed_apps() {
        let chosen = Application {
            name: "Custom Editor".into(),
            path: "/Applications/Custom.app".into(),
            icon: None,
        };
        let mut catalog = ApplicationCatalog::default();
        include_selections(
            &mut catalog,
            BTreeMap::from([
                ("editor".into(), chosen.path.clone()),
                ("terminal".into(), "/Applications/Removed.app".into()),
            ]),
            false,
            |path| (path == Path::new(&chosen.path)).then(|| chosen.clone()),
        );
        assert_eq!(catalog.editor, [chosen]);
        assert!(catalog.terminal.is_empty());
    }

    #[test]
    fn installed_and_custom_choices_are_deduplicated_by_exact_location() {
        let application = Application {
            name: "Editor".into(),
            path: "/Applications/Editor.app".into(),
            icon: None,
        };
        let mut catalog = ApplicationCatalog {
            editor: vec![application.clone()],
            ..Default::default()
        };
        include_selections(
            &mut catalog,
            BTreeMap::from([("editor".into(), application.path.clone())]),
            false,
            |_| Some(application.clone()),
        );
        assert_eq!(catalog.editor, [application]);
    }
}
