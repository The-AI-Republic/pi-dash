//! D-15 runner web handlers: runs, approvals, metrics, signals (stage 5, PIDASHCONV-542).
//!
//! Ports `apps/api/pi_dash/runner/views/runs.py:1-695`,
//! `views/approvals.py:1-105`, `views/metrics.py:1-105` and `signals.py:1-66`
//! (routes `runner/web_urls.py` under `/api/runners/` plus `runner/urls.py`
//! `metrics/` under `/api/v1/runner/`).
//!
//! * [`runs`] — run list / create (`comment_and_run`, `run_ai`, direct),
//!   re-tick, detail (`+include_events`), cancel, release-pin.
//! * [`approvals`] — creator-routed approval list + decide.
//! * [`metrics`] — the five Prometheus gauges (AllowAny, `text/plain`).
//! * [`signals`] — `create_default_pod_for_new_project` as an explicit
//!   post-commit fn (the project-creation owner wires the call; that
//!   wiring belongs to that domain's split, not this issue).
//!
//! Fixture: `rust-api/fixtures/runner_runs/fx-run-08-handlers-web.golden.json`
//! (FX-RUN-08; trace: `rust-api/fixtures/runner_runs/TRACE.md`). Contract:
//! `rust-api/contract-tests/runner/test_web_runs.py`,
//! `test_web_approvals.py`, `test_daemon_health.py` (metrics half).
//!
//! # Layering: reuse, then proxy
//!
//! Everything portable with merged providers is owned here: L2 rows
//! ([`pidash_db::runner_runs`]), L3 guards + shapes
//! ([`pidash_services::runner_runs::guards`] / `shape`), L4 finalization
//! plans ([`pidash_services::runner_runs::finalization`]), the D-13 pod-mini
//! kernel, and the foundation session-auth / membership / runner kernels.
//! Branches that need providers that have not landed (D-13
//! `validate_run_creation`, PIDASHCONV-587; D-14 `send_to_runner`,
//! PIDASHCONV-553, and `drain_pod_by_id`, PIDASHCONV-552; D-12 scheduling)
//! proxy to Django through [`crate::edge::proxy`] — byte-exact by
//! construction, never a stub — following the `app_pages` bad-id
//! precedent for handler-internal proxying. Malformed-JSON bodies also
//! proxy (Django answers the DRF `ParseError`); bad-UUID path segments
//! proxy before auth (Django's `<uuid:>` converter 404s before the view
//! runs). Unowned methods proxy at the route level via [`owned`], so
//! Django's 405-after-auth, metadata and OPTIONS responses are preserved.
//!
//! Request order everywhere (Django's order, preserved): Django-session
//! authN ([`crate::license::resolve_actor`]: 401) before any method,
//! body or branch decision — except the bad-id proxy, which (like URL
//! resolution) runs first. CSRF needs no check: the views use
//! `BaseSessionAuthentication`, whose `enforce_csrf` is a no-op
//! (`authentication/session.py:8-10`).
//!
//! # Ported bugs (translate, don't redesign; also listed in the PR)
//!
//! * Web decide always 500s on Postgres: `select_for_update` over the
//!   nullable `agent_run__runner` join raises `NotSupportedError`
//!   (`approvals.py:58`) before the creator gate, so the 404/409s, the
//!   AWAITING→RUNNING flip and the decide fan-out are unreachable. Valid
//!   decisions proxy so Django exhibits the 500 itself.
//! * Malformed `work_item` UUIDs on create / `run_ai` escape validation
//!   into an unhandled 500 (only re-tick guards the parse). Those arms
//!   proxy, so the 500 is Django's own.
//! * Cancel's fan-out sends the FULL untruncated body reason while the row
//!   stores `[:512]` (`runs.py:560` vs `:633`); the send arms proxy.
//! * `run.runner.owner` grant runs before the private-runner gate
//!   (`runs.py:98-106`) — inherited through the L3 kernel as written.
//!
//! # Documented approximations (no contract input covers them)
//!
//! * `AnonRateThrottle` (30/min) on the AllowAny metrics endpoint is not
//!   ported (the `web` / `space` AllowAny precedent); divergence needs
//!   30+ scrapes/min from one IP.
//! * DB-down 500s render [`SERVER_ERROR_BODY`] (the merged-gate bytes),
//!   not Django's HTML 500 page; unreachable in `serve`, which fail-fasts
//!   pool-less.
//! * `str.strip()` parity is exact (Rust `White_Space` plus U+001C–U+001F);
//!   `uuid.UUID()` parity is exact except single `_` between hex digits,
//!   which CPython's `int(x, 16)` accepts and this port 400s (pathological;
//!   no contract input).
//! * Unknown `status` / `executor_kind` / `trigger` / tool-call `status`
//!   values in the DB (model-violating: Django renders them raw) answer
//!   500 here, since the L3 shapes need typed enums.

pub mod approvals;
pub mod metrics;
pub mod runs;
pub mod signals;

use axum::http::{header, StatusCode};
use axum::response::Response;
use axum::Router;

use crate::state::AppState;

