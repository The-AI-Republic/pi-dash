//! State shared by every handler.

use std::sync::Arc;

use pidash_db::config::Settings;
use pidash_db::redis::RedisHandle;
use pidash_db::DatabaseConnection;
use pidash_db::Pools;
use pidash_services::extensions::{CloudAgentToolsetsSeam, NoExtraToolsets};
use pidash_services::user_settings::{base_settings_schema, SettingsSchema};

use crate::desktop_gate::CeDesktopRequestGate;
use crate::desktop_gate::DesktopCredential;
use crate::desktop_gate::DesktopRequestGate;
use crate::edge::{EdgeHandle, DEFAULT_UPSTREAM};
use crate::edge_shadow::{ShadowConfig, ShadowGate};

/// The CE desktop-session gate: the `pidash_client == "desktop"`
/// session-key check. Served when the overlay installed no gate.
static CE_DESKTOP_GATE: CeDesktopRequestGate = CeDesktopRequestGate;

/// An installed desktop-session gate (PIDASHCONV-917). The overlay binary
/// installs its OIDC-`client`-claim gate; OSS serves with the CE default
/// (`None` on the state). Opaque under `Debug` — a trait object has no
/// useful rendering; `Clone` shares the gate.
#[derive(Clone)]
pub struct DesktopGateHandle(Arc<dyn DesktopRequestGate>);

