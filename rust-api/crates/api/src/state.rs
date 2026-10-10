//! State shared by every handler.

use std::sync::Arc;

use pidash_db::config::Settings;
use pidash_db::redis::RedisHandle;
use pidash_db::Pools;

use crate::edge::{EdgeHandle, DEFAULT_UPSTREAM};
use crate::edge_shadow::{ShadowConfig, ShadowGate};

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
    redis: Option<RedisHandle>,
    shadow_gate: Option<Arc<ShadowGate>>,
    shadow_config: Option<ShadowConfig>,
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
            redis: None,
            shadow_gate: None,
            shadow_config: None,
        }
    }

    /// Attach the shadow gate (PIDASHCONV-822). Installed once by the
    /// router builder, which also builds the gate's shadow app.
    pub fn with_shadow_gate(mut self, gate: Arc<ShadowGate>) -> Self {
        self.shadow_gate = Some(gate);
        self
    }

    /// The installed shadow gate, if the router builder installed one.
    pub fn shadow_gate(&self) -> Option<&Arc<ShadowGate>> {
        self.shadow_gate.as_ref()
    }

    /// Override the shadow config the router builder installs the gate
    /// with (tests enable shadow deterministically this way instead of
    /// through process env). `None` reads [`ShadowConfig::from_env`].
    pub fn with_shadow_config(mut self, config: ShadowConfig) -> Self {
        self.shadow_config = Some(config);
        self
    }

    /// The shadow config override, if any.
    pub fn shadow_config(&self) -> Option<&ShadowConfig> {
        self.shadow_config.as_ref()
    }

    /// Clone this state with a different edge handle. The shadow state
    /// drops the gate and the config override: shadow dispatch never
    /// spawns nested shadow work.
    pub fn with_edge_replaced(&self, edge: EdgeHandle) -> Self {
        Self {
            version: self.version.clone(),
            settings: self.settings.clone(),
            edge,
            pools: self.pools.clone(),
            redis: self.redis.clone(),
            shadow_gate: None,
            shadow_config: None,
        }
    }

    /// Attach the F-04 pools. `None` until the binary connects, so unit
    /// tests that never touch the database keep working unchanged.
    pub fn with_pools(mut self, pools: Pools) -> Self {
        self.pools = Some(Arc::new(pools));
        self
    }

    /// Attach the shared Redis handle (PIDASHCONV-265). `None` until the
    /// binary builds one from `Settings`, so unit tests that never touch
    /// the cache keep working unchanged; handlers treat `None` as
    /// cache-disabled (cancel still 204s, throttle allows, SSE serves the
    /// replay prefix).
    pub fn with_redis(mut self, handle: RedisHandle) -> Self {
        self.redis = Some(handle);
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

    pub fn redis(&self) -> Option<&RedisHandle> {
        self.redis.as_ref()
    }

    pub fn edge(&self) -> &EdgeHandle {
        &self.edge
    }
}
