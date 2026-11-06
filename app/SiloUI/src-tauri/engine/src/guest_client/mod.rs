//! The in-guest client's host side: tokens, the scope a token may use and the bridge folder
//! the guest writes requests into. `silod` runs the loop that serves them
//! (`docs/SiloUI-IN-GUEST-CLIENT.md`).

pub mod scope;
pub mod spool;
pub mod tokens;

pub use scope::{authorize, filter_result, Authorized, Filter, Refusal, Topology, ALLOWED};
pub use spool::{spool_dir, Spool, Taken};
pub use tokens::{Minted, Record, TokenStore};
