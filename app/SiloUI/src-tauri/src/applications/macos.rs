use super::{Application, ApplicationCatalog};
use objc2::{
    rc::{autoreleasepool, Retained},
    runtime::AnyObject,
    sel, AnyThread,
};
use objc2_app_kit::{
    NSBitmapImageFileType, NSBitmapImageRep, NSCompositingOperation, NSDeviceRGBColorSpace,
    NSGraphicsContext, NSImage, NSImageInterpolation, NSWorkspace,
};
use objc2_foundation::{
    NSBundle, NSCopying, NSData, NSDataBase64EncodingOptions, NSDictionary, NSObjectProtocol,
    NSPoint, NSRect, NSSize, NSString, NSURL,
};
use std::{fs, os::unix::fs::PermissionsExt, path::Path};

pub fn editor_command(application: &Application) -> Result<super::launch::EditorCommand, String> {
    let root = Path::new(&application.path);
    // Reuse Launch Services bundle identity, not an arbitrary app display name.
    let id = autoreleasepool(|_| {
        let url = NSURL::fileURLWithPath(&NSString::from_str(&application.path));
        NSBundle::bundleWithURL(&url)
            .and_then(|bundle| bundle.bundleIdentifier())
            .map(|id| id.to_string())
    });
    let Some((relative, zed)) = id.as_deref().and_then(editor_adapter) else {
        return Err(
            "Remote folders currently support Zed and Visual Studio Code. Choose one in Settings."
                .into(),
        );
    };
    let program = root.join(relative);
    if !super::launch::executable_file(&program) {
        return Err("The selected editor's command is unavailable.".into());
    }
    Ok(super::launch::EditorCommand {
        program,
        args: Vec::new(),
        zed,
    })
}

pub fn open_browser(selection: Option<&Path>, url: &str) -> Result<(), String> {
    let mut command = std::process::Command::new("/usr/bin/open");
    if let Some(path) = selection {
        application_at(path)
            .ok_or("The selected browser is unavailable. Choose another in Settings.")?;
        command.arg("-a").arg(path);
    }
    command.arg(url);
    launch_browser(command, std::time::Duration::from_secs(10))
}

fn launch_browser(
    command: std::process::Command,
    timeout: std::time::Duration,
) -> Result<(), String> {
    let status = run_bounded(command, timeout).map_err(|error| match error {
        Bounded::Spawn => "The browser could not be opened.",
        Bounded::TimedOut => "Opening the browser timed out. Check your browser and retry.",
    })?;
    if status.success() {
        Ok(())
    } else {
        Err("The browser could not be opened. Check your browser selection in Settings.".into())
    }
}

// Suggestions and defaults list only apps Silo can hand a computer to (G-07);
// Choose… can still select any valid application bundle.
// Terminals with a command launcher in `open_terminal`, in default order.
// Launch Services also advertises editors for shell scripts.
const TERMINAL_IDS: &[&str] = &[
    "com.apple.Terminal",
    "com.mitchellh.ghostty",
    "com.googlecode.iterm2",
];

// Editors `editor_command` supports, in default order.
const EDITOR_IDS: &[&str] = &[
    "com.microsoft.VSCode",
    "dev.zed.Zed",
    "com.microsoft.VSCodeInsiders",
    "dev.zed.Zed-Preview",
    "dev.zed.Zed-Nightly",
    "dev.zed.Zed-Dev",
];

/// The bundled CLI and whether it takes Zed's `ssh://` URI.
fn editor_adapter(id: &str) -> Option<(&'static str, bool)> {
    match id {
        "dev.zed.Zed" | "dev.zed.Zed-Preview" | "dev.zed.Zed-Nightly" | "dev.zed.Zed-Dev" => {
            Some(("Contents/MacOS/cli", true))
        }
        "com.microsoft.VSCode" | "com.microsoft.VSCodeInsiders" => {
            Some(("Contents/Resources/app/bin/code", false))
        }
        _ => None,
    }
}

/// Keeps a system default only when it is supported; otherwise the first
/// installed supported app, in the lists' order, becomes the default.
fn supported_default(system: Option<String>, installed_in_order: &[String]) -> Option<String> {
    system.or_else(|| installed_in_order.first().cloned())
}

