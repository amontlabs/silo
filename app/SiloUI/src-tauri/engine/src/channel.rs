//! The two Silo build channels and every name that must differ between them.
//!
//! Only two builds exist: PRODUCTION (`org.silo.preview`, "Silo", what users
//! install) and DEVELOPMENT (`org.silo.dev`, "Silo Dev", what `npm run desktop`
//! and the debug bundle produce). They never share state, so every host-visible
//! name (home directory, keychain services, remote bridge, editor
//! profile, desktop entry, tray identity) is derived here from the bundle
//! identifier. Production values are exactly the names released builds have
//! always used; changing one would orphan existing users' data.
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

/// The credential-store collections Silo owns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keychain {
    Github,
    Secrets,
    ProxyCa,
}

static CURRENT: OnceLock<Channel> = OnceLock::new();

/// Fixes the channel for this process from the bundle identifier. Call once, before
/// anything reads a channel name. Only the exact production identifier selects
/// production, so an unexpected identifier can never reach production's data.
pub fn init(identifier: &str) {
    let _ = CURRENT.set(Channel::from_identifier(identifier));
}

/// The running channel. Code that runs before `init` (unit tests) sees production.
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

    /// `~/<this>`: Silo's private account directory (runtime aliases, editor
    /// computer files, connections state, desktop sockets).
    pub fn state_dir_name(self) -> &'static str {
        self.pick(".silo", ".silo-dev")
    }

    pub fn state_dir(self, home: &Path) -> PathBuf {
        home.join(self.state_dir_name())
    }

    pub fn keychain_service(self, collection: Keychain) -> &'static str {
        match (self, collection) {
            (Self::Production, Keychain::Github) => "org.silo.Silo.github",
            (Self::Production, Keychain::Secrets) => "org.silo.Silo.secrets",
            (Self::Production, Keychain::ProxyCa) => "org.silo.Silo.proxy-ca",
            (Self::Development, Keychain::Github) => "org.silo.dev.github",
            (Self::Development, Keychain::Secrets) => "org.silo.dev.secrets",
            (Self::Development, Keychain::ProxyCa) => "org.silo.dev.proxy-ca",
        }
    }

    /// The link under `~/.local/bin` that SSH sessions of a controlling device run.
    pub fn remote_bridge_name(self) -> &'static str {
        self.pick("silo-remote", "silo-remote-dev")
    }

    /// The forced command of an installed connections key.
    pub fn remote_bridge_command(self) -> String {
        format!(
            "exec ~/.local/bin/{} --remote-bridge",
            self.remote_bridge_name()
        )
    }

    pub fn remote_key_comment(self) -> &'static str {
        self.pick("Silo remote management", "Silo Dev remote management")
    }

    /// Prefix of the SSH host aliases for other devices' computers.
    pub fn remote_alias_prefix(self) -> &'static str {
        self.pick("silo-remote", "silo-dev-remote")
    }

    pub fn vscode_profile(self) -> &'static str {
        self.pick("Silo", "Silo Dev")
    }

    pub fn desktop_entry_id(self) -> &'static str {
        self.pick("org.silo.preview.desktop", "org.silo.dev.desktop")
    }

    pub fn desktop_entry_name(self) -> &'static str {
        self.pick("Silo Preview", "Silo Dev")
    }

    /// The product name notification servers know this app by.
    pub fn notification_desktop_entry(self) -> &'static str {
        self.pick("Silo", "Silo Dev")
    }

    pub fn tray_id(self) -> &'static str {
        self.identifier()
    }

    pub fn tray_title(self) -> &'static str {
        self.pick("Silo", "Silo Dev")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOTH: [Channel; 2] = [Channel::Production, Channel::Development];

    #[test]
    fn production_names_are_exactly_the_released_names() {
        let production = Channel::Production;
        assert_eq!(production.identifier(), "org.silo.preview");
        assert_eq!(production.product_name(), "Silo");
        assert_eq!(production.state_dir_name(), ".silo");
        assert_eq!(
            production.keychain_service(Keychain::Github),
            "org.silo.Silo.github"
        );
        assert_eq!(
            production.keychain_service(Keychain::Secrets),
            "org.silo.Silo.secrets"
        );
        assert_eq!(
            production.keychain_service(Keychain::ProxyCa),
            "org.silo.Silo.proxy-ca"
        );
        assert_eq!(production.remote_bridge_name(), "silo-remote");
        assert_eq!(
            production.remote_bridge_command(),
            "exec ~/.local/bin/silo-remote --remote-bridge"
        );
        assert_eq!(production.remote_key_comment(), "Silo remote management");
        assert_eq!(production.remote_alias_prefix(), "silo-remote");
        assert_eq!(production.vscode_profile(), "Silo");
        assert_eq!(production.desktop_entry_id(), "org.silo.preview.desktop");
        assert_eq!(production.desktop_entry_name(), "Silo Preview");
        assert_eq!(production.notification_desktop_entry(), "Silo");
        assert_eq!(production.tray_id(), "org.silo.preview");
        assert_eq!(production.tray_title(), "Silo");
    }

    #[test]
    fn development_names_never_equal_production_names() {
        let (p, d) = (Channel::Production, Channel::Development);
        assert_eq!(d.identifier(), "org.silo.dev");
        assert_eq!(d.product_name(), "Silo Dev");
        let pairs = [
            (p.identifier(), d.identifier()),
            (p.product_name(), d.product_name()),
            (p.state_dir_name(), d.state_dir_name()),
            (
                p.keychain_service(Keychain::Github),
                d.keychain_service(Keychain::Github),
            ),
            (
                p.keychain_service(Keychain::Secrets),
                d.keychain_service(Keychain::Secrets),
            ),
            (
                p.keychain_service(Keychain::ProxyCa),
                d.keychain_service(Keychain::ProxyCa),
            ),
            (p.remote_bridge_name(), d.remote_bridge_name()),
            (p.remote_key_comment(), d.remote_key_comment()),
            (p.remote_alias_prefix(), d.remote_alias_prefix()),
            (p.vscode_profile(), d.vscode_profile()),
            (p.desktop_entry_id(), d.desktop_entry_id()),
            (p.desktop_entry_name(), d.desktop_entry_name()),
            (
                p.notification_desktop_entry(),
                d.notification_desktop_entry(),
            ),
            (p.tray_id(), d.tray_id()),
            (p.tray_title(), d.tray_title()),
        ];
        for (production, development) in pairs {
            assert_ne!(production, development);
        }
        assert_ne!(p.remote_bridge_command(), d.remote_bridge_command());
        assert_eq!(
            d.remote_bridge_command(),
            "exec ~/.local/bin/silo-remote-dev --remote-bridge"
        );
    }

    #[test]
    fn no_development_directory_or_alias_lies_inside_production() {
        let home = Path::new("/home/u");
        assert!(!Channel::Development
            .state_dir(home)
            .starts_with(Channel::Production.state_dir(home)));
        // A production alias prefix must not match a development alias with a
        // trailing separator, since aliases are matched by prefix.
        assert!(!format!("{}-", Channel::Development.remote_alias_prefix())
            .starts_with(&format!("{}-", Channel::Production.remote_alias_prefix())));
        for channel in BOTH {
            assert!(!channel.remote_key_comment().is_empty());
        }
    }

    #[test]
    fn only_the_exact_production_identifier_selects_production() {
        assert_eq!(
            Channel::from_identifier("org.silo.preview"),
            Channel::Production
        );
        for other in [
            "org.silo.dev",
            "org.silo.preview.dev",
            "",
            "ORG.SILO.PREVIEW",
        ] {
            assert_eq!(
                Channel::from_identifier(other),
                Channel::Development,
                "{other}"
            );
        }
    }

    #[test]
    fn tauri_configurations_match_the_channel_identifiers() {
        let read = |name: &str| -> serde_json::Value {
            let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(name);
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
        };
        let production = read("tauri.conf.json");
        assert_eq!(production["identifier"], PRODUCTION_IDENTIFIER);
        assert_eq!(
            production["productName"],
            Channel::Production.product_name()
        );
        let development = read("tauri.dev.conf.json");
        assert_eq!(development["identifier"], DEVELOPMENT_IDENTIFIER);
        assert_eq!(
            development["productName"],
            Channel::Development.product_name()
        );
    }
}
