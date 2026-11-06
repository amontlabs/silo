//! Linux without the `desktop` feature: a headless host (the engine daemon on a server) has no
//! desktop applications to list or open. Every call answers as a desktop with none installed.

use std::path::Path;

use super::{launch, Application, ApplicationCatalog};

const NO_DESKTOP: &str = "This Linux host has no desktop applications.";

pub fn editor_command(_application: &Application) -> Result<launch::EditorCommand, String> {
    Err(NO_DESKTOP.into())
}

pub fn open_browser(_selection: Option<&Path>, _url: &str) -> Result<(), String> {
    Err(NO_DESKTOP.into())
}

pub fn discover() -> Result<ApplicationCatalog, String> {
    Ok(ApplicationCatalog::default())
}

pub fn application_at(_path: &Path) -> Option<Application> {
    None
}

pub fn load_icons(_catalog: &mut ApplicationCatalog) {}

pub fn load_icon(_application: &mut Application) {}

pub fn open_terminal(_application: &Application, _command: &str) -> Result<(), String> {
    Err(NO_DESKTOP.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_headless_host_lists_no_applications_and_opens_none() {
        let catalog = discover().unwrap();
        assert!(catalog.terminal.is_empty() && catalog.editor.is_empty());
        assert!(catalog.browser.is_empty() && catalog.defaults.is_empty());
        assert!(application_at(Path::new("/usr/bin/sh")).is_none());
        assert!(open_browser(None, "https://example.com").is_err());
    }
}