const BROWSER_IDS: &[&str] = &[
    "com.apple.Safari",
    "com.google.Chrome",
    "org.mozilla.firefox",
    "com.microsoft.edgemac",
    "com.brave.Browser",
    "company.thebrowser.Browser",
    "company.thebrowser.dia",
    "org.torproject.torbrowser",
    "app.zen-browser.zen",
];

#[derive(Clone, Copy)]
enum HandlerFilter {
    Terminal,
    Editor,
    Browser,
    Any,
}

fn thumbnail_png(image: &NSImage) -> Option<Retained<NSData>> {
    // A dedicated bitmap context bounds the output independently of the source
    // image's representations and keeps all drawing on the calling worker.
    let bitmap = unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(), std::ptr::null_mut(), 64, 64, 8, 4,
            true, false, NSDeviceRGBColorSpace, 64 * 4, 32,
        )
    }?;
    let context = NSGraphicsContext::graphicsContextWithBitmapImageRep(&bitmap)?;
    let previous = NSGraphicsContext::currentContext();
    NSGraphicsContext::setCurrentContext(Some(&context));
    context.setImageInterpolation(NSImageInterpolation::High);
    image.drawInRect_fromRect_operation_fraction(
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(64.0, 64.0)),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0)),
        NSCompositingOperation::Copy,
        1.0,
    );
    context.flushGraphics();
    NSGraphicsContext::setCurrentContext(previous.as_deref());
    // The dictionary is empty, so it cannot contain incorrectly typed options.
    let png = unsafe {
        bitmap.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
    }?;
    (png.length() <= 64 * 1024).then_some(png)
}

fn application_icon(path: &str) -> Option<String> {
    let icon = NSWorkspace::sharedWorkspace()
        .iconForFile(&NSString::from_str(path))
        .copy();
    let png = thumbnail_png(&icon)?;
    Some(format!(
        "data:image/png;base64,{}",
        png.base64EncodedStringWithOptions(NSDataBase64EncodingOptions::empty())
    ))
}

