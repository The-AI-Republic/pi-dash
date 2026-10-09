//! Runner + machine session handlers (D-14, stage 5, PIDASHCONV-559).
//!
//! Ports `apps/api/pi_dash/runner/views/machine_sessions.py:54-352`
//! (machine open / delete / long-poll) to axum. This module owns the
//! final `routes()` + [`owned()`] assembly for all six D-14 session
//! routes; the sibling handler files merge their routers here when
//! they land (merges keep both sides, never drop a route).
//!
//! | Django path | Owner |
//! | --- | --- |
//! | `POST runners/<uuid>/sessions/` | sibling `runner_open` (PIDASHCONV-557, Backlog) |
//! | `DELETE runners/<uuid>/sessions/<uuid>/` | sibling `runner_open` (PIDASHCONV-557, Backlog) |
//! | `POST runners/<uuid>/sessions/<uuid>/poll` | [`runner_poll`] (PIDASHCONV-558) |
//! | `POST dev-machines/<uuid>/sessions/` | [`machine`] (PIDASHCONV-559) |
//! | `DELETE dev-machines/<uuid>/sessions/<uuid>/` | [`machine`] (PIDASHCONV-559) |
//! | `POST dev-machines/<uuid>/sessions/<uuid>/poll` | [`machine`] (PIDASHCONV-559) |
//!
//! # Reuse, not forks
//!
//! * Auth: D-13 [`crate::runner_enroll::auth::authenticate_machine_token`]
//!   (PIDASHCONV-589); the DRF 401 passes through byte-identical, the
//!   poll 401 is re-rendered spaced/challenge-free to match the
//!   hand-built `JsonResponse` (see [`machine`] docs).
//! * SQL text: [`pidash_db::runner_sessions::models::machine_session`]
//!   consts; the `DevMachine.last_seen_at` keyed update is spelled in
//!   [`machine`] (no D-13 const exists; Django
//!   `.filter(pk=…).update(…)` shape).
//! * Redis verbs: [`pidash_db::runner_sessions::machine_outbox`]; the
//!   eviction `SUBSCRIBE` rides the foundation [`pidash_db::redis::RedisHandle`]
//!   (`subscribe` + `next_payload`, the `runner_runs::sse` precedent).
//! * Bodies: [`pidash_types::runner_sessions`] response builders
//!   (DRF-compact for the endpoints, `JsonResponse`-spaced for the
//!   poll); shared [`crate::runner_runs`] response helpers.
//!
//! # Ported bugs (translate, don't redesign; also listed in the PR)
//!
//! * BUG-machine-open-no-bound-txn-waits (`machine_sessions.py:89-110`):
//!   unlike the runner open, the machine-open transaction runs with no
//!   lock-timeout guard and maps nothing to 503 — a wedged lock is a
//!   plain 500. Kept as-is.
//! * BUG-machine-open-unguarded-side-effects (`:112-119`): the post-tx
//!   marker clear, eviction publish, and offline drain are unguarded —
//!   a Redis failure 500s after the row committed. The runner twin
//!   swallows each; the asymmetry is kept.
//! * BUG-poll-nondict-ignored (`:326-327`): a well-formed non-dict JSON
//!   body (`[]`, `4`, `"x"`, `NaN`) is silently treated as `{}`.
//! * BUG-poll-fraction-omitted: `server_time` drops the fractional part
//!   when the microsecond is 0 (plain `.isoformat()`); matched with
//!   `AutoSi`.
//!
//! Fixture: `rust-api/fixtures/runner_sessions/fx-rses-02-shapes.json`
//! (FX-RSES-02, machine sections); the `#[cfg(test)]` suites replay the
//! handler-owned branches (ack parsing, plan, slices, the poll 401
//! re-render, route registration).

pub mod machine;
pub mod runner_poll;

use axum::Router;

use crate::state::AppState;

/// A session path: the owned methods serve from Rust, everything else
/// falls through to Django (its dispatch-405 and metadata responses
/// live there). The `app_intake::owned` precedent, restated per domain.
pub fn owned(
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
            "OPTIONS" => router.options(crate::edge::proxy),
            _ => router.get(crate::edge::proxy),
        };
    }
    router
}

/// Register the owned session routes (sibling handler issues
/// merge their own routers here; merges keep both sides).
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/runner/runners/{runner_id}/sessions/{sid}/poll",
            owned(
                axum::routing::post(runner_poll::runner_session_poll),
                &["GET", "PUT", "PATCH", "DELETE", "OPTIONS"],
            ),
        )
        .route(
            "/api/v1/runner/dev-machines/{dev_machine_id}/sessions/",
            owned(
                axum::routing::post(machine::machine_session_open),
                &["GET", "PUT", "PATCH", "DELETE", "OPTIONS"],
            ),
        )
        .route(
            "/api/v1/runner/dev-machines/{dev_machine_id}/sessions/{sid}/",
            owned(
                axum::routing::delete(machine::machine_session_delete),
                &["GET", "POST", "PUT", "PATCH", "OPTIONS"],
            ),
        )
        .route(
            "/api/v1/runner/dev-machines/{dev_machine_id}/sessions/{sid}/poll",
            owned(
                axum::routing::post(machine::machine_session_poll),
                &["GET", "PUT", "PATCH", "DELETE", "OPTIONS"],
            ),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt as _;

    use crate::edge::EdgeHandle;

    fn test_state() -> AppState {
        AppState::with_edge("0.1.0", EdgeHandle::for_tests("http://127.0.0.1:1"))
    }

    async fn status_for(method: &str, path: &str) -> StatusCode {
        let app = routes().with_state(test_state());
        let response = app
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("oneshot serves");
        response.status()
    }

    const MID: &str = "e985a543-b79a-48a8-bdfe-7e0aaa937914";
    const SID: &str = "399779fa-1d90-4aa0-8a64-501c6ba7ae0b";
    const RID: &str = "88d03d06-e974-463d-a76e-33b773bb7850";

    /// The runner poll path is registered with its owned method
    /// (pool-less state 500s inside the handler, proving the request
    /// reached Rust); unowned methods proxy to Django (502 against
    /// the dead test upstream).
    #[tokio::test]
    async fn routes_register_runner_poll_path() {
        let poll = format!("/api/v1/runner/runners/{RID}/sessions/{SID}/poll");
        assert_eq!(
            status_for("POST", &poll).await,
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(status_for("GET", &poll).await, StatusCode::BAD_GATEWAY);
    }

    /// All three machine paths are registered with their owned methods
    /// (pool-less state 500s inside the handler, proving the request
    /// reached Rust), unowned methods proxy to Django (502 against the
    /// dead test upstream), and anything else 404s.
    #[tokio::test]
    async fn routes_register_all_three_machine_paths() {
        let open = format!("/api/v1/runner/dev-machines/{MID}/sessions/");
        let delete = format!("/api/v1/runner/dev-machines/{MID}/sessions/{SID}/");
        let poll = format!("/api/v1/runner/dev-machines/{MID}/sessions/{SID}/poll");
        assert_eq!(
            status_for("POST", &open).await,
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(status_for("GET", &open).await, StatusCode::BAD_GATEWAY);
        assert_eq!(
            status_for("DELETE", &delete).await,
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(status_for("POST", &delete).await, StatusCode::BAD_GATEWAY);
        assert_eq!(
            status_for("POST", &poll).await,
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(status_for("GET", &poll).await, StatusCode::BAD_GATEWAY);
        assert_eq!(
            status_for("POST", "/api/v1/runner/dev-machines/nope/").await,
            StatusCode::NOT_FOUND
        );
    }
}
