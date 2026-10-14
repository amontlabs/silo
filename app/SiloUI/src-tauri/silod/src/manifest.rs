use std::{fs, path::Path};

use semver::Version;
use serde::Deserialize;
use silo_api::transport::PROTOCOL_VERSION;
use silo_engine::channel::{Channel, DEVELOPMENT_IDENTIFIER, PRODUCTION_IDENTIFIER};

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EngineManifest {
    pub channel_identifier: String,
    pub engine_version: String,
    pub engine_build: String,
    pub protocol_min: u32,
    pub protocol_max: u32,
}

impl EngineManifest {
    pub fn load(resources: &Path, bundle_identifier: &str) -> Result<Self, String> {
        let path = resources.join("engine-manifest.json");
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            format!(
                "Cannot read sealed engine manifest {}: {error}",
                path.display()
            )
        })?;
        if !metadata.file_type().is_file() || metadata.len() > 64 * 1024 {
            return Err("Engine manifest must be a regular file smaller than 64 KiB".to_owned());
        }
        let bytes = fs::read(&path).map_err(|error| {
            format!(
                "Cannot read sealed engine manifest {}: {error}",
                path.display()
            )
        })?;
        if bytes.len() > 64 * 1024 {
            return Err("Engine manifest exceeds 64 KiB".to_owned());
        }
        let manifest: Self = serde_json::from_slice(&bytes)
            .map_err(|error| format!("Invalid engine manifest: {error}"))?;
        let channel = manifest.channel()?;
        if bundle_identifier != format!("{}.engine", channel.identifier()) {
            return Err(format!(
                "Engine manifest channel {} does not match bundle identifier {bundle_identifier}",
                manifest.channel_identifier
            ));
        }
        Version::parse(&manifest.engine_version)
            .map_err(|error| format!("Invalid engineVersion in manifest: {error}"))?;
        if manifest.engine_build.trim().is_empty() {
            return Err("engineBuild in manifest must not be empty".to_owned());
        }
        if manifest.protocol_min == 0
            || manifest.protocol_min > manifest.protocol_max
            || PROTOCOL_VERSION < manifest.protocol_min
            || PROTOCOL_VERSION > manifest.protocol_max
        {
            return Err(
                "Engine manifest protocol range does not include this silod build".to_owned(),
            );
        }
        Ok(manifest)
    }

    pub fn channel(&self) -> Result<Channel, String> {
        match self.channel_identifier.as_str() {
            PRODUCTION_IDENTIFIER => Ok(Channel::Production),
            DEVELOPMENT_IDENTIFIER => Ok(Channel::Development),
            other => Err(format!(
                "unsupported channelIdentifier in engine manifest: {other}"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_channel_and_bundle_identity_must_agree() {
        let resources = tempfile::tempdir().unwrap();
        fs::write(
            resources.path().join("engine-manifest.json"),
            serde_json::json!({
                "channelIdentifier": PRODUCTION_IDENTIFIER,
                "engineVersion": "0.1.0",
                "engineBuild": "p2-silod-scaffold",
                "protocolMin": PROTOCOL_VERSION,
                "protocolMax": PROTOCOL_VERSION
            })
            .to_string(),
        )
        .unwrap();
        let manifest = EngineManifest::load(resources.path(), "org.silo.preview.engine").unwrap();
        assert_eq!(manifest.channel().unwrap(), Channel::Production);
        assert!(EngineManifest::load(resources.path(), "org.silo.dev.engine").is_err());
    }

    #[test]
    fn manifest_rejects_an_unrecognized_channel_or_protocol_range() {
        let resources = tempfile::tempdir().unwrap();
        let write = |channel: &str, min: u32, max: u32| {
            fs::write(
                resources.path().join("engine-manifest.json"),
                serde_json::json!({
                    "channelIdentifier": channel,
                    "engineVersion": "0.1.0",
                    "engineBuild": "test",
                    "protocolMin": min,
                    "protocolMax": max
                })
                .to_string(),
            )
            .unwrap();
        };
        write("org.silo.other", PROTOCOL_VERSION, PROTOCOL_VERSION);
        assert!(EngineManifest::load(resources.path(), "org.silo.other.engine").is_err());
        write(
            DEVELOPMENT_IDENTIFIER,
            PROTOCOL_VERSION + 1,
            PROTOCOL_VERSION + 2,
        );
        assert!(EngineManifest::load(resources.path(), "org.silo.dev.engine").is_err());
    }
}
