//! Export the native channel names for development and packaging scripts.
#[allow(dead_code)]
#[path = "../host/src/channel.rs"]
mod channel;

use channel::{Channel, Keychain};

fn names(channel: Channel) -> String {
    format!(
        r#"{{"identifier":{:?},"productName":{:?},"stateDir":{:?},"remoteBridge":{:?},"keychain":{{"github":{:?},"secrets":{:?}}}}}"#,
        channel.identifier(),
        channel.product_name(),
        channel.state_dir_name(),
        channel.remote_bridge_name(),
        channel.keychain_service(Keychain::Github),
        channel.keychain_service(Keychain::Secrets),
    )
}

fn main() {
    println!(
        r#"{{"production":{},"development":{}}}"#,
        names(Channel::Production),
        names(Channel::Development),
    );
}