/// Unreachable-in-`serve` 500 body (DB failure on an owned branch): the
/// merged-gate bytes (`license::SERVER_ERROR_BODY`, same text), pinned
/// equal in [`tests::server_error_matches_license_body`].
pub const SERVER_ERROR_BODY: &str = r#"{"error":"Something went wrong please try again later"}"#;

/// Render a JSON success/error body with the DRF content type.
pub fn json_response(status: StatusCode, body: String) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("runner-runs response")
}

/// The request pool, or the 500 when the server runs pool-less
/// (unreachable in `serve`, which fail-fasts at boot).
#[allow(clippy::result_large_err)]
pub fn pool_of(state: &AppState) -> Result<&sqlx::PgPool, Response> {
    match state.pools().map(|pools| pools.primary()) {
        Some(pool) => Ok(pool),
        None => Err(json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            SERVER_ERROR_BODY.to_owned(),
        )),
    }
}

/// Whether `raw` matches a Django `<uuid:>` path segment: the converter
/// regex `[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}`
/// (`django/urls/converters.py:25-26`) — lowercase hex only, so an
/// uppercase UUID matches NO route and Django's `custom_404_view`
/// answers before any view code runs. Handlers proxy non-matching
/// segments to Django (the `app_pages` bad-id precedent), reproducing
/// that 404 byte for byte.
pub fn is_uuid_path_segment(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    for (index, byte) in bytes.iter().enumerate() {
        let hyphen = matches!(index, 8 | 13 | 18 | 23);
        if hyphen {
            if *byte != b'-' {
                return false;
            }
        } else if !byte.is_ascii_hexdigit() || byte.is_ascii_uppercase() {
            return false;
        }
    }
    true
}

/// Proxy `parts` + already-collected `body_bytes` to Django: the
/// parse-then-proxy path for branches this port does not own (dispatch
/// tails, provider sends, malformed JSON). The bytes go back untouched,
/// so headers (`content-length` included) stay valid.
pub async fn proxy_with_body(
    state: AppState,
    parts: http::request::Parts,
    body_bytes: bytes::Bytes,
) -> Response {
    let request = axum::http::Request::from_parts(parts, axum::body::Body::from(body_bytes));
    crate::edge::proxy(axum::extract::State(state), request).await
}

/// An owned path: the owned methods serve from Rust, every other method
/// falls through to Django (its 405-after-auth, metadata, and OPTIONS
/// responses live there — answering 405 in Rust would mistranslate the
/// body). HEAD proxies explicitly: axum would otherwise auto-serve it
/// from the GET handler with the body stripped, while Django's views
/// define no `head` and answer 405-after-auth (the `v1_cycles_modules`
/// precedent). The [`crate::app_pages`] precedent for the rest.
fn owned(
    handler: axum::routing::MethodRouter<AppState>,
    unowned: &[&str],
) -> axum::routing::MethodRouter<AppState> {
    let mut router = handler;
    for method in unowned {
        router = match *method {
            "POST" => router.post(crate::edge::proxy),
            "PUT" => router.put(crate::edge::proxy),
            "PATCH" => router.patch(crate::edge::proxy),
            "DELETE" => router.delete(crate::edge::proxy),
            "HEAD" => router.head(crate::edge::proxy),
            "OPTIONS" => router.options(crate::edge::proxy),
            _ => router.get(crate::edge::proxy),
        };
    }
    router
}

/// Register the runner web routes (`runner/web_urls.py`, `RouteGroup::RunnerWeb`):
/// runs list/create, re-tick, run detail/cancel/release-pin, approvals
/// list/decide. Sibling D-15 handler issues (L8 web chat) extend this via
/// their own routers merged in `overlay.rs`; merges keep both sides.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/runners/runs/",
            owned(
                axum::routing::get(runs::list_runs).post(runs::create_run),
                &["PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"],
            ),
        )
        .route(
            "/api/runners/re-tick/",
            owned(
                axum::routing::post(runs::retick),
                &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"],
            ),
        )
        .route(
            "/api/runners/runs/{run_id}/",
            owned(
                axum::routing::get(runs::run_detail),
                &["POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"],
            ),
        )
        .route(
            "/api/runners/runs/{run_id}/cancel/",
            owned(
                axum::routing::post(runs::cancel_run),
                &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"],
            ),
        )
        .route(
            "/api/runners/runs/{run_id}/release-pin/",
            owned(
                axum::routing::post(runs::release_pin),
                &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"],
            ),
        )
        .route(
            "/api/runners/approvals/",
            owned(
                axum::routing::get(approvals::list_approvals),
                &["POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"],
            ),
        )
        .route(
            "/api/runners/approvals/{approval_id}/decide/",
            owned(
                axum::routing::post(approvals::decide_approval),
                &["GET", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"],
            ),
        )
}