impl std::fmt::Debug for DesktopGateHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DesktopGateHandle(..)")
    }
}

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
    orm: Option<DatabaseConnection>,
    redis: Option<RedisHandle>,
    shadow_gate: Option<Arc<ShadowGate>>,
    shadow_config: Option<ShadowConfig>,
    toolsets: Arc<dyn CloudAgentToolsetsSeam>,
    assistant_seam: Arc<dyn pidash_services::extensions::AssistantModelSeam>,
    user_settings_schema: Arc<SettingsSchema>,
    desktop_gate: Option<DesktopGateHandle>,
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
    /// The assistant seam defaults to the CE implementation; the overlay
    /// binary replaces it with [`AppState::with_assistant_seam`].
    pub fn with_settings_and_edge(
        version: impl Into<String>,
        settings: Settings,
        edge: EdgeHandle,
    ) -> Self {
        let settings = Arc::new(settings);
        let assistant_seam = Arc::new(crate::assistant::model_seam::CeAssistantSeam::new(
            settings.clone(),
        ));
        Self {
            version: version.into(),
            settings,
            edge,
            pools: None,
            orm: None,
            redis: None,
            shadow_gate: None,
            shadow_config: None,
            toolsets: Arc::new(NoExtraToolsets),
            assistant_seam,
            user_settings_schema: Arc::new(base_settings_schema()),
            desktop_gate: None,
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
    /// spawns nested shadow work. The desktop gate is request-independent
    /// config, so unlike the shadow gate it travels with the clone: the
    /// shadow side must answer the same desktop verdict.
    pub fn with_edge_replaced(&self, edge: EdgeHandle) -> Self {
        Self {
            version: self.version.clone(),
            settings: self.settings.clone(),
            edge,
            pools: self.pools.clone(),
            orm: self.orm.clone(),
            redis: self.redis.clone(),
            shadow_gate: None,
            shadow_config: None,
            toolsets: self.toolsets.clone(),
            assistant_seam: self.assistant_seam.clone(),
            user_settings_schema: self.user_settings_schema.clone(),
            desktop_gate: self.desktop_gate.clone(),
        }
    }

    /// Replace the assistant model-provider seam (PIDASHCONV-914). The
    /// overlay binary installs its own implementation here; tests install
    /// fakes to prove the callers consult the seam.
    pub fn with_assistant_seam(
        mut self,
        seam: Arc<dyn pidash_services::extensions::AssistantModelSeam>,
    ) -> Self {
        self.assistant_seam = seam;
        self
    }

    /// The assistant model-provider seam every non-turn caller consults.
    pub fn assistant_seam(&self) -> &Arc<dyn pidash_services::extensions::AssistantModelSeam> {
        &self.assistant_seam
    }

    /// Install the overlay's desktop-session gate (PIDASHCONV-917).
    /// Installed once by the overlay binary, whose gate answers from the
    /// verified client claim; OSS serves with the CE default (`None`).
    pub fn with_desktop_gate(mut self, gate: impl DesktopRequestGate + 'static) -> Self {
        self.desktop_gate = Some(DesktopGateHandle(Arc::new(gate)));
        self
    }

    /// The installed desktop-session gate, or the CE session-key gate
    /// when the overlay installed none. Both desktop-only endpoint
    /// families consult this (never the CE check directly), so the
    /// private overlay substitutes its OIDC-claim gate without touching
    /// the callers.
    pub fn desktop_gate(&self) -> &dyn DesktopRequestGate {
        match &self.desktop_gate {
            Some(handle) => &*handle.0,
            None => &CE_DESKTOP_GATE,
        }
    }

    /// The desktop verdict for one request, through the admission closure
    /// form (`IsDesktopSession.has_permission`: the authentication fold,
    /// then the EE seam verdict). `authenticated` is hard `true`:
    /// authentication is decided by the actor resolution that follows in
    /// each caller (DRF checks authentication before permissions, so anon
    /// still 401s), keeping this predicate purely the desktop check.
    pub fn is_desktop_request(&self, credential: &DesktopCredential) -> bool {
        let gate = self.desktop_gate();
        pidash_services::dispatch::is_desktop_session(true, || gate.is_desktop(credential))
    }

    /// Attach the F-04 pools. `None` until the binary connects, so unit
    /// tests that never touch the database keep working unchanged.
    pub fn with_pools(mut self, pools: Pools) -> Self {
        self.pools = Some(Arc::new(pools));
        self
    }

    /// Attach the shared SeaORM connection (ORM-01, design D5), built
    /// with [`pidash_db::sea_orm_connection`] over the primary pool.
    /// `None` until the binary connects, like [`AppState::with_pools`]:
    /// the handle wraps the pool (no second pool), and `Clone` is cheap
    /// (Arc), so the shadow clone shares it.
    pub fn with_orm(mut self, orm: DatabaseConnection) -> Self {
        self.orm = Some(orm);
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

    /// The shared SeaORM connection, if the binary attached one with
    /// [`AppState::with_orm`]. Domain services (stage 8) run their writes
    /// on this handle's transactions; reads stay on the sqlx pools.
    pub fn orm(&self) -> Option<&DatabaseConnection> {
        self.orm.as_ref()
    }

    pub fn redis(&self) -> Option<&RedisHandle> {
        self.redis.as_ref()
    }

    pub fn edge(&self) -> &EdgeHandle {
        &self.edge
    }

    /// Install the cloud-agent extra-toolsets seam (PIDASHCONV-915). The
    /// cloud binary installs its gateway seam here; creation call sites
    /// consult this instance. CE default: [`NoExtraToolsets`].
    pub fn with_toolsets(mut self, toolsets: Arc<dyn CloudAgentToolsetsSeam>) -> Self {
        self.toolsets = toolsets;
        self
    }

    /// The installed toolsets seam.
    pub fn toolsets(&self) -> &Arc<dyn CloudAgentToolsetsSeam> {
        &self.toolsets
    }

    /// Install the user-settings schema the profile endpoint validates
    /// `settings` PATCHes against (PIDASHCONV-916). CE keeps the empty
    /// default, so every `settings` write is rejected; the overlay
    /// installs `extend_settings_schema(&base_settings_schema(), &own)`
    /// instead — declaring a key is what makes it writable through the
    /// profile API.
    pub fn with_user_settings_schema(mut self, schema: SettingsSchema) -> Self {
        self.user_settings_schema = Arc::new(schema);
        self
    }

    /// The schema the profile endpoint validates `settings` PATCHes
    /// against: the CE default until the overlay installs its own.
    pub fn user_settings_schema(&self) -> &SettingsSchema {
        &self.user_settings_schema
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use pidash_services::user_settings::extend_settings_schema;

    #[test]
    fn toolsets_seam_defaults_to_ce_and_installs() {
        #[derive(Debug)]
        struct AcmeToolsets;

        impl CloudAgentToolsetsSeam for AcmeToolsets {
            fn schema_tool_name(&self) -> &'static str {
                "acme_get_tool_schema"
            }
        }

        let state = AppState::new("0.1.0");
        assert_eq!(state.toolsets().schema_tool_name(), "");
        let state = state.with_toolsets(Arc::new(AcmeToolsets));
        assert_eq!(state.toolsets().schema_tool_name(), "acme_get_tool_schema");
        // The shadow clone keeps the installed seam.
        let shadow = state.with_edge_replaced(EdgeHandle::for_tests(DEFAULT_UPSTREAM));
        assert_eq!(shadow.toolsets().schema_tool_name(), "acme_get_tool_schema");
    }

    #[test]
    fn settings_schema_defaults_to_empty_ce_schema() {
        assert!(AppState::new("test").user_settings_schema().is_empty());
    }

    #[test]
    fn settings_schema_is_injectable() {
        let schema = extend_settings_schema(
            &base_settings_schema(),
            &BTreeMap::from([(
                "openhub".to_string(),
                BTreeMap::from([("apps_enabled".to_string(), serde_json::json!(false))]),
            )]),
        )
        .expect("extend");
        let state = AppState::new("test").with_user_settings_schema(schema);
        assert_eq!(
            state.user_settings_schema()["openhub"]["apps_enabled"],
            serde_json::json!(false)
        );
    }

    /// An overlay-shaped gate: answers from the presented credential
    /// (like the cloud client-claim check) instead of the CE session
    /// dict.
    struct TokenDesktopGate;

    impl DesktopRequestGate for TokenDesktopGate {
        fn is_desktop(&self, credential: &DesktopCredential) -> bool {
            matches!(
                credential.authorization,
                Some(header) if header == "Bearer desktop-token"
            )
        }
    }

    fn credential<'a>(
        session: Option<&'a serde_json::Value>,
        authorization: Option<&'a str>,
    ) -> DesktopCredential<'a> {
        DesktopCredential {
            session,
            authorization,
            cookie: None,
        }
    }

    #[test]
    fn desktop_gate_defaults_to_the_ce_check() {
        let state = AppState::new("test");
        let desktop = serde_json::json!({"pidash_client": "desktop"});
        assert!(state.is_desktop_request(&credential(Some(&desktop), None)));
        let browser = serde_json::json!({"pidash_client": "browser"});
        assert!(!state.is_desktop_request(&credential(Some(&browser), None)));
        assert!(!state.is_desktop_request(&credential(None, None)));
    }

    #[test]
    fn installed_gate_replaces_the_default() {
        let state = AppState::new("test").with_desktop_gate(TokenDesktopGate);
        assert!(state.is_desktop_request(&credential(None, Some("Bearer desktop-token"))));
        let desktop = serde_json::json!({"pidash_client": "desktop"});
        assert!(!state.is_desktop_request(&credential(Some(&desktop), None)));
    }

    #[test]
    fn edge_replacement_keeps_the_gate() {
        let state = AppState::new("test").with_desktop_gate(TokenDesktopGate);
        let replaced = state.with_edge_replaced(EdgeHandle::for_tests("http://127.0.0.1:1"));
        assert!(replaced.is_desktop_request(&credential(None, Some("Bearer desktop-token"))));
        let desktop = serde_json::json!({"pidash_client": "desktop"});
        assert!(!replaced.is_desktop_request(&credential(Some(&desktop), None)));
    }
}