fn nonempty_string(value: &AnyObject) -> Option<String> {
    let text = value.downcast_ref::<NSString>()?.to_string();
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

fn bundle_name(bundle: &NSBundle, path: &Path) -> Option<String> {
    for dictionary in [bundle.localizedInfoDictionary(), bundle.infoDictionary()]
        .into_iter()
        .flatten()
    {
        for key in ["CFBundleDisplayName", "CFBundleName"] {
            if let Some(name) = dictionary
                .objectForKey(&NSString::from_str(key))
                .and_then(|value| nonempty_string(&value))
            {
                return Some(name);
            }
        }
    }
    let name = path.file_stem()?.to_str()?.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

fn inspect_application(path: &Path) -> Option<(Application, Retained<NSBundle>)> {
    if !path.is_absolute() {
        return None;
    }
    let path = fs::canonicalize(path).ok()?;
    if !path.is_dir() || !path.extension()?.to_str()?.eq_ignore_ascii_case("app") {
        return None;
    }
    let bundle = NSBundle::bundleWithPath(&NSString::from_str(path.to_str()?))?;
    let package = bundle
        .objectForInfoDictionaryKey(&NSString::from_str("CFBundlePackageType"))
        .and_then(|value| nonempty_string(&value))?;
    if package != "APPL" {
        return None;
    }
    let executable = bundle.executablePath()?.to_string();
    let metadata = fs::metadata(executable).ok()?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
        return None;
    }
    let application = Application {
        name: bundle_name(&bundle, &path)?,
        path: path.to_str()?.to_owned(),
        icon: None,
    };
    Some((application, bundle))
}

pub fn application_at(path: &Path) -> Option<Application> {
    autoreleasepool(|_| {
        inspect_application(path).map(|(mut application, _)| {
            application.icon = application_icon(&application.path);
            application
        })
    })
}

fn add_url(
    applications: &mut Vec<Application>,
    url: &NSURL,
    filter: HandlerFilter,
) -> Option<String> {
    if !url.isFileURL() {
        return None;
    }
    let path = url.path()?.to_string();
    let (mut application, bundle) = inspect_application(Path::new(&path))?;
    let identifier = bundle.bundleIdentifier().map(|value| value.to_string());
    let is_terminal = identifier
        .as_deref()
        .is_some_and(|id| TERMINAL_IDS.contains(&id));
    let eligible = match filter {
        HandlerFilter::Terminal => is_terminal,
        // Xcode and other source editors are not offered: the handoff
        // supports only Zed and Visual Studio Code (G-07).
        HandlerFilter::Editor => {
            !is_terminal
                && identifier
                    .as_deref()
                    .is_some_and(|id| EDITOR_IDS.contains(&id))
        }
        // Terminals, download managers and virtual-machine helpers also register
        // HTTPS, so a URL scheme alone does not establish browser relevance.
        HandlerFilter::Browser => identifier
            .as_deref()
            .is_some_and(|id| BROWSER_IDS.contains(&id)),
        HandlerFilter::Any => true,
    };
    if !eligible {
        return None;
    }
    let path = application.path.clone();
    if !applications.iter().any(|current| current.path == path) {
        application.icon = application_icon(&path);
        applications.push(application);
    }
    Some(path)
}

/// Adds installed known apps and returns their paths in `identifiers` order.
fn add_known(
    workspace: &NSWorkspace,
    applications: &mut Vec<Application>,
    identifiers: &[&str],
) -> Vec<String> {
    identifiers
        .iter()
        .filter_map(|identifier| {
            let url =
                workspace.URLForApplicationWithBundleIdentifier(&NSString::from_str(identifier))?;
            add_url(applications, &url, HandlerFilter::Any)
        })
        .collect()
}

fn add_default_handler(
    applications: &mut Vec<Application>,
    url: &NSURL,
    filter: HandlerFilter,
) -> Option<String> {
    // The current HTTPS association is an explicit system choice, including
    // browsers outside the curated suggestions. The bundle must still be valid.
    let filter = match filter {
        HandlerFilter::Browser => HandlerFilter::Any,
        other => other,
    };
    add_url(applications, url, filter)
}

fn add_handlers(
    workspace: &NSWorkspace,
    url: &NSURL,
    applications: &mut Vec<Application>,
    filter: HandlerFilter,
) -> Option<String> {
    // All-handler enumeration arrived in macOS 12; the default-handler query is
    // available since 10.6. Known installed bundles supplement the older result.
    if workspace.respondsToSelector(sel!(URLsForApplicationsToOpenURL:)) {
        for application in workspace.URLsForApplicationsToOpenURL(url) {
            add_url(applications, &application, filter);
        }
    }
    workspace
        .URLForApplicationToOpenURL(url)
        .and_then(|application| add_default_handler(applications, &application, filter))
}

pub fn discover() -> Result<ApplicationCatalog, String> {
    autoreleasepool(|_| {
        let workspace = NSWorkspace::sharedWorkspace();
        let mut catalog = ApplicationCatalog::default();
        for (kind, applications, extensions, filter) in [
            (
                "terminal",
                &mut catalog.terminal,
                &["command"][..],
                HandlerFilter::Terminal,
            ),
            (
                "editor",
                &mut catalog.editor,
                &["swift", "rs"][..],
                HandlerFilter::Editor,
            ),
        ] {
            for extension in extensions {
                // NSWorkspace requires file URLs to exist. These private empty
                // files provide content types by extension and are never opened.
                let probe = tempfile::Builder::new()
                    .prefix("silo-application-discovery-")
                    .suffix(&format!(".{extension}"))
                    .tempfile()
                    .map_err(|error| format!("Could not prepare application discovery: {error}"))?;
                let path = probe
                    .path()
                    .to_str()
                    .ok_or("The temporary application discovery path is invalid")?;
                let url = NSURL::fileURLWithPath(&NSString::from_str(path));
                if let Some(path) = add_handlers(&workspace, &url, applications, filter) {
                    catalog.defaults.entry(kind.into()).or_insert(path);
                }
            }
        }
        for (kind, applications, identifiers) in [
            ("terminal", &mut catalog.terminal, TERMINAL_IDS),
            ("editor", &mut catalog.editor, EDITOR_IDS),
        ] {
            let installed = add_known(&workspace, applications, identifiers);
            if let Some(path) = supported_default(catalog.defaults.remove(kind), &installed) {
                catalog.defaults.insert(kind.into(), path);
            }
        }
        // Querying URL handlers does not open the URL or launch an application.
        let https = NSURL::URLWithString(&NSString::from_str("https://example.invalid"))
            .ok_or("macOS could not construct an HTTPS handler query")?;
        if let Some(path) = add_handlers(
            &workspace,
            &https,
            &mut catalog.browser,
            HandlerFilter::Browser,
        ) {
            catalog.defaults.insert("browser".into(), path);
        }
        add_known(&workspace, &mut catalog.browser, BROWSER_IDS);
        Ok(catalog)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundle(root: &Path, name: &str, package: &str, executable: bool) -> std::path::PathBuf {
        let path = root.join(format!("{name}.app"));
        fs::create_dir_all(path.join("Contents/MacOS")).unwrap();
        fs::write(path.join("Contents/Info.plist"), format!(r#"<?xml version="1.0" encoding="UTF-8"?>
            <!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
            <plist version="1.0"><dict>
              <key>CFBundlePackageType</key><string>{package}</string>
              <key>CFBundleExecutable</key><string>fixture</string>
              <key>CFBundleIdentifier</key><string>org.silo.tests.{name}</string>
              <key>CFBundleDisplayName</key><string>  Display {name}  </string>
              <key>CFBundleName</key><string>Fallback</string>
              <key>CFBundleDevelopmentRegion</key><string>en</string>
            </dict></plist>"#)).unwrap();
        if executable {
            let binary = path.join("Contents/MacOS/fixture");
            fs::write(&binary, "#!/bin/sh\nexit 99\n").unwrap();
            fs::set_permissions(binary, fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }

    #[test]
    fn chosen_application_uses_bundle_name_without_a_known_category_identifier() {
        let directory = tempfile::tempdir().unwrap();
        let path = bundle(directory.path(), "Custom", "APPL", true);
        let application = application_at(&path).unwrap();
        assert_eq!(application.name, "Display Custom");
        assert_eq!(
            application.path,
            fs::canonicalize(path).unwrap().to_str().unwrap()
        );
        assert!(application
            .icon
            .unwrap()
            .starts_with("data:image/png;base64,"));
    }

    #[test]
    fn thumbnail_is_a_bounded_png_and_restores_the_worker_context() {
        autoreleasepool(|_| {
            let source = unsafe {
                NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
                    NSBitmapImageRep::alloc(), std::ptr::null_mut(), 128, 128, 8, 4,
                    true, false, NSDeviceRGBColorSpace, 128 * 4, 32,
                )
            }.unwrap();
            // The allocated non-planar RGBA bitmap owns exactly these bytes.
            unsafe {
                source.bitmapData().write_bytes(255, 128 * 128 * 4);
            }
            let image = NSImage::initWithSize(NSImage::alloc(), NSSize::new(128.0, 128.0));
            image.addRepresentation(&source);
            let before = NSGraphicsContext::currentContext();
            let png = thumbnail_png(&image).unwrap();
            assert_eq!(NSGraphicsContext::currentContext(), before);
            assert!(png.length() <= 64 * 1024);
            let decoded = image::load_from_memory(&png.to_vec()).unwrap().to_rgba8();
            assert_eq!((decoded.width(), decoded.height()), (64, 64));
            assert!(decoded.as_raw().iter().all(|component| *component == 255));
        });
    }

    #[test]
    fn https_handlers_do_not_make_helpers_or_terminals_browser_choices() {
        autoreleasepool(|_| {
            let directory = tempfile::tempdir().unwrap();
            for (name, identifier, expected) in [
                ("Browser", "com.apple.Safari", true),
                ("Terminal", "com.googlecode.iterm2", false),
                ("Chat", "com.openai.codex", false),
                ("Downloader", "org.freedownloadmanager.fdm6", false),
                ("VirtualMachine", "com.parallels.desktop.console", false),
                ("Other", "org.silo.tests.Other", false),
            ] {
                let path = bundle(directory.path(), name, "APPL", true);
                let info = path.join("Contents/Info.plist");
                let contents = fs::read_to_string(&info).unwrap()
                    .replace(&format!("org.silo.tests.{name}"), identifier)
                    .replace("</dict></plist>", r#"
                        <key>CFBundleURLTypes</key><array><dict>
                        <key>CFBundleURLSchemes</key><array><string>http</string><string>https</string></array>
                        </dict></array></dict></plist>"#);
                fs::write(info, contents).unwrap();
                let url = NSURL::fileURLWithPath(&NSString::from_str(path.to_str().unwrap()));
                assert_eq!(
                    add_url(&mut Vec::new(), &url, HandlerFilter::Browser).is_some(),
                    expected,
                    "{name}"
                );
                assert!(
                    application_at(&path).is_some(),
                    "explicit choices remain valid: {name}"
                );
            }
        });
    }

    #[test]
    fn current_browser_default_is_preserved_outside_the_suggested_browser_list() {
        autoreleasepool(|_| {
            let directory = tempfile::tempdir().unwrap();
            let path = bundle(directory.path(), "UnlistedBrowser", "APPL", true);
            let url = NSURL::fileURLWithPath(&NSString::from_str(path.to_str().unwrap()));
            let mut browsers = Vec::new();
            assert!(add_url(&mut browsers, &url, HandlerFilter::Browser).is_none());
            let default = add_default_handler(&mut browsers, &url, HandlerFilter::Browser).unwrap();
            assert_eq!(browsers.len(), 1);
            assert_eq!(browsers[0].path, default);
            assert_eq!(browsers[0].name, "Display UnlistedBrowser");
            assert!(browsers[0]
                .icon
                .as_deref()
                .unwrap()
                .starts_with("data:image/png;base64,"));
            assert!(add_default_handler(&mut Vec::new(), &url, HandlerFilter::Terminal).is_none());
        });
    }

    #[test]
    fn rejects_stale_non_application_and_non_executable_bundles() {
        let directory = tempfile::tempdir().unwrap();
        assert!(application_at(&directory.path().join("Missing.app")).is_none());
        assert!(application_at(directory.path()).is_none());
        assert!(application_at(&bundle(directory.path(), "Framework", "FMWK", true)).is_none());
        assert!(application_at(&bundle(directory.path(), "NoExecutable", "APPL", false)).is_none());
        let path = bundle(directory.path(), "NotExecutable", "APPL", true);
        fs::set_permissions(
            path.join("Contents/MacOS/fixture"),
            fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        assert!(application_at(&path).is_none());
    }

    #[test]
    fn alias_selection_resolves_to_the_same_application_path() {
        let directory = tempfile::tempdir().unwrap();
        let path = bundle(directory.path(), "Target", "APPL", true);
        let alias = directory.path().join("Alias.app");
        std::os::unix::fs::symlink(&path, &alias).unwrap();
        assert_eq!(
            application_at(&path).unwrap().path,
            application_at(&alias).unwrap().path
        );
    }

    #[test]
    fn localized_name_precedes_unlocalized_display_name() {
        let directory = tempfile::tempdir().unwrap();
        let path = bundle(directory.path(), "Localized", "APPL", true);
        let resources = path.join("Contents/Resources/en.lproj");
        fs::create_dir_all(&resources).unwrap();
        fs::write(
            resources.join("InfoPlist.strings"),
            r#""CFBundleName" = "Localized application";"#,
        )
        .unwrap();
        assert_eq!(application_at(&path).unwrap().name, "Localized application");
    }

    #[test]
    fn shell_script_handlers_do_not_make_editors_terminal_choices() {
        autoreleasepool(|_| {
            let directory = tempfile::tempdir().unwrap();
            let path = bundle(directory.path(), "Editor", "APPL", true);
            let info = path.join("Contents/Info.plist");
            let contents = fs::read_to_string(&info)
                .unwrap()
                .replace("org.silo.tests.Editor", "com.microsoft.VSCode");
            fs::write(info, contents).unwrap();
            let url = NSURL::fileURLWithPath(&NSString::from_str(path.to_str().unwrap()));
            let mut terminals = Vec::new();
            assert!(add_url(&mut terminals, &url, HandlerFilter::Terminal).is_none());
            assert!(terminals.is_empty());
            let mut editors = Vec::new();
            assert!(add_url(&mut editors, &url, HandlerFilter::Editor).is_some());
            assert_eq!(editors.len(), 1);
        });
    }

    #[test]
    fn editor_suggestions_are_only_editors_the_handoff_can_open() {
        autoreleasepool(|_| {
            let directory = tempfile::tempdir().unwrap();
            for (name, identifier, expected) in [
                // Xcode is the usual .swift handler; it cannot open computers.
                ("Xcode", "com.apple.dt.Xcode", false),
                ("SourceEditor", "org.silo.tests.SourceEditor", false),
                ("Cursor", "com.todesktop.230313mzl4w4u92", false),
                ("VSCodium", "com.visualstudio.code.oss", false),
                ("Warp", "dev.warp.Warp-Stable", false),
                ("Code", "com.microsoft.VSCode", true),
                ("Zed", "dev.zed.Zed", true),
                ("ZedNightly", "dev.zed.Zed-Nightly", true),
            ] {
                let path = bundle(directory.path(), name, "APPL", true);
                let info = path.join("Contents/Info.plist");
                let contents = fs::read_to_string(&info)
                    .unwrap()
                    .replace(&format!("org.silo.tests.{name}"), identifier)
                    .replace(
                        "</dict></plist>",
                        r#"
                        <key>CFBundleDocumentTypes</key><array><dict>
                        <key>CFBundleTypeRole</key><string>Editor</string>
                        <key>LSItemContentTypes</key><array><string>public.source-code</string></array>
                        </dict></array></dict></plist>"#,
                    );
                fs::write(info, contents).unwrap();
                let url = NSURL::fileURLWithPath(&NSString::from_str(path.to_str().unwrap()));
                assert_eq!(
                    add_url(&mut Vec::new(), &url, HandlerFilter::Editor).is_some(),
                    expected,
                    "{name}"
                );
                assert!(
                    application_at(&path).is_some(),
                    "explicit choices remain valid: {name}"
                );
            }
        });
    }

    #[test]
    fn editor_handoffs_require_an_executable_bundled_cli() {
        for (identifier, relative) in [
            ("com.microsoft.VSCode", "Contents/Resources/app/bin/code"),
            ("dev.zed.Zed", "Contents/MacOS/cli"),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let path = bundle(directory.path(), "Editor", "APPL", true);
            let info = path.join("Contents/Info.plist");
            let contents = fs::read_to_string(&info)
                .unwrap()
                .replace("org.silo.tests.Editor", identifier);
            fs::write(info, contents).unwrap();
            let application = Application {
                name: "Editor".into(),
                path: path.to_str().unwrap().into(),
                icon: None,
            };
            assert!(editor_command(&application).is_err());
            let cli = path.join(relative);
            fs::create_dir_all(cli.parent().unwrap()).unwrap();
            fs::write(&cli, "#!/bin/sh\nexit 0\n").unwrap();
            fs::set_permissions(&cli, fs::Permissions::from_mode(0o644)).unwrap();
            assert!(
                editor_command(&application).is_err(),
                "{identifier}: a non-executable CLI was accepted"
            );
            fs::set_permissions(&cli, fs::Permissions::from_mode(0o755)).unwrap();
            assert!(editor_command(&application).is_ok());
        }
    }

    #[test]
    fn every_suggested_editor_has_a_handoff() {
        for identifier in EDITOR_IDS {
            assert!(editor_adapter(identifier).is_some(), "{identifier}");
        }
        let directory = tempfile::tempdir().unwrap();
        let path = bundle(directory.path(), "Nightly", "APPL", true);
        let info = path.join("Contents/Info.plist");
        let contents = fs::read_to_string(&info)
            .unwrap()
            .replace("org.silo.tests.Nightly", "dev.zed.Zed-Nightly");
        fs::write(info, contents).unwrap();
        let cli = path.join("Contents/MacOS/cli");
        fs::write(&cli, b"").unwrap();
        fs::set_permissions(&cli, fs::Permissions::from_mode(0o755)).unwrap();
        let application = application_at(&path).unwrap();
        let crate::applications::launch::EditorCommand {
            program: command,
            zed,
            ..
        } = editor_command(&application).unwrap();
        assert!(zed);
        assert!(command.ends_with("Contents/MacOS/cli"));
        assert!(editor_adapter("com.todesktop.230313mzl4w4u92").is_none());
    }

    #[test]
    fn terminal_suggestions_are_only_terminals_with_a_launcher() {
        autoreleasepool(|_| {
            let directory = tempfile::tempdir().unwrap();
            for (name, identifier, expected) in [
                ("Terminal", "com.apple.Terminal", true),
                ("Ghostty", "com.mitchellh.ghostty", true),
                ("ITerm", "com.googlecode.iterm2", true),
                ("Warp", "dev.warp.Warp-Stable", false),
                ("Alacritty", "org.alacritty", false),
                ("WezTerm", "com.github.wez.wezterm", false),
            ] {
                let path = bundle(directory.path(), name, "APPL", true);
                let info = path.join("Contents/Info.plist");
                let contents = fs::read_to_string(&info)
                    .unwrap()
                    .replace(&format!("org.silo.tests.{name}"), identifier);
                fs::write(info, contents).unwrap();
                let url = NSURL::fileURLWithPath(&NSString::from_str(path.to_str().unwrap()));
                assert_eq!(
                    add_url(&mut Vec::new(), &url, HandlerFilter::Terminal).is_some(),
                    expected,
                    "{name}"
                );
            }
        });
    }

    #[test]
    fn an_unsupported_system_default_falls_back_to_a_supported_app() {
        let installed = [
            "/Applications/Visual Studio Code.app".to_string(),
            "/Applications/Zed.app".to_string(),
        ];
        assert_eq!(
            supported_default(Some("/Applications/Zed.app".into()), &installed).as_deref(),
            Some("/Applications/Zed.app")
        );
        assert_eq!(
            supported_default(None, &installed).as_deref(),
            Some("/Applications/Visual Studio Code.app")
        );
        assert_eq!(supported_default(None, &[]), None);
    }
}

pub fn open_terminal(application: &Application, command: &str) -> Result<(), String> {
    let id = autoreleasepool(|_| {
        let url = NSURL::fileURLWithPath(&NSString::from_str(&application.path));
        NSBundle::bundleWithURL(&url)
            .and_then(|b| b.bundleIdentifier())
            .map(|id| id.to_string())
    });
    if id.as_deref() == Some("com.mitchellh.ghostty") {
        // Callers run on a worker. osascript keeps the first-run Automation
        // prompt or a busy Ghostty from freezing Silo's main thread (G-05).
        return match run_bounded(ghostty_launch(&application.path, command), GHOSTTY_TIMEOUT) {
            Ok(status) if status.success() => Ok(()),
            Ok(_) => Err("Ghostty could not open the terminal. Use Ghostty 1.3 or newer and allow Silo in System Settings → Privacy & Security → Automation.".into()),
            Err(Bounded::Spawn) => Err("Could not contact the terminal launcher.".into()),
            Err(Bounded::TimedOut) => Err("Ghostty did not respond. Check its Automation permission before retrying.".into()),
        };
    }
    if !matches!(
        id.as_deref(),
        Some("com.apple.Terminal" | "com.googlecode.iterm2")
    ) {
        return Err("This terminal does not have a supported command launcher. Choose Ghostty, Terminal, or iTerm in Settings.".into());
    }
    let file = crate::terminal::command_file(command)?;
    let mut launch = std::process::Command::new("/usr/bin/open");
    launch.arg("-a").arg(&application.path).arg(&file);
    let result = crate::terminal::launch(launch);
    if result.is_err() {
        let _ = fs::remove_file(file);
    }
    result
}
/// Ghostty's scripting dictionary (1.3+). The command arrives as `argv`,
/// never as AppleScript source, so it needs no AppleScript escaping.
const GHOSTTY_SCRIPT: &str = r#"on run argv
 with timeout of 30 seconds
  tell application __SILO_GHOSTTY_PATH__
   activate
   set cfg to new surface configuration
   set command of cfg to item 1 of argv
   set wait after command of cfg to true
   if (count of windows) is 0 then
    new window with configuration cfg
   else
    set newTab to new tab in front window with configuration cfg
    select tab newTab
   end if
  end tell
 end timeout
end run"#;
/// Longer than the script's own Apple Event timeout, so a first-run
/// Automation prompt has time to be answered.
const GHOSTTY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(45);

fn ghostty_script(application_path: &str) -> String {
    // A literal target loads terminology from the selected bundle too.
    let path = application_path
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t");
    GHOSTTY_SCRIPT.replace("__SILO_GHOSTTY_PATH__", &format!("\"{path}\""))
}

fn ghostty_launch(application_path: &str, command: &str) -> std::process::Command {
    let mut launch = std::process::Command::new("/usr/bin/osascript");
    launch
        .arg("-e")
        .arg(ghostty_script(application_path))
        .arg("--")
        .arg(command);
    launch
}

#[derive(Debug, PartialEq)]
enum Bounded {
    Spawn,
    TimedOut,
}

/// Runs a helper to completion or kills it at the deadline.
fn run_bounded(
    mut command: std::process::Command,
    timeout: std::time::Duration,
) -> Result<std::process::ExitStatus, Bounded> {
    let mut child = command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|_| Bounded::Spawn)?;
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(25))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Bounded::TimedOut);
            }
        }
    }
}

#[cfg(test)]
mod terminal_tests {
    use super::*;

    #[test]
    fn ghostty_targets_the_selected_bundle_instead_of_its_registered_copy() {
        for path in ["/Applications/Ghostty A.app", "/Applications/Ghostty B.app"] {
            let launch = ghostty_launch(path, "ssh example");
            let args: Vec<_> = launch.get_args().map(|arg| arg.to_str().unwrap()).collect();
            assert!(args[1].contains(&format!("tell application \"{path}\"")));
            assert!(!args[1].contains("tell application id"));
            assert_eq!(&args[2..], &["--", "ssh example"]);
        }
    }

    #[test]
    fn ghostty_runs_in_osascript_with_the_command_as_data() {
        let command = r#"'/usr/bin/ssh' '-F' '/a "b"\c' $(touch /tmp/never)"#;
        let launch = ghostty_launch("/Applications/Ghostty.app", command);
        assert_eq!(launch.get_program(), "/usr/bin/osascript");
        let args: Vec<_> = launch.get_args().map(|arg| arg.to_str().unwrap()).collect();
        assert_eq!(
            args,
            [
                "-e",
                &ghostty_script("/Applications/Ghostty.app"),
                "--",
                command
            ]
        );
        assert!(GHOSTTY_SCRIPT.contains("set command of cfg to item 1 of argv"));
    }

    #[test]
    fn ghostty_bundle_paths_remain_literal_applescript_text() {
        let path = "/tmp/a\"b\\c\n\r\t.app";
        let script = ghostty_script(path);
        let target = script
            .lines()
            .find_map(|line| line.trim().strip_prefix("tell application "))
            .unwrap();
        // Evaluate only the target literal, without contacting an application.
        let output = std::process::Command::new("/usr/bin/osascript")
            .args(["-e", &format!("return {target}")])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            format!("{path}\n")
        );
    }

    #[test]
    fn osascript_passes_the_command_through_argv_unchanged() {
        // Same argument shape as Ghostty's launch, without contacting any app.
        let command = "-x 'a\"b' \\n $(y)";
        let output = std::process::Command::new("/usr/bin/osascript")
            .args([
                "-e",
                "on run argv\n return item 1 of argv\nend run",
                "--",
                command,
            ])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            format!("{command}\n")
        );
    }

    #[test]
    fn browser_launcher_reports_timeout_failure_and_success() {
        let mut sleeper = std::process::Command::new("/bin/sleep");
        sleeper.arg("0.2");
        assert!(launch_browser(sleeper, std::time::Duration::from_millis(20)).is_err());
        assert!(launch_browser(
            std::process::Command::new("/usr/bin/false"),
            std::time::Duration::from_secs(1)
        )
        .is_err());
        assert!(launch_browser(
            std::process::Command::new("/usr/bin/true"),
            std::time::Duration::from_secs(1)
        )
        .is_ok());
    }

    #[test]
    fn an_unresponsive_helper_is_stopped_at_the_deadline() {
        let started = std::time::Instant::now();
        let mut sleeper = std::process::Command::new("/bin/sleep");
        sleeper.arg("30");
        assert_eq!(
            run_bounded(sleeper, std::time::Duration::from_millis(200)),
            Err(Bounded::TimedOut)
        );
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
        let status = run_bounded(
            std::process::Command::new("/usr/bin/false"),
            GHOSTTY_TIMEOUT,
        )
        .unwrap();
        assert!(!status.success());
    }
}
