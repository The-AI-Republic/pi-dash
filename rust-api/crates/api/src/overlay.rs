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
#[derive(Debug, Clone, Default)]
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
/// so Django's CSRF-failure page is preserved) plus Auth (Gitea OAuth,
/// PIDASHCONV-341, and Google OAuth app + space initiate/callback,
/// PIDASHCONV-335); every other group's endpoints arrive with their
/// domain ports.
///
/// The App group serves the issue-list family (`app_issues`, pilot 2 of
/// D-26): exactly the four list GETs — plus the D-33 app-integrations
/// family (`app_integrations`, PIDASHCONV-443/446/450/452/454): the five
/// GitHub App install-flow routes (443), the workspace GitHub shell
/// (446), the project-github bind + status routes (450), the two git
/// project-repository paths (452), and the two AI-assistant POSTs plus
/// the Unsplash GET (454); sibling D-33 handler issues extend that
/// merge, keeping both sides — plus the D-31 legacy v1 asset routes
/// (`app_assets`, PIDASHCONV-394; sibling handler issues 400/412 extend
/// that merge, keeping both sides) — plus the D-23 OpenAPI schema trio
/// (`v1_openapi`, PIDASHCONV-535), served only while a tied `api/` flip
/// is on. Registration is the cutover
/// granularity — sibling paths have no Rust route and keep proxying to
/// Django through the fallback, so no per-path flag is needed.
///
/// The License group serves the D-01 instance console (`license`):
/// `GET`/`PATCH /api/instances/` and
/// `POST /api/instances/admins/sign-up-screen-visited/` (PIDASHCONV-120),
/// plus the configuration + workspace family
/// (`license::config_workspace_routes`, PIDASHCONV-123): the five owned
/// paths with their owned methods; every other method on those paths
/// proxies to Django.
///
/// The Space group serves the D-02 project/meta/taxonomy family (`space`):
/// the nine owned GETs (PIDASHCONV-174). Sibling `api/public/` paths have
/// no Rust route and keep proxying to Django through the fallback.
///
/// The Loop group serves the D-03 instance-admin surface (`loop::admin`,
/// PIDASHCONV-161): the `/api/instances/loop/jobs/` paths with their
/// owned methods — plus the D-03 user auto-pm surface (`loop::user`,
/// PIDASHCONV-159): `GET`/`PATCH /api/users/me/auto-pm/` and
/// `PATCH /api/users/me/auto-pm/jobs/{slug}/`; every other method on
/// those paths proxies to Django.
///
/// The Prompting group serves the D-04 prompt-section surface
/// (`prompting`, PIDASHCONV-158): the four owned paths (section list,
/// section detail PUT/DELETE, compiled, preview) with their owned
/// methods; every other method on those paths proxies to Django.
///
/// The Assistant group serves the D-06 thread surface (`assistant`,
/// PIDASHCONV-255): thread list/create/detail, message list/create,
/// cancel, and the SSE event stream, with their owned methods — plus
/// the handlers-C surface (PIDASHCONV-257): transcribe POST, the
/// desktop agent-profile GET / agent-token POST pair, and the MCP
/// server list/create + detail PATCH/DELETE; sibling `ai-assistant/`
/// paths have no Rust route and keep proxying to Django through the
/// fallback. Sibling handler issues merge their routers in
/// `assistant::routes`; merges keep both sides.
pub(crate) fn oss_group_routes(group: RouteGroup) -> Router<AppState> {
    match group {
        RouteGroup::Web => Router::new()
            .route("/", any(web::health_check))
            .route("/robots.txt", any(web::robots_txt)),
        // App handlers merge their routers here (D-26 issue lists in
        // `app_issues`; D-29 search (PIDASHCONV-276) + views/favorites
        // (PIDASHCONV-275), both in `app_views_search`;
        // D-32 intake-issues/inbox-issues list+create,
        // PIDASHCONV-385, plus intake-issue detail + versions,
        // PIDASHCONV-395, in `app_intake`; D-33 GitHub App install flow
        // (PIDASHCONV-443) + workspace GitHub handlers (PIDASHCONV-446)
        // + project-github (PIDASHCONV-450) + git project-repository
        // (PIDASHCONV-452) + external LLM/Unsplash (PIDASHCONV-454)
        // + git provider-accounts (PIDASHCONV-451) + webhooks
        // (PIDASHCONV-453), all in `app_integrations`; D-31 v2
        // user/workspace/static/restore assets (PIDASHCONV-400) + v2
        // project/bulk/check/duplicate/downloads (PIDASHCONV-412) in
        // `app_assets`; D-34 notification viewset core
        // (PIDASHCONV-301) in `app_notifications`; D-27 archived
        // cycles + archive/unarchive (PIDASHCONV-377) + CycleViewSet
        // CRUD (PIDASHCONV-321) + date-check + transfer +
        // user-properties (PIDASHCONV-357) in `app_cycles`; D-28
        // module CRUD (PIDASHCONV-391) in `app_modules` (siblings
        // 397/407 extend that merge, keeping both sides); D-35
        // analytics + analytic-view viewset (PIDASHCONV-389) +
        // workspace advance analytics (PIDASHCONV-414) + analytic
        // handlers-B — saved-analytic-view, export-analytics,
        // default-analytics, project-stats (PIDASHCONV-399; gates:
        // PIDASHCONV-358) + export-issues GET + POST
        // (PIDASHCONV-430) in `app_analytics`; D-30 page favorites +
        // description (PIDASHCONV-332) + state ops (PIDASHCONV-328)
        // in `app_pages`; D-36 scheduler definitions (PIDASHCONV-633) +
        // scheduler bindings (PIDASHCONV-634) + occurrences
        // (PIDASHCONV-635) in `app_scheduler`; D-24 invitations + join +
        // join-requests (PIDASHCONV-617) + accounts/profile/graphs/
        // dashboard (PIDASHCONV-620) in `app_workspace`
        // (siblings 615-619/621-624 extend that module's merge,
        // keeping both sides); D-25 project core (PIDASHCONV-571) +
        // members (PIDASHCONV-572) + invites/favorites/boards
        // (PIDASHCONV-573) + states/estimates (PIDASHCONV-574) in
        // `app_project` (cutover: PIDASHCONV-575); the D-23 OpenAPI schema trio
        // (`v1_openapi`, PIDASHCONV-535): `/api/schema/`,
        // `/api/schema/swagger-ui/`, `/api/schema/redoc/` plus the
        // slashless 301 — registered on the first tied `api/` row and
        // gated per request on any of the four flips (see
        // `v1_openapi::rust_serves`); sibling handler issues
        // extend the merge; merges keep both sides).
        RouteGroup::App => crate::app_issues::routes()
            .merge(crate::app_views_search::routes())
            .merge(crate::app_intake::routes())
            .merge(crate::app_integrations::routes())
            .merge(crate::app_notifications::routes())
            .merge(crate::app_cycles::routes())
            .merge(crate::app_modules::routes())
            .merge(crate::app_assets::routes())
            .merge(crate::app_analytics::routes())
            .merge(crate::app_pages::routes())
            .merge(crate::app_scheduler::routes())
            .merge(crate::app_workspace::routes())
            .merge(crate::app_project::routes())
            .merge(crate::v1_openapi::routes()),
        RouteGroup::License => crate::license::routes(),
        // Space handlers merge their routers here (intake: PIDASHCONV-177;
        // sibling handler issues extend the merge; merges keep both sides).
        RouteGroup::Space => crate::space::routes(),
        RouteGroup::Loop => crate::r#loop::routes(),
        // Prompting handlers merge their router here (PIDASHCONV-158);
        // sibling handler issues extend the merge; merges keep both sides.
        RouteGroup::Prompting => crate::prompting::routes(),
        // Assistant handlers merge their router here (threads/messages/
        // cancel/SSE: PIDASHCONV-255; LLM/STT config + title:
        // PIDASHCONV-256; sibling handler issues extend the
        // merge; merges keep both sides).
        RouteGroup::Assistant => crate::assistant::routes(),
        // ApiV1 handlers merge their routers here (device start/approve/
        // token: PIDASHCONV-342; workspaces/machine-token/revoke:
        // PIDASHCONV-343; D-19 project routes, PIDASHCONV-369, plus
        // member/invite/user, PIDASHCONV-371, plus states/estimates,
        // PIDASHCONV-372, via `v1_projects::routes`; D-20 cycle routes,
        // PIDASHCONV-362, plus module routes, PIDASHCONV-406, via
        // `v1_cycles_modules::routes`; D-22 runner delete,
        // PIDASHCONV-538, via `v1_cli_auth::routes`; D-18 link/comment
        // routes, PIDASHCONV-674, plus PR/review-link routes,
        // PIDASHCONV-680, via `v1_work_items::routes`; D-21
        // assets/stickies/intake, PIDASHCONV-426, via
        // `v1_assets::routes`; sibling handler issues extend the merge;
        // merges keep both sides;
        // `auth_oauth::routes` already covers the device flow.
        // D-18 work-item action routes, PIDASHCONV-678, via
        // `v1_work_items::routes` (sibling handler issues extend that
        // module's merge, keeping both sides).
        // Registration is the cutover granularity — sibling paths have no
        // Rust route and keep proxying to Django through the fallback.
        RouteGroup::ApiV1 => crate::auth_oauth::routes()
            .merge(crate::v1_projects::routes())
            .merge(crate::v1_cycles_modules::routes())
            .merge(crate::v1_cli_auth::routes())
            .merge(crate::v1_work_items::routes())
            .merge(crate::v1_assets::routes()),
        // Auth handlers merge their routers here (D-17 Gitea OAuth
        // initiate/callback, PIDASHCONV-341; GitHub OAuth initiate/callback,
        // PIDASHCONV-336; GitLab OAuth initiate/callback, PIDASHCONV-339;
        // Google OAuth app + space initiate/callback, PIDASHCONV-335;
        // email sessions, PIDASHCONV-422; magic-link generate /
        // sign-in / sign-up, app + space, PIDASHCONV-431; D-16
        // password/CSRF closure, PIDASHCONV-434;
        // merges keep both sides).
        // Registration is the cutover granularity — sibling `auth/`
        // paths have no Rust route and keep proxying to Django.
        RouteGroup::Auth => crate::auth_oauth::oauth_gitea::routes()
            .merge(crate::auth_oauth::oauth_github::routes())
            .merge(crate::auth_oauth::oauth_gitlab::routes())
            .merge(crate::auth_oauth::oauth_google::routes())
            .merge(crate::auth_session::routes()),
        // Runner handlers merge their routers here (D-15 L7 runs +
        // approvals + metrics, PIDASHCONV-542; L8 run endpoints + chat
        // web/daemon/SSE, PIDASHCONV-543; D-13 web runners/machines/pods,
        // PIDASHCONV-591; D-13 daemon enrollment, PIDASHCONV-590;
        // D-13 deletes + machine commands (web), PIDASHCONV-593 (its
        // daemon result route merges under `RouteGroup::Runner` below);
        // D-13 desktop enroll + project lists, PIDASHCONV-595; D-14
        // machine sessions, PIDASHCONV-559; D-13 refresh + revokes,
        // PIDASHCONV-592; sibling handler issues extend the merge;
        // merges keep both sides).
        // Registration is the cutover granularity — sibling paths have
        // no Rust route and keep proxying to Django through the
        // fallback.
        RouteGroup::RunnerWeb => crate::runner_runs::routes()
            .merge(crate::runner_runs::chat::web_routes())
            .merge(crate::runner_enroll::manage::routes())
            .merge(crate::runner_enroll::enroll::web_routes())
            .merge(crate::runner_enroll::delete_cmds::web_routes())
            .merge(crate::runner_enroll::teardown::web_routes())
            .merge(crate::runner_enroll::projects::web_routes()),
        RouteGroup::Runner => crate::runner_runs::run_endpoints::routes()
            .merge(crate::runner_runs::chat::daemon_routes())
            .merge(crate::runner_runs::daemon_routes())
            .merge(crate::runner_enroll::teardown::daemon_routes())
            .merge(crate::runner_sessions::routes())
            .merge(crate::runner_enroll::enroll::daemon_routes())
            .merge(crate::runner_enroll::delete_cmds::daemon_routes())
            .merge(crate::runner_enroll::desktop::routes())
            .merge(crate::runner_enroll::projects::daemon_routes()),
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
    let mut oss = Router::new().route("/healthz", get(healthz)).route(
        crate::edge_shadow::METRICS_PATH,
        get(crate::edge_shadow::metrics),
    );
    for group in RouteGroup::ALL {
        oss = oss.merge(match overlay.replaced(group) {
            Some(replacement) => replacement,
            None => oss_group_routes(group),
        });
    }
    // Shadow gate (PIDASHCONV-822): a second app over the same groups and
    // extras under a flags-on shadow state, installed into the real state
    // before it is shared with the handlers. Pure router construction, no
    // I/O; when shadow is disabled the gate only counts nothing.
    let shadow_config = state
        .shadow_config()
        .cloned()
        .unwrap_or_else(crate::edge_shadow::ShadowConfig::from_env);
    let shadow_state = state.with_edge_replaced(state.edge().shadow_side());
    let mut shadow_groups = Router::new();
    for group in RouteGroup::ALL {
        shadow_groups = shadow_groups.merge(match overlay.replaced(group) {
            Some(replacement) => replacement,
            None => oss_group_routes(group),
        });
    }
    let shadow_app =
        crate::edge_shadow::build_shadow_app(&shadow_state, shadow_groups, overlay.extra.clone());
    let state = state.with_shadow_gate(std::sync::Arc::new(crate::edge_shadow::ShadowGate::new(
        shadow_config,
        shadow_app,
    )));
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
    async fn app_group_mounts_project_routes() {
        // D-25 cutover (PIDASHCONV-575): the App group merges
        // `app_project::routes()`, so an owned project path resolves to a
        // Rust handler instead of falling through to the Django proxy
        // (502 fail-closed here — the silent-fallback regression this pins:
        // unmounted, the contract suite stays green against Django while
        // proving nothing about Rust).
        let app = build_router_with_overlay(test_state(), Overlay::new());
        let response = app
            .oneshot(
                axum::http::Request::get("/api/workspaces/nope/projects/")
                    .body(axum::body::Body::empty())
                    .expect("request"),
            )
            .await
            .expect("serve");
        assert_ne!(response.status(), StatusCode::BAD_GATEWAY);
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
