//! Member gate + owned-thread scope for the assistant surface (D-06, stage 5).
//!
//! Ports `apps/api/pi_dash/assistant/views/_base.py:1-34`
//! (`role_error_response`, `AssistantBaseView.require_member`,
//! `AssistantBaseView.owned_thread`) and the SSE resolve matrix in
//! `apps/api/pi_dash/assistant/views/events.py:28-41`. Fixture id F-A6-07
//! (`rust-api/fixtures/assistant/perms.json`).
//!
//! Shape of the port: the role itself crosses as [`Option<i32>`] — the value
//! `workspace_role_by_slug` (`pi_dash/core/permissions.py:48-56`) returns for
//! the caller's active `WorkspaceMember` row (`None` = no row, which is also
//! what anonymous callers resolve to). Fetching stays with the handlers; this
//! module only compares (`role is None or role < ROLE_MEMBER -> 403`,
//! `_base.py:25-29`) and renders the exact denial bytes. Role constants come
//! from [`pidash_auth::permissions`](pidash_auth::permissions), the same
//! single source of truth the Python side shares.
//!
//! Gate order (preserved, not redesigned): DRF `initial()` runs authentication
//! and `check_throttles` *before* the handler body calls `require_member`, so
//! a throttled guest answers 429 ([`crate::assistant::throttles`]), never 403.
//! The 403 below fires only once the request survives throttling.
//!
//! Response shapes:
//!
//! * Workspace routes deny with `role_error_response` (`_base.py:14-19`):
//!   403 + `{"error":"role_not_allowed","detail":"The assistant is available
//!   to workspace members."}` (compact DRF rendering, key order preserved).
//! * A thread scope that matches nothing answers 404
//!   `{"error":"not_found"}` on every thread-scoped endpoint
//!   (`messages.py:50,69,121`, `threads.py:73,89,103`).
//! * The SSE stream is a plain Django view, not DRF: `_resolve` failures
//!   answer with *empty* bodies — 401 when unauthenticated, 404 when the role
//!   is below member or the thread scope matches nothing (`events.py:62-66`).
//!   A guest therefore sees 404 on the stream but 403 on the REST routes;
//!   both spellings are ported as written.

use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use pidash_auth::permissions::ROLE_MEMBER;
use pidash_db::assistant::models::assistant_thread;

/// Exact bytes of `role_error_response` (`_base.py:14-19`): compact DRF JSON,
/// key order `error`, `detail`. Rendered from the live module in the
/// PIDASHCONV-249 run (`json.dumps(..., separators=(",", ":"),
/// ensure_ascii=False)`).
pub const ROLE_NOT_ALLOWED_BODY: &str =
    r#"{"error":"role_not_allowed","detail":"The assistant is available to workspace members."}"#;

/// Exact bytes of the thread-scope miss body (`{"error": "not_found"}`,
/// 404 — same compact rendering).
pub const THREAD_NOT_FOUND_BODY: &str = r#"{"error":"not_found"}"#;

/// `Content-Type` Django sends with the empty SSE denials: a bare
/// `HttpResponse(status=...)` defaults to `settings.DEFAULT_CONTENT_TYPE`
/// (`"text/html"`) plus `"; charset=utf-8"`. There is no JSON body.
pub const EMPTY_DENIAL_CONTENT_TYPE: &str = "text/html; charset=utf-8";

fn json_forbidden(body: &'static str) -> Response {
    Response::builder()
        .status(StatusCode::FORBIDDEN)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("static role-denied response")
}

fn json_not_found(body: &'static str) -> Response {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body))
        .expect("static thread-not-found response")
}

fn empty_with_status(status: StatusCode) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, EMPTY_DENIAL_CONTENT_TYPE)
        .body(axum::body::Body::empty())
        .expect("static empty denial response")
}

/// Rejection for a failed member gate: answers the exact 403
/// `role_not_allowed` body.
#[derive(Debug, Clone, Copy, Default)]
pub struct RoleNotAllowed;

impl IntoResponse for RoleNotAllowed {
    fn into_response(self) -> Response {
        json_forbidden(ROLE_NOT_ALLOWED_BODY)
    }
}

/// Rejection for a thread scope that matches nothing: answers the exact 404
/// `not_found` body.
#[derive(Debug, Clone, Copy, Default)]
pub struct ThreadNotFound;

impl IntoResponse for ThreadNotFound {
    fn into_response(self) -> Response {
        json_not_found(THREAD_NOT_FOUND_BODY)
    }
}

/// Mirror of `AssistantBaseView.require_member` (`_base.py:25-29`): the active
/// workspace role passes iff it is `>= MEMBER` (15) — guests (5), strangers
/// (`None`), and anonymous callers (`None`) are denied with [`RoleNotAllowed`].
pub fn require_member(role: Option<i32>) -> Result<(), RoleNotAllowed> {
    match role {
        Some(role) if role >= ROLE_MEMBER => Ok(()),
        _ => Err(RoleNotAllowed),
    }
}

