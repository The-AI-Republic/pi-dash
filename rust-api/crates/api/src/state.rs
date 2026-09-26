//! State shared by every handler.

use std::sync::Arc;

use pidash_db::Pools;

use crate::edge::{EdgeHandle, DEFAULT_UPSTREAM};

/// Data every handler can read. Extended by later issues (session keys
/// under F-05); handlers take it by extractor, never globals.
#[derive(Debug, Clone)]
pub struct AppState {
    version: String,
    edge: EdgeHandle,
    pools: Option<Arc<Pools>>,
}

impl AppState {
    /// All prefix flags off, proxying to [`DEFAULT_UPSTREAM`].
    pub fn new(version: impl Into<String>) -> Self {
        Self::with_edge(version, EdgeHandle::for_tests(DEFAULT_UPSTREAM))
    }

    pub fn with_edge(version: impl Into<String>, edge: EdgeHandle) -> Self {
        Self {
            version: version.into(),
            edge,
            pools: None,
        }
    }

    /// Attach the F-04 pools. `None` until the binary connects, so unit
    /// tests that never touch the database keep working unchanged.
    pub fn with_pools(mut self, pools: Pools) -> Self {
        self.pools = Some(Arc::new(pools));
        self
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn pools(&self) -> Option<&Pools> {
        self.pools.as_deref()
    }

    pub fn edge(&self) -> &EdgeHandle {
        &self.edge
    }
}
