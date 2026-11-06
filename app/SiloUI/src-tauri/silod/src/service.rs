use objc2::rc::autoreleasepool;
use objc2_foundation::{NSBundle, NSString};
use objc2_service_management::{SMAppService, SMAppServiceStatus};
use silo_engine::channel::Channel;

use crate::{
    code_signature::{bundle_path_for_executable, validate_bundle},
    manifest::EngineManifest,
    paths::default_resource_dir,
};

pub fn current_bundle_identifier() -> Result<String, String> {
    NSBundle::mainBundle()
        .bundleIdentifier()
        .map(|value| value.to_string())
        .ok_or_else(|| "silod has no bundle identifier".to_owned())
}

pub fn current_bundle_manifest() -> Result<EngineManifest, String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let resources = default_resource_dir(&executable)?;
    let bundle_identifier = current_bundle_identifier()?;
    let bundle_path = bundle_path_for_executable(&executable)?;
    validate_bundle(&bundle_path, &bundle_identifier)?;
    EngineManifest::load(&resources, &bundle_identifier)
}

pub fn engine_bundle_identifier(channel: Channel) -> String {
    format!("{}.engine", channel.identifier())
}

pub fn launch_agent_plist_name(channel: Channel) -> String {
    format!("{}.plist", engine_bundle_identifier(channel))
}

pub fn run_command(args: &[String]) -> Result<i32, String> {
    autoreleasepool(|_| run_command_inner(args))
}

fn run_command_inner(args: &[String]) -> Result<i32, String> {
    let Some(command) = args.first().map(String::as_str) else {
        return Err("usage: silod --service status|register|unregister|open-settings".to_owned());
    };
    if args.len() != 1 {
        return Err("--service accepts exactly one command".to_owned());
    }
    let manifest = current_bundle_manifest()?;
    let channel = manifest.channel()?;
    let plist_name = NSString::from_str(&launch_agent_plist_name(channel));
    // SMAppService resolves this plist from the calling bundle's
    // Contents/Library/LaunchAgents directory.
    let service = unsafe { SMAppService::agentServiceWithPlistName(&plist_name) };
    match command {
        "status" => {
            println!("{}", status_name(unsafe { service.status() }));
            Ok(0)
        }
        "register" => {
            unsafe { service.registerAndReturnError() }
                .map_err(|error| error.localizedDescription().to_string())?;
            println!("{}", status_name(unsafe { service.status() }));
            Ok(0)
        }
        "unregister" => {
            unsafe { service.unregisterAndReturnError() }
                .map_err(|error| error.localizedDescription().to_string())?;
            println!("{}", status_name(unsafe { service.status() }));
            Ok(0)
        }
        "open-settings" => {
            unsafe { SMAppService::openSystemSettingsLoginItems() };
            Ok(0)
        }
        _ => Err(format!("unsupported service command: {command}")),
    }
}

fn status_name(status: SMAppServiceStatus) -> &'static str {
    match status {
        SMAppServiceStatus::NotRegistered => "notRegistered",
        SMAppServiceStatus::Enabled => "enabled",
        SMAppServiceStatus::RequiresApproval => "requiresApproval",
        SMAppServiceStatus::NotFound => "notFound",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_names_are_derived_from_the_channel() {
        assert_eq!(
            launch_agent_plist_name(Channel::Production),
            "org.silo.preview.engine.plist"
        );
        assert_eq!(
            launch_agent_plist_name(Channel::Development),
            "org.silo.dev.engine.plist"
        );
    }
}