/// `workspaces` table name for the scope SQL below. No shared const exists
/// (the license handlers inline the same literal); the name is pinned by the
/// F-A6-06 fixture SQL for this exact query.
pub const WORKSPACES_TABLE: &str = "workspaces";
/// `workspaces.slug` column for the scope SQL below (same fixture pin).
pub const WORKSPACES_SLUG_COLUMN: &str = "slug";

/// Mirror of `AssistantBaseView.owned_thread` (`_base.py:31-34`):
/// `.filter(id=thread_id, user=request.user, workspace__slug=slug).first()`.
///
/// Returns the compiler-form SQL (Django debug-interpolation style, exactly
/// what F-A6-06 `owned_thread` records): the `assistant_thread` column list
/// in `_meta` field order (taken from [`assistant_thread::COLUMNS`] so the
/// two layers cannot drift), `INNER JOIN "workspaces"`, the three scope
/// predicates, the model's default `ORDER BY "updated_at" DESC`
/// (`Meta.ordering = ("-updated_at",)`, `models.py:55`), and `LIMIT 1` (what
/// `.first()` compiles to). Production executors must bind parameters
/// instead of interpolating; the `slug` single-quote doubling here exists
/// only so the comparison form stays total.
pub fn owned_thread_sql(thread_id: &str, user_id: &str, slug: &str) -> String {
    let columns = assistant_thread::COLUMNS
        .iter()
        .map(|column| format!("\"{}\".\"{}\"", assistant_thread::TABLE, column))
        .collect::<Vec<_>>()
        .join(", ");
    let slug_escaped = slug.replace('\'', "''");
    format!(
        "SELECT {columns} FROM \"{thread_table}\" \
         INNER JOIN \"{ws_table}\" ON (\"{thread_table}\".\"workspace_id\" = \"{ws_table}\".\"id\") \
         WHERE (\"{thread_table}\".\"id\" = {thread_id} \
         AND \"{thread_table}\".\"user_id\" = {user_id} \
         AND \"{ws_table}\".\"{ws_slug}\" = '{slug_escaped}') \
         ORDER BY \"{thread_table}\".\"updated_at\" DESC LIMIT 1",
        thread_table = assistant_thread::TABLE,
        ws_table = WORKSPACES_TABLE,
        ws_slug = WORKSPACES_SLUG_COLUMN,
    )
}

/// Decision of the SSE `_resolve` helper (`events.py:28-41`): authentication,
/// then the member gate, then the owned-thread scope. `thread_found` is
/// whether the [`owned_thread_sql`] scope matched a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SseResolve {
    /// Caller (or its thread scope) resolves: open the stream.
    Open,
    /// `user is None or not user.is_authenticated` (`events.py:30-31`):
    /// the stream answers 401 with an empty body.
    Unauthenticated,
    /// Role below member *or* no thread in scope (`events.py:32-38`): the
    /// stream answers 404 with an empty body. Note the deliberate asymmetry
    /// with the REST routes — a guest reads 404 here, 403 there.
    NoThread,
}

/// Mirror of `_resolve` (`events.py:28-41`) as a pure decision: `authenticated`
/// is `user.is_authenticated`, `role` is the `workspace_role_by_slug` value,
/// `thread_found` is whether the owned-thread scope matched.
pub fn resolve_sse(authenticated: bool, role: Option<i32>, thread_found: bool) -> SseResolve {
    if !authenticated {
        return SseResolve::Unauthenticated;
    }
    if require_member(role).is_err() || !thread_found {
        return SseResolve::NoThread;
    }
    SseResolve::Open
}

/// Rejection for [`SseResolve::Unauthenticated`]: empty 401, Django
/// `HttpResponse(status=401)` spelling.
#[derive(Debug, Clone, Copy, Default)]
pub struct SseUnauthenticated;

impl IntoResponse for SseUnauthenticated {
    fn into_response(self) -> Response {
        empty_with_status(StatusCode::UNAUTHORIZED)
    }
}

/// Rejection for [`SseResolve::NoThread`]: empty 404, Django
/// `HttpResponse(status=404)` spelling.
#[derive(Debug, Clone, Copy, Default)]
pub struct SseNoThread;

impl IntoResponse for SseNoThread {
    fn into_response(self) -> Response {
        empty_with_status(StatusCode::NOT_FOUND)
    }
}

