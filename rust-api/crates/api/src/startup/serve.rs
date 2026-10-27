//! The OSS HTTP server as a composable builder (PIDASHCONV-1541, OVL-03).
//!
//! [`ServeBuilder::from_env`] owns the `serve` startup sequence the OSS
//! `main.rs` used to spell out: settings resolution, the cutover edge,
//! pools, the SeaORM handle, the queue schema, Redis, [`AppState`],
//! [`build_app`](crate::build_app), bind, `axum::serve`, and graceful
//! shutdown — in that order, with the same fail-fast behaviour. The OSS
//! binary calls `from_env` plus [`ServeBuilder::run`]; an extension binary
//! composes the same path with its own settings, seams, routes and layers:
//!
//! ```rust
//! let parts = pidash_api::startup::ServeBuilder::for_tests()
//!     .merge_routes(axum::Router::new().route(
//!         "/api/overlay-ping",
//!         axum::routing::get(|| async { "pong" }),
//!     ))
//!     .build();
//! assert_eq!(parts.state.version(), env!("CARGO_PKG_VERSION"));
//! let _router: axum::Router = parts.router;
//! ```

use std::convert::Infallible;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use axum::body::Body;
use axum::http::Request;
use axum::response::IntoResponse;
use axum::routing::Route;
use tower::{Layer, Service};

use pidash_db::config::Settings;
use pidash_services::extensions::{AssistantModelSeam, CloudAgentToolsetsSeam};
use pidash_services::user_settings::SettingsSchema;

use super::{connect_serve_pools, settings_from_env, shutdown_signal};
use crate::desktop_gate::DesktopRequestGate;
use crate::edge::{EdgeHandle, DEFAULT_UPSTREAM};
use crate::state::AppState;

/// The composed server without the socket: the state the router was
/// built over plus the router. [`ServeBuilder::build`] returns this for
/// tests and embedding; [`ServeBuilder::run`] builds the same pair
/// (with pools attached) and serves it.
///
/// The state is the pre-router state: [`build_app`](crate::build_app)
/// installs the shadow gate on its own copy, like every other caller.
pub struct ServeParts {
    pub state: AppState,
    pub router: axum::Router,
}

/// One deferred state tweak, applied in call order when the state is
/// built. Seams defer (rather than mutating an eager state) so
/// [`ServeBuilder::with_settings`] never orphans an installed seam: the
/// state is built once, from the final settings.
type StateMod = Box<dyn FnOnce(AppState) -> AppState + Send>;

/// One deferred router wrap, applied in call order outside the OSS
/// middleware stack.
type RouteWrap = Box<dyn FnOnce(axum::Router) -> axum::Router + Send>;

/// Owns the `serve` startup sequence; extensions plug in through the
/// `with_*` seams, [`ServeBuilder::merge_routes`] and [`ServeBuilder::wrap`].
///
/// The OSS binary serves with `from_env` plus [`ServeBuilder::run`] (and
/// the CLI bind address); every other method is an extension point the
/// cloud overlay uses instead of re-doing the sequence by hand.
pub struct ServeBuilder {
    version: String,
    settings: Settings,
    edge: EdgeHandle,
    bind: SocketAddr,
    state_mods: Vec<StateMod>,
    extra: axum::Router<AppState>,
    wraps: Vec<RouteWrap>,
}

