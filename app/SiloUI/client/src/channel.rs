//! The two Silo build channels as a client needs them: the names and the engine socket.
//! `silo-engine` keeps its own copy for engine paths; the identifiers are the contract.

use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

pub const PRODUCTION_IDENTIFIER: &str = "org.silo.preview";
pub const DEVELOPMENT_IDENTIFIER: &str = "org.silo.dev";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Production,
    Development,
}

static CURRENT: OnceLock<Channel> = OnceLock::new();

/// Fixes the channel for this process from the bundle identifier. Only the exact
/// production identifier selects production.
pub fn init(identifier: &str) {
    let _ = CURRENT.set(Channel::from_identifier(identifier));
}

/// The running channel; production before `init` (unit tests).
pub fn current() -> Channel {
    CURRENT.get().copied().unwrap_or(Channel::Production)
}

impl Channel {
    pub fn from_identifier(identifier: &str) -> Self {
        if identifier == PRODUCTION_IDENTIFIER {
            Self::Production
        } else {
            Self::Development
        }
    }

    fn pick<T>(self, production: T, development: T) -> T {
        match self {
            Self::Production => production,
            Self::Development => development,
        }
    }

    pub fn identifier(self) -> &'static str {
        self.pick(PRODUCTION_IDENTIFIER, DEVELOPMENT_IDENTIFIER)
    }

    pub fn product_name(self) -> &'static str {
        self.pick("Silo", "Silo Dev")
    }

    pub fn is_development(self) -> bool {
        self == Self::Development
    }

    pub fn tray_id(self) -> &'static str {
        self.identifier()
    }

    pub fn tray_title(self) -> &'static str {
        self.pick("Silo", "Silo Dev")
    }

    /// `~/<this>`: the channel's private account directory.
    pub fn state_dir_name(self) -> &'static str {
        self.pick(".silo", ".silo-dev")
    }

    pub fn state_dir(self, home: &Path) -> PathBuf {
        home.join(self.state_dir_name())
    }

    /// The engine's socket, `~/.silo/engine.sock`.
    pub fn engine_socket(self, home: &Path) -> PathBuf {
        self.state_dir(home).join("engine.sock")
    }

    /// The engine's owner record, `~/.silo/engine-owner.json`.
    pub fn engine_owner_record(self, home: &Path) -> PathBuf {
        self.state_dir(home).join("engine-owner.json")
    }

    /// The bundle identifier of the engine helper, `<identifier>.engine`.
    pub fn engine_bundle_identifier(self) -> String {
        format!("{}.engine", self.identifier())
    }

    /// The LaunchAgent plist name inside the calling bundle.
    pub fn launch_agent_plist(self) -> String {
        format!("{}.plist", self.engine_bundle_identifier())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_the_released_channels() {
        let (p, d) = (Channel::Production, Channel::Development);
        assert_eq!(p.identifier(), "org.silo.preview");
        assert_eq!(p.product_name(), "Silo");
        assert_eq!(d.product_name(), "Silo Dev");
        assert_eq!(p.launch_agent_plist(), "org.silo.preview.engine.plist");
        assert_eq!(d.launch_agent_plist(), "org.silo.dev.engine.plist");
        let home = Path::new("/h");
        assert_eq!(p.engine_socket(home), Path::new("/h/.silo/engine.sock"));
        assert_eq!(d.engine_socket(home), Path::new("/h/.silo-dev/engine.sock"));
        assert_eq!(
            Channel::from_identifier("org.silo.preview.x"),
            Channel::Development
        );
    }
}
