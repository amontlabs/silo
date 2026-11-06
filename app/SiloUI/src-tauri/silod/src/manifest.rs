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
        let manifest = Self::read(resources)?;
        let channel = manifest.channel()?;
        if bundle_identifier != format!("{}.engine", channel.identifier()) {
            return Err(format!(
                "Engine manifest channel {} does not match bundle identifier {bundle_identifier}",
                manifest.channel_identifier
            ));
        }
        manifest.validate()
    }

    /// The manifest of an engine that has no bundle (a Linux host): the same checks as
    /// [`EngineManifest::load`] except the bundle identifier, which does not exist there.
    /// Nothing seals the file yet; signed Linux archives are roadmap milestone 11.
    #[cfg(target_os = "linux")]
    pub fn load_unbundled(resources: &Path) -> Result<Self, String> {
        let manifest = Self::read(resources)?;
        manifest.channel()?;
        manifest.validate()
    }

    fn read(resources: &Path) -> Result<Self, String> {
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
        serde_json::from_slice(&bytes).map_err(|error| format!("Invalid engine manifest: {error}"))
    }

    fn validate(self) -> Result<Self, String> {
        Version::parse(&self.engine_version)
            .map_err(|error| format!("Invalid engineVersion in manifest: {error}"))?;
        if self.engine_build.trim().is_empty() {
            return Err("engineBuild in manifest must not be empty".to_owned());
        }
        if self.protocol_min == 0
            || self.protocol_min > self.protocol_max
            || PROTOCOL_VERSION < self.protocol_min
            || PROTOCOL_VERSION > self.protocol_max
        {
            return Err(
                "Engine manifest protocol range does not include this silod build".to_owned(),
            );
        }
        Ok(self)
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

    #[cfg(target_os = "linux")]
    #[test]
    fn an_unbundled_manifest_names_its_channel_and_keeps_every_other_check() {
        let resources = tempfile::tempdir().unwrap();
        let write = |value: serde_json::Value| {
            fs::write(
                resources.path().join("engine-manifest.json"),
                value.to_string(),
            )
            .unwrap();
        };
        let valid = serde_json::json!({
            "channelIdentifier": DEVELOPMENT_IDENTIFIER,
            "engineVersion": "0.2.0",
            "engineBuild": "linux-test",
            "protocolMin": PROTOCOL_VERSION,
            "protocolMax": PROTOCOL_VERSION
        });
        write(valid.clone());
        let manifest = EngineManifest::load_unbundled(resources.path()).unwrap();
        assert_eq!(manifest.channel().unwrap(), Channel::Development);

        for (key, value) in [
            ("channelIdentifier", serde_json::json!("org.silo.other")),
            ("engineVersion", serde_json::json!("not-a-version")),
            ("engineBuild", serde_json::json!(" ")),
            ("protocolMin", serde_json::json!(PROTOCOL_VERSION + 1)),
        ] {
            let mut changed = valid.clone();
            changed[key] = value;
            write(changed);
            assert!(
                EngineManifest::load_unbundled(resources.path()).is_err(),
                "{key}"
            );
        }

        fs::remove_file(resources.path().join("engine-manifest.json")).unwrap();
        let elsewhere = resources.path().join("elsewhere.json");
        fs::write(&elsewhere, valid.to_string()).unwrap();
        std::os::unix::fs::symlink(&elsewhere, resources.path().join("engine-manifest.json"))
            .unwrap();
        assert!(EngineManifest::load_unbundled(resources.path()).is_err());
    }
}
