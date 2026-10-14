//! A thin client for `engine.sock`: framing, the handshake, request correlation and the
//! session event feed. It knows nothing about engine domains and does not depend on
//! `silo-engine`; the wire types come from `silo-api`.

pub mod channel;
mod codec;
mod session;

pub use codec::{read_frame, write_frame, MAX_FRAME_BYTES};
pub use session::{establish, Client, ClientError, ConnectOptions, EventFeed, Handshake, RequestOptions};
