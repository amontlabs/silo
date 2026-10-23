//! Stateless host helpers shared by the Silo app and the engine.

pub mod applications;
pub mod channel;
pub mod installer;
pub mod notifications;
pub mod system_integrations;
pub mod system_shutdown;
pub mod terminal;

#[cfg(test)]
mod test_support;