/// Register the runner daemon routes this issue owns (`runner/urls.py`,
/// `RouteGroup::Runner`): metrics only — `HealthEndpoint` lives in
/// `views/register.py` (D-13) and the run/chat endpoints belong to L8
/// (PIDASHCONV-543), which extends the group merge; merges keep both sides.
pub fn daemon_routes() -> Router<AppState> {
    Router::new().route(
        "/api/v1/runner/metrics/",
        owned(
            axum::routing::get(metrics::metrics),
            &["POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"],
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;
    use tower::ServiceExt;

    #[test]
    fn server_error_matches_license_body() {
        assert_eq!(SERVER_ERROR_BODY, crate::license::SERVER_ERROR_BODY);
    }

    #[test]
    fn uuid_path_segment_matches_django_converter() {
        // Canonical lowercase: owned.
        assert!(is_uuid_path_segment("8dbd5acc-b3ce-4259-91b2-570768a0b918"));
        // Uppercase hex: Django's `[0-9a-f]` regex never matches → proxy.
        assert!(!is_uuid_path_segment(
            "8DBD5ACC-B3CE-4259-91B2-570768A0B918"
        ));
        assert!(!is_uuid_path_segment("nope"));
        assert!(!is_uuid_path_segment(""));
        assert!(!is_uuid_path_segment("8dbd5accb3ce425991b2570768a0b918"));
        assert!(!is_uuid_path_segment(
            "{8dbd5acc-b3ce-4259-91b2-570768a0b918}"
        ));
        assert!(!is_uuid_path_segment("8dbd5acc-b3ce-4259-91b2-570768a0b91"));
        assert!(!is_uuid_path_segment(
            "8dbd5acc-b3ce-4259-91b2-570768a0b918 "
        ));
    }

    /// The route table: every owned method reaches its handler (pool-less
    /// state answers the handler 500) while unowned methods and bad-id
    /// segments proxy (no upstream on port 1 → 502 `bad_gateway`).
    /// Paths mirror `runner/web_urls.py` + `runner/urls.py` `metrics/`.
    #[tokio::test]
    async fn route_table_matches_django_urls() {
        use crate::edge::EdgeHandle;
        let app = crate::routes::build_router(AppState::with_edge(
            "0.1.0",
            EdgeHandle::for_tests("http://127.0.0.1:1"),
        ));
        let run = "8dbd5acc-b3ce-4259-91b2-570768a0b918";
        // (method, path, body) → expected status.
        let cases: &[(&str, String, u16)] = &[
            ("GET", "/api/runners/runs/".to_owned(), 500),
            ("HEAD", "/api/runners/runs/".to_owned(), 502),
            ("POST", "/api/runners/runs/".to_owned(), 500),
            ("PUT", "/api/runners/runs/".to_owned(), 502),
            ("PATCH", "/api/runners/runs/".to_owned(), 502),
            ("DELETE", "/api/runners/runs/".to_owned(), 502),
            ("OPTIONS", "/api/runners/runs/".to_owned(), 502),
            ("POST", "/api/runners/re-tick/".to_owned(), 500),
            ("GET", "/api/runners/re-tick/".to_owned(), 502),
            ("HEAD", "/api/runners/re-tick/".to_owned(), 502),
            ("GET", format!("/api/runners/runs/{run}/"), 500),
            ("HEAD", format!("/api/runners/runs/{run}/"), 502),
            ("GET", "/api/runners/runs/nope/".to_owned(), 502),
            (
                "GET",
                format!("/api/runners/runs/{}/", run.to_uppercase()),
                502,
            ),
            ("POST", format!("/api/runners/runs/{run}/"), 502),
            ("POST", format!("/api/runners/runs/{run}/cancel/"), 500),
            ("GET", format!("/api/runners/runs/{run}/cancel/"), 502),
            ("HEAD", format!("/api/runners/runs/{run}/cancel/"), 502),
            ("POST", "/api/runners/runs/nope/cancel/".to_owned(), 502),
            ("POST", format!("/api/runners/runs/{run}/release-pin/"), 500),
            ("GET", format!("/api/runners/runs/{run}/release-pin/"), 502),
            ("HEAD", format!("/api/runners/runs/{run}/release-pin/"), 502),
            ("GET", "/api/runners/approvals/".to_owned(), 500),
            ("HEAD", "/api/runners/approvals/".to_owned(), 502),
            ("POST", "/api/runners/approvals/".to_owned(), 502),
            ("POST", format!("/api/runners/approvals/{run}/decide/"), 500),
            ("GET", format!("/api/runners/approvals/{run}/decide/"), 502),
            ("HEAD", format!("/api/runners/approvals/{run}/decide/"), 502),
            (
                "POST",
                "/api/runners/approvals/nope/decide/".to_owned(),
                502,
            ),
            ("GET", "/api/v1/runner/metrics/".to_owned(), 500),
            ("HEAD", "/api/v1/runner/metrics/".to_owned(), 502),
            ("POST", "/api/v1/runner/metrics/".to_owned(), 502),
        ];
        for (method, path, want) in cases {
            let request = Request::builder()
                .method(*method)
                .uri(path.as_str())
                .header(header::CONTENT_TYPE, "application/json")
                .body(axum::body::Body::from("{}"))
                .expect("request");
            let response = app.clone().oneshot(request).await.expect("serve");
            assert_eq!(response.status().as_u16(), *want, "{method} {path}");
        }
    }
}
