//! State shared by every handler.

use std::sync::Arc;

use pidash_db::config::Settings;
use pidash_db::Pools;

use crate::edge::{EdgeHandle, DEFAULT_UPSTREAM};

/// Data every handler can read. Extended by later issues (session keys
/// under F-05); handlers take it by extractor, never globals.
///
/// The settings travel with the state so handlers never re-read the
/// environment per request: `main` resolves [`Settings`] once at boot and
/// builds the state from it. [`AppState::new`] uses deterministic
/// [`Settings::test_defaults`] and a test [`EdgeHandle`] for unit tests.
#[derive(Debug, Clone)]
pub struct AppState {
    version: String,
    settings: Arc<Settings>,
    edge: EdgeHandle,
    pools: Option<Arc<Pools>>,
}

impl AppState {
    /// Test/dev constructor: deterministic settings, test edge, no pools.
    pub fn new(version: impl Into<String>) -> Self {
        Self::with_settings_and_edge(
            version,
            Settings::test_defaults(),
            EdgeHandle::for_tests(DEFAULT_UPSTREAM),
        )
    }

    /// Boot constructor: the binary resolves [`Settings`] (OSS or overlay)
    /// and builds the state from it. The edge stays a test handle; `serve`
    /// uses [`AppState::with_settings_and_edge`] for a live edge.
    pub fn with_settings(version: impl Into<String>, settings: Settings) -> Self {
        Self::with_settings_and_edge(version, settings, EdgeHandle::for_tests(DEFAULT_UPSTREAM))
    }

    /// Cutover constructor (F-02): explicit edge handle with deterministic
    /// test settings. All prefix flags come from the handle.
    pub fn with_edge(version: impl Into<String>, edge: EdgeHandle) -> Self {
        Self::with_settings_and_edge(version, Settings::test_defaults(), edge)
    }

    /// Full constructor: explicit settings and edge. `serve` builds the
    /// state this way from `Settings::from_env` plus `EdgeHandle::from_env`.
    pub fn with_settings_and_edge(
        version: impl Into<String>,
        settings: Settings,
        edge: EdgeHandle,
    ) -> Self {
        Self {
            version: version.into(),
            settings: Arc::new(settings),
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

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn pools(&self) -> Option<&Pools> {
        self.pools.as_deref()
    }

    pub fn edge(&self) -> &EdgeHandle {
        &self.edge
    }
}
