use std::path::{Path, PathBuf};

use core_foundation::url::CFURL;
use security_framework::os::macos::code_signing::{Flags, SecRequirement, SecStaticCode};
use silo_engine::channel::Channel;

pub fn bundle_path_for_executable(executable: &Path) -> Result<PathBuf, String> {
    let macos_directory = executable
        .parent()
        .ok_or_else(|| "silod executable has no parent directory".to_owned())?;
    let contents_directory = macos_directory
        .parent()
        .ok_or_else(|| "silod executable is not inside Contents/MacOS".to_owned())?;
    if macos_directory
        .file_name()
        .is_none_or(|name| name != "MacOS")
        || contents_directory
            .file_name()
            .is_none_or(|name| name != "Contents")
    {
        return Err("silod must run from a signed application bundle".to_owned());
    }
    let bundle = contents_directory
        .parent()
        .ok_or_else(|| "application bundle has no parent".to_owned())?;
    if bundle
        .components()
        .any(|component| component.as_os_str() == "AppTranslocation")
    {
        return Err(
            "translocated application bundles cannot register or serve an engine".to_owned(),
        );
    }
    Ok(bundle.to_path_buf())
}

pub fn validate_bundle(bundle_path: &Path, bundle_identifier: &str) -> Result<(), String> {
    let requirement = requirement_for_bundle(
        bundle_identifier,
        option_env!("SILOD_PRODUCTION_CODE_REQUIREMENT"),
        option_env!("SILOD_DEVELOPMENT_CODE_REQUIREMENT"),
    )?;
    let url = CFURL::from_path(bundle_path, true).ok_or_else(|| {
        format!(
            "Cannot create a Security.framework URL for {}",
            bundle_path.display()
        )
    })?;
    let code = SecStaticCode::from_path(&url, Flags::NONE).map_err(|error| {
        format!(
            "Cannot inspect signature for {}: {error}",
            bundle_path.display()
        )
    })?;
    let requirement: SecRequirement = requirement
        .parse()
        .map_err(|error| format!("Invalid silod code-signing requirement: {error}"))?;
    let flags = Flags::CHECK_ALL_ARCHITECTURES
        | Flags::CHECK_NESTED_CODE
        | Flags::STRICT_VALIDATE
        | Flags::NO_NETWORK_ACCESS;
    code.check_validity(flags, &requirement).map_err(|error| {
        format!(
            "Code signature validation failed for {}: {error}",
            bundle_path.display()
        )
    })
}

fn requirement_for_bundle(
    bundle_identifier: &str,
    production_requirement: Option<&str>,
    development_requirement: Option<&str>,
) -> Result<String, String> {
    if bundle_identifier == engine_bundle_identifier(Channel::Production) {
        return pinned_requirement(production_requirement, bundle_identifier);
    }
    if bundle_identifier == engine_bundle_identifier(Channel::Development) {
        let base = development_requirement
            .filter(|requirement| !requirement.trim().is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("identifier \"{bundle_identifier}\""));
        return Ok(requirement_with_identifier(&base, bundle_identifier));
    }
    Err(format!(
        "unsupported silod bundle identifier: {bundle_identifier}"
    ))
}

fn engine_bundle_identifier(channel: Channel) -> String {
    format!("{}.engine", channel.identifier())
}

fn pinned_requirement(
    requirement: Option<&str>,
    bundle_identifier: &str,
) -> Result<String, String> {
    let requirement = requirement
        .filter(|requirement| !requirement.trim().is_empty())
        .ok_or_else(|| {
            "production silod requires SILOD_PRODUCTION_CODE_REQUIREMENT at build time".to_owned()
        })?;
    Ok(requirement_with_identifier(requirement, bundle_identifier))
}

fn requirement_with_identifier(requirement: &str, bundle_identifier: &str) -> String {
    format!("({requirement}) and identifier \"{bundle_identifier}\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_bundle_requires_a_build_time_pinned_requirement() {
        let production = engine_bundle_identifier(Channel::Production);
        assert!(requirement_for_bundle(&production, None, None).is_err());
        assert_eq!(
            requirement_for_bundle(
                &production,
                Some("anchor apple generic and certificate leaf[subject.OU] = \"ABCDE12345\""),
                None
            )
            .unwrap(),
            "(anchor apple generic and certificate leaf[subject.OU] = \"ABCDE12345\") and identifier \"org.silo.preview.engine\""
        );
    }

    #[test]
    fn development_bundle_uses_its_identity_or_a_pinned_requirement() {
        let development = engine_bundle_identifier(Channel::Development);
        assert_eq!(
            requirement_for_bundle(&development, None, None).unwrap(),
            format!("(identifier \"{development}\") and identifier \"{development}\"")
        );
        assert!(requirement_for_bundle("org.silo.other.engine", None, None).is_err());
    }
}
