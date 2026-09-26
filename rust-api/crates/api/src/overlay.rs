#![forbid(unsafe_code)]

//! Named route-group replacement for the private overlay (inventory §4
//! item 3, the Rust form of `pi_dash/urls.py`'s include structure).
//!
//! Django composes URLs from named includes; the cloud URL conf then
//! replaces whole groups (dropping every OSS `api/auth/*` mount so the
//! OIDC flow wins cleanly), adds cloud-only groups, and shadows individual
//! OSS paths by placing cloud routes *before* the OSS include. Merging one
//! flat `extra` router cannot express any of that, so the builder works in
//! named groups instead:
//!
//! - [`RouteGroup`] names the groups after the §0 prefix map (the same
//!   names as [`Prefix`](crate::edge::Prefix)).
//! - [`Overlay`] replaces a group wholesale (`replace`/`drop`), or adds
//!   routes that shadow OSS paths by merge order (`add_routes`).
//! - [`build_router_with_overlay`] assembles foundation groups plus the
//!   overlay. `with_routes`/`build_app` stay as the additive-only path.
//!
//! Groups whose endpoints have not been ported yet (everything but Web)
//! contribute no OSS routes today; replacing one now is how the private
//! crate claims its groups ahead of the domain ports, and dropping the
//! Auth group is the `./cloud` `_strip_oss_auth` equivalent.

use std::collections::HashMap;

use axum::{
    routing::{any, get},
    Json, Router,
};

use crate::edge;
use crate::state::AppState;
use crate::web;

/// Named route groups, one per row family of the §0 prefix map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RouteGroup {
    Web,
    App,
    Assistant,
    Loop,
    Prompting,
    Space,
    License,
    RunnerWeb,
    ApiV1,
    Runner,
    Auth,
}

impl RouteGroup {
    /// All groups, in prefix-table order.
    pub const ALL: [RouteGroup; 11] = [
        RouteGroup::Web,
        RouteGroup::App,
        RouteGroup::Assistant,
        RouteGroup::Loop,
        RouteGroup::Prompting,
        RouteGroup::Space,
        RouteGroup::License,
        RouteGroup::RunnerWeb,
        RouteGroup::ApiV1,
        RouteGroup::Runner,
        RouteGroup::Auth,
    ];

    /// The URL prefix this group owns (no leading slash; `""` is the site
    /// root that also acts as the catch-all). Mirrors `PREFIX_TABLE`.
    pub fn path_prefix(self) -> &'static str {
        match self {
            RouteGroup::Web => "",
            RouteGroup::App | RouteGroup::Assistant | RouteGroup::Loop | RouteGroup::Prompting => {
                "api/"
            }
            RouteGroup::Space => "api/public/",
            RouteGroup::License => "api/instances/",
            RouteGroup::RunnerWeb => "api/runners/",
            RouteGroup::ApiV1 => "api/v1/",
            RouteGroup::Runner => "api/v1/runner/",
            RouteGroup::Auth => "auth/",
        }
    }
}

/// The private crate's contribution: group replacements plus additive
/// routes. Empty by default (pure OSS build).
#[derive(Debug, Default)]
pub struct Overlay {
    replace: HashMap<RouteGroup, Router<AppState>>,
    extra: Vec<Router<AppState>>,
}

impl Overlay {
    pub fn new() -> Self {
        Self::default()
    }

    /// Serve `router` instead of the OSS routes for `group` (the cloud
    /// auth replacement: `replace(RouteGroup::Auth, cloud_auth())`).
    pub fn replace(mut self, group: RouteGroup, router: Router<AppState>) -> Self {
        self.replace.insert(group, router);
        self
    }

    /// Serve nothing for `group` (the `_strip_oss_auth` equivalent: drop
    /// every OSS mount in the group without adding a replacement).
    pub fn drop(mut self, group: RouteGroup) -> Self {
        self.replace.insert(group, Router::new());
        self
    }

    /// Merge `router` additively. Additive routes sit in front of the OSS
    /// groups (see [`build_router_with_overlay`]) so a same-path overlay
    /// route shadows the OSS one — the cloud conf placing its paths ahead
    /// of the OSS include. (Axum `merge` panics on duplicate paths, so
    /// shadowing cannot be a merge; it is fallback dispatch.)
    pub fn add_routes(mut self, router: Router<AppState>) -> Self {
        self.extra.push(router);
        self
    }

    fn replaced(&self, group: RouteGroup) -> Option<Router<AppState>> {
        self.replace.get(&group).cloned()
    }
}

/// OSS routes for one group. Only Web has Rust handlers today (`GET /`
/// and `GET /robots.txt` behind the web flag; unsafe methods still proxy
/// so Django's CSRF-failure page is preserved); every other group's
/// endpoints arrive with their domain ports.
fn oss_group_routes(group: RouteGroup) -> Router<AppState> {
    match group {
        RouteGroup::Web => Router::new()
            .route("/", any(web::health_check))
            .route("/robots.txt", any(web::robots_txt)),
        _ => Router::new(),
    }
}