impl ServeBuilder {
    /// Resolve settings plus the cutover edge from the environment. No
    /// I/O: pools connect in [`ServeBuilder::run`].
    pub fn from_env() -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            settings: settings_from_env()?,
            edge: EdgeHandle::from_env()?,
            bind: SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 8080),
            state_mods: Vec::new(),
            extra: axum::Router::new(),
            wraps: Vec::new(),
        })
    }

    /// Deterministic pool-less builder for tests and embedding: test
    /// settings, a test edge, no pools. [`ServeBuilder::build`] composes
    /// from this; [`ServeBuilder::run`] still connects real pools.
    pub fn for_tests() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            settings: Settings::test_defaults(),
            edge: EdgeHandle::for_tests(DEFAULT_UPSTREAM),
            bind: SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 8080),
            state_mods: Vec::new(),
            extra: axum::Router::new(),
            wraps: Vec::new(),
        }
    }

    /// Report `version` from the state (the `/healthz` body). Defaults to
    /// this crate's version; the OSS binary installs its own here (as it
    /// always reported), and any embedding binary does the same.
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = version.into();
        self
    }

    /// Tweak the resolved settings before the state is built. Runs before
    /// every seam install, so the CE assistant seam (built from these
    /// settings) and every handler read the final values.
    pub fn with_settings(mut self, f: impl FnOnce(&mut Settings)) -> Self {
        f(&mut self.settings);
        self
    }

    /// Listen on `bind` instead of `0.0.0.0:8080`. The OSS binary passes
    /// its parsed `--bind` here (parsing stays with the CLI, so a bad
    /// address fails exactly as before).
    pub fn with_bind(mut self, bind: SocketAddr) -> Self {
        self.bind = bind;
        self
    }

    /// Install the cloud-agent extra-toolsets seam (PIDASHCONV-915). CE
    /// default: [`NoExtraToolsets`](pidash_services::extensions::NoExtraToolsets).
    pub fn with_toolsets(mut self, toolsets: Arc<dyn CloudAgentToolsetsSeam>) -> Self {
        self.state_mods
            .push(Box::new(move |state| state.with_toolsets(toolsets)));
        self
    }

    /// Install the overlay's desktop-session gate (PIDASHCONV-917). OSS
    /// serves with the CE session-key gate when none is installed.
    pub fn with_desktop_gate(mut self, gate: impl DesktopRequestGate + 'static) -> Self {
        self.state_mods
            .push(Box::new(move |state| state.with_desktop_gate(gate)));
        self
    }

    /// Replace the assistant model-provider seam (PIDASHCONV-914). CE
    /// default: [`CeAssistantSeam`](crate::assistant::model_seam::CeAssistantSeam).
    pub fn with_assistant_seam(mut self, seam: Arc<dyn AssistantModelSeam>) -> Self {
        self.state_mods
            .push(Box::new(move |state| state.with_assistant_seam(seam)));
        self
    }

    /// Install the user-settings schema the profile endpoint validates
    /// `settings` PATCHes against (PIDASHCONV-916). CE keeps the empty
    /// default, so every `settings` write is rejected.
    pub fn with_user_settings_schema(mut self, schema: SettingsSchema) -> Self {
        self.state_mods.push(Box::new(move |state| {
            state.with_user_settings_schema(schema)
        }));
        self
    }

    /// Tweak the built state (any [`AppState`] setter the typed seams do
    /// not cover). Runs after the typed seam installs, in call order with
    /// the other state mods.
    pub fn with_state(mut self, f: impl FnOnce(AppState) -> AppState + Send + 'static) -> Self {
        self.state_mods.push(Box::new(f));
        self
    }

    /// Merge additive routes. They ride the overlay `add_routes` path
    /// inside [`build_app`](crate::build_app) (fallback dispatch ahead of
    /// the OSS groups), so an extra route never fights an OSS route — but
    /// two `merge_routes` calls with the same path *and* method panic,
    /// like every axum `merge`.
    pub fn merge_routes(mut self, router: axum::Router<AppState>) -> Self {
        self.extra = self.extra.merge(router);
        self
    }

    /// Wrap the built router with `layer`, outside the OSS middleware
    /// stack. The bounds mirror `axum::Router::layer` exactly, so every
    /// layer the router accepts the builder accepts.
    pub fn wrap<L>(mut self, layer: L) -> Self
    where
        L: Layer<Route> + Clone + Send + Sync + 'static,
        L::Service: Service<Request<Body>> + Clone + Send + Sync + 'static,
        <L::Service as Service<Request<Body>>>::Response: IntoResponse + 'static,
        <L::Service as Service<Request<Body>>>::Error: Into<Infallible> + 'static,
        <L::Service as Service<Request<Body>>>::Future: Send + 'static,
    {
        self.wraps.push(Box::new(move |router| router.layer(layer)));
        self
    }

    /// Compose the state and the router without binding or connecting:
    /// the pool-less [`ServeParts`] for tests and embedding.
    pub fn build(self) -> ServeParts {
        let Self {
            version,
            settings,
            edge,
            bind: _,
            state_mods,
            extra,
            wraps,
        } = self;
        let state = Self::assemble_state(version, settings, edge, None, state_mods);
        let router = Self::assemble_router(state.clone(), extra, wraps);
        ServeParts { state, router }
    }

    /// Connect pools, ensure the queue schema, build the state and the
    /// router, bind, and serve with graceful shutdown — the `serve`
    /// sequence, in `serve` order.
    pub async fn run(self) -> Result<(), Box<dyn std::error::Error>> {
        let Self {
            version,
            settings,
            edge,
            bind: addr,
            state_mods,
            extra,
            wraps,
        } = self;
        let pools = connect_serve_pools().await?;
        // Enqueue paths (magic-link mail, password mail, …) INSERT into the
        // Rust-owned queue tables; without them every such handler 500s.
        // `worker` runs the same DDL at its own boot, but `serve` also runs
        // workerless (the V-02 matrix starts no worker for the Rust run), so
        // `serve` must ensure the schema too (PIDASHCONV-854). Fail-fast like
        // `worker`: refusing to start beats per-request 500s.
        pidash_jobs::queue::ensure_schema(pools.primary()).await?;
        tracing::info!(
            %addr,
            upstream = edge.upstream(),
            flags = ?edge.flags(),
            "serving HTTP"
        );
        // Shared Redis client (PIDASHCONV-265): cancel signals, the throttle
        // cache, and the SSE live tail multiplex over it. Absent without a
        // `REDIS_URL` (handlers degrade per-site: swallow / allow / replay-only).
        let redis = pidash_db::redis::RedisHandle::from_settings(&settings);
        if redis.is_some() {
            tracing::info!("redis handle ready (client connects lazily per operation)");
        }
        // The shared SeaORM connection wraps the primary pool (ORM-01,
        // design D5): one pool, two handles.
        let orm = pidash_db::sea_orm_connection(pools.primary());
        let state = Self::assemble_state(
            version,
            settings,
            edge,
            Some((pools, orm, redis)),
            state_mods,
        );
        let app = Self::assemble_router(state, extra, wraps);
        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(shutdown_signal())
        .await?;
        Ok(())
    }

    /// The state every path serves from: `with_settings_and_edge` plus the
    /// pools when connected, then every state mod in call order.
    fn assemble_state(
        version: String,
        settings: Settings,
        edge: EdgeHandle,
        pools: Option<(
            pidash_db::Pools,
            pidash_db::DatabaseConnection,
            Option<pidash_db::redis::RedisHandle>,
        )>,
        state_mods: Vec<StateMod>,
    ) -> AppState {
        let mut state = AppState::with_settings_and_edge(version, settings, edge);
        if let Some((pools, orm, redis)) = pools {
            state = state.with_pools(pools).with_orm(orm);
            if let Some(handle) = redis {
                state = state.with_redis(handle);
            }
        }
        for apply in state_mods {
            state = apply(state);
        }
        state
    }

    /// The router every path serves: [`build_app`](crate::build_app) over
    /// the state plus the merged extras, then every wrap in call order.
    fn assemble_router(
        state: AppState,
        extra: axum::Router<AppState>,
        wraps: Vec<RouteWrap>,
    ) -> axum::Router {
        let mut router = crate::build_app(state, Some(extra));
        for wrap in wraps {
            router = wrap(router);
        }
        router
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower::ServiceExt;

    /// Toolsets double with a marker schema-tool name.
    #[derive(Debug)]
    struct MarkerToolsets;

    impl CloudAgentToolsetsSeam for MarkerToolsets {
        fn schema_tool_name(&self) -> &'static str {
            "acme_get_tool_schema"
        }
    }

    /// Assistant-seam double with a marker title.
    #[derive(Debug)]
    struct MarkerAssistantSeam;

    impl AssistantModelSeam for MarkerAssistantSeam {
        fn generate_title(
            &self,
            request: pidash_services::extensions::TitleRequest,
        ) -> pidash_services::extensions::SeamFuture<
            '_,
            Result<String, pidash_services::extensions::TitleError>,
        > {
            let _ = request;
            Box::pin(async move { Ok("marker-title".to_owned()) })
        }
    }

    /// Desktop-gate double with a fixed verdict.
    struct FixedGate(bool);

    impl DesktopRequestGate for FixedGate {
        fn is_desktop(&self, credential: &crate::desktop_gate::DesktopCredential) -> bool {
            let _ = credential;
            self.0
        }
    }

    fn empty_credential() -> crate::desktop_gate::DesktopCredential<'static> {
        crate::desktop_gate::DesktopCredential {
            session: None,
            authorization: None,
            cookie: None,
        }
    }

    async fn get_status(router: axum::Router, path: &str) -> axum::http::StatusCode {
        router
            .oneshot(
                axum::http::Request::get(path)
                    .body(axum::body::Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve")
            .status()
    }

    /// The builder serves OSS routes plus one extra route, pool-less.
    #[tokio::test]
    async fn builder_serves_oss_and_extra_routes() {
        let extra: axum::Router<AppState> =
            axum::Router::new().route("/api/ping", axum::routing::get(|| async { "pong" }));
        let parts = ServeBuilder::for_tests().merge_routes(extra).build();
        assert_eq!(
            get_status(parts.router.clone(), "/healthz").await,
            axum::http::StatusCode::OK
        );
        assert_eq!(
            get_status(parts.router, "/api/ping").await,
            axum::http::StatusCode::OK
        );
    }

    /// Two `merge_routes` calls both land.
    #[tokio::test]
    async fn builder_merges_repeated_extra_routes() {
        let parts = ServeBuilder::for_tests()
            .merge_routes(
                axum::Router::new().route("/api/one", axum::routing::get(|| async { "one" })),
            )
            .merge_routes(
                axum::Router::new().route("/api/two", axum::routing::get(|| async { "two" })),
            )
            .build();
        assert_eq!(
            get_status(parts.router.clone(), "/api/one").await,
            axum::http::StatusCode::OK
        );
        assert_eq!(
            get_status(parts.router, "/api/two").await,
            axum::http::StatusCode::OK
        );
    }

    /// Swapping a seam is observable on the built state: toolsets,
    /// assistant, user-settings schema and desktop gate.
    #[tokio::test]
    async fn seam_swaps_are_observable_on_built_state() {
        let parts = ServeBuilder::for_tests().build();
        assert_eq!(parts.state.toolsets().schema_tool_name(), "");
        assert!(parts.state.user_settings_schema().is_empty());
        assert!(!parts.state.is_desktop_request(&empty_credential()));

        let schema = pidash_services::user_settings::extend_settings_schema(
            &pidash_services::user_settings::base_settings_schema(),
            &std::collections::BTreeMap::from([(
                "openhub".to_string(),
                std::collections::BTreeMap::from([(
                    "apps_enabled".to_string(),
                    serde_json::json!(false),
                )]),
            )]),
        )
        .expect("extend");
        let parts = ServeBuilder::for_tests()
            .with_toolsets(Arc::new(MarkerToolsets))
            .with_assistant_seam(Arc::new(MarkerAssistantSeam))
            .with_user_settings_schema(schema)
            .with_desktop_gate(FixedGate(true))
            .build();
        assert_eq!(
            parts.state.toolsets().schema_tool_name(),
            "acme_get_tool_schema"
        );
        let title = parts
            .state
            .assistant_seam()
            .generate_title(pidash_services::extensions::TitleRequest {
                user_id: uuid::Uuid::nil(),
                description: "Fix the thing".to_owned(),
                byok: None,
            })
            .await
            .expect("marker title");
        assert_eq!(title, "marker-title");
        assert!(!parts.state.user_settings_schema().is_empty());
        assert!(parts.state.is_desktop_request(&empty_credential()));
    }

    /// `with_settings` lands before the state (and its CE seam) is built.
    #[test]
    fn with_settings_mutates_before_state_build() {
        let parts = ServeBuilder::for_tests()
            .with_settings(|settings| settings.debug = true)
            .build();
        assert!(parts.state.settings().debug);
    }

    /// `with_state` runs after the typed seam installs.
    #[test]
    fn with_state_runs_last() {
        let parts = ServeBuilder::for_tests()
            .with_toolsets(Arc::new(MarkerToolsets))
            .with_state(|state| {
                assert_eq!(state.toolsets().schema_tool_name(), "acme_get_tool_schema");
                state
            })
            .build();
        assert_eq!(
            parts.state.toolsets().schema_tool_name(),
            "acme_get_tool_schema"
        );
    }

    /// `wrap` layers run outside the stack: a header middleware marks the
    /// response on OSS and extra routes alike.
    #[tokio::test]
    async fn wrap_marks_oss_and_extra_responses() {
        async fn mark(
            request: axum::http::Request<axum::body::Body>,
            next: axum::middleware::Next,
        ) -> axum::response::Response {
            let mut response = next.run(request).await;
            response.headers_mut().insert(
                "x-overlay-marker",
                axum::http::HeaderValue::from_static("wrapped"),
            );
            response
        }

        let parts = ServeBuilder::for_tests()
            .merge_routes(
                axum::Router::new().route("/api/ping", axum::routing::get(|| async { "pong" })),
            )
            .wrap(axum::middleware::from_fn(mark))
            .build();
        for path in ["/healthz", "/api/ping"] {
            let response = parts
                .router
                .clone()
                .oneshot(
                    axum::http::Request::get(path)
                        .body(axum::body::Body::empty())
                        .expect("request"),
                )
                .await
                .expect("serve");
            assert_eq!(response.status(), axum::http::StatusCode::OK, "{path}");
            assert_eq!(
                response.headers().get("x-overlay-marker").expect("marked"),
                "wrapped",
                "{path}"
            );
        }
    }

    /// The version defaults to this crate's (the OSS binary's) and an
    /// embedding binary overrides it.
    #[test]
    fn version_defaults_and_overrides() {
        assert_eq!(
            ServeBuilder::for_tests().build().state.version(),
            env!("CARGO_PKG_VERSION")
        );
        assert_eq!(
            ServeBuilder::for_tests()
                .with_version("cloud-1")
                .build()
                .state
                .version(),
            "cloud-1"
        );
    }
}
