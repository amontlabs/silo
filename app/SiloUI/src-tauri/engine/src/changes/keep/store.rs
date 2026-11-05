//! Where the engine gets the manifests a keep compares: the collector's store for the
//! fork-time base, and a fresh capture for each computer it looks at.

use std::sync::Arc;

use base64::Engine as _;

use super::env::ManifestStore;
use crate::{
    changes::{
        collect::{self, Options, TargetRunner},
        store::{Slot, Store},
    },
    guest,
    runtime::{self, RuntimeHost},
};

pub struct StoredManifests {
    host: Arc<dyn RuntimeHost>,
    store: Store,
}

impl StoredManifests {
    pub fn new(host: Arc<dyn RuntimeHost>) -> Self {
        let store = Store::for_engine(host.engine_context().as_ref());
        Self { host, store }
    }

    fn base(
        &self,
        computer: &str,
    ) -> Result<Option<(String, crate::changes::store::Meta)>, String> {
        self.store.get(computer, Slot::Base)
    }
}

impl ManifestStore for StoredManifests {
    fn fork_time(&self, fork: &str) -> Result<String, String> {
        self.base(fork)?.map(|(text, _)| text).ok_or_else(|| {
            format!("{fork} has no fork-time record, so there is nothing to compare a keep with.")
        })
    }

    fn capture(&self, computer: &str) -> Result<String, String> {
        let context = self.host.engine_context();
        let target = guest::resolve(
            runtime::runtime_paths(self.host.as_ref()),
            &context.paths().app_data,
            computer,
        )
        .map_err(|error| error.message)?;
        target.ensure_running().map_err(|error| error.message)?;
        let runner = TargetRunner(target);
        let text = collect::capture(&runner, &Options::default()).map_err(|error| error.message)?;
        // A fork's package files are attributed to their packages, as `silo diff` does.
        Ok(match self.base(computer)? {
            Some((base, _)) => collect::with_owners(&runner, &base, text),
            None => text,
        })
    }

    fn baseline(&self, fork: &str) -> Option<(String, String)> {
        let (_, meta) = self.base(fork).ok().flatten()?;
        self.store
            .get_baseline(meta.source.as_deref()?)
            .ok()
            .flatten()
    }

    fn fork_time_crontab(&self, fork: &str, user: &str) -> Result<Option<String>, String> {
        let Some((text, _)) = self.base(fork)? else {
            return Ok(None);
        };
        for line in text.lines() {
            let mut fields = line.split('\t');
            if fields.next() == Some("CRONTXT") && fields.next() == Some(user) {
                let encoded = fields.next().unwrap_or("");
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(encoded)
                    .map_err(|e| format!("The recorded crontab is unreadable: {e}"))?;
                return Ok(Some(String::from_utf8_lossy(&bytes).into_owned()));
            }
        }
        Ok(None)
    }

    fn source(&self, fork: &str) -> Result<Option<String>, String> {
        Ok(self.base(fork)?.and_then(|(_, meta)| meta.source))
    }
}

pub fn production(host: Arc<dyn RuntimeHost>) -> Arc<dyn ManifestStore> {
    Arc::new(StoredManifests::new(host))
}