/// Map an [`SseResolve`] decision to its rejection (`None` opens the stream).
pub fn sse_rejection(decision: SseResolve) -> Option<Response> {
    match decision {
        SseResolve::Open => None,
        SseResolve::Unauthenticated => Some(SseUnauthenticated.into_response()),
        SseResolve::NoThread => Some(SseNoThread.into_response()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    async fn body_of(response: Response) -> (StatusCode, String, Option<String>) {
        let status = response.status();
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok().map(str::to_owned));
        let bytes = to_bytes(response.into_body(), 1024)
            .await
            .expect("read body");
        (
            status,
            String::from_utf8(bytes.to_vec()).expect("utf-8"),
            content_type,
        )
    }

    /// F-A6-07 `require_member.matrix`: guest (5) and non-member (None) deny,
    /// member (15) and admin (20) pass — plus the 14/15 boundary.
    #[test]
    fn member_gate_matrix_matches_fixture() {
        assert!(require_member(None).is_err(), "non-member denies");
        assert!(require_member(Some(5)).is_err(), "guest denies");
        assert!(require_member(Some(14)).is_err(), "below-member denies");
        assert!(require_member(Some(15)).is_ok(), "member passes");
        assert!(require_member(Some(20)).is_ok(), "admin passes");
    }

    #[tokio::test]
    async fn role_denial_is_byte_identical() {
        let (status, body, content_type) = body_of(RoleNotAllowed.into_response()).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body, ROLE_NOT_ALLOWED_BODY);
        assert_eq!(
            body,
            r#"{"error":"role_not_allowed","detail":"The assistant is available to workspace members."}"#
        );
        assert_eq!(content_type.as_deref(), Some("application/json"));
    }

    #[tokio::test]
    async fn thread_miss_is_byte_identical() {
        let (status, body, content_type) = body_of(ThreadNotFound.into_response()).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, THREAD_NOT_FOUND_BODY);
        assert_eq!(body, r#"{"error":"not_found"}"#);
        assert_eq!(content_type.as_deref(), Some("application/json"));
    }

    /// F-A6-07 `owned_thread.filter`: the three scope predicates over the
    /// join the fixture SQL records, with the model's column list, default
    /// ordering, and `.first()` limit.
    #[test]
    fn owned_thread_sql_matches_fixture_shape() {
        let sql = owned_thread_sql(
            "db68f428-df63-4de8-b060-2d4038a5b1f4",
            "a7107351-38ac-4f3d-91d5-ee34c2459bd6",
            "acme",
        );
        assert_eq!(
            sql,
            "SELECT \"assistant_thread\".\"id\", \"assistant_thread\".\"workspace_id\", \
             \"assistant_thread\".\"user_id\", \"assistant_thread\".\"title\", \
             \"assistant_thread\".\"kind\", \"assistant_thread\".\"is_archived\", \
             \"assistant_thread\".\"active_turn_id\", \"assistant_thread\".\"created_at\", \
             \"assistant_thread\".\"updated_at\" \
             FROM \"assistant_thread\" \
             INNER JOIN \"workspaces\" ON (\"assistant_thread\".\"workspace_id\" = \"workspaces\".\"id\") \
             WHERE (\"assistant_thread\".\"id\" = db68f428-df63-4de8-b060-2d4038a5b1f4 \
             AND \"assistant_thread\".\"user_id\" = a7107351-38ac-4f3d-91d5-ee34c2459bd6 \
             AND \"workspaces\".\"slug\" = 'acme') \
             ORDER BY \"assistant_thread\".\"updated_at\" DESC LIMIT 1"
        );
    }

    #[test]
    fn owned_thread_sql_escapes_slug_quotes() {
        let sql = owned_thread_sql("t", "u", "o'brien");
        assert!(sql.contains("\"workspaces\".\"slug\" = 'o''brien'"));
    }

    /// F-A6-07 `sse_resolve.matrix`: unauthenticated -> 401; guest/non-member
    /// -> 404 (thread None); member with another user's thread -> 404.
    #[test]
    fn sse_resolve_matrix_matches_fixture() {
        assert_eq!(resolve_sse(false, None, false), SseResolve::Unauthenticated);
        assert_eq!(
            resolve_sse(true, Some(5), true),
            SseResolve::NoThread,
            "guest resolves to thread None"
        );
        assert_eq!(
            resolve_sse(true, None, true),
            SseResolve::NoThread,
            "non-member resolves to thread None"
        );
        assert_eq!(
            resolve_sse(true, Some(15), false),
            SseResolve::NoThread,
            "member, other-user thread"
        );
        assert_eq!(resolve_sse(true, Some(15), true), SseResolve::Open);
        assert_eq!(resolve_sse(true, Some(20), true), SseResolve::Open);
    }

    #[tokio::test]
    async fn sse_denials_are_empty_with_django_content_type() {
        let (status, body, content_type) = body_of(SseUnauthenticated.into_response()).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body, "");
        assert_eq!(content_type.as_deref(), Some(EMPTY_DENIAL_CONTENT_TYPE));

        let (status, body, content_type) = body_of(SseNoThread.into_response()).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, "");
        assert_eq!(content_type.as_deref(), Some(EMPTY_DENIAL_CONTENT_TYPE));
    }
}
