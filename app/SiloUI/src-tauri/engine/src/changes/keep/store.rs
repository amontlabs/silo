//! Where the engine gets the manifests a keep compares. `changes::collect` records them per
//! computer; this is the one place that reads them for a keep.

use std::sync::Arc;

use super::env::ManifestStore;
use crate::{changes::manifest::Manifest, context::EngineContext};

/// Answers every read with the reason there are no manifests. Replaced by the collector's
/// store when `changes::collect` lands in the tree.
struct Unavailable;

const REASON: &str =
    "Change capture is not available in this build, so there is nothing to compare a keep with.";

impl ManifestStore for Unavailable {
    fn fork_time(&self, _: &str) -> Result<Manifest, String> {
        Err(REASON.into())
    }
    fn capture(&self, _: &str) -> Result<Manifest, String> {
        Err(REASON.into())
    }
    fn source(&self, _: &str) -> Result<Option<String>, String> {
        Err(REASON.into())
    }
}

pub fn production(_context: &dyn EngineContext) -> Arc<dyn ManifestStore> {
    Arc::new(Unavailable)
}
