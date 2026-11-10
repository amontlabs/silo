//! The in-guest client's host side: tokens and the bridge folder the guest writes requests
//! into. `silod` runs the loop that serves them (`docs/SiloUI-IN-GUEST-CLIENT.md`); what each
//! call may do is the policy's (`crate::policy`).

pub mod hold;
pub mod spool;
pub mod tokens;

pub use hold::{bridge_held, hold_bridge, BridgeHold};
pub use spool::{spool_dir, Spool, Taken};
pub use tokens::{now_secs, Minted, OwnedJob, PendingFork, Record, TokenStore, MAX_JOBS};