async fn healthz(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> impl axum::response::IntoResponse {
    Json(pidash_services::health_report(state.version()))
}

/// Assemble the application router from the OSS groups plus the overlay.
///
/// Two layers: the inner router holds `/healthz` plus the groups in
/// prefix-table order (a replacement where the overlay claimed the
/// group), with the Django proxy as its fallback; the outer router holds
/// the additive overlay routes and dispatches everything else to the
/// inner one via fallback. A same-path overlay route therefore shadows
/// the OSS handler — the cloud conf placing its paths ahead of the OSS
/// include — without ever merging duplicate paths (which axum rejects).
pub fn build_router_with_overlay(state: AppState, overlay: Overlay) -> Router {
    let mut oss = Router::new().route("/healthz", get(healthz));
    for group in RouteGroup::ALL {
        oss = oss.merge(match overlay.replaced(group) {
            Some(replacement) => replacement,
            None => oss_group_routes(group),
        });
    }
    let oss = oss.fallback(edge::proxy).with_state(state.clone());
    let mut app: Router<AppState> = Router::new();
    for extra in overlay.extra {
        app = app.merge(extra);
    }
    app.fallback_service(oss).with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use tower::ServiceExt;

    fn test_state() -> AppState {
        AppState::with_edge("0.1.0", edge::EdgeHandle::for_tests("http://127.0.0.1:1"))
    }

    async fn fetch(app: Router, path: &str) -> (StatusCode, serde_json::Value) {
        let response = app
            .oneshot(
                axum::http::Request::get(path)
                    .body(axum::body::Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("body");
        let json: serde_json::Value =
            serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null);
        (status, json)
    }

    #[tokio::test]
    async fn empty_overlay_matches_foundation_routes() {
        let app = build_router_with_overlay(test_state(), Overlay::new());
        let (status, body) = fetch(app, "/healthz").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body,
            serde_json::json!({"status": "ok", "version": "0.1.0"})
        );
    }

    #[tokio::test]
    async fn replace_auth_group_claims_the_group() {
        // The cloud auth replacement in miniature: OSS owns no auth
        // routes yet, the overlay's group serves, everything else is
        // untouched.
        let cloud_auth: Router<AppState> = Router::new().route(
            "/auth/me/",
            get(|| async { Json(serde_json::json!({"cloud": true})) }),
        );
        let overlay = Overlay::new().replace(RouteGroup::Auth, cloud_auth);
        let app = build_router_with_overlay(test_state(), overlay);
        let (status, body) = fetch(app, "/auth/me/").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, serde_json::json!({"cloud": true}));
        let (status, _) = fetch(
            build_router_with_overlay(
                test_state(),
                Overlay::new().replace(
                    RouteGroup::Auth,
                    Router::new().route("/auth/me/", get(|| async { Json(serde_json::json!({})) })),
                ),
            ),
            "/healthz",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn replace_web_group_swaps_oss_handlers() {
        let overlay = Overlay::new().replace(
            RouteGroup::Web,
            Router::new().route("/", get(|| async { "overlay-root" })),
        );
        let app = build_router_with_overlay(test_state(), overlay);
        let response = app
            .oneshot(
                axum::http::Request::get("/")
                    .body(axum::body::Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .expect("body");
        assert_eq!(&body[..], b"overlay-root");
    }

    #[tokio::test]
    async fn drop_group_removes_oss_mounts() {
        // `_strip_oss_auth`: the group serves nothing afterwards, and the
        // request falls through to the proxy (502 fail-closed here, never
        // a Rust 404 masking Django's own).
        let app = build_router_with_overlay(test_state(), Overlay::new().drop(RouteGroup::Web));
        let response = app
            .oneshot(
                axum::http::Request::get("/")
                    .body(axum::body::Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    }

    #[tokio::test]
    async fn additive_route_shadows_oss_path() {
        // Cloud shadowing: a same-path overlay route placed ahead of the
        // OSS include wins over the OSS handler.
        let overlay =
            Overlay::new().add_routes(Router::new().route("/healthz", get(|| async { "shadow" })));
        let app = build_router_with_overlay(test_state(), overlay);
        let response = app
            .oneshot(
                axum::http::Request::get("/healthz")
                    .body(axum::body::Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .expect("body");
        assert_eq!(&body[..], b"shadow");
    }

    #[test]
    fn group_prefixes_match_the_prefix_table() {
        assert_eq!(RouteGroup::Auth.path_prefix(), "auth/");
        assert_eq!(RouteGroup::Runner.path_prefix(), "api/v1/runner/");
        assert_eq!(RouteGroup::Web.path_prefix(), "");
        for group in RouteGroup::ALL {
            assert!(
                crate::edge::PREFIX_TABLE
                    .iter()
                    .any(|(prefix, _, _)| *prefix == group.path_prefix()),
                "{group:?} has no prefix-table row",
            );
        }
    }
}
